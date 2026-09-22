//! Discussion protocol boundary (spec §15).
//!
//! This module is the protocol's **rules**: the mechanical verdict on whether two
//! debaters agree, who took part in a round, how a round is allowed to end, and
//! the private instructions and synthesizer prompt that the protocol is made of.
//! It is policy, and it is pure.
//!
//! The **control flow** that applies these rules — the round loop, the two
//! concurrent turns, the closing call — lives in the `agent` layer, because that
//! layer is the only writer of the event stream (through its one `append_event`
//! path) and the only caller of a provider (spec §3). So `discussion` never
//! touches `provider`: it decides, `agent` writes.

pub mod protocol;

use std::collections::BTreeSet;

use crate::events::{Event, EventPayload, RoundMode, StopReason};
use protocol::round_attendance;

/// How many debaters v1 runs.
///
/// Fixed at two, and enforced at assembly: the mechanical verdict is a pairwise
/// comparison, and N > 2 would reopen the "N = 2 does not arbitrate" decision
/// (spec §15, Out of Scope).
pub const DEBATERS: usize = 2;

/// The round cap: one independent round, then one targeted round.
pub const DEFAULT_MAX_ROUNDS: u32 = 2;

/// What the protocol does after a debate round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundPlan {
    /// Stop debating, and end the round with this reason.
    Stop(StopReason),
    /// Open one targeted round: each side sees the other's answer and is asked to
    /// respond to the divergence.
    TargetedRound,
}

/// Apply the round rules: does the debate stop here, and with which reason?
///
/// The four reasons the spec fixes for `RoundEnded` are exactly this function's
/// `Stop` results. `BudgetExhausted` is the fifth input to that vocabulary and
/// belongs to the budget gate (ticket 14), which stops the debate *before* this
/// question is asked.
pub fn plan_after_round(outcome: protocol::RoundOutcome, round: u32, max_rounds: u32) -> RoundPlan {
    use protocol::RoundOutcome;
    match outcome {
        // Agreement after a targeted round is a convergence; agreement in the
        // first round means no divergence ever arose, which is a different fact
        // about the discussion and gets a different reason.
        RoundOutcome::Agreed if round > 1 => RoundPlan::Stop(StopReason::Consensus),
        RoundOutcome::Agreed => RoundPlan::Stop(StopReason::NoDivergence),
        RoundOutcome::Diverged if round < max_rounds => RoundPlan::TargetedRound,
        RoundOutcome::Diverged => RoundPlan::Stop(StopReason::RoundsExhausted),
        // Fewer than two answers: nothing was compared, so nothing was agreed.
        // The absence is already on the stream as the missing side's own
        // `TurnEnded { Error }`, which is why this reason may claim so little.
        RoundOutcome::Incomplete => RoundPlan::Stop(StopReason::NoDivergence),
    }
}

/// A debater's private identity: the protocol instruction.
///
/// It lives here — not in the stream — because otherwise "a later round's
/// `messages` can be recomputed from the stream" would be false the moment the
/// instruction changed anything (spec §15).
///
/// It is **constant for the whole discussion** on purpose. The targeted round's
/// rule is stated here once rather than injected per round: the system prompt is
/// the head of the prefix, and rewriting it mid-discussion would invalidate the
/// whole prefix cache (spec §4). The round the debater is in is visible in the
/// projection's `[轮 N · 名字]` prefixes and in the other side's revealed answer.
pub fn debater_identity(name: &str) -> String {
    format!(
        "你是本次讨论中的一位讨论者，代号「{name}」。同一次讨论里还有另一位讨论者，\
你们各自独立回答同一个问题，谁的判断都不从属于对方。\n\
\n\
作答规则：\n\
1. 先写出你的作答正文。\n\
2. 正文结束后另起一行，以 `{marker}` 开头写出**一行**结论。这一行是两个讨论者是否一致的\
唯一机械依据：必须是一行、要有信息量，不要写成整段，也不要在它之后再写任何内容。\n\
3. 如果这一轮你能看到另一位讨论者的作答，说明你们上一轮的结论冲突了：只针对分歧回应，\
不要复述对方的全文，并照常以一行结论收尾。\n\
4. 不要为了达成一致而改变判断，也不要替用户做最终决定或输出汇总——那由合成器负责。",
        name = name,
        marker = protocol::CONCLUSION_MARKER,
    )
}

