//! 全屏外壳画进一个 `TestBackend`，于是几何不用终端也能断言
//! （`.scratch/tui-sidebar/spec.md` §1–§2，
//! `测试决定`）。
//!
//! 接缝是 [`draw_frame`]：一个状态进去，一块定尺的缓冲区出来。
//! 下面每一条断言说的都是人看得见的东西 —— 有哪些区域、左栏那一页
//! 写着什么、回合条上的格在哪、挤得下几条提示 —— 从不涉及布局
//! 在路上算出来的那些矩形。

use heng::config::{DiffViewerSettings, FileViewerSettings, ReasoningEffort};
use heng::render::palette;
use heng::render::width::text_columns;
use heng::render::{
    CatalogEntry, ConsoleRequest, FrontEndEvent, Key, RenderEvent, SessionFacts, TOKEN_COMMAND,
    TOKEN_REFERENCE, TuiState, draw_frame, wording,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::style::{Color, Modifier};

fn facts() -> SessionFacts {
    SessionFacts {
        switchable: true,
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: "~/code/fortystory/heng".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        // 会话被组装时所处的模式：状态行里模式那一栏。想测另一档的
        // 测试在自己的 facts 里覆盖它。
        mode: heng::permissions::Mode::Ask,
        budget_limit: Some(100_000),
        number_style: heng::render::wording::NumberStyle::Cn,
        file_viewer: FileViewerSettings::default(),
        diff_viewer: DiffViewerSettings::default(),
        speaker_order: Vec::new(),
    }
}

fn state() -> TuiState {
    TuiState::new(facts(), std::path::PathBuf::from("/x/heng"), None)
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

/// 转录的第一行。外框走了之后它曾经就是终端的第一行
/// （`.scratch/tui-chrome/spec.md` §1）；2026-10-06 起主列顶上多了页签条那两行（标签行与它
/// 下面那条线），转录从第三行起（`.scratch/trace-in-main/spec.md` §1）。
const TRANSCRIPT_TOP: usize = 2;

/// 一帧画出来之后转录占的那些行：第一条横线画在输入区上面，
/// 而它和转录之间还夹着状态行那一行。
///
/// 量出来的，不是记住的，因为终端高度、左栏的高度阶梯
/// 与草稿三者都会把它挪动。外框与状态行上方那条线都走了之后，
/// 那条横线不再有端点交叉符，所以探针找的是延伸到屏幕右缘的
/// 那条虚线本身（页签条的两条横线到分隔列就结束，不会以它结尾）。
fn transcript_rows(rows: &[String]) -> usize {
    // 页签条下面那条线也以 `┄` 结尾（它同样画到屏幕右缘），所以从转录的第一行起往后找
    // （`.scratch/trace-in-main/spec.md` §1）。
    rows.iter()
        .skip(TRANSCRIPT_TOP)
        .position(|row| row.ends_with('┄'))
        .expect("主列里输入区上面那条横线在屏幕上")
        // 转录与那条线之间还夹着状态行那一行。
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

/// 详情覆盖层顶上那条标签条上的那几个面名 —— 它们连成一串，就是那一行。
///
/// 它同时是「覆盖层立着、正落在**默认面**上」的判据：工具落在**参数**、消息落在**正文**
/// （票 13 第 5 条）。今天那块正文里的小节标题 `── 参数 ──` 不再画了 —— 面名由标签条说，
/// 正文不重复它（票 13 第 9 条）。
const TOOL_FACES: &str = "参数┆输出┆计时┆来源┆概述";
const MESSAGE_FACES: &str = "正文┆计时┆概述";
const THINKING_FACES: &str = "思考┆计时┆概述";

/// 切到下一面：覆盖层立着时 `Tab` 是空键，于是它归标签条（票 13 第 3 条）。
fn next_face(state: &mut TuiState) {
    state.key(Key::Tab);
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
    // 那一行空行下面开始（2026-10-01 真机反馈）；120x24 装不下块字，所以这里是收起来的
    // 那一版 —— 五行网格、拼音在第 2 行、汉字在第 3 行。
    assert!(
        rows[2].contains("héng") && rows[3].contains('衡'),
        "标记的拼音与汉字落在左栏的那两行上：{:?} / {:?}",
        rows[2],
        rows[3]
    );

    // 左栏的页签条：上下两条横线夹着三个标签（`轨迹` 不在这一列里了），两条横线从屏幕左缘
    // 一直画到分隔列（`.scratch/trace-in-main/spec.md` §2）。
    assert_eq!(
        rows[6].trim_matches(|ch| ch == '┄' || ch == '┆' || ch == ' '),
        "",
        "页签条的上横线横跨整条左栏：{:?}",
        rows[6]
    );
    assert!(
        rows[7].contains("调用量") && rows[7].contains("文件") && !rows[7].contains("轨迹"),
        "三个页签占自己那一行：{:?}",
        rows[7]
    );

    // 主列的页签条：标签行就是屏幕第一行，它下面一条线，转录从第三行起
    // （`.scratch/trace-in-main/spec.md` §1）。
    assert!(
        rows[0].contains("对话") && rows[0].contains("轨迹"),
        "两个视图占主列的第一行：{:?}",
        rows[0]
    );
    assert!(rows[1].ends_with('┄'), "标签下面一条线：{:?}", rows[1]);

    // 分隔线跑满屏幕的整个高度。24 行终端留给转录十五行：
    // 三行归输入区的地板、六行归外壳 —— 页签条那两行也在这里
    // （`.scratch/trace-in-main/spec.md` §1）。
    assert_eq!(transcript_rows(&rows), 15, "120x24 给转录 15 行");
    assert!(
        !rows[16].contains('┄'),
        "状态行上方那条横线已经不画了：{:?}",
        rows[16]
    );
    assert!(
        rows[17].contains("模型 claude-sonnet-4-5")
            && rows[17].contains("┆ 询问 ┆")
            && rows[17].contains("上下文 —")
            && rows[17].contains("🌕 就绪")
            && !rows[17].contains('…'),
        "状态行报出模型、模式、占比与带字形循环的状态词：{:?}",
        rows[17]
    );
    assert!(
        rows[18].ends_with('┄'),
        "输入区上面那条横线横跨主列：{:?}",
        rows[18]
    );
    assert!(
        rows[19].trim_matches(['┆', ' ']).is_empty(),
        "输入区的第一行是一份空草稿：{:?}",
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
    // 提示行只跨**主列**（`.scratch/tui-feedback/spec.md` §2），所以竖虚线跟左栏同高、一直
    // 画到屏幕最后一行。
    assert_eq!(frame[(40, 22)].symbol(), "┆", "画到屏幕倒数第二行");
    assert_eq!(
        frame[(40, 23)].symbol(),
        "┆",
        "最后一行（提示行那一行）也有它：左栏恢复全高"
    );
    // 标记从顶上留的那一行空行**下面**开始：第 1 行；120x24 里是收起来的那一版，
    // 五行网格里汉字在第 3 行，从往里十七列处开始（列 1 + 17 = 18）。
    assert_eq!(frame[(0, 1)].symbol(), " ", "左边一列空气");
    assert_eq!(frame[(18, 3)].symbol(), "衡", "然后是标记");
    // 标记宽 38 列：2 + 38 = 40，所以最后一列空气在 39，
    // 分隔线在 40。
    assert_eq!(frame[(39, 1)].symbol(), " ", "右边也有一列");
}

#[test]
fn the_mark_keeps_a_dim_preamble_in_both_its_forms() {
    // 静止的标记 —— 画家负责的那一半：字形是 `wording` 的，
    // 它们落在哪几行、哪一行暗却是画家的，所以在
    // 看得见它的地方逐格断言，就在缓冲里。动的那一半是
    // `the_mark_does_not_move_while_a_run_is_in_flight`
    // （`.scratch/tui-input-pulse/spec.md` §2）。
    //
    // 120x24 装不下块字，所以这里先是收起来的那一版：拼音加一个汉字。
    let frame = buffer(120, 24, &mut state());
    assert_eq!(
        frame[(17, 2)].symbol(),
        "h",
        "拼音起于收起来那版标记的第二行（往里十六列处）"
    );
    assert_eq!(
        frame[(18, 3)].symbol(),
        "衡",
        "汉字在它下一行，与拼音中心对齐"
    );
    assert_eq!(frame[(1, 2)].fg, Color::Magenta, "标着拼音的那一行退一档");
    assert_eq!(frame[(1, 3)].fg, Color::LightMagenta, "汉字那一行是亮的");

    // 高到 32 行往上（`BLOCK_LOGO_MIN_PAGE_ROWS`）就换成篆书：二十三行全在屏幕上，且与
    // `wording::logo_lines` 逐格相同；拼音在第一行、字形从第二行起。
    let mut tall = state();
    let block = mark_rows_at(120, 32, 23, &mut tall);
    assert_eq!(
        block,
        wording::logo_lines().map(str::to_owned).to_vec(),
        "120x32 画的就是 `logo_lines` 那二十三行"
    );
    let frame = buffer(120, 32, &mut state());
    assert_eq!(frame[(1, 1)].fg, Color::Magenta, "块字的拼音那一行也退一档");
    assert_eq!(frame[(1, 2)].fg, Color::LightMagenta, "字形第一行是亮的");

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
            !any_mark_on_screen(&rows.join("\n")),
            "{width}x{height} 在标记的那一档之下：{:#?}",
            rows[1]
        );
        assert_eq!(
            rows.join("\n").contains("heng"),
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
            assert!(!text.contains("调用量"), "{width} 列时没有左栏：{text}");
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
fn ctrl_o_takes_the_sidebar_away_and_brings_it_back() {
    // `Ctrl-O` 是左栏的开关：收起时那一栏连同分隔线整列还给主列，
    // 再按一下逐格回到原样（`.scratch/sidebar-toggle/spec.md` §2、§3）。
    let mut state = state();
    let open = screen(120, 24, &mut state);
    assert!(
        open.join("\n").contains('┆'),
        "收起之前左栏与分隔线在：{}",
        open.join("\n")
    );

    state.key(Key::CtrlO);
    let closed = screen(120, 24, &mut state);
    assert!(
        !closed.join("\n").contains("调用量"),
        "收起之后整帧没有分隔列：{}",
        closed.join("\n")
    );
    assert_ne!(open, closed, "收起改变了这一帧");

    state.key(Key::CtrlO);
    let reopened = screen(120, 24, &mut state);
    assert_eq!(open, reopened, "再按一下逐格回到原样");
}

#[test]
fn ctrl_o_cannot_bring_the_sidebar_back_below_eighty_columns() {
    // 意愿与宽度相乘（`.scratch/sidebar-toggle/spec.md` §2）：窄于 80 列时左栏本来就放不下，
    // 按开关不该改变任何东西 —— 不报错、不弹文案、帧逐格相等。
    for (width, height) in [(79u16, 24u16), (60, 24), (40, 10)] {
        let mut state = state();
        let before = screen(width, height, &mut state);
        state.key(Key::CtrlO);
        let after = screen(width, height, &mut state);
        assert_eq!(
            before, after,
            "{width}x{height} 下按 Ctrl-O 不该改变任何东西"
        );
    }

    // 80 列是窄档的门槛：这里它收得起来，也叫得回来。
    let mut state = state();
    let open = screen(80, 24, &mut state);
    state.key(Key::CtrlO);
    let closed = screen(80, 24, &mut state);
    assert_ne!(open, closed, "80 列下 Ctrl-O 收得起左栏");
    state.key(Key::CtrlO);
    assert_eq!(open, screen(80, 24, &mut state), "80 列下也叫得回来");
}

#[test]
fn ctrl_o_works_while_a_run_is_in_flight() {
    // 纯视图手势：跑着的时候恰恰最想多看几行转录（`.scratch/sidebar-toggle/spec.md` §3）。
    // 与 `Ctrl-D` 相反 —— 那一个是空闲键盘的手势。
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    let open = screen(120, 24, &mut state);
    assert!(open.join("\n").contains('┆'), "跑着的时候左栏还在");

    state.key(Key::CtrlO);
    let closed = screen(120, 24, &mut state);
    assert_ne!(open, closed, "跑着的时候 Ctrl-O 也算数");
    assert!(
        !closed.join("\n").contains("调用量"),
        "跑着的时候也收得起来：{}",
        closed.join("\n")
    );
}

#[test]
fn ctrl_o_is_ignored_while_the_detail_overlay_is_up() {
    // 详情覆盖层是自成一体的「查看态」、独占键盘，所以它拦得住这个键
    // （`.scratch/sidebar-toggle/spec.md` §3）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-24",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-24", true, Some("body"), None));
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    let open = screen(120, 40, &mut state);
    assert!(
        open.join("\n").contains(TOOL_FACES),
        "覆盖层立着：{}",
        open.join("\n")
    );

    state.key(Key::CtrlO);
    let after = screen(120, 40, &mut state);
    assert_eq!(open, after, "覆盖层立着时 Ctrl-O 一个像素都不动");
    assert!(after.join("\n").contains(TOOL_FACES), "覆盖层也没被关掉");
}

#[test]
fn ctrl_o_still_works_while_a_questionnaire_is_up() {
    // 问卷不吞键盘独占权，而左栏不在它的管辖范围内 —— 它是纯视图手势
    // （`.scratch/sidebar-toggle/spec.md` §3 的分派表）。
    use heng::questions::{Choice, UserQuestion};
    let (mut state, _answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![Choice {
            label: "唯一".to_owned(),
            description: None,
        }],
    });
    let open = screen(120, 24, &mut state);
    state.key(Key::CtrlO);
    let closed = screen(120, 24, &mut state);
    assert_ne!(open, closed, "问卷立着时 Ctrl-O 也生效");
    assert!(
        !closed.join("\n").contains("调用量"),
        "左栏确实收起来了：{}",
        closed.join("\n")
    );
}

#[test]
fn the_hidden_sidebar_has_no_tabs_to_click() {
    // 命中矩形一律「这一帧真的画了什么就记什么」，所以页签随左栏一起
    // 消失 —— 没有第二处要同步的状态（`.scratch/sidebar-toggle/spec.md` §2）。
    let mut state = state();
    assert!(
        screen(120, 24, &mut state).join("\n").contains("调用量"),
        "左栏带着页签"
    );

    state.key(Key::CtrlO);
    let closed = screen(120, 24, &mut state);
    let text = closed.join("\n");
    assert!(!text.contains("调用量"), "收起后页签不在：{text}");

    // 原先页签所在的那一格现在是转录区：点它什么都不发生。
    click(&mut state, 2, 6);
    let after = screen(120, 24, &mut state);
    assert_eq!(closed, after, "收起后点页签原来的位置没有反应");
}

#[test]
fn a_todo_list_landing_while_the_sidebar_is_hidden_does_not_pop_it_back() {
    // 用户刚说了「我现在不要这栏」，模型的一次工具调用不该覆盖它：标签在后台
    // 就位，按回来才看得见（`.scratch/sidebar-toggle/spec.md` §5）。
    let mut state = state_with_roster(&["kimi"]);
    state.key(Key::CtrlO);
    assert!(
        !tab_bar(&mut state, 120, 24).contains(wording::TAB_TODO),
        "收起时看不见这个页签"
    );

    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("写测试", "pending")]),
    );
    assert!(
        !tab_bar(&mut state, 120, 24).contains(wording::TAB_TODO),
        "列表落地也不该把它弹回来"
    );

    state.key(Key::CtrlO);
    assert!(
        tab_bar(&mut state, 120, 24).contains(wording::TAB_TODO),
        "叫回来就看得见"
    );

    // 那一页的内容也在：意愿只决定栏在不在，不决定标签或页里的东西。
    click_row(&mut state, 120, 24, wording::TAB_TODO);
    let shown = screen(120, 24, &mut state).join("\n");
    assert!(shown.contains("写测试"), "todo 页的内容还在：{shown}");
}

#[test]
fn hiding_the_sidebar_widens_what_the_editor_wraps_against() {
    // 意愿必须走进「喂给编辑器多少列」，否则收起左栏后转录变宽了、
    // 输入区还按旧宽度折行（`.scratch/sidebar-toggle/spec.md` §2）。
    let area = ratatui::layout::Rect::new(0, 0, 120, 24);
    let shown = heng::render::layout::content_width(area, true);
    let hidden = heng::render::layout::content_width(area, false);
    assert!(hidden > shown, "收起后主列更宽：{shown} → {hidden}");
    assert_eq!(hidden, 120, "没有左栏时主列就是整屏");

    // 转录正文也拿到那份宽度：`w − 2`（右缘恒留滚动条与回合条各一列），
    // 而显示左栏时它是 `79 − 2`。
    use heng::render::layout;
    assert_eq!(
        layout::plan(area, 3, false).transcript_text().width,
        118,
        "收起后转录正文宽 = w − 2"
    );
    assert_eq!(
        layout::plan(area, 3, true).transcript_text().width,
        77,
        "显示时转录正文宽 = 主列 79 − 2"
    );
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
    // 主子顶上的页签条花掉两行之后，40x10 的地板紧了一档：输入区仍拿满三行，转录只剩一
    // 行（`.scratch/trace-in-main/spec.md` §1）。输入区是第 5 到 7 行，
    // 它上面那条横线在第 4 行，提示行在第 9 行。
    assert_eq!(transcript_rows(&rows), 1, "一行转录：{rows:#?}");
    assert!(
        rows[5].trim().is_empty(),
        "输入区的第一行是一份空草稿：{:?}",
        rows[5]
    );
    assert!(
        rows[6].trim_matches(' ').is_empty(),
        "它下面还有一行空白：{:?}",
        rows[6]
    );
    assert!(
        rows[3].contains("询问 ┆") && rows[3].contains("上下文 —"),
        "在地板上状态行先让出模型：{:?}",
        rows[3]
    );
    assert!(
        !rows[3].contains("模型"),
        "模型是第一个让位的字段：{:?}",
        rows[3]
    );
    assert!(
        !rows[3].contains("claude-sonnet"),
        "这是它丢掉的第一个东西：{:?}",
        rows[3]
    );
    assert!(rows[9].contains("ctrl-c"), "提示行：{:?}", rows[9]);
    assert!(
        !rows.join("\n").contains("heng"),
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
    // 提示行落在**主列**里（`.scratch/tui-feedback/spec.md` §2），所以它的宽度是主列内容宽、
    // 不再是终端宽度：120 列终端给出 79 列提示，80 列给出 51 列。量出来的阶梯因此是
    // 40 列 -> 一条提示 + 出口，80 -> 三条，100 -> 四条，120 -> 五条。
    //
    // `ctrl-o 左栏` 排在最末、`ctrl-t 换模型` 排在它前面一格，所以这两条只有最宽那一档看得见
    // （`.scratch/sidebar-toggle/spec.md` §4 的「位置就是优先级」）。
    assert_eq!(hint_items(40).len(), 2, "40 列：{:?}", hint_items(40));
    assert_eq!(hint_items(80).len(), 3, "80 列：{:?}", hint_items(80));
    assert_eq!(hint_items(100).len(), 4, "100 列：{:?}", hint_items(100));
    assert_eq!(hint_items(120).len(), 5, "120 列：{:?}", hint_items(120));
    assert_eq!(hint_items(174).len(), 8, "174 列：{:?}", hint_items(174));

    // 在地板上，出口之前只挤得下一条提示 —— 而 `就绪` 不在这里，它住在状态行里。左栏在地板
    // 上本来就不画，所以提示行拿到的就是整屏 40 列。
    let floor = hint_items(40);
    assert_eq!(
        floor,
        vec!["enter 发送", "ctrl-c/ctrl-d 退出"],
        "40 列：{floor:?}"
    );

    // 80 列终端的主列是 51 列（窄左栏 28 + 分隔列），出口之前放得下两条提示。
    let narrow = hint_items(80);
    assert_eq!(
        narrow,
        vec!["enter 发送", "ctrl-j 换行", "ctrl-c/ctrl-d 退出"],
        "80 列：{narrow:?}"
    );

    // 120 列是参考尺寸：主列 79 列，出口之前放得下四条提示。
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
        "四条提示加出口"
    );
    // 忙碌那一档的出口短六列，但状态词不在这一行上，不论忙闲 —— 它住在状态行里。
    let busy = hint_items_busy(120);
    assert_eq!(
        busy,
        vec![
            "enter 发送",
            "ctrl-j 换行",
            "esc 取消",
            "shift+tab 模式",
            "ctrl-c 退出",
        ],
        "忙碌只换出口那一段：{busy:?}"
    );

    // 174 列能把每条提示都放下。
    let roomy = hint_items(174);
    assert_eq!(roomy.first().unwrap(), "enter 发送", "{roomy:?}");
    assert!(
        roomy.contains(&"PgUp/PgDn 滚动".to_owned()),
        "后面还跟着整份提示表：{roomy:?}"
    );
    assert!(
        roomy.contains(&"ctrl-t 换模型".to_owned()),
        "换模型的快捷键在表里：{roomy:?}"
    );
    assert!(
        roomy.contains(&"ctrl-o 左栏".to_owned()),
        "左栏开关是这一档多出来的那一条：{roomy:?}"
    );
    // 120 列下它已经被让掉 —— 窄终端先丢最后两条，而这一行还有更常用的键要放。
    assert!(
        !hint_items(120).contains(&"ctrl-t 换模型".to_owned()),
        "120 列还看不到它：{:?}",
        hint_items(120)
    );
    assert!(
        !screen(174, 24, &mut state())
            .join("\n")
            .contains("shift+enter"),
        "任何宽度下都没有幽灵换行键"
    );
}

#[test]
fn the_hint_row_sits_under_the_input_and_inside_the_main_column() {
    // 提示行与输入区同列同宽、就在它正下方（`.scratch/tui-feedback/spec.md` §2，推翻
    // `tui-visual-language` §16）。120 列下的落点是第 41 列（分隔列右边一格）到主列右缘 ——
    // 左栏那一行的最后一格仍然是左栏自己的东西。
    let mut state = idle();
    let frame = buffer(120, 24, &mut state);
    let hints = (0..24)
        .find(|y| row_text(&frame, *y, 120).contains("enter 发送"))
        .expect("提示行在屏幕上");
    assert_eq!(hints, 23, "提示行是屏幕最后一行");
    assert_eq!(frame[(40, hints)].symbol(), "┆", "分隔列照旧到底");
    let left = cells(&frame, hints, 0, 41);
    assert!(
        !left.contains("enter"),
        "提示不再从屏幕最左列起（那是左栏底下）：{left:?}"
    );
    assert_eq!(frame[(41, hints)].symbol(), "e", "它从主列起点起");

    // 几何层：它就是输入区那一块的正下方，同列同宽。
    use heng::render::layout::plan;
    use ratatui::layout::Rect;
    let regions = plan(Rect::new(0, 0, 120, 24), 3, true);
    assert_eq!(regions.hints.x, regions.input.x, "同列");
    assert_eq!(regions.hints.width, regions.input.width, "同宽");
    assert_eq!(regions.hints.y, regions.input.bottom() + 1, "就在它正下方");
}

#[test]
fn the_transcript_pane_shows_both_the_notices_and_the_streaming_tail() {
    use heng::events::SpeakerId;
    use heng::render::{DeltaKind, RenderEvent};

    let mut state = state();
    state.apply(RenderEvent::notice(
        "heng：会话 abc · 模型 m · 模式 询问 · /tmp/x".to_owned(),
    ));
    state.apply(RenderEvent::Delta {
        speaker: SpeakerId::Debater("kimi".into()),
        kind: DeltaKind::Text,
        text: "正在读文件".to_owned(),
    });

    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("heng：会话 abc"),
        "提示是转录里的一行：{text}"
    );
    assert!(text.contains("正在读文件"), "流式的尾巴也在窗格里：{text}");
}

/// 屏幕上那版**块字**标记在不在：找它最上面那行字形。
///
/// 不能拿汉字当判据 —— 块字是拼出来的，屏幕上一个大标记里一个「衡」字都没有。
fn block_mark_on_screen(text: &str) -> bool {
    text.contains(wording::logo_lines()[1].trim())
}

/// 屏幕上那版**收起来**的标记在不在：它仍是拼音加一个汉字。
fn compact_mark_on_screen(text: &str) -> bool {
    text.contains(wording::logo_lines_compact()[2].trim())
}

/// 两者的并集：屏幕上画了标记，无论哪一版。
fn any_mark_on_screen(text: &str) -> bool {
    block_mark_on_screen(text) || compact_mark_on_screen(text)
}

/// 标记在 120x24 下的五行颜色，自上而下：画家写的那一列
/// 是左栏的第二列，标记的第一个字形在它每一行上都坐在
/// 那里。
///
/// 120x24 装不下篆书（二十三行），所以这里量的总是收起来的那一版。
fn mark_colours(state: &mut TuiState) -> Vec<Color> {
    let frame = buffer(120, 24, state);
    (1..=5u16).map(|y| frame[(1, y)].fg).collect()
}

/// 人眼里看到的整块标记：从一帧上读出的那几行、它预留的整幅宽度。
///
/// 标记从左栏第二列起（`mark_colours` 也是这么找它的），宽 `LOGO_WIDTH` 列，`rows` 说读
/// 几行（篆书二十三行，收起来那版五行）。这里读的是**格**、不是字符 —— `cells` 按显示宽度走。
fn mark_rows_at(width: u16, height: u16, rows: u16, state: &mut TuiState) -> Vec<String> {
    let frame = buffer(width, height, state);
    let width = heng::render::layout::LOGO_WIDTH;
    (1..=rows).map(|y| cells(&frame, y, 1, 1 + width)).collect()
}

/// 120x24 里那版收起来的标记。
fn mark_rows(state: &mut TuiState) -> Vec<String> {
    mark_rows_at(120, 24, 5, state)
}

#[test]
fn the_mark_holds_still_while_nothing_runs() {
    // 歇着的标记一个像素都不动：字形与它的基线色一起，逐格不变。它曾经有一条下落的短横
    // （票 08 关掉），2026-10-07 起多了一束**只在运行中**扫过的反光 —— 所以"不动"这条
    // 现在有前提：没有运行在进行中（`.scratch/mark-sweep/spec.md` §2）。
    let mut state = state();
    let still = mark_rows(&mut state);
    assert_eq!(
        still,
        heng::render::wording::logo_lines_compact()
            .map(str::to_owned)
            .to_vec(),
        "屏幕上那五行就是 `logo_lines_compact` 里的那五行"
    );
    let colours = vec![
        Color::LightMagenta,
        Color::Magenta,
        Color::LightMagenta,
        Color::LightMagenta,
        Color::LightMagenta,
    ];
    assert_eq!(mark_colours(&mut state), colours, "基线色：字亮、注暗");

    for frame in 0..12 {
        state.tick();
        assert_eq!(
            mark_rows(&mut state),
            still,
            "空闲第 {frame} 帧：标记没有动"
        );
        assert_eq!(
            mark_colours(&mut state),
            colours,
            "空闲第 {frame} 帧：颜色也没变"
        );
    }
}

/// 一束光扫过整块标记要用几帧，以及它的投影怎么算 —— 测试与画家共用同一份算术
/// （[`heng::render::tui::mark_sweep`]），免得两边各写一遍方向。
fn sweep_progress(column: usize, row: usize, columns: usize, rows: usize) -> i64 {
    (columns - 1 - column) as i64 + (rows - 1 - row) as i64
}

/// 某一帧里被这束光照到白的那几格，换算成"右下 0、左上最大"的投影值。
fn lit_at(frame: u64, columns: usize, rows: usize) -> Vec<i64> {
    (0..rows)
        .flat_map(|row| (0..columns).map(move |column| (column, row)))
        .filter(|(column, row)| {
            heng::render::tui::mark_sweep(*column, *row, columns, rows, frame) == Some(Color::White)
        })
        .map(|(column, row)| sweep_progress(column, row, columns, rows))
        .collect()
}

#[test]
fn the_light_sweeps_from_the_bottom_right_to_the_top_left() {
    // `.scratch/mark-sweep/spec.md` §2：光从右下进场、往左上走。这条量的是**方向**，
    // 用一个纯函数，逐帧看它照亮的那几格的投影是不是一直在往 s 大的方向推。
    let (columns, rows) = (38usize, 23usize);
    let period = heng::render::tui::SWEEP_PERIOD;
    let span = sweep_progress(0, 0, columns, rows);

    let frames: Vec<(u64, Vec<i64>)> = (0..period).map(|f| (f, lit_at(f, columns, rows))).collect();
    let lit: Vec<i64> = frames
        .iter()
        .flat_map(|(_, values)| values.iter().copied())
        .collect();
    assert!(!lit.is_empty(), "整轮里总有一帧被照到");
    assert!(
        lit.iter().any(|s| *s <= 4),
        "光要扫到右下角那一片（投影接近 0）：{lit:?}"
    );
    assert!(
        lit.iter().any(|s| *s >= span - 4),
        "也要扫到左上角那一片（投影接近最大 {span}）：{lit:?}"
    );

    // 每一帧的**前沿**（这一帧照亮的最大投影）随着帧号单调不减，且中间确实在推进 ——
    // 这就是"从右下往左上走"。
    let mut front = i64::MIN;
    let mut advanced = 0;
    for (frame, values) in &frames {
        if values.is_empty() {
            continue;
        }
        let now = *values.iter().max().expect("非空");
        assert!(
            now >= front,
            "第 {frame} 帧的前沿 {now} 退回到了 {front} 后面：{values:?}"
        );
        if now > front {
            advanced += 1;
        }
        front = now;
    }
    assert!(
        advanced >= period as i32 / 2,
        "整轮里前沿一直在走：{advanced}"
    );

    // 一轮里没有一帧是变宽的乱扫：白格永远聚在一段连续的投影上（核心半宽以内）。
    for (frame, values) in &frames {
        if let (Some(lo), Some(hi)) = (values.iter().min(), values.iter().max()) {
            assert!(
                hi - lo <= 4,
                "第 {frame} 帧白格跨度 {lo}..{hi} 太宽，不像一条光带"
            );
        }
    }
}

#[test]
fn the_sweep_shows_up_on_the_mark_only_while_a_run_is_in_flight() {
    // 屏幕上的那一份：空闲时标记里没有一格是白的；一次运行中，某一帧必然有。
    use heng::render::layout::LOGO_WIDTH;
    let white_cells = |state: &mut TuiState| -> Vec<(u16, u16)> {
        let frame = buffer(120, 32, state);
        (1..=23u16)
            .flat_map(|y| (1..1 + LOGO_WIDTH).map(move |x| (x, y)))
            .filter(|(x, y)| frame[(*x, *y)].fg == Color::White)
            .collect()
    };

    let mut idle = state();
    for frame in 0..heng::render::tui::SWEEP_PERIOD {
        idle.tick();
        assert!(
            white_cells(&mut idle).is_empty(),
            "空闲第 {frame} 帧：标记里不该有反光"
        );
    }

    let mut running = state();
    running.request(ConsoleRequest::RunState { running: true });
    let mut seen = Vec::new();
    for frame in 0..heng::render::tui::SWEEP_PERIOD {
        running.tick();
        let cells = white_cells(&mut running);
        if !cells.is_empty() {
            seen.push((frame, cells));
        }
    }
    assert!(!seen.is_empty(), "运行一轮里那束光总该扫过标记");
    let (frame, first) = &seen[0];
    // 屏幕坐标换成标记自己的格：左栏在 x=1 处居中（120 列宽档），身份从 y=1 起。
    let progress: Vec<i64> = first
        .iter()
        .map(|(x, y)| sweep_progress((x - 1) as usize, (y - 1) as usize, 38, 23))
        .collect();
    assert!(
        progress.iter().all(|s| *s < 12),
        "光从右下进场，第 {frame} 帧不该已经跑到左上：{first:?}"
    );
}

/// 草稿第一格：屏幕上那一行里写着 `hello` 的第一个字，以及它在哪一行。
///
/// 输入框不再有提示符（2026-10-08，`.scratch/ui-trim/spec.md`），所以「草稿从哪一格起」就是
/// 输入行的第一格 —— 不再有那个要跳过去的前导。
fn draft_cell(state: &mut TuiState) -> (u16, u16) {
    let frame = buffer(120, 24, state);
    for y in 0..24 {
        if row_text(&frame, y, 120).contains("hello") {
            let x = (0..120)
                .find(|x| frame[(*x, y)].symbol() == "h")
                .expect("草稿的第一个字在屏幕上");
            return (x, y);
        }
    }
    panic!("草稿在屏幕上");
}

#[test]
fn the_draft_starts_at_the_first_column_of_the_input_row() {
    // 输入框没有前导了：第一行第一个字就是草稿的第一个字，而它穿的是终端
    // 自己的前景色（草稿归正文档，`.scratch/tui-visual-language/spec.md` §19）。
    let mut state = state();
    for ch in "hello".chars() {
        state.key(Key::Char(ch));
    }
    state.tick();
    let frame = buffer(120, 24, &mut state);
    let (x, y) = draft_cell(&mut state);
    let draft = &frame[(x, y)];
    assert_eq!(draft.symbol(), "h");
    assert_eq!(
        format!("{:?}", draft.fg),
        format!("{:?}", Color::Reset),
        "草稿是终端自己的前景色，没被动过：{draft:?}"
    );
    assert!(
        !draft.modifier.contains(Modifier::BOLD),
        "草稿不再整段加粗：{draft:?}"
    );
}

#[test]
fn the_mark_stays_still_on_the_narrow_rung_too() {
    // 文字身份与标记都不动：两者的下落短横在票 08 关停，
    // 代码随本 effort 的死代码收口删掉（`.scratch/tui-visual-language/spec.md` §34）。
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    for frame in 0..5 {
        state.tick();
        let text = screen(100, 24, &mut state).join("\n");
        assert!(
            text.contains(&format!("heng {}", env!("CARGO_PKG_VERSION"))),
            "第 {frame} 帧：身份还是它自己：{text}"
        );
    }
}

#[test]
fn a_pulse_frame_touches_the_prompt_and_the_status_glyph_and_nothing_else() {
    // 钟在动的东西：状态行那个字形循环（`.scratch/tui-visual-language/spec.md` §30–§32）
    // —— 别的什么都没有（没有回合条、没有别的状态行文字、也没有输入区：提示符的色相随
    // 提示符一起退场，`.scratch/ui-trim/spec.md`）。80 列以下整条左栏都没有，差别仍然恰好
    // 是那一格。
    //
    // **左栏那一束扫光不在这条的范围里**：一次运行中它会改标记那几格的颜色，由
    // `the_sweep_shows_up_on_the_mark_only_while_a_run_is_in_flight` 单独管。这里按排版
    // 给出的左栏矩形把它筛掉，免得两条测试为同一件事各写一份期望格。
    use heng::render::wording;
    for (width, height) in [(120u16, 24u16), (60, 24), (40, 10)] {
        let mut state = state();
        state.request(ConsoleRequest::RunState { running: true });
        // 让正文尾巴非空：否则对话视图末尾那条「正在思考…」也在走它的点号（2026-10-05 的
        // 另一条时钟动画），会把这条断言的靶子弄糊。
        state.apply(RenderEvent::Delta {
            speaker: heng::events::SpeakerId::Debater("kimi".into()),
            kind: heng::render::DeltaKind::Text,
            text: "正在读文件".to_owned(),
        });
        let before = buffer(width, height, &mut state);
        // 八帧之后运行中那个字形换一格（§31）。
        for _ in 0..8 {
            state.tick();
        }
        let after = buffer(width, height, &mut state);
        let sidebar_right = {
            use heng::render::layout::plan;
            use ratatui::layout::Rect;
            plan(Rect::new(0, 0, width, height), 1, true)
                .sidebar
                .map_or(0, |sidebar| sidebar.right())
        };
        let changed: Vec<(u16, u16)> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .filter(|(x, y)| *x >= sidebar_right && before[(*x, *y)] != after[(*x, *y)])
            .collect();
        // 状态行上那个字形格：新一帧穿的还是循环里的字形，而不是空白。
        let glyph = (0..height)
            .find_map(|y| {
                (0..width)
                    .find(|x| wording::PULSE_GLYPHS.contains(&after[(*x, y)].symbol()))
                    .map(|x| (x, y))
            })
            .expect("状态行那个字形在屏幕上");
        assert_eq!(
            changed,
            vec![glyph],
            "{width}x{height}：只有状态行那个字形在变"
        );
    }
}

/// 就绪时它**定住**：月相只描述「还在跑」。2026-10-08 维护者收掉了
/// `tui-visual-language` §31 那条「空闲也在动，只是更慢」（`.scratch/ui-trim/spec.md`）。
#[test]
fn the_status_glyph_holds_still_while_idle() {
    use heng::render::wording;
    let glyph = |state: &mut TuiState| -> String {
        let frame = buffer(120, 24, state);
        let y = (0..24)
            .find(|y| row_text(&frame, *y, 120).contains("就绪"))
            .expect("状态行在屏幕上");
        let x = (0..120)
            .find(|x| wording::PULSE_GLYPHS.contains(&frame[(*x, y)].symbol()))
            .expect("字形在状态行上");
        frame[(x, y)].symbol().to_owned()
    };

    let mut state = state();
    assert_eq!(glyph(&mut state), "🌕", "就绪时停在满月");
    for _ in 0..64 {
        state.tick();
    }
    assert_eq!(glyph(&mut state), "🌕", "整轮月相的时间过去，它一格都没换");
}

/// 同一个人连着说的几段正文：屏幕上**只留一个名字**，段与段之间空一行
/// （2026-10-06 维护者的优化，`.scratch/tui-visual-language/spec.md` §23）。
#[test]
fn a_speaker_who_says_several_things_in_a_row_is_named_once() {
    let mut state = state();
    state.apply(message(1, "方案定了，我记一下计划。", None));
    state.apply(message(2, "票的格式清楚了。", None));
    state.apply(message(3, "开始写 spec 前先确认两条文档护栏。", None));

    let rows = screen(120, 24, &mut state);
    // 只看主列：左栏那几行里也有字，混进来会把「空行」判错。
    let main =
        |row: &String| -> String { row.chars().skip(41).collect::<String>().trim().to_owned() };
    let spoken: Vec<String> = rows.iter().map(main).collect();
    let text = spoken.join("\n");
    assert_eq!(text.matches("[kimi]").count(), 1, "名字只出现一次：{text}");

    let row_of = |needle: &str| {
        spoken
            .iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle} 在屏幕上：{text}"))
    };
    let first = row_of("计划");
    assert!(
        spoken[first - 1].contains("[kimi]"),
        "名字就在第一段上面一行：{:?}",
        &spoken[first - 1]
    );
    let second = row_of("格式");
    assert_eq!(
        spoken[first + 1..second],
        [""],
        "两段之间正好一条空行：{:?}",
        &spoken[first..second]
    );
    assert!(row_of("护栏") > second, "第三段在第二段下面：{text}");
}

