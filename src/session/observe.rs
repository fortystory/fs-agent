//! `sessions` CLI 背后那些给人看的视图（spec §18）。
//!
//! 这里每个视图都是**对事件流的一次 group-by**，绝不是新的采集点：流是唯一真相源，而第二份诊断
//! 存储就是第二件要维持诚实的东西。`seq` 是唯一的身份，所以一个视图就是一次顺序读加一次内存里的
//! 过滤 —— 这里刻意没有索引。
//!
//! 四个视图：
//!
//! * [`list`] / [`summarize`] —— 有哪些会话，供 `sessions ls` 用；
//! * [`timeline`] —— 按轮分组的转录，供 `sessions show` 用，[`Filter`] 是它的逃生口，工具调用与
//!   各自的结果合并在一起；
//! * [`file_history`] —— 唯一一个按**工作区对象**而不是按时间索引的视图（「谁在那一轮改了这个文
//!   件」），从一次成功写入所报的 `wrote:` 行推出来；
//! * [`stats`] —— 固定的指标集，其中包括别处都不露面的两个量：单边缺席率与编辑阶梯的降级分布。
//!
//! 派生指标所读的那些约定文本（`edit match level:`、`wrote:`、读后写的拒绝、`no match` 失败）来
//! 自生产者用的常量，绝不是某处写第二遍的字面量：一个漂移的前缀会把一个计数悄悄变成零
//! （spec §18）。

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::{LandingPoint, PriceTable, Routing, SessionConfig};
use crate::discussion::protocol::round_attendance;
use crate::events::{
    hook_format, ContextSource, Decision, DecisionSource, Event, EventPayload, HistoryReason, Role,
    RoundMode, SessionId, SpeakerId, StopReason, ToolCallId, Usage,
};
use crate::tools::edit::EditError;
use crate::tools::file::{MATCH_LEVEL_PREFIX, WROTE_PATH_PREFIX};
use crate::tools::READ_BEFORE_WRITE_PREFIX;

use super::store::{SessionStore, StoredSession};

// ---------------------------------------------------------------------------
// 列表
// ---------------------------------------------------------------------------

/// 一场会话的一行：足以找到你想找的那一场。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    pub id: SessionId,
    /// 可搬运的会话目录。
    pub dir: PathBuf,
    /// 权威的工作区，来自 `SessionStarted`。
    pub cwd: Option<String>,
    /// 会话开始的时间，来自它自己的第一条事件。
    pub started: Option<DateTime<Utc>>,
    /// 流最后一次被写入的时间，来自文件的 mtime。
    pub written: Option<DateTime<Utc>>,
    /// 记下的会话收尾，如果有一条。
    pub ended: Option<StopReason>,
    /// 会话累计 token，从 `UsageRecorded` 加总。
    pub tokens: u64,
    /// 流上已完成的消息数。
    pub messages: usize,
    /// 开过的轮次。
    pub rounds: usize,
}

/// 列出会话：某一个 cwd 的桶；`cwd` 为 `None` 时是每个桶。
///
/// 最新的在前，也就是 `--continue` 会考虑它们的顺序。
pub fn list(store: &SessionStore, cwd: Option<&Path>) -> io::Result<Vec<Listing>> {
    let sessions = match cwd {
        Some(cwd) => store.list(cwd)?,
        None => store.list_all()?,
    };
    sessions.iter().map(summarize).collect()
}

/// 从一场已存会话自己的流汇总出它的一行。
pub fn summarize(session: &StoredSession) -> io::Result<Listing> {
    let events = crate::events::read_events(&session.log_path)?;
    let mut listing = Listing {
        id: session.id.clone(),
        dir: session.dir.clone(),
        cwd: None,
        started: None,
        written: written_at(&session.log_path),
        ended: None,
        tokens: 0,
        messages: 0,
        rounds: 0,
    };
    for event in &events {
        if listing.started.is_none() {
            listing.started = Some(event.at);
        }
        match &event.payload {
            EventPayload::SessionStarted { cwd, .. } => listing.cwd = Some(cwd.clone()),
            EventPayload::SessionEnded { reason } => listing.ended = Some(*reason),
            EventPayload::MessageCompleted { .. } => listing.messages += 1,
            EventPayload::RoundStarted { .. } => listing.rounds += 1,
            _ => {}
        }
    }
    listing.tokens = crate::events::total_usage(&events).total_tokens();
    Ok(listing)
}

fn written_at(path: &Path) -> Option<DateTime<Utc>> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(DateTime::<Utc>::from)
}

// ---------------------------------------------------------------------------
// 时间线
// ---------------------------------------------------------------------------

/// 按轮分组的转录（`sessions show`）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub session_id: Option<SessionId>,
    pub cwd: Option<String>,
    pub groups: Vec<TimelineGroup>,
}

