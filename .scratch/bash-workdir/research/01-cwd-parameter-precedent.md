# 一手调研：给 shell 工具加一个「路径」参数

> 这是 `.scratch/bash-workdir/` 那条意向的 research：查清「coding agent 的 shell 工具加 cwd /
> workdir 参数」在 2026 年是不是主流做法、有哪些先例、各自怎么做、代价是什么。**写就于
> 2026-10-09。**
>
> 核实过的一手来源：Anthropic 官方文档站（`code.claude.com/docs/en/{tools-reference,
> sandboxing}`、`platform.claude.com/docs/en/agents-and-tools/tool-use/bash-tool`）、
> `openai/codex` 与 `anomalyco/opencode`、`google-gemini/gemini-cli`、`aaif-goose/goose`
> 四个仓库在 2026-10-09 的源码（`codeload.github.com` 的 tarball，Codex 取 `main`、
> opencode 取 `dev`、goose 取 `main`）、crates.io 上发布的 `codex-protocol 0.63.0`、
> OpenAI 官方开发者文档（`developers.openai.com/api/docs/guides/tools-{shell,local-shell}`）、
> OpenHands 官方文档的 agent-server OpenAPI、以及 MCP 官方规范的 roots 页。
> **全部 URL 列在末尾。**
>
> 本文只落在 `.scratch/bash-workdir/research/` 下，没有改 `.scratch/README.md`、没有写 spec、
> 没有建票，也没有动 `src/` 与 `docs/`。

## 结论摘要

1. **「工具参数 + 每次调用一个全新进程」是 2026 年的主流形态，而且 cwd 参数是被明确推荐
   的做法**：Codex 给 `workdir`、Gemini CLI 给 `dir_path`、opencode 给 `workdir`，三者都把
   相对路径**解析到会话工作区**再启动进程；Anthropic 的官方 bash 工具规格
   （`bash_20250124`）与 Claude Code 的 Bash 工具则**没有** cwd 字段。
2. **持久 shell 是另一条路，代价由 Anthropic 自己写清楚**：官方规格要求「应用自己养一个长驻
   bash 进程」，用 sentinel 行给每次调用的输出分帧，超时要杀掉整个进程组再重启；Claude Code
   走的是折中——每条命令一个独立进程，但把 `cd` 的结果记成会话状态，越出工作目录就重置并
   在结果里明说。
3. **cwd 同时是安全边界，不是纯便利参数**：Claude Code 的 bubblewrap 沙箱默认可写集就是
   「工作目录 + 每用户临时目录 + `--add-dir` 加的目录」，Codex 的 `workspace-write` 把
   `workdir` 列为可写根，工作目录是 linked worktree 时还要额外放行主仓库的 `.git`。cwd 一变，
   可写集合就得跟着重算。
4. **最容易踩的坑是「相对谁解析」**：Codex 在源码注释里专门写明相对路径必须相对「选中环境的
   cwd」而不是进程 cwd，并为此把字段保持原始字符串到环境选定之后；Gemini CLI 用
   `path.resolve(targetDir, dir_path)` 并把结果过一遍工作区校验。
5. **不加 cwd 参数的先例同样有据可查**：goose 的模型侧 shell schema 只有 `command` 与
   `timeout_secs`，cwd 是宿主 API（`shell_with_cwd`）的参数；OpenAI 新的 `shell` 工具
   （Responses API）根本没有 cwd 字段，legacy 的 `local_shell` 曾经有 `working_directory`；
   MCP 的 roots 是「客户端向服务器声明的目录边界」，与「每次调用传 cwd」是两件事。

---

## 1. 谁有什么：对照表

「shell 是否持久」指同一会话里连着跑时，上一条命令的进程状态（cwd、环境变量、后台作业）是否
留给下一条。

