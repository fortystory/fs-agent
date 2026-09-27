//! 组装接缝用的脚本化钩子。
//!
//! 循环调用注入的 `Hook` 端口，与它调用 `Asker` 一模一样，所以
//! 测试可以按调用顺序脚本化约束与反馈，并检查钩子收到了什么。它的
//! `history` 记下钩子看见的那些公开事件的种类，封闭子集
//! 这条规矩就是从外面这样检查的。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_agent::hooks::{Constraint, Hook, HookError, PostHookCall, PreHookCall};
use serde_json::Value;

/// 从钩子那一侧看过去的一次 `hook.pre` 调用。
#[derive(Debug, Clone)]
pub struct PreCall {
    pub tool_name: String,
    pub args: Value,
    /// 钩子能看见的那些事件的种类，按顺序。
    pub history: Vec<&'static str>,
}

/// 从钩子那一侧看过去的一次 `hook.post` 调用。
#[derive(Debug, Clone)]
pub struct PostCall {
    pub tool_name: String,
    pub ok: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    /// 钩子能看见的那些事件的种类，按顺序。
    pub history: Vec<&'static str>,
}

/// 按调用顺序照脚本作答、并记下每一次调用的钩子。
///
/// 克隆共享同一份脚本与日志，于是测试可以自己留一个把手做断言，
/// 而 harness 手里握着注入的那一份。
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

    /// 每一次调用都放行、且不注入任何反馈的钩子。
    pub fn continuing() -> Self {
        Self::new(vec![Ok(Constraint::Continue)], vec![Ok(None)])
    }

    pub fn pre_calls(&self) -> Vec<PreCall> {
        self.inner
            .pre_calls
            .lock()
            .expect("脚本化钩子已中毒")
            .clone()
    }

    pub fn post_calls(&self) -> Vec<PostCall> {
        self.inner
            .post_calls
            .lock()
            .expect("脚本化钩子已中毒")
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
            .expect("脚本化钩子已中毒")
            .push(PreCall {
                tool_name: call.tool_name.to_owned(),
                args: call.args.clone(),
                history: call.history.iter().map(|event| event.kind()).collect(),
            });

        self.inner
            .pre
            .lock()
            .expect("脚本化钩子已中毒")
            .pop_front()
            .expect("ScriptedHook: 这次调用已经没有脚本化的前置约束了")
    }

    async fn post(&self, call: &PostHookCall<'_>) -> Result<Option<String>, HookError> {
        self.inner
            .post_calls
            .lock()
            .expect("脚本化钩子已中毒")
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
            .expect("脚本化钩子已中毒")
            .pop_front()
            .expect("ScriptedHook: 这次调用已经没有脚本化的后置反馈了")
    }
}
