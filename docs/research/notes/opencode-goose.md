# opencode vs goose — design and feature notes

Research notes on two open-source coding agents, built from **primary sources only**: the projects' own
docs sites, their docs source in-repo, and their source code. Every substantive claim carries a URL from
the owning domain/GitHub repo.

- **opencode** — https://github.com/anomalyco/opencode (canonical; `github.com/sst/opencode` now redirects here). Docs: https://opencode.ai/docs/
- **goose** — https://github.com/aaif-goose/goose (canonical; `github.com/block/goose` now redirects here). Docs: https://goose-docs.ai/docs/

## 0. Source-of-truth corrections (read first)

Several URLs in the task brief are stale. Verified by HTTP request:

| Assumed | Verified reality |
| --- | --- |
| `https://github.com/sst/opencode` | **302 → https://github.com/anomalyco/opencode**; default branch is `dev` |
| `https://opencode.ai/docs/` | Still correct (HTTP 200); doc pages are `/docs/<slug>/` |
| `https://github.com/block/goose` | **302 → https://github.com/aaif-goose/goose**; default branch `main` |
| `https://block.github.io/goose/docs/` | **HTTP 404 — dead.** Docs moved to **https://goose-docs.ai/docs/** |

Evidence: the goose Docusaurus config sets `url: "https://goose-docs.ai/"` and
`organizationName: "aaif-goose"` —
https://github.com/aaif-goose/goose/blob/main/documentation/docusaurus.config.ts

Verification: `curl -L` on 2026-09-10 returned 404 for `block.github.io/goose/docs/`, 200 for
`goose-docs.ai/docs/`, and 200 after redirect for both GitHub repo URLs.

**Method note.** The two repos were cloned locally (sparse checkout: opencode
`packages/web/src/content/docs` + `packages/opencode/src`; goose `documentation/docs` + `crates`) and read
directly. Doc-site URL patterns below are the in-repo file path with the extension dropped:

- opencode docs page = `https://opencode.ai/docs/<name>/` ↔ `packages/web/src/content/docs/<name>.mdx`
- goose docs page = `https://goose-docs.ai/docs/<path>/` ↔ `documentation/docs/<path>.md`
- Source citations use `.../blob/dev/...` for opencode and `.../blob/main/...` for goose.

---

## 1. Agent loop & tool-calling protocol

### opencode

- opencode is a **provider-agnostic multi-turn tool loop** built on the Vercel AI SDK: `session/llm.ts`
  imports and calls `streamText` from `ai`.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/llm.ts
- The loop is bounded by an explicit per-agent step counter rather than a model-chosen stop:
  `const maxSteps = agent.steps ?? Infinity; const isLastStep = step >= maxSteps`.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/prompt.ts
- On the final step opencode **injects a synthetic assistant message** containing `MAX_STEPS_PROMPT`
  (imported from `@opencode-ai/core/session/runner/max-steps`) — i.e. the stop condition is *forced text
  output*, exactly as documented: "the agent receives a special system prompt instructing it to respond
  with a summarization of its work and recommended remaining tasks."
  https://opencode.ai/docs/agents/#max-steps
- **Parallel tool calls within one turn are supported** and the repo runs unbounded concurrent
  per-tool-call work: `Effect.forEach(..., { concurrency: "unbounded" })` appears both when building the
  tool registry and when awaiting in-flight tool calls at end of turn.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/processor.ts
- Tool-call streaming is explicit: the processor keeps a `toolcalls: Record<string, ToolCall>` map,
  emitting `tool-call` / partial state updates while running.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/processor.ts
- **The driving loop is a manual `while (true)`** (line 1088 of `session/prompt.ts`) that re-reads durable
  history each iteration rather than relying on the SDK's own step loop; the AI SDK call is one provider
  turn inside it. https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/prompt.ts
- The processor signals the loop with a three-valued result —
  `export type Result = "compact" | "stop" | "continue"` — returned as:
  `if (ctx.needsCompaction) return "compact"; if (ctx.blocked || ctx.assistantMessage.error) return "stop";
  return "continue"`. So "stop" is driven by *blocking or error*, **not** by the provider's finish reason:
  pending tool calls keep the turn alive even when the provider reports `stop`.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/processor.ts
- Tool-choice control exists at the LLM boundary: `toolChoice?: "auto" | "required" | "none"`, forced to
  `"required"` when a structured-output (`json_schema`) format is used.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/llm.ts
- An **opt-in native LLM runtime** (`OPENCODE_EXPERIMENTAL_NATIVE_LLM`) replaces the AI SDK path with
  `packages/opencode/src/session/llm/native-runtime.ts`, which "either returns a ready LLMEvent stream or
  a concrete fallback reason" — a runtime seam, not a different loop.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/llm.ts
- In-repo `AGENTS.md` documents an ongoing **V2 session core** rewrite that requires "one explicit
  `llm.stream(request)` call per provider turn" and forbids "delegat[ing] orchestration to an in-memory
  tool loop" — useful signal that the loop is being made durable/admission-based.
  https://github.com/anomalyco/opencode/blob/dev/AGENTS.md

### goose

- goose's canonical description: three components — **interface, agent, extensions**; the agent "runs
  goose's core logic, managing the interactive loop", extensions (MCP servers) supply tools.
  https://goose-docs.ai/docs/goose-architecture/
- The documented loop is: human request → provider chat (model emits a tool call) → goose executes the
  tool call → results returned to the model → **context revision** → repeat until the model answers.
  Same URL as above.
