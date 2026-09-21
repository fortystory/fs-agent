//! The fullscreen four-pane geometry (spec §2).
//!
//! Pure arithmetic: a terminal size and the two variable row counts go in, the
//! rectangles that exist at that size come out. Keeping it apart from the drawing
//! is what gives the degrade ladder exactly one home — the renderer has none —
//! and what lets the thresholds be read off in one sitting rather than
//! reconstructed from four call sites.

use ratatui::layout::Rect;

/// The smallest terminal the four-pane layout is drawn in. One column or row
/// smaller and the only thing on screen is [`crate::render::wording::too_small`].
pub const MIN_WIDTH: u16 = 40;
pub const MIN_HEIGHT: u16 = 10;

/// The width from which the header has room for its second line.
const HEADER_TWO_LINE_WIDTH: u16 = 60;

/// Rows the block chrome costs whatever the terminal size: the header's borders,
/// the middle block's borders, the bottom block's borders, and the hint row. The
/// airy rows and the input rows come on top of this.
const CHROME: u16 = 7;

/// The most input rows the bottom block will ever hold (spec §7).
const MAX_INPUT_ROWS: u16 = 10;

/// The width below which the information panel is never drawn: the transcript is
/// worth more than the numbers.
const PANEL_MIN_WIDTH: u16 = 80;

/// The fewest middle-content rows that can hold the panel's four core fields.
const PANEL_MIN_ROWS: u16 = 4;

/// The narrowest transcript worth drawing beside the panel.
const TRANSCRIPT_MIN_WIDTH: u16 = 20;

/// One frame's regions, in terminal coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panes {
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

/// Whether the terminal is too small for anything but the notice.
pub fn too_small(area: Rect) -> bool {
    area.width < MIN_WIDTH || area.height < MIN_HEIGHT
}

/// Lay out one frame. `draft_rows` is how many rows the input's draft wraps to.
///
/// The order here **is** the degrade ladder: the panel is hidden first (by the
/// renderer, which knows the transcript's width), then the header loses its second
/// line, then the airy rows go. The floor is [`MIN_WIDTH`] x [`MIN_HEIGHT`].
pub fn plan(area: Rect, draft_rows: u16) -> Panes {
    let header_rows = header_content_rows(area.width, area.height);
    // Airy is decided against the smallest draft there can be, so a draft that
    // grows gives up its own room rather than the whitespace: at 120x24 the input
    // may take all ten rows it is allowed, and the middle block still keeps some.
    let airy = fits_airy(area.height, header_rows, 1);
    let cap = max_input_rows(area.height, header_rows, airy);
    let input_rows = draft_rows.max(1).min(cap);
    let airy_rows = if airy { 2 } else { 0 };
    let middle_rows = area.height - CHROME - header_rows - input_rows - airy_rows;

    let header = Rect::new(area.x, area.y, area.width, header_rows + 2);
    let middle = Rect::new(
        area.x,
        header.y + header.height + u16::from(airy),
        area.width,
        middle_rows + 2,
    );
    let bottom = Rect::new(
        area.x,
        middle.y + middle.height + u16::from(airy),
        area.width,
        input_rows + 3,
    );

    // The panel is the first thing the degrade ladder gives up, and its width is a
    // quarter of the terminal with a floor and a ceiling (spec §2).
    let mut outer = if area.width >= PANEL_MIN_WIDTH && middle_rows >= PANEL_MIN_ROWS {
        panel_outer(area.width)
    } else {
        0
    };
    // The two panes share the seam column, so the panel's outer width counts it
    // once: the transcript keeps everything to the left of it.
    if outer > 0 && area.width - outer - 1 < TRANSCRIPT_MIN_WIDTH {
        outer = 0;
    }
    let transcript_width = if outer > 0 {
        area.width - outer - 1
    } else {
        area.width - 2
    };
    let panel = (outer > 0).then(|| {
        Rect::new(
            middle.x + area.width - outer + 1,
            middle.y + 1,
            outer - 2,
            middle_rows,
        )
    });

    Panes {
        header,
        header_content: inside(header, header_rows),
        middle,
        transcript: Rect::new(middle.x + 1, middle.y + 1, transcript_width, middle_rows),
        panel,
        bottom,
        input: inside(bottom, input_rows),
        hints: Rect::new(bottom.x + 1, bottom.y + 1 + input_rows, bottom.width - 2, 1),
    }
}

/// The panel's outer width, the shared seam column included.
fn panel_outer(width: u16) -> u16 {
    ((u32::from(width) * 26 / 100) as u16).clamp(25, 31)
}

/// The content rectangle of a block: inside a one-cell border.
fn inside(block: Rect, rows: u16) -> Rect {
    Rect::new(
        block.x + 1,
        block.y + 1,
        block.width.saturating_sub(2),
        rows,
    )
}

/// The header is two lines whenever the terminal is wide enough for the directory
/// and the mode to sit apart, and one line at the floor height, where a second
/// line would leave the middle block with nothing.
fn header_content_rows(width: u16, height: u16) -> u16 {
    if width < HEADER_TWO_LINE_WIDTH || height <= MIN_HEIGHT {
        1
    } else {
        2
    }
}

/// Whether the airy rows survive: one middle row has to be left for them to be
/// worth having.
fn fits_airy(height: u16, header_rows: u16, input_rows: u16) -> bool {
    height >= CHROME + header_rows + input_rows + 3
}

/// How many rows the input may take at this size.
fn max_input_rows(height: u16, header_rows: u16, airy: bool) -> u16 {
    let airy_rows = if airy { 2 } else { 0 };
    let room = height.saturating_sub(CHROME + header_rows + airy_rows + 1);
    MAX_INPUT_ROWS.min(room).max(1)
}
