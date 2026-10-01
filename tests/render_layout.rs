//! 全屏外壳画进一个 `TestBackend`，于是几何不用终端也能断言
//! （`.scratch/tui-sidebar/spec.md` §1–§2，
//! `Testing Decisions`）。
//!
//! 接缝是 [`draw_frame`]：一个状态进去，一块定尺的缓冲区出来。
//! 下面每一条断言说的都是人看得见的东西 —— 有哪些区域、左栏那一页
//! 写着什么、回合条上的格在哪、挤得下几条提示 —— 从不涉及布局
//! 在路上算出来的那些矩形。

use fs_agent::render::editor;
use fs_agent::render::width::text_columns;
use fs_agent::render::{
    draw_frame, wording, CatalogEntry, ConsoleRequest, FrontEndEvent, Key, RenderEvent,
    SessionFacts, TuiState, PULSE_PALETTE,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

fn facts() -> SessionFacts {
    SessionFacts {
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: "~/code/fortystory/fs-agent".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        // 会话被组装时所处的模式：状态行里 `模式 …` 那一栏。想测另一档的
        // 测试在自己的 facts 里覆盖它。
        mode: fs_agent::permissions::Mode::Ask,
        budget_limit: Some(100_000),
        speaker_order: Vec::new(),
    }
}

fn state() -> TuiState {
    TuiState::new(facts())
}

/// 指名册的会话用的注入 facts：一个名字是单口会话，
/// 两个名字就是一场讨论。
fn facts_with_roster(names: &[&str]) -> SessionFacts {
    let mut facts = facts();
    facts.speaker_order = names.iter().map(|name| (*name).to_owned()).collect();
    facts
}

/// 循环**在等一行**的那个状态 —— 空闲的前端就是这个样子。
///
/// 这个区分要紧：一次运行当中，`Esc` 与 `Ctrl-C` 是取消手势，而不是
/// 「关掉这个」/「退出」（spec §6），而「在跑」这件事由循环来说
/// （`ConsoleRequest::RunState`）。全新的状态本来就是空闲的；提示请求
/// 加上的是给这些测试打字用的活编辑器。空闲 UI 的那些测试 —— `/` 菜单、
/// `Esc` 会清掉的草稿、提示阶梯 —— 说的都是这个等待中的状态。
fn idle() -> TuiState {
    let mut state = state();
    let (reply, line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    drop(line);
    state
}

/// 按固定尺寸画一帧，再把屏幕读回来，一行一段文本。
fn screen(width: u16, height: u16, state: &mut TuiState) -> Vec<String> {
    let frame = buffer(width, height, state);
    (0..height).map(|y| row_text(&frame, y, width)).collect()
}

/// 某一行的文本，取它在帧里两列之间的那一段。
///
/// 一个宽字素会盖住它后面那一格，而那一格在后端缓冲里是一个空格；
/// 照字面读出来会把 `终端` 拼成 `终 端`。按符号的显示宽度前进，
/// 文本才读得像终端。
fn cells(frame: &Buffer, y: u16, from: u16, to: u16) -> String {
    let mut text = String::new();
    let mut x = from;
    while x < to {
        let symbol = frame[(x, y)].symbol();
        text.push_str(symbol);
        x += symbol.cell_width().max(1);
    }
    text
}

/// 缓冲的一整行，作为文本。
fn row_text(buffer: &Buffer, y: u16, width: u16) -> String {
    cells(buffer, y, 0, width)
}

/// 转录的第一行。外框走了之后它就是终端的第一行
/// （`.scratch/tui-chrome/spec.md` §1）。
const TRANSCRIPT_TOP: usize = 0;

/// 一帧画出来之后转录占的那些行：第一条横线画在输入区上面，
/// 而它和转录之间还夹着状态行那一行。
///
/// 量出来的，不是记住的，因为终端高度、左栏的高度阶梯
/// 与草稿三者都会把它挪动。外框与状态行上方那条线都走了之后，
/// 那条横线不再有端点交叉符，所以探针找的是延伸到屏幕右缘的
/// 那条虚线本身（页签条的两条横线到分隔列就结束，不会以它结尾）。
fn transcript_rows(rows: &[String]) -> usize {
    rows.iter()
        .position(|row| row.ends_with('┄'))
        .expect("主列的第一条横线在屏幕上")
        - TRANSCRIPT_TOP
        - 1
}

/// 画出来的这一帧本身，用来断言某个具体的格子。
fn buffer(width: u16, height: u16, state: &mut TuiState) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("TestBackend");
    terminal
        .draw(|frame| draw_frame(frame, state))
        .expect("画一帧");
    terminal.backend().buffer().clone()
}

#[test]
fn a_terminal_below_the_minimum_shows_one_centred_notice() {
    // 39x24 比下限少一列；40x9 少一行。两种都只画
    // 那句提示，别的什么都不画 —— 不画半截的窗格（spec §2）。
    for (width, height) in [(39, 24), (40, 9)] {
        let rows = screen(width, height, &mut state());
        let text = rows.join("\n");
        assert!(
            text.contains("终端太小：至少 40×10"),
            "{width}x{height} 说明了它为什么不画：{text}"
        );
        assert!(
            !text.contains('┌') && !text.contains('└'),
            "{width}x{height} 一个窗格都没画：{text}"
        );
        let notice = rows
            .iter()
            .position(|row| row.contains("终端太小"))
            .expect("提示在某一行上");
        assert_eq!(
            notice,
            height as usize / 2,
            "{width}x{height} 把提示纵向居中"
        );
    }
}

#[test]
fn a_wide_terminal_draws_the_mark_the_sidebar_and_the_main_column() {
    // 120x24 是参考尺寸：一圈帧，一条全高左栏带着标记、
    // 页签条与六项读数，外加主列的转录、状态行、
    // 输入区与提示行 —— 它们之间不留一条空行
    // （`.scratch/tui-sidebar/spec.md` §1）。
    let rows = screen(120, 24, &mut state());

    // 四周没有边框：外框已经离开（`.scratch/tui-chrome/spec.md` §1），
    // 终端自己就是边界。
    assert!(
        !rows.join("\n").contains(['┌', '┐', '└', '┘']),
        "屏幕上没有外框：{rows:#?}"
    );

    // 左栏：宽档上的标记，居中，两侧各留一列空气。它从顶上留的
    // 那一行空行下面开始（2026-10-01 真机反馈）。
    assert!(
        rows[1].contains("▄▀▀█") && rows[5].contains("▀▀▀"),
        "标记的首尾两行就是左栏的首尾两行：{:?} / {:?}",
        rows[1],
        rows[5]
    );

    // 页签条：上下两条横线夹着三个标签，两条横线从屏幕左缘
    // 一直画到分隔列。
    assert_eq!(
        rows[6].trim_matches(|ch| ch == '┄' || ch == '┆' || ch == ' '),
        "",
        "页签条的上横线横跨整条左栏：{:?}",
        rows[6]
    );
    assert!(
        rows[7].contains("调用量") && rows[7].contains("轨迹") && rows[7].contains("文件"),
        "三个页签占自己那一行：{:?}",
        rows[7]
    );

    // 分隔线跑满屏幕的整个高度。24 行终端留给转录十七行：
    // 三行归输入区的地板、四行归外壳 —— 外框的两行与
    // 状态行上方那一行都还给了内容
    // （`.scratch/tui-chrome/spec.md` §1–§2）。
    assert_eq!(transcript_rows(&rows), 17, "120x24 给转录 17 行");
    assert!(
        !rows[16].contains('┄'),
        "状态行上方那条横线已经不画了：{:?}",
        rows[16]
    );
    assert!(
        rows[17].contains("模型 claude-sonnet-4-5")
            && rows[17].contains("模式 询问")
            && rows[17].contains("上下文 —"),
        "状态行报出模型、模式与占比：{:?}",
        rows[17]
    );
    assert!(
        rows[18].ends_with('┄'),
        "输入区上面那条横线横跨主列：{:?}",
        rows[18]
    );
    assert!(
        rows[19].contains(&format!("┆{}", editor::PROMPT)),
        "输入区的第一行带着提示符：{:?}",
        rows[19]
    );
    assert!(
        rows[20].trim_matches(['┆', ' ']).is_empty()
            && rows[21].trim_matches(['┆', ' ']).is_empty(),
        "它下面两行是同一个框里的空行：{:?} / {:?}",
        rows[20],
        rows[21]
    );
    assert!(
        rows[23].contains("ctrl-c"),
        "提示行列出了出口：{:?}",
        rows[23]
    );

    assert!(
        !rows.join("\n").contains("shift+enter"),
        "没有幽灵般的换行键"
    );
    // 外壳上不再有工作目录、也不再有钟（spec §8）。
    assert!(
        !rows.join("\n").contains("~/code"),
        "目录不在屏幕上：{rows:#?}"
    );
}

#[test]
fn the_wide_sidebar_is_forty_columns_and_centres_the_mark() {
    // 宽档是 40 列，标记 38 列，所以两侧各得一列
    // 空气（spec §2）。分隔线占自己那一列，在第 40 列 ——
    // 外框走了之后，它就是最左那一列内容加四十。
    let frame = buffer(120, 24, &mut state());
    assert_eq!(frame[(40, 0)].symbol(), "┆", "分隔线从屏幕顶起");
    assert_eq!(frame[(40, 23)].symbol(), "┆", "一直画到屏幕底");
    // 标记从顶上留的那一行空行**下面**开始：第 1 行。
    assert_eq!(frame[(0, 1)].symbol(), " ", "左边一列空气");
    assert_eq!(frame[(1, 1)].symbol(), "▄", "然后是标记");
    // 标记宽 38 列：2 + 38 = 40，所以最后一列空气在 39，
    // 分隔线在 40。
    assert_eq!(frame[(39, 1)].symbol(), " ", "右边也有一列");
}

#[test]
fn the_mark_is_lit_from_above_and_only_on_the_wide_rung() {
    // 静止的标记 —— 画家负责的那一半：字符是 `wording` 的，
    // 它们落在哪几行、渐变怎么下落却是画家的，所以在
    // 看得见它的地方逐格断言，就在缓冲里。动的那一半是
    // `the_mark_walks_the_pulse_ring_while_a_run_is_in_flight`
    // （`.scratch/tui-input-pulse/spec.md` §2）。
    let frame = buffer(120, 24, &mut state());
    assert_eq!(
        frame[(1, 1)].symbol(),
        "▄",
        "标记的第一行起于左栏的第一个内容行"
    );
    assert_eq!(
        frame[(1, 1)].fg,
        Color::LightMagenta,
        "标记的顶端是亮的那一头"
    );
    assert_eq!(
        frame[(1, 5)].fg,
        Color::Magenta,
        "而最下面那一行是暗的那一头"
    );

    // 标记是宽档独占的：100 列拿到的是 28 列宽的左栏
    // （标记塞不进去），60 列干脆没有左栏。
    for (width, height, identity) in [
        (100, 24, true),
        (80, 24, true),
        (60, 24, false),
        (40, 24, false),
    ] {
        let mut fresh = state();
        let rows = screen(width, height, &mut fresh);
        assert!(
            !rows.join("\n").contains('▄'),
            "{width}x{height} 在标记的那一档之下：{:#?}",
            rows[1]
        );
        assert_eq!(
            rows.join("\n").contains("fs-agent"),
            identity,
            "{width}x{height} 在画左栏的地方显示的是文字身份：{:#?}",
            rows[1]
        );
    }
}

#[test]
fn the_sidebar_has_two_widths_and_a_hidden_third() {
    // 阶梯只由宽度决定 —— 左栏是独立的一列，所以它的高度
    // 不归转录花（spec §2）。120 往上算宽档，
    // 80 到 119 算窄档，80 以下整条左栏不见。
    for (width, divider) in [
        (174, 40u16),
        (120, 40),
        (100, 28),
        (80, 28),
        (79, 0),
        (40, 0),
    ] {
        let frame = buffer(width, 24, &mut state());
        let text: String = (0..24)
            .map(|y| row_text(&frame, y, width))
            .collect::<Vec<_>>()
            .join("\n");
        if divider == 0 {
            assert!(!text.contains('┆'), "{width} 列时没有左栏：{text}");
        } else {
            assert_eq!(
                frame[(divider, 0)].symbol(),
                "┆",
                "{width} 列时分隔线在第 {divider} 列"
            );
            // 主列的横线从分隔线起，到屏幕右缘为止，而且它盖在
            // 分隔列上（横线后画）。
            assert_eq!(
                frame[(divider, 18)].symbol(),
                "┆",
                "主列的横线挨着它画，竖线仍然贯通：{text}"
            );
        }
    }

    // 左栏在 80x14 下也画 —— 四行字段不再是地板了，
    // 因为左栏自己的高度就是终端高度减掉顶上那一行留白。
    let smallest = buffer(80, 14, &mut state());
    assert_eq!(smallest[(28, 0)].symbol(), "┆", "左栏在 80x14 下也画");
}

#[test]
fn the_main_rules_stop_short_of_the_divide_column() {
    // 竖线要从屏幕顶贯通到底：主列那两条横线从分隔列**右边一格**起画，
    // 把分隔列那一格留给 `┆`（2026-10-01 真机反馈 —— 原先横线把竖线
    // 截成了三截）。
    let frame = buffer(120, 24, &mut state());
    let divide = 40u16;
    for y in [18u16, 22] {
        assert_eq!(
            frame[(divide, y)].symbol(),
            "┆",
            "第 {y} 行的分隔列仍是竖线"
        );
        assert_eq!(
            frame[(divide + 1, y)].symbol(),
            "┄",
            "而横线紧跟在它右边：第 {y} 行"
        );
    }

    // 没有左栏时没有竖线要让，横线从屏幕左缘起。
    let narrow = buffer(60, 24, &mut state());
    for y in [18u16, 22] {
        assert_eq!(narrow[(0, y)].symbol(), "┄", "60 列下横线从第 0 列起");
    }
}

#[test]
fn a_floor_sized_terminal_still_draws_the_main_column() {
    // 40x10 在下限之内：左栏被藏起来，主列仍然
    // 有它的转录、状态行、输入区与提示行（spec §2）。
    let rows = screen(40, 10, &mut state());
    for (row, line) in rows.iter().enumerate() {
        assert!(
            !line.contains(['┌', '┐', '└', '┘']),
            "第 {row} 行没有外框：{line:?}"
        );
    }
    // 外框与状态行上方那条线都走了之后，40x10 的地板宽裕了：
    // 输入区拿满三行，转录还留三行
    // （`.scratch/tui-chrome/spec.md` §1–§2）。输入区是第 5 到 7 行，
    // 它上面那条横线在第 4 行，提示行在第 9 行。
    assert_eq!(transcript_rows(&rows), 3, "三行转录：{rows:#?}");
    assert!(
        rows[5].starts_with(editor::PROMPT),
        "输入区的第一行是提示符那一行：{:?}",
        rows[5]
    );
    assert!(
        rows[6].trim_matches(' ').is_empty(),
        "它下面还有一行空白：{:?}",
        rows[6]
    );
    assert!(
        rows[3].contains("模式 询问") && rows[3].contains("上下文 —"),
        "在地板上状态行先让出模型：{:?}",
        rows[3]
    );
    assert!(
        !rows[3].contains("claude-sonnet"),
        "这是它丢掉的第一个东西：{:?}",
        rows[3]
    );
    assert!(rows[9].contains("ctrl-c"), "提示行：{:?}", rows[9]);
    assert!(
        !rows.join("\n").contains("fs-agent"),
        "左栏整条藏起来：{rows:#?}"
    );
}

/// 状态照原样时，提示行里有多少个 `·` 分隔的项。
fn hint_items_of(width: u16, state: &mut TuiState) -> Vec<String> {
    let rows = screen(width, 24, state);
    let row = rows
        .iter()
        .find(|row| row.contains("ctrl-c"))
        .expect("提示行在屏幕上");
    row.trim_matches(['┆', ' '])
        .split(" · ")
        .map(str::to_owned)
        .collect()
}

/// 一个**在等一行**的会话的提示行 —— 只有这时它才敢承诺
/// `enter 发送`。
fn hint_items(width: u16) -> Vec<String> {
    let mut state = state();
    let (reply, _line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    hint_items_of(width, &mut state)
}

/// 同上，但有一次运行进行中：出口变成 `ctrl-c 退出`，短七列，
/// 120 列下把那个状态词换了回来。
fn hint_items_busy(width: u16) -> Vec<String> {
    let mut state = state();
    let (reply, _line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    state.request(ConsoleRequest::RunState { running: true });
    hint_items_of(width, &mut state)
}

#[test]
fn a_session_with_no_line_being_read_promises_only_what_the_keyboard_does() {
    // 回合进行中，或者在一次性的 `discuss` 里：没人在读行，所以 `enter
    // 发送` 就是假话，模式手势也没有东西可切。
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    let rows = screen(120, 24, &mut state);
    let row = rows
        .iter()
        .find(|row| row.contains("ctrl-c"))
        .expect("提示行在屏幕上");
    assert!(row.contains("esc 取消"), "{row}");
    assert!(row.contains("PgUp/PgDn 滚动"), "{row}");
    assert!(!row.contains("enter"), "{row}");
    assert!(!row.contains("shift+tab"), "{row}");
}

#[test]
fn the_hint_row_gives_up_hints_before_it_gives_up_the_way_out() {
    // 提示行的宽度是**主列的**内容宽度，不是终端的
    // 宽度：左栏那几列不是能送出去的提示（`tui-sidebar` spec
    // §2）。所以量出来的空闲阶梯是：40 列 -> 一条提示 + 出口
    // + 状态词，80 -> 两条，100 -> 三条，120 -> 四条（而且没有状态
    // 词：四条提示加出口再没地方留它），174 -> 五条。
    assert_eq!(hint_items(40).len(), 3, "40 列：{:?}", hint_items(40));
    assert_eq!(hint_items(80).len(), 3, "80 列：{:?}", hint_items(80));
    assert_eq!(hint_items(100).len(), 5, "100 列：{:?}", hint_items(100));
    assert_eq!(hint_items(120).len(), 5, "120 列：{:?}", hint_items(120));
    assert_eq!(hint_items(174).len(), 7, "174 列：{:?}", hint_items(174));

    // 在地板上，出口之前只挤得下一条提示，状态词却还在 ——
    // 40 列终端让出的是再上一档的换行提示。
    let floor = hint_items(40);
    assert_eq!(floor[0], "就绪", "40 列下状态词还挤得下：{floor:?}");
    assert_eq!(floor[1], "enter 发送", "然后是发送提示：{floor:?}");
    assert!(
        !floor.contains(&"ctrl-j 换行".to_owned()),
        "宽度的代价就是换行提示：{floor:?}"
    );
    assert_eq!(floor.last().unwrap(), "ctrl-c/ctrl-d 退出");

    // 80 列是窄档左栏那一档，所以它能用的提示列比光秃秃的 80 列
    // 终端还少：它的 29 列归左栏和那条分隔线。
    // 挤得下两条提示，让出去的是状态词。
    let narrow = hint_items(80);
    assert_eq!(narrow[0], "enter 发送", "80 列下没有状态词：{narrow:?}");
    assert!(
        !narrow.contains(&"就绪".to_owned()),
        "状态词是第一个被让出去的：{narrow:?}"
    );
    assert_eq!(narrow.last().unwrap(), "ctrl-c/ctrl-d 退出");

    // 120 列是参考尺寸，它钉住一个刻意的结果：四条提示加出口挤得下，
    // 而 `就绪` 挤不下（spec §2 —— 「这是预期行为，不是
    // bug」）。忙碌会话的出口短七列，于是把状态词
    // 换了回来。
    let wide = hint_items(120);
    assert_eq!(
        wide,
        vec![
            "enter 发送",
            "ctrl-j 换行",
            "esc 取消",
            "shift+tab 模式",
            "ctrl-c/ctrl-d 退出",
        ],
        "四条提示加出口，没有状态词"
    );
    let busy = hint_items_busy(120);
    assert_eq!(
        busy.first().map(String::as_str),
        Some("工作中"),
        "忙碌时的出口更短，所以状态词挤得下：{busy:?}"
    );

    // 174 列能把每条提示和状态词一起放下。
    let roomy = hint_items(174);
    assert_eq!(roomy[0], "就绪", "状态词回来了：{roomy:?}");
    assert!(
        roomy.contains(&"PgUp/PgDn 滚动".to_owned()),
        "后面还跟着整份提示表：{roomy:?}"
    );
    assert!(
        !screen(174, 24, &mut state())
            .join("\n")
            .contains("shift+enter"),
        "任何宽度下都没有幽灵换行键"
    );
}

#[test]
fn the_transcript_pane_shows_both_the_notices_and_the_streaming_tail() {
    use fs_agent::events::SpeakerId;
    use fs_agent::render::{DeltaKind, RenderEvent};

    let mut state = state();
    state.apply(RenderEvent::Notice(
        "fs-agent：会话 abc · 模型 m · 模式 询问 · /tmp/x".to_owned(),
    ));
    state.apply(RenderEvent::Delta {
        speaker: SpeakerId::Debater("kimi".into()),
        kind: DeltaKind::Text,
        text: "正在读文件".to_owned(),
    });

    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("fs-agent：会话 abc"),
        "提示是转录里的一行：{text}"
    );
    assert!(text.contains("正在读文件"), "流式的尾巴也在窗格里：{text}");
}

/// 标记在 120x24 下的五行颜色，自上而下：画家写的那一列
/// 是左栏的第二列，标记的第一个字形在它每一行上都坐在
/// 那里。
fn mark_colours(state: &mut TuiState) -> Vec<Color> {
    let frame = buffer(120, 24, state);
    (1..=5u16).map(|y| frame[(1, y)].fg).collect()
}

/// 人眼里看到的标记短横格：`fs-agent` 那条短横所在的四列
/// 里的五行，从 120x24 的一帧上读出来。
///
/// 标记从左栏第二列起，它的短横格从往里十列处
/// 开始（`fs-agent` 是八个字形格、每格四列，中间隔一空列）。
fn dash_cell(state: &mut TuiState) -> Vec<String> {
    let frame = buffer(120, 24, state);
    (1..=5u16)
        .map(|y| cells(&frame, y, 1 + 10, 1 + 14))
        .collect()
}

#[test]
fn the_mark_does_not_move_while_a_run_is_in_flight() {
    // 票 08 把下落的短横关掉了：标记又静止了，这正是维护者在真终端上
    // 看过之后要的。会让它动的那些代码留着（并在它所在之处继续单测），
    // 所以这里是把它按住的断言 ——
    // 一次运行进行中时逐格打出来的帧必须完全一样。
    // 假如横条真动过（上移一行或下移一行），那一格会长的样子。
    let moved = [
        ["▀▀▀▀", "    ", "    ", "    ", "    "],
        ["    ", "    ", "    ", "    ", "▀▀▀▀"],
    ];
    let mut state = state();
    let still = dash_cell(&mut state);
    assert_eq!(still[2], "▀▀▀▀", "标记停在它一直在画的那一行上：{still:?}");
    assert_eq!(
        still,
        fs_agent::render::wording::logo_lines()
            .map(|row| row.chars().skip(10).take(4).collect::<String>())
            .to_vec(),
        "而且那一行逐字节等于 `logo_lines` 里的那一行"
    );

    state.request(ConsoleRequest::RunState { running: true });
    for frame in 0..8 {
        state.tick();
        assert_eq!(
            dash_cell(&mut state),
            still,
            "运行中的第 {frame} 帧：标记没有动"
        );
        // 而且动的也不是颜色：色带没被碰过。
        assert_eq!(
            mark_colours(&mut state),
            vec![
                Color::LightMagenta,
                Color::LightMagenta,
                Color::LightMagenta,
                Color::LightMagenta,
                Color::Magenta
            ],
            "颜色也没变：第 {frame} 帧"
        );
    }
    for shape in &moved {
        assert_ne!(
            dash_cell(&mut state),
            shape.map(str::to_owned).to_vec(),
            "横条从不落在别的行上：下落是关着的"
        );
    }
}

/// 提示符画在哪：那个装着 `❱` 的格子，按人在屏幕上找它的方式找到，
/// 而不是从一块这个测试还得跟布局同步的矩形里找。
fn prompt_at(width: u16, height: u16, state: &mut TuiState) -> (u16, u16) {
    let frame = buffer(width, height, state);
    for y in 0..height {
        for x in 0..width {
            if frame[(x, y)].symbol() == "❱" {
                return (x, y);
            }
        }
    }
    panic!("提示符在屏幕上");
}

/// 输入区第一行里提示符那一格：它的符号，以及它被涂上的颜色。
fn prompt_cell(state: &mut TuiState) -> (String, Color) {
    let frame = buffer(120, 24, state);
    let (x, y) = prompt_at(120, 24, state);
    (frame[(x, y)].symbol().to_owned(), frame[(x, y)].fg)
}

#[test]
fn the_prompt_is_an_angle_bracket_that_holds_still_while_you_type() {
    // 提示符的颜色是这个界面的动画，维护者给它定的规矩是
    // 一场长争论的结论：**agent 干活时它动，用户打字时它
    // 停**（`.scratch/tui-input-pulse/spec.md` §2b，票 09）。静止色
    // 是第 0 帧 —— 他们那条脚本起步的那一帧 —— 所以每次键盘回到
    // 他们手里都是同一个颜色。
    let mut state = state();
    let (symbol, colour) = prompt_cell(&mut state);
    assert_eq!(symbol, "❱", "提示符字形");
    assert_eq!(
        colour,
        Color::Rgb(216, 97, 97),
        "在等一行的提示符穿的是那条脚本的第一个颜色"
    );

    // 打字不会惊动它：一个 tick 也没把颜色挪离静止帧。
    for frame in 0..5 {
        state.tick();
        assert_eq!(
            prompt_cell(&mut state).1,
            colour,
            "键盘归写的人时第 {frame} 个 tick：什么都没动"
        );
    }
}

#[test]
fn the_prompts_colour_walks_the_wheel_while_a_run_is_in_flight() {
    // 那条规矩的另一半：一旦有回合进行中颜色就动起来，而且它的
    // 任何两帧都不穿同一个颜色。
    let mut state = state();
    let resting = prompt_cell(&mut state).1;
    state.request(ConsoleRequest::RunState { running: true });
    let mut seen = Vec::new();
    for _ in 0..5 {
        state.tick();
        seen.push(prompt_cell(&mut state).1);
    }
    let mut unique = seen.clone();
    unique.sort_by_key(|colour| format!("{colour:?}"));
    unique.dedup();
    assert_eq!(
        unique.len(),
        seen.len(),
        "一次运行的每一帧都有自己的颜色：{seen:?}"
    );
    assert!(!seen.contains(&resting), "而且没有一帧是静止色：{seen:?}");

    // 运行结束时提示符回到静止，下一次运行也从那儿起步 ——
    // 计数器属于某一次运行，所以静止色永远不是「它停在哪儿就是哪儿」。
    state.request(ConsoleRequest::RunState { running: false });
    assert_eq!(
        prompt_cell(&mut state).1,
        resting,
        "运行结束把提示符放回静止"
    );
    state.request(ConsoleRequest::RunState { running: true });
    state.tick();
    assert_eq!(
        prompt_cell(&mut state).1,
        seen[0],
        "下一次运行从头重走上一次走过的那些帧"
    );
}

#[test]
fn the_prompts_colour_stays_out_of_the_draft() {
    // 提示符是自己的一个 span，这样才能上色；挨着它的草稿不是。
    // 被染色的草稿意味着 span 边界丢了，写的人看到的会是
    // 自己的文字在眼皮底下变色。
    let mut state = state();
    for ch in "hello".chars() {
        state.key(Key::Char(ch));
    }
    state.tick();
    let frame = buffer(120, 24, &mut state);
    let (x, y) = prompt_at(120, 24, &mut state);
    assert!(
        row_text(&frame, y, 120).contains("❱ hello"),
        "草稿紧跟在提示符后面：{:?}",
        row_text(&frame, y, 120)
    );
    // 提示符自己的格子带着色相；它后面那个空格属于同一个 span，
    // 草稿从那之后再往后一格开始。
    assert_eq!(frame[(x, y)].fg, prompt_cell(&mut state).1, "色相");
    let draft = &frame[(x + 2, y)];
    assert_eq!(
        format!("{:?}", draft.fg),
        format!("{:?}", Color::Reset),
        "草稿还是终端自己的前景色，没被动过：{draft:?}"
    );
    assert!(
        draft.modifier.contains(Modifier::BOLD),
        "但它保住了输入区的字重：{draft:?}"
    );
}

#[test]
fn the_mark_stays_still_on_the_narrow_rung_too() {
    // 文字身份的下落短横随标记那条一起关掉了（票 08）：窄档
    // 显示的身份和这一切之前完全一样，而 `identity_falling` 留
    // 在它旁边，仍在它所在之处单测。
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    for frame in 0..5 {
        state.tick();
        let text = screen(100, 24, &mut state).join("\n");
        assert!(
            text.contains(&format!("fs-agent {}", env!("CARGO_PKG_VERSION"))),
            "第 {frame} 帧：身份还是它自己：{text}"
        );
    }
}

#[test]
fn a_pulse_frame_touches_the_prompt_and_nothing_else() {
    // 钟在动的东西，出现在画外壳的每一处（`.scratch/tui-input-pulse/spec.md`
    // §2b）：提示符的颜色，别的什么都没有 —— 没有标记、没有回合条、没有状态行。
    // 80 列以下整条左栏都没有，差别仍然恰好是那两格。
    for (width, height) in [(120u16, 24u16), (60, 24), (40, 10)] {
        let mut state = state();
        state.request(ConsoleRequest::RunState { running: true });
        state.tick();
        let before = buffer(width, height, &mut state);
        state.tick();
        let after = buffer(width, height, &mut state);
        let (prompt_x, prompt_y) = prompt_at(width, height, &mut state);
        let changed: Vec<(u16, u16)> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .filter(|(x, y)| before[(*x, *y)] != after[(*x, *y)])
            .collect();
        let expected: Vec<(u16, u16)> = (0..fs_agent::render::editor::prompt_columns())
            .map(|offset| (prompt_x + offset, prompt_y))
            .collect();
        assert_eq!(
            changed, expected,
            "{width}x{height}：只有提示符自己那两格在变"
        );
    }
}

#[test]
fn the_colour_ring_is_kept_off_screen() {
    // 票 05 退掉了色相环：在真终端上试过两个版本，
    // 读起来都不行，所以屏幕上留的是下落的短横。调色板留在
    // 代码里，因为用户要求留着它 —— 而一件留着的东西在没人决定
    // 该让它出现的情况下又爬回屏幕，就是这个测试要抓的。
    let ring: Vec<String> = PULSE_PALETTE.iter().map(|c| format!("{c:?}")).collect();
    assert_eq!(ring.len(), 6, "这个环是六个色相：{ring:?}");
    for colour in &ring {
        assert!(
            colour.starts_with("Light"),
            "只有一个明度，所以颜色信号永远不会闪：{colour}"
        );
    }
    // 一帧忙碌的画面，按外壳能到的最大的尺寸画：没有一格穿着
    // 标记自己的色带里本来没有的环上色相。（LightMagenta 两边都有，
    // 而且它是色带顶端的颜色，所以它不构成「环被画了」的证据。）
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    state.tick();
    let frame = buffer(174, 50, &mut state);
    let off_screen = [
        Color::LightBlue,
        Color::LightCyan,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightRed,
    ];
    for y in 0..50u16 {
        for x in 0..174u16 {
            let fg = frame[(x, y)].fg;
            assert!(
                !off_screen.contains(&fg),
                "屏幕上没有东西穿着退掉的环：({x}, {y}) 处的 {fg:?}"
            );
        }
    }
}

#[test]
fn every_size_in_the_matrix_draws_the_regions_its_budget_allows() {
    // spec 的几何表覆盖的那张尺寸矩阵，每一行都按它进表的
    // 理由断言：左栏的档位与身份、状态行的档位、
    // 以及外壳让出的转录行数（spec §1–§2）。
    let cases = [
        // 宽、高、左栏档位、身份、状态行上有没有模型、转录行数
        // 输入区的地板是三行，所以每个档位给转录 `h - 4 - 3`
        // （`.scratch/tui-chrome/spec.md` §1–§2）。
        (40u16, 10u16, None, "", false, 3usize),
        (40, 24, None, "", false, 17),
        (60, 24, None, "", true, 17),
        (80, 14, Some(28u16), "fs-agent", true, 7),
        (80, 24, Some(28), "fs-agent", true, 17),
        (100, 24, Some(28), "fs-agent", true, 17),
        (120, 24, Some(40), "mark", true, 17),
        (174, 50, Some(40), "mark", true, 43),
    ];
    for (width, height, tier, identity, model, rows_expected) in cases {
        let rows = screen(width, height, &mut state());
        let text = rows.join("\n");
        assert!(
            !text.contains("终端太小"),
            "{width}x{height} 在下限之内：{text}"
        );
        assert!(
            !text.contains(['┌', '┐', '└', '┘']),
            "{width}x{height} 没有外框：{:?}",
            rows[0]
        );
        assert!(
            text.contains("ctrl-c"),
            "{width}x{height} 保住了出口：{text}"
        );
        // 状态行从不让出：要拿走它的那个档位需要一条比终端地板
        // 允许的更窄的主列（spec §2）。
        assert!(
            text.contains("模式 询问"),
            "{width}x{height} 总是画状态行：{text}"
        );
        assert_eq!(
            transcript_rows(&rows),
            rows_expected,
            "{width}x{height} 给转录 {rows_expected} 行：{rows:#?}"
        );
        // 转录下面紧贴的是状态行（它上方那条线已经离开，
        // `.scratch/tui-chrome/spec.md` §2），再下一行才是输入区
        // 上面那条横线。
        assert!(
            rows[TRANSCRIPT_TOP + rows_expected].contains("模式 询问"),
            "{width}x{height} 转录正下方是状态行：{:?}",
            rows[rows_expected]
        );
        assert!(
            rows[TRANSCRIPT_TOP + rows_expected + 1].ends_with('┄'),
            "{width}x{height} 把那条横线画在状态行正下方：{:?}",
            rows[rows_expected + 1]
        );
        match tier {
            Some(tier) => {
                let frame = buffer(width, height, &mut state());
                assert_eq!(
                    frame[(tier, 0)].symbol(),
                    "┆",
                    "{width}x{height} 把分隔线画在第 {tier} 列"
                );
            }
            None => assert!(!text.contains('┆'), "{width}x{height} 整条藏起左栏：{text}"),
        }
        assert_eq!(
            text.contains('▄'),
            identity == "mark",
            "{width}x{height} 只在宽档上画标记：{text}"
        );
        assert_eq!(
            text.contains("fs-agent"),
            identity == "fs-agent",
            "{width}x{height} 只在窄档上画文字身份：{text}"
        );
        assert_eq!(
            text.contains("claude-sonnet-4-5"),
            model,
            "{width}x{height} 只在放得下时把模型留在状态行上：{text}"
        );
    }
}

#[test]
fn the_sidebar_gives_up_its_identity_then_its_fields_as_it_shrinks() {
    // 左栏自己的高度阶梯（spec §2）：先走的是**标记** —— 退到文字
    // 身份，再退到什么都不画 —— 这之后字段才从尾部离开（缓存 →
    // 输出 → 输入）。地板是页签条加 上下文 / token / 回合，宽度在这
    // 整件事里从不参与。
    // 外框与状态行上方那条线离开之后，左栏的内容行**就是**终端高度
    // （不再减二），于是「先退到文字身份、再退到什么都不画」的后两档
    // 落到了 40×10 地板以下 —— 它们够不到了，而这也正是 40×10 现在
    // 更宽裕的同一笔账（`.scratch/tui-chrome/spec.md` §1–§2）。
    // 左栏顶上留的那一行空行是**花掉的**，所以阶梯看到的内容行是 `h − 1`。
    let cases = [
        // 高度、身份是什么、活下来几项读数
        (15u16, "mark", 6usize),
        (14, "fs-agent", 6),
        (11, "fs-agent", 6),
        (10, "none", 6),
    ];
    for (height, identity, fields) in cases {
        let mut state = state();
        let rows = screen(120, height, &mut state);
        let text = rows.join("\n");
        let mark = text.contains('▄');
        assert_eq!(
            mark,
            identity == "mark",
            "{height} 行只在十六行往上才画标记：{text}"
        );
        assert_eq!(
            text.contains("fs-agent"),
            identity == "fs-agent",
            "{height} 行只在标记那一档之下才画文字身份：{text}"
        );
        let page = panel_text(120, height, &mut state);
        assert_eq!(
            page.len(),
            fields,
            "{height} 行留下 {fields} 项读数：{page:?}"
        );
        assert_eq!(
            text.contains("缓存"),
            fields == 6,
            "{height} 行先丢缓存那一行，再丢输入与输出那两行：{text}"
        );
        // 不管丢掉什么，页签条与回答「还剩多少余地」的那三项读数
        // 都留下。
        assert!(text.contains("调用量"), "{height} 行留下页签条：{text}");
        assert!(
            text.contains("上下文"),
            "{height} 行留下上下文那一行：{text}"
        );
        assert!(
            text.contains("token"),
            "{height} 行留下 token 那一行：{text}"
        );
        assert!(text.contains("回合"), "{height} 行留下回合那一行：{text}");
    }
}

/// 某个页签的标签起始的那一格，从帧上读出来。
///
/// 那些词只出现在标签里，所以找到其中一个的第一个字符
/// 就是找到了页签 —— 而且找法跟人一样：在屏幕上找。
fn tab_cell(frame: &Buffer, width: u16, height: u16, label: &str) -> (u16, u16) {
    let first: String = label.chars().take(1).collect();
    find_cell(frame, width, height, &first).unwrap_or_else(|| panic!("{label} 页签在屏幕上"))
}

#[test]
fn the_selected_tab_is_the_bright_one_and_the_others_are_dim() {
    // 页签条不用一个字就说明在显示哪一页：选中的标签是亮品红
    // 加粗，其余的是叙述灰（spec §3）。
    let frame = buffer(120, 24, &mut state());
    let (column, row) = tab_cell(&frame, 120, 24, "调用量");
    assert_eq!(
        frame[(column, row)].fg,
        Color::LightMagenta,
        "选中的页签是亮的那一个"
    );
    assert!(
        frame[(column, row)].modifier.contains(Modifier::BOLD),
        "而且它是粗的"
    );
    for label in ["轨迹", "文件"] {
        let (column, row) = tab_cell(&frame, 120, 24, label);
        assert_eq!(
            frame[(column, row)].fg,
            Color::DarkGray,
            "{label} 不是选中的那一页"
        );
        assert!(
            !frame[(column, row)].modifier.contains(Modifier::BOLD),
            "{label} 不粗"
        );
    }
}

#[test]
fn clicking_a_tab_switches_the_sidebar_page() {
    use fs_agent::render::wording;

    let mut state = state();
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        !text.contains(wording::tab_placeholder()),
        "在显示调用量那一页：{text}"
    );

    // 轨迹 还没做，所以它照实说，而不是显示编出来的数据。
    let frame = buffer(120, 24, &mut state);
    let (column, row) = tab_cell(&frame, 120, 24, "轨迹");
    state.mouse(click(column, row));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains(wording::tab_placeholder()),
        "页面上是那句占位：{text}"
    );
    assert!(!text.contains("token"), "而读数不在：{text}");
    // 于是状态行成了唯一一项读数 —— 这是占位页被接受的
    // 代价（spec §3）。
    assert!(text.contains("上下文"), "状态行的占比还在：{text}");

    // 选中的标签跟着它一起挪了。
    let frame = buffer(120, 24, &mut state);
    let (column, row) = tab_cell(&frame, 120, 24, "轨迹");
    assert_eq!(frame[(column, row)].fg, Color::LightMagenta);

    // 再切回来：调用量恢复了那些字段。
    let (column, row) = tab_cell(&frame, 120, 24, "调用量");
    state.mouse(click(column, row));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("token"), "读数回来了：{text}");
    assert!(
        !text.contains(wording::tab_placeholder()),
        "占位不见了：{text}"
    );
}

