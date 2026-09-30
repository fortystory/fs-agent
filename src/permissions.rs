//! 权限边界（spec §12）：规则、模式、断路器，以及委派链上的继承。
//!
//! 每次工具调用都经过同一个纯函数 [`decide`]。它读策略与一个 [`Call`] —— 工具名、
//! 已解析的路径、以及命令类工具将要跑的 argv —— 并回答那个封闭的三态 [`Decision`]。
//! 它从不读对话文本、从不读环境、从不发问、也从不追加事件。一切交互都发生在 `agent`
//! 循环里、权限门之外：循环通过 [`Asker`] 发问，没有 [`Asker`] 时它把权限门那个忠实的
//! `Ask` 降级成 `Deny`，并把理由记进 `PermissionDecided`。
//!
//! 代数是**一个格、一次合并**：`deny > ask > allow`，不看专指程度，于是规则优先级、
//! 钩子的收紧与子会话继承来的约束全都在同一个序上取上确界。有两样刻意留在这条合并
//! 之外：
//!
//! - **断路器**在任何规则被评估之前就把一个硬 `Deny` 短路掉，所以没有任何 allow、也
//!   没有哪个钩子能翻转它；
//! - **模式的地板**（`readonly` 拒绝每一次非只读调用）任何规则都降不下去，而模式的
//!   *缺省*可以 —— 这正是 `ask` 模式下「总是允许」能起作用的原因。

use std::fmt;
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::events::{Decision, ParticipantId, SpeakerId};
use crate::tools::sandbox;
use crate::tools::{Effect, ToolError};

/// 写入之后绝不自动放行的那些 shell 启动文件：在这里一次静默的写入会改变往后每一个
/// shell，那是家目录级别的破坏。
const SHELL_RC_FILES: &[&str] = &[
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".profile",
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".npmrc",
];

/// 内容就是用户自己那张安全网的目录。写进去直接拒绝，而不只是问一句。
const PROTECTED_DIRS: &[&str] = &[".git", ".ssh"];

/// `.env` 一族的逃生口：那些不携带真密钥、本来就打算提交的文件。
const ENV_TEMPLATE_SUFFIXES: &[&str] = &[".example", ".sample", ".template"];

/// 权限模式，按手势循环它们时的顺序排。
///
/// 四档，而顺序就是循环：`readonly` 最紧、`auto` 最松，按一次 `Shift+Tab` 走到下一档
/// （`.scratch/todo-and-modes` 的 spec §1、`.scratch/workspace-mode` 的 spec §1）。模式是
/// 会话对写的立场、不是计划：`plan` 那一档如今是模型的 `todo` 工具。
///
/// 第四档 `workspace` 按**区内写的严格度**插在 `ask` 之后：对同一次工作区内的写，
/// `readonly` 拒、`ask` 问、`workspace` 允许、`auto` 允许。于是按一次 `Shift+Tab` 仍然是
/// 「松一档」，而区外那一侧的差别落在 `outside_read` 与升级手势上。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// 任何非只读调用一律拒；没有写豁免。
    Readonly,
    /// 写要问、读放行 —— 交互式的默认档。
    Ask,
    /// 工作区内一律放行；区外的写要问一次（区外的读按 `outside_read`）。
    Workspace,
    /// 默认放行。**不是**「跳过权限」：断路器、`.env` 地板与每一条规则仍然有效。
    Auto,
}

/// 一个模式对某种副作用的固定裁决。
///
/// 三个字段刻意绑在一起走：`default` 是没有规则匹配时生效的东西，`floor` 是任何规则都
/// 降不下去的东西，`reason` 是裁决说出口的那句话。一张表让三者保持同步。
struct Stance {
    default: Decision,
    floor: Option<Decision>,
    reason: &'static str,
}

