//! 重新打开一个会话：历史被重新铺回转录，以及它与这次运行
//! 追加的内容之间的接缝（`.scratch/tui-history-replay/spec.md`）。
//!
//! 接缝与 `render_layout.rs`、`ask_user_question_tui.rs` 测的是
//! 同一条：一个 `TuiState` 收渲染事件与前端请求，一块定尺的
//! `TestBackend` 出来，而键盘断言说的是循环会收到什么。重放走
//! 生产接缝驱动 —— 组装之后 CLI 推的那条
//! `ConsoleRequest::Replay` —— 并用循环调用的同一个 `replay_batch`
//! 推进，于是「走到一半」是测试可以站进去的一个状态，
//! 而不是它得去推断的东西。

use std::path::Path;

use fs_agent::events::{
    read_events, ContextSource, Event, EventPayload, HistoryReason, Role, SessionId, SpeakerId,
    StopReason, ToolCallId, Usage,
};
use fs_agent::render::{
    draw_frame, wording, ConsoleRequest, Key, RenderEvent, SessionFacts, TuiState,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::Terminal;

fn facts() -> SessionFacts {
    SessionFacts {
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: "~/code/fortystory/fs-agent".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        // 会话被组装时所处的模式；`ask` 是默认档，想测另一档的
        // 测试在自己的 facts 里说清楚。
        mode: fs_agent::permissions::Mode::Ask,
        budget_limit: Some(100_000),
        number_style: fs_agent::render::wording::NumberStyle::Cn,
        speaker_order: vec!["kimi".to_owned()],
    }
}

fn state() -> TuiState {
    TuiState::new(facts(), std::path::PathBuf::from("/x/fs-agent"), None)
}

/// 会话目录是 `dir` 的那个状态，工具详情就是从它这里读
/// `outputs/<id>.txt` 的。
fn state_in(dir: &Path) -> TuiState {
    TuiState::new(
        SessionFacts {
            session_dir: dir.display().to_string(),
            ..facts()
        },
        std::path::PathBuf::from("/x/fs-agent"),
        None,
    )
}

/// 循环在等一行的那个状态，于是 `Enter` 有个地方可以提交。
fn idle() -> (TuiState, tokio::sync::oneshot::Receiver<Option<String>>) {
    let mut state = state();
    let (reply, line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    (state, line)
}

// ---------------------------------------------------------------------------
// 事件
// ---------------------------------------------------------------------------

fn event(seq: u64, payload: EventPayload) -> Event {
    Event::new(seq, SpeakerId::Debater("kimi".into()), payload)
}

fn session_started(seq: u64) -> Event {
    event(
        seq,
        EventPayload::SessionStarted {
            session_id: SessionId::new("01J8ZQ4K7M"),
            cwd: "/workspace".to_owned(),
            schema_version: 1,
        },
    )
}

fn message(seq: u64, text: &str, reasoning: Option<&str>) -> Event {
    event(
        seq,
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: text.to_owned(),
            reasoning: reasoning.map(str::to_owned),
        },
    )
}

fn turn_started(seq: u64) -> Event {
    event(
        seq,
        EventPayload::TurnStarted {
            agent: SpeakerId::Debater("kimi".into()),
            iteration: 1,
        },
    )
}

fn turn_ended(seq: u64) -> Event {
    event(
        seq,
        EventPayload::TurnEnded {
            reason: StopReason::Completed,
        },
    )
}

fn usage(seq: u64, input: u64, output: u64, cached: u64, miss: u64) -> Event {
    event(
        seq,
        EventPayload::UsageRecorded {
            usage: Usage {
                input_tokens: input,
                output_tokens: output,
                cached_tokens: cached,
                miss_tokens: miss,
                reasoning_tokens: None,
            },
        },
    )
}

fn tool_started(seq: u64, id: &str, tool: &str, args: serde_json::Value) -> Event {
    event(
        seq,
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new(id),
            tool_name: tool.to_owned(),
            args,
        },
    )
}

fn tool_completed(
    seq: u64,
    id: &str,
    ok: bool,
    output: Option<&str>,
    error: Option<&str>,
) -> Event {
    event(
        seq,
        EventPayload::ToolCallCompleted {
            tool_call_id: ToolCallId::new(id),
            ok,
            output: output.map(str::to_owned),
            error: error.map(str::to_owned),
            duration_ms: 3,
        },
    )
}