#[test]
fn only_the_tab_labels_answer_a_click() {
    use fs_agent::render::wording;

    // 填满页签行剩下部分的那条横线，以及两个标签之间的那个字形，
    // 都不是控件：点在那儿什么也不发生，因为那儿没有东西
    // 被画成页签（spec §3）。
    let mut state = state();
    let frame = buffer(120, 24, &mut state);
    let (_, row) = tab_cell(&frame, 120, 24, "调用量");
    let inside_the_sidebar = 0..40u16;
    let fill = inside_the_sidebar
        .clone()
        .find(|x| frame[(*x, row)].symbol() == "┄")
        .expect("标签之后那一行是填充");
    let separator = inside_the_sidebar
        .clone()
        .find(|x| frame[(*x, row)].symbol() == "┆")
        .expect("标签之间有分隔");

    for column in [separator, fill] {
        state.mouse(click(column, row));
        let text = screen(120, 24, &mut state).join("\n");
        assert!(
            !text.contains(wording::tab_placeholder()),
            "点在第 {column} 列不是一个页签：{text}"
        );
        assert!(text.contains("token"), "页没有动：{text}");
    }
}

#[test]
fn a_question_keeps_the_tabs_from_answering() {
    use fs_agent::permissions::Answer;
    use fs_agent::render::wording;

    // 一个问句独占了指针：点在页签条上会到达那个问句的
    // 处理器，然后停在那儿。它不该切页，而且 —— 这个测试正是
    // 为这个坑写的 —— 它也不该把问句关掉：画页签的那几帧
    // 和覆盖层把各自的命中矩形记进同一张表，
    // 所以点在页签上的点击是作为一个它并不拥有的动作到达问句的
    // （spec §7、§9）。
    let mut state = idle();
    let (request, mut answer) = ask_permission();
    state.request(request);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = tab_cell(&frame, 120, 24, "轨迹");
    state.mouse(click(column, row));

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("权限询问"), "问句还在：{text}");
    assert!(
        !text.contains(wording::tab_placeholder()),
        "它下面那一页也没有被切走：{text}"
    );
    assert!(answer.try_recv().is_err(), "而且没有人在读者背后作答");
    // 它仍然答得了，这才是「还在」该有的意思。
    state.key(Key::Char('y'));
    assert_eq!(
        answer.try_recv().unwrap(),
        Answer::Allow,
        "这个问句仍然答得了"
    );
}

#[test]
fn a_terminal_with_no_sidebar_has_no_tabs_to_click() {
    use fs_agent::render::wording;

    // 80 列以下左栏整条藏起来，所以标签根本不会画出来
    // —— 而点在一个本该是页签的位置上，就是主列上一次
    // 普通的点击（spec §2、§3）。
    let mut state = state();
    let frame = buffer(60, 24, &mut state);
    assert!(
        find_cell(&frame, 60, 24, "调").is_none(),
        "60 列下没有页签条"
    );
    for (column, row) in [(2u16, 1u16), (4, 3), (2, 8)] {
        state.mouse(click(column, row));
    }
    let text = screen(60, 24, &mut state).join("\n");
    assert!(
        !text.contains(wording::tab_placeholder()),
        "而没有东西切走了并不存在的那一页：{text}"
    );
}

/// 120x24 下回合条那一列：每个转录行一个字符，自上而下。
fn turn_rail_cells(state: &mut TuiState) -> Vec<char> {
    let rows = screen(120, 24, state);
    let transcript = transcript_rows(&rows);
    let frame = buffer(120, 24, state);
    (TRANSCRIPT_TOP..TRANSCRIPT_TOP + transcript)
        .map(|y| {
            frame[(RAIL_AT_120, y as u16)]
                .symbol()
                .chars()
                .next()
                .unwrap_or(' ')
        })
        .collect()
}

/// 回合条那一列压成的紧凑串：只有 `⋮` 与 `┃`/`┊`，空格丢掉。
fn turn_rail_shape(state: &mut TuiState) -> String {
    turn_rail_cells(state)
        .into_iter()
        .filter(|ch| *ch != ' ')
        .collect()
}

/// 一条来自 **user** 的 `MessageCompleted`。
fn user_message(seq: u64, text: &str) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, Role, SpeakerId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: text.to_owned(),
            reasoning: None,
        },
    ))
}

/// 一条 `TurnStarted`。
fn turn_started(seq: u64) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, SpeakerId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::TurnStarted {
            agent: SpeakerId::Debater("kimi".into()),
            iteration: 1,
        },
    ))
}

/// 一整个回合：用户的问题、回合的开始、一条答复与回合的
/// 结束。四行源代码，所以想要 N 个单元的测试就要 N 份这个。
fn a_turn(state: &mut TuiState, seq: u64, question: &str) {
    state.apply(user_message(seq, question));
    state.apply(turn_started(seq + 1));
    state.apply(message(seq + 2, "回答", None));
    state.apply(turn_ended(seq + 3));
}