| 项目 | shell 工具 | 有无 cwd 参数 | 字段名 | shell 是否持久 | 一手来源 |
| --- | --- | --- | --- | --- | --- |
| Claude Code（Anthropic CLI） | `Bash` / `PowerShell` / `Monitor` | 否 | —（`cd` 靠会话状态沿用） | 每条命令独立进程；cwd 跨命令沿用，越界重置 | `code.claude.com/docs/en/tools-reference` |
| Anthropic bash 工具规格（API 级） | `bash_20250124` | 否 | 只有 `command`、`restart` | 要求应用侧持有持久 bash 会话 | `platform.claude.com/.../bash-tool` |
| Codex CLI（`openai/codex`） | `exec_command` + `write_stdin` | 有 | `workdir`（相对 turn cwd） | 命令默认新进程；长命令用 `session_id` + `write_stdin` 续 | `codex-rs/core/src/tools/handlers/shell_spec.rs:38-107` |
| Gemini CLI | `run_shell_command` | 有 | `dir_path`（绝对或相对 workspace root） | 每次 spawn（PTY 或 child_process），只有 background 进程保留 | `packages/core/src/tools/shell.ts`、`geminicli.com/docs/tools/shell/` |
| opencode（`dev`） | `shell` | 有 | `workdir`（相对 instance directory） | 每次 spawn（`shell.ts` 的 `spawner.spawn`） | `packages/opencode/src/tool/shell/prompt.ts:16-24`、`shell.ts:611-614` |
| goose | developer extension 的 shell | 否（模型侧） | 宿主 API 参数 `working_dir` | 每次 spawn | `crates/goose/src/agents/platform_extensions/developer/shell.rs` |
| OpenHands agent-server | `POST /api/bash/execute_bash_command` | 有（执行服务层） | `cwd` | 每次执行 | `docs.openhands.dev/.../execute-bash-command` |
| OpenAI Responses API（现行 `shell`） | `shell` | 否 | `action` 只有 `commands`/`timeout_ms`/`max_output_length` | 由宿主 runtime 决定 | `developers.openai.com/api/docs/guides/tools-shell` |
| OpenAI Responses API（legacy `local_shell`） | `local_shell` | 有 | `working_directory` | 由宿主 runtime 决定 | `developers.openai.com/api/docs/guides/tools-local-shell` |
| MCP | （协议层无 shell 工具） | 不适用 | `roots` / `roots/list` | 不适用 | `modelcontextprotocol.io/specification/2025-06-18/client/roots` |

未能核实的项目见 §9。

---

## 2. Claude Code 与 Anthropic 的 bash 工具规格

### 2.1 工具 schema 里没有 cwd

Anthropic 官方 bash 工具规格页把输入字段列成一张两行的表：`command`（必填，除非用
`restart`）与 `restart`（可选，用来重启会话）。规格明确说这个工具是 **schema-less** 的——
`input_schema` 不能提供，schema 写死在模型里，因此集成方无法通过配置加一个 cwd 字段
（`platform.claude.com/docs/en/agents-and-tools/tool-use/bash-tool` 的 Parameters 一节）。

Claude Code 自己的 tools reference 描述 Bash 工具时提到的字段只有命令、描述、
`timeout`（默认两分钟，`BASH_MAX_TIMEOUT_MS` 给上限）与 `run_in_background`
（`code.claude.com/docs/en/tools-reference` 的 Bash tool behavior 一节）——同样没有 cwd。

### 2.2 shell 不持久，但 cwd 会沿用

同一节的第一句就是「The Bash tool runs each command in a separate process.」，紧接着
"What persists between commands" 给出这套语义：

- Claude 在主会话里跑 `cd`，新的工作目录会**带到后续的 Bash 调用**，只要它仍在项目目录
  或用 `--add-dir` / `/add-dir` / `additionalDirectories` 加进来的目录里；子代理的会话不沿用。
- `cd` 落到这些目录之外时，Claude Code 回到项目目录，并在工具结果里追加
  `Shell cwd was reset to <dir>`。
- 设 `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR=1` 可以关掉这个沿用，让每条命令都从项目目录开始。
- 环境变量不沿用；别名与函数来自启动文件，在会话开始时被 source 一次后套用到每条命令。
- 一条命令因为超时被移入后台时，里面的 `cd`/`pushd`/`popd`/`chdir` **不沿用**，结果里明说
  `Session cwd remains <dir>`。

也就是说，Claude Code 用「宿主记状态」替代了「工具传参数」。这条路的代价写在官方 bash 工具
规格里：应用要持有一个长驻 bash 进程（`start_new_session=True` 给它自己的进程组），每条命令后
打印唯一 sentinel 行给输出分帧，因为「往活进程的管道写永远不会 EOF」；命令挂死时要用
`os.killpg` 杀掉 shell 与它起的全部子进程，然后重启会话。规格的 Limitations 一节还写明
「Bash session state is client-side」——API 无状态，谁来养这个会话是应用自己的事。

### 2.3 工作目录集合与沙箱

