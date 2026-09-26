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
| TUI | `Tui` | the ratatui interface: one frame around a full-height sidebar and a main column (transcript + rail / status row / input / hints) on the alternate screen, and it owns the keyboard. The sidebar has two widths and carries the mark on the wide one (see "The shell" below). See ADR 0002. |

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

- **A tool call and its result are one block, painted by the result.** The call is
  emitted the moment `ToolCallCompleted` arrives, so the call line is on screen as
  soon as the tool finishes. The post-hook's feedback carries no `tool_call_id` — the
  loop emits it immediately after the result it annotates — so it travels as its own
  small block (`Block::ToolFeedback`) aimed at the call just painted. It used to be
  merged into the call block, which meant the call was held open until the next
  unrelated event closed it — in the TUI, the next answer's first streaming delta:
  invisible for the tool's whole run, and painted only once it was over. (It is still
  painted by the *result*, not by the start: making it appear when the call begins is a
  different shape, not this one.) A call whose result never arrives (a cancel) is
  flushed at end of stream rather than dropped.
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

## The shell

One frame around everything, a **full-height sidebar** on the left and a **main
column** on the right, and the geometry is one pure function of the terminal size
(`layout::plan`, `src/render/layout.rs`) — the painters never re-derive a ladder.

```
┌─ the frame ────────────────────────────────────────────────────────┐
│ mark / text identity │ transcript        (scrollbar + rail at its  │
│ ── tab bar ──────────┤                    right edge)               │
│ 上下文 / token / …    ├── the status row: 模型 … │ 模式 … │ 上下文 …% │
│                      ├── the input area                            │
│                      ├── the hint row                              │
└──────────────────────┴─────────────────────────────────────────────┘
```

- **The sidebar** is 40 columns from 120 up and 28 down to 80; below that it is
  hidden whole and the main column takes everything. Width decides whether it is
  there at all; its own height decides what it holds: the mark gives way first (to
  the text identity, then to nothing), then fields from the tail (缓存 → 输出 →
  输入), with the tab bar and 上下文 / token / 回合 as the floor.
- **The mark** is five rows of block shading spelling the `fs` mark, centred in the
  wide rung. The characters live in `wording::logo_lines` with every other
  human-facing phrase; the colour ramp that makes them read as glyphs lives in the
  painter (`mark_lines`), foreground only and no background, so it does not fight
  whatever theme the terminal is already running. **Nothing here moves**: the falling dash
  the mark and the text identity both grew, and the hue ring before it, were tried on a real
  terminal and turned off (`.scratch/tui-input-pulse/spec.md` §2, 票 04–08). Both are still
  in the module — `mark_lines` takes the frame it would fall on, `wording::identity_falling`
  builds the other rung's line — and both are unit-tested where they live, but
  `draw_sidebar_identity` passes `None` and `identity()`, so the left column is still. Two colour versions of this signal were tried on a real terminal and both were
  retired — 12 light/normal frames at 100 ms read as flickering (票 04) and six light hues
  at 400 ms read as abrupt — so `PULSE_PALETTE` stays in the code, off screen, with a test
  pinning that nothing wears it (票 05).
- **The tab bar** pages the sidebar: 调用量 is the session's readings, 轨迹 and 文件
  are not built yet and say so. The tabs are **clicked, never keyed** — `Tab`
  belongs to the `/` menu and `Shift+Tab` to plan mode — and on a placeholder page
  the status row's `上下文 n%` is the only reading left.
- **The rail** is the transcript's last column: one cell per turn, or per round in a
  discussion, newest at the foot, the viewport's own cell drawn bright. Its window
  follows the focus, so there is always exactly one bright cell; clicking a cell
  jumps to the question that opened that turn, top-aligned.
- **Chrome is 7 rows**: the frame's two, the main column's three rules, the status
  row and the hint row — so `转录行 = h − 7 − 输入行数`, where the input's rows are
  clamped to 3 … 10: three rows are held open before the draft needs them, and the
  transcript's last row wins where the two floors meet (at 40×10 the input takes two
  and the transcript keeps one). The hints are laid out at the **main column's**
  width, not the terminal's.
- The working directory and the clock are **not on screen at all** (they left with
  the old header); `SessionFacts.session_dir` is still injected because the detail
  overlay reads spilled tool output out of it.

## The keyboard

`render::input` is the seam between the loop and whichever front end owns the
terminal. The traffic is **request-driven**, not a stream: the loop asks for a
line only when it is ready for one, and for an answer only when the gate has
asked a question. A reader that read ahead would swallow a permission answer as
the next prompt.

