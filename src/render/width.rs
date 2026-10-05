//! 显示宽度的算术，以及它的两个出口：一段文字占多少终端列，怎么把它裁到某个宽度。
//!
//! 算术只住一处，因为渲染器有两处需要同一个答案 —— 窗格把带样式的行折到某个宽度，
//! 左栏、提示行与指示器把文字裁到某一个 —— 而第二份拷贝就是 `终` 被算成一列的由来。
//! 截断因此也在这里：`truncate_columns` 交回纯文本，`ellipsize_line` 保住每一片的样式。

use ratatui::buffer::CellWidth;
use ratatui::text::{Line, Span};

use super::wording;

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

/// `text` 里从第 `from` 列到第 `to` 列（不含）的那一段。
///
/// 按**显示列**切，宽字符不切半：跨过边界的那个宽字素整个丢掉，而不是留下半个（拖选取文本时
/// 宁可少取一个字符，也不能取回半个）。`from >= to` 或整段都在范围外时是空串。
pub fn slice_columns(text: &str, from: usize, to: usize) -> String {
    let mut out = String::new();
    let mut column = 0usize;
    for ch in text.chars() {
        let columns = char_columns(ch);
        if column + columns > to {
            break;
        }
        if column >= from {
            out.push(ch);
        }
        column += columns;
    }
    out
}

/// 把一条带样式的行裁到 `width` 列，并用一个 `…` 收尾。
///
/// 与 [`truncate_columns`] 是同一把尺子的两个出口：那个交回纯文本，这一个保住每一片的样式
/// —— 轨迹页里被截的那条消息行还带着发言者的颜色，砍掉样式会一并砍掉「谁在说话」。
pub fn ellipsize_line(line: Line<'static>, width: usize) -> Line<'static> {
    let budget = width.saturating_sub(1);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for span in line.spans {
        let mut kept = String::new();
        for ch in span.content.chars() {
            let columns = char_columns(ch);
            if used + columns > budget {
                break;
            }
            kept.push(ch);
            used += columns;
        }
        if !kept.is_empty() {
            spans.push(Span::styled(kept, span.style));
        }
        if used >= budget {
            break;
        }
    }
    // `…` 跟着最后一片的样式，好让它读起来是那一行的一部分。
    let style = spans.last().map(|span| span.style).unwrap_or_default();
    spans.push(Span::styled(wording::ELLIPSIS, style));
    Line {
        spans,
        style: line.style,
        alignment: line.alignment,
    }
}