/// 转录里一轮的那一片，外加第一轮之前那一段。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TimelineGroup {
    /// 轮次编号；会话的前奏是 `None`（单 agent 会话也用它，因为它根本没有轮次）。
    pub round: Option<u32>,
    pub mode: Option<RoundMode>,
    /// 这一轮是怎么结束的 —— 当它是结束辩论阶段的那一轮时。
    pub ended: Option<StopReason>,
    pub entries: Vec<Entry>,
}

/// 转录里的一行。工具调用与它的结果合并，钩子的后续反馈与它所评注的结果合并，所以一次调用读起来
/// 是一件事，而不是三件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "snake_case")]
pub enum Entry {
    Message {
        speaker: SpeakerId,
        role: Role,
        text: String,
    },
    Tool {
        speaker: SpeakerId,
        tool_call_id: ToolCallId,
        tool: String,
        args: serde_json::Value,
        /// 这次调用还没有结果时是 `None`。
        ok: Option<bool>,
        output: Option<String>,
        error: Option<String>,
        duration_ms: Option<u64>,
        /// 钩子对这次调用的结局，合并在这里，因为钩子事件不携带 `tool_call_id`，而这次调用正是它
        /// 所评注的那个。
        hook: Option<String>,
    },
    RoundStarted {
        round: u32,
        mode: RoundMode,
    },
    RoundEnded {
        round: u32,
        reason: StopReason,
    },
    SessionEnded {
        reason: StopReason,
    },
    TurnStarted {
        speaker: SpeakerId,
        iteration: u32,
    },
    Divergence {
        round: u32,
        topic: String,
        positions: Vec<String>,
    },
    TurnEnded {
        speaker: SpeakerId,
        reason: StopReason,
    },
    PermissionAsked {
        speaker: SpeakerId,
        request_id: String,
        tool_call_id: ToolCallId,
        request: serde_json::Value,
    },
    PermissionDecided {
        speaker: SpeakerId,
        request_id: String,
        decision: Decision,
        source: DecisionSource,
        reason: Option<String>,
    },
    Hook {
        speaker: SpeakerId,
        point: String,
        command: String,
        outcome: String,
    },
    ExecutorSpawned {
        speaker: SpeakerId,
        executor_id: crate::events::ParticipantId,
        parent: crate::events::ParticipantId,
        brief: String,
    },
    ExecutorFinished {
        speaker: SpeakerId,
        executor_id: crate::events::ParticipantId,
        reason: StopReason,
        summary: String,
    },
    Usage {
        speaker: SpeakerId,
        usage: Usage,
    },
    AgentError {
        speaker: SpeakerId,
        message: String,
        recoverable: bool,
    },
    SessionError {
        speaker: SpeakerId,
        code: String,
        detail: String,
    },
    History {
        speaker: SpeakerId,
        reason: HistoryReason,
        targets: Vec<u64>,
        summary: Option<String>,
    },
    Context {
        speaker: SpeakerId,
        source: ContextSource,
        content: String,
    },
}

impl Entry {
    /// 这一行稳定的名字，供 `--kind` 用。
    pub fn kind(&self) -> &'static str {
        match self {
            Entry::Message { .. } => "MessageCompleted",
            Entry::Tool { .. } => "Tool",
            Entry::RoundStarted { .. } => "RoundStarted",
            Entry::RoundEnded { .. } => "RoundEnded",
            Entry::SessionEnded { .. } => "SessionEnded",
            Entry::TurnStarted { .. } => "TurnStarted",
            Entry::Divergence { .. } => "DivergenceRecorded",
            Entry::TurnEnded { .. } => "TurnEnded",
            Entry::PermissionAsked { .. } => "PermissionAsked",
            Entry::PermissionDecided { .. } => "PermissionDecided",
            Entry::Hook { .. } => "HookExecuted",
            Entry::ExecutorSpawned { .. } => "ExecutorSpawned",
            Entry::ExecutorFinished { .. } => "ExecutorFinished",
            Entry::Usage { .. } => "UsageRecorded",
            Entry::AgentError { .. } => "AgentError",
            Entry::SessionError { .. } => "SessionError",
            Entry::History { .. } => "HistorySuperseded",
            Entry::Context { .. } => "ContextInjected",
        }
    }

    /// 这一行归到哪位发言者，如果有一位。
    pub fn speaker(&self) -> Option<&SpeakerId> {
        match self {
            Entry::Message { speaker, .. }
            | Entry::Tool { speaker, .. }
            | Entry::TurnStarted { speaker, .. }
            | Entry::TurnEnded { speaker, .. }
            | Entry::PermissionAsked { speaker, .. }
            | Entry::PermissionDecided { speaker, .. }
            | Entry::Hook { speaker, .. }
            | Entry::ExecutorSpawned { speaker, .. }
            | Entry::ExecutorFinished { speaker, .. }
            | Entry::Usage { speaker, .. }
            | Entry::AgentError { speaker, .. }
            | Entry::SessionError { speaker, .. }
            | Entry::History { speaker, .. }
            | Entry::Context { speaker, .. } => Some(speaker),
            Entry::RoundStarted { .. }
            | Entry::RoundEnded { .. }
            | Entry::SessionEnded { .. }
            | Entry::Divergence { .. } => None,
        }
    }

    /// `--only-error` 是否保留这一行：失败的调用、失败或被中断的回合、一条错误，或者以错误收尾的
    /// 一轮。
    pub fn is_error(&self) -> bool {
        match self {
            Entry::Tool { ok, .. } => *ok == Some(false),
            Entry::TurnEnded { reason, .. } => {
                matches!(reason, StopReason::Error | StopReason::Aborted)
            }
            Entry::RoundEnded { reason, .. } => {
                matches!(reason, StopReason::Error | StopReason::Aborted)
            }
            Entry::ExecutorFinished { reason, .. } => *reason != StopReason::Completed,
            Entry::AgentError { .. } | Entry::SessionError { .. } => true,
            _ => false,
        }
    }

    fn kind_matches(&self, wanted: &str) -> bool {
        canonical_kind(self.kind()) == canonical_kind(wanted)
    }
}

