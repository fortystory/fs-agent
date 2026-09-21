//! Skills: progressive-disclosure instruction packs (spec §9; ticket 08).
//!
//! Three seams are exercised:
//!
//! * the **pure** library in `context::skills` — discovery precedence,
//!   frontmatter parsing, the three budgets, and the recomputed loaded set;
//! * [`trim`] with the aggregate skill-body budget, a pure function over
//!   `messages`;
//! * the **assembly seam** — the catalog is injected as a pinned `user` message
//!   and the model loads a body through the built-in `skill` tool, which lands on
//!   the stream as an ordinary tool result appended at the tail.

mod support;

use std::path::{Path, PathBuf};

use fs_agent::config::SessionConfig;
use fs_agent::context::skills::{
    loaded_skill_names, Skills, MAX_CATALOG_TOKENS, MAX_LOADED_SKILL_TOKENS, MAX_SKILL_TOKENS,
    SKILL_TOOL,
};
use fs_agent::context::{estimate_tokens, trim, TrimPolicy, DROPPED_TOOL_RESULT};
use fs_agent::events::{
    read_events, ContextSource, Event, EventPayload, SessionId, SpeakerId, ToolCallId,
};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::capability::caps_for;
use fs_agent::provider::{FinishReason, Message, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{CaptureBuf, FakeProvider, Reply};

// --- fixtures --------------------------------------------------------------

/// Write `<root>/<dir>/<name>/SKILL.md` with a frontmatter block.
fn write_skill(root: &Path, dir: &str, name: &str, description: &str) {
    write_skill_full(root, dir, name, description, "the body of the skill\n");
}

fn write_skill_full(root: &Path, dir: &str, name: &str, description: &str, body: &str) {
    let path = root.join(dir).join("skills").join(name);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}"),
    )
    .unwrap();
}

fn skill_names(skills: &Skills) -> Vec<String> {
    let mut names: Vec<String> = skills.names().into_iter().map(str::to_owned).collect();
    names.sort();
    names
}

// --- discovery -------------------------------------------------------------

#[test]
fn discovery_reads_project_then_user_roots_in_precedence_order() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("repo");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&home).unwrap();

    // Project level: `.fs-agent` beats `.agents` beats `.claude`.
    write_skill(&cwd, ".fs-agent", "alpha", "project fs-agent wins");
    write_skill(&cwd, ".agents", "alpha", "project agents loses");
    write_skill(&cwd, ".claude", "beta", "project claude");
    // User level: all three roots, plus a name that the project already owns.
    write_skill(
        &home,
        ".config/fs-agent",
        "beta",
        "user fs-agent loses to the project",
    );
    write_skill(&home, ".config/fs-agent", "gamma", "user fs-agent");
    write_skill(&home, ".agents", "delta", "user agents");
    write_skill(&home, ".claude", "epsilon", "user claude");

    let skills = Skills::discover(&cwd, Some(&home));

    assert_eq!(
        skill_names(&skills),
        vec!["alpha", "beta", "delta", "epsilon", "gamma"],
        "both levels and all three roots are scanned"
    );
    assert_eq!(
        skills.get("alpha").unwrap().description,
        "project fs-agent wins",
        "the most specific root wins a name collision"
    );
    assert_eq!(
        skills.get("beta").unwrap().description,
        "project claude",
        "project beats user even when the user root is more specific"
    );
}

#[test]
fn discovery_skips_directories_without_a_usable_skill_file() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_path_buf();
    write_skill(&cwd, ".agents", "alpha", "a real skill");
    // A directory with no SKILL.md.
    std::fs::create_dir_all(cwd.join(".agents/skills/nothing")).unwrap();
    // A SKILL.md without a description cannot enter the catalog.
    let path = cwd.join(".agents/skills/undescribed");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("SKILL.md"), "---\nname: undescribed\n---\nbody\n").unwrap();

    let skills = Skills::discover(&cwd, None);
    assert_eq!(skill_names(&skills), vec!["alpha"]);
}

#[test]
fn a_quoted_description_keeps_its_colons_and_quotes() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_path_buf();
    let path = cwd.join(".agents/skills/quoted");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("SKILL.md"),
        "---\nname: quoted\ndescription: \"Review: check \\\"X\\\" before shipping\"\n---\nbody\n",
    )
    .unwrap();

    let skills = Skills::discover(&cwd, None);
    assert_eq!(
        skills.get("quoted").unwrap().description,
        "Review: check \"X\" before shipping"
    );
}

// --- catalog ---------------------------------------------------------------

