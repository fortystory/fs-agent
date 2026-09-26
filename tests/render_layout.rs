//! The fullscreen shell, rendered into a `TestBackend` so the geometry can be
//! asserted without a terminal (`.scratch/tui-sidebar/spec.md` §1–§2,
//! `Testing Decisions`).
//!
//! The seam is [`draw_frame`]: a state goes in, a fixed-size buffer comes out. Every
//! assertion below is about what a person would see — which regions exist, what the
//! sidebar's page says, where the rail's cells are, how many hints fit — never about
//! the rectangles the layout computes on the way there.

use fs_agent::render::width::text_columns;
use fs_agent::render::{
    draw_frame, CatalogEntry, ConsoleRequest, Key, RenderEvent, SessionFacts, TuiState,
    PULSE_PALETTE,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

fn facts() -> SessionFacts {
    SessionFacts {
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: "~/code/fortystory/fs-agent".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        budget_limit: Some(100_000),
        speaker_order: Vec::new(),
    }
}

fn state() -> TuiState {
    TuiState::new(facts())
}

/// The injected facts for a session with this roster: one name is a session with a
/// single speaker, two make it a discussion.
fn facts_with_roster(names: &[&str]) -> SessionFacts {
    let mut facts = facts();
    facts.speaker_order = names.iter().map(|name| (*name).to_owned()).collect();
    facts
}

/// A state whose loop is **waiting for a line** — what an idle front end looks like.
///
/// The distinction matters: inside a run, `Esc` and `Ctrl-C` are the cancel gesture
/// rather than "close this" / "quit" (spec §6), and the loop is what says a run is on
/// (`ConsoleRequest::RunState`). A fresh state is idle already; what the prompt request
/// adds is a live editor for these tests to type into. The idle-UI tests — the `/` menu,
/// a draft `Esc` would clear, the hint ladder — are about the waiting state.
fn idle() -> TuiState {
    let mut state = state();
    let (reply, line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    drop(line);
    state
}

/// Render one frame at a fixed size and read the screen back as rows of text.
fn screen(width: u16, height: u16, state: &mut TuiState) -> Vec<String> {
    let frame = buffer(width, height, state);
    (0..height).map(|y| row_text(&frame, y, width)).collect()
}

/// One row's text between two columns, read from the frame.
///
/// A wide grapheme covers the cell after it, and that cell holds a space in the
/// backend's buffer; reading it verbatim would spell `终 端` for `终端`. Advancing by
/// the symbol's display width is what makes the text read like the terminal.
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

/// A whole row of the buffer, as text.
fn row_text(buffer: &Buffer, y: u16, width: u16) -> String {
    cells(buffer, y, 0, width)
}

/// The transcript's first row: the row inside the frame, right under its top border.
///
/// The shell is one frame rather than one block per region, so there is no border
/// row to search for any more; what a test that counts rows needs is where the
/// transcript starts and how tall it is, and both are read off the frame below.
const TRANSCRIPT_TOP: usize = 1;

/// The rows the transcript occupies in a rendered frame: everything from the
/// frame's first content row down to the rule above the status row.
///
/// Measured rather than remembered, because the terminal's height, the sidebar's
/// height ladder and the draft's all move it.
fn transcript_rows(rows: &[String]) -> usize {
    rows.iter()
        .position(|row| row.ends_with('┤'))
        .expect("the main column's first rule is on screen")
        - TRANSCRIPT_TOP
}

/// The rendered frame itself, for assertions about a particular cell.
fn buffer(width: u16, height: u16, state: &mut TuiState) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("TestBackend");
    terminal
        .draw(|frame| draw_frame(frame, state))
        .expect("one frame");
    terminal.backend().buffer().clone()
}

#[test]
fn a_terminal_below_the_minimum_shows_one_centred_notice() {
    // 39x24 is one column short of the minimum; 40x9 is one row short. Both show the
    // notice and nothing else — no half-drawn panes (spec §2).
    for (width, height) in [(39, 24), (40, 9)] {
        let rows = screen(width, height, &mut state());
        let text = rows.join("\n");
        assert!(
            text.contains("终端太小：至少 40×10"),
            "{width}x{height} says why it will not draw: {text}"
        );
        assert!(
            !text.contains('┌') && !text.contains('└'),
            "{width}x{height} draws no panes at all: {text}"
        );
        let notice = rows
            .iter()
            .position(|row| row.contains("终端太小"))
            .expect("the notice is on some row");
        assert_eq!(
            notice,
            height as usize / 2,
            "{width}x{height} centres the notice vertically"
        );
    }
}

#[test]
fn a_wide_terminal_draws_the_mark_the_sidebar_and_the_main_column() {
    // 120x24 is the reference size: one frame, a full-height sidebar carrying the
    // mark, the tab bar and the six readings, and a main column of transcript,
    // status row, input and hints — with no blank row between any of them
    // (`.scratch/tui-sidebar/spec.md` §1).
    let rows = screen(120, 24, &mut state());

    // One frame around everything, closed on all four corners.
    assert!(rows[0].starts_with('┌'), "the frame opens: {:?}", rows[0]);
    assert!(rows[0].ends_with('┐'), "and closes: {:?}", rows[0]);
    assert!(rows[23].starts_with('└'), "at the foot: {:?}", rows[23]);
    assert!(rows[23].ends_with('┘'), "and on the right: {:?}", rows[23]);

    // The sidebar: the mark on the wide rung, centred with a column of air.
    assert!(
        rows[1].starts_with("│ ▄▀▀█") && rows[5].contains("▀▀▀"),
        "the mark's first and last rows are the sidebar's first and last: {:?} / {:?}",
        rows[1],
        rows[5]
    );

    // The tab bar: two rules with the three labels between them, and the rules join
    // the frame's left border and the divider.
    assert_eq!(
        rows[6].trim_matches(|ch| ch == '├' || ch == '┤' || ch == '─' || ch == '│' || ch == ' '),
        "",
        "the tab bar's top rule spans the sidebar: {:?}",
        rows[6]
    );
    assert!(
        rows[7].contains("调用量") && rows[7].contains("轨迹") && rows[7].contains("文件"),
        "the three tabs are on their own row: {:?}",
        rows[7]
    );

    // The divider runs the whole height of the frame, and the main column's rules
    // meet it with junctions. A 24-row terminal leaves the transcript fourteen rows:
    // three go to the input's floor and seven to the chrome
    // (`.scratch/tui-input-pulse/spec.md` §1).
    assert_eq!(
        transcript_rows(&rows),
        14,
        "120x24 gives the transcript 14 rows"
    );
    assert!(
        rows[15].contains('├') && rows[15].contains('┤'),
        "the rule above the status row spans the main column: {:?}",
        rows[15]
    );
    assert!(
        rows[16].contains("模型 claude-sonnet-4-5")
            && rows[16].contains("模式 询问")
            && rows[16].contains("上下文 —"),
        "the status row names the model, the mode and the share: {:?}",
        rows[16]
    );
    assert!(
        rows[18].contains("│> "),
        "the input's first row carries the prompt: {:?}",
        rows[18]
    );
    assert!(
        rows[19].trim_matches(['│', ' ']).is_empty()
            && rows[20].trim_matches(['│', ' ']).is_empty(),
        "and the two under it are blank rows of the same box: {:?} / {:?}",
        rows[19],
        rows[20]
    );
    assert!(
        rows[22].contains("ctrl-c"),
        "the hints name the way out: {:?}",
        rows[22]
    );

    assert!(
        !rows.join("\n").contains("shift+enter"),
        "no phantom newline key"
    );
    // The shell shows no working directory and no clock any more (spec §8).
    assert!(
        !rows.join("\n").contains("~/code"),
        "the directory is not on screen: {rows:#?}"
    );
}

#[test]
fn the_wide_sidebar_is_forty_columns_and_centres_the_mark() {
    // The wide rung is 40 columns and the mark is 38, so each side gets a column of
    // air (spec §2). The divider sits in its own column at 41.
    let frame = buffer(120, 24, &mut state());
    assert_eq!(
        frame[(41, 0)].symbol(),
        "┬",
        "the divider meets the top border"
    );
    assert_eq!(frame[(41, 23)].symbol(), "┴", "and the bottom one");
    assert_eq!(frame[(1, 1)].symbol(), " ", "one column of air on the left");
    assert_eq!(frame[(2, 1)].symbol(), "▄", "then the mark");
    // The mark is 38 columns: 2 + 38 = 40, so the last air column is 40 and the
    // divider is 41.
    assert_eq!(frame[(40, 1)].symbol(), " ", "and one on the right");
}

#[test]
fn the_mark_is_lit_from_above_and_only_on_the_wide_rung() {
    // The mark at rest — the painter's half of it: the characters are `wording`'s, but
    // which rows they land on and how the gradient falls is the painter's, so it is
    // asserted where it can be seen, in the buffer, cell by cell. The moving half is
    // `the_mark_walks_the_pulse_ring_while_a_run_is_in_flight`
    // (`.scratch/tui-input-pulse/spec.md` §2).
    let frame = buffer(120, 24, &mut state());
    assert_eq!(
        frame[(2, 1)].symbol(),
        "▄",
        "the mark's first row starts at the sidebar's first content row"
    );
    assert_eq!(
        frame[(2, 1)].fg,
        Color::LightMagenta,
        "the top of the mark is the bright end"
    );
    assert_eq!(
        frame[(2, 5)].fg,
        Color::Magenta,
        "and the bottom row is the dim end"
    );

    // The mark is the wide rung's alone: 100 columns gets the 28-column sidebar
    // (which the mark does not fit in) and 60 columns has no sidebar at all.
    for (width, height, identity) in [
        (100, 24, true),
        (80, 24, true),
        (60, 24, false),
        (40, 24, false),
    ] {
        let mut fresh = state();
        let rows = screen(width, height, &mut fresh);
        assert!(
            !rows.join("\n").contains('▄'),
            "{width}x{height} is below the mark's rung: {:#?}",
            rows[1]
        );
        assert_eq!(
            rows.join("\n").contains("fs-agent"),
            identity,
            "{width}x{height} shows the text identity where the sidebar is drawn: {:#?}",
            rows[1]
        );
    }
}

#[test]
fn the_sidebar_has_two_widths_and_a_hidden_third() {
    // The ladder is decided by width alone — the sidebar is a column of its own, so
    // its height is not the transcript's to spend (spec §2). 120 and up is the wide
    // rung, 80 through 119 the narrow one, and below 80 the sidebar is gone whole.
    for (width, divider) in [
        (174, 41u16),
        (120, 41),
        (100, 29),
        (80, 29),
        (79, 0),
        (40, 0),
    ] {
        let frame = buffer(width, 24, &mut state());
        let text: String = (0..24)
            .map(|y| row_text(&frame, y, width))
            .collect::<Vec<_>>()
            .join("\n");
        if divider == 0 {
            assert!(
                !text.contains('┬') && !text.contains('┴'),
                "no sidebar at {width} columns: {text}"
            );
        } else {
            assert_eq!(
                frame[(divider, 0)].symbol(),
                "┬",
                "the divider is at {divider} at {width} columns"
            );
            // The main column's rules start at the divider and end at the frame.
            assert_eq!(
                frame[(divider, 17)].symbol(),
                "├",
                "and the main column's rule meets it: {text}"
            );
        }
    }

    // The sidebar is drawn at 80x14 — four rows of fields is not a floor any more,
    // because the sidebar's own height is the terminal's less the frame.
    let smallest = buffer(80, 14, &mut state());
    assert_eq!(
        smallest[(29, 0)].symbol(),
        "┬",
        "the sidebar is drawn at 80x14"
    );
}

#[test]
fn a_floor_sized_terminal_still_draws_the_main_column() {
    // 40x10 is inside the minimum: the sidebar is hidden, and the main column still
    // has its transcript, status row, input and hints (spec §2).
    let rows = screen(40, 10, &mut state());
    for (row, line) in rows.iter().enumerate() {
        assert!(
            line.starts_with(['┌', '│', '└', '├', '┤']),
            "row {row} belongs to the frame: {line:?}"
        );
    }
    // The input's floor is three rows, but the transcript's last row wins where the
    // two meet: at the floor the input takes two and the transcript keeps one
    // (`.scratch/tui-input-pulse/spec.md` §1). Its rows are 5 and 6, the rules are
    // above and below them, and the hint row keeps the eighth either way.
    assert_eq!(transcript_rows(&rows), 1, "one transcript row: {rows:#?}");
    assert!(
        rows[5].starts_with("│> "),
        "the input's first row is the prompt's: {:?}",
        rows[5]
    );
    assert!(
        rows[6].trim_matches(['│', ' ']).is_empty(),
        "and it takes two rows here, not three: {:?}",
        rows[6]
    );
    assert!(
        rows[3].contains("模式 询问") && rows[3].contains("上下文 —"),
        "the status row gives up the model at the floor: {:?}",
        rows[3]
    );
    assert!(
        !rows[3].contains("claude-sonnet"),
        "which is the first thing it loses: {:?}",
        rows[3]
    );
    assert!(rows[8].contains("ctrl-c"), "the hint row: {:?}", rows[8]);
    assert!(
        !rows.join("\n").contains("fs-agent"),
        "the sidebar is hidden whole: {rows:#?}"
    );
}

/// How many `·`-separated items the hint row holds, for a state left as it is.
fn hint_items_of(width: u16, state: &mut TuiState) -> Vec<String> {
    let rows = screen(width, 24, state);
    let row = rows
        .iter()
        .find(|row| row.contains("ctrl-c"))
        .expect("the hint row is on screen");
    row.trim_matches(|ch| ch == '│' || ch == ' ')
        .split(" · ")
        .map(str::to_owned)
        .collect()
}

/// The hint row of a session **waiting for a line** — the only time it can promise
/// `enter 发送`.
fn hint_items(width: u16) -> Vec<String> {
    let mut state = state();
    let (reply, _line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    hint_items_of(width, &mut state)
}

/// The same, with a run in flight: the way out is then `ctrl-c 退出`, seven columns
/// shorter, which is what buys the state word back at 120 columns.
fn hint_items_busy(width: u16) -> Vec<String> {
    let mut state = state();
    let (reply, _line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    state.request(ConsoleRequest::RunState { running: true });
    hint_items_of(width, &mut state)
}

#[test]
fn a_session_with_no_line_being_read_promises_only_what_the_keyboard_does() {
    // Mid-turn, or inside a one-shot `discuss`: nothing is reading lines, so `enter
    // 发送` would be a lie and the plan-mode gesture has nothing to toggle.
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    let rows = screen(120, 24, &mut state);
    let row = rows
        .iter()
        .find(|row| row.contains("ctrl-c"))
        .expect("the hint row is on screen");
    assert!(row.contains("esc 取消"), "{row}");
    assert!(row.contains("PgUp/PgDn 滚动"), "{row}");
    assert!(!row.contains("enter"), "{row}");
    assert!(!row.contains("shift+tab"), "{row}");
}

#[test]
fn the_hint_row_gives_up_hints_before_it_gives_up_the_way_out() {
    // The hint row's width is the **main column's** content width, not the
    // terminal's: the sidebar's columns are not hints to give (`tui-sidebar` spec
    // §2). The measured idle ladder is therefore 40 columns -> one hint + the way
    // out + the state word, 80 -> two, 100 -> three, 120 -> four (and no state
    // word: four hints and the way out leave no room for it), 174 -> five.
    assert_eq!(hint_items(40).len(), 3, "40 columns: {:?}", hint_items(40));
    assert_eq!(hint_items(80).len(), 3, "80 columns: {:?}", hint_items(80));
    assert_eq!(
        hint_items(100).len(),
        5,
        "100 columns: {:?}",
        hint_items(100)
    );
    assert_eq!(
        hint_items(120).len(),
        5,
        "120 columns: {:?}",
        hint_items(120)
    );
    assert_eq!(
        hint_items(174).len(),
        7,
        "174 columns: {:?}",
        hint_items(174)
    );

    // At the floor only one hint fits before the way out, and the state word still
    // does — it is the newline hint, one rung up, that a 40-column terminal gives up.
    let floor = hint_items(40);
    assert_eq!(floor[0], "就绪", "the state word fits at 40: {floor:?}");
    assert_eq!(floor[1], "enter 发送", "then the send hint: {floor:?}");
    assert!(
        !floor.contains(&"ctrl-j 换行".to_owned()),
        "the newline hint is what the width costs: {floor:?}"
    );
    assert_eq!(floor.last().unwrap(), "ctrl-c/ctrl-d 退出");

    // 80 columns is the narrow sidebar's rung, so it has fewer hint columns than a
    // bare 80-column terminal would: 29 of its columns are the sidebar and its
    // divider. Two hints fit, and the state word is what goes.
    let narrow = hint_items(80);
    assert_eq!(narrow[0], "enter 发送", "no state word at 80: {narrow:?}");
    assert!(
        !narrow.contains(&"就绪".to_owned()),
        "the state word is the first thing given up: {narrow:?}"
    );
    assert_eq!(narrow.last().unwrap(), "ctrl-c/ctrl-d 退出");

    // 120 columns is the reference size, and it pins a deliberate outcome: four
    // hints and the way out fit and `就绪` does not (spec §2 — "这是预期行为，不是
    // bug"). A busy session's way out is seven columns shorter, which buys the state
    // word back.
    let wide = hint_items(120);
    assert_eq!(
        wide,
        vec![
            "enter 发送",
            "ctrl-j 换行",
            "esc 取消",
            "shift+tab 计划",
            "ctrl-c/ctrl-d 退出",
        ],
        "four hints and the way out, and no state word"
    );
    let busy = hint_items_busy(120);
    assert_eq!(
        busy.first().map(String::as_str),
        Some("工作中"),
        "the busy way out is shorter, so the state word fits: {busy:?}"
    );

    // 174 columns can hold every hint and the state word together.
    let roomy = hint_items(174);
    assert_eq!(roomy[0], "就绪", "the state word is back: {roomy:?}");
    assert!(
        roomy.contains(&"PgUp/PgDn 滚动".to_owned()),
        "with the whole hint list behind it: {roomy:?}"
    );
    assert!(
        !screen(174, 24, &mut state())
            .join("\n")
            .contains("shift+enter"),
        "no phantom newline key at any width"
    );
}

#[test]
fn the_transcript_pane_shows_both_the_notices_and_the_streaming_tail() {
    use fs_agent::events::SpeakerId;
    use fs_agent::render::{DeltaKind, RenderEvent};

    let mut state = state();
    state.apply(RenderEvent::Notice(
        "fs-agent：会话 abc · 模型 m · 模式 询问 · /tmp/x".to_owned(),
    ));
    state.apply(RenderEvent::Delta {
        speaker: SpeakerId::Debater("kimi".into()),
        kind: DeltaKind::Text,
        text: "正在读文件".to_owned(),
    });

    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("fs-agent：会话 abc"),
        "the notice is a transcript line: {text}"
    );
    assert!(
        text.contains("正在读文件"),
        "the streaming tail is in the pane too: {text}"
    );
}

/// The mark's five row colours at 120x24, top to bottom: the column the painter
/// writes is the sidebar's second column, and the mark's first glyph sits there on
/// every one of its rows.
fn mark_colours(state: &mut TuiState) -> Vec<Color> {
    let frame = buffer(120, 24, state);
    (1..=5u16).map(|y| frame[(2, y)].fg).collect()
}

#[test]
fn the_mark_walks_the_pulse_ring_while_a_run_is_in_flight() {
    // The busy half of the mark: the whole block takes one colour off the ring per
    // frame, and twelve frames bring it back to where it started
    // (`.scratch/tui-input-pulse/spec.md` §2). The idle ramp next door is the other
    // half of the same painter, and the ring's first frame is that ramp's own bright
    // end, so the two meet without a jump.
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    assert_eq!(
        PULSE_PALETTE[0],
        Color::LightMagenta,
        "frame 0 is the ramp's top colour"
    );
    // Two laps: one to show the hue moves, the second to show the ring closes.
    for frame in 1..=PULSE_PALETTE.len() * 2 {
        state.tick();
        let expected = PULSE_PALETTE[frame % PULSE_PALETTE.len()];
        assert_eq!(
            mark_colours(&mut state),
            vec![expected; 5],
            "frame {frame} of the ring: the whole mark is one colour"
        );
    }
}

#[test]
fn a_finished_run_puts_the_pulse_back_at_the_rings_first_frame() {
    // The pulse is one run's, not the session's: the mark goes back to the static ramp
    // when the run ends, and the next run does not resume mid-colour — it starts one
    // frame past the ring's first entry, whatever the last run left behind
    // (`.scratch/tui-input-pulse/spec.md` §2).
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    for _ in 0..3 {
        state.tick();
    }
    assert_eq!(
        mark_colours(&mut state),
        vec![PULSE_PALETTE[3]; 5],
        "three frames in, the mark is on frame three"
    );

    state.request(ConsoleRequest::RunState { running: false });
    assert_eq!(
        mark_colours(&mut state),
        vec![
            Color::LightMagenta,
            Color::LightMagenta,
            Color::LightMagenta,
            Color::LightMagenta,
            Color::Magenta
        ],
        "idle again: the ramp, not the frame the run stopped on"
    );

    state.request(ConsoleRequest::RunState { running: true });
    state.tick();
    assert_eq!(
        mark_colours(&mut state),
        vec![PULSE_PALETTE[1]; 5],
        "and the next run starts at the ring's first frame, not where the last one left off"
    );
}

#[test]
fn the_pulse_is_invisible_where_the_mark_is_not_drawn() {
    // The mark *is* the animation, so the rungs without one have no animation at all —
    // which the user accepted, and the text identity line stays a still line. What must
    // not happen is a tick redrawing anything: two frames apart come out identical
    // (`.scratch/tui-input-pulse/spec.md` §2).
    for (width, height) in [(100u16, 24u16), (60, 24), (40, 10)] {
        let mut state = state();
        state.request(ConsoleRequest::RunState { running: true });
        state.tick();
        let before = buffer(width, height, &mut state);
        state.tick();
        let after = buffer(width, height, &mut state);
        assert_eq!(
            before, after,
            "{width}x{height} has no mark, so a pulse frame changes nothing"
        );
        assert!(
            !row_text(&after, 1, width).contains('▄'),
            "{width}x{height} draws no mark to animate: {:?}",
            row_text(&after, 1, width)
        );
    }
}

#[test]
fn every_size_in_the_matrix_draws_the_regions_its_budget_allows() {
    // The size matrix the spec's geometry table covers, each row asserted for the
    // reason it is in the table: the sidebar's rung and identity, the status row's
    // rung, and the transcript rows the chrome leaves (spec §1–§2).
    let cases = [
        // width, height, sidebar tier, identity, model on the status row, transcript rows
        // The input's floor is three rows, so every rung that has the room gives the
        // transcript `h - 7 - 3`; at the 40x10 floor the input takes two instead and
        // the transcript keeps its last row (`.scratch/tui-input-pulse/spec.md` §1).
        (40u16, 10u16, None, "", false, 1usize),
        (40, 24, None, "", false, 14),
        (60, 24, None, "", true, 14),
        (80, 14, Some(28u16), "fs-agent", true, 4),
        (80, 24, Some(28), "fs-agent", true, 14),
        (100, 24, Some(28), "fs-agent", true, 14),
        (120, 24, Some(40), "mark", true, 14),
        (174, 50, Some(40), "mark", true, 40),
    ];
    for (width, height, tier, identity, model, rows_expected) in cases {
        let rows = screen(width, height, &mut state());
        let text = rows.join("\n");
        assert!(
            !text.contains("终端太小"),
            "{width}x{height} is inside the minimum: {text}"
        );
        assert!(
            rows[0].starts_with('┌') && rows[0].ends_with('┐'),
            "{width}x{height} is framed: {:?}",
            rows[0]
        );
        assert!(
            text.contains("ctrl-c"),
            "{width}x{height} keeps the way out: {text}"
        );
        // The status row is never given up: the rung that would take it needs a main
        // column narrower than the terminal floor allows (spec §2).
        assert!(
            text.contains("模式 询问"),
            "{width}x{height} always draws the status row: {text}"
        );
        assert_eq!(
            transcript_rows(&rows),
            rows_expected,
            "{width}x{height} gives the transcript {rows_expected} rows: {rows:#?}"
        );
        // The first rule of the main column sits right under the transcript: the
        // shell keeps no blank row between its parts, at any size.
        assert!(
            rows[TRANSCRIPT_TOP + rows_expected].ends_with('┤'),
            "{width}x{height} draws its first rule right under the transcript: {:?}",
            rows[rows_expected]
        );
        match tier {
            Some(tier) => {
                let frame = buffer(width, height, &mut state());
                assert_eq!(
                    frame[(tier + 1, 0)].symbol(),
                    "┬",
                    "{width}x{height} draws the divider at column {}",
                    tier + 1
                );
            }
            None => assert!(
                !text.contains('┬') && !text.contains('┴'),
                "{width}x{height} hides the sidebar whole: {text}"
            ),
        }
        assert_eq!(
            text.contains('▄'),
            identity == "mark",
            "{width}x{height} draws the mark only on the wide rung: {text}"
        );
        assert_eq!(
            text.contains("fs-agent"),
            identity == "fs-agent",
            "{width}x{height} draws the text identity only on the narrow rung: {text}"
        );
        assert_eq!(
            text.contains("claude-sonnet-4-5"),
            model,
            "{width}x{height} keeps the model on the status row only where it fits: {text}"
        );
    }
}

#[test]
fn the_sidebar_gives_up_its_identity_then_its_fields_as_it_shrinks() {
    // The sidebar's own height ladder (spec §2): the **mark** goes first — to the text
    // identity, then to nothing — and only then do fields leave from the tail (缓存 →
    // 输出 → 输入). The floor is the tab bar plus 上下文 / token / 回合, and width never
    // plays a part in any of this.
    let cases = [
        // height, what the identity is, how many readings survive
        (16u16, "mark", 6usize),
        (12, "fs-agent", 6),
        (11, "none", 6),
        (10, "none", 5),
    ];
    for (height, identity, fields) in cases {
        let mut state = state();
        let rows = screen(120, height, &mut state);
        let text = rows.join("\n");
        let mark = text.contains('▄');
        assert_eq!(
            mark,
            identity == "mark",
            "{height} rows draws the mark only from sixteen up: {text}"
        );
        assert_eq!(
            text.contains("fs-agent"),
            identity == "fs-agent",
            "{height} rows draws the text identity only below the mark's rung: {text}"
        );
        let page = panel_text(120, height, &mut state);
        assert_eq!(
            page.len(),
            fields,
            "{height} rows keeps {fields} readings: {page:?}"
        );
        assert_eq!(
            text.contains("缓存"),
            fields == 6,
            "{height} rows drops the cache row before the input and output ones: {text}"
        );
        // Whatever goes, the tab bar and the three readings that answer "how much room
        // is left" stay.
        assert!(
            text.contains("调用量"),
            "{height} rows keeps the tab bar: {text}"
        );
        assert!(
            text.contains("上下文"),
            "{height} rows keeps the context row: {text}"
        );
        assert!(
            text.contains("token"),
            "{height} rows keeps the token row: {text}"
        );
        assert!(
            text.contains("回合"),
            "{height} rows keeps the turn row: {text}"
        );
    }
}

/// The cell one tab's label starts at, read off the frame.
///
/// The labels are the only place those words appear, so finding the first character of
/// one is finding the tab — and it is found the way a person finds it: on screen.
fn tab_cell(frame: &Buffer, width: u16, height: u16, label: &str) -> (u16, u16) {
    let first: String = label.chars().take(1).collect();
    find_cell(frame, width, height, &first)
        .unwrap_or_else(|| panic!("the {label} tab is on screen"))
}

#[test]
fn the_selected_tab_is_the_bright_one_and_the_others_are_dim() {
    // The bar says which page is showing without a word of explanation: the selected
    // label is bright magenta and bold, the others are the narration grey (spec §3).
    let frame = buffer(120, 24, &mut state());
    let (column, row) = tab_cell(&frame, 120, 24, "调用量");
    assert_eq!(
        frame[(column, row)].fg,
        Color::LightMagenta,
        "the selected tab is the bright one"
    );
    assert!(
        frame[(column, row)].modifier.contains(Modifier::BOLD),
        "and it is bold"
    );
    for label in ["轨迹", "文件"] {
        let (column, row) = tab_cell(&frame, 120, 24, label);
        assert_eq!(
            frame[(column, row)].fg,
            Color::DarkGray,
            "{label} is not the selected page"
        );
        assert!(
            !frame[(column, row)].modifier.contains(Modifier::BOLD),
            "{label} is not bold"
        );
    }
}

#[test]
fn clicking_a_tab_switches_the_sidebar_page() {
    use fs_agent::render::wording;

    let mut state = state();
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        !text.contains(wording::tab_placeholder()),
        "the usage page is showing: {text}"
    );

    // 轨迹 is not built yet, so it says so rather than showing made-up data.
    let frame = buffer(120, 24, &mut state);
    let (column, row) = tab_cell(&frame, 120, 24, "轨迹");
    state.mouse(click(column, row));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains(wording::tab_placeholder()),
        "the placeholder is on the page: {text}"
    );
    assert!(!text.contains("token"), "and the readings are not: {text}");
    // Which leaves the status row as the only reading — the accepted cost of the
    // placeholder pages (spec §3).
    assert!(
        text.contains("上下文"),
        "the status row's share is still there: {text}"
    );

    // The selected label moved with it.
    let frame = buffer(120, 24, &mut state);
    let (column, row) = tab_cell(&frame, 120, 24, "轨迹");
    assert_eq!(frame[(column, row)].fg, Color::LightMagenta);

    // And back: 调用量 restores the fields.
    let (column, row) = tab_cell(&frame, 120, 24, "调用量");
    state.mouse(click(column, row));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("token"), "the readings are back: {text}");
    assert!(
        !text.contains(wording::tab_placeholder()),
        "and the placeholder is gone: {text}"
    );
}

