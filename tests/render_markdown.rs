//! 自己的接缝上的 Markdown 渲染器：编码 agent 会发出的那些结构变成带样式的行，
//! 而认不出来的一律原样存活成纯文本。
//!
//! 解析交给 `pulldown-cmark`（`.scratch/markdown-render/spec.md` §1），所以这些断言大多
//! 落在**结构**上：表头带粗体、分隔线存在、各列起点一致、语言名结束在右缘 —— 而不是整份
//! 文本快照。

use fs_agent::render::highlight::{self, Class};
use fs_agent::render::markdown::{to_lines, to_lines_indented};
use fs_agent::render::palette;
use fs_agent::render::width;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

/// 测试用的可用宽度：一个像样的主列。
const W: u16 = 78;

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

/// `needle` 在这一行里起始的**显示列**。
fn column_of(line: &Line<'_>, needle: &str) -> Option<usize> {
    let full = text(line);
    let at = full.find(needle)?;
    Some(width::text_columns(&full[..at]))
}

/// 一条表格的竖线从哪一列起。
fn bar_column(line: &Line<'_>) -> Option<usize> {
    column_of(line, "│")
}

/// 一条行的显示宽度。
fn columns_of(line: &Line<'_>) -> usize {
    width::text_columns(&text(line))
}

#[test]
fn headings_stand_out_and_deeper_ones_are_just_bold() {
    let top = to_lines("# Title", W);
    assert_eq!(text(&top[0]), "Title");
    assert!(has_modifier(&top[0], Modifier::BOLD));
    assert!(has_fg(&top[0], palette::CODE_HEADING));

    let deep = to_lines("### Note", W);
    assert_eq!(text(&deep[0]), "Note");
    assert!(has_modifier(&deep[0], Modifier::BOLD));
    assert!(!has_fg(&deep[0], palette::CODE_HEADING));

    // 收尾的井号串前面得有一个空格：`# C#` 是一个名叫 `C#` 的标题。
    assert_eq!(text(&to_lines("# C#", W)[0]), "C#");
    assert_eq!(text(&to_lines("## Title ##", W)[0]), "Title");
}

#[test]
fn inline_emphasis_code_and_links_are_styled() {
    let line = &to_lines("a **bold** and *italic* and `code`", W)[0];
    assert!(has_modifier(line, Modifier::BOLD));
    assert!(has_modifier(line, Modifier::ITALIC));
    assert!(has_fg(line, palette::CODE_QUIET), "行内 code：{line:?}");
    assert_eq!(text(line), "a bold and italic and code");

    let link = &to_lines("[docs](https://example.com/x)", W)[0];
    assert!(has_modifier(link, Modifier::UNDERLINED));
    assert!(
        text(link).contains("https://example.com/x"),
        "目标显示出来了：{link:?}"
    );
}

#[test]
fn a_fenced_block_is_kept_verbatim_and_never_parsed_as_markdown() {
    // 围栏本身不打印；语言名那一行在上方，代码行恒以两格缩进开头。
    let lines = to_lines("```rust\nfn main() {}\n# not a heading\n```", W);
    let code: Vec<&Line<'_>> = lines
        .iter()
        .filter(|line| {
            line.spans
                .first()
                .is_some_and(|span| span.content.as_ref() == "  ")
        })
        .collect();
    assert_eq!(code.len(), 2, "两行代码：{lines:?}");
    assert_eq!(text(code[0]), "  fn main() {}");
    assert_eq!(text(code[1]), "  # not a heading");
    assert!(!has_fg(code[1], palette::CODE_HEADING), "代码块里面的标题");
    assert!(!has_modifier(code[1], Modifier::BOLD));
}

