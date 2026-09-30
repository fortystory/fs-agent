//! 内建的 `skill(name)` 工具：按需取一个技能的全文指令。
//!
//! 这是全文加载的唯一漏斗（spec §9）：结果是一条普通的工具结果，所以它被计入用量、受裁剪约束、
//! 并被与其他任何调用同一套权限语言覆盖。它是 [`Effect::ReadOnly`]，因为它不碰任何工作区路径
//! —— 它读的那个库是在组装期、在任何模型给出的路径存在之前就发现好的 —— 所以它在只读模式里
//! 依然可用，也可以并发跑。

use async_trait::async_trait;
use serde_json::Value;

use crate::context::skills::SKILL_TOOL;
use crate::provider::ToolSpec;

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// 按名字取一个已发现技能的正文。
pub struct SkillTool;

#[async_trait]
impl Tool for SkillTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: SKILL_TOOL.to_owned(),
            description: "按名字加载一个技能的全文指令。你上下文里的技能清单列出了可用的名字，\
                          以及每个技能什么时候适用。"
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "要加载的技能名，与技能清单里写的完全一致"
                    }
                },
                "required": ["name"]
            }),
        }
    }

    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| ToolError::message("skill：需要一个非空的 `name`"))?;
        ctx.skills
            .load(name)
            .map(ToolOutput::new)
            .map_err(|error| ToolError::message(error.to_string()))
    }
}