#[test]
fn only_the_tab_labels_answer_a_click() {
    use fs_agent::render::wording;

    // The rule that fills the rest of the tab row, and the glyph between two labels,
    // are not controls: a click there does nothing, because nothing there was painted
    // as a tab (spec §3).
    let mut state = state();
    let frame = buffer(120, 24, &mut state);
    let (_, row) = tab_cell(&frame, 120, 24, "调用量");
    let inside_the_sidebar = 2..40u16;
    let fill = inside_the_sidebar
        .clone()
        .find(|x| frame[(*x, row)].symbol() == "─")
        .expect("the row fills after the labels");
    let separator = inside_the_sidebar
        .clone()
        .find(|x| frame[(*x, row)].symbol() == "│")
        .expect("the labels are separated");

    for column in [separator, fill] {
        state.mouse(click(column, row));
        let text = screen(120, 24, &mut state).join("\n");
        assert!(
            !text.contains(wording::tab_placeholder()),
            "a click at column {column} is not a tab: {text}"
        );
        assert!(text.contains("token"), "the page did not move: {text}");
    }
}

#[test]
fn a_question_keeps_the_tabs_from_answering() {
    use fs_agent::permissions::Answer;
    use fs_agent::render::{wording, AnswerChoice};

    // A question owns the pointer outright: a click on the tab bar reaches the
    // question's handler and stops there. It must not switch the page, and — the trap
    // this test was written for — it must not close the question either: the frames
    // that paint the tabs and the overlay record their hit rectangles into one table,
    // so a click on a tab arrives at the question as an action it does not own
    // (spec §7, §9).
    let mut state = idle();
    let (request, mut answer) = ask_permission();
    state.request(request);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = tab_cell(&frame, 120, 24, "轨迹");
    state.mouse(click(column, row));

    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("权限询问"),
        "the question is still up: {text}"
    );
    assert!(
        !text.contains(wording::tab_placeholder()),
        "and the page did not switch under it: {text}"
    );
    assert!(
        answer.try_recv().is_err(),
        "and nothing was answered behind the reader's back"
    );
    // It is still answerable, which is what "still up" has to mean.
    state.key(Key::Char('y'));
    assert_eq!(
        answer.try_recv().unwrap(),
        AnswerChoice::Permission(Answer::Allow),
        "the question can still be answered"
    );
}

#[test]
fn a_terminal_with_no_sidebar_has_no_tabs_to_click() {
    use fs_agent::render::wording;

    // Below 80 columns the sidebar is hidden whole, so the labels are not drawn at all
    // — and a click where a tab would have been is an ordinary click on the main
    // column (spec §2, §3).
    let mut state = state();
    let frame = buffer(60, 24, &mut state);
    assert!(
        find_cell(&frame, 60, 24, "调").is_none(),
        "there is no tab bar at 60 columns"
    );
    for (column, row) in [(2u16, 1u16), (4, 3), (2, 8)] {
        state.mouse(click(column, row));
    }
    let text = screen(60, 24, &mut state).join("\n");
    assert!(
        !text.contains(wording::tab_placeholder()),
        "and nothing switched a page that is not there: {text}"
    );
}

/// The rail's column at 120x24: one character per transcript row, top to bottom.
fn turn_rail_cells(state: &mut TuiState) -> Vec<char> {
    let rows = screen(120, 24, state);
    let transcript = transcript_rows(&rows);
    let frame = buffer(120, 24, state);
    (TRANSCRIPT_TOP..TRANSCRIPT_TOP + transcript)
        .map(|y| {
            frame[(RAIL_AT_120, y as u16)]
                .symbol()
                .chars()
                .next()
                .unwrap_or(' ')
        })
        .collect()
}

/// The rail's column as a compact string: `⋮` and `┃`/`┊` only, blanks dropped.
fn turn_rail_shape(state: &mut TuiState) -> String {
    turn_rail_cells(state)
        .into_iter()
        .filter(|ch| *ch != ' ')
        .collect()
}

/// One `MessageCompleted` from the **user**.
fn user_message(seq: u64, text: &str) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, Role, SpeakerId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: text.to_owned(),
            reasoning: None,
        },
    ))
}

/// One `TurnStarted`.
fn turn_started(seq: u64) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, SpeakerId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::TurnStarted {
            agent: SpeakerId::Debater("kimi".into()),
            iteration: 1,
        },
    ))
}

/// One whole turn: the user's question, the turn's start, one answer and the turn's
/// end. Four source lines, so a test that wants N units asks for N of these.
fn a_turn(state: &mut TuiState, seq: u64, question: &str) {
    state.apply(user_message(seq, question));
    state.apply(turn_started(seq + 1));
    state.apply(message(seq + 2, "回答", None));
    state.apply(turn_ended(seq + 3));
}

/// `count` whole turns, numbered from zero.
fn turns(state: &mut TuiState, count: u64) {
    for index in 0..count {
        a_turn(state, index * 4 + 1, &format!("问题 {index}"));
    }
}

/// The first transcript row's text, which is where a rail jump lands.
fn top_transcript_row(state: &mut TuiState) -> String {
    let rows = screen(120, 24, state);
    rows[TRANSCRIPT_TOP].clone()
}

#[test]
fn the_rail_grows_one_cell_per_turn_and_keeps_the_newest_at_the_foot() {
    // An empty session has an empty column: no cells, and no `⋮` pretending there is
    // history above (spec §4).
    let mut fresh = state();
    assert_eq!(turn_rail_shape(&mut fresh), "");

    let mut state = state();
    turns(&mut state, 3);
    // Three turns, bottom-anchored, and the newest is the bright one.
    assert_eq!(turn_rail_shape(&mut state), "┊┊┃");
    assert_eq!(
        turn_rail_cells(&mut state).len(),
        transcript_rows_at_120x24(),
        "the column is as tall as the transcript and no taller"
    );
    let cells = turn_rail_cells(&mut state);
    assert!(
        cells[transcript_rows_at_120x24() - 3..]
            .iter()
            .all(|ch| *ch != ' '),
        "the three cells sit at the foot: {cells:?}"
    );

    turns(&mut state, 1);
    assert_eq!(
        turn_rail_shape(&mut state),
        "┊┊┊┃",
        "the fourth turn adds a cell"
    );
}

