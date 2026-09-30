//! 回合循环。
//!
//! `agent` 层是事件流的唯一写者，而循环是唯一调用 provider 的地方。钩子与权限门是它按固定
//! 顺序施加的纯值变换（spec §3）：
//! `hook.pre -> gate -> [ask] -> dispatch -> hook.post -> append`。前置钩子在权限门之前
//! 跑，所以它能拦下一次询问，却永远绕不过一次；它的输出是一条约束，而最终裁决是那条约束与
//! 权限门裁决的上确界。
//!
//! 从第一张票起就有三条不变量成立：
//!
//! 1. 每一个 `tool_call` 恰好拿到一个结果；
//! 2. 只要还有 `tool_call` 没有结果，就绝不调用 provider；
//! 3. 每一条事件都走 [`append_event`] —— 循环自己的路径与它驱动的执行者端口是同一个唯一
//!    写者。
//!
//! [`executor`] 是本层的子模块，而不是自己的一道边界：跑一个执行者意味着驱动一个回合，所以
//! 它是控制流（spec §1、§16）。
//!
//! [`cancel`] 是那个能提前停下回合的手势的管路（spec §6）：provider 流进行中时、工具在跑时，
//! 回合 select 它；执行者持有同一套管路，于是一次手势能到达它下面整条链。

mod cancel;
mod executor;
mod history;
pub mod replay;

pub use cancel::{CancelObserver, CancelSignal};
pub use history::{recover_pending_calls, undo_last_edit, UndoOutcome};
pub use replay::{replay, ReplayError};

use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;

use crate::context;
use crate::events::{
    hook_format, last_assistant_has_tool_calls, pending_tool_calls_of, total_usage, ContextSource,
    Decision, DecisionSource, Event, EventLog, EventPayload, ParticipantId, Redactor, Role,
    RoundMode, SpeakerId, StopReason, ToolCallId, SCHEMA_VERSION,
};
use crate::hooks::{self, Constraint, HookPoint};
use crate::permissions::{self, Answer, PermissionRequest};
use crate::provider::capability::ModelCaps;
use crate::provider::projection::project;
use crate::provider::{ChatRequest, Message, Provider, StreamEvent, ToolCall, ToolChoice};
use crate::render::RenderHandle;
use crate::session::Session;
use crate::tools::{
    AllowedCall, BashLimits, CallFacts, DispatchOutcome, GuardedCall, PendingCall, ToolError,
    ToolOutput, TASK_TOOL,
};
use crate::Error;

use executor::{spawned_executors, ExecutorPort};

/// 一个回合被允许看到多少流。
///
/// 单 agent 回合看到全部。讨论轮次里的一个回合看到截止到该轮 `RoundStarted`（含）的一切，
/// 加上它自己更晚的事件 —— 永远看不到同一轮里另一个讨论者的事件（spec §15）。
///
/// 这是**结构性**的切分，不是对时序的指望。两个讨论者同时进行中，所以「两边都还没作答」是一场
/// 快的假 provider 立刻就会输掉的竞态；把流切在一个 `seq` 上，是唯一一种无论两个回合如何
/// 交错都成立的独立性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnScope {
    /// 日志里的一切：一次普通的单 agent 回合。
    Whole,
    /// 截止到 `seq == before_seq`（该轮的 `RoundStarted`，含）的一切，
    /// 加上正在行动的发言者自己更晚的事件。
    Round { before_seq: u64 },
    /// 执行者自己的窗口：钉住的注入加上它自己的事件，别的都没有。
    ///
    /// 执行者不是派发它那场对话的参与者。它靠自己的简报干活，所以派发它的那个会话说了什么、
    /// 另一个发言者答了什么，都不在它的窗口里 —— 这也是让一场长讨论不至于被回放进每个执行者
    /// 上下文的原因。简报本身是经流送达的，作为 `ExecutorSpawned`（spec §5、§16）。
    Executor,
}

/// 这个回合在 `scope` 之下对流的视图。
fn scoped_events(session: &Session, speaker: &SpeakerId, scope: TurnScope) -> Vec<Event> {
    scoped_events_slice(&session.events(), speaker, scope)
}

/// 在一段显式切片上做 [`scoped_events`]。
///
/// 切片形式是 `replay` 需要的：它手里那张快照已经在被复现的那次调用处切好了，所以它走不了
/// 活着的日志。两者共用这一条规则，这正是重算出的窗口与活着的窗口一模一样的原因
/// （spec §15、§18）。
pub(crate) fn scoped_events_slice(
    events: &[Event],
    speaker: &SpeakerId,
    scope: TurnScope,
) -> Vec<Event> {
    let mut events = events.to_vec();
    match scope {
        TurnScope::Whole => {}
        TurnScope::Round { before_seq } => {
            events.retain(|event| event.seq <= before_seq || &event.speaker_id == speaker);
        }
        // 钉住的注入是每个 agent 都会重放的会话头部，所以无论执行者有没有可能当场看到它，
        // 它都活过这次切分（执行者是在注入被记录之后才派发出来的）。
        TurnScope::Executor => events.retain(|event| {
            &event.speaker_id == speaker
                || matches!(
                    event.payload,
                    EventPayload::ContextInjected { .. } | EventPayload::SessionStarted { .. }
                )
        }),
    }
    events
}

/// 一段对**每一个**身份都成立的通用条款：思考用什么语言写。
///
/// 四个身份共用它（本程序、讨论者、合成器、执行者），所以它只写在这里一处 —— 在四段身份里各写
/// 一遍，迟早会漂开一份。它拼进 `system` 提示本身，而不是另发一条消息：`messages` 的第一条就是
/// 身份（[`build_messages`]），多一条消息就多一处要与 provider 和 `replay` 对齐的格式。
///
/// 为什么值得占掉前缀里的一行：思考文本和回答一样进流（`MessageCompleted.reasoning`），也一样
/// 进转录的详情弹窗（`DetailKind::Thinking`），所以 ADR 0005 那条「英文只留给不是散文的东西」
/// 管着它。它也比回答更容易漂回英文 —— 推理里夹着大量标识符、路径与代码。
///
/// 它是 `pub` 的，好让测试直接断言四个身份都拼上了它（`tests/thinking_language.rs`），而不是
/// 在断言里再抄一遍这句话。
pub const THINKING_IN_CHINESE: &str =
    "思考也用中文写：它和你的回答一样是给人读的散文。只有标识符、路径、命令原文与 schema 值留英文。";

/// 单 agent 的 `system` 提示词：本程序是什么。
///
/// 讨论者与执行者各自以自己的身份打头；而普通会话本来什么都不带，于是唯一描述这个程序的东西
/// 就是钉住的上下文 —— 模型读着一段技能正文，自我介绍成了「Claude Code」。这份身份和这里的
/// 其它身份一样从不进日志，所以 `replay` 从流的形状上把它推出来。
///
/// 中间那段是 `.scratch/todo-and-modes/spec.md` §3 的**规则段**：引导模型维护一份 `todo` 列表，
/// 不是强制。它搭在这里，是因为这是**每一次**请求里模型可见的前缀，也就是 ADR 0001 那条规矩
/// 最严苛的版本 —— 加一行是允许的，改一行或删一行会让每一个会话的缓存前缀作废。措辞是中文
/// （ADR 0005：这一侧的散文按「是不是标识符」分，不按「谁读它」分），而三个状态词仍是它点名的
/// 标识符；它还是 `const` 风格的字面量，另有一个理由：有测试钉住那三个词，所以工具的用词和这
/// 条指令没法漂开。
///
/// 末尾拼的是 [`THINKING_IN_CHINESE`]。于是它返回 `String` 而不是 `&'static str` —— 与另外两个
/// 身份函数（讨论者、合成器）一致，那两句各自拼同一段条款。
pub fn agent_identity() -> String {
    [
        "你是 fs-agent，一个自用的 coding agent CLI（Rust 实现），运行在用户自己的机器与工作区里。",
        "你直接读写文件、运行命令、搜索代码，并按这个仓库自己的约定干活（AGENTS.md、CONTEXT.md、",
        "docs/adr、.scratch 里的 spec 与 ticket）。你不是 Claude Code，也不是 Anthropic 的产品，",
        "不要自称是；被问到你是谁时，说你是 fs-agent。你没有跨会话记忆：需要上下文就读文件或问用户。",
        "\n\n",
        "开工前先把计划写下来，用 `todo` 工具：每一步都是一项，`status` 写 `pending`。正在做的那一项标成 \
         `in_progress`，每完成一项就更新这份列表（`completed`），最后一次调用把每一项都写成 `completed`，收尾。\
         列表是用户看你正在做什么、做到哪一步的地方，所以要一直更新，而不是只写一次。",
        "\n\n",
        THINKING_IN_CHINESE,
    ]
    .concat()
}