#[test]
fn the_catalog_lists_each_invocable_skill_as_name_colon_description() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_path_buf();
    write_skill(&cwd, ".agents", "alpha", "does the alpha thing");
    write_skill(&cwd, ".agents", "beta", "does the beta thing");

    let catalog = Skills::discover(&cwd, None)
        .catalog()
        .expect("two invocable skills make a catalog");
    assert!(
        catalog.contains("- alpha: does the alpha thing"),
        "{catalog}"
    );
    assert!(catalog.contains("- beta: does the beta thing"), "{catalog}");
    assert!(
        catalog.contains(SKILL_TOOL),
        "the catalog names the loading tool"
    );
    assert!(
        estimate_tokens(&catalog) <= MAX_CATALOG_TOKENS,
        "the catalog is independently capped"
    );
}

#[test]
fn the_catalog_omits_late_entries_rather_than_growing_past_its_budget() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_path_buf();
    for name in ["one", "two", "three", "four", "five"] {
        write_skill(
            &cwd,
            ".agents",
            name,
            &format!("{}: a deliberately long description", "d".repeat(120)),
        );
    }
    let skills = Skills::discover(&cwd, None);

    let budget = 80;
    let catalog = skills.render_catalog(budget).expect("the header fits");
    assert!(estimate_tokens(&catalog) <= budget, "{catalog}");
    assert!(
        catalog.contains("omitted"),
        "truncation is explicit: {catalog}"
    );
    assert!(
        // Directories are visited in name order, so `two` is last and is the
        // entry that must be left out whole rather than cut through.
        !catalog.contains("- two:"),
        "a line that does not fit is left out whole: {catalog}"
    );
}

#[test]
fn there_is_no_catalog_without_an_invocable_skill() {
    let dir = tempfile::tempdir().unwrap();
    let skills = Skills::discover(dir.path(), None);
    assert!(skills.is_empty());
    assert_eq!(skills.catalog(), None);
}

// --- disable-model-invocation ---------------------------------------------

#[test]
fn a_disabled_skill_is_absent_from_the_catalog_and_refuses_to_load() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_path_buf();
    write_skill(&cwd, ".agents", "public", "usable by the model");
    let path = cwd.join(".agents/skills/secret");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("SKILL.md"),
        "---\nname: secret\ndescription: user only\ndisable-model-invocation: true\n---\nbody\n",
    )
    .unwrap();

    let skills = Skills::discover(&cwd, None);
    let catalog = skills.catalog().unwrap();
    assert!(!catalog.contains("secret"), "not in the catalog: {catalog}");
    assert!(catalog.contains("public"));

    let error = skills.load("secret").unwrap_err().to_string();
    assert!(
        error.contains("disable-model-invocation"),
        "the refusal names the flag: {error}"
    );
    assert_eq!(skills.load("public").unwrap(), "the body of the skill");
    // The user path ignores the flag: `/<name>` is exactly what it reserves.
    assert_eq!(
        skills.invoke("secret").unwrap(),
        "body",
        "the user can load a model-disabled skill"
    );
}

#[test]
fn loading_an_unknown_skill_is_an_error_that_points_at_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let skills = Skills::discover(dir.path(), None);
    let error = skills.load("nope").unwrap_err().to_string();
    assert!(error.contains("nope"), "{error}");
    assert!(error.contains("catalog"), "{error}");
}

// --- single-skill budget ---------------------------------------------------

#[test]
fn a_body_over_the_single_skill_cap_is_truncated_with_a_pointer_to_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_path_buf();
    let long = "x".repeat((MAX_SKILL_TOKENS as usize) * 4 + 4_000);
    write_skill_full(&cwd, ".agents", "long", "a long one", &long);
    let skills = Skills::discover(&cwd, None);

    let loaded = skills.load("long").unwrap();
    assert!(
        estimate_tokens(&loaded) <= MAX_SKILL_TOKENS,
        "the body is capped at ~{MAX_SKILL_TOKENS} tokens"
    );
    assert!(loaded.contains("truncated"), "{loaded}");
    assert!(
        loaded.contains("SKILL.md"),
        "the pointer names the file to read in full: {loaded}"
    );
}

#[test]
fn a_body_under_the_cap_loads_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_path_buf();
    write_skill_full(&cwd, ".agents", "short", "a short one", "do exactly this\n");
    let skills = Skills::discover(&cwd, None);
    assert_eq!(skills.load("short").unwrap(), "do exactly this");
}

// --- the loaded set is recomputed, not stored ------------------------------

