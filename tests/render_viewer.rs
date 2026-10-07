//! 文件查看器那一屏的绘制：外来屏幕（`vt100` 的网格）搬进 ratatui 的缓冲。
//!
//! 进程那一半（spawn、pty、应答层）不在这里：测试不该起一个编辑器
//! （`.scratch/nvim-file-viewer/spec.md`）。这里只问 [`ScreenWidget`] —— 它是把
//! 「nvim 画了什么」变成「终端上长什么样」的那一层，而这一层的输入可以手写。
//!
//! 底色的形态抄的是真机：`nvim -M -R -c 'set nowrap' --cmd 'set mouse=a'` 在
//! `TERM=xterm-256color` 的 pty 里跑一帧，80×24 的网格上 974 个空格里有 853 个
//! **有文字为空的格子带着背景色**（`Rgb(26,27,38)`）—— 它铺底色的方式正是
//! 「设好背景、再用 `EL` / `ED` 擦」。

use heng::render::viewer::ScreenWidget;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::widgets::Widget;

/// nvim 那一屏的底色。
const BG: Color = Color::Rgb(26, 27, 38);

/// 把一屏外来画面画进一块新缓冲，回那一块。
fn painted(bytes: &[u8], area: Rect) -> Buffer {
    let mut parser = vt100::Parser::new(area.height, area.width, 0);
    parser.process(bytes);
    let mut buf = Buffer::empty(area);
    ScreenWidget {
        screen: parser.screen(),
    }
    .render(area, &mut buf);
    buf
}

#[test]
fn the_alien_screen_paints_its_background_where_there_is_no_text() {
    // 整屏擦一遍：那些格子**一个字都没有**，但背景色是 nvim 的。少了它们，浮层就
    // 只剩文字处有底色、空白处透出底下的转录 —— 而「没有框线，nvim 自己那块底色
    // 就是边界」（`.scratch/nvim-file-viewer/spec.md` §3）正是靠这个成立。
    let area = Rect::new(0, 0, 10, 3);
    let buf = painted(b"\x1b[48;2;26;27;38m\x1b[2J", area);

    for row in 0..area.height {
        for col in 0..area.width {
            let cell = buf.cell(Position::new(col, row)).expect("在缓冲里");
            assert_eq!(cell.bg, BG, "({col},{row}) 那一格没有文字，但底色该铺到它");
        }
    }
}

#[test]
fn the_alien_screen_paints_the_background_to_the_end_of_a_line() {
    // 擦到行尾（`EL`）是 nvim 每一帧都会发的形态：光标处往后全是被擦出来的空格。
    let area = Rect::new(0, 0, 8, 1);
    let buf = painted(b"\x1b[48;2;26;27;38m\x1b[K", area);

    for col in 0..area.width {
        assert_eq!(
            buf.cell(Position::new(col, 0)).expect("在缓冲里").bg,
            BG,
            "第 {col} 列被擦掉了，底色仍该在"
        );
    }
}

#[test]
fn the_alien_screen_still_paints_its_text() {
    // 铺底色不能把文字吃掉：符号、前景色、粗体三样都要照搬
    // （那是 `ScreenWidget` 本来就做的事）。
    let area = Rect::new(0, 0, 6, 1);
    let buf = painted(b"\x1b[48;2;26;27;38m\x1b[38;2;200;100;50m\x1b[1mhi", area);

    let cell = buf.cell(Position::new(0, 0)).expect("在缓冲里");
    assert_eq!(cell.symbol(), "h", "第一个字画出来");
    assert_eq!(cell.fg, Color::Rgb(200, 100, 50), "前景色跟着来");
    assert!(
        cell.modifier.contains(Modifier::BOLD),
        "粗体跟着来：{cell:?}"
    );
    assert_eq!(cell.bg, BG, "底下的底色也在");
    assert_eq!(
        buf.cell(Position::new(1, 0)).expect("在缓冲里").symbol(),
        "i"
    );
}

#[test]
fn a_wide_character_is_not_cut_in_half() {
    // 宽字符在 `vt100` 里占两格、第二格是空的：覆盖它会把这个字擦掉一半
    // （`ScreenWidget` 的注释说的就是这条）。
    let area = Rect::new(0, 0, 6, 1);
    let mut bytes = b"\x1b[48;2;26;27;38m\x1b[38;2;200;100;50m".to_vec();
    bytes.extend_from_slice("中".as_bytes());
    let buf = painted(&bytes, area);

    let cell = buf.cell(Position::new(0, 0)).expect("在缓冲里");
    assert_eq!(cell.symbol(), "中", "宽字画在前一格");
    assert_eq!(cell.bg, BG, "它也跟着底色");
    let tail = buf.cell(Position::new(1, 0)).expect("在缓冲里");
    assert!(
        !tail.symbol().is_empty(),
        "第二格该留给宽字的后半格，而不是被写成空串：{tail:?}"
    );
}
