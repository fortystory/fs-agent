//! The fullscreen shell's geometry (`.scratch/tui-sidebar/spec.md` §1–§2).
//!
//! Pure arithmetic: a terminal size and the input's row count go in, the
//! rectangles that exist at that size come out. Keeping it apart from the drawing
//! is what gives the degrade ladder exactly one home — the renderer has none — and
//! what lets the thresholds be read off in one sitting rather than reconstructed
//! from four call sites.
//!
//! The shell is **one frame, one full-height sidebar, one main column**: the frame
//! is the terminal's border, the sidebar holds the mark, the tab bar and the
//! session's readings, and the main column stacks the transcript, the status row,
//! the input and the hints. The sidebar's fate is decided here, not by the caller.

use ratatui::layout::Rect;

use crate::render::editor::Placed;

/// The smallest terminal the shell is drawn in. One column or row smaller and the
/// only thing on screen is [`crate::render::wording::too_small`].
pub const MIN_WIDTH: u16 = 40;
pub const MIN_HEIGHT: u16 = 10;

/// The rows the shell spends on chrome that is neither transcript nor input: the
/// frame's two rows, the main column's three rules, the status row and the hint
/// row. `转录行 = h − CHROME − 输入行数`, which is the whole of the vertical
/// arithmetic (spec §1).
const CHROME: u16 = 7;

/// The frame's two border columns.
const BORDER_COLUMNS: u16 = 2;

/// The rows the tab bar costs: a rule, the labels, a rule (spec §3).
const TAB_ROWS: u16 = 3;

/// The columns the transcript always keeps at its right edge: the scrollbar's
/// column and the rail's. Reserved whether or not they are drawn, so text never
/// rewraps because the transcript grew (spec §1).
const TRAILING_COLUMNS: u16 = 2;

/// The sidebar's two content widths (spec §2). There is deliberately no rung
/// between them: the prototype measured the middle one and the `（6%）` it buys
/// back is already in the status row.
const SIDEBAR_WIDE: u16 = 40;
const SIDEBAR_NARROW: u16 = 28;

/// The widths at which each sidebar rung starts.
const SIDEBAR_WIDE_FROM: u16 = 120;

/// Below this the sidebar is hidden whole and the main column takes everything.
const SIDEBAR_NARROW_FROM: u16 = 80;

/// The mark's own width, shared with the painter so the two cannot drift apart.
pub const LOGO_WIDTH: u16 = 38;

/// The rows the mark draws, and the one row the text identity takes.
const LOGO_ROWS: u16 = 5;
const IDENTITY_ROWS: u16 = 1;

/// The fields the sidebar's usage page holds when nothing is squeezed, and the
/// fewest it ever keeps: 上下文 / token / 回合 (spec §2).
const SIDEBAR_FIELDS: u16 = 6;
const SIDEBAR_MIN_FIELDS: u16 = 3;

/// The most input rows the input area will ever hold (spec §2).
const MAX_INPUT_ROWS: u16 = 10;

/// Which of the three sidebar identities a frame draws.
///
/// The ladder is decided here, in [`sidebar_content`], so the painter asks this
/// instead of re-deriving it from the sidebar's height — a painter that compared
/// heights itself would be holding half the ladder, and the two halves could
/// drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarKind {
    /// The mark, on the wide rung (spec §2).
    Mark,
    /// One line of text identity, which is what the narrow rung has room for.
    Text,
    /// Neither: the height ladder gave the identity up so the readings could stay.
    Hidden,
}

impl SidebarKind {
    /// The rows this identity spends at the top of the sidebar.
    pub fn rows(self) -> u16 {
        match self {
            SidebarKind::Mark => LOGO_ROWS,
            SidebarKind::Text => IDENTITY_ROWS,
            SidebarKind::Hidden => 0,
        }
    }
}

