//! 投影：`(&[Event], SpeakerId, ModelCaps) -> messages`。
//!
//! 它是事件流加上归属规则的纯函数。它不持有任何裁剪状态；裁剪是另一道纯步骤，落在票 07。同一批
//! 事件总是投影出同样的 `messages`，这正是「任何 agent 的 `messages` 都能从流 + 规则重算出来」
//! （spec §5）成立的原因。
//!
//! 本模块独占那个判定「自己」与「别人」的地方：
//!
//! * 正在发言的那一位自己的回合变成 `assistant`；其他人的变成 `user`（spec §5），因为两家厂商都
//!   没记下它如何处理连续多条同角色消息，所以连续的别人发言合并成一条 `user` 消息，而讨论轮次里
//!   的每一段都带一个 `[轮 N · 名字]` 前缀；
//! * 另一个讨论者的工具调用只以一行摘要留下来 —— 结果正文与 `reasoning_content` 不投影，与之配
//!   对的那条 `tool` 结果也一并丢掉（线上层的 `tool_call` ↔ `tool` 配对检查会拒掉别的做法）；
//! * 发言者自己的 `reasoning_content` 会重放（DeepSeek 缺了它直接 400），而它自己的工具往返加上
//!   一条 `PostToolUse` 钩子的反馈，按 `seq` 合并进 provider 唯一允许的那条 `tool` 消息。
//!
//! 逐字段的差异以数据形式住在 [`ModelCaps`] 上，所以这个文件按值分支，绝不按厂商名分支
//! （spec §5）。
//!
//! 这里的 `[轮 N · 名字]` 是**模型**那个前缀。渲染器给人看的前缀刻意是另一个生成器（spec §5）：
//! 两者早期看着很像，但一个为人快速浏览而逐行重复，另一个为模型而每个合并块只写一次。

use super::capability::ModelCaps;
use super::{Message, ToolCall};
use crate::events::{
    hook_format, superseded_seqs, ContextSource, Event, EventPayload, SpeakerId, ToolCallId,
};

/// 送进 `name` 字段的最长清洗后参与者名。
///
/// 没有厂商记下 `name` 的字符集或长度，所以这个值保持在任何一个讲道理的实现都会接受的形状上
/// （spec §5）。
const MAX_NAME_CHARS: usize = 64;

/// 另一位讨论者的工具摘要里保留的最长渲染参数列表。
const MAX_TOOL_SUMMARY_CHARS: usize = 160;

