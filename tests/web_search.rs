//! 内建的 `web_search` 工具与它下面那一层服务：组装期开关、结构化错误、`queries` 的规则，
//! 以及三个身份里的那段指引（`.scratch/web-search-tool/spec.md` §1–§3、§7、§8；票 01）。
//!
//! 没有网络：搜索后端是一个按脚本作答的假 provider，由组装层注入 —— 与 `src/provider/` 那条
//! 「provider 全部是假的」规矩同一档，只是这里扩到了 `src/web/`。

mod support;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use heng::agent::{agent_identity, WEB_GUIDANCE};
use heng::config::{SessionConfig, WebSettings};
use heng::discussion::{debater_identity, synthesizer_identity};
use heng::events::{
    read_events, Decision, DecisionSource, Event, EventPayload, SessionId, SpeakerId,
};
use heng::permissions::{Answer, Asker, Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{builtin, with_web, WEB_SEARCH_TOOL};
use heng::web::{SearchProvider, Source, WebError, WebService};
use heng::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{CaptureBuf, FakeProvider, Reply, ScriptedAsker};

// --- 假的搜索后端 ---------------------------------------------------------

/// 按脚本作答、并记下每一个查询的搜索后端。
#[derive(Clone)]
struct FakeSearch {
    inner: Arc<Inner>,
}

struct Inner {
    replies: Mutex<VecDeque<Result<Vec<Source>, WebError>>>,
    calls: Mutex<Vec<String>>,
}

impl FakeSearch {
    fn new(replies: Vec<Result<Vec<Source>, WebError>>) -> Self {
        Self {
            inner: Arc::new(Inner {
                replies: Mutex::new(replies.into()),
                calls: Mutex::new(Vec::new()),
            }),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.inner.calls.lock().expect("假后端已中毒").clone()
    }
}

#[async_trait]
impl SearchProvider for FakeSearch {
    fn name(&self) -> &str {
        "fake"
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Source>, WebError> {
        self.inner
            .calls
            .lock()
            .expect("假后端已中毒")
            .push(query.to_owned());
        let reply = self
            .inner
            .replies
            .lock()
            .expect("假后端已中毒")
            .pop_front()
            .expect("假搜索后端：脚本里的响应已经用完了");
        // 后端可以少给、不该多给：上限由服务层说了算。
        reply.map(|mut sources| {
            sources.truncate(limit);
            sources
        })
    }
}

fn source(url: &str, title: &str) -> Source {
    Source {
        url: url.to_owned(),
        title: Some(title.to_owned()),
        snippet: None,
        published_at: None,
    }
}

/// 一个开了开关的服务，可选地挂上一个后端。
fn service(search: Option<Arc<dyn SearchProvider>>, max_results: usize) -> WebService {
    let settings = WebSettings {
        enabled: true,
        search_max_results: max_results,
        ..WebSettings::default()
    };
    match search {
        Some(provider) => WebService::new(settings).with_search(provider),
        None => WebService::new(settings),
    }
}

// --- 会话 fixture ---------------------------------------------------------

fn web_reply(id: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: WEB_SEARCH_TOOL.to_owned(),
            arguments: args.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

fn completed_output(events: &[Event], tool_call_id: &str) -> Result<String, String> {
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                tool_call_id: id,
                ok,
                output,
                error,
                ..
            } if id.as_str() == tool_call_id => Some(if *ok {
                Ok(output.clone().unwrap_or_default())
            } else {
                Err(error.clone().unwrap_or_default())
            }),
            _ => None,
        })
        .expect("这次调用正好有一条结果")
}

fn completed_count(events: &[Event], tool_call_id: &str) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(&event.payload, EventPayload::ToolCallCompleted { tool_call_id: id, .. }
                if id.as_str() == tool_call_id)
        })
        .count()
}

struct Fixture {
    harness: Option<Harness>,
    log_path: PathBuf,
    /// 在整个测试期间保持活着。
    _dir: tempfile::TempDir,
}