impl Mode {
    /// 模式在线级与 CLI 上的拼写。`config.toml` 的 `[permissions] mode` 写的也是它，
    /// 而且它是唯一能解析成功的拼写。
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Readonly => "readonly",
            Mode::Ask => "ask",
            Mode::Workspace => "workspace",
            Mode::Auto => "auto",
        }
    }

    /// 一个拼写所指的模式；指不出任何模式时是 `None`。
    ///
    /// 两个入口 —— 配置表与 `--mode` 旗标 —— 共用同一个解析器，于是「哪些词是模式」
    /// 不会有第二处答案。
    pub fn parse(word: &str) -> Option<Mode> {
        [Mode::Readonly, Mode::Ask, Mode::Workspace, Mode::Auto]
            .into_iter()
            .find(|mode| mode.as_str() == word)
    }

    /// 循环里的下一档：`readonly → ask → workspace → auto → readonly`。
    ///
    /// 按一次 `Shift+Tab` 走一步。于是按四次就回到会话开始那一档，这也是这个手势是
    /// 「循环」而不是一座有顶的梯子的原因。
    pub fn next(self) -> Mode {
        match self {
            Mode::Readonly => Mode::Ask,
            Mode::Ask => Mode::Workspace,
            Mode::Workspace => Mode::Auto,
            Mode::Auto => Mode::Readonly,
        }
    }

    /// 这个模式对这次调用的立场。只有 `readonly` 有地板，而且它是无条件的：模式拒绝
    /// 的东西，任何规则都不许放行。
    ///
    /// `workspace` 档的判据是**写目标落不落在 cwd 之内**：区内放行、区外问一次。它没有
    /// 地板 —— 那条路径上限在别的三档里仍然是地板（见 [`decide`] 的 ④ 段）。
    fn stance(self, call: &Call<'_>) -> Stance {
        match (self, call.effect) {
            (Mode::Readonly, Effect::ReadOnly) => Stance {
                default: Decision::Allow,
                floor: None,
                reason: "模式 readonly：只读调用放行",
            },
            (Mode::Readonly, _) => Stance {
                default: Decision::Deny,
                floor: Some(Decision::Deny),
                reason: "模式 readonly：非只读调用一律拒绝（换一档才能放行）",
            },
            (Mode::Ask, Effect::ReadOnly) => Stance {
                default: Decision::Allow,
                floor: None,
                reason: "模式 ask：只读调用放行",
            },
            (Mode::Ask, _) => Stance {
                default: Decision::Ask,
                floor: None,
                reason: "模式 ask：写要问用户",
            },
            (Mode::Workspace, Effect::ReadOnly) => Stance {
                default: Decision::Allow,
                floor: None,
                reason: "模式 workspace：只读调用放行（区外读由 `[permissions] outside_read` 管）",
            },
            (Mode::Workspace, Effect::WritePaths(_)) => {
                let inside = call
                    .write_targets
                    .iter()
                    .all(|path| path.starts_with(call.cwd));
                if inside {
                    Stance {
                        default: Decision::Allow,
                        floor: None,
                        reason: "模式 workspace：工作区内的写放行",
                    }
                } else {
                    Stance {
                        default: Decision::Ask,
                        floor: None,
                        reason: "模式 workspace：工作区之外的写要问用户一次",
                    }
                }
            }
            (Mode::Workspace, Effect::Exclusive) => Stance {
                default: Decision::Allow,
                floor: None,
                reason: "模式 workspace：命令放行（区内区外的判据在沙箱那一侧）",
            },
            (Mode::Auto, _) => Stance {
                default: Decision::Allow,
                floor: None,
                reason: "模式 auto：默认放行",
            },
        }
    }
}

impl Default for Mode {
    /// 没有别的说法时，会话就从 `ask` 这一档开始：写要问、读放行。它也是
    /// `[permissions] mode` 的缺省值，而「两边同一个答案」正是这里写一个 impl、而不是
    /// 在每个前端各写一个字面量的原因（spec §12）。
    fn default() -> Self {
        Mode::Ask
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 一条规则适用于谁。`Any` 是常见情况；会传播的 `Deny`/`Ask` 规则就是一个参与者的
/// 约束到达它派出的执行者的途径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Any,
    Debater,
    Executor,
    /// 一个具名的参与者，讨论者或执行者。
    Participant(ParticipantId),
}

impl Subject {
    /// 为一条关于这个 agent 记住的规则（会话级的「总是允许」）给出指名那个发言者的
    /// subject。
    pub fn for_speaker(speaker: &SpeakerId) -> Subject {
        match speaker {
            SpeakerId::Debater(id) | SpeakerId::Executor(id) => Subject::Participant(id.clone()),
            SpeakerId::User | SpeakerId::System => Subject::Any,
        }
    }

    /// 这个 subject 是否覆盖 `speaker`。
    pub fn matches(&self, speaker: &SpeakerId) -> bool {
        match (self, speaker) {
            (Subject::Any, _) => true,
            (Subject::Debater, SpeakerId::Debater(_)) => true,
            (Subject::Executor, SpeakerId::Executor(_)) => true,
            (Subject::Participant(want), SpeakerId::Debater(id) | SpeakerId::Executor(id)) => {
                want == id
            }
            _ => false,
        }
    }

