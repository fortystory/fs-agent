//! 动态声明的工具（票 19，spec §14）。
//!
//! 两条接缝：声明是 `config.toml` 的纯函数（解析、
//! 校验、替换 argv），而这次调用像别的工具一样走那唯一一条组装接缝，
//! 所以经过的权限门与事件流
//! 与内置工具经过的是同一批。

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use heng::config::{resolve, EnvMap, ToolDeclaration, DEFAULT_CUSTOM_TOOL_TIMEOUT_MS};
use heng::events::{read_events, Decision, Event, EventPayload, SessionId, SpeakerId, StopReason};
use heng::permissions::{decide, Call, Mode, Policy, Rule, Scope, Subject};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{
    builtin, is_custom_tool, with_dynamic, CustomTool, Effect, Tool, TIMEOUT_PREFIX,
};
use heng::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

/// 一份把一个参数夹在字面前缀与后缀之间回显出来的声明。
const ECHO_TOML: &str = r#"
[tools.test.echo]
description = "Echo one argument."
command = ["/bin/echo", "{text}", "--done"]
parameters = { type = "object", properties = { text = { type = "string" } }, required = ["text"] }
"#;

fn declarations(toml: &str) -> Vec<ToolDeclaration> {
    resolve(Some(toml), &EnvMap::new())
        .expect("这份声明解析得过")
        .tools
}

fn tool(toml: &str) -> CustomTool {
    let declarations = declarations(toml);
    CustomTool::new(declarations.into_iter().next().expect("一份声明"))
}

// --- 声明是配置的纯函数 ----------------------------------------------------

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
    // 「有 `__` 当且仅当在配置里声明过」这条词法判定，只有在没有任何
    // 内置名字带它时才成立（spec §14）。`true` 要的是整张
    // 内置表 —— 可选工具也在内 —— 因为这条不变量说的是
    // 每一个内置工具，而不是某个 headless 会话碰巧
    // 对外声明了什么。
    for spec in builtin(true).specs() {
        assert!(
            !is_custom_tool(&spec.name),
            "内置的 `{}` 不能带 `__`",
            spec.name
        );
    }
}

#[test]
fn the_declared_tool_is_registered_and_recognizable() {
    let registry = with_dynamic(&declarations(ECHO_TOML), false);
    assert!(is_custom_tool("custom__test__echo"));
    assert!(registry.get("custom__test__echo").is_some());
    // 光那张内置表里没有它 —— 整张表、可选工具统统算上，
    // 也仍然不是声明出来的名字的来源。
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

// --- argv 替换以整个元素为单位 ---------------------------------------------

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
        "缺的那个元素被丢掉，而不是留空"
    );
    assert_eq!(
        tool.command(&serde_json::json!({ "text": null })),
        Some(vec!["/bin/echo".to_owned(), "--done".to_owned()]),
        "显式写的 null 同样算缺席"
    );
}

#[test]
fn an_array_or_object_is_one_element_not_an_expansion() {
    // 要害就在这儿：一个值改不动这条命令的形状。
    let tool = tool(ECHO_TOML);
    let argv = tool
        .command(&serde_json::json!({ "text": ["a", "b", "c"] }))
        .expect("argv");
    assert_eq!(argv.len(), 3, "模板里一个元素仍然对应一个元素：{argv:?}");
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
    // 替换以整个元素为单位，所以半截的拼接不会发生。
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

// --- 校验是启动时的工作 ----------------------------------------------------

fn refusal(toml: &str) -> String {
    resolve(Some(toml), &EnvMap::new())
        .expect_err("这份声明必须被拒")
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
    assert!(message.contains("程序"), "{message}");
}

#[test]
fn a_namespace_containing_the_separator_is_refused() {
    // `a__b` 会让这个名字重新解析时歧义。
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

// --- 这次调用走那唯一一条组装接缝 ------------------------------------------

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
        config: heng::config::SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-custom"),
            tools: with_dynamic(&declarations, false),
            locks: heng::tools::PathLocks::new(),
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

    /// 第一次已完成的调用对应的 `(ok, output_or_error)`。
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
            .expect("一次已完成的调用")
    }

    fn exists(&self, name: &str) -> bool {
        self.workspace.join(name).exists()
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.workspace.join(name)).unwrap()
    }
}

/// 一次对声明出来的工具的脚本化调用，然后一条收尾的文本回复。
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
    // 这里每一个 shell 元字符都只是文本：这个值是直接 spawn 时的一个
    // argv 元素，所以没有谁重新解析它（spec §14）。
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
        "这个字面参数被原样回显出来了：{output:?}"
    );
    for marker in ["pwned", "pwned2", "pwned3"] {
        assert!(
            !fixture.exists(marker),
            "`{marker}` 不能存在：没有哪条 shell 解释过这个参数"
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
        "这次调用在它自己的期限上返回了"
    );

    let (ok, output) = fixture.first_result();
    assert!(ok, "超时是一条结果，不是一个失败的调用：{output:?}");
    assert!(output.contains(TIMEOUT_PREFIX), "{output:?}");

    let child = fixture.read("child.pid").trim().parse::<u32>().unwrap();
    wait_until_gone(child).await;

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_dynamic_tool_is_refused_by_the_readonly_mode() {
    // `Exclusive` 意味着权限门把它当写看待，所以那一档模式
    // 不需要任何特例就拒了它（spec §12、§14）。
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
    assert!(!ok, "这次调用必须被拒：{message:?}");
    assert!(
        message.to_lowercase().contains("readonly") || message.to_lowercase().contains("denied"),
        "{message:?}"
    );

    fixture.harness.shutdown().await;
}

#[test]
fn one_tool_rule_covers_every_dynamic_tool() {
    // 名字带命名空间，所以一条 `Tool("custom__*")` 规则就是配置里
    // 声明的一切的兜底（spec §14）。
    let mut policy = Policy::for_mode(Mode::Auto);
    policy.push(Rule::new(
        Subject::Any,
        Scope::Tool("custom__*".to_owned()),
        Decision::Deny,
    ));

    let effect = Effect::Exclusive;
    let argv = vec!["/bin/echo".to_owned(), "hi".to_owned()];
    let call = Call {
        escalation: None,
        masks: &[],
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

/// 从 `/proc` 看过去，一个进程是不是没了（或者已经是个僵尸）。
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
    panic!("进程 {pid} 在它的进程组被杀之后还在跑");
}