/// 投影 → 前置私有身份 → 裁剪。
///
/// 一个回合的 provider `messages` 在这里被构造（spec §5、§10、§15），这是唯一的一处，所以
/// `replay` 是复现循环而不是近似它：同一个投影、同一份打头的 `system` 身份、同一条裁剪策略，
/// 同一个顺序。身份从不进日志，这正是它必须是一个共享函数、而不是两处「今天恰好一致」的调用
/// 点的原因。
pub(crate) fn build_messages(
    events: &[Event],
    speaker: &SpeakerId,
    caps: &ModelCaps,
    identity: Option<&str>,
    trim_policy: &context::TrimPolicy,
) -> Result<Vec<Message>, context::TrimError> {
    let projected = project(events, speaker, caps);
    // agent 的私有身份给请求打头、从不进日志（spec §15）。它计进预算但被钉住：`trim` 把
    // 打头的 `system` 消息当作永不丢弃的头部的一部分，所以协议指令不会被裁掉，而需要它们的
    // 那个问题还留着。
    let projected = match identity {
        Some(identity) => {
            let mut messages = Vec::with_capacity(projected.len() + 1);
            messages.push(Message::System {
                content: identity.to_owned(),
                name: None,
            });
            messages.extend(projected);
            messages
        }
        None => projected,
    };
    context::trim(projected, context::usable_input(caps), trim_policy)
}

/// 一个回合是怎么结束的，以及它最后一条消息里的助手文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOutcome {
    pub reason: StopReason,
    pub text: String,
}

/// 记下会话骨架。`agent` 模块拥有对日志的每一次写入，
/// 所以连一次性的 `SessionStarted` 事件也在这里记录。
pub fn record_session_started(session: &mut Session, render: &RenderHandle) -> Result<(), Error> {
    emit(
        session,
        render,
        &SpeakerId::System,
        EventPayload::SessionStarted {
            session_id: session.id().clone(),
            cwd: session.cwd().to_string_lossy().into_owned(),
            schema_version: SCHEMA_VERSION,
        },
    )
}

/// 在一个回合开始之前记下用户自己的消息。
pub fn record_user_message(
    session: &mut Session,
    render: &RenderHandle,
    text: &str,
) -> Result<(), Error> {
    emit(
        session,
        render,
        &SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: text.to_owned(),
            reasoning: None,
        },
    )
}

/// 记下一次钉住的上下文注入。
///
/// 注入是一等事件，这样 `project` 才能保持是流加规则的函数（spec §10）：模型重放的内容就是
/// 当时记录下来的内容，而不是那个文件今天写着什么。`ContextInjected` 归属给 `User`
/// （spec §5），投影把它变成第一条 `user` 消息 —— 从不合并，也从不被 [`context::trim`]
/// 丢掉。
pub fn record_context_injection(
    session: &mut Session,
    render: &RenderHandle,
    source: ContextSource,
    content: &str,
) -> Result<(), Error> {
    record_context_injection_from(session, render, &SpeakerId::User, source, content)
}

/// 同上，但归属给一个**参与者**而不是用户。
///
/// 只有一次注入走这条路：讨论者的人物（spec §15）。它的归属正是投影能把它只给那个讨论者、
/// 不给别人的原因 —— 论点的对侧没道理读到它 —— 所以这是唯一一处注入以用户之外的身份说话。
pub fn record_context_injection_from(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    source: ContextSource,
    content: &str,
) -> Result<(), Error> {
    emit(
        session,
        render,
        speaker,
        EventPayload::ContextInjected {
            source,
            content: content.to_owned(),
        },
    )
}

/// 为 `speaker` 跑完整整一个回合，并返回它为什么停下。
///
/// `scope` 是这个回合能看到多少流。它只会*去掉*另一个讨论者同轮的事件，所以单 agent 回合传
/// [`TurnScope::Whole`]，看到的正是它一向看到的流。
///
/// `cancelled` 是这个回合对会话取消手势的视图（spec §6）。provider 流进行中时、工具在跑时都会
/// select 它，它也会被克隆进这个回合派发的每一个执行者，于是一次手势也能停下它下面的链。
pub async fn run_turn(
    session: &mut Session,
    speaker: &SpeakerId,
    provider: &Arc<dyn Provider>,
    render: &RenderHandle,
    scope: TurnScope,
    cancelled: &CancelObserver,
) -> Result<TurnOutcome, Error> {
    let max_iterations = session.config().max_iterations;
    // 投影会按模型字段级的那些事实分叉，所以它们从 provider 读一次，而不是每一轮迭代重新推
    // 一遍。丢弃策略也是一个值，所以它在循环外构造一次。
    let caps = provider.caps();
    let trim_policy = context::TrimPolicy::default();
    let mut iteration: u32 = 0;
    let mut last_text = String::new();
    // 执行者 id 是 `<parent>-<n>`，从流上数出来而不是分配出来的，所以一个被续上的会话发不出
    // 它已经用过的 id。计数取一次，然后带着走完这个回合的各批。
    let mut executors_spawned = spawned_executors(&scoped_events(session, speaker, scope), speaker);
    // 这个回合的每一次调用都用这些值处理。它们在迭代之间不变，所以在循环外打成一个包。
    let context = TurnContext {
        render,
        speaker,
        scope,
        provider,
        cancelled,
    };

    loop {
        // 每次迭代一张快照：日志与另一个进行中的讨论者共享，所以对它的每一次读都是快照而不是
        // 借用。
        let events = scoped_events(session, speaker, scope);

        // 不变量 2：有挂着的 tool_call 就说明日志还欠一个结果，所以绝不能调用 provider。
        // 作用域限于这个发言者：两个讨论者同时进行中时，另一个没做完的调用不是这个回合的事 ——
        // 把它当成自己的会让这个回合以一个跟它毫不相干的错误收场。
        if !pending_tool_calls_of(&events, speaker).is_empty() {
            return end_turn(session, render, speaker, StopReason::Error, last_text);
        }

        // 落在迭代之间的手势会在下一个 provider 调用打开之前停下这个回合（spec §6）。在这里
        // 检查，而不是只在流内部检查，能让一个被取消的回合不去发一个它马上就会弃掉的请求。
        if cancelled.is_cancelled() {
            render.diagnostic("下一次模型调用之前，这个回合被取消了");
            return end_turn(session, render, speaker, StopReason::Aborted, last_text);
        }

        // 会话的硬停（spec §17）：累计花费一旦碰到额度，这个回合就不再打开新的调用，并以
        // `BudgetExhausted` 收场 —— 降级收尾，绝不留下半个单位。求和取自**整条**日志，而不是
        // 这个回合那点有作用域的视图：额度是会话的，而执行者的窗口刻意不包含与它分享额度的
        // 那些讨论者。
        //
        // 执行者自己的回合是**豁免**的：硬停拒绝派发新执行者，让已经在跑的那些跑完
        // （spec §17），所以这里没有任何东西能中途停下一个。它花掉的钱照样落在流上。
        let budget = session.config().budget.clone();
        let spent = total_usage(&session.events()).total_tokens();
        let gated = scope != TurnScope::Executor;
        if gated {
            if let Some(note) = budget.exhausted_note(spent) {
                render.diagnostic(&note);
                return end_turn(
                    session,
                    render,
                    speaker,
                    StopReason::BudgetExhausted,
                    last_text,
                );
            }
        }

        if iteration >= max_iterations {
            return end_turn(
                session,
                render,
                speaker,
                StopReason::MaxIterations,
                last_text,
            );
        }
        iteration += 1;

        emit(
            session,
            render,
            speaker,
            EventPayload::TurnStarted {
                agent: speaker.clone(),
                iteration,
            },
        )?;

        // 投影只做归属；裁剪是下一个纯步骤，也是唯一丢弃东西的地方。一次装不进预算的裁剪说明
        // 每一类可丢的材料都用尽了，那是这个回合的硬失败（spec §10）。
        let messages =
            match build_messages(&events, speaker, &caps, session.identity(), &trim_policy) {
                Ok(messages) => messages,
                Err(error) => {
                    render.diagnostic(&format!("上下文预算：{error}"));
                    return end_turn(session, render, speaker, StopReason::Error, last_text);
                }
            };

        // 会话闸门的预检那一半（spec §17）。估计很粗 —— 字符数 / 4 —— 所以阈值取剩余量的倍数
        // 而不是精确比较，而一次显然装不下的调用绝不会被发出去。输出 token 不做估计；额度
        // 自己的累计检查会把这次调用真正花的钱收上来。执行者的回合在这里同样豁免：钱花光的
        // 时候它已经在跑了。
        if gated {
            let estimated = context::estimate_messages_tokens(&messages);
            if !budget.admits_estimate(spent, estimated) {
                render.diagnostic(&budget.estimate_refusal_note(estimated));
                return end_turn(
                    session,
                    render,
                    speaker,
                    StopReason::BudgetExhausted,
                    last_text,
                );
            }
        }

        let request = ChatRequest {
            model: session.config().model.clone(),
            messages,
            tools: session.tools().specs(),
            tool_choice: ToolChoice::Auto,
            params: session.config().params.clone(),
            cache_key: Some(session.id().as_str().to_owned()),
        };

        // 请求自己有可能仍在进行中 —— 适配器只有在传输层作答之后才交回流 —— 所以这次发送也可以
        // select：一次手势不该被迫等一条卡住的连接。
        let mut cancel = cancelled.clone();
        let sent = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                render.diagnostic("模型调用进行中时，这个回合被取消了");
                return end_turn(session, render, speaker, StopReason::Aborted, last_text);
            }
            sent = provider.send(request) => sent,
        };
        let mut stream = match sent {
            Ok(stream) => stream,
            Err(error) => {
                render.diagnostic(&format!("provider 错误：{error}"));
                return end_turn(session, render, speaker, StopReason::Error, last_text);
            }
        };

        let mut text = String::new();
        let mut reasoning = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut failed = false;
        let mut saw_done = false;
        let mut aborted = false;

        loop {
            tokio::select! {
                // 手势在与一个已经就绪的条目打平时胜出：现在停下正是
                // 按下它的全部意义。
                biased;
                _ = cancel.cancelled() => {
                    aborted = true;
                    break;
                }
                item = stream.next() => match item {
                    Some(Ok(StreamEvent::TextDelta(delta))) => {
                        render.text_delta(speaker, &delta);
                        text.push_str(&delta);
                    }
                    Some(Ok(StreamEvent::ReasoningDelta(delta))) => {
                        render.reasoning_delta(speaker, &delta);
                        reasoning.push_str(&delta);
                    }
                    Some(Ok(StreamEvent::ToolCallStarted { .. })) => {
                        // 碎片由适配器拼装；循环只看到完成的
                        // 那次调用。
                    }
                    Some(Ok(StreamEvent::ToolCallCompleted {
                        id,
                        name,
                        arguments,
                        ..
                    })) => {
                        tool_calls.push(ToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                    Some(Ok(StreamEvent::Usage(usage))) => {
                        emit(
                            session,
                            render,
                            speaker,
                            EventPayload::UsageRecorded { usage },
                        )?;
                    }
                    Some(Ok(StreamEvent::Finished { finish_reason })) => {
                        // 流在 `[DONE]` 上结束。`finish_reason` 只作诊断；
                        // 这个回合的停止原因来自循环自己的续跑查询，
                        // 从不来自 provider。
                        render.diagnostic(&format!(
                            "provider 流结束：{}",
                            crate::render::wording::finish_reason(&finish_reason)
                        ));
                        saw_done = true;
                        break;
                    }
                    Some(Err(error)) => {
                        render.diagnostic(&format!("provider 流错误：{error}"));
                        failed = true;
                        break;
                    }
                    // 流就这么停了。没有任何东西完成，所以什么都不落进日志；
                    // 下面的 `[DONE]` 检查把它变成错误。
                    None => break,
                },
            }
        }

        // 一条被中断的流在这里被丢掉，和这个回合一起：对真实适配器来说，「停下进行中的
        // provider 流」就是这个意思，也正是收到一半的文本留在日志之外的原因 —— 一个从没到
        // `[DONE]` 的回合没有产出任何完成的单位（spec §6）。
        if aborted {
            render.diagnostic("模型流进行中时，这个回合被取消了");
            return end_turn(session, render, speaker, StopReason::Aborted, text);
        }

        // 只有 `[DONE]` 才结束一条消息：失败了、或者没有它就停下的流没有产出任何完成的单位，
        // 所以什么都不落进日志。
        if failed || !saw_done {
            if !failed {
                render.diagnostic("provider 流没有等到 [DONE] 就结束了");
            }
            return end_turn(session, render, speaker, StopReason::Error, text);
        }

        if !text.is_empty() || !reasoning.is_empty() || !tool_calls.is_empty() {
            emit(
                session,
                render,
                speaker,
                EventPayload::MessageCompleted {
                    role: Role::Assistant,
                    text: text.clone(),
                    reasoning: (!reasoning.is_empty()).then(|| reasoning.clone()),
                },
            )?;
        }

        // 每一个 `tool_call` 恰好拿到一个结果，在这里产出、别处不产出。前置钩子先跑，然后是
        // 权限门；一次拒绝（钩子的收紧 / 跳过 / 失败、策略拒绝、用户拒绝、或者 headless 下的
        // `Ask` 降级）合成它那一个错误结果，所以工具根本不会被触到。接着派发者握住那些共享
        // 护栏（改前先读、逐路径的写锁、读集合失效），这样没有哪个工具能选择退出；结果落进
        // 日志之后，后置钩子跑一次。
        //
        // 一次 `task` 调用被判定得和其它调用一模一样，然后被推迟：这一批里被推迟的调用一起
        // 跑，因为派发不碰工作区的任何路径，而几个执行者同时干活正是重点（spec §16）。别的
        // 一切照旧在原地跑，就像它一向那样。
        let mut deferred: Vec<DeferredCall> = Vec::new();
        for call in &tool_calls {
            // 手势在这次调用开始之前就落下的，不欠它任何东西：它还没有一丝一毫落在流上。
            // 被推迟的那些调用已经开始了，所以每一个仍然拿到它被欠的那一个结果（spec §6）。
            if cancelled.is_cancelled() {
                close_deferred_calls(session, render, speaker, deferred, CANCELLED_BEFORE_RUN)?;
                render.diagnostic("工具调用开跑之前，这个回合被取消了");
                return end_turn(session, render, speaker, StopReason::Aborted, last_text);
            }

            match process_call(session, &context, &mut executors_spawned, call).await? {
                Disposition::Finished => {}
                Disposition::Deferred(call) => deferred.push(*call),
                // 一次手势停下了这个回合（前置钩子的 `Stop`，或者一次抓住调用进行中时的取消）。
                // 那些已经启动并推迟的调用仍然各欠恰好一个结果；它们从没跑过，所以说出来。
                Disposition::Stopped(why) => {
                    close_deferred_calls(session, render, speaker, deferred, why)?;
                    return end_turn(session, render, speaker, StopReason::Aborted, last_text);
                }
            }
        }
        run_deferred(session, render, speaker, deferred).await?;

        last_text = text;

        if last_assistant_has_tool_calls(&session.events(), speaker) {
            continue;
        }

        return end_turn(session, render, speaker, StopReason::Completed, last_text);
    }
}