/// A persona's **soul**, framed as the character this debater answers as.
///
/// The user writes the description; the framing is a constant so the two sides agree on
/// what they are being told. It is recorded **on the stream** (a `ContextInjected`
/// attributed to this debater) rather than folded into the private identity, because the
/// identity has to stay recomputable from the stream (spec §15) — and because a soul is
/// user-authored text, exactly like the project rules, which already travel that way.
pub fn persona_brief(soul: &str) -> String {
    format!(
        "你的性格设定（用户给的，整场讨论都照它来）：\n{}\n\
         保持这个性格，不要为了和对方一致而丢掉它，也不要替用户做最终决定。",
        soul.trim()
    )
}

/// The synthesizer's private identity: draw the option space, never converge.
pub fn synthesizer_identity() -> String {
    "你是本次讨论的合成器。这是一次独立的单发调用：你不参与讨论、没有工具、不发表新观点。\n\
你的产出是把已有的回答整理成选项空间，供用户自己决定：\n\
1. 共识：双方都成立的部分，并写明各自的依据。\n\
2. 分歧：冲突点，以及**各自在什么前提下成立**。\n\
3. 未决：现有回答不足以判断的部分。\n\
不要收敛成一个答案，也不要放弃聚合——标出分叉点本身就是聚合。\n\
若有讨论者缺席，必须明确写出「该方缺席，只有一方作答」，绝不能把单方回答写成共识。\n\
只依据给出的作答正文，不要猜测任何未写出的推理过程。"
        .to_owned()
}

/// The synthesizer's one user message: the question, then every round's revealed
/// answers and every absence.
///
/// Derived from the stream rather than from loop state, so "who was absent" is the
/// same fact the log records. It reveals each debater's **speech** and never its
/// private reasoning (spec §15): a summary of the reasoning would be the harness
/// rewriting the argument for one side.
///
/// `since_round` scopes the materials to **this** discussion. One session can carry
/// more than one discussion (`/discuss` runs on the live stream), and rounds are
/// numbered after whatever the stream already holds — so without the bound, the
/// second discussion's synthesizer would be handed the first one's answers and asked
/// to synthesize both.
pub fn synthesis_prompt(question: &str, events: &[Event], since_round: u32) -> String {
    let mut prompt = String::from("问题：\n");
    prompt.push_str(question.trim());
    prompt.push('\n');

    for (round, mode) in debate_rounds(events)
        .into_iter()
        .filter(|(round, _)| *round >= since_round)
    {
        let attendance = round_attendance(events, round);
        prompt.push_str(&format!(
            "\n## 第 {round} 轮（{}）\n",
            crate::render::wording::round_mode(mode)
        ));
        for (speaker, answer) in &attendance.answers {
            prompt.push_str(&format!("\n### {speaker} 的作答\n{}\n", answer.trim()));
        }
        for speaker in &attendance.absent {
            prompt.push_str(&format!("\n### {speaker}\n这一轮没有作答（本轮缺席）。\n"));
        }
    }

    prompt.push_str("\n请按「共识 / 分歧（含各自成立的前提）/ 未决」三档输出。\n");
    prompt
}

