//! `Session` 值：唯一持有可变状态的结构。
//!
//! 它持有事件流句柄、会话身份、注入的配置、工具表、共享的路径锁、这个 agent 的读集合、这个 agent
//! 的私有身份，以及会话的权限策略。名册不在这里：一场讨论的讨论者各自是一个 `Session`，并排组装
//! 并共用一个日志，而执行者是一个嵌套 `Session`，它的事件仍然追加到父级的流上，工具表与路径锁也
//! 是同一批值。
//!
//! `Session` 从不自己发起写入：它把日志句柄交出去，而 `agent::append_event` 是唯一的写入路径，所
//! 以 `agent` 层是事件流的唯一写者；工具、钩子、权限与讨论都写不了。
//!
//! [`store`] 是它在磁盘上的对应物：会话目录住在哪里、`--continue` 怎么找到它、`prune` 怎么删掉
//! 它。[`ledger`] 读的正是这些目录，用来回答单场会话答不了的那个问题：某个 UTC 自然日花掉了厂商
//! 滚动配额窗口的多少（spec §17）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub mod ledger;
pub mod observe;
pub mod store;

pub use ledger::DayLedger;
pub use store::{SessionStore, StoredSession, new_session_id};

use crate::config::SessionConfig;
use crate::context::skills::Skills;
use crate::events::{Event, EventLog, Redactor, SessionId};
use crate::hooks::Hook;
use crate::permissions::{Asker, Mode, Policy, Rule};
use crate::questions::UserQuestions;
use crate::tools::{PathLocks, ReadSet, Registry, SessionPaths};

/// 组装一场会话所需的全部东西。它是注入的，库从不自己从环境里读：权限策略、权限门答 `Ask` 时用
/// 的询问端口，以及用户的家目录（只有 `rm` 断路器读它）都从这里进来。
///
/// 工具表与策略以共享句柄的形式进来，因为一场讨论同时有不止一场会话（spec §15）：两个讨论者必须
/// 派发进**同一批**工具、用**同一批**逐路径锁，而其中一个挣到的会话级许可必须到达另一个。
pub struct SessionParts {
    pub id: SessionId,
    pub cwd: PathBuf,
    pub log: EventLog,
    pub config: SessionConfig,
    /// 这场会话的工具表。
    pub tools: Arc<Registry>,
    /// 逐路径写锁。**同一张**表必须到达每个执行者，否则写互斥就成了按会话算，也就根本不算锁。
    pub locks: PathLocks,
    /// 这场会话的工具产物（`outputs/<tool_call_id>.*`）落在哪里。
    pub outputs_dir: PathBuf,
    /// 会话的权限策略：一个模式加上它的规则。
    pub policy: Arc<Mutex<Policy>>,
    /// 权限门答 `Ask` 时循环去问的端口。`None` 表示没有交互式应答者，循环会把 `Ask` 降级为
    /// `Deny`。
    pub asker: Option<Arc<dyn Asker>>,
    /// 模型发起的提问所走的端口（spec §7）。`None` 表示没有问卷应答者，于是
    /// `ask_user_question` 报告这一点而不是挂住；那种情况下工具表也不带这个工具。
    pub questions: Option<Arc<dyn UserQuestions>>,
    /// 挂在两个工具调用钩子点上的策略。`None` 表示循环不调任何钩子，也不追加 `HookExecuted`
    /// 事件。
    pub hook: Option<Arc<dyn Hook>>,
    /// 用户的家目录，已知时。
    pub home: Option<PathBuf>,
    /// 组装时发现的技能（spec §9）。与每一场嵌套会话共享，所以执行者看到的名录和它的父级一样。
    pub skills: Arc<Skills>,
    /// 这个 agent 的私有身份：给它的那条 `system` 消息，以及关于它、却永不进入事件流的那一件事
    /// （spec §15）。讨论的协议指令正住在这里，好让一轮的 `messages` 仍能从流重算出来。
    pub identity: Option<String>,
}

pub struct Session {
    id: SessionId,
    cwd: PathBuf,
    log: EventLog,
    config: SessionConfig,
    tools: Arc<Registry>,
    locks: PathLocks,
    paths: SessionPaths,
    outputs_dir: PathBuf,
    /// 这个 agent 读过的路径。从不继承：读权限是每个 agent 各自的。
    read_set: ReadSet,
    /// 会话的权限策略。一个值，绝不是事件：`--continue` 会回到配置里的那一档（spec §12）。共享，
    /// 因为某个讨论者回合里挣到的会话许可是一件属于整场会话的事实。
    policy: Arc<Mutex<Policy>>,
    /// 询问端口，与任何嵌套会话共享，好让执行者通过同一个渲染器提问。
    asker: Option<Arc<dyn Asker>>,
    /// 问卷端口，与任何兄弟会话共享，理由与询问端口相同：同一个键盘为整场会话作答。
    questions: Option<Arc<dyn UserQuestions>>,
    /// 钩子策略，与任何嵌套会话共享，好让执行者逃不出约束它父级的那个策略（spec §16）。
    hook: Option<Arc<dyn Hook>>,
    home: Option<PathBuf>,
    /// 发现到的技能库，内建 `skill` 工具读它（spec §9）。
    skills: Arc<Skills>,
    /// 这个 agent 的私有身份；没有给它身份时是 `None`。从不追加进日志：它是请求的一项输入，而事
    /// 件流不承载它（spec §15）。
    identity: Option<String>,
}

