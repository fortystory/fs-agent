//! The input editor, tested as a state machine: wrapping, the cursor, the keymap.
//!
//! The seam is [`editor::Input`]'s own API. That is deliberate: the cursor's only
//! state is a character index, so "where does the cursor go" is a pure question a
//! test can ask without a terminal — which is exactly what the inline viewport
//! made impossible (ADR 0002).

use fs_agent::render::editor::{self, Input};

/// An editor holding `text`, with the cursor at the end — what typing it leaves.
fn typed(text: &str) -> Input {
    let mut input = Input::new();
    input.insert_str(text);
    input
}

/// The rows a frame would draw, as plain strings.
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
    // `> ` leads the first row, two spaces lead every row after it, and the text
    // wraps at five columns: the prompt and the indent are the same width, so every
    // row holds the same amount of text (spec §2, §5).
    assert_eq!(rows(&typed("abc"), 5, 10), vec!["> abc"]);
    assert_eq!(rows(&typed("abcdefgh"), 5, 10), vec!["> abcde", "  fgh"]);
    // A newline is a row break of its own, not a wrap.
    assert_eq!(rows(&typed("ab\ncd"), 5, 10), vec!["> ab", "  cd"]);
    // An empty draft is still one row: the prompt is always there to type into.
    assert_eq!(rows(&Input::new(), 5, 10), vec!["> "]);
    assert_eq!(rows(&typed("ab\n"), 5, 10), vec!["> ab", "  "]);
}

#[test]
fn a_wide_character_takes_two_columns_in_the_draft() {
    // The same column arithmetic as the transcript: `你` is three bytes and two
    // columns, and counting bytes would wrap it three times too early.
    assert_eq!(rows(&typed("你好世界"), 6, 10), vec!["> 你好世", "  界"]);
    // Exactly filling a row keeps the cursor on that row's last cell, as with ASCII.
    assert_eq!(rows(&typed("你好世"), 6, 10), vec!["> 你好世"]);
}

#[test]
fn the_cursor_maps_onto_the_row_it_is_typed_on() {
    // Cursor at the end of `abcdefgh` wrapped at five columns: second row, after
    // `fgh`.
    let input = typed("abcdefgh");
    let (_, cursor) = input.view(5, 10);
    assert_eq!(cursor.row, 1);
    assert_eq!(cursor.column, 2 + 3);

    // Home puts it at the head of the whole draft.
    let mut input = input;
    input.home();
    let (_, cursor) = input.view(5, 10);
    assert_eq!((cursor.row, cursor.column), (0, 2));

    // A full row with the cursor at its end keeps it on the last cell — a terminal's
    // pending wrap — rather than opening a row of its own and pushing the draft down.
    let input = typed("abcde");
    assert_eq!(input.height(5), 1);
    let (_, cursor) = input.view(5, 10);
    assert_eq!(
        (cursor.row, cursor.column),
        (0, 2 + 4),
        "the last cell of the row"
    );

    // And an empty draft puts it right after the prompt.
    let (_, cursor) = Input::new().view(5, 10);
    assert_eq!((cursor.row, cursor.column), (0, 2));
}

#[test]
fn the_prompt_and_the_indent_are_the_same_width() {
    // The layout reserves columns from one constant and the editor draws the
    // prompt; they have to agree or every wrapped row is one column off.
    assert_eq!(
        editor::prompt_columns() as usize,
        fs_agent::render::width::text_columns(editor::PROMPT)
    );
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
    input.insert_char('\n'); // what Ctrl-J does
    input.insert_str("cd");
    assert_eq!(input.text(), "ab\ncd");

    // Left from the head of the second line steps onto the end of the first.
    input.home();
    let (_, cursor) = input.view(80, 10);
    assert_eq!(
        (cursor.row, cursor.column),
        (1, 2),
        "home is the line's head"
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
        "backspace at a line start joins upward"
    );

    let mut input = typed("ab\ncd");
    input.home();
    input.up();
    input.end();
    input.delete_forward();
    assert_eq!(
        input.text(),
        "abcd",
        "delete at a line end pulls the next up"
    );
}

#[test]
fn the_emacs_chords_stay_inside_the_cursor_line() {
    let mut input = typed("one\ntwo three");
    input.kill_to_line_start();
    assert_eq!(
        input.text(),
        "one\n",
        "Ctrl-U takes the line, not the draft"
    );

    let mut input = typed("one\ntwo three");
    input.home();
    input.kill_to_line_end();
    assert_eq!(input.text(), "one\n", "Ctrl-K takes the rest of the line");

    let mut input = typed("one\ntwo three");
    input.kill_word();
    assert_eq!(
        input.text(),
        "one\ntwo ",
        "Ctrl-W erases one word inside it"
    );
}

#[test]
fn up_and_down_hold_the_visual_column_across_a_short_line() {
    let mut input = typed("abcdef\nab\nabcdef");
    // `home` is the line's head, so walk up to the first line first.
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
        "clamped to the short line"
    );
    input.down();
    assert_eq!(input.view(80, 10).1.column, 2 + 4, "the goal survived it");
    input.up();
    input.up();
    assert_eq!(input.view(80, 10).1.column, 2 + 4);
    // The first line has nowhere further up to go.
    input.up();
    assert_eq!(input.view(80, 10).1.row, 0);
}

#[test]
fn a_draft_taller_than_the_area_scrolls_to_keep_the_cursor_in_view() {
    let mut input = typed(&"x".repeat(399));
    let height = 5;
    let (rows, cursor) = input.view(10, height);
    assert_eq!(rows.len(), height as usize, "the view is exactly the area");
    assert_eq!(
        cursor.row,
        height - 1,
        "the cursor is on the last visible row"
    );
    assert_eq!(cursor.column, 2 + 9);

    input.home();
    let (rows, cursor) = input.view(10, height);
    assert_eq!(
        (cursor.row, cursor.column),
        (0, 2),
        "home scrolls the head back"
    );
    let head: String = rows[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(head, "> xxxxxxxxxx", "ten columns of text fit the row");
}

#[test]
fn the_arrows_do_not_walk_the_history() {
    let mut input = Input::new();
    input.insert_str("first");
    input.submitted();
    input.insert_str("draft");
    // Up and down are the cursor's; they cannot recall anything.
    input.up();
    input.down();
    assert_eq!(input.text(), "draft");
    input.history_previous();
    assert_eq!(input.text(), "first");
    input.history_next();
    assert_eq!(input.text(), "draft", "the fresh draft came back");
}

#[test]
fn a_multi_line_draft_is_recalled_whole_and_keeps_its_blank_lines() {
    let mut input = Input::new();
    input.insert_str("第一行\n\n第二行");
    assert_eq!(
        input.submitted(),
        "第一行\n\n第二行",
        "only the ends are trimmed"
    );
    input.insert_str("next");
    input.history_previous();
    assert_eq!(input.text(), "第一行\n\n第二行");
}
