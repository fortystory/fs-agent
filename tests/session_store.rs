//! 票 12：会话存储、`--continue` 与 `/undo`。
//!
//! 存储通过库那条接缝驱动，与 CLI 将来驱动它的方式一模一样：
//! `SessionStore::create` 为一个 cwd 分出一个会话目录，
//! `SessionStore::latest` 是 `--continue` 扫的东西，而组装点
//! 打开那个目录里不管哪条流。没有网络，没有环境。

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use heng::config::SessionConfig;
use heng::events::{
    pending_tool_calls, read_events, Event, EventLog, EventPayload, HistoryReason, ParticipantId,
    Role, SessionId, SpeakerId, StopReason, ToolCallId,
};
use heng::permissions::{Mode, Policy};
use heng::provider::{FinishReason, Message, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::session::{SessionStore, StoredSession};
use heng::tools::{self, PathLocks};
use heng::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

/// 一个工作区、一个存储根，以及那两个被捕获的 sink —— 一个会话
/// 除了 provider 之外需要的一切。
struct Env {
    store: SessionStore,
    cwd: PathBuf,
    stdout: CaptureBuf,
    stderr: CaptureBuf,
    _dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(dir.path().join("store"));
        // 两边都规范化过，所以不管这个临时目录
        // 怎么拼，分桶的键都是稳定的。
        let cwd = std::fs::canonicalize(dir.path()).unwrap().join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();
        Self {
            store,
            cwd,
            stdout: CaptureBuf::default(),
            stderr: CaptureBuf::default(),
            _dir: dir,
        }
    }

    /// 第二个工作区，给分桶那些测试用。
    fn other_cwd(&self) -> PathBuf {
        let path = self._dir.path().join("other");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::canonicalize(path).unwrap()
    }

    /// 通过那唯一一条组装接缝打开会话目录 `stored`。
    ///
    /// 全新与续接在这里不是一个旗标：这个目录要么有一条
    /// 流，要么没有，这正是存储做过的那个判定。
    async fn open(&self, stored: &StoredSession, provider: FakeProvider) -> Harness {
        assemble(AssemblyParts {
            provider: Box::new(provider),
            speaker: SpeakerId::Debater("kimi".into()),
            config: SessionConfig::new("fake-model"),
            renderer: Renderer::headless(RenderSinks {
                stdout_result: Box::new(self.stdout.clone()),
                stderr_diagnostic: Box::new(self.stderr.clone()),
            }),
            scaffold: SessionScaffold {
                cwd: self.cwd.clone(),
                log_path: stored.log_path.clone(),
                session_id: stored.id.clone(),
                tools: tools::builtin(false),
                locks: PathLocks::new(),
                policy: Policy::for_mode(Mode::Ask),
                asker: Some(Arc::new(AlwaysAllow)),
                questions: None,
                hook: None,
                home: None,
            },
        })
        .await
        .unwrap()
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.cwd.join(name);
        std::fs::write(&path, content).unwrap();
        std::fs::canonicalize(&path).unwrap()
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.cwd.join(name)).unwrap()
    }
}

/// 一条脚本化的工具调用，在一条流里完成。
fn tool_reply(id: &str, name: &str, arguments: &str) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallStarted {
            index: 0,
            id: id.to_owned(),
            name: name.to_owned(),
        },
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

/// 往一条已有的流上追加一条事件，就像一个在工具调用
/// 中途被杀掉的进程会留下的那样。
fn append(log_path: &Path, speaker: SpeakerId, payload: EventPayload) {
    let mut log = EventLog::open(log_path).unwrap();
    log.append(speaker, payload).unwrap();
    log.flush().unwrap();
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[tokio::test]
async fn a_created_session_is_a_private_directory_bound_to_its_cwd() {
    let env = Env::new();
    let stored = env.store.create(&env.cwd).unwrap();

    // 目录的形状是存储的事；流是组装点的事，
    // 而在一个会话存在之前，它并不存在。
    assert!(stored.dir.is_dir());
    assert_eq!(
        stored.dir.file_name().unwrap().to_string_lossy(),
        stored.id.as_str()
    );
    assert_eq!(stored.log_path, stored.dir.join("log.jsonl"));
    assert_eq!(stored.outputs_dir, stored.dir.join("outputs"));
    assert!(stored.outputs_dir.is_dir());
    assert!(!stored.log_path.exists());

    // id 形如 `<UTC 时间戳>-<短后缀>`。
    let (stamp, suffix) = stored.id.as_str().rsplit_once('-').unwrap();
    assert_eq!(stamp.len(), 16, "YYYYMMDDTHHMMSSZ：{stamp}");
    assert_eq!(suffix.len(), 8, "八个十六进制数字：{suffix}");
    assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()), "{suffix}");

    let harness = env.open(&stored, FakeProvider::new(vec![])).await;
    assert_eq!(harness.session_id(), &stored.id);
    harness.shutdown().await;

    assert!(stored.log_path.is_file());
    #[cfg(unix)]
    {
        assert_eq!(mode(&stored.dir), 0o700, "会话目录是私有的");
        assert_eq!(mode(&stored.outputs_dir), 0o700, "产物目录是私有的");
        assert_eq!(mode(&stored.log_path), 0o600, "事件流是私有的");
        assert_eq!(
            mode(&env.store.bucket(&env.cwd)),
            0o700,
            "整条存储路径都是私有的，不只是最末那一层"
        );
    }

    // 桶是按工作区分的，而权威的 cwd 在流里。
    let events = read_events(&stored.log_path).unwrap();
    let recorded_cwd = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::SessionStarted { cwd, .. } => Some(cwd.clone()),
            _ => None,
        })
        .expect("会话记下了它的 cwd");
    assert_eq!(recorded_cwd, env.cwd.to_string_lossy().into_owned());
}