#[test]
fn the_truncation_mark_appears_only_where_units_were_cut() {
    let mut state = state();
    turns(&mut state, 3);
    assert!(
        !turn_rail_shape(&mut state).contains('⋮'),
        "nothing is cut when everything fits"
    );

    // More turns than rows: the newest are shown and the top cell says older ones are
    // above. Nothing is cut at the bottom, because the viewport is at the bottom.
    let mut scrolled = TuiState::new(facts());
    turns(&mut scrolled, 30);
    let shape = turn_rail_shape(&mut scrolled);
    assert_eq!(
        shape.chars().next(),
        Some('⋮'),
        "the top cell says there is more above: {shape}"
    );
    assert_eq!(
        shape.chars().last(),
        Some('┃'),
        "and the newest turn is the foot: {shape}"
    );
    assert_eq!(
        shape.matches('┃').count() + shape.matches('┊').count(),
        transcript_rows_at_120x24() - 1,
        "the mark costs one cell: {shape}"
    );

    // Park the viewport at the very top: now it is the units **below** that are cut.
    let _ = screen(120, 24, &mut scrolled);
    for _ in 0..40 {
        scrolled.key(fs_agent::render::Key::PageUp);
    }
    let shape = turn_rail_shape(&mut scrolled);
    assert_eq!(
        shape.chars().last(),
        Some('⋮'),
        "the foot now says there is more below: {shape}"
    );
    assert_eq!(
        shape.chars().next(),
        Some('┃'),
        "and the focus is the oldest turn on screen: {shape}"
    );
    assert_eq!(
        shape.matches('┃').count(),
        1,
        "exactly one cell is the focus: {shape}"
    );
}

#[test]
fn the_rail_window_follows_the_focus_wherever_the_viewport_is() {
    // The prototype's hole: keeping only the newest cells left a viewport parked on an
    // old unit with **no** bright cell at all
    // (`prototype/frames/120x24-rail-30-units-focus-12-gap.txt`). The window moves with
    // the focus instead, so there is always exactly one — this is that regression.
    let mut state = state();
    turns(&mut state, 30);
    let _ = screen(120, 24, &mut state);

    let visible = transcript_rows_at_120x24();
    // Walk up one page at a time and check the invariant at every stop, including the
    // ones where the focus lands in the middle of the unit list.
    for _ in 0..20 {
        state.key(fs_agent::render::Key::PageUp);
        let shape = turn_rail_shape(&mut state);
        assert_eq!(
            shape.matches('┃').count(),
            1,
            "exactly one bright cell at every position: {shape}"
        );
        assert!(
            shape.chars().filter(|ch| *ch != '⋮').count() <= visible,
            "and no more cells than the column has rows: {shape}"
        );
    }
}

#[test]
fn the_focus_is_the_unit_the_top_row_belongs_to() {
    let mut state = state();
    turns(&mut state, 30);
    let _ = screen(120, 24, &mut state);

    // At the bottom the focus is the newest unit, whatever the top row happens to be —
    // the viewport is following the conversation, and that is what "newest" means.
    assert_eq!(
        turn_rail_cells(&mut state)
            .iter()
            .rposition(|ch| *ch == '┃'),
        Some(transcript_rows_at_120x24() - 1),
        "following the bottom puts the focus on the last row"
    );

    // Scrolled away, the focus is the unit the viewport's top row is inside.
    let question = top_transcript_row(&mut state);
    state.key(fs_agent::render::Key::PageUp);
    let shape = turn_rail_shape(&mut state);
    assert_eq!(shape.matches('┃').count(), 1, "one focus cell: {shape}");
    assert_ne!(
        turn_rail_cells(&mut state)
            .iter()
            .rposition(|ch| *ch == '┃'),
        Some(transcript_rows_at_120x24() - 1),
        "and it is no longer the newest: {shape}"
    );
    assert!(!question.is_empty(), "the top row had text: {question:?}");
}

#[test]
fn clicking_a_rail_cell_jumps_to_that_turns_question() {
    // The story the rail exists for: click a cell, land on the question you asked —
    // top-aligned, so every jump lands where the eye expects (spec §4).
    let mut state = state();
    turns(&mut state, 30);
    let _ = screen(120, 24, &mut state);

    // The window is bottom-anchored under a `⋮`: at 120x24 the transcript is fourteen
    // rows, so one is the mark and thirteen are cells — units 17 through 30, top to
    // bottom. Offset 1 is therefore unit 17 (thirteen turns back from the end) and
    // offset 5 is unit 21 (nine back). Each is clicked in a fresh state, because a
    // jump moves the viewport — and with it the window of cells.
    for (offset, unit) in [(1usize, 17u64), (5, 21)] {
        let mut state = TuiState::new(facts());
        turns(&mut state, 30);
        let _ = screen(120, 24, &mut state);
        let cells = turn_rail_cells(&mut state);
        let mark = cells
            .iter()
            .position(|ch| *ch == '⋮')
            .expect("the column is cut at the top");
        state.mouse(click(RAIL_AT_120, (TRANSCRIPT_TOP + mark + offset) as u16));
        assert!(
            top_transcript_row(&mut state).contains(&format!("[用户] 问题 {unit}")),
            "the jump lands on that turn's own question: {:?}",
            top_transcript_row(&mut state)
        );
    }
}

#[test]
fn a_rail_cell_jump_at_the_end_clamps_to_the_bottom() {
    // The newest unit has less than a screenful left, so the jump clamps — which is the
    // same rule read at the end of the transcript rather than a special case, and it is
    // why clicking the focus cell usually looks like nothing happened (spec §4).
    let mut state = state();
    turns(&mut state, 30);
    let _ = screen(120, 24, &mut state);

    let before = top_transcript_row(&mut state);
    state.mouse(click(
        RAIL_AT_120,
        (TRANSCRIPT_TOP + transcript_rows_at_120x24() - 1) as u16,
    ));
    assert_eq!(
        top_transcript_row(&mut state),
        before,
        "clicking the foot cell keeps the viewport where it was"
    );
    assert_eq!(
        turn_rail_cells(&mut state).last(),
        Some(&'┃'),
        "and the viewport is still following the newest turn"
    );
}

#[test]
fn a_discussion_counts_rounds_where_a_session_counts_turns() {
    use fs_agent::events::{Event, EventPayload, Role, RoundMode, SpeakerId, StopReason};

    // `speaker_order` with more than one debater is what makes a session a discussion,
    // and a discussion counts its rounds — `CONTEXT.md` keeps 轮次 and 回合 apart
    // (spec §4).
    let mut state = TuiState::new(facts_with_roster(&["kimi", "deepseek"]));
    let kimi = SpeakerId::Debater("kimi".into());
    state.apply(user_message(1, "讨论题目"));
    for round in 0..3u32 {
        for (offset, payload) in [
            EventPayload::RoundStarted {
                round,
                mode: RoundMode::Independent,
            },
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text: format!("第 {round} 轮"),
                reasoning: None,
            },
            EventPayload::TurnEnded {
                reason: StopReason::Completed,
            },
            EventPayload::RoundEnded {
                round,
                reason: StopReason::Completed,
            },
        ]
        .into_iter()
        .enumerate()
        {
            state.apply(fs_agent::render::RenderEvent::Logged(Event::new(
                2 + u64::from(round) * 4 + offset as u64,
                kimi.clone(),
                payload,
            )));
        }
    }

    // Three rounds and three `TurnEnded`s: the rail counts the rounds.
    assert_eq!(turn_rail_shape(&mut state), "┊┊┃");

    // The first round's head is the user's question — the one message a discussion does
    // carry, recorded before the first round opened...
    let first = (TRANSCRIPT_TOP + transcript_rows_at_120x24() - 3) as u16;
    state.mouse(click(RAIL_AT_120, first));
    assert!(
        top_transcript_row(&mut state).contains("[用户] 讨论题目"),
        "the session's question is where the first cell lands: {:?}",
        top_transcript_row(&mut state)
    );

    // ...and a later round has no user message of its own, so its cell lands on the
    // round's opening line — the spec's fallback for a unit with nothing to aim at.
    let mut later = TuiState::new(facts_with_roster(&["kimi", "deepseek"]));
    later.apply(user_message(1, "讨论题目"));
    for round in 0..3u32 {
        for (offset, payload) in [
            EventPayload::RoundStarted {
                round,
                mode: RoundMode::Independent,
            },
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text: format!("第 {round} 轮"),
                reasoning: None,
            },
            EventPayload::RoundEnded {
                round,
                reason: StopReason::Completed,
            },
        ]
        .into_iter()
        .enumerate()
        {
            later.apply(fs_agent::render::RenderEvent::Logged(Event::new(
                2 + u64::from(round) * 3 + offset as u64,
                kimi.clone(),
                payload,
            )));
        }
    }
    // Padding, so the transcript is taller than the pane and a jump has somewhere to
    // land: a notice is neither a user message nor a boundary, so the units stand.
    for index in 0..40 {
        later.apply(fs_agent::render::RenderEvent::Notice(format!(
            "第 {index} 行"
        )));
    }
    assert_eq!(turn_rail_shape(&mut later), "┊┊┃");
    let second = (TRANSCRIPT_TOP + transcript_rows_at_120x24() - 2) as u16;
    later.mouse(click(RAIL_AT_120, second));
    assert!(
        top_transcript_row(&mut later).contains("── 第 1 轮"),
        "a round with no user message of its own lands on its first line: {:?}",
        top_transcript_row(&mut later)
    );
}

#[test]
fn the_pane_scrolls_back_through_the_transcript_and_returns_to_the_bottom() {
    use fs_agent::render::{Key, RenderEvent};

    let mut state = state();
    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }

    // At rest the viewport follows the newest row.
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("第 39 行"), "the newest row is on screen");
    assert!(!text.contains("第 0 行"), "the oldest has scrolled off");

    // PgUp leaves the bottom and shows older rows. One page is the pane's own
    // height, so the row at the top is derived from what the pane can show rather
    // than remembered — the mark header changed it, and a fixed row number would
    // have to change again the next time the ladder moves.
    let visible = transcript_rows_at_120x24();
    state.key(Key::PageUp);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        !text.contains("第 39 行"),
        "the newest row gives way: {text}"
    );
    assert!(
        text.contains(&format!("第 {} 行", 40 - visible)),
        "older rows appear: {text}"
    );

    // Ctrl-G comes back, and the viewport follows again.
    state.key(Key::CtrlG);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("第 39 行"), "back at the bottom: {text}");
    assert!(
        !text.contains("第 20 行"),
        "and the old rows are gone: {text}"
    );
}

/// How many transcript rows a 120x24 frame shows.
///
/// A page step and a wheel's reach are both measured in these, so tests that count
/// rows ask for the number instead of remembering it — the shell's chrome has moved
/// it more than once.
fn transcript_rows_at_120x24() -> usize {
    transcript_rows(&screen(120, 24, &mut state()))
}

/// The 120x24 main column, in columns: the wide sidebar (40) and its divider take
/// 1 through 41, so the main column runs 42..119 — and its transcript keeps the last
/// two of those for the scrollbar (117) and the rail (118), which leaves 42..116 for
/// text.
const MAIN_LEFT_AT_120: u16 = 42;
const TRANSCRIPT_TEXT_RIGHT_AT_120: u16 = 117;
const SCROLLBAR_AT_120: u16 = 117;
const RAIL_AT_120: u16 = 118;

/// The transcript's text at 120x24, one string per display row.
///
/// The scrollbar's column is **not** in it: the scrollbar is drawn from the number of
/// rows below the viewport, so it goes on moving while a detail overlay has the pane
/// frozen — comparing it would be comparing the indicator against the reader's
/// position rather than the text against the text.
fn transcript_text(frame: &Buffer, rows: usize) -> Vec<String> {
    (TRANSCRIPT_TOP..TRANSCRIPT_TOP + rows)
        .map(|y| {
            cells(
                frame,
                y as u16,
                MAIN_LEFT_AT_120,
                TRANSCRIPT_TEXT_RIGHT_AT_120,
            )
        })
        .collect()
}

/// The index of the first `第 N 行` notice visible on screen, if any.
fn first_notice(rows: &[String]) -> Option<usize> {
    rows.iter().find_map(|row| {
        let rest = row.split("第 ").nth(1)?;
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    })
}

/// The first cell holding `needle`, as `(column, row)`.
fn find_cell(frame: &Buffer, width: u16, height: u16, needle: &str) -> Option<(u16, u16)> {
    (0..height).find_map(|y| {
        (0..width)
            .find(|x| frame[(*x, y)].symbol() == needle)
            .map(|x| (x, y))
    })
}

#[test]
fn the_transcript_keeps_the_newest_twenty_thousand_source_lines() {
    use fs_agent::render::{Key, RenderEvent};

    // Every notice wraps to three display rows, so a cap counted in display rows
    // would keep a third of this history. That is the distinction this pins: the cap
    // is by source line, so the same history survives at any terminal width (spec §3).
    let mut state = state();
    for index in 0..20_001 {
        state.apply(RenderEvent::Notice(format!(
            "第 {index} 行 {}",
            "x".repeat(200)
        )));
    }
    // One frame first: a page step is measured in the rows the reader can see.
    let _ = screen(120, 24, &mut state);

    // The oldest source line is gone, not merely scrolled off: paging all the way up
    // must not bring it back, and the top is the line that followed it.
    //
    // "All the way up" is reached, not counted: a page step is a pane's worth of
    // display rows, and each notice wraps to two of them, so the number of steps is a
    // function of the terminal — asserting a step count would be asserting the mark
    // header's height through three layers.
    // A frame is only cheap relative to a page step, not free: 20 000 wrapped notices
    // are ~40 000 display rows, so the walk checks its position every few hundred
    // steps rather than after each one.
    let mut previous = None;
    for _ in 0..400 {
        for _ in 0..100 {
            state.key(Key::PageUp);
        }
        let rows = screen(120, 24, &mut state);
        let first = first_notice(&rows);
        if first == previous {
            break;
        }
        previous = first;
    }
    let rows = screen(120, 24, &mut state);
    assert!(
        !rows.join("\n").contains("第 0 行"),
        "the oldest was dropped"
    );
    assert_eq!(
        first_notice(&rows),
        Some(1),
        "the line after it is the oldest now"
    );
}

#[test]
fn the_indicator_counts_what_arrived_and_the_wheel_moves_three_rows() {
    use fs_agent::render::{Key, RenderEvent};
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    let mut state = state();
    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }
    let _ = screen(120, 24, &mut state);

    // Scrolling up with nothing new to read: the indicator is only the way back.
    state.key(Key::PageUp);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("点此到底"), "the way back is offered: {text}");
    assert!(
        !text.contains("行新内容"),
        "nothing has arrived yet: {text}"
    );
    // One page up from the bottom leaves a page's overlap showing: the step is the
    // pane's own height less the two rows the reader keeps, so where the viewport lands
    // is derived from what the pane shows rather than remembered.
    let visible = transcript_rows_at_120x24();
    let after_page_up = first_notice(&screen(120, 24, &mut state));
    assert_eq!(
        after_page_up,
        Some(40 - visible - (visible - 2)),
        "one page up lands on the pane's own height, not a remembered row"
    );

    // A row arrives while the reader is away, and the count is what arrived — not
    // everything that happens to be below the viewport.
    state.apply(RenderEvent::Notice("新的一行".to_owned()));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("↓ 1 行新内容 · 点此到底"),
        "one row arrived: {text}"
    );

    // The wheel moves three rows a notch, up and down.
    let mouse = |kind| MouseEvent {
        kind,
        column: 10,
        row: 10,
        modifiers: KeyModifiers::empty(),
    };
    // Notches move the viewport up and back; how many *notices* a notch crosses
    // depends on how many rows the pane shows, so the two ends are compared with
    // each other rather than with a remembered row — that is what "three rows a
    // notch, and back" means at any terminal size.
    state.mouse(mouse(MouseEventKind::ScrollUp));
    let after_wheel_up = first_notice(&screen(120, 24, &mut state));
    assert!(
        after_wheel_up < after_page_up,
        "the wheel moves the viewport up: {after_page_up:?} -> {after_wheel_up:?}"
    );
    state.mouse(mouse(MouseEventKind::ScrollDown));
    assert_eq!(
        first_notice(&screen(120, 24, &mut state)),
        after_page_up,
        "and one notch down returns to where it was"
    );

    // A click on the indicator goes back to the bottom; a click anywhere else is
    // ignored, because the transcript is the terminal's to select.
    let frame = buffer(120, 24, &mut state);
    let (column, row) = find_cell(&frame, 120, 24, "点").expect("the indicator is on screen");
    // It stops one column short of the scrollbar: `点此到底` is eight columns wide
    // and the transcript's last two columns belong to the scrollbar and the rail, so
    // its last glyph must not straddle the scrollbar's column.
    assert!(
        column + 8 <= SCROLLBAR_AT_120,
        "the indicator stays left of the scrollbar, starting at {column}"
    );
    state.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 10,
        modifiers: KeyModifiers::empty(),
    });
    assert_eq!(
        first_notice(&screen(120, 24, &mut state)),
        after_page_up,
        "a click in the transcript body changes nothing"
    );
    state.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    });
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("新的一行"), "back at the bottom: {text}");
    assert!(
        !text.contains("点此到底"),
        "and the indicator is gone: {text}"
    );
}

#[test]
fn a_resize_keeps_the_reader_on_the_same_line() {
    use fs_agent::render::{Key, RenderEvent};

    let mut state = state();
    // Long enough that two page steps still leave the top row short of the oldest:
    // 40 notices are two pages at a 16-row transcript, which would pin the test's
    // precondition to the shell's own chrome.
    for index in 0..80 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }

    // Following the bottom: a narrower terminal still follows the bottom.
    let narrowed = screen(80, 24, &mut state).join("\n");
    assert!(
        narrowed.contains("第 79 行"),
        "still at the bottom: {narrowed}"
    );

    // Scrolled away: the source line at the top is what survives the rewrap.
    let _ = screen(120, 24, &mut state);
    state.key(Key::PageUp);
    let one_page_up = first_notice(&screen(120, 24, &mut state));
    state.key(Key::PageUp);
    let before = first_notice(&screen(120, 24, &mut state));
    // What this test is about is that the *same source line* stays pinned across the
    // resize — so the precondition is "scrolled away from the bottom", not a
    // particular page-step arithmetic.
    assert!(
        before < one_page_up,
        "two pages up: {one_page_up:?} -> {before:?}"
    );
    assert!(
        before.is_some_and(|row| row > 0),
        "and away from the oldest row: {before:?}"
    );
    let after = first_notice(&screen(80, 24, &mut state));
    assert_eq!(
        after, before,
        "the same line is at the top after the resize"
    );
}

