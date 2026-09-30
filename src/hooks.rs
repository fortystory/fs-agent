//! 钩子边界（spec §3，用户故事 I）。
//!
//! 一个钩子是一份挂载在工具调用前后两点之一的用户**策略**。控制流归循环，两种变换由
//! 循环按固定顺序施加：
//!
//! ```text
//! hook.pre -> permission gate -> [ask] -> dispatch -> hook.post -> append
//! ```
//!
//! 这个模块存在的理由是**一条代数事实**：**前置钩子只能收紧**。它的产出是一个
//! [`Constraint`] —— `Continue | Rewrite(args) | Tighten(Ask|Deny) | Skip | Stop` ——
//! 而其中唯一裁决形状的变体带的是 [`Tightening`]，它没有 `Allow` 那一支。于是「钩子
//! 松不开权限」是**类型的性质**而不是运行时检查，生效裁决就是约束与权限门裁决在
//! `Allow < Ask < Deny` 上的上确界。这里没有 `PermissionRequest` 挂载点：`hook.pre`
//! 跑在权限门之前，所以它能把一次询问掐死在发生之前，却永远绕不过一次询问。
//!
//! 失败是不对称的（spec §3）。`hook.pre` 失败或超时一律 **fail-closed**：动作被拦住、
//! 这次失败被诊断出来，循环为这次调用合成那唯一一条错误结果。`hook.post` 失败只是
//! **丢掉反馈** —— 世界已经变了，那一侧 fail-closed 换不来任何安全。
//!
//! 钩子只观察事件流那个封闭的公开子集 [`HookEvent`]：工具、权限与会话边界事件。消息、
//! 推理、用量、执行者事件与会话错误都不在钩子的视野里。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::events::{
    hook_format, Decision, DecisionSource, Event, EventPayload, SessionId, StopReason, ToolCallId,
};
use crate::tools::Effect;

/// `hook.pre` 的产出：对这次调用的一条约束，永远不是一个裁决。
#[derive(Debug, Clone, PartialEq)]
pub enum Constraint {
    /// 不动这次调用；权限门的裁决说了算。
    Continue,
    /// 在权限门判定这次调用**之前**换掉参数，于是权限门与工具看到的都是改写后的
    /// 调用。
    Rewrite(Value),
    /// 抬高生效裁决。`Tightening` 表达不出 `Allow`。
    Tighten(Tightening),
    /// 不跑这个工具。循环为这次调用合成那唯一一条错误结果。
    Skip,
    /// 立刻结束这个回合。循环为当前这次调用合成那唯一一条错误结果，并以
    /// `StopReason::Aborted` 收尾。
    Stop,
}

impl Constraint {
    /// 这条约束逼出来的裁决 —— 如果它逼得出的话。
    ///
    /// `Continue`、`Rewrite`、`Skip` 与 `Stop` 讲的是流程、不是裁决；只有 `Tighten`
    /// 参与和权限门裁决取上确界。
    pub fn tightening(&self) -> Option<Decision> {
        match self {
            Constraint::Tighten(tightening) => Some(tightening.decision()),
            _ => None,
        }
    }

    /// 这条约束记进 `HookExecuted.outcome` 的文本。
    pub fn outcome(&self) -> String {
        match self {
            Constraint::Continue => hook_format::OUTCOME_CONTINUE.to_owned(),
            Constraint::Rewrite(_) => hook_format::OUTCOME_REWRITE.to_owned(),
            Constraint::Tighten(Tightening::Ask) => hook_format::OUTCOME_TIGHTEN_ASK.to_owned(),
            Constraint::Tighten(Tightening::Deny) => hook_format::OUTCOME_TIGHTEN_DENY.to_owned(),
            Constraint::Skip => hook_format::OUTCOME_SKIP.to_owned(),
            Constraint::Stop => hook_format::OUTCOME_STOP.to_owned(),
        }
    }
}

