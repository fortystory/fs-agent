//! DeepSeek 的搜索后端：Anthropic Messages 端点 + 原生 `web_search` 服务器工具
//! （`.scratch/web-search-tool/spec.md` §4）。
//!
//! 为什么是它：**零新密钥** —— 复用 `DEEPSEEK_API_KEY`。代价是一个完整的模型轮次（服务端把
//! 检索到的内容再总结一遍），延迟与费用都记在 `docs/web.md` 里。
//!
//! 一条硬规矩：**只取结构化块**。来源来自响应里的 `web_search_tool_result` →
//! `web_search_result` 条目，绝不从回复文本里抓 URL —— 文本是模型的散文，抓它等于把「模型
//! 说的」当成「互联网上有的」。响应里没有那个块就报错，不降级。

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{SearchProvider, Source, WebError};

/// 后端名，与配置里的 `search_provider` 对应。
pub const DEEPSEEK_SEARCH_PROVIDER: &str = "deepseek";

/// 搜索用的模型。官方现在的模型名是 `deepseek-flash`，`deepseek-v4-flash` 是仍在接受的
/// legacy 名；未知的模型名会被服务端自动映射到它。
pub const DEEPSEEK_SEARCH_MODEL: &str = "deepseek-flash";

/// 一次调用的输出上限。服务端的总结用不了更多。
const MAX_TOKENS: u64 = 4096;

/// 一次调用最多让服务端搜几次。
const MAX_USES: u32 = 5;

/// 服务端工具的声明串。
///
/// 取自 Anthropic Messages 的服务端工具定义：DeepSeek 的兼容表只列了**响应**块的
/// 支持状态（`server_tool_use` 与 `web_search_tool_result` 都 Supported），没有列请求里该写
/// 哪个 type 串。**真调用若被拒收，这里是第一个要看的地方** —— 它是票 02 留下的人工项。
const WEB_SEARCH_TOOL_TYPE: &str = "web_search_20250305";

/// 一次搜索的墙钟上限。
///
/// 比抓取那一档宽得多：一次搜索是一个完整的模型轮次（服务端可能连着搜几跳），而抓取只是一次
/// GET。它不进配置 —— 先跑一段看真实用量。
const SEARCH_TIMEOUT: Duration = Duration::from_secs(90);

/// 一次搜索的后端。
pub struct DeepSeekSearch {
    client: reqwest::Client,
    base_url: String,
    /// 密钥；`None` 表示这份配置里解析不到 —— 那时调用给出
    /// [`WebErrorCode::CredentialMissing`](super::WebErrorCode::CredentialMissing)，而不是让工具
    /// 从表里消失（spec §3）。
    api_key: Option<String>,
    model: String,
}

impl DeepSeekSearch {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        // 建不出 client 只可能是 TLS 后端初始化失败那一类；退回默认 client 而不是把整条组装
        // 路径变成 `Result` —— 真出问题的话，第一次调用会带着错误说出来。
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key,
            model: DEEPSEEK_SEARCH_MODEL.to_owned(),
        }
    }

    /// 换一个模型名（配置里没这一项，留给测试与以后的 profile）。
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// 这一步用的端点。
    pub fn endpoint(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }
}