fn plan_injected(seq: u64) -> Event {
    event(
        seq,
        EventPayload::ContextInjected {
            source: ContextSource::PlanMode,
            content: "PLAN.md".to_owned(),
        },
    )
}

fn mode_change(seq: u64) -> Event {
    event(
        seq,
        EventPayload::HistorySuperseded {
            targets: vec![1],
            reason: HistoryReason::ModeChange,
            summary: None,
        },
    )
}

// ---------------------------------------------------------------------------
// 驱动重放
// ---------------------------------------------------------------------------

/// 把一份历史交给状态，就像 CLI 在组装之后做的那样。
fn replay(state: &mut TuiState, events: Vec<Event>) {
    state.request(ConsoleRequest::Replay { events });
}

/// 一次一批地把重放跑完，就像循环做的那样。
fn run_replay(state: &mut TuiState) {
    while state.replay_pending() {
        state.replay_batch();
    }
}

/// 一份长到需要不止一批的历史：每条 `TurnEnded` 恰好画
/// 一行，所以 600 条没法一趟到达，而两批之间的那一帧
/// 是测试可以站进去的状态。这个数字故意不落在批次
/// 边界上：这里不断言一批装多少条事件。
fn long_history() -> Vec<Event> {
    (1..=600).map(turn_ended).collect()
}

// ---------------------------------------------------------------------------
// 那一帧
// ---------------------------------------------------------------------------

/// 按固定尺寸画一帧，再把屏幕读回来，一行一段文本。
fn screen(width: u16, height: u16, state: &mut TuiState) -> Vec<String> {
    let frame = buffer(width, height, state);
    (0..height).map(|y| row_text(&frame, y, width)).collect()
}

fn buffer(width: u16, height: u16, state: &mut TuiState) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("TestBackend");
    terminal
        .draw(|frame| draw_frame(frame, state))
        .expect("画一帧");
    terminal.backend().buffer().clone()
}

/// 120x40 下回合条那一列：画在其中的字符，空格丢掉。
///
/// 回合条坐在转录的最后一列，在帧里的 119 —— 滚动条占它
/// 前面那一列（`.scratch/tui-sidebar/spec.md` §1）。
fn turn_rail_shape(state: &mut TuiState) -> String {
    let frame = buffer(120, 40, state);
    (0..39u16)
        // 转录结束在主列第一条横线开始的地方。
        .take_while(|y| !row_text(&frame, *y, 120).ends_with('┄'))
        .map(|y| frame[(119, y)].symbol().chars().next().unwrap_or(' '))
        .filter(|ch| *ch != ' ')
        .collect()
}

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

fn row_of(state: &mut TuiState, width: u16, height: u16, needle: &str) -> Option<u16> {
    screen(width, height, state)
        .iter()
        .position(|row| row.contains(needle))
        .map(|row| row as u16)
}

fn click(column: u16, row: u16) -> ratatui::crossterm::event::MouseEvent {
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

/// 切到轨迹页：工具行与思考行现在只住在那里（`.scratch/trace-tab/spec.md` §2）。
fn open_trace_tab(state: &mut TuiState, width: u16, height: u16) {
    let row = row_of(state, width, height, wording::TAB_TRACE).expect("页签条在屏幕上");
    state.mouse(click(10, row));
    let _ = screen(width, height, state);
}

fn click_row(state: &mut TuiState, width: u16, height: u16, needle: &str) {
    let Some(row) = row_of(state, width, height, needle) else {
        panic!("屏幕上没有哪一行含 {needle:?}");
    };
    state.mouse(click(10, row));
}

/// 左栏里按距页首行的偏移取一个字段。
///
/// 这一页从页签条下面开始 —— 它下横线的后一行 —— 而字段是
/// 那些行里属于左栏自己的那一半，于是每个字段都以分隔线那一列收尾。
fn panel_field(rows: &[String], offset: usize) -> String {
    let mut rules = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.starts_with('┄'))
        .map(|(y, _)| y);
    rules.next().expect("页签条的上横线");
    let top = rules.next().expect("页签条的下横线") + 1;
    let row = &rows[top + offset];
    let end = row
        .char_indices()
        .find(|(_, ch)| matches!(ch, '┆' | '┄'))
        .map(|(index, _)| index)
        .expect("分隔线终结了左栏");
    row[..end].to_owned()
}

// ---------------------------------------------------------------------------
// 票 06 — 接缝、分帧与进度行
// ---------------------------------------------------------------------------

