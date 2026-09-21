//! The pure half of the discussion protocol (spec §15).
//!
//! The mechanical verdict on "did the two debaters conclude the same thing" is a
//! pure function of their two answers, and it must stay one: the spec's Testing
//! Decisions list it among the functions with no mock seam of their own. Two
//! properties carry the weight:
//!
//! * **zero extra calls** — no judge, no `response_format`, no controlled
//!   polarity labels. The two one-line conclusions *are* the divergence record.
//! * **no false negatives** — equal conclusions that are spelled with different
//!   emphasis, punctuation or case must still compare equal, which is why the
//!   comparison normalizes hard and also accepts substring containment in either
//!   direction. The price is a bounded false positive, and the bound is the
//!   round cap (spec §15).
//!
//! The marker is a shared constant because the protocol instruction (the
//! debater's private identity) and this parser must not drift: a marker the
//! prompt teaches but the parser does not know would silently make every round
//! divergent. This is the same "convention text with one shared constant" shape
//! the hook outcome uses (spec §18).

use crate::events::{Event, EventPayload, SpeakerId, StopReason};

/// The delimiter that starts a debater's one-line free conclusion.
///
/// Language-neutral on purpose: the answer is written in whatever language the
/// question was asked in, so the delimiter must not be a word in one of them.
pub const CONCLUSION_MARKER: &str = "CONCLUSION:";

/// Characters that carry no propositional weight in a one-line conclusion.
///
/// Markdown emphasis, quoting, brackets and sentence punctuation are all
/// decoration around the same conclusion, so none of them may decide agreement.
const IGNORED: &[char] = &[
    '*', '_', '`', '"', '\'', '“', '”', '‘', '’', '(', ')', '（', '）', '[', ']', '【', '】', '<',
    '>', '#', '-', '+', '~', '。', '．', '.', '，', ',', '、', '；', ';', '：', ':', '！', '!',
    '？', '?', '…', '—', '|',
];

/// Decoration a model may wrap the marker in without meaning anything by it:
/// list bullets, blockquote markers, emphasis, indentation.
const MARKER_LEAD: &[char] = &['*', '_', '`', '-', '+', '>', '#', ' ', '\t'];

/// The debater's one-line free conclusion: the last line that begins with
/// [`CONCLUSION_MARKER`] and has something after it.
///
/// Scanning from the end rather than demanding the marker be the very last line
/// keeps a trailing courtesy line from erasing a conclusion the model *did*
/// write. Decoration around the marker is tolerated for the same reason the
/// comparison normalizes hard: reading `**CONCLUSION:** x` as "no conclusion"
/// would be a false negative, and a false negative buys a second round for
/// nothing. A genuinely marker-less answer has no conclusion, and the protocol
/// must treat that as "cannot be shown to agree" rather than as agreement
/// (spec §15).
pub fn conclusion_of(answer: &str) -> Option<&str> {
    answer
        .lines()
        .rev()
        .filter_map(|line| {
            line.trim()
                .trim_start_matches(MARKER_LEAD)
                .strip_prefix(CONCLUSION_MARKER)
                .map(|rest| rest.trim().trim_matches(IGNORED).trim())
        })
        .find(|rest| !rest.is_empty())
}

/// Fold a conclusion down to the text that a comparison may look at.
///
/// Whitespace collapses to single spaces, [`IGNORED`] characters disappear, and
/// the rest is lowercased.
pub fn normalize(conclusion: &str) -> String {
    let mut normalized = String::with_capacity(conclusion.len());
    let mut pending_space = false;
    for character in conclusion.chars() {
        if character.is_whitespace() {
            pending_space = !normalized.is_empty();
            continue;
        }
        if IGNORED.contains(&character) {
            continue;
        }
        if pending_space {
            normalized.push(' ');
            pending_space = false;
        }
        normalized.extend(character.to_lowercase());
    }
    normalized
}