/// 每个让回合停下的手势给 `tool_call` 的那一个结果。
///
/// 只在这里命名一次，因为两者都经 [`close_deferred_calls`] 流过、而且模型会读到它们：工具
/// 有没有跑，决定了工作区有没有可能被改过。
const HOOK_STOPPED_TURN: &str = "钩子停掉了这个回合：工具没有跑";
const CANCELLED_BEFORE_RUN: &str = "这个回合被取消了：工具没有跑";
/// 会话 token 额度用尽时一次 `task` 调用拿到的那一个结果（spec §17）：执行者从没被派发
/// 出去，所以工作区没被它碰过。
const BUDGET_NO_NEW_EXECUTOR: &str =
    "会话 token 额度已用尽：不再派发新的执行者。已经在跑的活让它跑完。";
/// 一次抓住调用进行中时的取消：工具那个 future 被 drop 了，所以它有没有生效是未知的 —— 与
/// 崩溃恢复那条结果携带的是同一种诚实。
const CANCELLED_IN_FLIGHT: &str =
    "这个调用进行中时回合被取消了，所以它有没有生效是未知的。它没有被重跑；\
     在依赖任何一种结果之前先检查工作区。";

/// 钩子与权限门都说过话之后，循环对一次调用必须做什么。
enum Disposition {
    /// 这次调用完事了：它那一个结果已经在日志里。
    Finished,
    /// 这次调用可以跑，和这一批里别的被推迟的调用一起跑。
    Deferred(Box<DeferredCall>),
    /// 这个回合到此为止。调用方用 `why` 把这一批里已启动但未派发的调用收尾，
    /// 然后记录这次中止。
    Stopped(&'static str),
}

/// 一次被授权、与同批兄弟一起跑的调用。
struct DeferredCall {
    pending: PendingCall,
    allowed: AllowedCall,
    started: Instant,
}

/// 在回合结束、[`run_deferred`] 还没来得及走到时，给一批里每一个已启动但未派发的调用
/// 它被欠的那一个结果。
///
/// 一次 `task` 调用在被推迟之前就记为已启动，所以哪怕执行者从没跑过，它也欠着一个结果。
/// `why` 是停下这个回合的那个手势；形状只在一处，所以任何一条收尾路径都不可能忘掉一次调用。
fn close_deferred_calls(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    deferred: Vec<DeferredCall>,
    why: &str,
) -> Result<(), Error> {
    for call in deferred {
        emit_completed(
            session,
            render,
            speaker,
            ToolCallId::new(call.pending.tool_call_id.clone()),
            Err(ToolError::message(why)),
            call.started,
        )?;
    }
    Ok(())
}

/// 一次已决策的调用产出了什么，等着被记录。
struct CallCompletion<'a> {
    pending: &'a PendingCall,
    /// 这次调用可以碰的路径，当它过了权限门那个「是」的时候。
    allowed: Option<&'a AllowedCall>,
    started: Instant,
    /// 工具是不是真的跑过：只有跑过，后置钩子才挂上去。
    dispatched: bool,
    outcome: DispatchOutcome,
}

/// 这个回合自己的那些值，借给它处理的每一次调用。
///
/// 谁在行动、它能看多少流、它用什么作答、以及它可以被怎么停下，对一批里的每一次调用都一样，
/// 所以它们作为一个值交出去一次，而不是五个。
struct TurnContext<'a> {
    render: &'a RenderHandle,
    speaker: &'a SpeakerId,
    scope: TurnScope,
    provider: &'a Arc<dyn Provider>,
    cancelled: &'a CancelObserver,
}

