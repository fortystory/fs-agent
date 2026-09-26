//! 会话的读数：它是什么，以及它花了多少（`.scratch/tui-sidebar/spec.md` §3）。
//!
//! 读数现在是左栏的一页 —— 页签条上的调用量 —— 而其中一项，上下文占比，同时也是状态
//! 行的（`wording::status_row`）。这里画的东西是注入的事实与本模块从流上数出来的计数器
//! 的**纯函数**。帧与帧之间除计数本身之外什么都不记，所以没有第二本账会与
//! [`crate::events::total_usage`] 漂移 —— 下面这些求和与那个函数在一份完成的流上做的
//! 是同一组求和。
//!
//! 这一页到底画不画、它的几行放得下，由几何说了算（`layout::Regions::sidebar_page`，
//! 它的高度**就是**那个数）。一行的样子来自 prototype：一条与最宽标签同宽的标签列、一个
//! 空格，然后是一个数字靠右、文字靠左填进去的值。丢东西由宽度与高度决定：百分比与缓存行
//! 按宽度走，尾部那些行按高度走（它们排在最后，段落把它们裁掉）。两条按宽度丢的路从外壳
//! 自己那几档都到不了 —— 窄档下 21 列正好是最宽的那个值需要的 —— 留着它们，只是它们一
//! 直以来的那层保险。

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::events::Usage;

use super::transcript::Block;
use super::tui::SessionFacts;
use super::width::{text_columns, truncate_columns};
use super::wording;

/// 这个面板从流上数出来的东西。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    /// 每一条 `UsageRecorded`，用 [`Usage::accumulate`] 折叠 —— 与 `events::total_usage`
    /// 在一份完成的流上做的是同一个折叠，所以这个面板不会因为自己重推一遍算术而与会话
    /// 的账目漂移。
    total: Usage,
    /// 最近一次调用的输入 token：下一次请求会带走的东西。
    last_input: Option<u64>,
    turns: u64,
}

impl Panel {
    pub fn new() -> Self {
        Self::default()
    }

    /// 收下这个块贡献的东西，如果有的话。
    ///
    /// `cached` 与 `miss` 是对 `input` 的一个**拆分**，而供应商已经把推理 token 数在
    /// `output` 里了，所以哪个都不能再往上加 —— 与 `Usage::total_tokens` 和
    /// `events::total_usage` 是同一套算法（spec §8）。
    pub fn observe(&mut self, block: &Block) {
        match block {
            Block::Usage { usage, .. } => {
                self.total.accumulate(*usage);
                self.last_input = Some(usage.input_tokens);
            }
            Block::TurnEnded { .. } => self.turns = self.turns.saturating_add(1),
            _ => {}
        }
    }

    /// 最近一次调用的输入 token：状态行的 `上下文 n%` 是它的占比，左栏那个上下文字段拿
    /// 它与上限配成一对。
    pub fn last_input(&self) -> Option<u64> {
        self.last_input
    }

    /// 在 `area` 里要画的行，最重要的在前。
    pub fn lines(&self, facts: &SessionFacts, area: Rect) -> Vec<Line<'static>> {
        let value_columns = (area.width as usize).saturating_sub(label_columns() + 1);

        let context = match self.last_input {
            Some(used) => {
                // 宽度第一个拿走的就是百分比。
                let share = wording::context_pair(Some(used), facts.context_window, true);
                if text_columns(&share) <= value_columns {
                    share
                } else {
                    wording::context_pair(Some(used), facts.context_window, false)
                }
            }
            None => wording::context_pair(None, facts.context_window, false),
        };
        let tokens = wording::token_pair(self.total.total_tokens(), facts.budget_limit);
        let cache = wording::cache_pair(self.total.cached_tokens, self.total.miss_tokens);

        // 没有 `模型` 行：它搬到了状态行，在那里无论左栏在显示哪一页、无论左栏多宽都
        // 看得见（spec §3）。
        let mut rows: Vec<(&'static str, String, bool)> = vec![
            (wording::PANEL_CONTEXT, context, true),
            (wording::PANEL_TOKENS, tokens, true),
            (wording::PANEL_TURNS, wording::thousands(self.turns), true),
        ];
        // 按重要性倒着丢，所以它们加在最后：下面的高度裁剪从末尾把它们拿走。这些行刻意
        // 没有*宽度*下限 —— 面板只有内容至少 23 列时才画，所以一个值永远不少于 16 列，
        // 更宽的下限根本到不了。还是超出的值在上面被适配过。
        rows.push((
            wording::PANEL_INPUT,
            wording::thousands(self.total.input_tokens),
            true,
        ));
        rows.push((
            wording::PANEL_OUTPUT,
            wording::thousands(self.total.output_tokens),
            true,
        ));
        if text_columns(&cache) <= value_columns {
            rows.push((wording::PANEL_CACHE, cache, true));
        }
        // 高度在这里不用裁：这些行按重要性排好了，段落会把塞不进面板内容区的东西裁掉，
        // 所以走掉的正是最后那些行。
        rows.iter()
            .map(|(label, value, right)| row(label, value, value_columns, *right))
            .collect()
    }
}

/// 标签列：与最宽的标签同宽，这样没有标签会被裁，这一列也不会与自己装的那些词脱节。
fn label_columns() -> usize {
    [
        wording::PANEL_CONTEXT,
        wording::PANEL_TOKENS,
        wording::PANEL_TURNS,
        wording::PANEL_INPUT,
        wording::PANEL_OUTPUT,
        wording::PANEL_CACHE,
    ]
    .iter()
    .map(|label| text_columns(label))
    .max()
    .unwrap_or(0)
}

/// 面板的一行：标签列里一个暗标签，然后是填满剩余空间的值 —— 数字靠右，文字靠左。
fn row(label: &str, value: &str, width: usize, right: bool) -> Line<'static> {
    let labels = label_columns();
    let label = pad_right(&fit(label, labels), labels);
    let value = fit(value, width);
    let value = if right {
        pad_left(&value, width)
    } else {
        pad_right(&value, width)
    };
    Line::from(vec![
        Span::styled(label, Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::raw(value),
    ])
}

/// 把 `text` 适配到 `width` 列。
///
/// 千位分隔符先去 —— 它们是装饰，一个不带分隔符放得下的数字，比一个带着就放不下的数字
/// 更值钱。还是放不下的用省略号裁掉，所以一个短数字是*看得见地*短，而不是悄悄错了。
fn fit(text: &str, width: usize) -> String {
    if text_columns(text) <= width {
        return text.to_owned();
    }
    let bare = text.replace(',', "");
    if text_columns(&bare) <= width {
        return bare;
    }
    let mut cut = truncate_columns(&bare, width.saturating_sub(1));
    cut.push('…');
    cut
}

fn pad_right(text: &str, width: usize) -> String {
    let mut out = text.to_owned();
    for _ in text_columns(text)..width {
        out.push(' ');
    }
    out
}

fn pad_left(text: &str, width: usize) -> String {
    let used = text_columns(text);
    if used >= width {
        return text.to_owned();
    }
    let mut out = " ".repeat(width - used);
    out.push_str(text);
    out
}
