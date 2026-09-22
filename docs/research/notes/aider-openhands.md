# aider & OpenHands — design and features (primary-source research notes)

Research method: this file cites **only** primary sources — the projects' own docs sites and their
GitHub repos/source (including local clones that I read directly). Every substantive claim has a URL.
Claims I could not confirm from a primary source are explicitly marked **(unverified)**.

Artifacts actually inspected:

| Target | What I read | Version / commit |
|---|---|---|
| aider | `docs/` (all pages), `github.com/Aider-AI/aider` source | clone @ `5dc9490bb35f9729ef2c95d00a19ccd30c26339c` (2026-05-22) |
| OpenHands (classic, "V0") | `github.com/All-Hands-AI/OpenHands` source | clone @ tag `0.62.0` (last Python monolith release) |
| OpenHands (current, "V1") | `github.com/OpenHands/software-agent-sdk` source + `docs.openhands.dev` | clone @ `3fc7b221516485e07604e8068de2fdc2d0ef3f09` |
| OpenHands frontend | `github.com/All-Hands-AI/OpenHands` on `main` | clone @ `bb4ec4420e475dfa71cc2a7cd494d4938b15ca54` |

**Read section 2.0 first** — OpenHands was split into multiple repos, so "the OpenHands repo" is
ambiguous as of 2026 and half the checklist items now live somewhere other than
`All-Hands-AI/OpenHands`.

---

# PART 1 — aider

## 1.0 What aider fundamentally is

- aider is a **terminal AI pair-programming REPL**, **not** a tool-calling agent. There is no
  tool/function-call protocol: aider sends a system prompt + repo map + the full contents of the files in
  the chat, and the model replies with *plain text in a declared "edit format"*, which aider parses and
  writes to disk itself.
  <https://aider.chat/docs/more/edit-formats.html>
- Aider picks a default edit format per model; `--edit-format`/`--chat-mode` force one.
  <https://aider.chat/docs/more/edit-formats.html>
- Dependencies confirm the design: `litellm==1.82.3` (provider abstraction), `networkx==3.4.2`
  (repo-map graph ranking), `grep-ast==0.9.0` + `tree-sitter-language-pack==0.13.0` (symbol extraction),
  `prompt-toolkit==3.0.52` (TUI), `diskcache==5.6.3` (repo-map cache), `scipy` (pagerank),
  `posthog==7.9.12` (opt-in analytics).
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/requirements.txt>

## 1.1 Agent loop & tool-calling protocol

- Base class `Coder` in `aider/coders/base_coder.py`. `run()` loops: read user input →
  `run_one(user_message, preproc)` → `send_message()`. `send_message()` is a **generator**, which is what
  produces streaming output.
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/base_coder.py#L876-L892>
- **Multi-turn is limited to a "reflection" loop**: `run_one()` re-sends `self.reflected_message` to the
  model while it is set, bounded by `max_reflections = 3`. In current `main` this is a **class attribute,
  not a CLI flag** — there is no `--max-reflections` in `aider/args.py` (`unverified` whether it was a flag
  in older releases; the docs options reference does not list it).
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/base_coder.py#L100-L101>
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/base_coder.py#L924-L944>
- **Post-edit pipeline inside one turn** (`send_message`): `apply_updates()` → `auto_commit()` →
  `auto_lint` → run shell commands → `auto_test`. Lint errors ask *"Attempt to fix lint errors?"* and test
  errors ask *"Attempt to fix test errors?"*; on acceptance the errors become the next
  `reflected_message`, which is what drives the loop.
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/base_coder.py#L1585-L1623>
- **No parallel tool calls** — there are no tools. **Stop condition**: the model's reply produced no
  reflected message (and no further user input).
- Modes are coder classes: `code`, `ask`, `architect`, `help`, plus `context`. Switch sticky with
  `/chat-mode <mode>`, per-message with `/code` `/ask` `/architect` `/help`, at launch with
  `--chat-mode <mode>` or the `--architect` shortcut. Prompt shows the mode (`>`, `ask>`, `architect>`).
  <https://aider.chat/docs/usage/modes.html>

## 1.2 Tool set & edit strategy

There is no read/write/edit/bash/grep/glob tool set. Editing is done through **edit formats**, each backed
by a `Coder` subclass whose `edit_format` class attribute is the flag value:

| `--edit-format` | Coder class | Strategy |
|---|---|---|
| `whole` | `WholeFileCoder` | model returns the **entire updated file** |
| `diff` | `EditBlockCoder` | `<<<<<<< SEARCH` / `=======` / `>>>>>>> REPLACE` search-replace blocks |
| `diff-fenced` | `EditBlockFencedCoder` | same, but file path inside the fence (for Gemini) |
| `udiff` | `UnifiedDiffCoder` | simplified unified diff |
| `udiff-simple` | `UnifiedDiffSimpleCoder` | variant unified diff |
| `patch` | `PatchCoder` | patch-style edits |
| `editor-diff`, `editor-diff-fenced`, `editor-whole` | `EditorEditBlockCoder`, `EditorDiffFencedCoder`, `EditorWholeFileCoder` | streamlined prompts used for the **editor** model in architect mode |
| `architect`, `ask`, `context`, `help` | `ArchitectCoder`, `AskCoder`, `ContextCoder`, `HelpCoder` | non-editing / two-model modes |

- Registry: <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/__init__.py>
- Format syntax + rationale (`udiff` reduced GPT-4-Turbo "lazy coding"): <https://aider.chat/docs/more/edit-formats.html>
- `patch` coder: <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/patch_coder.py#L217>
- Designer's write-up on unified diffs: <https://aider.chat/2023/12/21/unified-diffs.html>
- "Tool-like" actions are user-invoked slash commands / model-suggested shell commands, not a protocol:
  `/run` (shell command, alias `!`), `/test`, `/lint`, `/web` (scrape page → markdown), `/git` (raw git,
  output excluded from chat).
  <https://aider.chat/docs/usage/commands.html>

## 1.3 Context management

- **Chat files are explicit**: `/add` (editable), `/read` & `/read-only` (reference only), `/drop`,
  `/ls`, `/reset` (drop all files + clear history), `/clear` (history only).
  <https://aider.chat/docs/usage/commands.html>
- **Repo map is the retrieval mechanism** — a concise map of the whole git repo with the most important
  classes/functions and their signatures, sent with every request. Built with tree-sitter; only the most
  relevant slice is sent, chosen by **graph ranking (PageRank over a file-dependency graph)** to fit the
  token budget. Class `RepoMap` in `aider/repomap.py`; `get_ranked_tags()` calls
  `nx.pagerank(G, weight="weight")`.
  <https://aider.chat/docs/repomap.html>
  <https://aider.chat/2023/10/22/repomap.html>
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/repomap.py#L365-L382>
- `--map-tokens`: default `None`, resolved by `Model.get_repo_map_tokens()` to `max_input_tokens/8`
  clamped to **1024..4096**; `0` disables the map. `--map-refresh auto|always|files|manual` (default
  `auto`), `--map-multiplier-no-files` expands the map when no files are in the chat.
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/models.py#L782-L789>
  <https://aider.chat/docs/config/options.html>
- **Chat-history compaction**: `ChatSummary` (`aider/history.py`) summarizes older messages, driven by
  `max_chat_history_tokens = clamp(max_input_tokens/16, 1024, 8192)`, using the **weak model**.
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/models.py#L339-L358>
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/base_coder.py#L510-L517>
- When the edit format changes mid-session (e.g. switching model/mode), aider **re-summarizes** the old
  messages, because the previous format's examples would otherwise poison the new prompt.
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/base_coder.py#L144-L170>
- **Token accounting UX**: `/tokens` prints a per-category breakdown — system messages, chat history,
  repository map, each editable file, each read-only file — each with its remedy
  (`/clear`, `--map-tokens`, `/drop`).
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/commands.py#L445-L504>
- **Ignore rules**: `.aiderignore` in the git root, `.gitignore` syntax; `--aiderignore <file>` for a
  different filename; `--subtree-only` ignores everything outside the current subtree.
  Default path: `aider/args.py:422-431`.
  <https://aider.chat/docs/faq.html>
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/args.py#L422-L431>
- Other context sources: file mentions detected in a message (`check_for_file_mentions`), image and URL
  handling, `/paste`, `/copy-context` (context as markdown for a web chat UI), `/editor`, `/web`.
  <https://aider.chat/docs/usage/commands.html>

## 1.4 System prompt & project rule files

- Conventions are **not** auto-discovered by filename. The documented idiom is a markdown file (canonically
  `CONVENTIONS.md`) added **read-only**: `/read CONVENTIONS.md`, `aider --read CONVENTIONS.md`, or
  permanently in `.aider.conf.yml` as `read: CONVENTIONS.md` (single value or list).
  <https://aider.chat/docs/usage/conventions.html>
- Read-only files are cached when prompt caching is on — the stated reason to prefer `--read` over `/add`.
  <https://aider.chat/docs/usage/conventions.html>
- **No `AGENTS.md` / `CLAUDE.md` / `.cursorrules` support**: grep of the source tree and docs finds no
  reference to those filenames.
  <https://github.com/Aider-AI/aider/tree/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider>
- System prompts are Python constants in `aider/coders/*_prompts.py` — code, not user-editable config.
  <https://github.com/Aider-AI/aider/tree/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders>

## 1.5 Permissions & safety

- **No sandbox and no container isolation for the agent's own edits.** aider writes directly into your
  working tree and runs your lint/test commands on the host. The published Docker images exist to run
  *aider itself* in a container with the repo bind-mounted (`-v $(pwd):/app`), which incidentally contains
  what aider executes.
  <https://aider.chat/docs/install/docker.html>
