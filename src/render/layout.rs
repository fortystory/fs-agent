//! The fullscreen four-pane geometry (spec §2).
//!
//! Pure arithmetic: a terminal size and the input's row count go in, the
//! rectangles that exist at that size come out. Keeping it apart from the drawing
//! is what gives the degrade ladder exactly one home — the renderer has none — and
//! what lets the thresholds be read off in one sitting rather than reconstructed
//! from four call sites. The panel's fate is decided here, not by the caller.

use ratatui::layout::Rect;

use crate::render::editor::Placed;

/// The smallest terminal the four-pane layout is drawn in. One column or row
/// smaller and the only thing on screen is [`crate::render::wording::too_small`].
pub const MIN_WIDTH: u16 = 40;
pub const MIN_HEIGHT: u16 = 10;

/// The width from which the header has room for its second line.
const HEADER_TWO_LINE_WIDTH: u16 = 60;

/// The rows one block's border costs: one above the content, one below.
const BORDER_ROWS: u16 = 2;

/// The hint row, which rides inside the bottom block under the input.
const HINT_ROWS: u16 = 1;

/// The blank rows the layout keeps above and below the middle block.
const AIRY_ROWS: u16 = 2;

/// The fewest middle-content rows worth drawing. Airy is given up to keep this
/// rather than the other way round.
const MIN_MIDDLE_ROWS: u16 = 1;

/// Rows the block chrome costs whatever the terminal size: the three blocks'
/// borders and the hint row. The airy rows and the input rows come on top.
const CHROME: u16 = 3 * BORDER_ROWS + HINT_ROWS;

/// The mark's own width, shared with the painter so the two cannot drift apart.
pub const LOGO_WIDTH: u16 = 38;

/// The rows under the mark that carry the directory, the mode and the clock.
pub const LOGO_INFO_ROWS: u16 = 1;

/// The header's content rows when the mark is drawn: the mark itself, then the one
/// line of facts under it.
pub const LOGO_HEIGHT: u16 = 7;

/// The columns the header needs before it carries the mark: [`LOGO_WIDTH`], the
/// block's own two border columns, and one column of air on each side. Without the
/// air the mark abuts the border and pushes it off the line — 40 columns is exactly
/// the mark plus its borders, and it is too narrow.
pub const LOGO_MIN_WIDTH: u16 = LOGO_WIDTH + BORDER_ROWS + 2;

/// The height from which the tall header is worth its rows, read off the ladder
/// rather than guessed: the header's content and border, the airy rows under it, the
/// middle block's border and the fewest content rows worth drawing, and the bottom
/// block's input row, hint row and border. Below this the header keeps its text form.
pub const LOGO_MIN_HEIGHT: u16 = LOGO_HEIGHT
    + BORDER_ROWS
    + AIRY_ROWS
    + BORDER_ROWS
    + MIN_MIDDLE_ROWS
    + BORDER_ROWS
    + 1
    + HINT_ROWS
    + BORDER_ROWS;

/// The most input rows the bottom block will ever hold (spec §7).
const MAX_INPUT_ROWS: u16 = 10;

/// The width below which the information panel is never drawn: the transcript is
/// worth more than the numbers.
const PANEL_MIN_WIDTH: u16 = 80;

/// The fewest middle-content rows that can hold the panel's four core fields.
const PANEL_MIN_ROWS: u16 = 4;

/// One frame's regions, in terminal coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Regions {
    /// The header block, its borders included.
    pub header: Rect,
    /// The header's content rows.
    pub header_content: Rect,
    /// The bordered middle block: the conversation pane, and the panel when it is
    /// drawn beside it.
    pub middle: Rect,
    /// The conversation pane's content.
    pub transcript: Rect,
    /// The information panel's content, when it is drawn. The pane's left edge is
    /// the shared seam, one column before this.
    pub panel: Option<Rect>,
    /// The bottom block, its borders included.
    pub bottom: Rect,
    /// The input rows inside the bottom block.
    pub input: Rect,
    /// The hint row inside the bottom block, under the input.
    pub hints: Rect,
}

/// Whether the terminal is too small for anything but the notice sentence
/// ([`crate::render::wording::too_small`]).
pub fn below_minimum(area: Rect) -> bool {
    area.width < MIN_WIDTH || area.height < MIN_HEIGHT
}

/// Which of the three headers a frame draws.
///
/// The ladder is decided here, in [`header_content_rows`], so both callers ask this
/// instead of re-deriving it from the content height — a painter that compared the
/// height itself would be holding half the ladder, and the two halves could drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderKind {
    /// One line: identity, mode and clock, the directory given up.
    TextOneLine,
    /// Two lines: identity and clock, then directory and mode.
    TextTwoLines,
    /// The mark, with the directory and the mode-and-clock line under it.
    Mark,
}

impl Regions {
    /// Which header this frame's geometry came out as.
    pub fn header_kind(&self) -> HeaderKind {
        if self.header_content.height >= LOGO_HEIGHT {
            HeaderKind::Mark
        } else if self.header_content.height <= 1 {
            HeaderKind::TextOneLine
        } else {
            HeaderKind::TextTwoLines
        }
    }

