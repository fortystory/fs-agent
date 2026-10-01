//! 共享的呈现层：事件只变成 [`Block`] 一次，plain 与 TUI 各按自己的方式画这些块。
//!
//! 两条规矩让它不止是一趟格式化：
//!
//! * **一次工具调用与它的结果是一个块，由结果绘制。** 后置 hook 的反馈不带
//!   `tool_call_id` —— 循环在它批注的那条结果之后紧接着发出它 —— 所以它作为自己的一个
//!   小块旅行，瞄准刚画好的那次调用。曾经改成把调用一直开着去合并它，结果是这次调用在
//!   它整个运行期间都不可见：TUI 要等到某个后来的事件到达才画它 —— 下一个回答的第一个
//!   流式增量，或者回合结束（票 02 §3）。
//! * **增量文本原样透传。** 增量绕过事件流，所以事后无法重新推出来；转录把它们转发
//!   过去，好让渲染器在它们到达时就画。

use serde_json::Value;

use crate::events::{
    hook_format, ContextSource, Decision, DecisionSource, Event, EventPayload, HistoryReason,
    ParticipantId, Role, RoundMode, SpeakerId, StopReason, ToolCallId, Usage,
};

use super::{DeltaKind, RenderEvent};

/// 转录里一个可直接显示的单元。
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// 增量模型输出，在去渲染器的路上。
    Delta {
        speaker: SpeakerId,
        kind: DeltaKind,
        text: String,
    },
    /// 一条完成的消息。它的正文已经作为增量流过去了；渲染器用它来定消息边界，而不是重印
    /// 正文。
    Message {
        speaker: SpeakerId,
        role: Role,
        text: String,
        /// 写完的推理 trace，供应商送了的话。整条 trace 只在这里存在 —— 增量是增量的，
        /// 而事件流没有单独的推理事件 —— 所以转录那条「思考结束」的行就是把它留给详情
        /// 视图的（票 02 §1）。
        reasoning: Option<String>,
    },
    RoundStarted {
        round: u32,
        mode: RoundMode,
    },
    RoundEnded {
        round: u32,
        reason: StopReason,
    },
    Divergence {
        topic: String,
        positions: Vec<String>,
    },
    /// 一次工具调用与它的结果：一个块，结果一落地就发出来。
    Tool(Box<ToolBlock>),
    /// 后置 hook 对它批注的那次调用的反馈。
    ///
    /// 它作为自己的一个块旅行，因为它批注的那次调用已经被画出来了：循环在结果之后紧接
    /// 着发出反馈，而把调用开着等它，正是以前让调用行整个运行期间都藏着的原因
    /// （票 02 §3）。
    ToolFeedback {
        outcome: String,
    },
    TurnStarted {
        speaker: SpeakerId,
        iteration: u32,
    },
    TurnEnded {
        speaker: SpeakerId,
        reason: StopReason,
    },
    PermissionAsked {
        speaker: SpeakerId,
        /// 这个问题所问的那个工具，流上记了的话。
        tool_name: Option<String>,
        /// 它正在问的那次调用的参数，好让画家显示人正在批准的命令或路径。
        args: Value,
    },
    PermissionDecided {
        speaker: SpeakerId,
        decision: Decision,
        source: DecisionSource,
        reason: Option<String>,
    },
    /// 一个前置 hook 的结果。后置 hook 的反馈搭在它的 [`ToolBlock`] 上。
    Hook {
        speaker: SpeakerId,
        point: String,
        outcome: String,
    },
    ExecutorSpawned {
        speaker: SpeakerId,
        executor_id: ParticipantId,
    },
    ExecutorFinished {
        executor_id: ParticipantId,
        reason: StopReason,
        summary: String,
    },
    Usage {
        speaker: SpeakerId,
        usage: Usage,
    },
    AgentError {
        speaker: SpeakerId,
        message: String,
    },
    SessionError {
        code: String,
        detail: String,
    },
    SessionEnded {
        reason: StopReason,
    },
    ContextInjected {
        source: ContextSource,
    },
    /// 这条流这一刻的沙箱状态（沙箱 spec §8）。
    ///
    /// 与上下文注入同一档：一行叙述，不带发言者、也不进模型上下文。`sessions show` 里也有
    /// 它 —— 两处说的是同一件事。
    Sandbox {
        /// 协议标记：`"bwrap"` / `"off"`。
        mode: String,
        unavailable_reason: Option<String>,
    },
    History {
        reason: HistoryReason,
        summary: Option<String>,
    },
    Diagnostic(String),
    /// 一行不为任何发言者说话、也不叙述任何事件，原样显示。
    Notice(String),
}

/// 一次工具调用，一直开着，直到它的结果（以及任何后置 hook 的反馈）到达。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolBlock {
    pub speaker: SpeakerId,
    pub tool_call_id: ToolCallId,
    pub tool: String,
    pub args: Value,
    pub outcome: Option<ToolOutcome>,
}

/// 一次完成（或被放弃）的工具调用产出了什么。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutcome {
    pub ok: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// 那个增量的「事件转块」状态机。
