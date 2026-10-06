//! 连接层：`rmcp` 那一侧（`.scratch/mcp-support/spec.md` §3；票 12）。
//!
//! 这一层只做四件事：**建传输**（stdio 子进程 / Streamable HTTP）、**握手**（固定
//! `ClientLifecycleMode::Discover`，只谈 2026-07-28、遇到旧版 server 直接报错不回退）、
//! **把协议结果翻译成服务层的形状**、以及**把协议错误翻成 [`McpError`]**。
//!
//! 三条纪律写在这里：
//!
//! * **连接归会话**：`[mcp] enabled` 打开时这些连接在组装期建、随会话活。关闭走 `Drop` 兜底
//!   （`close` 要 `&mut self`，而 `Tool` 只给 `&self`）；**不自动重连** —— server 崩了之后同一场
//!   会话里不再重试，重开会话才重连（与 web 的失败语义同一条线）。
//! * **子进程整组清理**：`rmcp` 的默认清理只保证直接子进程被杀，而 heng 的纪律是
//!   `process_group(0)` + `killpg` 杀整棵树（`bash` 那一侧已经这样做了）。所以 stdio 传输在
//!   `CommandWrap` 上叠了 process-wrap 的 `ProcessGroup::leader()`。
//! * **argv 先过沙箱**：与 `bash` 走的是同一个纯函数 [`Sandbox::wrap`]（票 13 再补环境白名单
//!   与可写根）。沙箱不可用时连接直接失败，而不是绕过它去裸跑一个进程。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::{FuturesUnordered, StreamExt};
use http::{HeaderName, HeaderValue};
use process_wrap::tokio::{CommandWrap, ProcessGroup};
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, ContentBlock, ElicitRequestParams,
    ElicitResult, ElicitationAction, ElicitationCapability, ElicitationSchema, ErrorCode,
    ErrorData, FormElicitationCapability, GetPromptRequestParams, Implementation, ProtocolVersion,
    ReadResourceRequestParams, ResourceContents,
};
use rmcp::service::{RequestContext, RoleClient, RunningService, ServiceError};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{ClientHandler, ClientLifecycleMode, ClientServiceExt};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::config::{McpServerConfig, McpSettings, McpTransport, SandboxSettings};
use crate::questions::{UserAnswers, UserQuestion, UserQuestions};
use crate::tools::sandbox::Sandbox;

use super::{
    McpConnection, McpError, McpService, PromptArgument, PromptSummary, ResourceSummary,
    ServerManifest, ToolSummary,
};

/// 组装期把配置里每一台 server 连起来：**并发起、失败的跳过**（spec §3）。
///
/// 每台各自一个 `connect_timeout_ms` 的超时；失败的那台不挂连接、只记下原因，于是它的元工具
/// 调用报出来的是一条**结构化错误 + 人话**（`MCP_SERVER_UNAVAILABLE：…`），而其余几台照常
/// 工作。一个坏 server 不会拖垮整场会话。
///
/// 不做 server 数量上限 —— 写进配置的就是人要的；重名在配置解析那一层就已经是启动错误。
pub async fn connect_all(settings: &McpSettings, options: &ConnectOptions<'_>) -> McpService {
    let mut service = McpService::new(settings.clone());
    if !settings.enabled || settings.servers.is_empty() {
        return service;
    }

    let timeout = Duration::from_millis(settings.connect_timeout_ms);
    let mut pending = FuturesUnordered::new();
    for (name, config) in &settings.servers {
        let name = name.clone();
        pending.push(async move {
            let outcome = tokio::time::timeout(timeout, RunClient::connect(config, options)).await;
            (name, outcome)
        });
    }

    while let Some((name, outcome)) = pending.next().await {
        service = match outcome {
            Ok(Ok(client)) => service.with_connection(name, Arc::new(client)),
            Ok(Err(error)) => service.with_unavailable(name, error.message().to_owned()),
            Err(_) => service.with_unavailable(
                name,
                format!("{} 毫秒内没有连上", settings.connect_timeout_ms),
            ),
        };
    }
    service
}

