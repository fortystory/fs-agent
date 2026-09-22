//! The user-question port (spec §7, §19): how a model-initiated question reaches
//! whoever owns the keyboard, and how the answer comes back.
//!
//! A **user question** is the third kind of asker this harness has. The
//! [`Asker`](crate::permissions::Asker) port belongs to the permission gate: its
//! answer decides whether an action happens. This port belongs to the model: the
//! answer is **context**, and it returns as the `ask_user_question` call's one
//! result. The renderer's own questions — an oversized paste, a draft `Esc` would
//! clear — travel neither port, because there is nobody to answer them.
//!
//! The types live here, apart from `permissions.rs`, because the two answer
//! vocabularies are different: a permission answer is a verdict, while a question
//! answer is selections plus optional custom text. Making the gate's `Answer`
//! carry a questionnaire variant would force the gate to know a shape it can never
//! produce (spec §19).
//!
//! The error is a plain string rather than a tool-layer type so this module stays
//! a leaf: the tool that owns this port is what turns "no answer came back" into
//! the model-readable `ToolError` its caller sees.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// One answer a question offers.
///
/// `description` is display-only; `label` is both what the user sees and the
/// string that comes back in [`UserAnswer::selected`], so it is never rewritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One question the model wants put to the user.
///
/// `header` is display-only chrome; `multi_select` changes both the control and
/// the encoding of the answer (spec §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserQuestion {
    /// Stable id, echoed in the answer so the model can pair them up.
    pub id: String,
    /// The question itself.
    pub question: String,
    /// A short label shown above the question. Display only.
    pub header: Option<String>,
    /// The choices to offer. Empty means the question is free text.
    pub options: Vec<Choice>,
    /// Whether more than one choice may be picked.
    pub multi_select: bool,
}

/// The user's answer to one question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserAnswer {
    /// The `id` of the question this answers, echoed.
    pub id: String,
    /// The labels the user picked. Empty with no [`custom`](Self::custom) is a skip.
    pub selected: Vec<String>,
    /// Free text the user typed. On a single-select question it **overrides** the
    /// choice, so `selected` is empty; on a multi-select one it **supplements**
    /// the choice, so both may be present (spec §7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<String>,
}

/// One questionnaire's answers: exactly the JSON the `ask_user_question` call
/// returns as its one result.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UserAnswers {
    pub answers: Vec<UserAnswer>,
}

/// The port a front end implements so the `ask_user_question` tool can ask.
///
/// It mirrors [`ExecutorSpawner`](crate::tools::ExecutorSpawner): `tools` knows
/// only the shape, and the layer that owns the keyboard implements it. `ask`
/// answers only when every question has an answer or an explicit skip; a run that
/// is cancelled while a question is up simply drops the request, and the caller
/// sees `Err` rather than a hang. An `id` absent from [`UserAnswers::answers`] was
/// never reached.
#[async_trait]
pub trait UserQuestions: Send + Sync {
    /// Put every question to the user and return their answers, or a readable
    /// reason no answer came back (input ended, or the run was cancelled).
    async fn ask(&self, questions: &[UserQuestion]) -> Result<UserAnswers, String>;
}
