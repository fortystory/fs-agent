//! HTML → markdown：只为一件事服务 —— 让 `web_fetch` 交给模型的是正文，而不是一坨标签
//! （`.scratch/web-search-tool/spec.md` §5；票 04）。
//!
//! 规则只有两条，都很硬：
//!
//! * **先删，再换。** 主动内容（`script` / `style` / `iframe` / `noscript` / 各种控件）与隐藏
//!   元素（`style="display:none"`、`hidden` 属性、`aria-hidden="true"`）连着它们的内容一起丢掉，
//!   然后才做转换。顺序不能反 —— 反过来会把脚本正文当文本吐出来。
//! * **绝不回原始 HTML。** 结构太深或解析不了时给一个固定的省略标记，而不是把标签倒进上下文。
//!
//! 解析用 `tree-sitter-html`（它已经在依赖里，给代码高亮用），输出是给渲染层吃的 GFM ——
//! `markdown-render` 那一轮的果子在这里兑现。

use tree_sitter::{Node, Parser};

/// 结构深到这个层数就不再往下走，改放一句固定的话。
///
/// 照 DSH 的 512 层：正常的网页离它很远，而一个能把它撑爆的文档本身就是我们不想读的那种。
pub const MAX_DEPTH: usize = 512;

/// 转换不出来时那句**固定**的话。它进流、给模型看，所以是中文。
pub const OMITTED_MARKER: &str = "[这一页的结构太深了，剩下的正文已省略]";

/// 连着内容一起丢掉的元素：主动内容、控件、以及永远不是正文的那些。
const DROPPED: &[&str] = &[
    "script", "style", "iframe", "noscript", "template", "svg", "canvas", "object", "embed",
    "form", "input", "button", "select", "option", "textarea", "head", "nav", "footer",
];

/// 按 markdown 渲染一段 HTML。
pub fn to_markdown(html: &str) -> String {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_html::LANGUAGE.into())
        .is_err()
    {
        return OMITTED_MARKER.to_owned();
    }
    let Some(tree) = parser.parse(html, None) else {
        return OMITTED_MARKER.to_owned();
    };

    let mut renderer = Renderer::new(html.as_bytes());
    renderer.children(tree.root_node());
    renderer.finish()
}

struct Renderer<'a> {
    source: &'a [u8],
    out: String,
    /// 当前嵌到第几层。
    depth: usize,
    /// 列表嵌到第几层，用来缩进。
    list_depth: usize,
}

