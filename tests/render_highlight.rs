//! 语法高亮层与 diff 层（spec §19，用户故事 134）。
//!
//! 它们是两套计算，这是故意的：diff 标签说的是这一行在补丁里算什么，
//! 语法类别说的是它是什么种类的代码。这些测试把两边都钉住，
//! 也钉住语法层用的就是仓库里已经有的那套 Rust 语法，而
//! 不是一个新依赖。

use heng::render::highlight::{
    Class, DiffTag, ansi_line, diff_tag, highlight_diff, highlight_rust,
};

#[test]
fn a_rust_keyword_is_its_own_span() {
    let lines = highlight_rust("fn main() {}");
    assert_eq!(lines.len(), 1);
    let function = lines[0]
        .iter()
        .find(|span| span.text == "fn")
        .expect("关键字是自己的一个 span");
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
        .expect("这个字面量是一个 span");
    assert_eq!(string.class, Class::String);
}

#[test]
fn highlighting_something_that_is_not_rust_still_returns_lines() {
    // 工具结果常常就是纯文本；颜色会退化，但从不出错。
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
    // 上下文行由语法层上色，而关掉颜色时这一行
    // 原样返回。
    assert_eq!(ansi_line("+added", false), "+added");
}

#[test]
fn a_diff_tag_is_independent_of_the_syntax_class() {
    // 两层从不互相查询：一个被删掉的关键字仍然是个
    // 关键字，也仍然是一次删除。diff 层把标记剥下来，好让
    // 语法层看到的是代码，而不是补丁。
    let line = "-fn main() {}";
    assert_eq!(diff_tag(line), DiffTag::Removed);
    let spans = highlight_diff(line);
    let marker = spans[0].first().expect("标记被重新挂回去，成为一个 span");
    assert_eq!(marker.text, "-");
    let keyword = spans[0]
        .iter()
        .find(|span| span.text == "fn")
        .expect("关键字从标记下面活了下来");
    assert_eq!(keyword.class, Class::Keyword);
}

#[test]
fn a_stripped_diff_body_is_highlighted_with_cross_line_state() {
    // 高亮整篇剥掉标记的文档、而不是逐行高亮，才让一个
    // 跨行的结构被当成一个来解析。
    let spans = highlight_diff("+fn main() {\n+    // inside\n+}");
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0][0].text, "+");
    assert!(spans[1].iter().any(|span| span.class == Class::Comment));
}