#[test]
fn the_scrollbar_column_is_reserved_and_filled_only_when_there_is_more_to_read() {
    use fs_agent::render::RenderEvent;

    // At 120x24 the main column is 77 columns and the transcript's text is 75 of
    // them: the last two belong to the scrollbar and the rail whether or not
    // anything is drawn in them, so text wraps at 75 and never reflows because those
    // columns are there (spec §1).
    const TEXT_X: u16 = 42;
    const SCROLLBAR_X: u16 = 117;
    let mut state = state();
    state.apply(RenderEvent::Notice("x".repeat(76)));
    let frame = buffer(120, 24, &mut state);
    assert_eq!(
        frame[(TEXT_X, 1)].symbol(),
        "x",
        "the row starts at the first column of the main column"
    );
    assert_eq!(
        frame[(TEXT_X, 2)].symbol(),
        "x",
        "76 columns of text overflow the 75-column text area onto a second row"
    );
    assert_eq!(
        frame[(SCROLLBAR_X, 6)].symbol(),
        " ",
        "nothing is drawn in the reserved column while everything fits"
    );

    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }
    let rows = screen(120, 24, &mut state);
    let transcript = transcript_rows(&rows) as u16;
    let frame = buffer(120, 24, &mut state);
    for y in 1..1 + transcript {
        assert_ne!(
            frame[(SCROLLBAR_X, y)].symbol(),
            " ",
            "a scrollbar appears once the transcript is longer than the pane, row {y}"
        );
    }
}

#[test]
fn the_input_area_holds_three_rows_before_it_grows_and_the_transcript_pays_for_it() {
    // The input sits under the transcript, the status row and their two rules; what a
    // taller draft buys is transcript rows given up, not a block that moves up as a
    // whole. Its floor is three rows, so an empty draft and a three-line one cost the
    // transcript exactly the same — that is what the floor is for
    // (`.scratch/tui-input-pulse/spec.md` §1).
    let mut state = state();
    let empty = screen(80, 24, &mut state);
    let rows = transcript_rows(&empty);
    assert_eq!(
        rows, 14,
        "three input rows leave the transcript fourteen: {empty:#?}"
    );
    let input = TRANSCRIPT_TOP + rows + 3;
    assert!(
        empty[input].contains("> "),
        "the prompt: {:?}",
        empty[input]
    );
    assert!(
        empty[input + 1].trim_matches(['│', ' ']).is_empty()
            && empty[input + 2].trim_matches(['│', ' ']).is_empty(),
        "and the two rows it holds open are blank, not a second block: {:?} / {:?}",
        empty[input + 1],
        empty[input + 2]
    );

    // Three lines of draft move nothing: the box was already that tall, so the
    // transcript keeps every row it had.
    state.paste("第一行\n第二行\n第三行");
    let three = screen(80, 24, &mut state);
    let rows = transcript_rows(&three);
    assert_eq!(
        rows, 14,
        "a three-row draft fits the floor, so the geometry does not move: {three:#?}"
    );
    let input = TRANSCRIPT_TOP + rows + 3;
    assert!(three[input].contains("第一行"), "{:?}", three[input]);
    assert!(
        three[input + 1].contains("第二行"),
        "{:?}",
        three[input + 1]
    );
    assert!(
        three[input + 2].contains("第三行"),
        "{:?}",
        three[input + 2]
    );
    assert!(
        three[input + 4].contains("ctrl-c"),
        "the hints stay under the input: {:?}",
        three[input + 4]
    );

    // From the fourth row on it grows again, one row of transcript per row of draft,
    // and the ten-row cap still holds it (spec §1).
    state.paste("\n第四行\n第五行");
    let five = screen(80, 24, &mut state);
    assert_eq!(
        transcript_rows(&five),
        12,
        "two rows past the floor cost the transcript two: {five:#?}"
    );
    assert!(
        five.join("\n").contains("第五行"),
        "the draft is on screen: {five:#?}"
    );
}

/// One `UsageRecorded`, as the loop would log it.
fn usage(
    seq: u64,
    input: u64,
    output: u64,
    cached: u64,
    miss: u64,
) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, SpeakerId, Usage};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::UsageRecorded {
            usage: Usage {
                input_tokens: input,
                output_tokens: output,
                cached_tokens: cached,
                miss_tokens: miss,
                reasoning_tokens: None,
            },
        },
    ))
}

/// One `TurnEnded`.
fn turn_ended(seq: u64) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, SpeakerId, StopReason};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::TurnEnded {
            reason: StopReason::Completed,
        },
    ))
}

/// The sidebar's first page row: the row under the tab bar's bottom rule.
///
/// The tab bar starts right under the identity — the mark's five rows, the text
/// identity's one, or nothing at all — so a test that counts fields asks for the page
/// rather than for a row number that has to be recomputed whenever that ladder moves.
fn sidebar_page(rows: &[String]) -> usize {
    let mut rules = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.starts_with('├'))
        .map(|(y, _)| y);
    rules.next().expect("the tab bar's top rule");
    rules.next().expect("the tab bar's bottom rule") + 1
}

/// The sidebar's half of one rendered row: from its own left border to the divider,
/// with both columns taken off. Padding is kept, because whether a value is flush
/// right or padded right is exactly what some of these tests are about.
///
/// The divider's column is not always `│`: where a rule of the main column meets it
/// the cell is a junction, and that ends the sidebar too.
fn sidebar_row(row: &str) -> String {
    let inner = row.trim_start_matches('│');
    let end = inner
        .char_indices()
        .find(|(_, ch)| matches!(ch, '│' | '├' | '┤'))
        .map(|(index, _)| index)
        .expect("the divider ends the sidebar");
    inner[..end].to_owned()
}

/// The sidebar's half of one rendered row, by row index.
fn sidebar_field(rows: &[String], row: usize) -> String {
    sidebar_row(&rows[row])
}

/// A sidebar field by its offset from the page's first row: `0` is `上下文`.
fn panel_field(rows: &[String], offset: usize) -> String {
    sidebar_field(rows, sidebar_page(rows) + offset)
}

#[test]
fn the_sidebar_shows_a_zero_and_a_dash_before_any_call() {
    let rows = screen(120, 24, &mut state());
    // The model is the status row's now — it is visible whatever page the sidebar is
    // showing and whatever width the sidebar has (spec §3).
    assert!(
        rows.iter()
            .any(|row| row.contains("模型 claude-sonnet-4-5")),
        "the model is on the status row: {rows:#?}"
    );
    assert!(
        !panel_field(&rows, 0).contains("模型"),
        "and nowhere in the sidebar: {:?}",
        panel_field(&rows, 0)
    );
    assert!(
        panel_field(&rows, 0).contains("上下文")
            && panel_field(&rows, 0).contains(fs_agent::render::wording::PANEL_UNKNOWN),
        "no call has reported usage yet: {:?}",
        panel_field(&rows, 0)
    );
    assert!(
        panel_field(&rows, 1).contains("token") && panel_field(&rows, 1).contains('0'),
        "nothing spent yet: {:?}",
        panel_field(&rows, 1)
    );
    assert!(
        panel_field(&rows, 2).contains("回合") && panel_field(&rows, 2).contains('0'),
        "no turn yet: {:?}",
        panel_field(&rows, 2)
    );
    assert!(
        panel_field(&rows, 3).contains("输入") && panel_field(&rows, 3).contains('0'),
        "nothing in: {:?}",
        panel_field(&rows, 3)
    );
    assert!(
        panel_field(&rows, 5).contains("0 / 0"),
        "and a cache that has never been consulted: {:?}",
        panel_field(&rows, 5)
    );
    assert!(
        !panel_field(&rows, 0).contains("费用") && !rows.join("\n").contains('$'),
        "money is not shown at all (spec §8)"
    );
}

#[test]
fn the_panel_reads_its_numbers_off_the_stream() {
    let mut state = state();
    // input = cached + miss, and neither may be added to the total on top of input.
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));
    state.apply(turn_ended(2));

    let rows = screen(120, 24, &mut state);
    assert!(
        panel_field(&rows, 0).contains("9,000 / 200,000（4%）"),
        "the window is the numerator of the last call: {:?}",
        panel_field(&rows, 0)
    );
    assert!(
        panel_field(&rows, 1).contains("12,345 / 100,000"),
        "spent is input plus output: {:?}",
        panel_field(&rows, 1)
    );
    assert!(
        panel_field(&rows, 2).contains('1'),
        "one turn: {:?}",
        panel_field(&rows, 2)
    );
    assert!(
        panel_field(&rows, 3).contains("9,000"),
        "input: {:?}",
        panel_field(&rows, 3)
    );
    assert!(
        panel_field(&rows, 4).contains("3,345"),
        "output: {:?}",
        panel_field(&rows, 4)
    );
    assert!(
        panel_field(&rows, 5).contains("5,000 / 4,000"),
        "the cache split: {:?}",
        panel_field(&rows, 5)
    );
}

/// A state whose session has no token allowance at all.
fn state_without_budget() -> TuiState {
    let mut facts = facts();
    facts.budget_limit = None;
    TuiState::new(facts)
}

#[test]
fn the_narrow_sidebar_keeps_six_fields_and_drops_the_percentage_when_it_must() {
    // 80x14 is the narrow rung with room for all six readings: twelve content rows
    // hold the text identity, the tab bar and six fields. What the 28-column rung
    // costs is the percentage on the context row — `12,345 / 200,000（6%）` is 22
    // columns and the value column is 21 (spec §2, §3).
    let mut state = state();
    // The output is zero so that the spend — input plus output — is the same five
    // digits the context pair needs, which is what makes the percentage too wide.
    state.apply(usage(1, 12_345, 0, 5_000, 7_345));
    state.apply(turn_ended(2));
    let rows = screen(80, 14, &mut state);
    let text = rows.join("\n");

    assert!(
        rows.iter()
            .any(|row| row.contains("模型 claude-sonnet-4-5")),
        "the model is on the status row: {text}"
    );
    let panel = panel_text(80, 14, &mut state);
    assert_eq!(panel.len(), 6, "all six readings fit: {panel:?}");
    assert!(
        panel[0].starts_with("上下文") && panel[0].ends_with("12,345 / 200,000"),
        "the context pair: {:?}",
        panel[0]
    );
    assert!(
        !panel[0].contains('（'),
        "and the percentage is what the width takes: {:?}",
        panel[0]
    );
    assert!(text.contains("12,345 / 100,000"), "the spend: {text}");
    assert!(text.contains("回合"), "the turns: {text}");
    assert!(
        panel[5].contains("5,000 / 7,345"),
        "the cache split: {:?}",
        panel[5]
    );
}

#[test]
fn a_cache_split_too_wide_for_its_column_is_left_out() {
    // The shell's two rungs both leave room for the split — 21 columns at the narrow
    // rung and 33 at the wide one — so the drop is asserted where it lives, in the
    // panel's own line builder, at a value column narrower than the pair (spec §3).
    use fs_agent::render::panel::Panel;
    use fs_agent::render::Block;
    use ratatui::layout::Rect;

    let facts = facts();
    let block = Block::Usage {
        speaker: fs_agent::events::SpeakerId::System,
        usage: fs_agent::events::Usage {
            input_tokens: 9_000,
            output_tokens: 3_345,
            cached_tokens: 1_234_567,
            miss_tokens: 9_876_543,
            reasoning_tokens: None,
        },
    };
    let mut panel = Panel::new();
    panel.observe(&block);

    // 20 columns leaves 13 for a value; the split needs 21, so the row goes whole.
    let narrow = panel.lines(&facts, Rect::new(0, 0, 20, 6));
    let narrow: Vec<String> = narrow
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect();
    assert!(
        !narrow.iter().any(|row| row.contains("1,234,567")),
        "the cache row does not fit: {narrow:?}"
    );
    // 29 leaves 22, which is exactly enough.
    let wide = panel.lines(&facts, Rect::new(0, 0, 29, 6));
    let wide: String = wide
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect();
    assert!(
        wide.contains("1,234,567 / 9,876,543"),
        "and comes back when it does: {wide}"
    );
}

#[test]
fn a_session_with_no_allowance_shows_its_spend_alone() {
    let mut state = state_without_budget();
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("12,345"), "what was spent: {text}");
    assert!(
        !text.contains("12,345 / "),
        "and no cap is invented for it: {text}"
    );
}

#[test]
fn a_tall_draft_costs_the_transcript_and_never_the_sidebar() {
    // The sidebar is a column of its own, so its height is the terminal's, not the
    // transcript's: a ten-row draft eats transcript rows and leaves the readings
    // exactly where they were (spec §2).
    let mut state = state();
    let draft: String = (0..10).map(|line| format!("第 {line} 行\n")).collect();
    state.paste(&draft);
    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("第 8 行"), "the draft is on screen: {text}");
    assert_eq!(
        transcript_rows(&rows),
        7,
        "the draft takes the transcript's rows: {rows:#?}"
    );
    assert!(
        text.contains("上下文"),
        "the sidebar keeps its readings: {text}"
    );
    assert!(
        text.contains("模型 claude-sonnet-4-5"),
        "and the status row: {text}"
    );
    // The tab bar and the divider are still there: nothing about the sidebar's fate
    // is the draft's to decide.
    assert!(text.contains("调用量") && text.contains('┬'), "{text}");
}

#[test]
fn the_context_numerator_is_the_last_call_while_the_spend_accumulates() {
    let mut state = state();
    state.apply(usage(1, 9_000, 1_000, 5_000, 4_000));
    // A one-digit percentage: `（15%）` would be one column too wide for a 22-column
    // value and get dropped, which the width test covers.
    state.apply(usage(2, 15_000, 2_000, 20_000, 10_000));

    let rows = screen(120, 24, &mut state);
    assert!(
        panel_field(&rows, 0).contains("15,000 / 200,000（7%）"),
        "the window is what the *last* call carried: {:?}",
        panel_field(&rows, 0)
    );
    assert!(
        panel_field(&rows, 1).contains("27,000 / 100,000"),
        "the spend is every call's input plus output: {:?}",
        panel_field(&rows, 1)
    );
    assert!(
        panel_field(&rows, 3).contains("24,000"),
        "input summed: {:?}",
        panel_field(&rows, 3)
    );
    assert!(
        panel_field(&rows, 4).contains("3,000"),
        "output summed: {:?}",
        panel_field(&rows, 4)
    );
}

/// The sidebar's page, one string per row, read out of a rendered frame.
///
/// The page's rows run from under the tab bar to the last field the height ladder
/// kept — the page rectangle is only as tall as its fields, so the first blank row is
/// where it ends.
fn panel_text(width: u16, height: u16, state: &mut TuiState) -> Vec<String> {
    let rows = screen(width, height, state);
    let mut out: Vec<String> = Vec::new();
    for row in rows.iter().skip(sidebar_page(&rows)) {
        if !row.starts_with('│') {
            break;
        }
        let field = sidebar_row(row);
        if field.trim().is_empty() {
            break;
        }
        out.push(field);
    }
    out
}

#[test]
fn the_panel_pads_its_labels_and_aligns_its_values_like_the_snapshot() {
    use fs_agent::render::width::text_columns;

    let mut state = state();
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));
    state.apply(turn_ended(2));
    let panel = panel_text(120, 24, &mut state);

    // Label column is six wide (`上下文` fills it), then one space, then the rest of
    // the sidebar's forty columns as the value field — which numbers fill from the
    // right.
    assert_eq!(
        panel[0],
        format!("上下文{}{}", " ".repeat(13), "9,000 / 200,000（4%）"),
        "the label field is six columns, then a space, then the value right-aligned in the other thirty-three"
    );
    for row in [&panel[1], &panel[3], &panel[4], &panel[5]] {
        assert!(
            row.ends_with("000") || row.ends_with("345"),
            "a number ends at the sidebar's right edge: {row:?}"
        );
    }
    for (index, row) in panel.iter().enumerate() {
        assert!(
            !row.ends_with(' '),
            "row {index} has no padding after its value: {row:?}"
        );
    }
    assert_eq!(panel.len(), 6, "the six readings: {panel:?}");
    for (index, row) in panel.iter().enumerate() {
        assert_eq!(
            text_columns(row),
            40,
            "row {index} fills the sidebar: {row:?}"
        );
    }
}

#[test]
fn a_number_too_wide_for_the_value_column_loses_its_separators_before_its_digits() {
    // The shell's narrow rung leaves 21 columns for a value and seven-digit counts
    // fit in that, so the fallback is asserted where it lives: in the panel's own
    // line builder, at the value column the prototype's 25-column panel used to have
    // (spec §3 keeps the v1 path even though no rung triggers it).
    use fs_agent::render::panel::Panel;
    use fs_agent::render::Block;
    use ratatui::layout::Rect;

    let facts = facts();
    let block = Block::Usage {
        speaker: fs_agent::events::SpeakerId::System,
        usage: fs_agent::events::Usage {
            input_tokens: 1_234_567,
            output_tokens: 1_000,
            cached_tokens: 0,
            miss_tokens: 0,
            reasoning_tokens: None,
        },
    };
    let mut panel = Panel::new();
    panel.observe(&block);
    // 24 columns leaves 17 for a value: `1,235,567 / 100,000` needs 19, so the
    // separators go and the digits stay.
    let rows = panel.lines(&facts, Rect::new(0, 0, 24, 6));
    let row = |index: usize| -> String {
        rows[index]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    };
    assert_eq!(
        row(1),
        "token   1235567 / 100000",
        "spend without separators"
    );
    assert_eq!(
        row(0),
        "上下文  1234567 / 200000",
        "and the window without them"
    );
}

