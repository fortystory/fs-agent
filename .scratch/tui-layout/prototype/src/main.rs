//! THROWAWAY probe for ticket 02: four-pane geometry + degrade ladder.
//!
//! **Revision 2 — the chosen look.** The first revision rendered a 1-column dim
//! vertical rule between transcript and panel, with no other borders and no
//! blank rows. The user looked at those pictures and chose instead:
//!
//! * every one of the four regions carries its own `Block` border
//!   (header / transcript / panel / bottom), and the header block's **bottom
//!   border is** the horizontal rule under the header — no second rule is drawn;
//! * `airy` spacing: one blank row under the header, one blank row above the
//!   bottom block;
//! * header fields separated by ` · `, panel share 26%, input continuation
//!   indent 2 columns (all unchanged).
//!
//! The pre-decision snapshots (`screens-*.txt`, `variant-*.txt`) are left on
//! disk as the rejected record; this binary only rewrites `chosen-*.txt`.
//!
//! Every Chinese string is copied verbatim from `src/render/wording.rs` or from
//! ticket 05's proposed `PANEL_*` labels — see `mod w`.
//!
//! Usage: `tui-layout-probe <output-dir>`

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

// ---------------------------------------------------------------------------
// Chinese wording, copied from src/render/wording.rs
// ---------------------------------------------------------------------------

mod w {
    /// wording.rs:402-408
    pub fn banner(session: &str, model: &str, mode: &str, dir: &str) -> String {
        format!("fs-agent：会话 {session} · 模型 {model} · 模式 {mode} · {dir}")
    }
    /// wording.rs:392-399
    pub fn mode_label(mode: &str) -> &'static str {
        match mode {
            "readonly" => "只读",
            "ask" => "询问",
            "auto" => "自动",
            _ => "计划",
        }
    }
    /// wording.rs:37-39
    pub fn round_section(round: u32, mode: &str) -> String {
        format!("── 第 {round} 轮（{mode}）──")
    }
    /// wording.rs:28-34
    pub const ROUND_MODE_INDEPENDENT: &str = "独立首轮";
    /// wording.rs:42-44
    pub fn turn_started(iteration: u32) -> String {
        format!("回合开始（第 {iteration} 次迭代）")
    }
    /// wording.rs:82-84
    pub fn tool_call(tool: &str, args: &str) -> String {
        format!("调用 {tool}({args})")
    }
    /// wording.rs:88-95
    pub fn tool_output_preview(text: &str, max_chars: usize) -> String {
        if text.chars().count() <= max_chars {
            return text.to_owned();
        }
        let mut out: String = text.chars().take(max_chars).collect();
        out.push_str("…（结果已省略）");
        out
    }
    /// wording.rs:176-178
    pub const TOOL_COMPLETED: &str = "工具完成";
    /// wording.rs:170-172
    pub const MESSAGE_COMPLETE: &str = "消息完成";
    /// wording.rs:304-311
    pub const SPEAKER_USER: &str = "[用户]";
    pub const SPEAKER_MAIN: &str = "[主控]";
    /// wording.rs:314-316 + 319-326
    pub const CONTEXT_PLAN_MODE: &str = "[上下文注入：计划模式]";
    /// wording.rs:330-343
    pub const HISTORY_MODE_CHANGE: &str = "[历史：模式变更] 历史已被取代";
    /// wording.rs:147-149
    pub fn agent_error(message: &str) -> String {
        format!("错误：{message}")
    }
    /// wording.rs:98-100
    pub const NO_TOOL_RESULT: &str = "（流上没有结果）";
    /// wording.rs:450-455
    pub const PLAN_ENTERED: &str = "已进入计划模式";

    /// ticket 05 §6, proposed labels (not yet in wording.rs).
    pub const PANEL_MODEL: &str = "模型";
    pub const PANEL_CONTEXT: &str = "上下文";
    pub const PANEL_TOKENS: &str = "token";
    pub const PANEL_TURNS: &str = "回合";
    pub const PANEL_INPUT: &str = "输入";
    pub const PANEL_OUTPUT: &str = "输出";
    pub const PANEL_CACHE: &str = "缓存";

    /// The hint line, in display order.
    ///
    /// `enter 发送` / `esc 取消` / `shift+tab 计划` / `ctrl-c 退出` are the four
    /// existing ones (`wording.rs:366`), kept verbatim and in relative order.
    /// `ctrl-j 换行` and `PgUp/PgDn 滚动` are this effort's additions.
    ///
    /// **`shift+enter 换行` is deliberately absent**: ticket 04 decided not to
    /// enable the keyboard-enhancement protocol, so Shift+Enter arrives as a
    /// plain Enter (submit) and must never be advertised as a newline.
    pub const KEY_HINTS: [&str; 6] = [
        "enter 发送",
        "ctrl-j 换行",
        "esc 取消",
        "shift+tab 计划",
        "PgUp/PgDn 滚动",
        "ctrl-c 退出",
    ];
    pub const HINT_EXIT: &str = "ctrl-c 退出";

    /// New string this ticket proposes (not in wording.rs yet).
    pub const TOO_SMALL: &str = "终端太小：至少 40×10";
}

