//! The fullscreen four-pane layout, rendered into a `TestBackend` so the geometry
//! can be asserted without a terminal (spec §2, §Testing Decisions).
//!
//! The seam is [`draw_frame`]: a state goes in, a fixed-size buffer comes out. Every
//! assertion below is about what a person would see — which regions exist, what the
//! header says, how many hints fit — never about the rectangles the layout computes
//! on the way there.

use fs_agent::render::{draw_frame, SessionFacts, TuiState};
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::Terminal;

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

/// Render one frame at a fixed size and read the screen back as rows of text.
fn screen(width: u16, height: u16, state: &TuiState) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("TestBackend");
    terminal
        .draw(|frame| draw_frame(frame, state))
        .expect("one frame");
    let buffer = terminal.backend().buffer();
    (0..height).map(|y| row_text(buffer, y, width)).collect()
}

/// One row of the buffer as text.
///
/// A wide grapheme covers the cell after it, and that cell holds a space in the
/// backend's buffer; reading it verbatim would spell `终 端` for `终端`. Advancing
/// by the symbol's display width is what makes the row read like the terminal.
fn row_text(buffer: &Buffer, y: u16, width: u16) -> String {
    let mut text = String::new();
    let mut x = 0;
    while x < width {
        let symbol = buffer[(x, y)].symbol();
        text.push_str(symbol);
        x += symbol.cell_width().max(1);
    }
    text
}

/// The rendered frame itself, for assertions about a particular cell.
fn buffer(width: u16, height: u16, state: &TuiState) -> Buffer {
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
        let rows = screen(width, height, &state());
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
fn a_wide_terminal_draws_the_header_the_transcript_and_the_bottom_block() {
    // 120x24 is the reference size: two header lines, twelve transcript rows, one
    // input row, one hint row, and a blank row above and below the middle block
    // (spec §2).
    let rows = screen(120, 24, &state());

    // The header block: two content lines between its borders.
    assert!(rows[0].starts_with('┌'), "the header opens: {:?}", rows[0]);
    assert!(rows[3].starts_with('└'), "the header closes: {:?}", rows[3]);
    assert!(
        rows[0].ends_with('┐') && rows[3].ends_with('┘'),
        "the header block is closed on the right too: {:?} / {:?}",
        rows[0],
        rows[3]
    );
    assert!(
        rows[1].contains("fs-agent") && rows[1].contains(':'),
        "the identity and the clock share the first line: {:?}",
        rows[1]
    );
    assert!(
        rows[2].contains("~/code/fortystory/fs-agent") && rows[2].contains("模式 询问"),
        "the directory and the mode share the second line: {:?}",
        rows[2]
    );

    // The airy rows: the layout breathes between the blocks.
    assert_eq!(rows[4].trim(), "", "a blank row under the header");
    assert_eq!(rows[19].trim(), "", "a blank row above the bottom block");

    // The middle block spans the transcript.
    assert!(rows[5].starts_with('┌'), "the middle opens: {:?}", rows[5]);
    assert!(
        rows[18].starts_with('└'),
        "the middle closes: {:?}",
        rows[18]
    );

    // The bottom block: the input line and the hints, in that order.
    assert!(
        rows[20].starts_with('┌'),
        "the bottom opens: {:?}",
        rows[20]
    );
    assert!(
        rows[23].starts_with('└'),
        "the bottom closes: {:?}",
        rows[23]
    );
    assert!(rows[21].contains("> "), "the input prompt: {:?}", rows[21]);
    assert!(
        rows[22].contains("ctrl-c 退出"),
        "the hints name the way out: {:?}",
        rows[22]
    );

    assert!(
        !rows.join("\n").contains("shift+enter"),
        "no phantom newline key"
    );
}

#[test]
fn a_floor_sized_terminal_still_draws_every_region() {
    // 40x10 is inside the minimum: one header line, one transcript row, one input
    // row and one hint row, with the airy rows given up to keep them (spec §2).
    let rows = screen(40, 10, &state());
    for (row, line) in rows.iter().enumerate() {
        assert!(
            line.starts_with('┌') || line.starts_with('│') || line.starts_with('└'),
            "row {row} belongs to a block: {line:?}"
        );
    }
    assert!(
        rows[1].contains("fs-agent") && rows[1].contains("模式 询问") && rows[1].contains(':'),
        "one header line carries the identity, the mode and the clock: {:?}",
        rows[1]
    );
    assert!(
        !rows[1].contains("~/code"),
        "the narrow header drops the directory: {:?}",
        rows[1]
    );
    assert!(rows[7].contains("> "), "the input row: {:?}", rows[7]);
    assert!(
        rows[8].contains("ctrl-c 退出"),
        "the hint row: {:?}",
        rows[8]
    );
}

#[test]
fn the_information_panel_shares_a_seam_with_the_transcript_only_when_there_is_room() {
    // 120 columns: the panel is drawn, and the two panes share one column — a
    // vertical rule that meets the middle block's borders with junctions rather
    // than doubling them (spec §2).
    let frame = buffer(120, 24, &state());
    let seam = 89;
    assert_eq!(
        frame[(seam, 5)].symbol(),
        "┬",
        "the seam meets the top border"
    );
    assert_eq!(frame[(seam, 18)].symbol(), "┴", "and the bottom border");
    for y in 6..=17 {
        assert_eq!(
            frame[(seam, y)].symbol(),
            "│",
            "the seam runs down at y={y}"
        );
    }

    // 60 columns is below the panel's minimum width: the transcript takes the
    // whole middle block and no seam is drawn at all.
    let narrow = buffer(60, 24, &state());
    let text: String = (0..24)
        .map(|y| row_text(&narrow, y, 60))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !text.contains('┬') && !text.contains('┴'),
        "no panel below 80 columns: {text}"
    );

    // 80x16 is the smallest terminal that fits the panel: four middle rows, which
    // is all the four core fields need.
    let smallest = buffer(80, 16, &state());
    let middle_top = (0..16)
        .find(|y| smallest[(0, *y)].symbol() == "┌" && row_text(&smallest, *y, 80).contains('┬'))
        .expect("the panel is drawn at 80x16");
    assert!(middle_top > 0);
}

