//! 输入编辑器，当作状态机来测：折行、光标、键位表。
//!
//! 接缝就是 [`editor::Input`] 自己的 API。这是故意的：光标的全部
//! 状态就是一个字符下标，所以「光标去哪儿」是一个测试不用终端
//! 就能问的纯问题 —— 而这恰恰是内联视口
//! 让它做不到的事（ADR 0002）。

use fs_agent::render::editor::{self, Input};

/// 编辑器画出来的那个提示符。这些测试关心的是折行与光标，
/// 不是那个字形，所以它们通过这里来写这个前导：草稿前面那个记号
/// 在票 08 换成了 `❱ `（`.scratch/tui-input-pulse/spec.md` §2b），而只留
/// 一处要改的地方就够了。
const P: &str = editor::PROMPT;

/// 一个装着 `text`、光标在末尾的编辑器 —— 把它打出来之后剩下的样子。
fn typed(text: &str) -> Input {
    let mut input = Input::new();
    input.insert_str(text);
    input
}

/// 一帧会画出来的那些行，作为纯字符串。
fn rows(input: &Input, width: u16, height: u16) -> Vec<String> {
    input
        .view(width, height)
        .0
        .iter()
        .map(|row| row.spans.iter().map(|span| span.content.as_ref()).collect())
        .collect()
}

#[test]
fn a_draft_wraps_by_display_columns_with_the_prompt_then_an_indent() {
    // 提示符领着第一行，两个空格领着它之后的每一行，而文本
    // 按五列折行：提示符与缩进同宽，所以每一行
    // 装同样多的文本（spec §2、§5）。
    assert_eq!(rows(&typed("abc"), 5, 10), vec![format!("{P}abc")]);
    assert_eq!(
        rows(&typed("abcdefgh"), 5, 10),
        vec![format!("{P}abcde"), "  fgh".to_owned()]
    );
    // 一个换行是它自己的一次换行，而不是折行。
    assert_eq!(
        rows(&typed("ab\ncd"), 5, 10),
        vec![format!("{P}ab"), "  cd".to_owned()]
    );
    // 空草稿仍然占一行：提示符永远在那儿等着你打字。
    assert_eq!(rows(&Input::new(), 5, 10), vec![P.to_owned()]);
    assert_eq!(
        rows(&typed("ab\n"), 5, 10),
        vec![format!("{P}ab"), "  ".to_owned()]
    );
}

#[test]
fn a_wide_character_takes_two_columns_in_the_draft() {
    // 与转录同一套列算术：`你` 是三个字节、两列，
    // 按字节数会让它早折三列。
    assert_eq!(
        rows(&typed("你好世界"), 6, 10),
        vec![format!("{P}你好世"), "  界".to_owned()]
    );
    // 正好填满一行时，光标留在这行的最后一格，与 ASCII 一样。
    assert_eq!(rows(&typed("你好世"), 6, 10), vec![format!("{P}你好世")]);
}

#[test]
fn the_cursor_maps_onto_the_row_it_is_typed_on() {
    // 光标在 `abcdefgh` 末尾，按五列折行：第二行，
    // 在 `fgh` 之后。
    let input = typed("abcdefgh");
    let (_, cursor) = input.view(5, 10);
    assert_eq!(cursor.row, 1);
    assert_eq!(cursor.column, 2 + 3);

    // `Home` 把它放到整份草稿的头部。
    let mut input = input;
    input.home();
    let (_, cursor) = input.view(5, 10);
    assert_eq!((cursor.row, cursor.column), (0, 2));

    // 光标在一整行的末尾时，它留在最后一格 —— 终端那种
    // 待定折行 —— 而不是自己另开一行、把草稿往下推。
    let input = typed("abcde");
    assert_eq!(input.height(5), 1);
    let (_, cursor) = input.view(5, 10);
    assert_eq!(
        (cursor.row, cursor.column),
        (0, 2 + 4),
        "这一行的最后一格"
    );

    // 而空草稿把它放在提示符后面第一格。
    let (_, cursor) = Input::new().view(5, 10);
    assert_eq!((cursor.row, cursor.column), (0, 2));
}

#[test]
fn the_prompt_and_the_indent_are_the_same_width() {
    // 布局从一个常量里预留列数，而编辑器画这个提示符；
    // 两边必须一致，否则每一行折出来的位置都差一列。
    assert_eq!(
        editor::prompt_columns() as usize,
        fs_agent::render::width::text_columns(editor::PROMPT)
    );
    // 两列，与 `> ` 当初一样：草稿前面那个字形在票 08 换了，
    // 正是这一条说明那次改动没有挪动任何人的文本。`❱` 是模糊宽度 ——
    // 在这个渲染器的表里算一列，在配置成把这类字符画成双宽的终端里算两列，
    // 人工清单要求真人去看的就是这个。
    assert_eq!(editor::PROMPT, "❱ ");
    assert_eq!(editor::prompt_columns(), 2);
}

