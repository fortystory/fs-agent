//! The plain renderer at the render seam: feed it events on the injected channel
//! and assert the two sinks (spec §19, Testing Decisions category 10).
//!
//! Nothing here drives a real turn: the renderer's contract is over the event
//! sequence, which is exactly what these tests supply.

mod support;

use fs_agent::events::{hook_format, Event, EventPayload, Role, SpeakerId, StopReason, ToolCallId};
use fs_agent::render::{channel, PlainOptions, RenderHandle, RenderSinks, Renderer};
use support::CaptureBuf;

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

/// Spawn the plain renderer on an injected channel and return its handle plus
/// the two captured sinks.
fn renderer(
    color: bool,
) -> (
    RenderHandle,
    CaptureBuf,
    CaptureBuf,
    tokio::task::JoinHandle<()>,
) {
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();
    let (handle, receiver) = channel();
    let task = Renderer::plain(PlainOptions {
        sinks: RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        },
        color,
    })
    .spawn(receiver);
    (handle, stdout, stderr, task)
}

async fn run(events: &[Event], color: bool) -> (CaptureBuf, CaptureBuf) {
    let (handle, stdout, stderr, task) = renderer(color);
    for event in events {
        handle.logged(event);
    }
    drop(handle);
    task.await.unwrap();
    (stdout, stderr)
}

#[tokio::test]
async fn every_streamed_line_carries_the_speaker_prefix() {
    // A multi-agent transcript interleaves, so a prefix that appears once per
    // block would make it unreadable (spec §19).
    let (handle, _stdout, stderr, task) = renderer(false);
    handle.text_delta(&kimi(), "first line\nsecond ");
    handle.text_delta(&kimi(), "line continues");
    drop(handle);
    task.await.unwrap();

    let text = stderr.text();
    assert!(text.contains("[kimi] first line"), "{text:?}");
    assert!(text.contains("[kimi] second line continues"), "{text:?}");
}

#[tokio::test]
async fn a_message_without_deltas_is_still_shown() {
    // A provider that does not stream, or the synthesizer's whole message, has
    // no deltas behind it — the completed message is the only copy.
    let events = [Event::new(
        1,
        kimi(),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: "hello\nthere".to_owned(),
            reasoning: None,
        },
    )];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    assert!(text.contains("[kimi] hello"), "{text:?}");
    assert!(text.contains("[kimi] there"), "{text:?}");
}