#[test]
fn a_replayed_history_is_laid_into_the_transcript() {
    // 全部要点：`--continue` 之后转录里装着上一场
    // 对话，而不是从空开始（spec §1，用户故事 1）。
    let mut state = state();
    replay(
        &mut state,
        vec![message(1, "昨天说的那件事，结论是 42。", None)],
    );
    assert!(state.replay_pending(), "这条请求开启重放");
    run_replay(&mut state);

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("昨天说的那件事，结论是 42。"),
        "历史在屏幕上：{text}"
    );
}

#[test]
fn an_empty_stream_never_enters_the_replay_state() {
    // 没有可铺的东西，所以既没有进度行也没有接缝：一次空的
    // `--continue` 读起来与它一直以来一模一样（spec §2，用户故事 25）。
    let mut state = state();
    replay(&mut state, Vec::new());
    assert!(!state.replay_pending(), "空历史不是一次重放");

    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains("恢复"), "也不画进度行：{text}");
}

#[test]
fn a_replay_in_flight_shows_partial_history_and_the_progress_count() {
    // 分帧契约：提示行说重放走到哪了，而它已经走到的那部分
    // 已经在屏幕上；等它流干，普通的状态行就回来
    // （spec §2、§4，用户故事 12）。这里断言计数是**部分**的，
    // 而不是某个特定数字：一批装多少事件是实现
    // 自己的事（spec §Testing Decisions）。
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("恢复历史") && text.contains("/600") && !text.contains("恢复历史 600/600"),
        "进度行显示它走到了哪：{text}"
    );
    assert!(
        text.contains("回合结束"),
        "而已经铺上去的那部分历史在屏幕上：{text}"
    );

    run_replay(&mut state);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains("恢复"), "历史铺完之后进度行就没了：{text}");
    assert!(
        text.contains("ctrl-c/ctrl-d 退出"),
        "而普通状态行回来了：{text}"
    );
}

#[test]
fn a_history_that_ends_exactly_on_a_batch_boundary_still_converges() {
    // 边界招来的那个差一错误：一条在批次用尽处
    // 用尽的流必须把重放关掉，而不是让它永远悬着。
    let mut state = state();
    replay(&mut state, (1..=1024).map(turn_ended).collect());
    run_replay(&mut state);
    assert!(!state.replay_pending(), "重放自己关上了");
    let text = screen(120, 20, &mut state).join("\n");
    assert!(!text.contains("恢复"), "也没在身后留下进度行：{text}");
}

#[test]
fn ctrl_o_is_ignored_while_a_replay_is_in_flight() {
    // 重放是一次性的界面临时态，键盘归它自己（`replay_key`），所以左栏
    // 开关在这个窗口里不生效；跑完之后照常算数
    // （`.scratch/sidebar-toggle/spec.md` §3）。
    let mut state = state();
    replay(&mut state, long_history());
    assert!(state.replay_pending(), "重放开着");

    let before = screen(120, 40, &mut state);
    assert!(before.join("\n").contains('┆'), "左栏还在");
    state.key(Key::CtrlO);
    let after = screen(120, 40, &mut state);
    assert_eq!(before, after, "重放期间 Ctrl-O 什么都不做");

    run_replay(&mut state);
    state.key(Key::CtrlO);
    let closed = screen(120, 40, &mut state);
    assert!(
        !closed.join("\n").contains('┆'),
        "重放结束之后它收得起来：{}",
        closed.join("\n")
    );
}

#[test]
fn the_progress_line_degrades_at_the_minimum_frame() {
    // 40×10 是最小的、还能画出东西的帧；它的提示行是 38 列，
    // 而计数就是在那里活了下来、短语没有（spec §4）。
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();

    let text = screen(40, 10, &mut state).join("\n");
    assert!(
        text.contains("恢复中") && text.contains("/600") && !text.contains("恢复中 600/600"),
        "最小帧下计数活下来了：{text}"
    );
    assert!(!text.contains("恢复历史"), "整个短语活不下来：{text}");
}

#[test]
fn the_pointer_does_nothing_while_a_replay_is_in_flight() {
    // 视口钉在底部，直到历史铺完：滚一格
    // 或点一下都会把它挪进还在到达的行里（spec §5，
    // 用户故事 16、34）。
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();

    let before = screen(120, 40, &mut state);
    state.mouse(ratatui::crossterm::event::MouseEvent {
        kind: ratatui::crossterm::event::MouseEventKind::ScrollUp,
        column: 40,
        row: 10,
        modifiers: ratatui::crossterm::event::KeyModifiers::empty(),
    });
    state.mouse(click(10, 5));
    let after = screen(120, 40, &mut state);
    assert_eq!(before, after, "指针什么都没改变");
}