/// `count` 个完整回合，从零编号。
fn turns(state: &mut TuiState, count: u64) {
    for index in 0..count {
        a_turn(state, index * 4 + 1, &format!("问题 {index}"));
    }
}

/// 转录第一行的文本，回合条跳转就落在这儿。
fn top_transcript_row(state: &mut TuiState) -> String {
    let rows = screen(120, 24, state);
    rows[TRANSCRIPT_TOP].clone()
}

#[test]
fn the_rail_grows_one_cell_per_turn_and_keeps_the_newest_at_the_foot() {
    // 空会话的空列：没有格子，也没有 `⋮` 假装上面
    // 有历史（spec §4）。
    let mut fresh = state();
    assert_eq!(turn_rail_shape(&mut fresh), "");

    let mut state = state();
    turns(&mut state, 3);
    // 三个回合，底端锚定，最新的是亮的那一个。
    assert_eq!(turn_rail_shape(&mut state), "┊┊┃");
    assert_eq!(
        turn_rail_cells(&mut state).len(),
        transcript_rows_at_120x24(),
        "这一列和转录一样高，不再高一丝"
    );
    let cells = turn_rail_cells(&mut state);
    assert!(
        cells[transcript_rows_at_120x24() - 3..]
            .iter()
            .all(|ch| *ch != ' '),
        "三个格子坐在脚上：{cells:?}"
    );

    turns(&mut state, 1);
    assert_eq!(turn_rail_shape(&mut state), "┊┊┊┃", "第四个回合添一个格子");
}

#[test]
fn the_truncation_mark_appears_only_where_units_were_cut() {
    let mut state = state();
    turns(&mut state, 3);
    assert!(
        !turn_rail_shape(&mut state).contains('⋮'),
        "全都放得下时什么都没被裁掉"
    );

    // 回合比行多：显示最新的那些，顶上那一格说明上面
    // 还有更早的。底下什么都没被裁，因为视口就在底部。
    let mut scrolled = TuiState::new(facts());
    turns(&mut scrolled, 30);
    let shape = turn_rail_shape(&mut scrolled);
    assert_eq!(
        shape.chars().next(),
        Some('⋮'),
        "顶上那一格说上面还有：{shape}"
    );
    assert_eq!(
        shape.chars().last(),
        Some('┃'),
        "而最新的回合在脚上：{shape}"
    );
    assert_eq!(
        shape.matches('┃').count() + shape.matches('┊').count(),
        transcript_rows_at_120x24() - 1,
        "标记占掉一格：{shape}"
    );

    // 把视口停在最顶上：这时被裁的是**下面**那些单元。
    let _ = screen(120, 24, &mut scrolled);
    for _ in 0..40 {
        scrolled.key(fs_agent::render::Key::PageUp);
    }
    let shape = turn_rail_shape(&mut scrolled);
    assert_eq!(
        shape.chars().last(),
        Some('⋮'),
        "这下脚上那一格说下面还有：{shape}"
    );
    assert_eq!(
        shape.chars().next(),
        Some('┃'),
        "焦点是屏幕上最老的回合：{shape}"
    );
    assert_eq!(shape.matches('┃').count(), 1, "恰好一个格子是焦点：{shape}");
}

#[test]
fn the_rail_window_follows_the_focus_wherever_the_viewport_is() {
    // 原型里的那个洞：只留最新的几格，会让视口停在一个
    // 老单元上，**一个**亮格都没有
    // （`prototype/frames/120x24-rail-30-units-focus-12-gap.txt`）。窗口改为跟着
    // 焦点走，所以永远恰好有一个 —— 这就是那条回归。
    let mut state = state();
    turns(&mut state, 30);
    let _ = screen(120, 24, &mut state);

    let visible = transcript_rows_at_120x24();
    // 一次往上一页，每一停都验一遍不变量，
    // 包括焦点落在单元表中间的那些位置。
    for _ in 0..20 {
        state.key(fs_agent::render::Key::PageUp);
        let shape = turn_rail_shape(&mut state);
        assert_eq!(
            shape.matches('┃').count(),
            1,
            "每一个位置上恰好有一个亮格：{shape}"
        );
        assert!(
            shape.chars().filter(|ch| *ch != '⋮').count() <= visible,
            "格子数不超过这一列的行数：{shape}"
        );
    }
}

#[test]
fn the_focus_is_the_unit_the_top_row_belongs_to() {
    let mut state = state();
    turns(&mut state, 30);
    let _ = screen(120, 24, &mut state);

    // 在底部时焦点是最新的单元，不管顶行落到哪儿 ——
    // 视口在跟对话，这就是「最新」的意思。
    assert_eq!(
        turn_rail_cells(&mut state)
            .iter()
            .rposition(|ch| *ch == '┃'),
        Some(transcript_rows_at_120x24() - 1),
        "跟着底部走，焦点就落在最后一行"
    );

    // 滚开之后，焦点是视口顶行落在里面的那个单元。
    let question = top_transcript_row(&mut state);
    state.key(fs_agent::render::Key::PageUp);
    let shape = turn_rail_shape(&mut state);
    assert_eq!(shape.matches('┃').count(), 1, "一个焦点格：{shape}");
    assert_ne!(
        turn_rail_cells(&mut state)
            .iter()
            .rposition(|ch| *ch == '┃'),
        Some(transcript_rows_at_120x24() - 1),
        "而且它不再是最新的：{shape}"
    );
    assert!(!question.is_empty(), "顶行上有文本：{question:?}");
}

#[test]
fn clicking_a_rail_cell_jumps_to_that_turns_question() {
    // 回合条存在就是为了这件事：点一格，落到你问过的那句问题
    // 上 —— 顶端对齐，所以每次跳转都落在眼睛预期的地方（spec §4）。
    let mut state = state();
    turns(&mut state, 30);
    let _ = screen(120, 24, &mut state);

    // 窗口底端锚定在一个 `⋮` 下面：120x24 下转录是十七
    // 行，所以一行是标记、十六行是格子 —— 单元 14 到 29，自上
    // 而下。所以偏移 1 是单元 14，偏移 5 是单元 18。每一个都在
    // 全新状态里点，因为跳转会移动视口 —— 也移动格子的窗口。
    for (offset, unit) in [(1usize, 14u64), (5, 18)] {
        let mut state = TuiState::new(facts());
        turns(&mut state, 30);
        let _ = screen(120, 24, &mut state);
        let cells = turn_rail_cells(&mut state);
        let mark = cells
            .iter()
            .position(|ch| *ch == '⋮')
            .expect("这一列在顶端被裁");
        state.mouse(click(RAIL_AT_120, (TRANSCRIPT_TOP + mark + offset) as u16));
        assert!(
            top_transcript_row(&mut state).contains(&format!("[用户] 问题 {unit}")),
            "跳转落在那一个回合自己的问题上：{:?}",
            top_transcript_row(&mut state)
        );
    }
}

#[test]
fn a_rail_cell_jump_at_the_end_clamps_to_the_bottom() {
    // 最新的单元剩下不到一屏，所以跳转会被夹住 —— 这是同一条
    // 规矩在转录末尾的读法，不是特例，也正是
    // 点焦点格通常看起来什么都没发生的原因（spec §4）。
    let mut state = state();
    turns(&mut state, 30);
    let _ = screen(120, 24, &mut state);

    let before = top_transcript_row(&mut state);
    state.mouse(click(
        RAIL_AT_120,
        (TRANSCRIPT_TOP + transcript_rows_at_120x24() - 1) as u16,
    ));
    assert_eq!(
        top_transcript_row(&mut state),
        before,
        "点脚上那一格，视口留在原处"
    );
    assert_eq!(
        turn_rail_cells(&mut state).last(),
        Some(&'┃'),
        "视口还在跟着最新的回合"
    );
}

#[test]
fn a_discussion_counts_rounds_where_a_session_counts_turns() {
    use fs_agent::events::{Event, EventPayload, Role, RoundMode, SpeakerId, StopReason};

    // 有不止一个讨论者的 `speaker_order` 才让一场会话成为讨论，
    // 而讨论会数自己的轮次 —— `CONTEXT.md` 把 轮次 与 回合 分开
    // （spec §4）。
    let mut state = TuiState::new(facts_with_roster(&["kimi", "deepseek"]));
    let kimi = SpeakerId::Debater("kimi".into());
    state.apply(user_message(1, "讨论题目"));
    for round in 0..3u32 {
        for (offset, payload) in [
            EventPayload::RoundStarted {
                round,
                mode: RoundMode::Independent,
            },
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text: format!("第 {round} 轮"),
                reasoning: None,
            },
            EventPayload::TurnEnded {
                reason: StopReason::Completed,
            },
            EventPayload::RoundEnded {
                round,
                reason: StopReason::Completed,
            },
        ]
        .into_iter()
        .enumerate()
        {
            state.apply(fs_agent::render::RenderEvent::Logged(Event::new(
                2 + u64::from(round) * 4 + offset as u64,
                kimi.clone(),
                payload,
            )));
        }
    }

    // 三个轮次和三条 `TurnEnded`：回合条数的是轮次。
    assert_eq!(turn_rail_shape(&mut state), "┊┊┃");

    // 第一个轮次的开头是用户的问题 —— 讨论确实会带的那一条
    // 消息，记在第一个轮次开始之前……
    let first = (TRANSCRIPT_TOP + transcript_rows_at_120x24() - 3) as u16;
    state.mouse(click(RAIL_AT_120, first));
    assert!(
        top_transcript_row(&mut state).contains("[用户] 讨论题目"),
        "会话的问题就是第一个格子落的地方：{:?}",
        top_transcript_row(&mut state)
    );

    // ……而后面的轮次没有自己的用户消息，所以它的格子落在
    // 轮次的开场行上 —— spec 给「没什么可瞄的单元」的兜底。
    let mut later = TuiState::new(facts_with_roster(&["kimi", "deepseek"]));
    later.apply(user_message(1, "讨论题目"));
    for round in 0..3u32 {
        for (offset, payload) in [
            EventPayload::RoundStarted {
                round,
                mode: RoundMode::Independent,
            },
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text: format!("第 {round} 轮"),
                reasoning: None,
            },
            EventPayload::RoundEnded {
                round,
                reason: StopReason::Completed,
            },
        ]
        .into_iter()
        .enumerate()
        {
            later.apply(fs_agent::render::RenderEvent::Logged(Event::new(
                2 + u64::from(round) * 3 + offset as u64,
                kimi.clone(),
                payload,
            )));
        }
    }
    // 垫料，让转录比窗格高，跳转才有地方
    // 落：提示既不是用户消息也不是边界，所以单元照旧。
    for index in 0..40 {
        later.apply(fs_agent::render::RenderEvent::Notice(format!(
            "第 {index} 行"
        )));
    }
    assert_eq!(turn_rail_shape(&mut later), "┊┊┃");
    let second = (TRANSCRIPT_TOP + transcript_rows_at_120x24() - 2) as u16;
    later.mouse(click(RAIL_AT_120, second));
    assert!(
        top_transcript_row(&mut later).contains("── 第 1 轮"),
        "没有自己用户消息的轮次落在它的第一行上：{:?}",
        top_transcript_row(&mut later)
    );
}

#[test]
fn the_pane_scrolls_back_through_the_transcript_and_returns_to_the_bottom() {
    use fs_agent::render::{Key, RenderEvent};

    let mut state = state();
    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }

    // 静止时视口跟着最新的那一行。
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("第 39 行"), "最新的那一行在屏幕上");
    assert!(!text.contains("第 0 行"), "最老的那一行已经滚出去了");

    // PgUp 离开底部、显示更早的行。一页就是窗格自己的
    // 高度，所以顶行是从窗格能显示什么推出来的，而不是
    // 记住的 —— 标记表头改过它，而一个写死的行号
    // 下次阶梯一挪还得再改。
    let visible = transcript_rows_at_120x24();
    state.key(Key::PageUp);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains("第 39 行"), "最新的那一行让位了：{text}");
    assert!(
        text.contains(&format!("第 {} 行", 40 - visible)),
        "更早的那些行出现了：{text}"
    );

    // Ctrl-G 回来，视口又跟上了。
    state.key(Key::CtrlG);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("第 39 行"), "回到最底下：{text}");
    assert!(!text.contains("第 20 行"), "那些老行不见了：{text}");
}

/// 120x24 的一帧显示多少行转录。
///
/// 一次翻页与滚轮的一程都拿这些来量，所以数行的测试
/// 要这个数而不是记住它 —— 外壳让过的
/// 东西已经不止一次挪动过它。
fn transcript_rows_at_120x24() -> usize {
    transcript_rows(&screen(120, 24, &mut state()))
}

/// 120x24 的主列，按列算：宽档左栏（40）加它的分隔线占
/// 0 到 40，所以主列是 41..119 —— 它的转录把其中最后
/// 两列留给滚动条（118）与回合条（119），剩下 41..117 给
/// 文本。
const MAIN_LEFT_AT_120: u16 = 41;
const TRANSCRIPT_TEXT_RIGHT_AT_120: u16 = 118;
const SCROLLBAR_AT_120: u16 = 118;
const RAIL_AT_120: u16 = 119;

/// 120x24 下转录的文本，每个显示行一个字符串。
///
/// 滚动条那一列**不**在里面：滚动条是按视口下面还有
/// 多少行画出来的，所以详情覆盖层把窗格冻住时它还在动
/// —— 比它就等于拿指示器比读者的
/// 位置，而不是拿文本比文本。
fn transcript_text(frame: &Buffer, rows: usize) -> Vec<String> {
    (TRANSCRIPT_TOP..TRANSCRIPT_TOP + rows)
        .map(|y| {
            cells(
                frame,
                y as u16,
                MAIN_LEFT_AT_120,
                TRANSCRIPT_TEXT_RIGHT_AT_120,
            )
        })
        .collect()
}

/// 屏幕上可见的第一条 `第 N 行` 提示的下标，如果有的话。
fn first_notice(rows: &[String]) -> Option<usize> {
    rows.iter().find_map(|row| {
        let rest = row.split("第 ").nth(1)?;
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    })
}

/// 第一个装着 `needle` 的格子，写成 `(column, row)`。
fn find_cell(frame: &Buffer, width: u16, height: u16, needle: &str) -> Option<(u16, u16)> {
    (0..height).find_map(|y| {
        (0..width)
            .find(|x| frame[(*x, y)].symbol() == needle)
            .map(|x| (x, y))
    })
}

#[test]
fn the_transcript_keeps_the_newest_twenty_thousand_source_lines() {
    use fs_agent::render::{Key, RenderEvent};

    // 每条提示都折成三个显示行，所以按显示行算的上限
    // 会把这批历史只留下三分之一。这就是这里钉住的区分：上限
    // 按源代码行算，所以同一批历史在任何终端宽度下都留得住（spec §3）。
    let mut state = state();
    for index in 0..20_001 {
        state.apply(RenderEvent::Notice(format!(
            "第 {index} 行 {}",
            "x".repeat(200)
        )));
    }
    // 先来一帧：一次翻页是按读者能看见的行来量的。
    let _ = screen(120, 24, &mut state);

    // 最老的源代码行是没了，不只是滚出去了：一路翻到顶
    // 也不该把它带回来，而顶上那行就是紧跟在它后面的那一行。
    //
    // 「一路翻到顶」是翻到的，不是数出来的：一次翻页是一窗格的
    // 显示行，而每条提示折成两行，所以步数是终端的
    // 函数 —— 断言步数等于隔三层去断言标记
    // 表头的高度。
    // 一帧只相对于一次翻页便宜，不是免费的：20 000 条折行提示
    // 是约 40 000 显示行，所以这趟走每几百步
    // 查一次位置，而不是每一步都查。
    let mut previous = None;
    for _ in 0..400 {
        for _ in 0..100 {
            state.key(Key::PageUp);
        }
        let rows = screen(120, 24, &mut state);
        let first = first_notice(&rows);
        if first == previous {
            break;
        }
        previous = first;
    }
    let rows = screen(120, 24, &mut state);
    assert!(!rows.join("\n").contains("第 0 行"), "最老的被丢掉了");
    assert_eq!(
        first_notice(&rows),
        Some(1),
        "紧跟它后面的那一行现在是最老的"
    );
}

#[test]
fn the_indicator_counts_what_arrived_and_the_wheel_moves_three_rows() {
    use fs_agent::render::{Key, RenderEvent};
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    let mut state = state();
    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }
    let _ = screen(120, 24, &mut state);

    // 没什么新东西可读时往上滚：指示器只是回去的路。
    state.key(Key::PageUp);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("点此到底"), "给出了回去的路：{text}");
    assert!(!text.contains("行新内容"), "还没有东西到来：{text}");
    // 从底部往上一页，会留下一页的重叠：步长是
    // 窗格自己的高度减去读者保住的那两行，所以视口落在
    // 哪里是从窗格显示什么推出来的，不是记住的。
    let visible = transcript_rows_at_120x24();
    let after_page_up = first_notice(&screen(120, 24, &mut state));
    assert_eq!(
        after_page_up,
        Some(40 - visible - (visible - 2)),
        "往上一页落在窗格自己的高度上，不是某个记住的行号"
    );

    // 读者不在的时候到了一行，而计数就是到的那一行 —— 不是
    // 恰好落在视口下面的全部东西。
    state.apply(RenderEvent::Notice("新的一行".to_owned()));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("↓ 1 行新内容 · 点此到底"), "到了一行：{text}");

    // 滚轮一格挪三行，上下都是。
    let mouse = |kind| MouseEvent {
        kind,
        column: 10,
        row: 10,
        modifiers: KeyModifiers::empty(),
    };
    // 一格把视口往上挪、再挪回来；一格跨过多少条*提示*
    // 取决于窗格显示多少行，所以两端是拿彼此比，
    // 而不是拿一个记住的行号比 —— 这就是「一格三行、
    // 再回来」在任何终端尺寸下的意思。
    state.mouse(mouse(MouseEventKind::ScrollUp));
    let after_wheel_up = first_notice(&screen(120, 24, &mut state));
    assert!(
        after_wheel_up < after_page_up,
        "滚轮把视口往上挪：{after_page_up:?} -> {after_wheel_up:?}"
    );
    state.mouse(mouse(MouseEventKind::ScrollDown));
    assert_eq!(
        first_notice(&screen(120, 24, &mut state)),
        after_page_up,
        "往下一格又回到原处"
    );

    // 点指示器回到最底下；点别的地方都
    // 不管，因为转录归终端选中。
    let frame = buffer(120, 24, &mut state);
    let (column, row) = find_cell(&frame, 120, 24, "点").expect("指示器在屏幕上");
    // 它停在滚动条前面一列：`点此到底` 宽八列，
    // 而转录的最后两列归滚动条与回合条，所以
    // 它最后一个字形绝不能跨在滚动条那一列上。
    assert!(
        column + 8 <= SCROLLBAR_AT_120,
        "指示器待在滚动条左边，从第 {column} 列起"
    );
    state.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 10,
        modifiers: KeyModifiers::empty(),
    });
    assert_eq!(
        first_notice(&screen(120, 24, &mut state)),
        after_page_up,
        "点在转录正文上什么都不改"
    );
    state.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    });
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("新的一行"), "回到最底下：{text}");
    assert!(!text.contains("点此到底"), "指示器不见了：{text}");
}

#[test]
fn a_resize_keeps_the_reader_on_the_same_line() {
    use fs_agent::render::{Key, RenderEvent};

    let mut state = state();
    // 长到两次翻页之后顶行仍够不到最老的那条：
    // 转录 16 行时 40 条提示算两页，那会把测试的前提
    // 钉死在外壳自己的东西上。
    for index in 0..80 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }

    // 跟着底部：窄一些的终端照样跟着底部。
    let narrowed = screen(80, 24, &mut state).join("\n");
    assert!(narrowed.contains("第 79 行"), "仍在最底下：{narrowed}");

    // 滚开时，顶上那行源代码行才是重折行后活下来的东西。
    let _ = screen(120, 24, &mut state);
    state.key(Key::PageUp);
    let one_page_up = first_notice(&screen(120, 24, &mut state));
    state.key(Key::PageUp);
    let before = first_notice(&screen(120, 24, &mut state));
    // 这个测试要的是同一条源代码行在改尺寸前后
    // 一直钉住 —— 所以前提是「离开底部」，而不是
    // 某一套翻页算术。
    assert!(
        before < one_page_up,
        "往上两页：{one_page_up:?} -> {before:?}"
    );
    assert!(
        before.is_some_and(|row| row > 0),
        "而且离开了最老的那一行：{before:?}"
    );
    let after = first_notice(&screen(80, 24, &mut state));
    assert_eq!(after, before, "改尺寸后顶上还是同一行");
}

#[test]
fn the_scrollbar_column_is_reserved_and_filled_only_when_there_is_more_to_read() {
    use fs_agent::render::RenderEvent;

    // 120x24 下主列是 79 列，转录的文本占其中 77
    // 列：最后两列归滚动条与回合条，不管里面
    // 画没画东西，所以文本在 77 列处折行，绝不会因为那两
    // 列在那儿而重新排（spec §1）。
    const TEXT_X: u16 = 41;
    const SCROLLBAR_X: u16 = 118;
    let mut state = state();
    state.apply(RenderEvent::Notice("x".repeat(78)));
    let frame = buffer(120, 24, &mut state);
    assert_eq!(frame[(TEXT_X, 0)].symbol(), "x", "这一行从主列的第一列开始");
    assert_eq!(
        frame[(TEXT_X, 1)].symbol(),
        "x",
        "78 列的文本溢出 77 列的文本区，落到第二行"
    );
    assert_eq!(
        frame[(SCROLLBAR_X, 5)].symbol(),
        " ",
        "全都放得下时预留的那一列什么都不画"
    );

    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }
    let rows = screen(120, 24, &mut state);
    let transcript = transcript_rows(&rows) as u16;
    let frame = buffer(120, 24, &mut state);
    for y in 0..transcript {
        assert_ne!(
            frame[(SCROLLBAR_X, y)].symbol(),
            " ",
            "转录一旦比窗格长就出现滚动条，在第 {y} 行"
        );
    }
}

