# `bash`

`bash(command, timeout_ms?)` runs one shell command in the session workspace and
returns its exit status, stdout and stderr. Spec §7 lists it as the last of the
v1 built-ins; §12 owns the gate it passes through; §20 owns what it deliberately
does **not** do (process-level isolation). This file is the human-facing map of
the decisions and where they live in the code.

## The shape

| Piece | Value | Why |
| --- | --- | --- |
| `spec()` | `bash(command: string, timeout_ms?: integer)` | one command string, one optional cap |
| `effect()` | **always `Exclusive`** | a shell can write anything, so the dispatcher takes the workspace-wide lock and the gate sees a write |
| `command(args)` | `["bash", "-lc", command]` | the gate, the `CommandPrefix` scope and the circuit breaker read the argv **before** the process starts |
| execution | a direct `spawn` of that argv | the model's command is **one element**, so it cannot splice a second shell layer |
| result | exit status + stdout + stderr, in sections | one tool result like any other; a non-zero exit is data, not an error |

`bash -lc` gives the command the user's login environment (`-l`) and reads the
command string as one argument (`-c`). Nothing is ever concatenated into a larger
shell line, so there is no place for a model-supplied string to become shell
syntax the model did not write.

## Where the gate stands

`bash` needs no special case in `permissions.rs`:

- `readonly` denies it because that mode denies every non-`ReadOnly` call, and
  `Exclusive` is not `ReadOnly`. There is no exemption to borrow: the one write
  exemption this project ever had (the old plan mode's `PLAN.md`, a `WritePaths`
  shape whose **whole** write set is that file) went with the mode, and a shell has
  no write set anyway (see `docs/adr/0003-plan-leaves-the-permission-modes.md`).
- `ask` asks; `auto` allows, subject to the rules and the breakers as always.
- `CommandPrefix` rules match the declared argv — `["bash", "-lc", …]` — exactly
  as declared. A rule written for the command's own argv would need the shell
  unwrapped, which this scope does not do.

### The `rm` circuit breaker sees through the wrapper

Spec §12 puts one hard link outside both the modes and the rules: `rm` against
`/`, `~`, or an ancestor of either is `Deny`, whatever any allow or hook says. A
`rm` hidden behind `bash -lc "…"` would be one word in, so the breaker looks at
what the shell will run, not at the wrapper (spec §7):

```
["bash", "-lc", "rm -rf /"]            → the command is tokenized and refused
["bash", "-lc", "cd /tmp && rm -rf /"] → split on `&&`, then refused
["bash", "-lc", "(rm -rf /)"]          → the wrapping parens are separators
["bash", "-lc", "if x; then rm -rf /"] → the leading `then` is grammar, dropped
["bash", "-o", "pipefail", "-c", "rm -rf /"] → the option's argument is skipped
["bash", "-lc", "echo rm -rf /"]       → `echo` is the command; ordinary work
```

**The scan is lexical and best-effort, on purpose.** Spec §12 is explicit that
the breaker is there to stop an accident, not to confine an adversary, and §20
says v1 ships no process-level sandbox. It handles quoting, the shell's control
operators, the grammatical keywords in front of a command, and a shell option
before `-c`; it does **not** follow indirection. A variable (`rm -rf $HOME`), a
wrapper program that changes what runs (`sudo rm …`, `env …, rm …`, `eval`,
`xargs`, an alias), command substitution (`$(rm …)`), a here-doc or a script
written to disk, or an obfuscated spelling is not seen. The real boundary is that
the agent has no root and its key is not the user's filesystem.

## The timeout and the process tree

A timeout that merely dropped the future would leave the shell's children
running in the background. `bash` therefore:

1. spawns the shell in **its own process group** (`Command::process_group(0)`,
   the `setsid`-equivalent — same group semantics, no separate session id), so
   its pid is its group id;
2. runs one bounded loop over three things: the shell's exit, stdout's EOF and
   stderr's EOF;
3. on expiry sends **`SIGKILL` to the whole group** (`libc::killpg`), then reaps
   the shell.

The deadline bounds the **whole call**, not just the shell's lifetime, because
those are not the same moment: a backgrounded child inherits the output pipes, so
`bash -lc "sleep 300 &"` exits at once while the pipes stay open. Waiting only on
the shell would hang the turn for the child's whole lifetime. If a process
deliberately left the group and still holds a pipe, a short grace applies after
the kill and the result reports what is known rather than hanging.

The kill lives in a `ProcessGroup` guard that fires on **drop** as well, which
covers the other way a call ends early: the loop drops an in-flight tool on a
cancel gesture (spec §6). Both paths leave the same guarantee — nothing started
by the command is still running afterwards.

| Knob | Default | Where |
| --- | --- | --- |
| default cap | 120 s | `config::DEFAULT_BASH_TIMEOUT_MS`, `SessionConfig::bash_timeout_ms` |
| hard ceiling | 600 s | `config::MAX_BASH_TIMEOUT_MS`, `SessionConfig::max_bash_timeout_ms` |

The model may ask for less via `timeout_ms`; it can never ask for more, so no one
command can hold the workspace-wide `Exclusive` lock indefinitely. A `timeout_ms`
of zero (or a non-integer) is an argument error, not a silent fallback. Both
values reach the tool through `ToolContext::bash` as a `BashLimits` pair, built
per call from `SessionConfig` — the same shape as the repo map's budget.

A timeout is a **result**, not a `ToolError`: the output reports
`timed out after <n> ms; the process group was killed`, the signal that killed the
shell, and whatever stdout/stderr had been produced. The model can see what
happened; only a failure to spawn or reap the shell itself is an error.

## It is non-interactive

- **stdin is `/dev/null`**, and **no TTY is allocated**: an interactive program
  sees EOF rather than hanging forever.
- **The environment is inherited as-is.** v1 does no environment sanitation and
  does not invent `TERM`, `NO_COLOR` or anything else (§20); a command that wants
  a different environment sets it itself. `bash -lc` also sources the user's
  login files, so the command starts from the environment a terminal would give
  it — minus the terminal.

## The result, and truncation

The result text is three named sections — `exit code: …`, `--- stdout ---`,
`--- stderr ---` — with the exit code expressed as a code or as `killed by signal
N`. A non-zero exit is not a failure of the tool: the model asked for a process
and got one.

An oversized body is **not** handled here. It goes through the same pre-stream
pipeline as every other tool result (spec §10, ticket 07): spill to
`outputs/<tool_call_id>.txt`, and the event carries a head/tail preview plus the
pointer. `bash` neither knows nor needs to know that this happens.

## What `bash` does not include

- **A process-level sandbox.** Spec §20: v1 relies on write serialization, the
  permission gate and the breakers, and the upgrade path is "Linux-only
  bubblewrap", with no abstraction built ahead of it.
- **PTY / interactive programs.** No terminal is allocated and stdin is empty by
  design.
- **Background jobs and job control.** A command may background a process inside
  its own shell, but nothing manages or reports jobs; a timeout or a cancel kills
  the whole group.

## Where the code lives

| Piece | Module |
| --- | --- |
| `BashTool`, the argv, the timeout, the process-group kill, the result format | `tools/bash.rs` |
| `BashLimits` (the two caps the tool is handed) | `tools/tool.rs` |
| The default and the ceiling | `config.rs` (`SessionConfig::bash_timeout_ms` / `max_bash_timeout_ms`) |
| The `rm` breaker and its shell unwrapping | `permissions.rs` (`rm_breaker`, `simple_commands`) |
| Spill + preview pointer | `context.rs` (`truncate_result`), applied by `agent.rs` |
