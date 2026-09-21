//! Repo map: an on-demand symbol map of the workspace (spec §9; ticket 09).
//!
//! Three seams are exercised:
//!
//! * the **pure extraction** — the official Rust `tags.scm` through tree-sitter,
//!   names only, with the method/function duplicate collapsed;
//! * the **pure ranking and rendering** — [`rank`] is the replaceable seam, and
//!   [`render`] is the only place the budget is spent;
//! * the **assembly seam** — the model asks for the map through the built-in
//!   `repo_map` tool, the result lands on the stream as an ordinary tool result
//!   (never an injection), and the ranking follows what the session just read.

mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fs_agent::config::{SessionConfig, DEFAULT_REPO_MAP_TOKENS, MAX_REPO_MAP_TOKENS};
use fs_agent::context::estimate_tokens;
use fs_agent::context::repo_map::{
    extract, rank, render, Definition, RankContext, Relevance, RepoMap, Scored, SymbolKind,
    REPO_MAP_TOOL,
};
use fs_agent::events::{
    read_events, Event, EventPayload, Role, SessionId, SpeakerId, ToolCallId,
};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, Message, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{CaptureBuf, FakeProvider, Reply};

// --- pure: extraction ------------------------------------------------------

fn definition(file: &str, name: &str, kind: SymbolKind) -> Definition {
    Definition {
        file: PathBuf::from(file),
        name: name.to_owned(),
        kind,
    }
}

fn scored(file: &str, name: &str) -> Scored {
    Scored {
        definition: Definition {
            file: PathBuf::from(file),
            name: name.to_owned(),
            kind: SymbolKind::Function,
        },
        relevance: Relevance::default(),
        references: 0,
        definitions: 1,
    }
}

fn names(ranked: &[Scored]) -> Vec<&str> {
    ranked
        .iter()
        .map(|scored| scored.definition.name.as_str())
        .collect()
}

#[test]
fn the_official_query_extracts_names_with_their_kinds() {
    let source = r#"
pub struct Widget;
pub enum Colour { Red }
pub trait Greet { fn hello(&self); }
pub mod inner { pub fn nested() {} }
pub fn top() {}
macro_rules! shout { () => {}; }

impl Greet for Widget {
    fn hello(&self) {}
}
"#;

    let symbols = extract(source);
    let found: Vec<(&str, SymbolKind)> = symbols
        .definitions
        .iter()
        .map(|(name, kind)| (name.as_str(), *kind))
        .collect();

    assert!(found.contains(&("Widget", SymbolKind::Class)), "{found:?}");
    assert!(found.contains(&("Colour", SymbolKind::Class)), "{found:?}");
    assert!(found.contains(&("Greet", SymbolKind::Interface)), "{found:?}");
    assert!(found.contains(&("inner", SymbolKind::Module)), "{found:?}");
    // The official query classifies a function inside any `declaration_list` —
    // an `impl`, a `trait`, or a `mod` body — as a method. The map follows the
    // query rather than second-guessing it.
    assert!(found.contains(&("nested", SymbolKind::Method)), "{found:?}");
    assert!(found.contains(&("top", SymbolKind::Function)), "{found:?}");
    assert!(found.contains(&("shout", SymbolKind::Macro)), "{found:?}");
    assert!(
        symbols.references.iter().any(|name| name == "Greet"),
        "a trait impl references the trait: {:?}",
        symbols.references
    );
}

#[test]
fn a_method_is_recorded_once_with_the_method_kind() {
    // The official query matches a method twice: once through the impl block's
    // `definition.method`, once through the generic `definition.function`. The
    // map must not list it twice.
    let symbols = extract("impl Widget { pub fn build() -> Self { Widget } }\n");
    let builds: Vec<&(String, SymbolKind)> = symbols
        .definitions
        .iter()
        .filter(|(name, _)| name == "build")
        .collect();
    assert_eq!(builds.len(), 1, "{:?}", symbols.definitions);
    assert_eq!(builds[0].1, SymbolKind::Method);
}

// --- pure: ranking ---------------------------------------------------------