Claude Code 的沙箱文档给出与本仓库最可比的一套结构（`code.claude.com/docs/en/sandboxing`）：

- **默认可写**：「当前工作目录及其子目录 + `--add-dir` 加的目录 + 每用户临时目录」；默认可读是
  整机（凭据文件也读得到，除非用 `sandbox.credentials` 挡住）。
- **Linux 用 bubblewrap**，macOS 用 Seatbelt，底层是开源的 `@anthropic-ai/sandbox-runtime`。
- **git worktree 特例**：工作目录是 linked worktree 时，沙箱额外允许写主仓库共享的 `.git`
  目录，好让 `git commit` 能更新 refs 与 index，但其中的 `hooks/` 与 `config` 仍被拒。
- **受保护路径**按「在工作目录内」「只在工作目录内」「会把工作目录变成 bare 仓库」分组，
  其中裸仓库那组（顶层 `HEAD`、`objects`、`refs`）在 Linux 上是沙箱在命令运行期间主动删除
  掉新出现的这些东西。
- 沙箱只包 shell 命令：文件工具、MCP server、hooks、LSP 都在沙箱外，按权限规则而不是沙箱边界
  判断。

### 2.4 把 cwd 当会话状态：Claude Code 也有一个显式工具

`EnterWorktree` / `ExitWorktree` 是官方工具表里的两个条目（`tools-reference` 的工具表）。
`EnterWorktree` 接受一个 `path`，切到既有 worktree；文档写明进入 `.claude/worktrees/` 之外的
路径会先问用户同意，因为「它会移动会话的工作目录与写权限」。这是「cwd 作为会话状态」的
一手先例，与工具参数是两种不同的东西。

---

## 3. Codex CLI：参数、turn cwd、沙箱三件事

Codex 是材料最足的一个，三层都有一手出处。

### 3.1 模型看到的 schema

`codex-rs/core/src/tools/handlers/shell_spec.rs` 里 `create_exec_command_tool_with_environment_id`
构造 `exec_command` 工具，属性表（`:38-72`）是：

| 字段 | schema 里的描述（原文） |
| --- | --- |
| `cmd` | `Shell command to execute.` |
| `workdir` | `Working directory for the command. Defaults to the turn cwd.` |
| `tty` | `True allocates a PTY for the command; false or omitted uses plain pipes.` |
| `yield_time_ms` | `Wait before yielding output. Defaults to 10000 ms; effective range is 250-30000 ms.` |
| `max_output_tokens` | 输出 token 预算，默认 10000 |
| `shell` | 启动哪个 shell 二进制，默认用户默认 shell |
| `login` | `-l/-i` 语义，默认 true |
| `environment_id` | 来自 `<environment_context>` 的环境 id，省略则用主环境 |
| `sandbox_permissions` | 每次命令的沙箱覆盖：`use_default` / `with_additional_permissions` / `require_escalated` |
| `justification` | `require_escalated` 时给用户看的审批理由 |
| `prefix_rule` | 可复用的 `cmd` 前缀审批规则，例如 `["git", "pull"]` |
| `additional_permissions` | `{network:{enabled}, file_system:{read:[绝对路径], write:[绝对路径]}}` |

值得抄的两点：**`workdir` 的默认值被写成「turn cwd」而不是「上次命令的 cwd」**——Codex 明确
不把 `cd` 的结果当会话状态；**权限申请字段（`additional_permissions.file_system.read/write`）
要求绝对路径**，与我们 `escalation.writable_paths` 的口径一致。

`workdir` 与持久性是两件事：另有一个 `write_stdin` 工具（`shell_spec.rs:106-158`），参数是
`session_id` + `chars` + `yield_time_ms` + `max_output_tokens`，描述为「向已有的 unified exec 会话
写入字符并返回最近的输出」。持久的是**还在运行的进程**，不是 cwd。

### 3.2 workdir 相对谁解析

`codex-rs/core/src/tools/handlers/unified_exec.rs:44-52` 的注释是一手教训：

> Keep this raw until after environment selection; relative paths must be resolved against the
> selected environment cwd, not the process cwd.

实现对应 `codex-rs/core/src/tools/handlers/unified_exec/exec_command.rs:191-200`：先由
`environment_id` 选出 turn 环境并取它的 `cwd()`，然后
`workdir` 为空就用它，否则 `native_environment_cwd.join(workdir)`。

