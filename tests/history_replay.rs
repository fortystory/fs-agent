//! Reopening a session: the history laid back into the transcript, and the seam
//! between it and what this run adds (`.scratch/tui-history-replay/spec.md`).
//!
//! The seam is the same one `render_layout.rs` and `ask_user_question_tui.rs` test
//! through: a `TuiState` takes render events and front-end requests, a fixed-size
//! `TestBackend` comes back out, and the keyboard assertions are about what the loop
//! would receive. The replay is driven through the production seam — the
//! `ConsoleRequest::Replay` the CLI pushes after assembly — and advanced with the
//! same `replay_batch` the loop calls, so "half way through" is a state a test can
//! stand in rather than something it has to infer.

use std::path::Path;

use fs_agent::events::{
    ContextSource, Event, EventPayload, HistoryReason, Role, SessionId, SpeakerId, StopReason,
    ToolCallId, Usage,
};
use fs_agent::render::{
    draw_frame, wording, ConsoleRequest, Key, RenderEvent, SessionFacts, TuiState,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::Terminal;

fn facts() -> SessionFacts {
    SessionFacts {
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: "~/code/fortystory/fs-agent".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        budget_limit: Some(100_000),
        speaker_order: vec!["kimi".to_owned()],
    }
}

fn state() -> TuiState {
    TuiState::new(facts())
}

/// A state whose session directory is `dir`, which is what the tool detail reads
/// `outputs/<id>.txt` out of.
fn state_in(dir: &Path) -> TuiState {
    TuiState::new(SessionFacts {
        session_dir: dir.display().to_string(),
        ..facts()
    })
}

/// A state whose loop is waiting for a line, so `Enter` has somewhere to submit.
fn idle() -> (TuiState, tokio::sync::oneshot::Receiver<Option<String>>) {
    let mut state = state();
    let (reply, line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    (state, line)
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn event(seq: u64, payload: EventPayload) -> Event {
    Event::new(seq, SpeakerId::Debater("kimi".into()), payload)
}

fn session_started(seq: u64) -> Event {
    event(
        seq,
        EventPayload::SessionStarted {
            session_id: SessionId::new("01J8ZQ4K7M"),
            cwd: "/workspace".to_owned(),
            schema_version: 1,
        },
    )
}

fn message(seq: u64, text: &str, reasoning: Option<&str>) -> Event {
    event(
        seq,
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: text.to_owned(),
            reasoning: reasoning.map(str::to_owned),
        },
    )
}

fn turn_started(seq: u64) -> Event {
    event(
        seq,
        EventPayload::TurnStarted {
            agent: SpeakerId::Debater("kimi".into()),
            iteration: 1,
        },
    )
}

fn turn_ended(seq: u64) -> Event {
    event(
        seq,
        EventPayload::TurnEnded {
            reason: StopReason::Completed,
        },
    )
}

fn usage(seq: u64, input: u64, output: u64, cached: u64, miss: u64) -> Event {
    event(
        seq,
        EventPayload::UsageRecorded {
            usage: Usage {
                input_tokens: input,
                output_tokens: output,
                cached_tokens: cached,
                miss_tokens: miss,
                reasoning_tokens: None,
            },
        },
    )
}

fn tool_started(seq: u64, id: &str, tool: &str, args: serde_json::Value) -> Event {
    event(
        seq,
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new(id),
            tool_name: tool.to_owned(),
            args,
        },
    )
}

fn tool_completed(
    seq: u64,
    id: &str,
    ok: bool,
    output: Option<&str>,
    error: Option<&str>,
) -> Event {
    event(
        seq,
        EventPayload::ToolCallCompleted {
            tool_call_id: ToolCallId::new(id),
            ok,
            output: output.map(str::to_owned),
            error: error.map(str::to_owned),
            duration_ms: 3,
        },
    )
}

fn plan_injected(seq: u64) -> Event {
    event(
        seq,
        EventPayload::ContextInjected {
            source: ContextSource::PlanMode,
            content: "PLAN.md".to_owned(),
        },
    )
}

fn mode_change(seq: u64) -> Event {
    event(
        seq,
        EventPayload::HistorySuperseded {
            targets: vec![1],
            reason: HistoryReason::ModeChange,
            summary: None,
        },
    )
}

// ---------------------------------------------------------------------------
// Driving the replay
// ---------------------------------------------------------------------------

/// Hand the state a history, the way the CLI does after assembly.
fn replay(state: &mut TuiState, events: Vec<Event>) {
    state.request(ConsoleRequest::Replay { events });
}

/// Run the replay out, one batch at a time, the way the loop does.
fn run_replay(state: &mut TuiState) {
    while state.replay_pending() {
        state.replay_batch();
    }
}

/// A history long enough to need more than one batch: each `TurnEnded` draws exactly
/// one line, so 600 of them cannot arrive in a single pass and the frame between two
/// batches is a state a test can stand in. The count is deliberately not a batch
/// boundary: nothing here asserts how many events a batch holds.
fn long_history() -> Vec<Event> {
    (1..=600).map(turn_ended).collect()
}

// ---------------------------------------------------------------------------
// The frame
// ---------------------------------------------------------------------------

/// Render one frame at a fixed size and read the screen back as rows of text.
fn screen(width: u16, height: u16, state: &mut TuiState) -> Vec<String> {
    let frame = buffer(width, height, state);
    (0..height).map(|y| row_text(&frame, y, width)).collect()
}

fn buffer(width: u16, height: u16, state: &mut TuiState) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("TestBackend");
    terminal
        .draw(|frame| draw_frame(frame, state))
        .expect("one frame");
    terminal.backend().buffer().clone()
}

