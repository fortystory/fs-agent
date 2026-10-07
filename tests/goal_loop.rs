//! 目标循环：归属、`/loop` 的启动边界与从流派生的进度
//! （`.scratch/goal-loop/spec.md` §3、§4）。
//!
//! 这里断言的两种东西都在**事件流**与**纯函数**上：归属是流上的一条只追加事件，进度是它的
//! 一次派生 —— 没有第二处存储要照看。

mod support;

use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use heng::config::SessionConfig;
use heng::events::{
    ContextSource, Event, EventPayload, Redactor, SessionId, SpeakerId, current_goal, read_events,
};
use heng::events::{GoalStopReason, StopReason};
use heng::goals::{
    self, Manifest, NoProgress, Progress, Retry, StartRefusal, ThresholdStep, TodoCall,
    check_start, progress, threshold_step, unfinished,
};
use heng::permissions::{Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{PathLocks, builtin, todo};
use heng::{AssemblyParts, Harness, SessionScaffold, assemble};
use support::{CaptureBuf, FakeProvider, Reply};

// --- 一场会话 --------------------------------------------------------------

/// 一场真会话，外加它那份 JSONL 的路径 —— `sessions replay` 读的就是它。
struct Session {
    harness: Harness,
    log_path: PathBuf,
}

/// 一个模型照脚本行事的会话，挂在 `root` 下的一个会话目录里。
async fn session(root: &Path, id: &str, replies: Vec<Reply>) -> Session {
    session_with(root, id, FakeProvider::new(replies)).await
}

/// 同上，但调用方自己拿着那个假 provider —— 要读它收到的请求时用它。
async fn session_with(root: &Path, id: &str, provider: FakeProvider) -> Session {
    session_with_config(root, id, provider, SessionConfig::new("fake-model")).await
}

/// 同上，但会话的那些值也由调用方给 —— 试额度时用它。
async fn session_with_config(
    root: &Path,
    id: &str,
    provider: FakeProvider,
    config: SessionConfig,
) -> Session {
    let session_dir = root.join(id);
    let workspace = root.join("workspace");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session_dir.join("log.jsonl");
    let harness = assemble(AssemblyParts {
        provider: Box::new(provider),
        speaker: SpeakerId::Debater("kimi".into()),
        config,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace,
            log_path: log_path.clone(),
            session_id: SessionId::new(id),
            tools: builtin(false),
            locks: PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();
    Session { harness, log_path }
}

impl Session {
    /// 这条流，按磁盘上写下的样子 —— 也就是重放读到的那些。
    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }
}

/// 一次 `todo` 调用的脚本：它带 `id` 引用清单条目。
fn todo_reply(id: &str, args: &serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: todo::TODO_TOOL.into(),
            arguments: args.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

/// 一次带用量的文本回答：额度那道闸门数的就是这个数。
fn usage_reply(tokens: u64) -> Reply {
    Reply::Stream(vec![
        StreamEvent::TextDelta("好".to_owned()),
        StreamEvent::Usage(heng::events::Usage {
            input_tokens: tokens,
            output_tokens: 0,
            cached_tokens: 0,
            miss_tokens: 0,
            reasoning_tokens: None,
        }),
        StreamEvent::Finished {
            finish_reason: FinishReason::Stop,
        },
    ])
}

fn items(items: &[(&str, &str, &str)]) -> serde_json::Value {
    serde_json::json!({
        "items": items
            .iter()
            .map(|(id, content, status)| {
                serde_json::json!({ "id": id, "content": content, "status": status })
            })
            .collect::<Vec<_>>()
    })
}

fn goals_in(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::GoalSelected { goal } => Some(goal.clone()),
            _ => None,
        })
        .collect()
}

// --- `/loop` 的启动边界（§4） ----------------------------------------------

#[test]
fn the_three_start_refusals_each_say_their_own_thing() {
    let manifest = Manifest {
        name: "sandbox".to_owned(),
        entries: vec![goals::Entry {
            id: "01".to_owned(),
            content: "一件事".to_owned(),
        }],
    };

    // 清单不在：说找不到，并提示先 `/goal new`。
    assert_eq!(
        check_start(None, false, false, true),
        Err(StartRefusal::Unknown)
    );
    let text = heng::render::wording::loop_unknown_goal("sandbox");
    assert!(text.contains("sandbox"), "{text}");
    assert!(text.contains("/goal-new"), "提示先建清单：{text}");

    // 全部完成：说没活可干。
    assert_eq!(
        check_start(Some(&manifest), true, false, true),
        Err(StartRefusal::NoWork)
    );
    assert!(heng::render::wording::loop_no_work("sandbox").contains("没活可干"));

    // 已经有一个 loop 在跑：说正在跑 —— 而且它最先判，与另一个名字好不好无关。
    assert_eq!(
        check_start(Some(&manifest), false, true, true),
        Err(StartRefusal::AlreadyRunning)
    );
    assert_eq!(
        check_start(None, true, true, false),
        Err(StartRefusal::AlreadyRunning)
    );
    assert!(heng::render::wording::loop_already_running("sandbox").contains("正在跑"));

    // 档位不够（§5）：排在清单那两条之前 —— 跑都跑不起来时，名字对不对是下一步的事。
    assert_eq!(
        check_start(None, false, false, false),
        Err(StartRefusal::Unattended)
    );
    assert_eq!(
        check_start(Some(&manifest), true, false, false),
        Err(StartRefusal::Unattended)
    );
    let text = heng::render::wording::loop_needs_unattended_mode(
        heng::render::wording::mode_label(heng::permissions::Mode::Ask),
    );
    assert!(text.contains("无人值守"), "{text}");
    assert!(text.contains("workspace"), "要说清换哪一档：{text}");

    // 四条都过了才放行。
    assert_eq!(check_start(Some(&manifest), false, false, true), Ok(()));
}

#[test]
fn only_the_two_upper_permission_modes_can_run_unattended() {
    use heng::permissions::Mode;

    // 判据不是「哪一档更宽松」，而是「第一次写会不会停在等人」（§5）。
    assert!(!Mode::Readonly.allows_unattended());
    assert!(!Mode::Ask.allows_unattended());
    assert!(Mode::Workspace.allows_unattended());
    assert!(Mode::Auto.allows_unattended());
}

// --- 归属是一条只追加的事件（§4） ------------------------------------------

#[tokio::test]
async fn selecting_a_goal_appends_one_event_and_the_stream_says_which_goal_is_current() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session(dir.path(), "s-a", Vec::new()).await;

    session.harness.select_goal("foo").unwrap();
    let events = session.events();
    assert_eq!(goals_in(&events), ["foo"]);
    assert_eq!(current_goal(&events), Some("foo"));

    // 切换就是再记一条：不是一个字段被改写，所以两条都在流上。
    session.harness.select_goal("bar").unwrap();
    let events = session.events();
    assert_eq!(goals_in(&events), ["foo", "bar"]);
    assert_eq!(
        current_goal(&events),
        Some("bar"),
        "当前目标 = 流上最后一条，所以重放算出来的就是它"
    );

    session.harness.shutdown().await;
}