#[test]
fn loaded_skill_names_is_recomputed_from_the_stream() {
    let debater = SpeakerId::Debater("kimi".into());
    let call = |seq: u64, id: &str, name: &str| {
        Event::new(
            seq,
            debater.clone(),
            EventPayload::ToolCallStarted {
                tool_call_id: ToolCallId::new(id),
                tool_name: SKILL_TOOL.to_owned(),
                args: serde_json::json!({ "name": name }),
            },
        )
    };
    let completed = |seq: u64, id: &str, ok: bool| {
        Event::new(
            seq,
            debater.clone(),
            EventPayload::ToolCallCompleted {
                tool_call_id: ToolCallId::new(id),
                ok,
                output: ok.then(|| "body".to_owned()),
                error: (!ok).then(|| "no such skill".to_owned()),
                duration_ms: 1,
            },
        )
    };

    let events = vec![
        call(1, "c1", "alpha"),
        completed(2, "c1", true),
        call(3, "c2", "nope"),
        completed(4, "c2", false),
        // A second load of the same skill is not a second entry.
        call(5, "c3", "alpha"),
        completed(6, "c3", true),
        // A different tool that happens to take a `name` argument is not a skill.
        Event::new(
            7,
            debater.clone(),
            EventPayload::ToolCallStarted {
                tool_call_id: ToolCallId::new("c4"),
                tool_name: "read_file".to_owned(),
                args: serde_json::json!({ "name": "not-a-skill" }),
            },
        ),
        completed(8, "c4", true),
    ];

    assert_eq!(loaded_skill_names(&events), vec!["alpha"]);
}

// --- trim: the aggregate loaded-body budget --------------------------------

fn assistant_calling(calls: &[(&str, &str)]) -> Message {
    Message::Assistant {
        content: None,
        reasoning_content: None,
        tool_calls: calls
            .iter()
            .map(|(id, name)| fs_agent::provider::ToolCall {
                id: (*id).to_owned(),
                name: (*name).to_owned(),
                arguments: "{}".to_owned(),
            })
            .collect(),
        name: Some("kimi".to_owned()),
    }
}

fn tool_result(id: &str, content: &str) -> Message {
    Message::Tool {
        tool_call_id: id.to_owned(),
        content: content.to_owned(),
    }
}

fn tool_content(message: &Message) -> Option<&str> {
    match message {
        Message::Tool { content, .. } => Some(content),
        _ => None,
    }
}

#[test]
fn old_skill_bodies_are_stubbed_once_the_loaded_total_exceeds_its_budget() {
    let policy = TrimPolicy {
        loaded_skill_budget: 150,
        ..TrimPolicy::default()
    };
    let messages = vec![
        Message::User {
            content: "rules".to_owned(),
            name: None,
            injected: true,
        },
        Message::User {
            content: "first".to_owned(),
            name: Some("user".to_owned()),
            injected: false,
        },
        assistant_calling(&[("s1", SKILL_TOOL), ("o1", "read_file"), ("s2", SKILL_TOOL)]),
        tool_result("s1", &"a".repeat(400)),
        tool_result("o1", &"b".repeat(400)),
        tool_result("s2", &"c".repeat(400)),
        Message::User {
            content: "second".to_owned(),
            name: Some("user".to_owned()),
            injected: false,
        },
    ];

    // The window budget is generous: only the skill-body budget can trigger a
    // drop, and it must reach the oldest skill body, never the ordinary result.
    let trimmed = trim(messages, 10_000, &policy).unwrap();

    assert_eq!(
        tool_content(&trimmed[3]),
        Some(DROPPED_TOOL_RESULT),
        "the oldest skill body is dropped first"
    );
    assert_eq!(
        tool_content(&trimmed[5]),
        Some("c".repeat(400).as_str()),
        "the newest skill body still fits the loaded budget"
    );
    assert_eq!(
        tool_content(&trimmed[4]),
        Some("b".repeat(400).as_str()),
        "the aggregate skill budget never touches ordinary tool results"
    );
}

#[test]
fn the_loaded_skill_budget_is_a_total_across_the_active_round_too() {
    // Every body here was loaded in the active round, so the window's
    // "never drop the current round" rule does not apply: the aggregate budget
    // is a cap on the whole request, and the oldest body goes.
    let policy = TrimPolicy {
        loaded_skill_budget: 150,
        ..TrimPolicy::default()
    };
    let messages = vec![
        Message::User {
            content: "rules".to_owned(),
            name: None,
            injected: true,
        },
        Message::User {
            content: "question".to_owned(),
            name: Some("user".to_owned()),
            injected: false,
        },
        assistant_calling(&[("s1", SKILL_TOOL), ("s2", SKILL_TOOL)]),
        tool_result("s1", &"a".repeat(400)),
        tool_result("s2", &"c".repeat(400)),
    ];

    let trimmed = trim(messages, 10_000, &policy).unwrap();

    assert_eq!(
        tool_content(&trimmed[3]),
        Some(DROPPED_TOOL_RESULT),
        "the oldest loaded body is dropped even inside the active round"
    );
    assert_eq!(
        tool_content(&trimmed[4]),
        Some("c".repeat(400).as_str()),
        "only as many bodies as the budget allows survive"
    );
}

