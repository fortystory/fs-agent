//! The TUI's state machine and shared block rendering, tested without a terminal.
//!
//! The terminal itself is crossterm's; what this crate owns is the state that
//! decides what to draw and what a key means. Splitting it from the terminal is
//! what makes that testable (spec §Testing Decisions).

use fs_agent::events::{
    hook_format, Decision, DecisionSource, Event, EventPayload, Role, SpeakerId, StopReason,
    ToolCallId,
};
use fs_agent::permissions::{Answer, PermissionRequest};
use fs_agent::render::{
    pane, render_block, AnswerChoice, AskRequest, Block, ConsoleRequest, FrontEndEvent, Key,
    Question, RenderEvent, SessionFacts, ToolBlock, ToolOutcome, Transcript, TuiState,
};
use ratatui::buffer::CellWidth;
use ratatui::style::{Color, Modifier};

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

fn facts() -> SessionFacts {
    SessionFacts {
        session_id: "01J8ZQ4K7M".to_owned(),
        cwd: "~/code/fortystory/fs-agent".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        budget_limit: Some(100_000),
    }
}

fn new_state() -> TuiState {
    TuiState::new(facts())
}

fn state_with_prompt() -> (TuiState, tokio::sync::oneshot::Receiver<Option<String>>) {
    let mut state = new_state();
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
    let mut state = new_state();
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
    let mut state = new_state();
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
    let mut state = new_state();
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
    let mut state = new_state();
    state.key(Key::CtrlC);
    assert!(state.should_quit());

    let mut state = new_state();
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
    let mut state = new_state();
    state.key(Key::BackTab);
    assert_eq!(state.take_events(), vec![FrontEndEvent::TogglePlan]);
}

#[test]
fn a_tool_call_and_its_hook_become_one_ready_block() {
    // The merger is the transcript's, not the state machine's: it holds the call
    // open until an unrelated event closes it, so the result and the hook that
    // annotates it land in one block.
    let id = ToolCallId::new("call-1");
    let mut transcript = Transcript::new();
    let call = Event::new(
        1,
        kimi(),
        EventPayload::ToolCallStarted {
            tool_call_id: id.clone(),
            tool_name: "read_file".to_owned(),
            args: serde_json::json!({"path": "a.rs"}),
        },
    );
    let result = Event::new(
        2,
        kimi(),
        EventPayload::ToolCallCompleted {
            tool_call_id: id,
            ok: true,
            output: Some("fn main() {}".to_owned()),
            error: None,
            duration_ms: 2,
        },
    );
    let hook = Event::new(
        3,
        kimi(),
        EventPayload::HookExecuted {
            point: hook_format::POINT_POST.to_owned(),
            command: "check".to_owned(),
            outcome: hook_format::feedback("ok").to_owned(),
        },
    );
    for event in [call, result, hook] {
        assert!(
            transcript.push(RenderEvent::Logged(event)).is_empty(),
            "the call is held open"
        );
    }
    // Nothing is ready until the next unrelated event closes the block.
    let ready = transcript.push(RenderEvent::Logged(Event::new(
        4,
        kimi(),
        EventPayload::TurnEnded {
            reason: StopReason::Completed,
        },
    )));
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
fn a_notice_is_a_transcript_line_shown_as_it_is() {
    // The startup banner is a notice, not a diagnostic: no `[diag]` label is
    // added, and it belongs to the transcript, where it stays put rather than
    // scrolling away with the streaming tail it deliberately avoids
    // (spec §A.12, §3).
    let banner = "fs-agent: session abc · model m · mode ask · /tmp/x";
    let lines = render_block(&Block::Notice(banner.to_owned()));
    let text: String = lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(text, banner);
}

#[test]
fn the_answer_block_is_rendered_as_markdown() {
    // The finalized answer goes through the Markdown renderer, so a heading is a
    // heading and a bullet is a bullet — the readable half of the transcript.
    let lines = render_block(&Block::Message {
        speaker: kimi(),
        role: Role::Assistant,
        text: "# 标题\n\n- 一\n- 二\n".to_owned(),
    });
    let heading = &lines[0];
    assert!(
        heading
            .spans
            .iter()
            .any(|span| span.style.fg == Some(Color::Cyan)
                && span.style.add_modifier.contains(Modifier::BOLD)),
        "the first line is the rendered heading: {heading:?}"
    );
    assert_eq!(heading.spans.last().unwrap().content.as_ref(), "标题");

    let rendered: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect();
    assert!(
        rendered.iter().any(|line| line.contains("• 一")),
        "the list marker is rendered, not the raw dash: {rendered:?}"
    );
}

#[test]
fn intermediate_narration_is_dim_and_the_answer_is_not() {
    // Every narration line reads the same dim grey, whichever event it narrates,
    // so the model's answer — at full brightness — is what stands out.
    let verdict = render_block(&Block::PermissionDecided {
        speaker: kimi(),
        decision: Decision::Allow,
        source: DecisionSource::User,
        reason: Some("mode ask".to_owned()),
    });
    assert_eq!(
        verdict[0].spans[0].style.fg,
        Some(Color::DarkGray),
        "a permission verdict is narration"
    );

    let asked = render_block(&Block::PermissionAsked {
        speaker: kimi(),
        tool_name: Some("bash".to_owned()),
        args: serde_json::json!({"command": "ls"}),
    });
    assert_eq!(asked[0].spans[0].style.fg, Some(Color::DarkGray));

    let answer = render_block(&Block::Message {
        speaker: kimi(),
        role: Role::Assistant,
        text: "正文".to_owned(),
    });
    assert_ne!(
        answer[0].spans[1].style.fg,
        Some(Color::DarkGray),
        "the answer body is not dimmed"
    );
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
fn the_live_tail_wraps_on_display_columns_not_bytes() {
    // A CJK character is two columns wide but three bytes long. Wrapping on the byte
    // index cut the streaming tail at a third of the width — the one place where
    // Chinese looked broken even though every cell was right (spec §3).
    let rows = pane::wrap_text("你好世界五六", 10);
    let texts: Vec<String> = rows
        .iter()
        .map(|row| row.spans.iter().map(|span| span.content.as_ref()).collect())
        .collect();
    assert_eq!(texts, vec!["你好世界五", "六"]);
}

#[test]
fn the_cursor_column_counts_a_wide_character_as_two() {
    // The cursor sat one column left of the input for every CJK character typed.
    let mut state = new_state();
    for ch in "你好".chars() {
        state.key(Key::Char(ch));
    }

    // Two prompt cells, then two characters of two columns each.
    assert_eq!(state.cursor_column(80), 6);
}

#[test]
fn the_arrows_and_emacs_keys_edit_the_line_in_place() {
    let (mut state, mut answer) = state_with_prompt();
    for ch in "helo".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Left); // hel|o
    state.key(Key::Char('l')); // hell|o
    state.key(Key::Home);
    state.key(Key::Delete); // removes the leading 'h' -> ell|o
    state.key(Key::End);
    state.key(Key::Backspace); // ell|
    state.key(Key::Char('o'));
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), Some("ello".to_owned()));
}

