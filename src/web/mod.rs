//! 出网（web）：工具层 / 服务层 / 后端三层里中间的那一层
//! （`.scratch/web-search-tool/spec.md` §1）。
//!
//! 依赖单向向下：`tools → web`。`web` 自己发 HTTP，不碰会话的 provider —— 会话模型的调用仍
//! 只做 OpenAI 兼容那一套，而**工具内部的 HTTP 是工具自己的事**。
//!
//! 三层的分工：
//!
//! * **工具层**（`src/tools/web_search.rs`、`src/tools/web_fetch.rs`）拥有面向模型的约定：
//!   工具名、schema、参数校验、结果上限、不可信标记与引用格式。它**绝不**问后端「可用吗」、
//!   绝不枚举后端 —— 唯一的执行路径就是这里的 [`WebService::search`] 与 [`WebService::fetch`]。
//! * **服务层**（这里）拥有后端选择、查询合并与错误码：后端缺失或出错时返回带 code 的
//!   [`WebError`]，由工具层渲染成模型可读的句子。
//! * **后端**拥有出网、协议与解析。换后端不动工具声明 —— 工具声明是缓存前缀的一部分，一次
//!   定死。
//!
//! 一条边界写在这里，免得日后被读成疏漏：**这一层不是一道墙**。真正的界是人在 `[web]` 里
//! 打开开关那一步；`bash` 里的 `curl` 一个字没动。

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{FuturesUnordered, StreamExt};

use crate::config::WebSettings;

pub mod search_deepseek;

/// 一条搜索结果来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub url: String,
    pub title: Option<String>,
    pub snippet: Option<String>,
    /// 这一页发布 / 更新于何时，后端给得出时才有。
    pub published_at: Option<String>,
}

/// 一次搜索的产物：来源，以及「还有来源没列出来」这个事实。
///
/// 合并、去重与截断都发生在服务层，所以 `truncated` 也是在这里算出来的 —— 工具层只负责把
/// 它说给模型听。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchOutcome {
    pub sources: Vec<Source>,
    /// 去重之后还有来源装不下时为真。
    pub truncated: bool,
}

/// 一次抓取的产物。
///
/// 非 2xx 也是**结果**而不是错误：404 是被抓资源的状态，模型该看到它。所以状态码是一个字段，
/// 不是 `Err`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchOutcome {
    /// 最终 URL（跟着同源重定向走完之后的那一个）。
    pub url: String,
    pub status: u16,
    pub content: FetchedContent,
    /// 正文被 `fetch_max_chars` 截过时为真。
    pub truncated: bool,
}

/// 抓回来的正文是哪一类。HTML 要在工具层换成 markdown，纯文本原样通过。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchedContent {
    Html(String),
    Text(String),
}

/// 一次出网调用为什么失败。
///
/// 是有 code 的结构化错误（照 DSH）：`code` 是 schema 值那一类、保持英文，给人读的句子在
/// 工具层拼。工具层据此渲染成模型可读的一条结果，而不是让一个字面量字符串漏到线上。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebError {
    code: WebErrorCode,
    message: String,
}

/// 错误码。字符串形式见 [`WebErrorCode::as_str`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebErrorCode {
    /// 配置里点名了后端，但它在这个进程里没被挂上。
    ProviderUnavailable,
    /// 后端的凭据解析不到。
    CredentialMissing,
    /// 后端自己失败了：传输、HTTP、协议、解析。
    ProviderError,
    /// URL 本身不合法（scheme、内嵌凭据、太长）。
    InvalidUrl,
    /// URL 合法，但它指向的目标是被拒绝的（SSRF 防护）。
    BlockedUrl,
    /// 响应体超过了字节上限。
    FetchTooLarge,
    /// 抓取超时。
    FetchTimeout,
}

impl WebErrorCode {
    /// 线上的那一串。**保持英文**：它是 schema 值，不是散文（ADR 0005）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderUnavailable => "WEB_PROVIDER_UNAVAILABLE",
            Self::CredentialMissing => "WEB_PROVIDER_CREDENTIAL_MISSING",
            Self::ProviderError => "WEB_PROVIDER_ERROR",
            Self::InvalidUrl => "WEB_INVALID_URL",
            Self::BlockedUrl => "WEB_BLOCKED_URL",
            Self::FetchTooLarge => "WEB_FETCH_TOO_LARGE",
            Self::FetchTimeout => "WEB_FETCH_TIMEOUT",
        }
    }
}