#[tokio::test]
async fn a_goal_is_visible_to_a_replay_of_the_written_stream_without_any_other_store() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session(dir.path(), "s-b", Vec::new()).await;
    session.harness.select_goal("sandbox").unwrap();
    session.harness.shutdown().await;

    // 重新从磁盘读一遍：`sessions replay` 与 `--continue` 走的就是这条路。
    let replayed = read_events(&session.log_path).unwrap();
    assert_eq!(current_goal(&replayed), Some("sandbox"));
    assert!(goals::has_goal(&replayed, "sandbox"));
    assert!(!goals::has_goal(&replayed, "other"));
}

#[test]
fn a_goal_selected_event_has_a_stable_kind_and_its_name_is_an_identifier_not_prose() {
    let payload = EventPayload::GoalSelected {
        goal: "sandbox".to_owned(),
    };
    assert_eq!(payload.kind(), "GoalSelected");

    // 名字是键、不是散文：打码不碰它（打码走的是 `redact` 的穷尽匹配）。
    let redactor = Redactor::new(["super-secret-key-value".to_owned()]);
    let mut payload = EventPayload::GoalSelected {
        goal: "super-secret-key-value".to_owned(),
    };
    payload.redact(&redactor);
    match payload {
        EventPayload::GoalSelected { goal } => assert_eq!(goal, "super-secret-key-value"),
        other => panic!("期望还是那条归属，得到 {other:?}"),
    }
}

// --- 进度：从各会话的 `todo` 调用派生（§3） --------------------------------

fn entries(ids: &[&str]) -> Vec<goals::Entry> {
    ids.iter()
        .map(|id| goals::Entry {
            id: (*id).to_owned(),
            content: format!("条目 {id}"),
        })
        .collect()
}

/// 一份清单：名字加几条条目。
fn manifest(name: &str, ids: &[&str]) -> Manifest {
    Manifest {
        name: name.to_owned(),
        entries: entries(ids),
    }
}

fn at(hour: i64) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 1, hour as u32, 0, 0)
        .unwrap()
}

fn call(session_at: chrono::DateTime<Utc>, seq: u64, args: serde_json::Value) -> TodoCall {
    TodoCall {
        session_at,
        seq,
        items: todo::read_items(&args),
    }
}

#[test]
fn a_goal_carries_completion_across_sessions() {
    // 会话 1 把 `03` 标成完成；会话 2 的列表里根本没有它 —— 它仍然是完成的。这正是「一个
    // 目标跨几个会话」要保住的信息。
    let manifest = entries(&["01", "02", "03"]);
    let calls = [
        call(
            at(10),
            1,
            items(&[("01", "一", "completed"), ("03", "三", "completed")]),
        ),
        call(at(11), 1, items(&[("02", "二", "in_progress")])),
    ];
    let derived = progress(&manifest, &calls);

    assert_eq!(derived.total(), 3);
    assert_eq!(derived.completed(), 2);
    assert!(!derived.is_complete());
    assert!(derived.unknown.is_empty());

    // 会话 2 再把 `02` 补完，目标就完成了。
    let calls = [
        calls[0].clone(),
        call(at(11), 1, items(&[("02", "二", "completed")])),
    ];
    let derived = progress(&manifest, &calls);
    assert_eq!(derived.completed(), 3);
    assert!(derived.is_complete(), "所有条目 completed 就是完成");
}

#[test]
fn the_latest_call_wins_and_an_entry_nobody_mentioned_stays_pending() {
    let manifest = entries(&["01", "02"]);

    // 同一条目在后续调用里被改回 `pending`：以最新的为准。
    let calls = [
        call(at(10), 1, items(&[("01", "一", "completed")])),
        call(at(10), 2, items(&[("01", "一", "pending")])),
    ];
    let derived = progress(&manifest, &calls);
    assert_eq!(derived.statuses["01"], todo::Status::Pending);
    assert_eq!(
        derived.statuses["02"],
        todo::Status::Pending,
        "一条都没提过的条目是 pending（清单是封闭的，所以它始终存在）"
    );
}

#[test]
fn the_merge_order_is_session_time_first_then_the_seq_inside_it() {
    // 更晚的会话赢，哪怕它的 seq 更小：排序键是（会话时间，seq）。
    let manifest = entries(&["01"]);
    let calls = [
        call(at(12), 1, items(&[("01", "一", "pending")])),
        call(at(10), 9, items(&[("01", "一", "completed")])),
    ];
    assert_eq!(
        progress(&manifest, &calls).statuses["01"],
        todo::Status::Pending
    );

    // 同一场会话里按 seq（大的后落地）：与它们在调用方那边的顺序无关。
    let calls = [
        call(at(10), 9, items(&[("01", "一", "completed")])),
        call(at(10), 1, items(&[("01", "一", "pending")])),
    ];
    assert_eq!(
        progress(&manifest, &calls).statuses["01"],
        todo::Status::Completed,
        "会话 10 点这一场里，seq 9 是后落地的那份列表"
    );
    let calls = [
        call(at(10), 1, items(&[("01", "一", "completed")])),
        call(at(10), 9, items(&[("01", "一", "pending")])),
    ];
    assert_eq!(
        progress(&manifest, &calls).statuses["01"],
        todo::Status::Pending
    );

    // 时间打平时（同一秒铸出的两场会话）仍然有确定的先后。
    let tie = [call(at(10), 1, items(&[("01", "一", "completed")]))];
    assert!(progress(&manifest, &tie).is_complete());
}