#[test]
fn enter_does_not_submit_while_a_replay_is_in_flight() {
    // 等待的时间就是打字的时间，但 `Enter` 不许对着
    // 铺了一半的历史开一个回合；草稿留着，之后一次 `Enter`
    // 把它发出去（spec §3，用户故事 15、18）。
    let (mut state, mut answer) = idle();
    replay(&mut state, long_history());
    state.replay_batch();

    for ch in "half typed".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert!(answer.try_recv().is_err(), "历史还在到达的时候什么都没提交");

    run_replay(&mut state);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("half typed"), "而草稿还在那里：{text}");

    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().expect("重放结束后提交了"),
        Some("half typed".to_owned())
    );
}

#[test]
fn ctrl_c_quits_during_a_replay_and_ctrl_d_and_esc_are_inert() {
    // 重放不是一次运行：没有东西可取消，所以 `Ctrl-C` 是出路；
    // `Ctrl-D` 与 `Esc` 什么都不做（spec §5，用户故事 26、27）。
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();

    state.key(Key::CtrlD);
    assert!(!state.should_quit(), "重放期间 Ctrl-D 不退出");
    state.key(Key::Esc);
    assert!(!state.should_quit(), "重放期间 Esc 不退出");
    // 多行草稿上的 Esc 本来会提出清空它；重放期间
    // 它根本不许提这个问题。
    for ch in "one\ntwo".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Esc);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(!text.contains("清空"), "不提清空草稿的问题：{text}");

    // `Ctrl-C` 是出路，但它也走双击（`.scratch/exit-gesture/spec.md` §1）：第一下举手、
    // 第二下退。
    state.key(Key::CtrlC);
    assert!(!state.should_quit(), "第一下只举手");
    state.key(Key::CtrlC);
    assert!(state.should_quit(), "Ctrl-C 双击是重放的出路");
}

#[test]
fn ctrl_z_suspends_even_during_a_replay() {
    // 挂起是终端层手势，不属于任何一个视图的键位表 —— 重放中它照常可用，而且它不退出、
    // 不打断重放、也不推任何手势（`.scratch/suspend-gesture/spec.md` §2）。
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();

    state.key(Key::CtrlZ);
    assert!(state.take_suspend_request(), "重放中也能挂起");
    assert!(!state.should_quit(), "挂起不是退出");
    assert!(state.replay_pending(), "重放本身照旧等着下一批");
    assert!(state.take_events().is_empty(), "它不推任何手势");
}

#[test]
fn a_raised_gesture_takes_over_the_replay_progress_line() {
    // 举手期间进度行让位给那句催促 —— 它是**整条替换**，不是追加
    // （`.scratch/exit-gesture/spec.md` §2）。
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();
    let before = screen(120, 40, &mut state).join("\n");
    assert!(before.contains("恢复历史"), "进度行本来在：{before}");

    state.key(Key::CtrlC);
    let after = screen(120, 40, &mut state).join("\n");
    assert!(
        after.contains("再按一次 ctrl-c 退出"),
        "举手之后那句催促在：{after}"
    );
    assert!(!after.contains("恢复历史"), "而进度行让了位：{after}");
}

#[test]
fn the_scroll_keys_are_ignored_during_a_replay() {
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();

    let before = screen(120, 40, &mut state);
    for key in [Key::PageUp, Key::PageDown, Key::CtrlG] {
        state.key(key);
    }
    let after = screen(120, 40, &mut state);
    assert_eq!(before, after, "转录保持钉在底部");
}

#[test]
fn a_live_event_arriving_mid_replay_waits_for_the_history() {
    // banner 是在历史还在铺的时候发过来的；它不许
    // 出现在历史中间（spec §3，用户故事 19、21）。
    let mut state = state();
    replay(&mut state, long_history());
    state.replay_batch();
    state.live_event(RenderEvent::Notice("fs-agent 启动".to_owned()));

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("fs-agent 启动"),
        "历史还在到达的时候 banner 被压着：{text}"
    );

    run_replay(&mut state);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("fs-agent 启动"),
        "历史铺完之后才落上去：{text}"
    );
}