/// The rail's column at 120x40: the characters drawn in it, blanks dropped.
///
/// The rail sits in the transcript's last column, inside the frame at 119 — the
/// scrollbar takes the one before it (`.scratch/tui-sidebar/spec.md` §1).
fn turn_rail_shape(state: &mut TuiState) -> String {
    let frame = buffer(120, 40, state);
    (1..39u16)
        // The transcript ends where the main column's first rule begins.
        .take_while(|y| !row_text(&frame, *y, 120).ends_with('┤'))
        .map(|y| frame[(118, y)].symbol().chars().next().unwrap_or(' '))
        .filter(|ch| *ch != ' ')
        .collect()
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

fn row_text(buffer: &Buffer, y: u16, width: u16) -> String {
    cells(buffer, y, 0, width)
}

fn row_of(state: &mut TuiState, width: u16, height: u16, needle: &str) -> Option<u16> {
    screen(width, height, state)
        .iter()
        .position(|row| row.contains(needle))
        .map(|row| row as u16)
}

fn click(column: u16, row: u16) -> ratatui::crossterm::event::MouseEvent {
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

fn click_row(state: &mut TuiState, width: u16, height: u16, needle: &str) {
    let Some(row) = row_of(state, width, height, needle) else {
        panic!("nothing on screen contains {needle:?}");
    };
    state.mouse(click(10, row));
}

/// A sidebar field by its offset from the page's first row.
///
/// The page starts under the tab bar — the row after its bottom rule — and the fields
/// are the sidebar's own half of those rows, so the divider's column ends each one.
fn panel_field(rows: &[String], offset: usize) -> String {
    let mut rules = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.starts_with('├'))
        .map(|(y, _)| y);
    rules.next().expect("the tab bar's top rule");
    let top = rules.next().expect("the tab bar's bottom rule") + 1;
    let row = &rows[top + offset];
    let inner = row.trim_start_matches('│');
    let end = inner
        .char_indices()
        .find(|(_, ch)| matches!(ch, '│' | '├' | '┤'))
        .map(|(index, _)| index)
        .expect("the divider ends the sidebar");
    inner[..end].to_owned()
}

// ---------------------------------------------------------------------------
// 票 06 — the seam, the framing and the progress line
// ---------------------------------------------------------------------------

#[test]
fn a_replayed_history_is_laid_into_the_transcript() {
    // The whole point: after `--continue` the transcript holds the previous
    // conversation rather than starting empty (spec §1, user story 1).
    let mut state = state();
    replay(
        &mut state,
        vec![message(1, "昨天说的那件事，结论是 42。", None)],
    );
    assert!(state.replay_pending(), "the request starts the replay");
    run_replay(&mut state);

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("昨天说的那件事，结论是 42。"),
        "the history is on screen: {text}"
    );
}