// ---------------------------------------------------------------------------
// Column arithmetic (one CJK character = 2 columns)
// ---------------------------------------------------------------------------

/// Display width of one character, in terminal columns.
///
/// East-Asian Wide/Fullwidth ranges only; good enough for the exact strings
/// this probe draws. The snapshot itself is produced by ratatui, not by this
/// function — this only decides wrapping/fitting before the widget runs.
fn char_w(c: char) -> usize {
    let cp = c as u32;
    let wide = (0x1100..=0x115F).contains(&cp)
        || ((0x2E80..=0xA4CF).contains(&cp) && cp != 0x303F)
        || (0xAC00..=0xD7A3).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0xFE30..=0xFE6F).contains(&cp)
        || (0xFF00..=0xFF60).contains(&cp)
        || (0xFFE0..=0xFFE6).contains(&cp)
        || (0x20000..=0x3FFFD).contains(&cp);
    if cp == 0 {
        0
    } else if wide {
        2
    } else {
        1
    }
}

fn cols(s: &str) -> usize {
    s.chars().map(char_w).sum()
}

/// Longest prefix of `s` that fits in `width` columns, with `…` appended when
/// something was cut (ticket 05 §6: right-truncate the model name).
fn truncate_cols(s: &str, width: usize) -> String {
    if cols(s) <= width {
        return s.to_owned();
    }
    let budget = width.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = char_w(ch);
        if used + cw > budget {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    out
}

/// Pre-wrap `s` at `width` columns, breaking on character boundaries.
fn wrap_cols(s: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    if s.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = char_w(ch);
        if used + cw > width && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            used = 0;
        }
        cur.push(ch);
        used += cw;
    }
    out.push(cur);
    out
}

/// Pad `s` on the right to `width` columns.
fn pad_right(s: &str, width: usize) -> String {
    let mut out = s.to_owned();
    let used = cols(s);
    for _ in used..width {
        out.push(' ');
    }
    out
}

/// Left-pad `s` so it ends at `width` columns (numbers are right-aligned).
fn pad_left(s: &str, width: usize) -> String {
    let used = cols(s);
    if used >= width {
        return s.to_owned();
    }
    let mut out = " ".repeat(width - used);
    out.push_str(s);
    out
}

// ---------------------------------------------------------------------------
// The chosen combination, as constants
// ---------------------------------------------------------------------------

/// Frozen in charting: the smallest terminal the TUI claims to support.
const MIN_W: u16 = 40;
const MIN_H: u16 = 10;

/// The right pane is a clamped share of the width, not a fixed column count.
const PANEL_PCT: u32 = 26;
const PANEL_MIN: u16 = 25;
const PANEL_MAX: u16 = 31;

/// The right pane only appears from this width on.
const PANEL_MIN_W: u16 = 80;
/// The four core panel rows must fit, or the whole pane is hidden.
const PANEL_MIN_ROWS: u16 = 4;
/// The transcript must keep at least this many content columns.
const TRANSCRIPT_MIN_COLS: u16 = 20;

/// Rows the chrome itself costs in the chosen look:
/// header borders (2) + middle borders (2) + bottom borders (2) + hints (1).
/// The header's *bottom* border is the rule under the header — it is one of
/// these two header rows, not an extra one.
const CHROME: u16 = 7;
/// The input area grows with the draft, capped at 10 rows (frozen).
const INPUT_CAP: u16 = 10;

#[derive(Clone, Copy)]
struct Facts {
    version: &'static str,
    cwd: &'static str,
    mode: &'static str,
    clock: &'static str,
    clock_short: &'static str,
    model: &'static str,
    context_used: u64,
    context_usable: u64,
    tokens_used: u64,
    tokens_limit: Option<u64>,
    tokens_in: u64,
    tokens_out: u64,
    cache_hit: u64,
    cache_miss: u64,
    turns: u64,
}