    fn describe(&self) -> String {
        match self {
            Subject::Any => "任何".to_owned(),
            Subject::Debater => "讨论者".to_owned(),
            Subject::Executor => "执行者".to_owned(),
            Subject::Participant(id) => format!("参与者 {id}"),
        }
    }
}

/// 一条规则的作用域：一个作用于调用的谓词，而不是名字匹配器。
///
/// 因为作用域能看到整个调用（它的副作用、它的写集合、它的 argv），像「不是只读
/// **且**写集合正好是这一组」这样的条件可以直接写出来，而不必让一条宽泛的 deny 和一条
/// 狭窄的 allow 在一个这里根本不存在的「专指度」轴上打架。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// 工具名，按 glob 匹配（`edit_file`、`custom__*`）。
    Tool(String),
    /// 一条命令的 argv 前缀（`["git", "status"]`），逐元素比对。
    CommandPrefix(Vec<String>),
    /// 一个路径 glob，相对会话 cwd 匹配（模式以 `/` 开头时按绝对路径），拿这次调用的
    /// 写与读目标去比。
    Path(String),
    /// **写**集合正好是这一组：一条安全的豁免需要的形状，因为一个顺带还写了别的
    /// 东西的调用借不到它。
    PathSet(Vec<PathBuf>),
    /// 每一次调用。
    All,
}

impl Scope {
    /// 这个作用域是否覆盖这次调用。
    pub fn matches(&self, call: &Call<'_>) -> bool {
        match self {
            Scope::Tool(pattern) => glob_match(pattern, call.tool_name),
            Scope::CommandPrefix(prefix) => call
                .argv
                .is_some_and(|argv| argv.starts_with(prefix.as_slice())),
            Scope::Path(pattern) => call
                .write_targets
                .iter()
                .chain(call.read_targets.iter())
                .any(|path| path_matches(pattern, path, call.cwd)),
            Scope::PathSet(exact) => write_set_equals(call, exact),
            Scope::All => true,
        }
    }

    fn describe(&self) -> String {
        match self {
            Scope::Tool(pattern) => format!("工具 {pattern}"),
            Scope::CommandPrefix(prefix) => format!("命令 {}", prefix.join(" ")),
            Scope::Path(pattern) => format!("路径 {pattern}"),
            Scope::PathSet(exact) => {
                let paths: Vec<String> = exact.iter().map(|p| p.display().to_string()).collect();
                format!("写集恰好 [{}]", paths.join(", "))
            }
            Scope::All => "任何调用".to_owned(),
        }
    }
}

/// 一条权限规则：谁、在什么之上、怎么办，以及它是否到达执行者。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// 规则适用于谁。
    pub subject: Subject,
    /// 这条规则拿什么谓词去匹配调用。
    pub scope: Scope,
    /// 匹配上这条规则意味着什么。
    pub action: Decision,
    /// 这条规则是否被派出的执行者继承。默认由 action 决定（`Deny`/`Ask` 是，
    /// `Allow` 否）。
    pub propagate: bool,
}

impl Rule {
    /// 建一条 `propagate` 取 action 默认值的规则。
    pub fn new(subject: Subject, scope: Scope, action: Decision) -> Self {
        Self {
            propagate: action.default_propagate(),
            subject,
            scope,
            action,
        }
    }

    /// 为这条规则覆盖 action 的默认传播性。
    pub fn with_propagate(mut self, propagate: bool) -> Self {
        self.propagate = propagate;
        self
    }

    /// 会话级的「总是允许」追加的那条规则：这个参与者可以用这个工具。它绝不传播、
    /// 也绝不离开这个会话。
    pub fn always_allow(speaker: &SpeakerId, tool_name: &str) -> Rule {
        Rule::new(
            Subject::for_speaker(speaker),
            Scope::Tool(tool_name.to_owned()),
            Decision::Allow,
        )
    }

    fn describe(&self) -> String {
        format!(
            "规则 {} {} → {}",
            self.subject.describe(),
            self.scope.describe(),
            self.action.as_str()
        )
    }
}

/// 一个模式加上它的规则，外加那条与档位正交的区外读旋钮。
///
/// 模式是 `Session` 的一个值：它从不进事件流，而 `--continue` 会回到配置里那一档
/// （spec §12）。`outside_read` 来自 `[permissions] outside_read`，同样是值、同样不进
/// 事件流。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    mode: Mode,
    outside_read: Decision,
    rules: Vec<Rule>,
}

impl Policy {
    /// 一个跑这个模式、没有规则的策略。区外读缺省是 `Deny`：那条地板是策略级的，只有
    /// 显式写下来才算放弃（`.scratch/workspace-mode/spec.md` §2）。
    pub fn for_mode(mode: Mode) -> Self {
        Self {
            mode,
            outside_read: Decision::Deny,
            rules: Vec::new(),
        }
    }

