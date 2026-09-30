//! 内建的 `todo(items)` 工具：模型自己的计划，保存在一个能读回来的地方
//! （`.scratch/todo-and-modes/spec.md` §2）。
//!
//! 形状就是整个设计：一次调用**提交整份列表**，而列表活在那次调用的参数里。由此有三个后果，
//! 它们是这个形状的理由，而不是它的意外：
//!
//! * **不动 schema。** 事件流本来就记录一次工具调用的参数，所以 `--continue`、
//!   `sessions replay`、一次审计、以及侧栏的 `todo` 页都能重算出同一份列表，没有任何新东西要
//!   存 —— 参数是唯一真相源，而结果只是一句回执。
//! * **不问权限。** [`effect`](Tool::effect) 是 [`ReadOnly`](Effect::ReadOnly)，因为 `effect`
//!   分类的是**工作区**副作用（spec §7），而一份活在自己调用参数里的列表不碰任何工作区路径。
//!   所以同一条消息里两次这样的调用会并发跑、按 `seq` 定序，而后落地的那份列表生效 ——
//!   replace-all 正是让「后者胜」有确切定义的东西。
//! * **它归模型所有。** 这份列表是一份工作记录，不是一种权限立场：这里没有任何东西强制模型写
//!   一份，也没有哪个门读它。这就是这个工具与权限模式之间的那条线，也正是
//!   `.scratch/todo-and-modes` 拆开的东西。

use async_trait::async_trait;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// 工具名，只在这里命名一次，好让注册表、工具表、侧栏与测试不会互相漂离。
pub const TODO_TOOL: &str = "todo";

/// 一项处在什么位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// 写下来了，还没开始。
    Pending,
    /// 正在做的那一项（没有任何东西强制最多只有一项）。
    InProgress,
    /// 做完了。
    Completed,
}

impl Status {
    /// 线级拼写：`status` 只接受的这三个词。
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::InProgress => "in_progress",
            Status::Completed => "completed",
        }
    }

    fn parse(word: &str) -> Option<Status> {
        [Status::Pending, Status::InProgress, Status::Completed]
            .into_iter()
            .find(|status| status.as_str() == word)
    }
}

/// 列表里的一项，就是一次调用的参数所携带的样子。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub content: String,
    pub status: Status,
}

/// 那个工具。
pub struct TodoTool;

/// 一份列表里有几项做完了。给两个需要它的读者共用同一个表达式 —— 模型拿到的那句回执与侧栏的
/// 计数行 —— 所以它们对「completed 数的是什么」不可能有分歧。
pub fn completed(items: &[Item]) -> usize {
    items
        .iter()
        .filter(|item| item.status == Status::Completed)
        .count()
}

/// 从一次 `todo` 调用的参数里读出它的列表。
///
/// 这是每个消费者共用的读者 —— 工具自己的 `call`、侧栏那一页、以及以后任何一页 —— 所以形状只
/// 定义一次：工具接受的东西，恰好就是读者读得出的东西。读不出来的列表读成**空**列表（绝不读成
/// 半份列表，也绝不 panic）：下面那个写入者从一开始就会拒掉这样的调用，所以这是一个不许崩的
/// 读者，而不是一个会话能到达的状态 —— 而空列表是那一页本来就有词可说的状态。args 依旧是真相；
/// 回执文本没有人去解析。
pub fn read_items(args: &Value) -> Vec<Item> {
    TodoTool::parse(args).unwrap_or_default()
}

