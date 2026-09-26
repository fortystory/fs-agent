//! 一个收尾原因该怎么读。
//!
//! `Completed` 绝不能看起来像 `Aborted` 或 `Error`（spec §19，用户故事 135）：一次撞
//! 了墙的运行与一次跑完的运行，是人最需要一眼分清的两样东西。讨论的四个终止原因
//! （`NoDivergence` / `Consensus` / `RoundsExhausted` / `BudgetExhausted`）同理，要互相
//! 分得开，而不是塌缩成一句「完了」。
//!
//! 这套分类只住在这个模块里；plain 渲染器把它变成 ANSI 码，TUI 把它变成 ratatui `Style`。

use crate::events::StopReason;

/// 一个原因该有多显眼。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// 循环做成了它要做的事。
    Good,
    /// 一个正常收尾，但不是朴素的成功（一次共识、一份用光的轮次预算）。
    Note,
    /// 运行提前停了，而理由不是失败（一次取消、一条回合上限）。
    Warn,
    /// 出了错，或者撞上了一条硬上限。
    Bad,
}

impl Severity {
    /// 把一个原因归类。
    ///
    /// 这个分组是刻意的：`Completed` / `Consensus` / `NoDivergence` 是一次运行*成功*的
    /// 各种方式；`Aborted` / `MaxIterations` / `RoundsExhausted` 提前停了，但不是错误；
    /// `Error` / `MistakeLimit` / `BudgetExhausted` 是人绝不能漏看的那些理由。
    pub fn of(reason: StopReason) -> Severity {
        match reason {
            StopReason::Completed | StopReason::Consensus | StopReason::NoDivergence => {
                Severity::Good
            }
            StopReason::RoundsExhausted => Severity::Note,
            StopReason::Aborted | StopReason::MaxIterations => Severity::Warn,
            StopReason::Error | StopReason::MistakeLimit | StopReason::BudgetExhausted => {
                Severity::Bad
            }
        }
    }

    /// 这一档严重度的 ANSI SGR 前缀。`Good` 用终端默认前景色；其余每一档都上色，好让四
    /// 档永不互相塌缩。画不画由调用方决定。
    pub fn ansi(self) -> &'static str {
        match self {
            Severity::Good => "\x1b[32m",
            Severity::Note => "\x1b[36m",
            Severity::Warn => "\x1b[33m",
            Severity::Bad => "\x1b[31m",
        }
    }

    /// 给 [`Severity::ansi`] 收尾的那条 ANSI 复位。
    pub const ANSI_RESET: &'static str = "\x1b[0m";
}
