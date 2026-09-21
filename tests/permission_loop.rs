//! The permission gate as the loop drives it: the loop synthesizes exactly one
//! error result for a policy deny, a user deny and a headless `Ask` downgrade,
//! and every call still gets exactly one result.
//!
//! These drive the one assembly seam with a scripted provider and a scripted
//! answerer, then assert the JSONL stream and the workspace — the same
//! observable contract the other end-to-end tests use.

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use fs_agent::config::SessionConfig;
use fs_agent::events::{
    read_events, Decision, DecisionSource, Event, EventPayload, SessionId, SpeakerId,
};
use fs_agent::permissions::{Answer, Asker, Mode, Policy, Rule, Scope, Subject};
use fs_agent::provider::{FinishReason, StreamEvent};
use fs_agent::render::RenderSinks;
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply, ScriptedAsker};

struct Fixture {
    harness: Harness,
    provider: FakeProvider,
    log_path: PathBuf,
    outputs: PathBuf,
    workspace: PathBuf,
    dir: tempfile::TempDir,
}

async fn fixture(
    replies: Vec<Reply>,
    policy: Mode,
    rules: Vec<Rule>,
    asker: Option<Arc<dyn Asker>>,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::new(replies);

    let mut session_policy = Policy::for_mode(policy);
    for rule in rules {
        session_policy.push(rule);
    }

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        sinks: RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        },
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-perm"),
            tools: fs_agent::tools::builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            policy: session_policy,
            asker,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    let outputs = harness.outputs_dir().to_path_buf();
    Fixture {
        harness,
        provider,
        log_path,
        outputs,
        workspace,
        dir,
    }
}

impl Fixture {
    fn write(&self, name: &str, content: &str) {
        let path = self.workspace.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.workspace.join(name)).unwrap()
    }

    fn exists(&self, name: &str) -> bool {
        self.workspace.join(name).exists()
    }

    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }

    fn decisions(&self) -> Vec<(Decision, DecisionSource, Option<String>)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::PermissionDecided {
                    decision,
                    source,
                    reason,
                    ..
                } => Some((*decision, *source, reason.clone())),
                _ => None,
            })
            .collect()
    }

    fn asked_count(&self) -> usize {
        self.events()
            .iter()
            .filter(|event| matches!(event.payload, EventPayload::PermissionAsked { .. }))
            .count()
    }

    fn results(&self) -> Vec<(String, bool, Option<String>)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::ToolCallCompleted {
                    tool_call_id,
                    ok,
                    error,
                    ..
                } => Some((tool_call_id.as_str().to_owned(), *ok, error.clone())),
                _ => None,
            })
            .collect()
    }
}

fn tool_reply(id: &str, name: &str, arguments: &str) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

fn write_reply(id: &str, file: &str) -> Reply {
    tool_reply(
        id,
        "write_file",
        &serde_json::json!({ "file_path": file, "content": "written\n" }).to_string(),
    )
}

