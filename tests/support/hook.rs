//! A scripted hook for the assembly seam.
//!
//! The loop calls the injected `Hook` port exactly as it calls the `Asker`, so a
//! test scripts constraints and feedback in call order and can inspect what the
//! hook was handed. Its `history` records the kinds of the public events the
//! hook saw, which is how the closed-subset rule is checked from the outside.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_agent::hooks::{Constraint, Hook, HookError, PostHookCall, PreHookCall};
use serde_json::Value;

/// One `hook.pre` invocation, as observed from the hook's side.
#[derive(Debug, Clone)]
pub struct PreCall {
    pub tool_name: String,
    pub args: Value,
    /// The kinds of the events the hook could see, in order.
    pub history: Vec<&'static str>,
}

/// One `hook.post` invocation, as observed from the hook's side.
#[derive(Debug, Clone)]
pub struct PostCall {
    pub tool_name: String,
    pub ok: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    /// The kinds of the events the hook could see, in order.
    pub history: Vec<&'static str>,
}

/// A hook that answers by call order from a script and records every call.
///
/// Cloning shares the same script and log, so a test can keep a handle for
/// assertions while the harness owns an injected copy.
#[derive(Clone)]
pub struct ScriptedHook {
    inner: Arc<Inner>,
}

struct Inner {
    pre: Mutex<VecDeque<Result<Constraint, HookError>>>,
    post: Mutex<VecDeque<Result<Option<String>, HookError>>>,
    pre_calls: Mutex<Vec<PreCall>>,
    post_calls: Mutex<Vec<PostCall>>,
}

impl ScriptedHook {
    pub fn new(
        pre: Vec<Result<Constraint, HookError>>,
        post: Vec<Result<Option<String>, HookError>>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                pre: Mutex::new(pre.into()),
                post: Mutex::new(post.into()),
                pre_calls: Mutex::new(Vec::new()),
                post_calls: Mutex::new(Vec::new()),
            }),
        }
    }

    /// A hook that continues every call and injects no feedback.
    pub fn continuing() -> Self {
        Self::new(vec![Ok(Constraint::Continue)], vec![Ok(None)])
    }

    pub fn pre_calls(&self) -> Vec<PreCall> {
        self.inner
            .pre_calls
            .lock()
            .expect("scripted hook poisoned")
            .clone()
    }

    pub fn post_calls(&self) -> Vec<PostCall> {
        self.inner
            .post_calls
            .lock()
            .expect("scripted hook poisoned")
            .clone()
    }
}

#[async_trait]
impl Hook for ScriptedHook {
    fn command(&self) -> &str {
        "scripted-hook"
    }

    async fn pre(&self, call: &PreHookCall<'_>) -> Result<Constraint, HookError> {
        self.inner
            .pre_calls
            .lock()
            .expect("scripted hook poisoned")
            .push(PreCall {
                tool_name: call.tool_name.to_owned(),
                args: call.args.clone(),
                history: call.history.iter().map(|event| event.kind()).collect(),
            });

        self.inner
            .pre
            .lock()
            .expect("scripted hook poisoned")
            .pop_front()
            .expect("ScriptedHook: no scripted pre constraint left for this call")
    }

    async fn post(&self, call: &PostHookCall<'_>) -> Result<Option<String>, HookError> {
        self.inner
            .post_calls
            .lock()
            .expect("scripted hook poisoned")
            .push(PostCall {
                tool_name: call.tool_name.to_owned(),
                ok: call.ok,
                output: call.output.map(str::to_owned),
                error: call.error.map(str::to_owned),
                history: call.history.iter().map(|event| event.kind()).collect(),
            });

        self.inner
            .post
            .lock()
            .expect("scripted hook poisoned")
            .pop_front()
            .expect("ScriptedHook: no scripted post feedback left for this call")
    }
}
