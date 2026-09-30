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
            description: "派一个执行者：一个独立的 agent，有自己的上下文和自己的预算，在这个\
                          工作区里完成一项任务。把 `brief` 写得能独立成立——执行者看到的是仓库\
                          规则和你的 `brief`，不是这段对话。它回报一份摘要；它的步骤、它的工具\
                          调用与那些输出你都看不到。它也不能再派下一个执行者。"
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "brief": {
                        "type": "string",
                        "description": "任务本身，要能独立成立：做什么、在哪里做，以及摘要该\
                                        回报什么"
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
            .ok_or_else(|| ToolError::message(format!("{TASK_TOOL}：需要一个非空的 `brief`")))?;
        let Some(spawner): Option<&dyn ExecutorSpawner> = ctx.executor else {
            return Err(ToolError::message(format!(
                "{TASK_TOOL}：这个会话没有挂执行者端口"
            )));
        };
        spawner.spawn(brief).await
    }
}