#[async_trait]
impl SearchProvider for DeepSeekSearch {
    fn name(&self) -> &str {
        DEEPSEEK_SEARCH_PROVIDER
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Source>, WebError> {
        let Some(key) = self.api_key.as_deref() else {
            return Err(WebError::credential_missing(
                DEEPSEEK_SEARCH_PROVIDER,
                "DEEPSEEK_API_KEY",
            ));
        };
        let endpoint = self.endpoint();
        let body = request_body(query, &self.model);

        let response = self
            .client
            .post(&endpoint)
            .header("x-api-key", key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .timeout(SEARCH_TIMEOUT)
            .send()
            .await
            .map_err(|error| {
                // 端点配错是最常见的一种失败，所以消息里带上它。
                WebError::provider_error(DEEPSEEK_SEARCH_PROVIDER, format!("{endpoint}：{error}"))
            })?;

        let status = response.status();
        let text = response.text().await.map_err(|error| {
            WebError::provider_error(DEEPSEEK_SEARCH_PROVIDER, format!("{endpoint}：{error}"))
        })?;
        if !status.is_success() {
            return Err(WebError::provider_error(
                DEEPSEEK_SEARCH_PROVIDER,
                format!("{endpoint} 回了 HTTP {status}：{}", excerpt(&text)),
            ));
        }

        let value: Value = serde_json::from_str(&text).map_err(|error| {
            WebError::provider_error(
                DEEPSEEK_SEARCH_PROVIDER,
                format!("{endpoint} 的响应不是 JSON：{error}"),
            )
        })?;
        let mut sources = parse_sources(&value)?;
        // 后端可以少给、不该多给：上限由服务层说了算。
        sources.truncate(limit);
        Ok(sources)
    }
}

/// 一次搜索的请求体。
///
/// 纯函数，好让测试对它的每一个字段做断言：工具声明是缓存前缀之外的另一处「一次定死」的东西，
/// 它一漂，线上的协议就对不上了。
pub fn request_body(query: &str, model: &str) -> Value {
    json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "tools": [{
            "type": WEB_SEARCH_TOOL_TYPE,
            "name": "web_search",
            "max_uses": MAX_USES,
        }],
        "messages": [{
            "role": "user",
            "content": format!("Perform a web search for the query: {query}"),
        }],
    })
}

/// 从响应里取出结构化来源。
///
/// 只认 `web_search_tool_result` 块里的 `web_search_result` 条目；没有那个块、或服务端把失败
/// 放在那个块里，都是 [`WebErrorCode::ProviderError`](super::WebErrorCode::ProviderError)。
pub fn parse_sources(response: &Value) -> Result<Vec<Source>, WebError> {
    let Some(blocks) = response.get("content").and_then(Value::as_array) else {
        return Err(WebError::provider_error(
            DEEPSEEK_SEARCH_PROVIDER,
            "响应里没有 `content` 数组",
        ));
    };

    let mut sources = Vec::new();
    let mut saw_block = false;
    for block in blocks {
        if block.get("type").and_then(Value::as_str) != Some("web_search_tool_result") {
            continue;
        }
        saw_block = true;
        let Some(results) = block.get("content").and_then(Value::as_array) else {
            // 服务端把搜索失败放在同一个块里：`content` 是一个错误对象。
            let code = block
                .get("content")
                .and_then(|content| content.get("error_code"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(WebError::provider_error(
                DEEPSEEK_SEARCH_PROVIDER,
                format!("服务端搜索失败（{code}）"),
            ));
        };
        for result in results {
            if result.get("type").and_then(Value::as_str) != Some("web_search_result") {
                continue;
            }
            let Some(url) = result.get("url").and_then(Value::as_str) else {
                continue;
            };
            sources.push(Source {
                url: url.to_owned(),
                title: text_field(result, "title"),
                // 这个端点不给片段 —— 服务端只回标题、URL 与页面年龄，加上一段加密内容。
                snippet: None,
                published_at: text_field(result, "page_age"),
            });
        }
    }

    if !saw_block {
        return Err(WebError::provider_error(
            DEEPSEEK_SEARCH_PROVIDER,
            "响应里没有 `web_search_tool_result` 块 —— 这个后端不降级，\
             也绝不从回复文本里抓 URL",
        ));
    }
    Ok(sources)
}

fn text_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|text| !text.trim().is_empty())
}

/// 错误消息里带一点响应体。够看出「哪里不对」，又不至于把整页倒进上下文。
fn excerpt(text: &str) -> String {
    const LIMIT: usize = 200;
    let trimmed = text.trim();
    let mut taken: String = trimmed.chars().take(LIMIT).collect();
    if trimmed.chars().count() > LIMIT {
        taken.push('…');
    }
    taken
}