#[test]
fn a_logged_event_arriving_mid_replay_is_not_painted_twice() {
    // 重新打开的会话在做恢复时，会把合成结果写进日志 **并且**
    // 在渲染通道上发出它们，而重放的快照里握着同一批
    // 事件。接缝之后那条实时副本不许再画一条调用行。
    let mut events: Vec<Event> = (1..=600).map(turn_ended).collect();
    events.push(tool_started(
        601,
        "call-recovered",
        "bash",
        serde_json::json!({"command": "ls"}),
    ));
    events.push(tool_completed(
        602,
        "call-recovered",
        false,
        None,
        Some("the session was interrupted while this call was in flight"),
    ));
    let recovery = events[601].clone();

    let mut state = state();
    replay(&mut state, events);
    state.replay_batch();
    // 快照里已经握着的那条结果从渲染通道到达。
    state.live_event(RenderEvent::Logged(recovery));
    run_replay(&mut state);
    open_trace_tab(&mut state, 120, 40);

    let rows = screen(120, 40, &mut state);
    let calls = rows.iter().filter(|row| row.contains("▸ 调用")).count();
    assert_eq!(calls, 1, "被恢复的那条调用只画一次：{rows:?}");
}

#[test]
fn a_finished_replay_sits_at_the_bottom_with_no_indicator() {
    // 读者追上了：没有「N 行新内容」那条横条，而最新的一行历史
    // 就是窗格底部那一行（spec §2，用户故事 17）。
    let mut state = state();
    replay(
        &mut state,
        (1..=40)
            .map(|seq| message(seq, &format!("第 {seq} 段历史"), None))
            .collect(),
    );
    run_replay(&mut state);

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains(wording::back_to_bottom()),
        "没有可回到底部的地方：{text}"
    );
    assert!(
        text.contains("第 40 段历史"),
        "而最后一行历史在屏幕上：{text}"
    );
}

// ---------------------------------------------------------------------------
// 票 07 — 分隔行、面板与 header 的模式
// ---------------------------------------------------------------------------

#[test]
fn the_divider_separates_history_from_what_this_run_adds() {
    // `[历史] → [分隔行] → [banner]`，顺序正是这个
    // （spec §6，用户故事 20、21）。
    let mut state = state();
    replay(
        &mut state,
        vec![session_started(1), message(2, "上一段的回答", None)],
    );
    state.live_event(RenderEvent::Notice("本段的 banner".to_owned()));
    run_replay(&mut state);

    let rows = screen(120, 40, &mut state);
    let history = rows
        .iter()
        .position(|row| row.contains("上一段的回答"))
        .expect("历史画出来了");
    let divider = rows
        .iter()
        .position(|row| row.contains(wording::history_divider()))
        .expect("接缝画出来了");
    let banner = rows
        .iter()
        .position(|row| row.contains("本段的 banner"))
        .expect("banner 画出来了");
    assert!(
        history < divider && divider < banner,
        "历史在接缝之前、接缝在 banner 之前：{rows:?}"
    );
}

#[test]
fn a_history_that_drew_nothing_gets_no_divider() {
    // 一条空流与一副光秃秃的骨架什么都不画，所以没有可标记的
    // 接缝（spec §6，用户故事 25）。
    for events in [Vec::new(), vec![session_started(1)]] {
        let mut state = state();
        replay(&mut state, events.clone());
        run_replay(&mut state);
        let text = screen(120, 40, &mut state).join("\n");
        assert!(
            !text.contains(wording::history_divider()),
            "{events:?} 画了一条分隔行：{text}"
        );
    }
}

#[test]
fn the_divider_is_a_render_layer_line_that_is_fresh_on_every_reopen() {
    // 它从不进日志：把同一条组装好的流重放两次，留下两条
    // 接缝，而两份历史里面都没有第三条（spec §6，用户故事 22）。
    let history = vec![message(1, "旧的一段", None)];
    let mut state = state();
    replay(&mut state, history.clone());
    run_replay(&mut state);
    replay(&mut state, history);
    run_replay(&mut state);

    let rows = screen(120, 40, &mut state);
    let seams = rows
        .iter()
        .filter(|row| row.contains(wording::history_divider()))
        .count();
    assert_eq!(seams, 2, "每重新打开一次一条新接缝：{rows:?}");
}