/// **对话页里的消息不是入口**：名字那一行与它下面的正文点了都不弹窗 —— 这一页画的就是全文，
/// 覆盖层不省任何东西（`.scratch/tui-feedback/spec.md` §9）。轨迹页照旧：那里的消息只画
/// 首行 + `…`，点开才看得到全文。
#[test]
fn a_message_in_the_conversation_is_not_an_entry() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(message(1, "正文在这里", None));
    state.apply(user_message(2, "我问一句"));

    for needle in ["[kimi]", "正文在这里", "[用户]", "我问一句"] {
        let frame = buffer(120, 24, &mut state);
        let (column, row) = cell_of(&frame, 120, 24, needle).expect("在屏幕上");
        click(&mut state, column, row);
        let text = screen(120, 24, &mut state).join("\n");
        assert!(!text.contains(MESSAGE_FACES), "点 {needle} 不弹窗：{text}");
    }

    // 轨迹页那一侧一个字没改：点它的消息行照旧开全文。
    let mut trace = state_with_roster(&["kimi"]);
    trace.apply(message(1, "第一行在这里\n第二行在那里", None));
    open_trace_tab(&mut trace, 120, 24);
    let frame = buffer(120, 24, &mut trace);
    let (column, row) = cell_of(&frame, 120, 24, "第一行在这里").expect("轨迹页上那行消息");
    click(&mut trace, column, row);
    let text = screen(120, 24, &mut trace).join("\n");
    assert!(text.contains("第二行在那里"), "轨迹页照旧开全文：{text}");
}

/// 拖选：按住、拖过两格，那一带反白；抬起时**不**走原来那次点击的路子
/// （`.scratch/tui-feedback/spec.md` §5）。台面上的点击从此发生在抬起上，这一条就是那次挪动的
/// 全部理由。
#[test]
fn dragging_across_the_transcript_highlights_it_and_swallows_the_click() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(message(1, "第一行正文\n第二行正文", None));
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "第一行正文").expect("正文行在屏幕上");

    state.mouse(press(column, row));
    state.mouse(drag_to(column + 4, row + 1));
    let frame = buffer(120, 24, &mut state);
    assert!(
        frame[(column, row)].modifier.contains(Modifier::REVERSED),
        "起点反白"
    );
    assert!(
        frame[(column + 4, row + 1)]
            .modifier
            .contains(Modifier::REVERSED),
        "终点也反白"
    );
    assert!(
        !frame[(column + 6, row)]
            .modifier
            .contains(Modifier::REVERSED),
        "选区之外不动"
    );

    // 行尾的填充空白不反白：看起来是**文字**被选中，而不是屏幕上一块矩形
    // （`.scratch/tui-feedback/spec.md` §9）。
    let mut wide = state_with_roster(&["kimi"]);
    wide.apply(message(1, "短句", None));
    let frame = buffer(120, 24, &mut wide);
    let (column, row) = cell_of(&frame, 120, 24, "短句").expect("正文行在屏幕上");
    wide.mouse(press(column, row));
    wide.mouse(drag_to(100, row));
    let frame = buffer(120, 24, &mut wide);
    // 「短句」占 4 列（两个宽字素各两格，而后一格在缓冲里是跳过格）。所以看第 1 格与第 3 格：
    // 都在文字里、都反白；第 5 格起是它右边那块填充，不反白。
    assert!(
        frame[(column, row)].modifier.contains(Modifier::REVERSED)
            && frame[(column + 2, row)]
                .modifier
                .contains(Modifier::REVERSED),
        "文字那两格反白"
    );
    assert!(
        !frame[(column + 4, row)]
            .modifier
            .contains(Modifier::REVERSED),
        "文字右边那些空白格不反白"
    );

    // 抬起：那一次点击被拖选拦下（正文详情不开），反白跟着走掉。
    state.mouse(release(column + 4, row + 1));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains(MESSAGE_FACES), "拖选不吃成一次点击：{text}");
    let frame = buffer(120, 24, &mut state);
    assert!(
        !frame[(column, row)].modifier.contains(Modifier::REVERSED),
        "抬起之后反白走掉"
    );
}

/// 拖选之后提示行给一句回执：字数与行数（`.scratch/tui-feedback/spec.md` §6）。
#[test]
fn a_drag_that_selects_text_receipts_it_on_the_hint_row() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(message(1, "第一行正文\n第二行正文", None));
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "第一行正文").expect("正文行在屏幕上");
    state.mouse(press(column, row));
    state.mouse(drag_to(column + 3, row + 1));
    state.mouse(release(column + 3, row + 1));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("已复制"), "提示行给回执：{text}");
    assert!(text.contains("字"), "带着字数：{text}");

    // 交给剪贴板的那串字节：OSC 52，载荷是取到的那段文本的 base64。它由**运行期**写出去，
    // 状态机只把它放在这里（`.scratch/tui-feedback/spec.md` §6）—— 测试因此不会往自己的
    // stdout 吐一个剪贴板序列。
    let expected = heng::render::selection::osc52("第一行正文\n第二");
    assert_eq!(
        state.take_clipboard().as_deref(),
        Some(expected.as_str()),
        "选区的文本按区域取行"
    );
    assert!(state.take_clipboard().is_none(), "取走即清");

    // 没拖起来的那一次（一次普通点击）不留回执。
    let mut plain = state_with_roster(&["kimi"]);
    plain.apply(message(1, "正文在这里", None));
    let frame = buffer(120, 24, &mut plain);
    let (column, row) = cell_of(&frame, 120, 24, "正文在这里").expect("正文行在屏幕上");
    click(&mut plain, column, row);
    let text = screen(120, 24, &mut plain).join("\n");
    assert!(!text.contains("已复制"), "点一下不留回执：{text}");
}

/// 没拖动的按下-抬起仍是一次普通点击；**挪一格**就已经是拖了（门槛是一格 —— 真机试过两格之后
/// 撤回，见 `.scratch/tui-feedback/spec.md` §9）。
#[test]
fn a_press_and_release_without_a_drag_is_still_a_click() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(message(1, "被点开的消息", None));
    open_trace_tab(&mut state, 120, 24);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "被点开的消息").expect("轨迹页上那行消息");
    state.mouse(press(column, row));
    state.mouse(release(column, row));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains(MESSAGE_FACES), "没拖动就是一次点击：{text}");
}

/// 在主列页签上按住拖开、再松开：页签**不**切。
///
/// 页签那一行不是文本块，所以这一次按下没有所属区域、也就成不了拖选（票 05 的 `Drag.block`
/// 是 `None`）—— 松开仍然按一次点击处理，只是落点是拖到的那一格，而不是原来的标签。
#[test]
fn a_drag_that_starts_on_a_tab_does_not_switch_the_page() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(message(1, "问题", None));
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, wording::TAB_TRACE).expect("页签在屏幕上");
    state.mouse(press(column, row));
    state.mouse(drag_to(column + 6, row));
    state.mouse(release(column + 6, row));
    let conversation = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(conversation.contains("问题"), "还在对话页：{conversation}");
}

/// 详情覆盖层里按住拖动：覆盖层**不**关 —— 框内的拖动是在选它的正文。
#[test]
fn a_drag_inside_the_detail_overlay_selects_instead_of_closing_it() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(message(1, "第一段正文\n第二段正文", None));
    open_trace_tab(&mut state, 120, 24);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "第一段正文").expect("轨迹页上那行消息");
    click(&mut state, column, row);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains(MESSAGE_FACES), "详情开着：{text}");

    // 覆盖层里那一行正文：拖过两格。
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "第二段正文").expect("覆盖层里有全文");
    state.mouse(press(column, row));
    state.mouse(drag_to(column + 2, row));
    state.mouse(release(column + 2, row));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains(MESSAGE_FACES), "覆盖层还开着：{text}");
}

#[test]
fn every_size_in_the_matrix_draws_the_regions_its_budget_allows() {
    // spec 的几何表覆盖的那张尺寸矩阵，每一行都按它进表的
    // 理由断言：左栏的档位与身份、状态行的档位、
    // 以及外壳让出的转录行数（spec §1–§2）。
    let cases = [
        // 宽、高、左栏档位、身份、状态行上有没有模型、转录行数
        // 输入区的地板是三行，所以每个档位给转录 `h - 6 - 3` —— 那 6
        // 行里含主列页签条的两行（`.scratch/trace-in-main/spec.md` §1）。
        (40u16, 10u16, None, "", false, 1usize),
        (40, 24, None, "", false, 15),
        (60, 24, None, "", true, 15),
        // 80 列下状态行放不下「模型 + 档位」那一格了（2026-10-08：状态行多了一段档位，
        // `.scratch/model-switching/spec.md` §11），于是它在 100 列才回来。
        (80, 14, Some(28u16), "heng", false, 5),
        (80, 24, Some(28), "heng", false, 15),
        (100, 24, Some(28), "heng", true, 15),
        (120, 24, Some(40), "mark", true, 15),
        (174, 50, Some(40), "mark", true, 41),
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
            text.contains("询问 ┆"),
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
            rows[TRANSCRIPT_TOP + rows_expected].contains("询问 ┆"),
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
            None => assert!(
                !text.contains("调用量"),
                "{width}x{height} 整条藏起左栏：{text}"
            ),
        }
        assert_eq!(
            any_mark_on_screen(&text),
            identity == "mark",
            "{width}x{height} 只在宽档上画标记：{text}"
        );
        assert_eq!(
            text.contains("heng"),
            identity == "heng",
            "{width}x{height} 只在窄档上画文字身份：{text}"
        );
        assert_eq!(
            text.contains("claude-sonnet-4-5"),
            model,
            "{width}x{height} 只在放得下时把模型留在状态行上：{text}"
        );
        // 档位那一格与模型**同生共死**（`.scratch/model-switching/spec.md` §11）：模型在，
        // 它就在。80 列是它开始让位的那一档 —— 状态行多了一段就是这么贵。
    }
}

#[test]
fn the_sidebar_gives_up_its_identity_before_the_page_floor() {
    // 左栏自己的高度阶梯（`.scratch/trace-tab/spec.md` §4）：**标记**先走
    // —— 退到文字身份，再退到什么都不画 —— 只要页区还保得住那三行地板。
    // 宽度在这整件事里从不参与，页高也不再随「读数有几项」走。
    // 左栏顶上留的那一行空行是**花掉的**，而底下不再让给提示行：提示行回到了主列里
    // （`.scratch/tui-feedback/spec.md` §2），所以阶梯看到的内容行是 `h − 1`。
    use heng::render::layout::{SidebarKind, plan};
    use ratatui::layout::Rect;

    let cases = [
        // 高度、身份、页区行数
        (32u16, SidebarKind::Mark, 5u16), // 31 − 23 − 3：篆书与它那 5 行页区地板刚好都够
        (31, SidebarKind::MarkCompact, 22), // 30 − 5 − 3：篆书让位，页区几乎全拿回来
        (24, SidebarKind::MarkCompact, 15), // 23 − 5 − 3
        (12, SidebarKind::MarkCompact, 3), // 11 − 5 − 3，正好是地板
        (11, SidebarKind::Text, 6),       // 10 − 1 − 3：收起来那版让位换回页高
        (10, SidebarKind::Text, 5),       // 9 − 1 − 3
    ];
    for (height, kind, page_rows) in cases {
        let mut state = state();
        let rows = screen(120, height, &mut state);
        let text = rows.join("\n");
        assert_eq!(
            block_mark_on_screen(&text),
            kind == SidebarKind::Mark,
            "{height} 行画不画篆书：{text}"
        );
        assert_eq!(
            compact_mark_on_screen(&text),
            kind == SidebarKind::MarkCompact,
            "{height} 行画不画收起来那版标记：{text}"
        );
        assert_eq!(
            text.contains("heng"),
            kind == SidebarKind::Text,
            "{height} 行的文字身份：{text}"
        );
        let regions = plan(Rect::new(0, 0, 120, height), 1, true);
        assert_eq!(regions.sidebar_kind, kind, "{height} 行的身份档");
        assert_eq!(
            regions.sidebar_page.expect("左栏在").height,
            page_rows,
            "{height} 行的页区"
        );
        // 页签条与回答「还剩多少余地」的那三项读数在每一档都留下。
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

/// 页区撑满之后左栏四档的实测（`.scratch/trace-tab/spec.md` §4）：
/// 高度 = 内容行 − 身份 − 页签条，「用量字段数」那个常数退休。
#[test]
fn the_sidebar_page_fills_the_height_the_identity_and_tabs_leave() {
    use heng::render::layout::{SidebarKind, plan};
    use ratatui::layout::Rect;

    let cases = [
        // 宽、高、身份、页区行数
        (120u16, 24u16, SidebarKind::MarkCompact, 15u16), // 23 − 5 − 3
        (120, 32, SidebarKind::Mark, 5),                  // 31 − 23 − 3：篆书这一档的页区地板
        (80, 24, SidebarKind::Text, 19),                  // 23 − 1 − 3
        (80, 10, SidebarKind::Text, 5),                   // 9 − 1 − 3
    ];
    for (width, height, kind, page_rows) in cases {
        let regions = plan(Rect::new(0, 0, width, height), 1, true);
        assert_eq!(regions.sidebar_kind, kind, "{width}x{height} 的身份");
        let page = regions.sidebar_page.expect("左栏在");
        assert_eq!(page.height, page_rows, "{width}x{height} 的页区：{page:?}");
    }
}

/// 极矮终端里那行文字身份回来了：新阶梯给出的地板是「页签条 + 3 行」，
/// 不再拿身份去换那六行读数（`.scratch/trace-tab/issues/08-sidebar-page-fills-height.md`）。
#[test]
fn the_tiny_terminal_keeps_its_text_identity() {
    let rows = screen(80, 10, &mut state());
    let text = rows.join("\n");
    assert!(text.contains("heng"), "80×10 保住文字身份：{text}");
    assert!(
        text.contains(wording::TAB_USAGE) && text.contains(wording::TAB_TRACE),
        "页签条照旧：{text}"
    );
}

/// 调用量页不看高度：页区有 15 行时它仍然贴顶画它的六项读数。
#[test]
fn the_usage_page_still_draws_its_six_fields_from_the_top() {
    let page = panel_text(120, 24, &mut state());
    assert_eq!(page.len(), 6, "六项读数照旧：{page:?}");
    assert!(page[0].contains("上下文"), "{page:?}");
    assert!(page[5].contains("缓存"), "{page:?}");
}

/// 页高撑满之后 todo 页白捡了更多条目：12 项在 120×24 里全部画下，
/// 不再有 `＋N 项` 那一行（`.scratch/trace-tab/spec.md` §4）。
#[test]
fn the_todo_page_fits_more_items_now_that_the_page_fills_the_height() {
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
    let shown = (0..12)
        .filter(|index| {
            rows.iter()
                .any(|line| line.contains(&format!("第 {index} 项")))
        })
        .count();
    assert_eq!(shown, 12, "15 行的页区把 12 项都装下了：{rows:#?}");
    assert!(
        !rows.iter().any(|line| line.contains("＋")),
        "没有溢出行：{rows:#?}"
    );
    assert!(
        rows.iter().any(|line| line.contains("0/12")),
        "进度行照旧，只是搬到了页区第一行：{rows:#?}"
    );
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
    use heng::render::wording;

    let mut state = state();
    state.live_event(RenderEvent::notice("换页之前的一句话".to_owned()));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("token"), "在显示调用量那一页：{text}");

    // 切到 `文件`：这一页现在画的是工作区那棵树（还没有索引时说的是它在等），
    // 读数让位（spec §3）。左栏的页签只剩三页 —— `轨迹` 搬进了主列
    // （`.scratch/trace-in-main/spec.md` §2）。
    let frame = buffer(120, 24, &mut state);
    let (column, row) = tab_cell(&frame, 120, 24, wording::TAB_FILES);
    click(&mut state, column, row);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains(wording::files_loading()),
        "文件页说它在等工作区那份索引：{text}"
    );
    assert!(!text.contains("token"), "而读数不在：{text}");
    // 于是状态行成了唯一一项读数（spec §3）。
    assert!(text.contains("上下文"), "状态行的占比还在：{text}");

    // 选中的标签跟着它一起挪了。
    let frame = buffer(120, 24, &mut state);
    let (column, row) = tab_cell(&frame, 120, 24, wording::TAB_FILES);
    assert_eq!(frame[(column, row)].fg, Color::LightMagenta);

    // 再切回来：调用量恢复了那些字段。
    let (column, row) = tab_cell(&frame, 120, 24, wording::TAB_USAGE);
    click(&mut state, column, row);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("token"), "读数回来了：{text}");
    assert!(
        !text.contains(wording::files_loading()),
        "文件页那句话不见了：{text}"
    );
}

// ---------------------------------------------------------------------------
// 文件页：左栏那个 `文件` 页签画什么
// （`.scratch/files-page/spec.md` §1、§3、§4）
// ---------------------------------------------------------------------------

/// 点左栏的 `文件` 页签，然后重画一帧 —— 页区与命中区域都按画出来的那一帧算。
fn open_files_page(state: &mut TuiState, width: u16, height: u16) {
    let frame = buffer(width, height, state);
    let (column, row) = tab_cell(&frame, width, height, wording::TAB_FILES);
    click(state, column, row);
    let _ = screen(width, height, state);
}

#[test]
fn the_files_page_lists_the_workspace_top_level() {
    // 这一页的数据是 `@` 那份会话级索引（`.scratch/files-page/spec.md` §1）：
    // 目录带尾斜杠，子项在目录收起时不出现。
    let mut state = state();
    install_files(&mut state, &["README.md", "src/", "src/a.rs", "z.rs"]);
    open_files_page(&mut state, 120, 24);

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("src/"), "目录带尾斜杠画出来：{text}");
    assert!(
        text.contains("README.md") && text.contains("z.rs"),
        "顶层的文件也在：{text}"
    );
    assert!(!text.contains("a.rs"), "初始全部收起，子项不露面：{text}");
}

#[test]
fn the_tree_is_two_columns_per_level_and_puts_directories_first() {
    // 同层里目录排在文件前面，缩进每层两列 —— 而**顺序是渲染层重排的**：
    // 索引序里 `README.md` 在 `src/` 之前（`.scratch/files-page/spec.md` §1、§3）。
    let mut state = state();
    install_files(
        &mut state,
        &["README.md", "src/", "src/a.rs", "sub/", "sub/deep/"],
    );
    open_files_page(&mut state, 120, 24);

    let rows = screen(120, 24, &mut state);
    let page = sidebar_page(&rows);
    assert_eq!(
        sidebar_row(&rows[page]).trim_end(),
        "▸ src/",
        "顶层的第一个是目录，带折叠字形与尾斜杠"
    );
    assert_eq!(
        sidebar_row(&rows[page + 1]).trim_end(),
        "▸ sub/",
        "同类之间保持索引序"
    );
    assert_eq!(
        sidebar_row(&rows[page + 2]).trim_end(),
        "  README.md",
        "文件排在目录后面，字形列留空而名字与目录行同列"
    );
}

#[test]
fn clicking_a_directory_expands_and_collapses_it() {
    let mut state = state();
    install_files(&mut state, &["src/", "src/a.rs"]);
    open_files_page(&mut state, 120, 24);
    assert!(!screen(120, 24, &mut state).join("\n").contains("a.rs"));

    let page = sidebar_page(&screen(120, 24, &mut state));
    click_in_row(&mut state, 120, 24, page as u16, "src/");
    let rows = screen(120, 24, &mut state);
    assert!(
        rows.join("\n").contains("a.rs"),
        "点一下目录就摊开：{rows:#?}"
    );
    assert_eq!(
        sidebar_row(&rows[page + 1]).trim_end(),
        "    a.rs",
        "子项再缩进两列，字形列留空"
    );
    assert_eq!(
        sidebar_row(&rows[page]).trim_end(),
        "▾ src/",
        "展开态换一个字形"
    );

    click_in_row(&mut state, 120, 24, page as u16, "src/");
    assert!(
        !screen(120, 24, &mut state).join("\n").contains("a.rs"),
        "再点一下收起"
    );
}

#[test]
fn the_tree_keeps_its_shape_across_repaints_and_a_tab_switch() {
    // 展开状态活在进程里，重画与切页都不该把它丢掉
    // （`.scratch/files-page/spec.md` §1）。
    let mut state = state();
    install_files(&mut state, &["src/", "src/a.rs"]);
    open_files_page(&mut state, 120, 24);
    let page = sidebar_page(&screen(120, 24, &mut state));
    click_in_row(&mut state, 120, 24, page as u16, "src/");

    // 切到调用量再切回来。
    for label in [wording::TAB_USAGE, wording::TAB_FILES] {
        let frame = buffer(120, 24, &mut state);
        let (column, row) = tab_cell(&frame, 120, 24, label);
        click(&mut state, column, row);
    }
    let rows = screen(120, 24, &mut state);
    assert!(
        rows.join("\n").contains("a.rs"),
        "切页回来仍然是摊开的：{rows:#?}"
    );
}

#[test]
fn the_files_page_names_the_two_empty_states() {
    // 还没扫过 / 一次遍历在飞：说它在等。工作区真的是空的：说它是空的。
    // 两者都不画一块空白，也不编数据（`.scratch/files-page/spec.md` §1，用户故事 31）。
    let mut pending = state();
    open_files_page(&mut pending, 120, 24);
    let text = screen(120, 24, &mut pending).join("\n");
    assert!(text.contains(wording::files_loading()), "还没就绪：{text}");
    assert!(
        !text.contains(wording::files_empty()),
        "这不是「空」：{text}"
    );

    let mut empty = state();
    install_files(&mut empty, &[]);
    open_files_page(&mut empty, 120, 24);
    let text = screen(120, 24, &mut empty).join("\n");
    assert!(
        text.contains(wording::files_empty()),
        "读完了就是空的：{text}"
    );
    assert!(
        !text.contains(wording::files_loading()),
        "它不等什么了：{text}"
    );
}

#[test]
fn the_wheel_over_the_files_page_scrolls_the_tree() {
    let mut state = state();
    let paths: Vec<String> = (0..40).map(|index| format!("file-{index:02}.rs")).collect();
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    install_files(&mut state, &paths);
    open_files_page(&mut state, 120, 24);

    let page = sidebar_page(&screen(120, 24, &mut state));
    let before = screen(120, 24, &mut state);
    assert!(before.join("\n").contains("file-00.rs"), "从顶上开始");

    // 指针落在左栏页区里（第一列就是左栏），滚轮归这一页。
    state.mouse(wheel_at(2, page as u16 + 1, false));
    let after = screen(120, 24, &mut state);
    assert!(
        !after.join("\n").contains("file-00.rs") && after.join("\n").contains("file-01.rs"),
        "滚轮挪动了这一页：{after:#?}"
    );

    // 指针在主列里时照旧滚主列那一页。
    state.mouse(wheel_at(100, 5, false));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("file-01.rs"), "主列上的滚轮没有动这一页");
}

#[test]
fn a_narrow_sidebar_still_reads_the_tree() {
    // 80 列下左栏是 28 列那一档：长名字被截断，而树读得下去
    // （`.scratch/files-page/spec.md` §3）。
    let mut state = state();
    install_files(
        &mut state,
        &["src/", "src/a-very-long-file-name-that-will-not-fit.rs"],
    );
    open_files_page(&mut state, 80, 24);
    let page = sidebar_page(&screen(80, 24, &mut state));
    click_in_row(&mut state, 80, 24, page as u16, "src/");

    let rows = screen(80, 24, &mut state);
    let child = sidebar_row(&rows[page + 1]);
    assert!(child.contains('…'), "放不下的名字被截断：{child:?}");
    assert!(
        text_columns(child.trim_end()) <= 28,
        "而它没有溢出左栏：{child:?}"
    );
}

#[test]
fn clicking_the_tree_hands_it_the_keyboard_and_the_arrows_walk_the_rows() {
    // 点左栏一处就把键盘交给这一页，焦点行落在点的那一行上；`↑` / `↓` 沿着
    // 可见行移动，到顶不越界（`.scratch/files-page/spec.md` §5，用户故事 14、15、20）。
    let mut state = state();
    install_files(&mut state, &["a.rs", "b.rs", "c.rs"]);
    open_files_page(&mut state, 120, 24);
    let page = sidebar_page(&screen(120, 24, &mut state));

    click_in_row(&mut state, 120, 24, page as u16 + 2, "c.rs");
    // 点文件行会打开内容弹窗（票 08）：这一条只问焦点行，所以先把它关掉 ——
    // `Esc` 一次关一层，键盘仍留在这一页上。
    state.key(Key::Esc);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "c.rs").expect("c.rs 在屏幕上");
    assert_eq!(
        frame[(column, row)].fg,
        Color::LightMagenta,
        "焦点行是亮的那一个"
    );
    assert!(
        frame[(column, row)].modifier.contains(Modifier::BOLD),
        "而且它是粗的"
    );

    state.key(Key::Up);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "b.rs").expect("b.rs 在屏幕上");
    assert_eq!(
        frame[(column, row)].fg,
        Color::LightMagenta,
        "焦点往上挪一格"
    );

    // 到顶再按不越界：焦点停在第 0 行，而那一行仍然是亮的。
    state.key(Key::Up);
    state.key(Key::Up);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "a.rs").expect("a.rs 在屏幕上");
    assert_eq!(frame[(column, row)].fg, Color::LightMagenta, "停在第一行");
    assert_eq!(frame[(column, row)].modifier, Modifier::BOLD);
}

#[test]
fn clicking_the_input_area_takes_the_keyboard_back_from_the_files_page() {
    // 点左栏把键盘交给文件页之后，点回输入区该把它还回来 —— 否则一个点开过文件的人想接着
    // 打字，只能先按一下 `Esc`（`.scratch/files-page/spec.md` §5 的「归还」那一条）。
    let mut state = idle();
    install_files(&mut state, &["a.rs"]);
    open_files_page(&mut state, 120, 24);
    let page = sidebar_page(&screen(120, 24, &mut state));
    // 点文件行：键盘交给这一页，同时开出内容弹窗 —— 先按 `Esc` 把弹窗关掉（一次手势一层），
    // 键盘仍在文件页上。
    click_in_row(&mut state, 120, 24, page as u16, "a.rs");
    state.key(Key::Esc);
    state.key(Key::Char('x'));
    let (input_x, input_y) = input_corner(120, 24);
    let frame = buffer(120, 24, &mut state);
    assert!(
        !row_text(&frame, input_y, 120).contains('x'),
        "键盘在文件页时打字不落进草稿：{}",
        row_text(&frame, input_y, 120)
    );

    // 点输入区那一格：键盘该回到输入区。
    click(&mut state, input_x, input_y);
    state.key(Key::Char('x'));
    let frame = buffer(120, 24, &mut state);
    assert!(
        row_text(&frame, input_y, 120).contains('x'),
        "点过输入区之后，打字该落进草稿：{}",
        row_text(&frame, input_y, 120)
    );
}

#[test]
fn the_arrows_open_and_collapse_a_directory_and_enter_inserts_the_path() {
    let mut state = idle();
    install_files(&mut state, &["src/", "src/a.rs"]);
    open_files_page(&mut state, 120, 24);
    let page = sidebar_page(&screen(120, 24, &mut state));
    click_in_row(&mut state, 120, 24, page as u16, "src/");

    state.key(Key::Right);
    assert!(
        screen(120, 24, &mut state).join("\n").contains("a.rs"),
        "→ 摊开目录"
    );
    state.key(Key::Left);
    assert!(
        !screen(120, 24, &mut state).join("\n").contains("a.rs"),
        "← 收起目录"
    );

    // `Enter` 把这一行的路径作为 `@路径` 插进草稿（目录带尾斜杠），
    // 并把键盘还给输入区 —— 插完接着就要打字（用户故事 17、18）。
    state.key(Key::Enter);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("@src/"), "路径进了草稿：{text}");

    state.key(Key::Char('x'));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("@src/x"),
        "接着打的字落在同一个草稿里：{text}"
    );
}

#[test]
fn the_tree_keys_do_not_reach_the_draft() {
    // 键盘在这一页时，走树的那几个键归这一页：草稿一个字不动
    // （`.scratch/files-page/spec.md` §5，用户故事 16）。
    let mut state = idle();
    install_files(&mut state, &["src/", "src/a.rs"]);
    open_files_page(&mut state, 120, 24);
    for ch in "hi".chars() {
        state.key(Key::Char(ch));
    }
    let page = sidebar_page(&screen(120, 24, &mut state));
    click_in_row(&mut state, 120, 24, page as u16, "src/");

    for key in [Key::Down, Key::Up, Key::Right, Key::Left] {
        state.key(key);
    }
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("hi"), "草稿还在：{text}");
    assert!(!text.contains("@src/"), "那几个键一个都没落进草稿：{text}");

    // 打字也不落进草稿 —— 键盘确实在左栏，而不是「那几个键恰好被吃掉了」。
    state.key(Key::Char('z'));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains("hiz"), "打字没有落到草稿里：{text}");
}

#[test]
fn escape_hands_the_keyboard_back_without_cancelling_the_turn() {
    // `Esc` 只把键盘还回去 —— 一次手势一层，它**不**取消正在跑的回合
    // （`.scratch/files-page/spec.md` §5）。要取消就等键盘回去之后再按一下。
    let mut state = idle();
    install_files(&mut state, &["src/"]);
    open_files_page(&mut state, 120, 24);
    let page = sidebar_page(&screen(120, 24, &mut state));
    click_in_row(&mut state, 120, 24, page as u16, "src/");
    state.request(ConsoleRequest::RunState { running: true });
    let _ = state.take_events();

    state.key(Key::Esc);
    assert!(state.take_events().is_empty(), "第一下只还键盘，不取消回合");
    state.key(Key::Down);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("src/"),
        "键盘回去了，走树的键也不再被吃：{text}"
    );

    state.key(Key::Esc);
    assert_eq!(
        state.take_events(),
        vec![FrontEndEvent::Cancel],
        "键盘回去之后再按一下才按忙碌那一支取消"
    );
}

#[test]
fn the_tab_row_also_hands_the_keyboard_to_the_sidebar() {
    // 「点左栏任意处」把页签条也算进去：切到文件页那一下就够了，不必再点页区
    // （`.scratch/files-page/spec.md` §5）。
    let mut state = state();
    install_files(&mut state, &["a.rs", "b.rs"]);
    open_files_page(&mut state, 120, 24);

    state.key(Key::Down);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "b.rs").expect("b.rs 在屏幕上");
    assert_eq!(
        frame[(column, row)].fg,
        Color::LightMagenta,
        "↓ 走动了这一页的焦点行"
    );
}

/// 一个临时工作区：文件页的弹窗按 `TuiState.cwd` 读盘
/// （`.scratch/files-page/spec.md` §6）。
fn workspace(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("heng-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("临时工作区");
    dir
}

/// 一个以 `dir` 为工作目录、索引里装着 `paths` 的文件页。
fn files_in(dir: &std::path::Path, paths: &[&str]) -> TuiState {
    let mut state = TuiState::new(facts(), dir.to_path_buf(), None);
    install_files(&mut state, paths);
    state
}

#[test]
fn a_workspace_file_opens_in_the_detail_overlay() {
    let dir = workspace("open");
    std::fs::create_dir_all(dir.join("src")).expect("一个目录");
    std::fs::write(dir.join("src/hello.rs"), "fn main() {}\n").expect("写一个文件");
    let mut state = files_in(&dir, &["src/", "src/hello.rs"]);
    open_files_page(&mut state, 120, 24);
    // 先把目录摊开：关掉弹窗之后这一页要停在原处，摊开的状态也是其中一部分
    // （用户故事 12）。
    let page = sidebar_page(&screen(120, 24, &mut state));
    click_in_row(&mut state, 120, 24, page as u16, "src/");
    assert!(screen(120, 24, &mut state).join("\n").contains("hello.rs"));

    click_row(&mut state, 120, 24, "hello.rs");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("fn main() {}"), "文件内容画出来了：{text}");
    assert!(text.contains("esc 关闭"), "页脚点出出口：{text}");
    assert!(
        state.take_events().is_empty(),
        "看一眼文件既不推任何前端事件、也不进事件流"
    );

    // 关掉之后这一页停在原处：树还摊开着。
    state.key(Key::Esc);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("hello.rs"),
        "回到那棵树，而且它还摊着：{text}"
    );
    assert!(!text.contains("esc 关闭"), "弹窗关上了：{text}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_file_key_opens_the_same_overlay() {
    let dir = workspace("key");
    std::fs::write(dir.join("hello.rs"), "fn main() {}\n").expect("写一个文件");
    let mut state = files_in(&dir, &["hello.rs"]);
    open_files_page(&mut state, 120, 24);
    let page = sidebar_page(&screen(120, 24, &mut state));
    click_in_row(&mut state, 120, 24, page as u16, "hello.rs");

    state.key(Key::Right);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("fn main() {}"), "→ 也打开它：{text}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_binary_file_only_gets_one_line() {
    let dir = workspace("binary");
    std::fs::write(dir.join("blob.bin"), [0u8, 1, 2, 3, b'a', 0xFF, 0xFE]).expect("写一个二进制");
    let mut state = files_in(&dir, &["blob.bin"]);
    open_files_page(&mut state, 120, 24);

    click_row(&mut state, 120, 24, "blob.bin");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains(wording::file_binary()), "只报一句：{text}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_that_cannot_be_read_says_so() {
    let dir = workspace("missing");
    // 索引里有它，盘上没有：读不了要是一句话，不是一块空白。
    let mut state = files_in(&dir, &["missing.rs"]);
    open_files_page(&mut state, 120, 24);

    click_row(&mut state, 120, 24, "missing.rs");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains(wording::file_unreadable()), "读不了：{text}");
    assert!(
        text.contains("esc 关闭"),
        "而它仍然是一个能关掉的弹窗：{text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_that_is_not_utf8_says_so() {
    let dir = workspace("not-text");
    std::fs::write(dir.join("latin.txt"), [0xE4, 0xBD, 0x20, 0xE5]).expect("写一段坏字节");
    let mut state = files_in(&dir, &["latin.txt"]);
    open_files_page(&mut state, 120, 24);

    click_row(&mut state, 120, 24, "latin.txt");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains(wording::file_not_text()), "不是文本：{text}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_long_file_is_truncated_and_says_so() {
    let dir = workspace("long");
    let body: Vec<String> = (0..3_000).map(|line| format!("line {line}")).collect();
    std::fs::write(dir.join("long.txt"), body.join("\n")).expect("写一个长文件");
    let mut state = files_in(&dir, &["long.txt"]);
    open_files_page(&mut state, 120, 24);

    click_row(&mut state, 120, 24, "long.txt");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("line 0"), "从头开始画：{text}");

    // 翻到底：这个高度下一屏十三行，两千行要一百多下。
    for _ in 0..250 {
        state.key(Key::PageDown);
    }
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains(wording::detail_truncated()),
        "说明截断了：{text}"
    );
    assert!(!text.contains("line 2999"), "末尾那一行没读进来：{text}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_wide_line_wraps_to_the_text_area() {
    let dir = workspace("wide");
    let wide = format!("{}TAIL", "y".repeat(300));
    std::fs::write(dir.join("wide.txt"), wide).expect("写一条长行");
    let mut state = files_in(&dir, &["wide.txt"]);
    open_files_page(&mut state, 120, 24);

    click_row(&mut state, 120, 24, "wide.txt");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("TAIL"),
        "长行折到文本区宽，尾巴看得见：{text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_overlay_highlights_the_code_it_knows() {
    // 按扩展名认语言，走既有的那一层高亮（与 markdown 代码块同一个源）
    // （`.scratch/files-page/spec.md` §6，用户故事 23）。
    let dir = workspace("highlight");
    std::fs::write(dir.join("hi.rs"), "fn main() {}\n").expect("写一个 rust 文件");
    let mut state = files_in(&dir, &["hi.rs"]);
    open_files_page(&mut state, 120, 24);

    click_row(&mut state, 120, 24, "hi.rs");
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "fn").expect("那一段代码在屏幕上");
    assert_eq!(
        frame[(column, row)].fg,
        Color::Magenta,
        "关键字是内容域的关键字色"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_language_it_does_not_know_is_still_drawn_as_text() {
    let dir = workspace("plain-text");
    std::fs::write(dir.join("notes.weird"), "just some words\n").expect("写一个认不出的文件");
    let mut state = files_in(&dir, &["notes.weird"]);
    open_files_page(&mut state, 120, 24);

    click_row(&mut state, 120, 24, "notes.weird");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("just some words"), "照旧画出来：{text}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn line_numbers_sit_on_the_first_display_line_of_a_logical_line() {
    // 行号插在折行**之前**：折出来的续行自然不带行号
    // （`.scratch/files-page/spec.md` §6，用户故事 23）。
    let dir = workspace("numbers");
    std::fs::write(dir.join("wide.rs"), format!("{}\n", "x".repeat(300))).expect("写一条长行");
    let mut state = files_in(&dir, &["wide.rs"]);
    open_files_page(&mut state, 120, 24);

    click_row(&mut state, 120, 24, "wide.rs");
    let rows = screen(120, 24, &mut state);
    let first = rows
        .iter()
        .position(|row| row.contains("1 xxx"))
        .unwrap_or_else(|| panic!("逻辑行的第一行带着行号：{rows:#?}"));
    let next: String = rows[first + 1]
        .trim_start_matches(['┆', '┄', ' '])
        .to_owned();
    assert!(
        next.starts_with("xxxx"),
        "折出来的续行不带行号，也不缩进：{:?}",
        rows[first + 1]
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_workspace_change_asks_for_a_rescan_and_never_draws_a_line() {
    // 模型改过工作区之后这一页要跟上（`.scratch/files-page/spec.md` §2、用户故事 28–30）：
    // 循环侧推一条**静默**信号，前端置位，渲染循环下一轮真去起遍历。
    let mut state = state();
    assert!(state.take_file_scan(), "进 TUI 先预热一次");
    state.files_loaded(vec!["a.rs".into()]);
    let before = screen(120, 24, &mut state).join("\n");

    // 连着三次改动：一个位，因此是一次遍历。屏幕上一个字都不变。
    for _ in 0..3 {
        state.live_event(RenderEvent::WorkspaceChanged);
    }
    let after = screen(120, 24, &mut state).join("\n");
    assert_eq!(before, after, "这条信号不画任何东西");
    assert!(state.take_file_scan(), "它请了一次重扫");
    assert!(!state.take_file_scan(), "一次遍历还在飞，不并发");

    // 遍历在飞的时候又改了一次：位留着，结果落地之后的下一轮补发。
    state.live_event(RenderEvent::WorkspaceChanged);
    assert!(!state.take_file_scan(), "飞着的那次不被抢");
    state.files_loaded(vec!["a.rs".into(), "b.rs".into()]);
    assert!(state.take_file_scan(), "补发的那一次仍然发得出去");
}

#[test]
fn the_keyboard_goes_back_when_the_sidebar_is_not_the_files_page() {
    // 键盘归属只对文件页成立：另外两页没有能用方向键走的东西，把它扣在那儿只会让输入区静默
    // 失灵；收起左栏同理（与「没地方画覆盖层就关掉它」同一条纪律）。
    let mut state = idle();
    install_files(&mut state, &["src/", "src/a.rs"]);

    // 点 `调用量` 页签：打字照旧落进草稿。
    let frame = buffer(120, 24, &mut state);
    let (column, row) = tab_cell(&frame, 120, 24, wording::TAB_USAGE);
    click(&mut state, column, row);
    state.key(Key::Char('x'));
    let input_y = input_row(120, 24);
    let rows = screen(120, 24, &mut state);
    assert!(
        rows[input_y].contains('x'),
        "输入区收得到字：{}",
        rows[input_y]
    );

    // 文件页里按 `Ctrl-O` 收起左栏：键盘跟着还回去。
    open_files_page(&mut state, 120, 24);
    state.key(Key::CtrlO);
    state.key(Key::Char('y'));
    let rows = screen(120, 24, &mut state);
    assert!(
        rows[input_y].contains("xy"),
        "收起左栏之后打字也落得进去：{}",
        rows[input_y]
    );
}

#[test]
fn the_files_page_can_still_be_dragged_and_copied() {
    // 新增的点击动作不该把这一页的拖选吃掉：它是屏幕上的一块文本区域，与左栏另外两页一样
    // （`.scratch/files-page/spec.md` §4，用户故事 13）。
    let mut state = state();
    install_files(&mut state, &["src/", "src/a.rs"]);
    open_files_page(&mut state, 120, 24);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "src/").expect("目录行在屏幕上");

    state.mouse(press(column, row));
    state.mouse(drag_to(column + 3, row));
    state.mouse(release(column + 3, row));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("已复制"), "拖选出文本、提示行给回执：{text}");
    let expected = heng::render::selection::osc52("src/");
    assert_eq!(
        state.take_clipboard().as_deref(),
        Some(expected.as_str()),
        "取到的是那一行文字"
    );
    assert!(
        screen(120, 24, &mut state).join("\n").contains("src/"),
        "而这一次拖选没有顺手把目录展开或收起"
    );
}

#[test]
fn a_rescan_drops_the_expansion_of_a_directory_that_left_the_index() {
    // 「一次重扫之后，不在索引里的路径直接丢掉」（`.scratch/files-page/spec.md` §1）：
    // 目录回到工作区时是收起的，而不是带着上一次的展开状态复活。
    let mut state = state();
    install_files(&mut state, &["src/", "src/a.rs"]);
    open_files_page(&mut state, 120, 24);
    let page = sidebar_page(&screen(120, 24, &mut state));
    click_in_row(&mut state, 120, 24, page as u16, "src/");
    assert!(screen(120, 24, &mut state).join("\n").contains("a.rs"));

    install_files(&mut state, &["other.rs"]);
    install_files(&mut state, &["src/", "src/a.rs"]);
    let rows = screen(120, 24, &mut state);
    assert!(
        !rows.join("\n").contains("a.rs"),
        "展开状态跟着那一次索引一起走了：{rows:#?}"
    );
}

#[test]
fn only_the_tab_labels_answer_a_click() {
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
        click(&mut state, column, row);
        let text = screen(120, 24, &mut state).join("\n");
        assert!(
            text.contains("token"),
            "点在第 {column} 列不是一个页签，页没有动：{text}"
        );
    }
}