#[test]
fn the_input_area_holds_three_rows_before_it_grows_and_the_transcript_pays_for_it() {
    // 输入区坐在转录、状态行与它们那两条横线下面；更高的草稿
    // 换来的是让出的转录行，而不是一整块往上挪的
    // 东西。它的地板是三行，所以空草稿与三行草稿花的转录行
    // 完全一样 —— 地板就是为这个准备的
    // （`.scratch/tui-input-pulse/spec.md` §1）。
    let mut state = state();
    let empty = screen(80, 24, &mut state);
    let rows = transcript_rows(&empty);
    assert_eq!(rows, 17, "输入区三行给转录留下十七行：{empty:#?}");
    // 转录下面依次是状态行与输入区上方那条横线，所以输入区
    // 从转录底往下数第三行开始。
    let input = TRANSCRIPT_TOP + rows + 2;
    assert!(
        empty[input].contains(editor::PROMPT),
        "提示符：{:?}",
        empty[input]
    );
    assert!(
        empty[input + 1].trim_matches(['┆', ' ']).is_empty()
            && empty[input + 2].trim_matches(['┆', ' ']).is_empty(),
        "它占住的那两行是空行，不是第二块：{:?} / {:?}",
        empty[input + 1],
        empty[input + 2]
    );

    // 三行草稿什么都不挪：这个框本来就这么高，所以
    // 转录保住它原有的每一行。
    state.paste("第一行\n第二行\n第三行");
    let three = screen(80, 24, &mut state);
    let rows = transcript_rows(&three);
    assert_eq!(rows, 17, "三行草稿在地板之内，所以几何不动：{three:#?}");
    let input = TRANSCRIPT_TOP + rows + 2;
    assert!(three[input].contains("第一行"), "{:?}", three[input]);
    assert!(
        three[input + 1].contains("第二行"),
        "{:?}",
        three[input + 1]
    );
    assert!(
        three[input + 2].contains("第三行"),
        "{:?}",
        three[input + 2]
    );
    assert!(
        three[input + 4].contains("ctrl-c"),
        "提示行留在输入区下面：{:?}",
        three[input + 4]
    );

    // 从第四行起它又长起来，草稿一行换转录一行，
    // 十行的上限仍然按着它（spec §1）。
    state.paste("\n第四行\n第五行");
    let five = screen(80, 24, &mut state);
    assert_eq!(
        transcript_rows(&five),
        15,
        "超地板两行让转录少两行：{five:#?}"
    );
    assert!(
        five.join("\n").contains("第五行"),
        "草稿在屏幕上：{five:#?}"
    );
}

/// 一条 `UsageRecorded`，按循环会记下来的样子。
fn usage(
    seq: u64,
    input: u64,
    output: u64,
    cached: u64,
    miss: u64,
) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, SpeakerId, Usage};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::UsageRecorded {
            usage: Usage {
                input_tokens: input,
                output_tokens: output,
                cached_tokens: cached,
                miss_tokens: miss,
                reasoning_tokens: None,
            },
        },
    ))
}

/// 一条 `TurnEnded`。
fn turn_ended(seq: u64) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, SpeakerId, StopReason};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::TurnEnded {
            reason: StopReason::Completed,
        },
    ))
}

/// 左栏第一页的那一行：页签条下横线下面那一行。
///
/// 页签条紧贴在身份下面 —— 标记那五行、文字
/// 身份那一行，或者什么都没有 —— 所以数字段的测试要的是这一页
/// 而不是某个一旦阶梯挪动就得重算的行号。
fn sidebar_page(rows: &[String]) -> usize {
    let mut rules = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.starts_with('┄'))
        .map(|(y, _)| y);
    rules.next().expect("页签条的上横线");
    rules.next().expect("页签条的下横线") + 1
}

/// 某一渲染行的左栏那一半：从行首到分隔列，分隔列那一格取掉。
/// 留白保留，因为某个值是右贴齐还是右边留白
/// 正是其中一些测试要问的。
///
/// 分隔列那一格不总是 `┆`：主列的横线碰到它时
/// 那一格是被它盖过去的 `┄`，而它也照样终止左栏。
fn sidebar_row(row: &str) -> String {
    let end = row
        .char_indices()
        .find(|(_, ch)| matches!(ch, '┆' | '┄'))
        .map(|(index, _)| index)
        .expect("分隔线终止了左栏");
    row[..end].to_owned()
}

/// 某一渲染行的左栏那一半，按行号取。
fn sidebar_field(rows: &[String], row: usize) -> String {
    sidebar_row(&rows[row])
}

/// 按它离这一页第一行的偏移取左栏字段：`0` 是 `上下文`。
fn panel_field(rows: &[String], offset: usize) -> String {
    sidebar_field(rows, sidebar_page(rows) + offset)
}

#[test]
fn the_sidebar_shows_a_zero_and_a_dash_before_any_call() {
    let rows = screen(120, 24, &mut state());
    // 模型现在归状态行 —— 不管左栏显示哪一页、
    // 多宽，它都看得见（spec §3）。
    assert!(
        rows.iter()
            .any(|row| row.contains("模型 claude-sonnet-4-5")),
        "模型在状态行上：{rows:#?}"
    );
    assert!(
        !panel_field(&rows, 0).contains("模型"),
        "而左栏里哪儿都没有：{:?}",
        panel_field(&rows, 0)
    );
    assert!(
        panel_field(&rows, 0).contains("上下文")
            && panel_field(&rows, 0).contains(fs_agent::render::wording::PANEL_UNKNOWN),
        "还没有调用报过用量：{:?}",
        panel_field(&rows, 0)
    );
    assert!(
        panel_field(&rows, 1).contains("token") && panel_field(&rows, 1).contains('0'),
        "还什么都没花：{:?}",
        panel_field(&rows, 1)
    );
    assert!(
        panel_field(&rows, 2).contains("回合") && panel_field(&rows, 2).contains('0'),
        "还没有回合：{:?}",
        panel_field(&rows, 2)
    );
    assert!(
        panel_field(&rows, 3).contains("输入") && panel_field(&rows, 3).contains('0'),
        "输入是空的：{:?}",
        panel_field(&rows, 3)
    );
    assert!(
        panel_field(&rows, 5).contains("0 / 0"),
        "而缓存也从没被查过：{:?}",
        panel_field(&rows, 5)
    );
    assert!(
        !panel_field(&rows, 0).contains("费用") && !rows.join("\n").contains('$'),
        "钱根本不显示（spec §8）"
    );
}

#[test]
fn the_panel_reads_its_numbers_off_the_stream() {
    let mut state = state();
    // input = cached + miss，两者都不能在 input 之上再加进总数。
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));
    state.apply(turn_ended(2));

    let rows = screen(120, 24, &mut state);
    assert!(
        panel_field(&rows, 0).contains("9,000 / 200,000（4%）"),
        "窗口是最后一次调用的分子：{:?}",
        panel_field(&rows, 0)
    );
    assert!(
        panel_field(&rows, 1).contains("12,345 / 100,000"),
        "花掉的是输入加输出：{:?}",
        panel_field(&rows, 1)
    );
    assert!(
        panel_field(&rows, 2).contains('1'),
        "一个回合：{:?}",
        panel_field(&rows, 2)
    );
    assert!(
        panel_field(&rows, 3).contains("9,000"),
        "输入：{:?}",
        panel_field(&rows, 3)
    );
    assert!(
        panel_field(&rows, 4).contains("3,345"),
        "输出：{:?}",
        panel_field(&rows, 4)
    );
    assert!(
        panel_field(&rows, 5).contains("5,000 / 4,000"),
        "缓存的拆分：{:?}",
        panel_field(&rows, 5)
    );
}

/// 会话完全没有 token 额度的状态。
fn state_without_budget() -> TuiState {
    let mut facts = facts();
    facts.budget_limit = None;
    TuiState::new(facts)
}

#[test]
fn the_narrow_sidebar_keeps_six_fields_and_drops_the_percentage_when_it_must() {
    // 80x14 是能容下全部六项读数的窄档：十四个内容行
    // 装着文字身份、页签条与六个字段。28 列那一档的代价
    // 是上下文行上的百分比 —— `12,345 / 200,000（6%）` 是 22
    // 列，而值那一列是 21（spec §2、§3）。
    let mut state = state();
    // 输出取零，好让花销 —— 输入加输出 —— 正好是上下文那一对
    // 需要的五位数，这才让百分比也过宽。
    state.apply(usage(1, 12_345, 0, 5_000, 7_345));
    state.apply(turn_ended(2));
    let rows = screen(80, 14, &mut state);
    let text = rows.join("\n");

    assert!(
        rows.iter()
            .any(|row| row.contains("模型 claude-sonnet-4-5")),
        "模型在状态行上：{text}"
    );
    let panel = panel_text(80, 14, &mut state);
    assert_eq!(panel.len(), 6, "六项读数全都放得下：{panel:?}");
    assert!(
        panel[0].starts_with("上下文") && panel[0].ends_with("12,345 / 200,000"),
        "上下文那一对：{:?}",
        panel[0]
    );
    assert!(
        !panel[0].contains('（'),
        "而宽度拿走的就是百分比：{:?}",
        panel[0]
    );
    assert!(text.contains("12,345 / 100,000"), "花销：{text}");
    assert!(text.contains("回合"), "回合数：{text}");
    assert!(
        panel[5].contains("5,000 / 7,345"),
        "缓存的拆分：{:?}",
        panel[5]
    );
}

#[test]
fn a_cache_split_too_wide_for_its_column_is_left_out() {
    // 外壳的两档都给拆分留了地方 —— 窄档 21 列、
    // 宽档 33 列 —— 所以这次丢下是在它所在之处断言的：
    // 面板自己的行生成器，值列比那一对还窄时（spec §3）。
    use fs_agent::render::panel::Panel;
    use fs_agent::render::Block;
    use ratatui::layout::Rect;

    let facts = facts();
    let block = Block::Usage {
        speaker: fs_agent::events::SpeakerId::System,
        usage: fs_agent::events::Usage {
            input_tokens: 9_000,
            output_tokens: 3_345,
            cached_tokens: 1_234_567,
            miss_tokens: 9_876_543,
            reasoning_tokens: None,
        },
    };
    let mut panel = Panel::new();
    panel.observe(&block);

    // 20 列给值留 13；拆分需要 21，所以整行都走。
    let narrow = panel.lines(&facts, Rect::new(0, 0, 20, 6));
    let narrow: Vec<String> = narrow
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect();
    assert!(
        !narrow.iter().any(|row| row.contains("1,234,567")),
        "缓存那一行放不下：{narrow:?}"
    );
    // 29 给值留 22，正好够。
    let wide = panel.lines(&facts, Rect::new(0, 0, 29, 6));
    let wide: String = wide
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect();
    assert!(
        wide.contains("1,234,567 / 9,876,543"),
        "放得下时它又回来了：{wide}"
    );
}

#[test]
fn a_session_with_no_allowance_shows_its_spend_alone() {
    let mut state = state_without_budget();
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("12,345"), "花了多少：{text}");
    assert!(
        !text.contains("12,345 / "),
        "也没有给它编一个上限出来：{text}"
    );
}

#[test]
fn a_tall_draft_costs_the_transcript_and_never_the_sidebar() {
    // 左栏是独立的一列，所以它的高度是终端的，不是
    // 转录的：十行草稿吃掉转录行，而把读数留在
    // 原处（spec §2）。
    let mut state = state();
    let draft: String = (0..10).map(|line| format!("第 {line} 行\n")).collect();
    state.paste(&draft);
    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("第 8 行"), "草稿在屏幕上：{text}");
    assert_eq!(
        transcript_rows(&rows),
        10,
        "草稿吃掉的是转录的行：{rows:#?}"
    );
    assert!(text.contains("上下文"), "左栏保住它的读数：{text}");
    assert!(
        text.contains("模型 claude-sonnet-4-5"),
        "状态行也一样：{text}"
    );
    // 页签条与分隔线还在：左栏的命运不是
    // 草稿能决定的。
    assert!(text.contains("调用量") && text.contains('┆'), "{text}");
}

#[test]
fn the_context_numerator_is_the_last_call_while_the_spend_accumulates() {
    let mut state = state();
    state.apply(usage(1, 9_000, 1_000, 5_000, 4_000));
    // 一位数百分比：`（15%）` 对 22 列的值来说会宽一列
    // 而被丢掉，宽度测试覆盖了这一点。
    state.apply(usage(2, 15_000, 2_000, 20_000, 10_000));

    let rows = screen(120, 24, &mut state);
    assert!(
        panel_field(&rows, 0).contains("15,000 / 200,000（7%）"),
        "窗口是*最后一次*调用携带的那个值：{:?}",
        panel_field(&rows, 0)
    );
    assert!(
        panel_field(&rows, 1).contains("27,000 / 100,000"),
        "花销是每一次调用的输入加输出：{:?}",
        panel_field(&rows, 1)
    );
    assert!(
        panel_field(&rows, 3).contains("24,000"),
        "输入求和：{:?}",
        panel_field(&rows, 3)
    );
    assert!(
        panel_field(&rows, 4).contains("3,000"),
        "输出求和：{:?}",
        panel_field(&rows, 4)
    );
}

/// 左栏那一页，每行一个字符串，从渲染出来的帧里读出来。
///
/// 这一页的行从页签条下面开始，到高度阶梯留下的最后一个字段
/// 为止 —— 页面矩形只和它的字段一样高，所以第一行空行
/// 就是它结束的地方。
fn panel_text(width: u16, height: u16, state: &mut TuiState) -> Vec<String> {
    let rows = screen(width, height, state);
    let mut out: Vec<String> = Vec::new();
    for row in rows.iter().skip(sidebar_page(&rows)) {
        // 主列的横线会在它经过的那一行把分隔列盖成 `┄`，所以两者都算
        // 「这一行确实是左栏」的证据。
        if !row.contains('┆') && !row.contains('┄') {
            break;
        }
        let field = sidebar_row(row);
        if field.trim().is_empty() {
            break;
        }
        out.push(field);
    }
    out
}

#[test]
fn the_panel_pads_its_labels_and_aligns_its_values_like_the_snapshot() {
    use fs_agent::render::width::text_columns;

    let mut state = state();
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));
    state.apply(turn_ended(2));
    let panel = panel_text(120, 24, &mut state);

    // 标签列宽六（`上下文` 正好填满），然后一个空格，然后是
    // 左栏剩下那三十四列作为值字段 —— 数字从
    // 右边填起。
    assert_eq!(
        panel[0],
        format!("上下文{}{}", " ".repeat(13), "9,000 / 200,000（4%）"),
        "标签字段六列，然后一个空格，然后值在另外三十三列里右贴齐"
    );
    for row in [&panel[1], &panel[3], &panel[4], &panel[5]] {
        assert!(
            row.ends_with("000") || row.ends_with("345"),
            "数字停在左栏的右边缘上：{row:?}"
        );
    }
    for (index, row) in panel.iter().enumerate() {
        assert!(
            !row.ends_with(' '),
            "第 {index} 行的值后面没有留白：{row:?}"
        );
    }
    assert_eq!(panel.len(), 6, "六项读数：{panel:?}");
    for (index, row) in panel.iter().enumerate() {
        assert_eq!(text_columns(row), 40, "第 {index} 行填满左栏：{row:?}");
    }
}

#[test]
fn a_number_too_wide_for_the_value_column_loses_its_separators_before_its_digits() {
    // 外壳的窄档给值留 21 列，七位数的计数
    // 放得下，所以这条兜底是在它所在之处断言的：面板自己的
    // 行生成器里，值列取原型那 25 列面板曾经用过的宽度
    // （spec §3 留着 v1 那条路径，虽然没有任何档位会触发它）。
    use fs_agent::render::panel::Panel;
    use fs_agent::render::Block;
    use ratatui::layout::Rect;

    let facts = facts();
    let block = Block::Usage {
        speaker: fs_agent::events::SpeakerId::System,
        usage: fs_agent::events::Usage {
            input_tokens: 1_234_567,
            output_tokens: 1_000,
            cached_tokens: 0,
            miss_tokens: 0,
            reasoning_tokens: None,
        },
    };
    let mut panel = Panel::new();
    panel.observe(&block);
    // 24 列给值留 17：`1,235,567 / 100,000` 需要 19 列，所以
    // 分隔符走，数字留下。
    let rows = panel.lines(&facts, Rect::new(0, 0, 24, 6));
    let row = |index: usize| -> String {
        rows[index]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    };
    assert_eq!(row(1), "token   1235567 / 100000", "花销不带分隔符");
    assert_eq!(row(0), "上下文  1234567 / 200000", "窗口也不带");
}

/// 一次针对写操作的询问，按循环从 console 通道发起的样子。
fn ask_permission() -> (
    fs_agent::render::ConsoleRequest,
    tokio::sync::oneshot::Receiver<fs_agent::permissions::Answer>,
) {
    use fs_agent::permissions::PermissionRequest;
    use fs_agent::render::{AskRequest, ConsoleRequest};
    let (tx, rx) = tokio::sync::oneshot::channel::<fs_agent::permissions::Answer>();
    (
        ConsoleRequest::Ask(AskRequest {
            request: PermissionRequest {
                escalation: None,
                speaker: None,
                request_id: "r-1".to_owned(),
                tool_call_id: "c-1".to_owned(),
                tool_name: "write_file".to_owned(),
                args: serde_json::json!({"path": "a.rs"}),
                reason: "mode ask".to_owned(),
            },
            reply: tx,
        }),
        rx,
    )
}

#[test]
fn a_permission_question_lands_in_the_middle_as_a_covered_overlay() {
    use fs_agent::render::RenderEvent;

    let mut state = state();
    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }
    let (ask, _rx) = ask_permission();
    state.request(ask);

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("权限询问："), "{text}");
    assert!(text.contains("[y] 允许"), "键位跟着它一起来：{text}");

    let modal = rows
        .iter()
        .position(|row| row.contains("权限询问"))
        .expect("覆盖层在屏幕上");
    assert!(rows[modal].contains('┆'), "在一个框里：{:?}", rows[modal]);

    // 模式手势在等：一个问句独占键盘，直到它被回答
    // （spec §9）。
    state.key(fs_agent::render::Key::BackTab);
    assert!(state.take_events().is_empty(), "Shift-Tab 不是离开问句的路");

    // 主列里只有这个框自己的两条边框，别的什么都没有：左栏
    // 自己那几列在扫描范围的左边，而转录最后那两列
    // 随框盖住的那些字一起被抹白了。
    let row = modal as u16;
    let frame = buffer(120, 24, &mut state);
    let borders: Vec<u16> = (MAIN_LEFT_AT_120..TRANSCRIPT_TEXT_RIGHT_AT_120)
        .filter(|x| frame[(*x, row)].symbol() == "┆")
        .collect();
    assert_eq!(borders.len(), 2, "这个框的边框：{borders:?}");
    assert!(
        borders[0] > MAIN_LEFT_AT_120 && borders[1] < TRANSCRIPT_TEXT_RIGHT_AT_120 - 1,
        "而且它在主列里居中：{borders:?}"
    );
    // 这个框自己的几条边，从它的左边框往外走：标题是它的第一个
    // 内容行，后面几行是问句的其余部分。
    let left = borders[0];
    let box_top = (0..row)
        .rev()
        .find(|y| frame[(left, *y)].symbol() == "┌")
        .expect("框的上边框");
    let box_bottom = (row..24u16)
        .find(|y| frame[(left, *y)].symbol() == "└")
        .expect("框的下边框");
    assert_eq!(box_top + 1, row, "标题领在问句前面");
    // 在主列里居中，而主列如今就是整屏：上下的余地一样，
    // 差的是整数除法留下的那一行。
    let above = box_top - TRANSCRIPT_TOP as u16;
    let below = 23 - box_bottom;
    assert!(
        above.abs_diff(below) <= 1,
        "居中：框上面 {above} 行，下面 {below} 行"
    );
}

#[test]
fn a_question_splits_into_a_title_a_description_a_call_and_a_row_of_buttons() {
    let mut state = state();
    let (ask, _rx) = ask_permission();
    state.request(ask);

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    let row_of = |needle: &str| {
        rows.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} 在屏幕上：\n{text}"))
    };

    // 四个部分，自上而下：问的是什么、这次调用*为的*是什么（用的是
    // 转录里那条折起行用过的同一句话）、具体的调用，以及回答它的键位。
    // 标题不再点工具名，那句大白话摘要也没了，因为
    // 描述行用读者已经见过的词说了同一件事
    // （2026-09-23，用户要求：弹窗在重复自己）。
    let title = row_of("权限询问：");
    let description = row_of("调用 write_file a.rs");
    let call = row_of("write_file（path=a.rs）");
    let keys = row_of("[y] 允许");
    assert!(
        title < description && description < call && call < keys,
        "标题、描述、调用，然后是按钮：\n{text}"
    );
    // 每个部分保住自己那一行：命令再也不能把键位挤进
    // 一句话中间，键位也埋不掉命令。
    assert!(
        !rows[keys].contains("path=a.rs") && !rows[keys].contains("权限询问"),
        "按钮独占那一行：{:?}",
        rows[keys]
    );
    assert!(
        !rows[call].contains("调用 write_file"),
        "调用那一行带的是调用本身，不是描述：{:?}",
        rows[call]
    );
    assert!(
        !rows[description].contains("path=a.rs"),
        "而描述说的是它为的什么，不是怎么跑它：{:?}",
        rows[description]
    );
}

#[test]
fn a_cancelled_run_leaves_no_overlay_behind() {
    // 覆盖层属于提出这个问句的那一次运行。那次运行一结束，
    // 循环就不再等答案，所以屏幕上不能留东西去收答案 ——
    // 否则下一次按键会发给一个没人等的问句（spec §6、§9）。
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    let (ask, _answer) = ask_permission();
    state.request(ask);
    let rows = screen(120, 24, &mut state);
    assert!(
        rows.iter().any(|row| row.contains("权限询问")),
        "它的运行还在时覆盖层也在：\n{}",
        rows.join("\n")
    );

    state.request(ConsoleRequest::RunState { running: false });
    let rows = screen(120, 24, &mut state);
    assert!(
        !rows.iter().any(|row| row.contains("权限询问")),
        "运行走了，覆盖层也跟着走了：\n{}",
        rows.join("\n")
    );
}

#[test]
fn a_long_command_still_says_what_it_would_do() {
    use fs_agent::permissions::PermissionRequest;
    use fs_agent::render::AskRequest;

    // 这一行回答的那句抱怨：一整墙 shell 不是人
    // 读得下去的东西，所以问句先说这次调用*为的*是什么 —— 用的
    // 是转录里那条折起行的同一句话 —— 然后才是那墙东西。
    let mut state = state();
    let (tx, _rx) = tokio::sync::oneshot::channel::<fs_agent::permissions::Answer>();
    state.request(ConsoleRequest::Ask(AskRequest {
        request: PermissionRequest {
            escalation: None,
            speaker: None,
            request_id: "r-1".to_owned(),
            tool_call_id: "c-1".to_owned(),
            tool_name: "bash".to_owned(),
            args: serde_json::json!({
                "command": "for f in $(git ls-files '*.rs'); do grep -L 'mod tests' \"$f\"; done | xargs wc -l | sort -n",
            }),
            reason: "mode ask: a write asks the user".to_owned(),
        },
        reply: tx,
    }));

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    let description = rows
        .iter()
        .position(|row| row.contains("调用 bash"))
        .unwrap_or_else(|| panic!("描述在屏幕上：\n{text}"));
    let keys = rows
        .iter()
        .position(|row| row.contains("[y] 允许"))
        .unwrap_or_else(|| panic!("按钮在屏幕上：\n{text}"));
    assert!(description < keys, "描述领在按钮前面：\n{text}");
    assert!(
        text.contains("git ls-files"),
        "而命令还留在那儿可读：\n{text}"
    );
}