/// 内容域的取色：语法数字与界面上的警告不同色，注释与标点复用同一档退后
/// （`.scratch/tui-visual-language/spec.md` §3、票 16）。
#[test]
fn syntax_classes_take_their_colours_from_the_content_palette() {
    let lines = to_lines("```rust\n// note\nlet x = 42;\n```", W);
    let spans: Vec<&ratatui::text::Span<'_>> =
        lines.iter().flat_map(|line| line.spans.iter()).collect();
    let comment = spans
        .iter()
        .find(|span| span.content.contains("note"))
        .expect("注释是它自己的一个 span");
    assert_eq!(comment.style.fg, Some(palette::CODE_QUIET));
    assert!(comment.style.add_modifier.contains(Modifier::ITALIC));
    let number = spans
        .iter()
        .find(|span| span.content.as_ref() == "42")
        .expect("数字是它自己的一个 span");
    assert_eq!(number.style.fg, Some(palette::CODE_NUMBER));
    assert_ne!(
        palette::CODE_NUMBER,
        palette::WARN,
        "代码里的数字不再与界面上的警告同色"
    );
}

#[test]
fn list_items_keep_their_marker_and_task_boxes_become_checkboxes() {
    assert_eq!(text(&to_lines("- alpha", W)[0]), "• alpha");
    assert_eq!(text(&to_lines("1. first", W)[0]), "1. first");
    assert_eq!(text(&to_lines("- [x] done", W)[0]), "☑ done");
    assert_eq!(text(&to_lines("- [ ] todo", W)[0]), "☐ todo");
    // 缩进来自**嵌套结构**，每层两格 —— 解析器已经把作者敲的原始空格换成了结构。
    let nested = to_lines("- alpha\n  - nested\n    - deep", W);
    assert_eq!(text(&nested[0]), "• alpha");
    assert_eq!(text(&nested[1]), "  • nested");
    assert_eq!(text(&nested[2]), "    • deep");
}

#[test]
fn quotes_rules_and_tables_render_as_structure() {
    let quote = &to_lines("> quoted", W)[0];
    assert_eq!(text(quote), "│ quoted");
    assert!(has_fg(quote, palette::CODE_QUIET));

    let rule = &to_lines("---", W)[0];
    assert!(text(rule).chars().all(|ch| ch == '─'), "{rule:?}");

    // 表格现在是真网格：表头行，一条 `─┼─` 分隔线，然后是数据行。
    let table = to_lines("| a | b |\n|---|---|\n| 1 | 2 |", W);
    assert_eq!(text(&table[0]).trim_end(), "a │ b");
    assert!(table[1]
        .spans
        .iter()
        .all(|span| span.content.chars().all(|ch| ch == '─' || ch == '┼')));
    assert!(
        table[1]
            .spans
            .iter()
            .all(|span| span.style.fg == Some(palette::CODE_QUIET)),
        "网格线用内容域的退后一档：{:?}",
        table[1]
    );
    assert_eq!(text(&table[2]).trim_end(), "1 │ 2");
}

#[test]
fn an_intraword_underscore_is_not_emphasis() {
    let line = &to_lines("call snake_case_word now", W)[0];
    assert_eq!(text(line), "call snake_case_word now");
    assert!(!has_modifier(line, Modifier::ITALIC));
}

#[test]
fn malformed_markdown_degrades_to_plain_text_rather_than_vanishing() {
    // `#` 与 `>` 不在这一串里：它们在 CommonMark 里是**合法但空**的结构（空标题、空引用），
    // 渲染成空本来就是对的；这里的每一条都该留下能读的文字。
    for source in [
        "**unterminated",
        "`unclosed",
        "[label](",
        "| a | b",
        "1.",
        "<b",
    ] {
        let lines = to_lines(source, W);
        assert!(!lines.is_empty(), "{source:?} 一行都没产出");
        let rendered: String = lines.iter().map(text).collect();
        assert!(!rendered.is_empty(), "{source:?} 渲染成了空的");
    }
}

#[test]
fn empty_structures_render_empty_rather_than_panicking() {
    for source in ["", "#", ">", "```\n```"] {
        let lines = to_lines(source, W);
        assert!(
            lines.iter().all(|line| text(line).is_empty()),
            "{source:?} 应该什么都不画：{lines:?}"
        );
    }
}

// ── 表格（spec §2）───────────────────────────────────────────────────────────

