//! 凭证与打码管线，端到端（票 16，spec §20）。
//!
//! 接缝与其它端到端测试用的是同一条：
//! `assemble` 配一个脚本化的 provider，
//! 断言落在 JSONL 流、溢出的落盘文件与工作区上。
//! 被测的那条不变量只陈述一次、从几个角度检查：
//! **流上的文本等于模型看到的文本**，密钥值不在其中，
//! 而产生这段文本的那个工具仍然拿真值跑过。
//!
//! `outputs/<tool_call_id>.before` 是唯一一个故意留着真字节的产物：
//! 它是 `/undo` 的字节级还原源，装的是用户自己的工作区内容，
//! 不是模型输出。

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use heng::config::SessionConfig;
use heng::events::{
    Event, EventPayload, REDACTED, Redactor, SessionId, SpeakerId, StopReason, read_events,
};
use heng::permissions::{Mode, Policy};
use heng::provider::{FinishReason, Message, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{PathLocks, builtin};
use heng::{
    AssemblyParts, DebaterParts, DiscussionParts, Harness, SessionScaffold, SynthesizerParts,
    assemble, assemble_discussion,
};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

/// 一个长得像厂商密钥的值：长到能过打码器的下限，
/// 又足够特别，于是「流上不含这个」是一条货真价实的
/// 断言，而不是巧合。
const SECRET: &str = "sk-test-SECRET-0123456789";

struct Fixture {
    harness: Harness,
    provider: FakeProvider,
    log_path: PathBuf,
    workspace: PathBuf,
    outputs: PathBuf,
    _dir: tempfile::TempDir,
}

/// 组装一个 headless 会话，它的 `SessionConfig` 把 `secrets` 当作要打码的值
/// —— 与 `Config::session_config` 从解析出的 provider 密钥里填进去的
/// 是同一处。
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
            tools: builtin(false),
            locks: PathLocks::new(),
            policy: Policy::for_mode(Mode::Ask),
            // 一个每次写都批准的 user，于是这些测试里唯一的拒绝
            // 都来自凭证护栏自己。
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

    /// 整条流落在磁盘上的样子 —— 泄露会落进去的那个产物。
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

    /// 每个已完成调用的 `(tool_call_id, ok, output_or_error)`。
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

    /// `tool` 的第一条 `ToolCallStarted` 的参数，作为 JSON 文本。
    fn started_args(&self, tool: &str) -> String {
        self.events()
            .iter()
            .find_map(|event| match &event.payload {
                EventPayload::ToolCallStarted {
                    tool_name, args, ..
                } if tool_name == tool => Some(args.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("没有记下 {tool} 调用"))
    }
}

/// 一次调用，由适配器会发出的那三条流事件拼成。
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
        // 长的那个值以短的那个开头，所以必须先替换
        // 长的那个：先撞上短的，长的值的尾巴就会
        // 留在流上。
        "sk-abcdefgh-longer".to_owned(),
        "sk-abcdefgh".to_owned(),
        // 低于下限：一个短的「密钥」会把各处的普通行文
        // 都改写掉，让会话读不下去。
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

    // JSON 遍历覆盖工具调用的参数：那里的值是任意
    // 一棵树的叶子，而不是整段字符串。
    let mut args = serde_json::json!({
        "content": format!("x {SECRET} y"),
        "nested": {"list": ["plain", SECRET]},
        "count": 3,
    });
    redactor.redact_value(&mut args);
    let text = args.to_string();
    assert!(!text.contains("sk-abcdefgh"), "{text}");

    // 没有可藏之物的打码器是空操作，而不是一次改写。
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

    // 磁盘上的流与内存里的事件都不带这个值。
    let on_disk = fixture.log_text();
    assert!(!on_disk.contains(SECRET), "{on_disk}");

    // 工具自己的结果 —— 文件内容 —— 在流上打了码。
    let (_, ok, output) = fixture.results().remove(0);
    assert!(ok, "{output}");
    assert!(output.contains(REDACTED), "{output}");
    assert!(!output.contains(SECRET), "{output}");

    // 助手消息正文也一样：一旦投影不再重放别人的结果正文
    // （spec §20），某个讨论者复述一个密钥就是剩下的
    // 唯一一条跨 agent 通道。
    let texts = fixture.completed_texts();
    let answer = texts.last().unwrap();
    assert!(answer.contains(REDACTED), "{answer}");
    assert!(!texts.iter().any(|text| text.contains(SECRET)), "{texts:?}");

    // 流上的文本就是模型的文本：第二次请求重放的是
    // 打码后的结果，而不是工具读到的那个值。
    let requests = fixture.provider.requests();
    let second = requests[1]
        .messages
        .iter()
        .map(message_text)
        .collect::<Vec<_>>();
    assert!(
        second.iter().any(|text| text.contains(REDACTED)),
        "模型看到的也必须是打码后的文本：{second:?}"
    );
    assert!(
        !second.iter().any(|text| text.contains(SECRET)),
        "{second:?}"
    );

    // 工具拿的是真值：它读的那个工作区文件没变。
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

    // 这次写落下了真字节：打码发生在事件之前，
    // 而不是在工具之前。
    assert_eq!(fixture.read("copy.txt"), SECRET);

    // 记下的参数与整条流都不带这个值。
    let args = fixture.started_args("write_file");
    assert!(args.contains(REDACTED), "{args}");
    assert!(!args.contains(SECRET), "{args}");
    assert!(!fixture.log_text().contains(SECRET));

    // 模型自己那次被重放的调用也打了码，这正是打码之后
    // 「流上的文本 == 模型的文本」仍然成立的原因。
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
        // 上限得真的咬到，否则没有可断言的溢出。
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

    // 打码跑在截断与落盘之前（spec §10：打码 →
    // 截断 → 落盘），所以那个 `.txt` 产物也是打码过的。
    let spill = std::fs::read_to_string(fixture.outputs.join("call-1.txt")).unwrap();
    assert!(spill.contains(REDACTED), "{spill}");
    assert!(!spill.contains(SECRET), "{spill}");

    let (_, ok, preview) = fixture.results().remove(0);
    assert!(ok, "{preview}");
    assert!(preview.contains(REDACTED), "{preview}");
    assert!(!preview.contains(SECRET), "{preview}");
    assert!(!fixture.log_text().contains(SECRET));

    // 工作区里仍然留着用户自己的字节。
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

    // `outputs/<id>.before` 是 `/undo` 的字节级还原源，装的是
    // 用户自己的工作区内容：它**故意**不打码。
    let before = std::fs::read_to_string(fixture.outputs.join("call-2.before")).unwrap();
    assert_eq!(before, format!("token: {SECRET}"));

    // 这次编辑本身拿的是真值……
    assert_eq!(fixture.read("notes.txt"), "token: placeholder\n");
    // ……而被记下的参数（它们引用了被替换的那段区域）
    // 与流上其它文本一样打了码。
    let args = fixture.started_args("edit_file");
    assert!(args.contains(REDACTED), "{args}");
    assert!(!args.contains(SECRET), "{args}");
    assert!(!fixture.log_text().contains(SECRET));

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn undo_refuses_when_the_recorded_region_held_the_secret() {
    // 给 `ToolCallStarted.args` 打码的代价：`/undo` 靠记下的
    // `new_string` 定位那段区域，再用记下的 `old_string` 重放校验它
    // （spec §11 —— 流上不带字节偏移），而这里两者都是
    // `[redacted]`。它宁可拒绝也不猜，工作区则原样留在
    // 这次编辑留下的样子。
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
        "参数打了码时 undo 必须拒绝而不是猜：{refused:?}"
    );
    assert_eq!(fixture.read("notes.txt"), "token: placeholder\n");
}

#[tokio::test]
async fn undo_still_works_when_the_edit_does_not_touch_the_secret() {
    // 打码的波及范围是被编辑的那段区域，而不是整个文件：同一个
    // 文件里别处的编辑，记下的参数原样保存，`/undo`
    // 能把它还原回去。
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
                    "new_string": "name: heng",
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
        format!("token: {SECRET}\nname: heng\n")
    );

    let undone = fixture.harness.undo_last_edit().await.unwrap();
    assert!(undone.is_some(), "这次编辑本该是 undo 得掉的");
    assert_eq!(fixture.read("notes.txt"), original);

    // 整条流一路都保持打码。
    assert!(!fixture.log_text().contains(SECRET));
}