async fn fixture(
    replies: Vec<Reply>,
    mode: Mode,
    asker: Option<Arc<dyn Asker>>,
    web: WebService,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = session.join("log.jsonl");
    let provider = FakeProvider::new(replies);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace,
            log_path: log_path.clone(),
            session_id: SessionId::new("s-web"),
            // 组装期的那一步：`enabled` 与后端挂没挂是两件事。
            tools: with_web(builtin(false), web),
            locks: heng::tools::PathLocks::new(),
            policy: Policy::for_mode(mode),
            asker,
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness: Some(harness),
        log_path,
        _dir: dir,
    }
}

impl Fixture {
    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }

    fn decisions(&self) -> Vec<(Decision, DecisionSource, Option<String>)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::PermissionDecided {
                    decision,
                    source,
                    reason,
                    ..
                } => Some((*decision, *source, reason.clone())),
                _ => None,
            })
            .collect()
    }

    async fn run_turn(&mut self, input: &str) {
        self.harness
            .as_mut()
            .expect("harness 已经关掉了")
            .run_turn(input)
            .await
            .unwrap();
    }

    async fn shutdown(&mut self) {
        if let Some(harness) = self.harness.take() {
            harness.shutdown().await;
        }
    }
}

// --- 注册与开关 -----------------------------------------------------------

#[test]
fn the_tool_stays_out_of_the_table_until_the_config_turns_it_on() {
    let off = with_web(builtin(false), WebService::new(WebSettings::default()));
    assert!(
        off.get(WEB_SEARCH_TOOL).is_none(),
        "缺省 `enabled = false`：工具不进表"
    );

    // 后端没挂时工具**仍在**表里：表不随凭据状态抖动，调用给出的是结构化错误。
    let on = with_web(builtin(false), service(None, 8));
    assert!(on.get(WEB_SEARCH_TOOL).is_some());
    assert!(
        on.for_executor().get(WEB_SEARCH_TOOL).is_some(),
        "执行者也拿到它（spec §8）"
    );
}

