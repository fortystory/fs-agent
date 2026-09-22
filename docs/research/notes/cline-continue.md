# Cline & Continue — Agent Design and Feature Notes

Research notes on the **design and features** of two open-source coding agents, built from **primary sources only**
(official docs sites, official GitHub repos/raw source, official blogs/changelogs).

**Conventions used below**

- Every substantive claim carries the URL that owns it, next to the claim.
- Claims not confirmed against a primary source are marked **unverified**.
- Where official sources contradict each other, that is called out as a **doc conflict**.
- Two useful fetch techniques discovered while researching:
  Cline docs are Mintlify; **`https://docs.cline.bot/<path>.md`** returns raw markdown, and the docs source tree is in
  the repo, so **`https://raw.githubusercontent.com/cline/cline/main/docs/<path>.mdx`** is a stable, fetch-friendly twin.
  For the blog, **`https://cline.ghost.io/<slug>/`** is the server-rendered mirror of `cline.bot/blog/<slug>` (the
  cline.bot pages are client-rendered and come back as a nav shell only).

---

## 0. Executive summary

The two projects are in opposite lifecycle phases, and that shapes every comparison.

- **Cline** is actively and aggressively re-architected. It now ships **two harnesses side by side**: a legacy
  VS Code/JetBrains "classic" harness (XML-style tools, `attempt_completion`, Focus Chain) and the current
  **ClineCore / SDK** harness (`@cline/agents` loop, native tool schemas, hub-spoke daemon, webview clients). The docs
  describe both and draw the line explicitly — "Some older docs/examples reference XML-style names like `read_file`,
  `replace_in_file`, or `execute_command`. Current SDK/ClineCore runtime uses the built-in tool names listed above"
  ([tools reference](https://docs.cline.bot/tools-reference/all-cline-tools)).
  Consequences: **Focus Chain is deprecated with no replacement**
  ([deprecations](https://docs.cline.bot/resources/deprecations)); tool names differ between docs pages; and a large
  block of capability (plugins, custom tools, agent teams, connectors, scheduling, Kanban) is **CLI/SDK/Kanban-only**
  and explicitly "not applicable on VSCode and JetBrains Extension for now"
  ([plugins](https://docs.cline.bot/customization/plugins), [agent teams](https://docs.cline.bot/cli/agent-teams),
  [scheduling](https://docs.cline.bot/cli/scheduling), [connectors](https://docs.cline.bot/cli/connectors)).
- **Continue** is frozen. "**Note: The `continuedev/continue` repository is no longer actively maintained and is
  read-only for all users.**" with a final 2.0.0 release that "included removing anonymous telemetry, pulling out
  authentication, squashing bugs"
  ([README](https://raw.githubusercontent.com/continuedev/continue/main/README.md)).
  Its docs contain large deprecated surfaces — `@Codebase`, `@Docs`, most `@`-context providers, `config.json` — and
  the recommended replacement everywhere is **agent tools + rules + MCP**
  ([codebase awareness guide](https://docs.continue.dev/guides/codebase-documentation-awareness)).

The single most instructive design contrast: **Cline invested in a persistent, addressable session/daemon layer**
(hub-spoke, sessions as first-class records, connectors, cron), while **Continue's frozen design keeps the agent loop
in-process** with permissions expressed as a small declarative policy file.

---

# Part 1 — Cline

## 1.1 Identity, surfaces, licensing

- "Cline is an AI coding agent that lives in your editor and your terminal. It can read and write files, run terminal
  commands, use a browser, and help you build features through natural conversation. **Every action requires your
  explicit approval.** You're always in control." ([overview](https://docs.cline.bot/cline-overview)).
  - **Nuance:** this is the IDE framing. The CLI's own README states tool approval is **auto-approved by default** and
    `--auto-approve false` is what requires review
    ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
- End-user surfaces: **VS Code extension**, **JetBrains plugin**, **CLI** (`npm i -g cline`), **Cline SDK**
  (`npm install @cline/sdk`), **Kanban** (`npx kanban`), and a **Desktop app** for macOS/Windows (Tauri + Bun sidecar
  + Next.js) ([README](https://raw.githubusercontent.com/cline/cline/main/README.md),
  [overview](https://raw.githubusercontent.com/cline/cline/main/docs/cline-overview.mdx)).
- The **JetBrains plugin is not open source**: "Currently we are not open-sourcing JetBrains plugins"
  ([README](https://raw.githubusercontent.com/cline/cline/main/README.md)).
- License: **Apache 2.0** (© Cline Bot Inc.) ([README](https://raw.githubusercontent.com/cline/cline/main/README.md)).
- Repo is a monorepo: `apps/` (cli, vscode, cline-hub, examples, vscode-rollout), `sdk/packages/` (shared, llms,
  agents, core, sdk, ui), `evals/`, `docs/`, `.clinerules/`
  ([README](https://raw.githubusercontent.com/cline/cline/main/README.md),
  [contents](https://api.github.com/repos/cline/cline/contents/)).
- Toolchain: **Bun 1.3.13 + Node >= 22**; SDK packages resolve through compiled `dist/`, so `bun run build:sdk` is
  required after SDK changes ([AGENTS.md](https://raw.githubusercontent.com/cline/cline/main/AGENTS.md)).
- Model access paths: **Cline (usage-billing)** pay-as-you-go credits, **ClinePass** flat **$9.99/month** claiming
  "2-5x the usage on popular open coding models compared to standard API rate", or **BYOK**
  ([overview](https://docs.cline.bot/cline-overview),
  [clinepass.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/getting-started/clinepass.mdx)).

## 1.2 Agent loop & tool-calling protocol

**Current (ClineCore / SDK) loop**

- Documented loop: `run()` or `continue()` → model request → tool calls (if any) → tool results → repeat until
  complete ([docs/sdk/clinecore.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/clinecore.mdx)).
- `@cline/agents` is described as the **"Browser-compatible stateless agent execution loop"**, exporting
  `AgentRuntime` (aliased `Agent`), `createAgentRuntime`, `createAgent`, `AgentRuntimeConfig`, `AgentRunInput`,
  `AgentEventListener`, and `createTool`. `AgentRuntime` methods: **`run`, `continue`, `abort`, `subscribe`,
  `restore`, `snapshot`** ([architecture/overview](https://docs.cline.bot/sdk/architecture/overview)).
- **`Agent` from `@cline/agents` ships no built-ins by default**; you pass tools explicitly. `ClineCore` is what
  enables the built-in suite ([tools reference](https://docs.cline.bot/tools-reference/all-cline-tools)).
- Multi-turn state is held internally; to persist, store externally and rehydrate with `restore(messages)` before
  continuing ([docs/sdk/clinecore.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/clinecore.mdx)).
- Iterations are observable as `iteration_start` / `iteration_end` events
  ([events reference](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/reference/events.mdx)).

**Stop conditions (this is the clearest primary-source answer)**

- The `done` event carries an explicit termination reason enum:
  **`"completed" | "max_iterations" | "aborted" | "mistake_limit" | "error"`**
  ([events reference](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/reference/events.mdx)).
- `AgentRunResult.status` is `"completed" | "aborted" | "failed"`, alongside `iterations` and `usage`
  ([agent reference](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/reference/agent.mdx)).
- `maxIterations` is a constructor option, e.g. `new Agent({ providerId, modelId, apiKey, maxIterations: 1 })`
  ([SDK overview](https://docs.cline.bot/sdk/overview)).
- A **consecutive-mistake ceiling** exists: `max_consecutive_mistakes` (field 124 of `Settings`), surfaced on the CLI
  as `--retries <count>` ("Maximum consecutive mistakes (retries) before halting")
  ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto),
  [cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx)).
- Task-level timeout: `-t, --timeout <seconds>` (default `0` = no timeout)
  ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx)).
- A **rejected tool call is a normal loop input, not a stall**: "The agent does not get stuck in a loop. The rejection
  counts as a response, and the agent proceeds with its next iteration."
  ([permission handling](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/guides/permission-handling.mdx)).

**Parallel tool calls — supported, conditional, model-family-dependent**

- Commit `6d1bfc3a1ba46d5bf762600d4fdead01e5526f48` (PR #8331, "feat(prompts): enable parallel tool usage for claude
  and gemini 3 models") introduces an **`enableParallelToolCalling`** field on `SystemPromptContext`, read from the
  global setting of the same name
  ([commit patch](https://github.com/cline/cline/commit/6d1bfc3a1ba46d5bf762600d4fdead01e5526f48.patch)).
- The old universal wording "You can use one tool per message…" was replaced with "You will receive the results of all
  tool uses in the user's response."
  ([same patch](https://github.com/cline/cline/commit/6d1bfc3a1ba46d5bf762600d4fdead01e5526f48.patch)).
- Per-model-family behaviour from the same patch: **GPT-5 family — parallel always on, prompt unchanged**;
  **Gemini 3 — conditional on the toggle**; **Claude 4+ — parallel wording when on, empty instruction when off** (to
  avoid colliding with Claude's built-in multi-tool system prompt).
- **MCP calls remain explicitly sequential** ("MCP operations should be used one at a time… Wait for confirmation of
  success before proceeding"), and that line is now conditional on MCP servers actually being enabled via a new
  `hasEnabledMcpServers(context)` helper
  ([commit patch](https://github.com/cline/cline/commit/6d1bfc3a1ba46d5bf762600d4fdead01e5526f48.patch)).
- The legacy harness baseline was strictly one-tool-per-message
  ([system-prompt snapshot @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/anthropic_claude_sonnet_4-basic.snap)).
- Independently of the toggle, **batching happens inside a single call**: `read_files` = "Read one or more files",
  `run_commands` = "Execute shell commands"
  ([docs/sdk/tools.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/tools.mdx)).
- A native-tool-calling path exists separately: `native_tool_call_enabled` (field 32 of `UpdateSettingsRequest`)
  switches the harness from XML tool blocks to provider-native tool schemas
  ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).

**Completion tool**

- Legacy: `attempt_completion` with required `result`, optional `command` ("A CLI command to execute to show a live
  demo of the result… DO NOT use commands like `echo` or `cat` that merely print text"), and optional `task_progress`
  ([snapshot @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/anthropic_claude_sonnet_4-basic.snap)).
  - Legacy guard: it "CANNOT be used until you've confirmed from the user that any previous tool uses were successful",
    and the prompt forbids ending the result with a question.
- Current SDK equivalent: **`submit_and_exit`** — "Submit a final answer and stop"
  ([docs/sdk/tools.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/tools.mdx)).
- Modern SDK alternative: a tool marked `lifecycle: { completesRun: true }` signals the run is done
  ([building an agent](https://docs.cline.bot/sdk/guides/building-an-agent)).

**Tool definition contract**

- `createTool({ name, description, inputSchema, execute })` where `inputSchema` may be JSON Schema **or Zod**
  ([docs/sdk/tools.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/tools.mdx),
  [tools API](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/reference/tools-api.mdx)).
- Worked example uses `z.object({...})` with `.describe()` per field and `z.enum(["critical","warning","suggestion"])`,
  noting "`z.enum` constrains severity to valid values, which improves model accuracy"
  ([building an agent](https://docs.cline.bot/sdk/guides/building-an-agent)).
- `AgentTool` defaults: `timeoutMs: 30000`, `retryable: true`, `maxRetries: 3`
  ([tools API](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/reference/tools-api.mdx)).

## 1.3 Runtime topology (hub-spoke) — a genuinely distinctive design

- "A background daemon (the hub) coordinates session state and event routing, **spoke workers execute the agent
  loop**, and clients (CLI, VS Code, JetBrains, etc.) attach as peers over **WebSocket**."
  ([hub-spoke](https://docs.cline.bot/sdk/architecture/hub-spoke)).
- Three roles, with an explicit invariant: "**clients participate, spokes execute, the hub coordinates.** No two roles
  should overlap." Hub = singleton daemon per machine; it manages sessions, routes events and approvals, manages
  schedules, and brokers capabilities — but "**Does not run the agent loop**". Spoke = worker process running
  `@cline/core`, owned by the daemon, not by any client
  ([hub-spoke](https://docs.cline.bot/sdk/architecture/hub-spoke)).
- Communication flow: client discovers/starts hub → `client.register` over WebSocket **advertising its capabilities**
  (shell access, file editing, diff viewing, …) → hub spawns/assigns a spoke → spoke executes and streams partial
  output, handling abort/cancel → **hub fans out events to all attached clients** → if the client disconnects,
  "the spoke continues executing. Another client can attach to the same session and pick up the event stream
  mid-flight." ([hub-spoke](https://docs.cline.bot/sdk/architecture/hub-spoke)).
- Backend modes via `ClineCore.create({ backendMode })`:

  | Mode | Behavior |
  |---|---|
  | `auto` | Prefer compatible local hub, else fall back to local in-process. **Default.** |
  | `hub` | Requires a compatible WebSocket hub; throws if unreachable |
  | `remote` | Requires explicit remote WebSocket hub endpoint |
  | `local` | Always in-process with local SQLite/file storage; no hub, no shared sessions |

  Source: [hub-spoke](https://docs.cline.bot/sdk/architecture/hub-spoke)
- Hub default address `127.0.0.1:25463` (`CLINE_HUB_ADDRESS`); discovery via lock files at
  `~/.cline/locks/hub/owners/`; hub logs to `~/.cline/logs/hub-daemon.log`
  ([hub-spoke](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/architecture/hub-spoke.mdx),
  [cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx)).
- Motivation stated: sessions that survive a closed window, multiple clients on one session, scheduled agents with no
  client connected, and process isolation "so a runaway agent doesn't freeze the UI"
  ([hub-spoke](https://docs.cline.bot/sdk/architecture/hub-spoke)).

**Package layering (strict dependency direction, downward only)**

```
Your application / CLI / VS Code / JetBrains
          │
          ▼
@cline/core     Sessions, storage, built-in tools, hub, automation, telemetry
   ├── @cline/agents   Browser-compatible AgentRuntime / Agent loop
   ├── @cline/llms     Provider handlers, gateway, model catalogs
   └── @cline/shared   Types, schemas, tools, hooks, extension contracts
```

Source: [packages](https://docs.cline.bot/sdk/architecture/overview). `@cline/core` exports `ClineCore`,
`ClineCoreOptions`, `ClineCoreStartInput`, `CoreSessionConfig`, `SessionRecord`, `AgentPlugin`, `createTool`, and its
capabilities include "local/hub/remote runtime backends", "session manifests and message artifacts", "built-in tools",
"tool approvals", "automation/scheduling services", "telemetry hooks", "plugin/extension loading" and
"**team/sub-agent tools**" ([packages](https://docs.cline.bot/sdk/architecture/overview)).

## 1.4 Tool set & edit strategy

**Current built-in tools (tools reference page)**

| Tool | Description |
|---|---|
| `bash` | Execute shell commands |
| `editor` | View and edit files |
| `read_files` | Batch read multiple files |
| `apply_patch` | Apply unified diffs to files |
| `search` | Ripgrep-powered codebase search |
| `fetch_web` | HTTP requests with HTML-to-markdown conversion |
| `ask_question` | Ask the user for input |

Source: [tools reference](https://docs.cline.bot/tools-reference/all-cline-tools). Categories as documented:
codebase ops (`editor`, `read_files`, `apply_patch`, `search`), execution (`bash`), external retrieval (`fetch_web`),
human-in-the-loop (`ask_question`).

- **Doc conflict — tool naming.** The SDK tools page lists the enabled suite as `read_files`, **`search_codebase`**,
  **`run_commands`**, **`fetch_web_content`**, `apply_patch`, `editor`, `skills`, `ask_question`, `submit_and_exit`,
  and `toolPolicies` examples elsewhere use those same SDK names, while the tools reference page uses `search`, `bash`,
  `fetch_web` ([docs/sdk/tools.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/tools.mdx),
  [permission handling](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/guides/permission-handling.mdx),
  [tools reference](https://docs.cline.bot/tools-reference/all-cline-tools)). Treat exact identifiers as
  version-dependent.
- **Legacy XML tool set** (registry listing @be7548ff): `access_mcp_resource`, `apply_patch`,
  `ask_followup_question`, `attempt_completion`, `browser_action`, `execute_command`, `focus_chain`,
  `list_code_definition_names`, `list_files`, `load_mcp_documentation`, `new_task`, `plan_mode_respond`, `read_file`,
  `replace_in_file`, `search_files`, `use_mcp_tool`, `web_fetch`, `write_to_file`
  ([contents listing @be7548ff](https://api.github.com/repos/cline/cline/contents/src/core/prompts/system-prompt/tools?ref=be7548fff1012b8aa9caf6df4ba24dc85fce55d7)).
- Browser tool in the legacy harness: `browser_action` with `action ∈ {launch, click, type, scroll_down, scroll_up,
  close}`, `url`, `coordinate`, `text`; **Puppeteer-controlled, 1280x720 viewport, must start with launch and end with
  close, and only `browser_action` may be used while the browser is open**
  ([snapshot @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/anthropic_claude_sonnet_4-basic.snap)).
  In the current ClineCore list the only external-retrieval tool is `fetch_web` / `fetch_web_content`; a distinct
  browser tool is not listed, though "Use the browser" survives as an auto-approve permission category
  ([tools reference](https://docs.cline.bot/tools-reference/all-cline-tools),
  [auto approve](https://docs.cline.bot/features/auto-approve)) — **status of a browser tool in the current harness is
  unresolved**.
- `web_fetch` (legacy): HTTP auto-upgraded to HTTPS; read-only
  ([snapshot @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/anthropic_claude_sonnet_4-basic.snap)).

**Edit strategy #1 — `replace_in_file` exact search/replace format (legacy, search-replace)**

The `diff` parameter must contain one or more blocks in exactly this shape
([snapshot @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/anthropic_claude_sonnet_4-basic.snap)):

```
------- SEARCH
[exact content to find]
=======
[new content to replace with]
+++++++ REPLACE
```

Documented rules (same snapshot):

- SEARCH must match **character-for-character** including whitespace, indentation and line endings.
- Blocks replace only the **first** match; multiple blocks must be listed **in file order**.
- Keep blocks short and never truncate lines mid-way.
- To **move** code, use two blocks (delete + insert); to **delete** code, use an empty REPLACE section.
- Marker hygiene is enforced in-prompt: "`------- SEARCH>` is INVALID", the `+++++++ REPLACE` marker must not be
  forgotten, and "Malformed XML will cause complete tool failure"
  ([old-generic-with-focus.snap](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/old-generic-with-focus.snap)).
- Tool-choice policy: "**Default to replace_in_file** for most changes"; use `write_to_file` for new files, heavy
  restructuring, boilerplate, or when replace would be riskier. Writes return the file's **final post-auto-format
  state**, and the model is told to use that as the reference for subsequent SEARCH blocks.
- `write_to_file` requires `path` + `content` (complete file, no truncation);
  `search_files` takes `path`, `regex` ("Uses Rust regex syntax") and optional `file_pattern` glob.

**Edit strategy #2 — `apply_patch` exact V4A diff format (GPT-5 / native-next-gen variants)**

- Payload shape ([tools/apply_patch.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/tools/apply_patch.ts)):

  ```
  apply_patch <<"EOF"
  *** Begin Patch
  *** [ACTION] File: [path/to/file]
  ...
  *** End Patch
  EOF
  ```

- ACTION ∈ {`Add`, `Update`, `Delete`}; then `-` old lines and `+` new lines with context lines; `*** Move to:` is also
  recognised by the repo's own `.clineignore` guard
  ([tools/apply_patch.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/tools/apply_patch.ts),
  [clineignore.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/customization/clineignore.mdx)).
- Default context is **3 lines above and below**; use ` class BaseClass` / ` def method():` anchors when 3 lines
  are not unique; **no line numbers are used** — context alone identifies the code
  ([tools/apply_patch.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/tools/apply_patch.ts)).
- Availability is gated on model id:
  `contextRequirements: (context) => context.providerInfo.model.id.includes("gpt-5")` for the native GPT-5 variant
  ([same file](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/tools/apply_patch.ts)).
- The current tools reference describes `apply_patch` flatly as "Apply unified diffs to files"
  ([tools reference](https://docs.cline.bot/tools-reference/all-cline-tools)).

**Diff-approval UI**

- "In VS Code and JetBrains, **every edit shows up as a diff you can review, modify, or revert.** All changes are
  tracked with checkpoints" ([README](https://raw.githubusercontent.com/cline/cline/main/README.md)).
- Checkpoint rows expose **Compare** (opens the editor's diff viewer showing additions, deletions and modifications
  across all affected files) and **Restore**
  ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints)).
- `taskCompletionViewChanges` is an RPC on `TaskService` — "Shows task completion changes diff in a view"
  ([proto/cline/task.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/task.proto)).
- **"Explain Changes" is deprecated**; "View Changes" remains, and users are told to ask Cline directly instead
  ([deprecations](https://docs.cline.bot/resources/deprecations)).
- In **Kanban**, clicking any diff line leaves an inline comment sent back to the agent as feedback
  ([kanban core workflow](https://raw.githubusercontent.com/cline/cline/main/docs/kanban/core-workflow.mdx)).

## 1.5 Checkpoints & undo

**Mechanism**

- "Cline maintains a **shadow Git repository separate from your project's actual Git history**. After each tool use
  (file edits, commands, etc.), Cline commits the current state of your files to this shadow repo. Your main Git
  repository stays untouched." ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints)).
- Source detail: one unique shadow git per **workspace** (identified by name, hashed to a number), all commits for that
  workspace in one shadow git on a **single branch**; nested git repos are temporarily disabled during operations
  ([CheckpointTracker.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointTracker.ts)).
- Commit message format `checkpoint-{cwdHash}-{taskId}`, created with `--allow-empty --no-verify`
  ([CheckpointTracker.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointTracker.ts)).
- Restore is `git reset --hard <commitHash>`; legacy `HEAD ` prefixes on stored hashes are defensively stripped
  ([same file](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointTracker.ts)).
- Writes are guarded by a **per-workspace folder lock** (`tryAcquireCheckpointLockWithRetry`); concurrent Cline
  instances can fail with "another Cline instance may be performing checkpoint operations". The lock is *skipped* when
  running in VS Code ([same file](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointTracker.ts)).

**Storage location**

- `getShadowGitPath(cwdHash)` → `<HostProvider.globalStorageFsPath>/checkpoints/{cwdHash}/.git`, documented in-source
  as `globalStorage/ checkpoints/ {cwdHash}/ .git/`
  ([CheckpointUtils.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointUtils.ts)).
- `cwdHash` is a **13-character numeric hash** of the workspace path (rolling `hash*31 + charCode`, truncated to 13
  digits) ([same file](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointUtils.ts)).
- **Unverified:** the literal resolved `globalStorageFsPath` per OS (injected by the host at runtime via
  `HostProvider`).

**Enable/disable**

- Enabled by default; toggle at Cline settings (gear) → **"Feature Settings"** → **"Enable Checkpoints"**
  ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints)).
- Setting key `enable_checkpoints_setting` (field 35 of `Settings`); in the extension it maps to the VS Code setting
  `cline.enableCheckpoints`; when disabled, `CheckpointTracker.create()` returns `undefined` immediately
  ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto),
  [CheckpointTracker.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointTracker.ts)).
- **Git must be installed**, or tracker creation throws "Git must be installed to use checkpoints."
  ([CheckpointTracker.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointTracker.ts)).

**Three restore options — separating code state from conversation state**

| Option | What It Does |
|---|---|
| **Restore Files** | Reverts project files to the snapshot; conversation retained |
| **Restore Task Only** | Deletes messages after this point; files untouched |
| **Restore Files & Task** | Reverts files *and* deletes messages after this point |

Source: [checkpoints](https://docs.cline.bot/core-workflows/checkpoints).

- Rationale given: "Instead of carefully reviewing every change before approving, you can let Cline move fast and roll
  back if something goes wrong. The cost of a mistake drops to nearly zero."
  ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints)).
- Editing a previous message and choosing **Restore All** restores files to the checkpoint at that point before
  resubmitting the edited message ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints)).

**Granularity & limitations**

- One checkpoint **per tool use**; editing three files in sequence yields three independently restorable checkpoints
  ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints)).
- **Doc conflict — "captures everything".** The docs claim "Checkpoints capture everything, including files not
  tracked by Git" ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints)), but the source writes an
  `info/exclude` built from `getDefaultExclusions()`, excluding `node_modules/`, `dist/`, `build/`, `out/`,
  `coverage/`, `vendor/`, `venv/`, `__pycache__/`, `.next/`, `.gradle/`, `.idea/`, `.vscode/`, `.clinerules/`, `bin/`,
  `temp/`, `Pods/`, plus media (`*.png`, `*.jpg`, `*.mp4`, …), archives (`*.zip`, `*.exe`, `*.so`, …), databases
  (`*.csv`, `*.db`, `*.sqlite`, `*.parquet`, …), config (`*.env*`, `*.local`, `*.production`), logs, and workspace
  Git-LFS patterns from `.gitattributes`
  ([CheckpointExclusions.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointExclusions.ts)).
  Accurate statement: **gitignored-but-not-default-excluded files are tracked; the listed categories are not.**
- **Terminal commands are NOT captured as restorable state.** Checkpoints are file snapshots committed after a tool
  use; there is no record of executed commands, and restoring files cannot un-run a command or undo a DB migration,
  package install, or external side effect
  ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints),
  [CheckpointTracker.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointTracker.ts)).
- Protected directories: checkpoints refuse to operate when the workspace **is** the home dir, Desktop, Documents or
  Downloads ([CheckpointUtils.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointUtils.ts)).
- Multi-root: the tracker validates all workspace paths but "for now, we just use the first valid path"
  ([CheckpointTracker.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointTracker.ts)).
- Performance: "For very large repositories, checkpoints may use significant storage and slow down Cline as it commits
  file snapshots after each tool use" ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints)).
- Checkpoints **persist across editor sessions** ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints)).

## 1.6 Permissions & safety

**Auto Approve model**

- "Auto Approve is evaluated **per tool call**." ([auto approve](https://docs.cline.bot/features/auto-approve)).
- "Read all files" and "Edit all files" only *extend* the base toggle — if the base toggle is off, the "all files"
  option does nothing ([auto approve](https://docs.cline.bot/features/auto-approve)).
- Notifications: OS-level notification when approval is required, and when an auto-approved terminal command has been
  running for **30 seconds** ([auto approve](https://docs.cline.bot/features/auto-approve)).

**Documented per-tool toggles**

| Setting | What It Allows |
|---|---|
| Read project files | Read files, list files, search in your workspace |
| Read all files | Read files outside your workspace (requires base toggle) |
| Edit project files | Create and edit files in your workspace |
| Edit all files | Edit files outside your workspace (requires base toggle) |
| Execute safe commands | Run terminal commands marked safe |
| Execute all commands | Run commands requiring approval (requires base toggle) |
| Use the browser | Browser tool for web fetching and searching |
| Use MCP servers | MCP tools and resources |
| Enable notifications | Notifies you about long-running commands |

Source: [auto approve](https://docs.cline.bot/features/auto-approve)

**Underlying machine-readable keys** (`AutoApprovalSettings { version; actions: AutoApprovalActions;
enable_notifications }`), with `AutoApprovalActions` = `read_files`, `read_files_externally`, `edit_files`,
`edit_files_externally`, `execute_safe_commands`, `execute_all_commands`, `use_browser`, `use_mcp` — all
`optional bool` ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).

**Command safety heuristic vs real allowlists**

- IDE/docs position: "**Cline does not use a fixed allowlist.** The model marks each command with a
  `requires_approval` flag based on the command and arguments. These are examples, not guarantees."
  ([auto approve](https://docs.cline.bot/features/auto-approve)).
  - Commonly treated as safe: `npm run build`, `npm test`, `git status`, `ls -la`, `cat package.json`.
  - Commonly requiring approval: `npm install <pkg>`, `rm -rf <path>`, `mv <a> <b>`, `sed -i ...`.
  - `execute_command` in the legacy harness literally took a required boolean `requires_approval` parameter
    ([snapshot @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/anthropic_claude_sonnet_4-basic.snap)).
- **But the CLI does have a real allow/deny policy**: env var **`CLINE_COMMAND_PERMISSIONS`**, JSON
  `{"allow": [...globs], "deny": [...globs], "allowRedirects": bool}`. Rules: **`deny` overrides `allow`**; if `allow`
  is set, non-matching commands are denied; **`allowRedirects` defaults to `false`**. Documented example:
  `export CLINE_COMMAND_PERMISSIONS='{"allow": ["npm *", "git *"], "deny": ["rm -rf *", "sudo *"]}'`
  ([getting-started/config.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/getting-started/config.mdx),
  [cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx)).

**Approval mode in the CLI**

- Tool approval is **auto-approved by default**; `--auto-approve false` requires review. TTY prompt format:
  `Approve tool "<tool_name>" with input <preview>? [y/N]`. **If stdin/stdout is not a TTY, required-approval calls are
  denied** ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
- `CLINE_TOOL_APPROVAL_MODE` (`desktop` = file IPC; unset = terminal prompt) and `CLINE_TOOL_APPROVAL_DIR`
  ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).

**YOLO mode**

- "Check the box and Cline auto-approves everything: file changes, terminal commands, browser actions, MCP tools, and
  **mode transitions**." Enabled at Settings → Features → "YOLO Mode"; "No confirmation dialogs"
  ([auto approve](https://docs.cline.bot/features/auto-approve)).
- Key `yolo_mode_toggled` (field 47 of `Settings`, field 22 of `UpdateSettingsRequest`)
  ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).
- YOLO removes *interactive safety*, not just approvals: the plan-mode prompt **drops the "ask clarifying questions"
  instruction** when `yoloModeToggled` is true
  ([components/act_vs_plan_mode.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/components/act_vs_plan_mode.ts)).
- Enterprise can centrally forbid it with remote config `{"yoloModeAllowed": false}` — the toggle is then disabled in
  all UIs and local settings cannot override it
  ([yolo-mode.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/configuration/infrastructure-configuration/control-other-cline-features/yolo-mode.mdx)).
- **Hooks are disabled in CLI `--yolo` mode**; use `--act` or `--plan` instead
  ([clineignore.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/customization/clineignore.mdx)).

**Plan mode as read-only safety**

- Plan mode "cannot modify any files or execute commands"; the constraint is described as intentional
  ([plan and act](https://docs.cline.bot/core-workflows/plan-and-act)).
- A separate **`strict_plan_mode_enabled`** setting exists (field 46 of `Settings`, field 16 of
  `UpdateSettingsRequest`)
  ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).
- **Unverified:** open GitHub issues (surfaced in search only, not fetched) report plan-mode file modifications, so the
  read-only guarantee is asserted by docs but contested by users.

**SDK tool policies**

- `toolPolicies: { <tool>: {...} }` with `{ autoApprove: true }` (runs without asking), `{ autoApprove: false }`
  (waits for approval), `{ enabled: false }` (tool is completely disabled — "model won't see it"), and
  **no policy set → defaults to enabled AND auto-approved**
  ([permission handling](https://docs.cline.bot/sdk/guides/permission-handling)).
- Host-side callback: `capabilities: { requestToolApproval: async () => ({ approved: true }) }`
  ([permission handling](https://docs.cline.bot/sdk/guides/permission-handling)).
- Worked pattern: auto-approve `ls/cat/grep/find/git status/git log/git diff` and approve reads under `/src/` or
  `/tests/` ([permission handling](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/guides/permission-handling.mdx)).

**Hooks as an enforcement layer**

- A `PreToolUse` hook can **block** a tool call: emitting `{"cancel": true, "errorMessage": "..."}` blocks execution
  and shows the error; `cancel: false`/omitted continues
  ([.clinerules/hooks/README.md](https://raw.githubusercontent.com/cline/cline/main/.clinerules/hooks/README.md),
  [clineignore.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/customization/clineignore.mdx)).
- Multi-hook semantics: global + workspace hooks for a step execute **concurrently via `Promise.all`**, **execution
  order is not guaranteed**, if **all** allow the tool proceeds, and **if ANY hook returns `cancel: true` execution is
  blocked**. `contextModification` strings are concatenated with `\n\n`; `errorMessage` with `\n`
  ([.clinerules/hooks/README.md](https://raw.githubusercontent.com/cline/cline/main/.clinerules/hooks/README.md)).
- **Hook context affects FUTURE AI decisions, not the current tool call** — a hook cannot modify the parameters the
  model already chose ([.clinerules/hooks/README.md](https://raw.githubusercontent.com/cline/cline/main/.clinerules/hooks/README.md)).
- Limits: **30s timeout** (`HOOK_EXECUTION_TIMEOUT_MS`); context modifications **capped at 50KB**
  (`MAX_CONTEXT_MODIFICATION_SIZE`) ([.clinerules/hooks/README.md](https://raw.githubusercontent.com/cline/cline/main/.clinerules/hooks/README.md)).
- Windows: hooks documented as **"Not currently supported"** in that README
  ([.clinerules/hooks/README.md](https://raw.githubusercontent.com/cline/cline/main/.clinerules/hooks/README.md)).

**`.clineignore` — explicitly NOT a security boundary**

- "`.clineignore` filters what Cline loads automatically, but it is **not a security or access-control boundary** —
  ignored files can still be read via explicit `@` mentions or shell commands."
  ([.clineignore](https://docs.cline.bot/customization/clineignore)).
- It is separate from `.gitignore`; monorepo/multi-root workspaces can each have their own. Syntax is gitignore-like
  with the full documented pattern set (`*`, `**`, `?`, `[abc]`, `{a,b}`), `!` negation, `#` comments, `/build/`
  root-only anchoring ([.clineignore](https://docs.cline.bot/customization/clineignore)).
- What it restricts: matching files are excluded from (a) the file listing Cline sees at task start, (b) automatic
  context gathering, (c) search results
  ([.clineignore](https://docs.cline.bot/customization/clineignore)).
- Documented motivation: "Adding a `.clineignore` can cut your starting context from **200k+ tokens to under 50k**"
  ([.clineignore](https://docs.cline.bot/customization/clineignore)).
- The docs' own guard script uses `git check-ignore` scoped to `.clineignore`, canonicalises paths lexically, blocks
  attempts to modify `.clineignore` itself, and **does not resolve symlinks**
  ([clineignore.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/customization/clineignore.mdx)).

## 1.7 Plan & Act mode

**Mode semantics**

- Two modes. The **current mode is injected into every user message** via `environment_details`
  ([components/act_vs_plan_mode.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/components/act_vs_plan_mode.ts)).
- **ACT MODE:** all tools *except* `plan_mode_respond`; completion signalled with `attempt_completion`
  ([same file](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/components/act_vs_plan_mode.ts)).
- **PLAN MODE:** gains `plan_mode_respond`; goal is to gather context and produce a plan the user reviews before
  switching to ACT; exploration tools like `read_file` / `search_files` remain available
  ([same file](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/components/act_vs_plan_mode.ts)).
- "Cline retains the full context from your planning session"; "The conversation history carries over when you switch
  modes" ([plan and act](https://docs.cline.bot/core-workflows/plan-and-act)).
- Mode enum `PlanActMode { PLAN = 0; ACT = 1 }` with RPC `togglePlanActModeProto`
  ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).
- Documented workflow: start in Plan → explore → discuss → switch to Act → implement; cycle back to Plan on unexpected
  complexity ([plan and act](https://docs.cline.bot/core-workflows/plan-and-act)).
- Task-size guidance: small → Act only; medium → Plan → Act; large → `/deep-planning`
  ([plan and act](https://docs.cline.bot/core-workflows/plan-and-act)).

**`plan_mode_respond`**

- Available only in PLAN MODE; the model is told to deliver the plan *directly* via this tool rather than via
  `<thinking>` tags, and "You MUST use the response parameter, do not simply place the response text directly within
  `<plan_mode_respond>` tags"
  ([snapshot @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/anthropic_claude_sonnet_4-basic.snap)).
- Must be used only **after** exploration: "DO NOT use this tool to announce what files you're going to read - just read
  them first" ([same snapshot](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/anthropic_claude_sonnet_4-basic.snap)).
- Parameters: `response` (required), optional `needs_more_exploration` boolean (the escape hatch signalling the model
  will return to exploration tools), optional `task_progress`
  ([same snapshot](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/anthropic_claude_sonnet_4-basic.snap)).
- The legacy prompt frames plan mode as "a brainstorming session"; the model should confirm the user is happy with the
  plan and finally ask the user to switch back to ACT MODE
  ([act_vs_plan_mode.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/components/act_vs_plan_mode.ts)).

**`/deep-planning` — the structured planning protocol**

Four steps: (1) **Silent Investigation**, (2) **Discussion** (targeted questions), (3) **Plan Creation** — generates
**`implementation_plan.md`**, (4) **Task Creation** — a new task with trackable implementation steps
([using commands](https://docs.cline.bot/core-workflows/using-commands)).
The deep-planning prompt "is optimized for each model family"
([plan-and-act.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/plan-and-act.mdx)).

**Separate models per mode (model tiering)**

- Toggle: "Use different models for Plan and Act"; underlying key `plan_act_separate_models_setting` (field 34)
  ([plan and act](https://docs.cline.bot/core-workflows/plan-and-act),
  [proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).
- Documented example pairings: cost optimisation = GLM 4.6 (plan) / Grok Code Fast (act); maximum quality =
  Claude Opus / Claude Sonnet; speed = Gemini 3 Flash / Cerebras
  ([plan-and-act.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/plan-and-act.mdx)).
- The proto carries **full parallel sets** of plan-mode and act-mode model fields
  (`plan_mode_api_provider`, `plan_mode_api_model_id`, `plan_mode_thinking_budget_tokens`,
  `plan_mode_reasoning_effort`, and provider-specific `plan_mode_*` / `act_mode_*` ids)
  ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).
- CLI: `-p, --plan` starts in plan mode; default is act mode with auto-approve
  ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx)).

## 1.8 Focus Chain / todo lists — **DEPRECATED**

This directly answers the checklist item, and the answer is that the feature is gone.

- **Status: Deprecated, "No direct replacement".** "'Focus Chain' maintained a todo list during longer tasks to help
  Cline track progress. In our testing, we found that it was no longer providing enough additional benefit on top of
  the current harness to maintain it as a separate feature. This means the 'Focus Chain' todo list and the previous
  todo file behavior **will no longer be part of the current Cline experience**. The new harness is designed to manage
  task execution and context more effectively without relying on 'Focus Chain' as a separate layer."
  ([deprecations](https://docs.cline.bot/resources/deprecations)).
- The docs redirect map confirms removal: `/customization/focus-chain` and `/features/focus-chain` → the
  deep-planning anchor ([docs.json](https://raw.githubusercontent.com/cline/cline/main/docs/docs.json)).

**How it worked (valuable as design prior art, from the legacy source)**

- It **was** a setting: `FocusChainSettings { bool enabled = 1; int32 remind_cline_interval = 2; }`, exposed as
  `focus_chain_settings` (field 53 of `Settings`, field 17 of `UpdateSettingsRequest`). The prompt section is gated on
  `context.focusChainSettings?.enabled`
  ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto),
  [components/task_progress.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/components/task_progress.ts)).
- `remind_cline_interval` explains the observed reminder cadence; the prompt said "Every 10th API request, you will be
  prompted to review and update the current todo list if one exists"
  ([state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto),
  [old-generic-with-focus.snap](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/old-generic-with-focus.snap)).
  - **Note:** Cline's own blog says the default reminder interval was **every 6 messages** and that Field Chain was
    "enabled by default in v3.25" ([blog](https://cline.ghost.io/how-to-think-about-context-engineering-in-cline/),
    which is the same content as [cline.bot/blog](https://cline.bot/blog/how-to-think-about-context-engineering-in-cline)).
    The source-default is not stated in the snapshot, so the 6-vs-10 discrepancy is **unresolved**.
- **There was no real `focus_chain` tool.** The registry entry is a stub: `description: ""` with the comment
  "HACK: Placeholder to act as tool dependency", id `ClineDefaultTool.TODO`, name `focus_chain`, gated on
  `focusChainSettings?.enabled === true`. **The todo list actually rode on the `task_progress` parameter** attached to
  other tool calls ([tools/focus_chain.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/tools/focus_chain.ts)).
- **Todo list format:** standard Markdown checkboxes embedded in `task_progress`, e.g.
  `- [x] Set up project structure` / `- [ ] Test application`. Rules: update silently (never announce), supply the
  **whole** checklist each time, keep items at milestone granularity, rewrite if scope changes, and before
  `attempt_completion` ensure the final item is checked
  ([components/task_progress.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/components/task_progress.ts)).
- The prompt asserted automatic re-injection: "The system will automatically include todo list context in your prompts
  when appropriate - these reminders are important"
  ([same file](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/components/task_progress.ts)).
- **Survival across condensation** was the point: per-message `task_progress` plus periodic re-injection kept the plan
  anchored across context windows — which is exactly the layer the new harness absorbed. The Auto Compact docs still
  advise structured task lists generically ("Structured task lists can help maintain progress across summarizations")
  without naming Focus Chain ([auto compact](https://docs.cline.bot/features/auto-compact)).
- The context-engineering blog confirms the historical intent: "Cline generates a todo list at task start and reinjects
  it into context on a cadence so the thread does not drift", and "With Focus Chain on, the todo list persists through
  summarizations"
  ([blog mirror](https://cline.ghost.io/how-to-think-about-context-engineering-in-cline/)).

**Current replacements for task tracking:** `/deep-planning` produces `implementation_plan.md` plus a new task with
trackable steps ([using commands](https://docs.cline.bot/core-workflows/using-commands)); **Kanban** provides a real
task board with dependency chains
([kanban core workflow](https://raw.githubusercontent.com/cline/cline/main/docs/kanban/core-workflow.mdx)); **Agent
Teams** provide a persistent `task-board.json`
([multi-agent teams](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/guides/multi-agent-teams.mdx)).

## 1.9 Context window management & condensation

**Setting names**

- `use_auto_condense` (bool, field 48 of `Settings`, field 18 of `UpdateSettingsRequest`) and
  `auto_condense_threshold` (double, field 56 of `Settings`, field 24 of `UpdateSettingsRequest`)
  ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).
- Naming drift: the **setting** is `use_auto_condense`, but the **feature is documented as "Auto Compact"**
  ([proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto),
  [auto compact](https://docs.cline.bot/features/auto-compact)).
- CLI equivalent: **`--compaction <agentic|basic|off>`**, defaulting to `agentic`; `basic` = local truncation; `off` =
  disabled ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
- **Unverified:** the default value of `auto_condense_threshold` and whether Auto Compact is on by default in current
  releases (PR #12739 "Enable Auto Compact by default in VS Code" appeared in search results only; the PR API returned
  HTTP 403 rate-limit).

**How the trigger is computed — the per-provider buffer formula**

- `getContextWindowInfo(api)` returns `{ contextWindow, maxAllowedSize }` with these hard buffers:
  **64k → −27,000**; **128k → −30,000**; **200k → −40,000**; otherwise
  `max(contextWindow − 40_000, contextWindow * 0.8)`. Default context window when the model reports none is
  `128_000`, and DeepSeek on the OpenAI-compatible handler is forced to `128_000`
  ([context-window-utils.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/context/context-management/context-window-utils.ts)).
- Trigger condition: total tokens from the **previous** API request
  (`tokensIn + tokensOut + cacheWrites + cacheReads`) `>= min(floor(contextWindow * thresholdPercentage),
  maxAllowedSize)`
  ([ContextManager.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/context/context-management/ContextManager.ts)).
- When `thresholdPercentage` is not supplied, the code uses `maxAllowedSize` directly, clamped by
  `min(floor(contextWindow * thresholdPercentage), maxAllowedSize)`
  ([same file](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/context/context-management/ContextManager.ts)).

**Auto Compact (summarisation path)**

- Cline "monitors token usage"; when close to the limit it (1) creates a comprehensive summary, (2) preserves technical
  details/code changes/decisions, (3) **replaces** the conversation history with the summary, (4) continues exactly
  where it left off ([auto compact](https://docs.cline.bot/features/auto-compact)).
- "You'll see a **summarization tool call** when this happens, showing the cost like any other API call."
- Stated improvement over the old behaviour: "Previously, Cline would truncate older messages when hitting context
  limits, losing important context. Now with summarization: all technical decisions and code patterns are preserved…"
- **Cost:** summarisation "leverages your existing prompt cache from the conversation, so it costs about the same as
  any other tool call. Since most input tokens are already cached, you're primarily paying for summary generation
  (output tokens)" ([auto compact](https://docs.cline.bot/features/auto-compact)).
- **Fallback:** "With other models, Cline falls back to standard rule-based context truncation, **even if Auto Compact
  is enabled**." ([auto compact](https://docs.cline.bot/features/auto-compact)).
- Recoverability: use checkpoints to restore task state from before a summarisation, or edit a message before the
  summarisation tool call ([auto compact](https://docs.cline.bot/features/auto-compact)).

**Legacy truncation path (when auto-condense is off) — a precise, quotable algorithm**

All from [ContextManager.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/context/context-management/ContextManager.ts):

- It first tries **context optimisation** (deduplicating repeated file reads). If that saves **≥ 30%** of characters in
  range, truncation is skipped entirely.
- Otherwise it keeps the first user/assistant pair (indices 0 and 1) and removes either **half** of the remaining
  user-assistant pairs, or **three quarters** if `totalTokens/2 > maxAllowedSize` (a guard against switching from a
  200k model to a 64k model).
- `getNextTruncationRange` supports `"none" | "lastTwo" | "half" | "quarter"`; the range end is nudged by one so
  removal always ends on an assistant message, preserving user-assistant alternation.
- Truncation is **announced to the model**: the first assistant message is rewritten with a context-truncation notice
  and the first user message via `processFirstUserMessageForTruncation()`, recorded as timestamped updates in
  `context_history.json` so they can be undone when moving to an earlier checkpoint. The user-visible marker is
  `[NOTE] Some previous conversation history with the user has been removed`.
- `ensureToolResultsFollowToolUse()` repairs `tool_use` / `tool_result` pairing after truncation, inserting
  `"result missing"` placeholders and reordering tool_results to match tool_use order — **necessary for Anthropic's API
  contract**. This is a strong design lesson for anyone building a harness.
- Context-edit history persists at `<taskDirectory>/context_history.json` (`GlobalFileNames.contextHistory`) and
  supports **binary-search truncation by timestamp**
  ([ContextManager.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/context/context-management/ContextManager.ts),
  [disk.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/storage/disk.ts)).

**Prompt-level context management (from Cline's official eval work)**

- Cline's SDK let them measure "the average token size of Cline requests", and they found "our requests ran **20–30%
  heavier** than the publicly advertised averages of the most efficient harnesses"
  ([open-sourcing evals](https://cline.ghost.io/open-sourcing-evals-for-open-weight-agents/)).
- The concrete fix that caused visible token bloat: "any tool output over **50K got clamped to 8K** by keeping the head
  and tail and deleting the middle. That was pure deletion without compression… It still helped because **an agent
  resends its entire history on every turn.** One giant observation gets paid for on every future request"
  ([same post](https://cline.ghost.io/open-sourcing-evals-for-open-weight-agents/)).
- Measured effect of that one clamp, and why a shared harness is hard: minimax-m2.7 **+13.5**, glm-5.1 **+9.0**, but
  deepseek-v4-pro **−4.5** and deepseek-v4-flash **−6.7** points
  ([same post](https://cline.ghost.io/open-sourcing-evals-for-open-weight-agents/)).

**Manual compaction & context-reduction controls**

- `/smol` (alias `/compact`) compresses the conversation in place, staying in the same task; it is "the same action as
  Auto Compact but can be triggered manually"
  ([using commands](https://docs.cline.bot/core-workflows/using-commands),
  [blog](https://cline.ghost.io/how-to-think-about-context-engineering-in-cline/)).
- `/newtask` packages "overall plan, work accomplished, relevant files, next steps" into a **fresh task with a clean
  context window** ([using commands](https://docs.cline.bot/core-workflows/using-commands)).
- Terminal output volume caps: `terminal_output_line_limit` (field 39) and `subagent_terminal_output_line_limit`
  (field 126), plus `shell_integration_timeout`
  ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).
- **No documented character/line truncation limit for `read_file`** was found. What *is* attested is
  **duplicate file-read suppression**: repeated reads of the same file are replaced with a "duplicate file read notice",
  keeping the final read of each file
  ([ContextManager.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/context/context-management/ContextManager.ts)).

**`@` context references**

- File content: `@/path/to/file`; folder contents: `@/path/to/folder/` (**trailing slash required**). "Cline sees the
  complete file content, including imports, related functions, and surrounding context." Multi-root:
  `@workspace-name:/path/to/file` ([adding context](https://docs.cline.bot/core-workflows/working-with-files)).
- Two ways in: type `@` in the chat input, or click the **+** button bottom-left to browse files or images
  ([adding context](https://docs.cline.bot/core-workflows/working-with-files)).
- Drag & drop works — **in VS Code hold Shift** while dragging; dragging workspace files auto-creates file mentions.
  Supported: text files from the workspace plus images, PDFs, CSVs and Excel files from the filesystem; images require
  a multimodal model ([adding context](https://docs.cline.bot/core-workflows/working-with-files)).
- For other context (git history, web pages, terminal errors) "just describe it. Cline will run `git log`, fetch the
  URL, or read the output itself" ([adding context](https://docs.cline.bot/core-workflows/working-with-files)).
- Explicit `@` mentions **bypass** `.clineignore`
  ([.clineignore](https://docs.cline.bot/customization/clineignore)).
- **Mention types beyond file/folder are inferred, not documented:** telemetry defines `mention_type` values
  `file/url/folder/terminal/problems/git` for `task.mention_used` / `task.mention_failed`, and the redirect map shows
  older pages for url/terminal/problems/git mentions were consolidated — implying those mention types exist, but the
  current page documents only file and folder
  ([opentelemetry-events.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/opentelemetry-events.mdx),
  [docs.json](https://raw.githubusercontent.com/cline/cline/main/docs/docs.json)).

**Memory Bank (cross-session context methodology, not a feature)**

- Structured markdown files plus a rule file: `.clinerules/memory-bank.md` with `memory-bank/` containing
  `projectbrief.md`, `productContext.md`, `activeContext.md`, `systemPatterns.md`, `techContext.md`, `progress.md`.
  Driven by phrases "initialize memory bank", "update memory bank", "follow your custom instructions"
  ([memory bank](https://docs.cline.bot/best-practices/memory-bank)).
- `activeContext.md` "updates most frequently"; `progress.md` tracks "what works, what's left, known issues"
  ([memory bank](https://docs.cline.bot/best-practices/memory-bank)).

## 1.10 System prompt & project rule files

- Rules are "markdown files that provide persistent instructions across all conversations"
  ([rules](https://docs.cline.bot/customization/cline-rules)).
- **Supported rule sources** (all auto-detected, all togglable in the Rules panel):

  | Rule Type | Location |
  |---|---|
  | Cline Rules | `.clinerules/` (primary format) |
  | Cursor Rules | `.cursorrules` |
  | Windsurf Rules | `.windsurfrules` |
  | AGENTS.md | `AGENTS.md`, `~/.agents/AGENTS.md` |

  Source: [rules](https://docs.cline.bot/customization/cline-rules)
- **`AGENTS.md` IS officially supported** in two locations — workspace `AGENTS.md` and cross-tool global
  `~/.agents/AGENTS.md` ([rules](https://docs.cline.bot/customization/cline-rules)).
- `.clinerules` is documented as a **directory**: Cline "processes all `.md` and `.txt` files inside `.clinerules/`,
  combining them into a unified set of rules". Numeric prefixes like `01-coding.md` are organisation only.
  **Unverified:** `.clinerules` as a single *file* is not mentioned anywhere in the current docs.
- Two scopes: **workspace rules** in `.clinerules/` at project root (version-controllable) and **global rules** in the
  system Cline Rules directory. Global defaults: Windows `Documents\Cline\Rules`; macOS and Linux/WSL
  `~/Documents/Cline/Rules` (falling back to `~/Cline/Rules` if absent)
  ([rules](https://docs.cline.bot/customization/cline-rules)).
- **Precedence:** when both workspace and global rules exist, Cline combines them, and **workspace rules take
  precedence when they conflict with global rules** ([rules](https://docs.cline.bot/customization/cline-rules)).
  **Unverified:** precedence/ordering *between* rule types (Cline vs Cursor vs Windsurf vs AGENTS.md) is not documented.
- **Conditional rules** use YAML frontmatter; `paths` is the only supported conditional and takes an array of globs.
  Behaviour: no frontmatter = always active; `paths: []` = never activates; **invalid YAML = fails open** (rule
  activates, raw content visible) ([rules](https://docs.cline.bot/customization/cline-rules)).
- **"Current context"** for activation = paths in your message, open tabs, visible files, files Cline edited, and
  pending operations. Activation is surfaced as
  `Conditional rules applied: workspace:frontend-rules.md`
  ([rules](https://docs.cline.bot/customization/cline-rules)).
- Rules consume context tokens; docs advise keeping them concise and one concern per file
  ([rules](https://docs.cline.bot/customization/cline-rules)).
- `/newrule` creates a rule interactively, saved to `.clinerules` and "automatically loaded for future conversations"
  ([using commands](https://docs.cline.bot/core-workflows/using-commands)).
- Rules count as context-window content: "System instructions that guide Cline's behavior (including Cline Rules)"
  ([task-management.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/task-management.mdx)).
- Global rule dirs also appear in the unified config layout as `~/.cline/rules/` plus the
  `~/Documents/Cline/Rules` compatibility path
  ([config](https://docs.cline.bot/getting-started/config)).
- The org's own repo demonstrates the convention: `.clinerules/` holds `general.md`, `bun-and-node.md`,
  `cline-overview.md`, `debug-harness.md`, `network.md`, `protobuf-development.md`, `sdk-migration.md`, `storage.md`,
  plus `hooks/` and `workflows/` subdirs
  ([contents](https://api.github.com/repos/cline/cline/contents/.clinerules)).
- **Unverified:** the exact mechanism by which rule text reaches the system prompt (no prompt-section name, ordering or
  token budget is documented). `apps/vscode/src/core/prompts/` now contains only `responses.ts` and `__tests__`, so
  prompt assembly moved into the SDK
  ([contents](https://api.github.com/repos/cline/cline/contents/apps/vscode/src/core/prompts)).
- Rule toggling is instrumented: `task.rule_toggled` with attributes `rule_name, enabled, is_global`
  ([opentelemetry-events.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/opentelemetry-events.mdx)).

**Skills vs rules — a deliberate two-tier design**

- "Unlike rules (which are **always active**), skills load **on-demand** so they don't consume context when you're
  working on something unrelated" ([skills](https://docs.cline.bot/customization/skills)).
- Progressive loading, with token costs quantified:

  | Level | When Loaded | Token Cost | Content |
  |---|---|---|---|
  | Metadata | Always (at startup) | ~100 tokens per skill | `name` + `description` from YAML frontmatter |
  | Instructions | When skill is triggered | Under 5k tokens | `SKILL.md` body |
  | Resources | As needed | Effectively unlimited | Bundled files via `read_file` or executed scripts |

  Source: [skills](https://docs.cline.bot/customization/skills)
- Activation: Cline sees the skill list with descriptions and activates via the **`use_skill` tool**, which loads the
  full SKILL.md ([skills](https://docs.cline.bot/customization/skills)).
- Layout: a directory with required `SKILL.md` plus optional `docs/`, `templates/`, `scripts/`
  ([skills](https://docs.cline.bot/customization/skills)).
- Frontmatter: exactly two required fields — `name` (must **exactly match the directory name**) and `description`
  (**max 1024 characters**) ([skills](https://docs.cline.bot/customization/skills)).
- Discovery paths — project: `.cline/skills/` (recommended), `.clinerules/skills/`, `.claude/skills/`; global:
  `~/.cline/skills/` (and `C:\Users\USERNAME\.cline\skills\` on Windows)
  ([skills](https://docs.cline.bot/customization/skills)).
- **Name-collision rule: when a global skill and a project skill share a name, the GLOBAL skill takes precedence**
  ([skills](https://docs.cline.bot/customization/skills)). (Note this is the opposite direction from rules, where
  workspace wins — worth being careful about.)
- Scripts are token-efficient because "only their output enters context, not the code itself"
  ([skills](https://docs.cline.bot/customization/skills)).
- Skills can be invoked explicitly as slash commands (e.g. `/aws-deploy`)
  ([skills](https://docs.cline.bot/customization/skills)).
- Plugins can bundle skills via a top-level `skills/` directory next to `package.json`
  ([writing plugins](https://docs.cline.bot/sdk/guides/writing-plugins)).

## 1.11 Extensibility

**MCP**

- Purpose: connect Cline to external APIs/services, add custom tools beyond built-ins, support local **or** remote
  hosted servers ([MCP](https://docs.cline.bot/mcp/mcp-overview)).
- **Config file — doc conflict across three official pages:**
  - MCP overview: **CLI** → `~/.cline/mcp.json`; **IDE extensions** → configured via the panel's MCP Servers icon →
    Configure tab → "Configure MCP Servers" button, which "opens the MCP settings JSON used by the extension"
    ([MCP](https://docs.cline.bot/mcp/mcp-overview))
  - Tools reference: "MCP servers configured in **`.cline/mcp.json`**"
    ([tools reference](https://docs.cline.bot/tools-reference/all-cline-tools))
  - Config page: `~/.cline/data/settings/cline_mcp_settings.json` is listed as "MCP settings"
    ([config](https://docs.cline.bot/getting-started/config))
  - CLI reference also lists project-level `.cline/mcp.json`
    ([cli-reference](https://docs.cline.bot/cli/cli-reference))
- Config schema: top-level `mcpServers` object. **Local (STDIO):** `command`, `args`, `env`, `disabled`,
  **`autoApprove` (array of tool names)**. **Remote:** `type` (`"streamableHttp"` or `"sse"`), `url`, `headers`,
  `disabled`, `autoApprove` ([MCP](https://docs.cline.bot/mcp/mcp-overview)).
- **Transport default gotcha:** omitting `type` defaults to the **legacy `sse`** transport for backward compatibility;
  docs say set `"type": "streamableHttp"` explicitly for the recommended transport
  ([MCP](https://docs.cline.bot/mcp/mcp-overview)).
- **Per-MCP-tool auto-approve** is the `autoApprove` array on each server entry, on top of the global "Use MCP servers"
  permission. Docs security guidance: "Limit `autoApprove` to safe tools"
  ([MCP](https://docs.cline.bot/mcp/mcp-overview), [auto approve](https://docs.cline.bot/features/auto-approve)).
- CLI management: `cline mcp` wizard (List / Add / Edit / Enable-Disable / Delete); non-interactive listing via
  `cline config mcp` and `cline config mcp --json`; add-with-prefill
  `cline mcp install fs -- npx -y @modelcontextprotocol/server-filesystem /tmp` and remote
  `cline mcp install ctx7 --transport http https://mcp.context7.com/mcp`
  ([MCP](https://docs.cline.bot/mcp/mcp-overview),
  [apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
- MCP tools are "loaded alongside built-ins, so Cline can use both local tools and external integrations in one task"
  ([tools reference](https://docs.cline.bot/tools-reference/all-cline-tools)).
- Instrumented as `task.mcp_tool_called` with `status` (started/success/error), `tool_name`, `server_name`
  ([opentelemetry-events.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/opentelemetry-events.mdx)).
- Enterprise: MCP server allowlisting and remote MCP server management are admin controls
  ([mcp-server-controls](https://docs.cline.bot/enterprise-solutions/configuration/infrastructure-configuration/control-other-cline-features/mcp-server-controls)).

**MCP Marketplace (real, official, but docs page was removed)**

- It exists as a product surface — nav item "MCP Marketplace" at <https://cline.bot/mcp-marketplace> — and as the
  submission repo <https://github.com/cline/mcp-marketplace>.
- Purpose: a curated collection to browse official and community MCP servers, search by name/category/tags/metadata,
  and **install with one click, triggering Cline to autonomously handle cloning, setup, and configuration**
  ([mcp-marketplace README](https://raw.githubusercontent.com/cline/mcp-marketplace/main/README.md)).
- Submission requirements: GitHub repo URL, 400×400 PNG logo, reason for inclusion; submitted via the
  `mcp-server-submission.yml` issue template. Approval criteria = community adoption, developer credibility, project
  maturity, security (extra scrutiny for financial services and crypto)
  ([README](https://raw.githubusercontent.com/cline/mcp-marketplace/main/README.md)).
- **One-click install hint mechanism:** Cline reads the server's `README.md`; an **`llms-install.md`** file is optional
  extra agent guidance ([README](https://raw.githubusercontent.com/cline/mcp-marketplace/main/README.md)). The launch
  blog calls the convention `llms-installation.md` — a **minor naming discrepancy across official sources**
  ([blog](https://cline.ghost.io/introducing-the-mcp-marketplace-clines-new-app-store/)).
- **The docs-level marketplace page has been removed:** `docs.json` redirects `/mcp/mcp-marketplace` →
  `/mcp/mcp-overview` ([docs.json](https://raw.githubusercontent.com/cline/cline/main/docs/docs.json)), and
  `mcp_marketplace_enabled` survives as field 6 of `UpdateSettingsRequest`
  ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).

**Hooks**

- **The dedicated docs page is a stub.** Its full source is 143 bytes: `title: "Hooks"`,
  `description: "See details under SDK Plugins page."`, body `See details under [SDK Plugins](/sdk/plugins).`
  ([hooks.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/customization/hooks.mdx),
  [live page](https://docs.cline.bot/customization/hooks)). All older hook pages redirect to it
  ([docs.json](https://raw.githubusercontent.com/cline/cline/main/docs/docs.json)).
- **Extension hook lifecycle events:** `TaskStart` (new task, not resume), `TaskResume` (existing task resumed),
  `TaskCancel` (cancel or user-aborted hook; **not cancellable**), `TaskComplete` (**"coming soon"**),
  `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PreCompact` (**"coming soon"**)
  ([.clinerules/hooks/README.md](https://raw.githubusercontent.com/cline/cline/main/.clinerules/hooks/README.md)).
- **Locations:** extension → global `~/Documents/Cline/Hooks/`, workspace `.clinerules/hooks/`; CLI/SDK →
  `.cline/hooks/` (workspace), `~/.cline/hooks/` (global), or a custom `--hooks-dir`
  ([.clinerules/hooks/README.md](https://raw.githubusercontent.com/cline/cline/main/.clinerules/hooks/README.md),
  [clineignore.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/customization/clineignore.mdx),
  [config](https://docs.cline.bot/getting-started/config)).
- **Naming:** "must be named exactly after their event with **no file extension**" (`PreToolUse`, not `PreToolUse.sh`)
  and need a shebang plus the executable bit in the extension; the CLI/SDK variant uses `PreToolUse.sh` in
  `.cline/hooks/` ([clineignore.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/customization/clineignore.mdx),
  [.clinerules/hooks/README.md](https://raw.githubusercontent.com/cline/cline/main/.clinerules/hooks/README.md)).
- **No JSON config file** for hooks — they are discovered as executables on disk. The extension requires checking
  **"Enable Hooks"** in feature settings (**unverified:** no documented JSON config).
- **JSON payload (stdin)** common fields: `clineVersion`, `hookName`, `timestamp`, `taskId`, `workspaceRoots[]`,
  `userId`; per-event blocks: `taskStart.taskMetadata{taskId,ulid,initialTask}`,
  `taskResume.taskMetadata + previousState{lastMessageTs,messageCount,conversationHistoryDeleted}`,
  `taskCancel.taskMetadata{...,completionStatus}`, `userPromptSubmit{prompt,attachments[]}`,
  `preToolUse{toolName,parameters}`,
  `postToolUse{toolName,parameters,result,success,executionTimeMs}`,
  `preCompact{contextSize,messagesToCompact,compactionStrategy}`
  ([.clinerules/hooks/README.md](https://raw.githubusercontent.com/cline/cline/main/.clinerules/hooks/README.md)).
- **JSON output (stdout):** `{"cancel": boolean, "contextModification": string, "errorMessage": string}`
  ([same](https://raw.githubusercontent.com/cline/cline/main/.clinerules/hooks/README.md)).
- **SDK/plugin `HookStage`s** — a much richer surface: `input, runtime_event, session_start, run_start,
  iteration_start, turn_start, before_agent_start, tool_call_before, tool_call_after, turn_end, stop_error,
  iteration_end, run_end, session_shutdown, error`. `before_agent_start` injects context / modifies prompt or messages;
  `tool_call_before` audits or blocks ([plugins](https://docs.cline.bot/sdk/plugins)).
- SDK plugin hook methods: `beforeRun`, `afterRun`, `beforeModel`, `afterModel`, `beforeTool`, `afterTool`, `onEvent`
  ([plugins](https://docs.cline.bot/sdk/plugins)).
- **Hook policies** (a genuinely nice design detail): `mode` (`"blocking"`/`"async"`), `timeoutMs`, `retries`,
  `retryDelayMs`, **`failureMode` (`"fail_open"`/`"fail_closed"`)**, `maxConcurrency`, `queueLimit` — with the advice
  to use `fail_closed` for policy enforcement ([plugins](https://docs.cline.bot/sdk/plugins)).
- CLI: `cline hook` handles a hook payload from stdin (`cat payload.json | cline hook`)
  ([cli-reference](https://docs.cline.bot/cli/cli-reference)).
- Instrumented: `hooks.enabled`, `hooks.disabled`, `hooks.cancel_requested`, `hooks.context_modified`,
  `hooks.discovery_completed` (`hooks_count, global_count, workspace_count`), `hooks.execution` (`hook_name`, `status`,
  `duration_ms`, `context_modified`)
  ([opentelemetry-events.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/opentelemetry-events.mdx)).

**Plugins (SDK/CLI/Kanban only)**

- Scope warning repeated in the docs: "This feature currently only applies to Cline SDK, CLI, and Kanban. This feature
  is **not applicable on VSCode and JetBrains Extension** for now."
  ([plugins](https://docs.cline.bot/customization/plugins)).
- A plugin is an `AgentPlugin` object implementing the SDK extension interface. **Extension points:** Tool
  (model-callable action), Command (slash command), Hook (lifecycle logic/policy checks), Rules ("Prompts that steer
  the agent and will be included in every session"), Events (external events triggering agent actions), Plugin (bundles
  all of the above) ([plugins](https://docs.cline.bot/sdk/plugins)).
- Install sources: **file URL** (single `.ts`/`.js`; remote must be `https://`), **git repo** (with `@ref` for
  branch/tag; also `git@github.com:...`), **npm** (`cline plugin install npm:@scope/my-plugin` or `--npm my-plugin`),
  **local path** ([plugins](https://docs.cline.bot/customization/plugins)).
- Install flags: `--force`, `--json`, `--cwd <path>` (installs to `<path>/.cline/plugins` instead of global), plus
  `--npm` / `--git` type hints ([plugins](https://docs.cline.bot/customization/plugins),
  [cli-reference](https://docs.cline.bot/cli/cli-reference)).
- Plugin store layout: `~/.cline/plugins/` with `_installed/{npm,git,remote,local}/`; project at `.cline/plugins/`;
  also `~/Documents/Cline/Plugins/` for compatibility
  ([plugins](https://docs.cline.bot/customization/plugins), [config](https://docs.cline.bot/getting-started/config)).
- **Manifest format:** `package.json` with a top-level `cline` field → `cline.plugins` array of
  `{ paths: ["./index.ts"], capabilities: ["tools","hooks"] }` or a plain string path; each path must export an
  `AgentPlugin` ([plugins](https://docs.cline.bot/customization/plugins)).
- Auto-discovery fallback: if no `cline.plugins` field, the installer looks for standard entry points, then recursively
  scans `.ts`/`.js`, skipping `node_modules` and `.git`
  ([plugins](https://docs.cline.bot/customization/plugins)).
- **Host-provided dependencies:** `@cline/*` packages are provided by the host runtime and stripped before
  `npm install`; the host currently provides `@cline/sdk`, `@cline/core`, `@cline/agents`, `@cline/llms`,
  `@cline/shared` — declare them as optional peer dependencies
  ([plugins](https://docs.cline.bot/customization/plugins)).
- Single-file plugins "can only import Node builtins and `@cline/*`"; anything needing an npm dependency must ship as a
  package with `package.json` ([writing plugins](https://docs.cline.bot/sdk/guides/writing-plugins)).
- Design guidance: keep `setup()` synchronous and fast; register all tools in `setup()`, not in hooks; use hooks for
  observation (a thrown error in `beforeTool` counts as a **tool failure**)
  ([writing plugins](https://docs.cline.bot/sdk/guides/writing-plugins)).
- Reference example plugin: `typescript-lsp-plugin`, adding a `goto_definition` tool via the TypeScript Language
  Service API ([plugins](https://docs.cline.bot/customization/plugins)).
- **Unverified:** no documented per-plugin enable/disable toggle (unlike rules/skills/workflows, which all document
  toggles).

**Slash commands / reusable prompts**

- Built-in chat slash commands:

  | Command | What It Does |
  |---|---|
  | `/newtask` | Start fresh task with distilled context from current conversation |
  | `/smol` | Compress conversation history while preserving essential context (alias `/compact`) |
  | `/newrule` | Create a rule file to teach Cline your preferences |
  | `/deep-planning` | Investigate codebase, plan thoroughly, then create implementation task |
  | `/reportbug` | Report a bug with diagnostic info |

  Source: [using commands](https://docs.cline.bot/core-workflows/using-commands)
- Any **enabled skill** is also invokable as a slash command
  ([using commands](https://docs.cline.bot/core-workflows/using-commands)).
- **Custom workflow files: `.clinerules/workflows/`** — confirmed by the historical official Workflows doc and by the
  repo itself:
  - Historical doc: "**Workspace workflows** go in `.clinerules/workflows/` at your project root… **Global workflows**
    go in your system's Cline Workflows directory" (Windows `Documents\Cline\Workflows`; macOS and Linux/WSL
    `~/Documents/Cline/Workflows`). "Workspace workflows take precedence when names match global workflows"
    ([workflows.mdx @9dea336c](https://raw.githubusercontent.com/cline/cline/9dea336c/docs/customization/workflows.mdx)).
  - Current config doc: global workflows also resolve from `~/.cline/data/workflows/`, with
    `~/Documents/Cline/Workflows/` still supported as an additional search path; project-level `workflows/` is **not**
    listed in the current `.cline/` tree ([config](https://docs.cline.bot/getting-started/config)).
  - Repo evidence: `.clinerules/workflows/` contains `address-pr-comments.md`, `find-pr-reviewers.md`,
    `git-branch-analysis.md`, `hotfix-release.md`, `pr-review.md`, `release.md`, `writing-documentation.md`
    ([contents](https://api.github.com/repos/cline/cline/contents/.clinerules/workflows)).
- **Format:** markdown with a title and steps; **the filename becomes the command** — `demo-workflow.md` is invoked
  with `/demo-workflow.md`. Steps can be natural language or use the legacy XML tool syntax
  (`<execute_command>`, `<read_file>`, `<ask_followup_question>`, `<use_mcp_tool>`). Cline executes steps in sequence
  and pauses for approval ([workflows.mdx @9dea336c](https://raw.githubusercontent.com/cline/cline/9dea336c/docs/customization/workflows.mdx)).
- **No frontmatter** — the doc specifies only "a title and steps", and the real
  `.clinerules/workflows/release.md` starts directly with `# Release`
  ([workflows.mdx @9dea336c](https://raw.githubusercontent.com/cline/cline/9dea336c/docs/customization/workflows.mdx),
  [release.md](https://raw.githubusercontent.com/cline/cline/main/.clinerules/workflows/release.md)).
- **Current status caveat:** the Workflows page is no longer part of docs.cline.bot — `docs.json` redirects
  `/customization/workflows` → `/customization/cline-rules`, and the current "Using Commands" page documents only
  built-in slash commands + skills ([docs.json](https://raw.githubusercontent.com/cline/cline/main/docs/docs.json),
  [using commands](https://docs.cline.bot/core-workflows/using-commands)).
- **`.cline/commands` does not exist** as a documented location; the project `.cline/` tree documents only
  `rules/ skills/ hooks/ agents/ plugins/ cron/` ([config](https://docs.cline.bot/getting-started/config)).
- Plugins can register slash commands as the "Command" extension point
  ([plugins](https://docs.cline.bot/sdk/plugins)).

## 1.12 Provider abstraction & multi-model support

- **Any OpenAI-compatible endpoint is officially supported** via the provider **"OpenAI Compatible"**, configured with
  **Base URL**, API Key (or "**Use Azure Identity Authentication**" for managed identity), **Model ID**, and a
  **Model Configuration** section (Max Output Tokens, Context Window size, Image Support, Computer Use, Input Price per
  token/million, Output Price) ([openai-compatible](https://docs.cline.bot/provider-config/openai-compatible)).
  - Documented caveat: the base URL must **not** be `https://api.openai.com/v1` — that is the dedicated OpenAI
    provider ([openai-compatible](https://docs.cline.bot/provider-config/openai-compatible)).
  - Explicit per-model **price fields** in provider config is a notable design choice (enables cost display for
    arbitrary endpoints).
- Provider categories named by the README: Anthropic; OpenAI; Google; **OpenRouter** (200+ models); Vercel AI Gateway;
  **AWS Bedrock**; Azure / GCP Vertex; Cerebras / Groq; **Ollama / LM Studio**; "Any OpenAI-compatible API"
  ([README](https://raw.githubusercontent.com/cline/cline/main/README.md)).
- **"Other 30+ Providers"** reference list (each with its own setup section): AIHubMix, AskSage, Baseten, Cerebras,
  Dify.ai, Doubao, Fireworks AI, GCP Vertex AI, Groq, Hicap, Huawei Cloud MaaS, Hugging Face, Mistral, Moonshot,
  Nebius AI Studio, Nous Research, Oracle Code Assist, Qwen Code, Requesty, SambaNova, SAP AI Core, Together, Vercel AI
  Gateway, **VS Code Language Model API**, xAI (Grok)
  ([other-30-plus-providers](https://docs.cline.bot/provider-config/other-30-plus-providers)).
- **Local runtimes**, each documented: **Ollama** (base URL `http://localhost:11434`), **LM Studio** (server default
  `http://localhost:1234`), **Atomic Chat** (OpenAI-compatible, default `http://127.0.0.1:1337/v1`). The docs recommend
  enabling **"Use Compact Prompt"** in Cline Settings → Features for local inference, and give hardware guidance of
  16–32GB / 32–64GB / 64GB+ RAM
  ([running-models-locally/overview.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/running-models-locally/overview.mdx)).
- **Provider config storage:** `~/.cline/data/settings/providers.json` ("API keys and provider configuration"),
  `global-settings.json`, `cline_mcp_settings.json`; the CLI likewise uses `providers.json`
  ([config](https://docs.cline.bot/getting-started/config), [cli-reference](https://docs.cline.bot/cli/cli-reference)).
- SDK provider layer `@cline/llms` exports `DefaultGateway`, `createGateway`, `createHandler` /
  `createHandlerAsync`, `getAllProviders`, `getProviderIds`, `getModelsForProvider`, **`registerProvider`**,
  **`registerModel`**, `ModelInfo`, `ProviderInfo`. `Agent` accepts either a prebuilt `model: AgentModel` or
  `providerId`/`modelId` plus optional `apiKey`, `baseUrl`, `headers`
  ([model providers](https://docs.cline.bot/sdk/model-providers),
  [architecture/overview.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/architecture/overview.mdx)).
- SDK OpenAI-compatible path is explicit:
  `new Agent({ providerId: "openai-compatible", modelId, apiKey, baseUrl: "https://your-provider.com/v1" })`
  ([model providers](https://docs.cline.bot/sdk/model-providers)).
- Bedrock uses the standard AWS credential chain plus `providerConfig: { awsRegion: "us-east-1" }`
  ([model providers](https://docs.cline.bot/sdk/model-providers)).
- Environment variables by provider: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`,
  `GOOGLE_API_KEY` / `GOOGLE_APPLICATION_CREDENTIALS`, `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` /
  `AWS_SESSION_TOKEN`, `MISTRAL_API_KEY` ([model providers](https://docs.cline.bot/sdk/model-providers)).
- CLI provider flags: `-P/--provider <id>` (default `cline`), `-m/--model`, `-k/--key`, and
  `cline auth --provider openai-native --apikey sk-... --modelid gpt-5 --baseurl https://api.example.com/v1`;
  default model per the README options table is `anthropic/claude-sonnet-4.6`
  ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx),
  [apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
- Anthropic extras: a **custom base URL** checkbox; a **Claude Code** provider path using the Claude CLI plus a
  Claude Max/Pro subscription (documented limits: may not stream token-by-token, limited image uploads and prompt
  caching); and **Extended Thinking** as a checkbox below the model selector, where Claude 3.7+ returns only a summary
  of thinking but **you are billed for the full thinking tokens**
  ([anthropic.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/provider-config/anthropic.mdx)).
- **Model tiering is manual, not automatic.** Per-mode model selection is the documented lever
  ([plan-and-act.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/plan-and-act.mdx)). I found
  **no primary source describing automatic model tiering/routing**.
- Multi-model orchestration via config dirs is documented: `cline --config ~/.cline-haiku auth anthropic --modelid
  claude-haiku-4-20250514`, then `cline --config ~/.cline-opus "complex reasoning task"`, with `--thinking high|xhigh`
  and parallel reviews via shell `&` + `wait`
  ([model-orchestration.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/samples/model-orchestration.mdx)).
- Network layer: VS Code extension uses VS Code proxy settings automatically; CLI uses
  `http_proxy`/`https_proxy`/`no_proxy`; JetBrains uses IDE HTTP proxy settings. Documented limits: **HTTP proxies only
  — no SOCKS, no PAC, no auth beyond basic user/password**
  ([networking-and-proxies.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/troubleshooting/networking-and-proxies.mdx)).
- Certificate trust: the `cline` wrapper harvests OS trust anchors into `~/.cline/cli-node-extra-ca-certs.pem` and
  points `NODE_EXTRA_CA_CERTS` at it; a user-set `NODE_EXTRA_CA_CERTS` is **merged, not replaced**
  ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
- **Unverified:** the VS Code–specific `settings.json` inside the extension's `globalStorage` (the classic Cline
  location) is not mentioned in current docs; only the unified `~/.cline/` layout is described. Also,
  `cline-rules` links to `/getting-started/config#storage-locations`, but the current config page has **no such
  heading** (broken anchor)
  ([rules](https://docs.cline.bot/customization/cline-rules), [config](https://docs.cline.bot/getting-started/config)).

## 1.13 Cost & token control

- **Per-task cost display:** "Cline tracks these costs automatically and displays them in the **task header**"; the
  estimate updates after each API request using the selected provider's pricing and "may vary slightly from your final
  bill" ([task-management.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/task-management.mdx)).
- The persisted task record carries exactly `total_cost` (double), `tokens_in`, `tokens_out`, `cache_writes`,
  `cache_reads` ([proto/cline/task.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/task.proto)).
  - Note the harness distinguishes **cache writes from cache reads** at the data-model level — caching is first-class
    in accounting.
- Task history can be sorted by **"Most Expensive" / "Most Tokens"**
  ([task-management.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/task-management.mdx)).
- SDK: `ClineCore.getAccumulatedUsage(sessionId)` returns session token/cost totals; a `usage` event carries
  `inputTokens`, `outputTokens`, `cacheReadTokens`, `cacheWriteTokens`, `cost` plus running `total*` equivalents
  ([docs/sdk/clinecore.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/clinecore.mdx),
  [events reference](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/reference/events.mdx)).
- **Per-subagent cost:** subagent "tokens and API spend are tracked separately per subagent and rolled into the task's
  total cost", with per-subagent stats (tool calls, tokens, cost) visible live in the chat UI
  ([subagents](https://docs.cline.bot/features/subagents)).
- **TUI status area** shows "the active model, **context usage, cost**, workspace/branch, git diff stats, Plan/Act
  state, and whether auto-approve all is enabled" ([TUI](https://docs.cline.bot/usage/tui)).
- **Cron-run accounting:** schedule execution history shows "status, duration, **tokens, and cost**", plus statistics
  for success rate, average duration, last failure ([scheduling](https://docs.cline.bot/cli/scheduling)).
- **Prompt caching** is tracked automatically: "Some providers also support prompt caching, which reduces costs when
  the same context (like your cline rules or large files) appears in multiple requests. **Cline automatically tracks
  cache savings when available.**"
  ([task-management.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/task-management.mdx)).
  - Provider-level cache toggles in the settings schema: `aws_bedrock_use_prompt_cache` (field 3) and
    `lite_llm_use_prompt_cache` (field 26)
    ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).
  - Anthropic: "Claude 3 models support prompt caching, which can significantly reduce costs and latency for repeated
    prompts"; context window 200,000 tokens
    ([anthropic.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/provider-config/anthropic.mdx)).
  - Auto Compact explicitly relies on the existing prompt cache, so "you're primarily paying for summary generation
    (output tokens)" ([auto compact](https://docs.cline.bot/features/auto-compact)).
  - The ClinePass pricing table lists **Cached Read and Cached Write rates per model**, evidence caching is measured
    per model ([clinepass.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/getting-started/clinepass.mdx)).
  - **Unverified:** a per-provider cache-support matrix.
- **Thinking budget as a cost knob:** `--thinking <none|low|medium|high|xhigh>`, default `medium`, and
  `--ak thinking=6000` for evals
  ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx),
  [hill climbing](https://cline.ghost.io/a-practical-guide-to-hill-climbing/)).
  Cline's own eval data warns against assuming more thinking is better: "**medium thinking outperformed high,
  extra-high, and maximum**" on Opus 5's FrontierCode main split, and "thinking tokens are very expensive output
  tokens" ([open-sourcing evals](https://cline.ghost.io/open-sourcing-evals-for-open-weight-agents/)).
- Billing-model summary from the docs: Cline Provider = pay-per-use credits; ClinePass = flat monthly subscription
  against a quota (**three limits: a 5-hour rolling window, weekly, and monthly**); direct API keys = billed by your
  provider; OpenRouter/Requesty = aggregated billing; **local models = free**
  ([task-management.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/task-management.mdx),
  [clinepass.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/getting-started/clinepass.mdx)).
- ClinePass model IDs use a `cline-pass/<model>` slug form, e.g. `cline-pass/glm-5.3`, `cline-pass/kimi-k3`,
  `cline-pass/deepseek-v4-pro` ([clinepass.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/getting-started/clinepass.mdx)).

## 1.14 Session persistence, resume, fork, task history

**What a task/session is**

- "Every interaction with Cline happens within a **task**." Each task has a unique identifier and dedicated storage
  directory, contains the full conversation history, "Tracks token usage, API costs, and execution time", "Can be
  interrupted and resumed across sessions", and creates checkpoints
  ([task-management.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/task-management.mdx)).
- Scoping guidance: "**one task = one goal**"; when unsure, "err on the side of starting fresh"
  ([task-management.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/task-management.mdx)).
- Legacy naming calls it a **task**; the current SDK calls the same thing a **session**
  ([task-management.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/task-management.mdx),
  [docs/sdk/clinecore.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/clinecore.mdx)).

**Legacy VS Code harness persistence (precise file layout — useful prior art)**

Per-task directory `<globalStorageFsPath>/tasks/<taskId>/` with these files, from `GlobalFileNames`
([disk.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/storage/disk.ts)):

- `api_conversation_history.json` — the model-facing message array (Anthropic message format)
- `ui_messages.json` — the UI/`ClineMessage[]` transcript (legacy `claude_messages.json` migrated on read)
- `context_history.json` — context truncation/dedup edits with timestamps
- `task_metadata.json` — `{ files_in_context, model_usage }`
- `settings.json` — per-task settings overlay

Task history index: `<stateDir>/taskHistory.json`, where state is `<globalStorageFsPath>/state/`
([disk.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/storage/disk.ts)).
**Key design detail:** the **model-facing transcript and the UI transcript are stored separately** — a clean
separation that makes context surgery (truncation, dedup) auditable without corrupting what the user sees.

**Current CLI/SDK persistence**

- Sessions at `~/.cline/data/sessions/` with a SQLite index plus one authoritative JSON record per session
  ([hub-spoke.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/architecture/hub-spoke.mdx)):

  ```
  ~/.cline/data/sessions/
    sessions.db          # SQLite index
    [session-id].json    # Authoritative session record
  ```

- Sessions have multiple participants each with a role: **`creator`, `participant`, `observer`**
  ([hub-spoke.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/architecture/hub-spoke.mdx)).
  This is the mechanism behind multi-client attach.
- `ClineCore` persistence API: `list(limit?, options?)`, `get(sessionId)`, `readMessages(sessionId)`,
  `getAccumulatedUsage(sessionId)`, `send({sessionId, prompt})`, `abort(sessionId, reason?)`, `stop(sessionId)`,
  `delete(sessionId)`, `dispose(reason?)`. A session result exposes `sessionId`, `manifest`, `manifestPath`,
  `messagesPath`, and the final `result`
  ([docs/sdk/clinecore.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/clinecore.mdx)).
- `CLINE_DATA_DIR` replaces `~/.cline/data/`; `cline --data-dir <path>` uses isolated local state (and **enables
  sandbox mode automatically**); `cline --config <path>` sets the config dir
  ([config](https://docs.cline.bot/getting-started/config),
  [cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx)).

**Resuming**

- Documented resume flow: open the task from history → Cline loads the complete conversation → **file states are checked
  against checkpoints** → the task continues with awareness of the interruption
  ([task-management.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/task-management.mdx)).
- "This works across sessions. Even if you close the editor and return days later, Cline can pick up where you left
  off." ([task-management.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/task-management.mdx)).
- CLI: `cline --id <session-id>` resumes by ID; `cline history` / `cline h` lists or manages saved sessions
  ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx)).
- TUI: `/history`, `Ctrl+L` to clear
  ([TUI](https://docs.cline.bot/usage/tui)).
- Team state persists so `cline --team-name auth-sprint "Continue with incomplete tasks"` resumes
  ([agent teams](https://docs.cline.bot/cli/agent-teams)).

**Search & history management**

- Fuzzy search across prompts, responses, code snippets and file names; sort by Newest/Oldest, Most Expensive, Most
  Tokens, Most Relevant, Favorites; favourited tasks are protected from deletion
  ([task-management.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/core-workflows/task-management.mdx)).
- Legacy RPCs: `getTaskHistory(GetTaskHistoryRequest{favorites_only, search_query, sort_by, current_workspace_only})`,
  `toggleTaskFavorite`, `deleteTasksWithIds`, `deleteAllTaskHistory`, **`exportTaskWithId`** (exports a task to
  markdown), `getTotalTasksSize`
  ([proto/cline/task.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/task.proto)).

**Fork**

- **No "fork task" feature was found in any primary Cline source.** The nearest equivalents:
  - **Restore Task Only** — deletes messages after a checkpoint, letting you retry with a different prompt while keeping
    files ([checkpoints](https://docs.cline.bot/core-workflows/checkpoints)).
  - `/newtask` — a fresh task seeded with distilled context
    ([using commands](https://docs.cline.bot/core-workflows/using-commands)).
  - Legacy `new_task` tool — creates a new task with a **mandatory 5-section context summary** (Current Work / Key
    Technical Concepts / Relevant Files and Code / Problem Solving / Pending Tasks and Next Steps **with verbatim
    quotes**), with a user-visible preview
    ([snapshot @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/anthropic_claude_sonnet_4-basic.snap)).
  - **Kanban resume ID** — trashed cards keep a resume ID
    ([kanban core workflow](https://raw.githubusercontent.com/cline/cline/main/docs/kanban/core-workflow.mdx)).
  - **Kanban task linking** — ⌘/Ctrl+click links cards into dependency chains that auto-start the next task
    ([kanban core workflow](https://raw.githubusercontent.com/cline/cline/main/docs/kanban/core-workflow.mdx)).
  - The deprecations page mentions no forking, and the settings proto has no fork-related field
    ([deprecations](https://docs.cline.bot/resources/deprecations),
    [proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).

## 1.15 Subagents & parallel orchestration

**Subagents (`use_subagents` tool) — experimental, on by default**

- Explicitly flagged: "Subagents is an **experimental** feature. Behavior may change in future releases."
  ([subagents](https://docs.cline.bot/features/subagents)).
- Spawned via the **`use_subagents` tool**, which "launches independent agents simultaneously". Each gets its own
  prompt, "Runs with a **separate context window and token budget**", explores independently, and returns a report
  "focused on the most relevant file paths for the main agent to read next". Design intent: "This keeps the main
  agent's context clean while gathering broad information fast"
  ([subagents](https://docs.cline.bot/features/subagents)).
- Enabled **by default**; Cline decides when parallel research is worth the overhead. Disable by turning off the
  `use_subagents` tool at **Settings → Features → Agent**; the setting applies across VS Code, JetBrains and CLI
  ([subagents](https://docs.cline.bot/features/subagents)).
- Setting keys: `subagents_enabled` (field 125 of `Settings`, field 29 of `UpdateSettingsRequest`) and
  `subagent_terminal_output_line_limit` (fields 126 / 30)
  ([proto/cline/state.proto @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto)).
- **Read-only by construction.** Allowed: `read_file`, `list_files`, `search_files`,
  `list_code_definition_names`, `execute_command` (read-only only), `use_skill`. Forbidden: writing files, applying
  patches, using the browser, accessing MCP servers, performing web searches, and **spawning nested subagents**.
  Commands run in the background and "will not run commands that modify files or system state"
  ([subagents](https://docs.cline.bot/features/subagents)).
- **Auto-approve coupling:** subagents follow the **Read project files** permission — launches are auto-approved if
  that is on; otherwise Cline asks first and **shows the prompts it plans to send**
  ([subagents](https://docs.cline.bot/features/subagents)).
- **Cost:** per-subagent tokens/API spend tracked separately and rolled into the task total
  ([subagents](https://docs.cline.bot/features/subagents)).
- Legacy CLI-subagent mechanism (different implementation, same intent): the prompt told the model to shell out with
  `cline "your prompt here"`, with guidance "only create agents when it seems likely you may be exploring across 10 or
  more files", and the section is omitted when `isCliSubagent` to prevent nesting
  ([components/cli_subagents.ts @be7548ff](https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/components/cli_subagents.ts)).
- SDK switch: `enableSpawnAgent: true`; persisted **within session only**, no shared state
  ([multi-agent teams](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/guides/multi-agent-teams.mdx)).

**Agent Teams (persistent, coordinator + specialists) — CLI/SDK/Kanban only**

- Scope warning: "This feature currently only applies to Cline SDK, CLI, and Kanban. This feature is not applicable on
  VSCode and JetBrains Extension for now." ([agent teams](https://docs.cline.bot/cli/agent-teams)).
- "One agent acts as the **coordinator**, delegating subtasks to specialist agents", coordinating "through a shared task
  board" ([agent teams](https://docs.cline.bot/cli/agent-teams)).
- Enabled with `cline --team-name auth-sprint "Plan and implement user authentication with tests"`; the coordinator
  "gets additional tools for spawning teammates and delegating tasks". Disabled with `cline --no-teams`; **teams are
  enabled by default** ([agent teams](https://docs.cline.bot/cli/agent-teams)).
- Coordinator tools: `team_spawn_teammate`, `team_delegate_task`, `team_check_status`, `team_get_result`
  ([multi-agent teams](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/guides/multi-agent-teams.mdx)).
- **Durable state** at `~/.cline/data/teams/[team-name]/` containing **`task-board.json`** (tasks + status),
  **`mailbox.json`** (inter-agent messages), and **`mission-log.json`** (activity history)
  ([multi-agent teams](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/guides/multi-agent-teams.mdx),
  [agent teams](https://docs.cline.bot/cli/agent-teams)).
- Config: `enableAgentTeams: true` and `teamName` in ClineCore session config
  ([multi-agent teams](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/guides/multi-agent-teams.mdx)).
- **Documented distinction table:** Sub-Agents = parent-child, within-session persistence, no shared state;
  Teams = peer-to-peer with task board, cross-session persistence, task board + mailbox + mission log
  ([multi-agent teams](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/guides/multi-agent-teams.mdx)).
- "Teams add overhead" — recommended only when work decomposes into independent subtasks, benefits from different
  system prompts, spans multiple sessions, or needs a persistent delegation record
  ([multi-agent teams](https://raw.githubusercontent.com/cline/cline/main/docs/sdk/guides/multi-agent-teams.mdx)).
- Interactive form: `/team <prompt>` ([agent teams](https://docs.cline.bot/cli/agent-teams)).

**Kanban — parallel agents via git worktrees**

- Research preview: "Some features described here use experimental capabilities. Expect changes."
  ([kanban](https://docs.cline.bot/usage/kanban)). Launch `npx kanban` from a git repo root (Node 18+); also
  `cline kanban` ([kanban](https://docs.cline.bot/usage/kanban),
  [cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx)).
- **Isolation mechanism is the key design choice:** hitting play on a card creates an **ephemeral git worktree** —
  "an isolated copy of your repo where the agent can make changes without affecting your main working directory or
  other tasks". "Multiple tasks run in parallel, each in their own worktree, so agents never create merge conflicts with
  each other" ([kanban core workflow](https://raw.githubusercontent.com/cline/cline/main/docs/kanban/core-workflow.mdx)).
- Gitignored files such as `node_modules` are **symlinked** from the main repo into the worktree to avoid slow
  reinstalls — with a documented caveat that agents modifying those files **write through to the main repo**
  ([kanban core workflow](https://raw.githubusercontent.com/cline/cline/main/docs/kanban/core-workflow.mdx)).
- Dependency chains: when a linked card completes and is moved to trash, the next linked task **automatically starts**,
  "enabling fully autonomous chains"
  ([kanban core workflow](https://raw.githubusercontent.com/cline/cline/main/docs/kanban/core-workflow.mdx)).
- Shipping: **Commit** (merge worktree changes into a commit on the base branch) or **Open PR** (new branch + PR); in
  both cases Kanban sends a dynamic prompt to the agent, which "**intelligently handles merge conflicts if the base
  branch has moved**" ([kanban core workflow](https://raw.githubusercontent.com/cline/cline/main/docs/kanban/core-workflow.mdx)).
- Auto-commit / Auto-PR are optional settings; sidebar chat can orchestrate ("ask an agent to break work into cards and
  start flows") ([kanban](https://docs.cline.bot/usage/kanban)).
- **Multi-vendor:** Kanban "works with Cline CLI, **Claude Code, Codex, OpenCode**, and related runtimes available in
  settings" ([kanban](https://docs.cline.bot/usage/kanban)).

## 1.16 Streaming output & TUI/GUI/IDE UX

- Three interactive surfaces: IDE (VS Code, JetBrains), **TUI** (`cline`, `cline -i` / `--tui`), and the **Kanban**
  web board ([TUI](https://docs.cline.bot/usage/tui), [kanban](https://docs.cline.bot/usage/kanban)).
- TUI is built on **OpenTUI** with markdown rendering, syntax-highlighted diffs, scrollable chat and mouse support
  ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
- TUI keys: `Tab` toggles Plan/Act; **`Shift+Tab` toggles auto-approve all**; `Ctrl+C` aborts a running turn (press
  again to exit); `Ctrl+D` exits when prompt empty and idle; `Ctrl+L` clears the chat view
  ([TUI](https://docs.cline.bot/usage/tui)).
- TUI slash commands: `/settings`, `/model`, `/account`, `/mcp`, `/compact`, `/undo`, `/clear`, `/history`, `/help`,
  `/quit`; `@file` mentions for workspace context. Status area shows model, context usage, cost, workspace/branch, git
  diff stats, Plan/Act state, auto-approve-all state ([TUI](https://docs.cline.bot/usage/tui)).
- `/undo` in the TUI is the checkpoint-restore affordance in the terminal ([TUI](https://docs.cline.bot/usage/tui)).
- Double `Ctrl+C` within 1 second exits; a single `Ctrl+C` shows a "ctrl+c to exit" message for 1 second
  ([extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts))
  — *note: this specific item is from the Continue CLI, not Cline; listed here only to avoid confusion, see Part 2.*
- **Streaming protocol (SDK):** the runtime emits typed deltas, e.g.
  `agent.subscribe((event) => { if (event.type === "assistant-text-delta") process.stdout.write(event.text ?? "") })`.
  Host-facing event categories: **Content** (`content_start`, `content_update`, `content_end`) for text/reasoning/tool
  UI; **Iterations** (`iteration_start`, `iteration_end`); **Usage** (`usage`); **Notices** (`notice`) for recovery and
  status; **Completion** (`done`, `error`)
  ([events](https://docs.cline.bot/sdk/events)).
- Session-level subscription: `cline.subscribe(listener)` with an optional `{ sessionId }` filter
  ([events](https://docs.cline.bot/sdk/events)).
- **ACP (Agent Client Protocol)** extends the CLI into third-party editors: "Cline CLI speaks the Agent Client
  Protocol (ACP), an open standard that lets editors and other tools drive terminal-based coding agents. Any
  ACP-capable client can use Cline as its coding agent without a dedicated extension." Started with `cline --acp`,
  communicating over **stdio** ([ACP](https://docs.cline.bot/usage/acp)).
  - Documented clients: **Zed** (`agent_servers` → `{"type":"custom","command":"cline","args":["--acp"],"env":{}}` in
    `settings.json`), **JetBrains** AI Assistant (`~/.jetbrains/acp.json`), **Neovim** via CodeCompanion's built-in
    `cline_cli` adapter (or avante.nvim / agentic.nvim), **Emacs** via agent-shell; the ACP client directory maintains
    the full list ([ACP](https://docs.cline.bot/usage/acp)).
  - What works over ACP: sign-in from the client (Cline, ClinePass, or ChatGPT subscription; `cline auth` credentials
    reused), Plan/Act switching, model and provider selection, **permission prompts through the client UI (nothing
    auto-approved by default)**, session resume, images for vision models, organization switching
    ([ACP](https://docs.cline.bot/usage/acp)).
  - **The `--auto-approve` default flips to `false` in ACP mode**
    ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx)).
  - ACP env vars under `env`: `CLINE_API_KEY`, `CLINE_PROVIDER` (pins the provider, disabling switching), `CLINE_MODEL`
    ([ACP](https://docs.cline.bot/usage/acp)).
  - Path caveat: if a client can't spawn the agent it may not inherit `PATH` — use the absolute path from `which cline`
    ([ACP](https://docs.cline.bot/usage/acp)).
- Kanban's review loop is diff-centric with inline line comments, checkpoint-scoped diffs by message range, and the
  agent's TUI embedded in the card detail view
  ([kanban core workflow](https://raw.githubusercontent.com/cline/cline/main/docs/kanban/core-workflow.mdx)).
- **Desktop app** exists for macOS/Windows (Tauri + Bun sidecar + Next.js)
  ([README](https://raw.githubusercontent.com/cline/cline/main/README.md)).

## 1.17 Non-interactive / headless / CI mode & scriptability

- Install: `npm install -g cline` (nightly: `cline@nightly`); **Node.js 20+ required (22 recommended)**; platform
  binaries are published for macOS, Linux, Windows on `arm64` and `x64`, resolved via optional dependencies, so no
  Node/Bun/Zig runtime is needed at install time
  ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md),
  [installing-cline.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/getting-started/installing-cline.mdx)).
- Prereq: provider authenticated via `cline auth` ([CLI overview](https://docs.cline.bot/usage/cli-overview)).
- Quick start: `cline` (interactive), `cline "refactor this module to use async/await"`, and
  `cline --json "list TODO comments"` ([CLI overview](https://docs.cline.bot/usage/cli-overview)).
- **Headless activation is implicit as well as explicit:** "Headless is triggered when using flags like `--json`, when
  **stdin is piped**, or when **output is redirected**"
  ([CLI overview](https://docs.cline.bot/usage/cli-overview)).
- Prompt input forms: `cline "your prompt"` (defaults to act mode with auto-approve enabled); `echo "prompt" | cline`;
  `cline` alone enters interactive mode ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx)).
- `--json` is non-interactive and requires either a prompt argument or piped stdin; `--key` takes precedence over
  environment variables ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
- **JSON output is NDJSON** — one message object per line, e.g.
  `{"type":"say","text":"I'll create the file now.","ts":1760501486669,"say":"text"}`, documented fields `type`
  (`"ask"`|`"say"`), `text`, `ts` (Unix ms), `say`, `ask`, `reasoning`, `partial` (streaming flag)
  ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx)).
  - **Doc conflict — the JSON envelope is not stable across official sources.** The CLI README and the GitHub
    integration sample parse a different shape:
    `jq -r 'select(.type == "agent_event" and .event.type == "done") | .event.text'`
    ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md),
    [github-integration.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/samples/github-integration.mdx)).
    An older headless doc claimed `--json` follows `~/.cline/data/tasks/<id>/ui_messages.json` with `jq '.text'`
    ([three-core-flows.mdx @9dea336c](https://raw.githubusercontent.com/cline/cline/9dea336c/docs/cline-cli/three-core-flows.mdx)).
- **Exit codes are NOT documented.** No exit-code table exists in the CLI reference, CLI overview, or CLI README;
  only "process exits automatically when complete" appears in the archived headless doc
  ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx),
  [CLI overview](https://docs.cline.bot/usage/cli-overview),
  [three-core-flows.mdx @9dea336c](https://raw.githubusercontent.com/cline/cline/9dea336c/docs/cline-cli/three-core-flows.mdx)).
  **Treat exit codes as unverified.**
- Full global flag set (CLI reference + CLI README): `-p/--plan`, `-t/--timeout <seconds>` (default `0`), `-m/--model`,
  `-P/--provider` (default `cline`), `-k/--key`, `-c/--cwd`, `--config <path>` (default `~/.cline/data/settings`),
  `--data-dir <path>` (default `~/.cline`; enables sandbox mode automatically), `--thinking
  <none|low|medium|high|xhigh>` (default `medium` per the reference; the README says thinking is off when the flag is
  omitted — **doc conflict**), `--retries <count>` (README default `3`; reference describes it as "maximum consecutive
  mistakes before halting"), `--hooks-dir <path>` (default `~/.cline/hooks`), `--acp`, `-s/--system`,
  `-z/--zen`, `-v/--verbose`, `-V/--version`, `--auto-approve <boolean>` (default `true`, `false` in ACP mode),
  `-y/--yolo` (skip approval prompts, enable `submit_and_exit`, disable spawn/team tools by default),
  `--compaction <agentic|basic|off>` (default `agentic`)
  ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx),
  [apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
- **`-z/--zen`** is a notable pattern: submit the task to the local hub daemon and **exit immediately**; runs with full
  tool auto-approval (same semantics as yolo), spawn/team disabled, incompatible with `--data-dir` and `--tui`;
  retrieve later with `cline history` ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
  This is only possible because of the hub-spoke architecture.
- Top-level commands: `auth`, `config`, `connect`, `mcp`, `dev`, `doctor`, `history|h`, `hook`, `plugin`, `schedule`,
  `hub`, `update`, `version`, `kanban`; the README adds `cline doctor fix` (kill stale local RPC listeners) and
  `cline doctor log` (open the runtime log file)
  ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx),
  [apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
- **Automation patterns** are a documented docs section: pipe context in, chain tasks, restrict command execution,
  include images, set execution timeout; there is a documented **JSON Output Schema** section
  ([CLI overview](https://docs.cline.bot/usage/cli-overview)).
- Images in tasks: `cline -i "fix the layout issue shown in @./screenshot.png"` or inline `@./design-mockup.png`
  ([CLI overview](https://docs.cline.bot/usage/cli-overview)).
- **CI/CD samples ship in the repo** — a `@cline`-mention responder workflow (`.github/workflows/cline-responder.yml`,
  installing Node 22, `npm install -g cline`, `cline auth --provider openrouter --apikey ${{ secrets.OPENROUTER_API_KEY }}`,
  running `cline --auto-approve true --json` and extracting the summary via jq)
  ([github-integration.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/samples/github-integration.mdx)).
  Official production PR-review workflow:
  <https://github.com/cline/cline/blob/main/.github/workflows/cline-pr-review.yml>
  ([model-orchestration.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/samples/model-orchestration.mdx)).
  Recommendation: run on a clean branch since autonomous execution modifies files without prompts
  ([CLI overview](https://docs.cline.bot/usage/cli-overview)).
- **Cron scheduling:** `cline schedule` opens a wizard (create, list, upcoming runs, active executions, trigger now,
  pause/resume, execution history with status/duration/tokens/cost, statistics, delete). Creation by flag:
  `cline schedule create "PR summary" --cron "0 9 * * MON-FRI" --prompt "..." --workspace /path/to/repo
  --model anthropic/claude-sonnet-4-6`; the README adds `--timeout 3600`, `--tags automation,review`, and
  `--delivery-adapter` / `--delivery-bot` / `--delivery-thread` for routing results to chat surfaces. Standard 5-field
  cron syntax ([scheduling](https://docs.cline.bot/cli/scheduling),
  [apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
  - **Scheduling requires the hub** (started automatically when you create a schedule); scheduled agents "persist
    across process restarts and run independently of any terminal session"
    ([scheduling](https://docs.cline.bot/cli/scheduling)).
  - Cron specs live in `~/.cline/cron/` (global) and `.cline/cron/` (workspace); SQLite `cron.db` under
    `~/.cline/data/db/` ([config](https://docs.cline.bot/getting-started/config)).
- **Chat-platform connectors:** `cline connect` wizard connects the agent to **Telegram, Slack, Discord, Google Chat,
  WhatsApp, Linear**. "Each incoming message creates or continues an agent session, and the agent's response is sent
  back to the conversation." Direct commands: `cline connect telegram|slack|discord|gchat|whatsapp|linear`
  ([connectors](https://docs.cline.bot/cli/connectors)).
  - Connector **security** is via `--allowed-user-id`, or a `--hook-command` receiving
    `{"payload":{"actor":{"participantKey","displayName"},"message":"..."}}` on stdin and returning
    `{"action":"allow"}` or `{"action":"deny","message":"reason"}`. **Without either, everything is auto-approved**
    ([connectors](https://docs.cline.bot/cli/connectors)).
  - Slack maps **thread → agent session** (context preserved per thread); socket mode is single-workspace only and
    needs `connections:write`; connectors require the hub (`cline hub start`); stop with `cline connect --stop [adapter]`
    ([connectors](https://docs.cline.bot/cli/connectors)).
  - Discord specifics: webhook path `/api/webhooks/discord`, `--enable-tools`, `--port`, `/health` endpoint, and
    in-chat commands `/help`, `/start`, `/new`, `/clear`, `/whereami`, `/tools [on|off|toggle]`,
    `/yolo [on|off|toggle]`, `/cwd [path]`, `/schedule create/list/trigger/delete`, `/abort`, `/exit`
    ([connectors](https://docs.cline.bot/cli/connectors)).

## 1.18 Observability

- **Log file locations (documented):**
  - Runtime log default `<CLINE_DATA_DIR>/logs/cline.log` via `CLINE_LOG_PATH`; controlled by `CLINE_LOG_ENABLED`,
    `CLINE_LOG_LEVEL` (`trace|debug|info|warn|error|fatal|silent`, default `info`), `CLINE_LOG_NAME`
    ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
  - `~/.cline/data/logs/` containing `hub-daemon.log`
    ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx)).
  - `~/.cline/cline-core-service.log` for CLI and JetBrains, used to verify proxy configuration and network errors
    ([networking-and-proxies.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/troubleshooting/networking-and-proxies.mdx)).
  - Session database is SQLite under `~/.cline/data/sessions/`; other SQLite DBs under `~/.cline/data/db/`
    ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx),
    [config](https://docs.cline.bot/getting-started/config)).
- **Log views:** `cline dev log` opens the CLI log file
  ([config](https://docs.cline.bot/getting-started/config)); `cline doctor log` opens the CLI runtime log
  ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)); in VS Code the extension
  writes to the **Output panel (Cline channel)**
  ([.clinerules/hooks/README.md](https://raw.githubusercontent.com/cline/cline/main/.clinerules/hooks/README.md)).
  `cline -v/--verbose` shows verbose runtime diagnostics including elapsed time, tokens and estimated cost when
  available ([apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).
- **OpenTelemetry: yes, official and opt-in.** Exports **metrics and logs** via OTLP to your own observability
  infrastructure; formats **gRPC (default, recommended)**, **HTTP/protobuf**, and **HTTP/JSON**
  ([opentelemetry](https://docs.cline.bot/enterprise-solutions/monitoring/opentelemetry)).
  - Configuration is via **Remote Configuration from the Cline dashboard, not a local JSON file** — endpoint, protocol,
    opt-out of TLS for gRPC, per-signal custom protocols/endpoints, metrics export interval, logs batch size / batch
    timeout / max queue size, and custom auth headers
    ([opentelemetry](https://docs.cline.bot/enterprise-solutions/monitoring/opentelemetry)).
  - Debug: `TEL_DEBUG_DIAGNOSTICS=true code .` outputs configuration, exporters created, connection attempts, and
    export successes/failures ([opentelemetry](https://docs.cline.bot/enterprise-solutions/monitoring/opentelemetry)).
  - **Documented OTel limitations:** OTLP metrics ✅, OTLP logs ✅, basic config via Remote Configuration ✅;
    **distributed tracing ❌ (not yet implemented)**, **custom instrumentation API ❌**, **sampling configuration ❌
    (uses defaults)** ([opentelemetry](https://docs.cline.bot/enterprise-solutions/monitoring/opentelemetry)).
  - Setting name appearing in docs: `openTelemetryMetricExportInterval`
    ([opentelemetry-events.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/opentelemetry-events.mdx)).
- **Event namespaces instrumented:** `user.*`, `task.*`, `workspace.*`, `ui.*`, `hooks.*`, `worktree.*`, `host.*`,
  `test.*`, plus `cline.test.connection`
  ([opentelemetry-events.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/opentelemetry-events.mdx)).
- **Event envelope schema:**
  `{ event, timestamp, attributes{}, resource{ service_name:"cline", service_version, host_type: vscode|jetbrains|cli } }`
  ([opentelemetry-events.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/opentelemetry-events.mdx)).
- **Representative events:** `task.tool_used` (`tool_name, success, duration_ms, auto_approved`),
  `task.mcp_tool_called`, `task.browser_tool_start/end/error`, `task.terminal_execution`,
  `task.terminal_output_failure`, `task.terminal_user_intervention`, `task.terminal_hang` (with `command_hash`),
  `task.tokens` (`task_id, tokens_in, tokens_out, cached_tokens, cost, model, provider`),
  `task.conversation_turn` (`role`, `provider`, `model`, `tokens_in`, `tokens_out`),
  `task.completed` (`duration_ms`, `model`, `provider`, `tokens_total`), `task.summarize_task`,
  **`task.auto_condense_toggled`**, `task.provider_api_error`, `task.diff_edit_failed`,
  `task.ai_output.accepted/rejected`, `task.skill_used`, `task.subagent_*`, `ui.rules_menu_opened`
  ([opentelemetry-events.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/opentelemetry-events.mdx)).
- **Privacy:** file paths, command content/arguments, user identifiers (anonymized tokens) and branch names are
  **hashed, never logged in raw form**
  ([opentelemetry-events.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/opentelemetry-events.mdx)).
- Built-in (non-OTel) telemetry: anonymous usage events, user-toggleable via a "Cline Telemetry" setting; enterprise
  admins can set the default with remote config `{"telemetryEnabled": true}` while users can still opt out locally.
  Telemetry never includes code/file contents, file paths or names, command arguments, conversation content, personal
  info, or credentials ([telemetry.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/telemetry.mdx)).
- Enterprise **Prompt Storage** — automatic backup of conversation history to AWS S3 or Cloudflare R2 for
  compliance/audit/DR ([telemetry.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/telemetry.mdx)).
- `/reportbug` "collects diagnostic information… It gathers relevant context like your configuration, recent errors,
  and system details" ([using commands](https://docs.cline.bot/core-workflows/using-commands)).

## 1.19 Evaluation & regression tests — **Cline has a real, documented eval harness**

This is a strong contrast with Continue (§2.18). Cline open-sourced both the harness and the results.

- **In-repo harness: `evals/`.** Structure: `smoke-tests/` (quick provider validation, `run-smoke-tests.ts`, 5 curated
  scenarios), `e2e/` (`run-cline-bench.ts`), **`cline-bench/` (a git submodule → <https://github.com/cline/cline-bench>
  with 12 production bug fixes)**, `analysis/` (`metrics.ts` for pass@k/pass^k, `classifier.ts` for failure pattern
  matching, `reporters/` for Markdown/JSON, `patterns/cline-failures.yaml`), and `baselines/`
  ([evals/README.md](https://raw.githubusercontent.com/cline/cline/main/evals/README.md),
  [contents](https://api.github.com/repos/cline/cline/contents/evals)).
- **Three eval layers:**
  - **Layer 1 — contract tests** (unit, `src/core/api/transform/__tests__/`, **no LLM**)
  - **Layer 2 — smoke tests** (minutes; real LLM calls; **5 scenarios × 3 trials for pass@k**; "Runs the `cline` CLI
    with `--config`, `-y`, `-t`, and `-m`")
  - **Layer 3 — E2E** (hours; 12 real-world problems; Docker/Daytona execution via **Harbor**; nightly CI intended)

  Source: [evals/README.md](https://raw.githubusercontent.com/cline/cline/main/evals/README.md)
- Commands: `npm run eval:smoke:run` (optional `-- --scenario 01-create-file`,
  `-- --model anthropic/claude-sonnet-4.5`), `npm run eval:e2e` (optional `-- --tasks discord`,
  `-- --provider openai --model gpt-4o`); requires `CLINE_API_KEY` for the Cline provider
  ([evals/README.md](https://raw.githubusercontent.com/cline/cline/main/evals/README.md)).
- **Metrics defined:** **pass@k** = P(≥1 of k passes) (solution finding); **pass^k** = P(all k pass) (reliability);
  **Flakiness** = entropy of pass rate. With 3 trials: all pass → `pass`, all fail → `fail`, mixed → **`flaky`**
  ([evals/README.md](https://raw.githubusercontent.com/cline/cline/main/evals/README.md)).
- **Honest current status:** the repo README states smoke tests (Layer 2) are **partially disabled** while the
  framework is repointed at the new SDK CLI; the old build-and-link helpers and the auto-running
  `cline-evals-regression.yml` workflow are off; "**Current PR gate: contract tests only**"; nightly E2E with
  cline-bench is **not yet implemented** (open TODO)
  ([evals/README.md](https://raw.githubusercontent.com/cline/cline/main/evals/README.md)).
- **Official benchmark results on Terminal Bench:** Cline ran Terminal Bench's **89 real-world coding tasks** and
  improved from **47% to 57%**, which the post says put Cline "ahead of Claude Code, OpenHands, and OpenCode" and 5%
  above Claude Code ([a practical guide to hill climbing](https://cline.ghost.io/a-practical-guide-to-hill-climbing/)).
  - **No SWE-bench Pro results were found on any primary Cline source.** The blog itself notes most coding-agent evals
    are "either single-turn or too saturated", linking OpenAI's post on why they no longer evaluate SWE-bench Verified
    ([same post](https://cline.ghost.io/a-practical-guide-to-hill-climbing/)).
    **Treat any SWE-bench Pro claim about Cline as unverified.**
- **Eval infrastructure:** **Harbor** (<https://github.com/laude-institute/harbor>, by the Terminal-Bench creators) for
  sandbox management/agent loop/rollout monitoring, and **Modal** for parallel cloud runs (89 containers); OpenRouter
  recommended as the most reliable provider for evals; a full run takes **~35–50 minutes with Modal vs many hours
  sequentially**; `pip install harbor` / `uv tool install harbor`
  ([a practical guide to hill climbing](https://cline.ghost.io/a-practical-guide-to-hill-climbing/)).
- Concrete knobs: `-d terminal-bench@2.0`, `-a cline-cli`, `-m openrouter:<model>`, `--env modal`, `-n 89 -l 89`,
  `--override-cpus`, `--override-memory-mb`, `--ak thinking=6000`, `--ak timeout=2400`, `--ak github_user=cline`,
  **`--ak commit_hash=<branch>`** ("lets you A/B a PR branch without merging")
  ([same post](https://cline.ghost.io/a-practical-guide-to-hill-climbing/)).
- **Failure-driven fixes the post attributes to eval results:** CLI default 600s timeout too short (→ `--ak
  timeout=2400`); "Missing expected files — Cline assumed success without verifying" (→ PR #9154, require verification
  before completion); "Command exit codes not surfaced" (→ PR #9156); "Long-running commands cut off" (→ PR #9159)
  ([same post](https://cline.ghost.io/a-practical-guide-to-hill-climbing/)).
- **The "Hill Climber's Checklist" — five documented eval heuristics:** get a North Star metric; quantify the noise;
  break down the failure mode (by **task, then model, then provider**); remember more thinking is not automatically
  better; keep a private set ([open-sourcing evals](https://cline.ghost.io/open-sourcing-evals-for-open-weight-agents/)).
- **Reported variance is unusually large, and Cline reports it honestly** (Terminal Bench, 89 tasks, same build):
  minimax-m3 43.8%→56.2%; deepseek-v4-pro 44.9% vs 53.9%; glm-5.1 46.1/47.2/49.4%;
  deepseek-v4-flash 38.2/44.9/48.3%; glm-5.2 56.2%→74.2%
  ([open-sourcing evals](https://cline.ghost.io/open-sourcing-evals-for-open-weight-agents/)).
- **Provider-route variance:** the same GLM-5.2 build scored **CoreWeave 66/89 (74.2%)** vs **OpenRouter general
  routing 55/89 (61.8%)** at medium reasoning — "the route alone was worth 11 tasks" — plus "**up to a 2x difference in
  net tokens and cache hit rates** between providers, which can turn a $20 run into a $43 run for the same eval"
  ([same post](https://cline.ghost.io/open-sourcing-evals-for-open-weight-agents/)).
- **Cross-model regression is real:** one improvement had opposite signs on different models (see §1.9), so Cline
  "pursued option B", using **prompt families for different models**
  ([same post](https://cline.ghost.io/open-sourcing-evals-for-open-weight-agents/)).
- OSS artifact: "dozens of eval run traces" published at <https://cline-open-eval-ledger.netlify.app/>
  ([same post](https://cline.ghost.io/open-sourcing-evals-for-open-weight-agents/)).
- Cline also runs a harness migration at scale: "How We Migrated 11 Million Users to Cline's Biggest Harness Upgrade"
  — migrating the VS Code extension to the new Cline SDK with a safe A/B rollout, claiming "**reduced agent failures by
  10x**" ([blog](https://cline.ghost.io/how-we-migrated-11-million-users-to-clines-biggest-harness-upgrade/)).

## 1.20 Configuration layout (high-value reference)

Global vs project configuration, verbatim from the official config page
([config](https://docs.cline.bot/getting-started/config)):

```
~/.cline/
  data/
    settings/
      providers.json           # API keys and provider configuration
      global-settings.json     # Global settings
      cline_mcp_settings.json  # MCP settings
    teams/                     # Team state
    sessions/                  # Session data
    db/                        # SQLite databases (for example cron.db)
    workflows/                 # Global workflows
  rules/                       # Global rules
  hooks/                       # Global hooks
  skills/                      # Global skills
  agents/                      # Global agent definitions
  plugins/                     # Global plugins (.js, .ts)
  cron/                        # Global cron specs
```

```
.cline/
  rules/                       # Project rules
  skills/                      # Project skills
  hooks/                       # Lifecycle hooks
  agents/                      # Project agent definitions
  plugins/                     # Project plugins
  cron/                        # Workspace cron specs
```

Additional compatibility search path: `~/Documents/Cline/{Rules,Hooks,Plugins,Workflows}`.

- Scope rule: "Use **global (`~/.cline/`)** for defaults shared across all Cline applications (IDE, CLI, SDK) on your
  machine. Use **project (`.cline/`)** for team-shared behavior that should travel with the repo."
  ([config](https://docs.cline.bot/getting-started/config)).
- Environment variables: **`CLINE_DATA_DIR`**, `CLINE_HUB_ADDRESS` (default `127.0.0.1:25463`),
  `CLINE_SESSION_BACKEND_MODE` (`local|hub|remote|auto`), `CLINE_SANDBOX_DATA_DIR`, `CLINE_SANDBOX`, `CLINE_HOOKS_DIR`,
  `CLINE_BUILD_ENV`, `CLINE_DEBUG_PORT_BASE`, `CLINE_COMMAND_PERMISSIONS`, `CLINE_TEAM_DATA_DIR`, `CLINE_DEBUG_HOST`
  (default `127.0.0.1`), `CLINE_TOOL_APPROVAL_MODE`, `CLINE_TOOL_APPROVAL_DIR`, `CLINE_LOG_ENABLED`, `CLINE_LOG_LEVEL`,
  `CLINE_LOG_PATH`, `CLINE_LOG_NAME`, `CLINE_DEBUG`
  ([cli-reference.mdx](https://raw.githubusercontent.com/cline/cline/main/docs/cli/cli-reference.mdx),
  [apps/cli/README.md](https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md)).

## 1.21 Deprecation ledger

From the official deprecations page ([deprecations](https://docs.cline.bot/resources/deprecations)):

| Feature | Primary surface | Status | Recommended alternative |
|---|---|---|---|
| `.clineignore` | VS Code extension | Deprecating soon | Block Ignored File Access plugin |
| Explain Changes | VS Code extension | Deprecated | Ask Cline for an explanation |
| **Focus Chain** | VS Code extension | **Deprecated** | **No direct replacement** |

Rationale for the SDK-backed move: "Explain Changes is being deprecated as part of Cline's move to the SDK backed
architecture. Rather than rebuilding the previous inline explanation experience, we are focusing on the current code
review workflow" ([deprecations](https://docs.cline.bot/resources/deprecations)).

**Docs-site redirects reveal further removed/renamed surfaces** (all from
[docs.json](https://raw.githubusercontent.com/cline/cline/main/docs/docs.json)): `/mcp/mcp-marketplace` →
`/mcp/mcp-overview`; `/customization/workflows` → `/customization/cline-rules`; `/cli/configuration` →
`/getting-started/config`; `/cline-cli/*` → new locations; all `/features/hooks/*` → `/customization/hooks` (a stub);
all `/features/at-mentions/*` → `/core-workflows/working-with-files`; `/features/focus-chain` →
`#deep-planning`.

---

# Part 2 — Continue

## 2.1 Project status (read this first)

- **The repository is frozen.** Verbatim: "**Note: The `continuedev/continue` repository is no longer actively
  maintained and is read-only for all users.**"
  ([README](https://raw.githubusercontent.com/continuedev/continue/main/README.md)).
- "**Final 2.0.0 Release** — We polished Continue and did a final 2.0.0 release of the VS Code extension, CLI, and
  JetBrains plugin. This included **removing anonymous telemetry, pulling out authentication**, squashing bugs, and
  more." ([README](https://raw.githubusercontent.com/continuedev/continue/main/README.md)).
- License **Apache 2.0** (© 2023-2026 Continue Dev, Inc.); the CLI package declares `"license": "Apache-2.0"`
  ([README](https://raw.githubusercontent.com/continuedev/continue/main/README.md),
  [extensions/cli/package.json](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/package.json)).
- Surfaces: **CLI**, **VS Code extension** (also OpenVSX), **JetBrains plugin**. The README explicitly recommends
  "using the Continue CLI instead of the JetBrains plugin"
  ([README](https://raw.githubusercontent.com/continuedev/continue/main/README.md)).
- Contributions require a **CLA** (comment "I have read the CLA Document and I hereby sign the CLA" on the PR)
  ([CONTRIBUTING.md](https://raw.githubusercontent.com/continuedev/continue/main/CONTRIBUTING.md)).
- Release model: single permanent `main` branch; tags like `v1.3.x-vscode` trigger preview then main release workflows;
  latest tags observed `v2.1.0-vscode`, `v2.0.0-vscode`
  ([CONTRIBUTING.md](https://raw.githubusercontent.com/continuedev/continue/main/CONTRIBUTING.md),
  [tags](https://api.github.com/repos/continuedev/continue/tags)).
- **Practical implication:** treat the docs as describing a fixed artifact. Several docs pages are stale relative to the
  2.0.0 shipped build (see §2.17 telemetry).

## 2.2 Feature surface & modes

- Officially listed features: **Agent mode**, **Chat mode**, **Edit mode**, **Autocomplete**, and **Continue CLI
  (`cn`)** ([docs home](https://docs.continue.dev/)).
- Modes are selected in a dropdown below the chat input; **`Cmd/Ctrl + .`** cycles modes
  ([agent quick start](https://docs.continue.dev/ide-extensions/agent/quick-start)).
- Mode semantics, stated plainly: "**Chat mode**: No tools available, pure conversation. **Plan mode**: Read-only tools
  for safe exploration and planning. **Agent mode**: All tools available for making changes."
  ([agent quick start](https://docs.continue.dev/ide-extensions/agent/quick-start)).
- Agent/Plan mode require tool support: "If Agent mode or Plan mode is disabled with a `Not Supported` message, the
  selected model or provider doesn't support tools"
  ([agent quick start](https://docs.continue.dev/ide-extensions/agent/quick-start)).

## 2.3 Agent loop & tool-calling protocol

**The explicit six-step tool handshake** — the clearest statement of the loop in either project
([how agent mode works](https://docs.continue.dev/ide-extensions/agent/how-it-works)):

1. In Agent mode, available tools are sent along with `user` chat requests
2. The model can choose to include a tool call in its response
3. **The user gives permission. This step is skipped if the policy for that tool is set to `Automatic`**
4. Continue calls the tool using built-in functionality or the MCP server that offers that particular tool
5. Continue sends the result back to the model
6. The model responds, potentially with another tool call and step 2 begins again

- Tool definition format: "provided to the model as a JSON object with a **name and an arguments schema**." Example
  given: a `read_file` tool with a `filepath` argument
  ([how agent mode works](https://docs.continue.dev/ide-extensions/agent/how-it-works)).
- Tool availability by mode: Chat → no tools; Plan → only read-only tools; Agent → all tools
  ([how agent mode works](https://docs.continue.dev/ide-extensions/agent/how-it-works)).
- Tool results auto-feed back: "Any data returned from a tool call is automatically fed back into the model as a
  context item. **Most errors are also caught and returned**, so that Agent mode can decide how to proceed."
  ([agent quick start](https://docs.continue.dev/ide-extensions/agent/quick-start)).
- Agent-mode context is automatic: "Tool call responses are automatically included as context items. This enables Agent
  mode to see the result of the previous action and decide what to do next."
  ([context selection](https://docs.continue.dev/ide-extensions/agent/context-selection)).
- **Fallback for models without native tool support:** "Continue uses **system message tools** as a fallback for models
  without native tool support, so most models should work with Agent mode automatically."
  ([FAQs](https://docs.continue.dev/faqs)).
- **Parallel tool calls: YES — supported, but only for read-only tools.** The agent system message states verbatim:
  "If you need to use multiple tools, you can call **multiple read-only tools simultaneously**."
  ([core/llm/defaultSystemMessages.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/llm/defaultSystemMessages.ts)).
  The handshake prose is written serially, so the docs alone are misleading here — the capability is stated in the
  system prompt, not the docs.
- **Stop conditions: unverified** as an explicit enum. Implied by step 6's conditional (model responds without a further
  tool call).
- Concurrency primitive that does exist: **background jobs** — `/jobs` "List background jobs"
  ([TUI mode](https://docs.continue.dev/cli/tui-mode)) and a `CheckBackgroundJob` tool among the default-allowed
  read-only tools ([tool permissions](https://docs.continue.dev/cli/tool-permissions)).

## 2.4 Tool set & edit strategy

**Plan mode (read-only) tools** ([how agent mode works](https://docs.continue.dev/ide-extensions/agent/how-it-works)):

- `read_file` — Read file
- `read_currently_open_file` — Read currently open file
- `ls` — List directory
- `glob_search` — Glob search
- `grep_search` — Grep search
- `fetch_url_content` — Fetch URL content
- `search_web` — Search web
- `view_diff` — View diff
- `view_repo_map` — View repo map
- `view_subdirectory` — View subdirectory
- `codebase_tool` — Codebase tool

**Agent mode adds:**

- `create_new_file` — Create a new file within the project
- `edit_existing_file` — Make changes to existing files
- `run_terminal_command` — Run commands from the workspace root
- `create_rule_block` — Create a new rule block in `.continue/rules`
- "All other write/execute tools for modifying the codebase" (unspecified)

Source: [how agent mode works](https://docs.continue.dev/ide-extensions/agent/how-it-works)

- Notable: **`view_repo_map` and `codebase_tool` are first-class read-only tools** — repo structure is exposed as a
  tool, not only as context ([how agent mode works](https://docs.continue.dev/ide-extensions/agent/how-it-works)).
- **Doc conflict — tool naming convention differs between surfaces.** The IDE docs use snake_case
  (`edit_existing_file`, `read_file`), while the CLI permissions docs use **PascalCase**: `Read`, `List`, `Search`,
  `Fetch`, `Diff`, `AskQuestion`, `Checklist`, `Status`, `CheckBackgroundJob`, `ReportFailure`, `UploadArtifact`,
  `Edit`, `MultiEdit`, `Write`, `Bash`
  ([how agent mode works](https://docs.continue.dev/ide-extensions/agent/how-it-works),
  [tool permissions](https://docs.continue.dev/cli/tool-permissions)). `MultiEdit` and `Checklist` have no IDE
  counterpart. The CLI example permissions file confirms the names are **normalized**:
  `allow: - Read # or read_file, read, READ - all normalized`; `ask: - Write # or write_file; Terminal # or
  run_terminal_command, Bash, bash`
  ([example-permissions.yaml @4bc0a84](https://raw.githubusercontent.com/continuedev/continue/4bc0a84453a95c7d582076305907e56b28ef7b8c/example-permissions.yaml)).
  **URL caveat:** that path returns **404 on `main`**; only the pinned commit URL works.

**Canonical tool names in source (and where the docs drift)**
([core/tools/builtIn.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/builtIn.ts),
[core/tools/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/index.ts)):

- `BuiltInToolNames` enum: `read_file`, `read_file_range`, `edit_existing_file`, `single_find_and_replace`, `multi_edit`,
  `read_currently_open_file`, `create_new_file`, `run_terminal_command`, `grep_search`, **`file_glob_search`**,
  `search_web`, `view_diff`, `ls`, `create_rule_block`, `request_rule`, `fetch_url_content`, **`codebase`**,
  `read_skill` — plus `view_repo_map` and `view_subdirectory`, which are commented **"excluded from allTools for now"**.
- **Naming discrepancies vs the docs:** the docs call the glob tool `glob_search` but source uses **`file_glob_search`**;
  the docs call the codebase tool `codebase_tool` but source uses **`codebase`**
  ([docs tool list](https://docs.continue.dev/ide-extensions/agent/how-it-works)).
- Base (always-on) set: `readFileTool`, `createNewFileTool`, `runTerminalCommandTool`, `globSearchTool`, `viewDiffTool`,
  `readCurrentlyOpenFileTool`, `lsTool`, `createRuleBlock`, `fetchUrlContentTool`.
- Config-dependent set: `requestRuleTool`, `readSkillTool`, `searchWebTool`; plus `viewRepoMapTool`,
  `viewSubdirectoryTool`, `codebaseTool`, `readFileRangeTool` **only when `enableExperimentalTools`**; plus
  `multiEditTool` **or** (`editFileTool` + `singleFindAndReplaceTool`) depending on
  **`isRecommendedAgentModel(modelName)`** — i.e. the edit strategy *changes with the model*; and `grepSearchTool` only
  when `!isRemote`.
- **`CLIENT_TOOLS_IMPLS` = `[edit_existing_file, single_find_and_replace, multi_edit]`** — those three edit tools are
  executed **client-side**, not in core
  ([builtIn.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/builtIn.ts),
  [callTool.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/callTool.ts)).
- `BUILT_IN_GROUP_NAME = "Built-In"` — tools are grouped and can be toggled **by group** in the UI.
- Tools carry UI metadata: `type: "function"`, `displayTitle`, `wouldLikeTo`/`isCurrently`/`hasAlready`
  (Mustache-templated, e.g. `"read {{{ filepath }}}"`), `readonly`, `isInstant`, `group`, `toolCallIcon`
  ([readFile.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/definitions/readFile.ts)).
- `run_terminal_command` args are `command` (required) and **`waitForCompletion`** (boolean; "Default is true. Set to
  false to run the command in the background"). Its instructions tell the model the shell is **stateless**, never to
  suggest Ctrl+C for background jobs, that it must not require admin privileges, and to "use Edit/MultiEdit tools
  instead of bash commands (sed, awk, etc)"
  ([runTerminalCommand.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/definitions/runTerminalCommand.ts)).
- **MCP tool identity:** tools are addressed by URI
  `mcp://<encodeURIComponent(mcpId)>/<encodeURIComponent(toolName)>`; args are coerced to the tool's JSON schema via
  `coerceArgsToSchema`; a call timeout comes from `client.options.timeout`; `isError === true` throws with the
  serialized content, and non-text/non-resource content produces an "MCP Item Error" context item
  ([core/tools/callTool.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/callTool.ts)).
- Edit strategy is **whole-file/rewrite-oriented at the tool level** (`create_new_file`, `edit_existing_file`), with a
  dedicated **Apply model role** for executing targeted modifications accurately
  ([models](https://docs.continue.dev/customize/models)).
- **Diff surfacing:** `view_diff` is a first-class read-only tool; the CLI adds remote-only `/diff` and `/apply` slash
  commands ([how agent mode works](https://docs.continue.dev/ide-extensions/agent/how-it-works),
  [TUI mode](https://docs.continue.dev/cli/tui-mode)).
- **No browser tool** is documented; web access is `fetch_url_content` + `search_web`
  ([how agent mode works](https://docs.continue.dev/ide-extensions/agent/how-it-works)).
- **CHECKPOINTS / UNDO: Continue has no equivalent.** There is **no shadow-git snapshot system, no checkpoint timeline,
  and no restore-to-N UI** anywhere in the Continue docs or source. The closest primitives are:
  - **Edit (inline, per-hunk)**: highlight code and press `Cmd/Ctrl + I` for an inline streaming diff with
    **accept/reject per hunk** — "recommended for small, targeted changes". This is a per-edit review gate, not a
    session-level undo ([Edit quick start](https://docs.continue.dev/ide-extensions/edit/quick-start)).
  - `view_diff` (read-only) and the CLI's remote-only `/diff` + `/apply`
    ([how agent mode works](https://docs.continue.dev/ide-extensions/agent/how-it-works),
    [TUI mode](https://docs.continue.dev/cli/tui-mode)).
  - `~/.continue/.diffs` exists as a persisted path in `paths.ts` (name implies stored diffs), but its role is
    **unverified** ([core/util/paths.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/util/paths.ts)).
  - **Implication:** Continue relies on your own git for rollback. Cline's three-way restore
    (files / task / both) is a genuine capability gap between the two.

## 2.5 Context management

**The context-provider system is largely deprecated and replaced by tools + rules + MCP.** The docs nav explicitly
labels three reference pages "Deprecated": Context Providers, `@Codebase`, `@Docs` ([docs home](https://docs.continue.dev/)).

- Migration guidance, verbatim: "The `@Codebase` context provider has been deprecated. Instead: 1. **Use built-in
  tools**: Agent mode can now use file exploration and search tools to understand your codebase. 2. **Add rules**:
  Create `.continue/rules` files to provide context about your project structure. 3. **Use MCP servers**: For external
  codebases, use DeepWiki MCP or custom MCP servers."
  ([codebase awareness](https://docs.continue.dev/guides/codebase-documentation-awareness)).
- "The `@Docs` context provider has been deprecated. Instead: 1. **Use Context7 MCP**… 2. **Add documentation links in
  rules**… 3. **Use custom MCP servers**"
  ([codebase awareness](https://docs.continue.dev/guides/codebase-documentation-awareness)).
- Stated rationale: "The new approach provides better integration with Continue's Agent mode features and more
  intelligent context selection"
  ([codebase awareness](https://docs.continue.dev/guides/codebase-documentation-awareness)).
- Replacement rule stated elsewhere: "To provide context beyond the built-in context providers, we now recommend using
  **MCP Servers**." ([custom providers](https://docs.continue.dev/customize/deep-dives/custom-providers)).

**What still exists**

- `config.yaml` keeps a `context` array: each entry has `provider` (**required**), `name`, `params`. Documented
  built-in providers include `file`, `code`, `diff`, `currentFile`, `terminal`, `open` (`params: { onlyPinned: true }`),
  `clipboard`, `tree`, `problems`, `debugger` (`params: { stackDepth: 3 }`, VS Code only), `repo-map`
  (`params: { includeSignatures: false }`, default `true`), `os`, and `http`
  (`params: { url, headers }`, where the server receives `POST { query, fullInput }` and must return a `ContextItem` or
  array of them, `{name, description, content}`)
  ([custom providers](https://docs.continue.dev/customize/deep-dives/custom-providers),
  [reference](https://docs.continue.dev/reference)).
- `@Repository Map` is documented as "inspired by Aider's repository map", and "Signatures will not be included if
  indexing is disabled" ([custom providers](https://docs.continue.dev/customize/deep-dives/custom-providers)).
- **CLI `@` means file/directory references**, not the IDE provider dropdown:
  `@src/auth/middleware.ts`, `@tests/`, `@package.json @tsconfig.json`
  ([TUI mode](https://docs.continue.dev/cli/tui-mode)).
- **Compaction:** `/compact` — "Summarize chat history into a compact form"
  ([TUI mode](https://docs.continue.dev/cli/tui-mode)). Source file `src/compaction.ts` exists with tests
  `compaction.infiniteLoop.test.ts` and `compaction.pruneLastMessage.test.ts`
  ([src listing](https://api.github.com/repos/continuedev/continue/contents/extensions/cli/src)).
- **Per-mode system message overrides** are the documented lever for shaping behaviour:
  `chatOptions.baseSystemMessage` (Chat), `chatOptions.baseAgentSystemMessage` (Agent),
  `chatOptions.basePlanSystemMessage` (Plan) ([reference](https://docs.continue.dev/reference),
  [customize agent](https://docs.continue.dev/ide-extensions/agent/how-to-customize)).
- Token accounting in-session: `/info` — "Show session information, including **token usage and cost**"
  ([TUI mode](https://docs.continue.dev/cli/tui-mode)).

**Ignore rules**

- Continue "respects `.gitignore` files in order to determine which files should not be indexed. If you'd like to
  exclude additional files, you can add them to a **`.continueignore`** file, which follows the exact same rules as
  `.gitignore`." A **global** `~/.continue/.continueignore` is also supported, "respected for all workspaces"
  ([deprecated @Codebase](https://docs.continue.dev/reference/deprecated-codebase)).
- Source confirmation: `getWorkspaceContinueIgArray` reads `${workspaceDir}/.continueignore` per workspace dir;
  `getGlobalContinueIgnorePath()` = `path.join(getContinueGlobalPath(), ".continueignore")` and **creates an empty file
  if missing**. Parsing (`gitIgArrayFromFile`) splits on `\r?\n`, trims, and drops lines matching `/^#|^$/`
  ([core/indexing/continueignore.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/indexing/continueignore.ts),
  [core/util/paths.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/util/paths.ts),
  [core/indexing/ignore.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/indexing/ignore.ts)).
- **Hard-coded default ignores go far beyond `.gitignore`, with a security-first posture.** Always excluded file types
  include `*.env`, `*.env.*`, `.env*`, `config.json`, `config.yaml`, `config.yml`, `settings.json`,
  `appsettings*.json`, `*.key`, `*.pem`, `*.p12`, `*.pfx`, `*.crt`, `*.cer`, `*.jks`, `*.keystore`, `*.truststore`,
  `*.db`, `*.sqlite*`, `*.mdb`, `*.accdb`, `*.secret(s)`, `auth.json`, `*.token`, `*.bak`, `*.backup`, `*.old`,
  `*.orig`, `docker-compose.override.y(a)ml`, `id_rsa`, `id_dsa`, `id_ecdsa`, `id_ed25519`, `*.ppk`, `*.gpg`.
  Security dirs: `.env/`, `env/`, `.aws/`, `.gcp/`, `.azure/`, `.kube/`, `.docker/`, `secrets/`, `.secrets/`,
  `private/`, `.private/`, `certs/`, `certificates/`, `keys/`, `.ssh/`, `.gnupg/`, `.gpg/`, `tmp/secrets/`,
  `temp/secrets/`, `.tmp/`. Violations raise `ContinueErrorReason.FileIsSecurityConcern` ("Reading or Editing <file>
  is not allowed because it is a security concern"). General ignores include `*-lock.json`, `*.lock`, `*.log`,
  images, archives, binaries, `go.sum`, `*.csv`, `*.jsonl`, `.continue/`, and dirs `.git/`, `.svn/`, `node_modules/`,
  `dist/`, `build/`, `target/`, `out/`, `bin/`, `.venv/`, `venv/`, `.vscode/`, `.idea/`, `.vs/`
  ([core/indexing/ignore.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/indexing/ignore.ts)).
  **This is the strongest safety-by-default posture found in either project.**
- `~/.continue/.continuerc.json` is auto-created with `{ "disableIndexing": true }` — source comment: "Disable indexing
  of the config folder to prevent infinite loops"
  ([core/util/paths.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/util/paths.ts)).
- **Caveat:** `.continueignore` is documented only on the *deprecated* `@Codebase` page. Whether it still applies now
  that `@Codebase` is deprecated is **unverified** — the replacement approach uses agent tools governed by tool
  policies rather than an ignore file.

## 2.6 Indexing & retrieval via embeddings

**In-repo spec (the authoritative description)** — [core/indexing/README.md](https://raw.githubusercontent.com/continuedev/continue/main/core/indexing/README.md):

- Continue uses "a tagging system along with content addressing to ensure that nothing needs to be indexed twice.
  When you change branches, Continue will only re-index the files that are newly modified and that we don't already
  have a copy of." Concepts: *artifact* (embeddings / full-text index / code-snippet table), *cacheKey* ("always hash of
  file contents at this point"), *`CodebaseIndex`*.
- Five-step flow: (1) check modified timestamps of all repo files; (2) diff against a **SQLite catalog** to produce
  add/remove lists; (3) for "add", reuse a SQLite cache entry when the `cacheKey` matches on another branch ("addTag")
  else "compute"; (4) for "remove", "delete" if only one tag remains else "removeTag"; (5) pass the four lists
  (`compute`, `delete`, `addTag`, `removeTag`) to each `CodebaseIndex.update`, which yields progress used to officially
  mark a file indexed so progress isn't falsely recorded if the extension closes mid-index.
- **Four indexes** (all must be returned by `getIndexesToBuild` in `CodebaseIndexer.ts`):
  - `CodeSnippetsCodebaseIndex` — tree-sitter queries for functions/classes/top-level objects
  - `FullTextSearchCodebaseIndex` — SQLite **FTS5**
  - `ChunkCodebaseIndex` — recursive structural chunking, feeding embeddings
  - `LanceDbIndex` — embeddings per chunk into **LanceDB**, metadata into SQLite; "for each branch, a unique table is
    created in LanceDB"
- **Known problem stated in the source:** "`FullTextSearchCodebaseIndex` doesn't differentiate between tags (branch,
  repo), so results may come from any branch/repo."

**Local vs remote**

- "By default, all embeddings are calculated **locally using `transformers.js`** and stored locally in
  `~/.continue/index`" ([deprecated @Codebase](https://docs.continue.dev/reference/deprecated-codebase)).
- Built-in embedder detail: "`transformers.js` is used as a built-in embeddings model in VS Code"; "The model used is
  **`all-MiniLM-L6-v2`**, which is shipped alongside the Continue extension"; "In JetBrains, there currently is no
  built-in embedder" ([model roles: embeddings](https://docs.continue.dev/customize/model-roles/embeddings)).
- On-disk paths (source): `~/.continue/index/index.sqlite` (`getIndexSqlitePath`),
  `~/.continue/index/lancedb` (`getLanceDbPath`), `~/.continue/index/docs.sqlite` (`getDocsSqlitePath`),
  `~/.continue/index/autocompleteCache.sqlite`, `~/.continue/index/globalContext.json`
  ([core/util/paths.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/util/paths.ts)).
- Inspect what was indexed: metadata in `~/.continue/index/index.sqlite`, table **`tag_catalog`** (view with DB Browser
  for SQLite) ([deprecated @Codebase](https://docs.continue.dev/reference/deprecated-codebase)).

**Retrieval tuning (legacy, deprecated)**

```yaml
context:
  - provider: codebase
    params:
      nRetrieve: 25   # initial results from vector DB (default: 25)
      nFinal: 5       # final results after re-ranking (default: 5)
      useReranking: true  # LLM re-ranks nRetrieve -> nFinal (default: true)
```

Source: [deprecated @Codebase](https://docs.continue.dev/reference/deprecated-codebase)

**Embeddings & reranking are model roles, not top-level keys**

- Per the migration guide: "`embeddingsProvider` in config should have `roles: [embed]`" and "`reranker` in config
  should have `roles: [rerank]`"
  ([yaml migration](https://docs.continue.dev/reference/yaml-migration)).
  **`embeddingsProvider` and `reranker` are `config.json`-only legacy keys**; there is no top-level key for them in
  `config.yaml`.
- `embed` models take `embedOptions: { maxChunkSize, maxBatchSize }` — "Minimum is 128 tokens" per chunk, "Minimum is
  1 chunk" per batch ([reference](https://docs.continue.dev/reference)).
- Current YAML embeddings form (Voyage example): `provider: voyage`, `model: voyage-code-3`, `apiKey`, `roles: [embed]`.
  Recommended: `voyage-code-3`, or `nomic-embed-text` via Ollama for local embeddings
  ([model roles: embeddings](https://docs.continue.dev/customize/model-roles/embeddings)).
- Supported embedding providers listed: Voyage AI, Ollama, `transformers.js` (VS Code only), HuggingFace TEI
  (`apiBase: http://localhost:8080`), OpenAI, Cohere, Gemini, Vertex, Mistral, NVIDIA, Bedrock, WatsonX, LMStudio
  ([model roles: embeddings](https://docs.continue.dev/customize/model-roles/embeddings)).
- `docs` indexing config for `@Docs` used `name` (required), `startUrl` (required), `favicon`, `useLocalCrawling`
  ("Skip the default crawler and only crawl using a local crawler")
  ([reference](https://docs.continue.dev/reference)).

**Re-indexing & troubleshooting**

- Full rebuild: **`Continue: Rebuild codebase index`** — "If you are having persistent errors with indexing, our
  recommendation is to rebuild your index from scratch" ([FAQs](https://docs.continue.dev/faqs)).
- **Linux x64 blocker:** "Codebase indexing disabled - Your Linux system lacks required CPU features (AVX2, FMA)".
  "We use **LanceDB** as our vector database… On x64 Linux systems, LanceDB requires specific CPU features (FMA and
  AVX2) which may not be available on older processors." Autocomplete/chat still work; `@codebase`, `@files`, `@folder`
  are disabled ([FAQs](https://docs.continue.dev/faqs)).
- **Custom RAG path (official guide):** use `voyage-code-3` (16,000-token max context; 1024 dims), LanceDB as vector
  DB, three chunking strategies (truncate / fixed-length / AST-based, with `core/indexing/chunk/code.ts` given as the
  official reference chunker), a separate table per repo, and expose it to Continue **as an MCP server**
  (`mcpServers: - name: custom-rag, command: python, args: [...], env: { VOYAGE_API_KEY: ... }`). Optional reranking
  with Voyage `rerank-2` (retrieve ~50, rerank to 10)
  ([custom code RAG](https://docs.continue.dev/guides/custom-code-rag)).
- **Status caveat:** the retrieval pipeline above is documented as deprecated; the current recommended answer is agent
  tools + rules + MCP. Whether the local index is still built and used by default in 2.0.0 is **unverified** (the FAQ's
  references to index rebuilding and LanceDB suggest the machinery remains present).

## 2.7 System prompt & project rule files

- Rules are "used to provide **system message instructions** to the model for Agent mode, Chat mode, and Edit mode
  requests"; they are **not** included in autocomplete or apply
  ([rules deep dive](https://docs.continue.dev/customize/deep-dives/rules)).
- **Primary location: `.continue/rules`** — "Create a folder called `.continue/rules` at the top level of your
  workspace", containing `.md` files. "Rules files are loaded in **lexicographical order**, so you can prefix them with
  numbers to control the order in which they are applied. For example: `01-general.md`, `02-frontend.md`,
  `03-backend.md`." ([rules deep dive](https://docs.continue.dev/customize/deep-dives/rules)).
- Officially documented gotcha: "**File location**: Ensure rules are in `.continue/rules/` (not `.continue/rule/`)"
  ([rules deep dive](https://docs.continue.dev/customize/deep-dives/rules)).
- **Rule frontmatter:** `name` (required for YAML), `globs`, `regex`, `description`, `alwaysApply`
  ([rules deep dive](https://docs.continue.dev/customize/deep-dives/rules)).
- `alwaysApply` semantics, stated precisely:
  - `true` → always included, regardless of file context
  - `false` → included if globs exist AND match file context, **or** the agent decides to pull the rule into context
    based on its `description`
  - `undefined` (default) → included if no globs exist, OR globs exist and match

  Source: [rules deep dive](https://docs.continue.dev/customize/deep-dives/rules)
- `globs` and `regex` accept a single pattern or an array of patterns; example
  `globs: docs/**/*.{md,mdx}`, `regex: "^import .* from '.*';$"`
  ([rules deep dive](https://docs.continue.dev/customize/deep-dives/rules)).
- Both **Markdown-with-frontmatter and YAML** rule formats are supported: "we introduced Markdown for easier editing.
  While both are still supported, we recommend Markdown"
  ([rules deep dive](https://docs.continue.dev/customize/deep-dives/rules)).
- **System message assembly:** "To form the system message, rules are joined with new lines, in the order they appear in
  the toolbar. This includes the base chat system message"
  ([rules deep dive](https://docs.continue.dev/customize/deep-dives/rules)).
- The default system message is deliberately minimal and exists "to help the model provide reliable codeblock formats
  in its output", inspectable at `core/llm/constructMessages.ts`
  ([rules deep dive](https://docs.continue.dev/customize/deep-dives/rules),
  [constructMessages.ts](https://github.com/continuedev/continue/blob/main/core/llm/constructMessages.ts#L4)).
- Config-level alternative — the `rules` top-level array accepts inline strings or `uses:` / `file://` references:

  ```yaml
  rules:
    - uses: sanity/sanity-opinionated
    - uses: file://user/Desktop/rules.md
    - Give concise responses
  ```
  — [reference](https://docs.continue.dev/reference)
- Agent-created rules: "When in Agent mode, you can prompt the agent to create a rule for you using the
  **`create_rule_block`** tool if enabled… a rule will be created for you in `.continue/rules` based on your
  conversation" ([rules deep dive](https://docs.continue.dev/customize/deep-dives/rules)).

**AGENTS.md — SUPPORTED, confirmed in source, but absent from the docs site**

- **Source-verified:** `export const SUPPORTED_AGENT_FILES = ["AGENTS.md", "AGENT.md", "CLAUDE.md"];` — loaded from the
  **workspace root** of each workspace dir. An agent file's content is parsed into a rule with `source: "agentFile"`,
  `sourceFile: <uri>`, and **`alwaysApply: true` forced**
  ([core/config/markdown/loadMarkdownRules.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/config/markdown/loadMarkdownRules.ts)).
  - Implementation nuance: the filename loop contains a `break` at the end of the `try` block (outside the
    `if (exists)`), so in practice only the first entry (`AGENTS.md`) is ever probed — `AGENT.md` and `CLAUDE.md`
    appear effectively unreachable. *(Reading of the code, not documented behaviour.)*
- `AGENTS.md` is also recognised as a config-related file for watchers: `isContinueConfigRelatedUri()` matches
  `.continuerc.json`, `.prompt`, any `SUPPORTED_AGENT_FILES`, `.continuerules`, and `.continue/*.{yaml,yml,json}`
  ([core/config/loadLocalAssistants.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/config/loadLocalAssistants.ts)).
- The Continue **CLI TUI has an `/init` slash command** documented as "Create an `AGENTS.md` file for the current
  project" ([TUI mode](https://docs.continue.dev/cli/tui-mode)). So the CLI both writes and reads `AGENTS.md`.
- However the rules documentation does **not** list `AGENTS.md` among rule sources; it documents only `.continue/rules`
  and the config `rules` key ([rules](https://docs.continue.dev/customize/rules),
  [rules deep dive](https://docs.continue.dev/customize/deep-dives/rules)).
- An **open issue in the official repo** requests this: continuedev/continue#6716, "Add Support for Agent Rules
  Standard via Root AGENTS.md File for Unified AI Coding Guidelines"
  ([issue](https://github.com/continuedev/continue/issues/6716)) — the issue *body* could not be fetched (github.com
  HTML fetch failed); the title came from search results only.
- **Conclusion:** AGENTS.md read support **exists in the frozen source** (forced `alwaysApply: true`) even though the
  docs never mention it, and the CLI both writes and reads it. The accurate statement is **"implemented but
  undocumented"**, not "unsupported" — issue #6716 either predates or overlooks the implementation.

**Other rule sources verified in source (undocumented on the docs site)** — these materially widen Continue's rule
surface ([getWorkspaceContinueRuleDotFiles.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/config/getWorkspaceContinueRuleDotFiles.ts),
[core/llm/rules/constants.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/llm/rules/constants.ts),
[loadCodebaseRules.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/config/markdown/loadCodebaseRules.ts),
[loadMarkdownSkills.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/config/markdown/loadMarkdownSkills.ts)):

- **`.continuerules`** — a single dot-file at each workspace root, loaded as a rule with `source: ".continuerules"`
  (`SYSTEM_PROMPT_DOT_FILE = ".continuerules"`).
- **Colocated `rules.md`** — `RULES_MARKDOWN_FILENAME = "rules.md"`, discovered by walking the whole workspace; they get
  `source: "colocated-markdown"` and **directory-scoped globs**, auto-prefixed with the containing directory
  (`relativeDir + "**/" + glob`, or `relativeDir + "**/*"` if no globs). Non-root rules must match files inside their own
  directory — an elegant scoping rule that appears nowhere in the docs.
- **Skills** — `SKILL.md` files under `.continue/skills/**` and `.claude/skills/**` (workspace and global), with a strict
  frontmatter schema requiring `name` (min 1) and `description` (min 1); sibling files in each skill directory are
  collected as `files` and read via the **`read_skill`** tool.
- Both workspace and global rule dirs are scanned (`includeGlobal: true, includeWorkspace: true`).

**Negative rules, ordering, and rule policy**
([core/llm/rules/getSystemMessageWithRules.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/llm/rules/getSystemMessageWithRules.ts)):

- `globs`/`regex` support arrays and **negative patterns prefixed with `!`**; arrays require at least one positive match
  and no negative match (the docs omit negation entirely).
- Composition: `getSystemMessageWithRules` appends each applied rule to the base system message separated by `\n\n`, in
  array order.
- **"Global rule" determination in source:** `alwaysApply === true`, OR a root-level rule (no `sourceFile`, or path
  containing `.continue/`) with no `globs`/`regex` and `alwaysApply !== false`.
- Rules can be disabled by name via a per-rule policy string: `rulePolicies[rule.name] === "off"`.
- **Stale docs link:** the rules page links the base chat system message to `core/llm/constructMessages.ts#L4`, but
  **that path 404s on `main`**. The live file is `core/llm/defaultSystemMessages.ts`, exporting
  `DEFAULT_CHAT_SYSTEM_MESSAGE`, `DEFAULT_AGENT_SYSTEM_MESSAGE`, `DEFAULT_PLAN_SYSTEM_MESSAGE` and
  `CODEBLOCK_FORMATTING_INSTRUCTIONS`
  ([defaultSystemMessages.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/llm/defaultSystemMessages.ts)).

## 2.8 Permissions & safety

There are **two distinct permission systems**, one per surface, and notably **neither is configured in `config.yaml`**.

**(a) IDE extensions — three policies per tool, stored locally per user**

- **Ask First (default)**: "Request user permission with 'Cancel' and 'Continue' buttons"
- **Automatic**: "Automatically call the tool without requesting permission"
- **Excluded**: "Do not send the tool to the model"
- Managed via the tools icon in the input toolbar; you can "change policies by clicking on the policy text" and
  "toggle groups of tools on/off". "**Tool policies are stored locally per user.**"
- Warning given: "Be careful setting tools to 'automatic' if their behavior is not read-only."

Source: [customize agent mode](https://docs.continue.dev/ide-extensions/agent/how-to-customize)

**(b) CLI (`cn`) — a precedence-ordered policy system**

- Three levels: `allow` (runs automatically, no prompt), `ask` (prompts before running — **TUI only**), `exclude`
  (hidden from the agent entirely) ([tool permissions](https://docs.continue.dev/cli/tool-permissions)).
- **Defaults:** read-only tools → `allow` — `Read`, `List`, `Search`, `Fetch`, `Diff`, `AskQuestion`, `Checklist`,
  `Status`, `CheckBackgroundJob`, `ReportFailure`, `UploadArtifact`. Write tools (`Edit`, `MultiEdit`, `Write`) and
  `Bash` → **`ask`** ([tool permissions](https://docs.continue.dev/cli/tool-permissions)).
- `AskQuestion` is "a built-in read-only tool that lets the agent pause and ask for clarification before continuing"
  ([tool permissions](https://docs.continue.dev/cli/tool-permissions)).
- **Headless has no approver** — see the doc conflict below.
- Flags: `--allow`, `--ask`, `--exclude`; "Flags take precedence over all other permission sources"
  ([tool permissions](https://docs.continue.dev/cli/tool-permissions)).
- **Glob-argument matching** — a notable design detail:
  - `Write` — matches any call to the `Write` tool
  - `Write(*)` — same as above
  - `Write(**/*.ts)` — matches `Write` calls where the primary argument matches the glob
  - Examples: `cn --allow "Write(**/*.ts")`; `cn --allow Bash --exclude "Bash(npm install*)"`

  Source: [tool permissions](https://docs.continue.dev/cli/tool-permissions)
- **Persistent permissions file: `~/.continue/permissions.yaml`**, "updated when you choose 'Continue + don't ask
  again' in the TUI approval prompt":

  ```yaml
  allow:
    - Read(*)
    - Write(**/*.ts)
  ask:
    - Bash
  exclude: []
  ```
  "Changes take effect on the next session." ([tool permissions](https://docs.continue.dev/cli/tool-permissions))
- **Precedence (highest first):** (1) **Mode policies** — `--auto` and `--readonly` "override everything";
  (2) **CLI flags**; (3) **`permissions.yaml`**; (4) **defaults**
  ([tool permissions](https://docs.continue.dev/cli/tool-permissions)).
- **Modes as permission presets** — switchable with `Shift+Tab` during a TUI session or at launch:

  | Mode | Effect |
  |---|---|
  | **normal** (default) | Uses configured permissions |
  | **plan** (`--readonly`) | Excludes all write tools, allows reads and `Bash` |
  | **auto** (`--auto`) | Allows everything — `*: allow` |

  "Plan and auto modes are **absolute overrides**. They ignore `--allow`, `--exclude`, and `permissions.yaml`
  entirely." Source: [tool permissions](https://docs.continue.dev/cli/tool-permissions)
- Modes have UI indicators `[plan]` (blue) and `[auto]` (green); normal shows none
  ([extensions/cli/spec/modes.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/modes.md)).
- Note the deliberate asymmetry: **plan mode allows `Bash`** while excluding write tools
  ([tool permissions](https://docs.continue.dev/cli/tool-permissions)).
- TUI approval offers three choices: **Continue**, **Continue + don't ask again** (writes a policy rule to
  `~/.continue/permissions.yaml`), and **No** ("reject the call and give the agent new instructions")
  ([TUI mode](https://docs.continue.dev/cli/tui-mode)).

**Doc conflict — headless `ask` behaviour**

- Published docs: "In headless mode, `ask` tools are **excluded** since there's no one to approve them"
  ([tool permissions](https://docs.continue.dev/cli/tool-permissions)).
- In-repo CLI spec: `ask` tools "will cause the process to **exit with an error message**", showing
  `cn -p --allow "*"` / `cn -p --exclude run_terminal_command`
  ([extensions/cli/spec/permissions.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/permissions.md)).
- **Treat the spec as source-of-truth and the docs as aspirational.**
- Additionally, that same spec describes a `permissions:` block in `config.yaml` as "**implement later** … This should
  not be implemented yet", yet the published precedence list ranks config.yaml permissions above `permissions.yaml`
  ([extensions/cli/spec/permissions.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/permissions.md),
  [tool permissions](https://docs.continue.dev/cli/tool-permissions)). **Unresolved.**

**Plan mode (IDE) as the read-only safety mode**

- "Plan mode filters the available tools to only include read-only operations." You cannot "Create, edit, or delete
  files", "Run terminal commands", or "Make any system changes"
  ([plan mode](https://docs.continue.dev/ide-extensions/agent/plan-mode)).

**Doc conflict — MCP in Plan mode: RESOLVED by the source.** The two docs pages contradict each other, but the plan
system message settles it:

- Plan mode page: "**MCP support**: Works with all MCP tools alongside built-in read-only tools" and "Use all MCP tools"
  ([plan mode](https://docs.continue.dev/ide-extensions/agent/plan-mode)).
- MCP deep dive: "**MCP can only be used in the agent mode.**"
  ([MCP](https://docs.continue.dev/customize/deep-dives/mcp)).
- **Source resolution:** `DEFAULT_PLAN_SYSTEM_MESSAGE` says "Only use read-only tools. Do not use any tools that would
  write to non-temporary files," and carries the comment: *"The note about read-only tools is for MCP servers / For now,
  **all MCP tools are included** so model can decide if they are read-only."*
  ([core/llm/defaultSystemMessages.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/llm/defaultSystemMessages.ts)).
  **So MCP tools *are* available in Plan mode; enforcement is prompt-level instruction, not tool filtering.** The MCP
  deep dive's blanket claim is simply wrong.

**Continue DOES have dynamic, non-declarative safety policies — in the tool layer.** This corrects the docs-only
impression that safety is entirely structural:

- The policy type has exactly three values:
  `export type ToolPolicy = "allowedWithPermission" | "allowedWithoutPermission" | "disabled";`
  ([packages/terminal-security/src/types.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/terminal-security/src/types.ts)).
- Tools declare a `defaultToolPolicy`. Verified examples: `read_file` → `allowedWithoutPermission`;
  `create_new_file` → `allowedWithPermission`; `run_terminal_command` → `allowedWithPermission`; `search_web` →
  `allowedWithoutPermission`
  ([core/tools/definitions/readFile.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/definitions/readFile.ts),
  [createNewFile.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/definitions/createNewFile.ts),
  [runTerminalCommand.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/definitions/runTerminalCommand.ts),
  [searchWeb.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/definitions/searchWeb.ts)).
- **Per-call re-evaluation** via `evaluateToolCallPolicy(basePolicy, parsedArgs, processedArgs)`:
  - **Path containment is enforced:** `evaluateFileAccessPolicy` keeps `disabled` disabled, keeps the base policy for
    in-workspace paths, and **always downgrades to `allowedWithPermission` for paths outside the workspace**
    ([core/tools/policies/fileAccess.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/policies/fileAccess.ts)).
    This *is* the path allowlist the docs never describe. `read_file` accepts relative, absolute, tilde (`~/...`) or
    `file://` URIs and runs `preprocessArgs` through `resolveInputPath` to compute `resolvedPath.isWithinWorkspace` for
    exactly this check
    ([readFile.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/definitions/readFile.ts)).
  - **Command analysis:** `run_terminal_command` calls `evaluateTerminalCommandSecurity(basePolicy, command)` from the
    dedicated **`@continuedev/terminal-security`** package — a real dangerous-command analyzer, shipped as its own
    package ([runTerminalCommand.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/definitions/runTerminalCommand.ts),
    [packages listing](https://api.github.com/repos/continuedev/continue/contents/packages)).
- Per-model tool disabling: `chatOptions.toolOverrides.<toolName>.disabled: true`
  ([models.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/models.ts)).
- Tools are serialized to the client via `serializeTool()`, which **strips `preprocessArgs` and
  `evaluateToolCallPolicy`** — i.e. policy logic stays core-side and is not exposed
  ([core/tools/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/tools/index.ts)).
- **Organization-level policy** is a separate schema (not part of `config.yaml`): `allowAnonymousTelemetry`,
  `allowOtherOrgs`, `allowCodebaseIndexing`, `allowMcpServers` (plus a commented-out `allowLocalConfigFile`)
  ([packages/config-yaml/src/schemas/policy.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/policy.ts)).
  Shared config is applied by `modifyAnyConfigWithSharedConfig`, and the loader deliberately does not try/catch:
  *"Don't try catch this - has security implications and failure should be fatal."*
  ([loadYaml.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/config/yaml/loadYaml.ts)).
- **UI/shared settings** live in `SharedConfig` (`~/.continue/sharedConfig.json`), **not** in `config.yaml`:
  `continueAfterToolRejection`, `onlyUseSystemMessageTools`, `enableExperimentalTools`, `codebaseToolCallingOnly`,
  **`allowAnonymousTelemetry`**, `disableIndexing`, `showSessionTabs`
  ([core/config/sharedConfig.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/config/sharedConfig.ts)).
  This also **resolves the earlier "setting id unverified" gap** — `allowAnonymousTelemetry` is a real key.

## 2.9 Session persistence, resume, fork

- `cn --resume` — "Resume the most recent session"; or `/resume` inside a session: "This restores the full conversation
  history, so the agent remembers prior context."
  ([CLI quickstart](https://docs.continue.dev/cli/quickstart), [TUI mode](https://docs.continue.dev/cli/tui-mode)).
- **Headless resume:** `cn -p --resume` — "This replays the previous session's history, which can be useful for
  inspecting past results in automation" ([headless mode](https://docs.continue.dev/cli/headless-mode)).
- **Fork is explicit:** `/fork` — "Start a forked chat session from the current history"
  ([TUI mode](https://docs.continue.dev/cli/tui-mode)). The CLI also has a `--fork <sessionId>` flag
  ([extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts)).
  **Continue has a first-class fork; Cline does not.**
- Session listing: `cn ls` — "List recent chat sessions and select one to resume", with `--json` for scripting (JSON
  mode "always shows 10 sessions"); the list is limited by screen height
  ([extensions/cli/README.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/README.md)).
- Session titling: `/title` to set the title, `/rename` as an alias. `/clear` clears chat history; `/jobs` lists
  background jobs ([TUI mode](https://docs.continue.dev/cli/tui-mode)).
- **Core session storage layout:** `getSessionsFolderPath()` → `~/.continue/sessions`, index file
  `~/.continue/sessions/sessions.json`, per-session files `<sessionId>.json`
  ([core/util/paths.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/util/paths.ts)).
  **Unverified:** whether the CLI writes to that same directory (the CLI has its own `src/session.ts`).
- Config selection persists: "**Saved config** — the last-used configuration, persisted across sessions" is step 2 of
  config resolution, and `/config` selection "is saved for next time"
  ([CLI configuration](https://docs.continue.dev/cli/configuration)).
- **Reset semantics:** Continue stores data in `~/.continue` (`%USERPROFILE%\.continue` on Windows); removing that
  directory is the documented clean reset "including removing all configuration files, indices, etc"
  ([FAQs](https://docs.continue.dev/faqs)).
- **Unverified:** whether the IDE extensions expose a session list/fork UI comparable to the CLI (docs document
  `--resume` and `/fork` under CLI only).

## 2.10 Plan mode / todo lists / goal tracking

- Continue **has a Plan mode**, but its documented design is *read-only tool filtering*, not a todo-list/goal object:
  "Plan mode is a restricted environment that provides read-only access to your codebase. It's designed for safe
  exploration, understanding code, and planning changes without making any modifications."
  ([plan mode](https://docs.continue.dev/ide-extensions/agent/plan-mode)).
- Intended workflow: "1. **Start in Plan mode** to explore and understand. 2. **Develop your approach** with the
  model's help. 3. **Switch to Agent mode** when ready to implement."
  ([plan mode](https://docs.continue.dev/ide-extensions/agent/plan-mode)).
- Plan mode "shares the same interface and context features as Chat and Agent modes. You can use `@` context providers
  and highlight code just like in other modes" ([plan mode](https://docs.continue.dev/ide-extensions/agent/plan-mode)).
- A **separate plan-mode system prompt** is configurable per model via
  `chatOptions.basePlanSystemMessage: "You are a planning agent. Create clear, actionable steps."`
  ([customize agent mode](https://docs.continue.dev/ide-extensions/agent/how-to-customize)).
- There is also a standalone guide, "Using Plan Mode with Continue"
  ([plan mode guide](https://docs.continue.dev/guides/plan-mode-guide)).
- **Todo lists / focus chain / goal tracking: no equivalent feature is documented.** The closest primitives are the
  `Checklist` CLI tool (default `allow`, listed among read-only tools —
  [tool permissions](https://docs.continue.dev/cli/tool-permissions)) and `.continue/rules` for persistent instructions
  ([rules deep dive](https://docs.continue.dev/customize/deep-dives/rules)). I found **no** documented todo-list
  artifact analogous to Cline's (now-deprecated) Focus Chain.

## 2.11 Subagents & parallel orchestration

- **No subagent feature is documented on the docs site.** Nothing in the Continue docs describes spawning child agents,
  parallel research agents, or an orchestration/teams layer.
- **However, beta subagent machinery exists in the CLI source.** Flags `--beta-subagent-tool` and `--beta-status-tool`
  are declared in the CLI's option definitions, and a `src/subagent/` directory exists in the CLI source tree
  ([extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts),
  [src listing](https://api.github.com/repos/continuedev/continue/contents/extensions/cli/src)).
  So "subagents exist but are undocumented and beta-gated" is the accurate statement; the docs claim nothing.
- Other concurrency primitives: **background jobs** — `/jobs` "List background jobs"
  ([TUI mode](https://docs.continue.dev/cli/tui-mode)) and `CheckBackgroundJob` / `Status` tools among the
  default-allowed read-only tools ([tool permissions](https://docs.continue.dev/cli/tool-permissions)).
- Continue also points users outward for large-scale context rather than orchestrating agents: "For faster retrieval and
  lower costs with very large internal codebases, consider implementing a **custom code RAG** system"
  ([codebase awareness](https://docs.continue.dev/guides/codebase-documentation-awareness)).

## 2.12 Extensibility

**MCP**

- `mcpServers` key in `config.yaml`; "Currently custom tools can be configured using the Model Context Protocol
  standard" ([MCP deep dive](https://docs.continue.dev/customize/deep-dives/mcp)).
- **Reusable MCP blocks:** create `.continue/mcpServers/` at the workspace top level and drop in e.g.
  `playwright-mcp.yaml`; standalone block files require the metadata fields `name`, `version`, `schema`
  ([MCP deep dive](https://docs.continue.dev/customize/deep-dives/mcp)).
- **Interop:** "If you're coming from another tool that uses JSON MCP format configuration files (like Claude Desktop,
  Cursor, or Cline), you can copy those JSON config files directly into your `.continue/mcpServers/` directory
  (**note the plural 'Servers'**) and Continue will automatically pick them up."
  ([MCP deep dive](https://docs.continue.dev/customize/deep-dives/mcp)).
- Server properties: `name` (required), `command` (required for stdio), `args`, `env`, `cwd`,
  `requestOptions` (for `sse` / `streamable-http`), `connectionTimeout` (initial connection)
  ([MCP deep dive](https://docs.continue.dev/customize/deep-dives/mcp), [reference](https://docs.continue.dev/reference)).
- **Transports:** `stdio`, `sse`, `streamable-http` — remote servers specified by `url`:

  ```yaml
  mcpServers:
    - name: Name
      type: sse
      url: https://....
  ```
  — [MCP deep dive](https://docs.continue.dev/customize/deep-dives/mcp)
- **Secrets** use the same interpolation:

  ```yaml
  env:
    GITHUB_PERSONAL_ACCESS_TOKEN: ${{ secrets.GITHUB_PERSONAL_ACCESS_TOKEN }}
  ```
  — [MCP deep dive](https://docs.continue.dev/customize/deep-dives/mcp)
- **MCP secret resolution order (CLI source):** (1) **Organization/Package secrets** via the Continue API first;
  (2) then local env vars in the order `process.env` → `~/.continue/.env` → `<workspace>/.continue/.env` →
  `<workspace>/.env`
  ([extensions/cli/spec/mcp.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/mcp.md)).
  - **Note this is the *reverse* of the documented order for model secrets** (§2.13) — a real inconsistency.
- Only servers for the **current config** are loaded. Server statuses: `idle`, `connecting`, `connected`, `error`; only
  `connected` servers contribute prompts/tools. `/mcp` opens a management menu (restart all / stop all / view servers /
  back; per-server: restart, stop, warnings, prompts, tools) with status icons (red=error, yellow=warnings,
  green=connected, gray=idle)
  ([extensions/cli/spec/mcp.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/mcp.md)).
- `--mcp <owner/package>` adds a server at launch (repeatable)
  ([extensions/cli/src/shared-options.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/shared-options.ts)).
- **No MCP marketplace is documented**; the docs point to example MCP servers and to third-party servers (DeepWiki MCP,
  Context7 MCP) ([MCP examples](https://docs.continue.dev/customize/deep-dives/mcp-examples),
  [codebase awareness](https://docs.continue.dev/guides/codebase-documentation-awareness)).

**Prompts / slash commands**

- Prompts are "included as **user messages**" and "are especially useful as instructions for repetitive and/or complex
  tasks" — distinct from rules, which are system-message instructions
  ([prompts deep dive](https://docs.continue.dev/customize/deep-dives/prompts)).
- Making a markdown file invokable uses frontmatter `name`, `description`, **`invokable: true`** — "which will be
  available when you type / in Chat, Plan, and Agent mode"
  ([prompts deep dive](https://docs.continue.dev/customize/deep-dives/prompts)).
- Config form supports hub references and inline definitions:

  ```yaml
  prompts:
    - uses: supabase/create-functions
    - name: test
      description: Unit test a function
      prompt: |
        ...
  ```
  — [reference](https://docs.continue.dev/reference)
- **CLI invocation:** `cn --prompt supabase/create-functions "I need a function that checks for the health status"`;
  headless form `cn -p --prompt supabase/create-functions "..."` — described as kicking off a "Continuous AI workflow"
  ([prompts deep dive](https://docs.continue.dev/customize/deep-dives/prompts)).

**Custom tools / blocks / hooks**

- **Custom tools come from MCP servers**, per the docs ("Currently custom tools can be configured using the Model
  Context Protocol standard") ([MCP deep dive](https://docs.continue.dev/customize/deep-dives/mcp)). There is no
  documented first-party in-process plugin API for defining new built-in tools.
- The docs' "Context Providers" page no longer contains a "build a custom context provider in TypeScript" section;
  customization is redirected to MCP, and the legacy `/guides/build-your-own-context-provider` URL redirects to
  `/customize/deep-dives/custom-providers`
  ([custom providers](https://docs.continue.dev/customize/deep-dives/custom-providers),
  [docs.json](https://raw.githubusercontent.com/continuedev/continue/main/docs/docs.json)).
- "Blocks" in the docs means **reusable config fragments** (rules blocks, prompts blocks, MCP blocks, models blocks)
  referenced with `uses:`, not a code extension API ([reference](https://docs.continue.dev/reference)).
- **Hooks DO exist in the CLI (undocumented on the website).** Config locations, lowest→highest precedence:
  `~/.claude/settings.json`, `~/.continue/settings.json` (user-global) → `.claude/settings.json`,
  `.continue/settings.json` (project) → `.claude/settings.local.json`, `.continue/settings.local.json` (project-local).
  - **Exit-code semantics: 0 = proceed, 2 = block (stderr becomes feedback), anything else = non-blocking error.**
    Optional JSON output with `hookSpecificOutput`. Hook types `command` (shell) and `http` (POST) are supported;
    `prompt`/`agent` hooks are "not yet implemented". Types are described as "**Claude Code-compatible**".
  - Source: [extensions/cli/AGENTS.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/AGENTS.md)
  - CLI hook implementation lives in `src/hooks/` (`HookService.ts`, `hookConfig.ts`, `hookRunner.ts`, `fireHook.ts`,
    `types.ts`) ([extensions/cli/AGENTS.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/AGENTS.md)).
  - **This corrects the common assumption that Continue has no hooks.** It has them, Claude-Code-compatible, in the CLI.

## 2.13 config.yaml — the configuration surface

- "Continue Agents are defined using the `config.yaml` specification. **Agents** are composed of models, rules, and
  tools (MCP servers)." ([reference](https://docs.continue.dev/reference)).
- **Top-level properties:** `name` (**required**), `version` (**required**), `schema` (**required**), `models`,
  `context`, `rules`, `prompts`, `docs`, `mcpServers`, `data`. "All properties at all levels are optional unless
  explicitly marked as required" ([reference](https://docs.continue.dev/reference)).
- **Schema verified in source:** the Zod schema's top-level key set is `name`, `version`, `schema`, `metadata`, `env`,
  `requestOptions`, `models`, `context`, `data`, `mcpServers`, `rules`, `prompts`, `docs`
  ([packages/config-yaml/src/schemas/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/index.ts)).
  There is **no** top-level `embeddingsProvider`, `reranker`, `tools`, or `policies`/`permissions` key. (`tools` exists
  only in the legacy internal `configSchema` in that same file, not in `configYamlSchema`.)
- **Model entry keys:** `name` (required), `provider` (required), `model` (required), `apiBase`, `roles`,
  `capabilities`, `maxStopWords`, `promptTemplates`, `chatOptions`, `embedOptions`, `defaultCompletionOptions`,
  `requestOptions`, `autocompleteOptions` ([reference](https://docs.continue.dev/reference)).
- **Model roles** (the provider-abstraction core): `chat`, `autocomplete`, `embed`, `rerank`, `edit`, `apply`,
  `summarize`. "The default value is `[chat, edit, apply, summarize]`. Note that the **`summarize` role is not
  currently used**." ([reference](https://docs.continue.dev/reference)).
- **There is a NINTH role in source that the docs omit: `subagent`.** The source enum is
  `["chat","autocomplete","embed","rerank","edit","apply","summarize","subagent"]`, and the internal config builds
  `modelsByRole` / `selectedModelByRole` buckets for it
  ([packages/config-yaml/src/schemas/models.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/models.ts),
  [core/config/yaml/loadYaml.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/config/yaml/loadYaml.ts)).
  This is corroborating evidence for a subagent capability that is otherwise undocumented (see §2.11).
- **Full model key set from the source schema** (a superset of the docs):
  `name`, `model`, `provider` (required for a full model), `apiKey`, `apiBase`, `contextLength`, `maxStopWords`,
  `roles`, `capabilities`, `defaultCompletionOptions`, **`cacheBehavior`**, `requestOptions`, `embedOptions`,
  `chatOptions`, `promptTemplates`, `useLegacyCompletionsEndpoint`, `useResponsesApi`, **`env`**, `autocompleteOptions`
  ([models.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/models.ts)).
  `env` is a `string → string|boolean|number` record on a model, and `requestOptions` is **also available at the top
  level** of `config.yaml` (`baseConfigYamlSchema`).
- **`chatOptions` has more than the three system messages:** alongside `baseSystemMessage`,
  `baseAgentSystemMessage` and `basePlanSystemMessage`, there is **`toolOverrides`** — a record keyed by tool name
  allowing `description`, `displayTitle`, `wouldLikeTo`, `isCurrently`, `hasAlready`, `systemMessageDescription` and
  `disabled` ([models.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/models.ts)).
- **Model capabilities:** `tool_use` — "Enables function/tool calling support (**required for Agent mode**)"; and
  `image_input`. "Continue automatically detects these capabilities for most models, but you can override this" —
  ([reference](https://docs.continue.dev/reference)). Workaround documented for detection failures: add
  `capabilities: ["tool_use"]` ([FAQs](https://docs.continue.dev/faqs)).
  - Source adds a third capability, **`next_edit`**, and accepts arbitrary strings for forward-compat
    ([models.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/models.ts)).
  - **Important limitation:** "You cannot override autodetection - you can only **add** capabilities." An empty
    `capabilities: []` does **not** disable autodetection
    ([model capabilities](https://docs.continue.dev/customize/deep-dives/model-capabilities)).
  - Autodetection lives in `PROVIDER_TOOL_SUPPORT` (per-provider regex/substring heuristics) and
    `modelSupportsNativeTools()` prefers `capabilities.tools` when set, else falls back to the provider heuristic
    ([core/llm/toolSupport.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/llm/toolSupport.ts)).
- `defaultCompletionOptions`: `contextLength`, `maxTokens`, `temperature`, `topP`, `topK`, `stop`, `reasoning`,
  `reasoningBudgetTokens`, and `keepAlive` (Ollama, "default: `1800`")
  ([reference](https://docs.continue.dev/reference)).
  - Source adds **`minP`, `presencePenalty`, `frequencyPenalty`, `n`, `promptCaching`, `stream`**
    ([models.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/models.ts)).
    **`stream` and `promptCaching` as per-model completion options** are design-relevant and undocumented.
- **Documented role fallback:** "The selected chat model will also be used for Edit and Apply if no `edit` or `apply`
  models are specified, respectively" ([chat role](https://docs.continue.dev/customize/model-roles/chat)).
  **No automatic model tiering/fallback chain is documented** — the only artifact is a PR titled "feat: fallback to chat
  model from apply" (#5058), seen as a search-result title only, **unverified**.
- `requestOptions`: `timeout`, `verifySsl`, `caBundlePath`, `proxy`, `headers`, `extraBodyProperties`, `noProxy`,
  `clientCertificate{cert,key,passphrase}` ([reference](https://docs.continue.dev/reference)).
- `autocompleteOptions` reveals the completion design: `disable`, `maxPromptTokens`, `debounceDelay`, `modelTimeout`,
  `maxSuffixPercentage`, `prefixPercentage`, `transform`, `template` (Mustache with `{{{ prefix }}}`, `{{{ suffix }}}`,
  `{{{ filename }}}`, `{{{ reponame }}}`, `{{{ language }}}`), `onlyMyCode`, `useCache`, `useImports`,
  `useRecentlyEdited`, `useRecentlyOpened` ([reference](https://docs.continue.dev/reference)).
- `promptTemplates` can override role prompts: valid values are `chat`, `edit`, `apply`, `autocomplete`; "The `chat`
  property must be a valid template name, such as `llama3` or `anthropic`"
  ([reference](https://docs.continue.dev/reference)).
- **Config file locations:** macOS/Linux `~/.continue/config.yaml`; Windows `%USERPROFILE%\.continue\config.yaml`.
  Saving reloads config automatically ("no restart required"), though the FAQ notes VS Code may need "Reload Window"
  ([understanding configs](https://docs.continue.dev/guides/understanding-configs), [FAQs](https://docs.continue.dev/faqs)).
- **CLI resolution order (documented):** (1) `--config` flag → (2) saved config (last-used, persisted) → (3) default
  `~/.continue/config.yaml` ([CLI configuration](https://docs.continue.dev/cli/configuration)).
- **CLI resolution order (source, slightly different):** `--config` flag (accepts a **file path or an assistant slug**)
  → local `~/.continue/config.yaml` if it exists → remote default agent fetched as `continuedev` /
  **`default-cli-config`** via the platform API. Path-vs-slug heuristic: treated as a **file path** if it starts with
  `.`, `/`, `~`, a Windows drive letter, a UNC `\\`, or contains `.yaml`, `.yml`, or `.json`; otherwise a slug
  ([extensions/cli/src/configLoader.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/configLoader.ts)).
- **YAML anchors** are supported for de-duplication, requiring the `%YAML 1.1` header and a `model_defaults` anchor
  ([reference](https://docs.continue.dev/reference)).
- **Secrets:** `${{ secrets.SECRET_NAME }}` anywhere in config, resolved in order
  (1) `<workspace-root>/.env`, (2) `<workspace-root>/.continue/.env`, (3) `~/.continue/.env`,
  (4) process environment variables. Critical caveat: "**IDE extensions (VS Code, JetBrains) cannot read your shell
  environment variables.** Setting `export OPENAI_API_KEY=...` in your terminal will not make the key available to
  Continue running inside your IDE. You must use a `.env` file instead." Process env vars work only with the CLI —
  ([FAQs](https://docs.continue.dev/faqs)).
- `CONTINUE_GLOBAL_DIR` overrides the `~/.continue` home directory (used by both CLI `env.continueHome` and core)
  ([extensions/cli/src/env.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/env.ts),
  [core/util/paths.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/util/paths.ts)).
- Deprecated `config.json` remains documented; "If a `config.yaml` file is present, it will be loaded instead of
  config.json" ([yaml migration](https://docs.continue.dev/reference/yaml-migration)).
- **`config.json` → `config.yaml` migration map** (useful for reading older material):
  `models` → `roles: [chat]`; `tabAutocompleteModel` → `roles: [autocomplete]`; `embeddingsProvider` →
  `roles: [embed]`; `reranker` → `roles: [rerank]`; `experimental.modelRoles.inlineEdit` → `roles: [chat, edit]`;
  `applyCodeBlock` → `roles: [chat, apply]`; `completionOptions` → `defaultCompletionOptions`; `contextProviders`
  (with JSON `name` → YAML `provider`) → `context`; `systemMessage` → `rules: [...]`; `customCommands` → `prompts`;
  docs `title` → `name`; `experimental.modelContextProtocolServers` → `mcpServers`. Deprecated with no YAML equivalent:
  `slashCommands`, top-level `requestOptions`, top-level `completionOptions`, `tabAutocompleteOptions.*` (except some),
  `analytics`, `customCommands`, `experimental`, `userToken`, and the `repoMapFileSelection` model role
  ([yaml migration](https://docs.continue.dev/reference/yaml-migration)).

## 2.14 Provider abstraction & multi-model support

- Model roles are the abstraction seam: one config binds different providers/models to `chat`, `autocomplete`,
  `embed`, `rerank`, `edit`, `apply` ([reference](https://docs.continue.dev/reference)).
- **Any OpenAI-compatible endpoint** is configured with `provider: openai` plus `apiBase`:

  ```yaml
  models:
    - name: My Model - OpenAI-Compatible
      provider: openai
      apiBase: http://my-endpoint/v1
      model: my-custom-model
      capabilities:
        - tool_use
        - image_input
      roles: [chat, edit]
  ```
  — [reference](https://docs.continue.dev/reference)
- "For many cases, either Continue will have a built-in provider or the API you use will be OpenAI-compatible, in which
  case you can use the 'openai' provider and change the 'baseUrl' to point to the server"
  ([self-host a model](https://docs.continue.dev/guides/how-to-self-host-a-model)).
- Official list of OpenAI-compatible servers named: KoboldCpp, text-gen-webui, FastChat, LocalAI, llama-cpp-python,
  TensorRT-LLM, **vLLM**, BerriAI/litellm, Tetrate Agent Router Service
  ([openai provider](https://docs.continue.dev/customize/model-providers/top-level/openai)).
  - **Unverified / gap:** `https://docs.continue.dev/customize/model-providers/more/vllm` returns **404**, even though
    redirects for a vLLM page are declared and `deploy-os-code-llm` covers vLLM — so vLLM is officially supported only
    via the OpenAI-compatible `apiBase` pattern
    ([openai provider](https://docs.continue.dev/customize/model-providers/top-level/openai),
    [docs.json](https://raw.githubusercontent.com/continuedev/continue/main/docs/docs.json)).
- Extra OpenAI-compat keys: `useLegacyCompletionsEndpoint: true` forces `/completions` instead of `/chat/completions`;
  `useResponsesApi: false` disables OpenAI's `/responses` endpoint (used by default for o-series and gpt-5)
  ([openai provider](https://docs.continue.dev/customize/model-providers/top-level/openai)).
- Providers named in official docs: Anthropic, OpenAI, Gemini, Ollama, **Amazon Bedrock**, **Azure**, xAI, and more;
  plus Mistral, OpenRouter, Voyage, Morph, Relace, Inception
  ([models](https://docs.continue.dev/customize/models)). Provider inventory pages also cover Azure AI Foundry,
  Hugging Face, LM Studio, Tetrate, Vertex AI, Groq, Together, DeepInfra, Cohere, NVIDIA, Cloudflare, MiniMax,
  SambaNova, Watson x, Sagemaker, Nebius, and `more/` pages for asksage, clawrouter, deepseek, groq, llamacpp,
  llamastack, mimo, mistral, moonshot, nous, nvidia, tensorix, together, xAI, zai
  ([providers overview](https://docs.continue.dev/customize/model-providers/overview),
  [docs.json](https://raw.githubusercontent.com/continuedev/continue/main/docs/docs.json)).
- Provider implementation contract (contributor docs): providers live in `core/llm/llms`, must extend `BaseLLM`, be
  registered in `core/llm/llms/index.ts`, and optionally be added to `PROVIDER_SUPPORTS_IMAGES` in
  `core/llm/autodetect.ts` ([CONTRIBUTING.md](https://raw.githubusercontent.com/continuedev/continue/main/CONTRIBUTING.md)).
- **Local models** are a first-class role assignment:

  ```yaml
  - name: Ollama Starcoder
    provider: ollama
    model: starcoder
    roles: [autocomplete]
  ```
  — [reference](https://docs.continue.dev/reference)
- **LM Studio** default `apiBase` is `http://localhost:1234/v1`
  ([lmstudio provider](https://docs.continue.dev/customize/model-providers/top-level/lmstudio)).
- **Ollama** three config methods: (1) model blocks `uses: ollama/deepseek-r1-32b` (blocks only supply config — you must
  still `ollama pull` the exact tag); (2) `model: AUTODETECT` with `provider: ollama` (scans local `ollama list`);
  (3) manual ([ollama guide](https://docs.continue.dev/guides/ollama-guide)).
- Honest capability caveat from Continue about local models: "Their limited tool calling and reasoning capabilities will
  make it challenging to use agent mode" ([models](https://docs.continue.dev/customize/models)).
  Known issue that some models (e.g. DeepSeek R1) advertise tools but fail
  ([ollama guide](https://docs.continue.dev/guides/ollama-guide)).
- Common local-model errors documented: `404 model "x" not found, try pulling it first`; "Model requires more system
  memory" → lower `defaultCompletionOptions.contextLength` (e.g. 2048) because "Continue may set a higher default
  context length than other Ollama tools"; remote access needs `OLLAMA_HOST=0.0.0.0:11434` (and often
  `OLLAMA_ORIGINS=*`); Docker → `host.docker.internal` (Win/Mac) or `172.17.0.1` (Linux)
  ([ollama guide](https://docs.continue.dev/guides/ollama-guide),
  [ollama provider](https://docs.continue.dev/customize/model-providers/top-level/ollama),
  [FAQs](https://docs.continue.dev/faqs)).
- `chatOptions` allows per-model system prompt overrides for all three modes
  ([reference](https://docs.continue.dev/reference)).
- **Offline/air-gapped path is documented:** install from `.vsix`, turn off anonymous telemetry, set a local model,
  restart ([without internet](https://docs.continue.dev/guides/running-continue-without-internet)).
- Self-hosting recipes referenced: HuggingFace TGI, vLLM, SkyPilot, Anyscale Private Endpoints ("OpenAI compatible
  API"), Lambda — all under <https://github.com/continuedev/deploy-os-code-llm>
  ([self-host a model](https://docs.continue.dev/guides/how-to-self-host-a-model)).
- TLS troubleshooting: `unable to verify the first certificate`, `self signed certificate in certificate chain`,
  `certificate verify failed`, `CERT_UNTRUSTED` → set `requestOptions.caBundlePath`; mutual TLS →
  `clientCertificate`; `verifySsl: false` only as a temporary debug step
  ([troubleshooting](https://docs.continue.dev/troubleshooting)).
- Enterprise/auth nuance: 2.0.0 "pulled out authentication", so the older hub/assistant-based config sharing described
  in some pages may not match the frozen artifact — treat hub `uses:` references as partially unverified for 2.0.0
  although the syntax is still documented
  ([README](https://raw.githubusercontent.com/continuedev/continue/main/README.md),
  [reference](https://docs.continue.dev/reference)).

## 2.15 Streaming output & TUI/GUI/IDE UX

- Surfaces: VS Code extension, JetBrains plugin, and the **`cn` TUI**. "Continue CLI (`cn`) is a terminal-based coding
  agent… the **same agent that powers the Continue IDE extensions**, running in your terminal."
  ([CLI quickstart](https://docs.continue.dev/cli/quickstart)).
- "`cn` uses the same underlying agent as the Continue IDE extensions"
  ([TUI mode](https://docs.continue.dev/cli/tui-mode)).
- **Install:** shell installer
  `curl -fsSL https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/scripts/install.sh | bash`;
  Windows PowerShell `irm https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/scripts/install.ps1 | iex`;
  or npm `npm i -g @continuedev/cli`. **Node.js 20+** for the npm path; the shell installer bundles its own runtime.
  Verify with `cn --version` ([CLI quickstart](https://docs.continue.dev/cli/quickstart),
  [extensions/cli/README.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/README.md)).
  - Package metadata: `@continuedev/cli`, `"bin": { "cn": "dist/cn.js" }`, `"type": "module"`,
    `"license": "Apache-2.0"`, `"engine": { "node": ">=18" }`, in-repo version placeholder `0.0.0-dev` (releases via
    semantic-release) ([extensions/cli/package.json](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/package.json)).
- **Auth:** `cn login` (opens browser, WorkOS-based) or `CONTINUE_API_KEY` env var for headless/CI; or a direct
  Anthropic API key ([CLI quickstart](https://docs.continue.dev/cli/quickstart),
  [extensions/cli/AGENTS.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/AGENTS.md)).
- **TUI implementation:** Ink/React-based; `src/ui/TUIChat.tsx` is the main chat component. There are **three runtime
  modes in source: headless, TUI, and "standard" (readline)**
  ([extensions/cli/AGENTS.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/AGENTS.md)).
- TUI slash commands:

  | Command | What it does |
  |---|---|
  | `/help` | Show the help message and keyboard shortcuts |
  | `/clear` | Clear the chat history |
  | `/login` | Authenticate with your account |
  | `/logout` | Sign out of your current session |
  | `/update` | Update the Continue CLI |
  | `/whoami` | Check who you're currently logged in as |
  | `/info` | Show session information, including token usage and cost |
  | `/model` | Switch between configured chat models |
  | `/config` | Switch configuration or organization |
  | `/mcp` | Manage MCP server connections |
  | **`/init`** | **Create an `AGENTS.md` file for the current project** |
  | `/compact` | Summarize chat history into a compact form |
  | `/resume` | Resume a previous chat session |
  | `/fork` | Start a forked chat session from the current history |
  | `/title` | Set the title for the current session |
  | `/rename` | Rename the current session (alias for `/title`) |
  | `/exit` | Exit the chat |
  | `/jobs` | List background jobs |

  Source: [TUI mode](https://docs.continue.dev/cli/tui-mode). When connected to a remote environment, Continue adds
  remote-only commands such as `/diff` and `/apply`.
- **Interactive questions in the TUI:** the agent's `AskQuestion` tool renders "a quiz-style prompt directly in the
  terminal" — arrow keys to highlight, Enter to submit, typing sends a custom answer, and "If a default answer is
  provided, pressing Enter on an empty input submits that default"
  ([TUI mode](https://docs.continue.dev/cli/tui-mode)).
- Double `Ctrl+C` within 1 second exits; a single `Ctrl+C` shows a "ctrl+c to exit" message for 1 second
  ([extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts)).
- **`cn serve` / `cn remote` — an HTTP wire protocol (hidden from the docs).** `cn serve [prompt]` is a **hidden**
  command starting "an HTTP server with `/state` and `/message` endpoints"; options `--timeout <seconds>` (default
  **300**), `--port` (default **8000**), `--id <storageId>`, `--beta-upload-artifact-tool`
  ([extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts)).
  The protocol is specified as polling REST: `GET /state` returns `chatHistory[]` (with `role`, `content`,
  `isStreaming`, `messageType` of `tool-start|tool-result|tool-error|system`, `toolName`, `toolResult`) plus
  `isProcessing` and `messageQueueLength`; `POST /message` returns `queued`, `position`, `willInterrupt` (an empty
  message interrupts; `/exit` shuts down); `GET /diff` returns the git diff vs main (404 if not a repo); `POST /exit`.
  **Client polls every 500 ms. The server binds to `127.0.0.1` with NO authentication.**
  ([extensions/cli/spec/wire-format.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/wire-format.md)).
  - **Doc/source conflict:** `index.ts` defaults `--port` to **8000**, but `spec/wire-format.md` says the server runs on
    **port 3000**. Unresolved.
- `cn checks [action] [pr-url]` — "Show CI check statuses for a PR"; `cn review` — "Run AI-powered reviews on your
  changes" with `--base <ref>`, `--format`, `--fix`, `--patch`, `--fail-fast`, `--review-agents <agents...>`,
  `--verbose` ([extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts)).
- GUI detail: `gui/` is a separate React front-end with a named theme layer at `gui/src/styles/theme.ts` (Tailwind +
  VS Code theme variables) and a dev-only Theme Test Page at Settings → Help
  ([CONTRIBUTING.md](https://raw.githubusercontent.com/continuedev/continue/main/CONTRIBUTING.md)).
- **Unverified:** streaming behaviour specifics beyond the CLI wire format (partial-token rendering, tool-call
  streaming) are not documented on any fetched page.

## 2.16 Non-interactive / headless / CI mode & scriptability

- `cn -p "your prompt"` runs a single task: "The agent runs to completion and prints its response to stdout. Use this in
  scripts, CI/CD, and git hooks." `-p` is the short form of `--print`; the commander description reads: "Continue CLI -
  AI-powered development assistant. Starts an interactive session by default, use -p/--print for non-interactive output"
  ([headless mode](https://docs.continue.dev/cli/headless-mode),
  [extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts)).
- **Stdin piping is a first-class context mechanism:**

  ```bash
  git diff --staged | cn -p "Write a commit message for this diff"
  cat error.log | cn -p "Explain what went wrong"
  ```

- **Stdout piping back into tooling:**

  ```bash
  cn -p "Generate a commit message" | git commit -F -
  cn -p "List all TODO comments in src/" --silent > todos.txt
  ```
  — [headless mode](https://docs.continue.dev/cli/headless-mode)
- **Source detail on stdin handling:** in **headless** mode piped input is wrapped and prepended as
  `<stdin>\n...\n</stdin>\n\n<prompt>`; in **TUI** mode piped input is concatenated as `stdinInput + "\n\n" + prompt`
  instead ([extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts)).
  Different wrapping per mode is a subtle but real design detail.
- A prompt is required with `-p` unless `--agent`, `--prompt`, or `--resume` is supplied; otherwise it prints usage and
  exits **1** ([extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts)).
- **TTY-less design:** "The CLI will not attempt to read stdin or initialize the interactive UI when running in
  headless mode with a supplied prompt"; it "Skips stdin reading when a prompt is supplied / Disables interactive UI
  components / Ensures clean stdout/stderr output"; **`FORCE_NO_TTY`** forces TTY-less mode
  ([extensions/cli/README.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/README.md)).
- **Output control:** `--silent` "Strip `<think>` tags and excess whitespace from output"; `--format json` "Output
  structured JSON". Both work **only with `-p`/`--print`**
  ([headless mode](https://docs.continue.dev/cli/headless-mode),
  [extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts)).
  - **Unverified:** the exact JSON *schema* emitted by `--format json`. `extensions/cli/spec/wire-format.md` describes
    a different JSON surface (`cn remote` ⇄ `cn serve` HTTP), not `--format json`.
- **Headless permission model:** see §2.8 for the resolved doc conflict — docs say `ask` tools are excluded; the spec
  says they cause a process **error exit**. Enabling writes is explicit:
  `cn -p "Fix the type errors in src/" --allow Write --allow Edit`, or `--allow "*"` to allow everything
  ([headless mode](https://docs.continue.dev/cli/headless-mode),
  [tool permissions](https://docs.continue.dev/cli/tool-permissions)).
- **CI examples given officially:** a git hook (`cn -p "Review staged changes for obvious bugs" --silent`), a CI
  pipeline auto-fix (`cn -p "Fix all ESLint errors in src/" --allow Write --allow Edit --allow Bash`), docs generation
  (`cn -p "@src/api/ Generate OpenAPI documentation for these endpoints" --silent > api-docs.yaml`), assistant
  selection (`--config my-org/error-triage-assistant`), Docker (`docker run --rm my-image cn -p "Generate docs"`), and
  explicit use "From VSCode/IntelliJ extension terminal tool"
  ([headless mode](https://docs.continue.dev/cli/headless-mode),
  [extensions/cli/README.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/README.md)).
- **Launch-time config injection without editing files:** `cn --rule ./rules/style.md`,
  `cn --rule "Always use TypeScript strict mode"`, `cn --agent my-org/pr-reviewer`, `cn --mcp <slug>`,
  `cn --model <slug>`, `cn --prompt <slug>` — all repeatable
  ([CLI configuration](https://docs.continue.dev/cli/configuration),
  [extensions/cli/src/shared-options.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/shared-options.ts)).
- **Exit code 130 on keyboard interruption** in non-TUI flows; unhandled rejections/exceptions are recorded
  (`markUnhandledError`) and cause a non-zero exit without immediately killing the process
  ([extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts)).
  **Continue documents an interrupt exit code; Cline documents none.**
- `--org <slug>` is "supported only in headless mode"; org identity is read via `getOrganizationId(authConfig)` from
  stored auth ([extensions/cli/src/shared-options.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/shared-options.ts)).
  **Unverified:** a `CONTINUE_ORGANIZATION_ID` variable — the telemetry service instead reads
  `process.env.ORGANIZATION_ID` and `process.env.ACCOUNT_UUID`
  ([telemetryService.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/telemetry/telemetryService.ts)).
- CLI-specific env vars: `CONTINUE_CLI_DISABLE_COMMIT_SIGNATURE` (disable the Continue commit signature in generated
  commit messages) and `FORCE_NO_TTY`
  ([extensions/cli/README.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/README.md));
  `CONTINUE_API_BASE` (default `https://api.continue.dev/`) and `CONTINUE_API_KEY`
  ([.env.example](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/.env.example),
  [env.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/env.ts)).
- CLI background-job tooling: `/jobs`, `CheckBackgroundJob`, `Status`
  ([TUI mode](https://docs.continue.dev/cli/tui-mode), [tool permissions](https://docs.continue.dev/cli/tool-permissions)).

## 2.17 Observability

- **Token usage and cost are shown per session** via `/info` — "Show session information, including token usage and
  cost" ([TUI mode](https://docs.continue.dev/cli/tui-mode)).
- **`--verbose`** sets the logger level to `debug` and prints the session id and log path, e.g.
  `Verbose logging enabled (session: <id>)`, `Logs: <path>`, and suggests
  `grep '\[<sessionId>\]' <logfile>`. In headless mode those console lines are suppressed
  ([extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts)).
- **CLI log file is `~/.continue/logs/cn.log`** (winston file transport, `maxsize` 10 MB, `maxFiles` 5), with a
  per-process 8-hex-char session id in every line and timestamp format `YYYY-MM-DD HH:mm:ss.SSS`. In headless mode
  `logger.error` also writes to stderr. The convenience script is
  `watch:logs` = `touch ~/.continue/logs/cn.log && tail -f ~/.continue/logs/cn.log`
  ([extensions/cli/src/util/logger.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/util/logger.ts),
  [extensions/cli/package.json](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/package.json)).
- In headless mode `console.info` is overridden to a no-op so only intended output reaches stdout
  ([extensions/cli/src/logger.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/logger.ts),
  [extensions/cli/src/logging.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/logging.ts)).
- Core/IDE logs: JetBrains → `~/.continue/logs/core.log`; VS Code → Developer Tools console (set level to "Verbose");
  prompt/analytics logs via the setting **"Continue: Enable Console"** plus the command
  "Continue: Focus on Continue Console View" ([troubleshooting](https://docs.continue.dev/troubleshooting)).
  Core paths in source: `~/.continue/logs/core.log` (`getCoreLogsPath`) and `~/.continue/logs/prompt.log`
  (`getPromptLogsPath`) ([core/util/paths.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/util/paths.ts)).

**OpenTelemetry — present, source-level, far richer than the docs**

- Metrics are only enabled when an OTLP target is configured: `enabled = telemetryEnabled && hasOtelConfig`, where
  `hasOtelConfig` requires one of `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_METRICS_ENDPOINT`, or
  `OTEL_METRICS_EXPORTER`. Default `OTEL_METRICS_EXPORTER` is `console` (not `otlp`)
  ([telemetryService.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/telemetry/telemetryService.ts)).
- **Kill switches, in precedence order:** `CONTINUE_METRICS_ENABLED=0|1` (preferred, takes precedence) → else
  `CONTINUE_CLI_ENABLE_TELEMETRY !== "0"`
  ([telemetryService.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/telemetry/telemetryService.ts)).
- **Doc/source conflict:** the source implements **neither** `CONTINUE_TELEMETRY_ENABLED` (the documented CLI opt-out)
  **nor** `OTEL_METRICS_INCLUDE_SESSION_ID` in that file
  ([telemetryService.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/telemetry/telemetryService.ts),
  [telemetry.mdx](https://raw.githubusercontent.com/continuedev/continue/main/docs/customize/telemetry.mdx)).
- OTLP env vars documented in the spec: `CONTINUE_METRICS_ENABLED`, `CONTINUE_CLI_ENABLE_TELEMETRY` (legacy, lower
  precedence), `OTEL_METRICS_EXPORTER` (`console`|`otlp`|`prometheus`), `OTEL_LOGS_EXPORTER`,
  `OTEL_EXPORTER_OTLP_PROTOCOL` (`grpc`|`http/json`|`http/protobuf`), `OTEL_EXPORTER_OTLP_ENDPOINT`,
  `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_METRIC_EXPORT_INTERVAL` (spec default 60000; **source default 20000**),
  `OTEL_LOGS_EXPORT_INTERVAL` (default 5000), `OTEL_LOG_USER_PROMPTS`. Cardinality controls:
  `OTEL_METRICS_INCLUDE_SESSION_ID` (default true), `OTEL_METRICS_INCLUDE_VERSION` (default false),
  `OTEL_METRICS_INCLUDE_ACCOUNT_UUID` (default true)
  ([extensions/cli/spec/otlp-metrics.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/otlp-metrics.md),
  [telemetryService.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/telemetry/telemetryService.ts)).
  - **Spec/source conflict:** `OTEL_METRIC_EXPORT_INTERVAL` defaults 60000 in the spec vs 20000 in source.
- **Implemented metric names:** `continue_cli_session_count`, `continue_cli_lines_of_code_count`,
  `continue_cli_pull_request_count`, `continue_cli_commit_count`, `continue_cli_cost_usage` (USD),
  **`continue_cli_token_usage`** (types `input|output|cacheRead|cacheCreation`),
  `continue_cli_code_edit_tool_decision`, `continue_cli_active_time_total`, `continue_cli_auth_attempts`,
  `continue_cli_mcp_connections` (observable gauge), `continue_cli_startup_time`, `continue_cli_response_time`, plus
  `continue_cli_slash_command_usage` (present in source but not in the spec doc). Resource attributes:
  `service.name = continue-cli`, service version, host name, `deployment.environment` (from `NODE_ENV`, default
  `development`), `os.type`
  ([telemetryService.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/telemetry/telemetryService.ts)).
- **Deliberately Claude Code–compatible:** "The core metrics … use identical naming and attribute structures to Claude
  Code, allowing for easy dashboard migration by simply changing the metric prefix from `claude_code_*` to
  `continue_cli_*`." Privacy: "No PII … User prompts are redacted unless `OTEL_LOG_USER_PROMPTS=1`"
  ([spec/otlp-metrics.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/otlp-metrics.md)).
  Spec implementation status: core metrics 6/7, core events 3/3, additional metrics 4/7.
- Event-like logs (`continue_cli_user_prompt`, `continue_cli_tool_result`, `continue_cli_api_request`) exist as methods
  but are marked "TODO: Implement OTLP logs export" — they currently only write to the debug logger
  ([telemetryService.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/telemetry/telemetryService.ts)).

**Built-in telemetry (and a significant staleness signal)**

- Official telemetry policy: PostHog is the analytics platform; implementation claimed at
  `gui/src/hooks/CustomPostHogProvider.tsx`. **What is tracked:** suggestion accept/reject (excluding actual
  code/prompts), model and command name, number of tokens generated, OS name and IDE name, pageview statistics
  ([docs/customize/telemetry.mdx](https://raw.githubusercontent.com/continuedev/continue/main/docs/customize/telemetry.mdx)).
- Opt out (IDE): toggle "Allow Anonymous Telemetry" off; in VS Code also uncheck **"Continue: Telemetry Enabled"**
  (which "will override the Settings Page settings"). Opt out (CLI):
  `export CONTINUE_TELEMETRY_ENABLED=0` or inline `CONTINUE_TELEMETRY_ENABLED=0 cn <prompt>`
  ([telemetry.mdx](https://raw.githubusercontent.com/continuedev/continue/main/docs/customize/telemetry.mdx)).
- **Unverified:** the `allowAnonymousTelemetry` setting *id* — docs give only display labels.
- **The live docs page is 404:** `https://docs.continue.dev/customize/telemetry` returns 404 and the page is absent
  from `docs.json` navigation, even though `/telemetry` → `/customize/telemetry` redirects are declared
  ([docs.json](https://raw.githubusercontent.com/continuedev/continue/main/docs/docs.json)).
- **Doc conflict:** the README says 2.0.0 included "**removing anonymous telemetry**", while
  `docs/customize/telemetry.mdx` still documents PostHog telemetry and full opt-out instructions. Treat the README note
  as the newer state of the shipped build and the `.mdx` as stale
  ([README](https://raw.githubusercontent.com/continuedev/continue/main/README.md),
  [telemetry.mdx](https://raw.githubusercontent.com/continuedev/continue/main/docs/customize/telemetry.mdx)).
- **Development data — a separate, local, first-class observability surface:** "By default, this development data is
  saved to **`.continue/dev_data`** on your local machine." Source paths:
  `~/.continue/dev_data/devdata.sqlite` and `~/.continue/dev_data/<schema>/<eventName>.jsonl`. Custom destinations are
  configured under the `data` key with `name`, `destination` (HTTP POST endpoint or `file://` dir), `schema`
  (`0.1.0`|`0.2.0`), `events[]`, **`level` (`all`|`noCode`** — the latter excludes file contents, prompts and
  completions), `apiKey`, `requestOptions`. Event/schema definitions in `packages/config-yaml/src/schemas/data`
  ([development data](https://docs.continue.dev/customize/deep-dives/development-data),
  [reference](https://docs.continue.dev/reference),
  [core/util/paths.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/util/paths.ts)).
  Example event names include `autocomplete` and `chatInteraction` ([reference](https://docs.continue.dev/reference)).

## 2.18 Evaluation & regression tests

- **There is no eval harness.** The `eval/` directory exists in the repo but contains only a `.gitignore` whose sole
  line is `repos` — verified on `main`, at tag `v1.5.45`, and at the 2024 commit `d0f3209` which is the most recent
  commit touching the `eval` path per the commits API
  ([eval/.gitignore](https://raw.githubusercontent.com/continuedev/continue/main/eval/.gitignore),
  [contents](https://api.github.com/repos/continuedev/continue/contents/eval),
  [at v1.5.45](https://api.github.com/repos/continuedev/continue/contents/eval?ref=v1.5.45)).
- **No `eval/builtin` configs, eval harness, or benchmark results were found in the current tree or in official docs.**
  The docs nav contains no eval pages ([docs.json](https://raw.githubusercontent.com/continuedev/continue/main/docs/docs.json)).
  **Unverified / apparently absent.**
- What *does* exist as official test tooling, per CONTRIBUTING: "We have a mix of unit, functional, and e2e test
  suites, with a primary focus on functional testing. These tests run on each pull request." No eval framework is named
  ([CONTRIBUTING.md](https://raw.githubusercontent.com/continuedev/continue/main/CONTRIBUTING.md)).
- `core/package.json` declares both `jest.config.js` and `vitest.config.ts`; test files live throughout `core/` (e.g.
  `core/indexing/CodebaseIndexer.test.ts`, `FullTextSearchCodebaseIndex.test.ts`, `LanceDbIndex.test.skip.ts`,
  `ignore.vitest.ts`, `shouldIgnore.test.ts`, `walkDir.test.ts`, `CodeSnippetsIndex.test.ts`)
  ([core/indexing listing](https://api.github.com/repos/continuedev/continue/contents/core/indexing)).
- **CLI-specific test harness (real, mostly undocumented):** `vitest.config.ts`, `vitest.e2e.config.ts`,
  `vitest.smoke-api.config.ts`, `vitest.setup.ts`, `smoke-test.mjs`, `demo.expect`, `test-fixtures/`, and source dirs
  `src/e2e/`, `src/smoke-api/`, `src/__tests__/`, `src/test-helpers/`. npm scripts: `test`, `test:watch`, `test:ui`,
  `test:e2e`, `test:smoke`, `test:smoke-api`
  ([extensions/cli listing](https://api.github.com/repos/continuedev/continue/contents/extensions/cli),
  [extensions/cli/package.json](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/package.json)).
- Additional CLI spec docs exist (filenames only; contents not all fetched): `spec/testing-strategies.md`,
  `spec/config-loading.md`, `spec/ctrl-c-behavior.md`, `spec/index.md`, `spec/mcp.md`, `spec/modes.md`,
  `spec/onboarding.md`, `spec/permissions.md`, `spec/shell-mode.md`, `spec/tty-less-support.md`, `spec/tui.md`,
  `spec/wire-format.md`, `spec/otlp-metrics.md`
  ([extensions/cli/spec listing](https://api.github.com/repos/continuedev/continue/contents/extensions/cli/spec)).
  **The existence of a `spec/` directory of behavioural specifications is itself notable design practice.**
- **Non-eval quality gating in the product itself:** `cn review` (with `--fail-fast`, `--review-agents`) and
  `cn checks` are the closest thing to automated quality gates
  ([extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts)).
- **Stale claim to discount:** `extensions/cli/AGENTS.md` states "No existing test files found - tests should be added
  when writing new functionality", which is contradicted by ~20 `.test.ts` files in the directory listing
  ([extensions/cli/AGENTS.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/AGENTS.md),
  [extensions/cli listing](https://api.github.com/repos/continuedev/continue/contents/extensions/cli)).
- Continue publishes **recommended models by role** rather than agent benchmark scores — a table of "Best open models /
  Best closed models" per role (Agent Plan, Chat Edit, Autocomplete, Apply, Embed, Rerank) with qualitative notes like
  "Closed models are slightly better than open models" ([models](https://docs.continue.dev/customize/models)).

## 2.19 Architecture

- **Repo layout:** top-level `.claude`, `.continue`, `.github`, `.husky`, `.idea`, `.vscode`, `actions`, `binary`,
  `core`, `docs`, `eval`, `extensions`, `gui`, `manual-testing-sandbox`, `media`, `packages`, `scripts`, `skills`,
  `sync` ([contents](https://api.github.com/repos/continuedev/continue/contents/)).
- `extensions/` contains exactly three entry points: `cli`, `intellij`, `vscode`
  ([extensions](https://api.github.com/repos/continuedev/continue/contents/extensions)).
- `packages/` holds the extracted npm packages: `config-types`, `config-yaml`, `continue-sdk`, `fetch`, `llm-info`,
  `openai-adapters`, **`terminal-security`** ([packages](https://api.github.com/repos/continuedev/continue/contents/packages)).
- `core/` internals: `autocomplete`, `codeRenderer`, `commands`, `config`, `context`, `continueServer`, `data`,
  `deploy`, `diff`, `edit`, `indexing`, `llm`, `nextEdit`, `promptFiles`, `protocol`, `tag-qry`, `tools`, `util`,
  `utils`, `vendor`, plus `core.ts` ([core listing](https://api.github.com/repos/continuedev/continue/contents/core)).
- **The agent core is shared across surfaces** — the CLI is described as the same agent as the IDE extensions
  ([CLI quickstart](https://docs.continue.dev/cli/quickstart), [TUI mode](https://docs.continue.dev/cli/tui-mode)).
- **Packaged-core model:** `binary/` bundles `core` with `esbuild` then `pkg` so it "can be run from any IDE or
  platform". The JetBrains extension spawns it as a subprocess and communicates over **stdin/stdout**, with an optional
  **TCP** mode (`useTcp = true` in `CoreMessenger.kt`) used for debugging
  ([binary/README.md](https://raw.githubusercontent.com/continuedev/continue/main/binary/README.md)).
- **IDE ⇄ core protocol** is a typed RPC surface in `core/protocol/` (`core.ts`, `coreWebview.ts`, `ide.ts`,
  `ideCore.ts`, `ideWebview.ts`, `webview.ts`, `passThrough.ts`, `util.ts`, plus a `messenger/` directory)
  ([core/protocol listing](https://api.github.com/repos/continuedev/continue/contents/core/protocol)).
  - `ToIdeFromWebviewOrCoreProtocol` (webview/core → IDE) is a method-name → [params, return] map including
    `getIdeInfo`, `getWorkspaceDirs`, `writeFile`, `removeFile`, `showVirtualFile`, `openFile`, `openUrl`, `runCommand`,
    `getSearchResults`, `getFileResults`, `subprocess`, `saveFile`, `fileExists`, `readFile`, `getProblems`,
    `getOpenFiles`, `getCurrentFile`, `getPinnedFiles`, `showLines`, `readRangeInFile`, `getDiff`,
    `getTerminalContents`, `getDebugLocals`, `getTopLevelCallStackSources`, `getAvailableThreads`,
    **`isTelemetryEnabled`**, `isWorkspaceRemote`, `getUniqueId`, `getTags`, `readSecrets`, `writeSecrets`,
    `getIdeSettings`, `getBranch`, `getRepoName`, `showToast`, `getGitRootPath`, `listDir`, `getFileStats`,
    `gotoDefinition`, `gotoTypeDefinition`, `getSignatureHelp`, `getReferences`, `getDocumentSymbols`, `reportError`,
    `closeSidebar`.
  - The reverse direction `ToWebviewOrCoreFromIdeProtocol` currently has **exactly one** message:
    `didChangeActiveTextEditor: [{ filepath: string }, void]`.
  - **`isTelemetryEnabled` being part of the IDE contract means the host IDE can veto telemetry.**
  (Source: [core/protocol/ide.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/protocol/ide.ts))
- The `core` package is standalone npm-installable (`core/index.d.ts` ~49 KB of exported types; `tsconfig.npm.json`,
  `.npmignore` present) and is consumed by the CLI as a `file:../../core` dependency
  ([core listing](https://api.github.com/repos/continuedev/continue/contents/core),
  [extensions/cli/package.json](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/package.json)).
- **The CLI does NOT use the IDE protocol:** it depends on `@continuedev/sdk`, `@continuedev/config-yaml`,
  `@continuedev/openai-adapters`, `@continuedev/terminal-security`, and `core`, and builds to a single bundled
  `dist/cn.js`. Runtime `dependencies` are minimal (`fdir`, `find-up`, `fzf`, `js-yaml`); everything else is a
  devDependency bundled by esbuild (`build.mjs`), with `validate-aliases.mjs` run first
  ([extensions/cli/package.json](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/package.json)).
- **CLI architecture components:** entry `src/index.ts` (three modes: headless / TUI / standard readline);
  `src/auth/` (`ensureAuth.ts`, `workos.ts`); `src/continueSDK.ts` (SDK client init: API key auth, slug-based assistant
  config, organization support); `src/ui/` (`TUIChat.tsx`, `UserInput.tsx`, `TextBuffer.ts`); `src/tools/` (file ops,
  code search, terminal execution, diff viewing, **"Exit tool (headless mode only)"**); `src/mcp.ts`; `src/hooks/`;
  `src/permissions/` (with `defaultPolicies.ts`); `src/subagent/`; `src/stream/`; `src/environment/`;
  `src/integration/`; `src/services/`; `src/commands/` (chat, checks, ls, review, serve)
  ([extensions/cli/AGENTS.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/AGENTS.md),
  [extensions/cli/src listing](https://api.github.com/repos/continuedev/continue/contents/extensions/cli/src)).
- There is **no maintained in-repo architecture prose doc**: CONTRIBUTING's table of contents still lists a
  "Continue Architecture" section whose body is absent from the current file, so the protocol source files are the
  authoritative description
  ([CONTRIBUTING.md](https://raw.githubusercontent.com/continuedev/continue/main/CONTRIBUTING.md)).
- VS Code dev model: run task `install-all-dependencies`, then "Launch extension" for a *Host VS Code* window.
  Breakpoints work in `core` and `extensions/vscode` but **not inside `gui`**; `gui` hot-reloads via Vite; changes to
  `core` or `extensions/vscode` require "Reload Window". Packaging: `npm run package` in `extensions/vscode` produces
  `extensions/vscode/build/continue-{VERSION}.vsix`
  ([CONTRIBUTING.md](https://raw.githubusercontent.com/continuedev/continue/main/CONTRIBUTING.md)).
- Required dev toolchain: **Node.js 20.20.1 (LTS) or higher**, `npm i -g vite`, Prettier with format-on-save
  ([CONTRIBUTING.md](https://raw.githubusercontent.com/continuedev/continue/main/CONTRIBUTING.md)).
- **Unverified:** per-directory licensing nuances beyond root Apache 2.0 (e.g. any non-Apache license for `gui/` or
  `sync/`); and the IDE↔core messaging transport details beyond the stdin/stdout + optional TCP model above.

## 2.20 Additional source-verified details the docs omit

These were found only by reading the frozen source, and they are the parts most likely to be missed by anyone
researching Continue from the docs alone.

**Blocks and composition** ([getBlockType.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/load/getBlockType.ts),
[schemas/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/index.ts)):

- "Continue uses a slug in the format of `owner/item-name` to resolve blocks" (e.g. `anthropic/claude-4-sonnet`),
  imported with `uses:` alongside `with:` (secrets/inputs) and `override:` (property overrides)
  ([configuring models/rules/tools](https://docs.continue.dev/guides/configuring-models-rules-tools)).
- **Block types are exactly:** `["models", "context", "data", "mcpServers", "rules", "prompts", "docs"]`
  (`BLOCK_TYPES`) — note `context` and `data` are blockable too, which the docs' examples do not show.
- A block is a `config.yaml`-shaped file containing **exactly one** item in one of those arrays; `blockSchema`
  intersects `baseConfigYamlSchema` with a union of objects each having `length(1)` in one array. Every block-able item
  also accepts the `{ uses, with?, override? }` wrapper (`blockItemWrapperSchema`, `blockOrSchema`).

**Workspace config semantics — union, not override**
([core/config/loadLocalAssistants.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/config/loadLocalAssistants.ts),
[core/config/yaml/loadYaml.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/config/yaml/loadYaml.ts)):

- Workspace config is **not** a second `config.yaml`. Workspace `.continue/<blockType>` directories are loaded **in
  addition to** global `~/.continue/<blockType>` — a **union, not an override**.
  `getDotContinueSubDirs()` pushes `joinPathsToUri(dir, ".continue", subDirName)` for each workspace dir and then
  `getGlobalFolderWithName(subDirName)`.
- Local blocks are injected via `injectBlocks: localPackageIdentifiers` and merged with
  `mergeUnrolledAssistants(config, unrolledLocal.config)`.
- Documented organisation dirs: `.continue/models`, `.continue/rules`, `.continue/mcpServers`.
- Legacy mechanisms still supported: **`.continuerc.json`** (same format as `config.json` plus a `mergeBehavior`
  property, `"merge"` default or `"overwrite"`) and **`~/.continue/config.ts`** exporting
  `modifyConfig(config: Config): Config`
  ([configuration deep dive](https://docs.continue.dev/customize/deep-dives/configuration)).
- **Config path resolution:** `getPrimaryConfigFilePath()` returns `config.yaml` if it exists, else `config.json`;
  `config.yaml` is auto-created from `defaultConfig` when absent or empty and is **chmod'd to `0o600` on non-Windows**
  ([core/util/paths.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/util/paths.ts)).
  The `0o600` default is a small but real security posture detail.

**Agent files — a packaged agent definition format (undocumented on the docs site)**
([packages/config-yaml/src/markdown/agentFiles.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/markdown/agentFiles.ts)):

- `parseAgentFile` parses markdown with frontmatter `{ name (required, min 1), description?, model?, tools?, rules? }`
  plus the markdown body as `prompt`. Missing `name` errors with: "Agent file must contain YAML frontmatter with a
  'name' field".
- **`tools` is a comma-separated string** (source TODO: "also accept yaml array") supporting:
  `owner/package` (all tools from an MCP server), `owner/package:tool_name`, `https://mcp.url.com` /
  `https://mcp.url.com:tool_name`, a bare `ToolName`/`tool_name` (built-in tool), and the special keyword **`built_in`**
  ("all built-in tools"). Whitespace in colon-separated references throws.
- `parseAgentFileRules` splits a comma-separated string into rule names.
- **Agent-config YAML files are recognised only under** `/.continue/agents/`, `/.continue/assistants/`, or
  `/.continue/configs/` (`isContinueAgentConfigFile`)
  ([loadLocalAssistants.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/config/loadLocalAssistants.ts)).
- The markdown utilities index exports `createMarkdownPrompt`, `createMarkdownRule`, `getRuleType`, `markdownToRule`,
  `agentFiles` ([markdown/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/markdown/index.ts)).

**Prompt file discovery**
([core/promptFiles/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/promptFiles/index.ts),
[getPromptFiles.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/promptFiles/getPromptFiles.ts)):

- Prompt dirs scanned: `.continue/prompts` (V2), `.continue/rules`, and the legacy **`.prompts`** (V1,
  `DEFAULT_PROMPTS_FOLDER_V1 = ".prompts"`), plus global `~/.continue/prompts` and `~/.continue/rules`.
- Accepted extensions: **`.prompt` and `.md`** — the `.prompt` extension is undocumented.
- `SUPPORTED_PROMPT_CONTEXT_PROVIDERS` (providers usable inside prompt files): `file`, `clipboard`, `repo-map`,
  `currentFile`, `os`, `problems`, `codebase`, `tree`, `open`, `debugger`, `terminal`, `diff`.
- Prompt blocks converted to slash commands get `source: "yaml-prompt-block"`
  ([promptBlockSlashCommand.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/commands/slash/promptBlockSlashCommand.ts)).

**Session storage on disk**
([core/util/paths.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/util/paths.ts),
[core/util/GlobalContext.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/util/GlobalContext.ts)):

- `getSessionsFolderPath()` → `<globalDir>/sessions` (created if missing); `getSessionFilePath(sessionId)` →
  `<globalDir>/sessions/<sessionId>.json`; `getSessionsListPath()` → `<globalDir>/sessions/sessions.json`,
  initialised to the literal contents `[]` if absent.
- So on disk: `~/.continue/sessions/` containing one `<sessionId>.json` per session plus `sessions.json` as the index.
  **There is no session list in `GlobalContext`** — sessions live in the sessions folder.
- Other persisted state: `~/.continue/index/globalContext.json`, `~/.continue/sharedConfig.json`, `~/.continue/.env`,
  `~/.continue/logs/{core,prompt}.log`, `~/.continue/index/{index.sqlite,lancedb,docs.sqlite,autocompleteCache.sqlite}`,
  `~/.continue/.diffs`.
- `GlobalContext` fields include `lastSelectedProfileForWorkspace`, `selectedModelsByProfileId`, `indexingPaused`,
  `mcpOauthStorage`, `sharedConfig`, `hasAlreadyCreatedAPromptFile`.
- Session-related UI settings live in `SharedConfig`: `showSessionTabs` and `disableSessionTitles` (applied to
  `config.ui.showSessionTabs` / `config.disableSessionTitles`) — **source evidence of an in-IDE session-tabs UI that no
  docs page describes**.

**CLI internals worth noting**
([extensions/cli/src/index.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts),
[configLoader.ts](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/configLoader.ts),
[extensions/cli/AGENTS.md](https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/AGENTS.md)):

- Hidden subcommands not in the docs nav: **`cn serve [prompt]`** (HTTP server with `/state` and `/message`,
  `--timeout` default 300, `--port` default 8000, `--id`, `--beta-upload-artifact-tool`), **`cn remote`**, and
  **`cn ls`** (session selector, `--json` shows 10 sessions). The `cn remote` ⇄ `cn serve` wire format is a **polling
  REST protocol** with a **500 ms client poll**, and the server binds to `127.0.0.1` with **no authentication**.
- `cn review` (AI code review with `--fix`, `--fail-fast`, `--review-agents`) and `cn checks` (PR CI status) are
  product-level quality gates beyond the agent loop.
- `--org <slug>` is "supported only in headless mode". `--beta-status-tool` and `--beta-subagent-tool` are declared but
  undocumented.
- `CONTINUE_CLI_DISABLE_COMMIT_SIGNATURE` disables the Continue commit signature in generated commit messages.
- CLI subsystems in the tree: `src/compaction.ts` (with `compaction.infiniteLoop.test.ts`,
  `compaction.pruneLastMessage.test.ts`), `src/permissions/` (`defaultPolicies.ts`), `src/subagent/`, `src/stream/`,
  `src/hooks/`, `src/commands/` (chat, checks, ls, review, serve)
  ([src listing](https://api.github.com/repos/continuedev/continue/contents/extensions/cli/src)).

---

# Part 3 — Cross-cutting comparison

| Domain | Cline | Continue |
|---|---|---|
| **Lifecycle** | Actively developed; harness re-architected to ClineCore/SDK; two harnesses coexist | **Repo read-only / unmaintained**; final 2.0.0 |
| **Agent loop** | `run()/continue()` → model → tool calls → results → repeat; `AgentRuntime` (`run`, `continue`, `abort`, `subscribe`, `restore`, `snapshot`) | Explicit 6-step handshake; permission step skippable via `Automatic` policy; errors fed back as context |
| **Stop conditions** | Explicit enum: `completed \| max_iterations \| aborted \| mistake_limit \| error`; `max_iterations`, `max_consecutive_mistakes`, `--timeout` | Not an explicit enum; implied by "model responds without a tool call" |
| **Parallel tool calls** | **Yes** — `enableParallelToolCalling`, model-family gated (GPT-5 always on; Gemini 3 toggle; Claude 4+ wording). MCP stays sequential | **Yes, read-only tools only** — stated in the agent system prompt; the docs' serial handshake is misleading |
| **Runtime topology** | **Hub-spoke**: hub daemon coordinates, spokes execute the loop, clients attach over WebSocket; sessions survive disconnects | In-process core; IDE spawns `binary/` over stdin/stdout (optional TCP); CLI does not use the IDE protocol |
| **Tool naming** | Current: `bash`, `editor`, `read_files`, `apply_patch`, `search`, `fetch_web`, `ask_question`; SDK variants `search_codebase`/`run_commands`/`fetch_web_content`; legacy XML names retired | IDE: snake_case (`read_file`, `edit_existing_file`); CLI: PascalCase (`Read`, `Edit`, `Write`, `Bash`, `MultiEdit`) |
| **Edit strategy** | `apply_patch` = V4A/unified diffs (3-line context, no line numbers); legacy `replace_in_file` = `------- SEARCH / ======= / +++++++ REPLACE` blocks | `create_new_file` / `edit_existing_file` whole-file oriented; separate **Apply model role** |
| **Diff review UI** | Inline diffs in VS Code/JetBrains; checkpoint Compare/Restore; Kanban inline line comments | `view_diff` tool; remote-only `/diff` + `/apply` in CLI |
| **Plan mode** | Plan/Act; Plan = no writes, no commands; per-mode model choice; `/deep-planning` → `implementation_plan.md` | Plan = read-only tool filter (11 read-only tools incl. `view_repo_map`, `view_diff`); `basePlanSystemMessage` |
| **Todo / focus chain** | **Focus Chain deprecated, no replacement** (was `task_progress` markdown checklist + `remind_cline_interval`) | None documented |
| **Checkpoints** | Shadow git per workspace at `<globalStorage>/checkpoints/{cwdHash}/.git`; commit per tool use; 3 restore modes; excludes build/media/config dirs; **no terminal-command state** | Not documented in the docs; `git diff` via `view_diff` / `GET /diff` |
| **Fork** | **No fork feature**; nearest = Restore Task Only, `/newtask`, Kanban resume ID | **Yes** — `/fork` and `--fork <sessionId>` |
| **Approval model** | IDE: 9 per-category auto-approve toggles + per-tool-call evaluation; model flags `requires_approval`; CLI: real `CLINE_COMMAND_PERMISSIONS` allow/deny JSON; YOLO mode | IDE: Ask First / Automatic / Excluded per tool (local per user) + **dynamic per-call re-evaluation**. CLI: `allow`/`ask`/`exclude` + `~/.continue/permissions.yaml` + argument globs; modes `--auto`/`--readonly` are absolute overrides |
| **Path containment** | Two-level: base toggle + "Read/Edit all files" to extend outside the workspace | **`evaluateFileAccessPolicy` always downgrades out-of-workspace paths to `allowedWithPermission`** regardless of base policy |
| **Dangerous-command handling** | Model-supplied `requires_approval` flag (heuristic, no fixed allowlist); CLI adds real allow/deny globs | **`@continuedev/terminal-security` package** with `evaluateTerminalCommandSecurity(basePolicy, command)` |
| **Secret file safety** | `.clineignore` explicitly NOT a security boundary; being deprecated | Hard-coded `FileIsSecurityConcern` blocklist of key/env/credential files+dirs in the indexer |
| **Hooks** | Full lifecycle (`TaskStart`, `TaskResume`, `TaskCancel`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`; `TaskComplete`/`PreCompact` coming soon); concurrent multi-hook, any-cancel-blocks; fail_open/fail_closed policies; 30s timeout; 50KB cap; **Windows unsupported** | **Yes, CLI only, undocumented on the site**: `~/.continue/settings.json` + `.claude/settings.json` etc.; **exit 2 = block**; Claude Code–compatible; `prompt`/`agent` hook types not yet implemented |
| **Rules** | `.clinerules/` (all `.md` + `.txt`), plus `.cursorrules`, `.windsurfrules`, `AGENTS.md`, `~/.agents/AGENTS.md`; conditional `paths` globs; workspace beats global | `.continue/rules/*.md` with `globs`/`regex`/`alwaysApply`/`description`; lexicographic order; **`AGENTS.md`/`AGENT.md`/`CLAUDE.md` supported in source (forced `alwaysApply`)**; also `.continuerules` and colocated `rules.md`; docs omit all of it |
| **Skills** | Yes — `SKILL.md`, progressive loading (~100 tok metadata / <5k instructions), `use_skill`; **global beats project on name collision** | Not a concept; nearest is prompt files with `invokable: true` |
| **Ignore rules** | `.clineignore` — deprecating; not a boundary; 200k→50k token claim | `.continueignore` + `.gitignore` + large hard-coded security blocklist |
| **Indexing / RAG** | None documented (agent uses `search`); Cline instead clamps oversized tool output (50K→8K) | LanceDB + local `transformers.js` (`all-MiniLM-L6-v2`), SQLite catalog + FTS5 + chunk + snippets indexes; **deprecated** in favor of tools |
| **MCP** | `mcpServers`; CLI `~/.cline/mcp.json`, IDE panel, `cline_mcp_settings.json`; `autoApprove` array per server; omitting `type` defaults to legacy `sse` | `mcpServers` in `config.yaml` or `.continue/mcpServers/*.yaml`; stdio/sse/streamable-http; ingests other tools' JSON configs; `/mcp` menu |
| **MCP marketplace** | **Yes** — official, one-click install driven by the server README + `llms-install.md` | None |
| **Extensibility API** | Plugins (tools, hooks, slash commands, rules, events) with `cline.plugins` manifest + host-provided `@cline/*` deps; SDK/CLI/Kanban only | MCP only; `uses:` config blocks; no in-process tool API |
| **Subagents** | `use_subagents` tool, parallel, read-only by construction, separate token budget, per-subagent cost rolled up | Undocumented; `subagent` **model role** + `--beta-subagent-tool` + `src/subagent/` exist in source |
| **Teams / parallel orchestration** | Agent Teams (coordinator, `task-board.json` + `mailbox.json` + `mission-log.json`) and Kanban (git worktree per card, dependency chains, auto-commit/auto-PR, multi-vendor) | None |
| **Scheduling** | `cline schedule` + cron, hub-backed, persists across restarts, results routed to chat via delivery adapters | None |
| **Chat connectors** | Telegram, Slack, Discord, Google Chat, WhatsApp, Linear | None |
| **Cost display** | Task header (updates per request), TUI status area, per-subagent, schedule history, `task.tokens` OTel event | `/info` (token usage + cost), `continue_cli_cost_usage` metric |
| **Token accounting granularity** | `total_cost`, `tokens_in`, `tokens_out`, **`cache_writes`**, `cache_reads` | `continue_cli_token_usage` with `input\|output\|cacheRead\|cacheCreation` |
| **Context condensation** | Auto Compact (summarisation replacing history) + legacy truncation with 30% dedup short-circuit, half/quarter removal, tool_use/tool_result repair; per-provider buffers (64k→−27k, 128k→−30k, 200k→−40k) | `/compact`; `src/compaction.ts` |
| **Headless** | `cline --json` (NDJSON), implicit on piped stdin/redirected output, JSON output schema docs; **exit codes undocumented** | `cn -p`/`--print`, stdin wrapped in `<stdin>` tags, `--silent`, `--format json`; **exit 130 on interrupt**, exit 1 for missing prompt |
| **Observability** | OTel metrics + logs (gRPC/HTTP), Remote-Configuration-driven, **no distributed tracing**; rich `task.*` event taxonomy; hashed paths; enterprise S3/R2 prompt storage | OTel metrics (spec + source), Claude Code–compatible metric names, opt-in via OTLP env; `~/.continue/logs/cn.log`; `data` export to HTTP/`.jsonl`; telemetry reportedly removed in 2.0.0 |
| **Eval / benchmarks** | **Real harness** (`evals/` with smokes, e2e, `cline-bench` submodule, pass@k/pass^k/flakiness); Terminal Bench 47%→57%; published traces | **None** — `eval/` contains only a `.gitignore` with `repos`; unit/functional/e2e tests only |
| **Test/spec practice** | Contract + smoke + e2e layers; nightly E2E not yet implemented; PR gate is contract tests only | A **`spec/` directory** of behavioural specifications (13 documents) is the standout |
| **Provider abstraction** | `@cline/llms` gateway + `registerProvider`/`registerModel`; official "OpenAI Compatible" provider with per-model price fields; 30+ providers; Ollama/LM Studio/Atomic Chat | Model **roles** bind providers to capabilities; `provider: openai` + `apiBase`; canonical provider list; Ollama/LM Studio |
| **Config location** | `~/.cline/` (global) + `.cline/` (project); `providers.json`, `global-settings.json`, `cline_mcp_settings.json` | `~/.continue/config.yaml`; `${{ secrets.X }}` resolved from `.env` files in a documented order |
| **Config schema source of truth** | Protobuf settings schema (`proto/cline/state.proto`, field numbers) | Zod schema (`packages/config-yaml/src/schemas/index.ts`) |
| **License** | Apache 2.0 | Apache 2.0 (CLA required for contributions) |

---

# Part 4 — Gaps and unverified items (consolidated)

**Cline**

- The **default value** of `auto_condense_threshold`, and whether Auto Compact is on by default in current releases
  (PR #12739 title suggests yes; PR fetch failed with HTTP 403).
- The **literal OS path** of VS Code's `globalStorageFsPath`, and therefore the absolute checkpoints/tasks directories
  on disk.
- Whether **prompt caching** is supported on a per-provider matrix; only Anthropic and the two explicit proto toggles
  (Bedrock, LiteLLM) are attested.
- Whether the **browser tool** still exists as a distinct tool in the current harness or was folded into
  `fetch_web`/`fetch_web_content` (only "Use the browser" survives, as a permission category).
- **MCP config path** is inconsistent across four official statements
  (`.cline/mcp.json`, `~/.cline/mcp.json`, `~/.cline/data/settings/cline_mcp_settings.json`).
- **MCP server control key names** in enterprise remote config (page not fetched).
- **Exit codes** for the Cline CLI: no exit-code table exists in the CLI reference, overview, or README.
- Whether **`.clinerules` as a single file** (as opposed to a folder) is supported — not mentioned in current docs.
- Precedence/ordering **between** rule types (Cline vs Cursor vs Windsurf vs AGENTS.md).
- The exact mechanism by which rule text is **injected into the system prompt** (prompt assembly moved into the SDK;
  empty listing at `apps/vscode/src/core/prompts/`).
- Any documented **file-read truncation limit** (character or line cap) for `read_file`.
- Whether **plan mode genuinely cannot write files** in all cases — docs assert it; open GitHub issues (search results
  only, not fetched) report violations.
- No **"fork task"** feature found in docs, redirects, or the settings proto.
- No **"read-only mode"** feature by that name (closest: Plan mode, disabled auto-approve,
  `toolPolicies {enabled:false}`, read-only subagents).
- The **Focus Chain reminder interval** discrepancy: source snapshot says every 10th API request; the official blog says
  every 6 messages.
- No **SWE-bench Pro** results on any primary Cline source; the reported benchmark is Terminal Bench.
- **Content checks:** PR #7340 and commit `febeb77` on Focus Chain prompting were seen in search results only, not
  fetched. The `cline.bot/blog/...` pages are client-rendered; the `cline.ghost.io` mirror was used instead.

**Continue**

*Resolved during research (previously listed here, now answered):*
- ~~AGENTS.md read support~~ → **RESOLVED: implemented.** `SUPPORTED_AGENT_FILES = ["AGENTS.md","AGENT.md","CLAUDE.md"]`
  at workspace root, forced `alwaysApply: true` (§2.7).
- ~~MCP in Plan mode~~ → **RESOLVED: MCP tools ARE available in Plan mode**, enforced only by prompt instruction
  (§2.8).
- ~~Parallel tool calls~~ → **RESOLVED: supported for read-only tools** by explicit system-prompt instruction (§2.3).
- ~~No path allowlist / no dangerous-command detection~~ → **RESOLVED: both exist** —
  `evaluateFileAccessPolicy` downgrades out-of-workspace paths, and `evaluateTerminalCommandSecurity` ships in the
  dedicated `@continuedev/terminal-security` package (§2.8).
- ~~`allowAnonymousTelemetry` setting id unverified~~ → **RESOLVED:** it is a real `SharedConfig` key (§2.8).
- ~~Subagents undocumented~~ → **PARTLY RESOLVED:** a `subagent` **model role** exists in the schema and
  `--beta-subagent-tool` / `src/subagent/` exist in the CLI, but user-facing behaviour remains undocumented (§2.11).

*Still open:*
- Whether the **local embedding index** is still built/used by default in 2.0.0 given `@Codebase` deprecation (the FAQ's
  index-rebuild and LanceDB references suggest the machinery remains).
- Whether **`.continueignore`** still has effect post-`@Codebase` deprecation.
- **Headless `ask`-tool behaviour** — excluded (docs) vs process error exit (spec).
- **`config.yaml` permissions block** — spec says "implement later"; published precedence list ranks it above
  `permissions.yaml`.
- **`cn serve` default port** — 8000 (source) vs 3000 (spec).
- **Telemetry presence and opt-out** — README says 2.0.0 removed anonymous telemetry, but `telemetry.mdx` documents
  PostHog and an opt-out whose env var (`CONTINUE_TELEMETRY_ENABLED`) does not appear in `telemetryService.ts` (which
  uses `CONTINUE_METRICS_ENABLED` / `CONTINUE_CLI_ENABLE_TELEMETRY`); `OTEL_METRIC_EXPORT_INTERVAL` defaults differ
  (60000 spec vs 20000 source).
- **`CONTINUE_ORGANIZATION_ID`** — no primary source; the CLI uses `--org` and `process.env.ORGANIZATION_ID`.
- The schema of **`--format json`** output — undocumented in every source fetched.
- **Secret resolution order for MCP** (org secrets → process.env → `~/.continue/.env` → workspace `.continue/.env` →
  workspace `.env`) is **the reverse** of the documented order for model secrets (workspace `.env` → workspace
  `.continue/.env` → `~/.continue/.env` → process.env).
- A dedicated **vLLM provider page** — 404; vLLM is supported only via the OpenAI-compatible `apiBase` route.
- `/cli/authentication` and `/cli/reference` — do not exist.
- **Custom TypeScript context providers** — the docs no longer document this; customization is redirected to MCP.
- **Stop conditions** and **streaming specifics** for the Continue agent loop.
- Whether the **CLI writes sessions** to the shared `~/.continue/sessions` directory (the CLI has its own
  `src/session.ts`); and whether **IDE extensions expose session list/fork UI** comparable to the CLI
  (`showSessionTabs` exists in source but is undocumented).
- **Built-in IDE slash commands** — `/edit` and `/comment` could not be confirmed in *any* current primary source;
  `/share`, `/cmd`, `/commit`, `/http`, `/issue`, `/onboard` appear **only** on the explicitly deprecated `config.json`
  page, so their continued existence is unverified.
- **`AGENT.md` / `CLAUDE.md`** appear unreachable in practice due to a `break` placement in
  `loadMarkdownRules.ts` (code reading, not documented).
- **Per-directory licensing** nuances beyond root Apache 2.0.
- `extensions/cli/AGENTS.md` claiming "No existing test files found" is **stale** (contradicted by ~20 `.test.ts`
  files).
- The bodies of issues/PRs **#6716, #9308, #5058, #9128, #4274, #8644** could not be fetched (github.com HTML fetch
  failed / API 403) — cited as search-result titles only.

---

# Part 5 — Source index

**Cline (official docs, `docs.cline.bot`)**
<https://docs.cline.bot/cline-overview> · <https://docs.cline.bot/tools-reference/all-cline-tools> ·
<https://docs.cline.bot/core-workflows/plan-and-act> · <https://docs.cline.bot/core-workflows/checkpoints> ·
<https://docs.cline.bot/core-workflows/using-commands> · <https://docs.cline.bot/core-workflows/working-with-files> ·
<https://docs.cline.bot/core-workflows/task-management> · <https://docs.cline.bot/features/auto-approve> ·
<https://docs.cline.bot/features/auto-compact> · <https://docs.cline.bot/features/subagents> ·
<https://docs.cline.bot/customization/cline-rules> · <https://docs.cline.bot/customization/clineignore> ·
<https://docs.cline.bot/customization/skills> · <https://docs.cline.bot/customization/plugins> ·
<https://docs.cline.bot/customization/hooks> · <https://docs.cline.bot/mcp/mcp-overview> ·
<https://docs.cline.bot/getting-started/config> · <https://docs.cline.bot/best-practices/memory-bank> ·
<https://docs.cline.bot/resources/deprecations> · <https://docs.cline.bot/usage/tui> ·
<https://docs.cline.bot/usage/cli-overview> · <https://docs.cline.bot/cli/cli-reference> ·
<https://docs.cline.bot/cli/agent-teams> · <https://docs.cline.bot/cli/scheduling> ·
<https://docs.cline.bot/cli/connectors> · <https://docs.cline.bot/usage/acp> · <https://docs.cline.bot/usage/kanban> ·
<https://docs.cline.bot/provider-config/openai-compatible> ·
<https://docs.cline.bot/provider-config/other-30-plus-providers> ·
<https://docs.cline.bot/enterprise-solutions/monitoring/opentelemetry> ·
<https://docs.cline.bot/enterprise-solutions/configuration/infrastructure-configuration/control-other-cline-features/mcp-server-controls>

**Cline (SDK docs)**
<https://docs.cline.bot/sdk/overview> · <https://docs.cline.bot/sdk/architecture/overview> ·
<https://docs.cline.bot/sdk/architecture/hub-spoke> · <https://docs.cline.bot/sdk/guides/building-an-agent> ·
<https://docs.cline.bot/sdk/guides/permission-handling> · <https://docs.cline.bot/sdk/events> ·
<https://docs.cline.bot/sdk/model-providers> · <https://docs.cline.bot/sdk/plugins>

**Cline (raw docs source / repo)**
<https://raw.githubusercontent.com/cline/cline/main/docs/docs.json> ·
<https://raw.githubusercontent.com/cline/cline/main/README.md> ·
<https://raw.githubusercontent.com/cline/cline/main/AGENTS.md> ·
<https://raw.githubusercontent.com/cline/cline/main/.clinerules/hooks/README.md> ·
<https://raw.githubusercontent.com/cline/cline/main/evals/README.md> ·
<https://raw.githubusercontent.com/cline/cline/main/apps/cli/README.md> ·
<https://raw.githubusercontent.com/cline/cline/main/docs/features/auto-compact.mdx> ·
<https://raw.githubusercontent.com/cline/cline/main/docs/customization/clineignore.mdx> ·
<https://raw.githubusercontent.com/cline/cline/main/docs/customization/hooks.mdx> ·
<https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/opentelemetry-events.mdx> ·
<https://raw.githubusercontent.com/cline/cline/main/docs/enterprise-solutions/monitoring/telemetry.mdx> ·
<https://raw.githubusercontent.com/cline/cline/main/docs/sdk/guides/multi-agent-teams.mdx> ·
<https://raw.githubusercontent.com/cline/cline/main/docs/kanban/core-workflow.mdx> ·
<https://raw.githubusercontent.com/cline/cline/main/docs/cli/samples/github-integration.mdx> ·
<https://raw.githubusercontent.com/cline/cline/main/docs/cli/samples/model-orchestration.mdx> ·
<https://raw.githubusercontent.com/cline/cline/main/.clinerules/workflows/release.md> ·
<https://api.github.com/repos/cline/cline/contents/.clinerules> ·
<https://api.github.com/repos/cline/cline/contents/evals>

**Cline (legacy harness, pinned commit `be7548ff`)**
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/state.proto> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/proto/cline/task.proto> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointTracker.ts> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointUtils.ts> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/integrations/checkpoints/CheckpointExclusions.ts> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/context/context-management/ContextManager.ts> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/context/context-management/context-window-utils.ts> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/storage/disk.ts> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/anthropic_claude_sonnet_4-basic.snap> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/__tests__/__snapshots__/old-generic-with-focus.snap> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/components/task_progress.ts> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/components/act_vs_plan_mode.ts> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/components/cli_subagents.ts> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/tools/focus_chain.ts> ·
<https://raw.githubusercontent.com/cline/cline/be7548fff1012b8aa9caf6df4ba24dc85fce55d7/src/core/prompts/system-prompt/tools/apply_patch.ts> ·
<https://raw.githubusercontent.com/cline/cline/9dea336c/docs/customization/workflows.mdx> ·
<https://raw.githubusercontent.com/cline/cline/9dea336c/docs/cline-cli/three-core-flows.mdx> ·
<https://github.com/cline/cline/commit/6d1bfc3a1ba46d5bf762600d4fdead01e5526f48.patch>

**Cline (official blog)**
<https://cline.ghost.io/open-sourcing-evals-for-open-weight-agents/> ·
<https://cline.ghost.io/a-practical-guide-to-hill-climbing/> ·
<https://cline.ghost.io/how-to-think-about-context-engineering-in-cline/> ·
<https://cline.ghost.io/how-we-migrated-11-million-users-to-clines-biggest-harness-upgrade/> ·
<https://cline.ghost.io/introducing-the-mcp-marketplace-clines-new-app-store/> ·
<https://cline.bot/blog/llm-benchmarks> · <https://cline.bot/mcp-marketplace> ·
<https://github.com/cline/mcp-marketplace> · <https://github.com/cline/cline-bench> ·
<https://cline-open-eval-ledger.netlify.app/>

**Continue (official docs)**
<https://docs.continue.dev/> · <https://docs.continue.dev/reference> ·
<https://docs.continue.dev/reference/yaml-migration> · <https://docs.continue.dev/reference/deprecated-codebase> ·
<https://docs.continue.dev/reference/deprecated-context-providers> ·
<https://docs.continue.dev/ide-extensions/agent/quick-start> ·
<https://docs.continue.dev/ide-extensions/agent/how-it-works> ·
<https://docs.continue.dev/ide-extensions/agent/plan-mode> ·
<https://docs.continue.dev/ide-extensions/agent/how-to-customize> ·
<https://docs.continue.dev/customize/models> · <https://docs.continue.dev/customize/rules> ·
<https://docs.continue.dev/customize/prompts> · <https://docs.continue.dev/customize/mcp-tools> ·
<https://docs.continue.dev/customize/deep-dives/rules> · <https://docs.continue.dev/customize/deep-dives/prompts> ·
<https://docs.continue.dev/customize/deep-dives/mcp> ·
<https://docs.continue.dev/customize/deep-dives/custom-providers> ·
<https://docs.continue.dev/customize/deep-dives/development-data> ·
<https://docs.continue.dev/customize/model-roles/embeddings> ·
<https://docs.continue.dev/customize/model-providers/top-level/openai> ·
<https://docs.continue.dev/customize/model-providers/top-level/ollama> ·
<https://docs.continue.dev/customize/model-providers/top-level/lmstudio> ·
<https://docs.continue.dev/customize/model-providers/overview> ·
<https://docs.continue.dev/guides/understanding-configs> ·
<https://docs.continue.dev/guides/codebase-documentation-awareness> ·
<https://docs.continue.dev/guides/custom-code-rag> · <https://docs.continue.dev/guides/ollama-guide> ·
<https://docs.continue.dev/guides/how-to-self-host-a-model> ·
<https://docs.continue.dev/guides/running-continue-without-internet> ·
<https://docs.continue.dev/cli/quickstart> · <https://docs.continue.dev/cli/tui-mode> ·
<https://docs.continue.dev/cli/headless-mode> · <https://docs.continue.dev/cli/configuration> ·
<https://docs.continue.dev/cli/tool-permissions> · <https://docs.continue.dev/faqs> ·
<https://docs.continue.dev/troubleshooting>

**Continue (repo, raw source)**
<https://raw.githubusercontent.com/continuedev/continue/main/README.md> ·
<https://raw.githubusercontent.com/continuedev/continue/main/CONTRIBUTING.md> ·
<https://raw.githubusercontent.com/continuedev/continue/main/binary/README.md> ·
<https://raw.githubusercontent.com/continuedev/continue/main/eval/.gitignore> ·
<https://raw.githubusercontent.com/continuedev/continue/main/.continueignore> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/indexing/README.md> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/indexing/continueignore.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/indexing/ignore.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/util/paths.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/protocol/ide.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/index.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/README.md> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/AGENTS.md> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/package.json> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/index.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/shared-options.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/configLoader.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/env.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/util/logger.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/src/telemetry/telemetryService.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/permissions.md> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/modes.md> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/mcp.md> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/wire-format.md> ·
<https://raw.githubusercontent.com/continuedev/continue/main/extensions/cli/spec/otlp-metrics.md> ·
<https://raw.githubusercontent.com/continuedev/continue/main/docs/docs.json> ·
<https://raw.githubusercontent.com/continuedev/continue/main/docs/customize/telemetry.mdx> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/llm/defaultSystemMessages.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/llm/toolSupport.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/llm/rules/getSystemMessageWithRules.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/llm/rules/constants.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/config/markdown/loadMarkdownRules.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/config/markdown/loadMarkdownSkills.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/config/markdown/loadCodebaseRules.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/config/getWorkspaceContinueRuleDotFiles.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/config/loadLocalAssistants.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/config/yaml/loadYaml.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/config/sharedConfig.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/promptFiles/index.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/promptFiles/getPromptFiles.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/commands/slash/promptBlockSlashCommand.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/tools/builtIn.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/tools/index.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/tools/callTool.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/tools/policies/fileAccess.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/tools/definitions/readFile.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/tools/definitions/createNewFile.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/tools/definitions/runTerminalCommand.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/tools/definitions/searchWeb.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/core/util/GlobalContext.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/models.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/mcp/index.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/schemas/policy.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/markdown/agentFiles.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/markdown/index.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/packages/config-yaml/src/load/getBlockType.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/main/packages/terminal-security/src/types.ts> ·
<https://raw.githubusercontent.com/continuedev/continue/4bc0a84453a95c7d582076305907e56b28ef7b8c/example-permissions.yaml> ·
<https://github.com/continuedev/continue/issues/6716>

## Appendix — research method notes

- **Fetch technique that unlocked the Cline docs:** Mintlify serves raw markdown at
  `https://docs.cline.bot/<path>.md`, and the docs source is in-repo, so
  `https://raw.githubusercontent.com/cline/cline/main/docs/<path>.mdx` is a stable, timeout-free twin.
- **Blog mirror:** `cline.bot/blog/<slug>` is client-rendered and returns only a nav shell; the server-rendered mirror
  is `https://cline.ghost.io/<slug>/`.
- **Legacy Cline harness:** the pre-ClineCore source (`src/`, `proto/cline/*.proto`, prompt-registry snapshots) is
  removed from `main` but fully retrievable at commit `be7548fff1012b8aa9caf6df4ba24dc85fce55d7`. That commit is the
  richest primary source for the original tool contracts, the `replace_in_file` format, context-window math, and
  checkpoint internals.
- **Continue:** `raw.githubusercontent.com/.../main/<path>` gives full file contents; the GitHub contents API was used
  for directory listings but returned **HTTP 403 rate-limit** partway through. Several `github.com` HTML fetches
  (issue/PR bodies) failed outright and are cited as search-result titles only.
- **No third-party sources were used.** Aggregators surfaced in search results were never fetched or cited.
