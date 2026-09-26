//! The built-in `todo(items)` tool: the model's own plan, kept where it can be
//! read back (`.scratch/todo-and-modes/spec.md` §2).
//!
//! The shape is the whole design: a call **submits the entire list**, and the
//! list lives in that call's arguments. Three consequences follow, and they are
//! the reason for the shape rather than accidents of it:
//!
//! * **no schema change.** The event stream already records a tool call's
//!   arguments, so `--continue`, `sessions replay`, an audit and the sidebar's
//!   `todo` page all recompute the same list with nothing new to store — the
//!   arguments are the single source of truth and the result is only a receipt.
//! * **no permission question.** [`effect`](Tool::effect) is
//!   [`ReadOnly`](Effect::ReadOnly), because `effect` classifies **workspace**
//!   side effects (spec §7) and a list that lives in the call's own arguments
//!   touches no workspace path. Two of these in one message therefore run
//!   concurrently, ordered by `seq`, and the later one's list is the one in force
//!   — replace-all is what makes "last one wins" well defined.
//! * **the model owns it.** The list is a working record, not a permission stance:
//!   nothing here forces the model to write one, and no gate reads it. That is the
//!   line between this tool and the permission modes, which is what
//!   `.scratch/todo-and-modes` split apart.

use async_trait::async_trait;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// The tool name, named once so the registry, the table, the sidebar and the
/// tests cannot drift apart.
pub const TODO_TOOL: &str = "todo";

/// Where one item stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Written down, not started.
    Pending,
    /// The one being worked on (nothing enforces that there is at most one).
    InProgress,
    /// Done.
    Completed,
}

impl Status {
    /// The wire spelling: the only three words `status` accepts.
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

/// One item of a list, as a call's arguments carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub content: String,
    pub status: Status,
}

/// The tool.
pub struct TodoTool;

/// How many of a list's items are done. One expression for the two readers that
/// need it — the receipt the model gets and the sidebar's count row — so they
/// cannot disagree about what "completed" counts.
pub fn completed(items: &[Item]) -> usize {
    items
        .iter()
        .filter(|item| item.status == Status::Completed)
        .count()
}

/// Read a `todo` call's list out of its arguments.
///
/// This is the reader every consumer shares — the tool's own `call`, the sidebar's
/// page and any later one — so the shape is defined once: what the tool accepts is
/// exactly what a reader can read. A list it cannot read reads as an **empty** one
/// (never half a list, and never a panic): the writer below refuses to record such
/// a call in the first place, so this is a reader that must not break rather than a
/// state a session can reach — and an empty list is a state the page already has a
/// word for. The args stay the truth; the receipt text is parsed by nobody.
pub fn read_items(args: &Value) -> Vec<Item> {
    TodoTool::parse(args).unwrap_or_default()
}

impl TodoTool {
    /// The one parse, and it is strict: every mistake is a message the model can
    /// act on, and a list with one bad item is refused **whole** rather than
    /// quietly trimmed — a half-submitted list is a list the model believes
    /// something false about. An empty or absent list is not a mistake: it is how
    /// a list is cleared.
    ///
    /// Written by hand rather than with `serde`'s derive for the sake of those
    /// messages: a derived error says "invalid type: map, expected a sequence"
    /// without naming the field the model got wrong.
    fn parse(args: &Value) -> Result<Vec<Item>, ToolError> {
        let Some(object) = args.as_object() else {
            return Err(ToolError::message(format!(
                "{TODO_TOOL}: the arguments must be a JSON object with an `items` array"
            )));
        };
        for key in object.keys() {
            if key != "items" {
                return Err(ToolError::message(format!(
                    "{TODO_TOOL}: unknown argument `{key}`; this tool takes `items` and nothing \
                     else"
                )));
            }
        }
        let Some(raw) = object.get("items").filter(|value| !value.is_null()) else {
            return Ok(Vec::new());
        };
        let Some(raw) = raw.as_array() else {
            return Err(ToolError::message(format!(
                "{TODO_TOOL}: `items` must be an array of `{{content, status}}` objects; send \
                 `[]` to clear the list"
            )));
        };

        let mut items = Vec::with_capacity(raw.len());
        for (index, raw) in raw.iter().enumerate() {
            let Some(object) = raw.as_object() else {
                return Err(ToolError::message(format!(
                    "{TODO_TOOL}: item {index} is not an object; every item is \
                     `{{content, status}}`"
                )));
            };
            for key in object.keys() {
                if key != "content" && key != "status" {
                    return Err(ToolError::message(format!(
                        "{TODO_TOOL}: item {index} has an unknown field `{key}`; every item is \
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
                    "{TODO_TOOL}: item {index} needs a non-empty `content` — one line saying what \
                     it is"
                )));
            }
            let status = match object.get("status").and_then(Value::as_str) {
                Some(word) => Status::parse(word).ok_or_else(|| {
                    ToolError::message(format!(
                        "{TODO_TOOL}: item {index} has `status` = `{word}`; the three words are \
                         `pending`, `in_progress` and `completed`"
                    ))
                })?,
                None => {
                    return Err(ToolError::message(format!(
                        "{TODO_TOOL}: item {index} needs a `status` of `pending`, `in_progress` \
                         or `completed`"
                    )))
                }
            };
            items.push(Item { content, status });
        }
        Ok(items)
    }
}

/// The receipt: what the model is told the call did. Short, English (model-visible
/// text, ADR 0001) and deliberately **not** the list — the list is the call's
/// arguments, and repeating it here would create a second copy that can drift.
fn receipt(items: &[Item]) -> String {
    if items.is_empty() {
        return format!("{TODO_TOOL}: cleared");
    }
    let noun = if items.len() == 1 { "item" } else { "items" };
    format!(
        "{TODO_TOOL}: {} {noun} ({} completed)",
        items.len(),
        completed(items)
    )
}

#[async_trait]
impl Tool for TodoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: TODO_TOOL.to_owned(),
            description: "Record the plan you are working to, as a list of items with their \
                          status. One call submits the whole list and replaces the previous one, \
                          so send every item each time: leave `items` out (or send an empty \
                          array) to clear the list. Each item is `{content, status}` with \
                          `status` one of `pending`, `in_progress`, `completed`; `content` must \
                          be a non-empty line. The result is a one-line receipt — the list itself \
                          is this call's arguments, which is where you and the user read it back \
                          from."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "description": "The whole list, in the order it should be read. Omit it \
                                        (or send `[]`) to clear the list.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": {
                                    "type": "string",
                                    "description": "One line saying what the item is."
                                },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed"],
                                    "description": "`pending` is written down, `in_progress` is \
                                                    being worked on, `completed` is done."
                                }
                            },
                            "required": ["content", "status"]
                        }
                    }
                }
            }),
        }
    }

    /// A list that lives in the call's own arguments writes no workspace path
    /// (spec §7), so this is a read — and that is what lets two of them run
    /// concurrently, ordered by `seq`.
    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    fn delegable(&self) -> bool {
        // The default, said out loud because it is a decision: an executor keeps
        // its own list (spec §16). A dispatcher's list and its executor's are two
        // lists — the sidebar shows the main session's, the transcript shows both.
        true
    }

    async fn call(&self, _ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let items = Self::parse(&args)?;
        Ok(ToolOutput::new(receipt(&items)))
    }
}
