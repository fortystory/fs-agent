//! 选择用的**屏幕文本层**：每帧记下每块区域画出来的显示行，外加拖选的手势值与反白
//! （`.scratch/tui-feedback/spec.md` §5–§6）。
//!
//! 为什么要有这一层：应用占着鼠标，终端自己的选择因此被接管（要按住 shift 才回到终端手里），
//! 而终端原生选择按**终端的网格**取文本 —— 左栏、边框、弹窗会与它盖住的行混在一起，TUI 自己
//! 折出来的续行也被当成硬换行。所以要按**区域**取行，就得有一份「屏幕上这块区域画了什么」的
//! 记账。
//!
//! 它与 [`crate::render::tui`] 里的命中区域同一套纪律：每帧重建、由下一次指针事件来读，所以
//! 「记下来的」与「看见的」不可能漂开。记录点是**画那一行的同一个地方**。

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::Frame;

/// 一块能被拖选的区域是哪一类。
///
/// 它现在只用来读代码时认人（谁在哪一层），判断从不用它：换行语义按 `folded` 走、取值只能在
/// 所属区域的矩形内。留着它是为了 `block_at` 的调试与将来可能的分档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// 转录：对话页或轨迹页画出来的那些显示行。
    Transcript,
    /// 详情覆盖层的主体。
    Detail,
    /// 左栏的页（调用量 / todo / 文件）。
    Sidebar,
    /// 问卷的那些行（表头、题目、选项、自由文本行）。
    Questionnaire,
    /// 输入区的草稿。
    Draft,
}

/// 一条显示行：它的文本，以及它是不是上一行**软折**出来的续行。
///
/// `folded` 就是「复制时该不该在这里换行」的全部判据：同一来源行折出来的下一片要拼回去，
/// 而区域自己带的换行（Markdown 段落、代码块的多行）保留（spec §6）。
#[derive(Debug, Clone)]
pub struct TextRow {
    pub text: String,
    pub folded: bool,
}

impl TextRow {
    /// 一条没有软折的显示行。
    pub fn plain(text: String) -> Self {
        Self {
            text,
            folded: false,
        }
    }
}

/// 一块画在屏幕上的文本区域：它在哪，以及它画了哪些行。
#[derive(Debug, Clone)]
pub struct TextBlock {
    pub rect: Rect,
    pub kind: BlockKind,
    pub rows: Vec<TextRow>,
}

/// 这一帧的屏幕文本。每帧 `clear` 之后由各绘制点重新填。
#[derive(Debug, Default, Clone)]
pub struct ScreenText {
    blocks: Vec<TextBlock>,
}

impl ScreenText {
    /// 忘掉上一帧记下的一切（与 `Regions::clear` 同一处调用）。
    pub fn clear(&mut self) {
        self.blocks.clear();
    }

    /// 记下一块区域这一帧画了什么。
    ///
    /// 空矩形或没有行时什么都不记 —— 一块看不见的区域不该接得住指针。
    pub fn push(&mut self, rect: Rect, kind: BlockKind, rows: Vec<TextRow>) {
        if rect.width == 0 || rect.height == 0 || rows.is_empty() {
            return;
        }
        self.blocks.push(TextBlock { rect, kind, rows });
    }

    /// 指针落进哪一块：**最后画的那一块**（它盖在上面）。详情覆盖层因此在转录之上。
    pub fn block_at(&self, x: u16, y: u16) -> Option<usize> {
        self.blocks
            .iter()
            .rposition(|block| block.rect.contains((x, y).into()))
    }

    pub fn block(&self, index: usize) -> Option<&TextBlock> {
        self.blocks.get(index)
    }
}

/// 按下到抬起之间的一次拖选。
///
/// `block` 是按下那一刻落进的那块区域 —— 选区**只在那块区域里延伸**（spec §5 的明确不做那
/// 一条：不跨区域）。没落进任何文本块时它是 `None`，那次按下的抬起照旧按一次普通点击处理。
#[derive(Debug, Clone, Copy)]
pub struct Drag {
    pub block: Option<usize>,
    pub anchor: (u16, u16),
    pub head: (u16, u16),
    pub selecting: bool,
}

impl Drag {
    /// 一次按下的起点：还没有选什么。
    pub fn press(anchor: (u16, u16), block: Option<usize>) -> Self {
        Self {
            block,
            anchor,
            head: anchor,
            selecting: false,
        }
    }
}

/// 越过多远才算**拖**。
///
/// 一格就够：手抖是半格的事，而按住不放走一格已经是一次有意的动作。它与 `selecting` 是
/// 「这次抬起到手算点击还是算选择」的唯一分歧点。
const DRAG_THRESHOLD: u16 = 1;

