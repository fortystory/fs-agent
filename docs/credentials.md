# Credentials, redaction, and the boundaries around them

The agent's own API key sits in `~/.config/fs-agent/config.toml` on the same
machine, and the agent can read files, write files and run commands. Spec §20
owns this surface; this file is the human-facing map of the decisions and where
they live in the code.

## The four exposure paths

| Path | What stops it |
| --- | --- |
| (a) **file-tool read** | the `.env` family is denied by policy (`permissions::env_family`) **and** every model-supplied path is confined to the session cwd (`tools::paths::SessionPaths`) — the key's real home is outside the workspace, so it is refused before a tool runs |
| (b) **command echo** | redaction happens **before the event is appended**, so `cat config.toml`'s output is scrubbed on its way into the stream (and into the file, the renderer and the projection at once) |
| (c) **cross agent** | projection already drops another speaker's tool-result bodies and reasoning; what is left — a speaker repeating a key in their own words — is covered because redaction includes message bodies |
| (d) **egress** | **honestly: nothing.** A `curl` with a key in it is indistinguishable from legitimate work, and v1 ships no process sandbox. The real boundary is not giving the agent a key you cannot afford to lose |

## The pipeline: redact → truncate → spill

Spec §10 fixes the order of the pre-stream pipeline, and §20 puts redaction at
its head. Both run in `agent::emit_completed` for a tool result, and the second
half of the guarantee — *every* event — runs in `agent::append_event`, the one
write path to the stream:

```
tool runs on the TRUE value
  └─ redact the result text            agent::emit_completed
       └─ truncate over the cap        context::truncate_result
            └─ spill to outputs/<id>.txt   (redacted bytes)
  └─ redact every free-text field      EventPayload::redact
       └─ append + render              agent::append_event  ← the only writer
```

The consequence, stated the other way round: **the text on the stream equals the
text the model saw.** The projection replays what is in the log, so a value that
was scrubbed on the way in is also what the next request carries — a run cannot
"remember" a key the stream no longer has.

| Artifact | Redacted? | Why |
| --- | --- | --- |
| the JSONL stream (and the renderer) | yes | it is what the model replays |
| tool calls' `args`, permission requests | yes, walked as JSON | a key pasted into a `write_file` argument is the same leak as one pasted into prose |
| `outputs/<tool_call_id>.txt` (spill) | yes | it is written from the already-redacted text |
| `outputs/<tool_call_id>.before` | **no** | it is `/undo`'s byte-level restore source and holds the user's own workspace content, not model output (§11) |
| `TurnOutcome.text` / `DiscussionOutcome.synthesis` | yes | the same value the stream carries is what a front end or an executor summary gets, so no second unscrubbed copy exists |

### What the redactor is

`events::Redactor` holds the **values** to hide — in practice every resolved
provider key (`Config::redactor`, filled into `SessionConfig` by
`Config::session_config`, the one place configuration becomes injected values).
It replaces each occurrence with `[redacted]`.

- **Value-level and exact.** No pattern matching, no entropy heuristics, no
  guessing at "looks like a key".
- **Longest first.** A shorter value that is a prefix of a longer one would
  otherwise leave the longer one's tail on the stream.
- **A floor of 8 characters.** Replacing a three-character string would rewrite
  ordinary prose everywhere it appeared; every vendor key here is far longer.
- **JSON leaves too**, because a tool call's arguments are an arbitrary tree.

## What value-level redaction is not

- It does **not** know about secrets it was not configured with. Keys the user
  exports for something else, copied into the workspace, are invisible to it.
- It does **not** act on values shorter than eight characters. A configured
  "key" that short is treated as a non-secret rather than replaced everywhere it
  appears: value-level replacement of a three-letter string would rewrite
  ordinary prose and make the session unreadable, and no vendor key is that
  short. The floor is a deliberate trade-off, not a bug — but it does mean a
  pathologically short custom key is **not** scrubbed.
- It does **not** decode. Base64, hex, URL-encoding, or a key split across two
  lines passes through.
- It does **not** touch a live stream. Incremental text deltas never enter the
  event stream (spec §2) and are rendered as they arrive; a key the model types
  is visible on the terminal that produced it, and is only scrubbed where it
  would become durable — in the completed message that lands in the log.
- It does **not** follow (d): a value the model sends out over the network has
  already left.

