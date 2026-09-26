//! 语法高亮与 diff 着色 —— 刻意分成**两层**（spec §19）。
//!
//! ⚠️ **这个模块目前没有任何生产消费者。** 留着它是刻意的，不是疏忽：见
//! [`docs/highlight.md`](../../docs/highlight.md)，那里记着把它留在这里的两个决定，以及
//! 它要回来或要走掉得发生什么。短版本：TUI 曾经为工具输出合成这两层，而
//! `.scratch/tui-ux/` 的票 02 把工具输出改成以纯文本进详情覆盖层，于是最后一个调用方
//! 没了。现在只有这个模块自己的测试与 `tests/render_highlight.rs` 练它，所以这里的回归
//! 对产品是隐形的，直到又有人调它。
//!
//! diff 层回答关于一行的一个问题：它是新增、删除、hunk 头，还是上下文？语法层回答的是
//! 另一个：这是什么代码？一行可以既是新增又是关键字，于是调用方把两种样式合起来
//! （[`Class::style`] 盖在 [`DiffTag::style`] 上），而不是挑一个赢家。
//!
//! 语法层通过 `tree-sitter-highlight` 跑那份已经是依赖的 Rust 语法 —— syntect 会走的
//! Oniguruma 那条路没有走（spec §19，Out of Scope）。

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

/// 这个渲染器认得的 capture 名。查询点到、而这里没有的名字退回纯文本，这就是为什么一次
/// 语法更新弄不坏渲染 —— 它只能让东西不上色。
const CAPTURES: &[&str] = &[
    "attribute",
    "boolean",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "escape",
    "function",
    "function.builtin",
    "function.macro",
    "function.method",
    "keyword",
    "label",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "string",
    "string.special",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// 就上色而言，这是一种什么代码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Plain,
    Keyword,
    Function,
    Type,
    String,
    Comment,
    Number,
    Variable,
    Constant,
    Operator,
    Punctuation,
}

impl Class {
    fn of(capture: &str) -> Class {
        // capture 名是点分的（`function.method`），前缀决定属于哪一族。两个前缀都可能
        // 匹配时，长的在前。
        if capture.starts_with("comment") {
            Class::Comment
        } else if capture.starts_with("string") || capture == "escape" {
            Class::String
        } else if capture.starts_with("keyword") {
            Class::Keyword
        } else if capture.starts_with("function") {
            Class::Function
        } else if capture.starts_with("type") || capture == "constructor" {
            Class::Type
        } else if capture.starts_with("number") {
            Class::Number
        } else if capture.starts_with("constant") || capture == "boolean" {
            Class::Constant
        } else if capture.starts_with("variable") || capture == "label" || capture == "property" {
            Class::Variable
        } else if capture.starts_with("operator") {
            Class::Operator
        } else if capture.starts_with("punctuation") {
            Class::Punctuation
        } else {
            Class::Plain
        }
    }

    /// 这一类的 ANSI SGR 前缀；不上色时是 `""`。
    pub fn ansi(self) -> &'static str {
        match self {
            Class::Plain => "",
            Class::Keyword => "\x1b[35m",
            Class::Function => "\x1b[34m",
            Class::Type => "\x1b[36m",
            Class::String => "\x1b[32m",
            Class::Comment => "\x1b[90m",
            Class::Number | Class::Constant => "\x1b[33m",
            Class::Variable => "",
            Class::Operator | Class::Punctuation => "\x1b[37m",
        }
    }

    /// 这一类的 TUI 样式。
    pub fn style(self) -> Style {
        match self {
            Class::Plain | Class::Variable => Style::default(),
            Class::Keyword => Style::default().fg(Color::Magenta),
            Class::Function => Style::default().fg(Color::Blue),
            Class::Type => Style::default().fg(Color::Cyan),
            Class::String => Style::default().fg(Color::Green),
            Class::Comment => Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
            Class::Number | Class::Constant => Style::default().fg(Color::Yellow),
            Class::Operator | Class::Punctuation => Style::default().fg(Color::Gray),
        }
    }
}

/// 一行里上了色的一小片。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub class: Class,
}

/// 一行属于 diff 的哪一层。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffTag {
    /// 根本不属于一份 diff。
    Context,
    Added,
    Removed,
    /// 一个文件头（`+++` / `---`）或一个 hunk 头（`@@`）。
    Hunk,
}

impl DiffTag {
    /// plain 终端画这个标签用的 ANSI 颜色。
    pub fn ansi(self) -> &'static str {
        match self {
            DiffTag::Context => "",
            DiffTag::Added => "\x1b[32m",
            DiffTag::Removed => "\x1b[31m",
            DiffTag::Hunk => "\x1b[36m",
        }
    }

    /// 这个标签的 TUI 样式。它是**背景**，这样它与语法层的前景是叠加的，而不是互相打架。
    pub fn style(self) -> Style {
        match self {
            DiffTag::Context => Style::default(),
            DiffTag::Added => Style::default().bg(Color::Rgb(0, 40, 0)),
            DiffTag::Removed => Style::default().bg(Color::Rgb(50, 0, 0)),
            DiffTag::Hunk => Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        }
    }
}

