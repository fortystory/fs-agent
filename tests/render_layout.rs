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
fn a_wide_terminal_draws_the_header_the_transcript_and_the_bottom_block() {
    // 120x24 is the reference size: two header lines, twelve transcript rows, one
    // input row, one hint row, and a blank row above and below the middle block
    // (spec §2).
    let rows = screen(120, 24, &mut state());

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
    let rows = screen(40, 10, &mut state());
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
    let frame = buffer(120, 24, &mut state());
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
    let narrow = buffer(60, 24, &mut state());
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
    let smallest = buffer(80, 16, &mut state());
    let middle_top = (0..16)
        .find(|y| smallest[(0, *y)].symbol() == "┌" && row_text(&smallest, *y, 80).contains('┬'))
        .expect("the panel is drawn at 80x16");
    assert!(middle_top > 0);
}

/// How many `·`-separated items the hint row holds, the state word included.
fn hint_items(width: u16) -> Vec<String> {
    let rows = screen(width, 24, &mut state());
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

#[test]
fn every_size_in_the_matrix_draws_the_regions_its_budget_allows() {
    // The rest of the matrix the geometry table covers, each asserted for the
    // reason it is in the table: 40x12 is where airy comes back at 40 columns,
    // 80x24 is the panel with room to spare, and 174x50 is the ceiling — the panel
    // is capped at 31 columns and the transcript takes the rest.
    for (width, height) in [(40, 12), (80, 24), (174, 50)] {
        let rows = screen(width, height, &mut state());
        let text = rows.join("\n");
        assert!(
            !text.contains("终端太小"),
            "{width}x{height} is inside the minimum: {text}"
        );
        assert!(
            rows[0].starts_with('┌') && rows[0].ends_with('┐'),
            "{width}x{height} opens a header block: {:?}",
            rows[0]
        );
        assert!(
            text.contains("ctrl-c 退出"),
            "{width}x{height} keeps the way out: {text}"
        );
    }

    // 40x12 has the airy row under the header; 40x10 (covered above) does not.
    let airy = screen(40, 12, &mut state());
    assert_eq!(airy[3].trim(), "", "airy returns at 40x12: {:?}", airy[3]);
    let floor = screen(40, 10, &mut state());
    assert_ne!(
        floor[3].trim(),
        "",
        "and is given up at 40x10: {:?}",
        floor[3]
    );

    // 174x50 is wide enough that the panel is at its 31-column cap: the seam sits
    // 31 columns from the right edge.
    let wide = buffer(174, 50, &mut state());
    let seam = 174 - 31;
    assert_eq!(wide[(seam, 5)].symbol(), "┬", "the seam at the cap");
    assert_eq!(
        wide[(174 - 1, 5)].symbol(),
        "┐",
        "the panel ends at the right border"
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

    // PgUp leaves the bottom and shows older rows.
    state.key(Key::PageUp);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        !text.contains("第 39 行"),
        "the newest row gives way: {text}"
    );
    assert!(text.contains("第 20 行"), "older rows appear: {text}");

    // Ctrl-G comes back, and the viewport follows again.
    state.key(Key::CtrlG);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("第 39 行"), "back at the bottom: {text}");
    assert!(
        !text.contains("第 20 行"),
        "and the old rows are gone: {text}"
    );
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
    for _ in 0..8_000 {
        state.key(Key::PageUp);
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
    assert_eq!(first_notice(&screen(120, 24, &mut state)), Some(18));

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
    state.mouse(mouse(MouseEventKind::ScrollUp));
    assert_eq!(
        first_notice(&screen(120, 24, &mut state)),
        Some(15),
        "three rows up"
    );
    state.mouse(mouse(MouseEventKind::ScrollDown));
    assert_eq!(
        first_notice(&screen(120, 24, &mut state)),
        Some(18),
        "three rows back down"
    );

    // A click on the indicator goes back to the bottom; a click anywhere else is
    // ignored, because the transcript is the terminal's to select.
    let frame = buffer(120, 24, &mut state);
    let (column, row) = find_cell(&frame, 120, 24, "点").expect("the indicator is on screen");
    // It stops one column short of the scrollbar: `点此到底` is eight columns wide
    // and the pane's last column belongs to the scrollbar, so its last glyph must
    // not straddle that column.
    assert!(
        column + 8 <= 88,
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
        Some(18),
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
    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }

    // Following the bottom: a narrower terminal still follows the bottom.
    let narrowed = screen(80, 24, &mut state).join("\n");
    assert!(
        narrowed.contains("第 39 行"),
        "still at the bottom: {narrowed}"
    );

    // Scrolled away: the source line at the top is what survives the rewrap.
    let _ = screen(120, 24, &mut state);
    state.key(Key::PageUp);
    state.key(Key::PageUp);
    let before = first_notice(&screen(120, 24, &mut state));
    assert_eq!(before, Some(8), "two pages up");
    let after = first_notice(&screen(80, 24, &mut state));
    assert_eq!(
        after, before,
        "the same line is at the top after the resize"
    );
}

#[test]
fn the_scrollbar_column_is_reserved_and_filled_only_when_there_is_more_to_read() {
    use fs_agent::render::RenderEvent;

    // 120x24 leaves the transcript 88 content columns; the last of them belongs to
    // the scrollbar whether or not anything is drawn in it, so text wraps at 87.
    let mut state = state();
    state.apply(RenderEvent::Notice("x".repeat(88)));
    let frame = buffer(120, 24, &mut state);
    assert_eq!(
        frame[(1, 6)].symbol(),
        "x",
        "the row starts at the first column"
    );
    assert_eq!(
        frame[(1, 7)].symbol(),
        "x",
        "88 columns of text overflow the 87-column text area"
    );
    assert_eq!(
        frame[(88, 6)].symbol(),
        " ",
        "nothing is drawn in the reserved column while everything fits"
    );

    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }
    let frame = buffer(120, 24, &mut state);
    for y in 6..18 {
        assert_ne!(
            frame[(88, y)].symbol(),
            " ",
            "a scrollbar appears once the transcript is longer than the pane, row {y}"
        );
    }
}

