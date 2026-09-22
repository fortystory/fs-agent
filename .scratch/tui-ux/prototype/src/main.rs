//! THROWAWAY probe for ticket 03 — the *form* of the collapse hint lines, the
//! tool line and the detail overlay.
//!
//! It renders real frames through ratatui's `TestBackend` (no pty, no raw mode)
//! and dumps the screen to text. It answers "what should this look like": the
//! variants differ in the clickable affordance, the thinking-line glyph, the
//! detail-overlay section style and the overlay width, across four terminal
//! sizes. Geometry is a simplified stand-in — the exact four-pane numbers belong
//! to a different ticket, not this question.
//!
//! Run:
//!   CARGO_TARGET_DIR=/tmp/tui-ux-probe-target cargo run --offline \
//!     --manifest-path .scratch/tui-ux/prototype/Cargo.toml -- .scratch/tui-ux/prototype

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Terminal;

// ---------------------------------------------------------------------------
// Width helpers (approximate: East-Asian wide/fullwidth counts as 2 columns)
// ---------------------------------------------------------------------------

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

/// Wrap on character count (same coarse discipline as the real `pane::wrap_line`).
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

/// Cut a string to at most `width` columns (a wide glyph that would straddle the
/// edge is dropped whole).
fn truncate_cols(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let w = cw(ch);
        if used + w > width {
            break;
        }
        out.push(ch);
        used += w;
    }
    out
}

// ---------------------------------------------------------------------------
// What the variants vary
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Click {
    Marker,
    Underline,
    Suffix,
    None,
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Rule,
    Colon,
}

#[derive(Clone, Copy)]
struct Cfg {
    click: Click,
    section: Section,
    overlay: bool,
    wide_overlay: bool,
    thinking_glyph: bool,
}

impl Cfg {
    const BASE: Cfg = Cfg {
        click: Click::Underline,
        section: Section::Rule,
        overlay: false,
        wide_overlay: false,
        thinking_glyph: false,
    };
}

/// The user's pick from the first review round (2026-09-23): `▸ ` marker,
/// `── 标题 ──` rules, a 90-column overlay, and `…` / `✓` thinking glyphs.
const CHOSEN: Cfg = Cfg {
    click: Click::Marker,
    section: Section::Rule,
    overlay: false,
    wide_overlay: true,
    thinking_glyph: true,
};

struct Entry {
    text: String,
    style: Style,
    clickable: bool,
}

fn transcript(cfg: &Cfg) -> Vec<Entry> {
    let dim = Style::default().fg(Color::DarkGray);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let red = Style::default().fg(Color::Red);
    let running = if cfg.thinking_glyph {
        "… 正在思考"
    } else {
        "正在思考"
    };
    let done = if cfg.thinking_glyph {
        "✓ 思考完成"
    } else {
        "思考完成"
    };
    vec![
        Entry {
            text: "[用户] 把折叠、详情覆盖层和角色配色做出来".to_owned(),
            style: Style::default(),
            clickable: false,
        },
        Entry {
            text: "Kimi: 提示行落成普通转录源行，计入 20 000 上限；命中映射随后补。".to_owned(),
            style: Style::default(),
            clickable: false,
        },
        Entry {
            text: format!("[kimi] {running}"),
            style: dim,
            clickable: false,
        },
        Entry {
            text: format!("[kimi] {done}"),
            style: dim,
            clickable: true,
        },
        Entry {
            text: "[kimi] 调用 bash command=cargo test --all-targets".to_owned(),
            style: bold,
            clickable: true,
        },
        Entry {
            text: "[kimi] 调用 read_file path=src/render/tui.rs 失败".to_owned(),
            style: red,
            clickable: true,
        },
    ]
}

// ---------------------------------------------------------------------------
// Simplified four-region geometry (a stand-in for the real layout)
// ---------------------------------------------------------------------------

struct Panes {
    header: Rect,
    transcript: Rect,
    panel: Option<Rect>,
    bottom: Rect,
}

fn plan(w: u16, h: u16) -> Panes {
    let header_rows = if w >= 60 && h >= 14 { 2 } else { 1 };
    let header = Rect::new(0, 0, w, header_rows + 2);
    let bottom = Rect::new(0, h.saturating_sub(4), w, 4);
    let middle_y = header.height;
    let middle_h = bottom.y.saturating_sub(middle_y);
    let panel_w = if w >= 80 && middle_h >= 6 {
        ((u32::from(w) * 26 / 100) as u16).clamp(25, 31)
    } else {
        0
    };
    let (transcript, panel) = if panel_w > 0 {
        (
            Rect::new(0, middle_y, w - panel_w, middle_h),
            Some(Rect::new(w - panel_w, middle_y, panel_w, middle_h)),
        )
    } else {
        (Rect::new(0, middle_y, w, middle_h), None)
    };
    Panes {
        header,
        transcript,
        panel,
        bottom,
    }
}

