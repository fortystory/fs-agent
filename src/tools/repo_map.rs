//! 内建的 `repo_map(focus?)` 工具：按需给出工作区的符号。
//!
//! 与 `skill(name)` 同一个形状（spec §9）：一个内建的**只读**工具，产物是一条普通的工具结果，
//! 所以它被同一套机器计入用量、裁剪、丢掉。它刻意**不**注入：注入版的地图会随文件变化而需要
//! 刷新，而每次刷新都会把它之后的历史挤出缓存前缀。
//!
//! 它不读任何模型给出的工作区路径 —— 它自己走会话 cwd —— 所以 `effect()` 是
//! [`Effect::ReadOnly`]，也不声明读路径，这使它在只读模式里依旧可用。

use async_trait::async_trait;
use serde_json::Value;

use crate::context::repo_map::{RepoMap, REPO_MAP_TOOL};
use crate::provider::ToolSpec;

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// 在配置的预算内渲染工作区的符号地图。
pub struct RepoMapTool {
    map: RepoMap,
}

impl RepoMapTool {
    /// 在组装工具表时把官方 tags 查询编译一次。
    pub fn new() -> Self {
        Self {
            map: RepoMap::new(),
        }
    }
}

impl Default for RepoMapTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for RepoMapTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: REPO_MAP_TOOL.to_owned(),
            description: "Map this workspace's Rust symbols: each file and the functions, types, \
                          traits, modules and macros it defines. Call it first when exploring an \
                          unfamiliar repository. Pass `focus` (identifiers, a subsystem, a path) \
                          to bias the map toward one part of the code. The map is ranked by what \
                          this session recently read or mentioned and is capped by a fixed \
                          budget; it is a snapshot of the files as they are now."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "focus": {
                        "type": "string",
                        "description": "Optional words to bias the map toward: identifiers, a subsystem, or a path. There is no `tokens` argument; the budget is configuration."
                    }
                }
            }),
        }
    }

    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        // 预算是配置，绝不是参数：模型硬塞来的 `tokens` 键会被忽略，而不是被采纳。
        let focus = args
            .get("focus")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|focus| !focus.is_empty());
        let context = ctx.repo_map.context.clone().with_focus(focus);
        let map = self.map.build(ctx.cwd, &context, ctx.repo_map.tokens);
        if map.is_empty() {
            return Ok(ToolOutput::new(format!(
                "no Rust symbols found under {}",
                ctx.cwd.display()
            )));
        }
        Ok(ToolOutput::new(map))
    }
}