#[test]
fn the_input_area_grows_with_the_draft_and_the_transcript_gives_up_the_rows() {
    let mut state = state();
    let one = screen(80, 24, &mut state);
    assert!(one[20].starts_with('┌'), "one input row: {:?}", one[20]);
    assert!(one[21].contains("> "), "the prompt: {:?}", one[21]);

    state.paste("第一行\n第二行\n第三行");
    let three = screen(80, 24, &mut state);
    assert!(
        three[18].starts_with('┌'),
        "the bottom block moved up: {:?}",
        three[18]
    );
    assert!(three[19].contains("第一行"), "{:?}", three[19]);
    assert!(three[20].contains("第二行"), "{:?}", three[20]);
    assert!(three[21].contains("第三行"), "{:?}", three[21]);
    assert!(
        three[22].contains("ctrl-c 退出"),
        "the hints stay under the input: {:?}",
        three[22]
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

#[test]
fn the_panel_names_the_model_and_shows_a_zero_and_a_dash_before_any_call() {
    let rows = screen(120, 24, &mut state());
    assert!(
        rows[6].contains("模型") && rows[6].contains("claude-sonnet-4-5"),
        "the model: {:?}",
        rows[6]
    );
    assert!(
        rows[7].contains("上下文") && rows[7].contains(fs_agent::render::wording::PANEL_UNKNOWN),
        "no call has reported usage yet: {:?}",
        rows[7]
    );
    assert!(
        rows[8].contains("token") && rows[8].contains('0'),
        "nothing spent yet: {:?}",
        rows[8]
    );
    assert!(
        rows[9].contains("回合") && rows[9].contains('0'),
        "no turn yet: {:?}",
        rows[9]
    );
    assert!(
        rows[10].contains("输入") && rows[10].contains('0'),
        "nothing in: {:?}",
        rows[10]
    );
    assert!(
        rows[12].contains("0 / 0"),
        "and a cache that has never been consulted: {:?}",
        rows[12]
    );
    assert!(
        !rows[6].contains("费用") && !rows.join("\n").contains('$'),
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
    assert!(rows[6].contains("claude-sonnet-4-5"), "{:?}", rows[6]);
    assert!(
        rows[7].contains("9,000 / 200,000（4%）"),
        "the window is the numerator of the last call: {:?}",
        rows[7]
    );
    assert!(
        rows[8].contains("12,345 / 100,000"),
        "spent is input plus output: {:?}",
        rows[8]
    );
    assert!(rows[9].contains('1'), "one turn: {:?}", rows[9]);
    assert!(rows[10].contains("9,000"), "input: {:?}", rows[10]);
    assert!(rows[11].contains("3,345"), "output: {:?}", rows[11]);
    assert!(
        rows[12].contains("5,000 / 4,000"),
        "the cache split: {:?}",
        rows[12]
    );
}

/// A state whose session has no token allowance at all.
fn state_without_budget() -> TuiState {
    let mut facts = facts();
    facts.budget_limit = None;
    TuiState::new(facts)
}

#[test]
fn a_short_narrow_panel_keeps_the_four_core_fields_and_drops_the_rest() {
    // 80x16 is the smallest terminal that draws the panel at all: 23 columns of
    // content and four rows.
    let mut state = state();
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));
    state.apply(turn_ended(2));
    let rows = screen(80, 16, &mut state);
    let text = rows.join("\n");

    assert!(text.contains("模型"), "the model: {text}");
    // Exactly the plain value: the percentage would have to be truncated to fit, and
    // a truncated number reads as a smaller one.
    let panel = panel_text(80, 16, &mut state);
    assert_eq!(
        panel[1], "上下文  9,000 / 200,000",
        "the percentage is the first thing the width takes"
    );
    assert!(text.contains("12,345 / 100,000"), "the spend: {text}");
    assert!(text.contains("回合"), "the turns: {text}");
    assert!(
        !text.contains("缓存") && !text.contains("3,345"),
        "the detail rows are the first thing the height takes: {text}"
    );
}

#[test]
fn a_cache_split_too_wide_for_the_panel_is_left_out() {
    let mut state = state();
    state.apply(usage(1, 9_000, 3_345, 1_234_567, 9_876_543));

    // 80 columns leaves 16 for a value; the split needs 21, so it goes.
    let narrow = screen(80, 24, &mut state).join("\n");
    assert!(
        !narrow.contains("1,234,567"),
        "the cache row does not fit: {narrow}"
    );
    // 120 leaves 22, which is exactly enough.
    let wide = screen(120, 24, &mut state).join("\n");
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
fn a_tall_draft_takes_the_panel_away_whole() {
    // The counter-intuitive one the prototype measured: ten rows of draft leave
    // three rows of middle, and three rows cannot hold the four core fields — so the
    // whole panel goes rather than showing a third of it.
    let mut state = state();
    let draft: String = (0..10).map(|line| format!("第 {line} 行\n")).collect();
    state.paste(&draft);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("第 8 行"), "the draft is on screen: {text}");
    assert!(!text.contains("模型"), "the panel is gone: {text}");
    assert!(!text.contains('┬'), "and so is its seam: {text}");
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
        rows[7].contains("15,000 / 200,000（7%）"),
        "the window is what the *last* call carried: {:?}",
        rows[7]
    );
    assert!(
        rows[8].contains("27,000 / 100,000"),
        "the spend is every call's input plus output: {:?}",
        rows[8]
    );
    assert!(rows[10].contains("24,000"), "input summed: {:?}", rows[10]);
    assert!(rows[11].contains("3,000"), "output summed: {:?}", rows[11]);
}