    /// 换掉区外读那一条裁决。三个值都是这条旋钮的合法取值，而缺省是 `Deny`。
    pub fn with_outside_read(mut self, outside_read: Decision) -> Self {
        self.outside_read = outside_read;
        self
    }

    /// 读目标落在 `cwd` 之外时这条策略给的裁决。
    pub fn outside_read(&self) -> Decision {
        self.outside_read
    }

    /// 这个策略跑在哪一档。
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// 规则，按压入顺序。
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// 加一条规则。「总是允许」就是这么改会话策略的 —— 也是它唯一的改动途径：不写
    /// `config.toml`、不进事件。
    pub fn push(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// 换掉模式、留下规则。这是模式循环手势对策略唯一的影响：一个值，永远不是一个
    /// 事件（spec §12）。
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// 会跟着走到派出的执行者那里的规则：只有标了 `propagate` 的那些 —— 缺省是拒绝与
    /// 问 —— 所以继承只可能收紧。
    ///
    /// 算出子会话的整份策略是 [`crate::agent::executor`] 的事，而它拿这张列表做什么
    /// 才是重点：子会话与派发者跑在**同一个模式**下（模式是会话对写的立场，继承到一个
    /// 更松的就是放宽），保留每一条传播来的规则，并以一个空的读集合起步。于是它的权限
    /// 是派发者的子集；永远不跟着走的是**许可**，而如果 `Allow` 会传播，这些规则正是
    /// 会带上许可的那种（spec §12、§16）。
    pub fn inherited_rules(&self) -> Vec<Rule> {
        self.rules
            .iter()
            .filter(|rule| rule.propagate)
            .cloned()
            .collect()
    }
}

/// 一个解析不了的目标是**读**还是**写**。
///
/// 方向决定 `decide()` 的 ④ 段拿哪一条裁决去回答越界：读的越界按
/// [`Policy::outside_read`]，写的越界按档位（`workspace` 档问一次，其余三档仍是地板）
/// —— `.scratch/workspace-mode/spec.md` §2、§3。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Read,
    Write,
}

impl Direction {
    /// 一个方向的中文名，给诊断文本用。
    pub fn label(self) -> &'static str {
        match self {
            Direction::Read => "读",
            Direction::Write => "写",
        }
    }
}

/// 一个没能解析到工作区里的目标，带着它是读还是写。
///
/// 权限门只读它，所以错误的**文本**原样留在里面：真正拒掉这次调用的是门（或它给出的
/// `Ask`），而护栏在门说「不」时交出的就是这一份错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathError {
    /// 这个目标是读还是写。
    pub direction: Direction,
    /// 路径解析本来的失败。
    pub error: ToolError,
}

impl PathError {
    pub fn read(error: ToolError) -> Self {
        Self {
            direction: Direction::Read,
            error,
        }
    }

    pub fn write(error: ToolError) -> Self {
        Self {
            direction: Direction::Write,
            error,
        }
    }
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

/// 一次**升级申请**：命令被沙箱拒了之后，模型带上的那份理由与它要放开的路径。
///
/// 形状照 DSH 的越界升级（`.scratch/sandbox/research/01` §⑦）：模型在同一回合里声明式地
/// 重试，而不是让人从拒绝输出里猜。这里存的是**解析后**的绝对路径，所以门与沙箱用的是
/// 同一批字符串（`.scratch/workspace-mode/spec.md` §4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escalation {
    /// 为什么要越界，一句话，模型自己写的。
    pub justification: String,
    /// 这一次调用要放开的路径，**就是这些路径本身**，不做父目录提升。
    pub writable_paths: Vec<PathBuf>,
}

/// 一次调用在权限门眼里的样子。由循环用注册表解析出的事实建出来；借用，所以权限门
/// 始终是一次纯读。
#[derive(Debug)]
pub struct Call<'a> {
    /// 模型要的那个工具名。
    pub tool_name: &'a str,
    /// 工具声明的对工作区的副作用。
    pub effect: &'a Effect,
    /// 已解析的绝对写目标（副作用不是 `WritePaths` 时为空）。解析不了的目标保留它的
    /// 字面形式，于是权限门照样看得见这次调用。
    pub write_targets: &'a [PathBuf],
    /// 已解析的绝对读目标。
    pub read_targets: &'a [PathBuf],
    /// 命令类工具将要跑的 argv —— 当它要跑一条命令时。
    pub argv: Option<&'a [String]>,
    /// 会话 cwd，相对路径模式与 `rm` 断路的基准。
    pub cwd: &'a Path,
    /// 用户的 home —— 知道的时候。只有 `rm` 断路器读它。
    pub home: Option<&'a Path>,
    /// 某个目标为什么解析不到工作区里 —— 如果确实有，以及它是读还是写。路径上限据此给
    /// 裁决，于是记下来的裁决与实际结果一致，而不是报一个这次调用根本没用上的 `Allow`。
    pub path_error: Option<&'a PathError>,
    /// 这一次调用带上的升级申请 —— 当模型带了一个。
    pub escalation: Option<&'a Escalation>,
    /// 遮罩目录（`[sandbox] masks`）。升级的写死边界判据要用它，而权限门是纯的，所以
    /// 它随调用一起进来。
    pub masks: &'a [PathBuf],
}