// ---------------------------------------------------------------------------
// 模式循环（`.scratch/todo-and-modes/spec.md` §1）
// ---------------------------------------------------------------------------

#[test]
fn shift_tab_cycles_the_status_row_through_the_four_modes_and_back() {
    // 手势打在帧上：按一下，状态行显示的模式往前走一档，
    // 按四下把会话送回组装时给它的那一档。这一行是
    // 模式唯一可见的地方 —— 计划模式覆盖层与它的注入
    // 随模式本身一起没了 —— 所以这一帧就是整个用户故事
    // 「干活的时候看得见自己在哪一档」。
    let mut state = idle();
    let first = screen(120, 24, &mut state).join("\n");
    assert!(first.contains("模式 询问"), "组装时给的那一档：{first}");

    for expected in ["模式 工作区", "模式 自动", "模式 只读", "模式 询问"] {
        state.key(Key::BackTab);
        let text = screen(120, 24, &mut state).join("\n");
        assert!(text.contains(expected), "这一行显示 {expected}：{text}");
    }
    assert_eq!(
        state.take_events(),
        vec![
            FrontEndEvent::CycleMode,
            FrontEndEvent::CycleMode,
            FrontEndEvent::CycleMode,
            FrontEndEvent::CycleMode
        ],
        "按一下，给循环一个手势"
    );
}

#[test]
fn the_status_row_starts_on_the_mode_the_session_was_assembled_with() {
    // `--mode readonly`（或 `[permissions] mode`）必须从第一帧起
    // 就出现在这一行上。一行老是从 询问 起步，就是对用户
    // 刚配好的那道闸门撒了谎。
    let mut state = TuiState::new(SessionFacts {
        mode: fs_agent::permissions::Mode::Readonly,
        ..facts()
    });
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("模式 只读"), "{text}");
}

#[test]
fn no_key_opens_a_plan_mode_any_more() {
    // 曾经存在的那个手势什么都没留下：`Shift+Tab` 走的是
    // 权限模式，而没有任何键会画计划覆盖层或注入一条指令
    // （`.scratch/todo-and-modes/spec.md` §1）。
    let mut state = idle();
    state.key(Key::BackTab);
    let text = screen(120, 24, &mut state).join("\n");
    for gone in ["计划", "PLAN.md", "硬计划"] {
        assert!(!text.contains(gone), "`{gone}` 不见了：{text}");
    }
}

// ---------------------------------------------------------------------------
// 左栏的 `todo` 页签（`.scratch/todo-and-modes/spec.md` §4）
// ---------------------------------------------------------------------------

/// 一次 `todo` 调用的参数：整份列表，每项是 `{content, status}`。
fn todo_args(list: &[(&str, &str)]) -> serde_json::Value {
    serde_json::json!({
        "items": list
            .iter()
            .map(|(content, status)| serde_json::json!({ "content": content, "status": status }))
            .collect::<Vec<_>>()
    })
}

fn kimi() -> fs_agent::events::SpeakerId {
    fs_agent::events::SpeakerId::Debater("kimi".into())
}

fn executor() -> fs_agent::events::SpeakerId {
    fs_agent::events::SpeakerId::Executor(fs_agent::events::ParticipantId::new("kimi-1"))
}

/// 渲染器看到的一次 `todo` 调用：开始那条带着参数，
/// 而结果落地时块才画出来 —— 所以两条都进。
fn apply_todo(
    state: &mut TuiState,
    id: &str,
    speaker: fs_agent::events::SpeakerId,
    args: serde_json::Value,
) {
    use fs_agent::events::{Event, EventPayload, ToolCallId};
    state.apply(RenderEvent::Logged(Event::new(
        1,
        speaker.clone(),
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new(id),
            tool_name: fs_agent::tools::TODO_TOOL.to_owned(),
            args,
        },
    )));
    state.apply(RenderEvent::Logged(Event::new(
        2,
        speaker,
        EventPayload::ToolCallCompleted {
            tool_call_id: ToolCallId::new(id),
            ok: true,
            output: Some("todo: ok".to_owned()),
            error: None,
            duration_ms: 1,
        },
    )));
}

/// 页签条自己那一行，从帧上读出来。终端太窄
/// 放不下左栏时是空的，那时根本就没有条。
fn tab_bar(state: &mut TuiState, width: u16, height: u16) -> String {
    let rows = screen(width, height, state);
    rows.iter()
        .find(|row| row.contains(wording::TAB_USAGE))
        .cloned()
        .unwrap_or_default()
}

/// 左栏自己那几列，逐行。转录很可能也带着同样的
/// 词 —— 一次 `todo` 调用的参数也在折起行上 —— 所以针对
/// **页面**的断言必须读这一页的列，而不是整个屏幕。
fn sidebar_rows(state: &mut TuiState, width: u16, height: u16) -> Vec<String> {
    let frame = buffer(width, height, state);
    (0..height)
        .map(|y| cells(&frame, y, 0, SIDEBAR_COLUMNS))
        .collect()
}

/// 宽档左栏加它那条分隔列占掉的列数：左栏 40 列，分隔线在第 40 列，
/// 所以内容落在 0..39 上。
const SIDEBAR_COLUMNS: u16 = 40;

/// 页签条那些标签所在的行号。
fn tab_bar_row(state: &mut TuiState, width: u16, height: u16) -> u16 {
    screen(width, height, state)
        .iter()
        .position(|row| row.contains(wording::TAB_USAGE))
        .expect("页签条在屏幕上") as u16
}

#[test]
fn no_todo_tab_before_a_list_has_ever_landed() {
    // 这个页签不是外壳的固定件：从没立过待办的会话
    // 有三个页面，两种左栏宽度下都是 —— 而 60 列下压根
    // 没有左栏，也就没有条来带这个标签。
    for (width, height) in [(120u16, 24u16), (80, 24), (60, 24)] {
        let mut state = state_with_roster(&["kimi"]);
        let bar = tab_bar(&mut state, width, height);
        assert!(
            !bar.contains(wording::TAB_TODO),
            "{width}x{height} 在有列表之前没有 `todo` 页签：{bar}"
        );
    }
}

#[test]
fn the_todo_tab_appears_with_a_non_empty_list_and_then_stays() {
    // 用户选了「见过一次就一直在」：一个在读者手底下
    // 来来去去的页签会挪走他们正在看的那一页。所以全部完成和
    // 清空都和第一份列表一样牢牢留着它。
    for (width, height) in [(120u16, 24u16), (80, 24)] {
        let mut state = state_with_roster(&["kimi"]);
        apply_todo(
            &mut state,
            "call-1",
            kimi(),
            todo_args(&[("写测试", "pending")]),
        );
        assert!(
            tab_bar(&mut state, width, height).contains(wording::TAB_TODO),
            "{width}x{height} 在有列表之后显示这个页签"
        );

        apply_todo(
            &mut state,
            "call-2",
            kimi(),
            todo_args(&[("写测试", "completed")]),
        );
        assert!(
            tab_bar(&mut state, width, height).contains(wording::TAB_TODO),
            "{width}x{height} 全部做完时也留着它"
        );

        apply_todo(
            &mut state,
            "call-3",
            kimi(),
            serde_json::json!({ "items": [] }),
        );
        assert!(
            tab_bar(&mut state, width, height).contains(wording::TAB_TODO),
            "{width}x{height} 列表被清空之后也留着它"
        );
    }
}

#[test]
fn an_executors_list_stays_out_of_the_sidebar() {
    // 派发者的列表和执行者的列表是两份列表（§2）。执行者的那份是
    // 它自己的记录：它出现在转录里，而左栏 —— 显示的是
    // 主会话那一份 —— 连为它长出一个页签都不该。
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "exec-1",
        executor(),
        todo_args(&[("派出去的活", "pending")]),
    );

    assert!(!tab_bar(&mut state, 120, 24).contains(wording::TAB_TODO));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("派出去的活") || text.contains("todo"),
        "调用本身还在转录里：{text}"
    );
}

#[test]
fn the_todo_page_lists_each_item_with_its_glyph_and_counts_them() {
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[
            ("还没开始", "pending"),
            ("正在做", "in_progress"),
            ("做完了", "completed"),
        ]),
    );
    let row = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, row, wording::TAB_TODO);

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("☐ 还没开始"), "{text}");
    assert!(text.contains("▸ 正在做"), "{text}");
    assert!(text.contains("✓ 做完了"), "{text}");
    assert!(text.contains("已完成 1/3"), "{text}");
}

#[test]
fn the_todo_page_shows_what_fits_then_says_how_many_more_there_are() {
    // 这一页不滚动：高度阶梯给出行数，而计数前面那
    // 一行说明有多少项没挤下。计数那一行是地板 ——
    // 别的都留不下时剩下的就是它。
    let items: Vec<(String, &str)> = (0..12)
        .map(|index| (format!("第 {index} 项"), "pending"))
        .collect();
    let borrowed: Vec<(&str, &str)> = items
        .iter()
        .map(|(content, status)| (content.as_str(), *status))
        .collect();

    let mut state = state_with_roster(&["kimi"]);
    apply_todo(&mut state, "call-1", kimi(), todo_args(&borrowed));
    let row = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, row, wording::TAB_TODO);

    let rows = sidebar_rows(&mut state, 120, 24);
    let count = rows
        .iter()
        .position(|line| line.contains("已完成 0/12"))
        .unwrap_or_else(|| panic!("计数那一行在页面上：{rows:#?}"));
    let shown = rows
        .iter()
        .take(count)
        .filter(|line| (0..12).any(|index| line.contains(&format!("第 {index} 项"))))
        .count();
    let overflow = rows[count - 1].clone();
    assert!(
        overflow.contains(&format!("＋{} 项", 12 - shown)),
        "计数上面那一行说明有多少项没挤下（显示了 {shown} 项）：{rows:#?}"
    );
    assert!(shown < 12, "这一页没能把它们全装下：{rows:#?}");
}

#[test]
fn a_page_one_row_tall_degrades_to_the_count_line_alone() {
    // 走面板而不是走一帧，因为没有终端会向布局要一页
    // 只有一行的页面 —— `SIDEBAR_MIN_FIELDS` 才是地板 —— 而这条规矩
    // 在那儿仍然得成立，而不是画出一个跑出来的项。
    use fs_agent::render::todo::TodoPanel;
    use fs_agent::render::{Block, ToolBlock, ToolOutcome};
    use ratatui::layout::Rect;

    let mut panel = TodoPanel::default();
    panel.observe(&Block::Tool(Box::new(ToolBlock {
        speaker: kimi(),
        tool_call_id: fs_agent::events::ToolCallId::new("call-1"),
        tool: fs_agent::tools::TODO_TOOL.to_owned(),
        args: todo_args(&[("一件事", "pending")]),
        outcome: Some(ToolOutcome {
            ok: true,
            output: Some("todo: ok".to_owned()),
            error: None,
            duration_ms: 1,
        }),
    })));

    let lines = panel.lines(Rect::new(0, 0, 28, 1));
    let text: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
    assert_eq!(text.len(), 1, "{text:?}");
    assert!(text[0].contains("已完成 0/1"), "{text:?}");
}

#[test]
fn the_four_tab_labels_fit_at_the_narrow_width() {
    // 调用量┆todo┆轨迹┆文件 是十一个格加三个分隔符，所以窄档
    // 仍然放得下全部四个 —— 标签条不该把一个挤出边缘。
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("一件事", "pending")]),
    );

    let bar = tab_bar(&mut state, 80, 24);
    assert!(
        bar.contains(&format!(
            "{}┆{}┆{}┆{}",
            wording::TAB_USAGE,
            wording::TAB_TODO,
            wording::TAB_TRACE,
            wording::TAB_FILES
        )),
        "四个标签按顺序、带着它们的分隔符：{bar}"
    );

    // 而且标签仍然是条里唯一的控件：分隔符与
    // 填满这一行剩下部分的那条横线是画上去的，不是**画成页签**，所以
    // 点它们任何一个都不发生什么（`draw_tab_bar` 推出来的分隔符、填充
    // 以及来自同一份标签列表的命中矩形）。
    let frame = buffer(80, 24, &mut state);
    let row = tab_bar_row(&mut state, 80, 24);
    let inside_the_sidebar = 0..28u16;
    let fill = inside_the_sidebar
        .clone()
        .find(|x| frame[(*x, row)].symbol() == "┄")
        .expect("四个标签之后那一行是填充");
    let separator = inside_the_sidebar
        .clone()
        .find(|x| frame[(*x, row)].symbol() == "┆")
        .expect("标签之间有分隔");
    for column in [separator, fill] {
        state.mouse(click(column, row));
        let text = screen(80, 24, &mut state).join("\n");
        assert!(
            !text.contains("☐ 一件事"),
            "点在第 {column} 列不是一个页签：{text}"
        );
        assert!(text.contains("token"), "页没有动：{text}");
    }
}

#[test]
fn the_todo_page_stays_put_when_the_list_is_cleared_under_it() {
    // 一个有条件的页签唯一能做的事，就是把页面从正在读它的人
    // 手里拿走。它做不到：闩是「曾经有过列表」，而清空
    // 列表之后页面留着，显示计数。
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("一件事", "pending")]),
    );
    let row = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, row, wording::TAB_TODO);
    assert!(sidebar_rows(&mut state, 120, 24)
        .join("\n")
        .contains("一件事"));

    apply_todo(
        &mut state,
        "call-2",
        kimi(),
        serde_json::json!({ "items": [] }),
    );
    let page = sidebar_rows(&mut state, 120, 24).join("\n");
    assert!(page.contains("已完成 0/0"), "{page}");
    assert!(!page.contains("一件事"), "而那些项随列表一起没了：{page}");
}

// --- `/` 菜单 --------------------------------------------------------------

/// 装进循环报出来的那些名字：它解析的内建命令，然后是会话
/// 发现的技能。渲染器没有自己的一份 —— 这就是整个菜单。
///
/// 内建那一半是从 [`wording::BUILT_IN_COMMANDS`] 读来的 —— 也就是
/// 循环解析、未知命令文本点名的那同一份列表 —— 而不是在这里写死，
/// 所以这份 fixture 不会再和真菜单漂开（那些命令退役之后
/// 它还给 `/plan` 和 `/endplan` 供了一段日子）。
fn install_catalog(state: &mut TuiState) {
    let mut entries: Vec<CatalogEntry> = wording::BUILT_IN_COMMANDS
        .iter()
        .map(|command| CatalogEntry::new(command.name, command.description))
        .collect();
    entries.extend([
        CatalogEntry::new("ask-matt", "不知道用哪个 skill 时问它"),
        CatalogEntry::new("review", "审查一个变更"),
    ]);
    state.request(ConsoleRequest::Catalog { entries });
}

/// 一次进行中的提示请求，好让测试读回一次提交送出去了什么。
fn awaiting_line(state: &mut TuiState) -> tokio::sync::oneshot::Receiver<Option<String>> {
    let (reply, line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    line
}

/// `/` 菜单把它的框画在哪：`(x, y, width, height)`。
///
/// 从缓冲里找，跟上面那些边框断言的做法一样：装着
/// `needle` 的那一行，在它左边是框的左边框，然后是它的角。
fn menu_box(frame: &Buffer, width: u16, height: u16, needle: &str) -> (u16, u16, u16, u16) {
    let row = (0..height)
        .find(|y| row_text(frame, *y, width).contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} 在屏幕上"));
    let column = row_text(frame, row, width).find(needle).unwrap() as u16;
    let x = (0..column)
        .rev()
        .find(|c| frame[(*c, row)].symbol() == "┆")
        .expect("菜单的左边框");
    let top = (0..row)
        .rev()
        .find(|y| frame[(x, *y)].symbol() == "┌")
        .expect("菜单的上边框");
    let bottom = (row..height)
        .find(|y| frame[(x, *y)].symbol() == "└")
        .expect("菜单的下边框");
    let right = (x..width)
        .find(|c| frame[(*c, top)].symbol() == "┐")
        .expect("菜单的右边框");
    (x, top, right - x + 1, bottom - top + 1)
}

/// 草稿打在哪一行：提示符是主列里的第一样东西，
/// 所以它跟在左栏与分隔线后面。
fn input_row(rows: &[String]) -> usize {
    rows.iter()
        .position(|row| row.contains(&format!("┆{}", editor::PROMPT)))
        .expect("输入区那一行")
}

#[test]
fn a_slash_opens_a_menu_of_the_names_the_loop_reported() {
    let mut state = state();
    install_catalog(&mut state);
    state.key(Key::Char('/'));

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    // 内建命令那一组排在前面，所以先露出来的是它们。
    for name in ["/undo", "/discuss", "/goal-new"] {
        assert!(text.contains(name), "提供了 {name}：\n{text}");
    }
    assert!(
        !text.contains("/plan") && !text.contains("/endplan"),
        "退役的命令不再提供了：\n{text}"
    );
    assert!(
        text.contains("回滚上一次编辑"),
        "带着它是干什么的：\n{text}"
    );

    // 一族命令用连字符写成一条，所以 `/goal-` 这个前缀在菜单里列出整族（现在只有 `new`）——
    // 空格形状的 `/goal new` 补不出第二个词，这正是改名的理由。
    let mut family = self::state();
    install_catalog(&mut family);
    for ch in "/goal-".chars() {
        family.key(Key::Char(ch));
    }
    let text = screen(120, 24, &mut family).join("\n");
    assert!(text.contains("/goal-new"), "命令族按前缀列出来：\n{text}");
    assert!(
        !text.contains("/undo") && !text.contains("/loop"),
        "别的命令被滤掉了：\n{text}"
    );

    // 而这正是改名的理由：`Tab` 把整条命令补进草稿。
    family.key(Key::Tab);
    let text = screen(120, 24, &mut family).join("\n");
    assert!(
        text.contains(&format!("┆{}/goal-new", editor::PROMPT)),
        "Tab 补出整条命令：\n{text}"
    );

    // 菜单是一扇**窗口**，而技能排在命令之后：装不下的那些靠打字滤出来 —— 它们在目录里，
    // 不是被丢掉了。打一个 `/rev` 就只剩 `/review`。
    let mut narrowed = self::state();
    install_catalog(&mut narrowed);
    for ch in "/rev".chars() {
        narrowed.key(Key::Char(ch));
    }
    let text = screen(120, 24, &mut narrowed).join("\n");
    assert!(text.contains("/review"), "技能仍然在目录里：\n{text}");

    // 它是浮着的 —— 框、边框全都浮着 —— 在它所属的输入区上面。
    let frame = buffer(120, 24, &mut state);
    let (_, top, _, height) = menu_box(&frame, 120, 24, "/undo");
    assert!(height >= 3, "是一个框，不是一行：高 {height}");
    assert!(
        top as usize + height as usize <= input_row(&rows),
        "菜单坐在输入区上面：{top}+{height} 对 {}",
        input_row(&rows)
    );
}

#[test]
fn the_menu_filters_on_what_has_been_typed_after_the_slash() {
    let mut state = state();
    install_catalog(&mut state);
    for ch in "/ask".chars() {
        state.key(Key::Char(ch));
    }
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("/ask-matt"), "{text}");
    assert!(!text.contains("/undo"), "别的都被滤掉了：\n{text}");

    // 一个什么都没指到的前缀会关上这个框，而不是显示一个空框。
    for ch in "zzz".chars() {
        state.key(Key::Char(ch));
    }
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains("┆ /"), "没指到东西的前缀没有菜单：\n{text}");
}

#[test]
fn the_menu_follows_the_cursor_column() {
    let mut state = state();
    install_catalog(&mut state);
    state.key(Key::Char('/'));
    let frame = buffer(120, 24, &mut state);
    let (before, ..) = menu_box(&frame, 120, 24, "/undo");
    // 再多一个字符，框就跟着打它的那个光标走。
    state.key(Key::Char('a'));
    let frame = buffer(120, 24, &mut state);
    let (after, ..) = menu_box(&frame, 120, 24, "/ask-matt");
    assert_eq!(after, before + 1, "框跟着光标移动了");
}

#[test]
fn tab_fills_the_highlighted_name_in_and_does_not_submit_it() {
    let mut state = state();
    install_catalog(&mut state);
    let mut line = awaiting_line(&mut state);
    for ch in "/as".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Tab);
    assert!(
        line.try_recv().is_err(),
        "Tab 只补全；它从不把草稿里的东西发出去"
    );
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains(&format!("┆{}/ask-matt", editor::PROMPT)),
        "名字在草稿里：\n{text}"
    );
    // 而且补全把菜单关上了，所以后面还能接着打一个任务名。
    assert!(!text.contains("┆ /ask-matt"), "菜单结束了：\n{text}");
}

#[test]
fn enter_fills_the_highlighted_name_in_and_submits_it_in_one_press() {
    let mut state = state();
    install_catalog(&mut state);
    let mut line = awaiting_line(&mut state);
    for ch in "/ask".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert_eq!(
        line.try_recv().unwrap(),
        Some("/ask-matt".to_owned()),
        "`/ask` + Enter 是一个手势，而它送出去的名字是完整的"
    );
}

#[test]
fn the_arrows_walk_the_matches() {
    let mut state = state();
    install_catalog(&mut state);
    let mut line = awaiting_line(&mut state);
    // 光一个 `/` 什么都不高亮，所以第一个 `↓` 拿第一个名字，
    // 第二个拿它后面那一个。
    state.key(Key::Char('/'));
    state.key(Key::Down);
    state.key(Key::Down);
    state.key(Key::Enter);
    assert_eq!(
        line.try_recv().unwrap(),
        Some("/discuss".to_owned()),
        "内建列表里的第二个名字"
    );
}

