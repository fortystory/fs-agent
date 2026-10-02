//! DeepSeek 搜索后端：请求体、结构化解析、凭据与打码
//! （`.scratch/web-search-tool/spec.md` §4；票 02）。
//!
//! **零网络**：这一份里的每一次断言都落在纯函数上，或者落在一个「在发请求之前就失败」的调用
//! 上。真发一次调用是票 02 留下来的人工项（见 `docs/web.md` 的走查一节）。

mod support;

use std::sync::Arc;

use async_trait::async_trait;
use fs_agent::config::{self, EnvMap};
use fs_agent::events::{read_events, EventPayload, SessionId, SpeakerId};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, StreamEvent};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::tools::{builtin, with_web, WEB_SEARCH_TOOL};
use fs_agent::web::search_deepseek::{
    parse_sources, request_body, DeepSeekSearch, DEEPSEEK_SEARCH_MODEL, DEEPSEEK_SEARCH_PROVIDER,
};
use fs_agent::web::{SearchProvider, Source, WebError, WebService};
use fs_agent::{assemble, AssemblyParts, SessionScaffold};
use support::{CaptureBuf, FakeProvider, Reply};

// --- 请求体 ---------------------------------------------------------------

#[test]
fn the_request_is_an_anthropic_messages_call_with_the_native_search_tool() {
    let body = request_body("rust async book", DEEPSEEK_SEARCH_MODEL);

    assert_eq!(body["model"], "deepseek-flash");
    assert_eq!(body["max_tokens"], 4096);
    assert_eq!(body["tools"][0]["name"], "web_search");
    assert!(
        body["tools"][0]["type"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("web_search")),
        "服务端工具的类型串：{}",
        body["tools"][0]["type"]
    );
    assert_eq!(body["tools"][0]["max_uses"], 5);
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(
        body["messages"][0]["content"],
        "Perform a web search for the query: rust async book"
    );

    // 没有多余字段：这个 body 的每一个键都是一次协议承诺。
    let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["max_tokens", "messages", "model", "tools"]);
}

#[test]
fn the_endpoint_is_the_anthropic_one_not_the_openai_one() {
    // 会话端点走 OpenAI 兼容格式，搜索走 Anthropic 兼容格式 —— 两者只差一个 `/anthropic`
    // 前缀，所以这条断言值得写出来。
    let backend = DeepSeekSearch::new("https://api.deepseek.com/anthropic/", Some("k".to_owned()));
    assert_eq!(
        backend.endpoint(),
        "https://api.deepseek.com/anthropic/v1/messages"
    );
    assert_eq!(backend.name(), DEEPSEEK_SEARCH_PROVIDER);
}

// --- 解析 -----------------------------------------------------------------

#[test]
fn the_structured_block_becomes_sources() {
    let response = serde_json::json!({
        "content": [
            {
                "type": "server_tool_use",
                "id": "srvtoolu_1",
                "name": "web_search",
                "input": { "query": "rust book" }
            },
            {
                "type": "web_search_tool_result",
                "tool_use_id": "srvtoolu_1",
                "content": [
                    {
                        "type": "web_search_result",
                        "url": "https://doc.rust-lang.org/book/",
                        "title": "The Rust Book",
                        "page_age": "2025-09-01"
                    },
                    {
                        "type": "web_search_result",
                        "url": "https://example.com/other",
                        "title": "",
                        "page_age": null
                    }
                ]
            },
            {
                "type": "text",
                "text": "总结里还可能提到 https://not-a-source.example"
            }
        ]
    });

    let sources = parse_sources(&response).unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].url, "https://doc.rust-lang.org/book/");
    assert_eq!(sources[0].title.as_deref(), Some("The Rust Book"));
    assert_eq!(sources[0].published_at.as_deref(), Some("2025-09-01"));
    assert_eq!(sources[1].title, None, "空标题当作没有");
    assert!(
        !sources
            .iter()
            .any(|source| source.url.contains("not-a-source")),
        "绝不从回复文本里抓 URL：文本是模型的散文，不是来源"
    );
}

#[test]
fn a_response_without_the_block_is_an_error_rather_than_a_guess() {
    let response = serde_json::json!({
        "content": [{ "type": "text", "text": "我认为是 https://example.com" }]
    });
    let error = parse_sources(&response).unwrap_err();
    assert_eq!(error.code_str(), "WEB_PROVIDER_ERROR");
    assert!(
        error.message().contains("web_search_tool_result"),
        "{error}"
    );
}

#[test]
fn a_server_side_search_failure_keeps_its_own_code() {
    let response = serde_json::json!({
        "content": [{
            "type": "web_search_tool_result",
            "tool_use_id": "srvtoolu_1",
            "content": {
                "type": "web_search_tool_result_error",
                "error_code": "max_uses_exceeded"
            }
        }]
    });
    let error = parse_sources(&response).unwrap_err();
    assert_eq!(error.code_str(), "WEB_PROVIDER_ERROR");
    assert!(error.message().contains("max_uses_exceeded"), "{error}");
}

