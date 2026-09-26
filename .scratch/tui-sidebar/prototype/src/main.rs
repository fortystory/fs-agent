//! THROWAWAY probe for ticket 01: the new shell geometry (spec §1–§2).
//!
//! Questions this probe answers with pictures:
//!
//! 1. sidebar tier **40 vs 42** (the mark is 38 columns wide, with or without air);
//! 2. the mid tier's **34 vs 28**;
//! 3. the focus cell's colour: **yellow vs bright magenta**;
//! 4. the **40×10** floor: how many columns the transcript keeps, and whether it reads.
//!
//! It is a probe, not product code: one file, no tests, no layers worth keeping. Every
//! frame in `frames/` is rendered by ratatui into a fixed-size `TestBackend` buffer and
//! dumped — nothing here is hand-drawn.
//!
//! Every Chinese string is copied verbatim from `src/render/wording.rs` (line numbers in
//! `mod w`); the handful spec §6 **adds** (`调用量` / `轨迹` / `文件`, the page
//! placeholder, `上下文 <n>%`, the status row's ladder, the rail's glyphs) are marked
//! `NEW`, because they do not exist in wording.rs yet.
//!
//! Usage: `tui-sidebar-probe <prototype-dir>` — writes `<dir>/frames/*.txt`,
//! `<dir>/geometry-table.md` and `<dir>/SNAPSHOTS.md`.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Terminal;

// ---------------------------------------------------------------------------
// Wording, copied from src/render/wording.rs (line numbers are that file's)
// ---------------------------------------------------------------------------

mod w {
    use crate::cols;

    /// wording.rs:1358
    pub fn banner(session: &str, model: &str, mode: &str, dir: &str) -> String {
        format!("fs-agent：会话 {session} · 模型 {model} · 模式 {mode} · {dir}")
    }
    /// wording.rs:1348
    pub fn mode_label(mode: &str) -> &'static str {
        match mode {
            "readonly" => "只读",
            "ask" => "询问",
            "auto" => "自动",
            _ => "计划",
        }
    }
    /// wording.rs:1331
    pub fn mode_field(mode: &str) -> String {
        format!("模式 {}", mode_label(mode))
    }
    /// wording.rs:30
    pub fn round_mode(mode: &str) -> &'static str {
        match mode {
            "targeted" => "定向第二轮",
            "synthesis" => "合成",
            _ => "独立首轮",
        }
    }
    /// wording.rs:39
    pub fn round_section(round: u32, mode: &str) -> String {
        format!("── 第 {round} 轮（{}）──", round_mode(mode))
    }
    /// wording.rs:44
    pub fn turn_started(iteration: u32) -> String {
        format!("回合开始（第 {iteration} 次迭代）")
    }
    /// wording.rs:184
    pub fn tool_call(tool: &str, args: &str) -> String {
        format!("调用 {tool}({args})")
    }
    /// wording.rs:190
    pub fn tool_output_preview(text: &str, max_chars: usize) -> String {
        if text.chars().count() <= max_chars {
            return text.to_owned();
        }
        let mut out: String = text.chars().take(max_chars).collect();
        out.push_str("…（结果已省略）");
        out
    }
    /// wording.rs:568
    pub const TOOL_COMPLETED: &str = "工具完成";
    /// wording.rs:272
    pub const MESSAGE_COMPLETE: &str = "消息完成";
    /// wording.rs:997 (the unattributed speakers)
    pub const SPEAKER_USER: &str = "[用户]";
    pub const SPEAKER_SYSTEM: &str = "[系统]";
    /// wording.rs:1007 + 1017
    pub const CONTEXT_PLAN_MODE: &str = "[上下文注入：计划模式]";
    /// wording.rs:1026 + 1036
    pub const HISTORY_MODE_CHANGE: &str = "[历史：模式变更] 历史已被取代";
    /// wording.rs:249
    pub fn agent_error(message: &str) -> String {
        format!("错误：{message}")
    }
    /// wording.rs:200
    pub const NO_TOOL_RESULT: &str = "（流上没有结果）";
    /// wording.rs:1459
    pub const PLAN_ENTERED: &str = "已进入计划模式";
    /// wording.rs:1219–1229. 模型 stays here because the status row uses it (spec §3
    /// deletes the panel's own 模型 row).
    pub const PANEL_MODEL: &str = "模型";
    pub const PANEL_CONTEXT: &str = "上下文";
    pub const PANEL_TOKENS: &str = "token";
    pub const PANEL_TURNS: &str = "回合";
    pub const PANEL_INPUT: &str = "输入";
    pub const PANEL_OUTPUT: &str = "输出";
    pub const PANEL_CACHE: &str = "缓存";
    /// wording.rs:1232
    pub const PANEL_UNKNOWN: &str = "—";
    /// wording.rs:1235
    pub fn thousands(value: u64) -> String {
        let digits = value.to_string();
        let mut out = String::with_capacity(digits.len() + digits.len() / 3);
        for (index, ch) in digits.chars().enumerate() {
            if index > 0 && (digits.len() - index) % 3 == 0 {
                out.push(',');
            }
            out.push(ch);
        }
        out
    }
    /// wording.rs:1256
    pub fn token_pair(used: u64, limit: Option<u64>) -> String {
        match limit {
            Some(limit) => format!("{} / {}", thousands(used), thousands(limit)),
            None => thousands(used),
        }
    }
    /// wording.rs:1270
    pub fn context_pair(used: Option<u64>, usable: u64, with_share: bool) -> String {
        let Some(used) = used else {
            return PANEL_UNKNOWN.to_owned();
        };
        let pair = format!("{} / {}", thousands(used), thousands(usable));
        if with_share {
            format!("{}（{}%）", pair, used.saturating_mul(100) / usable.max(1))
        } else {
            pair
        }
    }
    /// wording.rs:1284
    pub fn cache_pair(cached: u64, miss: u64) -> String {
        format!("{} / {}", thousands(cached), thousands(miss))
    }
    /// wording.rs:1301
    pub fn too_small(width: u16, height: u16) -> String {
        format!("终端太小：至少 {width}×{height}")
    }
    /// wording.rs:1306
    pub fn identity() -> String {
        "fs-agent 0.1.0".to_owned()
    }
    /// wording.rs:1320
    pub fn logo_lines() -> [&'static str; 5] {
        [
            "▄▀▀█ ▄▀▀█      ▄▀▀▄ ▄▀▀▀ ▄▀▀█ █  █ ▀█▀",
            "▓▄▄  ▓         ▓▄▄▓ ▓ ▀▓ ▓▄▄  ▓▄ ▓  ▓ ",
            "▒     ▀▀▄ ▀▀▀▀ ▒  ▒ ▒  ▒ ▒    ▒ ▀▒  ▒ ",
            "░    ░  ░      ░  ░ ░  ░ ░  ▄ ░  ░  ░ ",
            "▀    ▀▀▀       ▀  ▀  ▀▀▀  ▀▀▀ ▀  ▀  ▀ ",
        ]
    }
    /// wording.rs:1052
    pub fn status_word(busy: bool) -> &'static str {
        if busy {
            "工作中"
        } else {
            "就绪"
        }
    }
    /// wording.rs:1063
    pub const KEY_HINTS: [&str; 5] = [
        "enter 发送",
        "ctrl-j 换行",
        "esc 取消",
        "shift+tab 计划",
        "PgUp/PgDn 滚动",
    ];
    /// wording.rs:1074 / 1079
    pub fn exit_hint(busy: bool) -> &'static str {
        if busy {
            "ctrl-c 退出"
        } else {
            "ctrl-c/ctrl-d 退出"
        }
    }

    // --- NEW: spec §6 wording that is not in wording.rs yet -----------------

    /// spec §6: `TAB_USAGE` / `TAB_TRACE` / `TAB_FILES`.
    pub const TAB_USAGE: &str = "调用量";
    pub const TAB_TRACE: &str = "轨迹";
    pub const TAB_FILES: &str = "文件";

    /// spec §6: `tab_placeholder()`.
    pub const TAB_PLACEHOLDER: &str = "此页尚未实现（另有票在跟）";

    /// spec §6: `context_share(used, usable)` — the status row's short form. Taken to
    /// **include the label**, because §2's fourth rung is `只留 上下文 <n>%（仍带标签）`.
    pub fn context_share(used: Option<u64>, usable: u64) -> String {
        match used {
            Some(used) => format!(
                "{} {}%",
                PANEL_CONTEXT,
                used.saturating_mul(100) / usable.max(1)
            ),
            None => format!("{} {}", PANEL_CONTEXT, PANEL_UNKNOWN),
        }
    }

    /// spec §6: `status_row(model, mode, share, width)`. The ladder is here, not in the
    /// renderer (spec §2, §5); an empty string means "the row and its rule disappear".
    ///
    /// Each segment is padded one column left and right and `│` joins them; whatever is
    /// left over is blank (spec §5).
    pub fn status_row(model: &str, mode: &str, share: &str, width: usize) -> String {
        let seg = |text: &str| format!(" {text} ");
        let full = format!(
            "{}│{}│{}",
            seg(&format!("{PANEL_MODEL} {model}")),
            seg(mode),
            seg(share)
        );
        if cols(&full) <= width {
            return full;
        }
        let two = format!("{}│{}", seg(mode), seg(share));
        if cols(&two) <= width {
            return two;
        }
        let one = seg(share);
        if cols(&one) <= width {
            return one;
        }
        String::new()
    }

    /// spec §6: the rail's three glyph constants.
    pub const RAIL_CELL: &str = "┊";
    pub const RAIL_FOCUS: &str = "┃";
    pub const RAIL_TRUNCATED: &str = "⋮";
}

