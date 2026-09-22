//! THROWAWAY probe for ticket 05 — the four-pane geometry with **airy removed**
//! and the Mark header's internal spacing swapped.
//!
//! It re-implements the *proposed* `layout::plan` arithmetic (the current one
//! still has `AIRY_ROWS`), prints an old→new geometry table, and renders real
//! `TestBackend` snapshots of the new layout. Run:
//!
//!   CARGO_TARGET_DIR=/tmp/tui-ux-probe-target cargo run --offline \
//!     --manifest-path .scratch/tui-ux/prototype/Cargo.toml --bin geometry -- \
//!     .scratch/tui-ux/prototype/geometry

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

// --- the consts, copied from src/render/layout.rs --------------------------

const MIN_WIDTH: u16 = 40;
const MIN_HEIGHT: u16 = 10;
const HEADER_TWO_LINE_WIDTH: u16 = 60;
const BORDER_ROWS: u16 = 2;
const HINT_ROWS: u16 = 1;
const AIRY_ROWS: u16 = 2;
const MIN_MIDDLE_ROWS: u16 = 1;
const CHROME: u16 = 3 * BORDER_ROWS + HINT_ROWS;
const LOGO_WIDTH: u16 = 38;
const LOGO_HEIGHT: u16 = 7;
const LOGO_MIN_WIDTH: u16 = LOGO_WIDTH + BORDER_ROWS + 2;
const MAX_INPUT_ROWS: u16 = 10;
const PANEL_MIN_WIDTH: u16 = 80;
const PANEL_MIN_ROWS: u16 = 4;

fn logo_min_height(airy: bool) -> u16 {
    LOGO_HEIGHT
        + BORDER_ROWS
        + (if airy { AIRY_ROWS } else { 0 })
        + BORDER_ROWS
        + MIN_MIDDLE_ROWS
        + BORDER_ROWS
        + 1
        + HINT_ROWS
        + BORDER_ROWS
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    TextOneLine,
    TextTwoLines,
    Mark,
}

fn header_content_rows(width: u16, height: u16, airy: bool) -> u16 {
    if width >= LOGO_MIN_WIDTH && height >= logo_min_height(airy) {
        LOGO_HEIGHT
    } else if width < HEADER_TWO_LINE_WIDTH || height <= MIN_HEIGHT {
        1
    } else {
        2
    }
}

fn fits_airy(height: u16, header_rows: u16, input_rows: u16) -> bool {
    height >= CHROME + header_rows + input_rows + AIRY_ROWS + MIN_MIDDLE_ROWS
}

fn max_input_rows(height: u16, header_rows: u16, airy: bool) -> u16 {
    let airy_rows = if airy { AIRY_ROWS } else { 0 };
    let room = height.saturating_sub(CHROME + header_rows + airy_rows + MIN_MIDDLE_ROWS);
    MAX_INPUT_ROWS.min(room).max(1)
}

fn panel_outer(width: u16) -> u16 {
    ((u32::from(width) * 26 / 100) as u16).clamp(25, 31)
}

/// `airy_allowed` = the OLD model lets `fits_airy` veto airy; the NEW model has
/// no airy at all. Everything else is the current arithmetic.
fn plan(w: u16, h: u16, draft_rows: u16, airy_allowed: bool) -> Option<Regions> {
    if w < MIN_WIDTH || h < MIN_HEIGHT {
        return None;
    }
    let header_rows = header_content_rows(w, h, airy_allowed);
    let airy = airy_allowed && fits_airy(h, header_rows, 1);
    let cap = max_input_rows(h, header_rows, airy);
    let input_rows = draft_rows.max(1).min(cap);
    let airy_rows = if airy { AIRY_ROWS } else { 0 };
    let middle_rows = h - CHROME - header_rows - input_rows - airy_rows;

    let header = Rect::new(0, 0, w, header_rows + BORDER_ROWS);
    let middle = Rect::new(0, header.height + u16::from(airy), w, middle_rows + BORDER_ROWS);
    let bottom = Rect::new(
        0,
        middle.y + middle.height + u16::from(airy),
        w,
        input_rows + HINT_ROWS + BORDER_ROWS,
    );
    let outer = if w >= PANEL_MIN_WIDTH && middle_rows >= PANEL_MIN_ROWS {
        panel_outer(w)
    } else {
        0
    };
    let transcript_width = if outer > 0 { w - outer - 1 } else { w - 2 };
    let panel = (outer > 0).then(|| {
        Rect::new(
            w - outer + 1,
            middle.y + 1,
            outer - BORDER_ROWS,
            middle_rows,
        )
    });
    let kind = if header_rows >= LOGO_HEIGHT {
        Kind::Mark
    } else if header_rows <= 1 {
        Kind::TextOneLine
    } else {
        Kind::TextTwoLines
    };
    Some(Regions {
        header,
        header_rows,
        middle,
        transcript: Rect::new(1, middle.y + 1, transcript_width, middle_rows),
        panel,
        bottom,
        input_rows,
        hints: Rect::new(1, bottom.y + 1 + input_rows, w - 2, HINT_ROWS),
        kind,
        airy,
    })
}