「turn cwd」是什么也有出处：`codex-rs/core/src/session/turn.rs:841-856` 的注释说 turn cwd
预期是一个目录，若是文件则忽略失败的 `<cwd>/.git` 探测并从其父目录继续向上找；显示用的仓库根
由 `find_nearest_ancestor_with_markers(..., vec![".git"], ...)` 从 turn cwd 向上搜，搜不到就退回
cwd 本身。也就是说 **Codex 的工作目录锚点是会话启动时确定的，不是模型每次调用现给的**。

### 3.3 沙箱与 workdir 的关系

`codex-rs/utils/sandbox-summary/src/sandbox_summary.rs` 把沙箱策略渲染成人读字符串，
其中 `workspace-write` 一栏的第一个可写根就叫 `workdir`（`:36`、`:119`），样例形如
`workspace-write [workdir, /tmp, $TMPDIR, ...]`。也就是：**cwd 落在可写集合里是沙箱的前提，
不是例外**。

### 3.4 旧字段：`ShellToolCallParams`

crates.io 上发布的 `codex-protocol 0.63.0` 里，`src/models.rs:330-341` 定义了
`ShellToolCallParams`（用于函数名 `container.exec` 或 `shell` 的调用参数反序列化）：

```rust
pub struct ShellToolCallParams {
    pub command: Vec<String>,
    pub workdir: Option<String>,
    pub timeout_ms: Option<u64>,
    pub with_escalated_permissions: Option<bool>,
    pub justification: Option<String>,
}
```

即用户问的 `with_escalated_permissions` 与 `justification` **确实存在过**，与
`ShellToolCallParams.workdir` 并列。但 2026-10-09 的 `main` 分支里已经搜不到
`with_escalated_permissions` 这个字符串，权限请求换成了 §3.1 那套
`sandbox_permissions` + `additional_permissions` + `prefix_rule`；`command` 也从 argv 数组变成
了一个 shell 命令字符串（`cmd`）。引用这一段时要说清版本。

---

## 4. Gemini CLI：`dir_path`

官方文档把参数列成四条（`geminicli.com/docs/tools/shell/`）：`command`（必填）、
`description`、`dir_path`（「absolute path or relative path from workspace root」）、
`is_background`。

源码里的形状（`packages/core/src/tools/shell.ts`）：

- `ShellToolParams` 是 `command`、`description?`、`dir_path?`、`is_background?`、`delay_ms?`，
  外加一个沙箱扩展位 `[PARAM_ADDITIONAL_PERMISSIONS]?: SandboxPermissions`（网络 + 文件读写路径
  列表）。
- 解析与校验是两处：`ShellTool.validateToolParamValues` 在参数校验阶段就
  `path.resolve(config.getTargetDir(), params.dir_path)` 再 `config.validatePathAccess(resolved)`；
  `ShellToolInvocation.execute` 里再算一次 cwd 并做同样的校验，失败返回
  `ToolErrorType.PATH_NOT_IN_WORKSPACE`（错误展示文案是 `Path not in workspace.`）。
- cwd 一路传到执行层：`ShellExecutionService.execute(commandToExecute, cwd, ...)`，最终交给
  `sandboxManager.prepareCommand({ command, args, env, cwd, policy })`。**沙箱与 cwd 在同一处汇合**，
  这点与我们把沙箱挂到工作区上的做法一致。
- 每次调用都新起进程：`childProcessFallback` 里 `cpSpawn(..., { cwd: finalCwd, stdio: ['ignore', ...] })`，
  或走 PTY 的 `ptyInfo.module.spawn(..., { cwd: finalCwd, ... })`。跨调用唯一保留下来的是
  `is_background` 的进程（PTY 留在 `activePtys` 里，并按 sessionId 记进
  `backgroundProcessHistory`）。环境变量每次重新构造并清洗（`sanitizeEnvironment`），所以也没有
  「export 一次后面还在」这回事。
- 顺带一提：同一个 `execute` 里有一段针对「网络受限但命令需要联网」的启发式，会把
  `params.dir_path || config.getTargetDir()` 直接加进建议的写权限列表——**cwd 决定了向用户要哪些
  写权限**。

---

## 5. opencode：把「用 workdir 而不是 cd」写进 schema 描述

`packages/opencode/src/tool/shell/prompt.ts:16-24` 的参数 schema：