#[test]
fn a_question_keeps_the_tabs_from_answering() {
    use heng::permissions::Answer;
    use heng::render::wording;

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
    click(&mut state, column, row);

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("权限询问"), "问句还在：{text}");
    assert!(
        !text.contains(wording::files_loading()),
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
    use heng::render::wording;

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
        click(&mut state, column, row);
    }
    // 那一栏整个没画出来，所以「切页」在这块屏幕上没有别的可观察结果：能断言的是那几行
    // 点击什么都没打开。
    let text = screen(60, 24, &mut state).join("\n");
    assert!(
        !text.contains(wording::files_loading()),
        "而没有东西被切到：{text}"
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

/// 一个固定的时刻：时间戳的断言要可复现，所以不读真时钟
/// （`.scratch/trace-in-main/spec.md` §5）。
fn fixed_at(hour: u32, minute: u32, second: u32) -> chrono::DateTime<chrono::Utc> {
    use chrono::TimeZone;
    chrono::Utc
        .with_ymd_and_hms(2026, 10, 6, hour, minute, second)
        .single()
        .expect("那是一个真实的时刻")
}

/// 一条带**固定时刻**的事件 —— 时间戳断言用的就是它。
fn at_event(
    seq: u64,
    at: chrono::DateTime<chrono::Utc>,
    payload: heng::events::EventPayload,
) -> heng::render::RenderEvent {
    heng::render::RenderEvent::Logged(heng::events::Event {
        seq,
        at,
        speaker_id: heng::events::SpeakerId::System,
        payload,
    })
}

/// 轨迹页里一条块行去掉行首那个时间戳之后的样子 —— 它是定宽的一列，所以按列切。
fn without_stamp(row: &str) -> &str {
    let columns = heng::render::layout::STAMP_COLUMNS as usize;
    if row.len() >= columns && row.is_char_boundary(columns) {
        &row[columns..]
    } else {
        row
    }
}

/// 一条来自 **user** 的 `MessageCompleted`。
fn user_message(seq: u64, text: &str) -> heng::render::RenderEvent {
    use heng::events::{Event, EventPayload, Role, SpeakerId};
    heng::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: text.to_owned(),
            reasoning: None,

            first_token_ms: None,
        },
    ))
}

/// 一条 `TurnStarted`。
fn turn_started(seq: u64) -> heng::render::RenderEvent {
    use heng::events::{Event, EventPayload, SpeakerId};
    heng::render::RenderEvent::Logged(Event::new(
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
    let mut scrolled = TuiState::new(facts(), std::path::PathBuf::from("/x/heng"), None);
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
        scrolled.key(heng::render::Key::PageUp);
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
        state.key(heng::render::Key::PageUp);
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
    state.key(heng::render::Key::PageUp);
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

    // 窗口底端锚定在一个 `⋮` 下面：120x24 下转录是十五
    // 行，所以一行是标记、十四行是格子 —— 单元 16 到 29，自上
    // 而下。所以偏移 1 是单元 16，偏移 5 是单元 20。每一个都在
    // 全新状态里点，因为跳转会移动视口 —— 也移动格子的窗口。
    for (offset, unit) in [(1usize, 16u64), (5, 20)] {
        let mut state = TuiState::new(facts(), std::path::PathBuf::from("/x/heng"), None);
        turns(&mut state, 30);
        let _ = screen(120, 24, &mut state);
        let cells = turn_rail_cells(&mut state);
        let mark = cells
            .iter()
            .position(|ch| *ch == '⋮')
            .expect("这一列在顶端被裁");
        click(
            &mut state,
            RAIL_AT_120,
            (TRANSCRIPT_TOP + mark + offset) as u16,
        );
        // 名字独占一行，所以落点是名字那一行，话在它下面一行（2026-10-05 的排版修订）。
        let rows = screen(120, 24, &mut state);
        assert!(
            rows[TRANSCRIPT_TOP].contains("[用户]")
                && rows[TRANSCRIPT_TOP + 1].contains(&format!("问题 {unit}")),
            "跳转落在那一个回合自己的问题上：{:?}",
            &rows[TRANSCRIPT_TOP..TRANSCRIPT_TOP + 2]
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
    click(
        &mut state,
        RAIL_AT_120,
        (TRANSCRIPT_TOP + transcript_rows_at_120x24() - 1) as u16,
    );
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
    use heng::events::{Event, EventPayload, Role, RoundMode, SpeakerId, StopReason};

    // 有不止一个讨论者的 `speaker_order` 才让一场会话成为讨论，
    // 而讨论会数自己的轮次 —— `CONTEXT.md` 把 轮次 与 回合 分开
    // （spec §4）。
    let mut state = TuiState::new(
        facts_with_roster(&["kimi", "deepseek"]),
        std::path::PathBuf::from("/x/heng"),
        None,
    );
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

                first_token_ms: None,
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
            state.apply(heng::render::RenderEvent::Logged(Event::new(
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
    click(&mut state, RAIL_AT_120, first);
    let rows = screen(120, 24, &mut state);
    assert!(
        rows[TRANSCRIPT_TOP].contains("[用户]") && rows[TRANSCRIPT_TOP + 1].contains("讨论题目"),
        "会话的问题就是第一个格子落的地方：{:?}",
        &rows[TRANSCRIPT_TOP..TRANSCRIPT_TOP + 2]
    );

    // ……而后面的轮次没有自己的用户消息，所以它的格子落在
    // 轮次的开场行上 —— spec 给「没什么可瞄的单元」的兜底。
    let mut later = TuiState::new(
        facts_with_roster(&["kimi", "deepseek"]),
        std::path::PathBuf::from("/x/heng"),
        None,
    );
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

                first_token_ms: None,
            },
            EventPayload::RoundEnded {
                round,
                reason: StopReason::Completed,
            },
        ]
        .into_iter()
        .enumerate()
        {
            later.apply(heng::render::RenderEvent::Logged(Event::new(
                2 + u64::from(round) * 3 + offset as u64,
                kimi.clone(),
                payload,
            )));
        }
    }
    // 垫料，让转录比窗格高，跳转才有地方
    // 落：提示既不是用户消息也不是边界，所以单元照旧。
    for index in 0..40 {
        later.apply(heng::render::RenderEvent::notice(format!("第 {index} 行")));
    }
    assert_eq!(turn_rail_shape(&mut later), "┊┊┃");
    let second = (TRANSCRIPT_TOP + transcript_rows_at_120x24() - 2) as u16;
    click(&mut later, RAIL_AT_120, second);
    // 轮次开始那一行现在只住在轨迹页（2026-10-05 维护者收紧），所以兜底落点是那一轮
    // **第一条留在对话里的行** —— 它的第一条发言。
    let rows = screen(120, 24, &mut later);
    // 名字只画在**一段连续发言的第一段**上（2026-10-06 的优化），而这三轮都是 kimi 一个人说
    // 的，所以兜底落点就是正文那一行 —— 上面那条空行是分段用的，回合条会跳过它。
    assert!(
        rows[TRANSCRIPT_TOP].contains("第 1 轮"),
        "没有自己用户消息的轮次落在它留下的第一行上：{:?}",
        &rows[TRANSCRIPT_TOP..TRANSCRIPT_TOP + 2]
    );
}

#[test]
fn the_pane_scrolls_back_through_the_transcript_and_returns_to_the_bottom() {
    use heng::render::{Key, RenderEvent};

    let mut state = state();
    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 行")));
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
    use heng::render::{Key, RenderEvent};

    // 每条提示都折成三个显示行，所以按显示行算的上限
    // 会把这批历史只留下三分之一。这就是这里钉住的区分：上限
    // 按源代码行算，所以同一批历史在任何终端宽度下都留得住（spec §3）。
    let mut state = state();
    for index in 0..20_001 {
        state.apply(RenderEvent::notice(format!(
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
    use heng::render::{Key, RenderEvent};

    let mut state = state();
    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 行")));
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
    state.apply(RenderEvent::notice("新的一行".to_owned()));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("↓ 1 行新内容 · 点此到底"), "到了一行：{text}");

    // 滚轮一格挪三行，上下都是。指针落在**转录**上 —— 左栏页矩形里的滚轮归轨迹页
    // （`.scratch/trace-tab/spec.md` §5）。
    let mouse = |kind| ratatui::crossterm::event::MouseEvent {
        kind,
        column: 60,
        row: 10,
        modifiers: ratatui::crossterm::event::KeyModifiers::empty(),
    };
    // 一格把视口往上挪、再挪回来；一格跨过多少条*提示*
    // 取决于窗格显示多少行，所以两端是拿彼此比，
    // 而不是拿一个记住的行号比 —— 这就是「一格三行、
    // 再回来」在任何终端尺寸下的意思。
    state.mouse(mouse(ratatui::crossterm::event::MouseEventKind::ScrollUp));
    let after_wheel_up = first_notice(&screen(120, 24, &mut state));
    assert!(
        after_wheel_up < after_page_up,
        "滚轮把视口往上挪：{after_page_up:?} -> {after_wheel_up:?}"
    );
    state.mouse(mouse(ratatui::crossterm::event::MouseEventKind::ScrollDown));
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
    click(&mut state, 10, 10);
    assert_eq!(
        first_notice(&screen(120, 24, &mut state)),
        after_page_up,
        "点在转录正文上什么都不改"
    );
    click(&mut state, column, row);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("新的一行"), "回到最底下：{text}");
    assert!(!text.contains("点此到底"), "指示器不见了：{text}");
}

#[test]
fn a_resize_keeps_the_reader_on_the_same_line() {
    use heng::render::{Key, RenderEvent};

    let mut state = state();
    // 长到两次翻页之后顶行仍够不到最老的那条：
    // 转录 16 行时 40 条提示算两页，那会把测试的前提
    // 钉死在外壳自己的东西上。
    for index in 0..80 {
        state.apply(RenderEvent::notice(format!("第 {index} 行")));
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
    use heng::render::RenderEvent;

    // 120x24 下主列是 79 列，转录的文本占其中 77
    // 列：最后两列归滚动条与回合条，不管里面
    // 画没画东西，所以文本在 77 列处折行，绝不会因为那两
    // 列在那儿而重新排（spec §1）。
    const TEXT_X: u16 = 41;
    const SCROLLBAR_X: u16 = 118;
    let mut state = state();
    state.apply(RenderEvent::notice("x".repeat(78)));
    let frame = buffer(120, 24, &mut state);
    let first = TRANSCRIPT_TOP as u16;
    assert_eq!(
        frame[(TEXT_X, first)].symbol(),
        "x",
        "这一行从主列的第一列开始"
    );
    assert_eq!(
        frame[(TEXT_X, first + 1)].symbol(),
        "x",
        "78 列的文本溢出 77 列的文本区，落到第二行"
    );
    assert_eq!(
        frame[(SCROLLBAR_X, 5)].symbol(),
        " ",
        "全都放得下时预留的那一列什么都不画"
    );

    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 行")));
    }
    let rows = screen(120, 24, &mut state);
    let transcript = transcript_rows(&rows) as u16;
    let frame = buffer(120, 24, &mut state);
    // 只画滑块、不画轨道（`.scratch/tui-visual-language/issues/07` 决定 1）：那一列于是
    // 有东西、但不占满。
    let thumbs = (0..transcript)
        .filter(|y| frame[(SCROLLBAR_X, *y)].symbol() == "█")
        .count();
    assert!(thumbs > 0, "有东西可读时滑块在");
    assert!(
        thumbs < transcript as usize,
        "轨道不画，所以不是每一行都有字形：{thumbs} / {transcript}"
    );
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
    assert_eq!(rows, 15, "输入区三行给转录留下十五行：{empty:#?}");
    // 转录下面依次是状态行与输入区上方那条横线，所以输入区
    // 从转录底往下数第三行开始。
    let input = TRANSCRIPT_TOP + rows + 2;
    assert!(
        empty[input].trim_matches(['┆', ' ']).is_empty(),
        "输入区的第一行是一份空草稿：{:?}",
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
    assert_eq!(rows, 15, "三行草稿在地板之内，所以几何不动：{three:#?}");
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
        13,
        "超地板两行让转录少两行：{five:#?}"
    );
    assert!(
        five.join("\n").contains("第五行"),
        "草稿在屏幕上：{five:#?}"
    );
}

/// 一条 `UsageRecorded`，按循环会记下来的样子。
fn usage(seq: u64, input: u64, output: u64, cached: u64, miss: u64) -> heng::render::RenderEvent {
    use heng::events::{Event, EventPayload, SpeakerId, Usage};
    heng::render::RenderEvent::Logged(Event::new(
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
fn turn_ended(seq: u64) -> heng::render::RenderEvent {
    use heng::events::{Event, EventPayload, SpeakerId, StopReason};
    heng::render::RenderEvent::Logged(Event::new(
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
            && panel_field(&rows, 0).contains(heng::render::wording::PANEL_UNKNOWN),
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
        panel_field(&rows, 0).contains("9,000 / 20万（4%）"),
        "窗口是最后一次调用的分子：{:?}",
        panel_field(&rows, 0)
    );
    assert!(
        panel_field(&rows, 1).contains("1.2万 / 10万"),
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

#[test]
fn the_context_and_spend_rows_carry_a_share_bar() {
    // 有分母的两行在值前面画一条最多 **10 列**的块字符条（`▓` 满 / `░` 空，前景静音档）——
    // 它占列，所以值列要给它让位置（`.scratch/tui-visual-language/issues/07` 决定 3）。
    // 标签列六列加一个空格，于是条从 x = 7 起、最多到 x = 16。
    let mut state = state();
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));

    let rows = screen(120, 24, &mut state);
    let frame = buffer(120, 24, &mut state);
    let top = sidebar_page(&rows) as u16;

    let bar = |y: u16| -> String {
        (7u16..17)
            .map(|x| frame[(x, y)].symbol().to_owned())
            .collect()
    };
    // 上下文：9,000 / 200,000 = 4.5% → ceil(0.45) = 一格。
    assert_eq!(bar(top), "▓░░░░░░░░░", "上下文那一行");
    assert_eq!(frame[(7, top)].fg, palette::MUTED, "条穿静音档");
    // 花销：12,345 / 100,000 = 12.345% → ceil(1.2345) = 两格。
    assert_eq!(bar(top + 1), "▓▓░░░░░░░░", "花销那一行");

    // 数字一个字没丢：值列让出条与它后面那个空格之后仍然装得下。
    assert!(panel_field(&rows, 0).contains("9,000 / 20万（4%）"));
    assert!(panel_field(&rows, 1).contains("1.2万 / 10万"));
}

#[test]
fn a_row_without_a_denominator_carries_no_share_bar() {
    let mut state = state_without_budget();
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));

    let rows = screen(120, 24, &mut state);

    // 没设额度就没有分母：花销那一行一个块字符都没有。
    assert!(
        !panel_field(&rows, 1).contains(['▓', '░']),
        "没有分母就不画条：{:?}",
        panel_field(&rows, 1)
    );
    // 上下文行自带分母（窗口），所以照画。
    assert!(
        panel_field(&rows, 0).contains('▓'),
        "上下文行照画：{:?}",
        panel_field(&rows, 0)
    );
}

#[test]
fn the_share_bar_survives_the_narrow_sidebar() {
    // 窄档下条自己缩短，读数一个字都不丢：21 列的值列里上下文那一对占 18 列，条只剩两列。
    let mut state = state();
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));

    let rows = screen(80, 14, &mut state);
    let frame = buffer(80, 14, &mut state);
    let top = sidebar_page(&rows) as u16;

    assert_eq!(frame[(7, top)].symbol(), "▓", "窄档也画条");
    assert_eq!(frame[(8, top)].symbol(), "░", "而条只有两列");
    assert_ne!(frame[(9, top)].symbol(), "░", "两列之后就没了");
    assert!(
        panel_field(&rows, 0).ends_with("9,000 / 20万（4%）"),
        "读数一个字不丢：{:?}",
        panel_field(&rows, 0)
    );
}

/// 会话完全没有 token 额度的状态。
fn state_without_budget() -> TuiState {
    let mut facts = facts();
    facts.budget_limit = None;
    TuiState::new(facts, std::path::PathBuf::from("/x/heng"), None)
}

#[test]
fn the_narrow_sidebar_keeps_six_fields_and_their_percentage() {
    // 80x14 是能容下全部六项读数的窄档：十四个内容行
    // 装着文字身份、页签条与六个字段。这条曾经断言的是
    // `12,345 / 200,000（6%）` 那 22 列放不进 21 列的值列、
    // 于是百分比先走；数字制式换成万/亿（`1.2万 / 20万（6%）`，
    // 18 列）之后那个前提不再成立 —— 保的是降级链本身，
    // 所以这里改成断言百分比留了下来（`.scratch/usage-stats-format/issues/02`）。
    let mut state = state();
    // 输出取零，好让花销 —— 输入加输出 —— 正好是上下文那一对
    // 需要的五位数，这才让百分比也过宽。
    state.apply(usage(1, 12_345, 0, 5_000, 7_345));
    state.apply(turn_ended(2));
    let rows = screen(80, 14, &mut state);
    let text = rows.join("\n");

    // 月相是 Emoji：一格占 **2 列**，所以状态行比 `◐` 那套宽一格 —— 80 列的窄档（主列 51 列）
    // 因此放不下整行，**先让位的是模型**（`.scratch/tui-visual-language/issues/11` 的降级顺序）。
    assert!(
        !rows
            .iter()
            .any(|row| row.contains("模型 claude-sonnet-4-5")),
        "80 列下模型先让位：{text}"
    );
    assert!(
        rows.iter()
            .any(|row| row.contains("询问 ┆") && row.contains("上下文") && row.contains("就绪")),
        "剩下的三段都还在：{text}"
    );
    let panel = panel_text(80, 14, &mut state);
    assert_eq!(panel.len(), 6, "六项读数全都放得下：{panel:?}");
    assert!(
        panel[0].starts_with("上下文") && panel[0].ends_with("1.2万 / 20万（6%）"),
        "上下文那一对：{:?}",
        panel[0]
    );
    assert!(
        panel[0].contains('（'),
        "18 列放得进 21 列，所以百分比留着：{:?}",
        panel[0]
    );
    assert!(text.contains("1.2万 / 10万"), "花销：{text}");
    assert!(text.contains("回合"), "回合数：{text}");
    assert!(
        panel[5].contains("5,000 / 7,345"),
        "缓存的拆分：{:?}",
        panel[5]
    );
}

#[test]
fn a_session_with_no_allowance_shows_its_spend_alone() {
    let mut state = state_without_budget();
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("1.2万"), "花了多少：{text}");
    assert!(
        !text.contains("1.2万 / "),
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
    assert_eq!(transcript_rows(&rows), 8, "草稿吃掉的是转录的行：{rows:#?}");
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
    // 上一次调用的分子只在这一行上：`1.5万 / 20万` 与
    // 累计花销那一行是两回事。
    state.apply(usage(2, 15_000, 2_000, 20_000, 10_000));

    let rows = screen(120, 24, &mut state);
    assert!(
        panel_field(&rows, 0).contains("1.5万 / 20万（7%）"),
        "窗口是*最后一次*调用携带的那个值：{:?}",
        panel_field(&rows, 0)
    );
    assert!(
        panel_field(&rows, 1).contains("2.7万 / 10万"),
        "花销是每一次调用的输入加输出：{:?}",
        panel_field(&rows, 1)
    );
    assert!(
        panel_field(&rows, 3).contains("2.4万"),
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
    use heng::render::width::text_columns;

    let mut state = state();
    state.apply(usage(1, 9_000, 3_345, 5_000, 4_000));
    state.apply(turn_ended(2));
    let panel = panel_text(120, 24, &mut state);

    // 标签列宽六（`上下文` 正好填满），然后一个空格，然后是
    // 左栏剩下那三十三列作为值字段 —— 数字从
    // 右边填起。
    let context = "9,000 / 20万（4%）";
    // 标签列六列、一个空格、值在剩下的列里右贴齐 —— 三件事分着断言，
    // 这样换制式之后值变短，前导空格的数目不必再手写。
    assert_eq!(text_columns("上下文"), 6, "标签列宽六");
    assert!(
        panel[0].starts_with("上下文 ") && panel[0].ends_with(context),
        "标签字段六列，然后一个空格，然后值右贴齐：{:?}",
        panel[0]
    );
    assert_eq!(text_columns(&panel[0]), 40, "整行填满左栏：{:?}", panel[0]);
    for row in [&panel[1], &panel[3], &panel[4], &panel[5]] {
        assert!(
            row.ends_with("000") || row.ends_with("345") || row.ends_with('万'),
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
fn a_number_too_wide_for_the_value_column_no_longer_needs_the_bare_form() {
    // 这条测的曾经是 `fit()` 的第一档降级：值放不下时先去千分位。
    // 换成万/亿制式之后面板里不再有千位分隔符可去（`thousands`
    // 只在 < 10000 时出现，最长 13 列，而真实值列最少 16 列），
    // 所以那一档在面板里**已经不可达**；`fit()` 本身照 spec §3 原样
    // 留着。这里改成断言制式之后的形态，值仍然是右贴齐的。
    // 外壳的窄档给值留 21 列，七位数的计数放得下，所以这条兜底
    // 是在它所在之处断言的：面板自己的行生成器。
    use heng::render::Block;
    use heng::render::panel::Panel;
    use ratatui::layout::Rect;

    let facts = facts();
    let block = Block::Usage {
        speaker: heng::events::SpeakerId::System,
        usage: heng::events::Usage {
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
    assert!(
        row(1).ends_with("123.6万 / 10万"),
        "花销按制式写：{:?}",
        row(1)
    );
    assert!(
        row(0).ends_with("123.5万 / 20万"),
        "窗口也按制式写：{:?}",
        row(0)
    );
}

/// 一次针对写操作的询问，按循环从 console 通道发起的样子。
fn ask_permission() -> (
    heng::render::ConsoleRequest,
    tokio::sync::oneshot::Receiver<heng::permissions::Answer>,
) {
    use heng::permissions::PermissionRequest;
    use heng::render::{AskRequest, ConsoleRequest};
    let (tx, rx) = tokio::sync::oneshot::channel::<heng::permissions::Answer>();
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
    use heng::render::RenderEvent;

    let mut state = state();
    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 行")));
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
    state.key(heng::render::Key::BackTab);
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
    // 角是空的，所以从边框那一列往里一格认虚线。
    let box_top = (0..row)
        .rev()
        .find(|y| frame[(left + 1, *y)].symbol() == "┄")
        .expect("框的上边框");
    let box_bottom = (row..24u16)
        .find(|y| frame[(left + 1, *y)].symbol() == "┄")
        .expect("框的下边框");
    assert_eq!(box_top + 1, row, "标题领在问句前面");
    // 在主列里居中，而主列如今就是整屏：上下的余地一样，
    // 差的是整数除法留下的那一行。
    let above = box_top;
    let below = 23 - box_bottom;
    assert!(
        above.abs_diff(below) <= 1,
        "居中：框上面 {above} 行，下面 {below} 行"
    );
}

/// 浮层表面共用一套框架：`CHROME` 的虚线、**四角为空**
/// （`.scratch/tui-visual-language/spec.md` §25）。
#[test]
fn every_overlay_shares_one_chrome_frame_with_empty_corners() {
    // 模态。
    let mut modal = state();
    let (ask, _rx) = ask_permission();
    modal.request(ask);
    let frame = buffer(120, 24, &mut modal);
    let (left, top, _) = overlay_frame(&frame, 120, 24).expect("模态的框");
    assert_eq!(frame[(left, top)].symbol(), " ", "角是空的");
    assert_eq!(frame[(left + 1, top)].symbol(), "┄", "上边框是虚线");
    assert_eq!(frame[(left + 1, top)].fg, palette::CHROME, "框色只剩一个");
    // 按钮行归焦点色（§26）。
    let (button_x, button_y) = (top..24)
        .find_map(|y| {
            (left..120)
                .find(|x| frame[(*x, y)].symbol() == "[")
                .map(|x| (x, y))
        })
        .expect("按钮行在框里");
    assert_eq!(
        frame[(button_x, button_y)].fg,
        palette::ACCENT,
        "按钮行归焦点色"
    );

    // `/` 菜单：同一个框。
    let mut menu = state();
    install_catalog(&mut menu);
    menu.key(Key::Char('/'));
    let _ = screen(120, 24, &mut menu);
    let frame = buffer(120, 24, &mut menu);
    let (left, top, _) = overlay_frame(&frame, 120, 24).expect("菜单的框");
    assert_eq!(frame[(left, top)].symbol(), " ", "角是空的");
    assert_eq!(
        frame[(left + 1, top)].fg,
        palette::CHROME,
        "菜单也是同一个框色"
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
    use heng::permissions::PermissionRequest;
    use heng::render::AskRequest;

    // 这一行回答的那句抱怨：一整墙 shell 不是人
    // 读得下去的东西，所以问句先说这次调用*为的*是什么 —— 用的
    // 是转录里那条折起行的同一句话 —— 然后才是那墙东西。
    let mut state = state();
    let (tx, _rx) = tokio::sync::oneshot::channel::<heng::permissions::Answer>();
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
    assert!(first.contains("┆ 询问 ┆"), "组装时给的那一档：{first}");

    for expected in ["┆ 工作区 ┆", "┆ 自动 ┆", "┆ 只读 ┆", "┆ 询问 ┆"] {
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
    let mut state = TuiState::new(
        SessionFacts {
            switchable: true,
            mode: heng::permissions::Mode::Readonly,
            ..facts()
        },
        std::path::PathBuf::from("/x/heng"),
        None,
    );
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("┆ 只读 ┆"), "{text}");
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

fn kimi() -> heng::events::SpeakerId {
    heng::events::SpeakerId::Debater("kimi".into())
}

fn debater(name: &str) -> heng::events::SpeakerId {
    heng::events::SpeakerId::Debater(name.into())
}

fn executor() -> heng::events::SpeakerId {
    heng::events::SpeakerId::Executor(heng::events::ParticipantId::new("kimi-1"))
}

/// 渲染器看到的一次 `todo` 调用：开始那条带着参数，
/// 而结果落地时块才画出来 —— 所以两条都进。
fn apply_todo(
    state: &mut TuiState,
    id: &str,
    speaker: heng::events::SpeakerId,
    args: serde_json::Value,
) {
    use heng::events::{Event, EventPayload, ToolCallId};
    state.apply(RenderEvent::Logged(Event::new(
        1,
        speaker.clone(),
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new(id),
            tool_name: heng::tools::TODO_TOOL.to_owned(),
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
    // 执行者的调用是一条 Tool 行，只住在轨迹页里（票 10）—— 而左栏的 `todo` 页显示的
    // 是主会话那一份，所以这次调用不会让它长出一个页签。
    open_trace_tab(&mut state, 120, 24);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("派出去的活") || text.contains("todo"),
        "调用本身还在轨迹页里：{text}"
    );
}

#[test]
fn the_sidebar_page_leads_with_the_progress_line_and_keeps_only_the_open_items() {
    // 摘要压在最上面：进度行是页区第一行，正在做的那条排在最前，
    // 做完的不再出现在页上 —— 于是「还有什么没做」在页面上是连续的一块。
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

    let page = sidebar_rows(&mut state, 120, 24);
    let at = |needle: &str| {
        page.iter()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("{needle} 在页面上：{page:#?}"))
    };
    let (progress, busy, waiting) = (at("1/3"), at("正在做"), at("还没开始"));
    assert!(
        progress < busy && busy < waiting,
        "进度行在最上、正在做的排在那一条待做之前：{page:#?}"
    );
    assert!(
        page[busy].starts_with('●'),
        "正在做的那一条带的是 `●`：`{}`",
        page[busy]
    );
    assert!(
        !page.iter().any(|line| line.contains("做完了")),
        "做完的不再出现在页上：{page:#?}"
    );
    assert!(
        !page.iter().any(|line| line.contains('▸')),
        "`▸` 退出这一页（它与转录里可折叠块的记号同形）：{page:#?}"
    );
}

#[test]
fn the_todo_page_shows_what_fits_then_says_how_many_more_there_are() {
    // 这一页不滚动：进度行占掉页区第一行，条目拿剩下的，放不下的
    // 在末行报出来。页区撑满之后（`.scratch/trace-tab/spec.md` §4）这一页高 15 行，
    // 所以要 30 项才逼得出溢出行。
    let items: Vec<(String, &str)> = (0..30)
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
        .position(|line| line.contains("0/30"))
        .unwrap_or_else(|| panic!("进度那一行在页面上：{rows:#?}"));
    // 进度行**在页区第一行**（不是像旧的计数行那样在最后），所以条目在它下面。
    let shown = rows
        .iter()
        .skip(count + 1)
        .filter(|line| (0..30).any(|index| line.contains(&format!("第 {index} 项"))))
        .count();
    let overflow = rows.last().cloned().unwrap_or_default();
    assert_eq!(
        shown, 13,
        "15 行的页区里进度行拿一行、溢出行拿一行，条目十四行里画得下十三行：{rows:#?}"
    );
    assert!(
        overflow.contains("＋17 项"),
        "末行说的是**一条都没露过面**的那几条（这一条露过，所以不算）：{overflow}"
    );
}

#[test]
fn a_page_one_row_tall_degrades_to_the_progress_line_alone() {
    // 走面板而不是走一帧，因为没有终端会向布局要一页
    // 只有一行的页面 —— `SIDEBAR_MIN_PAGE_ROWS` 才是地板 —— 而这条规矩
    // 在那儿仍然得成立，而不是画出一个跑出来的项。
    use heng::render::todo::TodoPanel;
    use heng::render::{Block, ToolBlock, ToolOutcome};
    use ratatui::layout::Rect;

    let mut panel = TodoPanel::default();
    panel.observe(&Block::Tool(Box::new(ToolBlock {
        speaker: kimi(),
        tool_call_id: heng::events::ToolCallId::new("call-1"),
        tool: heng::tools::TODO_TOOL.to_owned(),
        args: todo_args(&[("一件事", "pending")]),
        outcome: Some(ToolOutcome {
            ok: true,
            output: Some("todo: ok".to_owned()),
            error: None,
            duration_ms: 1,
        }),
        timing: Default::default(),
    })));

    let lines = panel.lines(Rect::new(0, 0, 28, 1));
    let text: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
    assert_eq!(text.len(), 1, "{text:?}");
    assert!(text[0].contains("0/1"), "{text:?}");
}

#[test]
fn the_three_tab_labels_fit_at_the_narrow_width() {
    // 调用量┆todo┆文件 是九个格加两个分隔符，所以窄档
    // 仍然放得下全部三个 —— 标签条不该把一个挤出边缘。`轨迹` 不在这一列里：它是主列页签条
    // 上的第二个标签（`.scratch/trace-in-main/spec.md` §2）。
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
            "{}┆{}┆{}",
            wording::TAB_USAGE,
            wording::TAB_TODO,
            wording::TAB_FILES
        )),
        "三个标签按顺序、带着它们的分隔符：{bar}"
    );
    assert!(
        !bar.contains(wording::TAB_TRACE),
        "轨迹不在这条页签条上：{bar}"
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
        .expect("三个标签之后那一行是填充");
    let separator = inside_the_sidebar
        .clone()
        .find(|x| frame[(*x, row)].symbol() == "┆")
        .expect("标签之间有分隔");
    for column in [separator, fill] {
        click(&mut state, column, row);
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
    // 列表之后页面留着，显示进度。
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("一件事", "pending")]),
    );
    let row = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, row, wording::TAB_TODO);
    assert!(
        sidebar_rows(&mut state, 120, 24)
            .join("\n")
            .contains("一件事")
    );

    apply_todo(
        &mut state,
        "call-2",
        kimi(),
        serde_json::json!({ "items": [] }),
    );
    let page = sidebar_rows(&mut state, 120, 24).join("\n");
    assert!(page.contains(wording::TODO_EMPTY), "{page}");
    assert!(!page.contains("一件事"), "而那些项随列表一起没了：{page}");
}

/// 左栏 `todo` 页里的行不是可点开的东西。
///
/// 那一页与转录**共用同一批屏幕行**，可两列说的不是一回事 —— 它没有可点开的条目。
/// 点它以转录的行号去取详情，开着的是另一个视图里的东西；指针落在哪一列正是这件事
/// 的判据。
/// 一行的内容从第几列开始（跳过状态字形与前缀里的空格）。
fn content_column(row: &str) -> usize {
    row.chars()
        .position(|ch| !ch.is_whitespace() && ch != '☐' && ch != '●' && ch != '✓')
        .unwrap_or_else(|| panic!("{row:?} 里没有内容"))
}

#[test]
fn a_long_item_wraps_instead_of_being_cut() {
    // 今天这一页把超宽的一条按页宽截断，长句子直接少半截。
    // 折行之后全文都在，而折出来的续行缩进到**内容列** —— 那是一条的对齐口径，
    // 与「前缀补满」一起让带 id 的与不带 id 的两行内容起于同一列。
    let long = "一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十";
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("短的一条", "pending")]),
    );
    apply_todo(
        &mut state,
        "call-2",
        kimi(),
        todo_args(&[(long, "pending"), ("还有一条", "pending")]),
    );
    let row = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, row, wording::TAB_TODO);

    let page = sidebar_rows(&mut state, 120, 24);
    let first = page
        .iter()
        .position(|line| line.contains("一二三四"))
        .unwrap_or_else(|| panic!("那一条在页面上：{page:#?}"));
    let head = page[first].clone();
    let cont = page[first + 1].clone();
    assert!(!head.contains('…'), "这一页放得下就不截断：{page:#?}");
    assert!(
        cont.contains("七八九十"),
        "后半句在下一行，不是丢掉：{page:#?}"
    );
    assert_eq!(
        content_column(&head),
        content_column(&cont),
        "续行缩进到内容列：{page:#?}"
    );
    assert!(
        page.iter().any(|line| line.contains("还有一条")),
        "后面那一条照旧排着：{page:#?}"
    );
}

/// 左栏 `todo` 页现在是**点得开**的一页：整页点一下就是那份完整清单
/// （`.scratch/todo-page/spec.md` §5、§6）。
#[test]
fn clicking_the_todo_page_opens_the_whole_list() {
    let mut state = state_with_roster(&["kimi"]);
    let items: Vec<(String, &str)> = (0..8)
        .map(|index| {
            let status = if index == 0 { "in_progress" } else { "pending" };
            (format!("第 {index} 项"), status)
        })
        .collect();
    let borrowed: Vec<(&str, &str)> = items
        .iter()
        .map(|(content, status)| (content.as_str(), *status))
        .collect();
    apply_todo(&mut state, "call-1", kimi(), todo_args(&borrowed));

    // 把转录填到左栏页区里去：对话视图里的一条消息正是可点开的那种行。
    for index in 0..12 {
        state.apply(RenderEvent::notice(format!("第 {index} 句话")));
    }
    state.apply(message(1, "被点开的消息", None));

    let rows = screen(120, 24, &mut state);
    let page_top = sidebar_page(&rows);
    let beside = rows
        .iter()
        .position(|row| row.contains("被点开的消息"))
        .expect("转录里那条消息画出来了");
    assert!(
        beside >= page_top,
        "布置：转录里那条消息要落在左栏页区里（第 {beside} 行，页从第 {page_top} 行起）"
    );

    let tab = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, tab, wording::TAB_TODO);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "第 2 项").expect("todo 页上那一项");
    click(&mut state, column, row);

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains(&wording::detail_todo_title(None)),
        "弹窗的标题是这一份：{text}"
    );
    assert!(
        text.contains("第 0 项") && text.contains("第 7 项"),
        "**整份**清单都在，包括页上放不下的那些：{text}"
    );
    assert!(text.contains("esc 关闭"), "页脚点出出口：{text}");

    state.key(Key::Esc);
    let page = sidebar_rows(&mut state, 120, 40).join("\n");
    assert!(page.contains("第 2 项"), "关掉之后左栏仍停在那一页：{page}");
}

#[test]
fn the_done_items_in_the_todo_detail_step_back_one_shade() {
    // 降暗取**样**判断，不钉死某个色值 —— 那一档的归属由语义色板说了算
    // （spec 测试决定「要钉住的行为 → 弹窗」）。
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("还等着的那一条", "pending"), ("做完的那一条", "completed")]),
    );
    let tab = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, tab, wording::TAB_TODO);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "还等着的那一条").expect("那一项在页上");
    click(&mut state, column, row);

    let frame = buffer(120, 40, &mut state);
    // 弹窗在 120 列下宽 116、居中，**盖住左栏** —— 这一帧里那两行只在弹窗里。
    let (open, open_row) = cell_of(&frame, 120, 40, "还等着的那一条").expect("弹窗里有它");
    let (done, done_row) = cell_of(&frame, 120, 40, "做完的那一条").expect("做完的也在弹窗里");
    assert_ne!(
        first_ink_fg(&frame, open, open_row),
        first_ink_fg(&frame, done, done_row),
        "做完的那一项退到另一档，而不只是少个记号"
    );
}