struct Regions {
    header: Rect,
    header_rows: u16,
    middle: Rect,
    transcript: Rect,
    panel: Option<Rect>,
    bottom: Rect,
    input_rows: u16,
    hints: Rect,
    kind: Kind,
    airy: bool,
}

// --- width helpers (approximate) -------------------------------------------

fn cw(c: char) -> usize {
    let u = c as u32;
    let wide = (0x1100..=0x115F).contains(&u)
        || (0x2E80..=0xA4CF).contains(&u)
        || (0xAC00..=0xD7A3).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFE30..=0xFE4F).contains(&u)
        || (0xFF00..=0xFF60).contains(&u)
        || (0xFFE0..=0xFFE6).contains(&u)
        || (0x20000..=0x3FFFD).contains(&u);
    if wide {
        2
    } else {
        1
    }
}

fn cols(s: &str) -> usize {
    s.chars().map(cw).sum()
}

fn wrap(s: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut row = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let w = cw(ch);
        if used + w > width && !row.is_empty() {
            out.push(std::mem::take(&mut row));
            used = 0;
        }
        row.push(ch);
        used += w;
    }
    if !row.is_empty() || out.is_empty() {
        out.push(row);
    }
    out
}

fn pad_right(s: &str, width: usize) -> String {
    let mut out = s.to_owned();
    let have = cols(s);
    if have < width {
        out.push_str(&" ".repeat(width - have));
    }
    out
}

fn edges(left: &str, right: &str, width: usize) -> String {
    let gap = width.saturating_sub(cols(left) + cols(right));
    let mut out = left.to_owned();
    out.push_str(&" ".repeat(gap));
    out.push_str(right);
    out
}

fn logo_lines() -> [&'static str; 5] {
    [
        "▄▀▀█ ▄▀▀█      ▄▀▀▄ ▄▀▀▀ ▄▀▀█ █  █ ▀█▀",
        "▓▄▄  ▓         ▓▄▄▓ ▓ ▀▓ ▓▄▄  ▓▄ ▓  ▓ ",
        "▒     ▀▀▄ ▀▀▀▀ ▒  ▒ ▒  ▒ ▒    ▒ ▀▒  ▒ ",
        "░    ░  ░      ░  ░ ░  ░ ░  ▄ ░  ░  ░ ",
        "▀    ▀▀▀       ▀  ▀  ▀▀▀  ▀▀▀ ▀  ▀  ▀ ",
    ]
}

// --- rendering the NEW layout ----------------------------------------------