impl TodoTool {
    /// 唯一一次解析，而且是严格的：每个错误都是模型能据以行动的一句话，而带一个坏项的列表会被
    /// **整份**拒掉，而不是被悄悄裁掉毛病 —— 半份提交的列表，是一份模型对之抱有错误信念的列表。
    /// 空列表或没有列表不是错误：那是清空一份列表的方式。
    ///
    /// 为了那些消息，这里是手写而不是用 `serde` 的 derive：derive 出来的错误会说「invalid
    /// type: map, expected a sequence」，却不说模型搞错的是哪个字段。
    fn parse(args: &Value) -> Result<Vec<Item>, ToolError> {
        let Some(object) = args.as_object() else {
            return Err(ToolError::message(format!(
                "{TODO_TOOL}：参数必须是一个带 `items` 数组的 JSON 对象"
            )));
        };
        for key in object.keys() {
            if key != "items" {
                return Err(ToolError::message(format!(
                    "{TODO_TOOL}：不认识的参数 `{key}`；这个工具只收 `items`，别的都不收"
                )));
            }
        }
        let Some(raw) = object.get("items").filter(|value| !value.is_null()) else {
            return Ok(Vec::new());
        };
        let Some(raw) = raw.as_array() else {
            return Err(ToolError::message(format!(
                "{TODO_TOOL}：`items` 必须是一个由 `{{content, status}}` 对象组成的数组；发 \
                 `[]` 可以清空列表"
            )));
        };

        let mut items = Vec::with_capacity(raw.len());
        for (index, raw) in raw.iter().enumerate() {
            let Some(object) = raw.as_object() else {
                return Err(ToolError::message(format!(
                    "{TODO_TOOL}：第 {index} 项不是一个对象；每一项都是 `{{content, status}}`"
                )));
            };
            for key in object.keys() {
                if key != "content" && key != "status" {
                    return Err(ToolError::message(format!(
                        "{TODO_TOOL}：第 {index} 项有一个不认识的字段 `{key}`；每一项都是 \
                         `{{content, status}}`"
                    )));
                }
            }
            let content = object
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_owned();
            if content.is_empty() {
                return Err(ToolError::message(format!(
                    "{TODO_TOOL}：第 {index} 项需要一个非空的 `content`——一行，说这一项是\
                     什么"
                )));
            }
            let status = match object.get("status").and_then(Value::as_str) {
                Some(word) => Status::parse(word).ok_or_else(|| {
                    ToolError::message(format!(
                        "{TODO_TOOL}：第 {index} 项的 `status` 是 `{word}`；只有 `pending`、\
                         `in_progress`、`completed` 这三个词"
                    ))
                })?,
                None => {
                    return Err(ToolError::message(format!(
                        "{TODO_TOOL}：第 {index} 项需要一个 `status`，取 `pending`、\
                         `in_progress` 或 `completed`"
                    )))
                }
            };
            items.push(Item { content, status });
        }
        Ok(items)
    }
}

/// 回执：说给模型的、这次调用做了什么。很短、是中文（模型可见文本，ADR 0005），而且刻意**不是**
/// 那份列表 —— 列表就是这次调用的参数，在这里重复一遍会造出第二份可能漂移的副本。
fn receipt(items: &[Item]) -> String {
    if items.is_empty() {
        return format!("{TODO_TOOL}：已清空");
    }
    format!(
        "{TODO_TOOL}：{} 项（{} 项已完成）",
        items.len(),
        completed(items)
    )
}

#[async_trait]
impl Tool for TodoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TODO_TOOL.to_owned(),
            description: "把你正在照做的计划记下来：一份带状态的待办列表。一次调用提交整份列表，并\
                          替换掉前一份，所以每次都要把所有项都发过来：不写 `items`（或发 `[]`）就\
                          是清空列表。每一项是 `{content, status}`，`status` 取 `pending`、\
                          `in_progress`、`completed` 之一；`content` 必须是非空的一行。结果是一行\
                          回执——列表本身就在这次调用的参数里，你和用户都从那里读回它。"
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "description": "整份列表，按它该被读的顺序。不写它（或发 `[]`）就是\
                                        清空列表",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": {
                                    "type": "string",
                                    "description": "一行，说这一项是什么"
                                },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed"],
                                    "description": "`pending` 是写下来了，`in_progress` 是正在\
                                                    做，`completed` 是做完了"
                                }
                            },
                            "required": ["content", "status"]
                        }
                    }
                }
            }),
        }
    }

    /// 一份活在自己调用参数里的列表不写任何工作区路径（spec §7），所以这是一次读 —— 也正是这
    /// 一条让两次这样的调用能并发跑、按 `seq` 定序。
    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    fn delegable(&self) -> bool {
        // 这是默认值，但说出来，因为这是一条决定：执行者保有自己的那份列表（spec §16）。派发者
        // 的列表与它执行者的列表是两份列表 —— 侧栏显示主会话那份，转录两份都显示。
        true
    }

    async fn call(&self, _ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let items = Self::parse(&args)?;
        Ok(ToolOutput::new(receipt(&items)))
    }
}
