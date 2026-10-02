//! MRTR：server 要人补一个输入时，问题走**既有的**问询端口，答案拼回 `inputResponses` 重试
//! （`.scratch/mcp-support/spec.md` §3、§8；票 15）。
//!
//! 走真 stdio + 真协议帧：假 server 第一次回 `resultType: "input_required"`，第二次（带着
//! `inputResponses`）才给最终结果。回路是 `rmcp` 的 `RunningService::call_tool` 驱动的，我们只
//! 提供那个 `ClientHandler`。

mod support;

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_agent::config::{
    McpServerConfig, McpSettings, SandboxAvailability, SandboxMode, SandboxSettings,
};
use fs_agent::mcp::{connect_all, ConnectOptions, McpService};
use fs_agent::questions::{UserAnswer, UserAnswers, UserQuestion, UserQuestions};
use fs_agent::tools::Sandbox;

// --- 假的问询端口 ---------------------------------------------------------

/// 按脚本作答、并记下每一道题的端口。就是 `ask_user_question` 那条端口。
#[derive(Clone, Default)]
struct FakeQuestions {
    answers: Arc<Mutex<VecDeque<UserAnswers>>>,
    asked: Arc<Mutex<Vec<Vec<UserQuestion>>>>,
    /// 为真时报一条「这一次运行被取消了」，用来验取消的语义。
    cancelled: bool,
}

impl FakeQuestions {
    fn answering(answers: Vec<UserAnswers>) -> Self {
        Self {
            answers: Arc::new(Mutex::new(answers.into())),
            asked: Arc::new(Mutex::new(Vec::new())),
            cancelled: false,
        }
    }

    fn cancelled() -> Self {
        Self {
            cancelled: true,
            ..Self::default()
        }
    }

    fn asked(&self) -> Vec<Vec<UserQuestion>> {
        self.asked.lock().expect("端口已中毒").clone()
    }
}

#[async_trait]
impl UserQuestions for FakeQuestions {
    async fn ask(&self, questions: &[UserQuestion]) -> Result<UserAnswers, String> {
        self.asked
            .lock()
            .expect("端口已中毒")
            .push(questions.to_vec());
        if self.cancelled {
            return Err("这一次运行被取消了".to_owned());
        }
        Ok(self
            .answers
            .lock()
            .expect("端口已中毒")
            .pop_front()
            .expect("假端口：脚本里的答案已经用完了"))
    }
}

fn answer(id: &str, text: &str) -> UserAnswers {
    UserAnswers {
        answers: vec![UserAnswer {
            id: id.to_owned(),
            selected: Vec::new(),
            custom: Some(text.to_owned()),
        }],
    }
}

// --- 组装 -----------------------------------------------------------------

fn fake_server() -> &'static str {
    env!("CARGO_BIN_EXE_fake-mcp-server")
}

fn settings(server: McpServerConfig) -> McpSettings {
    McpSettings {
        enabled: true,
        connect_timeout_ms: 10_000,
        servers: BTreeMap::from([(server.name.clone(), server)]),
    }
}

fn sandbox_for(cwd: &Path) -> Option<Sandbox> {
    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    settings.search_path = std::env::var_os("PATH");
    settings.availability = fs_agent::tools::sandbox::probe(settings.search_path.as_deref(), cwd);
    if matches!(
        settings.availability,
        SandboxAvailability::Unavailable { .. }
    ) {
        return None;
    }
    Some(Sandbox::new(&settings))
}

macro_rules! skip_without_bwrap {
    ($cwd:expr) => {
        match sandbox_for($cwd) {
            Some(sandbox) => sandbox,
            None => {
                eprintln!("跳过：这台机器上没有可用的 bwrap");
                return;
            }
        }
    };
}

fn workspace() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&cwd).unwrap();
    (dir, cwd)
}

fn base_env() -> BTreeMap<String, String> {
    ["PATH", "HOME", "LANG"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)))
        .collect()
}

async fn connect(
    server: McpServerConfig,
    cwd: &Path,
    sandbox: &Sandbox,
    questions: Option<Arc<dyn UserQuestions>>,
) -> McpService {
    let env = base_env();
    let options = ConnectOptions::new(cwd, sandbox, &env).with_questions(questions);
    connect_all(&settings(server), &options).await
}

// --- 往返 -----------------------------------------------------------------

#[tokio::test]
async fn a_request_for_input_reaches_the_port_and_the_answer_goes_back() {
    let (_dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);
    let port = FakeQuestions::answering(vec![answer("answer", "42")]);

    let service = connect(
        McpServerConfig::stdio("fake", vec![fake_server().to_owned()]),
        &cwd,
        &sandbox,
        Some(Arc::new(port.clone())),
    )
    .await;
    assert_eq!(service.unavailable_reason("fake"), None);

    let text = service
        .call_tool("fake", "ask", serde_json::json!({}))
        .await
        .expect("MRTR 回路该把这一笔走完");
    assert!(
        text.contains("补好了：42"),
        "答案被拼回 `inputResponses` 并重试了一次：{text}"
    );
    assert!(
        text.contains("state=state-1"),
        "`requestState` 原样回传：{text}"
    );

    let asked = port.asked();
    assert_eq!(asked.len(), 1, "只问了一次");
    assert_eq!(asked[0].len(), 1, "schema 里有一个属性就是一道题");
    assert_eq!(asked[0][0].id, "answer", "题号就是属性名");
    assert_eq!(
        asked[0][0].header.as_deref(),
        Some("这一笔需要你补一个值"),
        "server 的话摆在题头上"
    );
}

#[tokio::test]
async fn a_cancelled_port_turns_into_a_decline_and_a_readable_failure() {
    let (_dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);
    let port = FakeQuestions::cancelled();

    let service = connect(
        McpServerConfig::stdio("fake", vec![fake_server().to_owned()]),
        &cwd,
        &sandbox,
        Some(Arc::new(port.clone())),
    )
    .await;
    assert_eq!(service.unavailable_reason("fake"), None);

    let error = service
        .call_tool("fake", "ask", serde_json::json!({}))
        .await
        .expect_err("没有答案就没有结果");
    assert_eq!(error.code_str(), "MCP_TOOL_ERROR");
    assert!(
        error.message().contains("用户没有给这个输入"),
        "server 拒绝之后的话要带出来：{error}"
    );
    assert_eq!(port.asked().len(), 1, "问题确实摆出去过");
}

#[tokio::test]
async fn the_client_declares_the_elicitation_capability() {
    let (_dir, cwd) = workspace();
    let sandbox = skip_without_bwrap!(&cwd);
    let capabilities = cwd.join("capabilities.json");
    let mut server = McpServerConfig::stdio("fake", vec![fake_server().to_owned()]);
    server.env.insert(
        "FAKE_MCP_CAPABILITIES_FILE".to_owned(),
        capabilities.display().to_string(),
    );

    let service = connect(server, &cwd, &sandbox, None).await;
    assert_eq!(service.unavailable_reason("fake"), None);

    let declared = std::fs::read_to_string(&capabilities).expect("假 server 该把它写下来");
    assert!(
        declared.contains("elicitation"),
        "握手报文里要有 elicitation，否则 server 根本不会发 `input_required`：{declared}"
    );
}
