//! 显示宽度的算术：一段文字占多少终端列。
//!
//! 只住一处，因为渲染器有两处需要同一个答案 —— 对话窗格把带样式的行折到某个宽度，
//! 左栏、提示行与指示器把文字裁到某一个 —— 而第二份拷贝就是 `终` 被算成一列的由来。

use ratatui::buffer::CellWidth;

/// `text` 的显示宽度，单位是终端列。
pub fn text_columns(text: &str) -> usize {
    text.cell_width() as usize
}

/// 单个字符的显示宽度，单位是终端列。
///
/// 控制字符算零列：画家会把它们滤掉，把它们数进去就会把光标放到文字并不在的地方。
/// 这也让这里避开 `cell_width` 那条路 —— 走到那里它会直接断言。
pub fn char_columns(ch: char) -> usize {
    if ch.is_control() {
        return 0;
    }
    let mut buf = [0u8; 4];
    ch.encode_utf8(&mut buf).cell_width() as usize
}

/// `text` 里能塞进 `width` 列的最长前缀。
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