- Confirmation is prompt-based, not policy-based: `InputOutput.confirm_ask()` / `prompt_ask()`. `--yes`
  ("Always say yes to every confirmation") answers everything affirmatively. There is **no**
  dangerous-command blocklist and **no** path allowlist; grep for `shadow`/`checkpoint` in `aider/`
  returns nothing.
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/io.py#L807-L830>
  <https://aider.chat/docs/scripting.html>
- Guard rails that do exist: `--dry-run`; read-only files (`/read-only`); `--no-auto-commits`;
  `--no-dirty-commits`; `--no-git`; `--suggest-shell-commands` (default **True**) gating
  model-proposed shell commands.
  <https://aider.chat/docs/scripting.html>
- Git safety: before editing files that already have uncommitted changes aider first commits them
  ("dirty commits"), so your edits stay separable from its edits. **Pre-commit hooks are skipped by
  default** (`--git-commit-verify=False` → `--no-verify`); `--git-commit-verify` opts in.
  <https://aider.chat/docs/git.html>

## 1.6 Session persistence, resume, fork

- Two local files in the git root: `.aider.input.history` (prompt-toolkit `FileHistory`) and
  `.aider.chat.history.md` (markdown transcript). `--restore-chat-history` (default **False**) reloads the
  transcript into `done_messages` at startup; `--llm-history-file` logs the raw LLM conversation.
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/args.py#L271-L300>
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/base_coder.py#L519-L523>
- Scriptable save/replay: `/save` writes commands that reconstruct the current chat session's files;
  `/load` loads and executes commands from a file.
  <https://aider.chat/docs/usage/commands.html>
- **No fork.** No session server, no conversation IDs, no branching.

## 1.7 Plan mode / todo lists / goal tracking

- **None as first-class features.** Planning is a prompt convention: `/ask` to discuss and agree a plan,
  then `/code` with a terse "go ahead" — documented as "a more fluid version of architect mode".
  <https://aider.chat/docs/usage/modes.html>
- `/context` mode shows surrounding code context. There is no todo/task-list tool and no goal tracker.
  <https://aider.chat/docs/usage/commands.html>

## 1.8 Subagents & parallel orchestration

- **No subagents and no parallel orchestration.** Architect mode is a *sequential two-model pipeline*: the
  main model acts as architect and proposes a plain-text solution; a second "editor model" converts that
  into edits via `editor-diff`/`editor-whole`. Costs two LLM requests.
  <https://aider.chat/docs/usage/modes.html>
- Config: `--editor-model <model>`, `--editor-edit-format <format>`, `--auto-accept-architect`
  (default **True**).
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/args.py#L179-L205>
- Design write-up: <https://aider.chat/2024/09/26/architect.html>

## 1.9 Extensibility

- **No MCP support** — a case-insensitive grep for `mcp` across the source tree matches only a test
  fixture; there is no MCP client code.
  <https://github.com/Aider-AI/aider/tree/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider>
- Extension points that do exist:
  - `--lint-cmd <cmd>`, `--lint "language: cmd"` (repeatable), `--auto-lint` (default True),
    `--test-cmd`, `--auto-test`. <https://aider.chat/docs/usage/lint-test.html>
  - `--commit-prompt` overrides the commit-message prompt (default template `aider/prompts.py:5`,
    Conventional Commits). <https://aider.chat/docs/git.html>
  - `.aider.conf.yml`, searched in *home dir → git root → cwd*, later overriding earlier;
    `--config <file>` loads exactly one. Plus `AIDER_*` env vars and `.env`.
    <https://aider.chat/docs/config/aider_conf.html>
  - Python API: `from aider.coders import Coder; from aider.models import Model; Coder.create(...)`,
    `coder.run("...")`, `coder.run("/tokens")`, `InputOutput(yes=True)`. Documented as **unofficial and
    subject to breaking change**. <https://aider.chat/docs/scripting.html>
  - IDE integration via `--watch-files` and in-file comment triggers `AI`, `AI!`, `AI?`.
    <https://aider.chat/docs/usage/watch.html>
  - Voice: `/voice` command, `--voice-format wav|mp3|webm` (default `wav`), `--voice-language`
    (ISO 639-1, default `en`), `--voice-input-device`; transcription uses OpenAI `whisper-1`.
    <https://aider.chat/docs/usage/voice.html>
    <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/voice.py#L170>

## 1.10 Checkpoints & undo

- The undo mechanism **is git itself** — no shadow repo, no checkpoint store. Every time aider edits files
  it commits with a descriptive message, so `/undo` can revert.
  <https://aider.chat/docs/git.html>
- `/undo` (`Commands.raw_cmd_undo`) is deliberately conservative and refuses when: no git repo; the commit
  is the repo's first; the hash is not in this session's `aider_commit_hashes`; the commit is a merge; any
  changed file currently has uncommitted changes; a file did not exist in the previous commit; or the
  commit was already pushed to `origin/<branch>`. It restores only files touched by that commit, via
  `git checkout HEAD~1 <file>`.
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/commands.py#L553-L636>
- Related: `/diff` (changes since your last message), `/commit`, `/git <cmd>`.
  <https://aider.chat/docs/git.html>

## 1.11 Cost & token control

- Prompt caching via `--cache-prompts`; aider deliberately orders the prompt so the cacheable prefix is
  system prompt → read-only files (`--read`/`/read-only`) → repo map → editable files. Supported for
  Anthropic (Sonnet/Haiku, 5-minute default TTL) and DeepSeek.
  <https://aider.chat/docs/usage/caching.html>
- `--cache-keepalive-pings N` keeps the cache warm. **Cache statistics and costs are unavailable while
  streaming** — use `--no-stream` to get them.
  <https://aider.chat/docs/usage/caching.html>
- **Model tiering**: `--weak-model` handles commit messages and chat-history summarization (default derived
  from `--model`); `--editor-model` handles architect-mode edits.
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/args.py#L185-L205>
- Budget knobs are indirect (`--map-tokens`, internal `max_chat_history_tokens`, `/drop`, `/clear`).
  There is **no `--max-cost`/budget-cap flag**.
- "Infinite output": for models supporting prefill, aider prefills a partial assistant response and
  repeats, to continue past the output-token limit.
  <https://aider.chat/docs/more/infinite-output.html>

## 1.12 Provider abstraction & multi-model support

- Provider abstraction is **litellm**: `from aider.llm import litellm` (`aider/models.py:21`), imported
  lazily because it costs ~1.5s of startup (`aider/llm.py:16`); `litellm==1.82.3` pinned.
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/models.py#L21>
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/llm.py#L16>
- Documented providers: OpenAI, Anthropic, Gemini, GROQ, xAI, Azure, Cohere, DeepSeek, OpenRouter,
  GitHub Copilot, Vertex AI, Amazon Bedrock, **Ollama** and **LM Studio** (local), plus a generic
  **OpenAI-compatible** endpoint page.
  <https://aider.chat/docs/llms.html>
- Multi-model knobs: `--model`, `--weak-model`, `--editor-model`, `--list-models`,
  `--model-settings-file`, `--model-metadata-file`, model aliases, `--set-env`, `--api-key PROVIDER=KEY`.
  <https://aider.chat/docs/config/options.html>
  <https://aider.chat/docs/config/model-aliases.html>

## 1.13 Streaming output & TUI/UX

- `--stream` / `--no-stream`, default **True** (`AIDER_STREAM`).
  <https://aider.chat/docs/scripting.html>
- TUI is **prompt-toolkit**: emacs and vi keybindings, CONTROL-C interrupt, multi-line input via `{ ... }`
  or `{tag ... tag}` delimiters, `/multiline-mode`, up-arrow history, CONTROL-R search.
  <https://aider.chat/docs/usage/commands.html>
- Also: notifications when waiting for input, a browser GUI (`aider --browser`), and `/copy`.
  <https://aider.chat/docs/usage/notifications.html> · <https://aider.chat/docs/usage/browser.html>

## 1.14 Non-interactive / headless / CI mode

- `--message/-m "<text>"` (env `AIDER_MESSAGE`) sends one instruction, applies edits, exits;
  `--message-file/-f` reads the instruction from a file. The documented CI pattern is a shell loop over
  files.
  <https://aider.chat/docs/scripting.html>
- `--yes` (env `AIDER_YES`) answers all confirmations affirmatively; `--commit` commits pending changes
  with a generated message then exits; `--dry-run`; `--stream/--no-stream`;
  `--auto-commits/--no-auto-commits`, `--dirty-commits/--no-dirty-commits`.
  <https://aider.chat/docs/scripting.html>
- Every option has an `AIDER_*` env var, so CI can be configured without flags.
  <https://aider.chat/docs/config/options.html>

## 1.15 Evaluation & regression tests

- Harness lives in `benchmark/` and is explicitly designed to run **inside Docker**, because it executes
  LLM-written code unsupervised.
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/benchmark/README.md>
- Two leaderboards: the classic **code editing** benchmark (133 Exercism Python exercises) — now replaced
  by the harder **polyglot** leaderboard. Tracked metrics are "percent completed correctly" and "percent
  using correct edit format".
  <https://aider.chat/docs/leaderboards/edit.html> · <https://aider.chat/docs/leaderboards/notes.html>
- Published example: `o1` and `claude-3-5-sonnet-20241022` both at **84.2%** correct / **99.2%** correct
  edit format on the code-editing leaderboard.
  <https://aider.chat/docs/leaderboards/edit.html>
- **SWE-bench**: aider reported SOTA on both main SWE-bench and SWE-bench Lite in June 2024 — **18.9%** on
  main SWE-bench, and **17.0%** with GPT-4o alone (previous top entry 13.8%; Devin 13.9%), following a
  **26.3%** SWE-bench Lite result. Benchmark harness/statistics code is in the repo.
  <https://aider.chat/2024/06/02/main-swe-bench.html> · <https://aider.chat/2024/05/22/swe-bench-lite.html>