```ts
workdir: Schema.optional(Schema.String).annotate({
  description: `The working directory to run the command in. Defaults to the current directory. Use this instead of 'cd' commands.`,
})
```

实现对应 `packages/opencode/src/tool/shell.ts:611-614`：
`params.workdir ? resolvePath(params.workdir, instanceCtx.directory, shell) : instanceCtx.directory`——
相对 instance directory 解析（`resolvePath` 还负责 `~` 展开等，`shell.ts:369-380` 的 `argPath`
用了同一个函数）。

这是全部材料里**唯一一处把「不要用 cd」直接写给模型看**的地方，值得当成 prompt 措辞的参考。
同一文件里还有一点：`shell.ts:28` 定义了 `CWD = new Set(["cd", "chdir", "popd", "pushd",
"push-location", "set-location"])`，这些命令被归到会改工作目录的那一类做权限判断。

---

## 6. goose：cwd 是宿主 API 的参数，不进模型 schema

`crates/goose/src/agents/platform_extensions/developer/shell.rs` 里，模型侧结构体是：

```rust
pub struct ShellParams {
    pub command: String,
    pub timeout_secs: Option<u64>,
}
```

而工作目录出现在宿主侧的 Rust 方法签名上：`shell_with_cwd(params, working_dir: Option<&Path>, ...)`
与 `shell_with_cwd_and_emitter(params, working_dir, session_id, emitter, cancellation_token)`
（`shell.rs:369-383`），默认入口 `shell(params)` 传的是 `None`（`shell.rs:365`）。也就是说
**要换工作目录得由调用 goose 的一方决定，而不是让模型在工具参数里说**。

这对我们的意义是：goose 把「工作目录」当成部署配置（扩展进程从哪儿启动、宿主想让它看哪个
目录），而不是模型可以拨的旋钮。想清楚这一层，才知道 cwd 参数到底属于谁。

---

## 7. OpenHands：cwd 在执行服务那一层

OpenHands 的 agent-server 文档给出 `POST /api/bash/execute_bash_command` 的 OpenAPI
（`docs.openhands.dev/sdk/guides/agent-server/api-reference/bash/execute-bash-command`），
`ExecuteBashRequest` 的字段是：

- `command`（必填）：要执行的 bash 命令；
- `cwd`（可空）：「The current working directory」；
- `timeout`（整数，默认 300 秒）。

返回的 `BashOutput` 里 `exit_code` 可空，文档写明「None implies the command is still running」，
配合另两个接口（start / 读输出）构成后台命令模型。这是「cwd 由执行服务收、由模型的下游传」的
形态：模型给命令，执行服务决定在哪儿跑。

---

## 8. 模型厂商的 shell 工具规格：cwd 的三次演变

这一组最值得对照，因为它们是同一件事在两年里的三种答案。

### 8.1 Anthropic `bash_20250124`（2025-01 起）

- 输入只有 `command` 与 `restart`（`platform.claude.com/.../bash-tool`）。
- 文档第一句就是「Your application keeps one bash process alive across tool calls, so state
  persists between commands. The working directory, environment variables, and any files a command
  creates are still there for the next command.」
- 「The API is stateless. Nothing about your shell session travels between requests, so your
  application decides when the session starts, how long it lives, and when to restart it.」
- `restart: true` 的语义是「杀掉 shell、起一个新的，并回一个确认重启的 tool_result」，重启后
  「the working directory, environment variables, and any running processes are gone」。
- 代价那一节写得很直白：不能跑交互式命令；会话状态在客户端；API 不截断输出；输出不流式。

### 8.2 OpenAI legacy `local_shell`（Responses API）

`action` 的字段是 `command`（argv 数组）、`env`、`timeout_ms`、`user`、`working_directory`。
官方示例代码统一写成 `cwd: action.working_directory ?? process.cwd()`
（`developers.openai.com/api/docs/guides/tools-local-shell`）。**这是唯一一处厂商规格里
cwd 是模型可见字段的**——而且该工具已于 2026-02-12 结束支持，官方让新集成改用 `shell`。

### 8.3 OpenAI 现行 `shell`（Responses API）

`shell_call` 的 `action` 只有 `commands`（字符串数组）、`timeout_ms`、`max_output_length`，
输出是 `shell_call_output` 的数组（每段带 `stdout`/`stderr`/`outcome`）。**没有 cwd 字段**。
cwd 的位置变成：

