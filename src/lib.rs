//! fs-agent：一个自用的 coding agent CLI。
//!
//! 一条只追加的事件流，加上每个 agent 一份投影。事件流是唯一真相源；agent 的
//! `messages` 由它重算，从不存下来。
//!
//! # 组装接缝
//!
//! [`assemble`] 与 [`assemble_discussion`] 是那条唯一的端到端接缝的两头。它们接收
//! 注入的 [`Provider`]、注入的渲染接收端与注入的配置值，并且**不读任何环境** —— 于是
//! 一个测试可以拿一个按脚本应答的假 provider 驱动整场会话，再对 JSONL 事件流与那两个
//! 接收端做断言。后来的票给这条接缝加场景；它们不会另开 mock 接缝。
//!
//! 一次讨论就是同一个脚手架按参与者各开一次：一条事件流、两个共享它的讨论者会话，再加
//! 一场会话负责收尾那一次单发调用（spec §15）。
//!
//! # 边界
//!
//! 十三个顶层模块，只向下依赖：`events` · `config` · `provider` · `tools` ·
//! `permissions` · `questions` · `hooks` · `context` · `agent` · `discussion` ·
//! `session` · `render` · `cli`。`events` 不依赖任何内部模块；[`provider::projection`]
//! 是 `provider` 的子模块、不是一条边界。`discussion` 永远不碰 `provider`：它持着
//! 协议的规则，而每一次调用都由 `agent` 层驱动。

pub mod agent;
pub mod cli;
pub mod config;
pub mod context;
pub mod discussion;
pub mod events;
pub mod goals;
pub mod hooks;
pub mod permissions;
pub mod provider;
pub mod questions;
pub mod render;
pub mod session;
pub mod tools;

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::task::JoinHandle;

use crate::agent::{CancelSignal, TurnOutcome};
use crate::config::{SandboxAvailability, SandboxMode, SandboxSettings, SessionConfig};
use crate::context::skills::Skills;
use crate::events::{ContextSource, Event, EventLog, EventPayload, Role, SessionId, SpeakerId};
use crate::hooks::Hook;
use crate::permissions::{Asker, Mode, Policy};
use crate::provider::Provider;
use crate::questions::UserQuestions;
use crate::render::{RenderHandle, Renderer};
use crate::session::{Session, SessionParts};
use crate::tools::sandbox;
use crate::tools::{PathLocks, Registry};

/// 一场会话需要的一切，除了谁在发言、用的又是哪个模型。
///
/// 一个脚手架可以开出不止一场会话，一次讨论就是这样：两个讨论者必然共享事件流、工具
/// 表、路径锁、权限策略、询问端口与钩子，因为那些是会话级事实、而不是每个 agent 各自的
/// 东西（spec §15）。渲染接收端刻意**不**在这里：一个进程只有一个渲染器，所以它们由
/// 组装的那一方一次性消费掉。
pub struct SessionScaffold {
    /// 会话的工作目录，记在 `SessionStarted` 里。
    pub cwd: PathBuf,
    /// 这场会话 JSONL 事件日志的路径。它的父目录必须已存在。
    pub log_path: PathBuf,
    /// 会话身份；跨 `--continue` 永不改变。
    pub session_id: SessionId,
    /// 工具表。一个运行时值，在这里组装出来，永远不是全局量。
    pub tools: Registry,
    /// 按路径的写锁。**同一张**表必须到达每一个执行者，否则写互斥就只是每场会话各自
    /// 的、也就等于根本没有锁。
    pub locks: PathLocks,
    /// 这场会话的权限策略：一个模式加上它的规则。
    pub policy: Policy,
    /// 权限门答 `Ask` 时用的询问端口。`None` 表示没有交互式作答者，于是循环把 `Ask`
    /// 降级成 `Deny`。
    pub asker: Option<Arc<dyn Asker>>,
    /// 把模型发起的问句摆给用户的那条端口（spec §7）。`None` 表示没有问卷调查作答者
    /// —— headless 组装永远不挂 —— 于是工具表不宣告 `ask_user_question`，而它万一被
    /// 调到也会可读地失败。
    pub questions: Option<Arc<dyn UserQuestions>>,
    /// 挂在工具调用钩子点上的策略。`None` 表示循环不调任何钩子。
    pub hook: Option<Arc<dyn Hook>>,
    /// 用户的 home 目录 —— 调用方知道的时候。只有 `rm` 断路器读它。
    pub home: Option<PathBuf>,
}

/// 单 agent 会话需要的一切，全部注入。
pub struct AssemblyParts {
    /// 这一切跑在其中的那场会话。
    pub scaffold: SessionScaffold,
    /// 模型客户端。真的 profile 在票 02 到；测试注入假的。
    pub provider: Box<dyn Provider>,
    /// 这场会话扮演的那个 agent。
    pub speaker: SpeakerId,
    /// 这个 agent 的模型与预算值。
    pub config: SessionConfig,
    /// 渲染器，启动时选定。三种模式正好跑一种，而组装只创建它要消费的那一条通道
    /// （spec §19）。
    pub renderer: Renderer,
}