/// The loop asking about a write, as it would through the console channel.
fn ask_permission() -> (
    fs_agent::render::ConsoleRequest,
    tokio::sync::oneshot::Receiver<fs_agent::render::AnswerChoice>,
) {
    use fs_agent::permissions::PermissionRequest;
    use fs_agent::render::{AnswerChoice, AskRequest, ConsoleRequest, Question};
    let (tx, rx) = tokio::sync::oneshot::channel::<AnswerChoice>();
    (
        ConsoleRequest::Ask(AskRequest {
            question: Question::Permission(PermissionRequest {
                request_id: "r-1".to_owned(),
                tool_call_id: "c-1".to_owned(),
                tool_name: "write_file".to_owned(),
                args: serde_json::json!({"path": "a.rs"}),
                reason: "mode ask".to_owned(),
            }),
            reply: tx,
        }),
        rx,
    )
}

#[test]
fn a_permission_question_lands_in_the_middle_as_a_covered_overlay() {
    use fs_agent::render::RenderEvent;

    let mut state = state();
    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }
    let (ask, _rx) = ask_permission();
    state.request(ask);

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("权限询问："), "{text}");
    assert!(text.contains("[y] 允许"), "the keys come with it: {text}");

    let modal = rows
        .iter()
        .position(|row| row.contains("权限询问"))
        .expect("the overlay is on screen");
    assert!(rows[modal].contains('│'), "inside a box: {:?}", rows[modal]);

    // The plan-mode gesture waits: a question owns the keyboard until it is answered
    // (spec §9).
    state.key(fs_agent::render::Key::BackTab);
    assert!(
        state.take_events().is_empty(),
        "Shift-Tab is not a way out of a question"
    );

    // The main column holds the box's own two borders and nothing else: the sidebar's
    // own columns are left of the scan, and the transcript's two trailing columns were
    // blanked with the words the box covers.
    let row = modal as u16;
    let frame = buffer(120, 24, &mut state);
    let borders: Vec<u16> = (MAIN_LEFT_AT_120..TRANSCRIPT_TEXT_RIGHT_AT_120)
        .filter(|x| frame[(*x, row)].symbol() == "│")
        .collect();
    assert_eq!(borders.len(), 2, "the box's borders: {borders:?}");
    assert!(
        borders[0] > MAIN_LEFT_AT_120 && borders[1] < TRANSCRIPT_TEXT_RIGHT_AT_120 - 1,
        "and it is centred in the main column: {borders:?}"
    );
    // The box's own edges, walked out from its left border: the title is its first
    // content row, and the rows that follow are the question's other parts.
    let left = borders[0];
    let box_top = (0..row)
        .rev()
        .find(|y| frame[(left, *y)].symbol() == "┌")
        .expect("the box's top border");
    let box_bottom = (row..24u16)
        .find(|y| frame[(left, *y)].symbol() == "└")
        .expect("the box's bottom border");
    assert_eq!(box_top + 1, row, "the title leads the question");
    // Centred in the main column, which runs from the frame's first content row to
    // its last: the room above and below is the same, within the row the integer
    // division leaves over.
    let above = box_top - TRANSCRIPT_TOP as u16;
    let below = 22 - box_bottom;
    assert!(
        above.abs_diff(below) <= 1,
        "centred: {above} rows above the box, {below} below"
    );
}

#[test]
fn a_question_splits_into_a_title_a_description_a_call_and_a_row_of_buttons() {
    let mut state = state();
    let (ask, _rx) = ask_permission();
    state.request(ask);

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    let row_of = |needle: &str| {
        rows.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} is on screen:\n{text}"))
    };

    // Four parts, top to bottom: what is asked, what the call is *for* (the same words
    // the folded transcript line uses), the concrete call, and the keys that answer it.
    // The title stopped naming the tool and the plain-sentence summary is gone, because
    // the description row says the same thing in the words the reader has already seen
    // (2026-09-23, user request: the popup repeated itself).
    let title = row_of("权限询问：");
    let description = row_of("调用 write_file a.rs");
    let call = row_of("write_file（path=a.rs）");
    let keys = row_of("[y] 允许");
    assert!(
        title < description && description < call && call < keys,
        "title, description, call, then the buttons:\n{text}"
    );
    // Each part keeps its own row: a command can no longer push the keys into the
    // middle of a sentence, and the keys cannot bury the command.
    assert!(
        !rows[keys].contains("path=a.rs") && !rows[keys].contains("权限询问"),
        "the buttons have the row to themselves: {:?}",
        rows[keys]
    );
    assert!(
        !rows[call].contains("调用 write_file"),
        "and the call row carries the call, not the description: {:?}",
        rows[call]
    );
    assert!(
        !rows[description].contains("path=a.rs"),
        "while the description says what it is for, not how to run it: {:?}",
        rows[description]
    );
}

#[test]
fn a_cancelled_run_leaves_no_overlay_behind() {
    // The overlay belongs to the run that raised the question. Once that run is over the
    // loop is not waiting for an answer, so nothing may stay on screen to collect one —
    // the next keypress would go to a question nobody is waiting for (spec §6, §9).
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    let (ask, _answer) = ask_permission();
    state.request(ask);
    let rows = screen(120, 24, &mut state);
    assert!(
        rows.iter().any(|row| row.contains("权限询问")),
        "the overlay is up while its run is:\n{}",
        rows.join("\n")
    );

    state.request(ConsoleRequest::RunState { running: false });
    let rows = screen(120, 24, &mut state);
    assert!(
        !rows.iter().any(|row| row.contains("权限询问")),
        "the overlay went with the run:\n{}",
        rows.join("\n")
    );
}

#[test]
fn a_long_command_still_says_what_it_would_do() {
    use fs_agent::permissions::PermissionRequest;
    use fs_agent::render::{AnswerChoice, AskRequest, Question};

    // The complaint this row answers: a wall of shell is not something a person can
    // read, so the question says what the call is *for* — in the same words the folded
    // transcript line uses — before the wall itself.
    let mut state = state();
    let (tx, _rx) = tokio::sync::oneshot::channel::<AnswerChoice>();
    state.request(ConsoleRequest::Ask(AskRequest {
        question: Question::Permission(PermissionRequest {
            request_id: "r-1".to_owned(),
            tool_call_id: "c-1".to_owned(),
            tool_name: "bash".to_owned(),
            args: serde_json::json!({
                "command": "for f in $(git ls-files '*.rs'); do grep -L 'mod tests' \"$f\"; done | xargs wc -l | sort -n",
            }),
            reason: "mode ask: a write asks the user".to_owned(),
        }),
        reply: tx,
    }));

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    let description = rows
        .iter()
        .position(|row| row.contains("调用 bash"))
        .unwrap_or_else(|| panic!("the description is on screen:\n{text}"));
    let keys = rows
        .iter()
        .position(|row| row.contains("[y] 允许"))
        .unwrap_or_else(|| panic!("the buttons are on screen:\n{text}"));
    assert!(
        description < keys,
        "the description leads the buttons:\n{text}"
    );
    assert!(
        text.contains("git ls-files"),
        "and the command is still there to read:\n{text}"
    );
}

// --- the `/` menu ----------------------------------------------------------

/// Install the names the loop reports: the built-ins it parses, then the skills the
/// session discovered. The renderer has no list of its own — this is the whole menu.
fn install_catalog(state: &mut TuiState) {
    state.request(ConsoleRequest::Catalog {
        entries: [
            ("undo", "回滚上一次编辑"),
            ("plan", "进入硬计划模式"),
            ("endplan", "退出硬计划模式"),
            ("quit", "退出会话"),
            ("ask-matt", "不知道用哪个 skill 时问它"),
            ("review", "审查一个变更"),
        ]
        .iter()
        .map(|(name, description)| CatalogEntry::new(*name, *description))
        .collect(),
    });
}

/// A prompt in flight, so a test can read back what a submission sent.
fn awaiting_line(state: &mut TuiState) -> tokio::sync::oneshot::Receiver<Option<String>> {
    let (reply, line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    line
}

/// Where the `/` menu drew its box: `(x, y, width, height)`.
///
/// Found from the buffer, the way the border assertions above work: the row holding
/// `needle`, the box's left border to the left of it, then its corners.
fn menu_box(frame: &Buffer, width: u16, height: u16, needle: &str) -> (u16, u16, u16, u16) {
    let row = (0..height)
        .find(|y| row_text(frame, *y, width).contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} is on screen"));
    let column = row_text(frame, row, width).find(needle).unwrap() as u16;
    let x = (0..column)
        .rev()
        .find(|c| frame[(*c, row)].symbol() == "│")
        .expect("the menu's left border");
    let top = (0..row)
        .rev()
        .find(|y| frame[(x, *y)].symbol() == "┌")
        .expect("the menu's top border");
    let bottom = (row..height)
        .find(|y| frame[(x, *y)].symbol() == "└")
        .expect("the menu's bottom border");
    let right = (x..width)
        .find(|c| frame[(*c, top)].symbol() == "┐")
        .expect("the menu's right border");
    (x, top, right - x + 1, bottom - top + 1)
}

/// The row the draft is typed on: the prompt is the first thing in the main column,
/// so it follows the sidebar and the divider.
fn input_row(rows: &[String]) -> usize {
    rows.iter()
        .position(|row| row.contains("│> "))
        .expect("the input row")
}

#[test]
fn a_slash_opens_a_menu_of_the_names_the_loop_reported() {
    let mut state = state();
    install_catalog(&mut state);
    state.key(Key::Char('/'));

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    for name in [
        "/undo",
        "/plan",
        "/endplan",
        "/quit",
        "/ask-matt",
        "/review",
    ] {
        assert!(text.contains(name), "{name} is offered:\n{text}");
    }
    assert!(
        text.contains("回滚上一次编辑"),
        "with what it does:\n{text}"
    );

    // It floats — box, borders and all — above the input it belongs to.
    let frame = buffer(120, 24, &mut state);
    let (_, top, _, height) = menu_box(&frame, 120, 24, "/undo");
    assert!(height >= 3, "a box, not a row: {height} tall");
    assert!(
        top as usize + height as usize <= input_row(&rows),
        "the menu sits above the input: {top}+{height} vs {}",
        input_row(&rows)
    );
}

#[test]
fn the_menu_filters_on_what_has_been_typed_after_the_slash() {
    let mut state = state();
    install_catalog(&mut state);
    for ch in "/ask".chars() {
        state.key(Key::Char(ch));
    }
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("/ask-matt"), "{text}");
    assert!(
        !text.contains("/undo"),
        "everything else is filtered out:\n{text}"
    );

    // A prefix that names nothing closes the box rather than showing an empty one.
    for ch in "zzz".chars() {
        state.key(Key::Char(ch));
    }
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        !text.contains("│ /"),
        "no menu for a prefix that names nothing:\n{text}"
    );
}

#[test]
fn the_menu_follows_the_cursor_column() {
    let mut state = state();
    install_catalog(&mut state);
    state.key(Key::Char('/'));
    let frame = buffer(120, 24, &mut state);
    let (before, ..) = menu_box(&frame, 120, 24, "/undo");
    // One more character, and the box moves with the cursor that typed it.
    state.key(Key::Char('a'));
    let frame = buffer(120, 24, &mut state);
    let (after, ..) = menu_box(&frame, 120, 24, "/ask-matt");
    assert_eq!(after, before + 1, "the box moved with the cursor");
}

#[test]
fn tab_fills_the_highlighted_name_in_and_does_not_submit_it() {
    let mut state = state();
    install_catalog(&mut state);
    let mut line = awaiting_line(&mut state);
    for ch in "/as".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Tab);
    assert!(
        line.try_recv().is_err(),
        "Tab fills in; it never sends what is in the draft"
    );
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("│> /ask-matt"),
        "the name is in the draft:\n{text}"
    );
    // And the fill-in closed the menu, so a task can be typed after it.
    assert!(!text.contains("│ /ask-matt"), "the menu is done:\n{text}");
}

#[test]
fn enter_fills_the_highlighted_name_in_and_submits_it_in_one_press() {
    let mut state = state();
    install_catalog(&mut state);
    let mut line = awaiting_line(&mut state);
    for ch in "/ask".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert_eq!(
        line.try_recv().unwrap(),
        Some("/ask-matt".to_owned()),
        "`/ask` + Enter is one gesture, and the name it sent is a complete one"
    );
}

#[test]
fn the_arrows_walk_the_matches() {
    let mut state = state();
    install_catalog(&mut state);
    let mut line = awaiting_line(&mut state);
    // A bare `/` highlights nothing, so the first `↓` takes the first name and the
    // second takes the one after it.
    state.key(Key::Char('/'));
    state.key(Key::Down);
    state.key(Key::Down);
    state.key(Key::Enter);
    assert_eq!(line.try_recv().unwrap(), Some("/plan".to_owned()));
}

#[test]
fn enter_on_a_bare_slash_sends_what_was_typed() {
    // Nothing is highlighted until a name is typed or an arrow walks the list, so an
    // `Enter` pressed only to look at the menu cannot run the first command in it.
    let mut state = state();
    install_catalog(&mut state);
    let mut line = awaiting_line(&mut state);
    state.key(Key::Char('/'));
    state.key(Key::Enter);
    assert_eq!(line.try_recv().unwrap(), Some("/".to_owned()));
}

#[test]
fn an_arrow_on_a_bare_slash_picks_the_row_enter_takes() {
    // The deliberate path: walk the list, take what the highlight landed on.
    let mut state = state();
    install_catalog(&mut state);
    let mut line = awaiting_line(&mut state);
    state.key(Key::Char('/'));
    state.key(Key::Down);
    state.key(Key::Enter);
    assert_eq!(line.try_recv().unwrap(), Some("/undo".to_owned()));
}

#[test]
fn the_arrows_wrap_at_the_ends() {
    let mut state = state();
    install_catalog(&mut state);
    let mut line = awaiting_line(&mut state);
    // Up from the first match wraps to the last one.
    state.key(Key::Char('/'));
    state.key(Key::Up);
    state.key(Key::Enter);
    assert_eq!(line.try_recv().unwrap(), Some("/review".to_owned()));
}

#[test]
fn esc_closes_the_menu_and_leaves_the_draft_where_it_was() {
    let mut state = idle();
    install_catalog(&mut state);
    state.key(Key::Char('/'));
    state.key(Key::Esc);

    // The draft survives: `Esc` closed the menu, it did not start throwing the draft
    // away, and it did not clear a one-line draft either (spec §6, §7).
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("│> /"), "the draft is still there:\n{text}");
    assert!(!text.contains("│ /undo"), "the menu is gone:\n{text}");
    assert!(!text.contains("清空输入"), "nothing was asked:\n{text}");
}

#[test]
fn a_question_hides_the_menu_because_it_owns_the_keyboard() {
    let mut state = state();
    install_catalog(&mut state);
    state.key(Key::Char('/'));
    let (ask, _rx) = ask_permission();
    state.request(ask);

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("权限询问"), "the question is up:\n{text}");
    assert!(
        !text.contains("│ /undo"),
        "and nothing is offering keys that answer something else:\n{text}"
    );
}

#[test]
fn the_menu_keeps_its_corners_over_text_that_is_not_ascii() {
    // A wide glyph owns two cells and the second one is skipped when a frame is diffed
    // to the terminal, so a box whose left border landed there used to lose its
    // top-left corner. The half-covered glyph is blanked instead — half a glyph cannot
    // be drawn anyway — and the box stays a box.
    let mut state = state();
    install_catalog(&mut state);
    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("对话第 {index} 行")));
    }
    state.key(Key::Char('/'));
    let _ = screen(120, 24, &mut state);
    let frame = buffer(120, 24, &mut state);
    let (x, top, width, height) = menu_box(&frame, 120, 24, "/undo");
    let (bottom, right) = (top + height - 1, x + width - 1);
    for y in top..=bottom {
        let (left, rightmost) = if y == top {
            ("┌", "┐")
        } else if y == bottom {
            ("└", "┘")
        } else {
            ("│", "│")
        };
        assert_eq!(frame[(x, y)].symbol(), left, "row {y}, left border");
        assert_eq!(
            frame[(right, y)].symbol(),
            rightmost,
            "row {y}, right border"
        );
    }
}

#[test]
fn there_is_no_menu_before_the_loop_has_said_what_exists() {
    let mut state = state();
    state.key(Key::Char('/'));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        !text.contains("│ /"),
        "an empty catalog offers nothing:\n{text}"
    );
}

#[test]
fn every_question_kind_takes_the_overlay() {
    use fs_agent::render::{AskRequest, ConsoleRequest, Key, Question};
    use std::path::PathBuf;

    // A plan-mode conflict, which the loop asks about too.
    let mut plan = state();
    let (tx, _rx) = tokio::sync::oneshot::channel();
    plan.request(ConsoleRequest::Ask(AskRequest {
        question: Question::PlanConflict(PathBuf::from("/tmp/PLAN.md")),
        reply: tx,
    }));
    let text = screen(120, 24, &mut plan).join("\n");
    assert!(text.contains("计划文件冲突"), "the title: {text}");
    assert!(text.contains("/tmp/PLAN.md 已存在"), "{text}");
    assert!(text.contains("[o] 覆盖"), "with its keys: {text}");

    // An oversized paste.
    let mut paste = state();
    paste.paste(&"x".repeat(100_001));
    let text = screen(120, 24, &mut paste).join("\n");
    assert!(text.contains("粘贴确认"), "the title: {text}");
    assert!(text.contains("粘贴 100001 字符"), "{text}");
    assert!(text.contains("[y] 粘贴"), "with its keys: {text}");

    // And `Esc` on a multi-line draft.
    let mut draft = idle();
    draft.paste("第一行\n第二行");
    draft.key(Key::Esc);
    let text = screen(120, 24, &mut draft).join("\n");
    assert!(text.contains("清空输入"), "{text}");
    assert!(
        text.contains("草稿有多行"),
        "what it would throw away: {text}"
    );
    assert!(text.contains("[y] 清空"), "with its keys: {text}");
}

#[test]
fn a_character_key_answers_the_question_and_never_reaches_the_draft() {
    use fs_agent::permissions::Answer;
    use fs_agent::render::{AnswerChoice, ConsoleRequest, Key};

    let mut state = state();
    let (tx, mut submitted) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    let (ask, mut asked) = ask_permission();
    state.request(ask);

    // `x` is not one of the answers, so the safe one is taken — and the draft, which
    // the question is covering, never sees the key.
    state.key(Key::Char('x'));
    assert_eq!(
        asked.try_recv().unwrap(),
        AnswerChoice::Permission(Answer::Deny)
    );
    state.key(Key::Enter);
    // The draft is empty, so the line that reaches the loop is empty too — and an
    // empty line is a line, not the end of input (the loop discards it).
    assert_eq!(
        submitted.try_recv().unwrap(),
        Some(String::new()),
        "the draft stayed empty"
    );
}