impl Default for Facts {
    fn default() -> Self {
        Self {
            version: "0.1.0",
            cwd: "~/code/fortystory/fs-agent",
            mode: "ask",
            clock: "2026-09-21 14:32",
            clock_short: "14:32",
            model: "claude-sonnet-4-5",
            context_used: 12_345,
            context_usable: 200_000,
            tokens_used: 12_345,
            tokens_limit: Some(100_000),
            tokens_in: 9_000,
            tokens_out: 3_345,
            cache_hit: 9_000,
            cache_miss: 3_345,
            turns: 7,
        }
    }
}

/// The transcript, as raw unwrapped lines of real wording.
fn transcript_lines(f: &Facts) -> Vec<String> {
    vec![
        w::banner("01J8ZQ4K7M", f.model, w::mode_label(f.mode), f.cwd),
        w::round_section(1, w::ROUND_MODE_INDEPENDENT),
        format!(
            "{} 把渲染层改成四分区全屏布局，右栏放模型与 token。",
            w::SPEAKER_USER
        ),
        format!("{} 好的，先读 src/render/tui.rs 看现在的 draw_live。", w::SPEAKER_MAIN),
        w::tool_call("read", "path=\"src/render/tui.rs\""),
        w::TOOL_COMPLETED.to_owned(),
        w::tool_output_preview(
            "pub fn paint_scrollback(lines: &[Line<'_>], buf: &mut Buffer) { … }",
            24,
        ),
        format!("{} 现在把 draw_live 拆成四块，用一个 plan() 算出每个 Rect。", w::SPEAKER_MAIN),
        w::turn_started(1),
        w::CONTEXT_PLAN_MODE.to_owned(),
        w::HISTORY_MODE_CHANGE.to_owned(),
        w::agent_error("provider 返回 429，稍后重试"),
        w::NO_TOOL_RESULT.to_owned(),
        w::PLAN_ENTERED.to_owned(),
        w::MESSAGE_COMPLETE.to_owned(),
    ]
}

const DRAFT_EMPTY: &str = "";
const DRAFT_THREE: &str = "把 draw_live 改成四分区，第二行起留出两格缩进。\n第二行：输入区随内容长高，上限 10 行。\n第三行：超过上限时内部滚动到光标。";
const DRAFT_TWELVE: &str = "第一行：这一行只是用来把草稿撑过十行上限。\n第二行\n第三行\n第四行\n第五行\n第六行\n第七行\n第八行\n第九行\n第十行\n第十一行\n第十二行";

// ---------------------------------------------------------------------------
// The layout itself
// ---------------------------------------------------------------------------

struct Plan {
    /// Header content rows (inside the header block's borders).
    header_rows: u16,
    /// Whether the two blank airy rows survived at this height.
    airy: bool,
    /// Transcript block inner height.
    middle_rows: u16,
    input_rows: u16,
    header: Rect,
    transcript: Rect,
    transcript_content: Rect,
    panel: Option<Rect>,
    panel_content: Option<Rect>,
    bottom: Rect,
    input: Rect,
    hints: Rect,
}

fn panel_outer_width(w: u16) -> u16 {
    ((w as u32 * PANEL_PCT / 100) as u16).clamp(PANEL_MIN, PANEL_MAX)
}

