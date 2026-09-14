//! Scripted answerers for the permission gate's `Ask` path.
//!
//! The loop asks through the injected `Asker` port; a test scripts the answers
//! exactly as it scripts provider replies, so "the user approved" and "the user
//! denied" are reproducible without a terminal.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_agent::permissions::{Answer, Asker, PermissionRequest};

/// An interactive user who approves every question.
pub struct AlwaysAllow;

#[async_trait]
impl Asker for AlwaysAllow {
    async fn ask(&self, _request: &PermissionRequest) -> Answer {
        Answer::Allow
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
    requests: Mutex<Vec<PermissionRequest>>,
}

impl ScriptedAsker {
    pub fn new(answers: Vec<Answer>) -> Self {
        Self {
            inner: Arc::new(Inner {
                answers: Mutex::new(answers.into()),
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
}