/// Do two answers state the same conclusion?
///
/// Exact equality or substring containment in either direction. Both sides must
/// actually *have* a conclusion: an empty normalized conclusion is a substring of
/// everything, so without this guard a marker-less answer would agree with every
/// answer in the session — the "one answer reads as consensus" failure the spec
/// singles out as the most dangerous misread on this path.
pub fn answers_agree(left: &str, right: &str) -> bool {
    let (Some(left), Some(right)) = (conclusion_of(left), conclusion_of(right)) else {
        return false;
    };
    let (left, right) = (normalize(left), normalize(right));
    if left.is_empty() || right.is_empty() {
        return false;
    }
    left == right || left.contains(&right) || right.contains(&left)
}

/// What one round's slice of the stream says about who took part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoundAttendance {
    /// Debater answers that landed in this round, in `seq` order, with the full
    /// revealed text. Private reasoning is not carried: the protocol reveals the
    /// speech and never the reasoning behind it (spec §15).
    pub answers: Vec<(SpeakerId, String)>,
    /// Debaters whose turn ended in `Error` in this round **and** left no answer.
    ///
    /// This is the spec's absence query, derived from the stream rather than
    /// stored in a field. It matters because the alternative reading — "only one
    /// answer came back, so that is the consensus" — is the most dangerous
    /// misread on this path, and a derived query is the only shape that cannot go
    /// stale: there is no field to forget to set.
    pub absent: Vec<SpeakerId>,
}

/// Who answered in `round`, and which debaters were absent from it.
///
/// Only debaters count. An executor's turn lands on the same stream inside the
/// round that spawned it (spec §16), and the synthesizer's product is `System`
/// (spec §2); counting either as a debater's answer would let the protocol find
/// agreement between a debater and its own executor.
pub fn round_attendance(events: &[Event], round: u32) -> RoundAttendance {
    let mut current: Option<u32> = None;
    let mut answers: Vec<(SpeakerId, String)> = Vec::new();
    let mut failed: Vec<SpeakerId> = Vec::new();

    for event in events {
        match &event.payload {
            EventPayload::RoundStarted { round: started, .. } => current = Some(*started),
            EventPayload::RoundEnded { .. } => current = None,
            _ => {
                if current != Some(round) {
                    continue;
                }
                let SpeakerId::Debater(_) = &event.speaker_id else {
                    continue;
                };
                match &event.payload {
                    EventPayload::MessageCompleted { text, .. } => {
                        answers.push((event.speaker_id.clone(), text.clone()));
                    }
                    EventPayload::TurnEnded {
                        reason: StopReason::Error,
                    } if !failed.contains(&event.speaker_id) => {
                        failed.push(event.speaker_id.clone());
                    }
                    _ => {}
                }
            }
        }
    }

    let absent = failed
        .into_iter()
        .filter(|speaker| !answers.iter().any(|(answered, _)| answered == speaker))
        .collect();

    RoundAttendance { answers, absent }
}

/// The mechanical verdict on one debate round.
///
/// Nothing here decides anything about *stopping* — that is
/// [`crate::discussion::plan_after_round`]. This is only what the two (or more)
/// conclusions say to each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundOutcome {
    /// At least two answers, and every adjacent pair states the same conclusion.
    Agreed,
    /// At least two answers that conflict.
    Diverged,
    /// Fewer than two answers: nothing was compared, so nothing was agreed. The
    /// missing sides are the ones [`RoundAttendance::absent`] names.
    Incomplete,
}

/// Read the verdict off one round's attendance.
///
/// One comparison, because the roster is two debaters and assembly refuses any
/// other size (spec §15: N > 2 would reopen "N = 2 does not arbitrate"). Fewer
/// than two answers is not a verdict but an absence.
pub fn round_outcome(attendance: &RoundAttendance) -> RoundOutcome {
    let [first, second, ..] = &attendance.answers[..] else {
        return RoundOutcome::Incomplete;
    };
    if answers_agree(&first.1, &second.1) {
        RoundOutcome::Agreed
    } else {
        RoundOutcome::Diverged
    }
}