#[test]
fn enter_on_a_bare_slash_sends_what_was_typed() {
    // 在打了名字或用箭头走过列表之前什么都不高亮，所以
    // 只为看一眼菜单而按下的 `Enter` 不可能跑掉里面的第一条命令。
    let mut state = state();
    install_catalog(&mut state);
    let mut line = awaiting_line(&mut state);
    state.key(Key::Char('/'));
    state.key(Key::Enter);
    assert_eq!(line.try_recv().unwrap(), Some("/".to_owned()));
}

#[test]
fn an_arrow_on_a_bare_slash_picks_the_row_enter_takes() {
    // 刻意走的那条路：走过列表，拿走高亮落到的那一个。
    let mut state = state();
    install_catalog(&mut state);
    let mut line = awaiting_line(&mut state);
    state.key(Key::Char('/'));
    state.key(Key::Down);
    state.key(Key::Enter);
    assert_eq!(line.try_recv().unwrap(), Some("/undo".to_owned()));
}

#[test]
fn the_arrows_wrap_at_the_ends() {
    let mut state = state();
    install_catalog(&mut state);
    let mut line = awaiting_line(&mut state);
    // 从第一个匹配再往上，绕到最后一个。
    state.key(Key::Char('/'));
    state.key(Key::Up);
    state.key(Key::Enter);
    assert_eq!(line.try_recv().unwrap(), Some("/review".to_owned()));
}

#[test]
fn esc_closes_the_menu_and_leaves_the_draft_where_it_was() {
    let mut state = idle();
    install_catalog(&mut state);
    state.key(Key::Char('/'));
    state.key(Key::Esc);

    // 草稿活下来：`Esc` 关的是菜单，它没有开始把草稿
    // 扔掉，也没有清掉一行长的草稿（spec §6、§7）。
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains(&format!("┆{}/", editor::PROMPT)),
        "草稿还在那儿：\n{text}"
    );
    assert!(!text.contains("┆ /undo"), "菜单不见了：\n{text}");
    assert!(!text.contains("清空输入"), "什么都没被问：\n{text}");
}

#[test]
fn a_question_hides_the_menu_because_it_owns_the_keyboard() {
    let mut state = state();
    install_catalog(&mut state);
    state.key(Key::Char('/'));
    let (ask, _rx) = ask_permission();
    state.request(ask);

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("权限询问"), "问句起来了：\n{text}");
    assert!(
        !text.contains("┆ /undo"),
        "也没有东西在提供回答别的东西的键位：\n{text}"
    );
}

#[test]
fn the_menu_keeps_its_corners_over_text_that_is_not_ascii() {
    // 一个宽字形占两格，而把一帧差分到终端时第二格会被跳过，
    // 所以左边框落在那儿的框，过去会丢掉它的
    // 左上角。被盖住一半的字形改为抹白 —— 半个字形反正也
    // 画不出来 —— 而框还是一个框。
    let mut state = state();
    install_catalog(&mut state);
    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("对话第 {index} 行")));
    }
    state.key(Key::Char('/'));
    let _ = screen(120, 24, &mut state);
    let frame = buffer(120, 24, &mut state);
    let (x, top, width, height) = menu_box(&frame, 120, 24, "/undo");
    let (bottom, right) = (top + height - 1, x + width - 1);
    for y in top..=bottom {
        let (left, rightmost) = if y == top {
            ("┌", "┐")
        } else if y == bottom {
            ("└", "┘")
        } else {
            ("┆", "┆")
        };
        assert_eq!(frame[(x, y)].symbol(), left, "第 {y} 行，左边框");
        assert_eq!(frame[(right, y)].symbol(), rightmost, "第 {y} 行，右边框");
    }
}

#[test]
fn there_is_no_menu_before_the_loop_has_said_what_exists() {
    let mut state = state();
    state.key(Key::Char('/'));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains("┆ /"), "空清单什么都提供不了：\n{text}");
}

#[test]
fn every_question_kind_takes_the_overlay() {
    use fs_agent::render::Key;

    // 循环的权限询问。
    let mut asking = state();
    let (ask, _answer) = ask_permission();
    asking.request(ask);
    let text = screen(120, 24, &mut asking).join("\n");
    assert!(text.contains("权限询问："), "标题：{text}");
    assert!(text.contains("[y] 允许"), "带着它的键位：{text}");

    // 一次过大的粘贴。
    let mut paste = state();
    paste.paste(&"x".repeat(100_001));
    let text = screen(120, 24, &mut paste).join("\n");
    assert!(text.contains("粘贴确认"), "标题：{text}");
    assert!(text.contains("粘贴 100001 字符"), "{text}");
    assert!(text.contains("[y] 粘贴"), "带着它的键位：{text}");

    // 还有多行草稿上的 `Esc`。
    let mut draft = idle();
    draft.paste("第一行\n第二行");
    draft.key(Key::Esc);
    let text = screen(120, 24, &mut draft).join("\n");
    assert!(text.contains("清空输入"), "{text}");
    assert!(text.contains("草稿有多行"), "它会扔掉什么：{text}");
    assert!(text.contains("[y] 清空"), "带着它的键位：{text}");
}

#[test]
fn a_character_key_answers_the_question_and_never_reaches_the_draft() {
    use fs_agent::permissions::Answer;
    use fs_agent::render::{ConsoleRequest, Key};

    let mut state = state();
    let (tx, mut submitted) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    let (ask, mut asked) = ask_permission();
    state.request(ask);

    // `x` 不是任何一个答案，所以取的是安全的那一个 —— 而问句
    // 盖着的草稿，永远见不到这个键。
    state.key(Key::Char('x'));
    assert_eq!(asked.try_recv().unwrap(), Answer::Deny);
    state.key(Key::Enter);
    // 草稿是空的，所以到达循环的那一行也是空的 —— 而
    // 空行是一行，不是输入的结束（循环会丢掉它）。
    assert_eq!(
        submitted.try_recv().unwrap(),
        Some(String::new()),
        "草稿仍然是空的"
    );
}

#[test]
fn the_wheel_follows_the_pointer_while_a_question_is_up() {
    // 这是 `tui-chrome` 推翻掉的那条旧规矩的替代品：问题立着时，
    // 滚轮**不再**被一口吃掉，而是看指针落在哪一块
    // （`.scratch/tui-chrome/spec.md` §5）。点击的优先级没变 ——
    // 覆盖层仍然先接点击，变的是滚轮。
    use fs_agent::render::{Key, RenderEvent};
    use ratatui::crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};

    let mut state = state();
    for index in 0..40 {
        state.apply(RenderEvent::Notice(format!("第 {index} 行")));
    }
    let _ = screen(120, 24, &mut state);
    state.key(Key::PageUp);
    let before = first_notice(&screen(120, 24, &mut state));

    let (ask, _rx) = ask_permission();
    state.request(ask);
    let _ = screen(120, 24, &mut state);

    // 指针落在覆盖层**之外**（这张 120x24 的屏上它横跨第 44 到 115 列，
    // 第 8 到 14 行）——那里是左栏，于是滚轮归背后的转录。
    state.mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 10,
        row: 10,
        modifiers: KeyModifiers::empty(),
    });
    let outside = first_notice(&screen(120, 24, &mut state));
    assert_ne!(outside, before, "覆盖层外的滚轮滚的是转录");

    // 落在覆盖层**里面**：模态自己没有可滚的内容，所以视口恰好停在原地。
    state.mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 60,
        row: 10,
        modifiers: KeyModifiers::empty(),
    });
    assert_eq!(
        first_notice(&screen(120, 24, &mut state)),
        outside,
        "覆盖层里的滚轮不碰它背后的转录"
    );
}

#[test]
fn the_overlay_blanks_what_is_behind_it_rather_than_drawing_over_it() {
    use fs_agent::render::Key;

    // 一个**短**问句：它两侧留出的地方正是面板自己
    // 标签所在之处，所以背景里剩下的任何东西都会出现在这些字旁边。
    let mut state = idle();
    state.paste("第一行\n第二行");
    state.key(Key::Esc);
    let rows = screen(120, 24, &mut state);
    let title = rows
        .iter()
        .position(|row| row.contains("清空输入"))
        .expect("覆盖层在屏幕上");
    let keys = rows
        .iter()
        .position(|row| row.contains("[y] 清空"))
        .expect("它的按钮也在");

    let frame = buffer(120, 24, &mut state);
    let borders: Vec<u16> = (MAIN_LEFT_AT_120..TRANSCRIPT_TEXT_RIGHT_AT_120)
        .filter(|x| frame[(*x, title as u16)].symbol() == "┆")
        .collect();
    assert_eq!(borders.len(), 2, "这个框的边框：{borders:?}");
    assert_eq!(
        cells(&frame, title as u16, borders[0] + 1, borders[1]).trim(),
        "清空输入",
        "内部装着标题，没有一样原先在它后面的东西"
    );
    assert_eq!(
        cells(&frame, keys as u16, borders[0] + 1, borders[1]).trim(),
        "[y] 清空   [n] 保留",
        "而按钮就是它们自己那一整行"
    );
}

/// 渲染出来的帧，以及它被显示时——如果有显示——光标落在哪里。
fn frame_and_cursor(width: u16, height: u16, state: &mut TuiState) -> (Buffer, Option<(u16, u16)>) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("TestBackend");
    terminal
        .draw(|frame| draw_frame(frame, state))
        .expect("画一帧");
    let backend = terminal.backend();
    let cursor = backend.cursor_visible().then(|| {
        let position = backend.cursor_position();
        (position.x, position.y)
    });
    (backend.buffer().clone(), cursor)
}

#[test]
fn the_cursor_comes_back_to_the_draft_once_a_question_is_answered() {
    use fs_agent::permissions::Answer;
    use fs_agent::render::Key;

    let mut state = state();
    for ch in "hi".chars() {
        state.key(Key::Char(ch));
    }
    let (_, before) = frame_and_cursor(120, 24, &mut state);
    let before = before.expect("光标坐在草稿里");

    let (ask, mut asked) = ask_permission();
    state.request(ask);
    let (_, during) = frame_and_cursor(120, 24, &mut state);
    assert_eq!(during, None, "问句拿走键盘，所以没有光标");

    state.key(Key::Char('y'));
    assert_eq!(asked.try_recv().unwrap(), Answer::Allow);
    let (_, after) = frame_and_cursor(120, 24, &mut state);
    assert_eq!(after, Some(before), "然后它回到原来那儿");
}

// ---------------------------------------------------------------------------
// 折叠：思考行、工具输出与详情覆盖层
// （票 01/02/03）
// ---------------------------------------------------------------------------

/// 一条给讨论者的 `MessageCompleted`。
fn message(seq: u64, text: &str, reasoning: Option<&str>) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, Role, SpeakerId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: text.to_owned(),
            reasoning: reasoning.map(str::to_owned),
        },
    ))
}

/// 一条来自讨论者的推理增量。
fn reasoning_delta(text: &str) -> fs_agent::render::RenderEvent {
    use fs_agent::events::SpeakerId;
    fs_agent::render::RenderEvent::Delta {
        speaker: SpeakerId::Debater("kimi".into()),
        kind: fs_agent::render::DeltaKind::Reasoning,
        text: text.to_owned(),
    }
}

/// 一条来自讨论者的正文增量。
fn text_delta(text: &str) -> fs_agent::render::RenderEvent {
    use fs_agent::events::SpeakerId;
    fs_agent::render::RenderEvent::Delta {
        speaker: SpeakerId::Debater("kimi".into()),
        kind: fs_agent::render::DeltaKind::Text,
        text: text.to_owned(),
    }
}

/// 一条来自讨论者的 `ToolCallStarted`。
fn tool_started(
    seq: u64,
    id: &str,
    tool: &str,
    args: serde_json::Value,
) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, SpeakerId, ToolCallId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new(id),
            tool_name: tool.to_owned(),
            args,
        },
    ))
}

/// 一条针对某次调用的 `PermissionAsked`。
fn permission_asked(seq: u64, id: &str) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, SpeakerId, ToolCallId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::PermissionAsked {
            request_id: "r-1".to_owned(),
            tool_call_id: ToolCallId::new(id),
            request: serde_json::json!({"tool_name": "bash", "args": {"command": "ls"}}),
        },
    ))
}

/// 一条 `PermissionDecided`：用户说了是。
fn permission_decided(seq: u64) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Decision, DecisionSource, Event, EventPayload, SpeakerId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::PermissionDecided {
            request_id: "r-1".to_owned(),
            decision: Decision::Allow,
            source: DecisionSource::User,
            reason: None,
        },
    ))
}

/// 一条针对先前开始的调用发来的 `ToolCallCompleted`。
fn tool_completed(
    seq: u64,
    id: &str,
    ok: bool,
    output: Option<&str>,
    error: Option<&str>,
) -> fs_agent::render::RenderEvent {
    use fs_agent::events::{Event, EventPayload, SpeakerId, ToolCallId};
    fs_agent::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::ToolCallCompleted {
            tool_call_id: ToolCallId::new(id),
            ok,
            output: output.map(str::to_owned),
            error: error.map(str::to_owned),
            duration_ms: 3,
        },
    ))
}

/// 点屏幕上一个格子。
fn click(column: u16, row: u16) -> ratatui::crossterm::event::MouseEvent {
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

#[test]
fn a_thinking_segment_opens_in_place_and_settles_in_place() {
    // 从屏幕上看整个状态机：只有推理到了的时候出现 `正在思考`，
    // 正文的第一个增量把同一行落定成 `思考完成`，不多加
    // 一行，而完成的轨迹留着（票 02 §1）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("先看依赖，"));
    state.apply(reasoning_delta("再看测试。"));

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(
        text.contains("[kimi] … 正在思考"),
        "未落定的那行读起来是进行中：{text}"
    );
    assert!(!text.contains("思考完成"), "而且没有自称已完成：{text}");

    state.apply(text_delta("答案是 42。"));
    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(
        text.contains("[kimi] ▸ ✓ 思考完成"),
        "同一行原地落定：{text}"
    );
    assert!(
        !text.contains("正在思考"),
        "进行中那一行没了，没有被复制：{text}"
    );
    assert_eq!(
        text.matches("思考完成").count(),
        1,
        "一段思考就是一行：{text}"
    );

    // 冻结之后正文仍然在流；完成的块把它换掉。
    state.apply(message(2, "答案是 42。", Some("先看依赖，再看测试。")));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("[kimi] 答案是 42。") || text.contains("答案是 42。"),
        "答案在转录里：{text}"
    );
    assert_eq!(
        text.matches("思考完成").count(),
        1,
        "已记下的轨迹不会多添一行思考行：{text}"
    );
}

#[test]
fn a_turn_with_no_reasoning_adds_no_thinking_line() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(message(1, "直接作答。", None));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        !text.contains("思考") && !text.contains("正在思考"),
        "没有推理就没有思考行：{text}"
    );
}

#[test]
fn a_synthesizer_trace_streams_but_records_nothing() {
    // 合成器发来推理增量，却写下 `reasoning: None`。这一行
    // 仍然落定 —— 读者看见它在想 —— 而它的详情说整段文本
    // 从没被记下（票 02 §1、票 04 §3）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("综合两边的意见。"));
    state.apply(message(2, "结论。", None));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("[kimi] ▸ ✓ 思考完成"),
        "没有记下轨迹时这一行也落定：{text}"
    );

    click_row(&mut state, 120, 24, "✓ 思考完成");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("本次未记录思考全文"),
        "详情说这段文本没有被记下：{text}"
    );
}

#[test]
fn a_tool_result_is_folded_into_its_call_line() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-1",
        "bash",
        serde_json::json!({"command": "cargo test"}),
    ));
    state.apply(tool_completed(
        2,
        "call-1",
        true,
        Some("line one\nline two\nline three"),
        None,
    ));

    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("[kimi] ▸ 调用 bash 运行 cargo test"),
        "调用行保住它的参数摘要，还多了一个记号：{text}"
    );
    assert!(
        !text.contains("line one") && !text.contains("line two"),
        "输出的正文被折起来了：{text}"
    );

    // 失败是同一行，末尾加 `失败` —— 绝不是第二行。
    let mut failed = state_with_roster(&["kimi"]);
    failed.apply(tool_started(
        1,
        "call-3",
        "read_file",
        serde_json::json!({"path": "missing.rs"}),
    ));
    failed.apply(tool_completed(
        2,
        "call-3",
        false,
        None,
        Some("no such file"),
    ));
    let rows = screen(120, 24, &mut failed);
    let text = rows.join("\n");
    assert!(
        text.contains("[kimi] ▸ 调用 read_file missing.rs 失败"),
        "失败是调用行上的一个后缀：{text}"
    );
    assert!(
        !text.contains("no such file"),
        "错误正文在详情里，不在转录里：{text}"
    );
}

#[test]
fn a_click_opens_the_detail_and_a_second_click_closes_it() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-9",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-9", true, Some("alpha\nbeta"), None));

    // 一个很高的终端，好让整段正文都放得下：最短的那个覆盖层会滚动，
    // 那是下一个测试的主题。
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("── 参数 ──"), "参数那一节：{text}");
    assert!(text.contains("── 输出 ──"), "输出那一节：{text}");
    assert!(text.contains("alpha"), "整段输出：{text}");
    assert!(text.contains("esc 关闭"), "页脚点出出口：{text}");

    // Esc 关上它，转录回来了。
    state.key(Key::Esc);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains("── 参数 ──"), "Esc 关上覆盖层：{text}");
    assert!(
        text.contains("调用 bash"),
        "它打开时所在的那一行还在：{text}"
    );
}

#[test]
fn the_detail_body_scrolls_with_the_keys_and_the_wheel() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-10",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    let body: Vec<String> = (0..40).map(|line| format!("输出第 {line} 行")).collect();
    state.apply(tool_completed(
        2,
        "call-10",
        true,
        Some(&body.join("\n")),
        None,
    ));

    click_row(&mut state, 120, 40, "调用 bash");
    // 正文从顶部开始，40 行时那儿是参数那一节。
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("── 参数 ──"), "正文从顶部开始：{text}");

    // 用翻页键一路走到最底下，然后单个箭头往回挪一行：
    // 箭头和翻页作用在同一段正文上。
    for _ in 0..8 {
        state.key(Key::PageDown);
    }
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("esc 关闭"), "正文走到了末尾：{text}");
    state.key(Key::Down);
    state.key(Key::Up);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("esc 关闭"), "而箭头仍然在它里面挪动：{text}");

    // 滚轮把同一段正文一格挪一行。
    let before = text.clone();
    state.mouse(wheel(ratatui::crossterm::event::MouseEventKind::ScrollDown));
    let after = screen(120, 40, &mut state).join("\n");
    assert_ne!(before, after, "滚轮挪动了详情正文");
}

#[test]
fn the_detail_overlay_reads_the_spilled_tool_output() {
    // 事件带的是预览；全文在那个工具调用 id 指名的文件里。
    // `SessionFacts.cwd` 是会话目录，所以覆盖层读
    // `<cwd>/outputs/<tool_call_id>.txt`（票 02 §4）。
    let dir = std::env::temp_dir().join(format!("fs-agent-detail-{}", std::process::id()));
    let outputs = dir.join("outputs");
    std::fs::create_dir_all(&outputs).expect("会话的 outputs 目录");
    std::fs::write(
        outputs.join("call-11.txt"),
        "the whole output\nwith a second line the preview never carried",
    )
    .expect("落盘的那个文件");

    let mut state = TuiState::new(SessionFacts {
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: dir.display().to_string(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        // 会话被组装时所处的模式：状态行里 `模式 …` 那一栏。想测另一档的
        // 测试在自己的 facts 里覆盖它。
        mode: fs_agent::permissions::Mode::Ask,
        budget_limit: Some(100_000),
        speaker_order: vec!["kimi".to_owned()],
    });
    state.apply(tool_started(
        1,
        "call-11",
        "bash",
        serde_json::json!({"command": "cat big"}),
    ));
    state.apply(tool_completed(
        2,
        "call-11",
        true,
        Some("the whole output\n[已截断：999 字符]"),
        None,
    ));

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("with a second line the preview never carried"),
        "显示的是落盘全文，不是预览：{text}"
    );
    assert!(!text.contains("全文不可用"), "而且没有降级说明：{text}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_missing_spilled_file_degrades_to_the_preview() {
    // 压根没有 `outputs/` 目录：详情显示事件自带的预览，
    // 并说明全文拿不到（票 02 §4）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-12",
        "bash",
        serde_json::json!({"command": "cat big"}),
    ));
    state.apply(tool_completed(
        2,
        "call-12",
        true,
        Some("head of the output\n[已截断：999 字符]"),
        None,
    ));

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("head of the output"), "预览：{text}");
    assert!(text.contains("全文不可用"), "降级被说出来了：{text}");
}

#[test]
fn a_question_in_the_way_keeps_the_collapsed_lines_unclickable() {
    // 问句独占指针：它起来的时候点一条折起的行会被
    // 丢掉，也不会在问句上面打开详情（票 02 §4、票 04 §2）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-13",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-13", true, Some("body"), None));

    // 在问句盖住窗格之前，先找到调用行那一行。
    let row = row_of(&mut state, 120, 40, "调用 bash").expect("调用行画出来了");
    state.request(ask_permission().0);
    state.mouse(click(20, row));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("── 参数 ──"),
        "详情没有在问句上面打开：{text}"
    );
    assert!(text.contains("权限询问"), "而屏幕上还是那个问句：{text}");
}

/// 注入名册是给定讨论者名字的状态，转录里的名字
/// 正是靠它上色（票 07 §1）。
fn state_with_roster(names: &[&str]) -> TuiState {
    TuiState::new(SessionFacts {
        speaker_order: names.iter().map(|name| (*name).to_owned()).collect(),
        ..facts()
    })
}

/// 滚轮一格。
fn wheel(kind: ratatui::crossterm::event::MouseEventKind) -> ratatui::crossterm::event::MouseEvent {
    use ratatui::crossterm::event::{KeyModifiers, MouseEvent};
    // 落在问卷占着的底部块里（120x24 下输入区是第 19 到 21 行）。
    // 详情覆盖层整块占着指针，所以这一个坐标对它没有影响。
    MouseEvent {
        kind,
        column: 41,
        row: 20,
        modifiers: KeyModifiers::empty(),
    }
}

/// 某句话画在哪一行，重新渲染一次。
fn row_of(state: &mut TuiState, width: u16, height: u16, needle: &str) -> Option<u16> {
    let rows = screen(width, height, state);
    rows.iter()
        .position(|row| row.contains(needle))
        .map(|row| row as u16)
}