fn draw(frame: &mut ratatui::Frame, r: &Regions, draft_rows: u16) {
    let dim = Style::default().fg(Color::DarkGray);
    let block = || Block::default().borders(Borders::ALL).border_style(dim);

    // header
    let hc = Rect::new(1, 1, r.header.width - 2, r.header_rows);
    match r.kind {
        Kind::Mark => {
            let mut rows: Vec<Line> = logo_lines()
                .iter()
                .map(|row| {
                    Line::from(Span::styled(
                        (*row).to_owned(),
                        Style::default().fg(Color::Magenta),
                    ))
                })
                .collect();
            // the spacing change: one blank row, then the info row.
            rows.push(Line::from(" "));
            rows.push(Line::from(edges(
                "会话路径 /…/20260923T1042Z",
                "模式 询问 · 2026-09-23 10:42",
                hc.width as usize,
            )));
            frame.render_widget(Paragraph::new(rows).block(block()), r.header);
        }
        Kind::TextTwoLines => {
            let rows = vec![
                Line::from("fs-agent 0.1.0 · 2026-09-23 10:42"),
                Line::from(edges(
                    "会话路径 /…/20260923T1042Z",
                    "模式 询问",
                    hc.width as usize,
                )),
            ];
            frame.render_widget(Paragraph::new(rows).block(block()), r.header);
        }
        Kind::TextOneLine => {
            let rows = vec![Line::from("fs-agent 0.1.0 · 模式 询问 · 10:42")];
            frame.render_widget(Paragraph::new(rows).block(block()), r.header);
        }
    }

    // middle
    frame.render_widget(block(), r.middle);
    let text_w = r.transcript.width.saturating_sub(1) as usize;
    let sample = [
        "[用户] 删掉 airy，把 logo 与会话路径之间留一行",
        "[kimi] … 正在思考",
        "▸ [kimi] ✓ 思考完成",
        "▸ [kimi] 调用 bash command=cargo test --all-targets",
        "▸ [kimi] 调用 read_file path=src/render/tui.rs 失败",
    ];
    let mut rows: Vec<Line> = Vec::new();
    for line in sample {
        for row in wrap(line, text_w.max(1)) {
            rows.push(Line::from(row));
        }
    }
    let th = r.transcript.height as usize;
    let shown: Vec<Line> = if rows.len() > th {
        rows[rows.len() - th..].to_vec()
    } else {
        rows
    };
    frame.render_widget(Paragraph::new(shown), r.transcript);

    if let Some(panel) = r.panel {
        // The transcript and the panel share one seam column: draw a single
        // vertical line at `panel.x - 1`, with T-junctions where it meets the
        // middle block's top and bottom borders.
        let seam_x = panel.x - 1;
        let last = r.middle.y + r.middle.height - 1;
        for y in (r.middle.y + 1)..last {
            if let Some(cell) = frame.buffer_mut().cell_mut((seam_x, y)) {
                cell.set_symbol("│");
            }
        }
        if let Some(cell) = frame.buffer_mut().cell_mut((seam_x, r.middle.y)) {
            cell.set_symbol("┬");
        }
        if let Some(cell) = frame.buffer_mut().cell_mut((seam_x, last)) {
            cell.set_symbol("┴");
        }
        let info = vec![
            Line::from("模型  deepseek-flash"),
            Line::from("上下文  18 402 / 63 000"),
            Line::from("token  1 204 882 / 5 000 000"),
            Line::from("回合  12"),
        ];
        frame.render_widget(Paragraph::new(info), panel);
    }

    // bottom
    frame.render_widget(block(), r.bottom);
    let bc = Rect::new(1, r.bottom.y + 1, r.bottom.width - 2, r.input_rows);
    let mut draft: Vec<Line> = Vec::new();
    for i in 0..draft_rows.max(1) {
        draft.push(Line::from(if i == 0 {
            "> 把 airy 删掉".to_owned()
        } else {
            "  第二行草稿".to_owned()
        }));
    }
    frame.render_widget(Paragraph::new(draft), bc);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "就绪 · enter 发送 · ctrl-j 换行 · ctrl-d 退出",
            dim,
        ))),
        r.hints,
    );
}

fn render(w: u16, h: u16, draft_rows: u16) -> String {
    let Some(r) = plan(w, h, draft_rows, false) else {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        "终端太小：至少 40×10",
                        Style::default().fg(Color::Red),
                    ))),
                    frame.area(),
                );
            })
            .expect("draw");
        return dump(terminal.backend(), w, h, "TOO-SMALL");
    };
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.draw(|frame| draw(frame, &r, draft_rows)).expect("draw");
    dump(terminal.backend(), w, h, "NEW (no airy)")
}

fn dump(backend: &TestBackend, w: u16, h: u16, level: &str) -> String {
    let mut out = String::new();
    writeln!(out, "size: {w}x{h}   {level}").unwrap();
    let mut ruler = String::from("    ");
    for x in 0..w {
        if x % 10 == 0 {
            ruler.push(char::from_digit(((x / 10) % 10) as u32, 10).unwrap());
        } else {
            ruler.push('.');
        }
    }
    writeln!(out, "{ruler}").unwrap();
    writeln!(out, "   |{}|", "-".repeat(w as usize)).unwrap();
    let body = format!("{backend}");
    for (i, line) in body.lines().enumerate() {
        let line = line
            .split(" Hidden by multi-width symbols")
            .next()
            .unwrap_or(line);
        let inner = line
            .strip_prefix('"')
            .and_then(|l| l.strip_suffix('"'))
            .unwrap_or(line);
        writeln!(out, "{:>3}|{}|", i, pad_right(inner, w as usize)).unwrap();
    }
    writeln!(out, "   |{}|", "-".repeat(w as usize)).unwrap();
    out
}

// --- the table --------------------------------------------------------------