/// 一次讨论里的一个讨论者：谁发言，以及替它作答的是什么。
pub struct DebaterParts {
    /// 必须是 [`SpeakerId::Debater`]：协议要在讨论者之间比较。
    pub speaker: SpeakerId,
    /// 这个讨论者自己的模型与取值。
    pub config: SessionConfig,
    pub provider: Box<dyn Provider>,
    /// 用户给这一方写的**人物**：他是什么样的人，用用户自己的话。在第一轮之前作为一条
    /// 私有注入记下来（spec §15）。
    pub soul: Option<String>,
}

/// 合成器的那一次调用：它不是 agent，所以不需要发言归属、不需要工具、也没有回合
/// （spec §15）。
pub struct SynthesizerParts {
    pub config: SessionConfig,
    pub provider: Box<dyn Provider>,
}

/// 一次讨论需要的一切，全部注入。
pub struct DiscussionParts {
    /// 这一切跑在其中的那场会话。每个讨论者各开一次。
    pub scaffold: SessionScaffold,
    /// 正好 [`discussion::DEBATERS`] 个讨论者（spec §15；N > 2 会重新打开「N = 2 不做
    /// 裁决」那个决议，所以 v1 拒掉它）。
    pub debaters: Vec<DebaterParts>,
    pub synthesizer: SynthesizerParts,
    /// 讨论轮次的上限。`None` 取协议的缺省，也就是一个独立首轮加一个定向第二轮
    /// （[`discussion::DEFAULT_MAX_ROUNDS`]）；`Some` 是调用方改它的方式，而零是
    /// 拒掉的。
    pub max_rounds: Option<u32>,
    /// 渲染器，启动时选定。三种模式正好跑一种，而组装只创建它要消费的那一条通道
    /// （spec §19）。
    pub renderer: Renderer,
}

/// 组装好之后由调用方驱动的 harness。
pub struct Harness {
    session: Session,
    /// 共享的，因为这场会话派出的执行者在同一个客户端上作答：执行者的模型是继承来的，
    /// 除非有 profile 覆盖（spec §16）。
    provider: Arc<dyn Provider>,
    speaker: SpeakerId,
    render: RenderHandle,
    /// 这场会话自己那一端的取消手势（spec §6）。前端持有一个并举起它；各回合拿到的是
    /// 它的观察端。
    cancel: CancelSignal,
    /// 这条流上每个 agent 共享的策略 —— 放在这里而不是从会话里够过去：模式循环手势
    /// 必须在一个钉住的 run future 借着 harness 的时候也可用（spec §12）。
    policy: Arc<Mutex<Policy>>,
    render_task: JoinHandle<()>,
}

/// 组装好之后由调用方驱动的讨论。
pub struct DiscussionHarness {
    discussion: agent::Discussion,
    /// 共享的日志句柄，给断言与 `--continue` 记账用。
    log: EventLog,
    render: RenderHandle,
    /// 讨论自己那一端的取消手势（spec §6）：一次手势同时到达两个讨论者与它们派出的
    /// 每一个执行者。
    cancel: CancelSignal,
    /// 三个参与者共用的那一份策略（spec §15）；留着的理由和 [`Harness`] 一样：讨论的
    /// run future 借着 harness 的时候，模式手势必须够得着。
    policy: Arc<Mutex<Policy>>,
    /// 这台机器上有没有可用的沙箱（组装期定下来的那一件事）。`workspace` 档在循环里
    /// 会不会被跳过就看它，见 [`ModeCycle`]。
    workspace_available: bool,
    render_task: JoinHandle<()>,
}

/// 做完它那一次性工作的脚手架：一份日志、一张工具表、一张锁表、一份策略、一个渲染器。
/// 从它开出的每一场会话都共享这些。
struct OpenedSession {
    id: SessionId,
    cwd: PathBuf,
    log: EventLog,
    tools: Arc<Registry>,
    locks: PathLocks,
    outputs_dir: PathBuf,
    policy: Arc<Mutex<Policy>>,
    asker: Option<Arc<dyn Asker>>,
    /// 这场会话的提问端口（spec §7）。会话级，像询问端那样与每一场兄弟会话共享：讨论者
    /// 也通过同一个渲染器发问。
    questions: Option<Arc<dyn UserQuestions>>,
    hook: Option<Arc<dyn Hook>>,
    home: Option<PathBuf>,
    skills: Arc<Skills>,
    /// `AGENTS.md`，在会话存在之前读一次。
    agents_md: Option<String>,
    /// 这条流是否已经带着一条 `SessionStarted`：带着的那份日志是继续中的会话，不是新的
    /// 一场。
    resuming: bool,
    /// 沙箱那一次探测的结果（沙箱 spec §3）：组装期探一次，这场会话开出的每一个 agent 复用同一个
    /// 答案。`mode = "off"` 或者调用方已经给了结果时，这个格子根本不会被填。
    sandbox_probe: OnceLock<SandboxAvailability>,
    render: RenderHandle,
    render_task: JoinHandle<()>,
}

