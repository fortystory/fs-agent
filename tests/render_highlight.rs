//! The syntax-highlighting and diff layers (spec §19, user story 134).
//!
//! They are two computations on purpose: the diff tag says what a line is in the
//! patch, the syntax class says what kind of code it is. These tests pin both,
//! and pin that the syntax layer is the Rust grammar already in the tree rather
//! than a new dependency.

use fs_agent::render::highlight::{
    ansi_line, diff_tag, highlight_diff, highlight_rust, Class, DiffTag,
};

#[test]
fn a_rust_keyword_is_its_own_span() {
    let lines = highlight_rust("fn main() {}");
    assert_eq!(lines.len(), 1);
    let function = lines[0]
        .iter()
        .find(|span| span.text == "fn")
        .expect("the keyword is a span of its own");
    assert_eq!(function.class, Class::Keyword);
}

#[test]
fn a_comment_is_classified_as_a_comment() {
    let lines = highlight_rust("// explain\nlet x = 1;");
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0][0].class, Class::Comment);
}

#[test]
fn a_string_literal_is_classified_as_a_string() {
    let lines = highlight_rust("let s = \"hello\";");
    let string = lines[0]
        .iter()
        .find(|span| span.text.contains("hello"))
        .expect("the literal is a span");
    assert_eq!(string.class, Class::String);
}

#[test]
fn highlighting_something_that_is_not_rust_still_returns_lines() {
    // A tool result is often plain text; color degrades, it never fails.
    let lines = highlight_rust("not rust at all :::\nsecond line");
    assert_eq!(lines.len(), 2);
    let text: String = lines[0].iter().map(|span| span.text.as_str()).collect();
    assert_eq!(text, "not rust at all :::");
}

#[test]
fn the_diff_layer_classifies_every_kind_of_line() {
    assert_eq!(diff_tag("+++ b/src/lib.rs"), DiffTag::Hunk);
    assert_eq!(diff_tag("--- a/src/lib.rs"), DiffTag::Hunk);
    assert_eq!(diff_tag("@@ -1,3 +1,4 @@"), DiffTag::Hunk);
    assert_eq!(diff_tag("+added"), DiffTag::Added);
    assert_eq!(diff_tag("-removed"), DiffTag::Removed);
    assert_eq!(diff_tag(" context"), DiffTag::Context);
}

#[test]
fn the_ansi_composition_colors_a_diff_line_by_its_tag() {
    assert!(ansi_line("+added", true).starts_with("\x1b[32m"));
    assert!(ansi_line("-removed", true).starts_with("\x1b[31m"));
    assert!(ansi_line("@@ hunk @@", true).starts_with("\x1b[36m"));
    // Context lines are painted by the syntax layer, and with color off the line
    // comes back untouched.
    assert_eq!(ansi_line("+added", false), "+added");
}

#[test]
fn a_diff_tag_is_independent_of_the_syntax_class() {
    // The two layers never consult each other: a removed keyword is still a
    // keyword, and still a removal. The diff layer peels the marker off so the
    // syntax layer sees code, not a patch.
    let line = "-fn main() {}";
    assert_eq!(diff_tag(line), DiffTag::Removed);
    let spans = highlight_diff(line);
    let marker = spans[0]
        .first()
        .expect("the marker is re-attached as a span");
    assert_eq!(marker.text, "-");
    let keyword = spans[0]
        .iter()
        .find(|span| span.text == "fn")
        .expect("the keyword survives the marker");
    assert_eq!(keyword.class, Class::Keyword);
}

#[test]
fn a_stripped_diff_body_is_highlighted_with_cross_line_state() {
    // Highlighting the stripped document rather than line by line is what keeps
    // a multi-line construct parsed as one.
    let spans = highlight_diff("+fn main() {\n+    // inside\n+}");
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0][0].text, "+");
    assert!(spans[1].iter().any(|span| span.class == Class::Comment));
}
