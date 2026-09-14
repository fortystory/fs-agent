//! The OpenAI-compatible client: one implementation, two vendor profiles.
//!
//! KIMI and DeepSeek are not two providers, they are two sets of facts about
//! the same request and response shape. This module is the execution point for
//! every one of those differences (spec §4):
//!
//! * `tool_call` argument fragments (including `index`) are assembled here, so
//!   the layer above never sees a fragment;
//! * both vendors' `usage` shapes are normalized to `cached` / `miss`;
//! * a parameter that was explicitly set but is not supported is **dropped with
//!   a warning**, never silently;
//! * the six `ProviderError` classes are assigned here, with `QuotaExhausted`
//!   and `RateLimited` kept apart;
//! * transport-level retries are bounded and live here; the layer above never
//!   re-runs a turn.
//!
//! The vendor-specific behaviour is isolated in value-in/value-out functions —
//! [`build_body`], [`StreamDecoder`], [`normalize_usage`], [`classify_status`],
//! [`retry_delay`] — so it is tested with recorded chunk shapes and no network.

use std::collections::{btree_map::Entry, BTreeMap, VecDeque};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::{self, Stream, StreamExt};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::capability::{caps_for, ModelCaps, UnknownModel};
use super::{
    ChatRequest, EventStream, FinishReason, Message, Provider, ProviderError, StreamEvent,
    ToolCall, ToolChoice, ToolSpec,
};
use crate::config::{Config, ConfigError, KeySource, ProviderProfile, Vendor};
use crate::events::Usage;

/// Where adapter warnings go. Injected, so the library never assumes a sink.
pub type WarningSink = Arc<dyn Fn(&str) + Send + Sync>;

/// Drop warnings on the floor. For tests and embedders that chose their own.
pub fn silent_warnings() -> WarningSink {
    Arc::new(|_| {})
}

/// Send warnings to stderr with a stable prefix. The CLI injects this.
pub fn stderr_warnings() -> WarningSink {
    Arc::new(|message| eprintln!("fs-agent: warning: {message}"))
}

/// Bounded transport retry policy. Retries are counted per `send` call and are
/// deliberately small: the upper layer does not re-run a turn.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(4),
        }
    }
}

impl RetryPolicy {
    fn backoff(&self, attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1).min(16);
        (self.base_delay * (1u32 << shift)).min(self.max_delay)
    }
}

/// The delay before attempt `attempt + 1`, or `None` when the error must not be
/// retried. `attempt` is the 1-based number of the attempt that just failed.
pub fn retry_delay(policy: RetryPolicy, attempt: u32, error: &ProviderError) -> Option<Duration> {
    if attempt >= policy.max_attempts {
        return None;
    }
    match error {
        ProviderError::RateLimited { retry_after } => Some(
            retry_after
                .unwrap_or_else(|| policy.backoff(attempt))
                .min(policy.max_delay),
        ),
        ProviderError::Transport { .. } => Some(policy.backoff(attempt)),
        _ => None,
    }
}

/// The real client for one model. Built from resolved configuration.
pub struct OpenAiProvider {
    http: reqwest::Client,
    profile: ProviderProfile,
    model: String,
    caps: ModelCaps,
    retry: RetryPolicy,
    warnings: WarningSink,
}