#[test]
fn an_empty_stream_never_enters_the_replay_state() {
    // There is nothing to lay down, so there is no progress line and no seam: an
    // empty `--continue` reads exactly as it always did (spec §2, user story 25).
    let mut state = state();
    replay(&mut state, Vec::new());
    assert!(!state.replay_pending(), "an empty history is not a replay");

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("恢复"),
        "and no progress line is drawn: {text}"
    );
}

#[test]
fn a_replay_in_flight_shows_partial_history_and_the_progress_count() {
    // The framing contract: the hint row says how far the replay has come, and the
    // part it has reached is already on screen; when it drains, the ordinary status
    // line comes back (spec §2, §4, user story 12). The count is asserted to be
    // **partial**, not to be any particular number: how many events a batch holds is
    // the implementation's business (spec §Testing Decisions).
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("恢复历史") && text.contains("/600") && !text.contains("恢复历史 600/600"),
        "the progress line shows how far it has come: {text}"
    );
    assert!(
        text.contains("回合结束"),
        "and the part of the history already applied is on screen: {text}"
    );

    run_replay(&mut state);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("恢复"),
        "the progress line is gone once the history is in: {text}"
    );
    assert!(
        text.contains("ctrl-c/ctrl-d 退出"),
        "and the ordinary status line is back: {text}"
    );
}

#[test]
fn a_history_that_ends_exactly_on_a_batch_boundary_still_converges() {
    // The off-by-one the boundary invites: a stream that runs out where a batch runs
    // out must close the replay rather than leave it pending for ever.
    let mut state = state();
    replay(&mut state, (1..=1024).map(turn_ended).collect());
    run_replay(&mut state);
    assert!(!state.replay_pending(), "the replay closed itself");
    let text = screen(120, 20, &mut state).join("\n");
    assert!(
        !text.contains("恢复"),
        "and left no progress line behind: {text}"
    );
}

#[test]
fn the_progress_line_degrades_at_the_minimum_frame() {
    // 40×10 is the smallest frame that draws at all; its hint row is 38 columns, and
    // that is where the count survives but the phrase does not (spec §4).
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();

    let text = screen(40, 10, &mut state).join("\n");
    assert!(
        text.contains("恢复中") && text.contains("/600") && !text.contains("恢复中 600/600"),
        "the count survives at the minimum frame: {text}"
    );
    assert!(
        !text.contains("恢复历史"),
        "the full phrase does not: {text}"
    );
}

#[test]
fn the_pointer_does_nothing_while_a_replay_is_in_flight() {
    // The viewport is pinned to the bottom until the history is done: a wheel notch
    // or a click would move it into rows that are still arriving (spec §5, user
    // story 16, 34).
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();

    let before = screen(120, 40, &mut state);
    state.mouse(ratatui::crossterm::event::MouseEvent {
        kind: ratatui::crossterm::event::MouseEventKind::ScrollUp,
        column: 40,
        row: 10,
        modifiers: ratatui::crossterm::event::KeyModifiers::empty(),
    });
    state.mouse(click(10, 5));
    let after = screen(120, 40, &mut state);
    assert_eq!(before, after, "the pointer changed nothing");
}

#[test]
fn enter_does_not_submit_while_a_replay_is_in_flight() {
    // Waiting time is typing time, but `Enter` must not fire a turn into a
    // half-laid history; the draft survives and a later `Enter` sends it (spec §3,
    // user story 15, 18).
    let (mut state, mut answer) = idle();
    replay(&mut state, long_history());
    state.replay_batch();

    for ch in "half typed".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert!(
        answer.try_recv().is_err(),
        "nothing was submitted while the history was still arriving"
    );

    run_replay(&mut state);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("half typed"),
        "and the draft is still there: {text}"
    );

    state.key(Key::Enter);
    assert_eq!(
        answer
            .try_recv()
            .expect("submitted once the replay was over"),
        Some("half typed".to_owned())
    );
}

