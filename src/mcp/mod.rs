//! MCP 接入的三层里中间的那一层（`.scratch/mcp-support/spec.md` §1）。
//!
//! 依赖单向向下：`tools → mcp → rmcp`。三层的分工与 `src/web/` 同构：
//!
//! * **工具层**（`src/tools/mcp_*.rs`）拥有面向模型的约定：四个固定名字、schema、参数校验、
//!   结果形状、不可信标记。它绝不问「连接可用吗」，也绝不自己枚举 server。
//! * **服务层**（这里）拥有「名字 → 连接」的解析与错误码：未知 server、连接缺失、协议失败都
//!   在这里变成一条带 code 的 [`McpError`]，由工具层渲染成模型可读的句子。
//! * **连接层**是 `rmcp`（票 12 落地）：协议帧、生命周期、传输。服务层是唯一碰它的地方。
//!
//! 一条边界写在这里，免得日后被读成疏漏：**server 的工具永远不进工具表**。表里只有那四个固定
//! 名字，server 侧增删工具只影响 `mcp_list` 的**返回** —— 这正是「工具表是缓存前缀的一部分」
//! 与「只有连上才知道 server 有哪些工具」之间那个撞点的绕法。
//!
//! 连接是**会话级**的：`enabled` 打开时在组装期建、随会话活；关闭走 Drop 兜底，不自动重连
//! （server 崩了会在 `mcp_list` 里报成结构化错误，重开会话才重连）。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;

use crate::config::McpSettings;

pub mod rmcp_client;

pub use rmcp_client::{BASE_ENV_KEYS, ConnectOptions, RunClient, StderrSink, connect_all};

/// 一台 server 声明的一个提示词模板（`.scratch/mcp-support/spec.md` §8）。
///
/// 模板**由人发起**：它只出现在 `/` 菜单里，不进工具表、也不进前缀缓存 —— 模型看不到它。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSummary {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Vec<PromptArgument>,
}

/// 模板要的一个参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptArgument {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
}

/// 一次模板清单询问的结论：与 [`ServerListing`] 同构。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptListing {
    pub server: String,
    pub result: Result<Vec<PromptSummary>, McpError>,
}

/// 一条 server 声明的工具，摘出模型需要的那几样。
///
/// `schema` 是 server 给的原样 JSON Schema：我们只**读**它来写摘要，绝不据此做权限判断
/// —— 规范明文写着 `ToolAnnotations` 只是 hints，这条同样适用于参数 schema 里的任何自述。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSummary {
    pub name: String,
    pub description: Option<String>,
    pub schema: serde_json::Value,
}

/// 一份资源，摘出模型需要的那几样。资源是 URI 坐标系里的东西，与工作区路径互不操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSummary {
    pub uri: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

/// 一台 server 在一次清单询问里报回来的东西。
#[derive(Debug, Clone, PartialEq)]
pub struct ServerManifest {
    /// server 自己报的名字（诊断用；对外一律以配置里那个键为准）。
    pub server: String,
    /// server 的自述。它是**外部文本**：进 `mcp_list` 的结果、带不可信标记，绝不进系统提示词。
    pub instructions: Option<String>,
    pub tools: Vec<ToolSummary>,
}

/// 一次清单询问的结论：每台 server 一条，失败的那台也占一条。
///
/// 失败**不下沉成整个调用失败**：一台坏 server 不该让另外几台的清单读不到（spec §3 的
/// 「并发起、失败的跳过」）。未知 server 名才是整个调用的错。
#[derive(Debug, Clone, PartialEq)]
pub struct ServerListing {
    pub server: String,
    pub result: Result<ServerManifest, McpError>,
}

/// 一次资源清单询问的结论：与 [`ServerListing`] 同构，只是那台报的是资源而不是工具。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceListing {
    pub server: String,
    pub result: Result<Vec<ResourceSummary>, McpError>,
}

