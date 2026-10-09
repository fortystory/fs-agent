//! 左栏「改动」页：一列改动的文件、键盘归属与焦点行
//! （`.scratch/diff-page/spec.md`，票 09 与票 10）。
//!
//! 接缝与别的帧测试同一个：[`draw_frame`] 一个状态进去、一块定尺的缓冲出来。数据用既有的
//! **状态机口子**送进去 —— 拉取数位（`take_changes`）、把结果回填（`changes_loaded`）——
//! 所以前面那些断言不起任何进程。末尾几条是**真 git 的临时仓库**，它们钉的是取数那一层：
//! 三态齐备、空仓库、非仓库，以及「读一次不该写回 index」。

use std::path::Path;

use heng::config::{DiffViewerSettings, FileViewerSettings};
use heng::render::width::text_columns;
use heng::render::{
    ConsoleRequest, FrontEndEvent, Key, RenderEvent, SessionFacts, TuiState, changes, draw_frame,
    palette, wording,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Modifier;

fn facts() -> SessionFacts {
    SessionFacts {
        switchable: true,
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: "~/code/fortystory/heng".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        mode: heng::permissions::Mode::Ask,
        budget_limit: Some(100_000),
        number_style: heng::render::wording::NumberStyle::Cn,
        file_viewer: FileViewerSettings::default(),
        diff_viewer: DiffViewerSettings::default(),
        // 这一场会话的工具声明：Schema 面按名字查它（票 24 收口补的那一面）。这些测试
        // 不打开工具详情，于是给一份空的。
        tool_schemas: Default::default(),
        speaker_order: Vec::new(),
    }
}

fn state() -> TuiState {
    TuiState::new(facts(), std::path::PathBuf::from("/x/heng"), None)
}

/// [`state`] 的另一个名字：有些测试体里 `state` 已经被那个局部变量占住了。
fn fresh() -> TuiState {
    state()
}

/// 一个空闲的、在等一行的状态 —— 「打字落进草稿」那几条要的就是它。
fn idle() -> TuiState {
    let mut state = state();
    let (reply, line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    drop(line);
    state
}

/// 画出来的这一帧本身，用来断言某个具体的格子。
fn buffer(width: u16, height: u16, state: &mut TuiState) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("TestBackend");
    terminal
        .draw(|frame| draw_frame(frame, state))
        .expect("画一帧");
    terminal.backend().buffer().clone()
}

/// 某一行的文本，取它在帧里两列之间的那一段。
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

fn row_text(buffer: &Buffer, y: u16, width: u16) -> String {
    cells(buffer, y, 0, width)
}

fn screen(width: u16, height: u16, state: &mut TuiState) -> Vec<String> {
    let frame = buffer(width, height, state);
    (0..height).map(|y| row_text(&frame, y, width)).collect()
}

/// 左栏那几列，逐行（宽档 40 列、窄档 28 列）。改动页的断言都读它 —— 屏幕别处也可能有同样
/// 的词。
fn sidebar(state: &mut TuiState, width: u16, height: u16, columns: u16) -> Vec<String> {
    let frame = buffer(width, height, state);
    (0..height)
        .map(|y| cells(&frame, y, 0, columns).trim_end().to_owned())
        .collect()
}

/// `改动` 页签所在的那一格。
fn changes_tab_cell(frame: &Buffer, width: u16, height: u16) -> (u16, u16) {
    for y in 0..height {
        let text = row_text(frame, y, width);
        if let Some(at) = text.find(wording::TAB_CHANGES) {
            return (text_columns(&text[..at]) as u16, y);
        }
    }
    panic!("`{}` 页签在屏幕上", wording::TAB_CHANGES);
}

fn press(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

fn release(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

fn click(state: &mut TuiState, column: u16, row: u16) {
    state.mouse(press(column, row));
    state.mouse(release(column, row));
}

/// 把一帧里含 `needle` 的那一行的行号找出来（左栏那几列里找）。
fn sidebar_row_of(rows: &[String], needle: &str) -> usize {
    rows.iter()
        .position(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} 在左栏里：{rows:#?}"))
}

/// 焦点行画出来是什么样：常驻选中那一档（`ACCENT` + `BOLD`）。
fn focused_sidebar_row(frame: &Buffer, height: u16) -> Option<u16> {
    (0..height).find(|y| {
        let cell = &frame[(0, *y)];
        cell.fg == palette::ACCENT && cell.modifier.contains(Modifier::BOLD)
    })
}

/// 送一份读数进去：先拉一次取数位，再回填结果 —— 这就是渲染循环走的那条路。
fn loaded(state: &mut TuiState, files: &[(&str, changes::Kind)]) {
    assert!(state.take_changes(), "进 TUI 就该请一次取数");
    state.changes_loaded(changes::Outcome::Changed(
        files
            .iter()
            .map(|(path, kind)| changes::ChangedFile {
                path: (*path).to_owned(),
                kind: *kind,
            })
            .collect(),
    ));
}

/// 切到改动页：点它的页签。
fn open_changes(state: &mut TuiState, width: u16, height: u16) {
    let frame = buffer(width, height, state);
    let (column, row) = changes_tab_cell(&frame, width, height);
    click(state, column, row);
}

// --- 票 09：一列改动的文件 ---------------------------------------------------

#[test]
fn the_changes_tab_is_the_fourth_one_and_is_always_there() {
    // 常驻（§1）：工作区干净、这里不是仓库、甚至找不到 git，页签都在同一个位置上 ——
    // 页签条不该在会话中途跳。两档宽度都要放得下（窄档 28 列下四签占 21 列）。
    for (width, height, columns) in [(120u16, 24u16, 40u16), (80, 24, 28)] {
        let mut state = state();
        state.take_changes();
        for outcome in [
            changes::Outcome::Changed(Vec::new()),
            changes::Outcome::Changed(vec![changes::ChangedFile {
                path: "a.rs".to_owned(),
                kind: changes::Kind::Modified,
            }]),
            changes::Outcome::NotARepo,
            changes::Outcome::NoGit,
        ] {
            state.changes_loaded(outcome);
            let rows = sidebar(&mut state, width, height, columns);
            assert!(
                rows.iter().any(|row| row.contains(wording::TAB_CHANGES)),
                "{width} 列下第四签在页签条上：{rows:#?}"
            );
        }
    }
}

#[test]
fn before_the_first_readout_the_page_says_it_is_reading() {
    let mut state = state();
    assert!(state.take_changes(), "进 TUI 一次");
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    assert!(
        rows.iter()
            .any(|row| row.contains(wording::changes_loading())),
        "还没取到数：{rows:#?}"
    );
}

#[test]
fn the_list_groups_by_status_then_sorts_by_path_inside_each_group() {
    let mut state = state();
    loaded(
        &mut state,
        &[
            ("b.rs", changes::Kind::Modified),
            ("a.rs", changes::Kind::Modified),
            ("new.rs", changes::Kind::Added),
            ("gone.rs", changes::Kind::Deleted),
            ("brand-new.txt", changes::Kind::Untracked),
        ],
    );
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);

    // 组的先后：已修改 → 新增 → 已删除 → 未跟踪；组内按路径字典序（a.rs 在 b.rs 之前）。
    let order: Vec<usize> = [
        wording::CHANGE_GROUP_MODIFIED,
        "a.rs",
        "b.rs",
        wording::CHANGE_GROUP_ADDED,
        "new.rs",
        wording::CHANGE_GROUP_DELETED,
        "gone.rs",
        wording::CHANGE_GROUP_UNTRACKED,
        "brand-new.txt",
    ]
    .iter()
    .map(|needle| sidebar_row_of(&rows, needle))
    .collect();
    assert!(
        order.windows(2).all(|pair| pair[0] < pair[1]),
        "分组与组内顺序都不对：{rows:#?}"
    );
}

#[test]
fn each_group_has_exactly_one_header_and_an_empty_group_is_not_drawn() {
    let mut state = state();
    loaded(
        &mut state,
        &[
            ("a.rs", changes::Kind::Modified),
            ("b.rs", changes::Kind::Modified),
            ("new.txt", changes::Kind::Untracked),
        ],
    );
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let text = rows.join("\n");
    assert_eq!(text.matches(wording::CHANGE_GROUP_MODIFIED).count(), 1);
    assert_eq!(text.matches(wording::CHANGE_GROUP_UNTRACKED).count(), 1);
    assert!(
        !text.contains(wording::CHANGE_GROUP_ADDED)
            && !text.contains(wording::CHANGE_GROUP_DELETED),
        "空组一个标题都不画：{rows:#?}"
    );
}

#[test]
fn a_row_is_a_two_cell_glyph_then_the_whole_path() {
    let mut state = state();
    loaded(
        &mut state,
        &[
            ("src/render/tui.rs", changes::Kind::Modified),
            ("brand-new.txt", changes::Kind::Untracked),
        ],
    );
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let modified = &rows[sidebar_row_of(&rows, "src/render/tui.rs")];
    let untracked = &rows[sidebar_row_of(&rows, "brand-new.txt")];
    // 字形占满两格，于是两个名字严格同列：`M ` 与 `??` 各占两格，后面跟一个空格。
    assert_eq!(modified, "M  src/render/tui.rs");
    assert_eq!(untracked, "?? brand-new.txt");
}

#[test]
fn a_path_that_does_not_fit_is_cut_with_an_ellipsis() {
    let mut state = state();
    let long = "src/render/very-long-module-name/deep/tui.rs";
    loaded(&mut state, &[(long, changes::Kind::Modified)]);
    // 窄档：左栏 28 列，字形占两格加一个空格，路径只剩 25 列。
    open_changes(&mut state, 80, 24);
    let rows = sidebar(&mut state, 80, 24, 28);
    let row = &rows[sidebar_row_of(&rows, "M ").min(rows.len() - 1)];
    assert!(row.ends_with('…'), "超宽的路径用 `…` 收尾：{row:?}");
    assert!(
        text_columns(row) <= 28,
        "它没有溢出左栏：{row:?}（{} 列）",
        text_columns(row)
    );
}

#[test]
fn too_many_changes_fill_the_page_and_say_how_many_are_left() {
    let mut state = state();
    let many: Vec<(String, changes::Kind)> = (0..30)
        .map(|index| (format!("file{index:02}.rs"), changes::Kind::Modified))
        .collect();
    let many: Vec<(&str, changes::Kind)> = many
        .iter()
        .map(|(path, kind)| (path.as_str(), *kind))
        .collect();
    loaded(&mut state, &many);
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);

    // 宽档 120×24 下页区是 40 列 × 15 行：标题一行 + 13 个文件 + 末行那一句。
    let last = rows
        .iter()
        .rev()
        .find(|row| !row.is_empty())
        .expect("页区里有东西");
    assert!(
        last.contains("还有 17 处改动"),
        "画满 15 行之后说清省掉了多少：{last:?}\n{rows:#?}"
    );
    assert!(
        !rows.iter().any(|row| row.contains("file29.rs")),
        "装不下的那些一行都不画：{rows:#?}"
    );
    assert!(
        rows.iter().any(|row| row.contains("file12.rs")),
        "装得下的都画了：{rows:#?}"
    );
}

#[test]
fn clean_not_a_repo_and_no_git_each_say_their_own_line() {
    for (outcome, expected) in [
        (
            changes::Outcome::Changed(Vec::new()),
            wording::changes_empty(),
        ),
        (changes::Outcome::NotARepo, wording::changes_not_a_repo()),
        (changes::Outcome::NoGit, wording::changes_no_git()),
    ] {
        let mut state = state();
        state.take_changes();
        state.changes_loaded(outcome);
        open_changes(&mut state, 120, 24);
        let rows = sidebar(&mut state, 120, 24, 40);
        assert!(
            rows.iter().any(|row| row.contains(expected)),
            "页里该写 {expected:?}：{rows:#?}"
        );
    }
}

#[test]
fn a_failed_readout_keeps_the_last_numbers_and_leaves_a_receipt() {
    // 失败**保留上一次读数**（§4）：正看着这一列的人不该因为一次取数失败看到它被换成一句
    // 错误 —— 提示行说一句就够了。
    let mut state = state();
    loaded(&mut state, &[("a.rs", changes::Kind::Modified)]);
    open_changes(&mut state, 120, 24);
    let before = sidebar(&mut state, 120, 24, 40);

    // 再请一次取数（`WorkspaceChanged` 就是它的一个触发点），然后让它失败。
    state.live_event(RenderEvent::WorkspaceChanged);
    assert!(state.take_changes());
    state.changes_loaded(changes::Outcome::Failed);
    let after = sidebar(&mut state, 120, 24, 40);
    assert_eq!(before, after, "页里一个字节都没动");
    let screen = screen(120, 24, &mut state).join("\n");
    assert!(
        screen.contains(wording::changes_read_failed()),
        "提示行给了一句失败回执：{screen}"
    );
}

#[test]
fn a_workspace_change_asks_for_one_read_at_a_time_and_draws_nothing() {
    // 与文件索引那道守卫同构：一个位 + 在飞时不并发，但**不丢**（位留着，下一轮补发）。
    // 改动页用它自己的位 —— 不共用文件索引那一位。
    let mut state = state();
    assert!(state.take_changes(), "进 TUI 先取一次");
    state.changes_loaded(changes::Outcome::Changed(Vec::new()));
    let before = sidebar(&mut state, 120, 24, 40);

    for _ in 0..3 {
        state.live_event(RenderEvent::WorkspaceChanged);
    }
    let after = sidebar(&mut state, 120, 24, 40);
    assert_eq!(before, after, "这条信号不画任何东西");
    assert!(state.take_changes(), "它请了一次取数");
    assert!(!state.take_changes(), "一次取数还在飞，不并发");

    // 飞着的时候工作区又变了：位留着，结果落地之后的下一轮补发。
    state.live_event(RenderEvent::WorkspaceChanged);
    assert!(!state.take_changes(), "飞着的那次不被抢");
    state.changes_loaded(changes::Outcome::Changed(Vec::new()));
    assert!(state.take_changes(), "补发的那一次仍然发得出去");
}

#[test]
fn a_submit_and_a_workspace_change_both_ask_for_a_read() {
    // 提交之后（§4）与 `WorkspaceChanged`（任何非只读工具调用收尾，含 `/undo`）各请一次。
    let mut state = idle();
    assert!(state.take_changes());
    state.changes_loaded(changes::Outcome::Changed(Vec::new()));
    assert!(!state.take_changes(), "没有新的触发就不取");

    let mut line = {
        let (reply, line) = tokio::sync::oneshot::channel();
        state.request(ConsoleRequest::Prompt { reply });
        line
    };
    state.key(Key::Char('h'));
    state.key(Key::Enter);
    assert!(line.try_recv().is_ok(), "这一行送出去了");
    assert!(state.take_changes(), "提交也算一次触发");
}

#[test]
fn the_page_keeps_reading_even_when_it_is_not_in_front() {
    // 与文件索引的重扫同构：左栏停在别的页、甚至收起来时都照旧取数 —— 切回来即时看到。
    let mut state = state();
    assert!(state.take_changes());
    state.changes_loaded(changes::Outcome::Changed(Vec::new()));
    state.live_event(RenderEvent::WorkspaceChanged);
    assert!(state.take_changes(), "页不在前台也照旧取数");
}

#[test]
fn r_reads_again_only_when_the_keyboard_is_on_this_page() {
    let mut state = idle();
    loaded(&mut state, &[("a.rs", changes::Kind::Modified)]);
    // 点页签条也是一次「点左栏」：键盘交给这一页（§7）。点页区里的一行会同时打开弹窗，
    // 那是另一条路 —— 这里要的是「键盘在改动页上」。
    open_changes(&mut state, 120, 24);

    state.key(Key::Char('r'));
    assert!(state.take_changes(), "`r` 请了一次取数");
    let screen = screen(120, 24, &mut state).join("\n");
    assert!(
        screen.contains(wording::changes_refreshing()),
        "它给了一句回执：{screen}"
    );

    // 键盘不在这一页时 `r` 只是一个普通字符：落进草稿。
    let mut state = idle();
    loaded(&mut state, &[("a.rs", changes::Kind::Modified)]);
    open_changes(&mut state, 120, 24);
    state.key(Key::Char('r'));
    let frame = buffer(120, 24, &mut state);
    let text: String = (0..24).map(|y| row_text(&frame, y, 120)).collect();
    assert!(
        text.contains('r'),
        "键盘不在改动页上时，`r` 照旧落进草稿：{text}"
    );
}

// --- 票 10：键盘归属与焦点行 ---------------------------------------------------

#[test]
fn clicking_the_page_hands_it_the_keyboard_and_the_arrows_walk_the_file_rows() {
    let mut state = state();
    loaded(
        &mut state,
        &[
            ("a.rs", changes::Kind::Modified),
            ("b.rs", changes::Kind::Modified),
            ("new.txt", changes::Kind::Untracked),
        ],
    );
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let second = sidebar_row_of(&rows, "b.rs") as u16;

    // 点左栏的一行：焦点落到那一行上，而且它看得出来（常驻选中那一档）。这一下同时打开那份
    // diff（§6），所以先 `Esc` 把它关掉 —— 关掉之后这一页停在原处。
    click(&mut state, 3, second);
    let frame = buffer(120, 24, &mut state);
    assert_eq!(
        focused_sidebar_row(&frame, 24),
        Some(second),
        "点的就是焦点行"
    );
    state.key(Key::Esc);
    let frame = buffer(120, 24, &mut state);
    assert_eq!(
        focused_sidebar_row(&frame, 24),
        Some(second),
        "关掉弹窗之后这一页停在原处"
    );

    // `↓` 走到下一个**文件行**：跨过下一组的标题，落在 new.txt 上。
    state.key(Key::Down);
    let rows = sidebar(&mut state, 120, 24, 40);
    let untracked = sidebar_row_of(&rows, "new.txt") as u16;
    let frame = buffer(120, 24, &mut state);
    assert_eq!(
        focused_sidebar_row(&frame, 24),
        Some(untracked),
        "`↓` 跳过分组标题"
    );

    // 两端停住：再按两下 `↓` 还在最后一行上。
    state.key(Key::Down);
    state.key(Key::Down);
    let frame = buffer(120, 24, &mut state);
    assert_eq!(focused_sidebar_row(&frame, 24), Some(untracked));

    state.key(Key::Up);
    let frame = buffer(120, 24, &mut state);
    let rows = sidebar(&mut state, 120, 24, 40);
    let second = sidebar_row_of(&rows, "b.rs") as u16;
    assert_eq!(focused_sidebar_row(&frame, 24), Some(second));
}

#[test]
fn escape_gives_the_keyboard_back_without_cancelling_the_running_turn() {
    let mut state = state();
    loaded(&mut state, &[("a.rs", changes::Kind::Modified)]);
    open_changes(&mut state, 120, 24);
    state.request(ConsoleRequest::RunState { running: true });

    // 一次手势一层：这一下只把键盘还回去。
    state.key(Key::Esc);
    assert!(
        state.take_events().is_empty(),
        "第一下只还键盘，不取消正在跑的回合"
    );
    state.key(Key::Char('x'));
    let frame = buffer(120, 24, &mut state);
    let text: String = (0..24).map(|y| row_text(&frame, y, 120)).collect();
    assert!(text.contains('x'), "键盘回去了，打字落进草稿：{text}");

    // 键盘不在这一页之后，`Esc` 照旧是取消手势。
    state.key(Key::Esc);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
}

#[test]
fn clicking_the_input_area_takes_the_keyboard_back_from_this_page() {
    let mut state = idle();
    loaded(&mut state, &[("a.rs", changes::Kind::Modified)]);
    open_changes(&mut state, 120, 24);

    // 点在输入区上：键盘还给输入区（与文件页同一条归还路径）。
    let panes = heng::render::layout::plan(ratatui::layout::Rect::new(0, 0, 120, 24), 1, true);
    click(&mut state, panes.input.x + 2, panes.input.y);
    state.key(Key::Char('z'));
    let frame = buffer(120, 24, &mut state);
    let text: String = (0..24).map(|y| row_text(&frame, y, 120)).collect();
    assert!(text.contains('z'), "点回输入区之后打字落在草稿上：{text}");
}

#[test]
fn the_arrows_still_walk_the_files_page_and_nothing_else_moved() {
    // 改动页接上之后，文件页那套归属一个字不变（§7）。
    let mut state = idle();
    state.files_loaded(vec!["src/".into(), "src/a.rs".into()]);
    let frame = buffer(120, 24, &mut state);
    let (column, row) = {
        // 页签条那一行：在屏幕上找 `文件` 那个标签（与 `changes_tab_cell` 同一个找法）。
        let mut found = None;
        for y in 0..24 {
            let text = row_text(&frame, y, 120);
            if let Some(at) = text.find(wording::TAB_FILES) {
                found = Some((text_columns(&text[..at]) as u16, y));
                break;
            }
        }
        found.expect("文件页签在屏幕上")
    };
    click(&mut state, column, row);
    // 画一帧：文件页的可见行是**画的时候**排出来的，键盘拿它换下标。
    let _ = buffer(120, 24, &mut state);
    state.key(Key::Down);
    state.key(Key::Down);
    state.key(Key::Enter);
    // `Enter` 在文件页上照旧把路径插进草稿（不是打开 diff）。
    let frame = buffer(120, 24, &mut state);
    let text: String = (0..24).map(|y| row_text(&frame, y, 120)).collect();
    assert!(
        text.contains("@src/") || text.contains("@src/a.rs"),
        "文件页的 `Enter` 照旧插 `@路径`：{text}"
    );
    // `调用量` 页没有能用方向键走的东西：键盘根本不该被扣在那里。
    let mut state = idle();
    state.key(Key::Up);
    state.key(Key::Char('q'));
    let frame = buffer(120, 24, &mut state);
    let text: String = (0..24).map(|y| row_text(&frame, y, 120)).collect();
    assert!(text.contains('q'), "别的页上打字照旧落进草稿：{text}");
}

// --- 票 11：点开一份 diff -----------------------------------------------------

/// 详情覆盖层里那块**文本区**（框内、去掉内边距）：正文与标题都画在它里面。
fn overlay_area(width: u16, height: u16) -> ratatui::layout::Rect {
    let panes =
        heng::render::layout::plan(ratatui::layout::Rect::new(0, 0, width, height), 1, true);
    let area = panes.detail().expect("有地方画覆盖层");
    let inner = heng::render::layout::inner(area);
    let (pad_x, pad_y) = heng::render::layout::detail_padding(inner);
    ratatui::layout::Rect::new(
        inner.x + pad_x,
        inner.y + pad_y,
        inner.width.saturating_sub(pad_x * 2),
        inner.height.saturating_sub(pad_y * 2),
    )
}

/// 详情覆盖层里的那几行文本：第一行是标题，然后是主体。
fn overlay(state: &mut TuiState, width: u16, height: u16) -> Vec<String> {
    let text = overlay_area(width, height);
    let frame = buffer(width, height, state);
    (0..text.height)
        .map(|row| {
            cells(&frame, text.y + row, text.x, text.x + text.width)
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// 弹窗里含 `needle` 的那一格（按显示列算）与它的屏幕行。
fn overlay_cell(frame: &Buffer, width: u16, height: u16, needle: &str) -> (u16, u16) {
    let text = overlay_area(width, height);
    for row in text.y..text.y + text.height {
        let line = cells(frame, row, text.x, text.x + text.width);
        if let Some(at) = line.find(needle) {
            let offset = text_columns(&line[..at]) as u16;
            return (text.x + offset, row);
        }
    }
    panic!("{needle:?} 在弹窗里");
}

/// 打开那一刻置下的那次读，把结果回填进去 —— 渲染循环走的就是这条分工。
fn feed_diff(state: &mut TuiState, body: changes::Body) {
    let (serial, _file, _columns) = state.take_diff_read().expect("打开那一刻就置了请求");
    state.diff_loaded(serial, body, None);
}

/// 一份最小的补丁：一行上下文、一行删除、一行新增。
fn small_patch() -> String {
    "diff --git a/a.rs b/a.rs\n\
     index 1111111..2222222 100644\n\
     --- a/a.rs\n\
     +++ b/a.rs\n\
     @@ -1,3 +1,4 @@ fn main\n\
     \x20context\n\
     -removed\n\
     +added\n"
        .to_owned()
}

#[test]
fn clicking_a_row_opens_the_diff_and_enter_does_the_same() {
    let mut state = state();
    loaded(
        &mut state,
        &[
            ("a.rs", changes::Kind::Modified),
            ("b.rs", changes::Kind::Modified),
        ],
    );
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let row = sidebar_row_of(&rows, "a.rs") as u16;
    click(&mut state, 3, row);

    let text = overlay(&mut state, 120, 24).join("\n");
    assert!(text.contains("esc 关闭"), "点一行就开了覆盖层：{text}");
    assert!(text.contains("a.rs"), "标题是那个文件：{text}");
    // 正文还没到：弹窗先立起来，一句话占位（一次 git diff 是子进程）。
    assert!(
        text.contains(wording::changes_diff_loading()),
        "正文后到，先写一句：{text}"
    );

    // `Enter` 走同一条路（键盘在改动页上，点页签那一下就交给它了）。
    let mut second = fresh();
    loaded(&mut second, &[("a.rs", changes::Kind::Modified)]);
    open_changes(&mut second, 120, 24);
    second.key(Key::Enter);
    let text = overlay(&mut second, 120, 24).join("\n");
    assert!(
        text.contains("esc 关闭") && text.contains("a.rs"),
        "`Enter` 也开：{text}"
    );
}

#[test]
fn the_body_is_the_patch_with_line_numbers_and_without_the_four_head_lines() {
    let mut state = state();
    loaded(&mut state, &[("src/a.rs", changes::Kind::Modified)]);
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let row = sidebar_row_of(&rows, "src/a.rs") as u16;
    click(&mut state, 3, row);
    feed_diff(&mut state, changes::Body::Patch(small_patch()));

    let rows = overlay(&mut state, 120, 24);
    let text = rows.join("\n");
    assert!(
        !text.contains("diff --git") && !text.contains("index 1111") && !text.contains("+++ b/"),
        "头的四行一个都不画：{rows:#?}"
    );
    assert!(
        rows.iter()
            .any(|row| row.contains("@@ -1,3 +1,4 @@ fn main")),
        "`@@` 那行留着（它带函数上下文）：{rows:#?}"
    );
    // 行号：上下文取新文件的号、删除取旧文件的号、新增取新文件的号。
    assert!(
        rows.iter().any(|row| row.contains("1  context")),
        "上下文行：{rows:#?}"
    );
    assert!(
        rows.iter().any(|row| row.contains("2 -removed")),
        "删除行取旧号：{rows:#?}"
    );
    assert!(
        rows.iter().any(|row| row.contains("2 +added")),
        "新增行取新号：{rows:#?}"
    );
}

#[test]
fn a_binary_patch_is_one_line_instead_of_garbage() {
    let mut state = state();
    loaded(&mut state, &[("logo.bin", changes::Kind::Modified)]);
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let row = sidebar_row_of(&rows, "logo.bin") as u16;
    click(&mut state, 3, row);
    feed_diff(&mut state, changes::Body::Binary);

    let text = overlay(&mut state, 120, 24).join("\n");
    assert!(
        text.contains(wording::changes_binary()),
        "二进制只报一句：{text}"
    );
}

#[test]
fn an_untracked_file_opens_under_a_new_file_title_with_its_whole_text() {
    // 未跟踪的文件没有 diff 可比（`git diff HEAD` 对它就是空输出），那一档在**同一条取数路**
    // 里直接读盘 —— 于是配了 `[ui] diff_viewer` 时它的全文也会交给那个命令（票 06 的
    // 「未跟踪也一视同仁」）。标题说清它是什么，正文是全文。
    let root = tempfile::TempDir::new().expect("临时工作区");
    std::fs::write(root.path().join("brand-new.txt"), "first\nsecond\n").expect("写一个文件");
    let mut state = TuiState::new(facts(), root.path().to_path_buf(), None);
    loaded(&mut state, &[("brand-new.txt", changes::Kind::Untracked)]);
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let row = sidebar_row_of(&rows, "brand-new.txt") as u16;
    click(&mut state, 3, row);

    // 弹窗先立起来、正文后到（与已跟踪那一档同一个形状）。
    let (serial, _file, _columns) = state
        .take_diff_read()
        .expect("未跟踪那一档也走同一条取数路");
    state.diff_loaded(
        serial,
        changes::Body::NewFile(heng::render::files::FileBody::Text {
            text: "first\nsecond\n".to_owned(),
            truncated: false,
        }),
        None,
    );

    let rows = overlay(&mut state, 120, 24);
    let text = rows.join("\n");
    assert!(
        text.contains(wording::changes_new_file()),
        "标题写「新文件」：{rows:#?}"
    );
    assert!(
        rows.iter().any(|row| row.contains("1 first"))
            && rows.iter().any(|row| row.contains("2 second")),
        "正文是全文，每行带行号：{rows:#?}"
    );
}

#[test]
fn a_patch_with_nothing_but_head_lines_says_so_instead_of_showing_a_blank() {
    // 只有 `old mode` / `new mode` 这类头行的 diff（没有 hunk）：给一句，而不是一块空白。
    let mut state = state();
    loaded(&mut state, &[("a.rs", changes::Kind::Modified)]);
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let row = sidebar_row_of(&rows, "a.rs") as u16;
    click(&mut state, 3, row);
    feed_diff(
        &mut state,
        changes::Body::Patch(
            "diff --git a/a.rs b/a.rs\nold mode 100644\nnew mode 100755\n".to_owned(),
        ),
    );
    let text = overlay(&mut state, 120, 24).join("\n");
    assert!(
        text.contains(wording::changes_nothing_to_show()),
        "没有正文要说出来：{text}"
    );
}

#[test]
fn the_first_read_failing_does_not_leave_the_page_saying_it_is_reading() {
    // 一次都没取到过数就失败：页里那句得跟着变 —— 一句永远挂着的「正在读取改动…」是骗人。
    let mut state = state();
    assert!(state.take_changes());
    state.changes_loaded(changes::Outcome::Failed);
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    assert!(
        rows.iter()
            .any(|row| row.contains(wording::changes_failed())),
        "页里说读不出来：{rows:#?}"
    );
    assert!(
        !rows
            .iter()
            .any(|row| row.contains(wording::changes_loading())),
        "而不是继续说它在读：{rows:#?}"
    );
}

#[test]
fn too_long_a_patch_is_cut_and_says_how_many_rows_were_left_out() {
    let mut state = state();
    loaded(&mut state, &[("a.rs", changes::Kind::Modified)]);
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let row = sidebar_row_of(&rows, "a.rs") as u16;
    click(&mut state, 3, row);

    let mut patch = String::from("@@ -1,1 +1,1 @@\n");
    let extra = 12;
    for index in 0..heng::render::files::MAX_LINES + extra {
        patch.push_str(&format!("+line {index}\n"));
    }
    feed_diff(&mut state, changes::Body::Patch(patch));

    // 那一句在正文的末尾，所以先翻到底（一次 `PageDown` 是一页）。
    let _ = buffer(120, 24, &mut state);
    for _ in 0..300 {
        state.key(Key::PageDown);
    }
    let text = overlay(&mut state, 120, 24).join("\n");
    assert!(
        text.contains(&wording::changes_truncated(
            heng::render::files::MAX_LINES,
            extra + 1,
        )),
        "截断说清省掉了多少：{text}"
    );
}

#[test]
fn a_long_path_in_the_title_keeps_its_tail_with_an_ellipsis() {
    let mut state = state();
    let path = "src/render/a-really-quite-long-module-name/another/deep/one/more/even/yet/tui.rs";
    loaded(&mut state, &[(path, changes::Kind::Modified)]);
    open_changes(&mut state, 80, 24);
    let rows = sidebar(&mut state, 80, 24, 28);
    let row = sidebar_row_of(&rows, "M ").min(rows.len() - 1) as u16;
    click(&mut state, 3, row);

    let rows = overlay(&mut state, 80, 24);
    let title = rows.first().expect("有标题").clone();
    assert!(title.ends_with('…'), "超宽的标题用 `…` 收尾：{title:?}");
    assert!(title.starts_with("src/render/"), "它保住了开头：{title:?}");
}

#[test]
fn a_stale_answer_does_not_replace_a_dialog_opened_later() {
    // 打开 A、关掉、打开 B，然后 A 的结果才回来：那一份不该盖掉 B 正在显示的东西
    // （序号就是为这件事存在的）。
    let mut state = state();
    loaded(
        &mut state,
        &[
            ("a.rs", changes::Kind::Modified),
            ("b.rs", changes::Kind::Modified),
        ],
    );
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let first = sidebar_row_of(&rows, "a.rs") as u16;
    click(&mut state, 3, first);
    let (stale_serial, _file, _columns) = state.take_diff_read().expect("A 的读置下了");

    state.key(Key::Esc);
    let rows = sidebar(&mut state, 120, 24, 40);
    let second = sidebar_row_of(&rows, "b.rs") as u16;
    click(&mut state, 3, second);

    state.diff_loaded(stale_serial, changes::Body::Patch(small_patch()), None);
    let text = overlay(&mut state, 120, 24).join("\n");
    assert!(
        text.contains(wording::changes_diff_loading()),
        "A 的迟到结果被丢掉了，B 还在等它自己那一份：{text}"
    );
}

#[test]
fn the_patch_is_coloured_in_two_layers() {
    // diff 那一层给**背景**，语法层给**前景**，两层叠在一行上：于是一行既是「新增」又是
    // 一个关键字（`.scratch/diff-page/spec.md` §6）。
    let mut state = state();
    loaded(&mut state, &[("a.rs", changes::Kind::Modified)]);
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let row = sidebar_row_of(&rows, "a.rs") as u16;
    click(&mut state, 3, row);
    feed_diff(
        &mut state,
        changes::Body::Patch(
            "@@ -1,3 +1,3 @@\n fn main() {}\n-let old = 1;\n+let new = 2;\n".to_owned(),
        ),
    );

    let frame = buffer(120, 24, &mut state);
    let (at, row) = overlay_cell(&frame, 120, 24, "@@ -1,3");
    assert_eq!(frame[(at, row)].fg, palette::DIFF_HUNK, "hunk 头是前景");
    assert!(
        frame[(at, row)].modifier.contains(Modifier::BOLD),
        "hunk 头还配粗体"
    );

    let (at, row) = overlay_cell(&frame, 120, 24, "+let new");
    assert_eq!(frame[(at, row)].bg, palette::DIFF_ADDED, "新增行是背景色");

    let (at, row) = overlay_cell(&frame, 120, 24, "-let old");
    assert_eq!(
        frame[(at, row)].bg,
        palette::DIFF_REMOVED,
        "删除行另一种背景"
    );

    // 语法层照旧在：上下文行里的 `fn` 是关键字那一档，而它没有 diff 背景。
    let (at, row) = overlay_cell(&frame, 120, 24, "fn main");
    assert_eq!(
        frame[(at, row)].fg,
        palette::CODE_KEYWORD,
        "`fn` 是关键字的颜色"
    );
    assert_eq!(
        frame[(at, row)].bg,
        ratatui::style::Color::Reset,
        "上下文行不带 diff 背景"
    );

    // 行号那一列仍是静音：它不参与上色。
    let (at, row) = overlay_cell(&frame, 120, 24, "2 -let old");
    assert_eq!(frame[(at, row)].fg, palette::MUTED);
}

// --- 票 14：外部工具那一档 -----------------------------------------------------

/// 一个假的 diff 工具：`sh` 脚本做 `output` 那件事，stdin 是那份 diff。
///
/// 这台机器上 `delta` / `diff-so-fancy` / `colordiff` / `difft` 一个都没装
/// （`.scratch/diff-page/spec.md` 补记），所以这一档的真进程验证靠替身 —— 与 MCP 那个假
/// server 同一个做法。
fn fake_viewer(dir: &Path, output: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("fake-diff-viewer.sh");
    std::fs::write(&path, format!("#!/bin/sh\n{output}\n")).expect("写一个假工具");
    let mut permissions = std::fs::metadata(&path).expect("它的元数据").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("让它能跑");
    path
}

/// 一个改了内容的真仓库，加一个 `Modified` 的读数。
fn changed_repository() -> (tempfile::TempDir, changes::ChangedFile) {
    let root = repository();
    std::fs::write(root.path().join("tracked.txt"), "one\ntwo\n").expect("改一个文件");
    let file = changes::ChangedFile {
        path: "tracked.txt".to_owned(),
        kind: changes::Kind::Modified,
    };
    (root, file)
}

#[tokio::test]
async fn an_external_viewer_draws_its_own_output() {
    if !git_available() {
        return;
    }
    let (root, file) = changed_repository();
    let viewer = fake_viewer(root.path(), "printf '\\033[1;32m+twe\\033[0m\\n'");
    let settings = DiffViewerSettings {
        program: Some(viewer.display().to_string()),
        args: Vec::new(),
    };
    let (body, note) = changes::body(root.path(), &file, &settings, 100).await;
    assert!(note.is_none(), "跑成了就没有回执：{note:?}");

    let changes::Body::External {
        tool,
        lines,
        skipped,
    } = body
    else {
        panic!("外部工具那一档：{body:?}")
    };
    assert_eq!(tool, viewer.display().to_string());
    assert_eq!(skipped, 0);
    let text: String = lines[0].iter().map(|piece| piece.text.as_str()).collect();
    assert_eq!(text, "+twe", "它吐的文本原样画");
    assert_eq!(
        lines[0][0].style,
        changes::Sgr {
            fg: Some(ratatui::style::Color::Green),
            bold: true,
            ..changes::Sgr::default()
        },
        "它吐的颜色被认下来了"
    );
}

#[tokio::test]
async fn the_diff_goes_in_through_stdin_and_the_width_through_the_environment() {
    if !git_available() {
        return;
    }
    let (root, file) = changed_repository();
    // 把 stdin 转出来、并在前面记下 `COLUMNS`：两句都从这里看得见。
    let viewer = fake_viewer(root.path(), "printf 'COLUMNS=%s ' \"$COLUMNS\"\ncat");
    let settings = DiffViewerSettings {
        program: Some(viewer.display().to_string()),
        args: Vec::new(),
    };
    let (body, _note) = changes::body(root.path(), &file, &settings, 100).await;
    let changes::Body::External { lines, .. } = body else {
        panic!("外部工具那一档：{body:?}")
    };
    let text: String = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|piece| piece.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.starts_with("COLUMNS=100 "),
        "正文宽从环境变量进去（一条命令行参数都不注入）：{text}"
    );
    assert!(
        text.contains("diff --git"),
        "而那份 diff 从 stdin 进去：{text}"
    );
}

#[tokio::test]
async fn a_viewer_that_cannot_run_falls_back_and_leaves_a_receipt() {
    if !git_available() {
        return;
    }
    let (root, file) = changed_repository();
    let settings = DiffViewerSettings {
        program: Some("heng-not-a-real-diff-viewer".to_owned()),
        args: Vec::new(),
    };
    let (body, note) = changes::body(root.path(), &file, &settings, 100).await;
    assert!(
        matches!(body, changes::Body::Patch(_)),
        "回退内置那一档，正文永远还是那份 diff：{body:?}"
    );
    let note = note.expect("给一句回执");
    assert!(note.contains("heng-not-a-real-diff-viewer"), "{note}");
}

#[tokio::test]
async fn a_viewer_that_hangs_falls_back_after_the_timeout() {
    if !git_available() {
        return;
    }
    let (root, file) = changed_repository();
    let viewer = fake_viewer(root.path(), "sleep 30");
    let settings = DiffViewerSettings {
        program: Some(viewer.display().to_string()),
        args: Vec::new(),
    };
    let started = std::time::Instant::now();
    let (body, note) = changes::body(root.path(), &file, &settings, 100).await;
    assert!(matches!(body, changes::Body::Patch(_)));
    assert!(note.expect("给一句回执").contains("超时"));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "超时是墙钟的上限"
    );
}

#[tokio::test]
async fn a_nonzero_exit_falls_back_without_painting_the_stderr() {
    if !git_available() {
        return;
    }
    let (root, file) = changed_repository();
    let viewer = fake_viewer(root.path(), "echo 出错了 >&2\nexit 3");
    let settings = DiffViewerSettings {
        program: Some(viewer.display().to_string()),
        args: Vec::new(),
    };
    let (body, note) = changes::body(root.path(), &file, &settings, 100).await;
    let changes::Body::Patch(text) = &body else {
        panic!("回退内置那一档：{body:?}")
    };
    assert!(!text.contains("出错了"), "stderr 不许画进正文");
    assert!(note.expect("给一句回执").contains("没能画出"));
}

#[tokio::test]
async fn an_untracked_file_is_read_from_disk_and_never_asks_git() {
    // `git diff HEAD` 对未跟踪文件是空输出 + 退出码 0，拿它当正文只会画出一块空白 ——
    // 所以那一档直接读盘（`.scratch/diff-page/spec.md` §2）。
    let root = tempfile::TempDir::new().expect("临时工作区");
    std::fs::write(root.path().join("brand-new.txt"), "first\nsecond\n").expect("写一个文件");
    let file = changes::ChangedFile {
        path: "brand-new.txt".to_owned(),
        kind: changes::Kind::Untracked,
    };
    let (body, note) = changes::body(root.path(), &file, &DiffViewerSettings::default(), 100).await;
    assert!(note.is_none());
    assert_eq!(
        body,
        changes::Body::NewFile(heng::render::files::FileBody::Text {
            text: "first\nsecond\n".to_owned(),
            truncated: false,
        })
    );
}

#[tokio::test]
async fn an_untracked_file_goes_through_the_external_viewer_too() {
    // 票 06 的边界那一条：未跟踪文件的「那份 diff」是全文，读者选了外部呈现就一视同仁。
    let root = tempfile::TempDir::new().expect("临时工作区");
    std::fs::write(root.path().join("brand-new.txt"), "first\nsecond\n").expect("写一个文件");
    let viewer = fake_viewer(root.path(), "sed 's/^/S:/'");
    let settings = DiffViewerSettings {
        program: Some(viewer.display().to_string()),
        args: Vec::new(),
    };
    let file = changes::ChangedFile {
        path: "brand-new.txt".to_owned(),
        kind: changes::Kind::Untracked,
    };
    let (body, note) = changes::body(root.path(), &file, &settings, 100).await;
    assert!(note.is_none(), "{note:?}");
    let changes::Body::External { lines, .. } = body else {
        panic!("外部工具那一档：{body:?}")
    };
    let text: String = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|piece| piece.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("S:first"), "全文也喂给了它：{text}");
}

#[test]
fn the_dialog_signs_the_tool_and_keeps_its_colours_as_text() {
    let mut state = state();
    loaded(&mut state, &[("a.rs", changes::Kind::Modified)]);
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let row = sidebar_row_of(&rows, "a.rs") as u16;
    click(&mut state, 3, row);
    feed_diff(
        &mut state,
        changes::Body::External {
            tool: "delta".to_owned(),
            lines: vec![vec![
                changes::Piece {
                    text: "加的一行".to_owned(),
                    style: changes::Sgr {
                        fg: Some(ratatui::style::Color::Green),
                        ..changes::Sgr::default()
                    },
                },
                changes::Piece {
                    text: " 后半段".to_owned(),
                    style: changes::Sgr::default(),
                },
            ]],
            skipped: 0,
        },
    );

    let text = overlay(&mut state, 120, 24).join("\n");
    assert!(
        text.contains("a.rs · delta"),
        "标题右端标出是谁画的：{text}"
    );
    assert!(!text.contains('\u{1b}'), "转义序列不留在文本里：{text}");
    let frame = buffer(120, 24, &mut state);
    let (at, row) = overlay_cell(&frame, 120, 24, "加的一行");
    assert_eq!(frame[(at, row)].fg, ratatui::style::Color::Green);
    let (at, _row) = overlay_cell(&frame, 120, 24, "后半段");
    assert_eq!(
        frame[(at, row)].fg,
        ratatui::style::Color::Reset,
        "没有样式的那些片回到正文档"
    );
}

#[test]
fn a_falling_back_viewer_leaves_its_receipt_on_the_hint_line() {
    let mut state = state();
    loaded(&mut state, &[("a.rs", changes::Kind::Modified)]);
    open_changes(&mut state, 120, 24);
    let rows = sidebar(&mut state, 120, 24, 40);
    let row = sidebar_row_of(&rows, "a.rs") as u16;
    click(&mut state, 3, row);

    let (serial, _file, _columns) = state.take_diff_read().expect("打开那一刻就置了请求");
    state.diff_loaded(
        serial,
        changes::Body::Patch(small_patch()),
        Some(wording::changes_viewer_missing("delta")),
    );
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains(&wording::changes_viewer_missing("delta")),
        "回退时提示行说一句：{text}"
    );
    // 而正文仍是那份 diff —— 而且标题不带工具名（它不是外部工具画的）。
    let overlay_text = overlay(&mut state, 120, 24).join("\n");
    assert!(overlay_text.contains("@@ -1,3 +1,4 @@"), "{overlay_text}");
    assert!(!overlay_text.contains("· delta"), "{overlay_text}");
}

// --- 取数那一层：真 git 的临时仓库 -------------------------------------------

/// 这一台机器上有没有 git；没有就让真进程那几条安静地跳过（本仓库的规矩：环境缺件不是失败）。
fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// 在 `root` 里跑一条 git 命令，跑失败了就当测试出错。
fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("跑一条 git");
    assert!(status.success(), "git {args:?} 成功了");
}

