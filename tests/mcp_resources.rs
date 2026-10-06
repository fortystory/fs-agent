//! 两个资源元工具：`mcp_resources(server?)` 与 `mcp_read(server, uri)`
//! （`.scratch/mcp-support/spec.md` §8；票 16）。
//!
//! 两件要钉住的事：资源是 **URI 坐标系**里的东西（读它不影响工作区的读集合），以及它与工具
//! 是两条分开的入口（形状不同）。

mod support;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use heng::config::{McpServerConfig, McpSettings};
use heng::events::{read_events, Decision, Event, EventPayload, SessionId, SpeakerId};
use heng::mcp::{McpConnection, McpError, McpService, ResourceSummary};
use heng::permissions::{Mode, Policy};
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{builtin, with_mcp, MCP_READ_TOOL, MCP_RESOURCES_TOOL, READ_BEFORE_WRITE_PREFIX};
use heng::{assemble, AssemblyParts, Harness, SessionScaffold};
use serde_json::json;
use support::{CaptureBuf, FakeProvider, Reply};

// --- 假的 MCP 连接 --------------------------------------------------------

#[derive(Clone, Default)]
struct FakeMcp {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    resources: Vec<ResourceSummary>,
    contents: HashMap<String, String>,
    reads: Mutex<Vec<String>>,
    lists: Mutex<usize>,
}

impl FakeMcp {
    fn with_resource(uri: &str, name: &str, body: &str) -> Self {
        let mut fake = Self::default();
        let inner = Arc::get_mut(&mut fake.inner).expect("刚建出来，没人共享它");
        inner.resources.push(ResourceSummary {
            uri: uri.to_owned(),
            name: Some(name.to_owned()),
            description: Some("一份外部数据".to_owned()),
            mime_type: Some("text/plain".to_owned()),
        });
        inner.contents.insert(uri.to_owned(), body.to_owned());
        fake
    }

    fn reads(&self) -> Vec<String> {
        self.inner.reads.lock().expect("假连接已中毒").clone()
    }

    fn list_calls(&self) -> usize {
        *self.inner.lists.lock().expect("假连接已中毒")
    }
}

#[async_trait]
impl McpConnection for FakeMcp {
    async fn list_tools(&self) -> Result<heng::mcp::ServerManifest, McpError> {
        Err(McpError::unsupported("列工具"))
    }

    async fn list_resources(&self) -> Result<Vec<ResourceSummary>, McpError> {
        *self.inner.lists.lock().expect("假连接已中毒") += 1;
        Ok(self.inner.resources.clone())
    }

    async fn read_resource(&self, uri: &str) -> Result<String, McpError> {
        self.inner
            .reads
            .lock()
            .expect("假连接已中毒")
            .push(uri.to_owned());
        self.inner
            .contents
            .get(uri)
            .cloned()
            .ok_or_else(|| McpError::unknown_resource("db", uri))
    }
}

fn settings() -> McpSettings {
    McpSettings {
        enabled: true,
        connect_timeout_ms: 10_000,
        servers: BTreeMap::from([(
            "db".to_owned(),
            McpServerConfig::stdio("db", vec!["true".to_owned()]),
        )]),
    }
}

fn service(connection: Option<Arc<dyn McpConnection>>) -> McpService {
    match connection {
        Some(connection) => McpService::new(settings()).with_connection("db", connection),
        None => McpService::new(settings()),
    }
}

// --- 会话 fixture ---------------------------------------------------------

fn tool_call(id: &str, name: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: name.to_owned(),
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

struct Fixture {
    harness: Option<Harness>,
    log_path: PathBuf,
    workspace: PathBuf,
    /// 在整个测试期间保持活着。
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, mcp: McpService) -> Fixture {
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
        config: heng::config::SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-mcp-res"),
            tools: with_mcp(builtin(false), mcp),
            locks: heng::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
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
        workspace,
        _dir: dir,
    }
}

impl Fixture {
    fn events(&self) -> Vec<Event> {
        read_events(&self.log_path).unwrap()
    }

