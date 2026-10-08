# shell 与一等专用工具：各家 coding agent 的实际做法

## 调研问题与范围

**问题**：主流 LLM CLI harness 在工具设计上，优先给模型「领域专用的一等工具」（`grep`/`glob`/`read_file`/`web_search`/`lsp` 这类），还是只给一条万能 shell？它们具体怎么引导模型选哪条路？其中最关键的一条是：**有没有 agent 在 system prompt 或工具描述里明确劝阻「用 shell 拼等价命令来代替专用工具」**，原文怎么措辞。

**调研日期**：2026-10-08。

**方法与 source 类型**：只用一手来源——

- 官方文档站：Claude Code docs（`code.claude.com/docs`）、Codex 官方文档（`learn.chatgpt.com/docs`，由 `developers.openai.com/codex/*` 跳转）、Cline 文档站、opencode 文档站；
- 上游源码仓库本身的文件（`raw.githubusercontent.com` 上的分支路径；能拿到 commit hash 的写成 permalink）；
- 项目自带的工具描述源码（工具声明本身就是被调研的对象）。

**没有采用的**：blog 解读、对比文章、教程、新闻稿、DeepWiki / 第三方 prompt 收集站。凡是在这类二手材料里出现、但本次没拿到一手来源的事实，一律在文末「未能追溯」一节列出，不写进正文。

**覆盖的 agent**：Claude Code、Codex CLI、Gemini CLI、opencode、goose、Cline，共 6 家。未覆盖：Amp、Continue、aider、OpenHands、mini-SWE-agent（这几家在仓库既有笔记里已有结论，本次没有重新做一手核对）。

**与既有材料的关系**：本文件是对 [`../coding-agent-features.md`](../coding-agent-features.md) 第 2 节（工具集合与编辑策略）、第 5 节（权限与安全）的补充与加深，**不是重写**。既有笔记（`cline-continue.md`、`opencode-goose.md` 等）里已经核对过的编辑格式、降级匹配、compaction 等内容这里不重复。凡与既有结论冲突的地方，在下面第 0 节单列。

---

## 0. 与既有结论的冲突与修正

### 0.1 Claude Code：默认**不给** `Grep` / `Glob`（既有结论要加限定）

既有总表把 Claude Code 记作「专用 read/grep/glob 工具 ✅」。官方工具参考里有一句直接推翻这个前提：

> "On macOS, Linux, and WSL, Claude Code leaves Glob and Grep out of the default tool set, and Claude searches with `find` and `grep` through the Bash tool instead. In Claude's shell those two commands run embedded versions of `bfs` and `ugrep`, and the searches reach your hooks and permission rules as `Bash` calls."