    /// The column the conversation pane and the panel share, when the panel is
    /// drawn. The panel's own left edge is one column to the right of it.
    pub fn seam(&self) -> Option<u16> {
        self.panel.map(|panel| panel.x - 1)
    }

    /// The transcript's text area: its content less the scrollbar's column.
    ///
    /// The column is reserved whether or not the scrollbar is drawn, so text never
    /// rewraps because the transcript grew (spec §4).
    pub fn transcript_text(&self) -> Rect {
        Rect::new(
            self.transcript.x,
            self.transcript.y,
            self.transcript.width.saturating_sub(SCROLLBAR_COLUMN),
            self.transcript.height,
        )
    }

    /// The width of the question overlay inside this middle block.
    pub fn modal_width(&self) -> u16 {
        self.middle
            .width
            .saturating_sub(MODAL_MARGIN)
            .min(MODAL_MAX_WIDTH)
    }

    /// Where a question `rows` display rows tall goes: centred in the middle block, or
    /// nowhere when it cannot be drawn legibly there.
    pub fn modal(&self, rows: u16) -> Option<Rect> {
        let width = self.modal_width();
        let height = rows.saturating_add(BORDER_ROWS);
        if width <= BORDER_ROWS || height > self.middle.height {
            return None;
        }
        Some(Rect::new(
            self.middle.x + (self.middle.width - width) / 2,
            self.middle.y + (self.middle.height - height) / 2,
            width,
            height,
        ))
    }

    /// The scrollbar's column inside the transcript's content.
    pub fn scrollbar(&self) -> Rect {
        Rect::new(
            self.transcript.right().saturating_sub(SCROLLBAR_COLUMN),
            self.transcript.y,
            SCROLLBAR_COLUMN,
            self.transcript.height,
        )
    }

    /// How many content rows a floating menu anchored at `anchor` can take where it
    /// would open: everything above the cursor's row, or the room below it when there
    /// is nothing above.
    ///
    /// The caller trims the matches to this before asking for a rectangle, so a menu
    /// scrolls instead of being refused.
    pub fn menu_room(&self, anchor: Placed) -> u16 {
        let cursor_y = self.input.y.saturating_add(anchor.row);
        let above = cursor_y.saturating_sub(self.header.y);
        if above > BORDER_ROWS {
            return above - BORDER_ROWS;
        }
        self.bottom
            .bottom()
            .saturating_sub(cursor_y + 1)
            .saturating_sub(BORDER_ROWS)
    }

    /// Where the floating `/` menu goes: a bordered box `width` columns wide and
    /// `rows` content rows tall, anchored at the cursor so it follows what is being
    /// typed (spec §6).
    ///
    /// It opens **upwards** — the input is at the foot of the screen, so that is the
    /// side with room — and drops below the cursor only when there is nothing above.
    /// `None` when it does not fit either way, which is the honest answer on a
    /// terminal that small.
    pub fn menu(&self, anchor: Placed, width: u16, rows: u16) -> Option<Rect> {
        if rows == 0 || width < MENU_MIN_WIDTH {
            return None;
        }
        let height = rows.saturating_add(BORDER_ROWS);
        let cursor_y = self.input.y.saturating_add(anchor.row);
        let top = self.header.y;
        let y = if cursor_y.saturating_sub(top) >= height {
            cursor_y - height
        } else if self.bottom.bottom().saturating_sub(cursor_y + 1) >= height {
            cursor_y + 1
        } else {
            return None;
        };
        let left = self.header.x;
        let right = self.header.x.saturating_add(self.header.width);
        let width = width.min(right.saturating_sub(left));
        let x = self
            .input
            .x
            .saturating_add(anchor.column)
            .min(right.saturating_sub(width))
            .max(left);
        Some(Rect::new(x, y, width, height))
    }
}

/// The width one input row has for text: the bottom block's content, less the
/// prompt. Known before [`plan`] runs, because the draft's own height is what plan
/// needs.
pub fn input_text_width(area: Rect) -> u16 {
    area.width
        .saturating_sub(BORDER_ROWS + crate::render::editor::prompt_columns())
}

