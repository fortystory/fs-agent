//! TUI 转录的 Markdown 渲染。
//!
//! 模型用 Markdown 作答；这里把它常见的那一部分变成带样式的 [`Line`]。**解析**交给
//! `pulldown-cmark`（spec §1、[ADR 0008](../../docs/adr/0008-markdown-parsing-by-pulldown-cmark.md)），
//! **渲染** —— 到 ratatui `Line` 的每一处决定：列宽、折行、表头样式、代码块、语言名那一行
//! —— 仍是我们自己的。姿态是「不手写解析器」：它认不出的东西一律原样透传，所以不完美的
//! 输入会降级成纯文本，而不是消失。
//!
//! 调色板是**答案的**：这里没有任何东西被调暗成叙述的灰色。发言前缀与块的续行缩进归
//! 调用方管（spec §5）。
//!
//! `width` 是转录内容的可用列数。表格的列宽与超宽代码行的折行都要知道它，于是**源行不
//! 再宽度无关**：宽度变了要按新宽度重跑这里，而不是只重新折行（spec §1）。

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::highlight::{self, Class};
use super::wording;
use super::{pane, width};

/// 行内代码与围栏块里认不出语言时的底色。
const CODE: Color = Color::Yellow;
/// 静音的结构：引用条、分隔线、链接目标、代码块的语言名。
const MUTED: Color = Color::Gray;
/// 表格的网格线。
const GRID: Color = Color::Gray;
/// 一条主题分隔画多长。
const RULE_WIDTH: usize = 24;
/// 列表每嵌套一层缩进几格（spec §6）。解析器给的是嵌套**结构**，原始缩进已经没有了。
const LIST_INDENT: usize = 2;
/// 代码块内容缩几格（spec §3）。
const CODE_INDENT: usize = 2;
/// 表格格与格之间那条 ` │ ` 占的列数。
const GAP_WIDTH: usize = 3;

/// 把一个 Markdown 块渲染成终端行，`width` 是可用列数。
pub fn to_lines(text: &str, width: u16) -> Vec<Line<'static>> {
    to_lines_indented(text, width, 0)
}

/// 带「首行前缀」的渲染：`indent` 是调用方会在**第一行**前面加的那个前缀占的列数。
///
/// 它带来两件事（spec §1、§5）：
///
/// - **需要左边界对齐的块**（表格、代码块）每一行都从第 `indent` 列起，宽度预算也因此是
///   `width - indent` —— 表头与数据行、语言名与代码行才落在同一个左边界上。调用方给第一
///   行加前缀时，把那 `indent` 个空格**换成**前缀，两边的列数正好对上。
/// - **其余块**（段落、标题、列表、引用）照常从第 0 列吐：调用方把消息的第一行推右
///   `indent` 列，它们的续行顶格、占满整个 `width`。
pub fn to_lines_indented(text: &str, width: u16, indent: u16) -> Vec<Line<'static>> {
    let mut options = Options::empty();
    // 只开这三件。`ENABLE_GFM` 不是它们仨的总开关（实测：只开它时表格仍是段落），
    // 脚注则明确不开（spec §1）。
    options
        .insert(Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH);
    let mut renderer = Renderer::new(width.max(1) as usize, indent as usize);
    for event in Parser::new_ext(text, options) {
        renderer.event(event);
    }
    renderer.finish()
}

/// 一行的行内样式栈上的一项：强调是叠在基础样式上的修饰，不是替换（spec §6）。
struct Frame {
    /// 链接或图片的目标；`End` 时补在后面的那个 ` (url)`。
    url: String,
    /// 标签从当前行的第几片 span 起 —— 图片要把它整段取出来重排。
    start: usize,
}

/// 一个还没定界的围栏块或缩进代码块。
struct CodeBuffer {
    /// info string 的第一个词，保留作者的大小写；`None` 表示没写语言。
    lang: Option<String>,
    text: String,
}

/// 一张还没收齐的表格：列宽必须先知道整张表才能算（spec §2）。
struct TableBuffer {
    alignments: Vec<Alignment>,
    rows: Vec<TableRowBuffer>,
    cells: Vec<Vec<Span<'static>>>,
    current: Vec<Span<'static>>,
    header: bool,
    in_cell: bool,
}

