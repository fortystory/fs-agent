//! 转录窗格的滚动缓冲（spec §3、§4）。
//!
//! 它被**两个视图**共用：对话视图与轨迹视图各持一个实例，各自折行、各自记视口。
//!
//! 转录归这个窗格所有，而不是归终端的滚动回退（ADR 0002），所以它得回答三个以前由终端
//! 替我们回答的问题：留多少历史、在当前宽度下怎么折行、视口待在哪儿。上限回答第一个，
//! 折行缓存回答第二个，吸底回答第三个。
//!
//! **刻意用两个单位。** 上限数的是*来源*行 —— 一个块在折行之前渲染成的东西 —— 因为用
//! 显示行数的上限会在不同终端宽度下留下不同数量的历史。视口、滚动条与「新内容」指示器
//! 数的是*显示*行，那是人真正看到的东西。

use std::collections::VecDeque;

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::width::char_columns;

/// 窗格在丢掉最旧的那些之前保留多少来源行（spec §3）。
pub const CAP: usize = 20_000;

/// 一次 PgUp/PgDn 从它正离开的那一页留下几行，好让读者不断线索（spec §4）。
const PAGE_OVERLAP: usize = 2;

/// 一格滚轮滚几行（spec §4）。
const WHEEL_ROWS: usize = 3;

/// 对话窗格的缓冲与视口。
pub struct Pane {
    /// 来源行，最旧的在前。长度永不超过 [`CAP`]。
    lines: VecDeque<Line<'static>>,
    /// 每条来源行起始的显示行。与 `lines` 平行。
    starts: VecDeque<usize>,
    /// `lines` 在 `width` 下折行后的显示行。
    wrapped: VecDeque<Line<'static>>,
    /// `wrapped` 已经算进了多少条来源行。
    wrapped_sources: usize,
    /// 上面那些东西折行时用的宽度；第一帧之前是零。
    width: u16,
    /// 上一帧窗格的高度，好让一次按键或一格滚轮知道自己的步长。
    height: u16,
    /// 上一帧的显示行数：来源行与实时尾巴合在一起。
    total: usize,
    /// 视口顶端的那一个显示行。
    top: usize,
    /// 视口顶端所在的那条来源行 —— 重新折行时盯住的就是它。
    top_source: usize,
    /// 视口是不是跟着底部走。
    follow: bool,
    /// 上一个跟着底部的帧当时的 `total`。指示器数的是那之后到达的东西。
    seen: usize,
    /// 计数是不是被按住了 —— 读者把转录停在一行上看着，而不是从头滚到尾（票 02 §4）。
    holding: bool,
}

impl Pane {
    pub fn new() -> Self {
        Self {
            lines: VecDeque::new(),
            starts: VecDeque::new(),
            wrapped: VecDeque::new(),
            wrapped_sources: 0,
            width: 0,
            height: 0,
            total: 0,
            top: 0,
            top_source: 0,
            follow: true,
            seen: 0,
            holding: false,
        }
    }

    /// 追加一条来源行；到了 [`CAP`] 就把最旧的丢掉，并**报出丢了几条**。
    ///
    /// 那个返回数是裁剪的**唯一权威**：平行表（行链接、每源行索引、回合条）全都按它裁，
    /// 于是「平行表与窗格的源行窗口同进同出」是契约而不是巧合
    /// （`.scratch/trace-tab/issues/07-pane-evict-accounting.md`）。
    pub fn push(&mut self, line: Line<'static>) -> usize {
        self.lines.push_back(line);
        self.wrap_pending();
        self.evict()
    }

    /// 替换最新那条来源行。给转录里唯一那条会变的行用：思考提示在它开始时写下，在 trace
    /// 写完时原地重写一遍，所以读者永远不会为一个想法看到两行（票 02 §1）。
    ///
    /// 在空窗格上什么都不做，这是诚实的答案：没有东西可重写。
    pub fn replace_last(&mut self, line: Line<'static>) {
        let Some(last) = self.lines.back_mut() else {
            return;
        };
        *last = line;
        self.forget_last_wrap();
    }