#[test]
fn ctrl_a_e_u_k_edit_like_emacs() {
    let (mut state, mut answer) = state_with_prompt();
    for ch in "abcdef".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::CtrlA); // |abcdef
    state.key(Key::CtrlK); // clears forward -> |
    state.key(Key::Char('x')); // x|
    state.key(Key::Char('y')); // xy|
    state.key(Key::CtrlA); // |xy
    state.key(Key::Delete); // |y
    state.key(Key::CtrlE); // y|
    state.key(Key::CtrlU); // clears backward -> |
    state.key(Key::Char('z'));
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), Some("z".to_owned()));
}

#[test]
fn ctrl_w_erases_the_word_before_the_cursor() {
    let (mut state, mut answer) = state_with_prompt();
    for ch in "cargo test --lib".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::CtrlW);
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), Some("cargo test".to_owned()));
}

#[test]
fn ctrl_p_and_ctrl_n_walk_the_prompt_history() {
    let (mut state, mut first) = state_with_prompt();
    for ch in "first".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert_eq!(first.try_recv().unwrap(), Some("first".to_owned()));

    let (tx, mut second) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    for ch in "second".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert_eq!(second.try_recv().unwrap(), Some("second".to_owned()));

    let (tx, mut third) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    state.key(Key::CtrlP); // newest: second
    state.key(Key::CtrlP); // older: first
    state.key(Key::CtrlP); // already oldest: stays
    state.key(Key::CtrlN); // back to second
    state.key(Key::Enter);
    assert_eq!(third.try_recv().unwrap(), Some("second".to_owned()));
}

#[test]
fn up_and_down_are_history_too_and_a_fresh_line_is_restored() {
    let (mut state, mut first) = state_with_prompt();
    for ch in "kept".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert_eq!(first.try_recv().unwrap(), Some("kept".to_owned()));

    let (tx, mut second) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    for ch in "draft".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Up); // browse: kept
    state.key(Key::Down); // back to the draft
    state.key(Key::Enter);
    assert_eq!(second.try_recv().unwrap(), Some("draft".to_owned()));
}

#[test]
fn the_cursor_column_follows_the_cursor_not_the_end_of_the_line() {
    let (mut state, _answer) = state_with_prompt();
    for ch in "abc".chars() {
        state.key(Key::Char(ch));
    }
    assert_eq!(state.cursor_column(80), 5, "> abc");
    state.key(Key::Home);
    assert_eq!(state.cursor_column(80), 2, "> |abc");
    state.key(Key::Right);
    assert_eq!(state.cursor_column(80), 3, "> a|bc");
}

#[test]
fn a_long_line_scrolls_so_the_cursor_stays_on_screen() {
    let (mut state, _answer) = state_with_prompt();
    for ch in "x".repeat(200).chars() {
        state.key(Key::Char(ch));
    }
    let width = 40;
    let column = state.cursor_column(width);
    assert!(
        column < width,
        "the cursor stays inside the terminal: {column}"
    );
    state.key(Key::Home);
    assert_eq!(state.cursor_column(width), 2, "home brings the head back");
}

#[test]
fn a_users_message_keeps_its_lines_and_its_length() {
    // The transcript is for reading back what was asked. A pasted multi-line prompt
    // has to survive whole: it used to be squashed onto one line and cut at 500
    // characters (spec §3).
    let long = "x".repeat(600);
    let text = format!("第一行\n{long}\n第三行");
    let lines = render_block(&Block::Message {
        speaker: SpeakerId::User,
        role: Role::User,
        text,
    });

    let rendered: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect();
    assert_eq!(rendered.len(), 3, "one row per line of the message");
    assert!(rendered[0].ends_with("第一行"), "{:?}", rendered[0]);
    assert!(rendered[1].contains(&long), "nothing is elided");
    assert!(rendered[2].ends_with("第三行"), "{:?}", rendered[2]);

    // The continuations line up under the body of the first line, not under the
    // attribution.
    let indent = rendered[0].chars().count() - "第一行".chars().count();
    for continuation in &rendered[1..] {
        assert!(
            continuation.starts_with(&" ".repeat(indent)),
            "lined up under the body: {continuation:?}"
        );
    }
}