/// 把一次工具调用从 `ToolCallStarted` 送到它那一个结果：解析它、跑前置钩子、问权限门，
/// 然后 —— 除非这次调用是一次被推迟的 `task` —— 跑那个工具。
///
/// 做成一个函数而不是内联代码，这样被推迟的路径与原地路径没法漂开：两者都在
/// [`finish_call`] 收尾。
async fn process_call(
    session: &mut Session,
    context: &TurnContext<'_>,
    executors_spawned: &mut u32,
    call: &ToolCall,
) -> Result<Disposition, Error> {
    let TurnContext {
        render,
        speaker,
        scope,
        provider,
        cancelled,
    } = *context;
    let tool_call_id = ToolCallId::new(call.id.clone());
    emit(
        session,
        render,
        speaker,
        EventPayload::ToolCallStarted {
            tool_call_id: tool_call_id.clone(),
            tool_name: call.name.clone(),
            args: parse_tool_args(&call.arguments),
        },
    )?;

    // 在读集合被借走之前，一切都先从会话上读出来。
    let paths = session.paths().clone();
    let locks = session.path_locks().clone();
    let skills = session.skills().clone();
    // `repo_map` 按这个会话正在干什么来排序，所以它的输入从流上重算 ——
    // 但只为真会用到它的那次调用算。
    let repo_map = if call.name == context::repo_map::REPO_MAP_TOOL {
        context::repo_map::RepoMapInput {
            context: context::repo_map::RankContext::from_session(
                &scoped_events(session, speaker, scope),
                session.cwd(),
            ),
            tokens: session.config().repo_map_tokens,
        }
    } else {
        context::repo_map::RepoMapInput::default()
    };
    let mut pending = PendingCall {
        tool_call_id: tool_call_id.as_str().to_owned(),
        tool_name: call.name.clone(),
        args: parse_tool_args(&call.arguments),
        outputs_dir: session.outputs_dir().to_path_buf(),
        paths,
        locks,
        skills,
        repo_map,
        // `bash` 工具的那两条限制随调用一起走，就像仓库地图的预算：配置是通过交到手里的
        // 上下文到达工具的，从不靠伸手进会话里去取。
        bash: BashLimits {
            default_timeout_ms: session.config().bash_timeout_ms,
            max_timeout_ms: session.config().max_bash_timeout_ms,
        },
        executor: None,
        // 问题端口是会话的，不是循环的：带在这里，是为了让 `ask_user_question` 工具经它被
        // 派发时的上下文收到它，而执行者端口是逐次授权调用填进去的（spec §7）。
        questions: session.questions().cloned(),
    };

    let started = Instant::now();
    // 调用只解析一次：权限门、钩子与护栏读的是同一批事实，而这是唯一一个为了解析路径去碰
    // 文件系统的步骤。
    let mut facts = match session
        .tools()
        .facts(&pending.tool_name, &pending.args, &pending.paths)
    {
        Ok(facts) => facts,
        Err(error) => {
            // 没有工具可判、也没有东西可跑：这次调用的那一个结果
            // 就是这次失败。
            emit_completed(session, render, speaker, tool_call_id, Err(error), started)?;
            return Ok(Disposition::Finished);
        }
    };

    // ① hook.pre。它在权限门之前跑，所以能拦下一次询问；它的约束在下面与权限门的裁决合并。
    //    一次 `Rewrite` 会改变权限门与工具看到的东西，所以它必须发生在这里，而不是权限门
    //    之后。
    //
    //    每次调用恰好记录一条 `HookExecuted`，无论结果如何，这样流就是挂载点的一份完整
    //    记录，票 19 能按钩子结果分组。
    let mut hook_verdict: Option<Decision> = None;
    if let Some(hook) = session.hook().cloned() {
        let constraint = {
            let history = hooks::public_history(&session.events());
            let pre_call = hooks::PreHookCall {
                tool_call_id: pending.tool_call_id.as_str(),
                tool_name: &facts.tool_name,
                args: &pending.args,
                effect: &facts.effect,
                write_targets: &facts.write_targets,
                read_targets: &facts.read_paths,
                argv: facts.argv.as_deref(),
                cwd: session.cwd(),
                history: &history,
            };
            hook.pre(&pre_call).await
        };
        let outcome = match &constraint {
            Ok(constraint) => constraint.outcome(),
            Err(error) => hook_format::failed(&error.to_string()),
        };
        record_hook(
            session,
            render,
            speaker,
            HookPoint::PreToolUse,
            hook.command(),
            outcome,
        )?;

        // 只有 `Tighten` 会强制一个裁决；其余的都是流程。
        hook_verdict = constraint.as_ref().ok().and_then(Constraint::tightening);

        match constraint {
            Ok(Constraint::Continue | Constraint::Tighten(_)) => {}
            Ok(Constraint::Rewrite(new_args)) => {
                // 权限门与工具看到的都是改写后的调用，所以两边读之前
                // 事实要重新解析一次。
                pending.args = new_args;
                facts =
                    match session
                        .tools()
                        .facts(&pending.tool_name, &pending.args, &pending.paths)
                    {
                        Ok(facts) => facts,
                        Err(error) => {
                            emit_completed(
                                session,
                                render,
                                speaker,
                                tool_call_id,
                                Err(error),
                                started,
                            )?;
                            return Ok(Disposition::Finished);
                        }
                    };
            }
            Ok(Constraint::Skip) => {
                let skipped = ToolError::message("钩子跳过了执行：工具没有跑");
                emit_completed(
                    session,
                    render,
                    speaker,
                    tool_call_id,
                    Err(skipped),
                    started,
                )?;
                return Ok(Disposition::Finished);
            }
            Ok(Constraint::Stop) => {
                // 这个回合到此为止，但这次调用已经启动了，所以它仍然欠着
                // 恰好一个结果。这一批里其余的调用从没启动过，因此不欠。
                let stopped = ToolError::message(HOOK_STOPPED_TURN);
                emit_completed(
                    session,
                    render,
                    speaker,
                    tool_call_id,
                    Err(stopped),
                    started,
                )?;
                return Ok(Disposition::Stopped(HOOK_STOPPED_TURN));
            }
            Err(error) => {
                // 失败即关闭：挡住这次动作、诊断它、并合成这次调用的那一个
                // 错误结果。回合继续，所以一个坏掉的钩子仍然可被诊断，
                // 而不会变成致命的。
                render.diagnostic(&format!(
                    "{} 的 hook.pre 失败：{error}；这次动作被挡住",
                    pending.tool_name
                ));
                let blocked = ToolError::message(format!("钩子失败，动作被挡住：{error}"));
                emit_completed(
                    session,
                    render,
                    speaker,
                    tool_call_id,
                    Err(blocked),
                    started,
                )?;
                return Ok(Disposition::Finished);
            }
        }
    }

    // ② 权限门，③ 询问。最终裁决是钩子那条约束与权限门自己裁决的
    //    上确界。
    let authorized = authorize(
        session,
        render,
        speaker,
        &tool_call_id,
        &pending.args,
        &facts,
        hook_verdict,
    )
    .await?;

    let (allowed, outcome) = match authorized {
        Authorized::Refuse { message } => (
            None,
            DispatchOutcome::failure(ToolError::message(message), false),
        ),
        Authorized::Allow => {
            // 护栏是对事实加上这个 agent 的读集合的一次纯读取；决定在
            // `finish_call` 里施加到读集合上。
            match facts.guardrails(session.read_set()) {
                GuardedCall::Refused(error) => (None, DispatchOutcome::failure(error, false)),
                GuardedCall::Run(allowed) => {
                    // 一次 `task` 调用经循环就地构造的端口派发：这一层才是持有 provider 与
                    // 渲染器的地方，而逐次授权调用构造端口，正是让每个执行者在关于它的任何
                    // 东西被记录之前就有自己的 id 的原因。
                    if pending.tool_name == TASK_TOOL {
                        // 会话在它第二个落点上的硬停（spec §17）：一个额度耗尽的会话不派发
                        // **新的**执行者。已经在跑的执行者不受影响 —— 它们按自己的回合上限
                        // 跑完。这次调用已经启动，所以它仍然拿到那一个结果，而那条结果说
                        // 执行者从没跑过。
                        let spent = total_usage(&session.events()).total_tokens();
                        if let Some(note) = session.config().budget.exhausted_note(spent) {
                            render.diagnostic(&format!("{note}；没有派发新的执行者"));
                            let refused = ToolError::message(BUDGET_NO_NEW_EXECUTOR);
                            emit_completed(
                                session,
                                render,
                                speaker,
                                tool_call_id,
                                Err(refused),
                                started,
                            )?;
                            return Ok(Disposition::Finished);
                        }
                        *executors_spawned += 1;
                        pending.executor = Some(Arc::new(ExecutorPort::new(
                            session,
                            speaker,
                            provider,
                            render,
                            ParticipantId::new(format!("{speaker}-{executors_spawned}")),
                            cancelled,
                        )));
                        return Ok(Disposition::Deferred(Box::new(DeferredCall {
                            pending,
                            allowed,
                            started,
                        })));
                    }
                    // ④ 派发，原地：碰工作区的调用在它一向所在的地方、按这一批的顺序跑。
                    //    手势在这里也会被 select，所以一个进行中的工具会被就地丢掉；这次调用
                    //    仍然保留它那一个必需的结果，在下面合成（spec §6）。
                    let tools = session.shared_tools();
                    let mut cancel = cancelled.clone();
                    let outcome = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => {
                            let stopped = ToolError::message(CANCELLED_IN_FLIGHT);
                            emit_completed(
                                session,
                                render,
                                speaker,
                                tool_call_id,
                                Err(stopped),
                                started,
                            )?;
                            return Ok(Disposition::Stopped(CANCELLED_BEFORE_RUN));
                        }
                        outcome = tools.dispatch(&pending, &allowed) => outcome,
                    };
                    finish_call(
                        session,
                        render,
                        speaker,
                        CallCompletion {
                            pending: &pending,
                            allowed: Some(&allowed),
                            started,
                            dispatched: true,
                            outcome,
                        },
                    )
                    .await?;
                    return Ok(Disposition::Finished);
                }
            }
        }
    };

    finish_call(
        session,
        render,
        speaker,
        CallCompletion {
            pending: &pending,
            allowed,
            started,
            dispatched: false,
            outcome,
        },
    )
    .await?;
    Ok(Disposition::Finished)
}

