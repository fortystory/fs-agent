//! 时间轴那三行横带的纯函数层：span 表与一格换算（`.scratch/trace-ledger/spec.md` §10、票 22）。
//!
//! 帧层只进高风险处（票的「测试决定」），而这一票的大部分逻辑都在纯算术里：哪些格是谁的、
//! 不足一格的段画不画、刻度留几个、窄到什么时候退成哪一档。都在这里逐条钉住。

use chrono::{DateTime, TimeDelta, Utc};
use heng::events::{ParticipantId, SpeakerId, ToolCallId};
use heng::render::timeline::{
    Axis, BandKind, HitBand, LABEL_COLUMNS, Lane, ModelCell, Timeline, hit_bands, ticks,
};

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

// ---------------------------------------------------------------------------
// 轴上的命中底色（`.scratch/trace-ledger/spec.md` §11、票 23）
// ---------------------------------------------------------------------------

/// 一段命中底色在哪几格 —— 断言里反复要的那对数。
fn cells_of(band: &HitBand) -> (u16, u16) {
    (band.first, band.last)
}

#[test]
fn a_cell_is_the_whole_span_divided_by_the_columns() {
    assert_eq!(
        one_second_a_cell().cell(),
        TimeDelta::seconds(1),
        "20 秒铺在 20 格上：一格一秒"
    );
    let coarse = Axis::new(at(0), at(200), 20).expect("轴");
    assert_eq!(
        coarse.cell(),
        TimeDelta::seconds(10),
        "会话越长一格越粗 —— 阈值跟的就是这个数，不是写死的秒数"
    );
}

#[test]
fn hits_less_than_a_cell_apart_merge_into_one_band() {
    // 一格一秒：3.0 s 与 3.4 s 之间那 0.4 格画不出分界，于是合成一段（票 23 第 3 条）。
    let bands = hit_bands(
        one_second_a_cell(),
        [(Lane::Model, at(3)), (Lane::Model, ms(3_400))],
    );
    assert_eq!(bands.len(), 1, "分界画不出来就不画：{bands:?}");
    assert_eq!(bands[0].lane, Lane::Model);
    assert_eq!(cells_of(&bands[0]), (3, 3), "两块落在同一格上");
}

#[test]
fn a_gap_of_one_cell_breaks_the_band_in_two() {
    // 正好隔一格（>= 一格）就断开成两段 —— 阈值那条线的另一半。
    let bands = hit_bands(
        one_second_a_cell(),
        [(Lane::Model, at(3)), (Lane::Model, at(4))],
    );
    assert_eq!(bands.len(), 2, "隔着一格就断开：{bands:?}");
    assert_eq!(cells_of(&bands[0]), (3, 3));
    assert_eq!(cells_of(&bands[1]), (4, 4));
}

#[test]
fn the_threshold_follows_the_resolution() {
    // 同一对时刻（相隔 2 秒）在两个分辨率上是两件事：一格一秒时它们之间看得见空档，
    // 一格十秒时看不见。阈值跟着一格走，不写死秒数（票 23 第 3 条）。
    let hits = [(Lane::Model, at(3)), (Lane::Model, at(5))];
    let fine = Axis::new(at(0), at(20), 20).expect("轴");
    assert_eq!(hit_bands(fine, hits).len(), 2, "一格一秒：看得见那条空档");
    let coarse = Axis::new(at(0), at(200), 20).expect("轴");
    assert_eq!(hit_bands(coarse, hits).len(), 1, "一格十秒：两块在同一段里");
}

#[test]
fn two_clusters_of_hits_leave_the_gap_between_them_blank() {
    let bands = hit_bands(
        one_second_a_cell(),
        [
            (Lane::Tool, at(1)),
            (Lane::Tool, ms(1_400)),
            (Lane::Tool, at(2)),
            // 空档：六格之外才有下一次命中。
            (Lane::Tool, at(8)),
            (Lane::Tool, ms(8_400)),
        ],
    );
    assert_eq!(bands.len(), 2, "两簇各一段：{bands:?}");
    assert_eq!(cells_of(&bands[0]), (1, 2), "第一簇从它第一块铺到最后一块");
    assert_eq!(cells_of(&bands[1]), (8, 8), "第二簇");
}

#[test]
fn a_lone_hit_keeps_the_cell_it_falls_in() {
    // 命中说的是「**这一刻**有命中」—— 与视口区间同一条位置口径，两端都算在内。孤零零的
    // 一块因此也留得下一格，否则「只搜到一个」在轴上永远看不见。
    let bands = hit_bands(one_second_a_cell(), [(Lane::Tool, at(7))]);
    assert_eq!(bands.len(), 1);
    assert_eq!(bands[0].lane, Lane::Tool);
    assert_eq!(cells_of(&bands[0]), (7, 7));
}

#[test]
fn a_session_of_continuous_hits_is_one_band() {
    // 几乎整场都是命中的会话里，底色连成一整段 —— 分辨率的诚实后果，不额外切碎
    // （票 23 验收那一条）。
    let hits: Vec<(Lane, DateTime<Utc>)> = (0..20)
        .map(|index| (Lane::Model, ms(index * 700)))
        .collect();
    let bands = hit_bands(one_second_a_cell(), hits);
    assert_eq!(bands.len(), 1, "一整场连成一段：{bands:?}");
    assert_eq!(cells_of(&bands[0]), (0, 13), "从第一块铺到最后一块");
}

#[test]
fn the_two_lanes_never_merge_with_each_other() {
    // 同一段时刻里模型与工具各有命中：那是**两条泳道上的两段**，不是一段 —— 底色铺在
    // 不同的行上（票 23 第 1 条）。
    let bands = hit_bands(
        one_second_a_cell(),
        [
            (Lane::Model, at(2)),
            (Lane::Tool, ms(2_200)),
            (Lane::Model, ms(2_400)),
        ],
    );
    assert_eq!(bands.len(), 2, "两条泳道各一段：{bands:?}");
    let model = bands
        .iter()
        .find(|band| band.lane == Lane::Model)
        .expect("模型那一段");
    let tool = bands
        .iter()
        .find(|band| band.lane == Lane::Tool)
        .expect("工具那一段");
    assert_eq!(cells_of(model), (2, 2));
    assert_eq!(cells_of(tool), (2, 2), "同一格上，不同的行");
}

#[test]
fn nothing_hit_is_no_band_at_all() {
    assert!(
        hit_bands(one_second_a_cell(), []).is_empty(),
        "没有命中就没有底色可铺"
    );
}