- **Errors are fed back as tool responses rather than aborting the loop** ("invalid JSON, missing tools,
  etc. are sent back to the model as tool responses").
  https://goose-docs.ai/docs/goose-architecture/ and
  https://goose-docs.ai/docs/goose-architecture/error-handling/
- **Parallel tool calls: yes.** Multiple tool requests from one model turn are executed concurrently and
  their output streams multiplexed via `futures::stream::select_all` over per-request streams in
  `handle_approved_and_denied_tools` / `handle_approval_tool_requests`.
  https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/agent.rs
- **Stop condition = turn budget, not model choice.** `GOOSE_MAX_TURNS` (default 1000) caps consecutive
  turns without user input; on hitting it goose stops and asks "Would you like me to continue?".
  https://goose-docs.ai/docs/guides/sessions/smart-context-management/#maximum-turns
  and https://goose-docs.ai/docs/guides/environment-variables/
- A second bound: `--max-tool-repetitions <NUMBER>` limits how many times the same tool may be called
  consecutively with identical parameters, "to help prevent infinite loops".
  https://goose-docs.ai/docs/guides/goose-cli-commands/
- **The agent loop is mid-migration.** The repo's own `AGENTS.md` states the legacy loop in
  `crates/goose/src/agents/agent.rs` is being replaced by a state machine in
  `crates/goose/src/agents/state_machine/`, gated behind `GOOSE_STATE_MACHINE=1`, and that until the
  migration completes "changes to agent-loop behavior must be implemented and tested in both paths."
  https://github.com/aaif-goose/goose/blob/main/AGENTS.md
- The state-machine directory enumerates the loop's concerns as discrete ops — useful design lens:
  `ops_llm`, `ops_maxturns`, `ops_compaction`, `ops_tool_approval`, `ops_tool_pair_compaction`,
  `ops_retry`, `ops_steer`, `ops_recipe`, `ops_skills`, `ops_slash_command`, `ops_project`,
  `ops_entry_hook`, `ops_stop_hook`, `ops_exit_on_error`, `ops_status`, `ops_doctor`.
  https://github.com/aaif-goose/goose/tree/main/crates/goose/src/agents/state_machine

---

## 2. Tool set & edit strategy

### opencode — built-in tools

Documented built-ins: `bash`, `edit`, `write`, `read`, `grep`, `glob`, `lsp` (experimental), `apply_patch`,
`skill`, `todowrite`, `webfetch`, `websearch`, `question`, plus custom tools and MCP tools.
https://opencode.ai/docs/tools/

- **Edit strategy = exact search-replace, not unified diff or AST.** `edit` takes
  `{ filePath, oldString, newString, replaceAll? }` and is described as "Modify existing files using
  exact string replacements."
  https://opencode.ai/docs/tools/#edit and
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/edit.ts
- opencode layers **ten fallback replacers** on top of the exact match, in this order:
  `SimpleReplacer, LineTrimmedReplacer, BlockAnchorReplacer, WhitespaceNormalizedReplacer,
  IndentationFlexibleReplacer, EscapeNormalizedReplacer, TrimmedBoundaryReplacer, ContextAwareReplacer,
  MultiOccurrenceReplacer`. The file header credits cline's `diff-apply` and gemini-cli's
  `editCorrector` as sources.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/edit.ts
- Two deliberate guardrails: an edit **fails** if `oldString` is not found, and fails if it matches
  multiple times unless `replaceAll: true`; plus an `isDisproportionateMatch` check that refuses a
  replacement whose matched span is far larger than `oldString` ("Re-read the file and provide the full
  exact oldString").
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/edit.txt and
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/edit.ts
- Per-file writes are serialized with an in-process `Semaphore` keyed by resolved path.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/edit.ts
- **`apply_patch` is a separate tool** using OpenAI-style marker lines embedded in `patchText`
  (`*** Add File:`, `*** Update File:`, `*** Move to:`, `*** Delete File:`) with paths relative to the
  project root — so opencode offers *both* search-replace and patch-file editing.
  https://opencode.ai/docs/tools/#apply_patch
- `edit`, `write` and `apply_patch` are **all gated by the single `edit` permission**.
  https://opencode.ai/docs/tools/#write
- **The two edit strategies are mutually exclusive and model-selected.** The tool registry filters on the
  model id: `const usePatch = input.modelID.includes("gpt-") && !input.modelID.includes("oss") &&
  !input.modelID.includes("gpt-4")`; when true, `apply_patch` is exposed and `edit`/`write` are *removed*,
  otherwise the reverse. So GPT-5-family models edit via patch files, everything else via search-replace.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/registry.ts
- `grep`/`glob` are implemented on **ripgrep**, which by default respects `.gitignore`; a project-root
  `.ignore` file with `!node_modules/` etc. re-includes ignored paths. `glob` returns paths sorted by
  modification time.
  https://opencode.ai/docs/tools/#internals
- The `lsp` tool exposes `goToDefinition`, `findReferences`, `hover`, `documentSymbol`,
  `workspaceSymbol`, `goToImplementation`, `prepareCallHierarchy`, `incomingCalls`, `outgoingCalls`, and
  is gated behind `OPENCODE_EXPERIMENTAL_LSP_TOOL=true`.
  https://opencode.ai/docs/tools/#lsp-experimental
- `websearch` is only available with the OpenCode provider or when `OPENCODE_ENABLE_EXA` /
  `OPENCODE_ENABLE_PARALLEL` is truthy; it hits a hosted MCP service with no API key.
  https://opencode.ai/docs/tools/#websearch
- `todowrite` is **disabled for subagents by default**.
  https://opencode.ai/docs/tools/#todowrite

### goose — built-in (Developer extension) tools

The Developer platform extension exposes exactly five tools — asserted by a unit test in the source
(`assert_eq!(names, vec!["write", "edit", "shell", "tree", "read_image"])`):

| Tool | Purpose | Risk (docs) |
| --- | --- | --- |
| `shell` | execute shell commands | High |
| `write` | create/overwrite files | High |
| `edit` | find-and-replace, params are **`before` / `after`** | High |
| `tree` | list directory trees with line counts | Low |
| `read_image` | read a local/remote image | Low |

- https://goose-docs.ai/docs/mcp/developer-mcp/#developer-extension-tools
- Source: https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/developer/mod.rs
- **Edit strategy = exact, unique search-replace (`edit`), no AST and no unified-diff tool.** The tool
  description in source is precise about the contract: "Edit a file by finding and replacing text. The
  `before` text must match exactly and uniquely. Use empty `after` text to delete." Parameter names are
  `before`/`after`, **not** opencode's `oldString`/`newString`.
  https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/developer/mod.rs
  - Failure modes are reported as *prompts* back to the model: zero matches → error plus a "Did you mean"
    hint and a 20-line preview; more than one match → error listing the first two matches by line number.
    Same URL. (Per the source-verification pass; **rendered failure text not re-read line by line** —
    treat the exact wording as **unverified**.)
- **There is no `glob`, `grep`, `read`, `patch`, `text_editor`, `str_replace`, `insert` or `undo_edit`
  tool.** Repo-wide searches for those tool names return zero hits; only the five tools above exist, and a
  unit test pins the list. `tree` is the directory-listing tool and search is delegated to `shell`/ripgrep
  (the architecture doc says goose will "use ripgrep to skip system files").
  https://goose-docs.ai/docs/goose-architecture/ and
  https://goose-docs.ai/docs/mcp/developer-mcp/
  - **Doc debt to be aware of:** other goose docs pages still reference the older `text_editor` tool
    (hooks, subagents, permissions pages). Those references are stale relative to the source. Same URLs.
- Note the docs' own caveat: the Developer extension enables command execution and file modification
  controlled separately through permission modes and tool permissions (§5).
  https://goose-docs.ai/docs/mcp/developer-mcp/
- Additional platform extensions in-tree (not called "tools" in the docs but registered as MCP
  extensions): `analyze`, `apps`, `chatrecall`, `code_execution`, `ext_manager`, `orchestrator`,
  `scheduler`, `summarize`, `summon`, `todo`, `tom`.
  https://github.com/aaif-goose/goose/tree/main/crates/goose/src/agents/platform_extensions
- Platform-extension tool names read from source include, in `orchestrator`: `list_sessions`,
  `view_session`, `start_agent`, `send_message`, `interrupt_agent`; in `summon`: `delegate` and `load`;
  and a `write_todo`-style tool in `todo`.
  https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/orchestrator.rs
  and https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/summon.rs
- Codebase *analysis* is deliberately split out of Developer into a separate `analyze` extension.
  https://goose-docs.ai/docs/mcp/developer-mcp/

---

## 3. Context management

### opencode

- Compaction is configured under the `compaction` key: `auto` (default `true`), `prune` (default `false`,
  removes old tool outputs), and `reserved` — "token buffer for compaction … leave enough window to avoid
  overflow during compaction".
  https://opencode.ai/docs/config/#compaction
- **The hard input budget is computed, not configured**, in `session/overflow.ts`:
  `limit.input − min(20_000, maxOutputTokens)` — i.e. a fixed 20 k-token headroom cap, or the model's
  max-output setting if smaller. Overflow is detected from **two independent sites** (a post-step check and
  a `ContextOverflowError` from the provider), each of which triggers auto-compaction.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/overflow.ts
- Auto-compaction can be disabled with `OPENCODE_DISABLE_AUTOCOMPACT`.
  https://opencode.ai/docs/cli/#environment-variables
- Compaction is implemented as a **first-class agent**: a hidden `compaction` primary agent that "compacts
  long context into a smaller summary", alongside hidden `title` and `summary` agents.
  https://opencode.ai/docs/agents/#use-compaction
- Implementation files: `session/compaction.ts`, `session/overflow.ts`, `session/summary.ts`,
  `session/reminders.ts`, and `session/prompt/` prompt fragments.
  https://github.com/anomalyco/opencode/tree/dev/packages/opencode/src/session
- **`@` file references**: the TUI's `@` key fuzzy-searches project files and inserts a reference
  (`How is authentication handled in @packages/functions/src/api/index.ts`); the headless equivalent is
  `opencode run --file/-f <path>`.
  https://opencode.ai/docs/ and https://opencode.ai/docs/cli/#run
- **Ignore rules** for search/listing come from ripgrep's `.gitignore` support plus a project `.ignore`
  file; separately `watcher.ignore` globs control the **file watcher**
  (`{"watcher": {"ignore": ["node_modules/**", "dist/**", ".git/**"]}}`).
  https://opencode.ai/docs/tools/#ignore-patterns and https://opencode.ai/docs/config/#watcher
- `OPENCODE_EXPERIMENTAL_OUTPUT_TOKEN_MAX` sets max output tokens per response.
  https://opencode.ai/docs/cli/#experimental
- There is no documented user-facing token *budget* cap; cost control is via `steps`, model choice, and
  `compaction.reserved`.

### goose

goose runs a documented **two-tier** scheme.

1. **Auto-compaction** — proactively summarizes older conversation when approaching the limit. Default
   trigger is **80 %** of the token limit in both Desktop and CLI.
   https://goose-docs.ai/docs/guides/sessions/smart-context-management/
   - Tuned by `GOOSE_AUTO_COMPACT_THRESHOLD` (float 0.0–1.0, default `0.8`; `0.0` disables).
     https://goose-docs.ai/docs/guides/environment-variables/
   - Manual equivalents: `/compact` in the CLI (`/summarize` is a deprecated alias) and "Compact now" in
     Desktop. https://goose-docs.ai/docs/guides/sessions/smart-context-management/
   - The summarization prompt is user-editable via the `compaction.md` prompt template.
     https://goose-docs.ai/docs/guides/sessions/smart-context-management/
2. **Context-limit strategies** (`GOOSE_CONTEXT_STRATEGY`) if auto-compaction is off or insufficient:
   `summarize` (Desktop + CLI), `truncate` (CLI only), `clear` (CLI only), `prompt` (CLI only). Defaults
   differ by mode: interactive → prompt; **headless `goose run` → summarize**. Same URL.

- **Tool-output summarization** is separate and backgrounded: older tool call outputs are summarized while
  recent ones stay full-detail, tuned by `GOOSE_TOOL_CALL_CUTOFF`.
  https://goose-docs.ai/docs/guides/sessions/smart-context-management/ and
  https://goose-docs.ai/docs/guides/environment-variables/
- Other token-shaping knobs: `GOOSE_MAX_TOKENS` (max tokens per model response), `GOOSE_MOIM_MESSAGE_TEXT`
  / `GOOSE_MOIM_MESSAGE_FILE` (persistent "working memory" injected **every turn**, file capped at 64 KB),
  `GOOSE_MAX_TOOL_RESPONSE_SIZE` (default 200 000 chars, larger responses spooled to a temp file).
  https://goose-docs.ai/docs/guides/environment-variables/
- Context-limit *display* override: `GOOSE_CONTEXT_LIMIT` (resolution order: env var → model metadata …
  → global default 128 000 tokens); `GOOSE_INPUT_LIMIT` maps to Ollama's `num_ctx`.
  https://goose-docs.ai/docs/guides/sessions/smart-context-management/ and
  https://goose-docs.ai/docs/guides/environment-variables/
- Design rationale in the architecture doc: "goose summarizes with faster and smaller LLMs", "goose
  includes everything versus a semantic search", "uses algorithms to delete old or irrelevant content",
  and "will use find and replace instead of rewriting large files, use ripgrep to skip system files, and
  summarize verbose command outputs."
  https://goose-docs.ai/docs/goose-architecture/

---

## 4. System prompt & project rule files

### opencode

- Project rules live in `AGENTS.md` at the project root; global rules in `~/.config/opencode/AGENTS.md`.
  `/init` generates a project `AGENTS.md`.
  https://opencode.ai/docs/ and https://opencode.ai/docs/rules/#types
- **Precedence** (startup order, first match wins per category):
  1. local files by traversing up from cwd (`AGENTS.md`, else `CLAUDE.md`)
  2. global `~/.config/opencode/AGENTS.md`
  3. `~/.claude/CLAUDE.md` unless disabled
  https://opencode.ai/docs/rules/#precedence
- Claude Code compatibility is an explicit fallback layer (`CLAUDE.md`, `~/.claude/CLAUDE.md`,
  `~/.claude/skills/`), individually switchable with `OPENCODE_DISABLE_CLAUDE_CODE`,
  `OPENCODE_DISABLE_CLAUDE_CODE_PROMPT`, `OPENCODE_DISABLE_CLAUDE_CODE_SKILLS`.
  https://opencode.ai/docs/rules/#claude-code-compatibility and
  https://opencode.ai/docs/cli/#environment-variables
- Extra instruction files, including **remote URLs**, via the `instructions` array:
  `"instructions": ["CONTRIBUTING.md", "docs/guidelines.md", ".cursor/rules/*.md", "https://…/style.md"]`.
  Remote instructions are fetched with a 5-second timeout; all instruction files are combined with the
  `AGENTS.md` files.
  https://opencode.ai/docs/rules/#custom-instructions
- opencode does **not** parse file references inside `AGENTS.md`; the recommended equivalent is the
  `instructions` field. https://opencode.ai/docs/rules/#referencing-external-files
- Per-agent system prompts are set with `prompt`, supporting `{file:./prompts/code-review.txt}` paths
  relative to the config file. https://opencode.ai/docs/agents/#prompt
- System-prompt composition lives in `session/system.ts`, with dynamic reminders applied per step via
  `session/reminders.ts` (visible in the loop as `SessionReminders.apply`).
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/prompt.ts
- opencode's own repo demonstrates the convention: a root `AGENTS.md` with build/test/style rules.
  https://github.com/anomalyco/opencode/blob/dev/AGENTS.md

### goose

- Two context files by default: **`AGENTS.md`** and **`.goosehints`**, loaded at **every directory level**
  ("goose looks for both `AGENTS.md` and `.goosehints` at each level"), with nested loading.
  https://goose-docs.ai/docs/guides/context-engineering/using-goosehints/
- **There is no `.gooseignore`** — repo-wide search finds no such file or feature; ignore behaviour comes
  from the Rust `ignore` crate (gitignore semantics) rather than a goose-specific ignore file. Marked
  **unverified** as a definitive negative, but no positive evidence exists.
  https://github.com/aaif-goose/goose/blob/main/.gitignore
- goose's own repository ships a root **`.goosehints`** (alongside `AGENTS.md`), so the convention is
  dogfooded. https://github.com/aaif-goose/goose/blob/main/.goosehints
- Global hints live at `~/.config/goose/.goosehints`; local hints apply per directory hierarchy. When both
  exist goose considers both, and **local wins on conflict**.
  https://goose-docs.ai/docs/guides/context-engineering/using-goosehints/
- The filename list is configurable: `CONTEXT_FILE_NAMES`, a JSON array, "default is
  `["AGENTS.md", ".goosehints"]`".
  https://goose-docs.ai/docs/guides/context-engineering/using-goosehints/#custom-context-files
  - ⚠️ **Doc inconsistency (verified):** the env-var reference table lists the default as
    `[".goosehints", "AGENTS.md"]` — the reverse order. Treat load order between the two as **unverified**.
    https://goose-docs.ai/docs/guides/environment-variables/
- Reading `.goosehints` requires the **Developer extension** to be enabled.
  https://goose-docs.ai/docs/guides/context-engineering/using-goosehints/
- Prompt templates are user-customizable (e.g. `compaction.md`), and goose supports custom agents and
  per-agent instructions.
  https://goose-docs.ai/docs/guides/context-engineering/prompt-templates/ and
  https://goose-docs.ai/docs/guides/context-engineering/custom-agents/
- Persistent cross-turn instructions have a dedicated channel: "working memory" injected every turn via
  `GOOSE_MOIM_MESSAGE_TEXT` or `GOOSE_MOIM_MESSAGE_FILE` (≤ 64 KB).
  https://goose-docs.ai/docs/guides/context-engineering/using-persistent-instructions/
- goose's own repo uses `AGENTS.md` (+ a `CLAUDE.md` that is just `@AGENTS.md`), and the docs directory
  carries a second `AGENTS.md` with a brand rule ("goose" always lowercase).
  https://github.com/aaif-goose/goose/blob/main/AGENTS.md

---

## 5. Permissions & safety

### opencode

- The `permission` config replaces the deprecated `tools` boolean config (deprecated as of `v1.1.1`).
  Every rule resolves to `"allow"` (run), `"ask"` (prompt) or `"deny"` (block).
  https://opencode.ai/docs/permissions/
- **Auto mode**: `opencode --auto` (also `opencode run --auto`) auto-approves anything not explicitly
  denied; the TUI exposes "Enable/Disable auto-approve permissions" in the command palette and shows a
  muted `auto` indicator. Explicit `deny` still wins.
  https://opencode.ai/docs/permissions/#auto-mode
- **Granular object syntax** with wildcards, evaluated **last matching rule wins**:
  ```json
  {"permission": {"bash": {"*": "ask", "git *": "allow", "rm *": "deny"},
                  "edit": {"*": "deny", "packages/web/src/content/docs/*.mdx": "allow"}}}
  ```
  Patterns: `*` = zero+ chars, `?` = exactly one char; `~`/`$HOME` expand at the start of a pattern.
  https://opencode.ai/docs/permissions/#granular-rules-object-syntax
- Permission keys: `read`, `edit` (covers `edit` + `write` + `apply_patch`), `glob`, `grep`, `bash`
  (matches parsed commands like `git status --porcelain`), `task` (subagent type), `skill`, `lsp`,
  `question`, `webfetch` (URL), `websearch` (query), `external_directory`, `doom_loop`.
  https://opencode.ai/docs/permissions/#available-permissions
- **Defaults are permissive**: most permissions `"allow"`; `doom_loop` and `external_directory` default to
  `"ask"`; `read` is allow **except `.env`** — `*.env` and `*.env.*` are denied, `*.env.example` allowed.
  https://opencode.ai/docs/permissions/#defaults
  - ⚠️ **Code/doc tension (verified).** The evaluator's fallback when no rule matches is `action: "ask"`
    (`ruleset.findLast(...) ?? { action: "ask" }`), and the shipped `.env` default in code is `ask`, while
    the docs page publishes `deny` for `*.env`. Treat "the effective default is *allow*" as
    docs-sourced, and the `.env` value as **docs/code mismatch**.
    https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/permission/index.ts
  - Rule resolution is **last-match-wins implemented as `findLast`**, not a merge or a first-match scan.
    Same URL.
- **`bash` patterns are derived from parsed commands, not raw strings.** opencode tree-sitter-parses the
  command line and consults an **arity dictionary** to decide how many tokens form the "command", so
  `git status --porcelain` can be matched while subcommand-aware rules stay meaningful (e.g. `"npm run": 3`
  covers `npm run dev`). The default bash timeout is 2 minutes.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/permission/arity.ts and
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/permission/evaluate.ts
- **Path allowlist**: `external_directory` governs any tool touching paths outside the working directory
  (`read`, `edit`, `glob`, `grep`, and many bash commands), e.g.
  `{"external_directory": {"~/projects/personal/**": "allow"}}`. Home expansion does *not* make an
  external path part of the workspace.
  https://opencode.ai/docs/permissions/#external-directories
- **Dangerous-command / loop guard**: `doom_loop` fires "when the same tool call repeats 3 times with
  identical input" (default `ask`). https://opencode.ai/docs/permissions/#available-permissions
- **What "ask" offers**: `once` (this request), `always` (future requests matching the suggested
  patterns, for the rest of the session), `reject`. The `always` pattern set is supplied by the tool —
  bash approvals typically whitelist a safe command prefix like `git status*`.
  https://opencode.ai/docs/permissions/#what-ask-does
- Agent-level permissions merge with global config and **agent rules take precedence**; can be set in JSON
  or in agent Markdown frontmatter (`permission: {edit: deny, bash: ask, webfetch: deny}`).
  https://opencode.ai/docs/permissions/#agents
- **Plan mode** is enforced by permissions: the built-in `plan` primary agent sets `file edits` (all
  writes/patches/edits) and `bash` to `ask` by default.
  https://opencode.ai/docs/agents/#use-plan
- Separate from permissions, **Policies** (experimental, `experimental.policies`) gate whether a
  *resource* may be used at all — currently the single action `provider.use`, e.g. deny the `openai`
  provider, which then removes it from model selection even if credentials exist.
  https://opencode.ai/docs/policies/
- Headless/CI can hard-pin safety via env: opencode's own GitHub workflow runs with
  `OPENCODE_PERMISSION: '{"bash": "deny"}'`.
  https://github.com/anomalyco/opencode/blob/dev/.github/workflows/opencode.yml

### goose

- **Four permission modes** (docs name them; CLI values in parentheses):
  | Mode | Behaviour |
  | --- | --- |
  | Completely Autonomous (`auto`) | modify/use/delete with no approval — **the default** |
  | Manual Approval (`approve`) | asks before any tool/extension use; supports granular tool permissions |
  | Smart Approval (`smart_approve`) | risk-based: auto-approves low-risk, flags the rest |
  | Chat Only (`chat`) | no extension use or file modification at all |
  https://goose-docs.ai/docs/guides/managing-tools/goose-permissions/ and
  https://goose-docs.ai/docs/mcp/developer-mcp/
- Set with `GOOSE_MODE` (values `"auto"`, `"approve"`, `"chat"`, `"smart_approve"`; default `"auto"`), via
  `goose configure` → goose settings → goose mode, or in-session with `/mode auto|approve|smart_approve|chat`.
  https://goose-docs.ai/docs/guides/environment-variables/ and
  https://goose-docs.ai/docs/guides/managing-tools/goose-permissions/
- In manual/smart modes goose asks only for tools it deems **write** tools ("text editor write", "text
  editor edit", `bash - rm, cp, mv`); "Read/write approval makes best effort attempt at classifying read
  or write tools. This is interpreted by your LLM provider."
  https://goose-docs.ai/docs/guides/managing-tools/goose-permissions/
- **Granular tool permissions** (`Always allow` / `Ask before` / `Never allow`) apply per extension tool in
  Manual or Smart Approval modes. https://goose-docs.ai/docs/guides/managing-tools/tool-permissions/
- **Granular rules live in `permission.yaml`**, not in `config.yaml`. The struct fields are
  `always_allow: Vec<String>`, `ask_before: Vec<String>`, `never_allow: Vec<String>`, and entries are
  keyed by tool name with principals distinguishing the **`user`** from the **`smart_approve`** judge — so
  the LLM risk classifier can hold different rules than the human.
  https://github.com/aaif-goose/goose/blob/main/crates/goose/src/config/permission.rs
- The smart-approval classifier itself is an **injection-aware read-only judge**: "Read/write approval makes
  best effort attempt at classifying read or write tools. This is interpreted by your LLM provider."
  https://goose-docs.ai/docs/guides/managing-tools/goose-permissions/
- **Extension allowlist**: `GOOSE_ALLOWLIST` points at a URL listing permitted extensions.
  https://goose-docs.ai/docs/guides/environment-variables/ and https://goose-docs.ai/docs/guides/allowlist/
- **Security extras** beyond modes: an `adversary mode`, prompt-injection detection, and a
  classification API, all under the security guide.
  https://goose-docs.ai/docs/guides/security/ ,
  https://goose-docs.ai/docs/guides/security/adversary-mode/ ,
  https://goose-docs.ai/docs/guides/security/prompt-injection-detection/ and
  https://goose-docs.ai/docs/guides/security/classification-api-spec/
- **Hooks can block tool calls** (see §9): a `PreToolUse` hook exiting `2` or printing
  `{"decision":"block"}` denies the call; `on_failure: block` makes hook *failures* deny by default.
  https://goose-docs.ai/docs/guides/context-engineering/hooks/
- Host isolation is available via `goose session --container <container_id>` / `goose run --container`,
  which runs extensions inside a Docker container.
  https://goose-docs.ai/docs/guides/goose-cli-commands/ and
  https://goose-docs.ai/docs/tutorials/goose-in-docker/
- **No path allowlist equivalent to opencode's `external_directory` was found in the docs** — access
  control is mode/permission-based, plus the extension allowlist. Marked **unverified** as a hard
  negative; failure to find is not proof of absence.

---

## 6. Session persistence, resume, fork, sharing

### opencode

- Sessions are stored on disk under `~/.local/share/opencode/` — in-project sessions under
  `./<project-slug>/storage/` when inside a git repo, otherwise `./global/storage/`.
  https://opencode.ai/docs/troubleshooting/
- Resume/fork flags exist on **both** the TUI and `run`: `--continue`/`-c`, `--session`/`-s`, and
  `--fork` ("Fork the session when continuing (use with `--continue` or `--session`)").
  https://opencode.ai/docs/cli/
- Session management CLI: `opencode session list [--max-count/-n N] [--format table|json]`,
  `opencode session delete <sessionID>`, `opencode export [sessionID] [--sanitize]`,
  `opencode import <file|share-url>`, and `opencode db [query]` with `opencode db path`.
  https://opencode.ai/docs/cli/
- **Sharing**: three modes via the `share` config key — `"manual"` (default, `/share` generates and copies
  a link), `"auto"` (share all new conversations), `"disabled"`. Shared conversations sync to
  opencode's servers and are public at `opncd.ai/s/<share-id>`; `OPENCODE_AUTO_SHARE` is the env
  equivalent. `/unshare` revokes.
  https://opencode.ai/docs/share/ and https://opencode.ai/docs/config/#sharing
- Sharing is disabled from the enterprise side too — there is a dedicated enterprise docs page.
  https://opencode.ai/docs/enterprise/

### goose

- **Storage migrated to SQLite.** "Starting with version 1.10.0, goose uses a SQLite database
  (`sessions.db`) instead of individual `.jsonl` files", auto-importing old sessions; legacy `.jsonl`
  remains on disk but is unmanaged. Path: `~/.local/share/goose/sessions/sessions.db`.
  https://goose-docs.ai/docs/guides/goose-cli-commands/ and
  https://goose-docs.ai/docs/guides/sessions/session-management/
- `goose session` options: `--session-id <id>`, `-n/--name`, `--path` (legacy), `-r/--resume`, `--history`,
  `--container`, `--debug`, `--max-tool-repetitions`, `--max-turns`.
  https://goose-docs.ai/docs/guides/goose-cli-commands/
- **Fork exists**: `--fork` creates "a new duplicate session with copied history", and **must** be used
  with `--resume`; `--name`/`--session-id` selects which session to fork, otherwise the most recent.
  https://goose-docs.ai/docs/guides/goose-cli-commands/
- **Conversation editing** is a distinctive feature: `--edit` opens the session in `$VISUAL`/`$EDITOR`/`vi`
  **as YAML** so you can "Edit, trim, or rewrite messages, then save and close to continue the session with
  the edited conversation"; combinable with `--fork` to branch from the edited result.
  https://goose-docs.ai/docs/guides/goose-cli-commands/
- Lifecycle: `goose session list [--format text|json] [--ascending] [-w working_dir] [-l limit]`,
  `goose session remove [--session-id|-n|-r regex|--path]`, and `goose run --resume/-r` to resume a run.
  https://goose-docs.ai/docs/guides/goose-cli-commands/
- Some session behaviour is declared environment-driven: `GOOSE_DISABLE_SESSION_NAMING` disables the
  background model call that auto-names sessions.
  https://goose-docs.ai/docs/guides/environment-variables/
- **Sharing has two distinct mechanisms, and one is unusual for this category:**
  - *Recipes*: `goose recipe deeplink <name> [-p k=v]` generates a shareable link
    (`goose recipe open` resolves one back into Desktop).
    https://goose-docs.ai/docs/guides/goose-cli-commands/
  - *Sessions over **Nostr***: `crates/goose/src/session/nostr_share.rs` defines
    `pub const EVENT_KIND: u16 = 30278` and reads relay configuration from the config key
    **`GOOSE_NOSTR_RELAYS`**; the source's own test is named
    `publish_builds_deeplink_and_encrypted_kind_30278_event`, i.e. the shared event is encrypted and the
    deeplink is derived from it.
    https://github.com/aaif-goose/goose/blob/main/crates/goose/src/session/nostr_share.rs
- **Session import is format-aware for other agents' transcripts**: `crates/goose/src/session/import_formats/`
  contains `claude_code.rs`, `codex.rs`, `pi.rs` — so Claude Code / Codex / Pi session logs can be imported
  into goose's session store.
  https://github.com/aaif-goose/goose/tree/main/crates/goose/src/session/import_formats
- The SQLite store keeps a **`parent_session_id`** column, which is how forked and subagent sessions are
  linked, alongside `goose_mode`, `model_config_json` and `recipe_json`.
  https://github.com/aaif-goose/goose/blob/main/crates/goose/src/session/session_manager.rs

---

## 7. Plan mode / todo lists / goal tracking

### opencode

- **Plan mode** is a built-in primary agent toggled with **Tab** (or the `switch_agent` keybind); it
  disables modification by setting file edits and bash to `ask`, and shows an indicator in the lower-right.
  Documented workflow: Tab → describe → iterate on the plan → Tab back to Build → "go ahead".
  https://opencode.ai/docs/ and https://opencode.ai/docs/agents/#use-plan
- Plan mode appears to be flag-gated: `OPENCODE_EXPERIMENTAL_PLAN_MODE`. There are dedicated
  `tool/plan.ts` and prompt fragments `tool/plan-enter.txt` / `tool/plan-exit.txt` in the source.
  https://opencode.ai/docs/cli/#experimental and
  https://github.com/anomalyco/opencode/tree/dev/packages/opencode/src/tool
- **Todo lists**: a `todowrite` tool creates/updates task lists "to track progress during complex
  operations", with `todo.updated` plugin events. Disabled for subagents by default.
  https://opencode.ai/docs/tools/#todowrite and https://opencode.ai/docs/plugins/#events
- There is no separate "goal tracking" concept; `steps` + todos + the final-step summarization prompt are
  the closest analogues. https://opencode.ai/docs/agents/#max-steps

### goose

- **Plan mode is a CLI prompt-completion mode, not a sandbox.** `/plan <message_text>` enters plan mode
  ("Create a plan based on the current messages and ask user if they want to act on it") and `/endplan`
  exits. Plan mode is interactive and "asks clarifying questions to understand your project before
  creating a plan".
  https://goose-docs.ai/docs/guides/goose-cli-commands/ and
  https://goose-docs.ai/docs/guides/context-engineering/creating-plans/
- Plan mode can use a **different provider and model** from the executing agent:
  `GOOSE_PLANNER_PROVIDER` and `GOOSE_PLANNER_MODEL` (both fall back to `GOOSE_PROVIDER`/`GOOSE_MODEL`).
  https://goose-docs.ai/docs/guides/environment-variables/ and
  https://goose-docs.ai/docs/guides/context-engineering/creating-plans/
- Recipes provide a stronger "plan" primitive via `settings.max_turns`, retry/success-check blocks and
  `instructions` — see §9. https://goose-docs.ai/docs/guides/recipes/recipe-reference/
- **Todo tool**: confirmed in source as **`todo_write`** (single `content` parameter), capped by
  **`GOOSE_TODO_MAX_CHARS` (default 50000)**; todos persist in the session's `extension_data` and are
  re-injected into context through MOIM (working memory).
  https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/todo.rs
  - It is **not described on the user-facing docs pages found** — reachable in practice but undocumented.
    Marked **partially documented**.
- **Goal tracking exists and is a hidden-nudge mechanism, not a planner.** `/goal <description>` sets "a
  goal the agent must satisfy before finishing" (clear with `/goal off`), and `/grind <description>` sets a
  goal "the agent pursues relentlessly until max_turns" (clear with `/grind off`). Both are implemented in
  the CLI's execute-commands handler and work by injecting continuation nudges into the loop; the goal text
  is retrievable ("Current goal: …" / "No goal set. Use `/goal <description>` to set one.").
  https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/execute_commands.rs
- **No goal-tracking equivalent in opencode** — grep for `goal` across its `src/` finds only prompt-template
  prose, no subsystem.

---

## 8. Subagents & parallel orchestration

### opencode

- Two agent kinds: **primary** (Tab-switchable, own the main conversation) and **subagent** (invoked
  automatically by a primary agent, or manually by `@`-mention, e.g. `@general help me search`).
  https://opencode.ai/docs/agents/#types
- Built-ins: primary `build` (default, all tools) and `plan` (restricted); subagents `general` (full tools
  except todo, "Use this to run multiple units of work in parallel"), `explore` (fast read-only), `scout`
  (read-only external docs/dependency research); hidden system agents `compaction`, `title`, `summary`.
  https://opencode.ai/docs/agents/#built-in
- **Subagents do *not* inherit the parent's full ruleset — only its denies and its `external_directory`
  rules** ("Parent agent restrictions only govern that agent; the subagent's own permissions determine its
  capabilities"), and `todowrite` + `task` are force-denied unless the subagent's own ruleset already
  mentions them. This is exactly why nested fan-out is hard to do accidentally.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/agent/subagent-permissions.ts
- **Nesting is explicitly bounded**: `subagent_depth` default `1` (primary → subagent only), `2` allows one
  more level, `0` disables subagent launches entirely.
  https://opencode.ai/docs/config/#subagent-depth
- **Model tiering per agent**: `agent.<name>.model` overrides the model; subagents with no model configured
  inherit the invoking primary agent's model.
  https://opencode.ai/docs/agents/#model
- Permissions per agent (including `task` permission matched against the subagent type) and per-agent
  MCP/tool enablement.
  https://opencode.ai/docs/permissions/#agents and https://opencode.ai/docs/mcp-servers/#per-agent
- Background subagent tasks are an experimental flag: `OPENCODE_EXPERIMENTAL_BACKGROUND_SUBAGENTS`;
  a background job subsystem exists in source (`src/background/job.ts`).
  https://opencode.ai/docs/cli/#experimental and
  https://github.com/anomalyco/opencode/tree/dev/packages/opencode/src/background
- The task tool implementation is `tool/task.ts` with prompt fragment `tool/task.txt`.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/task.ts

### goose

- Subagents are **natural-language driven**: "ask goose to delegate tasks using natural language. goose
  automatically decides when to spawn subagents and handles their lifecycle."
  https://goose-docs.ai/docs/guides/context-engineering/subagents/
- Parallelism is triggered by wording: the docs state that "parallel", "simultaneously", "at the same
  time", "concurrently" cause tasks to execute simultaneously rather than sequentially.
  https://goose-docs.ai/docs/guides/context-engineering/subagents/
- Implementation: the **`summon` platform extension** provides the `delegate` and `load` tools and is
  enabled by default. Its tool description in source is unusually explicit about orchestration discipline:
  "Delegates know only instructions + source content"; "Delegates cannot coordinate. Same-file work =
  conflicts."; "Parallel: async: true, then load(taskId) to wait and get results. Single: sync.";
  "Research (read-only): parallelize freely … Work (writes): partition files strictly — no two delegates
  touch the same file."; "Decompose → async delegates → load(taskId) for each → synthesize."
  https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/summon.rs
- `load(source:"task_id", peek:true)` checks progress without blocking; `load(source:"task_id",
  cancel:true)` cancels. Same URL.
- Recipes with `sub_recipes` get `summon` auto-injected — but **a recipe with an explicit `extensions`
  block must list `summon` itself** or `delegate` is unavailable.
  https://goose-docs.ai/docs/guides/context-engineering/subagents/
- Budgets and concurrency: `GOOSE_SUBAGENT_MAX_TURNS` (default **25**), `GOOSE_MAX_BACKGROUND_TASKS`
  (default **5**); subagent failures/timeouts default to a **5-minute** timeout and in parallel runs
  "if any subagent fails, you get results only from the successful ones".
  https://goose-docs.ai/docs/guides/environment-variables/ and
  https://goose-docs.ai/docs/guides/context-engineering/subagents/
- A separate **`orchestrator`** platform extension exposes multi-agent control tools to the model:
  `list_sessions`, `view_session` (modes `first_last` / `summarize`), `start_agent` (new session with own
  working dir, inherits provider+model, returns a `session_id`), `send_message` (errors if the target agent
  is busy), `interrupt_agent`.
  https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/orchestrator.rs
- Subagent sessions are **first-class persisted sessions** (`SessionType::SubAgent`, linked by
  `parent_session_id`), not ephemeral in-memory contexts.
  https://github.com/aaif-goose/goose/blob/main/crates/goose/src/session/session_manager.rs
- **Permission mode and ACP mode are mapped, not independent.** Verified in source:
  `("plan", vec![GooseMode::Chat])`, `("default", vec![GooseMode::Approve])`,
  `("auto_edit", vec![GooseMode::SmartApprove])`, and `reject_all_tools = goose_mode == GooseMode::Chat`.
  So selecting "plan" in an ACP client downgrades goose to **Chat** — tools off — which is how goose gets a
  plan-only mode without a dedicated plan sandbox.
  https://github.com/aaif-goose/goose/blob/main/crates/goose/src/acp/provider.rs
- **Reported but not independently verified:** that `delegate`/`summon` subagents are unavailable in
  `approve` / `smart_approve` / `chat` modes. Treated as **unverified** here.
- `goose review` ships a **Rust-driven parallel orchestrator** for code review with per-check subagents,
  `.agents/checks/*.md` discovery and `--no-orchestrate` to fall back to a single-prompt path.
  https://goose-docs.ai/docs/guides/goose-cli-commands/

---

## 9. Extensibility (MCP, plugins, hooks, recipes, custom commands)

### opencode

- **MCP**: local servers (`"type": "local"`, `command: [...]`, `cwd`, `environment`, `enabled`,
  `timeout` default 5000 ms) and remote (`"type": "remote"`, `url`, `headers`, `enabled`), with OAuth
  (automatic and pre-registered), `opencode mcp add|list|auth|logout|debug <name>`, and per-agent enabling
  by disabling globally and re-enabling in the agent's `tools` block.
  https://opencode.ai/docs/mcp-servers/
  - MCP tool names are prefixed with the server name, so `"mymcpservername_*"` toggles a whole server;
    wildcards `*`/`?` work.
    https://opencode.ai/docs/mcp-servers/#glob-patterns
- **Plugins & hooks**: plugins are TS/JS files in `.opencode/plugins/` or `~/.config/opencode/plugins/`, or
  npm packages listed in the `plugin` config array, installed with `opencode plugin <module> [-g] [-f]`
  (alias `opencode plug`). `--pure` runs without external plugins;
  `OPENCODE_DISABLE_DEFAULT_PLUGINS` disables defaults.
  https://opencode.ai/docs/plugins/ and https://opencode.ai/docs/cli/
  - **Documented events**: `command.executed`; `file.edited`, `file.watcher.updated`;
    `installation.updated`; `lsp.client.diagnostics`, `lsp.updated`; `message.part.removed|updated`,
    `message.removed`, `message.updated`; `permission.asked`, `permission.replied`; `server.connected`;
    `session.created|compacted|deleted|diff|error|idle|status|updated`; `todo.updated`; `shell.env`;
    `tool.execute.before`, `tool.execute.after`; `tui.prompt.append`, `tui.command.execute`,
    `tui.toast.show`.
    https://opencode.ai/docs/plugins/#events
  - There is a dedicated "Custom Context" plugin section and documented compaction hooks.
    https://opencode.ai/docs/plugins/
- **Custom commands**: Markdown files in `.opencode/command/` (and global), or JSON `command.<name>`.
  Frontmatter `options`: `template`, `description`, `agent`, `subtask`, `model`. Prompt config supports
  `$ARGUMENTS` and positional `$1..$N`, `!`shell`` output injection, and `@file` references.
  https://opencode.ai/docs/commands/ and https://opencode.ai/docs/config/#commands
  - In source, shell injections inside a command template are executed **concurrently**
    (`Promise.all(shellMatches.map(...))`) and then substituted, and positional placeholders are
    regex-replaced with the last placeholder absorbing the remaining arguments.
    https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/prompt.ts
- **Custom tools**: TS files in `.opencode/tools/` or `~/.config/opencode/tools/` using
  `tool()` from `@opencode-ai/plugin`; filename becomes the tool name, and multiple exports in one file
  become `<filename>_<exportname>`. Name collisions with built-ins are documented.
  https://opencode.ai/docs/custom-tools/
- **Agent Skills**: `SKILL.md` with `name`+`description` frontmatter, discovered in six locations
  (`<project>/.opencode/skills/`, `~/.config/opencode/skills/`, `.claude/skills/`, `~/.claude/skills/`,
  `.agents/skills/`, `~/.agents/skills/`), loaded on demand through the `skill` tool; project-local
  discovery walks up to the git worktree.
  https://opencode.ai/docs/skills/
- **ACP** support as a server (`opencode acp`, nd-JSON over stdin/stdout) and an **SDK**
  (`@opencode-ai/sdk`) plus a documented HTTP **Server** API.
  https://opencode.ai/docs/acp/ , https://opencode.ai/docs/sdk/ , https://opencode.ai/docs/server/

### goose

- **Everything is an MCP extension.** "goose utilizes MCP to connect to MCP systems/servers. In goose,
  these systems/servers are referred to as extensions." Built-in extensions ship in-tree; external
  extensions are MCP servers; custom extensions are MCP servers too.
  https://goose-docs.ai/docs/goose-architecture/ ,
  https://goose-docs.ai/docs/getting-started/using-extensions/ ,
  https://goose-docs.ai/docs/tutorials/custom-extensions/
- The extension contract is a Rust trait:
  ```rust
  pub trait Extension: Send + Sync {
      fn name(&self) -> &str;
      fn description(&self) -> &str;
      fn instructions(&self) -> &str;
      fn tools(&self) -> &[Tool];
      async fn status(&self) -> AnyhowResult<HashMap<String, Value>>;
      async fn call_tool(&self, tool_name: &str, parameters: HashMap<String, Value>) -> ToolResult<Value>;
  }
  ```
  with `ErrorData` for tool errors ("the errors become prompts") and `anyhow::Error` for extension status.
  https://goose-docs.ai/docs/goose-architecture/extensions-design/
- Extension transport types reachable from the CLI: stdio (`--with-extension`), Streamable HTTP
  (`--with-streamable-http-extension`), and builtins (`--with-builtin developer`). Stdio extension naming
  controls the tool prefix: `--with-extension "memory:npx -y @modelcontextprotocol/server-memory"` exposes
  `memory__search`; unnamed extensions are named after the launcher, and colliding launchers get their full
  command line instead — "That name prefixes every tool the server exposes (`npx__search`)".
  https://goose-docs.ai/docs/guides/goose-cli-commands/
- **Recipes** are the flagship extension point: YAML with a documented schema.
  Core required fields `title`, `description`, and (`instructions` and/or `prompt`); `prompt` is required in
  headless mode. Also `version` (default `1.0.0`).
  https://goose-docs.ai/docs/guides/recipes/recipe-reference/
  - `activities` (list), `extensions` (with `type`, `name`, `cmd`, `args`, `env_keys`, `timeout`,
    `bundled`, `description`, **`available_tools`** to restrict which tools of an extension are exposed),
    `parameters` (`key`, `input_type` ∈ string|number|boolean|date|file|select, `requirement` ∈
    required|optional|user_prompt, `description`, `default`, `options`), `response` (`json_schema` for
    output validation), `retry` (`max_retries`, `checks` with `type: shell` + `command` that must exit 0,
    `timeout_seconds` default 300, `on_failure_timeout_seconds` default 600, `on_failure`),
    `settings` (`goose_provider`, `goose_model`, `temperature`, `max_turns`), and `sub_recipes`
    (`name`, `path`, `values`, **`sequential_when_repeated`**, `description`).
    https://goose-docs.ai/docs/guides/recipes/recipe-reference/
  - Templating with `{{ param }}`, an `indent()` filter for multi-line values, template inheritance, and a
    built-in `recipe_dir` parameter ("Automatically set to the directory containing the recipe file").
    https://goose-docs.ai/docs/guides/recipes/recipe-reference/
  - Recipes are discoverable from local dirs and GitHub repos via `GOOSE_RECIPE_PATH` and
    `GOOSE_RECIPE_GITHUB_REPO`; `goose recipe list|validate|open|deeplink` manage them.
    https://goose-docs.ai/docs/guides/recipes/recipe-reference/ and
    https://goose-docs.ai/docs/guides/goose-cli-commands/
- **Hooks** (lifecycle interception, distinct from plugins):
  - Events: `SessionStart`, `SessionEnd`, `Stop`, `UserPromptSubmit`, `PreToolUse`, `PreToolUseResult`,
    `PostToolUse`, `PostToolUseFailure`, `BeforeReadFile`, `AfterFileEdit`, `BeforeShellExecution`,
    `AfterShellExecution`.
    https://goose-docs.ai/docs/guides/context-engineering/hooks/
  - Config keys: `matcher` (a **regex, not a glob**, tested against e.g. the tool name), `hooks`,
    `type` (only `command` today), `command` (run via `sh -c`), `timeout` (default **30 s**),
    `on_failure` (`allow` default / `block`) for `PreToolUse`.
    https://goose-docs.ai/docs/guides/context-engineering/hooks/
  - Payload fields include `session_id`, `matcher_context`, `tool_name`, `tool_input`,
    `last_assistant_message`, `working_dir`, `tool_call_id`, `decision`, `policy_evaluated`, `blocked_by`,
    `reason`, `cause` (`policy_denial` | `hook_failure`).
    https://goose-docs.ai/docs/guides/context-engineering/hooks/
- **Plugins** are git-backed packages installed with `goose plugin install <git-url> [--auto-update]` /
  `goose plugin update <name>`, stored under `~/.agents/plugins/<plugin-name>/`, providing skills and hooks.
  https://goose-docs.ai/docs/guides/goose-cli-commands/ and
  https://goose-docs.ai/docs/guides/context-engineering/plugins/
- **Skills**: `goose skills list` lists installed/discoverable skills "including token counts and source
  locations"; `/skills [<name>...]` lists or loads them in-session.
  https://goose-docs.ai/docs/guides/goose-cli-commands/ and
  https://goose-docs.ai/docs/guides/context-engineering/using-skills/
- **Slash commands** in-session: `/?`, `/builtin`, `/clear`, `/compact`, `/endplan`, `/exit`, `/extension`,
  `/mode`, `/plan`, `/prompt`, `/prompts`, `/r`, `/skills`, `/t`. Custom slash commands can run recipes.
  https://goose-docs.ai/docs/guides/goose-cli-commands/ and
  https://goose-docs.ai/docs/guides/context-engineering/slash-commands/
- **Distribution-level extensibility**: custom distributions and a custom-context/wrapper path.
  https://goose-docs.ai/docs/guides/custom-distributions/
- **ACP both ways**: `goose acp` serves ACP over stdio to editors (Zed/JetBrains), and external ACP agents
  (Claude Code, Codex) can be used **as providers**, with goose passing its extensions through as MCP
  servers. https://goose-docs.ai/docs/goose-architecture/ and
  https://goose-docs.ai/docs/guides/acp-providers/

---

## 10. Checkpoints & undo

### opencode — yes

- **Git-backed snapshots** are enabled by default and exist specifically so changes "can be rolled back
  through the UI"; disable with `"snapshot": false`, which means "changes made by the agent cannot be
  rolled back".
  https://opencode.ai/docs/config/#snapshot
- Implementation uses an **internal git repository** ("it tracks all changes using an internal git
  repository"), and the source drives git plumbing directly — `git cat-file --batch` for snapshot diffs
  with a per-file `git show` fallback, and reuse of object hashes between the original repo and the
  snapshot store.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/snapshot/index.ts
  - The shadow repo is **per-worktree**: `gitdir: path.join(Global.Path.data, "snapshot", ctx.project.id,
    Hash.fast(ctx.worktree))` — one snapshot gitdir per project *and* per worktree hash, kept outside the
    user's repository. It seeds its index from the source repo "so already-hashed entries are reused",
    which is the mitigation for the slow-indexing cost the docs warn about. Same URL.
- User-facing `/undo` reverts changes and restores the original message so you can retry; `/redo` reapplies;
  `/undo` can be run repeatedly.
  https://opencode.ai/docs/ and https://opencode.ai/docs/tui/
- Each edit records a `Snapshot.FileDiff` with per-file `diagnostics`, so file state and diagnostics are
  versioned together.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/edit.ts
- Related session machinery: `session/revert.ts`, and `session.diff` is a published plugin event.
  https://github.com/anomalyco/opencode/tree/dev/packages/opencode/src/session and
  https://opencode.ai/docs/plugins/#events

### goose — no built-in undo

- **No checkpoint or file-undo feature was found.** Searches over `documentation/docs/` and
  `crates/goose/src/` returned no `undo_edit` tool, no checkpoint/rewind command, and no snapshot store.
  The Developer extension's five tools are `write`, `edit`, `shell`, `tree`, `read_image` only.
  https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/developer/mod.rs
- The nearest primitives are **conversation-level**, not filesystem-level: `/clear` (clear chat history),
  `/compact` (summarize), and `goose session --edit` / `--fork` (rewrite or branch the conversation).
  https://goose-docs.ai/docs/guides/goose-cli-commands/
- Third-party guidance in goose's own docs tells users to commit before letting goose change code, "so you
  have a clean snapshot to compare against … revert if needed" — consistent with undo being the user's VCS.
  https://goose-docs.ai/docs/mcp/jetbrains-mcp/
- Caveat: absence-of-evidence in docs+source is strong but not absolute; marked **unverified** as a
  definitive negative.

---

## 11. Cost & token control

### opencode

- `opencode stats` reports token usage and cost across sessions, with `--days N`, `--tools N`,
  `--models [N]` (model usage breakdown, hidden by default), `--project`.
  https://opencode.ai/docs/cli/#stats
- **Per-agent step caps are framed as a cost control**: "This allows users who wish to control costs to set
  a limit on agentic actions." https://opencode.ai/docs/agents/#max-steps
- Cost/token accounting is carried on the assistant message itself: the message record contains `cost: 0`
  and `tokens: { input, output, reasoning, cache: { read, write } }` — i.e. **prompt-cache reads and writes
  are tracked separately**.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/prompt.ts
- `opencode models --verbose` shows metadata "like costs"; the model catalogue is cached and refreshed with
  `--refresh`. https://opencode.ai/docs/cli/#models
- Per-agent **model tiering** is the documented cost lever ("a faster model for planning, a more capable
  model for implementation"), and compaction `reserved` buffers the window.
  https://opencode.ai/docs/agents/#model and https://opencode.ai/docs/config/#compaction
- No documented hard *budget* or spend cap was found — **unverified** as a negative.

### goose

- **Token usage is shown live in both interfaces**: after the first message, Desktop shows a colored circle
  next to the model name (green/orange/red at 80 % of capacity) and the CLI shows dots `●○` plus current
  token count and context limit.
  https://goose-docs.ai/docs/guides/sessions/smart-context-management/
- **Session cost estimate** appears at the bottom of the goose window and updates dynamically, with
  per-model breakdown when multiple models are used; "Ollama and local deployments always show a cost of
  $0.00" and costs "are estimates only, and not connected to your actual provider bill."
  https://goose-docs.ai/docs/guides/sessions/smart-context-management/
- `GOOSE_CLI_SHOW_COST` toggles cost display in CLI output (default off).
  https://goose-docs.ai/docs/guides/environment-variables/
- **Prompt caching is automatic for Claude**: "goose automatically enables Anthropic's prompt caching when
  using Claude models via Anthropic, Amazon Bedrock, Databricks, OpenRouter, and LiteLLM providers. This
  adds `cache_control` markers to requests." TTL is controlled by `GOOSE_CACHE_TTL` (`5m` default, `1h`
  costs 2× on writes vs 1.25×), and "Headless runs (`goose run`, subagents, scheduled recipes) always use
  `5m`".
  https://goose-docs.ai/docs/getting-started/providers/ and
  https://goose-docs.ai/docs/guides/environment-variables/
- **Model tiering**: compaction is delegated to smaller/faster LLMs (architecture doc), plan mode can use a
  separate `GOOSE_PLANNER_MODEL`, and recipes/subagents can pin their own `goose_model` and `max_turns`.
  https://goose-docs.ai/docs/goose-architecture/ ,
  https://goose-docs.ai/docs/guides/context-engineering/creating-plans/ ,
  https://goose-docs.ai/docs/guides/recipes/recipe-reference/
- **Context/output caps**: `GOOSE_MAX_TOKENS`, `GOOSE_CONTEXT_LIMIT`, `GOOSE_TOOL_CALL_CUTOFF`,
  `GOOSE_MAX_TOOL_RESPONSE_SIZE` (200 000 chars → spooled to temp file).
  https://goose-docs.ai/docs/guides/environment-variables/
- A **credit balance monitor** exists for providers that expose balance.
  https://goose-docs.ai/docs/guides/sessions/smart-context-management/
- Also documented: handling LLM rate limits, and anonymous usage-data collection controlled by
  `GOOSE_TELEMETRY_ENABLED` (default false).
  https://goose-docs.ai/docs/guides/handling-llm-rate-limits-with-goose/ and
  https://goose-docs.ai/docs/guides/usage-data/

---

## 12. Provider abstraction & multi-model support

### opencode

- **Provider list is delegated to models.dev.** "OpenCode is powered by the provider list at Models.dev";
  `opencode auth login` stores credentials in `~/.local/share/opencode/auth.json`, and providers load from
  that file, from environment variables, or from a project `.env`.
  https://opencode.ai/docs/cli/#auth and https://opencode.ai/docs/providers/
- models.dev is a **core module** in the codebase, imported as `@opencode-ai/core/models-dev` by the CLI
  (`cli/cmd/models.ts`, `cli/cmd/providers.ts`, `cli/cmd/github.handler.ts`) and by
  `provider/provider.ts`. Source comments show it is the source of truth for provider-specific quirks
  ("Models from models.dev may already include prefixes like us., eu., global.", "models.dev advertises
  GOOGLE_VERTEX_PROJECT for Vertex", "models.dev lists Anthropic ids with dotted versions").
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/provider/provider.ts
- The catalogue is cached and refreshable: `opencode models [provider] [--refresh] [--verbose]`.
  A custom catalogue URL can be supplied with `OPENCODE_MODELS_URL`; `OPENCODE_DISABLE_MODELS_FETCH`
  disables remote fetching; `OPENCODE_ENABLE_EXPERIMENTAL_MODELS` opts into experimental models.
  https://opencode.ai/docs/cli/
- Model IDs are always `provider/model`; `opencode models anthropic` filters by provider.
  https://opencode.ai/docs/cli/#models
- Provider selection is controllable at multiple layers: `disabled_providers` (takes priority over
  `enabled_providers`), per-agent `model`, per-command `model`, `--model/-m` flags, `--variant`
  ("provider-specific reasoning effort"), and the experimental `provider.use` policy.
  https://opencode.ai/docs/config/#disabled-providers ,
  https://opencode.ai/docs/config/#enabled-providers ,
  https://opencode.ai/docs/policies/ , https://opencode.ai/docs/cli/#run
- There is also a first-party curated gateway, **OpenCode Zen** (`/connect`, `opencode.ai/auth`), and an
  **OpenCode Go** provider referenced by the websearch tool gate.
  https://opencode.ai/docs/zen/ and https://opencode.ai/docs/tools/#websearch
- Local/OpenAI-compatible endpoints and network/proxy configuration have dedicated pages.
  https://opencode.ai/docs/providers/ , https://opencode.ai/docs/network/ , https://opencode.ai/docs/models/

### goose

- goose maintains its **own provider implementations** in Rust rather than delegating to a catalogue
  service. The docs page lists ~45 providers with their exact credential env vars — a sample:
  **Anthropic** (`ANTHROPIC_API_KEY`, `ANTHROPIC_HOST`), **OpenAI** (`OPENAI_API_KEY`, `OPENAI_HOST`,
  `OPENAI_ORGANIZATION`, `OPENAI_PROJECT`, `OPENAI_BASE_PATH`, `OPENAI_CUSTOM_HEADERS`, `OPENAI_STORE`),
  **Gemini** (`GOOGLE_API_KEY`, `GEMINI3_THINKING_LEVEL`), **GCP Vertex AI** (`GCP_PROJECT_ID`,
  `GCP_LOCATION`, retry/backoff vars), **Amazon Bedrock** (AWS credential chain or
  `AWS_BEARER_TOKEN_BEDROCK`), **Azure OpenAI** and **Azure AI Foundry**, **Databricks**
  (`DATABRICKS_HOST`, `DATABRICKS_TOKEN`), **LiteLLM** (`LITELLM_HOST`, `LITELLM_BASE_PATH`,
  `LITELLM_API_KEY`, `LITELLM_CUSTOM_HEADERS`, `LITELLM_TIMEOUT`), **OpenRouter**, **Groq**, **Mistral**,
  **Ollama** (`OLLAMA_HOST`), **LM Studio** (`localhost:1234`), **Docker Model Runner**, **Ramalama**,
  **Cerebras**, **xAI**, **Snowflake**, **VMware Tanzu**, and many OpenAI-compatible aggregators.
  https://goose-docs.ai/docs/getting-started/providers/
- **Local models** have a dedicated section (download requirement, `OLLAMA_HOST`, Docker Model Runner via
  `OPENAI_HOST`/`OPENAI_BASE_PATH`). https://goose-docs.ai/docs/getting-started/providers/#local-llms
- Provider can be configured by env (`GOOSE_PROVIDER`, `GOOSE_MODEL`, `GOOSE_TEMPERATURE`,
  `GOOSE_MAX_TOKENS`, `GOOSE_PROVIDER__TYPE`, `GOOSE_PROVIDER__HOST`, `GOOSE_PROVIDER__API_KEY`) or the
  `goose configure` TUI, and expanded per-provider keys can be placed in `config.yaml`.
  https://goose-docs.ai/docs/guides/environment-variables/ and https://goose-docs.ai/docs/guides/config-files/
- **API keys are not read from `config.yaml`** — they live in the system keyring (via `goose configure`) or
  in `secrets.yaml` when file-based storage is used; the provider env var takes precedence over stored
  secrets. `GOOSE_DISABLE_KEYRING` disables the keyring.
  https://goose-docs.ai/docs/guides/config-files/ and https://goose-docs.ai/docs/guides/environment-variables/
- **Multi-model within one session**: `GOOSE_PLANNER_PROVIDER`/`GOOSE_PLANNER_MODEL` split planning from
  execution, and there is a `guides/multi-model/` section.
  https://goose-docs.ai/docs/guides/multi-model/
  - If you are looking for a **lead/worker** tiering mode, that is **absent** in the revision inspected:
    the multi-model page is a landing page and no `GOOSE_LEAD*` variables or lead-worker module exist.
    Marked **unverified** as a definitive negative.
- **CLI providers** and **ACP providers** let goose delegate to other agents: `cursor-agent`, `claude-acp`
  (Claude Code) and `codex-acp` — in those cases the ACP agent "handles tool execution internally. goose
  passes configured extensions through as MCP servers", and (in approve mode) the external agent's
  permission prompts are routed through goose's UI.
  https://goose-docs.ai/docs/guides/cli-providers/ ,
  https://goose-docs.ai/docs/guides/acp-providers/ and
  https://goose-docs.ai/docs/guides/managing-tools/goose-permissions/
- **Tool-shim for models without tool calling**: `GOOSE_TOOLSHIM` plus `GOOSE_TOOLSHIM_BACKEND`
  (`ollama` | `local` | `llama.cpp`), `GOOSE_TOOLSHIM_OLLAMA_MODEL` (default `mistral-nemo`).
  https://goose-docs.ai/docs/guides/environment-variables/ and https://goose-docs.ai/docs/guides/tool-shim/
- Vendored/local-inference and roaming crates exist in-tree: `crates/goose-local-inference`,
  `crates/goose-providers`, `crates/goose-provider-types`, `crates/goose-roaming`.
  https://github.com/aaif-goose/goose/tree/main/crates

---

## 13. Streaming output & TUI/UX

### opencode — client/server split

- **The TUI is a client of a local server.** `opencode` starts a server and the TUI by default;
  `opencode serve` starts a headless HTTP server with no TUI; `opencode attach [url]` attaches a TUI to an
  **already running** backend — "This allows using the TUI with a remote OpenCode backend", e.g.
  `opencode web --port 4096 --hostname 0.0.0.0` then
  `opencode attach http://10.20.30.40:4096` in another terminal.
  https://opencode.ai/docs/cli/ and https://opencode.ai/docs/server/
- `opencode run --attach http://localhost:4096 "..."` reuses a running server specifically "to avoid MCP
  server cold boot times on every run". https://opencode.ai/docs/cli/#run
- Server surfaces: `--port`, `--hostname`, `--mdns`, `--mdns-domain`, `--cors`; auth via HTTP basic with
  `OPENCODE_SERVER_PASSWORD` (username default `opencode`, overridable with `OPENCODE_SERVER_USERNAME`).
  https://opencode.ai/docs/server/
- Documented HTTP API groups include Global, Project, Path & VCS, Instance, Config, Provider, Sessions,
  Messages, Commands, Files (`/find/file`), Tools (experimental), LSP/Formatters/MCP, Agents, Logging,
  TUI, Auth, **Events**, and docs.
  https://opencode.ai/docs/server/#apis
- Also: `opencode web` (headless server + browser UI), a desktop app, IDE extensions, and a separate
  Go client. https://opencode.ai/docs/web/ , https://opencode.ai/docs/ide/ , https://opencode.ai/docs/go/
- **LSP diagnostics are surfaced directly into the agent loop**, not just the UI: after an edit the tool
  calls `lsp.diagnostics()` and, if non-empty for that file, appends to the tool output:
  `LSP errors detected in this file, please fix:` followed by `LSP.Diagnostic.report(...)`; per-file
  diagnostics are also stored in the snapshot diff.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/edit.ts
- LSP is **disabled by default**, auto-starts per detected file extension, covers ~35 built-in servers,
  and auto-downloads servers unless `OPENCODE_DISABLE_LSP_DOWNLOAD` is set.
  https://opencode.ai/docs/lsp/
  - The docs are notably candid about the tradeoff: "Language servers can get out of sync, use significant
    memory… In many projects it is better to have the agent run lint, typecheck, or other diagnostic CLI
    tools directly, so errors are fed back into the agent loop without those tradeoffs."
    https://opencode.ai/docs/lsp/#best-practices
- **Formatters** run after edits: `formatter: true` enables built-ins, with per-formatter `disabled`,
  `command` (e.g. `["npx","prettier","--write","$FILE"]`) and `environment` overrides.
  https://opencode.ai/docs/formatters/ and https://opencode.ai/docs/config/#formatters
- TUI behaviours documented: `@` file fuzzy-search, drag-and-drop images into the terminal, `/undo`,
  `/redo`, `/share`, `/init`, `/connect`, `/compact`, `/new`, `/sessions`, `/models`, `/editor`, `/details`,
  `/export`, `/unshare`, `/themes`, `/thinking`, plus configurable `keybinds` in `tui.json` and a
  `theme`.
  https://opencode.ai/docs/tui/ and https://opencode.ai/docs/keybinds/

### goose

- Interface/agent split: CLI and Electron desktop are both clients of the same core agent. **There is no
  `goose-server` crate** — verified by `ls crates/` on the clone. The server role lives in `goose-cli`:
  - `goose serve` — serves **ACP over HTTP/WS**, `--host` default `127.0.0.1`, **`--port` default `3284`**,
    plus `--tls`, `--tls-cert-path`, `--tls-key-path`. Requires `GOOSE_SERVER__SECRET_KEY` unless
    `--dangerously-unauthenticated` is passed.
    https://github.com/aaif-goose/goose/blob/main/crates/goose-cli/src/cli.rs
  - `goose acp` — the same ACP surface over stdio for editors.
  - `GOOSE_TLS`, `GOOSE_TLS_CERT_PATH`, `GOOSE_TLS_KEY_PATH`, `GOOSE_SERVER__SECRET_KEY` mirror the flags.
    https://goose-docs.ai/docs/guides/environment-variables/
  https://goose-docs.ai/docs/goose-architecture/ ,
  https://goose-docs.ai/docs/guides/remote-goose-server/
- The desktop app is explicitly **Electron** and the repo's own `AGENTS.md` names the entry points:
  `crates/goose-cli/src/main.rs` (CLI), `ui/desktop/src/main.ts` (UI),
  `crates/goose/src/agents/agent.rs` (agent), and notes `ui/text/` is a "deprecated ACP TUI".
  https://github.com/aaif-goose/goose/blob/main/AGENTS.md
- Because `ui/text` (the old terminal UI) is deprecated, goose's primary interactive surface is the CLI
  REPL and the Desktop app rather than a full-screen TUI — a real architectural difference from
  opencode's Bubble Tea–style TUI.
  https://github.com/aaif-goose/goose/tree/main/ui/text
- **ACP integration** is the IDE story: `goose acp` serves ACP over stdio for Zed and JetBrains.
  https://goose-docs.ai/docs/goose-architecture/ and
  https://goose-docs.ai/docs/gdk/acp/
- Streaming/tool-output UX knobs: `GOOSE_CLI_SHOW_THINKING` (surface model reasoning),
  `GOOSE_DEBUG`/`/r` (full tool parameters untruncated), `GOOSE_MAX_CODE_BLOCK_LINES` (default 50, full
  content saved to a temp file), `GOOSE_TRUNCATED_SHOW_LINES` (default 20), `GOOSE_NO_CODE_TRUNCATION`,
  `GOOSE_CLI_MIN_PRIORITY` (tool-output verbosity), `GOOSE_CLI_BELL`.
  https://goose-docs.ai/docs/guides/environment-variables/ and
  https://goose-docs.ai/docs/guides/managing-tools/adjust-tool-output/
- Terminal integration and shell-awareness: `GOOSE_TERMINAL` is set to `1` when goose runs a command so
  shell configs can adapt.
  https://goose-docs.ai/docs/guides/terminal-integration/ and
  https://goose-docs.ai/docs/guides/environment-variables/

---

## 14. Non-interactive / headless / CI mode

### opencode

- `opencode run [message..]` — "Run opencode in non-interactive mode by passing a prompt directly… useful
  for scripting, automation, or when you want a quick answer without launching the full TUI."
  https://opencode.ai/docs/cli/#run
- Flags: `--command` (use message for args), `--continue/-c`, `--session/-s`, `--fork`, `--share`,
  `--model/-m`, `--agent`, `--file/-f`, `--format default|json` ("raw JSON events"), `--title`,
  `--attach <url>`, `--password/-p`, `--username/-u`, `--dir`, `--port`, `--variant`, `--thinking`,
  `--auto`.
  https://opencode.ai/docs/cli/#run
- `opencode serve` exposes the same capability as an HTTP API, and `opencode github run` /
  `opencode github install` provide repository automation with `--event` (GitHub mock event) and `--token`.
  https://opencode.ai/docs/cli/ and https://opencode.ai/docs/github/
  - GitHub CI setup is a first-class workflow: `opencode github install` "sets up the necessary GitHub
    Actions workflow", and opencode dogfoods it by responding to `/oc` and `/opencode` comment commands.
    https://opencode.ai/docs/github/ and
    https://github.com/anomalyco/opencode/blob/dev/.github/workflows/opencode.yml
- GitLab integration exists too. https://opencode.ai/docs/gitlab/
- Global flags relevant to CI/automation: `--print-logs` (logs to stderr), `--log-level DEBUG|INFO|WARN|ERROR`,
  `--pure` (no external plugins).
  https://opencode.ai/docs/cli/#global-flags
- The `run` help text also documents `--format json` behaviour in source
  (`choices: ["default", "json"]`, and JSON mode suppresses the interactive/TUI branches).
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/cli/cmd/run.ts

### goose

- `goose run` is a fully documented headless entry point.
  https://goose-docs.ai/docs/guides/goose-cli-commands/ and https://goose-docs.ai/docs/guides/running-tasks/
- **Input**: `-i/--instructions <FILE>` (use `-` for stdin), `-t/--text <TEXT>`, `--system <TEXT>`
  (additional system instructions), `--recipe <FILE> <OPTIONS>`, `--params k=v` (repeatable),
  `--sub-recipe <RECIPE>` (repeatable).
  https://goose-docs.ai/docs/guides/goose-cli-commands/
- **Session**: `-s/--interactive` (continue interactively after initial input), `-n/--name`,
  `-r/--resume`, `--path`, `--container`, `--no-session` ("Run goose commands without creating or storing a
  session file").
  https://goose-docs.ai/docs/guides/goose-cli-commands/
- **Extensions**: `--with-extension`, `--with-streamable-http-extension`, `--with-builtin`.
  https://goose-docs.ai/docs/guides/goose-cli-commands/
- **Control**: `--debug`, `--max-tool-repetitions <N>`, `--max-turns <N>` (default 1000), `--explain`
  (show a recipe's title/description/parameters without running), `--render-recipe` (print the rendered
  recipe), `-q/--quiet` (only the model response to stdout), `--provider`, `--model`.
  https://goose-docs.ai/docs/guides/goose-cli-commands/
- **Structured output for automation**: `--output-format <text|json|stream-json>`, default `text`;
  "`json` for results after completion, `stream-json` for events as they occur".
  https://goose-docs.ai/docs/guides/goose-cli-commands/ and
  https://goose-docs.ai/docs/guides/running-tasks/
  - Documented examples include `goose run --output-format stream-json -t "your instructions"`,
    `goose run --output-format json --recipe recipe.yaml`, and
    `goose run --output-format json --no-session -t "automated task"`.
    https://goose-docs.ai/docs/guides/running-tasks/
- CI/CD has a dedicated tutorial, and headless goose has its own tutorial page.
  https://goose-docs.ai/docs/tutorials/cicd/ and https://goose-docs.ai/docs/tutorials/headless-goose/
- goose runs itself in CI: `.github/workflows/goose-issue-solver.yml`, `goose-pr-reviewer.yml`,
  `goose-release-notes.yml`, and a `code-review.yml`.
  https://github.com/aaif-goose/goose/tree/main/.github/workflows
- Deterministic safe headless recipe validation is baked into the project's own workflow: the repo's
  `AGENTS.md` instructs contributors to "update goose-self-test.yaml, rebuild, then run
  `goose run --recipe goose-self-test.yaml` to validate."
  https://github.com/aaif-goose/goose/blob/main/AGENTS.md

---

## 15. Evaluation & regression tests

### opencode

- **No agent/LLM eval harness was found.** `find`-style searches across the repo for `eval`/`benchmark`
  surfaced only front-end performance benchmarks (`packages/app/e2e/performance/*benchmark*`) and a console
  DB schema file (`benchmark.sql.ts`) — nothing that scores model behaviour. Marked as a genuine gap,
  **verified by enumeration of the repo tree**.
  https://github.com/anomalyco/opencode/tree/dev/packages/app/e2e/performance
- **Regression testing is extensive and ordinary**: `packages/opencode/test/` contains **382 files**
  (counted via `git ls-tree`), including targeted agent-loop regressions such as
  `test/agent/plan-mode-subagent-bypass.test.ts` and `test/agent/plugin-agent-regression.test.ts`.
  https://github.com/anomalyco/opencode/tree/dev/packages/opencode/test
- The repo's testing culture is documented for contributors: "Avoid mocks as much as possible", "Test
  actual implementation, do not duplicate logic into tests", and tests must run from package dirs, not repo
  root (guard `do-not-run-tests-from-root`).
  https://github.com/anomalyco/opencode/blob/dev/AGENTS.md

### goose

- **Two conformance harnesses in CI, both real and named:**
  1. **MCP Conformance** — runs on push/PR to `main`, builds conformance binaries via
     `just mcp-conformance-build`, with checked-in expected-failure baselines per MCP spec version, e.g.
     `crates/goose-cli/tests/mcp-conformance/expected-failures-2025-11-25-0.1.16.yaml`.
     https://github.com/aaif-goose/goose/blob/main/.github/workflows/mcp-conformance.yml
  2. **Model Tool Call Conformance** — daily scheduled sweep (`cron: '0 3 * * *'`) that "sweeps many models
     and reports which ones can actually drive an MCP tool call through goose", explicitly "NOT a PR gate:
     it exercises third-party models over the network, so a failure here usually means a provider or model
     regressed". Inputs: `model_count` (default 10 top OpenRouter tool-capable models) or an explicit
     `models` list.
     https://github.com/aaif-goose/goose/blob/main/.github/workflows/model-toolcall-conformance.yml
- **Agent-level self-test as a recipe**: `goose-self-test.yaml` at repo root — "A comprehensive meta-testing
  recipe where goose tests its own capabilities using its own tools", with parameters `test_phases`
  (all|basic|extensions|delegation|reasoning|acp-effort|advanced), `test_depth`
  (quick|standard|deep), `workspace_dir`, `parallel_tests`. It exercises file ops incl. **undo**, shell
  error handling, extension discovery, the `load` tool, the `delegate` tool (sync and async),
  multi-turn thinking preservation, ACP thinking-effort discovery, and observability hook lifecycle events.
  https://github.com/aaif-goose/goose/blob/main/goose-self-test.yaml
- Unit/integration test surface: dedicated crates `goose-test` and `goose-test-support`, plus
  `crates/goose/tests/` with ACP-focused tests (`acp_fork_session_test.rs`, `acp_provider_test.rs`,
  `acp_bootstrap_effort_test.rs`) and `crates/goose-agent/tests/tool_operation.rs`.
  https://github.com/aaif-goose/goose/tree/main/crates/goose/tests
- **Recorded provider-scenario tests** exist as a distinct layer: `crates/goose-cli/src/scenario_tests/`
  with checked-in recordings under `recordings/{anthropic,openai,google,groq,azure_openai}/`, replayed
  without network. `just record-mcp-tests` regenerates MCP recordings.
  https://github.com/aaif-goose/goose/tree/main/crates/goose-cli/src/scenario_tests
- The state machine has its own lifecycle test suites under
  `crates/goose/src/agents/state_machine/tests/` (e.g. `recipe_scheduling_lifecycle.rs`).
  https://github.com/aaif-goose/goose/tree/main/crates/goose/src/agents/state_machine/tests
- The repo is candid about the difference between **gates and signals**: model tool-call conformance is
  scheduled, not blocking, because its failures usually mean a *provider* regressed rather than goose.
  That is a notably disciplined stance versus treating eval failures as regressions. Same CI URLs.

### Contrast with opencode

opencode's regression investment is in **unit/integration tests, not eval**: no recorded-provider scenario
layer and no scheduled model sweep were found, only the 382-file test tree plus front-end perf benchmarks.

---

## 16. Observability

### opencode

- **Logs**: `~/.local/share/opencode/log/` on macOS/Linux (Windows `%USERPROFILE%\.local\share\opencode\log`);
  files are timestamp-named (`2025-01-09T123456.log`) and only the **10 most recent** are kept.
  https://opencode.ai/docs/troubleshooting/
- `--print-logs` (to stderr) and `--log-level DEBUG|INFO|WARN|ERROR` on every command.
  https://opencode.ai/docs/cli/#global-flags
- **`opencode debug`** subcommands exist for agents, config, file, LSP, ripgrep, skill, snapshot, startup
  and v2 internals — a deliberate introspection surface.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/cli/cmd/debug/index.ts
- Data layout under `~/.local/share/opencode/`: `log/` (application logs), project-scoped
  `./<project-slug>/storage/` sessions inside a git repo (else `./global/storage/`).
  https://opencode.ai/docs/troubleshooting/
- **Token accounting** is per assistant message and split into `input`, `output`, `reasoning`, and
  `cache: { read, write }`; aggregated with `opencode stats`.
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/prompt.ts and
  https://opencode.ai/docs/cli/#stats
- Traces: the LLM layer wraps spans and tags them with the session id
  (`span.setAttribute("session.id", input.sessionID)`) and logs runtime selection
  (`"llm.runtime"`, `"llm.provider"`, `"llm.model"`).
  https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/llm.ts
- Server exposes a Logging API group and an Events API group for external consumers.
  https://opencode.ai/docs/server/#apis
- `opencode stats` is the user-facing analytics surface (tokens, cost, tool usage, model breakdown).
  https://opencode.ai/docs/cli/#stats

### goose

- **Logs are local-only and never sent anywhere**: "goose is a local application and all goose log files are
  stored locally. These logs are never sent to external servers."
  https://goose-docs.ai/docs/guides/logs/
- Log locations: system logs `~/.local/state/goose/logs/` (Windows
  `%APPDATA%\Block\goose\data\logs\`); CLI logs in `.../logs/cli/`; server logs in `.../logs/server/`;
  Desktop app log at `~/Library/Application Support/Goose/logs/main.log` (macOS). CLI and server logs are
  organized into date-based directories and **cleaned up after two weeks**.
  https://goose-docs.ai/docs/guides/logs/
- **Session records** are the durable transcript store, shared across CLI and Desktop ("your conversation
  history is consistent regardless of which interface you use"), backed by SQLite at
  `~/.local/share/goose/sessions/sessions.db` and inspectable with `sqlite3` queries.
  https://goose-docs.ai/docs/guides/logs/ and https://goose-docs.ai/docs/guides/sessions/session-management/
- When **prompt-injection detection** is enabled, CLI and server logs additionally record detection data.
  https://goose-docs.ai/docs/guides/logs/
- **External LLM observability integrations ship as first-class tutorials**: Langfuse, Laminar, and MLflow.
  https://goose-docs.ai/docs/tutorials/langfuse/ ,
  https://goose-docs.ai/docs/tutorials/laminar/ ,
  https://goose-docs.ai/docs/tutorials/mlflow/
- Token/cost accounting is user-visible in-session (§11) and inspectable per session.
  https://goose-docs.ai/docs/guides/sessions/smart-context-management/
- A diagnostics-and-reporting bundle path exists for bug reports, and usage telemetry is opt-in
  (`GOOSE_TELEMETRY_ENABLED`, default false).
  https://goose-docs.ai/docs/troubleshooting/diagnostics-and-reporting/ and
  https://goose-docs.ai/docs/guides/usage-data/

---

## 17. Coverage checklist — gaps and honest negatives

| Domain | opencode | goose |
| --- | --- | --- |
| Agent loop / tool protocol | ✅ AI-SDK `streamText` loop, `steps` cap, unbounded parallel tool exec | ✅ streaming loop, `stream::select_all` parallel tool exec, `GOOSE_MAX_TURNS`=1000, state-machine migration behind `GOOSE_STATE_MACHINE=1` |
| Tool set / edit strategy | ✅ 13 built-ins; search-replace `edit` w/ 10 replacers **plus** model-gated `apply_patch` | ⚠️ 5 Developer tools only (`shell`/`write`/`edit`/`tree`/`read_image`); `edit` takes `before`/`after`; **no glob/grep/read/patch** |
| Context management | ✅ computed budget `limit.input − min(20k, maxOutput)`, `compaction.{auto,prune,reserved}`, ripgrep/.gitignore/.ignore | ✅ 80 % auto-compact, `GOOSE_CONTEXT_STRATEGY`, tool-output cutoff, MOIM working memory, no `.gooseignore` |
| Permissions & safety | ✅ allow/ask/deny, `findLast` last-match-wins, tree-sitter+arity bash patterns, `external_directory`, `doom_loop`, `--auto`, policies | ✅ 4 modes (`GOOSE_MODE`, default `auto`), `permission.yaml` (`always_allow`/`ask_before`/`never_allow`), hooks-as-policy, extension allowlist, OSC/OSV checks, adversary mode, containers |
| Session persistence / resume / fork / share | ✅ continue/session/fork/export/import/db; public share links (`opncd.ai/s/<id>`) | ✅ SQLite `sessions.db` w/ `parent_session_id`, `--resume`/`--fork`/`--edit` YAML editing, **encrypted Nostr sharing** (`GOOSE_NOSTR_RELAYS`, kind 30278), import from Claude Code/Codex/Pi |
| Plan / todos / goals | ✅ plan agent + `todowrite`; **no goal-tracking subsystem** | ✅ `/plan` + `/endplan` w/ planner model; `todo_write` (`GOOSE_TODO_MAX_CHARS` 50000); **`/goal` and `/grind`** nudge loops |
| Subagents & parallelism | ✅ primary/subagent, `subagent_depth` (default 1), per-agent model, background flag | ✅ NL-driven delegation via `summon` (`delegate`/`load`), `orchestrator` session tools, subagent budgets |
| Extensibility | ✅ MCP, plugins+hooks (30+ events), commands, custom tools, skills, ACP, SDK | ✅ MCP-everything extension trait, recipes/subrecipes, hooks (12 events), plugins, skills, slash commands, ACP both ways |
| Checkpoints & undo | ✅ git-backed snapshots, `/undo`, `/redo` | ❌ **none found** (no `undo_edit`, no rewind/checkpoint) |
| Cost & token control | ✅ per-message cost + cache read/write tokens, `stats`, per-agent models; no hard budget found | ✅ live token indicator, session cost + per-model breakdown, Anthropic prompt caching (`GOOSE_CACHE_TTL`), planner model split |
| Provider abstraction | ✅ **models.dev** catalogue (`@opencode-ai/core/models-dev`), auth.json, `disabled_providers`, policies | ✅ ~45 own Rust provider impls w/ documented env vars, LiteLLM/Ollama/OpenAI-compatible, CLI+ACP providers as models |
| Streaming / TUI / LSP | ✅ real client–server split (`serve`/`attach`/`web`), LSP diagnostics injected into tool output after edits, formatters | ⚠️ no `goose-server` crate — `goose serve` (ACP over HTTP/WS, port 3284) + `goose acp` (stdio) in `goose-cli`; old `ui/text` TUI deprecated; ACP for IDEs; no LSP-diagnostic loop found |
| Headless / CI / scriptability | ✅ `opencode run --format json`, `serve` API, `github run`, env-injected permissions | ✅ `goose run` with `--output-format text\|json\|stream-json`, `--no-session`, `-q`, `--recipe`, `--params` |
| Evaluation | ⚠️ no agent eval harness; 382 test files as regression suite; no recorded-provider scenarios | ✅ MCP conformance + daily model tool-call conformance CI, recorded provider scenario tests, state-machine lifecycle suites, `goose-self-test.yaml` recipe |
| Observability | ✅ `~/.local/share/opencode/log/` (10 files), `debug` subcommands, spans w/ session id, `stats` | ✅ local-only logs, 2-week retention, SQLite transcripts, Langfuse/Laminar/MLflow tutorials, opt-in telemetry |

### Not verified / open questions

- opencode: whether `plan` mode is usable without `OPENCODE_EXPERIMENTAL_PLAN_MODE` (docs describe plan as a
  built-in agent without the caveat; the env-var table lists a plan-mode flag) — **unverified**.
- opencode: any hard cost budget or spend cap — none found. There is also **no goal-tracking subsystem**;
  the only `goal` hits in `src/` are English prose inside prompt templates.
- opencode: `@opencode-ai/core` is an external workspace package, so `MAX_STEPS_PROMPT`'s exact text, the
  SQLite/Drizzle schema, `Global.Path` values and the models.dev cache path could not be read from the
  clone — **unverified**.
- opencode: **docs→code mismatches found.** (a) permissions docs say most permissions default to `allow`
  while the evaluator's no-match fallback is `ask`; (b) docs publish `.env` → `deny` while shipped code
  uses `ask`. Both verified in source; the docs are the stale side.
- goose: the true default order of `AGENTS.md` vs `.goosehints` (two docs pages disagree) — **unverified**.
- goose: whether `delegate`/`summon` is really unavailable in `approve`/`smart_approve`/`chat` modes —
  **reported, not independently verified**.
- goose: existence of any path allowlist equivalent to opencode's `external_directory` — none found.
- goose: **docs→source debt.** Several pages (hooks, subagents, permissions) still describe a `text_editor`
  tool that no longer exists; the Developer extension ships only the five tools above.
- goose: the Desktop UI (`ui/`) was not in the inspected checkout, so all Desktop-specific claims here are
  doc-sourced only. There is also **no full-screen TUI** — `ui/text` is a deprecated ACP TUI.
- goose: no lead/worker multi-model tiering found in this revision.
- Both: current CLI `--help` output was not executed (no binaries installed); all flags above come from
  official docs and source, not from running the tools.
- Both: negative claims ("X does not exist") rest on repo-wide search plus doc reading. They are strong but
  not absolute; they are labelled **unverified** rather than asserted.

### Repo revisions inspected

- opencode: branch `dev`, commit `859106eb17d5b840475f5e4b78e64c9622f8750e` (2026-09-10).
- goose: `main`, HEAD `bea9954` — "fix(cli): return failure for interrupted headless runs (#11938)".

Both are **late revisions**, which is why several assumptions in the original brief (§0, §2, §3, §12) no
longer hold for the current code.
