//! 时间轴那三行横带的纯函数层：span 表与一格换算（`.scratch/trace-ledger/spec.md` §10、票 22）。
//!
//! 帧层只进高风险处（票的「测试决定」），而这一票的大部分逻辑都在纯算术里：哪些格是谁的、
//! 不足一格的段画不画、刻度留几个、窄到什么时候退成哪一档。都在这里逐条钉住。

use chrono::{DateTime, TimeDelta, Utc};
use heng::events::{ParticipantId, SpeakerId, ToolCallId};
use heng::render::timeline::{Axis, BandKind, LABEL_COLUMNS, ModelCell, Timeline, ticks};

/// 一个固定时刻打底：测试里的秒数都是相对它数的。
fn t0() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).expect("一个固定时刻")
}

/// 第 `seconds` 秒那一刻。
fn at(seconds: i64) -> DateTime<Utc> {
    t0() + TimeDelta::seconds(seconds)
}

/// 第 `ms` 毫秒那一刻。
fn ms(milliseconds: i64) -> DateTime<Utc> {
    t0() + TimeDelta::milliseconds(milliseconds)
}

fn kimi() -> SpeakerId {
    SpeakerId::Debater(ParticipantId::from("kimi".to_owned()))
}

fn sandy() -> SpeakerId {
    SpeakerId::Debater(ParticipantId::from("sandy".to_owned()))
}

fn call(name: &str) -> ToolCallId {
    ToolCallId::from(name.to_owned())
}

/// 一条 20 秒的会话铺在 20 格上：一格一秒，好让断言里的格号就是秒数。
fn one_second_a_cell() -> Axis {
    Axis::new(at(0), at(20), 20).expect("轴")
}

#[test]
fn a_model_span_runs_to_the_next_start_of_the_same_speaker() {
    let mut timeline = Timeline::default();
    timeline.turn_started(&kimi(), 1, at(0));
    timeline.turn_started(&kimi(), 2, at(10));

    let cells = timeline.model_cells(one_second_a_cell(), at(10));
    assert_eq!(
        cells[..10],
        [Some(ModelCell::Decode); 10],
        "第一次调用占了头十格：{cells:?}"
    );
    assert!(
        cells[10..].iter().all(Option::is_none),
        "第二次调用还没收尾，但它的起点把上一段收住了：{cells:?}"
    );
}

#[test]
fn the_units_end_closes_the_last_model_span() {
    let mut timeline = Timeline::default();
    timeline.turn_started(&kimi(), 1, at(0));
    timeline.turn_started(&kimi(), 2, at(5));
    timeline.turn_ended(&kimi(), at(9));

    let cells = timeline.model_cells(one_second_a_cell(), at(20));
    assert_eq!(
        cells[..5],
        [Some(ModelCell::Decode); 5],
        "第一段：{cells:?}"
    );
    assert_eq!(
        cells[5..9],
        [Some(ModelCell::Decode); 4],
        "第二段：{cells:?}"
    );
    assert!(
        cells[9..].iter().all(Option::is_none),
        "收尾之后没有第三段：{cells:?}"
    );
}

#[test]
fn another_speakers_span_keeps_running_when_one_of_them_ends() {
    // 讨论会话里两个发言者各跑各的：一个人收尾不该把另一个人也关掉。
    let mut timeline = Timeline::default();
    timeline.turn_started(&kimi(), 1, at(0));
    timeline.turn_started(&sandy(), 1, at(2));
    timeline.turn_ended(&kimi(), at(6));

    let cells = timeline.model_cells(one_second_a_cell(), at(10));
    assert_eq!(
        cells[..6],
        [Some(ModelCell::Decode); 6],
        "kimi 那一段：{cells:?}"
    );
    assert_eq!(
        cells[6..10],
        [Some(ModelCell::Decode); 4],
        "sandy 那一段：{cells:?}"
    );
}

#[test]
fn the_first_token_splits_the_model_span_in_two() {
    let mut timeline = Timeline::default();
    timeline.turn_started(&kimi(), 1, at(0));
    timeline.message_completed(&kimi(), Some(4_000));
    timeline.turn_ended(&kimi(), at(10));

    let cells = timeline.model_cells(one_second_a_cell(), at(10));
    assert_eq!(
        cells[..4],
        [Some(ModelCell::Wait); 4],
        "等首 token：{cells:?}"
    );
    assert_eq!(
        cells[4..10],
        [Some(ModelCell::Decode); 6],
        "吐字：{cells:?}"
    );
}