#[test]
fn the_panel_adds_up_the_history_and_keeps_accumulating() {
    // 面板是应用每个块带来的副作用，所以历史也计数、实时用量从
    // 那里接着走，而不是重新开始（spec §7，用户故事 23）。
    let mut state = state();
    replay(
        &mut state,
        vec![
            usage(1, 100, 20, 30, 70),
            turn_ended(2),
            message(3, "历史上的一轮", None),
        ],
    );
    run_replay(&mut state);

    // 接缝之后到达一条实时调用。
    state.live_event(RenderEvent::Logged(usage(4, 50, 10, 0, 50)));
    state.live_event(RenderEvent::Logged(turn_ended(5)));

    let rows = screen(120, 40, &mut state);
    let tokens = panel_field(&rows, 1);
    let turns = panel_field(&rows, 2);
    let input = panel_field(&rows, 3);
    assert!(tokens.contains("180"), "100+20+50+10: {tokens:?}");
    assert!(turns.contains('2'), "两个回合：{turns:?}");
    assert!(input.contains("150"), "100+50 输入：{input:?}");
}

#[test]
fn the_rail_grows_with_the_replayed_history() {
    // 回合条与别的东西一样从流派生，所以重新打开的会话会发现自己的
    // 那些回合已经在那一列上了 —— 而重放期间它一次填一批，
    // 而不是整根一次出现（spec §4）。
    let mut state = state();
    let history: Vec<Event> = (1..=40).map(turn_ended).collect();
    replay(&mut state, history);
    run_replay(&mut state);

    // 四十个回合，全被裁到这一列自己的高度：顶部是那个标记，
    // 最新那个回合在脚下、是焦点，因为重放结束后视口
    // 回到底部。
    let shape = turn_rail_shape(&mut state);
    assert_eq!(
        shape.chars().next(),
        Some('⋮'),
        "这一列说更旧的回合在上面：{shape}"
    );
    assert_eq!(
        shape.chars().last(),
        Some('┃'),
        "而最新那个回合是焦点：{shape}"
    );
    assert_eq!(shape.matches('┃').count(), 1, "恰好一个焦点格：{shape}");

    // 接缝之后一次实时回合加上它自己的格，于是历史与这场会话共用一根
    // 回合条，而不是另起一根。
    let before = shape.len();
    state.live_event(RenderEvent::Logged(turn_ended(41)));
    let after = turn_rail_shape(&mut state);
    assert!(
        after.len() >= before && after.ends_with('┃'),
        "新回合占了自己的格，焦点也移到了它上面：{after}"
    );
}

#[test]
fn the_two_retired_variants_still_deserialize_off_an_old_stream() {
    // 这两个变体为什么还留在 schema 里：在 `.scratch/todo-and-modes`
    // 之前写下的会话带着这两条事件，而 `--continue` 在任何渲染器看到它之前
    // 先用 `read_events` 解析那条流。把这些行写成 JSONL 再读回来，是
    // 钉住「schema 仍然接受它们」最诚实的办法 —— 像这样的
    // 一次移除正是它出问题的地方
    // （ADR 0003）。
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.jsonl");
    let lines: Vec<String> = [plan_injected(1), mode_change(2)]
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect();
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();

    // 是 payload，不是整条事件：`at` 是墙上时钟的戳，所以同一个 payload 的
    // 两条 Event 除了写下的那一瞬间之外处处相等。
    let read = read_events(&path).expect("旧的流仍然能解析");
    let payloads: Vec<EventPayload> = read.into_iter().map(|event| event.payload).collect();
    assert_eq!(
        payloads,
        vec![plan_injected(1).payload, mode_change(2).payload]
    );
}

#[test]
fn an_old_streams_plan_events_still_replay_without_moving_the_mode() {
    // 旧的流带着计划模式过去会发出的那两条事件 —— 一次 `PlanMode`
    // 注入，以及退役掉它的那条 `ModeChange`。两个变体都留在
    // schema 里，好让这样的流仍然能反序列化并重放（ADR 0003），而行上的
    // 文本仍然说它当时说的话。它们不再做的事是挪动会话的
    // 模式：模式是前端被组装时拿到的那个会话值
    // （`.scratch/todo-and-modes/spec.md` §1）。
    for events in [
        vec![plan_injected(1), mode_change(2)],
        vec![plan_injected(1)],
        vec![message(1, "没有模式事件", None)],
    ] {
        let mut state = state();
        replay(&mut state, events.clone());
        run_replay(&mut state);
        let text = screen(120, 40, &mut state).join("\n");
        assert!(
            text.contains("模式 询问"),
            "{events:?} 保持组装时的那一档模式：{text}"
        );
    }

    // 注入本身仍然可读 —— 那一行点名了它来自哪个来源 ——
    // 所以旧会话读回来还是它当时的样子。
    let mut state = state();
    replay(&mut state, vec![plan_injected(1)]);
    run_replay(&mut state);
    // 注入行住在轨迹页里（票 10），状态行上的模式两边都看得见。
    open_trace_tab(&mut state, 120, 40);
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("上下文注入：计划模式"), "{text}");
}

