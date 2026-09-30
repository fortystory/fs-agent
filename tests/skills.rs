//! 技能：渐进披露的指令包（spec §9；票 08）。
//!
//! 练到三条接缝：
//!
//! * `context::skills` 里那个**纯函数**库 —— 发现的优先级、
//!   frontmatter 解析、三个预算，以及重算出来的已加载集合；
//! * 带技能正文合计预算的 [`trim`]，一个作用在 `messages` 上的
//!   纯函数；
//! * **组装接缝** —— 技能清单作为一条被钉住的 `user` 消息注入，
//!   而模型通过内置的 `skill` 工具加载一份正文，它以一条普通
//!   工具结果的身份落在流尾巴上。

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

// --- fixture ---------------------------------------------------------------

/// 写出带一段 frontmatter 的 `<root>/<dir>/<name>/SKILL.md`。
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

// --- 发现 ------------------------------------------------------------------

#[test]
fn discovery_reads_project_then_user_roots_in_precedence_order() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("repo");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&home).unwrap();

    // 项目级：`.fs-agent` 赢 `.agents`，`.agents` 赢 `.claude`。
    write_skill(&cwd, ".fs-agent", "alpha", "project fs-agent wins");
    write_skill(&cwd, ".agents", "alpha", "project agents loses");
    write_skill(&cwd, ".claude", "beta", "project claude");
    // 用户级：三个根都扫，另加一个项目已经占掉的名字。
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
        "两级、三个根都扫到了"
    );
    assert_eq!(
        skills.get("alpha").unwrap().description,
        "project fs-agent wins",
        "名字撞车时，更具体的那个根赢"
    );
    assert_eq!(
        skills.get("beta").unwrap().description,
        "project claude",
        "即使用户那个根更具体，项目也压得住用户"
    );
}

#[test]
fn discovery_skips_directories_without_a_usable_skill_file() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_path_buf();
    write_skill(&cwd, ".agents", "alpha", "a real skill");
    // 一个没有 SKILL.md 的目录。
    std::fs::create_dir_all(cwd.join(".agents/skills/nothing")).unwrap();
    // 一份没有 description 的 SKILL.md 进不了技能清单。
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

// --- 技能清单 --------------------------------------------------------------

#[test]
fn the_catalog_lists_each_invocable_skill_as_name_colon_description() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_path_buf();
    write_skill(&cwd, ".agents", "alpha", "does the alpha thing");
    write_skill(&cwd, ".agents", "beta", "does the beta thing");

    let catalog = Skills::discover(&cwd, None)
        .catalog()
        .expect("两份可调用的技能就够成一份清单");
    assert!(
        catalog.contains("- alpha: does the alpha thing"),
        "{catalog}"
    );
    assert!(catalog.contains("- beta: does the beta thing"), "{catalog}");
    assert!(catalog.contains(SKILL_TOOL), "技能清单点了加载工具的名");
    assert!(
        estimate_tokens(&catalog) <= MAX_CATALOG_TOKENS,
        "技能清单有自己独立的上限"
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
    let catalog = skills.render_catalog(budget).expect("表头装得下");
    assert!(estimate_tokens(&catalog) <= budget, "{catalog}");
    assert!(catalog.contains("被省略"), "裁剪是明说的：{catalog}");
    assert!(
        // 目录按名字顺序访问，所以 `two` 排在最后，是那条必须
        // 整条略去、而不是从中间切开的条目。
        !catalog.contains("- two:"),
        "装不下的那一行整条略去：{catalog}"
    );
}

#[test]
fn there_is_no_catalog_without_an_invocable_skill() {
    let dir = tempfile::tempdir().unwrap();
    let skills = Skills::discover(dir.path(), None);
    assert!(skills.is_empty());
    assert_eq!(skills.catalog(), None);
}

