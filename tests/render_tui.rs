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
    pane, render_block_uncoloured, AnswerChoice, AskRequest, Block, ConsoleRequest, DeltaKind,
    FrontEndEvent, Key, Question, RenderEvent, SessionFacts, ToolBlock, ToolOutcome, Transcript,
    TuiState,
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
        speaker_order: Vec::new(),
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

/// A state the loop reports as **inside a run**.
///
/// This is the only way to be inside one: the front end never infers it, so a test that
/// wants the cancel gesture has to say so the way the loop does.
fn state_running() -> TuiState {
    let mut state = new_state();
    state.request(ConsoleRequest::RunState { running: true });
    state
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
fn an_empty_submission_is_an_empty_line_not_the_end_of_input() {
    // `None` on the prompt channel means a closed stdin, and the loop stops on it — so
    // Enter on an empty draft must not send it. It used to, and pressing Enter at an
    // empty prompt quit the session.
    let (mut state, mut answer) = state_with_prompt();
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), Some(String::new()));
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
    // A question is asked *outside* a run (a front end sitting on a question the loop
    // raised on its own): `Esc` there is the non-acting answer, never an approval.
    let (mut state, _line) = state_with_prompt();
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
    // "Working" is the loop's own report, not something the stream implies: a delta says
    // a call *started*, which is not the same as running — the synthesizer's single call
    // is one of these.
    let mut state = state_running();
    state.key(Key::Esc);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
}

#[test]
fn ctrl_c_quits_before_the_loop_has_asked_for_its_first_line() {
    // The bug this pins: `busy` was `prompt_reply.is_none()`, and that is true from the
    // first frame until the loop asks its first line — all of assembly, which the TUI
    // spends in raw mode reading keys. `Ctrl-C` there became a cancel gesture, and the
    // idle loop discards those, so the key did nothing at all. A pty probe showed it:
    // one `Ctrl-C` ~20ms in left the process running for as long as it was watched.
    let mut fresh = new_state();
    fresh.key(Key::CtrlC);
    assert!(fresh.should_quit(), "a keyboard nobody is reading quits");
    assert!(fresh.take_events().is_empty());
}

#[test]
fn an_idle_ctrl_c_quits_and_a_working_one_cancels() {
    // Idle means the loop is waiting for a line, and it says so twice over: it has asked
    // for one, and it has reported that nothing is running (spec §6).
    let (mut idle, _line) = state_with_prompt();
    idle.key(Key::CtrlC);
    assert!(idle.should_quit());

    // A run in flight is the other way round: the same key is the cancel gesture.
    let mut state = state_running();
    state.key(Key::CtrlC);
    assert!(!state.should_quit());
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
}

#[test]
fn a_run_that_never_ended_a_turn_still_leaves_ctrl_c_quitting() {
    // The bug this pins: `busy` used to be inferred from render events, and only
    // `TurnEnded` ever cleared it. The synthesizer's single call — the closing call of a
    // discussion — streams deltas and ends **no** turn, so after a discussion the TUI
    // went on believing it was working, `Ctrl-C` became a cancel gesture, and the idle
    // loop ignores those: the session could not be quit out of.
    //
    // The stream below is the same one that used to strand the front end; what differs is
    // that the loop reports the end of the run itself, so no turn has to end for the
    // keyboard to come back.
    let mut state = state_running();
    state.apply(RenderEvent::Delta {
        speaker: kimi(),
        kind: DeltaKind::Text,
        text: "synthesizing".to_owned(),
    });
    // The discussion is over and the loop is waiting for the next line.
    state.request(ConsoleRequest::RunState { running: false });

    state.key(Key::CtrlC);
    assert!(
        state.should_quit(),
        "an idle Ctrl-C quits even when no `TurnEnded` ever arrived"
    );
    assert!(state.take_events().is_empty());
}

#[test]
fn shift_tab_is_the_plan_gesture() {
    let mut state = new_state();
    state.key(Key::BackTab);
    assert_eq!(state.take_events(), vec![FrontEndEvent::TogglePlan]);
}