/// wording.rs:1128–1153, copied because the probe cannot call the crate. The width is
/// the **main column's** content width (spec §2).
fn hint_line(state: &str, hints: &[&str], exit: &str, width: usize) -> String {
    let exit_columns = cols(exit);
    let mut chosen = String::new();
    for hint in hints {
        let candidate = if chosen.is_empty() {
            (*hint).to_owned()
        } else {
            format!("{chosen} · {hint}")
        };
        if cols(&candidate) + 3 + exit_columns > width {
            break;
        }
        chosen = candidate;
    }
    let run = if chosen.is_empty() {
        exit.to_owned()
    } else {
        format!("{chosen} · {exit}")
    };
    let with_state = format!("{state} · {run}");
    if cols(&with_state) <= width {
        with_state
    } else {
        run
    }
}

/// How many hints the ladder kept and whether the state word survived — measured, for
/// the frame headers and the geometry table.
fn hint_summary(busy: bool, width: usize) -> String {
    let exit = w::exit_hint(busy);
    let mut chosen = 0usize;
    let mut text = String::new();
    for hint in w::KEY_HINTS {
        let candidate = if text.is_empty() {
            hint.to_owned()
        } else {
            format!("{text} · {hint}")
        };
        if cols(&candidate) + 3 + cols(exit) > width {
            break;
        }
        text = candidate;
        chosen += 1;
    }
    let run = if text.is_empty() {
        exit.to_owned()
    } else {
        format!("{text} · {exit}")
    };
    let state = if cols(&format!("{} · {run}", w::status_word(busy))) <= width {
        " + 就绪"
    } else {
        "（放不下就绪）"
    };
    format!("{chosen} 条 + 退出{state}")
}

// ---------------------------------------------------------------------------
// Column arithmetic (one CJK character = 2 columns)
// ---------------------------------------------------------------------------

/// Display width of one character. East-Asian Wide/Fullwidth ranges only — an
/// approximation that decides the probe's own pre-wrapping; the alignment on screen is
/// ratatui's (the real implementation uses `unicode-width`, `tui.rs::cell_width`).
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