/// 把一个 kind 名折叠成 `--kind` 用来比较的唯一形态。
///
/// 大小写、下划线与横线都只是同一个事件名周围的装饰；合并后的工具行同时答应它取代的那三个名字。公
/// 开是因为 CLI 那句「这是用量行吗」的判断必须与过滤器一致，而两个归一化器迟早会不一致。
pub fn canonical_kind(kind: &str) -> String {
    let normalized: String = kind
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    // 合并后的工具行同时答应它取代的那三个名字。
    if matches!(
        normalized.as_str(),
        "tool" | "toolcall" | "toolcallstarted" | "toolcallcompleted"
    ) {
        "tool".to_owned()
    } else {
        normalized
    }
}

/// 构建按轮分组的转录。工具调用按 `tool_call_id` 与各自的结果合并，钩子的结局与它所评注的结果合
/// 并。
pub fn timeline(events: &[Event]) -> Timeline {
    let mut timeline = Timeline::default();
    let mut open: Option<usize> = None;
    // `tool_call_id -> (组, 行)`：一条结果可能落进转录已经走过的那一组，所以这一行是按身份找回来
    // 的。
    let mut tools: Vec<(ToolCallId, usize, usize)> = Vec::new();

    for event in events {
        match &event.payload {
            EventPayload::SessionStarted {
                session_id, cwd, ..
            } => {
                timeline.session_id = Some(session_id.clone());
                timeline.cwd = Some(cwd.clone());
            }
            EventPayload::RoundStarted {
                round: started,
                mode,
            } => {
                timeline.groups.push(TimelineGroup {
                    round: Some(*started),
                    mode: Some(*mode),
                    ended: None,
                    entries: vec![Entry::RoundStarted {
                        round: *started,
                        mode: *mode,
                    }],
                });
                open = Some(timeline.groups.len() - 1);
                continue;
            }
            EventPayload::RoundEnded {
                round: ended,
                reason,
            } => {
                let index = open_group(&mut timeline, &mut open);
                timeline.groups[index].entries.push(Entry::RoundEnded {
                    round: *ended,
                    reason: *reason,
                });
                timeline.groups[index].ended = Some(*reason);
                open = None;
                continue;
            }
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } => {
                let index = open_group(&mut timeline, &mut open);
                let entry = timeline.groups[index].entries.len();
                timeline.groups[index].entries.push(Entry::Tool {
                    speaker: event.speaker_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    tool: tool_name.clone(),
                    args: args.clone(),
                    ok: None,
                    output: None,
                    error: None,
                    duration_ms: None,
                    hook: None,
                });
                tools.push((tool_call_id.clone(), index, entry));
                continue;
            }
            EventPayload::ToolCallCompleted {
                tool_call_id,
                ok,
                output,
                error,
                duration_ms,
            } => {
                if let Some((_, group, entry)) = tools
                    .iter()
                    .rev()
                    .find(|(id, _, _)| id == tool_call_id)
                    .cloned()
                {
                    if let Some(Entry::Tool {
                        ok: slot_ok,
                        output: slot_output,
                        error: slot_error,
                        duration_ms: slot_duration,
                        ..
                    }) = timeline.groups[group].entries.get_mut(entry)
                    {
                        *slot_ok = Some(*ok);
                        *slot_output = output.clone();
                        *slot_error = error.clone();
                        *slot_duration = Some(*duration_ms);
                    }
                } else {
                    // 起点不在这份流上的结果是一种续跑残留；显示它，而不是丢掉它。
                    let index = open_group(&mut timeline, &mut open);
                    timeline.groups[index].entries.push(Entry::Tool {
                        speaker: event.speaker_id.clone(),
                        tool_call_id: tool_call_id.clone(),
                        tool: String::new(),
                        args: serde_json::Value::Null,
                        ok: Some(*ok),
                        output: output.clone(),
                        error: error.clone(),
                        duration_ms: Some(*duration_ms),
                        hook: None,
                    });
                }
                continue;
            }
            // 钩子的后续反馈属于它所评注的那条结果；钩子事件不携带 id，而循环紧接结果之后就发出
            // 它，所以最近的那条结果就是配对关系（spec §3、§19）。
            EventPayload::HookExecuted { point, outcome, .. }
                if point == hook_format::POINT_POST =>
            {
                if let Some(entry) = last_tool_entry(&mut timeline) {
                    *entry = Some(outcome.clone());
                }
                continue;
            }
            _ => {}
        }

        let index = open_group(&mut timeline, &mut open);
        if let Some(entry) = entry_of(event) {
            timeline.groups[index].entries.push(entry);
        }
    }

    timeline
}