#[test]
fn the_todo_detail_holds_the_list_of_that_moment() {
    // 弹窗是**打开那一刻**的快照：开着的时候模型又提交了新的一份，
    // 开着的那个不动，关掉重开看到的是新的那份。
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("刚开工时的那一条", "pending")]),
    );
    let tab = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, tab, wording::TAB_TODO);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "刚开工时的那一条").expect("那一项在页上");
    click(&mut state, column, row);

    apply_todo(
        &mut state,
        "call-2",
        kimi(),
        todo_args(&[("后来才立的那一条", "pending")]),
    );
    let open = screen(120, 40, &mut state).join("\n");
    assert!(
        open.contains("刚开工时的那一条") && !open.contains("后来才立的那一条"),
        "开着的那个是快照，不跟着模型重写：{open}"
    );

    state.key(Key::Esc);
    let frame = buffer(120, 40, &mut state);
    let (column, row) = cell_of(&frame, 120, 40, "后来才立的那一条").expect("页上已经是新的那份");
    click(&mut state, column, row);
    let reopened = screen(120, 40, &mut state).join("\n");
    assert!(
        reopened.contains("后来才立的那一条") && !reopened.contains("刚开工时的那一条"),
        "重开看到的是新的那份：{reopened}"
    );
}

#[test]
fn a_folded_item_is_copied_back_as_one_line() {
    // 一条折成两行的待办，选中它之后剪贴板里是**一条**：续行接在上一行后面，
    // 而不是各占一行 —— 那正是屏幕文本层认得「这是续行」的那件事。
    let long = "一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十";
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[(long, "pending")]),
    );
    let tab = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, tab, wording::TAB_TODO);

    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "一二三四").expect("那一条在页上");
    state.mouse(press(column, row));
    state.mouse(drag_to(column + 20, row + 1));
    state.mouse(release(column + 20, row + 1));

    // 复制出来的是**一条**：上半截接上下半截，中间没有一个换行（若屏幕文本层没认出那是
    // 续行，这里就会多出一个 `\n`）。下半截前面那几格是缩进，它是那一行的内容。
    let clip = state.take_clipboard().expect("拖选出文本");
    assert_eq!(
        clip,
        heng::render::selection::osc52(
            "一二三四五六七八九十一二三四五六七     八九十一二三四五六七"
        ),
        "折行的续行接在上一行后面，而不是自成一行：{clip:?}"
    );
    assert!(
        !screen(120, 24, &mut state).join("\n").contains("esc 关闭"),
        "拖选不是点开"
    );
}

#[test]
fn the_sidebar_page_says_so_when_there_is_no_list_at_all() {
    // 列表被清空之后，页上是一句实话而不是一块空白 —— 一块空白分不清「没有」与「坏了」。
    // 而页签**仍在**：它是一只闩（`.scratch/todo-and-modes/spec.md` §4）。
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("一件事", "pending")]),
    );
    let tab = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, tab, wording::TAB_TODO);
    assert!(
        sidebar_rows(&mut state, 120, 24)
            .join("\n")
            .contains("一件事")
    );

    apply_todo(
        &mut state,
        "call-2",
        kimi(),
        serde_json::json!({ "items": [] }),
    );
    let page = sidebar_rows(&mut state, 120, 24).join("\n");
    assert!(page.contains(wording::TODO_EMPTY), "页里是一句话：{page}");
    assert!(
        !page.contains("0/0"),
        "而不是一个读不出该做什么的 0/0：{page}"
    );
}

#[test]
fn an_empty_list_gives_no_button_and_opens_nothing() {
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("一件事", "pending")]),
    );
    let tab = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, tab, wording::TAB_TODO);
    apply_todo(
        &mut state,
        "call-2",
        kimi(),
        serde_json::json!({ "items": [] }),
    );

    let page = sidebar_rows(&mut state, 120, 24).join("\n");
    assert!(
        !page.contains(wording::TODO_BUTTON),
        "空列表时不画那一块：{page}"
    );

    let row = tab_bar_row(&mut state, 120, 24) + 4;
    let before = screen(120, 24, &mut state);
    click(&mut state, 30, row);
    assert_eq!(
        screen(120, 24, &mut state),
        before,
        "而点那一块开不出一个空弹窗"
    );
}

#[test]
fn everything_done_leaves_the_progress_line_alone() {
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("做完了", "completed"), ("也做完了", "completed")]),
    );
    let tab = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, tab, wording::TAB_TODO);

    let page = sidebar_rows(&mut state, 120, 24);
    let at = page
        .iter()
        .position(|line| line.contains("2/2"))
        .expect("进度行在页上");
    assert!(
        page[at].contains("完成") && !page.iter().any(|line| line.contains("做完了")),
        "全做完之后页上就那一行进度，条目区留白：{page:#?}"
    );
}

#[test]
fn the_todo_detail_names_who_submitted_that_list() {
    // 讨论会话里各方各有一份，而页上是**最后一个落地者**的那份 —— 弹窗标题要说出那是谁，
    // 否则读的人读到的是别人的计划而不自知。
    let mut state = state_with_roster(&["kimi", "claude"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("kimi 那一轮的那一条", "pending")]),
    );
    apply_todo(
        &mut state,
        "call-2",
        debater("claude"),
        todo_args(&[("claude 那一轮的那一条", "pending")]),
    );

    let tab = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, tab, wording::TAB_TODO);
    let frame = buffer(120, 24, &mut state);
    let (column, row) =
        cell_of(&frame, 120, 24, "claude 那一轮的那一条").expect("页上是最后落地的那一份");
    click(&mut state, column, row);

    let text = screen(120, 40, &mut state).join(
        "
",
    );
    assert!(
        text.contains(&wording::detail_todo_title(Some("claude"))),
        "标题写清是谁提交的那份：{text}"
    );
    assert!(
        !text.contains("kimi 那一轮的那一条"),
        "而打开的是最后落地的那份：{text}"
    );

    // 执行者的那份仍然只住在转录里：左栏连页签都不为它长。
    apply_todo(
        &mut state,
        "call-3",
        executor(),
        todo_args(&[("执行者交回来的那一条", "pending")]),
    );
    state.key(Key::Esc);
    let page = sidebar_rows(&mut state, 120, 40).join("\n");
    assert!(
        !page.contains("执行者交回来的那一条"),
        "执行者的那份不进左栏：{page}"
    );
}

#[test]
fn dragging_across_the_todo_page_selects_instead_of_opening() {
    // 这一页现在整页可点，而拖选仍然是另一条路（按下起拖选、松开才决定是复制还是点）。
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("一件事", "pending")]),
    );
    let tab = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, tab, wording::TAB_TODO);

    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "一件事").expect("那一项在页上");
    state.mouse(press(column, row));
    state.mouse(drag_to(column + 4, row));
    state.mouse(release(column + 4, row));

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("已复制"), "拖选出文本、提示行给回执：{text}");
    assert!(
        !text.contains("esc 关闭"),
        "而这一次拖选没有顺手把整份清单弹出来：{text}"
    );
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
        .map(|command| CatalogEntry::command(command.name, command.description))
        .collect();
    entries.extend([
        CatalogEntry::skill("ask-matt", "不知道用哪个 skill 时问它"),
        CatalogEntry::skill("review", "审查一个变更"),
        // 第三类来源也摆上（票 09）：三色各有一条，才谈得上「看得出是三类」。
        CatalogEntry::template("db:user_report", "按 id 出一份报告"),
    ]);
    state.request(ConsoleRequest::Catalog { entries });
}

/// 给状态装一份文件索引：`@` 的候选来源（票 01 的那个会话级值）。
fn install_files(state: &mut TuiState, paths: &[&str]) {
    state.files_loaded(paths.iter().map(std::path::PathBuf::from).collect());
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
    let text = row_text(frame, row, width);
    // 列号**不是**字节号：这一行里的中文一个字三字节，于是 `find` 的偏移会比它所在的列大出
    // 一截。菜单变宽之后（票 09）描述整句落进这一行，按字节当列号取坐标会直接跑到屏幕外面。
    let column = text_columns(&text[..text.find(needle).unwrap()]) as u16;
    let x = (0..column)
        .rev()
        .find(|c| frame[(*c, row)].symbol() == "┆")
        .expect("菜单的左边框");
    let top = (0..row)
        .rev()
        .find(|y| frame[(x + 1, *y)].symbol() == "┄")
        .expect("菜单的上边框");
    let bottom = (row..height)
        .find(|y| frame[(x + 1, *y)].symbol() == "┄")
        .expect("菜单的下边框");
    let right = (x..width)
        .rev()
        .find(|c| frame[(*c, top)].symbol() == "┄")
        .expect("菜单的右上角");
    (x, top, right - x + 2, bottom - top + 1)
}

/// 草稿打在哪一行：输入框的第一行。
///
/// 它不再靠找提示符定位（2026-10-08 起输入框没有提示符，`.scratch/ui-trim/spec.md`）——
/// 那两列消失之后，「含 `┆` 的第一行」是左栏与主列之间的分隔线所在的第一行，不是输入区。
fn input_row(width: u16, height: u16) -> usize {
    input_corner(width, height).1 as usize
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

    // 而这正是改名的理由：`Tab` 把整条命令补进草稿 —— 后面还跟一个**分隔的空格**，
    // 于是接着打的就是这一段任务的第一个字。
    family.key(Key::Tab);
    let text = screen(120, 24, &mut family).join("\n");
    assert!(text.contains("┆/goal-new"), "Tab 补出整条命令：\n{text}");
    family.key(Key::Char('x'));
    let text = screen(120, 24, &mut family).join("\n");
    assert!(
        text.contains("┆/goal-new x"),
        "补全自带一个分隔空格：\n{text}"
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
        top as usize + height as usize <= input_row(120, 24),
        "菜单坐在输入区上面：{top}+{height} 对 {}",
        input_row(120, 24)
    );
}

/// 菜单里名字第一格的前景色。
fn menu_colour(frame: &Buffer, width: u16, height: u16, needle: &str) -> Color {
    let (row, line) = (0..height)
        .map(|y| (y, row_text(frame, y, width)))
        .find(|(_, line)| line.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} 在菜单里"));
    let byte = line.find(needle).expect("刚刚找到过");
    let column = text_columns(&line[..byte]) as u16;
    frame[(column, row)].fg
}

#[test]
fn the_menu_colours_each_kind_of_name_differently() {
    // 三类来源共用一个菜单，而人得一眼看出哪些是程序自带的、哪些是装进来的
    // （`.scratch/tui-feedback/spec.md` §11）。判据是屏幕上的颜色，不是那个枚举本身。
    let mut state = state();
    install_catalog(&mut state);
    state.key(Key::Char('/'));
    let frame = buffer(120, 24, &mut state);

    assert_eq!(
        menu_colour(&frame, 120, 24, "/undo"),
        palette::TOKEN_COMMAND,
        "命令：与草稿里那个名字同色"
    );
    assert_eq!(
        menu_colour(&frame, 120, 24, "/review"),
        palette::MENU_SKILL,
        "技能"
    );
    assert_eq!(
        menu_colour(&frame, 120, 24, "/db:user_report"),
        palette::MENU_TEMPLATE,
        "MCP 模板"
    );

    // 描述留在正文档：它是一句解释，不参与分类。
    let (row, line) = (0..24)
        .map(|y| (y, row_text(&frame, y, 120)))
        .find(|(_, line)| line.contains("/undo"))
        .expect("菜单里那一行");
    let column = text_columns(&line[..line.find("回滚上一次编辑").unwrap()]) as u16;
    assert_eq!(frame[(column, row)].fg, palette::PLAIN, "描述归正文档");

    // 光标行是**临时光标**：反显，不叠类别色（`tui-visual-language` §27 的另一半照旧）。
    state.key(Key::Down);
    let frame = buffer(120, 24, &mut state);
    assert_eq!(
        menu_colour(&frame, 120, 24, "/undo"),
        palette::PLAIN,
        "高亮那一行不占颜色"
    );
}

#[test]
fn a_long_description_gets_the_wider_menu_before_it_is_cut() {
    // 票 09：上限 72 → 96。描述被截在半句上，菜单就在教人按 `Tab` 去猜后半句。
    let description = "说明".repeat(20);
    let mut state = state();
    state.request(ConsoleRequest::Catalog {
        entries: vec![CatalogEntry::command("demo", description.clone())],
    });
    state.key(Key::Char('/'));
    let frame = buffer(200, 24, &mut state);

    let rows: Vec<String> = (0..24).map(|y| row_text(&frame, y, 200)).collect();
    assert!(
        rows.join("\n").contains(&description),
        "描述整句都在屏幕上：\n{}",
        rows.join("\n")
    );
    let (_, _, width, _) = menu_box(&frame, 200, 24, "/demo");
    assert!(
        width > 74,
        "菜单宽过旧上限（74 = 旧上限 72 加两条边框），实际 {width}"
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
fn the_file_index_is_warmed_once_and_rescanned_after_a_submission() {
    // 预热：进 TUI 就发一次遍历；重扫：每提交一条消息之后再发一次
    // （`.scratch/input-tokens/spec.md` §1）。这里钉的是**什么时候**扫与**不会重发**——
    // 遍历本身由循环用 `spawn_blocking` 去跑，所以状态机不用起任务就能测。
    let mut state = idle();
    assert!(state.take_file_scan(), "进 TUI 就先预热一次");
    assert!(!state.take_file_scan(), "一次遍历还在飞，不重复发");
    state.files_loaded(Vec::new()); // 预热那次落地

    let mut line = awaiting_line(&mut state);
    state.key(Key::Char('x'));
    state.key(Key::Enter);
    assert_eq!(
        line.try_recv().expect("提交送出去了一行"),
        Some("x".to_owned())
    );
    assert!(state.take_file_scan(), "提交之后重扫一次");
    assert!(!state.take_file_scan(), "重扫也只发一次");
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
    assert!(text.contains("┆/ask-matt"), "名字在草稿里：\n{text}");
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
    // 从第一个匹配再往上，绕到最后一个 —— 目录里的末条是那条 MCP 模板（票 09 起它也在
    // fixture 里，好让三类来源各有代表）。
    state.key(Key::Char('/'));
    state.key(Key::Up);
    state.key(Key::Enter);
    assert_eq!(line.try_recv().unwrap(), Some("/db:user_report".to_owned()));
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
    assert!(text.contains("┆/"), "草稿还在那儿：\n{text}");
    assert!(!text.contains("┆ /undo"), "菜单不见了：\n{text}");
    assert!(!text.contains("清空输入"), "什么都没被问：\n{text}");
}

#[test]
fn esc_keeps_its_word_per_token_not_per_query() {
    // spec §2：`Esc` 的「否掉」按**记号**记账 —— 关掉一个不该牵连同一行里另一个记号，
    // 哪怕两者打的是同一个名字。菜单开着时才接受 `Tab`，所以「Tab 补不补全」就是它的
    // 可见回执。
    let mut state = idle();
    // `a` 让两个 `@a` 都兑现（成为 chip，光标才会吸附到记号边界）；`ab` 排在前面，
    // 于是「菜单开着」有一个看得见的回执 —— `Tab` 把它补成 `@ab`。
    install_files(&mut state, &["ab", "a"]);
    for ch in "@a @a".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Esc);
    state.key(Key::Tab);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("┆@a @a"),
        "`Esc` 关掉的那一个记号里 `Tab` 什么都补不了：\n{text}"
    );

    // 把光标移回**前**一个记号里：那是另一个记号，有自己的账，菜单该重开。
    state.key(Key::Left);
    state.key(Key::Left);
    state.key(Key::Tab);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("┆@ab @a"),
        "另一个记号重新开菜单，`Tab` 于是补全了它：\n{text}"
    );
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
        state.apply(RenderEvent::notice(format!("对话第 {index} 行")));
    }
    state.key(Key::Char('/'));
    let _ = screen(120, 24, &mut state);
    let frame = buffer(120, 24, &mut state);
    let (x, top, width, height) = menu_box(&frame, 120, 24, "/undo");
    let (bottom, right) = (top + height - 1, x + width - 1);
    for y in top..=bottom {
        // 四个角是空格，横线是虚线，竖线是虚竖线
        // （`.scratch/tui-visual-language/spec.md` §25）。
        let (left, rightmost) = if y == top || y == bottom {
            (" ", " ")
        } else {
            ("┆", "┆")
        };
        assert_eq!(frame[(x, y)].symbol(), left, "第 {y} 行，左边框");
        assert_eq!(frame[(right, y)].symbol(), rightmost, "第 {y} 行，右边框");
    }
    assert_eq!(frame[(x + 1, top)].symbol(), "┄", "上边框是虚线");
    assert_eq!(frame[(x, top + 1)].symbol(), "┆", "左边框接着往下走");
}

#[test]
fn there_is_no_menu_before_the_loop_has_said_what_exists() {
    let mut state = state();
    state.key(Key::Char('/'));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains("┆ /"), "空清单什么都提供不了：\n{text}");
}

#[test]
fn an_at_opens_a_menu_only_once_a_character_has_been_typed() {
    // 两个前缀共用一套浮层，但候选来源不同：`/` 是循环报上来的命令表，`@` 是文件索引
    // （`.scratch/input-tokens/spec.md` §2、§3）。
    let mut state = idle();
    install_files(&mut state, &["src/", "src/main.rs", "README.md"]);

    // 裸 `@` 不列候选：命令表只有几十条，「看全部」成立，而文件有几千个，它没有意义。
    state.key(Key::Char('@'));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains("┆ @"), "裸 `@` 不画菜单：\n{text}");

    // 打第一个字符才出现，而且只列前缀匹配上的那些 —— 文件与目录都列。
    state.key(Key::Char('s'));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("┆ @src/"), "候选画在菜单里：\n{text}");
    assert!(text.contains("┆ @src/main.rs"), "文件也在：\n{text}");
    assert!(!text.contains("@README.md"), "别的被滤掉了：\n{text}");

    // 索引未就绪时同样什么都不显示 —— 预热让它几乎不可能被看见。
    let mut cold = idle();
    for ch in "@s".chars() {
        cold.key(Key::Char(ch));
    }
    let text = screen(120, 24, &mut cold).join("\n");
    assert!(!text.contains("┆ @"), "还没有索引就没有候选：\n{text}");
}

#[test]
fn the_at_menu_finds_a_file_by_a_later_path_segment() {
    // 打 `cli` 要配得到 `src/cli.rs`（`.scratch/tui-feedback/spec.md` §4）：候选的用处正是
    // 「我不记得它在哪一层」，而前缀匹配恰好要求你记得。
    let mut state = idle();
    install_files(
        &mut state,
        &["src/", "src/main.rs", "src/cli.rs", "docs/cli.md"],
    );
    for ch in "@cli".chars() {
        state.key(Key::Char(ch));
    }
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("┆ @src/cli.rs"), "菜单里有它：\n{text}");
    assert!(!text.contains("@src/main.rs"), "没指到的不列：\n{text}");
}

#[test]
fn accepting_a_directory_keeps_the_menu_open_on_the_next_level() {
    let mut state = idle();
    install_files(
        &mut state,
        &[
            "second.txt",
            "src/",
            "src/main.rs",
            "src/render/",
            "src/render/tui.rs",
        ],
    );
    for ch in "@s".chars() {
        state.key(Key::Char(ch));
    }
    // 索引是排好序的，所以第一个候选是 `second.txt`；`↓` 走到 `src/` 那个目录上，
    // `Tab` 把它补进草稿。
    state.key(Key::Down);
    state.key(Key::Tab);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("┆@src/"), "目录补进了草稿：\n{text}");
    assert!(
        text.contains("┆ @src/main.rs"),
        "菜单接着列这一层：\n{text}"
    );
    assert!(
        text.contains("┆ @src/render/"),
        "子目录也在这一层里：\n{text}"
    );
    assert!(
        !text.contains("@second.txt"),
        "query 换成了新前缀，上一层的东西不再列：\n{text}"
    );

    // 反过来，接受一个**文件**就把菜单关掉。
    let mut file = idle();
    install_files(&mut file, &["src/main.rs", "src/map.md"]);
    for ch in "@src/m".chars() {
        file.key(Key::Char(ch));
    }
    file.key(Key::Tab);
    let text = screen(120, 24, &mut file).join("\n");
    assert!(text.contains("┆@src/main.rs"), "文件补进了草稿：\n{text}");
    assert!(!text.contains("┆ @src/map.md"), "菜单关掉了：\n{text}");

    // 补完之后自带一个**分隔的空格**：接着打就是下一样东西，不会粘在路径上。
    file.key(Key::Char('x'));
    let text = screen(120, 24, &mut file).join("\n");
    assert!(
        text.contains("┆@src/main.rs x"),
        "补全自带一个分隔空格：\n{text}"
    );
}

#[test]
fn enter_in_the_at_menu_accepts_without_sending() {
    let mut state = idle();
    install_files(&mut state, &["src/main.rs"]);
    let mut line = awaiting_line(&mut state);
    for ch in "@s".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert!(line.try_recv().is_err(), "`@` 菜单里回车只接受，不发送");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("┆@src/main.rs"), "路径落进草稿：\n{text}");

    // 再按一次才是发送 —— 插进去的路径只是句子的一部分（spec §2 那处刻意分叉）。
    state.key(Key::Enter);
    assert_eq!(line.try_recv().unwrap(), Some("@src/main.rs".to_owned()));

    // 而 `/` 那一半没有变：回车仍是「插入并发送」。
    let mut slash = idle();
    install_catalog(&mut slash);
    let mut line = awaiting_line(&mut slash);
    for ch in "/rev".chars() {
        slash.key(Key::Char(ch));
    }
    slash.key(Key::Enter);
    assert_eq!(line.try_recv().unwrap(), Some("/review".to_owned()));
}

/// 输入行里 `needle` 那一格的前景色。
///
/// 找的是**输入行**（排版给出的那个矩形）：`/` 菜单会浮在它上面，而菜单里也有同样的文字。
fn draft_colour(frame: &Buffer, width: u16, height: u16, needle: &str) -> Color {
    let row = input_row(width, height) as u16;
    let line = row_text(frame, row, width);
    let byte = line
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} 在输入行上：{line:?}"));
    let column = text_columns(&line[..byte]) as u16;
    frame[(column, row)].fg
}

#[test]
fn only_a_token_that_can_be_honoured_is_coloured() {
    // 判据只有一条：`/` 的记要在命令表里命中、`@` 的记要在索引里命中（spec §4）。上色与
    // chip 共用它，所以这里钉的是那一判据在屏幕上的样子。
    let mut commands = idle();
    install_catalog(&mut commands);
    for ch in "/undo".chars() {
        commands.key(Key::Char(ch));
    }
    let frame = buffer(120, 24, &mut commands);
    assert_eq!(
        draft_colour(&frame, 120, 24, "/undo"),
        TOKEN_COMMAND,
        "命中的命令是命令蓝"
    );

    // 命中不了的保持普通文本色 —— 给 `看 /tmp/x` 里的 `/tmp/x` 上蓝色会教出错误直觉。
    let mut paths = idle();
    install_catalog(&mut paths);
    for ch in "look /tmp/x".chars() {
        paths.key(Key::Char(ch));
    }
    let frame = buffer(120, 24, &mut paths);
    assert_eq!(
        draft_colour(&frame, 120, 24, "/tmp/x"),
        Color::Reset,
        "命令表里没有的记号不着色"
    );

    // `@` 的那一半：索引里有的路径是引用紫，拼错的是普通文本。
    let mut reference = idle();
    install_files(&mut reference, &["src/render/tui.rs"]);
    for ch in "@src/render/tui.rs".chars() {
        reference.key(Key::Char(ch));
    }
    let frame = buffer(120, 24, &mut reference);
    assert_eq!(
        draft_colour(&frame, 120, 24, "@src/render/tui.rs"),
        TOKEN_REFERENCE,
        "命中索引的引用是引用紫"
    );

    let mut typo = idle();
    install_files(&mut typo, &["src/render/tui.rs"]);
    for ch in "@src/reneder/tui.rs".chars() {
        typo.key(Key::Char(ch));
    }
    let frame = buffer(120, 24, &mut typo);
    assert_eq!(
        draft_colour(&frame, 120, 24, "@src/reneder/tui.rs"),
        Color::Reset,
        "索引里没有的路径不着色"
    );
}

#[test]
fn a_pasted_token_is_a_chip_like_a_typed_one() {
    // 粘贴一视同仁：含有效记号的文本粘进来就成一块（spec §4）。一次退格把它整块带走。
    let mut state = idle();
    install_catalog(&mut state);
    state.paste("改 /undo 它");
    state.key(Key::Left);
    state.key(Key::Left);
    state.key(Key::Backspace);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("┆改  它"),
        "一下退格带走整块，且不吞旁边的空白：\n{text}"
    );
}

#[test]
fn every_question_kind_takes_the_overlay() {
    use heng::render::Key;

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
    use heng::permissions::Answer;
    use heng::render::{ConsoleRequest, Key};

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
    use heng::render::{Key, RenderEvent};
    use ratatui::crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};

    let mut state = state();
    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 行")));
    }
    let _ = screen(120, 24, &mut state);
    state.key(Key::PageUp);
    let before = first_notice(&screen(120, 24, &mut state));

    let (ask, _rx) = ask_permission();
    state.request(ask);
    let _ = screen(120, 24, &mut state);

    // 指针落在覆盖层**之外**、且还在转录上：这张 120x24 的屏上覆盖层横跨第 44 到 115 列、
    // 第 8 到 14 行，所以主列靠左那一小条（第 41 到 43 列）是覆盖层外的转录。左栏页矩形里
    // 的滚轮归轨迹页（`.scratch/trace-tab/spec.md` §5），所以这里不取那一档。
    state.mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 42,
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
    use heng::render::Key;

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
    use heng::permissions::Answer;
    use heng::render::Key;

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
// 终端物理光标：`TestBackend` 看不见的那一半
// ---------------------------------------------------------------------------

/// 后端写出来的字节留在一份共享缓冲里，好让 vt100 回放它们。
#[derive(Clone, Default)]
struct Bytes(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Bytes {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("字节缓冲已中毒")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 真的 crossterm 后端，只把尺寸钉死 —— 测试里没有 tty，`size()` 去问终端会失败。
///
/// 除了尺寸，一切都走真后端：这一层要看的正是**真后端会发出的字节**，自己写一个模拟
/// 后端只会让断言断言回自己身上。
struct GhostBackend {
    inner: ratatui::backend::CrosstermBackend<Bytes>,
    size: ratatui::layout::Size,
}

impl ratatui::backend::Backend for GhostBackend {
    type Error = std::io::Error;

    fn draw<'a, I>(&mut self, content: I) -> std::io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        self.inner.draw(content)
    }

    fn hide_cursor(&mut self) -> std::io::Result<()> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> std::io::Result<()> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> std::io::Result<ratatui::layout::Position> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<ratatui::layout::Position>>(
        &mut self,
        position: P,
    ) -> std::io::Result<()> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> std::io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ratatui::backend::ClearType) -> std::io::Result<()> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> std::io::Result<ratatui::layout::Size> {
        Ok(self.size)
    }

    fn window_size(&mut self) -> std::io::Result<ratatui::backend::WindowSize> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// 一帧一帧地画，每画完一帧回答「终端**物理**光标在哪」。
///
/// 与 `frame_and_cursor` 的差别只在最后那个问题上：`TestBackend` 记得的是 ratatui 想不想
/// 露光标，而光标隐藏时终端把它停在**最后写入的那一格** —— ratatui 的 `hide_cursor` 只发
/// `\x1b[?25l`，一个移动光标的字节都不发。输入法的预编辑画的就是这个位置。
struct PhysicalCursor {
    terminal: Terminal<GhostBackend>,
    bytes: Bytes,
    seen: usize,
    screen: vt100::Parser,
}

impl PhysicalCursor {
    fn new(width: u16, height: u16) -> Self {
        let bytes = Bytes::default();
        let backend = GhostBackend {
            inner: ratatui::backend::CrosstermBackend::new(bytes.clone()),
            size: ratatui::layout::Size::new(width, height),
        };
        Self {
            terminal: Terminal::new(backend).expect("幽灵后端"),
            bytes,
            seen: 0,
            screen: vt100::Parser::new(height, width, 0),
        }
    }

    /// 画一帧，返回物理光标的 `(row, col)` 以及此刻光标是不是收着。
    fn draw(&mut self, state: &mut TuiState) -> ((u16, u16), bool) {
        heng::render::paint_frame(&mut self.terminal, state);
        let written = self.bytes.0.lock().expect("字节缓冲已中毒").clone();
        self.screen.process(&written[self.seen..]);
        self.seen = written.len();
        let screen = self.screen.screen();
        (screen.cursor_position(), screen.hide_cursor())
    }
}

#[test]
fn a_hidden_cursor_still_marks_the_input_area_not_the_status_row() {
    // 用户看见的那条路：输入法把预编辑的字形画在**终端物理光标**上。光标藏起来的那一半
    // 里，ratatui 不移动光标，于是终端把它留在最后写入的那一格 —— 空闲时唯一在动的是状态
    // 行那个月相，于是它停在月亮与「就绪」之间，攒拼音的字母就长在那里（症状是
    // `🌑a 就绪`）。
    //
    // 组合期间 heng 一个字符都收不到，所以这条什么都不输入：让它自己空转，脉冲照走。
    use heng::render::layout::plan;
    use ratatui::layout::Rect;

    let (width, height) = (120u16, 24u16);
    let mut probe = PhysicalCursor::new(width, height);
    let mut state = state();
    let input = plan(Rect::new(0, 0, width, height), 1, true).input;
    for frame in 0..16 {
        if frame > 0 {
            state.tick();
        }
        let ((row, col), hidden) = probe.draw(&mut state);
        assert!(
            input.x <= col && col < input.right() && input.y <= row && row < input.bottom(),
            "第 {frame} 帧：物理光标落在输入区之外（{col},{row}，输入区是 {input:?}）——\
             输入法会把预编辑画在那里"
        );
        // 闪还在：八帧一翻，收起的那一半只是把可见性收走，位置不许跟着走。
        let should_hide = !(frame / 8u32).is_multiple_of(2);
        assert_eq!(hidden, should_hide, "第 {frame} 帧的露面/收起与脉冲对不上");
    }
}

// ---------------------------------------------------------------------------
// 折叠：思考行、工具输出与详情覆盖层
// （票 01/02/03）
// ---------------------------------------------------------------------------

/// 一条给讨论者的 `MessageCompleted`。
fn message(seq: u64, text: &str, reasoning: Option<&str>) -> heng::render::RenderEvent {
    use heng::events::{Event, EventPayload, Role, SpeakerId};
    heng::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: text.to_owned(),
            reasoning: reasoning.map(str::to_owned),

            first_token_ms: None,
        },
    ))
}

/// 一条来自讨论者的推理增量。
fn reasoning_delta(text: &str) -> heng::render::RenderEvent {
    use heng::events::SpeakerId;
    heng::render::RenderEvent::Delta {
        speaker: SpeakerId::Debater("kimi".into()),
        kind: heng::render::DeltaKind::Reasoning,
        text: text.to_owned(),
    }
}

/// 一条来自讨论者的正文增量。
fn text_delta(text: &str) -> heng::render::RenderEvent {
    use heng::events::SpeakerId;
    heng::render::RenderEvent::Delta {
        speaker: SpeakerId::Debater("kimi".into()),
        kind: heng::render::DeltaKind::Text,
        text: text.to_owned(),
    }
}