/// 从事件流的一段里重算出某个 agent 该重放的 `messages`。
///
/// 传的是一段而不是整份日志：流是只追加的，所以它的一个前缀是完美的合法输入，而一个讨论轮次要的
/// 正好就是这个 —— 讨论者的窗口切在它那一轮的 `RoundStarted`（spec §15）。传事件也让这个函数在
/// 「自己是纯的」这件事上保持一致。
///
/// `caps` 提供 provider 的逐字段事实，所以「模型自己的推理是否必须往返」这样的差异是表里的一个
/// 值，而不是一条代码路径。
pub fn project(events: &[Event], speaker: &SpeakerId, caps: &ModelCaps) -> Vec<Message> {
    let superseded = superseded_seqs(events);
    let mut messages = Vec::new();
    let mut others = OtherBlock::default();
    // 第一条发言 `user` 消息是否已经发出。它是钉住的：会话的开头必须逐字节稳定，前缀缓存才能命
    // 中，所以它绝不会再吸收后来某位的发言（spec §5）。钉住的 `ContextInjected` 是另一回事，不
    // 消耗这个标志。
    let mut head_emitted = false;
    let mut pending: Option<PendingAssistant> = None;
    let mut round: Option<u32> = None;

    for event in events {
        if superseded.contains(&event.seq) {
            continue;
        }
        let mine = &event.speaker_id == speaker;
        // 「什么进入模型上下文」的穷举表（spec §5）。这里刻意没有 `_` 分支：新的 payload 必须在
        // 这里被分类，而不是悄悄默认成不可见。
        match &event.payload {
            // 会话骨架：身份与 harness 记账保持私有。
            EventPayload::SessionStarted { .. } => {}
            // 沙箱状态同样是**只进日志**的 harness 记账（沙箱 spec §8）：它既不是发言、也不是注入，
            // 所以投影是零 —— 这正是 replay 能看到它、而钉住的前缀不受它影响的原因。
            EventPayload::SandboxStatus { .. } => {}
            // 钉住的注入属于钉住的开头，绝不与发言合并：它必须每一回合都看起来一模一样，前缀缓存
            // 才能继续命中（spec §5、§10）。开头那几条注入 —— 项目规矩与技能清单 —— 是**一条**
            // `user` 消息（spec §10、决定 09：「与 AGENTS.md 同一条」），所以连续的一串注入合并成
            // 一条消息，而不是变成连续的同角色消息。会话中途的注入（用户加载的技能正文，spec §9）
            // 前面有历史，所以它自成一条消息。
            EventPayload::ContextInjected { source, content } => {
                // 人物属于它所描述的那一位：那是那个讨论者自己的指令，而它所反驳的另一方没有理由读
                // 到它。其余每一条注入都是用户在对整场会话说话（项目规矩、技能清单、加载的技能正
                // 文），所以它们到达所有人。
                if matches!(source, ContextSource::Persona(_)) && !mine {
                    continue;
                }
                close_pending_if_settled(&mut messages, &mut pending, speaker);
                flush_others(&mut messages, &mut others, &mut head_emitted);
                let leading = at_pinned_head(&messages);
                match messages.last_mut() {
                    Some(Message::User {
                        content: body,
                        name: None,
                        injected: true,
                    }) if leading => {
                        body.push('\n');
                        body.push_str(content);
                    }
                    _ => messages.push(Message::User {
                        content: content.clone(),
                        name: None,
                        injected: true,
                    }),
                }
            }
            EventPayload::SessionEnded { .. } => {}
            // 一轮是一条硬合并边界，而模型侧的前缀记的就是它的编号（spec §5）。
            EventPayload::RoundStarted { round: started, .. } => {
                close_pending_if_settled(&mut messages, &mut pending, speaker);
                flush_others(&mut messages, &mut others, &mut head_emitted);
                round = Some(*started);
            }
            EventPayload::RoundEnded { .. } => {
                close_pending_if_settled(&mut messages, &mut pending, speaker);
                flush_others(&mut messages, &mut others, &mut head_emitted);
                round = None;
            }
            EventPayload::DivergenceRecorded { .. } => {}
            EventPayload::TurnStarted { .. } => {}
            EventPayload::MessageCompleted {
                text, reasoning, ..
            } => {
                if mine {
                    close_pending_if_settled(&mut messages, &mut pending, speaker);
                    flush_others(&mut messages, &mut others, &mut head_emitted);
                    pending = Some(PendingAssistant::new(text, reasoning, caps));
                } else if speaks_to_others(&event.speaker_id) {
                    close_pending_if_settled(&mut messages, &mut pending, speaker);
                    push_other(
                        &mut messages,
                        &mut others,
                        &mut head_emitted,
                        round,
                        &event.speaker_id,
                        text,
                    );
                }
            }
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } => {
                if mine {
                    if let Some(pending) = pending.as_mut() {
                        pending.add_call(tool_call_id, tool_name, args);
                    }
                } else if speaks_to_others(&event.speaker_id) {
                    // 另一位讨论者的工具调用以一行摘要留下来；它的结果正文不会（spec §5）。
                    close_pending_if_settled(&mut messages, &mut pending, speaker);
                    push_other(
                        &mut messages,
                        &mut others,
                        &mut head_emitted,
                        round,
                        &event.speaker_id,
                        &tool_summary(tool_name, args),
                    );
                }
            }
            EventPayload::ToolCallCompleted {
                tool_call_id,
                output,
                error,
                ..
            } => {
                if mine {
                    if let Some(pending) = pending.as_mut() {
                        pending.add_result(tool_call_id, output, error);
                    }
                }
                // 另一位发言者的结果正文从不投影。
            }
            EventPayload::UsageRecorded { .. } => {}
            EventPayload::TurnEnded { .. } => {}
            EventPayload::PermissionAsked { .. } => {}
            EventPayload::PermissionDecided { .. } => {}
            EventPayload::HookExecuted { point, outcome, .. } => {
                if mine && point == hook_format::POINT_POST {
                    if let Some(feedback) = hook_format::feedback_text(outcome) {
                        if let Some(pending) = pending.as_mut() {
                            // 钩子的后续反馈是一条追加的事件，但 provider 对每条 `tool_call` 只
                            // 允许一条 `tool` 消息，所以它合并进它所评注的那条结果。
                            // `HookExecuted` 没有 `tool_call_id`，而循环紧接结果之后就发出它，所
                            // 以「最近的那条结果」就是配对关系。
                            pending.add_feedback(feedback);
                        }
                    }
                }
            }
            // 这次派生归到执行者头上（`parent` 指的是派发者），而它在投影上的唯一效果落在执行者自
            // 己身上：简报成为执行者自己的第一条发言。这让执行者的 `messages` 成为事件流的函数，而
            // 不是旁边另传的一个参数（spec §5、§16）。讨论者对此一无所见。
            EventPayload::ExecutorSpawned {
                executor_id,
                parent,
                brief,
            } => {
                if matches!(speaker, SpeakerId::Executor(id) if id == executor_id) {
                    close_pending_if_settled(&mut messages, &mut pending, speaker);
                    flush_others(&mut messages, &mut others, &mut head_emitted);
                    // 由派发者给出名字，所以**不是**无名：钉住的开头是那一串开头处的无名 `user`
                    // 消息，而会话中途的注入（用户加载的技能正文）必须自成一条消息，而不是合并进
                    // 简报（spec §5、§9、§10）。
                    messages.push(Message::User {
                        content: brief.clone(),
                        name: Some(sanitize_name(parent.as_str())),
                        injected: false,
                    });
                }
            }
            // 执行者通过 `task` 调用的工具结果与派发者自己的那个参数来汇报，所以它的过程从不进入
            // 讨论者的投影（spec §5）。
            EventPayload::ExecutorFinished { .. } => {}
            // 模型必须看见并能改正自己的错误（spec §2）；别人的错误不是这个模型该修的。
            EventPayload::AgentError { message, .. } if mine => {
                close_pending_if_settled(&mut messages, &mut pending, speaker);
                flush_others(&mut messages, &mut others, &mut head_emitted);
                messages.push(Message::User {
                    content: message.clone(),
                    name: None,
                    injected: false,
                });
            }
            EventPayload::AgentError { .. } => {}
            // 会话级失败不是模型该修的（spec §2）。
            EventPayload::SessionError { .. } => {}
            // 被取代的那一段上面已经滤掉了；这条记录本身是记账，不是消息。
            EventPayload::HistorySuperseded { .. } => {}
        }
    }

    // 发言者自己的那一组排在任何「等它的工具调用出结果时缓冲下来的发言」之前，所以一条
    // `tool_call` 永远不会与回答它的那条 `tool` 消息分开。
    close_pending_if_settled(&mut messages, &mut pending, speaker);
    flush_others(&mut messages, &mut others, &mut head_emitted);
    messages
}

