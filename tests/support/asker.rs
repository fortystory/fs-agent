//! Scripted answerers for the permission gate's `Ask` path.
//!
//! The loop asks through the injected `Asker` port; a test scripts the answers
//! exactly as it scripts provider replies, so "the user approved" and "the user
//! denied" are reproducible without a terminal.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_agent::permissions::{Answer, Asker, PermissionRequest, PlanConflict};

/// An interactive user who approves every question.
pub struct AlwaysAllow;

#[async_trait]
impl Asker for AlwaysAllow {
    async fn ask(&self, _request: &PermissionRequest) -> Answer {
        Answer::Allow
    }

    /// Approving every question includes the one destructive answer plan mode
    /// can ask for; a test that means to keep the file uses [`ScriptedAsker`].
    async fn ask_plan_conflict(&self, _path: &Path) -> PlanConflict {
        PlanConflict::Overwrite
    }
}

/// An answerer that answers by call order from a script and records every
/// question it was handed. Cloning shares the same script and log.
#[derive(Clone, Default)]
pub struct ScriptedAsker {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    answers: Mutex<VecDeque<Answer>>,
    conflicts: Mutex<VecDeque<PlanConflict>>,
    conflict_paths: Mutex<Vec<PathBuf>>,
    requests: Mutex<Vec<PermissionRequest>>,
}

impl ScriptedAsker {
    /// An answerer with permission answers and no plan-conflict script: being
    /// asked about an existing `PLAN.md` is then a test failure, which is what a
    /// scenario that expects no conflict wants to hear about.
    pub fn new(answers: Vec<Answer>) -> Self {
        Self::with_conflicts(answers, Vec::new())
    }

    /// An answerer scripted for both questions, in their own queues.
    pub fn with_conflicts(answers: Vec<Answer>, conflicts: Vec<PlanConflict>) -> Self {
        Self {
            inner: Arc::new(Inner {
                answers: Mutex::new(answers.into()),
                conflicts: Mutex::new(conflicts.into()),
                conflict_paths: Mutex::new(Vec::new()),
                requests: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn requests(&self) -> Vec<PermissionRequest> {
        self.inner
            .requests
            .lock()
            .expect("scripted asker poisoned")
            .clone()
    }

    /// The paths the existing-plan question was asked about, in order.
    pub fn conflict_paths(&self) -> Vec<PathBuf> {
        self.inner
            .conflict_paths
            .lock()
            .expect("scripted asker poisoned")
            .clone()
    }
}

#[async_trait]
impl Asker for ScriptedAsker {
    async fn ask(&self, request: &PermissionRequest) -> Answer {
        self.inner
            .requests
            .lock()
            .expect("scripted asker poisoned")
            .push(request.clone());

        self.inner
            .answers
            .lock()
            .expect("scripted asker poisoned")
            .pop_front()
            .expect("ScriptedAsker: no scripted answer left for this question")
    }

    async fn ask_plan_conflict(&self, path: &Path) -> PlanConflict {
        self.inner
            .conflict_paths
            .lock()
            .expect("scripted asker poisoned")
            .push(path.to_path_buf());

        self.inner
            .conflicts
            .lock()
            .expect("scripted asker poisoned")
            .pop_front()
            .expect("ScriptedAsker: no scripted plan-conflict answer left")
    }
}