/// Lay out one frame. `draft_rows` is how many rows the input's draft wraps to.
///
/// The order here **is** the degrade ladder: the panel is hidden first (by the
/// renderer, which knows the transcript's width), then the header loses its second
/// line, then the airy rows go. The floor is [`MIN_WIDTH`] x [`MIN_HEIGHT`].
pub fn plan(area: Rect, draft_rows: u16) -> Regions {
    let header_rows = header_content_rows(area.width, area.height);
    // Airy is decided against the smallest draft there can be, so a draft that
    // grows gives up its own room rather than the whitespace: at 120x24 the input
    // may take all ten rows it is allowed, and the middle block still keeps some.
    let airy = fits_airy(area.height, header_rows, 1);
    let cap = max_input_rows(area.height, header_rows, airy);
    let input_rows = draft_rows.max(1).min(cap);
    let airy_rows = if airy { AIRY_ROWS } else { 0 };
    let middle_rows = area.height - CHROME - header_rows - input_rows - airy_rows;

    let header = Rect::new(area.x, area.y, area.width, header_rows + BORDER_ROWS);
    let middle = Rect::new(
        area.x,
        header.y + header.height + u16::from(airy),
        area.width,
        middle_rows + BORDER_ROWS,
    );
    let bottom = Rect::new(
        area.x,
        middle.y + middle.height + u16::from(airy),
        area.width,
        input_rows + HINT_ROWS + BORDER_ROWS,
    );

    // The panel is the first thing the degrade ladder gives up, and its width is a
    // quarter of the terminal with a floor and a ceiling (spec §2).
    let outer = if area.width >= PANEL_MIN_WIDTH && middle_rows >= PANEL_MIN_ROWS {
        panel_outer(area.width)
    } else {
        0
    };
    // The two panes share the seam column, so the panel's outer width counts it
    // once: the transcript keeps everything to the left of it (48 columns at the
    // narrowest panel-on size, 80x16).
    let transcript_width = if outer > 0 {
        area.width - outer - 1
    } else {
        area.width - 2
    };
    let panel = (outer > 0).then(|| {
        Rect::new(
            middle.x + area.width - outer + 1,
            middle.y + 1,
            outer - BORDER_ROWS,
            middle_rows,
        )
    });

    Regions {
        header,
        header_content: inside(header, header_rows),
        middle,
        transcript: Rect::new(middle.x + 1, middle.y + 1, transcript_width, middle_rows),
        panel,
        bottom,
        input: inside(bottom, input_rows),
        hints: Rect::new(
            bottom.x + 1,
            bottom.y + 1 + input_rows,
            bottom.width - BORDER_ROWS,
            HINT_ROWS,
        ),
    }
}

/// The column the transcript always keeps for its scrollbar, drawn or not.
const SCROLLBAR_COLUMN: u16 = 1;

/// The widest the question overlay ever gets. Wider than this and the eye has to
/// travel: a question is one sentence, not a page (spec §9).
const MODAL_MAX_WIDTH: u16 = 72;

/// The blank columns the overlay leaves on either side of the middle block.
const MODAL_MARGIN: u16 = 4;

/// The most rows the `/` menu shows before its matches scroll. A menu is a hint, not
/// a catalogue: past this the reader is scrolling a list to find a name they could
/// have typed (spec §6).
pub const MENU_MAX_ROWS: u16 = 8;

/// The widest the `/` menu ever gets: a name and its one-line description, without
/// the eye having to travel.
pub const MENU_MAX_WIDTH: u16 = 72;

/// The narrowest a menu box is worth drawing: below this the border is most of it.
const MENU_MIN_WIDTH: u16 = 12;

/// The panel's outer width, the shared seam column included.
fn panel_outer(width: u16) -> u16 {
    ((u32::from(width) * 26 / 100) as u16).clamp(25, 31)
}

/// The content rectangle of a bordered area.
pub fn inner(area: Rect) -> Rect {
    Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(BORDER_ROWS),
        area.height.saturating_sub(BORDER_ROWS),
    )
}

/// The content rectangle of a block: inside a one-cell border, `rows` tall.
fn inside(block: Rect, rows: u16) -> Rect {
    Rect::new(
        block.x + 1,
        block.y + 1,
        block.width.saturating_sub(BORDER_ROWS),
        rows,
    )
}

/// The header's content rows, and with them which header is drawn.
///
/// Three rungs, and the order between them is the point: the **mark** at
/// [`LOGO_MIN_WIDTH`] x [`LOGO_MIN_HEIGHT`] and above; the two-line text header
/// whenever the terminal is wide enough for the directory and the mode to sit
/// apart; and one line at the floor height, where a second line would leave the
/// middle block with nothing.
fn header_content_rows(width: u16, height: u16) -> u16 {
    if width >= LOGO_MIN_WIDTH && height >= LOGO_MIN_HEIGHT {
        LOGO_HEIGHT
    } else if width < HEADER_TWO_LINE_WIDTH || height <= MIN_HEIGHT {
        1
    } else {
        2
    }
}

/// Whether the airy rows survive: [`MIN_MIDDLE_ROWS`] has to be left for them to
/// be worth having.
fn fits_airy(height: u16, header_rows: u16, input_rows: u16) -> bool {
    height >= CHROME + header_rows + input_rows + AIRY_ROWS + MIN_MIDDLE_ROWS
}

/// How many rows the input may take at this size.
fn max_input_rows(height: u16, header_rows: u16, airy: bool) -> u16 {
    let airy_rows = if airy { AIRY_ROWS } else { 0 };
    let room = height.saturating_sub(CHROME + header_rows + airy_rows + MIN_MIDDLE_ROWS);
    MAX_INPUT_ROWS.min(room).max(1)
}