#[tokio::test]
async fn ids_do_not_collide_and_latest_follows_the_most_recently_written_stream() {
    let env = Env::new();

    let first = env.store.create(&env.cwd).unwrap();
    env.open(&first, FakeProvider::new(vec![]))
        .await
        .shutdown()
        .await;
    std::thread::sleep(Duration::from_millis(10));

    let second = env.store.create(&env.cwd).unwrap();
    env.open(&second, FakeProvider::new(vec![]))
        .await
        .shutdown()
        .await;

    assert_ne!(first.id, second.id);
    assert_eq!(
        env.store.latest(&env.cwd).unwrap().unwrap().id,
        second.id,
        "更晚的那个会话才是 --continue 会续的"
    );

    // 说话的是活跃程度而不是创建顺序：往更旧的那条流上追加，
    // 又把它变回最新的了。
    std::thread::sleep(Duration::from_millis(10));
    append(
        &first.log_path,
        SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: "asked later".to_owned(),
            reasoning: None,
        },
    );
    assert_eq!(
        env.store.latest(&env.cwd).unwrap().unwrap().id,
        first.id,
        "mtime，而不是 id 顺序，才是 --continue 用的平手判定"
    );
}

#[tokio::test]
async fn a_different_workspace_is_a_different_bucket() {
    let env = Env::new();
    let other = env.other_cwd();

    let here = env.store.create(&env.cwd).unwrap();
    env.open(&here, FakeProvider::new(vec![]))
        .await
        .shutdown()
        .await;
    let there = env.store.create(&other).unwrap();
    env.open(&there, FakeProvider::new(vec![]))
        .await
        .shutdown()
        .await;

    assert_ne!(env.store.bucket(&env.cwd), env.store.bucket(&other));
    assert_eq!(env.store.latest(&env.cwd).unwrap().unwrap().id, here.id);
    assert_eq!(env.store.latest(&other).unwrap().unwrap().id, there.id);
    let ids: Vec<SessionId> = env
        .store
        .list(&env.cwd)
        .unwrap()
        .into_iter()
        .map(|session| session.id)
        .collect();
    assert_eq!(ids, vec![here.id], "这个桶里只有它自己那个 cwd");
}

#[tokio::test]
async fn prune_removes_every_session_but_the_newest_it_keeps() {
    let env = Env::new();
    let mut stored = Vec::new();
    for _ in 0..3 {
        let session = env.store.create(&env.cwd).unwrap();
        env.open(&session, FakeProvider::new(vec![]))
            .await
            .shutdown()
            .await;
        stored.push(session);
        std::thread::sleep(Duration::from_millis(10));
    }

    let removed = env.store.prune(&env.cwd, 1).unwrap();

    assert_eq!(removed.len(), 2);
    assert!(!stored[0].dir.exists());
    assert!(!stored[1].dir.exists());
    assert!(stored[2].dir.is_dir());
    assert_eq!(env.store.list(&env.cwd).unwrap().len(), 1);
}