- The loop holds `ConsoleHandle` (prompts and questions) and `ConsoleEvents`
  (unsolicited gestures: cancel, plan toggle, quit). They are two values because
  the loop selects on both at once.
- The front end holds `ConsolePort`. The TUI serves it from its own `select!` over
  broadcast / console port / keyboard, plus **one timer that runs always**: the pulse that
  colours the prompt's `❱` (`.scratch/tui-input-pulse/spec.md` §2b, 票 08). Nothing else is
  waiting to be *noticed* — a pending question arrives on the console port, an event arrives
  on the rendering channel, a key is a key — but a colour that walks the hue wheel is a
  function of time alone, so it needs a clock, and that clock is not gated on a run: the
  prompt is on screen while the loop waits for a line. It is an `interval` rather than a
  sleep built fresh each pass, because a sleep would be reset by every event in a burst and
  the prompt would stop breathing exactly when the session is busiest. Plain mode serves the
  port with `render::spawn_plain_console`, which reads stdin line by line.
- `ConsoleAsker` implements the permission gate's `Asker` on the same handle, so
  the gate's `Ask` and the plan-mode conflict question use the one keyboard.
- `ConsoleQuestions` implements the model-question port on that same handle, so a
  model-initiated questionnaire (`ask_user_question`) reaches the one keyboard
  too. In the TUI it takes over the bottom input area — one question at a time,
  paged, with an explicit skip — rather than the middle overlay the harness's
  questions use; plain mode answers it line by line.

In the TUI, `TuiState` is the testable half: it holds the transcript, the input
line and any pending question, and `Key` is its own key vocabulary rather than
crossterm's, so the state machine is tested without a terminal.

## Reopening a session: the history replay

`--continue` reopens this workspace's newest session, and the TUI lays its whole
event stream back into the transcript so the screen does not start empty. The
seam is a front-end control request rather than a render event: the CLI pushes
`ConsoleRequest::Replay { events }` (`ConsoleHandle::replay`) right after
assembly and before the startup banner, carrying the **assembled** snapshot
(`Harness::events()`), which already includes the synthetic results recovery
wrote for dangling tool calls. Plain mode ignores the request; it keeps no
transcript to replay into. `RenderEvent`, the transcript and the headless
renderer are untouched.

- **One apply path.** Every replayed event goes through the same `TuiState::apply`
  a live event does, so blocks, collapsed hint lines, name colours, the
  information panel, the rail and the mode are rebuilt as side effects rather than
  by a second implementation. That is also why a history row is clickable: the
  detail hit table is maintained by `apply`, not rebuilt for history.
- **Framed, with a progress line.** `replay_batch` applies one slice per loop
  pass, bounded by both 512 events and 2000 source lines, and the loop never waits
  while a replay is pending. The bottom hint row is
  temporarily replaced by `恢复历史 n/m` (narrower terminals get `恢复中 n/m`,
  then `恢复中`); `wording::history_progress_line` owns that ladder.
- **The seam.** `TuiOptions::reopened` says a replay is coming, and the TUI waits for
  that first console request before it renders anything: assembly has already emitted
  the recovery results on the render channel, and they are the same events the
  snapshot holds, so nothing may be painted until the replay that owns them is up.
  When the last slice lands, a render-layer line `── 以上为历史 ──`
  (`wording::history_divider`) is inserted — only if the history actually drew
  something — and only then are the held-back live events flushed, in arrival order.
  The banner therefore lands after the seam. A live *logged* event arriving during a
  replay is dropped rather than buffered, because the snapshot already contains it;
  only events that never enter the log (the banner, diagnostics, streaming deltas) are
  held. The divider is never logged, so the next reopen inserts a fresh one.
- **Held input.** While a replay is in flight the draft still edits, but `Enter`
  does not submit, the pointer is ignored, and the scroll keys are ignored: the
  viewport stays pinned to the transcript's end until the history is done.
  `Ctrl-C` quits (this is not a run, so there is nothing to cancel); `Ctrl-D` and
  `Esc` are inert.

The behaviour is asserted in `tests/history_replay.rs` against a fixed-size
`TestBackend`; the terminal ownership of a reopen is guarded by the
`--continue` run in `scripts/tui-startup-check.py`, and the feel of a large
session stays on the manual list (`docs/tui-manual-checklist.md`, ⑭).

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