/// 一条来自讨论者的 `ToolCallStarted`。
fn tool_started(
    seq: u64,
    id: &str,
    tool: &str,
    args: serde_json::Value,
) -> heng::render::RenderEvent {
    use heng::events::{Event, EventPayload, SpeakerId, ToolCallId};
    heng::render::RenderEvent::Logged(Event::new(
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
fn permission_asked(seq: u64, id: &str) -> heng::render::RenderEvent {
    use heng::events::{Event, EventPayload, SpeakerId, ToolCallId};
    heng::render::RenderEvent::Logged(Event::new(
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
fn permission_decided(seq: u64) -> heng::render::RenderEvent {
    use heng::events::{Decision, DecisionSource, Event, EventPayload, SpeakerId};
    heng::render::RenderEvent::Logged(Event::new(
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
) -> heng::render::RenderEvent {
    tool_completed_after(seq, id, ok, output, error, 3)
}

/// 同上，而**那次调用自己的墙钟**由调用方给 —— 轨迹页行尾那段耗时就是它
/// （`.scratch/trace-ledger/issues/17-row-numbers-and-anomalies.md`）。
fn tool_completed_after(
    seq: u64,
    id: &str,
    ok: bool,
    output: Option<&str>,
    error: Option<&str>,
    duration_ms: u64,
) -> heng::render::RenderEvent {
    use heng::events::{Event, EventPayload, SpeakerId, ToolCallId};
    heng::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::ToolCallCompleted {
            tool_call_id: ToolCallId::new(id),
            ok,
            output: output.map(str::to_owned),
            error: error.map(str::to_owned),
            duration_ms,
        },
    ))
}

/// 一次左键**按下**。
///
/// 它与 [`release`] 配对才是完整的一次点击：`.scratch/tui-feedback/spec.md` §5 之后，点击动作
/// 发生在**抬起**上 —— 拖选正是这样拦下它的。
fn press(column: u16, row: u16) -> ratatui::crossterm::event::MouseEvent {
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

/// 同上的抬起。
fn release(column: u16, row: u16) -> ratatui::crossterm::event::MouseEvent {
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

/// 一次按住并移动：拖选的中间那一段。
fn drag_to(column: u16, row: u16) -> ratatui::crossterm::event::MouseEvent {
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

/// 点屏幕上一个格子：按下 + 抬起。
fn click(state: &mut TuiState, column: u16, row: u16) {
    state.mouse(press(column, row));
    state.mouse(release(column, row));
}

#[test]
fn a_thinking_segment_opens_in_place_and_settles_in_place() {
    // 从屏幕上看整个状态机：只有推理到了的时候出现 `正在思考`，
    // 正文的第一个增量把同一行落定成 `思考完成`，不多加
    // 一行，而完成的轨迹留着（票 02 §1）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("先看依赖，"));
    state.apply(reasoning_delta("再看测试。"));
    // 思考行只住在轨迹页上（票 10）。
    open_trace_tab(&mut state, 120, 24);

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
    // 思考行是一条过程行，只在轨迹页上（票 10）。
    open_trace_tab(&mut state, 120, 24);
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
    open_trace_tab(&mut state, 120, 24);

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
    open_trace_tab(&mut failed, 120, 24);
    // 轨迹页正文收窄到 38 列之后这一行会折成两行，所以按行尾裁掉空白再拼起来看。
    let page = trace_page(&mut failed, 120, 24);
    let text: String = page.iter().map(|row| row.trim_end()).collect();
    assert!(
        text.contains("[kimi] ▸ 调用 read_file missing.rs 失败"),
        "失败是调用行上的一个后缀：{text}"
    );
}

#[test]
fn a_failed_call_grows_one_row_with_the_first_line_of_its_error() {
    // 错误正文仍然整段在详情里，而它的**首行**上屏：不点开也知道坏在哪
    // （`.scratch/trace-ledger/issues/17-row-numbers-and-anomalies.md` 第 5 条）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-3",
        "read_file",
        serde_json::json!({"path": "missing.rs"}),
    ));
    state.apply(tool_completed(
        2,
        "call-3",
        false,
        None,
        Some("no such file\n后面那些行只在详情里"),
    ));
    open_trace_tab(&mut state, 120, 40);

    let page = trace_page(&mut state, 120, 40);
    let call = page
        .iter()
        .find(|row| row.contains("调用 read_file"))
        .expect("调用行");
    assert!(call.contains("失败"), "末尾那个红 `失败` 还在：{call}");
    // 第二行从内容起点（第 9 列）起排：它没有自己的时刻戳。这块的账目（耗时、用量尾巴）
    // 按既有规矩落在块的**最后一条**行尾，所以这一行后面可以有它们。
    let error = page
        .iter()
        .find(|row| row.contains("no such file"))
        .expect("错误首行");
    assert!(
        error.starts_with(&format!("{}no such file", " ".repeat(9))),
        "错误首行从第 9 列起，只画一行：{error:?}"
    );
    assert!(
        !page.iter().any(|row| row.contains("后面那些行")),
        "第二行往后仍然只在详情里：{page:#?}"
    );
    assert!(
        !page.iter().any(|row| row.contains('✗')),
        "行首不加 `✗` —— 那要让全体行的内容宽降两列：{page:#?}"
    );

    // `ok = false` 而没有错误正文时，那一行不画 —— 没有可说的话就不占一格。
    let mut mute = state_with_roster(&["kimi"]);
    mute.apply(tool_started(
        1,
        "call-4",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    mute.apply(tool_completed(2, "call-4", false, None, None));
    open_trace_tab(&mut mute, 120, 40);
    let page = trace_page(&mut mute, 120, 40);
    assert!(
        page.iter()
            .any(|row| row.contains("调用 bash") && row.contains("失败")),
        "失败那一行还在：{page:#?}"
    );
    assert!(
        !page.iter().any(|row| row.starts_with("         ")),
        "没有错误正文就没有第二行：{page:#?}"
    );
}

#[test]
fn a_healthy_call_row_gains_the_duration_and_nothing_else() {
    // 常规成功行除了耗时一个字也不加（`.scratch/trace-ledger/issues/17-row-numbers-and-anomalies.md`
    // 第 6 条）：账本还得是一份好扫的流。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-1",
        "bash",
        serde_json::json!({"command": "cargo test"}),
    ));
    state.apply(tool_completed_after(
        2,
        "call-1",
        true,
        Some("line one\nline two"),
        None,
        1_500,
    ));
    open_trace_tab(&mut state, 120, 40);

    let page = trace_page(&mut state, 120, 40);
    let call = page
        .iter()
        .find(|row| row.contains("调用 bash"))
        .expect("调用行");
    // 时刻那一格由事件信封给（测试里是「现在」），所以只钉它行尾那一段。
    assert!(
        call.trim_end()
            .ends_with("[kimi] ▸ 调用 bash 运行 cargo test · 1.5 s"),
        "行尾只有耗时那一段：{call:?}"
    );
    assert!(
        !page
            .iter()
            .any(|row| row.contains("无输出") || row.contains("已截断")),
        "成功的行不补结果摘要：{page:#?}"
    );
}

#[test]
fn the_duration_sits_right_before_the_usage_tail() {
    // ADR 0016 把行尾定成用量的家，而耗时是后来者（票 17 第 1、2 条）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(turn_started(1));
    state.apply(tool_started(
        2,
        "call-1",
        "bash",
        serde_json::json!({"command": "cargo test"}),
    ));
    state.apply(tool_completed_after(
        3,
        "call-1",
        true,
        Some("out"),
        None,
        1_500,
    ));
    state.apply(usage(4, 19_502, 1_880, 0, 0));
    open_trace_tab(&mut state, 120, 40);

    let page = trace_page(&mut state, 120, 40);
    let call = page
        .iter()
        .find(|row| row.contains("调用 bash"))
        .expect("调用行");
    assert!(
        call.contains("· 1.5 s in=19502 out=1880"),
        "耗时排在那条用量尾巴前面：{call:?}"
    );
}

#[test]
fn a_row_too_narrow_for_the_duration_keeps_the_usage_and_drops_it() {
    // 放不下时丢的是耗时那一段，用量尾巴完整保留（票 17 第 2 条）。同一个调用在宽终端上
    // 两段都在，窄下来之后先走的是耗时。
    let command = "cargo test --offline --workspace --all-features --no-fail-fast --quiet";
    let mut state = state_with_roster(&["kimi"]);
    state.apply(turn_started(1));
    state.apply(tool_started(
        2,
        "call-1",
        "bash",
        serde_json::json!({"command": command}),
    ));
    state.apply(tool_completed_after(
        3,
        "call-1",
        true,
        Some("out"),
        None,
        1_500,
    ));
    state.apply(usage(4, 19_502, 1_880, 0, 0));
    open_trace_tab(&mut state, 200, 40);

    let wide = trace_page(&mut state, 200, 40).join("\n");
    assert!(
        wide.contains("· 1.5 s in=19502 out=1880"),
        "宽得下时两段都在：{wide}"
    );

    let narrow = trace_page(&mut state, 80, 40).join("\n");
    // 窄到这一行自己都折行时，尾巴也会被窗格折开 —— 那是窗格既有的折行，不是这一段算术
    // 丢的。这条断言钉的是：**先走的是耗时那一段**，用量还在。
    assert!(narrow.contains("in=19"), "用量尾巴留着：{narrow}");
    assert!(
        !narrow.contains("1.5 s"),
        "放不下时丢的是耗时那一段，而且不留 0 也不留占位符：{narrow}"
    );
}

#[test]
fn an_empty_or_truncated_result_says_so_in_one_phrase() {
    // 结果摘要只在异常时给一句（票 17 第 6 条）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-1",
        "bash",
        serde_json::json!({"command": "true"}),
    ));
    state.apply(tool_completed(2, "call-1", true, Some("   \n"), None));
    state.apply(tool_started(
        3,
        "call-2",
        "bash",
        serde_json::json!({"command": "cargo test"}),
    ));
    state.apply(tool_completed_after(
        4,
        "call-2",
        true,
        Some(&format!(
            "头\n{}18842 字符，约 4710 token；全文在 /tmp/call-2.txt]\n尾",
            heng::context::TRUNCATED_MARKER
        )),
        None,
        1_500,
    ));
    // `ok = true` 而**结果字段是空的**（`None`）与一段空串同一档：都是「没有输出」
    // （票 17 第 6 条、票 04 §6 的口径）。
    state.apply(tool_started(
        5,
        "call-3",
        "bash",
        serde_json::json!({"command": "true"}),
    ));
    state.apply(tool_completed(6, "call-3", true, None, None));
    open_trace_tab(&mut state, 120, 40);

    let page = trace_page(&mut state, 120, 40);
    let text = page.join("\n");
    assert_eq!(
        page.iter().filter(|row| row.contains("· 无输出")).count(),
        2,
        "两种没输出的形状都说一句：{text}"
    );
    assert!(
        text.contains("· 已截断 18842 字符"),
        "被截断说一句，且只说流上真有的那个数：{text}"
    );
    assert!(
        !text.contains("约 4710 token"),
        "标记的其余部分仍然只在详情里：{text}"
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
    open_trace_tab(&mut state, 120, 40);

    // 一个很高的终端，好让整段正文都放得下：最短的那个覆盖层会滚动，
    // 那是下一个测试的主题。
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains(TOOL_FACES), "顶上那句标签列着这几面：{text}");
    assert!(text.contains("\"command\""), "打开时落在参数那一面：{text}");

    // 输出那一面要按一下 `Tab`：它就是这次调用产出了什么（票 13 第 3 条）。
    next_face(&mut state);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("alpha"), "整段输出：{text}");
    assert!(text.contains("esc 关闭"), "页脚点出出口：{text}");

    // Esc 关上它，转录回来了。
    state.key(Key::Esc);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains(TOOL_FACES), "Esc 关上覆盖层：{text}");
    assert!(
        text.contains("调用 bash"),
        "它打开时所在的那一行还在：{text}"
    );
}

#[test]
fn the_detail_body_is_wrapped_to_the_real_text_width() {
    // 详情正文排版的宽度是**文本区**的宽度 —— 框宽减掉两列边框与两侧内边距 ——
    // 而不是框宽。按框宽排出来的行，尾部四列会被 `Paragraph` 裁掉，
    // 读的人永远看不到（`.scratch/files-page/spec.md` §6，票 05）。
    let mut state = state_with_roster(&["kimi"]);
    // 120 列下框宽 116、文本区 112：这一行正好落在两者之间。
    let long = format!("{}TAIL", "x".repeat(112));
    state.apply(tool_started(
        1,
        "call-wide",
        "bash",
        serde_json::json!({"command": "cat wide"}),
    ));
    state.apply(tool_completed(2, "call-wide", true, Some(&long), None));
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    // 那一段长正文在「输出」那一面上（打开时落在「参数」）。
    next_face(&mut state);

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("TAIL"),
        "正文的尾巴没有被框边裁掉，而是折到了下一行：{text}"
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

    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    // 那 40 行输出在「输出」那一面上，而它从顶部开始。
    next_face(&mut state);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("输出第 0 行"), "正文从顶部开始：{text}");

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
    let dir = std::env::temp_dir().join(format!("heng-detail-{}", std::process::id()));
    let outputs = dir.join("outputs");
    std::fs::create_dir_all(&outputs).expect("会话的 outputs 目录");
    std::fs::write(
        outputs.join("call-11.txt"),
        "the whole output\nwith a second line the preview never carried",
    )
    .expect("落盘的那个文件");

    let mut state = TuiState::new(
        SessionFacts {
            switchable: true,
            session_id: "01J8ZQ4K7M".to_owned(),
            session_dir: dir.display().to_string(),
            model: "claude-sonnet-4-5".to_owned(),
            context_window: 200_000,
            // 会话被组装时所处的模式：状态行里模式那一栏。想测另一档的
            // 测试在自己的 facts 里覆盖它。
            mode: heng::permissions::Mode::Ask,
            budget_limit: Some(100_000),
            number_style: heng::render::wording::NumberStyle::Cn,
            file_viewer: FileViewerSettings::default(),
            diff_viewer: DiffViewerSettings::default(),
            speaker_order: vec!["kimi".to_owned()],
        },
        std::path::PathBuf::from("/x/heng"),
        None,
    );
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

    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    // 落盘全文在「输出」那一面上。
    next_face(&mut state);
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

    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    next_face(&mut state);
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
    open_trace_tab(&mut state, 120, 40);

    // 在问句盖住窗格之前，先找到调用行那一行。
    let row = row_of(&mut state, 120, 40, "调用 bash").expect("调用行画出来了");
    state.request(ask_permission().0);
    click(&mut state, 20, row);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains(TOOL_FACES), "详情没有在问句上面打开：{text}");
    assert!(text.contains("权限询问"), "而屏幕上还是那个问句：{text}");
}

/// 注入名册是给定讨论者名字的状态，转录里的名字
/// 正是靠它上色（票 07 §1）。
fn state_with_roster(names: &[&str]) -> TuiState {
    TuiState::new(
        SessionFacts {
            switchable: true,
            speaker_order: names.iter().map(|name| (*name).to_owned()).collect(),
            ..facts()
        },
        std::path::PathBuf::from("/x/heng"),
        None,
    )
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

/// 点某句话画在的那一行 —— 就点它画着的那个格子。
///
/// 先渲染帧，因为只有画出来的东西才点得到，而
/// 那句话是在同一帧里查的（票 04 §1）。**列不是无所谓的**：点击按指针落在哪个
/// 窗格里分派（`.scratch/trace-tab/spec.md` §1），所以点在文字上才是真实的点击 ——
/// 一个固定在左栏列上的坐标会点到左栏的页上，而不是这句话身上。
fn click_row(state: &mut TuiState, width: u16, height: u16, needle: &str) {
    click_text(state, width, height, needle);
}

/// 输入区第一格在屏幕上的位置。
///
/// 输入框不再有提示符（2026-10-08，`.scratch/ui-trim/spec.md`），所以测试不能再靠找那个字形
/// 来定位它：位置问排版，而排版与画出来的是同一个来源。草稿按一行算（这里要的只是那个矩形）。
fn input_corner(width: u16, height: u16) -> (u16, u16) {
    use heng::render::layout::plan;
    use ratatui::layout::Rect;
    let regions = plan(Rect::new(0, 0, width, height), 1, true);
    (regions.input.x, regions.input.y)
}

// ---------------------------------------------------------------------------
// 两种问句形状上的鼠标作答（票 04）
// ---------------------------------------------------------------------------

/// 某句话起始的那个屏幕格，自上而下搜。
///
/// 匹配是拿一行**按终端读它的方式**读出来的 —— 一个宽字素
/// 前进两列、它后面那一格跳过 —— 所以匹配报出来的列
/// 是屏幕列，而鼠标事件带的正是它。
/// 那一行**第一个画出东西的格子**的前景色（从 `column` 往左找，跳过前缀的空格）。
fn first_ink_fg(frame: &Buffer, column: u16, row: u16) -> ratatui::style::Color {
    (0..=column)
        .rev()
        .find_map(|x| {
            let cell = &frame[(x, row)];
            (cell.symbol() != " ").then_some(cell.fg)
        })
        .unwrap_or(ratatui::style::Color::Reset)
}

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
    click(state, column, row);
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
    click(state, column, row);
}

#[test]
fn a_permission_question_is_answered_by_clicking_a_button() {
    for (label, expected) in [
        ("[y] 允许", heng::permissions::Answer::Allow),
        ("[a] 总是允许", heng::permissions::Answer::AlwaysAllow),
        ("[n] 拒绝", heng::permissions::Answer::Deny),
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
        click(&mut state, column, row);
    }
    assert!(answer.try_recv().is_err(), "点在按钮之外不作答");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("权限询问"), "而问句还在：{text}");
}

#[test]
fn a_raised_gesture_replaces_the_way_out_and_keeps_the_hints() {
    // 举手换掉的是**出口那一段**，不是往后追加（`.scratch/exit-gesture/spec.md` §2）：
    // 宽终端里别的键位提示照旧在，而出口那一截换成了那句催促。
    let (mut idle, _line) = {
        let mut state = state_with_roster(&["kimi"]);
        let (reply, line) = tokio::sync::oneshot::channel();
        state.request(ConsoleRequest::Prompt { reply });
        (state, line)
    };
    let before = screen(120, 24, &mut idle).join("\n");
    assert!(!before.contains("再按一次"), "没举手时没有这句：{before}");

    idle.key(Key::CtrlC);
    let after = screen(120, 24, &mut idle).join("\n");
    assert!(
        after.contains("再按一次 ctrl-c/ctrl-d 退出"),
        "举手之后出口段换了：{after}"
    );
    assert!(after.contains("enter 发送"), "别的键位提示照旧在：{after}");
    assert!(
        !after.contains("· ctrl-c/ctrl-d 退出"),
        "旧的出口段是被换掉、不是被追加：{after}"
    );

    // 忙碌那一档把两件事都说出来：回合停了、再按会退出。
    let mut busy = state_with_roster(&["kimi"]);
    busy.request(ConsoleRequest::RunState { running: true });
    busy.key(Key::CtrlC);
    let busy_text = screen(120, 24, &mut busy).join("\n");
    assert!(
        busy_text.contains("已取消 · 再按一次 ctrl-c 退出"),
        "忙碌的举手文案：{busy_text}"
    );
    assert!(
        busy_text.contains("esc 取消"),
        "键位提示照旧在：{busy_text}"
    );
}

/// 一个屏幕上摆着 `question` 的问卷，以及它的答案接收端。
fn questionnaire_state(
    question: heng::questions::UserQuestion,
) -> (
    TuiState,
    tokio::sync::oneshot::Receiver<Result<heng::questions::UserAnswers, String>>,
) {
    use heng::render::QuestionnaireRequest;
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
    use heng::questions::{Choice, UserQuestion};
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
fn the_main_tabs_still_answer_clicks_while_a_questionnaire_is_up() {
    // 问卷占的是**底部输入区**，不是整个指针：主列上方的页签条、转录与左栏照旧归它们本来管的
    // 人。以前它把每一次点击都吃掉，于是问卷一立起来，「轨迹」就点不动了。
    use heng::questions::{Choice, UserQuestion};
    let (mut state, _answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![Choice {
            label: "甲".to_owned(),
            description: None,
        }],
    });
    // 注入行只住在轨迹页，所以它是「现在这一页是哪一页」的判据。
    state.apply(injected(1, "注入的正文"));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains("[上下文注入"), "对话页不画注入行：{text}");

    open_trace_tab(&mut state, 120, 24);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("[上下文注入"),
        "点「轨迹」之后它在那儿：{text}"
    );
    assert!(text.contains("1. 甲"), "切页不动问卷，它仍在底部：{text}");
}

#[test]
fn a_sidebar_tab_click_while_a_questionnaire_is_up_switches_but_keeps_the_keyboard() {
    // 切页可以，键盘不跟着走：那一页平时会收走键盘（`.scratch/files-page/spec.md` §5），而问
    // 卷还立着，`j` 仍该是问卷的键（`.scratch/questionnaire-keys/spec.md` §7 的补记）。
    use heng::questions::{Choice, UserQuestion};
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

    let frame = buffer(120, 24, &mut state);
    let (column, row) = tab_cell(&frame, 120, 24, wording::TAB_FILES);
    click(&mut state, column, row);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains(wording::files_loading()),
        "左栏切到文件页了：{text}"
    );

    state.key(Key::Char('j'));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("> ○ 2. 乙"), "键盘还在问卷里：{text}");
}

#[test]
fn a_multi_select_option_only_toggles_when_clicked() {
    use heng::questions::{Choice, UserQuestion};
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
    use heng::questions::{Choice, UserQuestion};
    let (mut state, mut answers) = {
        use heng::render::QuestionnaireRequest;
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
fn clicking_next_marks_the_question_skipped_like_the_arrow_key() {
    // `.scratch/questionnaire-keys/spec.md` §11：`→` 与页脚「下一题 →」是**同一个手势**，
    // 所以点击离开一道没作答的题时，那一笔也要记上。
    use heng::questions::{Choice, UserQuestion};
    use heng::render::QuestionnaireRequest;

    let mut state = state_with_roster(&["kimi"]);
    let (reply, _answers) = tokio::sync::oneshot::channel();
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

    click_in_row(&mut state, 120, 24, 23, "下一题 →");
    // 翻回来：页脚该说这一题交回去的是什么。
    click_in_row(&mut state, 120, 24, 23, "← 上一题");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("1 / 2 已跳过"),
        "点击与 `→` 记的是同一笔：{text}"
    );
}

#[test]
fn clicking_the_custom_row_hands_it_the_cursor_and_paging_takes_it_back() {
    use heng::questions::{Choice, UserQuestion};
    let (mut state, _answers) = {
        use heng::render::QuestionnaireRequest;
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
fn the_questionnaire_footer_carries_the_keys_and_the_gesture_receipt() {
    // 页脚最后那一段是键位提示；举手期间它被回执**替换**（不是追加）
    // —— `.scratch/questionnaire-keys/spec.md` §6。
    use heng::questions::{Choice, UserQuestion};
    let (mut state, mut answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![Choice {
            label: "甲".to_owned(),
            description: None,
        }],
    });

    let text = screen(130, 24, &mut state).join("\n");
    assert!(
        text.contains("j/k 移动"),
        "键位提示跟在进度与按钮后面：{text}"
    );
    assert!(text.contains("esc 退出询问"), "{text}");

    // 举 `Esc` 的手：那一段换成回执。
    state.key(Key::Esc);
    let text = screen(130, 24, &mut state).join("\n");
    assert!(text.contains("再按一次 esc 退出询问"), "{text}");
    assert!(!text.contains("j/k 移动"), "回执替换了键位提示：{text}");

    // 第二下真的退出这次询问：发送端被丢掉，而不是作答。
    state.key(Key::Esc);
    assert!(answers.try_recv().is_err(), "退出询问 = 丢下发送端");
}

#[test]
fn the_questionnaire_footer_shows_the_exit_receipt_the_hint_line_cannot() {
    // 问卷期间提示行整行被页脚替换，所以 `Ctrl-C` 举起的那只手只有落在这里才看得见
    // —— `.scratch/questionnaire-keys/spec.md` §6。
    use heng::questions::{Choice, UserQuestion};
    let (mut state, _answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![Choice {
            label: "甲".to_owned(),
            description: None,
        }],
    });
    state.request(ConsoleRequest::RunState { running: true });

    state.key(Key::CtrlC);
    let text = screen(130, 24, &mut state).join("\n");
    assert!(
        text.contains("已取消 · 再按一次 ctrl-c 退出"),
        "回执落在页脚：{text}"
    );
}

#[test]
fn the_questionnaire_footer_drops_the_teaching_hint_before_the_buttons() {
    // 窄到放不下整段时，先丢教学性的那部分，进度与按钮保住
    // （`.scratch/questionnaire-keys/spec.md` §6）。
    use heng::questions::{Choice, UserQuestion};
    let (mut state, _answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![Choice {
            label: "甲".to_owned(),
            description: None,
        }],
    });

    let wide = screen(130, 24, &mut state).join("\n");
    assert!(
        wide.contains("ctrl-n/ctrl-p"),
        "宽终端里有 Emacs 别名：{wide}"
    );

    // 让这一题有着落，页脚才画得出那个「提交」按钮。
    state.key(Key::Char(' '));

    let narrow = screen(40, 24, &mut state).join("\n");
    assert!(narrow.contains("1 / 1"), "进度永远保：{narrow}");
    assert!(narrow.contains("提交"), "能点的按钮永远保：{narrow}");
    assert!(
        narrow.contains("esc 退出询问"),
        "窄到只剩出口那一句：{narrow}"
    );
    assert!(
        !narrow.contains("j/k 移动"),
        "先丢的是教学性的键位提示：{narrow}"
    );
}

#[test]
fn the_questionnaire_footer_says_what_enter_would_do_and_what_was_skipped() {
    // `.scratch/questionnaire-keys/spec.md` §11：页脚那段提示跟着**回车实际会做什么**变，
    // 而当前题被交回去时，进度后面跟一句 `已跳过`。
    use heng::questions::{Choice, UserQuestion};
    use heng::render::QuestionnaireRequest;

    let question = |id: &str| UserQuestion {
        id: id.to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![Choice {
            label: "甲".to_owned(),
            description: None,
        }],
    };

    let mut state = state();
    let (reply, _answers) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Questionnaire(QuestionnaireRequest {
        questions: vec![question("q1"), question("q2")],
        reply,
    }));

    let text = screen(130, 24, &mut state).join("\n");
    assert!(
        text.contains("回车 下一题"),
        "后面还有题，回车是往前走：{text}"
    );
    assert!(!text.contains("已跳过"), "还没人被跳过：{text}");

    // 回车把第一题记成跳过、走到第二题；第二题成了末题，回车于是变成「提交」。
    state.key(Key::Enter);
    let text = screen(130, 24, &mut state).join("\n");
    assert!(text.contains("2 / 2"), "{text}");
    assert!(text.contains("回车 提交"), "每题都有着落了：{text}");

    // 翻回去看得见那一笔 —— 它属于进度段，不参与提示的降级。
    state.key(Key::Left);
    let text = screen(130, 24, &mut state).join("\n");
    assert!(text.contains("1 / 2 已跳过"), "进度后面跟着那一笔：{text}");

    let narrow = screen(40, 24, &mut state).join("\n");
    assert!(
        narrow.contains("1 / 2 已跳过"),
        "窄到只剩出口，那一笔也还在：{narrow}"
    );
}

#[test]
fn every_row_of_a_wrapped_option_answers_that_option() {
    // 折行不改变点击：长选项的**第二行**也命中同一个选项
    // （`.scratch/questionnaire-keys/spec.md` §7）。
    use heng::questions::{Choice, UserQuestion};
    let long = "x".repeat(200);
    let (mut state, mut answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![
            Choice {
                label: "短的".to_owned(),
                description: None,
            },
            Choice {
                label: long.clone(),
                description: None,
            },
        ],
    });

    let rows = screen(120, 24, &mut state);
    let wrapped: Vec<u16> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.contains("xxxx"))
        .map(|(y, _)| y as u16)
        .collect();
    assert!(
        wrapped.len() >= 2,
        "长选项折成了多行：\n{}",
        rows.join("\n")
    );

    click_in_row(&mut state, 120, 24, wrapped[1], "xxxx");
    state.key(Key::Enter);
    let answers = answers.try_recv().expect("提交了").expect("作答了");
    assert_eq!(
        answers.answers[0].selected,
        vec![long],
        "折行之后的每一行都还是那个选项"
    );
}

#[test]
fn only_the_zone_that_holds_the_keyboard_is_lit() {
    // 同一时刻只有一个视觉焦点：选项区拿着键盘时高亮反显；输入区拿着键盘时它降暗，
    // 而自由文本那一行提亮（`.scratch/questionnaire-keys/spec.md` §8）。
    use heng::questions::{Choice, UserQuestion};
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

    let frame = buffer(120, 24, &mut state);
    let (option, row) = find_cell(&frame, 120, 24, "甲").expect("选项在屏幕上");
    assert!(
        frame[(option, row)].modifier.contains(Modifier::REVERSED),
        "选项区拿着键盘时高亮反显"
    );

    // 越过末项进输入区：高亮留在「乙」上，但它不再反显 —— 全屏唯一的 `REVERSED` 是键盘
    // 所在，而键盘在输入区（`.scratch/tui-visual-language/spec.md` §29）。`DIM` 退场。
    state.key(Key::Char('j'));
    state.key(Key::Char('j'));
    let frame = buffer(120, 24, &mut state);
    let (option, row) = find_cell(&frame, 120, 24, "乙").expect("第二个选项在屏幕上");
    assert!(
        !frame[(option, row)].modifier.contains(Modifier::REVERSED),
        "输入区拿着键盘时高亮不再反显"
    );
    assert!(
        !frame[(option, row)].modifier.contains(Modifier::DIM),
        "`DIM` 不参与层级"
    );
    let (label, row) = find_cell(&frame, 120, 24, "自").expect("自由文本那一行在屏幕上");
    assert!(
        frame[(label, row)].modifier.contains(Modifier::REVERSED),
        "键盘在输入区，所以反显在自定义那一行"
    );
}

/// 一帧问卷里**只有一行**反显：键盘在哪儿就只有哪儿（`.scratch/tui-visual-language/spec.md`
/// §29）。
#[test]
fn a_questionnaire_frame_has_exactly_one_reversed_row() {
    use heng::questions::{Choice, UserQuestion};

    let reversed_rows = |state: &mut TuiState| -> usize {
        let frame = buffer(120, 24, state);
        (0..24)
            .filter(|y| (0..120).any(|x| frame[(x, *y)].modifier.contains(Modifier::REVERSED)))
            .count()
    };

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
    let _ = screen(120, 24, &mut state);
    assert_eq!(reversed_rows(&mut state), 1, "选项区聚焦时只有一行反显");

    // 越过末项进输入区：反显整块挪过去，而不是多出第二处。
    state.key(Key::Char('j'));
    state.key(Key::Char('j'));
    assert_eq!(reversed_rows(&mut state), 1, "输入区聚焦时也只有一行反显");
}

#[test]
fn the_wheel_moves_the_questionnaire_highlight() {
    use heng::questions::{Choice, UserQuestion};
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
    use heng::questions::{Choice, UserQuestion};
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
        state.apply(heng::render::RenderEvent::notice(format!("第 {index} 行")));
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
    open_trace_tab(&mut state, 120, 40);
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
    open_trace_tab(&mut state, 120, 40);
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
        state.apply(heng::render::RenderEvent::notice(format!("第 {index} 行")));
    }
    // 打开方是**轨迹页**（今天唯一有入口的一页）：这条测试说的正是覆盖层把它背后的那一页冻住
    // （票 13 把冻结收窄到打开方之后仍然成立）。
    state.apply(message(41, "被点开的消息", None));
    open_trace_tab(&mut state, 120, 24);

    let _ = screen(120, 24, &mut state);
    click_row(&mut state, 120, 24, "被点开的消息");
    let before = screen(120, 24, &mut state);
    assert!(before.join("\n").contains(MESSAGE_FACES), "覆盖层起来了");
    let frozen = transcript_text(&buffer(120, 24, &mut state), transcript_rows(&before));

    // 覆盖层开着的时候有新输出到来。
    for index in 40..60 {
        state.apply(heng::render::RenderEvent::notice(format!("第 {index} 行")));
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
    // 工具行只住在轨迹页里（票 10），所以先把左栏切过去。
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains(TOOL_FACES), "覆盖层打开了：{text}");

    state.request(ask_permission().0);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains(TOOL_FACES), "覆盖层为问句让了位：{text}");
    assert!(text.contains("权限询问"), "而问句起来了：{text}");
}

#[test]
fn the_questionnaire_footer_buttons_hit_where_they_are_drawn() {
    // 页脚按钮之间的空隙既被数也被画，所以
    // 点击落进的区域就是那些字形所在的按钮 —— 这里钉的是
    // 那条 bug：空隙只数不画，会把每个区域都挪出去三列
    // （票 04 §7）。
    use heng::questions::{Choice, UserQuestion};
    let (mut state, mut answers) = {
        use heng::render::QuestionnaireRequest;
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
    open_trace_tab(&mut state, 120, 40);

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
    open_trace_tab(&mut state, 120, 40);
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
fn ctrl_d_closes_the_detail_overlay() {
    // 「覆盖层忽略其它所有键」的唯一例外：`Ctrl-D` 关上它，而不是参与退出手势
    // （`.scratch/exit-gesture/spec.md` §1）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-22",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-22", true, Some("body"), None));
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains(TOOL_FACES), "覆盖层打开了：{text}");

    state.key(Key::CtrlD);
    assert!(!state.should_quit(), "关上不是退出");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains(TOOL_FACES), "覆盖层关上了：{text}");
}

#[test]
fn a_tool_body_over_the_reading_limit_is_cut_and_says_so() {
    // 落盘文件自己没有上限，所以裁的是读者那份，
    // 而正文说出这件事（票 02 §4）。
    let dir = std::env::temp_dir().join(format!("heng-detail-big-{}", std::process::id()));
    let outputs = dir.join("outputs");
    std::fs::create_dir_all(&outputs).expect("会话的 outputs 目录");
    let big = "x".repeat(200_001);
    std::fs::write(outputs.join("call-23.txt"), &big).expect("落盘的那个文件");

    let mut state = TuiState::new(
        SessionFacts {
            switchable: true,
            session_id: "01J8ZQ4K7M".to_owned(),
            session_dir: dir.display().to_string(),
            model: "claude-sonnet-4-5".to_owned(),
            context_window: 200_000,
            // 会话被组装时所处的模式：状态行里模式那一栏。想测另一档的
            // 测试在自己的 facts 里覆盖它。
            mode: heng::permissions::Mode::Ask,
            budget_limit: Some(100_000),
            number_style: heng::render::wording::NumberStyle::Cn,
            file_viewer: FileViewerSettings::default(),
            diff_viewer: DiffViewerSettings::default(),
            speaker_order: vec!["kimi".to_owned()],
        },
        std::path::PathBuf::from("/x/heng"),
        None,
    );
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
    open_trace_tab(&mut state, 120, 40);
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
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");

    state.key(Key::CtrlC);
    assert!(!state.should_quit(), "Ctrl-C 不会从覆盖层里退出");
    assert!(state.take_events().is_empty(), "也不会取消任何东西");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains(TOOL_FACES), "覆盖层还开着：{text}");

    // `Ctrl-Z` 是那个「别的全部忽略」的唯一例外：挂起是终端层手势，不属于任何一个视图的
    // 键位表，所以覆盖层立着也拦不住它（`.scratch/suspend-gesture/spec.md` §2）。
    state.key(Key::CtrlZ);
    assert!(state.take_suspend_request(), "覆盖层立着也能挂起");
    assert!(
        state.take_events().is_empty(),
        "挂起不是取消，也不推任何手势"
    );

    // 可打印的键也到不了草稿：覆盖层的键就是
    // 覆盖层的，它后面的编辑器没在被打字。
    state.key(Key::Char('x'));
    // 详情覆盖层现在屏幕居中（`.scratch/tui-chrome/spec.md` §4），
    // 120x40 下它横跨第 2 到 117 列，把输入区也压在下面 ——
    // 所以草稿要等覆盖层关掉之后才看得见。
    state.key(Key::Esc);
    let rows = screen(120, 40, &mut state);
    let input = input_corner(120, 40).1 as usize;
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
    open_trace_tab(&mut state, 120, 24);

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
    open_trace_tab(&mut asked, 120, 24);
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
    open_trace_tab(&mut state, 120, 40);
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
    open_trace_tab(&mut wide, 200, 40);
    click_row(&mut wide, 200, 40, "调用 bash");
    let frame = buffer(200, 40, &mut wide);
    let detail = overlay_width(&frame, 200, 40).expect("覆盖层的上边框");
    assert_eq!(detail, 135, "而在宽终端上压着它的是那个上限");
}

/// 一个浮层画出来的框：左上角那一格（**角是空的**，`.scratch/tui-visual-language/spec.md`
/// §25）、它的行，以及框的宽度。
///
/// 找的不是角，而是一条**两端各有一格空角的虚线** —— 页签条与输入区那两条线一直铺到左栏，
/// 所以「两端是空角」这条判据把浮层与它们分开。
fn overlay_frame(frame: &Buffer, width: u16, height: u16) -> Option<(u16, u16, u16)> {
    for y in 0..height {
        // 这一行上所有连续虚线；左栏的页签条也画虚线，所以必须挑那一段两端是空角的。
        let mut runs: Vec<(u16, u16)> = Vec::new();
        let mut open: Option<(u16, u16)> = None;
        for x in 1..width.saturating_sub(1) {
            if frame[(x, y)].symbol() == "┄" {
                open = Some(match open {
                    Some((first, _)) => (first, x),
                    None => (x, x),
                });
            } else if let Some(run) = open.take() {
                runs.push(run);
            }
        }
        if let Some(run) = open {
            runs.push(run);
        }
        for (first, last) in runs {
            if first > 1
                && frame[(first - 1, y)].symbol() == " "
                && frame[(last + 1, y)].symbol() == " "
            {
                return Some((first - 1, y, last - first + 3));
            }
        }
    }
    None
}

