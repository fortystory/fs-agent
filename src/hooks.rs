//! Hook boundary (spec §3, user stories I).
//!
//! A hook is a user *strategy* mounted at one of two points around a tool call.
//! The loop owns the control flow and applies both transformations in a fixed
//! order:
//!
//! ```text
//! hook.pre -> permission gate -> [ask] -> dispatch -> hook.post -> append
//! ```
//!
//! One algebraic fact is the reason this module exists: **a pre-hook can only
//! tighten**. Its output is a [`Constraint`] —
//! `Continue | Rewrite(args) | Tighten(Ask|Deny) | Skip | Stop` — and the only
//! verdict-shaped variant carries a [`Tightening`], which has no `Allow` case. So
//! "hooks cannot loosen permissions" is a property of the type rather than a
//! runtime check, and the effective verdict is the supremum of the constraint and
//! the gate's verdict on `Allow < Ask < Deny`. There is no `PermissionRequest`
//! mount point: `hook.pre` runs before the gate, so it can stop an ask from ever
//! happening but can never bypass one.
//!
//! Failure is asymmetric (spec §3). A `hook.pre` failure or timeout is
//! **fail-closed**: the action is blocked, the failure is diagnosed, and the loop
//! synthesizes the call's one error result. A `hook.post` failure only **drops
//! feedback** — the world has already changed, and fail-closed buys no safety on
//! that side.
//!
//! A hook observes only the closed public subset of the stream, [`HookEvent`]:
//! tool, permission and session-boundary events. Messages, reasoning, usage,
//! executor events and session errors are not part of a hook's face.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::events::{
    hook_format, Decision, DecisionSource, Event, EventPayload, SessionId, StopReason, ToolCallId,
};
use crate::tools::Effect;

/// What a `hook.pre` returns: a constraint on the call, never a verdict.
#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    /// Leave the call alone; the gate's verdict stands.
    Continue,
    /// Replace the arguments **before** the gate evaluates the call, so both the
    /// gate and the tool see the rewritten call.
    Rewrite(Value),
    /// Raise the effective verdict. `Tightening` cannot express `Allow`.
    Tighten(Tightening),
    /// Do not run the tool. The loop synthesizes the call's one error result.
    Skip,
    /// Stop the turn now. The loop synthesizes the current call's one error
    /// result and ends the turn with `StopReason::Aborted`.
    Stop,
}

impl Constraint {
    /// The verdict this constraint forces, when it forces one.
    ///
    /// `Continue`, `Rewrite`, `Skip` and `Stop` are flow, not verdicts; only
    /// `Tighten` contributes to the supremum with the gate's verdict.
    pub fn tightening(&self) -> Option<Decision> {
        match self {
            Constraint::Tighten(tightening) => Some(tightening.decision()),
            _ => None,
        }
    }

    /// The text recorded in `HookExecuted.outcome` for this constraint.
    pub fn outcome(&self) -> String {
        match self {
            Constraint::Continue => hook_format::OUTCOME_CONTINUE.to_owned(),
            Constraint::Rewrite(_) => hook_format::OUTCOME_REWRITE.to_owned(),
            Constraint::Tighten(Tightening::Ask) => hook_format::OUTCOME_TIGHTEN_ASK.to_owned(),
            Constraint::Tighten(Tightening::Deny) => hook_format::OUTCOME_TIGHTEN_DENY.to_owned(),
            Constraint::Skip => hook_format::OUTCOME_SKIP.to_owned(),
            Constraint::Stop => hook_format::OUTCOME_STOP.to_owned(),
        }
    }
}

/// The two verdicts a hook may tighten to. `Allow` is absent **by construction**,
/// which is what makes "only tighten" a typing property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tightening {
    Ask,
    Deny,
}

impl Tightening {
    /// The verdict this tightening contributes to the supremum.
    pub fn decision(self) -> Decision {
        match self {
            Tightening::Ask => Decision::Ask,
            Tightening::Deny => Decision::Deny,
        }
    }
}

/// The one merge: the effective verdict is the supremum of a pre-hook's
/// tightening and the gate's verdict. `None` means the hook did not tighten.
///
/// This is the only place the two are combined, so the loop and the pure test
/// cannot drift apart — and because `Tightening` has no `Allow`, the merge can
/// only ever raise a verdict.
pub fn effective_verdict(gate: Decision, tightening: Option<Decision>) -> Decision {
    gate.join(tightening.unwrap_or(Decision::Allow))
}

/// Where a hook is mounted. The one place the two points are spelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPoint {
    PreToolUse,
    PostToolUse,
}

impl HookPoint {
    /// The spelling stored in `HookExecuted.point`.
    pub fn as_str(self) -> &'static str {
        match self {
            HookPoint::PreToolUse => hook_format::POINT_PRE,
            HookPoint::PostToolUse => hook_format::POINT_POST,
        }
    }
}

/// Why a hook could not produce an outcome. Both cases are fail-closed at the
/// pre mount point and feedback-dropping at the post one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HookError {
    #[error("{0}")]
    Failed(String),
    #[error("hook timed out")]
    Timeout,
}

/// One call as a pre-hook sees it: the resolved facts plus the closed public
/// subset of the stream so far.
///
/// The facts are what the gate will judge, so a hook can reason about the same
/// effect and the same resolved paths the gate sees. `history` is borrowed from
/// the session's log and never contains an event outside [`HookEvent`].
pub struct PreHookCall<'a> {
    pub tool_call_id: &'a str,
    pub tool_name: &'a str,
    /// The arguments as they stand now; a `Rewrite` replaces them.
    pub args: &'a Value,
    pub effect: &'a Effect,
    pub write_targets: &'a [PathBuf],
    pub read_targets: &'a [PathBuf],
    pub argv: Option<&'a [String]>,
    pub cwd: &'a Path,
    /// The public subset of the stream, in `seq` order.
    pub history: &'a [HookEvent],
}

