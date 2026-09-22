//! The conversation pane's scroll buffer (spec §3, §4).
//!
//! The pane owns the transcript rather than the terminal's scrollback (ADR 0002),
//! so it has to answer three questions the terminal used to answer for us: how
//! much history to keep, how it wraps at the current width, and where the viewport
//! sits. The cap answers the first, the wrap cache the second, and the sticky
//! bottom the third.
//!
//! **Two units, deliberately.** The cap counts *source* lines — what a block
//! renders to, before wrapping — because a cap counted in display rows would keep
//! different amounts of history at different terminal widths. The viewport, the
//! scrollbar and the "new content" indicator count *display* rows, which is what a
//! person actually sees.

use std::collections::VecDeque;

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::width::char_columns;

/// Source lines the pane keeps before dropping the oldest (spec §3).
pub const CAP: usize = 20_000;

/// Rows a PgUp/PgDn step keeps from the page it is leaving, so the reader does not
/// lose the thread (spec §4).
const PAGE_OVERLAP: usize = 2;

/// Rows one wheel notch scrolls (spec §4).
const WHEEL_ROWS: usize = 3;

/// The conversation pane's buffer and viewport.
pub struct Pane {
    /// Source lines, oldest first. Never longer than [`CAP`].
    lines: VecDeque<Line<'static>>,
    /// The display row each source line starts on. Parallel to `lines`.
    starts: VecDeque<usize>,
    /// The wrapped display rows of `lines`, at `width`.
    wrapped: VecDeque<Line<'static>>,
    /// How many source lines `wrapped` already accounts for.
    wrapped_sources: usize,
    /// The width everything above was wrapped at; zero until the first frame.
    width: u16,
    /// The last frame's pane height, so a key or a wheel notch knows its step.
    height: u16,
    /// Display rows in the last frame: source rows and the live tail together.
    total: usize,
    /// The display row at the top of the viewport.
    top: usize,
    /// The source line the viewport top sits in — what a rewrap keeps its eye on.
    top_source: usize,
    /// Whether the viewport tracks the bottom.
    follow: bool,
    /// `total` as of the last frame that followed the bottom. The indicator counts
    /// what has arrived since.
    seen: usize,
}

impl Pane {
    pub fn new() -> Self {
        Self {
            lines: VecDeque::new(),
            starts: VecDeque::new(),
            wrapped: VecDeque::new(),
            wrapped_sources: 0,
            width: 0,
            height: 0,
            total: 0,
            top: 0,
            top_source: 0,
            follow: true,
            seen: 0,
        }
    }

    /// Append one source line; the oldest is dropped once [`CAP`] is reached.
    pub fn push(&mut self, line: Line<'static>) {
        self.lines.push_back(line);
        self.wrap_pending();
        self.evict();
    }

    /// Replace the newest source line. Used by the transcript's one mutable line:
    /// the thinking hint is written as it starts and rewritten in place when the
    /// trace is finished, so a reader never sees two lines for one thought (票 02 §1).
    ///
    /// A no-op on an empty pane, which is the honest answer: there is nothing to
    /// rewrite.
    pub fn replace_last(&mut self, line: Line<'static>) {
        let Some(last) = self.lines.back_mut() else {
            return;
        };
        *last = line;
        // The rewritten line still has to reach the wrap cache. Dropping the cached
        // rows for it is enough; the next `view` re-wraps it at the frame's width.
        if self.wrapped_sources == self.lines.len() {
            self.wrapped_sources -= 1;
            self.starts.pop_back();
            self.wrapped.clear();
        }
    }

    /// The source line a display row belongs to, if any.
    ///
    /// This is how a click turns a screen row into a block: the pane counts display
    /// rows, and everything a click can open is addressed by source line (票 04 §1).
    pub fn source_at(&self, display_row: usize) -> Option<usize> {
        if display_row >= self.total {
            return None;
        }
        match self.starts.binary_search(&display_row) {
            Ok(exact) => (exact < self.lines.len()).then_some(exact),
            Err(insert) => {
                let source = insert.checked_sub(1)?;
                (source < self.lines.len()).then_some(source)
            }
        }
    }