- hosted 模式：容器有固定默认工作目录 `/mnt/data`，文档写明「Default working directory is
  `/mnt/data`」，并且「`/mnt/data` is always present and is the supported path for
  user-downloadable artifacts」；
- local 模式：官方给的 `ShellExecutor.run` 示例根本没设 `cwd`，等于继承宿主进程的 cwd。

也就是说新版把 cwd 从「模型参数」降级成「宿主/环境的属性」。如果照这条路线走，模型要用
`cd` 或绝对路径，而不是工具参数。

---

## 9. MCP：roots 是边界声明，不是调用参数

MCP 规范 2025-06-18 的 Roots 页把这件事讲得很清楚
（`modelcontextprotocol.io/specification/2025-06-18/client/roots`）：

- roots 是客户端**向服务器暴露**的文件系统根，作用是让服务器知道自己的活动边界；
- 支持方在初始化时声明 `roots` capability，`listChanged` 表示会不会在列表变化时通知；
- 服务器通过 `roots/list` 请求拿到列表；
- 「implementations are free to expose roots through any interface pattern that suits their
  needs—the protocol itself does not mandate any specific user interaction model」，并提到可以
  结合版本控制系统的自动工作区探测。

所以生态里已经有一个「告诉服务器我在哪些目录里工作」的约定，但它表达的是**边界集合**，
不是「这次命令在哪儿跑」。这两者不要混：前者是授权与发现，后者是执行。

---

## 10. 反例与教训（逐条给出处）

1. **「cd 会话状态」与「工具参数」二选一，不要既搞又搞。** opencode 在 schema 描述里直接写
   `Use this instead of 'cd' commands.`（`shell/prompt.ts:16-24`），Codex 把默认值定成
   `Defaults to the turn cwd.` 而不是「上次 cd 的结果」（`shell_spec.rs:41-45`）。
   Claude Code 走了另一条：沿用 cd，越界重置并告知（`tools-reference`）。
2. **相对路径要相对哪个 cwd，必须写死并注释。** Codex 的注释是「relative paths must be resolved
   against the selected environment cwd, not the process cwd」，并为此把字段保持原始字符串
   （`unified_exec.rs:44-52`、`exec_command.rs:191-200`）。Gemini 用
   `path.resolve(targetDir, dir_path)` + `validatePathAccess` 两处都过一遍
   （`shell.ts` 的 `validateToolParamValues` 与 `execute`）。
3. **cwd 参与权限判定，因此加了参数就要同时想清楚沙箱。** Claude Code 的默认可写集就是
   「工作目录 + 临时目录 + add-dir」（`sandboxing` 的 What the sandbox restricts 一节）；
   Codex 的 `workspace-write [workdir, ...]`（`sandbox_summary.rs:36`）；Gemini 的沙箱扩展建议会
   把 `dir_path` 当作要申请的写路径（`shell.ts` 的 `shouldConfirmExecute` 启发式）。
   一旦允许模型换 cwd，可写集合要么跟着变（更贵），要么把可写集合锚在会话工作区、把 cwd 参数
   当作「工作区内的子目录」（更省，但模型会撞墙）。
4. **git 仓库定位通常在上层解决，不靠每次调用的 cwd。** Codex 从 turn cwd 向上找 `.git`
   标记来得到仓库根（`session/turn.rs:841-856`）；Claude Code 在 linked worktree 下额外放行
   主仓库的 `.git` 可写（`sandboxing` 的 Filesystem isolation 一节）。两者都没有让模型自己
   指仓库根。
5. **不加参数也有正当理由。** goose 把 cwd 留给宿主 API（§6）；OpenAI 新 `shell` 干脆没有 cwd
   字段（§8.3）；Anthropic 的 API 级 bash 规格 schema 不可改（§2.1）。这三种「不加」的理由
   不同：分别是「这是部署的事」「环境已经有确定 cwd」「协议不给你这个位置」。
6. **持久 shell 的维护成本是真实的、有出处的。** sentinel 分帧、进程组超时、
   杀掉后必须重启、后台命令的 cwd 不沿用——都在 Anthropic 官方规格与 Claude Code tools
   reference 里写明（§2.2）。Codex 的做法是折中：短命令新进程，长命令用 `session_id` +
   `write_stdin` 接着跑，cwd 始终由每次调用的参数或 turn cwd 决定。

---

## 11. 未能核实 / 只有二手线索

以下几项**没有**拿到一手材料，正文里不作断言：

