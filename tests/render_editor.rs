//! 输入编辑器，当作状态机来测：折行、光标、键位表。
//!
//! 接缝就是 [`editor::Input`] 自己的 API。这是故意的：光标的全部
//! 状态就是一个字符下标，所以「光标去哪儿」是一个测试不用终端
//! 就能问的纯问题 —— 而这恰恰是内联视口
//! 让它做不到的事（ADR 0002）。

use heng::render::editor::{self, Input};
use ratatui::style::{Color, Style};

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
    assert_eq!((cursor.row, cursor.column), (0, 2 + 4), "这一行的最后一格");

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
        heng::render::width::text_columns(editor::PROMPT)
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
    assert_eq!((cursor.row, cursor.column), (1, 2), "home 是这一行的行首");
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
    assert_eq!(input.text(), "abcd", "行首的退格往上拼到上一行");

    let mut input = typed("ab\ncd");
    input.home();
    input.up();
    input.end();
    input.delete_forward();
    assert_eq!(input.text(), "abcd", "行尾的删除把下一行拉上来");
}

#[test]
fn the_emacs_chords_stay_inside_the_cursor_line() {
    let mut input = typed("one\ntwo three");
    input.kill_to_line_start();
    assert_eq!(input.text(), "one\n", "Ctrl-U 拿的是这一行，不是整份草稿");

    let mut input = typed("one\ntwo three");
    input.home();
    input.kill_to_line_end();
    assert_eq!(input.text(), "one\n", "Ctrl-K 拿的是这一行剩下的部分");

    let mut input = typed("one\ntwo three");
    input.kill_word();
    assert_eq!(input.text(), "one\ntwo ", "Ctrl-W 只擦掉它里面的一个词");
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
    assert_eq!(input.view(80, 10).1.column, 2 + 2, "被短的那一行夹住了");
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
    assert_eq!(cursor.row, height - 1, "光标在最后一条可见行上");
    assert_eq!(cursor.column, 2 + 9);

    input.home();
    let (rows, cursor) = input.view(10, height);
    assert_eq!((cursor.row, cursor.column), (0, 2), "home 把头部卷回来");
    let head: String = rows[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(head, format!("{P}xxxxxxxxxx"), "十列文本正好填满这一行");
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
    assert_eq!(input.submitted(), "第一行\n\n第二行", "只裁掉两头的空白");
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
    assert_eq!(input.text(), "first", "那份空草稿留在历史之外");

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

// --- 记号 ------------------------------------------------------------------

#[test]
fn a_token_starts_where_it_is_typed_and_ends_at_the_first_space() {
    // 菜单据此过滤的是什么：那个前缀字符，加上它后面已经打进去的东西。
    let input = typed("/ask");
    let token = input.token().expect("一个记号");
    assert_eq!(token.prefix, '/');
    assert_eq!(token.start, 0);
    assert_eq!(token.query, "ask");
    // 一个光秃秃的斜杠是还没打任何东西的记号：只打 `/` 菜单就开。
    assert_eq!(typed("/").token().unwrap().query, "");
    // 算数的是光标在不在记号里，而不是它在名字的哪一格：记号就是整段名字，而补全替换的
    // 也是整段（`/ask-matt` 里的 `/ask` 被替掉之后，尾巴 `-matt` 不会留在原地）。
    let mut input = typed("/ask-matt");
    input.home();
    for _ in 0..4 {
        input.right();
    }
    assert_eq!(input.token().unwrap().query, "ask-matt");
}

#[test]
fn a_slash_token_can_start_anywhere_on_any_line() {
    // 与 `@` 对等：`/` 的记号在草稿**任意行、任意位置**开头
    // （`.scratch/input-tokens/spec.md` §2 那张表）—— 判据不同，位置的规则不该各写一套。
    let token = typed("看看 /tmp/x").token().expect("一个记号");
    assert_eq!((token.start, token.query.as_str()), (3, "tmp/x"));
    let token = typed("第一行\n/undo").token().expect("第二行也算");
    assert_eq!((token.start, token.query.as_str()), (4, "undo"));
    assert!(typed("ask-matt").token().is_none(), "没有记号就没有");
}

#[test]
fn an_at_token_starts_only_after_whitespace_or_the_start_of_the_draft() {
    // 引用出现在句子里是常态（`帮我改 @src/a.rs`），但 `src/@foo` 不是引用 ——
    // `@` 的边界比 `/` 严，因为一个字面 `@` 在路径与邮箱里都很常见。
    let token = typed("帮我改 @src/a.rs").token().expect("一个记号");
    assert_eq!(token.prefix, '@');
    assert_eq!(token.query, "src/a.rs");
    assert!(typed("@README.md").token().is_some(), "草稿起点算");
    assert!(typed("a@b").token().is_none(), "紧跟在字后面的 `@` 不算");
    assert!(typed("(a)@b").token().is_none(), "只有空白与草稿起点算");
    // `src/@foo` 里那个 `@` 同样不是引用 —— 光标所在的是 `/foo` 那个 `/` 记号。`@` 的边界
    // 不因为同一个位置上还有 `/` 而放宽。
    assert_eq!(typed("src/@foo").token().unwrap().prefix, '/');
}

#[test]
fn two_tokens_in_one_sentence_are_each_their_own() {
    // 一个句子里可以有多个记号，只认**光标所在**的那一个（spec §2）。
    let mut input = typed("改 @src/a.rs 和 /undo");
    assert_eq!(input.token().unwrap().prefix, '/', "光标在末尾的记号里");
    input.home();
    for _ in 0..5 {
        input.right();
    }
    let token = input.token().expect("走到 `@` 那个记号里");
    assert_eq!(token.prefix, '@');
    assert_eq!(token.query, "src/a.rs");
}

#[test]
fn completing_a_token_replaces_what_was_typed_and_leaves_the_cursor_after_it() {
    let mut input = typed("/ask-matt 优化这个");
    // 光标在末尾，而记号不在那儿 —— 所以什么都没被补全。
    assert!(!input.complete_token('/', "undo"));
    assert_eq!(input.text(), "/ask-matt 优化这个");

    // 在记号里面时，这个名字替换掉整个记号，而空格之后那个任务原封不动留在原处。
    let mut input = typed("/ask 优化这个");
    input.home();
    for _ in 0..3 {
        input.right();
    }
    assert!(input.complete_token('/', "ask-matt"));
    assert_eq!(input.text(), "/ask-matt 优化这个");
    assert_eq!(input.submitted(), "/ask-matt 优化这个");

    // `@` 走同一条路：补的是那个 `@` 记号，前文一个字不动。
    let mut input = typed("帮我改 @src/re 吧");
    input.home();
    for _ in 0..5 {
        input.right();
    }
    assert!(input.complete_token('@', "src/render/tui.rs"));
    assert_eq!(input.text(), "帮我改 @src/render/tui.rs 吧");
}

#[test]
fn the_editor_can_look_at_the_character_after_the_cursor() {
    // 补全用它决定要不要补一个分隔空格：末尾要，后面已经是空白就不要再补一个。
    let mut input = typed("ab");
    input.home();
    assert_eq!(input.next_char(), Some('a'));
    input.right();
    assert_eq!(input.next_char(), Some('b'));
    input.end();
    assert_eq!(input.next_char(), None, "末尾后面什么都没有");
}

// --- 记号：吸附与整块删（票 03） -------------------------------------------

/// 一个装好记号的编辑器：`[start, end)` 那一段是**能兑现**的记号。
///
/// 真实运行时这个区间由 `TuiState` 算好同步进来 —— 判据要查命令表与文件索引，而 `Input`
/// 两样都不认识（spec §4 那条缝）。所以这里直接设：测的正是缝的另一侧，纯几何的吸附与
/// 整块删。
fn with_token(text: &str, start: usize, end: usize) -> Input {
    let mut input = typed(text);
    input.set_token_spans(vec![editor::TokenSpan {
        start,
        end,
        style: Style::default().fg(Color::Magenta),
    }]);
    input
}

#[test]
fn the_cursor_never_lands_inside_a_token() {
    // `改 @src/a.rs 吧`：`@src/a.rs` 占字符区间 2..11（`改` 0、空格 1、`@` 2 … `s` 10）。
    let mut input = with_token("改 @src/a.rs 吧", 2, 11);
    input.end();
    assert_eq!(input.cursor(), 13, "末尾");
    input.left();
    assert_eq!(input.cursor(), 12, "记号后面那一格还能走");
    input.left();
    assert_eq!(input.cursor(), 11, "记号右边界是合法的落脚点");
    input.left();
    assert_eq!(input.cursor(), 2, "再往左一步跨过整块");
    input.right();
    assert_eq!(input.cursor(), 11, "往右也一步跨过整块");
    input.right();
    assert_eq!(input.cursor(), 12, "记号之外照旧一格一格");

    // `Home`/`End` 落在记号里时吸附到最近的边界。
    input.home();
    assert_eq!(input.cursor(), 0);
    input.end();
    assert_eq!(input.cursor(), 13);

    // 没有记号的草稿（就是那份 highlights 为空的）一切照旧。
    let mut plain = typed("look /tmp/x");
    plain.end();
    plain.left();
    plain.left();
    assert_eq!(plain.cursor(), 9);
}

#[test]
fn moving_between_lines_snaps_onto_the_nearest_token_edge() {
    // 第二行的 `@abc.rs` 占 13..20（`宽` 11、空格 12、`@` 13 … `s` 19）。
    let mut input = with_token("0123456789\n宽 @abc.rs 尾", 13, 20);
    input.up();
    input.home();
    for _ in 0..6 {
        input.right();
    }
    assert_eq!(input.cursor(), 6, "第一行的第 6 格");
    input.down();
    // 目标列落在记号里面（字符 16），吸附到**近的那个**边界：13。
    assert_eq!(input.cursor(), 13, "跳到最近的记号边界");
}

#[test]
fn deleting_across_a_token_takes_the_whole_token() {
    // 退格：光标在记号右边界，一下删掉整块，且**不**吞掉旁边的空白。
    let mut input = with_token("改 @src/a.rs 吧", 2, 11);
    input.end();
    input.left();
    input.left();
    assert_eq!(input.cursor(), 11);
    input.backspace();
    assert_eq!(input.text(), "改  吧");
    assert_eq!(input.cursor(), 2);

    // 删除：光标在左边界，一下删掉整块。
    let mut input = with_token("改 @src/a.rs 吧", 2, 11);
    input.home();
    input.right();
    input.right();
    assert_eq!(input.cursor(), 2);
    input.delete_forward();
    assert_eq!(input.text(), "改  吧");

    // `Ctrl-W`：光标贴着记号右边界时，抹词抹到的是整块。
    let mut input = with_token("改 @src/a.rs 吧", 2, 11);
    input.end();
    input.left();
    input.left();
    input.kill_word();
    assert_eq!(input.text(), "改  吧");

    // 没有记号时，删除照旧一格一格。
    let mut plain = typed("look /tmp/x");
    plain.end();
    plain.backspace();
    assert_eq!(plain.text(), "look /tmp/");
}

#[test]
fn a_token_that_wraps_keeps_its_style_across_the_fold() {
    let mut input = typed("@src/render/tui.rs");
    input.set_token_spans(vec![editor::TokenSpan {
        start: 0,
        end: 18,
        style: Style::default().fg(Color::Magenta),
    }]);
    let (rows, _) = input.view(5, 10);
    let mut seen = 0;
    for row in &rows {
        // 第一个 span 是引子（提示符或缩进），正文从第二个起。
        for span in row.spans.iter().skip(1) {
            if span.content.is_empty() {
                continue;
            }
            seen += span.content.chars().count();
            assert_eq!(span.style.fg, Some(Color::Magenta), "折行之后样式还在");
        }
    }
    assert_eq!(seen, 18, "整块都上色了");
}