/// 当前打开的组，首次使用时打开会话的前奏组。
fn open_group(timeline: &mut Timeline, open: &mut Option<usize>) -> usize {
    if let Some(index) = *open {
        return index;
    }
    timeline.groups.push(TimelineGroup::default());
    *open = Some(timeline.groups.len() - 1);
    timeline.groups.len() - 1
}

/// 最近的那一行工具调用，不管它落在哪里。
fn last_tool_entry(timeline: &mut Timeline) -> Option<&mut Option<String>> {
    for group in timeline.groups.iter_mut().rev() {
        for entry in group.entries.iter_mut().rev() {
            if let Entry::Tool { hook, .. } = entry {
                return Some(hook);
            }
        }
    }
    None
}

fn entry_of(event: &Event) -> Option<Entry> {
    let speaker = event.speaker_id.clone();
    Some(match &event.payload {
        EventPayload::MessageCompleted { role, text, .. } => Entry::Message {
            speaker,
            role: *role,
            text: text.clone(),
        },
        EventPayload::SessionEnded { reason } => Entry::SessionEnded { reason: *reason },
        EventPayload::TurnStarted { iteration, .. } => Entry::TurnStarted {
            speaker,
            iteration: *iteration,
        },
        EventPayload::DivergenceRecorded {
            round,
            topic,
            positions,
        } => Entry::Divergence {
            round: *round,
            topic: topic.clone(),
            positions: positions.clone(),
        },
        EventPayload::TurnEnded { reason } => Entry::TurnEnded {
            speaker,
            reason: *reason,
        },
        EventPayload::PermissionAsked {
            request_id,
            tool_call_id,
            request,
        } => Entry::PermissionAsked {
            speaker,
            request_id: request_id.clone(),
            tool_call_id: tool_call_id.clone(),
            request: request.clone(),
        },
        EventPayload::PermissionDecided {
            request_id,
            decision,
            source,
            reason,
        } => Entry::PermissionDecided {
            speaker,
            request_id: request_id.clone(),
            decision: *decision,
            source: *source,
            reason: reason.clone(),
        },
        EventPayload::HookExecuted {
            point,
            command,
            outcome,
        } => Entry::Hook {
            speaker,
            point: point.clone(),
            command: command.clone(),
            outcome: outcome.clone(),
        },
        EventPayload::ExecutorSpawned {
            executor_id,
            parent,
            brief,
        } => Entry::ExecutorSpawned {
            speaker,
            executor_id: executor_id.clone(),
            parent: parent.clone(),
            brief: brief.clone(),
        },
        EventPayload::ExecutorFinished {
            executor_id,
            reason,
            summary,
        } => Entry::ExecutorFinished {
            speaker,
            executor_id: executor_id.clone(),
            reason: *reason,
            summary: summary.clone(),
        },
        EventPayload::UsageRecorded { usage } => Entry::Usage {
            speaker,
            usage: *usage,
        },
        EventPayload::AgentError {
            message,
            recoverable,
        } => Entry::AgentError {
            speaker,
            message: message.clone(),
            recoverable: *recoverable,
        },
        EventPayload::SessionError { code, detail } => Entry::SessionError {
            speaker,
            code: code.clone(),
            detail: detail.clone(),
        },
        EventPayload::HistorySuperseded {
            reason,
            targets,
            summary,
        } => Entry::History {
            speaker,
            reason: *reason,
            targets: targets.clone(),
            summary: summary.clone(),
        },
        EventPayload::ContextInjected { source, content } => Entry::Context {
            speaker,
            source: source.clone(),
            content: content.clone(),
        },
        // 由上面的分组循环处理。
        EventPayload::SessionStarted { .. }
        | EventPayload::RoundStarted { .. }
        | EventPayload::RoundEnded { .. }
        | EventPayload::ToolCallStarted { .. }
        | EventPayload::ToolCallCompleted { .. } => return None,
    })
}