/// 一个浮动框画出来的宽度，从它上边框所在的那一行读出来。
fn overlay_width(frame: &Buffer, width: u16, height: u16) -> Option<u16> {
    overlay_frame(frame, width, height).map(|(_, _, box_width)| box_width)
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
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains(TOOL_FACES), "覆盖层打开了：{text}");

    // 外面：覆盖层在屏幕上留出的上边距（第 0 行）——整圈框都是关闭目标，
    // 而这里的覆盖层压住了它下面几乎每一行（120×40 下它占 2..38）。
    click(&mut state, MAIN_LEFT_AT_120, 0);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains(TOOL_FACES), "点在转录上把它关上了：{text}");

    // 里面：什么都不发生，因为覆盖层自己没有按钮 —— 而且
    // 这包括覆盖层盖住它时那一行原本所在的屏幕行。老的
    // 「再点同一行」**不是**关上被盖住的行的办法；可靠的
    // 出口是 `Esc`、`Ctrl-D`，以及在别处点一下（票 02 §4，2026-09-23 修正）。
    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains(TOOL_FACES), "重新打开了：{text}");
    click(&mut state, 60, 12);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains(TOOL_FACES), "在里面点一下它照样开着：{text}");
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
        state.apply(heng::render::RenderEvent::notice(format!("第 {index} 行")));
    }
    // 先来一帧，然后是中间夹着一帧的实时思考段。左栏先切到轨迹页：思考行住在那里
    // （票 10），而转录那一半的窗口不受它影响。
    open_trace_tab(&mut state, 120, 24);
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
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    next_face(&mut state);

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
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    next_face(&mut state);

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
    open_trace_tab(&mut state, 120, 40);
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
    open_trace_tab(&mut asked, 120, 40);
    let text = screen(120, 40, &mut asked).join("\n");
    // 轨迹页只有 40 列，所以这一行会折成两行 —— 断言两段都在，而不是那一整句。
    assert!(
        text.contains("调用 ask_user_question 下一步") && text.contains("失败"),
        "问题自己的摘要描述了这次调用：{text}"
    );

    // 参数还在详情里：打开就落在「参数」那一面。
    click_row(&mut asked, 120, 40, "调用 ask_user_question");
    let text = screen(120, 40, &mut asked).join("\n");
    assert!(text.contains(TOOL_FACES), "参数那一面：{text}");
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
    open_trace_tab(&mut state, 120, 40);
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
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    let frame = buffer(120, 40, &mut state);

    // 覆盖层的左上角：角是空的，所以认那条两端各一格空角的虚线。
    let (x, y, _) = overlay_frame(&frame, 120, 40).expect("覆盖层的左上角");
    assert_eq!(
        frame[(x + 1, y)].fg,
        palette::CHROME,
        "边框归框架（`CHROME`）"
    );
    assert_eq!(
        frame[(x + 2, y + 2)].fg,
        Color::LightCyan,
        "而「这是谁的行」退到标题行上，仍穿发言者的颜色"
    );
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
    overlay_frame(frame, width, height).map(|(x, _, box_width)| (x, box_width))
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
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    // 那 200 行在「输出」那一面；页脚数的是**当前这一面**的那一对数
    // （票 13 第 8 条）。
    next_face(&mut state);

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
fn ask_bash(command: &str) -> heng::render::ConsoleRequest {
    use heng::permissions::PermissionRequest;
    use heng::render::AskRequest;
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
    // 折起行住在轨迹页里（票 10），所以先把左栏切过去；问句随后立在主列上。
    open_trace_tab(&mut state, 120, 40);
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
        column += heng::render::width::char_columns(ch);
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
    // 名字独占一行之后，表格从**第 0 列**起画，表头与数据行自然在同一列
    // （2026-10-05 的排版修订把前缀移出了正文那一行）。
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
    // 而表头上一行确实是 `[kimi]`。
    assert!(
        rows[head - 1].contains("[kimi]"),
        "名字在表头上一行：{:?}",
        &rows[head - 1..head + 1]
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

// ---------------------------------------------------------------------------
// 轨迹视图（`.scratch/trace-tab/spec.md` §1、§3、§5；票 09）
// ---------------------------------------------------------------------------

/// 指定坐标上的一格滚轮 —— 两个视口按指针位置分派，所以坐标是这件事的一部分。
fn wheel_at(column: u16, row: u16, up: bool) -> ratatui::crossterm::event::MouseEvent {
    use ratatui::crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    MouseEvent {
        kind: if up {
            MouseEventKind::ScrollUp
        } else {
            MouseEventKind::ScrollDown
        },
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

/// 主列轨迹页里的那些行：页签条、状态行、输入区与提示行都不算，左栏不算
/// （`.scratch/trace-in-main/spec.md` §1、§3）。
fn trace_page_with_blanks(state: &mut TuiState, width: u16, height: u16) -> Vec<String> {
    let rows = conversation_rows(state, width, height);
    let height_of_page = transcript_rows(&rows);
    rows[TRANSCRIPT_TOP..TRANSCRIPT_TOP + height_of_page].to_vec()
}

/// 同上，但空行不占位置 —— 大多数断言只关心有字的那些。
fn trace_page(state: &mut TuiState, width: u16, height: u16) -> Vec<String> {
    trace_page_with_blanks(state, width, height)
        .iter()
        .filter(|row| !row.trim().is_empty())
        .cloned()
        .collect()
}

/// 主列那一半的行，左栏不算 —— 用来断言「转录一个字没动」。
///
/// 起点按**这一帧真的画了没有**分栏来定（分隔列 `┆` 就在左栏右边那一格），而不是按宽度
/// 猜一档：`Ctrl-O` 收起与窄终端走的是同一支降级，两者都让主列从第 0 列起（票 11）。
///
/// 判据是**整列**的 `┆`：主列页签条那一行自己也带一个分隔符（`对话┆轨迹`），按单行找会把它
/// 当成分隔列（`.scratch/trace-in-main/spec.md` §1）。
fn conversation_rows(state: &mut TuiState, width: u16, height: u16) -> Vec<String> {
    let frame = buffer(width, height, state);
    let left = divide_column(&frame, width, height).map_or(0, |divide| divide + 1);
    (0..height).map(|y| cells(&frame, y, left, width)).collect()
}

/// 左栏与主列之间那一列，屏幕上没有左栏时是 `None`。
fn divide_column(frame: &Buffer, width: u16, height: u16) -> Option<u16> {
    let half = (height / 2) as usize;
    (0..width).find(|x| {
        (0..height)
            .filter(|y| frame[(*x, *y)].symbol() == "┆")
            .count()
            > half
    })
}

/// 切到轨迹页，跟点它的页签一样。
///
/// 切换之后**再画一帧**：`trace_rect` 记的是上一帧真画出来的东西，指针分派就靠它
/// （票 04 §1 的那条纪律）。
fn open_trace_tab(state: &mut TuiState, width: u16, height: u16) {
    // 轨迹是主列页签条上的第二个标签（`.scratch/trace-in-main/spec.md` §2）：它画在屏幕第一
    // 行上，而不是左栏那条页签条上。
    click_in_row(
        state,
        width,
        height,
        TRANSCRIPT_TOP as u16 - 2,
        wording::TAB_TRACE,
    );
    let _ = screen(width, height, state);
}

/// 轨迹页是一条真视图：它画着转录里的东西，而且贴底跟随。
#[test]
fn the_trace_page_carries_the_transcript_and_sits_at_the_bottom() {
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 句话")));
    }
    open_trace_tab(&mut state, 120, 24);
    let page = trace_page(&mut state, 120, 24);
    assert!(
        page.iter().any(|row| row.contains("第 39 句话")),
        "最新那句贴底可见：{page:#?}"
    );
    assert!(
        !page.iter().any(|row| row.contains("第 0 句话")),
        "更老的那些被滚出了页区：{page:#?}"
    );
}

/// 滚轮滚**当前显示的那一页**，而两个视口的滚动位置互不打扰
/// （`.scratch/trace-in-main/spec.md` §4，推翻 `trace-tab` §5 那条按指针位置的分派）。
#[test]
fn the_wheel_scrolls_the_page_on_screen_and_leaves_the_other_alone() {
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 句话")));
    }
    open_trace_tab(&mut state, 120, 24);
    let page_before = trace_page(&mut state, 120, 24);
    // 指针停在哪儿都一样：屏幕上只有轨迹这一页。
    state.mouse(wheel_at(10, 12, true));
    let page = trace_page(&mut state, 120, 24);
    assert_ne!(page, page_before, "轨迹页往上滚了：{page:#?}");
    assert!(
        !page.iter().any(|row| row.contains("第 39 句话")),
        "它离开了底部：{page:#?}"
    );

    // 切到对话页：它仍在底部，一个字没动 —— 两个视口各记各的。
    click_in_row(
        &mut state,
        120,
        24,
        TRANSCRIPT_TOP as u16 - 2,
        wording::TAB_CONVERSATION,
    );
    let conversation_before = conversation_rows(&mut state, 120, 24);
    assert!(
        conversation_before
            .iter()
            .any(|row| row.contains("第 39 句话")),
        "对话视图仍在底部：{conversation_before:#?}"
    );

    // 在对话页上滚一格：动的是对话。
    state.mouse(wheel_at(90, 12, true));
    assert!(
        !conversation_rows(&mut state, 120, 24)
            .iter()
            .any(|row| row.contains("第 39 句话")),
        "对话跟着滚了"
    );

    // 切回轨迹页：它留在刚才放下的地方。
    click_in_row(
        &mut state,
        120,
        24,
        TRANSCRIPT_TOP as u16 - 2,
        wording::TAB_TRACE,
    );
    assert_eq!(trace_page(&mut state, 120, 24), page, "轨迹页留在原处");
}

/// 轨迹页离开底部之后也画「N 条新行」，而且那个块点得动。
#[test]
fn the_trace_page_offers_a_clickable_way_back_to_the_bottom() {
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 句话")));
    }
    open_trace_tab(&mut state, 120, 24);
    state.mouse(wheel_at(10, 12, true));
    let frame = buffer(120, 24, &mut state);
    let Some((column, row)) = cell_of(&frame, 120, 24, "点此到底") else {
        panic!(
            "轨迹页上该出现回去的指示器：{:?}",
            screen(120, 24, &mut state)
        );
    };
    click(&mut state, column, row);
    let page = trace_page(&mut state, 120, 24);
    assert!(
        page.iter().any(|row| row.contains("第 39 句话")),
        "点它回到了最新：{page:#?}"
    );
}

/// 切页不丢阅读进度：轨迹页的位置在切走再切回之后还在（票 09 验证 1）。
#[test]
fn the_trace_page_keeps_its_place_across_a_tab_switch() {
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 句话")));
    }
    open_trace_tab(&mut state, 120, 24);
    state.mouse(wheel_at(10, 12, true));
    let before = trace_page(&mut state, 120, 24);
    let top_before = before.first().cloned().expect("轨迹页有内容");

    // 切到对话页，再切回来：轨迹页的位置还在（主列页签条在屏幕第一行）。
    click_in_row(
        &mut state,
        120,
        24,
        TRANSCRIPT_TOP as u16 - 2,
        wording::TAB_CONVERSATION,
    );
    let conversation = conversation_rows(&mut state, 120, 24);
    assert!(
        conversation.iter().any(|row| row.contains("第 39 句话")),
        "对话页在它的底部"
    );
    open_trace_tab(&mut state, 120, 24);
    let after = trace_page(&mut state, 120, 24);
    assert_eq!(
        after.first().cloned(),
        Some(top_before),
        "位置还在：{after:#?}"
    );
}

/// 轨迹页里点一行开得出详情（票 09 验证 1）。
#[test]
fn a_row_in_the_trace_page_opens_its_detail() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-1",
        "bash",
        serde_json::json!({"command": "ls -la"}),
    ));
    state.apply(tool_completed(2, "call-1", true, Some("全部文件"), None));
    open_trace_tab(&mut state, 120, 24);
    let frame = buffer(120, 24, &mut state);
    let Some((column, row)) = cell_of(&frame, 120, 24, "调用 bash") else {
        panic!("轨迹页上该有那次调用");
    };
    click(&mut state, column, row);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("参数"), "详情覆盖层开着：{text}");
    assert!(text.contains("ls -la"), "里面是这次调用的参数：{text}");
}

/// 轨迹页里的一条消息只占**首行 + `…`**，点开才看得到全文（票 09 验证 2）。
#[test]
fn a_message_in_the_trace_page_is_one_line_that_opens_its_full_text() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(message(1, "第一行在这里\n第二行在那里", None));
    open_trace_tab(&mut state, 120, 24);
    let page = trace_page(&mut state, 120, 24);
    assert!(
        page.iter()
            .any(|row| row.contains("第一行在这里") && row.contains('…')),
        "首行带省略号：{page:#?}"
    );
    assert!(
        !page.iter().any(|row| row.contains("第二行在那里")),
        "第二行不在轨迹页上：{page:#?}"
    );

    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "第一行在这里").expect("轨迹页上那行消息");
    click(&mut state, column, row);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("第二行在那里"), "详情里有全文：{text}");
}

/// 轨迹页在主列里，而主列最小 40 列 —— 所以前缀**总是**带方括号
/// （`.scratch/trace-in-main/spec.md` §3）。去括号那一支还留在代码里（它与共享渲染那条路径
/// 共用签名），但屏幕上到不了。
#[test]
fn the_trace_page_keeps_the_brackets_around_the_prefix() {
    for width in [80u16, 120] {
        let mut state = state_with_roster(&["kimi"]);
        state.apply(user_message(1, "问题"));
        open_trace_tab(&mut state, width, 24);
        let page = trace_page(&mut state, width, 24);
        assert!(
            page.iter()
                .any(|row| without_stamp(row).starts_with("[用户] ")),
            "{width} 列下前缀带方括号：{page:#?}"
        );
    }
}

/// 宽度变化按目标掩码重放：两个视口都按新宽度重建，且共享源只被放一遍
/// （票 09 验证 4 走的就是这条 `emit_painted` 路径 —— `--continue` 的重放与实时
/// 共用它）。
#[test]
fn a_width_change_replays_the_trace_page_too() {
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 句话")));
    }
    open_trace_tab(&mut state, 120, 24);
    let _ = screen(120, 24, &mut state);

    // 缩到 80 列：左栏从 40 变 28，两个视口的源行都得按新宽度重排。
    let page = trace_page(&mut state, 80, 24);
    assert!(
        page.iter().any(|row| row.contains("第 39 句话")),
        "重放之后轨迹页仍贴底：{page:#?}"
    );
    let conversation = conversation_rows(&mut state, 80, 24);
    assert!(
        conversation.iter().any(|row| row.contains("第 39 句话")),
        "对话视图也还在：{conversation:#?}"
    );
    let lines = conversation
        .iter()
        .filter(|row| row.contains("句话"))
        .count();
    assert!(lines <= 24, "对话视图没有被推第二遍：{conversation:#?}");
}

// ---------------------------------------------------------------------------
// 两个视图的分工（`.scratch/trace-tab/spec.md` §2；票 10）
// ---------------------------------------------------------------------------

/// 一条上下文注入。
fn injected(seq: u64, content: &str) -> heng::render::RenderEvent {
    use heng::events::{ContextSource, Event, EventPayload, SpeakerId};
    heng::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::System,
        EventPayload::ContextInjected {
            source: ContextSource::McpCatalog,
            content: content.to_owned(),
        },
    ))
}

/// 主列只剩对话：用户文本、assistant 正文、保留清单；过程行全在轨迹页
/// （票 10 验证 1）。
#[test]
fn the_split_keeps_the_process_rows_out_of_the_conversation() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, "我问的问题"));
    state.apply(tool_started(
        2,
        "call-1",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(3, "call-1", true, Some("out"), None));
    state.apply(reasoning_delta("一段推理。"));
    state.apply(message(4, "助手的回答。", None));
    state.apply(injected(5, "注入的正文"));
    state.apply(RenderEvent::notice("命令回执".to_owned()));

    let conversation = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(conversation.contains("我问的问题"), "{conversation}");
    assert!(conversation.contains("助手的回答。"), "{conversation}");
    assert!(conversation.contains("命令回执"), "{conversation}");
    assert!(
        !conversation.contains("调用 bash"),
        "工具行进轨迹：{conversation}"
    );
    assert!(
        !conversation.contains("思考完成") && !conversation.contains("正在思考"),
        "思考行进轨迹：{conversation}"
    );
    assert!(
        !conversation.contains("上下文注入"),
        "注入行进轨迹：{conversation}"
    );

    // 轨迹页是全量：过程行与对话本身都在。
    open_trace_tab(&mut state, 120, 24);
    let page = trace_page(&mut state, 120, 24).join("\n");
    assert!(page.contains("调用 bash"), "{page}");
    assert!(page.contains("思考完成"), "{page}");
    assert!(page.contains("上下文注入"), "{page}");
    assert!(page.contains("我问的问题"), "{page}");
    assert!(page.contains("助手的回答。"), "{page}");
}

/// `▸` 的判据 = **谁把内容折起来谁就有**（`.scratch/tui-visual-language/spec.md` §13）。
///
/// 思考行、工具行、上下文注入行、轨迹视图的消息行都点得开（全文在详情里），所以都有；对话
/// 视图的消息行已经把全文显出来了，所以没有。
#[test]
fn only_the_rows_that_fold_their_content_carry_the_marker() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, "我问的问题"));
    state.apply(reasoning_delta("一段推理。"));
    state.apply(tool_started(
        2,
        "call-1",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(3, "call-1", true, Some("out"), None));
    state.apply(message(4, "助手的回答。", None));
    state.apply(injected(5, "注入的正文"));

    // 对话视图里没有任何一行折着内容。
    let conversation = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(
        !conversation.contains('▸'),
        "对话视图的消息行不给这个字形：{conversation}"
    );
    assert!(conversation.contains("助手的回答。"), "{conversation}");

    // 轨迹页上四个折起来的落点各一行。
    open_trace_tab(&mut state, 120, 24);
    let page = trace_page(&mut state, 120, 24);
    let text = page.join("\n");
    for folded in ["思考完成", "调用 bash", "上下文注入", "助手的回答。"] {
        let line = page
            .iter()
            .find(|line| line.contains(folded))
            .unwrap_or_else(|| panic!("{folded} 在轨迹页上：{text}"));
        assert!(
            line.contains('▸'),
            "{folded} 折起来了，所以带这个字形：{line}"
        );
    }
}

/// 换发言者空一行；同一人连发的块之间**不**空
/// （`.scratch/tui-visual-language/spec.md` §23）。
///
/// 插入点在块序列生成期，所以轨迹页与对话视图各记各的「上一个发言者」；这里在轨迹页上看，
/// 因为那里同框的发言者最多。
#[test]
fn a_speaker_change_opens_one_blank_line_and_a_run_of_tools_does_not() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, "问题"));
    state.apply(tool_started(
        2,
        "c1",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(3, "c1", true, Some("out"), None));
    state.apply(tool_started(
        4,
        "c2",
        "read_file",
        serde_json::json!({"file_path": "a.rs"}),
    ));
    state.apply(tool_completed(5, "c2", true, Some("out"), None));
    state.apply(message(6, "回答", None));

    open_trace_tab(&mut state, 120, 40);
    let page: Vec<String> = trace_page_with_blanks(&mut state, 120, 40)
        .iter()
        .map(|row| row.trim_end().to_owned())
        .collect();
    let at = |needle: &str| {
        page.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle} 在轨迹页上：{page:#?}"))
    };

    // 用户 → kimi：中间恰好一行空白。
    let question = at("问题");
    let first_tool = at("调用 bash");
    assert_eq!(first_tool, question + 2, "换发言者空一行：{page:#?}");
    assert!(
        page[question + 1].trim().is_empty(),
        "那一行是空的：{page:#?}"
    );

    // kimi 连发两条工具行、再跟自己的回答：中间一个空行都没有 —— 一组动作读起来是一组。
    let second_tool = at("调用 read_file");
    assert_eq!(second_tool, first_tool + 1, "同一人连发不拆散：{page:#?}");
    let answer = at("回答");
    assert_eq!(answer, second_tool + 1, "同一人接着说不拆散：{page:#?}");
}

/// 例外一：用户的话在对话视图里靠右，助手仍靠左（票 10 验证 2）。
#[test]
fn the_user_message_sits_on_the_right_and_the_assistant_on_the_left() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, "我说的这句话"));
    state.apply(message(2, "助手说的那句话", None));
    let frame = buffer(120, 24, &mut state);
    let (user_x, _) = cell_of(&frame, 120, 24, "我说的这句话").expect("用户那一行");
    let (assistant_x, _) = cell_of(&frame, 120, 24, "助手说的那句话").expect("助手那一行");
    assert!(user_x > 90, "用户的话靠右（列 {user_x}）");
    assert!(assistant_x < 60, "助手的话靠左（列 {assistant_x}）");
}

#[test]
fn a_multi_line_user_message_keeps_one_left_edge() {
    // 用户消息是一个**气泡**：等宽、贴右缘。靠右的是块，不是每一行 —— 各自贴右缘会把一段
    // 三行的输入折成参差的一列，长行往左伸、短行缩到最右边
    // （`.scratch/trace-tab/spec.md` §2 的补记）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, "长一点的这一段\n短\n中等的这一段"));
    let frame = buffer(120, 24, &mut state);
    let at = |needle: &str| {
        cell_of(&frame, 120, 24, needle)
            .unwrap_or_else(|| panic!("{needle} 在屏幕上"))
            .0
    };
    let (long, short, medium) = (at("长一点的这一段"), at("短"), at("中等的这一段"));
    assert_eq!(long, short, "气泡里每一行从同一条左边界起");
    assert_eq!(short, medium, "气泡里每一行从同一条左边界起");
    // 那条左边界由「最宽的那一行」与「贴右缘」一起定：气泡右缘就是转录的右缘。
    let area = transcript_area();
    let width = text_columns("长一点的这一段") as u16;
    assert_eq!(area.x + area.width - width, long, "气泡贴着转录的右缘");
}

#[test]
fn a_bubble_never_grows_past_two_thirds_of_the_transcript() {
    // 宽度封顶才有气泡感：一条顶满宽度的用户消息读起来与助手正文没有分别
    // （`.scratch/trace-tab/spec.md` §2 的补记）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, &"x".repeat(200)));
    let frame = buffer(120, 24, &mut state);
    let area = transcript_area();
    let inner = area.width as usize * 2 / 3;
    let (start, _) = cell_of(&frame, 120, 24, "xxx").expect("气泡的第一行");
    assert_eq!(
        start,
        area.x + area.width - inner as u16,
        "气泡的左边界由那条三分之二定"
    );
    // 一行都没丢：都折在气泡里。
    let xs: usize = (0..24)
        .map(|y| row_text(&frame, y, 120).matches('x').count())
        .sum();
    assert_eq!(xs, 200, "200 个字全都画了出来");
}

#[test]
fn copying_a_bubble_takes_only_the_text() {
    // 气泡里的填充与它左边那片留白都是**装饰**：拖选复制只带走正文，而你自己写下的缩进
    // 一个字不动（`.scratch/trace-tab/spec.md` §2 的补记）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, "第一行\n  缩进的第二行"));
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "第一行").expect("气泡在屏幕上");
    // 起点落在气泡左边那片留白里，终点越过第二行的末尾 —— 两头都是装饰。
    state.mouse(press(column - 10, row));
    state.mouse(drag_to(column + 20, row + 1));
    state.mouse(release(column + 20, row + 1));
    let expected = heng::render::selection::osc52("第一行\n  缩进的第二行");
    assert_eq!(
        state.take_clipboard().as_deref(),
        Some(expected.as_str()),
        "复制出来的是正文本身"
    );
}

/// 一帧里转录正文那块矩形（`120×24`，左栏在）。
fn transcript_area() -> ratatui::layout::Rect {
    use heng::render::layout::plan;
    use ratatui::layout::Rect;
    plan(Rect::new(0, 0, 120, 24), 3, true).transcript_text()
}

/// 例外二：注入、用户、助手三类前缀各一色（票 10 验证 2）。
///
/// 注入行只住在轨迹页，所以这次比对在轨迹页上做 —— 三种前缀在那里同框。
#[test]
fn the_three_prefixes_wear_three_colours() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(injected(1, "注入的正文"));
    state.apply(user_message(2, "问题"));
    state.apply(message(3, "回答", None));
    open_trace_tab(&mut state, 120, 24);
    let frame = buffer(120, 24, &mut state);
    let colour = |frame: &Buffer, needle: &str| {
        let (x, y) = cell_of(frame, 120, 24, needle).unwrap_or_else(|| panic!("{needle} 在屏幕上"));
        frame[(x, y)].fg
    };
    let injected = colour(&frame, "[上下文注入");
    let user = colour(&frame, "[用户]");
    let assistant = colour(&frame, "[kimi]");
    assert_eq!(injected, Color::LightBlue, "注入行有自己的专色");
    assert_eq!(user, Color::LightGreen, "用户照旧");
    assert_eq!(assistant, Color::LightCyan, "助手照旧");
    assert_ne!(injected, user);
    assert_ne!(injected, assistant);
}

// ---------------------------------------------------------------------------
// 左栏不可见时过程行也只在轨迹里（`.scratch/trace-tab/spec.md` §6 的 2026-10-05 推翻）
// ---------------------------------------------------------------------------

/// 一场带过程行的会话：用户的话、一次调用、一段思考、一条回答、一次注入、一对
/// 回合边界行与一条诊断。
fn a_session_with_process_rows(state: &mut TuiState) {
    state.apply(user_message(1, "问题"));
    state.apply(tool_started(
        2,
        "call-1",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(3, "call-1", true, Some("out"), None));
    state.apply(reasoning_delta("一段推理。"));
    state.apply(message(4, "回答。", None));
    state.apply(injected(5, "注入的正文"));
    state.apply(turn_started(6));
    state.apply(turn_ended(7));
    state.apply(RenderEvent::diagnostic(
        "provider 流结束：正常停止".to_owned(),
    ));
}

/// 这些过程行在该在的地方，不在对话视图里。
fn assert_process_rows_are_out_of_the_conversation(conversation: &str) {
    for present in ["问题", "回答。"] {
        assert!(conversation.contains(present), "{conversation}");
    }
    for absent in [
        "调用 bash",
        "思考完成",
        "上下文注入",
        "回合开始",
        "回合结束",
        "诊断",
    ] {
        assert!(
            !conversation.contains(absent),
            "{absent} 不该出现在对话视图里：{conversation}"
        );
    }
}

/// `Ctrl-O` 收起左栏**不再**把过程行放回对话视图：收起就是「过程行暂时看不到」
/// （2026-10-05 维护者选的这一支，推翻了原先的降级）。
#[test]
fn hiding_the_sidebar_leaves_the_process_rows_out_of_the_conversation() {
    let mut state = state_with_roster(&["kimi"]);
    a_session_with_process_rows(&mut state);
    assert_process_rows_are_out_of_the_conversation(
        &conversation_rows(&mut state, 120, 24).join("\n"),
    );

    state.key(Key::CtrlO);
    assert_process_rows_are_out_of_the_conversation(
        &conversation_rows(&mut state, 120, 24).join("\n"),
    );

    // 叫回来也一样。
    state.key(Key::CtrlO);
    assert_process_rows_are_out_of_the_conversation(
        &conversation_rows(&mut state, 120, 24).join("\n"),
    );
}

/// 80 列以下左栏本来就不存在，规则不变：过程行此时哪儿都不显示。
#[test]
fn a_narrow_terminal_leaves_the_process_rows_out_of_the_conversation() {
    let mut state = state_with_roster(&["kimi"]);
    a_session_with_process_rows(&mut state);
    assert_process_rows_are_out_of_the_conversation(
        &conversation_rows(&mut state, 60, 24).join("\n"),
    );
}

/// 收起与叫回不改动滚动意图：来回之后两个视图都还在底部，指示器不出现
/// （票 11 验证 3）。
#[test]
fn hiding_and_showing_the_sidebar_leaves_the_scroll_intent_alone() {
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 句话")));
    }
    open_trace_tab(&mut state, 120, 24);
    let _ = screen(120, 24, &mut state);

    state.key(Key::CtrlO);
    let rows = screen(120, 24, &mut state);
    assert!(
        !rows.iter().any(|row| row.contains("行新内容")),
        "收起不把任何视图踢出底部：{}",
        rows.join("\n")
    );

    state.key(Key::CtrlO);
    let rows = screen(120, 24, &mut state);
    assert!(
        !rows.iter().any(|row| row.contains("行新内容")),
        "叫回来也一样：{}",
        rows.join("\n")
    );
    let page = trace_page(&mut state, 120, 24);
    assert!(
        page.iter().any(|row| row.contains("第 39 句话")),
        "轨迹页仍在最新那一行：{page:#?}"
    );
}

// ---------------------------------------------------------------------------
// 轨迹页用分隔线分开轮次（`.scratch/trace-tab/spec.md` §3；票 12 的 2026-10-05 修订）
// ---------------------------------------------------------------------------

/// 轨迹页里含 `needle` 的那一行，以及它上面最近的那条分隔线的行
/// （`.scratch/trace-in-main/spec.md` §3）。
fn rule_before(state: &mut TuiState, needle: &str) -> Option<(u16, u16)> {
    let frame = buffer(120, 24, state);
    let row = (TRANSCRIPT_TOP as u16..24).find(|y| {
        cells(&frame, *y, MAIN_LEFT_AT_120, TRANSCRIPT_TEXT_RIGHT_AT_120).contains(needle)
    })?;
    // 组的界是**组头那一行** —— 它是一条带文字的虚线（`.scratch/trace-ledger/spec.md` §5），
    // 所以判据是「含虚线且写着 `回合` / `轮次`」，不是「整行都是虚线」。
    let rule = (TRANSCRIPT_TOP as u16..row).rev().find(|y| {
        // 只看正文那一段：最右那两列是滚动条与回合条的位置，滑块会盖在线上
        // （`.scratch/tui-visual-language/issues/07` 决定 2）。
        let line = cells(&frame, *y, MAIN_LEFT_AT_120, TRANSCRIPT_TEXT_RIGHT_AT_120);
        (line.contains("回合 ") || line.contains("轮次 ")) && line.contains('┄')
    })?;
    Some((row, rule))
}

/// 轨迹页里两个单位之间画一条横向虚线 —— 那条线就是轮次的边界，没有底色
/// （票 12 的 2026-10-05 修订）。
#[test]
fn the_trace_page_opens_each_turn_with_a_header() {
    let mut state = state_with_roster(&["kimi"]);
    turns(&mut state, 3);
    // 高一点的终端：三个回合各四行、各一条组头，换发言者处还各多一行空白
    // （`.scratch/tui-visual-language/spec.md` §23），24 行的页装不下三份。
    open_trace_tab(&mut state, 120, 40);
    let page = trace_page(&mut state, 120, 40);

    // 每个回合一条组头。**并进**单位之间那条虚线，所以它占的那一行就是边界那一行、零额外行数
    // （`.scratch/trace-ledger/spec.md` §5）。
    let headers: Vec<usize> = page
        .iter()
        .enumerate()
        .filter(|(_, row)| (row.contains("回合 ") || row.contains("轮次 ")) && row.contains('┄'))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(headers.len(), 3, "三个回合三条组头：{page:#?}");
    for (position, header) in headers.iter().enumerate() {
        // 组头**长在这一级组的最前面**：它下面紧跟的就是那一回合的第一行成员。
        assert!(
            page[header + 1].contains("回合开始"),
            "组头紧跟在那之前那一行成员上面：{page:#?}"
        );
        assert!(
            page[*header].contains(&format!("回合 {}", position + 1)),
            "序号从 1 起数：{page:#?}"
        );
    }
}

/// 轨迹页补了滚动条，画在**最右列**，正文因此 38 / 26 列
/// （`.scratch/tui-visual-language/issues/07` 决定 2）。
#[test]
fn the_trace_page_has_a_scrollbar_in_its_rightmost_column() {
    // 主列内容宽 79 列、正文 77 列（滚动条与回合条那两列永远留着）：77 个字一句话一行放得下，
    // 78 个就折（`.scratch/trace-in-main/spec.md` §3、§4）。
    let mut state = state_with_roster(&["kimi"]);
    // 主列内容 77 列里，行首 9 列归时间戳（`.scratch/trace-in-main/spec.md` §5），所以正文
    // 拿到 68 列。
    state.apply(RenderEvent::notice("x".repeat(68)));
    state.apply(RenderEvent::notice("y".repeat(69)));
    open_trace_tab(&mut state, 120, 24);
    let page = trace_page(&mut state, 120, 24);
    assert_eq!(
        page.iter().filter(|row| row.contains('x')).count(),
        1,
        "68 列一句话一行：{page:#?}"
    );
    assert_eq!(
        page.iter().filter(|row| row.contains('y')).count(),
        2,
        "69 列一句话折成两行：{page:#?}"
    );

    // 有东西可读时主列右缘那一列（x = 118）出现滑块。
    for index in 0..20 {
        state.apply(RenderEvent::notice(format!("第 {index} 句话")));
    }
    let frame = buffer(120, 24, &mut state);
    assert!(
        (TRANSCRIPT_TOP as u16..24).any(|y| frame[(SCROLLBAR_AT_120, y)].symbol() == "█"),
        "主列右缘那一列是滚动条"
    );

    // 窄档（80 列）也一样：左栏让出 29 列，主列内容因此 51 列、正文 49 列，滚动条在 x = 78。
    let mut narrow = state_with_roster(&["kimi"]);
    narrow.apply(RenderEvent::notice("z".repeat(40)));
    narrow.apply(RenderEvent::notice("w".repeat(41)));
    open_trace_tab(&mut narrow, 80, 24);
    let page = trace_page(&mut narrow, 80, 24);
    assert_eq!(
        page.iter().filter(|row| row.contains('z')).count(),
        1,
        "40 列一句话一行：{page:#?}"
    );
    assert_eq!(
        page.iter().filter(|row| row.contains('w')).count(),
        2,
        "41 列一句话折成两行：{page:#?}"
    );
    for index in 0..20 {
        narrow.apply(RenderEvent::notice(format!("第 {index} 句")));
    }
    let frame = buffer(80, 24, &mut narrow);
    assert!(
        (TRANSCRIPT_TOP as u16..24).any(|y| frame[(78, y)].symbol() == "█"),
        "窄档的滚动条在 x = 78"
    );
}

/// 线是**行**，所以滚动时它跟着内容走，不跟着屏幕行重排（票 12 的修订）。
#[test]
fn the_trace_rule_travels_with_the_content() {
    let mut state = state_with_roster(&["kimi"]);
    turns(&mut state, 6);
    open_trace_tab(&mut state, 120, 24);
    let _ = screen(120, 24, &mut state);
    let before = rule_before(&mut state, "问题 5").expect("最后一个回合的问题在屏幕上");
    state.mouse(wheel_at(10, 12, true));
    let after = rule_before(&mut state, "问题 5").expect("它还在屏幕上");
    assert_ne!(before.0, after.0, "内容挪了屏幕行");
    assert_eq!(
        before.0 - before.1,
        after.0 - after.1,
        "而它上面那条线跟着它一起挪"
    );
}

/// 轨迹页与主列都不铺底：区分轮次的是那条线，不是色块（票 12 的修订）。
#[test]
fn neither_view_paints_a_stripe_background() {
    let mut state = state_with_roster(&["kimi"]);
    turns(&mut state, 3);
    open_trace_tab(&mut state, 120, 24);
    let frame = buffer(120, 24, &mut state);
    for y in 0..24 {
        assert_eq!(frame[(1, y)].bg, Color::Reset, "左栏第 {y} 行没有底色");
        assert_eq!(frame[(50, y)].bg, Color::Reset, "主列第 {y} 行没有底色");
    }
}

// ---------------------------------------------------------------------------
// 主列页签条与轨迹视图的归属（`.scratch/trace-in-main/spec.md` §2、§3、§4）
// ---------------------------------------------------------------------------

/// 主列页签条上的两个标签切换主列那一页，默认是对话。
#[test]
fn clicking_the_main_tab_switches_the_page_in_the_main_column() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, "问题"));
    state.apply(tool_started(
        2,
        "c1",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(3, "c1", true, Some("out"), None));

    // 起步是对话页：过程行不在上面。
    let conversation = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(conversation.contains("问题"), "{conversation}");
    assert!(
        !conversation.contains("调用 bash"),
        "过程行不进对话：{conversation}"
    );

    // 点 `轨迹`：主列换成轨迹页，而它是全量。
    click_in_row(
        &mut state,
        120,
        24,
        TRANSCRIPT_TOP as u16 - 2,
        wording::TAB_TRACE,
    );
    let page = trace_page(&mut state, 120, 24).join("\n");
    assert!(page.contains("调用 bash"), "轨迹页画全量块：{page}");

    // 点 `对话` 回来。
    click_in_row(
        &mut state,
        120,
        24,
        TRANSCRIPT_TOP as u16 - 2,
        wording::TAB_CONVERSATION,
    );
    let conversation = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(conversation.contains("问题"), "{conversation}");
    assert!(
        !conversation.contains("调用 bash"),
        "过程行仍不进对话：{conversation}"
    );
}

/// 轨迹页在主列的页签上，所以 `Ctrl-O` 收起左栏、终端窄到 60 列时它照样可达
/// （`.scratch/trace-in-main/spec.md` §4，推翻 `trace-tab` §6 那条「过程行暂时看不到」）。
#[test]
fn the_trace_page_is_reachable_however_narrow_the_terminal_is() {
    for (width, hide) in [(120u16, true), (60, false)] {
        let mut state = state_with_roster(&["kimi"]);
        a_session_with_process_rows(&mut state);
        if hide {
            state.key(Key::CtrlO);
        }
        open_trace_tab(&mut state, width, 24);
        let page = trace_page(&mut state, width, 24).join("\n");
        assert!(
            page.contains("调用 bash"),
            "{width} 列（收起左栏 = {hide}）下轨迹页画着过程行：{page}"
        );
    }
}

/// 回合条只画在对话页上：它量的是对话视口的单位位置，画在轨迹页上会指着另一个视口
/// （`.scratch/trace-in-main/spec.md` §4）。
#[test]
fn the_turn_rail_is_only_on_the_conversation_page() {
    let mut state = state_with_roster(&["kimi"]);
    turns(&mut state, 3);
    let _ = screen(120, 24, &mut state);
    assert!(!turn_rail_shape(&mut state).is_empty(), "对话页上有格子");
    open_trace_tab(&mut state, 120, 24);
    assert_eq!(turn_rail_shape(&mut state), "", "轨迹页那一列是空的");
}

/// 键盘三键归**当前显示的那一页**（`.scratch/trace-in-main/spec.md` §4，推翻
/// `trace-tab` 用户故事 32 那条「轨迹只吃滚轮」）。
#[test]
fn the_page_keys_follow_the_page_on_screen() {
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 句话")));
    }
    open_trace_tab(&mut state, 120, 24);
    let before = trace_page(&mut state, 120, 24);
    state.key(Key::PageUp);
    let page = trace_page(&mut state, 120, 24);
    assert_ne!(page, before, "轨迹页翻了一页：{page:#?}");
    state.key(Key::CtrlG);
    assert!(
        trace_page(&mut state, 120, 24)
            .iter()
            .any(|row| row.contains("第 39 句话")),
        "回底键把它带回最新"
    );

    // 切到对话页：同一批键动的是对话，而轨迹页停在它被放下的地方。
    let page_left = trace_page(&mut state, 120, 24);
    click_in_row(
        &mut state,
        120,
        24,
        TRANSCRIPT_TOP as u16 - 2,
        wording::TAB_CONVERSATION,
    );
    state.key(Key::PageUp);
    assert!(
        !conversation_rows(&mut state, 120, 24)
            .iter()
            .any(|row| row.contains("第 39 句话")),
        "对话被翻走了"
    );
    state.key(Key::CtrlG);
    click_in_row(
        &mut state,
        120,
        24,
        TRANSCRIPT_TOP as u16 - 2,
        wording::TAB_TRACE,
    );
    assert_eq!(trace_page(&mut state, 120, 24), page_left, "轨迹页留在原处");
}

// ---------------------------------------------------------------------------
// 轨迹行的时间戳（`.scratch/trace-in-main/spec.md` §5）
// ---------------------------------------------------------------------------

/// 轨迹页里每个块的开头都是**产生它的那一刻**：不同事件的两个块读起来就是两个时刻。
#[test]
fn every_block_on_the_trace_page_opens_with_its_own_time() {
    use heng::events::{EventPayload, SpeakerId, StopReason};

    let mut state = state_with_roster(&["kimi"]);
    let early = fixed_at(4, 16, 53);
    let late = fixed_at(9, 5, 1);
    state.apply(at_event(
        1,
        early,
        EventPayload::TurnStarted {
            agent: SpeakerId::Debater("kimi".into()),
            iteration: 1,
        },
    ));
    state.apply(at_event(
        2,
        late,
        EventPayload::TurnEnded {
            reason: StopReason::Completed,
        },
    ));
    open_trace_tab(&mut state, 120, 24);
    let page = trace_page(&mut state, 120, 24);
    let first = wording::stamp(early);
    let second = wording::stamp(late);
    assert_ne!(first, second, "两个时刻读起来不同");
    assert!(
        page.iter().any(|row| row.starts_with(&first)),
        "回合开始那一行带着它自己的时刻（{first}）：{page:#?}"
    );
    assert!(
        page.iter().any(|row| row.starts_with(&second)),
        "回合结束那一行带着它自己的时刻（{second}）：{page:#?}"
    );
}

