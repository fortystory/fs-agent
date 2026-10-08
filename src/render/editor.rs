//! 输入编辑器：一份多行草稿、它的键位表，以及它的光标在哪儿。
//!
//! 光标唯一的状态是草稿里的一个**字符下标** —— 从不是行、从不是列、从不是字节偏移。
//! 显示行只在画一帧的时候存在，每次都从这个下标推出来。就是这样才让一次改尺寸、一次重新
//! 折行或一个宽字符，不会把光标留在用户没放过它的地方：内联视口那个失败模式（ADR 0002）。
//!
//! 编辑是**按行感知**的。Emacs 的 Ctrl 组合键与 `Home`/`End` 作用在光标所在的那条逻辑行
//! 上，所以一份多行草稿不会被一个 `Ctrl-U` 吃掉好几行，而 `↑`/`↓` 移动光标而不是翻历史
//! —— 翻历史只有 `Ctrl-P` / `Ctrl-N` 两个键（spec §6）。

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::token::{self, Token};
use super::width::char_columns;

/// 草稿**没有提示符**：第一行第一个字就是草稿的第一个字，折行之后每一行也顶格
/// （2026-10-08 维护者点掉了原来那个 `❱ `，`.scratch/ui-trim/spec.md`）。
///
/// 它原来占两列，承担两件事：一个「键在这里」的眼色，以及画家给那个 span 上色的位置
/// （`.scratch/tui-input-pulse/spec.md` §2b）。色相那套随它一起退场，于是两件事都不再需要
/// 它：输入行留给文字的宽度就是主列的内容宽度。
const NEWLINE: char = '\n';

/// 光标落在一帧画出来的那些行里的哪儿。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub row: u16,
    pub column: u16,
}

/// 草稿里一段要上色的字符区间。
///
/// 判据是「能兑现」—— `/` 的记要在命令表里命中、`@` 的记要在文件索引里命中 —— 而
/// [`Input`] 两样都不认识。所以区间由 `TuiState` 算好、同步进来（与它记着的菜单选择同
/// 构），编辑器只做**纯几何**的吸附与整块删。这条缝正是本节的重点：它让编辑器继续不认识
/// 命令表与文件系统（`.scratch/input-tokens/spec.md` §4）。
///
/// 样式也一并从外面进来，所以「命令蓝、引用紫」那对常量不散写在这里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSpan {
    /// 区间起始的字符下标。
    pub start: usize,
    /// 刚过一格的字符下标。
    pub end: usize,
    pub style: Style,
}

/// 草稿的一个显示行。
struct Row {
    text: String,
    /// 这一行起始处的字符下标。
    start: usize,
    /// 这一行最后一个字符刚过一格的字符下标。
    end: usize,
}