/// The panel's content, one string per row, read out of a rendered frame.
///
/// The panel sits right of the shared seam and left of the middle block's border, so
/// the seam column is what locates it.
fn panel_text(width: u16, height: u16, state: &mut TuiState) -> Vec<String> {
    let frame = buffer(width, height, state);
    let (seam, top) = find_cell(&frame, width, height, "┬").expect("the panel is drawn");
    let content = (seam + 1)..(width - 1);
    // `┬` sits on the middle block's top border, so the content starts below it.
    ((top + 1)..height)
        .map(|y| cells(&frame, y, content.start, content.end))
        .take_while(|row| !row.trim().is_empty())
        // Padding is kept: whether a value is flush right or padded right is exactly
        // what these tests are about.
        .collect()
}

#[test]
fn the_panel_pads_its_labels_and_aligns_its_values_like_the_snapshot() {
    use fs_agent::render::width::text_columns;

    let mut state = state();
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));
    state.apply(turn_ended(2));
    let panel = panel_text(120, 24, &mut state);

    // Label column is six wide (`上下文` fills it), then one space, then twenty-two
    // columns of value.
    assert_eq!(
        panel[1], "上下文  9,000 / 200,000（4%）",
        "the label field is six columns and the value is right-aligned in the rest"
    );
    // Text fills from the left of the value field; numbers from the right.
    assert_eq!(panel[0].trim_end(), "模型   claude-sonnet-4-5");
    assert!(
        panel[0].ends_with("     "),
        "the model is padded on its right: {:?}",
        panel[0]
    );
    for row in [&panel[2], &panel[5], &panel[6]] {
        assert!(
            row.ends_with("000") || row.ends_with("345"),
            "a number ends at the panel's right edge: {row:?}"
        );
        assert!(!row.ends_with(' '), "and has no padding after it: {row:?}");
    }
    // Seven rows, each filling the panel's 29 columns.
    assert_eq!(panel.len(), 7, "{panel:?}");
    for (index, row) in panel.iter().enumerate() {
        assert_eq!(
            text_columns(row),
            29,
            "row {index} fills the panel: {row:?}"
        );
    }
}

