//! Context boundary: usable-input accounting, single-result truncation, and
//! budget-driven dropping (spec §10).
//!
//! Two mechanisms, at two different moments, over two different sets of data:
//!
//! * **Single-result truncation** happens **before an event is appended**: an
//!   oversized tool result is spilled to disk and the stream carries a preview
//!   plus a pointer. The pointer is an enhancement — a missing spill file
//!   degrades to the inline preview and never fails.
//! * **Over-budget dropping** happens in [`trim`], *after* projection, and is
//!   **read-only**: the log is never touched, only the `messages` sent to the
//!   provider. The drop order is fixed (spec §10): old ordinary tool results →
//!   old skill bodies → old whole rounds → this turn hard-fails.
//!
//! Both are pure functions of values, so "an agent's `messages` is recomputable
//! from the stream + the spill files + the projection rules" holds, and the
//! window layer needs no lock: the current budget is a value, not state.
//!
//! Dropping a tool result **stubs its body** rather than removing the message:
//! a provider requires exactly one `tool` message per `tool_call` (spec §5), so
//! the paired `tool` message must survive with a shorter body. Only whole-round
//! dropping removes messages, and it removes a round's assistant message and its
//! results together, which keeps the pairing intact.
//!
//! [`skills`] and [`repo_map`] are the sibling concerns (spec §9). Skills keep
//! the description catalog and load bodies on demand; the aggregate cap on
//! loaded skill bodies is enforced here, before the window budget is even
//! consulted. `repo_map` extracts and ranks workspace symbols for the on-demand
//! `repo_map` tool, and its product is a tool result like any other.

pub mod repo_map;
pub mod skills;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::provider::capability::ModelCaps;
use crate::provider::Message;
use skills::MAX_LOADED_SKILL_TOKENS;

/// Input space reserved for the model's own output (spec §10).
///
/// The reserve is `min(20_000, max_output_tokens)`, so a model whose output cap
/// is below 20k reserves only what it can actually write.
pub const OUTPUT_RESERVE_TOKENS: u32 = 20_000;

/// v1's crude estimator: one token per four characters (spec §10).
const CHARS_PER_TOKEN: usize = 4;

/// How much smaller than the per-result cap the inline preview is.
const PREVIEW_DIVISOR: u64 = 10;

/// A preview never shrinks below this, so a tiny cap cannot hide the shape of a
/// result.
const MIN_PREVIEW_CHARS: usize = 200;

/// The body a dropped tool result is replaced with.
///
/// The message itself stays, because the wire contract pairs one `tool` message
/// with one `tool_call` (spec §5); only its body is discarded.
pub const DROPPED_TOOL_RESULT: &str =
    "[dropped: this old tool result was removed from the context to fit the budget]";

/// The project rules file, read once at startup and injected as the first
/// `user` message (spec §10).
pub const AGENTS_MD: &str = "AGENTS.md";

/// The usable input budget for one agent, computed from **its own** model
/// (spec §10). There is deliberately no session-wide budget.
///
/// Saturating on purpose: a capability table with a tiny window still has a
/// well-defined budget rather than panicking under a subtraction.
pub fn usable_input(caps: &ModelCaps) -> u64 {
    let reserve = caps.max_output_tokens.min(OUTPUT_RESERVE_TOKENS);
    u64::from(caps.context_window.saturating_sub(reserve))
}

/// The crude v1 estimate for one string: characters / 4, rounded up, so any
/// non-empty text costs at least one token.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(CHARS_PER_TOKEN as u64)
}

/// The estimated size of a projected request.
pub fn estimate_messages_tokens(messages: &[Message]) -> u64 {
    messages.iter().map(estimate_message_tokens).sum()
}

