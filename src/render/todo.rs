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
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::events::SpeakerId;
use crate::tools::todo::{Item, Status, TODO_TOOL, completed, read_items};

use super::palette;
use super::pane;
use super::transcript::Block;
use super::width::{text_columns, truncate_columns};
use super::wording;

/// `todo` 页签显示什么，以及它到底给不给。
#[derive(Debug, Default)]
pub struct TodoPanel {
    /// 主会话有没有提交过一份非空列表。置上就不再清：它就是这页签有没有的全部条件。
    seen: bool,
    /// 当时生效的列表，来自最近一次主会话调用。空在这里是一个真实状态（列表被清空了），
    /// 它不清 `seen`。
    items: Vec<Item>,
    /// 上一帧真的画出来的那块矩形。指针只回应它 —— 与文件页 / 改动页同一条纪律
    /// （`.scratch/todo-page/spec.md` §5）。
    rect: Option<Rect>,
    /// 当前生效的那份列表**是谁提交的**。讨论会话里各方各有一份，而页上是最后一个落地
    /// 落地者的那份 —— 弹窗标题要说清这一点（§1、§6）。
    speaker: Option<SpeakerId>,
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
        self.speaker = Some(tool.speaker.clone());
    }

    /// 页签条上有没有 `todo` 这个标签。
    pub fn visible(&self) -> bool {
        self.seen
    }

    /// 这一页从上到下的行，给这个高度的页区。
    ///
    /// 它是**摘要**：第一行一条进度行（做完几条 / 共几条 / 有几条在做），下面才是条目 ——
    /// 做完的折掉，「还有什么没做」于是是连续的一块，正在做的那条排在最前。**不滚动**：
    /// 装不下时末行报「还有几条一条都没露过面」，而那条被 `…` 截断、至少露过一行的不算在里面。
    pub fn lines(&self, area: Rect) -> Vec<Line<'static>> {
        self.lines_with_folds(area).0
    }

    /// 这一页从上到下的行，**外加哪些行是折出来的续行**。
    ///
    /// 那张平行表是给屏幕文本层的：它登记的是**画出来的行**，不标出续行的话，一条折行的
    /// 待办复制出来会多出几个换行（`.scratch/todo-page/spec.md` §8）。
    pub fn lines_with_folds(&self, area: Rect) -> (Vec<Line<'static>>, Vec<bool>) {
        let room = area.height as usize;
        if room == 0 {
            return (Vec::new(), Vec::new());
        }
        let width = area.width as usize;
        // 一份被清空的列表：页上是一句实话，而不是一块空白 —— 空白分不清「没有」与「坏了」。
        if self.items.is_empty() {
            return (vec![empty_line()], vec![false]);
        }
        let mut rows = vec![self.progress_line(width)];
        let mut folded = vec![false];
        // 一行高的页区就是进度行本身 —— 与今天那条「计数行先留出来」的规矩同源，只是搬到了顶上。
        if room == 1 {
            return (rows, folded);
        }

        let open: Vec<&Item> = self.open().collect();
        let body: Vec<ItemRows> = open.iter().map(|item| item_rows(item, width)).collect();
        let total: usize = body.iter().map(|rows| rows.lines.len()).sum();
        if rows.len() + total <= room {
            for part in &body {
                folded.extend_from_slice(&part.folded);
                rows.extend(part.lines.clone());
            }
            return (rows, folded);
        }

        // 装不下，于是把最后一行留给「还有几条没露过面」。
        let mut budget = room - rows.len() - 1;
        let mut hidden = 0;
        for (index, part) in body.iter().enumerate() {
            if budget == 0 {
                hidden = body.len() - index;
                break;
            }
            if part.lines.len() <= budget {
                rows.extend(part.lines.clone());
                folded.extend_from_slice(&part.folded);
                budget -= part.lines.len();
                continue;
            }
            // 这一条露它的前几行、末行加记号。它露过面，所以不算进那个数字。
            let mut shown = part.lines[..budget].to_vec();
            if let Some(last) = shown.last_mut() {
                // 记号要**在预算里**：那一行已经占满页宽，直接拼上去会被框边裁掉，
                // 于是读者以为这一条没有被截断（半角内容最容易撞上这个）。
                *last = Line::from(truncate_columns(
                    &format!("{last}{}", wording::ELLIPSIS),
                    width,
                ));
            }
            rows.extend(shown);
            folded.extend(part.folded[..budget].iter().copied());
            hidden = body.len() - index - 1;
            break;
        }
        if hidden > 0 {
            rows.push(Line::from(truncate_columns(
                &wording::todo_overflow(hidden),
                width,
            )));
            folded.push(false);
        }
        (rows, folded)
    }

    /// 进度行，右端留着那一块按钮。
    fn progress_line(&self, width: usize) -> Line<'static> {
        let head = wording::todo_progress(completed(&self.items), self.items.len(), self.busy());
        let button = wording::TODO_BUTTON;
        let gap = width.saturating_sub(text_columns(&head) + text_columns(button));
        Line::from(format!("{head}{}{button}", " ".repeat(gap.max(1))))
    }

    /// 这一格是不是落在 `todo` 页的页区里 —— **整页**可点，所以只要落在这块矩形里就行
    /// （这一页没有焦点行，也不需要「屏幕行 ↔ 逻辑项」的换算）。
    pub fn page_contains(&self, point: (u16, u16)) -> bool {
        self.rect.is_some_and(|page| page.contains(point.into()))
    }

    /// 记下上一帧真的画出来的那块矩形，由画它的那一处调。
    pub fn set_rect(&mut self, area: Rect) {
        self.rect = Some(area);
    }

    /// 这一帧没画这一页：指针于是不该再落在一样东西上 —— 与文件页 / 改动页同一条
    /// 「只认真画出来的东西」。
    pub fn clear_rect(&mut self) {
        self.rect = None;
    }

    /// 当前生效的那份列表（弹窗的快照就是它的一份拷贝）。
    pub fn all(&self) -> &[Item] {
        &self.items
    }

    /// 当前生效的那份列表是谁提交的（`None` = 还没见过任何列表）。
    pub fn speaker(&self) -> Option<&SpeakerId> {
        self.speaker.as_ref()
    }

    /// 列表是不是空的（空着就不给那个按钮，开不出一个空弹窗）。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 页上要画的那几项：**未完成**的，`in_progress` 的按提交顺序排在最前，其余保持原序。
    fn open(&self) -> impl Iterator<Item = &Item> {
        let (busy, waiting) = split(&self.items);
        busy.into_iter().chain(waiting)
    }

    /// 有几条标着在做。
    fn busy(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.status == Status::InProgress)
            .count()
    }
}