/// How many `·`-separated items the hint row holds, the state word included.
fn hint_items(width: u16) -> Vec<String> {
    let rows = screen(width, 24, &state());
    let row = rows
        .iter()
        .find(|row| row.contains("ctrl-c 退出"))
        .expect("the hint row is on screen");
    row.trim_matches(|ch| ch == '│' || ch == ' ')
        .split(" · ")
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_hint_row_gives_up_hints_before_it_gives_up_the_way_out() {
    // The measured ladder: three items at 40 columns, four at 60, five at 80, six
    // at 120 — and the state word joins them on the left once they fit (spec §10).
    assert_eq!(hint_items(40).len(), 3, "40 columns: {:?}", hint_items(40));
    assert_eq!(hint_items(60).len(), 5, "60 columns: {:?}", hint_items(60));
    assert_eq!(hint_items(80).len(), 6, "80 columns: {:?}", hint_items(80));
    assert_eq!(
        hint_items(120).len(),
        7,
        "120 columns: {:?}",
        hint_items(120)
    );

    // At the floor the state word is the first thing to go, because giving up a
    // hint to keep it would cost the newline key — the one hint that explains how
    // to write a second line at all.
    let floor = hint_items(40);
    assert_eq!(floor[0], "enter 发送", "the send hint survives: {floor:?}");
    assert!(
        floor.contains(&"ctrl-j 换行".to_owned()),
        "so does the newline hint: {floor:?}"
    );
    assert_eq!(floor.last().unwrap(), "ctrl-c 退出");

    // From 60 columns up the state word is there, leftmost.
    let roomy = hint_items(60);
    assert_eq!(roomy[0], "就绪", "the state word comes back: {roomy:?}");
    assert!(
        roomy.contains(&"esc 取消".to_owned()),
        "and one more hint fits: {roomy:?}"
    );

    // The long list only appears where there is room for it.
    assert!(
        hint_items(120).contains(&"PgUp/PgDn 滚动".to_owned()),
        "the scroll hint is for wide terminals: {:?}",
        hint_items(120)
    );
    assert!(
        !screen(174, 24, &state()).join("\n").contains("shift+enter"),
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

    let text = screen(120, 24, &state).join("\n");
    assert!(
        text.contains("fs-agent：会话 abc"),
        "the notice is a transcript line: {text}"
    );
    assert!(
        text.contains("正在读文件"),
        "the streaming tail is in the pane too: {text}"
    );
}
