//! 内建的 `goal_note(notes)` 工具：把执行中冒出来的新工作记下来
//! （`.scratch/goal-loop/spec.md` §11）。
//!
//! 目标清单是**封闭**的 —— 执行中不能往里加条目，否则「所有条目都完成」就不再是一个稳定的
//! 判据。所以「我发现还需要做 X」必须另有一个出口，否则它只能活在模型的上下文里，而上下文
//! 过八成就要被压成摘要。
//!
//! 形状与 [`crate::tools::todo`] **完全同构**，理由也一样：
//!
//! * **args 即真相。** 一条 note 活在那次调用的参数里，所以 `sessions replay` 能重算、
//!   `--continue` 后自然重建，不为它新增任何事件、也不动 schema。
//! * **不问权限。** [`effect`](Tool::effect) 是 [`ReadOnly`](Effect::ReadOnly)，因为 `effect`
//!   分类的是**工作区**副作用，而一份活在自己调用参数里的记录不碰任何工作区路径 —— 于是两次
//!   这样的调用可以并发，无人值守时也不会停下来等人。
//! * **结果只是一句回执。** 与 `todo` 一样，名单本身就在参数里，回执再抄一遍会造出第二份可能
//!   漂移的副本。
//!
//! 消费者是目标收尾时的那份汇总（§11）：清单是封闭的，新工作只能在那里交代。

use async_trait::async_trait;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// 工具名，只在这里命名一次，好让注册表、工具表与测试不会互相漂离。
pub const GOAL_NOTE_TOOL: &str = "goal_note";

/// 那个工具。
pub struct GoalNoteTool;

/// 从一次 `goal_note` 调用的参数里读出它的那些 note。
///
/// 与 [`crate::tools::todo::read_items`] 同一个形状的读者：读不出来的调用读成**空**（绝不读成
/// 半份，也绝不 panic）。下面那个写入者从一开始就会拒掉这样的调用，所以这是一个不许崩的读者；
/// 而空是这一页本来就说得通的状态 —— 他还什么都没记。
pub fn read_notes(args: &Value) -> Vec<String> {
    GoalNoteTool::parse(args).unwrap_or_default()
}

impl GoalNoteTool {
    /// 唯一一次解析，而且是严格的：每个错误都是模型能据以行动的一句话。一次调用里的 note 有一
    /// 条坏，整次就被拒 —— 半份记下的清单，是一份模型对之抱有错误信念的清单。
    ///
    /// 手写而不是 `serde` derive，理由与 `todo` 相同：derive 的错误会说「invalid type」，却不说
    /// 模型搞错的是哪一项。
    fn parse(args: &Value) -> Result<Vec<String>, ToolError> {
        let Some(object) = args.as_object() else {
            return Err(ToolError::message(format!(
                "{GOAL_NOTE_TOOL}：参数必须是一个带 `notes` 数组的 JSON 对象"
            )));
        };
        for key in object.keys() {
            if key != "notes" {
                return Err(ToolError::message(format!(
                    "{GOAL_NOTE_TOOL}：不认识的参数 `{key}`；这个工具只收 `notes`，别的都不收"
                )));
            }
        }
        let Some(raw) = object.get("notes").filter(|value| !value.is_null()) else {
            return Err(ToolError::message(format!(
                "{GOAL_NOTE_TOOL}：需要 `notes`：一个由字符串组成的数组，一行一条新工作"
            )));
        };
        let Some(raw) = raw.as_array() else {
            return Err(ToolError::message(format!(
                "{GOAL_NOTE_TOOL}：`notes` 必须是一个字符串数组，一行一条；读到的是\
                 `{}`",
                raw
            )));
        };

        let mut notes = Vec::with_capacity(raw.len());
        for (index, raw) in raw.iter().enumerate() {
            let Some(note) = raw.as_str() else {
                return Err(ToolError::message(format!(
                    "{GOAL_NOTE_TOOL}：第 {index} 条不是字符串；每一条都是一行说清那件新工作的话"
                )));
            };
            let note = note.trim();
            if note.is_empty() {
                return Err(ToolError::message(format!(
                    "{GOAL_NOTE_TOOL}：第 {index} 条是空的；一行说清那件新工作是什么"
                )));
            }
            notes.push(note.to_owned());
        }
        if notes.is_empty() {
            return Err(ToolError::message(format!(
                "{GOAL_NOTE_TOOL}：`notes` 是空的；至少记一条，一行说清那件新工作"
            )));
        }
        Ok(notes)
    }
}

/// 回执：说给模型的、这次调用做了什么。刻意**不是**那份名单 —— 名单就是这次调用的参数。
fn receipt(notes: &[String]) -> String {
    format!("{GOAL_NOTE_TOOL}：记下 {} 条", notes.len())
}

#[async_trait]
impl Tool for GoalNoteTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: GOAL_NOTE_TOOL.to_owned(),
            description: "记下执行中冒出来的新工作。目标清单是封闭的 —— 你不能往里加条目 —— 所以\
                          需要用 `goal_note` 把这些事记下来：一行一条，写成一次调用的 `notes` \
                          数组。它们会进最后那份收尾汇总，于是清单之外还做了什么有人交代。\
                          结果是一行回执——名单本身就在这次调用的参数里。"
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "notes": {
                        "type": "array",
                        "description": "要记下的新工作，一行一条。至少一条",
                        "items": {
                            "type": "string",
                            "description": "一行，说清那件新工作是什么"
                        }
                    }
                },
                "required": ["notes"]
            }),
        }
    }

    /// 一份活在自己调用参数里的记录不写任何工作区路径（spec §7），所以这是一次读 —— 也正是这
    /// 一条让两次这样的调用能并发跑、按 `seq` 定序。
    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    fn delegable(&self) -> bool {
        // 这是默认值，但说出来，因为这是一条决定：执行者也该能记它自己撞见的新工作（spec §16）。
        true
    }

    async fn call(&self, _ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let notes = Self::parse(&args)?;
        Ok(ToolOutput::new(receipt(&notes)))
    }
}