/// 给统一 diff 的一行归类。
pub fn diff_tag(line: &str) -> DiffTag {
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("@@") {
        DiffTag::Hunk
    } else if line.starts_with('+') {
        DiffTag::Added
    } else if line.starts_with('-') {
        DiffTag::Removed
    } else {
        DiffTag::Context
    }
}

/// 配好的 Rust 语法，每进程构建一次。
///
/// 查询构建失败按「没有高亮可用」处理，而不是报错：渲染器的差事是显示输出，丢掉颜色是
/// 降级，不是失败。
fn rust_config() -> Option<&'static HighlightConfiguration> {
    static CONFIG: OnceLock<Option<HighlightConfiguration>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut config = HighlightConfiguration::new(
                tree_sitter_rust::LANGUAGE.into(),
                "rust",
                tree_sitter_rust::HIGHLIGHTS_QUERY,
                tree_sitter_rust::INJECTIONS_QUERY,
                "",
            )
            .ok()?;
            config.configure(CAPTURES);
            Some(config)
        })
        .as_ref()
}

/// 把 Rust 源码高亮成逐行的 span。
///
/// 解析树来自仓库地图已经依赖的那一份语法；不为高亮加第二份语法。任何失败都让源码按每行
/// 一个纯文本 span 回来。
pub fn highlight_rust(source: &str) -> Vec<Vec<Span>> {
    match try_highlight(source) {
        Some(lines) => lines,
        None => plain_lines(source),
    }
}

fn try_highlight(source: &str) -> Option<Vec<Vec<Span>>> {
    let config = rust_config()?;
    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(config, source.as_bytes(), None, None, |_| None)
        .ok()?;

    let mut lines: Vec<Vec<Span>> = vec![Vec::new()];
    let mut stack: Vec<Class> = Vec::new();
    for event in events {
        match event.ok()? {
            HighlightEvent::HighlightStart(highlight) => {
                // `configure` 把每个认得的 capture 映到它在 `CAPTURES` 里的下标，所以
                // 这个下标就是那个 capture 的名字。
                let name = CAPTURES.get(highlight.0).copied().unwrap_or("plain");
                stack.push(Class::of(name));
            }
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                let class = stack.last().copied().unwrap_or(Class::Plain);
                let Some(text) = source.get(start..end) else {
                    continue;
                };
                for (index, part) in text.split('\n').enumerate() {
                    if index > 0 {
                        lines.push(Vec::new());
                    }
                    if !part.is_empty() {
                        if let Some(line) = lines.last_mut() {
                            line.push(Span {
                                text: part.to_owned(),
                                class,
                            });
                        }
                    }
                }
            }
        }
    }
    Some(lines)
}

/// 把一份补丁（或普通文本）高亮成逐行的 span，**叠在** diff 层之上。
///
/// 这就是两层在合成：先剥掉 diff 标记，剩下的代码当作一整份文档来高亮（所以跨行的字符串
/// 或注释照样能解析），然后标记作为一个纯文本 span 贴回去。于是一个被删掉的 `fn` 既是
/// 删除又是关键字 —— 这正是把两层分开的全部理由。
pub fn highlight_diff(source: &str) -> Vec<Vec<Span>> {
    let lines: Vec<&str> = source.split('\n').collect();
    let mut code_lines: Vec<&str> = Vec::with_capacity(lines.len());
    let mut markers: Vec<Option<&str>> = Vec::with_capacity(lines.len());
    for line in &lines {
        match diff_tag(line) {
            DiffTag::Added | DiffTag::Removed => {
                let split = line
                    .char_indices()
                    .nth(1)
                    .map(|(index, _)| index)
                    .unwrap_or(line.len());
                let (marker, rest) = line.split_at(split);
                markers.push(Some(marker));
                code_lines.push(rest);
            }
            _ => {
                markers.push(None);
                code_lines.push(line);
            }
        }
    }

    let mut highlighted = highlight_rust(&code_lines.join("\n"));
    highlighted.resize(code_lines.len(), Vec::new());
    for (line, marker) in highlighted.iter_mut().zip(markers) {
        if let Some(marker) = marker {
            line.insert(
                0,
                Span {
                    text: marker.to_owned(),
                    class: Class::Plain,
                },
            );
        }
    }
    highlighted
}

fn plain_lines(source: &str) -> Vec<Vec<Span>> {
    source
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                Vec::new()
            } else {
                vec![Span {
                    text: line.to_owned(),
                    class: Class::Plain,
                }]
            }
        })
        .collect()
}

/// 为终端画一行：这一行属于 diff 时 diff 标签说话，而语法层给上下文行上色。两者是各自
/// 独立算出来的 —— 谁也不是从另一个推出来的。
pub fn ansi_line(line: &str, color: bool) -> String {
    if !color {
        return line.to_owned();
    }
    let tag = diff_tag(line);
    if tag != DiffTag::Context {
        return format!("{}{line}\x1b[0m", tag.ansi());
    }
    let mut out = String::new();
    for span in highlight_diff(line).into_iter().flatten() {
        let code = span.class.ansi();
        if code.is_empty() {
            out.push_str(&span.text);
        } else {
            out.push_str(code);
            out.push_str(&span.text);
            out.push_str("\x1b[0m");
        }
    }
    out
}
