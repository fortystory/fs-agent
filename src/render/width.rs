//! Display-width arithmetic: how many terminal columns a piece of text takes.
//!
//! One home for it, because two parts of the renderer need the same answer — the
//! conversation pane wraps styled lines to a width, and the header, the input line
//! and the indicator clip text to one — and a second copy is how `终` ends up
//! counted as one column.

use ratatui::buffer::CellWidth;

/// The display width of `text`, in terminal columns.
pub fn text_columns(text: &str) -> usize {
    text.cell_width() as usize
}

/// The display width of one character, in terminal columns.
pub fn char_columns(ch: char) -> usize {
    let mut buf = [0u8; 4];
    ch.encode_utf8(&mut buf).cell_width() as usize
}

/// The longest prefix of `text` that fits in `width` columns.
pub fn truncate_columns(text: &str, width: usize) -> String {
    let mut used = 0;
    let mut end = 0;
    for (index, ch) in text.char_indices() {
        let columns = char_columns(ch);
        if used + columns > width {
            break;
        }
        used += columns;
        end = index + ch.len_utf8();
    }
    text[..end].to_owned()
}