- Plotting code for those results: `benchmark/swe_bench.py`; data in `benchmark/swe-bench.txt` and
  `benchmark/swe-bench-lite.txt`; harness test `benchmark/test_benchmark.py`.
  <https://github.com/Aider-AI/aider/tree/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/benchmark>
- Unit tests: `tests/` under pytest (`pytest.ini` at repo root).
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/pytest.ini>

## 1.16 Observability

- Token/cost accounting: `/tokens` breakdown (see 1.3); per-message `message_cost` reset in
  `init_before_message()`; `/settings` prints effective settings.
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/base_coder.py#L864-L874>
- Raw LLM log via `--llm-history-file`; full markdown transcript `.aider.chat.history.md`; `/diff` shows
  what changed.
  <https://aider.chat/docs/config/options.html> · <https://aider.chat/docs/git.html>
- Analytics are **opt-in and anonymous** via PostHog (`posthog==7.9.12`, project key hard-coded in
  `aider/analytics.py`).
  <https://aider.chat/docs/more/analytics.html>
  <https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/analytics.py#L56-L57>
- **No OpenTelemetry / tracing / span support**: grep for `opentelemetry`, `otel`, `traceloop`, `langfuse`
  across the source tree and `requirements.txt` returns nothing.

---

# PART 2 — OpenHands

## 2.0 CRITICAL: the project has been restructured (read this first)

The URL in the task, `https://github.com/All-Hands-AI/OpenHands`, now serves the **TypeScript "Agent
Canvas" application**, not the Python agent. Its README is titled "Agent Canvas — The self-hosted developer
control center for coding agents and automations" and describes running "OpenHands, Claude Code, Codex,
Gemini, or any ACP-compatible agent across local, remote, and cloud backends".
<https://github.com/All-Hands-AI/OpenHands>

- A shallow clone of `main` contains mostly `src/`, `electron/`, `vite.config.ts`, `playwright.*.config.ts`
  (a React Router + Electron + Vite app) and only **10 `.py` files**, versus the classic tag `0.62.0` which
  contains the full `openhands/` Python package and an `evaluation/` harness.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/…>
- The rename is a real GitHub redirect: `https://github.com/All-Hands-AI/OpenHands` returns **HTTP 301 →
  `https://github.com/OpenHands/OpenHands`** (verified with `curl -o /dev/null -w '%{http_code} %{redirect_url}'`).
  <https://github.com/OpenHands/OpenHands>
- The former V0 code is preserved in a dedicated repo, and the evaluation harness moved out too:
  `https://github.com/OpenHands/legacy` (V0 monolith) and `https://github.com/OpenHands/benchmarks`
  (both return HTTP 200).
- Docs moved: **`https://docs.all-hands.dev/` → HTTP 308 → `https://docs.openhands.dev/`**.
- The Python agent now lives in **`https://github.com/OpenHands/software-agent-sdk`** (four packages:
  `openhands-sdk`, `openhands-tools`, `openhands-workspace`, `openhands-agent-server`) — its README says it
  "is the engine behind the OpenHands CLI and OpenHands Cloud".
  <https://github.com/OpenHands/software-agent-sdk>
- The CLI is a **separate repository**: `https://github.com/OpenHands/OpenHands-CLI` (returns HTTP 200).
  Its README states it is **no longer actively maintained**, and it pins an older SDK
  (`openhands-sdk==1.28.1`) than the SDK repo's current release (1.46.0).
  <https://github.com/OpenHands/OpenHands-CLI>
- The SDK's own design doc frames this as "OpenHands V1 … a complete architectural rework based on lessons
  from OpenHands V0", naming V0's problems: "tight coupling between research and production, mandatory
  sandboxing, mutable state, and configuration sprawl".
  <https://docs.openhands.dev/sdk/arch/design>

**Consequence for this note:** the checklist items that the task lists under "OpenHands" (CodeActAgent,
event stream, Docker runtime, microagents, `openhands` CLI headless mode, SWE-bench harness, litellm) are
**V0 concepts**. I document V0 (from tag `0.62.0`) in section 2.1 and their V1 replacements in 2.2.

## 2.1 Classic architecture (V0, tag `0.62.0`)

### Agent loop

- `CodeActAgent` (`VERSION = '2.2'`) implements the **CodeAct** idea — consolidating the agent's action
  space into *code*: "Execute any valid Linux `bash` command" and "Execute any valid `Python` code with an
  interactive Python interpreter". Docstring cites the CodeAct paper <https://arxiv.org/abs/2402.01030>.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/agenthub/codeact_agent/codeact_agent.py#L49-L76>
- `Agent.step(state) -> Action` is a **single-step** method; `AgentController._step()` drives the loop.
  Default `max_iterations = OH_MAX_ITERATIONS = 500`; a per-task `max_budget_per_task` also exists.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/config_utils.py#L8>
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/openhands_config.py#L98-L99>
- `step()` (function-calling path): condense history → build messages → `self.llm.completion(...)` with
  `params['tools']` → `response_to_actions(response)` → push onto `self.pending_actions` deque, popping one
  per call. **Important precision: V0 does NOT execute tool calls in parallel.** The response parser can
  produce several actions, but they are queued in `pending_actions` and **drained exactly one per `step()`**
  (and `function_calling.py` asserts `len(response.choices) == 1`). Parallel tool calls are therefore
  *serialized* across successive steps.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/agenthub/codeact_agent/codeact_agent.py#L183-L225>
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/agenthub/codeact_agent/function_calling.py#L76>
- Stop conditions: `AgentFinishAction`, `AgentState.FINISHED`/`REJECTED`/`STOPPED`, iteration/budget flags,
  and a **stuck detector** (`openhands/controller/stuck.py`, `_is_stuck()`, `headless_mode` changes how much
  history it inspects).
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/controller/agent_controller.py#L852-L882>
- `headless_mode` is a controller constructor argument that also changes control-flag limit increases.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/controller/agent_controller.py#L128-L159>

### Event stream architecture

- `EventStream` implements `EventStore`; components subscribe by name via the
  `EventStreamSubscriber` enum: `AGENT_CONTROLLER`, `RESOLVER`, `SERVER`, `MEMORY`, `MAIN`. Key methods
  `subscribe()`, `add_event(event, source)`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/events/stream.py#L23-L43>
- Events are `Action`/`Observation` pairs with a `source`; supporting machinery includes
  `event_filter.py`, `event_store.py` (`search_events`, `filtered_events_by_source`, `get_latest_event_id`),
  `nested_event_store.py`, and `serialization/`.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/openhands/events>

### Tool set & edit strategy (V0)

Tools are `ChatCompletionToolParam` JSON schemas, assembled in `CodeActAgent._get_tools()`:

| Tool | Config gate (default) |
|---|---|
| `create_cmd_run_tool` (bash) | `enable_cmd` (**True**) |
| `ThinkTool` | `enable_think` (**True**) |
| `FinishTool` | `enable_finish` (**True**) |
| `CondensationRequestTool` | `enable_condensation_request` (False) |
| `BrowserTool` | `enable_browsing` (**True**) |
| `IPythonTool` | `enable_jupyter` (**True**) |
| `create_task_tracker_tool` | `enable_plan_mode` (**True**) |
| `LLMBasedFileEditTool` | `enable_llm_editor` (False) |
| `create_str_replace_editor_tool` | `enable_editor` (**True**) — used when `enable_llm_editor` is False |

<https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/agenthub/codeact_agent/codeact_agent.py#L108-L153>
<https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/agent_config.py#L22-L54>

- **Edit strategy**: `str_replace_editor` (search/replace-style, ACI-based) is the default; the alternative
  `LLMBasedFileEditTool` is deprecated/off by default. Short tool descriptions are used automatically for
  `gpt-4`, `o1`, `o3`, `o4` model substrings to stay under OpenAI's tool-description limit.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/agenthub/codeact_agent/codeact_agent.py#L109-L123>

### Context management / condensation (V0)

- Ten condenser implementations: `amortized_forgetting_condenser`, `browser_output_condenser`,
  `conversation_window_condenser`, `llm_attention_condenser`, `llm_summarizing_condenser`,
  `no_op_condenser`, `observation_masking_condenser`, `pipeline`, `recent_events_condenser`,
  `structured_summary_condenser`.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/openhands/memory/condenser/impl>
- **The default is `ConversationWindowCondenserConfig`**, with an in-code rationale: using NoOp would mean
  a context-length overflow generates a condensation request that is never handled.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/agent_config.py#L54-L59>
- `ConversationWindowCondenser.get_condensation()` keeps essential initial events (SystemMessageAction,
  first user MessageAction, RecallAction/observation), keeps roughly half the history, preserves
  action–observation pairs, and returns a `CondensationAction(forgotten_event_ids=...)`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/memory/condenser/impl/conversation_window_condenser.py#L22-L45>
- Config keys across the family include `keep_first` and `max_size`; `CondenserPipelineConfig` chains a list
  of condensers.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/condenser_config.py#L43-L172>
- In `step()`, the condenser result is matched: a `View` is used for the LLM query, a `Condensation` is
  returned as an action so the controller immediately re-steps with the new view.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/agenthub/codeact_agent/codeact_agent.py#L192-L207>
- **Condensation in V0 is non-destructive**: `View.from_events()` derives a *filtered view* over the
  append-only event log — nothing is deleted from history. Benchmark runs deliberately force the NoOp
  condenser via an `EVAL_CONDENSER` setting, so eval numbers are measured without summarization.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/memory/view.py>

### Microagents (the V0 "project rules" mechanism)

- `MicroagentType` enum: `KNOWLEDGE` (keyword-triggered), `REPO` (always active), `TASK` (requires user
  input). Metadata fields: `name`, `type`, `version`, `agent`, `triggers`, `inputs`, `mcp_tools`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/microagent/types.py>