impl<'a> Renderer<'a> {
    fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            out: String::new(),
            depth: 0,
            list_depth: 0,
        }
    }

    fn finish(self) -> String {
        let mut text = self.out;
        // 段落之间最多留一个空行，首尾不留空。
        while text.contains("\n\n\n") {
            text = text.replace("\n\n\n", "\n\n");
        }
        text.trim().to_owned()
    }

    /// 遍历具名子节点。
    fn children(&mut self, node: Node<'_>) {
        if self.depth >= MAX_DEPTH {
            self.blank_line();
            self.out.push_str(OMITTED_MARKER);
            return;
        }
        self.depth += 1;
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.render(child);
        }
        self.depth -= 1;
    }

    fn render(&mut self, node: Node<'_>) {
        match node.kind() {
            "text" => {
                let Ok(raw) = node.utf8_text(self.source) else {
                    return;
                };
                let folded = raw.split_whitespace().collect::<Vec<_>>().join(" ");
                if !folded.is_empty() {
                    self.inline(&folded);
                }
            }
            "entity" => {
                if let Ok(raw) = node.utf8_text(self.source) {
                    let decoded = decode_entity(raw);
                    if !decoded.is_empty() {
                        self.inline(&decoded);
                    }
                }
            }
            "element" | "script_element" | "style_element" => self.element(node),
            // 注释、doctype、以及别的结构节点：什么都不留。
            _ => {}
        }
    }

    fn element(&mut self, node: Node<'_>) {
        let Some(name) = tag_name(node, self.source) else {
            return self.children(node);
        };
        let name = name.to_ascii_lowercase();
        if DROPPED.contains(&name.as_str()) || self.is_hidden(node) {
            return;
        }

        match name.as_str() {
            "br" => self.out.push('\n'),
            "hr" => {
                self.blank_line();
                self.out.push_str("---\n");
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name[1..].parse::<usize>().unwrap_or(1);
                self.blank_line();
                self.out.push_str(&"#".repeat(level.min(6)));
                self.out.push(' ');
                self.block_body(node);
            }
            "p" | "div" | "section" | "article" | "main" | "header" | "aside" | "figure"
            | "figcaption" | "dd" | "dt" | "caption" | "address" => {
                self.blank_line();
                self.block_body(node);
            }
            "ul" | "ol" => self.list(node, name == "ol"),
            "blockquote" => self.quote(node),
            "pre" => self.code_block(node),
            "code" => self.inline_code(node),
            "a" => self.link(node),
            "img" => self.image(node),
            "strong" | "b" => self.wrapped(node, "**"),
            "em" | "i" => self.wrapped(node, "*"),
            "del" | "s" | "strike" => self.wrapped(node, "~~"),
            "table" => self.table(node),
            // 表格的行与单元格由 `table` 统一处理；单独出现时退化成普通内容。
            "tr" | "td" | "th" | "thead" | "tbody" | "tfoot" => self.children(node),
            _ => self.children(node),
        }
    }

    /// 一个块级元素的内容：渲染完补一个换行（空行由 `blank_line` 在下一块之前补）。
    fn block_body(&mut self, node: Node<'_>) {
        self.children(node);
        self.out.push('\n');
    }

    /// 行内内容：在需要的地方补一个空格，别把两段词粘在一起。
    fn inline(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        match self.out.chars().last() {
            None | Some('\n') | Some(' ') => {}
            _ => self.out.push(' '),
        }
        self.out.push_str(text);
    }

    /// 保证当前内容以空行结束（输出为空时什么都不做）。
    fn blank_line(&mut self) {
        self.trim_trailing();
        if self.out.is_empty() {
            return;
        }
        while !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn trim_trailing(&mut self) {
        while self.out.ends_with([' ', '\n']) {
            self.out.pop();
        }
    }

    fn list(&mut self, node: Node<'_>, ordered: bool) {
        self.blank_line();
        let mut index = 1usize;
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let Some(tag) = tag_name(child, self.source) else {
                continue;
            };
            if !tag.eq_ignore_ascii_case("li") {
                continue;
            }
            self.trim_trailing();
            if !self.out.ends_with("\n\n") && !self.out.is_empty() {
                self.out.push('\n');
            }
            let indent = "  ".repeat(self.list_depth);
            let marker = if ordered {
                format!("{index}. ")
            } else {
                "- ".to_owned()
            };
            self.out.push_str(&indent);
            self.out.push_str(&marker);
            self.list_depth += 1;
            self.children(child);
            self.list_depth -= 1;
            index += 1;
        }
        self.blank_line();
    }

    fn quote(&mut self, node: Node<'_>) {
        self.blank_line();
        let start = self.out.len();
        self.children(node);
        let body = self.out[start..].trim().to_owned();
        self.out.truncate(start);
        for line in body.lines() {
            self.out.push_str("> ");
            self.out.push_str(line);
            self.out.push('\n');
        }
        self.blank_line();
    }

    fn code_block(&mut self, node: Node<'_>) {
        self.blank_line();
        let mut raw = String::new();
        collect_raw(node, self.source, &mut raw);
        let code = raw.trim_matches('\n');
        self.out.push_str("```\n");
        self.out.push_str(code);
        self.out.push_str("\n```\n");
    }

    fn inline_code(&mut self, node: Node<'_>) {
        let mut raw = String::new();
        collect_raw(node, self.source, &mut raw);
        let code = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if !code.is_empty() {
            // 反引号会把这个行内代码块提前关掉。
            self.inline(&format!("`{}`", code.replace('`', "'")));
        }
    }

    fn link(&mut self, node: Node<'_>) {
        let href = attribute(node, self.source, "href");
        let start = self.out.len();
        self.children(node);
        let label = self.out[start..].trim().to_owned();
        self.out.truncate(start);

        match href.filter(|href| !href.is_empty() && !href.starts_with('#')) {
            Some(href) => {
                let label = if label.is_empty() {
                    href.clone()
                } else {
                    label.replace('[', "(").replace(']', ")")
                };
                self.inline(&format!("[{label}]({href})"));
            }
            None => {
                if !label.is_empty() {
                    self.inline(&label);
                }
            }
        }
    }

    fn image(&mut self, node: Node<'_>) {
        let Some(src) = attribute(node, self.source, "src").filter(|src| !src.is_empty()) else {
            return;
        };
        let alt = attribute(node, self.source, "alt").unwrap_or_default();
        self.inline(&format!("![{}]({src})", alt.replace(['[', ']'], " ")));
    }

    fn wrapped(&mut self, node: Node<'_>, marker: &str) {
        let start = self.out.len();
        self.children(node);
        let body = self.out[start..].trim().to_owned();
        self.out.truncate(start);
        if body.is_empty() {
            return;
        }
        self.inline(&format!("{marker}{body}{marker}"));
    }

    /// GFM 表格：第一行当表头，第二行是分隔行。
    fn table(&mut self, node: Node<'_>) {
        let mut rows: Vec<Vec<String>> = Vec::new();
        collect_rows(node, self.source, &mut rows);
        let Some(header) = rows.first().cloned() else {
            return;
        };
        if header.is_empty() {
            return;
        }

        self.blank_line();
        self.out.push_str(&row_line(&header));
        self.out.push('|');
        for _ in &header {
            self.out.push_str(" --- |");
        }
        self.out.push('\n');
        for row in rows.iter().skip(1) {
            self.out.push_str(&row_line(row));
        }
        self.blank_line();
    }

    /// 这个元素自己带着「别显示我」的意思吗。
    fn is_hidden(&self, node: Node<'_>) -> bool {
        if attribute(node, self.source, "hidden").is_some() {
            return true;
        }
        if attribute(node, self.source, "aria-hidden").as_deref() == Some("true") {
            return true;
        }
        match attribute(node, self.source, "style") {
            Some(style) => {
                let style = style.to_ascii_lowercase().replace(' ', "");
                style.contains("display:none") || style.contains("visibility:hidden")
            }
            None => false,
        }
    }
}

