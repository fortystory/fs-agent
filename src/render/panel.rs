//! The information panel: what the session is and what it has cost (spec §8).
//!
//! The panel is a **pure function** of the injected facts and the counters this
//! module keeps off the stream. Nothing here is remembered between frames except
//! the counts themselves, so there is no second ledger to drift from
//! [`crate::events::total_usage`] — the sums below are the same sums that function
//! makes over a finished stream.
//!
//! Whether the panel exists at all is the geometry's call — it is hidden below 80
//! columns or four rows of middle — so what follows is the layout **inside** a panel
//! that is drawn. A row is the prototype's: a label column as wide as the widest
//! label, a space, then a value that numbers fill from the right and text from the
//! left. The drops are width- and height-driven, in the order ticket 05 fixed:
//! the percentage and the cache row go by width, the detail rows by height (they are
//! last, and the paragraph clips the tail) — and the four core fields are never
//! dropped, because a panel too small for them is hidden whole, which is the
//! geometry's rule rather than this module's.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::events::Usage;

use super::transcript::Block;
use super::tui::SessionFacts;
use super::width::{text_columns, truncate_columns};
use super::wording;

/// What the panel counts off the stream.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    /// Every `UsageRecorded`, folded by [`Usage::accumulate`] — the same fold
    /// `events::total_usage` makes over a finished stream, so the panel cannot drift
    /// from the session's ledger by re-deriving the arithmetic.
    total: Usage,
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
                self.total.accumulate(*usage);
                self.last_input = Some(usage.input_tokens);
            }
            Block::TurnEnded { .. } => self.turns = self.turns.saturating_add(1),
            _ => {}
        }
    }

    /// The rows to draw in `area`, most important first.
    pub fn lines(&self, facts: &SessionFacts, area: Rect) -> Vec<Line<'static>> {
        let value_columns = (area.width as usize).saturating_sub(label_columns() + 1);

        let context = match self.last_input {
            Some(used) => {
                // The percentage is the first thing the width takes away.
                let share = wording::context_pair(Some(used), facts.context_window, true);
                if text_columns(&share) <= value_columns {
                    share
                } else {
                    wording::context_pair(Some(used), facts.context_window, false)
                }
            }
            None => wording::context_pair(None, facts.context_window, false),
        };
        let tokens = wording::token_pair(self.total.total_tokens(), facts.budget_limit);
        let cache = wording::cache_pair(self.total.cached_tokens, self.total.miss_tokens);

        let mut rows: Vec<(&'static str, String, bool)> = vec![
            (wording::PANEL_MODEL, facts.model.clone(), false),
            (wording::PANEL_CONTEXT, context, true),
            (wording::PANEL_TOKENS, tokens, true),
            (wording::PANEL_TURNS, wording::thousands(self.turns), true),
        ];
        // Dropped in reverse order of importance, which is why they are appended
        // last: the height cut below takes them off the end. There is deliberately no
        // *width* floor for these rows — the panel is drawn only with at least 23
        // columns of content, so a value never has fewer than 16 columns, and a wider
        // floor would be unreachable. A value that still overruns is fitted above.
        rows.push((
            wording::PANEL_INPUT,
            wording::thousands(self.total.input_tokens),
            true,
        ));
        rows.push((
            wording::PANEL_OUTPUT,
            wording::thousands(self.total.output_tokens),
            true,
        ));
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

/// The label column: as wide as the widest label, so no label is ever cut and the
/// column cannot fall out of step with the words it holds.
fn label_columns() -> usize {
    [
        wording::PANEL_MODEL,
        wording::PANEL_CONTEXT,
        wording::PANEL_TOKENS,
        wording::PANEL_TURNS,
        wording::PANEL_INPUT,
        wording::PANEL_OUTPUT,
        wording::PANEL_CACHE,
    ]
    .iter()
    .map(|label| text_columns(label))
    .max()
    .unwrap_or(0)
}

/// One panel row: a dim label in the label column, then the value filling what is
/// left — numbers from the right, text from the left.
fn row(label: &str, value: &str, width: usize, right: bool) -> Line<'static> {
    let labels = label_columns();
    let label = pad_right(&fit(label, labels), labels);
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

/// `text` fitted to `width` columns.
///
/// Thousands separators go first — they are decoration, and a number that fits
/// without them is worth more than one that does not fit with them. What still does
/// not fit is cut with an ellipsis, so a short number is *visibly* short rather than
/// silently wrong.
fn fit(text: &str, width: usize) -> String {
    if text_columns(text) <= width {
        return text.to_owned();
    }
    let bare = text.replace(',', "");
    if text_columns(&bare) <= width {
        return bare;
    }
    let mut cut = truncate_columns(&bare, width.saturating_sub(1));
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