/// Decide the geometry for a `w` x `h` terminal. `None` means "too small".
///
/// The search order encodes the priorities:
///   1. the header keeps its second row as long as it can — compressing it is a
///      structural loss, while `airy` is only two blank rows;
///   2. `airy` is therefore the first thing given up, and only when it does not
///      fit at all;
///   3. the panel is gated separately below (it is the first region hidden, per
///      the frozen order), and because it needs four middle content rows, every
///      height at which the header falls back to one row already has the panel
///      hidden — so the header never compresses while the panel is on screen.
fn plan(w: u16, h: u16, draft_rows_req: u16) -> Option<Plan> {
    if w < MIN_W || h < MIN_H {
        return None;
    }
    let wants_two_row_header = w >= 60;

    let mut picked: Option<(bool, u16, u16)> = None; // (airy, header_rows, fixed)
    'outer: for header_rows in [2u16, 1] {
        if header_rows == 2 && !wants_two_row_header {
            continue;
        }
        for airy in [true, false] {
            let airy_rows = if airy { 2 } else { 0 };
            let fixed = CHROME + header_rows + airy_rows;
            // One input row and one transcript row are the structural minimum.
            if h < fixed + 2 {
                continue;
            }
            picked = Some((airy, header_rows, fixed));
            break 'outer;
        }
    }
    let (airy, header_rows, fixed) = picked?;

    let cap_input = (h - fixed - 1).min(INPUT_CAP).max(1);
    let input_rows = draft_rows_req.clamp(1, cap_input);
    let middle_rows = h - fixed - input_rows;
    let airy_rows: u16 = if airy { 1 } else { 0 };

    let header = Rect::new(0, 0, w, header_rows + 2);
    let mut y = header_rows + 2 + airy_rows;

    let middle_h = middle_rows + 2;
    let pw = panel_outer_width(w);
    let show_panel = w >= PANEL_MIN_W
        && middle_rows >= PANEL_MIN_ROWS
        && pw - 2 >= 23
        && (w - pw - 1) >= TRANSCRIPT_MIN_COLS;
    let tw = if show_panel { w - pw + 1 } else { w };

    let transcript = Rect::new(0, y, tw, middle_h);
    let transcript_content = Rect::new(1, y + 1, tw - 2, middle_rows);
    let (panel, panel_content) = if show_panel {
        let p = Rect::new(tw - 1, y, w - tw + 1, middle_h);
        (
            Some(p),
            Some(Rect::new(p.x + 1, p.y + 1, p.width - 2, middle_rows)),
        )
    } else {
        (None, None)
    };
    y += middle_h + airy_rows;

    let bottom = Rect::new(0, y, w, input_rows + 1 + 2);
    let input = Rect::new(1, y + 1, w - 2, input_rows);
    let hints = Rect::new(1, y + 1 + input_rows, w - 2, 1);

    Some(Plan {
        header_rows,
        airy,
        middle_rows,
        input_rows,
        header,
        transcript,
        transcript_content,
        panel,
        panel_content,
        bottom,
        input,
        hints,
    })
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn header_lines(f: &Facts, inner_w: u16, rows: u16) -> Vec<Line<'static>> {
    let name = format!("fs-agent {}", f.version);
    let mode = format!("模式 {}", w::mode_label(f.mode));
    let dim = Style::default().fg(Color::DarkGray);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    if rows == 2 {
        let gap0 = (inner_w as usize)
            .saturating_sub(cols(&name) + cols(f.clock))
            .max(1);
        let cwd = truncate_cols(f.cwd, (inner_w as usize).saturating_sub(cols(&mode) + 1));
        let gap1 = (inner_w as usize)
            .saturating_sub(cols(&cwd) + cols(&mode))
            .max(1);
        let row0 = format!("{name}{}{}", " ".repeat(gap0), f.clock);
        let row1 = format!("{cwd}{}{mode}", " ".repeat(gap1));
        return vec![
            Line::from(vec![
                Span::styled(name.clone(), bold),
                Span::styled(row0[name.len()..].to_owned(), dim),
            ]),
            Line::from(vec![
                Span::styled(cwd.clone(), dim),
                Span::styled(row1[cwd.len()..].to_owned(), Style::default()),
            ]),
        ];
    }
    // 1-row header: name · cwd · mode · clock, dropping cwd first, then clock.
    let mut fields = vec![name, f.cwd.to_owned(), mode, f.clock_short.to_owned()];
    loop {
        let candidate = fields.join(" · ");
        if cols(&candidate) <= inner_w as usize || fields.len() <= 2 {
            return vec![Line::from(vec![
                Span::styled(fields[0].clone(), bold),
                Span::styled(candidate[fields[0].len()..].to_owned(), Style::default()),
            ])];
        }
        if fields.len() > 3 {
            fields.remove(1);
        } else {
            fields.pop();
        }
    }
}

fn hints_line(inner_w: u16) -> Line<'static> {
    let mut shown = String::new();
    for h in w::KEY_HINTS.iter().filter(|h| **h != w::HINT_EXIT) {
        let cand = if shown.is_empty() {
            (*h).to_owned()
        } else {
            format!("{shown} · {h}")
        };
        if cols(&format!("{cand} · {}", w::HINT_EXIT)) <= inner_w as usize {
            shown = cand;
        }
    }
    let line = if shown.is_empty() {
        w::HINT_EXIT.to_owned()
    } else {
        format!("{shown} · {}", w::HINT_EXIT)
    };
    let line = if cols(&line) <= inner_w as usize {
        line
    } else {
        w::HINT_EXIT.to_owned()
    };
    Line::from(Span::styled(line, Style::default().fg(Color::DarkGray)))
}

