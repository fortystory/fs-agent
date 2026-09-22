//! Dynamically declared tools (ticket 19, spec §14).
//!
//! Two seams: the declaration is a pure function of `config.toml` (parse,
//! validate, substitute argv), and the call runs through the one assembly seam
//! like any other tool, so the permission gate and the event stream are the same
//! ones a built-in goes through.

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fs_agent::config::{resolve, EnvMap, ToolDeclaration, DEFAULT_CUSTOM_TOOL_TIMEOUT_MS};
use fs_agent::events::{
    read_events, Decision, Event, EventPayload, SessionId, SpeakerId, StopReason,
};
use fs_agent::permissions::{decide, Call, Mode, Policy, Rule, Scope, Subject};
use fs_agent::provider::{FinishReason, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::tools::{
    builtin, is_custom_tool, with_dynamic, CustomTool, Effect, Tool, TIMEOUT_PREFIX,
};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

/// A declaration that echoes one argument between a literal prefix and suffix.
const ECHO_TOML: &str = r#"
[tools.test.echo]
description = "Echo one argument."
command = ["/bin/echo", "{text}", "--done"]
parameters = { type = "object", properties = { text = { type = "string" } }, required = ["text"] }
"#;

fn declarations(toml: &str) -> Vec<ToolDeclaration> {
    resolve(Some(toml), &EnvMap::new())
        .expect("the declaration parses")
        .tools
}

fn tool(toml: &str) -> CustomTool {
    let declarations = declarations(toml);
    CustomTool::new(declarations.into_iter().next().expect("one declaration"))
}

// --- the declaration is a pure function of the config ----------------------

#[test]
fn a_declaration_becomes_a_namespaced_tool() {
    let config = resolve(Some(ECHO_TOML), &EnvMap::new()).unwrap();
    assert_eq!(config.tools.len(), 1);
    let declaration = &config.tools[0];
    assert_eq!(declaration.name, "custom__test__echo");
    assert_eq!(declaration.namespace, "test");
    assert_eq!(declaration.tool, "echo");
    assert_eq!(declaration.timeout_ms, DEFAULT_CUSTOM_TOOL_TIMEOUT_MS);
}

#[test]
fn builtin_names_never_contain_the_separator() {
    // The lexical predicate "has `__` iff declared in configuration" only holds
    // while no built-in name contains it (spec §14). `true` asks for the whole
    // built-in table — the optional tool included — because the invariant is
    // about every built-in, not about whatever a headless session happens to
    // advertise.
    for spec in builtin(true).specs() {
        assert!(
            !is_custom_tool(&spec.name),
            "built-in `{}` must not contain `__`",
            spec.name
        );
    }
}

#[test]
fn the_declared_tool_is_registered_and_recognizable() {
    let registry = with_dynamic(&declarations(ECHO_TOML), false);
    assert!(is_custom_tool("custom__test__echo"));
    assert!(registry.get("custom__test__echo").is_some());
    // The built-in table alone does not have it — the full table, optional
    // tools and all, is still not where a declared name comes from.
    assert!(builtin(true).get("custom__test__echo").is_none());
}

#[test]
fn a_dynamic_tool_cannot_claim_to_be_read_only() {
    let registry = with_dynamic(&declarations(ECHO_TOML), false);
    let declared = registry.get("custom__test__echo").unwrap();
    assert_eq!(declared.effect(&serde_json::json!({})), Effect::Exclusive);
}

#[test]
fn the_declared_schema_is_sent_verbatim() {
    let registry = with_dynamic(&declarations(ECHO_TOML), false);
    let spec = registry.get("custom__test__echo").unwrap().spec();
    assert_eq!(spec.name, "custom__test__echo");
    assert_eq!(spec.description, "Echo one argument.");
    assert_eq!(
        spec.parameters,
        serde_json::json!({
            "type": "object",
            "properties": { "text": { "type": "string" } },
            "required": ["text"]
        })
    );
}

// --- argv substitution is by whole element ---------------------------------

#[test]
fn a_parameter_replaces_whole_argv_elements() {
    let tool = tool(ECHO_TOML);
    assert_eq!(
        tool.command(&serde_json::json!({ "text": "hi" })),
        Some(vec![
            "/bin/echo".to_owned(),
            "hi".to_owned(),
            "--done".to_owned()
        ])
    );
}

#[test]
fn an_absent_parameter_omits_its_element() {
    let tool = tool(ECHO_TOML);
    assert_eq!(
        tool.command(&serde_json::json!({})),
        Some(vec!["/bin/echo".to_owned(), "--done".to_owned()]),
        "the missing element is dropped, not left empty"
    );
    assert_eq!(
        tool.command(&serde_json::json!({ "text": null })),
        Some(vec!["/bin/echo".to_owned(), "--done".to_owned()]),
        "an explicit null is absent too"
    );
}

#[test]
fn an_array_or_object_is_one_element_not_an_expansion() {
    // The whole point: a value cannot change the shape of the command.
    let tool = tool(ECHO_TOML);
    let argv = tool
        .command(&serde_json::json!({ "text": ["a", "b", "c"] }))
        .expect("argv");
    assert_eq!(
        argv.len(),
        3,
        "still one element per template element: {argv:?}"
    );
    assert_eq!(
        argv[1],
        serde_json::to_string(&serde_json::json!(["a", "b", "c"])).unwrap()
    );

    let argv = tool
        .command(&serde_json::json!({ "text": { "k": 1 } }))
        .expect("argv");
    assert_eq!(argv.len(), 3, "{argv:?}");
    assert_eq!(
        argv[1],
        serde_json::to_string(&serde_json::json!({ "k": 1 })).unwrap()
    );
}

#[test]
fn a_placeholder_inside_a_larger_element_is_literal() {
    // Substitution is by whole element, so a partial splice is not performed.
    let toml = r#"
[tools.test.echo]
description = "Echo."
command = ["/bin/echo", "--path={p}"]
parameters = { type = "object" }
"#;
    let tool = tool(toml);
    assert_eq!(
        tool.command(&serde_json::json!({ "p": "/tmp/x" })),
        Some(vec!["/bin/echo".to_owned(), "--path={p}".to_owned()])
    );
}

#[test]
fn numbers_and_booleans_render_as_their_json_text() {
    let toml = r#"
[tools.test.echo]
description = "Echo."
command = ["/bin/echo", "{n}", "{flag}"]
parameters = { type = "object", properties = { n = { type = "integer" }, flag = { type = "boolean" } } }
"#;
    let tool = tool(toml);
    assert_eq!(
        tool.command(&serde_json::json!({ "n": 3, "flag": true })),
        Some(vec![
            "/bin/echo".to_owned(),
            "3".to_owned(),
            "true".to_owned()
        ])
    );
}

// --- validation is startup work --------------------------------------------

fn refusal(toml: &str) -> String {
    resolve(Some(toml), &EnvMap::new())
        .expect_err("the declaration must be refused")
        .to_string()
}

#[test]
fn an_unknown_placeholder_is_a_startup_error() {
    let message = refusal(
        r#"
[tools.test.echo]
description = "Echo."
command = ["/bin/echo", "{missing}"]
parameters = { type = "object", properties = { text = { type = "string" } } }
"#,
    );
    assert!(message.contains("missing"), "{message}");
}

#[test]
fn a_placeholder_as_the_program_is_refused() {
    let message = refusal(
        r#"
[tools.test.echo]
description = "Echo."
command = ["{program}", "hi"]
parameters = { type = "object", properties = { program = { type = "string" } } }
"#,
    );
    assert!(message.contains("program"), "{message}");
}

#[test]
fn a_namespace_containing_the_separator_is_refused() {
    // `a__b` would make the name ambiguous to reparse.
    let message = refusal(
        r#"
[tools.a__b.echo]
description = "Echo."
command = ["/bin/echo", "hi"]
parameters = { type = "object" }
"#,
    );
    assert!(message.contains("__"), "{message}");
}

#[test]
fn an_empty_command_is_refused() {
    let message = refusal(
        r#"
[tools.test.echo]
description = "Echo."
command = []
parameters = { type = "object" }
"#,
    );
    assert!(message.contains("command"), "{message}");
}

// --- the call goes through the one assembly seam ---------------------------

struct Fixture {
    harness: Harness,
    log_path: PathBuf,
    workspace: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(toml: &str, replies: Vec<Reply>, policy: Policy) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::new(replies);
    let declarations = declarations(toml);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider),
        speaker: SpeakerId::Debater("kimi".into()),
        config: fs_agent::config::SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-custom"),
            tools: with_dynamic(&declarations, false),
            locks: fs_agent::tools::PathLocks::new(),
            policy,
            asker: Some(Arc::new(AlwaysAllow)),
            questions: None,
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
        _dir: dir,
    }
}