#[test]
fn ctrl_d_asks_before_it_quits_and_the_safe_answer_is_no() {
    // Idle: `Ctrl-D` opens the exit confirmation rather than quitting. The overlay is
    // answered like the renderer's other own questions — `y` means yes, and every
    // other reachable key (and `Esc`) means no, because a hand reaches for `Enter`
    // without reading (票 06 §1, §2).
    let (mut idle, _line) = state_with_prompt();
    idle.key(Key::CtrlD);
    assert!(
        !idle.should_quit(),
        "the confirmation comes before the quit"
    );
    idle.key(Key::Enter);
    assert!(
        !idle.should_quit(),
        "Enter is the safe answer, not the exit"
    );

    // `Esc` closes the confirmation and leaves the session running.
    let (mut escaped, _line) = state_with_prompt();
    escaped.key(Key::CtrlD);
    escaped.key(Key::Esc);
    assert!(!escaped.should_quit());

    // `y` is the one key that quits.
    let (mut confirmed, _line) = state_with_prompt();
    confirmed.key(Key::CtrlD);
    confirmed.key(Key::Char('y'));
    assert!(confirmed.should_quit(), "y confirms the exit");
}

#[test]
fn ctrl_d_is_ignored_while_a_run_is_in_flight() {
    // Busy: the gesture does nothing at all — no confirmation, no quit, no cancel
    // (票 06 §1, §3). `Ctrl-C` remains the way to stop a run.
    let mut state = state_running();
    state.key(Key::CtrlD);
    assert!(!state.should_quit());
    assert!(state.take_events().is_empty(), "no cancel gesture either");
}

#[test]
fn ctrl_d_is_ignored_while_a_question_is_up() {
    // A question owns the keyboard, so `Ctrl-D` is that question's to ignore. The
    // permission overlay below is the loop's, and the answer it is waiting for is
    // unaffected by the stray key.
    let (mut state, _line) = state_with_prompt();
    let (reply, mut answer) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        question: Question::Permission(PermissionRequest {
            request_id: "r-1".to_owned(),
            tool_call_id: "c-1".to_owned(),
            tool_name: "bash".to_owned(),
            args: serde_json::json!({"command": "ls"}),
            reason: "mode ask".to_owned(),
        }),
        reply,
    }));
    state.key(Key::CtrlD);
    assert!(!state.should_quit(), "the key is ignored, not acted on");
    state.key(Key::Char('y'));
    assert_eq!(
        answer.try_recv().expect("the permission answer went out"),
        AnswerChoice::Permission(Answer::Allow),
        "and the question it was ignored for is still answerable"
    );
}

#[test]
fn a_tool_call_is_painted_by_its_result_and_annotated_by_its_hook() {
    // The merger is the transcript's. The **result** is what ends the call and what
    // paints it, so the call line is ready the moment the tool finishes; the
    // post-hook, which carries no `tool_call_id`, follows as its own block aimed at
    // the call just painted. Holding the call open for the hook instead hid it for
    // the whole run (票 02 §3).
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

    assert!(
        transcript.push(RenderEvent::Logged(call)).is_empty(),
        "a call in flight is not painted yet"
    );
    let ready = transcript.push(RenderEvent::Logged(result));
    let tool = ready
        .iter()
        .find_map(|block| match block {
            Block::Tool(tool) => Some(tool.as_ref()),
            _ => None,
        })
        .expect("the result paints the call");
    assert_eq!(tool.tool, "read_file");
    assert!(tool.outcome.as_ref().unwrap().ok);

    let feedback = transcript.push(RenderEvent::Logged(hook));
    assert_eq!(
        feedback.len(),
        1,
        "the hook is its own block: {feedback:#?}"
    );
    assert!(
        matches!(
            &feedback[0],
            Block::ToolFeedback { outcome, .. } if outcome == &hook_format::feedback("ok")
        ),
        "and it carries the feedback it annotates the call with: {feedback:#?}"
    );
}

#[test]
fn a_completed_turn_and_an_aborted_one_render_in_different_colors() {
    let good = render_block_uncoloured(&Block::TurnEnded {
        speaker: kimi(),
        reason: StopReason::Completed,
    });
    let aborted = render_block_uncoloured(&Block::TurnEnded {
        speaker: kimi(),
        reason: StopReason::Aborted,
    });
    let bad = render_block_uncoloured(&Block::TurnEnded {
        speaker: kimi(),
        reason: StopReason::Error,
    });
    // The line is `[name] text` in two spans: the name carries the speaker colour and
    // the body carries the severity, so the severity is asserted on the second span.
    for (line, color) in [
        (&good[0], Color::Green),
        (&aborted[0], Color::Yellow),
        (&bad[0], Color::Red),
    ] {
        assert_eq!(
            line.spans[1].style.fg,
            Some(color),
            "the body keeps the severity: {line:?}"
        );
    }
    // With no roster every name is the narration grey, never a severity colour.
    assert_eq!(good[0].spans[0].style.fg, Some(Color::DarkGray));
}