- **aider**：未核实其模型侧是否有 shell 工具与 cwd 语义。`raw.githubusercontent.com` 上
  `aider/tools/bash.py`、`aider/tools/__init__.py` 均返回 404（2026-10-09），
  `api.github.com/repos/Aider-AI/aider/contents/aider/tools` 返回 403（匿名速率限制），
  未读 docs.aider.chat。
- **Continue（docs.continue.dev）**：只搜到一篇二手/第三方页面，未读官方 tools 参考，
  不下结论。
- **Cline**：`packages/core/src/tools/executeCommandTool.ts` 在 `main` 上 404，
  `github.com/cline/cline/tree/main/packages/core/src/tools` 的页面里没有该目录内容，
  GitHub API 403，未读 docs.cline.bot。
- **OpenHands 源码**：新的 `OpenHands/OpenHands` 仓库顶层是 Web 前端（配置里只有
  vite/playwright），旧的 `openhands/runtime/impl/action_execution/action_execution_client.py`
  路径 404；本文只采用官方文档里的 agent-server OpenAPI（§7）。
- **Amp、Zed assistant**：完全未核实，没有找到可引用的一手工具 schema 文档。
- **Codex 在 `main` 分支上是否还有名为 `shell` 的工具**：未见；`main` 上是 `exec_command`
  （PTY + session）这套，「`shell` / `container.exec`」只出现在 crates.io 的
  `codex-protocol 0.63.0` 反序列化结构里（§3.4）。

抓取过程中遇到的重定向与失效，如实记录：

- `https://developers.openai.com/codex/security.md` 跨源重定向到
  `https://learn.chatgpt.com/docs/security.md`（跟随被拒），而该目标页是 Codex Security
  产品页，与 shell 沙箱无关；`docs/sandbox.md` 现在只有一行指路
  （`https://raw.githubusercontent.com/openai/codex/main/docs/sandbox.md`），真正页面在
  `learn.chatgpt.com/docs/agent-approvals-security`，本次未抓取。
- `https://raw.githubusercontent.com/openai/codex/main/codex-rs/core/src/tools/handlers/shell.rs`
  → **404**（该文件在历史 commit 里存在，例如 `d807d44a`；`main` 上对应文件是
  `handlers/unified_exec/` 与 `handlers/shell_spec.rs`）。
- `https://raw.githubusercontent.com/openai/codex/main/codex-rs/protocol/src/tools.rs` → **404**。
- `https://docs.rs/codex-protocol/latest/codex_protocol/tools/index.html` → **404**（无 `tools` 模块）。

---

## 12. 对本仓库意味着什么（只列事实对应，不给方案）

「衡 / heng」今天的 `bash` 工具没有 cwd 参数，命令作为单个 argv 元素交给 `bash -lc`，工作区
= 会话 cwd，沙箱（bubblewrap）假定一个确定的工作区，兄弟工具用工作区相对路径。上面的先例
把可选项收敛成三种，每种都有出处：

- **加参数、相对工作区解析**（Gemini `dir_path`、Codex `workdir`、opencode `workdir`）：
  沙箱可写集合可以继续锚在会话工作区，把参数解析结果限制在其内；代价是要在权限判定里把
  「这次命令的工作目录」纳入考虑，并处理「cd 之后相对路径解析」的老问题（opencode 的做法是
  在 schema 描述里禁止 cd）。
- **维持会话状态**（Claude Code）：不加工具参数，但要在宿主记 cwd、处理越界重置，并决定后台
  命令算不算；沙箱可写集合要跟着会话 cwd 走。
- **什么都不加，把 cwd 钉死在会话工作区**（goose、OpenAI 新 `shell`）：模型用绝对路径或
  `cd`；代价是 monorepo 子目录场景要靠命令自己解决，且「模型想换个目录干活」时体验最差。

真正的分歧点在**沙箱与权限**：Claude Code 与 Codex 都把 cwd 当成可写集合的锚，Gemini 与
opencode 则把工作目录参数解析后交给沙箱层处理。要选哪条，先定的是「可写集合锚在哪」，
而不是「模型要不要能填 cwd」。

---

## 附：全部 URL（均为 2026-10-09 抓取）

Anthropic（Claude Code 与 Claude Platform）：