#[test]
fn an_id_that_is_not_on_the_manifest_is_ignored_but_never_silently() {
    // 沉默会让模型以为它记下了，所以越界的 id 被收下来，留给转录说一句。
    let manifest = entries(&["01"]);
    let calls = [call(
        at(10),
        1,
        items(&[
            ("01", "一", "completed"),
            ("07", "清单外的一件事", "completed"),
        ]),
    )];
    let derived = progress(&manifest, &calls);

    assert_eq!(derived.total(), 1);
    assert_eq!(derived.unknown, ["07"]);
    assert_eq!(derived.completed(), 1, "越界的 id 不改变任何条目的状态");
    assert!(derived.is_complete());

    // 那一行说给模型听，点名是哪些 id，也点名清单是封闭的、新工作该走哪儿。
    let line = heng::render::wording::goal_unknown_ids("sandbox", &derived.unknown);
    assert!(line.contains("07"), "{line}");
    assert!(line.contains("goal_note"), "{line}");
}

#[test]
fn a_call_without_ids_contributes_nothing_but_is_not_an_error() {
    // 老调用（没有 id）照常解析，只是对目标进度没有贡献。
    let manifest = entries(&["01"]);
    let calls = [call(
        at(10),
        1,
        serde_json::json!({ "items": [{ "content": "没有 id 的一项", "status": "completed" }] }),
    )];
    let derived = progress(&manifest, &calls);
    assert_eq!(derived.completed(), 0);
    assert!(!derived.is_complete());
    assert!(derived.unknown.is_empty());
}

#[tokio::test]
async fn two_sessions_on_one_goal_recompute_the_second_ones_progress_from_the_first() {
    // 会话 A 做完一条、落流；会话 B 接着干。B 的进度是**两条流合起来**算出来的。
    let dir = tempfile::tempdir().unwrap();
    let manifest = entries(&["01", "02"]);

    let mut first = session(
        dir.path(),
        "s-1",
        vec![
            todo_reply("call-1", &items(&[("01", "一", "completed")])),
            Reply::text("记下了"),
        ],
    )
    .await;
    first.harness.select_goal("sandbox").unwrap();
    first
        .harness
        .inject_context(ContextSource::Goal, "# sandbox\n")
        .unwrap();
    first.harness.run_injected_turn().await.unwrap();
    let first_events = first.events();
    first.harness.shutdown().await;

    let mut second = session(dir.path(), "s-2", Vec::new()).await;
    second.harness.select_goal("sandbox").unwrap();
    let mut calls = goals::todo_calls(&first_events);
    calls.extend(goals::todo_calls(&second.events()));
    let derived = progress(&manifest, &calls);

    assert_eq!(
        derived.statuses["01"],
        todo::Status::Completed,
        "会话 2 一句话没说，会话 1 的完成也还在"
    );
    assert_eq!(derived.statuses["02"], todo::Status::Pending);
    assert_eq!(derived.completed(), 1);

    second.harness.shutdown().await;
}

// --- 完成判据与汇总（§1、§11） ---------------------------------------------

/// 一条 `todo` 把清单上的条目**全部**标成完成。
fn complete_items(ids: &[&str]) -> serde_json::Value {
    let all: Vec<(&str, &str, &str)> = ids.iter().map(|id| (*id, "一件事", "completed")).collect();
    items(&all)
}

fn completions(events: &[Event]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::GoalCompleted { goal, summary } => Some((goal.clone(), summary.clone())),
            _ => None,
        })
        .collect()
}

fn usage_count(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::UsageRecorded { .. }))
        .count()
}

#[tokio::test]
async fn finishing_a_goal_records_one_completion_with_the_summary_the_model_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = manifest("sandbox", &["01", "02"]);
    let provider = FakeProvider::new(vec![
        todo_reply("call-1", &complete_items(&["01", "02"])),
        Reply::text("记下了"),
        Reply::text("目标做完了：两条都完成，只跨了一个会话。新工作只有一条，我记在 else 里。"),
        Reply::text("还在"),
    ]);
    let mut session = session_with(dir.path(), "s-1", provider.clone()).await;

    session.harness.select_goal("sandbox").unwrap();
    session
        .harness
        .inject_context(ContextSource::Goal, "# sandbox\n")
        .unwrap();
    session.harness.run_injected_turn().await.unwrap();

    // 最后一条 `todo` 把全部条目标成完成 → 判据机械地成立。
    let events = session.events();
    let derived = progress(&manifest.entries, &goals::todo_calls(&events));
    assert!(derived.is_complete());

    let before = usage_count(&events);
    let summary = session
        .harness
        .finish_goal("sandbox", &manifest, &derived, 1, &[])
        .await
        .unwrap();
    assert_eq!(
        summary, "目标做完了：两条都完成，只跨了一个会话。新工作只有一条，我记在 else 里。",
        "落流的是那次模型调用写出来的那段叙述"
    );

    let events = session.events();
    assert_eq!(
        completions(&events),
        [("sandbox".to_owned(), summary.clone())],
        "流上恰好一条完成，带着目标名与那段汇总"
    );
    assert_eq!(
        usage_count(&events),
        before + 1,
        "写汇总的那次调用照记用量 —— 它是一次 provider 调用"
    );

    // 汇总那次调用的简报里带着真实的条数与会话数。
    let requests = provider.requests();
    let prompt = last_prompt(&requests);
    assert!(prompt.contains("2/2 条完成"), "{prompt}");
    assert!(prompt.contains("跨会话：1 个会话"), "{prompt}");
    assert!(prompt.contains("01"), "清单条目在简报里：{prompt}");

    // 完成后不退出：同一个 harness 还能接着跑一个回合。
    let outcome = session.harness.run_turn("还在吗").await.unwrap();
    assert_eq!(outcome.text, "还在");

    session.harness.shutdown().await;
}