fn inner(r: Rect) -> Rect {
    Rect::new(
        r.x + 1,
        r.y + 1,
        r.width.saturating_sub(2),
        r.height.saturating_sub(2),
    )
}

/// The left edge of the detail overlay (it is centred over the middle block), so
/// the transcript underneath can be clipped at it.
fn overlay_x(cfg: &Cfg, panes: &Panes) -> u16 {
    let middle_w = panes.transcript.width + panes.panel.map(|p| p.width).unwrap_or_default();
    let max_w: u16 = if cfg.wide_overlay { 90 } else { 72 };
    let width = middle_w.saturating_sub(4).min(max_w);
    panes.transcript.x + middle_w.saturating_sub(width) / 2
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn entry_rows(entry: &Entry, width: usize, clip: Option<usize>, cfg: &Cfg) -> Vec<Line<'static>> {
    let mut style = entry.style;
    let mut rows = wrap(&entry.text, width);
    if let Some(clip) = clip {
        rows = rows
            .into_iter()
            .map(|row| truncate_cols(&row, clip))
            .collect();
    }
    if entry.clickable {
        match cfg.click {
            Click::Underline => style = style.add_modifier(Modifier::UNDERLINED),
            Click::Marker => {
                for (i, row) in rows.iter_mut().enumerate() {
                    if i == 0 {
                        *row = format!("▸ {row}");
                    } else {
                        *row = format!("  {row}");
                    }
                }
            }
            Click::Suffix => {
                if let Some(last) = rows.last_mut() {
                    last.push_str(" [详情]");
                }
            }
            Click::None => {}
        }
    }
    rows.into_iter()
        .map(|row| Line::from(Span::styled(row, style)))
        .collect()
}

fn draw(
    frame: &mut ratatui::Frame,
    cfg: &Cfg,
    panes: &Panes,
    overlay_open: bool,
) {
    let dim = Style::default().fg(Color::DarkGray);
    let block = || Block::default().borders(Borders::ALL).border_style(dim);

    // header
    let head_rows = panes.header.height.saturating_sub(2) as usize;
    let head: Vec<Line> = if head_rows <= 1 {
        vec![Line::from("fs-agent 0.1.0 · 模式 询问 · 10:42")]
    } else {
        vec![
            Line::from("fs-agent 0.1.0 · 2026-09-23 10:42"),
            Line::from("~/.local/share/fs-agent/sessions/…/20260923T1042Z · 模式 询问"),
        ]
    };
    frame.render_widget(Paragraph::new(head).block(block()), panes.header);

    // transcript
    frame.render_widget(block(), panes.transcript);
    let tc = inner(panes.transcript);
    // When the overlay is up it covers the transcript it sits on: clip every
    // transcript row at the overlay's left edge so nothing runs under its border.
    let clip = cfg
        .overlay
        .then(|| overlay_x(cfg, panes).saturating_sub(tc.x) as usize);
    let mut rows: Vec<Line> = Vec::new();
    for entry in transcript(cfg) {
        rows.extend(entry_rows(&entry, tc.width as usize, clip, cfg));
    }
    let th = tc.height as usize;
    let shown: Vec<Line> = if rows.len() > th {
        rows[rows.len() - th..].to_vec()
    } else {
        rows
    };
    frame.render_widget(Paragraph::new(shown), tc);

    // panel
    if let Some(panel) = panes.panel {
        frame.render_widget(block(), panel);
        // An open overlay covers the panel text; skip it rather than let a wide
        // glyph straddle the overlay border.
        if !cfg.overlay {
            let pc = inner(panel);
            let info = vec![
                Line::from("模型  deepseek-flash"),
                Line::from("上下文  18 402 / 63 000（29%）"),
                Line::from("token  1 204 882 / 5 000 000"),
                Line::from("回合  12"),
            ];
            frame.render_widget(Paragraph::new(info), pc);
        }
    }

    // bottom
    frame.render_widget(block(), panes.bottom);
    let bc = inner(panes.bottom);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::DarkGray)),
            Span::raw("把折叠落成普通转录行"),
        ])),
        Rect::new(bc.x, bc.y, bc.width, 1),
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "就绪 · enter 发送 · ctrl-j 换行 · esc 取消 · ctrl-d 退出",
            dim,
        ))),
        Rect::new(bc.x, bc.y + 1, bc.width, 1),
    );

    if overlay_open {
        draw_overlay(frame, cfg, panes);
    }
}