/// 作用于 [`Timeline`] 的过滤器（`sessions show`）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub round: Option<u32>,
    pub speaker: Option<SpeakerId>,
    pub kind: Option<String>,
    pub tool: Option<String>,
    pub only_error: bool,
}

impl Filter {
    /// 这个过滤器是否什么都没点名；那样调用方可以直接用它已经建好的转录，而不必克隆一份过滤后的
    /// 副本。
    pub fn is_empty(&self) -> bool {
        self.round.is_none()
            && self.speaker.is_none()
            && self.kind.is_none()
            && self.tool.is_none()
            && !self.only_error
    }
}

impl Timeline {
    /// 施加一个 [`Filter`]；空的组会掉出去，于是一个过滤后的视图仍然按轮分组，却没有空的小标题。
    pub fn filtered(&self, filter: &Filter) -> Timeline {
        let mut timeline = Timeline {
            session_id: self.session_id.clone(),
            cwd: self.cwd.clone(),
            groups: Vec::new(),
        };
        for group in &self.groups {
            if let Some(round) = filter.round {
                if group.round != Some(round) {
                    continue;
                }
            }
            let entries: Vec<Entry> = group
                .entries
                .iter()
                .filter(|entry| matches_filter(entry, filter))
                .cloned()
                .collect();
            if entries.is_empty() {
                continue;
            }
            timeline.groups.push(TimelineGroup {
                round: group.round,
                mode: group.mode,
                ended: group.ended,
                entries,
            });
        }
        timeline
    }
}

fn matches_filter(entry: &Entry, filter: &Filter) -> bool {
    if filter.only_error && !entry.is_error() {
        return false;
    }
    if let Some(speaker) = &filter.speaker {
        if entry.speaker() != Some(speaker) {
            return false;
        }
    }
    if let Some(kind) = &filter.kind {
        if !entry.kind_matches(kind) {
            return false;
        }
    }
    if let Some(tool) = &filter.tool {
        match entry {
            Entry::Tool { tool: name, .. } if name == tool => {}
            _ => return false,
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 文件历史
// ---------------------------------------------------------------------------

/// 一个被改过的文件，归到改了它的那个 agent 与那一轮。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub tool: String,
    pub speaker: SpeakerId,
    pub round: Option<u32>,
    pub seq: u64,
}

/// 按工作区对象看的视图：每一次成功的写入或编辑落在过的每一个文件。
///
/// 路径来自**结果**那一行（`wrote:`），不是来自调用的参数：`hook.pre` 可能改写过参数，所以结果是
/// 「实际写下了什么」的唯一记录（spec §16、§18）。
pub fn file_history(events: &[Event]) -> Vec<FileChange> {
    let mut round: Option<u32> = None;
    let mut names: BTreeMap<ToolCallId, String> = BTreeMap::new();
    let mut changes = Vec::new();

    for event in events {
        match &event.payload {
            EventPayload::RoundStarted { round: started, .. } => round = Some(*started),
            EventPayload::RoundEnded { .. } => round = None,
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                ..
            } => {
                names.insert(tool_call_id.clone(), tool_name.clone());
            }
            EventPayload::ToolCallCompleted {
                tool_call_id,
                ok: true,
                output: Some(output),
                ..
            } => {
                if let Some(path) = output
                    .strip_prefix(WROTE_PATH_PREFIX)
                    .and_then(|rest| rest.lines().next())
                {
                    changes.push(FileChange {
                        path: path.to_owned(),
                        tool: names.get(tool_call_id).cloned().unwrap_or_default(),
                        speaker: event.speaker_id.clone(),
                        round,
                        seq: event.seq,
                    });
                }
            }
            _ => {}
        }
    }
    changes
}

// ---------------------------------------------------------------------------
// 统计
// ---------------------------------------------------------------------------

/// 一个有名字的计数，好让 JSON 消费方不必依赖 map 的顺序就能读一个分布。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Count {
    pub name: String,
    pub count: usize,
}

/// 固定的指标集（`sessions stats`），全都是对事件流的一次 group-by。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub session: SessionStats,
    /// 每位参与者一行：讨论者、合成器，以及每一个执行者。
    pub speakers: Vec<SpeakerStats>,
    pub rounds: Vec<RoundStats>,
    pub absence: AbsenceStats,
    pub executors: ExecutorStats,
    pub edits: EditStats,
    pub guards: GuardStats,
    pub permissions: PermissionStats,
    pub hooks: HookStats,
    /// 两边记下了分歧的轮次。
    pub divergences: usize,
    /// `divergences / debate rounds`；一轮辩论都没有时是 `None`。
    pub divergence_rate: Option<f64>,
    /// 辩论阶段是怎么结束的，按原因分。
    pub stops: Vec<Count>,
}

