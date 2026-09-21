# Discussion

A discussion is **two heterogeneous debaters answering the same question on one
shared event stream**, plus one closing call that lays out the option space. It is
spec §15, and it is the reason this project exists: a single model's judgement has
no counterpart, so the harness shows the user where two models actually disagree
instead of converging for them.

## Who does what

The protocol is split three ways, and the split is load-bearing:

| Layer | Holds |
| --- | --- |
| `discussion` (`src/discussion.rs`, `src/discussion/protocol.rs`) | the **rules**: whether two conclusions agree, who answered in a round, whether another round is allowed, the private instructions and the synthesizer prompt. Pure, and it never touches `provider`. |
| `agent` (`run_turn`, `run_discussion`, `run_single_shot`, `agent::executor`) | the **control flow**: the round loop, the two concurrent turns, the closing call, the executor a `task` call dispatches. This layer is the only writer of the event stream — every append goes through `agent::append_event` — and the only caller of a provider (spec §3). |
| assembly (`assemble_discussion`) | the **roster**: one `SessionScaffold` opened into two debater sessions and one synthesizer session, all sharing one log, one tool table, one lock table and one permission policy. |

`discussion` decides; `agent` writes. That is why the round-boundary events are
appended by `agent::record_round_*` even though the protocol decides when they
happen.

## The round structure

**Independent first round → reveal → targeted second round → synthesis.** One
discussion is **3 provider calls** (no divergence) or **5** (divergence). The
verdict between rounds is mechanical and costs nothing:

- Each debater ends its answer with a one-line free conclusion, on its own line,
  starting with `CONCLUSION:` (`discussion::protocol::CONCLUSION_MARKER` — prompt
  and parser share the constant so they cannot drift). A decorated marker
  (`**CONCLUSION:** x`) still reads as a conclusion.
- Normalize both conclusions (decoration, punctuation, case, whitespace), then
  agree if they are equal **or one contains the other**. No judge, no
  `response_format`, no polarity labels: the two conclusion lines *are* the
  divergence record (`DivergenceRecorded { round, topic, positions }`).
- Disagreement opens exactly one targeted round, capped at
  `DEFAULT_MAX_ROUNDS = 2` (configurable through `DiscussionParts::max_rounds`).
  A third round would need no new mechanism — the `RoundStarted { mode }` slot is
  already there — but the cap is what bounds the false positives of substring
  matching.
- `RoundEnded` is emitted **only by the round that ends the debate phase**, so its
  four terminal reasons (`NoDivergence` / `Consensus` / `RoundsExhausted` /
  `BudgetExhausted`) are always terminal and the renderer can act on them
  directly. A round that opens a targeted round is closed by the next
  `RoundStarted` instead.

## Two rules that must not be broken

**The first round is independent by construction, not by timing.** Both debaters
are in flight at once, so a fast provider finishes one answer while the other is
still thinking. A debater's projection is therefore cut at its round's
`RoundStarted`: it sees everything up to that `seq` plus its own later events, and
never the other's same-round events (`agent::TurnScope::Round`). Any replay
(`sessions replay`, ticket 17) has to reproduce this window or it will not match
what was actually sent.

**The protocol instruction is a constant private identity.** It reaches the model
as the leading `system` message and never enters the event stream, which is what
keeps "a later round's `messages` is recomputable from the stream" true. It is
constant for the whole discussion — including the targeted-round rule — because
the system prompt is the head of the prefix, and rewriting it mid-discussion would
invalidate the entire prefix cache (same reasoning as pinning
`reasoning_effort`, spec §4). What round the model is in reaches it through the
projection's `[轮 N · 名字]` prefixes and the other side's revealed answer.

## Failure

Every failure is recorded and the discussion moves on; **nothing is ever re-run**.

| Situation | What happens |
| --- | --- |
| One side fails | That side is **absent** for the round; the debate phase ends with `NoDivergence` and the synthesizer still runs. The absence is a *query* over the stream (`round_attendance`): that side's own `TurnEnded { Error }` plus no `MessageCompleted` in the round. |
| Both sides fail | `RoundEnded { Error }` + `SessionError`, no closing call. |
| The synthesizer fails | `RoundEnded { Error }` + `SessionError`, empty product. |

The synthesizer's prompt is rebuilt from the stream and names every absent side
("这一轮没有作答（本轮缺席）"), because the one misread this whole path is built to
prevent is reading a single answer as the consensus. It reveals each debater's
speech in full and never its private reasoning.

## Where the synthesizer's product goes

One independent single-shot call: no turn, no tools, no seat in the rounds. Its
product lands as a `MessageCompleted` attributed to `System`, and that is what the
headless renderer puts on **stdout**. Every debater turn also ends `Completed`, so
inside a round a completed turn is narration (stderr) rather than a final product
— the round boundary is what tells the two apart.

## Deliberately not here

N > 2 debaters (it would reopen "N = 2 does not arbitrate" — assembly refuses any
roster but two), an arbiter or judge (evidence-rejected, spec §15), a second
question on one harness (round numbers are per discussion), and trimming the
synthesis prompt (the spec asks for the *full* speech; the budget gate belongs in
ticket 14).