/// 点某句话画在的那一行。
///
/// 先渲染帧，因为只有画出来的东西才点得到，而
/// 那句话是在同一帧里查的（票 04 §1）。
fn click_row(state: &mut TuiState, width: u16, height: u16, needle: &str) {
    let Some(row) = row_of(state, width, height, needle) else {
        panic!("屏幕上没有东西包含 {needle:?}");
    };
    // 点击的列只要落在这一行里就行；行才是窗格
    // 映射回源代码行的东西。
    state.mouse(click(10, row));
}

// ---------------------------------------------------------------------------
// 两种问句形状上的鼠标作答（票 04）
// ---------------------------------------------------------------------------

/// 某句话起始的那个屏幕格，自上而下搜。
///
/// 匹配是拿一行**按终端读它的方式**读出来的 —— 一个宽字素
/// 前进两列、它后面那一格跳过 —— 所以匹配报出来的列
/// 是屏幕列，而鼠标事件带的正是它。
fn cell_of(frame: &Buffer, width: u16, height: u16, needle: &str) -> Option<(u16, u16)> {
    for y in 0..height {
        let row = row_text(frame, y, width);
        if let Some(at) = row.find(needle) {
            return Some((text_columns(&row[..at]) as u16, y));
        }
    }
    None
}

/// 点新渲染出来的帧上画着 `needle` 的第一个格子。
fn click_text(state: &mut TuiState, width: u16, height: u16, needle: &str) {
    let frame = buffer(width, height, state);
    let Some((column, row)) = cell_of(&frame, width, height, needle) else {
        panic!("屏幕上没有东西包含 {needle:?}");
    };
    state.mouse(click(column, row));
}

/// 点某一屏幕行里画着 `needle` 的那个格子。
///
/// 问卷页脚的标签同时也是它正文的用词，所以两者只能
/// 靠它们所在的行分辨 —— 这正是指针
/// 做的区分（票 04 §4）。
fn click_in_row(state: &mut TuiState, width: u16, height: u16, row: u16, needle: &str) {
    let frame = buffer(width, height, state);
    let text = row_text(&frame, row, width);
    let Some(at) = text.find(needle) else {
        panic!("第 {row} 行不包含 {needle:?}：{text:?}");
    };
    let column = text_columns(&text[..at]) as u16;
    state.mouse(click(column, row));
}

#[test]
fn a_permission_question_is_answered_by_clicking_a_button() {
    for (label, expected) in [
        ("[y] 允许", fs_agent::permissions::Answer::Allow),
        ("[a] 总是允许", fs_agent::permissions::Answer::AlwaysAllow),
        ("[n] 拒绝", fs_agent::permissions::Answer::Deny),
    ] {
        let mut state = state_with_roster(&["kimi"]);
        let (request, mut answer) = ask_permission();
        state.request(request);
        click_text(&mut state, 120, 24, label);
        assert_eq!(
            answer.try_recv().expect("答案送出去了"),
            expected,
            "点 {label} 与按它对应的键一样作答"
        );
        // 覆盖层没了，键盘回到草稿上。
        let text = screen(120, 24, &mut state).join("\n");
        assert!(!text.contains("权限询问"), "问句关上了：{text}");
    }
}

#[test]
fn clicking_a_question_body_or_border_does_nothing() {
    let mut state = state_with_roster(&["kimi"]);
    let (request, mut answer) = ask_permission();
    state.request(request);

    // 标题行，以及覆盖层正文的中间。
    for (column, row) in [(60, 10), (60, 11), (2, 10)] {
        state.mouse(click(column, row));
    }
    assert!(answer.try_recv().is_err(), "点在按钮之外不作答");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("权限询问"), "而问句还在：{text}");
}

#[test]
fn the_renderer_confirmations_answer_by_click() {
    // 退出确认是渲染器自己的：点 `[y] 退出` 就退出。
    let (mut idle, _line) = {
        let mut state = state_with_roster(&["kimi"]);
        let (reply, line) = tokio::sync::oneshot::channel();
        state.request(ConsoleRequest::Prompt { reply });
        (state, line)
    };
    idle.key(Key::CtrlD);
    click_text(&mut idle, 120, 24, "[y] 退出");
    assert!(idle.should_quit(), "这次点击确认了退出");

    // 而点它上面的 `[n] 取消` 让会话继续跑。
    let (mut escaped, _line) = {
        let mut state = state_with_roster(&["kimi"]);
        let (reply, line) = tokio::sync::oneshot::channel();
        state.request(ConsoleRequest::Prompt { reply });
        (state, line)
    };
    escaped.key(Key::CtrlD);
    click_text(&mut escaped, 120, 24, "[n] 取消");
    assert!(!escaped.should_quit(), "安全的那个答案不是退出");
}

/// 一个屏幕上摆着 `question` 的问卷，以及它的答案接收端。
fn questionnaire_state(
    question: fs_agent::questions::UserQuestion,
) -> (
    TuiState,
    tokio::sync::oneshot::Receiver<Result<fs_agent::questions::UserAnswers, String>>,
) {
    use fs_agent::render::QuestionnaireRequest;
    let mut state = state_with_roster(&["kimi"]);
    let (reply, answers) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Questionnaire(QuestionnaireRequest {
        questions: vec![question],
        reply,
    }));
    (state, answers)
}

#[test]
fn a_single_select_option_is_chosen_by_clicking_its_row() {
    use fs_agent::questions::{Choice, UserQuestion};
    let (mut state, mut answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![
            Choice {
                label: "甲".to_owned(),
                description: None,
            },
            Choice {
                label: "乙".to_owned(),
                description: None,
            },
        ],
    });

    // 唯一的那个问题上，点一下作答但**不**提交：最后一
    // 个问题仍需单独提交（票 04 §4）。
    click_text(&mut state, 120, 24, "2. 乙");
    assert!(answers.try_recv().is_err(), "最后一个问题点一下只作答");

    // 所有问题都处理完之后，`Enter` 才是提交。
    state.key(Key::Enter);
    let answers = answers.try_recv().expect("问卷提交了");
    let answers = answers.expect("一次成功的作答");
    assert_eq!(answers.answers.len(), 1);
    assert_eq!(answers.answers[0].selected, vec!["乙".to_owned()]);
}

#[test]
fn a_multi_select_option_only_toggles_when_clicked() {
    use fs_agent::questions::{Choice, UserQuestion};
    let (mut state, mut answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选几个".to_owned(),
        multi_select: true,
        options: vec![
            Choice {
                label: "甲".to_owned(),
                description: None,
            },
            Choice {
                label: "乙".to_owned(),
                description: None,
            },
        ],
    });

    click_text(&mut state, 120, 24, "1. 甲");
    assert!(answers.try_recv().is_err(), "多选上点一下不提交");
    // 勾落在点击落到的那一行上。
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("[x] 1. 甲"), "选中被显示出来了：{text}");

    // 处理过东西之后页脚的提交按钮才出现，而且能提交。
    let row = row_of(&mut state, 120, 40, "提交").expect("提交按钮画出来了");
    click_in_row(&mut state, 120, 40, row, "提交");
    let answers = answers.try_recv().expect("提交按钮提交了");
    assert_eq!(
        answers.expect("一次成功的作答").answers[0].selected,
        vec!["甲".to_owned()]
    );
}

#[test]
fn the_questionnaire_footer_pages_with_a_click() {
    use fs_agent::questions::{Choice, UserQuestion};
    let (mut state, mut answers) = {
        use fs_agent::render::QuestionnaireRequest;
        let mut state = state_with_roster(&["kimi"]);
        let (reply, answers) = tokio::sync::oneshot::channel();
        let question = |id: &str| UserQuestion {
            id: id.to_owned(),
            header: None,
            question: format!("第 {id} 题"),
            multi_select: false,
            options: vec![Choice {
                label: "唯一".to_owned(),
                description: None,
            }],
        };
        state.request(ConsoleRequest::Questionnaire(QuestionnaireRequest {
            questions: vec![question("q1"), question("q2")],
            reply,
        }));
        (state, answers)
    };

    // 第一个问题没有 `← 上一题`；它确实有 `下一题 →`。
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains("← 上一题"), "第一个问题上没有上一题：{text}");
    assert!(text.contains("下一题 →"), "但有下一题：{text}");

    // 点一下往前走，第二个问题提供回去的路。
    click_in_row(&mut state, 120, 24, 23, "下一题 →");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("2 / 2"), "这次点击翻到下一页：{text}");
    assert!(text.contains("← 上一题"), "回去的路出现了：{text}");
    click_in_row(&mut state, 120, 24, 23, "← 上一题");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("1 / 2"), "这次点击翻回上一页：{text}");
    assert!(answers.try_recv().is_err(), "翻页从不提交");
}

#[test]
fn clicking_the_custom_row_hands_it_the_cursor_and_paging_takes_it_back() {
    use fs_agent::questions::{Choice, UserQuestion};
    let (mut state, _answers) = {
        use fs_agent::render::QuestionnaireRequest;
        let mut state = state_with_roster(&["kimi"]);
        let (reply, answers) = tokio::sync::oneshot::channel();
        let question = |id: &str| UserQuestion {
            id: id.to_owned(),
            header: None,
            question: format!("第 {id} 题"),
            multi_select: false,
            options: vec![Choice {
                label: "唯一".to_owned(),
                description: None,
            }],
        };
        state.request(ConsoleRequest::Questionnaire(QuestionnaireRequest {
            questions: vec![question("q1"), question("q2")],
            reply,
        }));
        (state, answers)
    };

    let (_, before) = frame_and_cursor(120, 24, &mut state);
    assert_eq!(before, None, "这一行被聚焦之前没有光标");

    click_text(&mut state, 120, 24, "自定义：");
    let (_, focused) = frame_and_cursor(120, 24, &mut state);
    assert!(focused.is_some(), "这次点击把光标放到自定义那一行上");

    // 翻走会重置焦点：下一个问题的自定义行起步时没有焦点。
    click_in_row(&mut state, 120, 24, 23, "下一题 →");
    let (_, after) = frame_and_cursor(120, 24, &mut state);
    assert_eq!(after, None, "翻页重置了焦点");
}

#[test]
fn the_wheel_moves_the_questionnaire_highlight() {
    use fs_agent::questions::{Choice, UserQuestion};
    let (mut state, _answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![
            Choice {
                label: "甲".to_owned(),
                description: None,
            },
            Choice {
                label: "乙".to_owned(),
                description: None,
            },
        ],
    });
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("> ○ 1. 甲"), "高亮从 1 开始：{text}");

    state.mouse(wheel(ratatui::crossterm::event::MouseEventKind::ScrollDown));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("> ○ 2. 乙"), "滚轮挪动了高亮：{text}");
}

#[test]
fn the_wheel_over_the_transcript_scrolls_it_while_a_questionnaire_is_up() {
    // 问卷占着底部输入区，而转录还在上面露着 —— 指针在转录上，
    // 滚的就是转录；问卷的高亮一动不动（`.scratch/tui-chrome/spec.md` §5）。
    use fs_agent::questions::{Choice, UserQuestion};
    use ratatui::crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};

    let (mut state, _answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![
            Choice {
                label: "甲".to_owned(),
                description: None,
            },
            Choice {
                label: "乙".to_owned(),
                description: None,
            },
        ],
    });
    for index in 0..40 {
        state.apply(fs_agent::render::RenderEvent::Notice(format!(
            "第 {index} 行"
        )));
    }
    let _ = screen(120, 24, &mut state);
    let before = first_notice(&screen(120, 24, &mut state));

    // 指针落在转录上（第 5 行），而问卷在下半屏。
    state.mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 60,
        row: 5,
        modifiers: KeyModifiers::empty(),
    });
    let after = first_notice(&screen(120, 24, &mut state));
    assert_ne!(after, before, "指针在转录上就滚转录");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("> ○ 1. 甲"), "而高亮留在原处：{text}");
}

#[test]
fn one_message_never_gets_two_thinking_lines() {
    // 正文的第一个增量早早就把这行落定了，远在 `MessageCompleted`
    // 到来之前 —— 而完成事件带着整段轨迹，所以一句天真的
    // 「没开就开」会给同一个念头添第二行。这里事件之间
    // 不画帧，而正是这一点让这个测试的早期版本
    // 通过了（票 02 §1）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("先看依赖。"));
    state.apply(text_delta("答案。"));
    state.apply(message(1, "答案。", Some("先看依赖。")));
    let text = screen(120, 40, &mut state).join("\n");
    assert_eq!(
        text.matches("思考完成").count(),
        1,
        "一段思考就是一行：{text}"
    );
    assert!(!text.contains("正在思考"), "进行中那一行没了：{text}");
}

#[test]
fn reasoning_never_joins_the_message_body() {
    // 推理折叠进它自己那一行；放它进实时尾巴的话
    // 会在 `正在思考` 底下打印出原始念头（票 02 §3）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("这是不该出现的思考正文。"));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("… 正在思考"), "思考行在那儿：{text}");
    assert!(
        !text.contains("这是不该出现的思考正文"),
        "而原始推理不在：{text}"
    );
}

#[test]
fn the_detail_overlay_freezes_the_transcript() {
    // 打开了一行的读者会一直看着它：覆盖层起来时到来的
    // 输出不许把窗格往下拽（票 02 §4）。
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..40 {
        state.apply(fs_agent::render::RenderEvent::Notice(format!(
            "第 {index} 行"
        )));
    }
    state.apply(tool_started(
        1,
        "call-20",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-20", true, Some("body"), None));
    let _ = screen(120, 24, &mut state);

    let _ = screen(120, 24, &mut state);
    click_row(&mut state, 120, 24, "调用 bash");
    let before = screen(120, 24, &mut state);
    assert!(before.join("\n").contains("── 参数 ──"), "覆盖层起来了");
    let frozen = transcript_text(&buffer(120, 24, &mut state), transcript_rows(&before));

    // 覆盖层开着的时候有新输出到来。
    for index in 40..60 {
        state.apply(fs_agent::render::RenderEvent::Notice(format!(
            "第 {index} 行"
        )));
    }
    let during = screen(120, 24, &mut state);
    let after = transcript_text(&buffer(120, 24, &mut state), transcript_rows(&during));
    assert_eq!(after, frozen, "覆盖层后面的转录没有动");
    // 到来的那些行也不算进读者的「新行」里：位置是
    // 覆盖层在保的，所以「N 行新内容」会是在它底下数。
    let counter = |rows: &[String]| {
        rows.iter()
            .find(|row| row.contains("行新内容"))
            .cloned()
            .unwrap_or_default()
    };
    assert_eq!(counter(&during), counter(&before), "而新行计数也被按住了");
}

#[test]
fn a_question_closes_the_detail_overlay_instead_of_stacking_on_it() {
    // 问句不能画在覆盖层之上：底下那个模态会
    // 没法作答，因为拥有键盘的是覆盖层（票 02 §4）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-21",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-21", true, Some("body"), None));
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("── 参数 ──"), "覆盖层打开了：{text}");

    state.request(ask_permission().0);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains("── 参数 ──"), "覆盖层为问句让了位：{text}");
    assert!(text.contains("权限询问"), "而问句起来了：{text}");
}

#[test]
fn the_questionnaire_footer_buttons_hit_where_they_are_drawn() {
    // 页脚按钮之间的空隙既被数也被画，所以
    // 点击落进的区域就是那些字形所在的按钮 —— 这里钉的是
    // 那条 bug：空隙只数不画，会把每个区域都挪出去三列
    // （票 04 §7）。
    use fs_agent::questions::{Choice, UserQuestion};
    let (mut state, mut answers) = {
        use fs_agent::render::QuestionnaireRequest;
        let mut state = state_with_roster(&["kimi"]);
        let (reply, answers) = tokio::sync::oneshot::channel();
        let question = |id: &str| UserQuestion {
            id: id.to_owned(),
            header: None,
            question: format!("第 {id} 题"),
            multi_select: false,
            options: vec![Choice {
                label: "唯一".to_owned(),
                description: None,
            }],
        };
        state.request(ConsoleRequest::Questionnaire(QuestionnaireRequest {
            questions: vec![question("q1"), question("q2")],
            reply,
        }));
        (state, answers)
    };

    // 第一个问题上只画了 `下一题 →`。点它的字形就前进。
    click_in_row(&mut state, 120, 24, 23, "下一题 →");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("2 / 2"), "这次点击前进了：{text}");

    // 第二个问题先画的是 `← 上一题`：点它就回去，别让
    // 它落在别的任何东西上。
    click_in_row(&mut state, 120, 24, 23, "← 上一题");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("1 / 2"), "这次点击回去了：{text}");
    assert!(answers.try_recv().is_err(), "翻页从不提交");
}

#[test]
fn a_thinking_line_tints_its_speakers_name() {
    // 思考提示带着一个 `speaker_label`，所以它的名字取讨论者的
    // 颜色，而标记与状态词保持叙述灰
    // （票 07 §2）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("先看依赖。"));
    state.apply(text_delta("答案。"));

    let frame = buffer(120, 40, &mut state);
    let Some((column, row)) = cell_of(&frame, 120, 40, "✓ 思考完成") else {
        panic!("思考行在屏幕上");
    };
    // 名字正好坐在标记前面。
    let name_x = column - text_columns("▸ ") as u16 - text_columns("[kimi] ") as u16;
    assert_eq!(frame[(name_x, row)].symbol(), "[", "名字前缀在那儿");
    assert_eq!(
        frame[(name_x, row)].fg,
        Color::LightCyan,
        "这个名字取的是名册的第一个位置"
    );
    assert_eq!(
        frame[(column, row)].fg,
        Color::DarkGray,
        "而状态词保持叙述灰"
    );
}

#[test]
fn reasoning_that_interleaves_opens_a_new_line_per_segment() {
    // 推理增量与正文增量交替出现，所以单条消息可以装好几
    // 段思考：每一段在它站的地方落定，下一段开自己
    // 那一行（票 02 §1）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("第一段思考。"));
    state.apply(text_delta("第一段正文。"));
    state.apply(reasoning_delta("第二段思考。"));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("… 正在思考"), "第二段还开着：{text}");
    assert_eq!(
        text.matches("思考完成").count(),
        1,
        "而第一段落定了：{text}"
    );

    state.apply(message(1, "第一段正文。", Some("第一段思考。第二段思考。")));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("正在思考"),
        "完成事件把开着的那一段落定：{text}"
    );
}

#[test]
fn ctrl_d_closes_the_detail_overlay_rather_than_asking_to_quit() {
    // 「覆盖层忽略其它所有键」的唯一例外：`Ctrl-D` 关上它
    // 而不是打开退出确认（票 06 §5）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-22",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-22", true, Some("body"), None));
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("── 参数 ──"), "覆盖层打开了：{text}");

    state.key(Key::CtrlD);
    assert!(!state.should_quit(), "关上不是退出");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains("── 参数 ──"), "覆盖层关上了：{text}");
    assert!(!text.contains("退出会话"), "而且没有问过确认：{text}");
}