    /// The rows to draw: `height` display rows from the viewport top, with the
    /// streaming `live` text wrapped and appended after the source lines.
    ///
    /// This is also where the wrap cache is brought up to date, which is why it
    /// takes `&mut self`: the width that matters is the one the frame is actually
    /// drawn at, and that is only known here.
    pub fn view(&mut self, width: u16, height: u16, live: &str) -> Vec<Line<'static>> {
        self.ensure(width);
        let live_rows = wrap_text(live, width.max(1) as usize);
        self.height = height;
        self.total = self.wrapped.len() + live_rows.len();

        let height = height as usize;
        let max_top = self.total.saturating_sub(height);
        if self.follow {
            self.top = max_top;
        } else if self.top > max_top {
            // The transcript shrank under the viewport: the cap dropped rows, or
            // the pane grew. Landing at the bottom is the only honest place left.
            self.top = max_top;
        }
        if self.top >= max_top {
            self.follow = true;
            self.seen = self.total;
        }
        self.sync_top_source();
        self.window(height, &live_rows)
    }

    /// Scroll by `rows` display rows; negative is up.
    pub fn scroll(&mut self, rows: isize) {
        let max_top = self.total.saturating_sub(self.height as usize);
        if rows < 0 {
            // Leaving the bottom is what starts counting what arrives next; `seen`
            // stays where the last frame left it, so the indicator measures what
            // has arrived *since*, not what is merely below.
            self.follow = false;
        }
        self.top = (self.top as isize + rows).clamp(0, max_top as isize) as usize;
        if self.top >= max_top {
            self.follow = true;
            self.seen = self.total;
        }
        self.sync_top_source();
    }

    /// One page, keeping [`PAGE_OVERLAP`] rows of the page being left.
    pub fn page(&mut self, up: bool) {
        let step = (self.height as usize).saturating_sub(PAGE_OVERLAP).max(1);
        self.scroll(if up { -(step as isize) } else { step as isize });
    }

    /// One wheel notch.
    pub fn wheel(&mut self, up: bool) {
        self.scroll(if up {
            -(WHEEL_ROWS as isize)
        } else {
            WHEEL_ROWS as isize
        });
    }

    /// Follow the bottom again; the next frame puts the viewport there.
    pub fn to_bottom(&mut self) {
        self.follow = true;
        self.top = self.total.saturating_sub(self.height as usize);
        self.seen = self.total;
        self.sync_top_source();
    }

    /// Whether the viewport is tracking the bottom.
    pub fn following(&self) -> bool {
        self.follow
    }

    /// Display rows that arrived since the viewport last left the bottom.
    pub fn fresh(&self) -> usize {
        if self.follow {
            0
        } else {
            self.total.saturating_sub(self.seen)
        }
    }

    /// Display rows in the last frame.
    pub fn total(&self) -> usize {
        self.total
    }

    /// The display row at the top of the viewport.
    pub fn top(&self) -> usize {
        self.top
    }

    /// Bring the wrap cache up to date for `width`, keeping the viewport on the
    /// source line it was showing when the width changes.
    fn ensure(&mut self, width: u16) {
        if width == self.width {
            self.wrap_pending();
            return;
        }
        self.width = width;
        self.wrapped.clear();
        self.starts.clear();
        self.wrapped_sources = 0;
        self.wrap_pending();
        if !self.follow {
            // Every display row moved, so the row number means something else now;
            // the source line is what survives a rewrap (spec §4).
            self.top = self.starts.get(self.top_source).copied().unwrap_or(0);
        }
    }

    /// Wrap the source lines that arrived since the last pass.
    fn wrap_pending(&mut self) {
        if self.width == 0 {
            // No frame has been drawn yet, so there is no width to wrap to. The
            // first `view` does all of it.
            return;
        }
        let width = self.width.max(1) as usize;
        while self.wrapped_sources < self.lines.len() {
            let line = self.lines[self.wrapped_sources].clone();
            self.starts.push_back(self.wrapped.len());
            for row in wrap_line(&line, width) {
                self.wrapped.push_back(row);
            }
            self.wrapped_sources += 1;
        }
    }

    /// Drop the oldest source lines until the cap holds again.
    fn evict(&mut self) {
        while self.lines.len() > CAP {
            if self.wrapped_sources == 0 {
                // Nothing has been wrapped yet, so the line costs nothing else.
                self.lines.pop_front();
                continue;
            }
            let height = match self.starts.get(1) {
                Some(next) => next.saturating_sub(self.starts[0]),
                None => self.wrapped.len().saturating_sub(self.starts[0]),
            };
            self.lines.pop_front();
            self.starts.pop_front();
            for _ in 0..height {
                self.wrapped.pop_front();
            }
            self.wrapped_sources -= 1;
            // Every remaining start moves up by the rows that just left, and so does
            // the viewport. This runs once per evicted line — at most once per
            // completed block, once the transcript is at its cap.
            for start in self.starts.iter_mut() {
                *start = start.saturating_sub(height);
            }
            self.top = self.top.saturating_sub(height);
            self.total = self.total.saturating_sub(height);
            self.seen = self.seen.saturating_sub(height);
            self.top_source = self.top_source.saturating_sub(1);
        }
    }

    /// Note which source line the viewport top is inside, for the next rewrap.
    fn sync_top_source(&mut self) {
        self.top_source = match self.starts.binary_search(&self.top) {
            Ok(exact) => exact,
            Err(insert) => insert.saturating_sub(1),
        };
    }

    /// `height` display rows from `top`, taking source rows first and then the live
    /// tail.
    fn window(&self, height: usize, live: &[Line<'static>]) -> Vec<Line<'static>> {
        let mut rows = Vec::new();
        for index in self.top..self.total {
            if rows.len() == height {
                break;
            }
            let line = if index < self.wrapped.len() {
                self.wrapped.get(index)
            } else {
                live.get(index - self.wrapped.len())
            };
            match line {
                Some(line) => rows.push(line.clone()),
                None => break,
            }
        }
        rows
    }
}

impl Default for Pane {
    fn default() -> Self {
        Self::new()
    }
}

/// The display rows of `text` at `width` columns: one per logical line, each
/// wrapped on display columns.
pub fn wrap_text(text: &str, width: usize) -> Vec<Line<'static>> {
    if text.is_empty() {
        // No tail at all is no rows at all. Splitting would give one empty row, and
        // a blank row at the bottom of every frame is a row the pane does not have.
        return Vec::new();
    }
    text.split('\n')
        .flat_map(|raw| wrap_line(&Line::from(raw.to_owned()), width))
        .collect()
}

