//! 只追加的事件流：schema、信封与日志。
//!
//! `events` 坐在内部依赖 DAG 的最底层：它不依赖任何别的内部模块，而其他每个模块都可以
//! 依赖它。事件流是一个会话的唯一真相源；agent 的 `messages` 是它的一次投影（见
//! [`crate::provider::projection`]）。
//!
//! 持久化契约（spec §2）：一个会话一个 JSONL 文件、单写者、每行一次 `flush` 且
//! **不做** `fsync`，末行写坏也被容忍。

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 记进 [`EventPayload::SessionStarted`] 的 schema 版本。
///
/// payload 的形状一变它就涨；跨版本不承诺任何向后兼容。
pub const SCHEMA_VERSION: u32 = 1;

/// 定义一个透明序列化的自有字符串标识。
///
/// 让三个标识 newtype 共用一份实现，于是它们保持一致，也没有哪一个会退化成裸
/// `String`。
macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
    };
}

string_id! {
    /// 一个能行动的参与者的稳定标识：讨论者或执行者。
    ///
    /// 词汇表禁止 `agent` 作类型名；参与者角色住在 [`SpeakerId`] 里。
    ParticipantId
}

string_id! {
    /// 一个会话的标识。跨 `--continue` 永不改变，所以 provider 的前缀缓存能一直命中。
    SessionId
}

string_id! {
    /// 一次工具调用的标识，在它所属的会话里唯一。
    ToolCallId
}

/// 谁在发言。归属永远不从 provider 的 `role` 推断。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SpeakerId {
    Debater(ParticipantId),
    Executor(ParticipantId),
    User,
    System,
}

impl fmt::Display for SpeakerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpeakerId::Debater(id) => write!(f, "{id}"),
            SpeakerId::Executor(id) => write!(f, "executor:{id}"),
            SpeakerId::User => f.write_str("user"),
            SpeakerId::System => f.write_str("system"),
        }
    }
}

/// 一条完成消息携带的 provider 层角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
}

/// 一个循环为什么停。这个枚举由回合循环、讨论轮次循环、会话与执行者共用；嵌套的循环
/// 各自留下自己那个原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    // 单循环的那五个值。
    Completed,
    MaxIterations,
    Aborted,
    MistakeLimit,
    Error,
    // 讨论的那三个值。
    Consensus,
    NoDivergence,
    RoundsExhausted,
    // 预算的那一个值。
    BudgetExhausted,
}

impl StopReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            StopReason::Completed => "Completed",
            StopReason::MaxIterations => "MaxIterations",
            StopReason::Aborted => "Aborted",
            StopReason::MistakeLimit => "MistakeLimit",
            StopReason::Error => "Error",
            StopReason::Consensus => "Consensus",
            StopReason::NoDivergence => "NoDivergence",
            StopReason::RoundsExhausted => "RoundsExhausted",
            StopReason::BudgetExhausted => "BudgetExhausted",
        }
    }
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 一个目标为什么停下来了 —— 停 = **不是**「做完了」。
///
/// 三个值都是协议标记（进流、要稳定），人可读的那句话在 `GoalStopped.detail` 里。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalStopReason {
    /// 连续 N 次翻页零条目完成（§9）：卡住了。
    NoProgress,
    /// provider 调用重试耗尽（§9）。
    ProviderFailed,
    /// 人按 Esc 主动停（§5）。
    UserStopped,
    /// 额度撞顶：**降级收尾**，不是中断（§8、§17）。
    BudgetExhausted,
}

impl GoalStopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            GoalStopReason::NoProgress => "no_progress",
            GoalStopReason::ProviderFailed => "provider_failed",
            GoalStopReason::UserStopped => "user_stopped",
            GoalStopReason::BudgetExhausted => "budget_exhausted",
        }
    }
}

impl fmt::Display for GoalStopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 跨供应商归一化之后的 token 记账。
///
/// `cached_tokens` 与 `miss_tokens` 是一等公民：要判断前缀缓存到底有没有在起作用，
/// 只有靠它们。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub miss_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

impl Usage {
    /// 会话额度要数的那些 token：输入加输出。
    ///
    /// `cached_tokens` 与 `miss_tokens` 是 `input_tokens` 的一次**拆分**，再各加一次
    /// 就是把同一段 prompt 数两遍；而推理 token 供应商本来就算在 `output_tokens` 里
    /// （spec §17）。
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    /// 把另一条记录的计数并进这一条。
    ///
    /// 在没有供应商报出推理 token 之前 `reasoning_tokens` 一直是 `None`，于是总数
    /// 永远不会声称一个没人量过的推理计数。
    pub fn accumulate(&mut self, other: Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cached_tokens += other.cached_tokens;
        self.miss_tokens += other.miss_tokens;
        if let Some(tokens) = other.reasoning_tokens {
            *self.reasoning_tokens.get_or_insert(0) += tokens;
        }
    }
}