// --- disable-model-invocation ----------------------------------------------

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
    assert!(!catalog.contains("secret"), "不在技能清单里：{catalog}");
    assert!(catalog.contains("public"));

    let error = skills.load("secret").unwrap_err().to_string();
    assert!(
        error.contains("disable-model-invocation"),
        "这次拒绝点出了那个旗标：{error}"
    );
    assert_eq!(skills.load("public").unwrap(), "the body of the skill");
    // 用户那条路不看这个旗标：`/<name>` 正是它留下来的那一次调用。
    assert_eq!(
        skills.invoke("secret").unwrap(),
        "body",
        "用户能加载一份禁掉模型调用的技能"
    );
}

#[test]
fn loading_an_unknown_skill_is_an_error_that_points_at_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let skills = Skills::discover(dir.path(), None);
    let error = skills.load("nope").unwrap_err().to_string();
    assert!(error.contains("nope"), "{error}");
    assert!(error.contains("技能清单"), "{error}");
}

// --- 单份技能的预算 --------------------------------------------------------

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
        "正文被压到约 {MAX_SKILL_TOKENS} 个 token"
    );
    assert!(loaded.contains("已截断"), "{loaded}");
    assert!(
        loaded.contains("SKILL.md"),
        "那个指针点名了要读全文的那个文件：{loaded}"
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

// --- 已加载集合是重算出来的，不是存下来的 ----------------------------------

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
        // 同一份技能再加载一次，不算第二条。
        call(5, "c3", "alpha"),
        completed(6, "c3", true),
        // 另一个碰巧也收 `name` 参数的工具，不是技能。
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

// --- 裁剪：已加载正文的合计预算 --------------------------------------------

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

    // 窗口预算很宽松：只有技能正文那个预算可能触发一次
    // 丢弃，而且它必须够到最旧的技能正文，绝不去动普通的结果。
    let trimmed = trim(messages, 10_000, &policy).unwrap();

    assert_eq!(
        tool_content(&trimmed[3]),
        Some(DROPPED_TOOL_RESULT),
        "最旧的技能正文先被丢掉"
    );
    assert_eq!(
        tool_content(&trimmed[5]),
        Some("c".repeat(400).as_str()),
        "最新的技能正文仍然装得进已加载预算"
    );
    assert_eq!(
        tool_content(&trimmed[4]),
        Some("b".repeat(400).as_str()),
        "技能合计预算从不碰普通的工具结果"
    );
}

#[test]
fn the_loaded_skill_budget_is_a_total_across_the_active_round_too() {
    // 这里每一份正文都是在本轮加载的，所以窗口那条
    // 「永远不丢当前轮」的规矩不适用：合计预算
    // 管的是整个请求的上限，于是最旧的那份正文走人。
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
        "即使在当前轮里，最旧的那份已加载正文也会被丢掉"
    );
    assert_eq!(
        tool_content(&trimmed[4]),
        Some("c".repeat(400).as_str()),
        "预算允许多少份正文，就只有多少份活下来"
    );
}

#[test]
fn the_default_loaded_skill_budget_is_the_spec_value() {
    let policy = TrimPolicy::default();
    assert_eq!(policy.loaded_skill_budget, MAX_LOADED_SKILL_TOKENS);
    assert_eq!(policy.sticky_tool_names, vec![SKILL_TOOL.to_owned()]);
}

// --- 组装接缝 --------------------------------------------------------------

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
            tools: fs_agent::tools::builtin(false),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            // 不扫用户级的根，于是测试永远不会读到这台机器自己的技能。
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
            .expect("harness 已经关掉了")
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