/// 把这一批里被推迟的调用一起跑，最多同时
/// [`SessionConfig::max_parallel_executors`] 个，并按这一批的顺序记录它们的结果。
///
/// 这才是「一批里几个执行者同时跑」在没有第二套投递机制的情况下成真的原因：每次调用仍然是
/// 一次普通的工具调用、恰好一个结果，而真正要紧的那种写互斥发生在执行者内部、在共享路径锁
/// 上（spec §16）。这条上限是成本与速率的闸门，不是安全的闸门。
async fn run_deferred(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    deferred: Vec<DeferredCall>,
) -> Result<(), Error> {
    if deferred.is_empty() {
        return Ok(());
    }
    let cap = session.config().max_parallel_executors.max(1);
    // 一个共享句柄，这样各个 future 借的是工具表而不是会话：结果落地时，会话还是循环可以
    // 继续改的东西。
    let tools = session.shared_tools();
    // 在 await 之前收集好，这样每个 future 借的是同一个 `deferred`。
    let mut batch = Vec::with_capacity(deferred.len());
    for call in &deferred {
        batch.push(tools.dispatch(&call.pending, &call.allowed));
    }
    let outcomes = futures::stream::iter(batch)
        .buffered(cap)
        .collect::<Vec<_>>()
        .await;

    for (call, outcome) in deferred.into_iter().zip(outcomes) {
        finish_call(
            session,
            render,
            speaker,
            CallCompletion {
                pending: &call.pending,
                allowed: Some(&call.allowed),
                started: call.started,
                dispatched: true,
                outcome,
            },
        )
        .await?;
    }
    Ok(())
}

/// 一次已决策的调用之后的一切：读集合、这次调用的那一个结果，以及后置钩子。
///
/// 两条派发路径共用一份实现，所以无论工具实际在哪条路径上跑过，不变量都成立：一次读只有
/// 在成功时才算读，一次失败的匹配会撤回那条路径的读权限，而结果在后置钩子跑之前就进日志
/// （一个卡住的钩子藏不住一条渲染器本该已经看见的结果）。
async fn finish_call(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    completion: CallCompletion<'_>,
) -> Result<(), Error> {
    let CallCompletion {
        pending,
        allowed,
        started,
        dispatched,
        outcome,
    } = completion;

    if let Some(allowed) = allowed {
        if outcome.is_ok() {
            session.record_reads(&allowed.read_paths);
        }
        if outcome.invalidated_reads {
            if let Some(path) = outcome
                .result
                .as_ref()
                .err()
                .and_then(ToolError::invalidated_path)
            {
                session.invalidate_read(path);
            }
        }
    }

    let (ok, output, error) = match &outcome.result {
        Ok(output) => (true, Some(output.text.clone()), None),
        Err(error) => (false, None, Some(error.to_string())),
    };
    emit_completed(
        session,
        render,
        speaker,
        ToolCallId::new(pending.tool_call_id.clone()),
        outcome.result,
        started,
    )?;

    // ⑤ hook.post。只有工具真的跑了它才跑，而它的失败最多只能丢掉反馈。
    if dispatched {
        run_post_hook(
            session,
            render,
            speaker,
            pending,
            ok,
            output.as_deref(),
            error.as_deref(),
        )
        .await?;
    }
    Ok(())
}

/// 运行时的讨论者：它自己的会话（它的读集合、它的私有身份、它的模型），以及为它作答的
/// provider。
pub struct Debater {
    /// 用户给这一方的人物，如果给了的话：在第一轮之前作为私有注入记录（spec §15）。
    pub soul: Option<String>,
    pub speaker: SpeakerId,
    pub session: Session,
    /// 共享，因为这个讨论者派发的执行者在同一个 client 上作答（spec §16：执行者的模型按
    /// 缺省是继承来的）。
    pub provider: Arc<dyn Provider>,
}

/// 合成器（CONTEXT.md）：一个要写进去的会话，以及一个要调的 provider。
///
/// 它没有发言归属、也没有工具，因为它不是 agent：它是 harness 代表自己做出的那一次调用
/// （spec §15）。
pub struct Synthesizer {
    pub session: Session,
    pub provider: Box<dyn Provider>,
}

/// 一场讨论：名册、收尾调用，以及轮次上限。
pub struct Discussion {
    debaters: Vec<Debater>,
    synthesizer: Synthesizer,
    max_rounds: u32,
    /// 这场讨论要写入的那条流上已有的最高轮次。轮次记为 `round_offset + n`，所以本地计数
    /// `n` 仍然是协议里的那个（「这是第一轮吗？」「碰到上限了吗？」），而流上的数字在会话内
    /// 保持唯一。
    round_offset: u32,
}

/// 一场讨论是怎么结束的，以及合成器从中得出了什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscussionOutcome {
    /// 辩论阶段为什么停下；没人作答时是 `Error`。
    pub reason: StopReason,
    /// 合成器三档的产出。讨论在收尾调用之前就失败、或者那次调用本身没产出东西时为空。
    pub synthesis: String,
    /// 跑了几轮辩论：没有分歧时一轮，有分歧时两轮。
    pub rounds: u32,
    /// 至少缺席一轮的讨论者，按名册顺序。为什么缺席由流上携带（它们自己的
    /// `TurnEnded { Error }`，加上缺失的那条 `MessageCompleted`）；这里只是摘要。
    pub absent: Vec<SpeakerId>,
}

impl Discussion {
    /// 取一份名册。名册的形状在组装期已经校验过，所以这里不再复查。
    ///
    /// 这场讨论记录的轮次号从**流上已有的内容之后**开始：一个会话可以装不止一场讨论
    /// （`/discuss` 就在活着的流上跑），而 `RoundStarted { round }` 得说清自己属于哪一场，
    /// 否则每一个跨轮次的查询都会把它们混在一起（spec §15、`debate_phase_start`）。
    pub fn new(debaters: Vec<Debater>, synthesizer: Synthesizer, max_rounds: u32) -> Self {
        let round_offset = crate::discussion::last_round(&debaters[0].session.events());
        Self {
            debaters,
            synthesizer,
            max_rounds,
            round_offset,
        }
    }

    pub fn session_id(&self) -> &crate::events::SessionId {
        self.debaters[0].session.id()
    }

    /// 代表整场讨论记录的那个会话。
    ///
    /// 讨论者共用一条日志，所以它们任何一个的会话都能追加一个轮次边界、或把流读回来。走一个
    /// 访问器，让「哪个会话代表这场讨论说话」成为一个在一处做出的决定，而不是在每个调用点
    /// 重复的 `debaters[0]`。
    fn recorder(&mut self) -> &mut Session {
        &mut self.debaters[0].session
    }

    /// 共享的那条流，以快照的形式。
    fn stream(&self) -> Vec<Event> {
        self.debaters[0].session.events()
    }
}

