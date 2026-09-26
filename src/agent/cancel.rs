//! 取消手势的管路（spec §6）。
//!
//! 一次取消是**手势**，不是事件：发起它从不进流，与 `/undo` 的写回、`/plan` 的覆盖
//! 同规矩。这个模块携带的只是「某个回合应当在原地停下」这个信号 —— 流上记录的只有
//! 各方随后拿它做了什么（`TurnEnded { Aborted }`、合成的那些结果、执行者自己的
//! `ExecutorFinished`）。
//!
//! 两端刻意是不同类型。[`CancelSignal`] 由拥有这个手势的一方 —— 前端 —— 持有，也是
//! 唯一能发起取消的那一端。[`CancelObserver`] 是交给一个回合的东西，并逐层克隆到该
//! 回合派发的每一个执行者。于是「取消沿委派链向下传播、从不向上」（spec §6）是类型层
//! 面的性质，而不是一条要记住的规则：回合手里没有任何能发起取消的东西，所以「执行者
//! 被取消」不会把派发者的回合记成失败。
//!
//! 手势的作用域是**一次运行**：回合（或讨论）开始时 harness 会重置信号，所以在什么都
//! 没跑的时候按下的那一下停不掉任何东西，会话也照常能回答下一个问题。

use tokio::sync::watch;

/// 手势自己那一端：唯一能发起取消的句柄。
///
/// 它住在前端；前端同时决定**第二次**按下是什么意思（spec §6：它会把进程按下去）；
/// 这个类型只记录第一次按下的发生。
#[derive(Debug, Clone)]
pub struct CancelSignal {
    cancelled: watch::Sender<bool>,
}

/// 一个回合对手势的视图：只读，而且克隆进每个嵌套回合都很便宜。
#[derive(Debug, Clone)]
pub struct CancelObserver {
    cancelled: watch::Receiver<bool>,
}

impl CancelSignal {
    /// 还没有任何观察端的信号。每个回合用
    /// [`observer`](Self::observer) 自己铸一个。
    pub fn new() -> Self {
        Self {
            cancelled: watch::channel(false).0,
        }
    }

    /// 发起手势。幂等，而且在这轮运行余下的时间里一直有效 —— 哪怕它是在没有观察端
    /// 活着的窗口里发起的。[`reset`](Self::reset) 才是结束它效力的东西，在下一次运行
    /// 开始时调用。
    pub fn cancel(&self) {
        // 用 `send_replace` 而不是 `send`：没人看着时 `send` 是空操作，
        // 那会静默吞掉在这样一个窗口里发起的手势。
        self.cancelled.send_replace(true);
    }

    /// 开始一次新手势，丢掉先前发起的那一次。
    ///
    /// harness 在一次运行开始时调用它，这正是手势的作用域限于一次运行的原因：什么都
    /// 没跑的时候按下的那一下 —— 或者一个已经结束的回合的手势 —— 停不掉后面那一次。
    /// 只有信号端能做这件事；正在跑的回合手里拿的是观察端。
    pub fn reset(&self) {
        self.cancelled.send_replace(false);
    }

    /// 是否已经发起过取消。前端读它来区分第一次按下（取消）与第二次（退出）。
    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }

    /// 给一个回合的视图。按回合铸而不是共享，这样每个回合各自拥有它那头等待所需的
    /// `&mut`。
    pub fn observer(&self) -> CancelObserver {
        CancelObserver {
            cancelled: self.cancelled.subscribe(),
        }
    }
}

impl Default for CancelSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelObserver {
    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }

    /// 手势被发起时 resolve。
    ///
    /// 已经发起过就立即 resolve。信号端没了就再也没人能发起手势，所以这个等待会永久
    /// park —— 这正是让「发送端被 drop」不至于被读成「已取消」的原因。
    pub async fn cancelled(&mut self) {
        if self.is_cancelled() {
            return;
        }
        loop {
            match self.cancelled.changed().await {
                Ok(()) => {
                    if self.is_cancelled() {
                        return;
                    }
                }
                Err(_) => std::future::pending::<()>().await,
            }
        }
    }
}