#[tokio::test]
async fn a_failed_summary_call_still_records_the_completion_with_a_mechanical_note() {
    // 判据是机械的，所以「做完了」不依赖模型能不能开口；缺的只是那段叙述。
    let dir = tempfile::tempdir().unwrap();
    let manifest = manifest("sandbox", &["01"]);
    let provider = FakeProvider::new(vec![
        todo_reply("call-1", &complete_items(&["01"])),
        Reply::text("记下了"),
        Reply::Fail(heng::provider::ProviderError::Transport {
            detail: "断了".to_owned(),
        }),
    ]);
    let mut session = session_with(dir.path(), "s-1", provider).await;
    session.harness.select_goal("sandbox").unwrap();
    session
        .harness
        .inject_context(ContextSource::Goal, "# sandbox\n")
        .unwrap();
    session.harness.run_injected_turn().await.unwrap();

    let events = session.events();
    let derived = progress(&manifest.entries, &goals::todo_calls(&events));
    let summary = session
        .harness
        .finish_goal(
            "sandbox",
            &manifest,
            &derived,
            2,
            &["顺手补了个测试".to_owned()],
        )
        .await
        .unwrap();

    assert!(summary.contains("1 / 1 条完成"), "{summary}");
    assert!(summary.contains("跨 2 个会话"), "{summary}");
    assert!(
        summary.contains("顺手补了个测试"),
        "新工作不能省：{summary}"
    );

    let events = session.events();
    assert_eq!(completions(&events).len(), 1, "汇总没写出来也照样完成");
    assert_eq!(completions(&events)[0].1, summary);

    session.harness.shutdown().await;
}

#[test]
fn the_summary_prompt_carries_all_four_things_and_never_omits_the_new_work() {
    let manifest = manifest("sandbox", &["01", "02"]);
    let calls = [call(at(10), 1, complete_items_call(&["01"]))];
    let derived = progress(&manifest.entries, &calls);
    let prompt = goals::summary_prompt("sandbox", &manifest, &derived, 2, &["新工作".to_owned()]);

    assert!(prompt.contains("sandbox"), "目标名：{prompt}");
    assert!(prompt.contains("1/2 条完成"), "条目完成情况：{prompt}");
    assert!(
        prompt.contains("跨会话：2 个会话"),
        "跨了几个会话：{prompt}"
    );
    assert!(prompt.contains("新工作"), "清单外的新工作：{prompt}");

    // 一条新工作都没有时也要说出来，而不是留白。
    let quiet = goals::summary_prompt("sandbox", &manifest, &derived, 2, &[]);
    assert!(quiet.contains("没有记下任何一条"), "{quiet}");
}

/// 一次 `todo` 调用（纯数据版，给不跑真会话的测试用）。
fn complete_items_call(ids: &[&str]) -> serde_json::Value {
    let all: Vec<(&str, &str, &str)> = ids.iter().map(|id| (*id, "一件事", "completed")).collect();
    items(&all)
}

/// 假 provider 收到的最后一个请求里的那条 `user` 消息。
fn last_prompt(requests: &[heng::provider::ChatRequest]) -> String {
    let request = requests.last().expect("汇总那次调用到达了 provider");
    request
        .messages
        .iter()
        .rev()
        .find_map(|message| match message {
            heng::provider::Message::User { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("一次单发调用带着一条 user 消息")
}

// --- 翻页与压缩（§7） -------------------------------------------------------

#[tokio::test]
async fn compaction_folds_the_history_into_a_summary_and_rollover_carries_it_over() {
    use heng::events::HistoryReason;
    use heng::session::SessionStore;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let provider = FakeProvider::new(vec![
        Reply::text("第一段回答，读过 src/goals.rs"),
        Reply::text("这段是摘要：做到 01，踩过一个坑。"),
    ]);
    let mut session = session_with(root, "s-1", provider.clone()).await;
    session.harness.select_goal("sandbox").unwrap();
    session
        .harness
        .inject_context(ContextSource::Goal, "# sandbox\n")
        .unwrap();
    session.harness.run_injected_turn().await.unwrap();

    let old_log = session.log_path.clone();
    let old_events = session.events();
    let before = usage_count(&old_events);
    assert_eq!(
        heng::events::current_goal(&old_events),
        Some("sandbox"),
        "旧会话先认领了目标"
    );

    // 新会话由 store 分配 —— 与 CLI 那条路完全一样。
    let store = SessionStore::new(root.join("store"));
    let stored = store.create(&root.join("workspace")).unwrap();
    let summary = session.harness.compact_and_rollover(&stored).await.unwrap();
    assert_eq!(
        summary.as_deref(),
        Some("这段是摘要：做到 01，踩过一个坑。")
    );

    // 旧会话：文件**仍在**磁盘上，流是完整的，而它的收尾是一条 `HistorySuperseded`。
    let old = read_events(&old_log).unwrap();
    assert_eq!(
        old.len(),
        old_events.len() + 3,
        "摘要那次调用（用量 + 消息）与那条取代事件都落了下来"
    );
    let superseded = old
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::HistorySuperseded {
                targets,
                reason,
                summary,
            } => Some((targets.clone(), *reason, summary.clone())),
            _ => None,
        })
        .expect("流上记着「这段历史不再权威」");
    assert_eq!(superseded.1, HistoryReason::Compaction);
    assert_eq!(
        superseded.2.as_deref(),
        Some("这段是摘要：做到 01，踩过一个坑。")
    );
    assert!(
        superseded.0.contains(&1),
        "targets 指向被摘要替代的那批事件：{:?}",
        superseded.0
    );
    assert_eq!(
        usage_count(&old),
        before + 1,
        "写摘要的那次调用照记用量 —— 它就是一次 provider 调用"
    );

    // 新会话：从空开始，开头就有那段摘要。
    let new = read_events(&stored.log_path).unwrap();
    assert!(matches!(
        new[0].payload,
        EventPayload::SessionStarted { .. }
    ));
    assert!(new.iter().any(|event| matches!(
        &event.payload,
        EventPayload::ContextInjected { source: ContextSource::Compaction, content }
            if content == "这段是摘要：做到 01，踩过一个坑。"
    )));

    // 摘要那次调用读的是**压缩前**那段历史。
    let prompt = last_prompt(&provider.requests());
    assert!(prompt.contains("第一段回答"), "{prompt}");

    // `sessions replay` 对新旧两个会话都跑得通：两条流都读得回来。
    assert!(!read_events(&old_log).unwrap().is_empty());
    assert!(!read_events(&stored.log_path).unwrap().is_empty());

    session.harness.shutdown().await;
}

#[tokio::test]
async fn a_turn_that_never_finishes_never_rolls_over() {
    use std::sync::Arc;
    use tokio::sync::Notify;

    // 翻页只能发生在回合边界：一个还没落地的回合不会自己触发它。这里用一条永不结束的流证明
    // 这一点 —— 取消把它收下来，而流上没有任何「历史被取代」。
    let dir = tempfile::tempdir().unwrap();
    let opened = Arc::new(Notify::new());
    let provider = FakeProvider::new(vec![Reply::Stall(
        Arc::clone(&opened),
        vec![StreamEvent::TextDelta("半句话".to_owned())],
    )]);
    let mut session = session_with(dir.path(), "s-1", provider).await;
    session.harness.select_goal("sandbox").unwrap();

    let signal = session.harness.cancel_signal();
    // 与循环驱动一个回合同样的形状：流一开出来就举起手势，然后等它落下来。
    {
        let mut turn = Box::pin(session.harness.run_injected_turn());
        tokio::select! {
            _ = opened.notified() => {
                signal.cancel();
                let _ = turn.await;
            }
            result = &mut turn => {
                let _ = result;
            }
        }
    }

    let events = session.events();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::HistorySuperseded { .. })),
        "翻页不会在回合中途自己发生"
    );
    assert_eq!(goals_in(&events), ["sandbox"], "会话也还在原地");

    session.harness.shutdown().await;
}