#[tokio::test]
async fn resuming_keeps_the_id_closes_dangling_calls_and_continues_the_session() {
    let env = Env::new();
    let stored = env.store.create(&env.cwd).unwrap();
    env.write("notes.txt", "one\ntwo\n");

    let mut first = env
        .open(
            &stored,
            FakeProvider::new(vec![
                tool_reply("call-read", "read_file", "{\"file_path\":\"notes.txt\"}"),
                tool_reply(
                    "call-edit",
                    "edit_file",
                    "{\"file_path\":\"notes.txt\",\"old_string\":\"one\\n\",\"new_string\":\"uno\\n\"}",
                ),
                Reply::text("renamed it"),
            ]),
        )
        .await;
    first.run_turn("rename the first line").await.unwrap();
    assert_eq!(env.read("notes.txt"), "uno\ntwo\n");
    first.shutdown().await;

    // 崩溃：进程死在一次调用开始与它的结果之间，一个讨论者
    // 与它派出的一个执行者都是如此。
    let crash_args = serde_json::json!({
        "file_path": "notes.txt",
        "old_string": "uno\n",
        "new_string": "eins\n",
    });
    append(
        &stored.log_path,
        SpeakerId::Debater("kimi".into()),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: "editing again".to_owned(),
            reasoning: None,
        },
    );
    append(
        &stored.log_path,
        SpeakerId::Debater("kimi".into()),
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new("call-crash"),
            tool_name: "edit_file".to_owned(),
            args: crash_args,
        },
    );
    append(
        &stored.log_path,
        SpeakerId::Executor(ParticipantId::new("kimi-1")),
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new("call-crash-exec"),
            tool_name: "bash".to_owned(),
            args: serde_json::json!({"command": "sleep 1000"}),
        },
    );

    // `--continue`：存储找到同一个会话，而它的 id 不挪窝。
    let resumed = env.store.latest(&env.cwd).unwrap().unwrap();
    assert_eq!(resumed.id, stored.id);

    let provider = FakeProvider::new(vec![Reply::text("carrying on")]);
    let mut second = env.open(&resumed, provider.clone()).await;
    assert_eq!(second.session_id(), &stored.id);
    let outcome = second.run_turn("what now?").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(outcome.text, "carrying on");
    let requests = provider.requests();
    second.shutdown().await;

    let events = read_events(&stored.log_path).unwrap();

    // 一个会话一个表头，续接多少次都一样。
    let starts: Vec<&Event> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::SessionStarted { .. }))
        .collect();
    assert_eq!(starts.len(), 1, "一次续接永远不追加第二个表头");
    match &starts[0].payload {
        EventPayload::SessionStarted { session_id, .. } => assert_eq!(session_id, &stored.id),
        other => panic!("期望 SessionStarted，实际得到 {other:?}"),
    }

    // 两条悬着的调用各拿到正好一条结果，归属是发起它的那一个，
    // 而且两条都没有被重跑。
    assert!(pending_tool_calls(&events).is_empty(), "没有任何调用悬着");
    for (id, speaker) in [
        ("call-crash", SpeakerId::Debater("kimi".into())),
        (
            "call-crash-exec",
            SpeakerId::Executor(ParticipantId::new("kimi-1")),
        ),
    ] {
        let result = events
            .iter()
            .find(|event| {
                matches!(
                    &event.payload,
                    EventPayload::ToolCallCompleted { tool_call_id, .. }
                        if tool_call_id.as_str() == id
                )
            })
            .expect("被中断的那次调用被收尾了");
        assert_eq!(result.speaker_id, speaker);
        match &result.payload {
            EventPayload::ToolCallCompleted { ok, error, .. } => {
                assert!(!ok);
                let error = error.as_deref().unwrap();
                assert!(error.contains("结果未知"), "{error}");
                assert!(error.contains("没有被重跑"), "{error}");
            }
            other => panic!("期望 ToolCallCompleted，实际得到 {other:?}"),
        }
    }

    // 工作区没有凭一次猜测被重新编辑。
    assert_eq!(env.read("notes.txt"), "uno\ntwo\n");

    // 续接的那个回合真的跑了：它的请求带着恢复出来的结果。
    let last = requests.last().expect("续接的那个回合调用了 provider");
    let recovered = last.messages.iter().find_map(|message| match message {
        Message::Tool {
            tool_call_id,
            content,
        } if tool_call_id == "call-crash" => Some(content.clone()),
        _ => None,
    });
    assert!(
        recovered
            .expect("恢复出来的结果到达了模型")
            .contains("结果未知"),
        "模型被告知那条结果是未知的"
    );

    // 整条链上 id 都稳定，所以两家厂商的前缀缓存
    // 一直命中。
    let session_ids: Vec<&str> = requests
        .iter()
        .map(|request| request.cache_key.as_deref().unwrap())
        .collect();
    assert!(session_ids.iter().all(|id| *id == stored.id.as_str()));
}