/// 一次 MCP 调用为什么失败。
///
/// 与 [`WebError`](crate::web::WebError) 同一形状：`code` 是 schema 值那一类、保持英文，给人
/// 读的句子由工具层拼。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpError {
    code: McpErrorCode,
    message: String,
}

/// 错误码。字符串形式见 [`McpErrorCode::as_str`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpErrorCode {
    /// 配置里没有这个名字的 server。
    UnknownServer,
    /// 名字在配置里，但这个会话没建起它的连接。
    ServerUnavailable,
    /// 连接层自己失败了：协议、传输、生命周期。
    ProviderError,
    /// server 那侧把这次调用报成失败。
    ProviderToolError,
    /// server 说它没有这个工具。
    UnknownTool,
    /// server 说它没有这个 URI 的资源。
    UnknownResource,
    /// 这一层还没有实现这类原语。
    Unsupported,
}

impl McpErrorCode {
    /// 线上的那一串。**保持英文**：它是 schema 值，不是散文（ADR 0005）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnknownServer => "MCP_UNKNOWN_SERVER",
            Self::ServerUnavailable => "MCP_SERVER_UNAVAILABLE",
            Self::ProviderError => "MCP_PROVIDER_ERROR",
            Self::ProviderToolError => "MCP_TOOL_ERROR",
            Self::UnknownTool => "MCP_UNKNOWN_TOOL",
            Self::UnknownResource => "MCP_UNKNOWN_RESOURCE",
            Self::Unsupported => "MCP_UNSUPPORTED",
        }
    }
}

