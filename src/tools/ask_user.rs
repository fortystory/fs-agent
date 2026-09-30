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
            .map_err(|error| ToolError::message(format!("{ASK_USER_QUESTION_TOOL}：{error}")))?;
        if raw.questions.is_empty() {
            return Err(ToolError::message(format!(
                "{ASK_USER_QUESTION_TOOL}：`questions` 至少要有一道题"
            )));
        }

        let mut seen = HashSet::new();
        let mut questions = Vec::with_capacity(raw.questions.len());
        for raw in raw.questions {
            let id = raw.id.trim().to_owned();
            if id.is_empty() {
                return Err(ToolError::message(format!(
                    "{ASK_USER_QUESTION_TOOL}：每道题都需要一个非空的 `id`"
                )));
            }
            if !seen.insert(id.clone()) {
                return Err(ToolError::message(format!(
                    "{ASK_USER_QUESTION_TOOL}：题目的 id `{id}` 重复了；id 必须唯一"
                )));
            }
            let question = raw.question.trim().to_owned();
            if question.is_empty() {
                return Err(ToolError::message(format!(
                    "{ASK_USER_QUESTION_TOOL}：题目 `{id}` 需要一个非空的 `question`"
                )));
            }
            let mut options = Vec::with_capacity(raw.options.len());
            for option in raw.options {
                let label = option.label.trim().to_owned();
                if label.is_empty() {
                    return Err(ToolError::message(format!(
                        "{ASK_USER_QUESTION_TOOL}：题目 `{id}` 有一个选项没有 `label`"
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
            description: "需要做决定、需要确认，或者需要工作区里拿不到的信息时，向用户提一个或多个\
                          问题。把所有问题都放进同一次调用；用户作答，答案就是这次调用的结果——唯\
                          一的结果，没有后续消息。\n\n结果是 JSON：\
                          {\"answers\":[{\"id\":...,\"selected\":[...],\"custom\":...}]}。按这几条\
                          规矩读它：\n- `selected` 装用户勾选的 label，`custom` 装用户自己敲的自由\
                          文本。`selected: []` 且没有 `custom`，意思是用户跳过了那道题；\
                          `answers` 里少一个 `id`，意思是那道题根本没走到。\n- 在 `multi_select` \
                          为 false 的题上，自定义文本覆盖所选，所以答案是 `selected: []` 加上 \
                          `custom`。在多选题上，自定义文本是所选之外的补充，两者可以同时出现。\
                          \n- 选项有限时给出 `options`，想要自由文本就不给。要推荐某个选项，把它放\
                          在第一个，并让它的 `label` 以 `(Recommended)` 结尾；答案里的值就是那个 \
                          label 原样，标记也一起。"
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "questions": {
                        "type": "array",
                        "minItems": 1,
                        "description": "要问用户的问题，按顺序。每道题被明确作答（或被跳过）之后\
                                        才显示下一道",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {
                                    "type": "string",
                                    "description": "这道题的稳定 id；属于它的那条答案里会原样\
                                                    带回它"
                                },
                                "question": {
                                    "type": "string",
                                    "description": "题目本身"
                                },
                                "header": {
                                    "type": "string",
                                    "description": "显示在题目上方的一行短标签，只做显示"
                                },
                                "options": {
                                    "type": "array",
                                    "description": "要给出的选项。想要自由文本就不给这一项",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": {
                                                "type": "string",
                                                "description": "选项显示的文本；它被勾中时，答案里\
                                                                的值就是这一串原样"
                                            },
                                            "description": {
                                                "type": "string",
                                                "description": "一行说明这个选项，只做显示"
                                            }
                                        },
                                        "required": ["label"]
                                    }
                                },
                                "multi_select": {
                                    "type": "boolean",
                                    "description": "是否可以勾选多个选项，默认 false"
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
                "{ASK_USER_QUESTION_TOOL}：这个会话没有向用户提问的途径；没有挂上提问端口"
            )));
        };
        let answers = port.ask(&questions).await.map_err(ToolError::message)?;
        let text = serde_json::to_string(&answers).map_err(|error| {
            ToolError::message(format!(
                "{ASK_USER_QUESTION_TOOL}：编码不了这些答案：{error}"
            ))
        })?;
        Ok(ToolOutput::new(text))
    }
}