#[tokio::test]
async fn undo_restores_a_downgraded_edit_and_retires_it_from_the_projection() {
    let env = Env::new();
    let stored = env.store.create(&env.cwd).unwrap();
    // 文件用制表符缩进；模型发的是空格，所以这次编辑落在
    // line-trim 这一级上，而 `.before` 里存的是 `old_string` 没有的字节。
    env.write("code.rs", "fn main() {\n\trun();\n}\n");

    let provider = FakeProvider::new(vec![
        tool_reply("call-read", "read_file", "{\"file_path\":\"code.rs\"}"),
        tool_reply(
            "call-edit",
            "edit_file",
            "{\"file_path\":\"code.rs\",\"old_string\":\"    run();\",\"new_string\":\"    run_twice();\"}",
        ),
        Reply::text("patched"),
        Reply::text("all clear"),
    ]);
    let mut harness = env.open(&stored, provider.clone()).await;

    harness.run_turn("fix the call").await.unwrap();
    assert_eq!(env.read("code.rs"), "fn main() {\n    run_twice();\n}\n");

    let undone = harness.undo_last_edit().await.unwrap().unwrap();
    assert_eq!(undone.tool_call_id.as_str(), "call-edit");
    assert_eq!(
        undone.path,
        std::fs::canonicalize(env.cwd.join("code.rs")).unwrap()
    );
    // 逐字节回到模型从没发过的那个制表符。
    assert_eq!(env.read("code.rs"), "fn main() {\n\trun();\n}\n");

    // 下一个回合的投影完全不再重放这次编辑。
    harness.run_turn("anything else?").await.unwrap();
    let requests = provider.requests();
    let last = requests.last().unwrap();
    for message in &last.messages {
        if let Message::Tool { tool_call_id, .. } = message {
            assert_ne!(tool_call_id, "call-edit", "这次编辑的结果被退休了");
        }
        if let Message::Assistant { tool_calls, .. } = message {
            assert!(
                !tool_calls.iter().any(|call| call.id == "call-edit"),
                "这次编辑的工具调用被退休了"
            );
        }
    }

    harness.shutdown().await;

    // 流是只追加的：那次退休是一条新事件，点名它退休了哪些
    // seq，而快照的来源原封不动。
    let events = read_events(&stored.log_path).unwrap();
    let superseded = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::HistorySuperseded {
                targets,
                reason,
                summary,
            } if *reason == HistoryReason::Undo => Some((targets.clone(), summary.clone())),
            _ => None,
        })
        .expect("undo 被记成一次退休");
    let (targets, summary) = superseded;
    assert_eq!(targets.len(), 2);
    let retired: Vec<&str> = events
        .iter()
        .filter(|event| targets.contains(&event.seq))
        .map(|event| event.payload.kind())
        .collect();
    assert_eq!(retired, vec!["ToolCallStarted", "ToolCallCompleted"]);
    assert!(summary.unwrap().contains("code.rs"));
    assert_eq!(
        std::fs::read_to_string(stored.outputs_dir.join("call-edit.before")).unwrap(),
        "\trun();"
    );
    #[cfg(unix)]
    assert_eq!(
        mode(&stored.outputs_dir.join("call-edit.before")),
        0o600,
        "产物与事件流一样，只归所有者"
    );
}

#[tokio::test]
async fn undo_with_nothing_left_to_undo_is_a_no_op() {
    let env = Env::new();
    let stored = env.store.create(&env.cwd).unwrap();

    let mut harness = env
        .open(&stored, FakeProvider::new(vec![Reply::text("hello")]))
        .await;
    harness.run_turn("hi").await.unwrap();

    assert_eq!(harness.undo_last_edit().await.unwrap(), None);
    harness.shutdown().await;
}

#[tokio::test]
async fn undo_refuses_a_stale_snapshot_and_leaves_the_file_alone() {
    let env = Env::new();
    let stored = env.store.create(&env.cwd).unwrap();
    env.write("notes.txt", "one\ntwo\n");

    let mut harness = env
        .open(
            &stored,
            FakeProvider::new(vec![
                tool_reply("call-read", "read_file", "{\"file_path\":\"notes.txt\"}"),
                tool_reply(
                    "call-edit",
                    "edit_file",
                    "{\"file_path\":\"notes.txt\",\"old_string\":\"one\\n\",\"new_string\":\"uno\\n\"}",
                ),
                Reply::text("edited"),
            ]),
        )
        .await;
    harness.run_turn("rename it").await.unwrap();

    // 编辑之后有人（或者有什么东西）把这个文件又改了：快照
    // 再也说不出它替换的是哪一段。
    env.write("notes.txt", "something else entirely\n");

    let error = harness.undo_last_edit().await.unwrap_err();
    assert!(error.to_string().contains("撤销"), "{error}");
    assert_eq!(env.read("notes.txt"), "something else entirely\n");
    harness.shutdown().await;
}