- Microagents are markdown files with front-matter, loaded from the repo; a legacy
  `.openhands_instructions` file is still supported. A microagent may declare `mcp_tools` — but in 0.62.0
  only **stdio** MCP servers are allowed from that path.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/microagent/microagent.py#L52-L133>
- Shipped microagents include `github.md`, `gitlab.md`, `bitbucket.md`, `docker.md`, `kubernetes.md`,
  `npm.md`, `ssh.md`, `security.md`, `fix_test.md`, `update_test.md`, `add_agent.md`,
  `add_repo_inst.md`, `code-review.md`, `codereview-roasted.md`, `onboarding.md`, `pdflatex.md`,
  `swift-linux.md`, `address_pr_comments.md`, `update_pr_description.md`, `agent_memory.md`,
  `default-tools.md`, `fix-py-line-too-long.md`, `flarglebargle.md`.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/microagents>
- Injection into the prompt is via `prompts/microagent_info.j2`; related prompt templates include
  `system_prompt.j2`, `system_prompt_interactive.j2`, `system_prompt_long_horizon.j2`,
  `system_prompt_tech_philosophy.j2`, `security_risk_assessment.j2`, `in_context_learning_example*.j2`.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/openhands/agenthub/codeact_agent/prompts>
- **`enable_plan_mode` is not a real plan mode.** It only (a) swaps in `system_prompt_long_horizon.j2` —
  which is literally `{% include "system_prompt.j2" %}` plus a `<TASK_MANAGEMENT>` block — and (b) registers
  the `task_tracker` tool. There is no gating and no approval step before execution.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/agenthub/codeact_agent/prompts/system_prompt_long_horizon.j2>
  `enable_plan_mode` (default **True**) does this silently via `resolved_system_prompt_filename`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/agent_config.py#L71-L79>

### Permissions & safety (V0)

- `SecurityAnalyzer` ABC with `security_risk(action) -> ActionSecurityRisk` and a registry
  `SecurityAnalyzers` in `openhands/security/options.py`; implementations under `security/llm/`,
  `security/invariant/`, `security/grayswan/`. Configured via a **TOML-only** `[security]` section:
  `security_analyzer ∈ {invariant, llm, grayswan}` — note there is **no `--security-analyzer` CLI flag**.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/security/analyzer.py>
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/security/options.py>
- When `confirmation_mode` is on, **only HIGH-risk actions** (or UNKNOWN risk with no analyzer configured)
  are set to `AWAITING_CONFIRMATION`; the controller then moves the agent to
  `AgentState.AWAITING_USER_CONFIRMATION`. Applies to `CmdRunAction`, `IPythonRunCellAction`,
  `BrowseInteractiveAction`, `FileEditAction`, `FileReadAction`. CLI mode is special-cased.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/controller/agent_controller.py#L945-L1000>
- **Sandbox**: V0 ran everything in Docker by default. `[sandbox]` TOML keys include `timeout`, `user_id`,
  `base_container_image`, `use_host_network`, `enable_auto_lint`, `initialize_plugins`,
  `runtime_extra_deps`, `runtime_container_image`, `keep_runtime_alive`, `pause_closed_runtimes`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/config.template.toml>
- Runtime implementations: `docker`, `local`, `remote`, `kubernetes`, `cli`, `action_execution`.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/openhands/runtime/impl>
- **The Docker isolation is thinner than the branding implies.** `DockerRuntime` launches the container via
  `containers.run(...)` with **no `cap_drop`, no `security_opt`, no `privileged=False`, no `mem_limit`, and
  no `user=`** — isolation rests on Docker defaults plus the mounted workspace. The runtime image runs as
  user `openhands`, but that user is in the `sudo` group with `'%sudo ALL=(ALL) NOPASSWD:ALL'` written to
  `/etc/sudoers`, i.e. effectively passwordless root inside the container.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/runtime/impl/docker/docker_runtime.py#L523>
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/containers/app/Dockerfile#L53-L68>
- `enable_history_truncation` (default True), `enable_mcp` (default True),
  `disabled_microagents` are additional `AgentConfig` gates.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/agent_config.py#L44-L52>

### Session persistence, resume (V0)

- `openhands/storage/` provides `local.py`, `files.py`, `s3.py`, `google_cloud.py`, `memory.py`,
  `web_hook.py`, `batched_web_hook.py`, `locations.py`, and `conversation/`
  (`conversation_store.py`, `file_conversation_store.py`, `conversation_validator.py`).
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/openhands/storage>
- Config: `file_store` (default `'local'`) and `file_store_path` (default `'~/.openhands'`), plus webhook
  file-store options.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/openhands_config.py#L72-L76>
- CLI: `openhands --resume <conversation-id>`, plus a `serve` subcommand with `--mount-cwd` / `--gpu`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands-cli/openhands_cli/argparsers/main_parser.py>
- **No fork** in V0 (unverified if the web app had a fork action; the SDK only gained it in V1 — see 2.2).

### Plan mode / todo (V0)

- `enable_plan_mode` (default True) enables the `task_tracker` tool, whose command enum is
  `['view', 'plan']` and whose `task_list` items require `title`, `status`, `id` with status
  `todo`/`in_progress`/`done`. Its description mandates updating status dynamically as work progresses.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/agenthub/codeact_agent/tools/task_tracker.py#L142-L194>
- Backed by the long-horizon system prompt (see above).

### Subagents & delegation (V0)

- Delegation is a first-class **action**, not a tool argument:
  `AgentDelegateAction(agent: str, inputs: dict, thought: str)`. Delegate level is tracked as
  `State.delegate_level` and nested controllers share the parent's iteration/budget flags
  (`parent_iteration=self.state.iteration_flag.current_value`).
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/events/action/agent.py#L77-L85>
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/controller/agent_controller.py#L752-L760>
- Delegate agents available in V0: `browsing_agent`, `visualbrowsing_agent`, `loc_agent`,
  `readonly_agent`, `dummy_agent`.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/openhands/agenthub>
- Delegation is **strictly sequential and lossy**: `self.delegate` is a single attribute, and
  `should_step()` returns `False` while a delegate exists, so the parent cannot act in parallel. Worse, the
  parent is **blinded to the sub-trajectory** — `StateTracker._init_history` deletes all events between
  `AgentDelegateAction` and its `AgentDelegateObservation`, and the hand-off is a raw text blob (the source
  carries `# TODO: replace this with AI-generated summary (#2395)`).
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/controller/agent_controller.py#L402-L410>

### Extensibility (V0)

- MCP: `openhands/mcp/` (`client.py`, `tool.py`, `utils.py`); `MCPConfig` supports `sse_servers`,
  `stdio_servers`, `shttp_servers`, configured from a `[mcp]` TOML section.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/mcp_config.py#L222-L233>
- Runtime plugins: `PluginRequirement` with `AgentSkillsRequirement` and `JupyterRequirement` bundled by
  `CodeActAgent.sandbox_plugins` (AgentSkills must precede Jupyter).
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/agenthub/codeact_agent/codeact_agent.py#L70-L76>
- Custom agents: drop a new module in `openhands/agenthub/` and select with `--agent-cls`.

### Checkpoints & undo (V0)

- **No session-level undo/checkpoint/revert.** The only undo is per-file: the file editor supports an
  `undo_edit` command. There is no git-backed checkpoint store.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/openhands/runtime>

### Cost & token control (V0)

- `openhands/llm/metrics.py`: `Metrics` tracks `accumulated_cost`, a list of `Cost`, per-call `TokenUsage`
  entries, and `accumulated_token_usage`; costs can be serialized via `OpenHandsJSONEncoder`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/llm/metrics.py#L47-L111>
- Budgets: `max_budget_per_task` config field and `-b/--max-budget-per-task` CLI flag; `BudgetFlag` plus
  `iteration_flag` enforce limits in the loop, with `state_tracker.sync_budget_flag_with_metrics()`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/arg_utils.py#L124-L129>
- Prompt caching: `caching_prompt` defaults to **True** when the provider supports it; cached tokens are
  tracked via litellm's `PromptTokensDetails`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/llm_config.py#L86>

### Provider abstraction (V0)

- Uses **litellm** directly: `from litellm import completion as litellm_completion`,
  `completion_cost as litellm_completion_cost`, `ModelInfo`, `PromptTokensDetails`,
  `litellm.modify_params = self.config.modify_params`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/llm/llm.py#L16-L28>
- `LLMConfig` fields and defaults: `model` (default `claude-sonnet-4-20250514`), `api_key`, `base_url`
  (documented as "necessary for local LLMs"), `num_retries` (**5**), `timeout`, `temperature` (**0.0**),
  `custom_llm_provider`, `max_input_tokens`, `max_output_tokens`, `ollama_base_url`, `drop_params`
  (**True**), `caching_prompt` (**True**).
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/llm_config.py#L53-L86>
- Also `openhands/llm/router/`, `llm_registry.py`, `bedrock.py`, `model_features.py`,
  `async_llm.py` / `streaming_llm.py`, `fn_call_converter.py`.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/openhands/llm>

### Streaming & TUI (V0)

- `streaming_llm.py` and a `stream` config; the CLI is a **prompt-toolkit** TUI
  (`openhands-cli/openhands_cli/tui/`, `pt_style.py`), with `gui_launcher.py` to launch the web UI.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/openhands-cli/openhands_cli>

### Headless / CI (V0)

- **Careful — there are TWO CLI stacks inside tag `0.62.0`.** `openhands/core/config/arg_utils.py` (below) is
  the *classic monolith's* headless parser. The `openhands-cli/` directory at this tag is a **different,
  newer package** (`openhands` v1.0.6 on PyPI) built on `openhands-sdk`/`openhands-tools` — so the
  `openhands` console script is the **V1 SDK CLI**, not the classic entrypoint. Its parser offers
  `openhands --resume <conversation-id>` and a `serve` subcommand with `--mount-cwd` / `--gpu`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands-cli/openhands_cli/argparsers/main_parser.py>
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/openhands-cli>
- Headless mode in the **classic** stack runs through `get_headless_parser()` with `--task`/`-t`, `--file`,
  `--directory`/`-d`, `--agent-cls`/`-c`, `--max-iterations`/`-i`, `--max-budget-per-task`/`-b`,
  `--no-auto-continue`, `--selected-repo`, `--config-file`. **Note there is no `--headless`, `--sandbox`,
  `--disable-network`, `--mcp-config`, `--max-budget-per-run`, or classic `--resume` flag at this tag** —
  do not attribute those to V0.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/arg_utils.py#L16-L142>
