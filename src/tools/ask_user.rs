//! 内建的 `ask_user_question(questions)` 工具：模型向用户提问的方式（spec §7）。
//!
//! 这个工具是包在 [`UserQuestions`] 端口外面的一层薄壳，正如 `task` 是包在
//! [`ExecutorSpawner`](crate::tools::ExecutorSpawner) 外面的壳。有两条性质属于它自己：
//!
//! * `effect` 是 [`ReadOnly`](Effect::ReadOnly)，因为 `effect` 分类的是**工作区**副作用
//!   （spec §7），而提问不碰任何工作区路径 —— 与 `task` 得到的是同一个判断。
//! * 它不可 [`delegable`](Tool::delegable)，所以执行者的工具表根本没有办法提问。这与「让
//!   `task` 不进执行者工具表」是同一个机制（递归深度为一，spec §16）；刻意不再来一条规则。
//!
//! 工具描述里带着那三条编码约定（spec §7）。它们不是装饰：答案是模型必须解码的 JSON，没有它们
//! `selected: []` 就与「从没走到过」无从区分，而一个覆盖性的 `custom` 也与一个补充性的 `custom`
//! 无从区分。

use std::collections::HashSet;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::provider::ToolSpec;
use crate::questions::{Choice, UserQuestion};

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// 工具名，只在这里命名一次，好让注册表、工具表与测试不会互相漂离。
pub const ASK_USER_QUESTION_TOOL: &str = "ask_user_question";

/// 向用户提一个或多个问题。
pub struct AskUserQuestionTool;

/// 模型发过来的线级形状，在验证把它变成 [`UserQuestion`] 之前。
#[derive(Deserialize)]
struct RawArgs {
    questions: Vec<RawQuestion>,
}

#[derive(Deserialize)]
struct RawQuestion {
    #[serde(default)]
    id: String,
    #[serde(default)]
    question: String,
    #[serde(default)]
    header: Option<String>,
    #[serde(default)]
    options: Vec<RawChoice>,
    #[serde(default)]
    multi_select: bool,
}

#[derive(Deserialize)]
struct RawChoice {
    #[serde(default)]
    label: String,
    #[serde(default)]
    description: Option<String>,
}