/// 一次固定注入来自哪里。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextSource {
    AgentsMd,
    SkillsCatalog,
    /// 组装期这一层 MCP 加载成什么样：哪些 server 连上了、哪些没有、各自的可信配置
    /// （`.scratch/mcp-support/issues/19-mcp-catalog-in-context.md`）。
    ///
    /// 与技能清单同形：进流、可重放，也投影成一条 `user` 消息 —— 模型因此知道这一场会话手里
    /// 有哪些外部 server。**工具清单不在这里**：那是 `mcp_list` 现问的事。
    McpCatalog,
    /// **用户**用 `/<skill>` 载入的一份技能全文（spec §9）。它落在尾部，不像钉在头部
    /// 的那些注入：模型侧的缓存前缀永远不动。
    Skill,
    /// 拼给模型的**私有身份**（系统提示词）。
    ///
    /// 与上面那几个不同，它**不是事件**、也永远不进事件流：身份在每一次请求里现拼、从不
    /// 落盘（`build_messages`，spec §15）。这一枚只被**渲染层**用到 —— 转录里那条记录是
    /// 「按当前代码拼的一份」，一次 `--continue` 重开看到的是今天的拼法，不是当时那份。
    Identity,
    /// 旧**计划模式**进入时注入的那条指令。已经没有任何地方产出它了 —— 那一档模式退场
    /// 了（`docs/adr/0003-plan-leaves-the-permission-modes.md`）—— 这个变体留着，只
    /// 为让**那次改动之前**写下的流还能反序列化：丢掉它会让 `--continue` 在一个老会话
    /// 上失败。
    PlanMode,
    /// 某个讨论者的人物设定 —— 用户为它写的那份 `soul`（spec §15）。
    ///
    /// 与其他每一种注入不同，这一条**只属于它那个参与者**：它以那个讨论者为发言归属
    /// 记下来，投影也只发给它自己，因为它描述的是争论的一方。
    Persona(ParticipantId),
    /// 这个会话在照做的那份**目标清单**（`.scratch/goal-loop/spec.md` §4）。
    ///
    /// 循环在选定目标之后注入一次：清单是封闭的，所以模型得在开跑时就知道上面有哪些条目
    /// —— 它不能往里加东西，只能按 id 标完成。
    Goal,
    /// 翻页带过去的那段**压缩摘要**（§7）。
    ///
    /// 压缩给的是**经验**（为什么这么做、踩了什么坑、读过哪些文件），而派生给的只是**进度**
    /// —— 后者压不出来，前者派生不出来。所以它是新会话开场时最要紧的那一条注入。
    Compaction,
    /// 上下文过半时给模型的那一次提醒（§6）。
    ///
    /// 措辞是「把还没落流的东西落下来」，不是「你该收敛了」：再过一会儿历史会被压成摘要，
    /// 而摘要只留得住它读得到的东西。跨过阈值时注入**一次**，不是每一轮。
    Reminder,
}

/// 一次讨论轮次跑在哪一档。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundMode {
    Independent,
    Targeted,
    Synthesis,
}

/// 一段历史为什么不再权威。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoryReason {
    Regenerate,
    Undo,
    Compaction,
    /// 会话的权限模式变了，于是描述旧模式的那条指令不再作数。已经没有任何地方产出它
    /// 了 —— 它随计划模式指令一同退场，而这样的指令已经不存在
    /// （`docs/adr/0003-plan-leaves-the-permission-modes.md`）—— 它留着的理由和
    /// [`ContextSource::PlanMode`] 一样：老流带着它，而且与其他三个不同，它退掉的是
    /// harness 内容、而不是一次对话交换。
    ModeChange,
}

/// 封闭的三态权限裁决。
///
/// 变体顺序**就是**决策格：`Allow < Ask < Deny`。权限门的规则（spec §12）与前置钩子
/// 的收紧（spec §3）都在这条序上取上确界来合并，于是系统里正好只有一套合并语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Decision {
    Allow,
    Ask,
    Deny,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Ask => "ask",
            Decision::Deny => "deny",
        }
    }

    /// 两个裁决的上确界：更严的那个胜出。
    pub fn join(self, other: Decision) -> Decision {
        self.max(other)
    }

    /// 这个动作默认是否沿委派链往下走。
    ///
    /// `Deny` 与 `Ask` 是约束、会继承；`Allow` 不会 —— 于是「继承拒绝、永不继承
    /// 许可」是这条缺省的必然结果，而不是一个特例（spec §12）。
    pub fn default_propagate(self) -> bool {
        !matches!(self, Decision::Allow)
    }
}

/// 一个权限裁决来自哪里。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionSource {
    User,
    Hook,
    Policy,
}