// --- 阈值与提醒（§6、§7） ---------------------------------------------------

#[test]
fn the_two_thresholds_are_lines_on_the_window() {
    // 边界值：过线才算，而正好落在线上也算（`>=`，与预算那道闸门同一条规矩）。
    for (percent, expected) in [
        (0, ThresholdStep::None),
        (49, ThresholdStep::None),
        (50, ThresholdStep::Remind),
        (51, ThresholdStep::Remind),
        (79, ThresholdStep::Remind),
        (80, ThresholdStep::Compact),
        (81, ThresholdStep::Compact),
        (100, ThresholdStep::Compact),
    ] {
        assert_eq!(
            threshold_step(percent, 50, 80, false),
            expected,
            "{percent}% 这一刻"
        );
    }
}

#[test]
fn the_reminder_lands_once_per_crossing_and_a_rollover_resets_it() {
    // 跨过阈值只注入一次：连续多个回合都在 50% 以上，流上也只有一条提醒。
    assert_eq!(threshold_step(60, 50, 80, true), ThresholdStep::None);
    // 而翻页那一档不受这个标记影响 —— 它是一次动作，不是一次提醒。
    assert_eq!(
        threshold_step(90, 50, 80, true),
        ThresholdStep::Compact,
        "翻页不因为提醒过就不发生"
    );
    // 翻页之后标记复位：新会话过线时还会再提醒一次。
    assert_eq!(threshold_step(60, 50, 80, false), ThresholdStep::Remind);
}

#[test]
fn the_reminder_is_chinese_model_text_that_says_what_to_save() {
    let text = heng::render::wording::goal_reminder();
    assert!(
        text.contains("落下来"),
        "措辞是「把还没落流的东西落下来」：{text}"
    );
    assert!(text.contains("摘要"), "它要说清再过一会儿就压缩了：{text}");
    assert!(
        text.contains("todo") || text.contains("结论"),
        "它要点名哪些东西会丢：{text}"
    );
    // 模型可见的散文走中文（ADR 0005）。
    assert!(
        text.chars()
            .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch)),
        "{text}"
    );
}

/// 一场上下文已经超过八成、但一个 token 都没花的会话：窗口小、预算无穷。
async fn nearly_full_session(root: &Path) -> Session {
    use heng::provider::capability::caps_for;

    let mut caps = caps_for("deepseek-flash").unwrap();
    caps.context_window = 20_000;
    caps.max_output_tokens = 1_000;
    let provider = FakeProvider::with_caps(
        vec![Reply::text("干了一点活"), Reply::text("这段是摘要")],
        caps,
    );
    let mut session = session_with(root, "s-1", provider).await;
    session.harness.select_goal("sandbox").unwrap();
    // 一条足够长的注入把估算推过八成（19_000 的窗口，八成是 15_200 token ≈ 61k 字符）。
    let filler = "长".repeat(70_000);
    session
        .harness
        .inject_context(ContextSource::Goal, &filler)
        .unwrap();
    session.harness.run_injected_turn().await.unwrap();
    session
}

#[tokio::test]
async fn the_judgement_reads_the_window_and_not_the_budget() {
    let dir = tempfile::tempdir().unwrap();
    let session = nearly_full_session(dir.path()).await;

    let percent = session.harness.context_percent();
    assert!(percent >= 80, "窗口已经过八成：{percent}%");
    assert_eq!(
        threshold_step(percent, 50, 80, false),
        ThresholdStep::Compact
    );

    // 判据与预算无关：这场会话一个 token 都还没花（两者混起来是本文件要防的那个错）。
    let spent = heng::events::total_usage(&session.events()).total_tokens();
    assert_eq!(spent, 0, "百分比看的是窗口，不是累计 token");

    session.harness.shutdown().await;
}

#[tokio::test]
async fn crossing_the_compact_threshold_compacts_and_opens_a_new_session() {
    use heng::events::HistoryReason;
    use heng::session::SessionStore;

    let dir = tempfile::tempdir().unwrap();
    let session = nearly_full_session(dir.path()).await;
    let mut session = session;
    let old_log = session.log_path.clone();

    let stored = SessionStore::new(dir.path().join("store"))
        .create(&dir.path().join("workspace"))
        .unwrap();
    let summary = session.harness.compact_and_rollover(&stored).await.unwrap();
    assert_eq!(summary.as_deref(), Some("这段是摘要"));

    // 过八成那一下：旧流上有压缩，新会话里有那段摘要，而旧文件还在。
    let old = read_events(&old_log).unwrap();
    assert!(old.iter().any(|event| matches!(
        &event.payload,
        EventPayload::HistorySuperseded {
            reason: HistoryReason::Compaction,
            ..
        }
    )));
    let new = read_events(&stored.log_path).unwrap();
    assert!(new.iter().any(|event| matches!(
        &event.payload,
        EventPayload::ContextInjected {
            source: ContextSource::Compaction,
            ..
        }
    )));
    // 新会话的窗口用量回落到低位，所以标记复位之后还会再提醒一次。
    assert!(
        session.harness.context_percent() < 80,
        "翻页之后窗口回到低位"
    );

    session.harness.shutdown().await;
}