/// 一个真的 git 仓库，带着主分支上的一次提交。
fn repository() -> tempfile::TempDir {
    let root = tempfile::TempDir::new().expect("临时仓库");
    git(root.path(), &["init", "-q", "-b", "main"]);
    git(root.path(), &["config", "user.name", "heng"]);
    git(
        root.path(),
        &["config", "user.email", "heng@example.invalid"],
    );
    std::fs::write(root.path().join("tracked.txt"), "one\n").expect("写一个文件");
    git(root.path(), &["add", "tracked.txt"]);
    git(root.path(), &["commit", "-qm", "init"]);
    root
}

fn as_pairs(outcome: &changes::Outcome) -> Vec<(String, changes::Kind)> {
    match outcome {
        changes::Outcome::Changed(files) => files
            .iter()
            .map(|file| (file.path.clone(), file.kind))
            .collect(),
        other => panic!("读数不是一份改动清单：{other:?}"),
    }
}

#[tokio::test]
async fn a_real_repository_reports_staged_unstaged_and_untracked_files() {
    if !git_available() {
        return;
    }
    let root = repository();
    // 未暂存：改一个已跟踪的文件。
    std::fs::write(root.path().join("tracked.txt"), "one\ntwo\n").expect("改一个文件");
    // 已暂存：新增一个并 `add`。
    std::fs::write(root.path().join("staged.txt"), "staged\n").expect("写一个文件");
    git(root.path(), &["add", "staged.txt"]);
    // 未跟踪：写一个什么都不做的。
    std::fs::write(root.path().join("brand-new.txt"), "new\n").expect("写一个文件");

    let files = as_pairs(&changes::status(root.path()).await);
    assert!(
        files.contains(&("tracked.txt".to_owned(), changes::Kind::Modified)),
        "未暂存的改动在：{files:?}"
    );
    assert!(
        files.contains(&("staged.txt".to_owned(), changes::Kind::Added)),
        "已暂存的新文件算「新增」：{files:?}"
    );
    assert!(
        files.contains(&("brand-new.txt".to_owned(), changes::Kind::Untracked)),
        "未跟踪的也在：{files:?}"
    );
}