- GitHub issue resolver: `openhands/resolver/resolve_issue.py` with flags `--repo`, `--issue-number`,
  `--issue-type`, `--comment-id`, `--token`, `--username`, `--base-domain`, `--repo-instruction-file`,
  `--prompt-file`, `--output-dir`, `--llm-model`, `--llm-api-key`, `--llm-base-url`, `--runtime`,
  `--runtime-container-image`, `--base-container-image`, `--max-iterations`, `--is-experimental`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/resolver/resolve_issue.py>
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/resolver/README.md>

### Evaluation (V0)

- `evaluation/benchmarks/` covers ~28 harnesses, including `swe_bench`, `multi_swe_bench`,
  `visual_swe_bench`, `swe_perf`, `aider_bench`, `commit0`, `gaia`, `webarena`, `terminal_bench`,
  `miniwob`, `humanevalfix`, `ml_bench`, `the_agent_company`, `bird`, `gpqa`.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/evaluation/benchmarks>
- SWE-bench harness: `evaluation/benchmarks/swe_bench/run_infer.py`, `eval_infer.py`, plus
  `binary_patch_utils.py`, `live_utils.py`, `run_infer_interact.py`, `run_localize.py`, `scripts/`,
  `prompts/`, and docs `SWE-bench-Live.md`, `SWE-Gym.md`, `SWE-Interact.md`, `SWE-rebench.md`.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/evaluation/benchmarks/swe_bench>
- Launch signature:
  `./evaluation/benchmarks/swe_bench/scripts/run_infer.sh [model_config] [git-version] [agent] [eval_limit] [max_iter] [num_workers] [dataset] [dataset_split] [n_runs] [mode]`,
  where `mode` may be `swt` / `swt-ci` (pre-installs the environment and tells the model the exact test
  command). `eval_infer.sh` also accepts a JSONL in SWE-bench prediction format
  (`model_patch`/`model_name_or_path`/`instance_id`) and emits `report.json` with `resolved_ids`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/evaluation/benchmarks/swe_bench/README.md>
- **Documented V0 SWE-bench score: (unverified).** The `0.62.0` README does not state a headline score; I
  could not confirm a primary-source V0 number from this tag. The current headline claim is in 2.2.

### Observability (V0)

- The **event stream is the log and trace**. `openhands/io/json.py` provides `OpenHandsJSONEncoder`,
  serializing `Event` → `event_to_dict`, `Metrics`, `ModelResponse`, and `CmdOutputMetadata`.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/io/json.py>
- `debug: bool = False` config, plus `openhands/core/logger.py`; `_prepare_metrics_for_frontend(action)`
  logs per-action metrics for the UI.
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/core/config/openhands_config.py#L104>
  <https://github.com/All-Hands-AI/OpenHands/blob/0.62.0/openhands/controller/agent_controller.py#L999-L1003>
- The server (`openhands/server/`) exposes conversation/event REST routes; the CLI reads the same stream.
  <https://github.com/All-Hands-AI/OpenHands/tree/0.62.0/openhands/server>

## 2.2 Current architecture (V1, `OpenHands/software-agent-sdk`)

### Four-package split and two deployment modes

| Package | Role | Required? |
|---|---|---|
| `openhands.sdk` | core agent framework + `Workspace`/`LocalWorkspace` base | always |
| `openhands.tools` | pre-built tools (bash, file editing, …) | optional |
| `openhands.workspace` | extended workspaces (Docker, remote) | optional |
| `openhands.agent_server` | FastAPI/WebSocket multi-user API server | optional |

- Mode 1 (local): `pip install openhands-sdk openhands-tools`, everything in one process, no Docker.
  Mode 2 (production): install all four; `RemoteWorkspace` auto-spawns agent-server in containers.
  "Same agent code works in both modes — just swap the workspace type
  (`LocalWorkspace` → `DockerWorkspace` → `RemoteAPIWorkspace`)."
  <https://docs.openhands.dev/sdk/arch/overview>
- Agent Server exposes REST + WebSocket endpoints for conversations, bash, files, events, desktop, VSCode,
  plus an **OpenAI-compatible `/v1/chat/completions`** endpoint and API-key auth.
  <https://docs.openhands.dev/sdk/arch/overview>

### Design principles (explicit V0→V1 changes)

- **Optional isolation over mandatory sandboxing**: "Every tool call in V0 executed in a sandboxed Docker
  container by default… V1 unifies agent and tool execution within a single process by default, aligning
  with MCP's local-execution model."
- **Stateless by default, one source of truth for state**: "All components … are immutable Pydantic models
  validated at construction. The only mutable entity is the conversation state."
- **Clear boundaries between agent and applications**: SDK / tools / workspace / agent server, with
  applications talking over APIs.
- **Composable components**: "Agents are defined as graphs of interchangeable components—tools, prompts,
  LLMs, and contexts."
  <https://docs.openhands.dev/sdk/arch/design>

### Agent loop

- `Agent` (`openhands-sdk/openhands/sdk/agent/agent.py`) is the "stateless reasoning-action loop executor";
  `AgentBase` defines the interface; `AgentContext` holds skills/prompts/metadata.
  <https://docs.openhands.dev/sdk/arch/agent>
- `step()` flow: (1) execute pending confirmation-queued actions; (2) call `condenser.condense()` — a
  `View` continues in the same step, a `Condensation` is emitted and returned for the next step;
  (3) query the LLM; (4) parse the response into events (tool calls → `ActionEvent`s, text →
  `MessageEvent`); (5) if actions need approval, set status `WAITING_FOR_CONFIRMATION` and return;
  (6) execute tools → `ObservationEvent`s. Described as "Stateless … Event-Driven … Interruptible: Each
  step is atomic and can be paused/resumed."
  <https://docs.openhands.dev/sdk/arch/agent>

### Event system & parallel tool calls

- Events are an **immutable, append-only log** with Pydantic models. LLM-convertible types:
  `MessageEvent` (user/agent), `ActionEvent`, `ObservationEvent`, `UserRejectObservation`,
  `AgentErrorEvent`, `SystemPromptEvent`, `CondensationSummaryEvent`. Internal types:
  `ConversationStateUpdateEvent`, `CondensationRequest`, `Condensation`, `PauseEvent`. Source is
  `user`/`agent`/`environment` and is deliberately independent of the LLM `role`.
  <https://docs.openhands.dev/sdk/arch/events>
- **Parallel function calling is explicitly supported**: when multiple `ActionEvent`s share the same
  `llm_response_id`, they are grouped and combined into a single assistant `Message` with multiple
  `tool_calls`; only the first event's `thought`/`reasoning_content`/`thinking_blocks` are kept.
  <https://docs.openhands.dev/sdk/arch/events>
- Two distinct error paths: `AgentErrorEvent` (LLM-convertible, tool-scoped, conversation continues) vs
  `ConversationErrorEvent` (not LLM-convertible, run loop → ERROR, `run()` raises `ConversationRunError`).
  <https://docs.openhands.dev/sdk/arch/events>
- `Conversation`/`LocalConversation`/`RemoteConversation` + `ConversationState` + `EventLog`
  ("immutable append-only store with efficient queries"); the factory picks local vs remote from the
  workspace type.
  <https://docs.openhands.dev/sdk/arch/conversation>

### Tool set & edit strategy (V1)

Tools live in `openhands-tools/openhands/tools/`: `apply_patch`, `ask_oracle`, `browser_use`, `delegate`,
`file_editor`, `gemini`, `glob`, `grep`, `planning_file_editor`, `preset`, `task`, `task_tracker`,
`terminal`, `tom_consult`, `workflow`.

- `FileEditorTool` command enum: `Literal["view", "create", "str_replace", "insert", "undo_edit"]`.
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-tools/openhands/tools/file_editor/definition.py#L26-L34>
- `ApplyPatchTool` takes a patch string in OpenAI's GPT-5.1 prompt-guide format
  (`*** Begin Patch` … `*** End Patch`) — a second, V4A-style edit strategy alongside `str_replace`.
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-tools/openhands/tools/apply_patch/definition.py#L23-L38>
- `glob` and `grep` now exist as first-class tools (V0 had neither).
  <https://github.com/OpenHands/software-agent-sdk/tree/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-tools/openhands/tools>
- Tool framework: `ToolBase`, `ToolDefinition`, `Action`, `Observation`, `ToolExecutor` (ABC with
  `__call__`), `ToolAnnotations` (MCP-spec hints: readOnly/destructive/idempotent/openWorld),
  `Tool` spec + `ToolRegistry`. Contract is strictly `Action → Observation`; tools know nothing about
  events, LLM messages, or conversation state.
  <https://docs.openhands.dev/sdk/arch/tool-system>
- Registration/selection: `Tool(name=TerminalTool.name)` in an `Agent(tools=[...])` list; alternative
  `ToolSet` classes group related tools (`TaskToolSet`, `WorkflowToolSet`, `BrowserToolSet`).
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-tools/openhands/tools/task/definition.py#L198-L210>
- **Parallel tool execution is opt-in and defaults OFF**: `Agent.tool_concurrency_limit` defaults to **1**
  ("Default is 1 (sequential). Values > 1 enable parallel execution"). The field description warns that
  concurrent tools "share the conversation object, filesystem, and working directory, so mutations to shared
  state may race." Execution uses a thread pool (`parallel_executor.py`, `max_workers=self.tool_concurrency_limit`)
  plus per-tool `declared_resources()` locks so tools that touch the same resource don't collide.
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-sdk/openhands/sdk/agent/base.py#L293-L302>
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-sdk/openhands/sdk/agent/agent.py#L423>
  The docs warn to keep dependent operations and same-file writes sequential.
  <https://docs.openhands.dev/sdk/guides/parallel-tool-execution>