#[test]
fn rank_puts_session_relevant_symbols_first() {
    let definitions = vec![
        definition("src/a.rs", "alpha", SymbolKind::Function),
        definition("src/b.rs", "beta", SymbolKind::Function),
        definition("src/g.rs", "gamma", SymbolKind::Function),
    ];
    let references = BTreeMap::new();

    // No context at all: the stable alphabetical tiebreak.
    let plain = rank(&definitions, &references, &RankContext::default());
    assert_eq!(names(&plain), ["alpha", "beta", "gamma"]);

    // A file this session read outranks an alphabetically earlier one.
    let context = RankContext {
        recent_paths: vec![PathBuf::from("src/b.rs")],
        ..Default::default()
    };
    assert_eq!(
        names(&rank(&definitions, &references, &context)),
        ["beta", "alpha", "gamma"]
    );

    // A name the recent conversation used is relevance too.
    let context = RankContext {
        recent_identifiers: vec!["gamma".to_owned()],
        ..Default::default()
    };
    assert_eq!(
        names(&rank(&definitions, &references, &context)),
        ["gamma", "alpha", "beta"]
    );

    // The model's explicit focus is the strongest signal.
    let context = RankContext {
        focus: vec!["alpha".to_owned()],
        recent_paths: vec![PathBuf::from("src/b.rs")],
        ..Default::default()
    };
    assert_eq!(
        names(&rank(&definitions, &references, &context)),
        ["alpha", "beta", "gamma"]
    );
}

#[test]
fn rank_breaks_ties_on_the_structural_signal() {
    let definitions = vec![
        definition("src/a.rs", "alpha", SymbolKind::Function),
        definition("src/a.rs", "beta", SymbolKind::Function),
        definition("src/b.rs", "beta", SymbolKind::Method),
    ];
    let mut references = BTreeMap::new();
    references.insert("alpha".to_owned(), 7);

    let ranked = rank(&definitions, &references, &RankContext::default());

    assert_eq!(ranked[0].definition.name, "alpha", "more references wins the tie");
    assert_eq!(ranked[0].references, 7);
    assert_eq!(ranked[0].definitions, 1);
    // The two `beta` sites are the same name defined in two files: they beat
    // nothing, and are ordered by file path.
    assert_eq!(names(&ranked), ["alpha", "beta", "beta"]);
    assert_eq!(ranked[1].definitions, 2);
    assert_eq!(ranked[1].definition.file, PathBuf::from("src/a.rs"));
    assert_eq!(ranked[2].definition.file, PathBuf::from("src/b.rs"));
}

#[test]
fn focus_matches_the_file_path_as_well_as_the_name() {
    let definitions = vec![
        definition("src/repo_map.rs", "something", SymbolKind::Function),
        definition("src/other.rs", "whatever", SymbolKind::Function),
    ];
    let context = RankContext {
        focus: vec!["map".to_owned()],
        ..Default::default()
    };
    let ranked = rank(&definitions, &BTreeMap::new(), &context);
    assert_eq!(names(&ranked), ["something", "whatever"]);
    assert_eq!(ranked[0].relevance.focus_matches, 1);
    assert_eq!(ranked[1].relevance.focus_matches, 0);
}

// --- pure: rendering -------------------------------------------------------

#[test]
fn render_groups_names_by_file_within_the_budget() {
    let ranked = vec![
        scored("src/b.rs", "beta"),
        scored("src/a.rs", "alpha"),
        scored("src/a.rs", "alpine"),
    ];

    let text = render(&ranked, Path::new(""), 1_000);

    assert_eq!(text, "src/b.rs: beta\nsrc/a.rs: alpha, alpine");
    assert!(estimate_tokens(&text) <= 1_000);
}