/// 权限门的答案：一个裁决，加一句永远填好的理由（票 19 读 `reason` 来回答「我的权限
/// 策略是不是太烦了？」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// 权限门的裁决。
    pub decision: Decision,
    /// 为什么，一句话；永远不空。
    pub reason: String,
}

/// 权限门。纯函数：不读环境、不交互、不落事件。
pub fn decide(policy: &Policy, speaker: &SpeakerId, call: &Call<'_>) -> Verdict {
    // ① 断路器在任何规则被评估之前就把一个硬拒绝短路掉，所以没有任何 allow（以及
    //    往后任何钩子）能翻转它。
    if let Some(verdict) = circuit_breaker(call) {
        return verdict;
    }

    let stance = policy.mode.stance(call);

    // ② 模式的缺省，或者 —— 有规则匹配时 —— 匹配规则的**上确界**。专指程度是刻意
    //    不看的：宽泛的 deny 压过狭窄的 allow，所以豁免必须写进 deny 的谓词里。
    let matching: Vec<&Rule> = policy
        .rules
        .iter()
        .filter(|rule| rule.subject.matches(speaker) && rule.scope.matches(call))
        .collect();

    let mut parts: Vec<(Decision, String)> = if matching.is_empty() {
        vec![(stance.default, stance.reason.to_owned())]
    } else {
        matching
            .iter()
            .map(|rule| (rule.action, rule.describe()))
            .collect()
    };

    // ③ 模式的地板（只有 `readonly` 有）在规则替换掉模式的缺省之后也要一并取最大。
    if !matching.is_empty() {
        if let Some(floor) = stance.floor {
            parts.push((floor, stance.reason.to_owned()));
        }
    }

    // ④ 属于策略缺省、而不是来自文件的规则的约束：工作区的路径上限、绝不自动放行的
    //    敏感写入，以及 `.env` 一族。前一条按**方向**分支（读看 `outside_read`、写看
    //    档位），后两条都是地板，所以没有规则能降下去。
    if let Some(error) = call.path_error {
        let decision = match error.direction {
            Direction::Read => policy.outside_read,
            Direction::Write => match policy.mode {
                // 选这一档就是同意「区外要问」——路径上限在这一档下不再是地板，它仍然是
                // 另外三档的地板（`.scratch/workspace-mode/spec.md` §3）。
                Mode::Workspace => Decision::Ask,
                _ => Decision::Deny,
            },
        };
        parts.push((
            decision,
            format!("路径上限（{}）：{error}", error.direction.label()),
        ));
    }
    // 升级申请：它请的是**放宽沙箱**，而沙箱不因权限模式而关，所以除了 `readonly`
    // （那一档连跑都不让）之外各档都有效。写死的安全默认不给任何通道 —— 挂载表里那条
    // 遮罩或只读挂载会盖掉批准，批准等于白批，而用户会以为自己批准了
    // （`.scratch/workspace-mode/spec.md` §4、§5）。
    if let Some(escalation) = call.escalation {
        if policy.mode != Mode::Readonly {
            let sealed = escalation
                .writable_paths
                .iter()
                .find(|path| sandbox::sealed(path, call.cwd, call.masks));
            match sealed {
                Some(path) => parts.push((
                    Decision::Deny,
                    format!(
                        "沙箱升级被拒：{} 是写死的安全默认，这一条没有任何通道放宽",
                        path.display()
                    ),
                )),
                None => {
                    let paths: Vec<String> = escalation
                        .writable_paths
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect();
                    parts.push((
                        Decision::Ask,
                        format!(
                            "沙箱升级（一次批准）：申请写 {}；理由：{}",
                            paths.join("、"),
                            escalation.justification
                        ),
                    ));
                }
            }
        }
    }
    if let Some(verdict) = never_auto_approved(call) {
        parts.push((verdict.decision, verdict.reason));
    }
    if let Some(verdict) = env_family(call) {
        parts.push((verdict.decision, verdict.reason));
    }

    let decision = parts
        .iter()
        .map(|(decision, _)| *decision)
        .max()
        .unwrap_or(Decision::Allow);
    let mut reasons: Vec<String> = parts
        .iter()
        .filter(|(part, _)| *part == decision)
        .map(|(_, reason)| reason.clone())
        .collect();
    reasons.dedup();
    Verdict {
        decision,
        reason: reasons.join("；"),
    }
}

