//! The `ask_user_question` takeover of the TUI's bottom input area (spec §7, §19).
//!
//! The seam is the same one `render_layout.rs` tests through: a `TuiState` goes
//! into [`draw_frame`] and a fixed-size `TestBackend` buffer comes out, so every
//! assertion is about what a person would see. The keyboard assertions are about
//! the answer the loop would receive, which is the other half of the takeover.
//!
//! This lives in its own file rather than in `render_layout.rs` so the ticket that
//! owns that file can keep editing it without colliding with this one.

use fs_agent::questions::{UserAnswer, UserAnswers, UserQuestion};
use fs_agent::render::{
    draw_frame, ConsoleRequest, FrontEndEvent, Key, QuestionnaireRequest, SessionFacts, TuiState,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::Terminal;
use tokio::sync::oneshot;

fn facts() -> SessionFacts {
    SessionFacts {
        session_id: "01J8ZQ4K7M".to_owned(),
        cwd: "~/code/fortystory/fs-agent".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        budget_limit: Some(100_000),
    }
}

fn state() -> TuiState {
    TuiState::new(facts())
}

/// One question, with options and the multi-select flag.
fn question(id: &str, text: &str, options: &[&str], multi_select: bool) -> UserQuestion {
    UserQuestion {
        id: id.to_owned(),
        question: text.to_owned(),
        header: None,
        options: options
            .iter()
            .map(|label| fs_agent::questions::Choice {
                label: (*label).to_owned(),
                description: None,
            })
            .collect(),
        multi_select,
    }
}

/// Put a questionnaire to the state, the way the loop would.
fn ask(
    state: &mut TuiState,
    questions: Vec<UserQuestion>,
) -> oneshot::Receiver<Result<UserAnswers, String>> {
    let (reply, answers) = oneshot::channel();
    state.request(ConsoleRequest::Questionnaire(QuestionnaireRequest {
        questions,
        reply,
    }));
    answers
}

/// The answer the state sent, if it sent one yet.
fn answer(rx: &mut oneshot::Receiver<Result<UserAnswers, String>>) -> Option<UserAnswers> {
    match rx.try_recv() {
        Ok(Ok(answers)) => Some(answers),
        Ok(Err(reason)) => panic!("the state refused to answer: {reason}"),
        Err(oneshot::error::TryRecvError::Empty) => None,
        Err(oneshot::error::TryRecvError::Closed) => {
            panic!("the state dropped the questionnaire without answering")
        }
    }
}

fn buffer(width: u16, height: u16, state: &mut TuiState) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("TestBackend");
    terminal
        .draw(|frame| draw_frame(frame, state))
        .expect("one frame");
    terminal.backend().buffer().clone()
}

fn cells(frame: &Buffer, y: u16, from: u16, to: u16) -> String {
    let mut text = String::new();
    let mut x = from;
    while x < to {
        let symbol = frame[(x, y)].symbol();
        text.push_str(symbol);
        x += symbol.cell_width().max(1);
    }
    text
}

fn screen(width: u16, height: u16, state: &mut TuiState) -> Vec<String> {
    let frame = buffer(width, height, state);
    (0..height).map(|y| cells(&frame, y, 0, width)).collect()
}

#[test]
fn the_question_and_its_options_take_over_the_bottom_input_area() {
    let mut state = state();
    let _rx = ask(
        &mut state,
        vec![question(
            "q",
            "Which framework?",
            &["serde", "manual"],
            false,
        )],
    );

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("Which framework?"), "{text}");
    assert!(
        text.contains("1. serde"),
        "the options are numbered: {text}"
    );
    assert!(text.contains("2. manual"), "{text}");

    // It is in the bottom block, not the middle overlay: the question's row sits
    // below the last middle-block border.
    let middle_bottom = rows
        .iter()
        .rposition(|row| row.starts_with('└'))
        .expect("the bottom block's bottom border");
    let question_row = rows
        .iter()
        .position(|row| row.contains("Which framework?"))
        .expect("the question is on screen");
    assert!(
        question_row < middle_bottom,
        "the question is drawn in the bottom block:\n{text}"
    );
    // And there is no middle-pane overlay: only three blocks open.
    let opens = rows.iter().filter(|row| row.starts_with('┌')).count();
    assert_eq!(opens, 3, "no fifth box floats over the transcript:\n{text}");
}

#[test]
fn a_single_select_choice_advances_and_the_footer_pages() {
    let mut state = state();
    let _rx = ask(
        &mut state,
        vec![
            question("one", "First?", &["a", "b"], false),
            question("two", "Second?", &["c", "d"], false),
            question("three", "Third?", &["e", "f"], false),
        ],
    );

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("1 / 3"), "the footer pages: {text}");

    // Confirming the highlighted option on a single-select question advances to
    // the next one.
    state.key(Key::Enter);
    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("Second?"), "{text}");
    assert!(text.contains("2 / 3"), "{text}");
    assert!(
        !text.contains("First?"),
        "one question is on screen at a time: {text}"
    );
}