/// 唯一那条事件总线上的一条记录。
///
/// 消费者按需过滤；这里没有可见性字段，因为可见性是投影规则的产出。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventPayload {
    // 会话骨架。
    SessionStarted {
        session_id: SessionId,
        cwd: String,
        schema_version: u32,
    },
    ContextInjected {
        source: ContextSource,
        content: String,
    },
    /// 这条流开跑时沙箱处在哪一档（沙箱 spec §8）。**log-only**：投影不把它变成任何一条
    /// `messages`，所以钉住的前缀逐字不变，而 replay 能重算出某条命令当时有没有被关着。
    ///
    /// `mode` 是协议标记（`"bwrap"` / `"off"`），不是散文；不可用时那句原因是。
    SandboxStatus {
        mode: String,
        unavailable_reason: Option<String>,
    },
    SessionEnded {
        reason: StopReason,
    },
    /// 这个会话在为哪个目标干活（`.scratch/goal-loop/spec.md` §4）。
    ///
    /// **只追加**，所以**当前目标 = 流上最后一条** —— 切换目标天然就是再记一条，与 `todo`
    /// 同一条派生纪律（[`current_goal`]），`--continue` 之后自然重建。
    ///
    /// **log-only**：投影不把它变成任何一条 `messages`，所以钉住的前缀逐字不变。`goal` 是
    /// 标识符（清单文件的名字），不是散文。
    GoalSelected {
        goal: String,
    },
    /// 一个目标**停下来**了，而它不是做完了（§5、§9）。
    ///
    /// 「停」有四种：目标完成、无进展、provider 失败、人主动停。第一种是
    /// [`GoalCompleted`](Self::GoalCompleted)，其余三种都走这一条 —— 于是恢复
    /// （§10）能一眼分出「正常收尾」与「异常中断」，而报告带得出计数与相关 id。
    GoalStopped {
        goal: String,
        reason: GoalStopReason,
        /// 人可读的说明（散文，打码）。
        detail: String,
        /// 停下时还没完成、或者反复卡住的那几条的 id。报告要可核对，所以它们随事件一起落。
        #[serde(default)]
        stuck: Vec<String>,
        /// 与 `reason` 配套的那个计数：无进展时是连续几次翻页零完成，provider 失败时是试了
        /// 几次，人主动停时是 0。
        #[serde(default)]
        count: u32,
    },
    /// 一个目标做完了，附上那份收尾汇总（§1、§11）。
    ///
    /// 它**不是**工具调用的产物 —— 没有 args 可依 —— 所以要自己落一条事件，否则汇总只在屏幕上
    /// 刷过去就没了，`--continue` 之后谁也读不回来。
    ///
    /// `goal` 是标识符（不打码），`summary` 是散文（打码）。
    GoalCompleted {
        goal: String,
        summary: String,
    },
    // 讨论协议（只占槽位；时序由协议决定）。
    RoundStarted {
        round: u32,
        mode: RoundMode,
    },
    RoundEnded {
        round: u32,
        reason: StopReason,
    },
    DivergenceRecorded {
        round: u32,
        topic: String,
        positions: Vec<String>,
    },
    // 一个 agent 回合。
    TurnStarted {
        agent: SpeakerId,
        iteration: u32,
    },
    MessageCompleted {
        role: Role,
        text: String,
        reasoning: Option<String>,
    },
    ToolCallStarted {
        tool_call_id: ToolCallId,
        tool_name: String,
        args: serde_json::Value,
    },
    ToolCallCompleted {
        tool_call_id: ToolCallId,
        ok: bool,
        output: Option<String>,
        error: Option<String>,
        duration_ms: u64,
    },
    UsageRecorded {
        usage: Usage,
    },
    TurnEnded {
        reason: StopReason,
    },
    // 权限。
    PermissionAsked {
        request_id: String,
        tool_call_id: ToolCallId,
        request: serde_json::Value,
    },
    PermissionDecided {
        request_id: String,
        decision: Decision,
        source: DecisionSource,
        reason: Option<String>,
    },
    // 钩子。
    HookExecuted {
        point: String,
        command: String,
        outcome: String,
    },
    // 执行者。
    ExecutorSpawned {
        executor_id: ParticipantId,
        parent: ParticipantId,
        brief: String,
    },
    ExecutorFinished {
        executor_id: ParticipantId,
        reason: StopReason,
        summary: String,
    },
    // 错误：两类，因为它们的可见性不同。
    AgentError {
        message: String,
        recoverable: bool,
    },
    SessionError {
        code: String,
        detail: String,
    },
    // 历史操作。
    HistorySuperseded {
        targets: Vec<u64>,
        reason: HistoryReason,
        summary: Option<String>,
    },
}