/// 建连接时需要的那几件会话事实。
///
/// 装成一个值，而不是给 [`connect_all`] 排一串参数：票 15 还要往里加问询端口，接口稳定一点好。
pub struct ConnectOptions<'a> {
    /// 会话工作目录（沙箱的工作区，也是相对路径的锚）。
    pub cwd: &'a Path,
    /// 这台机器上的沙箱。每台 server 的 `writable_roots` 会作为这一次的额外可写根叠上去。
    pub sandbox: &'a Sandbox,
    /// 进程环境快照。连接层**只**从里面取白名单那三个键（见 [`BASE_ENV_KEYS`]）。
    pub base_env: &'a BTreeMap<String, String>,
    /// 会话 home，给 `~` 展开用。
    pub home: Option<&'a Path>,
    /// server 的 stderr 一行行去哪儿。`None` 就是丢掉 —— `rmcp` 默认 `inherit`，那会让 server
    /// 的崩溃信息直接落进终端。
    pub stderr: Option<StderrSink>,
    /// 把 server 的输入请求交给用户的那条端口（票 15）。无头组装是 `None`，那时 elicitation
    /// 如实回 `decline`。
    pub questions: Option<Arc<dyn UserQuestions>>,
}

impl<'a> ConnectOptions<'a> {
    /// 会话必须的那三件。`home` 与 stderr 用下面的 setter 补。
    pub fn new(
        cwd: &'a Path,
        sandbox: &'a Sandbox,
        base_env: &'a BTreeMap<String, String>,
    ) -> Self {
        Self {
            cwd,
            sandbox,
            base_env,
            home: None,
            stderr: None,
            questions: None,
        }
    }

    pub fn with_home(mut self, home: Option<&'a Path>) -> Self {
        self.home = home;
        self
    }

    pub fn with_stderr(mut self, sink: StderrSink) -> Self {
        self.stderr = Some(sink);
        self
    }

    pub fn with_questions(mut self, questions: Option<Arc<dyn UserQuestions>>) -> Self {
        self.questions = questions;
        self
    }
}

/// server 的 stderr 一行行去哪儿。
pub type StderrSink = Arc<dyn Fn(&str) + Send + Sync>;

/// 「最小必需」的那三个环境变量。
///
/// **白名单，不是黑名单**：父进程导出的其余一切（密钥、代理、凭据）都不进 server 进程。黑名单
/// fail open，而沙箱这一层的调性是 fail closed（ADR 0006）—— 同一个道理。
pub const BASE_ENV_KEYS: [&str; 3] = ["PATH", "HOME", "LANG"];

/// 一条已经握过手的连接。
pub struct RunClient {
    server: String,
    service: RunningService<RoleClient, HengHandler>,
}

impl RunClient {
    /// 连一台 server 并完成握手。
    ///
    /// 生命周期固定 [`ClientLifecycleMode::Discover`]：只发 `server/discover`、**不回退**到旧版的
    /// `initialize` —— 这正是「只谈 2026-07-28」的实现，而且是零配置达成的。
    pub async fn connect(
        config: &McpServerConfig,
        options: &ConnectOptions<'_>,
    ) -> Result<Self, McpError> {
        let handler = HengHandler::new(options.questions.clone());
        let service = match config.transport {
            McpTransport::Stdio => {
                let transport = stdio_transport(config, options)?;
                handler.serve_with_lifecycle(transport, discover()).await
            }
            McpTransport::Http => {
                let transport = http_transport(config)?;
                handler.serve_with_lifecycle(transport, discover()).await
            }
        }
        .map_err(|error| McpError::server_unavailable(&config.name, &error.to_string()))?;

        Ok(Self {
            server: config.name.clone(),
            service,
        })
    }

    /// server 在握手时自报的 `instructions`（`DiscoverResult` 里的那一段）。
    ///
    /// 它是**外部文本**：进 `mcp_list` 的结果、带不可信标记，绝不进系统提示词。
    fn instructions(&self) -> Option<String> {
        self.service
            .peer()
            .peer_info()
            .and_then(|info| info.instructions.clone())
    }
}

