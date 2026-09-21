//! The information panel: what the session is and what it has cost (spec §8).
//!
//! The panel is a **pure function** of the injected facts and the counters this
//! module keeps off the stream. Nothing here is remembered between frames except
//! the counts themselves, so there is no second ledger to drift from
//! [`crate::events::total_usage`] — the sums below are the same sums that function
//! makes over a finished stream.
//!
//! The layout of a row is the prototype's: a six-column label (`上下文` is the
//! widest), a space, then a value that numbers fill from the right and text from
//! the left. The drops are width- and height-driven, in the order ticket 05 fixed:
//! the percentage and the cache row go by width, the detail rows by height (they are
//! last, and the paragraph clips the tail) — and the four core fields are never
//! dropped, because a panel too small for them is hidden whole, which is the
//! geometry's rule rather than this module's.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::transcript::Block;
use super::tui::SessionFacts;
use super::width::{text_columns, truncate_columns};
use super::wording;

/// The label column: `上下文` is the widest label.
const LABEL_COLUMNS: usize = 6;

/// What the panel counts off the stream.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    input: u64,
    output: u64,
    cached: u64,
    miss: u64,
    /// The most recent call's input tokens: what the next request would carry.
    last_input: Option<u64>,
    turns: u64,
}

impl Panel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take what this block contributes, if anything.
    ///
    /// `cached` and `miss` are a **split** of `input`, and a vendor counts reasoning
    /// tokens inside `output` already, so neither may be added on top — the same
    /// accounting `Usage::total_tokens` and `events::total_usage` do (spec §8).
    pub fn observe(&mut self, block: &Block) {
        match block {
            Block::Usage { usage, .. } => {
                self.input = self.input.saturating_add(usage.input_tokens);
                self.output = self.output.saturating_add(usage.output_tokens);
                self.cached = self.cached.saturating_add(usage.cached_tokens);
                self.miss = self.miss.saturating_add(usage.miss_tokens);
                self.last_input = Some(usage.input_tokens);
            }
            Block::TurnEnded { .. } => self.turns = self.turns.saturating_add(1),
            _ => {}
        }
    }

    /// The rows to draw in `area`, most important first.
    pub fn lines(&self, facts: &SessionFacts, area: Rect) -> Vec<Line<'static>> {
        let value_columns = (area.width as usize).saturating_sub(LABEL_COLUMNS + 1);

        let context = match self.last_input {
            Some(used) => {
                let plain = wording::context_pair(Some(used), facts.context_window);
                let percent = wording::context_pair_percent(used, facts.context_window);
                // The percentage is the first thing the width takes away.
                if text_columns(&percent) <= value_columns {
                    percent
                } else {
                    plain
                }
            }
            None => wording::context_pair(None, facts.context_window),
        };
        let tokens =
            wording::token_pair(self.input.saturating_add(self.output), facts.budget_limit);
        let cache = wording::cache_pair(self.cached, self.miss);

        let mut rows: Vec<(&'static str, String, bool)> = vec![
            (wording::PANEL_MODEL, facts.model.clone(), false),
            (wording::PANEL_CONTEXT, context, true),
            (wording::PANEL_TOKENS, tokens, true),
            (wording::PANEL_TURNS, wording::thousands(self.turns), true),
        ];
        // Dropped in reverse order of importance, which is why they are appended
        // last: the height cut below takes them off the end. There is deliberately no
        // width floor for the detail rows themselves — the panel is drawn only when
        // it has at least 23 columns of content, so the only thing that can take them
        // away is the height, and a floor here would be unreachable.
        rows.push((wording::PANEL_INPUT, wording::thousands(self.input), true));
        rows.push((wording::PANEL_OUTPUT, wording::thousands(self.output), true));
        if text_columns(&cache) <= value_columns {
            rows.push((wording::PANEL_CACHE, cache, true));
        }
        // The height needs no cut here: the rows are in order of importance, and the
        // paragraph clips whatever does not fit the panel's content area, so the last
        // rows are the ones that go.
        rows.iter()
            .map(|(label, value, right)| row(label, value, value_columns, *right))
            .collect()
    }
}

/// One panel row: a dim label in the six-column field, then the value filling what
/// is left — numbers from the right, text from the left.
fn row(label: &str, value: &str, width: usize, right: bool) -> Line<'static> {
    let label = pad_right(&fit(label, LABEL_COLUMNS), LABEL_COLUMNS);
    let value = fit(value, width);
    let value = if right {
        pad_left(&value, width)
    } else {
        pad_right(&value, width)
    };
    Line::from(vec![
        Span::styled(label, Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::raw(value),
    ])
}

/// `text` cut to `width` columns, an ellipsis marking what was left out.
fn fit(text: &str, width: usize) -> String {
    if text_columns(text) <= width {
        return text.to_owned();
    }
    let mut cut = truncate_columns(text, width.saturating_sub(1));
    cut.push('…');
    cut
}

fn pad_right(text: &str, width: usize) -> String {
    let mut out = text.to_owned();
    for _ in text_columns(text)..width {
        out.push(' ');
    }
    out
}

fn pad_left(text: &str, width: usize) -> String {
    let used = text_columns(text);
    if used >= width {
        return text.to_owned();
    }
    let mut out = " ".repeat(width - used);
    out.push_str(text);
    out
}
