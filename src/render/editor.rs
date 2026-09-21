//! The input editor: a multi-line draft, its keymap, and where its cursor is.
//!
//! The cursor's only state is a **character index** into the draft — never a row,
//! never a column, never a byte offset. Display rows exist only while a frame is
//! being drawn, derived from that index each time. That is what keeps a resize, a
//! rewrap or a wide character from leaving the cursor somewhere the user did not
//! put it: the failure mode the inline viewport had (ADR 0002).
//!
//! Editing is **line-aware**. Emacs' Ctrl chords and `Home`/`End` act on the
//! logical line the cursor is on, so a multi-line draft cannot lose several lines
//! to one `Ctrl-U`, and `↑`/`↓` move the cursor instead of walking history — the
//! history keys are `Ctrl-P` / `Ctrl-N` alone (spec §6).

use ratatui::text::Line;

use super::width::char_columns;

/// The prompt on the draft's first row.
pub const PROMPT: &str = "> ";

/// The columns the prompt takes — and therefore the indent every row after the
/// first one carries, so every row holds the same amount of text.
pub const PROMPT_COLUMNS: u16 = 2;

/// The separator between logical lines in the draft.
const NEWLINE: char = '\n';

/// Where the cursor sits among the rows a frame drew.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub row: u16,
    pub column: u16,
}

/// One display row of the draft.
struct Row {
    text: String,
    /// The character index this row starts at.
    start: usize,
    /// The character index just past this row's last character.
    end: usize,
}

/// What the user has typed, and where the cursor is in it.
pub struct Input {
    text: String,
    /// The cursor, as a **character** index into `text`. See the module docs.
    cursor: usize,
    /// Drafts already submitted, oldest first, for `Ctrl-P` / `Ctrl-N`.
    history: Vec<String>,
    /// Where in [`Input::history`] the browse currently is.
    history_at: Option<usize>,
    /// The fresh draft stashed when a browse began.
    draft: String,
    /// The visual column `↑`/`↓` are trying to hold, until something else moves the
    /// cursor.
    goal: Option<usize>,
}