/// 钩子能收紧到的两个裁决。`Allow` 是**按构造**缺席的，正因为如此「只能收紧」才是条
/// 类型性质。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tightening {
    Ask,
    Deny,
}

impl Tightening {
    /// 这条收紧贡献给上确界的裁决。
    pub fn decision(self) -> Decision {
        match self {
            Tightening::Ask => Decision::Ask,
            Tightening::Deny => Decision::Deny,
        }
    }
}

/// 唯一的那一次合并：生效裁决是前置钩子的收紧与权限门裁决的上确界。`None` 表示钩子
/// 没有收紧。
///
/// 两者只在这一处合流，所以循环与纯测试漂不开 —— 而且因为 `Tightening` 里没有
/// `Allow`，这次合并只可能把裁决抬高。
pub fn effective_verdict(gate: Decision, tightening: Option<Decision>) -> Decision {
    gate.join(tightening.unwrap_or(Decision::Allow))
}

/// 钩子挂在哪。两个挂载点只在这一处拼写。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPoint {
    PreToolUse,
    PostToolUse,
}

impl HookPoint {
    /// 存进 `HookExecuted.point` 的那个拼写。
    pub fn as_str(self) -> &'static str {
        match self {
            HookPoint::PreToolUse => hook_format::POINT_PRE,
            HookPoint::PostToolUse => hook_format::POINT_POST,
        }
    }
}

/// 钩子为什么产不出结果。两种情况在前挂载点都是 fail-closed，在后挂载点都是丢掉
/// 反馈。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HookError {
    #[error("{0}")]
    Failed(String),
    #[error("钩子超时")]
    Timeout,
}

/// 一次调用在前置钩子眼里的样子：已解析好的事实，加上事件流目前为止那个封闭的公开
/// 子集。
///
/// 这些事实就是权限门将要判的东西，所以钩子能就权限门看到的同一个副作用与同一批已
/// 解析路径去推理。`history` 从会话的事件流借来，永远不会包含 [`HookEvent`] 之外的
/// 事件。
pub struct PreHookCall<'a> {
    pub tool_call_id: &'a str,
    pub tool_name: &'a str,
    /// 此刻的参数原文；`Rewrite` 会换掉它们。
    pub args: &'a Value,
    pub effect: &'a Effect,
    pub write_targets: &'a [PathBuf],
    pub read_targets: &'a [PathBuf],
    pub argv: Option<&'a [String]>,
    pub cwd: &'a Path,
    /// 事件流的公开子集，按 `seq` 顺序。
    pub history: &'a [HookEvent],
}

/// 一次已解析的调用在后置钩子眼里的样子。
pub struct PostHookCall<'a> {
    pub tool_call_id: &'a str,
    pub tool_name: &'a str,
    pub args: &'a Value,
    /// 这次工具调用有没有产出一条成功的结果。
    pub ok: bool,
    pub output: Option<&'a str>,
    pub error: Option<&'a str>,
    /// 事件流的公开子集，按 `seq` 顺序。
    pub history: &'a [HookEvent],
}

/// 循环在两个挂载点都调的那条端口。
///
/// 钩子自报 `command`（记进 `HookExecuted` 的身份），两侧可以任选一侧实现：默认实现
/// 不动调用、也不注入反馈。一个会话正好有一个钩子值，像询问端那样与嵌套会话共享，
/// 所以执行者不会丢掉这份策略。
#[async_trait]
pub trait Hook: Send + Sync {
    /// 记进 `HookExecuted.command` 的身份。
    fn command(&self) -> &str;

    /// 跑在权限门之前。默认实现不动这次调用。
    async fn pre(&self, _call: &PreHookCall<'_>) -> Result<Constraint, HookError> {
        Ok(Constraint::Continue)
    }

    /// 在工具解析完之后跑。`Some` 里的文本是反馈，投影会把它并进那个工具的消息；
    /// 默认实现什么都不注入。
    async fn post(&self, _call: &PostHookCall<'_>) -> Result<Option<String>, HookError> {
        Ok(None)
    }
}

