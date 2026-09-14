//! Provider adapter boundary.
//!
//! The trait is streaming-only and dyn-compatible (via `async-trait`) so the
//! assembly entry can inject a scripted fake provider. KIMI and DeepSeek are two
//! profiles of one OpenAI-compatible client, not two implementations.
//!
//! `dyn` exists here to make the fake injectable, not to support many vendors.
//! `projection` is a submodule rather than its own boundary: projection is a
//! provider-side concern.

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

/// Request-shaping values live in `config` (they are configuration), and are
/// re-exported here because they are part of a provider request.
pub use crate::config::{GenerationParams, ReasoningEffort};

/// A streaming response: completed units only, with tool-call fragments already
/// assembled by the adapter.
pub type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, ProviderError>> + Send>>;

/// One provider. Every call is stateless and self-contained: history is fully
/// replayed in the [`ChatRequest`], because neither vendor has a server-side
/// session primitive.
#[async_trait]
pub trait Provider: Send + Sync {
    async fn send(&self, request: ChatRequest) -> Result<EventStream, ProviderError>;

    /// The field-level facts about the model this client speaks to.
    ///
    /// The projection reads these instead of asking the vendor's name, so a
    /// per-field difference is data on [`ModelCaps`] rather than a branch in
    /// [`projection::project`].
    fn caps(&self) -> ModelCaps;
}

/// The already-projected request handed to a provider.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub tool_choice: ToolChoice,
    pub params: GenerationParams,
    /// Session identifier for prompt-prefix caching; `None` when unsupported.
    pub cache_key: Option<String>,
}

/// Wire-level message shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Message {
    System {
        content: String,
        name: Option<String>,
    },
    User {
        content: String,
        name: Option<String>,
    },
    Assistant {
        content: Option<String>,
        /// Must be replayed when the model requires it (DeepSeek 400s otherwise).
        reasoning_content: Option<String>,
        tool_calls: Vec<ToolCall>,
        name: Option<String>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

/// A tool call whose argument fragments have been assembled by the adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Wire-level tool declaration: JSON Schema as the provider expects it.
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

/// One completed unit of a streaming response.
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
    /// The `[DONE]` marker. `finish_reason` is diagnostic only; the loop never
    /// uses it as a stop signal.
    Finished {
        finish_reason: FinishReason,
    },
}

/// The provider's own terminal label. Diagnostic only.
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

/// The six error classes. `QuotaExhausted` and `RateLimited` stay separate
/// because the vendors signal them differently (Kimi 429, DeepSeek 402).
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