// --- `/clear`：同一个 rollover 的另一段入口（§12） ---------------------------

#[tokio::test]
async fn clear_is_the_same_rollover_without_a_summary_and_leaves_no_trace_on_the_stream() {
    use heng::session::SessionStore;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut session = session_with(
        root,
        "s-1",
        FakeProvider::new(vec![Reply::text("先聊两句")]),
    )
    .await;
    session.harness.run_turn("在吗").await.unwrap();

    let old_log = session.log_path.clone();
    let before = session.events();
    assert!(
        before
            .iter()
            .any(|event| matches!(event.payload, EventPayload::MessageCompleted { .. }))
    );

    // `/clear` 调的**就是**翻页那条机制，差别只有一处：不带压缩、不带摘要注入。
    let store = SessionStore::new(root.join("store"));
    let stored = store.create(&root.join("workspace")).unwrap();
    session.harness.rollover(&stored).unwrap();

    // 手势本身不进事件流：旧会话的流**一字未动** —— 没有收尾事件、什么都没有。
    assert_eq!(read_events(&old_log).unwrap(), before);

    // 新会话从空开始，开头是它自己的 `SessionStarted`：审计看的就是这个边界。
    let new = read_events(&stored.log_path).unwrap();
    assert!(matches!(
        new[0].payload,
        EventPayload::SessionStarted { .. }
    ));
    assert!(
        !new.iter().any(|event| matches!(
            event.payload,
            EventPayload::HistorySuperseded { .. } | EventPayload::ContextInjected { .. }
        )),
        "没有压缩、没有摘要注入：{new:?}"
    );
    assert_eq!(
        session.harness.session_id().as_str(),
        stored.id.as_str(),
        "当前会话已经换了"
    );

    // `--continue` 打开的是**最新**的那一个 —— 也就是刚开的这场。
    let latest = store.latest(&root.join("workspace")).unwrap().unwrap();
    assert_eq!(latest.id, stored.id);

    session.harness.shutdown().await;
}

// --- 无进展与停止（§9） -----------------------------------------------------

#[test]
fn the_no_progress_counter_counts_rollovers_and_resets_on_any_completion() {
    // 计数对象是**翻页**，不是回合；连续 `N` 次零完成才算卡住。
    let mut counter = NoProgress::new(0);
    assert_eq!(counter.after_rollover(0), 1);
    assert!(!counter.reached(3), "N-1 次不停");
    assert_eq!(counter.after_rollover(0), 2);
    assert!(!counter.reached(3), "还是 N-1 次，不停");
    assert_eq!(counter.after_rollover(0), 3);
    assert!(counter.reached(3), "N 次停");

    // 中途完成任意一条就归零，重新计。
    let mut counter = NoProgress::new(0);
    counter.after_rollover(0);
    counter.after_rollover(0);
    assert_eq!(counter.after_rollover(1), 0, "有进展就归零");
    assert!(!counter.reached(3));
    assert_eq!(counter.after_rollover(2), 0, "又有进展，还是零");
    assert_eq!(counter.after_rollover(2), 1, "这一次没有新条目完成，从头攒");
}

#[test]
fn the_retry_budget_is_spent_by_failures_and_refilled_by_a_success() {
    let mut retry = Retry::new(2);
    assert!(retry.another_attempt(), "第一次失败之后还能再试");
    assert!(retry.another_attempt(), "第二次失败之后还能再试");
    assert!(!retry.another_attempt(), "第三次：预算耗尽，停下");
    assert_eq!(retry.failures(), 3, "报告里要数得出试了几次");

    // 中途成功一次，计数归零。
    let mut retry = Retry::new(2);
    retry.another_attempt();
    retry.another_attempt();
    retry.succeeded();
    assert_eq!(retry.failures(), 0);
    assert!(retry.another_attempt());

    // 0 次就是不重试。
    let mut none = Retry::new(0);
    assert!(!none.another_attempt());
}

#[test]
fn the_unfinished_entries_are_the_ones_the_report_names() {
    let entries = entries(&["01", "02", "03"]);
    let calls = [call(
        at(10),
        1,
        items(&[("01", "一", "completed"), ("02", "二", "in_progress")]),
    )];
    let derived = progress(&entries, &calls);
    assert_eq!(unfinished(&entries, &derived), ["02", "03"]);
}

#[tokio::test]
async fn stopping_a_goal_records_one_event_with_the_count_and_the_stuck_entries() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session_with(dir.path(), "s-1", FakeProvider::new(Vec::new())).await;
    session.harness.select_goal("sandbox").unwrap();

    session
        .harness
        .stop_goal(
            "sandbox",
            GoalStopReason::NoProgress,
            "连续 3 次翻页没有任何条目完成",
            vec!["02".to_owned(), "03".to_owned()],
            3,
        )
        .unwrap();

    let events = session.events();
    let stopped: Vec<(&str, u32, Vec<String>)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::GoalStopped {
                reason,
                detail,
                stuck,
                count,
                ..
            } => {
                assert_eq!(*reason, GoalStopReason::NoProgress);
                assert!(detail.contains("连续 3 次"));
                Some((reason.as_str(), *count, stuck.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(stopped.len(), 1, "恰好一条收尾事件");
    assert_eq!(stopped[0].0, "no_progress", "reason 是稳定短名");
    assert_eq!(stopped[0].1, 3);
    assert_eq!(stopped[0].2, ["02", "03"], "卡住的条目随事件一起落");
    // 它与 `GoalCompleted` 是并列的两条：这一条流上没有「做完了」。
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::GoalCompleted { .. }))
    );

    // 说给人听的那一段点名了卡住的条目。
    let line = heng::render::wording::goal_stopped(
        "sandbox",
        GoalStopReason::NoProgress,
        3,
        &["02".to_owned(), "03".to_owned()],
    );
    assert!(line.contains("02"), "{line}");
    assert!(line.contains("连续 3 次"), "{line}");

    // 报告里那几句是散文、要打码；`goal`、条目 id 与计数都是键，不动。
    let redactor = Redactor::new(["super-secret-key-value".to_owned()]);
    let mut payload = EventPayload::GoalStopped {
        goal: "super-secret-key-value".to_owned(),
        reason: GoalStopReason::ProviderFailed,
        detail: "试了 super-secret-key-value 次".to_owned(),
        stuck: vec!["02".to_owned()],
        count: 3,
    };
    payload.redact(&redactor);
    match payload {
        EventPayload::GoalStopped {
            goal,
            detail,
            stuck,
            count,
            ..
        } => {
            assert_eq!(goal, "super-secret-key-value");
            assert_eq!(detail, "试了 [redacted] 次");
            assert_eq!(stuck, ["02"]);
            assert_eq!(count, 3);
        }
        other => panic!("期望还是那条收尾，得到 {other:?}"),
    }

    session.harness.shutdown().await;
}