#[test]
fn a_speakers_name_is_drawn_in_its_role_colour() {
    use fs_agent::render::{render_block, SpeakerColors};

    let name = |blocks: Vec<Block>, roster: &[&str]| {
        let roster: Vec<String> = roster.iter().map(|name| (*name).to_owned()).collect();
        let mut colors = SpeakerColors::new(&roster);
        let lines: Vec<ratatui::text::Line<'static>> = blocks
            .iter()
            .flat_map(|block| render_block(block, &mut colors))
            .collect();
        lines
            .first()
            .map(|line| line.spans[0].style.fg)
            .expect("a line")
    };

    // The two debaters take the palette in roster order, and the caller's order is
    // the colour: the first is cyan, the second magenta (票 07 §1).
    assert_eq!(
        name(
            vec![Block::TurnStarted {
                speaker: SpeakerId::Debater("kimi".into()),
                iteration: 1,
            }],
            &["kimi", "claude"],
        ),
        Some(Color::LightCyan)
    );
    assert_eq!(
        name(
            vec![Block::TurnStarted {
                speaker: SpeakerId::Debater("claude".into()),
                iteration: 1,
            }],
            &["kimi", "claude"],
        ),
        Some(Color::LightMagenta)
    );

    // An executor, the user and the system have fixed roles, whatever the roster.
    assert_eq!(
        name(
            vec![Block::TurnStarted {
                speaker: SpeakerId::Executor("worker".into()),
                iteration: 1,
            }],
            &["kimi", "claude"],
        ),
        Some(Color::LightYellow)
    );
    assert_eq!(
        name(
            vec![Block::Message {
                speaker: SpeakerId::User,
                role: Role::User,
                text: "hello".to_owned(),
                reasoning: None,
            }],
            &["kimi", "claude"],
        ),
        Some(Color::LightGreen)
    );
    assert_eq!(
        name(vec![Block::Notice(String::new())], &["kimi", "claude"]),
        Some(Color::DarkGray),
        "a line with no speaker keeps the narration grey"
    );

    // A debater the roster does not name — the mid-session `/discuss` case — takes the
    // first unclaimed slot, and every later line for that name keeps it.
    let mut colors = SpeakerColors::new(&["kimi".to_owned()]);
    let first = render_block(
        &Block::TurnStarted {
            speaker: SpeakerId::Debater("newcomer".into()),
            iteration: 1,
        },
        &mut colors,
    );
    let second = render_block(
        &Block::TurnStarted {
            speaker: SpeakerId::Debater("newcomer".into()),
            iteration: 2,
        },
        &mut colors,
    );
    assert_eq!(first[0].spans[0].style.fg, Some(Color::LightMagenta));
    assert_eq!(
        second[0].spans[0].style.fg,
        Some(Color::LightMagenta),
        "and the colour does not move under the reader"
    );
}