/// 跑一场讨论：一次独立首轮，只在结论冲突时来一次定向的第二轮，然后是合成器那一次调用
/// （spec §15）。
///
/// 控制流住在这个层里，因为这个层是事件流的唯一写者、也是 provider 的唯一调用者（spec §3）；
/// [`crate::discussion`] 持有这个函数所施加的那些规则。
///
/// 失败语义全都是「记录并继续，绝不重跑」：一边失败就是那一边这一轮的缺席，讨论照走；两边
/// 都失败是一个以 `Error` 收场的轮次加上一条 `SessionError`，没有收尾调用。
pub async fn run_discussion(
    discussion: &mut Discussion,
    render: &RenderHandle,
    question: &str,
    cancelled: &CancelObserver,
) -> Result<DiscussionOutcome, Error> {
    // 问题就是用户自己的消息，在各轮之前记录一次，好让两个讨论者投影到同一条。
    record_user_message(discussion.recorder(), render, question)?;
    // 人物，在第一轮之前：每一份都是归属给它所描述的那个讨论者的用户文本，所以投影只把它
    // 交给那一边。记录而不是放在私有身份里，这样一轮的 `messages` 永远能从流上重算
    // （spec §5、§15）。
    let personas: Vec<(SpeakerId, String)> = discussion
        .debaters
        .iter()
        .filter_map(|debater| {
            debater.soul.as_deref().map(|soul| {
                (
                    debater.speaker.clone(),
                    crate::discussion::persona_brief(soul),
                )
            })
        })
        .collect();
    for (speaker, brief) in personas {
        let source = ContextSource::Persona(match &speaker {
            SpeakerId::Debater(name) => name.clone(),
            other => crate::events::ParticipantId(other.to_string()),
        });
        record_context_injection_from(discussion.recorder(), render, &speaker, source, &brief)?;
    }

    let mut rounds: u32 = 0;
    // 这场讨论的轮次号在流上从哪儿开始（见 `Discussion`）。
    let offset = discussion.round_offset;
    let mut absent: Vec<SpeakerId> = Vec::new();
    // 会话的额度是每个参与者共用的同一个值（spec §17）；组装期会拒绝在它上面不一致的名册，
    // 所以记录者的那一份代表整场讨论。
    let budget = discussion.debaters[0].session.config().budget.clone();

    let reason = loop {
        // 在这一轮打开之前落下的手势什么都没打开：辩论阶段就地结束（spec §6）。
        if cancelled.is_cancelled() {
            break StopReason::Aborted;
        }

        // 会话的硬停（spec §17）：额度耗尽的会话不再打开新的轮次，直接走向合成器 —— 那唯一
        // 一次绝不能跳过的调用。带着 `rounds > 0` 走到这个循环顶部，说明前一轮**没有**结束
        // 辩论，所以那一轮就是这个原因所收束的；还没跑过任何一轮时，没有轮次边界可记录，
        // 这个原因只随 `DiscussionOutcome` 传出去。
        let spent = total_usage(&discussion.stream()).total_tokens();
        if let Some(note) = budget.exhausted_note(spent) {
            render.diagnostic(&format!("{note}；直接走向合成器"));
            if rounds > 0 {
                record_round_ended(
                    discussion.recorder(),
                    render,
                    offset + rounds,
                    StopReason::BudgetExhausted,
                )?;
            }
            break StopReason::BudgetExhausted;
        }

        rounds += 1;
        let mode = if rounds == 1 {
            RoundMode::Independent
        } else {
            RoundMode::Targeted
        };
        // 落进流的是那个会话内唯一的数字；`rounds` 保持这场讨论自己的计数，协议规则正是用
        // 那个计数写的。
        let recorded = offset + rounds;
        let started = record_round_started(discussion.recorder(), render, recorded, mode)?;

        // 两个讨论者同时作答。`join_all` 在这个任务上交替轮询两个回合：一个在等自己的
        // provider 流时，另一个就在推进 —— 对两个受网络所限的回合来说，「并发」能有的意思
        // 就是这些 —— 而且这让两个回合都经同一条共享日志写。
        let scope = TurnScope::Round {
            before_seq: started.seq,
        };
        let turns = futures::future::join_all(discussion.debaters.iter_mut().map(|debater| {
            run_turn(
                &mut debater.session,
                &debater.speaker,
                &debater.provider,
                render,
                scope,
                cancelled,
            )
        }))
        .await;
        for turn in turns {
            // 日志写入失败是一个回合唯一会当作错误返回的东西；
            // 那时流已经不能用了，讨论也一样。
            turn?;
        }

        // 手势压过这一轮自己的判定。用户停下的一轮不是一个辩论结果，而把按下之前恰好落地的
        // 那一个回答读成一致，正是缺席查询存在所要防的那种误读（spec §6）。
        if cancelled.is_cancelled() {
            record_round_ended(
                discussion.recorder(),
                render,
                offset + rounds,
                StopReason::Aborted,
            )?;
            break StopReason::Aborted;
        }

        // 把这一轮从流上读回来。出席、顺序、一致与缺席全都是对事件的查询，从不是循环状态
        // （spec §15）。
        let attendance =
            crate::discussion::protocol::round_attendance(&discussion.stream(), recorded);
        for speaker in &attendance.absent {
            if !absent.contains(speaker) {
                absent.push(speaker.clone());
            }
        }

        // 根本没人作答：这是会话级失败而不是辩论结果，而合成器也没什么可合成的。除非是预算
        // 停下了每一方 —— 那是硬停在做它该做的事（降级收尾），把它记成故障会让闸门看起来像
        // 坏掉了（spec §17）。
        if attendance.answers.is_empty() {
            if budget.is_exhausted(total_usage(&discussion.stream()).total_tokens()) {
                record_round_ended(
                    discussion.recorder(),
                    render,
                    recorded,
                    StopReason::BudgetExhausted,
                )?;
                break StopReason::BudgetExhausted;
            }
            record_round_ended(discussion.recorder(), render, recorded, StopReason::Error)?;
            record_session_error(
                discussion.recorder(),
                render,
                "discussion_failed",
                "这一轮没有讨论者作答",
            )?;
            return Ok(DiscussionOutcome {
                reason: StopReason::Error,
                synthesis: String::new(),
                rounds,
                absent,
            });
        }

        let outcome = crate::discussion::protocol::round_outcome(&attendance);
        if outcome == crate::discussion::protocol::RoundOutcome::Diverged {
            let positions = attendance
                .answers
                .iter()
                .map(|(_, answer)| crate::discussion::position_of(answer))
                .collect();
            record_divergence(
                discussion.recorder(),
                render,
                recorded,
                &crate::discussion::divergence_topic(question),
                positions,
            )?;
        }

        // 会话的硬停在这个循环顶部已经表过态了：额度耗尽的会话永远走不到这个判定，它直接去
        // 合成器（spec §17）。这里剩下的是协议自己的四个原因。
        match crate::discussion::plan_after_round(outcome, rounds, discussion.max_rounds) {
            crate::discussion::RoundPlan::Stop(reason) => {
                record_round_ended(discussion.recorder(), render, recorded, reason)?;
                break reason;
            }
            // 没有结束辩论的那一轮不写 `RoundEnded`：下一个 `RoundStarted` 会把它关上。这让
            // 每一条 `RoundEnded` 都是渲染器能据以行动的原因，而那四个终值正是为此存在。
            crate::discussion::RoundPlan::TargetedRound => continue,
        }
    };

    // 一场被取消的讨论根本不会靠近合成器：手势的意思就是停，而收尾调用和其它调用一样是一次
    // provider 调用。把辩论阶段以 `Aborted` 而不是 `Error` 收场，正是「用户停下了它」不至于
    // 被记成失败的原因（spec §6）。
    if reason == StopReason::Aborted {
        return Ok(DiscussionOutcome {
            reason,
            synthesis: String::new(),
            rounds,
            absent,
        });
    }

    // 合成器：那一次绝不能跳过的调用。它不是回合、也不是参与者，但它被一个轮次括起来，所以
    // 流上仍然说得出它是什么时候跑的。
    let synthesis_round = offset + rounds + 1;
    record_round_started(
        discussion.recorder(),
        render,
        synthesis_round,
        RoundMode::Synthesis,
    )?;
    // 材料的作用域限于这次合成所收尾的那个阶段：流上可能已经带着更早一场讨论的轮次。
    let materials = discussion.stream();
    let prompt = crate::discussion::synthesis_prompt(
        question,
        &materials,
        crate::discussion::debate_phase_start(&materials, synthesis_round),
    );
    let synthesis = run_single_shot(
        &mut discussion.synthesizer.session,
        discussion.synthesizer.provider.as_ref(),
        render,
        &prompt,
        cancelled,
    )
    .await?;

    // 一个到达收尾调用的手势也让讨论就此结束：不要半个产出，也不要为一次用户停下的调用写
    // `synthesis_failed`。在调用已经到达 `[DONE]` 之后才到的手势不会撤销它 —— 完成的单位
    // 保持完成，与一个回合自己那条完成的消息一模一样。
    let cancelled_in_synthesis = synthesis.is_none() && cancelled.is_cancelled();
    let ended = if cancelled_in_synthesis {
        StopReason::Aborted
    } else if synthesis.is_some() {
        StopReason::Completed
    } else {
        record_session_error(
            &mut discussion.synthesizer.session,
            render,
            "synthesis_failed",
            "合成器这次调用没有产出",
        )?;
        StopReason::Error
    };
    record_round_ended(
        &mut discussion.synthesizer.session,
        render,
        synthesis_round,
        ended,
    )?;

    Ok(DiscussionOutcome {
        // 停下这场讨论的是手势，不是辩论阶段。
        reason: if cancelled_in_synthesis {
            StopReason::Aborted
        } else {
            reason
        },
        synthesis: synthesis.unwrap_or_default(),
        rounds,
        absent,
    })
}