/// 断路器们，在每一条规则之前先查。这里的 `Deny` 是终局的。
fn circuit_breaker(call: &Call<'_>) -> Option<Verdict> {
    if let Some(verdict) = rm_breaker(call) {
        return Some(verdict);
    }
    protected_write(call).map(|path| Verdict {
        decision: Decision::Deny,
        reason: format!(
            "断路器：往 {} 里写一律拒绝，任何规则都不例外",
            path.display()
        ),
    })
}

/// 对文件系统根或家目录（或两者的某个祖先）动手的 `rm`，是没有任何规则能批准的那一
/// 类：它是 agent 能做的最不可逆的事。
///
/// `bash` 工具把它的 argv 声明成 `["bash", "-lc", command]`（spec §7），所以看的是
/// shell 真正要跑的那条命令，而不是启动它的那层包装 —— 否则每一条 `rm` 都能藏在一个
/// 词背后躲过去。
fn rm_breaker(call: &Call<'_>) -> Option<Verdict> {
    let argv = call.argv?;
    for command in simple_commands(argv) {
        if command.first().copied() != Some("rm") {
            continue;
        }
        if let Some(target) = command.iter().skip(1).find(|arg| {
            !arg.starts_with('-')
                && !arg.is_empty()
                && targets_root_or_home(arg, call.cwd, call.home)
        }) {
            return Some(Verdict {
                decision: Decision::Deny,
                reason: format!("断路器：冲着 {target} 去的 rm 一律拒绝，任何规则都不例外"),
            });
        }
    }
    None
}

/// 一条 argv 将会跑的那些简单命令，按 token 列表给出。
///
/// 对一条普通 argv，它就是这条 argv 本身。对一个 shell 包装（`bash -lc "<command>"`），
/// 它是把命令串按 shell 的控制操作符切开再分词。这次扫描是**词法、best-effort** 的：
/// 断路器存在是为了拦住事故，不是为了圈禁对手（spec §12、§20），所以它看不见的写法
/// 是写进文档，而不是去追。
fn simple_commands(argv: &[String]) -> Vec<Vec<&str>> {
    match shell_command_string(argv) {
        Some(script) => shell_simple_commands(script),
        None => vec![argv.iter().map(String::as_str).collect()],
    }
}

/// 一次 shell 调用将要跑的命令串 —— 如果这条 argv 确实是的话。
///
/// 认出带一个含 `-c` 旗标的 `bash`/`sh` 调用：那个旗标后面的参数就是脚本。扫描会跳过
/// 选项，所以先吃一个选项参数再上 `-c` 的 shell（`bash -o pipefail -c "…"`）照样能
/// 被看见；给的是脚本文件的 shell（`bash build.sh`）在这里没有命令串，它的 argv 就
/// 按普通 argv 处理。
fn shell_command_string(argv: &[String]) -> Option<&str> {
    let shell = Path::new(argv.first()?).file_name()?.to_str()?;
    if !matches!(shell, "bash" | "sh") {
        return None;
    }
    for (index, arg) in argv.iter().enumerate().skip(1) {
        let Some(cluster) = arg.strip_prefix('-') else {
            // 一个不是旗标的 token（脚本文件，或某个选项的参数）不带这个旗标；
            // 后面的 `-c` 仍然可能带。
            continue;
        };
        if cluster.is_empty() || cluster.starts_with('-') {
            // `-`（stdin）或长选项（`--norc`）；两者都不带这个旗标。
            continue;
        }
        if cluster.contains('c') {
            return argv.get(index + 1).map(String::as_str);
        }
    }
    None
}

/// 把一条 shell 命令串切成简单命令，并为每条分词。
///
/// 控制操作符（`;`、`&`、`|`、`(`、`)`、换行）开启一条新的简单命令，成对的引号会从
/// token 上剥掉，开头的语法关键字（`then`、`do`、`if`、`!`……）会被丢掉，于是
/// `rm -rf "/"`、`(rm -rf /)` 与 `if x; then rm -rf /; fi` 读出来都是一个 `rm`。
fn shell_simple_commands(script: &str) -> Vec<Vec<&str>> {
    script
        .split([';', '&', '|', '(', ')', '\n'])
        .map(|segment| {
            let mut tokens: Vec<&str> = segment
                .split_whitespace()
                .map(strip_quotes)
                .filter(|token| !token.is_empty())
                .collect();
            while tokens
                .first()
                .is_some_and(|token| SHELL_KEYWORDS.contains(token))
            {
                tokens.remove(0);
            }
            tokens
        })
        .filter(|command: &Vec<&str>| !command.is_empty())
        .collect()
}

