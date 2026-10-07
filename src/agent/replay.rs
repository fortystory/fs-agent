//! `sessions replay`：从流上重算一次请求（spec §18）。
//!
//! 「事件流是唯一真相源」的验收方式，是**从流重算出的 `messages` 等于当时真发给 provider
//! 的 `messages`**。这只有在 replay 就是循环**自己**那条流水线 —— 投影、私有身份、裁剪
//! 策略 —— 时才可能成立，而不是靠第二份「今天恰好也一致」的实现。
//! [`super::build_messages`] 就是那条流水线；本模块补上流不携带的那一个输入（正在复现的
//! 是哪一次调用），以及那份从流的形状上推出来的身份。
//!
//! # 复现一次调用，而不是最终状态
//!
//! 一条已经结束的流里装着被复现那次调用的响应，所以投影整条日志会把「请求发出时还不存在
//! 的事件」也算进去。切点就是该发言者这次调用对应的 `TurnStarted`：循环在追加那条事件之前
//! 刚好给流拍了一张快照，所以 `seq < TurnStarted` 正是那次请求看到的东西。再配上
//! [`TurnScope`]，重算出的轮次窗口就是当时那个活着的窗口。
//!
//! 合成器不是回合：它没有 `TurnStarted`，它的请求也根本不是一次投影 —— 那是它自己的私有
//! 身份加上 [`crate::discussion::synthesis_prompt`]。两者都是流上的函数，所以它同样能被
//! 复现出来。

use crate::context;
use crate::discussion;
use crate::events::{Event, EventPayload, Role, RoundMode, SpeakerId};
use crate::provider::Message;
use crate::provider::capability::ModelCaps;

use super::executor::executor_identity;
use super::{TurnScope, build_messages, scoped_events_slice};

/// 一次请求为什么无法从流上重算出来。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    #[error("这个会话没有第 {0} 轮可以重放")]
    UnknownRound(u32),
    #[error(
        "这位发言人从没发起过模型调用，所以没有请求可以重放；\
         --round 点名的是它没有作答的那一轮"
    )]
    NoProviderCall(SpeakerId),
    #[error("这次讨论不止一轮，所以必须用 --round 挑一轮")]
    RoundRequired,
    #[error("这个会话没有合成轮可以重放")]
    NoSynthesis,
    #[error("第 {round} 轮不是合成轮")]
    NotSynthesis { round: u32 },
    #[error("流上没有记下合成器作答过的问题")]
    NoQuestion,
    #[error("重算出来的请求装不进模型的上下文窗口：{0}")]
    Trim(#[from] context::TrimError),
}

/// 重算 `speaker` 发给它那个 provider 的内容。
///
/// `round` 用来选一次讨论的轮次：讨论里的讨论者必需，对合成器则是点名合成轮。`None` 表示
/// 整条流，也就是单 agent 回合的作用域。
pub fn replay(
    events: &[Event],
    speaker: &SpeakerId,
    round: Option<u32>,
    caps: &ModelCaps,
) -> Result<Vec<Message>, ReplayError> {
    if matches!(speaker, SpeakerId::System) {
        return synthesizer(events, round);
    }

    let scope = scope_for(events, speaker, round)?;
    // 这次被复现调用的切点：循环拍快照时它已经追加过的全部内容。没有切点的发言者就没有
    // 可复现的调用。
    let cut = last_call_cut(events, speaker, round)
        .ok_or(ReplayError::NoProviderCall(speaker.clone()))?;
    let snapshot: Vec<Event> = events
        .iter()
        .filter(|event| event.seq < cut)
        .cloned()
        .collect();
    let scoped = scoped_events_slice(&snapshot, speaker, scope);
    let identity = identity_for(events, speaker);
    Ok(build_messages(
        &scoped,
        speaker,
        caps,
        identity.as_deref(),
        &context::TrimPolicy::default(),
    )?)
}

/// 被复现的那次调用看到了多少流。
fn scope_for(
    events: &[Event],
    speaker: &SpeakerId,
    round: Option<u32>,
) -> Result<TurnScope, ReplayError> {
    match round {
        Some(round) => {
            let started = events
                .iter()
                .find(|event| {
                    matches!(
                        &event.payload,
                        EventPayload::RoundStarted { round: started, .. } if *started == round
                    )
                })
                .ok_or(ReplayError::UnknownRound(round))?;
            Ok(TurnScope::Round {
                before_seq: started.seq,
            })
        }
        // 一个不在讨论里的讨论者，形状是执行者而不是普通会话：它的窗口里只有自己的事件
        // （spec §16）。
        None if matches!(speaker, SpeakerId::Executor(_)) => Ok(TurnScope::Executor),
        // 讨论里的讨论者需要一个轮次：不给的话，「整条流」会把对面那一方的回答放进窗口，
        // 而轮次切点存在的意义正是防这个。拒绝是诚实的，猜不是。
        None if has_rounds(events) && matches!(speaker, SpeakerId::Debater(_)) => {
            Err(ReplayError::RoundRequired)
        }
        None => Ok(TurnScope::Whole),
    }
}

