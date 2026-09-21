# Dynamic tools

A dynamic tool is **a command declared in `config.toml`** (spec §14). The model
sees it like a built-in: it has a name, a description and a JSON Schema, and a
call to it produces an ordinary tool result on the event stream. What makes it
safe to add without writing Rust is what the declaration *cannot* say.

## The declaration

```toml
# ~/.config/fs-agent/config.toml

[tools.git.status]
description = "Show the working tree status as porcelain."
command = ["git", "status", "--porcelain"]
parameters = { type = "object", properties = {} }

[tools.git.log]
description = "Show recent commits for one path."
command = ["git", "log", "--oneline", "-n", "{count}", "--", "{path}"]
parameters = { type = "object", properties = { count = { type = "integer" }, path = { type = "string" } }, required = ["path"] }
timeout_ms = 10000
```

The table is `[tools.<namespace>.<tool>]`; the wire name is
**`custom__<namespace>__<tool>`** (`custom__git__log` above).

- `description` and `parameters` are the declaration the provider receives,
  **verbatim** — the JSON Schema is the wire shape, so there is no translation
  layer to drift.
- `command` is the argv template.
- `timeout_ms` is optional; default 30s, ceiling 600s.

The table is fixed **at assembly**, for the life of the process. Nothing adds or
removes a tool mid-session: the tool array is part of the cached prefix, and
changing it would throw that prefix away.

## There is no side-effect field

A dynamic tool's `effect()` is **always `Exclusive`**. The declaration syntax has
no field for an effect class, so "this one is really read-only" has nowhere to be
said — the model cannot lie about it and neither can a typo.

The cost is real and documented:

- a **genuinely read-only** dynamic tool is still serialized workspace-wide, so
  it cannot run in parallel with anything else;
- **`read-before-edit` cannot cover it**, because the tool never declares a
  `WritePaths` set to check.

The constraint that remains is the **permission gate**, which sees the call's
declaration name and its argv:

```toml
# (illustrative rule shape; rules are evaluated by the gate)
# deny everything declared in configuration
tool = "custom__*"
```

A single `Tool("custom__*")` rule is the backstop for every declared tool. The
gate also matches `CommandPrefix` against the resolved argv, the same way it does
for `bash`.

## argv substitution: whole elements, never a shell

The command is spawned **directly** — there is no shell between the harness and
the program — and a parameter replaces **one whole argv element**:

| template element | argument | resulting element |
| --- | --- | --- |
| `"{path}"` | `"src/lib.rs"` | `src/lib.rs` |
| `"{path}"` | absent or `null` | (the element is omitted) |
| `"{tags}"` | `["a", "b"]` | `["a","b"]` — one element, **not** expanded |
| `"{opts}"` | `{"k": 1}` | `{"k":1}` — one element |
| `"--path={path}"` | anything | `--path={path}` — literal, no partial splice |
| `"status"` | — | `status` |

Because the value is one element of a direct spawn, shell metacharacters in an
argument are text: `; rm -rf /`, `$(...)` and backticks are passed to the program
as written and are never re-parsed. (A declaration that *chooses* to run a shell
— `command = ["bash", "-c", ...]` — is the user asking for one; the harness does
not add a layer.)

Missing arguments are omitted rather than left empty, so a template element that
is a flag disappears cleanly when the model does not send it.

## Timeout and process tree

Every call runs under a wall-clock cap. On expiry the **process group** is
SIGKILLed, not just the direct child, so a command that backgrounded children
does not leave them behind. The same guard fires if the call is dropped (a
cancel). This is the runner `bash` uses (`src/tools/process.rs`), so the two
tools cannot drift.

A non-zero exit is a **result**, not an error: the model sees the exit code,
stdout and stderr and decides what to do. Only a failure to spawn or wait is an
error, because only then is there nothing to report.

## Validation

Declarations are validated **at startup**, so a mistake is a startup error rather
than a surprise at the model's first call:

- a namespace or tool name that is empty, uses characters outside
  `[A-Za-z0-9_-]`, or contains `__` (which would make the name ambiguous) is
  refused;
- `command` must not be empty, and its **first** element (the program) must be a
  literal, not a `{placeholder}`;
- every `{name}` placeholder must be declared under `parameters.properties`.

## Name predicate

No built-in name contains `__`, and every declared tool does. So

> this tool name contains `__` **iff** it came from configuration

is decidable from the string alone — which is what lets the gate, the renderer
and a user's rule talk about "dynamic tools" without a registry lookup.