/// One frame's regions, in terminal coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Regions {
    /// The main column: everything inside the frame right of the divider. It is
    /// what the floating overlays are centred in (spec §1).
    pub main: Rect,
    /// The transcript's rows: text, scrollbar and rail together.
    pub transcript: Rect,
    /// The rail's single column, at the transcript's right edge. Drawn whether or
    /// not the session has units to put in it.
    pub rail: Rect,
    /// The status row's content row: `模型 … │ 模式 … │ 上下文 …%`. Always drawn
    /// (spec §2, §5).
    pub status: Rect,
    /// The input rows inside the main column.
    pub input: Rect,
    /// The hint row, under the input.
    pub hints: Rect,
    /// The sidebar's content, when it is drawn at all. The divider's column is
    /// **not** part of it.
    pub sidebar: Option<Rect>,
    /// The tab bar's label row, when the sidebar is drawn.
    pub tabs: Option<Rect>,
    /// The sidebar's page rows — one row per field the height ladder kept, so the
    /// rectangle's own height is that count (spec §2, §3).
    pub sidebar_page: Option<Rect>,
    /// The column the sidebar and the main column share.
    pub divide: Option<u16>,
    /// Which sidebar identity this frame came out as.
    pub sidebar_kind: SidebarKind,
}

/// Whether the terminal is too small for anything but the notice sentence
/// ([`crate::render::wording::too_small`]).
pub fn below_minimum(area: Rect) -> bool {
    area.width < MIN_WIDTH || area.height < MIN_HEIGHT
}

impl Regions {
    /// The transcript's text area: its content less the two columns its right edge
    /// always keeps — the scrollbar's and the rail's.
    pub fn transcript_text(&self) -> Rect {
        Rect::new(
            self.transcript.x,
            self.transcript.y,
            self.transcript.width.saturating_sub(TRAILING_COLUMNS),
            self.transcript.height,
        )
    }

    /// The scrollbar's column, one to the left of the rail's.
    pub fn scrollbar(&self) -> Rect {
        Rect::new(
            self.transcript.right().saturating_sub(TRAILING_COLUMNS),
            self.transcript.y,
            TRAILING_COLUMNS - 1,
            self.transcript.height,
        )
    }

    /// The width of the question overlay inside the main column.
    pub fn modal_width(&self) -> u16 {
        self.main
            .width
            .saturating_sub(MODAL_MARGIN)
            .min(MODAL_MAX_WIDTH)
    }

    /// The width of the detail overlay: the same centring as a question, but with its
    /// own (wider) ceiling. A tool output is a body, not a sentence, so it is allowed
    /// more room before the eye has to travel (票 03 §Answer).
    pub fn detail_width(&self) -> u16 {
        self.main
            .width
            .saturating_sub(MODAL_MARGIN)
            .min(DETAIL_MAX_WIDTH)
    }

    /// Where the detail overlay goes: centred in the main column, one row short of
    /// the borders so a sliver of the transcript stays visible above and below.
    ///
    /// It is `None` when the terminal is too small to show a useful body — the same
    /// honest answer [`Regions::modal`] gives, and the detail view is then not opened
    /// at all rather than opened as two rows of border.
    pub fn detail(&self) -> Option<Rect> {
        let width = self.detail_width();
        if width <= BORDER_COLUMNS || self.main.height <= BORDER_COLUMNS + DETAIL_MIN_ROWS {
            return None;
        }
        let height = self
            .main
            .height
            .saturating_sub(BORDER_COLUMNS + DETAIL_MARGIN_ROWS);
        Some(Rect::new(
            self.main.x + (self.main.width - width) / 2,
            self.main.y + (self.main.height - height) / 2,
            width,
            height,
        ))
    }

    /// Where a question `rows` display rows tall goes: centred in the main column,
    /// or nowhere when it cannot be drawn legibly there.
    pub fn modal(&self, rows: u16) -> Option<Rect> {
        let width = self.modal_width();
        let height = rows.saturating_add(BORDER_COLUMNS);
        if width <= BORDER_COLUMNS || height > self.main.height {
            return None;
        }
        Some(Rect::new(
            self.main.x + (self.main.width - width) / 2,
            self.main.y + (self.main.height - height) / 2,
            width,
            height,
        ))
    }