/// 会话级的合计。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionStats {
    pub tokens: Usage,
    pub cost: Option<f64>,
    pub calls: usize,
    pub messages: usize,
    pub rounds: usize,
}

/// 一位参与者的花费：token 总是有，钱只在这个发言者点了名时才有。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakerStats {
    pub speaker: SpeakerId,
    pub tokens: Usage,
    pub calls: usize,
    pub cost: Option<f64>,
    /// `cached / (cached + miss)`；什么都没计费时是 `None`。
    pub hit_rate: Option<f64>,
}

/// 一轮：它花掉的调用数以及它是怎么收尾的。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundStats {
    pub round: u32,
    pub mode: RoundMode,
    pub calls: usize,
    pub ended: Option<StopReason>,
}

/// 辩论轮次上的缺席图景（spec §15 的那个查询）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbsenceStats {
    /// 辩论轮数（合成那一轮不算）。
    pub rounds: usize,
    /// 恰好一边缺席、另一边作答了的轮次。
    pub one_sided: usize,
    /// 每位发言者当缺席那一方的次数。
    pub per_speaker: Vec<Count>,
    /// `one_sided / rounds`；一轮辩论都没有时是 `None`。
    pub rate: Option<f64>,
}

/// 一场会话派出过的执行者，以及它们为什么停下。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutorStats {
    pub spawned: usize,
    pub finished: usize,
    pub by_reason: Vec<Count>,
    pub tokens: Usage,
}

/// 编辑阶梯的结局（spec §8）：多少次编辑落了地、落在哪一级。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditStats {
    pub succeeded: usize,
    /// 匹配阶梯的降级分布（`exact` 与两次降级）。
    pub levels: Vec<Count>,
    /// 因为没有一级匹配得上而被拒的编辑 —— 就是撤掉路径读权限的那种情况。
    pub failed_matches: usize,
}

/// 两条护栏，它们的拒绝在别处看不见（spec §7）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardStats {
    pub read_before_write: usize,
    pub invalidated_reads: usize,
}

/// 问过的权限询问，以及它们是怎么裁定的。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionStats {
    pub asked: usize,
    pub decided: Vec<Count>,
}

/// 挂着的钩子在这场会话里做了什么。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookStats {
    pub executed: usize,
    pub pre: usize,
    pub post: usize,
    /// 把反馈带给了模型的钩子结局。
    pub feedback: usize,
    /// 钩子失败或超时（前置钩子失败即关闭、后置反馈被丢掉）。
    pub failed: usize,
    pub outcomes: Vec<Count>,
}

/// 每位发言者按哪个模型计价，供知道名册的视图用。
///
/// `UsageRecorded` 不携带模型 —— 名册住在配置里、不在流上 —— 所以只有在调用方点名模型时才显示
/// 钱。token 与命中率不需要模型，总是会报。路由规则用的是 [`SessionConfig::model_for`] 的，不是
/// 它的第二份拷贝（spec §17）：被路由的合成器或执行者按它实际作答时用的模型计价。
#[derive(Debug, Clone)]
pub struct CostModel {
    config: SessionConfig,
}

impl CostModel {
    /// 为一场应答者全都用 `base_model` 的会话定价。
    pub fn new(base_model: impl Into<String>, pricing: PriceTable) -> Self {
        Self {
            config: SessionConfig::new(base_model).with_pricing(pricing),
        }
    }

    /// 施加配置里的 `[routing]` 覆盖，好让合成器与执行者按它们真正跑过的模型计价。
    pub fn with_routing(mut self, routing: &Routing) -> Self {
        routing.apply(&mut self.config);
        self
    }

    /// 一位发言者按哪个模型计价：系统唯一那条路由规则，作用在一位发言者上，而不是在这里重说一
    /// 遍。
    pub fn model_for(&self, speaker: &SpeakerId) -> &str {
        match speaker {
            SpeakerId::System => self.config.model_for(LandingPoint::Synthesizer),
            SpeakerId::Executor(_) => self.config.model_for(LandingPoint::Executor),
            // 讨论者绝不是一个落点（spec §17）：它用会话自己的模型作答。
            SpeakerId::Debater(_) | SpeakerId::User => &self.config.model,
        }
    }

    fn cost(&self, speaker: &SpeakerId, usage: Usage) -> Option<f64> {
        self.config.pricing.cost(self.model_for(speaker), usage)
    }
}

