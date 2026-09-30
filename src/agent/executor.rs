//! 执行者：讨论者通过 `task` 派发出来的嵌套会话（spec §16）。
//!
//! 它是 `agent` 的子模块，而不是自己的一道边界，因为跑一个执行者**就是**控制流 —— 它驱动
//! 一次 [`run_turn`] —— 而 `agent` 层要保持是事件流的唯一写者、也是 provider 的唯一调用者
//! （spec §1、§3）。住在这里的，是「对执行者成立、对讨论者不成立」的那一切：它的私有身份、
//! 循环交给 `task` 的那个端口、从简报进到报告的管路，以及报告的元数据所派生的那些流查询。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::config::{LandingPoint, SessionConfig};
use crate::context::skills::Skills;
use crate::events::{
    usage_of, Event, EventLog, EventPayload, ParticipantId, SessionId, SpeakerId, StopReason, Usage,
};
use crate::hooks::Hook;
use crate::permissions::{Asker, Policy};
use crate::provider::Provider;
use crate::render::RenderHandle;
use crate::session::{Session, SessionParts};
use crate::tools::file::WROTE_PATH_PREFIX;
use crate::tools::{ExecutorSpawner, PathLocks, Registry, ToolError, ToolOutput};
use crate::Error;

use super::{append_event, run_turn, CancelObserver, TurnScope};

/// 执行者的私有身份（spec §16）。
///
/// 与讨论者的协议指令一样，它从不进事件流：它是流不携带的那个请求输入之一（spec §15）。
/// 它不含任何讨论协议 —— 执行者不辩论，它干活。
const EXECUTOR_IDENTITY: &str = "你是一个执行者。另一个 agent 通过 `task` 工具把你派到这个仓库里，\
     做一件工作，而你有自己的上下文：本仓库的规则，加上你的简报 —— 它就是最后那条 user 消息。没有人\
     看得见你的步骤 —— 你的工具调用看不见，它们的输出也看不见 —— 所以一直干到任务完成，然后用一份\
     简短报告回答：你做了什么、你发现了什么、以及派发者必须知道的事情。那份报告就是回传的全部。\
     你不能再派发执行者。任务做不了就直说，并解释为什么，不要猜。";

/// 执行者请求里打头的那条 `system` 消息：它的身份，加上那段通用条款。
///
/// 拼 [`THINKING_IN_CHINESE`] 的理由与另外三个身份一样：执行者的思考也进流、也进转录的详情
/// 弹窗（TUI 里那条 `[执行者] ▸ ✓ 思考完成`）。做成函数而不是第二个常量，是因为 `concat!` 只吃
/// 字面量，而这一句必须只有一处真相。
pub(super) fn executor_identity() -> String {
    [EXECUTOR_IDENTITY, "\n\n", super::THINKING_IN_CHINESE].concat()
}

/// 跑一个嵌套执行者的端口（spec §16）：[`ExecutorSpawner`]，由循环构造、经工具上下文交给
/// `task`。
///
/// 它给派发中的会话拍一张快照，这样执行者可以在那个会话正忙着自己被派发的时候跑：所有共享的
/// 东西（日志、路径锁、询问端口、钩子、技能库）都是句柄，而所有按 agent 分的东西（读集合、
/// 身份、策略）要么是新的、要么是派生的。唯一没法快照的是 provider —— 它以共享句柄的形式
/// 传进来，因为执行者在派发者的 client 上作答。
pub(super) struct ExecutorPort {
    /// 谁派发的它：`ExecutorSpawned` 记为 `parent` 的那个发言者。
    parent: SpeakerId,
    executor_id: ParticipantId,
    cwd: PathBuf,
    log: EventLog,
    session_id: SessionId,
    outputs_dir: PathBuf,
    locks: PathLocks,
    /// 执行者自己的策略：它自己的模式，加上父会话里**会传播**的那些规则，仅此而已 ——
    /// 于是它继承拒绝与询问，从不继承放行（spec §12、§16）。
    policy: Policy,
    asker: Option<Arc<dyn Asker>>,
    hook: Option<Arc<dyn Hook>>,
    home: Option<PathBuf>,
    skills: Arc<Skills>,
    /// 执行者的工具表：会话那张表减掉不可委派的部分，所以 `task` 在构造上就不在其中
    /// （spec §16）。
    tools: Arc<Registry>,
    /// 执行者自己的值：继承来的模型，加上它自己的回合上限。
    config: SessionConfig,
    provider: Arc<dyn Provider>,
    render: RenderHandle,
    /// 派发者手里那个取消手势的视图（spec §6）：执行者观察的是**同一个**手势，所以一次
    /// 按下能到达它下面整条链。它是观察端而不是信号端 —— 执行者取消不了它的派发者。
    cancelled: CancelObserver,
}