#[tokio::test]
async fn repeated_undos_step_back_through_the_sessions_edits() {
    let env = Env::new();
    let stored = env.store.create(&env.cwd).unwrap();
    env.write("notes.txt", "a\nb\n");

    let mut harness = env
        .open(
            &stored,
            FakeProvider::new(vec![
                tool_reply("call-read-1", "read_file", "{\"file_path\":\"notes.txt\"}"),
                tool_reply(
                    "call-edit-1",
                    "edit_file",
                    "{\"file_path\":\"notes.txt\",\"old_string\":\"a\\n\",\"new_string\":\"A\\n\"}",
                ),
                Reply::text("first"),
                tool_reply("call-read-2", "read_file", "{\"file_path\":\"notes.txt\"}"),
                tool_reply(
                    "call-edit-2",
                    "edit_file",
                    "{\"file_path\":\"notes.txt\",\"old_string\":\"b\\n\",\"new_string\":\"B\\n\"}",
                ),
                Reply::text("second"),
            ]),
        )
        .await;
    harness.run_turn("edit a").await.unwrap();
    harness.run_turn("edit b").await.unwrap();
    assert_eq!(env.read("notes.txt"), "A\nB\n");

    // 每一次 undo 回滚最近那一次编辑，而下一次再往回退一步：
    // 那是一次回着走的查询，正是它让这件事成立。
    assert_eq!(
        harness
            .undo_last_edit()
            .await
            .unwrap()
            .unwrap()
            .tool_call_id
            .as_str(),
        "call-edit-2"
    );
    assert_eq!(env.read("notes.txt"), "A\nb\n");
    assert_eq!(
        harness
            .undo_last_edit()
            .await
            .unwrap()
            .unwrap()
            .tool_call_id
            .as_str(),
        "call-edit-1"
    );
    assert_eq!(env.read("notes.txt"), "a\nb\n");
    assert_eq!(harness.undo_last_edit().await.unwrap(), None);
    harness.shutdown().await;
}

#[tokio::test]
async fn undo_restores_every_occurrence_a_replace_all_changed() {
    let env = Env::new();
    let stored = env.store.create(&env.cwd).unwrap();
    env.write("counts.txt", "let a = 1;\nlet b = 1;\n");

    let mut harness = env
        .open(
            &stored,
            FakeProvider::new(vec![
                tool_reply("call-read", "read_file", "{\"file_path\":\"counts.txt\"}"),
                tool_reply(
                    "call-edit",
                    "edit_file",
                    "{\"file_path\":\"counts.txt\",\"old_string\":\"= 1;\",\"new_string\":\"= 2;\",\"replace_all\":true}",
                ),
                Reply::text("bumped both"),
            ]),
        )
        .await;
    harness.run_turn("bump the version").await.unwrap();
    assert_eq!(env.read("counts.txt"), "let a = 2;\nlet b = 2;\n");

    assert!(harness.undo_last_edit().await.unwrap().is_some());
    assert_eq!(env.read("counts.txt"), "let a = 1;\nlet b = 1;\n");
    harness.shutdown().await;
}

#[tokio::test]
async fn the_event_stream_never_keeps_a_call_open_across_a_resume() {
    // 对同一条不变量的更窄一次探查：被中断的调用在任何
    // 续接会话的 provider 请求之前就被收尾，所以不变量 2（绝不在
    // 一次调用缺结果时调用 provider）熬过了这次崩溃。
    let env = Env::new();
    let stored = env.store.create(&env.cwd).unwrap();
    env.open(&stored, FakeProvider::new(vec![]))
        .await
        .shutdown()
        .await;

    append(
        &stored.log_path,
        SpeakerId::Debater("kimi".into()),
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new("call-open"),
            tool_name: "read_file".to_owned(),
            args: serde_json::json!({"file_path": "notes.txt"}),
        },
    );

    let resumed = env.store.latest(&env.cwd).unwrap().unwrap();
    let harness = env
        .open(&resumed, FakeProvider::new(vec![Reply::text("ok")]))
        .await;
    harness.shutdown().await;

    let events = read_events(&stored.log_path).unwrap();
    assert!(pending_tool_calls(&events).is_empty());
    let closed = events
        .iter()
        .position(|event| matches!(event.payload, EventPayload::ToolCallCompleted { .. }))
        .expect("这次调用被收尾了");
    let started = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventPayload::ToolCallStarted { tool_call_id, .. }
                    if tool_call_id.as_str() == "call-open"
            )
        })
        .unwrap();
    assert!(closed > started);
}