/// `Discover` 生命周期，协议版本只认 2026-07-28。
fn discover() -> ClientLifecycleMode {
    ClientLifecycleMode::Discover {
        preferred_versions: vec![ProtocolVersion::LATEST],
    }
}

/// 这个 client 的握手身份与回调。
///
/// 它覆写 `create_elicitation`，把 server 要人补的输入交给**既有的**问询端口
/// （`.scratch/mcp-support/spec.md` §3；票 15）—— 不新开第四类发起者。端口是组装期握进来的
/// 那个 `Arc`，与 `ask_user_question` 用的是**同一个**值。
#[derive(Clone)]
struct HengHandler {
    info: ClientConfig,
    questions: Option<Arc<dyn UserQuestions>>,
}

impl HengHandler {
    fn new(questions: Option<Arc<dyn UserQuestions>>) -> Self {
        let mut capabilities = ClientCapabilities::default();
        // 声明 elicitation 能力：server 看到它之后才会发 `input_required`（`Discover` 模式下
        // 这些能力随每条请求的 `_meta` 送出）。
        capabilities.elicitation = Some(
            ElicitationCapability::new()
                .with_form(FormElicitationCapability::new().with_schema_validation(true)),
        );
        Self {
            info: ClientConfig::new(
                capabilities,
                Implementation::new("heng", env!("CARGO_PKG_VERSION")),
            ),
            questions,
        }
    }
}

impl ClientHandler for HengHandler {
    fn get_info(&self) -> ClientConfig {
        self.info.clone()
    }

    /// server 要人补一个输入时，把问题摆到既有的问询通道上，把答案编成 `inputResponses` 交回
    /// 给 `rmcp` 的重试回路。
    ///
    /// 三条降级都是**如实**的，而不是编一个答案：没有端口（无头）＝ `decline`；端口报错
    /// （输入结束、这次运行被取消）＝ `cancel`；有人答了但有必填项空着 ＝ `decline`。
    /// URL 模式的 elicitation 不在这一版的范围（票 15 的「不做」），同样回 `decline`。
    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, ErrorData> {
        let ElicitRequestParams::FormElicitationParams {
            message,
            requested_schema,
            ..
        } = request
        else {
            return Ok(ElicitResult::new(ElicitationAction::Decline));
        };
        let Some(port) = self.questions.as_ref() else {
            return Ok(ElicitResult::new(ElicitationAction::Decline));
        };

        let questions = elicitation_questions(&message, &requested_schema);
        if questions.is_empty() {
            return Ok(ElicitResult::new(ElicitationAction::Decline));
        }

        match port.ask(&questions).await {
            Ok(answers) => match elicitation_content(&questions, &answers) {
                Some(content) => {
                    Ok(ElicitResult::new(ElicitationAction::Accept).with_content(content))
                }
                None => Ok(ElicitResult::new(ElicitationAction::Decline)),
            },
            Err(_) => Ok(ElicitResult::new(ElicitationAction::Cancel)),
        }
    }
}

/// form 模式的 schema 翻成几道题：**一个属性一道题**（属性名就是题号，答案原样按属性名回填）。
///
/// 这一版只把它当自由文本题：`UserQuestion` 的选项面是「几个 label 里挑」，而 elicitation 的
/// 属性是带类型的字段，硬把两者对上会在数字、布尔上失真。答案由用户打字给。
fn elicitation_questions(message: &str, schema: &ElicitationSchema) -> Vec<UserQuestion> {
    schema
        .properties
        .keys()
        .map(|name| UserQuestion {
            id: name.clone(),
            question: name.clone(),
            header: Some(message.to_owned()),
            options: Vec::new(),
            multi_select: false,
        })
        .collect()
}

