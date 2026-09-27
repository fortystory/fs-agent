//! 输入编辑器：一份多行草稿、它的键位表，以及它的光标在哪儿。
//!
//! 光标唯一的状态是草稿里的一个**字符下标** —— 从不是行、从不是列、从不是字节偏移。
//! 显示行只在画一帧的时候存在，每次都从这个下标推出来。就是这样才让一次改尺寸、一次重新
//! 折行或一个宽字符，不会把光标留在用户没放过它的地方：内联视口那个失败模式（ADR 0002）。
//!
//! 编辑是**按行感知**的。Emacs 的 Ctrl 组合键与 `Home`/`End` 作用在光标所在的那条逻辑行
//! 上，所以一份多行草稿不会被一个 `Ctrl-U` 吃掉好几行，而 `↑`/`↓` 移动光标而不是翻历史
//! —— 翻历史只有 `Ctrl-P` / `Ctrl-N` 两个键（spec §6）。

use ratatui::text::{Line, Span};

use super::width::{char_columns, text_columns};

/// 草稿第一行上的提示符。
///
/// 用 `❱`（U+2771）而不是 `>`：编辑器待在那里时，画家给这个字形的颜色会绕着色相轮走
/// （`.scratch/tui-input-pulse/spec.md` §2b），而在这个字重下，尖括号是读起来像箭头的那个
/// 形状。在这个渲染器的宽度表里它算**一列**，所以提示符正好与 `> ` 一样是两列宽，
/// [`prompt_columns`] 下游的一切都不动。一个配成把模糊宽度字符画成双倍的终端会把它显示成
/// 两列，这一点记成一条手工检查，而不是在这里去防。
pub const PROMPT: &str = "❱ ";

/// 提示符占的列数 —— 也因此是第一行之后每一行都带的缩进，好让每一行装同样多的文字。从
/// [`PROMPT`] 推出来，好让两者不会脱节；布局留出的就是这个数。
pub fn prompt_columns() -> u16 {
    text_columns(PROMPT) as u16
}

/// 草稿里逻辑行之间的分隔符。
const NEWLINE: char = '\n';

/// 光标落在一帧画出来的那些行里的哪儿。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub row: u16,
    pub column: u16,
}

/// 光标所在的那个 `/` 记号：一个正在被打的斜杠命令。
///
/// 它**只是第一行的开头**，别的都不是。循环就是在那里找命令，所以那也是菜单唯一可以提供
/// 命令的地方：一个在后面的行里的 `/`，或者在起头的那个空格之后的 `/`，都是一个提示里的
/// 普通字符，补全它会覆盖掉用户本来想写的东西。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashToken {
    /// `/` 所在的字符下标。
    pub start: usize,
    /// 名字刚过一格的字符下标：斜杠之后的第一个空白，或者草稿末尾。补全会替换掉这一整段，
    /// 因为光标*之后*那部分名字仍然是正在打的东西的一部分。
    pub end: usize,
    /// 斜杠之后一直打到光标为止的东西。
    pub prefix: String,
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
    /// 一行的引子 —— 第一行的提示符、其余行的缩进 —— 是它**自己的 span**，这样画家有地方
    /// 放提示符的颜色，而不必伸手进草稿的正文里（`.scratch/tui-input-pulse/spec.md` §2b）。
    /// 字符一个没变，所以编辑器自己的测试像人那样读这一行：把 span 拼起来。
    pub fn view(&self, width: u16, height: u16) -> (Vec<Line<'static>>, Placed) {
        let (rows, placed) = self.display_rows(width.max(1) as usize);
        let height = (height.max(1)) as usize;
        let top = if placed.row as usize >= height {
            placed.row as usize + 1 - height
        } else {
            0
        };
        let indent = " ".repeat(prompt_columns() as usize);
        let lines = rows
            .iter()
            .enumerate()
            .skip(top)
            .take(height)
            .map(|(index, row)| {
                let lead = if index == 0 { PROMPT } else { indent.as_str() };
                Line::from(vec![
                    Span::raw(lead.to_owned()),
                    Span::raw(row.text.clone()),
                ])
            })
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
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.remove_range(self.cursor - 1, self.cursor);
    }

    /// 删掉光标后的一个字符；在行尾时它把下一行拉上来。
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.len() {
            return;
        }
        self.remove_range(self.cursor, self.cursor + 1);
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
        self.cursor_moved();
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
        self.cursor_moved();
    }

    /// 光标所在那条**行**的开头，不是草稿的开头。
    pub fn home(&mut self) {
        self.cursor = self.line_bounds().0;
        self.cursor_moved();
    }

    /// 光标所在那条**行**的末尾，不是草稿的末尾。
    pub fn end(&mut self) {
        self.cursor = self.line_bounds().1;
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

    /// `Ctrl-W`：先丢光标前的尾部空格，再丢一段非空格 —— shell 的抹词，但关在这一行之内。
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

    // --- 斜杠记号 ----------------------------------------------------------

    /// 光标所在的那个 `/` 记号，如果它在里面的话。
    ///
    /// 记号必须从草稿最开头起，而且还没碰到空格：`/ask-matt` 是一个记号，
    /// `/ask-matt 优化这个` 是一个带任务的命令，`看看 /tmp/x` 是一条路径。只看第一行，因为
    /// 循环只在第一行找命令。
    pub fn slash_token(&self) -> Option<SlashToken> {
        let (line_start, _) = self.line_bounds();
        if line_start != 0 || !self.text.starts_with('/') {
            return None;
        }
        // 名字一直延到出现的第一个空白 —— 换行结束这一行，空格开始任务 —— 或者到草稿末尾。
        let end = self
            .text
            .chars()
            .position(char::is_whitespace)
            .unwrap_or_else(|| self.len());
        // 光标越过了名字，就是落在任务里，那里没有东西可补全。
        if self.cursor > end {
            return None;
        }
        Some(SlashToken {
            start: 0,
            end,
            prefix: self
                .text
                .chars()
                .skip(1)
                .take(self.cursor.saturating_sub(1))
                .collect(),
        })
    }

    /// 把 `/` 记号替换成 `/<name>`，光标留在它后面。
    ///
    /// 光标不在一个记号里时返回 `false` —— 什么都不改 —— 于是一份过时的菜单永远写不进
    /// 用户已经走开的草稿。
    pub fn complete_slash(&mut self, name: &str) -> bool {
        let Some(token) = self.slash_token() else {
            return false;
        };
        self.remove_range(token.start, token.end);
        self.cursor = token.start;
        self.insert_str(&format!("/{name}"));
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
        let prompt = prompt_columns() as usize;
        let column = prompt
            + rows[at]
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
                column: column.min(prompt + width - 1) as u16,
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