#[tokio::test]
async fn a_missing_backend_is_a_readable_result_with_exactly_one_event() {
    let mut fixture = fixture(
        vec![
            web_reply(
                "call-1",
                serde_json::json!({ "queries": ["rg 的 Rust 库"] }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(None, 8),
    )
    .await;
    fixture.run_turn("search").await;

    let events = fixture.events();
    let output = completed_output(&events, "call-1").unwrap();
    assert!(output.contains("WEB_PROVIDER_UNAVAILABLE"), "{output}");
    assert!(output.contains("[web]"), "消息要说清去哪儿配：{output}");
    assert_eq!(completed_count(&events, "call-1"), 1);
    fixture.shutdown().await;
}

#[tokio::test]
async fn malformed_queries_are_refused_before_anything_is_sent() {
    let provider = FakeSearch::new(Vec::new());
    let mut fixture = fixture(
        vec![
            web_reply("call-1", serde_json::json!({ "queries": [] })),
            web_reply("call-2", serde_json::json!({ "queries": ["   "] })),
            web_reply(
                "call-3",
                serde_json::json!({ "queries": ["a", "b", "c", "d", "e"] }),
            ),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(provider.clone())), 8),
    )
    .await;
    fixture.run_turn("search").await;

    let events = fixture.events();
    for (id, needle) in [
        ("call-1", "至少要有一条"),
        ("call-2", "空白查询"),
        ("call-3", "一次最多 4 条"),
    ] {
        let error = completed_output(&events, id).unwrap_err();
        assert!(error.contains(needle), "{id}：{error}");
    }
    assert!(
        provider.calls().is_empty(),
        "三种毛病都在执行前被拒：{:?}",
        provider.calls()
    );
    fixture.shutdown().await;
}

// --- `queries` 的规则（服务层）--------------------------------------------

#[tokio::test]
async fn identical_queries_are_asked_once() {
    let provider = FakeSearch::new(vec![Ok(vec![source("https://a", "A")])]);
    let outcome = service(Some(Arc::new(provider.clone())), 8)
        .search(&["same".to_owned(), "same".to_owned()])
        .await
        .unwrap();

    assert_eq!(provider.calls(), vec!["same".to_owned()]);
    assert_eq!(outcome.sources.len(), 1);
}

#[tokio::test]
async fn results_merge_round_robin_and_dedupe_by_url() {
    let provider = FakeSearch::new(vec![
        Ok(vec![
            source("https://one", "One"),
            source("https://shared", "Shared"),
        ]),
        Ok(vec![
            source("https://two", "Two"),
            source("https://shared", "Shared"),
        ]),
    ]);
    let outcome = service(Some(Arc::new(provider.clone())), 8)
        .search(&["first".to_owned(), "second".to_owned()])
        .await
        .unwrap();

    let urls: Vec<&str> = outcome.sources.iter().map(|s| s.url.as_str()).collect();
    assert_eq!(
        urls,
        vec!["https://one", "https://two", "https://shared"],
        "轮询合并：先各取第一条，再轮到第二条；同一条 URL 只留一次"
    );
    assert!(!outcome.truncated);
}

#[tokio::test]
async fn the_merge_stops_at_the_configured_limit_and_says_so() {
    let provider = FakeSearch::new(vec![
        Ok(vec![
            source("https://one", "One"),
            source("https://two", "Two"),
        ]),
        Ok(vec![source("https://three", "Three")]),
    ]);
    let outcome = service(Some(Arc::new(provider.clone())), 2)
        .search(&["first".to_owned(), "second".to_owned()])
        .await
        .unwrap();

    let urls: Vec<&str> = outcome.sources.iter().map(|s| s.url.as_str()).collect();
    assert_eq!(urls, vec!["https://one", "https://three"]);
    assert!(outcome.truncated, "还有来源装不下这个事实要往下传");
}

#[tokio::test]
async fn one_failed_query_throws_the_whole_merge_away() {
    let provider = FakeSearch::new(vec![
        Ok(vec![source("https://one", "One")]),
        Err(WebError::provider_error("fake", "第二家挂了")),
    ]);
    let error = service(Some(Arc::new(provider.clone())), 8)
        .search(&["first".to_owned(), "second".to_owned()])
        .await
        .expect_err("一条失败就整次失败");

    assert_eq!(error.code_str(), "WEB_PROVIDER_ERROR");
    assert!(error.message().contains("第二家挂了"), "{error}");
}

#[tokio::test]
async fn a_missing_backend_never_reaches_a_provider() {
    let error = service(None, 8)
        .search(&["anything".to_owned()])
        .await
        .expect_err("没有挂后端");
    assert_eq!(error.code_str(), "WEB_PROVIDER_UNAVAILABLE");
    assert!(
        error.message().contains("deepseek"),
        "点名配的是哪一家：{error}"
    );
}

// --- 结果的形状 -----------------------------------------------------------

#[tokio::test]
async fn the_result_carries_the_untrusted_marker_and_the_sources() {
    let provider = FakeSearch::new(vec![Ok(vec![Source {
        url: "https://example.com/page".to_owned(),
        title: Some("一页文档".to_owned()),
        snippet: Some("这是片段\n换过行".to_owned()),
        published_at: Some("2026-01-02".to_owned()),
    }])]);
    let mut fixture = fixture(
        vec![
            web_reply("call-1", serde_json::json!({ "queries": ["doc"] })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(provider)), 8),
    )
    .await;
    fixture.run_turn("search").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.starts_with("[外部内容："), "{output}");
    assert!(output.contains("Sources:"), "{output}");
    assert!(
        output.contains("- [一页文档](https://example.com/page) — 这是片段 换过行 (2026-01-02)"),
        "标题、片段与日期都在同一条上：{output}"
    );
    assert!(
        output.contains("把相关 URL 作为 markdown 链接引用"),
        "{output}"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn an_empty_result_says_so_rather_than_returning_nothing() {
    let provider = FakeSearch::new(vec![Ok(Vec::new())]);
    let mut fixture = fixture(
        vec![
            web_reply("call-1", serde_json::json!({ "queries": ["nothing"] })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(provider)), 8),
    )
    .await;
    fixture.run_turn("search").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("没有搜到任何结果"), "{output}");
    fixture.shutdown().await;
}

// --- 权限 -----------------------------------------------------------------

#[tokio::test]
async fn readonly_allows_a_search_and_ask_does_not_interrupt_it() {
    let readonly_provider = FakeSearch::new(vec![Ok(vec![source("https://a", "A")])]);
    let mut readonly = fixture(
        vec![
            web_reply("call-1", serde_json::json!({ "queries": ["x"] })),
            Reply::text("done"),
        ],
        Mode::Readonly,
        None,
        service(Some(Arc::new(readonly_provider)), 8),
    )
    .await;
    readonly.run_turn("search").await;
    let decisions = readonly.decisions();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].0, Decision::Allow);
    readonly.shutdown().await;

    let provider = FakeSearch::new(vec![Ok(vec![source("https://a", "A")])]);
    let asker = ScriptedAsker::new(Vec::new());
    let mut ask = fixture(
        vec![
            web_reply("call-1", serde_json::json!({ "queries": ["x"] })),
            Reply::text("done"),
        ],
        Mode::Ask,
        Some(Arc::new(asker.clone())),
        service(Some(Arc::new(provider)), 8),
    )
    .await;
    ask.run_turn("search").await;
    assert!(
        asker.requests().is_empty(),
        "`ask` 档下一次只读出网不该打断人：{:?}",
        asker.requests()
    );
    assert_eq!(ask.decisions()[0].0, Decision::Allow);
    ask.shutdown().await;
}