#[tokio::test]
async fn untracked_directories_arrive_expanded_so_one_row_is_always_one_file() {
    if !git_available() {
        return;
    }
    let root = repository();
    std::fs::create_dir(root.path().join("d")).expect("建一个目录");
    std::fs::write(root.path().join("d/a.txt"), "a\n").expect("写一个文件");
    std::fs::write(root.path().join("d/b.txt"), "b\n").expect("写一个文件");

    let files = as_pairs(&changes::status(root.path()).await);
    assert!(
        files.contains(&("d/a.txt".to_owned(), changes::Kind::Untracked))
            && files.contains(&("d/b.txt".to_owned(), changes::Kind::Untracked)),
        "`-uall` 把未跟踪的目录摊成文件：{files:?}"
    );
    assert!(
        !files.iter().any(|(path, _)| path == "d/"),
        "目录自己不再占一行：{files:?}"
    );
}

#[tokio::test]
async fn an_empty_repository_without_head_still_lists_its_files() {
    // 还没有第一次提交时 `status` 照常成功（而 `diff HEAD` 会死）—— 这一页靠的就是它。
    if !git_available() {
        return;
    }
    let root = tempfile::TempDir::new().expect("临时仓库");
    git(root.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(root.path().join("first.txt"), "hi\n").expect("写一个文件");

    assert_eq!(
        as_pairs(&changes::status(root.path()).await),
        vec![("first.txt".to_owned(), changes::Kind::Untracked)]
    );
}

#[tokio::test]
async fn a_directory_that_is_not_a_repository_gets_its_own_answer() {
    if !git_available() {
        return;
    }
    let root = tempfile::TempDir::new().expect("临时目录");
    assert_eq!(
        changes::status(root.path()).await,
        changes::Outcome::NotARepo
    );
}

#[tokio::test]
async fn reading_the_changes_does_not_write_the_index_back() {
    // `git status` 默认会写回 index —— 这一条实测确凿，而不锁就是动用户的仓库
    // （`.scratch/diff-page/research/02-git-readouts.md` 第二节）。`--no-optional-locks`
    // 是那一行 `--no-lock` 的由来。
    if !git_available() {
        return;
    }
    let root = repository();
    let index = root.path().join(".git/index");
    // 让 index 显得陈旧、文件显得新 —— 那正是 git 想 refresh 并写回 index 的时机。
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    std::fs::File::options()
        .write(true)
        .open(&index)
        .expect("打开 index")
        .set_modified(old)
        .expect("把 index 的时间戳拨旧");
    std::fs::write(root.path().join("tracked.txt"), "one\ntwo\n").expect("改一个文件");

    let _ = changes::status(root.path()).await;
    let after = std::fs::metadata(&index)
        .expect("index 还在")
        .modified()
        .expect("它有 mtime");
    assert_eq!(after, old, "读一次不改动用户的 index");
}
