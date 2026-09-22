# Codex CLI vs Gemini CLI — design & feature notes

Research notes on the design and features of **OpenAI Codex CLI** and **Google Gemini CLI**,
compiled from primary sources only (official docs, official repos/source, official changelogs).

## Provenance

| Tool | Repo | Commit examined | Date |
|---|---|---|---|
| Codex CLI | https://github.com/openai/codex | `ddea03ad049142943bdbf13e937b1d67e8c1ba0c` | 2026-09-10 |
| Gemini CLI | https://github.com/google-gemini/gemini-cli | `ed2ac40df67a319bf348bd7e3d10494696b31b38` | 2026-09-08 |

Source-code citations use `https://github.com/<owner>/<repo>/blob/main/<path>` (shallow `main` clones).

**Important sourcing fact for Codex:** the `docs/*.md` files inside the repo are **stubs**. They contain
only links out to the docs site — e.g. `docs/config.md` is 15 lines, `docs/sandbox.md` and
`docs/exec.md` are 3 lines each. The real documentation now lives on the official docs site, and
`developers.openai.com/codex/` + page name **308-redirects** to `learn.chatgpt.com/docs/` + slug.
Appending `.md` to a `learn.chatgpt.com` docs URL returns clean Markdown (verified: the config
reference returns ~112 KB of Markdown). The canonical index is `https://learn.chatgpt.com/llms.txt`.

- Codex repo stub: https://github.com/openai/codex/blob/main/docs/config.md
- Docs index: https://learn.chatgpt.com/llms.txt
- Redirect example: https://developers.openai.com/codex/config-reference → https://learn.chatgpt.com/docs/config-file/config-reference

**Important sourcing fact for Gemini:** the repo's `docs/` tree is first-class and current, and the
official docs site is **https://geminicli.com/docs/** (marked "our documentation" from the README).
`https://geminicli.com/llms.txt` returns a single-file export of the whole docs set (verified, ~956 KB).
Citations below use the repo path (source of truth, diffable against the commit above); the same
the same pages are served from the `https://geminicli.com/docs/` site at the identical path.

- Gemini README docs link: https://github.com/google-gemini/gemini-cli/blob/main/README.md
- Gemini docs export: https://geminicli.com/llms.txt

Where a claim is uncertain it is marked **unverified**. Where a feature does not exist, it is stated
explicitly rather than omitted.

---

# 1. Gemini CLI (google-gemini/gemini-cli)

## 1.1 Agent loop & tool-calling protocol

- **Single turn = one model stream call; multi-turn = recursive re-entry with the same `prompt_id`.**
  Entry point `GeminiClient.sendMessageStream(request, signal, prompt_id, turns = MAX_TURNS, ...)`
  delegating to `processTurn(...)`. One turn is one `turn.run(modelConfigKey, request, signal, ...)`
  call returning an `AsyncGenerator<ServerGeminiStreamEvent>`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/core/client.ts ·
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/core/turn.ts
- **Turn ceiling: `const MAX_TURNS = 100`**, enforced by `const boundedTurns = Math.min(turns, MAX_TURNS)`.
  If a stream ends with no `turn.pendingToolCalls`, `checkNextSpeaker()` may return `model`, and the
  client injects `[{ text: 'Please continue.' }]` and recurses with `boundedTurns - 1`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/core/client.ts
- **Per-session cap is separate from the per-prompt cap:** `sessionTurnCount`; when
  `getMaxSessionTurns() > 0 && sessionTurnCount > getMaxSessionTurns()` it yields
  `GeminiEventType.MaxSessionTurns`. Setting `model.maxSessionTurns`, default `-1` (unlimited).
  Error string: "Reached max session turns for this session. Increase the number of turns by
  specifying maxSessionTurns in settings.json."
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/config/config.ts ·
  https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/config/settingsSchema.ts
- **Architectural seam:** the core **emits** function calls (`GeminiEventType.ToolCallRequest`) and
  does *not* execute them — the CLI layer schedules and executes. Tool continuation is therefore
  driven by the CLI, not by a core-owned loop.
- **Parallel tool calls: yes, gated.** `CoreToolScheduler.schedule(request | request[])` fans out
  validation with `await Promise.all(validatingCalls.map(...))` and execution with
  `const execResults = await Promise.all(scheduledCalls.map((c) => this._execute(c, signal)))`.
  `_isParallelizable()` returns **false** for `update_topic` and for any member of `EDIT_TOOL_NAMES`
  (= `replace` + `write_file`); otherwise it honors the per-call arg `wait_for_previous`, with the
  source comment "Default to parallel if the flag is omitted."
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/scheduler/scheduler.ts ·
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/tool-names.ts
- **Tool results feed back as `functionResponse` parts** built by
  `convertToFunctionResponse(name, callId, output, activeModel, config)`; failures become
  `{ functionResponse: { id, name, response: { error: message } } }`. Oversized output is truncated
  and spilled to a file via `truncateOutputIfNeeded`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/scheduler/tool-executor.ts ·
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/utils/generateContentResponseUtilities.ts
- **Stop conditions include loop detection.** `loopDetectionService.ts` constants:
  `TOOL_CALL_LOOP_THRESHOLD = 5`, `CONTENT_LOOP_THRESHOLD = 10`, `CONTENT_CHUNK_SIZE = 50`,
  `MAX_HISTORY_LENGTH = 5000`, `LLM_CHECK_AFTER_TURNS = 30`, `DEFAULT_LLM_CHECK_INTERVAL = 10`
  (bounded 5–15), `LLM_CONFIDENCE_THRESHOLD = 0.9`. Loop types: `CONSECUTIVE_IDENTICAL_TOOL_CALLS`,
  `CONTENT_CHANTING_LOOP`, `LLM_DETECTED_LOOP`. First detection triggers `_recoverFromLoop()`, which
  injects a "System: Potential loop detected…" user message and retries; a repeat aborts with
  `GeminiEventType.LoopDetected`. Disable with `model.disableLoopDetection`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/services/loopDetectionService.ts

## 1.2 Tool set & edit strategy

Canonical tool names are declared in
[`base-declarations.ts`](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/definitions/base-declarations.ts):

| Tool | Key parameters |
|---|---|
| `glob` | pattern matching |
| `grep_search` (legacy alias `search_file_content`) | — |
| `list_directory` | — |
| `read_file` | `start_line`, `end_line` |
| `run_shell_command` | `command`, `description`, `dir_path`, `is_background` |
| `write_file` | `file_path`, `content` (whole-file) |
| `replace` | `file_path`, `old_string`, `new_string`, `instruction`, `allow_multiple` |
| `google_web_search` | `query` |
| `web_fetch` | `prompt` |
| `write_todos` | `todos[]` of `description` + `status` |
| `read_many_files` | `include`, `exclude`, `recursive`, `useDefaultExcludes` |
| `get_internal_docs` | `path` |
| `activate_skill` | `name` |
| `ask_user` | `questions` |
| `enter_plan_mode` | `reason` |
| `exit_plan_mode` | **`plan_filename`** |
| `update_topic` | `title`, `summary`, `strategic_intent` |

Plus MCP resource tools `read_mcp_resource` / `list_mcp_resources`, six experimental `tracker_*`
tools, and the subagent-only `AGENT_TOOL_NAME = 'invoke_agent'` / `complete_task`.
Aggregate lists live in
[`tool-names.ts`](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/tool-names.ts)
(`ALL_BUILTIN_TOOL_NAMES`, `PLAN_MODE_TOOLS`).

- **Edit strategy is exact search/replace only — there is no unified-diff tool and no AST-based
  editing.** `replace` requires a *unique* `old_string` unless `allow_multiple` is set. The tool
  description instructs: "Must uniquely identify the instance(s) to change. Include at least 3 lines
  of context BEFORE and AFTER the target text".
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/edit.ts
- Failure strings (verified verbatim in `getErrorReplaceResult()`): "Could not find an exact match
  for old_string in '<file>'." (`EDIT_NO_OCCURRENCE_FOUND`); "Failed to edit, expected 1 occurrence
  but found ${occurrences}." plus "If you intended to replace multiple occurrences, set
  'allow_multiple' to true." (`EDIT_EXPECTED_OCCURRENCE_MISMATCH`); "No changes to apply. The
  old_string and new_string are identical." (`EDIT_NO_CHANGE`).
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/edit.ts
- **Self-correction on edit failure:** matching strategies are typed
  `strategy?: 'exact' | 'flexible' | 'regex' | 'fuzzy'` with `ENABLE_FUZZY_MATCH_RECOVERY = true`,
  `FUZZY_MATCH_THRESHOLD = 0.1`, `WHITESPACE_PENALTY_FACTOR = 0.1` (Levenshtein). On failure
  `attemptSelfCorrection()` calls `FixLLMEditWithInstruction(...)` and retries, emitting an
  `EditCorrectionEvent`; it re-reads on-disk content and detects external modification by SHA-256
  hash. https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/utils/llm-edit-fixer.ts
- **User-in-the-loop correction:** `ToolConfirmationOutcome` ∈ `proceed_once`, `proceed_always`,
  `proceed_always_and_save`, `modify_with_editor`. `getModifyContext()` opens `$EDITOR`, after which
  the tool reports "User modified the `content` to be: …".
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/modifiable-tool.ts
- `write_file` is whole-file create/overwrite and **rejects omission placeholders**: "`content`
  contains an omission placeholder (for example 'rest of methods ...'). Provide complete file
  content." https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/write-file.ts
- Tools are declared with **JSON Schema** (`parametersJsonSchema`) in per-model-family sets
  (`default-legacy.ts`, `gemini-3.ts`) selected by `getToolSet(modelId)` via `resolver.ts`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/definitions/resolver.ts
- **Surprising absence (verified):** `read_many_files` is **not registered as a model-callable
  tool** — only the `@` processor and ACP instantiate it. And **`save_memory` does not exist**: the
  system prompt says verbatim "There is no `save_memory` tool". Memory is written by editing Markdown
  with `replace`/`write_file`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/prompts/snippets.ts ·
  https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/memory.md

## 1.3 Context management

- **Compaction is token-threshold driven.** `chatCompressionService.ts`:
  `DEFAULT_COMPRESSION_TOKEN_THRESHOLD = 0.5`, `COMPRESSION_PRESERVE_THRESHOLD = 0.3`,
  `COMPRESSION_FUNCTION_RESPONSE_TOKEN_BUDGET = 50_000`. Fires when
  `originalTokenCount >= threshold * tokenLimit(model)`. `findCompressSplitPoint()` only splits at a
  `role === 'user'` message that lacks a `functionResponse`. Setting `model.compressionThreshold`
  (default `0.5`). `/compress` (altNames `summarize`, `compact`) forces it via
  `tryCompressChat(promptId, true)`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/context/chatCompressionService.ts ·
  https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/ui/commands/compressCommand.ts
- Pre-flight overflow guard: when `estimatedRequestTokenCount > remainingTokenCount` the client
  yields `GeminiEventType.ContextWindowWillOverflow` and returns early. A failed summarization is
  remembered (`hasFailedCompressionAttempt`) so only truncation is retried.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/core/client.ts
- **Token counting is an estimate, not a real tokenizer:**
  `ASCII_TOKENS_PER_CHAR = 0.33`, `NON_ASCII_TOKENS_PER_CHAR = 1.5`, `MSG_OVERHEAD_TOKENS = 5` in
  `estimateTokenCountSync()`. The exact `countTokens` API is called only when `inlineData`/`fileData`
  parts are present.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/utils/tokenCalculation.ts
- **`@` file references** are handled in the CLI, not the core: a directory resolves to
  `path.join(relativePath, '**')` (debug line "Path … resolved to directory, using glob: …"), and
  unresolved paths fall back to the `glob` tool **only** if `getEnableRecursiveFileSearch()` is on.
  Files are then read in a single `ReadManyFilesTool` invocation wrapped in
  `REFERENCE_CONTENT_START`/`REFERENCE_CONTENT_END`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/ui/hooks/atCommandProcessor.ts
