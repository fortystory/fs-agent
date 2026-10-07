//! 内建的 `web_fetch` 工具与正文提取：HTML → markdown、失败分支、非 2xx、上限与权限
//! （`.scratch/web-search-tool/spec.md` §2、§5、§7；票 04）。
//!
//! **零网络**：抓取后端是一个按脚本作答的假 provider，由组装层注入。

mod support;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use heng::config::{SessionConfig, WebSettings};
use heng::events::{Decision, Event, EventPayload, SessionId, SpeakerId, read_events};
use heng::permissions::{Asker, Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{WEB_FETCH_TOOL, builtin, with_web};
use heng::web::html::{OMITTED_MARKER, to_markdown};
use heng::web::{FetchOutcome, FetchProvider, FetchedContent, WebError, WebService};
use heng::{AssemblyParts, Harness, SessionScaffold, assemble};
use support::{CaptureBuf, FakeProvider, Reply, ScriptedAsker};

// --- HTML → markdown ------------------------------------------------------

#[test]
fn active_and_hidden_content_is_dropped_before_anything_is_converted() {
    let html = r#"
        <html><head><style>body { color: red }</style></head>
        <body>
          <script>alert('不要执行我')</script>
          <p>正文第一段。</p>
          <div style="display:none">隐藏的广告</div>
          <div hidden>也是隐藏的</div>
          <div aria-hidden="true">还是隐藏的</div>
          <!-- 一条注释 -->
          <p>正文第二段。</p>
        </body></html>
    "#;
    let markdown = to_markdown(html);

    assert!(markdown.contains("正文第一段。"), "{markdown}");
    assert!(markdown.contains("正文第二段。"), "{markdown}");
    for dropped in [
        "alert",
        "color: red",
        "隐藏的广告",
        "也是隐藏的",
        "还是隐藏的",
        "一条注释",
    ] {
        assert!(
            !markdown.contains(dropped),
            "`{dropped}` 不该出现：{markdown}"
        );
    }
    for tag in ["<script", "<style", "<div", "<p>", "<!--"] {
        assert!(!markdown.contains(tag), "原始标签不该残留：{markdown}");
    }
}

#[test]
fn headings_links_lists_and_emphasis_become_markdown() {
    let html = r#"
        <h2>标题</h2>
        <p>这是一段带 <a href="https://example.com/x">链接</a> 与
        <strong>粗体</strong>、<em>斜体</em>、<del>删除线</del> 的正文。</p>
        <ul><li>第一项</li><li>第二项</li></ul>
        <pre><code>fn main() {}</code></pre>
    "#;
    let markdown = to_markdown(html);

    assert!(markdown.contains("## 标题"), "{markdown}");
    assert!(
        markdown.contains("[链接](https://example.com/x)"),
        "{markdown}"
    );
    assert!(markdown.contains("**粗体**"), "{markdown}");
    assert!(markdown.contains("*斜体*"), "{markdown}");
    assert!(markdown.contains("~~删除线~~"), "{markdown}");
    assert!(markdown.contains("- 第一项"), "{markdown}");
    assert!(markdown.contains("- 第二项"), "{markdown}");
    assert!(markdown.contains("```\nfn main() {}\n```"), "{markdown}");
}

#[test]
fn a_table_becomes_a_gfm_table() {
    let html = r#"
        <table>
          <thead><tr><th>名字</th><th>版本</th></tr></thead>
          <tbody>
            <tr><td>ignore</td><td>0.4</td></tr>
            <tr><td>globset</td><td>0.4</td></tr>
          </tbody>
        </table>
    "#;
    let markdown = to_markdown(html);

    assert!(markdown.contains("| 名字 | 版本 |"), "{markdown}");
    assert!(markdown.contains("| --- | --- |"), "{markdown}");
    assert!(markdown.contains("| ignore | 0.4 |"), "{markdown}");
    assert!(markdown.contains("| globset | 0.4 |"), "{markdown}");
}

#[test]
fn a_document_too_deep_to_finish_gets_a_fixed_marker_never_the_tags() {
    let deep = format!("{}{}{}", "<div>".repeat(600), "深处", "</div>".repeat(600));
    let markdown = to_markdown(&deep);

    assert!(markdown.contains(OMITTED_MARKER), "{markdown}");
    assert!(!markdown.contains("<div"), "绝不回原始 HTML：{markdown}");
}

#[test]
fn entities_are_decoded_and_comments_leave_nothing_behind() {
    let html = "<p>a &amp; b &lt; c</p><!-- hidden --><p>d</p>";
    let markdown = to_markdown(html);

    assert!(markdown.contains("a & b < c"), "{markdown}");
    assert!(markdown.contains("d"), "{markdown}");
    assert!(!markdown.contains("hidden"), "{markdown}");
}

// --- 会话层：工具、上限与权限 ---------------------------------------------

/// 按脚本作答、并记下每一个 URL 的抓取后端。
#[derive(Clone)]
struct FakeFetch {
    inner: Arc<Inner>,
}

struct Inner {
    replies: Mutex<VecDeque<Result<FetchOutcome, WebError>>>,
    calls: Mutex<Vec<String>>,
}

impl FakeFetch {
    fn new(replies: Vec<Result<FetchOutcome, WebError>>) -> Self {
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
impl FetchProvider for FakeFetch {
    fn name(&self) -> &str {
        "fake"
    }

    async fn fetch(&self, url: &str) -> Result<FetchOutcome, WebError> {
        self.inner
            .calls
            .lock()
            .expect("假后端已中毒")
            .push(url.to_owned());
        self.inner
            .replies
            .lock()
            .expect("假后端已中毒")
            .pop_front()
            .expect("假抓取后端：脚本里的响应已经用完了")
    }
}

fn outcome(url: &str, status: u16, content: FetchedContent, truncated: bool) -> FetchOutcome {
    FetchOutcome {
        url: url.to_owned(),
        status,
        content,
        truncated,
    }
}

fn service(fetch: Option<Arc<dyn FetchProvider>>, max_chars: usize) -> WebService {
    let settings = WebSettings {
        enabled: true,
        fetch_max_chars: max_chars,
        ..WebSettings::default()
    };
    match fetch {
        Some(provider) => WebService::new(settings).with_fetch(provider),
        None => WebService::new(settings),
    }
}

fn fetch_reply(id: &str, url: &str) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: WEB_FETCH_TOOL.to_owned(),
            arguments: serde_json::json!({ "url": url }).to_string(),
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
            session_id: SessionId::new("s-fetch"),
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

    fn decisions(&self) -> Vec<(Decision, heng::events::DecisionSource, Option<String>)> {
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

#[test]
fn the_tool_comes_and_goes_with_the_same_switch_as_the_search() {
    let off = with_web(builtin(false), WebService::new(WebSettings::default()));
    assert!(off.get(WEB_FETCH_TOOL).is_none(), "缺省关着");

    let on = with_web(builtin(false), service(None, 100_000));
    assert!(on.get(WEB_FETCH_TOOL).is_some());
    assert!(
        on.for_executor().get(WEB_FETCH_TOOL).is_some(),
        "执行者也拿到它"
    );
}

#[tokio::test]
async fn a_fetched_page_arrives_as_markdown_with_the_untrusted_marker() {
    let provider = FakeFetch::new(vec![Ok(outcome(
        "https://example.com/page",
        200,
        FetchedContent::Html(
            "<html><body><script>bad()</script><h1>标题</h1><p>正文</p></body></html>".to_owned(),
        ),
        false,
    ))]);
    let mut fixture = fixture(
        vec![
            fetch_reply("call-1", "https://example.com/page"),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(provider.clone())), 100_000),
    )
    .await;
    fixture.run_turn("read it").await;

    let events = fixture.events();
    let output = completed_output(&events, "call-1").unwrap();
    assert!(
        output.starts_with("Fetched https://example.com/page (HTTP 200)"),
        "{output}"
    );
    assert!(output.contains("[外部内容："), "{output}");
    assert!(output.contains("# 标题"), "{output}");
    assert!(output.contains("正文"), "{output}");
    assert!(!output.contains("bad()"), "{output}");
    assert!(!output.contains("<script"), "原始 HTML 绝不进流：{output}");
    assert_eq!(
        provider.calls(),
        vec!["https://example.com/page".to_owned()]
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(&event.payload, EventPayload::ToolCallCompleted { tool_call_id, .. } if tool_call_id.as_str() == "call-1"))
            .count(),
        1,
        "一次调用恰好一条结果"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_non_2xx_status_is_a_result_rather_than_an_error() {
    let provider = FakeFetch::new(vec![Ok(outcome(
        "https://example.com/missing",
        404,
        FetchedContent::Html("<html><body><h1>Not Found</h1></body></html>".to_owned()),
        false,
    ))]);
    let mut fixture = fixture(
        vec![
            fetch_reply("call-1", "https://example.com/missing"),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(provider)), 100_000),
    )
    .await;
    fixture.run_turn("read it").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(
        output.starts_with("Fetched https://example.com/missing (HTTP 404)"),
        "{output}"
    );
    assert!(output.contains("Not Found"), "{output}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_truncated_body_says_so() {
    let provider = FakeFetch::new(vec![Ok(outcome(
        "https://example.com/long",
        200,
        FetchedContent::Text("x".repeat(50)),
        true,
    ))]);
    let mut fixture = fixture(
        vec![
            fetch_reply("call-1", "https://example.com/long"),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(provider)), 50),
    )
    .await;
    fixture.run_turn("read it").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("超过 50 个字符"), "{output}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_blocked_target_is_a_readable_result_with_its_code() {
    let provider = FakeFetch::new(vec![Err(WebError::new(
        heng::web::WebErrorCode::BlockedUrl,
        "`169.254.169.254` 指向 169.254.169.254，那不是公共单播地址",
    ))]);
    let mut fixture = fixture(
        vec![
            fetch_reply("call-1", "http://169.254.169.254/latest/meta-data/"),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(provider)), 100_000),
    )
    .await;
    fixture.run_turn("read it").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("WEB_BLOCKED_URL"), "{output}");
    assert!(output.contains("公共单播"), "{output}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn a_missing_url_argument_is_a_tool_error() {
    let mut fixture = fixture(
        vec![
            Reply::Stream(vec![
                StreamEvent::ToolCallCompleted {
                    index: 0,
                    id: "call-1".to_owned(),
                    name: WEB_FETCH_TOOL.to_owned(),
                    arguments: serde_json::json!({ "url": "  " }).to_string(),
                },
                StreamEvent::Finished {
                    finish_reason: FinishReason::ToolCalls,
                },
            ]),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(FakeFetch::new(Vec::new()))), 100_000),
    )
    .await;
    fixture.run_turn("read it").await;

    let error = completed_output(&fixture.events(), "call-1").unwrap_err();
    assert!(error.contains("`url` 是必填的"), "{error}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn an_oversized_result_spills_to_a_file_like_every_other_tool() {
    let provider = FakeFetch::new(vec![Ok(outcome(
        "https://example.com/huge",
        200,
        FetchedContent::Text("这是一段很长的正文。".repeat(20_000)),
        false,
    ))]);
    let mut fixture = fixture(
        vec![
            fetch_reply("call-1", "https://example.com/huge"),
            Reply::text("done"),
        ],
        Mode::Auto,
        None,
        service(Some(Arc::new(provider)), 100_000_000),
    )
    .await;
    fixture.run_turn("read it").await;

    let events = fixture.events();
    let output = completed_output(&events, "call-1").unwrap();
    assert!(
        output.contains("[已截断："),
        "{}",
        &output[..120.min(output.len())]
    );
    let spilled = fixture
        .harness
        .as_ref()
        .expect("harness 还活着")
        .outputs_dir()
        .join("call-1.txt");
    assert!(spilled.exists(), "全文落在 {}", spilled.display());
    fixture.shutdown().await;
}

#[tokio::test]
async fn readonly_allows_a_fetch_and_ask_does_not_interrupt_it() {
    let readonly_provider = FakeFetch::new(vec![Ok(outcome(
        "https://example.com/",
        200,
        FetchedContent::Text("hi".to_owned()),
        false,
    ))]);
    let mut readonly = fixture(
        vec![
            fetch_reply("call-1", "https://example.com/"),
            Reply::text("done"),
        ],
        Mode::Readonly,
        None,
        service(Some(Arc::new(readonly_provider)), 100_000),
    )
    .await;
    readonly.run_turn("read it").await;
    assert_eq!(readonly.decisions()[0].0, Decision::Allow);
    readonly.shutdown().await;

    let asker = ScriptedAsker::new(Vec::new());
    let provider = FakeFetch::new(vec![Ok(outcome(
        "https://example.com/",
        200,
        FetchedContent::Text("hi".to_owned()),
        false,
    ))]);
    let mut ask = fixture(
        vec![
            fetch_reply("call-1", "https://example.com/"),
            Reply::text("done"),
        ],
        Mode::Ask,
        Some(Arc::new(asker.clone())),
        service(Some(Arc::new(provider)), 100_000),
    )
    .await;
    ask.run_turn("read it").await;
    assert!(
        asker.requests().is_empty(),
        "`ask` 档下一次只读抓取不该打断人：{:?}",
        asker.requests()
    );
    ask.shutdown().await;
}