struct TableRowBuffer {
    header: bool,
    cells: Vec<Vec<Span<'static>>>,
}

/// 事件驱动的块级渲染器。
///
/// 表格与代码块要**整块缓冲**（前者算列宽、后者定界），其余块逐行吐出。这就是它是一台
/// 状态机、而不是一趟 `split('\n')` 的原因（spec §1）。
struct Renderer {
    width: usize,
    out: Vec<Line<'static>>,
    /// 正在拼的那条逻辑行。
    line: Vec<Span<'static>>,
    /// 行内的修饰栈：粗体、斜体、删除线、链接标签。
    modifiers: Vec<Modifier>,
    /// 引用的层数。
    quote: usize,
    /// 列表栈；每层的 `Some(n)` 是有序列表的下一个编号，`None` 是无序。
    lists: Vec<Option<u64>>,
    /// 当前条目还没写出去的标记。
    marker: Option<String>,
    /// 正在收的标题级别。
    heading: Option<HeadingLevel>,
    /// 块与块之间要不要空一行（Markdown 的源空行不产生事件，所以得自己记）。
    pending_blank: bool,
    code: Option<CodeBuffer>,
    table: Option<TableBuffer>,
    frames: Vec<Frame>,
    /// 调用方会在第一行前面加的前导占多少列（`to_lines` 走的是 0）。
    indent: usize,
}