/// 一条要求调用一次 `skill(name)` 的响应。
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
        .expect("这次调用正好有一条结果")
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
    // 两条事件，各带自己的来源……
    assert_eq!(injections.len(), 2, "一条 AGENTS.md，一条技能清单");
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
        panic!("第二条注入是技能清单");
    };
    assert!(
        content.contains("- alpha: does the alpha thing"),
        "{content}"
    );

    // ……但投影把开头那几条注入合进那唯一一条被钉住的
    // 首条 `user` 消息（spec §10：规则与清单共用它），所以
    // 线上永远不会出现连着两条同角色的消息。它每一轮都逐字节稳定，
    // 于是前缀缓存一直命中。走在最前面的是程序的身份。
    for request in fixture.provider.requests() {
        assert!(
            matches!(request.messages.first(), Some(Message::System { .. })),
            "身份走在最前面：{:?}",
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
            "技能清单没有变成第二条被钉住的消息：{:?}",
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
        "这份正文是一条工具结果，不是一次注入"
    );
    assert_eq!(
        loaded_skill_names(&events),
        vec!["alpha"],
        "已加载集合是从流里重算出来的"
    );

    // 正文追加在它所回应的那段历史之后，所以被缓存的那段
    // 前缀永远不挪窝。
    let requests = fixture.provider.requests();
    assert_eq!(requests.len(), 2, "一次调用加载，一次作答");
    let second = &requests[1];
    assert_eq!(
        second.messages.last(),
        Some(&Message::Tool {
            tool_call_id: "call-1".to_owned(),
            content: "STEP ONE: do the alpha thing".to_owned(),
        }),
        "技能正文就是那条尾巴"
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
    // 这个旗标给用户留了一次调用：技能不在技能清单里，
    // 工具也拒它，但 `/<name>` 会把它加载进上下文、
    // 落在历史之后的尾巴上，所以被缓存的前缀永远不挪窝。
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
        assert!(harness.has_skill("secret"), "前端能把它摆出来");
        assert!(
            harness.skill_names().contains(&"secret"),
            "禁掉模型调用的技能仍然可以被用户调用：{:?}",
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
        "这份正文是一次 Skill 注入"
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
        .expect("这份正文以一条注入的 user 消息到达了模型");
    assert!(at > 0, "会话中途的注入不是那个被钉住的头部：{messages:?}");
    assert!(
        matches!(
            messages.last(),
            Some(Message::User { content, injected: false, .. }) if content == "go"
        ),
        "任务就是紧随其后的那个回合：{messages:?}"
    );
}

#[tokio::test]
async fn a_bare_skill_invocation_runs_there_and_then() {
    // `/greet` + 回车是**一个**手势：正文被加载，而且它起的那个回合
    // 就跑起来，因为正文*本身*就是指令。一次只加载、不跑的
    // 裸调用，会变成用户必须调两遍的命令。
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();
    let path = workspace.join(".agents/skills/greet");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("SKILL.md"),
        "---\nname: greet\ndescription: say hello\n---\nGREET STEP\n",
    )
    .unwrap();

    let mut fixture = fixture(vec![Reply::text("done")], &workspace).await;
    let outcome = fixture
        .harness
        .as_mut()
        .unwrap()
        .run_skill("greet")
        .await
        .unwrap();
    assert_eq!(
        outcome.reason,
        fs_agent::events::StopReason::Completed,
        "技能自己那个回合跑了"
    );
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
        Some("GREET STEP"),
        "这份正文以一次 Skill 注入的形式上了流"
    );
    // 唯一绝不能出现的东西：一条没人打过的 `user` 消息。这份正文
    // 是一条 `ContextInjected`，把指令带给模型的正是它。
    assert!(
        events.iter().all(|event| !matches!(
            &event.payload,
            EventPayload::MessageCompleted {
                role: fs_agent::events::Role::User,
                ..
            }
        )),
        "没有为这次裸调用凭空造出一条提示词"
    );
    let requests = fixture.provider.requests();
    let messages = &requests[0].messages;
    // 在一个还没有历史的会话里，正文搭的是那条被钉住的头部消息 ——
    // 投影会把相邻的注入合并 —— 所以这里查的是这条指令到了，
    // 而不是它单独到的。要紧的是它带着 `injected`：
    // 说话的是技能，不是用户。
    assert!(
        matches!(
            messages.last(),
            Some(Message::User { content, injected: true, .. }) if content.contains("GREET STEP")
        ),
        "这份正文以一条注入的 `user` 消息到达模型：{messages:?}"
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
