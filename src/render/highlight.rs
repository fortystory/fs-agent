//! 语法高亮与 diff 着色 —— 刻意分成**两层**（spec §19）。
//!
//! 语法层现在的生产消费者是转录里的**代码块**：`render::markdown` 按围栏的 info string 挑
//! 一份文法，把这里给的 [`Class`] 铺到代码行上（`.scratch/markdown-render/spec.md` §3、§4）。
//! 它管**十种语言**，全部硬依赖，没有 feature 门控，也不在启动时把十份 query 全编译一遍。
//!
//! diff 层回答关于一行的一个问题：它是新增、删除、hunk 头，还是上下文？语法层回答的是
//! 另一个：这是什么代码？一行可以既是新增又是关键字，于是调用方把两种样式合起来
//! （[`Class::style`] 盖在 [`DiffTag::style`] 上），而不是挑一个赢家。diff 层这一轮仍然
//! 没有调用方：它是为工具输出留着的。
//!
//! 语法层通过 `tree-sitter-highlight` 跑文法；syntect 会走的 Oniguruma 那条路没有走
//! （spec §19，Out of Scope）。

use std::cell::RefCell;
use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

/// 这个渲染器认得的 capture 名。查询点到、而这里没有的名字退回纯文本，这就是为什么一次
/// 语法更新弄不坏渲染 —— 它只能让东西不上色。
const CAPTURES: &[&str] = &[
    "attribute",
    "boolean",
    "comment",
    "conditional",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "escape",
    "field",
    "float",
    "function",
    "function.builtin",
    "function.macro",
    "function.method",
    "keyword",
    "label",
    "module",
    "module.builtin",
    "number",
    "operator",
    "parameter",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "storageclass",
    "string",
    "string.special",
    "tag",
    "tag.error",
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
        } else if capture.starts_with("keyword")
            || capture == "conditional"
            || capture == "storageclass"
        {
            Class::Keyword
        } else if capture.starts_with("function") {
            Class::Function
        } else if capture.starts_with("type")
            || capture == "constructor"
            || capture == "tag"
            || capture.starts_with("tag.")
            || capture == "module"
            || capture.starts_with("module.")
        {
            Class::Type
        } else if capture.starts_with("number") || capture == "float" {
            Class::Number
        } else if capture.starts_with("constant") || capture == "boolean" {
            Class::Constant
        } else if capture.starts_with("variable")
            || capture == "label"
            || capture == "property"
            || capture == "field"
            || capture == "parameter"
        {
            Class::Variable
        } else if capture.starts_with("operator") {
            Class::Operator
        } else if capture.starts_with("punctuation") {
            Class::Punctuation
        } else {
            // `spell`（`tree-sitter-sequel` 给未归类词的兜底）落在这里，这是有意的：
            // 它本来就是「不知道是什么」，不该上色。
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

/// info string 里的语言名到文法的别名表（spec §4）。
///
/// 别名归我们管：`pulldown-cmark` 只把 info string 原样交出来，`rs` 与 `rust` 是同一份
/// 文法这件事得我们说。`yaml` 不在十种语言里，所以**不映射** —— 名单之外的一律不上色。
fn canonical_language(language: &str) -> Option<&'static str> {
    match language.to_ascii_lowercase().as_str() {
        "rust" | "rs" => Some("rust"),
        "bash" | "sh" | "shell" => Some("bash"),
        "json" => Some("json"),
        "toml" => Some("toml"),
        "html" => Some("html"),
        "javascript" | "js" => Some("javascript"),
        "typescript" | "ts" => Some("typescript"),
        "php" => Some("php"),
        "sql" => Some("sql"),
        "python" | "py" => Some("python"),
        _ => None,
    }
}

/// 一种语言的文法配置：**第一次用到它才编译 query**。
///
/// 实测首次编译 0.05 ms（json）到 70 ms（php）；十份全在启动时算会白付几百毫秒，所以
/// 每种语言一个 `OnceLock`，而不是一张启动时填满的表（spec §4）。
macro_rules! grammar {
    ($cell:ident, $language:expr, $name:literal, $highlights:expr, $injections:expr) => {{
        static $cell: OnceLock<Option<HighlightConfiguration>> = OnceLock::new();
        $cell.get_or_init(|| {
            let mut config =
                HighlightConfiguration::new($language, $name, $highlights, $injections, "").ok()?;
            config.configure(CAPTURES);
            Some(config)
        })
    }};
}