/// 钩子可以观察的事件流那个封闭的公开子集（spec §2）：工具、权限与会话边界事件，
/// 一共七个变体。
///
/// 用一个独立类型、而不是在 [`EventPayload`] 上加一道过滤，正是让观察面封闭的原因：
/// 这里没有给消息、推理、用量、执行者事件、历史操作或会话错误留任何一支，所以将来新增
/// 的事件默认漏不进钩子。
#[derive(Debug, Clone, PartialEq)]
pub enum HookEvent {
    SessionStarted {
        session_id: SessionId,
        cwd: String,
        schema_version: u32,
    },
    SessionEnded {
        reason: StopReason,
    },
    ToolCallStarted {
        tool_call_id: ToolCallId,
        tool_name: String,
        args: Value,
    },
    ToolCallCompleted {
        tool_call_id: ToolCallId,
        ok: bool,
        output: Option<String>,
        error: Option<String>,
        duration_ms: u64,
    },
    PermissionAsked {
        request_id: String,
        tool_call_id: ToolCallId,
        request: Value,
    },
    PermissionDecided {
        request_id: String,
        decision: Decision,
        source: DecisionSource,
        reason: Option<String>,
    },
    AgentError {
        message: String,
        recoverable: bool,
    },
}

impl HookEvent {
    /// 把一条 payload 投影进公开子集；钩子不该看到的返回 `None`。
    pub fn from_payload(payload: &EventPayload) -> Option<Self> {
        match payload {
            EventPayload::SessionStarted {
                session_id,
                cwd,
                schema_version,
            } => Some(HookEvent::SessionStarted {
                session_id: session_id.clone(),
                cwd: cwd.clone(),
                schema_version: *schema_version,
            }),
            EventPayload::SessionEnded { reason } => {
                Some(HookEvent::SessionEnded { reason: *reason })
            }
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } => Some(HookEvent::ToolCallStarted {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                args: args.clone(),
            }),
            EventPayload::ToolCallCompleted {
                tool_call_id,
                ok,
                output,
                error,
                duration_ms,
            } => Some(HookEvent::ToolCallCompleted {
                tool_call_id: tool_call_id.clone(),
                ok: *ok,
                output: output.clone(),
                error: error.clone(),
                duration_ms: *duration_ms,
            }),
            EventPayload::PermissionAsked {
                request_id,
                tool_call_id,
                request,
            } => Some(HookEvent::PermissionAsked {
                request_id: request_id.clone(),
                tool_call_id: tool_call_id.clone(),
                request: request.clone(),
            }),
            EventPayload::PermissionDecided {
                request_id,
                decision,
                source,
                reason,
            } => Some(HookEvent::PermissionDecided {
                request_id: request_id.clone(),
                decision: *decision,
                source: *source,
                reason: reason.clone(),
            }),
            EventPayload::AgentError {
                message,
                recoverable,
            } => Some(HookEvent::AgentError {
                message: message.clone(),
                recoverable: *recoverable,
            }),
            _ => None,
        }
    }

    /// 诊断与断言用的一行稳定短名。
    pub fn kind(&self) -> &'static str {
        match self {
            HookEvent::SessionStarted { .. } => "SessionStarted",
            HookEvent::SessionEnded { .. } => "SessionEnded",
            HookEvent::ToolCallStarted { .. } => "ToolCallStarted",
            HookEvent::ToolCallCompleted { .. } => "ToolCallCompleted",
            HookEvent::PermissionAsked { .. } => "PermissionAsked",
            HookEvent::PermissionDecided { .. } => "PermissionDecided",
            HookEvent::AgentError { .. } => "AgentError",
        }
    }
}

/// 把整条事件流投影成钩子可以观察的公开子集，按 `seq` 顺序。
pub fn public_history(events: &[Event]) -> Vec<HookEvent> {
    events
        .iter()
        .filter_map(|event| HookEvent::from_payload(&event.payload))
        .collect()
}
