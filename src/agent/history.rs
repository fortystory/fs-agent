//! The two stream operations that are not the turn loop: closing the calls a
//! killed process left open, and `/undo`.
//!
//! Both are history edits, and both live in the `agent` layer for the same
//! reason every other write does — it is the single writer of the event stream.
//! Recovery **completes** a range the crash left open; `/undo` **retires** a
//! range and restores the bytes it changed (spec §11).
//!
//! Neither is a gesture in the stream: a resume is implied by an unfinished
//! call, and an undo's only trace is the `HistorySuperseded` record of what it
//! did (spec §6, §11).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use serde_json::Value;

use super::{emit, emit_completed, Error};
use crate::events::{
    pending_tool_calls, superseded_seqs, Event, EventPayload, HistoryReason, SpeakerId, ToolCallId,
};
use crate::render::RenderHandle;
use crate::session::Session;
use crate::tools::edit;
use crate::tools::{before_artifact, EditCall, ToolError, EDIT_FILE};

/// The one result a killed process never got to write.
///
/// It says "unknown", not "failed": the call may have taken effect before the
/// process died, so the model must not assume the workspace is untouched — and
/// it must not be re-run on the chance that it did not (spec §11).
const INTERRUPTED: &str =
    "the session was interrupted while this call was in flight, so its result is unknown. \
     It was not re-run; check the workspace before relying on either outcome.";

/// Close every `tool_call` an interrupted process left without a result.
///
/// The query is session-wide (`pending_tool_calls`, not the per-speaker loop
/// form): the whole process died, so every agent's unfinished call is this
/// session's unfinished call (spec §11, Further Notes). Returns how many results
/// were synthesized.
///
/// It is called once, at assembly, before any turn runs — which is also what
/// keeps the loop's invariant 2 true across a resume.
pub fn recover_pending_calls(session: &mut Session, render: &RenderHandle) -> Result<usize, Error> {
    let events = session.events();
    let mut recovered = 0;
    for tool_call_id in pending_tool_calls(&events) {
        // The result must be attributed to the agent that started the call, or
        // projection cannot pair it with the assistant message still holding the
        // `tool_call`.
        let Some(speaker) = started_by(&events, &tool_call_id) else {
            continue;
        };
        emit_completed(
            session,
            render,
            &speaker,
            tool_call_id,
            Err(ToolError::message(INTERRUPTED)),
            Instant::now(),
        )?;
        recovered += 1;
    }
    Ok(recovered)
}

/// Who started a call, read from the stream rather than guessed.
fn started_by(events: &[Event], tool_call_id: &ToolCallId) -> Option<SpeakerId> {
    events.iter().find_map(|event| match &event.payload {
        EventPayload::ToolCallStarted {
            tool_call_id: id, ..
        } if id == tool_call_id => Some(event.speaker_id.clone()),
        _ => None,
    })
}

/// One edit `/undo` can roll back.
#[derive(Debug, Clone, PartialEq)]
struct UndoableEdit {
    tool_call_id: ToolCallId,
    /// The `ToolCallStarted` and `ToolCallCompleted` seqs the undo retires, so
    /// the projection stops replaying the edit.
    started_seq: u64,
    completed_seq: u64,
    /// The arguments the model sent, exactly as the stream recorded them.
    args: Value,
}

/// What an `/undo` did, for the front end to narrate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoOutcome {
    pub tool_call_id: ToolCallId,
    pub path: PathBuf,
}

/// The edit the next `/undo` would roll back, or `None` when there is none.
///
/// A pure query over the stream: the most recent `edit_file` call that succeeded
/// and has not already been retired (by an earlier undo, or by anything else
/// that supersedes history). Walking backwards is what makes repeated undos step
/// back through the session's edits one at a time.
fn last_undoable_edit(events: &[Event]) -> Option<UndoableEdit> {
    let superseded = superseded_seqs(events);
    let mut open: HashMap<ToolCallId, (u64, String, Value)> = HashMap::new();
    let mut last = None;
    for event in events {
        if superseded.contains(&event.seq) {
            continue;
        }
        match &event.payload {
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } => {
                open.insert(
                    tool_call_id.clone(),
                    (event.seq, tool_name.clone(), args.clone()),
                );
            }
            EventPayload::ToolCallCompleted {
                tool_call_id, ok, ..
            } => {
                if let Some((started_seq, tool_name, args)) = open.remove(tool_call_id) {
                    if *ok && tool_name == EDIT_FILE {
                        last = Some(UndoableEdit {
                            tool_call_id: tool_call_id.clone(),
                            started_seq,
                            completed_seq: event.seq,
                            args,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    last
}

/// Roll back the most recent edit: restore its bytes and retire its events.
///
/// The restore source is `outputs/<tool_call_id>.before` — the **actual replaced
/// bytes**, so an edit that landed on a downgraded match restores exactly too
/// (spec §11). The same per-path lock the edit took is taken here, so an undo
/// cannot interleave with another writer, and the user's git is never touched.
///
/// Returns `Ok(None)` when there is no edit to undo. Every other failure is an
/// error and leaves the workspace alone: a stale snapshot is refused rather than
/// guessed at.
pub async fn undo_last_edit(
    session: &mut Session,
    render: &RenderHandle,
) -> Result<Option<UndoOutcome>, Error> {
    let events = session.events();
    let Some(edit) = last_undoable_edit(&events) else {
        return Ok(None);
    };

    let parsed: EditCall = serde_json::from_value(edit.args.clone()).map_err(|error| {
        Error::Undo(format!(
            "the recorded edit_file arguments cannot be read back: {error}"
        ))
    })?;
    let path = session
        .paths()
        .resolve(&PathBuf::from(&parsed.file_path))
        .map_err(|error| Error::Undo(error.to_string()))?;

    let before = std::fs::read_to_string(
        session
            .outputs_dir()
            .join(before_artifact(edit.tool_call_id.as_str())),
    )
    .map_err(|error| {
        Error::Undo(format!(
            "cannot read the snapshot for {}: {error}",
            edit.tool_call_id
        ))
    })?;

    // The same lock the edit itself took (spec §11): an undo is a writer of the
    // path, so it queues with every other writer of it.
    let _guard = session.path_locks().lock(&path).await;

    let content = std::fs::read_to_string(&path)
        .map_err(|error| Error::Undo(format!("cannot read {}: {error}", path.display())))?;
    let restored = edit::revert(
        &content,
        &before,
        &parsed.old_string,
        &parsed.new_string,
        parsed.replace_all,
    )
    .map_err(|error| {
        Error::Undo(format!(
            "cannot undo the edit to {}: {error}",
            path.display()
        ))
    })?;
    std::fs::write(&path, restored.as_bytes())
        .map_err(|error| Error::Undo(format!("cannot restore {}: {error}", path.display())))?;

    // The gesture itself never enters the stream; its effect does.
    emit(
        session,
        render,
        &SpeakerId::User,
        EventPayload::HistorySuperseded {
            targets: vec![edit.started_seq, edit.completed_seq],
            reason: HistoryReason::Undo,
            summary: Some(format!("undo edit_file on {}", path.display())),
        },
    )?;

    Ok(Some(UndoOutcome {
        tool_call_id: edit.tool_call_id,
        path,
    }))
}
