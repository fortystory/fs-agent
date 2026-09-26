//! 用户提问端口（spec §7、§19）：模型发起的问句怎么送到持有键盘的人手里，答案又怎么
//! 回来。
//!
//! **用户提问**是这个 harness 的第三类发起者。[`Asker`](crate::permissions::Asker)
//! 端口归权限门：它的答案决定一次动作做不做。这个
//! 端口归模型：答案是**上下文**，并作为 `ask_user_question` 调用那唯一的结果返回。渲染器
//! 自己的问句 —— 一次过大的粘贴、一个按 `Esc` 就能清掉的草稿 —— 两条端口都不走，因为
//! 没有别人可以回答它们。
//!
//! 这些类型住在这里而不是 `permissions.rs`，因为两边的答案词汇不同：权限的答案是裁决，
//! 而提问的答案是选项再加上可选的自定义文本。让权限门的 `Answer` 带上一个问卷变体，就
//! 等于逼权限门去认识一种它永远产不出来的形状（spec §19）。
//!
//! 错误用一条普通字符串而不是工具层的类型，为的是让这个模块保持叶子：把「没有答案回来」
//! 翻成调用方看到的、模型可读的 `ToolError`，是持有这个端口的那个工具的事。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// 一道题提供的一个答案。
///
/// `description` 只用于显示；`label` 既是用户看到的东西，也是原样回在
/// [`UserAnswer::selected`] 里的那个字符串，所以它永远不会被改写。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// 模型想摆到用户面前的一道题。
///
/// `header` 只是显示用的门面；`multi_select` 同时改变控件与答案的编码（spec §7）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserQuestion {
    /// 稳定的 id，会原样回在答案里，好让模型把问题和答案对起来。
    pub id: String,
    /// 问题本身。
    pub question: String,
    /// 问题上方显示的一行短标签。只用于显示。
    pub header: Option<String>,
    /// 给出的选项。为空表示这道题是自由文本。
    pub options: Vec<Choice>,
    /// 是否可以选中多于一个选项。
    pub multi_select: bool,
}

/// 用户对一道题的作答。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserAnswer {
    /// 它所回答的那道题的 `id`，原样回声。
    pub id: String,
    /// 用户选中的那些 label。为空且没有 [`custom`](Self::custom) 就是跳过。
    pub selected: Vec<String>,
    /// 用户打的自由文本。单选题上它**覆盖**选项，于是 `selected` 为空；多选题上它
    /// **补充**选项，于是两者可以并存（spec §7）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<String>,
}

/// 一份问卷的答案：正是 `ask_user_question` 调用作为唯一结果返回的那段 JSON。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UserAnswers {
    pub answers: Vec<UserAnswer>,
}

/// 前端实现的那条端口，好让 `ask_user_question` 工具问得出去。
///
/// 它与 [`ExecutorSpawner`](crate::tools::ExecutorSpawner) 是一对镜像：`tools` 只知道
/// 形状，实现它的是持有键盘的那一层。`ask` 只在每道题都有答案或显式跳过时才作答；一次
/// 在问题摆着时被取消的运行会直接丢掉请求，调用方看到的是 `Err` 而不是挂住。
/// [`UserAnswers::answers`] 里缺席的 `id` 就是压根没问到的那道题。
#[async_trait]
pub trait UserQuestions: Send + Sync {
    /// 把每道题都摆给用户并返回他们的答案；没有答案回来时返回一条可读的理由
    /// （输入结束，或这次运行被取消）。
    async fn ask(&self, questions: &[UserQuestion]) -> Result<UserAnswers, String>;
}