impl Drag {
    /// 指针动到 `head`：越过门槛之后这次按下才算拖选。
    pub fn moved(&mut self, head: (u16, u16), rect: Rect) {
        // 夹在所属区域里 —— 拖出区域不是「选更多」，而是「不再选那里」。
        self.head = (
            head.0.clamp(rect.x, rect.right().saturating_sub(1)),
            head.1.clamp(rect.y, rect.bottom().saturating_sub(1)),
        );
        let moved = self.head.0.abs_diff(self.anchor.0) + self.head.1.abs_diff(self.anchor.1);
        if moved > DRAG_THRESHOLD {
            self.selecting = true;
        }
    }

    /// 这次拖选盖住的那块矩形，按行规范化（从不倒着画）。选区还没跨过门槛时是 `None`。
    pub fn cover(&self, text: &ScreenText) -> Option<Rect> {
        if !self.selecting {
            return None;
        }
        let block = self.block.and_then(|index| text.block(index))?;
        let rect = block.rect;
        let (left, right) = if self.anchor.0 <= self.head.0 {
            (self.anchor.0, self.head.0)
        } else {
            (self.head.0, self.anchor.0)
        };
        let (top, bottom) = if self.anchor.1 <= self.head.1 {
            (self.anchor.1, self.head.1)
        } else {
            (self.head.1, self.anchor.1)
        };
        Some(Rect::new(
            left.max(rect.x),
            top.max(rect.y),
            right.min(rect.right().saturating_sub(1)) - left.max(rect.x) + 1,
            bottom.min(rect.bottom().saturating_sub(1)) - top.max(rect.y) + 1,
        ))
    }
}

/// 把选区反白画上去。
///
/// 它只碰缓冲（`Modifier::REVERSED`），不动任何绘制函数 —— 于是覆盖层、菜单、问卷、名字的
/// 颜色与字形都不会因为选择而改形。调用点是 `draw_frame` 的最后一步，所以它盖在所有层之上。
pub fn paint(frame: &mut Frame, text: &ScreenText, drag: Option<&Drag>) {
    let Some(drag) = drag else {
        return;
    };
    let Some(rect) = drag.cover(text) else {
        return;
    };
    let buffer = frame.buffer_mut();
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            buffer[(x, y)].modifier.insert(Modifier::REVERSED);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block() -> ScreenText {
        let mut text = ScreenText::default();
        text.push(
            Rect::new(41, 2, 20, 4),
            BlockKind::Transcript,
            vec![
                TextRow::plain("第一行".to_owned()),
                TextRow {
                    text: "续行".to_owned(),
                    folded: true,
                },
            ],
        );
        text
    }

    #[test]
    fn the_pointer_lands_on_the_block_that_was_drawn_last() {
        let mut text = block();
        // 后画的一块盖在同样位置上：详情覆盖层与转录的关系。
        text.push(
            Rect::new(41, 2, 20, 2),
            BlockKind::Detail,
            vec![TextRow::plain("覆盖层".to_owned())],
        );
        assert_eq!(text.block_at(41, 2), Some(1), "最上层的那一块");
        assert_eq!(text.block_at(41, 5), Some(0), "覆盖层之外仍是转录");
        assert_eq!(text.block_at(10, 5), None, "左栏之外没有东西");
    }

    #[test]
    fn a_drag_stays_inside_its_block_and_normalises_the_direction() {
        let text = block();
        let mut drag = Drag::press((45, 4), Some(0));
        assert!(!drag.selecting, "按下本身不是选择");
        drag.moved((41, 2), Rect::new(41, 2, 20, 4));
        assert!(drag.selecting, "越过门槛之后才是");
        assert_eq!(drag.head, (41, 2));
        let cover = drag.cover(&text).expect("有覆盖矩形");
        assert_eq!((cover.x, cover.y), (41, 2));
        assert_eq!((cover.width, cover.height), (5, 3), "两个方向都规范化");
        // 拖出区域：头被夹在区域里，不会选到隔壁去。
        drag.moved((200, 200), Rect::new(41, 2, 20, 4));
        assert_eq!(drag.head, (60, 5));
    }

    #[test]
    fn a_single_cell_wobble_is_not_a_selection() {
        let text = block();
        let mut drag = Drag::press((45, 4), Some(0));
        drag.moved((45, 4), Rect::new(41, 2, 20, 4));
        assert!(!drag.selecting, "没动就不是选择");
        assert!(drag.cover(&text).is_none());
    }
}
