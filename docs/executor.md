# Executor

An executor (CONTEXT.md: 执行者) is a **nested session** a debater dispatches
through the built-in `task(brief)` tool to do real work. It has its own context,
its own turn budget and the same workspace, and it reports back **a summary plus
metadata** — never its process. Spec §16 owns the decisions; this file is the
human-facing map of where they live in the code and why the shape is what it is.

## The flow of one `task` call

```
debater ── task(brief) ──► hook.pre ──► permission gate ──► deferred
                                                             │
                        ┌────────────────────────────────────┘
                        ▼
   ExecutorSpawned{executor_id, parent, brief}        ← the executor's own event
   TurnStarted · … the executor's own turns … · TurnEnded
   ExecutorFinished{executor_id, reason, summary}
                        │
                        ▼
   the `task` call's one ToolCallCompleted: summary + files changed + tokens
```

Everything lands on the **one** session stream, so the log stays the single
source of truth, `seq` stays the line number, and an executor's edits sit in the
same session directory as the dispatcher's — which is why `/undo` covers them.

`effect()` for `task` is `ReadOnly`, because `effect()` classifies **workspace**
side effects and dispatching touches no workspace path. That is what lets a batch
of `task` calls run at once; the write exclusion that matters happens inside the
executors, on the shared `PathLocks`.

## Visibility: two directions, deliberately different

| Question | Answer | Why |
| --- | --- | --- |
| Does an executor's process enter a debater's window? | **No** — not its messages, not its tool results, not `ExecutorFinished` | The dispatcher reports in its own words; the discussion carries speech, not a shared tool history (and only the summary is affordable) |
| Does the dispatching debater get the summary? | **Yes**, as the `task` call's result | Ordinary tool call, ordinary result: no second delivery mechanism |
| Does the other debater get it? | Only through the dispatcher's speech | One rule for the dispatcher and the others, or the discussion is asymmetric |
| What is in the executor's own window? | The pinned injections plus **its own** events | It works from its brief, not from a replay of the debate |
| How does the brief get there? | `ExecutorSpawned` projects into the target executor's first speech message, named by the dispatcher | So `messages = project(stream + rules)` holds in a nested session, and the brief is not mistaken for the pinned head |

An executor's *own* tool round-trip is projected in full (its results, its
reasoning, merged by `seq`); the "another speaker's tool calls survive as one
line" rule is what applies to everyone else.

## Its own budget, and what it shares

| Value | Own or shared | Where |
| --- | --- | --- |
| Turn cap | **Own**, default **25**, set by `[turn] executor_max_iterations` | `SessionConfig` |
| Model | **Inherited** from the dispatcher; `executor_model` routes it elsewhere on the *same* client | `SessionConfig` |
| Token spend | **Shared**: counted in the session total | `UsageRecorded` on the one stream |
| Read set | **Own, empty**: neither direction flows | read-before-edit is per agent's picture |
| Todo list | **Own**: an executor plans with `todo` like anyone else, and its list is its own | the list lives in that call's arguments |
| Event log · path locks · outputs dir · skill library · ask port · hook | **Shared handles** | one session, several agents |

The read set not travelling is what forces an executor to read before it writes —
"the parent read it" is not a licence, because the guardrail is about *this*
agent's picture of the file. The todo list not travelling is the same kind of
statement: an executor's list is its own record of its own steps, visible in the
transcript (its `todo` call is an ordinary line) and **not** on the sidebar — that
page shows the main session's list (`tools::todo::read_items`, and the sidebar's
`TodoPanel`, which ignores an executor's calls outright).

## Permissions: a subset of the dispatcher's

An executor runs under its dispatcher's **mode** (a mode is the session's stance
on writes: an `auto` dispatcher's executor may write, a `readonly` one's may not)
plus every rule the dispatcher marked as propagating — `Deny` and `Ask` travel by
default, `Allow` never does — and it starts with an **empty read set**. Its
authority is therefore a subset of the dispatcher's: a delegation can never be
more permissive, can never borrow an allowance the dispatcher earned, and can
never be the way around a hard stance.

`ask` behaves the same for both: with no interactive answerer (a headless run) the
loop downgrades it to `Deny`, and in an interactive session the executor asks
through the *same* port as the dispatcher.

## Depth one, enforced by the tool table

An executor cannot dispatch an executor. The first line of defense is that its
table has no `task` at all: `Tool::delegable()` (`false` for `task`) and
`Registry::for_executor()` hand a nested session the same tools minus the
non-delegable ones. A *rule* would be a weaker second line — the tool would still
be advertised to the model, and a rule bug would be a recursion bug.

## Concurrency

One batch may hold several `task` calls, and they run together, capped by
`max_parallel_executors` (default **5** — a cost and rate gate, not a safety
gate). The loop authorizes each call in batch order and *defers* the authorized
`task` calls, then runs the deferred ones with bounded concurrency and records
their results in batch order. Each is still an ordinary tool call with exactly
one result; if a hook stops the turn after a `task` was deferred, that call still
gets its one (error) result, because it was already started on the stream.

## Failure

`ExecutorFinished{reason}` carries one of the loop's single-loop values. Anything
but `Completed` becomes an **error-content tool result** for the `task` call. The
`files changed:` line is derived from the executor's successful results — the path
each write reported, not the arguments the model sent, since a `hook.pre` may have
rewritten them:

```
executor <id> finished: Error
files changed: none
tokens: input 0, output 0, cached 0, miss 0
report:
…
```

A failed executor does not interrupt the discussion: the dispatcher reads the
result, and can re-delegate, split the work, or do it itself.

## Where the code lives

| Piece | Module |
| --- | --- |
| `task(brief)` — a thin shell over the port | `tools::task` |
| The port's shape (`ExecutorSpawner`) | `tools::tool` |
| The executor's table | `tools::registry` (`for_executor`) |
| Sending it, running it, reporting back | `agent::executor` (the port) + `agent` (`run_deferred`) |
| Stopping it when the user cancels | `agent::cancel` (the observer the port clones), `agent::executor` (winds down, `ExecutorFinished { Aborted }`) |
| The brief as a message | `provider::projection` |
| An executor turn is never the session's product | `render` |