/// The right pane: label column (6 cols) + 1 space + right-aligned values.
///
/// Returns the rows plus whether the context percentage survived.
fn panel_rows(f: &Facts, content_w: u16, rows_avail: u16) -> (Vec<Line<'static>>, bool) {
    const LABEL_W: usize = 6; // 上下文 is the widest label
    let value_area = (content_w as usize).saturating_sub(LABEL_W + 1);

    let context = format!(
        "{} / {}",
        thousands(f.context_used),
        thousands(f.context_usable)
    );
    let context_pct = format!(
        "{}（{}%）",
        context,
        f.context_used * 100 / f.context_usable.max(1)
    );
    let tokens = match f.tokens_limit {
        Some(limit) => format!("{} / {}", thousands(f.tokens_used), thousands(limit)),
        None => thousands(f.tokens_used),
    };
    let cache = format!("{} / {}", thousands(f.cache_hit), thousands(f.cache_miss));

    // Width-driven drops (ticket 05 §7 order: cache -> in/out -> context %).
    let show_pct = cols(&context_pct) <= value_area;
    let show_details = value_area >= 5;
    let show_cache = cols(&cache) <= value_area && show_details;

    let mut rows: Vec<(String, String, bool)> = Vec::new(); // (label, value, right_align)
    rows.push((
        w::PANEL_MODEL.to_owned(),
        truncate_cols(f.model, value_area),
        false,
    ));
    rows.push((
        w::PANEL_CONTEXT.to_owned(),
        if show_pct { context_pct } else { context },
        true,
    ));
    rows.push((w::PANEL_TOKENS.to_owned(), tokens, true));
    rows.push((w::PANEL_TURNS.to_owned(), f.turns.to_string(), true));
    if show_details {
        rows.push((w::PANEL_INPUT.to_owned(), thousands(f.tokens_in), true));
        rows.push((w::PANEL_OUTPUT.to_owned(), thousands(f.tokens_out), true));
    }
    if show_cache {
        rows.push((w::PANEL_CACHE.to_owned(), cache, true));
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, (label, value, right)) in rows.iter().enumerate() {
        if i as u16 >= rows_avail {
            break;
        }
        let label = pad_right(label, LABEL_W);
        let value = if *right {
            pad_left(value, value_area)
        } else {
            pad_right(value, value_area)
        };
        lines.push(Line::from(vec![
            Span::styled(label, Style::default().fg(Color::DarkGray)),
            Span::styled(" ".to_owned(), Style::default()),
            Span::styled(value, Style::default()),
        ]));
    }
    (lines, show_pct)
}