impl OpenAiProvider {
    /// Build the client for one model id, using the default retry policy.
    pub fn build(
        config: &Config,
        model_id: &str,
        warnings: WarningSink,
    ) -> Result<Self, BuildError> {
        let (model, profile) = config.resolve_model(Some(model_id))?;
        let caps = caps_for(&model.id)?;
        if profile.api_key.is_none() {
            return Err(BuildError::MissingKey {
                provider: profile.name.clone(),
                hint: profile.key_env.clone(),
            });
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| BuildError::HttpClient {
                provider: profile.name.clone(),
                detail: error.to_string(),
            })?;
        Ok(Self {
            http,
            profile: profile.clone(),
            model: model.id.clone(),
            caps,
            retry: RetryPolicy::default(),
            warnings,
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn caps(&self) -> ModelCaps {
        self.caps
    }

    pub fn profile(&self) -> &ProviderProfile {
        &self.profile
    }

    fn key_source_description(&self) -> String {
        match &self.profile.key_source {
            KeySource::Config => "config.toml".to_owned(),
            KeySource::Env(name) => format!("environment variable {name}"),
            KeySource::Missing => "nowhere".to_owned(),
        }
    }

    /// Add the readable cross-vendor diagnosis to a 401/403.
    fn with_auth_hint(&self, error: ProviderError) -> ProviderError {
        match error {
            ProviderError::Auth { detail } => ProviderError::Auth {
                detail: format!(
                    "{detail} (provider `{}`, base_url `{}`, key from {}). {}",
                    self.profile.name,
                    self.profile.base_url,
                    self.key_source_description(),
                    self.auth_hint()
                ),
            },
            other => other,
        }
    }

    /// What to check when the vendor rejects the credentials. Kimi's two
    /// systems are the common trap, so they are named explicitly.
    fn auth_hint(&self) -> &'static str {
        match self.profile.vendor {
            Some(Vendor::Kimi) => {
                "Kimi runs two separate systems: a Kimi Code (coding plan) `sk-kimi-` key goes to \
                 https://api.kimi.com/coding/v1, while a Kimi Open Platform key goes to \
                 https://api.moonshot.cn/v1. Keys and base URLs are not interchangeable, and a \
                 401 can also mean the plan does not include the requested model."
            }
            Some(Vendor::DeepSeek) => {
                "check that the key belongs to https://api.deepseek.com and is still active."
            }
            None => "check that the key and base_url belong together.",
        }
    }

    async fn post(&self, body: &Value) -> Result<reqwest::Response, ProviderError> {
        let url = chat_completions_url(&self.profile.base_url);
        let key = self.profile.api_key.as_deref().unwrap_or_default();
        let mut attempt: u32 = 1;
        loop {
            let result = self
                .http
                .post(&url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .bearer_auth(key)
                .json(body)
                .send()
                .await;

            match result {
                Ok(response) if response.status().is_success() => return Ok(response),
                Ok(response) => {
                    let status = response.status().as_u16();
                    let retry_after = parse_retry_after(
                        response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|value| value.to_str().ok()),
                    );
                    let text = response.text().await.unwrap_or_default();
                    let error = self.with_auth_hint(classify_status(status, &text, retry_after));
                    match retry_delay(self.retry, attempt, &error) {
                        Some(delay) => {
                            tokio::time::sleep(delay).await;
                            attempt += 1;
                        }
                        None => return Err(error),
                    }
                }
                Err(error) => {
                    let error = ProviderError::Transport {
                        detail: error.to_string(),
                    };
                    match retry_delay(self.retry, attempt, &error) {
                        Some(delay) => {
                            tokio::time::sleep(delay).await;
                            attempt += 1;
                        }
                        None => return Err(error),
                    }
                }
            }
        }
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    async fn send(&self, request: ChatRequest) -> Result<EventStream, ProviderError> {
        if request.model != self.model {
            return Err(ProviderError::InvalidRequest {
                detail: format!(
                    "this provider is bound to model `{}` but the request asked for `{}`",
                    self.model, request.model
                ),
            });
        }
        let (body, warnings) = build_body(&request, self.caps);
        for warning in &warnings {
            (self.warnings)(warning);
        }
        let response = self.post(&body).await?;
        Ok(Box::pin(sse_stream(response.bytes_stream(), self.caps)))
    }
}

/// The `/chat/completions` endpoint for a base URL that may or may not carry a
/// `/v1` suffix.
pub fn chat_completions_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

/// Render a [`ChatRequest`] as the wire body, filtering parameters the model
/// does not support and returning one warning per dropped or clamped value.
///
/// `user_id` is deliberately never sent (spec §4): it exists for vendor-side
/// identity and would leak a stable handle for no benefit.
pub fn build_body(request: &ChatRequest, caps: ModelCaps) -> (Value, Vec<String>) {
    let mut body = Map::new();
    let mut warnings = Vec::new();
    let model = request.model.clone();

    body.insert("model".to_owned(), json!(model));
    body.insert("messages".to_owned(), messages_json(&request.messages));
    body.insert("stream".to_owned(), json!(true));
    if caps.supports_stream_options {
        body.insert(
            "stream_options".to_owned(),
            json!({ "include_usage": true }),
        );
    }

    if !request.tools.is_empty() {
        if caps.supports_tools {
            body.insert("tools".to_owned(), tools_json(&request.tools));
            body.insert(
                "tool_choice".to_owned(),
                tool_choice_json(&request.tool_choice),
            );
        } else {
            warnings.push(format!(
                "model `{model}` does not support tools; dropped {} tool declaration(s)",
                request.tools.len()
            ));
        }
    }

    let params = &request.params;
    if let Some(temperature) = params.temperature {
        if caps.supports_temperature {
            body.insert("temperature".to_owned(), json!(temperature));
        } else {
            warnings.push(format!(
                "model `{model}` fixes temperature; dropped the explicitly set temperature={temperature}"
            ));
        }
    }
    if let Some(top_p) = params.top_p {
        if caps.supports_top_p {
            body.insert("top_p".to_owned(), json!(top_p));
        } else {
            warnings.push(format!(
                "model `{model}` fixes top_p; dropped the explicitly set top_p={top_p}"
            ));
        }
    }
    if let Some(requested) = params.max_output_tokens {
        let effective = requested.min(caps.max_output_tokens);
        if effective < requested {
            warnings.push(format!(
                "model `{model}` caps output at {} tokens; clamped the explicitly set max_output_tokens={requested}",
                caps.max_output_tokens
            ));
        }
        body.insert(
            caps.max_tokens_field.field_name().to_owned(),
            json!(effective),
        );
    }
    if let Some(effort) = params.reasoning_effort {
        if caps.supports_reasoning_effort {
            body.insert("reasoning_effort".to_owned(), json!(effort.as_str()));
        } else {
            warnings.push(format!(
                "model `{model}` does not accept reasoning_effort; dropped the explicitly set `{}`",
                effort.as_str()
            ));
        }
    }

    if let Some(cache_key) = &request.cache_key {
        // The harness sets this, not the user, so an unsupported model omits it
        // without a warning: prompt-prefix caching is best-effort by nature.
        if caps.supports_prompt_cache_key {
            body.insert("prompt_cache_key".to_owned(), json!(cache_key));
        }
    }

    (Value::Object(body), warnings)
}

fn messages_json(messages: &[Message]) -> Value {
    Value::Array(messages.iter().map(message_json).collect())
}

fn message_json(message: &Message) -> Value {
    let mut object = Map::new();
    match message {
        Message::System { content, name } => {
            object.insert("role".to_owned(), json!("system"));
            object.insert("content".to_owned(), json!(content));
            insert_name(&mut object, name);
        }
        Message::User { content, name } => {
            object.insert("role".to_owned(), json!("user"));
            object.insert("content".to_owned(), json!(content));
            insert_name(&mut object, name);
        }
        Message::Assistant {
            content,
            reasoning_content,
            tool_calls,
            name,
        } => {
            object.insert("role".to_owned(), json!("assistant"));
            object.insert(
                "content".to_owned(),
                json!(content.clone().unwrap_or_default()),
            );
            // DeepSeek 400s when a tools request drops its own reasoning, and
            // Kimi K3 keeps reasoning across turns; either way it must round-trip.
            if let Some(reasoning) = reasoning_content {
                object.insert("reasoning_content".to_owned(), json!(reasoning));
            }
            if !tool_calls.is_empty() {
                object.insert(
                    "tool_calls".to_owned(),
                    Value::Array(tool_calls.iter().map(tool_call_json).collect()),
                );
            }
            insert_name(&mut object, name);
        }
        Message::Tool {
            tool_call_id,
            content,
        } => {
            object.insert("role".to_owned(), json!("tool"));
            object.insert("tool_call_id".to_owned(), json!(tool_call_id));
            object.insert("content".to_owned(), json!(content));
        }
    }
    Value::Object(object)
}

fn insert_name(object: &mut Map<String, Value>, name: &Option<String>) {
    if let Some(name) = name {
        if !name.is_empty() {
            object.insert("name".to_owned(), json!(name));
        }
    }
}

fn tool_call_json(call: &ToolCall) -> Value {
    json!({
        "id": call.id,
        "type": "function",
        "function": { "name": call.name, "arguments": call.arguments },
    })
}

fn tools_json(tools: &[ToolSpec]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    },
                })
            })
            .collect(),
    )
}

