# Rendering

Rendering is the **one boundary every front end goes through** (spec §19). One
renderer runs per process, chosen at startup, and it consumes the one broadcast
channel the assembly creates. There is no second event bus: incremental model
text and logged events travel together, so their relative order is defined, and
incremental text never enters the event log.

## One trait, three implementations

`src/render/` holds the boundary:

| Mode | Type | What it is for |
| --- | --- | --- |
| headless | `Headless` | the machine mode. Two explicit sinks; `stdout` carries the final product and nothing else. |
| plain | `Plain` | the human transcript for a pipe or a simple terminal: a speaker prefix on every line, section lines, indented divergence blocks, one block per tool call. |
| TUI | `Tui` | the ratatui interface: the full-screen four-pane layout on the alternate screen (header / transcript / panel / input + hints), and it owns the keyboard. See ADR 0002. |

The selection is the value type `Renderer`
(`Renderer::headless` / `::plain` / `::tui`). Because the choice is a value and
not a set of subscribers, "exactly one renderer runs" is a property of the type.
`OpenedSession::open` creates the channel and injects the consumer end into the
selected renderer (`render::channel` + `Renderer::spawn`).

`Headless` is the only mode with a purity contract: it writes to exactly two
explicit sinks, and `stdout_result` receives **only** the final product — the
completed turn of a single-agent session, or the synthesizer's `System` message
inside a discussion. An executor's turn is never the final product and never
overwrites the dispatcher's text. `cargo test --test e2e_single_turn` is the
regression assertion for that.

## One presentation layer, two painters

`render::transcript` turns events into `Block`s **once**; plain and TUI only
paint them. Two rules live there:

- **A tool call, its result and its post-hook feedback are one block.** The hook
  event carries no `tool_call_id` — the loop emits it immediately after the
  result it annotates — so the call is held open until the next unrelated event
  closes it. This is the same grouping `session::observe` performs on a finished
  stream, done incrementally for a live one. A call whose result never arrives
  (a cancel) is flushed at end of stream rather than dropped.
- **Incremental text passes straight through** as `Block::Delta`, because deltas
  bypass the log and cannot be re-derived later.

The `[speaker]` prefix is the **human's** generator, deliberately separate from
the projection's model-side prefix (spec §5): the human one repeats per line, the
model one is written once per merged block.

## Severity

`render::severity` classifies every `StopReason` into `Good` / `Note` / `Warn` /
`Bad`. The plain renderer turns that into ANSI colors and the TUI into a ratatui
`Style`; the two must never collapse `Completed` into `Aborted` or `Error`
(user story 135). `RoundsExhausted` is a `Note`; `Aborted` and `MaxIterations`
are warnings; `Error`, `MistakeLimit` and `BudgetExhausted` are errors.

## Highlighting

`render::highlight` is two layers, and they never consult each other:

- the **diff layer** (`diff_tag`) says what a line is in a patch: added,
  removed, hunk, or context;
- the **syntax layer** (`highlight_rust`) says what kind of code it is.

`highlight_diff` composes them: the diff marker is peeled off, the remaining code
is highlighted as one document (so a multi-line comment or string still parses),
and the marker is re-attached. In the TUI the syntax class is the foreground and
the diff tag the background, so an added keyword is both.

The grammar is the Rust `tree-sitter` that the repo map already depends on,
through `tree-sitter-highlight`. There is no C build step: syntect's Oniguruma
path is not taken (spec §19, Out of Scope).

## The keyboard

`render::input` is the seam between the loop and whichever front end owns the
terminal. The traffic is **request-driven**, not a stream: the loop asks for a
line only when it is ready for one, and for an answer only when the gate has
asked a question. A reader that read ahead would swallow a permission answer as
the next prompt.

- The loop holds `ConsoleHandle` (prompts and questions) and `ConsoleEvents`
  (unsolicited gestures: cancel, plan toggle, quit). They are two values because
  the loop selects on both at once.
- The front end holds `ConsolePort`. The TUI serves it from its own `select!`
  over broadcast / tick / keyboard; plain mode serves it with
  `render::spawn_plain_console`, which reads stdin line by line.
- `ConsoleAsker` implements the permission gate's `Asker` on the same handle, so
  the gate's `Ask` and the plan-mode conflict question use the one keyboard.

In the TUI, `TuiState` is the testable half: it holds the transcript, the input
line and any pending question, and `Key` is its own key vocabulary rather than
crossterm's, so the state machine is tested without a terminal.

## The interactive CLI

`fs-agent` with no subcommand starts an interactive session in the current
workspace:

- renderer: the TUI when stdout is a terminal, the plain transcript otherwise;
  `--plain` / `--tui` force one (and the two are mutually exclusive);
- `--continue` resumes this workspace's newest session, keeping its id;
- `--config`, `--model`, `--cwd` as elsewhere;
- commands: `/undo`, `/plan`, `/endplan`, `/quit`; Esc cancels the running turn
  in the TUI and Shift+Tab toggles plan mode.

The session starts in the `ask` mode, and the assembly injects the console asker,
so a write asks on the same keyboard the prompt came from.

What a real terminal has to confirm — the cursor, the mouse, resizing, quitting
clean — is written down as a follow-along list in
[`docs/tui-manual-checklist.md`](tui-manual-checklist.md). Everything a fixed-size
`TestBackend` buffer or `scripts/tui-startup-check.py` can see is asserted there
instead.