/// 算出这套指标。`cost` 可能是 `None`：钱只作显示，且需要一个流上不携带的模型（spec §17）。
pub fn stats(events: &[Event], cost: Option<&CostModel>) -> Stats {
    let mut agents: BTreeMap<SpeakerId, Usage> = BTreeMap::new();
    let mut agent_calls: BTreeMap<SpeakerId, usize> = BTreeMap::new();
    let mut rounds: Vec<RoundStats> = Vec::new();
    let mut round_index: BTreeMap<u32, usize> = BTreeMap::new();
    let mut stops: BTreeMap<String, usize> = BTreeMap::new();
    let mut hook_outcomes: BTreeMap<String, usize> = BTreeMap::new();
    let mut decisions: BTreeMap<String, usize> = BTreeMap::new();
    let mut levels: BTreeMap<String, usize> = BTreeMap::new();
    let mut executor_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut per_speaker_absent: BTreeMap<String, usize> = BTreeMap::new();

    let mut session = SessionStats {
        tokens: Usage::default(),
        cost: None,
        calls: 0,
        messages: 0,
        rounds: 0,
    };
    let mut executors = ExecutorStats {
        spawned: 0,
        finished: 0,
        by_reason: Vec::new(),
        tokens: Usage::default(),
    };
    let mut edits = EditStats {
        succeeded: 0,
        levels: Vec::new(),
        failed_matches: 0,
    };
    let mut guards = GuardStats {
        read_before_write: 0,
        invalidated_reads: 0,
    };
    let mut permissions = PermissionStats {
        asked: 0,
        decided: Vec::new(),
    };
    let mut hooks = HookStats {
        executed: 0,
        pre: 0,
        post: 0,
        feedback: 0,
        failed: 0,
        outcomes: Vec::new(),
    };
    let mut divergences = 0;
    let mut round: Option<u32> = None;

    for event in events {
        match &event.payload {
            EventPayload::SessionStarted { .. } => {}
            EventPayload::ContextInjected { .. } => {}
            EventPayload::SessionEnded { reason } => {
                *stops.entry(reason.as_str().to_owned()).or_default() += 1;
            }
            EventPayload::RoundStarted {
                round: started,
                mode,
            } => {
                round = Some(*started);
                session.rounds += 1;
                round_index.insert(*started, rounds.len());
                rounds.push(RoundStats {
                    round: *started,
                    mode: *mode,
                    calls: 0,
                    ended: None,
                });
            }
            EventPayload::RoundEnded {
                round: ended,
                reason,
            } => {
                if let Some(index) = round_index.get(ended) {
                    rounds[*index].ended = Some(*reason);
                }
                *stops.entry(reason.as_str().to_owned()).or_default() += 1;
                round = None;
            }
            EventPayload::DivergenceRecorded { .. } => divergences += 1,
            EventPayload::TurnStarted { .. } => {}
            EventPayload::MessageCompleted { .. } => session.messages += 1,
            EventPayload::ToolCallStarted { .. } => {}
            EventPayload::ToolCallCompleted {
                ok, output, error, ..
            } => {
                let (levels, failed_match) = tally_tool(
                    *ok,
                    output.as_deref(),
                    error.as_deref(),
                    &mut levels,
                    &mut guards,
                );
                edits.succeeded += levels;
                if failed_match {
                    edits.failed_matches += 1;
                }
            }
            EventPayload::UsageRecorded { usage } => {
                session.tokens.accumulate(*usage);
                session.calls += 1;
                agents
                    .entry(event.speaker_id.clone())
                    .or_default()
                    .accumulate(*usage);
                *agent_calls.entry(event.speaker_id.clone()).or_default() += 1;
                if let Some(round) = round {
                    if let Some(index) = round_index.get(&round) {
                        rounds[*index].calls += 1;
                    }
                }
                if matches!(event.speaker_id, SpeakerId::Executor(_)) {
                    executors.tokens.accumulate(*usage);
                }
            }
            EventPayload::TurnEnded { .. } => {}
            EventPayload::PermissionAsked { .. } => permissions.asked += 1,
            EventPayload::PermissionDecided { decision, .. } => {
                *decisions.entry(decision.as_str().to_owned()).or_default() += 1;
            }
            EventPayload::HookExecuted { point, outcome, .. } => {
                hooks.executed += 1;
                match point.as_str() {
                    hook_format::POINT_PRE => hooks.pre += 1,
                    hook_format::POINT_POST => hooks.post += 1,
                    _ => {}
                }
                if hook_format::feedback_text(outcome).is_some() {
                    hooks.feedback += 1;
                }
                if outcome.starts_with(hook_format::FAILED_PREFIX) {
                    hooks.failed += 1;
                }
                *hook_outcomes.entry(outcome.clone()).or_default() += 1;
            }
            EventPayload::ExecutorSpawned { .. } => executors.spawned += 1,
            EventPayload::ExecutorFinished { reason, .. } => {
                executors.finished += 1;
                *executor_reasons
                    .entry(reason.as_str().to_owned())
                    .or_default() += 1;
            }
            EventPayload::AgentError { .. } | EventPayload::SessionError { .. } => {}
            EventPayload::HistorySuperseded { .. } => {}
        }
    }

    // 缺席是关于轮次的问题，所以它按轮向流提问，而不是在走流的过程中重建（spec §15 的那个查询）。
    let debate_rounds: Vec<u32> = rounds
        .iter()
        .filter(|round| round.mode != RoundMode::Synthesis)
        .map(|round| round.round)
        .collect();
    let mut one_sided = 0;
    for round in &debate_rounds {
        let attendance = round_attendance(events, *round);
        if attendance.absent.len() == 1 {
            one_sided += 1;
        }
        for speaker in &attendance.absent {
            *per_speaker_absent.entry(speaker.to_string()).or_default() += 1;
        }
    }
    let absence = AbsenceStats {
        rounds: debate_rounds.len(),
        one_sided,
        per_speaker: counted(per_speaker_absent),
        rate: (!debate_rounds.is_empty()).then(|| one_sided as f64 / debate_rounds.len() as f64),
    };

    let speaker_stats: Vec<SpeakerStats> = agents
        .into_iter()
        .map(|(speaker, tokens)| {
            let billed = tokens.cached_tokens + tokens.miss_tokens;
            SpeakerStats {
                calls: agent_calls.get(&speaker).copied().unwrap_or(0),
                cost: cost.and_then(|cost| cost.cost(&speaker, tokens)),
                hit_rate: (billed > 0).then(|| tokens.cached_tokens as f64 / billed as f64),
                speaker,
                tokens,
            }
        })
        .collect();

    // 会话总计就是上面各行之和，所以 token 与钱的算术永远不会互相漂移。
    session.tokens = speaker_stats
        .iter()
        .fold(Usage::default(), |mut total, speaker| {
            total.accumulate(speaker.tokens);
            total
        });
    session.cost = speaker_stats
        .iter()
        .map(|speaker| speaker.cost)
        .collect::<Option<Vec<f64>>>()
        .map(|amounts| amounts.iter().sum());

    Stats {
        session,
        speakers: speaker_stats,
        rounds,
        absence,
        executors: ExecutorStats {
            by_reason: counted(executor_reasons),
            ..executors
        },
        edits: EditStats {
            levels: counted(levels),
            ..edits
        },
        guards,
        permissions: PermissionStats {
            decided: counted(decisions),
            ..permissions
        },
        hooks: HookStats {
            outcomes: counted(hook_outcomes),
            ..hooks
        },
        divergences,
        divergence_rate: (!debate_rounds.is_empty())
            .then(|| divergences as f64 / debate_rounds.len() as f64),
        stops: counted(stops),
    }
}