fn tool_choice_json(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::None => json!("none"),
        ToolChoice::Required => json!("required"),
        ToolChoice::Tool(name) => json!({ "type": "function", "function": { "name": name } }),
    }
}

// --- SSE decoding and tool-call assembly ----------------------------------

/// A streaming accumulator for one response: bytes in, completed units out.
///
/// It holds only the bytes in flight and the partial tool calls of the request
/// currently streaming — this is transport state, not session state; `Session`
/// remains the only value that holds session state. The same bytes always yield
/// the same units, which is why tests drive it directly.
///
/// Feed it raw response chunks with [`StreamDecoder::push`]; when the byte
/// stream ends, call [`StreamDecoder::finish`]. Nothing is emitted as complete
/// until `data: [DONE]` arrives, so a truncated stream never looks finished.
pub struct StreamDecoder {
    caps: ModelCaps,
    buffer: Vec<u8>,
    tools: BTreeMap<u32, PartialToolCall>,
    usage: Option<Usage>,
    finish_reason: Option<FinishReason>,
    terminated: bool,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl StreamDecoder {
    pub fn new(caps: ModelCaps) -> Self {
        Self {
            caps,
            buffer: Vec::new(),
            tools: BTreeMap::new(),
            usage: None,
            finish_reason: None,
            terminated: false,
        }
    }

    /// Consume one response chunk and return whatever became complete.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<StreamEvent>, ProviderError> {
        let mut events = Vec::new();
        if self.terminated {
            return Ok(events);
        }
        self.buffer.extend_from_slice(chunk);
        while let Some((end, separator)) = find_frame_end(&self.buffer) {
            let frame: Vec<u8> = self.buffer.drain(..end).collect();
            self.buffer.drain(..separator);
            let frame = String::from_utf8(frame).map_err(|error| ProviderError::Protocol {
                detail: format!("SSE frame is not valid UTF-8: {error}"),
            })?;
            self.handle_frame(&frame, &mut events)?;
            if self.terminated {
                break;
            }
        }
        Ok(events)
    }