#[test]
fn the_default_loaded_skill_budget_is_the_spec_value() {
    let policy = TrimPolicy::default();
    assert_eq!(policy.loaded_skill_budget, MAX_LOADED_SKILL_TOKENS);
    assert_eq!(policy.sticky_tool_names, vec![SKILL_TOOL.to_owned()]);
}

// --- the assembly seam -----------------------------------------------------

struct Fixture {
    harness: Option<Harness>,
    provider: FakeProvider,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, workspace: &Path) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    std::fs::create_dir_all(&session).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::with_caps(replies, caps_for("deepseek-flash").unwrap());
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.to_path_buf(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-skills"),
            tools: fs_agent::tools::builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            hook: None,
            // No user-level roots, so a test never reads the machine's own skills.
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness: Some(harness),
        provider,
        log_path,
        _dir: dir,
    }
}

impl Fixture {
    async fn run_turn(&mut self, input: &str) -> fs_agent::agent::TurnOutcome {
        self.harness
            .as_mut()
            .expect("harness already shut down")
            .run_turn(input)
            .await
            .unwrap()
    }

    async fn shutdown(&mut self) {
        if let Some(harness) = self.harness.take() {
            harness.shutdown().await;
        }
    }

    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }
}

/// A response that asks for one `skill(name)` call.
fn skill_reply(id: &str, name: &str) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: SKILL_TOOL.to_owned(),
            arguments: serde_json::json!({ "name": name }).to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

fn completed_output(events: &[Event], tool_call_id: &str) -> Result<String, String> {
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                tool_call_id: id,
                ok,
                output,
                error,
                ..
            } if id.as_str() == tool_call_id => Some(if *ok {
                Ok(output.clone().unwrap_or_default())
            } else {
                Err(error.clone().unwrap_or_default())
            }),
            _ => None,
        })
        .expect("the call has exactly one result")
}

#[tokio::test]
async fn the_catalog_is_injected_once_and_stays_pinned_ahead_of_history() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();
    write_skill(&workspace, ".agents", "alpha", "does the alpha thing");
    std::fs::write(workspace.join("AGENTS.md"), "PROJECT RULES\n").unwrap();

    let mut fixture = fixture(vec![Reply::text("hi"), Reply::text("again")], &workspace).await;
    fixture.run_turn("first").await;
    fixture.run_turn("second").await;
    fixture.shutdown().await;

    let events = fixture.events();
    let injections: Vec<&EventPayload> = events
        .iter()
        .filter_map(|event| match &event.payload {
            payload @ EventPayload::ContextInjected { .. } => Some(payload),
            _ => None,
        })
        .collect();
    // Two events, each with its own source...
    assert_eq!(injections.len(), 2, "one AGENTS.md and one skills catalog");
    assert!(matches!(
        injections[0],
        EventPayload::ContextInjected {
            source: ContextSource::AgentsMd,
            ..
        }
    ));
    let EventPayload::ContextInjected {
        source: ContextSource::SkillsCatalog,
        content,
    } = injections[1]
    else {
        panic!("the second injection is the catalog");
    };
    assert!(
        content.contains("- alpha: does the alpha thing"),
        "{content}"
    );

    // ...but the projection merges the leading injections into the one pinned
    // first `user` message (spec §10: the rules and the catalog share it), so
    // the wire never carries consecutive same-role messages. It is byte-stable
    // every turn so the prefix cache keeps hitting. The program's identity leads.
    for request in fixture.provider.requests() {
        assert!(
            matches!(request.messages.first(), Some(Message::System { .. })),
            "the identity leads: {:?}",
            request.messages
        );
        assert_eq!(
            request.messages.get(1),
            Some(&Message::User {
                content: format!("PROJECT RULES\n\n{content}"),
                name: None,
                injected: true,
            })
        );
        assert!(
            !matches!(
                request.messages.get(2),
                Some(Message::User { injected: true, .. })
            ),
            "the catalog does not become a second pinned message: {:?}",
            request.messages
        );
    }
}