impl Input {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_at: None,
            draft: String::new(),
            goal: None,
        }
    }

    /// The draft, as it would be submitted.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the draft spans more than one logical line.
    pub fn has_multiple_lines(&self) -> bool {
        self.text.contains(NEWLINE)
    }

    /// How many display rows the draft needs at `width` text columns.
    pub fn rows(&self, width: u16) -> u16 {
        self.display_rows(width.max(1) as usize).0.len() as u16
    }

    /// The rows to draw and where the cursor sits among them.
    ///
    /// Scrolled to keep the cursor's row inside `height`: the draft grows to its
    /// ten-row limit and then scrolls, rather than hiding what is being typed
    /// (spec §5).
    pub fn view(&self, width: u16, height: u16) -> (Vec<Line<'static>>, Placed) {
        let (rows, placed) = self.display_rows(width.max(1) as usize);
        let height = (height.max(1)) as usize;
        let top = if placed.row as usize >= height {
            placed.row as usize + 1 - height
        } else {
            0
        };
        let lines = rows
            .iter()
            .enumerate()
            .skip(top)
            .take(height)
            .map(|(index, row)| Line::from(format!("{}{}", indent(index), row.text)))
            .collect();
        (
            lines,
            Placed {
                row: placed.row - top as u16,
                column: placed.column,
            },
        )
    }

    // --- editing ------------------------------------------------------------

    pub fn insert_char(&mut self, ch: char) {
        let at = self.byte_at(self.cursor);
        self.text.insert(at, ch);
        self.cursor += 1;
        self.edited();
    }

    /// Insert a run of text at the cursor, as one edit.
    ///
    /// This is the paste path: the text may carry newlines of its own, and it must
    /// never submit (spec §7).
    pub fn insert_str(&mut self, text: &str) {
        let at = self.byte_at(self.cursor);
        self.text.insert_str(at, text);
        self.cursor += text.chars().count();
        self.edited();
    }

    /// Delete one character before the cursor; at a line start this joins the line
    /// to the one above.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.remove_range(self.cursor - 1, self.cursor);
    }

    /// Delete one character after the cursor; at a line end this pulls the next
    /// line up.
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.len() {
            return;
        }
        self.remove_range(self.cursor, self.cursor + 1);
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
        self.moved();
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
        self.moved();
    }

    /// The head of the cursor's **line**, not of the draft.
    pub fn home(&mut self) {
        self.cursor = self.line_bounds().0;
        self.moved();
    }

    /// The end of the cursor's **line**, not of the draft.
    pub fn end(&mut self) {
        self.cursor = self.line_bounds().1;
        self.moved();
    }

    /// Move up a line, holding the visual column the cursor had.
    pub fn up(&mut self) {
        let (start, _) = self.line_bounds();
        if start == 0 {
            return;
        }
        let want = self.goal.unwrap_or_else(|| self.column());
        self.cursor = start - 1;
        let line = self.line_bounds();
        self.place_at_column(line, want);
        self.goal = Some(want);
    }

    /// Move down a line, holding the visual column the cursor had.
    pub fn down(&mut self) {
        let (_, end) = self.line_bounds();
        if end >= self.len() {
            return;
        }
        let want = self.goal.unwrap_or_else(|| self.column());
        self.cursor = end + 1;
        let line = self.line_bounds();
        self.place_at_column(line, want);
        self.goal = Some(want);
    }

    pub fn kill_to_line_start(&mut self) {
        let start = self.line_bounds().0;
        self.remove_range(start, self.cursor);
    }

    pub fn kill_to_line_end(&mut self) {
        let end = self.line_bounds().1;
        self.remove_range(self.cursor, end);
    }

    /// `Ctrl-W`: drop trailing spaces, then one run of non-spaces, before the
    /// cursor — the shell's word-erase, held inside the line.
    pub fn kill_word(&mut self) {
        let (start, _) = self.line_bounds();
        let chars: Vec<char> = self
            .text
            .chars()
            .skip(start)
            .take(self.cursor - start)
            .collect();
        let mut word = chars.len();
        while word > 0 && chars[word - 1].is_whitespace() {
            word -= 1;
        }
        while word > 0 && !chars[word - 1].is_whitespace() {
            word -= 1;
        }
        self.remove_range(start + word, self.cursor);
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.history_at = None;
        self.draft.clear();
        self.goal = None;
    }

    /// Take the draft for submission: trimmed, cleared, and remembered.
    ///
    /// Only the ends are trimmed — the blank lines inside a multi-line draft are
    /// part of what the user wrote (spec §6).
    pub fn submitted(&mut self) -> String {
        let line = self.text.trim().to_owned();
        self.clear();
        if !line.is_empty() && self.history.last() != Some(&line) {
            self.history.push(line.clone());
        }
        line
    }

    // --- history ------------------------------------------------------------

    /// `Ctrl-P`: step to the older draft, stashing the fresh one first.
    pub fn history_previous(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let at = match self.history_at {
            None => {
                self.draft = self.text.clone();
                self.history.len() - 1
            }
            Some(0) => return,
            Some(at) => at - 1,
        };
        self.history_at = Some(at);
        let recalled = self.history[at].clone();
        self.set(&recalled);
    }

    /// `Ctrl-N`: step to the newer draft, or back to the fresh one.
    pub fn history_next(&mut self) {
        match self.history_at {
            None => {}
            Some(at) if at + 1 < self.history.len() => {
                self.history_at = Some(at + 1);
                let recalled = self.history[at + 1].clone();
                self.set(&recalled);
            }
            Some(_) => {
                self.history_at = None;
                let draft = std::mem::take(&mut self.draft);
                self.set(&draft);
            }
        }
    }

    // --- internals ----------------------------------------------------------

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// The byte offset of character index `at`, clamped to the end.
    fn byte_at(&self, at: usize) -> usize {
        self.text
            .char_indices()
            .nth(at)
            .map(|(index, _)| index)
            .unwrap_or(self.text.len())
    }

    /// A fresh edit: the history browse, the goal column and the draft are done.
    fn edited(&mut self) {
        self.history_at = None;
        self.goal = None;
    }

    /// A move that is not `↑`/`↓`: it keeps the history browse but drops the goal.
    fn moved(&mut self) {
        self.goal = None;
    }

    /// The character indices bounding the line the cursor is on: the first
    /// character, and the newline that ends it.
    fn line_bounds(&self) -> (usize, usize) {
        let len = self.len();
        let mut start = 0;
        let mut end = len;
        for (index, ch) in self.text.chars().enumerate() {
            if ch != NEWLINE {
                continue;
            }
            if index < self.cursor {
                start = index + 1;
            } else if end == len {
                end = index;
            }
        }
        (start, end)
    }

    /// The cursor's visual column within its line.
    fn column(&self) -> usize {
        let (start, _) = self.line_bounds();
        self.text
            .chars()
            .skip(start)
            .take(self.cursor - start)
            .map(char_columns)
            .sum()
    }

    /// Put the cursor on `line` at the character nearest visual column `want`,
    /// clamped to the line's end.
    fn place_at_column(&mut self, line: (usize, usize), want: usize) {
        let (start, end) = line;
        let mut used = 0;
        let mut cursor = start;
        for (offset, ch) in self.text.chars().skip(start).take(end - start).enumerate() {
            if used >= want {
                break;
            }
            used += char_columns(ch);
            cursor = start + offset + 1;
        }
        self.cursor = cursor;
    }

    fn remove_range(&mut self, from: usize, to: usize) {
        let start = self.byte_at(from);
        let end = self.byte_at(to);
        self.text.replace_range(start..end, "");
        self.cursor = from;
        self.edited();
    }

    fn set(&mut self, text: &str) {
        self.text = text.to_owned();
        self.cursor = self.len();
        self.goal = None;
    }

    /// The wrapped rows and where the cursor sits among them.
    fn display_rows(&self, width: usize) -> (Vec<Row>, Placed) {
        let mut rows: Vec<Row> = Vec::new();
        let mut index = 0usize;
        for line in self.text.split(NEWLINE) {
            let mut text = String::new();
            let mut start = index;
            let mut used = 0usize;
            for ch in line.chars() {
                let columns = char_columns(ch);
                if used + columns > width && used > 0 {
                    rows.push(Row {
                        text: std::mem::take(&mut text),
                        start,
                        end: index,
                    });
                    start = index;
                    used = 0;
                }
                text.push(ch);
                used += columns;
                index += 1;
            }
            rows.push(Row {
                text,
                start,
                end: index,
            });
            index += 1; // the newline the split consumed
        }

        let cursor = self.cursor.min(self.len());
        let mut at = 0;
        for (index, row) in rows.iter().enumerate() {
            if row.start <= cursor {
                at = index;
            }
        }
        // A cursor sitting on a wrap boundary belongs to the row that starts there;
        // on a line boundary it belongs to the end of the line it is leaving.
        if rows[at].end == cursor && rows.get(at + 1).is_some_and(|next| next.start == cursor) {
            at += 1;
        }
        let offset = cursor - rows[at].start;
        let column = PROMPT_COLUMNS as usize
            + rows[at]
                .text
                .chars()
                .take(offset)
                .map(char_columns)
                .sum::<usize>();
        if column >= PROMPT_COLUMNS as usize + width {
            // The cursor is at the end of a full row and there is nowhere to put it:
            // it gets a row of its own, as a terminal would give it.
            rows.push(Row {
                text: String::new(),
                start: cursor,
                end: cursor,
            });
            at = rows.len() - 1;
            return (
                rows,
                Placed {
                    row: at as u16,
                    column: PROMPT_COLUMNS,
                },
            );
        }
        (
            rows,
            Placed {
                row: at as u16,
                column: column as u16,
            },
        )
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

/// What leads a display row: the prompt on the draft's first row, an indent as
/// wide as it on every row after.
fn indent(index: usize) -> &'static str {
    if index == 0 {
        PROMPT
    } else {
        "  "
    }
}

/// Normalise pasted text: line endings become `\n`, and control characters are
/// dropped except the two that carry meaning.
///
/// crossterm hands a paste through untouched — no `\r` stripping, no control
/// filtering, no length limit (research §6.3) — so the cleaning is ours.
pub fn normalize_paste(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|ch| *ch == NEWLINE || *ch == '\t' || !ch.is_control())
        .collect()
}