/// A short honest label for what this geometry actually degraded.
fn degrade_level(p: &Plan, show_pct: bool) -> String {
    let hdr = if p.header_rows == 2 { "hdr2" } else { "hdr1" };
    let airy = if p.airy { "airy" } else { "no-airy" };
    let pane = match p.panel_content {
        Some(_) if show_pct => "panel+%",
        Some(_) => "panel",
        None => "panel-off",
    };
    format!("{hdr}/{airy}/{pane}")
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn render(f: &Facts, w: u16, h: u16, draft: &str, busy: bool) -> String {
    let draft_rows_req = draft
        .split('\n')
        .map(|l| wrap_cols(l, (w as usize).saturating_sub(2).max(1)).len() as u16)
        .sum::<u16>()
        .max(1);

    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).expect("TestBackend terminal");

    let Some(p) = plan(w, h, draft_rows_req) else {
        terminal
            .draw(|frame| {
                let area = frame.area();
                let line = w::TOO_SMALL;
                let y = area.height / 2;
                let x = (area.width as usize).saturating_sub(cols(line)) / 2;
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        line,
                        Style::default().fg(Color::Red),
                    ))),
                    Rect::new(x as u16, y, area.width.saturating_sub(x as u16), 1),
                );
            })
            .expect("draw");
        return dump(terminal.backend(), w, h, "TOO-SMALL", None);
    };

    let (panel_lines, show_pct) = match p.panel_content {
        Some(content) => panel_rows(f, content.width, content.height),
        None => (Vec::new(), false),
    };
    let summary = degrade_level(&p, show_pct);

    let transcript = transcript_lines(f);
    let input_lines: Vec<String> = draft
        .split('\n')
        .flat_map(|l| wrap_cols(l, (w as usize).saturating_sub(2).max(1)))
        .collect();
    let input_rows = usize::from(p.input_rows);
    let shown_input: Vec<String> = if input_lines.len() > input_rows {
        input_lines[input_lines.len() - input_rows..].to_vec()
    } else {
        input_lines
    };

    let dim = Style::default().fg(Color::DarkGray);
    let header_block_lines = header_lines(f, w - 2, p.header_rows);
    let hint_line = hints_line(w - 2);
    let p2 = &p;

    terminal
        .draw(|frame| {
            // --- four bordered regions -------------------------------------
            let block = || Block::default().borders(Borders::ALL).border_style(dim);

            // header: its bottom border IS the horizontal rule; no extra rule.
            frame.render_widget(
                Paragraph::new(header_block_lines.clone()).block(block()),
                p2.header,
            );

            // transcript
            frame.render_widget(block(), p2.transcript);
            let tw = p2.transcript_content.width as usize;
            let mut wrapped: Vec<String> = Vec::new();
            for raw in &transcript {
                wrapped.extend(wrap_cols(raw, tw.max(1)));
            }
            let th = p2.transcript_content.height as usize;
            let shown: Vec<String> = if wrapped.len() > th {
                wrapped[wrapped.len() - th..].to_vec()
            } else {
                let mut v = vec![String::new(); th - wrapped.len()];
                v.extend(wrapped);
                v
            };
            for (i, line) in shown.iter().enumerate() {
                let style = if line.starts_with("错误") {
                    Style::default().fg(Color::Red)
                } else if line.starts_with("调用 ") {
                    Style::default().fg(Color::Yellow)
                } else if line.starts_with('[') {
                    dim
                } else {
                    Style::default()
                };
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(line.clone(), style))),
                    Rect::new(
                        p2.transcript_content.x,
                        p2.transcript_content.y + i as u16,
                        p2.transcript_content.width,
                        1,
                    ),
                );
            }

            // panel
            if let (Some(panel), Some(content)) = (p2.panel, p2.panel_content) {
                frame.render_widget(block(), panel);
                for (i, line) in panel_lines.iter().enumerate() {
                    frame.render_widget(
                        Paragraph::new(line.clone()),
                        Rect::new(content.x, content.y + i as u16, content.width, 1),
                    );
                }
                // The two blocks share the boundary column; patch the corners
                // into T-junctions so the middle reads as one split box.
                let bx = panel.x;
                let top = (bx, panel.y);
                let bottom = (bx, panel.y + panel.height - 1);
                if let Some(cell) = frame.buffer_mut().cell_mut(top) {
                    cell.set_symbol("┬");
                }
                if let Some(cell) = frame.buffer_mut().cell_mut(bottom) {
                    cell.set_symbol("┴");
                }
            }

            // bottom block: input rows, then the hint line inside the border
            frame.render_widget(block(), p2.bottom);
            for (i, line) in shown_input.iter().enumerate() {
                let prompt = if i == 0 {
                    "> ".to_owned()
                } else {
                    "  ".to_owned()
                };
                let style = if i == 0 {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(prompt, style),
                        Span::styled(line.clone(), Style::default()),
                    ])),
                    Rect::new(p2.input.x, p2.input.y + i as u16, p2.input.width, 1),
                );
            }
            let _ = busy;
            frame.render_widget(Paragraph::new(hint_line.clone()), p2.hints);
        })
        .expect("draw");

    dump(terminal.backend(), w, h, &summary, Some(&p))
}