#[test]
fn ctrl_c_quits_during_a_replay_and_ctrl_d_and_esc_are_inert() {
    // A replay is not a run: there is nothing to cancel, so `Ctrl-C` is the way out;
    // `Ctrl-D` and `Esc` do nothing at all (spec §5, user story 26, 27).
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();

    state.key(Key::CtrlD);
    assert!(!state.should_quit(), "Ctrl-D does not quit during a replay");
    state.key(Key::Esc);
    assert!(!state.should_quit(), "Esc does not quit during a replay");
    // Esc on a multi-line draft would ordinarily offer to clear it; during a replay
    // it must not raise the question at all.
    for ch in "one\ntwo".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Esc);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("清空"),
        "no draft-clearing question is raised: {text}"
    );

    state.key(Key::CtrlC);
    assert!(state.should_quit(), "Ctrl-C is the way out of a replay");
}

#[test]
fn the_scroll_keys_are_ignored_during_a_replay() {
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();

    let before = screen(120, 40, &mut state);
    for key in [Key::PageUp, Key::PageDown, Key::CtrlG] {
        state.key(key);
    }
    let after = screen(120, 40, &mut state);
    assert_eq!(before, after, "the transcript stays pinned to the bottom");
}

#[test]
fn a_live_event_arriving_mid_replay_waits_for_the_history() {
    // The banner is sent while the history is still being laid down; it must not
    // appear in the middle of it (spec §3, user story 19, 21).
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();
    state.live_event(RenderEvent::Notice("fs-agent 启动".to_owned()));

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("fs-agent 启动"),
        "the banner is held back while the history is arriving: {text}"
    );

    run_replay(&mut state);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("fs-agent 启动"),
        "and lands once the history is done: {text}"
    );
}