/// 来自这一位发言者的事件是否进入别人的上下文。
///
/// 只有执行者被藏起来：讨论者能看见另一个讨论者、合成器（`System`）和人。执行者自己的投影是完整
/// 的，所以这里只在事件不属于正在发言的那一位时才被查。
fn speaks_to_others(from: &SpeakerId) -> bool {
    !matches!(from, SpeakerId::Executor(_))
}

/// 到目前为止除了钉住的注入什么都没发过，所以一条新的注入仍属于开头那一块并合并进去。
///
/// 钉住的注入是此刻唯一带着 `injected` 标记的 `user` 消息：发言带着发言者的 `name`，而
/// `AgentError` 不可能排在会话开头的注入之前。空切片意味着这是第一次 push，此时 `last_mut` 找不
/// 到东西，于是 push 而不是合并。
fn at_pinned_head(messages: &[Message]) -> bool {
    messages
        .iter()
        .all(|message| matches!(message, Message::User { injected: true, .. }))
}

/// 加一段别人的发言，并在第一次 `user` 消息处钉住。
fn push_other(
    messages: &mut Vec<Message>,
    others: &mut OtherBlock,
    head_emitted: &mut bool,
    round: Option<u32>,
    speaker: &SpeakerId,
    body: &str,
) {
    if body.is_empty() {
        return;
    }
    let name = speaker_name(speaker);
    if !*head_emitted && !others.is_empty() && !others.has_speaker(&name) {
        // 钉住的开头在换到另一个发言者时结束。
        others.flush_into(messages);
        *head_emitted = true;
    }
    others.push(round, &name, body);
}