#[tokio::test]
async fn a_discussion_refuses_a_roster_whose_redactors_disagree() {
    // 要擦掉的值是流上每个参与者共用的一份（spec §20），
    // 与 token 额度一样：不一致意味着某个发言者的事件被擦过、
    // 另一个的没被擦，而界面上什么都看不出来。
    let dir = tempfile::tempdir().unwrap();
    let other = Redactor::new(["sk-other-SECRET-987654321".to_owned()]);
    let debater = |speaker: &str, redactor: Redactor| DebaterParts {
        speaker: SpeakerId::Debater(speaker.into()),
        config: SessionConfig::new("fake-model").with_redactor(redactor),
        provider: Box::new(FakeProvider::new(vec![Reply::text("hi")])),
        soul: None,
    };
    let assembled = assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd: dir.path().to_path_buf(),
            log_path: dir.path().join("log.jsonl"),
            session_id: SessionId::new("s-credentials"),
            tools: builtin(false),
            locks: PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
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
        Ok(_) => panic!("对打码器意见不一致的名册必须被拒"),
        Err(error) => error,
    };
    assert!(
        matches!(&error, heng::Error::Discussion(message) if message.contains("打码器")),
        "得到 {error:?}"
    );
}

#[tokio::test]
async fn a_command_echo_is_redacted_before_it_enters_the_stream() {
    // spec §20 里的暴露路径 (b)：命令自己的输出。`bash` 是
    // `Exclusive`，所以在 `ask` 模式下由注入的应答者批准这次调用。
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

    // 命令拿的是真值：它打印的那个文件没被动过。
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
        .expect(".env 那次读拿到了结果");
    assert!(!env.1, ".env 这一族被策略拒了：{env:?}");
    assert!(env.2.contains(".env"), "{:?}", env.2);

    // 密钥自己的家在工作区之外，所以收纳规则在任何工具
    // 跑起来之前就拒了它（spec §20）。
    let outside = results
        .iter()
        .find(|(id, ..)| id == "call-2")
        .expect("工作区之外那次读拿到了结果");
    assert!(!outside.1, "{outside:?}");
    assert!(outside.2.contains("在会话工作区之外"), "{:?}", outside.2);

    // 两次试图读的东西一点都没进流：.env 文件里真正的
    // 密钥从没机会变成一条事件。
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
    let refusal = heng::cli::root_refusal(0).expect("root 被拒");
    assert_eq!(
        refusal,
        heng::render::wording::root_refusal(),
        "这条拒绝就是措辞层的文本"
    );

    assert_eq!(heng::cli::root_refusal(1), None);
    assert_eq!(heng::cli::root_refusal(1000), None);
}

