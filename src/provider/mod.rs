//! Provider 适配器边界。
//!
//! 这个 trait 只做流式，并且是 dyn 兼容的（借 `async-trait`），好让组装入口注入一个脚本化的
//! 假 provider。KIMI 与 DeepSeek 是同一个 OpenAI 兼容客户端的两个档案，不是两套实现。
//!
//! 这里之所以要 `dyn`，是为了让假 provider 可注入，而不是为了支持多家厂商。`projection` 是子
//! 模块、不是自己的一道边界：投影是 provider 一侧的事。

pub mod capability;
pub mod openai;
pub mod projection;

use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};

use crate::events::Usage;

use self::capability::ModelCaps;

/// 塑造请求的那些值住在 `config` 里（它们是配置），在这里重新导出，因为它们是 provider 请求
/// 的一部分。
pub use crate::config::{GenerationParams, ReasoningEffort};

/// 一条流式响应：只给已经完整的单元，工具调用的碎片已由适配器拼好。
pub type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, ProviderError>> + Send>>;

/// 一个 provider。每次调用都无状态、自包含：历史完整地在 [`ChatRequest`] 里重放，因为两家厂商
/// 都没有服务端会话原语。
#[async_trait]
pub trait Provider: Send + Sync {
    async fn send(&self, request: ChatRequest) -> Result<EventStream, ProviderError>;

    /// 这个客户端所对话的那个模型的逐字段事实。
    ///
    /// 投影读这些字段，而不是去问厂商名字，所以字段级的差异是 [`ModelCaps`] 上的数据，而不是
    /// [`projection::project`] 里的一个分支。
    fn caps(&self) -> ModelCaps;
}

/// 交给 provider 的、已经投影好的请求。
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub tool_choice: ToolChoice,
    pub params: GenerationParams,
    /// 用于前缀缓存的会话标识；模型不支持时是 `None`。
    pub cache_key: Option<String>,
}

/// 线上层的消息形状。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Message {
    System {
        content: String,
        name: Option<String>,
    },
    User {
        content: String,
        name: Option<String>,
        /// 这条消息是 harness 注入的内容（`ContextInjected`）还是某个发言者的话。它不属于线上
        /// 形状 —— 编码器忽略它 —— 但它正是 [`crate::context::trim`] 用来判定一条 `user` 消息
        /// 无论落在哪里都被钉住的东西，包括用户加载技能正文时在会话中途注入的那一条
        /// （spec §9、§10）。
        #[serde(default)]
        injected: bool,
    },
    Assistant {
        content: Option<String>,
        /// 在模型要求时必须重放（否则 DeepSeek 直接 400）。
        reasoning_content: Option<String>,
        tool_calls: Vec<ToolCall>,
        name: Option<String>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

/// 一条参数碎片已由适配器拼好的工具调用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 线上层的工具声明：按 provider 期望的形状给出的 JSON Schema。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolChoice {
    #[default]
    Auto,
    None,
    Required,
    Tool(String),
}

/// 流式响应里一个已完成的单元。
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallStarted {
        index: u32,
        id: String,
        name: String,
    },
    ToolCallCompleted {
        index: u32,
        id: String,
        name: String,
        arguments: String,
    },
    Usage(Usage),
    /// 那个 `[DONE]` 标记。`finish_reason` 只作诊断；循环从不把它当成停止信号。
    Finished {
        finish_reason: FinishReason,
    },
}

/// provider 自己的终局标签。只作诊断。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    InsufficientSystemResource,
    Aborted,
    Other(String),
}

/// 六个错误类别。`QuotaExhausted` 与 `RateLimited` 保持分开，因为厂商对它们的信号不同
/// （Kimi 429，DeepSeek 402）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("authentication failed: {detail}")]
    Auth { detail: String },
    #[error("quota exhausted: {detail}")]
    QuotaExhausted { detail: String },
    #[error("rate limited (retry_after: {retry_after:?})")]
    RateLimited { retry_after: Option<Duration> },
    #[error("invalid request: {detail}")]
    InvalidRequest { detail: String },
    #[error("transport error: {detail}")]
    Transport { detail: String },
    #[error("protocol error: {detail}")]
    Protocol { detail: String },
}