    /// The byte stream ended. Dispatch a trailing frame that arrived without its
    /// blank-line separator; otherwise emit nothing, so a stream that never saw
    /// `[DONE]` produces no completed unit.
    pub fn finish(&mut self) -> Result<Vec<StreamEvent>, ProviderError> {
        let mut events = Vec::new();
        if self.terminated || self.buffer.is_empty() {
            return Ok(events);
        }
        let remainder = std::mem::take(&mut self.buffer);
        let frame = String::from_utf8(remainder).map_err(|error| ProviderError::Protocol {
            detail: format!("SSE frame is not valid UTF-8: {error}"),
        })?;
        self.handle_frame(&frame, &mut events)?;
        Ok(events)
    }

    fn handle_frame(
        &mut self,
        frame: &str,
        events: &mut Vec<StreamEvent>,
    ) -> Result<(), ProviderError> {
        let mut data_lines: Vec<&str> = Vec::new();
        for line in frame.lines() {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
            }
        }
        if data_lines.is_empty() {
            return Ok(());
        }
        let payload = data_lines.join("\n");
        if payload.trim() == "[DONE]" {
            self.terminate(events);
            return Ok(());
        }
        let chunk: Chunk =
            serde_json::from_str(&payload).map_err(|error| ProviderError::Protocol {
                detail: format!("malformed streaming chunk: {error}: {payload}"),
            })?;
        self.handle_chunk(chunk, events);
        Ok(())
    }

    fn handle_chunk(&mut self, chunk: Chunk, events: &mut Vec<StreamEvent>) {
        if let Some(usage) = chunk.usage.as_ref().filter(|usage| !usage.is_null()) {
            self.usage = Some(normalize_usage(self.caps.vendor, usage));
        }
        let Some(choice) = chunk.choices.and_then(|choices| choices.into_iter().next()) else {
            return;
        };
        if let Some(usage) = choice.usage.as_ref().filter(|usage| !usage.is_null()) {
            self.usage = Some(normalize_usage(self.caps.vendor, usage));
        }
        if let Some(reason) = choice.finish_reason.as_deref() {
            self.finish_reason = Some(parse_finish_reason(reason));
        }
        let Some(delta) = choice.delta else {
            return;
        };
        // Reasoning always precedes content in a delta (Kimi's documented
        // order); keep that order so the renderer shows thinking first.
        if let Some(reasoning) = delta.reasoning_content.filter(|text| !text.is_empty()) {
            events.push(StreamEvent::ReasoningDelta(reasoning));
        }
        if let Some(content) = delta.content.filter(|content| !content.is_empty()) {
            events.push(StreamEvent::TextDelta(content));
        }
        for fragment in delta.tool_calls.into_iter().flatten() {
            let index = fragment.index.unwrap_or(0);
            let id = fragment.id.clone().unwrap_or_default();
            let name = fragment
                .function
                .as_ref()
                .and_then(|function| function.name.clone())
                .unwrap_or_default();
            if let Entry::Vacant(entry) = self.tools.entry(index) {
                entry.insert(PartialToolCall::default());
                events.push(StreamEvent::ToolCallStarted {
                    index,
                    id: id.clone(),
                    name: name.clone(),
                });
            }
            let call = self.tools.get_mut(&index).expect("just inserted");
            if call.id.is_empty() {
                call.id = id;
            }
            if call.name.is_empty() {
                call.name = name;
            }
            if let Some(function) = fragment.function {
                if let Some(arguments) = function.arguments {
                    call.arguments.push_str(&arguments);
                }
            }
        }
    }

    fn terminate(&mut self, events: &mut Vec<StreamEvent>) {
        self.terminated = true;
        for (index, call) in std::mem::take(&mut self.tools) {
            events.push(StreamEvent::ToolCallCompleted {
                index,
                id: call.id,
                name: call.name,
                arguments: call.arguments,
            });
        }
        if let Some(usage) = self.usage {
            events.push(StreamEvent::Usage(usage));
        }
        events.push(StreamEvent::Finished {
            finish_reason: self.finish_reason.clone().unwrap_or(FinishReason::Stop),
        });
    }
}

