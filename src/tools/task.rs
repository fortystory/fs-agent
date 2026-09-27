//! 内建的 `task(brief)` 工具：派出一个执行者（spec §16）。
//!
//! 工具本身只是一层薄壳。一个执行者*是*什么 —— 它的嵌套会话、它自己的预算、传给它的那些权限、
//! 它写下的事件 —— 都属于 `agent` 层（`agent::executor`），那一层驱动回合、拥有事件流。这层壳
//! 的存在是为了让「派出一个执行者」变成一次普通的工具调用：它恰好拿到一条结果、被权限门裁决、
//! 也不需要第二套交付机制。
//!
//! 有两条性质属于这个工具自己：
//!
//! * `effect` 是 [`ReadOnly`](Effect::ReadOnly)，因为 `effect` 分类的是**工作区**副作用
//!   （spec §7），而派发不碰任何工作区路径。正是这一条让一批里的几个 `task` 调用能同时跑；
//!   真正的写互斥发生在执行者自己的那些调用上，经共用的路径锁。
//! * 它不可 [`delegable`](Tool::delegable)，所以执行者的工具表里根本没有 `task`（递归深度为
//!   一，spec §16）。

use async_trait::async_trait;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::tool::{Effect, ExecutorSpawner, Tool, ToolContext, ToolError, ToolOutput};

/// 工具名，只在这里命名一次，好让注册表、循环与测试不会互相漂离。
pub const TASK_TOOL: &str = "task";

/// 派出一个执行者去完成一项任务。
pub struct TaskTool;

#[async_trait]
impl Tool for TaskTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TASK_TOOL.to_owned(),
            description: "Dispatch an executor: a separate agent with its own context and its own \
                          budget that carries out one task in this workspace. Write the brief so it \
                          stands alone — the executor sees the repository rules and your brief, not \
                          this conversation. It reports back a summary; you do not see its steps, its \
                          tool calls or their output. It cannot dispatch further executors."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "brief": {
                        "type": "string",
                        "description": "The task, self-contained: what to do, where, and what the \
                                        summary should report back."
                    }
                },
                "required": ["brief"]
            }),
        }
    }

    /// 派发不碰任何工作区路径：真正可能写的是执行者自己的那些调用，每一个都在路上被门裁决
    /// （spec §16）。
    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    fn delegable(&self) -> bool {
        false
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let brief = args
            .get("brief")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|brief| !brief.is_empty())
            .ok_or_else(|| {
                ToolError::message(format!("{TASK_TOOL}: a non-empty `brief` is required"))
            })?;
        let Some(spawner): Option<&dyn ExecutorSpawner> = ctx.executor else {
            return Err(ToolError::message(format!(
                "{TASK_TOOL}: this session mounted no executor port"
            )));
        };
        spawner.spawn(brief).await
    }
}