#[test]
fn a_logged_event_arriving_mid_replay_is_not_painted_twice() {
    // A reopened session's recovery writes its synthesized results to the log **and**
    // emits them on the render channel, and the replay's snapshot holds the same
    // events. The live copy must not paint a second call line after the seam.
    let mut events: Vec<Event> = (1..=600).map(turn_ended).collect();
    events.push(tool_started(
        601,
        "call-recovered",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    events.push(tool_completed(
        602,
        "call-recovered",
        false,
        None,
        Some("the session was interrupted while this call was in flight"),
    ));
    let recovery = events[601].clone();

    let mut state = state();
    replay(&mut state, events);
    state.replay_batch();
    // The same result the snapshot already holds arrives on the render channel.
    state.live_event(RenderEvent::Logged(recovery));
    run_replay(&mut state);

    let rows = screen(120, 40, &mut state);
    let calls = rows.iter().filter(|row| row.contains("▸ 调用")).count();
    assert_eq!(calls, 1, "the recovered call is painted once: {rows:?}");
}

#[test]
fn a_finished_replay_sits_at_the_bottom_with_no_indicator() {
    // The reader is caught up: no "N new rows" bar, and the newest history row is
    // the one at the bottom of the pane (spec §2, user story 17).
    let mut state = state();
    replay(
        &mut state,
        (1..=40)
            .map(|seq| message(seq, &format!("第 {seq} 段历史"), None))
            .collect(),
    );
    run_replay(&mut state);

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains(wording::back_to_bottom()),
        "there is nowhere to go back to: {text}"
    );
    assert!(
        text.contains("第 40 段历史"),
        "and the last history row is on screen: {text}"
    );
}

// ---------------------------------------------------------------------------
// 票 07 — the divider, the panel and the header mode
// ---------------------------------------------------------------------------

#[test]
fn the_divider_separates_history_from_what_this_run_adds() {
    // `[history] → [divider] → [banner]`, in exactly that order (spec §6, user story
    // 20, 21).
    let mut state = state();
    replay(
        &mut state,
        vec![session_started(1), message(2, "上一段的回答", None)],
    );
    state.live_event(RenderEvent::Notice("本段的 banner".to_owned()));
    run_replay(&mut state);

    let rows = screen(120, 40, &mut state);
    let history = rows
        .iter()
        .position(|row| row.contains("上一段的回答"))
        .expect("the history is drawn");
    let divider = rows
        .iter()
        .position(|row| row.contains(wording::history_divider()))
        .expect("the seam is drawn");
    let banner = rows
        .iter()
        .position(|row| row.contains("本段的 banner"))
        .expect("the banner is drawn");
    assert!(
        history < divider && divider < banner,
        "history before seam before banner: {rows:?}"
    );
}

#[test]
fn a_history_that_drew_nothing_gets_no_divider() {
    // An empty stream and a bare skeleton draw nothing, so there is no seam to mark
    // (spec §6, user story 25).
    for events in [Vec::new(), vec![session_started(1)]] {
        let mut state = state();
        replay(&mut state, events.clone());
        run_replay(&mut state);
        let text = screen(120, 40, &mut state).join("\n");
        assert!(
            !text.contains(wording::history_divider()),
            "{events:?} drew a divider: {text}"
        );
    }
}

#[test]
fn the_divider_is_a_render_layer_line_that_is_fresh_on_every_reopen() {
    // It never enters the log: replaying the same assembled stream twice leaves two
    // seams and no third one inside either history (spec §6, user story 22).
    let history = vec![message(1, "旧的一段", None)];
    let mut state = state();
    replay(&mut state, history.clone());
    run_replay(&mut state);
    replay(&mut state, history);
    run_replay(&mut state);

    let rows = screen(120, 40, &mut state);
    let seams = rows
        .iter()
        .filter(|row| row.contains(wording::history_divider()))
        .count();
    assert_eq!(seams, 2, "one fresh seam per reopen: {rows:?}");
}

#[test]
fn the_panel_adds_up_the_history_and_keeps_accumulating() {
    // The panel is a side effect of applying each block, so history counts and live
    // usage continues from there rather than restarting (spec §7, user story 23).
    let mut state = state();
    replay(
        &mut state,
        vec![
            usage(1, 100, 20, 30, 70),
            turn_ended(2),
            message(3, "历史上的一轮", None),
        ],
    );
    run_replay(&mut state);

    // A live call arrives after the seam.
    state.live_event(RenderEvent::Logged(usage(4, 50, 10, 0, 50)));
    state.live_event(RenderEvent::Logged(turn_ended(5)));

    let rows = screen(120, 40, &mut state);
    let tokens = panel_field(&rows, 1);
    let turns = panel_field(&rows, 2);
    let input = panel_field(&rows, 3);
    assert!(tokens.contains("180"), "100+20+50+10: {tokens:?}");
    assert!(turns.contains('2'), "two turns: {turns:?}");
    assert!(input.contains("150"), "100+50 input: {input:?}");
}

#[test]
fn the_rail_grows_with_the_replayed_history() {
    // The rail is derived from the stream like everything else, so a reopened session
    // finds its turns already on the column — and during the replay it fills a batch at
    // a time rather than appearing whole (spec §4).
    let mut state = state();
    let history: Vec<Event> = (1..=40).map(turn_ended).collect();
    replay(&mut state, history);
    run_replay(&mut state);

    // Forty turns, all cut down to the column's own height: the mark at the top and the
    // newest turn at the foot as the focus, because a finished replay returns the
    // viewport to the bottom.
    let shape = turn_rail_shape(&mut state);
    assert_eq!(
        shape.chars().next(),
        Some('⋮'),
        "the column says older turns are above: {shape}"
    );
    assert_eq!(
        shape.chars().last(),
        Some('┃'),
        "and the newest turn is the focus: {shape}"
    );
    assert_eq!(
        shape.matches('┃').count(),
        1,
        "exactly one focus cell: {shape}"
    );

    // A live turn after the seam adds its cell, so history and this session share one
    // rail rather than starting a second.
    let before = shape.len();
    state.live_event(RenderEvent::Logged(turn_ended(41)));
    let after = turn_rail_shape(&mut state);
    assert!(
        after.len() >= before && after.ends_with('┃'),
        "the new turn took its cell and the focus moved to it: {after}"
    );
}

#[test]
fn the_status_row_mode_is_whatever_the_history_ended_on() {
    // Plan injection then a mode change reads as 询问; plan injection alone reads as
    // 计划; no mode event keeps the assembled default (spec §7, user story 24).
    let cases: Vec<(Vec<Event>, &str)> = vec![
        (vec![plan_injected(1), mode_change(2)], "询问"),
        (vec![plan_injected(1)], "计划"),
        (vec![message(1, "没有模式事件", None)], "询问"),
    ];
    for (events, expected) in cases {
        let mut state = state();
        replay(&mut state, events.clone());
        run_replay(&mut state);
        let text = screen(120, 40, &mut state).join("\n");
        assert!(
            text.contains(expected),
            "{events:?} should read as {expected}: {text}"
        );
    }
}

// ---------------------------------------------------------------------------
// 票 08 — history lines open the same detail overlay
// ---------------------------------------------------------------------------

#[test]
fn a_history_tool_line_opens_the_same_detail_overlay() {
    // History rows go through the one `apply`, so they carry the one hit table: a
    // click on a replayed tool call opens the detail a live one would (spec §8, user
    // story 6).
    let mut state = state();
    replay(
        &mut state,
        vec![
            tool_started(1, "call-h1", "bash", serde_json::json!({"command": "ls"})),
            tool_completed(2, "call-h1", true, Some("a\nb"), None),
        ],
    );
    run_replay(&mut state);

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("── 参数 ──"),
        "the overlay opened on the history row: {text}"
    );
    assert!(text.contains("\"command\""), "with the arguments: {text}");
}

