//! The keyboard seam: the loop asks, the front end answers, and neither reads the
//! terminal directly (spec §19, user story 133).
//!
//! These tests stand a tiny scripted front end in for a renderer, which is the
//! point of the seam: the loop's side is exercised without a terminal.

use std::sync::Arc;

use fs_agent::permissions::{Answer, Asker, PermissionRequest};
use fs_agent::render::{console, ConsoleAsker, ConsoleRequest, FrontEndEvent};

#[tokio::test]
async fn a_prompt_travels_out_and_the_answer_comes_back() {
    let (handle, mut port, _events) = console();
    let front_end = tokio::spawn(async move {
        match port.recv().await {
            Some(ConsoleRequest::Prompt { reply }) => {
                let _ = reply.send(Some("fix the bug".to_owned()));
            }
            other => panic!("expected a prompt, got {other:?}"),
        }
        port
    });

    assert_eq!(handle.prompt().await.as_deref(), Some("fix the bug"));
    front_end.await.unwrap();
}

#[tokio::test]
async fn end_of_input_is_reported_as_none() {
    let (handle, mut port, _events) = console();
    let front_end = tokio::spawn(async move {
        if let Some(ConsoleRequest::Prompt { reply }) = port.recv().await {
            let _ = reply.send(None);
        }
    });

    assert_eq!(handle.prompt().await, None);
    front_end.await.unwrap();
}

#[tokio::test]
async fn the_asker_routes_a_permission_question_through_the_same_keyboard() {
    let (handle, mut port, _events) = console();
    let asker = ConsoleAsker::from_handle(&handle);
    let front_end = tokio::spawn(async move {
        match port.recv().await {
            Some(ConsoleRequest::Ask(ask)) => {
                assert_eq!(ask.request.tool_name, "write_file");
                let _ = ask.reply.send(Answer::AlwaysAllow);
            }
            other => panic!("expected an ask, got {other:?}"),
        }
    });

    let request = PermissionRequest {
        request_id: "r-1".to_owned(),
        tool_call_id: "c-1".to_owned(),
        tool_name: "write_file".to_owned(),
        args: serde_json::json!({"path": "a.txt"}),
        reason: "mode ask".to_owned(),
    };
    assert_eq!(asker.ask(&request).await, Answer::AlwaysAllow);
    front_end.await.unwrap();
}

#[tokio::test]
async fn with_no_front_end_the_answer_is_the_non_acting_one() {
    // A closed front end must not invent consent: the question is denied (spec §12).
    let (handle, port, _events) = console();
    drop(port);
    let asker = ConsoleAsker::from_handle(&handle);
    let request = PermissionRequest {
        request_id: "r-1".to_owned(),
        tool_call_id: "c-1".to_owned(),
        tool_name: "bash".to_owned(),
        args: serde_json::json!({"command": "rm -rf /"}),
        reason: "mode ask".to_owned(),
    };
    assert_eq!(asker.ask(&request).await, Answer::Deny);
}

#[tokio::test]
async fn a_gesture_reaches_the_loop_without_being_asked_for() {
    // Esc and Shift+Tab are the front end's own schedule: the loop selects on
    // this while a turn is in flight.
    let (_handle, port, mut events) = console();
    port.emit(FrontEndEvent::Cancel);
    port.emit(FrontEndEvent::CycleMode);
    assert_eq!(events.recv().await, Some(FrontEndEvent::Cancel));
    assert_eq!(events.recv().await, Some(FrontEndEvent::CycleMode));
    drop(port);
    assert_eq!(events.recv().await, None);
}

#[tokio::test]
async fn a_generic_asker_handle_can_be_shared() {
    // The gate holds an `Arc<dyn Asker>`; the asker must therefore be usable
    // through a shared reference.
    let (handle, mut port, _events) = console();
    let asker: Arc<dyn Asker> = Arc::new(ConsoleAsker::from_handle(&handle));
    let front_end = tokio::spawn(async move {
        if let Some(ConsoleRequest::Ask(ask)) = port.recv().await {
            let _ = ask.reply.send(Answer::Allow);
        }
    });
    let request = PermissionRequest {
        request_id: "r-2".to_owned(),
        tool_call_id: "c-2".to_owned(),
        tool_name: "edit_file".to_owned(),
        args: serde_json::json!({}),
        reason: "mode ask".to_owned(),
    };
    assert_eq!(asker.ask(&request).await, Answer::Allow);
    front_end.await.unwrap();
}
