# Design & features: Anthropic Claude Code vs. Amp (Sourcegraph)

Research notes on the **design and features** of two coding-agent CLIs, from primary sources only
(official docs sites, official repos/blogs, official changelogs). Every substantive claim carries
the URL it came from. Anything I could not confirm from a primary source is marked **unverified**;
features that genuinely do not exist are called out as **absent**.

Scope note on sourcing and versions
- Claude Code's docs moved. `docs.claude.com/en/docs/claude-code/...` now redirects; the canonical
  docs live at **`https://code.claude.com/docs/en/<page>`** with a machine index at
  `https://code.claude.com/docs/llms.txt` and a raw-markdown form at `<page>.md`. All Claude Code
  citations below are `code.claude.com/docs/en/*`, which I fetched directly.
- The Claude Code docs I read describe a **version newer than any I can date from a changelog**:
  they reference Claude Code v2.1.17x–v2.1.26x, and models Opus 4.6/4.7/4.8, Sonnet 5, Fable 5.1,
  Haiku 4.5, and "Mythos 5". Treat version-gated statements as "as documented at fetch time".
- Amp's `ampcode.com/manual` now redirects to `ampcode.com/docs`; the manual is the docs site.
  Amp's docs likewise describe a future state (GPT-5.6, Claude Fable 5.1, GLM-5.2, news posts dated
  2026). Dates below are quoted as printed on the pages.

---

# 1. Anthropic Claude Code

## 1.1 Agent loop & tool-calling protocol

