//! The `bash` tool end to end (ticket 20): a real command's result, a non-zero
//! exit, the `rm` circuit breaker through the shell wrapper, and the process
//! tree a timeout kills.
//!
//! The seam is the one the other end-to-end tests use: `assemble` with a
//! scripted provider, assertions on the JSONL stream and the workspace. Nothing
//! here opens a second seam.

mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use fs_agent::config::{SessionConfig, DEFAULT_BASH_TIMEOUT_MS, MAX_BASH_TIMEOUT_MS};
use fs_agent::events::{
    read_events, Decision, Event, EventPayload, SessionId, SpeakerId, StopReason,
};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::tools::{builtin, BashLimits, Effect, STDERR_HEADER, STDOUT_HEADER, TIMEOUT_PREFIX};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

struct Fixture {
    harness: Harness,
    log_path: PathBuf,
    workspace: PathBuf,
    outputs: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, mode: Mode, config: SessionConfig) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::new(replies);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-bash"),
            tools: builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(mode),
            // An answerer that allows everything, so nothing but the breaker or
            // a mode can refuse a call in these tests.
            asker: Some(Arc::new(AlwaysAllow)),
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness,
        log_path,
        workspace,
        outputs: session.join("outputs"),
        _dir: dir,
    }
}

impl Fixture {
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.workspace.join(name)).unwrap()
    }

    fn exists(&self, name: &str) -> bool {
        self.workspace.join(name).exists()
    }

    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }

    /// `(tool_call_id, ok, output_or_error)` for every completed call.
    fn results(&self) -> Vec<(String, bool, String)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::ToolCallCompleted {
                    tool_call_id,
                    ok,
                    output,
                    error,
                    ..
                } => Some((
                    tool_call_id.as_str().to_owned(),
                    *ok,
                    output.clone().or_else(|| error.clone()).unwrap_or_default(),
                )),
                _ => None,
            })
            .collect()
    }

    fn decisions(&self) -> Vec<(Decision, Option<String>)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::PermissionDecided {
                    decision, reason, ..
                } => Some((*decision, reason.clone())),
                _ => None,
            })
            .collect()
    }
}