/// 可能出现在一条简单命令前面的语法性 shell 词，好让断路器读的是命令、而不是它前面
/// 那个关键字。
///
/// 这些是语法、不是间接层：剥掉它们仍然是对同一条命令的词法读取。改变**跑的是什么**
/// 的包装（`sudo`、`env`、`eval`、别名）刻意不在表里 —— 这次扫描看不见什么，见
/// `docs/bash.md`。
const SHELL_KEYWORDS: &[&str] = &[
    "if", "then", "elif", "else", "fi", "while", "until", "do", "done", "time", "!",
];

/// 去掉一对外围引号（成对匹配）之后的 token。
fn strip_quotes(token: &str) -> &str {
    let bytes = token.as_bytes();
    if token.len() >= 2 {
        let first = bytes[0];
        let last = bytes[token.len() - 1];
        if (first == b'"' || first == b'\'') && first == last {
            // 引号是 ASCII 的，所以两处切点都落在字符边界上。
            return &token[1..token.len() - 1];
        }
    }
    token
}

/// 一个 `rm` 参数是否指向 `/`、`~`，或两者的某个祖先。
///
/// 相对参数会折进会话 cwd，所以在家目录下某个目录里跑 `rm -rf ../..` 会与它的绝对
/// 写法一样被逮住。
fn targets_root_or_home(arg: &str, cwd: &Path, home: Option<&Path>) -> bool {
    let trimmed = arg.trim_end_matches('/');
    // `""` 就是 `/` 或 `//`。
    if trimmed.is_empty() {
        return true;
    }
    if trimmed == "~" {
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if rest.is_empty() {
            return true;
        }
        if let Some(home) = home {
            return reaches_root_or_home(&fold(&home.join(rest)), Some(home));
        }
        // 没有 home 时，只有明确能折出来的写法才判得了。
        return rest.split('/').all(|component| component == "..");
    }
    let path = Path::new(trimmed);
    let folded = if path.is_absolute() {
        fold(path)
    } else {
        fold(&cwd.join(path))
    };
    reaches_root_or_home(&folded, home)
}

/// 一条词法折叠后的路径是否就是文件系统根，或者是家目录的祖先（家目录本身也算）。
fn reaches_root_or_home(folded: &Path, home: Option<&Path>) -> bool {
    if folded == Path::new("/") {
        return true;
    }
    home.is_some_and(|home| home.starts_with(folded))
}

/// 词法地折掉 `.` 与 `..`。权限门没法 canonicalize —— 它是纯的 —— 而词法折叠正是
/// 那个不碰磁盘就能逮住 `/home/u/../..` 的东西。
fn fold(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 写进 `.git/` 或 `.ssh/` 里。
fn protected_write<'a>(call: &Call<'a>) -> Option<&'a Path> {
    call.write_targets
        .iter()
        .find(|path| {
            path.components().any(|component| match component {
                Component::Normal(name) => PROTECTED_DIRS
                    .iter()
                    .any(|dir| name == std::ffi::OsStr::new(dir)),
                _ => false,
            })
        })
        .map(PathBuf::as_path)
}

/// 写一个 shell 启动文件：绝不自动放行，但用户仍然可以逐个批准。
fn never_auto_approved(call: &Call<'_>) -> Option<Verdict> {
    call.write_targets
        .iter()
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| SHELL_RC_FILES.contains(&name))
        })
        .map(|path| Verdict {
            decision: Decision::Ask,
            reason: format!(
                "敏感写入：{} 绝不自动放行（用户仍然可以逐个批准）",
                path.display()
            ),
        })
}

/// `.env` 一族：它的全部内容都是凭据，而一次读会永久落进会话文件与 `outputs/` 里。
/// 模板不携带真密钥。
fn env_family(call: &Call<'_>) -> Option<Verdict> {
    call.write_targets
        .iter()
        .chain(call.read_targets.iter())
        .find(|path| is_env_file(path))
        .map(|path| Verdict {
            decision: Decision::Deny,
            reason: format!("缺省策略：.env 一族一律拒绝（{}）", path.display()),
        })
}