/// Turn a byte stream of SSE frames into completed units.
pub fn sse_stream<S, B>(
    byte_stream: S,
    caps: ModelCaps,
) -> impl Stream<Item = Result<StreamEvent, ProviderError>> + Send
where
    S: Stream<Item = Result<B, reqwest::Error>> + Send + 'static,
    B: AsRef<[u8]> + Send + 'static,
{
    stream::unfold(
        DecoderState {
            bytes: Box::pin(byte_stream),
            decoder: StreamDecoder::new(caps),
            queue: VecDeque::new(),
            ended: false,
        },
        |mut state| async move {
            loop {
                if let Some(event) = state.queue.pop_front() {
                    return Some((Ok(event), state));
                }
                if state.ended {
                    return None;
                }
                match state.bytes.next().await {
                    Some(Ok(chunk)) => match state.decoder.push(chunk.as_ref()) {
                        Ok(events) => state.queue.extend(events),
                        Err(error) => {
                            state.ended = true;
                            return Some((Err(error), state));
                        }
                    },
                    Some(Err(error)) => {
                        state.ended = true;
                        return Some((
                            Err(ProviderError::Transport {
                                detail: error.to_string(),
                            }),
                            state,
                        ));
                    }
                    None => {
                        state.ended = true;
                        match state.decoder.finish() {
                            Ok(events) => state.queue.extend(events),
                            Err(error) => return Some((Err(error), state)),
                        }
                    }
                }
            }
        },
    )
}

struct DecoderState<S> {
    bytes: Pin<Box<S>>,
    decoder: StreamDecoder,
    queue: VecDeque<StreamEvent>,
    ended: bool,
}