impl ExecutorPort {
    pub(super) fn new(
        session: &Session,
        parent: &SpeakerId,
        provider: &Arc<dyn Provider>,
        render: &RenderHandle,
        executor_id: ParticipantId,
        cancelled: &CancelObserver,
    ) -> Self {
        // 执行者的策略：派发者的**立场**，加上派发者标为可传播的每一条规则，别的什么都没有。
        // 它的权限是派发者权限的子集 —— `auto` 会话的执行者可以写，`readonly` 会话的不可以，
        // `ask` 会话的执行者经同一个端口发问 —— 而*放行*从不随之下行，因为 `Allow` 不传播
        // （spec §12、§16）。
        let parent_policy = session.policy();
        // `outside_read` 是**策略级**的那一条旋钮，与档位正交（`.scratch/workspace-mode` 的
        // spec §2）：它是配置写下来的立场，不是某个 agent 挣到的许可，所以执行者照旧沿用 ——
        // 否则一个 `outside_read = "ask"` 的会话会派出一组连问都不问、直接拒的执行者，而
        // 讨论者（走 `fork`、共享同一份 `Policy`）却不这样。
        let mut policy =
            Policy::for_mode(parent_policy.mode()).with_outside_read(parent_policy.outside_read());
        for rule in parent_policy.inherited_rules() {
            policy.push(rule);
        }

        // 模型是继承来的，除非某个 profile 把执行者路由到别处（spec §16、§17）：执行者在
        // 派发者的 client 上作答，所以覆盖只能点名那个 client 能服务的模型，而真正属于它自己
        // 的只有回合上限和模型。路由规则本身住在 `SessionConfig::model_for`，合成器用的也是
        // 同一个 —— 那两处是更便宜的模型唯一可被路由到的落点。
        let mut config = session.config().clone();
        config.max_iterations = config.executor_max_iterations;
        config.model = config.model_for(LandingPoint::Executor).to_owned();

        Self {
            parent: parent.clone(),
            executor_id,
            cwd: session.cwd().to_path_buf(),
            log: session.log().clone(),
            session_id: session.id().clone(),
            outputs_dir: session.outputs_dir().to_path_buf(),
            locks: session.path_locks().clone(),
            policy,
            asker: session.asker().cloned(),
            hook: session.hook().cloned(),
            home: session.home().map(Path::to_path_buf),
            skills: session.skills().clone(),
            tools: Arc::new(session.tools().for_executor()),
            config,
            provider: Arc::clone(provider),
            render: render.clone(),
            cancelled: cancelled.clone(),
        }
    }

    /// 把执行者跑完，并从中塑出那一个工具结果。
    async fn run(&self, brief: &str) -> Result<ToolOutput, ToolError> {
        let executor = SpeakerId::Executor(self.executor_id.clone());
        // 派发记录在执行者做任何事之前写下，所以哪怕整件事就是一次阻塞的工具调用，流读起来
        // 也是因果的。它归属给执行者 —— 这是执行者的生命周期事件，`parent` 才是点名派发者的
        // 地方 —— 而这也正是简报能进入执行者自己那次投影的原因（spec §5、§16）。
        append_event(
            &self.log,
            &self.config.redactor,
            &self.render,
            executor.clone(),
            EventPayload::ExecutorSpawned {
                executor_id: self.executor_id.clone(),
                parent: participant_of(&self.parent),
                brief: brief.to_owned(),
            },
        )
        .map_err(spawn_failed)?;

        let mut session = Session::new(SessionParts {
            id: self.session_id.clone(),
            cwd: self.cwd.clone(),
            log: self.log.clone(),
            config: self.config.clone(),
            tools: Arc::clone(&self.tools),
            locks: self.locks.clone(),
            outputs_dir: self.outputs_dir.clone(),
            // 一个新的策略值，绝不是父会话那个句柄：父会话挣来的放行绝不能
            // 传到子会话（spec §12）。
            policy: Arc::new(Mutex::new(self.policy.clone())),
            asker: self.asker.clone(),
            // 刻意不携带：执行者的表里根本没有 `ask_user_question`
            // （spec §7、§16），所以这里没有任何东西需要一个端口来回答。
            questions: None,
            hook: self.hook.clone(),
            home: self.home.clone(),
            skills: Arc::clone(&self.skills),
            identity: Some(executor_identity()),
        });

        // 读集合从空开始，而且两个方向都不流（spec §16）：这条护栏讲的是一个 agent 对工作区
        // 的认知，而一个还没看过某个文件的子会话对它就还没有认知。
        let outcome = run_turn(
            &mut session,
            &executor,
            &self.provider,
            &self.render,
            TurnScope::Executor,
            &self.cancelled,
        )
        .await;

        let (reason, summary) = match outcome {
            Ok(outcome) => (outcome.reason, outcome.text),
            // 日志写入失败是唯一致命的结局：流已经不能用了，所以也没有什么东西
            // 还能经它上报。
            Err(error) => {
                self.render
                    .diagnostic(&format!("执行者 {}：{error}", self.executor_id));
                (StopReason::Error, String::new())
            }
        };
        append_event(
            &self.log,
            &self.config.redactor,
            &self.render,
            executor.clone(),
            EventPayload::ExecutorFinished {
                executor_id: self.executor_id.clone(),
                reason,
                summary: summary.clone(),
            },
        )
        .map_err(spawn_failed)?;

        // 元数据从流上派生（spec §16）：执行者自己花的钱、以及它改过的文件，不需要第二本
        // 账、也不需要新字段。
        let events = self.log.events();
        let usage = usage_of(&events, &executor);
        let changed = changed_files(&events, &self.executor_id);
        let report = executor_report(&self.executor_id, reason, &summary, usage, &changed);

        // 那四个失败值是一条「错误内容」的工具结果；讨论不会因为它们被
        // 打断（spec §16）。
        if reason == StopReason::Completed {
            Ok(ToolOutput::new(report))
        } else {
            Err(ToolError::message(report))
        }
    }
}