impl McpError {
    pub fn new(code: McpErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 配置里没有这台 server。消息里点名是哪个键 —— 修法可以直接抄。
    pub fn unknown_server(server: &str) -> Self {
        Self::new(
            McpErrorCode::UnknownServer,
            format!(
                "没有配置名叫 `{server}` 的 server —— 检查 `[mcp.servers.{server}]`，\
                 或仓库根的 `.mcp.json` 里的 `mcpServers.{server}`"
            ),
        )
    }

    /// 名字在配置里，但连接没建起来。`reason` 是连接层给的那一句话。
    pub fn server_unavailable(server: &str, reason: &str) -> Self {
        Self::new(
            McpErrorCode::ServerUnavailable,
            format!("server `{server}` 不可用：{reason}"),
        )
    }

    /// 连接层自己失败了。
    pub fn provider_error(server: &str, message: impl Into<String>) -> Self {
        Self::new(
            McpErrorCode::ProviderError,
            format!("server `{server}` 这一趟失败了：{}", message.into()),
        )
    }

    /// server 把这次调用报成失败。
    pub fn tool_error(server: &str, tool: &str, message: impl Into<String>) -> Self {
        Self::new(
            McpErrorCode::ProviderToolError,
            format!("server `{server}` 的 `{tool}` 没有成功：{}", message.into()),
        )
    }

    /// server 说它没有这个工具。
    pub fn unknown_tool(server: &str, tool: &str) -> Self {
        Self::new(
            McpErrorCode::UnknownTool,
            format!("server `{server}` 没有名叫 `{tool}` 的工具；先 `mcp_list` 看它有什么"),
        )
    }

    /// server 说它没有这个 URI 的资源。
    pub fn unknown_resource(server: &str, uri: &str) -> Self {
        Self::new(
            McpErrorCode::UnknownResource,
            format!("server `{server}` 没有 `{uri}` 这份资源；先 `mcp_resources` 看它有什么"),
        )
    }

    /// 这一层还没有实现这类原语。
    pub fn unsupported(what: &str) -> Self {
        Self::new(
            McpErrorCode::Unsupported,
            format!("这个会话还没接上{what}这一类原语"),
        )
    }

    pub fn code(&self) -> McpErrorCode {
        self.code
    }

    pub fn code_str(&self) -> &'static str {
        self.code.as_str()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for McpError {
    /// `CODE：一句话` —— code 英文、句子中文，合起来是模型看到的那一行。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}：{}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for McpError {}

/// 服务层与连接层之间那条唯一的缝。
///
/// 真实现是 `rmcp`（票 12），测试给的是进程内假连接 —— 于是工具层与服务层的行为可以在零进程、
/// 零网络下钉死，而协议帧那几件事由真 stdio 集成测试覆盖。
///
/// 四个方法对应四类原语里我们支持的四种动作。后三个带默认实现，为的是让测试里的**假连接**
/// 只写自己关心的那一个；真连接（[`rmcp_client::RunClient`]）四个都实现。
#[async_trait]
pub trait McpConnection: Send + Sync {
    /// 问一次工具清单。**每次现问**：不订阅 `toolsListChanged`、不做会话内缓存。
    async fn list_tools(&self) -> Result<ServerManifest, McpError>;

    /// 调一个工具，`arguments` 原样透传（schema 在 server 那侧，我们不校验）。
    async fn call_tool(
        &self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        let _ = (tool, arguments);
        Err(McpError::unsupported("调用工具"))
    }

    /// 问一次资源清单。
    async fn list_resources(&self) -> Result<Vec<ResourceSummary>, McpError> {
        Err(McpError::unsupported("列资源"))
    }

    /// 按 URI 读一份资源。
    async fn read_resource(&self, uri: &str) -> Result<String, McpError> {
        let _ = uri;
        Err(McpError::unsupported("读资源"))
    }

    /// 问一次提示词模板清单（票 17）。模板只去 `/` 菜单，模型看不到它们。
    async fn list_prompts(&self) -> Result<Vec<PromptSummary>, McpError> {
        Err(McpError::unsupported("列模板"))
    }

    /// 取一份模板渲染出来的文本（`prompts/get`）。
    async fn get_prompt(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        let _ = (name, arguments);
        Err(McpError::unsupported("取模板"))
    }
}

/// 四个元工具唯一的执行路径。
///
/// 组装期先建它、再挂连接：一台连接都没挂时它照样是一个可用的服务 —— 调用返回
/// [`McpErrorCode::ServerUnavailable`]，而**工具仍在表里**。工具表不随连接状态抖动。
pub struct McpService {
    settings: McpSettings,
    connections: BTreeMap<String, Arc<dyn McpConnection>>,
    /// 建连接失败的那几台，以及一句人话原因。它的元工具调用报的是**结构化错误 + 这句话**，
    /// 而不是「没有连接」这种谁看了都不知道下一步做什么的消息。
    unavailable: BTreeMap<String, String>,
}

impl McpService {
    pub fn new(settings: McpSettings) -> Self {
        Self {
            settings,
            connections: BTreeMap::new(),
            unavailable: BTreeMap::new(),
        }
    }

    /// 挂上一台 server 的连接。真连接在组装期建（票 12），测试挂假连接。
    pub fn with_connection(
        mut self,
        server: impl Into<String>,
        connection: Arc<dyn McpConnection>,
    ) -> Self {
        let server = server.into();
        self.unavailable.remove(&server);
        self.connections.insert(server, connection);
        self
    }

    /// 记下一台**没连上**的 server 与原因（票 12 的「失败的跳过」）。
    pub fn with_unavailable(
        mut self,
        server: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        self.unavailable.insert(server.into(), reason.into());
        self
    }

    /// 这台 server 的不可用原因（如果有）。
    pub fn unavailable_reason(&self, server: &str) -> Option<&str> {
        self.unavailable.get(server).map(String::as_str)
    }

    /// 「这台没有连接」那条结构化错误，带上建连接时记下的原因。
    fn unavailable_error(&self, server: &str) -> McpError {
        match self.unavailable.get(server) {
            Some(reason) => McpError::server_unavailable(server, reason),
            None => McpError::server_unavailable(server, "这个会话没有建起它的连接"),
        }
    }

    /// 部署设置。工具层从这里读四个元工具在不在表里、以及那一串上限。
    pub fn settings(&self) -> &McpSettings {
        &self.settings
    }

    /// 配置里所有 server 的名字，按稳定顺序。
    pub fn server_names(&self) -> Vec<String> {
        self.settings.servers.keys().cloned().collect()
    }

    /// 这个名字在配置里吗。
    pub fn has_server(&self, server: &str) -> bool {
        self.settings.servers.contains_key(server)
    }

    /// 这台 server 的配置（连接层用它拿 argv、env、可写根与那三个信任位）。
    pub fn server(&self, name: &str) -> Option<&crate::config::McpServerConfig> {
        self.settings.servers.get(name)
    }

    /// 问一份（或全部）server 的工具清单。
    ///
    /// `server` 是 `Some` 时只问那一台，点名一个不在配置里的名字是整个调用的错
    /// （[`McpErrorCode::UnknownServer`]）；不传时逐台问，**每台一条结论**，失败的那台也占一条。
    pub async fn list_tools(&self, server: Option<&str>) -> Result<Vec<ServerListing>, McpError> {
        let names = match server {
            Some(name) => {
                if !self.has_server(name) {
                    return Err(McpError::unknown_server(name));
                }
                vec![name.to_owned()]
            }
            None => self.server_names(),
        };

        let mut listings = Vec::new();
        for name in names {
            let result = match self.connections.get(&name) {
                Some(connection) => connection.list_tools().await,
                None => Err(self.unavailable_error(&name)),
            };
            listings.push(ServerListing {
                server: name,
                result,
            });
        }
        Ok(listings)
    }

    /// 调一台 server 上的一个工具，`arguments` **原样透传**（schema 在 server 那侧，我们不校验）。
    ///
    /// 未知 server 是 [`McpErrorCode::UnknownServer`]；名字在配置里但没有连接是
    /// [`McpErrorCode::ServerUnavailable`]；server 把这次调用报成失败时，连接层给的是
    /// [`McpErrorCode::ProviderToolError`]。三种都是**一条可读结果**，不是整个会话的错。
    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        if !self.has_server(server) {
            return Err(McpError::unknown_server(server));
        }
        let Some(connection) = self.connections.get(server) else {
            return Err(self.unavailable_error(server));
        };
        connection.call_tool(tool, arguments).await
    }

    /// 问一份（或全部）server 的资源清单。形状与 [`McpService::list_tools`] 逐字同构。
    pub async fn list_resources(
        &self,
        server: Option<&str>,
    ) -> Result<Vec<ResourceListing>, McpError> {
        let names = match server {
            Some(name) => {
                if !self.has_server(name) {
                    return Err(McpError::unknown_server(name));
                }
                vec![name.to_owned()]
            }
            None => self.server_names(),
        };

        let mut listings = Vec::new();
        for name in names {
            let result = match self.connections.get(&name) {
                Some(connection) => connection.list_resources().await,
                None => Err(self.unavailable_error(&name)),
            };
            listings.push(ResourceListing {
                server: name,
                result,
            });
        }
        Ok(listings)
    }

    /// 按 URI 读一份资源，拿回它的正文。
    ///
    /// 资源是**另一套坐标系**里的东西：这里的 URI 由 server 定义，与工作区路径互不操作，读它
    /// 也不给任何工作区路径发读权限（spec §8）。
    pub async fn read_resource(&self, server: &str, uri: &str) -> Result<String, McpError> {
        if !self.has_server(server) {
            return Err(McpError::unknown_server(server));
        }
        let Some(connection) = self.connections.get(server) else {
            return Err(self.unavailable_error(server));
        };
        connection.read_resource(uri).await
    }

    /// 问一份（或全部）server 的模板清单。形状与 [`McpService::list_tools`] 逐字同构。
    pub async fn list_prompts(&self, server: Option<&str>) -> Result<Vec<PromptListing>, McpError> {
        let names = match server {
            Some(name) => {
                if !self.has_server(name) {
                    return Err(McpError::unknown_server(name));
                }
                vec![name.to_owned()]
            }
            None => self.server_names(),
        };

        let mut listings = Vec::new();
        for name in names {
            let result = match self.connections.get(&name) {
                Some(connection) => connection.list_prompts().await,
                None => Err(self.unavailable_error(&name)),
            };
            listings.push(PromptListing {
                server: name,
                result,
            });
        }
        Ok(listings)
    }

    /// 每台可用 server 的模板，拍平成一个列表；**失败的 server 直接跳过**。
    ///
    /// `/` 菜单用的就是这一条：server 不可用时它的模板条目**根本不出现**，而不是出现之后点开
    /// 报错（spec §8）。
    pub async fn prompt_entries(&self) -> Vec<(String, PromptSummary)> {
        let Ok(listings) = self.list_prompts(None).await else {
            return Vec::new();
        };
        listings
            .into_iter()
            .flat_map(|listing| match listing.result {
                Ok(prompts) => prompts
                    .into_iter()
                    .map(|prompt| (listing.server.clone(), prompt))
                    .collect::<Vec<_>>(),
                Err(_) => Vec::new(),
            })
            .collect()
    }

    /// 取一份模板渲染出来的文本。发起者是人，所以这里的错误是给人看的一句话。
    pub async fn get_prompt(
        &self,
        server: &str,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        if !self.has_server(server) {
            return Err(McpError::unknown_server(server));
        }
        let Some(connection) = self.connections.get(server) else {
            return Err(self.unavailable_error(server));
        };
        connection.get_prompt(name, arguments).await
    }

    /// 组装期这一层加载成什么样，写成一段给**人和模型**看的话（票 19）。
    ///
    /// 没开开关、或者一台 server 都没配时是 `None` —— 那种会话不注入这一条，零影响。
    /// **只列键名与条数，不列值**：`env` / `headers` 里的东西是秘密。
    pub fn catalog_text(&self) -> Option<String> {
        if !self.settings.enabled || self.settings.servers.is_empty() {
            return None;
        }
        let mut text = String::from("[注入] MCP 加载\n");
        for (name, config) in &self.settings.servers {
            text.push_str(&format!("\n- `{name}`（{}）：", config.transport.as_str()));
            match self.unavailable.get(name) {
                Some(reason) => text.push_str(&format!("连不上 —— {reason}\n")),
                None => {
                    text.push_str("已连接\n");
                    text.push_str(&format!(
                        "  沙箱：{}；结果标记：{}；副作用放宽：{}；可写根 {} 条\n",
                        if config.sandbox { "过" } else { "不过" },
                        if config.trust_results {
                            "不带"
                        } else {
                            "带"
                        },
                        if config.trust_effects {
                            format!("允许（只读名单 {} 条）", config.read_only_tools.len())
                        } else {
                            "不允许".to_owned()
                        },
                        config.writable_roots.len(),
                    ));
                }
            }
        }
        Some(text)
    }

    /// 这台 server 开了 `trust_results` 吗（spec §6）。
    ///
    /// 打开后它返回的**内容**不再带那句不可信标记。名字不认识时是 `false` —— 缺省永远是最严。
    pub fn trusts_results(&self, server: &str) -> bool {
        self.settings
            .servers
            .get(server)
            .is_some_and(|server| server.trust_results)
    }

    /// 这条工具按**只读**处理吗（spec §6 的 `trust_effects` + `read_only_tools`）。
    ///
    /// 两个条件缺一不可：这台 server 显式开了 `trust_effects`，且这条工具在**人写的**那份名单
    /// 里。名字不认识、名单为空、或位没开，都是 `false` —— 缺省永远是最严。
    ///
    /// server 自报的 `ToolAnnotations` 在这里一个字都不读：规范原文写着它们全是 hints。
    pub fn is_tool_read_only(&self, server: &str, tool: &str) -> bool {
        self.settings.servers.get(server).is_some_and(|server| {
            server.trust_effects && server.read_only_tools.iter().any(|name| name == tool)
        })
    }
}

impl fmt::Debug for McpService {
    /// 连接是不透明的句柄，一条诊断能说的有用的话只有「这台挂没挂」。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpService")
            .field("settings", &self.settings)
            .field(
                "connections",
                &self.connections.keys().collect::<Vec<&String>>(),
            )
            .finish()
    }
}