impl WebError {
    pub fn new(code: WebErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 配置里点名了一个后端，但它没被挂上。`what` 是「搜索」或「抓取」。
    pub fn provider_unavailable(what: &str, provider: &str) -> Self {
        Self::new(
            WebErrorCode::ProviderUnavailable,
            format!(
                "这个会话没有可用的{what}后端（配置里点名的是 `{provider}`）—— \
                 检查 `[web]` 那一段，或用 `[web] enabled = false` 关掉这两个工具"
            ),
        )
    }

    /// 后端要的凭据解析不到。消息里写清该配哪一把。
    pub fn credential_missing(provider: &str, expected: &str) -> Self {
        Self::new(
            WebErrorCode::CredentialMissing,
            format!("`{provider}` 的凭据解析不到：在环境里导出 {expected}，或写进 `config.toml`"),
        )
    }

    /// 后端自己失败了。`message` 里带上端点 —— 端点配错是最常见的一种。
    pub fn provider_error(provider: &str, message: impl Into<String>) -> Self {
        Self::new(
            WebErrorCode::ProviderError,
            format!("`{provider}` 调用失败：{}", message.into()),
        )
    }

    pub fn code(&self) -> WebErrorCode {
        self.code
    }

    pub fn code_str(&self) -> &'static str {
        self.code.as_str()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for WebError {
    /// `CODE：一句话` —— code 英文、句子中文，两者合起来是模型看到的那一行。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}：{}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for WebError {}

/// 搜索后端：一次一个查询，返回按排名排好的来源。
#[async_trait]
pub trait SearchProvider: Send + Sync {
    /// 这个后端的名字，与配置里的 `search_provider` 对应。错误消息用它。
    fn name(&self) -> &str;

    /// 搜一个查询。`limit` 是这次要的上限，后端可以少给、不该多给。
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Source>, WebError>;
}

/// 抓取后端：把一个 URL 变成有界的正文。
#[async_trait]
pub trait FetchProvider: Send + Sync {
    fn name(&self) -> &str;

    /// 取一个 URL。URL 校验与 SSRF 防护是这一层的事（spec §5）。
    async fn fetch(&self, url: &str) -> Result<FetchOutcome, WebError>;
}

/// 两个工具唯一的执行路径。
///
/// 组装期先建它、再挂后端：没有挂后端时它照样是一个可用的服务 —— 调用返回
/// [`WebErrorCode::ProviderUnavailable`]，而**工具仍在表里**。工具表不随凭据状态抖动。
pub struct WebService {
    settings: WebSettings,
    search: Option<Arc<dyn SearchProvider>>,
    fetch: Option<Arc<dyn FetchProvider>>,
}

impl WebService {
    pub fn new(settings: WebSettings) -> Self {
        Self {
            settings,
            search: None,
            fetch: None,
        }
    }

    pub fn with_search(mut self, provider: Arc<dyn SearchProvider>) -> Self {
        self.search = Some(provider);
        self
    }

    pub fn with_fetch(mut self, provider: Arc<dyn FetchProvider>) -> Self {
        self.fetch = Some(provider);
        self
    }

    /// 部署设置。工具层从这里读条数与字符上限，所以上限只有一处答案。
    pub fn settings(&self) -> &WebSettings {
        &self.settings
    }

    /// 一次调用里的多条查询。
    ///
    /// 规则照 DSH（spec §2）：完全相同的查询去重、并发执行、按排名轮询合并、按 URL 去重、
    /// 在 `search_max_results` 处截断。**任何一条失败就中止其余、丢弃成功结果，只回首个
    /// 错误** —— 半份合并结果是给模型下套。
    pub async fn search(&self, queries: &[String]) -> Result<SearchOutcome, WebError> {
        let Some(provider) = self.search.as_ref() else {
            return Err(WebError::provider_unavailable(
                "搜索",
                &self.settings.search_provider,
            ));
        };

        // 去重保留首现位置：重复的查询是模型的手滑，不该让它多付一次调用的钱。
        let mut unique: Vec<&str> = Vec::new();
        for query in queries {
            if !unique.contains(&query.as_str()) {
                unique.push(query.as_str());
            }
        }

        let limit = self.settings.search_max_results;
        let mut pending = FuturesUnordered::new();
        for (index, query) in unique.iter().enumerate() {
            pending.push(async move { (index, provider.search(query, limit).await) });
        }

        let mut per_query: Vec<Vec<Source>> = vec![Vec::new(); unique.len()];
        while let Some((index, result)) = pending.next().await {
            match result {
                Ok(sources) => per_query[index] = sources,
                // 第一个错误就是这次调用的结论：`pending` 在这里被丢掉，也就是把其余查询取消
                // 掉（这里没有 spawn，drop 就是取消），成功的结果一并丢弃。
                Err(error) => return Err(error),
            }
        }

        Ok(merge(per_query, limit))
    }

    /// 取一页。
    pub async fn fetch(&self, url: &str) -> Result<FetchOutcome, WebError> {
        let Some(provider) = self.fetch.as_ref() else {
            return Err(WebError::provider_unavailable(
                "抓取",
                &self.settings.fetch_provider,
            ));
        };
        provider.fetch(url).await
    }
}

impl fmt::Debug for WebService {
    /// 后端是不透明的句柄，一条诊断能说的有用的话只有「挂没挂」。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebService")
            .field("settings", &self.settings)
            .field("search", &self.search.as_ref().map(|p| p.name()))
            .field("fetch", &self.fetch.as_ref().map(|p| p.name()))
            .finish()
    }
}

/// 按排名轮询合并各查询的结果，按 URL 去重，在 `limit` 处截断。
///
/// 轮询而不是「一个查询的结果接另一个」：排名是各后端**各自**的排名，把它们首尾相接等于让
/// 第一个查询独占全部名额。
fn merge(per_query: Vec<Vec<Source>>, limit: usize) -> SearchOutcome {
    let mut sources: Vec<Source> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut truncated = false;
    let mut rank = 0usize;
    loop {
        let mut any_at_this_rank = false;
        for result in &per_query {
            let Some(source) = result.get(rank) else {
                continue;
            };
            any_at_this_rank = true;
            if !seen.insert(source.url.clone()) {
                continue;
            }
            if sources.len() >= limit {
                truncated = true;
                continue;
            }
            sources.push(source.clone());
        }
        if !any_at_this_rank {
            break;
        }
        rank += 1;
    }
    SearchOutcome { sources, truncated }
}