- **Ignore rules:** `.geminiignore` (`GEMINI_IGNORE_FILE_NAME`) is combined with `.gitignore` and any
  `customIgnoreFilePaths` in `FileDiscoveryService` — source comment: "Create combined parser:
  .gitignore + .geminiignore + custom ignore". Settings under `fileFiltering`: `respectGitIgnore`
  (default true), `respectGeminiIgnore` (true), `enableRecursiveFileSearch` (true),
  `enableFuzzySearch` (true), `customIgnoreFilePaths` (`[]`), plus `maxFileCount`, `searchTimeout`.
  Tools accept per-call overrides `respect_git_ignore`, `respect_gemini_ignore`,
  `file_filtering_options`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/services/fileDiscoveryService.ts ·
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/utils/ignorePatterns.ts
- `read_many_files` output separator: `--- {filePath} ---` … `--- End of content ---`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/read-many-files.ts
- `read_file` truncation bound: `DEFAULT_MAX_LINES_TEXT_FILE = 2000`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/utils/constants.ts

## 1.4 Project rules & system prompt

- **`GEMINI.md` is the project rule file.** `DEFAULT_CONTEXT_FILENAME = 'GEMINI.md'` and
  `PROJECT_MEMORY_INDEX_FILENAME = 'MEMORY.md'`; `setGeminiMdFilename()` accepts `string | string[]`
  with path-traversal sanitization.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/memoryTool.ts
- **Three-tier discovery:** (1) global `~/.gemini/GEMINI.md`; (2) workspace dirs and their parents;
  (3) **just-in-time** context files discovered when a tool touches a directory. Imports via
  `@file.md` (relative and absolute).
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/gemini-md.md ·
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/jit-context.ts
- `contextFileName` is **not a `settings.json` key** — it is an **extension manifest** field
  (`ConfigParameters.contextFileName`).
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/config/config.ts
- `/memory` subcommands are exactly **`show`, `reload`, `list`**. `/memory add` and `/memory refresh`
  **do not exist** (a `refreshMemory()` function exists internally).
  https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/ui/commands/memoryCommand.ts
- **System prompt can be fully replaced** via the `GEMINI_SYSTEM_MD` env var read in
  `PromptProvider.getCoreSystemPrompt()`: `true`/`1` → `./.gemini/system.md`; an explicit path → that
  file (tilde expansion supported); missing file throws "missing system prompt file '<path>'". It is a
  **full replacement, not a merge**, and supports substitutions such as `${AgentSkills}`,
  `${SubAgents}`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/prompts/promptProvider.ts ·
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/system-prompt.md
- Prompt composition selects `snippets.ts` (modern models) vs `snippets.legacy.ts` based on
  `supportsModernFeatures(desiredModel)`. A `systemPromptMds` setting **does not exist**.

## 1.5 Permissions & safety

- **Approval modes:** `--approval-mode` is validated with
  `choices: ['default', 'auto_edit', 'yolo', 'plan']`. Note the internal enum differs:
  `ApprovalMode.AUTO_EDIT = 'autoEdit'`, so policy `modes` arrays use `autoEdit` while the CLI flag
  uses `auto_edit`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/config/config.ts ·
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/policy/types.ts
- `--yolo`/`-y` still exists but is documented **Deprecated**; combining it with `--approval-mode` is
  a hard error: "Cannot use both --yolo (-y) and --approval-mode together. Use --approval-mode=yolo
  instead." https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/cli-reference.md
- `general.defaultApprovalMode` accepts only `default`|`auto_edit`|`plan` — YOLO "can only be enabled
  via command line"; a `yolo` value in settings is ignored by the source. YOLO can be blocked by
  `security.disableYoloMode` or `admin.secureModeEnabled` (→ `FatalConfigError`), and an untrusted
  folder forces the mode back to `default`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/settings.md