#[test]
fn the_cursor_follows_the_text_not_the_end_of_the_line() {
    let mut input = typed("abc");
    assert_eq!(input.view(80, 10).1.column, 2 + 3, "> abc");
    input.home();
    assert_eq!(input.view(80, 10).1.column, 2, "> |abc");
    input.right();
    assert_eq!(input.view(80, 10).1.column, 3, "> a|bc");
}

#[test]
fn a_new_line_opens_between_the_lines_and_the_arrows_cross_it() {
    let mut input = typed("ab");
    input.insert_char('\n'); // `Ctrl-J` 干的就是这个
    input.insert_str("cd");
    assert_eq!(input.text(), "ab\ncd");

    // 从第二行行首按左键，踩到第一行的末尾。
    input.home();
    let (_, cursor) = input.view(80, 10);
    assert_eq!(
        (cursor.row, cursor.column),
        (1, 2),
        "home 是这一行的行首"
    );
    input.left();
    let (_, cursor) = input.view(80, 10);
    assert_eq!((cursor.row, cursor.column), (0, 2 + 2));
    input.right();
    let (_, cursor) = input.view(80, 10);
    assert_eq!((cursor.row, cursor.column), (1, 2));
}

#[test]
fn backspace_and_delete_join_lines_at_the_edges() {
    let mut input = typed("ab\ncd");
    input.home();
    input.backspace();
    assert_eq!(
        input.text(),
        "abcd",
        "行首的退格往上拼到上一行"
    );

    let mut input = typed("ab\ncd");
    input.home();
    input.up();
    input.end();
    input.delete_forward();
    assert_eq!(
        input.text(),
        "abcd",
        "行尾的删除把下一行拉上来"
    );
}

#[test]
fn the_emacs_chords_stay_inside_the_cursor_line() {
    let mut input = typed("one\ntwo three");
    input.kill_to_line_start();
    assert_eq!(
        input.text(),
        "one\n",
        "Ctrl-U 拿的是这一行，不是整份草稿"
    );

    let mut input = typed("one\ntwo three");
    input.home();
    input.kill_to_line_end();
    assert_eq!(input.text(), "one\n", "Ctrl-K 拿的是这一行剩下的部分");

    let mut input = typed("one\ntwo three");
    input.kill_word();
    assert_eq!(
        input.text(),
        "one\ntwo ",
        "Ctrl-W 只擦掉它里面的一个词"
    );
}

#[test]
fn up_and_down_hold_the_visual_column_across_a_short_line() {
    let mut input = typed("abcdef\nab\nabcdef");
    // `home` 是这一行的行首，所以先走到第一行。
    input.up();
    input.up();
    input.home();
    for _ in 0..4 {
        input.right();
    }
    assert_eq!(input.view(80, 10).1.column, 2 + 4);
    input.down();
    assert_eq!(
        input.view(80, 10).1.column,
        2 + 2,
        "被短的那一行夹住了"
    );
    input.down();
    assert_eq!(input.view(80, 10).1.column, 2 + 4, "那个目标列活了下来");
    input.up();
    input.up();
    assert_eq!(input.view(80, 10).1.column, 2 + 4);
    // 第一行再往上没地方去了。
    input.up();
    assert_eq!(input.view(80, 10).1.row, 0);
}