/// 这条路径是不是 `.env` 一族（模板后缀除外）。
///
/// crate 内可见，是因为沙箱的保护路径清单必须与这条地板**用同一份口径**：`bash` 里
/// 能改动的 `.env` 家族，与文件工具里能改动的那些，不能是两份会漂离的名单
/// （`.scratch/sandbox/issues/01-sandbox-wrap-pure-function.md` 第 5 条）。
pub(crate) fn is_env_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if ENV_TEMPLATE_SUFFIXES
        .iter()
        .any(|suffix| name.ends_with(suffix))
    {
        return false;
    }
    name == ".env" || name.starts_with(".env.") || name.ends_with(".env")
}

/// `PathSet` 精确比较**写**集合，而且只对 `WritePaths` 调用有意义：`Exclusive` 没有
/// 路径集合可言，所以它永远借不到路径形状的豁免。
fn write_set_equals(call: &Call<'_>, exact: &[PathBuf]) -> bool {
    if !matches!(call.effect, Effect::WritePaths(_)) {
        return false;
    }
    let mut actual: Vec<&PathBuf> = call.write_targets.iter().collect();
    let mut want: Vec<&PathBuf> = exact.iter().collect();
    actual.sort();
    actual.dedup();
    want.sort();
    want.dedup();
    actual == want
}

fn path_matches(pattern: &str, path: &Path, cwd: &Path) -> bool {
    if pattern.starts_with('/') {
        return glob_match(pattern, &path.to_string_lossy());
    }
    let Ok(relative) = path.strip_prefix(cwd) else {
        return false;
    };
    let relative = relative.to_string_lossy();
    if glob_match(pattern, &relative) {
        return true;
    }
    // `**/name` 也匹配 cwd 根下的 `name`：开头的 `**/` 可以代表零层目录，用户用它
    // 就是这个意思。
    pattern
        .strip_prefix("**/")
        .is_some_and(|rest| glob_match(rest, &relative))
}

/// 一个小 glob：`*` 匹配任意一段不跨 `/` 的字符，`**` 跨 `/`，`?` 匹配一个字符
/// （永远不匹配 `/`），其余都是字面量。没有依赖，也不会在「一条 deny 模式到底覆盖了
/// 什么」上出意外。
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();

    // dp[j]：目前消费掉的模式是否匹配 text[..j]？
    let mut dp = vec![false; text.len() + 1];
    dp[0] = true;

    let mut index = 0;
    while index < pattern.len() {
        let mut next = vec![false; text.len() + 1];
        if pattern[index] == '*' {
            if pattern.get(index + 1) == Some(&'*') {
                next[0] = dp[0];
                for j in 1..=text.len() {
                    next[j] = dp[j] || next[j - 1];
                }
                index += 2;
            } else {
                next[0] = dp[0];
                for j in 1..=text.len() {
                    next[j] = dp[j] || (next[j - 1] && text[j - 1] != '/');
                }
                index += 1;
            }
        } else if pattern[index] == '?' {
            for j in 1..=text.len() {
                next[j] = dp[j - 1] && text[j - 1] != '/';
            }
            index += 1;
        } else {
            for j in 1..=text.len() {
                next[j] = dp[j - 1] && text[j - 1] == pattern[index];
            }
            index += 1;
        }
        dp = next;
    }

    dp[text.len()]
}

/// 循环在权限门之外问的一个问题。
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRequest {
    /// `PermissionAsked` 与 `PermissionDecided` 这一对共用的标识。
    pub request_id: String,
    /// 这个问题所问的那次调用。
    pub tool_call_id: String,
    /// 工具名，给会点名动作的提示用。
    pub tool_name: String,
    /// 参数，好让提示能显示将要跑什么。
    pub args: Value,
    /// 权限门为什么问 —— 带上它，好让提示能自己解释自己。
    pub reason: String,
    /// 这一点**只在发起者不是主会话时**存在：执行者与讨论者各问各的，而作答者是人，
    /// 人至少要知道这次问的是谁（`.scratch/workspace-mode/spec.md` §7）。
    pub speaker: Option<SpeakerId>,
    /// 这一次询问是不是一次**升级**，以及它要放开的路径。弹窗据此多两行，前端不必从
    /// 理由文本里猜（`.scratch/workspace-mode/spec.md` §7）。
    pub escalation: Option<Escalation>,
}

/// 用户答了什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// 跑这次调用。
    Allow,
    /// 跑这次调用，并在余下的会话里记住这条许可。
    AlwaysAllow,
    /// 拒掉这次调用。
    Deny,
}

/// 循环发问走的那条端口。headless 会话什么都不注入，循环就把权限门的 `Ask` 降级成
/// `Deny`；交互式渲染器注入一个读键盘并作答的实现。
#[async_trait]
pub trait Asker: Send + Sync {
    /// 就一次调用发问，并返回用户的答案。
    async fn ask(&self, request: &PermissionRequest) -> Answer;
}