/// Wrap one styled line to `width` columns, keeping each piece's styling.
///
/// **Columns, not bytes.** A CJK character is three bytes and two columns, so
/// counting bytes wrapped a Chinese line at roughly a third of the pane's width —
/// the one place where Chinese looked broken even though every cell was right.
///
/// Per character rather than per grapheme: the offset has to be recoverable, and a
/// cluster's cells are not ours to split. A wide character alone in a one-column
/// pane still overflows it; wrapping cannot do better, it only has to keep moving.
///
/// A continuation row starts at column zero — the pane is a log, and an indent
/// would claim a structure the wrapped text does not have.
fn wrap_line(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for span in &line.spans {
        for ch in span.content.chars() {
            let columns = char_columns(ch);
            if used + columns > width && used > 0 {
                out.push(finish(line, std::mem::take(&mut spans)));
                used = 0;
            }
            push_char(&mut spans, ch, span.style);
            used += columns;
        }
    }
    if !spans.is_empty() || out.is_empty() {
        out.push(finish(line, spans));
    }
    out
}

/// One wrapped row, inheriting the line's own style and alignment.
fn finish(template: &Line<'static>, spans: Vec<Span<'static>>) -> Line<'static> {
    Line {
        spans,
        style: template.style,
        alignment: template.alignment,
    }
}

/// Append one character, extending the last span when the style matches so a
/// wrapped row stays one span per styled run rather than one per character.
fn push_char(spans: &mut Vec<Span<'static>>, ch: char, style: Style) {
    match spans.last_mut() {
        Some(last) if last.style == style => last.content.to_mut().push(ch),
        _ => spans.push(Span::styled(ch.to_string(), style)),
    }
}