fn flush_others(messages: &mut Vec<Message>, others: &mut OtherBlock, head_emitted: &mut bool) {
    if others.flush_into(messages) {
        *head_emitted = true;
    }
}

/// 连续的一串别人发言，合并成一条 `user` 消息。
///
/// 合并只看角色序列、没有条数阈值（spec §5）：目的是避开厂商对连续同角色消息那些没记录的行为，
/// 不是为了把任何东西缩短。
#[derive(Default)]
struct OtherBlock {
    segments: Vec<OtherSegment>,
}

struct OtherSegment {
    speaker: String,
    line: String,
}

impl OtherBlock {
    fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    fn has_speaker(&self, name: &str) -> bool {
        self.segments.iter().any(|segment| segment.speaker == name)
    }

    fn push(&mut self, round: Option<u32>, speaker: &str, body: &str) {
        if body.is_empty() {
            return;
        }
        // 讨论轮次之外没有 `N` 可写，而普通 CLI 会话恰好只有另一位发言者，所以正文保持素净，由
        // `name` 承载归属。
        let line = match round {
            Some(round) => format!("[轮 {round} · {speaker}] {body}"),
            None => body.to_owned(),
        };
        self.segments.push(OtherSegment {
            speaker: speaker.to_owned(),
            line,
        });
    }

    /// 发出合并后的块，并报告是否发出了一块。
    fn flush_into(&mut self, messages: &mut Vec<Message>) -> bool {
        if self.segments.is_empty() {
            return false;
        }
        // `name` 是增强，绝不是归属的保证：正文前缀才是。多位发言者合并成的块拿不到单个名字。
        let mut names: Vec<&str> = Vec::new();
        for segment in &self.segments {
            if !names.contains(&segment.speaker.as_str()) {
                names.push(&segment.speaker);
            }
        }
        // 字段而不是前缀：上面的前缀是逐字写下的，好让同一个模型的两个人物保持区分，而这个字段要
        // 清洗，因为厂商接受的 `name` 字符集没有记录。
        let name = (names.len() == 1).then(|| sanitize_name(names[0]));
        let content = self
            .segments
            .iter()
            .map(|segment| segment.line.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        self.segments.clear();
        messages.push(Message::User {
            content,
            name,
            injected: false,
        });
        true
    }
}

/// 正在发言的那一位进行中的 assistant 消息与它的工具结果。
struct PendingAssistant {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Vec<ToolCall>,
    results: Vec<Message>,
}

impl PendingAssistant {
    fn new(text: &str, reasoning: &Option<String>, caps: &ModelCaps) -> Self {
        Self {
            content: (!text.is_empty()).then(|| text.to_owned()),
            // 模型自己的推理是否必须往返是一个能力表事实，不是按厂商分支（spec §4、§5）。
            reasoning_content: reasoning.clone().filter(|_| caps.requires_reasoning_replay),
            tool_calls: Vec::new(),
            results: Vec::new(),
        }
    }

    /// 是否还有 `tool_call` 没拿到 `tool` 消息。已了结的一组可以发出；没了结的必须继续敞开，好
    /// 让它的配对在交错中存活（合法的回合从不产生交错）。
    fn awaiting_results(&self) -> bool {
        self.results.len() < self.tool_calls.len()
    }

    /// 这一组是否会发出一条什么都没有的 assistant 消息。
    ///
    /// 只有被取代的那一段才可能产生它：`/undo` 取代了某个编辑的工具调用，于是一个不携带任何文本的
    /// 回合就既没有内容也没有调用。线上没有这条消息的形状，所以一条都不发。
    fn is_empty(&self) -> bool {
        self.content.is_none() && self.reasoning_content.is_none() && self.tool_calls.is_empty()
    }