/// 一次独立的单发调用：合成器的形状（spec §15）。
///
/// 不是回合：没有 `TurnStarted`、没有 `TurnEnded`、没有工具、没有迭代。它的产出是一条来自
/// `System` 的 `MessageCompleted` —— harness 自己的声音，也是渲染器的最终产物 —— 而它的
/// 用量照样落在流上，会话的花费就是从那里求和出来的（spec §17）。
///
/// `Ok(None)` 表示这次调用没有产出：provider 失败、一条从没到 `[DONE]` 的流、空回答，或者
/// 一次取消手势（spec §6）。那不是日志错误，但调用方仍然得说清它意味着什么 —— 讨论失败
/// （`SessionError`、`RoundEnded { Error }`）还是被取消（`RoundEnded { Aborted }`，无
/// 错误）。只有调用方持有那个手势，所以只有调用方能区分这两者。
///
/// 会话的 token 额度刻意**不**给这次调用设闸：合成器是那唯一一次绝不能跳过的调用
/// （spec §17），这就是硬停把辩论阶段降级进它、而不是越过它的原因。
pub async fn run_single_shot(
    session: &mut Session,
    provider: &dyn Provider,
    render: &RenderHandle,
    prompt: &str,
    cancelled: &CancelObserver,
) -> Result<Option<String>, Error> {
    // 在请求构造之前就检查，而不是只在流内部检查：一次还没发出去的调用，绝不能在手势之后
    // 才发出去。
    if cancelled.is_cancelled() {
        return Ok(None);
    }

    let mut messages = Vec::new();
    if let Some(identity) = session.identity() {
        messages.push(Message::System {
            content: identity.to_owned(),
            name: None,
        });
    }
    messages.push(Message::User {
        content: prompt.to_owned(),
        name: None,
        // 这不是一次 `ContextInjected` 投影：合成器自己的简报是这里唯一一条 `user` 消息，
        // 而这条路径从不裁剪（spec §15）。
        injected: false,
    });

    let request = ChatRequest {
        model: session.config().model.clone(),
        messages,
        tools: Vec::new(),
        tool_choice: ToolChoice::None,
        params: session.config().params.clone(),
        cache_key: Some(session.id().as_str().to_owned()),
    };

    let mut cancel = cancelled.clone();
    let sent = tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            render.diagnostic("合成器的调用被取消了");
            return Ok(None);
        }
        sent = provider.send(request) => sent,
    };
    let mut stream = match sent {
        Ok(stream) => stream,
        Err(error) => {
            render.diagnostic(&format!("合成器的 provider 错误：{error}"));
            return Ok(None);
        }
    };

    let mut text = String::new();
    let mut saw_done = false;
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                render.diagnostic("合成器的流进行中时，它的调用被取消了");
                // 没有 `[DONE]` 就没有产出：丢掉这条流就是手势在这里的全部效果。
                return Ok(None);
            }
            item = stream.next() => match item {
                Some(Ok(StreamEvent::TextDelta(delta))) => {
                    render.text_delta(&SpeakerId::System, &delta);
                    text.push_str(&delta);
                }
                Some(Ok(StreamEvent::ReasoningDelta(delta))) => {
                    render.reasoning_delta(&SpeakerId::System, &delta);
                }
                Some(Ok(StreamEvent::Usage(usage))) => {
                    emit(
                        session,
                        render,
                        &SpeakerId::System,
                        EventPayload::UsageRecorded { usage },
                    )?;
                }
                Some(Ok(StreamEvent::Finished { finish_reason })) => {
                    render.diagnostic(&format!(
                        "合成器的流结束：{}",
                        crate::render::wording::finish_reason(&finish_reason)
                    ));
                    saw_done = true;
                    break;
                }
                // 没有提供任何工具，所以这里的一次调用是协议违规，而不是要派发的活。它仍然
                // 不能被派发：合成器没有可派发进去的工具表。
                Some(Ok(StreamEvent::ToolCallStarted { .. }))
                | Some(Ok(StreamEvent::ToolCallCompleted { .. })) => {
                    render.diagnostic("合成器要了一个工具；已忽略");
                }
                Some(Err(error)) => {
                    render.diagnostic(&format!("合成器的流错误：{error}"));
                    break;
                }
                None => break,
            },
        }
    }

    if !saw_done || text.trim().is_empty() {
        return Ok(None);
    }
    // 在这里打一次码，早于它既被发出、又被返回：讨论交回去的产出与流上携带的是同一段文本。
    let text = session.redacted(&text);
    emit(
        session,
        render,
        &SpeakerId::System,
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: text.clone(),
            reasoning: None,
        },
    )?;
    Ok(Some(text))
}

/// 记录一个轮次边界。协议决定*何时*（spec §15）；这一层负责写，所以「只有循环写流」保持
/// 字面为真。
pub fn record_round_started(
    session: &mut Session,
    render: &RenderHandle,
    round: u32,
    mode: RoundMode,
) -> Result<Event, Error> {
    emit_returning(
        session,
        render,
        &SpeakerId::System,
        EventPayload::RoundStarted { round, mode },
    )
}

/// 记录一个结束辩论的轮次的结尾，带上协议给出的原因。
pub fn record_round_ended(
    session: &mut Session,
    render: &RenderHandle,
    round: u32,
    reason: StopReason,
) -> Result<(), Error> {
    emit(
        session,
        render,
        &SpeakerId::System,
        EventPayload::RoundEnded { round, reason },
    )
}

/// 记录一次冲突：题目，以及每一方申明的立场。
pub fn record_divergence(
    session: &mut Session,
    render: &RenderHandle,
    round: u32,
    topic: &str,
    positions: Vec<String>,
) -> Result<(), Error> {
    emit(
        session,
        render,
        &SpeakerId::System,
        EventPayload::DivergenceRecorded {
            round,
            topic: topic.to_owned(),
            positions,
        },
    )
}

/// 记录一次会话级失败：一次模型永远看不到的运行失败（spec §2）。
pub fn record_session_error(
    session: &mut Session,
    render: &RenderHandle,
    code: &str,
    detail: &str,
) -> Result<(), Error> {
    emit(
        session,
        render,
        &SpeakerId::System,
        EventPayload::SessionError {
            code: code.to_owned(),
            detail: detail.to_owned(),
        },
    )
}

/// 权限门与用户都说过话之后，循环对一次调用必须做什么。
enum Authorized {
    Allow,
    /// 这次调用根本到不了工具；那条消息就是它那一个必需的结果。
    Refuse {
        message: String,
    },
}

/// 对一次调用施加权限门，把前置钩子的约束并进它的裁决，并在最终裁决是 `Ask` 时通过注入的
/// 端口问用户。
///
/// 权限门本身是纯的，从不发问、从不读环境、也从不写事件。一切交互都住在这里：那次询问、
/// 会话级的「总是允许」，以及无交互前端下 `Ask` 到 `Deny` 的降级（它的理由落进
/// `PermissionDecided`，所以审计能区分一次没有终端造成的拒绝和一次策略拒绝）。
///
/// `hook_verdict` 是前置钩子的 `Tighten` 强制过的裁决（如果有）。最终裁决是两者的上确界 ——
/// 钩子能抬高一个裁决，却永远抬不低，因为存在的那几种收紧只有 `Ask` 与 `Deny`。
async fn authorize(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    tool_call_id: &ToolCallId,
    args: &serde_json::Value,
    facts: &CallFacts,
    hook_verdict: Option<Decision>,
) -> Result<Authorized, Error> {
    let request_id = format!("perm-{tool_call_id}");
    let path_error = facts.path_error.as_ref().map(ToString::to_string);
    let verdict = {
        let call = permissions::Call {
            tool_name: &facts.tool_name,
            effect: &facts.effect,
            write_targets: &facts.write_targets,
            read_targets: &facts.read_paths,
            argv: facts.argv.as_deref(),
            cwd: session.cwd(),
            home: session.home(),
            path_error: path_error.as_deref(),
        };
        permissions::decide(&session.policy(), speaker, &call)
    };

    // 那唯一一次合并：`Allow < Ask < Deny`。一个收紧到权限门本来就说过的东西的钩子什么都没
    // 改变，因此它也不是被记录下来的那次决定的来源。
    let effective = hooks::effective_verdict(verdict.decision, hook_verdict);
    let tightened_by_hook = hook_verdict.is_some_and(|hook| hook > verdict.decision);
    let hook_note = tightened_by_hook.then_some("（钩子收紧了这个裁决）");
    let annotated = annotate_reason(&verdict.reason, hook_note);

    match effective {
        Decision::Allow => {
            record_decision(
                session,
                render,
                speaker,
                &request_id,
                Decision::Allow,
                DecisionSource::Policy,
                verdict.reason,
            )?;
            Ok(Authorized::Allow)
        }
        Decision::Deny => {
            let source = if tightened_by_hook {
                DecisionSource::Hook
            } else {
                DecisionSource::Policy
            };
            record_decision(
                session,
                render,
                speaker,
                &request_id,
                Decision::Deny,
                source,
                annotated.clone(),
            )?;
            Ok(refuse(&annotated))
        }
        Decision::Ask => {
            // 这次询问的存在可能正是某个钩子造成的；把那一点带进提问与审计，
            // 而不把它变成第四个状态。
            let ask_reason = annotated;

            let Some(asker) = session.asker().cloned() else {
                // 权限门保持它那个忠实的 `Ask`；循环才是把「没有作答者」
                // 变成拒绝的地方，而且它说了出来。
                let reason = format!("{ask_reason}；降级为拒绝：没有可交互的作答者");
                record_decision(
                    session,
                    render,
                    speaker,
                    &request_id,
                    Decision::Deny,
                    DecisionSource::Policy,
                    reason.clone(),
                )?;
                return Ok(refuse(&reason));
            };

            let request = PermissionRequest {
                request_id: request_id.clone(),
                tool_call_id: tool_call_id.as_str().to_owned(),
                tool_name: facts.tool_name.clone(),
                args: args.clone(),
                reason: ask_reason.clone(),
            };
            emit(
                session,
                render,
                speaker,
                EventPayload::PermissionAsked {
                    request_id: request_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    request: crate::events::permission_format::request(
                        &facts.tool_name,
                        args,
                        &ask_reason,
                    ),
                },
            )?;

            match asker.ask(&request).await {
                Answer::Allow => {
                    record_decision(
                        session,
                        render,
                        speaker,
                        &request_id,
                        Decision::Allow,
                        DecisionSource::User,
                        format!("用户允许：{ask_reason}"),
                    )?;
                    Ok(Authorized::Allow)
                }
                Answer::AlwaysAllow => {
                    // 只落在会话策略里：不写 `config.toml`，不发事件。
                    session
                        .remember_allow(permissions::Rule::always_allow(speaker, &facts.tool_name));
                    record_decision(
                        session,
                        render,
                        speaker,
                        &request_id,
                        Decision::Allow,
                        DecisionSource::User,
                        format!("用户总是允许：{ask_reason}"),
                    )?;
                    Ok(Authorized::Allow)
                }
                Answer::Deny => {
                    let reason = format!("用户拒绝：{ask_reason}");
                    record_decision(
                        session,
                        render,
                        speaker,
                        &request_id,
                        Decision::Deny,
                        DecisionSource::User,
                        reason.clone(),
                    )?;
                    Ok(refuse(&reason))
                }
            }
        }
    }
}