#[derive(Debug, Default)]
pub struct Transcript {
    pending_tool: Option<ToolBlock>,
    /// 一次调用已经画出来了，而它的后置 hook —— 那个不带 `tool_call_id` 的 —— 可能还在
    /// 路上。下一个到达的后置 hook 批注的就是那次调用。
    awaiting_hook: bool,
}

impl Transcript {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂一个渲染事件，取走它产出的块。
    ///
    /// 一次工具调用在它的**结果**到达时立刻被画出来。它曾经要等下一个无关事件，好把后置
    /// hook 并进同一个块，那意味着一次调用在整个运行期间都不可见 —— 而且在 TUI 里，要等
    /// 模型*下一个*回答流完（票 02 §3）。
    pub fn push(&mut self, event: RenderEvent) -> Vec<Block> {
        match event {
            RenderEvent::Delta {
                speaker,
                kind,
                text,
            } => {
                let mut blocks = self.flush();
                blocks.push(Block::Delta {
                    speaker,
                    kind,
                    text,
                });
                blocks
            }
            RenderEvent::Diagnostic(message) => {
                let mut blocks = self.flush();
                blocks.push(Block::Diagnostic(message));
                blocks
            }
            RenderEvent::Notice(message) => {
                let mut blocks = self.flush();
                blocks.push(Block::Notice(message));
                blocks
            }
            RenderEvent::Logged(event) => self.push_logged(event),
        }
    }

    /// 关上一个敞着的工具块，如果有的话。
    ///
    /// 画一次调用的是结果，所以这里只是兜底：一次结果始终没到的调用 —— 一条在调用中途
    /// 死掉的流。[`Plain`] 在流的末尾调它，所以那一行仍然到得了页面上。TUI 显示不了它：
    /// 流关上之后它没有帧了（一次被取消的调用*不是*这种情况 —— 循环为它开始的每一次调用
    /// 都写一条合成结果，而那条结果会画出这次调用）。
    ///
    /// [`Plain`]: crate::render::Plain
    pub fn flush(&mut self) -> Vec<Block> {
        match self.pending_tool.take() {
            Some(tool) => vec![Block::Tool(Box::new(tool))],
            None => Vec::new(),
        }
    }