    /// How many content rows a floating menu anchored at `anchor` can take where it
    /// would open: everything above the cursor's row, or the room below it when there
    /// is nothing above.
    ///
    /// The caller trims the matches to this before asking for a rectangle, so a menu
    /// scrolls instead of being refused.
    pub fn menu_room(&self, anchor: Placed) -> u16 {
        let cursor_y = self.input.y.saturating_add(anchor.row);
        let above = cursor_y.saturating_sub(self.main.y);
        if above > 1 {
            return above - 1;
        }
        self.main.bottom().saturating_sub(cursor_y + 1)
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
        let height = rows.saturating_add(BORDER_COLUMNS);
        let cursor_y = self.input.y.saturating_add(anchor.row);
        let top = self.main.y;
        let y = if cursor_y.saturating_sub(top) >= height {
            cursor_y - height
        } else if self.main.bottom().saturating_sub(cursor_y + 1) >= height {
            cursor_y + 1
        } else {
            return None;
        };
        let left = self.main.x;
        let right = self.main.right();
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

/// The width one input row has for text: the main column's content, less the
/// prompt. Known before [`plan`] runs, because the draft's own height is what plan
/// needs.
pub fn input_text_width(area: Rect) -> u16 {
    main_width(area.width).saturating_sub(crate::render::editor::prompt_columns())
}

/// The main column's content width.
///
/// The questionnaire needs this before [`plan`] runs — how many rows it wants
/// decides how tall the input area is — and it has to match the `input` rectangle
/// `plan` hands back, or the drawn rows and the requested height would disagree.
pub fn content_width(area: Rect) -> u16 {
    main_width(area.width)
}

/// Lay out one frame. `draft_rows` is how many rows the input's draft wraps to.
///
/// The order here **is** the degrade ladder: the sidebar is narrowed, then hidden,
/// as the terminal narrows; the input grows into the transcript's rows as the draft
/// does; and the sidebar's own height decides which of its parts survive (spec §2).
pub fn plan(area: Rect, draft_rows: u16) -> Regions {
    let inner = inner(area);
    let tier = sidebar_tier(area.width);
    let (sidebar_kind, fields) = sidebar_content(area.width, inner.height);
    let input_rows = draft_rows.max(1).min(max_input_rows(area.height));
    let transcript_rows = area.height.saturating_sub(CHROME + input_rows);

    let sidebar = tier.map(|tier| Rect::new(inner.x, inner.y, tier, inner.height));
    let divide = tier.map(|tier| inner.x + tier);
    let main_x = divide.map_or(inner.x, |divide| divide + 1);
    let main = Rect::new(main_x, inner.y, main_width(area.width), inner.height);

    let transcript = Rect::new(main.x, main.y, main.width, transcript_rows);
    let status = Rect::new(main.x, transcript.bottom() + 1, main.width, 1);
    let input = Rect::new(main.x, status.bottom() + 1, main.width, input_rows);
    let hints = Rect::new(main.x, input.bottom() + 1, main.width, 1);
    let rail = Rect::new(
        transcript.right().saturating_sub(1),
        transcript.y,
        1,
        transcript_rows,
    );

    Regions {
        main,
        transcript,
        rail,
        status,
        input,
        hints,
        sidebar,
        tabs: sidebar.map(|sidebar| {
            Rect::new(
                sidebar.x,
                sidebar.y + sidebar_kind.rows() + 1,
                sidebar.width,
                1,
            )
        }),
        sidebar_page: sidebar.map(|sidebar| {
            Rect::new(
                sidebar.x,
                sidebar.y + sidebar_kind.rows() + TAB_ROWS,
                sidebar.width,
                fields,
            )
        }),
        divide,
        sidebar_kind,
    }
}

/// The sidebar's content width at a terminal `width` wide, or `None` when it is
/// hidden. **Width alone decides that**: the sidebar is a column of its own, so its
/// height is not the transcript's to spend (spec §2).
fn sidebar_tier(width: u16) -> Option<u16> {
    if width >= SIDEBAR_WIDE_FROM {
        Some(SIDEBAR_WIDE)
    } else if width >= SIDEBAR_NARROW_FROM {
        Some(SIDEBAR_NARROW)
    } else {
        None
    }
}

/// The columns the main column gets: the frame's border, and the sidebar with its
/// divider column when the sidebar is drawn.
fn main_width(width: u16) -> u16 {
    let sidebar = sidebar_tier(width).map_or(0, |tier| tier + 1);
    width.saturating_sub(BORDER_COLUMNS + sidebar)
}

/// The sidebar's identity and how many usage fields it can show, given its own
/// content height.
///
/// The ladder is fixed: the **mark** goes first (to the text identity, then to
/// nothing), then fields from the tail — 缓存, then 输出, then 输入. The floor is
/// the tab bar plus 上下文 / token / 回合, so the three readings that answer "how
/// much is left" are the last to go (spec §2).
fn sidebar_content(width: u16, content_rows: u16) -> (SidebarKind, u16) {
    let Some(tier) = sidebar_tier(width) else {
        return (SidebarKind::Hidden, 0);
    };
    let mut kind = if tier >= LOGO_WIDTH {
        SidebarKind::Mark
    } else {
        SidebarKind::Text
    };
    let mut fields = SIDEBAR_FIELDS;
    loop {
        if kind.rows() + TAB_ROWS + fields <= content_rows {
            break;
        }
        match kind {
            SidebarKind::Mark => kind = SidebarKind::Text,
            SidebarKind::Text => kind = SidebarKind::Hidden,
            SidebarKind::Hidden if fields > SIDEBAR_MIN_FIELDS => fields -= 1,
            // The floor: the tab bar and the three readings. A terminal too short
            // even for those is below [`MIN_HEIGHT`] and never reaches here.
            SidebarKind::Hidden => break,
        }
    }
    (kind, fields)
}

/// The content rectangle of a bordered area.
pub fn inner(area: Rect) -> Rect {
    Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(BORDER_COLUMNS),
        area.height.saturating_sub(BORDER_COLUMNS),
    )
}

/// How many rows the input may take at this size: its cap, or what is left once the
/// transcript keeps its floor row, whichever is smaller.
fn max_input_rows(height: u16) -> u16 {
    let room = height.saturating_sub(CHROME + 1);
    MAX_INPUT_ROWS.min(room).max(1)
}

/// The widest the question overlay ever gets. Wider than this and the eye has to
/// travel: a question is one sentence, not a page (spec §9).
const MODAL_MAX_WIDTH: u16 = 72;

/// The blank columns the overlay leaves on either side of the main column.
const MODAL_MARGIN: u16 = 4;

/// The widest the detail overlay ever gets (票 03 §Answer；2026-09-23 加宽 50%：
/// 90 → 135)。A tool body is the one thing in the interface that is a page rather than
/// a sentence, so it gets the room until the terminal itself runs out: at 120 columns
/// the margin, not this ceiling, is what caps it.
const DETAIL_MAX_WIDTH: u16 = 135;

/// The fewest body rows a detail overlay is worth opening for.
const DETAIL_MIN_ROWS: u16 = 1;

/// The rows of transcript the detail overlay leaves showing, one above and one
/// below, so the reader keeps the place they clicked from.
const DETAIL_MARGIN_ROWS: u16 = 2;

/// The most rows the `/` menu shows before its matches scroll. A menu is a hint, not
/// a catalogue: past this the reader is scrolling a list to find a name they could
/// have typed (spec §6).
pub const MENU_MAX_ROWS: u16 = 8;

/// The widest the `/` menu ever gets: a name and its one-line description, without
/// the eye having to travel.
pub const MENU_MAX_WIDTH: u16 = 72;

/// The narrowest a menu box is worth drawing: below this the border is most of it.
const MENU_MIN_WIDTH: u16 = 12;