#[test]
fn a_table_header_is_bold_and_the_separator_is_drawn() {
    let table = to_lines("| name | value |\n|---|---|\n| a | 1 |", W);
    assert!(
        has_modifier(&table[0], Modifier::BOLD),
        "表头加粗：{table:?}"
    );
    assert!(table[0]
        .spans
        .iter()
        .all(|span| span.style.add_modifier.contains(Modifier::BOLD)));
    assert!(!has_modifier(&table[2], Modifier::BOLD), "数据行不加粗");
    assert!(
        text(&table[1]).contains('┼'),
        "分隔线：{:?}",
        text(&table[1])
    );
}

#[test]
fn table_columns_line_up_and_the_table_starts_at_column_zero() {
    let table = to_lines("| name | value |\n|---|---|\n| alpha | 1 |\n| b | 22 |", W);
    // 表头、数据、数据 —— 四条行里的竖线都在同一列。
    let bars: Vec<usize> = table.iter().filter_map(bar_column).collect();
    assert_eq!(bars.len(), 3, "表头 + 两条数据行：{table:?}");
    assert!(
        bars.windows(2).all(|pair| pair[0] == pair[1]),
        "列没对齐：{bars:?}"
    );
    // 顶格：`name` 那一列宽五格，所以竖线落在第 6 列，而不是被缩进推到别处。
    assert_eq!(bar_column(&table[0]), Some(6));
    assert!(text(&table[2]).starts_with("alpha"));
}

#[test]
fn a_table_spends_its_spare_width_on_the_last_column() {
    let table = to_lines("| a | b |\n|---|---|\n| 1 | 2 |", W);
    for line in &table {
        assert_eq!(columns_of(line), W as usize, "表格撑满可用宽度：{line:?}");
    }
}

#[test]
fn a_right_aligned_column_pads_on_the_left() {
    let table = to_lines("| l | r |\n| :-- | --: |\n| a | b |", 8);
    assert_eq!(text(&table[2]), "a │    b");
}

#[test]
fn an_overwide_table_wraps_its_widest_column_and_keeps_the_rows_equal_height() {
    let table = to_lines("| a | bbbb |\n|---|---|\n| cccc | dddd |", 10);
    // 表头两行（`bbbb` 折成两行）、一条分隔线、数据两行。
    assert_eq!(table.len(), 5, "{table:?}");
    for line in &table {
        assert_eq!(columns_of(line), 10, "每一行都是同一个宽度：{line:?}");
    }
    assert_eq!(text(&table[0]), "a    │ bbb");
    assert_eq!(text(&table[1]), "     │ b  ", "同行其余格撑到同一高度");
    assert_eq!(text(&table[3]), "cccc │ ddd");
    assert_eq!(text(&table[4]), "     │ d  ");
}

// ── 代码块与高亮（spec §3、§4）─────────────────────────────────────────────

#[test]
fn a_code_block_puts_its_language_at_the_right_edge() {
    let lines = to_lines("```rust\nfn main() {}\n```", W);
    assert_eq!(
        text(&lines[0]),
        format!("{}rust", " ".repeat(W as usize - 4))
    );
    assert_eq!(columns_of(&lines[0]), W as usize);
    assert_eq!(text(&lines[1]), "  fn main() {}");
}

#[test]
fn a_bare_fence_draws_no_language_line() {
    let lines = to_lines("```\nplain **text**\n```", W);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(text(&lines[0]), "  plain **text**");
}

#[test]
fn an_unknown_language_still_names_itself() {
    let lines = to_lines("```brainfuck\n+++\n```", 20);
    assert_eq!(text(&lines[0]), format!("{}brainfuck", " ".repeat(11)));
    // 认不出的是**高亮**，不是标签：代码行只有默认样式。
    assert_eq!(text(&lines[1]), "  +++");
    assert!(lines[1]
        .spans
        .iter()
        .all(|span| span.style == Style::default()));
}

#[test]
fn only_the_first_word_of_the_info_string_is_the_language() {
    let lines = to_lines("```rust ignore\nfn main() {}\n```", W);
    assert!(text(&lines[0]).ends_with("rust"), "{:?}", text(&lines[0]));
    assert!(!text(&lines[0]).contains("ignore"));
    // 大小写按作者写的留着。
    let lines = to_lines("```JS\nconst a = 1;\n```", W);
    assert!(text(&lines[0]).ends_with("JS"), "{:?}", text(&lines[0]));
}