    fn push_logged(&mut self, event: Event) -> Vec<Block> {
        let speaker = event.speaker_id.clone();
        // 这份期待正好只活一个事件：循环在它批注的那条结果之后紧接着发出一次调用的后置
        // hook，所以先到的任何别的东西都意味着 hook 不来了 —— 而一个此后很久才冒出来的
        // hook 绝不能钉到一次它从没批注过的调用上。只有下面那条匹配结果的臂会重新武装它。
        let expected_hook = std::mem::take(&mut self.awaiting_hook);
        if let EventPayload::HookExecuted { point, outcome, .. } = &event.payload {
            if point == hook_format::POINT_POST {
                let mut blocks = self.flush();
                if expected_hook || !blocks.is_empty() {
                    blocks.push(Block::ToolFeedback {
                        outcome: outcome.clone(),
                    });
                    return blocks;
                }
            }
        }

        // 这些事件属于一次调用开始与它结果之间的那段间隔：它们被叙述，但绝不能关掉那个
        // 敞着的块。`PermissionAsked`/`PermissionDecided` 是承重的那一例 —— 循环为
        // **每一次**调用都记一条裁决，问没问都记 —— 而前置 hook 也在 `ToolCallStarted`
        // 之后触发，所以把其中任何一个当成「无关」都会把每一次工具调用劈成两半。
        let inside_a_call = matches!(
            &event.payload,
            EventPayload::ToolCallCompleted { .. }
                | EventPayload::PermissionAsked { .. }
                | EventPayload::PermissionDecided { .. }
                | EventPayload::HookExecuted { .. }
        );
        let mut blocks = if inside_a_call {
            Vec::new()
        } else {
            self.flush()
        };
        match event.payload {
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } => {
                // 一次新调用顶掉上一条仍在期待中的反馈。
                self.awaiting_hook = false;
                self.pending_tool = Some(ToolBlock {
                    speaker,
                    tool_call_id,
                    tool: tool_name,
                    args,
                    outcome: None,
                });
                return blocks;
            }
            EventPayload::ToolCallCompleted {
                tool_call_id,
                ok,
                output,
                error,
                duration_ms,
            } => {
                if self
                    .pending_tool
                    .as_ref()
                    .is_some_and(|tool| tool.tool_call_id == tool_call_id)
                {
                    let mut tool = self.pending_tool.take().expect("刚匹配上");
                    tool.outcome = Some(ToolOutcome {
                        ok,
                        output,
                        error,
                        duration_ms,
                    });
                    // 这次调用仍可能被随后的后置 hook 批注。
                    self.awaiting_hook = true;
                    blocks.push(Block::Tool(Box::new(tool)));
                    return blocks;
                }
                // 一条没有匹配开始的结果：把它浮出来，而不是丢掉，好让转录仍然显示有东西
                // 完成了。
                let mut blocks = self.flush();
                blocks.push(Block::Tool(Box::new(ToolBlock {
                    speaker,
                    tool_call_id,
                    tool: "?".to_owned(),
                    args: Value::Null,
                    outcome: Some(ToolOutcome {
                        ok,
                        output,
                        error,
                        duration_ms,
                    }),
                })));
                return blocks;
            }
            EventPayload::MessageCompleted {
                role,
                text,
                reasoning,
            } => {
                blocks.push(Block::Message {
                    speaker,
                    role,
                    text,
                    reasoning,
                });
            }
            EventPayload::RoundStarted { round, mode } => {
                blocks.push(Block::RoundStarted { round, mode });
            }
            EventPayload::RoundEnded { round, reason } => {
                blocks.push(Block::RoundEnded { round, reason });
            }
            EventPayload::DivergenceRecorded {
                topic, positions, ..
            } => {
                blocks.push(Block::Divergence { topic, positions });
            }
            EventPayload::TurnStarted { iteration, .. } => {
                blocks.push(Block::TurnStarted { speaker, iteration });
            }
            EventPayload::TurnEnded { reason } => {
                blocks.push(Block::TurnEnded { speaker, reason });
            }
            EventPayload::PermissionAsked { request, .. } => {
                blocks.push(Block::PermissionAsked {
                    speaker,
                    tool_name: crate::events::permission_format::tool_name(&request)
                        .map(str::to_owned),
                    args: crate::events::permission_format::args(&request)
                        .cloned()
                        .unwrap_or(Value::Null),
                });
            }
            EventPayload::PermissionDecided {
                decision,
                source,
                reason,
                ..
            } => {
                blocks.push(Block::PermissionDecided {
                    speaker,
                    decision,
                    source,
                    reason,
                });
            }
            EventPayload::HookExecuted { point, outcome, .. } => {
                blocks.push(Block::Hook {
                    speaker,
                    point,
                    outcome,
                });
            }
            EventPayload::ExecutorSpawned { executor_id, .. } => {
                blocks.push(Block::ExecutorSpawned {
                    speaker,
                    executor_id,
                });
            }
            EventPayload::ExecutorFinished {
                executor_id,
                reason,
                summary,
            } => {
                blocks.push(Block::ExecutorFinished {
                    executor_id,
                    reason,
                    summary,
                });
            }
            EventPayload::UsageRecorded { usage } => {
                blocks.push(Block::Usage { speaker, usage });
            }
            EventPayload::AgentError { message, .. } => {
                blocks.push(Block::AgentError { speaker, message });
            }
            EventPayload::SessionError { code, detail } => {
                blocks.push(Block::SessionError { code, detail });
            }
            EventPayload::SessionEnded { reason } => {
                blocks.push(Block::SessionEnded { reason });
            }
            EventPayload::ContextInjected { source, .. } => {
                blocks.push(Block::ContextInjected { source });
            }
            EventPayload::HistorySuperseded {
                reason, summary, ..
            } => {
                blocks.push(Block::History { reason, summary });
            }
            // 会话骨架不是一个人会实时读的叙述；沙箱状态按上下文注入那一档画一行。
            EventPayload::SessionStarted { .. } => {}
            // 归属也是**只进日志**的记账：给人看的那一句由循环发（「开始做目标 <名字>」），而这
            // 条事件说的是这个会话在为谁干活。清单本身以一次上下文注入到达，那一行由
            // `ContextInjected` 画出来。
            EventPayload::GoalSelected { .. } => {}
            // 收尾汇总已经作为一条助手消息画出来了（写它的那次单发调用记的），这里不再重复。
            EventPayload::GoalCompleted { .. } => {}
            // 停下并报告的那一行由循环发（`notice`），所以这里不再画一遍。
            EventPayload::GoalStopped { .. } => {}
            EventPayload::SandboxStatus {
                mode,
                unavailable_reason,
            } => {
                blocks.push(Block::Sandbox {
                    mode,
                    unavailable_reason,
                });
            }
        }
        blocks
    }
}

/// 一次工具调用参数的一行摘要。
///
/// 值紧凑地渲染，整体有上限，所以一次带大体积正文的调用仍然读作一行 —— 这也是 `docs`
/// 把转录说成一份日志而不是一个调试器的原因。
pub fn summarize_args(args: &Value) -> String {
    const MAX: usize = 160;
    let rendered = match args {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| format!("{key}={}", summarize_value(value)))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Null => String::new(),
        other => summarize_value(other),
    };
    truncate(&rendered, MAX)
}

fn summarize_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.replace('\n', "\\n"),
        other => other.to_string(),
    }
}

/// 一个权限问题会跑什么的一行摘要，取自某个事件的 `request` 值。流上没记参数时是空的。
pub fn summarize_permission_target(request: &Value) -> String {
    match crate::events::permission_format::args(request) {
        Some(args) => summarize_args(args),
        None => String::new(),
    }
}

/// 裁到 `max` 个字符，并标记出裁过。
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}