impl Renderer {
    fn new(width: usize, indent: usize) -> Self {
        Self {
            width,
            indent,
            out: Vec::new(),
            line: Vec::new(),
            modifiers: Vec::new(),
            quote: 0,
            lists: Vec::new(),
            marker: None,
            heading: None,
            pending_blank: false,
            code: None,
            table: None,
            frames: Vec::new(),
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        if !self.line.is_empty() || self.marker.is_some() {
            self.flush_line();
        }
        self.out
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.write_text(&text),
            Event::Code(code) => {
                let style = self.style().fg(CODE);
                self.write_piece(&code, style);
            }
            // 终端画不出 HTML，剥标签等于替作者做了一次有损翻译；透传至少保真（spec §6）。
            Event::Html(html) | Event::InlineHtml(html) => self.write_text(&html),
            Event::SoftBreak | Event::HardBreak => self.flush_line(),
            Event::Rule => {
                self.blank_if_pending();
                self.out.push(Line::from(Span::styled(
                    "─".repeat(RULE_WIDTH),
                    Style::default().fg(MUTED),
                )));
                self.pending_blank = true;
            }
            Event::TaskListMarker(done) => {
                self.marker = Some(if done {
                    "☑ ".to_owned()
                } else {
                    "☐ ".to_owned()
                });
            }
            // 没开的那些扩展（脚注、数学、定义列表）与认得的一切：原样透传，一个字不丢。
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.open_block(),
            Tag::Heading { level, .. } => {
                self.open_block();
                self.heading = Some(level);
            }
            Tag::BlockQuote(_) => {
                self.open_block();
                self.quote += 1;
            }
            Tag::CodeBlock(kind) => {
                self.open_block();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => first_word(&info),
                    CodeBlockKind::Indented => None,
                };
                self.code = Some(CodeBuffer {
                    lang,
                    text: String::new(),
                });
            }
            Tag::List(start) => {
                self.open_block();
                self.lists.push(start);
            }
            Tag::Item => self.marker = Some(self.next_marker()),
            Tag::Table(alignments) => {
                self.open_block();
                self.table = Some(TableBuffer {
                    alignments,
                    rows: Vec::new(),
                    cells: Vec::new(),
                    current: Vec::new(),
                    header: false,
                    in_cell: false,
                });
            }
            Tag::TableHead => self.with_table(|table| {
                table.header = true;
                table.cells.clear();
            }),
            Tag::TableRow => self.with_table(|table| table.cells.clear()),
            Tag::TableCell => self.with_table(|table| {
                table.in_cell = true;
                table.current.clear();
            }),
            Tag::Emphasis => self.modifiers.push(Modifier::ITALIC),
            Tag::Strong => self.modifiers.push(Modifier::BOLD),
            Tag::Strikethrough => self.modifiers.push(Modifier::CROSSED_OUT),
            Tag::Link { dest_url, .. } => self.open_frame(dest_url.to_string()),
            Tag::Image { dest_url, .. } => self.open_frame(dest_url.to_string()),
            Tag::HtmlBlock => self.open_block(),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.close_flow(),
            TagEnd::Heading(_) => {
                let style = heading_style(self.heading.take());
                for span in &mut self.line {
                    span.style = span.style.patch(style);
                }
                self.close_flow();
            }
            TagEnd::BlockQuote(_) => {
                if !self.line.is_empty() {
                    self.flush_line();
                }
                self.quote = self.quote.saturating_sub(1);
                self.pending_blank = true;
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    self.write_code(code);
                }
                self.pending_blank = true;
            }
            TagEnd::List(_) => {
                self.lists.pop();
                self.pending_blank = true;
            }
            TagEnd::Item => {
                if !self.line.is_empty() || self.marker.is_some() {
                    self.flush_line();
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.write_table(table);
                }
                self.pending_blank = true;
            }
            TagEnd::TableHead => {
                self.with_table(|table| {
                    table.header = false;
                    let cells = std::mem::take(&mut table.cells);
                    table.rows.push(TableRowBuffer {
                        header: true,
                        cells,
                    });
                });
            }
            TagEnd::TableRow => {
                self.with_table(|table| {
                    let cells = std::mem::take(&mut table.cells);
                    table.rows.push(TableRowBuffer {
                        header: false,
                        cells,
                    });
                });
            }
            TagEnd::TableCell => {
                self.with_table(|table| {
                    table.in_cell = false;
                    let cell = std::mem::take(&mut table.current);
                    table.cells.push(cell);
                });
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.modifiers.pop();
            }
            TagEnd::Link => {
                self.modifiers.pop();
                self.close_link();
            }
            TagEnd::Image => {
                self.modifiers.pop();
                self.close_image();
            }
            TagEnd::HtmlBlock => {
                if !self.line.is_empty() {
                    self.flush_line();
                }
                self.pending_blank = true;
            }
            _ => {}
        }
    }

    /// 这一段块结束了：把还挂着的那条行落下去，并让下一个块与它之间空一行。
    fn close_flow(&mut self) {
        if !self.line.is_empty() || self.marker.is_some() {
            self.flush_line();
        }
        self.pending_blank = true;
    }

    fn with_table(&mut self, act: impl FnOnce(&mut TableBuffer)) {
        if let Some(table) = &mut self.table {
            act(table);
        }
    }

    /// 当前该往哪儿写：表格的一格，或者正在拼的那条行（spec §1）。
    fn target(&mut self) -> &mut Vec<Span<'static>> {
        match &mut self.table {
            Some(table) if table.in_cell => &mut table.current,
            _ => &mut self.line,
        }
    }

    /// 一段行内文字：`Text` 里的换行是**行边界**，不是文本内容（spec §1）。
    fn write_text(&mut self, text: &str) {
        if let Some(code) = &mut self.code {
            code.text.push_str(text);
            return;
        }
        if self.table.as_ref().is_some_and(|table| table.in_cell) {
            let style = self.style();
            if !text.is_empty() {
                self.target().push(Span::styled(text.to_owned(), style));
            }
            return;
        }
        for (index, part) in text.split('\n').enumerate() {
            if index > 0 {
                self.flush_line();
            }
            if !part.is_empty() {
                let style = self.style();
                self.line.push(Span::styled(part.to_owned(), style));
            }
        }
    }

    fn write_piece(&mut self, text: &str, style: Style) {
        if !text.is_empty() {
            self.target().push(Span::styled(text.to_owned(), style));
        }
    }

    /// 当前行的基础样式：引用里的一切都是叙述灰（spec §6）。
    fn style(&self) -> Style {
        let base = if self.quote > 0 {
            Style::default().fg(MUTED)
        } else {
            Style::default()
        };
        let mut modifier = Modifier::empty();
        for item in &self.modifiers {
            modifier |= *item;
        }
        base.add_modifier(modifier)
    }

    fn flush_line(&mut self) {
        let line = self.take_line();
        self.out.push(line);
    }

    /// 把正在拼的那条行交出去，并挂上它的前缀：引用条、列表缩进、条目标记。
    fn take_line(&mut self) -> Line<'static> {
        let marker = self.marker.take();
        let indent = self.lists.len().saturating_sub(1) * LIST_INDENT;
        if self.line.is_empty() && marker.is_none() && self.quote == 0 && indent == 0 {
            return Line::from("");
        }
        let mut spans = Vec::with_capacity(self.line.len() + 2);
        for _ in 0..self.quote {
            spans.push(Span::styled("│ ".to_owned(), Style::default().fg(MUTED)));
        }
        if indent > 0 {
            spans.push(Span::raw(" ".repeat(indent)));
        }
        if let Some(marker) = marker {
            spans.push(Span::raw(marker));
        }
        spans.extend(std::mem::take(&mut self.line));
        Line::from(spans)
    }

    /// 下一个列表条目用什么标记：有序列表按 `Start` 给的起始编号往下数（spec §6）。
    fn next_marker(&mut self) -> String {
        match self.lists.last_mut() {
            Some(Some(number)) => {
                let current = *number;
                *number += 1;
                format!("{current}. ")
            }
            _ => "• ".to_owned(),
        }
    }

    /// 一个块要从新的一行开始：先把上一条还没落地的行内尾巴交出去，再按需空一行。
    ///
    /// 紧凑列表里没有段落包裹，所以「上一个条目的文字」可以一直挂在 `line` 上，直到嵌套的
    /// 子列表事件到达 —— 那一下再 flush 就会把两层的文字并成一行。
    fn open_block(&mut self) {
        if !self.line.is_empty() {
            self.flush_line();
        }
        self.blank_if_pending();
    }

    /// 顶层的块与块之间空一行；列表里、引用里不空 —— 那儿的行本来就密。
    fn blank_if_pending(&mut self) {
        if self.pending_blank
            && self.quote == 0
            && self.lists.is_empty()
            && self.out.last().is_some_and(|line| !line.spans.is_empty())
        {
            self.out.push(Line::from(""));
        }
        self.pending_blank = false;
    }

    fn open_frame(&mut self, url: String) {
        // 标签写在**当前目标**上：一个格里的链接与图片落在那一格自己的 span 列表里，
        // 而段落的落在 `self.line` 上（`target()` 分派这两者）。
        let start = self.target().len();
        self.frames.push(Frame { url, start });
        self.modifiers.push(Modifier::UNDERLINED);
    }

    /// 链接画法不变：下划线标签 + 灰色 ` (url)`，两者相同时不重复（spec §6）。
    fn close_link(&mut self) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        let label: String = {
            let row = self.target();
            if frame.start < row.len() {
                row[frame.start..]
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            } else {
                String::new()
            }
        };
        if label != frame.url {
            let style = Style::default().fg(MUTED);
            self.target()
                .push(Span::styled(format!(" ({})", frame.url), style));
        }
    }

    /// 图片落成 `[图片] alt (url)`：终端暂时画不出图，但「这儿有一张图」得说出来
    /// （spec §6）。`alt` 为空时不留一个空的下划线 span。
    fn close_image(&mut self) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        let alt: Vec<Span<'static>> = {
            let row = self.target();
            if frame.start <= row.len() {
                row.split_off(frame.start)
            } else {
                Vec::new()
            }
        };
        let alt_text: String = alt.iter().map(|span| span.content.as_ref()).collect();
        let muted = Style::default().fg(MUTED);
        let mut spans = vec![Span::styled(wording::IMAGE_PLACEHOLDER.to_owned(), muted)];
        if !alt_text.is_empty() {
            spans.push(Span::styled(" ".to_owned(), muted));
            spans.extend(alt);
        }
        if alt_text != frame.url {
            spans.push(Span::styled(format!(" ({})", frame.url), muted));
        }
        self.target().extend(spans);
    }

    /// 一个围栏块（或缩进代码块）：上方一条右对齐的语言名，下面是高亮过的代码（spec §3）。
    fn write_code(&mut self, code: CodeBuffer) {
        // 整块从第 `indent` 列起，于是语言名仍然结束在转录的**右缘**（第 `width` 列）。
        let budget = self.width.saturating_sub(self.indent);
        if let Some(lang) = &code.lang {
            let pad = budget.saturating_sub(width::text_columns(lang));
            let mut spans = Vec::new();
            if self.indent > 0 {
                spans.push(Span::raw(" ".repeat(self.indent)));
            }
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            spans.push(Span::styled(lang.clone(), Style::default().fg(MUTED)));
            self.out.push(Line::from(spans));
        }
        self.out.extend(code_lines(
            &code.text,
            code.lang.as_deref(),
            budget,
            self.indent,
        ));
    }

    /// 一整张表：表头、分隔线、按对齐排好的列，超宽就在格子里折行（spec §2）。
    fn write_table(&mut self, table: TableBuffer) {
        let columns = table.alignments.len().max(
            table
                .rows
                .iter()
                .map(|row| row.cells.len())
                .max()
                .unwrap_or(0),
        );
        if columns == 0 {
            return;
        }
        // 列宽先是每列的自然宽度（内容最大显示宽度，CJK 按两列）。
        let mut widths = vec![0usize; columns];
        for row in &table.rows {
            for (index, cell) in row.cells.iter().enumerate() {
                if index < columns {
                    widths[index] = widths[index].max(cell_width(cell));
                }
            }
        }
        // 装得下就把余量全给最后一列；装不下就削最宽的那一列、把额度让给它折行，
        // 削到每列一格为止 —— 不截断（spec §2）。宽度预算是 `width - indent`：整张表从
        // 第 `indent` 列起，与调用方加在第一行上的前缀共占同一段列。
        let budget = self.width.saturating_sub(self.indent);
        let gaps = GAP_WIDTH * (columns - 1);
        let mut total: usize = widths.iter().sum::<usize>() + gaps;
        if total < budget {
            widths[columns - 1] += budget - total;
        } else {
            while total > budget {
                let Some(index) = widest(&widths) else {
                    break;
                };
                if widths[index] <= 1 {
                    break;
                }
                widths[index] -= 1;
                total -= 1;
            }
        }

        for row in &table.rows {
            let wrapped: Vec<Vec<Vec<Span<'static>>>> = (0..columns)
                .map(|index| {
                    let cell: &[Span<'static>] =
                        row.cells.get(index).map(Vec::as_slice).unwrap_or(&[]);
                    wrap_cell(cell, widths[index])
                })
                .collect();
            // 一个格折成三行，同行其余各格一起撑到三行高，网格竖线才连得上。
            let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
            for line_index in 0..height {
                let mut spans = Vec::new();
                if self.indent > 0 {
                    spans.push(Span::raw(" ".repeat(self.indent)));
                }
                for (index, width) in widths.iter().enumerate() {
                    if index > 0 {
                        spans.push(Span::styled(" │ ".to_owned(), Style::default().fg(GRID)));
                    }
                    let empty = Vec::new();
                    let content = wrapped
                        .get(index)
                        .and_then(|cell| cell.get(line_index))
                        .unwrap_or(&empty);
                    let alignment = table
                        .alignments
                        .get(index)
                        .copied()
                        .unwrap_or(Alignment::None);
                    push_cell(&mut spans, content, *width, alignment);
                }
                let mut line = Line::from(spans);
                if row.header {
                    for span in &mut line.spans {
                        span.style = span.style.add_modifier(Modifier::BOLD);
                    }
                }
                self.out.push(line);
            }
            if row.header {
                self.out.push(separator(&widths, self.indent));
            }
        }
    }
}