/// The highest round number the stream holds, or zero when it holds none.
///
/// A discussion numbers its rounds **after the stream it writes to**, which is what
/// keeps `round` unique inside one session: `/discuss` can run twice in a session,
/// and `RoundStarted { round }` has to say which discussion it belongs to or every
/// query over rounds (`round_attendance`, `sessions show --round N`) mixes them.
pub fn last_round(events: &[Event]) -> u32 {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::RoundStarted { round, .. } => Some(*round),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// Two distinct members of a pool, drawn from a seed, in pool order.
///
/// A pool exists so that different discussions can ask different pairs; the draw is
/// *deterministic from the seed* so that a session can be reproduced by pinning it and a
/// test can assert the choice instead of the distribution. `None` when the pool cannot
/// serve a discussion at all (fewer than two members).
///
/// In pool order, because the order decides which debater records for the discussion.
/// A cheap mix is enough: this picks which two models argue, not a cryptographic draw,
/// and the two indices can never collide because the second is drawn from the remaining
/// `len - 1` slots.
pub fn pick_pair(len: usize, seed: u64) -> Option<(usize, usize)> {
    if len < 2 {
        return None;
    }
    let len = len as u64;
    let first = seed % len;
    let mut second = (seed / len) % (len - 1);
    if second >= first {
        second += 1;
    }
    let (first, second) = (first as usize, second as usize);
    Some(if first < second {
        (first, second)
    } else {
        (second, first)
    })
}

/// The first round of the debate phase a synthesis at `synthesis_round` closes.
///
/// This is how the synthesizer's materials are scoped to **one** discussion. Rounds
/// are numbered after the stream they are written to (so a second `/discuss` in one
/// session numbers from where the first stopped), which means "every round in the log"
/// is not "this discussion's rounds" any more.
///
/// The rule is structural: a debate phase is a run of consecutive rounds whose only
/// possible `RoundEnded` is on its last round — a round that ended the debate closes
/// the phase (with or without a synthesis after it), and the next debate round on the
/// same stream opens a new one. So walking the debate rounds in order, every round
/// that ended a phase and is followed by another debate round starts a phase.
///
/// The live run and `sessions replay` both ask this question, so what the synthesizer
/// was sent and what a replay recomputes cannot drift apart.
pub fn debate_phase_start(events: &[Event], synthesis_round: u32) -> u32 {
    let mut debate: Vec<u32> = Vec::new();
    let mut ended: BTreeSet<u32> = BTreeSet::new();
    for event in events {
        match &event.payload {
            EventPayload::RoundStarted { round, mode }
                if *mode != RoundMode::Synthesis && *round < synthesis_round =>
            {
                debate.push(*round);
            }
            EventPayload::RoundEnded { round, .. } if *round < synthesis_round => {
                ended.insert(*round);
            }
            _ => {}
        }
    }
    debate.sort_unstable();
    debate.dedup();
    let mut start = debate.first().copied().unwrap_or(1);
    for pair in debate.windows(2) {
        if ended.contains(&pair[0]) {
            start = pair[1];
        }
    }
    start
}

/// A side's position in the divergence record.
///
/// Its stated conclusion when it gave one. When it broke the protocol and gave
/// none, the first line of its answer stands in: a conflict must not silently
/// drop one of its two sides from the record.
pub fn position_of(answer: &str) -> String {
    match protocol::conclusion_of(answer) {
        Some(conclusion) => conclusion.to_owned(),
        None => first_non_empty_line(answer),
    }
}

/// What a divergence is *about*: the question's first non-empty line.
pub fn divergence_topic(question: &str) -> String {
    first_non_empty_line(question)
}

/// The first non-empty line of a block of text, trimmed.
fn first_non_empty_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// The rounds the debate actually ran, in order, as `(round, mode)`.
///
/// The synthesis round is not a debate round: it is the closing call, so it never
/// appears as material to synthesize.
fn debate_rounds(events: &[Event]) -> Vec<(u32, RoundMode)> {
    let mut seen = BTreeSet::new();
    let mut rounds = Vec::new();
    for event in events {
        if let EventPayload::RoundStarted { round, mode } = &event.payload {
            if *mode != RoundMode::Synthesis && seen.insert(*round) {
                rounds.push((*round, *mode));
            }
        }
    }
    rounds
}