/// 合成出来的拒绝消息只有这一种形状，这样每一条拒绝路径在工具结果里读起来都一样。
fn refuse(reason: &str) -> Authorized {
    Authorized::Refuse {
        message: format!("权限拒绝：{reason}"),
    }
}

/// 钩子抬高了裁决时，把它的备注附到裁决理由后面。
fn annotate_reason(reason: &str, hook_note: Option<&str>) -> String {
    match hook_note {
        // 备注是中文括号里的一句话，紧贴着接上去，中间不加空格。
        Some(note) => format!("{reason}{note}"),
        None => reason.to_owned(),
    }
}

/// 记录一次权限裁决。每次调用恰好得到一条，无论有没有问过，所以 `decision × source` 是一
/// 个完整的计数器（票 19）。
fn record_decision(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    request_id: &str,
    decision: Decision,
    source: DecisionSource,
    reason: String,
) -> Result<(), Error> {
    emit(
        session,
        render,
        speaker,
        EventPayload::PermissionDecided {
            request_id: request_id.to_owned(),
            decision,
            source,
            reason: Some(reason),
        },
    )
}

/// 追加一次调用被欠的那一条 `ToolCallCompleted`，无论它出自哪条路径：真的派发、权限拒绝、
/// 钩子跳过，还是钩子失败。
///
/// 把合成放在一处，正是让「每一个 `tool_call` 恰好拿到一个结果」可被检查、而不是散在五个
/// 分支上的一条约定。
fn emit_completed(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    tool_call_id: ToolCallId,
    result: Result<ToolOutput, ToolError>,
    started: Instant,
) -> Result<(), Error> {
    let duration_ms = started.elapsed().as_millis() as u64;
    // 进流之前的流水线是打码 -> 裁剪 -> 溢出落盘（spec §10、§20），这里就是它跑的地方：先
    // 打码，这样磁盘上那份 `.txt` 产物也是打过码的，而不只是进流的那段预览。上面那个工具早
    // 就在真值上跑过了 —— 只有离开这个进程的东西才被打码。
    let max_tokens = session.config().max_tool_result_tokens;
    // 成功正文与失败正文裁剪方式一样；不同的只是预览落在 payload 的哪个字段里。
    let (ok, text) = match result {
        Ok(output) => (true, output.text),
        Err(error) => (false, error.to_string()),
    };
    let text = session.redacted(&text);
    let preview = context::truncate_result(
        &text,
        tool_call_id.as_str(),
        session.outputs_dir(),
        max_tokens,
    )
    .preview;
    let (output, error) = if ok {
        (Some(preview), None)
    } else {
        (None, Some(preview))
    };
    emit(
        session,
        render,
        speaker,
        EventPayload::ToolCallCompleted {
            tool_call_id,
            ok,
            output,
            error,
            duration_ms,
        },
    )
}

/// 追加一条 `HookExecuted`。
///
/// 两个挂载点和每一种结果 —— 包括失败 —— 都走这里，所以票 19 能按钩子结果给流分组，不需要
/// 第二种事件。
fn record_hook(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    point: HookPoint,
    command: &str,
    outcome: String,
) -> Result<(), Error> {
    emit(
        session,
        render,
        speaker,
        EventPayload::HookExecuted {
            point: point.as_str().to_owned(),
            command: command.to_owned(),
            outcome,
        },
    )
}

/// 为一次工具真的跑过的调用运行 `hook.post`。
///
/// 这里的失败刻意与 `hook.pre` 不对称：世界已经变了，所以一个坏掉的后置钩子最多只能丢掉它
/// 的反馈。失败会被记录并诊断；这次调用的结果已经在日志里，并且留在那里。
async fn run_post_hook(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    pending: &PendingCall,
    ok: bool,
    output: Option<&str>,
    error: Option<&str>,
) -> Result<(), Error> {
    let Some(hook) = session.hook().cloned() else {
        return Ok(());
    };
    let feedback = {
        let history = hooks::public_history(&session.events());
        let post_call = hooks::PostHookCall {
            tool_call_id: &pending.tool_call_id,
            tool_name: &pending.tool_name,
            args: &pending.args,
            ok,
            output,
            error,
            history: &history,
        };
        hook.post(&post_call).await
    };

    match feedback {
        Ok(None) => record_hook(
            session,
            render,
            speaker,
            HookPoint::PostToolUse,
            hook.command(),
            hook_format::OUTCOME_CONTINUE.to_owned(),
        ),
        Ok(Some(text)) => record_hook(
            session,
            render,
            speaker,
            HookPoint::PostToolUse,
            hook.command(),
            hook_format::feedback(&text),
        ),
        Err(error) => {
            render.diagnostic(&format!(
                "{} 的 hook.post 失败：{error}；反馈被丢掉",
                pending.tool_name
            ));
            record_hook(
                session,
                render,
                speaker,
                HookPoint::PostToolUse,
                hook.command(),
                hook_format::failed(&error.to_string()),
            )
        }
    }
}

/// 记下这个回合停下的原因，并返回结局。
///
/// 返回的文本像一切离开 harness 的东西一样被打过码：前端或执行者摘要拿到的就是流上携带的
/// 那个值，所以内存里不会存在第二份没打码的密钥副本等着被打印出来。
fn end_turn(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    reason: StopReason,
    text: String,
) -> Result<TurnOutcome, Error> {
    emit(session, render, speaker, EventPayload::TurnEnded { reason })?;
    let text = session.redacted(&text);
    Ok(TurnOutcome { reason, text })
}

/// 参数是以 JSON 字符串到达的；没有参数的调用什么都不发，
/// 那意味着「没有参数」，而不是 JSON 字面量 `null`。
fn parse_tool_args(arguments: &str) -> serde_json::Value {
    if arguments.trim().is_empty() {
        return serde_json::json!({});
    }
    serde_json::from_str(arguments).unwrap_or(serde_json::Value::Null)
}

/// 往流上追加一条事件，并把它叙述给渲染器。
///
/// **唯一的写路径**（spec §3，不变量 3）：循环通过一个会话到达它，它驱动的执行者端口通过
/// 共享的日志句柄到达它 —— 这正是让「`agent` 层是唯一写者」成为一个函数、而不是一条约定
/// 的原因。
///
/// 打码是追加**之前**发生的最后一件事，也是 payload 在进流路上唯一被做的事（spec §20）：
/// 每一个自由文本字段都用会话的 [`Redactor`] 打码，所以流、文件与渲染器携带的都是模型将要
/// 重放的那段文本。产出这段文本的那个工具更早跑过，跑在真值上。
///
/// 日志是一个便宜的共享句柄，所以通过克隆追加与那个会话自己追加是同一次追加：一个写者、
/// 一个 `seq`、一行。
pub(super) fn append_event(
    log: &EventLog,
    redactor: &Redactor,
    render: &RenderHandle,
    speaker_id: SpeakerId,
    mut payload: EventPayload,
) -> Result<Event, Error> {
    let mut log = log.clone();
    payload.redact(redactor);
    let event = log.append(speaker_id, payload)?;
    render.logged(&event);
    Ok(event)
}

fn emit(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    payload: EventPayload,
) -> Result<(), Error> {
    emit_returning(session, render, speaker, payload).map(|_| ())
}

/// 与 [`emit`] 相同，但把事件交回来。
///
/// 轮次循环需要一条 `RoundStarted` 的 `seq`：那个数字就是一轮投影窗口所取自的切点
/// （spec §15）。
fn emit_returning(
    session: &mut Session,
    render: &RenderHandle,
    speaker: &SpeakerId,
    payload: EventPayload,
) -> Result<Event, Error> {
    append_event(
        session.log(),
        session.redactor(),
        render,
        speaker.clone(),
        payload,
    )
}