/// 为那两个沉默的指标分类一次已完成的工具调用的结果文本。
///
/// 返回 `(报出的匹配级别数, 这次失败是不是一次失败的匹配)`。
fn tally_tool(
    ok: bool,
    output: Option<&str>,
    error: Option<&str>,
    levels: &mut BTreeMap<String, usize>,
    guards: &mut GuardStats,
) -> (usize, bool) {
    if ok {
        return output
            .map(|output| (count_levels(output, levels), false))
            .unwrap_or((0, false));
    }
    let Some(error) = error else {
        return (0, false);
    };
    if error.starts_with(READ_BEFORE_WRITE_PREFIX) {
        guards.read_before_write += 1;
    }
    // 这次作废不是单独的字段：一次失败的匹配**就是**撤掉读权限的那种情况。生产者会给路径加前缀
    // （`edit_file` 报的是 `"<path>: {error}"`），所以用该变体自己的渲染来匹配后缀 —— 解析器不
    // 可能与生产者的文本漂移（spec §18）。
    if error.ends_with(&EditError::NoMatch.to_string()) {
        guards.invalidated_reads += 1;
        return (0, true);
    }
    (0, false)
}

/// 数一次成功编辑结果里报出的匹配级别。
///
/// 那一行是 `edit match level: <level>: …`；级别通过生产者的前缀解析，所以两者不可能漂移
/// （spec §18）。
fn count_levels(output: &str, levels: &mut BTreeMap<String, usize>) -> usize {
    let mut seen = 0;
    for line in output.lines() {
        let Some(rest) = line.strip_prefix(MATCH_LEVEL_PREFIX) else {
            continue;
        };
        let level = rest
            .split(|ch: char| ch == ':' || ch.is_whitespace())
            .next()
            .unwrap_or_default();
        if level.is_empty() {
            continue;
        }
        *levels.entry(level.to_owned()).or_default() += 1;
        seen += 1;
    }
    seen
}

/// 一个分布，最频繁的在前，然后按名字排，好让 JSON 保持稳定。
fn counted(counts: BTreeMap<String, usize>) -> Vec<Count> {
    let mut counted: Vec<Count> = counts
        .into_iter()
        .map(|(name, count)| Count { name, count })
        .collect();
    counted.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.name.cmp(&right.name))
    });
    counted
}