### Context management / condenser (V1)

- `CondenserBase.condense()`, `RollingCondenser` (threshold-based triggering), `LLMSummarizingCondenser`,
  `NoOpCondenser`, `PipelineCondenser`, plus `View` / `Condensation`. Source dir:
  `openhands-sdk/openhands/sdk/context/condenser/` (files: `base.py`, `llm_summarizing_condenser.py`,
  `no_op_condenser.py`, `pipeline_condenser.py`, `utils.py`).
  <https://docs.openhands.dev/sdk/arch/condenser>
- **V1 shrank the condenser family from V0's ten to three** (`LLMSummarizing`, `NoOp`, `Pipeline`). The
  `LLMSummarizingCondenser` defaults are `max_size=240` and `keep_first=2`, with a validator requiring
  `keep_first < max_size // 2` "to leave room for" summarization; condensation triggers when
  `len(view) > max_size`.
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-sdk/openhands/sdk/context/condenser/llm_summarizing_condenser.py#L48-L78>
- Condensation is triggered by a `CondensationRequest` event emitted when the context window is exceeded;
  `Condensation` carries `forgotten_event_ids`, `summary`, `summary_offset`.
  <https://docs.openhands.dev/sdk/arch/events>
- A condensation **API endpoint** exists on agent-server: `condense-conversation`.
  <https://docs.openhands.dev/sdk/guides/agent-server/api-reference/conversations/condense-conversation>

### System prompt, skills, project rules (V1)

- Microagents were replaced by **Skills** (`openhands-sdk/openhands/sdk/skills/`: `skill.py`, `trigger.py`,
  `types.py`, `execute.py`, `fetch.py`, `installed.py`).
  <https://github.com/OpenHands/software-agent-sdk/tree/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-sdk/openhands/sdk/skills>
- Trigger types: `KeywordTrigger` (string match on user messages), `TaskTrigger` (keyword trigger for
  skills with user inputs), `PathTrigger` (glob match on a touched file path — "rules" — injected into the
  **tool result**, not model-invocable), and always-active repo skills (`trigger=None`).
  <https://docs.openhands.dev/sdk/arch/skill>
- **AGENTS.md is the recommended home for permanent repo instructions**: "Recommended: put these permanent
  instructions in `AGENTS.md` (and optionally `GEMINI.md` / `CLAUDE.md`) at the repo root." Skills also
  parse `.cursorrules`, `agents.md`, and other third-party formats.
  <https://docs.openhands.dev/sdk/arch/skill>
- Skills support **dynamic content**: `render_content_with_commands` executes inline `` !`command` ``
  patterns; skills can also carry MCP tool configuration.
  <https://docs.openhands.dev/sdk/arch/skill>
- `AgentContext` carries skills + prompts; `system_message_suffix` lets you append to the system message.
  <https://docs.openhands.dev/sdk/guides/parallel-tool-execution>
- **`system_prompt.j2` is gone in V1.** The system prompt is no longer a Jinja template but a typed
  **section registry** (`PromptRegistry` / prompt presets under `openhands-sdk/openhands/sdk/context/prompts/`,
  with a `sections/` subpackage), so prompt composition is compositional and typed rather than string-included.
  <https://github.com/OpenHands/software-agent-sdk/tree/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-sdk/openhands/sdk/context/prompts>

### Permissions & safety (V1)

- `SecurityAnalyzerBase.security_risk()`; implementations `LLMSecurityAnalyzer` (extracts an LLM-provided
  `security_risk` argument added to every tool schema) and `NoOpSecurityAnalyzer` (always UNKNOWN).
  `SecurityRisk` enum: `LOW`, `MEDIUM`, `HIGH`, `UNKNOWN`; `ConfirmationPolicy` maps risk → whether
  confirmation is required.
  <https://docs.openhands.dev/sdk/arch/security>
- Execution modes: **Direct** (execute immediately) and **Confirmation** (store as pending, wait for user).
  Risk handling: LOW → execute; MEDIUM → log warning and execute with monitoring; HIGH → block and request
  confirmation. Rejections surface as `UserRejectObservation`.
  <https://docs.openhands.dev/sdk/arch/agent>
- agent-server exposes `set-conversation-confirmation-policy`, `set-conversation-security-analyzer`, and
  `respond-to-confirmation` endpoints.
  <https://docs.openhands.dev/sdk/guides/agent-server/api-reference/conversations/set-conversation-confirmation-policy>
- Workspace isolation backends (`openhands-workspace/openhands/workspace/`): `docker`, `apptainer`,
  `cloud`, `remote_api`. The concrete classes are `LocalWorkspace`, `RemoteWorkspace`, `DockerWorkspace` /
  `DockerDevWorkspace`, `APIRemoteWorkspace`, `ApptainerWorkspace`, and `OpenHandsCloudWorkspace`.
  <https://github.com/OpenHands/software-agent-sdk/tree/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-workspace/openhands/workspace>
  <https://docs.openhands.dev/sdk/guides/agent-server/docker-sandbox>

### Session persistence, resume, fork (V1)

- **Persistence**: agent-server `persistence/store.py` defines `SettingsStore` and `SecretsStore` ABCs with
  atomic JSON writes and file locking.
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-agent-server/openhands/agent_server/persistence/store.py>
- **Fork exists in V1**: `fork()` on `ConversationBase`, `LocalConversation`, and `RemoteConversation`
  (`_copy_event_for_fork`, copies state events into the forked conversation). Remote fork does not support
  replacing the agent — `LocalConversation.fork(agent=...)` is required for that. Forking is
  **event-precise** (`fork(from_event_id=...)`) and pairs with `navigate_to`; agent-server exposes
  `POST /fork` and `POST /navigate`, and `StoredConversation` persists `forked_from_*` fields.
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-sdk/openhands/sdk/conversation/impl/local_conversation.py#L776>
  <https://docs.openhands.dev/sdk/guides/convo-fork>
- **Resume mechanism**: the SDK resumes by constructing a conversation with `agent=None` plus a
  `base_state.json`; `EventLog` replays by scanning for a marker and rescanning.
  <https://docs.openhands.dev/sdk/guides/convo-persistence>
- **Pause/resume** is a documented conversation feature: <https://docs.openhands.dev/sdk/guides/convo-pause-and-resume>
- **CLI resume**: conversations auto-save in `~/.openhands/conversations`; `openhands --resume` lists up to
  15 recent conversations with IDs/timestamps/first-message preview; `openhands --resume <id>` resumes;
  `openhands --resume --last` resumes the most recent. Resuming "loads the full conversation history from
  disk" including prior messages, agent actions, and file changes.
  <https://docs.openhands.dev/openhands/usage/cli/resume>

### Plan mode / todo / goal tracking (V1)

- `TaskTrackerTool` exists in `openhands-tools` (V0-style todo list).
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-tools/openhands/tools/task_tracker/definition.py#L403>
- V1 adds a real **goal completion loop**: `openhands-sdk/openhands/sdk/conversation/goal/` contains
  `controller.py`, `judge.py`, `runner.py`, `prompts.py`. `GoalStatusName = Literal["running", "complete",
  "capped", "interrupted"]`; `GoalStatus` carries `iteration` and `max_iterations`; `GoalController`
  "owns the iteration count and the `max_iterations`" (constructor default **max_iterations=10**) and
  exposes `start()` / `on_run_finished(events)`, returning `GoalContinue` or `GoalDone`. A separate
  `judge_llm` audits completion, so completion is model-judged rather than self-declared.
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-sdk/openhands/sdk/conversation/goal/controller.py#L25-L103>
  <https://docs.openhands.dev/sdk/guides/convo-goal>
- **Plan mode in V1 is a *preset agent*, not a flag.** There is no `PlanMode` switch; planning is
  `openhands-tools/openhands/tools/preset/planning.py`, an agent preset configured with `glob` + `grep` +
  `planning_file_editor` that writes a `PLAN.md` with a fixed five-section structure.
  <https://github.com/OpenHands/software-agent-sdk/tree/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-tools/openhands/tools/preset>
  <https://docs.openhands.dev/sdk/guides/iterative-refinement>
- GitHub workflow guides include todo management and PR review.
  <https://docs.openhands.dev/sdk/guides/github-workflows/todo-management>

### Subagents & parallel orchestration (V1)

This is the area that grew the most versus V0.

- `TaskTool` is explicitly documented in source as the **"Tool for launching (blocking) sub-agent tasks"**,
  with `TaskToolSet` backed by a shared `TaskManager`. Built-in `subagent_type` values shown in the docstring
  examples include `"test runner"`, `"web researcher"`, and `"general purpose"`.
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-tools/openhands/tools/task/definition.py#L169-L210>
  The task tool set is **blocking**, so true fan-out comes from `tool_concurrency_limit`,
  `Agent(tools=[...])` (see below), not from `TaskTool` itself.
  <https://docs.openhands.dev/sdk/guides/task-tool-set>