#[test]
fn render_omits_whole_symbols_and_notes_how_many() {
    let ranked: Vec<Scored> = (0..80)
        .map(|index| scored("src/generated.rs", &format!("generated_symbol_{index:03}")))
        .collect();

    let text = render(&ranked, Path::new(""), 60);

    assert!(estimate_tokens(&text) <= 60, "{text}");
    assert!(text.starts_with("src/generated.rs: generated_symbol_000"), "{text}");
    assert!(text.contains("more symbol(s) omitted"), "{text}");
    // Whole names only: no half-printed symbol ever appears.
    for line in text.lines().filter(|line| !line.starts_with('[')) {
        for name in line.split(": ").nth(1).unwrap_or_default().split(", ") {
            assert!(
                name == "generated_symbol_000"
                    || ranked.iter().any(|scored| scored.definition.name == name),
                "a whole symbol: {line}"
            );
        }
    }
}

// --- pure: session context -------------------------------------------------

#[test]
fn session_context_collects_recent_paths_and_identifiers() {
    let cwd = Path::new("/work");
    let events = vec![
        Event::new(
            1,
            SpeakerId::User,
            EventPayload::MessageCompleted {
                role: Role::User,
                text: "please fix the TrimPolicy budget is".to_owned(),
                reasoning: None,
            },
        ),
        Event::new(
            2,
            SpeakerId::Debater("kimi".into()),
            EventPayload::ToolCallStarted {
                tool_call_id: ToolCallId::new("call-1"),
                tool_name: "read_file".to_owned(),
                args: serde_json::json!({ "file_path": "./src/context.rs" }),
            },
        ),
        Event::new(
            3,
            SpeakerId::User,
            EventPayload::ToolCallStarted {
                tool_call_id: ToolCallId::new("call-2"),
                tool_name: "bash".to_owned(),
                args: serde_json::json!({ "command": "echo src/ignored.rs" }),
            },
        ),
    ];

    let context = RankContext::from_session(&events, cwd);

    assert_eq!(
        context.recent_paths,
        vec![PathBuf::from("/work/src/context.rs")],
        "only the file tools contribute, and `./` is collapsed"
    );
    assert!(context.recent_identifiers.contains(&"trimpolicy".to_owned()));
    assert!(context.recent_identifiers.contains(&"budget".to_owned()));
    assert!(
        !context.recent_identifiers.contains(&"is".to_owned()),
        "a token shorter than the identifier minimum is not an identifier"
    );
    assert!(context.focus.is_empty());
}

// --- the map over a real directory -----------------------------------------

fn write_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn build_maps_the_workspace_and_reuses_its_cache() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_file(
        root,
        "src/lib.rs",
        "pub fn root_fn() {}\npub struct RootType;\n",
    );
    write_file(root, "src/nested/mod.rs", "pub trait Nested {}\n");

    let map = RepoMap::new();
    let context = RankContext::default();
    let text = map.build(root, &context, DEFAULT_REPO_MAP_TOKENS);

    assert!(text.contains("src/lib.rs: "), "{text}");
    assert!(text.contains("root_fn"), "{text}");
    assert!(text.contains("RootType"), "{text}");
    assert!(text.contains("src/nested/mod.rs: Nested"), "{text}");
    assert!(estimate_tokens(&text) <= DEFAULT_REPO_MAP_TOKENS);
    assert_eq!(map.parses(), 2, "one parse per file");

    let again = map.build(root, &context, DEFAULT_REPO_MAP_TOKENS);
    assert_eq!(again, text, "a repeated call is byte-stable");
    assert_eq!(map.parses(), 2, "unchanged files are served from the cache");

    // A changed file is re-parsed and the map follows the change.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_file(root, "src/lib.rs", "pub fn renamed_fn() {}\n");
    let updated = map.build(root, &context, DEFAULT_REPO_MAP_TOKENS);
    assert!(updated.contains("renamed_fn"), "{updated}");
    assert!(!updated.contains("root_fn"), "{updated}");
    assert!(map.parses() > 2, "the changed file was parsed again");
}

#[test]
fn build_ignores_non_rust_files_and_hidden_directories() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_file(root, "src/lib.rs", "pub fn visible() {}\n");
    write_file(root, "notes.md", "pub fn not_rust() {}\n");
    write_file(root, "target/generated.rs", "pub fn build_output() {}\n");
    write_file(root, ".hidden/secret.rs", "pub fn hidden() {}\n");

    let text = RepoMap::new().build(root, &RankContext::default(), DEFAULT_REPO_MAP_TOKENS);

    assert!(text.contains("visible"), "{text}");
    assert!(!text.contains("not_rust"), "{text}");
    assert!(!text.contains("build_output"), "{text}");
    assert!(!text.contains("hidden"), "{text}");
}

