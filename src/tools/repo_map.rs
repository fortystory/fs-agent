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
            description: "画出这个工作区的 Rust 符号地图：每个文件，以及它定义的函数、类型、\
                          trait、模块与宏。探索不熟的仓库时先调它。传 `focus`（标识符、某个\
                          子系统，或一条路径）可以把地图偏向代码的某一部分。地图按本次会话最近\
                          读过或提过的东西排序，并由一个固定预算封顶；它是这些文件此刻的快照。"
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "focus": {
                        "type": "string",
                        "description": "可选，用来把地图偏向某些词：标识符、某个子系统或一条路径。\
                                        没有 `tokens` 参数；预算是配置"
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
                "{} 底下没有找到任何 Rust 符号",
                ctx.cwd.display()
            )));
        }
        Ok(ToolOutput::new(map))
    }
}