- The loop is documented as a multi-turn agentic loop: "Claude evaluates your prompt, calls tools to
  take action, receives the results, and repeats until the task is complete."
  (https://code.claude.com/docs/en/agent-sdk/agent-loop)
- **Turn definition:** "A turn is one round trip inside the loop: Claude produces output that includes
  tool calls, the SDK executes those tools, and the results feed back to Claude automatically. This
  happens without yielding control back to your code. Turns continue until Claude produces output with
  no tool calls, at which point the loop ends and the final result is delivered."
  (https://code.claude.com/docs/en/agent-sdk/agent-loop)
- **Stop conditions:** natural end (no tool calls) or a cap. `max_turns` / `maxTurns` "counts tool-use
  turns only"; `max_budget_usd` / `maxBudgetUsd` "cap turns based on a spend threshold". "Without
  limits, the loop runs until Claude finishes on its own."
  (https://code.claude.com/docs/en/agent-sdk/agent-loop)
- **Parallel tool calls — the notable design decision.** Concurrency is decided *per tool*, by
  read-only-ness: "When Claude requests multiple tool calls in a single turn, both SDKs can run them
  concurrently or sequentially depending on the tool. Read-only tools (like `Read`, `Glob`, `Grep`,
  and MCP tools marked as read-only) can run concurrently. Tools that modify state (like `Edit`,
  `Write`, and `Bash`) run sequentially to avoid conflicts." Custom tools default to sequential unless
  the tool sets `readOnlyHint` in its annotations (the MCP SDK field name).
  (https://code.claude.com/docs/en/agent-sdk/agent-loop)
- Loop message taxonomy (SDK): `SystemMessage`, `AssistantMessage`, `UserMessage` (also emitted after
  each tool execution with the tool result), `ResultMessage` ("marks the end of the agent loop.
  Contains the final text result, token usage, cost, and session ID"), plus `StreamEvent`. A
  `system` message subtype `"worker_shutting_down"` means "the loop will end after the current turn
  because the host is exiting or Remote Control disconnected".
  (https://code.claude.com/docs/en/agent-sdk/agent-loop)
- `ResultMessage` is not necessarily the last event: "A small number of trailing system events, such
  as `prompt_suggestion`, can arrive after it, so iterate the stream to completion rather than
  breaking on the result." (https://code.claude.com/docs/en/agent-sdk/agent-loop)

## 1.2 Tool set & edit strategy

- **Edit is exact string replacement, not diff or AST:** "The Edit tool performs exact string
  replacement. It takes an `old_string` and a `new_string` and replaces the first with the second.
  It doesn't use regex or fuzzy matching." (https://code.claude.com/docs/en/tools-reference)
- Three gates on an edit (https://code.claude.com/docs/en/tools-reference):
  1. **Read-before-edit** — the file must have been read in the current conversation; a read cut short
     with a `PARTIAL view` notice does not count. Opus 4.6/Haiku 4.5 and older always require the
     read; newer models may edit an unread file if reading it would need no permission prompt.
     Reading via `cat`, `nl`, `bat`, `batcat`, `head`, `tail`, `sed -n 'X,Yp'`, `grep`, `egrep`,
     `fgrep`, or `rg` on a single file (no pipes/redirects) satisfies the check.
  2. **Match** — `old_string` must match exactly; "A single character of whitespace or indentation
     difference is enough to miss."
  3. **Uniqueness** — `old_string` must appear exactly once, else Claude widens the context or sets
     `replace_all: true`.
  - A file changed on disk since the last read is still editable if `old_string` matches current
    content unambiguously; the result tells Claude the file carries other changes.
- Documented tool behaviours include: `Bash`, `Edit`, `Write`, `Read`, `Glob`, `Grep`, `NotebookEdit`,
  `WebFetch`, `WebSearch`, `Agent`, `AskUserQuestion`, `LSP`, `Monitor`, `PowerShell`,
  `SendFeedback`, `EndConversation`, and the Task tools (`TodoWrite`, `TaskCreate`, `TaskGet`,
  `TaskUpdate`, `TaskList`). Note the tool list is documented per-behaviour rather than as a single
  registry table, and `Agent` replaces the older `Task` naming.
  (https://code.claude.com/docs/en/tools-reference)
- **Tool set varies by model.** In v2.1.233+, `TodoWrite`, `TaskCreate`, `TaskGet`, `TaskUpdate`,
  `TaskList` are **not** available on Opus 4.8/Sonnet 5/Fable 5/Mythos 5 or later "unless you opt in",
  because "Those models keep track of multi-step work without a written checklist, and the tools'
  definitions and reminders take up context". Opt in with `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`, by naming
  a tool in `--allowedTools`, or by listing it in `--tools`.
  (https://code.claude.com/docs/en/tools-reference)
- **Subagent-spawning tool is named `Agent`** (not `Task`): "Spawns a subagent with its own context
  window to handle a task. With agent teams enabled, a call that carries a `name` can launch a teammate
  instead." The parent "doesn't see the subagent's intermediate tool calls or outputs, only that final
  result." (https://code.claude.com/docs/en/tools-reference)
- **Forked subagents** are a distinct mode: "The same Agent tool also launches forked subagents wherever
  fork mode is on. A fork inherits the full parent conversation instead of starting fresh, runs in the
  background apart from the cases that stay in the foreground, and still surfaces permission prompts in
  your terminal." (https://code.claude.com/docs/en/tools-reference)
- Subagent transcripts are reconstructable from the output stream via `--forward-subagent-text`, which
  emits subagent text/thinking "as `assistant` and `user` messages with `parent_tool_use_id` set"; it
  also covers **nested subagents** (subagents spawning subagents), setting `parent_tool_use_id` to the
  spawning Agent call. Requires `--print`, `--output-format stream-json`, and v2.1.211+ (nested: v2.1.219+);
  env var `CLAUDE_CODE_FORWARD_SUBAGENT_TEXT`. (https://code.claude.com/docs/en/cli-reference)
- **Tool naming has churned, and legacy names have surprising semantics.** `MultiEdit` is legacy: "If you
  write a path rule for `Write`, `NotebookEdit`, `Glob`, or the legacy `MultiEdit` tool instead, Claude
  Code accepts the rule but never consults it, and warns at startup… Use `Edit(docs/**)` in place of
  `Write(docs/**)`, `NotebookEdit(docs/**)`, or `MultiEdit(docs/**)`, and `Read(docs/**)` in place of
  `Glob(docs/**)`." (v2.1.210+). (https://code.claude.com/docs/en/permissions)
  - The exact version at which `Task` became `Agent`, and whether an alias is retained, is **unverified** —
    I only confirmed that the current documented name is `Agent`.
- Tool set varies by provider/platform/settings; the `advisor` tool is a **server tool** run by the API
  "rather than a tool that Claude Code implements. It has no name you can reference in permission rules
  or hook matchers." (https://code.claude.com/docs/en/tools-reference)

## 1.3 Context management

- Startup context is large and not just your prompt. Before the first prompt, Claude Code has loaded:
  the system prompt, `CLAUDE.md`, memory, the **skill index** (one-line descriptions only — "Full skill
  content loads only when Claude actually uses one"), and MCP tool definitions.
  (https://code.claude.com/docs/en/context-window)
- **Auto-compaction:** "Claude Code compacts automatically as you approach the limit, so a full context
  window doesn't end your session." (https://code.claude.com/docs/en/context-window)
- **Compaction is configurable to a token threshold**, in three places
  (https://code.claude.com/docs/en/model-config):
  - `/autocompact 500k` — saves to user settings key `autoCompactWindow`; `/autocompact auto` restores
    the model-tuned window.
  - `--autocompact <value>` CLI flag (one launch, not preempted by managed settings).
  - `CLAUDE_CODE_AUTO_COMPACT_WINDOW` env var — "takes precedence over the command, the flag, and the
    setting".
- Default threshold example: "On the Anthropic API, Sonnet 5 always runs with the 1M context window…
  Sessions auto-compact before the window fills, at about 967K tokens by default."
  (https://code.claude.com/docs/en/model-config)
- **`/compact` with a focus** — run with instructions, "like `/compact focus on the auth bug fix`,
  before starting a long new task. The summary keeps what you choose instead of what the automatic
  pass guesses is important." (https://code.claude.com/docs/en/context-window)
- **What survives compaction** is specified per mechanism: "Compaction replaces the conversation with a
  structured summary. System prompt, CLAUDE.md, memory, and MCP tools reload automatically. Claude Code
  also re-reads up to five of the files modified most recently, reloads the rules that match them, and
  re-injects the skills you invoked. The skill listing does not reload."
  (https://code.claude.com/docs/en/context-window)
  - Invoked-skill bodies are re-injected "capped at 5,000 tokens per skill".
    (https://code.claude.com/docs/en/context-window)
- Hooks interact with context: "Output reaches Claude via additionalContext JSON. Exit code 2 surfaces
  stderr to Claude. Plain stdout on exit 0 goes to the debug log, not the transcript."
  (https://code.claude.com/docs/en/context-window)
- Rewind offers summarization as a context tool, distinct from compaction: **"Summarize from here"** and
  **"Summarize up to here"** compress a chosen region of the conversation.
  (https://code.claude.com/docs/en/checkpointing)
- `@` file references, plus `--add-dir`/`/add-dir` and `permissions.additionalDirectories` for extra
  directories. Note the distinction: "The `permissions.additionalDirectories` setting in `settings.json`
  grants file access only and loads none of these" (skills, commands, agents) — while `--add-dir` does
  load them. (https://code.claude.com/docs/en/skills, https://code.claude.com/docs/en/permissions)
- Context accounting: `/context` lists what loaded, including **Memory files** and the skill index.
  (https://code.claude.com/docs/en/memory, https://code.claude.com/docs/en/context-window)
- Ignore/deny rules affect *what Claude can see at all*: "a bare tool name like `Bash` removes the tool
  from Claude's context entirely, so Claude never sees it" — and, because built-in tool definitions sit
  in the system-prompt cache layer, adding or removing such a rule mid-session **invalidates the
  prompt cache**. (https://code.claude.com/docs/en/permissions, https://code.claude.com/docs/en/prompt-caching)

## 1.4 System prompt & project rule files (CLAUDE.md / memory)

- **Claude Code reads `CLAUDE.md`, not `AGENTS.md`.** "If your repository already uses `AGENTS.md` for
  other coding agents, create a `CLAUDE.md` that imports it so both tools read the same instructions
  without duplicating them." The documented shim is a one-line `@AGENTS.md` import, or
  `ln -s AGENTS.md CLAUDE.md`. (https://code.claude.com/docs/en/memory)
- Load order, broadest scope first (https://code.claude.com/docs/en/memory):
  | Scope | Path |
  |---|---|
  | Managed policy | macOS `/Library/Application Support/ClaudeCode/CLAUDE.md`; Linux/WSL `/etc/claude-code/CLAUDE.md`; Windows `C:\Program Files\ClaudeCode\CLAUDE.md` |
  | User instructions | `~/.claude/CLAUDE.md` |
  | Project instructions | `./CLAUDE.md` or `./.claude/CLAUDE.md` |
  | Local instructions | `./CLAUDE.local.md` (gitignore it) |
- Hierarchy: CLAUDE.md/CLAUDE.local.md "in the directory hierarchy above the working directory are loaded
  at launch. Files in subdirectories load on demand when Claude reads files in those directories."
  (https://code.claude.com/docs/en/memory)
- Imports: `@path/to/import` syntax, expanded at launch. Import parsing "skips Markdown code spans and
  fenced code blocks", so `` `@README` `` stays literal. **External imports from a project-level memory
  file trigger an approval dialog the first time** ("to protect you from files other people commit to a
  shared project"); declining disables them permanently. (https://code.claude.com/docs/en/memory)
- Guidance is explicit about it being *context, not enforcement*: "CLAUDE.md files are loaded into the
  context window at the start of every session, consuming tokens alongside your conversation… Because
  they're context rather than enforced configuration, how you write instructions affects how reliably
  Claude follows them." Target **under 200 lines** per file.
  (https://code.claude.com/docs/en/memory)
- Path-scoped rules: `.claude/rules/` and `claudeMdExcludes` to skip other teams' CLAUDE.md in
  monorepos. (https://code.claude.com/docs/en/memory)
- `/init` generates a starting CLAUDE.md; `CLAUDE_CODE_NEW_INIT=1` enables a multi-phase flow that asks
  which artifacts to set up (CLAUDE.md, skills, hooks), explores the codebase with a subagent, and
  "presents a reviewable proposal before writing any files".
  (https://code.claude.com/docs/en/memory)
- **Auto memory** is a second mechanism alongside CLAUDE.md: "Auto memory lets Claude learn from your
  corrections without manual effort." (https://code.claude.com/docs/en/memory)
- **System prompt is replaceable:** `--system-prompt`, `--system-prompt-file`, and
  `--system-prompt-snapshot off` (rebuild the prompt every request rather than reusing the prompt
  snapshot). (https://code.claude.com/docs/en/cli-reference)
- **Output styles** are the sanctioned way to rewrite part of the prompt. Built-ins: **Default**,
  **Explanatory** ("Provides educational 'Insights'..."), **Learning** ("Claude Code will add
  `TODO(human)` markers in your code for you to implement"), and **Concise**. Selected with the
  `outputStyle` settings key (type string, default **unset**). A custom style is a Markdown file with
  frontmatter; key field
  `keep-coding-instructions: true` keeps Claude Code's built-in software-engineering instructions, and
  by default custom styles "leave out Claude Code's built-in software engineering instructions".
  (https://code.claude.com/docs/en/output-styles)
- The docs publish an explicit **injection-mechanism comparison** for prompt customisation, which is the
  clearest statement of where each mechanism sits in the request
  (https://code.claude.com/docs/en/output-styles):
  | Feature | How it works |
  |---|---|
  | Output styles | "Changes Claude Code's default instructions" |
  | **CLAUDE.md** | **"Adds a user message after the system prompt"** |
  | `--append-system-prompt` | "Appends to the system prompt without removing anything" |
  | Agents (subagents) | "Runs a subagent with its own system prompt, model, and tools" |
  | Skills | "Loads task-specific instructions when invoked or relevant" |
  - So CLAUDE.md is deliberately **not** part of the system prompt — it is injected as a user message
    after it. That is consistent with it being described as "context rather than enforced configuration".
    (https://code.claude.com/docs/en/output-styles, https://code.claude.com/docs/en/memory)
  - `--append-system-prompt` / `--append-system-prompt-file` append to the default prompt, and
    `--system-prompt-snapshot off` (v2.1.257+) rebuilds the system prompt on every request instead of
    reusing the snapshot recorded on the conversation's first request — explicitly for iterating on
    `--append-system-prompt` text across `--continue` runs.
    (https://code.claude.com/docs/en/cli-reference)

## 1.5 Permissions & safety

- Permission modes are switched with **`Shift+Tab`** in the CLI (also the VS Code mode indicator /
  Desktop mode selector). Cycle: `default` → `acceptEdits` → `plan` → back to `default`, with optional
  modes slotting in after `plan`. The status bar renders the mode, e.g. `⏸ manual mode on`,
  `⏵⏵ accept edits on`, `⏸ plan mode on`, `⏵⏵ auto mode on`, `⏵⏵ don't ask on`,
  `⏵⏵ bypass permissions on`. (https://code.claude.com/docs/en/permission-modes)
- `--permission-mode` accepts `default`, `acceptEdits`, `plan`, `auto`, `dontAsk`,
  `bypassPermissions`, or `manual` (alias for `default`).
  (https://code.claude.com/docs/en/cli-reference)
- `permissions.defaultMode` sets the starting mode. Notably, `auto` and `bypassPermissions` **do not
  take effect from project or local settings** — only user or managed settings, or the CLI flag
  (changed in v2.1.257). (https://code.claude.com/docs/en/settings)
- `acceptEdits` "Automatically accepts file edits and common filesystem commands such as `mkdir`,
  `touch`, `mv`, and `cp` for paths in the working directory or `additionalDirectories`."
  (https://code.claude.com/docs/en/permissions)
- `auto` mode replaces prompts with a **classifier**: `claude auto-mode defaults` prints the built-in
  classifier rules as JSON; `claude auto-mode config` prints effective config; `--label <prefix>`
  filters (e.g. `--label 'Git Destructive'`). `claude auto-mode reset` removes the `autoMode` section
  from user settings. (https://code.claude.com/docs/en/cli-reference)
- Rule syntax is `Tool(pattern)` inside `permissions.allow` / `ask` / `deny`
  (https://code.claude.com/docs/en/permissions):
  - `Bash(npm run build)` exact; `Bash(npm run *)` prefix wildcard; `Read(./.env)`; `WebFetch(domain:host)`;
    `Bash(run_in_background:true)`.
  - **Precedence is deny > ask > allow, and deny wins over a narrower allow:** "A broad deny rule like
    `Bash(aws *)` blocks every matching call, including calls that also match a narrower allow rule like
    `Bash(aws s3 ls)`, so a deny rule can't carry allowlist exceptions. The same precedence applies
    between ask and allow."
  - Matching a tool's *primary content* field is rejected: `Bash(command:rm *)` "would be bypassable by
    a compound command, so Claude Code ignores it and emits a startup warning." Primary fields are
    `command` (Bash/PowerShell), `file_path` (Read/Edit/Write), `path` (Grep/Glob), `notebook_path`,
    `url`.
  - `Bash(*)` ≡ `Bash`; as a deny rule both remove the tool from context.
  - Wildcard placement is validated: Claude Code warns at startup about an allow rule with a `*` before
    the subcommand such as `Bash(git * main)`.
- **Working directories** and path reach are a separate axis (see `permissions.additionalDirectories`,
  `permissions.blockReadsOutsideWorkingDirectories`).
  (https://code.claude.com/docs/en/permissions)
- **Two hard circuit breakers sit outside the allow-rule system** — a notable safety design, because they
  are explicitly *not* overridable by user policy (https://code.claude.com/docs/en/permission-modes):
  - **Protected paths**: "Writes to a small set of paths are never auto-approved, except in
    `bypassPermissions` mode… This prevents accidental corruption of repository state and Claude's own
    configuration." Per mode: `default`/`acceptEdits` prompt, `auto` routes to the classifier,
    `dontAsk` denies, `bypassPermissions` allows.
  - **Critical paths**: "Claude Code never lets a `permissions.allow` rule or a `PreToolUse` hook that
    returns `"allow"` approve an `rm` or `rmdir` command that targets a critical path, even in modes that
    skip other prompts. This circuit breaker guards against model error." A matching deny rule still
    blocks outright; otherwise the action is escalated per mode.
- `auto` mode is the **built-in default** on recent versions rather than an opt-in: "The built-in `auto`
  default requires Claude Code v2.1.228 or later on macOS, Linux, and WSL, and v2.1.233 or later on
  native Windows. On earlier versions, the built-in default is Manual." Auto mode also silently falls
  back to Manual when unavailable (e.g. an unsupported model, or Anthropic disabling it server-side).
  (https://code.claude.com/docs/en/permission-modes)
- **Sandboxing is OS-enforced and applies to Bash and its children**, not to the whole agent: "Instead of
  approving each command, you define which files and network domains commands can touch, and the
  operating system enforces that boundary for every Bash command and its child processes."
  (https://code.claude.com/docs/en/sandboxing)
  - Platform support and primitives: "The sandbox is built into Claude Code and runs on macOS, Linux, and
    WSL2. Native Windows is not supported. On Windows, run Claude Code inside a WSL2 distribution."
    "On macOS, there is nothing to install: sandboxing uses the built-in **Seatbelt** framework. On Linux
    and WSL2, the sandbox relies on two packages" — **`bubblewrap`** (unprivileged filesystem isolation)
    and **`socat`** (the relay that routes traffic through the sandbox proxy), plus a seccomp filter.
    "WSL1 is not supported because bubblewrap requires kernel features only available in WSL2."
    (https://code.claude.com/docs/en/sandboxing)
  - Known friction is documented rather than hidden: "**Go-based CLIs fail TLS verification on macOS**:
    tools such as `gh`, `gcloud`, and `terraform` may fail TLS verification under Seatbelt. List these
    tools in `excludedCommands` to run them outside the sandbox." There's also
    `sandbox.enableWeakerNetworkIsolation` for MITM-proxy setups with custom CAs, implying the network
    layer is proxy-based rather than TLS-inspecting by default.
    (https://code.claude.com/docs/en/sandboxing)
  - Two sandbox modes, same enforcement, differing only in auto-approval: an auto-approved mode where
    "Claude Code runs it inside the sandbox and approves it automatically", and a mode where
    non-sandboxable commands "fall back to the regular permission flow".
    (https://code.claude.com/docs/en/sandboxing)
  - **Default writable set:** "commands inside the sandbox can write to the working directory, the
    session temp directory, and any directories you've added" via `--add-dir`, `/add-dir`, or
    `permissions.additionalDirectories`. (https://code.claude.com/docs/en/sandboxing)
  - Filesystem config keys: `sandbox.filesystem.denyWrite`, `denyRead`, `allowRead`,
    `sandbox.filesystem.disabled` (skip filesystem isolation but keep network isolation), and
    `sandbox.network.allowUnixSockets`, `sandbox.excludedCommands`.
    (https://code.claude.com/docs/en/sandboxing)
  - Overlap rule is specificity-based: "When read rules overlap, the more specific path wins" —
    `"allowRead": ["~/"]` with `"denyRead": ["~/.env"]` keeps `.env` blocked, because "The deny holds
    inside a wider allow, so a broad allow can't silently re-expose a secret".
    (https://code.claude.com/docs/en/sandboxing)
  - **Credential protection** is a dedicated layer: `sandbox.credentials.files` with
    `{ "path": "~/.aws/credentials", "mode": "deny" }` and `sandbox.credentials.envVars` with
    `{ "name": "GITHUB_TOKEN", "mode": "deny" }`; modes include `deny` and `mask`. Env-var scrubbing is
    "independent of the filesystem layer", so it still applies when filesystem isolation is disabled.
    (https://code.claude.com/docs/en/sandboxing)
  - `/sandbox` has a **Dependencies** tab listing which of `ripgrep`, `bubblewrap`, `socat`, and the
    seccomp filter are missing. (https://code.claude.com/docs/en/sandboxing)
  - Honest escalation caveat: with filesystem isolation off and commands auto-allowed, a command can
    write shell startup files or `~/.claude/settings.json` "and use them to widen its own access on the
    next run". (https://code.claude.com/docs/en/sandboxing)
- Managed settings can lock the sandbox: `allowManagedDomainsOnly`, and where managed settings configure
  `sandbox.filesystem`, "only managed settings can set the key".
  (https://code.claude.com/docs/en/sandboxing)
- Hardening/test modes: `--restricted` "Start in restricted mode. Use it when an evaluation harness
  drives `claude` on a shared machine and Claude Code must not run commands or read that machine's user
  and project settings" — removes built-in command/code tools and WebFetch, confines file tools to
  working directories, loads only managed settings and `--settings`, and refuses `bypassPermissions`.
  `--safe-mode` starts "with all customizations disabled to troubleshoot a broken configuration".
  (https://code.claude.com/docs/en/cli-reference)
- Permission prompts in headless runs: `--permission-prompt-tool` (an MCP tool to answer prompts) and
  `--permission-prompts host|none` ("Pass `none` when nobody can answer, and Claude Code denies them
  instead").
  (https://code.claude.com/docs/en/cli-reference)

## 1.6 Session persistence, resume, fork

- Sessions save continuously to local `.jsonl` transcripts. Entry points: `claude --continue` (most
  recent in this directory), `claude --resume` (picker), `claude --resume <name>`,
  `claude --resume <session-id>`, or `claude --resume <absolute-path-to-.jsonl>`.
  (https://code.claude.com/docs/en/sessions)
- Cross-project resolution: "`claude --resume <session-id>` from any directory: Claude Code looks for
  the ID in the current project directory and its git worktrees first, then in every other project on
  this machine". (https://code.claude.com/docs/en/sessions)
- Interactive `-p`/SDK sessions are excluded from the picker and from `--continue`, but can be resumed
  by explicit session ID. Sessions whose first prompt was `/loop` are also hidden.
  (https://code.claude.com/docs/en/sessions)
- **Fork/branch:** `/branch` inside a session, or `--fork-session` combined with `--continue`/`--resume`:
  `claude --continue --fork-session`. "Sessions created with `/branch` or `--fork-session` get their own
  session IDs and appear as separate rows" in the picker.
  (https://code.claude.com/docs/en/sessions)
  - Semantics differ meaningfully: `/branch` "copies the transcript and switches the running Claude Code
    process to write to it", so "Allow for this session" grants are carried over; forking into a separate
    process with `--fork-session` means "the new process starts without them and you re-approve there".
    (https://code.claude.com/docs/en/sessions)
- Resume restores permission mode in some paths but not others: `claude --continue`,
  `claude --resume <session-id>`, or a name matching one session (without `-p`) restore the stored mode;
  the session *picker* does not — "It starts the session in the permission mode it would start a new
  session in from the same command line." (https://code.claude.com/docs/en/sessions)
- Naming: `claude -n "my-feature-work"` / `/rename`. Unnamed sessions get a default display name
  combining the directory and a two-char suffix (e.g. `my-app-3f`) which is explicitly **not** a resume
  handle, plus a generated title "written by a background request to the small/fast model, normally a
  Haiku-class model". (https://code.claude.com/docs/en/sessions)
- Background sessions: `claude agents` (agent view), `claude attach <id>`, `claude logs <id>`,
  `claude stop <id>`, `claude respawn <id>` ("Restart a background session… with its conversation
  intact"), `claude rm <id>`, and `claude daemon status|stop`.
  (https://code.claude.com/docs/en/cli-reference)
- `--no-session-persistence` (print mode only) avoids writing transcripts;
  `CLAUDE_CODE_SKIP_PROMPT_HISTORY` does the same in any mode.
  (https://code.claude.com/docs/en/cli-reference)

## 1.7 Plan mode / todo lists / goal tracking

- **Plan mode is a permission mode**, not a separate subsystem. It is documented under
  `permission-modes`, and `/docs/en/plan-mode` returns **404** ("Page Not Found") — a docs-structure
  detail worth knowing if you cite it.
  (https://code.claude.com/docs/en/permission-modes, https://code.claude.com/docs/en/plan-mode)
- Entry: "Enter plan mode by pressing `Shift+Tab` or prefixing a single prompt with `/plan`. You can
  also start in plan mode from the CLI: `claude --permission-mode plan`." Exit without approving:
  "Press `Shift+Tab` again to leave plan mode without approving a plan."
  (https://code.claude.com/docs/en/permission-modes)
- Approval transitions modes: "Approving a plan exits plan mode and switches the session to the
  permission mode each approve option describes, so Claude starts editing." The approve options are
  "Yes, and use auto mode", "Yes, manually approve edits", and "No, keep planning", and `Ctrl+G` opens
  the plan in `$EDITOR` for editing. (https://code.claude.com/docs/en/permission-modes,
  https://code.claude.com/docs/en/interactive-mode)
- Cycle details that matter for rebinding and for policy: optional modes "slot in after `plan`"
  (`bypassPermissions` first, `auto` last), and **`dontAsk` "never appears in the cycle"**. The
  rebindable action is exposed as **`chat:cycleMode`**, and `Shift+Tab` is not in the reserved-key list.
  (https://code.claude.com/docs/en/permission-modes, https://code.claude.com/docs/en/keybindings)
- Plan mode can be made the default via `permissions.defaultMode`; `useAutoModeDuringPlan` controls
  whether auto mode applies during planning.
  (https://code.claude.com/docs/en/permission-modes, https://code.claude.com/docs/en/settings)
- **Todo/task tracking is tool-based and model-gated**: `TodoWrite`, `TaskCreate`, `TaskGet`,
  `TaskUpdate`, `TaskList`, surfaced in the UI as the **task list**. On Opus 4.8/Sonnet 5/Fable 5/
  Mythos 5+ these are withheld unless opted in — the explicit rationale being context cost
  (https://code.claude.com/docs/en/tools-reference). Background sessions and Claude Code on the web
  provide them on every model. Subagents/teammates inherit the session's task-tool availability.
  (https://code.claude.com/docs/en/tools-reference)
- There is also a distinct concept of **scheduled/repeated prompts** via `/loop`
  (https://code.claude.com/docs/en/sessions), and **task lists** in interactive mode
  (https://code.claude.com/docs/en/interactive-mode).

## 1.8 Subagents & parallel orchestration

- Subagents are Markdown files with frontmatter, discovered by walking up from cwd: `.claude/agents/`
  (project, commit it) and `~/.claude/agents/` (user, all projects). Scanned **recursively**, so
  `agents/review/` subfolders work, and "The subdirectory path doesn't affect how a subagent is
  identified or invoked, because identity comes only from the `name` frontmatter field."
  (https://code.claude.com/docs/en/sub-agents)
- Precedence: managed subagents (in the managed settings directory) > project/user; across nested
  project dirs "the definition closest to the working directory wins" (v2.1.178+). Duplicate `name`
  values inside one directory resolve by filesystem read order — undocumented precedence — and
  `/doctor` reports them. (https://code.claude.com/docs/en/sub-agents)
- Live reload: "Claude Code watches `~/.claude/agents/` and `.claude/agents/`… detects the change within
  a few seconds and the next delegation uses the updated definition, with no restart needed." It does
  *not* watch `.claude/agents/` inside `--add-dir` directories.
  (https://code.claude.com/docs/en/sub-agents)
- Security-relevant limitation: "For security reasons, plugin subagents don't support the `hooks`,
  `mcpServers`, or `permissionMode` frontmatter fields. These fields are ignored when loading agents
  from a plugin." (https://code.claude.com/docs/en/sub-agents)
- Subagent contexts are isolated: a subagent's file reads do not enter the parent window — "Only the
  final summary comes back" (https://code.claude.com/docs/en/context-window).
- Parallelism beyond subagents: **agent teams** (`agent-teams`, incl. split-pane teammates running as
  separate processes) and **agent view** for dispatching/monitoring parallel background sessions
  (`claude agents --json`, `--cwd`, `--permission-mode`, `--model`, `--effort`, `--agent`).
  (https://code.claude.com/docs/en/agent-teams, https://code.claude.com/docs/en/agent-view,
  https://code.claude.com/docs/en/cli-reference)

## 1.9 Extensibility

- **Hooks.** Events are configured as matcher groups with handlers; the full documented event list is:
  `SessionStart`, `Setup`, `InstructionsLoaded`, `UserPromptSubmit`, `UserPromptExpansion`,
  `MessageDisplay`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PostToolUseFailure`,
  `PostToolBatch`, `PermissionDenied`, `Notification`, `SubagentStart`, `SubagentStop`, `TaskCreated`,
  `TaskCompleted`, `Stop`, `StopFailure`, `TeammateIdle`, `ConfigChange`, `CwdChanged`,
  `DirectoryAdded`, `FileChanged`, `WorktreeCreate`, `WorktreeRemove`, `PreCompact`, `PostCompact`,
  `PreModelSwitch`, `PostModelSwitch`, `SessionEnd`, `Elicitation`, `ElicitationResult`.
  (https://code.claude.com/docs/en/hooks)
- Hook mechanics (https://code.claude.com/docs/en/hooks):
  - **There are exactly 33 hook events** (counted from the documented event table).
  - **`PreToolUse` is the blocking gate and it runs *outside* the permission system**: PreToolUse hooks
    "run before every tool call, whether or not it needs permission", firing before any permission-mode
    check — so a hook denial holds even in `bypassPermissions`, and `dontAsk`. The governing asymmetry:
    **"Hooks can tighten restrictions but not loosen them."** By contrast `PostToolUse` cannot block, since
    "the tool already ran". Notably, `PermissionRequest` "run[s] only when Claude Code is about to ask you
    for permission, or when it would otherwise auto-deny a call that can't prompt", and its input carries
    an optional `permission_suggestions` array (e.g. adding an allow rule or changing mode). Neither event
    fires for `EndConversation`.
  - Handler types are five, not one: `type` ∈ **`"command"`, `"http"`, `"mcp_tool"`, `"prompt"`,
    `"agent"`**. Documented timeouts: default 600s for `command`/`http`/`mcp_tool`, 30s for `prompt`, 60s
    for `agent`; lowered to 30s on `UserPromptSubmit`, `PreModelSwitch`, `PostModelSwitch`, and to **10s**
    on `MessageDisplay`. `SessionEnd` hooks "share a 1.5-second budget". A timed-out hook's output is
    discarded, so "on most events a timed-out hook renders no decision".
  - Vocabulary is explicit: "**hook event** for the lifecycle point, **matcher group** for the filter,
    and **hook handler** for the shell command, HTTP endpoint, MCP tool, prompt, or agent that runs."
  - Input: JSON on stdin for command hooks, POST body for HTTP hooks. Matchers filter by tool name with
    alternation, e.g. `"matcher": "Bash|PowerShell"`; an extra `if` condition narrows to a specific
    command pattern like `Bash(rm *)`.
  - Output: exit 0 with no output = "no decision to report, so the tool call continues through the
    normal permission flow. The hook can deny the call, but staying silent doesn't approve it."
    A blocking hook returns JSON `hookSpecificOutput.permissionDecision: "deny"` plus a reason Claude is
    shown. Exit code 2 surfaces stderr to Claude.
  - `FileChanged` uses `matcher` "to specify which filenames to watch".
  - `PermissionDenied` supports `hookSpecificOutput.retry: true` to let the model retry a denied call.
  - `ConfigChange` fires for each settings-file change Claude Code detects (not for MDM/claude.ai-console
    managed settings). (https://code.claude.com/docs/en/hooks, https://code.claude.com/docs/en/settings)
  - Hooks can live in skills and agents; there's a `/hooks` menu (which labels each entry with a `[type]`
    prefix and its defining source) plus disable/remove paths.
    (https://code.claude.com/docs/en/hooks)
- **Slash commands.** The built-in command surface is documented at `/docs/en/commands`. Custom
  commands have been **merged into skills** — and the docs collation proves it: I downloaded
  `https://code.claude.com/docs/en/slash-commands.md` and `https://code.claude.com/docs/en/skills.md`
  and they are **byte-identical** (same MD5, both titled "Extend Claude with skills"). "A file at
  `.claude/commands/deploy.md` and a skill at `.claude/skills/deploy/SKILL.md` both create `/deploy` and
  work the same way. Your existing `.claude/commands/` files keep working." So `slash-commands` should
  not be cited as a distinct page; cite `commands` for the built-in list and `skills` for custom
  commands. (https://code.claude.com/docs/en/skills, https://code.claude.com/docs/en/commands)
- The built-in command surface is large — the `commands` page documents on the order of **112 slash
  commands**. Domain-relevant ones confirmed there include `/agents`, `/permissions`, `/hooks`,
  `/memory`, `/skills`, `/sandbox`, `/rewind`, `/cost`, `/usage`, `/usage-credits`, `/status`,
  `/doctor`, `/tasks`, `/diff`, `/init`, `/keybindings`, and `/config` (alias `/settings`; supports
  `key=value` shorthand such as `/config theme=dark` or `/config model=sonnet`, which also works in `-p`
  mode). Commands referenced from their own feature pages include `/compact`, `/autocompact`, `/plan`,
  `/model`, `/effort`, `/context`, `/add-dir`, `/branch`, `/resume`, `/mcp`, `/loop`, `/import`,
  `/reload-skills`, and `/rename`.
  (https://code.claude.com/docs/en/commands)
  - **Two commands I initially listed do not exist as such**: `/vim` is explicitly "Removed in v2.1.92.
    To toggle between Vim and Normal editing modes, use `/config` → Editor mode", and there is **no
    documented `/output-style` command** — output styles are chosen through `/config` or the `outputStyle`
    settings key, with style files in `~/.claude/output-styles/` or a project
    `.claude/output-styles/`. Verification note: I could not find `/output-style` anywhere in
    `commands.md`; the absence is evidence of removal but not an explicit statement, so treat it as
    **strongly indicated, not stated**.
    (https://code.claude.com/docs/en/commands, https://code.claude.com/docs/en/claude-directory)
- **Skills.** A directory with `SKILL.md`: "YAML frontmatter between `---` markers that tells Claude when
  to use the skill, and markdown content with the instructions". "The directory name becomes the command
  you type, and the `description` helps Claude decide when to load the skill automatically."
  Load scopes: enterprise (managed settings dir), personal `~/.claude/skills/<name>/SKILL.md`, project
  `.claude/skills/<name>/SKILL.md`, **nested** (`<subdir>/.claude/skills/…`, loaded lazily when Claude
  first touches a file there), additional directory via `--add-dir`, and plugin
  (`<plugin>/skills/<name>/SKILL.md`, invoked as `/plugin-name:skill-name`).
  (https://code.claude.com/docs/en/skills)
  - Model-invocation control: skills with `disable-model-invocation: true` "are not in this list. They
    stay completely out of context until you invoke them with `/name`" — a zero-context-cost opt-in.
    (https://code.claude.com/docs/en/context-window)
  - Reserved folder name `synced` (used for claude.ai-downloaded skills). A skill folder with
    `.claude-plugin/plugin.json` loads as a plugin. (https://code.claude.com/docs/en/skills)
- **MCP.** Transports: `http` (with `streamable-http` accepted as an alias), `sse`, `stdio`, and `ws`
  (WebSocket, config-file/`add-json` only — `claude mcp add --transport` doesn't accept `ws`, and it
  "supports neither" OAuth nor the transport flag). Commands: `claude mcp add --transport http <name>
  <url>`, `claude mcp add --transport sse <name> <url>`, `claude mcp add [options] <name> -- <command>
  [args...]` (everything after `--` goes to the server untouched), `claude mcp add-json`.
  Since v2.1.265, `--transport http` "tries the HTTP transport first and switches to SSE when the server
  doesn't accept it."
  (https://code.claude.com/docs/en/mcp)
  - Config footgun documented: "A JSON entry that has a `url` but no `type` is a configuration error,
    because Claude Code reads an entry with no `type` as a stdio server."
    (https://code.claude.com/docs/en/mcp)
  - Auth from the CLI without opening the TUI: `claude mcp login <name>` / `claude mcp logout <name>`
    (v2.1.186+), with `--no-browser` over SSH. (https://code.claude.com/docs/en/cli-reference)
  - Tool search / deferral: MCP tools may be deferred by **tool search** rather than loaded into the
    prompt prefix, with `alwaysLoad` to exempt a server; this choice directly affects prompt-cache
    behaviour. (https://code.claude.com/docs/en/prompt-caching, https://code.claude.com/docs/en/mcp)
  - `--mcp-config` and `--strict-mcp-config` (use only servers from `--mcp-config`).
    (https://code.claude.com/docs/en/cli-reference)
- **Plugins.** Install and load via `claude plugin install <name>@<marketplace>` (alias
  `claude plugins`), `--plugin-dir` (a directory, a `.zip`, or a folder of plugins), and `--plugin-url`
  (fetch a `.zip`). Plugin changes explicitly do **not** invalidate the prompt cache for their "skills,
  commands, agents, hooks, monitors, or themes", because that content is appended after the existing
  conversation. Plugins can also ship output styles in `output-styles/`.
  (https://code.claude.com/docs/en/plugins, https://code.claude.com/docs/en/prompt-caching,
  https://code.claude.com/docs/en/output-styles)
- **Plugins namespace everything at runtime**, which is how collisions are avoided across plugin,
  project, and user scopes: skills as `/plugin-name:skill-name`, MCP tools as
  `mcp__plugin_<plugin>_<server>__<tool>`, and agents as `<plugin>:<agent>` (e.g. `my-plugin:review:security`).
  (https://code.claude.com/docs/en/plugins)
- **Output styles operate above the system-prompt layer** and are scoped to the main agent: a custom
  style replaces Claude Code's built-in software-engineering instructions unless
  `keep-coding-instructions: true`, and styles **do not apply to subagents** — except forked subagents,
  which inherit the parent conversation. (https://code.claude.com/docs/en/output-styles,
  https://code.claude.com/docs/en/tools-reference)
- **Settings & config surface** is deep and layered. Precedence, highest first: managed →
  `--settings` (file or **inline JSON string**, ≤2 MiB) → `.claude/settings.local.json` →
  `.claude/settings.json` → `~/.claude/settings.json`. **Lists merge rather than override**
  ("Claude Code combines the lists instead of picking one"), with documented exceptions for
  `fallbackModel`, `modelPicker`, `availableModels`, `modelSettings`. Settings files are **watched and
  hot-reloaded**, applying to the running session for `permissions`, `hooks`, and credential helpers.
  (https://code.claude.com/docs/en/settings)
- Notable inversion for security: "For a few keys whose values restrict a session, Claude Code honors a
  restrictive value from a scope that otherwise couldn't override managed settings" —
  `disableClaudeAiConnectors`, `enableArtifact`, `isolatePeerMachines`, `remoteControlAtStartup`,
  `crossSessionInbound`, `useAutoModeDuringPlan`, `syncClaudeAiSkills`, `maxEffortLevel`.
  (https://code.claude.com/docs/en/settings)
- Repo-committed keys can be gated on trust: `permissions.allow`, `permissions.additionalDirectories`,
  `extraKnownMarketplaces`, and most `env` values "apply only after each teammate trusts the folder…
  `deny` and `ask` rules apply right away."
  (https://code.claude.com/docs/en/settings)
- TUI/interaction: `/status` shows `Setting sources`; `claude doctor` prints diagnostics and rejected
  settings entries; `/config` opens settings; `--setting-sources user,project,local` restricts which
  files load. (https://code.claude.com/docs/en/settings, https://code.claude.com/docs/en/cli-reference)

## 1.10 Checkpoints & undo

- Checkpointing is automatic and prompt-scoped: "checkpointing automatically captures the state of your
  code before each user prompt." (https://code.claude.com/docs/en/checkpointing)
- Retention and durability details (https://code.claude.com/docs/en/checkpointing):
  - "Claude Code keeps file snapshots for the 100 most recent checkpoints in a session."
  - "Claude Code saves checkpoints with the conversation, so you can still run `/rewind` after you
    resume a session."
  - Snapshots are deleted by the retention sweep, "by default about 30 days after the session last saved
    one"; extending requires `cleanupPeriodDays`. Rewinding past the sweep can fail with
    `No files were restored`.
- UI: "Run `/rewind`, or press `Esc` twice when the prompt input is empty, to open the rewind menu."
  If the input contains text, double-`Esc` clears it instead (the text goes to input history).
  (https://code.claude.com/docs/en/checkpointing)
- Six actions per checkpoint — notably **orthogonal restore axes**
  (https://code.claude.com/docs/en/checkpointing):
  - **Restore code and conversation** · **Restore conversation** (keep code) · **Restore code** (keep
    conversation) · **Summarize from here** · **Summarize up to here** · **Never mind**.
  - Code-restore options appear only when the checkpoint has tracked file changes; otherwise only
    `Restore conversation` plus the summarize options.
- Only **Claude's own file-editing tools** are tracked — checkpoints do not snapshot arbitrary shell side
  effects. This is corroborated by the VS Code extension using "each file's first snapshot… as the
  baseline for its session diffs". (https://code.claude.com/docs/en/checkpointing)
  - Practical consequence: Bash-mediated mutations (`rm`/`mv`/`cp`) and edits made by subagents are outside
    the checkpoint set, so `/rewind` will not restore them. This follows from "tracks all changes made by
    its file editing tools"; the docs do not enumerate every excluded path, so treat the exact boundary as
    **unverified**.
- Transcript storage path is specified: "By default, Claude Code stores transcripts as JSONL at
  `~/.claude/projects/<project>/<session-id>.jsonl`, where `<project>` is your working directory path
  with non-alphanumeric characters replaced by `-`." Long paths are truncated to 200 chars plus a hash.
  (https://code.claude.com/docs/en/sessions)
- **Git**: there is no automatic commit/checkpoint-to-git coupling documented for checkpoints;
  commit/PR workflows live in `common-workflows` and the Git-integration paths.
  (https://code.claude.com/docs/en/common-workflows)
- Agent SDK has a separate **file checkpointing** feature (`agent-sdk/file-checkpointing`).
  (https://code.claude.com/docs/en/agent-sdk/file-checkpointing)

## 1.11 Cost & token control

- **Prompt caching** is on by default and layered: "A change to the conversation layer leaves the system
  prompt and project context cached. A change to the system prompt invalidates everything, because all
  later content now sits behind a different prefix."
  (https://code.claude.com/docs/en/prompt-caching)
- Documented cache invalidators (each a real design constraint) — https://code.claude.com/docs/en/prompt-caching:
  switching model via `/model`; changing effort level; turning on fast mode; connecting/disconnecting an
  MCP server whose tools are loaded into the prefix; enabling/disabling a plugin (only for its own
  appended content, so mostly *not* an invalidator of the prefix); adding/removing a **bare-tool deny
  rule**; changing output style; **compacting the conversation** ("By design, this invalidates the
  conversation layer"); accumulating many images; upgrading Claude Code.
- Explicitly **cache-safe**: editing files in your repository, changing permission mode, and editing
  CLAUDE.md mid-session — the latter because "Your project-root and user-level CLAUDE.md files are read
  once at session start and held in memory. Editing them mid-session does not invalidate the cache, but
  the edit also doesn't apply. Claude keeps working with the version that was loaded at session start."
  (https://code.claude.com/docs/en/prompt-caching)
- One exception that saves the prefix: toggling `/advisor` keeps the cache, because "its definition sits
  after the cache breakpoint". (https://code.claude.com/docs/en/prompt-caching)
- Cache-warmth is user-visible: `/model` "asks you to confirm the switch only while the cache is still
  warm… Once that time passes, the cache has expired, so Claude Code switches without asking."
  (https://code.claude.com/docs/en/prompt-caching)
- Note: the specific cache **TTL values** (e.g. 5-minute vs 1-hour ephemeral breakpoints) are not in
  the page I read — the page refers to "cache TTL" and "Cache lifetime" generally.
  Treat exact TTL numbers as **unverified** for Claude Code.
- Cost/token surfaces: `/usage` (Session block with detailed token statistics; "Claude Code computes the
  dollar figure locally from token counts at list price, unless a `modelPricing` table is in effect" from
  managed settings — noted as an estimate, with the Console usage page authoritative);
  `modelPricing` in managed settings for contracted rates; `/usage-credits`;
  `~/.claude/usage-data/report.html` for usage-pattern analysis.
  (https://code.claude.com/docs/en/costs)
- Hard budget caps: `max_budget_usd`/`maxBudgetUsd` and `max_turns`/`maxTurns` in the SDK; `--max-turns`
  in the CLI. (https://code.claude.com/docs/en/agent-sdk/agent-loop,
  https://code.claude.com/docs/en/cli-reference)
- Auto-compaction window is itself a cost lever (`/autocompact`, `--autocompact`,
  `CLAUDE_CODE_AUTO_COMPACT_WINDOW`). (https://code.claude.com/docs/en/model-config)
- Model tiering: aliases `opus`, `sonnet`, `haiku`, `fable`, `best` (`best` = "the model the `fable`
  alias resolves to where Fable is available to you, otherwise the same model as `opus`"), plus
  `opusplan` for "an automated hybrid approach". Models behind aliases are provider-dependent and
  "update over time"; pin with a full name like `claude-opus-5` or `ANTHROPIC_DEFAULT_OPUS_MODEL` /
  `ANTHROPIC_DEFAULT_SONNET_MODEL` / `ANTHROPIC_DEFAULT_HAIKU_MODEL` / `ANTHROPIC_DEFAULT_FABLE_MODEL`.
  A small/fast Haiku-class model does background jobs such as generating session titles.
  (https://code.claude.com/docs/en/model-config, https://code.claude.com/docs/en/sessions)
- Effort is a first-class control: `/effort`, `--effort`, `effortLevel` and `modelSettings` settings,
  and an org cap via `maxEffortLevel` or Claude Enterprise per-role limits; "When both apply to a model,
  the lower cap applies." (https://code.claude.com/docs/en/model-config)
- Organization controls: `availableModels`, `enforceAvailableModels`, `fallbackModel`, automatic model
  fallback, and a documented **alias substitution** behaviour — when an allowlist blocks an alias's
  newest version, "Claude Code substitutes the newest version of the family that the allowlist permits
  and shows a notice naming both the requested and substituted models."
  (https://code.claude.com/docs/en/model-config)

## 1.12 Provider abstraction & multi-model support

- Multi-provider is documented as first-class with provider-specific pages: Amazon Bedrock
  (`amazon-bedrock`), Google Cloud's Agent Platform (`google-vertex-ai`), Microsoft Foundry
  (`microsoft-foundry`), a self-hosted **Claude apps gateway** (`claude-apps-gateway`,
  `claude-apps-gateway-config`; started with `claude gateway --config gateway.yaml`, v2.1.195+),
  `llm-gateway`, and `third-party-integrations`.
  (https://code.claude.com/docs/en/cli-reference, https://code.claude.com/docs/en/model-config)
- Provider-specific friction is documented honestly, e.g. auto mode is absent on some providers/models
  ("Only Claude Sonnet 5, Opus 4.7 or later, and the Fable models are supported on these providers"),
  and tool search may be unavailable on older Vertex models, custom `ANTHROPIC_BASE_URL` gateways, or
  certain Foundry deployments — which changes cache behaviour.
  (https://code.claude.com/docs/en/permission-modes, https://code.claude.com/docs/en/prompt-caching)
- Custom base URL / gateway support means the context window may be mis-assumed; there's a documented
  remedy ("Correct the window for a gateway or custom model ID").
  (https://code.claude.com/docs/en/context-window)
- Auth: `claude auth login` with `--email`, `--sso`, `--console` (Console/API billing vs subscription);
  `claude auth status` (JSON; `--text`); `claude setup-token` for a long-lived OAuth token for CI.

## 1.13 Streaming output & TUI/interaction UX

- Streaming in the SDK is opt-in: `include_partial_messages` / `includePartialMessages`, yielding
  `StreamEvent` messages (there's a full `StreamEvent` reference, a message-flow section, and a
  "Build a streaming UI" walkthrough). (https://code.claude.com/docs/en/agent-sdk/streaming-output)
- CLI streaming output: `--output-format text|json|stream-json` with `-p`; `--input-format stream-json`;
  `--include-partial-messages`; `--replay-user-messages` (re-emit stdin user messages on stdout for
  acknowledgment, requires stream-json in and out); `--prompt-suggestions` (emit a
  `prompt_suggestion` message after a turn; "Requires `--print`, `--output-format stream-json`, and
  `--verbose`"). (https://code.claude.com/docs/en/cli-reference)
- Interaction surface: `interactive-mode` (incl. the task list, prompt suggestions, vim mode),
  `statusline` (with a `session_name` field), `keybindings`, `terminal-config`, and `/doctor` for the
  in-session checkup that "can also apply fixes" vs `claude doctor` for read-only diagnostics.
  (https://code.claude.com/docs/en/interactive-mode, https://code.claude.com/docs/en/cli-reference)
- Session identity is surfaced in the TUI via the prompt bar (`-n`/`/rename` "also shows it on the
  prompt bar"). (https://code.claude.com/docs/en/cli-reference)

## 1.14 Non-interactive / headless / CI / print mode & scriptability

- `-p` / `--print` "Print response without interactive mode"; `claude -p "query"`,
  `cat file | claude -p "query"`, `claude -c -p "query"`. (https://code.claude.com/docs/en/cli-reference)
- Output/input: `--output-format text|json|stream-json`, `--input-format stream-json`,
  `--include-partial-messages`, `--verbose`, `--json-schema`. (https://code.claude.com/docs/en/cli-reference,
  https://code.claude.com/docs/en/headless)
- Continuity: `-c`/`--continue`, `-r`/`--resume`, `--session-id <uuid>`, `--fork-session`,
  `--from-pr` (open sessions associated with a PR). (https://code.claude.com/docs/en/cli-reference,
  https://code.claude.com/docs/en/sessions)
- Permissions in automation: `--permission-mode`, `--allowedTools`, `--disallowedTools`, `--tools`,
  `--permission-prompt-tool`, `--permission-prompts none`, `--dangerously-skip-permissions`,
  `--settings '<inline json>'`. (https://code.claude.com/docs/en/cli-reference)
- Startup-cost modes for automation: `--bare` ("Start faster with bare mode") and `--restricted`
  (evaluation-harness mode). Their difference is explicitly contrasted in the docs.
  (https://code.claude.com/docs/en/headless, https://code.claude.com/docs/en/cli-reference)
- CI: GitHub Actions integration (`github-actions`), plus `claude setup-token` for long-lived CI
  credentials. (https://code.claude.com/docs/en/github-actions,
  https://code.claude.com/docs/en/cli-reference)
- Agent SDK for programmatic embedding (Python and TypeScript), with `Options`/`ClaudeAgentOptions`
  fields for turns, budget, effort, permission mode, model, and custom tools.
  (https://code.claude.com/docs/en/agent-sdk/overview, https://code.claude.com/docs/en/agent-sdk/agent-loop)

## 1.15 Evaluation & regression tests

- **The Agent SDK ships eval-oriented primitives** rather than a bespoke harness: `max_turns`, budget
  caps, `--restricted` explicitly "when an evaluation harness drives `claude` on a shared machine",
  and `--json-schema` for structured assertions.
  (https://code.claude.com/docs/en/agent-sdk/agent-loop, https://code.claude.com/docs/en/cli-reference)
- A documented **regression-test use** of `/doctor`: it "reports files in the same directory that share
  a name and proposes renaming or removing all but one" for duplicate subagent definitions.
  (https://code.claude.com/docs/en/sub-agents)
- **No SWE-bench score or first-party eval harness/report is published in the docs I fetched.**
  Marked **absent/unverified** — I found no `eval` page in the docs index. If a SWE-bench number is
  needed, it should be sourced from Anthropic's model announcement posts (not fetched here), not the
  Claude Code docs.
- The docs do reference an official blog post on model/effort selection
  (https://claude.com/blog/claude-model-and-effort-level-in-claude-code) as guidance rather than as an
  eval. (https://code.claude.com/docs/en/model-config)

## 1.16 Observability

- **OpenTelemetry is the documented observability story**, enabled with `CLAUDE_CODE_ENABLE_TELEMETRY`
  and exported via `OTEL_METRICS_EXPORTER`, `OTEL_LOGS_EXPORTER`, and `OTEL_EXPORTER_OTLP_PROTOCOL`.
  (https://code.claude.com/docs/en/monitoring-usage)
- **Metric names** (all `claude_code.*`, extracted verbatim from the page):
  `claude_code.token.usage`, `claude_code.cost.usage`, `claude_code.session.count`,
  `claude_code.lines_of_code.count`, `claude_code.commit.count`, `claude_code.pull_request.count`,
  `claude_code.code_edit_tool.decision`, `claude_code.active_time.total`,
  `claude_code.tool_decision`, `claude_code.tool.execution`, `claude_code.compaction`,
  `claude_code.permission_mode_changed`, `claude_code.skill_activated`, `claude_code.subagent_completed`,
  `claude_code.at_mention`, `claude_code.mcp_server_connection`, `claude_code.api_retries_exhausted`,
  `claude_code.hook_execution_start`, `claude_code.hook_execution_complete`,
  `claude_code.plugin_loaded`, `claude_code.plugin_installed`, `claude_code.retention_sweep`,
  `claude_code.internal_error`, `claude_code.feedback_survey`.
  (https://code.claude.com/docs/en/monitoring-usage)
- **Log/event names** include the request/response lifecycle and privacy-relevant switches:
  `claude_code.api_request`, `claude_code.api_error`, `claude_code.api_refusal`,
  `claude_code.api_request_body`, `claude_code.api_response_body`, `claude_code.llm_request`,
  `claude_code.tool_result`, `claude_code.tool`, `claude_code.tool.blocked_on_user`,
  `claude_code.user_prompt`, `claude_code.assistant_response`, `claude_code.hook`, `claude_code.auth`,
  `claude_code.interaction`.
  (https://code.claude.com/docs/en/monitoring-usage)
- The presence of `api_request_body` / `api_response_body` implies prompt/response capture is available
  as an opt-in telemetry surface (relevant to both debugging and data-governance review).
  (https://code.claude.com/docs/en/monitoring-usage)
- Local diagnostics: `claude doctor` (read-only install + settings validation, "including install
  health, settings-file validation errors, and Remote Control eligibility"), `/doctor`, `--debug`,
  `claude logs <id>` for background sessions, `/status`, and `debug-your-config`.
  (https://code.claude.com/docs/en/cli-reference, https://code.claude.com/docs/en/debug-your-config)
- Settings diagnostics are tiered: **Settings Error** (whole file invalid → dialog offering to fix, exit,
  or continue), **Settings Warning** (individual entries skipped), and `~/.claude.json` corruption
  handling which copies the broken file to `~/.claude/backups/.claude.json.corrupted.<timestamp>` and
  keeps five `.claude.json.backup.<timestamp>` files. A `-p` run "shows no dialog" and silently skips
  broken files/values. (https://code.claude.com/docs/en/settings)
- Data governance surfaces: OpenTelemetry, `data-usage`, `Claude Code Analytics API`, and
  `claude project purge [path]` which deletes "transcripts, task lists, debug logs, file-edit history,
  prompt history lines, and the project's entry in `~/.claude.json`".
  (https://code.claude.com/docs/en/data-usage, https://code.claude.com/docs/en/cli-reference)

---

# 2. Amp (Sourcegraph)

Source set: `ampcode.com/docs/*`, `ampcode.com/news/*`, `ampcode.com/notes/*`, `ampcode.com/security`.
`ampcode.com/manual` now redirects to `ampcode.com/docs`.

## 2.1 Positioning / architecture (context for the rest)

- Amp is "a coding agent and development environment". The unit of work is a **thread**, which can run
  locally in the CLI, in an **orb** (a cloud machine Amp creates per thread), or on a **runner**.
  (https://ampcode.com/docs)
- Two components: **Amp Client** (CLI/app — "local code and context management, local settings, and
  local thread history") and **Amp Server** (ampcode.com, GCP — auth, accounts, workspaces, thread sync
  and storage, usage tracking). Thread actors connect to LLM providers "after the server authorizes
  inference and provides request-scoped credentials". No self-hosted deployment.
  (https://ampcode.com/security)
- Explicit product stance that shapes the feature set: "You're always using the good parts of Amp. If we
  don't use and love a feature, we kill it." / "No backward compatibility, no legacy features."
  (https://ampcode.com/docs)
- Thread data lives server-side in PostgreSQL, and a thread is portable across web, CLI, macOS/iOS, and
  Slack, with a URL `https://ampcode.com/threads/T-…`.
  (https://ampcode.com/security, https://ampcode.com/docs/threads)

## 2.2 Architecture evolution & the "agent is not the bottleneck" thesis

This is the most design-relevant material on Amp, and it **supersedes several statements in the current
static docs** — read this before trusting `docs/*` on context management, permissions, or queuing.

- **"The Coding Agent Is Dead" (Feb 19, 2026)** — the thesis: "With the newest models, the agent — the
  prompts and tools you wrap around a model — is no longer the limiting factor. These models can be
  powerful with nearly any tool you throw at them. **A simple tool called `bash` is often enough.**
  Whether you show LSP diagnostics here or there is dwarfed by what these models can do through sheer
  brute force. As long as it mostly gets out of the way, nearly any agent can get good results out of
  them." The asserted new bottleneck is not the harness: "**How you organize your codebase for agents,
  how your organization uses them — those are now the bottlenecks.**"
  (https://ampcode.com/news/the-coding-agent-is-dead)
- Concrete consequence: **they killed the editor extensions.** "First step: we're going to kill the Amp
  editor extensions for VS Code and Cursor. We're unshackling these models from the editor… The Amp
  editor extensions will self-destruct on March 5 at 8pm Pacific Time. Time to switch to the Amp CLI."
  The CLI is retained but framed as disposable: "Think of it as a ladder: we use it to climb up to the
  next level and then we might not need it." (https://ampcode.com/news/the-coding-agent-is-dead)
  - This directly contradicts the current `docs/cli` page, which documents editor integration for
    "VS Code, Cursor, Windsurf, Zed, and Neovim" (https://ampcode.com/docs/cli). Treat the editor
    extension status as **unverified** / in transition.
- **"Amp, Rebuilt" / codename Neo (May 6, 2026)** — a full CLI rewrite: "it's running on a completely
  new architecture: **remote-controllable, compaction-first, plugin-powered**, and much faster."
  (https://ampcode.com/news/neo)
  - **Remote control** from ampcode.com is the stated reason for the rewrite: "You'll not only get live
    updates but you can also send messages, queue and dequeue them, or cancel what the agent is
    currently doing… The architecture that enables this is the reason we rewrote Amp."
    (https://ampcode.com/news/neo); passkey requirement added later
    (https://ampcode.com/news/neo, https://ampcode.com/news/proof-of-human)
  - **Measured performance claims** on a ~5000-message thread: mean CPU 84.1% → 17.4%, peak CPU 86.3% →
    25.8%, idle memory 1814 MB → 540 MB ("70% less memory"), plus improved rendering.
    (https://ampcode.com/news/neo)
  - Neo became generally available May 27, 2026; the `--take-me-back` escape hatch was removed from the
    current CLI (older builds can still be run via `npx -y @ampcode/cli@0.0.1779896748-g596c49
    --take-me-back`). (https://ampcode.com/news/drop-the-neo)
- **Removed features are part of the design record** (https://ampcode.com/news/neo): handoff (obsoleted
  by compaction); rollback of file changes on message edit/restore; skill management subcommands
  (`amp skill add`/`import`/`update` — "That's better done by separate tools"); user-invokable skills;
  custom themes; and manual bash invocation via `$`/`$$` in the prompt editor. Rationale given
  throughout is "Amp should not make you work like it's still 2025."

## 2.3 Agent loop & tool-calling protocol

- Sourcegraph publishes the loop explicitly: model + system prompt + tools, tool definitions sent each
  request, on `content.Type == "tool_use"` execute locally and append a `tool_result`, re-infer, loop.
  The reference implementation is under 400 lines of Go. Their framing: "It's an LLM, a loop, and enough
  tokens." (https://ampcode.com/notes/how-to-build-an-agent)
- Their definition of an agent: "an agent is a model plus a system prompt plus tools. Tools allow the
  model to interact with the world outside its context window."
  (https://ampcode.com/guides/context-management)
- The loop is exposed as an event lifecycle to plugins:
  `session.start → agent.start → tool.call → tool.result → agent.end`, with `tool.call`/`tool.result`
  repeating per tool. **There is no `session.end` event**, and "Multiple threads can be started and
  continue to run at the same time in the same Amp CLI."
  (https://ampcode.com/docs/customize/plugins)
- **The stop condition is programmable**: `agent.end` may return
  `{ action: 'continue', userMessage: '…' }` to "append a follow-up user message and start another turn",
  with a documented warning to add a marker guard so the plugin "does not loop forever".
  (https://ampcode.com/docs/customize/plugins)
- Observed stop reasons in the wire format: `"end_turn" | "max_tokens" | "stop_sequence" | "tool_use" |
  "pause_turn" | "refusal" | null`. Run-level failure subtypes include `error_during_execution` and
  `error_max_turns`, and each run reports `num_turns`.
  (https://ampcode.com/docs/cli/streaming-json)

## 2.4 Tool set & edit strategy

- The authoritative built-in list is emitted in the `init` stream-json message:
  `["Bash","finder","create_file","edit_file","glob","Grep","mcp__postgres__query","oracle","Read",
  "read_mcp_resource","read_web_page","Task","todo_read","todo_write","undo_edit","web_search"]`
  (https://ampcode.com/docs/cli/streaming-json)
- `amp tools list` prints the builtin tools (https://ampcode.com/docs/tools).
- Design-relevant members of that list: `oracle` (second-opinion model as a *tool*), `Task` (subagents),
  `todo_read`/`todo_write` (task tracking), `undo_edit` (agent-invokable undo), `finder` (fuzzy file
  search), plus `Grep`, `glob`, `Read`, `create_file`, `edit_file`, `read_web_page`, `web_search`,
  `read_mcp_resource`. MCP tools are namespaced `mcp__<server>__<tool>`.
  (https://ampcode.com/docs/cli/streaming-json)
- Remote MCP tools saved on ampcode.com are **not** injected as JSON tool definitions: they "are
  discovered with `tool_search` and called through `code_exec`" — i.e. code execution as the tool-call
  protocol for that class of tools, with `outputSchema` validation of `structuredContent` and a
  `.raw(input)` escape hatch. (https://ampcode.com/docs/customize/mcp)
- **Edit strategy is `create_file` / `edit_file` at the tool-name level; whether `edit_file` is
  search-replace, unified diff, or AST-based is not documented — unverified.** No AST/LSP editing is
  documented. Compare Claude Code, where exact-string replacement *is* documented.
- Tool arguments are matchable by name in permission rules (e.g. `Bash` with `cmd`), implying declared
  per-tool argument schemas. (https://ampcode.com/news/tool-level-permissions)

## 2.5 Context management

- Thread ≈ context window. Contents: your messages, agent replies, tool calls, the system prompt,
  **all tool definitions**, the `AGENTS.md` files, and environmental data (OS, files in cwd, open file
  and selection). (https://ampcode.com/guides/context-management)
- Stated design principle: "**Quality degrades: the more context, the worse the results.**" Advice is
  focused threads, one per task. (https://ampcode.com/guides/context-management,
  https://ampcode.com/docs/prompting)
- `@` mentions: "Binary files are ignored, image files are attached as images, and text files will be
  included as is, except that they're truncated so as to not use up too much of the context window.
  Right now, they're truncated to a maximum of 500 lines and 2KB per line."
  (https://ampcode.com/guides/context-management)
- Thread references: paste a thread URL or `@T-…`; the agent uses a `read_thread` tool and a second model
  to extract only what's relevant. In the CLI, `@@` searches for a thread to mention.
  (https://ampcode.com/guides/context-management, https://ampcode.com/docs/prompting,
  https://ampcode.com/docs/threads)
- **Compaction was deliberately removed:** "We have removed compaction from Amp and replaced it with
  something that we think works a lot better: Handoff." Rationale: "It's lossy… compaction, we found,
  encourages long, meandering threads, in which you just compact once you run out of context window,
  stacking summary on top of summary." (https://ampcode.com/news/handoff)
- **Handoff** is the replacement: `/handoff <goal>` — "Amp then analyzes the current thread and
  generates a prompt to start the new thread, along with a list of relevant files," presented as an
  editable draft. Example invocations: `/handoff now implement this for teams as well, not just
  individual users`. (https://ampcode.com/news/handoff)
- **RESOLVED — compaction came back, handoff is out.** The apparent doc conflict (current `models` and
  `threads` pages still referencing compaction/handoff) is explained by the Neo rebuild: "So Amp now
  manages context for you. You don't have to watch context percentages anymore, or decide when to
  handoff, or extract information from a thread in a panic. When the context window fills up, Amp now
  compacts the thread: it summarizes the current context, starts a fresh window with that summary, and
  keeps going. **Compaction now runs automatically when the context window is 90% full.**"
  And explicitly: "So handoff is out. Compaction is in." (https://ampcode.com/news/neo)
  - The stated design reason is a deliberate bet on model capability: "A core principle behind the
    rebuild: build for what the frontier models can do now… Today's leading frontier models are great at
    handling compaction." (https://ampcode.com/news/neo)
  - Corroborating evidence that auto-compaction is load-bearing: "During one migration, we had to shut it
    off for a day and everyone complained." (https://ampcode.com/news/neo)
  - The earlier removal rationale is still worth reading as a design argument against compaction:
    "It's lossy… compaction, we found, encourages long, meandering threads."
    (https://ampcode.com/news/handoff)
  - Residual doc staleness: `docs/threads` still says to ask the agent to "Handoff and …"
    (https://ampcode.com/docs/threads). Treat `docs/*` mentions of handoff as **stale**.
- Thread references survive the rebuild: "You can also still reference other threads and Amp will read
  them and extract the relevant information… use Ctrl+O and `thread: new` to create a new thread, then
  hit Enter to quickly insert a reference to the previous thread."
  (https://ampcode.com/news/neo)
- Context editing primitives: `Tab` to select a previous message, then `e` to edit, `r` to restore,
  `f` to fork (https://ampcode.com/news/cli-tab-navigation). Editing resets the thread so the edited
  message becomes the last message and re-runs inference; restoring removes that message and everything
  after it. (https://ampcode.com/guides/context-management)
- Ignore/scope: `amp.fuzzy.alwaysIncludePaths` (array, default `[]`) — "Glob patterns for paths that
  should always be included in fuzzy file search, even if they are ignored by Git."
  (https://ampcode.com/docs/cli/settings)

## 2.6 System prompt & project rule files

- Amp's rule file is **`AGENTS.md`**. There is **no `AMP.md`** in current or historical docs. Timeline:
  `AGENT.md` introduced May 7, 2025 (https://ampcode.com/news/AGENT.md); switched to plural
  `AGENTS.md` on Aug 20, 2025 to converge on OpenAI's standard, "while staying backwards compatible with
  existing `AGENT.md` files" (https://ampcode.com/news/AGENTS.md).
- Resolution order (https://ampcode.com/docs/customize/agents-md):
  - `AGENTS.md` "in the current working directory (or editor workspace roots) *and* parent directories
    (up to `$HOME`) are always included."
  - Subtree `AGENTS.md` files "are included when the agent reads a file in the subtree" — lazy, like
    Claude Code's nested CLAUDE.md.
  - Always included if present: `$HOME/.config/amp/AGENTS.md`, `$HOME/.config/AGENTS.md`, and
    system-wide `/etc/ampcode/AGENTS.md` (Linux) / `/Library/Application Support/ampcode/AGENTS.md`
    (macOS) / `%ProgramData%\ampcode\AGENTS.md` (Windows).
  - Fallback: if no `AGENTS.md`, a sibling **`AGENT.md`** (no S) **or `CLAUDE.md`** is included.
  - Introspection: `agents-md list` from the command palette. Global AGENTS.md editable from
    Settings → Advanced.
- **Conditional context by glob** — a mechanism Claude Code doesn't have in comparable form: an
  `AGENTS.md` can `@`-mention other files, and a mentioned file with YAML frontmatter
  `globs: ['**/*.ts', '**/*.tsx']` "will only be included if Amp has read a file matching any of the
  globs... If no `globs` are specified, the file is always included when @-mentioned." Globs are
  implicitly prefixed with `**/` unless they start with `../` or `./`, and "@-mentions in code blocks
  are ignored, to avoid false positives." (https://ampcode.com/docs/customize/agents-md)
- The system prompt itself is Amp-supplied and **routes with the mode**: "Amp chooses the model,
  reasoning effort, system prompt, tools, and oracle for each mode."
  (https://ampcode.com/docs/the-dial)

## 2.7 Permissions & safety

- **Deny-by-default is inverted: Amp does not ask at all.** "By default, Amp does not ask for approval
  before running tools." (https://ampcode.com/docs/tools)
- **The rationale is documented and is a genuinely interesting design argument** — this is a deliberate
  rejection of static command inspection (https://ampcode.com/news/neo):
  - "What was once the `--dangerously-allow-all` flag is now the default behavior for users who have not
    configured permissions."
  - "A year ago tool calls were simpler to check: inspect the name, inspect the arguments, do string-based
    matching, allow or deny. Now, frontier models write throwaway scripts to get stuff done. They chain
    shell commands. It's near-impossible to determine statically whether a tool invocation will be
    destructive or not. When a model writes five 20-line Python scripts in parallel to do something,
    checking whether a tool call contains `rm -rf` gives you a false sense of security."
  - "On top of that, there are now custom skills and scripts, specifically built for agents. And different
    organizations have different policies around which model is allowed to call which tool.
    **So permissions now live in the Plugin API.**"
  - Compatibility path: "The old permissions system still exists. It's now a built-in plugin. If your
    existing Amp settings already opt into permissions — through `amp.permissions`,
    `amp.dangerouslyAllowAll: false`, or `amp.guardedFiles.allowlist` — Amp loads that plugin and works as
    before. (When the plugin is active, it applies in both `amp` and `amp --execute`.)"
  - Settings names `amp.dangerouslyAllowAll` and `amp.guardedFiles.allowlist` appear in this post but are
    **not** listed on the current `docs/cli/settings` page — treat those key names as
    documented-in-news-only.
  - Compare Claude Code, which moves in the **opposite** direction: OS-enforced sandboxing plus
    deny-before-ask rule precedence, with static pattern matching (`Bash(rm *)`) as the primary tool.
- The risk is stated plainly: "Amp acts on content in your workspace. Untrusted repositories, MCP
  servers, and other external inputs can influence what Amp does. If you regularly work with untrusted
  sources, consider creating a custom policy plugin, or using an isolated development environment."
  (https://ampcode.com/docs/tools)
- **Granular tool permissions** via the `amp.permissions` list. Each entry has `tool` (supports globs
  such as `mcp__*` and `*`), `matches` (argument globs, e.g. `{ "cmd": "*git commit*" }`, arrays
  allowed), and `action` ∈ **`allow` | `reject` | `ask` | `delegate`**. Crucially there is a
  **`delegate`** action that hands the decision to an external program: `{ "tool": "*", "action":
  "delegate", "to": "my-permission-helper" }` (helper must be on `$PATH`). "If no matching entry is
  found, Amp checks the built-in permission list that contains sensible defaults."
  (https://ampcode.com/news/tool-level-permissions)
- MCP server gating: `amp.mcpPermissions` (array, default `[]`) allows/blocks servers by `command`/
  `args` (local) or `url` (remote), first match wins, and "**If no rule matches an MCP server, Amp
  allows it**." (https://ampcode.com/docs/cli/settings, https://ampcode.com/news/mcp-permissions)
- Workspace-scoped MCP servers require explicit trust: servers in `.amp/settings.json` show as
  `awaiting approval` in `amp mcp doctor` and are approved with `amp mcp approve <server>`; servers in
  global settings or passed via `--mcp-config` do not require approval.
  (https://ampcode.com/docs/customize/mcp)
- Tool disabling: `amp.tools.disable` (array, default `[]`) supports globs and a `builtin:<toolname>`
  prefix so you can disable a built-in while keeping an MCP tool of the same name.
  (https://ampcode.com/docs/cli/settings)
- **Policy as code is the sanctioned extension point**: a plugin on `tool.call` can return `allow`,
  `reject-and-continue`, `modify` (rewrite the input), or `synthesize` (return a result without running
  the tool); workspace admins can distribute the plugin as a global workspace plugin.
  (https://ampcode.com/docs/customize/plugins, https://ampcode.com/docs/tools)
- **Sandboxing is environmental, not OS-level for the local CLI.** Orbs are "a sandboxed cloud machine";
  Amp Server "does not store a copy of the orb filesystem"; orb filesystems and snapshots are "encrypted
  at rest" (AES-256) and "All traffic in transit is encrypted using TLS 1.2+"; orbs pause after 5
  minutes of inactivity. The **local CLI relies on the developer's environment** — there is no
  documented OS-level filesystem/network sandbox comparable to Claude Code's `bubblewrap`/seatbelt
  Bash sandbox. Marked **absent** for the local CLI.
  (https://ampcode.com/security)
- Orb network egress policy is **not documented** (no allowlist/domain-control equivalent documented).
  Marked **unverified**.
- **No read-only / plan permission mode.** The documented way to get "plan only" is a prompt
  instruction: "If you want the model to not write any code, but only to research and plan, say so:
  'Do not edit any files.'" (https://ampcode.com/docs/prompting)
- **Secret redaction as a first-class layer**: Amp "automatically detects and redacts secrets before they
  can enter threads or be transmitted to any external service. This protection operates at the lowest
  level of the system to ensure that detected secrets are never visible to the LLM, stored in local
  cache, transmitted to LLM providers, or saved on ampcode.com," replacing them with a marker like
  `[REDACTED:amp]`. Coverage: AWS/GCP/Azure credentials, GitHub/GitLab/Sourcegraph/Amp tokens,
  OpenAI/Anthropic/HuggingFace keys, Stripe/Slack/npm tokens, plus generic API-key/webhook/password
  patterns. Documented as **best-effort**, with remediation guidance (edit the preceding message and
  resend, which overwrites the thread contents on ampcode.com; rotate the secret).
  (https://ampcode.com/security)
- Prompt-injection posture is defense-in-depth: current frontier models, Parallel for web context,
  automatic secret redaction, thread audit trail, a data management API, and retention controls.
  Prompt-injection reports are explicitly **out of scope for bug bounties** "due to LLMs' inherent
  nature and Amp's code execution capabilities". (https://ampcode.com/security)
- Client/network: allowlist `ampcode.com`, `auth.ampcode.com`, `production.ampworkers.com`,
  `static.ampcode.com`; credentials in `~/.local/share/amp/secrets.json`;
  "The Amp Client makes a best effort to avoid reading `.env` files and other credentials files."
  Token revocation via `POST https://ampcode.com/api/revoke` with the token in an `Authorization:
  Bearer` header. (https://ampcode.com/security)
- GitHub access: a GitHub App connection; the orb clone uses a credential "applies only to the clone
  command and is discarded afterwards", and subsequent git commands get short-lived per-command tokens
  from a credential helper — "Amp does not place a long-lived token in the orb." SSH URLs are rewritten
  to HTTPS. Commit signing uses an SSH keypair whose private key "never enters the orb".
  (https://ampcode.com/docs/github)
- Enterprise: SSO + SCIM via WorkOS; passkey requirement for sensitive actions including admins viewing
  workspace threads; managed settings enforced from
  `/Library/Application Support/ampcode/managed-settings.json` (macOS),
  `/etc/ampcode/managed-settings.json` (Linux), `%ProgramData%\ampcode\managed-settings.json`
  (Windows), plus `amp.admin.compatibilityDate`. (https://ampcode.com/security,
  https://ampcode.com/docs/cli/settings)

## 2.8 Session persistence, resume, fork

- Threads are server-persisted and device-independent by design: "A thread is not tied to the device you
  started it on." (https://ampcode.com/docs/threads)
- Resume/attach: `amp threads continue T-…` attaches the CLI to a thread; **`amp threads continue T-…
  -ox '<message>'` sends one message to the thread's own orb from CI or a script** without attaching.
  (https://ampcode.com/docs/threads)
- Management surface: `amp threads list|search|label|rename|archive|delete|markdown|export`, and
  `amp threads visibility private`, `amp threads share T-… --visibility unlisted`.
  (https://ampcode.com/docs/threads)
- Export: append `.md` to any thread URL; `amp threads markdown T-…`; `amp threads export T-…` gives
  full JSON to the thread's creator. (https://ampcode.com/docs/threads)
- **Fork was added and then deliberately removed.** "We're ripping out the Fork command." Forking was
  added July 2025; removal announced Jan 13, 2026: "Today we have better ways of sharing context between
  threads: handoff and thread mentions, which treat threads as first-class stores of context… We'd
  rather spend our time perfecting `handoff` and `thread mentions` than support `fork`." The replacement
  guidance is to start a new thread and mention the original by URL/ID.
  (https://ampcode.com/news/stick-a-fork-in-it)
- Message-level restore survives (`r` after `Tab`) even though thread forking was removed.
  (https://ampcode.com/news/cli-tab-navigation)
- Visibility model is part of persistence: `Private` / `Workspace` / `Group` (Enterprise) / `Unlisted`,
  with `amp.defaultVisibility` keyed by repository origin, e.g.
  `{"github.com/org/repo": "workspace"}`. (https://ampcode.com/docs/threads,
  https://ampcode.com/docs/cli/settings)

## 2.9 Plan mode / todo lists / goal tracking

- **No plan mode** in the permission sense. Planning is prompt-level ("Do not edit any files") plus plan
  documents the agent writes. (https://ampcode.com/docs/prompting)
- Task tracking is **tool-based**: `todo_read` and `todo_write` appear in the built-in tool list emitted
  by the streaming init message. (https://ampcode.com/docs/cli/streaming-json)
- Long-horizon work is handled by thread architecture rather than a goal stack: focused threads, handoff
  to a new thread, thread mentions to pull prior context, and agent-to-agent delegation.
  (https://ampcode.com/docs/prompting, https://ampcode.com/docs/orbs/agent-to-agent)

## 2.10 Subagents & parallel orchestration

- Specialist subagents: **Search** ("retrieves relevant code quickly"), **Oracle** ("handles difficult
  reasoning and planning questions"), **Librarian** ("researches external codebases and large bodies of
  source material"), **Read Thread** ("reads and summarizes other Amp threads"). "Each subagent has its
  own context window and access to tools like file editing and terminal commands."
  (https://ampcode.com/docs/models-and-subagents)
- **Isolation semantics are unusually explicit and deliberately restrictive:** "They work in isolation,
  so they can't communicate with each other, you can't guide them mid-task, and they start with the
  instructions and context the main agent gives them rather than the full conversation. The main agent
  only receives their final summary rather than monitoring their step-by-step work."
  (https://ampcode.com/docs/models-and-subagents)
- Selection: "Amp chooses subagents automatically for suitable tasks, most of `medium` mode but
  occasionally in other modes." Users can request them, including explicit fan-out patterns ("Convert
  these 5 files to use Tailwind, use one subagent per file").
  (https://ampcode.com/docs/models-and-subagents, https://ampcode.com/docs/prompting)
- **Oracle as a peer-review mechanism across model vendors:** "Every mode has an oracle that the main
  agent can consult for a second opinion. The more capable modes can pair different frontier models so
  one model can review the other's reasoning." In `high`, "GPT-5.6 Sol writes and Fable reviews"; in
  `ultra`, "Fable writes and GPT-5.6 Sol reviews". (https://ampcode.com/docs/the-dial,
  https://ampcode.com/news/the-dial)
  - Cost-aware by design: "We intentionally do not force the main agent to *always* use the oracle, due
    to higher costs and slower inference speed," and the docs recommend explicitly asking for it.
    (https://ampcode.com/docs/tools)
  - Oracle "runs with extra-high reasoning". (https://ampcode.com/docs/tools)
- **Agent-to-agent across threads** (a different axis from in-process subagents): an agent can start
  another thread in an orb or on a runner, exchange files, and wait for a result. Isolated workspaces are
  explicit: "Threads have separate workspaces. A message does not transfer files or commits by itself."
  Documented fan-out example: "Run four low-mode threads in parallel to test this flow in Chrome at four
  screen sizes… then summarize all four results here."
  (https://ampcode.com/docs/orbs/agent-to-agent)
- Librarian reach: all public GitHub code plus your private repos via the GitHub connection, but "The
  Librarian will only search code on the default branch of the repository."
  (https://ampcode.com/docs/tools)
- Concurrency guardrail: "Each user can start a burst of 20 metered orbs. After that burst, Amp starts
  one new orb every five minutes. Later orb starts wait instead of failing."
  (https://ampcode.com/docs/orbs)

## 2.11 Extensibility

- **MCP.** Local config in `amp.mcpServers`; `amp mcp add <name> -- <cmd>` and `amp mcp add <name>
  <url>`; remote definitions stored server-side via `amp mcp remote --personal|--workspace|--project add
  …` (scope option mandatory). Auth types `none | bearer | oauth`; tokens/secrets are passed **by file**
  (`--bearer-token-file`, `--oauth-client-secret-file`, `-` = stdin) and "Amp does not accept these
  secret values as command arguments." Loading precedence (highest to lowest): CLI `--mcp-config` >
  workspace `.amp/settings.json` > user `~/.config/amp/settings.json` > skills. OAuth redirect URI is
  `http://localhost:8976/oauth/callback`; `amp mcp oauth login/logout` manages local OAuth state.
  (https://ampcode.com/docs/customize/mcp)
- **Strong, opinionated guidance to bundle MCP servers in skills**, not globally: "Too many available
  tools can reduce model performance"; skill-provided tools "stay hidden until the skill loads," and
  `includeTools` glob-filters which tools a bundled server exposes. Best practices also say to prefer
  "MCP servers that expose a small number of high-level tools with high-quality descriptions" and to
  "consider using CLI tools instead". (https://ampcode.com/docs/customize/mcp,
  https://ampcode.com/docs/customize/skills)
- **Skills.** `SKILL.md` with YAML frontmatter `name` and `description`; "Amp lists every discovered
  skill for the model. The model sees each skill's `name` and `description` and uses them to decide when
  to load it. Amp loads the rest of `SKILL.md` only when the skill is invoked." The directory name and
  the frontmatter `name` must match. Skills may ship `scripts/` and `references/`, and can define MCP
  servers in a sibling `mcp.json` or in `mcpServers` frontmatter (frontmatter wins).
  (https://ampcode.com/docs/customize/skills)
  - Precedence is an explicit 11-step list, first `name` wins: `~/.config/agents/skills/` →
    `~/.agents/skills/` → `~/.config/amp/skills/` → project/ancestor `.agents/skills/` →
    `.claude/skills/` → `~/.claude/skills/` → `~/.claude/plugins/cache/` → `amp.skills.path` →
    built-in → personal skills repo → workspace skills repo. "Local and built-in skills therefore mask
    repository skills with the same name."
    (https://ampcode.com/docs/customize/skills)
  - **Amp reads Claude Code skills by default** (the `.claude/skills/` locations), switchable off with
    `amp.skills.disableClaudeCodeSkills`. (https://ampcode.com/docs/customize/skills,
    https://ampcode.com/docs/cli/settings)
  - Plugin-bundled skills get a qualified name `<plugin-name>:<skill-name>` so they don't collide.
    Tools listed in a skill's `builtin-tools` frontmatter stay hidden from the model until the skill
    loads. (https://ampcode.com/docs/customize/skills, https://ampcode.com/docs/customize/plugins)
  - **CONFLICT (unverified):** the Neo post states skill management was stripped back — "Amp still
    supports Agent Skills but we no longer offer commands or subcommands to add, remove, or update
    skills. That's better done by separate tools" — and that **user-invokable skills were removed**
    ("The latest generation of models now invokes skills reliably"). Yet the current
    `docs/customize/skills` page still documents `amp skill add <source>`, `amp skills repositories`,
    `amp skills list`, `amp skill import`, and `amp skill update`
    (https://ampcode.com/docs/customize/skills), and the settings page still documents
    `amp.skills.path` and `amp.skills.disableClaudeCodeSkills`
    (https://ampcode.com/docs/cli/settings). Treat the exact current skill CLI surface as
    **unverified** — the news post is newer than what the docs show, or the docs are ahead of it.
    (https://ampcode.com/news/neo)
  - Custom themes and manual bash invocation (`$`/`$$` in the prompt editor) were also removed in Neo.
    (https://ampcode.com/news/neo)
- **Plugins are the hook system.** TypeScript/JS modules using `@ampcode/plugin` types, from
  `.amp/plugins/`, `$XDG_CONFIG_HOME/amp/plugins/` or `~/.config/amp/plugins/`, personal, and workspace
  repos, with precedence **project > system > personal > workspace**. API surface:
  `amp.on(...)`, `amp.registerTool(...)`, `await amp.registerSkill(...)`, `amp.registerCommand(...)`,
  `amp.ai.ask(...)` (a "thread-scoped yes/no decision" classifier), `ctx.ui.notify/confirm/input/select`,
  `amp.configuration.get/update`, `amp.$` for shell, `ctx.thread.append(...)`, `amp.system.open(...)`,
  and `amp.onDispose` ("Runs on unload, reload, and graceful host shutdown (bounded to ~3s). Not run on
  crash/SIGKILL"). Helpers include `amp.helpers.shellCommandFromToolCall(event)` and
  `amp.helpers.filesModifiedByToolCall(event)`.
  (https://ampcode.com/docs/customize/plugins, https://ampcode.com/docs/plugin-api)
- Plugin events: `session.start` (fired when a thread session starts; "Put plugin-load initialization
  directly in the exported function body"), `agent.start`, `tool.call`, `tool.result`, `agent.end`.
  (https://ampcode.com/docs/customize/plugins)
- Commands live in a **command palette** (`Ctrl+O`) and are plugin-extensible; `registerCommand` takes
  `availability` of `{type:'enabled'}` / `{type:'disabled', reason}` / `{type:'hidden'}`.
  Plugin UI "is mirrored across TUI and Web surfaces", and "Plugin activation settings apply to both
  interactive `amp` sessions and `amp --execute` runs."
  (https://ampcode.com/docs/customize/plugins)
- Custom agent modes and custom subagents are plugin-declarable; custom modes "appear next to Amp's
  built-in modes in the picker". Proof that this is a real extension point: the deprecated
  `smart`/`deep`/`rush`/`large` modes were re-published as plugins
  (`amp plugins add --auto-update @amp/smart-classic`, `@amp/deep-classic`, `@amp/rush-classic`,
  `@amp/large-classic`), "with your model, your prompt, and your tools."
  (https://ampcode.com/docs/models-and-subagents, https://ampcode.com/news/the-dial)
- Distribution: personal and workspace plugins/skills live in **Git repositories** per scope;
  `amp plugins repositories`, `amp clone user-plugins` / `workspace-plugins`,
  `amp clone user-skills` / `workspace-skills`, `amp plugins import|update`, `amp skill add <source>`
  (`--global`), `amp plugins add <url>` (`--target workspace`). "Repository owners can require signed
  commits in the repository's Advanced settings." Importing is agent-mediated: "Amp finds the right
  repository, makes the requested change, commits it, and asks before pushing."
  (https://ampcode.com/docs/customize/plugins, https://ampcode.com/docs/customize/skills)
- **Project lifecycle hooks (orb-only)** — a different hook plane from plugins: executable
  `.agents/setup` and `.agents/resume` at the repo root, plus project-settings **Pre-clone Script** and
  **Pre-setup Script**. `.agents/setup` is capped at 20 minutes ("Amp stops setup after 20 minutes and
  continues starting the orb"), `.agents/resume` is awaited up to 10 seconds. Both may run more than
  once and must be idempotent. Setup must not leave long-running processes — "Amp stops every process the
  script leaves running when it exits, including commands started with `&`, `nohup`, or `setsid`" —
  use `.amp/services.yaml` or `systemctl enable --now`. Hook logs:
  `/home/user/.cache/amp/logs/setup.log` and `/home/user/.cache/amp/logs/resume.log`. Setup can mint
  OIDC tokens (`amp orb id-token --audience … --subject-scope project`).
  (https://ampcode.com/docs/orbs/customizing)
- Notifications/hooks for automation: `amp.notifications.enabled` (terminal bell over SSH or when
  `AMP_FORCE_BEL` is set), `amp.remoteThreadCreation.enabled`, `amp.updates.mode`.
  (https://ampcode.com/docs/cli/settings)

## 2.12 Checkpoints & undo

- **`undo_edit` is a built-in tool** — undo is agent-invokable, not only a user gesture.
  (https://ampcode.com/docs/cli/streaming-json)
- Message-level **restore**: `Tab` selects a prior message, then `r` "restore the thread up to and
  including that message" (Oct 21, 2025). (https://ampcode.com/news/cli-tab-navigation,
  https://ampcode.com/guides/context-management)
- **BUT file rollback was subsequently removed** — a direct contrast with Claude Code's `/rewind`:
  "Amp no longer rolls back file changes when you edit or restore a message. We've found ourselves using
  this less and less as models advanced. The models are now good enough to undo changes for you, with
  more finesse than a rollback. And, the truth is, the rollback feature was always best-effort: **if the
  agent wrote and ran code that generated files, we didn't keep track of that without elaborate
  snapshotting.**" (https://ampcode.com/news/neo)
  - So Amp's message restore is now conversation-only, and the stated replacement for undo is the model
    itself. This is the clearest single divergence from Claude Code's checkpoint design, which tracks
    per-prompt file snapshots precisely because generated-file side effects are otherwise untracked.
    (https://ampcode.com/news/neo, https://code.claude.com/docs/en/checkpointing)
  - Consequently the durable undo story in Amp is **git** (`Amp-Thread-ID` trailers, Ship/Review/Sync,
    `amp sync`), not a checkpoint store — with `undo_edit` as the in-turn tool.
    (https://ampcode.com/docs/threads, https://ampcode.com/docs/orbs)
- **Git is the durable checkpoint.** Agent commits carry an `Amp-Thread-ID: <thread-url>` trailer "so
  `git log` leads from a change back to the conversation that produced it" (`amp.git.commit.ampThread.
  enabled`, env `AMP_DISABLE_AMP_THREAD_TRAILER=1`) and a `Co-authored-by: Amp <amp@ampcode.com>`
  trailer (`amp.git.commit.coauthor.enabled`, env `AMP_DISABLE_AMP_COAUTHOR_TRAILER=1`).
  (https://ampcode.com/docs/threads, https://ampcode.com/docs/cli/settings)
- Orb → local sync keeps agent-side work reviewable: `amp sync <thread>` "mirror an orb thread's changes
  into your local checkout while the agent keeps working remotely." The web thread view has a **Changes**
  pane with **Ship** (commit and push per the project's Ship Behavior), **Review** (walk the diff), and
  **Sync**. (https://ampcode.com/docs/orbs, https://ampcode.com/docs/threads)
- **No documented file-level checkpoint equivalent to Claude Code's `/rewind`** with per-prompt
  snapshots and retention. Marked **absent/unverified**.

## 2.13 Cost & token control

- Modes exist explicitly to price capability. "The old modes were models in disguise… the only question
  left is capability against cost." Modes: `low` ("There is less to figure out for the model, so `low`
  builds it"), `medium` (default), `high`, `ultra`. Cost advice is direct: "Undershoot and the model
  churns… You pay three times for a result you could have had once. Overshoot and you're using Fable to
  fix a typo." (https://ampcode.com/news/the-dial, https://ampcode.com/docs/the-dial)
- Reasoning effort is **folded into the mode tier** rather than exposed separately: "Reasoning effort is
  part of the tier now. No more cycling `Opt+D` through effort levels on top of picking a mode."
  (https://ampcode.com/news/the-dial)
- Token/cache accounting is in the wire format: `usage` carries `input_tokens`,
  `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens`, `max_tokens`,
  `service_tier`, and a nested
  `cache_creation: { ephemeral_5m_input_tokens, ephemeral_1h_input_tokens }` — i.e. **two prompt-cache
  TTL tiers (5-minute and 1-hour)** are in use. (https://ampcode.com/docs/cli/streaming-json)
- Cost visibility: `amp.showCosts` (default `true`) "Show cost information for threads in the CLI while
  working" (workspace admins can hide it for all members); per-thread `$` in the web sidebar;
  `amp usage` for balance; "You can also ask Amp to explain your usage."
  (https://ampcode.com/docs/cli/settings, https://ampcode.com/docs/pricing)
- Billing model is stated precisely: "For individuals and non-enterprise workspaces, Amp does not add a
  markup to providers' API prices." Worked example: "if a thread uses $2 of Anthropic API usage and
  $0.50 of OpenAI API usage, Amp deducts $2.50 from your credits." Non-model tools such as web search
  also consume credits. Tiers: **Megawatt $20/mo** (≥$20 included agent usage, 750 hours of small orbs),
  **Gigawatt $200/mo** (≥$200, 1,000 hours of xxlarge orbs), and **Unconstrained** usage billing.
  Enterprise pricing is "50% more expensive". (https://ampcode.com/docs/pricing)
- Orb compute is a **second, separately metered resource** billed by the minute, paused orbs free:
  `a1.tiny` (1 CPU/2 GB) $0.08/h, `a1.small` (2/4) $0.17/h, `a1.medium` (4/8) $0.33/h,
  `a1.large` (8/16) $0.66/h, `a1.xxlarge` (16/32) $1.32/h, `a1.3xlarge` (16/44) $2.13/h — all with 60 GB
  disk; Enterprise +50%. `amp -ox "…" --orb-size a1.large` selects size per thread.
  (https://ampcode.com/docs/orbs/sizes-and-costs, https://ampcode.com/docs/orbs)
- Cost blast-radius controls: the 20-orb burst then one-per-five-minutes rate limit; auto-pause after 5
  minutes idle; "Purchase credits expire twelve months after purchase"; per-user cost controls and
  cost attribution/user groups on Enterprise. (https://ampcode.com/docs/orbs,
  https://ampcode.com/docs/pricing)
- ⚠️ Note on the numbers above: `ampcode.com/docs/pricing` and the news posts describe a future pricing
  state (GPT-6/GPT-5.6 era). Treat specific dollar figures as **time-sensitive**, not stable facts.

## 2.14 Provider abstraction & multi-model support

- Multi-model is a headline product claim: "**Multi-Model:** GPT-5.6, Claude Fable 5.1, fast models.
  Amp uses them all, for what each model is best at." (https://ampcode.com/docs)
- Routing is **server-side and dynamic**: "Amp may customize the main-agent and Oracle routing based on
  connected model provider subscriptions, workspace restrictions, and model availability."
  (https://ampcode.com/docs/the-dial)
- Users can override routing per mode: "If you want a mode to always use a specific model for its main
  agent, Oracle, or subagents, you can pin one per mode in the dial editor."
  (https://ampcode.com/docs/the-dial)
- Routing table at the time of the Dial announcement (documented as mutable: "This wiring will change as
  models improve. The dial won't.") — https://ampcode.com/news/the-dial:
  - `ultra`: Claude Fable 5, oracle GPT-5.6 Sol
  - `high`: GPT-5.6 Sol at `xhigh` reasoning effort, oracle Claude Fable 5
  - `medium`: GPT-5.6 Sol at medium effort, oracle GPT-5.6 Sol at high effort
  - `low`: **GLM-5.2 (Z.ai, open-weight)**, oracle GPT-5.6 Sol; workspace admins may substitute
    GPT-5.6 Terra low
  - The docs' tools page adds a later state: in `high`, main agent is GPT-6 Astra with medium reasoning
    and oracle is Claude Fable 5.1 (https://ampcode.com/docs/tools) — i.e. the two pages disagree;
    treat exact current routing as **unverified** and cite the routing table as of its announcement.
- Consumer-subscription linking is a distinct provider strategy: linking a ChatGPT subscription bills
  OpenAI model usage to that subscription instead of Amp credits and **changes Oracle routing** (`high`
  Oracle: Claude Fable 5.1 → GPT-6 Astra; `medium` Oracle → GPT-6 Astra on plans that include it). The
  `ultra` main agent deliberately stays on Amp credits "so it can run the strongest model Amp has
  selected for the hardest tasks". (https://ampcode.com/docs/the-dial)
- Bring-your-own-key is supported, with a documented provider constraint: Anthropic requires data
  retention for Claude Fable models, so BYOK Fable requires enabling data retention on the Anthropic
  workspace owning the key. Enterprise can also prevent members from using personal model routing.
  (https://ampcode.com/docs/models-and-subagents, https://ampcode.com/docs/pricing)
- Published provider/infra list (all inference on US infrastructure): Anthropic, OpenAI, SpaceXAI (x.ai),
  Meta, Gemini Enterprise Agent Platform (formerly Vertex AI), Amazon Bedrock, Baseten, Fireworks; e2b
  provides ephemeral compute for orbs; Parallel powers web search/retrieval; "We have no infrastructure
  or service providers based in China." (https://ampcode.com/security)

## 2.15 Streaming output & TUI/interaction UX

- Keybindings worth noting as design choices (https://ampcode.com/docs/cli/keybindings):
  `Ctrl+O` command palette ("Amp's most important keyboard shortcut"), `Ctrl+S` mode/dial,
  `Ctrl+R` prompt history, `Alt+T` expand thinking/tool blocks, `Alt+D` reasoning effort,
  `Alt+R` fast mode, `Ctrl+G` open prompt in `$EDITOR`, `Ctrl+C Ctrl+N` archive+new,
  `Ctrl+C Ctrl+E` archive+quit, `@` file mention. Full map: `amp config keymap`.
  Custom keymap via `amp.keymap`, supporting chords and a `<leader>` key; notably
  "keymaps in your user settings file at `~/.config/amp/settings.json` override workspace entries in
  `.amp/settings.json`" — the **opposite** of every other setting.
- **Steering vs. queueing is a first-class primitive**, and the default is steering:
  "If you send a message when the agent is still working, Amp steers the agent by default: your message
  is sent when the agent is done with its current step (such as a command or thinking block)." To queue
  until turn end: `Ctrl+Enter` on the web or `prompt: queue message`; to interrupt: `Esc Esc`.
  Configurable default under Settings → Advanced → Default Message Send Behavior. This surfaces in the
  wire protocol as a top-level `steer: boolean` on input messages.
  (https://ampcode.com/docs/prompting, https://ampcode.com/docs/cli/streaming-json)
  - **CONFLICT (unverified):** the Neo announcement states the default was **reversed** — "Queuing
    messages is now the default. When you send a message while the agent is busy, it'll get added to the
    queue instead of stopping and interrupting the agent… If you want to fast-track a queued message, you
    can *steer*… Use ↑ to select a queued message, then steer it with ⏎." The rationale is again
    model-capability-driven: "They work for longer and need fewer mid-flight yanks."
    (https://ampcode.com/news/neo). The current `docs/prompting` and `docs/cli/keybindings` pages still
    say steering is the default, so **which is actually current is unverified** — verify against a live
    CLI (`amp --help` / the Settings → Advanced toggle) before relying on either.
    (https://ampcode.com/docs/prompting, https://ampcode.com/docs/cli/keybindings)
- Editor integration: VS Code, Cursor, Windsurf, Zed, Neovim; gives Amp the current open file and
  selection, and "Edit files through your IDE, with full undo support".
  (https://ampcode.com/docs/cli)
- Input affordances: `Ctrl+V` paste image (must be Ctrl+V, not Cmd+V, even on macOS); voice dictation
  (`Cmd+Shift+D` macOS / `Ctrl+Shift+D` elsewhere) with a custom vocabulary
  (`amp config vocabulary add <term>`); `Shift+Enter` newline requires terminal modified-Enter support
  (Ghostty, WezTerm, Kitty, iTerm2, or tmux with `extended-keys`), with `Ctrl+J` as the universal
  fallback. (https://ampcode.com/docs/prompting, https://ampcode.com/docs/cli/keybindings)
- Thread UX includes **Snooze** ("hides the thread until the agent's next message, until 8am, or until
  Monday morning, and mutes its notifications"), Labels, Pin, Color, and Archive — with the honest
  caveat that "Snoozing does not stop the agent, and it does not pause the thread's automation;
  scheduled runs keep firing and using credits." (https://ampcode.com/docs/threads)

## 2.16 Non-interactive / headless / CI / print mode

- `-x` / `--execute`: "it sends the message provided to `-x` to the agent, waits until the agent ends its
  turn, prints its final message, and exits." (https://ampcode.com/docs/cli/execute-mode)
- `-ox`: runs the same prompt **in an orb** on Amp's servers; "The CLI prints the new thread's URL and
  exits while the agent keeps working." (https://ampcode.com/docs/cli/execute-mode,
  https://ampcode.com/docs/orbs)
- Piping: `echo "…" | amp -x`; "Execute mode is automatically turned on when you redirect stdout"; and
  with both a pipe and `-x`, "the agent can see both" (e.g. `cat ~/.vimrc | amp -x "which colorscheme
  is used?"`). (https://ampcode.com/docs/cli/execute-mode)
- `--stream-json` (requires `--execute`) emits one JSON object per line; `--stream-json-input` reads user
  messages from stdin until close, enabling multi-message scripted conversations; `--stream-json-thinking`
  adds `thinking`/`redacted_thinking` blocks and "is not compatible with Claude Code".
  (https://ampcode.com/docs/cli/streaming-json)
- **Deliberate Claude Code compatibility:** "Amp's stream JSON output tries to be compatible with Claude
  Code's format as much as possible," and user image blocks "are accepted as input but omitted from
  streamed `user` messages on stdout so the output remains compatible with Claude Code." The documented
  schema includes `system/init`, `user`, `assistant`, and a terminal `result` message with
  `duration_ms`, `num_turns`, `permission_denials`, and `usage`.
  (https://ampcode.com/docs/cli/streaming-json)
- `--mcp-config '<json>'` works with `-x`. `--plugin-ready-timeout [secs]` (bare = 10s, max 300, `0`
  disables) makes execute mode wait for plugins so `agent.start`/`agent.end` are not skipped.
  (https://ampcode.com/docs/cli/execute-mode)
- CI auth: `export AMP_API_KEY=<token from Settings>`; the token starts with `sgamp_`, and "The CLI
  rejects the short-lived session token that `amp login` stores, because it expires within an hour and
  cannot be refreshed from an environment variable." (https://ampcode.com/docs/cli/execute-mode)
- Mode state in automation: "Interactive CLI threads remember whether the last thread used Fast mode.
  Execute mode and other noninteractive thread creation default to Standard mode each time. Pass
  `--fast` to enable Fast mode for one invocation" (alias for `--features fast`).
  (https://ampcode.com/docs/cli/execute-mode)
- Headless runner: `amp --no-tui` serves remotely-created threads in the cwd; `--runner-id <id>` gives a
  stable id; `--remote-control-terminal` exposes terminals; `amp.remoteThreadCreation.enabled` turns any
  interactive TUI into a runner. (https://ampcode.com/docs/cli/runners)
- Executor selection: `amp --executor local|orb|runner:<id>`. (https://ampcode.com/docs/cli)
- Thread continuation from scripts: `amp threads continue T-… -ox '<message>'` (no attach), and any
  thread URL with `.md` appended is plain Markdown. (https://ampcode.com/docs/threads)
- SDK exists (`ampcode.com/docs/sdk`) — not read in depth here; treat SDK specifics as **unverified**.

## 2.17 Evaluation & regression tests

- Amp publishes a **model-evaluation** page, but its stated philosophy rejects evals as a quality oracle:
  "Everyone wants to treat evals as some magical oracle for what 'good' is, because this seems
  scientific. but really this is scientism, because it's impossible to fully capture product experience
  in evals and whatever evals you do construct are backward looking."
  (https://ampcode.com/news/model-evaluation)
- What they do use evals for: "evals are primarily useful as unit tests and regression tests, to ensure a
  new model doesn't regress some existing behavior you'd like to preserve."
  (https://ampcode.com/news/model-evaluation)
- Practical eval signals they name: real usage with public thread sharing (qualitative), and the
  "pelican riding a bicycle" style simple-to-verify test — with the caveat that "even that eval I suspect
  is quite gameable". Model triage is by role: frontier main-agent candidate / subagent specialist /
  fast utility model. (https://ampcode.com/news/model-evaluation)
- **No SWE-bench score and no published eval harness/CLI.** Marked **absent**. The clearest evidence of
  hands-on regression testing is the per-task commentary in the model-evaluation log itself, e.g.
  GPT-5 "has a habit of generating invalid JSON for tool args, causing gibberish to show up in tool calls
  or getting into generation loops that consume the entire output token limit."
  (https://ampcode.com/news/model-evaluation)

## 2.18 Observability

- **The thread is the audit trail**: "Thread storage preserves prompts, model responses, tool calls, tool
  results, and attachments, creating an audit trail to help determine whether a prompt-injection attempt
  occurred and what actions followed." (https://ampcode.com/security)
- Enterprise audit logging is **partial and honest about it**: WorkOS authentication events are available
  to Workspace Admins; "Amp maintains comprehensive application audit logs… These logs are collected
  internally and monitored by the Amp security team, but are not currently exposed to Workspace Admins,"
  available on request; "Audit logs are retained for a minimum of 30 days." Workspace-admin access to
  private threads "is audit-logged". (https://ampcode.com/security)
- A **Data management API** exists for "workspace analytics and thread data, enabling enterprise security
  teams to connect scanning and response workflows to Amp data". (https://ampcode.com/security)
- Usage/cost observability: `amp usage`, per-thread cost, thread export as Markdown or full JSON,
  thread search with a filter query language (`id:`, `label:`, `file:`, `parent:`, `project:`, `repo:`,
  `ref:`, `author:`, `archived:`, `snoozed:`, `pinned:`, `commit:`, `pr:`, `after:`, `before:`), and
  bookmarkable feed URLs like `/feed?time=7d&q=label:bug`. (https://ampcode.com/docs/pricing,
  https://ampcode.com/docs/threads, https://ampcode.com/docs/prompting)
- Diagnostics/bug reporting: `feedback: report bug` in the CLI palette, `amp report-bug "…"`
  (aliases `amp report`, `amp diagnostics`), Ctrl+R on a `--no-tui` runner; the report includes "thread
  state, client logs, and settings"; "Diagnostic data is deleted after 7 days."
  (https://ampcode.com/docs/support/reporting-bugs)
- **OpenTelemetry export is not documented for Amp.** Marked **absent**. The closest analogues are the
  structured stream-json output and the Data management API.
  (https://ampcode.com/docs/support/reporting-bugs, https://ampcode.com/security)
- Plugin-level logging: `amp.logger.log` / `ctx.logger.log` from plugins.
  (https://ampcode.com/docs/customize/plugins)

## 2.19 Modes history (explicitly requested: smart / rush)

- **Current modes are a "Dial": `low`, `medium`, `high`, `ultra`**, turned with `Ctrl+S` in the CLI or
  the mode picker in the web app. Choose by "how hard is this task?", not by model.
  (https://ampcode.com/docs/the-dial, https://ampcode.com/news/the-dial)
- **They replace `smart`, `deep`, `rush`, and `large`.** The announcement states verbatim: "Amp's agent
  modes are now a dial: `low`, `medium`, `high`, `ultra`. They replace `smart`, `deep`, `rush`, and
  `large`." Migration map: `smart`, `deep` → `medium` ("same model and effort as `deep`"); `rush` →
  `low`; `deep**3` → `ultra` or `high`. (https://ampcode.com/news/the-dial)
- Rationale: "The old modes were models in disguise: each name hid a model, a prompt, a reasoning effort
  — and to pick one, you had to know what that model was like this month. That world is gone."
  (https://ampcode.com/news/the-dial)
- Legacy modes are installable as plugins, which doubles as a demonstration that the mode abstraction is
  a plugin API: `amp plugins add --auto-update @amp/smart-classic` (also `@amp/deep-classic`,
  `@amp/rush-classic`, `@amp/large-classic`), then `plugins: reload`; they appear as "Smart (classic)",
  "Deep (classic)", etc., because "the original names stay reserved for the built-ins".
  (https://ampcode.com/news/the-dial)
- Custom dials: Settings → Mode Dial, gated behind the experimental **Custom Mode Dial** flag; drag 2–4
  modes into bays; personal dial overrides workspace dial; "Saving the factory lineup resets your dial to
  the default". (https://ampcode.com/docs/the-dial)
- Mode also carries a **Fast vs Standard** distinction independent of the Dial (`--fast`/`--features
  fast`, `Alt+R`). (https://ampcode.com/docs/cli/execute-mode, https://ampcode.com/docs/cli/keybindings)

---

# 3. Cross-tool deltas that matter for design

| Axis | Claude Code | Amp |
|---|---|---|
| Permission default | Prompts; OS-enforced Bash sandbox; 6 named modes via `Shift+Tab` | **No approval by default**; rule lists + plugins |
| Rule file | `CLAUDE.md` (explicitly *not* AGENTS.md; import it instead) | `AGENTS.md` (fallback `AGENT.md` **or `CLAUDE.md`**) |
| Edit primitive | Documented **exact string replacement** with read-before-edit + uniqueness gates | `edit_file`/`create_file` tool names only — **unverified** semantics |
| Parallel tool calls | Per-tool: read-only concurrent, mutating sequential | Not documented per-tool — **unverified** |
| Compaction | Auto-compaction, configurable window, `/compact <focus>`, specified survival rules | Removed → Handoff → **compaction restored & automatic at 90% full** (docs lag) |
| Undo | `/rewind` with per-prompt snapshots, 100 checkpoints, code/conversation restore split | `undo_edit` tool; **file rollback on restore explicitly removed** — "the models are good enough to undo changes for you" |
| Stated harness philosophy | Rich, layered harness: sandbox, modes, hooks, checkpoints, OTel | **"The agent is no longer the limiting factor… A simple tool called `bash` is often enough"** — deliberately thinning the harness |
| Fork | `/branch`, `--fork-session` (live) | Fork added Jul 2025, **removed** Jan 2026 |
| Plan mode | First-class permission mode (`Shift+Tab`, `/plan`) | **Absent**; prompt-level only |
| Todo tracking | `TodoWrite`/`Task*` tools, **withheld on newest models** for context cost | `todo_read`/`todo_write` tools |
| Subagent comms | Isolated context; teams/agent-view for coordination | Explicitly **cannot talk to each other**; isolated by design |
| Second-opinion model | Effort levels / model aliases (`best`, `opusplan`); advisor tool | **`oracle` as a tool**, cross-vendor in `high`/`ultra` |
| Context conditionality | `@import`, `.claude/rules/`, lazy nested CLAUDE.md | `@`-mention + **YAML `globs:` front matter** gating |
| Observability | Full OpenTelemetry metrics + events (`claude_code.*`) | Thread as audit trail + Data management API; **no documented OTEL** |
| Headless surface | `-p`, `--output-format stream-json`, SDK | `-x`, `--stream-json`, **`-ox`** to run in cloud orb |
| Protocol compat | Defines the format | **Deliberately mirrors Claude Code's stream-json** |
| Evals | No published harness/SWE-bench in docs | Publishes a philosophy page rejecting evals as oracle; **no harness** |
| Isolation model | Local OS sandbox (`bubblewrap`/seatbelt) + permissions | Remote **orb** sandbox + encrypted at rest; local CLI unsandboxed |

## Explicit gaps / things I could not verify from primary sources

- Claude Code: exact prompt-cache **TTL values** for the 5-minute/1-hour breakpoints (the page refers to
  "cache TTL"/"Cache lifetime" generically — unlike Amp, which exposes
  `ephemeral_5m_input_tokens`/`ephemeral_1h_input_tokens` in its wire format); any official **SWE-bench**
  number or first-party eval harness (none found in the docs); the precise version/changelog entry
  matching the docs' feature set; the version at which `Task` became `Agent`; whether `/output-style`
  was formally removed (its absence from `commands.md` is strong but circumstantial evidence); and the
  exact boundary of what `/rewind` can and cannot restore beyond "changes made by its file editing tools".
- Amp: `edit_file` semantics (search-replace vs diff vs AST); the **current** compaction/handoff state —
  resolved in favour of compaction by the Neo post, but the static docs still reference handoff, so the
  docs lag; exact current per-mode model routing (the `the-dial` and `tools` pages disagree with each
  other on the `high`/`medium` oracle); whether queuing or steering is the current send default (docs say
  steering, Neo says queuing); whether the skill-management CLI still exists (docs document it, Neo says
  it was removed); the status of editor extensions (docs document VS Code/Cursor/etc., while a Feb 2026
  post announced their deletion); orb network/egress policy; OTEL/trace export; SDK specifics; and whether
  any file-level checkpoint exists at all (file rollback on message edit/restore was explicitly removed).
- Amp has **no `AMP.md`** — the historical pre-`AGENTS.md` name was **`AGENT.md`** (May 2025), superseded
  by the plural form in Aug 2025.
- Version/temporal caveat for both tools: Amp's news posts carry dates into **2026** and reference models
  (GPT-5.6, Claude Fable 5.1, GLM-5.2) and pricing that are time-sensitive; Claude Code's docs reference
  v2.1.17x–v2.1.26x and model names I could not tie to a changelog entry. Any statement here gated on a
  version number should be re-checked against the live docs before being relied on.