- <https://code.claude.com/docs/en/tools-reference.md> —— Bash 工具行为：每次调用独立进程、
  cd 沿用与越界重置、`CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR`、后台命令不沿用 cd、
  `EnterWorktree` / `ExitWorktree`、工具表里没有 cwd 字段。
- <https://code.claude.com/docs/en/sandboxing.md> —— 默认可写集（工作目录 + 临时目录 +
  `--add-dir`）、Linux 用 bubblewrap、worktree 下放行主仓库 `.git`、受保护路径、只包 shell。
- <https://platform.claude.com/docs/en/agents-and-tools/tool-use/bash-tool.md> ——
  `bash_20250124` 的输入字段表（`command`、`restart`）、schema-less、持久会话与 sentinel
  实现、超时与重启、Limitations。

OpenAI：

- <https://developers.openai.com/api/docs/guides/tools-shell.md> —— 现行 `shell` 工具：
  `shell_call.action` 的字段、hosted 默认工作目录 `/mnt/data`、local 模式示例不设 cwd。
- <https://developers.openai.com/api/docs/guides/tools-local-shell.md> —— legacy `local_shell`：
  `working_directory`、`env`、`timeout_ms`、`user`，以及 `cwd: action.working_directory ??
  process.cwd()` 的官方示例；结束支持日期 2026-02-12。
- <https://raw.githubusercontent.com/openai/codex/main/docs/sandbox.md> —— 现在只有一行指路。
- <https://docs.rs/codex-protocol/latest/codex_protocol/models/struct.ShellToolCallParams.html>
  —— 字段与 crates.io 版本 0.63.0。
- <https://github.com/openai/codex>（`codeload.github.com/openai/codex/tar.gz/refs/heads/main`，
  2026-10-09 快照）—— 下列文件按 `文件:行号` 引用：
  `codex-rs/core/src/tools/handlers/shell_spec.rs:38-158`（`exec_command` 与 `write_stdin`
  schema、审批参数）、`shell_spec.rs:223-273`（`sandbox_permissions` / `justification` /
  `prefix_rule` / `additional_permissions`）、`shell_spec.rs:159-186`（`request_permissions`
  描述里的「Relative filesystem paths resolve against the selected environment cwd」）、
  `codex-rs/core/src/tools/handlers/unified_exec.rs:44-52`、`codex-rs/core/src/tools/handlers/unified_exec/exec_command.rs:191-200`、
  `codex-rs/core/src/session/turn.rs:841-856`、`codex-rs/utils/sandbox-summary/src/sandbox_summary.rs:36,119`。

Google Gemini CLI：

- <https://geminicli.com/docs/tools/shell/> —— `run_shell_command` 的参数表（`dir_path` 等）。
- <https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/tools/shell.ts>
  —— `ShellToolParams`、`validateToolParamValues` 与 `execute` 里的 cwd 解析与工作区校验、
  沙箱扩展权限、每次调用新起进程。
- <https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/services/shellExecutionService.ts>
  —— `execute(commandToExecute, cwd, ...)`、child_process / PTY 两条路径、每次重新构造环境、
  background 进程按 sessionId 记录。

opencode：

- <https://github.com/anomalyco/opencode>（`codeload.github.com/anomalyco/opencode/tar.gz/refs/heads/dev`，
  2026-10-09 快照）—— `packages/opencode/src/tool/shell/prompt.ts:16-24`（参数 schema 与
  workdir 描述）、`packages/opencode/src/tool/shell.ts:611-614`（cwd 解析）、`shell.ts:28`（`CWD`
  命令集合）、`shell.ts:369-380`（`resolvePath` / `argPath`）。

goose：

- <https://github.com/aaif-goose/goose>（`codeload.github.com/aaif-goose/goose/tar.gz/refs/heads/main`，
  2026-10-09 快照）——
  `crates/goose/src/agents/platform_extensions/developer/shell.rs`（`ShellParams` 只含
  `command` 与 `timeout_secs`；`shell_with_cwd` / `shell_with_cwd_and_emitter` 的宿主参数
  `working_dir`）。

OpenHands：

- <https://docs.openhands.dev/sdk/guides/agent-server/api-reference/bash/execute-bash-command.md>
  —— `ExecuteBashRequest` 的 `command` / `cwd` / `timeout` 与 `BashOutput` 的形状。

MCP：

- <https://modelcontextprotocol.io/specification/2025-06-18/client/roots> —— roots 的语义、
  `roots/list`、`listChanged`，以及「协议不规定交互模型」。