#[tokio::test]
async fn a_shell_network_call_still_asks_in_ask_mode() {
    // 反向锚：这条工具修的是「纯读取的出网在 `ask` 档要审批」。同一件事用 `bash` 拼 `curl`
    // 仍然过审批 —— 关掉工具不等于关掉出网。
    let asker = ScriptedAsker::new(vec![Answer::Allow]);
    let mut fixture = fixture(
        vec![
            Reply::Stream(vec![
                StreamEvent::ToolCallCompleted {
                    index: 0,
                    id: "call-1".to_owned(),
                    name: "bash".to_owned(),
                    arguments: serde_json::json!({ "command": "curl -s https://example.com" })
                        .to_string(),
                },
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::text("done"),
        ],
        Mode::Ask,
        Some(Arc::new(asker.clone())),
        service(None, 8),
    )
    .await;
    fixture.run_turn("fetch it").await;

    assert_eq!(asker.requests().len(), 1, "拼 shell 仍然要人点头");
    fixture.shutdown().await;
}

// --- 身份里的指引 ---------------------------------------------------------

#[test]
fn the_web_guidance_rides_in_the_identities_that_have_tools() {
    // 公开的那两段身份在这里断言；执行者那一段不在公开 API 上，它的断言住在
    // `tests/executor.rs`，从一次真实派发发出去的请求里读 —— 与思考语言那条同一处规矩。
    for (who, identity) in [
        ("本程序", agent_identity()),
        ("讨论者", debater_identity("kimi")),
    ] {
        assert!(
            identity.contains(WEB_GUIDANCE),
            "{who} 的身份里没有那段联网指引：{identity}"
        );
    }
    assert!(
        !synthesizer_identity().contains(WEB_GUIDANCE),
        "合成器不参与讨论、没有工具，指引不该出现在它的身份里"
    );
}

#[tokio::test]
async fn a_shortened_source_list_says_how_many_are_shown() {
    // 两个查询各回一条，而合并的上限是一：第二条装不下，于是那句提示出场。
    let provider = FakeSearch::new(vec![
        Ok(vec![source("https://one", "One")]),
        Ok(vec![source("https://two", "Two")]),
    ]);
    let mut fixture = fixture(
        vec![
            web_reply("call-1", serde_json::json!({ "queries": ["x", "y"] })),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(provider)), 1),
    )
    .await;
    fixture.run_turn("search").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("只列了前 1 条"), "{output}");
    assert!(output.contains("缩小查询"), "{output}");
    fixture.shutdown().await;
}