fn find_frame_end(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = find_subslice(buffer, b"\n\n").map(|index| (index, 2));
    let crlf = find_subslice(buffer, b"\r\n\r\n").map(|index| (index, 4));
    match (lf, crlf) {
        (Some(lf), Some(crlf)) => Some(if lf.0 <= crlf.0 { lf } else { crlf }),
        (Some(lf), None) => Some(lf),
        (None, Some(crlf)) => Some(crlf),
        (None, None) => None,
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[derive(Debug, Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Option<Vec<ChunkChoice>>,
    #[serde(default)]
    usage: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ChunkChoice {
    #[serde(default)]
    delta: Option<ChunkDelta>,
    #[serde(default)]
    finish_reason: Option<String>,
    #[serde(default)]
    usage: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ChunkDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallFragment>>,
}

#[derive(Debug, Deserialize)]
struct ToolCallFragment {
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionFragment>,
}

#[derive(Debug, Deserialize)]
struct FunctionFragment {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

fn parse_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "tool_calls" => FinishReason::ToolCalls,
        "content_filter" => FinishReason::ContentFilter,
        "insufficient_system_resource" => FinishReason::InsufficientSystemResource,
        "aborted" => FinishReason::Aborted,
        other => FinishReason::Other(other.to_owned()),
    }
}

// --- usage and error normalization ----------------------------------------

/// Normalize a vendor `usage` object into the neutral shape.
///
/// Kimi reports `cached_tokens`; DeepSeek reports `prompt_cache_hit_tokens` and
/// `prompt_cache_miss_tokens`. Both become `cached_tokens` / `miss_tokens`.
pub fn normalize_usage(vendor: Vendor, usage: &Value) -> Usage {
    let input_tokens = u64_field(usage, "prompt_tokens").unwrap_or(0);
    let output_tokens = u64_field(usage, "completion_tokens").unwrap_or(0);
    let (cached_tokens, miss_tokens) = match vendor {
        Vendor::Kimi => {
            let cached = u64_field(usage, "cached_tokens")
                .or_else(|| nested_u64(usage, "prompt_tokens_details", "cached_tokens"))
                .unwrap_or(0);
            (cached, input_tokens.saturating_sub(cached))
        }
        Vendor::DeepSeek => {
            let cached = u64_field(usage, "prompt_cache_hit_tokens")
                .or_else(|| nested_u64(usage, "prompt_tokens_details", "cached_tokens"))
                .unwrap_or(0);
            let miss = u64_field(usage, "prompt_cache_miss_tokens")
                .unwrap_or_else(|| input_tokens.saturating_sub(cached));
            (cached, miss)
        }
    };
    Usage {
        input_tokens,
        output_tokens,
        cached_tokens,
        miss_tokens,
        reasoning_tokens: nested_u64(usage, "completion_tokens_details", "reasoning_tokens"),
    }
}

fn u64_field(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn nested_u64(value: &Value, outer: &str, inner: &str) -> Option<u64> {
    value
        .get(outer)
        .and_then(|nested| nested.get(inner))
        .and_then(Value::as_u64)
}

/// Map an HTTP status plus error body onto one of the six classes.
///
/// The vendors disagree on codes, so the body refines the status where it
/// matters: Kimi Code reports plan limits as 403 (quota windows and the
/// concurrent-request cap) and DeepSeek reports an empty balance as 402, while
/// both can use 429 for a transient rate limit.
pub fn classify_status(status: u16, body: &str, retry_after: Option<Duration>) -> ProviderError {
    let detail = error_detail(body);
    match status {
        401 => ProviderError::Auth { detail },
        402 => ProviderError::QuotaExhausted { detail },
        403 => {
            if looks_like_concurrency(&detail) {
                // The concurrent-request cap clears once in-flight requests
                // finish, so it behaves like a rate limit.
                ProviderError::RateLimited { retry_after }
            } else if looks_like_quota(&detail) {
                ProviderError::QuotaExhausted { detail }
            } else {
                ProviderError::Auth { detail }
            }
        }
        429 => {
            if looks_like_quota(&detail) {
                ProviderError::QuotaExhausted { detail }
            } else {
                ProviderError::RateLimited { retry_after }
            }
        }
        400 | 404 | 409 | 422 => ProviderError::InvalidRequest { detail },
        408 | 425 => ProviderError::Transport { detail },
        status if (500..=599).contains(&status) => ProviderError::Transport { detail },
        _ => ProviderError::Protocol { detail },
    }
}

fn looks_like_quota(detail: &str) -> bool {
    let lowered = detail.to_ascii_lowercase();
    [
        "insufficient",
        "balance",
        "quota",
        "billing",
        "arrears",
        "usage limit",
        "欠费",
        "余额",
        "额度",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn looks_like_concurrency(detail: &str) -> bool {
    let lowered = detail.to_ascii_lowercase();
    lowered.contains("concurrent") || lowered.contains("too many requests")
}

fn error_detail(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
            return message.to_owned();
        }
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            return message.to_owned();
        }
        if let Some(code) = value.pointer("/error/code").and_then(Value::as_str) {
            return code.to_owned();
        }
    }
    let trimmed = body.trim();
    if trimmed.is_empty() {
        "empty error body".to_owned()
    } else {
        trimmed.chars().take(500).collect()
    }
}

/// Parse a `Retry-After` header value. Only the delta-seconds form is honored.
pub fn parse_retry_after(value: Option<&str>) -> Option<Duration> {
    let raw = value?.trim();
    if raw.is_empty() {
        return None;
    }
    let seconds = raw.parse::<f64>().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some(Duration::from_secs_f64(seconds.min(600.0)))
}

/// Failures that stop a provider from being built. Runtime failures are
/// [`ProviderError`] instead.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    UnknownModel(#[from] UnknownModel),
    #[error(
        "provider `{provider}` has no API key: set `api_key` in config.toml, or export {hint}"
    )]
    MissingKey { provider: String, hint: String },
    #[error("provider `{provider}`: could not build the HTTP client: {detail}")]
    HttpClient { provider: String, detail: String },
}