#[test]
fn a_segment_shorter_than_one_cell_is_not_drawn() {
    // 一格一秒：0.4 秒的等首段与 0.6 秒的吐字段都不到一格，两段都不画（宁缺勿假）。
    let mut timeline = Timeline::default();
    timeline.turn_started(&kimi(), 1, at(0));
    timeline.message_completed(&kimi(), Some(400));
    timeline.turn_ended(&kimi(), ms(1_000));

    let cells = timeline.model_cells(one_second_a_cell(), at(20));
    assert!(
        cells[..2].iter().all(Option::is_none),
        "不到一格的两段都不画：{cells:?}"
    );
}

#[test]
fn an_old_session_draws_the_model_span_in_one_piece() {
    // 老流没有首 token 时刻：整段画，不插占位符、也不切（票 22 第 2 条）。
    let mut timeline = Timeline::default();
    timeline.turn_started(&kimi(), 1, at(0));
    timeline.message_completed(&kimi(), None);
    timeline.turn_ended(&kimi(), at(10));

    let cells = timeline.model_cells(one_second_a_cell(), at(10));
    assert!(
        cells[..10]
            .iter()
            .all(|cell| *cell == Some(ModelCell::Decode)),
        "整段都是模型在跑：{cells:?}"
    );
    assert!(
        !cells.contains(&Some(ModelCell::Wait)),
        "没有首 token 时刻就没有「等首 token」那一段：{cells:?}"
    );
}

#[test]
fn concurrent_tool_calls_stack_in_the_tool_lane() {
    let mut timeline = Timeline::default();
    timeline.tool_started(&call("bash"), at(0));
    timeline.tool_started(&call("read"), at(4));
    timeline.tool_completed(&call("bash"), at(6));
    timeline.tool_completed(&call("read"), at(10));

    let depth = timeline.tool_depth(one_second_a_cell(), at(10));
    assert_eq!(depth[..4], [1; 4], "只有 bash：{depth:?}");
    assert_eq!(depth[4..6], [2; 2], "两次调用叠着：{depth:?}");
    assert_eq!(depth[6..10], [1; 4], "只剩 read：{depth:?}");
    assert!(
        depth[10..].iter().all(|cell| *cell == 0),
        "后面是空档：{depth:?}"
    );
}

#[test]
fn a_running_span_is_drawn_up_to_now() {
    let mut timeline = Timeline::default();
    timeline.turn_started(&kimi(), 1, at(0));
    timeline.tool_started(&call("bash"), at(1));

    let (start, end) = timeline.domain(at(6)).expect("有事的轴");
    assert_eq!(start, at(0));
    assert_eq!(end, at(6), "进行中的那些段把轴的右端带到当前时刻");

    let axis = Axis::new(start, end, 10).expect("轴");
    let cells = timeline.model_cells(axis, at(6));
    assert!(
        cells.iter().all(|cell| *cell == Some(ModelCell::Decode)),
        "还在跑的那一段画到当前时刻：{cells:?}"
    );
}

#[test]
fn the_axis_covers_the_whole_session_and_does_not_slide() {
    // 轴域是整趟会话：多来一条工具调用只把右端拉长一点，左端一个字节都不动。
    let mut timeline = Timeline::default();
    timeline.turn_started(&kimi(), 1, at(0));
    timeline.tool_completed(&call("bash"), at(30));

    let (start, end) = timeline.domain(at(30)).expect("有事的轴");
    assert_eq!((start, end), (at(0), at(30)));
}

#[test]
fn every_event_stretches_the_domain() {
    // 轴域是整趟会话：末尾一条与两条泳道无关的事件（注入、用量、通知）也把右端撑开 ——
    // 于是它之后的空白读作留白，而不是被裁掉。
    let mut timeline = Timeline::default();
    timeline.turn_started(&kimi(), 1, at(0));
    timeline.turn_ended(&kimi(), at(5));
    timeline.observed(at(30));

    let (start, end) = timeline.domain(at(30)).expect("有事的轴");
    assert_eq!((start, end), (at(0), at(30)));
}