    fn decisions(&self) -> Vec<Decision> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::PermissionDecided { decision, .. } => Some(*decision),
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

// --- 形状 -----------------------------------------------------------------

#[test]
fn the_two_resource_tools_share_the_one_switch() {
    let off = with_mcp(builtin(false), McpService::new(McpSettings::default()));
    assert!(off.get(MCP_RESOURCES_TOOL).is_none());
    assert!(off.get(MCP_READ_TOOL).is_none());

    let on = with_mcp(builtin(false), service(None));
    assert!(on.get(MCP_RESOURCES_TOOL).is_some());
    assert!(on.get(MCP_READ_TOOL).is_some());
}

#[tokio::test]
async fn resources_are_listed_and_read_with_the_untrusted_marker() {
    let connection = FakeMcp::with_resource("db://users/42", "user 42", "{\"id\":42}");
    let mut fixture = fixture(
        vec![
            tool_call("call-1", MCP_RESOURCES_TOOL, json!({ "server": "db" })),
            tool_call(
                "call-2",
                MCP_READ_TOOL,
                json!({ "server": "db", "uri": "db://users/42" }),
            ),
            Reply::text("done"),
        ],
        service(Some(Arc::new(connection.clone()))),
    )
    .await;
    fixture.run_turn("look at the data").await;

    let list = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(list.contains("db://users/42"), "{list}");
    assert!(list.contains("user 42"), "{list}");
    assert!(list.starts_with("[外部内容："), "{list}");

    let read = completed_output(&fixture.events(), "call-2").unwrap();
    assert!(read.starts_with("[外部内容："), "{read}");
    assert!(read.contains("{\"id\":42}"), "{read}");
    assert_eq!(connection.reads(), vec!["db://users/42".to_owned()]);
    fixture.shutdown().await;
}

#[tokio::test]
async fn listing_resources_asks_the_server_again() {
    let connection = FakeMcp::with_resource("db://a", "a", "1");
    let mut fixture = fixture(
        vec![
            tool_call("call-1", MCP_RESOURCES_TOOL, json!({ "server": "db" })),
            tool_call("call-2", MCP_RESOURCES_TOOL, json!({})),
            Reply::text("done"),
        ],
        service(Some(Arc::new(connection.clone()))),
    )
    .await;
    fixture.run_turn("list twice").await;

    assert_eq!(connection.list_calls(), 2, "与工具清单同一条规矩：每次现问");
    fixture.shutdown().await;
}

// --- 资源不进读集合 -------------------------------------------------------

#[tokio::test]
async fn reading_a_resource_does_not_unlock_a_workspace_edit() {
    let note = "note.txt";
    let connection = FakeMcp::with_resource("db://users/42", "user 42", "{\"id\":42}");
    let mut fixture = fixture(
        vec![
            // 先读一份资源 —— 它不在工作区坐标系里。
            tool_call(
                "call-1",
                MCP_READ_TOOL,
                json!({ "server": "db", "uri": "db://users/42" }),
            ),
            // 于是改一个没读过的文件仍然被「改前先读」拒。
            tool_call(
                "call-2",
                heng::tools::EDIT_FILE,
                json!({ "file_path": note, "old_string": "旧", "new_string": "新" }),
            ),
            // 真读一遍之后才放行。
            tool_call(
                "call-3",
                heng::tools::READ_FILE,
                json!({ "file_path": note }),
            ),
            tool_call(
                "call-4",
                heng::tools::EDIT_FILE,
                json!({ "file_path": note, "old_string": "旧", "new_string": "新" }),
            ),
            Reply::text("done"),
        ],
        service(Some(Arc::new(connection))),
    )
    .await;
    std::fs::write(fixture.workspace.join(note), "旧内容").unwrap();
    fixture.run_turn("edit it").await;

    let events = fixture.events();
    let refused = completed_output(&events, "call-2").unwrap_err();
    assert!(refused.contains(READ_BEFORE_WRITE_PREFIX), "{refused}");
    let allowed = completed_output(&events, "call-4").unwrap();
    assert!(
        allowed.contains("已写入"),
        "成功的写入有一条回执：{allowed}"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.workspace.join(note)).unwrap(),
        "新内容"
    );
    fixture.shutdown().await;
}

// --- 失败 -----------------------------------------------------------------

#[tokio::test]
async fn an_unknown_uri_is_a_readable_result() {
    let connection = FakeMcp::with_resource("db://users/42", "user 42", "{}");
    let mut fixture = fixture(
        vec![
            tool_call(
                "call-1",
                MCP_READ_TOOL,
                json!({ "server": "db", "uri": "db://nope" }),
            ),
            Reply::text("done"),
        ],
        service(Some(Arc::new(connection))),
    )
    .await;
    fixture.run_turn("read it").await;

    let output = completed_output(&fixture.events(), "call-1").unwrap();
    assert!(output.contains("MCP_UNKNOWN_RESOURCE"), "{output}");
    assert!(output.contains("db://nope"), "{output}");
    fixture.shutdown().await;
}

#[tokio::test]
async fn an_unknown_server_is_a_readable_result_on_both_entries() {
    let mut fixture = fixture(
        vec![
            tool_call("call-1", MCP_RESOURCES_TOOL, json!({ "server": "nope" })),
            tool_call(
                "call-2",
                MCP_READ_TOOL,
                json!({ "server": "nope", "uri": "db://a" }),
            ),
            Reply::text("done"),
        ],
        service(None),
    )
    .await;
    fixture.run_turn("read it").await;

    let events = fixture.events();
    for id in ["call-1", "call-2"] {
        let output = completed_output(&events, id).unwrap();
        assert!(output.contains("MCP_UNKNOWN_SERVER"), "{id}：{output}");
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn both_entries_are_read_only_in_every_mode() {
    let connection = FakeMcp::with_resource("db://a", "a", "1");
    let mut fixture = fixture(
        vec![
            tool_call("call-1", MCP_RESOURCES_TOOL, json!({ "server": "db" })),
            tool_call(
                "call-2",
                MCP_READ_TOOL,
                json!({ "server": "db", "uri": "db://a" }),
            ),
            Reply::text("done"),
        ],
        service(Some(Arc::new(connection))),
    )
    .await;
    fixture.run_turn("read them").await;

    assert_eq!(
        fixture.decisions(),
        vec![Decision::Allow, Decision::Allow],
        "两个入口都是只读，四档模式全放行"
    );
    fixture.shutdown().await;
}