// ---------------------------------------------------------------------------
// 票 08 — 历史行打开同一个详情覆盖层
// ---------------------------------------------------------------------------

#[test]
fn a_history_tool_line_opens_the_same_detail_overlay() {
    // 历史行走的是同一个 `apply`，所以它们带着同一张命中表：
    // 点一条被重放的工具调用，打开的是实时调用会打开的那个详情
    // （spec §8，用户故事 6）。
    let mut state = state();
    replay(
        &mut state,
        vec![
            tool_started(1, "call-h1", "bash", serde_json::json!({"command": "ls"})),
            tool_completed(2, "call-h1", true, Some("a\nb"), None),
        ],
    );
    run_replay(&mut state);
    open_trace_tab(&mut state, 120, 40);

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("── 参数 ──"),
        "覆盖层开在那条历史行上：{text}"
    );
    assert!(text.contains("\"command\""), "带着参数：{text}");
}

#[test]
fn a_history_thinking_line_opens_its_recorded_trace() {
    // `MessageCompleted.reasoning` 是走完的思考轨迹住的地方，所以一条
    // 带着它的重放消息有一条可点的「思考完成」行（spec §8，用户故事 7）。
    let mut state = state();
    replay(
        &mut state,
        vec![
            turn_started(1),
            message(2, "答案是 42。", Some("先看依赖，再看测试。")),
        ],
    );
    run_replay(&mut state);
    open_trace_tab(&mut state, 120, 40);

    click_row(&mut state, 120, 40, "思考完成");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("先看依赖，再看测试。"),
        "记下的轨迹被显示出来：{text}"
    );
}

#[test]
fn a_history_without_recorded_reasoning_has_no_thinking_line() {
    // 合成器的形状：增量流过，什么都没写下来。没有
    // 可伪造的轨迹，也没有可打开的空白框（spec §8，用户故事 11）。
    let mut state = state();
    replay(
        &mut state,
        vec![turn_started(1), message(2, "没有留下推理", None)],
    );
    run_replay(&mut state);

    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("思考完成"),
        "没有轨迹的消息就没有思考行：{text}"
    );
}

/// 一条工具调用的重放历史，它的预览带着 `output`。
fn history_with_tool_output(id: &str, output: Option<&str>) -> Vec<Event> {
    vec![
        tool_started(1, id, "bash", serde_json::json!({"command": "cat big"})),
        tool_completed(2, id, true, output, None),
    ]
}

#[test]
fn a_history_detail_reads_the_spilled_tool_output() {
    // 事件带着预览；整段文本是调用 id 在会话目录下
    // 点名的那个文件（spec §8）。
    let dir = tempfile::tempdir().expect("一个会话目录");
    let outputs = dir.path().join("outputs");
    std::fs::create_dir_all(&outputs).expect("会话的 outputs 目录");
    std::fs::write(
        outputs.join("call-h2.txt"),
        "the whole output\nwith a second line the preview never carried",
    )
    .expect("溢出的那个文件");

    let mut state = state_in(dir.path());
    replay(
        &mut state,
        history_with_tool_output("call-h2", Some("the whole output\n[已截断：999 字符]")),
    );
    run_replay(&mut state);
    open_trace_tab(&mut state, 120, 40);

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("with a second line the preview never carried"),
        "溢出的那个文件整段显示：{text}"
    );
    assert!(!text.contains("全文不可用"), "而没有降级：{text}");
}

#[test]
fn a_history_detail_degrades_when_the_spilled_file_is_gone() {
    // `prune`，或者有人手删了那个文件：预览加上一句说
    // 它不是全部的话（spec §8，用户故事 9）。
    let dir = tempfile::tempdir().expect("一个会话目录");
    let mut state = state_in(dir.path());
    replay(
        &mut state,
        history_with_tool_output("call-h3", Some("head of the output\n[已截断：999 字符]")),
    );
    run_replay(&mut state);
    open_trace_tab(&mut state, 120, 40);

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("head of the output"), "预览：{text}");
    assert!(text.contains("全文不可用"), "降级：{text}");
}