impl EventPayload {
    /// 诊断与断言用的一行稳定短名。
    pub fn kind(&self) -> &'static str {
        match self {
            EventPayload::SessionStarted { .. } => "SessionStarted",
            EventPayload::ContextInjected { .. } => "ContextInjected",
            EventPayload::SandboxStatus { .. } => "SandboxStatus",
            EventPayload::SessionEnded { .. } => "SessionEnded",
            EventPayload::GoalSelected { .. } => "GoalSelected",
            EventPayload::GoalStopped { .. } => "GoalStopped",
            EventPayload::GoalCompleted { .. } => "GoalCompleted",
            EventPayload::RoundStarted { .. } => "RoundStarted",
            EventPayload::RoundEnded { .. } => "RoundEnded",
            EventPayload::DivergenceRecorded { .. } => "DivergenceRecorded",
            EventPayload::TurnStarted { .. } => "TurnStarted",
            EventPayload::MessageCompleted { .. } => "MessageCompleted",
            EventPayload::ToolCallStarted { .. } => "ToolCallStarted",
            EventPayload::ToolCallCompleted { .. } => "ToolCallCompleted",
            EventPayload::UsageRecorded { .. } => "UsageRecorded",
            EventPayload::TurnEnded { .. } => "TurnEnded",
            EventPayload::PermissionAsked { .. } => "PermissionAsked",
            EventPayload::PermissionDecided { .. } => "PermissionDecided",
            EventPayload::HookExecuted { .. } => "HookExecuted",
            EventPayload::ExecutorSpawned { .. } => "ExecutorSpawned",
            EventPayload::ExecutorFinished { .. } => "ExecutorFinished",
            EventPayload::AgentError { .. } => "AgentError",
            EventPayload::SessionError { .. } => "SessionError",
            EventPayload::HistorySuperseded { .. } => "HistorySuperseded",
        }
    }

    /// 就地打码这条 payload 的每一个自由文本字段。
    ///
    /// 穷尽匹配才是重点：schema 自己知道哪些字段是人或模型写的文本，而新加一个 payload
    /// 变体时就必须决定它带不带这样的文本（spec §20）。身份与查询字段（`session_id`、
    /// `cwd`、`tool_name`、`code`、各种 id）一律不动：它们是事件流的键、不是散文，而
    /// 打码过的键会弄坏查询，而不是保护什么。
    ///
    /// args 与权限请求是模型搭出来的 JSON 树，所以逐叶子地走 —— 粘进 `write_file`
    /// 参数里的一个值，与粘进消息正文里的一个值，泄漏的是同一件事。
    pub fn redact(&mut self, redactor: &Redactor) {
        match self {
            EventPayload::SessionStarted { .. }
            | EventPayload::SessionEnded { .. }
            | EventPayload::GoalSelected { .. }
            | EventPayload::RoundStarted { .. }
            | EventPayload::RoundEnded { .. }
            | EventPayload::TurnStarted { .. }
            | EventPayload::UsageRecorded { .. }
            | EventPayload::TurnEnded { .. } => {}
            EventPayload::ContextInjected { content, .. } => redactor.redact(content),
            // 汇总要打码：它是人写的那类叙述文本，而 `goal` 是键，不动。
            EventPayload::GoalCompleted { summary, .. } => redactor.redact(summary),
            // 同上：说明是散文，`goal`、条目 id 与那个计数都是键。
            EventPayload::GoalStopped { detail, .. } => redactor.redact(detail),
            // 模式是协议标记，原因才是散文。
            EventPayload::SandboxStatus {
                unavailable_reason, ..
            } => {
                if let Some(reason) = unavailable_reason {
                    redactor.redact(reason);
                }
            }
            EventPayload::DivergenceRecorded {
                topic, positions, ..
            } => {
                redactor.redact(topic);
                for position in positions {
                    redactor.redact(position);
                }
            }
            EventPayload::MessageCompleted {
                text, reasoning, ..
            } => {
                redactor.redact(text);
                if let Some(reasoning) = reasoning {
                    redactor.redact(reasoning);
                }
            }
            EventPayload::ToolCallStarted { args, .. } => redactor.redact_value(args),
            EventPayload::ToolCallCompleted { output, error, .. } => {
                if let Some(output) = output {
                    redactor.redact(output);
                }
                if let Some(error) = error {
                    redactor.redact(error);
                }
            }
            EventPayload::PermissionAsked { request, .. } => redactor.redact_value(request),
            EventPayload::PermissionDecided { reason, .. } => {
                if let Some(reason) = reason {
                    redactor.redact(reason);
                }
            }
            EventPayload::HookExecuted {
                command, outcome, ..
            } => {
                redactor.redact(command);
                redactor.redact(outcome);
            }
            EventPayload::ExecutorSpawned { brief, .. } => redactor.redact(brief),
            EventPayload::ExecutorFinished { summary, .. } => redactor.redact(summary),
            EventPayload::AgentError { message, .. } => redactor.redact(message),
            EventPayload::SessionError { detail, .. } => redactor.redact(detail),
            EventPayload::HistorySuperseded { summary, .. } => {
                if let Some(summary) = summary {
                    redactor.redact(summary);
                }
            }
        }
    }
}