/// A scripted `bash` call.
fn bash_reply(id: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: "bash".into(),
            arguments: args.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

fn run(id: &str, command: &str) -> Reply {
    bash_reply(id, serde_json::json!({ "command": command }))
}

fn run_with_timeout(id: &str, command: &str, timeout_ms: u64) -> Reply {
    bash_reply(
        id,
        serde_json::json!({ "command": command, "timeout_ms": timeout_ms }),
    )
}

// --- the declared shape (no process needed) -------------------------------

#[test]
fn bash_declares_one_shell_argv_and_an_exclusive_effect() {
    let registry = builtin();
    let bash = registry.get("bash").expect("bash is a built-in tool");

    assert_eq!(
        bash.effect(&serde_json::json!({ "command": "echo hi" })),
        Effect::Exclusive,
        "a shell can write anything, so the workspace lock is taken"
    );
    assert_eq!(
        bash.command(&serde_json::json!({ "command": "echo hi" })),
        Some(vec![
            "bash".to_owned(),
            "-lc".to_owned(),
            "echo hi".to_owned()
        ]),
        "the command is one argv element: the model cannot splice a second shell"
    );
    assert_eq!(
        bash.command(&serde_json::json!({})),
        None,
        "a call with no command declares no argv"
    );
    assert_eq!(
        bash.command(&serde_json::json!({ "command": "   " })),
        None,
        "a blank command declares no argv"
    );
}

#[test]
fn the_configured_limits_are_the_default_and_a_hard_ceiling() {
    let config = SessionConfig::new("fake-model");
    assert_eq!(config.bash_timeout_ms, DEFAULT_BASH_TIMEOUT_MS);
    assert_eq!(config.max_bash_timeout_ms, MAX_BASH_TIMEOUT_MS);

    let limits = BashLimits::default();
    assert_eq!(
        limits.timeout(None),
        Duration::from_millis(DEFAULT_BASH_TIMEOUT_MS)
    );
    assert_eq!(limits.timeout(Some(25)), Duration::from_millis(25));
    assert_eq!(
        limits.timeout(Some(u64::MAX)),
        Duration::from_millis(MAX_BASH_TIMEOUT_MS),
        "a model may ask for less, never for more"
    );
}

// --- a real command -------------------------------------------------------

#[tokio::test]
async fn a_real_command_reports_its_stdout_and_exit_code() {
    let mut fixture = fixture(
        vec![
            run("call-bash", "printf 'made\\n' > made.txt; echo hello"),
            Reply::text("done"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;

    let outcome = fixture.harness.run_turn("run it").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "the command succeeded: {output}");
    assert!(
        output.starts_with("exit code: 0\n"),
        "the status leads the result: {output:?}"
    );
    assert!(output.contains(STDOUT_HEADER), "{output:?}");
    assert!(output.contains(STDERR_HEADER), "{output:?}");
    assert!(output.contains("hello"), "{output:?}");

    // The workspace side effect is the point of `bash`, not just its text.
    assert_eq!(fixture.read("made.txt"), "made\n");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_non_zero_exit_is_a_result_the_model_can_read() {
    let mut fixture = fixture(
        vec![
            run("call-bash", "echo oops >&2; exit 3"),
            Reply::text("saw it"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;

    fixture.harness.run_turn("fail it").await.unwrap();

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "a failing command is data, not a ToolError: {output:?}");
    assert!(output.contains("exit code: 3"), "{output:?}");
    assert!(
        output.contains("oops"),
        "stderr is in the result: {output:?}"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_zero_timeout_is_refused_as_an_argument_error() {
    let mut fixture = fixture(
        vec![
            run_with_timeout("call-bash", "echo never", 0),
            Reply::text("ok"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;

    fixture.harness.run_turn("try it").await.unwrap();

    let (_, ok, message) = fixture.results().remove(0);
    assert!(!ok);
    assert!(message.contains("positive"), "{message}");
    assert!(
        !fixture.exists("made.txt"),
        "nothing ran for a refused argument"
    );

    fixture.harness.shutdown().await;
}

// --- the circuit breaker --------------------------------------------------

#[tokio::test]
async fn rm_rf_root_is_refused_by_the_circuit_breaker() {
    // `auto` plus an always-allow answerer: only the breaker may refuse this.
    let mut fixture = fixture(
        vec![run("call-bash", "rm -rf /"), Reply::text("it refused")],
        Mode::Auto,
        SessionConfig::new("fake-model"),
    )
    .await;

    fixture.harness.run_turn("wipe it").await.unwrap();

    let decisions = fixture.decisions();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].0, Decision::Deny);
    assert!(
        decisions[0]
            .1
            .as_deref()
            .unwrap()
            .contains("circuit breaker"),
        "the audit names the breaker: {:?}",
        decisions[0].1
    );

    let (_, ok, message) = fixture.results().remove(0);
    assert!(!ok, "the shell never ran");
    assert!(message.contains("circuit breaker"), "{message}");
}

// --- the timeout and the process tree -------------------------------------

/// Whether a pid has finished: its `/proc` entry is gone, or it is a zombie
/// (killed but not yet reaped by whatever inherited it — still not running).
fn process_is_gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat
            .rsplit_once(')')
            .map(|(_, rest)| rest.trim_start().starts_with(['Z', 'X']))
            .unwrap_or(false),
    }
}

async fn wait_until_gone(pid: u32) {
    for _ in 0..200 {
        if process_is_gone(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("process {pid} is still running after its group was killed");
}

#[tokio::test]
async fn a_timed_out_command_is_killed_with_its_process_tree() {
    // The default is configuration, not a scripted value: the model sends no
    // `timeout_ms`, and the session's configured cap is what fires.
    let mut fixture = fixture(
        vec![
            run("call-bash", "sleep 30 & echo $! > child.pid; wait"),
            Reply::text("timed out"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model").with_bash_timeout_ms(300),
    )
    .await;

    let outcome = fixture.harness.run_turn("sleep").await.unwrap();
    assert_eq!(
        outcome.reason,
        StopReason::Completed,
        "a timeout is a result, not a failed turn"
    );

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "the partial result is reported: {output:?}");
    assert!(output.contains(TIMEOUT_PREFIX), "{output:?}");
    assert!(
        output.contains("killed by signal"),
        "the shell was signalled, not exited: {output:?}"
    );

    // The grandchild the command backgrounded is really gone: a `killpg` on the
    // shell's own group reached it, which killing the shell alone would not.
    let child = fixture.read("child.pid").trim().parse::<u32>().unwrap();
    wait_until_gone(child).await;

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_backgrounded_child_cannot_outlive_the_timeout() {
    // `bash -lc "sleep 30 & echo …"` exits at once, but the backgrounded child
    // inherits the output pipes. A timeout that watched only the shell would
    // hang the turn for the child's whole lifetime.
    let mut fixture = fixture(
        vec![
            run("call-bash", "sleep 30 & echo $! > child.pid"),
            Reply::text("timed out"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model").with_bash_timeout_ms(300),
    )
    .await;

    let started = std::time::Instant::now();
    fixture.harness.run_turn("background it").await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the call returned at its own deadline, not the child's"
    );

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "the result is reported: {output:?}");
    assert!(output.contains(TIMEOUT_PREFIX), "{output:?}");

    let child = fixture.read("child.pid").trim().parse::<u32>().unwrap();
    wait_until_gone(child).await;

    fixture.harness.shutdown().await;
}

// --- the existing truncation pipeline -------------------------------------

#[tokio::test]
async fn an_oversized_result_is_spilled_before_it_reaches_the_stream() {
    let mut fixture = fixture(
        vec![
            run("call-bash", "for i in {1..2000}; do printf b; done"),
            Reply::text("done"),
        ],
        Mode::Auto,
        SessionConfig::new("fake-model").with_max_tool_result_tokens(50),
    )
    .await;

    fixture.harness.run_turn("print a lot").await.unwrap();

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok);
    assert!(output.contains("truncated"), "{output}");
    let pointer = fixture.outputs.join("call-bash.txt");
    assert!(
        output.contains(&pointer.display().to_string()),
        "the stream carries the pointer: {output}"
    );
    let spilled = std::fs::read_to_string(&pointer).unwrap();
    assert!(
        spilled.contains(&"b".repeat(100)),
        "the whole body is on disk"
    );

    fixture.harness.shutdown().await;
}
