# Discussion

A discussion is **two debaters answering the same question on one shared event
stream**, plus one closing call that lays out the option space. It is spec §15, and
it is the reason this project exists: a single model's judgement has no counterpart,
so the harness shows the user where two models actually disagree instead of
converging for them. The two are *meant* to be heterogeneous — two vendors, so the
second judgement is independent rather than a second sample — but the protocol
itself only requires two **identities** (see [Running one](#running-one)).

## Who does what

The protocol is split three ways, and the split is load-bearing:

| Layer | Holds |
| --- | --- |
| `discussion` (`src/discussion.rs`, `src/discussion/protocol.rs`) | the **rules**: whether two conclusions agree, who answered in a round, whether another round is allowed, the private instructions and the synthesizer prompt. Pure, and it never touches `provider`. |
| `agent` (`run_turn`, `run_discussion`, `run_single_shot`, `agent::executor`, `agent::cancel`) | the **control flow**: the round loop, the two concurrent turns, the closing call, the executor a `task` call dispatches, and the one gesture that stops it early. This layer is the only writer of the event stream — every append goes through `agent::append_event` — and the only caller of a provider (spec §3). |
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
  terminal reasons (`NoDivergence` / `Consensus` / `RoundsExhausted` /
  `BudgetExhausted`, plus `Aborted` when a cancel gesture stops the round) are
  always terminal and the renderer can act on them directly. A round that opens a
  targeted round is closed by the next `RoundStarted` instead.

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
| A cancel gesture | The round ends `RoundEnded { Aborted }` — **not** `Error`, and no `SessionError`, because the user stopped it rather than the discussion failing. The debate phase closes without opening the synthesizer, and every started `tool_call` (a running executor's `task` included) still gets its one result (spec §6). |

The synthesizer's prompt is rebuilt from the stream and names every absent side
("这一轮没有作答（本轮缺席）"), because the one misread this whole path is built to
prevent is reading a single answer as the consensus. It reveals each debater's
speech in full and never its private reasoning.

## Running one

`fs-agent discuss "问题"` is the front end: configuration picks the roster, the
subcommand assembles it, and the transcript is the interface.

```toml
[discussion]
debaters = ["kimi-k3", "deepseek-v4-pro"]   # the pool, at least two (short form)
# max_rounds = 2                            # 1 independent + at most 1 targeted

[[discussion.debaters]]                     # or named personas
name = "张三"
model = "deepseek-v4-pro"
soul = "法外狂徒，思路不受限制"

[[discussion.debaters]]
name = "李四"
model = "deepseek-flash"
soul = "守法好公民，先找依据"
```

```sh
fs-agent discuss "问题"                      # draws two of the pool at random
fs-agent discuss --debaters 保守,激进 "问题"   # or names the two
```

- **A pool, not a pair.** `[discussion] debaters` is the set a discussion draws from,
  because who debates is a configuration fact while *which* two is a decision about one
  run — and because a debater is the one participant routing may never hand to another
  model (spec §17 — `[routing]` has `synthesizer_model` and `executor_model`, and
  deliberately no debater key). Resolution refuses a pool that cannot serve a
  discussion: fewer than two members, a model that is not configured, and a name that
  cannot be an identity.
- **A debater is a persona: a `name` and a `model`.** The name is its **identity** on
  the stream (everything downstream is a function of `speaker_id`, spec §5), what the
  transcript labels it, what its own private identity calls it, and what the
  model-visible `[轮 N · 名字]` prefix writes. It can be any single word, Chinese
  included: the projection writes the name verbatim into the prefix and sanitizes only
  the `name` *field*, because the body is what carries attribution and the field's
  charset is undocumented.
  The shorthand `debaters = ["kimi-k3", …]` names a debater after its model, which is
  why a pool that lists **one model twice must give them names** — one model id cannot
  be two identities, and the error says exactly what to write. There is no automatic
  suffix.
- **A persona may carry a `soul`: its character, in the user's own words.** It is the
  one thing about a debater that cannot be derived from its name, so it is **recorded on
  the stream** — a `ContextInjected { source: Persona(name) }` attributed to that
  debater, framed by `discussion::persona_brief` — and the projection hands it to that
  side and to **nobody else**: the other debater is arguing against this character, and
  the synthesizer reads answers, not characters. Two consequences worth naming:
  * it stays **recomputable**: `sessions replay` rebuilds the call from the stream, which
    is why the soul does not live in the private identity (that has to stay a pure
    function of the name, spec §15);
  * it is **pinned**: injections are never trimmed (spec §10), so a soul is capped at
    `MAX_DEBATER_SOUL` characters rather than allowed to eat the window.
- **Which two is a draw.** `discussion::pick_pair(len, seed)` picks two distinct pool
  members, in pool order, deterministically from a seed (the clock at run time, a
  constant in tests). `--debaters a,b` names the pair instead.
- **Heterogeneity is the design's case, not a requirement.** Two debaters from one
  vendor — even the same model twice — are allowed, because one expired subscription
  must not make the discussion unrunnable, and two calls to one model still disagree
  when the sampling does. What is lost is said out loud (one advisory line per
  discussion, from `Config::debaters_share_a_vendor` on the pair that actually
  debates).
- **The synthesizer** is the routing table's other landing point
  (`[routing].synthesizer_model`); with nothing routed it is the first debater's
  model. One value, one spelling.
- **The question** comes from the command line (`fs-agent discuss "…"`), or from
  stdin: a terminal is prompted for one line, a pipe is read to the end
  (`echo 问题 | fs-agent discuss`).
- **Watching it**: a terminal gets the TUI (the permission modal included, for
  whatever a debater wants to run), a pipe gets the plain transcript — round section
  lines and both debaters on stderr, the synthesizer's product alone on **stdout**.
  The TUI's transcript does not survive the process (ADR 0002), so the run ends by
  printing the session id: `fs-agent sessions show <id>` is the durable record, and
  `sessions stats <id>` breaks the spend down per debater.
- **Exit code**: 0 for a discussion that ran (a cancel included — that is what the
  user asked for), non-zero when the debate phase itself failed.

The library seam underneath is unchanged: `assemble_discussion(DiscussionParts)` then
`harness.discuss(question)`. One harness is one discussion, so a second question
means a second assembly — which is exactly what the subcommand does per invocation.

## In a session (`/discuss`)

`/discuss [--debaters a,b] [问题]` runs a discussion **on the stream of the session you
are already in**.
The debaters and the synthesizer are *siblings* of that session: `Session::fork` gives
them its log, tool table, write locks, permission policy, answerer, skills and renderer,
and only their own model and private identity differ (`Harness::discuss`). Two
consequences are the point:

- **The discussion inherits the session's context.** The projection turns the acting
  speaker's own turns into `assistant` and *everyone else's* into `user` (spec §5), so
  the session's question and the answers it already gave reach the debaters as the
  conversation so far. `/discuss` therefore asks "what do two models make of what we are
  doing", not "answer this in a vacuum".
- **It appends to the same stream.** A session can carry a turn, a discussion, another
  turn, another discussion — and `sessions show` reads them all back in order. The session
  is still usable afterwards; the discussion is one more thing that happened in it.

Consequences that follow from sharing a stream, and how they are handled:

| Sharing one stream means | Handled by |
| --- | --- |
| Round numbers must say *which* discussion they belong to | A discussion numbers after the stream (`discussion::last_round`), so a second `/discuss` in one session numbers 4, 5, 6 — unique inside the session, which is what `round_attendance` and `sessions show --round N` key on |
| The synthesizer must not be handed an earlier discussion's answers | `discussion::debate_phase_start` scopes its materials to the debate phase the synthesis closes; `sessions replay` asks the same function, so what was sent and what is recomputed cannot drift |
| The token allowance is already being spent | `total_usage` reads the **whole** stream, which is right: the allowance is a session-level fact (spec §17) |
| The permission gate is the session's | The debaters share its policy and asker, so a debater's tool call raises the same question the session would |
| A writer is needed for round boundaries | One log, one mutex, one `seq`: the siblings append through the same `EventLog` the session uses — the same "single writer" the two debaters of a fresh discussion already have |

A bare `/discuss` takes its question from the session: the last `MessageCompleted` with
the user role (`Harness::last_question`). With nothing asked yet there is nothing to
discuss, and the loop says so rather than inventing a question.

`fs-agent discuss "问题"` is still the other way in: a discussion with a stream **of its
own** — a fresh session, no inherited context, its own id to `sessions show` later. The
difference between the two is exactly `fork`: `assemble_discussion` opens the sessions
from the scaffold it just created, `Harness::discuss` forks the live one.

## Where the synthesizer's product goes

One independent single-shot call: no turn, no tools, no seat in the rounds. Its
product lands as a `MessageCompleted` attributed to `System`, and that is what the
headless renderer puts on **stdout**. Every debater turn also ends `Completed`, so
inside a round a completed turn is narration (stderr) rather than a final product
— the round boundary is what tells the two apart.

## Deliberately not here

N > 2 debaters (it would reopen "N = 2 does not arbitrate" — assembly refuses any
roster but two), an arbiter or judge (evidence-rejected, spec §15), a second
question on one *harness* (one harness is one discussion; asking twice means building
another — which is what `/discuss` does, on the same stream), and trimming the
synthesis prompt (the spec asks for the *full* speech; the budget gate belongs in
ticket 14).