/// `s` cut to `width` columns (no ellipsis: the geometry already promised the room).
fn fit_cols(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = char_w(ch);
        if used + cw > width {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out
}

/// Pre-wrap `s` at `width` columns on character boundaries (the repo's `wrap_take` has
/// the same no-word-boundary 口径; see README's "近似" table).
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

fn pad_right(s: &str, width: usize) -> String {
    let mut out = s.to_owned();
    for _ in cols(s)..width {
        out.push(' ');
    }
    out
}

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
// Frozen numbers (spec §1–§2)
// ---------------------------------------------------------------------------

/// The smallest terminal the shell is drawn in (spec §2).
const MIN_W: u16 = 40;
const MIN_H: u16 = 10;

/// Fixed chrome (spec §1): frame 2 + the main column's three rules 3 + status row 1 +
/// hints 1. When the status row vanishes its row and its rule go back to the transcript,
/// which leaves [`CHROME_NO_STATUS`].
const CHROME: u16 = 7;
const CHROME_NO_STATUS: u16 = 5;

/// The mark's own width (`layout::LOGO_WIDTH`, and `mark_lines`' debug_assert).
const LOGO_WIDTH: u16 = 38;

/// The most input rows the input area ever holds (spec §2).
const INPUT_CAP: u16 = 10;

/// The rows a sidebar tier spends on its tab bar: a rule, the labels, a rule (§3).
const TAB_ROWS: u16 = 3;

/// The panel's label column: as wide as the widest label (spec §3).
const LABEL_W: usize = 6;

/// spec §2's tier table. 0 = the whole sidebar is hidden.
fn tier_for(w: u16) -> u16 {
    if w >= 120 {
        40
    } else if w >= 100 {
        34
    } else if w >= 80 {
        28
    } else {
        0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarKind {
    Mark,
    Text,
    Hidden,
}

impl SidebarKind {
    fn label(self) -> &'static str {
        match self {
            SidebarKind::Mark => "mark",
            SidebarKind::Text => "文字身份",
            SidebarKind::Hidden => "无身份行",
        }
    }
    fn rows(self) -> u16 {
        match self {
            SidebarKind::Mark => 5,
            SidebarKind::Text => 1,
            SidebarKind::Hidden => 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Usage,
    Trace,
    Files,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FocusColor {
    Yellow,
    BrightMagenta,
}

/// What the status row's ladder came out as (spec §2).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rung {
    /// `模型 … │ 模式 … │ 上下文 …%`
    Full,
    /// The model dropped.
    ModeAndShare,
    /// Only `上下文 …%`.
    ShareOnly,
    /// The row and its rule are gone (transcript +2 rows).
    Gone,
}

impl Rung {
    fn label(self) -> &'static str {
        match self {
            Rung::Full => "1 · 模型+模式+上下文",
            Rung::ModeAndShare => "2 · 模式+上下文（丢模型）",
            Rung::ShareOnly => "3 · 只剩上下文",
            Rung::Gone => "4 · 整行消失（转录 +2 行）",
        }
    }
}

/// The injected facts, hand-filled. The counts are made up, but their shape is the
/// panel's real 口径 (thousands separators, `used / limit`), so digit count and
/// alignment are visible.
#[derive(Clone)]
struct Facts {
    model: &'static str,
    mode: &'static str,
    busy: bool,
    context_used: Option<u64>,
    context_window: u64,
    tokens_used: u64,
    budget: Option<u64>,
    turns: u64,
    tokens_in: u64,
    tokens_out: u64,
    cache_hit: u64,
    cache_miss: u64,
    /// Rail units: turns in an interactive session, rounds in a discussion (spec §4).
    units: usize,
    /// The unit the viewport's top row belongs to, 1-based. 0 = empty session.
    focus: usize,
}

impl Default for Facts {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-5",
            mode: "ask",
            busy: false,
            context_used: Some(12_345),
            context_window: 200_000,
            tokens_used: 12_345,
            budget: Some(100_000),
            turns: 8,
            tokens_in: 9_000,
            tokens_out: 3_345,
            cache_hit: 9_000,
            cache_miss: 3_345,
            units: 8,
            focus: 8,
        }
    }
}

/// The transcript sample, as raw unwrapped lines of real wording: CJK, ASCII paths, and
/// one line longer than any frame is wide.
fn transcript_lines(f: &Facts) -> Vec<String> {
    vec![
        w::banner(
            "01J8ZQ4K7M",
            f.model,
            w::mode_label(f.mode),
            "~/code/fortystory/fs-agent",
        ),
        w::round_section(1, "independent"),
        format!(
            "{} 把外壳改成全高左栏 + 回合条，转录右缘留两列。",
            w::SPEAKER_USER
        ),
        format!(
            "{} 好的，先读 src/render/layout.rs，再算一遍降级阶梯。",
            w::SPEAKER_SYSTEM
        ),
        w::tool_call("read", "path=\"src/render/layout.rs\""),
        w::TOOL_COMPLETED.to_owned(),
        w::tool_output_preview(
            "pub fn plan(area: Rect, draft_rows: u16) -> Regions { let header_rows = header_content_rows(area.width, area.height); … }",
            40,
        ),
        "这一行故意写得很长，用来在窄档里折行：左栏吃掉列之后主列还剩多少列，就是转录真正能读到的宽度。"
            .to_owned(),
        format!(
            "{} 现在把四块边框拆成「一圈外框 + 内部分隔线」。",
            w::SPEAKER_SYSTEM
        ),
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
const DRAFT_THREE: &str = "把外壳改成全高左栏，转录右缘留滚动条与回合条两列。\n第二行：状态行在输入区上方，三段可以按宽度丢。\n第三行：草稿长高先吃转录行，输入区上限 10 行。";
const DRAFT_TWELVE: &str = "第一行：这一行只是用来把草稿撑过十行上限。\n第二行\n第三行\n第四行\n第五行\n第六行\n第七行\n第八行\n第九行\n第十行\n第十一行\n第十二行";

// ---------------------------------------------------------------------------
// The geometry (spec §1–§2)
// ---------------------------------------------------------------------------

struct Plan {
    w: u16,
    h: u16,
    /// The sidebar's content rectangle (the divider's column is not part of it).
    sidebar: Option<Rect>,
    sidebar_kind: SidebarKind,
    /// The divider column, `None` when the sidebar is hidden.
    divide: Option<u16>,
    /// The main column's content rectangle.
    main: Rect,
    /// The transcript's rows: text + scrollbar + rail.
    transcript: Rect,
    /// The transcript's text area (both right columns taken out).
    text: Rect,
    scrollbar: Rect,
    rail: Rect,
    status: Option<Rect>,
    input: Rect,
    hints: Rect,
    input_rows: u16,
    transcript_rows: u16,
    rung: Rung,
    /// The panel fields the sidebar's height ladder left.
    fields: Vec<(&'static str, String)>,
    tier: u16,
}

/// Lay out one frame. `case.tier` overrides spec §2's tier (the two width variants) and
/// `case.mark_offset` overrides where the mark sits inside the sidebar (the 40 vs 42
/// question); everything else follows the spec.
fn plan(f: &Facts, case: &Case) -> Option<Plan> {
    let (w, h) = (case.w, case.h);
    if w < MIN_W || h < MIN_H {
        return None;
    }
    let tier = case.tier.unwrap_or_else(|| tier_for(w));
    // The mark only when the tier is wide enough for the mark itself (§2's table: the
    // mid and narrow tiers carry the text identity).
    let mut kind = if tier == 0 {
        SidebarKind::Hidden
    } else if tier >= LOGO_WIDTH {
        SidebarKind::Mark
    } else {
        SidebarKind::Text
    };

    let main_w = w - 2 - if tier > 0 { tier + 1 } else { 0 };

    // The status row's ladder, decided on the main column's width (spec §2).
    let status_text = w::status_row(
        f.model,
        &w::mode_field(f.mode),
        &w::context_share(f.context_used, f.context_window),
        main_w as usize,
    );
    let rung = if status_text.is_empty() {
        Rung::Gone
    } else if status_text.contains(w::PANEL_MODEL) {
        Rung::Full
    } else if status_text.contains(w::PANEL_CONTEXT) {
        Rung::ModeAndShare
    } else {
        Rung::ShareOnly
    };
    let status_shown = rung != Rung::Gone;

    // Input rows: the draft's wrapped height, capped by what is left once the transcript
    // keeps its floor row (spec §2).
    let input_text_w = main_w.saturating_sub(2).max(1) as usize; // "> " is two columns
    let draft_rows = wrap_draft(case.draft, input_text_w);
    let chrome = if status_shown { CHROME } else { CHROME_NO_STATUS };
    let cap = INPUT_CAP.min(h.saturating_sub(chrome + 1)).max(1);
    let input_rows = draft_rows.max(1).min(cap);
    let transcript_rows = h - chrome - input_rows;

    // The sidebar's height ladder (spec §2): drop the mark first (to the text identity,
    // then to nothing), then fields from the tail (缓存 → 输出 → 输入). The floor is the
    // tab bar plus 上下文 / token / 回合.
    let content_h = h - 2;
    let mut field_count = 6usize;
    if tier > 0 {
        loop {
            if kind.rows() + TAB_ROWS + field_count as u16 <= content_h {
                break;
            }
            match kind {
                SidebarKind::Mark => kind = SidebarKind::Text,
                SidebarKind::Text => kind = SidebarKind::Hidden,
                SidebarKind::Hidden => {
                    if field_count > 3 {
                        field_count -= 1;
                    } else {
                        break;
                    }
                }
            }
        }
    }
    let fields = panel_fields(f, tier as usize)
        .into_iter()
        .take(field_count)
        .collect();

    let sidebar = (tier > 0).then(|| Rect::new(1, 1, tier, content_h));
    let divide = (tier > 0).then_some(1 + tier);
    let main_x = if tier > 0 { 2 + tier } else { 1 };
    let main = Rect::new(main_x, 1, main_w, content_h);

    let transcript = Rect::new(main_x, 1, main_w, transcript_rows);
    let text = Rect::new(main_x, 1, main_w.saturating_sub(2), transcript_rows);
    let scrollbar = Rect::new(main_x + main_w - 2, 1, 1, transcript_rows);
    let rail = Rect::new(main_x + main_w - 1, 1, 1, transcript_rows);

    let mut y = 1 + transcript_rows;
    y += 1; // the rule above the status row
    let status = status_shown.then(|| {
        let rect = Rect::new(main_x, y, main_w, 1);
        y += 1;
        rect
    });
    y += 1; // the rule above the input area
    let input = Rect::new(main_x, y, main_w, input_rows);
    y += input_rows;
    y += 1; // the rule above the hint row
    let hints = Rect::new(main_x, y, main_w, 1);

    Some(Plan {
        w,
        h,
        sidebar,
        sidebar_kind: kind,
        divide,
        main,
        transcript,
        text,
        scrollbar,
        rail,
        status,
        input,
        hints,
        input_rows,
        transcript_rows,
        rung,
        fields,
        tier,
    })
}

fn wrap_draft(draft: &str, width: usize) -> u16 {
    draft
        .split('\n')
        .map(|line| wrap_cols(line, width).len() as u16)
        .sum::<u16>()
        .max(1)
}

/// The panel's fields in the order spec §3 fixes: 上下文 / token / 回合 / 输入 / 输出 /
/// 缓存 (**no `模型` row** — it moved to the status row). Label column 6 columns
/// left-aligned, values right-aligned, thousands separators unchanged.
fn panel_fields(f: &Facts, content_w: usize) -> Vec<(&'static str, String)> {
    let value_w = content_w.saturating_sub(LABEL_W + 1);
    let context = match f.context_used {
        Some(used) => {
            let with_share = cols(&w::context_pair(Some(used), f.context_window, true)) <= value_w;
            w::context_pair(Some(used), f.context_window, with_share)
        }
        None => w::context_pair(None, f.context_window, false),
    };
    let cache = w::cache_pair(f.cache_hit, f.cache_miss);
    let mut rows: Vec<(&'static str, String)> = vec![
        (w::PANEL_CONTEXT, context),
        (w::PANEL_TOKENS, w::token_pair(f.tokens_used, f.budget)),
        (w::PANEL_TURNS, w::thousands(f.turns)),
        (w::PANEL_INPUT, w::thousands(f.tokens_in)),
        (w::PANEL_OUTPUT, w::thousands(f.tokens_out)),
    ];
    // v1's width-driven drop of the cache row survives as a code path (spec §3); at
    // tier 28 the value column is 21 columns wide, so it never triggers.
    if cols(&cache) <= value_w {
        rows.push((w::PANEL_CACHE, cache));
    }
    rows
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum RailGlyph {
    Normal,
    Focus,
    Truncated,
}

/// The rail's cells for a transcript `rows` rows tall: newest at the bottom, at most
/// `units` cells, the top one `⋮` when there are more units than rows (spec §4).
fn rail_cells(rows: u16, units: usize, focus: usize) -> Vec<(u16, RailGlyph, Option<usize>)> {
    let n = rows as usize;
    let mut out = Vec::new();
    if n == 0 || units == 0 {
        return out;
    }
    let glyph = |unit: usize| {
        if unit == focus {
            RailGlyph::Focus
        } else {
            RailGlyph::Normal
        }
    };
    if units <= n {
        let first_row = n - units;
        for (i, unit) in (1..=units).enumerate() {
            out.push(((first_row + i - 1) as u16, glyph(unit), Some(unit)));
        }
    } else {
        out.push((0, RailGlyph::Truncated, None));
        let first_unit = units - (n - 1) + 1;
        for k in 0..n - 1 {
            let unit = first_unit + k;
            out.push((1 + k as u16, glyph(unit), Some(unit)));
        }
    }
    out
}

fn put(frame: &mut ratatui::Frame, x: u16, y: u16, symbol: &str, style: Style) {
    if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
        cell.set_symbol(symbol);
        cell.set_style(style);
    }
}

/// The sidebar's tab bar rows: (top rule, labels).
fn tab_rows(sidebar: Rect, kind: SidebarKind) -> (u16, u16) {
    let top = sidebar.y + kind.rows();
    (top, top + 1)
}

struct Rendered {
    text: String,
    head: String,
}

fn render(f: &Facts, case: &Case) -> Rendered {
    let (w, h) = (case.w, case.h);
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).expect("TestBackend terminal");

    let dim = Style::default().fg(Color::DarkGray);

    let Some(p) = plan(f, case) else {
        terminal
            .draw(|frame| {
                let line = w::too_small(MIN_W, MIN_H);
                let area = frame.area();
                let x = (area.width as usize).saturating_sub(cols(&line)) / 2;
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        line,
                        Style::default().fg(Color::Red),
                    ))),
                    Rect::new(x as u16, area.height / 2, area.width, 1),
                );
            })
            .expect("draw");
        let text = dump(
            terminal.backend(),
            w,
            h,
            &["只有一句 too_small：外框、左栏、回合条都不画".to_owned()],
            &[case.question],
        );
        return Rendered {
            head: head_of(&text),
            text,
        };
    };

    let status_text = match p.status {
        Some(_) => w::status_row(
            f.model,
            &w::mode_field(f.mode),
            &w::context_share(f.context_used, f.context_window),
            p.main.width as usize,
        ),
        None => String::new(),
    };
    let hint_text = hint_line(
        w::status_word(f.busy),
        &w::KEY_HINTS,
        w::exit_hint(f.busy),
        p.hints.width as usize,
    );
    let hint_note = hint_summary(f.busy, p.hints.width as usize);

    // The transcript: pre-wrapped by the probe and bottom-anchored (no scroll state
    // machine — see README), with the scrollbar's and rail's columns kept clear.
    let text_w = p.text.width.max(1) as usize;
    let mut wrapped: Vec<String> = Vec::new();
    for raw in transcript_lines(f) {
        wrapped.extend(wrap_cols(&raw, text_w));
    }
    let rows = p.transcript_rows as usize;
    let total = wrapped.len();
    let shown: Vec<String> = if total > rows {
        wrapped[total - rows..].to_vec()
    } else {
        let mut v = vec![String::new(); rows - total];
        v.extend(wrapped);
        v
    };

    // The input area's rows, from the draft.
    let input_lines: Vec<String> = case
        .draft
        .split('\n')
        .flat_map(|line| wrap_cols(line, p.input.width.saturating_sub(2).max(1) as usize))
        .collect();
    let shown_input: Vec<String> = if input_lines.len() > p.input_rows as usize {
        input_lines[input_lines.len() - p.input_rows as usize..].to_vec()
    } else {
        input_lines
    };

    let cells = rail_cells(p.transcript_rows, f.units, f.focus);
    let focus_color = match case.focus_color {
        FocusColor::Yellow => Color::Yellow,
        FocusColor::BrightMagenta => Color::LightMagenta,
    };
    let sidebar_fields = p.fields.clone();
    let sidebar_kind = p.sidebar_kind;
    let tab = case.tab;
    // The mark is 38 columns wide. §2's wide tier (40) is "mark 38 + 1 column of air each
    // side", so the mark is centred in it; the variants move it.
    let mark_offset = case
        .mark_offset
        .unwrap_or_else(|| p.tier.saturating_sub(LOGO_WIDTH) / 2);

    let rule_status_y = 1 + p.transcript_rows;
    let rule_input_y = rule_status_y + if p.status.is_some() { 2 } else { 0 };
    let rule_hints_y = rule_input_y + 1 + p.input_rows;
    let (tab_rule_top, tab_labels_y) = p
        .sidebar
        .map(|s| tab_rows(s, p.sidebar_kind))
        .unwrap_or((0, 0));
    let p2 = &p;

    terminal
        .draw(|frame| {
            // --- the one frame (spec §1) --------------------------------------
            frame.render_widget(
                Block::default().borders(Borders::ALL).border_style(dim),
                frame.area(),
            );

            // --- the divider column and the glyph table (spec §1) ------------
            if let Some(divide) = p2.divide {
                put(frame, divide, 0, "┬", dim);
                put(frame, divide, p2.h - 1, "┴", dim);
                for y in 1..p2.h - 1 {
                    let sidebar_rule =
                        p2.sidebar.is_some() && (y == tab_rule_top || y == tab_rule_top + 2);
                    put(frame, divide, y, if sidebar_rule { "┤" } else { "│" }, dim);
                }
            }

            // --- the sidebar --------------------------------------------------
            if let Some(sidebar) = p2.sidebar {
                match sidebar_kind {
                    SidebarKind::Mark => {
                        let lines: Vec<Line<'static>> = w::logo_lines()
                            .iter()
                            .enumerate()
                            .map(|(row, text)| {
                                // `mark_lines()`' ramp: everything but the last row
                                // LightMagenta, the last Magenta; foreground only.
                                let color = if row < 4 {
                                    Color::LightMagenta
                                } else {
                                    Color::Magenta
                                };
                                Line::from(Span::styled(
                                    (*text).to_owned(),
                                    Style::default().fg(color),
                                ))
                            })
                            .collect();
                        frame.render_widget(
                            Paragraph::new(lines),
                            Rect::new(
                                sidebar.x + mark_offset,
                                sidebar.y,
                                sidebar.width.saturating_sub(mark_offset),
                                5,
                            ),
                        );
                    }
                    SidebarKind::Text => {
                        frame.render_widget(
                            Paragraph::new(Line::from(Span::styled(w::identity(), dim))),
                            Rect::new(sidebar.x, sidebar.y, sidebar.width, 1),
                        );
                    }
                    SidebarKind::Hidden => {}
                }

                // The tab bar: two rules (`├` at the frame, `┤` at the divider) with the
                // labels between them (§3).
                for y in [tab_rule_top, tab_rule_top + 2] {
                    put(frame, 0, y, "├", dim);
                    for x in sidebar.x..sidebar.x + sidebar.width {
                        put(frame, x, y, "─", dim);
                    }
                    put(frame, p2.divide.unwrap_or(sidebar.x), y, "┤", dim);
                }
                let mut spans: Vec<Span<'static>> = Vec::new();
                let mut used = 0u16;
                for (index, label) in [w::TAB_USAGE, w::TAB_TRACE, w::TAB_FILES]
                    .iter()
                    .enumerate()
                {
                    let selected = matches!(
                        (tab, index),
                        (Tab::Usage, 0) | (Tab::Trace, 1) | (Tab::Files, 2)
                    );
                    let style = if selected {
                        Style::default()
                            .fg(Color::LightMagenta)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        dim
                    };
                    spans.push(Span::styled((*label).to_owned(), style));
                    used += cols(label) as u16;
                    if index < 2 {
                        spans.push(Span::styled("│".to_owned(), dim));
                        used += 1;
                    }
                }
                spans.push(Span::styled(
                    "─".repeat(sidebar.width.saturating_sub(used) as usize),
                    dim,
                ));
                frame.render_widget(
                    Paragraph::new(Line::from(spans)),
                    Rect::new(sidebar.x, tab_labels_y, sidebar.width, 1),
                );

                // The page the tab selects.
                let page_y = tab_rule_top + TAB_ROWS;
                if page_y < sidebar.y + sidebar.height {
                    match tab {
                        Tab::Trace | Tab::Files => frame.render_widget(
                            Paragraph::new(Line::from(Span::styled(
                                fit_cols(w::TAB_PLACEHOLDER, sidebar.width as usize),
                                dim,
                            ))),
                            Rect::new(sidebar.x, page_y, sidebar.width, 1),
                        ),
                        Tab::Usage => {
                            let value_w = (sidebar.width as usize).saturating_sub(LABEL_W + 1);
                            for (i, (label, value)) in sidebar_fields.iter().enumerate() {
                                if page_y + i as u16 >= sidebar.y + sidebar.height {
                                    break;
                                }
                                frame.render_widget(
                                    Paragraph::new(Line::from(vec![
                                        Span::styled(pad_right(label, LABEL_W), dim),
                                        Span::raw(" "),
                                        Span::raw(pad_left(value, value_w)),
                                    ])),
                                    Rect::new(sidebar.x, page_y + i as u16, sidebar.width, 1),
                                );
                            }
                        }
                    }
                }
            }

            // --- the main column's rules ---------------------------------------
            for y in [rule_status_y, rule_input_y, rule_hints_y] {
                let left = p2.divide.unwrap_or(0);
                put(frame, left, y, "├", dim);
                for x in left + 1..p2.w - 1 {
                    put(frame, x, y, "─", dim);
                }
                put(frame, p2.w - 1, y, "┤", dim);
            }

            // --- the transcript -------------------------------------------------
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
                    Rect::new(p2.text.x, p2.text.y + i as u16, p2.text.width, 1),
                );
            }

            // The scrollbar: the column the geometry always reserves (spec §1), drawn
            // only when there is more than a pane's worth (unchanged from v1).
            let track = p2.scrollbar;
            if track.width > 0 && track.height > 0 && total > track.height as usize {
                let mut state = ScrollbarState::new(total)
                    .position(total.saturating_sub(rows))
                    .viewport_content_length(track.height as usize);
                frame.render_stateful_widget(
                    Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .track_style(Style::default().fg(Color::DarkGray))
                        .thumb_style(Style::default().fg(Color::DarkGray)),
                    track,
                    &mut state,
                );
            }

            // The turn rail (spec §4).
            for (offset, glyph, _unit) in &cells {
                let (symbol, style) = match glyph {
                    RailGlyph::Normal => (w::RAIL_CELL, dim),
                    RailGlyph::Truncated => (w::RAIL_TRUNCATED, dim),
                    RailGlyph::Focus => (
                        w::RAIL_FOCUS,
                        Style::default()
                            .fg(focus_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                };
                put(frame, p2.rail.x, p2.transcript.y + offset, symbol, style);
            }

            // --- the status row --------------------------------------------------
            if let Some(status) = p2.status {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(status_text.clone(), dim))),
                    status,
                );
            }

            // --- the input area and the hint row ---------------------------------
            for (i, line) in shown_input.iter().enumerate() {
                let (prompt, style) = if i == 0 {
                    ("> ", Style::default().add_modifier(Modifier::BOLD))
                } else {
                    ("  ", Style::default())
                };
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(prompt.to_owned(), style),
                        Span::raw(line.clone()),
                    ])),
                    Rect::new(p2.input.x, p2.input.y + i as u16, p2.input.width, 1),
                );
            }
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(hint_text.clone(), dim))),
                p2.hints,
            );
        })
        .expect("draw");

    let summary = vec![
        format!(
            "sidebar={} / {} / 字段 {} / 用了 {} | status=rung {} | transcript={} 行 × {} 文本列（主列 {} − 滚动条 1 − 回合条 1） | rail={} 格（{}） | hints={}",
            if p.tier > 0 {
                p.tier.to_string()
            } else {
                "hidden".to_owned()
            },
            p.sidebar_kind.label(),
            if p.tier == 0 {
                "—".to_owned()
            } else {
                p.fields.len().to_string()
            },
            if p.tier == 0 {
                "—".to_owned()
            } else {
                format!(
                    "{} 行（内容 {} 行）",
                    p.sidebar_kind.rows() + TAB_ROWS + p.fields.len() as u16,
                    p.h - 2
                )
            },
            p.rung.label(),
            p.transcript_rows,
            p.text.width,
            p.main.width,
            p.transcript_rows,
            if f.units == 0 {
                "空会话（条为空）".to_owned()
            } else {
                format!("单位 {} 焦点 {}", f.units, f.focus)
            },
            hint_note,
        ),
        format!(
            "tab={} | 输入区 {} 行 | 主列 x={} 宽 {} | 分隔线列 x={}",
            tab_name(tab),
            p.input_rows,
            p.main.x,
            p.main.width,
            p.divide
                .map(|d| d.to_string())
                .unwrap_or_else(|| "—".to_owned())
        ),
    ];
    let text = dump(terminal.backend(), w, h, &summary, &[case.question]);
    Rendered {
        head: head_of(&text),
        text,
    }
}

