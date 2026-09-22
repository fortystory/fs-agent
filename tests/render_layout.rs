//! The fullscreen four-pane layout, rendered into a `TestBackend` so the geometry
//! can be asserted without a terminal (spec §2, §Testing Decisions).
//!
//! The seam is [`draw_frame`]: a state goes in, a fixed-size buffer comes out. Every
//! assertion below is about what a person would see — which regions exist, what the
//! header says, how many hints fit — never about the rectangles the layout computes
//! on the way there.

use fs_agent::render::width::text_columns;
use fs_agent::render::{
    draw_frame, CatalogEntry, ConsoleRequest, Key, RenderEvent, SessionFacts, TuiState,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::style::Color;
use ratatui::Terminal;

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

fn state() -> TuiState {
    TuiState::new(facts())
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

/// The row the middle block's top border sits on.
///
/// The header is 7, 2 or 1 content rows depending on the terminal, so any test that
/// counts from the middle block has to ask where it starts rather than assume — the
/// mark made a fixed row number wrong, and a fixed row number would go wrong again
/// the next time the ladder moves. Exactly three rows open a block, in order:
/// header, middle, bottom.
fn middle_top(rows: &[String]) -> usize {
    let opens: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.starts_with('┌'))
        .map(|(y, _)| y)
        .collect();
    assert_eq!(opens.len(), 3, "three blocks open: {opens:?}");
    opens[1]
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
fn a_wide_terminal_draws_the_mark_the_transcript_and_the_bottom_block() {
    // 120x24 is the reference size: a tall mark header (five mark rows, a blank
    // row, then the line of facts), the transcript, one input row and one hint row
    // — and no blank rows between the blocks (spec §2).
    let rows = screen(120, 24, &mut state());

    // The header block: the mark's rows between its borders.
    assert!(rows[0].starts_with('┌'), "the header opens: {:?}", rows[0]);
    assert!(rows[8].starts_with('└'), "the header closes: {:?}", rows[8]);
    assert!(
        rows[0].ends_with('┐') && rows[8].ends_with('┘'),
        "the header block is closed on the right too: {:?} / {:?}",
        rows[0],
        rows[8]
    );
    assert!(
        rows[1].contains("▄▀▀█") && rows[5].contains("▀▀▀"),
        "the mark's first and last rows are in the header: {:?} / {:?}",
        rows[1],
        rows[5]
    );
    assert_eq!(
        rows[6].trim_matches(|ch| ch == '│' || ch == ' '),
        "",
        "a blank row separates the mark from the facts: {:?}",
        rows[6]
    );
    assert!(
        rows[7].contains("~/code/fortystory/fs-agent") && rows[7].contains("模式 询问"),
        "the directory and the mode share the line under the mark: {:?}",
        rows[7]
    );

    let middle = middle_top(&rows);
    assert_eq!(middle, 9, "the mark header costs the extra rows: {rows:#?}");

    // No airy: the middle block opens on the row after the header's border and the
    // bottom block on the row after the middle's, so the transcript gets the two
    // rows that used to be blank.
    assert!(
        rows[middle - 1].starts_with('└'),
        "the middle opens right under the header: {:?}",
        rows[middle - 1]
    );

    // The middle block spans the transcript.
    assert!(
        rows[middle].starts_with('┌'),
        "the middle opens: {:?}",
        rows[middle]
    );
    assert!(
        rows[19].starts_with('└'),
        "the middle closes: {:?}",
        rows[19]
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
        rows[22].contains("ctrl-c"),
        "the hints name the way out: {:?}",
        rows[22]
    );

    assert!(
        !rows.join("\n").contains("shift+enter"),
        "no phantom newline key"
    );
}

#[test]
fn the_mark_is_drawn_on_the_top_rows_and_is_lit_from_above() {
    // The painter's half of the mark: the characters are `wording`'s, but which rows
    // they land on and how the gradient falls is the painter's, so it is asserted
    // where it can be seen — in the buffer, cell by cell.
    let frame = buffer(120, 24, &mut state());
    assert_eq!(
        frame[(1, 1)].symbol(),
        "▄",
        "the mark's first row starts at the header's first content row"
    );
    let top = frame[(1, 1)].fg;
    let bottom = frame[(1, 5)].fg;
    assert_eq!(
        top,
        Color::LightMagenta,
        "the top of the mark is the bright end"
    );
    assert_eq!(bottom, Color::Magenta, "and the bottom row is the dim end");

    // 40x10 is one row short of the tall header, and 60 columns is too narrow for the
    // mark: both keep the text header rather than a clipped mark.
    for (width, height) in [(40, 24), (40, 10), (42, 17)] {
        // A fresh state: the pane can legitimately hold block glyphs (the scrollbar),
        // and the question here is only whether the *mark* is in the header. 40 columns
        // is the mark's own width plus its borders with no air — too narrow — and 17
        // rows is one below the tall header, so both fall back to the text header.
        let mut fresh = state();
        let rows = screen(width, height, &mut fresh);
        assert!(
            !rows.join("\n").contains('▄'),
            "{width}x{height} is below the mark's size: {:#?}",
            rows[0]
        );
        assert!(
            rows.join("\n").contains("fs-agent"),
            "{width}x{height} keeps the identity in the text header: {:#?}",
            rows[0]
        );
    }

    // One row taller and the mark is drawn: the tall header's threshold moved down
    // to 18 when the airy rows left the ladder.
    let rows = screen(42, 18, &mut state());
    assert!(
        rows.join("\n").contains('▄'),
        "42x18 carries the mark: {rows:#?}"
    );
}

#[test]
fn a_floor_sized_terminal_still_draws_every_region() {
    // 40x10 is inside the minimum: one header line, one transcript row, one input
    // row and one hint row (spec §2).
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
    assert!(rows[8].contains("ctrl-c"), "the hint row: {:?}", rows[8]);
}

#[test]
fn the_information_panel_shares_a_seam_with_the_transcript_only_when_there_is_room() {
    // 120 columns: the panel is drawn, and the two panes share one column — a
    // vertical rule that meets the middle block's borders with junctions rather
    // than doubling them (spec §2).
    let frame = buffer(120, 24, &mut state());
    let seam = 89;
    // The seam has to meet whichever rows the middle block occupies, so they come
    // from the frame rather than from a remembered row number.
    let rows: Vec<String> = (0..24).map(|y| row_text(&frame, y, 120)).collect();
    let top = middle_top(&rows) as u16;
    assert_eq!(
        frame[(seam, top)].symbol(),
        "┬",
        "the seam meets the top border"
    );
    assert_eq!(
        frame[(seam, top + 10)].symbol(),
        "┴",
        "and the top border is ten rows above the bottom one"
    );
    for y in (top + 1)..top + 10 {
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

    // 80x14 is the smallest terminal that fits the panel: four middle rows, which
    // is all the four core fields need.
    let smallest = buffer(80, 14, &mut state());
    let middle_top = (0..14)
        .find(|y| smallest[(0, *y)].symbol() == "┌" && row_text(&smallest, *y, 80).contains('┬'))
        .expect("the panel is drawn at 80x14");
    assert!(middle_top > 0);
}

/// How many `·`-separated items the hint row holds, the state word included.
///
/// A prompt is in flight, because the measured ladder is the one a session shows
/// while it is waiting for a line — the only time it can promise `enter 发送`.
fn hint_items(width: u16) -> Vec<String> {
    let mut state = state();
    let (reply, _line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    let rows = screen(width, 24, &mut state);
    let row = rows
        .iter()
        .find(|row| row.contains("ctrl-c"))
        .expect("the hint row is on screen");
    row.trim_matches(|ch| ch == '│' || ch == ' ')
        .split(" · ")
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_session_with_no_line_being_read_promises_only_what_the_keyboard_does() {
    // Mid-turn, or inside a one-shot `discuss`: nothing is reading lines, so `enter
    // 发送` would be a lie and the plan-mode gesture has nothing to toggle.
    let mut state = state();
    state.apply(fs_agent::render::RenderEvent::Notice("工作中".to_owned()));
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
    // The measured ladder with `ctrl-c/ctrl-d 退出` as the reserved way out: three
    // items at 40 columns, four at 60, five at 80, seven at 120 — and the state word
    // joins them on the left only where it still fits (spec §10, 票 06 §4).
    assert_eq!(hint_items(40).len(), 3, "40 columns: {:?}", hint_items(40));
    assert_eq!(hint_items(60).len(), 4, "60 columns: {:?}", hint_items(60));
    assert_eq!(hint_items(80).len(), 5, "80 columns: {:?}", hint_items(80));
    assert_eq!(
        hint_items(120).len(),
        7,
        "120 columns: {:?}",
        hint_items(120)
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

    // At 60 the state word is what goes, so that two hints and the way out can fit:
    // the quirk of "hints first, state word only if it still fits" (票 06 §4).
    let sixty = hint_items(60);
    assert_eq!(sixty[0], "enter 发送", "no state word at 60: {sixty:?}");
    assert!(
        !sixty.contains(&"就绪".to_owned()),
        "the state word is the first thing given up: {sixty:?}"
    );
    assert!(
        sixty.contains(&"ctrl-j 换行".to_owned()),
        "which is what buys the newline hint: {sixty:?}"
    );

    // At 80 columns the terminal is wide enough for five items but not six, so the
    // state word is still the one that goes: two rows of border leave 78 columns for
    // the hint text, and the full line needs 80.
    let roomy = hint_items(80);
    assert_eq!(
        roomy[0], "enter 发送",
        "no state word at 80 either: {roomy:?}"
    );
    assert!(
        roomy.contains(&"shift+tab 计划".to_owned()),
        "but the plan-mode gesture fits: {roomy:?}"
    );

    // At 120 columns every hint and the state word are on the line.
    let wide = hint_items(120);
    assert_eq!(wide[0], "就绪", "the state word is back: {wide:?}");
    assert!(
        wide.contains(&"PgUp/PgDn 滚动".to_owned()),
        "with the whole hint list behind it: {wide:?}"
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
    // reason it is in the table: 80x24 is the panel with room to spare, and 174x50
    // is the ceiling — the panel is capped at 31 columns and the transcript takes
    // the rest.
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
            text.contains("ctrl-c"),
            "{width}x{height} keeps the way out: {text}"
        );
    }

    // Airy is gone at every size: the row under the header's bottom border is the
    // middle block's top border, however tall the terminal is.
    for (width, height) in [(40, 10), (40, 12), (80, 24), (120, 24)] {
        let rows = screen(width, height, &mut state());
        let middle = middle_top(&rows);
        assert!(
            rows[middle - 1].starts_with('└'),
            "{width}x{height} keeps no blank row under the header: {:?}",
            rows[middle - 1]
        );
    }

    // The panel appears from 80x14, not 80x16: with the airy rows gone the middle
    // block has four content rows there, which is the panel's own minimum.
    let narrow = screen(80, 14, &mut state());
    assert!(
        narrow.iter().any(|row| row.contains('┬')),
        "the panel is drawn at 80x14: {narrow:#?}"
    );
    let smaller = screen(80, 13, &mut state());
    assert!(
        !smaller.iter().any(|row| row.contains('┬')),
        "and not one row smaller: {smaller:#?}"
    );

    // 174x50 is wide enough that the panel is at its 31-column cap: the seam sits
    // 31 columns from the right edge.
    let wide = buffer(174, 50, &mut state());
    let seam = 174 - 31;
    let seam_row = (0..50)
        .find(|y| wide[(seam, *y)].symbol() == "┬")
        .expect("the seam meets the middle block's top border");
    assert_eq!(wide[(seam, seam_row)].symbol(), "┬", "the seam at the cap");
    assert_eq!(
        wide[(174 - 1, seam_row)].symbol(),
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

    // PgUp leaves the bottom and shows older rows. One page is the pane's own
    // height, so the row at the top is derived from what the pane can show rather
    // than remembered — the mark header changed it, and a fixed row number would
    // have to change again the next time the ladder moves.
    let visible = transcript_rows();
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
/// rows ask for the number instead of remembering it — the mark header changed it
/// once already.
fn transcript_rows() -> usize {
    // The transcript's **content** rows: the middle block's interior, which is its
    // height less the two border rows. It grows with the layout, so it is measured
    // from the frame here rather than remembered.
    let rows = screen(120, 24, &mut state());
    let middle = middle_top(&rows);
    let bottom = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.starts_with('└'))
        .map(|(y, _)| y)
        .nth(1)
        .expect("the middle block closes");
    bottom - middle - 1
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
    // pane's own height less the two rows the reader keeps, so at 120x24 the top of
    // the viewport lands on notice 24 — the bottom was 31, and one page is seven.
    let after_page_up = first_notice(&screen(120, 24, &mut state));
    assert_eq!(
        after_page_up,
        Some(24),
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

    // 120x24 leaves the transcript 88 content columns; the last of them belongs to
    // the scrollbar whether or not anything is drawn in it, so text wraps at 87.
    let mut state = state();
    state.apply(RenderEvent::Notice("x".repeat(88)));
    let frame = buffer(120, 24, &mut state);
    let rows: Vec<String> = (0..24).map(|y| row_text(&frame, y, 120)).collect();
    let content_row = middle_top(&rows) as u16 + 1;
    // The transcript's text starts one column inside the middle block's border, so the
    // header row the old row number pointed at is what had to change.
    assert_eq!(
        frame[(1, content_row)].symbol(),
        "x",
        "the row starts at the first column"
    );
    assert_eq!(
        frame[(1, content_row + 1)].symbol(),
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
    let rows: Vec<String> = (0..24).map(|y| row_text(&frame, y, 120)).collect();
    let top = middle_top(&rows) as u16;
    for y in (top + 1)..(top + 1 + 7) {
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
        three[22].contains("ctrl-c"),
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

/// The row the information panel's top border is on, if the panel is drawn.
fn right_panel_top(rows: &[String]) -> Option<usize> {
    rows.iter()
        .position(|row| row.ends_with('┐') && row.contains('┬'))
}

/// A panel field by its offset from the panel's own first content row.
///
/// The panel is drawn inside the middle block, whose top row the mark header moved.
/// A field is "the first one", "the third one" — not "row 6" — so the tests read the
/// field rather than a row number that has to be recomputed whenever the header
/// changes height.
fn panel_field(rows: &[String], offset: usize) -> String {
    let top = right_panel_top(rows).expect("the panel is drawn");
    rows[top + 1 + offset].clone()
}

#[test]
fn the_panel_names_the_model_and_shows_a_zero_and_a_dash_before_any_call() {
    let rows = screen(120, 24, &mut state());
    assert!(
        panel_field(&rows, 0).contains("模型")
            && panel_field(&rows, 0).contains("claude-sonnet-4-5"),
        "the model: {:?}",
        panel_field(&rows, 0)
    );
    assert!(
        panel_field(&rows, 1).contains("上下文")
            && panel_field(&rows, 1).contains(fs_agent::render::wording::PANEL_UNKNOWN),
        "no call has reported usage yet: {:?}",
        panel_field(&rows, 1)
    );
    assert!(
        panel_field(&rows, 2).contains("token") && panel_field(&rows, 2).contains('0'),
        "nothing spent yet: {:?}",
        panel_field(&rows, 2)
    );
    assert!(
        panel_field(&rows, 3).contains("回合") && panel_field(&rows, 3).contains('0'),
        "no turn yet: {:?}",
        panel_field(&rows, 3)
    );
    assert!(
        panel_field(&rows, 4).contains("输入") && panel_field(&rows, 4).contains('0'),
        "nothing in: {:?}",
        panel_field(&rows, 4)
    );
    assert!(
        panel_field(&rows, 6).contains("0 / 0"),
        "and a cache that has never been consulted: {:?}",
        panel_field(&rows, 6)
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
        panel_field(&rows, 0).contains("claude-sonnet-4-5"),
        "{:?}",
        panel_field(&rows, 0)
    );
    assert!(
        panel_field(&rows, 1).contains("9,000 / 200,000（4%）"),
        "the window is the numerator of the last call: {:?}",
        panel_field(&rows, 1)
    );
    assert!(
        panel_field(&rows, 2).contains("12,345 / 100,000"),
        "spent is input plus output: {:?}",
        panel_field(&rows, 2)
    );
    assert!(
        panel_field(&rows, 3).contains('1'),
        "one turn: {:?}",
        panel_field(&rows, 3)
    );
    assert!(
        panel_field(&rows, 4).contains("9,000"),
        "input: {:?}",
        panel_field(&rows, 4)
    );
    assert!(
        panel_field(&rows, 5).contains("3,345"),
        "output: {:?}",
        panel_field(&rows, 5)
    );
    assert!(
        panel_field(&rows, 6).contains("5,000 / 4,000"),
        "the cache split: {:?}",
        panel_field(&rows, 6)
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
    // 80x14 is the smallest terminal that draws the panel at all: 23 columns of
    // content and four rows.
    let mut state = state();
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));
    state.apply(turn_ended(2));
    let rows = screen(80, 14, &mut state);
    let text = rows.join("\n");

    assert!(text.contains("模型"), "the model: {text}");
    // Exactly the plain value: the percentage would have to be truncated to fit, and
    // a truncated number reads as a smaller one.
    let panel = panel_text(80, 14, &mut state);
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
        panel_field(&rows, 1).contains("15,000 / 200,000（7%）"),
        "the window is what the *last* call carried: {:?}",
        panel_field(&rows, 1)
    );
    assert!(
        panel_field(&rows, 2).contains("27,000 / 100,000"),
        "the spend is every call's input plus output: {:?}",
        panel_field(&rows, 2)
    );
    assert!(
        panel_field(&rows, 4).contains("24,000"),
        "input summed: {:?}",
        panel_field(&rows, 4)
    );
    assert!(
        panel_field(&rows, 5).contains("3,000"),
        "output summed: {:?}",
        panel_field(&rows, 5)
    );
}

/// The panel's content, one string per row, read out of a rendered frame.
///
/// The panel sits right of the shared seam and left of the middle block's border, so
/// the seam column is what locates it.
fn panel_text(width: u16, height: u16, state: &mut TuiState) -> Vec<String> {
    let frame = buffer(width, height, state);
    let (seam, top) = find_cell(&frame, width, height, "┬").expect("the panel is drawn");
    let content = (seam + 1)..(width - 1);
    // `┬` sits on the middle block's top border, so the content starts below it; the
    // panel's content ends where the block's bottom border begins.
    ((top + 1)..height)
        .map(|y| cells(&frame, y, content.start, content.end))
        .take_while(|row| {
            !row.trim().is_empty() && !row.trim_start_matches('─').is_empty() || !row.contains('─')
        })
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
    // Nine rows in a 24-row terminal now that the airy rows belong to the middle
    // block, each filling the panel's 29 columns.
    assert_eq!(panel.len(), 9, "{panel:?}");
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
    // Centred in the middle block: the room above and below is the same, within the
    // row the integer division leaves over.
    let (_, middle_top) = find_cell(&frame, 120, 24, "┬").expect("the seam, up top");
    let (_, middle_bottom) = find_cell(&frame, 120, 24, "┴").expect("the seam, at the foot");
    let above = box_top - (middle_top + 1);
    let below = (middle_bottom - 1) - box_bottom;
    assert!(
        above.abs_diff(below) <= 1,
        "centred: {above} rows above the box, {below} below"
    );
}

#[test]
fn a_question_splits_into_a_title_a_summary_a_call_and_a_row_of_buttons() {
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

    // Four parts, top to bottom: what is asked, what the action is, the concrete
    // call, and the keys that answer it.
    let title = row_of("权限询问：write_file");
    let summary = row_of("写入一个文件");
    let call = row_of("write_file（path=a.rs）");
    let keys = row_of("[y] 允许");
    assert!(
        title < summary && summary < call && call < keys,
        "title, then the summary, then the call, then the buttons:\n{text}"
    );
    // Each part keeps its own row: a command can no longer push the keys into the
    // middle of a sentence, and the keys cannot bury the command.
    assert!(
        !rows[keys].contains("path=a.rs") && !rows[keys].contains("权限询问"),
        "the buttons have the row to themselves: {:?}",
        rows[keys]
    );
    assert!(
        !rows[title].contains("path=a.rs") && !rows[title].contains("写入一个文件"),
        "so does the title: {:?}",
        rows[title]
    );
    assert!(
        !rows[summary].contains("path=a.rs"),
        "and the summary explains an action, it does not repeat the call: {:?}",
        rows[summary]
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
    // read, so the question says what kind of action it is *first*.
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
    let summary = rows
        .iter()
        .position(|row| row.contains("shell 命令"))
        .unwrap_or_else(|| panic!("the summary is on screen:\n{text}"));
    let keys = rows
        .iter()
        .position(|row| row.contains("[y] 允许"))
        .unwrap_or_else(|| panic!("the buttons are on screen:\n{text}"));
    assert!(summary < keys, "the summary leads the buttons:\n{text}");
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

/// The row the draft is typed on.
fn input_row(rows: &[String]) -> usize {
    rows.iter()
        .position(|row| row.starts_with("│> "))
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
    let borders: Vec<u16> = (1..119u16)
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
        cwd: dir.display().to_string(),
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
    let frozen: Vec<String> = before
        .iter()
        .skip(middle_top(&before) + 1)
        .take(transcript_rows())
        .cloned()
        .collect();

    // New output arrives while the overlay is open.
    for index in 40..60 {
        state.apply(fs_agent::render::RenderEvent::Notice(format!(
            "第 {index} 行"
        )));
    }
    let during = screen(120, 24, &mut state);
    let after: Vec<String> = during
        .iter()
        .skip(middle_top(&during) + 1)
        .take(transcript_rows())
        .cloned()
        .collect();
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
        cwd: dir.display().to_string(),
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
    assert_eq!(
        detail, 116,
        "and at 120 columns it is capped by the margin, not the ceiling"
    );
}

/// The painted width of a floating box, read off the row its top border is on.
///
/// The overlay is centred over the middle block, whose own border sits one column
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

    // Outside: the transcript row the line came from.
    let row = row_of(&mut state, 120, 40, "调用 bash").expect("the call line");
    state.mouse(click(4, row));
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
    state.mouse(click(40, 20));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("── 参数 ──"),
        "a click inside leaves it up: {text}"
    );
    // What decides is the **screen position**, not which transcript line is under it:
    // a click inside the overlay's rectangle does nothing. So when the line the overlay
    // was opened from sits under the overlay — which is where it usually is, since the
    // overlay covers most of the middle block — clicking there does nothing either, and
    // `Esc` / `Ctrl-D` / a click outside are the ways out (票 02 §4，2026-09-23 修正).
    // The "inside does nothing" click above is that case: (40, 20) is inside the
    // overlay's rectangle at this size.
    // The overlay's own borders bracket that click: (40, 20) is between them.
    let frame = buffer(120, 40, &mut state);
    let overlay = overlay_width(&frame, 120, 40).expect("the overlay");
    assert_eq!(
        overlay, 116,
        "at 120 columns the overlay is 116 wide, so the click above was inside it"
    );
    assert_eq!(
        (frame[(2, 20)].symbol(), frame[(117, 20)].symbol()),
        ("│", "│"),
        "and those are its two edges on that row"
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