    fn add_call(&mut self, tool_call_id: &ToolCallId, tool_name: &str, args: &serde_json::Value) {
        self.tool_calls.push(ToolCall {
            id: tool_call_id.as_str().to_owned(),
            name: tool_name.to_owned(),
            arguments: args.to_string(),
        });
    }

    /// 一条 `tool_call` 恰好拿到一条 `tool` 消息；它的正文已经合并了结果与任何钩子的后续反馈。
    fn add_result(
        &mut self,
        tool_call_id: &ToolCallId,
        output: &Option<String>,
        error: &Option<String>,
    ) {
        let content = output.clone().or_else(|| error.clone()).unwrap_or_default();
        self.results.push(Message::Tool {
            tool_call_id: tool_call_id.as_str().to_owned(),
            content,
        });
    }

    /// 把反馈合并进它所评注的那条结果。`failed:` 的结局到不了这里，这让「钩子失败只丢反馈」在模型
    /// 那一侧也成立。
    fn add_feedback(&mut self, feedback: &str) {
        if let Some(Message::Tool { content, .. }) = self.results.last_mut() {
            content.push_str("\n\n");
            content.push_str(hook_format::FEEDBACK_MARKER);
            content.push(' ');
            content.push_str(feedback);
        }
    }
}

/// 发出正在发言那一位的 assistant 组，除非它的 `tool_call` 仍在等一个更晚的事件可能带来的结果。
fn close_pending_if_settled(
    messages: &mut Vec<Message>,
    pending: &mut Option<PendingAssistant>,
    speaker: &SpeakerId,
) {
    if pending
        .as_ref()
        .is_some_and(PendingAssistant::awaiting_results)
    {
        return;
    }
    let Some(pending) = pending.take() else {
        return;
    };
    // 被取代的工具调用可能把这一组掏空；一条没有内容、没有推理、没有调用的 assistant 消息不是消
    // 息。
    if pending.is_empty() {
        return;
    }
    messages.push(Message::Assistant {
        content: pending.content,
        reasoning_content: pending.reasoning_content,
        tool_calls: pending.tool_calls,
        name: Some(wire_name(speaker)),
    });
    messages.extend(pending.results);
}

/// 一位发言者**被称**的名字，逐字：正文前缀写的就是它，也是对方必须能分辨的东西。
///
/// 讨论者是一个**人物**：它的名字是选出来的（`[discussion] debaters = [{ name = "保守", … }]`），
/// 所以可以是用户喜欢的任何词 —— `保守` 是一个读者和模型都能用的名字，把它改造会让同一场讨论的
/// 两方分不清。执行者的身份是 `executor-<id>`，即展示形式本来就有的形状。
///
/// **线上 `name` 字段**是另一个问题：[`wire_name`] 把它清洗成 `[A-Za-z0-9_-]`，因为没有厂商记下
/// 它的字符集。正文前缀承载同样的信息，所以一个忽略或截断那个字段的 provider 什么也不损失
/// （spec §5）。
fn speaker_name(speaker: &SpeakerId) -> String {
    match speaker {
        SpeakerId::Executor(id) => format!("executor-{id}"),
        other => other.to_string(),
    }
}

/// 同一个名字，取 `name` 字段允许的形状。
fn wire_name(speaker: &SpeakerId) -> String {
    sanitize_name(&speaker_name(speaker))
}

fn sanitize_name(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .take(MAX_NAME_CHARS)
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "unknown".to_owned()
    } else {
        sanitized
    }
}

/// 另一位发言者工具调用的一行：工具名加上截断的参数渲染，让「他们到底查了什么」仍然答得上来，而
/// 不必为结果正文付钱（spec §5）。
fn tool_summary(tool_name: &str, args: &serde_json::Value) -> String {
    let rendered = match args {
        serde_json::Value::Null => String::new(),
        other => truncate(&other.to_string(), MAX_TOOL_SUMMARY_CHARS),
    };
    if rendered.is_empty() || rendered == "{}" {
        format!("→ {tool_name}")
    } else {
        format!("→ {tool_name}({rendered})")
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut truncated: String = text.chars().take(max_chars).collect();
    truncated.push('…');
    truncated
}