/// 对话视图里**没有**时间戳：时刻只长在轨迹页上（`.scratch/trace-in-main/spec.md` §5）。
#[test]
fn the_conversation_page_carries_no_stamps() {
    use heng::events::{EventPayload, Role};

    let at = fixed_at(4, 16, 53);
    let mut state = state_with_roster(&["kimi"]);
    state.apply(at_event(
        1,
        at,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: "问题".to_owned(),
            reasoning: None,

            first_token_ms: None,
        },
    ));
    let conversation = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(conversation.contains("问题"), "{conversation}");
    assert!(
        !conversation.contains(&wording::stamp(at)),
        "对话视图里不该有时刻：{conversation}"
    );
}

/// 一个折成两行的块：只有首行顶着它的时刻，折出来的续行从第 0 列起 —— 时刻是**块**的属性，
/// 不是每一行的（`.scratch/trace-in-main/spec.md` §5）。
#[test]
fn a_wrapped_block_carries_its_stamp_on_the_first_line_only() {
    let at = fixed_at(4, 16, 53);
    let mut state = state_with_roster(&["kimi"]);
    state.apply(RenderEvent::Notice {
        at,
        message: "字".repeat(80),
    });
    open_trace_tab(&mut state, 120, 24);
    let page = trace_page(&mut state, 120, 24);
    let rows: Vec<&String> = page.iter().filter(|row| row.contains('字')).collect();
    assert!(rows.len() >= 2, "一句话折成两行：{page:#?}");
    assert!(
        rows[0].starts_with(&wording::stamp(at)),
        "首行带时刻：{:?}",
        rows[0]
    );
    assert!(
        !rows[1].starts_with(&wording::stamp(at)),
        "续行不再重复时刻：{:?}",
        rows[1]
    );
    assert!(
        rows[1].starts_with('字'),
        "续行从第 0 列接着上一行的话：{:?}",
        rows[1]
    );
    assert_eq!(
        text_columns(&rows[0][..heng::render::layout::STAMP_COLUMNS as usize]),
        heng::render::layout::STAMP_COLUMNS as usize,
        "时刻列宽就是那个常数"
    );
}

/// 一个 `HH:MM:SS ` 戳的形状：不读真时钟，因为推理增量没有信封，开着的行的时刻只能是
/// 「收到它的那一刻」（`.scratch/trace-thought-stamp/spec.md` §1）。
fn looks_like_a_stamp(text: &str) -> bool {
    let columns = heng::render::layout::STAMP_COLUMNS as usize;
    let bytes = text.as_bytes();
    bytes.len() >= columns
        && bytes[..columns - 1]
            .iter()
            .enumerate()
            .all(|(index, byte)| match index {
                2 | 5 => *byte == b':',
                _ => byte.is_ascii_digit(),
            })
        && bytes[columns - 1] == b' '
}

/// 还开着的那一行也有时刻：**思考开始**那一刻（`.scratch/trace-thought-stamp/spec.md` §1，
/// 推翻 `trace-in-main` 用户故事 21 的后半句）。
#[test]
fn an_open_thinking_line_carries_the_moment_its_thinking_started() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("先看依赖，"));
    open_trace_tab(&mut state, 120, 24);

    let page = trace_page(&mut state, 120, 24);
    let row = page
        .iter()
        .find(|row| row.contains("… 正在思考"))
        .expect("思考行在轨迹页上");
    assert!(looks_like_a_stamp(row), "行首是一个时刻：{row:?}");
    assert_eq!(
        without_stamp(row).trim_end(),
        "[kimi] … 正在思考",
        "戳只占行首那几列"
    );
}

/// 定稿时同一行的时间戳跳成完成那一刻（`.scratch/trace-thought-stamp/spec.md` §2）。
#[test]
fn a_settling_thinking_line_jumps_to_the_moment_it_finished() {
    use heng::events::{EventPayload, Role};

    let done = fixed_at(4, 16, 53);
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("先看依赖，"));
    state.apply(at_event(
        2,
        done,
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: "答案是 42。".to_owned(),
            reasoning: Some("先看依赖，".to_owned()),

            first_token_ms: None,
        },
    ));
    open_trace_tab(&mut state, 120, 24);

    let page = trace_page(&mut state, 120, 24);
    let row = page
        .iter()
        .find(|row| row.contains("思考完成"))
        .expect("定稿的那一行");
    assert!(
        row.starts_with(&wording::stamp(done)),
        "定稿后写的是完成那一刻（{}）：{row:?}",
        wording::stamp(done)
    );
}

/// 宽度变化会把轨迹页整批重放，重放出来的那一行带着同一个戳
/// （`.scratch/trace-thought-stamp/spec.md` §3）。
#[test]
fn a_replayed_open_thinking_line_keeps_its_stamp() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(reasoning_delta("先看依赖，"));
    open_trace_tab(&mut state, 120, 24);
    let before = trace_page(&mut state, 120, 24)
        .into_iter()
        .find(|row| row.contains("… 正在思考"))
        .expect("思考行在轨迹页上");

    let page = trace_page(&mut state, 100, 24);
    let after = page
        .iter()
        .find(|row| row.contains("… 正在思考"))
        .expect("重放出来的思考行");
    assert!(looks_like_a_stamp(after), "重放出来的行也有戳：{after:?}");
    assert_eq!(
        without_stamp(after).trim_end(),
        without_stamp(&before).trim_end(),
        "重放不改变这一行的内容"
    );
}

// ---------------------------------------------------------------------------
// 详情按视图还原（`.scratch/trace-tab/spec.md` §5；票 13）
// ---------------------------------------------------------------------------

/// 主列里含 `needle` 的那一屏行（轨迹视图的那些行住在这里）。
fn main_row_of(state: &mut TuiState, needle: &str) -> u16 {
    let frame = buffer(120, 24, state);
    (0..24)
        .find(|y| {
            cells(&frame, *y, MAIN_LEFT_AT_120, TRANSCRIPT_TEXT_RIGHT_AT_120).contains(needle)
        })
        .unwrap_or_else(|| panic!("主列里没有含 {needle:?} 的行"))
}

/// 从轨迹页打开的详情，关掉之后回到轨迹页打开前的位置，而对话视图一个字没动
/// （票 13 验证 2）。
#[test]
fn closing_a_trace_detail_returns_to_where_it_was_opened() {
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..10 {
        state.apply(RenderEvent::notice(format!("第 {index} 句话")));
    }
    state.apply(message(11, "被点开的消息", None));
    for index in 0..10 {
        state.apply(RenderEvent::notice(format!("后 {index} 句话")));
    }
    open_trace_tab(&mut state, 120, 24);
    let _ = screen(120, 24, &mut state);
    // 往上滚一格，进入回看态。
    state.mouse(wheel_at(10, 12, true));
    let page_before = trace_page(&mut state, 120, 24);
    let conversation_before = conversation_rows(&mut state, 120, 24);
    assert!(
        page_before.iter().any(|row| row.contains("被点开的消息")),
        "那条消息还在页上：{page_before:#?}"
    );

    let row = main_row_of(&mut state, "被点开的消息");
    click(&mut state, MAIN_LEFT_AT_120, row);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains(MESSAGE_FACES), "详情开着：{text}");

    state.key(Key::Esc);
    assert_eq!(
        trace_page(&mut state, 120, 24),
        page_before,
        "轨迹页回到打开前的位置"
    );
    assert_eq!(
        conversation_rows(&mut state, 120, 24),
        conversation_before,
        "对话视图没动"
    );
}

/// 指针在左栏、但显示的不是轨迹页时，滚轮仍归转录：轨迹 pane 这一帧没被画出来，滚它会
/// 算出错的落点。判据是「轨迹页在不在屏幕上」，不是「指针在不在左栏」。
#[test]
fn the_wheel_over_the_sidebar_goes_to_the_conversation_when_the_trace_page_is_hidden() {
    let mut state = state_with_roster(&["kimi"]);
    for index in 0..40 {
        state.apply(RenderEvent::notice(format!("第 {index} 句话")));
    }
    let _ = screen(120, 24, &mut state);
    let conversation_before = conversation_rows(&mut state, 120, 24);

    state.mouse(wheel_at(10, 12, true));
    assert_ne!(
        conversation_rows(&mut state, 120, 24),
        conversation_before,
        "调用量页上滚的是转录"
    );
}

/// 维护者 2026-10-05 列的那几类运行日志只在轨迹页：回合 / 轮次的开始、正常的收尾、
/// 权限裁决、诊断（`.scratch/trace-tab/spec.md` §2 的收紧）。
#[test]
fn the_run_log_lines_stay_out_of_the_conversation() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, "问题"));
    state.apply(turn_started(2));
    state.apply(message(3, "回答。", None));
    state.apply(permission_asked(4, "call-1"));
    state.apply(permission_decided(5));
    state.apply(turn_ended(6));
    state.apply(RenderEvent::diagnostic(
        "provider 流结束：正常停止".to_owned(),
    ));

    let conversation = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(conversation.contains("问题"), "{conversation}");
    assert!(conversation.contains("回答。"), "{conversation}");
    for absent in ["回合开始", "回合结束", "权限裁决", "诊断"] {
        assert!(
            !conversation.contains(absent),
            "{absent} 不在对话视图里：{conversation}"
        );
    }

    open_trace_tab(&mut state, 120, 24);
    let page = trace_page(&mut state, 120, 24).join("\n");
    for present in ["回合开始", "回合结束", "权限裁决", "诊断"] {
        assert!(page.contains(present), "{present} 在轨迹页里：{page}");
    }
}

/// 名字独占一行，话在下一行（2026-10-05 维护者的排版优化）。
#[test]
fn the_name_sits_on_its_own_row_in_the_conversation() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, "我问的问题"));
    state.apply(message(2, "助手的回答。", None));
    let rows = conversation_rows(&mut state, 120, 24);

    let user = rows
        .iter()
        .position(|row| row.trim() == "[用户]")
        .expect("用户的名字独占一行");
    assert!(rows[user + 1].contains("我问的问题"), "{rows:#?}");

    let assistant = rows
        .iter()
        .position(|row| row.trim() == "[kimi]")
        .expect("助手的名字独占一行");
    assert!(rows[assistant + 1].contains("助手的回答。"), "{rows:#?}");
}

/// 模型还没吐出第一个字时，对话视图末尾有一条会走的「正在思考…」；正文一到它就消失
/// （2026-10-05 维护者的优化）。
#[test]
fn the_conversation_shows_a_waiting_hint_while_the_model_is_quiet() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, "问题"));
    state.request(ConsoleRequest::RunState { running: true });

    let first = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(first.contains("正在思考."), "还没出字时有等待提示：{first}");

    // 点号每 8 帧挪一格（一帧 60 ms）。
    for _ in 0..8 {
        state.tick();
    }
    let second = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(second.contains("正在思考.."), "点号往前走了一格：{second}");

    // 第一个正文增量一到，提示让位。
    state.apply(text_delta("答案。"));
    let third = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(!third.contains("正在思考"), "正文到了提示就走：{third}");
    assert!(third.contains("答案。"), "{third}");
}

/// 等待提示长什么样：`[名字]` 一行，它**在做什么**一行 —— 有工具在跑就说那个工具在干什么，
/// 否则说它在想（2026-10-05 的两条优化）。
#[test]
fn the_waiting_hint_names_the_speaker_and_what_it_is_doing() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, "问题"));
    state.request(ConsoleRequest::RunState { running: true });

    // 没有工具在跑：它在想。
    let thinking = conversation_rows(&mut state, 120, 24);
    let name = thinking
        .iter()
        .rposition(|row| row.trim() == "[kimi]")
        .expect("名字那一行");
    assert!(thinking[name + 1].contains("正在思考."), "{thinking:#?}");

    // 读文件：说它正在看哪一份。
    state.apply(tool_started(
        2,
        "c1",
        "read_file",
        serde_json::json!({"path": "CONTEXT.md"}),
    ));
    let reading = conversation_rows(&mut state, 120, 24);
    let name = reading
        .iter()
        .rposition(|row| row.trim() == "[kimi]")
        .expect("名字那一行");
    assert!(
        reading[name + 1].contains("正在查看 CONTEXT.md…"),
        "{reading:#?}"
    );

    // 写文件：说它正在写哪一份。
    state.apply(tool_started(
        3,
        "c2",
        "write_file",
        serde_json::json!({"path": "docs/render.md"}),
    ));
    let writing = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(writing.contains("正在写 docs/render.md…"), "{writing}");

    // shell：命令自己的描述已经带着动词。
    state.apply(tool_started(
        4,
        "c3",
        "bash",
        serde_json::json!({"command": "cargo test"}),
    ));
    let running = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(running.contains("正在运行 cargo test…"), "{running}");

    // 结果一到，提示回到「在想」。
    state.apply(tool_completed(5, "c3", true, Some("ok"), None));
    let settled = conversation_rows(&mut state, 120, 24).join("\n");
    assert!(settled.contains("正在思考."), "{settled}");
}

/// 正文一开始流，`[名字]` **不许消失** —— 它一直留在那行上，直到 markdown 完成、名字改由
/// 源行带来（2026-10-05 维护者报告的观感问题）。
#[test]
fn the_name_row_survives_the_streaming_body() {
    let mut state = state_with_roster(&["kimi"]);
    state.apply(user_message(1, "问题"));
    state.request(ConsoleRequest::RunState { running: true });
    state.apply(text_delta("答案的第一段。"));

    let rows = conversation_rows(&mut state, 120, 24);
    let name = rows
        .iter()
        .rposition(|row| row.trim() == "[kimi]")
        .expect("流式正文上面那一行名字还在");
    assert!(rows[name + 1].contains("答案的第一段。"), "{rows:#?}");
    assert!(
        !rows.join("\n").contains("正在思考"),
        "提示让位给正文了：{rows:#?}"
    );

    // 完成之后名字仍在那儿 —— 这一回由**源行**带来。一轮收尾，末尾不再有等待块
    // （有的话它是新一组的名字，不是这条消息的）。
    state.apply(message(2, "答案的第一段。", None));
    state.request(ConsoleRequest::RunState { running: false });
    let rows = conversation_rows(&mut state, 120, 24);
    let name = rows
        .iter()
        .position(|row| row.trim() == "[kimi]")
        .expect("完成之后名字也在");
    assert!(rows[name + 1].contains("答案的第一段。"), "{rows:#?}");
}

/// 一帧里第一行含有 `needle` 的那一行：它是第几行，文本是什么。
fn row_holding(frame: &Buffer, needle: &str) -> (u16, String) {
    let area = frame.area;
    for y in 0..area.height {
        let text = cells(frame, y, area.x, area.right());
        if text.contains(needle) {
            return (y, text);
        }
    }
    panic!("画出来的帧里没有一行含有 {needle:?}");
}

#[test]
fn the_addresses_in_the_conversation_are_drawn_underlined() {
    // `.scratch/clickable-links/spec.md` §4：对话视图里点得开的那几列看得见地带上划线，
    // 别的一列都不带。识别本身是纯函数（`render::links`），这一条说的是**屏幕上**的事。
    let path = ".scratch/sandbox/eli5-sandbox.html";
    let url = "https://example.com/x";
    let mut state = state();
    state.apply(message(1, &format!("产物在 {path}，另见 {url}。"), None));
    let frame = buffer(120, 24, &mut state);
    let (row, text) = row_holding(&frame, path);

    let mut expected = vec![false; frame.area.width as usize];
    for target in [path, url] {
        let at = text.find(target).expect("两个目标都在这条正文行里");
        let start = text_columns(&text[..at]);
        let width = text_columns(target);
        expected[start..start + width].fill(true);
    }
    for (column, hot) in expected.iter().enumerate() {
        let cell = &frame[(column as u16, row)];
        assert_eq!(
            cell.modifier.contains(Modifier::UNDERLINED),
            *hot,
            "第 {column} 列该不该带下划线不对（这一行是 {text:?}）"
        );
    }
    assert!(
        expected.iter().filter(|hot| **hot).count() > 20,
        "两段地址都得认出来，不然这条测试什么都没测"
    );
}

// --- 链接热区：点一下就打开（`.scratch/clickable-links/spec.md` §1、§3） ------------

/// 工作目录就是**仓库根**的状态。
///
/// 这一组不能拿 `state()` 那个不存在的 `/x/heng`：热区里的路径要真存在、真在区内才解析
/// 得出目标（§3），而那个判据正是这几条测试要问的。
fn state_at_repo() -> TuiState {
    TuiState::new(
        facts(),
        std::env::current_dir().expect("测试的工作目录"),
        None,
    )
}

/// 一次拖选：按住、挪过门槛、松手。
fn drag(state: &mut TuiState, from: (u16, u16), to: (u16, u16)) {
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    for (kind, (column, row)) in [
        (MouseEventKind::Down(MouseButton::Left), from),
        (MouseEventKind::Drag(MouseButton::Left), to),
        (MouseEventKind::Up(MouseButton::Left), to),
    ] {
        state.mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        });
    }
}

#[test]
fn clicking_a_url_in_the_conversation_asks_to_open_it() {
    let mut state = state_at_repo();
    state.apply(message(1, "见 https://example.com/x 那一页", None));
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "https://example.com/x").expect("画在屏幕上");

    click(&mut state, column + 5, row);

    assert_eq!(
        state.take_open_request(),
        Some("https://example.com/x".to_owned()),
        "点 URL 中段：目标就是它本身"
    );
    assert_eq!(state.take_open_request(), None, "取走一次就没了");
}

#[test]
fn clicking_a_workspace_file_asks_to_open_its_absolute_path() {
    let mut state = state_at_repo();
    state.apply(message(1, "跑完了：Cargo.toml", None));
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "Cargo.toml").expect("画在屏幕上");

    click(&mut state, column + 2, row);

    let root = std::env::current_dir()
        .expect("测试的工作目录")
        .canonicalize()
        .expect("仓库根");
    assert_eq!(
        state.take_open_request(),
        Some(root.join("Cargo.toml").to_string_lossy().into_owned()),
        "文件传绝对路径，交给系统去挑程序"
    );
}

#[test]
fn clicking_plain_text_asks_for_nothing() {
    let mut state = state_at_repo();
    state.apply(message(1, "见 https://example.com/x 那一页", None));
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "https://example.com/x").expect("画在屏幕上");

    click(&mut state, column.saturating_sub(2), row);

    assert_eq!(state.take_open_request(), None, "点普通文字什么都不发生");
}

#[test]
fn a_missing_or_outside_path_asks_for_nothing() {
    for text in ["没有这个文件.html", "/etc/hostname"] {
        let mut state = state_at_repo();
        state.apply(message(1, text, None));
        let frame = buffer(120, 24, &mut state);
        let (column, row) = cell_of(&frame, 120, 24, text).expect("画在屏幕上");

        click(&mut state, column + 1, row);

        assert_eq!(state.take_open_request(), None, "{text} 不该被打开");
    }
}

#[test]
fn dragging_across_an_address_copies_it_instead_of_opening_it() {
    let mut state = state_at_repo();
    state.apply(message(1, "见 https://example.com/x 那一页", None));
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "https://example.com/x").expect("画在屏幕上");

    drag(&mut state, (column, row), (column + 8, row));

    assert_eq!(state.take_open_request(), None, "拖选永不打开");
    assert!(state.take_clipboard().is_some(), "拖选照旧只复制");
}

#[test]
fn clicking_away_from_the_conversation_asks_for_nothing() {
    let mut state = state_at_repo();
    state.apply(message(1, "见 https://example.com/x 那一页", None));
    let _ = buffer(120, 24, &mut state);

    // 左栏里的一格与提示行里的一格：两处都没有链接热区。
    for (column, row) in [(3, 3), (60, 23)] {
        click(&mut state, column, row);
        assert_eq!(
            state.take_open_request(),
            None,
            "({column}, {row}) 上什么都没有"
        );
    }
}

#[test]
fn the_receipt_of_an_open_lands_in_the_hint_row() {
    // 运行期把结果写回来（`note_open_receipt`），提示行照着说 —— 这一条跨的是那一小段路
    // （`.scratch/clickable-links/spec.md` §4）。成功与失败各说各的。
    let mut state = state_at_repo();
    state.apply(message(1, "见 https://example.com/x 那一页", None));
    assert!(
        !screen(120, 24, &mut state).join("\n").contains("已打开"),
        "还没点，什么都不该有"
    );

    for receipt in [
        wording::opened("https://example.com/x"),
        wording::open_failed("No such file or directory"),
    ] {
        state.note_open_receipt(receipt.clone());
        let rows = screen(120, 24, &mut state).join("\n");
        assert!(rows.contains(&receipt), "提示行该说 {receipt:?}：{rows}");
    }
}

// ---------------------------------------------------------------------------
// 用量尾巴与回合合计
// （ADR 0016、.scratch/trace-usage-tail/spec.md）
// ---------------------------------------------------------------------------

/// 一条 `TurnStarted`，迭代号由调用方给 —— 工具循环里的第二次调用就是 `iteration: 2`。
fn turn_started_at(seq: u64, iteration: u32) -> heng::render::RenderEvent {
    use heng::events::{Event, EventPayload, SpeakerId};
    heng::render::RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::TurnStarted {
            agent: SpeakerId::Debater("kimi".into()),
            iteration,
        },
    ))
}

#[test]
fn a_usage_tail_rides_the_row_of_its_call() {
    // 一笔用量不再是自己的块：它长在产生它的那次调用的行尾，而独立的那一行没有了
    // （ADR 0016）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(turn_started(1));
    state.apply(reasoning_delta("先看依赖，"));
    state.apply(text_delta("答案是 42。"));
    state.apply(usage(2, 10, 2, 6, 4));
    state.apply(message(3, "答案是 42。", Some("先看依赖，")));
    open_trace_tab(&mut state, 120, 24);

    let text = trace_page(&mut state, 120, 24).join("\n");
    assert!(
        text.contains("✓ 思考完成 in=10 out=2"),
        "尾巴长在它那次调用的行上：{text}"
    );
    assert!(!text.contains("用量 in="), "独立的那一行没有了：{text}");
    assert!(
        !text.contains("cached="),
        "缓存那一半留在诊断通道里：{text}"
    );
}

#[test]
fn a_usage_tail_lands_on_the_turn_row_when_nothing_was_thought() {
    // 这次调用没有推理：它最后画出来的那条过程行是回合的开始行，尾巴就长在那儿。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(turn_started(1));
    state.apply(usage(2, 10, 2, 6, 4));
    state.apply(message(3, "答案是 42。", None));
    open_trace_tab(&mut state, 120, 24);

    let text = trace_page(&mut state, 120, 24).join("\n");
    assert!(
        text.contains("回合开始（第 1 次迭代） in=10 out=2"),
        "没有思考行时它跟在这次调用的第一行上：{text}"
    );
    assert!(!text.contains("用量 in="), "仍然没有独立的一行：{text}");
}

#[test]
fn each_call_carries_its_own_tail_and_the_turn_sums_them() {
    // 一次发言跨两次模型调用：两笔各长在各次调用的行上，收尾那行给一条合计（§5）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(turn_started(1));
    state.apply(reasoning_delta("第一次调用在调工具。"));
    state.apply(usage(2, 10, 2, 6, 4));
    state.apply(message(3, "", Some("第一次调用在调工具。")));
    state.apply(tool_started(
        4,
        "call-1",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(5, "call-1", true, Some("ok"), None));
    state.apply(turn_started_at(6, 2));
    state.apply(reasoning_delta("第二次调用收尾。"));
    state.apply(text_delta("答案是 42。"));
    state.apply(usage(7, 5, 1, 0, 5));
    state.apply(message(8, "答案是 42。", Some("第二次调用收尾。")));
    state.apply(turn_ended(9));
    open_trace_tab(&mut state, 120, 24);

    let rows = trace_page(&mut state, 120, 24);
    let text = rows.join("\n");
    let first = rows
        .iter()
        .find(|row| row.contains("in=10 out=2"))
        .unwrap_or_else(|| panic!("第一笔在屏幕上：{text}"));
    assert!(
        first.contains("思考完成"),
        "第一笔长在它那次调用的行上：{first}"
    );
    let second = rows
        .iter()
        .find(|row| row.contains("in=5 out=1"))
        .unwrap_or_else(|| panic!("第二笔在屏幕上：{text}"));
    assert!(second.contains("思考完成"), "第二笔同理：{second}");
    let total = rows
        .iter()
        .find(|row| row.contains("合计 in=15 out=3"))
        .unwrap_or_else(|| panic!("合计在屏幕上：{text}"));
    assert!(total.contains("回合结束"), "合计并进收尾那一行：{total}");
}

#[test]
fn a_single_call_turn_carries_no_total() {
    // 只调用一次时那笔数字就在尾巴上，再加一条合计是同一个数印两遍（§5）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(turn_started(1));
    state.apply(reasoning_delta("想一下。"));
    state.apply(text_delta("答案是 42。"));
    state.apply(usage(2, 10, 2, 6, 4));
    state.apply(message(3, "答案是 42。", Some("想一下。")));
    state.apply(turn_ended(4));
    open_trace_tab(&mut state, 120, 24);

    let text = trace_page(&mut state, 120, 24).join("\n");
    assert!(text.contains("in=10 out=2"), "尾巴还在：{text}");
    assert!(!text.contains("合计"), "没有合计：{text}");
}

#[test]
fn a_call_that_never_wrote_prose_keeps_its_usage() {
    // 只推理、不产出正文的那次调用：用量到的时候思考行还开着，定稿是整行重写 —— 它得跟着
    // 这条行走，不能被吞掉，也不能被顶掉（§7 的回归一）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(turn_started(1));
    state.apply(reasoning_delta("只想了，没写。"));
    state.apply(usage(2, 10, 2, 6, 4));
    state.apply(message(3, "", Some("只想了，没写。")));
    open_trace_tab(&mut state, 120, 24);

    let text = trace_page(&mut state, 120, 24).join("\n");
    assert!(
        text.contains("✓ 思考完成 in=10 out=2"),
        "定稿没有把这笔用量吞掉：{text}"
    );
    assert_eq!(
        text.matches("思考完成").count(),
        1,
        "一段思考就是一行：{text}"
    );
    assert!(!text.contains("正在思考"), "也没有留下进行中那行：{text}");
}

#[test]
fn a_width_change_replays_the_usage_tail_once() {
    // 尾巴跟着绘制记录走，所以换宽度重排之后它还在、而且只有一份（§2）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(turn_started(1));
    state.apply(reasoning_delta("先看依赖，"));
    state.apply(text_delta("答案是 42。"));
    state.apply(usage(2, 10, 2, 6, 4));
    state.apply(message(3, "答案是 42。", Some("先看依赖，")));
    open_trace_tab(&mut state, 120, 24);
    let _ = trace_page(&mut state, 120, 24);

    let text = trace_page(&mut state, 100, 24).join("\n");
    assert_eq!(
        text.matches("in=10 out=2").count(),
        1,
        "重排之后尾巴不多不少：{text}"
    );
    assert!(!text.contains("正在思考"), "也没有留下进行中那行：{text}");
}

#[test]
fn a_synthesizer_usage_keeps_its_own_row() {
    // 合成器那次调用不属于任何回合，没有可承载的宿主 —— 它照旧自己占一行（§4）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(usage(1, 10, 2, 6, 4));
    open_trace_tab(&mut state, 120, 24);

    let text = trace_page(&mut state, 120, 24).join("\n");
    assert!(
        text.contains("用量 in=10 out=2 cached=6 miss=4"),
        "合成器那笔仍是独立一行：{text}"
    );
}

#[test]
fn a_narrow_trace_wraps_the_tail_instead_of_dropping_it() {
    // 窄档照折行，不省略、也不消失（§6）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(turn_started(1));
    state.apply(reasoning_delta("先看依赖，"));
    state.apply(text_delta("答案是 42。"));
    state.apply(usage(2, 10, 2, 6, 4));
    state.apply(message(3, "答案是 42。", Some("先看依赖，")));
    open_trace_tab(&mut state, 80, 24);

    let text = trace_page(&mut state, 80, 24).join("\n");
    assert!(text.contains("in=10"), "窄档里这笔数字还在：{text}");
}

#[test]
fn the_usage_tail_does_not_take_over_the_rows_detail() {
    // 尾巴是纯文本：那一行点开还是它自己的详情（§6）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(turn_started(1));
    state.apply(reasoning_delta("先看依赖，"));
    state.apply(text_delta("答案是 42。"));
    state.apply(usage(2, 10, 2, 6, 4));
    state.apply(message(3, "答案是 42。", Some("先看依赖，")));
    open_trace_tab(&mut state, 120, 24);

    click_row(&mut state, 120, 24, "思考完成 in=10 out=2");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains(THINKING_FACES), "弹的还是思考详情：{text}");
    assert!(text.contains("先看依赖，"), "而且带全文：{text}");
}

#[test]
fn a_replayed_call_that_never_wrote_prose_keeps_one_thinking_row() {
    // 回归二：只推理、不产出正文的那次调用，换宽度重排之后不会多出一条永不消失的
    // 「正在思考」（§7）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(turn_started(1));
    state.apply(reasoning_delta("只想了，没写。"));
    state.apply(usage(2, 10, 2, 6, 4));
    state.apply(message(3, "", Some("只想了，没写。")));
    open_trace_tab(&mut state, 120, 24);
    let _ = trace_page(&mut state, 120, 24);

    let text = trace_page(&mut state, 100, 24).join("\n");
    assert_eq!(text.matches("思考完成").count(), 1, "定稿只有一条：{text}");
    assert!(!text.contains("正在思考"), "没有卡住的那一行：{text}");
    assert_eq!(text.matches("in=10 out=2").count(), 1, "尾巴也还在：{text}");
}

// ---------------------------------------------------------------------------
// 会话中途换模型与思考强度（`.scratch/model-switching/spec.md`）
// ---------------------------------------------------------------------------

/// 循环推回来的那条新事实（spec §3）。
fn session_update(
    model: &str,
    effort: Option<ReasoningEffort>,
    window: u64,
    speakers: &[&str],
) -> ConsoleRequest {
    ConsoleRequest::SessionUpdate {
        model: model.to_owned(),
        effort,
        context_window: window,
        speakers: speakers.iter().map(|name| (*name).to_owned()).collect(),
    }
}

/// 造一份清单：`label`、`detail` 与「当前」那一行。
fn picker(
    title: &str,
    options: &[(&str, bool)],
    current: usize,
) -> (
    ConsoleRequest,
    tokio::sync::oneshot::Receiver<Option<usize>>,
) {
    let owned: Vec<(&str, bool)> = options.to_vec();
    picker_detailed(title, &owned, current, |_| String::new())
}

/// 同一件事，但每一条的 `detail` 由调用方给 —— 表格第二列（profile）就靠它。
fn picker_detailed(
    title: &str,
    options: &[(&str, bool)],
    current: usize,
    detail: impl Fn(&str) -> String,
) -> (
    ConsoleRequest,
    tokio::sync::oneshot::Receiver<Option<usize>>,
) {
    let (reply, answer) = tokio::sync::oneshot::channel();
    (
        ConsoleRequest::Picker(heng::render::PickerRequest {
            title: title.to_owned(),
            options: options
                .iter()
                .enumerate()
                .map(|(index, (label, enabled))| heng::render::PickerOption {
                    label: (*label).to_owned(),
                    detail: detail(label),
                    current: index == current,
                    enabled: *enabled,
                })
                .collect(),
            reply,
        }),
        answer,
    )
}

#[test]
fn the_status_row_shows_the_effort_and_re_rewrites_it_when_the_session_moves() {
    let mut state = idle();
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("模型 claude-sonnet-4-5"),
        "状态行报出模型：{text}"
    );

    // 循环推回来的新事实：模型、档位、窗口与名册一起改（spec §3）。
    state.request(session_update(
        "MiniMax-M3.1-Flash-Preview",
        Some(ReasoningEffort::Xhigh),
        1_000_000,
        &["minimax-cn"],
    ));
    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(
        text.contains("模型 MiniMax-M3.1-Flash-Preview · xhigh"),
        "状态行改写成了新模型与新档位：{text}"
    );
    // 上下文那一栏的分母也换了 —— 它本来就是新模型的能力事实。
    let status = rows
        .iter()
        .find(|row| row.contains("上下文"))
        .expect("状态行");
    assert!(
        !status.contains("%") || status.contains("上下文"),
        "{status}"
    );
}

#[test]
fn the_two_status_cells_open_their_picker_and_nothing_else_on_that_row_does() {
    let mut state = idle();
    state.request(session_update(
        "kimi-k3",
        Some(ReasoningEffort::High),
        200_000,
        &["kimi"],
    ));

    // 点模型那一格 → 要开模型清单。
    click_text(&mut state, 120, 24, "kimi-k3");
    assert_eq!(
        state.take_events(),
        vec![FrontEndEvent::OpenPicker(heng::render::PickerKind::Model)]
    );

    // 点档位那一格 → 要开档位清单。
    let mut state = idle();
    state.request(session_update(
        "kimi-k3",
        Some(ReasoningEffort::High),
        200_000,
        &["kimi"],
    ));
    click_text(&mut state, 120, 24, "high");
    assert_eq!(
        state.take_events(),
        vec![FrontEndEvent::OpenPicker(heng::render::PickerKind::Effort)]
    );

    // 状态词与标签不是可点的：它们什么都不上行。
    let mut state = idle();
    state.request(session_update(
        "kimi-k3",
        Some(ReasoningEffort::High),
        200_000,
        &["kimi"],
    ));
    click_text(&mut state, 120, 24, "就绪");
    assert!(state.take_events().is_empty(), "状态词那一格不点得动");
}

#[test]
fn clicking_a_status_cell_does_not_take_the_keyboard_away_from_the_input() {
    // 点状态行**不是**「点输入区」：那一格要上行一个手势，但键盘仍然归输入区，
    // 所以下一个可打印字符仍然落在草稿里。
    let mut state = idle();
    click_text(&mut state, 120, 24, "claude-sonnet-4-5");
    assert_eq!(
        state.take_events(),
        vec![FrontEndEvent::OpenPicker(heng::render::PickerKind::Model)]
    );
    state.key(Key::Char('x'));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains('x'), "键盘还在输入区：{text}");
}

#[test]
fn a_discussion_session_only_says_it_cannot_switch() {
    // 讨论会话的模型是**两个模型的拼法**，那是名册里的配置事实，不是一场活会话能中途
    // 改的值（spec §12）。
    let mut state = {
        let mut facts = facts();
        facts.model = "kimi-k3 + deepseek-v4-pro".to_owned();
        facts.switchable = false;
        let (reply, line) = tokio::sync::oneshot::channel();
        let mut state = TuiState::new(facts, std::path::PathBuf::from("/x/heng"), None);
        state.request(ConsoleRequest::Prompt { reply });
        drop(line);
        state
    };
    let mut events = state.take_events();
    events.clear();
    click_text(&mut state, 120, 24, "kimi-k3 + deepseek-v4-pro");
    assert!(events.is_empty(), "占位");
    assert!(state.take_events().is_empty(), "讨论会话不上行切换手势");
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains(&wording::heng(wording::switch_not_switchable())),
        "它给的是一句说明：{text}"
    );
}

#[test]
fn ctrl_t_asks_for_the_model_picker() {
    let mut state = idle();
    state.key(Key::CtrlT);
    assert_eq!(
        state.take_events(),
        vec![FrontEndEvent::OpenPicker(heng::render::PickerKind::Model)]
    );
}

#[test]
fn the_picker_starts_on_the_current_row_and_answers_with_the_highlighted_one() {
    let mut state = idle();
    let (request, mut answer) = picker("模型", &[("kimi-k3", true), ("deepseek-v4-pro", true)], 1);
    state.request(request);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("模型"), "浮层有标题：{text}");
    assert!(
        text.contains("kimi-k3") && text.contains("deepseek-v4-pro"),
        "{text}"
    );

    // 往下走到头就停住，不绕回。
    state.key(Key::Char('j'));
    state.key(Key::Char('j'));
    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().expect("答案送出去了"),
        Some(1),
        "回的是高亮那一行的下标"
    );
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains("deepseek-v4-pro"), "浮层关上了：{text}");
}

#[test]
fn a_disabled_picker_row_does_not_answer_and_esc_cancels() {
    let mut state = idle();
    let (request, mut answer) = picker("模型", &[("kimi-k3", true), ("MiniMax-M3", false)], 0);
    state.request(request);

    // 第二行是禁用的：回车在上面不响应。
    state.key(Key::Char('j'));
    state.key(Key::Enter);
    assert!(answer.try_recv().is_err(), "禁用的那一行不回答，浮层还立着");
    // 然后取消。
    state.key(Key::Esc);
    assert_eq!(answer.try_recv().expect("取消送出去了"), None);

    // 键盘回到输入区。
    state.key(Key::Char('y'));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains('y'), "浮层关掉后打字照旧落进草稿：{text}");
}

#[test]
fn the_picker_takes_the_keyboard_while_it_is_up() {
    let mut state = idle();
    let (request, _answer) = picker("思考强度", &[("默认", true), ("high", true)], 0);
    state.request(request);
    // 立着的时候可打印字符不进文本（这一格被它占着）。
    state.key(Key::Char('z'));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains('z'), "打字落不到草稿上：{text}");
    // `ctrl-t` 在它立着时不再上行。
    assert!(state.take_events().is_empty());
    state.key(Key::CtrlT);
    assert!(
        state.take_events().is_empty(),
        "选择器立着时 ctrl-t 归它，不上行"
    );
}

#[test]
fn clicking_outside_the_picker_cancels_it() {
    let mut state = idle();
    let (request, mut answer) = picker("模型", &[("kimi-k3", true)], 0);
    state.request(request);
    // 点框外（第一行是页签条，那儿不属于浮层）。
    click(&mut state, 2, 0);
    assert_eq!(answer.try_recv().expect("取消送出去了"), None);
    let text = screen(120, 24, &mut state).join("\n");
    // 候选名只出现在浮层上（状态行那一格还是 `claude-sonnet-4-5`），所以它是「浮层还在不在」
    // 的判据。
    assert!(!text.contains("kimi-k3"), "浮层关上了：{text}");
}