fn draw_overlay(frame: &mut ratatui::Frame, cfg: &Cfg, panes: &Panes) {
    let dim = Style::default().fg(Color::DarkGray);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let middle = Rect::new(
        panes.transcript.x,
        panes.transcript.y,
        panes.transcript.width
            + panes.panel.map(|p| p.width).unwrap_or_default(),
        panes.transcript.height,
    );
    let max_w: u16 = if cfg.wide_overlay { 90 } else { 72 };
    let width = middle.width.saturating_sub(4).min(max_w);
    if width < 20 {
        return;
    }
    let content_w = width.saturating_sub(2) as usize;

    let mut rows: Vec<Line> = Vec::new();
    rows.push(Line::from(Span::styled("[kimi] 思考完成", bold)));
    rows.push(Line::from(" "));

    let section = |rows: &mut Vec<Line>, title: &str, body: &[&str]| match cfg.section {
        Section::Rule => {
            let rule = format!("── {title} {}", "─".repeat(content_w.saturating_sub(cols(title) + 5)));
            rows.push(Line::from(Span::styled(rule, dim)));
            for line in body {
                for row in wrap(line, content_w) {
                    rows.push(Line::from(row));
                }
            }
            rows.push(Line::from(" "));
        }
        Section::Colon => {
            rows.push(Line::from(Span::styled(format!("{title}："), bold)));
            for line in body {
                for row in wrap(line, content_w) {
                    rows.push(Line::from(row));
                }
            }
            rows.push(Line::from(" "));
        }
    };

    section(
        &mut rows,
        "思考",
        &[
            "先把提示行落成普通转录源行，这样它会计入 20 000 源行上限、参与吸底。",
            "命中映射需要一个「显示行 → 块」的表；票 01 查明 Pane 目前只有行级结构。",
        ],
    );
    section(
        &mut rows,
        "参数",
        &["command = \"cargo test --all-targets\"", "cwd = \"/home/forty/code/fortystory/fs-agent\""],
    );
    section(
        &mut rows,
        "输出",
        &[
            "test result: ok. 633 passed; 0 failed; 0 ignored",
            "[truncated: 12 044 chars, ~3 011 tokens; full output at outputs/abc.txt]",
        ],
    );
    while rows.last().map(|l| l.width() == 0).unwrap_or(false) {
        rows.pop();
    }
    rows.push(Line::from(Span::styled("↕ 12/58 · esc 关闭", dim)));

    let height = (rows.len() as u16 + 2).min(middle.height);
    let area = Rect::new(
        middle.x + (middle.width.saturating_sub(width)) / 2,
        middle.y + (middle.height.saturating_sub(height)) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    let body: Vec<Line> = rows.into_iter().take(height.saturating_sub(2) as usize).collect();
    frame.render_widget(
        Paragraph::new(body).block(Block::default().borders(Borders::ALL).border_style(dim)),
        area,
    );
}

fn render(cfg: &Cfg, w: u16, h: u16) -> String {
    let panes = plan(w, h);
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).expect("TestBackend terminal");
    terminal
        .draw(|frame| draw(frame, cfg, &panes, cfg.overlay))
        .expect("draw");
    dump(terminal.backend(), w, h, cfg)
}

fn dump(backend: &TestBackend, w: u16, h: u16, cfg: &Cfg) -> String {
    let mut out = String::new();
    let click = match cfg.click {
        Click::Marker => "marker ▸",
        Click::Underline => "underline",
        Click::Suffix => "suffix [详情]",
        Click::None => "none (control)",
    };
    let section = match cfg.section {
        Section::Rule => "── rule",
        Section::Colon => "label：",
    };
    writeln!(
        out,
        "size: {w}x{h}   click: {click} | section: {section} | thinking glyph: {} | overlay: {}",
        if cfg.thinking_glyph { "on" } else { "off" },
        if cfg.overlay {
            if cfg.wide_overlay { "open (wide 90)" } else { "open (72)" }
        } else {
            "closed"
        }
    )
    .unwrap();
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
    out.push_str(&style_footer(cfg));
    out
}

fn trunc(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let w = cw(ch);
        if used + w > width {
            out.push('…');
            break;
        }
        out.push(ch);
        used += w;
    }
    out
}