#[tokio::test]
async fn loading_a_skill_appends_its_body_as_an_ordinary_tool_result_at_the_tail() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();
    write_skill_full(
        &workspace,
        ".agents",
        "alpha",
        "does the alpha thing",
        "STEP ONE: do the alpha thing\n",
    );

    let mut fixture = fixture(
        vec![skill_reply("call-1", "alpha"), Reply::text("followed it")],
        &workspace,
    )
    .await;
    fixture.run_turn("use alpha").await;
    fixture.shutdown().await;

    let events = fixture.events();
    assert_eq!(
        completed_output(&events, "call-1").unwrap(),
        "STEP ONE: do the alpha thing",
        "the body is a tool result, not an injection"
    );
    assert_eq!(
        loaded_skill_names(&events),
        vec!["alpha"],
        "the loaded set is recomputed from the stream"
    );

    // The body is appended after the history it answers, so the cached prefix
    // never moves.
    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 2, "one call to load, one to answer");
    let second = &requests[1];
    assert_eq!(
        second.messages.last(),
        Some(&Message::Tool {
            tool_call_id: "call-1".to_owned(),
            content: "STEP ONE: do the alpha thing".to_owned(),
        }),
        "the skill body is the tail"
    );
    assert!(matches!(
        second.messages.get(1),
        Some(Message::User { name: None, .. })
    ));
}

#[tokio::test]
async fn a_disabled_skill_is_refused_by_the_tool_and_the_turn_continues() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();
    let path = workspace.join(".agents/skills/secret");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("SKILL.md"),
        "---\nname: secret\ndescription: user only\ndisable-model-invocation: true\n---\nbody\n",
    )
    .unwrap();

    let mut fixture = fixture(
        vec![skill_reply("call-1", "secret"), Reply::text("ok")],
        &workspace,
    )
    .await;
    let outcome = fixture.run_turn("load secret").await;
    fixture.shutdown().await;

    assert_eq!(outcome.reason, fs_agent::events::StopReason::Completed);
    let error = completed_output(&fixture.events(), "call-1").unwrap_err();
    assert!(error.contains("disable-model-invocation"), "{error}");
    assert_eq!(loaded_skill_names(&fixture.events()), Vec::<String>::new());
}

#[tokio::test]
async fn the_user_path_reaches_a_model_disabled_skill() {
    // The flag reserves one invocation for the user: the skill is absent from the
    // catalog and the tool refuses it, but `/<name>` loads it into the context at
    // the tail, after the history, so the cached prefix never moves.
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();
    let path = workspace.join(".agents/skills/secret");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("SKILL.md"),
        "---\nname: secret\ndescription: user only\ndisable-model-invocation: true\n---\nUSER ONLY STEP\n",
    )
    .unwrap();

    let mut fixture = fixture(
        vec![Reply::text("first"), Reply::text("did it")],
        &workspace,
    )
    .await;
    fixture.run_turn("hi").await;
    {
        let harness = fixture.harness.as_mut().unwrap();
        assert!(harness.has_skill("secret"), "the front end can offer it");
        assert!(
            harness.skill_names().contains(&"secret"),
            "a disabled skill is still user-invocable: {:?}",
            harness.skill_names()
        );
        harness.load_skill("secret").unwrap();
    }
    fixture.run_turn("go").await;
    fixture.shutdown().await;

    let events = fixture.events();
    let injected = events.iter().find_map(|event| match &event.payload {
        EventPayload::ContextInjected {
            source: ContextSource::Skill,
            content,
        } => Some(content.clone()),
        _ => None,
    });
    assert_eq!(
        injected.as_deref(),
        Some("USER ONLY STEP"),
        "the body is a Skill injection"
    );

    let requests = fixture.provider.requests();
    let messages = &requests[1].messages;
    let at = messages
        .iter()
        .position(|message| {
            matches!(
                message,
                Message::User { content, injected: true, .. } if content == "USER ONLY STEP"
            )
        })
        .expect("the body reached the model as an injected user message");
    assert!(
        at > 0,
        "a mid-session injection is not the pinned head: {messages:?}"
    );
    assert!(
        matches!(
            messages.last(),
            Some(Message::User { content, injected: false, .. }) if content == "go"
        ),
        "the task is the turn that follows it: {messages:?}"
    );
}

#[tokio::test]
async fn an_unknown_skill_gets_a_normal_error_result() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixture = fixture(
        vec![skill_reply("call-1", "ghost"), Reply::text("ok")],
        dir.path(),
    )
    .await;
    let outcome = fixture.run_turn("load ghost").await;
    fixture.shutdown().await;

    assert_eq!(outcome.reason, fs_agent::events::StopReason::Completed);
    let error = completed_output(&fixture.events(), "call-1").unwrap_err();
    assert!(error.contains("ghost"), "{error}");
}