#[test]
fn an_empty_workspace_produces_an_empty_map() {
    let dir = tempfile::tempdir().unwrap();
    let text = RepoMap::new().build(dir.path(), &RankContext::default(), DEFAULT_REPO_MAP_TOKENS);
    assert!(text.is_empty(), "{text}");
}

#[test]
fn the_configured_budget_is_capped_at_the_documented_ceiling() {
    let config = SessionConfig::new("fake-model").with_repo_map_tokens(999_999);
    assert_eq!(config.repo_map_tokens, MAX_REPO_MAP_TOKENS);
    assert_eq!(
        SessionConfig::new("fake-model").repo_map_tokens,
        DEFAULT_REPO_MAP_TOKENS,
        "the default is the documented 1k"
    );

    let dir = tempfile::tempdir().unwrap();
    for index in 0..600 {
        write_file(
            dir.path(),
            &format!("src/module_{index:03}.rs"),
            &format!("pub fn a_rather_long_symbol_name_number_{index:03}() {{}}\n"),
        );
    }
    // Even a caller that bypasses configuration cannot exceed the ceiling.
    let text = RepoMap::new().build(dir.path(), &RankContext::default(), u64::MAX);
    assert!(
        estimate_tokens(&text) <= MAX_REPO_MAP_TOKENS,
        "~{} tokens",
        estimate_tokens(&text)
    );
    assert!(text.contains("omitted"), "{text}");
}

// --- the assembly seam -----------------------------------------------------

struct Fixture {
    harness: Option<Harness>,
    provider: FakeProvider,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, workspace: &Path, config: SessionConfig) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    std::fs::create_dir_all(&session).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::new(replies);
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.to_path_buf(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-repo-map"),
            tools: fs_agent::tools::builtin(),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            hook: None,
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

fn tool_call(id: &str, index: u32, name: &str, args: serde_json::Value) -> StreamEvent {
    StreamEvent::ToolCallCompleted {
        index,
        id: id.to_owned(),
        name: name.to_owned(),
        arguments: args.to_string(),
    }
}

fn repo_map_reply(id: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        tool_call(id, 0, REPO_MAP_TOOL, args),
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
async fn repo_map_lands_as_an_ordinary_tool_result_never_an_injection() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();
    write_file(
        &workspace,
        "src/lib.rs",
        "pub fn mapped_symbol() {}\npub struct MappedType;\n",
    );

    let mut fixture = fixture(
        vec![repo_map_reply("call-1", serde_json::json!({})), Reply::text("done")],
        &workspace,
        SessionConfig::new("fake-model"),
    )
    .await;
    fixture.run_turn("map it").await;
    fixture.shutdown().await;

    let events = fixture.events();
    let output = completed_output(&events, "call-1").unwrap();
    assert!(output.contains("mapped_symbol"), "{output}");
    assert!(output.contains("MappedType"), "{output}");
    assert!(
        estimate_tokens(&output) <= DEFAULT_REPO_MAP_TOKENS,
        "~{} tokens",
        estimate_tokens(&output)
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::ContextInjected { .. })),
        "the map is never injected: {events:?}"
    );

    // Appended at the tail, after the call that asked for it, so the cached
    // prefix never moves.
    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 2, "one call for the map, one to answer");
    let second = &requests[1];
    assert!(
        matches!(second.messages.first(), Some(Message::User { .. })),
        "there is no pinned injection here, only the user's question: {:?}",
        second.messages
    );
    assert!(
        matches!(second.messages.last(), Some(Message::Tool { tool_call_id, .. }) if tool_call_id == "call-1"),
        "{:?}",
        second.messages
    );
}