/// 标题自己那一行：`#`/`##` 用青色挑出来，更深的只加粗。
fn heading_style(level: Option<HeadingLevel>) -> Style {
    let style = Style::default().add_modifier(Modifier::BOLD);
    if matches!(level, Some(HeadingLevel::H1 | HeadingLevel::H2)) {
        style.fg(Color::Cyan)
    } else {
        style
    }
}

/// info string 的第一个词，保留作者写的大小写：`rust ignore` 只显示 `rust`，
/// 后面的词是给文档工具的指令，不是语言名（spec §3）。
fn first_word(info: &str) -> Option<String> {
    info.split_whitespace().next().map(str::to_owned)
}

/// 代码块的每一行：两格缩进、逐字保留、按宽度折行，续行保持缩进（spec §3）。
///
/// `budget` 已经扣掉了整块那个 `indent`，而 `indent` 要补回每一行的行首 —— 于是代码块与
/// 语言名那一行共享同一个左边界。
fn code_lines(text: &str, lang: Option<&str>, budget: usize, indent: usize) -> Vec<Line<'static>> {
    // 围栏里的文字以一个换行收尾，那是定界符的一部分，不是一行空代码。
    let text = text.strip_suffix('\n').unwrap_or(text);
    if text.is_empty() {
        return Vec::new();
    }
    let rows = match lang.and_then(|lang| highlight::highlight_code(lang, text)) {
        Some(rows) => rows,
        // 认不出的语言、或高亮不可用：代码退纯文本，但**不消失**（spec §3）。
        None => plain_rows(text),
    };
    let mut out = Vec::new();
    for row in rows {
        let content: Vec<Span<'static>> = row
            .into_iter()
            .map(|span| Span::styled(span.text, span.class.style()))
            .collect();
        out.extend(wrap_code(&content, budget, indent));
    }
    out
}

