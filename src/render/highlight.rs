//! Syntax highlighting and diff coloring — deliberately **two layers** (spec
//! §19).
//!
//! The diff layer answers one question about a line: is it added, removed, a
//! hunk header, or context? The syntax layer answers a different one: what kind
//! of code is this? A line can be both an addition and a keyword, and the TUI
//! composes the two styles ([`Class::style`] patched over [`DiffTag::style`])
//! rather than picking a winner.
//!
//! The syntax layer runs the Rust grammar that is already a dependency through
//! `tree-sitter-highlight`, so the build stays C-free — the Oniguruma path
//! syntect would take is not used (spec §19, Out of Scope).

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

/// The capture names this renderer recognizes. Anything a query names that is
/// not here falls back to plain text, which is why a grammar update cannot break
/// rendering — it can only go uncolored.
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

/// A kind of code, as far as coloring is concerned.
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
        // Capture names are dotted (`function.method`), and the prefix decides
        // the family. Longest prefixes first where two could match.
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

    /// The ANSI SGR prefix for this class, or `""` for no color.
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

    /// The TUI style for this class.
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

/// One colored piece of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub class: Class,
}

/// Which layer of a diff a line belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffTag {
    /// Not part of a diff at all.
    Context,
    Added,
    Removed,
    /// A file header (`+++` / `---`) or a hunk header (`@@`).
    Hunk,
}

impl DiffTag {
    /// The ANSI color a plain terminal paints this tag with.
    pub fn ansi(self) -> &'static str {
        match self {
            DiffTag::Context => "",
            DiffTag::Added => "\x1b[32m",
            DiffTag::Removed => "\x1b[31m",
            DiffTag::Hunk => "\x1b[36m",
        }
    }

    /// The TUI style for this tag. It is a **background** so it composes with
    /// the syntax layer's foreground instead of fighting it.
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

/// Classify one line of a unified diff.
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

/// The configured Rust grammar, built once per process.
///
/// A failure to build the query is treated as "no highlighting available" rather
/// than an error: the renderer's job is to show output, and losing color is a
/// degradation, not a failure.
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

/// Highlight Rust source into per-line spans.
///
/// The parse tree comes from the same grammar the repo map already depends on;
/// no second grammar is added for highlighting. On any failure the source comes
/// back as one plain span per line.
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
                // `configure` maps every recognized capture onto its index in
                // `CAPTURES`, so the index is the capture's name.
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
                        lines
                            .last_mut()
                            .expect("a line was just ensured")
                            .push(Span {
                                text: part.to_owned(),
                                class,
                            });
                    }
                }
            }
        }
    }
    Some(lines)
}

/// Highlight a patch (or ordinary text) into per-line spans, **on top of** the
/// diff layer.
///
/// This is the two layers composing: the diff marker is peeled off first, the
/// remaining code is highlighted as one document (so a string or comment that
/// spans lines still parses), and the marker is re-attached as a plain span. A
/// removed `fn` is therefore both a removal and a keyword, which is the whole
/// point of keeping the layers separate.
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

/// Paint one line for a terminal: the diff tag wins when the line is part of a
/// diff, and the syntax layer colors context lines. The two are computed
/// independently — neither one is derived from the other.
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