#[test]
fn the_stop_reasons_are_stable_protocol_marks() {
    // 恢复（§10）靠它们分清「正常收尾」与「异常中断」，所以它们是协议标记：换名字就是换 schema。
    assert_eq!(GoalStopReason::NoProgress.as_str(), "no_progress");
    assert_eq!(GoalStopReason::ProviderFailed.as_str(), "provider_failed");
    assert_eq!(GoalStopReason::UserStopped.as_str(), "user_stopped");
    assert_eq!(
        EventPayload::GoalStopped {
            goal: "g".to_owned(),
            reason: GoalStopReason::NoProgress,
            detail: String::new(),
            stuck: Vec::new(),
            count: 0,
        }
        .kind(),
        "GoalStopped"
    );
}

// --- 预算认到目标上（§8） ---------------------------------------------------

#[tokio::test]
async fn a_carried_usage_counts_toward_the_goal_budget_and_a_rollover_does_not_reset_it() {
    use heng::session::SessionStore;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // 额度取大一点：预检那一半拿估计去比剩余量，而一份 prompt 本身就有几百个 token。
    let budget = || SessionConfig::new("fake-model").with_session_token_limit(100_000);

    // 会话 A：用掉 60。
    let mut session = session_with_config(
        root,
        "s-1",
        FakeProvider::new(vec![usage_reply(60_000), usage_reply(50_000)]),
        budget(),
    )
    .await;
    session.harness.select_goal("sandbox").unwrap();
    session.harness.run_turn("开工").await.unwrap();
    let spent = heng::events::total_usage(&session.events()).total_tokens();
    assert_eq!(spent, 60_000);

    // 翻页：新会话带上到现在为止的累计 —— 循环做的就是这一件事。
    let stored = SessionStore::new(root.join("store"))
        .create(&root.join("workspace"))
        .unwrap();
    session.harness.rollover(&stored).unwrap();
    session.harness.carry_usage(spent);

    // 会话 B：自己的流里只有 50k，而闸门看到的是 60k + 50k = 110k > 100k —— 下一次调用不再打开，
    // 回合以 `BudgetExhausted` 降级收尾。
    let outcome = session.harness.run_turn("继续").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    let outcome = session.harness.run_turn("再来一个").await.unwrap();
    assert_eq!(
        outcome.reason,
        StopReason::BudgetExhausted,
        "翻页没有重置额度：闸门在跨会话的累计上判"
    );

    session.harness.shutdown().await;
}

#[tokio::test]
async fn without_a_goal_the_budget_is_the_plain_session_one() {
    // 没有目标归属时没有东西要累计（`carried_tokens` 是 0），行为与从前完全一样。
    let dir = tempfile::tempdir().unwrap();
    let mut session = session_with_config(
        dir.path(),
        "s-1",
        FakeProvider::new(vec![usage_reply(60_000), usage_reply(30_000)]),
        SessionConfig::new("fake-model").with_session_token_limit(100_000),
    )
    .await;

    // 60k 花掉了，但 60k < 100k，所以第二回合照常打开调用。
    session.harness.run_turn("第一回合").await.unwrap();
    let outcome = session.harness.run_turn("第二回合").await.unwrap();
    assert_eq!(
        outcome.reason,
        StopReason::Completed,
        "会话自己的 90k 还没撞顶"
    );

    let spent = heng::events::total_usage(&session.events()).total_tokens();
    assert_eq!(spent, 90_000);

    session.harness.shutdown().await;
}

#[test]
fn the_goal_budget_counts_the_other_sessions_and_not_the_current_one_twice() {
    use heng::events::Usage;

    let used = |seq: u64, tokens: u64| {
        Event::new(
            seq,
            SpeakerId::System,
            EventPayload::UsageRecorded {
                usage: Usage {
                    input_tokens: tokens,
                    output_tokens: 0,
                    cached_tokens: 0,
                    miss_tokens: 0,
                    reasoning_tokens: None,
                },
            },
        )
    };
    let sessions = [
        ("s-1".to_owned(), vec![used(1, 60_000)]),
        ("s-2".to_owned(), vec![used(1, 40_000)]),
    ];

    // 当前会话自己那份不算在「别处」里 —— 它就在自己那条流上，闸门已经数过一遍了。
    assert_eq!(goals::usage_apart_from(&sessions, "s-2"), 60_000);
    assert_eq!(goals::usage_apart_from(&sessions, "s-9"), 100_000);
    assert_eq!(goals::usage_apart_from(&[], "s-1"), 0);
}

#[tokio::test]
async fn a_rollover_does_not_carry_the_goal_budget_into_an_untargeted_session() {
    use heng::session::SessionStore;

    // `/clear` 走的就是这一条路：清场之后的新会话**没有**当前目标，所以它的额度必须退回会话级
    // （§8、§12）。带过去的 carried 会把上一个会话的花费算进一个与它无关的会话。
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut session = session_with_config(
        root,
        "s-1",
        FakeProvider::new(vec![
            usage_reply(60_000),
            usage_reply(50_000),
            usage_reply(0),
        ]),
        SessionConfig::new("fake-model").with_session_token_limit(100_000),
    )
    .await;
    session.harness.select_goal("sandbox").unwrap();
    session.harness.run_turn("开工").await.unwrap();
    let spent = heng::events::total_usage(&session.events()).total_tokens();
    session.harness.carry_usage(spent);

    let stored = SessionStore::new(root.join("store"))
        .create(&root.join("workspace"))
        .unwrap();
    session.harness.rollover(&stored).unwrap();

    // 新会话自己再花 50k。带着旧的 carried 时闸门看到的是 60k + 50k = 110k，第三个回合直接
    // 降级收尾；不带时是 50k，第三个回合照常打开调用。
    session.harness.run_turn("清场之后").await.unwrap();
    let outcome = session.harness.run_turn("再一个回合").await.unwrap();
    assert_eq!(
        outcome.reason,
        StopReason::Completed,
        "新会话的额度从零起算：上一个会话那 60k 不该跟过来"
    );

    session.harness.shutdown().await;
}