fn estimate_message_tokens(message: &Message) -> u64 {
    match message {
        Message::System { content, .. } | Message::User { content, .. } => estimate_tokens(content),
        Message::Assistant {
            content,
            reasoning_content,
            tool_calls,
            ..
        } => {
            option_tokens(content.as_deref())
                + option_tokens(reasoning_content.as_deref())
                + tool_calls
                    .iter()
                    .map(|call| estimate_tokens(&call.name) + estimate_tokens(&call.arguments))
                    .sum::<u64>()
        }
        Message::Tool { content, .. } => estimate_tokens(content),
    }
}

fn option_tokens(text: Option<&str>) -> u64 {
    text.map(estimate_tokens).unwrap_or(0)
}

/// The drop policy [`trim`] reads. It is a value, not state.
#[derive(Debug, Clone)]
pub struct TrimPolicy {
    /// Tool names whose result bodies are stickier than ordinary tool results
    /// (spec §10: a loaded skill body belongs to the model's current work).
    /// Ticket 08 mounts the `skill` tool this names.
    pub sticky_tool_names: Vec<String>,
    /// Aggregate cap on the loaded skill bodies in one request (spec §9).
    ///
    /// Independent of the window: once the total exceeds it the oldest bodies are
    /// stubbed, whatever the window budget says. Ordinary tool results are never
    /// touched by this pre-pass.
    pub loaded_skill_budget: u64,
}

impl Default for TrimPolicy {
    fn default() -> Self {
        Self {
            sticky_tool_names: vec![skills::SKILL_TOOL.to_owned()],
            loaded_skill_budget: MAX_LOADED_SKILL_TOKENS,
        }
    }
}

/// Why a trim could not fit its budget.
///
/// This is the "real overflow" signal (spec §10): every droppable class has
/// been exhausted, which is also the trigger a future compaction would hang
/// from.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TrimError {
    #[error(
        "context budget exceeded: ~{estimated} estimated tokens against a budget of {budget}, \
         and nothing droppable is left"
    )]
    OverBudget { budget: u64, estimated: u64 },
}

/// Fit a projected request into `budget` tokens, dropping in the fixed order
/// (spec §10): old ordinary tool results, then old skill bodies, then old whole
/// rounds. Still over budget after that is a hard failure for the turn.
///
/// Before any of that, the aggregate loaded-skill budget is enforced (spec §9):
/// the oldest skill bodies are stubbed once their total exceeds
/// [`TrimPolicy::loaded_skill_budget`], even when the window budget is generous.
///
/// Read-only with respect to the event log: this only rewrites the `messages`
/// value it is handed. The pinned injection at the head and the turn currently
/// being assembled are never dropped.
pub fn trim(
    mut messages: Vec<Message>,
    budget: u64,
    policy: &TrimPolicy,
) -> Result<Vec<Message>, TrimError> {
    // Names are needed by both the skill-body budget and the window drop order.
    let names = tool_names(&messages);
    stub_skill_bodies_over_budget(&mut messages, &names, policy);

    if fits(&messages, budget) {
        return Ok(messages);
    }

    let pinned = pinned_len(&messages);

    // Classes 1 and 2: old tool-result bodies, oldest first, ordinary results
    // before skill bodies. The same shrink, applied to one stickiness class at a
    // time, which is what makes the order strict.
    for sticky in [false, true] {
        for index in old_result_indices(&messages, &names, policy, sticky) {
            if fits(&messages, budget) {
                return Ok(messages);
            }
            stub_tool_result(&mut messages[index]);
        }
    }

    // Class 3: old whole rounds, oldest first. The active round and the pinned
    // injection stay: the model must still have the question it is answering.
    while !fits(&messages, budget) {
        let starts = round_starts(&messages, pinned);
        if starts.len() <= 1 {
            break;
        }
        messages.drain(starts[0]..starts[1]);
    }

    if fits(&messages, budget) {
        Ok(messages)
    } else {
        Err(TrimError::OverBudget {
            budget,
            estimated: estimate_messages_tokens(&messages),
        })
    }
}

fn fits(messages: &[Message], budget: u64) -> bool {
    estimate_messages_tokens(messages) <= budget
}

