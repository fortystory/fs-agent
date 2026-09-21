# Plan mode

Plan mode (spec §13) is the **hard** version of "let me think first": the session
may read, and the one thing it may write is the project-root `PLAN.md`. Every
other write — including a shell, which can write anything — is denied, not
questioned. Entering and leaving is a **user gesture** and nothing else.

Spec §12 owns the mode table and the four invariants; this file is the
human-facing map of where the decisions live in the code and why the shape is
what it is.

## It is a mode preset, not a state machine

`Mode::Plan` is the fourth `Mode`, evaluated by the same pure gate
(`permissions::decide`) as `readonly`, `ask` and `auto`. There is no second
policy engine, no arm/disarm flag threaded through the loop, and no new event.

| | `readonly` | `plan` |
| --- | --- | --- |
| reads | allow | allow |
| `WritePaths` writing anywhere else | deny | deny |
| `WritePaths` whose **whole** write set is `<repo>/PLAN.md` | deny | **allow** |
| `Exclusive` (a shell) | deny | deny |
| a matching `Allow` rule ("always allow") | cannot loosen | cannot loosen |

The difference between the two modes is that one exemption, which is why the
mode needs no new machinery.

## The exemption is a conjunct inside the deny

The gate's algebra is one lattice with one merge: `deny > ask > allow`, ignoring
specificity. A narrow `allow` rule can therefore never beat a broad `deny`, so a
"PLAN.md is allowed" rule would be dead on arrival. The exemption has to live
inside the mode's own predicate instead:

```
plan:  deny  unless  (WritePaths whose write set == { <cwd>/PLAN.md })
```

That shape is also the security property. The predicate compares the **whole**
write set, so a call that writes `PLAN.md` *and* something else cannot borrow the
exemption; `Exclusive` has no write set at all, so it can never qualify; and
`PLAN.md` is always `<cwd>/PLAN.md`, so a file of the same name deeper in the
tree is not it. `is_plan_write` in `permissions.rs` is the whole rule.

`Deny` is a **floor** for every call the exemption does not cover, so no rule and
no hook can lower it — the circuit breaker's short-circuit still runs first, and
the `.env` family is still denied.

## The gesture: the only way in and out

There is deliberately **no `exit_plan_mode` tool**. A tool would hand "may I
write?" back to the model, which is exactly the decision this mode exists to take
away from it. The frontier instead:

| Gesture | Library call | Effect |
| --- | --- | --- |
| `/plan` · Shift+Tab | `Harness::enter_plan_mode()` | swap the mode, inject one instruction, ask about an existing `PLAN.md` |
| `/endplan` · Shift+Tab | `Harness::exit_plan_mode()` | restore the mode the session had before it entered, and retire the instruction |

The keybindings belong to a renderer (ticket 18); the library exposes the two
calls and reads no keyboard itself. A session **assembled** in plan mode has no
destination to return to, so it stays in it: where such a session should land,
and the config key that could put it there, belong to the front end's
mode-selection surface (ticket 18).

The mode is a **`Session` value** (`Policy::set_mode`), never an event:
`--continue` assembles a fresh harness from `config.toml`, so a resumed session
starts in the configured mode rather than the one the killed process was in. The
audit trail is `PermissionDecided`, whose `reason` names plan mode.

## An existing `PLAN.md`

`/plan` on a directory that already has a plan asks first (through the injected
`Asker`, the same port the permission gate uses):

| Answer | File effect | What the instruction says |
| --- | --- | --- |
| **overwrite** | the CLI clears the file | write the plan |
| **append** | none | read it, then extend it |
| **keep** | none | read it, and treat it as the plan in force |

Only *overwrite* touches the disk, and the **CLI** does the clearing: the model
cannot, because read-before-write refuses to clobber a file it has not read, and
the tool never deletes a file the user owns. *Append* and *keep* are the same
file on disk, so the injected instruction is what tells them apart.

With **no answerer** (a scripted or headless caller) the session picks `keep`:
a gesture cannot happen without a front end, but a call can, and it must never
clear a file the user wrote on their own behalf.

## The instruction, and why it is pinned

Entering records one pinned `ContextInjected { source: PlanMode }`
(`context::plan_mode_instruction`). It is short, it points at the file, and it
offers no way out. Four things keep it useful:

- **Pinned wherever it sits.** `Message::User` carries an `injected` marker that
  never reaches the wire. `context::trim` pins such a message by kind rather than
  counting a leading run, so a mid-session injection survives the dropping of the
  rounds around it — and, just as important, does not count as a round boundary
  that would split a round in two.
- **Its own message.** It was injected after history existed, so it stays a
  separate `user` message instead of merging into the leading block (which is
  what the `AGENTS.md` and skills-catalog injections do).
- **Injected once.** Re-entering plan mode is a no-op; nothing re-injects after a
  compaction, because the pinned message is already in the window.
- **Retired when the mode ends.** The instruction describes a state, so leaving
  plan mode takes it back out of the window — by appending
  `HistorySuperseded { reason: ModeChange }` over it, the same mechanism `/undo`
  uses for an exchange, since history is never rewritten (spec §2). The record
  stays in the log; the projection stops replaying it. A `--continue` whose
  configured mode is not `plan` does the same on startup, or a killed process
  would keep telling the model it may not write while the gate lets it through.
  The one cost is a prefix-cache miss at the switch, which is the gesture's to
  pay.

## It survives delegation

Dispatching is a **read** (`task`'s effect is `ReadOnly`), so `task` itself is
allowed in plan mode — and the executor it spawns runs under the **same mode**,
because an executor's policy is built as "the dispatcher's mode plus the rules
that propagate" (see `docs/executor.md`). A delegation is therefore not a way
around the mode: an executor that tries to write anything but `PLAN.md` is
refused by its own gate, and the refusal is attributed to the executor.

## What plan mode does *not* include

- **A `config.toml` key for the mode**, and the keybindings themselves. Both land
  with the interactive front end (ticket 18).

The `bash` tool is **denied** in plan mode, and that is the real tool: ticket 20
landed it, its `effect()` is `Exclusive`, and the e2e drives it rather than a
stand-in. A shell has no write set, so it can never borrow the `PLAN.md`
exemption; see `docs/bash.md`.

## Where the code lives

| Piece | Module |
| --- | --- |
| `Mode::Plan`, `PlanConflict`, the exemption predicate, the floor | `permissions.rs` |
| `Asker::ask_plan_conflict` — the one interactive port, two questions | `permissions.rs` |
| `Harness::enter_plan_mode` / `exit_plan_mode` / `mode` | `lib.rs` |
| Retiring the instruction, and the resume repair | `agent.rs` (`retire_plan_instructions`), `lib.rs` (`OpenedSession::start`) |
| `HistoryReason::ModeChange` | `events.rs` |
| The instruction, phrased per conflict answer | `context.rs` |
| `Session::mode` / `set_mode` — the value, never an event | `session/mod.rs` |
| The `injected` marker and what it pins | `provider/mod.rs` (`Message`), `provider/projection.rs`, `context.rs` (`trim`) |