/// 弹窗正文：进度行 → 空行 → 未完成项（正在做置顶）→ 空行 → 已完成项（**静音档**）。
///
/// 不加「已完成」小标题 —— 标题行已经说过一次（`.scratch/todo-page/spec.md` §6）。
pub fn detail_rows(items: &[Item], width: usize) -> Vec<Line<'static>> {
    if items.is_empty() {
        return vec![empty_line()];
    }
    let (busy, waiting) = split(items);
    let busy_count = busy.len();
    let mut rows = vec![
        Line::from(wording::todo_progress(
            completed(items),
            items.len(),
            busy_count,
        )),
        Line::from(""),
    ];
    for item in busy.iter().chain(waiting.iter()) {
        rows.extend(item_lines(item, width, false));
    }
    let done: Vec<&Item> = items
        .iter()
        .filter(|item| item.status == Status::Completed)
        .collect();
    if !done.is_empty() {
        rows.push(Line::from(""));
        for item in done {
            rows.extend(item_lines(item, width, true));
        }
    }
    rows
}

/// 「还没有待办」那一行 —— 页与弹窗共用它，所以两处说的是同一句话。
fn empty_line() -> Line<'static> {
    Line::from(Span::styled(
        wording::TODO_EMPTY,
        Style::default().fg(palette::MUTED),
    ))
}

/// 一份列表按「在做的 / 还在等的 / 做完的」三档分开：页与弹窗共用这一个顺序口径。
fn split(items: &[Item]) -> (Vec<&Item>, Vec<&Item>) {
    let (busy, waiting): (Vec<_>, Vec<_>) = items
        .iter()
        .filter(|item| item.status != Status::Completed)
        .partition(|item| item.status == Status::InProgress);
    (busy, waiting)
}

/// 一个条目占的那些行，外加「哪些是折出来的续行」。
struct ItemRows {
    lines: Vec<Line<'static>>,
    folded: Vec<bool>,
}

/// 一个条目占的那几行：状态字形、可选的目标条目 id，以及**折出来**的内容。
///
/// 前缀补满成五列 —— 字形一列、一格空格、id 两列（没有 id 就两个空格占位）、一格空格 ——
/// 于是带 id 与不带 id 的两行内容严格同列，与文件页「字形列每行都占满两格」同族。
///
/// 折行**缩进到内容列**，那是对 `pane::wrap_line` 那条纪律（「续行从第零列起 —— 缩进会主张
/// 一种折行后的文字并没有的结构」）的一次**有意例外**：窗格是一份日志，而这一页不是；这里的
/// 缩进主张的是「这一片属于哪一项」，那是真结构。
fn item_rows(item: &Item, width: usize) -> ItemRows {
    let head = head_of(item);
    let indent = " ".repeat(text_columns(&head));
    let budget = width.saturating_sub(text_columns(&head)).max(1);
    let parts: Vec<String> = pane::wrap_text(&item.content, budget)
        .iter()
        .map(|line| line.to_string())
        .collect();
    let mut lines: Vec<Line<'static>> = parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            if index == 0 {
                Line::from(format!("{head}{part}"))
            } else {
                Line::from(format!("{indent}{part}"))
            }
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::from(head));
    }
    let folded = (0..lines.len()).map(|index| index > 0).collect();
    ItemRows { lines, folded }
}

/// 一个条目的那几行，`muted` 是弹窗里「已做完」那一档（页上做完了就不画，所以那里用不到）。
fn item_lines(item: &Item, width: usize, muted: bool) -> Vec<Line<'static>> {
    let rows = item_rows(item, width);
    if !muted {
        return rows.lines;
    }
    let style = Style::default().fg(palette::MUTED);
    rows.lines
        .into_iter()
        .map(|line| {
            Line::from(
                line.spans
                    .into_iter()
                    .map(|span| Span::styled(span.content, style))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

/// 一个条目前缀：状态字形 + 空格 + 目标条目 id（没有就两个空格占位）+ 空格。
fn head_of(item: &Item) -> String {
    format!(
        "{} {} ",
        wording::todo_glyph(item.status),
        item.id.as_deref().unwrap_or("  ")
    )
}