/// 一个被打码的值替换成的那条标记。
///
/// 用固定文本而不是保留长度的掩码：重点是那个值没了，而一条保留原长的标记会诱使人把
/// 密钥的形状从流里读回来。
pub const REDACTED: &str = "[redacted]";

/// 打码器会动手的最短值。
///
/// 值级打码是件钝器：把一个三字符的字符串替换掉，会把它出现过的每一处普通散文都改写，
/// 会话也就没法读了；而这个项目持有的每一个供应商密钥都远长于此（spec §20 说的是
/// best-effort，不是穷尽）。
const MIN_SECRET_CHARS: usize = 8;

/// 值级、best-effort 的密钥打码（spec §20）。
///
/// 打码器持有那些**绝不能进流的值** —— 实际上就是解析出来的 provider API key —— 并把
/// 每一处出现都换成 [`REDACTED`]。它在一条事件被追加之前施加，这正是「流上的文本等于
/// 模型看到的文本」成立的来由；而产出这段文本的那个工具早就拿着真值跑完了。
///
/// 它住在 `events` 里，因为事件流在依赖 DAG 的最底层，而 `config`（知道密钥的那个）与
/// `agent`（唯一的写者）都得够得着它。
///
/// 它刻意**不做**的事：不猜没登记过的密钥、不解各种编码、也不把碎片拼回成一个值。诚实
/// 的边界是「这个进程配置里带着的那些密钥」（见 `docs/credentials.md`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Redactor {
    /// 长的排前面，于是以另一个值开头的值会被整体替换掉，而不是留下更长那个的尾巴。
    secrets: Vec<String>,
}

impl Redactor {
    /// 在 `secrets` 之上建一个打码器，丢掉短得不像密钥的值。
    pub fn new(secrets: impl IntoIterator<Item = String>) -> Self {
        let mut secrets: Vec<String> = secrets
            .into_iter()
            .map(|secret| secret.trim().to_owned())
            .filter(|secret| secret.chars().count() >= MIN_SECRET_CHARS)
            .collect();
        // 先去重，再按长度降序、同长按稳定次序排，于是无论配置以什么顺序到达，替换
        // 都是确定的。
        secrets.sort();
        secrets.dedup();
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.chars().count()));
        Self { secrets }
    }

    /// 这个打码器有没有要藏的值。
    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }

    /// 把 `text` 里每一处出现都替换掉，就地改。
    ///
    /// 没有东西可替换时，原分配原样留下：取值与写回都是移动而不是拷贝，所以没有密钥的
    /// 情况只付扫描的代价。
    pub fn redact(&self, text: &mut String) {
        if self.is_empty() {
            return;
        }
        let mut redacted = std::mem::take(text);
        for secret in &self.secrets {
            if redacted.contains(secret.as_str()) {
                redacted = redacted.replace(secret.as_str(), REDACTED);
            }
        }
        *text = redacted;
    }

    /// [`Redactor::redact`] 的一个「从一个字符串到另一个字符串」的版本。
    pub fn redacted(&self, text: &str) -> String {
        let mut redacted = text.to_owned();
        self.redact(&mut redacted);
        redacted
    }

    /// 走一遍 JSON 值，把每个字符串叶子打码。
    ///
    /// 对象的**键**一律不动：它们是 schema 的字段名，而一个碰巧等于密钥的键并不是什么
    /// 值在往外泄漏。
    pub fn redact_value(&self, value: &mut serde_json::Value) {
        if self.is_empty() {
            return;
        }
        match value {
            serde_json::Value::String(text) => self.redact(text),
            serde_json::Value::Array(items) => {
                for item in items {
                    self.redact_value(item);
                }
            }
            serde_json::Value::Object(fields) => {
                for field in fields.values_mut() {
                    self.redact_value(field);
                }
            }
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            }
        }
    }
}

/// [`EventPayload::HookExecuted`] 之上的文本约定。
///
/// 钩子 payload 是 `{ point, command, outcome }`：schema 里没有放结构化 outcome 的
/// 位置，所以 outcome 就是一行带稳定前缀的文本。产出与解析共用这些常量，因为约定一旦
/// 漂掉，票 19 的钩子统计会**悄悄**变成零（spec §18）。
///
/// `feedback:` 是投影会并进它所标注那条工具消息的唯一一种 outcome；`failed:` 的
/// outcome 则被丢掉，这也正是「后置钩子失败只丢反馈」在模型那一侧同样成立的原因。
pub mod hook_format {
    /// 前挂载点的 `HookExecuted.point`。
    pub const POINT_PRE: &str = "pre_tool_use";
    /// 后挂载点的 `HookExecuted.point`。
    pub const POINT_POST: &str = "post_tool_use";

