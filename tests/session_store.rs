//! Ticket 12: the session store, `--continue`, and `/undo`.
//!
//! The store is driven through the library seam exactly as the CLI will drive
//! it: `SessionStore::create` allocates a session directory for a cwd,
//! `SessionStore::latest` is what `--continue` scans, and the assembly point
//! opens whichever stream that directory holds. No network, no environment.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fs_agent::config::SessionConfig;
use fs_agent::events::{
    pending_tool_calls, read_events, Event, EventLog, EventPayload, HistoryReason, ParticipantId,
    Role, SessionId, SpeakerId, StopReason, ToolCallId,
};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, Message, StreamEvent};
use fs_agent::render::RenderSinks;
use fs_agent::session::{SessionStore, StoredSession};
use fs_agent::tools::{self, PathLocks};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

/// A workspace, a store root, and the two captured sinks — everything a session
/// needs except the provider.
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
        // Both sides are canonicalized, so the bucket key is stable no matter
        // how the temp directory is spelled.
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

    /// A second workspace, for the bucketing tests.
    fn other_cwd(&self) -> PathBuf {
        let path = self._dir.path().join("other");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::canonicalize(path).unwrap()
    }

    /// Open the session directory `stored` through the one assembly seam.
    ///
    /// Fresh and resumed are not a flag here: the directory either holds a
    /// stream or it does not, which is exactly the decision the store made.
    async fn open(&self, stored: &StoredSession, provider: FakeProvider) -> Harness {
        assemble(AssemblyParts {
            provider: Box::new(provider),
            speaker: SpeakerId::Debater("kimi".into()),
            config: SessionConfig::new("fake-model"),
            sinks: RenderSinks {
                stdout_result: Box::new(self.stdout.clone()),
                stderr_diagnostic: Box::new(self.stderr.clone()),
            },
            scaffold: SessionScaffold {
                cwd: self.cwd.clone(),
                log_path: stored.log_path.clone(),
                session_id: stored.id.clone(),
                tools: tools::builtin(),
                locks: PathLocks::new(),
                policy: Policy::for_mode(Mode::Ask),
                asker: Some(Arc::new(AlwaysAllow)),
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

/// One scripted tool call, completed in one stream.
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

/// Append one event to an existing stream, as a process that was killed
/// mid-tool-call would have left it.
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

    // The directory shape is the store's job; the stream is the assembly
    // point's, and it does not exist until a session does.
    assert!(stored.dir.is_dir());
    assert_eq!(
        stored.dir.file_name().unwrap().to_string_lossy(),
        stored.id.as_str()
    );
    assert_eq!(stored.log_path, stored.dir.join("log.jsonl"));
    assert_eq!(stored.outputs_dir, stored.dir.join("outputs"));
    assert!(stored.outputs_dir.is_dir());
    assert!(!stored.log_path.exists());

    // The id is `<UTC timestamp>-<short suffix>`.
    let (stamp, suffix) = stored.id.as_str().rsplit_once('-').unwrap();
    assert_eq!(stamp.len(), 16, "YYYYMMDDTHHMMSSZ: {stamp}");
    assert_eq!(suffix.len(), 8, "eight hex digits: {suffix}");
    assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()), "{suffix}");

    let harness = env.open(&stored, FakeProvider::new(vec![])).await;
    assert_eq!(harness.session_id(), &stored.id);
    harness.shutdown().await;

    assert!(stored.log_path.is_file());
    #[cfg(unix)]
    {
        assert_eq!(mode(&stored.dir), 0o700, "the session directory is private");
        assert_eq!(mode(&stored.outputs_dir), 0o700, "artifact dir is private");
        assert_eq!(mode(&stored.log_path), 0o600, "the stream is private");
        assert_eq!(
            mode(&env.store.bucket(&env.cwd)),
            0o700,
            "the whole store path is private, not just the leaf"
        );
    }

    // The bucket is per workspace, and the authoritative cwd is in the stream.
    let events = read_events(&stored.log_path).unwrap();
    let recorded_cwd = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::SessionStarted { cwd, .. } => Some(cwd.clone()),
            _ => None,
        })
        .expect("the session records its cwd");
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
        "the later session is the one --continue resumes"
    );

    // Activity, not creation order, decides: appending to the older stream makes
    // it the newest again.
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
        "mtime, not id order, is the tie-break --continue uses"
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
    assert_eq!(ids, vec![here.id], "the bucket holds only its own cwd");
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

    // The crash: the process died between a call starting and its result, for a
    // debater and for an executor it had spawned.
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

    // `--continue`: the store finds the same session, and its id does not move.
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

    // One session, one header, whatever the number of resumes.
    let starts: Vec<&Event> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::SessionStarted { .. }))
        .collect();
    assert_eq!(starts.len(), 1, "a resume never appends a second header");
    match &starts[0].payload {
        EventPayload::SessionStarted { session_id, .. } => assert_eq!(session_id, &stored.id),
        other => panic!("expected SessionStarted, got {other:?}"),
    }

    // Both dangling calls got exactly one result, attributed to whoever started
    // them, and neither was re-run.
    assert!(
        pending_tool_calls(&events).is_empty(),
        "no call stays pending"
    );
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
            .expect("the interrupted call is closed");
        assert_eq!(result.speaker_id, speaker);
        match &result.payload {
            EventPayload::ToolCallCompleted { ok, error, .. } => {
                assert!(!ok);
                let error = error.as_deref().unwrap();
                assert!(error.contains("unknown"), "{error}");
                assert!(error.contains("not re-run"), "{error}");
            }
            other => panic!("expected ToolCallCompleted, got {other:?}"),
        }
    }

    // The workspace was not re-edited on the strength of a guess.
    assert_eq!(env.read("notes.txt"), "uno\ntwo\n");

    // The resumed turn really ran: its request carried the recovered result.
    let last = requests
        .last()
        .expect("the resumed turn called the provider");
    let recovered = last.messages.iter().find_map(|message| match message {
        Message::Tool {
            tool_call_id,
            content,
        } if tool_call_id == "call-crash" => Some(content.clone()),
        _ => None,
    });
    assert!(
        recovered
            .expect("the recovered result reaches the model")
            .contains("unknown"),
        "the model is told the result is unknown"
    );

    // The id is stable across the whole chain, so both vendors' prefix caches
    // keep hitting.
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
    // The file indents with a tab; the model sends spaces, so the edit lands on
    // the line-trim level and `.before` holds bytes `old_string` does not.
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
    // Byte-for-byte back to the tab the model never sent.
    assert_eq!(env.read("code.rs"), "fn main() {\n\trun();\n}\n");

    // The next turn's projection no longer replays the edit at all.
    harness.run_turn("anything else?").await.unwrap();
    let requests = provider.requests();
    let last = requests.last().unwrap();
    for message in &last.messages {
        if let Message::Tool { tool_call_id, .. } = message {
            assert_ne!(tool_call_id, "call-edit", "the edit's result is retired");
        }
        if let Message::Assistant { tool_calls, .. } = message {
            assert!(
                !tool_calls.iter().any(|call| call.id == "call-edit"),
                "the edit's tool call is retired"
            );
        }
    }

    harness.shutdown().await;

    // The stream is append-only: the retirement is a new event naming the seqs
    // it retires, and the snapshot source is untouched.
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
        .expect("undo is recorded as a retirement");
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
        "an artifact is owner-only, like the stream"
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

    // Someone (or something) moves the file on after the edit: the snapshot can
    // no longer say which region it replaced.
    env.write("notes.txt", "something else entirely\n");

    let error = harness.undo_last_edit().await.unwrap_err();
    assert!(error.to_string().contains("undo"), "{error}");
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

    // Each undo rolls back the most recent edit, and the next one steps back one
    // further: the walked-back query is what makes that true.
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
    // A narrower probe of the same invariant: an interrupted call is closed
    // before any provider request of the resumed session, so invariant 2 (never
    // call the provider while a call lacks a result) survives the crash.
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
        .expect("the call is closed");
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
