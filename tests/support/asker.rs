//! 权限门 `Ask` 那条路上的脚本化作答者。
//!
//! 循环通过注入的 `Asker` 端口提问；测试脚本化作答的方式，与它
//! 脚本化 provider 回复完全一样，于是「用户允许」与「用户
//! 拒绝」不用终端也能复现。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use heng::permissions::{Answer, Asker, PermissionRequest};

/// 每一次询问都放行的交互式用户。
pub struct AlwaysAllow;

#[async_trait]
impl Asker for AlwaysAllow {
    async fn ask(&self, _request: &PermissionRequest) -> Answer {
        Answer::Allow
    }
}

/// 按调用顺序照脚本作答、并记下递给它的每一个问句的作答者。
/// 克隆共享同一份脚本与日志。
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
    /// 带一份权限答案脚本的作答者，按调用顺序逐条发出。
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
            .expect("脚本化作答者已中毒")
            .clone()
    }
}

#[async_trait]
impl Asker for ScriptedAsker {
    async fn ask(&self, request: &PermissionRequest) -> Answer {
        self.inner
            .requests
            .lock()
            .expect("脚本化作答者已中毒")
            .push(request.clone());

        self.inner
            .answers
            .lock()
            .expect("脚本化作答者已中毒")
            .pop_front()
            .expect("ScriptedAsker: 这个问句已经没有脚本答案了")
    }
}