#[test]
fn a_long_code_line_wraps_with_the_indent_kept() {
    let lines = to_lines(
        "```rust\nlet x = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\";\n```",
        30,
    );
    assert_eq!(lines.len(), 3, "语言名 + 折出来的两行：{lines:?}");
    for line in &lines[1..] {
        let body = text(line);
        assert!(body.starts_with("  "), "缩进在：{body:?}");
        assert!(!body.starts_with("   "), "恰好两格：{body:?}");
    }
    assert!(text(&lines[2]).starts_with("  a"), "{:?}", text(&lines[2]));
}

#[test]
fn an_indented_code_block_takes_the_same_path_as_a_fence() {
    let lines = to_lines("text\n\n    indented code\n    more\n", W);
    assert_eq!(text(&lines[0]), "text");
    assert_eq!(text(&lines[2]), "  indented code");
    assert_eq!(text(&lines[3]), "  more");
}

#[test]
fn rust_highlighting_is_wired_back_into_code_blocks() {
    let lines = to_lines("```rust\nfn main() {}\n```", W);
    let code = &lines[1];
    assert!(
        code.spans
            .iter()
            .any(|span| span.style == Class::Keyword.style()),
        "`fn` 拿到了关键字的样式：{code:?}"
    );
}

#[test]
fn highlight_classes_come_back_for_rust() {
    let rows =
        highlight::highlight_code("rust", "fn main() { let s = \"hi\"; }").expect("rust 有文法");
    let keyword = rows
        .iter()
        .flatten()
        .find(|span| span.text == "fn")
        .expect("关键字是自己的一个 span");
    assert_eq!(keyword.class, Class::Keyword);
    let string = rows
        .iter()
        .flatten()
        .find(|span| span.text.contains("hi"))
        .expect("这个字面量是一个 span");
    assert_eq!(string.class, Class::String);
}

#[test]
fn all_ten_grammars_are_wired_up() {
    let samples = [
        ("rust", "fn f() {}"),
        ("bash", "echo $HOME"),
        ("json", "{\"a\": 1}"),
        ("toml", "a = 1"),
        ("html", "<p>x</p>"),
        ("javascript", "const a = 1"),
        ("typescript", "const a: number = 1"),
        ("php", "<?php echo 1;"),
        ("sql", "SELECT 1 FROM t"),
        ("python", "def f(): pass"),
    ];
    for (language, source) in samples {
        let rows = highlight::highlight_code(language, source)
            .unwrap_or_else(|| panic!("{language} 没接上文法"));
        assert!(
            rows.iter().flatten().any(|span| span.class != Class::Plain),
            "{language} 一片都没上色：{rows:?}"
        );
    }
}

#[test]
fn aliases_resolve_to_the_same_grammar() {
    for (alias, full) in [
        ("rs", "rust"),
        ("py", "python"),
        ("js", "javascript"),
        ("ts", "typescript"),
        ("sh", "bash"),
        ("shell", "bash"),
    ] {
        let source = "let x = 1;";
        assert_eq!(
            highlight::highlight_code(alias, source),
            highlight::highlight_code(full, source),
            "`{alias}` 与 `{full}` 应该是同一份文法"
        );
    }
}

#[test]
fn an_unmapped_language_does_not_reach_the_highlighter() {
    assert!(highlight::highlight_code("brainfuck", "+++").is_none());
    // `yaml` 不在十种语言里，所以不映射。
    assert!(highlight::highlight_code("yaml", "a: 1").is_none());
    assert!(highlight::highlight_code("", "x").is_none());
}

// ── 图片与 HTML（spec §6）───────────────────────────────────────────────────