#[test]
fn the_wheel_is_ignored_while_a_question_is_up() {
    use fs_agent::render::{Key, RenderEvent};
    use ratatui::crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};

    let mut state = state();
    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }
    let _ = screen(120, 24, &mut state);
    state.key(Key::PageUp);
    let before = first_notice(&screen(120, 24, &mut state));

    let (ask, _rx) = ask_permission();
    state.request(ask);
    let _ = screen(120, 24, &mut state);
    state.mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 10,
        row: 10,
        modifiers: KeyModifiers::empty(),
    });

    // The modal owns the pointer: the viewport is exactly where it was.
    assert_eq!(first_notice(&screen(120, 24, &mut state)), before);
}

#[test]
fn the_overlay_blanks_what_is_behind_it_rather_than_drawing_over_it() {
    use fs_agent::render::Key;

    // A **short** question: the room it leaves on either side is where the panel's own
    // label sits, so anything left of the background would show up beside the words.
    let mut state = idle();
    state.paste("第一行\n第二行");
    state.key(Key::Esc);
    let rows = screen(120, 24, &mut state);
    let title = rows
        .iter()
        .position(|row| row.contains("清空输入"))
        .expect("the overlay is on screen");
    let keys = rows
        .iter()
        .position(|row| row.contains("[y] 清空"))
        .expect("its buttons are too");

    let frame = buffer(120, 24, &mut state);
    let borders: Vec<u16> = (MAIN_LEFT_AT_120..TRANSCRIPT_TEXT_RIGHT_AT_120)
        .filter(|x| frame[(*x, title as u16)].symbol() == "│")
        .collect();
    assert_eq!(borders.len(), 2, "the box's borders: {borders:?}");
    assert_eq!(
        cells(&frame, title as u16, borders[0] + 1, borders[1]).trim(),
        "清空输入",
        "the interior holds the title and nothing that was behind it"
    );
    assert_eq!(
        cells(&frame, keys as u16, borders[0] + 1, borders[1]).trim(),
        "[y] 清空   [n] 保留",
        "and the buttons are the whole of their own row"
    );
}

/// The rendered frame and where its cursor ended up, when it was shown at all.
fn frame_and_cursor(width: u16, height: u16, state: &mut TuiState) -> (Buffer, Option<(u16, u16)>) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("TestBackend");
    terminal
        .draw(|frame| draw_frame(frame, state))
        .expect("one frame");
    let backend = terminal.backend();
    let cursor = backend.cursor_visible().then(|| {
        let position = backend.cursor_position();
        (position.x, position.y)
    });
    (backend.buffer().clone(), cursor)
}

#[test]
fn the_cursor_comes_back_to_the_draft_once_a_question_is_answered() {
    use fs_agent::permissions::Answer;
    use fs_agent::render::{AnswerChoice, Key};

    let mut state = state();
    for ch in "hi".chars() {
        state.key(Key::Char(ch));
    }
    let (_, before) = frame_and_cursor(120, 24, &mut state);
    let before = before.expect("the cursor sits in the draft");

    let (ask, mut asked) = ask_permission();
    state.request(ask);
    let (_, during) = frame_and_cursor(120, 24, &mut state);
    assert_eq!(during, None, "a question takes the keyboard, so no cursor");

    state.key(Key::Char('y'));
    assert_eq!(
        asked.try_recv().unwrap(),
        AnswerChoice::Permission(Answer::Allow)
    );
    let (_, after) = frame_and_cursor(120, 24, &mut state);
    assert_eq!(after, Some(before), "and it comes back where it was");
}

// ---------------------------------------------------------------------------
// The collapse: thinking lines, tool output, and the detail overlay
// (tickets 01/02/03)
// ---------------------------------------------------------------------------

/// One `MessageCompleted` for the debater.
fn message(seq: u64, text: &str, reasoning: Option<&str>) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, Role, SpeakerId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: text.to_owned(),
            reasoning: reasoning.map(str::to_owned),
        },
    ))
}

/// One reasoning delta from the debater.
fn reasoning_delta(text: &str) -> fs_agent::render::RenderEvent {
    use fs_agent::events::SpeakerId;
    fs_agent::render::RenderEvent::Delta {
        speaker: SpeakerId::Debater("kimi".into()),
        kind: fs_agent::render::DeltaKind::Reasoning,
        text: text.to_owned(),
    }
}

/// One body-text delta from the debater.
fn text_delta(text: &str) -> fs_agent::render::RenderEvent {
    use fs_agent::events::SpeakerId;
    fs_agent::render::RenderEvent::Delta {
        speaker: SpeakerId::Debater("kimi".into()),
        kind: fs_agent::render::DeltaKind::Text,
        text: text.to_owned(),
    }
}

/// One `ToolCallStarted` from the debater.
fn tool_started(
    seq: u64,
    id: &str,
    tool: &str,
    args: serde_json::Value,
) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, SpeakerId, ToolCallId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new(id),
            tool_name: tool.to_owned(),
            args,
        },
    ))
}

/// One `PermissionAsked` about a call.
fn permission_asked(seq: u64, id: &str) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, SpeakerId, ToolCallId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::PermissionAsked {
            request_id: "r-1".to_owned(),
            tool_call_id: ToolCallId::new(id),
            request: serde_json::json!({"tool_name": "bash", "args": {"command": "ls"}}),
        },
    ))
}

/// One `PermissionDecided`: the user said yes.
fn permission_decided(seq: u64) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Decision, DecisionSource, Event, EventPayload, SpeakerId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::PermissionDecided {
            request_id: "r-1".to_owned(),
            decision: Decision::Allow,
            source: DecisionSource::User,
            reason: None,
        },
    ))
}

/// One `ToolCallCompleted` for a call that started earlier.
fn tool_completed(
    seq: u64,
    id: &str,
    ok: bool,
    output: Option<&str>,
    error: Option<&str>,
) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, SpeakerId, ToolCallId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::ToolCallCompleted {
            tool_call_id: ToolCallId::new(id),
            ok,
            output: output.map(str::to_owned),
            error: error.map(str::to_owned),
            duration_ms: 3,
        },
    ))
}

/// A click on a screen cell.
fn click(column: u16, row: u16) -> ratatui::crossterm::event::MouseEvent {
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

#[test]
fn a_thinking_segment_opens_in_place_and_settles_in_place() {
    // The whole state machine, seen from the screen: `正在思考` appears while only
    // reasoning has arrived, the body's first delta settles that same line to
    // `思考完成` without adding a second one, and the finished trace is kept (票 02 §1).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("先看依赖，"));
    state.apply(reasoning_delta("再看测试。"));

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(
        text.contains("[kimi] … 正在思考"),
        "the open line reads as in-progress: {text}"
    );
    assert!(
        !text.contains("思考完成"),
        "and does not claim to be finished: {text}"
    );

    state.apply(text_delta("答案是 42。"));
    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(
        text.contains("[kimi] ▸ ✓ 思考完成"),
        "the same line settles in place: {text}"
    );
    assert!(
        !text.contains("正在思考"),
        "and the in-progress line is gone, not duplicated: {text}"
    );
    assert_eq!(
        text.matches("思考完成").count(),
        1,
        "one thinking segment is one line: {text}"
    );

    // The body still streams live after the freeze; the completed block replaces it.
    state.apply(message(2, "答案是 42。", Some("先看依赖，再看测试。")));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("[kimi] 答案是 42。") || text.contains("答案是 42。"),
        "the answer is in the transcript: {text}"
    );
    assert_eq!(
        text.matches("思考完成").count(),
        1,
        "a recorded trace does not add a second thinking line: {text}"
    );
}

#[test]
fn a_turn_with_no_reasoning_adds_no_thinking_line() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(message(1, "直接作答。", None));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        !text.contains("思考") && !text.contains("正在思考"),
        "no reasoning, no thinking line: {text}"
    );
}

#[test]
fn a_synthesizer_trace_streams_but_records_nothing() {
    // The synthesizer sends reasoning deltas and writes `reasoning: None`. The line
    // still settles — the reader saw it thinking — and its detail says the whole text
    // was never recorded (票 02 §1, 票 04 §3).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("综合两边的意见。"));
    state.apply(message(2, "结论。", None));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("[kimi] ▸ ✓ 思考完成"),
        "the line settles even with no recorded trace: {text}"
    );

    click_row(&mut state, 120, 24, "✓ 思考完成");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("本次未记录思考全文"),
        "the detail says the text was not recorded: {text}"
    );
}

#[test]
fn a_tool_result_is_folded_into_its_call_line() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-1",
        "bash",
        serde_json::json!({"command": "cargo test"}),
    ));
    state.apply(tool_completed(
        2,
        "call-1",
        true,
        Some("line one\nline two\nline three"),
        None,
    ));

    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("[kimi] ▸ 调用 bash 运行 cargo test"),
        "the call line keeps its parameter summary and gains the marker: {text}"
    );
    assert!(
        !text.contains("line one") && !text.contains("line two"),
        "the output body is folded away: {text}"
    );

    // A failure is the same line, with `失败` at its end — never a second line.
    let mut failed = state_with_roster(&["kimi"]);
    failed.apply(tool_started(
        1,
        "call-3",
        "read_file",
        serde_json::json!({"path": "missing.rs"}),
    ));
    failed.apply(tool_completed(
        2,
        "call-3",
        false,
        None,
        Some("no such file"),
    ));
    let rows = screen(120, 24, &mut failed);
    let text = rows.join("\n");
    assert!(
        text.contains("[kimi] ▸ 调用 read_file missing.rs 失败"),
        "the failure is a suffix on the call line: {text}"
    );
    assert!(
        !text.contains("no such file"),
        "and the error body is in the detail, not the transcript: {text}"
    );
}

#[test]
fn a_click_opens_the_detail_and_a_second_click_closes_it() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-9",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-9", true, Some("alpha\nbeta"), None));

    // A tall terminal, so the whole body fits: the shortest overlay scrolls, which is
    // the next test's subject.
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("── 参数 ──"), "the arguments section: {text}");
    assert!(text.contains("── 输出 ──"), "the output section: {text}");
    assert!(text.contains("alpha"), "the whole output: {text}");
    assert!(
        text.contains("esc 关闭"),
        "the footer names the way out: {text}"
    );

    // Esc closes it and the transcript is back.
    state.key(Key::Esc);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("── 参数 ──"),
        "Esc closes the overlay: {text}"
    );
    assert!(
        text.contains("调用 bash"),
        "and the line it opened from is still there: {text}"
    );
}

#[test]
fn the_detail_body_scrolls_with_the_keys_and_the_wheel() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-10",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    let body: Vec<String> = (0..40).map(|line| format!("输出第 {line} 行")).collect();
    state.apply(tool_completed(
        2,
        "call-10",
        true,
        Some(&body.join("\n")),
        None,
    ));

    click_row(&mut state, 120, 40, "调用 bash");
    // The body starts at the top, which at 40 rows is the arguments section.
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("── 参数 ──"),
        "the body starts at the top: {text}"
    );

    // Walk to the very bottom with the page key, then a single arrow moves one row
    // back up: the arrows and the pages both act on the same body.
    for _ in 0..8 {
        state.key(Key::PageDown);
    }
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("esc 关闭"),
        "the body reached the end: {text}"
    );
    state.key(Key::Down);
    state.key(Key::Up);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("esc 关闭"),
        "and the arrows still move inside it: {text}"
    );

    // The wheel moves the same body by one row a notch.
    let before = text.clone();
    state.mouse(wheel(ratatui::crossterm::event::MouseEventKind::ScrollDown));
    let after = screen(120, 40, &mut state).join("\n");
    assert_ne!(before, after, "the wheel moves the detail body");
}

#[test]
fn the_detail_overlay_reads_the_spilled_tool_output() {
    // The event carries the preview; the whole text is the file the tool call id
    // names. `SessionFacts.cwd` is the session directory, so the overlay reads
    // `<cwd>/outputs/<tool_call_id>.txt` (票 02 §4).
    let dir = std::env::temp_dir().join(format!("fs-agent-detail-{}", std::process::id()));
    let outputs = dir.join("outputs");
    std::fs::create_dir_all(&outputs).expect("the session's outputs directory");
    std::fs::write(
        outputs.join("call-11.txt"),
        "the whole output\nwith a second line the preview never carried",
    )
    .expect("the spilled file");

    let mut state = TuiState::new(SessionFacts {
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: dir.display().to_string(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        budget_limit: Some(100_000),
        speaker_order: vec!["kimi".to_owned()],
    });
    state.apply(tool_started(
        1,
        "call-11",
        "bash",
        serde_json::json!({"command": "cat big"}),
    ));
    state.apply(tool_completed(
        2,
        "call-11",
        true,
        Some("the whole output\n[truncated: 999 chars]"),
        None,
    ));

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("with a second line the preview never carried"),
        "the whole spilled text is shown, not the preview: {text}"
    );
    assert!(
        !text.contains("全文不可用"),
        "and no degradation note: {text}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_missing_spilled_file_degrades_to_the_preview() {
    // No `outputs/` directory at all: the detail shows the event's own preview and
    // says the full text is not available (票 02 §4).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-12",
        "bash",
        serde_json::json!({"command": "cat big"}),
    ));
    state.apply(tool_completed(
        2,
        "call-12",
        true,
        Some("head of the output\n[truncated: 999 chars]"),
        None,
    ));

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("head of the output"), "the preview: {text}");
    assert!(
        text.contains("全文不可用"),
        "the degradation is stated: {text}"
    );
}

#[test]
fn a_question_in_the_way_keeps_the_collapsed_lines_unclickable() {
    // A question owns the pointer: a click on a collapsed line while one is up is
    // discarded, and no detail opens over the question (票 02 §4, 票 04 §2).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-13",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-13", true, Some("body"), None));

    // Find the call line's row before the question covers the pane.
    let row = row_of(&mut state, 120, 40, "调用 bash").expect("the call line is drawn");
    state.request(ask_permission().0);
    state.mouse(click(20, row));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("── 参数 ──"),
        "the detail did not open over the question: {text}"
    );
    assert!(
        text.contains("权限询问"),
        "and the question is still the thing on screen: {text}"
    );
}

/// A state whose injected roster is the given debater names, which is what gives the
/// transcript's names their colours (票 07 §1).
fn state_with_roster(names: &[&str]) -> TuiState {
    TuiState::new(SessionFacts {
        speaker_order: names.iter().map(|name| (*name).to_owned()).collect(),
        ..facts()
    })
}

/// One wheel notch.
fn wheel(kind: ratatui::crossterm::event::MouseEventKind) -> ratatui::crossterm::event::MouseEvent {
    use ratatui::crossterm::event::{KeyModifiers, MouseEvent};
    MouseEvent {
        kind,
        column: 40,
        row: 10,
        modifiers: KeyModifiers::empty(),
    }
}

/// The row a phrase is drawn on, rendered fresh.
fn row_of(state: &mut TuiState, width: u16, height: u16, needle: &str) -> Option<u16> {
    let rows = screen(width, height, state);
    rows.iter()
        .position(|row| row.contains(needle))
        .map(|row| row as u16)
}

/// Click the row a phrase is drawn on.
///
/// The frame is rendered first, because only what was drawn can be clicked, and the
/// phrase is looked up in that same frame (票 04 §1).
fn click_row(state: &mut TuiState, width: u16, height: u16, needle: &str) {
    let Some(row) = row_of(state, width, height, needle) else {
        panic!("nothing on screen contains {needle:?}");
    };
    // The click's column only has to be inside the line; the row is what the pane
    // maps back to a source line.
    state.mouse(click(10, row));
}

// ---------------------------------------------------------------------------
// Mouse answers on the two question shapes (ticket 04)
// ---------------------------------------------------------------------------

/// The screen cell a phrase starts on, searching top to bottom.
///
/// The match is against a row read **the way a terminal reads it** — a wide grapheme
/// advances two columns and the cell after it is skipped — so the column a match
/// reports is a screen column, which is what a mouse event carries.
fn cell_of(frame: &Buffer, width: u16, height: u16, needle: &str) -> Option<(u16, u16)> {
    for y in 0..height {
        let row = row_text(frame, y, width);
        if let Some(at) = row.find(needle) {
            return Some((text_columns(&row[..at]) as u16, y));
        }
    }
    None
}

/// Click the first cell where `needle` is drawn on the freshly rendered frame.
fn click_text(state: &mut TuiState, width: u16, height: u16, needle: &str) {
    let frame = buffer(width, height, state);
    let Some((column, row)) = cell_of(&frame, width, height, needle) else {
        panic!("nothing on screen contains {needle:?}");
    };
    state.mouse(click(column, row));
}

/// Click the cell in one screen row where `needle` is drawn.
///
/// The questionnaire's footer labels are also its body's wording, so the two can only
/// be told apart by the row they are on — which is exactly the distinction the pointer
/// makes (票 04 §4).
fn click_in_row(state: &mut TuiState, width: u16, height: u16, row: u16, needle: &str) {
    let frame = buffer(width, height, state);
    let text = row_text(&frame, row, width);
    let Some(at) = text.find(needle) else {
        panic!("row {row} does not contain {needle:?}: {text:?}");
    };
    let column = text_columns(&text[..at]) as u16;
    state.mouse(click(column, row));
}

#[test]
fn a_permission_question_is_answered_by_clicking_a_button() {
    for (label, expected) in [
        (
            "[y] 允许",
            fs_agent::render::AnswerChoice::Permission(fs_agent::permissions::Answer::Allow),
        ),
        (
            "[a] 总是允许",
            fs_agent::render::AnswerChoice::Permission(fs_agent::permissions::Answer::AlwaysAllow),
        ),
        (
            "[n] 拒绝",
            fs_agent::render::AnswerChoice::Permission(fs_agent::permissions::Answer::Deny),
        ),
    ] {
        let mut state = state_with_roster(&["kimi"]);
        let (request, mut answer) = ask_permission();
        state.request(request);
        click_text(&mut state, 120, 24, label);
        assert_eq!(
            answer.try_recv().expect("the answer went out"),
            expected,
            "clicking {label} answers as its key would"
        );
        // The overlay is gone and the keyboard is back on the draft.
        let text = screen(120, 24, &mut state).join("\n");
        assert!(!text.contains("权限询问"), "the question closed: {text}");
    }
}