- A separate `delegate` tool exposes a `DelegateAction` whose command enum is `["spawn", "delegate"]`.
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-tools/openhands/tools/delegate/definition.py#L16-L47>
- Subagent definitions are markdown-with-frontmatter loaded from disk
  (`openhands-sdk/openhands/sdk/subagent/`: `schema.py`, `registry.py`, `load.py`). Front-matter fields
  include `tools`, `skills`, `mcp_config`, `permission_mode`, `max_iteration_per_run`, `max_budget_per_run`,
  `hooks`, `condenser`, `profile_store_dir`, plus `color` and `examples` extracted from the description.
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-sdk/openhands/sdk/subagent/schema.py#L33-L198>
- **Parallel orchestration is real**: the official example
  `examples/01_standalone_sdk/45_parallel_tool_execution.py` builds an orchestrator agent that "delegates to
  multiple sub-agents in parallel, and each sub-agent itself runs tools concurrently", using
  `tool_concurrency_limit=4` on each sub-agent and `TaskToolSet` + `DelegationVisualizer`.
  <https://docs.openhands.dev/sdk/guides/parallel-tool-execution>

### Extensibility (V1)

- **MCP**: `openhands-sdk/openhands/sdk/mcp/` (`client.py`, `config.py`, `definition.py`, `tool.py`).
  Configured with an `mcp_config` dictionary of server name → connection details, following the FastMCP
  client configuration format; supports tool filtering and OAuth. The config model is `MCPServer` as a
  **flat dict entry**, and supported transports are `stdio`, `http`, `streamable-http`, and `sse`. Note the
  SDK's settings shape is **not** the `.mcp.json` wrapper format — plugin files use the wrapper, settings do
  not.
  <https://docs.openhands.dev/sdk/guides/mcp>
  <https://github.com/OpenHands/software-agent-sdk/tree/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-sdk/openhands/sdk/mcp>
- **Hooks**: `openhands-sdk/openhands/sdk/hooks/` (`config.py`, `conversation_hooks.py`, `executor.py`,
  `manager.py`, `types.py`). Hook types and whether they can block: `PreToolUse` (blocks, exit 2),
  `PostToolUse`, `UserPromptSubmit` (blocks), `Stop` (blocks), `SessionStart`, `SessionEnd`. Configured from
  `.openhands/hooks.json` (and `~/.openhands`); there are `command`, `prompt`, and `agent` hook types, and a
  hook can block either with **exit code 2** or by returning `{"decision": "deny"}`.
  <https://docs.openhands.dev/sdk/guides/hooks>
- **Plugins**: `openhands-sdk/openhands/sdk/plugin/` (`plugin.py`, `loader.py`, `discovery.py`, `format/`,
  `installed.py`, `source.py`, `fetch.py`, `types.py`) plus `openhands-sdk/openhands/sdk/marketplace/` and
  `sdk/extensions/`. Docs: <https://docs.openhands.dev/sdk/guides/plugins>
- **Custom tools**: subclass the tool framework; docs at
  <https://docs.openhands.dev/sdk/guides/custom-tools>
- **Skills** as a user-facing extension surface: <https://docs.openhands.dev/sdk/guides/skill>

### Checkpoints & undo (V1)

- Still **no git-backed checkpoint/snapshot system** for the conversation. A grep for
  `checkpoint|revert|rollback` across `openhands-sdk/openhands/sdk/` returns only two unrelated comments in
  `remote_conversation.py` about reverting a conversation *status*. The only "undo" is the file editor's
  `undo_edit` command and conversation **fork**.
  <https://github.com/OpenHands/software-agent-sdk/tree/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-sdk/openhands/sdk>
- Fork (2.2 above) is the closest analogue: it copies the event log so you can branch from a point.

### Cost & token control (V1)

- Docs: metrics tracking <https://docs.openhands.dev/sdk/guides/metrics>; secrets registry
  <https://docs.openhands.dev/sdk/guides/secrets>.
- **Model routing / tiering**: `openhands-sdk/openhands/sdk/llm/router/` with implementations
  `multimodal.py` and `random.py`. <https://docs.openhands.dev/sdk/guides/llm-routing>
- **Fallback** between models: <https://docs.openhands.dev/sdk/guides/llm-fallback>; **profiles/store**:
  <https://docs.openhands.dev/sdk/guides/llm-profile-store>; GPT-5 preset:
  <https://docs.openhands.dev/sdk/guides/llm-gpt5-preset>; reasoning controls:
  <https://docs.openhands.dev/sdk/guides/llm-reasoning>.
- Budgets: per-run `max_iterations` (**500**) and a hard cost cap via `max_budget_per_run`; per-subagent
  `max_budget_per_run` / `max_iteration_per_run` come from subagent front-matter. I did **not** find a
  conversation-level budget flag distinct from these **(unverified)**.
  <https://github.com/OpenHands/software-agent-sdk/blob/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-sdk/openhands/sdk/subagent/schema.py#L242-L256>

### Provider abstraction (V1)

- The `LLM` class is described as a "Provider-agnostic language model interface with retry and telemetry".
  <https://docs.openhands.dev/sdk/arch/overview>
- Registry: `openhands-sdk/openhands/sdk/llm/llm_registry.py`-equivalent via `LLM(model=..., api_key=...,
  base_url=...)`; docs: <https://docs.openhands.dev/sdk/guides/llm-registry>.
- Error handling and retries: <https://docs.openhands.dev/sdk/guides/llm-error-handling>; image input:
  <https://docs.openhands.dev/sdk/guides/llm-image-input>; subscriptions:
  <https://docs.openhands.dev/sdk/guides/llm-subscriptions>.
- Example usage from the README: `LLM(model="gpt-5.5", api_key=os.getenv("LLM_API_KEY"))`, with
  `LLM_BASE_URL` honoured in the parallel-tool example (OpenAI-compatible endpoints).
  <https://github.com/OpenHands/software-agent-sdk>
  <https://docs.openhands.dev/sdk/guides/parallel-tool-execution>

### Streaming & TUI/UX (V1)

- Streaming in the SDK: <https://docs.openhands.dev/sdk/guides/llm-streaming>.
- agent-server speaks REST + **WebSocket**; OpenAI-compatible `/v1/chat/completions`.
  <https://docs.openhands.dev/sdk/arch/overview>
- The terminal UI is the separate **OpenHands CLI** repo, built with **Textual + Typer + rich**. Its
  commands include `openhands acp`, `openhands web`, `openhands serve`, and an `mcp` subcommand, with
  `--headless`/`-t`/`-f`/`--json`/`--resume`/`--always-approve`/`--llm-approve`.
  <https://github.com/OpenHands/OpenHands-CLI>
  <https://docs.openhands.dev/openhands/usage/cli/command-reference>
- IDE integrations documented: VSCode, JetBrains, Zed, Toad.
  <https://docs.openhands.dev/openhands/usage/cli/ide/overview>

### Headless / CI (V1)

- `openhands --headless -t "Your task here"` (or `-f task.txt`). "Headless mode always runs in
  `always-approve` mode. The agent will execute all actions without any confirmation. This cannot be
  changed—`--llm-approve` is not available in headless mode."
  <https://docs.openhands.dev/openhands/usage/cli/headless>
- `--json` emits **structured JSONL**, streaming one JSON object per agent event
  (`{"type": "action", "action": "write", "path": "app.py", ...}` /
  `{"type": "observation", ...}`), explicitly intended for CI parsing, logging, and integration.
  <https://docs.openhands.dev/openhands/usage/cli/headless>
- Full flag list: <https://docs.openhands.dev/openhands/usage/cli/command-reference>
- GitHub workflows exist as first-class guides (PR review, assign reviews, todo management).
  <https://docs.openhands.dev/sdk/guides/github-workflows/pr-review>

### Evaluation (V1)

- **Headline SWE-bench claim: 77.6.** The SDK README carries a badge
  `SWEBench-77.6` linking to a published results spreadsheet. Treat the number as a first-party claim
  whose methodology lives in the linked sheet, not in the README.
  <https://github.com/OpenHands/software-agent-sdk>
- Technical report / paper: `arXiv:2511.03690` (linked from the SDK README as the "Tech Report"; the v2
  version is titled as an MLSys 2026 paper and describes V1 as a "complete architectural redesign",
  reporting a **61% reduction in system-attributable failures** versus V0).
  <https://arxiv.org/abs/2511.03690>
- **The evaluation harness is NOT in the SDK repo.** It moved to a dedicated repository,
  `https://github.com/OpenHands/benchmarks` (HTTP 200). The V0 harness at
  `All-Hands-AI/OpenHands@0.62.0/evaluation/` is the historical location.
  <https://github.com/OpenHands/benchmarks>
- What the SDK repo *does* carry is `scripts/event_sourcing_benchmarks/` (a corpus of ~433 SWE-Bench
  Verified conversations used to benchmark event sourcing) plus CI labels for "API compliance" and
  "condenser" tests — i.e. regression testing of the event log and condensation, not agent capability.
  <https://github.com/OpenHands/software-agent-sdk/tree/3fc7b221516485e07604e8068de2fdc2d0ef3f09/scripts>

### Observability (V1)

- `openhands-sdk/openhands/sdk/observability/` contains `laminar.py` and `utils.py` — **Laminar** is the
  tracing integration, driven by environment variables, exposing an `observe` decorator and
  `record_tool_result`.
  <https://github.com/OpenHands/software-agent-sdk/tree/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-sdk/openhands/sdk/observability>
  Docs: <https://docs.openhands.dev/sdk/guides/observability>
- agent-server `telemetry/` is **consent-gated** (PostHog / HTTP export) and degrades to a no-op when
  consent is not granted.
  <https://github.com/OpenHands/software-agent-sdk/tree/3fc7b221516485e07604e8068de2fdc2d0ef3f09/openhands-agent-server/openhands/agent_server/telemetry>
  <https://docs.openhands.dev/sdk/guides/agent-server/api-reference/files/download-trajectory>
- Event stream remains the primary trace; `ConversationStateUpdateEvent` synchronizes state and serves as
  the state-snapshot channel for observers. agent-server exposes event search/count/get and a
  **download-trajectory** endpoint (`files/download-trajectory`).
  <https://docs.openhands.dev/sdk/arch/events>
  <https://docs.openhands.dev/sdk/guides/agent-server/api-reference/files/download-trajectory>