#[test]
fn a_history_detail_degrades_when_the_spilled_file_is_empty() {
    let dir = tempfile::tempdir().expect("一个会话目录");
    let outputs = dir.path().join("outputs");
    std::fs::create_dir_all(&outputs).expect("会话的 outputs 目录");
    std::fs::write(outputs.join("call-h4.txt"), "").expect("空文件");

    let mut state = state_in(dir.path());
    replay(
        &mut state,
        history_with_tool_output("call-h4", Some("head of the output\n[已截断：999 字符]")),
    );
    run_replay(&mut state);
    open_trace_tab(&mut state, 120, 40);

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(text.contains("head of the output"), "预览：{text}");
    assert!(text.contains("全文不可用"), "降级：{text}");
}

#[test]
fn a_history_result_without_the_truncation_note_is_its_own_full_text() {
    // 没有那句话就说明从没写过溢出文件：事件里的文本**就是**
    // 全部正文，说它不可用会是一句假警告。这也是
    // `--continue` 为一条悬着的调用写下的形状（spec §8，用户故事 10）。
    let dir = tempfile::tempdir().expect("一个会话目录");
    let mut state = state_in(dir.path());
    replay(
        &mut state,
        vec![
            tool_started(1, "call-h5", "bash", serde_json::json!({"command": "ls"})),
            tool_completed(
                2,
                "call-h5",
                false,
                None,
                Some("the session was interrupted while this call was in flight"),
            ),
        ],
    );
    run_replay(&mut state);
    open_trace_tab(&mut state, 120, 40);

    click_row(&mut state, 120, 40, "调用 bash");
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        text.contains("interrupted while this call was in flight"),
        "事件里的文本被显示出来：{text}"
    );
    assert!(!text.contains("全文不可用"), "也没有谎称降级：{text}");
}

#[test]
fn the_divider_and_section_lines_are_not_clickable() {
    // 只有读者能打开的折叠行才带那个标记；接缝与
    // 回合横线都只是文本（spec §8）。
    let mut state = state();
    replay(
        &mut state,
        vec![
            message(1, "历史", None),
            turn_ended(2),
            message(3, "又一段", None),
        ],
    );
    run_replay(&mut state);

    let rows = screen(120, 40, &mut state);
    let seam = rows
        .iter()
        .position(|row| row.contains(wording::history_divider()))
        .expect("接缝画出来了");
    state.mouse(click(10, seam as u16));
    let text = screen(120, 40, &mut state).join("\n");
    assert!(
        !text.contains("── 参数 ──") && !text.contains("── 推理 ──"),
        "点接缝什么都不开：{text}"
    );
}

#[test]
fn a_history_detail_freezes_the_viewport_and_releases_it() {
    // 覆盖层就是实时那一个，所以历史这条路继承它的视口契约：
    // 打开会冻住转录，其间到达的输出不会把它
    // 拽走，而关掉会回到底部（spec §8，用户故事 35）。
    let mut state = state();
    replay(
        &mut state,
        vec![
            tool_started(1, "call-h6", "bash", serde_json::json!({"command": "ls"})),
            tool_completed(2, "call-h6", true, Some("body"), None),
        ],
    );
    run_replay(&mut state);
    open_trace_tab(&mut state, 120, 40);

    click_row(&mut state, 120, 40, "调用 bash");
    assert!(
        screen(120, 40, &mut state)
            .join("\n")
            .contains("── 参数 ──"),
        "历史详情是开着的"
    );

    // 覆盖层挂着的时候输出到达：它被追加，而不是被跟随。
    state.live_event(RenderEvent::Notice("历史详情打开时的新内容".to_owned()));
    let frozen = screen(120, 40, &mut state);
    assert!(
        frozen.iter().any(|row| row.contains("── 参数 ──")),
        "正在读的仍然是那个覆盖层：{frozen:?}"
    );

    // `Esc` 关上它，视口回到最底下 —— 连它冻住期间
    // 到达的那一行也算上。
    state.key(Key::Esc);
    let after = screen(120, 40, &mut state).join("\n");
    assert!(
        after.contains("历史详情打开时的新内容"),
        "关掉会回到最新的那一行：{after}"
    );
    assert!(
        !after.contains(wording::back_to_bottom()),
        "而视口又开始跟随了：{after}"
    );
}