#[test]
fn a_lone_message_still_gives_the_axis_something_to_draw() {
    // 一个刚开出来的会话：第一条用户消息还没配上一次 `TurnStarted`，但轴已经从这里起算。
    let mut timeline = Timeline::default();
    timeline.observed(at(0));
    let (start, end) = timeline.domain(at(0)).expect("有事的轴");
    assert_eq!((start, end), (at(0), at(0)), "只有一刻的轴没有跨度");
    assert!(Axis::new(start, end, 20).is_none(), "跨度为零就不画轴");
}

#[test]
fn nothing_has_happened_yet_is_an_empty_domain() {
    let timeline = Timeline::default();
    assert!(timeline.is_empty());
    assert!(timeline.domain(at(10)).is_none(), "没有时间可以铺就不画轴");
}

#[test]
fn the_unit_boundary_gets_a_mark_and_an_iteration_boundary_does_not() {
    let mut timeline = Timeline::default();
    timeline.turn_started(&kimi(), 1, at(0));
    timeline.turn_started(&kimi(), 2, at(5));
    timeline.turn_started(&kimi(), 3, at(9));

    let marks = timeline.turn_marks(one_second_a_cell());
    assert_eq!(marks, vec![0], "只有起始迭代为 1 的那一条算边界：{marks:?}");
}

#[test]
fn a_discussion_round_also_marks_its_boundary() {
    let mut timeline = Timeline::default();
    timeline.round_started(at(3));
    timeline.turn_started(&kimi(), 1, at(3));

    let marks = timeline.turn_marks(one_second_a_cell());
    assert_eq!(marks, vec![3], "同一刻的轮次与起始迭代只画一道：{marks:?}");
}

#[test]
fn the_tick_row_keeps_five_labels_until_the_axis_narrows() {
    let axis = Axis::new(at(0), at(60), 60).expect("轴");
    let full = ticks(axis, BandKind::Full);
    assert_eq!(full.len(), 5, "完整档是起止加三个中点：{full:?}");
    assert_eq!(full.first().map(|(column, _)| *column), Some(0));
    assert_eq!(
        full.last().map(|(column, _)| *column),
        Some(60 - 8),
        "最后一个标签贴着右缘：{full:?}"
    );

    let sparse = ticks(axis, BandKind::Sparse);
    assert_eq!(sparse.len(), 2, "稀疏档只留起止：{sparse:?}");

    let none = ticks(axis, BandKind::Empty);
    assert!(none.is_empty(), "整条不画时一个标签都没有：{none:?}");
}

#[test]
fn the_band_falls_back_three_steps_as_the_axis_narrows() {
    // 120 列终端上横轴 63 格（票 22 的宽度口径）：完整档。
    assert_eq!(BandKind::of_axis(63), BandKind::Full);
    // 80 列终端（窄左栏）：35 格，退成稀疏刻度。
    assert_eq!(BandKind::of_axis(35), BandKind::Sparse);
    // 40 列终端（没有左栏）：24 格，再退成一行三态版。
    assert_eq!(BandKind::of_axis(24), BandKind::Compact);
    // 再窄就是整条不画。
    assert_eq!(BandKind::of_axis(8), BandKind::Empty);
    assert_eq!(BandKind::of_axis(0), BandKind::Empty);
}

#[test]
fn degrading_gives_the_rows_back_to_the_ledger() {
    assert_eq!(BandKind::Full.rows(), 3);
    assert_eq!(BandKind::Sparse.rows(), 3);
    assert_eq!(BandKind::Compact.rows(), 2, "一行三态版省下一行");
    assert_eq!(BandKind::Empty.rows(), 0, "整条让位时一行都不占");
    assert!(BandKind::Full.labels(), "两条泳道要有个名字");
    assert!(!BandKind::Compact.labels(), "一行三态版把那五列还给轴");
}

#[test]
fn the_lane_label_takes_five_columns() {
    assert_eq!(LABEL_COLUMNS, 5, "`模型 ` 两个汉字加一格空白");
}