也就是说，在最主流的三个桌面平台上，Claude Code 的默认形态是**让模型用 shell 做文件查找与内容搜索**，专用工具要用户显式要回来：在启动时用 `--tools` / `--allowedTools` 点名 `Glob` 或 `Grep`、用 `deny` 规则或 `--disallowedTools` / `--restricted` 把 `Bash` 整个去掉、或者让某个 subagent 在 `tools` 里列上它并去掉 `Bash`。（[tools-reference](https://code.claude.com/docs/en/tools-reference)）

同一个页面还有两处配套事实：权限说明里 `Bash` 虽标为 "Permission required: Yes"，但 "runs a built-in set of read-only commands without prompting"；而 Bash 的 exit 1 语义白名单里点名了 `grep`、`rg`、`egrep`、`fgrep`、`find`、`diff`、`test`、`[`、`git diff`、`git grep`——**在语义层面，shell 搜索是被官方特殊照顾的一类**。（[tools-reference](https://code.claude.com/docs/en/tools-reference)）

### 0.2 Codex：默认引导仍是 shell，但工具箱里已出现专用搜索工具

既有结论说「Codex 核心工具集里没有 `read_file` / `write_file` / `grep` / `glob`」。这条现在需要改写成两半：

- **默认引导仍然是 shell**。Codex 的 base instructions 明确写着（[default.md](https://raw.githubusercontent.com/openai/codex/main/codex-rs/protocol/src/prompts/base_instructions/default.md)，分支路径）：

  > "## Shell commands
  >
  > When using the shell, you must adhere to the following guidelines:
  > - When searching for text or files, prefer using `rg` or `rg --files` respectively because `rg` is much faster than alternatives like `grep`. (If the `rg` command is not found, then use alternatives.)
  > - Do not use python scripts to attempt to output larger chunks of a file."

- **但代码里已经有专用搜索工具**。`codex-rs/core/src/tools/spec.rs` 在 commit `d807d44a` 时注册的 handler 列表里包含 `ReadFileHandler`、`GrepFilesHandler`、`ListDirHandler`、`ViewImageHandler`（[spec.rs permalink](https://raw.githubusercontent.com/openai/codex/d807d44ae7fb69e8e05fc6e6fddea65f7e9421f5/codex-rs/core/src/tools/spec.rs)）。`grep_files` 的实现也拿到了（[grep_files.rs permalink](https://raw.githubusercontent.com/openai/codex/d807d44ae7fb69e8e05fc6e6fddea65f7e9421f5/codex-rs/core/src/tools/handlers/grep_files.rs)）。

  它 spawn 的是真 `rg`：`rg --files-with-matches --sortr=modified --regexp <pattern> --no-messages`，可选 `--glob <include>`，**30 秒超时**，超出时限的报错是 `"rg timed out after 30 seconds"`。

  **未能确认**：`grep_files` / `read_file` 的注册条件（是否默认开启、受哪个 feature flag 控制）—— `spec.rs` 超过十万字符，抓取被截断，注册段落没读到。见文末「未能追溯」。

### 0.3 Cline 的工具名已经变了

既有笔记里的 Cline 工具名对应的是旧架构。新文档里 `ClineCore` 的内建工具是（[all-cline-tools](https://docs.cline.bot/tools-reference/all-cline-tools)）：

| 工具 | 文档给的描述 |
| --- | --- |
| `read_files` | Read one or more files |
| `search_codebase` | Search the workspace |
| `run_commands` | Execute shell commands |
| `fetch_web_content` | Fetch web content |
| `apply_patch` | Apply patch/diff edits |
| `editor` | Edit files |
| `skills` | Invoke configured skills |
| `ask_question` | Ask the user for input |
| `submit_and_exit` | Submit a final answer and stop |

同一个 session 提供 `editor` 或 `apply_patch`，**不同时提供**；Act 模式默认对 GPT / Codex 模型以及 `openai-native` provider 的所有模型给 `apply_patch`，其他模型给 `editor`，Plan 模式两者都不给。（[all-cline-tools](https://docs.cline.bot/tools-reference/all-cline-tools)）

---

## 1. 谁给了哪些领域专用的一等工具，参数面长什么样

### 1.1 Claude Code

工具清单里与本题相关的有 `Read`、`Write`、`Edit`、`Glob`、`Grep`、`Bash`、`WebFetch`、`WebSearch`、`LSP`、`NotebookEdit`（[tools-reference](https://code.claude.com/docs/en/tools-reference)）。从文档能确认的参数面：

- `Grep`：基于 ripgrep，用 ripgrep 正则语法而非 POSIX；三种 `output_mode`——`files_with_matches`（默认，只给路径）、`content`（给行内容与行号）、`count`（每文件计数 + 一个总数，且**总数覆盖被 `head_limit` / `offset` 截断掉的命中**）；可按文件用 `glob` 收窄（如 `**/*.tsx`）、按语言用 `type`（如 `py`、`rust`）；默认单行匹配，可设 `multiline: true`；遵守 `.gitignore`。
- `Glob`：标准 glob 语法（`**`、`src/**/*.ts`、`*.{json,yaml}`）；结果**按修改时间排序，上限 100 个文件**，命中上限时"Claude sees a truncation flag in the result"；**默认不遵守 `.gitignore`**（要遵守得设 `CLAUDE_CODE_GLOB_NO_IGNORE=false`）——这与 `Grep` 相反。
- `Read`：`offset` / `limit`；整文件超过 token 上限时返回首页 + `PARTIAL view` 提示并说明怎么续读。

### 1.2 Codex CLI

默认形态是 shell（`shell` / `shell_command`）+ freeform 的 `apply_patch` + `update_plan`（[default.md](https://raw.githubusercontent.com/openai/codex/main/codex-rs/protocol/src/prompts/base_instructions/default.md)）。`apply_patch` 的调用样例直接写在 base instructions 里：

> "Use the `apply_patch` tool to edit files (NEVER try `applypatch` or `apply-patch`, only `apply_patch`): {"command":["apply_patch","*** Begin Patch\\n*** Update File: path/to/file.py\\n@@ def example():\\n- pass\\n+ return 123\\n*** End Patch"]}"

新增的 `grep_files` 参数面（[grep_files.rs permalink](https://raw.githubusercontent.com/openai/codex/d807d44ae7fb69e8e05fc6e6fddea65f7e9421f5/codex-rs/core/src/tools/handlers/grep_files.rs)）：

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `pattern` | 是 | 不能为空，否则报 `"pattern must not be empty"` |
| `include` | 否 | 转成 `rg --glob` |
| `path` | 否 | 相对 turn 的 cwd 解析；路径不存在时先报错 |
| `limit` | 否 | 默认 **100**，实现里 `args.limit.min(MAX_LIMIT)` 且 `MAX_LIMIT = 2000`；传 0 会被拒 |

### 1.3 Gemini CLI

工具声明集中在 `definitions/model-family-sets/default-legacy.ts`（[default-legacy.ts](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/tools/definitions/model-family-sets/default-legacy.ts)，分支路径），参数名有常量表钉住：

- `read_file`：`absolute_path`（必填）、`start_line`、`end_line`。描述里承诺支持图片、音频（`MP3`/`WAV`/`AIFF`/`AAC`/`OGG`/`FLAC`）、PDF，并说明超长时会截断并告知怎么用 `start_line` / `end_line` 续读。
- `grep_search`：`pattern`（必填）、`dir_path`、`include_pattern`、`exclude_pattern`、`names_only`、`max_matches_per_file`、`total_max_matches`。**工具描述只有一句**：`'Searches for a regular expression pattern within file contents. Max 100 matches.'`
- `grep_search_ripgrep`（另一套声明，同一个工具名）：参数面大得多——多出 `case_sensitive`、`fixed_strings`、`context`（等价 `grep -C`）、`after`（`grep -A`）、`before`（`grep -B`）、`no_ignore`，描述里直接写 "By default, treated as a Rust-flavored regular expression"。
- `glob`：`pattern`（必填）、`dir_path`、`case_sensitive`、`respect_git_ignore`（默认 true，仅在 git 仓库里有效）、`respect_gemini_ignore`。描述承诺返回**按修改时间排序的绝对路径**（新到旧）。
- `list_directory`：`dir_path`（必填）、`ignore`、`file_filtering`（内含 `respect_git_ignore` / `respect_gemini_ignore`）。
- `run_shell_command`：`command`（必填）、`description`、`dir_path`、`is_background`、`delay_ms`；开启 tool sandboxing 时还多一个 `additional_permissions`（`network` 布尔 + `fileSystem.read` / `fileSystem.write` 路径数组），描述写明用途是"请求额外的沙箱权限以应对上一次的 `Operation not permitted`"。（[dynamic-declaration-helpers.ts](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/tools/definitions/dynamic-declaration-helpers.ts)）
- 另有 `replace`、`write_file`、`write_todos`、`web_search`（`query` 必填）、`web_fetch`（`prompt` 必填，一次最多 20 个 URL）、`read_many_files`、`ask_user`、`enter_plan_mode` / `exit_plan_mode`、`read_mcp_resource` / `list_mcp_resources`、`activate_skill`。

### 1.4 opencode

内建工具：`bash`、`read`、`grep`、`glob`、`edit`、`write`、`apply_patch`、`task`、`webfetch`、`websearch`、`todowrite`、`question`、`skill`，加实验开关下的 `lsp`（`OPENCODE_EXPERIMENTAL_LSP_TOOL=true`）与 `plan`（实验 + 仅 CLI）（[registry.ts](https://cdn.jsdelivr.net/gh/anomalyco/opencode@dev/packages/opencode/src/tool/registry.ts)、[tools 文档](https://opencode.ai/docs/tools/)）。文档明确说"By default, all tools are enabled and don't need permission to run"。

- `shell`（工具 id 是 `ShellID.ToolID`）：`command`（必填）、`timeout`（可选，毫秒）、`workdir`（可选，描述写 "Use this instead of 'cd' commands"）。（[shell/prompt.ts](https://raw.githubusercontent.com/anomalyco/opencode/dev/packages/opencode/src/tool/shell/prompt.ts)）
- `grep`：`pattern`（必填）、`path`、`include`——**只有三个参数**，没有 head_limit、offset、输出模式。（[grep.ts](https://cdn.jsdelivr.net/gh/anomalyco/opencode@dev/packages/opencode/src/tool/grep.ts)）
- 编辑路径按模型 id 二选一、互斥：`usePatch = modelID.includes("gpt-") && !includes("oss") && !includes("gpt-4")`，命中时 `apply_patch` 顶替 `edit` 与 `write`，否则反之。（[registry.ts](https://cdn.jsdelivr.net/gh/anomalyco/opencode@dev/packages/opencode/src/tool/registry.ts)）

### 1.5 goose

Developer extension 只有 **5 个**工具，并且有单元测试把这个顺序钉死：

```rust
assert_eq!(names, vec!["write", "edit", "shell", "tree", "read_image"]);
```

（[developer/mod.rs](https://raw.githubusercontent.com/aaif-goose/goose/main/crates/goose/src/agents/platform_extensions/developer/mod.rs)，分支路径）

- `write`："Create a new file or overwrite an existing file. Creates parent directories if needed."
- `edit`："Edit a file by finding and replacing text. The before text must match exactly and uniquely. Use empty after text to delete."
- `shell`："Execute a shell command in the current dir. Commands run under `<shell>` (set GOOSE_SHELL to override) ... Returns an object with stdout and stderr as separate fields. **The output of each stream is limited to up to 2000 lines, and longer outputs will be saved to a temporary file.**"
- `tree`："List a directory tree with line counts. Traversal respects .gitignore rules."
- `read_image`："Read an image from a local file path or http(s) URL ... Supports png, jpeg, gif, and webp."

**没有 `grep`、没有 `glob`、没有 `read`**，这一点与既有结论一致。

### 1.6 Cline

见上文 0.3 的工具表。有 `search_codebase`（搜索）与 `run_commands`（执行）两个分开的工具。

---

## 2. 基本只靠 shell 的那几家，对 shell 能力有无限制

| agent | shell 之外的读/搜工具 | shell 能力限制（一手来源） |
| --- | --- | --- |
| **Codex CLI** | 默认无（`grep_files` / `read_file` 是否默认注册未能确认） | `config.toml` 有 shell 专属加固项 `allow_login_shell = false # optional hardening: disallow login shells for shell-based tools`（[agent-approvals-security](https://learn.chatgpt.com/docs/agent-approvals-security.md)）；执行输出有 `original_token_count`（"Approximate token count before output truncation"）字段（[spec.rs permalink](https://raw.githubusercontent.com/openai/codex/d807d44ae7fb69e8e05fc6e6fddea65f7e9421f5/codex-rs/core/src/tools/spec.rs)） |
| **goose** | 无（只有 `tree` 算半个：`tree` 带行数的目录树） | 每流 2000 行上限，超出落临时文件；cmd.exe 下命令必须单行 |
| **aider** | 无（沿用既有笔记结论，本次未一手核对） | 未能追溯 |
| **Claude Code（macOS/Linux/WSL 默认形态）** | `Read` / `Write` / `Edit` 有，**`Grep` / `Glob` 没有** | read-only 命令集免提示；其余命令按规则批准；exit 1 白名单；输出上限 30k 字符（可配到 128k），超限落文件 + 2k 字符预览 |

---

## 3. 有没有 agent 明确劝阻「用 shell 拼等价命令代替专用工具」（原文）

**这是本次调研最实的一条。答案是：有，但只有 opencode 一家把这句话写在模型能看到的地方，而且分两处写。**

### 3.1 opencode —— 写在 **shell 工具描述**里（最完整的一版）

第一句，在 `shell/shell.txt` 模板正文里（[shell.txt](https://raw.githubusercontent.com/anomalyco/opencode/dev/packages/opencode/src/tool/shell/shell.txt)，分支路径；由 [shell/prompt.ts](https://raw.githubusercontent.com/anomalyco/opencode/dev/packages/opencode/src/tool/shell/prompt.ts) 做 `${...}` 插值）：

> "IMPORTANT: This tool is for terminal operations like git, npm, docker, etc. **DO NOT use it for file operations (reading, writing, editing, searching, finding files)** - use the specialized tools for this instead."

第二句，在 bash 专属的命令段里（同一文件插值出的 [shell/prompt.ts](https://raw.githubusercontent.com/anomalyco/opencode/dev/packages/opencode/src/tool/shell/prompt.ts)）：

> "- Avoid using Bash with the `find`, `grep`, `cat`, `head`, `tail`, `sed`, `awk`, or `echo` commands, unless explicitly instructed or when these commands are truly necessary for the task. Instead, **always prefer using the dedicated tools** for these commands:
>   - File search: Use Glob (NOT find or ls)
>   - Content search: Use Grep (NOT grep or rg)
>   - Read files: Use Read (NOT cat/head/tail)
>   - Edit files: Use Edit (NOT sed/awk)
>   - Write files: Use Write (NOT echo >/cat <<EOF)
>   - Communication: Output text directly (NOT echo/printf)"

同一段里还有一句针对截断的劝阻（与「不要自己拼截断」同源）：

> "- If the output exceeds ... lines or ... bytes, it will be truncated and the full output will be written to a file. You can use Read with offset/limit to read specific sections or Grep to search the full content. **Do NOT use `head`, `tail`, or other truncation commands to limit output**; the full output will already be captured to a file for more precise searching."

**注意覆盖范围**：它管的不只是搜索，还包括读、写、编辑、以及「不要用 `echo` 跟模型说话」。

### 3.2 opencode —— 写在 **grep 工具描述**里（反向分流：告诉你什么时候该退回 shell）

[grep.txt](https://raw.githubusercontent.com/anomalyco/opencode/dev/packages/opencode/src/tool/grep.txt)，分支路径，全文：

> "- Fast content search tool that works with any codebase size
> - Searches file contents using regular expressions
> - Supports full regex syntax (eg. "log.*Error", "function\s+\w+", etc.)
> - Filter files by pattern with the include parameter (eg. "*.js", "*.{ts,tsx}")
> - Returns file paths and line numbers with matching lines
> - Use this tool when you need to find files containing specific patterns
> - **If you need to identify/count the number of matches within files, use the Bash tool with `rg` (ripgrep) directly. Do NOT use `grep`.**
> - **When you are doing an open-ended search that may require multiple rounds of globbing and grepping, use the Task tool instead**"

这两句是本节最有价值的内容：**opencode 不只把模型往专用工具上推，还明确标出了"这里该退回 shell"和"这里该交给 subagent"两个出口**。`glob.txt` 里只有后半句（"When you are doing an open-ended search ... use the Task tool instead"）加上 "It is always better to speculatively perform multiple searches as a batch"（[glob.txt](https://raw.githubusercontent.com/anomalyco/opencode/dev/packages/opencode/src/tool/glob.txt)）。

### 3.3 Codex —— 写在 **base instructions** 里（往 shell 方向教，不劝阻）

[default.md](https://raw.githubusercontent.com/openai/codex/main/codex-rs/protocol/src/prompts/base_instructions/default.md)：

> "When searching for text or files, prefer using `rg` or `rg --files` respectively because `rg` is much faster than alternatives like `grep`. (If the `rg` command is not found, then use alternatives.)"
> "Do not use python scripts to attempt to output larger chunks of a file."
> "Do not waste tokens by re-reading files after calling `apply_patch` on them. The tool call will fail if it didn't work."

注意这三条的方向：它约束的是"用 shell 时怎么用"，不是"别用 shell"。Codex 手上没有专用搜索工具可推荐（至少 prompt 是这么写的），所以它只能教最优 shell 命令。

### 3.4 Gemini CLI —— 工具描述里没有这类劝阻

`run_shell_command` 的描述整段只讲三件事：怎么执行、怎么后台、以及返回什么字段（[dynamic-declaration-helpers.ts](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/tools/definitions/dynamic-declaration-helpers.ts)）：

> "This tool executes a given shell command as `bash -c <command>`. ... Command is executed as a subprocess that leads its own process group. Command process group can be terminated as `kill -- -PGID` ... The following information is returned: Output: Combined stdout/stderr ... Exit Code: Only included if non-zero (command failed). ..."

开 `enableEfficiency` 时会额外加一段 "Efficiency Guidelines"（`npm install --silent`、`git --no-pager`），但没有一句"别用 shell 代替 grep/glob/read_file"。

system prompt 层面也是**正向引导**而非劝阻（[snippets.ts](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/prompts/snippets.ts)）：

> "**Command Execution:** Use the `run_shell_command` tool for running shell commands, remembering the safety rule to explain modifying commands first."

而搜索侧的措辞是在 "Context Efficiency" 一节里，用的是"优先 + 带上界"的写法，不是"禁止 shell"：

> "- Prefer using tools like `grep_search` to identify points of interest instead of reading lots of files individually."
> "- You can reduce context usage by limiting the outputs of tools but take care not to cause more token consumption via additional turns required to recover from a tool failure ..."
> "- **Searching:** utilize search tools like `grep_search` and `glob` with a conservative result count (`total_max_matches`) and a narrow scope (`include_pattern` and `exclude_pattern` parameters)."

值得注意的是，这段 prompt 是**按工具是否存在动态切文案**的：`workflowStepResearch` 收到 `enableGrep` / `enableGlob` 标志，为真才拼出 "Use `grep_search` and `glob` search tools extensively (in parallel if independent)"，否则退化成 "Use search tools extensively"。（[promptProvider.ts](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/prompts/promptProvider.ts)）—— 这是一种"工具在就把话说满"的写法。

### 3.5 goose —— 反向：instructions 里指定用 shell

goose 没有 grep/glob 工具，于是它在 extension 的 instructions 里明确指定该用哪些命令，并**给出了理由**（[developer/mod.rs](https://raw.githubusercontent.com/aaif-goose/goose/main/crates/goose/src/agents/platform_extensions/developer/mod.rs)）：

> "For editing software, prefer the flow of using `tree` to understand the codebase structure and file sizes. **When you need to search, prefer `rg` which correctly respects gitignored content.** Then use `cat` or `sed` to gather the context you need, always reading before editing. Use write and edit to efficiently make changes. Test and verify as appropriate."

Windows 分支对应 `findstr` / `Select-String` / `type` / `Get-Content`。

### 3.6 与 heng 现有措辞的逐条对比

heng 的 `grep` 工具描述（[`src/tools/grep.rs`](../../src/tools/grep.rs) 第 53–56 行）：

> "在工作区里按行搜索正则（rg 语法）。这是搜代码的首选方式 —— **不要用 `bash` 拼 `rg` / `grep`**：这个工具是只读的，在每一档权限模式下都放行。范围是会话工作区，遵守 `.gitignore` 并跳过隐藏文件；结果形如 `path:line:文本`，命中太多时会被截断并给出一条落盘路径。"

四个差别，都值得记：

| 维度 | heng | opencode | Codex | Gemini CLI |
| --- | --- | --- | --- | --- |
| 写在哪个工具的描述里 | 搜索工具（`grep`） | **shell 工具**（主）+ 搜索工具（例外分流） | base instructions | system prompt（正向） |
| 覆盖的命令 | 只有 `rg` / `grep` | 搜索、读、写、编辑、`echo`、截断共 7 类 | 只讲 `rg` 优先 | 无 |
| 语气 | "不要用" | "DO NOT use it for file operations" / "Use X (**NOT** Y)" | "prefer" / "Do not use python scripts" | "Prefer using ..." |
| 是否给理由 | 给了（只读 → 权限门） | 给了例外条件（"unless explicitly instructed or when these commands are truly necessary"） | 给了理由（`rg` 更快 / 浪费 token） | 给了理由（省轮次） |
| 是否给出"何时该退回 shell" | **没有** | **有**（计数场景 → `rg`；开放式搜索 → Task 工具） | 不适用 | 不适用 |

---

## 4. 有没有 agent 硬性禁用用 shell 代替专用工具

**没有找到任何一家在执行层（按命令内容拦截）禁止用 shell 拼搜索命令。** 能找到的最接近的三类，都不是同一件事：

1. **prompt 级的"绝不许绕过"**，但针对的是别的工具。Gemini CLI 在 Plan Mode 的 prompt 里连着写了两次：

   > "ONLY use the built-in `exit_plan_mode` tool to present the plan for formal approval AFTER you have reached an informal agreement with the user ... **CRITICAL: NEVER attempt to call this tool via `run_shell_command`.**"
   > "6. **Direct Modification:** If asked to modify code, explain you are in Plan Mode and use the built-in `exit_plan_mode` tool to request approval. **CRITICAL: NEVER attempt to call this tool via `run_shell_command`.**"

   （[snippets.ts](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/prompts/snippets.ts)）这是"不许用 shell 去驱动一个专用工具"，不是"不许用 shell 去搜索"。

2. **prompt 级的强制唯一编辑路径**。Codex base instructions："Use the `apply_patch` tool to edit files (**NEVER try `applypatch` or `apply-patch, only `apply_patch`**)"，并且 "Do not waste tokens by re-reading files after calling `apply_patch` on them"。同样没有执行层校验。

3. **权限层的间接做法：把工具整个拿掉。** Gemini CLI 的文档里有一段 WARNING 讲的是这件事——`tools.core` 不是 shell 专用的白名单：

   > "**WARNING:** The `tools.core` setting is an **allowlist for _all_ built-in tools**, not just shell commands. When you set `tools.core` to any value, _only_ the tools explicitly listed will be enabled. This includes all built-in tools like `read_file`, `write_file`, `glob`, `grep_search`, `list_directory`, `replace`, etc."

   （[shell.md](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/tools/shell.md)，分支路径）效果上这确实能达成"没有 grep 工具就别想搜"，但它是**移除工具**而不是**禁止命令**。

   Claude Code 有一处结构上等价的做法：`Read(...)` 的 deny 规则同时封住同路径的 Edit / Write，因为"both tools change content Claude has to be able to read back"（[tools-reference](https://code.claude.com/docs/en/tools-reference)）—— 同样是规则层的耦合，不是命令层的禁止。

**结论**：在这个样本里，"专用工具优先"完全是 prompt 工程，没有一家把它做成运行时不变量。理由大概率的解释是静态命令检查挡不住等价拼法——opencode 自己就用 tree-sitter 解析命令来生成权限建议，但它也没有试图禁掉 `grep`（反而在 grep 描述里让模型在计数场景用 `rg`）。

---

## 5. 权限模型里 shell 是什么角色

### 5.1 Codex：sandbox 与 approval 的主语就是"执行命令"

官方文档对两层的分工写得很直接（[agent-approvals-security](https://learn.chatgpt.com/docs/agent-approvals-security.md)）：

> "Codex security controls come from two layers that work together:
> - **Sandbox mode**: What Codex can do technically (for example, where it can write and whether it can reach the network) **when it executes model-generated commands**.
> - **Approval policy**: When Codex must ask you before it executes an action (for example, leaving the sandbox, using the network, or running commands outside a trusted set)."

- `sandbox_mode` 取值：`read-only` / `workspace-write`（默认）/ `danger-full-access`（别名 `--yolo` / `--dangerously-bypass-approvals-and-sandbox`）。
- `approval_policy`：`on-request`（默认）/ `never` / `granular = { sandbox_approval, rules, mcp_elicitations, request_permissions, skill_approval }`。旧值 `untrusted` **已退役**，文档专门用 "Migrate from the retired `untrusted` approval policy" 一节说明改法。
- 沙箱实现按平台分：macOS `sandbox-exec` + Seatbelt profile，Linux `bwrap` + seccomp，Windows 原生实现或 WSL2。
- 默认网络关闭，要开得设 `[sandbox_workspace_write] network_access = true`；`features.network_proxy` 可以把命令流量收进域名规则（`deny` 永远赢过 `allow`），但**它不管 web search、app/MCP 工具、浏览器**。
- 受保护路径：可写根里的 `.git`、`.agents`、`.codex` 递归只读。
- `approvals_reviewer = "auto_review"` 可以把"本来就要问人的批准"路由给一个 reviewer 模型；文档强调它只审**已经需要批准**的动作，沙箱内的一律不审。
- 一处 shell 专属开关：`allow_login_shell = false # optional hardening: disallow login shells for shell-based tools`。

**判定**：Codex 把 shell 当成权限模型的**主语**，专用工具是同一层里的其他公民。

### 5.2 Claude Code：shell 内部按命令分三类

- 模式层面 `Bash` 是需要批准的少数工具之一，但内置 read-only 命令集免提示；`acceptEdits` 一档还额外免掉常见文件命令（`mkdir` / `touch` / `mv` / `cp`）。
- **命令级**：Bash 的 exit 1 白名单点名 `grep` / `rg` / `egrep` / `fgrep` / `find` / `diff` / `test` / `[` / `git diff` / `git grep`；PowerShell 侧是 `grep` / `rg` / `egrep` / `fgrep` / `findstr` / `git grep` 与 `git diff`。
- **工具级耦合**：`Bash(npm run *)` / `Read(~/secrets/**)` / `Edit(/src/**)` / `WebFetch(domain:...)` 各有自己的 specifier 格式，其中 `Read(...)` 的 deny 会连带封住 Edit / Write。权限规则格式表里 `Read(~/secrets/**)` 一行写明它适用于 "Read, Grep, Glob, LSP" —— **专用读工具与 shell 共享同一条路径规则**。
- `Monitor` 工具"uses the same permission rules as Bash"，auto 模式下 allow 规则对 `Monitor` 直接失效、与 Bash 一样交给分类器。
- auto 模式下 `Bash` 的 allow 规则被整体放下，改由分类器模型逐条裁。

（[tools-reference](https://code.claude.com/docs/en/tools-reference)、[permissions](https://code.claude.com/docs/en/permissions)）

### 5.3 Gemini CLI：shell 与其它工具在同一张 allowlist 里，但 shell 多一套语法

- `tools.core` 是**所有**内建工具的 allowlist（上面那条 WARNING）。
- shell 额外获得一套前缀语法：`"tools": {"core": ["run_shell_command(git)"]}`；裸的 `run_shell_command` 当通配符。
- `tools.exclude` 同一套语法但是封禁语义，**已标 DEPRECATED**，推荐改用 policy engine TOML；文档给的替代简写是 `commandPrefix` 与 `commandRegex`（"syntactic sugar for combining `toolName = \"run_shell_command\"` with an `argsPattern`"，明确"不是 `run_shell_command` 的参数"）。
- 校验逻辑三条：命令链（`&&` / `||` / `;`）会被**拆开逐段**校验，任一段不允许则整条拒；前缀匹配；**blocklist 优先于 allowlist**。
- 批准模式另有 `ToolConfirmationOutcome`（既有笔记已记录）。

（[shell.md](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/tools/shell.md)）

### 5.4 opencode：shell 是一个普通 tool id，但权限请求带解析出的模式

- 配置层：`permission` 按 tool id 三态，`{"bash": "ask"}` 这样写；支持 `mymcp_*` 通配。
- 运行时：shell 工具用 **tree-sitter（bash + powershell 两套 grammar）** 解析命令，从中抽出命令序列与路径参数，再据此发起**两种** `ctx.ask`——`external_directory`（解析出工作区外的目录）和 `shell` 本身（带上 `patterns` 与一个由 `BashArity.prefix(tokens).join(" ") + " *"` 生成的"以后都放行"建议前缀）。
- 文档层：`write` 由 `edit` 权限管（"The `write` tool is controlled by the `edit` permission, which covers all file modifications (edit, write, apply_patch)"）。

（[shell.ts](https://cdn.jsdelivr.net/gh/anomalyco/opencode@dev/packages/opencode/src/tool/shell.ts)、[tools 文档](https://opencode.ai/docs/tools/)、[permissions 文档](https://opencode.ai/docs/permissions/)）

### 5.5 goose：用 MCP annotations 表达风险，而不是靠工具分类

goose 给每个工具挂 `ToolAnnotations::from_raw(title, read_only_hint, destructive_hint, idempotent_hint, open_world_hint)`：`shell` 是 `(Shell, read_only=false, destructive=true, idempotent=false, open_world=true)`；`tree` 是 `(Tree, read_only=true, destructive=false, idempotent=true, open_world=false)`。也就是说**风险等级跟着工具声明走**，shell 只是恰好被标成 destructive 的那一个。（[developer/mod.rs](https://raw.githubusercontent.com/aaif-goose/goose/main/crates/goose/src/agents/platform_extensions/developer/mod.rs)）

### 5.6 Cline

文档把 `run_commands` 列为典型"需要批准"工具，把读/搜工具列为典型自动批准工具，并说策略可以逐工具关掉。（[all-cline-tools](https://docs.cline.bot/tools-reference/all-cline-tools)）

---

## 6. 专用工具的结果形状、上限与零匹配

| | 返回给模型的形状 | 条数/长度上限 | 零匹配怎么表达 |
| --- | --- | --- | --- |
| **opencode `grep`** | 纯文本：`Found N matches (more matches available)` + 按文件分组的 `path:` / `  Line N: text`；另有 `metadata: { matches, truncated }` 与展示串分离 | limit **100**（写死，不进参数） | **`"No files found"`**（注意：是"没有文件"，不是"没有匹配行"） |
| **Gemini CLI `grep_search`** | `{ llmContent: string, returnDisplay: { summary, matches[] } }`——结构化结果与渲染串分离 | `total_max_matches` 默认 **100**；另有每文件上限 `max_matches_per_file` | **`No matches found for pattern "<pattern>" in the workspace directory (filter: "<include>").`** |
| **Codex `grep_files`** | `FunctionToolOutput::from_text(..., Some(true/false))` | `limit` 默认 100、硬上限 **2000**；`rg` 自身 **30 秒超时** | **`"No matches found."`** |
| **Claude Code `Glob`** | 文件路径列表，按 mtime 排序 | **100**，命中上限给截断标志 | 未在文档中明示 |
| **Claude Code `Grep`** | 三种模式；`count` 模式的总计数覆盖被 `head_limit`/`offset` 截断的命中 | `head_limit` / `offset` 由模型给 | `offset` 越界时**单独一句** `No entries at this offset`，明确区别于"没有匹配"；ripgrep 拒绝的 pattern 返回带诊断信息的错误而不是 `No files found` |
| **goose `shell`** | 结构化对象（stdout / stderr 分开） | 每流 **2000 行**，超出落临时文件 | 不适用 |
| **heng `grep`** | `相对路径:行号:文本` | **500** 条，超出末尾写「还有 N 条未列出：请缩小搜索范围，或用 `glob` 只搜一部分文件」；token 截断走统一流水线 | 一句如实说明，不返回空字符串（[`docs/grep.md`](../../docs/grep.md)） |

### 6.1 Gemini CLI 的 auto-context：唯一一家主动加上下文的

[grep-utils.ts](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/tools/grep-utils.ts) 里有一段其它家没有的逻辑：

> "Automatically enrich grep results with surrounding context if the match count is low and no specific context was requested. This optimization can enable the agent to **skip turns that would be spent reading files after grep calls**."

触发条件是：匹配数在 1–3 之间、且模型没有显式要 `context` / `before` / `after`。补的行数是 **1 条匹配补 ±50 行，2–3 条补 ±15 行**。源码里的注释给了理由与量级：

> "If the result count is low and Gemini didn't request before/after lines of context add a small amount anyways to enable the agent to avoid one or more extra turns reading the matched files. **This optimization reduces turns count by ~10% in SWEBench.**"

另外两点也值得记：

- 它的搜索有**三层降级**：`git grep`（带 `--untracked -n -E --ignore-case`）→ 系统 `grep`（`grep -r -n -H -E -I -i`，把 ignore glob 转成 `--exclude-dir`）→ 纯 JS 实现（`globStream` + `new RegExp`）。ripgrep 不在依赖里。
- 超时错误自带出路指引：`Operation timed out after <ms>ms. In large repositories, consider narrowing your search scope by specifying a 'dir_path' or an 'include_pattern'.`
- 展示串里区分命中行与上下文行：命中用 `L12: `，上下文用 `L12- `；单行超过 `MAX_LINE_LENGTH_TEXT_FILE` 截断并追加 `... [truncated]`。

（[grep.ts](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/tools/grep.ts)、[grep-utils.ts](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/tools/grep-utils.ts)）

---

## 7. 同一批能力的不同包装，还是各有独特取舍

**共识部分（各家都在做的）**：

- 编辑走精确 search-replace 或其变体，参数面都是"旧串 + 新串"；
- 搜索工具的结果形状是"按文件分组 + 行号 + 行内容"；
- 工具输出有上限，超限落盘并在结果里留指针；
- shell 工具都带 `timeout` 与"用参数换目录而不是 `cd`"这类 poka-yoke 提示（opencode 的 `workdir` 描述、Gemini 的 `dir_path`）；
- 工具描述里都写死"编辑前先读"（Gemini 的 `replace` 描述："Always use the `read_file` tool to examine the file's current content before attempting a text replacement"；goose instructions："always reading before editing"）。

**各家真正不同的取舍**：

| agent | 独特之处 |
| --- | --- |
| **Claude Code** | 非 Windows 默认**不给** `Grep`/`Glob`，改用 shell 里内嵌的 `bfs`/`ugrep`；专用工具要显式点名才回来。`Glob` 默认**不**遵守 `.gitignore` 而 `Grep` 遵守，两者刻意不一致 |
| **Codex CLI** | 编辑被 prompt 钉成唯一的 `apply_patch` 入口；默认用 shell，base instructions 直接教 `rg`；工具箱里已有 `grep_files`（真 spawn `rg`，30 秒超时），但注册条件未能确认 |
| **Gemini CLI** | grep 的 auto-context（≤3 条命中自动补上下文，注释称 SWEBench 上省约 10% 轮次）；三层搜索降级；prompt 按工具是否存在动态拼句；`glob` / `grep` 都把"遵守哪些 ignore"做成了显式参数 |
| **opencode** | 编辑路径按模型 id 二选一（`apply_patch` ↔ `edit`+`write` 互斥）；把"别用 shell 做文件操作"的完整对照表写进 shell 工具描述；grep 描述里明确标注两个出口（计数 → `rg`，开放式 → Task 工具）；权限请求用 tree-sitter 解析出的模式生成 |
| **goose** | 5 个工具的极简下界（单测钉住）；`tree`（带行数的目录树）替代 glob；instructions 里指定用 `rg`/`cat`/`sed` 并给理由；风险等级写在 MCP annotations 上 |
| **Cline** | 用 `search_codebase` 一个工具合并搜索；`editor` 与 `apply_patch` 按模型互斥且 Plan 模式都不给 |

---

## 对照表

| agent | 有一等专用工具 | 劝阻措辞写在哪、原文要点 | shell 在权限模型里的位置 |
| --- | --- | --- | --- |
| **Claude Code** | 有（`Read`/`Write`/`Edit`/`Grep`/`Glob`/`WebFetch`/`WebSearch`/`LSP`），但**非 Windows 默认只给前三者** | 公开文档里无此类措辞（prompt 未公开）；反向证据是默认让模型走 `find`/`grep` shell | 需要批准的少数工具之一；内置 read-only 命令集免提示；exit 1 有命令白名单；`Read(...)` 路径规则同时覆盖 `Grep`/`Glob` |
| **Codex CLI** | 默认无（`grep_files`/`read_file` 存在于代码，注册条件未确认） | base instructions 里反向教：`prefer using rg`；`Use the apply_patch tool to edit files (NEVER try applypatch...)` | **权限模型的主语**：`sandbox_mode` 与 `approval_policy` 都以"执行命令"为描述对象；另有 `allow_login_shell` 专属开关 |
| **Gemini CLI** | 有（`read_file`/`grep_search`/`glob`/`list_directory`/`replace`/…） | 工具描述里**没有**；system prompt 里是 "Prefer using tools like `grep_search`…" + 保守范围建议；Plan Mode 有 "NEVER attempt to call this tool via `run_shell_command`" | 与其它工具同在 `tools.core` allowlist，但额外获得 `run_shell_command(<prefix>)` 语法与命令链拆解校验；`tools.exclude` 已弃用 |
| **opencode** | 有（`read`/`grep`/`glob`/`edit`/`write`/`apply_patch`/`lsp`…） | **两处**：shell 描述里 "DO NOT use it for file operations" + 六条 "Use X (NOT Y)" 对照表；grep 描述里反向分流 "use the Bash tool with `rg` … Do NOT use `grep`" | 普通 tool id，但运行时用 tree-sitter 解析命令生成 `bash` 与 `external_directory` 两类批准请求；`write` 归 `edit` 权限管 |
| **goose** | 无（只有 `tree`/`read_image`/`write`/`edit`/`shell`） | 反向：instructions 里指定 "prefer `rg` which correctly respects gitignored content" | 靠 MCP annotations 标 destructive/open-world；goose 自己的 `GOOSE_MODE` / `permission.yaml` 在工具级 |
| **Cline** | 有（`read_files`/`search_codebase`/`apply_patch`/`editor`…） | **未能追溯**（源码路径多次取不到，见下） | 文档列为典型需批准工具；读/搜工具自动批准 |

---

## 对 heng 的启示（只写从上面这些一手来源读得出来的）

1. **劝阻措辞放在哪一侧，两种做法都有实现。** opencode 把它写在 **shell 工具**的描述里，覆盖 7 类命令；heng 写在**搜索工具**自己的描述里，只覆盖 `rg`/`grep`。从 opencode 的覆盖面看，把劝阻放在被劝阻的那一侧，模型在"想拼一条命令"的那一刻能读到的概率更高——因为那时它正在读 shell 的 schema。

2. **措辞强度与"例外出口"是两件事。** 没有任何一家在执行层拦截，所以强度只能来自文本。opencode 用的是 `DO NOT` / `NOT` 加"unless explicitly instructed or when these commands are truly necessary"这类出口条件；Codex 用 `NEVER try ... only ...`。反过来，**只写禁令不给出路会有代价**：opencode 的 grep 描述专门为"计数"这个专用工具做不到的场景开了个 `rg` 的口子，Gemini 的 `grep_search` 则干脆不做计数（`names_only` 只给路径），Codex 的 `grep_files` 则把 `--files-with-matches` 固定住、只给路径列表——三家对"计数"这件事的处理各不相同。

3. **"只读搜索走专用工具"确实能省掉一次批准，这有三处一手佐证。** Claude Code 内置 read-only 命令集（`grep`/`rg`/`find`/`git diff`/`git grep`…）免提示；Gemini CLI 允许把 `tools.core` 收成 `["run_shell_command(git)"]`——注意这条 WARNING 说这样一收 `grep_search` 也会一起消失；opencode 的权限请求是从 tree-sitter 解析出的命令模式出发的。这三点都指向 heng 把 `grep` 的 `effect()` 定成 `Effect::ReadOnly` 所换来的东西是真实的（见 [`docs/grep.md`](../../docs/grep.md)）。

4. **零匹配必须写句子，且要与"翻页越界"分开。** 四家都给了明确句子（opencode `"No files found"`、Gemini `No matches found for pattern "…" in … (filter: "…").`、Codex `"No matches found."`、heng 的中文说明），Claude Code 还专门把 `offset` 越界写成另一句 `No entries at this offset`，以免模型把"翻过头了"读成"没有匹配"。heng 已有这个区分，值得保持。

5. **上限值区间是 100–500。** Claude `Glob` 100、opencode `grep` 100、Gemini `total_max_matches` 默认 100、Codex `grep_files` 100（硬上限 2000）、heng 500。截断时的收尾句各家都有：Claude 给截断标志，opencode 加 "(Results truncated. Consider using a more specific path or pattern.)"，Gemini 在行内加 "(results limited to N matches for performance)"，Codex 直接截断不提示，heng 写「还有 N 条未列出：请缩小搜索范围，或用 `glob` 只搜一部分文件」——**最后这句把可用的下一步工具点名了**，是这几家里唯一在截断提示里推荐自家另一个参数的。

6. **"结果里带上下文"这件事 Gemini 是唯一默认做的**，而且给了可引用的量级（≤3 条命中时自动补上下文，源码注释称 SWEBench 上减少约 10% 轮次）。它的触发条件写得很克制：只在模型没有显式要 `context`/`before`/`after` 时才补。这是一个可对照的实验点，但**本次调研没有找到第二家的一手证据**，也没有找到对照实验数据。

---

## 未能追溯到 primary source 的 claim

1. **Codex `shell` 工具的描述原文**。`codex-rs/core/src/tools/spec.rs` 单文件超过十万字符，抓取被截断，`create_shell_tool` 的描述串没读到。因此"Codex 有没有劝阻模型别用 `sed` 之类改文件"这个问题**没有答案**（只能确认 base instructions 里有 `apply_patch` 的强制句）。
2. **Codex `grep_files` / `read_file` / `list_dir` 的注册条件**，即它们是否默认对模型可见、在哪个 feature flag 下开启。只知道 handler 存在（[spec.rs permalink](https://raw.githubusercontent.com/openai/codex/d807d44ae7fb69e8e05fc6e6fddea65f7e9421f5/codex-rs/core/src/tools/spec.rs)），注册段落被截断。
3. **Cline 的 system prompt 原文**，因此"Cline 有没有劝阻用 `run_commands` 代替 `search_codebase`"**未能追溯**。尝试过 `src/core/prompts/system.ts`、`src/core/prompts/system-prompt/index.ts`、`src/core/prompts/system-prompt/README.md`、`docs/exploring-clines-tools/cline-tools-guide.mdx`（均在 main 分支 404），grep.app 的 code search API 持续返回 429 验证页，GitHub REST API 也被限流。只能引用其官方文档站的工具表。
4. **Claude Code 的 system prompt 与工具描述原文**（闭源，无一手来源）。文中所有关于 Claude Code 的说法都来自官方文档的**行为描述**，不是模型实际看到的字符串。
5. **aider / Continue / Amp / OpenHands / mini-SWE-agent 的本次一手核对**。这五家在 `docs/research/coding-agent-features.md` 与 `docs/research/notes/` 里已有结论，本次没有重新验证，因此正文里不引用它们的新事实。
6. **opencode `read` 与 `glob` 的参数面**（只抓到了 `grep.txt` / `glob.txt` 的描述文本与 `grep.ts` 的完整参数，`read.ts`、`glob.ts` 未抓）。
7. **Codex 的 execpolicy（Starlark `prefix_rule` / `network_rule`）策略文件原文**。既有笔记记录了它，本次只在官方文档里确认到 granular approval 里有 `rules = true` 这一项，没有重抓策略文件。

## 没有覆盖到的 agent

Amp、Continue、aider、OpenHands、mini-SWE-agent，以及闭源或文档稀少的若干（Cursor、Windsurf 等）。目标里要求覆盖 5 个以上，本次做到了 6 个（Claude Code、Codex CLI、Gemini CLI、opencode、goose、Cline）。