/// One resolved call as a post-hook sees it.
pub struct PostHookCall<'a> {
    pub tool_call_id: &'a str,
    pub tool_name: &'a str,
    pub args: &'a Value,
    /// Whether the tool call produced a successful result.
    pub ok: bool,
    pub output: Option<&'a str>,
    pub error: Option<&'a str>,
    /// The public subset of the stream, in `seq` order.
    pub history: &'a [HookEvent],
}

/// The port the loop calls at both mount points.
///
/// A hook states its own `command` (the identity recorded in `HookExecuted`),
/// and may implement either side: the defaults leave a call alone and inject no
/// feedback. There is exactly one hook value per session, shared with nested
/// sessions the way the asker is, so an executor does not lose the strategy.
#[async_trait]
pub trait Hook: Send + Sync {
    /// Identity recorded in `HookExecuted.command`.
    fn command(&self) -> &str;

    /// Runs before the permission gate. The default leaves the call alone.
    async fn pre(&self, _call: &PreHookCall<'_>) -> Result<Constraint, HookError> {
        Ok(Constraint::Continue)
    }

    /// Runs after the tool resolved. `Some` text is feedback the projection
    /// merges into that tool's message; the default injects nothing.
    async fn post(&self, _call: &PostHookCall<'_>) -> Result<Option<String>, HookError> {
        Ok(None)
    }
}

/// The closed public subset of the event stream a hook may observe (spec §2):
/// tool, permission and session-boundary events, seven variants in all.
///
/// A separate type, not a filter over [`EventPayload`], is what keeps the
/// observation surface closed: there is no arm here for messages, reasoning,
/// usage, executor events, history operations or session errors, so no future
/// event leaks into a hook by default.
#[derive(Debug, Clone, PartialEq)]
pub enum HookEvent {
    SessionStarted {
        session_id: SessionId,
        cwd: String,
        schema_version: u32,
    },
    SessionEnded {
        reason: StopReason,
    },
    ToolCallStarted {
        tool_call_id: ToolCallId,
        tool_name: String,
        args: Value,
    },
    ToolCallCompleted {
        tool_call_id: ToolCallId,
        ok: bool,
        output: Option<String>,
        error: Option<String>,
        duration_ms: u64,
    },
    PermissionAsked {
        request_id: String,
        tool_call_id: ToolCallId,
        request: Value,
    },
    PermissionDecided {
        request_id: String,
        decision: Decision,
        source: DecisionSource,
        reason: Option<String>,
    },
    AgentError {
        message: String,
        recoverable: bool,
    },
}

impl HookEvent {
    /// Project one payload into the public subset, or `None` when a hook may not
    /// see it.
    pub fn from_payload(payload: &EventPayload) -> Option<Self> {
        match payload {
            EventPayload::SessionStarted {
                session_id,
                cwd,
                schema_version,
            } => Some(HookEvent::SessionStarted {
                session_id: session_id.clone(),
                cwd: cwd.clone(),
                schema_version: *schema_version,
            }),
            EventPayload::SessionEnded { reason } => {
                Some(HookEvent::SessionEnded { reason: *reason })
            }
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } => Some(HookEvent::ToolCallStarted {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                args: args.clone(),
            }),
            EventPayload::ToolCallCompleted {
                tool_call_id,
                ok,
                output,
                error,
                duration_ms,
            } => Some(HookEvent::ToolCallCompleted {
                tool_call_id: tool_call_id.clone(),
                ok: *ok,
                output: output.clone(),
                error: error.clone(),
                duration_ms: *duration_ms,
            }),
            EventPayload::PermissionAsked {
                request_id,
                tool_call_id,
                request,
            } => Some(HookEvent::PermissionAsked {
                request_id: request_id.clone(),
                tool_call_id: tool_call_id.clone(),
                request: request.clone(),
            }),
            EventPayload::PermissionDecided {
                request_id,
                decision,
                source,
                reason,
            } => Some(HookEvent::PermissionDecided {
                request_id: request_id.clone(),
                decision: *decision,
                source: *source,
                reason: reason.clone(),
            }),
            EventPayload::AgentError {
                message,
                recoverable,
            } => Some(HookEvent::AgentError {
                message: message.clone(),
                recoverable: *recoverable,
            }),
            _ => None,
        }
    }

    /// A short, stable name for diagnostics and assertions.
    pub fn kind(&self) -> &'static str {
        match self {
            HookEvent::SessionStarted { .. } => "SessionStarted",
            HookEvent::SessionEnded { .. } => "SessionEnded",
            HookEvent::ToolCallStarted { .. } => "ToolCallStarted",
            HookEvent::ToolCallCompleted { .. } => "ToolCallCompleted",
            HookEvent::PermissionAsked { .. } => "PermissionAsked",
            HookEvent::PermissionDecided { .. } => "PermissionDecided",
            HookEvent::AgentError { .. } => "AgentError",
        }
    }
}

/// Project a whole stream into the public subset a hook may observe, in `seq`
/// order.
pub fn public_history(events: &[Event]) -> Vec<HookEvent> {
    events
        .iter()
        .filter_map(|event| HookEvent::from_payload(&event.payload))
        .collect()
}