#[test]
fn a_history_thinking_line_opens_its_recorded_trace() {
    // `MessageCompleted.reasoning` is where a finished trace lives, so a replayed
    // message with one has a clickable 思考完成 line (spec §8, user story 7).
    let mut state = state();
    replay(
        &mut state,
        vec![
            turn_started(1),
            message(2, "答案是 42。", Some("先看依赖，再看测试。")),
        ],
    );
    run_replay(&mut state);

    click_row(&mut state, 120, 40, "思考完成");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("先看依赖，再看测试。"),
        "the recorded trace is shown: {text}"
    );
}

#[test]
fn a_history_without_recorded_reasoning_has_no_thinking_line() {
    // The synthesizer's shape: deltas streamed, nothing written down. There is no
    // trace to fake and no empty box to open (spec §8, user story 11).
    let mut state = state();
    replay(
        &mut state,
        vec![turn_started(1), message(2, "没有留下推理", None)],
    );
    run_replay(&mut state);

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("思考完成"),
        "no thinking line for a message with no trace: {text}"
    );
}

/// The replayed history for a tool call whose preview carries `output`.
fn history_with_tool_output(id: &str, output: Option<&str>) -> Vec<Event> {
    vec![
        tool_started(1, id, "bash", serde_json::json!({"command": "cat big"})),
        tool_completed(2, id, true, output, None),
    ]
}

#[test]
fn a_history_detail_reads_the_spilled_tool_output() {
    // The event carries the preview; the whole text is the file the call id names
    // under the session directory (spec §8).
    let dir = tempfile::tempdir().expect("a session directory");
    let outputs = dir.path().join("outputs");
    std::fs::create_dir_all(&outputs).expect("the session's outputs directory");
    std::fs::write(
        outputs.join("call-h2.txt"),
        "the whole output\nwith a second line the preview never carried",
    )
    .expect("the spilled file");

    let mut state = state_in(dir.path());
    replay(
        &mut state,
        history_with_tool_output("call-h2", Some("the whole output\n[truncated: 999 chars]")),
    );
    run_replay(&mut state);

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("with a second line the preview never carried"),
        "the spilled file is shown whole: {text}"
    );
    assert!(!text.contains("全文不可用"), "and not degraded: {text}");
}

#[test]
fn a_history_detail_degrades_when_the_spilled_file_is_gone() {
    // `prune`, or a hand-deleted file: the preview plus the sentence that says it is
    // not the whole thing (spec §8, user story 9).
    let dir = tempfile::tempdir().expect("a session directory");
    let mut state = state_in(dir.path());
    replay(
        &mut state,
        history_with_tool_output(
            "call-h3",
            Some("head of the output\n[truncated: 999 chars]"),
        ),
    );
    run_replay(&mut state);

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("head of the output"), "the preview: {text}");
    assert!(text.contains("全文不可用"), "the degradation: {text}");
}