impl OpenedSession {
    /// 那一次性工作：建日志与它的产物目录、发现技能、共享工具表与策略、起渲染器。
    fn open(scaffold: SessionScaffold, renderer: Renderer) -> Result<Self, Error> {
        let SessionScaffold {
            cwd,
            log_path,
            session_id,
            tools,
            locks,
            policy,
            asker,
            questions,
            hook,
            home,
        } = scaffold;

        // 工具的产物住在事件日志旁边，于是会话始终是一个可搬运的目录（spec §11）。
        // 目录由需要它的那个工具按需创建。
        let outputs_dir = log_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("outputs");
        // 通道在这里创建，它的消费端则注入到唯一选中的那个渲染器：一个进程一个
        // 渲染器，永远没有并发的订阅者（spec §19）。
        let (render, receiver) = render::channel();
        let render_task = renderer.spawn(receiver);
        // 全新还是继续，由日志是否存在决定，而那是调用方的决定：它交过来的是它刚在新
        // session id 下分配的一条路径，或是更早一次运行留下的那份。这样库就不用去读环境
        // 旗标（spec §1），同时 `--continue` 仍然只有一条代码路径（spec §11）。
        let log = if log_path.exists() {
            EventLog::open(&log_path)?
        } else {
            EventLog::create(&log_path)?
        };
        let resuming = log
            .events()
            .iter()
            .any(|event| matches!(event.payload, EventPayload::SessionStarted { .. }));
        let agents_md = context::load_agents_md(&cwd);
        let skills = Arc::new(Skills::discover(&cwd, home.as_deref()));

        Ok(Self {
            id: session_id,
            cwd,
            log,
            tools: Arc::new(tools),
            locks,
            outputs_dir,
            policy: Arc::new(Mutex::new(policy)),
            asker,
            questions,
            hook,
            home,
            skills,
            agents_md,
            resuming,
            sandbox_probe: OnceLock::new(),
            render,
            render_task,
        })
    }

    /// 开出一个 agent 的会话。便宜：每一个会话级的值都是共享的。
    fn session(&self, mut config: SessionConfig, identity: Option<String>) -> Session {
        config.sandbox.availability = self.sandbox_availability(&config.sandbox);
        Session::new(SessionParts {
            id: self.id.clone(),
            cwd: self.cwd.clone(),
            log: self.log.clone(),
            config,
            tools: Arc::clone(&self.tools),
            locks: self.locks.clone(),
            outputs_dir: self.outputs_dir.clone(),
            policy: Arc::clone(&self.policy),
            asker: self.asker.clone(),
            questions: self.questions.clone(),
            hook: self.hook.clone(),
            home: self.home.clone(),
            skills: Arc::clone(&self.skills),
            identity,
        })
    }

    /// 沙箱那一次探测（沙箱 spec §3）：`mode = "off"` 或调用方已经给了结果时原样返回，否则探一次
    /// 并记住 —— 这场会话开出的每一个 agent（讨论的两个讨论者、以及每一个执行者）拿到的都是
    /// 同一个答案，PATH 中途变了也不重探。
    fn sandbox_availability(&self, settings: &SandboxSettings) -> SandboxAvailability {
        if !settings.needs_probe() {
            return settings.availability.clone();
        }
        self.sandbox_probe
            .get_or_init(|| sandbox::resolve_availability(settings, &self.cwd))
            .clone()
    }

    /// 那一次性头部工作：全新的流记下会话骨架；继续的流把被杀的进程没答完的那些调用
    /// 收尾。
    ///
    /// 继续的流**不会**得到第二条 `SessionStarted`、也不会重新注入上下文 —— 两者都已经
    /// 在日志里，而重放记下来的头部正是让这次继续在前缀缓存面前逐字节稳定的原因
    /// （spec §10、§11）。
    fn start(&self, session: &mut Session) -> Result<(), Error> {
        if !self.resuming {
            return self.record_skeleton(session);
        }
        // 继续的流不再记骨架，但沙箱状态是**这一刻**的事实：两次运行之间这台机器上的
        // `bwrap` 可能变得可用、也可能用不了了，而它只进日志、不进 `messages`，所以补记
        // 一条不碰前缀稳定性（`spec §8`）。
        self.record_sandbox_status(session)?;
        let recovered = agent::recover_pending_calls(session, &self.render)?;
        if recovered > 0 {
            self.render.diagnostic(&format!(
                "继续的会话：收尾了 {recovered} 个结果未知的中断工具调用"
            ));
        }
        Ok(())
    }

    /// 把这一刻的沙箱状态记进流里（沙箱 spec §8）：**只进日志**，投影不把它变成
    /// `messages`，所以钉住的前缀逐字不变。
    fn record_sandbox_status(&self, session: &mut Session) -> Result<(), Error> {
        let (mode, reason) = {
            let sandbox = &session.config().sandbox;
            let reason = match &sandbox.availability {
                SandboxAvailability::Unavailable { reason } => Some(reason.clone()),
                _ => None,
            };
            (sandbox.mode, reason)
        };
        agent::record_sandbox_status(session, &self.render, mode, reason.as_deref())
    }