#[test]
fn clicking_a_question_body_or_border_does_nothing() {
    let mut state = state_with_roster(&["kimi"]);
    let (request, mut answer) = ask_permission();
    state.request(request);

    // The title row and the middle of the overlay's body.
    for (column, row) in [(60, 10), (60, 11), (2, 10)] {
        state.mouse(click(column, row));
    }
    assert!(
        answer.try_recv().is_err(),
        "a click off the buttons answers nothing"
    );
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("权限询问"),
        "and the question is still up: {text}"
    );
}

#[test]
fn a_plan_conflict_and_the_renderer_confirmations_answer_by_click() {
    // Plan conflict: `[o] 覆盖` is the destructive answer, and a click runs it.
    let mut state = state_with_roster(&["kimi"]);
    let (reply, mut answer) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(fs_agent::render::AskRequest {
        question: fs_agent::render::Question::PlanConflict(std::path::PathBuf::from(
            "/tmp/plan.md",
        )),
        reply,
    }));
    click_text(&mut state, 120, 24, "[o] 覆盖");
    assert!(matches!(
        answer.try_recv().expect("the answer went out"),
        fs_agent::render::AnswerChoice::Plan(fs_agent::permissions::PlanConflict::Overwrite)
    ));

    // The exit confirmation is the renderer's own: clicking `[y] 退出` quits.
    let (mut idle, _line) = {
        let mut state = state_with_roster(&["kimi"]);
        let (reply, line) = tokio::sync::oneshot::channel();
        state.request(ConsoleRequest::Prompt { reply });
        (state, line)
    };
    idle.key(Key::CtrlD);
    click_text(&mut idle, 120, 24, "[y] 退出");
    assert!(idle.should_quit(), "the click confirmed the exit");

    // And clicking `[n] 取消` on it leaves the session running.
    let (mut escaped, _line) = {
        let mut state = state_with_roster(&["kimi"]);
        let (reply, line) = tokio::sync::oneshot::channel();
        state.request(ConsoleRequest::Prompt { reply });
        (state, line)
    };
    escaped.key(Key::CtrlD);
    click_text(&mut escaped, 120, 24, "[n] 取消");
    assert!(!escaped.should_quit(), "the safe answer is not the exit");
}

/// A questionnaire with `question` on screen, and its answer receiver.
fn questionnaire_state(
    question: fs_agent::questions::UserQuestion,
) -> (
    TuiState,
    tokio::sync::oneshot::Receiver<Result<fs_agent::questions::UserAnswers, String>>,
) {
    use fs_agent::render::QuestionnaireRequest;
    let mut state = state_with_roster(&["kimi"]);
    let (reply, answers) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Questionnaire(QuestionnaireRequest {
        questions: vec![question],
        reply,
    }));
    (state, answers)
}

#[test]
fn a_single_select_option_is_chosen_by_clicking_its_row() {
    use fs_agent::questions::{Choice, UserQuestion};
    let (mut state, mut answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![
            Choice {
                label: "甲".to_owned(),
                description: None,
            },
            Choice {
                label: "乙".to_owned(),
                description: None,
            },
        ],
    });

    // On the only question, a click answers but does **not** submit: the last
    // question still needs the separate submit (票 04 §4).
    click_text(&mut state, 120, 24, "2. 乙");
    assert!(
        answers.try_recv().is_err(),
        "the last question only answers on a click"
    );

    // `Enter` is the submit once everything is handled.
    state.key(Key::Enter);
    let answers = answers.try_recv().expect("the questionnaire submitted");
    let answers = answers.expect("a successful answer");
    assert_eq!(answers.answers.len(), 1);
    assert_eq!(answers.answers[0].selected, vec!["乙".to_owned()]);
}

#[test]
fn a_multi_select_option_only_toggles_when_clicked() {
    use fs_agent::questions::{Choice, UserQuestion};
    let (mut state, mut answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选几个".to_owned(),
        multi_select: true,
        options: vec![
            Choice {
                label: "甲".to_owned(),
                description: None,
            },
            Choice {
                label: "乙".to_owned(),
                description: None,
            },
        ],
    });

    click_text(&mut state, 120, 24, "1. 甲");
    assert!(
        answers.try_recv().is_err(),
        "a multi-select click does not submit"
    );
    // The tick is on the row the click landed on.
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("[x] 1. 甲"), "the pick is shown: {text}");

    // The footer's submit button appears once something is handled, and submits.
    let row = row_of(&mut state, 120, 40, "提交").expect("the submit button is drawn");
    click_in_row(&mut state, 120, 40, row, "提交");
    let answers = answers.try_recv().expect("the submit button submitted");
    assert_eq!(
        answers.expect("a successful answer").answers[0].selected,
        vec!["甲".to_owned()]
    );
}

#[test]
fn the_questionnaire_footer_pages_with_a_click() {
    use fs_agent::questions::{Choice, UserQuestion};
    let (mut state, mut answers) = {
        use fs_agent::render::QuestionnaireRequest;
        let mut state = state_with_roster(&["kimi"]);
        let (reply, answers) = tokio::sync::oneshot::channel();
        let question = |id: &str| UserQuestion {
            id: id.to_owned(),
            header: None,
            question: format!("第 {id} 题"),
            multi_select: false,
            options: vec![Choice {
                label: "唯一".to_owned(),
                description: None,
            }],
        };
        state.request(ConsoleRequest::Questionnaire(QuestionnaireRequest {
            questions: vec![question("q1"), question("q2")],
            reply,
        }));
        (state, answers)
    };

    // The first question has no `← 上一题`; it does have `下一题 →`.
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        !text.contains("← 上一题"),
        "no previous on the first: {text}"
    );
    assert!(text.contains("下一题 →"), "but a next: {text}");

    // A click advances, and the second question offers the way back.
    click_in_row(&mut state, 120, 24, 22, "下一题 →");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("2 / 2"), "the click paged forward: {text}");
    assert!(
        text.contains("← 上一题"),
        "and the way back appears: {text}"
    );
    click_in_row(&mut state, 120, 24, 22, "← 上一题");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("1 / 2"), "the click paged back: {text}");
    assert!(answers.try_recv().is_err(), "paging never submits");
}

#[test]
fn clicking_the_custom_row_hands_it_the_cursor_and_paging_takes_it_back() {
    use fs_agent::questions::{Choice, UserQuestion};
    let (mut state, _answers) = {
        use fs_agent::render::QuestionnaireRequest;
        let mut state = state_with_roster(&["kimi"]);
        let (reply, answers) = tokio::sync::oneshot::channel();
        let question = |id: &str| UserQuestion {
            id: id.to_owned(),
            header: None,
            question: format!("第 {id} 题"),
            multi_select: false,
            options: vec![Choice {
                label: "唯一".to_owned(),
                description: None,
            }],
        };
        state.request(ConsoleRequest::Questionnaire(QuestionnaireRequest {
            questions: vec![question("q1"), question("q2")],
            reply,
        }));
        (state, answers)
    };

    let (_, before) = frame_and_cursor(120, 24, &mut state);
    assert_eq!(before, None, "no cursor until the row is focused");

    click_text(&mut state, 120, 24, "自定义：");
    let (_, focused) = frame_and_cursor(120, 24, &mut state);
    assert!(
        focused.is_some(),
        "the click put the cursor on the custom row"
    );

    // Paging away resets the focus: the next question's custom row starts unfocused.
    click_in_row(&mut state, 120, 24, 22, "下一题 →");
    let (_, after) = frame_and_cursor(120, 24, &mut state);
    assert_eq!(after, None, "the page turn reset the focus");
}

#[test]
fn the_wheel_moves_the_questionnaire_highlight() {
    use fs_agent::questions::{Choice, UserQuestion};
    let (mut state, _answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![
            Choice {
                label: "甲".to_owned(),
                description: None,
            },
            Choice {
                label: "乙".to_owned(),
                description: None,
            },
        ],
    });
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("> ○ 1. 甲"),
        "the highlight starts on 1: {text}"
    );

    state.mouse(wheel(ratatui::crossterm::event::MouseEventKind::ScrollDown));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("> ○ 2. 乙"),
        "the wheel moved the highlight: {text}"
    );
}

#[test]
fn one_message_never_gets_two_thinking_lines() {
    // The body's first delta settles the line long before `MessageCompleted`
    // arrives — and the completion carries the whole trace, so a naive "open if not
    // open" would add a second line for the same thought. No frame is drawn between
    // the events here, which is what made the earlier version of this test pass
    // (票 02 §1).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("先看依赖。"));
    state.apply(text_delta("答案。"));
    state.apply(message(1, "答案。", Some("先看依赖。")));
    let text = screen(120, 40, &mut state).join("\n");
    assert_eq!(
        text.matches("思考完成").count(),
        1,
        "one thinking segment is one line: {text}"
    );
    assert!(
        !text.contains("正在思考"),
        "and the in-progress line is gone: {text}"
    );
}

#[test]
fn reasoning_never_joins_the_message_body() {
    // Reasoning is folded into its own line; letting it through to the live tail would
    // print the raw thought under `正在思考` (票 02 §3).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("这是不该出现的思考正文。"));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("… 正在思考"),
        "the thinking line is there: {text}"
    );
    assert!(
        !text.contains("这是不该出现的思考正文"),
        "and the raw reasoning is not: {text}"
    );
}

#[test]
fn the_detail_overlay_freezes_the_transcript() {
    // A reader who opened a line keeps looking at it: output that arrives while the
    // overlay is up must not pull the pane to the bottom (票 02 §4).
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..40 {
        state.apply(fs_agent::render::RenderEvent::Notice(format!(
            "第 {index} 行"
        )));
    }
    state.apply(tool_started(
        1,
        "call-20",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-20", true, Some("body"), None));
    let _ = screen(120, 24, &mut state);

    let _ = screen(120, 24, &mut state);
    click_row(&mut state, 120, 24, "调用 bash");
    let before = screen(120, 24, &mut state);
    assert!(
        before.join("\n").contains("── 参数 ──"),
        "the overlay is up"
    );
    let frozen = transcript_text(&buffer(120, 24, &mut state), transcript_rows(&before));

    // New output arrives while the overlay is open.
    for index in 40..60 {
        state.apply(fs_agent::render::RenderEvent::Notice(format!(
            "第 {index} 行"
        )));
    }
    let during = screen(120, 24, &mut state);
    let after = transcript_text(&buffer(120, 24, &mut state), transcript_rows(&during));
    assert_eq!(
        after, frozen,
        "the transcript behind the overlay did not move"
    );
    // The rows that arrived are not counted at the reader either: the overlay is
    // holding the position, so "N new rows" would be counting under it.
    let counter = |rows: &[String]| {
        rows.iter()
            .find(|row| row.contains("行新内容"))
            .cloned()
            .unwrap_or_default()
    };
    assert_eq!(
        counter(&during),
        counter(&before),
        "and the new-rows count is held too"
    );
}

#[test]
fn a_question_closes_the_detail_overlay_instead_of_stacking_on_it() {
    // A question may not be drawn over the overlay: the modal underneath would be
    // unanswerable, because the overlay is what owns the keyboard (票 02 §4).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-21",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-21", true, Some("body"), None));
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("── 参数 ──"), "the overlay opened: {text}");

    state.request(ask_permission().0);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("── 参数 ──"),
        "the overlay stood down for the question: {text}"
    );
    assert!(text.contains("权限询问"), "and the question is up: {text}");
}

#[test]
fn the_questionnaire_footer_buttons_hit_where_they_are_drawn() {
    // The gaps between the footer's buttons are painted as well as counted, so the
    // region a click lands in is the button the glyphs are in — this pins the bug
    // where a counted-but-unpainted gap put every region three columns out
    // (票 04 §7).
    use fs_agent::questions::{Choice, UserQuestion};
    let (mut state, mut answers) = {
        use fs_agent::render::QuestionnaireRequest;
        let mut state = state_with_roster(&["kimi"]);
        let (reply, answers) = tokio::sync::oneshot::channel();
        let question = |id: &str| UserQuestion {
            id: id.to_owned(),
            header: None,
            question: format!("第 {id} 题"),
            multi_select: false,
            options: vec![Choice {
                label: "唯一".to_owned(),
                description: None,
            }],
        };
        state.request(ConsoleRequest::Questionnaire(QuestionnaireRequest {
            questions: vec![question("q1"), question("q2")],
            reply,
        }));
        (state, answers)
    };

    // On the first question only `下一题 →` is drawn. Clicking its glyphs advances.
    click_in_row(&mut state, 120, 24, 22, "下一题 →");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("2 / 2"), "the click advanced: {text}");

    // On the second, `← 上一题` is drawn first: clicking it goes back, and do not let
    // it land on anything else.
    click_in_row(&mut state, 120, 24, 22, "← 上一题");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("1 / 2"), "the click went back: {text}");
    assert!(answers.try_recv().is_err(), "paging never submits");
}

#[test]
fn a_thinking_line_tints_its_speakers_name() {
    // The thinking hint carries a `speaker_label`, so its name takes the speaker's
    // colour while the marker and the state word stay the narration grey
    // (票 07 §2).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("先看依赖。"));
    state.apply(text_delta("答案。"));

    let frame = buffer(120, 40, &mut state);
    let Some((column, row)) = cell_of(&frame, 120, 40, "✓ 思考完成") else {
        panic!("the thinking line is on screen");
    };
    // The name sits just before the marker.
    let name_x = column - text_columns("▸ ") as u16 - text_columns("[kimi] ") as u16;
    assert_eq!(
        frame[(name_x, row)].symbol(),
        "[",
        "the name prefix is there"
    );
    assert_eq!(
        frame[(name_x, row)].fg,
        Color::LightCyan,
        "the name takes the first roster slot"
    );
    assert_eq!(
        frame[(column, row)].fg,
        Color::DarkGray,
        "and the state word stays narration grey"
    );
}

#[test]
fn reasoning_that_interleaves_opens_a_new_line_per_segment() {
    // Reasoning and body deltas interleave, so a single message can hold several
    // thinking segments: each one settles where it stands and the next opens its own
    // line (票 02 §1).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("第一段思考。"));
    state.apply(text_delta("第一段正文。"));
    state.apply(reasoning_delta("第二段思考。"));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("… 正在思考"),
        "the second segment is open: {text}"
    );
    assert_eq!(
        text.matches("思考完成").count(),
        1,
        "and the first is settled: {text}"
    );

    state.apply(message(1, "第一段正文。", Some("第一段思考。第二段思考。")));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("正在思考"),
        "the completion settles the open segment: {text}"
    );
}

#[test]
fn ctrl_d_closes_the_detail_overlay_rather_than_asking_to_quit() {
    // The one exception to "the overlay ignores every other key": `Ctrl-D` closes it
    // instead of opening the exit confirmation (票 06 §5).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-22",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-22", true, Some("body"), None));
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("── 参数 ──"), "the overlay opened: {text}");

    state.key(Key::CtrlD);
    assert!(!state.should_quit(), "closing is not quitting");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains("── 参数 ──"), "the overlay closed: {text}");
    assert!(
        !text.contains("退出会话"),
        "and no confirmation was asked: {text}"
    );
}

