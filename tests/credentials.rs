//! Credentials and the redaction pipeline, end to end (ticket 16, spec §20).
//!
//! The seam is the one the other end-to-end tests use: `assemble` with a
//! scripted provider, assertions on the JSONL stream, the spill files and the
//! workspace. The invariant under test is stated once and checked from several
//! angles: **the text on the stream equals the text the model saw**, a secret
//! value is not in it, and the tool that produced the text still ran on the
//! true value.
//!
//! `outputs/<tool_call_id>.before` is the one artifact that deliberately keeps
//! the true bytes: it is `/undo`'s byte-level restore source and it holds the
//! user's own workspace content, not model output.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use fs_agent::config::SessionConfig;
use fs_agent::events::{
    read_events, Event, EventPayload, Redactor, SessionId, SpeakerId, StopReason, REDACTED,
};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, Message, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::tools::{builtin, PathLocks};
use fs_agent::{
    assemble, assemble_discussion, AssemblyParts, DebaterParts, DiscussionParts, Harness,
    SessionScaffold, SynthesizerParts,
};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

/// A value shaped like a vendor key: long enough to pass the redactor's floor,
/// and distinctive enough that "the stream does not contain this" is a real
/// assertion rather than a coincidence.
const SECRET: &str = "sk-test-SECRET-0123456789";

struct Fixture {
    harness: Harness,
    provider: FakeProvider,
    log_path: PathBuf,
    workspace: PathBuf,
    outputs: PathBuf,
    _dir: tempfile::TempDir,
}

/// Assemble one headless session whose `SessionConfig` carries `secrets` as the
/// values to redact — the same value `Config::session_config` fills in from the
/// resolved provider keys.
async fn fixture(replies: Vec<Reply>, secrets: &[&str]) -> Fixture {
    let config = SessionConfig::new("fake-model").with_redactor(Redactor::new(
        secrets.iter().map(|value| (*value).to_owned()),
    ));
    fixture_with(replies, config).await
}

async fn fixture_with(replies: Vec<Reply>, config: SessionConfig) -> Fixture {
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
            session_id: SessionId::new("s-credentials"),
            tools: builtin(),
            locks: PathLocks::new(),
            policy: Policy::for_mode(Mode::Ask),
            // A user who approves every write, so the only refusals in these
            // tests come from the credential guardrails themselves.
            asker: Some(Arc::new(AlwaysAllow)),
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness,
        provider,
        log_path,
        workspace,
        outputs: session.join("outputs"),
        _dir: dir,
    }
}