    /// 钩子什么都没改：前置钩子不动这次调用，后置钩子不注入反馈。`point` 说明是哪
    /// 一种。
    pub const OUTCOME_CONTINUE: &str = "continue";
    /// 前置钩子换掉了工具参数。
    pub const OUTCOME_REWRITE: &str = "rewrite";
    /// 前置钩子把生效裁决收紧到问。
    pub const OUTCOME_TIGHTEN_ASK: &str = "tighten:ask";
    /// 前置钩子把生效裁决收紧到拒。
    pub const OUTCOME_TIGHTEN_DENY: &str = "tighten:deny";
    /// 前置钩子跳过了执行。
    pub const OUTCOME_SKIP: &str = "skip";
    /// 前置钩子停掉了这个回合。
    pub const OUTCOME_STOP: &str = "stop";

    /// 携带给模型的反馈的那种后置钩子 outcome 的前缀。
    pub const FEEDBACK_PREFIX: &str = "feedback: ";
    /// 任何失败或超时的钩子 outcome 的前缀。
    pub const FAILED_PREFIX: &str = "failed: ";

    /// 投影放在合并进来的反馈前面的那条标记。
    pub const FEEDBACK_MARKER: &str = "[hook feedback]";

    /// 为后置钩子注入的反馈拼出那条 outcome。
    pub fn feedback(text: &str) -> String {
        format!("{FEEDBACK_PREFIX}{text}")
    }

    /// 为失败或超时的钩子拼出那条 outcome。
    pub fn failed(message: &str) -> String {
        format!("{FAILED_PREFIX}{message}")
    }

    /// 这条 outcome 是不是一次失败（或超时）—— 判据只此一处，读的人不必自己写前缀比较。
    pub fn is_failed(outcome: &str) -> bool {
        outcome.starts_with(FAILED_PREFIX)
    }

    /// 一条 outcome 携带的反馈；它不带反馈时是 `None`（一条普通的 `continue`、一次
    /// 失败，或一条前置钩子 outcome）。
    pub fn feedback_text(outcome: &str) -> Option<&str> {
        outcome.strip_prefix(FEEDBACK_PREFIX)
    }
}

/// [`EventPayload::PermissionAsked`] 的 `request` 值的形状。
///
/// 这条事件把问题作为 JSON 携带、而不是一个有类型的结构，所以这些键得有一个归宿：读
/// 事件流的人和写它的循环不能各说各话，而一句点名工具的叙述否则会悄悄把它丢掉。
pub mod permission_format {
    /// 这个问题问的是哪个工具。
    pub const TOOL: &str = "tool";
    /// 参数，好让读者能显示将要跑什么。
    pub const ARGS: &str = "args";
    /// 权限门为什么问。
    pub const REASON: &str = "reason";

    /// `PermissionAsked.request` 携带的工具名 —— 当它带了一个的时候。
    pub fn tool_name(request: &serde_json::Value) -> Option<&str> {
        request.get(TOOL).and_then(serde_json::Value::as_str)
    }

    /// 这个问题所问那次调用的参数 —— 当请求记了它们的时候。
    pub fn args(request: &serde_json::Value) -> Option<&serde_json::Value> {
        request.get(ARGS)
    }

    /// 权限门为什么问 —— 当请求记了它的时候。
    pub fn reason(request: &serde_json::Value) -> Option<&str> {
        request.get(REASON).and_then(serde_json::Value::as_str)
    }

    /// 拼出一次权限询问的 `request` 值。键来自这个模块，于是写者与读者共用一种形状。
    pub fn request(tool_name: &str, args: &serde_json::Value, reason: &str) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        map.insert(
            TOOL.to_owned(),
            serde_json::Value::String(tool_name.to_owned()),
        );
        map.insert(ARGS.to_owned(), args.clone());
        map.insert(
            REASON.to_owned(),
            serde_json::Value::String(reason.to_owned()),
        );
        serde_json::Value::Object(map)
    }
}

/// 信封。`seq` 是一条事件唯一的身份：它就是 JSONL 的行号，所以不存在第二套身份编码。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub at: DateTime<Utc>,
    pub speaker_id: SpeakerId,
    pub payload: EventPayload,
}

impl Event {
    /// 为测试与内存里构造建一个信封。日志在追加时会自己盖上 `seq` 与 `at`。
    pub fn new(seq: u64, speaker_id: SpeakerId, payload: EventPayload) -> Self {
        Self {
            seq,
            at: Utc::now(),
            speaker_id,
            payload,
        }
    }
}

/// 查询：某条 `HistorySuperseded` 事件已经退掉的全部 `seq`。
///
/// 一条被退掉的事件不再生效，但永远不被删（spec §2）。投影会排除这些 seq，`/undo`
/// 也会越过它们，所以这条规则只有一个归宿。
pub fn superseded_seqs(events: &[Event]) -> BTreeSet<u64> {
    let mut retired = BTreeSet::new();
    for event in events {
        if let EventPayload::HistorySuperseded { targets, .. } = &event.payload {
            retired.extend(targets.iter().copied());
        }
    }
    retired
}