#[test]
fn a_history_detail_degrades_when_the_spilled_file_is_empty() {
    let dir = tempfile::tempdir().expect("a session directory");
    let outputs = dir.path().join("outputs");
    std::fs::create_dir_all(&outputs).expect("the session's outputs directory");
    std::fs::write(outputs.join("call-h4.txt"), "").expect("the empty file");

    let mut state = state_in(dir.path());
    replay(
        &mut state,
        history_with_tool_output(
            "call-h4",
            Some("head of the output\n[truncated: 999 chars]"),
        ),
    );
    run_replay(&mut state);

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("head of the output"), "the preview: {text}");
    assert!(text.contains("全文不可用"), "the degradation: {text}");
}

#[test]
fn a_history_result_without_the_truncation_note_is_its_own_full_text() {
    // No note means no spilled file was ever written: the event's text **is** the
    // whole body, and calling it unavailable would be a false warning. This is also
    // the shape `--continue` writes for a dangling call (spec §8, user story 10).
    let dir = tempfile::tempdir().expect("a session directory");
    let mut state = state_in(dir.path());
    replay(
        &mut state,
        vec![
            tool_started(1, "call-h5", "bash", serde_json::json!({"command": "ls"})),
            tool_completed(
                2,
                "call-h5",
                false,
                None,
                Some("the session was interrupted while this call was in flight"),
            ),
        ],
    );
    run_replay(&mut state);

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("interrupted while this call was in flight"),
        "the event text is shown: {text}"
    );
    assert!(
        !text.contains("全文不可用"),
        "and no false degradation is claimed: {text}"
    );
}

#[test]
fn the_divider_and_section_lines_are_not_clickable() {
    // Only the collapsed lines a reader can open carry the marker; the seam and the
    // turn rules are text (spec §8).
    let mut state = state();
    replay(
        &mut state,
        vec![
            message(1, "历史", None),
            turn_ended(2),
            message(3, "又一段", None),
        ],
    );
    run_replay(&mut state);

    let rows = screen(120, 40, &mut state);
    let seam = rows
        .iter()
        .position(|row| row.contains(wording::history_divider()))
        .expect("the seam is drawn");
    state.mouse(click(10, seam as u16));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("── 参数 ──") && !text.contains("── 推理 ──"),
        "a click on the seam opens nothing: {text}"
    );
}

#[test]
fn a_history_detail_freezes_the_viewport_and_releases_it() {
    // The overlay is the live one, so the history path inherits its viewport
    // contract: opening freezes the transcript, output arriving meanwhile does not
    // pull it, and closing returns to the bottom (spec §8, user story 35).
    let mut state = state();
    replay(
        &mut state,
        vec![
            tool_started(1, "call-h6", "bash", serde_json::json!({"command": "ls"})),
            tool_completed(2, "call-h6", true, Some("body"), None),
        ],
    );
    run_replay(&mut state);

    click_row(&mut state, 120, 40, "调用 bash");
    assert!(
        screen(120, 40, &mut state)
            .join("\n")
            .contains("── 参数 ──"),
        "the history detail is open"
    );

    // Output arrives while the overlay is up: it is appended, not followed.
    state.live_event(RenderEvent::Notice("历史详情打开时的新内容".to_owned()));
    let frozen = screen(120, 40, &mut state);
    assert!(
        frozen.iter().any(|row| row.contains("── 参数 ──")),
        "the overlay is still the thing being read: {frozen:?}"
    );

    // `Esc` closes it, and the viewport is back at the bottom — including the line
    // that arrived while it was frozen.
    state.key(Key::Esc);
    let after = screen(120, 40, &mut state).join("\n");
    assert!(
        after.contains("历史详情打开时的新内容"),
        "closing returns to the newest line: {after}"
    );
    assert!(
        !after.contains(wording::back_to_bottom()),
        "with the viewport following again: {after}"
    );
}