    /// 通过其中一场会话记下会话骨架：`SessionStarted`，然后那两条钉住的注入。
    ///
    /// 身份 -> 规则 -> 技能清单 -> 历史（spec §10）。缺 `AGENTS.md` 不是错误，只表示
    /// 没有那条注入。每条流正好有一场会话做这件事，一次讨论的所有讨论者因此投影出同一个
    /// 钉住的头部。
    fn record_skeleton(&self, session: &mut Session) -> Result<(), Error> {
        agent::record_session_started(session, &self.render)?;
        self.record_sandbox_status(session)?;
        if let Some(content) = &self.agents_md {
            agent::record_context_injection(
                session,
                &self.render,
                ContextSource::AgentsMd,
                content,
            )?;
        }
        if let Some(catalog) = self.skills.catalog() {
            agent::record_context_injection(
                session,
                &self.render,
                ContextSource::SkillsCatalog,
                &catalog,
            )?;
        }
        Ok(())
    }
}

/// `workspace` 档在没有可用沙箱的地方不存在（`.scratch/workspace-mode/spec.md` §6）。
///
/// 这一档对 shell 的承诺完全建立在沙箱上：判不出区内区外的命令先跑、被内核拒了才走升级。
/// 没有沙箱时它的那个承诺是空的，所以宁可让这一档不存在，也不给一个看起来在保护、实际有
/// 一个大洞的档位。
///
/// 判据两条，任一条成立就拒：显式写了 `[sandbox] mode = "off"`，或者探测说 `bwrap` 在这台
/// 机器上起不来。检查发生在**组装期** —— 它是唯一同时看得到权限策略与沙箱状态的地方 ——
/// 而 `cli` 的每一个组装点照旧把 [`Error`] 打到 stderr。
fn workspace_needs_a_sandbox(
    mode: Mode,
    settings: &SandboxSettings,
    cwd: &Path,
) -> Result<(), Error> {
    if mode != Mode::Workspace {
        return Ok(());
    }
    let Some(reason) = sandbox_unavailable_reason(settings, cwd) else {
        return Ok(());
    };
    Err(Error::WorkspaceWithoutSandbox(format!(
        "权限模式 workspace（工作区）需要一层可用的沙箱，而这里没有：{reason}。\
         两条出路：把 `[sandbox] mode` 换回 \"bwrap\"（并让 `bwrap` 出现在 PATH 上），\
         或者换一档（`ask` / `auto`）"
    )))
}

/// 这一份沙箱设置为什么让 `workspace` 档不成立 —— 它成立时是 `None`。
///
/// 判据两条，任一条成立就拒：显式写了 `[sandbox] mode = "off"`，或者探测说 `bwrap` 在这台
/// 机器上起不来。组装期与模式循环手势（第二个入口）共用它，于是「这一档不存在」在两处
/// 是同一个答案。
fn sandbox_unavailable_reason(settings: &SandboxSettings, cwd: &Path) -> Option<String> {
    if settings.mode == SandboxMode::Off {
        return Some("`[sandbox] mode` 被显式设成了 \"off\"".to_owned());
    }
    match sandbox::resolve_availability(settings, cwd) {
        SandboxAvailability::Unavailable { reason } => Some(reason),
        _ => None,
    }
}

/// 名册里的每一个参与者都得过 [`workspace_needs_a_sandbox`] 那一关。
///
/// 两个讨论组装点（一场自己的讨论、一场跑在活会话上的讨论）共用它，于是「新增一个组装
/// 点时漏掉某一个参与者」这件事不会发生。
fn roster_needs_a_sandbox(
    mode: Mode,
    debaters: &[DebaterParts],
    synthesizer: &SynthesizerParts,
    cwd: &Path,
) -> Result<(), Error> {
    for debater in debaters {
        workspace_needs_a_sandbox(mode, &debater.config.sandbox, cwd)?;
    }
    workspace_needs_a_sandbox(mode, &synthesizer.config.sandbox, cwd)
}

/// 组装一场单 agent 会话。不读任何环境。
pub async fn assemble(parts: AssemblyParts) -> Result<Harness, Error> {
    let AssemblyParts {
        scaffold,
        provider,
        speaker,
        config,
        renderer,
    } = parts;

    workspace_needs_a_sandbox(scaffold.policy.mode(), &config.sandbox, &scaffold.cwd)?;

    let opened = OpenedSession::open(scaffold, renderer)?;
    let mut session = opened.session(config, Some(agent::agent_identity()));
    opened.start(&mut session)?;

    let policy = Arc::clone(&opened.policy);
    Ok(Harness {
        session,
        provider: provider.into(),
        speaker,
        render: opened.render,
        cancel: CancelSignal::new(),
        policy,
        render_task: opened.render_task,
    })
}