/// The pinned head: the agent's private identity, then the projected
/// `ContextInjected` events, which is why they are `user` messages with no
/// `name`, always first (spec §5, §10, §15). They never take part in trimming.
///
/// Counting the identity here is also what keeps [`round_starts`] from mistaking
/// the pinned injection for a droppable round.
///
/// A later mid-session injection (plan mode, ticket 15) is not in this leading
/// prefix; that ticket must extend the pin marker when it lands.
fn pinned_len(messages: &[Message]) -> usize {
    messages
        .iter()
        .take_while(|message| {
            matches!(
                message,
                Message::System { .. } | Message::User { name: None, .. }
            )
        })
        .count()
}

/// Indices where a round starts: every non-pinned `user` message. A round runs
/// from its start to the next start (or the end), so dropping a whole round
/// takes its assistant turns and their tool results with it.
fn round_starts(messages: &[Message], pinned: usize) -> Vec<usize> {
    (pinned..messages.len())
        .filter(|&index| matches!(messages[index], Message::User { .. }))
        .collect()
}

/// `tool_call_id -> tool name` for every call the projection emitted, used to
/// tell an ordinary result from a skill body. Owned so trimming can mutate the
/// messages while consulting the map.
fn tool_names(messages: &[Message]) -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    for message in messages {
        if let Message::Assistant { tool_calls, .. } = message {
            for call in tool_calls {
                names.insert(call.id.clone(), call.name.clone());
            }
        }
    }
    names
}

/// Live (not-yet-stubbed) tool results outside the pinned head, oldest first,
/// stopping before `limit`.
///
/// Both drop mechanisms scan the same shape; `limit` is what distinguishes them:
/// the window order stops at the active round (the model keeps the question it
/// is answering), while the aggregate skill budget scans the whole request.
fn live_tool_indices(messages: &[Message], limit: usize) -> Vec<usize> {
    (pinned_len(messages)..limit.min(messages.len()))
        .filter(|&index| {
            matches!(&messages[index], Message::Tool { content, .. } if content.as_str() != DROPPED_TOOL_RESULT)
        })
        .collect()
}

/// Where the active (last) round starts, or the end when there is no round to
/// distinguish.
fn active_round_start(messages: &[Message]) -> usize {
    let starts = round_starts(messages, pinned_len(messages));
    starts.last().copied().unwrap_or(messages.len())
}

/// The old (non-active-round) tool results of one class, oldest first.
fn old_result_indices(
    messages: &[Message],
    names: &BTreeMap<String, String>,
    policy: &TrimPolicy,
    sticky: bool,
) -> Vec<usize> {
    live_tool_indices(messages, active_round_start(messages))
        .into_iter()
        .filter(|&index| is_sticky_result(&messages[index], names, policy) == sticky)
        .collect()
}

/// Whether a tool result belongs to the sticky class: its call was made by a tool
/// named in [`TrimPolicy::sticky_tool_names`] (a loaded skill body, by default).
/// Drives both the aggregate skill budget and the window drop order.
fn is_sticky_result(
    message: &Message,
    names: &BTreeMap<String, String>,
    policy: &TrimPolicy,
) -> bool {
    match message {
        Message::Tool { tool_call_id, .. } => names
            .get(tool_call_id.as_str())
            .is_some_and(|name| policy.sticky_tool_names.iter().any(|tool| tool == name)),
        _ => false,
    }
}

/// Enforce the aggregate loaded-skill budget (spec §9) independently of the
/// window: while the live skill bodies total more than the policy allows, stub
/// the oldest.
///
/// This is a cap on the whole request, the active turn included: a round that
/// loads more than the budget itself loses its oldest body (and may load it
/// again). It is a separate budget from the window, so it does not wait for the
/// window to overflow.
fn stub_skill_bodies_over_budget(
    messages: &mut [Message],
    names: &BTreeMap<String, String>,
    policy: &TrimPolicy,
) {
    let candidates: Vec<usize> = live_tool_indices(messages, messages.len())
        .into_iter()
        .filter(|&index| is_sticky_result(&messages[index], names, policy))
        .collect();

    let mut total: u64 = candidates
        .iter()
        .map(|&index| estimate_message_tokens(&messages[index]))
        .sum();
    for index in candidates {
        if total <= policy.loaded_skill_budget {
            break;
        }
        total -= estimate_message_tokens(&messages[index]);
        stub_tool_result(&mut messages[index]);
    }
}