#[test]
fn a_tool_block_paints_one_line_and_folds_the_rest() {
    // The collapsed tool block is the call line alone. The output body is not painted
    // at all any more — it lives behind the call line's detail view — and a failure is
    // a suffix on that same line rather than a line of its own (票 02 §3).
    let tool = |outcome: Option<ToolOutcome>| ToolBlock {
        speaker: kimi(),
        tool_call_id: ToolCallId::new("call-2"),
        tool: "read_file".to_owned(),
        args: serde_json::json!({"path": "a.rs"}),
        outcome,
    };
    let ok = render_block_uncoloured(&Block::Tool(Box::new(tool(Some(ToolOutcome {
        ok: true,
        output: Some("fn main() {}".to_owned()),
        error: None,
        duration_ms: 1,
    })))));
    assert_eq!(ok.len(), 1, "a successful call is one line: {ok:#?}");
    let call: String = ok[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(call, "[kimi] ▸ 调用 read_file path=a.rs");
    assert!(
        !ok[0].spans.iter().any(|span| span.style.bg.is_some()),
        "and carries no output body: {:#?}",
        ok[0]
    );

    // A failure is the same line with `失败` at its end; the error body is not here.
    let failed = render_block_uncoloured(&Block::Tool(Box::new(tool(Some(ToolOutcome {
        ok: false,
        output: None,
        error: Some("no such file".to_owned()),
        duration_ms: 1,
    })))));
    assert_eq!(
        failed.len(),
        1,
        "a failed call is one line too: {failed:#?}"
    );
    let call: String = failed[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(call, "[kimi] ▸ 调用 read_file path=a.rs 失败");

    // The post-hook's feedback is policy, not output: it stays on screen, as its own
    // block about the call just painted.
    let hooked = render_block_uncoloured(&Block::ToolFeedback {
        outcome: "formatted with rustfmt".to_owned(),
    });
    assert_eq!(hooked.len(), 1, "one feedback line: {hooked:#?}");
    assert!(
        hooked[0]
            .spans
            .iter()
            .any(|span| span.content.contains("formatted with rustfmt")),
        "and it carries the hook's words: {hooked:#?}"
    );
}

#[test]
fn the_synthesizers_product_renders_with_the_system_speaker() {
    let lines = render_block_uncoloured(&Block::Message {
        speaker: SpeakerId::System,
        role: Role::Assistant,
        text: "consensus".to_owned(),
        reasoning: None,
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
    let lines = render_block_uncoloured(&Block::Notice(banner.to_owned()));
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
    let lines = render_block_uncoloured(&Block::Message {
        speaker: kimi(),
        role: Role::Assistant,
        text: "# 标题\n\n- 一\n- 二\n".to_owned(),
        reasoning: None,
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
    let verdict = render_block_uncoloured(&Block::PermissionDecided {
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

    let asked = render_block_uncoloured(&Block::PermissionAsked {
        speaker: kimi(),
        tool_name: Some("bash".to_owned()),
        args: serde_json::json!({"command": "ls"}),
    });
    assert_eq!(asked[0].spans[0].style.fg, Some(Color::DarkGray));

    let answer = render_block_uncoloured(&Block::Message {
        speaker: kimi(),
        role: Role::Assistant,
        text: "正文".to_owned(),
        reasoning: None,
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
    let lines = render_block_uncoloured(&Block::Message {
        speaker: SpeakerId::User,
        role: Role::Assistant,
        text: "one\ntwo".to_owned(),
        reasoning: None,
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
fn the_arrows_move_the_cursor_while_ctrl_p_and_ctrl_n_walk_the_history() {
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
    // The arrows are the cursor's now: `Up` here must not swap the draft for the
    // last submission — so what gets submitted still starts with what was typed.
    state.key(Key::Up);
    state.key(Key::Char('!'));
    state.key(Key::Enter);
    assert_eq!(second.try_recv().unwrap(), Some("draft!".to_owned()));

    let (tx, mut third) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    state.key(Key::CtrlP); // the newest submission
    state.key(Key::CtrlN); // and back to the fresh draft, which is empty
    state.key(Key::Char('x'));
    state.key(Key::Enter);
    assert_eq!(third.try_recv().unwrap(), Some("x".to_owned()));
}

#[test]
fn a_users_message_keeps_its_lines_and_its_length() {
    // The transcript is for reading back what was asked. A pasted multi-line prompt
    // has to survive whole: it used to be squashed onto one line and cut at 500
    // characters (spec §3).
    let long = "x".repeat(600);
    let text = format!("第一行\n{long}\n第三行");
    let lines = render_block_uncoloured(&Block::Message {
        speaker: SpeakerId::User,
        role: Role::User,
        text,
        reasoning: None,
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

#[test]
fn a_paste_never_submits_and_its_line_endings_are_normalised() {
    let (mut state, mut answer) = state_with_prompt();
    // CRLF, a bare CR and a stray control character: all of it arrives as text, and
    // none of it presses Enter (spec §7).
    state.paste("第一行\r\n第二行\r第三行\x07");
    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().unwrap(),
        Some("第一行\n第二行\n第三行".to_owned())
    );
}

#[test]
fn an_oversized_paste_asks_first_and_only_yes_takes_it() {
    let (mut state, mut answer) = state_with_prompt();
    // Declining leaves an empty draft, and pressing Enter on one submits an empty
    // line — never the end of input. What arrives here is that empty line; that it
    // starts no turn is the loop's call (`cli.rs` trims it and asks again).
    let empty = Some(String::new());
    let huge = "x".repeat(100_001);

    state.paste(&huge);
    state.key(Key::Char('n'));
    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().unwrap(),
        empty,
        "declined: the text is gone"
    );

    let (tx, mut second) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    state.paste(&huge);
    state.key(Key::Esc); // the safe answer is no, so Esc declines too
    state.key(Key::Enter);
    assert_eq!(second.try_recv().unwrap(), empty, "Esc is not consent");

    let (tx, mut enter) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    state.paste(&huge);
    state.key(Key::Enter); // the other safe answer: Enter declines as well
    state.key(Key::Enter); // and this one submits, because the question is gone
    assert_eq!(
        enter.try_recv().unwrap(),
        empty,
        "the huge paste was refused"
    );

    let (tx, mut third) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    state.paste(&huge);
    state.key(Key::Char('y'));
    state.key(Key::Enter);
    let pasted = third.try_recv().unwrap().expect("the paste was taken");
    assert_eq!(pasted.chars().count(), 100_001, "all of it, in one piece");
}

#[test]
fn a_paste_is_ignored_while_a_question_is_up() {
    let (mut state, mut answer) = state_with_prompt();
    let huge = "x".repeat(100_001);
    // A paste must not answer a question, nor overwrite one: the oversized paste
    // asks, and a second paste while it asks changes nothing (spec §9, §7).
    state.paste(&huge);
    // A second paste while the question is up must change nothing: not the question,
    // and not the draft behind it.
    state.paste("small");
    state.key(Key::Char('y'));
    state.key(Key::Enter);
    let pasted = answer
        .try_recv()
        .unwrap()
        .expect("the question survived the second paste");
    assert_eq!(pasted.chars().count(), 100_001, "and so did its text");
    assert!(
        !pasted.contains("small"),
        "the small paste did not reach the draft"
    );
}

#[test]
fn a_tab_in_a_paste_becomes_spaces_rather_than_breaking_the_column_count() {
    let (mut state, mut answer) = state_with_prompt();
    state.paste("fn main() {\n\tbody\n}");
    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().unwrap(),
        Some("fn main() {\n    body\n}".to_owned())
    );
}

#[test]
fn esc_asks_before_it_throws_away_a_multi_line_draft() {
    let (mut state, mut answer) = state_with_prompt();
    state.paste("第一行\n第二行");
    state.key(Key::Esc);
    state.key(Key::Char('n'));
    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().unwrap(),
        Some("第一行\n第二行".to_owned()),
        "the draft survived"
    );

    // A single line still clears on the spot: there is nothing to lose, and the
    // Enter that follows submits the empty draft it left behind — a line the loop
    // discards, not a closed input.
    let (tx, mut second) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    for ch in "one line".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Esc);
    state.key(Key::Enter);
    assert_eq!(
        second.try_recv().unwrap(),
        Some(String::new()),
        "cleared without asking"
    );
}

#[test]
fn submitting_trims_the_ends_and_keeps_the_lines_between_them() {
    let (mut state, mut answer) = state_with_prompt();
    state.paste("\n  第一行\n\n第二行  \n");
    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().unwrap(),
        Some("第一行\n\n第二行".to_owned())
    );
}

#[test]
fn escape_while_working_is_the_cancel_gesture_even_with_a_question_up() {
    // The keymap's order is the spec's: a turn in flight makes `Esc` the cancel
    // gesture, and a pending question does not change that (spec §6). The question
    // waits for one of its own keys — `Ctrl-C` is the other way out.
    let mut state = state_running();
    let (tx, mut asked) = tokio::sync::oneshot::channel();
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

    state.key(Key::Esc);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
    assert!(
        asked.try_recv().is_err(),
        "the question is still waiting for its own keys"
    );
}

#[test]
fn a_cancelled_run_takes_its_unanswered_question_with_it() {
    // The bug this pins: while a run is in flight `Esc` is the cancel gesture, not an
    // answer, so a question the run had open is never answered — and the loop's ask dies
    // with the run that raised it. The front end kept the overlay up regardless, so the
    // next keypress went to a question **nobody was waiting for**: it failed silently and
    // read as a dead key (spec §6, §9).
    let mut state = state_running();
    let (tx, mut asked) = tokio::sync::oneshot::channel();
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

    // The gesture cancels the run; it does not answer the question.
    state.key(Key::Esc);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
    assert!(asked.try_recv().is_err(), "the question was not answered");

    // The run ends, and the loop says so — which is what makes that question stale.
    state.request(ConsoleRequest::RunState { running: false });

    // So the next key is an ordinary key again: it edits the draft rather than answering
    // a question from a run that is over.
    let (reply, mut line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    state.key(Key::Char('x'));
    state.key(Key::Enter);
    assert_eq!(line.try_recv().unwrap(), Some("x".to_owned()));
    assert!(
        asked.try_recv().is_err(),
        "nothing answers a question whose run is over"
    );
}

#[test]
fn a_second_question_does_not_displace_the_one_on_screen() {
    // The loop asks one question at a time and waits, so a second ask means something
    // went wrong; keeping the question the user can see answerable is the safe reading.
    let mut state = new_state();
    let (first_tx, mut first) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        question: Question::PlanConflict("PLAN.md".into()),
        reply: first_tx,
    }));
    let (second_tx, mut second) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        question: Question::PlanConflict("other.md".into()),
        reply: second_tx,
    }));
    assert!(second.try_recv().is_err(), "the late question was dropped");

    state.key(Key::Char('k'));
    assert_eq!(
        first.try_recv().unwrap(),
        AnswerChoice::Plan(fs_agent::permissions::PlanConflict::Keep)
    );
}