- Enterprise docs list "observability platforms" integrations.
  <https://docs.openhands.dev/enterprise/integrations/observability-platforms>

---

# PART 3 — Domain checklist cross-reference

| Domain | aider | OpenHands V0 (0.62.0) | OpenHands V1 (SDK) |
|---|---|---|---|
| Agent loop & tool-calling | No tool protocol; text edit-format response + `reflected_message` loop (max 3); no parallel calls | `Agent.step()` + `AgentController._step()`; function calling via litellm tools; multiple calls queued in `pending_actions` but **drained one per step (serialized)**; `max_iterations=500`; stuck detector | `Agent.step()` stateless loop; typed `ActionEvent`/`ObservationEvent`; parallel function calling grouped by `llm_response_id`; execution parallelism via `tool_concurrency_limit` (**default 1 = sequential**) |
| Event stream design | None (chat transcript + repo map) | `EventStream(EventStore)` + named subscribers (controller/memory/server/resolver/main) | Immutable append-only `EventLog`; ~13 typed event classes; `source` vs LLM `role` decoupled |
| Tool set & edit strategy | Edit formats: whole / diff (SEARCH-REPLACE) / diff-fenced / udiff / patch / editor-* | `cmd_run` (bash), `str_replace_editor` (default), IPython, browser, think, finish, task_tracker, LLM-based edit (off) | `terminal`, `file_editor` (`view/create/str_replace/insert/undo_edit`), `apply_patch` (GPT-5.1 patch format), `glob`, `grep`, `browser_use`, `ask_oracle`, `task`, `task_tracker`, `delegate`, `workflow`, `tom_consult` |
| Context management | Repo map (tree-sitter + PageRank, `--map-tokens` 1024–4096), `ChatSummary`, `/tokens`, `/drop`, `.aiderignore`, `--subtree-only` | 10 condensers, default `ConversationWindowCondenser`; `CondensationAction(forgotten_event_ids)` | `CondenserBase`/`RollingCondenser`/`LLMSummarizingCondenser`/`PipelineCondenser`; `CondensationRequest` → `Condensation`; `condense-conversation` API |
| System prompt & rules | `CONVENTIONS.md` via `/read`/`--read`; **no** AGENTS.md | Microagents: `knowledge`/`repo`/`task` types + `.openhands_instructions`; `system_prompt*.j2` | **Skills** with `KeywordTrigger`/`TaskTrigger`/`PathTrigger`; **AGENTS.md recommended**, plus CLAUDE.md/GEMINI.md/.cursorrules; inline `` !`cmd` `` |
| Permissions & safety | `--yes`, `confirm_ask`, `--dry-run`, read-only files; **no sandbox, no blocklist, no allowlist** | `confirmation_mode` + security analyzer; only HIGH risk (or UNKNOWN w/o analyzer) awaits confirmation; Docker sandbox by default | `SecurityRisk` LOW/MED/HIGH/UNKNOWN, `ConfirmationPolicy`, Direct vs Confirmation modes; workspace backends (docker/apptainer/cloud/remote_api); **isolation now opt-in** |
| Session persistence / resume / fork | `.aider.input.history`, `.aider.chat.history.md`, `--restore-chat-history`, `/save`+`/load`; **no fork** | `openhands/storage/` local/s3/gcs/webhook; `file_store_path=~/.openhands`; `--resume <id>`; no fork | `SettingsStore`/`SecretsStore`; `~/.openhands/conversations`; `--resume`, `--resume --last`; **`Conversation.fork()` exists** |
| Plan mode / todo / goals | None (ask mode is the idiom) | `enable_plan_mode` → `task_tracker` (`view`/`plan`, statuses todo/in_progress/done) + long-horizon prompt; **not a real plan mode** (no gating/approval) | `TaskTrackerTool` **plus** `conversation/goal/` `GoalController` with a judge LLM, `max_iterations=10`, statuses running/complete/capped/interrupted; plan mode = a **preset agent** (glob+grep+planning_file_editor → `PLAN.md`) |
| Subagents & parallel orchestration | **None**; architect/editor is a sequential 2-model pipeline | `AgentDelegateAction(agent, inputs)`, `delegate_level`; strictly sequential, single `self.delegate`, parent blinded to sub-trajectory | `TaskTool` (blocking sub-agents), `delegate` (`spawn`/`delegate`), markdown-defined subagents; **parallel** via opt-in `tool_concurrency_limit` |
| Extensibility | No MCP; `--lint-cmd`/`--test-cmd`/`--commit-prompt`, YAML config, python API, `--watch-files` | `openhands/mcp/` (`[mcp]` sse/stdio/shttp); runtime plugins (`AgentSkills`, `Jupyter`); custom `agenthub` agents | MCP via `mcp_config` dict (FastMCP format); **hooks** (PreToolUse/PostToolUse/UserPromptSubmit/Stop/SessionStart/SessionEnd); plugins + marketplace; custom tools |
| Checkpoints & undo | git auto-commits + `/undo` (refuses if pushed/dirty/merge); no shadow repo | none (only file-editor `undo_edit`; recovery limited to `LoopRecoveryAction` on stuck detection) | none (only `undo_edit`); **`fork(from_event_id=...)` + `navigate_to`** is the closest analogue |
| Cost & token control | `/tokens`, `--cache-prompts` (Anthropic/DeepSeek), `--cache-keepalive-pings`, `--weak-model` tiering; no budget cap | `Metrics(accumulated_cost, token_usages)`, `max_budget_per_task`, `-b` flag, `BudgetFlag`, `caching_prompt=True` | metrics + secrets guides; `llm/router/` (multimodal, random); per-subagent `max_budget_per_run` / `max_iteration_per_run` |
| Provider abstraction | **litellm** (`aider/models.py`, lazy import), 15+ documented providers incl. Ollama/LM Studio/OpenAI-compatible | **litellm** directly (`llm.py`), `LLMConfig` with `base_url`, `ollama_base_url`, `custom_llm_provider`, `num_retries=5` | `LLM` provider-agnostic class; registry, fallback, routing, profiles, presets |
| Streaming & TUI/UX | prompt-toolkit REPL, `--stream` default True, multiline `{}`/`{tag}`, vi/emacs, browser GUI, notifications | prompt-toolkit TUI (`openhands-cli/tui`), `streaming_llm.py`, web UI via `serve` | REST+WebSocket server, OpenAI-compatible endpoint, streaming guide; TUI in separate OpenHands CLI repo |
| Non-interactive / headless / CI | `-m/--message`, `-f/--message-file`, `--yes`, `--commit`, `--dry-run`, `AIDER_*` env vars | `get_headless_parser()`: `-t/--task`, `--file`, `-i/--max-iterations`, `-b/--max-budget-per-task`, `--no-auto-continue`, `--selected-repo`; `resolver/resolve_issue.py` | `openhands --headless -t/-f`; **always-approve**; `--json` JSONL event stream; agent-server REST; GitHub workflow guides |
| Evaluation | `benchmark/` (Exercism 133 + polyglot), `swe_bench.py`, leaderboards; SWE-bench 18.9% main / 26.3% Lite (2024) | `evaluation/benchmarks/` ~28 harnesses incl. `swe_bench` with `run_infer.sh`/`eval_infer.sh`; no score table at this tag (unverified) | SWE-bench **77.6** badge + `arXiv:2511.03690` (61% fewer system-attributable failures vs V0); harness moved to **`OpenHands/benchmarks`**; SDK repo has `scripts/event_sourcing_benchmarks` |
| Observability | `/tokens`, `message_cost`, `--llm-history-file`, markdown transcript, opt-in PostHog; **no OTel** | event stream as log; `io/json.py` `OpenHandsJSONEncoder`; `debug` flag; per-action metrics for frontend | Laminar tracing (`observability/laminar.py`), agent-server `telemetry/`, event search/count APIs, `download-trajectory` |

---

# PART 4 — Uncertainties and gaps

1. **OpenHands V0 SWE-bench score**: the `0.62.0` README does not state one and no score table exists at
   that tag; the README points to an external spreadsheet. Marked **(unverified)** rather than reused from
   a secondary write-up. Do not attribute a percentage to tag `0.62.0`.
2. **OpenHands V1 budget scope**: `max_iterations` (500) and `max_budget_per_run` are per-run/per-subagent;
   I found no separate conversation-level budget cap. **(unverified)**
3. **aider `--max-reflections`**: `max_reflections = 3` is a class attribute in the current source; I did
   not confirm whether an older release exposed it as a CLI flag.
4. **aider OTel**: absent by grep. Whether that is deliberate or simply unimplemented is a judgement, not a
   documented fact.
5. **Version skew across OpenHands repos**: SDK HEAD is `1.46.0` while the (unmaintained) OpenHands CLI pins
   `openhands-sdk==1.28.1`, and Agent Canvas is `1.17.0`. Pin a commit/version when citing behaviour, since
   these move independently.
6. **Git history was unavailable**: all clones were `--depth 1` and `api.github.com` returned 403, so the
   restructure timeline rests on READMEs, package manifests, docs, and the paper — not on commit history.
7. **V0 `openhands/server/` REST route names** were not enumerated (directory listing only).
8. **Shared scratch directory was deleted mid-session.** A sibling task cleaned
   `.scratch/research/` (removing a clone and a draft) while this research was in flight; the clones were
   re-created and the load-bearing citations (`should_step`, the `function_calling` assert, the Dockerfile
   sudo setup, `tool_concurrency_limit`, condenser defaults, MCP transports, hooks paths) were re-verified
   against the fresh clones. Deeper V0 citations were read live before the deletion and are pinned to tag
   `0.62.0` permalinks.
9. Agent-specific `step()` bodies in V0 (`browsing_agent`, `visualbrowsing_agent`, `loc_agent`,
   `readonly_agent`, `dummy_agent`) and the `agent_skills` function list were not read line-by-line.
