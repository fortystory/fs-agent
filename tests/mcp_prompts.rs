//! 提示词模板：服务层那一侧与「不进工具表」这条边界（`.scratch/mcp-support/spec.md` §8；
//! 票 17）。
//!
//! 模板的发起者是**人**：它进 `/` 菜单（`src/cli.rs` 的 `slash_catalog` 与 `submission` 各有一
//! 条单元测试），不进工具表、不进前缀缓存 —— 这个文件钉住的正是后面那一条。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_agent::config::{McpServerConfig, McpSettings};
use fs_agent::mcp::{
    McpConnection, McpError, McpService, PromptArgument, PromptSummary, ServerManifest,
};
use fs_agent::tools::{
    builtin, with_mcp, MCP_CALL_TOOL, MCP_LIST_TOOL, MCP_READ_TOOL, MCP_RESOURCES_TOOL,
};

// --- 假的 MCP 连接 --------------------------------------------------------

#[derive(Clone, Default)]
struct FakeMcp {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    prompts: Vec<PromptSummary>,
    /// 每一次 `prompts/get` 收到的（模板名，实参）。
    calls: Mutex<Vec<(String, serde_json::Value)>>,
    /// `prompts/list` 被问了几次。
    list_calls: Mutex<usize>,
    /// 为真时列模板报错 —— 「这台 server 用不了」。
    broken: bool,
}

impl FakeMcp {
    fn with_prompts(prompts: Vec<PromptSummary>) -> Self {
        let mut fake = Self::default();
        Arc::get_mut(&mut fake.inner)
            .expect("刚建出来，没人共享它")
            .prompts = prompts;
        fake
    }

    fn broken() -> Self {
        let mut fake = Self::default();
        Arc::get_mut(&mut fake.inner)
            .expect("刚建出来，没人共享它")
            .broken = true;
        fake
    }

    fn calls(&self) -> Vec<(String, serde_json::Value)> {
        self.inner.calls.lock().expect("假连接已中毒").clone()
    }

    fn list_calls(&self) -> usize {
        *self.inner.list_calls.lock().expect("假连接已中毒")
    }
}

#[async_trait]
impl McpConnection for FakeMcp {
    async fn list_tools(&self) -> Result<ServerManifest, McpError> {
        Err(McpError::unsupported("列工具"))
    }

    async fn list_prompts(&self) -> Result<Vec<PromptSummary>, McpError> {
        *self.inner.list_calls.lock().expect("假连接已中毒") += 1;
        if self.inner.broken {
            return Err(McpError::provider_error("fake", "这一台用不了"));
        }
        Ok(self.inner.prompts.clone())
    }

    async fn get_prompt(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        self.inner
            .calls
            .lock()
            .expect("假连接已中毒")
            .push((name.to_owned(), arguments.clone()));
        let id = arguments
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("(没有)");
        Ok(format!("user {id} 的报告"))
    }
}

// --- 组装 -----------------------------------------------------------------

fn template(name: &str, required: bool) -> PromptSummary {
    PromptSummary {
        name: name.to_owned(),
        description: Some("按 id 出一份报告".to_owned()),
        arguments: vec![PromptArgument {
            name: "id".to_owned(),
            description: Some("用户 id".to_owned()),
            required,
        }],
    }
}

fn settings(servers: Vec<(&str, Arc<dyn McpConnection>)>) -> (McpSettings, McpService) {
    let mut configured = BTreeMap::new();
    for (name, _) in &servers {
        configured.insert(
            (*name).to_owned(),
            McpServerConfig::stdio(*name, vec!["true".to_owned()]),
        );
    }
    let settings = McpSettings {
        enabled: true,
        connect_timeout_ms: 10_000,
        servers: configured,
    };
    let mut service = McpService::new(settings.clone());
    for (name, connection) in servers {
        service = service.with_connection(name, connection);
    }
    (settings, service)
}

// --- 清单与渲染 -----------------------------------------------------------

#[tokio::test]
async fn templates_are_listed_per_server_and_rendered_with_the_arguments() {
    let db = FakeMcp::with_prompts(vec![template("user_report", true)]);
    let jira = FakeMcp::broken();
    let (_, service) = settings(vec![("db", Arc::new(db.clone())), ("jira", Arc::new(jira))]);

    // 菜单那一半：拍的清单里只有 db —— jira 那台列不出来，于是它的条目根本不出现。
    let entries = service.prompt_entries().await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].0, "db");
    assert_eq!(entries[0].1.name, "user_report");
    assert_eq!(entries[0].1.arguments[0].name, "id");
    assert!(entries[0].1.arguments[0].required);

    // 每次现问，与工具清单同一条规矩。
    service.prompt_entries().await;
    assert_eq!(db.list_calls(), 2);

    // 取一份：实参原样过去，正文回得来。
    let text = service
        .get_prompt("db", "user_report", serde_json::json!({ "id": "42" }))
        .await
        .unwrap();
    assert_eq!(text, "user 42 的报告");
    assert_eq!(
        db.calls(),
        vec![("user_report".to_owned(), serde_json::json!({ "id": "42" }))]
    );
}

#[tokio::test]
async fn an_unavailable_server_contributes_no_menu_entries() {
    // 名字在配置里，但连接没建起来 —— 这条路等价于「这台没连上」。
    let settings = McpSettings {
        enabled: true,
        connect_timeout_ms: 10_000,
        servers: BTreeMap::from([(
            "db".to_owned(),
            McpServerConfig::stdio("db", vec!["true".to_owned()]),
        )]),
    };
    let service = McpService::new(settings).with_unavailable("db", "起不来");
    assert!(service.prompt_entries().await.is_empty());

    // 显式问那一台时，错误仍然是结构化、可读的。
    let error = service
        .get_prompt("db", "user_report", serde_json::json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code_str(), "MCP_SERVER_UNAVAILABLE");
}

#[tokio::test]
async fn get_prompt_refuses_an_unknown_server_by_name() {
    let (_, service) = settings(vec![("db", Arc::new(FakeMcp::default()))]);
    let error = service
        .get_prompt("nope", "user_report", serde_json::json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code_str(), "MCP_UNKNOWN_SERVER");
}

// --- 不进工具表 -----------------------------------------------------------

#[test]
fn the_templates_never_reach_the_tool_table() {
    let (_, service) = settings(vec![("db", Arc::new(FakeMcp::default()))]);
    let table = with_mcp(builtin(false), service);
    for name in [
        MCP_LIST_TOOL,
        MCP_CALL_TOOL,
        MCP_RESOURCES_TOOL,
        MCP_READ_TOOL,
    ] {
        assert!(table.get(name).is_some(), "{name} 该在表里");
    }
    let specs = table.specs();
    assert!(
        specs
            .iter()
            .all(|spec| !spec.name.contains("prompt") && !spec.name.contains("template")),
        "模型看不到模板：{:?}",
        specs.iter().map(|spec| &spec.name).collect::<Vec<_>>()
    );
}