fn tab_name(tab: Tab) -> &'static str {
    match tab {
        Tab::Usage => w::TAB_USAGE,
        Tab::Trace => w::TAB_TRACE,
        Tab::Files => w::TAB_FILES,
    }
}

fn head_of(text: &str) -> String {
    text.lines().take(6).collect::<Vec<_>>().join("\n")
}

fn dump(backend: &TestBackend, w: u16, h: u16, summary: &[String], notes: &[&str]) -> String {
    let mut out = String::new();
    writeln!(out, "size: {w}x{h}").unwrap();
    for line in summary {
        writeln!(out, "{line}").unwrap();
    }
    for note in notes {
        writeln!(out, "note: {note}").unwrap();
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
        // TestBackend's Display appends this note for cells hidden by a wide symbol; drop
        // it, then the quotes it wraps each row in.
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
// The screen catalogue
// ---------------------------------------------------------------------------

struct Case {
    file: &'static str,
    w: u16,
    h: u16,
    draft: &'static str,
    tier: Option<u16>,
    mark_offset: Option<u16>,
    tab: Tab,
    focus_color: FocusColor,
    question: &'static str,
}

impl Case {
    fn new(file: &'static str, w: u16, h: u16, draft: &'static str, question: &'static str) -> Self {
        Self {
            file,
            w,
            h,
            draft,
            tier: None,
            mark_offset: None,
            tab: Tab::Usage,
            focus_color: FocusColor::BrightMagenta,
            question,
        }
    }
}

/// A case's rail state, when it is not the default (units 8, focus 8).
struct Rail {
    units: usize,
    focus: usize,
    turns: u64,
}

fn main() {
    let out_dir = std::env::args().nth(1).expect("usage: probe <prototype-dir>");
    let out = Path::new(&out_dir);
    let frames = out.join("frames");
    fs::create_dir_all(&frames).expect("create frames dir");

    let base = Facts::default();

    // (case, rail override) — the rail's units are its turns in an interactive session.
    let cases: Vec<(Case, Option<Rail>)> = vec![
        (
            Case::new(
                "40x10.txt",
                40,
                10,
                DRAFT_EMPTY,
                "地板尺寸：左栏（<80）整栏隐藏，主列吃掉全部 38 列；状态行退到第 2 档（丢模型）",
            ),
            None,
        ),
        (
            Case::new(
                "60x24.txt",
                60,
                24,
                DRAFT_EMPTY,
                "60 列仍 <80：左栏隐藏，转录拿到 56 列文本；状态行第 2 档",
            ),
            None,
        ),
        (
            Case::new(
                "80x14.txt",
                80,
                14,
                DRAFT_EMPTY,
                "左栏第一次出现，窄档 28（文字身份，没有 mark）；高度 14 恰好放得下身份 + tab + 6 字段",
            ),
            None,
        ),
        (
            Case::new(
                "80x24.txt",
                80,
                24,
                DRAFT_EMPTY,
                "窄档 28 + 24 行：左栏底部留空，转录 17 行",
            ),
            None,
        ),
        (
            Case::new(
                "100x24.txt",
                100,
                24,
                DRAFT_EMPTY,
                "中档 34：主列内容 63 列，状态行三段齐全（48 列刚好放得下）",
            ),
            None,
        ),
        (
            Case::new(
                "120x24.txt",
                120,
                24,
                DRAFT_EMPTY,
                "宽档 40：mark 居中（左右各 1 格 air，§2 的读法）；主列 77 列",
            ),
            None,
        ),
        (
            Case::new(
                "174x50.txt",
                174,
                50,
                DRAFT_EMPTY,
                "很宽很高：左栏仍是 40（档位只由宽度决定），转录 42 行、回合条 42 格（12 个单位不溢出）",
            ),
            Some(Rail {
                units: 12,
                focus: 12,
                turns: 12,
            }),
        ),
        (
            Case::new(
                "120x24-draft-3.txt",
                120,
                24,
                DRAFT_THREE,
                "草稿 3 行：输入区 3 行，转录从 16 行降到 14 行",
            ),
            None,
        ),
        (
            Case::new(
                "120x24-draft-12.txt",
                120,
                24,
                DRAFT_TWELVE,
                "草稿 12 行：输入区吃满 10 行上限，转录只剩 7 行",
            ),
            None,
        ),
        // --- the width variants --------------------------------------------
        (
            {
                let mut case = Case::new(
                    "120x24-variant-sidebar-42.txt",
                    120,
                    24,
                    DRAFT_EMPTY,
                    "变体：左栏 42（mark + 左右各 2 格 air），代价是转录像左少 2 列",
                );
                case.tier = Some(42);
                case
            },
            None,
        ),
        (
            {
                let mut case = Case::new(
                    "120x24-variant-sidebar-40-flush.txt",
                    120,
                    24,
                    DRAFT_EMPTY,
                    "变体：左栏 40、mark 顶左画（票面说的「40 无留白」）——38 列的 mark 贴在最左，剩下 2 列全留在右边",
                );
                case.tier = Some(40);
                case.mark_offset = Some(0);
                case
            },
            None,
        ),
        (
            {
                let mut case = Case::new(
                    "100x24-variant-sidebar-mid-28.txt",
                    100,
                    24,
                    DRAFT_EMPTY,
                    "变体：中档直接收到 28（面板自己需要的宽度），转录多拿 6 列",
                );
                case.tier = Some(28);
                case
            },
            None,
        ),
        (
            {
                let mut case = Case::new(
                    "120x24-variant-focus-yellow.txt",
                    120,
                    24,
                    DRAFT_EMPTY,
                    "变体：焦点格用黄（现有「可以动手」语义）而不是亮品红",
                );
                case.focus_color = FocusColor::Yellow;
                case
            },
            None,
        ),
        // --- the rail's two states -----------------------------------------
        (
            Case::new(
                "120x24-rail-3-units.txt",
                120,
                24,
                DRAFT_EMPTY,
                "回合条状态 ①：3 个单位，吸底、焦点在最新（第 3 个）",
            ),
            Some(Rail {
                units: 3,
                focus: 3,
                turns: 3,
            }),
        ),
        (
            Case::new(
                "120x24-rail-30-units.txt",
                120,
                24,
                DRAFT_EMPTY,
                "回合条状态 ②：30 个单位、转录 16 行 → 溢出，最上一格 ⋮；焦点在第 19 个（= 从最新往回数第 12 格）",
            ),
            Some(Rail {
                units: 30,
                focus: 19,
                turns: 30,
            }),
        ),
        (
            Case::new(
                "120x24-rail-30-units-focus-12-gap.txt",
                120,
                24,
                DRAFT_EMPTY,
                "回合条状态 ② 的反例：同样 30 个单位，但视口停在**第 12 个单位**（从最早那头数）——按 §4「只保留最近 N 格」这一格根本不在条上，于是整条没有一个格子是亮的",
            ),
            Some(Rail {
                units: 30,
                focus: 12,
                turns: 30,
            }),
        ),
        // --- the tabs and the busy hints -----------------------------------
        (
            {
                let mut case = Case::new(
                    "120x24-tab-trace.txt",
                    120,
                    24,
                    DRAFT_EMPTY,
                    "tab 切到 轨迹：面板读数换成一行占位符，状态行的 上下文 % 成为唯一读数（spec §3 接受的代价）",
                );
                case.tab = Tab::Trace;
                case
            },
            None,
        ),
        (
            {
                let mut case = Case::new(
                    "120x24-tab-files.txt",
                    120,
                    24,
                    DRAFT_EMPTY,
                    "tab 切到 文件：与 轨迹 同一张占位页，只是选中的标签不同",
                );
                case.tab = Tab::Files;
                case
            },
            None,
        ),
        (
            Case::new(
                "120x24-busy-hints.txt",
                120,
                24,
                DRAFT_EMPTY,
                "忙碌态（工作中）：退出提示变成 ctrl-c 退出，比空闲态短 7 列，提示行因此又放得下 就绪",
            ),
            None,
        ),
    ];

    let mut index = String::from(
        "# ticket 01 prototype snapshots —— 新几何（全高左栏 + 回合条 + 状态行）\n\n\
         每一张都由 `tui-sidebar-probe` 用 ratatui `TestBackend` 渲染进固定尺寸 buffer 后 dump，\
         **不是手绘**。命令见 `README.md`；每档的行列数见 `geometry-table.md`。\n\n\
         | 文件 | 尺寸 | 这张在回答什么 |\n| --- | --- | --- |\n",
    );

    for (case, rail) in &cases {
        let mut facts = base.clone();
        if let Some(rail) = rail {
            facts.units = rail.units;
            facts.focus = rail.focus;
            facts.turns = rail.turns;
        }
        if case.file == "120x24-busy-hints.txt" {
            facts.busy = true;
        }
        let rendered = render(&facts, case);
        fs::write(frames.join(case.file), &rendered.text).expect("write frame");
        writeln!(
            index,
            "| `frames/{}` | {}x{} | {} |",
            case.file, case.w, case.h, case.question
        )
        .unwrap();
        println!("wrote frames/{} ({}x{})", case.file, case.w, case.h);
        println!("    {}", rendered.head.replace('\n', "\n    "));
    }

    fs::write(out.join("geometry-table.md"), geometry_table(&base)).expect("write geometry table");
    fs::write(out.join("SNAPSHOTS.md"), index).expect("write snapshot index");
    println!("wrote geometry-table.md and SNAPSHOTS.md");
}

// ---------------------------------------------------------------------------
// The geometry table — every number below is printed by `plan()`, not asserted
// ---------------------------------------------------------------------------

fn geometry_row(f: &Facts, w: u16, h: u16, draft: &'static str, tier: Option<u16>) -> String {
    let case = Case {
        file: "",
        w,
        h,
        draft,
        tier,
        mark_offset: None,
        tab: Tab::Usage,
        focus_color: FocusColor::BrightMagenta,
        question: "",
    };
    match plan(f, &case) {
        None => "| 太小 | — | — | — | — | — | — |".to_owned(),
        Some(p) => format!(
            "| {} | {} | {} | {} | {} | {} | {} |",
            if p.tier > 0 {
                p.tier.to_string()
            } else {
                "隐藏".to_owned()
            },
            p.sidebar_kind.label(),
            p.rung.label(),
            p.transcript_rows,
            p.text.width,
            f.units.min(p.transcript_rows as usize),
            if p.tier == 0 {
                "—".to_owned()
            } else {
                p.fields.len().to_string()
            },
        ),
    }
}

fn off(out: &mut String, title: &str) {
    out.push_str(&format!("\n## {title}\n\n"));
}

fn geometry_table(f: &Facts) -> String {
    let mut out = String::from(
        "# ticket 01 实测几何表\n\n\
         全部数字由探针的 `plan()` 打印（画帧用的同一套代码），不是手抄。\n\
         `转录文本列数 = 主列内容宽 − 1（滚动条）− 1（回合条）`；\
         `回合条格数 = min(单位数, 转录行数)`（溢出时最上一格是 `⋮`，仍占一格）。\n\n\
         ## A. 规格里的尺寸矩阵（空草稿；回合条假数据：单位 8 / 焦点 8）\n\n\
         | 尺寸 | 左栏档位 | 左栏类型 | 状态行档位 | 转录行数 | 转录文本列数 | 回合条格数 | 面板字段数 |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for (w, h) in [
        (40u16, 10u16),
        (60, 24),
        (80, 14),
        (80, 24),
        (100, 24),
        (120, 24),
        (174, 50),
    ] {
        writeln!(out, "| {w}×{h} {}", geometry_row(f, w, h, DRAFT_EMPTY, None)).unwrap();
    }

    off(&mut out, "B. 两个草稿档（120×24）");
    writeln!(
        out,
        "| 档 | 输入区行 | 转录行数 | 转录文本列数 | 回合条格数 |"
    )
    .unwrap();
    writeln!(out, "| --- | --- | --- | --- | --- |").unwrap();
    for (draft, name) in [(DRAFT_THREE, "草稿 3 行"), (DRAFT_TWELVE, "草稿 12 行")] {
        let case = Case::new("", 120, 24, draft, "");
        let p = plan(f, &case).expect("120x24 plans");
        writeln!(
            out,
            "| {name} | {} | {} | {} | {} |",
            p.input_rows,
            p.transcript_rows,
            p.text.width,
            f.units.min(p.transcript_rows as usize),
        )
        .unwrap();
    }

    off(&mut out, "C. 两个争议宽度档（变体，画进了 frames/ 的那几张）");
    writeln!(
        out,
        "| 尺寸 | 左栏档位 | 左栏类型 | 状态行档位 | 转录行数 | 转录文本列数 | 回合条格数 | 面板字段数 |"
    )
    .unwrap();
    writeln!(out, "| --- | --- | --- | --- | --- | --- | --- | --- |").unwrap();
    for (w, tier) in [(120u16, 40u16), (120, 42), (100, 34), (100, 28)] {
        writeln!(out, "| {w}×24 {}", geometry_row(f, w, 24, DRAFT_EMPTY, Some(tier))).unwrap();
    }

    off(&mut out, "D. 宽度扫描（h=24，空草稿）—— 档位边界落在哪一列");
    writeln!(
        out,
        "| w | 左栏档位 | 左栏类型 | 主列内容宽 | 状态行档位 | 转录行数 | 转录文本列数 | 回合条格数 | 提示行 |"
    )
    .unwrap();
    writeln!(out, "| --- | --- | --- | --- | --- | --- | --- | --- | --- |").unwrap();
    for w in [
        40u16, 48, 60, 72, 79, 80, 88, 96, 99, 100, 104, 112, 119, 120, 128, 144, 174,
    ] {
        let case = Case::new("", w, 24, DRAFT_EMPTY, "");
        match plan(f, &case) {
            None => writeln!(out, "| {w} | 太小 | — | — | — | — | — | — | — |").unwrap(),
            Some(p) => writeln!(
                out,
                "| {w} | {} | {} | {} | {} | {} | {} | {} | {} |",
                if p.tier > 0 {
                    p.tier.to_string()
                } else {
                    "隐藏".to_owned()
                },
                p.sidebar_kind.label(),
                p.main.width,
                p.rung.label(),
                p.transcript_rows,
                p.text.width,
                f.units.min(p.transcript_rows as usize),
                hint_summary(f.busy, p.hints.width as usize),
            )
            .unwrap(),
        }
    }

    off(&mut out, "E. 高度扫描（空草稿）—— 转录行数怎么被输入区吃掉、左栏怎么变");
    writeln!(
        out,
        "| w | h | 输入区行 | 转录行数 | 左栏内容行 | 左栏用了 | 左栏类型 | 面板字段数 |"
    )
    .unwrap();
    writeln!(out, "| --- | --- | --- | --- | --- | --- | --- | --- |").unwrap();
    for w in [120u16, 100, 80] {
        for h in [10u16, 11, 12, 14, 16, 20, 24, 30, 50] {
            let case = Case::new("", w, h, DRAFT_EMPTY, "");
            match plan(f, &case) {
                None => writeln!(out, "| {w} | {h} | — | 太小 | — | — | — | — |").unwrap(),
                Some(p) => writeln!(
                    out,
                    "| {w} | {h} | {} | {} | {} | {} | {} | {} |",
                    p.input_rows,
                    p.transcript_rows,
                    h - 2,
                    if p.tier == 0 {
                        "—".to_owned()
                    } else {
                        (p.sidebar_kind.rows() + TAB_ROWS + p.fields.len() as u16).to_string()
                    },
                    p.sidebar_kind.label(),
                    if p.tier == 0 {
                        "—".to_owned()
                    } else {
                        p.fields.len().to_string()
                    },
                )
                .unwrap(),
            }
        }
    }

    off(&mut out, "F. 状态行的阶梯（直接调 `status_row()`）");
    writeln!(out, "| 主列内容宽 | 结果 | 档 |").unwrap();
    writeln!(out, "| --- | --- | --- |").unwrap();
    let share = w::context_share(f.context_used, f.context_window);
    let mode = w::mode_field(f.mode);
    for width in [77usize, 63, 45, 38, 30, 24, 11, 10] {
        let text = w::status_row(f.model, &mode, &share, width);
        let rung = if text.is_empty() {
            "4 · 整行消失".to_owned()
        } else if text.contains(w::PANEL_MODEL) {
            "1 · 三段齐全".to_owned()
        } else if text.contains(w::PANEL_CONTEXT) {
            "2 · 丢模型".to_owned()
        } else {
            "3 · 只剩上下文".to_owned()
        };
        writeln!(
            out,
            "| {width} | `{}` | {rung} |",
            if text.is_empty() {
                "（空 → 行消失）"
            } else {
                &text
            }
        )
        .unwrap();
    }
    writeln!(
        out,
        "\n**第 4 档（整行消失）到不了**：它要求主列内容宽 < 11；左栏隐藏时主列内容宽 = `w − 2`，\
         于是要 `w < 13` —— 早于 40×10 地板就被 `too_small` 拦下了。所以「状态行消失 → 转录 +2 行」\
         这条**只能靠 `status_row()` 的单元测试覆盖，几何上不可达**（与 v1 那句「左栏高度不足永远\
         触发不到」同一类）。"
    )
    .unwrap();

    off(&mut out, "G. 回合条的两种状态（120×24 → 转录 16 行）");
    for (units, focus) in [(3usize, 3usize), (30, 19), (30, 12), (0, 0)] {
        let cells = rail_cells(16, units, focus);
        let focus_drawn = cells.iter().any(|(_, g, _)| *g == RailGlyph::Focus);
        let truncated = cells.iter().any(|(_, g, _)| *g == RailGlyph::Truncated);
        writeln!(
            out,
            "- 单位 {units}、焦点 {focus} → 格数 {}、最上一格 `⋮`：{}、焦点格画得出来：{}",
            cells.len(),
            if truncated { "是" } else { "否" },
            if focus_drawn { "是" } else { "**否**" }
        )
        .unwrap();
    }
    writeln!(
        out,
        "\n最后一行是 spec §4 的洞：溢出时只保留「最近 N 格」，一旦焦点单位比 N 还旧，条上就没有\
         任何一格是亮的 —— 用户故事 12「一眼找到自己在哪」此时失效（帧 \
         `frames/120x24-rail-30-units-focus-12-gap.txt` 就是这个状态）。"
    )
    .unwrap();

    off(&mut out, "H. 提示行的宽度账（宽度取主列内容宽，spec §2）");
    writeln!(out, "| 尺寸 | 主列内容宽 | 提示行 |").unwrap();
    writeln!(out, "| --- | --- | --- |").unwrap();
    for (w, h) in [
        (40u16, 10u16),
        (60, 24),
        (80, 24),
        (100, 24),
        (120, 24),
        (174, 50),
    ] {
        let case = Case::new("", w, h, DRAFT_EMPTY, "");
        if let Some(p) = plan(f, &case) {
            writeln!(
                out,
                "| {w}×{h} | {} | {} |",
                p.hints.width,
                hint_summary(f.busy, p.hints.width as usize)
            )
            .unwrap();
        }
    }
    writeln!(
        out,
        "\n注意 120 列这一档实测是 **4 条 + 退出、放不下就绪**（主列 77 列）。忙碌态的退出提示 \
         `ctrl-c 退出` 比空闲态短 7 列，所以忙碌时反而放得下 `就绪` —— 与 spec §2 写的\
         「120 列 → 4 条 + 退出 + 就绪」差一个状态词，见 Answer。"
    )
    .unwrap();

    out
}