impl Fixture {
    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.workspace.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        path
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.workspace.join(name)).unwrap()
    }

    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }

    /// The whole stream as it sits on disk — the artifact a leak would land in.
    fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log_path).unwrap()
    }

    fn completed_texts(&self) -> Vec<String> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::MessageCompleted { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
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

    /// The arguments of the first `ToolCallStarted` for `tool`, as JSON text.
    fn started_args(&self, tool: &str) -> String {
        self.events()
            .iter()
            .find_map(|event| match &event.payload {
                EventPayload::ToolCallStarted {
                    tool_name, args, ..
                } if tool_name == tool => Some(args.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {tool} call was recorded"))
    }
}

/// One call, assembled from the three stream events the adapter would emit.
fn tool_call(id: &str, tool: &str, arguments: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallStarted {
            index: 0,
            id: id.into(),
            name: tool.into(),
        },
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: tool.into(),
            arguments: arguments.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

#[test]
fn the_redactor_replaces_values_exactly_and_ignores_short_ones() {
    let redactor = Redactor::new([
        // The longer value starts with the shorter one, so the longer has to be
        // replaced first: hitting the shorter one first would leave the longer
        // value's tail sitting on the stream.
        "sk-abcdefgh-longer".to_owned(),
        "sk-abcdefgh".to_owned(),
        // Below the floor: a short "secret" would rewrite ordinary prose
        // everywhere and make the session unreadable.
        "abc".to_owned(),
        String::new(),
    ]);

    assert_eq!(
        redactor.redacted("token sk-abcdefgh-longer end"),
        format!("token {REDACTED} end")
    );
    assert_eq!(
        redactor.redacted("token sk-abcdefgh end"),
        format!("token {REDACTED} end")
    );
    assert_eq!(
        redactor.redacted("abc is not redacted"),
        "abc is not redacted"
    );
    assert_eq!(
        redactor.redacted("nothing secret here"),
        "nothing secret here"
    );

    // The JSON walk covers a tool call's arguments, where a value is a leaf of
    // an arbitrary tree rather than a whole string.
    let mut args = serde_json::json!({
        "content": format!("x {SECRET} y"),
        "nested": {"list": ["plain", SECRET]},
        "count": 3,
    });
    redactor.redact_value(&mut args);
    let text = args.to_string();
    assert!(!text.contains("sk-abcdefgh"), "{text}");

    // A redactor with nothing to hide is a no-op, not a rewrite.
    let empty = Redactor::new(Vec::<String>::new());
    assert!(empty.is_empty());
    assert_eq!(empty.redacted("unchanged"), "unchanged");
}

#[tokio::test]
async fn a_secret_in_a_tool_result_and_a_message_body_is_redacted_on_the_stream() {
    let secret_file = format!("token: {SECRET}\n");
    let mut fixture = fixture(
        vec![
            tool_call(
                "call-1",
                "read_file",
                serde_json::json!({"file_path": "notes.txt"}),
            ),
            Reply::text(&format!("I read token: {SECRET} from notes.txt")),
        ],
        &[SECRET],
    )
    .await;
    fixture.write("notes.txt", &secret_file);

    let outcome = fixture
        .harness
        .run_turn("what is in notes.txt?")
        .await
        .unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    // Neither the stream on disk nor the in-memory events carry the value.
    let on_disk = fixture.log_text();
    assert!(!on_disk.contains(SECRET), "{on_disk}");

    // The tool's own result — the file's contents — is redacted on the stream.
    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "{output}");
    assert!(output.contains(REDACTED), "{output}");
    assert!(!output.contains(SECRET), "{output}");

    // So is the assistant's message body: a speaker repeating a key is the one
    // cross-agent channel left once the projection stops replaying other
    // speakers' result bodies (spec §20).
    let texts = fixture.completed_texts();
    let answer = texts.last().unwrap();
    assert!(answer.contains(REDACTED), "{answer}");
    assert!(!texts.iter().any(|text| text.contains(SECRET)), "{texts:?}");

    // The stream's text is the model's text: the second request replays the
    // redacted result, not the value the tool read.
    let requests = fixture.provider.requests();
    let second = requests[1]
        .messages
        .iter()
        .map(message_text)
        .collect::<Vec<_>>();
    assert!(
        second.iter().any(|text| text.contains(REDACTED)),
        "the model must see the redacted text too: {second:?}"
    );
    assert!(
        !second.iter().any(|text| text.contains(SECRET)),
        "{second:?}"
    );

    // The tool ran on the truth: the workspace file it read is unchanged.
    assert_eq!(fixture.read("notes.txt"), secret_file);

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_secret_in_a_tool_call_argument_never_reaches_the_stream_but_the_tool_runs_on_the_truth()
{
    let mut fixture = fixture(
        vec![
            tool_call(
                "call-1",
                "write_file",
                serde_json::json!({"file_path": "copy.txt", "content": SECRET}),
            ),
            Reply::text("wrote it"),
        ],
        &[SECRET],
    )
    .await;

    fixture
        .harness
        .run_turn("copy the token into copy.txt")
        .await
        .unwrap();

    // The write landed the true bytes: redaction happens before the event, not
    // before the tool.
    assert_eq!(fixture.read("copy.txt"), SECRET);

    // Neither the recorded arguments nor the whole stream carry the value.
    let args = fixture.started_args("write_file");
    assert!(args.contains(REDACTED), "{args}");
    assert!(!args.contains(SECRET), "{args}");
    assert!(!fixture.log_text().contains(SECRET));

    // The model's own replayed tool call is redacted too, which is what keeps
    // "the stream's text == the model's text" true after redaction.
    let requests = fixture.provider.requests();
    let replayed = requests[1]
        .messages
        .iter()
        .map(message_text)
        .collect::<Vec<_>>();
    assert!(
        !replayed.iter().any(|text| text.contains(SECRET)),
        "{replayed:?}"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_oversized_result_spills_redacted_text_not_the_secret() {
    let big = format!("{}\n", SECRET.repeat(200));
    let config = SessionConfig::new("fake-model")
        .with_redactor(Redactor::new([SECRET.to_owned()]))
        // The cap has to bite, or there is no spill to assert on.
        .with_max_tool_result_tokens(4);
    let mut fixture = fixture_with(
        vec![
            tool_call(
                "call-1",
                "read_file",
                serde_json::json!({"file_path": "big.txt"}),
            ),
            Reply::text("done"),
        ],
        config,
    )
    .await;
    fixture.write("big.txt", &big);

    fixture.harness.run_turn("read big.txt").await.unwrap();

    // Redaction runs before truncation and before the spill (spec §10: 打码 →
    // 截断 → 落盘), so the `.txt` artifact is redacted as well.
    let spill = std::fs::read_to_string(fixture.outputs.join("call-1.txt")).unwrap();
    assert!(spill.contains(REDACTED), "{spill}");
    assert!(!spill.contains(SECRET), "{spill}");

    let (_, ok, preview) = fixture.results().remove(0);
    assert!(ok, "{preview}");
    assert!(preview.contains(REDACTED), "{preview}");
    assert!(!preview.contains(SECRET), "{preview}");
    assert!(!fixture.log_text().contains(SECRET));

    // The workspace still holds the user's own bytes.
    assert_eq!(fixture.read("big.txt"), big);

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn the_before_snapshot_keeps_the_true_bytes() {
    let mut fixture = fixture(
        vec![
            tool_call(
                "call-1",
                "read_file",
                serde_json::json!({"file_path": "notes.txt"}),
            ),
            tool_call(
                "call-2",
                "edit_file",
                serde_json::json!({
                    "file_path": "notes.txt",
                    "old_string": format!("token: {SECRET}"),
                    "new_string": "token: placeholder",
                }),
            ),
            Reply::text("edited"),
        ],
        &[SECRET],
    )
    .await;
    fixture.write("notes.txt", &format!("token: {SECRET}\n"));

    fixture.harness.run_turn("replace the token").await.unwrap();

    // `outputs/<id>.before` is `/undo`'s byte-level restore source, and it holds
    // the user's own workspace content: it is deliberately **not** redacted.
    let before = std::fs::read_to_string(fixture.outputs.join("call-2.before")).unwrap();
    assert_eq!(before, format!("token: {SECRET}"));

    // The edit itself ran on the truth...
    assert_eq!(fixture.read("notes.txt"), "token: placeholder\n");
    // ...while the recorded arguments, which quote the replaced region, are
    // redacted like every other stream text.
    let args = fixture.started_args("edit_file");
    assert!(args.contains(REDACTED), "{args}");
    assert!(!args.contains(SECRET), "{args}");
    assert!(!fixture.log_text().contains(SECRET));

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn undo_refuses_when_the_recorded_region_held_the_secret() {
    // The cost of redacting `ToolCallStarted.args`: `/undo` locates the region
    // through the recorded `new_string` and replay-verifies it against the
    // recorded `old_string` (spec §11 — the stream carries no byte offsets), and
    // both are `[redacted]` here. It refuses rather than guessing, and the
    // workspace is left exactly as the edit left it.
    let mut fixture = fixture(
        vec![
            tool_call(
                "call-1",
                "read_file",
                serde_json::json!({"file_path": "notes.txt"}),
            ),
            tool_call(
                "call-2",
                "edit_file",
                serde_json::json!({
                    "file_path": "notes.txt",
                    "old_string": format!("token: {SECRET}"),
                    "new_string": "token: placeholder",
                }),
            ),
            Reply::text("edited"),
        ],
        &[SECRET],
    )
    .await;
    fixture.write("notes.txt", &format!("token: {SECRET}\n"));
    fixture.harness.run_turn("replace the token").await.unwrap();

    let refused = fixture.harness.undo_last_edit().await;
    assert!(
        refused.is_err(),
        "undo must refuse rather than guess with redacted arguments: {refused:?}"
    );
    assert_eq!(fixture.read("notes.txt"), "token: placeholder\n");
}

#[tokio::test]
async fn undo_still_works_when_the_edit_does_not_touch_the_secret() {
    // The blast radius of the redaction is the edited region, not the file: an
    // edit elsewhere in the same file keeps its recorded arguments intact and
    // `/undo` restores it.
    let original = format!("token: {SECRET}\nname: placeholder\n");
    let mut fixture = fixture(
        vec![
            tool_call(
                "call-1",
                "read_file",
                serde_json::json!({"file_path": "notes.txt"}),
            ),
            tool_call(
                "call-2",
                "edit_file",
                serde_json::json!({
                    "file_path": "notes.txt",
                    "old_string": "name: placeholder",
                    "new_string": "name: fs-agent",
                }),
            ),
            Reply::text("edited"),
        ],
        &[SECRET],
    )
    .await;
    fixture.write("notes.txt", &original);
    fixture.harness.run_turn("rename the field").await.unwrap();
    assert_eq!(
        fixture.read("notes.txt"),
        format!("token: {SECRET}\nname: fs-agent\n")
    );

    let undone = fixture.harness.undo_last_edit().await.unwrap();
    assert!(undone.is_some(), "the edit should have been undoable");
    assert_eq!(fixture.read("notes.txt"), original);

    // The stream stays redacted throughout.
    assert!(!fixture.log_text().contains(SECRET));
}

#[tokio::test]
async fn a_discussion_refuses_a_roster_whose_redactors_disagree() {
    // The values to scrub are one set shared by every participant on the stream
    // (spec §20), like the token allowance: a disagreement means one speaker's
    // events are scrubbed and another's are not, with nothing to show for it.
    let dir = tempfile::tempdir().unwrap();
    let other = Redactor::new(["sk-other-SECRET-987654321".to_owned()]);
    let debater = |speaker: &str, redactor: Redactor| DebaterParts {
        speaker: SpeakerId::Debater(speaker.into()),
        config: SessionConfig::new("fake-model").with_redactor(redactor),
        provider: Box::new(FakeProvider::new(vec![Reply::text("hi")])),
    };
    let assembled = assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd: dir.path().to_path_buf(),
            log_path: dir.path().join("log.jsonl"),
            session_id: SessionId::new("s-credentials"),
            tools: builtin(),
            locks: PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            hook: None,
            home: None,
        },
        debaters: vec![
            debater("kimi", Redactor::new([SECRET.to_owned()])),
            debater("deepseek", other.clone()),
        ],
        synthesizer: SynthesizerParts {
            config: SessionConfig::new("fake-model").with_redactor(other),
            provider: Box::new(FakeProvider::new(vec![Reply::text("hi")])),
        },
        max_rounds: Some(2),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
    })
    .await;

    let error = match assembled {
        Ok(_) => panic!("a roster that disagrees about the redactor must be refused"),
        Err(error) => error,
    };
    assert!(
        matches!(&error, fs_agent::Error::Discussion(message) if message.contains("redactor")),
        "got {error:?}"
    );
}

#[tokio::test]
async fn a_command_echo_is_redacted_before_it_enters_the_stream() {
    // Exposure path (b) in spec §20: the command's own output. `bash` is
    // `Exclusive`, so in `ask` mode the injected answerer approves the call.
    let mut fixture = fixture(
        vec![
            tool_call(
                "call-1",
                "bash",
                serde_json::json!({"command": "cat notes.txt"}),
            ),
            Reply::text("done"),
        ],
        &[SECRET],
    )
    .await;
    fixture.write("notes.txt", &format!("{SECRET}\n"));

    fixture.harness.run_turn("print notes.txt").await.unwrap();

    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "{output}");
    assert!(output.contains(REDACTED), "{output}");
    assert!(!output.contains(SECRET), "{output}");
    assert!(!fixture.log_text().contains(SECRET));

    // The command ran on the truth: the file it printed is untouched.
    assert_eq!(fixture.read("notes.txt"), format!("{SECRET}\n"));

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn credentials_are_blocked_at_the_filesystem_and_policy_boundaries() {
    let mut fixture = fixture(
        vec![
            tool_call(
                "call-1",
                "read_file",
                serde_json::json!({"file_path": ".env"}),
            ),
            tool_call(
                "call-2",
                "read_file",
                serde_json::json!({"file_path": "/etc/hostname"}),
            ),
            Reply::text("done"),
        ],
        &[SECRET],
    )
    .await;
    fixture.write(".env", &format!("DEEPSEEK_API_KEY={SECRET}\n"));
    fixture.write(".env.example", "DEEPSEEK_API_KEY=\n");

    fixture
        .harness
        .run_turn("read the credentials")
        .await
        .unwrap();

    let results = fixture.results();
    let env = results
        .iter()
        .find(|(id, ..)| id == "call-1")
        .expect("the .env read got a result");
    assert!(!env.1, "the .env family is denied by policy: {env:?}");
    assert!(env.2.contains(".env"), "{:?}", env.2);

    // The key's own home is outside the workspace, so the containment rule
    // refuses it before any tool runs (spec §20).
    let outside = results
        .iter()
        .find(|(id, ..)| id == "call-2")
        .expect("the outside read got a result");
    assert!(!outside.1, "{outside:?}");
    assert!(
        outside.2.contains("outside the session workspace"),
        "{:?}",
        outside.2
    );

    // Nothing from either attempted read is on the stream: the .env file's real
    // key never had a chance to become an event.
    assert!(!fixture.log_text().contains(SECRET));

    fixture.harness.shutdown().await;
}

fn message_text(message: &Message) -> String {
    match message {
        Message::System { content, .. } | Message::User { content, .. } => content.clone(),
        Message::Assistant {
            content,
            reasoning_content,
            tool_calls,
            ..
        } => {
            let mut text = content.clone().unwrap_or_default();
            if let Some(reasoning) = reasoning_content {
                text.push_str(reasoning);
            }
            for call in tool_calls {
                text.push_str(&call.arguments);
            }
            text
        }
        Message::Tool { content, .. } => content.clone(),
    }
}

#[test]
fn running_as_root_is_refused_with_no_bypass() {
    let refusal = fs_agent::cli::root_refusal(0).expect("root is refused");
    assert_eq!(
        refusal,
        fs_agent::render::wording::root_refusal(),
        "the refusal is the wording layer's text"
    );

    assert_eq!(fs_agent::cli::root_refusal(1), None);
    assert_eq!(fs_agent::cli::root_refusal(1000), None);
}

#[test]
fn the_configured_provider_key_becomes_the_sessions_redactor() {
    // The wiring from "a key in config.toml" to "a value the stream scrubs"
    // runs through the one place configuration becomes injected values
    // (`Config::session_config`), so no assembly path has to remember it.
    let file = r#"
default_model = "deepseek-v4-pro"

[providers.deepseek]
api_key = "sk-deepseek-config-key"

[models.deepseek-v4-pro]
provider = "deepseek"
"#;
    let config = fs_agent::config::resolve(Some(file), &fs_agent::config::EnvMap::new()).unwrap();
    let session = config.session_config("deepseek-v4-pro").unwrap();
    assert!(!session.redactor.is_empty());
    assert_eq!(
        session.redactor.redacted("x sk-deepseek-config-key y"),
        format!("x {REDACTED} y")
    );
}