impl Session {
    /// 包住一份刚建好的日志。记录 `SessionStarted` 是 `agent` 模块的事，这样所有写入都留在一处。
    pub fn new(parts: SessionParts) -> Self {
        let SessionParts {
            id,
            cwd,
            log,
            config,
            tools,
            locks,
            outputs_dir,
            policy,
            asker,
            questions,
            hook,
            home,
            skills,
            identity,
        } = parts;
        let paths = SessionPaths::new(&cwd);
        Self {
            id,
            cwd,
            log,
            config,
            tools,
            locks,
            paths,
            outputs_dir,
            read_set: ReadSet::default(),
            policy,
            asker,
            questions,
            hook,
            home,
            skills,
            identity,
        }
    }

    /// 这场会话事件的一份快照，按序。
    ///
    /// 快照而不是借用：日志是共享句柄，两个讨论者的回合可能在两次读之间往它上面追加，没有任何借用
    /// 能跨越那段时间。
    pub fn events(&self) -> Vec<Event> {
        self.log.events()
    }

    /// 会话的事件流。这里只读：投影把流当输入，只有 `agent` 模块往它上面追加。
    pub fn log(&self) -> &EventLog {
        &self.log
    }

    pub fn id(&self) -> &SessionId {
        &self.id
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// 把「别处已经花掉的 token」填进这场会话的额度口径
    /// （`.scratch/goal-loop/spec.md` §8）：翻页开的新会话带着同一个累计继续。
    pub fn carry_usage(&mut self, tokens: u64) {
        self.config.carried_tokens = tokens;
    }

    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// 会话中途换模型与档位：换掉这一场的配置，其余（流、工具表、策略、读集、取消信号）原样。
    ///
    /// **只有 [`crate::Harness`] 调它** —— 它是 `/model`、`/effort`、点状态行与快捷键四个入口
    /// 的共同落点，渲染器碰不到，也不该碰（spec §2）。字段保持私有而不改成 `pub`，是为了让
    /// 「换了配置」这件事永远与「换了 provider」一起发生（[`crate::Harness::switch_model`]）：
    /// 只改一半会让下一次请求拿新 id 去问旧 caps。
    pub fn retarget(&mut self, config: SessionConfig) {
        let mut config = config;
        // 沙箱是**会话**的事实：`retarget_to` 拿的是 `Config::session_config` 造的新配置，那
        // 一格还没探过。不带上它，换一次模型就会让 `bash` 从那一刻起永久拒绝，理由还是一句
        // 与这台机器无关的「沙箱状态还没有定下来」（2026-10-08 实测到的那次）。
        self.inherit_sandbox(&mut config);
        self.config = config;
    }

    /// 把这场会话已经定下来的沙箱那一格带进一份**新造**的配置里。
    ///
    /// 沙箱是会话的事实，不是每个 agent、也不是每次换模型的（`.scratch/sandbox/spec.md` §3）：
    /// 组装期探过一次就不再探第二次（库不读进程环境）。而 `Config::session_config` 造出来的每
    /// 一份新配置里，那一格都是解析期的 `Untested` —— 所以**每一条**新造配置的路径都要经这里，
    /// 否则它的去处第一次调 `bash` 就撞「沙箱状态还没有定下来」。讨论者（[`Self::fork`]）与
    /// 会话中途换模型（[`Self::retarget`]）都在这条路上。
    fn inherit_sandbox(&self, config: &mut SessionConfig) {
        if config.sandbox.needs_probe() {
            config.sandbox = self.config.sandbox.clone();
        }
    }

    /// 这场会话在文本进入事件流的路上会打码掉的那些值（spec §20）。
    ///
    /// 由 [`SessionConfig`] 持有，因为 `Config::session_config` 是配置变成注入值的唯一一处；这个
    /// 访问器让唯一写入路径与离开 harness 的文本都能拿到它，而不必亲自走进配置的字段里。
    pub fn redactor(&self) -> &Redactor {
        &self.config.redactor
    }

    /// 用这场会话的值给 `text` 打码（spec §20）。
    ///
    /// 离开 harness 的文本 —— 即将落盘的工具结果、一个回合的收尾、合成器的产出 —— 走这里、而不是
    /// 走流的写入路径，这样两边携带的是同一份文本。
    pub fn redacted(&self, text: &str) -> String {
        self.config.redactor.redacted(text)
    }

    pub fn log_path(&self) -> &Path {
        self.log.path()
    }

    /// 这场会话的工具表。
    pub fn tools(&self) -> &Registry {
        &self.tools
    }

    /// 以共享句柄的形式给出的工具表。
    ///
    /// 用于必须活过对本会话的借用的工作：循环那一批推迟的执行者调用，在它仍需要会话记录结果的同时
    /// 通过这张表派发（spec §16）。
    pub fn shared_tools(&self) -> Arc<Registry> {
        Arc::clone(&self.tools)
    }

    /// 这场会话的工具产物（`outputs/<tool_call_id>.*`）落在哪里。
    pub fn outputs_dir(&self) -> &Path {
        &self.outputs_dir
    }

    pub fn paths(&self) -> &SessionPaths {
        &self.paths
    }

    pub fn path_locks(&self) -> &PathLocks {
        &self.locks
    }

    /// 会话的权限策略。
    ///
    /// 按值取快照：策略是共享的，所以权限门取的是它据以裁决的那个值，而不是在一次交互式询问期间一
    /// 直持着锁。
    pub fn policy(&self) -> Policy {
        self.policy.lock().expect("策略互斥锁中毒").clone()
    }

    /// 记住一条会话级许可。它只改策略这个值：不写 `config.toml`、也不追加事件（spec §12）。
    pub fn remember_allow(&mut self, rule: Rule) {
        self.policy.lock().expect("策略互斥锁中毒").push(rule);
    }

    /// 这场会话当前跑在哪一档。
    pub fn mode(&self) -> Mode {
        self.policy.lock().expect("策略互斥锁中毒").mode()
    }

    /// 换掉会话的模式，保留它的规则。这是模式循环手势对策略的唯一作用 —— 一个值，绝不是事件，这
    /// 也正是 `--continue` 从配置里的那一档开始的原因（spec §12）。
    pub fn set_mode(&self, mode: Mode) {
        self.policy.lock().expect("策略互斥锁中毒").set_mode(mode);
    }

    /// 这个 agent 的私有身份，如果有的话。
    ///
    /// 它作为开头那条 `system` 消息到达 provider，永不进入事件流（spec §15）。
    pub fn identity(&self) -> Option<&str> {
        self.identity.as_deref()
    }

    /// 同一份流上的**兄弟**会话：每个会话级的值都共享 —— 事件流、工具表、写锁、权限策略、应答者、
    /// 钩子、家目录与发现到的技能 —— 只有 agent 自己的值不同（它的模型、它的生成参数、它的私有身
    /// 份）。
    ///
    /// 这正是讨论能**跑在一场活会话上**的原因（spec §15）：它的讨论者是用户所在那场会话的兄弟，所
    /// 以它们的投影把那场会话的回合变成 `user` 消息，而它们的轮次追加到同一份流上。读集合刻意
    /// **不**继承 —— 读权限是每个 agent 各自的（spec §12），而讨论者什么都没读过。
    ///
    /// 沙箱那一格经 [`Self::inherit_sandbox`] 带过去：它是**会话**的事实，不是每个 agent
    /// 各自的，而 fork 进来的那份配置（`cli` 用 `Config::session_config` 造出来的）里还没探过。
    pub(crate) fn fork(&self, mut config: SessionConfig, identity: Option<String>) -> Self {
        self.inherit_sandbox(&mut config);
        Self::new(SessionParts {
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

    /// 询问端口，如果这场会话有交互式应答者。
    pub fn asker(&self) -> Option<&Arc<dyn Asker>> {
        self.asker.as_ref()
    }

    /// 问卷端口，如果这场会话能把模型发起的提问交给用户（spec §7）。循环把它拷进每次调用的派发上
    /// 下文中，`ask_user_question` 工具在那里读它。
    pub fn questions(&self) -> Option<&Arc<dyn UserQuestions>> {
        self.questions.as_ref()
    }

    /// 钩子策略，如果挂了一个。
    pub fn hook(&self) -> Option<&Arc<dyn Hook>> {
        self.hook.as_ref()
    }

    /// 用户的家目录，在它被注入时。
    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    /// 发现到的技能库（spec §9）。`skill` 工具通过派发上下文读它；技能清单的注入在组装时由它算
    /// 出。
    pub fn skills(&self) -> &Arc<Skills> {
        &self.skills
    }

    /// 这个 agent 的读集合。`read-before-edit` 会查它；一次失败的比对会把一个路径从它里面撤掉，
    /// 而一次读会加进一个。
    pub fn read_set(&self) -> &ReadSet {
        &self.read_set
    }

    /// 记录这个 agent 读过的路径。
    ///
    /// 循环在调用完成后调它，而不是在派发内部，这样读集合永远不必与它正在派发进去的那张工具表同时
    /// 被可变借用。
    pub fn record_reads(&mut self, paths: &[std::path::PathBuf]) {
        self.read_set.record_all(paths.iter().cloned());
    }

    /// 撤销这个 agent 对某一个路径的读权限。
    pub fn invalidate_read(&mut self, path: &Path) {
        self.read_set.invalidate(path);
    }
}
