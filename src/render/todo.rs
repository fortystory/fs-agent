//! The sidebar's `todo` page and the latch that decides whether it exists
//! (`.scratch/todo-and-modes/spec.md` §4).
//!
//! Two facts make this a panel of its own rather than part of [`super::panel`]:
//!
//! * **the list is derived from the stream, not from state beside it.** Every
//!   `todo` call the renderer sees carries its whole list in its arguments, so the
//!   page is whatever the most recent main-session call said. Nothing is stored
//!   that could disagree with the stream, and `--continue` rebuilds the same page
//!   by replaying the same calls.
//! * **the tab is a latch, not a condition.** Once a main session has submitted a
//!   non-empty list, the tab is there for the rest of the session — even when the
//!   list is later cleared. A tab that came and went with the list would move the
//!   page its reader is looking at, and the user chose "once seen, always there".
//!
//! An executor's list stays out of this: it is its own record (spec §2), visible in
//! the transcript and nowhere in the sidebar.

use ratatui::layout::Rect;
use ratatui::text::Line;

use crate::events::SpeakerId;
use crate::tools::todo::{read_items, Item, TODO_TOOL};

use super::transcript::Block;
use super::width::truncate_columns;
use super::wording;

/// What the `todo` tab shows, plus whether it is offered at all.
#[derive(Debug, Default)]
pub struct TodoPanel {
    /// Whether a main session has ever submitted a non-empty list. Set once and
    /// never cleared: it is the tab's whole appearance condition.
    seen: bool,
    /// The list in force, from the most recent main-session call. Empty is a real
    /// state here (the list was cleared) and does not clear `seen`.
    items: Vec<Item>,
}

impl TodoPanel {
    /// Watch one rendered block. Only a landed `todo` call from a speaker that is
    /// not an executor counts.
    pub fn observe(&mut self, block: &Block) {
        let Block::Tool(tool) = block else {
            return;
        };
        if tool.tool != TODO_TOOL || matches!(tool.speaker, SpeakerId::Executor(_)) {
            return;
        }
        let items = read_items(&tool.args);
        if !items.is_empty() {
            self.seen = true;
        }
        self.items = items;
    }

    /// Whether the tab bar carries the `todo` label.
    pub fn visible(&self) -> bool {
        self.seen
    }

    /// The page's rows, top to bottom, for a page area of this height.
    ///
    /// There is no scrolling: the height ladder gives the rows, the **count line**
    /// is reserved first (it is what the page is for), and what does not fit is
    /// announced in one row — `＋3 项` — above it. A page with room for one row
    /// shows the count alone rather than a stray item.
    pub fn lines(&self, area: Rect) -> Vec<Line<'static>> {
        let room = area.height as usize;
        if room == 0 {
            return Vec::new();
        }
        let count = Line::from(wording::todo_count(
            crate::tools::todo::completed(&self.items),
            self.items.len(),
        ));
        if room == 1 {
            return vec![count];
        }

        // The count owns the last row, so the items get what is left — one of those
        // rows going to the overflow line when they do not all fit.
        let item_rows = room - 1;
        let width = area.width as usize;
        let mut lines: Vec<Line<'static>> = Vec::with_capacity(room);
        if self.items.len() <= item_rows {
            for item in &self.items {
                lines.push(item_line(item, width));
            }
        } else {
            for item in self.items.iter().take(item_rows - 1) {
                lines.push(item_line(item, width));
            }
            lines.push(Line::from(truncate_columns(
                &wording::todo_overflow(self.items.len() - (item_rows - 1)),
                width,
            )));
        }
        lines.push(count);
        lines
    }
}

/// One item's row: its status glyph, then its content, clamped to the page.
fn item_line(item: &Item, width: usize) -> Line<'static> {
    Line::from(truncate_columns(
        &format!("{} {}", wording::todo_glyph(item.status), item.content),
        width,
    ))
}