/// 答案编成 `ElicitResult.content`：`{属性名: 值}`。任何一道题没答案就给 `None`（＝ `decline`）。
fn elicitation_content(questions: &[UserQuestion], answers: &UserAnswers) -> Option<Value> {
    let mut content = serde_json::Map::new();
    for question in questions {
        let answer = answers
            .answers
            .iter()
            .find(|answer| answer.id == question.id)?;
        let value = answer
            .custom
            .clone()
            .or_else(|| answer.selected.first().cloned())?;
        content.insert(question.id.clone(), Value::String(value));
    }
    Some(Value::Object(content))
}

/// stdio 传输：argv 先过 [`Sandbox::wrap`]，再交给 `rmcp` 起进程。
///
/// 这一处同时是**三件进程纪律**的落点（spec §4）：Sandbox 包装（这台 server 的
/// `writable_roots` 追加进去）、**环境白名单**（清空之后只注入最小必需 + 配置声明的项）、
/// 以及显式 piped 的 stderr。
fn stdio_transport(
    config: &McpServerConfig,
    options: &ConnectOptions<'_>,
) -> Result<TokioChildProcess, McpError> {
    // 每台 server 的 `writable_roots` 是**这一次连接**的额外可写根。它们与升级批准共用同一条
    // 通道（`with_grants`）：沙箱只能绑已经存在的路径，所以一条不存在的声明会明确失败，而不是
    // 静默不生效。
    let roots: Vec<PathBuf> = config
        .writable_roots
        .iter()
        .map(|root| crate::config::expand_home(root, options.home))
        .collect();
    // `sandbox = false` 是**显式**声明：这台 server 不过 bwrap（缺省永远过）。它只影响包不包
    // 这一层，`writable_roots` 照样算出来 —— 不过沙箱时那份声明自然无事发生。
    let base = if config.sandbox {
        options.sandbox.clone()
    } else {
        Sandbox::new(&SandboxSettings::off())
    };
    let sandbox = base.with_grants(&roots);

    let argv = sandbox
        .wrap(&config.command, options.cwd)
        .map_err(|error| McpError::server_unavailable(&config.name, &error.to_string()))?;
    let Some((program, args)) = argv.split_first() else {
        return Err(McpError::server_unavailable(
            &config.name,
            "`command` 是空的",
        ));
    };

    let mut command = Command::new(program);
    command.args(args);
    // **环境白名单**：先清空，再只注入两样 —— 白名单那三个键（会话环境里有才有），以及配置里
    // 显式声明的项。清洗必须发生在交给 `rmcp` 之前：它自己不做。
    command.env_clear();
    for key in BASE_ENV_KEYS {
        if let Some(value) = options.base_env.get(key).filter(|value| !value.is_empty()) {
            command.env(key, value);
        }
    }
    command.envs(&config.env);
    // 整组清理：`ProcessGroup::leader()` 把子进程放进它自己的进程组，`process-wrap` 的 kill
    // 走的就是 `killpg` —— 与 `bash` 那一侧同一条纪律。
    let mut wrap = CommandWrap::from(command);
    wrap.wrap(ProcessGroup::leader());

    // stderr 显式 piped：`rmcp` 的默认是 `inherit`，那会把 server 的崩溃信息直接写进终端
    // （TUI 底下就是它）。接住之后一行行交给诊断口。
    //
    // **不管有没有诊断口，都要把这条管道读掉**：读端一关，server 往 stderr 写就会拿到 EPIPE，
    // 而 `eprintln!` 在写失败时是 panic —— 一个只是想抱怨一句的 server 会因此当场死掉。
    let (child, stderr) = TokioChildProcess::builder(wrap)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            McpError::server_unavailable(&config.name, &format!("起不了 server 进程：{error}"))
        })?;
    if let Some(stderr) = stderr {
        let sink = options.stderr.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(sink) = &sink {
                    sink(&line);
                }
            }
        });
    }
    Ok(child)
}