// --- 崩溃恢复与主动停（§10） ------------------------------------------------

/// 一场会话跑过的东西：一个归属、一个回合，以及（可选的）一条收尾。
async fn session_with_ending(root: &Path, id: &str, ending: Option<&str>) -> Session {
    let mut session = session_with(
        root,
        id,
        FakeProvider::new(vec![Reply::text("干了一点活"), Reply::text("汇总")]),
    )
    .await;
    session.harness.select_goal("sandbox").unwrap();
    session.harness.run_injected_turn().await.unwrap();
    match ending {
        Some("completed") => {
            session.harness.complete_goal("sandbox", "做完了").unwrap();
        }
        Some("stopped") => {
            session
                .harness
                .stop_goal(
                    "sandbox",
                    GoalStopReason::UserStopped,
                    "人按了停下",
                    Vec::new(),
                    0,
                )
                .unwrap();
        }
        _ => {}
    }
    session
}

#[tokio::test]
async fn an_interrupted_stream_resumes_the_goal_it_was_working_on() {
    let dir = tempfile::tempdir().unwrap();
    let session = session_with_ending(dir.path(), "s-1", None).await;

    // 归属 + 回合 + **没有**收尾事件 = 异常中断（进程被杀、机器重启）。
    assert_eq!(goals::ending(&session.events()), goals::Ending::Interrupted);
    assert_eq!(
        session.harness.resume_goal().as_deref(),
        Some("sandbox"),
        "接着做当前目标 —— 当前目标同样从流派生"
    );
    assert_eq!(session.harness.current_goal().as_deref(), Some("sandbox"));

    session.harness.shutdown().await;
}

#[tokio::test]
async fn a_completed_goal_comes_back_idle_and_does_not_resume_itself() {
    let dir = tempfile::tempdir().unwrap();
    let session = session_with_ending(dir.path(), "s-1", Some("completed")).await;

    assert_eq!(goals::ending(&session.events()), goals::Ending::Closed);
    assert_eq!(
        session.harness.resume_goal(),
        None,
        "正常收尾：回来是空闲等人"
    );
    assert!(
        session.harness.current_goal().is_some(),
        "归属还在，只是不再跑"
    );

    session.harness.shutdown().await;
}

#[tokio::test]
async fn a_goal_a_person_stopped_comes_back_idle_too() {
    let dir = tempfile::tempdir().unwrap();
    let session = session_with_ending(dir.path(), "s-1", Some("stopped")).await;

    // 主动停是人的意思，该尊重它：`--continue` 回来别自己又跑起来。
    assert_eq!(goals::ending(&session.events()), goals::Ending::Closed);
    assert_eq!(session.harness.resume_goal(), None);

    session.harness.shutdown().await;
}

#[tokio::test]
async fn a_plain_session_without_a_goal_is_left_exactly_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = session_with(
        dir.path(),
        "s-1",
        FakeProvider::new(vec![Reply::text("聊了两句")]),
    )
    .await;
    session.harness.run_turn("在吗").await.unwrap();

    // 没有归属：恢复这条路一个字都不说，`--continue` 照旧只铺历史。
    assert_eq!(session.harness.current_goal(), None);
    assert_eq!(session.harness.resume_goal(), None);

    session.harness.shutdown().await;
}

#[tokio::test]
async fn deciding_whether_to_resume_writes_no_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let session = session_with_ending(dir.path(), "s-1", None).await;

    // 判据完全从流派生：不写恢复标记，也没有第二个地方记着这件事。
    let before = std::fs::read_dir(dir.path().join("s-1"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    let _ = session.harness.resume_goal();
    let after = std::fs::read_dir(dir.path().join("s-1"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(before, after, "判定不写任何新文件");

    session.harness.shutdown().await;
}

#[test]
fn the_recovery_lines_are_chinese_and_say_which_of_the_two_it_is() {
    let resumed = heng::render::wording::resumed_goal("sandbox");
    assert!(resumed.contains("接着"), "{resumed}");
    assert!(resumed.contains("sandbox"), "{resumed}");

    let closed = heng::render::wording::resumed_closed_goal("sandbox");
    assert!(closed.contains("正常收尾"), "{closed}");
    assert!(closed.contains("sandbox"), "{closed}");
    assert_ne!(resumed, closed, "两条路各说各的话");
}

// --- 会话桶里的归属筛（§4） -------------------------------------------------

#[test]
fn only_the_sessions_that_claimed_the_goal_count_for_it() {
    let claimed = [
        heng::events::Event::new(
            1,
            SpeakerId::System,
            EventPayload::GoalSelected {
                goal: "sandbox".to_owned(),
            },
        ),
        heng::events::Event::new(
            2,
            SpeakerId::System,
            EventPayload::SessionEnded {
                reason: heng::events::StopReason::Completed,
            },
        ),
    ];
    let other = [heng::events::Event::new(
        1,
        SpeakerId::System,
        EventPayload::GoalSelected {
            goal: "grep-tool".to_owned(),
        },
    )];

    assert!(goals::has_goal(&claimed, "sandbox"));
    assert!(!goals::has_goal(&claimed, "grep-tool"));
    assert!(!goals::has_goal(&other, "sandbox"));
    assert_eq!(current_goal(&other), Some("grep-tool"));
    // 时间戳只用来排序，这里顺手确认派生对空调用是安全的。
    assert_eq!(
        progress(&entries(&["01"]), &[]),
        Progress {
            statuses: [("01".to_owned(), todo::Status::Pending)]
                .into_iter()
                .collect(),
            unknown: Vec::new(),
        }
    );
}