/// 名册的那些不变量，凡组装讨论的地方都查一遍。
///
/// 两条入口 —— 一场有自己事件流的讨论（[`assemble_discussion`]）与一场跑在活动会话上
/// 的讨论（[`Harness::discuss`]）—— 问的是同一组问题，于是一份名册不可能在一条路径上
/// 可以接受、在另一条上被拒。
fn validate_roster(
    debaters: &[DebaterParts],
    synthesizer: &SynthesizerParts,
    max_rounds: u32,
) -> Result<(), Error> {
    if debaters.len() != discussion::DEBATERS {
        return Err(Error::Discussion(format!(
            "v1 固定跑 {} 位讨论者，拿到的是 {}；再多就要重开 \
             「N = 2 does not arbitrate」那个决定（spec §15，Out of Scope）",
            discussion::DEBATERS,
            debaters.len()
        )));
    }
    for debater in debaters {
        if !matches!(debater.speaker, SpeakerId::Debater(_)) {
            return Err(Error::Discussion(format!(
                "讨论者必须以讨论者的身份发言，拿到的是 {}",
                debater.speaker
            )));
        }
    }
    // 两个讨论者就是两个参与者。每一份投影都是 `speaker_id` 的函数，所以两个讨论者共用
    // 一个名字会把对方的作答当成自己的发给各自那一方（spec §5）—— 在定向第二轮里最
    // 显眼，那一轮的全部意义就是讨论者看到*对方*的回答。名册里重复一个模型没问题；重复
    // 一个*身份*不行。
    let mut speakers = BTreeSet::new();
    for debater in debaters {
        if !speakers.insert(debater.speaker.to_string()) {
            return Err(Error::Discussion(format!(
                "两位讨论者共用了 speaker id {}；一次讨论需要两个身份，\
                 否则两边的消息分不开",
                debater.speaker
            )));
        }
    }
    if max_rounds == 0 {
        return Err(Error::Discussion("一次讨论至少要有一轮".to_owned()));
    }

    // token 额度是**会话**级事实，由两个讨论者、合成器与它们派出的每一个执行者共享
    // （spec §17），所以一份在这一点上不一致的名册是组装错误，而不是悄悄挑出来的一个
    // 赢家。
    let budget = debaters[0].config.budget.clone();
    for debater in debaters.iter().skip(1) {
        if debater.config.budget != budget {
            return Err(Error::Discussion(format!(
                "{} 与 {} 拿到了不同的 token 额度；额度是整个会话共用\
                 的一个值（spec §17）",
                debaters[0].speaker, debater.speaker
            )));
        }
    }
    if synthesizer.config.budget != budget {
        return Err(Error::Discussion(
            "合成器拿到的 token 额度与讨论者不同；额度是整个会话共用的\
             一个值（spec §17）"
                .to_owned(),
        ));
    }

    // 出于同样的理由，打码器也是会话级事实，而不一致比预算不一致更糟：同一个参与者的
    // 事件会被打码、另一个的不会，而且是在**同一条**流上，事后没有任何迹象（spec §20）。
    // `Config::session_config` 把同一个值填进每一份 config，所以不一致就意味着有人手搓了
    // 一份。
    let redactor = debaters[0].config.redactor.clone();
    for debater in debaters.iter().skip(1) {
        if debater.config.redactor != redactor {
            return Err(Error::Discussion(format!(
                "{} 与 {} 拿到了不同的打码器；要打掉的值是整个会话共用\
                 的一套（spec §20）",
                debaters[0].speaker, debater.speaker
            )));
        }
    }
    if synthesizer.config.redactor != redactor {
        return Err(Error::Discussion(
            "合成器拿到的打码器与讨论者不同；要打掉的值是整个会话共用\
             的一套（spec §20）"
                .to_owned(),
        ));
    }
    Ok(())
}

/// 一次讨论的三个参与者，落在 `fork` 够得到的那条流上。
///
/// `fork` 就是组装一场讨论的两条路径之间的全部差别：一场自己的讨论从它刚建好的脚手架开
/// 会话，一场跑在活动会话上的讨论则分叉出用户所在的那场会话。绝不能有差别的东西在这里
/// 定下 —— 每个讨论者的私有身份，以及合成器的落点（spec §15、§17）。
fn discussion_participants(
    debaters: Vec<DebaterParts>,
    synthesizer: SynthesizerParts,
    fork: impl Fn(SessionConfig, Option<String>) -> Session,
) -> (Vec<agent::Debater>, agent::Synthesizer) {
    let roster = debaters
        .into_iter()
        .map(|part| {
            let DebaterParts {
                speaker,
                config,
                provider,
                soul,
            } = part;
            let identity = discussion::debater_identity(speaker.to_string().as_str());
            agent::Debater {
                speaker,
                session: fork(config, Some(identity)),
                provider: provider.into(),
                soul,
            }
        })
        .collect();

    let SynthesizerParts {
        mut config,
        provider,
    } = synthesizer;
    // 合成器是更便宜的模型可以被改派过去的另一个落点（spec §17）。上面的讨论者直接由
    // 各自的 config 组装出来、从不经过这条规则，这就是「讨论者绝不是落点」在结构上的
    // 含义。
    config.model = config
        .model_for(config::LandingPoint::Synthesizer)
        .to_owned();
    let synthesizer = agent::Synthesizer {
        session: fork(config, Some(discussion::synthesizer_identity())),
        provider,
    };
    (roster, synthesizer)
}