fn dump(backend: &TestBackend, w: u16, h: u16, level: &str, plan: Option<&Plan>) -> String {
    let mut out = String::new();
    writeln!(out, "size: {w}x{h}   {level}").unwrap();
    if let Some(p) = plan {
        let panel = match (p.panel, p.panel_content) {
            (Some(outer), Some(inner)) => format!(
                "panel outer {}x{} / content {}x{}",
                outer.width, outer.height, inner.width, inner.height
            ),
            _ => "panel hidden".to_owned(),
        };
        writeln!(
            out,
            "regions: header content {} rows (block {} rows) | transcript content {}x{} | {} | input {} rows | hints 1 | airy {}",
            p.header_rows,
            p.header.height,
            p.transcript_content.width,
            p.transcript_content.height,
            panel,
            p.input_rows,
            if p.airy { "on" } else { "OFF" }
        )
        .unwrap();
    }
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
        // TestBackend's Display appends this note for cells hidden by a wide
        // symbol; drop it, then drop the quotes it wraps each row in.
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

// ---------------------------------------------------------------------------
// Screen catalogue
// ---------------------------------------------------------------------------

struct Case {
    file: String,
    w: u16,
    h: u16,
    draft: &'static str,
    question: &'static str,
}

fn main() {
    let out_dir = std::env::args().nth(1).expect("usage: probe <out-dir>");
    let out = Path::new(&out_dir);
    fs::create_dir_all(out).expect("create out dir");

    let f = Facts::default();
    let mut cases: Vec<Case> = Vec::new();
    let mut push = |file: &str, w: u16, h: u16, draft: &'static str, question: &'static str| {
        cases.push(Case {
            file: file.to_owned(),
            w,
            h,
            draft,
            question,
        });
    };

    // --- the chosen look, one screen per degrade step ----------------------
    push(
        "chosen-40x10.txt",
        40,
        10,
        DRAFT_EMPTY,
        "冻结的最小尺寸：四个边框区域都在，但 airy 留白放不下（转录只剩 1 行内容）",
    );
    push(
        "chosen-40x12.txt",
        40,
        12,
        DRAFT_EMPTY,
        "带 airy 的完整新基线真正需要的最小高度：40x12",
    );
    push(
        "chosen-40x24.txt",
        40,
        24,
        DRAFT_EMPTY,
        "窄屏：无右栏，header 1 行（cwd 被丢），airy 保留",
    );
    push(
        "chosen-60x24.txt",
        60,
        24,
        DRAFT_EMPTY,
        "右栏仍隐藏（<80）；header 2 行；airy 保留",
    );
    push(
        "chosen-80x16.txt",
        80,
        16,
        DRAFT_EMPTY,
        "右栏出现的最低高度：中段内容 4 行，只放得下四个核心字段",
    );
    push(
        "chosen-80x24.txt",
        80,
        24,
        DRAFT_EMPTY,
        "右栏最窄档：外框 25 列 / 内容 23 列，上下文百分比被丢",
    );
    push(
        "chosen-120x24.txt",
        120,
        24,
        DRAFT_EMPTY,
        "正常尺寸：header 2 行 + airy + 右栏 31 列（内容 29，含上下文 %）",
    );
    push(
        "chosen-174x50.txt",
        174,
        50,
        DRAFT_EMPTY,
        "很宽：右栏封顶 31 列，转录吃满剩余",
    );
    push(
        "chosen-120x24-input-3-lines.txt",
        120,
        24,
        DRAFT_THREE,
        "输入区 3 行：中段从 12 行降到 10 行，右栏仍满字段",
    );
    push(
        "chosen-120x24-input-12-lines.txt",
        120,
        24,
        DRAFT_TWELVE,
        "草稿 12 行：输入区吃满 10 行上限，中段只剩 3 行 -> 右栏整个被挤掉",
    );
    push(
        "chosen-too-small-39x24.txt",
        39,
        24,
        DRAFT_EMPTY,
        "宽度 39：低于最小 40，只显示「终端太小」",
    );
    push(
        "chosen-too-small-40x9.txt",
        40,
        9,
        DRAFT_EMPTY,
        "高度 9：低于最小 10，只显示「终端太小」",
    );

    let mut index = String::from(
        "# ticket 02 prototype snapshots —— 选定组合（2026-09-21 用户看图为证）\n\n\
         **选定 = 四个区域各自 Block 边框（header 底边框即分隔横线）+ airy 留白（header 下、\
         输入区上各 1 行）+ 右栏 26% 夹紧 + header 分隔符 ` · ` + 输入续行缩进 2 格。**\n\n\
         每一张都由 `tui-layout-probe`（修订 2）用 ratatui `TestBackend` 渲染成固定尺寸 buffer 后 dump，\
         **不是手绘**。命令：\n\n\
         ```sh\n\
         CARGO_TARGET_DIR=/tmp/tui-layout-probe-target cargo run --offline --manifest-path \\\n\
           .scratch/tui-layout/prototype/Cargo.toml -- .scratch/tui-layout/prototype\n\
         ```\n\n\
         | 文件 | 尺寸 | 回答的问题 |\n| --- | --- | --- |\n",
    );
    for case in &cases {
        let text = render(&f, case.w, case.h, case.draft, false);
        fs::write(out.join(&case.file), &text).expect("write screen");
        writeln!(
            index,
            "| `{}` | {}x{} | {} |",
            case.file, case.w, case.h, case.question
        )
        .unwrap();
        println!("wrote {} ({}x{})", case.file, case.w, case.h);
    }

    index.push_str(
        "\n## 被否决的旧基线（决议前，仅作记录）\n\n\
         `screens-*.txt` 与 `variant-*.txt` 是**第一版探针**的产物，代表被否决的组合：\n\
         中左/中右之间 1 列暗色竖线、其余区域无边框、紧凑留白。\n\
         **注意：那些图里的提示行含 `shift+enter 换行`。票 04 已决定不启用键盘增强协议，\
         该提示是错的、已从本源码删除；不要据旧图实现。**\n\
         当前基线只看 `chosen-*.txt`。\n",
    );

    // --- geometry table, chosen look, per width ----------------------------
    let mut geo = String::from(
        "# chosen geometry —— 每个宽度（h=24，空草稿）\n\n\
         四区皆带 1 行边框；header 底边框即分隔横线；airy = header 下 / 输入区上各 1 行。\n\n\
         | w | header 内容行 | header 块行 | airy | 中段内容行 | 转录外框 x 内容 | 右栏外框 x 内容 | 输入区行 | 底部块行 |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for w in [
        40u16, 48, 60, 72, 80, 88, 96, 100, 104, 112, 116, 119, 120, 128, 144, 174,
    ] {
        match plan(w, 24, 1) {
            None => writeln!(geo, "| {w} | TOO-SMALL | — | — | — | — | — | — | — |").unwrap(),
            Some(p) => {
                let panel = match (p.panel, p.panel_content) {
                    (Some(o), Some(c)) => {
                        format!("{}x{} / {}x{}", o.width, o.height, c.width, c.height)
                    }
                    _ => "hidden".to_owned(),
                };
                writeln!(
                    geo,
                    "| {w} | {} | {} | {} | {} | {}x{} | {} | {} | {} |",
                    p.header_rows,
                    p.header.height,
                    if p.airy { "on" } else { "off" },
                    p.middle_rows,
                    p.transcript.width,
                    p.transcript_content.width,
                    panel,
                    p.input_rows,
                    p.bottom.height,
                )
                .unwrap();
            }
        }
    }

    // --- geometry table, chosen look, per height ---------------------------
    geo.push_str(
        "\n# chosen geometry —— 每个高度（w=120）\n\n\
         | h | header 内容行 | airy | 草稿行 | 输入区行 | 中段内容行 | 右栏用了几行 | 档 |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for h in [10u16, 11, 12, 13, 14, 16, 20, 24, 30, 50] {
        for draft in [1u16, 3, 7, 12] {
            match plan(120, h, draft) {
                None => writeln!(geo, "| {h} | — | — | {draft} | TOO-SMALL | — | — | — |").unwrap(),
                Some(p) => {
                    let (used, pct) = match p.panel_content {
                        Some(c) => {
                            let (lines, pct) = panel_rows(&f, c.width, c.height);
                            (lines.len(), pct)
                        }
                        None => (0, false),
                    };
                    writeln!(
                        geo,
                        "| {h} | {} | {} | {draft} | {} | {} | {} | {} |",
                        p.header_rows,
                        if p.airy { "on" } else { "off" },
                        p.input_rows,
                        p.middle_rows,
                        used,
                        degrade_level(&p, pct)
                    )
                    .unwrap();
                }
            }
        }
    }

    // --- minimum-size matrix ----------------------------------------------
    geo.push_str(
        "\n# 最小尺寸矩阵（哪些尺寸还画得出来、airy 是否还在手上）\n\n\
         | w\\h | 10 | 11 | 12 | 13 | 14 | 15 | 16 |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for w in [40u16, 48, 60, 72, 80, 96, 120] {
        let mut row = format!("| {w} ");
        for h in 10u16..=16 {
            match plan(w, h, 1) {
                None => row.push_str("| 太小 "),
                Some(p) => {
                    let tag = format!(
                        "h{}{}m{}",
                        p.header_rows,
                        if p.airy { "a" } else { "A" },
                        p.middle_rows
                    );
                    row.push_str(&format!("| {tag} "));
                }
            }
        }
        row.push('|');
        writeln!(geo, "{row}").unwrap();
    }
    geo.push_str(
        "\n图例：`hN` = header 内容 N 行；`a` = airy 在，`A` = airy 被丢；`mN` = 中段内容 N 行。\n\
         右栏出现还需要 `w>=80` 且中段内容 >= 4 行 —— 上表里 `m4` 才是右栏能出现的最小高度。\n",
    );

    fs::write(out.join("geometry-table.md"), geo).expect("write geo");
    fs::write(out.join("SNAPSHOTS.md"), index).expect("write index");
    println!("wrote geometry-table.md and SNAPSHOTS.md");
}