#[test]
fn a_tool_body_over_the_reading_limit_is_cut_and_says_so() {
    // 落盘文件自己没有上限，所以裁的是读者那份，
    // 而正文说出这件事（票 02 §4）。
    let dir = std::env::temp_dir().join(format!("fs-agent-detail-big-{}", std::process::id()));
    let outputs = dir.join("outputs");
    std::fs::create_dir_all(&outputs).expect("会话的 outputs 目录");
    let big = "x".repeat(200_001);
    std::fs::write(outputs.join("call-23.txt"), &big).expect("落盘的那个文件");

    let mut state = TuiState::new(SessionFacts {
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: dir.display().to_string(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        // 会话被组装时所处的模式：状态行里 `模式 …` 那一栏。想测另一档的
        // 测试在自己的 facts 里覆盖它。
        mode: fs_agent::permissions::Mode::Ask,
        budget_limit: Some(100_000),
        speaker_order: vec!["kimi".to_owned()],
    });
    state.apply(tool_started(
        1,
        "call-23",
        "bash",
        serde_json::json!({"command": "cat big"}),
    ));
    // 事件的 `output` 是**裁过**的预览，而正是它说明还有一整份
    // 文件可去读 —— 预览里没有标记就是整段正文，而
    // 详情不会去找一个从没写过的文件。
    state.apply(tool_completed(
        2,
        "call-23",
        true,
        Some(
            "head\n[已截断：200001 字符，约 50000 token；全文在 \
             /tmp/nonexistent-elsewhere/call-23.txt]\ntail",
        ),
        None,
    ));
    click_row(&mut state, 120, 40, "调用 bash");

    // 标记远在可见正文的下面，所以走到它的末尾。
    for _ in 0..4000 {
        state.key(Key::PageDown);
    }
    let rows = screen(120, 40, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("已截断"), "裁剪被说出来了：{text}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_detail_overlay_ignores_every_key_but_its_own() {
    // 覆盖层独占键盘：`Esc` 与 `Ctrl-D` 关上它，箭头与
    // 翻页键滚动它，而**别的全部忽略** —— 包括 `Ctrl-C`，所以
    // 一个误碰的手势不能从读者手底下退出或取消（票 02 §4）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-24",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-24", true, Some("body"), None));
    click_row(&mut state, 120, 40, "调用 bash");

    state.key(Key::CtrlC);
    assert!(!state.should_quit(), "Ctrl-C 不会从覆盖层里退出");
    assert!(state.take_events().is_empty(), "也不会取消任何东西");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("── 参数 ──"), "覆盖层还开着：{text}");

    // 可打印的键也到不了草稿：覆盖层的键就是
    // 覆盖层的，它后面的编辑器没在被打字。
    state.key(Key::Char('x'));
    // 详情覆盖层现在屏幕居中（`.scratch/tui-chrome/spec.md` §4），
    // 120x40 下它横跨第 2 到 117 列，把输入区的提示符也压在下面 ——
    // 所以草稿要等覆盖层关掉之后才看得见。
    state.key(Key::Esc);
    let rows = screen(120, 40, &mut state);
    let input = rows
        .iter()
        .position(|row| row.contains(editor::PROMPT))
        .expect("输入区那一行画出来了");
    assert!(
        !rows[input].contains('x'),
        "草稿里什么都没落进去：{:?}",
        rows[input]
    );
}

#[test]
fn a_tool_call_is_on_screen_as_soon_as_its_result_arrives() {
    // 转录过去会把一次调用开着，直到*后面*某个不相关的事件把它关掉，
    // 所以调用行要等模型已经答完下一轮迭代才出现：
    // 在整次工具运行的这一刻之前，转录关于这次调用
    // 什么都没显示（票 02 §3）。结果才是结束调用的东西，也正是
    // 必须画它的东西。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-30",
        "bash",
        serde_json::json!({"command": "ls -la"}),
    ));
    state.apply(tool_completed(2, "call-30", true, Some("total 0"), None));

    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("调用 bash 查看"),
        "结果一落地调用行就起来了：{text}"
    );

    // **针对这次调用**的权限询问也不拖它：
    // 调用还在进行中的时候叙述就已经画出来了，而调用行照旧
    // 随结果到来。
    let mut asked = state_with_roster(&["kimi"]);
    asked.apply(tool_started(
        1,
        "call-31",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    asked.apply(permission_asked(2, "call-31"));
    asked.apply(permission_decided(3));
    let text = screen(120, 24, &mut asked).join("\n");
    assert!(text.contains("权限询问"), "问句先被叙述出来：{text}");
    assert!(
        !text.contains("调用 bash"),
        "而调用不会在它的结果之前画出来：{text}"
    );
    asked.apply(tool_completed(4, "call-31", true, Some("out"), None));
    let text = screen(120, 24, &mut asked).join("\n");
    assert!(
        text.contains("调用 bash 查看"),
        "结果一落地调用行就起来了：{text}"
    );
}

#[test]
fn the_detail_overlay_is_wider_than_a_question() {
    // 正文是一页，所以它拿到这份余地：上限从 90 涨到 135，而 139
    // 列以下压着它的是覆盖层自己的边距，不是那个上限（票 03
    // §Answer，2026-09-23 加宽 50%）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-41",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-41", true, Some("body"), None));
    click_row(&mut state, 120, 40, "调用 bash");
    let frame = buffer(120, 40, &mut state);
    let detail = overlay_width(&frame, 120, 40).expect("覆盖层的上边框");

    // 问句的覆盖层更窄，而且画法一样：问一个来
    // 量它。
    let mut asked = state_with_roster(&["kimi"]);
    asked.request(ask_permission().0);
    let frame = buffer(120, 40, &mut asked);
    let question = overlay_width(&frame, 120, 40).expect("问句的上边框");

    assert!(
        detail > question,
        "详情覆盖层（{detail}）比问句的（{question}）宽"
    );
    // 120 列下压着详情覆盖层的是屏幕自己的边距：120 减去
    // 边距留住的四列，给它 116（那个 135 列的上限到不了）；
    // 问句的覆盖层仍然对着主列量，留下 72。
    assert_eq!(detail, 116, "120 列下的详情覆盖层");
    assert_eq!(question, 72, "而问句那个 72 列的上限");

    // 要撞到上限，终端得宽到屏幕自己长过它：
    // 200 列留下 196，四列的边距再留下 192 —— 越过了详情
    // 覆盖层从不超出的那个 135。
    let mut wide = state_with_roster(&["kimi"]);
    wide.apply(tool_started(
        1,
        "call-41",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    wide.apply(tool_completed(2, "call-41", true, Some("body"), None));
    click_row(&mut wide, 200, 40, "调用 bash");
    let frame = buffer(200, 40, &mut wide);
    let detail = overlay_width(&frame, 200, 40).expect("覆盖层的上边框");
    assert_eq!(detail, 135, "而在宽终端上压着它的是那个上限");
}

/// 一个浮动框画出来的宽度，从它上边框所在的那一行读出来。
///
/// 覆盖层都不贴第一列：详情居中在屏幕上，问句居中在主列里，
/// 两者两侧都留着余地 —— 所以找这个框的办法是找那个**不在**第一列的 `┌`，
/// 然后量到与它配对的 `┐`。
fn overlay_width(frame: &Buffer, width: u16, height: u16) -> Option<u16> {
    for y in 0..height {
        // 按列算，不按字节偏移：带 CJK 的一行，字节数比
        // 宽度长，而框是按列量的。
        let mut left = None;
        for x in 1..width {
            match frame[(x, y)].symbol() {
                "┌" if left.is_none() => left = Some(x),
                "┐" => {
                    if let Some(left) = left {
                        return Some(x - left + 1);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

#[test]
fn a_click_outside_the_detail_overlay_closes_it() {
    // 覆盖层之外整圈帧都是可以点下去的关闭目标：它来的那一
    // 行、它周围的转录、面板、页脚（票 02 §4，2026-09-23
    // 修正，原先只认「再点同一行」）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-40",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-40", true, Some("body"), None));
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("── 参数 ──"), "覆盖层打开了：{text}");

    // 外面：覆盖层在主列里留出的边距，在它来
    // 自的那一行上。
    let row = row_of(&mut state, 120, 40, "调用 bash").expect("调用行");
    state.mouse(click(MAIN_LEFT_AT_120, row));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains("── 参数 ──"), "点在转录上把它关上了：{text}");

    // 里面：什么都不发生，因为覆盖层自己没有按钮 —— 而且
    // 这包括覆盖层盖住它时那一行原本所在的屏幕行。老的
    // 「再点同一行」**不是**关上被盖住的行的办法；可靠的
    // 出口是 `Esc`、`Ctrl-D`，以及在别处点一下（票 02 §4，2026-09-23 修正）。
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("── 参数 ──"), "重新打开了：{text}");
    state.mouse(click(60, 12));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("── 参数 ──"),
        "在里面点一下它照样开着：{text}"
    );
    // 决定的是**屏幕位置**，不是它底下压着哪一条转录行：
    // 点在覆盖层矩形里面什么都不发生。所以当覆盖层是从某一行
    // 打开来的时候，而那一行正好在覆盖层底下 —— 通常都在那儿，因为
    // 覆盖层盖住主列的大部分 —— 点那儿也什么都不发生，
    // 而出路是 `Esc` / `Ctrl-D` / 在别处点一下（票 02 §4，2026-09-23 修正）。
    // 上面那次「里面什么都不发生」的点击就是这种情况：(60, 12) 在这个尺寸下
    // 落在覆盖层的矩形里，在下面读出来的两条边框之间。
    let frame = buffer(120, 40, &mut state);
    let overlay = overlay_width(&frame, 120, 40).expect("覆盖层");
    assert_eq!(overlay, 116, "120 列下压着详情覆盖层的是屏幕自己的边距");
    assert_eq!(
        (frame[(2, 12)].symbol(), frame[(117, 12)].symbol()),
        ("┆", "┆"),
        "那就是它在那次点击落到的行上的两条边 —— 屏幕居中，所以压着左栏"
    );
}

#[test]
fn a_settling_thinking_line_keeps_the_history_before_it() {
    // 活的会话在事件**之间**画帧，而正是它填满窗格的
    // 折行缓存。于是思考行会*原地*落定，而那次重写不能
    // 把在它之前的所有显示行扔掉。
    //
    // 它扔了。`replace_last` 清掉了整份折行缓存，而 `starts` 仍然
    // 指着旧的偏移，所以窗格回来时几乎没有行：
    // 历史不见了，窗格不再填满自己的高度，PgUp 也没什么
    // 可滚的。当时报的是「bash 命令都没了 / 输出没有占满屏幕 / PgUp 没有反映」
    // （2026-09-23）。
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..40 {
        state.apply(fs_agent::render::RenderEvent::Notice(format!(
            "第 {index} 行"
        )));
    }
    // 先来一帧，然后是中间夹着一帧的实时思考段。
    let _ = screen(120, 24, &mut state);
    state.apply(reasoning_delta("先想一下。"));
    let _ = screen(120, 24, &mut state);
    state.apply(text_delta("答案。"));

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("▸ ✓ 思考完成"), "思考行落定了：{text}");
    assert!(text.contains("答案。"), "而落定它的那段正文也在：{text}");
    let notices = rows.iter().filter(|row| row.contains("第 ")).count();
    assert!(
        notices >= 5,
        "它之前的历史还在屏幕上（{notices} 行）：{text}"
    );
    state.key(Key::PageUp);
    let rows = screen(120, 24, &mut state);
    assert!(
        rows.iter().any(|row| row.contains("第 2")),
        "而 PgUp 还能翻到更早：{}",
        rows.join("\n")
    );
}

#[test]
fn a_complete_result_does_not_claim_its_text_is_unavailable() {
    // 只有**被裁过**的结果才有落盘文件；短的结果从没写到
    // 任何地方，它的预览*就是*整段正文。对它说 `全文不可用` 是
    // 读者看不穿的谎 —— 读起来像「这份详情不完整」
    // （2026-09-23，用户报告：每个 bash 详情末尾都是 `--- 标准错误 ---`
    // 全文不可用`）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-50",
        "bash",
        serde_json::json!({"command": "echo hi"}),
    ));
    state.apply(tool_completed(
        2,
        "call-50",
        true,
        Some("退出码：0\n--- 标准输出 ---\nhi\n--- 标准错误 ---\n"),
        None,
    ));
    click_row(&mut state, 120, 40, "调用 bash");

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("--- 标准错误 ---"),
        "工具自己的那些小节原样显示：{text}"
    );
    assert!(
        !text.contains("全文不可用"),
        "而从没被裁过的正文不会被说成降级：{text}"
    );
}

#[test]
fn a_cut_result_still_says_when_the_whole_text_is_gone() {
    // 同一条规矩的另一半：真被裁过的预览，若没有文件
    // 可以读回来，就留着那句说明。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-51",
        "bash",
        serde_json::json!({"command": "cat big"}),
    ));
    state.apply(tool_completed(
        2,
        "call-51",
        true,
        Some("head\n[已截断：999 字符，约 250 token；全文在 /x/outputs/call-51.txt]\ntail"),
        None,
    ));
    click_row(&mut state, 120, 40, "调用 bash");

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("全文不可用"),
        "被裁过的正文没有可读文件时照实说：{text}"
    );
}

#[test]
fn a_tool_call_line_describes_the_call_and_folds_the_arguments_away() {
    // `调用 工具 描述`：读者看到这次调用*为的*是什么，而具体的
    // 参数在详情里，一次点击之外（票 02 §2，2026-09-23 修正）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-60",
        "bash",
        serde_json::json!({"command": "find .scratch -type f"}),
    ));
    state.apply(tool_completed(2, "call-60", true, Some("out"), None));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("[kimi] ▸ 调用 bash 查询 .scratch"),
        "这一行描述这次调用：{text}"
    );
    assert!(
        !text.contains("find .scratch -type f"),
        "而参数不印在上面：{text}"
    );

    // 问卷用问题自己的头部来描述自己。
    let mut asked = state_with_roster(&["kimi"]);
    asked.apply(tool_started(
        1,
        "call-61",
        "ask_user_question",
        serde_json::json!({"questions": [{"id": "q", "header": "下一步", "question": "接着做哪个？"}]}),
    ));
    asked.apply(tool_completed(2, "call-61", false, None, Some("declined")));
    let text = screen(120, 40, &mut asked).join("\n");
    assert!(
        text.contains("[kimi] ▸ 调用 ask_user_question 下一步 失败"),
        "问题自己的摘要描述了这次调用：{text}"
    );

    // 参数还在详情里，在它们自己的标题下。
    click_row(&mut asked, 120, 40, "调用 ask_user_question");
    let text = screen(120, 40, &mut asked).join("\n");
    assert!(text.contains("── 参数 ──"), "参数那一节：{text}");
    assert!(text.contains("下一步"), "而它带着具体的参数：{text}");
}

#[test]
fn the_call_line_wears_the_narration_grey_after_its_speakers_name() {
    // 描述是叙述，不是模型的答案，所以它穿和思考行
    // 一样的灰；名字保持讨论者的颜色（2026-09-23）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-62",
        "bash",
        serde_json::json!({"command": "ls -la"}),
    ));
    state.apply(tool_completed(2, "call-62", true, Some("out"), None));
    let frame = buffer(120, 40, &mut state);

    let Some((name_x, row)) = cell_of(&frame, 120, 40, "[kimi]") else {
        panic!("调用行在屏幕上");
    };
    let Some((call_x, _)) = cell_of(&frame, 120, 40, "调用 bash") else {
        panic!("调用行在屏幕上");
    };
    assert_eq!(
        frame[(name_x, row)].fg,
        Color::LightCyan,
        "名字保持讨论者的颜色"
    );
    assert_eq!(
        frame[(call_x, row)].fg,
        Color::DarkGray,
        "而描述穿的是叙述灰"
    );
}

#[test]
fn the_detail_overlay_wears_the_speakers_colour_and_keeps_a_cell_of_air() {
    // 边框是讨论者的颜色 —— 还没读到一个字，你就知道
    // 在读谁的那一行 —— 而字坐在边框里往里一格（2026-09-23）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-63",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-63", true, Some("out"), None));
    click_row(&mut state, 120, 40, "调用 bash");
    let frame = buffer(120, 40, &mut state);

    // 覆盖层自己的那个角：一个不是中间那块框的 `┌`。
    let mut corner = None;
    for y in 0..40 {
        for x in 2..120 {
            if frame[(x, y)].symbol() == "┌" {
                corner = Some((x, y));
                break;
            }
        }
        if corner.is_some() {
            break;
        }
    }
    let (x, y) = corner.expect("覆盖层的左上角");
    assert_eq!(frame[(x, y)].fg, Color::LightCyan, "边框穿着讨论者的颜色");
    assert_eq!(
        (
            frame[(x + 1, y + 1)].symbol(),
            frame[(x + 2, y + 1)].symbol()
        ),
        (" ", " "),
        "紧贴边框里面那几格是空气，两个轴上都是"
    );
    assert_eq!(
        frame[(x + 2, y + 2)].symbol(),
        "[",
        "而标题在两个方向上都从边框往里一格开始"
    );
}

/// 覆盖层在屏幕上画出来的框：它的左列与宽度。
fn overlay_box(frame: &Buffer, width: u16, height: u16) -> Option<(u16, u16)> {
    for y in 0..height {
        let mut left = None;
        for x in 1..width {
            match frame[(x, y)].symbol() {
                "┌" if left.is_none() => left = Some(x),
                "┐" => {
                    if let Some(left) = left {
                        return Some((left, x - left + 1));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

#[test]
fn the_permission_modals_buttons_are_centred() {
    // 正文是一段居中的文字，而按钮是更短的一行：按它们自己的宽度
    // 居中，才把它们放在这些字下面，而不是偏到左边
    // （2026-09-23，用户报告）。
    let mut state = state_with_roster(&["deepseek"]);
    state.request(ask_permission().0);
    let frame = buffer(120, 24, &mut state);
    let (x, width) = overlay_box(&frame, 120, 24).expect("模态的框");

    let (first, row) = cell_of(&frame, 120, 24, "[y] 允许").expect("第一个按钮");
    let (last, _) = cell_of(&frame, 120, 24, "[n] 拒绝").expect("最后一个按钮");
    let last_end = last as usize + text_columns("[n] 拒绝");
    let left_gap = first as usize - (x as usize + 1);
    let right_gap = (x as usize + width as usize - 1) - last_end;
    assert!(
        row == cell_of(&frame, 120, 24, "[n] 拒绝")
            .expect("最后一个按钮")
            .1,
        "按钮在同一行"
    );
    assert!(
        left_gap.abs_diff(right_gap) <= 1,
        "按钮行居中：左边 {left_gap} 列空气，右边 {right_gap} 列"
    );
}

#[test]
fn the_detail_footer_counts_the_last_row_on_screen() {
    // 已经滚到最底下的读者就在最底下：页脚数的是
    // 屏幕上最后一行，不是窗口恰好起始的那一行。最后一行可见时
    // 它却读成 `94/154`（2026-09-23，用户报告）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-70",
        "bash",
        serde_json::json!({"command": "seq 1 200"}),
    ));
    let body: Vec<String> = (1..=200).map(|n| format!("第 {n} 行")).collect();
    state.apply(tool_completed(
        2,
        "call-70",
        true,
        Some(&body.join("\n")),
        None,
    ));
    click_row(&mut state, 120, 40, "调用 bash");

    // 页脚那两个数，不管它们在屏幕上的哪儿。
    let counts = |state: &mut TuiState| -> (usize, usize) {
        let rows = screen(120, 40, state);
        let row = rows
            .iter()
            .find(|row| row.contains('↕'))
            .expect("页脚画出来了");
        let tail = row.split('↕').nth(1).expect("标记之后");
        let pair = tail.split('·').next().expect("键位之前").trim();
        let (seen, total) = pair.split_once('/').expect("一对");
        (
            seen.trim().parse().expect("一个数字"),
            total.trim().parse().expect("一个数字"),
        )
    };

    let (seen, total) = counts(&mut state);
    assert!(total > 100, "正文长到可以滚动：{seen}/{total}");
    assert!(seen < total, "而窗口起点还在离末尾一截的地方");

    for _ in 0..50 {
        state.key(Key::PageDown);
    }
    let (seen, total) = counts(&mut state);
    assert_eq!(seen, total, "到底时页脚读的是最后一行：{seen}/{total}");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("第 200 行"), "而最后一行真的在屏幕上：{text}");
}

/// 一次针对 shell 命令的权限询问，按循环问它的样子。
fn ask_bash(command: &str) -> fs_agent::render::ConsoleRequest {
    use fs_agent::permissions::PermissionRequest;
    use fs_agent::render::AskRequest;
    let (reply, _answer) = tokio::sync::oneshot::channel();
    ConsoleRequest::Ask(AskRequest {
        request: PermissionRequest {
            escalation: None,
            speaker: None,
            request_id: "r-1".to_owned(),
            tool_call_id: "c-1".to_owned(),
            tool_name: "bash".to_owned(),
            args: serde_json::json!({ "command": command }),
            reason: "mode ask".to_owned(),
        },
        reply,
    })
}

#[test]
fn the_permission_question_describes_the_call_the_way_the_line_does() {
    // 问句和它关于的那条折起行是**同一个**函数产出的，所以
    // 它们不可能漂开 —— 而这次调用将怎么跑就留在它们下面，因为
    // 批准的这一刻正是那条确切命令必须可读的时刻（2026-09-23，用户
    // 要求：弹窗要读起来像那一行，两个都留着）。
    let command = "head -5 README.md";
    let mut state = state_with_roster(&["deepseek"]);
    // 同一次调用，先折进转录……
    state.apply(tool_started(
        1,
        "call-80",
        "bash",
        serde_json::json!({ "command": command }),
    ));
    state.apply(tool_completed(2, "call-80", true, Some("out"), None));
    // ……然后被问起。
    state.request(ask_bash(command));

    let rows = screen(120, 40, &mut state);
    let text = rows.join("\n");
    assert_eq!(
        text.matches("调用 bash 查看 README.md").count(),
        2,
        "折起行和问句带着同一句描述：{text}"
    );
    let description = rows
        .iter()
        .position(|row| row.contains("调用 bash 查看 README.md") && row.contains('┆'))
        .expect("覆盖层里的描述行");
    let call = rows
        .iter()
        .position(|row| row.contains("bash（command=head -5 README.md）"))
        .expect("确切的那条调用还显示着");
    assert!(description < call, "而确切的那条调用在它后面：{text}");
}

/// 一行里每一根表格竖线的显示列号 —— 一张表画的还是不是一张表，看这个。
fn table_bars(row: &str) -> Option<Vec<usize>> {
    if !row.contains('│') {
        return None;
    }
    let mut bars = Vec::new();
    let mut column = 0;
    for ch in row.chars() {
        if ch == '│' {
            bars.push(column);
        }
        column += fs_agent::render::width::char_columns(ch);
    }
    Some(bars)
}

#[test]
fn narrowing_the_terminal_relays_a_table_out_by_the_new_width() {
    // `to_lines` 收宽度之后，源行本身也依赖宽度：表格的列宽是**渲染时**算的，所以窗口
    // 一变窄，那几行必须按新宽度重排，而不是把一份按旧宽度排好的旧行硬折
    // （`.scratch/markdown-render/spec.md` §1）。
    //
    // 硬折与重排的区别看得见：旧行更宽，折出来的续行是空行，于是分隔线不再紧跟着表头。
    let mut state = state();
    state.apply(message(
        1,
        "| name | value |\n|---|---|\n| alpha | 1 |\n| b | 22 |",
        None,
    ));

    for width in [120, 70] {
        let rows = screen(width, 24, &mut state);
        let head = rows
            .iter()
            .position(|row| row.contains("name") && row.contains('│'))
            .unwrap_or_else(|| panic!("{width} 列下没有表头：{rows:?}"));
        let table = &rows[head..head + 4];
        assert!(
            table[1].contains('┼'),
            "{width} 列：分隔线紧跟表头：{table:?}"
        );
        assert!(
            table[2].contains("alpha") && table[2].contains('│'),
            "{width} 列：第一条数据行紧跟分隔线：{table:?}"
        );
        assert!(
            table[3].contains("22"),
            "{width} 列：第二条数据行：{table:?}"
        );
        // **表头与数据行**的列都对得齐 —— 前缀只替换表格整块那一段前导，网格在屏幕上
        // 仍然是一张网格（spec §2 的「列对得齐」）。
        let bars: Vec<Vec<usize>> = table.iter().filter_map(|row| table_bars(row)).collect();
        assert_eq!(bars.len(), 3, "{width} 列：三条行带竖线：{table:?}");
        assert!(
            bars.iter().all(|row| row == &bars[0]),
            "{width} 列：表头与数据行在同一组列上：{bars:?}"
        );
    }
}

#[test]
fn a_table_at_the_head_of_an_answer_lines_up_with_its_header() {
    // 回答的第一行带 `[name] ` 前缀，而表格的表头就是那一行：前缀占的列从表格的预算里
    // 出，渲染器把整块推到那一列之后，前缀再把它换回来。于是表头与数据行仍在同一列。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(message(
        1,
        "| name | value |\n|---|---|\n| alpha | 1 |",
        None,
    ));
    let rows = screen(120, 24, &mut state);
    let head = rows
        .iter()
        .position(|row| row.contains("name") && row.contains('│'))
        .expect("表头画出来了");
    let header = table_bars(&rows[head]).expect("表头有竖线");
    let data = table_bars(&rows[head + 2]).expect("数据行有竖线");
    assert_eq!(
        header,
        data,
        "表头与数据行在同一列上：{:?}",
        &rows[head..head + 3]
    );
    // 而第一行确实由 `[kimi] ` 引领。
    assert!(
        rows[head].contains("[kimi]"),
        "第一行是前缀加表头：{:?}",
        rows[head]
    );
}

#[test]
fn a_code_block_is_highlighted_within_the_transcript() {
    // 代码块的语法高亮铺在转录里的样子：`fn` 是关键字那一档，而不是整片代码一个颜色。
    let mut state = state();
    state.apply(message(1, "```rust\nfn main() {}\n```", None));
    let rows = screen(120, 24, &mut state);
    let code = rows
        .iter()
        .position(|row| row.contains("fn main()"))
        .expect("代码行画出来了");
    assert!(
        rows[code].contains("fn main() {}"),
        "逐字保留：{:?}",
        rows[code]
    );
}