/// 十种语言各自的配置。查询构建失败按「这门语言没有高亮」处理，而不是报错：渲染器的差事
/// 是显示输出，丢掉颜色是降级，不是失败。
///
/// 两个名字与语言对不上的坑写在 `Cargo.toml` 里（`tree-sitter-toml-ng`、`tree-sitter-sequel`），
/// 另一个在常量名上：`bash` 与 `javascript` 导出的是**单数** `HIGHLIGHT_QUERY`，其余八种是
/// 复数。typescript 与 php 各有两个 `LANGUAGE`，这一轮取第一个（`LANGUAGE_TYPESCRIPT` /
/// `LANGUAGE_PHP`），JSX 那份 query 不拼接（spec §7）。
fn config_for(language: &str) -> Option<&'static HighlightConfiguration> {
    let config = match language {
        "rust" => grammar!(
            RUST,
            tree_sitter_rust::LANGUAGE.into(),
            "rust",
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY
        ),
        "bash" => grammar!(
            BASH,
            tree_sitter_bash::LANGUAGE.into(),
            "bash",
            tree_sitter_bash::HIGHLIGHT_QUERY,
            ""
        ),
        "json" => grammar!(
            JSON,
            tree_sitter_json::LANGUAGE.into(),
            "json",
            tree_sitter_json::HIGHLIGHTS_QUERY,
            ""
        ),
        "toml" => grammar!(
            TOML,
            tree_sitter_toml_ng::LANGUAGE.into(),
            "toml",
            tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
            ""
        ),
        "html" => grammar!(
            HTML,
            tree_sitter_html::LANGUAGE.into(),
            "html",
            tree_sitter_html::HIGHLIGHTS_QUERY,
            tree_sitter_html::INJECTIONS_QUERY
        ),
        "javascript" => grammar!(
            JAVASCRIPT,
            tree_sitter_javascript::LANGUAGE.into(),
            "javascript",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::INJECTIONS_QUERY
        ),
        "typescript" => grammar!(
            TYPESCRIPT,
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "typescript",
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ""
        ),
        "php" => grammar!(
            PHP,
            tree_sitter_php::LANGUAGE_PHP.into(),
            "php",
            tree_sitter_php::HIGHLIGHTS_QUERY,
            tree_sitter_php::INJECTIONS_QUERY
        ),
        "sql" => grammar!(
            SQL,
            tree_sitter_sequel::LANGUAGE.into(),
            "sql",
            tree_sitter_sequel::HIGHLIGHTS_QUERY,
            ""
        ),
        "python" => grammar!(
            PYTHON,
            tree_sitter_python::LANGUAGE.into(),
            "python",
            tree_sitter_python::HIGHLIGHTS_QUERY,
            ""
        ),
        _ => return None,
    };
    config.as_ref()
}

/// 把一段源码按语言高亮成逐行的 span。
///
/// 语言认不出、或它的文法构建失败时返回 `None`：调用方退纯文本，而不是让代码消失
/// （`.scratch/markdown-render/spec.md` §3）。
pub fn highlight_code(language: &str, source: &str) -> Option<Vec<Vec<Span>>> {
    let language = canonical_language(language)?;
    let config = config_for(language)?;
    try_highlight(config, source)
}

/// 把 Rust 源码高亮成逐行的 span。
///
/// 任何失败都让源码按每行一个纯文本 span 回来。
pub fn highlight_rust(source: &str) -> Vec<Vec<Span>> {
    highlight_code("rust", source).unwrap_or_else(|| plain_lines(source))
}

thread_local! {
    /// `tree-sitter-highlight` 的文档建议复用 `Highlighter`，而且说每线程要一个。
    /// 这就是那条收口：以前每次调用都新建一个。
    static HIGHLIGHTER: RefCell<Highlighter> = RefCell::new(Highlighter::new());
}

fn try_highlight(config: &HighlightConfiguration, source: &str) -> Option<Vec<Vec<Span>>> {
    HIGHLIGHTER.with(|cell| {
        let mut highlighter = cell.borrow_mut();
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
    })
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
