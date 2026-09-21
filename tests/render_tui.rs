//! The TUI's state machine and shared block rendering, tested without a terminal.
//!
//! The terminal itself is crossterm's; what this crate owns is the state that
//! decides what to draw and what a key means. Splitting it from the terminal is
//! what makes that testable (spec §19).

use fs_agent::events::{hook_format, Event, EventPayload, Role, SpeakerId, StopReason, ToolCallId};
use fs_agent::permissions::{Answer, PermissionRequest};
use fs_agent::render::{
    paint_scrollback, render_block, AnswerChoice, AskRequest, Block, ConsoleRequest, DeltaKind,
    FrontEndEvent, Key, Question, RenderEvent, ToolBlock, ToolOutcome, TuiState,
};
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Line;

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

fn state_with_prompt() -> (TuiState, tokio::sync::oneshot::Receiver<Option<String>>) {
    let mut state = TuiState::new();
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    (state, rx)
}

#[test]
fn a_typed_line_is_submitted_to_the_loop() {
    let (mut state, mut answer) = state_with_prompt();
    for ch in "hello".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), Some("hello".to_owned()));
}

#[test]
fn backspace_edits_the_line() {
    let (mut state, mut answer) = state_with_prompt();
    state.key(Key::Char('a'));
    state.key(Key::Char('b'));
    state.key(Key::Backspace);
    state.key(Key::Char('c'));
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), Some("ac".to_owned()));
}

#[test]
fn an_empty_submission_closes_the_prompt() {
    let (mut state, mut answer) = state_with_prompt();
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), None);
}

#[test]
fn a_permission_question_is_answered_by_key() {
    let mut state = TuiState::new();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        question: Question::Permission(PermissionRequest {
            request_id: "r-1".to_owned(),
            tool_call_id: "c-1".to_owned(),
            tool_name: "write_file".to_owned(),
            args: serde_json::json!({}),
            reason: "mode ask".to_owned(),
        }),
        reply: tx,
    }));
    state.key(Key::Char('a'));
    assert_eq!(
        rx.try_recv().unwrap(),
        AnswerChoice::Permission(Answer::AlwaysAllow)
    );
}

#[test]
fn escape_answers_a_question_with_the_non_acting_choice() {
    let mut state = TuiState::new();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        question: Question::Permission(PermissionRequest {
            request_id: "r-1".to_owned(),
            tool_call_id: "c-1".to_owned(),
            tool_name: "bash".to_owned(),
            args: serde_json::json!({}),
            reason: "mode ask".to_owned(),
        }),
        reply: tx,
    }));
    state.key(Key::Esc);
    assert_eq!(
        rx.try_recv().unwrap(),
        AnswerChoice::Permission(Answer::Deny)
    );
}

#[test]
fn escape_while_working_is_a_cancel_gesture() {
    let mut state = TuiState::new();
    // A delta means a turn is in flight.
    state.apply(RenderEvent::Delta {
        speaker: kimi(),
        kind: fs_agent::render::DeltaKind::Text,
        text: "thinking".to_owned(),
    });
    state.key(Key::Esc);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
}

#[test]
fn an_idle_ctrl_c_quits_and_a_working_one_cancels() {
    let mut state = TuiState::new();
    state.key(Key::CtrlC);
    assert!(state.should_quit());

    let mut state = TuiState::new();
    state.apply(RenderEvent::Delta {
        speaker: kimi(),
        kind: fs_agent::render::DeltaKind::Text,
        text: "working".to_owned(),
    });
    state.key(Key::CtrlC);
    assert!(!state.should_quit());
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
}

#[test]
fn shift_tab_is_the_plan_gesture() {
    let mut state = TuiState::new();
    state.key(Key::BackTab);
    assert_eq!(state.take_events(), vec![FrontEndEvent::TogglePlan]);
}

#[test]
fn a_tool_call_and_its_hook_become_one_ready_block() {
    let id = ToolCallId::new("call-1");
    let mut state = TuiState::new();
    state.apply(RenderEvent::Logged(Event::new(
        1,
        kimi(),
        EventPayload::ToolCallStarted {
            tool_call_id: id.clone(),
            tool_name: "read_file".to_owned(),
            args: serde_json::json!({"path": "a.rs"}),
        },
    )));
    state.apply(RenderEvent::Logged(Event::new(
        2,
        kimi(),
        EventPayload::ToolCallCompleted {
            tool_call_id: id,
            ok: true,
            output: Some("fn main() {}".to_owned()),
            error: None,
            duration_ms: 2,
        },
    )));
    state.apply(RenderEvent::Logged(Event::new(
        3,
        kimi(),
        EventPayload::HookExecuted {
            point: hook_format::POINT_POST.to_owned(),
            command: "check".to_owned(),
            outcome: hook_format::feedback("ok").to_owned(),
        },
    )));
    // Nothing is ready until the next unrelated event closes the block.
    assert!(state.take_ready().is_empty());
    state.apply(RenderEvent::Logged(Event::new(
        4,
        kimi(),
        EventPayload::TurnEnded {
            reason: StopReason::Completed,
        },
    )));
    let ready = state.take_ready();
    let tool = ready
        .iter()
        .find_map(|block| match block {
            Block::Tool(tool) => Some(tool.as_ref()),
            _ => None,
        })
        .expect("the call is one block");
    assert_eq!(tool.tool, "read_file");
    assert_eq!(tool.hook.as_deref(), Some("feedback: ok"));
    assert!(tool.outcome.as_ref().unwrap().ok);
}