fn stub_tool_result(message: &mut Message) {
    if let Message::Tool { content, .. } = message {
        *content = DROPPED_TOOL_RESULT.to_owned();
    }
}

/// A tool result after the pre-stream truncation pipeline (spec §10): the text
/// that goes into the event (a preview plus a pointer) and where the full text
/// landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpilledResult {
    /// What the stream carries — always complete on its own.
    pub preview: String,
    /// The spill file, when it could be written.
    pub pointer: Option<PathBuf>,
    /// Whether the text was over the cap.
    pub truncated: bool,
}

/// Truncate one tool result before it enters the stream: spill the overflow to
/// `<outputs_dir>/<tool_call_id>.txt` and return a head/tail preview carrying
/// the pointer.
///
/// Never fails, and never grows the stream: a body so small that a preview plus
/// the pointer note would be longer than the body itself is left whole (it is
/// already too small to blow a window). A spill that cannot be written degrades
/// to "preview only" (spec §11: a dead pointer degrades to the preview).
pub fn truncate_result(
    text: &str,
    tool_call_id: &str,
    outputs_dir: &Path,
    max_tokens: u64,
) -> SpilledResult {
    let total_chars = text.chars().count();
    if estimate_tokens(text) <= max_tokens {
        return SpilledResult {
            preview: text.to_owned(),
            pointer: None,
            truncated: false,
        };
    }

    let pointer = outputs_dir.join(format!("{tool_call_id}.txt"));
    let spilled = std::fs::create_dir_all(outputs_dir)
        .and_then(|()| crate::tools::paths::write_owner_only(&pointer, text.as_bytes()))
        .is_ok();
    let pointer = spilled.then_some(pointer);
    let preview = preview(text, max_tokens, pointer.as_deref());
    if preview.chars().count() >= total_chars {
        // The pointer note costs more than the body saves: keeping the body is
        // strictly better, and the cap exists to bound the stream, not to
        // enforce a number.
        return SpilledResult {
            preview: text.to_owned(),
            pointer: None,
            truncated: false,
        };
    }
    SpilledResult {
        preview,
        pointer,
        truncated: true,
    }
}

fn preview(text: &str, max_tokens: u64, pointer: Option<&Path>) -> String {
    let total_chars = text.chars().count();
    let total_tokens = estimate_tokens(text);
    let wanted = ((max_tokens / PREVIEW_DIVISOR) as usize)
        .saturating_mul(CHARS_PER_TOKEN)
        .max(MIN_PREVIEW_CHARS);
    // Keep head and tail from both covering the same characters.
    let preview_chars = wanted.min(total_chars.saturating_sub(1));
    let head_chars = preview_chars / 2;
    let tail_chars = preview_chars - head_chars;

    let head: String = text.chars().take(head_chars).collect();
    let mut tail: Vec<char> = text.chars().rev().take(tail_chars).collect();
    tail.reverse();
    let tail: String = tail.into_iter().collect();

    let note = match pointer {
        Some(path) => format!("full output at {}", path.display()),
        None => "full output could not be spilled to disk".to_owned(),
    };
    format!("{head}\n[truncated: {total_chars} chars, ~{total_tokens} tokens; {note}]\n{tail}")
}

/// Read the project's `AGENTS.md`, if it exists and is not blank.
///
/// Absence is not an error: a repository without project rules simply has no
/// pinned injection. Unreadable files behave the same way rather than stopping
/// the session before it starts.
pub fn load_agents_md(cwd: &Path) -> Option<String> {
    let text = std::fs::read_to_string(cwd.join(AGENTS_MD)).ok()?;
    (!text.trim().is_empty()).then_some(text)
}