/// 组装一场讨论：一条流上两个讨论者，加上合成器。
///
/// 和 [`assemble`] 一样不读任何环境。讨论者就是同一个会话脚手架开两次，所以它们共享
/// 日志、工具、路径锁与权限策略；不同的是它们的发言归属、模型与私有身份（spec §15）。
pub async fn assemble_discussion(parts: DiscussionParts) -> Result<DiscussionHarness, Error> {
    let DiscussionParts {
        scaffold,
        debaters,
        synthesizer,
        max_rounds,
        renderer,
    } = parts;

    let max_rounds = max_rounds.unwrap_or(discussion::DEFAULT_MAX_ROUNDS);
    validate_roster(&debaters, &synthesizer, max_rounds)?;
    roster_needs_a_sandbox(
        scaffold.policy.mode(),
        &debaters,
        &synthesizer,
        &scaffold.cwd,
    )?;
    // 名册刚被校验过，至少两位讨论者，而它们是同一个脚手架开出来的。
    let workspace_available =
        sandbox_unavailable_reason(&debaters[0].config.sandbox, &scaffold.cwd).is_none();

    let opened = OpenedSession::open(scaffold, renderer)?;

    let (mut roster, synthesizer) =
        discussion_participants(debaters, synthesizer, |config, identity| {
            opened.session(config, identity)
        });
    // 第一场会话为整条流记下骨架，或者在继续时收尾更早那个进程留下没答完的调用：它就是
    // 每个讨论者随后要投影的那个会话级头部。
    if let Some(first) = roster.first_mut() {
        opened.start(&mut first.session)?;
    }

    let policy = Arc::clone(&opened.policy);
    Ok(DiscussionHarness {
        discussion: agent::Discussion::new(roster, synthesizer, max_rounds),
        log: opened.log.clone(),
        render: opened.render,
        cancel: CancelSignal::new(),
        policy,
        workspace_available,
        render_task: opened.render_task,
    })
}

impl Harness {
    /// 记下一条用户消息，并把一个回合跑到结束。
    pub async fn run_turn(&mut self, user_input: &str) -> Result<TurnOutcome, Error> {
        agent::record_user_message(&mut self.session, &self.render, user_input)?;
        self.drive_turn().await
    }

    /// 一个回合 —— 从它起点的那点东西已经在流上之后算起。
    ///
    /// 真正驱动一个回合的唯一地方。两条入口 —— 一条打进来的提示与一个裸技能 —— 只在
    /// 它们先追加什么上有差别，所以它们不可能在「一个回合*是*什么」上漂开。
    async fn drive_turn(&mut self) -> Result<TurnOutcome, Error> {
        // 手势的范围是一次运行：按下去停掉上一个回合的那一下不能停掉这一个，否则一次被
        // 取消的会话在同一进程里就再也不能用了（spec §6）。
        self.cancel.reset();
        // 这个回合对手势的看法。
        let cancelled = self.cancel.observer();
        agent::run_turn(
            &mut self.session,
            &self.speaker,
            &self.provider,
            &self.render,
            agent::TurnScope::Whole,
            &cancelled,
        )
        .await
    }

    /// 在**这场会话的流上**跑一次讨论（spec §15）。
    ///
    /// 讨论者与合成器是这场会话的*兄弟*：它们共享它的日志、工具表、写锁、权限策略、
    /// 作答者、技能与渲染器，只在各自的模型与私有身份上不同。两条后果才是重点：
    ///
    /// * 投影把**这个 agent 的**回合变成它们眼里的 `user` 消息（spec §5），于是一次
    ///   讨论继承这场会话的上下文 —— 它争论的是这场会话在谈什么，而不是真空里的一个
    ///   问题；
    /// * 轮次追加在同一条流上，接在它已有的内容之后，于是一场会话可以装一个回合、一次
    ///   讨论、再来一个回合 —— 而 `sessions show` 把整段读回来。
    ///
    /// 名册的校验与 [`assemble_discussion`] 一模一样。这场会话**没有**开出的流（一场
    /// 全新的、自己的讨论）走 [`assemble_discussion`]：那条路径记骨架，这条往一个骨架
    /// 已经躺在它上面的流里追加。
    pub async fn discuss(
        &mut self,
        question: &str,
        debaters: Vec<DebaterParts>,
        synthesizer: SynthesizerParts,
        max_rounds: Option<u32>,
    ) -> Result<agent::DiscussionOutcome, Error> {
        let max_rounds = max_rounds.unwrap_or(discussion::DEFAULT_MAX_ROUNDS);
        validate_roster(&debaters, &synthesizer, max_rounds)?;
        let mode = self.policy.lock().expect("策略互斥锁已中毒").mode();
        let cwd = self.session.cwd().to_path_buf();
        roster_needs_a_sandbox(mode, &debaters, &synthesizer, &cwd)?;
        let (roster, synthesizer) =
            discussion_participants(debaters, synthesizer, |config, identity| {
                self.session.fork(config, identity)
            });
        // 没有骨架、也没有恢复：这条流是活的，它的 `SessionStarted` 就在头部。
        let mut discussion = agent::Discussion::new(roster, synthesizer, max_rounds);
        // 一次讨论就是一次运行，像一个回合：手势从干净的地方开始。
        self.cancel.reset();
        let cancelled = self.cancel.observer();
        agent::run_discussion(&mut discussion, &self.render, question, &cancelled).await
    }

    /// **用户**在这场会话里最后问的那句话，或者 `None`。
    ///
    /// 一个裸 `/discuss` 讨论的就是它：「我们刚才在聊的那件事」才是值得摆给两个模型的
    /// 问题。只有 user 角色的 `MessageCompleted` 算数 —— 一条上下文注入也归属于用户，
    /// 但它不是一个问题。
    pub fn last_question(&self) -> Option<String> {
        self.session
            .events()
            .iter()
            .rev()
            .find_map(|event| match &event.payload {
                EventPayload::MessageCompleted {
                    role: Role::User,
                    text,
                    ..
                } => Some(text.clone()),
                _ => None,
            })
    }