#[tokio::test]
async fn repo_map_ranks_the_file_the_session_just_read_first() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();
    write_file(&workspace, "src/alpha.rs", "pub fn alpha_symbol() {}\n");
    write_file(&workspace, "src/beta.rs", "pub fn beta_symbol() {}\n");

    let mut fixture = fixture(
        vec![
            Reply::Stream(vec![
                tool_call(
                    "read-1",
                    0,
                    "read_file",
                    serde_json::json!({ "file_path": "src/beta.rs" }),
                ),
                tool_call("map-1", 1, REPO_MAP_TOOL, serde_json::json!({})),
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::text("done"),
        ],
        &workspace,
        SessionConfig::new("fake-model"),
    )
    .await;
    fixture.run_turn("what is beta?").await;
    fixture.shutdown().await;

    let output = completed_output(&fixture.events(), "map-1").unwrap();
    let beta = output.find("src/beta.rs:").expect("beta in the map");
    let alpha = output.find("src/alpha.rs:").expect("alpha in the map");
    assert!(
        beta < alpha,
        "the just-read file ranks first, whatever the alphabet says: {output}"
    );
}

#[tokio::test]
async fn repo_map_ignores_a_model_supplied_tokens_argument() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();
    for index in 0..120 {
        write_file(
            &workspace,
            &format!("src/module_{index:03}.rs"),
            &format!("pub fn a_symbol_name_that_is_long_enough_{index:03}() {{}}\n"),
        );
    }

    let mut fixture = fixture(
        vec![
            repo_map_reply("call-1", serde_json::json!({ "tokens": 999_999 })),
            Reply::text("done"),
        ],
        &workspace,
        SessionConfig::new("fake-model"),
    )
    .await;
    fixture.run_turn("map everything").await;
    fixture.shutdown().await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(
        estimate_tokens(&output) <= DEFAULT_REPO_MAP_TOKENS,
        "the budget is configuration, not an argument: ~{} tokens",
        estimate_tokens(&output)
    );
    assert!(output.contains("omitted"), "{output}");
}

#[tokio::test]
async fn repo_map_says_so_when_the_workspace_has_no_rust_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();

    let mut fixture = fixture(
        vec![repo_map_reply("call-1", serde_json::json!({})), Reply::text("ok")],
        &workspace,
        SessionConfig::new("fake-model"),
    )
    .await;
    fixture.run_turn("anything here?").await;
    fixture.shutdown().await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("no Rust symbols found"), "{output}");
}

// --- the recorded regression baseline --------------------------------------

/// The measured cost of one map over a large repository, kept as the regression
/// baseline the ticket asks for.
///
/// Synthetic on purpose: the numbers must not depend on what happens to be on
/// this machine. Run it with
/// `cargo test --test repo_map -- --ignored --nocapture`.
#[test]
#[ignore = "timing baseline; run explicitly with --ignored --nocapture"]
fn large_repo_baseline() {
    use std::time::Instant;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut bytes = 0usize;
    let mut files = 0usize;
    for file in 0..400 {
        let mut source = String::new();
        for symbol in 0..12 {
            source.push_str(&format!(
                "pub fn symbol_{file:03}_{symbol:02}() {{ helper_{symbol}(); }}\n"
            ));
        }
        source.push_str("pub fn helper_0() {}\npub struct Type_000;\n");
        bytes += source.len();
        files += 1;
        write_file(root, &format!("src/module_{file:03}.rs"), &source);
    }

    let map = RepoMap::new();
    let context = RankContext::default();
    let started = Instant::now();
    let text = map.build(root, &context, DEFAULT_REPO_MAP_TOKENS);
    let cold = started.elapsed();

    let started = Instant::now();
    let again = map.build(root, &context, DEFAULT_REPO_MAP_TOKENS);
    let warm = started.elapsed();

    println!(
        "repo_map baseline: {files} files / {bytes} bytes -> cold {cold:?} ({} parses), \
         warm {warm:?}, output {} chars ~{} tokens",
        map.parses(),
        text.len(),
        estimate_tokens(&text),
    );

    assert_eq!(again, text, "the cache does not change the output");
    assert!(estimate_tokens(&text) <= DEFAULT_REPO_MAP_TOKENS);
}