fn describe(w: u16, h: u16, draft: u16, airy_allowed: bool) -> String {
    match plan(w, h, draft, airy_allowed) {
        None => "太小".to_owned(),
        Some(r) => {
            let panel = match r.panel {
                Some(p) => format!("{}×{} / {}×{}", p.width + 2, p.height + 2, p.width, p.height),
                None => "隐藏".to_owned(),
            };
            format!(
                "{} | {} | {} | {} | {}×{} / {}×{} | {} | {}",
                r.header_rows,
                if r.airy { "on" } else { "OFF" },
                r.transcript.height,
                if r.panel.is_some() { "有" } else { "无" },
                r.transcript.width + 2,
                r.transcript.height + 2,
                r.transcript.width,
                r.transcript.height,
                panel,
                r.input_rows
            )
        }
    }
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("usage: geometry <out-dir>"));
    fs::create_dir_all(&out).expect("mkdir");

    let mut table = String::new();
    writeln!(table, "# ticket 05 几何表 —— 删 airy 前 / 后（TestBackend 实测）\n").unwrap();
    writeln!(
        table,
        "`旧` = 现在的 `plan()`（**已含 Mark header**）；`新` = 删掉 airy 后的 `plan()`。空白草稿，除非另注。\n"
    )
    .unwrap();
    writeln!(
        table,
        "> ⚠️ **既有的文档漂移**：Mark header（`LOGO_HEIGHT` / `LOGO_MIN_WIDTH` / `LOGO_MIN_HEIGHT`）是在 tui-layout spec §2 那张几何表**之后**才落地的，所以 `旧` 列不等于 spec 表里的数字——最明显的是 `60×24`：spec 表写「header 内容行 2」，当前代码是 **7**（Mark）。这不是本票引入的，`/to-spec` 回改时要一并修。\n"
    )
    .unwrap();
    writeln!(table, "| 尺寸 | 版本 | header 内容行 | airy | 中段内容行 | 右栏 | 转录 外框×内容 | 右栏 外框×内容 | 输入行 |").unwrap();
    writeln!(table, "| --- | --- | --- | --- | --- | --- | --- | --- | --- |").unwrap();

    let sizes: [(u16, u16); 18] = [
        (39, 24),
        (40, 9),
        (40, 10),
        (40, 12),
        (42, 17),
        (42, 18),
        (60, 11),
        (60, 12),
        (60, 13),
        (60, 24),
        (80, 13),
        (80, 14),
        (80, 15),
        (80, 16),
        (80, 24),
        (120, 24),
        (174, 50),
        (120, 10),
    ];
    for (w, h) in sizes {
        for (label, airy) in [("旧", true), ("新", false)] {
            let d = describe(w, h, 1, airy);
            let cells: Vec<&str> = d.split(" | ").collect();
            if cells.len() == 7 {
                writeln!(
                    table,
                    "| {w}×{h} | {label} | {} | {} | {} | {} | {} | {} | {} |",
                    cells[0], cells[1], cells[2], cells[3], cells[4], cells[5], cells[6]
                )
                .unwrap();
            } else {
                writeln!(table, "| {w}×{h} | {label} | {d} |  |  |  |  |  |  |").unwrap();
            }
        }
    }

    writeln!(table, "\n## 草稿涨高时（`120×24`）\n").unwrap();
    writeln!(table, "| 草稿行 | 版本 | 中段内容行 | 右栏 | 输入行 |").unwrap();
    writeln!(table, "| --- | --- | --- | --- | --- |").unwrap();
    for draft in [1u16, 3, 7, 9, 10] {
        for (label, airy) in [("旧", true), ("新", false)] {
            let d = describe(120, 24, draft, airy);
            let cells: Vec<&str> = d.split(" | ").collect();
            if cells.len() == 7 {
                writeln!(
                    table,
                    "| {draft} | {label} | {} | {} | {} |",
                    cells[2], cells[3], cells[6]
                )
                .unwrap();
            }
        }
    }

    writeln!(table, "\n## 阈值\n").unwrap();
    writeln!(table, "| 量 | 旧 | 新 |").unwrap();
    writeln!(table, "| --- | --- | --- |").unwrap();
    writeln!(
        table,
        "| `LOGO_MIN_HEIGHT` | {} | {} |",
        logo_min_height(true),
        logo_min_height(false)
    )
    .unwrap();

    let cases: [(&str, u16, u16, u16); 9] = [
        ("geometry-39x24.txt", 39, 24, 1),
        ("geometry-40x9.txt", 40, 9, 1),
        ("geometry-40x10.txt", 40, 10, 1),
        ("geometry-40x12.txt", 40, 12, 1),
        ("geometry-60x24.txt", 60, 24, 1),
        ("geometry-80x16.txt", 80, 16, 1),
        ("geometry-80x24.txt", 80, 24, 1),
        ("geometry-120x24.txt", 120, 24, 1),
        ("geometry-120x24-draft10.txt", 120, 24, 10),
    ];
    for (file, w, h, draft) in cases {
        fs::write(out.join(file), render(w, h, draft)).expect("write snapshot");
    }
    fs::write(out.join("geometry-table.md"), table).expect("write table");
    println!("wrote geometry-table.md + {} snapshots", cases.len());
}