#[test]
fn the_configured_provider_key_becomes_the_sessions_redactor() {
    // 从「config.toml 里的一个密钥」到「流会擦掉的一个值」这条接线
    // 走在配置变成注入值的唯一那一处
    // （`Config::session_config`），于是没有哪条组装路径得记住它。
    let file = r#"
default_model = "deepseek-v4-pro"

[providers.deepseek]
api_key = "sk-deepseek-config-key"

[models.deepseek-v4-pro]
provider = "deepseek"
"#;
    let config = heng::config::resolve(Some(file), &heng::config::EnvMap::new()).unwrap();
    let session = config.session_config("deepseek-v4-pro").unwrap();
    assert!(!session.redactor.is_empty());
    assert_eq!(
        session.redactor.redacted("x sk-deepseek-config-key y"),
        format!("x {REDACTED} y")
    );
}

#[test]
fn mcp_env_and_header_values_become_the_sessions_redactor() {
    // MCP 那两侧的凭据走同一条路（`.scratch/mcp-support/spec.md` §5）：
    // server 进程的 `env` 与远端请求的 `headers` 都是「配置里的秘密」，
    // 所以它们与 provider 密钥一样由 `Config::session_config` 交给打码器。
    let file = r#"
default_model = "deepseek-v4-pro"

[providers.deepseek]
api_key = "sk-deepseek-config-key"

[models.deepseek-v4-pro]
provider = "deepseek"

[mcp]
enabled = true

[mcp.servers.github]
command = ["server-github"]
env = { GITHUB_TOKEN = "ghp-mcp-secret-0123456789" }

[mcp.servers.jira]
url = "https://jira.example.com/mcp"
headers = { Authorization = "jira-mcp-secret-0123456789" }
"#;
    let config = heng::config::resolve(Some(file), &heng::config::EnvMap::new()).unwrap();
    let session = config.session_config("deepseek-v4-pro").unwrap();
    for secret in ["ghp-mcp-secret-0123456789", "jira-mcp-secret-0123456789"] {
        assert_eq!(
            session.redactor.redacted(&format!("x {secret} y")),
            format!("x {REDACTED} y"),
            "{secret} 该进打码器"
        );
    }
}
