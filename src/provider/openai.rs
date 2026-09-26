//! OpenAI 兼容客户端：一套实现，两个厂商档案。
//!
//! KIMI 与 DeepSeek 不是两个 provider，而是关于同一套请求与响应形状的两组事实。这个模块是那些差
//! 异每一处的执行点（spec §4）：
//!
//! * `tool_call` 的参数碎片（含 `index`）在这里拼好，所以上面那层永远看不到碎片；
//! * 两家厂商的 `usage` 形状都归一成 `cached` / `miss`；
//! * 明确设了、模型却不支持的参数是**带着告警丢掉**，绝不悄悄丢；
//! * 六个 `ProviderError` 类别在这里指派，其中 `QuotaExhausted` 与 `RateLimited` 保持分开；
//! * 传输层重试有上界、且住在这里；上面那层从不重跑一个回合。
//!
//! 厂商特有的行为被隔离在值进值出的函数里 —— [`build_body`]、[`StreamDecoder`]、
//! [`normalize_usage`]、[`classify_status`]、[`retry_delay`] —— 所以它们用录下来的 chunk 形状
//! 测试，不需要网络。

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

/// 适配器告警的去处。注入，所以库从不假定某个去处。
pub type WarningSink = Arc<dyn Fn(&str) + Send + Sync>;

/// 把告警丢在地上。用于测试和自带去处的嵌入方。
pub fn silent_warnings() -> WarningSink {
    Arc::new(|_| {})
}

/// 把告警送到 stderr，带一个稳定的前缀。CLI 注入的是这个。
pub fn stderr_warnings() -> WarningSink {
    Arc::new(|message| eprintln!("fs-agent: warning: {message}"))
}

/// 有上界的传输重试策略。重试按每次 `send` 调用计数，且刻意很小：上层不重跑一个回合。
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

/// 第 `attempt + 1` 次尝试之前的延迟；错误绝不能重试时是 `None`。`attempt` 是刚刚失败那次尝试的
/// 序号（从 1 起）。
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

/// 某个模型真实客户端。由已解析的配置构建。
pub struct OpenAiProvider {
    http: reqwest::Client,
    profile: ProviderProfile,
    model: String,
    caps: ModelCaps,
    retry: RetryPolicy,
    warnings: WarningSink,
}

impl OpenAiProvider {
    /// 为某个模型 id 构建客户端，用默认的重试策略。
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

    /// 给 401/403 补上那条可读的跨厂商诊断。
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

    /// 厂商拒掉凭据时该查什么。Kimi 的两套系统是常见的坑，所以这里点名写出来。
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
    fn caps(&self) -> ModelCaps {
        self.caps
    }

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

/// 某个 base URL 的 `/chat/completions` 端点，那个 URL 可能带、也可能不带 `/v1` 后缀。
pub fn chat_completions_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

/// 把 [`ChatRequest`] 渲染成线上 body，滤掉模型不支持的参数，并为每一个被丢掉或被夹紧的值返回一
/// 条告警。
///
/// `user_id` 刻意从不发送（spec §4）：它存在的意义是厂商侧的身份，发出去只会白白漏掉一个稳定的
/// 句柄。
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
        // 这是 harness 设的，不是用户设的，所以不支持的模型直接省略它而不再给告警：前缀缓存本质上
        // 是尽力而为。
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
        Message::User { content, name, .. } => {
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
            // 带工具的请求一旦丢掉自己的推理，DeepSeek 直接 400；而 Kimi K3 跨回合保留推理；无论
            // 哪种，它都必须往返。
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

// --- SSE 解码与工具调用拼装 ----------------------------------

/// 一条响应的流式累加器：字节进，完成单元出。
///
/// 它只持有在飞的字节与当前这条请求的部分工具调用 —— 这是传输状态，不是会话状态；`Session` 仍是
/// 唯一持有会话状态的值。同一批字节总是产出同样的单元，这正是测试直接驱动它的原因。
///
/// 用 [`StreamDecoder::push`] 喂它原始响应 chunk；字节流结束时调 [`StreamDecoder::finish`]。在
/// `data: [DONE]` 到来之前，不会有任何东西以完成态发出，所以被截断的流永远不会看起来已经结束。
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

    /// 消费一个响应 chunk，返回其中变得完整的部分。
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

    /// 字节流结束了。派发最后一个没带空行分隔符就到达的帧；否则什么都不发，于是一个从没见过
    /// `[DONE]` 的流不会产出任何完成单元。
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
        // 在一个 delta 里推理总是排在内容之前（Kimi 记下的顺序）；保持这个顺序，渲染器好先显示思
        // 考。
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
            let call = self.tools.get_mut(&index).expect("刚插入的");
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

/// 把 SSE 帧的字节流变成完成单元。
///
/// 一个网络 chunk 可能装着整整一批帧，而解码器把它们全排进队列；随后流在同一口气里把它们交回来，
/// 中间没有 await 点。这样把一批排干而从不 yield 会让 runtime 上其他任务挨饿 —— 尤其是渲染器，它
/// 的通道是有界的 —— 所以这个泵会定期 yield。
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
            since_yield: 0,
        },
        |mut state| async move {
            loop {
                if let Some(event) = state.queue.pop_front() {
                    state.since_yield += 1;
                    if state.since_yield >= YIELD_EVERY {
                        state.since_yield = 0;
                        tokio::task::yield_now().await;
                    }
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

/// 泵向 runtime yield 之前可以交出多少个已解码事件。小到一次突发跑不过一个被唤醒的消费者（渲染
/// 通道容量 1024），大到这个 yield 不是每个 token 一次。
const YIELD_EVERY: u32 = 16;

struct DecoderState<S> {
    bytes: Pin<Box<S>>,
    decoder: StreamDecoder,
    queue: VecDeque<StreamEvent>,
    ended: bool,
    /// 上一次向 runtime yield 以来交出的事件数。
    since_yield: u32,
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

// --- 用量与错误的归一化 ----------------------------------------

/// 把厂商的 `usage` 对象归一成中性的形状。
///
/// Kimi 报的是 `cached_tokens`；DeepSeek 报的是 `prompt_cache_hit_tokens` 与
/// `prompt_cache_miss_tokens`。两者都变成 `cached_tokens` / `miss_tokens`。
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

/// 把一个 HTTP 状态码加错误正文映射到六个类别之一。
///
/// 厂商对状态码的说法不一致，所以在要紧的地方由正文来细化状态码：Kimi Code 把套餐限制报成 403
/// （配额窗口与并发请求上限），DeepSeek 把余额为零报成 402，而两家都可能用 429 表示一次短暂的限
/// 流。
pub fn classify_status(status: u16, body: &str, retry_after: Option<Duration>) -> ProviderError {
    let detail = error_detail(body);
    match status {
        401 => ProviderError::Auth { detail },
        402 => ProviderError::QuotaExhausted { detail },
        403 => {
            if looks_like_concurrency(&detail) {
                // 并发请求上限在在飞的请求结束后就会放开，所以它的行为像限流。
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

/// 解析一个 `Retry-After` 响应头值。只认 delta-seconds 那种形式。
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

/// 阻止 provider 被构建出来的失败。运行期失败走 [`ProviderError`]。
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