fn row_line(row: &[String]) -> String {
    let mut line = String::from("|");
    for cell in row {
        line.push(' ');
        line.push_str(cell);
        line.push_str(" |");
    }
    line.push('\n');
    line
}

/// 找出表格的每一行，以及每行里每个单元格渲染出来的文本。
fn collect_rows(node: Node<'_>, source: &[u8], rows: &mut Vec<Vec<String>>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let kind = child.kind();
        if kind != "element" {
            continue;
        }
        match tag_name(child, source).map(|name| name.to_ascii_lowercase()) {
            Some(name) if name == "tr" => {
                let mut cells = Vec::new();
                let mut inner = child.walk();
                for cell in child.named_children(&mut inner) {
                    let Some(tag) = tag_name(cell, source) else {
                        continue;
                    };
                    if !matches!(tag.to_ascii_lowercase().as_str(), "td" | "th") {
                        continue;
                    }
                    let mut renderer = Renderer::new(source);
                    renderer.children(cell);
                    cells.push(
                        renderer
                            .finish()
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                }
                rows.push(cells);
            }
            Some(name) if matches!(name.as_str(), "thead" | "tbody" | "tfoot" | "table") => {
                collect_rows(child, source, rows);
            }
            _ => {}
        }
    }
}

/// 一段原样的文本（代码块用）：不折叠空白，`<br>` 变成换行，实体解码。
fn collect_raw(node: Node<'_>, source: &[u8], out: &mut String) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "text" => {
                if let Ok(raw) = child.utf8_text(source) {
                    out.push_str(raw);
                }
            }
            "entity" => {
                if let Ok(raw) = child.utf8_text(source) {
                    out.push_str(&decode_entity(raw));
                }
            }
            "element" => match tag_name(child, source).map(|name| name.to_ascii_lowercase()) {
                Some(name) if name == "br" => out.push('\n'),
                _ => collect_raw(child, source, out),
            },
            _ => {}
        }
    }
}

/// 这个元素的标签名。
fn tag_name<'a>(node: Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !matches!(child.kind(), "start_tag" | "self_closing_tag") {
            continue;
        }
        let mut inner = child.walk();
        for part in child.children(&mut inner) {
            if part.kind() == "tag_name" {
                return part.utf8_text(source).ok();
            }
        }
    }
    None
}

/// 一个元素上的属性值（属性的名字按小写比）。
fn attribute(node: Node<'_>, source: &[u8], wanted: &str) -> Option<String> {
    let mut cursor = node.walk();
    let tag = node
        .children(&mut cursor)
        .find(|child| matches!(child.kind(), "start_tag" | "self_closing_tag"))?;

    let mut inner = tag.walk();
    for attr in tag.named_children(&mut inner) {
        if attr.kind() != "attribute" {
            continue;
        }
        let mut name = None;
        let mut value = String::new();
        let mut parts = attr.walk();
        for part in attr.named_children(&mut parts) {
            match part.kind() {
                "attribute_name" => {
                    name = part.utf8_text(source).ok().map(str::to_ascii_lowercase);
                }
                "quoted_attribute_value" | "attribute_value" => {
                    if let Ok(raw) = part.utf8_text(source) {
                        value = unquote(raw);
                    }
                }
                _ => {}
            }
        }
        if name.as_deref() == Some(wanted) {
            // 没有值的属性（`hidden`）在这里就是空串 —— 调用方多半只关心「有没有」。
            return Some(value);
        }
    }
    None
}

fn unquote(raw: &str) -> String {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
        .unwrap_or(trimmed);
    decode_entity(trimmed)
}

/// 常见实体的解码。没列到的原样留下 —— 转义不完整比把文本吃掉好。
fn decode_entity(raw: &str) -> String {
    match raw {
        "&amp;" => "&".to_owned(),
        "&lt;" => "<".to_owned(),
        "&gt;" => ">".to_owned(),
        "&quot;" => "\"".to_owned(),
        "&#39;" | "&apos;" => "'".to_owned(),
        "&nbsp;" => " ".to_owned(),
        "&mdash;" => "—".to_owned(),
        "&ndash;" => "–".to_owned(),
        "&hellip;" => "…".to_owned(),
        other => other.to_owned(),
    }
}