#[tokio::test]
async fn a_policy_deny_never_reaches_the_tool_and_yields_one_error_result() {
    let mut fixture = fixture(
        vec![
            write_reply("call-write", "notes.txt"),
            Reply::text("understood"),
        ],
        Mode::Readonly,
        vec![],
        // An answerer being present proves the refusal is a deny, not an ask.
        Some(Arc::new(AlwaysAllow)),
    )
    .await;
    fixture.write("notes.txt", "original\n");

    fixture.harness.run_turn("write it").await.unwrap();
    assert_eq!(
        fixture.read("notes.txt"),
        "original\n",
        "the file is untouched"
    );
    assert!(
        !fixture.outputs.join("call-write.before").exists(),
        "no snapshot for a call that never ran"
    );

    let results = fixture.results();
    assert_eq!(results.len(), 1, "exactly one result");
    assert!(!results[0].1);
    let error = results[0].2.clone().unwrap();
    assert!(error.contains("permission denied"), "{error}");
    assert!(error.contains("readonly"), "{error}");

    assert_eq!(fixture.asked_count(), 0, "a deny never asks");

    let decisions = fixture.decisions();
    assert_eq!(
        decisions.len(),
        1,
        "the denied call gets one policy verdict"
    );
    assert_eq!(decisions[0].0, Decision::Deny);
    assert_eq!(decisions[0].1, DecisionSource::Policy);

    // A deny does not remove the tool from the request prefix: the second call
    // still carries the full tool table.
    let requests = fixture.provider.requests();
    let tools = &requests[1].tools;
    assert!(
        tools.iter().any(|tool| tool.name == "write_file"),
        "the denied tool stays in the tools array"
    );
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_rule_can_deny_in_auto_mode_too() {
    let rule = Rule::new(
        Subject::Any,
        Scope::Tool("write_file".to_owned()),
        Decision::Deny,
    );
    let mut fixture = fixture(
        vec![write_reply("call-write", "notes.txt"), Reply::text("ok")],
        Mode::Auto,
        vec![rule],
        Some(Arc::new(AlwaysAllow)),
    )
    .await;

    fixture.harness.run_turn("write it").await.unwrap();
    assert!(!fixture.exists("notes.txt"));
    assert_eq!(fixture.decisions()[0].0, Decision::Deny);
    assert_eq!(fixture.decisions()[0].1, DecisionSource::Policy);
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn ask_mode_prompts_and_runs_when_the_user_approves() {
    let asker = ScriptedAsker::new(vec![Answer::Allow]);
    let mut fixture = fixture(
        vec![write_reply("call-write", "fresh.txt"), Reply::text("ok")],
        Mode::Ask,
        vec![],
        Some(Arc::new(asker.clone())),
    )
    .await;

    fixture.harness.run_turn("write it").await.unwrap();
    assert_eq!(fixture.read("fresh.txt"), "written\n");
    assert_eq!(fixture.asked_count(), 1, "the write asked exactly once");
    let questions = asker.requests();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].tool_name, "write_file");
    assert!(!questions[0].reason.is_empty());

    let decisions = fixture.decisions();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].0, Decision::Allow);
    assert_eq!(decisions[0].1, DecisionSource::User);
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_user_denial_yields_one_error_result() {
    let asker = ScriptedAsker::new(vec![Answer::Deny]);
    let mut fixture = fixture(
        vec![write_reply("call-write", "fresh.txt"), Reply::text("ok")],
        Mode::Ask,
        vec![],
        Some(Arc::new(asker)),
    )
    .await;

    fixture.harness.run_turn("write it").await.unwrap();
    assert!(!fixture.exists("fresh.txt"), "the user refused the write");
    let results = fixture.results();
    assert_eq!(results.len(), 1);
    assert!(!results[0].1);
    assert!(results[0].2.clone().unwrap().contains("user denied"));

    let decisions = fixture.decisions();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].0, Decision::Deny);
    assert_eq!(decisions[0].1, DecisionSource::User);
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn ask_without_an_answerer_downgrades_to_deny_and_says_why() {
    let mut fixture = fixture(
        vec![write_reply("call-write", "fresh.txt"), Reply::text("ok")],
        Mode::Ask,
        vec![],
        // Headless: nobody can answer.
        None,
    )
    .await;

    fixture.harness.run_turn("write it").await.unwrap();
    assert!(!fixture.exists("fresh.txt"));
    assert_eq!(fixture.asked_count(), 0, "no answerer means no question");

    let decisions = fixture.decisions();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].0, Decision::Deny, "the effective verdict");
    assert_eq!(decisions[0].1, DecisionSource::Policy);
    let reason = decisions[0].2.clone().unwrap();
    assert!(
        reason.contains("no interactive answerer"),
        "the downgrade is recorded, so it is not read as a policy denial: {reason}"
    );

    assert_eq!(fixture.results().len(), 1);
    assert!(fixture.results()[0]
        .2
        .clone()
        .unwrap()
        .contains("downgraded"));
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn always_allow_only_changes_the_session_policy() {
    let asker = ScriptedAsker::new(vec![Answer::AlwaysAllow]);
    let mut fixture = fixture(
        vec![
            write_reply("call-a", "a.txt"),
            write_reply("call-b", "b.txt"),
            Reply::text("ok"),
        ],
        Mode::Ask,
        vec![],
        Some(Arc::new(asker.clone())),
    )
    .await;

    fixture.harness.run_turn("write both").await.unwrap();
    assert_eq!(fixture.read("a.txt"), "written\n");
    assert_eq!(fixture.read("b.txt"), "written\n");

    // The first write asked; the second was settled by the remembered rule.
    assert_eq!(fixture.asked_count(), 1);
    assert_eq!(asker.requests().len(), 1);
    let decisions = fixture.decisions();
    assert_eq!(decisions.len(), 2);
    assert_eq!(decisions[0].1, DecisionSource::User);
    assert_eq!(decisions[1].0, Decision::Allow);
    assert_eq!(
        decisions[1].1,
        DecisionSource::Policy,
        "the second call was allowed by the session policy, not by a new question"
    );

    // No `config.toml` is written anywhere the session could reach: "always
    // allow" changes the `Session` value and nothing else.
    for candidate in [
        "config.toml",
        "session/config.toml",
        "workspace/config.toml",
    ] {
        let path = fixture.dir.path().join(candidate);
        assert!(!path.exists(), "{} must not be written", path.display());
    }
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn the_env_family_is_denied_in_the_loop_but_templates_are_not() {
    let mut fixture = fixture(
        vec![
            tool_reply("call-env", "read_file", r#"{"file_path":".env"}"#),
            tool_reply(
                "call-template",
                "read_file",
                r#"{"file_path":".env.example"}"#,
            ),
            Reply::text("ok"),
        ],
        Mode::Auto,
        vec![],
        Some(Arc::new(AlwaysAllow)),
    )
    .await;
    fixture.write(".env", "SECRET=1\n");
    fixture.write(".env.example", "SECRET=\n");

    fixture.harness.run_turn("read the env").await.unwrap();
    let results = fixture.results();
    assert_eq!(results.len(), 2);
    let env = results
        .iter()
        .find(|(id, ..)| id == "call-env")
        .expect("the .env result");
    assert!(!env.1);
    assert!(env.2.clone().unwrap().contains(".env"));

    let template = results
        .iter()
        .find(|(id, ..)| id == "call-template")
        .expect("the template result");
    assert!(template.1, "{:?}", template.2);

    let denied = fixture
        .decisions()
        .iter()
        .filter(|(decision, ..)| *decision == Decision::Deny)
        .count();
    assert_eq!(denied, 1, "only the real .env was denied");
    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_call_outside_the_workspace_is_denied_by_the_path_limit() {
    // The workspace limit is a gate floor, so even `auto` refuses it and the
    // recorded verdict says `Deny` — not an `Allow` the call never got to use.
    let outside = "/tmp/fs-agent-permission-test-outside.txt";
    let mut fixture = fixture(
        vec![write_reply("call-out", outside), Reply::text("ok")],
        Mode::Auto,
        vec![],
        None,
    )
    .await;

    fixture.harness.run_turn("write outside").await.unwrap();

    let results = fixture.results();
    assert_eq!(results.len(), 1);
    assert!(!results[0].1);
    let error = results[0].2.clone().unwrap();
    assert!(error.contains("permission denied"), "{error}");
    assert!(error.contains("outside the session workspace"), "{error}");

    let decisions = fixture.decisions();
    assert_eq!(decisions.len(), 1, "the call still got a verdict");
    assert_eq!(decisions[0].0, Decision::Deny);
    assert!(!PathBuf::from(outside).exists(), "nothing was written");
    fixture.harness.shutdown().await;
}