#[test]
fn an_image_becomes_a_readable_placeholder() {
    let line = &to_lines("![图](https://example.com/a.png)", W)[0];
    assert_eq!(text(line), "[图片] 图 (https://example.com/a.png)");
    let alt = line
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "图")
        .expect("alt 是一个自己的 span");
    assert!(alt.style.add_modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn an_image_without_alt_leaves_no_empty_label() {
    let line = &to_lines("![](https://example.com/a.png)", W)[0];
    assert_eq!(text(line), "[图片] (https://example.com/a.png)");
    assert!(
        !line.spans.iter().any(|span| span.content.is_empty()),
        "不留空 span：{line:?}"
    );
}

#[test]
fn an_image_whose_url_is_its_alt_does_not_repeat_it() {
    assert_eq!(text(&to_lines("![same](same)", W)[0]), "[图片] same");
}

#[test]
fn inline_html_is_passed_through_verbatim() {
    assert_eq!(
        text(&to_lines("before <b>x</b> after", W)[0]),
        "before <b>x</b> after"
    );
    assert_eq!(
        text(&to_lines("<div>block</div>", W)[0]),
        "<div>block</div>"
    );
}

#[test]
fn image_syntax_inside_a_fence_stays_verbatim() {
    let lines = to_lines("```\n![alt](url)\n```", W);
    assert_eq!(text(&lines[0]), "  ![alt](url)");
}

// ── 首行前缀与需要左边界对齐的块（spec §1、§5）────────────────────────────

#[test]
fn an_indented_table_keeps_every_row_on_the_same_left_edge() {
    // 调用方会在第一行前面加 `[name] `（这里假定 7 列）。表格整块从那一列起，于是表头、
    // 分隔线与数据行的竖线都在同一列上 —— 第一行的前缀正好替换掉渲染器铺的那段前导。
    const INDENT: u16 = 7;
    let table = to_lines_indented("| a | b |\n|---|---|\n| 1 | 2 |", W, INDENT);
    for line in &table {
        assert!(
            text(line).starts_with(&" ".repeat(INDENT as usize)),
            "整块从第 {INDENT} 列起：{line:?}"
        );
    }
    // 表头与数据行的竖线在同一列；分隔线用的是 `┼`，不算竖线。
    assert_eq!(bar_column(&table[0]), Some(9), "{table:?}");
    assert_eq!(bar_column(&table[1]), None, "分隔线：{:?}", text(&table[1]));
    assert_eq!(bar_column(&table[2]), Some(9), "{table:?}");
    // 右缘仍然撑满可用宽度。
    assert_eq!(columns_of(&table[0]), W as usize);
}

#[test]
fn an_indented_code_block_keeps_its_language_at_the_right_edge() {
    const INDENT: u16 = 7;
    let lines = to_lines_indented("```rust\nfn main() {}\n```", W, INDENT);
    // 语言名结束在转录的右缘（第 `width` 列），哪怕它前面还有前缀。
    assert_eq!(columns_of(&lines[0]), W as usize);
    assert!(text(&lines[0]).ends_with("rust"));
    // 代码行 = 前缀列 + 那两格缩进。
    assert_eq!(
        text(&lines[1]),
        format!("{}  fn main() {{}}", " ".repeat(INDENT as usize))
    );
}

// ── 一格里的行内 Markdown（spec §2 第 8 条）────────────────────────────────

#[test]
fn a_cell_renders_its_own_inline_markdown() {
    // 标签的起点按**当前目标**算：格里的链接落在那一格自己的 span 上，而不是行上。
    let table = to_lines("| x [docs](https://example.com/x) |\n|---|\n| y |", W);
    assert!(
        text(&table[0]).contains("x docs (https://example.com/x)"),
        "{:?}",
        text(&table[0])
    );
    assert!(has_modifier(&table[0], Modifier::UNDERLINED));
}

#[test]
fn a_cell_link_whose_url_is_its_label_does_not_repeat_it() {
    let table = to_lines("| x [docs](docs) |\n|---|\n| y |", W);
    assert!(
        text(&table[0]).contains("x docs"),
        "同标签同 url 时不补：{:?}",
        text(&table[0])
    );
    assert!(!text(&table[0]).contains("(docs)"), "{:?}", text(&table[0]));
}

#[test]
fn a_cell_renders_an_image_and_code_like_a_paragraph_does() {
    let table = to_lines("| ![图](a.png) |\n|---|\n| `code` |", W);
    assert!(
        text(&table[0]).contains("[图片] 图 (a.png)"),
        "{:?}",
        text(&table[0])
    );
    assert!(
        has_fg(&table[2], palette::CODE_QUIET),
        "行内 code：{:?}",
        table[2]
    );
}