/// Streamable HTTP 传输：只做远端形态，**不做** legacy HTTP+SSE（`rmcp` 也不提供）。
fn http_transport(
    config: &McpServerConfig,
) -> Result<StreamableHttpClientTransport<reqwest::Client>, McpError> {
    let url = config.url.clone().ok_or_else(|| {
        McpError::server_unavailable(&config.name, "这台是 http，但 `url` 是空的")
    })?;

    let mut headers = HashMap::new();
    for (name, value) in &config.headers {
        let header = HeaderName::try_from(name.as_str()).map_err(|error| {
            McpError::server_unavailable(
                &config.name,
                &format!("请求头名 `{name}` 不合法：{error}"),
            )
        })?;
        let value = HeaderValue::try_from(value.as_str()).map_err(|error| {
            McpError::server_unavailable(
                &config.name,
                &format!("请求头 `{name}` 的值不合法：{error}"),
            )
        })?;
        headers.insert(header, value);
    }

    let settings = StreamableHttpClientTransportConfig::with_uri(url).custom_headers(headers);
    Ok(StreamableHttpClientTransport::from_config(settings))
}

#[async_trait]
impl McpConnection for RunClient {
    async fn list_tools(&self) -> Result<ServerManifest, McpError> {
        let result = self
            .service
            .peer()
            .list_tools(None)
            .await
            .map_err(|error| self.failed("列工具", error))?;

        Ok(ServerManifest {
            server: self.server.clone(),
            instructions: self.instructions(),
            tools: result.tools.into_iter().map(tool_summary).collect(),
        })
    }

    async fn call_tool(&self, tool: &str, arguments: Value) -> Result<String, McpError> {
        let mut params = CallToolRequestParams::new(tool.to_owned());
        if let Some(object) = arguments.as_object() {
            params = params.with_arguments(object.clone());
        }

        // `RunningService::call_tool`（不是 `peer()` 上那个）**自动驱动 MRTR**：server 回
        // `input_required` 时它会把请求交给本地 `ClientHandler`，替我们把结果拼回去重试。
        let result = self
            .service
            .call_tool(params)
            .await
            .map_err(|error| match error {
                ServiceError::McpError(data) if data.code == ErrorCode::METHOD_NOT_FOUND => {
                    McpError::unknown_tool(&self.server, tool)
                }
                other => self.failed(tool, other),
            })?;

        let text = flatten_content_blocks(&result.content);
        if result.is_error == Some(true) {
            return Err(McpError::tool_error(&self.server, tool, text));
        }
        Ok(text)
    }

    async fn list_resources(&self) -> Result<Vec<ResourceSummary>, McpError> {
        let result = self
            .service
            .peer()
            .list_resources(None)
            .await
            .map_err(|error| self.failed("列资源", error))?;

        Ok(result
            .resources
            .into_iter()
            .map(|resource| ResourceSummary {
                uri: resource.uri,
                name: Some(resource.name),
                description: resource.description,
                mime_type: resource.mime_type,
            })
            .collect())
    }

    async fn read_resource(&self, uri: &str) -> Result<String, McpError> {
        let result = self
            .service
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
            .map_err(|error| match error {
                // 两种码都认：`-32002` 是 RESOURCE_NOT_FOUND 那个常量，而 2026-07-28 这一版
                // 把「URI 不存在」放进了 `INVALID_PARAMS`（`read_resource` 的参数只有 uri）。
                ServiceError::McpError(data)
                    if data.code == ErrorCode::RESOURCE_NOT_FOUND
                        || data.code == ErrorCode::INVALID_PARAMS =>
                {
                    McpError::unknown_resource(&self.server, uri)
                }
                other => self.failed(uri, other),
            })?;
        Ok(flatten_resource_contents(&result.contents))
    }

    async fn list_prompts(&self) -> Result<Vec<PromptSummary>, McpError> {
        let result = self
            .service
            .peer()
            .list_prompts(None)
            .await
            .map_err(|error| self.failed("列模板", error))?;
        Ok(result.prompts.into_iter().map(prompt_summary).collect())
    }