    /// 给最新那条来源行**追加一段**，不动它的其余部分。
    ///
    /// 一笔用量长在它所归属的那次调用的行尾（ADR 0016），所以它得能补写已经画出去的那一行，
    /// 而不是新起一行 —— 那行的时间戳、名字配色与详情入口都留在原地。
    pub fn append_to_last(&mut self, span: Span<'static>) {
        let Some(last) = self.lines.back_mut() else {
            return;
        };
        last.spans.push(span);
        self.forget_last_wrap();
    }

    /// 替换**任意一条**来源行，就地改写它。给「那一行自己会变、而它不是最后一行」用：
    /// 一个单位（回合 / 轮次）的组头随回合推进而增长（`.scratch/trace-ledger/spec.md` §5），
    /// 所以要能改写**已经画出去、后面还有行**的那一条。
    ///
    /// 与 [`Pane::replace_last`] 的差别正是「任意一条」：折行缓存里它的后面那些起点要平移，
    /// 视口那几项也要 —— 否则读者脚下的位置会被一次改写悄悄挪走。
    ///
    /// **替换一条不存在的行是空操作**（而不是改最后一行）：那是调用方算错了下标，而静默改掉
    /// 另一条内容更糟。
    pub fn replace_at(&mut self, source: usize, line: Line<'static>) {
        let Some(slot) = self.lines.get_mut(source) else {
            return;
        };
        *slot = line;
        if self.wrapped_sources <= source {
            // 这一条还没折过行，所以下一趟 `wrap_pending` 会折它 —— 没有陈旧缓存要清。
            return;
        }
        let width = self.width.max(1) as usize;
        let start = self.starts[source];
        let end = self
            .starts
            .get(source + 1)
            .copied()
            .unwrap_or(self.wrapped.len());
        let old_height = end.saturating_sub(start);
        let fresh = wrap_line(&self.lines[source], width);
        let new_height = fresh.len();
        self.wrapped.drain(start..end);
        // 后面的起点全体平移高度差：那一段换成了新折出来的行。
        let delta = new_height as isize - old_height as isize;
        for offset in self.starts.iter_mut().skip(source + 1) {
            *offset = shift(*offset, delta);
        }
        for (offset, row) in fresh.into_iter().enumerate() {
            self.wrapped.insert(start + offset, row);
        }
        // 视口那几项都按同一个高度差走 —— 读者看到的那一行不动，这是「就地改写」的全部意思。
        // `top_source` 记的是**来源行**下标，而来源行的条数没变，所以它一个字都不动。
        self.top = shift(self.top, delta);
        self.total = shift(self.total, delta);
        self.seen = shift(self.seen, delta);
    }

    /// 刚改过的那条来源行如果已经折过行，它的显示行就是陈旧的 —— 而且它们是缓存里**最后**
    /// 那些，所以正好丢掉它们就是全部工作。清掉整个缓存反而会把更早每一行的显示行都扔了，
    /// 而 `starts` 还在指它们的老偏移：于是窗格报出两行，历史从屏幕上消失，也没有什么可以
    /// 往回滚了（2026-09-23，用户报告）。
    fn forget_last_wrap(&mut self) {
        if self.wrapped_sources == self.lines.len() {
            if let Some(start) = self.starts.pop_back() {
                self.wrapped.truncate(start);
            }
            self.wrapped_sources -= 1;
        }
    }

    /// 清空来源行，好按**新的宽度**重放它们（`.scratch/markdown-render/spec.md` §1）。
    ///
    /// 宽度变化时要走这条而不是只重新折行：一条来源行本身现在就是按宽度排出来的（表格的
    /// 列宽、超宽代码行的折行），所以它们得整批重排。视口的**意图**留着 —— `follow`、
    /// `holding`，以及视口所在的那条来源行 —— 重放之后下一帧 `view` 会照着它重新折行。
    pub fn clear(&mut self) {
        self.lines.clear();
        self.starts.clear();
        self.wrapped.clear();
        self.wrapped_sources = 0;
        self.width = 0;
    }

    /// 一个显示行属于哪条来源行，如果有的话。
    ///
    /// 一次点击就是这样把屏幕行变回块的：窗格数的是显示行，而一次点击能打开的每样东西都
    /// 按来源行寻址（票 04 §1）。
    pub fn source_at(&self, display_row: usize) -> Option<usize> {
        if display_row >= self.total {
            return None;
        }
        match self.starts.binary_search(&display_row) {
            Ok(exact) => (exact < self.lines.len()).then_some(exact),
            Err(insert) => {
                let source = insert.checked_sub(1)?;
                (source < self.lines.len()).then_some(source)
            }
        }
    }

