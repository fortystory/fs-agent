# Observability: `sessions`

`sessions` answers questions about a **finished** session from its own event
stream (spec §18). Nothing here is a new capture point: every view is a group-by
over the JSONL the session already wrote, so a diagnostic can never disagree with
the source of truth. There is deliberately **no index** — `seq` is the line
number, so a view is a sequential read plus an in-memory filter.

```
fs-agent sessions ls [--all] [--cwd PATH] [--limit N] [--json]
fs-agent sessions show <id> [--round N] [--speaker X] [--kind K] [--tool T]
                           [--only-error] [--files] [--json]
fs-agent sessions replay <id> --speaker X [--round N] [--model ID] [--json]
fs-agent sessions stats <id> [--model ID] [--json]
```

`<id>` is a session id, or the path of a session directory (a session is a
movable directory, so that is often the handiest handle). stdout carries the
result and stderr the diagnostics, whatever the verb; every view has a `--json`
form for a pipeline.

## The four verbs

- **`ls`** — one row per session (id, cwd, start, tokens, rounds, messages, end
  reason), newest first, the order `--continue` considers. `--all` scans every
  bucket; without it only the workspace's bucket is read.
- **`show`** — the round-grouped transcript. A tool call and its result are
  **one row**, and a post-hook's feedback is merged into the row it annotated
  (the hook event carries no `tool_call_id`; the loop emits it right after the
  result, so the latest result is the pairing). `--files` switches to the
  workspace-object view: every file a successful write or edit landed on, with
  the round and the speaker. Filters: `--round`, `--speaker`, `--kind`, `--tool`,
  `--only-error`. `--files` honors `--round`, `--speaker` and `--tool`; `--kind`
  and `--only-error` describe transcript rows and have no meaning for a file
  change.
- **`replay`** — recompute what one call actually sent to the provider. See
  below; this is the acceptance for "the event stream is the single source of
  truth".
- **`stats`** — the fixed metric set: per-speaker tokens / cost / hit rate, calls
  per round, one-sided absence rate, executor stop reasons, the edit ladder's
  downgrade distribution, read-before-edit refusals and read-set invalidations,
  permission decisions, hook outcomes, divergence rate and stop distribution.

The two quantities nothing else surfaces are the point of the fixed set: the
**one-sided absence rate** (a round where one side failed out must never be read
as agreement) and the **edit-ladder downgrade distribution** (a downgraded match
must never be silent).

## `replay`: what, exactly, is recomputed

`replay` reproduces **one call**, not the round's final state. A finished stream
contains the answer to the call you are reproducing, so projecting the whole
stream would include events that did not exist when the request went out. The cut
is the speaker's **last `TurnStarted`** in the round: the loop snapshots the
stream just before it appends that event, so

```
(seq < TurnStarted) ∩ TurnScope
```

is exactly the window the request saw. `TurnScope` is the same structural round
cut the live loop uses, so a recomputed targeted round really contains the first
round and really excludes the other debater's same-round events.

Consequences worth knowing:

- A multi-iteration tool turn is reproduced at its **last** call. Earlier
  iterations of the same round are not addressable in v1.
- The **synthesizer** is not a turn and not a projection: it is its private
  identity plus `synthesis_prompt`, both functions of the stream (the question is
  the last `user` message before the synthesis round). `replay --speaker system`
  reconstructs exactly that.
- An **executor** replays on its own window (its brief plus the pinned
  injections, never the dispatching session's speech) with the executor identity.
- A **debater in a discussion** needs `--round`. Without one the whole-stream
  window would put the other side's answers in — replay refuses rather than
  guessing.

The equality is structural, not a coincidence: `run_turn` and `replay` both build
their request with `agent::build_messages` (project → prepend the private
identity → trim). The identity never enters the stream, so replay derives it from
the stream's shape — debater + rounds → the protocol instruction, executor → the
executor constant, plain single-agent → none.

## Money needs a model

`UsageRecorded` carries no model: prices are keyed by model id, and the roster
lives in configuration, not on the stream. So `stats` shows cost only when the
caller names a model — `--model`, otherwise `config.default_model` — and the
human view says which model it priced at (`no cost (no [pricing.<model>] entry)`
when the table has no entry, which is not the same as free). `[routing]` is
applied, so a routed synthesizer or executor is priced at the model it really
answered with. `replay` needs the same model, because the capability table (for
example whether reasoning must round-trip) is keyed by model id too.

## Reading the derived metrics

The diagnostics that only exist as text on an existing payload are read through
the producer's own constants — `MATCH_LEVEL_PREFIX`, `WROTE_PATH_PREFIX`,
`READ_BEFORE_WRITE_PREFIX`, and the edit ladder's `EditError` rendering — never
through a literal written twice. A failed ladder match lands as
`"<path>: <error>"`, so the read-set-invalidation count matches on the suffix.
This is the one place where a drifting string could silently turn a metric into
zero, which is why the convention has a single home (spec §18).