/// The text dump shows symbols only, so the colours and modifiers each variant
/// applies are listed here — that is the half of this probe a plain dump loses.
fn style_footer(cfg: &Cfg) -> String {
    let mut out = String::from("\nstyles (dump shows symbols only):\n");
    for entry in transcript(cfg) {
        let mut style = entry.style;
        let mut affordance = String::new();
        if entry.clickable {
            match cfg.click {
                Click::Underline => {
                    style = style.add_modifier(Modifier::UNDERLINED);
                    affordance = "[clickable: underline]".to_owned();
                }
                Click::Marker => affordance = "[clickable: ▸ prefix]".to_owned(),
                Click::Suffix => affordance = "[clickable: [详情] suffix]".to_owned(),
                Click::None => affordance = "[clickable: no affordance]".to_owned(),
            }
        }
        let fg = format!("{:?}", style.fg.unwrap_or(Color::Reset));
        let mut mods = Vec::new();
        if style.add_modifier.contains(Modifier::BOLD) {
            mods.push("BOLD");
        }
        if style.add_modifier.contains(Modifier::UNDERLINED) {
            mods.push("UNDERLINED");
        }
        writeln!(
            out,
            "  {:<46} fg={:<10} {:<15} {}",
            trunc(&entry.text, 44),
            fg,
            mods.join("+"),
            affordance
        )
        .unwrap();
    }
    if cfg.overlay {
        writeln!(
            out,
            "  overlay section style: {}",
            match cfg.section {
                Section::Rule => "── title ── rule, DarkGray",
                Section::Colon => "title：label, BOLD",
            }
        )
        .unwrap();
        writeln!(
            out,
            "  overlay width: {} (centred over the middle)",
            if cfg.wide_overlay { 90 } else { 72 }
        )
        .unwrap();
    }
    out
}

// ---------------------------------------------------------------------------
// Screen catalogue
// ---------------------------------------------------------------------------

struct Case {
    file: &'static str,
    cfg: Cfg,
    w: u16,
    h: u16,
}

fn main() {
    let out_dir = PathBuf::from(std::env::args().nth(1).expect("usage: probe <out-dir>"));
    fs::create_dir_all(&out_dir).expect("mkdir");

    let base = Cfg::BASE;
    let cases = vec![
        // --- clickable affordance, one size each ---------------------------
        Case { file: "variant-click-marker.txt", cfg: Cfg { click: Click::Marker, ..base }, w: 120, h: 24 },
        Case { file: "variant-click-underline.txt", cfg: Cfg { click: Click::Underline, ..base }, w: 120, h: 24 },
        Case { file: "variant-click-suffix.txt", cfg: Cfg { click: Click::Suffix, ..base }, w: 120, h: 24 },
        Case { file: "variant-click-none.txt", cfg: Cfg { click: Click::None, ..base }, w: 120, h: 24 },
        // --- thinking-line glyph ------------------------------------------
        Case { file: "variant-thinking-glyph.txt", cfg: Cfg { thinking_glyph: true, ..base }, w: 120, h: 24 },
        // --- detail overlay -----------------------------------------------
        Case { file: "variant-detail-rule.txt", cfg: Cfg { overlay: true, ..base }, w: 120, h: 24 },
        Case { file: "variant-detail-colon.txt", cfg: Cfg { overlay: true, section: Section::Colon, ..base }, w: 120, h: 24 },
        Case { file: "variant-detail-wide.txt", cfg: Cfg { overlay: true, wide_overlay: true, ..base }, w: 120, h: 24 },
        Case { file: "variant-detail-80x24.txt", cfg: Cfg { overlay: true, ..base }, w: 80, h: 24 },
        // --- size coverage of the base form -------------------------------
        Case { file: "variant-size-40x10.txt", cfg: base, w: 40, h: 10 },
        Case { file: "variant-size-80x24.txt", cfg: base, w: 80, h: 24 },
        Case { file: "variant-size-120x10.txt", cfg: base, w: 120, h: 10 },
        Case { file: "variant-size-120x24.txt", cfg: base, w: 120, h: 24 },
        // --- the chosen combination (user pick, 2026-09-23) ----------------
        Case { file: "chosen-40x10.txt", cfg: CHOSEN, w: 40, h: 10 },
        Case { file: "chosen-80x24.txt", cfg: CHOSEN, w: 80, h: 24 },
        Case { file: "chosen-120x10.txt", cfg: CHOSEN, w: 120, h: 10 },
        Case { file: "chosen-120x24.txt", cfg: CHOSEN, w: 120, h: 24 },
        Case { file: "chosen-detail-120x24.txt", cfg: Cfg { overlay: true, ..CHOSEN }, w: 120, h: 24 },
        Case { file: "chosen-detail-80x24.txt", cfg: Cfg { overlay: true, ..CHOSEN }, w: 80, h: 24 },
    ];

    for case in &cases {
        let snap = render(&case.cfg, case.w, case.h);
        fs::write(out_dir.join(case.file), snap).expect("write snapshot");
        println!("wrote {}", case.file);
    }
}
