//! The Markdown renderer at its own seam: the constructs a coding agent emits
//! become styled lines, and anything unrecognized survives as plain text.

use fs_agent::render::markdown::to_lines;
use ratatui::style::{Color, Modifier};
use ratatui::text::Line;

fn text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn has_modifier(line: &Line<'_>, modifier: Modifier) -> bool {
    line.spans
        .iter()
        .any(|span| span.style.add_modifier.contains(modifier))
}

fn has_fg(line: &Line<'_>, color: Color) -> bool {
    line.spans.iter().any(|span| span.style.fg == Some(color))
}

#[test]
fn headings_stand_out_and_deeper_ones_are_just_bold() {
    let top = to_lines("# Title");
    assert_eq!(text(&top[0]), "Title");
    assert!(has_modifier(&top[0], Modifier::BOLD));
    assert!(has_fg(&top[0], Color::Cyan));

    let deep = to_lines("### Note");
    assert_eq!(text(&deep[0]), "Note");
    assert!(has_modifier(&deep[0], Modifier::BOLD));
    assert!(!has_fg(&deep[0], Color::Cyan));

    // A closing hash run needs a space before it: `# C#` is a heading named `C#`.
    assert_eq!(text(&to_lines("# C#")[0]), "C#");
    assert_eq!(text(&to_lines("## Title ##")[0]), "Title");
}

#[test]
fn inline_emphasis_code_and_links_are_styled() {
    let line = &to_lines("a **bold** and *italic* and `code`")[0];
    assert!(has_modifier(line, Modifier::BOLD));
    assert!(has_modifier(line, Modifier::ITALIC));
    assert!(has_fg(line, Color::Yellow), "inline code: {line:?}");
    assert_eq!(text(line), "a bold and italic and code");

    let link = &to_lines("[docs](https://example.com/x)")[0];
    assert!(has_modifier(link, Modifier::UNDERLINED));
    assert!(
        text(link).contains("https://example.com/x"),
        "the target is shown: {link:?}"
    );
}

#[test]
fn a_fenced_block_is_kept_verbatim_and_never_parsed_as_markdown() {
    let lines = to_lines("```rust\nfn main() {}\n# not a heading\n```");
    assert_eq!(lines.len(), 2, "the fence itself is not printed");
    assert_eq!(text(&lines[0]), "  fn main() {}");
    assert_eq!(text(&lines[1]), "  # not a heading");
    assert!(has_fg(&lines[1], Color::Yellow));
    assert!(!has_fg(&lines[1], Color::Cyan), "a heading inside code");
    assert!(!has_modifier(&lines[1], Modifier::BOLD));
}

#[test]
fn list_items_keep_their_marker_and_task_boxes_become_checkboxes() {
    assert_eq!(text(&to_lines("- alpha")[0]), "• alpha");
    assert_eq!(text(&to_lines("1. first")[0]), "1. first");
    assert_eq!(text(&to_lines("- [x] done")[0]), "☑ done");
    assert_eq!(text(&to_lines("- [ ] todo")[0]), "☐ todo");
    // Nesting is expressed by the source indentation.
    assert_eq!(text(&to_lines("  - nested")[0]), "  • nested");
}

#[test]
fn quotes_rules_and_tables_render_as_structure() {
    let quote = &to_lines("> quoted")[0];
    assert_eq!(text(quote), "│ quoted");
    assert!(has_fg(quote, Color::Gray));

    let rule = &to_lines("---")[0];
    assert!(text(rule).chars().all(|ch| ch == '─'), "{rule:?}");

    let table = to_lines("| a | b |\n|---|---|\n| 1 | 2 |");
    assert_eq!(text(&table[0]), "a │ b");
    assert_eq!(text(&table[1]), "1 │ 2", "the separator row is dropped");
}

#[test]
fn an_intraword_underscore_is_not_emphasis() {
    let line = &to_lines("call snake_case_word now")[0];
    assert_eq!(text(line), "call snake_case_word now");
    assert!(!has_modifier(line, Modifier::ITALIC));
}

#[test]
fn malformed_markdown_degrades_to_plain_text_rather_than_vanishing() {
    for source in [
        "**unterminated",
        "`unclosed",
        "[label](",
        "| a | b",
        ">",
        "#",
    ] {
        let lines = to_lines(source);
        assert!(!lines.is_empty(), "{source:?} produced no lines");
        let rendered: String = lines.iter().map(text).collect();
        assert!(!rendered.is_empty(), "{source:?} rendered to nothing");
    }
}
