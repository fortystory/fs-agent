//! Projection: `(EventLog, SpeakerId) -> messages`.
//!
//! A pure function of the stream plus attribution rules. It holds no trimming
//! state; trimming is a separate pure step that lands in ticket 07.
//!
//! Ticket 01 covers the single-agent case: the acting speaker's own messages
//! become `assistant`, everyone else's become `user`, and the speaker's tool
//! round-trip is replayed. Other-speaker attribution (a one-line tool summary,
//! merged PostToolUse feedback) and capability-driven field differences land in
//! ticket 06.

use super::{Message, ToolCall};
use crate::events::{Event, EventPayload, SpeakerId};

/// Recompute the `messages` an agent should replay from the event stream.
pub fn project(events: &[Event], speaker: &SpeakerId) -> Vec<Message> {
    let mut messages = Vec::new();
    let mut pending: Option<PendingAssistant> = None;

    for event in events {
        let is_mine = &event.speaker_id == speaker;
        match &event.payload {
            EventPayload::MessageCompleted {
                text, reasoning, ..
            } => {
                finalize_assistant(&mut messages, &mut pending);
                if is_mine {
                    pending = Some(PendingAssistant {
                        content: (!text.is_empty()).then(|| text.clone()),
                        reasoning_content: reasoning.clone(),
                        tool_calls: Vec::new(),
                        results: Vec::new(),
                    });
                } else {
                    messages.push(Message::User {
                        content: text.clone(),
                        name: None,
                    });
                }
            }
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } if is_mine => {
                if let Some(pending) = pending.as_mut() {
                    pending.tool_calls.push(ToolCall {
                        id: tool_call_id.as_str().to_owned(),
                        name: tool_name.clone(),
                        arguments: args.to_string(),
                    });
                }
            }
            EventPayload::ToolCallCompleted {
                tool_call_id,
                output,
                error,
                ..
            } if is_mine => {
                if let Some(pending) = pending.as_mut() {
                    let content = output.clone().or_else(|| error.clone()).unwrap_or_default();
                    pending.results.push(Message::Tool {
                        tool_call_id: tool_call_id.as_str().to_owned(),
                        content,
                    });
                }
            }
            _ => {}
        }
    }

    finalize_assistant(&mut messages, &mut pending);
    messages
}

struct PendingAssistant {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Vec<ToolCall>,
    results: Vec<Message>,
}

fn finalize_assistant(messages: &mut Vec<Message>, pending: &mut Option<PendingAssistant>) {
    let Some(pending) = pending.take() else {
        return;
    };
    messages.push(Message::Assistant {
        content: pending.content,
        reasoning_content: pending.reasoning_content,
        tool_calls: pending.tool_calls,
        name: None,
    });
    messages.extend(pending.results);
}