#[async_trait::async_trait]
impl ExecutorSpawner for ExecutorPort {
    async fn spawn(&self, brief: &str) -> Result<ToolOutput, ToolError> {
        self.run(brief).await
    }
}

/// 把一次写入失败变成 `task` 调用那条错误结果。
fn spawn_failed(error: Error) -> ToolError {
    ToolError::message(format!("执行者：{error}"))
}

/// 一个发言者以什么身份行动。`parent` 是参与者 id，所以一个不是参与者的发言者（它本来也
/// 派发不了）折成自己的拼写，而不是另造一个身份。
fn participant_of(speaker: &SpeakerId) -> ParticipantId {
    match speaker {
        SpeakerId::Debater(id) | SpeakerId::Executor(id) => id.clone(),
        other => ParticipantId::new(other.to_string()),
    }
}

/// 这个参与者已经派发过多少个执行者，从流上数。id 就是由这个计数组成的 `<parent>-<n>`，
/// 所以一个被续上的会话不可能发出一个它已经用过的 id。
pub(super) fn spawned_executors(events: &[Event], parent: &SpeakerId) -> u32 {
    let parent = participant_of(parent);
    events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventPayload::ExecutorSpawned { parent: recorded, .. } if recorded == &parent
            )
        })
        .count() as u32
}

/// 一个执行者改过的文件，从流上派生（spec §16）。
///
/// 一次改动就是一次成功的调用、且它的结果点名了写向哪个文件（[`WROTE_PATH_PREFIX`]）。
/// 读**结果**而不是调用的参数，正是让它在 `hook.pre` 于 `ToolCallStarted` 记下模型所求
/// 之后改写这次调用时依然正确的原因。
///
/// 它住在这里而不是 `events` 里别的流查询旁边，因为它解析的是一条工具约定，而 `events`
/// 不依赖任何东西（spec §1）—— 在那里点名 `write_file` 会把依赖箭头指反。
fn changed_files(events: &[Event], executor: &ParticipantId) -> Vec<String> {
    let speaker = SpeakerId::Executor(executor.clone());
    let mut changed: BTreeSet<String> = BTreeSet::new();
    for event in events {
        if event.speaker_id != speaker {
            continue;
        }
        if let EventPayload::ToolCallCompleted {
            ok: true,
            output: Some(output),
            ..
        } = &event.payload
        {
            if let Some(path) = output.strip_prefix(WROTE_PATH_PREFIX) {
                if let Some(path) = path.lines().next().filter(|line| !line.is_empty()) {
                    changed.insert(path.to_owned());
                }
            }
        }
    }
    changed.into_iter().collect()
}

/// 执行者那份回复的唯一形状：摘要加元数据（spec §16）。
///
/// 没有人解析它 —— 它是派发者的阅读材料，作为 `task` 调用的结果送达 —— 所以改动过的文件
/// 清单和 token 计数放在摘要上方，读者能照着它们行动。
fn executor_report(
    executor: &ParticipantId,
    reason: StopReason,
    summary: &str,
    usage: Usage,
    changed: &[String],
) -> String {
    let files = if changed.is_empty() {
        "无".to_owned()
    } else {
        changed.join(", ")
    };
    format!(
        "执行者 {executor} 结束：{reason}\n\
         改动文件：{files}\n\
         token：input {}, output {}, cached {}, miss {}\n\
         报告：\n{summary}",
        usage.input_tokens, usage.output_tokens, usage.cached_tokens, usage.miss_tokens,
    )
}