/// 查询：这条流上的当前目标 —— 最后一条 `GoalSelected` 的名字（`.scratch/goal-loop/spec.md`
/// §4）。
///
/// 归属是派生的，不是存下来的：`--continue`、`sessions replay` 与一个刚开始的循环问的是同一个
/// 问题，读的也是同一条规则。
pub fn current_goal(events: &[Event]) -> Option<&str> {
    events.iter().rev().find_map(|event| match &event.payload {
        EventPayload::GoalSelected { goal } => Some(goal.as_str()),
        _ => None,
    })
}

/// 查询：一次 `tool_call` 还没有结果？
///
/// 待办的工作是对事件流的一次查询，绝不是藏起来的循环状态。只要这个非空，回合循环就
/// 不能调 provider。这个会话级的形式是给诊断与 `--continue` 恢复用的；循环用的是
/// [`pending_tool_calls_of`]，因为不变量 2 约束的是行动的那个 agent。
pub fn pending_tool_calls(events: &[Event]) -> Vec<ToolCallId> {
    pending_of(events, None)
}

/// 查询：`speaker` 的哪些 `tool_call` 还没有结果？
///
/// 不变量 2 是按行动者算的。两个讨论者同时进行中时，一个会话级的查询会把对方没跑完的
/// 调用读成自己的，然后以一个与它毫不相干的理由拒绝调 provider。
pub fn pending_tool_calls_of(events: &[Event], speaker: &SpeakerId) -> Vec<ToolCallId> {
    pending_of(events, Some(speaker))
}

fn pending_of(events: &[Event], only: Option<&SpeakerId>) -> Vec<ToolCallId> {
    let mut pending: Vec<ToolCallId> = Vec::new();
    for event in events {
        if only.is_some_and(|speaker| &event.speaker_id != speaker) {
            continue;
        }
        match &event.payload {
            EventPayload::ToolCallStarted { tool_call_id, .. } => {
                if !pending.contains(tool_call_id) {
                    pending.push(tool_call_id.clone());
                }
            }
            EventPayload::ToolCallCompleted { tool_call_id, .. } => {
                pending.retain(|id| id != tool_call_id);
            }
            _ => {}
        }
    }
    pending
}

/// 查询：行动的那个发言者最近一条助手消息是否要了工具？
///
/// 这是循环的继续判定。控制流永远不看 `finish_reason` 与 `[DONE]`。
pub fn last_assistant_has_tool_calls(events: &[Event], speaker: &SpeakerId) -> bool {
    let mut has_tool_calls = false;
    for event in events {
        if &event.speaker_id != speaker {
            continue;
        }
        match &event.payload {
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                ..
            } => {
                has_tool_calls = false;
            }
            EventPayload::ToolCallStarted { .. } => {
                has_tool_calls = true;
            }
            _ => {}
        }
    }
    has_tool_calls
}

/// 查询：会话累计用量，把所有 `UsageRecorded` 事件加总（spec §10、§17）。
///
/// 会话开销是对事件流的派生值、绝不是藏起来的状态，所以窗口层不需要锁，而执行者的用量
/// 不用第二本账也算得进来：它的事件就在同一条流上。
///
/// 在没有供应商报出它之前 `reasoning_tokens` 一直是 `None`；一旦有供应商报了，总数
/// 就把它加进去。
pub fn total_usage(events: &[Event]) -> Usage {
    sum_usage(events.iter())
}

/// 查询：一个发言者的用量，用它自己那些 `UsageRecorded` 事件加总。
///
/// 会话总数里属于它的那一片：执行者的开销算进会话（spec §16），同时也是 `task` 结果
/// 作为元数据报回去的东西，所以两个数都不需要第二本账。
pub fn usage_of(events: &[Event], speaker: &SpeakerId) -> Usage {
    sum_usage(events.iter().filter(|event| &event.speaker_id == speaker))
}

fn sum_usage<'a>(events: impl Iterator<Item = &'a Event>) -> Usage {
    let mut total = Usage::default();
    for event in events {
        let EventPayload::UsageRecorded { usage } = &event.payload else {
            continue;
        };
        total.accumulate(*usage);
    }
    total
}

/// 一个会话的只追加 JSONL 日志。
///
/// 这个句柄克隆起来很便宜，而每个克隆共享一个 writer、一份内存缓存和一个 `next_seq`
/// 计数器。正是这份共享让两个讨论者能把各自的回合并发地写进同一条流（spec §15），同时
/// 「日志只有一个写者」依然成立：锁让每一次追加都是原子的，于是两条事件永远不会在一行
/// 里交错，`seq` 也一直就是「行号」。内存里的事件是文件的一份缓存，由此刻持有那把锁的
/// 克隆保持同步。
#[derive(Debug, Clone)]
pub struct EventLog {
    path: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug)]