#[test]
fn a_number_too_wide_for_the_value_column_loses_its_separators_before_its_digits() {
    let mut state = state();
    // Seven-digit counts: `1,235,567 / 100,000` needs 19 of the 16 columns the panel
    // has at 80 wide, so the separators go and the digits stay.
    state.apply(usage(1, 1_234_567, 1_000, 0, 0));
    let panel = panel_text(80, 24, &mut state);
    assert_eq!(
        panel[2], "token  1235567 / 100000",
        "spend without separators"
    );
    assert_eq!(
        // `上下文` fills the label column exactly, so only the separator follows it.
        panel[1],
        "上下文 1234567 / 200000",
        "and the window without them"
    );
    // The model name is the one value with no bare form to fall back on, so it is cut
    // — visibly, with an ellipsis.
    assert_eq!(panel[0], "模型   claude-sonnet-4…");
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
    assert!(text.contains("权限询问：write_file"), "{text}");
    assert!(text.contains("[y] 允许"), "the keys come with it: {text}");

    let modal = rows
        .iter()
        .position(|row| row.contains("权限询问"))
        .expect("the overlay is on screen");
    assert!(rows[modal].contains('│'), "inside a box: {:?}", rows[modal]);
    assert!(
        rows[modal - 1].contains('┌') && rows[modal + 1].contains('└'),
        "with borders of its own: {:?} / {:?}",
        rows[modal - 1],
        rows[modal + 1]
    );

    // The plan-mode gesture waits: a question owns the keyboard until it is answered
    // (spec §9).
    state.key(fs_agent::render::Key::BackTab);
    assert!(
        state.take_events().is_empty(),
        "Shift-Tab is not a way out of a question"
    );

    // The box reaches across the seam: only its own two borders are left on the row,
    // because the shared seam that sits inside it was blanked with the panel behind.
    let row = modal as u16;
    let frame = buffer(120, 24, &mut state);
    let borders: Vec<u16> = (1..119u16)
        .filter(|x| frame[(*x, row)].symbol() == "│")
        .collect();
    assert_eq!(borders.len(), 2, "the box's borders: {borders:?}");
    assert!(
        borders[0] < 89 && borders[1] > 89,
        "and the box reaches across the seam: {borders:?}"
    );
    // Centred in the middle block: the room above and below is the same, within the
    // row the integer division leaves over.
    let (_, middle_top) = find_cell(&frame, 120, 24, "┬").expect("the seam, up top");
    let (_, middle_bottom) = find_cell(&frame, 120, 24, "┴").expect("the seam, at the foot");
    let above = (row - 1) - (middle_top + 1);
    let below = (middle_bottom - 1) - (row + 1);
    assert!(
        above.abs_diff(below) <= 1,
        "centred: {above} rows above the box, {below} below"
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
    assert!(text.contains("/tmp/PLAN.md 已存在"), "{text}");
    assert!(text.contains("[o] 覆盖"), "with its keys: {text}");

    // An oversized paste.
    let mut paste = state();
    paste.paste(&"x".repeat(100_001));
    let text = screen(120, 24, &mut paste).join("\n");
    assert!(text.contains("粘贴 100001 字符？"), "{text}");
    assert!(text.contains("[y] 粘贴"), "with its keys: {text}");

    // And `Esc` on a multi-line draft.
    let mut draft = state();
    draft.paste("第一行\n第二行");
    draft.key(Key::Esc);
    let text = screen(120, 24, &mut draft).join("\n");
    assert!(text.contains("清空输入？"), "{text}");
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
    let mut state = state();
    state.paste("第一行\n第二行");
    state.key(Key::Esc);
    let rows = screen(120, 24, &mut state);
    let modal = rows
        .iter()
        .position(|row| row.contains("清空输入？"))
        .expect("the overlay is on screen");

    let row = modal as u16;
    let frame = buffer(120, 24, &mut state);
    let borders: Vec<u16> = (1..119u16)
        .filter(|x| frame[(*x, row)].symbol() == "│")
        .collect();
    assert_eq!(borders.len(), 2, "the box's borders: {borders:?}");
    assert_eq!(
        cells(&frame, row, borders[0] + 1, borders[1]).trim(),
        "清空输入？[y] 清空 / [n] 保留",
        "the interior holds the question and nothing that was behind it"
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
