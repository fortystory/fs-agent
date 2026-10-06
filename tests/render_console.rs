//! 键盘接缝：循环提问，前端作答，两边都不直接读终端
//! （spec §19，用户故事 133）。
//!
//! 这些测试拿一个很小的脚本化前端顶替渲染器，这正是这条接缝
//! 的意义所在：循环那一侧不用终端就能练到。

use std::sync::Arc;

use heng::permissions::{Answer, Asker, PermissionRequest};
use heng::render::{console, ConsoleAsker, ConsoleRequest, FrontEndEvent};

#[tokio::test]
async fn a_prompt_travels_out_and_the_answer_comes_back() {
    let (handle, mut port, _events) = console();
    let front_end = tokio::spawn(async move {
        match port.recv().await {
            Some(ConsoleRequest::Prompt { reply }) => {
                let _ = reply.send(Some("fix the bug".to_owned()));
            }
            other => panic!("期望一次提示，实际得到 {other:?}"),
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
            other => panic!("期望一次询问，实际得到 {other:?}"),
        }
    });

    let request = PermissionRequest {
        escalation: None,
        speaker: None,
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
    // 已经关掉的前端不能凭空造出同意：这个问句被拒绝（spec §12）。
    let (handle, port, _events) = console();
    drop(port);
    let asker = ConsoleAsker::from_handle(&handle);
    let request = PermissionRequest {
        escalation: None,
        speaker: None,
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
    // Esc 与 Shift+Tab 是前端自己的日程：一个回合进行中的时候，
    // 循环 select 的正是这条通道。
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
    // 权限门手里是 `Arc<dyn Asker>`；所以作答者必须能
    // 通过一个共享引用使用。
    let (handle, mut port, _events) = console();
    let asker: Arc<dyn Asker> = Arc::new(ConsoleAsker::from_handle(&handle));
    let front_end = tokio::spawn(async move {
        if let Some(ConsoleRequest::Ask(ask)) = port.recv().await {
            let _ = ask.reply.send(Answer::Allow);
        }
    });
    let request = PermissionRequest {
        escalation: None,
        speaker: None,
        request_id: "r-2".to_owned(),
        tool_call_id: "c-2".to_owned(),
        tool_name: "edit_file".to_owned(),
        args: serde_json::json!({}),
        reason: "mode ask".to_owned(),
    };
    assert_eq!(asker.ask(&request).await, Answer::Allow);
    front_end.await.unwrap();
}