#[test]
fn submit_is_refused_until_every_question_is_answered_or_skipped() {
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![
            question("one", "First?", &["a"], false),
            question("two", "Second?", &["b"], false),
            question("three", "Third?", &["c"], false),
        ],
    );

    // Confirming the first answer advances, but must not submit while the
    // questions after it are unhandled.
    state.key(Key::Enter);
    assert!(
        answer(&mut rx).is_none(),
        "an unhandled question refuses submit"
    );

    // Same for the second: the answer is made, submit is still a separate press.
    state.key(Key::Enter);
    assert!(
        answer(&mut rx).is_none(),
        "one question still unhandled refuses submit"
    );

    // Explicitly skipping the last is what completes the questionnaire; the
    // skip itself does not submit either.
    state.key(Key::Tab);
    assert!(
        answer(&mut rx).is_none(),
        "skipping the last question does not submit on that key"
    );

    state.key(Key::Enter);
    let answers = answer(&mut rx).expect("all questions handled submits");
    assert_eq!(
        answers.answers,
        vec![
            UserAnswer {
                id: "one".to_owned(),
                selected: vec!["a".to_owned()],
                custom: None,
            },
            UserAnswer {
                id: "two".to_owned(),
                selected: vec!["b".to_owned()],
                custom: None,
            },
            UserAnswer {
                id: "three".to_owned(),
                selected: Vec::new(),
                custom: None,
            },
        ]
    );
}

#[test]
fn a_skipped_question_is_no_answer_even_after_typing() {
    // Skip is an explicit "no answer", and it outranks text typed before the
    // skip: the answer is `selected: []` with no `custom` (spec §7). Typing and
    // then deciding not to answer must not smuggle the text into the result.
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );

    state.key(Key::Char('x'));
    state.key(Key::Tab);
    state.key(Key::Enter);
    let answers = answer(&mut rx).expect("the skip completes the questionnaire");
    assert_eq!(
        answers.answers,
        vec![UserAnswer {
            id: "one".to_owned(),
            selected: Vec::new(),
            custom: None,
        }]
    );
}

#[test]
fn typing_overrides_a_single_select_choice_and_supplements_a_multi_select_one() {
    // Single-select: custom text wins, so `selected` comes back empty (spec §7).
    let mut single = state();
    let mut single_rx = ask(
        &mut single,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );
    single.key(Key::Enter);
    let text = screen(120, 24, &mut single).join("\n");
    assert!(
        text.contains("● 1. a"),
        "the choice is shown as picked: {text}"
    );
    single.key(Key::Char('x'));
    let text = screen(120, 24, &mut single).join("\n");
    assert!(
        text.contains("○ 1. a"),
        "typing clears the single-select choice, because custom text overrides it: {text}"
    );
    assert!(text.contains("自定义：x"), "{text}");
    single.key(Key::Enter);
    assert_eq!(
        answer(&mut single_rx).expect("answered").answers,
        vec![UserAnswer {
            id: "one".to_owned(),
            selected: Vec::new(),
            custom: Some("x".to_owned()),
        }]
    );

    // Multi-select: the choice stays, the custom text comes along with it.
    let mut multi = state();
    let mut multi_rx = ask(
        &mut multi,
        vec![question("one", "Pick?", &["a", "b"], true)],
    );
    multi.key(Key::Enter);
    multi.key(Key::Char('x'));
    let text = screen(120, 24, &mut multi).join("\n");
    assert!(
        text.contains("[x] 1. a"),
        "typing leaves a multi-select choice alone, because custom text supplements it: {text}"
    );
    multi.key(Key::Enter);
    assert_eq!(
        answer(&mut multi_rx).expect("answered").answers,
        vec![UserAnswer {
            id: "one".to_owned(),
            selected: vec!["a".to_owned()],
            custom: Some("x".to_owned()),
        }]
    );
}

#[test]
fn enter_keeps_typed_text_instead_of_re_confirming_an_option() {
    // `Enter` on an already-typed answer moves on; it does not confirm the
    // highlight, or typed free text would be silently replaced by an option
    // (spec §7).
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![
            question("one", "First?", &["a", "b"], false),
            question("two", "Second?", &["c"], false),
        ],
    );
    for ch in "typed".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("Second?"),
        "Enter moves to the next question:\n{text}"
    );

    state.key(Key::Enter);
    state.key(Key::Enter);
    assert_eq!(
        answer(&mut rx).expect("answered").answers,
        vec![
            UserAnswer {
                id: "one".to_owned(),
                selected: Vec::new(),
                custom: Some("typed".to_owned()),
            },
            UserAnswer {
                id: "two".to_owned(),
                selected: vec!["c".to_owned()],
                custom: None,
            },
        ]
    );
}