## Root is refused, with no bypass

`cli::main` checks the effective uid before it parses a single argument:
running as root exits with a refusal (`cli::root_refusal`, a pure function of
the uid, which is what makes "there is no flag" structural rather than a
promise). Every guardrail in this project assumes the worst case stays inside
the workspace; as root a single misjudgement is system-wide.

## Prompt injection lowers the ceiling; it does not move it to zero

The permission gate is a **pure function of `(policy, tool, args)` and never
reads conversation text**. That is the whole point: no sentence in any message —
from the user, another debater, or a file the agent read — can persuade the gate
to allow something, because the gate cannot see the sentence. The blast radius
of a successful injection is exactly the radius the configured policy already
allows.

What follows, and what does not:

- **Do not** down-weight a speaker because it might be injected. A model's words
  are an input to the *next* model call; the gate is not listening.
- **Do not** make the gate read the conversation, or the property above stops
  being a property and becomes a heuristic.
- **Do** keep the identity instructions (`system`) in front of each agent, keep
  another speaker's tool results out of the projection, and keep the gate
  text-blind. Those three are the mitigation, and they are already in place.
- **Do** treat an injection as a *policy* problem: the fix is to narrow what the
  policy allows (rules, modes, the breakers), not to write a better prompt.

## Known edges

- **`/undo` after a redacted edit.** `/undo` re-reads an edit's recorded
  arguments (`ToolCallStarted.args`) and replay-verifies them against the file.
  If the replaced or inserted region itself contained a configured key, those
  arguments are `[redacted]` on the stream, the replay check cannot confirm the
  region, and undo **refuses** (`Stale` / `Ambiguous`) instead of guessing
  (`tests/credentials.rs::undo_refuses_when_the_recorded_region_held_the_secret`;
  an edit elsewhere in the same file still undoes normally). That is the safe
  direction, and it is the price of scrubbing arguments at all — without it, the
  one-line summary projection gives another speaker whatever the model put in a
  tool's arguments, and the JSONL file keeps it.
- **The spill file's mode** is `0600` inside a `0700` session directory
  (`tools::paths::write_owner_only`), so even a redacted artifact is not
  world-readable.
- **A session whose cwd is your home directory.** The cwd rule keeps
  `~/.config/fs-agent/config.toml` out of reach because it is normally outside
  the workspace; started from `$HOME` it is inside it, and neither the cwd rule
  nor the `.env` rule covers it. What still holds is the redaction: a read of the
  configuration file lands on the stream with the key already `[redacted]`.
- **The workspace limit has no rule-based exception, contrary to spec §20.**
  Spec §20 says the exception to the cwd limit is "a permission rule widens it";
  the gate actually treats an out-of-workspace target as a **deny floor that no
  rule can lower** (`permissions::decide`, pinned by
  `tests/permission_gate.rs::the_path_limit_is_a_deny_floor`), and the
  dispatcher refuses before any tool runs. The reason it was left that way is
  that the rule algebra ignores specificity (spec §12), so "any matching allow
  widens containment" would mean a broad `Tool("read_file")` allow silently
  grants reads anywhere; a safe widening would need a rule shape that does not
  exist yet. Redaction, not a rule, is what protects a key inside the workspace
  today.
- **Redaction is not a sandbox.** Spec §20 keeps the upgrade path to "Linux-only
  bubblewrap" and deliberately builds no abstraction ahead of it; `docs/bash.md`
  lists what the `rm` breaker cannot see.

## Where the code lives

| Piece | Module |
| --- | --- |
| `Redactor`, `REDACTED`, `EventPayload::redact` | `src/events.rs` |
| redaction applied before the append (the one write path) | `src/agent.rs` (`append_event`) |
| redact → truncate → spill for tool results | `src/agent.rs` (`emit_completed`) + `src/context.rs` (`truncate_result`) |
| the executor's lifecycle events go through the same path | `src/agent/executor.rs` |
| the value set: every resolved provider key | `src/config.rs` (`Config::redactor`, `SessionConfig::redactor`) |
| cwd confinement for model-supplied paths | `src/tools/paths.rs` (`SessionPaths`) |
| the `.env` family's deny | `src/permissions.rs` (`env_family`) |
| the root refusal | `src/cli.rs` (`root_refusal`, called first in `main`) |
| tests | `tests/credentials.rs` |