/// 用户打进去的东西，以及光标在其中的哪里。
pub struct Input {
    text: String,
    /// 光标，作为 `text` 的一个**字符**下标。见模块文档。
    cursor: usize,
    /// 已经提交过的草稿，最旧的在前，供 `Ctrl-P` / `Ctrl-N` 用。
    history: Vec<String>,
    /// 此刻翻到 [`Input::history`] 的哪里。
    history_at: Option<usize>,
    /// 开始翻历史时先收起来的那份新草稿。
    draft: String,
    /// `↑`/`↓` 想保住的视觉列，直到别的什么东西挪动了光标。
    goal: Option<usize>,
    /// 能兑现的那些记号区间，由 `TuiState` 同步进来。
    ///
    /// 它们是**不可分割的一块**：光标吸附到边界上，删除命中时整块删。区间为空时编辑器完全
    /// 是今天那个纯文本模型。
    spans: Vec<TokenSpan>,
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
            spans: Vec::new(),
        }
    }

    /// 草稿，按提交出去的样子。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 草稿是不是跨了不止一条逻辑行。
    pub fn has_multiple_lines(&self) -> bool {
        self.text.contains(NEWLINE)
    }

    /// 在 `width` 个文字列下，草稿有几个显示行高。布局在知道别的任何事之前先问这个，因为
    /// 给输入区定尺寸的正是它。
    pub fn height(&self, width: u16) -> u16 {
        self.display_rows(width.max(1) as usize).0.len() as u16
    }

    /// 要画的行，以及光标落在它们中的哪儿。
    ///
    /// 滚动的目的是把光标那一行留在 `height` 之内：草稿长到它十行的上限然后开始滚，而不是
    /// 把正在打的东西藏起来（spec §5）。
    ///
    /// 每一行都是草稿自己的字，前后不带任何前缀 —— 原来第一行那个 `❱ ` 与其余行的等宽缩进
    /// 一起退场了（`.scratch/ui-trim/spec.md`）。
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
            .skip(top)
            .take(height)
            .map(|row| Line::from(self.row_spans(row)))
            .collect();
        (
            lines,
            Placed {
                row: placed.row - top as u16,
                column: placed.column,
            },
        )
    }

    // --- 编辑 --------------------------------------------------------------

    pub fn insert_char(&mut self, ch: char) {
        let at = self.byte_at(self.cursor);
        self.text.insert(at, ch);
        self.cursor += 1;
        self.text_edited();
    }

    /// 在光标处插入一段文字，算一次编辑。
    ///
    /// 这是粘贴那条路：这段文字可能自带换行，而它绝不能提交（spec §7）。
    pub fn insert_str(&mut self, text: &str) {
        let at = self.byte_at(self.cursor);
        self.text.insert_str(at, text);
        self.cursor += text.chars().count();
        self.text_edited();
    }

    /// 删掉光标前的一个字符；在行首时它把这一行接到上一行。
    ///
    /// 命中一个记号时**整块**删，但不额外吞掉旁边的空白。
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let (from, to) = self.expand_to_tokens(self.cursor - 1, self.cursor);
        self.remove_range(from, to);
    }

    /// 删掉光标后的一个字符；在行尾时它把下一行拉上来。
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.len() {
            return;
        }
        let (from, to) = self.expand_to_tokens(self.cursor, self.cursor + 1);
        self.remove_range(from, to);
    }

    pub fn left(&mut self) {
        self.cursor = self.snap_left(self.cursor.saturating_sub(1));
        self.cursor_moved();
    }

    pub fn right(&mut self) {
        self.cursor = self.snap_right((self.cursor + 1).min(self.len()));
        self.cursor_moved();
    }

    /// 光标所在那条**行**的开头，不是草稿的开头。
    pub fn home(&mut self) {
        self.cursor = self.snap_nearest(self.line_bounds().0);
        self.cursor_moved();
    }

    /// 光标所在那条**行**的末尾，不是草稿的末尾。
    pub fn end(&mut self) {
        self.cursor = self.snap_nearest(self.line_bounds().1);
        self.cursor_moved();
    }

    /// 往上移一行，保住光标原来的视觉列。
    pub fn up(&mut self) {
        let (start, _) = self.line_bounds();
        if start == 0 {
            return;
        }
        let want = self.goal.unwrap_or_else(|| self.column());
        self.cursor = start - 1;
        let line = self.line_bounds();
        self.place_at_column(line, want);
        self.cursor = self.snap_nearest(self.cursor);
        self.goal = Some(want);
    }

    /// 往下移一行，保住光标原来的视觉列。
    pub fn down(&mut self) {
        let (_, end) = self.line_bounds();
        if end >= self.len() {
            return;
        }
        let want = self.goal.unwrap_or_else(|| self.column());
        self.cursor = end + 1;
        let line = self.line_bounds();
        self.place_at_column(line, want);
        self.cursor = self.snap_nearest(self.cursor);
        self.goal = Some(want);
    }

    pub fn kill_to_line_start(&mut self) {
        let start = self.line_bounds().0;
        let (from, to) = self.expand_to_tokens(start, self.cursor);
        self.remove_range(from, to);
    }

    pub fn kill_to_line_end(&mut self) {
        let end = self.line_bounds().1;
        let (from, to) = self.expand_to_tokens(self.cursor, end);
        self.remove_range(from, to);
    }

    /// `Ctrl-W`：先丢光标前的尾部空格，再丢一段非空格 —— shell 的抹词，但关在这一行之内。
    ///
    /// 抹到的是一个能兑现的记号时，整块删。
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
        let (from, to) = self.expand_to_tokens(start + word, self.cursor);
        self.remove_range(from, to);
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.history_at = None;
        self.draft.clear();
        self.goal = None;
        self.spans.clear();
    }

    /// 取走草稿去提交：两端裁掉、清空、并记进历史。
    ///
    /// 只裁两端 —— 多行草稿中间的空行是用户写下的东西的一部分（spec §6）。
    pub fn submitted(&mut self) -> String {
        let line = self.text.trim().to_owned();
        self.clear();
        if !line.is_empty() && self.history.last() != Some(&line) {
            self.history.push(line.clone());
        }
        line
    }

    // --- 记号 --------------------------------------------------------------

    /// 光标在草稿里的**字符**下标。
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// 光标后面紧跟着的那个字符，有的话。
    ///
    /// 补全用它决定要不要补一个分隔空格：末尾当然要，而后面已经是一个空白时就**不**补 ——
    /// 那会写出两个连着的空格。
    pub fn next_char(&self) -> Option<char> {
        self.text.chars().nth(self.cursor)
    }

    /// 光标所在的那个记号，如果它落在某个记号里面的话。
    ///
    /// 推导本身是纯函数（[`super::token`]），所以菜单、补全与提交解析看的是同一份判断 ——
    /// 编辑器继续不认识命令表，也不认识文件索引。
    pub fn token(&self) -> Option<Token> {
        token::token_at(self.text(), self.cursor)
    }

    /// 把光标所在的那个记号替换成 `{prefix}{text}`，光标留在它后面。
    ///
    /// 光标不在这个前缀的记号里时返回 `false` —— 什么都不改 —— 于是一份过时的菜单永远
    /// 写不进用户已经走开的草稿。
    pub fn complete_token(&mut self, prefix: char, text: &str) -> bool {
        let Some(token) = self.token() else {
            return false;
        };
        if token.prefix != prefix {
            return false;
        }
        self.remove_range(token.start, token.end);
        self.cursor = token.start;
        self.insert_str(&format!("{prefix}{text}"));
        true
    }

    // --- 历史 --------------------------------------------------------------

    /// `Ctrl-P`：走到更旧的那份草稿，先把新的收起来。
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

    /// `Ctrl-N`：走到更新的那份草稿，或者回到那份新的。
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

    // --- 内部实现 ----------------------------------------------------------

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    // --- 记号（纯几何） ----------------------------------------------------

    /// 换上新的记号区间，由 `TuiState` 在草稿或判据变化时同步进来。
    pub fn set_token_spans(&mut self, spans: Vec<TokenSpan>) {
        self.spans = spans;
    }

    /// 光标往**左**一步的落点：目标落在记号内部就吸附到它的左边界。
    ///
    /// 方向性是必要的：从记号右边界出发的 `←` 必须跨过整块，而不是被「最近边界」按回原处。
    fn snap_left(&self, at: usize) -> usize {
        self.spans
            .iter()
            .find(|span| span.start < at && at < span.end)
            .map(|span| span.start)
            .unwrap_or(at)
    }

    /// 光标往**右**一步的落点：目标落在记号内部就吸附到它的右边界。
    fn snap_right(&self, at: usize) -> usize {
        self.spans
            .iter()
            .find(|span| span.start < at && at < span.end)
            .map(|span| span.end)
            .unwrap_or(at)
    }

    /// 任意落点（`Home`/`End`/上下行）落在记号内部时，吸附到**最近**的那个边界。
    fn snap_nearest(&self, at: usize) -> usize {
        self.spans
            .iter()
            .find(|span| span.start < at && at < span.end)
            .map(|span| {
                if at - span.start <= span.end - at {
                    span.start
                } else {
                    span.end
                }
            })
            .unwrap_or(at)
    }

    /// 把删除范围扩到「碰到哪个记号就整块吃掉哪个」，但**不**吞掉记号旁边的空白。
    fn expand_to_tokens(&self, from: usize, to: usize) -> (usize, usize) {
        let (mut from, mut to) = (from, to);
        for span in &self.spans {
            if span.start < to && from < span.end {
                from = from.min(span.start);
                to = to.max(span.end);
            }
        }
        (from, to)
    }

    /// 一个显示行的正文，按记号区间分段上色。
    ///
    /// 记号可能跨折行，所以分段是按**字符区间**做的，不是按显示行做的 —— 一个记号落在几行
    /// 上，每一行拿到的都是同一份样式。
    fn row_spans(&self, row: &Row) -> Vec<Span<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut text = String::new();
        let mut style = Style::default();
        for (offset, ch) in row.text.chars().enumerate() {
            let next = self.style_at(row.start + offset);
            if next != style && !text.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut text), style));
            }
            style = next;
            text.push(ch);
        }
        if !text.is_empty() {
            spans.push(Span::styled(text, style));
        }
        spans
    }

    /// 字符 `at` 落在哪个记号里，就给它哪个样式；不在任何记号里就是普通文本。
    fn style_at(&self, at: usize) -> Style {
        self.spans
            .iter()
            .find(|span| span.start <= at && at < span.end)
            .map(|span| span.style)
            .unwrap_or_default()
    }

    /// 字符下标 `at` 的字节偏移，夹在末尾上。
    fn byte_at(&self, at: usize) -> usize {
        self.text
            .char_indices()
            .nth(at)
            .map(|(index, _)| index)
            .unwrap_or(self.text.len())
    }

    /// 一次文字编辑：翻历史到此为止（草稿里的东西不再是回想出来的那一条），目标列也是。
    fn text_edited(&mut self) {
        self.history_at = None;
        self.goal = None;
    }

    /// 一次不是 `↑`/`↓` 的光标移动：翻历史活下来 —— 你还在回想出来的那份草稿里 —— 但保住
    /// 的列不再作数。
    fn cursor_moved(&mut self) {
        self.goal = None;
    }

    /// 围住光标所在那一行的字符下标：第一个字符，以及结束它的那个换行。
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

    /// 光标在它那一行之内的视觉列。
    fn column(&self) -> usize {
        let (start, _) = self.line_bounds();
        self.text
            .chars()
            .skip(start)
            .take(self.cursor - start)
            .map(char_columns)
            .sum()
    }

    /// 把光标放到 `line` 上最接近视觉列 `want` 的那个字符，夹在这一行的末尾上。
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
        self.text_edited();
    }

    fn set(&mut self, text: &str) {
        self.text = text.to_owned();
        self.cursor = self.len();
        self.goal = None;
        self.spans.clear();
    }

    /// 折行之后的那些行，以及光标落在它们中的哪儿。
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
            index += 1; // split 吃掉的那个换行
        }

        let cursor = self.cursor.min(self.len());
        let mut at = 0;
        for (index, row) in rows.iter().enumerate() {
            if row.start <= cursor {
                at = index;
            }
        }
        // 光标坐在折行边界上时归从那里开始的那一行；坐在行边界上时归它正在离开的那一行的
        // 末尾。
        if rows[at].end == cursor && rows.get(at + 1).is_some_and(|next| next.start == cursor) {
            at += 1;
        }
        let offset = cursor - rows[at].start;
        let column = rows[at]
            .text
            .chars()
            .take(offset)
            .map(char_columns)
            .sum::<usize>();
        // 光标在一条正好填满的行末尾时，停在它最后一格上，像终端的待决折行那样，而不是另开
        // 一行、把草稿剩下的部分往下推。
        (
            rows,
            Placed {
                row: at as u16,
                column: column.min(width - 1) as u16,
            },
        )
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

/// 归一化粘贴进来的文字：换行统一成 `\n`，制表符变成空格，其余控制字符丢掉。
///
/// crossterm 把一次粘贴原样递过来 —— 不剥 `\r`、不滤控制字符、不限长度（research §6.3）
/// —— 所以清理是我们自己的事。
///
/// 制表符变成四个空格而不是被丢掉：它们带着值得留住的缩进，而制表符在按显示列计数时没有
/// 位置（折行那张表对控制字符直接断言，而不是猜）。
pub fn normalize_paste(text: &str) -> String {
    /// 一个制表符变成什么。
    const TAB: &str = "    ";
    let mut out = String::with_capacity(text.len());
    for ch in text.replace("\r\n", "\n").replace('\r', "\n").chars() {
        match ch {
            NEWLINE => out.push(NEWLINE),
            '\t' => out.push_str(TAB),
            ch if ch.is_control() => {}
            ch => out.push(ch),
        }
    }
    out
}