fn has_rounds(events: &[Event]) -> bool {
    events
        .iter()
        .any(|event| matches!(event.payload, EventPayload::RoundStarted { .. }))
}

/// 该发言者在 `round` 里（`round` 为 `None` 时则在整条流里）最后一个 `TurnStarted`
/// 的 `seq`。
///
/// 循环是在它投影的那张快照**之后**追加 `TurnStarted` 的，所以这个 seq 就是那次调用所见
/// 内容的上界（不含）。能从一个已经结束的流里复现出其请求的那一个，是**最后一个**：更晚
/// 的迭代叠在更早的之上，而一个回合的最终回复只在它自己那次调用之后才追加。
fn last_call_cut(events: &[Event], speaker: &SpeakerId, round: Option<u32>) -> Option<u64> {
    let mut current: Option<u32> = None;
    let mut cut = None;
    for event in events {
        match &event.payload {
            EventPayload::RoundStarted { round: started, .. } => current = Some(*started),
            EventPayload::RoundEnded { .. } => current = None,
            EventPayload::TurnStarted { agent, .. } if agent == speaker => {
                if round.is_none_or(|round| current == Some(round)) {
                    cut = Some(event.seq);
                }
            }
            _ => {}
        }
    }
    cut
}

/// 一个 agent 的请求打头的那份私有身份。
///
/// 它从不进流（spec §15），所以 replay 改从流的形状上把它推出来：讨论里的讨论者拿到的是
/// 协议指令，没有轮次的讨论者就是一个带着本程序身份的普通会话，执行者有它自己的身份。
/// 合成器在这之前就由 [`synthesizer`] 处理掉了 —— 它的身份和它的提示词是一个整体 —— 而
/// 用户没有身份。
fn identity_for(events: &[Event], speaker: &SpeakerId) -> Option<String> {
    match speaker {
        SpeakerId::Executor(_) => Some(executor_identity()),
        SpeakerId::Debater(name) => Some(if has_rounds(events) {
            discussion::debater_identity(name.as_str())
        } else {
            super::agent_identity()
        }),
        SpeakerId::System | SpeakerId::User => None,
    }
}

/// 合成器的请求：它的身份，加上从流上推出来的提示词（spec §15）。
///
/// 这不是一次投影：[`crate::discussion::synthesis_prompt`] 从日志里渲染出每一轮的作答与
/// 缺席，与那次收尾调用当时构造出来的完全一致。
fn synthesizer(events: &[Event], round: Option<u32>) -> Result<Vec<Message>, ReplayError> {
    let started = match round {
        Some(round) => events
            .iter()
            .find(|event| {
                matches!(
                    &event.payload,
                    EventPayload::RoundStarted { round: started, mode: RoundMode::Synthesis }
                        if *started == round
                )
            })
            .ok_or_else(|| {
                if events.iter().any(|event| {
                    matches!(
                        &event.payload,
                        EventPayload::RoundStarted { round: started, .. } if *started == round
                    )
                }) {
                    ReplayError::NotSynthesis { round }
                } else {
                    ReplayError::UnknownRound(round)
                }
            })?,
        None => events
            .iter()
            .rev()
            .find(|event| {
                matches!(
                    event.payload,
                    EventPayload::RoundStarted {
                        mode: RoundMode::Synthesis,
                        ..
                    }
                )
            })
            .ok_or(ReplayError::NoSynthesis)?,
    };

    // 这次合成记在哪个轮次之下：材料的作用域是它所收尾的那个辩论阶段（一个会话里可以装
    // 不止一场讨论）。
    let EventPayload::RoundStarted {
        round: synthesis_round,
        ..
    } = &started.payload
    else {
        return Err(ReplayError::NoSynthesis);
    };

    let question = events
        .iter()
        .filter(|event| event.seq <= started.seq)
        .rev()
        .find_map(|event| match &event.payload {
            EventPayload::MessageCompleted {
                role: Role::User,
                text,
                ..
            } => Some(text.clone()),
            _ => None,
        })
        .ok_or(ReplayError::NoQuestion)?;

    let up_to: Vec<Event> = events
        .iter()
        .filter(|event| event.seq <= started.seq)
        .cloned()
        .collect();
    let prompt = discussion::synthesis_prompt(
        &question,
        &up_to,
        discussion::debate_phase_start(&up_to, *synthesis_round),
    );

    Ok(vec![
        Message::System {
            content: discussion::synthesizer_identity(),
            name: None,
        },
        Message::User {
            content: prompt,
            name: None,
            injected: false,
        },
    ])
}