struct Inner {
    writer: BufWriter<File>,
    events: Vec<Event>,
    next_seq: u64,
}

impl EventLog {
    /// 新建一份日志。文件已存在就失败；父目录必须已经存在。
    ///
    /// 文件建成只有属主可读写（`0600`）：一条流带着用户的源码，打码之后已经不带任何
    /// 秘密 —— 但它仍然是私密的（spec §11）。
    pub fn create(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let mut options = OpenOptions::new();
        options.create_new(true).append(true);
        // 权限在创建时就设好、而不是事后收窄，于是不存在一个流文件对全世界可读的窗口
        // （spec §11、§20）。
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            options.mode(0o600);
        }
        let file = options.open(&path)?;
        Ok(Self {
            path,
            inner: Arc::new(Mutex::new(Inner {
                writer: BufWriter::new(file),
                events: Vec::new(),
                next_seq: 1,
            })),
        })
    }

    /// 打开一份已有的日志准备追加。
    ///
    /// 末尾写坏的那一行（写到一半崩了）被丢掉，于是 `seq` 一直就是「行号」，下一次
    /// 追加也从一整行开始。
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let events = read_events(&path)?;
        repair_before_append(&path)?;
        let next_seq = events.len() as u64 + 1;
        let file = OpenOptions::new().append(true).open(&path)?;
        Ok(Self {
            path,
            inner: Arc::new(Mutex::new(Inner {
                writer: BufWriter::new(file),
                events,
                next_seq,
            })),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn next_seq(&self) -> u64 {
        self.inner().next_seq
    }

    /// 目前为止读到的每一条事件，按顺序的快照。
    ///
    /// 快照而不是借用：两个讨论者共享这个句柄，所以不存在一个调用方还能持有它内部引用
    /// 的生命周期。事件流是只追加的，所以快照就是日志的一个合法前缀 —— 这正是按轮次
    /// 范围的投影需要的东西（spec §15）。
    pub fn events(&self) -> Vec<Event> {
        self.inner().events.clone()
    }

    /// 追加一条事件。刷这一行，但从不 `fsync`。
    pub fn append(&mut self, speaker_id: SpeakerId, payload: EventPayload) -> io::Result<Event> {
        let mut inner = self.inner.lock().expect("事件流互斥锁已中毒");
        let event = Event {
            seq: inner.next_seq,
            at: Utc::now(),
            speaker_id,
            payload,
        };
        let mut line = serde_json::to_string(&event).expect("事件 payload 永远都可 JSON 序列化");
        line.push('\n');
        inner.writer.write_all(line.as_bytes())?;
        inner.writer.flush()?;
        inner.events.push(event.clone());
        inner.next_seq += 1;
        Ok(event)
    }

    /// 把缓冲的字节刷给操作系统。这不是 `fsync`。
    pub fn flush(&mut self) -> io::Result<()> {
        self.inner
            .lock()
            .expect("事件流互斥锁已中毒")
            .writer
            .flush()
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("事件流互斥锁已中毒")
    }
}

/// 从一份 JSONL 日志里读出每一条完整的事件。
///
/// 解析不了的**末**行按写坏处理、跳过；在别处解析不了的行就是损坏，返回错误。
pub fn read_events(path: impl AsRef<Path>) -> io::Result<Vec<Event>> {
    let path = path.as_ref();
    let file = File::open(path)?;
    let mut events = Vec::new();
    let mut lines = BufReader::new(file).lines().enumerate().peekable();
    while let Some((index, line)) = lines.next() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Event>(&line) {
            Ok(event) => events.push(event),
            Err(_) if lines.peek().is_none() => break,
            Err(error) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("事件流 {}：第 {} 行：{error}", path.display(), index + 1),
                ));
            }
        }
    }
    Ok(events)
}

/// 把一份已有日志的尾部收拾成可以安全追加的样子。
///
/// 一条完整事件只是少了结尾的换行时，它会被留下并补上换行；只有解析不了的坏尾巴才会被
/// 丢掉。完整事件永远不被删，所以重开之后 `seq` 一直就是「行号」。
fn repair_before_append(path: &Path) -> io::Result<()> {
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        return Ok(());
    }
    let tail_start = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|position| position + 1)
        .unwrap_or(0);
    let tail = &bytes[tail_start..];

    if serde_json::from_slice::<Event>(tail).is_ok() {
        let mut file = OpenOptions::new().append(true).open(path)?;
        file.write_all(b"\n")?;
        file.flush()?;
    } else {
        let file = OpenOptions::new().write(true).open(path)?;
        file.set_len(tail_start as u64)?;
    }
    Ok(())
}