#[test]
fn a_draft_taller_than_the_area_scrolls_to_keep_the_cursor_in_view() {
    let mut input = typed(&"x".repeat(399));
    let height = 5;
    let (rows, cursor) = input.view(10, height);
    assert_eq!(rows.len(), height as usize, "视图正好等于那块区域");
    assert_eq!(
        cursor.row,
        height - 1,
        "光标在最后一条可见行上"
    );
    assert_eq!(cursor.column, 2 + 9);

    input.home();
    let (rows, cursor) = input.view(10, height);
    assert_eq!(
        (cursor.row, cursor.column),
        (0, 2),
        "home 把头部卷回来"
    );
    let head: String = rows[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(
        head,
        format!("{P}xxxxxxxxxx"),
        "十列文本正好填满这一行"
    );
}

#[test]
fn the_arrows_do_not_walk_the_history() {
    let mut input = Input::new();
    input.insert_str("first");
    input.submitted();
    input.insert_str("draft");
    // 上下键归光标；它们什么都回想不起来。
    input.up();
    input.down();
    assert_eq!(input.text(), "draft");
    input.history_previous();
    assert_eq!(input.text(), "first");
    input.history_next();
    assert_eq!(input.text(), "draft", "那份新草稿回来了");
}

#[test]
fn a_multi_line_draft_is_recalled_whole_and_keeps_its_blank_lines() {
    let mut input = Input::new();
    input.insert_str("第一行\n\n第二行");
    assert_eq!(
        input.submitted(),
        "第一行\n\n第二行",
        "只裁掉两头的空白"
    );
    input.insert_str("next");
    input.history_previous();
    assert_eq!(input.text(), "第一行\n\n第二行");
}

#[test]
fn submitting_clears_the_draft_and_an_empty_one_is_an_empty_line() {
    // `submitted()` 是编辑器的全部产出：它裁掉两头的空白、清掉
    // 打进去的内容，并为 `Ctrl-P` 记住这一行。一份只有空白的草稿是一条
    // **空行** —— 循环怎么把「用户在空无一物上按了回车」与「stdin
    // 关了」分开，是提示通道的事，不是编辑器的事。
    let mut input = Input::new();
    assert_eq!(input.submitted(), "");
    assert_eq!(input.text(), "", "两种情况下草稿都被清掉");

    input.insert_str("   \n  ");
    assert_eq!(input.submitted(), "");
    // 没有值得回想的东西：空行永远进不了历史。
    input.insert_str("first");
    input.submitted();
    input.insert_str("draft");
    input.history_previous();
    assert_eq!(
        input.text(),
        "first",
        "那份空草稿留在历史之外"
    );

    // 同一行提交两次只留一条，于是 `Ctrl-P` 不会在刚发过的
    // 重复项里一趟趟走。
    let mut input = Input::new();
    input.insert_str("same");
    input.submitted();
    input.insert_str("same");
    input.submitted();
    input.insert_str("draft");
    input.history_previous();
    assert_eq!(input.text(), "same");
    input.history_previous();
    assert_eq!(input.text(), "same", "而它前面什么都没有");
}

// --- `/` 记号 --------------------------------------------------------------

#[test]
fn a_slash_token_is_the_head_of_the_first_line_and_ends_at_the_first_space() {
    // 菜单据此过滤的是什么：那个斜杠，加上它后面已经打进去的东西。
    let input = typed("/ask");
    let token = input.slash_token().expect("一个记号");
    assert_eq!(token.start, 0);
    assert_eq!(token.prefix, "ask");
    // 一个光秃秃的斜杠是还没打任何东西的记号：只打 `/` 菜单就开。
    assert_eq!(typed("/").slash_token().unwrap().prefix, "");
    // 算数的是光标，而不是草稿的末尾 —— 退到这
    // 个名字里面，记号就缩到光标之前的那部分。
    let mut input = typed("/ask-matt");
    input.home();
    for _ in 0..4 {
        input.right();
    }
    assert_eq!(input.slash_token().unwrap().prefix, "ask");
}

#[test]
fn a_slash_in_a_prompt_or_a_path_is_not_a_token() {
    // 循环只在第一行找命令，而且只找到第一个空格为止：
    // 其他一切都只是提示词里的字符，去补全它
    // 会覆盖掉用户本来想写的东西。
    assert!(typed("看看 /tmp/x").slash_token().is_none());
    assert!(typed("/ask-matt 优化这个").slash_token().is_none());
    assert!(typed("第一行\n/undo").slash_token().is_none());
    assert!(typed("ask-matt").slash_token().is_none());
    // 一份多行草稿，只要它的*第一*行是命令，菜单照样会开
    // —— 记号在第一行上，循环正是在那里读它。
    let mut input = typed("/ask\n帮我做 X");
    input.up();
    assert_eq!(input.slash_token().unwrap().prefix, "ask");
}

#[test]
fn completing_a_slash_token_replaces_what_was_typed_and_leaves_the_cursor_after_it() {
    let mut input = typed("/ask-matt 优化这个");
    // 光标在末尾，而记号不在那儿 —— 所以什么都没被补全。
    assert!(!input.complete_slash("undo"));
    assert_eq!(input.text(), "/ask-matt 优化这个");

    // 在记号里面时，这个名字替换掉整个记号，而空格之后
    // 那个任务原封不动留在原处。
    let mut input = typed("/ask 优化这个");
    input.home();
    for _ in 0..3 {
        input.right();
    }
    assert!(input.complete_slash("ask-matt"));
    assert_eq!(input.text(), "/ask-matt 优化这个");
    assert_eq!(input.submitted(), "/ask-matt 优化这个");
}
