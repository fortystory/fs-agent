//! TUI 转录的 Markdown 渲染。
//!
//! 模型用 Markdown 作答；这里把它常见的那一部分变成带样式的 [`Line`]，于是一个标题读
//! 起来像个标题，一个围栏块读起来像代码。它刻意不是 CommonMark：一支手写的扫描器，只
//! 扫 coding agent 真会吐出的那些构造，不依赖任何 parser。它认不出的东西一律原样透传，
//! 所以不完美的输入会降级成纯文本，而不是消失。
//!
//! 调色板是**答案的**：这里没有任何东西被调暗成叙述的灰色。发言前缀与块的续行缩进归
//! 调用方管。

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// 行内代码与围栏块。
const CODE: Color = Color::Yellow;
/// 静音的结构：引用条、分隔线、链接目标。
const MUTED: Color = Color::Gray;

/// 把一个 Markdown 块渲染成终端行。
pub fn to_lines(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    // 当前敞着的围栏，形如 `(字符, 长度)`。
    let mut fence: Option<(char, usize)> = None;

    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some((ch, len)) = fence {
            if closes_fence(raw, ch, len) {
                fence = None;
            } else {
                lines.push(code_line(raw));
            }
            continue;
        }

        let trimmed = raw.trim_start();
        if let Some(open) = opening_fence(trimmed) {
            fence = Some(open);
            continue;
        }
        if trimmed.is_empty() {
            lines.push(Line::from(""));
            continue;
        }
        if let Some((level, rest)) = heading(trimmed) {
            lines.push(heading_line(level, rest));
            continue;
        }
        if is_rule(trimmed) {
            lines.push(rule_line());
            continue;
        }
        if let Some(rest) = blockquote(trimmed) {
            let mut spans = vec![Span::styled("│ ".to_owned(), Style::default().fg(MUTED))];
            spans.extend(inline_spans(rest, Style::default().fg(MUTED)));
            lines.push(Line::from(spans));
            continue;
        }
        if let Some((indent, marker, rest)) = list_item(raw) {
            let mut spans = vec![Span::raw(format!("{indent}{marker}"))];
            spans.extend(inline_spans(rest, Style::default()));
            lines.push(Line::from(spans));
            continue;
        }
        if let Some(cells) = table_row(trimmed) {
            // `|---|---|` 这条对齐行不带内容。
            if !is_table_separator(&cells) {
                lines.push(table_line(&cells));
            }
            continue;
        }
        lines.push(Line::from(inline_spans(raw, Style::default())));
    }
    lines
}

/// 围栏里的一行代码：缩进、上代码色，逐字节保留。
fn code_line(raw: &str) -> Line<'static> {
    Line::from(Span::styled(format!("  {raw}"), Style::default().fg(CODE)))
}

/// 标题自己那一行：`#`/`##` 用青色挑出来，更深的只加粗。
fn heading_line(level: usize, rest: &str) -> Line<'static> {
    let mut style = Style::default().add_modifier(Modifier::BOLD);
    if level <= 2 {
        style = style.fg(Color::Cyan);
    }
    Line::from(Span::styled(rest.to_owned(), style))
}

fn rule_line() -> Line<'static> {
    Line::from(Span::styled("─".repeat(24), Style::default().fg(MUTED)))
}

/// 表格的一行：各格用一条看得见的分隔符连起来。
fn table_line(cells: &[String]) -> Line<'static> {
    let text = cells.join(" │ ");
    Line::from(Span::styled(text, Style::default()))
}

/// 把 `| a | b |` 拆成各格；这一行不是表格行时返回 `None`。
fn table_row(trimmed: &str) -> Option<Vec<String>> {
    let body = trimmed.strip_prefix('|')?.strip_suffix('|')?;
    Some(body.split('|').map(|cell| cell.trim().to_owned()).collect())
}

fn is_table_separator(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|cell| {
            !cell.is_empty() && cell.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
        })
}

/// `# Title` .. `###### Title`。只有当一个空格把它与正文隔开时才剥掉收尾的井号串，
/// 所以 `# C#` 保得住它那个 `#`。
fn heading(trimmed: &str) -> Option<(usize, &str)> {
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = trimmed[level..].strip_prefix(' ')?;
    let rest = rest.trim_end();
    let without_closing = match rest.rfind(' ') {
        Some(space) if rest[space + 1..].chars().all(|ch| ch == '#') => rest[..space].trim_end(),
        None if rest.chars().all(|ch| ch == '#') => "",
        _ => rest,
    };
    Some((level, without_closing))
}

/// 一条主题分隔：`-`、`*` 或 `_` 三个及以上，允许夹空格。
fn is_rule(trimmed: &str) -> bool {
    let mut marker = None;
    let mut count = 0;
    for ch in trimmed.chars() {
        if ch == ' ' {
            continue;
        }
        if !matches!(ch, '-' | '*' | '_') {
            return false;
        }
        match marker {
            Some(existing) if existing != ch => return false,
            None => marker = Some(ch),
            _ => {}
        }
        count += 1;
    }
    count >= 3
}