#[tokio::test]
async fn rounds_are_sectioned_and_divergences_are_indented() {
    let events = [
        Event::new(
            1,
            SpeakerId::System,
            EventPayload::RoundStarted {
                round: 2,
                mode: fs_agent::events::RoundMode::Targeted,
            },
        ),
        Event::new(
            2,
            SpeakerId::System,
            EventPayload::DivergenceRecorded {
                round: 2,
                topic: "the seam".to_owned(),
                positions: vec!["trace it".to_owned(), "map it".to_owned()],
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    assert!(text.contains("── round 2 (Targeted) ──"), "{text:?}");
    assert!(text.contains("!! divergence: the seam"), "{text:?}");
    assert!(
        text.contains("  - trace it") && text.contains("  - map it"),
        "{text:?}"
    );
}

#[tokio::test]
async fn a_tool_result_and_its_post_hook_read_as_one_block() {
    // The hook event carries no `tool_call_id`, so the only way to group it is to
    // hold the call open until the next unrelated event (spec §19).
    let id = ToolCallId::new("call-1");
    let events = [
        Event::new(
            1,
            kimi(),
            EventPayload::ToolCallStarted {
                tool_call_id: id.clone(),
                tool_name: "read_file".to_owned(),
                args: serde_json::json!({"path": "src/lib.rs"}),
            },
        ),
        Event::new(
            2,
            kimi(),
            EventPayload::ToolCallCompleted {
                tool_call_id: id.clone(),
                ok: true,
                output: Some("fn main() {}".to_owned()),
                error: None,
                duration_ms: 3,
            },
        ),
        Event::new(
            3,
            kimi(),
            EventPayload::HookExecuted {
                point: hook_format::POINT_POST.to_owned(),
                command: "check".to_owned(),
                outcome: hook_format::feedback("looks fine").to_owned(),
            },
        ),
        Event::new(
            4,
            kimi(),
            EventPayload::TurnEnded {
                reason: StopReason::Completed,
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    let head = text
        .find("[kimi] → read_file(path=src/lib.rs)")
        .expect("tool head");
    let output = text.find("  fn main() {}").expect("tool output");
    let hook = text
        .find("  [hook] feedback: looks fine")
        .expect("hook feedback");
    assert!(head < output && output < hook, "{text:?}");
}

#[tokio::test]
async fn the_post_hook_is_not_printed_as_a_separate_event() {
    // The pre-hook stays its own line; the post-hook is merged into the call it
    // annotates and must not also appear as a standalone row.
    let id = ToolCallId::new("call-2");
    let events = [
        Event::new(
            1,
            kimi(),
            EventPayload::ToolCallStarted {
                tool_call_id: id.clone(),
                tool_name: "bash".to_owned(),
                args: serde_json::json!({"command": "true"}),
            },
        ),
        Event::new(
            2,
            kimi(),
            EventPayload::ToolCallCompleted {
                tool_call_id: id,
                ok: true,
                output: Some(String::new()),
                error: None,
                duration_ms: 1,
            },
        ),
        Event::new(
            3,
            kimi(),
            EventPayload::HookExecuted {
                point: hook_format::POINT_POST.to_owned(),
                command: "check".to_owned(),
                outcome: hook_format::feedback("ok").to_owned(),
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    assert_eq!(text.matches("[hook]").count(), 1, "{text:?}");
    assert!(!text.contains("hook post_tool_use"), "{text:?}");
}

#[tokio::test]
async fn the_synthesizers_message_is_the_only_thing_on_stdout() {
    let events = [
        Event::new(
            1,
            SpeakerId::Debater("kimi".into()),
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text: "a debater's answer".to_owned(),
                reasoning: None,
            },
        ),
        Event::new(
            2,
            SpeakerId::System,
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text: "the option space".to_owned(),
                reasoning: None,
            },
        ),
    ];
    let (stdout, _stderr) = run(&events, false).await;
    assert_eq!(stdout.text(), "the option space\n");
}

#[tokio::test]
async fn completed_and_aborted_do_not_render_the_same() {
    // User story 135: a run that finished and a run that hit a wall must be
    // distinguishable at a glance.
    let events = [
        Event::new(
            1,
            kimi(),
            EventPayload::TurnEnded {
                reason: StopReason::Completed,
            },
        ),
        Event::new(
            2,
            kimi(),
            EventPayload::TurnEnded {
                reason: StopReason::Aborted,
            },
        ),
        Event::new(
            3,
            kimi(),
            EventPayload::TurnEnded {
                reason: StopReason::Error,
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, true).await;
    let text = stderr.text();
    // Green for the completed turn, yellow for the abort, red for the error.
    assert!(
        text.contains("\x1b[32m[kimi] turn ended: Completed"),
        "{text:?}"
    );
    assert!(
        text.contains("\x1b[33m[kimi] turn ended: Aborted"),
        "{text:?}"
    );
    assert!(
        text.contains("\x1b[31m[kimi] turn ended: Error"),
        "{text:?}"
    );
}

#[tokio::test]
async fn a_permission_decision_between_start_and_result_does_not_split_the_call() {
    // The loop records a `PermissionDecided` for **every** call, asked or not,
    // and a pre-hook `HookExecuted` also fires between `ToolCallStarted` and the
    // result. Neither may close the open block, or every live tool call would
    // render as a resultless call plus a synthetic `?`.
    let id = ToolCallId::new("call-4");
    let events = [
        Event::new(
            1,
            kimi(),
            EventPayload::ToolCallStarted {
                tool_call_id: id.clone(),
                tool_name: "write_file".to_owned(),
                args: serde_json::json!({"path": "a.txt"}),
            },
        ),
        Event::new(
            2,
            kimi(),
            EventPayload::HookExecuted {
                point: hook_format::POINT_PRE.to_owned(),
                command: "check".to_owned(),
                outcome: hook_format::OUTCOME_CONTINUE.to_owned(),
            },
        ),
        Event::new(
            3,
            kimi(),
            EventPayload::PermissionAsked {
                request_id: "r-1".to_owned(),
                tool_call_id: id.clone(),
                request: serde_json::json!({}),
            },
        ),
        Event::new(
            4,
            kimi(),
            EventPayload::PermissionDecided {
                request_id: "r-1".to_owned(),
                decision: fs_agent::events::Decision::Allow,
                source: fs_agent::events::DecisionSource::Policy,
                reason: Some("mode auto".to_owned()),
            },
        ),
        Event::new(
            5,
            kimi(),
            EventPayload::ToolCallCompleted {
                tool_call_id: id,
                ok: true,
                output: Some("wrote a.txt".to_owned()),
                error: None,
                duration_ms: 2,
            },
        ),
        Event::new(
            6,
            kimi(),
            EventPayload::TurnEnded {
                reason: StopReason::Completed,
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    assert!(
        text.contains("[kimi] → write_file(path=a.txt)"),
        "the call keeps its arguments: {text:?}"
    );
    assert!(text.contains("  wrote a.txt"), "and its result: {text:?}");
    assert!(
        !text.contains("→ ?("),
        "the completion must not become a second, anonymous block: {text:?}"
    );
    // The decision is still narrated, just not as a block boundary.
    assert!(text.contains("permission allow"), "{text:?}");
}

#[tokio::test]
async fn a_call_with_no_result_still_appears_when_the_stream_ends() {
    // A cancel leaves a `ToolCallStarted` with no result; the block is flushed at
    // end of stream rather than dropped (spec §19).
    let id = ToolCallId::new("call-3");
    let events = [Event::new(
        1,
        kimi(),
        EventPayload::ToolCallStarted {
            tool_call_id: id,
            tool_name: "bash".to_owned(),
            args: serde_json::json!({"command": "sleep 300"}),
        },
    )];
    let (_stdout, stderr) = run(&events, false).await;
    assert!(
        stderr.text().contains("(no result on the stream)"),
        "{:?}",
        stderr.text()
    );
}