#[test]
fn only_the_current_picker_row_carries_the_current_marker() {
    // 当前项一个 `▸`，而第三列的「状态」把它说成文字 —— 两处说同一件事：一处给位置、一处给读法。
    let mut state = idle();
    let (request, _answer) = picker(
        "模型",
        &[
            ("kimi-k3", true),
            ("MiniMax-M3", false),
            ("deepseek-v4-pro", true),
        ],
        2,
    );
    state.request(request);
    let rows = screen(120, 24, &mut state);
    let current = rows
        .iter()
        .find(|row| row.contains('▸'))
        .expect("当前那一行带 `▸`");
    assert!(
        current.contains("▸ deepseek-v4-pro"),
        "`▸` 跟着当前那个模型：{current}"
    );
    assert!(
        current.contains(wording::picker_status_current()),
        "状态列写「当前」：{current}"
    );
    assert!(
        !rows
            .iter()
            .any(|row| row.contains('▸') && row.contains("kimi-k3")),
        "不是当前的那些不带 `▸`"
    );
    // 选不了的那行没有记号（表格第三列已经说了状态），但它点了不响应。
    let disabled = rows
        .iter()
        .find(|row| row.contains("MiniMax-M3") && !row.contains('▸'))
        .expect("禁用那一行");
    assert!(
        !disabled.contains(wording::picker_status_current()),
        "它不是当前：{disabled}"
    );
}

#[test]
fn the_picker_lays_one_model_per_row_in_three_columns() {
    // 一行一个模型，三列是 `模型 id ┆ profile ┆ 状态` —— 表格要对齐，所以短的那些右面留白。
    let labels = [
        ("kimi-k3", true),
        ("k3-256k", true),
        ("deepseek-v4-pro", true),
    ];
    let mut state = idle();
    let (request, _answer) = picker("模型", &labels, 0);
    state.request(request);
    let rows = screen(120, 24, &mut state);
    let body: Vec<&String> = rows
        .iter()
        // 左栏的分隔列也是 `┆`，所以还要看第三列的状态词 —— 那才是浮层的行。
        .filter(|row| {
            row.contains(wording::PICKER_COLUMN)
                && (row.contains(wording::picker_status_current())
                    || row.contains(wording::picker_status_switchable()))
        })
        .collect();
    assert_eq!(body.len(), 3, "三个候选各占一行：{rows:#?}");
    for row in &body {
        // 一行只放得下一个模型 id。
        let hits = labels
            .iter()
            .filter(|(label, _)| row.contains(label))
            .count();
        assert_eq!(hits, 1, "一行一个模型：{row}");
        // 状态词**只出现一次** —— 它是第三列，而第一列与第二列后面都不该再冒一个出来
        // （回归：那条 `match index { 1 => …, _ => status }` 会在第 0 列后面也画一次）。
        let statuses = row.matches(wording::picker_status_current()).count()
            + row.matches(wording::picker_status_switchable()).count();
        assert_eq!(statuses, 1, "一行只有一个状态词：{row}");
    }
    // 第二列说它走哪个 profile（`detail` 就是那一列）—— 一条模型 id 与一条 profile 同行。
    let (request, mut answer) = picker_detailed(
        "模型",
        &[("kimi-k3", true), ("deepseek-v4-pro", true)],
        0,
        |label| {
            wording::model_detail_profile(if label == "kimi-k3" {
                "kimi"
            } else {
                "deepseek"
            })
        },
    );
    state.request(request);
    let rows = screen(120, 24, &mut state);
    let body: Vec<&String> = rows
        .iter()
        .filter(|row| row.contains('`') && row.contains(wording::PICKER_COLUMN))
        .collect();
    assert_eq!(body.len(), 2, "两行：{rows:#?}");
    // 第二列只给 profile 的名字（反引号包住），不加「走」那种动词。
    assert!(
        body[0].contains("kimi-k3") && body[0].contains("`kimi`"),
        "模型 id 与它的 profile 同行：{}",
        body[0]
    );
    assert!(
        !body[0].contains("走 `"),
        "第二列不再有那个动词：{}",
        body[0]
    );
    assert!(
        body[1].contains("deepseek-v4-pro") && body[1].contains("`deepseek`"),
        "模型 id 与它的 profile 同行：{}",
        body[0]
    );
    assert!(
        body[1].contains("deepseek-v4-pro") && body[1].contains("`deepseek`"),
        "第二行同样：{}",
        body[1]
    );
    let _ = answer.try_recv();

    // 三列对齐：第二列（profile）在每一行上都从同一列起。
    let profile_column = body
        .iter()
        .map(|row| {
            let at = row.rfind(wording::PICKER_COLUMN).expect("第二列");
            text_columns(row[..at].trim_end())
        })
        .collect::<Vec<_>>();
    assert!(
        profile_column.windows(2).all(|pair| pair[0] == pair[1]),
        "第二列逐行对齐：{profile_column:?}"
    );

    // 一行一个模型，所以横向与纵向是同一件事：`↓` 就是下一行。
    state.key(Key::Down);
    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().expect("答案送出去了"),
        Some(1),
        "`↓` 走到下一行"
    );
}

#[test]
fn clicking_a_row_inside_the_picker_selects_that_row() {
    let mut state = idle();
    let (request, mut answer) = picker("模型", &[("kimi-k3", true), ("deepseek-v4-pro", true)], 0);
    state.request(request);
    // 先画一帧，才知道第二行画在哪儿 —— 只有画出来的行点得到。
    let frame = buffer(120, 24, &mut state);
    let row = (0..24)
        .find(|y| row_text(&frame, *y, 120).contains("deepseek-v4-pro"))
        .expect("第二行在屏幕上");
    let column = row_text(&frame, row, 120)
        .find("deepseek-v4-pro")
        .map(|at| text_columns(&row_text(&frame, row, 120)[..at]) as u16)
        .expect("它在");
    click(&mut state, column, row);
    assert_eq!(
        answer.try_recv().expect("答案送出去了"),
        Some(1),
        "框内点一行等于选中那行"
    );
}

#[test]
fn a_long_picker_scrolls_and_answers_with_the_highlighted_row() {
    // 候选多过浮层的高度：它在内部滚动，于是回车回的永远是**高亮那一行**而不是第几行。
    let mut state = idle();
    let owned: Vec<String> = (0..30).map(|i| format!("模型-{i:02}")).collect();
    let labels: Vec<(&str, bool)> = owned.iter().map(|label| (label.as_str(), true)).collect();
    let (request, mut answer) = picker("模型", &labels, 0);
    state.request(request);
    // 往下走二十九格 —— 浮层只有几行高，于是它滚过去了。
    for _ in 0..29 {
        state.key(Key::Char('j'));
    }
    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().expect("答案送出去了"),
        Some(29),
        "回的是高亮那一行的下标"
    );
}

#[test]
fn after_escaping_the_picker_escape_is_free_to_raise_the_exit_gesture() {
    // 选择器关掉之后不留下任何会吃掉下一次 `Esc` 的东西：举手退出那两下照旧能用。
    let mut state = idle();
    let (request, mut answer) = picker("模型", &[("kimi-k3", true)], 0);
    state.request(request);
    state.key(Key::Esc);
    assert_eq!(answer.try_recv().expect("取消送出去了"), None);
    assert!(state.take_events().is_empty(), "取消不发任何手势");

    // 举手那两下照旧：`Ctrl-C` 第一下只举手（提示行出现「再按一次」），第二下才退。
    assert!(state.take_events().is_empty());
    state.key(Key::CtrlC);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("再按一次 ctrl-c/ctrl-d 退出"),
        "退出举手照旧举得起来：{text}"
    );
    state.key(Key::CtrlC);
    assert!(state.should_quit(), "第二下 `Ctrl-C` 真的退出");
}

#[test]
fn a_fixed_effort_model_says_instead_of_opening_an_empty_picker() {
    // 那个模型没有档位旋钮，所以点它只给一句说明（spec §8），不是一块空浮层。
    let mut state = {
        let mut facts = facts();
        facts.model = "MiniMax-M3".to_owned();
        let (reply, line) = tokio::sync::oneshot::channel();
        let mut state = TuiState::new(facts, std::path::PathBuf::from("/x/heng"), None);
        state.request(ConsoleRequest::Prompt { reply });
        drop(line);
        state
    };
    let _ = state.take_events();
    click_text(&mut state, 120, 24, "固定");
    assert!(state.take_events().is_empty(), "没有可开的清单");
    let rows = screen(120, 24, &mut state);
    let hint = rows.last().expect("有一行");
    assert!(
        hint.contains("没有档位旋钮，思考常开"),
        "提示行那一行是那句说明：{hint:?}"
    );
    // 而模型那一格照旧可点。
    let mut state = {
        let mut facts = facts();
        facts.model = "MiniMax-M3".to_owned();
        let (reply, line) = tokio::sync::oneshot::channel();
        let mut state = TuiState::new(facts, std::path::PathBuf::from("/x/heng"), None);
        state.request(ConsoleRequest::Prompt { reply });
        drop(line);
        state
    };
    let _ = state.take_events();
    click_text(&mut state, 120, 24, "MiniMax-M3");
    assert_eq!(
        state.take_events(),
        vec![FrontEndEvent::OpenPicker(heng::render::PickerKind::Model)]
    );
}

#[test]
fn a_new_speaker_name_takes_an_unclaimed_colour_and_the_old_one_keeps_its() {
    // 换 provider 就换发言者的名字：新的走「没人认领的槽位」，旧的颜色**不动**
    // （`.scratch/model-switching/spec.md` §4）。
    let mut state = {
        let mut facts = facts();
        facts.speaker_order = vec!["kimi".to_owned()];
        let (reply, line) = tokio::sync::oneshot::channel();
        let mut state = TuiState::new(facts, std::path::PathBuf::from("/x/heng"), None);
        state.request(ConsoleRequest::Prompt { reply });
        drop(line);
        state
    };
    state.apply(message(8, "kimi 答的", None));
    let before = screen(120, 24, &mut state);
    let frame_before = buffer(120, 24, &mut state);

    state.request(session_update(
        "MiniMax-M3.1-Flash-Preview",
        Some(ReasoningEffort::High),
        1_048_576,
        &["minimax-cn"],
    ));
    let _ = screen(120, 24, &mut state);

    // 换名之后新发言者的第一行拿到**另一个**颜色，而转录里已经画过的行不改名。
    state.apply(RenderEvent::Logged(heng::events::Event::new(
        9,
        heng::events::SpeakerId::Debater("minimax-cn".into()),
        heng::events::EventPayload::MessageCompleted {
            role: heng::events::Role::Assistant,
            text: "minimax 答的".to_owned(),
            reasoning: None,

            first_token_ms: None,
        },
    )));
    let rows = screen(120, 24, &mut state);
    let frame = buffer(120, 24, &mut state);
    let colour_at = |frame: &ratatui::buffer::Buffer, needle: &str| {
        (0..24)
            .find_map(|y| {
                let row = row_text(frame, y, 120);
                row.find(needle).map(|at| {
                    (
                        text_columns(&row[..at]) as u16,
                        y,
                        frame[(text_columns(&row[..at]) as u16, y)].fg,
                    )
                })
            })
            .map(|(_, _, colour)| colour)
    };
    let old = colour_at(&frame_before, "kimi");
    let new = colour_at(&frame, "minimax-cn");
    assert!(old.is_some(), "换之前那条发言在屏幕上");
    assert!(new.is_some(), "换之后那条发言在屏幕上");
    assert_ne!(old, new, "新名字没有复用旧名字的颜色槽位");
    assert!(
        rows.join("\n").contains("kimi") || !before.join("\n").contains("kimi"),
        "已经画过的行不改名：{:?}",
        rows.iter()
            .filter(|row| row.contains("kimi"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_trace_row_opens_its_detail_while_a_questionnaire_is_up() {
    // 读模型做决定之前想看一眼上一次调用发生了什么 —— 覆盖层以前在问卷立着时打不开
    // （`questioning` 那一串门），于是只能等这一轮问完再看
    // （`.scratch/questionnaire-reading/spec.md`）。
    use heng::questions::{Choice, UserQuestion};
    let (mut state, _answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![Choice {
            label: "甲".to_owned(),
            description: None,
        }],
    });
    state.apply(tool_started(
        1,
        "call-1",
        "bash",
        serde_json::json!({"command": "ls -la"}),
    ));
    state.apply(tool_completed(2, "call-1", true, Some("全部文件"), None));
    open_trace_tab(&mut state, 120, 24);
    click_text(&mut state, 120, 24, "调用 bash");

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("ls -la"), "详情覆盖层开着：{text}");
    // 浮层的底边被挡在问卷上面，所以那道题始终看得见 —— 一个看不见题面的问卷等于答不了。
    assert!(text.contains("选一个"), "题面还在底部：{text}");
    assert!(text.contains("1. 甲"), "选项也在：{text}");
}

#[test]
fn the_detail_holds_the_keyboard_and_closing_it_hands_the_questionnaire_back() {
    // 键盘跟着**当前那一层**：详情立着时 `Esc` 关的是它（不是「退出这次询问」），而关掉
    // 之后问卷的作答与焦点原样留在那儿（`.scratch/questionnaire-reading/spec.md`）。
    use heng::questions::{Choice, UserQuestion};
    let (mut state, mut answers) = questionnaire_state(UserQuestion {
        id: "q1".to_owned(),
        header: None,
        question: "选一个".to_owned(),
        multi_select: false,
        options: vec![Choice {
            label: "甲".to_owned(),
            description: None,
        }],
    });
    state.apply(tool_started(
        1,
        "call-1",
        "bash",
        serde_json::json!({"command": "ls -la"}),
    ));
    state.apply(tool_completed(2, "call-1", true, Some("全部文件"), None));
    open_trace_tab(&mut state, 120, 24);
    click_text(&mut state, 120, 24, "调用 bash");

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("ls -la"), "详情开着：{text}");

    // 键盘跟着当前那一层：空格（确认）与回车（提交）都落在详情里，问卷既没作答也没提交。
    state.key(Key::Char(' '));
    state.key(Key::Enter);
    assert!(
        answers.try_recv().is_err(),
        "详情占着键盘，问卷既没被作答也没被提交"
    );

    // `Esc` 关掉的是详情（它排在问卷那一支前面），而下面那道题的作答与焦点原样留着。
    state.key(Key::Esc);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains("ls -la"), "详情关掉了：{text}");

    // 键盘回到问卷：高亮仍在第一项，于是空格确认它、回车提交。
    state.key(Key::Char(' '));
    state.key(Key::Enter);
    let answers = answers.try_recv().expect("问卷提交了");
    assert_eq!(
        answers.expect("一次成功的作答").answers[0].selected,
        vec!["甲".to_owned()]
    );
}

#[test]
fn a_file_row_in_the_sidebar_opens_its_content_while_a_questionnaire_is_up() {
    // 文件页与轨迹页是同一类：点一行开详情覆盖层。以前问卷立着时这一列被 `questioning`
    // 挡着，连目录都展不开（`.scratch/questionnaire-reading/spec.md`）。
    let dir = workspace("questionnaire-files");
    std::fs::write(dir.join("hello.rs"), "fn main() {}\n").expect("写一个文件");
    let mut state = files_in(&dir, &["hello.rs"]);
    use heng::render::QuestionnaireRequest;
    let (reply, _answers) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Questionnaire(QuestionnaireRequest {
        questions: vec![heng::questions::UserQuestion {
            id: "q1".to_owned(),
            header: None,
            question: "选一个".to_owned(),
            multi_select: false,
            options: vec![heng::questions::Choice {
                label: "甲".to_owned(),
                description: None,
            }],
        }],
        reply,
    }));
    open_files_page(&mut state, 120, 24);
    click_row(&mut state, 120, 24, "hello.rs");

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("fn main() {}"), "文件内容画出来了：{text}");
    assert!(text.contains("选一个"), "题面还在底部：{text}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_overlay_stops_above_the_questionnaire_while_one_is_up() {
    // 纯几何那一半：浮层的底边就是问卷块顶上的那道线（`.scratch/questionnaire-reading/spec.md`）。
    // 看不见题面的问卷等于答不了 —— 所以这条比「点得开」更要紧。
    use heng::render::layout::{plan, plan_questionnaire};
    use ratatui::layout::Rect;

    let area = Rect::new(0, 0, 120, 24);
    let plain = plan(area, 1, true);
    let asked = plan_questionnaire(area, 6, true);

    // 平时地板就是屏幕底边，于是浮层那块形状一个字不变：上下各留两行转录。
    assert_eq!(plain.overlay_floor, area.y + area.height);
    assert_eq!(plain.detail().expect("有地方画").bottom(), area.height - 2);

    // 问卷立着时地板抬到它上面，浮层在**自己那段高度**里居中。
    assert_eq!(asked.overlay_floor, asked.input.y, "地板是问卷块的顶边");
    let overlay = asked.detail().expect("有地方画");
    assert!(
        overlay.bottom() <= asked.input.y,
        "浮层底边 {}{} 压住了第 {} 行的问卷",
        overlay.y,
        overlay.height,
        asked.input.y
    );
    assert_eq!(
        overlay.y,
        asked.input.y.saturating_sub(overlay.height) / 2,
        "它在可用高度里居中，而不是盖在上面"
    );
}

// ---------------------------------------------------------------------------
// 详情覆盖层的面（票 13）
// ---------------------------------------------------------------------------

/// 一个固定的时刻：计时面的那几条断言只拿它**算差**，不读它的字面值（那是本地时区的事）。
fn a_call_start() -> chrono::DateTime<chrono::Utc> {
    fixed_at(9, 12, 11)
}

/// 一次模型调用：起点、一笔用量、一条带首 token 时刻的完成 —— 8 秒前开始、8 秒后完成，
/// 首 token 等了 7.4 秒，报出来 222 个输出 token。
fn a_call_with_a_first_token(state: &mut TuiState) {
    use heng::events::{EventPayload, Role, Usage};
    let start = a_call_start();
    state.apply(at_event(
        1,
        start,
        EventPayload::TurnStarted {
            agent: kimi(),
            iteration: 1,
        },
    ));
    state.apply(at_event(
        2,
        start + chrono::Duration::milliseconds(200),
        EventPayload::UsageRecorded {
            usage: Usage {
                input_tokens: 12_000,
                output_tokens: 222,
                cached_tokens: 0,
                miss_tokens: 0,
                reasoning_tokens: None,
            },
        },
    ));
    state.apply(at_event(
        3,
        start + chrono::Duration::milliseconds(8_000),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: "答案是 42。".to_owned(),
            reasoning: None,
            first_token_ms: Some(7_400),
        },
    ));
}

/// 切到「计时」那一面：打开时落在默认面上，`Tab` 走一格。
fn tab_to_timing(state: &mut TuiState) {
    next_face(state);
}

#[test]
fn a_message_detail_walks_its_faces_with_the_tab_key() {
    // 一份消息详情有三个面：正文（默认面，票 13 第 5 条）、计时、概述。`Tab` 一路走，
    // `Shift+Tab` 走回来，而标签条上写着现在在哪一面、有哪几面（票 13 第 3 条）。
    let mut state = state_with_roster(&["kimi"]);
    a_call_with_a_first_token(&mut state);
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "答案是 42。");

    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains(MESSAGE_FACES), "顶上那条标签条：{text}");
    assert!(text.contains("答案是 42。"), "打开时落在正文上：{text}");

    next_face(&mut state);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("开始时刻"), "第二面是计时：{text}");
    // 正文那一面换掉了：整屏只剩**底下那条转录行**里的那一处 —— 覆盖层不再画它。这里数
    // 次数而不是问「有没有」，因为底下的行本来就写着同一句话，一句 `!contains` 证明不了
    // 这一面换了。
    assert_eq!(
        text.matches("答案是 42。").count(),
        1,
        "这一面换了，正文只剩底下那条转录行：{text}"
    );

    next_face(&mut state);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("一条消息"), "第三面是概述：{text}");

    // `Tab` 是环形的：再走一格回到正文。
    next_face(&mut state);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("答案是 42。"), "绕过末尾回到第一面：{text}");

    // `Shift+Tab` 往回走一格：正文 → 概述（最后一面）。
    state.key(Key::BackTab);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("一条消息"), "往回的 `Tab`：{text}");
}

#[test]
fn the_timing_face_reads_the_numbers_off_this_call() {
    // 计时面给的是**精确值**（与时间轴那个「给形状」的分工）：开始时刻 / 总时长 /
    // 首 token / 生成 / 吞吐，末行写清是哪两个时刻相减（票 13 第 6 条）。
    let mut state = state_with_roster(&["kimi"]);
    a_call_with_a_first_token(&mut state);
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "答案是 42。");
    tab_to_timing(&mut state);

    let text = screen(120, 40, &mut state).join("\n");
    for needle in ["开始时刻", "总时长", "首 token", "生成", "吞吐", "计时来源"] {
        assert!(text.contains(needle), "计时面读得到「{needle}」：{text}");
    }
    assert!(text.contains("8.0 s"), "总时长 = 完成 − 起点：{text}");
    assert!(text.contains("7.4 s"), "首 token 是流上那一笔：{text}");
    assert!(text.contains("0.6 s"), "生成 = 总时长 − 首 token：{text}");
    assert!(
        text.contains("370 token/s") && text.contains("输出 222"),
        "吞吐 = 输出 token ÷ 生成时长：{text}"
    );
    assert!(
        text.contains("TurnStarted.at → MessageCompleted.at"),
        "末行写清哪两个时刻相减：{text}"
    );
    assert!(
        !text.contains(wording::UNAVAILABLE),
        "这一面的数一个不缺：{text}"
    );
}

#[test]
fn an_old_stream_reports_the_three_numbers_as_unavailable() {
    // 老流上没有 `first_token_ms`：首 token / 生成 / 吞吐三项**不存在**，写 `不可用` 并把
    // 原因写进同一行的计时来源 —— 不填 0、不写占位符（票 13 第 6 条）。
    let mut state = state_with_roster(&["kimi"]);
    let start = a_call_start();
    use heng::events::{EventPayload, Role};
    state.apply(at_event(
        1,
        start,
        EventPayload::TurnStarted {
            agent: kimi(),
            iteration: 1,
        },
    ));
    state.apply(at_event(
        2,
        start + chrono::Duration::milliseconds(8_000),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: "答案是 42。".to_owned(),
            reasoning: None,
            first_token_ms: None,
        },
    ));
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "答案是 42。");
    tab_to_timing(&mut state);

    let rows = screen(120, 40, &mut state);
    let text = rows.join("\n");
    // 三项**各自那一行**都以「不可用」收尾 —— 不是数整屏出现几次：末尾那条计时来源里也写着
    // 这两个字（「首 token / 生成 / 吞吐 不可用」），数次数只会把那一句也算进去。
    for label in [
        wording::TIMING_FIRST_TOKEN,
        wording::TIMING_GENERATED,
        wording::TIMING_THROUGHPUT,
    ] {
        let row = rows
            .iter()
            .find(|row| row.contains(label) && !row.contains(wording::TIMING_SOURCE))
            .unwrap_or_else(|| panic!("计时面上有「{label}」那一行：{text}"));
        assert!(
            row.trim_end()
                .trim_end_matches('┆')
                .trim_end()
                .ends_with(wording::UNAVAILABLE),
            "「{label}」那一行的值写「{}」—— 不填 0、不写占位符：{text}",
            wording::UNAVAILABLE
        );
    }
    assert!(text.contains("8.0 s"), "拿得到的那个照旧写着：{text}");
    assert!(!text.contains("0.0 s"), "不拿 0 顶上去：{text}");
    assert!(
        text.contains("first_token_ms"),
        "原因与它写在同一行：{text}"
    );
}

#[test]
fn a_tool_timing_face_only_shows_the_section_it_can_prove() {
    // 工具行属于哪次模型调用今天没有记账，于是这一面只出现「这次工具调用」那一节 ——
    // 不出现一个空的「这次模型调用」（票 13 第 6 条）。两个口径并排，而话写清它们不相加。
    let mut state = state_with_roster(&["kimi"]);
    let start = a_call_start();
    state.apply(tool_started(
        1,
        "call-timing",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    // 一次**问过**的权限：问与答之间隔着 0.2 s。那一段是「其中等审批」这个读数 —— 它由两个
    // 时刻相减得来，而与总跨度并排、不相加（票 13 第 6 条）。
    use heng::events::{Decision, DecisionSource, EventPayload, ToolCallId};
    state.apply(at_event(
        2,
        start + chrono::Duration::milliseconds(100),
        EventPayload::PermissionAsked {
            request_id: "r-1".to_owned(),
            tool_call_id: ToolCallId::new("call-timing"),
            request: serde_json::json!({"tool_name": "bash", "args": {"command": "ls"}}),
        },
    ));
    state.apply(at_event(
        3,
        start + chrono::Duration::milliseconds(300),
        EventPayload::PermissionDecided {
            request_id: "r-1".to_owned(),
            decision: Decision::Allow,
            source: DecisionSource::User,
            reason: None,
        },
    ));
    state.apply(tool_completed_after(
        4,
        "call-timing",
        true,
        Some("out"),
        None,
        400,
    ));
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    next_face(&mut state); // 参数 → 输出
    next_face(&mut state); // 输出 → 计时

    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("开始时刻"), "{text}");
    assert!(text.contains("0.4 s"), "总时长是那笔墙钟：{text}");
    assert!(text.contains("其中等审批"), "{text}");
    assert!(
        text.contains(&format!("0.2 s · {}", wording::timing_two_measures())),
        "等审批那一段就是问与答之间那 0.2 s，而它和总跨度并排、不相加：{text}"
    );
    assert!(
        !text.contains(wording::timing_call_section()),
        "不出现空的「这次模型调用」：{text}"
    );
    assert!(
        text.contains("ToolCallStarted.at → ToolCallCompleted.at"),
        "末行写清哪两个时刻相减：{text}"
    );
}

#[test]
fn each_face_keeps_its_own_scroll_position() {
    // 切面不是滚动：每一面各记自己的 `top`，切走再切回来还在原处（票 13 第 3 条）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-scroll",
        "bash",
        serde_json::json!({"command": "seq 1 200"}),
    ));
    let body: Vec<String> = (1..=200).map(|n| format!("第 {n} 行")).collect();
    state.apply(tool_completed(
        2,
        "call-scroll",
        true,
        Some(&body.join("\n")),
        None,
    ));
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    next_face(&mut state); // 参数 → 输出

    // 主体高度是**上一帧量出来的那个数**（`DetailView.height` 由画那一帧写回），所以先渲染
    // 一帧 —— 真实的按键本来就发生在两帧之间。120×40 下覆盖层 36 行、带标签条时主体 29 行，
    // 于是 `PageDown` 一次滚 28 行（一页减一行重叠），5 次落在 140；200 行还没到底
    // （底是 200 − 29 = 171）。
    let _ = screen(120, 40, &mut state);
    for _ in 0..5 {
        state.key(Key::PageDown);
    }
    let scrolled = screen(120, 40, &mut state).join("\n");
    assert!(
        scrolled.contains("第 141 行"),
        "输出那一面滚下去了，顶上就是第 141 行：{scrolled}"
    );
    assert!(
        !scrolled.contains("第 1 行"),
        "而开头的那些行早就不在窗口里了：{scrolled}"
    );

    next_face(&mut state); // 输出 → 计时
    let timing = screen(120, 40, &mut state).join("\n");
    assert!(timing.contains("开始时刻"), "而计时面从头开始：{timing}");

    state.key(Key::BackTab); // 计时 → 输出
    let back = screen(120, 40, &mut state).join("\n");
    assert_eq!(back, scrolled, "切回来还在原处");
}

#[test]
fn clicking_a_label_switches_the_face() {
    // 点标签也切面：标签条上每个标签画出来的时候记了一个命中矩形（票 13 第 3 条）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-label",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-label", true, Some("out"), None));
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");

    let frame = buffer(120, 40, &mut state);
    let (column, row) = cell_of(&frame, 120, 40, "概述").expect("标签条上那个标签");
    click(&mut state, column, row);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("一次 bash 调用"),
        "点「概述」就落在概述面上：{text}"
    );
}

#[test]
fn the_label_bar_borrows_a_body_row_without_touching_the_box() {
    // **覆盖层的尺寸一行都不许动**：100×30 仍是今天实测量的那个框，96×26。标签条借走的是
    // **主体**的一行，框还是那个框、标题与页脚还在原处；页脚仍是那一对数加 `esc 关闭`，
    // **不加面号**（票 13 第 3、6、8 条）。
    //
    // 票面与 `spec.md` §6 里写的「92×26」是**旧数字**：那张图当初按别的宽度画的，而成文时
    // 的尺寸算术（`MODAL_MARGIN = 4`）给出的是 `100 − 4 = 96`。那两处算术这一次一行没动
    // （`src/render/layout.rs` 在这次改动里零 diff），所以这里钉的是今天实测量的形状 ——
    // 这个数对不上记在票的落地记录里，`spec.md` 的正文不动。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-box",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-box", true, Some("out"), None));
    open_trace_tab(&mut state, 100, 30);
    click_row(&mut state, 100, 30, "调用 bash");

    let frame = buffer(100, 30, &mut state);
    let (left, top, width) = overlay_frame(&frame, 100, 30).expect("覆盖层的上边框");
    assert_eq!(left, 2, "100 列下居中：100 − 96 的两半各留 2 列");
    assert_eq!(top, 2, "30 行下居中：30 − 26 的两半各留 2 行");
    assert_eq!(width, 96, "覆盖层的宽度与今天一致：100 − MODAL_MARGIN");
    assert_eq!(
        frame[(left + 2, top + 25)].symbol(),
        "┄",
        "下边框在框顶上第 25 行处：框高与今天一致（26）"
    );

    let rows = screen(100, 30, &mut state);
    // 框内自上而下：一行内边距、一行标题（标题在框内第一行，票 03 §Answer）、然后是标签条。
    let title = &rows[usize::from(top) + 2];
    assert!(title.contains("调用 bash"), "标题还在它那一行上：{rows:?}");
    let bar = &rows[usize::from(top) + 3];
    assert!(bar.contains(TOOL_FACES), "标签条紧挨着标题：{bar}");
    let footer = rows
        .iter()
        .find(|row| row.contains('↕'))
        .expect("页脚画出来了");
    assert!(
        footer.contains("· esc 关闭"),
        "页脚仍是那一对数加出口：{footer}"
    );
    assert!(!footer.contains("面"), "页脚不报面号：{footer}");
}

#[test]
fn a_detail_with_one_face_draws_no_bar_at_all() {
    // 面数 ≤ 1 的详情**不画**标签条，且与今天逐字相同：文件 / 改动 / 待办的主体本身就是一份
    // 完整的可滚动对象，不必多点一次（票 14 第 7 条）。这里拿待办那一份验。
    let mut state = state_with_roster(&["kimi"]);
    apply_todo(
        &mut state,
        "call-1",
        kimi(),
        todo_args(&[("一件事", "pending")]),
    );
    let tab = tab_bar_row(&mut state, 120, 24);
    click_in_row(&mut state, 120, 24, tab, wording::TAB_TODO);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = cell_of(&frame, 120, 24, "一件事").expect("那一项在页上");
    click(&mut state, column, row);

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains(wording::detail_summary_face())
            && !text.contains(wording::detail_source_face()),
        "单面详情连标签条都没有：{text}"
    );
    assert!(
        text.contains(&wording::detail_todo_title(None)),
        "正文还是今天那一份（标题 + 清单）：{text}"
    );
    assert!(text.contains("一件事"), "{text}");
}

/// 一次迭代：一个回合里的第几次模型调用 —— 组头编号与「上面那条迭代的回复」那一环都看它。
fn iteration(state: &mut TuiState, seq: u64, at: chrono::DateTime<chrono::Utc>, ordinal: u32) {
    use heng::events::EventPayload;
    state.apply(at_event(
        seq,
        at,
        EventPayload::TurnStarted {
            agent: kimi(),
            iteration: ordinal,
        },
    ));
}

/// 一次模型调用的完成：助手说的那一段。来源面那条链上「一次迭代的回复」就是它。
fn assistant_says(state: &mut TuiState, seq: u64, at: chrono::DateTime<chrono::Utc>, text: &str) {
    use heng::events::{EventPayload, Role};
    state.apply(at_event(
        seq,
        at,
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: text.to_owned(),
            reasoning: None,
            first_token_ms: None,
        },
    ));
}

#[test]
fn a_tool_detail_reads_the_read_only_chain_that_carried_this_row() {
    // 来源面：一条**只读的祖先链** —— 本行 → 上面那条迭代的回复 → 那一回合 → 上面那条用户
    // 消息，每一环带着行首文字与它在账本上的位置（票 14 第 1 条）。
    //
    // 这一次调用落在**第三次迭代**里，所以链上头还有更早的迭代 —— 链固定截到三环，更深的
    // 那些不再展开：一个几百次迭代的回合不会画出一长串（票 14 第 1 条）。
    let mut state = state_with_roster(&["kimi"]);
    let start = a_call_start();
    state.apply(user_message(1, "把 grep 的输出截断修一下"));
    iteration(&mut state, 2, start, 1);
    assistant_says(
        &mut state,
        3,
        start + chrono::Duration::milliseconds(500),
        "先看一眼那次调用。",
    );
    state.apply(tool_started(
        4,
        "call-first",
        "grep",
        serde_json::json!({"pattern": "截断"}),
    ));
    state.apply(tool_completed(5, "call-first", true, Some("out"), None));
    iteration(&mut state, 6, start + chrono::Duration::seconds(1), 2);
    assistant_says(
        &mut state,
        7,
        start + chrono::Duration::seconds(2),
        "头尾都留住了，省掉多少也写清楚了。",
    );
    state.apply(tool_started(
        8,
        "call-second",
        "bash",
        serde_json::json!({"command": "cargo test"}),
    ));
    state.apply(tool_completed(9, "call-second", true, Some("out"), None));
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    next_face(&mut state); // 参数 → 输出
    next_face(&mut state); // 输出 → 计时
    next_face(&mut state); // 计时 → 来源

    let rows = screen(120, 40, &mut state);
    let text = rows.join("\n");
    for (label, hint) in [
        (wording::chain_here(), "本行"),
        (wording::chain_reply(), "上面那条迭代的回复"),
        (wording::chain_unit(), "那一回合"),
        (wording::chain_user(), "上面那条用户消息"),
    ] {
        assert!(text.contains(label), "链上有{hint}：{text}");
    }
    assert!(
        text.contains("头尾都留住了，省掉多少也写清楚了。"),
        "回复那一环的行首文字是它上面最近的那一条：{text}"
    );
    assert!(
        text.contains("把 grep 的输出截断修一下"),
        "用户消息那一环的行首文字：{text}"
    );
    assert_eq!(
        text.matches(wording::chain_arrow()).count(),
        3,
        "四环连成一条：本行之外那三环各带一个「往上」的记号，一个不多一个不少：{text}"
    );
    assert_eq!(
        text.matches(wording::chain_reply()).count(),
        1,
        "更早的迭代不再展开：链只有那一条回复：{text}"
    );
    assert!(
        text.contains(wording::chain_deeper()),
        "而它写清了截到三环：{text}"
    );
    // 每一环都带着它在账本上的位置（`第 N 行`）。
    for row in rows
        .iter()
        .filter(|row| row.contains(wording::chain_arrow()) || row.contains(wording::chain_here()))
    {
        assert!(
            row.contains("第 ") && row.contains(" 行"),
            "这一环带着位置：{row:?}"
        );
    }
}

#[test]
fn a_chain_that_breaks_ends_there_instead_of_a_placeholder() {
    // 开场段落里的一次调用：它上面既没有迭代的回复、也没有回合与用户消息 —— 链在某一环自然
    // 断掉，面上**不写占位符**（票 14 第 2 条）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(tool_started(
        1,
        "call-preamble",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    state.apply(tool_completed(2, "call-preamble", true, Some("out"), None));
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "调用 bash");
    next_face(&mut state); // 参数 → 输出
    next_face(&mut state); // 输出 → 计时
    next_face(&mut state); // 计时 → 来源

    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains(wording::chain_here()), "链上仍有本行：{text}");
    assert!(
        !text.contains(wording::chain_arrow()),
        "而它上面一环都没有 —— 链就是渐短的：{text}"
    );
    for label in [
        wording::chain_reply(),
        wording::chain_unit(),
        wording::chain_user(),
    ] {
        assert!(
            !text.contains(label),
            "断在哪儿就画到哪儿，不补「{label}」：{text}"
        );
    }
    assert!(
        !text.contains(wording::UNAVAILABLE),
        "也不写「不可用」这种占位：{text}"
    );
    assert!(
        !text.contains(wording::chain_deeper()),
        "链没被截断：{text}"
    );
}

#[test]
fn an_injection_detail_grows_a_label_bar_and_still_opens_on_the_injection() {
    // 注入从今天那一面长出第二面来：标签条上写着「注入 ┆ 概述」，而**默认面仍是注入** ——
    // 打开就看到今天那份正文（来源标题 + 内容），面名只是第一次写出来（票 14 第 4 条）。
    let mut state = state_with_roster(&["kimi"]);
    state.apply(heng::render::RenderEvent::Logged(heng::events::Event::new(
        1,
        heng::events::SpeakerId::User,
        heng::events::EventPayload::ContextInjected {
            source: heng::events::ContextSource::AgentsMd,
            content: "注入的正文一段".to_owned(),
        },
    )));
    open_trace_tab(&mut state, 120, 40);
    click_row(&mut state, 120, 40, "上下文注入");

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains(&format!(
            "{}┆{}",
            wording::detail_injection_face(),
            wording::detail_summary_face()
        )),
        "顶上按顺序列着这两面：{text}"
    );
    assert!(
        text.contains(&wording::context_source(
            &heng::events::ContextSource::AgentsMd
        )),
        "默认落在注入那一面：{text}"
    );
    assert!(text.contains("注入的正文一段"), "{text}");
}
