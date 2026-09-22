//! `sessions replay`: recompute a request from the stream (spec §18).
//!
//! The acceptance for "the event log is the single source of truth" is that the
//! `messages` recomputed from the log equal the `messages` that were actually
//! sent to the provider. That can only hold if replay is the loop's **own**
//! pipeline — projection, the private identity, the trim policy — rather than a
//! second implementation that agrees today. [`super::build_messages`] is that
//! pipeline; this module supplies the one input the log does not carry (which
//! call is being reproduced) and the identity, derived from the stream.
//!
//! # Reproducing one call, not the final state
//!
//! A finished stream holds the response to the call being reproduced, so
//! projecting the whole log would include events that did not exist when the
//! request went out. The cut is the speaker's `TurnStarted` for that call: the
//! loop snapshots the stream just before it appends that event, so
//! `seq < TurnStarted` is exactly what the request saw. Combined with
//! [`TurnScope`], a recomputed round window is the live one.
//!
//! The synthesizer is not a turn: it has no `TurnStarted` and its request is not
//! a projection at all — it is its private identity plus
//! [`crate::discussion::synthesis_prompt`]. Both are functions of the stream, so
//! it is reproduced the same way.

use crate::context;
use crate::discussion;
use crate::events::{Event, EventPayload, Role, RoundMode, SpeakerId};
use crate::provider::capability::ModelCaps;
use crate::provider::Message;

use super::executor::EXECUTOR_IDENTITY;
use super::{build_messages, scoped_events_slice, TurnScope};

/// Why a request could not be recomputed from the stream.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    #[error("this session has no round {0} to replay")]
    UnknownRound(u32),
    #[error(
        "this speaker never opened a model call, so there is no request to replay; \
         --round names a round the speaker did not answer in"
    )]
    NoProviderCall(SpeakerId),
    #[error("this discussion has more than one round, so --round is required to pick one")]
    RoundRequired,
    #[error("this session has no synthesis round to replay")]
    NoSynthesis,
    #[error("round {round} is not a synthesis round")]
    NotSynthesis { round: u32 },
    #[error("the stream records no question for the synthesizer to have answered")]
    NoQuestion,
    #[error("the recomputed request does not fit the model's context window: {0}")]
    Trim(#[from] context::TrimError),
}

/// Recompute what `speaker` sent to its provider.
///
/// `round` selects a discussion round; it is required for a debater in a
/// discussion and names the synthesis round for the synthesizer. `None` means
/// the whole stream, which is a single-agent turn's scope.
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
    // The cut for the call being reproduced: everything the loop had appended
    // when it took its snapshot. A source with no cut has no call to reproduce.
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

/// How much of the stream the replayed call saw.
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
        // A debater out of a discussion is an executor's shape, not a plain
        // session's: only its own events are in its window (spec §16).
        None if matches!(speaker, SpeakerId::Executor(_)) => Ok(TurnScope::Executor),
        // A discussion's debater needs a round: without one, "the whole stream"
        // would put the other side's answers in the window, which is exactly what
        // the round cut exists to prevent. Refusing is honest; guessing is not.
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

/// The `seq` of the speaker's last `TurnStarted` in `round` (or in the stream,
/// when `round` is `None`).
///
/// The loop appends `TurnStarted` after the snapshot it projects, so this seq is
/// the exclusive upper bound of what the call saw. The **last** one is the one
/// whose request is reproducible from a finished stream: later iterations live on
/// top of earlier ones, and a turn's final response is appended only after its
/// own call.
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

/// The private identity an agent's request leads with.
///
/// It never enters the stream (spec §15), so replay derives it from the stream's
/// shape instead: a discussion debater has the protocol instruction, a debater
/// without rounds is a plain session with this program's identity, and an
/// executor has its own constant. The synthesizer is handled by [`synthesizer`]
/// before this is consulted — its identity and its prompt are one unit — and the
/// user has none.
fn identity_for(events: &[Event], speaker: &SpeakerId) -> Option<String> {
    match speaker {
        SpeakerId::Executor(_) => Some(EXECUTOR_IDENTITY.to_owned()),
        SpeakerId::Debater(name) => Some(if has_rounds(events) {
            discussion::debater_identity(name.as_str())
        } else {
            super::agent_identity().to_owned()
        }),
        SpeakerId::System | SpeakerId::User => None,
    }
}

/// The synthesizer's request: its identity plus the prompt derived from the
/// stream (spec §15).
///
/// Not a projection: [`crate::discussion::synthesis_prompt`] renders every
/// round's answers and absences from the log, exactly as the closing call built
/// it.
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

    // The round the synthesis was recorded under: the materials are scoped to the
    // debate phase it closes (a session can carry more than one discussion).
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