fn plain_rows(text: &str) -> Vec<Vec<highlight::Span>> {
    text.split('\n')
        .map(|line| {
            vec![highlight::Span {
                text: line.to_owned(),
                class: Class::Plain,
            }]
        })
        .collect()
}

/// 一条代码行折到预算内，续行**保持**那两格缩进 —— 交给 `pane::wrap_line` 硬折会把续行
/// 顶到最左边、跟代码块脱节（spec §3）。
fn wrap_code(content: &[Span<'static>], budget: usize, indent: usize) -> Vec<Line<'static>> {
    let lead = indent + CODE_INDENT;
    let budget = budget.max(lead + 1);
    let mut rows: Vec<Vec<Span<'static>>> = Vec::new();
    let mut row: Vec<Span<'static>> = Vec::new();
    let mut used = lead;
    for span in content {
        for ch in span.content.chars() {
            let columns = width::char_columns(ch);
            if used + columns > budget && used > lead {
                rows.push(std::mem::take(&mut row));
                used = lead;
            }
            pane::push_char(&mut row, ch, span.style);
            used += columns;
        }
    }
    rows.push(row);
    rows.into_iter()
        .map(|content| {
            let mut spans = vec![Span::raw(" ".repeat(lead))];
            spans.extend(content);
            Line::from(spans)
        })
        .collect()
}

/// 一格的显示宽度。
fn cell_width(cell: &[Span<'_>]) -> usize {
    cell.iter()
        .map(|span| width::text_columns(&span.content))
        .sum()
}

/// 一格里折出来的那几行；折行口径与窗格同一条（按字符、CJK 两列、保样式）。
fn wrap_cell(cell: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    let line = Line::from(cell.to_vec());
    pane::wrap_line(&line, width)
        .into_iter()
        .map(|line| line.spans)
        .collect()
}

/// 把一格的某一行按列宽补齐并对齐。右对齐的 padding 加在**左边**（spec §2）。
fn push_cell(
    out: &mut Vec<Span<'static>>,
    content: &[Span<'static>],
    width: usize,
    alignment: Alignment,
) {
    let used = cell_width(content);
    let pad = width.saturating_sub(used);
    let (left, right) = match alignment {
        Alignment::Right => (pad, 0),
        Alignment::Center => (pad / 2, pad - pad / 2),
        Alignment::Left | Alignment::None => (0, pad),
    };
    if left > 0 {
        out.push(Span::raw(" ".repeat(left)));
    }
    out.extend(content.iter().cloned());
    if right > 0 {
        out.push(Span::raw(" ".repeat(right)));
    }
}

/// 表头下面那条 `─┼─` 分隔线：它同时就是「上一行是表头」这个信号（spec §2）。
fn separator(widths: &[usize], indent: usize) -> Line<'static> {
    let mut text = String::new();
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            text.push('─');
            text.push('┼');
            text.push('─');
        }
        text.push_str(&"─".repeat(*width));
    }
    let mut spans = Vec::new();
    if indent > 0 {
        spans.push(Span::raw(" ".repeat(indent)));
    }
    spans.push(Span::styled(text, Style::default().fg(GRID)));
    Line::from(spans)
}

/// 最宽的那一列的列号；并列时取最后一个。
fn widest(widths: &[usize]) -> Option<usize> {
    widths
        .iter()
        .enumerate()
        .max_by_key(|(_, width)| **width)
        .map(|(index, _)| index)
}