impl AskUserQuestionTool {
    /// 把模型的参数变成问题，或者用一句模型能据以行动的消息拒掉它们。
    ///
    /// 在这里拒、而不是在端口那里拒，正是让一次畸形调用永远到不了一个人面前的东西：`id` 缺失
    /// 或重复会让答案配不上对，而空的 label 会往屏幕上放一行选不中的东西。
    fn parse(args: &Value) -> Result<Vec<UserQuestion>, ToolError> {
        let raw: RawArgs = serde_json::from_value(args.clone())
            .map_err(|error| ToolError::message(format!("{ASK_USER_QUESTION_TOOL}: {error}")))?;
        if raw.questions.is_empty() {
            return Err(ToolError::message(format!(
                "{ASK_USER_QUESTION_TOOL}: `questions` must hold at least one question"
            )));
        }

        let mut seen = HashSet::new();
        let mut questions = Vec::with_capacity(raw.questions.len());
        for raw in raw.questions {
            let id = raw.id.trim().to_owned();
            if id.is_empty() {
                return Err(ToolError::message(format!(
                    "{ASK_USER_QUESTION_TOOL}: every question needs a non-empty `id`"
                )));
            }
            if !seen.insert(id.clone()) {
                return Err(ToolError::message(format!(
                    "{ASK_USER_QUESTION_TOOL}: duplicate question id `{id}`; ids must be unique"
                )));
            }
            let question = raw.question.trim().to_owned();
            if question.is_empty() {
                return Err(ToolError::message(format!(
                    "{ASK_USER_QUESTION_TOOL}: question `{id}` needs a non-empty `question`"
                )));
            }
            let mut options = Vec::with_capacity(raw.options.len());
            for option in raw.options {
                let label = option.label.trim().to_owned();
                if label.is_empty() {
                    return Err(ToolError::message(format!(
                        "{ASK_USER_QUESTION_TOOL}: question `{id}` has an option with no `label`"
                    )));
                }
                options.push(Choice {
                    label,
                    description: option.description,
                });
            }
            questions.push(UserQuestion {
                id,
                question,
                header: raw.header,
                options,
                multi_select: raw.multi_select,
            });
        }
        Ok(questions)
    }
}

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: ASK_USER_QUESTION_TOOL.to_owned(),
            description: "Ask the user one or more questions when you need a decision, a \
                          confirmation, or information you cannot get from the workspace. Put every \
                          question in one call; the user answers them and the answers are this \
                          call's result — the only result, with no follow-up message.\n\nThe result \
                          is JSON: {\"answers\":[{\"id\":...,\"selected\":[...],\"custom\":...}]}. \
                          Read it by these rules:\n- `selected` holds the labels the user picked and \
                          `custom` holds free text they typed. `selected: []` with no `custom` means \
                          the user skipped that question; an `id` missing from `answers` means the \
                          question was never reached.\n- On a question with `multi_select` false, \
                          custom text overrides the choice, so the answer is `selected: []` with \
                          `custom` set. On a multi-select question custom text supplements the \
                          choice, so both may be present.\n- Give `options` when the choices are \
                          finite, or leave them out to ask for free text. To recommend one option, \
                          put it first and end its `label` with `(Recommended)`; the answer value is \
                          the label exactly as you wrote it, marker included."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "questions": {
                        "type": "array",
                        "minItems": 1,
                        "description": "The questions to put to the user, in order. Each is answered \
                                        explicitly (or skipped) before the next is shown.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {
                                    "type": "string",
                                    "description": "A stable id for this question; it is echoed in \
                                                    the answer that belongs to it."
                                },
                                "question": {
                                    "type": "string",
                                    "description": "The question itself."
                                },
                                "header": {
                                    "type": "string",
                                    "description": "A short label shown above the question. Display \
                                                    only."
                                },
                                "options": {
                                    "type": "array",
                                    "description": "The choices to offer. Omit to ask for free text.",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": {
                                                "type": "string",
                                                "description": "The choice as shown; this exact \
                                                                string is the answer value when it is \
                                                                picked."
                                            },
                                            "description": {
                                                "type": "string",
                                                "description": "One line explaining the choice. \
                                                                Display only."
                                            }
                                        },
                                        "required": ["label"]
                                    }
                                },
                                "multi_select": {
                                    "type": "boolean",
                                    "description": "Whether more than one option may be picked. \
                                                    Defaults to false."
                                }
                            },
                            "required": ["id", "question"]
                        }
                    }
                },
                "required": ["questions"]
            }),
        }
    }

    /// 提问不碰任何工作区路径：答案是上下文，不是一次写（spec §7）。
    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    /// 只有主会话能提问（spec §7）。执行者的工具表就是按这一条过滤出来的，所以执行者没有
    /// `ask_user_question` 可调 —— 与它没有 `task` 是同一个机制。
    fn delegable(&self) -> bool {
        false
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let questions = Self::parse(&args)?;
        // 降级地板（spec §19）：一个没挂提问端口就组装起来的会话，用一条可读的错误作答，而不是
        // 挂在一个永远来不了的答案上。
        let Some(port) = ctx.questions else {
            return Err(ToolError::message(format!(
                "{ASK_USER_QUESTION_TOOL}: this session has no way to ask the user; no question \
                 port is mounted"
            )));
        };
        let answers = port.ask(&questions).await.map_err(ToolError::message)?;
        let text = serde_json::to_string(&answers).map_err(|error| {
            ToolError::message(format!(
                "{ASK_USER_QUESTION_TOOL}: could not encode the answers: {error}"
            ))
        })?;
        Ok(ToolOutput::new(text))
    }
}