#[test]
fn a_completed_turn_and_an_aborted_one_render_in_different_colors() {
    let good = render_block(&Block::TurnEnded {
        speaker: kimi(),
        reason: StopReason::Completed,
    });
    let aborted = render_block(&Block::TurnEnded {
        speaker: kimi(),
        reason: StopReason::Aborted,
    });
    let bad = render_block(&Block::TurnEnded {
        speaker: kimi(),
        reason: StopReason::Error,
    });
    assert_eq!(good[0].spans[0].style.fg, Some(Color::Green));
    assert_eq!(aborted[0].spans[0].style.fg, Some(Color::Yellow));
    assert_eq!(bad[0].spans[0].style.fg, Some(Color::Red));
}

#[test]
fn a_diff_line_gets_a_background_from_the_diff_layer() {
    // The diff layer is independent of the syntax layer, so it survives even on a
    // line the grammar cannot parse as Rust (spec §19).
    let tool = ToolBlock {
        speaker: kimi(),
        tool_call_id: ToolCallId::new("call-2"),
        tool: "read_file".to_owned(),
        args: serde_json::json!({"path": "a.rs"}),
        outcome: Some(ToolOutcome {
            ok: true,
            output: Some("+fn main() {}".to_owned()),
            error: None,
            duration_ms: 1,
        }),
        hook: None,
    };
    let lines = render_block(&Block::Tool(Box::new(tool)));
    // Line 0 is the header; line 1 is the highlighted body.
    let body = &lines[1];
    assert!(body.spans.iter().any(|span| span.style.bg.is_some()));
}

#[test]
fn the_synthesizers_product_renders_with_the_system_speaker() {
    let lines = render_block(&Block::Message {
        speaker: SpeakerId::System,
        role: Role::Assistant,
        text: "consensus".to_owned(),
    });
    // The first span is the speaker prefix. Its exact words belong to the wording
    // layer; here it only has to be a bracketed attribution.
    let prefix = lines[0].spans[0].content.as_ref();
    assert!(
        prefix.starts_with('[') && prefix.ends_with("] "),
        "a bracketed speaker prefix: {prefix:?}"
    );
}

#[test]
fn a_notice_is_a_scrollback_line_shown_as_it_is() {
    // The startup banner is a notice, not a diagnostic: no `[diag]` label is
    // added, and it goes into scrollback rather than into the live region, which
    // is what keeps it clear of the status row (spec §19, §A.12).
    let banner = "fs-agent: session abc · model m · mode ask · /tmp/x";
    let mut state = TuiState::new();
    state.apply(RenderEvent::Notice(banner.to_owned()));

    let ready = state.take_ready();
    assert_eq!(ready.len(), 1);
    assert!(matches!(&ready[0], Block::Notice(message) if message == banner));

    let lines = render_block(&ready[0]);
    let text: String = lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(text, banner);
}

#[test]
fn a_message_continuation_indents_by_the_label_display_width() {
    // A Chinese label is narrower in characters than in columns (`[用户]` is 4
    // characters, 6 columns), so indenting by `chars().count()` put the second
    // line two columns left of the first. The indent must measure columns.
    let lines = render_block(&Block::Message {
        speaker: SpeakerId::User,
        role: Role::Assistant,
        text: "one\ntwo".to_owned(),
    });
    let prefix = lines[0].spans[0].content.as_ref().cell_width() as usize;
    let indent = lines[1].spans[0].content.as_ref().cell_width() as usize;
    assert_eq!(
        indent, prefix,
        "the continuation lines up under the body of the first line"
    );
}

#[test]
fn a_wide_grapheme_leaves_no_blank_cell_after_it() {
    // Scrollback goes out through `Terminal::insert_before`, which hands every
    // cell of the scratch buffer to the backend and prints the cell's symbol. A
    // wide grapheme's trailing cell is `Cell::EMPTY`, whose symbol is a space, so
    // a CJK transcript would print a blank column after every character — one
    // column too many per wide character, enough to soft-wrap the line and push
    // the live region out of place (spec §19).
    let line = Line::from("[user] 你好");
    let mut buf = Buffer::empty(Rect::new(0, 0, 16, 1));
    paint_scrollback(&[line], &mut buf);

    let symbols: Vec<&str> = (0..16).map(|x| buf[(x, 0)].symbol()).collect();
    assert_eq!(
        symbols,
        vec!["[", "u", "s", "e", "r", "]", " ", "你", "", "好", "", " ", " ", " ", " ", " "]
    );
}

#[test]
fn the_live_tail_wraps_on_display_columns_not_bytes() {
    // A CJK character is two columns wide but three bytes long. Wrapping on the
    // byte index cut the streaming tail at a third of the terminal width — the
    // one place where Chinese looked broken even though every cell was right
    // (spec §19).
    let mut state = TuiState::new();
    state.apply(RenderEvent::Delta {
        speaker: kimi(),
        kind: DeltaKind::Text,
        text: "你好世界五六".to_owned(),
    });

    // Six characters are twelve columns: five fit in ten, and the sixth wraps.
    assert_eq!(state.live_lines(10), vec!["你好世界五", "六"]);
}

#[test]
fn the_cursor_column_counts_a_wide_character_as_two() {
    // The cursor sat one column left of the input for every CJK character typed.
    let mut state = TuiState::new();
    for ch in "你好".chars() {
        state.key(Key::Char(ch));
    }

    // Two prompt cells, then two characters of two columns each.
    assert_eq!(state.cursor_column(), 6);
}