fn blockquote(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

/// 一个列表项：`(缩进, 标记, 剩余)`。任务框会变成 `☐` / `☑`。
fn list_item(raw: &str) -> Option<(String, String, &str)> {
    let indent_len = raw.len() - raw.trim_start().len();
    let indent = &raw[..indent_len];
    let trimmed = &raw[indent_len..];

    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(bullet) {
            let (marker, rest) = task_box(rest);
            return Some((indent.to_owned(), marker, rest));
        }
    }
    // `12. item`：先数字，再 `. `。
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        if let Some(rest) = trimmed[digits..].strip_prefix(". ") {
            return Some((indent.to_owned(), format!("{}. ", &trimmed[..digits]), rest));
        }
    }
    None
}

/// 项目符号后面的任务框：`[x] done` / `[ ] todo`。
fn task_box(rest: &str) -> (String, &str) {
    match rest.get(..4) {
        Some("[x] ") | Some("[X] ") => ("☑ ".to_owned(), &rest[4..]),
        Some("[ ] ") => ("☐ ".to_owned(), &rest[4..]),
        _ => ("• ".to_owned(), rest),
    }
}

/// 开出一个围栏块的行：三个及以上的反引号或波浪号。
fn opening_fence(trimmed: &str) -> Option<(char, usize)> {
    for ch in ['`', '~'] {
        let count = trimmed.chars().take_while(|c| *c == ch).count();
        if count >= 3 {
            return Some((ch, count));
        }
    }
    None
}

fn closes_fence(raw: &str, ch: char, len: usize) -> bool {
    let trimmed = raw.trim();
    let count = trimmed.chars().take_while(|c| *c == ch).count();
    count >= len && trimmed.chars().skip(count).all(|c| c == ch || c == ' ')
}

/// 一行的行内 span：代码、粗体、斜体、删除线与链接。
///
/// `prev` 记着光标前一个字符，好让 `snake_case` 不被读成强调：`_` 只在词边界上开。
fn inline_spans(text: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    let mut prev = '\n';

    while !rest.is_empty() {
        // 代码 span 胜过它里面的一切。
        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                flush(&mut spans, &mut plain, base);
                spans.push(Span::styled(after[..end].to_owned(), base.fg(CODE)));
                prev = '`';
                rest = &after[end + 1..];
                continue;
            }
        }
        if let Some((marker, modifier)) = strong_marker(rest, prev) {
            if let Some(end) = rest[marker.len()..].find(marker) {
                flush(&mut spans, &mut plain, base);
                let inner = &rest[marker.len()..marker.len() + end];
                spans.extend(inline_spans(inner, base.add_modifier(modifier)));
                prev = '*';
                rest = &rest[marker.len() + end + marker.len()..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix("~~") {
            if let Some(end) = after.find("~~") {
                flush(&mut spans, &mut plain, base);
                spans.extend(inline_spans(
                    &after[..end],
                    base.add_modifier(Modifier::CROSSED_OUT),
                ));
                prev = '~';
                rest = &after[end + 2..];
                continue;
            }
        }
        if let Some((marker, modifier)) = emphasis_marker(rest, prev) {
            if let Some(end) = rest[marker.len()..].find(marker) {
                flush(&mut spans, &mut plain, base);
                let inner = &rest[marker.len()..marker.len() + end];
                spans.extend(inline_spans(inner, base.add_modifier(modifier)));
                prev = '*';
                rest = &rest[marker.len() + end + marker.len()..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('[') {
            if let Some(close) = after.find("](") {
                if let Some(end) = after[close + 2..].find(')') {
                    flush(&mut spans, &mut plain, base);
                    let label = &after[..close];
                    let url = &after[close + 2..close + 2 + end];
                    spans.extend(inline_spans(label, base.add_modifier(Modifier::UNDERLINED)));
                    if url != label {
                        spans.push(Span::styled(format!(" ({url})"), base.fg(MUTED)));
                    }
                    prev = ')';
                    rest = &after[close + 2 + end + 1..];
                    continue;
                }
            }
        }

        let ch = rest.chars().next().expect("非空");
        plain.push(ch);
        prev = ch;
        rest = &rest[ch.len_utf8()..];
    }

    flush(&mut spans, &mut plain, base);
    spans
}

/// `**` 或 `__`，但不认孤立的 `*` 或 `_`。`__` 需要词边界，理由与单个 `_` 相同：
/// `__init__` 是个标识符，不是强调。
fn strong_marker(rest: &str, prev: char) -> Option<(&'static str, Modifier)> {
    if rest.starts_with("**") {
        return Some(("**", Modifier::BOLD));
    }
    if rest.starts_with("__") && !prev.is_alphanumeric() {
        return Some(("__", Modifier::BOLD));
    }
    None
}

/// 单个 `*` 或 `_`；`_` 只在词边界上，好让标识符活下来。
fn emphasis_marker(rest: &str, prev: char) -> Option<(&'static str, Modifier)> {
    if rest.starts_with('*') && !rest.starts_with("**") {
        return Some(("*", Modifier::ITALIC));
    }
    if rest.starts_with('_') && !rest.starts_with("__") && !prev.is_alphanumeric() {
        return Some(("_", Modifier::ITALIC));
    }
    None
}

fn flush(spans: &mut Vec<Span<'static>>, plain: &mut String, base: Style) {
    if !plain.is_empty() {
        spans.push(Span::styled(std::mem::take(plain), base));
    }
}