#[test]
fn a_tool_body_over_the_reading_limit_is_cut_and_says_so() {
    // The spilled file has no cap of its own, so the reader's does the cutting and
    // the body says it happened (票 02 §4).
    let dir = std::env::temp_dir().join(format!("fs-agent-detail-big-{}", std::process::id()));
    let outputs = dir.join("outputs");
    std::fs::create_dir_all(&outputs).expect("the session's outputs directory");
    let big = "x".repeat(200_001);
    std::fs::write(outputs.join("call-23.txt"), &big).expect("the spilled file");

    let mut state = TuiState::new(SessionFacts {
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: dir.display().to_string(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        budget_limit: Some(100_000),
        speaker_order: vec!["kimi".to_owned()],
    });
    state.apply(tool_started(
        1,
        "call-23",
        "bash",
        serde_json::json!({"command": "cat big"}),
    ));
    // The event's `output` is the **cut** preview, which is what says there is a whole
    // file to go and read — a preview with no marker in it is the whole body, and the
    // detail does not go looking for a file that was never written.
    state.apply(tool_completed(
        2,
        "call-23",
        true,
        Some(
            "head\n[truncated: 200001 chars, ~50000 tokens; full output at \
             /tmp/nonexistent-elsewhere/call-23.txt]\ntail",
        ),
        None,
    ));
    click_row(&mut state, 120, 40, "调用 bash");

    // The mark is far below the visible body, so walk to the end of it.
    for _ in 0..4000 {
        state.key(Key::PageDown);
    }
    let rows = screen(120, 40, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("已截断"), "the cut is stated: {text}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_detail_overlay_ignores_every_key_but_its_own() {
    // The overlay owns the keyboard: `Esc` and `Ctrl-D` close it, the arrows and the
    // page keys scroll it, and **everything else is ignored** — `Ctrl-C` included, so
    // a stray gesture cannot quit or cancel out from under a reader (票 02 §4).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-24",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-24", true, Some("body"), None));
    click_row(&mut state, 120, 40, "调用 bash");

    state.key(Key::CtrlC);
    assert!(
        !state.should_quit(),
        "Ctrl-C does not quit from the overlay"
    );
    assert!(
        state.take_events().is_empty(),
        "and it does not cancel anything either"
    );
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("── 参数 ──"),
        "the overlay is still up: {text}"
    );

    // A printable key does not reach the draft either: the overlay's keys are the
    // overlay's, and the editor behind it is not being typed into.
    state.key(Key::Char('x'));
    let rows = screen(120, 40, &mut state);
    let input = rows
        .iter()
        .position(|row| row.contains("> "))
        .expect("the input row is drawn");
    assert!(
        !rows[input].contains('x'),
        "nothing landed in the draft: {:?}",
        rows[input]
    );
}

#[test]
fn a_tool_call_is_on_screen_as_soon_as_its_result_arrives() {
    // The transcript used to hold a call open until some *later* unrelated event closed
    // it, so the call line only appeared once the model had already answered its next
    // iteration: for the whole tool run, and until then, the transcript showed nothing
    // about the call at all (票 02 §3). The result is what ends the call, and it is
    // what has to paint it.
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-30",
        "bash",
        serde_json::json!({"command": "ls -la"}),
    ));
    state.apply(tool_completed(2, "call-30", true, Some("total 0"), None));

    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("调用 bash 查看"),
        "the call line is up as soon as the result is: {text}"
    );

    // A permission question asked **about this call** does not delay it either: the
    // narration is painted while the call is still in flight, and the call line arrives
    // on the result regardless.
    let mut asked = state_with_roster(&["kimi"]);
    asked.apply(tool_started(
        1,
        "call-31",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    asked.apply(permission_asked(2, "call-31"));
    asked.apply(permission_decided(3));
    let text = screen(120, 24, &mut asked).join("\n");
    assert!(
        text.contains("权限询问"),
        "the question is narrated first: {text}"
    );
    assert!(
        !text.contains("调用 bash"),
        "and the call is not painted before its result: {text}"
    );
    asked.apply(tool_completed(4, "call-31", true, Some("out"), None));
    let text = screen(120, 24, &mut asked).join("\n");
    assert!(
        text.contains("调用 bash 查看"),
        "the call line is up as soon as the result is: {text}"
    );
}

#[test]
fn the_detail_overlay_is_wider_than_a_question() {
    // The body is a page, so it gets the room: the ceiling went 90 → 135 and below 139
    // columns what caps it is the overlay's own margin, not the ceiling (票 03
    // §Answer，2026-09-23 加宽 50%）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-41",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-41", true, Some("body"), None));
    click_row(&mut state, 120, 40, "调用 bash");
    let frame = buffer(120, 40, &mut state);
    let detail = overlay_width(&frame, 120, 40).expect("the overlay's top border");

    // A question's overlay is narrower, and it is painted the same way: ask one and
    // measure it.
    let mut asked = state_with_roster(&["kimi"]);
    asked.request(ask_permission().0);
    let frame = buffer(120, 40, &mut asked);
    let question = overlay_width(&frame, 120, 40).expect("the question's top border");

    assert!(
        detail > question,
        "the detail overlay ({detail}) is wider than a question's ({question})"
    );
    // At 120 columns the main column is what caps both: 77 columns less the four the
    // margin keeps leaves 73 for the detail (its 135-column ceiling is not reached)
    // and 72 for the question.
    assert_eq!(detail, 73, "the detail overlay at 120 columns");
    assert_eq!(question, 72, "and the question's 72-column ceiling");

    // The ceiling needs a terminal wide enough for the main column to outgrow it:
    // 200 columns leaves 157, and the four-column margin then leaves 153 — past the
    // 135 the detail overlay never exceeds.
    let mut wide = state_with_roster(&["kimi"]);
    wide.apply(tool_started(
        1,
        "call-41",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    wide.apply(tool_completed(2, "call-41", true, Some("body"), None));
    click_row(&mut wide, 200, 40, "调用 bash");
    let frame = buffer(200, 40, &mut wide);
    let detail = overlay_width(&frame, 200, 40).expect("the overlay's top border");
    assert_eq!(
        detail, 135,
        "and on a wide terminal the ceiling is what caps it"
    );
}

/// The painted width of a floating box, read off the row its top border is on.
///
/// The overlay is centred in the main column, whose own border sits one column
/// outside it on either side — so the box is found by looking for a `┌` that is **not**
/// in the first column, and measured to its matching `┐`.
fn overlay_width(frame: &Buffer, width: u16, height: u16) -> Option<u16> {
    for y in 0..height {
        // Columns, not byte offsets: a row with CJK in it is longer in bytes than it is
        // wide, and the box is measured in columns.
        let mut left = None;
        for x in 1..width {
            match frame[(x, y)].symbol() {
                "┌" if left.is_none() => left = Some(x),
                "┐" => {
                    if let Some(left) = left {
                        return Some(x - left + 1);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

#[test]
fn a_click_outside_the_detail_overlay_closes_it() {
    // The whole of the frame outside the overlay is a close target: the line it came
    // from, the transcript around it, the panel, the footer (票 02 §4，2026-09-23
    // 修正，原先只认「再点同一行」）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-40",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-40", true, Some("body"), None));
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("── 参数 ──"), "the overlay opened: {text}");

    // Outside: the margin the overlay leaves inside the main column, on the row the
    // line it came from sits on.
    let row = row_of(&mut state, 120, 40, "调用 bash").expect("the call line");
    state.mouse(click(MAIN_LEFT_AT_120, row));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("── 参数 ──"),
        "a click on the transcript closed it: {text}"
    );

    // Inside: nothing happens, because the overlay has no buttons of its own — and
    // that includes the screen row the line was on when the overlay covers it. The old
    // "click the same row again" is **not** what closes a covered line; the reliable
    // ways out are `Esc`, `Ctrl-D`, and a click outside (票 02 §4，2026-09-23 修正).
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("── 参数 ──"), "reopened: {text}");
    state.mouse(click(60, 12));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("── 参数 ──"),
        "a click inside leaves it up: {text}"
    );
    // What decides is the **screen position**, not which transcript line is under it:
    // a click inside the overlay's rectangle does nothing. So when the line the overlay
    // was opened from sits under the overlay — which is where it usually is, since the
    // overlay covers most of the main column — clicking there does nothing either, and
    // `Esc` / `Ctrl-D` / a click outside are the ways out (票 02 §4，2026-09-23 修正).
    // The "inside does nothing" click above is that case: (60, 12) is inside the
    // overlay's rectangle at this size, between the two borders read back here.
    let frame = buffer(120, 40, &mut state);
    let overlay = overlay_width(&frame, 120, 40).expect("the overlay");
    assert_eq!(
        overlay, 73,
        "at 120 columns the main column, not the ceiling, is what caps the overlay"
    );
    assert_eq!(
        (frame[(44, 12)].symbol(), frame[(116, 12)].symbol()),
        ("│", "│"),
        "and those are its two edges on the row that click landed on"
    );
}

#[test]
fn a_settling_thinking_line_keeps_the_history_before_it() {
    // A live session draws frames **between** events, and that is what fills the pane's
    // wrap cache. A thinking line then settles *in place*, and that rewrite must not
    // throw away the display rows of everything before it.
    //
    // It did. `replace_last` cleared the whole wrapped cache while `starts` kept
    // pointing at the old offsets, so the pane came back with almost no rows: the
    // history vanished, the pane stopped filling its height, and PgUp had nothing to
    // scroll. Reported as "bash 命令都没了 / 输出没有占满屏幕 / PgUp 没有反映"
    // (2026-09-23).
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..40 {
        state.apply(fs_agent::render::RenderEvent::Notice(format!(
            "第 {index} 行"
        )));
    }
    // A frame first, then the live thinking segment with a frame in the middle of it.
    let _ = screen(120, 24, &mut state);
    state.apply(reasoning_delta("先想一下。"));
    let _ = screen(120, 24, &mut state);
    state.apply(text_delta("答案。"));

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(
        text.contains("▸ ✓ 思考完成"),
        "the thinking line settled: {text}"
    );
    assert!(
        text.contains("答案。"),
        "and the body that settled it is there: {text}"
    );
    let notices = rows.iter().filter(|row| row.contains("第 ")).count();
    assert!(
        notices >= 5,
        "the history before it is still on screen ({notices} rows): {text}"
    );
    state.key(Key::PageUp);
    let rows = screen(120, 24, &mut state);
    assert!(
        rows.iter().any(|row| row.contains("第 2")),
        "and PgUp still reaches further back: {}",
        rows.join("\n")
    );
}

#[test]
fn a_complete_result_does_not_claim_its_text_is_unavailable() {
    // Only a result that was **cut** has a spilled file; a short one was never written
    // anywhere, and its preview *is* the whole body. Saying `全文不可用` about it is a
    // lie the reader cannot see through — it reads as "this detail is incomplete"
    // (2026-09-23, user report: every bash detail ended with `--- stderr ---
    // 全文不可用`).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-50",
        "bash",
        serde_json::json!({"command": "echo hi"}),
    ));
    state.apply(tool_completed(
        2,
        "call-50",
        true,
        Some("退出码 0\n--- stdout ---\nhi\n--- stderr ---\n"),
        None,
    ));
    click_row(&mut state, 120, 40, "调用 bash");

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("--- stderr ---"),
        "the tool's own sections are shown as they are: {text}"
    );
    assert!(
        !text.contains("全文不可用"),
        "and no degradation is claimed for a body that was never cut: {text}"
    );
}

#[test]
fn a_cut_result_still_says_when_the_whole_text_is_gone() {
    // The other half of the same rule: a preview that really was cut, with no file to
    // read it back from, keeps the note.
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-51",
        "bash",
        serde_json::json!({"command": "cat big"}),
    ));
    state.apply(tool_completed(
        2,
        "call-51",
        true,
        Some("head\n[truncated: 999 chars, ~250 tokens; full output at /x/outputs/call-51.txt]\ntail"),
        None,
    ));
    click_row(&mut state, 120, 40, "调用 bash");

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("全文不可用"),
        "a cut body with no readable file says so: {text}"
    );
}

#[test]
fn a_tool_call_line_describes_the_call_and_folds_the_arguments_away() {
    // `调用 工具 描述`: the reader sees what the call was *for*, and the concrete
    // arguments are one click away in the detail (票 02 §2，2026-09-23 修正).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-60",
        "bash",
        serde_json::json!({"command": "find .scratch -type f"}),
    ));
    state.apply(tool_completed(2, "call-60", true, Some("out"), None));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("[kimi] ▸ 调用 bash 查询 .scratch"),
        "the line describes the call: {text}"
    );
    assert!(
        !text.contains("find .scratch -type f"),
        "and the arguments are not printed on it: {text}"
    );

    // The questionnaire describes itself by the question's own header.
    let mut asked = state_with_roster(&["kimi"]);
    asked.apply(tool_started(
        1,
        "call-61",
        "ask_user_question",
        serde_json::json!({"questions": [{"id": "q", "header": "下一步", "question": "接着做哪个？"}]}),
    ));
    asked.apply(tool_completed(2, "call-61", false, None, Some("declined")));
    let text = screen(120, 40, &mut asked).join("\n");
    assert!(
        text.contains("[kimi] ▸ 调用 ask_user_question 下一步 失败"),
        "the question's own summary describes the call: {text}"
    );

    // The arguments are still in the detail, under their own heading.
    click_row(&mut asked, 120, 40, "调用 ask_user_question");
    let text = screen(120, 40, &mut asked).join("\n");
    assert!(text.contains("── 参数 ──"), "the args section: {text}");
    assert!(
        text.contains("下一步"),
        "and it carries the concrete arguments: {text}"
    );
}

#[test]
fn the_call_line_wears_the_narration_grey_after_its_speakers_name() {
    // The description is narration, not the model's answer, so it wears the same grey
    // as the thinking line; the name keeps the speaker's colour (2026-09-23).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-62",
        "bash",
        serde_json::json!({"command": "ls -la"}),
    ));
    state.apply(tool_completed(2, "call-62", true, Some("out"), None));
    let frame = buffer(120, 40, &mut state);

    let Some((name_x, row)) = cell_of(&frame, 120, 40, "[kimi]") else {
        panic!("the call line is on screen");
    };
    let Some((call_x, _)) = cell_of(&frame, 120, 40, "调用 bash") else {
        panic!("the call line is on screen");
    };
    assert_eq!(
        frame[(name_x, row)].fg,
        Color::LightCyan,
        "the name keeps the speaker's colour"
    );
    assert_eq!(
        frame[(call_x, row)].fg,
        Color::DarkGray,
        "and the description wears the narration grey"
    );
}

#[test]
fn the_detail_overlay_wears_the_speakers_colour_and_keeps_a_cell_of_air() {
    // The border is the speaker's colour — whose line you are reading, before a word of
    // it — and the words sit a cell inside it (2026-09-23).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-63",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-63", true, Some("out"), None));
    click_row(&mut state, 120, 40, "调用 bash");
    let frame = buffer(120, 40, &mut state);

    // The overlay's own corner: a `┌` that is not the middle block's.
    let mut corner = None;
    for y in 0..40 {
        for x in 2..120 {
            if frame[(x, y)].symbol() == "┌" {
                corner = Some((x, y));
                break;
            }
        }
        if corner.is_some() {
            break;
        }
    }
    let (x, y) = corner.expect("the overlay's top-left corner");
    assert_eq!(
        frame[(x, y)].fg,
        Color::LightCyan,
        "the border wears the speaker's colour"
    );
    assert_eq!(
        (
            frame[(x + 1, y + 1)].symbol(),
            frame[(x + 2, y + 1)].symbol()
        ),
        (" ", " "),
        "the cells just inside the border are air, on both axes"
    );
    assert_eq!(
        frame[(x + 2, y + 2)].symbol(),
        "[",
        "and the title starts a cell in from the border on both"
    );
}

/// The overlay's painted box on screen: its left column and its width.
fn overlay_box(frame: &Buffer, width: u16, height: u16) -> Option<(u16, u16)> {
    for y in 0..height {
        let mut left = None;
        for x in 1..width {
            match frame[(x, y)].symbol() {
                "┌" if left.is_none() => left = Some(x),
                "┐" => {
                    if let Some(left) = left {
                        return Some((left, x - left + 1));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

#[test]
fn the_permission_modals_buttons_are_centred() {
    // The body is a centred paragraph and the buttons are a shorter line: centring them
    // by their own width is what puts them under the words instead of off to the left
    // (2026-09-23, user report).
    let mut state = state_with_roster(&["deepseek"]);
    state.request(ask_permission().0);
    let frame = buffer(120, 24, &mut state);
    let (x, width) = overlay_box(&frame, 120, 24).expect("the modal's box");

    let (first, row) = cell_of(&frame, 120, 24, "[y] 允许").expect("the first button");
    let (last, _) = cell_of(&frame, 120, 24, "[n] 拒绝").expect("the last button");
    let last_end = last as usize + text_columns("[n] 拒绝");
    let left_gap = first as usize - (x as usize + 1);
    let right_gap = (x as usize + width as usize - 1) - last_end;
    assert!(
        row == cell_of(&frame, 120, 24, "[n] 拒绝")
            .expect("the last button")
            .1,
        "the buttons are one row"
    );
    assert!(
        left_gap.abs_diff(right_gap) <= 1,
        "the button row is centred: {left_gap} columns of air on the left, {right_gap} on the right"
    );
}

#[test]
fn the_detail_footer_counts_the_last_row_on_screen() {
    // A reader who has scrolled to the bottom is at the bottom: the footer counts the
    // last row on screen, not the row the window happens to start at. It read `94/154`
    // with the last row visible (2026-09-23, user report).
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-70",
        "bash",
        serde_json::json!({"command": "seq 1 200"}),
    ));
    let body: Vec<String> = (1..=200).map(|n| format!("第 {n} 行")).collect();
    state.apply(tool_completed(
        2,
        "call-70",
        true,
        Some(&body.join("\n")),
        None,
    ));
    click_row(&mut state, 120, 40, "调用 bash");

    // The footer's two numbers, wherever they are on screen.
    let counts = |state: &mut TuiState| -> (usize, usize) {
        let rows = screen(120, 40, state);
        let row = rows
            .iter()
            .find(|row| row.contains('↕'))
            .expect("the footer is drawn");
        let tail = row.split('↕').nth(1).expect("after the marker");
        let pair = tail.split('·').next().expect("before the key").trim();
        let (seen, total) = pair.split_once('/').expect("a pair");
        (
            seen.trim().parse().expect("a number"),
            total.trim().parse().expect("a number"),
        )
    };

    let (seen, total) = counts(&mut state);
    assert!(
        total > 100,
        "the body is long enough to scroll: {seen}/{total}"
    );
    assert!(seen < total, "and the window starts short of the end");

    for _ in 0..50 {
        state.key(Key::PageDown);
    }
    let (seen, total) = counts(&mut state);
    assert_eq!(
        seen, total,
        "at the bottom the footer reads the last row: {seen}/{total}"
    );
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("第 200 行"),
        "and the last row really is on screen: {text}"
    );
}

/// A permission question about a shell command, the way the loop asks one.
fn ask_bash(command: &str) -> fs_agent::render::ConsoleRequest {
    use fs_agent::permissions::PermissionRequest;
    use fs_agent::render::{AskRequest, Question};
    let (reply, _answer) = tokio::sync::oneshot::channel();
    ConsoleRequest::Ask(AskRequest {
        question: Question::Permission(PermissionRequest {
            request_id: "r-1".to_owned(),
            tool_call_id: "c-1".to_owned(),
            tool_name: "bash".to_owned(),
            args: serde_json::json!({ "command": command }),
            reason: "mode ask".to_owned(),
        }),
        reply,
    })
}

#[test]
fn the_permission_question_describes_the_call_the_way_the_line_does() {
    // The question and the folded line it is about are produced by **one** function, so
    // they cannot drift apart — and the call as it will run stays under them, because
    // approving is the moment the exact command has to be readable (2026-09-23, user
    // request: the popup should read like the line, both kept).
    let command = "head -5 README.md";
    let mut state = state_with_roster(&["deepseek"]);
    // The same call, first folded into the transcript...
    state.apply(tool_started(
        1,
        "call-80",
        "bash",
        serde_json::json!({ "command": command }),
    ));
    state.apply(tool_completed(2, "call-80", true, Some("out"), None));
    // ...and then asked about.
    state.request(ask_bash(command));

    let rows = screen(120, 40, &mut state);
    let text = rows.join("\n");
    assert_eq!(
        text.matches("调用 bash 查看 README.md").count(),
        2,
        "the folded line and the question carry the same description: {text}"
    );
    let description = rows
        .iter()
        .position(|row| row.contains("调用 bash 查看 README.md") && row.contains('│'))
        .expect("the description row in the overlay");
    let call = rows
        .iter()
        .position(|row| row.contains("bash（command=head -5 README.md）"))
        .expect("the exact call is still shown");
    assert!(
        description < call,
        "and the exact call comes after it: {text}"
    );
}