    /// 这场会话自己那一端的取消手势（spec §6）。
    ///
    /// 前端持有它并在 Esc 时举起；当 [`is_cancelled`](CancelSignal::is_cancelled) 已经
    /// 为真时再按一次，由前端把它变成一个退出，因为会话本身永远不需要知道自己是怎么被
    /// 杀的 —— `--continue` 会把进程留下没答完的收尾。
    pub fn cancel_signal(&self) -> CancelSignal {
        self.cancel.clone()
    }

    /// 发现到的每一个技能名，按优先级顺序：前端把它作为 `/<name>` 提供出来。
    pub fn skill_names(&self) -> Vec<&str> {
        self.session.skills().names()
    }

    /// 发现到的每一个技能，按 `(名字, 描述)`：前端的 `/` 菜单提供这些，带上说明这个技能
    /// 是干什么的那一行。
    ///
    /// 它保留 `disable-model-invocation` 的技能，因为菜单是用户的那张列表 —— 见
    /// [`crate::context::skills::Skills::entries`]。
    pub fn skill_catalog(&self) -> Vec<(&str, &str)> {
        self.session.skills().entries()
    }

    /// `/<name>` 是否指到一个发现到的技能。
    pub fn has_skill(&self, name: &str) -> bool {
        self.session.skills().get(name).is_some()
    }

    /// 把**用户**点名的技能（spec §9）追加到上下文尾部。
    ///
    /// 这正是 `disable-model-invocation: true` 保留的那次调用：这样的技能不在技能清单
    /// 里，[`Skills::load`] 也拒它，所以一个猜出名字的模型仍然够不到它 —— 用户够得到。
    /// 正文是一条 `ContextInjected { source: Skill }` 事件，它投影成当前历史之后的一条
    /// 独立 `user` 消息，于是缓存前缀永远不动。用它跑那个回合的是调用方。
    pub fn load_skill(&mut self, name: &str) -> Result<(), Error> {
        let body = self
            .session
            .skills()
            .invoke(name)
            .map_err(|error| Error::Skill(error.to_string()))?;
        agent::record_context_injection(
            &mut self.session,
            &self.render,
            ContextSource::Skill,
            &body,
        )
    }

    /// 跑一个裸 `/<skill>` 要的那个回合（spec §9）：载入技能，然后拿它作为模型看到的
    /// 最后一样东西去调模型。
    ///
    /// 刻意**没有合成的用户消息**。技能正文投影之后本来就是一条 `user` 消息，所以再追加
    /// 一条的回合等于替用户说话 —— 转录里会出现一句他们从没打过的提示，而模型会去答它而
    /// 不是答技能。这就是裸形式不是 `load_skill` 之后跟一个 `run_turn("")` 的原因：空
    /// 消息也是一条消息。
    pub async fn run_skill(&mut self, name: &str) -> Result<TurnOutcome, Error> {
        self.load_skill(name)?;
        self.drive_turn().await
    }

    /// 这场会话眼下跑在哪一档。
    pub fn mode(&self) -> Mode {
        self.session.mode()
    }

    /// 这场会话自己那一端的模式循环手势（spec §12）。
    ///
    /// 这个手势是策略上的一个值、永远不是一条事件，也什么都不注入。模型是在自己某次调用
    /// 被拒时、从 `PermissionDecided` 的 reason 里才知道立场变了 —— 这是刻意的代价，
    /// 因为往 `messages` 头部注入一行会在每一次按下时把前缀缓存整个扔掉（ADR 0003）。
    pub fn mode_cycle(&self) -> ModeCycle {
        ModeCycle {
            policy: Arc::clone(&self.policy),
            workspace_available: self.workspace_available(),
        }
    }

    /// 这一刻这台机器上有没有可用的沙箱（组装期已经定下来，探测只跑一次）。
    fn workspace_available(&self) -> bool {
        sandbox_unavailable_reason(&self.session.config().sandbox, self.session.cwd()).is_none()
    }

    pub fn session_id(&self) -> &SessionId {
        self.session.id()
    }

    /// 这场会话的工作目录（`--cwd` 已经在这一层解析过），于是调用方不必去问进程的 cwd。
    pub fn cwd(&self) -> &Path {
        self.session.cwd()
    }

    /// 对前端说一句不属于任何事件的话：启动横幅，以及交互式循环那些朴素的反馈。
    ///
    /// 它走渲染通道而不是直接写终端，因为从组装那一刻起终端归渲染器：第二个写者会插进
    /// 那个活着的区域里（spec §19）。
    pub fn notice(&self, message: &str) {
        self.render.notice(message);
    }

    /// 目前为止的整条流，按 `seq` 顺序。
    ///
    /// [`DiscussionHarness::events`] 的单 agent 对应物：两个 harness 一个取值形状，
    /// 于是调用方（或测试）拿哪一个都用同样的方式读这场会话的真相源。
    pub fn events(&self) -> Vec<Event> {
        self.session.events()
    }