    /// 要画的行：从视口顶端起 `height` 个显示行，其中正在流的 `live` 文本折行后接在来源
    /// 行之后。
    ///
    /// 折行缓存也是在这里被更新到最新的，这就是它收 `&mut self` 的原因：要紧的那个宽度是
    /// 这一帧真正画出来的宽度，而那个只在这里知道。
    pub fn view(&mut self, width: u16, height: u16, live: &[Line<'static>]) -> Vec<Line<'static>> {
        self.ensure(width);
        let width = width.max(1) as usize;
        // 尾巴是**带样式的行**，不是一段文字：等待提示与正文尾巴都从这里过，各自的样式
        // 才不会在折行时丢掉。
        let live_rows: Vec<Line<'static>> = live
            .iter()
            .flat_map(|line| wrap_line(line, width))
            .collect();
        self.height = height;
        self.total = self.wrapped.len() + live_rows.len();

        let height = height as usize;
        let max_top = self.total.saturating_sub(height);
        if self.follow {
            self.top = max_top;
        } else if self.top > max_top {
            // 转录在视口下面缩了：可能是上限丢了行，也可能是窗格长高了。落在底部是唯一
            // 诚实的去处。
            self.top = max_top;
        }
        if self.top >= max_top {
            self.follow = true;
            self.seen = self.total;
        }
        self.sync_top_source();
        self.window(height, &live_rows)
    }

    /// 把视口顶端放到一条**来源行**上，顶对齐。
    ///
    /// 回合条的格子就是这样跳的：一格代表的单位是一段来源行，落在它的第一条上就把读者放
    /// 在那个回合的开头，而不是它中间的某处（`.scratch/tui-sidebar/spec.md` §4）。一条已经
    /// 在最后一个整屏之后的来源行 —— 通常是最新的那个单位 —— 夹到底部，所以最后一格不
    /// 需要特例。
    pub fn scroll_to_source(&mut self, source: usize) {
        let row = self.starts.get(source).copied().unwrap_or(0);
        self.follow = false;
        let max_top = self.total.saturating_sub(self.height as usize);
        self.top = row.min(max_top);
        if self.top >= max_top {
            self.follow = true;
            self.seen = self.total;
        }
        self.sync_top_source();
    }

    /// 滚动 `rows` 个显示行；负数是往上。
    pub fn scroll(&mut self, rows: isize) {
        let max_top = self.total.saturating_sub(self.height as usize);
        if rows < 0 {
            // 离开底部才是「开始数此后到达的东西」的那一刻；`seen` 留在上一帧把它放在的
            // 地方，所以指示器量的是*那之后*到达的，而不只是底下有多少。
            self.follow = false;
        }
        self.top = (self.top as isize + rows).clamp(0, max_top as isize) as usize;
        if self.top >= max_top {
            self.follow = true;
            self.seen = self.total;
        }
        self.sync_top_source();
    }

    /// 翻一页，留下 [`PAGE_OVERLAP`] 行正在离开的那一页。
    pub fn page(&mut self, up: bool) {
        let step = (self.height as usize).saturating_sub(PAGE_OVERLAP).max(1);
        self.scroll(if up { -(step as isize) } else { step as isize });
    }

    /// 一格滚轮。
    pub fn wheel(&mut self, up: bool) {
        self.scroll(if up {
            -(WHEEL_ROWS as isize)
        } else {
            WHEEL_ROWS as isize
        });
    }

    /// 重新跟着底部；下一帧把视口放到那里。
    pub fn to_bottom(&mut self) {
        self.follow = true;
        self.top = self.total.saturating_sub(self.height as usize);
        self.seen = self.total;
        self.sync_top_source();
    }

    /// 把视口放回记下来的那个位置：`top` 是显示行，`follow` 是当时跟不跟底。
    ///
    /// 内容从那时起可能长了或短了，所以位置会被夹回合法范围；`follow` 为真时直接回到底部
    /// —— 那正是打开前贴底的情形，而它与 [`Pane::set_following`] 逐字相同。
    pub fn restore(&mut self, top: usize, follow: bool) {
        if follow {
            self.to_bottom();
            return;
        }
        let max_top = self.total.saturating_sub(self.height as usize);
        self.top = top.min(max_top);
        self.follow = false;
        self.sync_top_source();
    }

    /// 视口此刻是不是跟着底部走。
    pub fn following(&self) -> bool {
        self.follow
    }

    /// 停住对新行的计数，或者重新开始。
    ///
    /// 详情覆盖层占住视口；它开着的时候，「N 条新行」这个计数会跟着读者看不见、也没被要求
    /// 去读的输出一起涨（票 02 §4）。计数在打开扣住的那一帧重新取基准，所以松开它之后是
    /// 从读者的新位置开始量的。
    pub fn set_holding(&mut self, holding: bool) {
        if holding && !self.holding {
            self.seen = self.total;
        }
        self.holding = holding;
    }

    /// 把视口扣在原地，或者让它重新跟着底部走。
    ///
    /// 详情覆盖层读的是一份冻住的转录：不管 `follow` 的话，一阵突发输出会把读者打开的那
    /// 一行从他脚下拽走（票 02 §4）。松开它把人送回底部，一个已经不再读历史的读者想去的
    /// 正是那里。
    pub fn set_following(&mut self, follow: bool) {
        if follow {
            self.to_bottom();
        } else {
            self.follow = false;
        }
    }

    /// 视口上次离开底部之后到达的显示行数。
    pub fn fresh(&self) -> usize {
        if self.follow || self.holding {
            0
        } else {
            self.total.saturating_sub(self.seen)
        }
    }

    /// 上一帧的显示行数。
    pub fn total(&self) -> usize {
        self.total
    }

    /// 窗格现在持有多少条来源行 —— 平行表该有的长度。
    pub fn sources(&self) -> usize {
        self.lines.len()
    }

    /// 视口顶端的那一个显示行。
    pub fn top(&self) -> usize {
        self.top
    }

    /// 把折行缓存更新到 `width`，并在宽度变化时把视口留在它原来显示的那条来源行上。
    fn ensure(&mut self, width: u16) {
        if width == self.width {
            self.wrap_pending();
            return;
        }
        self.width = width;
        self.wrapped.clear();
        self.starts.clear();
        self.wrapped_sources = 0;
        self.wrap_pending();
        if !self.follow {
            // 每一个显示行都动了，所以行号现在指的是别的东西；一次重新折行之后活下来的
            // 是来源行（spec §4）。来源行被整批换掉时（`clear` 之后的重放）它可能已经
            // 不在了，那就留在原地，让 `view` 去夹。
            self.top = self
                .starts
                .get(self.top_source)
                .copied()
                .unwrap_or(self.top);
        }
    }

    /// 把上一趟之后到达的来源行折行。
    fn wrap_pending(&mut self) {
        if self.width == 0 {
            // 还没有画过任何一帧，所以没有宽度可折。第一次 `view` 会把全部做完。
            return;
        }
        let width = self.width.max(1) as usize;
        while self.wrapped_sources < self.lines.len() {
            let line = self.lines[self.wrapped_sources].clone();
            self.starts.push_back(self.wrapped.len());
            for row in wrap_line(&line, width) {
                self.wrapped.push_back(row);
            }
            self.wrapped_sources += 1;
        }
    }

    /// 丢掉最旧的来源行，直到上限重新成立；返回这次丢掉的条数（没丢就是 0）。
    fn evict(&mut self) -> usize {
        let mut dropped = 0;
        while self.lines.len() > CAP {
            if self.wrapped_sources == 0 {
                // 还什么都没折过行，所以这条行没有别的代价。
                self.lines.pop_front();
                dropped += 1;
                continue;
            }
            let height = match self.starts.get(1) {
                Some(next) => next.saturating_sub(self.starts[0]),
                None => self.wrapped.len().saturating_sub(self.starts[0]),
            };
            self.lines.pop_front();
            self.starts.pop_front();
            for _ in 0..height {
                self.wrapped.pop_front();
            }
            self.wrapped_sources -= 1;
            // 剩下每一个起始位置都上移刚走掉的那些行，视口也一样。这每丢一条来源行跑一次
            // —— 转录到达上限之后，最多每个完成的块一次。
            for start in self.starts.iter_mut() {
                *start = start.saturating_sub(height);
            }
            self.top = self.top.saturating_sub(height);
            self.total = self.total.saturating_sub(height);
            self.seen = self.seen.saturating_sub(height);
            self.top_source = self.top_source.saturating_sub(1);
            dropped += 1;
        }
        dropped
    }

    /// 记下视口顶端落在哪条来源行里，供下一次重新折行用。
    fn sync_top_source(&mut self) {
        self.top_source = match self.starts.binary_search(&self.top) {
            Ok(exact) => exact,
            Err(insert) => insert.saturating_sub(1),
        };
    }

    /// 从 `top` 起 `height` 个显示行，先取来源行，然后取实时尾巴。
    fn window(&self, height: usize, live: &[Line<'static>]) -> Vec<Line<'static>> {
        let mut rows = Vec::new();
        for index in self.top..self.total {
            if rows.len() == height {
                break;
            }
            let line = if index < self.wrapped.len() {
                self.wrapped.get(index)
            } else {
                live.get(index - self.wrapped.len())
            };
            match line {
                Some(line) => rows.push(line.clone()),
                None => break,
            }
        }
        rows
    }
}

impl Default for Pane {
    fn default() -> Self {
        Self::new()
    }
}

/// 一个显示行下标按高度差平移；夹在零以上 —— 负的显示行不存在。
fn shift(offset: usize, delta: isize) -> usize {
    let moved = offset as isize + delta;
    if moved < 0 { 0 } else { moved as usize }
}

/// `text` 在 `width` 列下的显示行：每条逻辑行一个，各自按显示列折行。
pub fn wrap_text(text: &str, width: usize) -> Vec<Line<'static>> {
    if text.is_empty() {
        // 完全没有尾巴就是完全没有行。切开会给出一行空的，而每帧底部多出一行空白是窗格
        // 并没有的一行。
        return Vec::new();
    }
    text.split('\n')
        .flat_map(|raw| wrap_line(&Line::from(raw.to_owned()), width))
        .collect()
}

/// 把一条带样式的行折到 `width` 列，保住每一片的样式。
///
/// **数的是列，不是字节。** 一个 CJK 字符是三个字节、两列，所以按字节数会把一行中文折
/// 到窗格宽度的三分之一左右 —— 那是中文看起来唯一坏掉的地方，尽管每一格都是对的。
///
/// 按字符而不是按字素簇：偏移必须能还原，而一个字素簇占的格子不归我们切。一个宽字符独自
/// 待在一列宽的窗格里照样溢出；折行做不了更好，它只需要一直往前走。
///
/// 续行从第零列起 —— 窗格是一份日志，缩进会主张一种折行后的文字并没有的结构。
/// Markdown 渲染器的表格单元格复用它（那里的续行同样从第零列起）。
///
/// **一处有意例外**：左栏的 `todo` 页把续行**缩进到内容列**（`.scratch/todo-page/spec.md` §3）。
/// 那一页不是日志 —— 缩进主张的是「这一片属于哪一项」，而那正是折行要保住的东西。
pub(crate) fn wrap_line(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for span in &line.spans {
        for ch in span.content.chars() {
            let columns = char_columns(ch);
            if used + columns > width && used > 0 {
                out.push(finish(line, std::mem::take(&mut spans)));
                used = 0;
            }
            push_char(&mut spans, ch, span.style);
            used += columns;
        }
    }
    if !spans.is_empty() || out.is_empty() {
        out.push(finish(line, spans));
    }
    out
}

/// 一个折出来的行，继承那一行自己的样式与对齐。
fn finish(template: &Line<'static>, spans: Vec<Span<'static>>) -> Line<'static> {
    Line {
        spans,
        style: template.style,
        alignment: template.alignment,
    }
}

/// 追加一个字符，样式相同时延长最后一个 span，好让一个折出来的行每一段样式一个 span，
/// 而不是每个字符一个。
pub(crate) fn push_char(spans: &mut Vec<Span<'static>>, ch: char, style: Style) {
    match spans.last_mut() {
        Some(last) if last.style == style => last.content.to_mut().push(ch),
        _ => spans.push(Span::styled(ch.to_string(), style)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 推过上限之后，`push` 报的数是它真丢掉的那些源行 —— 绘制侧的平行表就靠这个数跟窗格
    /// 同进同出（[`super::CAP`]，`.scratch/trace-tab/issues/07-pane-evict-accounting.md`）。
    #[test]
    fn push_reports_the_source_lines_the_cap_dropped() {
        let mut pane = Pane::new();
        let mut dropped = 0;
        for _ in 0..CAP + 2 {
            dropped += pane.push(Line::from("x"));
        }
        assert_eq!(dropped, 2, "推过上限两条，就该报两条");
        assert_eq!(pane.sources(), CAP, "报的条数与真留下的条数对得上");
    }

    /// 记账是按**这一次丢了几条**回答的，不是一个 0/1 的开关：一次性超限三条就得报三条。
    /// `push` 一次只加一行，所以这里直接把它放到上限之上，逼出那条一次丢多条的路径。
    #[test]
    fn evicting_can_drop_several_source_lines_at_once() {
        let mut pane = Pane::new();
        for _ in 0..CAP + 3 {
            pane.lines.push_back(Line::from("x"));
        }
        assert_eq!(pane.evict(), 3);
        assert_eq!(pane.sources(), CAP);
    }

    /// `replace_at` 改的是**指定那一条**，其余行与它们的顺序一个字都不动。
    ///
    /// 这是它存在的理由：单位组头随回合推进而增长，而它画出去的时候后面已经有行了
    /// （`.scratch/trace-ledger/spec.md` §5）。
    #[test]
    fn replacing_a_middle_line_leaves_the_others_alone() {
        let mut pane = Pane::new();
        for text in ["第一行", "第二行", "第三行", "第四行"] {
            pane.push(Line::from(text));
        }
        pane.view(20, 10, &[]);
        pane.replace_at(1, Line::from("换过的第二行"));
        let rows = pane.view(20, 10, &[]);
        let text: Vec<String> = rows.iter().map(|l| l.to_string()).collect();
        assert_eq!(text[0], "第一行");
        assert_eq!(text[1], "换过的第二行");
        assert_eq!(text[2], "第三行");
        assert_eq!(text[3], "第四行");
        assert_eq!(pane.sources(), 4, "改写不动来源行的条数");
    }

    /// 替换一条**不存在的**行是空操作，而**不是**改最后一行：那是调用方算错了下标，
    /// 静默改掉另一条内容更糟。
    #[test]
    fn replacing_a_line_that_is_not_there_does_nothing() {
        let mut pane = Pane::new();
        pane.push(Line::from("唯一一行"));
        pane.view(20, 10, &[]);
        pane.replace_at(7, Line::from("不该出现的行"));
        let rows = pane.view(20, 10, &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].to_string(), "唯一一行");
    }

    /// 空窗格上调用是安全的 —— 与 `replace_last` 同一个答案：没有东西可改写。
    #[test]
    fn replacing_on_an_empty_pane_does_nothing() {
        let mut pane = Pane::new();
        pane.replace_at(0, Line::from("x"));
        assert_eq!(pane.sources(), 0);
    }

    /// 折行高度变了（一条短的换成一条长的）之后，后面那些行的起点与视口都要跟着走 ——
    /// 读者看到的那一行不动。这是「就地改写」的全部意思。
    #[test]
    fn replacing_a_line_that_grows_shifts_the_rest_without_moving_the_viewport() {
        let mut pane = Pane::new();
        for text in ["短", "第二条", "第三条", "第四条"] {
            pane.push(Line::from(text));
        }
        // 宽度 18：只有被替换的那一条会折行（10 个汉字 20 列），其余三条各自一行。
        pane.view(18, 20, &[]);
        let before = pane.total();
        assert_eq!(before, 4);
        pane.replace_at(0, Line::from("换长了的一行很长很长"));
        let rows = pane.view(18, 20, &[]);
        let text: Vec<String> = rows.iter().map(|line| line.to_string()).collect();

        assert_eq!(rows.len(), before + 1, "多折出来的那一行把总数顶上去");
        assert!(
            text[0].starts_with('换'),
            "第一条是折出来的第一段：{:?}",
            text[0]
        );
        assert_eq!(text[2], "第二条", "后面三条整体下移一行，内容一字不改");
        assert_eq!(text[3], "第三条");
        assert_eq!(text[4], "第四条");
    }
}
