//! Markdown rendering for the TUI transcript.
//!
//! The model answers in Markdown; this turns the common subset of it into styled
//! [`Line`]s, so a heading reads like a heading and a fenced block reads like
//! code. It is deliberately not CommonMark: a hand-written scanner over the
//! constructs a coding agent actually emits, with no parser dependency. Anything
//! it does not recognize is passed through verbatim, so imperfect input degrades
//! to plain text rather than disappearing.
//!
//! The palette is the **answer's**: nothing here is dimmed to the narration grey.
//! The caller owns the speaker prefix and the block's continuation indent.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Inline code and fenced blocks.
const CODE: Color = Color::Yellow;
/// Muted structure: blockquote bars, rules, link targets.
const MUTED: Color = Color::Gray;

/// Render one Markdown block as terminal lines.
pub fn to_lines(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    // The fence that is currently open, as `(character, length)`.
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
            // The `|---|---|` alignment row carries no content.
            if !is_table_separator(&cells) {
                lines.push(table_line(&cells));
            }
            continue;
        }
        lines.push(Line::from(inline_spans(raw, Style::default())));
    }
    lines
}

/// A fenced code line: indented and in the code colour, kept byte-for-byte.
fn code_line(raw: &str) -> Line<'static> {
    Line::from(Span::styled(format!("  {raw}"), Style::default().fg(CODE)))
}

/// A heading's own line: `#`/`##` stand out in cyan, deeper ones just bold.
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

/// A table row: cells joined with a visible separator.
fn table_line(cells: &[String]) -> Line<'static> {
    let text = cells.join(" │ ");
    Line::from(Span::styled(text, Style::default()))
}

/// Split `| a | b |` into its cells, or `None` when the line is not a table row.
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

/// `# Title` .. `###### Title`, with the closing hashes stripped.
fn heading(trimmed: &str) -> Option<(usize, &str)> {
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = trimmed[level..].strip_prefix(' ')?;
    Some((level, rest.trim_end().trim_end_matches('#').trim_end()))
}

/// A thematic break: three or more of `-`, `*` or `_`, spaces allowed.
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

/// A list item: `(indent, marker, rest)`. Task boxes become `☐` / `☑`.
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
    // `12. item`: digits, then `. `.
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        if let Some(rest) = trimmed[digits..].strip_prefix(". ") {
            return Some((indent.to_owned(), format!("{}. ", &trimmed[..digits]), rest));
        }
    }
    None
}

/// A task-list box after a bullet: `[x] done` / `[ ] todo`.
fn task_box(rest: &str) -> (String, &str) {
    match rest.get(..4) {
        Some("[x] ") | Some("[X] ") => ("☑ ".to_owned(), &rest[4..]),
        Some("[ ] ") => ("☐ ".to_owned(), &rest[4..]),
        _ => ("• ".to_owned(), rest),
    }
}

/// A line that opens a fenced block: three or more backticks or tildes.
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

/// One line's inline spans: code, bold, italic, strikethrough and links.
///
/// `prev` tracks the character before the cursor so `snake_case` is not read as
/// emphasis: `_` only opens at a word boundary.
fn inline_spans(text: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    let mut prev = '\n';

    while !rest.is_empty() {
        // A code span wins over everything inside it.
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

        let ch = rest.chars().next().expect("non-empty");
        plain.push(ch);
        prev = ch;
        rest = &rest[ch.len_utf8()..];
    }

    flush(&mut spans, &mut plain, base);
    spans
}

/// `**` or `__`, but not a lone `*` or `_`. `__` needs a word boundary for the
/// same reason single `_` does: `__init__` is an identifier, not emphasis.
fn strong_marker(rest: &str, prev: char) -> Option<(&'static str, Modifier)> {
    if rest.starts_with("**") {
        return Some(("**", Modifier::BOLD));
    }
    if rest.starts_with("__") && !prev.is_alphanumeric() {
        return Some(("__", Modifier::BOLD));
    }
    None
}

/// A single `*` or `_`; `_` only at a word boundary, so identifiers survive.
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