- **Policy engine** — TOML rules in `~/.gemini/policies/*.toml`. Fields: `toolName`, `subagent`,
  `mcpName`, `toolAnnotations`, `argsPattern`, `commandPrefix`, `commandRegex`, `decision`,
  `priority`, `denyMessage`, `modes`, `interactive`, `allowRedirection`. Decisions are exactly
  `allow`, `deny`, `ask_user` (verified as an enum); in non-interactive mode `ask_user` is treated as
  `deny`. A global `deny` removes the tool from the model's toolset entirely.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/reference/policy-engine.md ·
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/policy/types.ts
- **Policy tiers** (priority resolution): Default 1, Extension 2, Workspace 3 (**currently
  disabled**, upstream issue #18186), User 4, Admin 5, with
  `final_priority = tier_base + (toml_priority / 1000)` and TOML `priority` in 0–999. Admin policy
  dirs include `/etc/gemini-cli/policies`, `/Library/Application Support/GeminiCli/policies`,
  `C:\ProgramData\gemini-cli\policies`; plus `--admin-policy`, `adminPolicyPaths`, and `--policy`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/reference/policy-engine.md
- **Tool allow/deny settings:** `tools.core` (allowlist), `tools.allowed` (bypass confirmation, e.g.
  `"run_shell_command(git)"`), `tools.confirmationRequired` (always confirm; wins),
  `tools.exclude` (blocklist, deprecated in favour of `deny` rules).
  https://github.com/google-gemini/gemini-cli/blob/main/docs/reference/configuration.md
- **Trusted folders:** `security.folderTrust.enabled`; trust file `~/.gemini/trustedFolders.json`,
  overridable via `GEMINI_CLI_TRUSTED_FOLDERS_PATH`; headless bypass with `--skip-trust` or
  `GEMINI_CLI_TRUST_WORKSPACE=true`, otherwise `FatalUntrustedWorkspaceError` (exit 55). `/permissions
  trust [<dir>]`. **Doc conflict:** `docs/cli/trusted-folders.md` says disabled by default, while
  `docs/cli/settings.md` lists the default as `true`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/trusted-folders.md
- Shell-specific: policy `allowRedirection` is required for `>`, `>>`, `<`, `<<`, `<<<`; by default
  redirection triggers confirmation even when a rule matches.

## 1.6 Sandboxing

- **Enabling:** `-s`/`--sandbox`, `GEMINI_SANDBOX=true|docker|podman|sandbox-exec|runsc|lxc`,
  `tools.sandbox` (boolean | string | object `{enabled, command, image, allowedPaths,
  networkAccess}`). Valid commands are exactly `docker`, `podman`, `sandbox-exec`, `runsc`, `lxc`,
  `windows-native`; `runsc` is Linux-only and never auto-detected. The image comes from
  `tools.sandbox.image`, `GEMINI_SANDBOX_IMAGE`, or a package.json `sandboxImageUri` — **there is no
  `sandboxImage` settings key**.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/sandbox.md ·
  https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/config/sandboxConfig.ts
- **Seatbelt (macOS) profiles** via `SEATBELT_PROFILE`: `permissive-open` (default),
  `permissive-proxied`, `restrictive-open`, `restrictive-proxied`, `strict-open`, `strict-proxied`.
  Backward-compatibility aliases also exist: `permissive-closed` → `strict-open` and
  **`restrictive-closed` → `strict-proxied`** (source comment: "Map standard 'closed' profiles to
  their strict counterparts for backward compatibility and fallback support").
  https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/utils/sandboxBuiltinProfiles.ts
  *Note: a prior pass claimed `restrictive-closed` does not exist; that is incorrect — it exists as an
  alias assignment.*
- Other knobs: `SANDBOX_MOUNTS` (`from:to:opts`), `SANDBOX_FLAGS`, `SANDBOX_SET_UID_GID`,
  `SANDBOX_PORTS`, `BUILD_SANDBOX` (source builds only), `.gemini/sandbox.Dockerfile`,
  `GEMINI_SANDBOX_PROXY_COMMAND`, `tools.sandboxAllowedPaths`, `tools.sandboxNetworkAccess`,
  `security.toolSandboxing` (default `false`). `docs/reference/configuration.md` claims the sandbox
  auto-enables under `--yolo`; no such linkage was found on the full-process sandbox path — treat as
  **docs-only, unverified**.

## 1.7 Session persistence, resume, fork

- **Storage:** `~/.gemini/tmp/<project_hash>/chats/` as JSON, written by `ChatRecordingService`
  (messages, tool executions, token usage, thoughts). Retention: `general.sessionRetention.{enabled,
  maxAge:"30d", maxCount, minRetention:"1d"}`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/config/storage.ts ·
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/session-management.md
- **Flags:** `--resume`/`-r` (bare value coerced to `RESUME_LATEST`; accepts `latest`, an index, or a
  UUID), `--session-id` (new session with a manual UUID, validated `^[a-zA-Z0-9-_]+$`),
  `--list-sessions`, `--delete-session <index>`, `--session-file <json>`, `--include-directories`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/config/config.ts
- **Slash commands:** `/resume` (Session Browser), plus named chat checkpoints
  `/resume save <name>`, `/resume list`, `/resume resume <name>`.
- **No session forking/branching exists.** The only in-session branch points are named chat
  checkpoints within the same session. **Verified absence.**

## 1.8 Checkpoints & undo

- **`/rewind`** (also Esc twice) rewinds the *current* session with three choices: revert
  conversation **and** code changes / conversation only / code changes only. It works via git
  snapshots and explicitly "does not undo manual edits or changes triggered by the shell tool".
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/rewind.md
- Separate from rewind: automatic checkpoints on edit tools, restored with `/restore
  <checkpoint_file>`, stored at `~/.gemini/tmp/<project_hash>/checkpoints`, controlled by
  `checkpointing.enabled`. **The old `--checkpointing` flag was removed.**
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/checkpointing.md

## 1.9 Plan mode, todos, goal tracking

- **Plan mode exists and is enabled by default.** Enter via `gemini --approval-mode=plan`, `/plan
  [goal]`, Shift+Tab cycling (`Default` → `Auto-Edit` → `Plan`), or the `enter_plan_mode` tool
  (`reason`). Exit via `exit_plan_mode`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/plan-mode.md
- **Doc/code discrepancy (verified):** `docs/reference/tools.md` documents the `exit_plan_mode`
  argument as **`plan_path`**, but the implementation uses **`plan_filename`** (required;
  `plan_filename is required.`). The code is authoritative.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/exit-plan-mode.ts ·
  https://github.com/google-gemini/gemini-cli/blob/main/docs/reference/tools.md
- Plan tools register only `if (this.isPlanEnabled())`; `PLAN_MODE_TOOLS` is the read-only allowlist,
  and web fetch requires explicit confirmation in plan mode.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/tool-names.ts
- **`write_todos`** takes `todos[]` with `description` + `status` ∈ `pending`/`in_progress`/
  `completed`/`cancelled`/`blocked`. Exactly one todo may be `in_progress` — error verbatim:
  'Invalid parameters: Only one task can be "in_progress" at a time.' State is session-scoped;
  Ctrl+T toggles the list view.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/write-todos.ts
- **Experimental task tracker** (`experimental.taskTracker`): `tracker_create_task`,
  `tracker_update_task`, `tracker_get_task`, `tracker_list_tasks`, `tracker_add_dependency`,
  `tracker_visualize`. State in `.gemini/tmp/tracker/<session-id>`; 6-char hex IDs; `create_task`
  types `epic|task|bug`; `update_task` statuses `open|in_progress|blocked|closed`; explicit
  dependency/topological ordering.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/tracker.md
- `update_topic` (`title`, `summary`, `strategic_intent`) is forced **sequential** by the scheduler.

## 1.10 Subagents & parallel orchestration

- **Enabled by default**; disable with `experimental.enableAgents: false` (schema default `true`).
  The tool is exactly **`invoke_agent`**; the reverse tool is `complete_task`. **There is no
  `list_agents` tool** — listing is `/agents list`. In policy matching, a subagent's name acts as a
  virtual `toolName` alias.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/tool-names.ts
- **Definitions are Markdown + YAML frontmatter** in `.gemini/agents/*.md` or `~/.gemini/agents/*.md`.
  Fields: `name`, `description`, `kind` (`local`|`remote`), `tools` (supports `*`, `mcp_*`,
  `mcp_server_*`), `mcpServers`, `model`, `temperature`, `max_turns` (default 30), `timeout_mins`
  (default 10). Writing `@name` in a prompt forces delegation.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/core/subagents.md
- Built-in agents: `codebase_investigator`, `cli_help`, `generalist`, `browser_agent` (disabled by
  default; `agents.browser.sessionMode` = `persistent`|`isolated`|`existing`, plus `allowedDomains`,
  `visualModel`, `maxActionsPerTask` = 100, `confirmSensitiveActions`, `blockFileUploads`).
- **Recursion protection:** subagents **cannot** call other subagents, even with the `*` wildcard.
  **No `maxSubagentDepth` setting exists.** Limits are per-agent `max_turns`/`timeout_mins` and
  `agents.overrides.<name>.runConfig.{maxTurns,maxTimeMinutes}`. **No documented
  parallel-subagent execution setting.**
- **Remote subagents use A2A:** `kind: remote` with `agent_card_url` or `agent_card_json`;
  `auth.type` ∈ `apiKey`, `http` (Bearer/Basic/raw scheme), `google-credentials`, `oauth` (PKCE,
  browser sign-in); dynamic secrets via `$ENV_VAR`, `!command` and `$$`/`!!` escapes;
  `google-credentials` is restricted to `*.googleapis.com` and `*.run.app`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/core/remote-agents.md
- **No scheduled/cron agent-task feature.** Background work is shell-level (`is_background`;
  `tools.shell.backgroundCompletionBehavior` = `silent`|`inject`|`notify`) plus
  `experimental.autoMemory` background extraction (idle ≥ 3 h, ≥ 10 user messages, reviewed via
  `/memory inbox`).

## 1.11 Extensibility

- **MCP** — three transports: stdio (`command`), SSE (`url`), streamable HTTP (`httpUrl`).
  Top-level `mcpServers` map; globals `mcp.serverCommand`, `mcp.allowed`, `mcp.excluded`. Per-server:
  `args`, `headers`, `env` (`$VAR`/`${VAR}`/`%VAR%`), `cwd`, `timeout` (default 600000 ms), `trust`,
  `includeTools`, `excludeTools` (exclude wins), `targetAudience`, `targetServiceAccount`. OAuth via
  `oauth.{enabled,clientId,clientSecret,authorizationUrl,tokenUrl,scopes,redirectUri}`, tokens in
  `~/.gemini/mcp-oauth-tokens.json`, `/mcp auth <server>`. CLI: `gemini mcp
  add|remove|list|enable|disable` with `-s/--scope` (user|project), `-t/--transport` (stdio|sse|http),
  `-e/--env`, `-H/--header`, `--timeout`, `--trust`, `--description`, `--include-tools`,
  `--exclude-tools`. https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/mcp-server.md
- **Hooks** — all 11 events verified in source: `SessionStart`, `SessionEnd`, `BeforeAgent`,
  `AfterAgent`, `BeforeModel`, `AfterModel`, `BeforeToolSelection`, `BeforeTool`, `AfterTool`,
  `PreCompress`, `Notification`. Configured under `hooks` in settings.json; each entry is
  `{matcher?, sequential?, hooks:[{type:'command', command, name?, timeout? (60000), description?}]}`.
  stdin JSON: `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `timestamp`. stdout JSON
  supports `systemMessage`, `suppressOutput`, `continue`, `stopReason`, `decision`
  (`allow`|`deny`, alias `block`), `reason`, and `hookSpecificOutput` (`tool_input`,
  `additionalContext`, `llm_request`, `llm_response`, `toolConfig.mode` `AUTO|ANY|NONE` +
  `allowedFunctionNames`, `clearContext`, `tailToolCallRequest`). Exit codes: 0 = success, 2 = system
  block, other = warning. Env: `GEMINI_PROJECT_DIR`, `GEMINI_PLANS_DIR`, `GEMINI_SESSION_ID`,
  `GEMINI_CWD`, plus a `CLAUDE_PROJECT_DIR` alias. Toggles `hooksConfig.enabled` /
  `hooksConfig.notifications`; `/hooks list|enable|disable|enable-all|disable-all`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md
- **Extensions** — `gemini extensions
  install|uninstall|list|update|enable|disable|link|new|validate|config`; manifest
  `gemini-extension.json` (`name`, `version`, `description`, `mcpServers`, `contextFileName`,
  `excludeTools` e.g. `run_shell_command(rm -rf)`, `migratedTo`, `plan.directory`, `settings[]` with
  `envVar`/`sensitive`, `themes[]`). Contributes `commands/`, `hooks/hooks.json`, `skills/`,
  `agents/`, `policies/` (tier 2 — extension `allow` decisions and `yolo` are **ignored** for
  security). Variables `${extensionPath}`, `${workspacePath}`, `${/}`. Gating:
  `security.blockGitExtensions`, `security.allowedExtensions`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/extensions/reference.md
- **Skills** — `SKILL.md` directories; precedence built-in < extension < user (`~/.gemini/skills/`,
  `~/.agents/skills/`) < workspace (`.gemini/skills/`, `.agents/skills/`). Activated via the
  `activate_skill` tool behind a consent prompt that also adds the skill dir to allowed paths.
  `skills.enabled` (default true). `gemini skills list|install|link|uninstall|enable|disable`;
  `/skills list|enable|disable|reload|link`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/skills.md
- **Custom commands** — TOML in `.gemini/commands/`, with subdirectories namespacing (`/gcs:sync`).
  Required `prompt`, optional `description`; `{{args}}` injection, `!{...}` shell execution (with
  confirmation), `@{...}` file injection.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/custom-commands.md

## 1.12 Non-interactive / headless / CI / scriptability

- `-p`/`--prompt` forces non-interactive and is appended to stdin input. A positional `query` now
  defaults to **interactive** in a TTY. `-i`/`--prompt-interactive` executes then stays interactive.
  Headless auto-triggers on non-TTY stdout/stdin or `CI=true`/`GITHUB_ACTIONS=true`, unless
  `GEMINI_CLI_INTEGRATION_TEST=true`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/utils/headless.ts
- `-o`/`--output-format` choices: `text`, `json`, `stream-json`.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/config/config.ts
- **`json` shape:** `{session_id?, response?, stats?, error?, warnings?}` with error
  `{type, message, code?}`. In `json` mode all non-response output is forced to stderr
  (`forceToStderr = outputFormat === 'json'`).
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/output/types.ts
- **`stream-json` is JSONL**, every event carrying `type` + `timestamp`: `init{session_id, model}`,
  `message{role, content, delta?}`, `tool_use{tool_name, tool_id, parameters}`,
  `tool_result{tool_id, status: 'success'|'error', output?, error?}`, `error{severity, message}`,
  `result{status, error?, stats?}`. `stats` =
  `{total_tokens, input_tokens, output_tokens, cached, input, duration_ms, tool_calls, models}` with
  per-model breakdowns.
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/output/types.ts
- **Exit codes (verified with class names):** 0 `SUCCESS`; 41 `FatalAuthenticationError`; 42
  `FatalInputError`; 44 `FatalSandboxError`; 52 `FatalConfigError`; 53 `FatalTurnLimitedError`; 54
  `FatalToolExecutionError`; 55 `FatalUntrustedWorkspaceError`; 130 `FatalCancellationError` (SIGINT).
  A yargs parse failure exits 1, and EPIPE on stdout exits 0. **The official
  `docs/cli/headless.md` lists only 0/1/42/53 — it is incomplete.**
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/utils/exitCodes.ts ·
  https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/utils/errors.ts
- Also: `--include-directories`, `--debug`/`-d` ("open debug console with F12"),
  `/stats session|model|tools`, `--allowed-mcp-server-names`, `--extensions`, `--raw-output` +
  `--accept-raw-output-risk`, and hidden test flags `--fake-responses`,
  `--fake-responses-non-strict`, `--record-responses`.
- **CI integration:** the official GitHub Action is `google-github-actions/run-gemini-cli` (PR
  reviews, issue triage, `@gemini-cli` mentions), plus a `/setup-github` slash command.
  https://github.com/google-gemini/gemini-cli/blob/main/README.md
- **ACP / IDE:** `--acp` starts ACP mode (`--experimental-acp` deprecated in source). JSON-RPC 2.0
  over stdio with methods `initialize`, `authenticate`, `newSession`, `loadSession`, `prompt`,
  `cancel`, `setSessionMode`, `unstable_setSessionModel`, plus a proxied file-system service.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/acp-mode.md
  **Stale doc:** `docs/cli/cli-reference.md` still lists `--experimental-zed-integration`, which is
  absent from source.

## 1.13 Cost & token control

- **No OpenAI-compatible provider exists.** A repo-wide search for "openai" hits only a redaction
  regex in `docs/hooks/best-practices.md` (and one browser test). `docs/examples/proxy-script.md`
  documents `GEMINI_SANDBOX_PROXY_COMMAND` **network egress filtering**, not LLM proxying. Generic
  base-URL overrides are `GOOGLE_GEMINI_BASE_URL` (gemini-api-key) and `GOOGLE_VERTEX_BASE_URL`
  (vertex-ai).
  https://github.com/google-gemini/gemini-cli/blob/main/docs/reference/configuration.md
- **Model precedence:** `--model`/`-m` > `GEMINI_MODEL` > `model.name` > local Gemma router >
  default `auto`. Aliases: `auto`, `pro`, `flash`, `flash-lite`. `/model manage`, `/model set
  <model> [--persist]`. **`--model` does not override subagent models.**
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/model-routing.md
- **"Model routing" is failure fallback, not complexity classification** — `ModelAvailabilityService`
  switches to a fallback model on quota/server errors, prompting by default. True classifier-based
  routing is the **experimental local Gemma router**:
  `experimental.gemmaModelRouter.enabled`, `.classifier.host`, `.classifier.model`
  (`"gemma3-1b-gpu-custom"`) — local Gemma classifies simple → Flash / complex → Pro, with silent
  cloud fallback. Commands `gemini gemma setup|status|start|stop|logs`, plus `/gemma`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/core/local-model-routing.md ·
  https://github.com/google-gemini/gemini-cli/blob/main/docs/core/gemma-setup.md
- `general.plan.modelRouting` switches Pro/Flash based on plan-mode state. **Model steering**
  (`experimental.modelSteering`) lets typing while the agent works inject a hint into the next turn.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/model-steering.md
- **Token caching is implicit and only for API key + Vertex AI** — explicitly **not** for
  OAuth/Code Assist: "the Code Assist API does not support cached content creation". `/stats` shows
  cached-token savings. No `cachedContent` setting or explicit-cache config is documented —
  **unverified**. https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/token-caching.md
- **Quota:** Google-account free tier 1,000 requests/user/day (README also states 60 req/min); Gemini
  API key free 250/day Flash-only; Vertex AI Express 90 days; Code Assist Standard 1,500 / Enterprise
  2,000. `billing.overageStrategy` = `ask`|`always`|`never`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/resources/quota-and-pricing.md
- Auth selection keys: `security.auth.selectedType|enforcedType|useExternal`.

## 1.14 Observability

- **Telemetry** settings and env overrides: `enabled`/`GEMINI_TELEMETRY_ENABLED` (default `false`),
  `traces`/`GEMINI_TELEMETRY_TRACES_ENABLED`, `target`/`GEMINI_TELEMETRY_TARGET` (`gcp`|`local`,
  default `local`), `otlpEndpoint`/`GEMINI_TELEMETRY_OTLP_ENDPOINT` (default
  `http://localhost:4317`), `otlpProtocol`/`GEMINI_TELEMETRY_OTLP_PROTOCOL` (`grpc`|`http`),
  `outfile`/`GEMINI_TELEMETRY_OUTFILE` (overrides the endpoint, e.g. `.gemini/telemetry.log`),
  `logPrompts`/`GEMINI_TELEMETRY_LOG_PROMPTS` (default `true`), `useCollector`, `useCliAuth`;
  `OTLP_GOOGLE_CLOUD_PROJECT`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/telemetry.md
- Metric names include `gemini_cli.tool.call.count`, `gemini_cli.tool.call.latency`,
  `gemini_cli.api.request.count`, `gemini_cli.api.request.latency`, `gemini_cli.token.usage`,
  `gemini_cli.file.operation.count`, `gemini_cli.lines.changed`, `gemini_cli.session.count`. Log
  events include `gemini_cli.config`, `gemini_cli.user_prompt`, `gemini_cli.tool_call`,
  `gemini_cli.api_request|response|error`, `gemini_cli.model_routing`, `gemini_cli.hook_call`,
  `gemini_cli.rewind`, `gemini_cli.conseca.verdict`. Prompt content is excluded when
  `telemetry.logPrompts` is `false`.
- Debugging: `--debug`/`-d` (F12 console), `DEBUG=1`, `general.debugKeystrokeLogging`,
  `general.logRagSnippets`, and `--acp --debug`.

## 1.15 Evaluation & regression testing

- **Behavioral evals ship in-repo** as vitest files `evals/*.eval.ts` with policies
  `ALWAYS_PASSES`/`USUALLY_PASSES`/`USUALLY_FAILS`. Scripts: `eval:inventory`, `eval:validate`,
  `eval:report`, `eval:coverage`, `test:always_passing_evals`, `test:all_evals` (`RUN_EVALS=1`), or
  directly `RUN_EVALS=true npx vitest run evals/<name>.eval.ts`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/behavioral-evals.md
- Integration tests: `npm run test:e2e`, `test:integration:all` (`test:integration:sandbox:none|
  docker|podman`), `test:memory`, `test:perf`, `npm run deflake -- --runs=5`, with env knobs
  `REGENERATE_MODEL_GOLDENS`, `VERBOSE`, `KEEP_OUTPUT`, `UPDATE_MEMORY_BASELINES`,
  `UPDATE_PERF_BASELINES`.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/integration-tests.md
- Release gating references a Google-internal dashboard `go/gemini-cli-offline-evals-dash` and a bug
  bash ≥ 72 h before Tier 1/Tier 2 releases.
  https://github.com/google-gemini/gemini-cli/blob/main/docs/release-confidence.md
- **No SWE-bench harness in this repo.** "SWE-bench" appears only in code comments (e.g.
  `packages/core/src/prompts/snippets.ts`: "you must run the major benchmarks, such as SWEBench,
  prior to committing any changes to the Context Efficiency section"; `grep-utils.ts`: "~10% in
  SWEBench") and `evals/README.md` explicitly states the behavioral evals "are also distinct from
  broad industry benchmarks (like SWE-bench)".

## 1.16 Verified to NOT exist in Gemini CLI

Each of these was checked by repo-wide search and returned no hit: `allowBuildArtifacts`; a top-level
`coreTools` settings key (only an internal `Config` field); a top-level `excludeTools` settings key
(only MCP-server-level and extension-manifest-level); a `sandboxImage` settings key (it is
`tools.sandbox.image`); `maxSubagentDepth`; a `list_agents` tool; the `save_memory` tool;
`systemPromptMds`; `/memory add` and `/memory refresh`; `read_many_files` as a model-callable tool;
`--experimental-zed-integration` (documented but absent from source); `--checkpointing` (removed in
0.11.0); any OpenAI-compatible LLM provider; any scheduled/cron agent-task feature; any unified-diff
or AST-based editing tool; any session forking/branching.

*Correction to an earlier research pass:* `SEATBELT_PROFILE=restrictive-closed` **does** exist — see
§1.6.

---

# 2. OpenAI Codex CLI (openai/codex)

Codex is a **Rust** workspace (`codex-rs/**`) plus a TypeScript SDK and a Python SDK. Source-code
citations below are the authoritative record for behaviour; the docs site is authoritative for
intent.

## 2.1 Agent loop & tool-calling protocol

- **Multi-turn, and the turn loop is explicit.** `RegularTask::run` delegates to `run_turn`, which
  contains a `loop { … }` where each iteration is one sampling request. The protocol is stated in a
  source doc comment: "either requested function calls / an assistant message … in practice, we
  generally one item per sampling request".
  https://github.com/openai/codex/blob/main/codex-rs/core/src/session/turn.rs
- **Parallel tool calls: yes.** Multiple function calls in one model response become independent
  futures held in a `FuturesOrdered`, drained by `drain_in_flight`
  (`while let Some(res) = in_flight.next().await`). Results are therefore consumed in arrival order.
  https://github.com/openai/codex/blob/main/codex-rs/core/src/session/turn.rs *(verified directly:
  `FuturesOrdered` imported at L129, `drain_in_flight` L2325, `in_flight.push_back(tool_future)`
  L2605)*
- **Concurrency is per-tool, gated by a read/write lock.** `ToolCallRuntime::handle_tool_call_with_source`
  takes either a shared read lock (parallel-capable) or an exclusive write lock (serialized) over an
  `Arc<RwLock<()>>`. The predicate is `ToolRouter::tool_supports_parallel` →
  `ToolRegistry::supports_parallel_tool_calls`; the **default is `false`**, and only tools that
  override it (`exec_command`, `write_stdin`, `view_image`, `tool_search`, MCP tools, resource tools,
  extension tools) actually run concurrently.
  https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/parallel.rs ·
  https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/registry.rs ·
  https://github.com/openai/codex/blob/main/codex-rs/tools/src/tool_executor.rs
- **Stop conditions:** `ResponseEvent::Completed { end_turn, .. }` sets `needs_follow_up = true` when
  `end_turn == false`; any tool call also sets `needs_follow_up = true`. The turn ends when
  `needs_follow_up == false`. `run_turn_stop_hooks` can additionally block continuation or stop the
  turn. A reached token limit triggers `run_auto_compact(..., CompactionReason::ContextLimit,
  CompactionPhase::MidTurn)` and continues. Interruption uses a `CancellationToken` yielding
  `CodexErr::TurnAborted` → `EventMsg::TurnAborted`; `TurnAbortReason` = `Interrupted`, `Replaced`,
  `ReviewEnded`, `BudgetLimited`.
  https://github.com/openai/codex/blob/main/codex-rs/core/src/session/turn.rs ·
  https://github.com/openai/codex/blob/main/codex-rs/core/src/stream_events_utils.rs ·
  https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs
- **There is no `max_turns` limit anywhere in `codex-rs`** (zero source hits) — unlike Gemini CLI's
  `MAX_TURNS = 100`. Codex bounds work through context/token limits and budgets instead.
- **Tool results are fed back as `ResponseInputItem::FunctionCallOutput`.** Recoverable tool failures
  return `FunctionCallError::RespondToModel(msg)` as a normal function-call output (so the model sees
  the error and can retry); only `FunctionCallError::Fatal` aborts the turn.
  https://github.com/openai/codex/blob/main/codex-rs/core/src/stream_events_utils.rs

## 2.2 Tool set & edit strategy

- **Tool names sent to the model** (from the registry assembly in `spec_plan.rs`): `exec_command` +
  `write_stdin` (the "unified exec" pair; there is also a legacy `shell` tool), **`apply_patch`**,
  `update_plan`, `view_image`, `web_search` (**a hosted/server-side tool spec, not a function tool**),
  `tool_search`, `list_mcp_resources`, `list_mcp_resource_templates`, `read_mcp_resource`,
  `request_permissions`, `request_user_input`, `request_user_input_async`,
  `send_message_to_user_async`, `get_context_remaining`, `new_context_window`, `sleep`,
  `wait_for_environment`, `current_time`, the code-mode freeform `exec` + `wait`, all MCP server
  tools, and multi-agent tools (§2.9).
  https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/spec_plan.rs
- **Notable absence: Codex has no core `read_file` / `write_file` / `grep` / `glob` file tools.**
  Reading and editing happen through `exec_command` (shell) and `apply_patch`. `read_file` /
  `write_file` / `search_contents` / `append_to_file` exist only as `notes`-namespace **extension**
  tools.
  https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/extension_tools.rs
- **Edit strategy: a custom freeform `apply_patch` tool driven by a Lark grammar (V4A), not JSON and
  not a search/replace pair.** The spec loads `include_str!(".../assets/tools/apply_patch.lark")`.
  Grammar: `start: begin_patch hunk+ end_patch`, `hunk: add_hunk | delete_hunk | update_hunk`, with
  markers `"*** Begin Patch"`, `"*** End Patch"`, `"*** Add File: "`, `"*** Delete File: "`,
  `"*** Update File: "`, `"*** Move to: "`, `"*** End of File"`, hunk context `"@@"` / `"@@ "`, and
  change lines `("+" | "-" | " ") /(.*)/ LF`. An optional `"*** Environment ID: "` line is injected
  only when multiple environments are active.
  https://github.com/openai/codex/blob/main/codex-rs/core/assets/tools/apply_patch.lark ·
  https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/apply_patch_spec.rs
  *(verified: the grammar file exists and markers appear verbatim in
  `codex-rs/apply-patch/src/*` tests)*
- Applying the patch is a separate, well-tested crate: `codex-rs/apply-patch/src/parser.rs`
  (`parse_patch`, `Hunk::{AddFile,DeleteFile,UpdateFile}`), `file_update.rs`
  (`derive_new_contents_from_chunks`, `unified_diff_from_chunks`), `seek_sequence.rs`,
  `streaming_parser.rs`, and `invocation.rs` (which also recognizes the legacy
  `apply_patch <<EOF` shell form via tree-sitter-bash heredoc extraction;
  `APPLY_PATCH_COMMANDS = ["apply_patch","applypatch"]`). A standalone `--codex-run-as-apply-patch`
  argv flag exists. https://github.com/openai/codex/blob/main/codex-rs/apply-patch/src/parser.rs
- **`apply_patch` does NOT run in parallel** — `ApplyPatchHandler` never overrides
  `supports_parallel_tool_calls`, so it inherits `false` and serializes on the write lock.
- **No AST-based editing.** `tree-sitter` is used only for patch application, execpolicy shell-script
  splitting, and embedded shell grammars — not for semantic code edits.

## 2.3 Context management

- **Token accounting** is centralised in `context_window_token_status`, producing
  `ContextWindowTokenStatus { active_context_tokens, auto_compact_scope_tokens,
  auto_compact_scope_limit, full_context_window_limit, base_window_tokens_remaining,
  auto_compact_window_prefill_tokens, full_context_window_limit_reached, token_limit_reached }`,
  with `active_context_tokens = sess.get_total_token_usage()`.
  https://github.com/openai/codex/blob/main/codex-rs/core/src/session/context_window.rs
- **Compaction is configurable and multi-strategy:** `run_auto_compact` dispatches to a token-budget
  compactor, a remote V2 compactor, or the local `compact.rs`. Reasons are
  `CompactionReason { UserRequested, ContextLimit, ModelDownshift, CompHashChanged }`; phases are
  `PreTurn` / `MidTurn`. The compaction prompt template is
  `codex-rs/prompts/templates/compact/prompt.md` ("You are performing a CONTEXT CHECKPOINT
  COMPACTION…"). `/compact` is a first-class slash command ("summarize conversation to prevent
  hitting the context limit"); `/recap` summarizes the current conversation on demand.
  https://github.com/openai/codex/blob/main/codex-rs/core/src/session/turn.rs ·
  https://github.com/openai/codex/blob/main/codex-rs/tui/src/slash_command.rs
- Config keys: `model_context_window`, `model_auto_compact_token_limit`, and
  `model_auto_compact_token_limit_scope` (`total` | `body_after_prefix`, the latter subtracting
  `prefill_input_tokens`). https://learn.chatgpt.com/docs/config-file/config-reference.md
- **Ignore rules: there is NO `.codexignore`** (zero hits repo-wide). Ignore semantics come from
  `.gitignore` / `.ignore` via the `ignore` crate in `codex-rs/file-search/`.
  https://github.com/openai/codex/blob/main/codex-rs/file-search/src/lib.rs
- **`@` file references are NOT a core feature.** `core/src/mention_syntax.rs` only re-exports plugin
  and tool sigils — `TOOL_MENTION_SIGIL = '$'`, `PLUGIN_TEXT_MENTION_SIGIL = '@'` — so `@` means a
  *plugin* mention, not a file. The TUI exposes `/mention` ("mention a file") and images are attached
  with `--image`/`-i`. https://github.com/openai/codex/blob/main/codex-rs/utils/plugins/src/mention_syntax.rs
  *(Whether the TUI `/mention` inlines file contents is **unverified** at source level.)*

## 2.4 System prompt & project rule files (AGENTS.md)

- **`AGENTS.md` is the project rule file.** Constants: `DEFAULT_AGENTS_MD_FILENAME = "AGENTS.md"`,
  `LOCAL_AGENTS_MD_FILENAME = "AGENTS.override.md"`. The source documents the algorithm: user-level
  instructions are prepended; the project root is located via `project_root_markers`, then the tree is
  walked root→cwd collecting every match.
  https://github.com/openai/codex/blob/main/codex-rs/core/src/agents_md.rs
- **Precedence:** global (`CODEX_HOME/AGENTS.override.md` else `AGENTS.md`, first non-empty only) →
  project walk root→cwd, checking per directory `AGENTS.override.md` → `AGENTS.md` →
  `project_doc_fallback_filenames`, **at most one file per directory** → concatenated root→cwd so
  closer files win, stopping at the combined `project_doc_max_bytes` cap (schema default **32768**
  bytes). `project_doc_fallback_filenames` defaults to `[]`, so **`AGENT.md` and `agents.md` are not
  defaults** — you must add them. Built once per run.
  https://learn.chatgpt.com/docs/agent-configuration/agents-md.md ·
  https://github.com/openai/codex/blob/main/codex-rs/core/config.schema.json
- `/init` scaffolds an `AGENTS.md`. There is no dedicated "disable AGENTS.md" flag.
  https://learn.chatgpt.com/docs/agent-configuration/agents-md.md
- **Config precedence:** `~/.codex/config.toml`, relocated by `CODEX_HOME`; project layer
  `.codex/config.toml` walked root→cwd (closest wins) and **loaded only when the project is
  trusted**. Project layers deliberately ignore `openai_base_url`, `chatgpt_base_url`,
  `model_provider`, `model_providers`, `notify`, `profile`, `profiles`, `otel`, `apps_mcp_product_sku`,
  and `experimental_realtime_ws_base_url`. Managed policy lives in `requirements.toml`.
  https://learn.chatgpt.com/docs/config-file/config-reference.md
- The generated `codex-rs/core/config.schema.json` (title `ConfigToml`, **98 top-level keys**,
  verified locally) is the most reliable config surface, because the prose docs drift. Repo `AGENTS.md`
  instructs contributors to run `just write-config-schema` after changing `ConfigToml`.
  https://github.com/openai/codex/blob/main/codex-rs/core/config.schema.json ·
  https://github.com/openai/codex/blob/main/AGENTS.md

## 2.5 Permissions & safety

- **Approval policies** (`AskForApproval`): `"untrusted"` (internal, for untrusted projects —
  commands need approval unless an execpolicy rule allows them), `"on-request"` (**default**;
  `"on-failure"` survives only as a serde *alias*), `"granular"` (a `GranularApprovalConfig` with
  `sandbox_approval`, `rules`, `skill_approval`, `request_permissions`, `mcp_elicitations`), and
  `"never"`.
  https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs *(verified verbatim)*
- **Sandbox modes** (`sandbox_mode` / `--sandbox` / `-s`): `read-only`, `workspace-write`,
  `danger-full-access`.
  https://github.com/openai/codex/blob/main/codex-rs/utils/cli/src/sandbox_mode_cli_arg.rs
- **CLI safety flags:** `--dangerously-bypass-approvals-and-sandbox` (alias `--yolo`), and
  `--approve-for-me` (alias `--not-so-yolo`, which conflicts with `--sandbox`/`--yolo`), plus
  `--add-dir`, `--cd`/`-C`, `--worktree`, `--dangerously-bypass-hook-trust`.
  https://github.com/openai/codex/blob/main/codex-rs/utils/cli/src/shared_options.rs
- **Exec policy (`execpolicy`) is Codex's command allowlisting layer:** decisions are
  `enum Decision { Allow, Prompt, Forbidden }` (parsed from lowercase `"allow"|"prompt"|"forbidden"`),
  expressed in a **Starlark** dialect via builtins `prefix_rule(pattern, decision, match, not_match,
  justification)` (decision defaults to `"allow"`) and `network_rule(host, protocol, decision,
  justification)`. Rule files are discovered in a `rules/` directory with extension `.rules` and a
  `default.rules` file; `codex execpolicy check` validates them (`--pretty`, repeatable `--rules`).
  https://github.com/openai/codex/blob/main/codex-rs/execpolicy/src/decision.rs ·
  https://github.com/openai/codex/blob/main/codex-rs/execpolicy/src/parser.rs ·
  https://learn.chatgpt.com/docs/agent-configuration/rules.md
- **Auto-review** routes sandbox-boundary approval prompts to a reviewer agent instead of the user:
  `--sandbox workspace-write --ask-for-approval on-request -c approvals_reviewer=auto_review`, with
  `approvals_reviewer` = `user` | `auto_review` | `guardian_subagent`.
  https://learn.chatgpt.com/docs/sandboxing/auto-review.md

## 2.6 Sandbox implementation

- Platform mechanisms are selected by `enum SandboxType { None, MacosSeatbelt, LinuxSeccomp,
  WindowsRestrictedToken }` with `SandboxablePreference { Auto, Require, Forbid }`.
  https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/manager.rs
- **macOS:** invokes `/usr/bin/sandbox-exec` with `-p <policy>`, composing
  `seatbelt_base_policy.sbpl`, `seatbelt_network_policy.sbpl`, `seatbelt_preferences_policy.sbpl`, and
  `seatbelt_read_only_platform_defaults.sbpl`. The base policy is closed by default (`(deny default)`)
  with Chrome-derived allow rules. https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/seatbelt.rs
- **Linux/WSL2:** a two-stage helper under `codex-rs/linux-sandbox/` — **bubblewrap** builds the
  filesystem view, then an inner stage runs `--apply-seccomp-then-exec` to apply `no_new_privs` +
  seccomp. The seccomp deny list blocks `ptrace`, `process_vm_readv/writev`, `io_uring_setup/enter/
  register`, and (when network is off) `connect`/`accept`/`bind`/`listen`/`sendto`/`sendmmsg` etc.
  **Landlock is labelled in-file as a legacy/backup utility**, not the primary mechanism. The bundled
  bwrap binary is sha256-pinned.
  https://github.com/openai/codex/blob/main/codex-rs/linux-sandbox/src/landlock.rs ·
  https://github.com/openai/codex/blob/main/codex-rs/linux-sandbox/src/bundled_bwrap.rs
- **Windows:** two backends — an elevated MXC/PSEC-based sandbox and an unelevated restricted-token
  sandbox, configured by `WindowsSandboxFilesystemOverrides`.
  https://github.com/openai/codex/blob/main/codex-rs/sandboxing/src/windows.rs
- **`workspace-write` protects sensitive subpaths:** `WritableRoot { root, read_only_subpaths }`
  keeps things like `.codex` and `.git/hooks` read-only even inside a writable root. Other
  `workspace-write` fields: `writable_roots`, `network_access`, `exclude_tmpdir_env_var`,
  `exclude_slash_tmp`.
  https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs
- A `codex sandbox` subcommand runs an arbitrary command inside the Codex sandbox.
  https://github.com/openai/codex/blob/main/codex-rs/cli/src/main.rs

## 2.7 Session persistence, resume, fork

- **Rollout files:** `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-YYYY-MM-DDThh-mm-ss-<uuid>.jsonl`
  (optionally `.jsonl.zst`-compressed); archived sessions move to a separate archived directory; a
  SQLite thread store also exists. Recorded items include `SessionMeta`, `ResponseItem`,
  `InterAgentCommunication`, `Compacted`, `TurnContext`, `TokenUsageRecord`, `WorldState`,
  `SecurityRiskScore`, `RetainedContext`, `EventMsg`.
  https://github.com/openai/codex/blob/main/codex-rs/rollout/src/lib.rs ·
  https://github.com/openai/codex/blob/main/codex-rs/history/src/lib.rs
- **Resume:** `codex resume [SESSION_ID]`, `codex resume --last`, `--all`,
  `--include-non-interactive`; `codex exec resume` with `--last`; TUI `/resume`. `tui.resume_cwd` =
  `current|session`.
  https://github.com/openai/codex/blob/main/codex-rs/cli/src/main.rs
- **Fork exists as a first-class command:** `codex fork` (picker by default, `--last` to fork the most
  recent), the TUI `/fork`, and Esc-Esc on an empty composer to fork from the previous user message.
  Internally `InitialHistory::{New, Cleared, Resumed(..), Forked(Vec<RolloutItem>)}`; forks record
  `forked_from_id` and can inherit a frozen rollout prefix (`ForkPersistence::Copied`). Spawn-time
  forking accepts `fork_turns` = `none` | `all` | a positive integer.
  https://github.com/openai/codex/blob/main/codex-rs/history/src/lib.rs ·
  https://github.com/openai/codex/blob/main/codex-rs/core/src/thread_manager.rs
  *(verified: `Subcommand::Fork` and `--worktree` interaction checks in `cli/src/main.rs`)*
- **There is no `--continue` flag** (zero source hits); `--last` is the equivalent. Other session
  subcommands: `codex queue` (queue a message for an existing session), `codex archive`,
  `codex unarchive`, `codex delete`, `codex migrate-rollouts`.
  https://github.com/openai/codex/blob/main/codex-rs/cli/src/main.rs

## 2.8 Plan mode, todos, goal tracking

- **Plan mode and the todo tool are two different features, and this is stated explicitly in the
  prompt template:** "`update_plan` is a checklist/progress/TODOs tool; it does not enter or exit Plan
  Mode". Calling it in plan mode errors: "update_plan is a TODO/checklist tool and is not allowed in
  Plan mode".
  https://github.com/openai/codex/blob/main/codex-rs/collaboration-mode-templates/templates/plan.md ·
  https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/plan.rs
- **Plan mode** is entered with `/plan` (or Shift+Tab) and accepts inline prompt text; rendering is
  driven by `ModeKind::{Plan, Default}`. A `plan_mode_reasoning_effort` config key overrides reasoning
  effort in plan mode.
  https://github.com/openai/codex/blob/main/codex-rs/tui/src/slash_command.rs ·
  https://learn.chatgpt.com/docs/config-file/config-reference.md
- **The `update_plan` tool is opt-in and OFF by default.** It is registered only when
  `turn_context.config.update_plan_enabled`, resolved from `tools.update_plan.enabled` whose schema
  default is `false`. There is **no `--include-plan-tool` flag**. Args are `{explanation?, plan[] of
  {step, status}}` with statuses **`pending`, `in_progress`, `completed`** and the constraint "At most
  one step can be in_progress at a time."
  https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/plan_spec.rs ·
  https://github.com/openai/codex/blob/main/codex-rs/core/config.schema.json
- **Goal tracking is a real first-class tool family:** `get_goal`, `create_goal`, `update_goal`
  (where `update_goal` accepts only actions `complete`, `blocked`, `paused` — resuming is
  user/system-controlled), plus `/goal` in the TUI, `[goals] max_goal_token_budget`, and app-server
  `thread/goal/set|get|clear`. https://github.com/openai/codex/blob/main/codex-rs/ext/goal/src/spec.rs ·
  https://learn.chatgpt.com/docs/developer-commands.md?surface=cli

## 2.9 Subagents & parallel orchestration

- **Multi-agent tools are enabled by default** (`features.multi_agent` is `Stage::Stable`,
  `default_enabled: true`; `agents.enabled` defaults `true`).
  https://github.com/openai/codex/blob/main/codex-rs/features/src/lib.rs
- **Two tool surfaces exist.** V1 is a namespace `multi_agent_v1` with `spawn_agent`, `send_input`,
  `wait_agent`, `resume_agent`, `close_agent`. V2 uses flat names: `spawn_agent`, `send_message`,
  `followup_task`, `interrupt_agent`, `list_agents` (plus `wait_agent` when
  `multi_agent_v2.wait_agent_enabled`). **V2 is `default_enabled: false`.**
  https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/multi_agents_spec.rs
- **Concurrency caps are explicit constants** (verified in source):
  `DEFAULT_AGENT_MAX_THREADS = Some(6)`, `DEFAULT_MULTI_AGENT_V2_MAX_CONCURRENT_THREADS_PER_SESSION = 4`,
  and `DEFAULT_AGENT_MAX_DEPTH = 1`. V2's effective limit is
  `max_concurrent_threads_per_session - 1` (the primary is excluded). Exceeding the cap fails with
  `CodexErrorDetails::AgentLimitReached { max_threads }` from `reserve_spawn_slot`.
  https://github.com/openai/codex/blob/main/codex-rs/core/src/config/mod.rs ·
  https://github.com/openai/codex/blob/main/codex-rs/core/src/agent/registry.rs
- Config: `[agents] enabled`, `max_concurrent_threads_per_session` (legacy alias `max_threads`),
  `max_depth` (V1 only; ignored by V2), `default_subagent_model`,
  `default_subagent_reasoning_effort`, `interrupt_message`.
  https://learn.chatgpt.com/docs/agent-configuration/subagents.md
- **Custom agents are standalone TOML files** discovered in `<config_folder>/agents` — i.e.
  `~/.codex/agents/` and `.codex/agents/`. Required fields are `name`, `description`, and
  `developer_instructions` (source hard-errors on a blank/missing `developer_instructions`); the
  `name` field is the source of truth. Other `config.toml` keys such as `model`,
  `model_reasoning_effort`, `sandbox_mode`, `mcp_servers`, and `skills.config` may be included and
  otherwise inherit from the parent. Built-in agents include `default`, `worker`, and `explorer`.
  https://github.com/openai/codex/blob/main/codex-rs/agent-roles/src/loader.rs ·
  https://learn.chatgpt.com/docs/agent-configuration/subagents.md
- **`wait_agent` genuinely fans out** over its targets using `FuturesUnordered` + `timeout_at`.
  Subagents inherit the parent's sandbox and approval policy, and live turn overrides are reapplied on
  spawn. https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/multi_agents/wait.rs

## 2.10 Extensibility

- **MCP** — transport is **inferred, not declared**: `command` → stdio, `url` → streamable HTTP. Only
  `Stdio` and `StreamableHttp` (plus an in-process transport) exist; **SSE is not supported**. Full
  `mcp_servers.<id>` field set includes `command`, `args`, `env`, `env_vars`, `cwd`, `url`,
  `auth` (`oauth`|`chatgpt`), `oauth.{client_id,callback_url,callback_port}`,
  `bearer_token_env_var`, `http_headers`, `env_http_headers`, `enabled`, `required`,
  `startup_timeout_sec`, `tool_timeout_sec`, `enabled_tools`, `disabled_tools` (applied *after*
  `enabled_tools`), `default_tools_approval_mode`, per-tool `tools.<tool>.approval_mode` and
  `.output_token_limit`, `scopes`, `oauth_resource`, `environment_id`,
  `supports_parallel_tool_calls`. Defaults: `startup_timeout_sec` = 10, `tool_timeout_sec` = 60,
  global `mcp_optional_startup_grace_ms` = 1000. A `required = true` server that fails to initialize
  makes `codex exec` exit with an error rather than continue.
  https://learn.chatgpt.com/docs/extend/mcp.md ·
  https://github.com/openai/codex/blob/main/codex-rs/rmcp-client/src/rmcp_client.rs
- `codex mcp` subcommands: `list`, `get`, `add`, `remove`, `login`, `logout` (`add` takes
  `--stdio`/`--streamable-http`). `codex mcp-server` is **deprecated** in favour of the app server.
  https://github.com/openai/codex/blob/main/codex-rs/cli/src/mcp_cmd.rs
- **Hooks — 12 lifecycle events:** `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`,
  `PostCompact`, `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `SubagentStart`, `SubagentStop`,
  `Stop`, `Interrupt`. Configured as `hooks.json` or inline `[hooks]` tables in `config.toml`,
  discovered beside active config layers (`~/.codex/hooks.json`, `~/.codex/config.toml`,
  `<repo>/.codex/...`); all matching sources merge rather than override, and project hooks load only
  when the project layer is trusted. Handler types: `command`, `mcp_tool`, `prompt`, `agent`.
  A `PreToolUse` hook can block a call via
  `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny",…}}`, or exit code
  2, and can rewrite a call with `permissionDecision:"allow"` + `updatedInput`. Oversized output is
  spilled to `<temp_dir>/hook_outputs/...` past ~2,500 tokens. `allow_managed_hooks_only = true` in
  `requirements.toml` ignores user/project/session hook configs while still loading managed hooks —
  and this setting is **only** honored in `requirements.toml`.
  https://learn.chatgpt.com/docs/hooks.md ·
  https://github.com/openai/codex/blob/main/docs/config.md
- **Skills** are directories containing `SKILL.md` with required frontmatter `name` and `description`,
  discovered in `.agents/skills` (cwd, parents, repo root), `$HOME/.agents/skills`, and
  `/etc/codex/skills`, plus bundled system skills. Progressive disclosure starts with name+description
  capped at 2% of the context window or 8,000 characters. Invoke via `/skills` or `$skill`. Note the
  directory is `.agents/skills`, **not** `.codex/skills`.
  https://learn.chatgpt.com/docs/build-skills.md
- **Custom prompts are deprecated** in favour of skills: Markdown files directly under
  `~/.codex/prompts/` (top-level only), YAML front matter `description:` / `argument-hint:`,
  placeholders `$1`–`$9`, `$ARGUMENTS`, named `KEY=value` arguments, `$$` for a literal `$`, invoked as
  `/prompts:<name>`. https://learn.chatgpt.com/docs/custom-prompts.md
- **Plugins** are installed via `/plugins`; model-facing tools `list_available_plugins_to_install`
  and `request_plugin_install` exist in source.
  https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/request_plugin_install.rs
- **Memories are OFF by default** (`[features] memories = true` enables them), stored under
  `~/.codex/memories/`, generated in the background with secrets redacted; config keys
  `memories.generate_memories`, `.use_memories`, `.disable_on_external_context`,
  `.min_rate_limit_remaining_percent`, `.extract_model`, `.consolidation_model`; `/memories` in the
  TUI. https://learn.chatgpt.com/docs/customization/memories.md

## 2.11 Checkpoints & undo — mostly absent

- **There is no `/undo`.** `SlashCommand` enumerates every slash command and contains no undo or
  checkpoint entry; the only "undo" in the TUI is Vim composer draft undo/redo.
  https://github.com/openai/codex/blob/main/codex-rs/tui/src/slash_command.rs
- **Automatic file snapshotting is dead.** The `undo` config flag maps to `Feature::GhostCommit` with
  `stage: Stage::Removed`, and `ghost_snapshot` is retained as "Compatibility-only settings… even
  though snapshots are no longer produced".
  https://github.com/openai/codex/blob/main/codex-rs/features/src/lib.rs ·
  https://github.com/openai/codex/blob/main/codex-rs/config/src/config_toml.rs *(both verified)*
- **The official guidance is to use Git manually:** "Create Git checkpoints before and after a task so
  you can revert changes".
  https://learn.chatgpt.com/docs/codex/cli.md
- **`thread/revert`** exists in the app server but is **transcript-only** — it does not restore files
  — and is not exposed as a TUI command.
  https://github.com/openai/codex/blob/main/codex-rs/thread-store/src/types.rs
- **What Codex does offer for isolation is Git worktrees:** the `codex-rs/worktree` crate, the
  `--worktree` flag, and `/worktree` ("start or continue a conversation in a new worktree").
  https://learn.chatgpt.com/docs/environments/git-worktrees.md
- `/diff` shows the git diff including untracked files.
  https://github.com/openai/codex/blob/main/codex-rs/tui/src/slash_command.rs
- *`features.shell_snapshot` snapshots the **shell environment** for speed, not files — do not
  confuse it with file checkpointing.*

## 2.12 Non-interactive / headless / CI

- **`codex exec`** (alias `codex e`) is the non-interactive entry point. **Its default sandbox is
  read-only** — automation must opt in. Progress streams to **stderr**; only the final agent message
  goes to **stdout**, which makes `codex exec "…" | tee out.md` work naturally.
  https://learn.chatgpt.com/docs/non-interactive-mode.md
- **Flags** (verified in `codex-rs/exec/src/cli.rs` and `shared_options.rs`): `--json` (alias
  `--experimental-json`), `--output-last-message`/`-o <path>`, `--output-schema <FILE>`,
  `--skip-git-repo-check`, `--ephemeral`, `--strict-config`, `--ignore-user-config`, `--ignore-rules`,
  `--color`, `--thread-source`, `--sandbox`/`-s`, `--image`/`-i`, `--model`/`-m`, `--oss`,
  `--local-provider`, `--profile`/`-p`, `--approve-for-me`, `--dangerously-bypass-approvals-and-sandbox`,
  `--cd`/`-C`, `--worktree`, `--add-dir`, and global `--enable`/`--disable FEATURE`, `-c key=value`.
  Subcommands: `codex exec resume`, `codex exec fork`, `codex exec review`. Prompt `-` reads stdin.
  https://github.com/openai/codex/blob/main/codex-rs/exec/src/cli.rs
- **`--full-auto` conflict (version skew).** Live docs say "Codex keeps `codex exec --full-auto` as a
  deprecated compatibility flag and prints a warning. Prefer the explicit `--sandbox
  workspace-write`", and the condensed manual lists it as "Deprecated compatibility flag". **But a
  case-insensitive search for `full.auto` across the entire clone at `ddea03ad` returns zero
  occurrences.** Marked as **version skew — the flag is absent from this commit but documented as
  existing**, so the live docs describe a different build. Do not rely on `--full-auto`; use
  `--sandbox workspace-write`.
  https://learn.chatgpt.com/docs/non-interactive-mode.md · contrast
  https://github.com/openai/codex/blob/main/codex-rs/exec/src/cli.rs
- **`--json` emits JSONL** with event types `thread.started{thread_id}`, `turn.started`,
  `turn.completed{usage}`, `turn.failed{error}`, `item.started`/`item.updated`/`item.completed{item}`,
  and `error{message}`. Item types include `agent_message`, `reasoning`, `command_execution`,
  `file_change`, `mcp_tool_call`, `collab_tool_call`, `web_search`, `todo_list`, `error`. `Usage` =
  `input_tokens`, `cached_input_tokens`, **`cache_write_input_tokens`**, `output_tokens`,
  `reasoning_output_tokens`.
  https://github.com/openai/codex/blob/main/codex-rs/exec/src/exec_events.rs
- **CI auth:** `CODEX_API_KEY` works with `codex exec`, `codex review`, the TypeScript SDK, and
  `codex exec-server --remote`. **There is no documented enumerated exit-code table**; non-zero on
  error, and child signal codes are propagated as 128+signal on Unix. A `required = true` MCP server
  that fails to initialize also makes `codex exec` fail.
  https://learn.chatgpt.com/docs/non-interactive-mode.md
- **`codex apply`** (alias `codex a`) still exists but "Apply the latest diff produced by Codex agent
  as a `git apply` to your local working tree" refers to a **Codex cloud** chat diff, not a local
  task's diff. https://github.com/openai/codex/blob/main/codex-rs/cli/src/main.rs
- Other automation surfaces: the GitHub Action `openai/codex-action` (accepts `codex-args`, emits a
  `final-message` output), the TypeScript SDK (`codex-sdk.md`), and `codex app-server` (JSONL over
  stdio, or `--listen ws://` / `unix://`, with `--experimental` to include gated fields).
  https://learn.chatgpt.com/docs/github-action.md ·
  https://learn.chatgpt.com/docs/app-server.md

## 2.13 Cost & token control

- **`TokenUsage`** fields: `input_tokens`, `cached_input_tokens`, **`cache_write_input_tokens`**,
  `output_tokens`, `reasoning_output_tokens`, `total_tokens`. Aggregates are reported as
  `TokenUsageInfo { total_token_usage, last_token_usage }`; the final message renders
  "Token usage: total=… input=… (+ N cached) output=… (reasoning N)".
  https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs
- **Rate limits are a first-class client concept**, carried alongside usage on
  `TokenCountEvent { info, rate_limits }`: `RateLimitSnapshot` has `primary`/`secondary`
  `RateLimitWindow`s (`used_percent`, `window_minutes`, `resets_at`), `credits`
  (`has_credits`, `unlimited`, `balance`), `individual_limit`, `spend_control_reached`, `plan_type`,
  and `rate_limit_reached_type` (including workspace owner/member credit-depleted variants).
  https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs
- **Cost is measurable via OpenTelemetry (source-only, undocumented in the config tables):**
  `record_turn_cost()` emits counter **`codex.turn.cost_microusd`** (tags `turn.id`,
  `conversation.id`, `turn.interrupted`, `speed`, `reasoning_effort`) plus
  `codex.turn.token_usage`; the TUI renders `estimated_usage_usd_micros`.
  https://github.com/openai/codex/blob/main/codex-rs/otel/src/metrics/names.rs *(verified)*
- **Prompt caching** is request-side via a `prompt_cache_key` field on the Responses payload; cache
  hits surface as `cached_input_tokens` / `cache_write_input_tokens`.
  https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/common.rs
- 429s are handled by a transport `RetryConfig { max_attempts, base_delay, retry_429, retry_5xx,
  retry_transport }`; notably most secondary endpoints set `retry_429: false`.
  https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/provider.rs
- Analytics can be disabled per machine with `[analytics] enabled = false`. Plan/credit economics
  (ChatGPT Work and Codex share usage, banked rate-limit resets, additional credits) are documented
  at https://learn.chatgpt.com/docs/pricing.md

## 2.14 Provider abstraction & multi-model support

- **`wire_api` accepts exactly one value: `"responses"`.** `"chat"` is **not** deprecated-but-working —
  it fails deserialization with: `` `wire_api = "chat"` is no longer supported. How to fix: set
  `wire_api = "responses"` in your provider config. More info:
  https://github.com/openai/codex/discussions/7782 `` The `ollama-chat` provider id was likewise
  removed (`OLLAMA_CHAT_PROVIDER_REMOVED_ERROR`). **Any "point Codex at an OpenAI-compatible chat
  completions endpoint" story is wrong at this commit.**
  https://github.com/openai/codex/blob/main/codex-rs/model-provider-info/src/lib.rs *(verified
  verbatim)*
- **Built-in provider ids (exhaustive):** `openai`, `amazon-bedrock`, `amazon-bedrock-runtime`,
  `ollama`, `lmstudio`. There is **no built-in id `oss`** — "oss" exists only as the `--oss` flag and
  the `oss_provider` key. `openai`, `ollama`, and `lmstudio` are reserved and cannot be overridden;
  the Bedrock ids permit only limited overrides (`base_url`, `auth`, `http_headers`, `aws.*`).
  https://github.com/openai/codex/blob/main/codex-rs/model-provider-info/src/lib.rs *(verified)*
- **`[model_providers.<id>]` fields** (`deny_unknown_fields`): `name`, `base_url`, `env_key`,
  `env_key_instructions`, `experimental_bearer_token`, `auth` (command-backed token:
  `command`, `args`, `timeout_ms`, `refresh_interval_ms`, `cwd`; mutually exclusive with `env_key`),
  `aws` (`profile`, `region`, `credential_export`, `auth_refresh`), `wire_api`, `query_params`,
  `http_headers`, `env_http_headers`, `request_max_retries` (default 4), `stream_max_retries`
  (default 5, user values capped at 100), `stream_idle_timeout_ms` (default 300000),
  `websocket_connect_timeout_ms`, `requires_openai_auth`, `supports_websockets`,
  `supports_standalone_web_search`.
  https://learn.chatgpt.com/docs/config-file/config-reference.md
- **To use an OpenAI-compatible endpoint** set `model_provider = "<id>"` plus a
  `[model_providers.<id>]` table with `base_url`/`env_key`; for the built-in OpenAI provider use
  `openai_base_url` instead (you cannot define `[model_providers.openai]`).
  https://learn.chatgpt.com/docs/config-file/config-reference.md
- **Local models:** `oss_provider = "lmstudio" | "ollama"`, selected per-run with `--local-provider`
  (and `--oss`, which also propagates to subcommands such as `codex exec --oss`). Base URLs are
  `http://localhost:11434/v1` (Ollama) and `http://localhost:1234/v1` (LM Studio), overridable with
  the experimental `CODEX_OSS_PORT` / `CODEX_OSS_BASE_URL`. With `--oss` and no provider chosen, the
  TUI prompts but **`codex exec` exits with an error**.
  https://github.com/openai/codex/blob/main/codex-rs/model-provider-info/src/lib.rs
- **Profiles changed shape — breaking:** `[profiles.<name>]` tables and the top-level
  `profile = "name"` selector were **removed in Codex 0.134.0**. Profiles are now separate layered
  files: `--profile <name>` overlays `~/.codex/<name>.config.toml` on top of `~/.codex/config.toml`.
  https://learn.chatgpt.com/docs/config-file/config-advanced.md
  *(Note: the clone's schema still contains `profiles`, another instance of the version skew below.)*
- **Reasoning controls:** `model_reasoning_effort` (docs: `minimal|low|medium|high|xhigh`),
  `plan_mode_reasoning_effort`, `model_reasoning_summary` (`auto|concise|detailed|none`),
  `model_verbosity` (`low|medium|high`), and the feature flag `reasoning_effort_override`. In the
  clone's schema `model_reasoning_effort` is an **open string**, not a closed enum.
  https://learn.chatgpt.com/docs/config-file/config-reference.md
- **Amazon Bedrock** is documented as first-class: `model_provider = "amazon-bedrock"`, auth order
  `AWS_BEARER_TOKEN_BEDROCK` (requires `AWS_REGION`) then the AWS SDK chain; fast mode, web search,
  image generation, and cloud chats are unavailable on that provider.
  https://learn.chatgpt.com/docs/amazon-bedrock.md

## 2.15 Streaming output & TUI / interaction UX

- **Real slash commands, from the enum** (authoritative over the docs tables): `/feedback`, `/new`,
  `/init`, `/compact`, `/recap`, `/review`, `/rename`, `/resume`, `/archive`, `/delete`, `/clear`,
  `/fork`, `/worktree`, `/app`, `/quit`, `/exit`, `/copy`, `/export`, `/raw`, `/diff`, `/mention`,
  `/skills`, `/import`, `/hooks`, `/status`, `/cd`, `/pwd`, `/usage`, `/debug-config`, `/title`,
  `/statusline`, `/theme`, `/pets`, `/ps`, `/stop`, `/model`, `/ide`, `/personality`, **`/plan`**,
  `/voice`, **`/goal`**, `/agents` ("view and switch between all active agent sessions"),
  `/multiagents` ("switch between this session's subagents"), `/side` (alias `/btw`).
  https://github.com/openai/codex/blob/main/codex-rs/tui/src/slash_command.rs
- Streaming is native: the TUI renders reasoning and tool events as they arrive, and slash commands
  can be queued with **Tab** mid-turn. `codex exec` deliberately splits streams: progress → stderr,
  final message → stdout. https://learn.chatgpt.com/docs/non-interactive-mode.md
- Image input: `--image`/`-i` (`value_delimiter = ','`, `num_args = 1..`) on both `codex` and
  `codex exec resume`; plus `features.view_image` and `features.image_detail_original`.
  https://github.com/openai/codex/blob/main/codex-rs/utils/cli/src/shared_options.rs
- `/status` shows current session configuration and token usage; `/usage` views account usage or
  consumes a usage-limit reset. `/debug-config` shows config layers and requirement sources.
- The TUI can attach to a remote app server with `--remote ws://host:port`, `--remote wss://…`, or
  `--remote unix://`, with `--remote-auth-token-env <ENV_VAR>` for bearer auth.
  https://learn.chatgpt.com/docs/developer-commands.md?surface=cli

## 2.16 Observability

- **OpenTelemetry is genuinely shipped and opt-in** (`[otel]`, disabled by default; with
  `exporter = "none"` Codex records events but sends nothing). Keys: `otel.environment` (default
  `"dev"`), `otel.exporter`, `otel.trace_exporter`, `otel.metrics_exporter`, `otel.log_user_prompt`
  (default false/redacted), and per-exporter `.endpoint`, `.protocol` (`binary`|`json`), `.headers`,
  `.tls.ca-certificate`/`.client-certificate`/`.client-private-key`. Trace, metric, and log pipelines
  are separate with async batching and flush on shutdown.
  https://learn.chatgpt.com/docs/config-file/config-advanced.md
- Exporter uses `opentelemetry-otlp` with features `grpc-tonic`, `http-proto`, `http-json`, `logs`,
  `metrics`, `trace`, `reqwest-rustls`, `tls-roots`. The **default release exporter is an internal
  Statsig OTLP/HTTP endpoint** (`https://ab.chatgpt.com/otlp/v1/metrics` with a `statsig-api-key`
  header), forced to `None` in debug builds.
  https://github.com/openai/codex/blob/main/codex-rs/otel/Cargo.toml ·
  https://github.com/openai/codex/blob/main/codex-rs/otel/src/config.rs
- Documented OTel events: `codex.conversation_starts`, `codex.api_request`, `codex.sse_event`,
  `codex.websocket_request`, `codex.websocket_event`, `codex.user_prompt`, `codex.tool_decision`,
  `codex.tool_result`, `codex.turn_cost`. Documented metrics: `codex.api_request(.duration_ms)`,
  `codex.sse_event(.duration_ms)`, `codex.websocket.request|event(.duration_ms)`,
  `codex.tool.call(.duration_ms)`, all tagged with `auth_mode`, `originator`, `session_source`,
  `model`, `app.version`.
  https://learn.chatgpt.com/docs/config-file/config-advanced.md
- Source adds many undocumented metrics: `codex.turn.e2e_duration_ms`, `.ttft.duration_ms`,
  `.ttfm.duration_ms`, `.token_usage`, `.cost_microusd`,
  `codex.responses_api_overhead.duration_ms`, `codex.responses_api_engine_iapi_ttft|tbt`,
  `codex.goal.*`, `codex.process.start`, `codex.artifact.operation.*`, `codex.guardian.review*`.
  https://github.com/openai/codex/blob/main/codex-rs/otel/src/metrics/names.rs
- **Log files are opt-in:** `log_dir` defaults to `$CODEX_HOME/log`, and setting it explicitly also
  enables the plaintext `codex-tui.log` in that directory (`TUI_LOG_FILE_NAME`). `codex exec` prints
  inline instead. https://learn.chatgpt.com/docs/config-file/environment-variables.md
- **`RUST_LOG` is the only documented logging env var** (accepts levels and filters like
  `codex_core=debug,codex_tui=debug`; `codex exec` defaults to `error`). **`CODEX_LOG` does not
  exist** — the only occurrences are literal strings in an exec-server test fixture.
  https://learn.chatgpt.com/docs/config-file/environment-variables.md
- Enterprise reporting surfaces (separate from OTel): a Codex analytics dashboard at
  `admin.openai.com/analytics/codex` explicitly **"not a stable schema contract"**, an **Analytics
  API** for aggregated usage (ChatGPT-workspace-scoped, authenticated with a Platform org API key),
  and a **Compliance API** of append-only audit records (`COMPLIANCE_API_KEY`) — not raw logs.
  https://learn.chatgpt.com/docs/enterprise/analytics-api.md
- **Stale doc:** the docs' "full event catalog" link points at
  `github.com/openai/codex/blob/main/docs/config.md#otel`, but `docs/config.md` at this commit
  contains only a "Lifecycle hooks" section and **no `otel` anchor**.
  https://github.com/openai/codex/blob/main/docs/config.md

## 2.17 Evaluation & regression testing

- **Codex ships ZERO model-quality evaluation harness. There is no SWE-bench infrastructure.**
  Verified by direct search: `swebench` / `swe-bench` / `swe_bench` returns **0 matches** across the
  entire repo, and there is **no `evals/` directory** anywhere.
- **Snapshot testing is the primary regression tool:** `insta` is used across ~15 `Cargo.toml` files,
  with **1003 `.snap` files** (verified count) spread over ~20 `snapshots/` directories.
  https://github.com/openai/codex/blob/main/codex-rs/core/Cargo.toml ·
  https://github.com/openai/codex/tree/main/codex-rs/tui/src/chatwidget/snapshots
- Integration tests are extensive: `codex-rs/core/tests/` (a `suite/` of ~156 files), `cli/tests/`
  (~25 files including `execpolicy.rs`, `worktree.rs`, `app_server.rs`), plus per-crate `tests/`
  directories. Runner: `just test` → `cargo nextest run --no-fail-fast`; `just bazel-test`.
  https://github.com/openai/codex/blob/main/justfile
- Benchmarks measure **latency, not task success**: `just bench` (`cargo bench --workspace`), and
  `just bench-e2e` → `bazel test //codex-rs:e2e-benchmarks`; only two benches exist
  (`cli/e2e_benches/codex_help.rs`, `utils/image/benches/prompt_images.rs`).
  https://github.com/openai/codex/blob/main/codex-rs/BUILD.bazel
- No `promptfoo` harness: the single repo match is a prose mention inside a bundled skill asset.
  https://github.com/openai/codex/blob/main/codex-rs/skills/src/assets/samples/openai-docs/references/official-docs.md

## 2.18 Verified to NOT exist in Codex CLI

Confirmed absent by repo-wide search at commit `ddea03ad`: any SWE-bench harness or `evals/`
directory; `--continue`; `/undo`; automatic file checkpointing (`ghost_snapshot` is dead
compatibility-only config, `Feature::GhostCommit` is `Stage::Removed`); a core
`read_file`/`write_file`/`grep`/`glob` tool; AST-based editing; `.codexignore`; MCP SSE transport;
`wire_api = "chat"` (hard error) and the `ollama-chat` provider id; a built-in provider id `oss`;
`[profiles.*]` tables (removed in 0.134.0 per docs); `include_apply_patch_tool`;
`model_max_output_tokens`; `preferred_auth_method`; `--include-plan-tool`; `CODEX_LOG`;
`--full-auto` (see skew note); a `max_turns` limit; a documented exit-code table; an eval harness.

## 2.19 Version skew warning (important when reading these notes)

The docs site and this clone **disagree**, and the disagreement is directional: the live docs
describe a **newer** build than commit `ddea03ad`. Two confirmed examples:

1. `--full-auto` — documented as a deprecated compatibility flag, but **absent entirely** from the clone.
2. `[profiles.*]` — documented as removed in 0.134.0, but **still present** in the clone's
   `config.schema.json`.

The clone's own `AGENTS.md` explains why the repo is a poor doc source: "Do not add general product or
user-facing documentation to the `docs/` folder. The official Codex documentation lives elsewhere."
`CHANGELOG.md` is also a stub (93 bytes, pointing at the releases page). Consequently, **when the two
disagree, prefer the generated `codex-rs/core/config.schema.json` and the Rust source for behaviour at
this commit, and the docs site for intended current product behaviour** — but always state which one
a claim rests on.
https://github.com/openai/codex/blob/main/AGENTS.md ·
https://github.com/openai/codex/blob/main/CHANGELOG.md

---

# 3. Cross-tool comparison (design-relevant)

| Dimension | OpenAI Codex CLI | Google Gemini CLI |
|---|---|---|
| Language / runtime | Rust workspace `codex-rs/**` (+ TS & Python SDKs) | TypeScript monorepo `packages/{core,cli}` |
| Turn loop | `run_turn` `loop {}`; multi-turn; **no max-turns limit** | `processTurn` recursion; `MAX_TURNS = 100` per prompt, `model.maxSessionTurns` per session |
| Parallel tool calls | Yes — `FuturesOrdered` + per-tool read/write lock; **default is serial** (`supports_parallel_tool_calls` defaults false) | Yes — `Promise.all`; **default is parallel**, but `replace`/`write_file`/`update_topic` are forced sequential |
| Edit strategy | Custom freeform **`apply_patch`** V4A diff with a Lark grammar | Exact **search/replace `replace`** (`old_string`/`new_string`, uniqueness required) + whole-file `write_file`; **no diff tool** |
| File read tools | **None as core tools** — reads go through `exec_command` | Rich: `read_file`, `glob`, `grep_search`, `list_directory` |
| Edit failure recovery | `RespondToModel` error returned as tool output | Fuzzy/regex self-correction via `FixLLMWithInstruction` + `$EDITOR` path |
| Context compaction | Auto-compact w/ `model_auto_compact_token_limit`, local/remote/token-budget strategies, `/compact` | `/compress` at `model.compressionThreshold` (default 0.5 of window), preserve 0.3, split only at user turns |
| Project rule file | `AGENTS.md` (+ `AGENTS.override.md`), cumulative, `project_doc_max_bytes` (32 KiB) | `GEMINI.md` 3-tier with **just-in-time** subdirectory loading, `@file.md` imports |
| Ignore rules | `.gitignore`/`.ignore` only — **no `.codexignore`** | `.gitignore` + **`.geminiignore`** + custom paths |
| Approval model | `AskForApproval`: `untrusted`/`on-request`/`granular`/`never` + `approvals_reviewer` | `--approval-mode`: `default`/`auto_edit`/`yolo`/`plan` + **TOML policy engine** (tiers 1–5, `allow`/`deny`/`ask_user`) |
| Sandbox | Seatbelt (macOS), bubblewrap + seccomp (Linux), MXC/restricted-token (Windows); **Landlock is legacy/backup** | Docker/Podman/`sandbox-exec`/`runsc`/`lxc`/`windows-native`; Seatbelt profiles incl. `restrictive-closed` |
| Sessions | `~/.codex/sessions/rollout-*.jsonl`; **`codex fork` exists** | `~/.gemini/tmp/<hash>/chats/`; **no fork** |
| Checkpoints / undo | **None** — docs tell you to use git; `/diff` + worktrees | `/rewind` + auto-checkpoints + `/restore` via a shadow git repo |
| Plan mode | Yes (`/plan`, Shift+Tab); task list `update_plan` **off by default** | Yes, default-on (`--approval-mode=plan`, `/plan`); `write_todos` always available, one `in_progress` |
| Subagents | `spawn_agent` etc.; default cap 6 threads (V2: 4), `max_depth` 1 | `invoke_agent`; `.gemini/agents/*.md`; subagents **cannot** recurse; no parallel setting |
| Hooks | 12 events (`PreToolUse`…), `hooks.json`/`[hooks]`, trust + managed-only mode | 11 events (`BeforeTool`…), settings.json `hooks`, exit-2 blocking |
| Headless | `codex exec` (read-only default), `--json` JSONL, `--output-schema` | `gemini -p`, `--output-format json\|stream-json`, **enumerated exit codes 41/42/44/52/53/54/55/130** |
| Multi-provider | `model_providers` + `--oss`/Ollama/LM Studio/Bedrock; **Responses API only** | **Gemini/Vertex only**; local Gemma router; **no OpenAI-compatible provider** |
| Cost control | OTel `codex.turn.cost_microusd`; `RateLimitSnapshot` credits/spend caps | `/stats` cached-token savings; quota tiers; `billing.overageStrategy` |
| Observability | Full OTel traces/metrics/logs, `RUST_LOG`, opt-in `codex-tui.log` | OTel (`local`/`gcp` targets), `GEMINI_TELEMETRY_*`, F12 debug console |
| Evals | **None** — insta snapshots (1003) + nextest + latency benches | In-repo vitest behavioral evals + integration/e2e + perf baselines |
| Noteworthiness | Docs in repo are stubs; docs site is ahead of the clone | Docs are in-repo and current; a few stale flags remain |

## 3.1 Highest-leverage design contrasts

1. **Two opposite edit philosophies.** Codex bets on a structured patch DSL (`apply_patch` + Lark
   grammar) applied by a dedicated, heavily snapshot-tested crate; Gemini bets on the model emitting
   an exact `old_string`/`new_string` pair, and invests instead in *recovery* (fuzzy/regex/LLM
   self-correction) when the match fails.
2. **Opposite parallelism defaults.** Gemini runs tool calls in parallel by default and carves out
   exceptions for edits; Codex defaults to serial and only opts specific read-only/exec tools into
   concurrency. Codex's read-modify-write tools share an `RwLock`; Gemini uses a name blacklist.
3. **Read tools vs shell.** Gemini ships first-class `read_file`/`glob`/`grep_search` tools; Codex
   deliberately has none, pushing reads through `exec_command`. This changes token cost, safety
   surface, and how easily the sandbox can reason about I/O.
4. **Undo is the biggest asymmetry.** Gemini has real checkpointing (`/rewind`, shadow git repo,
   `/restore`); Codex has none and documents "use git yourself". For an agent product this is a
   first-order safety feature, not a nicety.
5. **Eval culture.** Gemini treats behavioral evals as a release gate with a named internal
   dashboard; Codex has no model-quality harness at all — only unit/snapshot/integration/bench.
6. **Provider openness vs model coupling.** Codex exposes a documented OpenAI-compatible provider
   abstraction (`model_providers`, `base_url`, local Ollama/LM Studio, Bedrock) but narrowed the wire
   protocol to Responses-only; Gemini is effectively Gemini/Vertex-locked, with only base-URL
   overrides and an experimental local Gemma router.