#[tokio::test]
async fn a_missing_key_is_a_credential_error_before_any_request() {
    let backend = DeepSeekSearch::new("https://api.deepseek.com/anthropic", None);
    let error = backend.search("anything", 8).await.unwrap_err();

    assert_eq!(error.code_str(), "WEB_PROVIDER_CREDENTIAL_MISSING");
    assert!(
        error.message().contains("DEEPSEEK_API_KEY"),
        "消息要说清配哪一把：{error}"
    );
}

// --- 打码 -----------------------------------------------------------------

/// 一条 URL 里正好带着会话密钥的搜索后端 —— 打码这条不变量在 web 这条路径上的探针。
struct LeakySearch {
    secret: String,
}

#[async_trait]
impl SearchProvider for LeakySearch {
    fn name(&self) -> &str {
        "leaky"
    }

    async fn search(&self, _query: &str, _limit: usize) -> Result<Vec<Source>, WebError> {
        Ok(vec![Source {
            url: format!("https://example.com/?key={}", self.secret),
            title: Some("一页".to_owned()),
            snippet: None,
            published_at: None,
        }])
    }
}

#[tokio::test]
async fn a_secret_that_reaches_the_stream_through_web_results_is_redacted() {
    // 从「config.toml 里那一把密钥」到「流会擦掉的一个值」，接线走在
    // `Config::session_config` 那一处；搜索复用同一把密钥，所以这条不变量在联网这条路径上
    // 同样成立 —— 这里把它钉住。
    const SECRET: &str = "sk-deepseek-config-key-0123456789";
    let file = format!(
        "default_model = \"deepseek-v4-pro\"\n\n[providers.deepseek]\napi_key = \"{SECRET}\"\n\n\
         [models.deepseek-v4-pro]\nprovider = \"deepseek\"\n"
    );
    let config = config::resolve(Some(&file), &EnvMap::new()).unwrap();
    let mut session = config.session_config("deepseek-v4-pro").unwrap();
    assert!(!session.redactor.is_empty());
    // 搜索走的是同一个进程里的工具调用，不需要真的调模型。
    session.model = "fake-model".to_owned();

    let settings = fs_agent::config::WebSettings {
        enabled: true,
        ..fs_agent::config::WebSettings::default()
    };
    let service = WebService::new(settings).with_search(Arc::new(LeakySearch {
        secret: SECRET.to_owned(),
    }));

    let dir = tempfile::tempdir().unwrap();
    let session_dir = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session_dir).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session_dir.join("log.jsonl");
    let provider = FakeProvider::new(vec![
        Reply::Stream(vec![
            StreamEvent::ToolCallCompleted {
                index: 0,
                id: "call-1".to_owned(),
                name: WEB_SEARCH_TOOL.to_owned(),
                arguments: serde_json::json!({ "queries": ["x"] }).to_string(),
            },
            StreamEvent::Finished {
                finish_reason: FinishReason::ToolCalls,
            },
        ]),
        Reply::text("done"),
    ]);

    let mut harness = assemble(AssemblyParts {
        provider: Box::new(provider),
        speaker: SpeakerId::Debater("kimi".into()),
        config: session,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace,
            log_path: log_path.clone(),
            session_id: SessionId::new("s-web-secret"),
            tools: with_web(builtin(false), service),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();
    harness.run_turn("search").await.unwrap();
    harness.shutdown().await;

    let events = read_events(&log_path).unwrap();
    let output = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted { output, .. } => output.clone(),
            _ => None,
        })
        .expect("这次调用有一条结果");

    assert!(!output.contains(SECRET), "密钥不该进流：{output}");
    assert!(output.contains("[redacted]"), "{output}");

    let mut leaked = false;
    for event in &events {
        let json = serde_json::to_string(event).unwrap();
        leaked |= json.contains(SECRET);
    }
    assert!(!leaked, "整条流上都不该有那个值");
}

/// 会话配置里那一把密钥的来源就是 `[providers.deepseek]`，而搜索复用它 —— 这条断言钉住
/// 「零新密钥」这个选择。（真正的取用点在 `cli::web_service`。）
#[test]
fn the_search_backend_reuses_the_provider_key() {
    const SECRET: &str = "sk-deepseek-config-key-0123456789";
    let file = format!(
        "default_model = \"deepseek-v4-pro\"\n\n[providers.deepseek]\napi_key = \"{SECRET}\"\n\n\
         [models.deepseek-v4-pro]\nprovider = \"deepseek\"\n"
    );
    let config = config::resolve(Some(&file), &EnvMap::new()).unwrap();
    let backend = DeepSeekSearch::new(
        &config.web.search_base_url,
        config
            .provider(DEEPSEEK_SEARCH_PROVIDER)
            .and_then(|provider| provider.api_key.clone()),
    );
    assert_eq!(
        config.web.search_base_url,
        "https://api.deepseek.com/anthropic"
    );
    assert_eq!(
        backend.endpoint(),
        "https://api.deepseek.com/anthropic/v1/messages"
    );
}