    /// 回滚这场会话最近一次 `edit_file`：把它替换掉的字节还原回来，并退掉它的事件
    /// （spec §11）。
    ///
    /// `Ok(None)` 表示已经没有可撤销的东西了。手势只在前端存在；这是 `/undo` 调的那个
    /// 操作，而它永远不碰用户的 git。
    pub async fn undo_last_edit(&mut self) -> Result<Option<agent::UndoOutcome>, Error> {
        agent::undo_last_edit(&mut self.session, &self.render).await
    }

    /// 这场会话的工具产物落在哪里（`outputs/<tool_call_id>.*`）。
    pub fn outputs_dir(&self) -> &std::path::Path {
        self.session.outputs_dir()
    }

    /// 丢掉渲染通道，并等每一条缓冲中的渲染事件都写进接收端。对捕获的接收端做断言之前
    /// 调它。
    pub async fn shutdown(self) {
        drain_renderer(self.render, self.render_task).await;
    }
}

impl DiscussionHarness {
    /// 把问题摆给讨论者，把协议跑到结束。
    ///
    /// 一个 harness 就是一次讨论：轮次号是按讨论算的，所以在一个 harness 上问第二次会
    /// 让它们从一重新开始。第二个问题要另建一个 harness。
    pub async fn discuss(&mut self, question: &str) -> Result<agent::DiscussionOutcome, Error> {
        // 一次讨论就是一次运行，像一个回合：手势从干净的地方开始。
        self.cancel.reset();
        let cancelled = self.cancel.observer();
        agent::run_discussion(&mut self.discussion, &self.render, question, &cancelled).await
    }

    /// 讨论自己那一端的取消手势（spec §6），两个讨论者与它们派出的每一个执行者共享。
    pub fn cancel_signal(&self) -> CancelSignal {
        self.cancel.clone()
    }

    /// 讨论自己那一端的模式循环手势（spec §12）：一份策略盖住三个参与者，所以按一次
    /// 三个一起动。
    pub fn mode_cycle(&self) -> ModeCycle {
        ModeCycle {
            policy: Arc::clone(&self.policy),
            workspace_available: self.workspace_available,
        }
    }

    pub fn session_id(&self) -> &SessionId {
        self.discussion.session_id()
    }

    /// 目前为止的整条流，按 `seq` 顺序。
    pub fn events(&self) -> Vec<Event> {
        self.log.events()
    }

    pub fn log_path(&self) -> &Path {
        self.log.path()
    }

    /// 丢掉渲染通道，并等每一条缓冲中的渲染事件都写进接收端。对捕获的接收端做断言之前
    /// 调它。
    pub async fn shutdown(self) {
        drain_renderer(self.render, self.render_task).await;
    }
}

/// 这场会话自己那一端的模式循环手势（spec §12；`Shift+Tab`）。
///
/// 一个**句柄**，和 [`CancelSignal`] 一样，理由也一样：手势必须在一个钉住的 run future
/// 借着 harness 的时候够到策略 —— 循环能持有它，因为它是在那次借用之前克隆下来的。一条
/// 流上的每一场会话都共享它指向的那份策略，所以按一次动的是整场讨论对写的立场，而那就是
/// 模式该有的东西。
#[derive(Clone)]
pub struct ModeCycle {
    policy: Arc<Mutex<Policy>>,
    /// 这台机器上有没有可用的沙箱。没有时 `workspace` 档在循环里被**跳过**：组装期已经
    /// 拒过一次，而 `Shift+Tab` 是这一档的第二个入口 —— 「没有沙箱就没有这一档」要在两处
    /// 是同一个答案（`.scratch/workspace-mode/spec.md` §6）。
    workspace_available: bool,
}

impl ModeCycle {
    /// 绕循环走一档，并返回此刻生效的模式：于是前端显示的那个值就是权限门读的值。
    pub fn cycle(&self) -> Mode {
        let mut policy = self.policy.lock().expect("策略互斥锁已中毒");
        let mut mode = policy.mode().next();
        if mode == Mode::Workspace && !self.workspace_available {
            mode = mode.next();
        }
        policy.set_mode(mode);
        mode
    }
}

/// 丢掉渲染通道，并等每一条缓冲中的渲染事件都到达接收端。两个 harness 以同样的方式
/// 收尾；一个进程一个渲染器才是「字面上就是同一个操作」的来由。
async fn drain_renderer(render: RenderHandle, render_task: JoinHandle<()>) {
    drop(render);
    let _ = render_task.await;
}

/// 停下组装或循环本身的错误。provider 的失败由循环变成 `TurnEnded { Error }`，不是这个
/// 类型。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("事件流 i/o 错误：{0}")]
    Io(#[from] io::Error),
    /// 用来组装一次讨论的那份名册跑不了协议。
    #[error("讨论组装：{0}")]
    Discussion(String),
    /// `/undo` 不能安全地把工作区回滚。
    #[error("撤销：{0}")]
    Undo(String),
    /// 用户点名的技能载入不了。
    #[error("技能加载：{0}")]
    Skill(String),
    /// `workspace` 档在没有可用沙箱的地方不存在（`.scratch/workspace-mode/spec.md` §6）。
    #[error("权限模式：{0}")]
    WorkspaceWithoutSandbox(String),
}