#[test]
fn the_option_window_scrolls_so_the_highlighted_option_stays_visible() {
    // The bottom block is capped (layout `MAX_INPUT_ROWS`), so a question with
    // more options than fit must scroll its option window: the header and the
    // question stay put, and the highlighted option is always on screen
    // (spec §7).
    let labels: Vec<String> = (1..=20).map(|n| format!("opt-{n:02}")).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut state = state();
    let mut rx = ask(&mut state, vec![question("many", "Which?", &refs, false)]);

    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("1. opt-01"),
        "the window starts at the top:\n{text}"
    );

    for _ in 0..15 {
        state.key(Key::Down);
    }
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("16. opt-16"),
        "the highlighted option scrolled into view:\n{text}"
    );
    assert!(
        text.contains("Which?"),
        "the question stays pinned above the window:\n{text}"
    );

    // The tenth option onward is reachable too: the highlight is not a digit.
    state.key(Key::Enter);
    state.key(Key::Enter);
    assert_eq!(
        answer(&mut rx).expect("answered").answers,
        vec![UserAnswer {
            id: "many".to_owned(),
            selected: vec!["opt-16".to_owned()],
            custom: None,
        }]
    );
}

#[test]
fn digits_are_free_text_not_selection_keys() {
    // The decided keyboard has no digit selection, so a digit is an ordinary
    // character even on a question that offers options — which also means no
    // digit can be swallowed on a question that offers none (spec §7).
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("q", "How should it be named?", &["a", "b"], false)],
    );
    for ch in "v2".chars() {
        state.key(Key::Char(ch));
    }
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("自定义：v2"),
        "the digit lands in the free-text field:\n{text}"
    );
    state.key(Key::Enter);
    assert_eq!(
        answer(&mut rx).expect("answered").answers,
        vec![UserAnswer {
            id: "q".to_owned(),
            selected: Vec::new(),
            custom: Some("v2".to_owned()),
        }]
    );
}

#[test]
fn the_recommended_marker_is_display_only() {
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question(
            "one",
            "Pick?",
            &["serde (Recommended)", "manual"],
            false,
        )],
    );

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(
        text.contains("推荐"),
        "the marker is shown as a display badge: {text}"
    );
    assert!(
        !text.contains("(Recommended)"),
        "the raw marker is not what the option reads as: {text}"
    );

    // The answer keeps the original string, marker included.
    state.key(Key::Enter);
    state.key(Key::Enter);
    assert_eq!(
        answer(&mut rx).expect("answered").answers,
        vec![UserAnswer {
            id: "one".to_owned(),
            selected: vec!["serde (Recommended)".to_owned()],
            custom: None,
        }]
    );
}

#[test]
fn answering_hands_the_bottom_back_to_the_resident_input() {
    let mut state = state();
    let mut rx = ask(&mut state, vec![question("one", "Pick?", &["a"], false)]);
    state.key(Key::Enter);
    state.key(Key::Enter);
    assert!(answer(&mut rx).is_some());

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(!text.contains("Pick?"), "the takeover is gone:\n{text}");
    assert!(
        text.contains("esc 取消"),
        "the resident input's hints are back:\n{text}"
    );
}

#[test]
fn a_question_with_no_options_is_answered_with_free_text() {
    // The wire allows a question with no options: the answer is then the text the
    // user typed, and an empty one is not an answer (spec §7).
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("q", "How should it be named?", &[], false)],
    );

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("回答："), "a free-text line is shown: {text}");

    state.key(Key::Enter);
    assert!(
        answer(&mut rx).is_none(),
        "an empty free-text question is not answered"
    );
    // A digit is ordinary text: the keyboard has no digit selection, and there
    // is no option it could number here either.
    for ch in "v2-name".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert_eq!(
        answer(&mut rx).expect("answered").answers,
        vec![UserAnswer {
            id: "q".to_owned(),
            selected: Vec::new(),
            custom: Some("v2-name".to_owned()),
        }]
    );
}

#[test]
fn an_empty_questionnaire_is_refused_rather_than_panicking() {
    // The tool refuses an empty list before it reaches a port, so this is only
    // defense: a questionnaire with nothing to draw must not index into nothing.
    let mut state = state();
    let mut rx = ask(&mut state, Vec::new());
    match rx.try_recv() {
        Ok(Err(reason)) => assert!(reason.contains("at least one question"), "{reason}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn escape_still_cancels_the_run_and_never_answers_the_questionnaire() {
    // `Esc` keeps its meaning (spec §6, §19): it does not abandon the question,
    // it cancels the run, and the cancelled call gets its one result elsewhere.
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    let mut rx = ask(&mut state, vec![question("one", "Pick?", &["a"], false)]);

    state.key(Key::Esc);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
    assert!(
        answer(&mut rx).is_none(),
        "Esc cancels the run, it does not answer"
    );

    // The run's end withdraws the questionnaire, reading as "no answer".
    state.request(ConsoleRequest::RunState { running: false });
    let rows = screen(120, 24, &mut state);
    assert!(
        !rows.join("\n").contains("Pick?"),
        "the takeover goes with the run"
    );
    assert!(
        rx.try_recv().is_err(),
        "the sender was dropped, not answered"
    );
}
