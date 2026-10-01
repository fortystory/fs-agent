//! 左栏的 `todo` 页，以及决定它在不在的那只闩（`.scratch/todo-and-modes/spec.md` §4）。
//!
//! 两件事让它自成一个面板，而不是 [`super::panel`] 的一部分：
//!
//! * **列表是从流上推出来的，不是从它旁边的状态里来的。** 渲染器见到的每一次 `todo`
//!   调用都把整份列表带在自己的参数里，所以这一页就是最近一次主会话调用说的话。没有
//!   存下任何可能与流不一致的东西，而 `--continue` 靠重放同样的调用重建出同一页。
//! * **这个页签是一只闩，不是一个条件。** 主会话一旦提交过一份非空列表，这个页签在此后
//!   整个会话里都在 —— 哪怕列表后来被清空了。一个随列表来去的页签会挪动读者正在看的
//!   那一页，而用户选的是「看见了就一直在」。
//!
//! 执行者的列表不在其中：它是它自己的一份记录（spec §2），在转录里看得见，在左栏里
//! 哪儿都没有。

use ratatui::layout::Rect;
use ratatui::text::Line;

use crate::events::SpeakerId;
use crate::tools::todo::{read_items, Item, TODO_TOOL};

use super::transcript::Block;
use super::width::truncate_columns;
use super::wording;

/// `todo` 页签显示什么，以及它到底给不给。
#[derive(Debug, Default)]
pub struct TodoPanel {
    /// 主会话有没有提交过一份非空列表。置上就不再清：它就是这页签有没有的全部条件。
    seen: bool,
    /// 当时生效的列表，来自最近一次主会话调用。空在这里是一个真实状态（列表被清空了），
    /// 它不清 `seen`。
    items: Vec<Item>,
}

impl TodoPanel {
    /// 看住一个已渲染的块。只有来自非执行者发言、且已落地的 `todo` 调用才算数。
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

    /// 页签条上有没有 `todo` 这个标签。
    pub fn visible(&self) -> bool {
        self.seen
    }

    /// 这一页从上到下的行，给这个高度的页区。
    ///
    /// 没有滚动：高度阶梯给出行数，**计数行**先被留出来（这页就是为它而设的），塞不下
    /// 的在它上面用一行报出来 —— `＋3 项`。只放得下一行的页就只显示计数，而不是一个
    /// 孤零零的条目。
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

        // 计数占着最后一行，条目拿剩下的 —— 全都塞不下时，那些行里有一行归溢出行。
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

/// 一个条目的行：先它的状态字形，再（有的话）它的目标条目 id，再它的内容，按页宽裁掉。
///
/// 带 id 的项写成 `✓ 03 补测试`：id 是它引用目标清单哪一条的凭据，读的人要能看见它
/// （`.scratch/goal-loop/spec.md` §3）。没 id 的项照旧，宽度阶梯与计数行都不动。
fn item_line(item: &Item, width: usize) -> Line<'static> {
    let glyph = wording::todo_glyph(item.status);
    let text = match &item.id {
        Some(id) => format!("{glyph} {id} {}", item.content),
        None => format!("{glyph} {}", item.content),
    };
    Line::from(truncate_columns(&text, width))
}
