//! The built-in `ask_user_question(questions)` tool: the model's way to ask the
//! user (spec §7).
//!
//! The tool is a thin shell over the [`UserQuestions`] port, exactly as `task` is
//! a shell over [`ExecutorSpawner`](crate::tools::ExecutorSpawner). Two properties
//! are its own:
//!
//! * `effect` is [`ReadOnly`](Effect::ReadOnly) because `effect` classifies
//!   **workspace** side effects (spec §7) and asking touches no workspace path —
//!   the same judgement `task` gets.
//! * it is not [`delegable`](Tool::delegable), so an executor's table has no way
//!   to ask. That is the same mechanism that keeps `task` out of an executor's
//!   table (recursion depth one, spec §16); it is deliberately not a second rule.
//!
//! The tool description carries the three encoding conventions (spec §7). They are
//! not decoration: the answer is JSON the model must decode, and without them
//! `selected: []` is indistinguishable from "never reached" and an overriding
//! `custom` is indistinguishable from a supplementing one.

use std::collections::HashSet;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::provider::ToolSpec;
use crate::questions::{Choice, UserQuestion};

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// The tool name, named once so the registry, the table and the tests cannot
/// drift apart.
pub const ASK_USER_QUESTION_TOOL: &str = "ask_user_question";

/// Ask the user one or more questions.
pub struct AskUserQuestionTool;

/// The wire shape the model sends, before validation turns it into
/// [`UserQuestion`]s.
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
    /// Turn the model's arguments into questions, or refuse them with a message
    /// the model can act on.
    ///
    /// Refusing here rather than at the port is what keeps a malformed call from
    /// ever reaching a person: a missing or duplicated `id` would make the answer
    /// unpairable, and an empty label would put an unselectable row on screen.
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

    /// Asking touches no workspace path: the answer is context, not a write
    /// (spec §7).
    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    /// Only the main session may ask (spec §7). An executor's table is built by
    /// filtering on this, so an executor has no `ask_user_question` to call —
    /// the same mechanism that gives it no `task`.
    fn delegable(&self) -> bool {
        false
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let questions = Self::parse(&args)?;
        // The degradation floor (spec §19): a session assembled without a question
        // port answers with a readable error rather than hanging on an answer that
        // can never come.
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