    async fn get_prompt(&self, name: &str, arguments: Value) -> Result<String, McpError> {
        let mut params = GetPromptRequestParams::new(name.to_owned());
        if let Some(object) = arguments.as_object() {
            params.arguments = Some(object.clone());
        }
        // `RunningService::get_prompt`（不是 `peer()` 上那个）会自动驱动 MRTR，与 `call_tool`
        // 同一条规矩。
        let result = self
            .service
            .get_prompt(params)
            .await
            .map_err(|error| self.failed(name, error))?;
        Ok(result
            .messages
            .iter()
            .map(|message| flatten_content_blocks(std::slice::from_ref(&message.content)))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

impl RunClient {
    /// 把一次协议失败翻成 [`McpError`]。
    ///
    /// 三类要分开：**连接断了**是「不可用」（不自动重连），**server 回的 JSON-RPC error** 是
    /// 那一次调用失败，其余（协议错、超时、传输失败）都归「这一趟失败了」。
    fn failed(&self, what: &str, error: ServiceError) -> McpError {
        match error {
            ServiceError::McpError(data) => McpError::provider_error(
                &self.server,
                format!("{what}：server 回了 {} {}", data.code.0, data.message),
            ),
            ServiceError::TransportClosed => McpError::server_unavailable(
                &self.server,
                "连接已经断了；这一版不自动重连，重开会话才重连",
            ),
            other => McpError::provider_error(&self.server, format!("{what}：{other}")),
        }
    }
}

/// `rmcp` 的 `Tool` → 服务层那一份摘要。
///
/// **不读 `annotations`**：规范原文写着 `ToolAnnotations` 全是 hints，客户端不该据以做任何
/// 工具使用判断；这里连字段都不碰。
fn tool_summary(tool: rmcp::model::Tool) -> ToolSummary {
    ToolSummary {
        name: tool.name.into_owned(),
        description: tool.description.map(|text| text.into_owned()),
        schema: Value::Object((*tool.input_schema).clone()),
    }
}

/// `rmcp` 的 `Prompt` → 菜单需要的那一份摘要。
fn prompt_summary(prompt: rmcp::model::Prompt) -> PromptSummary {
    PromptSummary {
        name: prompt.name,
        description: prompt.description,
        arguments: prompt
            .arguments
            .unwrap_or_default()
            .into_iter()
            .map(|argument| PromptArgument {
                name: argument.name,
                description: argument.description,
                required: argument.required.unwrap_or(false),
            })
            .collect(),
    }
}

/// 把 `content` 数组拍成一段文本。
///
/// 这一版只呈现文本；图片、音频与别的内容块各留一句如实的中文说明，**不假装**它们是文本。
fn flatten_content_blocks(content: &[ContentBlock]) -> String {
    let parts: Vec<String> = content
        .iter()
        .map(|block| match block {
            ContentBlock::Text(text) => text.text.clone(),
            ContentBlock::Image(_) => "（server 回了一张图片；这一版只呈现文本）".to_owned(),
            ContentBlock::Audio(_) => "（server 回了一段音频；这一版只呈现文本）".to_owned(),
            ContentBlock::Resource(resource) => {
                flatten_resource_contents(std::slice::from_ref(&resource.resource))
            }
            ContentBlock::ResourceLink(link) => format!("（资源链接）：{}", link.uri),
            _ => "（server 回了一种这一版呈现不了的内容）".to_owned(),
        })
        .collect();
    parts.join("\n")
}

/// 资源正文拍成文本；二进制内容如实说清它是二进制，而不是把 base64 倒给模型。
fn flatten_resource_contents(contents: &[ResourceContents]) -> String {
    let parts: Vec<String> = contents
        .iter()
        .map(|content| match content {
            ResourceContents::TextResourceContents { text, .. } => text.clone(),
            ResourceContents::BlobResourceContents { blob, .. } => {
                format!("（二进制内容：{} 字节的 base64，这一版不展开）", blob.len())
            }
            _ => "（server 回了一种这一版呈现不了的资源内容）".to_owned(),
        })
        .collect();
    parts.join("\n")
}