impl Fixture {
    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }

    /// `(ok, output_or_error)` for the first completed call.
    fn first_result(&self) -> (bool, String) {
        self.events()
            .iter()
            .find_map(|event| match &event.payload {
                EventPayload::ToolCallCompleted {
                    ok, output, error, ..
                } => Some((
                    *ok,
                    output.clone().or_else(|| error.clone()).unwrap_or_default(),
                )),
                _ => None,
            })
            .expect("one completed call")
    }

    fn exists(&self, name: &str) -> bool {
        self.workspace.join(name).exists()
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.workspace.join(name)).unwrap()
    }
}

/// A scripted call to a declared tool, then a closing text reply.
fn call_reply(id: &str, name: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: name.into(),
            arguments: args.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

#[tokio::test]
async fn a_dynamic_tool_runs_argv_without_a_shell() {
    // Every shell metacharacter here is just text: the value is one argv element
    // of a direct spawn, so nothing re-parses it (spec §14).
    let payload = "; touch pwned; $(touch pwned2); `touch pwned3`";
    let mut fixture = fixture(
        ECHO_TOML,
        vec![
            call_reply(
                "call-1",
                "custom__test__echo",
                serde_json::json!({ "text": payload }),
            ),
            Reply::text("done"),
        ],
        Policy::for_mode(Mode::Auto),
    )
    .await;

    let outcome = fixture.harness.run_turn("echo it").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);

    let (ok, output) = fixture.first_result();
    assert!(ok, "{output:?}");
    assert!(
        output.contains(payload),
        "the literal argument is echoed back: {output:?}"
    );
    for marker in ["pwned", "pwned2", "pwned3"] {
        assert!(
            !fixture.exists(marker),
            "`{marker}` must not exist: no shell interpreted the argument"
        );
    }

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_dynamic_tool_times_out_and_kills_its_process_tree() {
    let toml = r#"
[tools.test.slow]
description = "Background a child and wait."
command = ["bash", "-c", "sleep 30 & echo $! > child.pid; wait"]
parameters = { type = "object" }
timeout_ms = 300
"#;
    let mut fixture = fixture(
        toml,
        vec![
            call_reply("call-1", "custom__test__slow", serde_json::json!({})),
            Reply::text("done"),
        ],
        Policy::for_mode(Mode::Auto),
    )
    .await;

    let started = std::time::Instant::now();
    fixture.harness.run_turn("slow").await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the call returned at its own deadline"
    );

    let (ok, output) = fixture.first_result();
    assert!(ok, "a timeout is a result, not a failed call: {output:?}");
    assert!(output.contains(TIMEOUT_PREFIX), "{output:?}");

    let child = fixture.read("child.pid").trim().parse::<u32>().unwrap();
    wait_until_gone(child).await;

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_dynamic_tool_is_refused_by_the_readonly_mode() {
    // `Exclusive` means the gate treats it as a write, so the mode refuses it
    // without a special case (spec §12, §14).
    let mut fixture = fixture(
        ECHO_TOML,
        vec![
            call_reply(
                "call-1",
                "custom__test__echo",
                serde_json::json!({ "text": "hi" }),
            ),
            Reply::text("done"),
        ],
        Policy::for_mode(Mode::Readonly),
    )
    .await;

    fixture.harness.run_turn("echo it").await.unwrap();
    let (ok, message) = fixture.first_result();
    assert!(!ok, "the call must be refused: {message:?}");
    assert!(
        message.to_lowercase().contains("readonly") || message.to_lowercase().contains("denied"),
        "{message:?}"
    );

    fixture.harness.shutdown().await;
}

#[test]
fn one_tool_rule_covers_every_dynamic_tool() {
    // The names are namespaced so a single `Tool("custom__*")` rule is the
    // backstop for everything declared in configuration (spec §14).
    let mut policy = Policy::for_mode(Mode::Auto);
    policy.push(Rule::new(
        Subject::Any,
        Scope::Tool("custom__*".to_owned()),
        Decision::Deny,
    ));

    let effect = Effect::Exclusive;
    let argv = vec!["/bin/echo".to_owned(), "hi".to_owned()];
    let call = Call {
        tool_name: "custom__git__status",
        effect: &effect,
        write_targets: &[],
        read_targets: &[],
        argv: Some(&argv),
        cwd: Path::new("."),
        home: None,
        path_error: None,
    };
    let verdict = decide(&policy, &SpeakerId::Debater("kimi".into()), &call);
    assert_eq!(verdict.decision, Decision::Deny);
    assert!(verdict.reason.contains("custom__*"), "{}", verdict.reason);
}

/// Whether a process is gone (or a zombie) as seen through `/proc`.
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
