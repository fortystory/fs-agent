# Coding Agent 功能调研：一流实现做了什么，自用极简版该砍什么

> 目的：为 `fs-agent`（一个从零实现的极简但真正可用的 coding agent CLI）提供**设计输入**，不是产品介绍。
> 每个功能点都带标签：**✅ table stakes（可用门槛）** / **🔷 differentiator（差异化）** / **⬜ nice-to-have（锦上添花）**。
> 最重要的结论在第 5 节（砍掉清单）和第 6 节（最小切片），可以先跳过去读。

**调研对象（10 个代表性实现）**

| 实现 | 形态 / 技术栈 | 最值得抄的一点 |
| --- | --- | --- |
| [Anthropic Claude Code](https://code.claude.com/docs/en/overview) | 闭源 CLI，TS/Node | agentic harness 的完整度：工具面、权限模式、context compaction、checkpoint、hooks |
| [OpenAI Codex CLI](https://github.com/openai/codex) | 开源 CLI，Rust | 沙箱与审批的组合（`exec` 非交互 + 容器内自动执行） |
| [aider](https://aider.chat/docs/) | 开源 CLI，Python | **edit format 分层**（whole / SEARCH-REPLACE / udiff）+ repo map + 每次编辑自动 git commit |
| [Cline](https://github.com/cline/cline) | VS Code 插件，TS | Plan/Act 双模式 + 逐工具人工确认 + shadow git checkpoint |
| [OpenHands](https://github.com/OpenHands/OpenHands)（原 All-Hands-AI/OpenHands） | 开源，Python/TS | 用 **event stream** 作为唯一真相源；V1 把沙箱从"强制"改为"可选"；会话级 `fork(from_event_id=...)` |
| [Gemini CLI](https://github.com/google-gemini/gemini-cli) | 开源 CLI，TS | **behavioral evals**（断言工具调用而非文本）的工程化 + 编辑失败的模糊自愈 |
| [opencode](https://github.com/anomalyco/opencode)（原 sst/opencode） | 开源 CLI，TS/Go | client/server 分离的 TUI + 基于 `models.dev` 的 provider 抽象 + 10 层编辑降级匹配 |
| [Goose](https://github.com/aaif-goose/goose)（Block 发起，现由 aaif-goose 维护） | 开源，Rust | extension（MCP）为一等公民 + recipes 可复用任务 |
| [Amp (Sourcegraph)](https://ampcode.com/) | 闭源 CLI | AGENTS.md 共建方；**主张 harness 正在贬值**；权限默认全放行 |
| [Continue](https://github.com/continuedev/continue) | IDE 插件 + CLI | YAML 配置模型角色 + rules 文件 + 动态文件/命令策略。**注意：仓库已 read-only、停止维护（最终版 2.0.0）** |
| [mini-SWE-agent](https://mini-swe-agent.com/latest/)（对照组） | ~100 行 Python | **只用 bash、不用 tool-calling API、线性历史**即可 >74% SWE-bench verified |

选 mini-SWE-agent 作为对照组是有意的：它是"极简 agent 能走多远"的上界证据，直接支撑第 5、6 节的裁剪决策。

> **一个应该先读的行业信号**：2026 年 2 月，Amp（Sourcegraph）发布了一篇题为 [The Coding Agent Is Dead](https://ampcode.com/news/the-coding-agent-is-dead) 的文章，核心论点是——
> "the agent — the prompts and tools you wrap around a model — is no longer the limiting factor… **A simple tool called `bash` is often enough.**"
>
> 也就是说，**harness 本身正在贬值，模型能力才是瓶颈**。这与 mini-SWE-agent 的实测结果（100 行、只有 bash、>74% SWE-bench verified）互为印证。它对本文的裁剪建议是决定性的：**凡是属于"用 scaffold 弥补模型弱点"的功能，都应该是最后才做的，而且要先证明模型确实需要它。** 后文凡是引用这条判断的地方都标为「harness 贬值」。

---

## 1. Agent 循环与工具调用协议

**解决什么问题**：把一次性的 LLM 调用变成能自主多步行动、并知道何时停下来的进程。这是 agent 与 chatbot 的唯一分界线。

**一流实现怎么做**

- Claude Code 官方把循环描述为三阶段：**gather context → take action → verify results**，阶段之间会反复穿插，模型根据上一步的结果决定下一步；用户可以随时打断插话。（[how-claude-code-works](https://code.claude.com/docs/en/how-claude-code-works)）
- Anthropic 明确要求 agent **必须有 stopping condition**，例如最大迭代轮数，因为自主 agent 的成本和错误会复利；同时强调"先用最简单可行的方案，只有在能证明收益时才加复杂度"。（[building-effective-agents](https://www.anthropic.com/engineering/building-effective-agents)）
- mini-SWE-agent 走另一个极端：**线性 history**（每一步只是往 messages 里追加），不用 tool-calling API，只用 bash，用 `subprocess.run` 让每条动作完全独立（而非维护一个有状态的 shell session）——作者称这对 agent 的稳定性是"a big deal"，也让切换沙箱变成替换一个函数。（[mini-swe-agent](https://mini-swe-agent.com/latest/)）
- **aider 是第二个"不用 tool-calling 协议"的成功例子，而且走得更远**：它**完全没有 tool/function 协议**——模型回复的是*某种 edit format 的纯文本*，由 aider 解析（对应不同的 Coder 类）。也就是说，"agent" 不必然需要 provider 的 function calling 能力，解析文本也能做到，而且这带来一个额外好处：**任何模型都能用**。（[aider](https://aider.chat/docs/more/edit-formats.html)）
  - aider 的循环还有一个值得抄的收尾阶段：**apply → auto-commit → lint → shell → test**，其中 lint/test 的错误会变成一个 `reflected_message` 回灌给模型重试，这个反省循环由 `max_reflections = 3` 封顶（注意：它是**类属性而不是 CLI 参数**——自研时把它做成可配的）。（[aider](https://aider.chat/docs/usage/lint-test.html)）
- **停止条件可以做成显式枚举**，Cline 的做法值得直接照抄：`completed | max_iterations | aborted | mistake_limit | error`。比一个布尔"完成没完成"信息量大得多，也让日志和 eval 能区分"正常收尾"和"撞墙退出"。（[Cline SDK events](https://docs.cline.bot/sdk/events)）
- 并行工具调用：一个 assistant 消息里返回多个 tool_use block，宿主并行执行后一起回填。Anthropic 把它列为 tool-use 的一等能力。（[parallel-tool-use](https://platform.claude.com/docs/en/agents-and-tools/tool-use/parallel-tool-use)）
- **Amp 把同一个循环做成了 <400 行 Go 的参考实现**，并给出定义："an agent is a model plus a system prompt plus tools"。（[how-to-build-an-agent](https://ampcode.com/notes/how-to-build-an-agent)、[context-management](https://ampcode.com/guides/context-management)）
- 停止条件是可以做成可插拔的：Amp 的插件事件里 `agent.end` 返回 `{ action: 'continue', userMessage }` 就能自动再开一轮，文档同时警告要自己防死循环；它的流式 JSON 里直接暴露 `stop_reason`（`end_turn`/`max_tokens`/`tool_use`/…）、`num_turns`，以及失败原因 `error_max_turns`、`error_during_execution`。（[plugins](https://ampcode.com/docs/customize/plugins)、[streaming-json](https://ampcode.com/docs/cli/streaming-json)）
- **重复调用守卫**：opencode 有一个内建权限项 `doom_loop`，当"同一个工具调用以完全相同的输入重复 3 次"时默认转人工询问。（[opencode permissions](https://opencode.ai/docs/permissions/#available-permissions)）
- goose 把轮数和重复次数做成显式 CLI 参数：`--max-turns`（默认 **1000**）、`--max-tool-repetitions`；goose 的并行工具调用是把每个调用做成 future 后用 `stream::select_all` 合并。（[goose CLI](https://goose-docs.ai/docs/guides/goose-cli-commands/)、[agent.rs](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/agent.rs)）
- **一条很容易写错、但会直接影响正确性的细节（来自 opencode 的实现）**：它的会话处理器返回 `"compact" | "stop" | "continue"` 三态，而 **`stop` 的判据是"被阻塞或出错"，不是 provider 返回的 finish reason**——**只要还有待执行的 tool call，即使 provider 报了 `stop`，这一轮也要继续**。（[processor.ts](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/processor.ts)）
  - **对自研 agent 的直接要求**：终止判断必须自己算（"本轮没有工具调用 **且** 没有待处理的工具结果"），**绝不能直接信任 `stop_reason`**。这是一个一旦写错就很难察觉、表现为"工具调用被静默丢弃"的 bug。opencode 的循环是手写 `while(true)` 反复读取持久化历史，而不是依赖 SDK 的会话概念。（[prompt.ts](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/prompt.ts)）
- **但要注意：也有实现刻意不设轮数上限。** Codex 的 `run_turn` 就是裸 `loop {}`，多轮、**没有 max-turns**；Gemini CLI 则是 `MAX_TURNS = 100`（每个 prompt）+ 可配的 `model.maxSessionTurns`。（[codex](https://github.com/openai/codex)、[gemini-cli](https://github.com/google-gemini/gemini-cli)）
  - 取舍很清楚：**有上限是防呆（对自研 agent 建议保留），没上限是信任模型的收尾能力。** 上限应该做成可配的参数而不是硬编码常量。
- **并行工具调用的默认值在两家之间是相反的**，这是本次调研里最值得注意的一个设计分歧：
  - **Gemini CLI 默认开启**（`Promise.all` 并发执行），但把 `replace` / `write_file` / `update_topic` **强制串行**。
  - **Codex 默认串行**——`supports_parallel_tool_calls` 默认 `false`，只有 exec / read / MCP 这类工具显式选择加入，实现上用 `FuturesOrdered` + 每工具的 `RwLock`。（[parallel.rs](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/parallel.rs)）
  - 两家共同点才是结论：**写操作必须串行化。** 这一点在 Claude Code 上是明文规则："Read-only tools (like Read, Glob, Grep, and MCP tools marked as read-only) can run concurrently. Tools that modify state (like Edit, Write, and Bash) run sequentially."，自定义工具默认串行，除非声明 `readOnlyHint`。（[agent-loop](https://code.claude.com/docs/en/agent-sdk/agent-loop)）
  - **可直接照抄的判定规则：按"有无副作用"给工具打标，只读并发、有副作用串行。** 这比"某个具体工具能不能并行"更本质，也更容易实现。
  - **第四、第五个数据点也印证这条规则**：Cline 支持并行但挂在 `enableParallelToolCalling` 开关后面，且**按模型家族区分**（GPT-5 恒开、Gemini 3 是开关、Claude 4+ 只在措辞上支持），MCP 工具保持串行；Continue 的并行**只对只读工具开放**，而且这条规则只写在 system prompt 里、不在文档里。（[Cline](https://github.com/cline/cline/commit/6d1bfc3a1ba46d5bf762600d4fdead01e5526f48.patch)、[defaultSystemMessages.ts](https://raw.githubusercontent.com/continuedev/continue/main/core/llm/defaultSystemMessages.ts)）
  - **综合起来**：三个默认值（Gemini 开 / Codex 串行 / OpenHands `tool_concurrency_limit = 1`）加上两家"仅只读"的限制，说明**并行是加分项而非必需项；但一旦做，就必须按副作用分类**。

**最小可用 vs 进阶**

- 最小可用：`while` 循环 —— 调用模型 → 若有工具调用则执行并把结果追加进 messages → 若模型没有工具调用则退出；外加一个硬性 `max_steps`（建议 30~50）。
- 进阶：并行工具调用（同一轮内多个只读工具并行：读多文件、并行 grep）；流式增量 tool_use；每步 verify（自动跑测试/类型检查，失败则把错误喂回循环）；从任意中断点恢复。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| 多轮循环 + 追加 tool result | ✅ | 没有它就不是 agent |
| 最大轮数 / 停止条件 | ✅ | 没有它一次失控就是无限烧钱；实现成本 1 行。goose 默认 1000 轮、Amp 在流式输出里专门报 `error_max_turns`，说明这是必须显式建模的东西 |
| 模型无工具调用即停止 | ✅ | 最自然的终止信号 |
| 重复工具调用守卫（同输入重复 N 次） | 🔷 | opencode 的 `doom_loop` 证明了这个场景真实存在；实现是一个 dict 计数器，是"卡死检测"最便宜的一种 |
| 并行工具调用 | 🔷 | 只读工具并行能显著压缩 wall-clock，但要处理"同一轮多个结果的排序/部分失败"。注意**两家的默认值是相反的**（Gemini 默认并发、Codex 默认串行），而**写操作必须串行化**是共识。先做串行，等真的嫌慢再并行 |
| 并行前先串行化写操作 | 🔷 | Gemini 强制 `replace`/`write_file` 串行、opencode 用 per-path Semaphore；做并行的那一天它是前提 |
| 显式三阶段状态机（gather/act/verify） | ⬜ | 好的循环自然会长成这样，硬编码成状态机反而限制模型 |
| 流式 tool_use 增量解析 | ⬜ | 体验优化，非能力 |

---

## 2. 工具集合与编辑策略（ACI，最影响成败的一环）

**解决什么问题**：模型与文件系统/终端的接口。**编辑是整条链路失败率最高的地方**——模型生成的 old_string 少一个空格就整块失败。Anthropic 自述在 SWE-bench 上"花在优化工具上的时间比优化整体 prompt 更多"，并举例：把文件路径参数从相对路径强制改为绝对路径后，模型的路径错误消失了。（[writing-tools-for-agents](https://www.anthropic.com/engineering/writing-tools-for-agents)、[building-effective-agents](https://www.anthropic.com/engineering/building-effective-agents)）

**一流实现的工具面（Claude Code 为参照）**（[tools-reference](https://code.claude.com/docs/en/tools-reference)）

| 工具 | 关键设计细节（值得抄的） |
| --- | --- |
| `Read` | 返回带行号内容；超 token 上限返回首页 + `PARTIAL view` 提示 + offset/limit 续读；支持图片/PDF/notebook |
| `Write` | 整文件覆写；**未读过的已存在文件默认拒绝覆写**（read-before-write） |
| `Edit` | **精确字符串替换**，`old_string` 必须唯一且逐字符匹配，无正则、无模糊匹配；必须 read-before-edit；不唯一时要求模型扩大上下文或显式 `replace_all` |
| `Bash` | 每条命令独立进程；命令级 timeout 参数 + 默认 2 分钟/上限 10 分钟；输出流式落盘，超过 ~30k 字符只回文件路径 + 预览，失败时只回 ~10k head-tail 摘录；退出码语义有白名单（`grep`/`diff`/`test` 的 exit 1 算成功） |
| `Glob` | 按修改时间排序，**上限 100 条并返回截断标志**；默认不遵守 `.gitignore` |
| `Grep` | 基于 ripgrep（非 POSIX 正则）；遵守 `.gitignore`；三种输出模式 `files_with_matches` / `content` / `count`，带 `head_limit`/`offset` |
| `WebFetch` / `WebSearch` / `LSP` / `NotebookEdit` | 领域专用工具 |

**编辑格式的选择是一道真实的设计题**。aider 把它做成了显式可配置的一等概念（`--edit-format`），并为不同模型选最优格式（[edit-formats](https://aider.chat/docs/more/edit-formats.html)）：

- `whole`：返回整个文件。最简单、最不容易解析失败，但慢且贵。
- `diff`：`<<<<<<< SEARCH / ======= / >>>>>>> REPLACE` 块，只返回改动部分。**这是 aider 的默认思路**。
- `udiff`：简化版 unified diff，主要给 GPT-4 Turbo 用（因为其他格式让它"偷懒"，把大段代码替换成 `# ... original code here ...`）。
- `editor-diff` / `editor-whole`：architect 模式下给 editor 模型用的窄化 prompt。

**跨实现的收敛结论（值得单独强调）**：**"精确 search-replace" 是编辑的主流，但有两个重要的例外，而 AST 编辑依然是零家。**

- Gemini CLI 的 `replace` 工具就是 `{file_path, old_string, new_string, instruction, allow_multiple}`；仓库级检索确认**既没有 unified diff 工具，也没有任何 AST 编辑工具**（verified absence）。它的工具描述甚至明文要求 "Include at least 3 lines of context BEFORE and AFTER the target text"。（[edit.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/edit.ts)）
- opencode 的 `edit` 同样是 `{filePath, oldString, newString, replaceAll?}`，文档描述为 "exact string replacements"。（[opencode tools](https://opencode.ai/docs/tools/#edit)）
- **但这里有个容易看错、却很重要的地方**：opencode **不是"两种策略都给"，而是按模型 id 二选一、互斥**。`tool/registry.ts` 的判定是 `usePatch = modelID.includes("gpt-") && !includes("oss") && !includes("gpt-4")`——命中时 `apply_patch` **替换掉** `edit` 和 `write`，否则才用带 10 层降级匹配的 search-replace `edit`。（[registry.ts](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/registry.ts)、[edit.ts](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/edit.ts)）
  - **这条极值得学**：编辑格式不是"哪个更好"，而是"哪个模型更能写对"——**所以它应该是可配置项，且默认值最好由模型决定**。这与 aider 的 `--edit-format` 是同一个洞察，只是 opencode 把决策自动化了。
- **例外一（Codex）：它压根不用 search-replace，而是自定义的 freeform patch DSL。** `apply_patch` 用一套 Lark V4A 语法（`*** Begin Patch` / `*** Update File:` / `*** End Patch`），**不是 JSON 参数、也不是搜索替换**。（[parallel.rs](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/parallel.rs)）
- **例外二（Codex，更激进）：核心工具集里没有 `read_file` / `write_file` / `grep` / `glob`**——读文件走 `exec_command`（即 shell）。也就是说，一个主流 agent 可以只保留"patch + shell"两个原语。（[codex](https://github.com/openai/codex)）
  - 这条直接支撑第 5 节的裁剪结论：**专用 grep/glob 工具是优化项，不是必需品**。

**真正的差异化在"编辑失败之后"**。三个独立项目各自实现了同一类自愈层，这是我在这次调研里认为最值得抄、也最容易被自研 agent 忽略的一点：

- **opencode 在精确匹配之上叠了 10 层 fallback replacer**，顺序为：`SimpleReplacer → LineTrimmedReplacer → BlockAnchorReplacer → WhitespaceNormalizedReplacer → IndentationFlexibleReplacer → EscapeNormalizedReplacer → TrimmedBoundaryReplacer → ContextAwareReplacer → MultiOccurrenceReplacer`。文件头注明这些源自 **cline 的 `diff-apply` 与 gemini-cli 的 `editCorrector`**。（[edit.ts](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/edit.ts)）
- **Gemini CLI 用 Levenshtein 模糊匹配 + LLM 自我修正**：匹配策略类型为 `'exact' | 'flexible' | 'regex' | 'fuzzy'`，`FUZZY_MATCH_THRESHOLD = 0.1`；失败时 `attemptSelfCorrection()` 调用 `FixLLMEditWithInstruction(...)` 后重试，并发出 `EditCorrectionEvent`；同时用 **SHA-256 校验磁盘内容**以发现外部修改。（[llm-edit-fixer.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/utils/llm-edit-fixer.ts)）
- 反"偷懒"的护栏也值得抄：Gemini 的 `write_file` 会**拒绝省略占位符**——如果 `content` 里出现 "rest of methods ..." 这类东西，直接报错 "Provide complete file content."；opencode 有一个 `isDisproportionateMatch` 检查，当匹配到的片段远大于 `oldString` 时拒绝应用。（[write-file.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/write-file.ts)、[edit.txt](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/edit.txt)）
- 并发细节：opencode 用按解析后路径为 key 的进程内 `Semaphore` **串行化对同一文件的写入**——并行工具调用下这是必须的。（[edit.ts](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/edit.ts)）

- **工具面可以小到什么程度，goose 给了下界**：它的 Developer extension **只有 5 个工具**——`shell` / `write` / `edit` / `tree` / `read_image`（有单元测试把这个列表钉住），`edit` 的参数是 `before` / `after` 且要求精确唯一匹配。**没有 `glob`、没有 `grep`、没有 `read`、没有 `patch`**——列表、搜索、读文件全部走 `shell`。（[developer/mod.rs](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/developer/mod.rs)）
  - 这与 Codex（只有 shell + patch）互为印证：**"4~5 个工具"是经过验证的可用下界。**

**最小可用 vs 进阶**

- 最小可用：`read_file` + `write_file`（整文件）+ `edit_file`（search-replace，要求唯一匹配，不唯一就报错让模型重试）+ `run_bash`。**不要一开始就做 unified diff 解析**——模型算不准 hunk header 行数，Anthropic 也建议"格式要贴近模型在互联网文本里自然见过的样子，不要有需要精确计数的 overhead"。（[building-effective-agents](https://www.anthropic.com/engineering/building-effective-agents)）
- 进阶：多文件补丁（一次调用改多个文件，原子性 or 逐个报告）；unified diff；read-before-edit 约束；工具输出按 token 预算截断；工具描述本身当 prompt 工程做（含示例、边界、参数命名要"poka-yoke"）；编辑后自动 lint/test 并把错误喂回循环——aider 的 `--lint-cmd` / `--test-cmd` / `--auto-test` 就是这个思路，命令返回非 0 即视为需要修复（[lint-test](https://aider.chat/docs/usage/lint-test.html)）。
- AST/tree-sitter 编辑：**没有代表性实现把它当作编辑主路径**。aider 只把 tree-sitter 用在 repo map 的符号抽取上，不用来生成编辑（[repomap](https://aider.chat/docs/repomap.html)）。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| read / write / bash | ✅ | 最小三件套 |
| edit：精确 search-replace | ✅ | 比整文件重写省 token、比 diff 更好生成；Claude Code / Gemini CLI / opencode / cline 全部采用 |
| 编辑前必须读过文件 | ✅ | 一行检查，直接消灭一类"凭想象改文件"的事故 |
| 工具输出截断 + 落盘 | ✅ | 一条 `cargo test` 就能吃掉整个上下文窗口 |
| **编辑失败后的自愈（trimmed / whitespace-normalized / indentation-flexible 等降级匹配）** | 🔷 | 这是"能用"和"顺手"的分水岭：opencode、cline、gemini-cli 三家独立收敛到同一设计。不需要 10 层，**先做 2 层（trim 后匹配、忽略末尾空白）就能吃掉大部分无谓失败** |
| 拒绝"省略占位符"的写入 | 🔷 | 一行正则检查，直接消灭模型"把中间代码省略成注释"这一整类事故 |
| 专用 grep / glob 工具 | 🔷 | 有 bash 时可以用 `rg`/`ls` 替代，但专用工具能给出结构化、可截断、可排序的结果，且弱模型更容易用对 |
| 编辑后自动 lint/test 并回灌错误 | 🔷 | 成本极低、收益很大：把"verify"从模型的自觉变成 harness 的保证 |
| 同文件写入串行化 | 🔷 | 做并行工具调用的那一刻就变成必需项，否则两个 edit 可能互相覆盖 |
| 多文件补丁（`apply_patch` 风格） | ⬜ | 顺序调用 edit 也能完成。注意 opencode 是把它作为 search-replace 的**替代品按模型二选一**，不是叠加 |
| unified diff 编辑格式 | ⬜ | 全部调研对象里只有 aider 在用，且是为了迁就特定模型的怪癖 |
| AST / tree-sitter 编辑 | ⬜ | **零个代表性实现把它当编辑路径**（Gemini CLI 已作 verified absence）；投入产出比最低的一项 |

---

## 3. 上下文管理

**解决什么问题**：上下文窗口是 agent 最硬的物理约束。Claude Code 的文档直接把它列为"首要约束"（managing context as your primary constraint）。上下文既决定能力上限，也决定每一轮的成本。

**一流实现怎么做**

- **启动即加载**。Claude Code 启动时（用户还没输入任何东西）上下文里已经有：system prompt（约 4.2k tokens）、auto memory `MEMORY.md`（前 200 行或 25KB）、环境信息（cwd / 平台 / shell / git 状态）、MCP 工具名（默认只列名字，schema 按需加载）、skill 的一行描述、全局 `~/.claude/CLAUDE.md`、项目 `CLAUDE.md`。（[context-window](https://code.claude.com/docs/en/context-window)）
- **Compaction（自动压缩）**。接近上限时自动把对话换成结构化摘要。关键细节是"压缩后什么会被重新注入"——这是设计上最容易漏的地方（[context-window](https://code.claude.com/docs/en/context-window)）：

  | 内容 | 压缩后 |
  | --- | --- |
  | system prompt / output style | 仍生效 |
  | 项目根 CLAUDE.md、无路径作用域的 rules | 从磁盘重新注入 |
  | auto memory | 从磁盘重新注入 |
  | plan mode 里写的计划 | 从磁盘重新注入 |
  | 带 `paths:` 的 rules、子目录嵌套 CLAUDE.md | 等模型读到匹配文件时重新加载 |
  | 模型读过/改过的文件 | **重读最近修改的最多 5 个** |
  | 已调用的 skill body | 重新注入，每 skill 上限 5k、总上限 25k tokens，超了丢最老的 |

- **工具结果截断**。Anthropic 明确说 Claude Code 默认把工具响应限制在 25,000 tokens，并建议工具实现"分页/范围选择/过滤/截断 + 合理默认值"，同时用截断提示引导模型改用更省 token 的策略（"多做小而定向的搜索，别做一次宽泛搜索"）。（[writing-tools-for-agents](https://www.anthropic.com/engineering/writing-tools-for-agents)）
- **检索式上下文（repo map）**。aider 用 tree-sitter 抽取全仓符号（类/函数签名），再用图排序算法（把文件当节点、依赖当边）挑出最相关的部分塞进预算；`--map-tokens` 默认 1k 并随对话状态动态伸缩——没加文件时会扩张到几乎整仓。（[repomap](https://aider.chat/docs/repomap.html)）
- **上下文预算是"算出来的"而不是"配出来的"**：opencode 的溢出判断用的是 `limit.input − min(20_000, maxOutputTokens)`——即从模型输入上限里预留出"给模型写输出"的空间，且预留量不超过 20k。（[overflow.ts](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/overflow.ts)）
  - **值得直接抄这个公式**：手写一个魔法数字阈值（如"8 万 token 就压缩"）会在换模型后立刻失效；用"输入上限 − 输出预留"则自动适配任何模型。
- **隔离而非压缩**。把大读取丢给 subagent，只有最终摘要回到主上下文（例：subagent 读了 6.1k tokens 的文件，只回 420 tokens）。（[context-window](https://code.claude.com/docs/en/context-window)）
- 用户的干预接口：`/context` 看占用明细、`/compact [focus 指令]`、`/clear`、`/autocompact 500k` 调阈值。（[context-window](https://code.claude.com/docs/en/context-window)）
- **一个重要的反方观点：Amp 把 compaction 整个删掉了**，理由是 "compaction is lossy and encourages long meandering threads"，改成让用户显式执行 `/handoff <goal>`——分析当前 thread，生成一份草稿 prompt + 文件清单，供你在**新** thread 里继续。（[handoff](https://ampcode.com/news/handoff)）
  - 这条对自研 agent 的含义：自动 compaction 是一个**有争议**的设计，不是必然。它的真实价值在"让长会话不爆窗"，代价是丢掉逐字历史。若不做自动 compaction，就必须给用户一条"手动开新会话 + 交接"的路径。
  - **后续发展（已核实）**：Amp 随后又把 handoff 也删掉，**恢复为自动 compaction，阈值是上下文 90% 满时触发**。也就是说它转了一圈回到原点——**自动 compaction 最终还是留下了**，但它换成了"自动触发"而不是"用户手动 handoff"。Amp 的静态文档仍在讲 handoff，所以是可查证的文档滞后。（[neo](https://ampcode.com/news/neo)）
  - **对自研 agent 的结论**：做自动 compaction，阈值取 ~90%，保留手动触发入口。这是八家实现里唯一"试过删掉又装回来"的组件，说明它的价值是被实测确认过的。

**最小可用 vs 进阶**

- 最小可用：**估算 token（字符数/4 就够用）+ 上限阈值 + 超限时从最老的工具结果开始丢（保留 system prompt、项目规则、最近 N 轮）**。不要做 embedding 检索。
- 进阶：自动 compaction（结构化摘要，并明确"哪些内容从磁盘重新注入"）；工具结果落盘 + 预览；`@file` 引用展开；`.gitignore` 感知；repo map；图片/PDF 按需降采样。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| token 估算 + 工具输出上限 | ✅ | 没有它 agent 跑十几轮必然爆窗 |
| 超限时丢弃最老的工具结果 | ✅ | 20 行代码，覆盖 90% 的长任务场景 |
| 自动 compaction（摘要） | ✅（可后置，且有争议） | 长任务必需，但可以先做"粗暴丢弃"顶一阵。Amp 公开质疑它是有损的——所以它不是必须自动化的东西，**"能开新会话 + 能交接"是更本质的需求** |
| compaction 后重新注入规则文件 / 重读最近文件 | 🔷 | 决定压缩后 agent 是否"失忆"，是 compaction 做得好不好的分水岭 |
| `@file` 引用 | 🔷 | 交互体验，可先用直接贴路径代替。注意要**限制单文件注入量**：Amp 的做法是文本截断到 500 行 / 每行 2KB，图片按图片传，二进制直接忽略 |
| repo map / 符号检索 | ⬜ | 需要 tree-sitter + 图排序，是一个独立项目；对中小仓库收益有限 |
| 向量检索 / RAG | ⬜ | 代表性的 agent 里没有以它为核心卖点的；grep + 模型自己找更可解释 |
| `.gitignore` 忽略规则 | 🔷 | 廉价且能显著减少噪声（尤其 `node_modules`/`target`） |

---

## 4. 系统提示与项目规则文件

**解决什么问题**：每个项目的构建命令、测试命令、代码风格都不同，而 agent 每开一个新会话都从零开始。

**一流实现怎么做**

- **AGENTS.md 已经成为跨工具的事实标准**：官网自称 60k+ 开源项目采用，被 Codex、Amp、Jules、Cursor、Factory、aider、goose、opencode、Zed、Warp、VS Code、Devin、Copilot 等采用，现由 Linux Foundation 下的 Agentic AI Foundation 托管。几个关键约定：**无必需字段**（就是普通 Markdown）；**离被编辑文件最近的 AGENTS.md 生效**（monorepo 里每个子包可以放一份，OpenAI 主仓自己有 88 份）；**冲突时最近的赢，用户显式指令覆盖一切**。（[agents.md](https://agents.md/)）
- Claude Code 用的是更重的一套：`CLAUDE.md` 层级（用户级 `~/.claude/CLAUDE.md` + 项目级 + 子目录嵌套）＋ 带 `paths:` frontmatter 的**路径作用域规则**（读到 `src/api/**` 下的文件才加载 `.claude/rules/api-conventions.md`）＋ auto memory（agent 自己攒的 `MEMORY.md`）。（[context-window](https://code.claude.com/docs/en/context-window)、[memory](https://code.claude.com/docs/en/memory)）
- 各家的差异只是文件名：Gemini CLI 用 `GEMINI.md`（`settings.json` 里 `context.fileName` 可改）、Cline 用 `.clinerules`、Goose 用 `.goosehints`。**aider 是唯一需要手动配置的**：它**不自动加载 `AGENTS.md`**（仓库级检索确认无内建支持），必须在 `.aider.conf.yml` 里写 `read: AGENTS.md`；它自己的约定文件是 `CONVENTIONS.md`。**OpenHands V1 则推荐 `AGENTS.md` 并同时解析 `CLAUDE.md` / `GEMINI.md` / `.cursorrules`。**（[AGENTS.md](https://agents.md/)、[OpenHands](https://docs.openhands.dev/)）
- **注意 Gemini CLI 里 `save_memory` 工具并不存在**——系统提示里明文写着 "There is no `save_memory` tool"，记忆是靠模型自己去编辑 Markdown 文件。（[memory.md](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/memory.md)）
- **Amp 的查找顺序是三个实现里最完整的，也是最值得抄的**（[agents-md](https://ampcode.com/docs/customize/agents-md)）：
  - cwd / 编辑器工作区根 **以及一直到 `$HOME` 的所有父目录**里的 `AGENTS.md`——**总是注入**；
  - 子树的 `AGENTS.md`——**当 agent 读到该子树里的文件时**才注入；
  - 机器级：`$HOME/.config/amp/AGENTS.md`、`/etc/ampcode/AGENTS.md`（Linux）；
  - 兜底：没有 `AGENTS.md` 时去找 `AGENT.md`（无 S）**或 `CLAUDE.md`**；
  - 在 agent 文件里用 `@` 引用其它文件时，可用 YAML frontmatter 的 `globs:` 做**条件包含**（只有 agent 读过匹配 glob 的文件后才引入）；
  - 有 `agents-md list` 命令可以查看实际加载了哪些文件——**这个"可验证性"设计很值得学**。
- AGENTS.md 官网的一个明确承诺：**如果里面写了测试命令，agent 会去执行它们并在收工前修掉失败**。
- **一个容易被忽略但很关键的实现细节：Claude Code 是把 `CLAUDE.md` 作为"system prompt 之后的一条 user message"注入的，而不是拼进 system prompt。** 官方文档同时明确写着 "Claude Code reads `CLAUDE.md`, not `AGENTS.md`."（[memory](https://code.claude.com/docs/en/memory)、[output-styles](https://code.claude.com/docs/en/output-styles)）
  - 为什么这值得注意：**放进 user message 意味着它不破坏 system prompt 的缓存前缀，也意味着它在语义上是"用户说的话"而非"系统的铁律"**。对自研 agent 的直接启示是——把规则文件放在 system prompt 尾部会造成"改一个 AGENTS.md 就废掉整个 prompt cache"的后果（见第 11 节），放进第一条 user message 更划算。

**最小可用 vs 进阶**

- 最小可用：启动时读 `./AGENTS.md`（不存在就静默跳过），拼到 system prompt 末尾。**成本约 10 行代码，收益是整个功能域里最高的。**
- 进阶：向上/向下查找层级；`paths:` 作用域规则；用户级全局文件；compaction 后重新注入；提供了一个让 agent 自己写记忆的机制。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| 读一个 `AGENTS.md` 注入 system prompt | ✅ | 10 行换掉用户每轮的重复交代；且直接对齐生态（60k+ 项目、十几个 agent 共用） |
| system prompt 里写清工具使用约定与停止条件 | ✅ | 工具描述之外的"行为契约" |
| 向上查找父目录的 `AGENTS.md` | 🔷 | 多一层循环，就能让 monorepo 子目录 / 用户级约定自动生效；Amp 的 `agents-md list` 式可验证输出更值得抄 |
| 层级 / 路径作用域的规则文件（`paths:` / `globs:`） | ⬜ | 只在大型 monorepo 上体现价值 |
| auto memory（agent 自己写笔记） | ⬜ | 有污染上下文的风险，收益不明确；Gemini CLI 明确不做（无 `save_memory` 工具） |

---

## 5. 权限与安全

**解决什么问题**：给了 agent shell，就等于给了它任意代码执行；而它会读入不可信内容（issue、网页、依赖源码），存在 prompt injection 诱导它执行危险操作的风险。

**一流实现怎么做**（Claude Code 的模型最完整，[permission-modes](https://code.claude.com/docs/en/permission-modes)）

| 模式 | 不询问就能做的事 |
| --- | --- |
| `default`（Manual） | 只读 |
| `acceptEdits` | 读 + 文件编辑 + 常见文件命令（`mkdir`/`touch`/`mv`/`cp`） |
| `plan` | 只读 + 探索，编辑被阻塞直到批准计划 |
| `auto` | 几乎全部，由**另一个分类器模型**在后台裁决 |
| `dontAsk` | 只做 `permissions.allow` 白名单里的工具——**专为 CI/脚本** |
| `bypassPermissions` | 全部（文档明确警告：只应在容器/VM 里用） |

值得抄的几个细节：

- **deny 规则在所有模式下都生效，包括 `bypassPermissions`**；allow 规则在 `bypassPermissions` 下无效。
- **protected paths**：`.git`、`.claude`、`.bashrc`/`.zshrc`、`.npmrc`、`.mcp.json`、`.devcontainer` 等敏感路径的写入永不自动批准（除 bypass）。注意实现顺序：安全检查在 allow 规则**之前**跑，所以 `Edit(.claude/**)` 这样的 allow 规则无法绕过它。
- **critical paths**：`rm`/`rmdir` 打到文件系统根、`/usr`、`/etc`、家目录、cwd 及其父目录时，**连 allow 规则和 `PreToolUse` hook 返回 "allow" 都不能批准**——这是防模型手滑的断路器。命令替换（`$(...)`、反引号）里的删除也照样能识别。
- `--dangerously-skip-permissions` 在 Linux/macOS 以 root/sudo 运行时会**拒绝启动**。
- 有 Bash 沙箱（macOS/Linux/WSL2）作为权限之外的第二层隔离（[sandboxing](https://code.claude.com/docs/en/sandboxing)）。
- MCP 规范层面要求：**host MUST obtain explicit user consent before invoking any tool**，并且工具描述（annotation）本身要当作不可信内容。（[MCP spec](https://modelcontextprotocol.io/specification/2025-06-18)）

**其他实现的做法（这里的分歧最大，值得看清楚再选边）**

| 实现 | 默认姿态 | 最值得抄的机制 |
| --- | --- | --- |
| Claude Code | `default` = 只读免问 | 六档模式 + protected/critical path 断路器；`dontAsk` 专供 CI |
| **Amp** | **默认完全不询问**："By default, Amp does not ask for approval before running tools." | `amp.permissions` 每条规则 = `{tool（支持 `mcp__*` 等 glob）, matches（参数 glob，如 `{"cmd": "*git commit*"}`）, action}`，action ∈ **`allow` / `reject` / `ask` / `delegate`**——**`delegate` 把判定交给 `$PATH` 上的一个外部程序**；first-match-wins。（[tool-level-permissions](https://ampcode.com/news/tool-level-permissions)） |
| **opencode** | 大部分 `allow` | allow/ask/deny 三态、**last-match-wins**、`external_directory` 路径白名单、`doom_loop` 守卫；**`.env` 系列默认 deny**（`*.env.example` 例外）；plan 模式就是用权限实现的（`edit`+`bash` 设为 `ask`）；CI 里直接 `OPENCODE_PERMISSION: '{"bash": "deny"}'`。（[permissions](https://opencode.ai/docs/permissions/)） |
| **goose** | **`auto`（完全自主）是默认值** | 四档 `auto` / `approve` / `smart_approve`（按风险分类，**由 LLM 判断读写**）/ `chat`（完全不改文件）；另有 `GOOSE_ALLOWLIST` 限制可用 extension。（[goose permissions](https://goose-docs.ai/docs/guides/managing-tools/goose-permissions/)） |
| **Cline** | 逐工具类别 Auto-Approve | **没有固定命令白名单**："The model marks each command with a `requires_approval` flag based on the command and arguments."（[auto-approve](https://docs.cline.bot/features/auto-approve)） |
| Gemini CLI | `--approval-mode` | `ToolConfirmationOutcome` 有 `proceed_once` / `proceed_always` / `proceed_always_and_save` / **`modify_with_editor`**——**允许你在批准前打开 `$EDITOR` 手改工具参数**。（[modifiable-tool.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/modifiable-tool.ts)） |
| **goose**（补充） | `GOOSE_MODE` 默认 `auto` | 另有 **`permission.yaml`**：`always_allow` / `ask_before` / `never_allow`，且为 `user` 与 `smart_approve` 两个**不同主体**分别配置（[permission.rs](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/config/permission.rs)） |
| **Codex** | 批准模式 `untrusted` / **`on-request`（默认）** / `granular` / `never` | 执行策略写成 **Starlark**：`prefix_rule` / `network_rule`，判定结果为 `Allow` / `Prompt` / `Forbidden`——**策略即数据，可以在不重编译的情况下改**。（[codex](https://github.com/openai/codex)） |

- Gemini CLI 的权限还可以用 **TOML 策略引擎**表达：`~/.gemini/policies/` 下按 tier 1–5 排序，动词是 `allow` / `deny` / `ask_user`。（[gemini-cli](https://github.com/google-gemini/gemini-cli)）
- 沙箱的实现分层也值得记录（按平台）：Codex 用 **Seatbelt（macOS）/ bubblewrap + seccomp（Linux）/ restricted-token（Windows）**；Gemini 支持 docker / podman / `sandbox-exec` / `runsc` / `lxc`，macOS 侧还有 6 种 Seatbelt profile。
- **Amp 更进一步，直接否定了"静态检查命令"这条路**：它的默认权限已改为"完全不询问"，理由原文是——"checking whether a tool call contains `rm -rf` gives you a false sense of security"，因为模型会 "write throwaway scripts"，即危险动作根本不长成能被字符串匹配到的样子。（[neo](https://ampcode.com/news/neo)）
- **三条"反直觉但重要"的诚实声明**，自研时值得照抄这种态度：
  - Cline 明确写：`.clineignore` **不是安全边界**——被忽略的文件仍可通过显式 `@` 引用或 shell 命令读到，并且真正的强制手段是 `PreToolUse` hook。（[clineignore](https://docs.cline.bot/customization/clineignore)）
  - Amp 明确写：它默认不问，且 "Untrusted repositories, MCP servers, and other external inputs can influence what Amp does"，建议用 policy plugin 或隔离环境。（[tools](https://ampcode.com/docs/tools)）
  - 加上 Amp 对黑名单的否定——**这三条合起来说明：命令黑名单的价值是"减少误伤"，不是"抵抗攻击"，也不该被当成主要防线。** 建议仍然做（成本 10 行），但要在文档里和 UI 上讲清它的边界。
- **秘密外泄防护**是一个常被忽略的域：Amp 在系统最底层做 secret redaction，命中即替换为 `[REDACTED:amp]`，覆盖 AWS/GCP/Azure 凭据、GitHub/GitLab token、OpenAI/Anthropic key、Stripe/Slack/npm token 等，并明确声明是 best-effort。（[security](https://ampcode.com/security)）

**最小可用 vs 进阶**

- 最小可用：三档 `readonly` / `ask`（写文件、跑命令都先问）/ `auto`（`--yes`）；把路径限制在 cwd 内；一个硬编码的危险命令黑名单（`rm -rf /`、`sudo`、`curl|sh`、`git push --force`、`:(){:|:&};`）。
- 进阶：路径/命令级 allow-deny 规则与优先级；AST 级（而非字符串级）命令解析以避免 `rm -rf /` 被拼出来绕过；独立分类器模型；hooks 可编程裁决（`PreToolUse` 返回 allow/deny/ask）；真正的 OS 沙箱（seatbelt / landlock / 容器）。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| 至少两档模式：需确认 / 自动（`--yes`、`--dangerously-skip-permissions`） | ✅ | 交互时是安全感，headless 时是前提。注意默认档的选择是有分歧的：Claude Code/Amp/goose 的默认都偏放行，你自己用建议默认"需确认" |
| 写操作与命令执行的逐次确认 | ✅ | 最小实现的 permission gate 就是"打印命令 + 等 y/n" |
| 路径限制在项目目录 | ✅ | 防误伤整个 home 目录；opencode 的 `external_directory` 是把它做成了可配置项 |
| 危险命令黑名单 | ✅（但**不是安全边界**） | 黑名单必然有绕过（`$(...)`、base64、别名、**模型自己写临时脚本**）；Cline 文档自己承认没有固定白名单这回事，Amp 更直接地说这种静态检查 "gives you a false sense of security"。价值在于挡住"模型手滑"，不挡攻击者——**做，但别把它当防线** |
| 用语法分析而非字符串匹配来判定 bash 权限 | 🔷 | opencode 用 tree-sitter 解析命令得到结构化形式，再配一张 arity 字典（如 `npm run` 后面跟 3 个参数）判断参数个数。这比正则黑名单本质得多——**它是"看得懂的检查"，黑名单只是"看得见的检查"**。自用版至少应做到"解析出顶层命令名"，而不是对整个命令串做子串匹配 |
| 批准的粒度：once / always（记住模式）/ reject | 🔷 | opencode 的 `always` 会由工具给出一个安全前缀（如 `git status*`）作为建议白名单；这比"每次都问"和"全放行"两个极端都实用 |
| 批准前可手改工具参数 | ⬜ | Gemini 的 `modify_with_editor`（打开 `$EDITOR` 改参数再执行）是个很聪明的交互，但属于打磨项 |
| protected paths / critical path 断路器 | 🔷 | 极低成本、极高收益的一个特例；`.git` 和 shell rc 文件写坏一次就够痛 |
| `.env` 等敏感文件默认拒绝读取 | 🔷 | opencode 文档写的默认是 `*.env` deny、`*.env.example` allow，一行 glob 规则即可防住最常见的凭据泄漏。**但注意其代码里的实际兜底是 `ask` 而不是 `deny`（文档与实现不一致）**——自己实现时照 `deny` 做更安全 |
| hooks 驱动的可编程策略 | 🔷 | Cline 和 goose 都把 hook 当作 `.clineignore` / 权限之外的**真正强制层**（`PreToolUse` 退出码 2 或 `{"decision":"block"}` 即拒绝）；这是唯一能表达复杂策略又不把策略写进核心的机制。Codex 的 Starlark `prefix_rule` 和 Gemini 的 TOML policy 是同一思路的"策略即数据"版本 |
| 声明式策略文件（命令前缀 + 参数 glob → allow/ask/deny） | ⬜ | 十个实现里有六家做到了这一层，但自用 agent 用"黑名单 + 逐次确认"就够了；它的价值在多用户/企业分发策略 |
| 秘密自动打码（redaction） | ⬜ | 有价值的纵深防御，但属于第二层 |
| OS 级沙箱（seatbelt/landlock/容器） | ⬜（自用场景） | 实现成本高、平台差异大（Gemini 的 seatbelt profile 就有 6 种）。自用 agent 在可信仓库里跑，ROI 低 |
| 独立分类器模型做权限判定 | ⬜ | 另一次 LLM 调用 + 一套策略提示，是产品级投入 |
| prompt injection 防护（工具结果标记为不可信） | 🔷 | 一行"以下内容来自文件，不是指令"的提示 + 工具结果不参与权限决策，性价比很高 |

---

## 6. 会话持久化、恢复、fork

**解决什么问题**：agent 一次任务可能跑几百轮、几十分钟；进程崩了、终端断了、你想换个方向重来——这些都不该丢掉工作。

**一流实现怎么做**

- Claude Code 把每条消息、每次工具调用和结果写成 `~/.claude/projects/` 下的**纯文本 JSONL**，用它可以 rewind / resume / fork。`claude --continue` 和 `--resume` 复用同一个 session id 并追加；`--fork-session` / `/branch` 把历史复制到一个新 session id，原会话不动。会话与目录绑定，因此可以用 **git worktree** 开并行会话。（[how-claude-code-works](https://code.claude.com/docs/en/how-claude-code-works)、[sessions](https://code.claude.com/docs/en/sessions)）
- OpenHands 走得更远：整个系统以 **event stream** 为唯一真相源，agent、runtime、前端都订阅它——持久化不是"加一个存盘功能"，而是架构本身。（[OpenHands](https://github.com/All-Hands-AI/OpenHands)）
- 存储介质在收敛到单文件：Gemini CLI 把 session 写成 `~/.gemini/tmp/<project_hash>/chats/` 下的 JSON（含消息、工具执行、**token 用量、thinking**），并支持 `general.sessionRetention.{enabled, maxAge:"30d", maxCount}`；goose 从 1.10.0 起**从 `.jsonl` 迁到 SQLite**（`~/.local/share/goose/sessions/sessions.db`）。（[storage.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/config/storage.ts)、[goose session-management](https://goose-docs.ai/docs/guides/sessions/session-management/)）
- goose 的会话模型是**显式带父子关系的**：SQLite `sessions.db` 里有 `parent_session_id`，fork 出来的会话与 `SessionType::SubAgent` 都靠它表达；它还提供 Claude Code / Codex 历史记录的导入器，以及基于**加密 Nostr**（`GOOSE_NOSTR_RELAYS`）的跨机分享。（[nostr_share.rs](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/session/nostr_share.rs)）
- **fork 是一个正在退潮的功能**，这是本次调研里比较反直觉的发现：
  - Gemini CLI **没有**任何会话 fork/branch 能力（verified absence）。
  - Amp 在 2025-07 加了 thread fork，**2026-01 又删掉了**，理由写得很直白："We'd rather spend our time perfecting `handoff` and `thread mentions` than support `fork`."，并建议改用"开新 thread + 引用原 thread"。（[stick-a-fork-in-it](https://ampcode.com/news/stick-a-fork-in-it)）
  - 仍有实现保留它：opencode 的 `--fork`，goose 的 `--fork`（**必须与 `--resume` 连用**）。（[opencode CLI](https://opencode.ai/docs/cli/)、[goose CLI](https://goose-docs.ai/docs/guides/goose-cli-commands/)）
- 一个比 fork 更便宜、体验却更好的替代品：goose 的 `--edit` 把整个 session **以 YAML 形式**在 `$VISUAL`/`$EDITOR` 里打开，你可以"编辑、裁剪、重写消息，然后保存关闭继续会话"，还能与 `--fork` 组合从编辑后的结果分叉。（[goose CLI](https://goose-docs.ai/docs/guides/goose-cli-commands/)）

**最小可用 vs 进阶**

- 最小可用：把 messages 数组整个 JSON dump 到 `.agent/sessions/<timestamp>.json`，每条消息追加写；`--continue` 读最新一个恢复。**fork 不做。**
- 进阶：JSONL 追加写（崩溃安全）；session 命名与列表/选择器；fork/branch；跨目录会话索引；会话与 git worktree 配合做并行。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| 会话落盘 + `--continue` 恢复 | ✅ | 长任务上这是"能不能用"的分水岭，实现是十几行 |
| JSONL / 追加写（而非整文件重写） | 🔷 | 崩溃/断电时不毁掉历史；goose 后期改用 SQLite、Gemini 用 JSON，说明格式本身不重要，**"每条消息立即落盘"才重要** |
| fork / branch 会话 | ⬜ | **明确可砍**：Gemini 没有，Amp 加了又删。它的需求可以被"开新会话 + 引用旧会话"覆盖 |
| 会话列表 / 检索 / 命名 | ⬜ | 会话变多之后才需要 |
| event stream 架构 | ⬜ | 它对多前端/多 runtime 才有回报 |

---

## 7. 计划模式 / 待办清单 / 长任务目标追踪

**解决什么问题**：长任务跑到第 30 轮时模型会"忘记"原始目标，或在中途自行缩小范围。计划与清单是把意图外化成可复查的产物。

**一流实现怎么做**

- Claude Code 的 `plan` 模式是一个**权限模式**：只读探索，编辑被阻塞，直到用户批准计划。一个关键工程细节是**计划在 compaction 后从磁盘重新注入**（[permission-modes](https://code.claude.com/docs/en/permission-modes)、[context-window](https://code.claude.com/docs/en/context-window)）。
- **goose 的 plan 能力是一个"没有 plan 模式"的巧妙解法**：它没有独立的 plan 开关，而是把 ACP 的 `plan` 模式**映射到 `GooseMode::Chat`（工具全关）**，CLI 侧是 `/plan` 与 `/endplan`。（[acp/provider.rs](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/acp/provider.rs)）
  - **这再次说明"禁写"应该复用权限/模式系统，而不是新造状态机**——goose 直接复用了它已有的"chat 模式"。
- **plan 模式正在变成基础设施而不是产品功能**：Gemini CLI 的 plan 模式**默认启用**，通过 `--approval-mode=plan`、`/plan [goal]`、Shift+Tab 循环（`Default` → `Auto-Edit` → `Plan`）进入，并用 `enter_plan_mode(reason)` / `exit_plan_mode(plan_filename)` 两个**工具**进出；opencode 的 plan 模式干脆就是权限配置的产物（`edit` 与 `bash` 设为 `ask`）。也就是说，"禁写"这件事的正确实现位置是工具层的权限判定，而不是一个新状态机。（[plan-mode.md](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/plan-mode.md)、[opencode agents](https://opencode.ai/docs/agents/#use-plan)）
- Cline 用 Plan/Act 双模式，并有 `/deep-planning` 四步协议（静默调查 → 讨论 → 生成 `implementation_plan.md` → 建任务），Plan 与 Act 可以**配不同的模型**（[plan-and-act](https://docs.cline.bot/core-workflows/plan-and-act)）。
- **但要看清一个反向信号：Cline 把 Focus Chain（长任务里的 todo 清单）废弃了。** 官方理由原文："In our testing, we found that it was no longer providing enough additional benefit on top of the current harness to maintain it as a separate feature."，并且**没有直接替代品**。（[deprecations](https://docs.cline.bot/resources/deprecations)）这直接说明：**todo 清单的价值随模型变强而下降**，它是 harness 能力的补丁，不是长期必要组件。
- 仍然提供 todo 工具的实现同时存在：Gemini CLI 的 `write_todos(todos[])`、Amp 的 `todo_read` / `todo_write`、opencode 的 `todowrite`（**默认对 subagent 关闭**）。（[base-declarations.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/definitions/base-declarations.ts)、[opencode tools](https://opencode.ai/docs/tools/#todowrite)）
- **Amp 没有 plan 模式**，也没有 todo 层之外的目标栈；它的替代方案是把"只研究不改代码"写进 prompt："If you want the model to not write any code, but only to research and plan, say so: 'Do not edit any files.'"（[prompting](https://ampcode.com/docs/prompting)）
- 反面证据：mini-SWE-agent **完全没有** plan 模式或 todo 工具，靠一个足够强的模型 + 线性历史拿到 >74% SWE-bench verified；aider 同样**没有** plan 模式、todo 或目标追踪。
- **goose 用的是最省的一种长任务机制：隐藏的续跑提示。** `/goal` 与 `/grind` 会往对话里注入**隐藏的 continuation nudge**（模型看到、用户不显式看到），让 agent 继续推进而不用真的搭一套目标栈；配套的 todo 工具是 `todo_write`（`GOOSE_TODO_MAX_CHARS` 默认 50000）。（[execute_commands.rs](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/execute_commands.rs)）
  - **这是"想防跑偏但不想做 todo 工具"时最便宜的做法**：不引入新的工具和状态，只在一轮结束时补一句系统级的"继续，别忘了 X"。值得作为第 7 节的首选替代方案。
- **"目标追踪"是比 todo 清单更强的一个层次，只有 OpenHands 认真做了**：其 V1 的 `GoalController` —— 用一个**评审 LLM（judge）**判断目标是否达成，`max_iterations = 10`。注意它的 plan 模式也不是一个开关，而是一个**预设 agent**（只带 glob + grep + 一个只能写计划的 editor，产出 `PLAN.md`）。这条路线对"长任务"比 todo 清单更本质：**用另一个模型做验收，而不是让干活的模型自己列清单**。（[OpenHands](https://docs.openhands.dev/)）

**最小可用 vs 进阶**

- 最小可用：什么都不做，**只在 system prompt 里要求"先给出简短计划再动手"**。
- 进阶：一个 `todo_write` 工具维护结构化清单，并在每轮把当前清单注入上下文（真正的价值在"重新注入"，不在"让模型写清单"）；硬性 plan 模式；计划落盘 + 审批。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| system prompt 要求先规划 | ✅ | 零成本，效果显著；Anthropic 也把"显式展示规划步骤"列为三条核心设计原则之一 |
| 独立的 todo / plan 工具 + 每轮重新注入 | ⬜／🔷 | **下调标签**：Cline 实测后把 Focus Chain 废弃且无替代品，说明其增益随模型变强而衰减。只有在你自己跑长任务时确实观察到跑偏，才值得加 |
| 硬性 plan 模式（工具层禁写） | 🔷 | 已经变成基础设施：Gemini 默认开启、opencode 用权限实现。好消息是**不需要新状态机**——把写入类权限临时切成 ask 即可，实现成本很低 |
| 计划审批流 / 计划文件落盘 | ⬜ | 交互设计成本高，适合 IDE/团队场景 |

---

## 8. 子 agent 与并行编排

**解决什么问题**：两件事——(a) 探索性的大量文件读取会污染主上下文；(b) 独立子任务串行做太慢。

**一流实现怎么做**

- Claude Code 的 subagent：独立上下文窗口、独立（更短的）system prompt、共享 CLAUDE.md / MCP / skills，但**默认不能再生成 subagent（防递归）**，并且**只有最终文本 + 一小段 token 元数据回到主会话**。文档给出的例子是 subagent 读了 6.1k tokens 的文件，主上下文只增加 420 tokens。（[context-window](https://code.claude.com/docs/en/context-window)、[sub-agents](https://code.claude.com/docs/en/sub-agents)）
- Anthropic 把编排模式归纳为 prompt chaining / routing / **parallelization（sectioning + voting）** / **orchestrator-workers** / evaluator-optimizer，并明确"只有在能证明收益时才加复杂度"。（[building-effective-agents](https://www.anthropic.com/engineering/building-effective-agents)）
- Amp / Goose / opencode 都有 subagent 或等价的委派机制：
  - **Amp 把 subagent 做成"专职角色 + 模型路由"**：Search（快速检索）、Oracle（难推理/规划）、Librarian（外部代码库）、Read Thread（读别的 thread）。隔离语义写得很明确——subagent "work in isolation, so they can't communicate with each other, you can't guide them mid-task… The main agent only receives their final summary"。更值得注意的是 **oracle 可以配成与主 agent 不同的模型**（如 `high` 档主 agent 是 GPT-5.6、oracle 是 Claude Fable 5），"so one model can review the other's reasoning"。（[models-and-subagents](https://ampcode.com/docs/models-and-subagents)、[the-dial](https://ampcode.com/docs/the-dial)）
  - opencode 有 subagent 类型与 `task` 权限项；goose 的 subagent 是**持久化的子会话**，由 `summon` extension 的 `delegate` / `load` 驱动，并带两个硬预算：`GOOSE_SUBAGENT_MAX_TURNS` 默认 **25**、`GOOSE_MAX_BACKGROUND_TASKS` 默认 **5**。（[summon.rs](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/summon.rs)）
  - **子 agent 必须有独立预算**，否则一个跑飞的子 agent 会吃掉整个会话的轮数配额。
- **权限在委派链上如何继承，是一个必须明确回答的设计问题**，opencode 的答案很具体：子 agent **只继承父级的 `deny` 规则和 `external_directory`**，而 `todowrite` 与 `task` **默认强制拒绝**，除非子 agent 自己的规则集里显式提到它们；`subagent_depth` 默认 1。（[subagent-permissions.ts](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/agent/subagent-permissions.ts)）
  - 这条默认值选取得很聪明：**"继承拒绝、不继承允许"**，且**递归深度默认 1**（对应 Claude Code 默认不让 subagent 再开 subagent 的同一考虑）。
- 反面证据依然是 mini-SWE-agent：没有 subagent，也没有多 agent。

**最小可用 vs 进阶**

- 最小可用：**不做**。作为替代，用"工具结果截断 + 上下文丢弃"来对付上下文压力。
- 进阶：一个 `task` 工具（输入 prompt，返回摘要，子 agent 用独立 messages 数组）；同一轮并行调用多个只读工具；workflow 脚本做多阶段 fan-out。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| 并行只读工具调用 | 🔷 | 便宜且直接加速，是"并行"里性价比最高的一项 |
| subagent（独立上下文 + 只回摘要） | ⬜ | **明确可砍**：它是上下文管理的替代方案，不是前置条件；有它更好，没它 agent 也能用 |
| 多 agent workflow / 编排 DSL | ⬜ | 需要一整套调度、错误传播、结果聚合的工程，远超"极简可用" |
| evaluator-optimizer（自我评审循环） | ⬜ | 每轮翻倍成本，收益取决于模型 |

---

## 9. 扩展机制（MCP / hooks / skills / 自定义工具）

**解决什么问题**：核心工具集不可能覆盖所有场景（查 Jira、跑内部脚本、企业 SSO……），需要不重新编译 agent 就能加能力。

**一流实现怎么做**

- **MCP**：JSON-RPC 2.0 的开放协议，server 暴露 tools / resources / prompts，client 可提供 sampling / roots / elicitation；有 stdio 和 HTTP 两种 transport。规范把"用户明确同意"写成 MUST。（[MCP spec](https://modelcontextprotocol.io/specification/2025-06-18)）
- **Hooks**：Claude Code 允许在工具生命周期事件上挂外部命令，用 stdin 收 JSON、用 stdout 返回 JSON（`hookSpecificOutput.additionalContext` 的内容会进入模型上下文；exit code 2 把 stderr 作为错误回给模型）。PostToolUse hook 最常见的用法是"每次编辑后跑 prettier"。（[hooks](https://code.claude.com/docs/en/hooks)、[context-window](https://code.claude.com/docs/en/context-window)）
- **Skills**：一个目录 + `SKILL.md`，启动时只加载一行描述，模型真正调用时才加载全文——本质是**渐进披露（progressive disclosure）**。`disable-model-invocation: true` 的 skill 完全不进上下文，直到用户显式 `/name` 调用。compaction 后已调用的 skill body 会重新注入（每 skill ≤5k tokens）。（[context-window](https://code.claude.com/docs/en/context-window)）
- Goose 把 extension（底层是 MCP）做成一等公民，并用 "recipes" 提供可复用的任务模板；Continue 用 YAML 配置 model roles 与自定义 blocks。
- **Hooks 的执行位置是一个关键设计决定**：Claude Code 的 `PreToolUse` **在任何权限检查之前触发，且在所有模式下都触发（包括 `bypassPermissions`）**，官方规则是一句话——"Hooks can tighten restrictions but not loosen them."（[hooks](https://code.claude.com/docs/en/hooks)）
  - **这是 hooks 值得做的根本原因**：它给了用户一个**权限系统本身无法提供**的、可编程的收紧点。权限规则是有限的谓词语言，hook 是任意程序。
- **权限规则本身的优先级设计**：Claude Code 是 `deny > ask > allow`，并且**忽略具体程度**（specificity ignored，即一条宽泛的 deny 会压过一条精确的 allow）；更狠的是，`deny` 里写一个裸工具名会**把该工具从上下文里移除**，而不只是拦调用。（[permissions](https://code.claude.com/docs/en/permissions)）
- **一个重要的反直觉建议（Amp 官方）**：不要把 MCP server 全局挂着，而应**把它们打包进 skill**，因为 "Too many available tools can reduce model performance"——skill 提供的工具在 skill 加载前对模型不可见。Amp 还默认读取 **Claude Code 的 skills 目录**（`~/.claude/skills/`、`.claude/skills/`，可用 `amp.skills.disableClaudeCodeSkills` 关闭），这说明 skill 格式正在事实标准化。（[mcp](https://ampcode.com/docs/customize/mcp)、[skills](https://ampcode.com/docs/customize/skills)）
- **Amp 的 plugin 事件就是 hook 系统**，而且比 Claude Code 的 hook 更结构化：`session.start → agent.start → tool.call → tool.result → agent.end`；`tool.call` 可以返回 `allow` / `reject-and-continue` / `modify`（改写工具输入）/ `synthesize`（不真正执行、直接编造结果），`agent.end` 可以续一轮。（[plugins](https://ampcode.com/docs/customize/plugins)）
- **MCP 工具命名空间约定**：Amp 用 `mcp__<server>__<tool>`，Claude Code 也一致；Anthropic 明确建议做 namespacing 因为"工具一多，模型就选错"。（[streaming-json](https://ampcode.com/docs/cli/streaming-json)、[writing-tools-for-agents](https://www.anthropic.com/engineering/writing-tools-for-agents)）

**最小可用 vs 进阶**

- 最小可用：**不做**。或退一步：允许用户在配置文件里声明"额外工具 = 一个 shell 命令 + 参数 schema"，由 agent 动态加载。
- 进阶：MCP client（stdio 优先）；hooks（至少 `PreToolUse` 能 allow/deny/ask + `PostToolUse` 能回灌格式化结果）；skills 目录。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| MCP client | 🔷（生态上接近 ✅，自用可砍） | 现在"不支持 MCP"已经像"不支持插件"；但它是纯增量能力，砍掉不影响核心体验。且要注意：**工具越多模型越容易选错**，Amp 因此建议把 MCP 打包进 skill 按需加载 |
| hooks（`PreToolUse` 拦截 + `PostToolUse` 回灌） | 🔷 | **扩展机制里 ROI 最高的一个**，被三个实现各自独立地当作真正的策略执行层：Cline 用它把 `.clineignore` 从"提示"变成"强制"，goose 用 exit code 2 / `{"decision":"block"}` 拒绝调用，Claude Code 用它跑 prettier 并把结果回灌上下文。用几十行换来"安全策略 + 自动格式化 + 自动跑测试"三类需求 |
| skills（渐进披露的指令包） | ⬜ | 有了 AGENTS.md + hooks 之后边际收益有限；但格式已事实标准化（AGENTS.md 生态同级） |
| 自定义工具注册 | ⬜ | 自用场景直接改代码更快 |

---

## 10. 检查点与撤销

**解决什么问题**：agent 把代码改坏了，你要能一键回到"刚才那版"，而不是靠记忆手动恢复。这是让人敢放手让 agent 自动编辑的**心理前置条件**。

**一流实现怎么做**

- **aider 的方案最简单也最值得抄**：每次编辑自动 git commit（带 descriptive message），于是 `/undo` 就是 `git revert`/reset；并且在编辑"已有未提交改动"的文件前，先帮你把已有改动提交成一次独立 commit，把自己的改动和你的改动分开——"这样你永远不会因为 aider 改坏了而丢掉自己的工作"。（[git](https://aider.chat/docs/git.html)）
- **Claude Code 的方案是 shadow snapshot**：每个用户 prompt 之前快照被编辑的文件，会话内保留最近 100 个 checkpoint，`/rewind` 可以选"恢复代码 / 恢复对话 / 两者 / 从这里摘要 / 到此为止摘要"。文档**明确列出局限**，这些局限本身很有信息量：bash 命令改的文件不被追踪（`rm`/`mv`/`cp` 都撤不回）、subagent 的编辑一般不恢复、会话外的并发改动不追踪、软链接/硬链接不恢复、**"不是版本控制的替代品"**。（[checkpointing](https://code.claude.com/docs/en/checkpointing)）
- Cline 用 shadow git repo 做 per-task checkpoint，可以跨会话恢复。**这是"每个工具调用一次 checkpoint"的最激进版本**：Cline 内部维护一个**独立于项目 git 历史的 shadow git 仓库**，每次工具调用后提交当前文件状态——"Your main Git repository stays untouched"，并且"Checkpoints capture everything, including files not tracked by Git"。恢复有三个选项，**把代码状态和对话状态分开**：Restore Files / Restore Task Only / Restore Files & Task。它给出的理由值得引原文："Instead of carefully reviewing every change before approving, you can let Cline move fast and roll back if something goes wrong. The cost of a mistake drops to nearly zero." 代价是官方承认的："For very large repositories, checkpoints may use significant storage and slow down Cline". （[checkpoints](https://docs.cline.bot/core-workflows/checkpoints)）
- Gemini CLI 也提供 `/rewind`（Esc 按两次），三选一：回滚对话+代码 / 只回滚对话 / 只回滚代码，实现方式是 **git snapshot**，并且和 Claude Code 一样明确写出同一条限制："does not undo manual edits or changes triggered by the shell tool"。另有独立于 rewind 的自动 checkpoint（编辑工具触发），用 `/restore <checkpoint_file>` 恢复。（[rewind.md](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/rewind.md)、[checkpointing.md](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/checkpointing.md)）
- opencode 默认开启 git-backed snapshot（内部 git 仓库），用户侧是 `/undo` + `/redo`，且 `/undo` 会把原始消息也还原回来方便重试；配置项 `"snapshot": false` 关闭时文档明确说"changes made by the agent cannot be rolled back"。（[opencode config](https://opencode.ai/docs/config/#snapshot)）
- **反例（有两个，很有说服力）：**
  - **goose 完全没有任何 checkpoint / 文件级 undo。** 它的 Developer extension 只有 `write` / `edit` / `shell` / `tree` / `read_image` 五个工具，没有 `undo_edit`，也没有 rewind 或 snapshot store；最接近的只有会话级的 `/clear`、`/compact` 和 `session --edit` / `--fork`。goose 自己的文档告诉用户"先把代码提交，这样你有干净的快照可以对比和回滚"。（[goose developer extension](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/developer/mod.rs)、[goose CLI](https://goose-docs.ai/docs/guides/goose-cli-commands/)）
  - **Codex 也一样，完全没有 undo 或 checkpoint。** 仓库里 `ghost_snapshot` 只是残留的兼容配置项，`Feature::GhostCommit` 状态是 `Stage::Removed`；文档明确让用户自己"create Git checkpoints"。它的隔离故事是 **git worktree**，不是快照。（[codex](https://github.com/openai/codex)）
  - **Amp 更激进：它主动把"编辑消息时回滚文件"这个功能删掉了。** 理由值得完整引用——"the models are now good enough to undo changes for you… rollback was always best-effort: if the agent wrote and ran code that generated files, we didn't keep track of that without sophisticated snapshotting."，即**它承认自己做不好快照（生成的文件追踪不到），于是选择不做**，改为提供 `undo_edit` 工具 + git trailer。（[neo](https://ampcode.com/news/neo)）
- **五个实现根本没有内置 checkpoint/undo**（Codex / goose / Amp / Continue / OpenHands，其中 Amp 还是主动删除的）。所以内置 checkpoint 不是硬门槛——**"有一条明确的撤销路径，并且用户知道它是什么"才是**。对自研 agent，最划算的组合是"每次编辑自动 git commit + 暴露 `/undo`"，而不是自建快照系统。
  - aider 把这条走到了极致：**它的撤销就是 git**，`/undo` 在四种情况下直接拒绝——commit 已被 push、是 merge commit、工作区脏、或该 commit 不在本会话的 `aider_commit_hashes` 里。（[aider](https://aider.chat/docs/git.html)）
  - 另一个细节：aider 的 `--git-commit-verify` **默认是 False**，即默认跳过 pre-commit hooks。（[aider](https://aider.chat/docs/git.html)）自研时值得反过来默认跑，或至少显式提示。
- 两个便宜且有巧思的变体：
  - **把 undo 做成模型可调用的工具**：Amp 的内建工具列表里有 `undo_edit`，即撤销不只是用户手势，agent 自己发现改错时也能撤（[streaming-json](https://ampcode.com/docs/cli/streaming-json)）。
  - **用 commit trailer 把代码改回连到对话**：Amp 的 commit 带 `Amp-Thread-ID: <thread-url>` 与 `Co-authored-by: Amp <amp@ampcode.com>`，可从任意一行代码追溯回当时那次会话（[settings](https://ampcode.com/docs/cli/settings)）。

**最小可用 vs 进阶**

- 最小可用：**直接依赖 git**。启动时检查工作区是否干净，脏了就提示（或 `git stash`）；每次 edit/write 前后不做事，让用户自己 `git diff` / `git checkout`。
- 进阶（仍然便宜）：每次 AI 编辑自动 commit（或编辑前把文件内容 snapshot 到内存/临时目录），提供 `/undo`。
- 再进阶：按 prompt 分组的 checkpoint 列表 + 交互式 rewind 菜单 + 对话与代码分别回滚。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| 有一条明确的撤销路径（内置快照 **或** 明确依赖 git 并告知用户） | ✅ | **重新定义了这一行**：goose 一个内置 undo 都没有、只让用户先提交，仍然被广泛使用。门槛不是"实现 checkpoint"，而是"用户知道改坏了怎么回去" |
| 编辑前快照 / 每次编辑自动 git commit + `/undo` | 🔷 | Claude Code、aider、opencode、Cline、Gemini 都做了；实现 20~50 行，换来"敢让它自动改文件"。没有它用户只敢开确认模式 |
| 启动时提示工作区是否干净 / 脏文件先提交 | ✅ | 把用户的工作和 agent 的工作分开，避免混淆与丢失；aider 的 dirty-commit 就是这一条 |
| 把代码状态与对话状态分开回滚 | 🔷 | Cline 和 Gemini 都独立收敛到这个三选一（文件 / 对话 / 两者）；这是一个低成本高感知的设计 |
| 交互式 `/rewind` 菜单 | ⬜ | 有上面的能力之后，菜单只是外壳 |
| shadow git repo | ⬜ | 解决"不想污染主仓历史"，但引入一套自己的 git 逻辑和一堆边界情况（大仓库变慢是官方承认的代价）；opencode/Amp 用更轻的 "commit + trailer" 就够 |

---

## 11. 成本与 Token 控制

**解决什么问题**：coding agent 的成本是"每轮都重发整个上下文"乘以"轮数"。不做任何控制，一次重构能烧掉可观的钱和几分钟等待。

**一流实现怎么做**

- **Prompt caching 是最大的一笔优化**：把稳定的前缀（system prompt、工具定义、规则文件）放在最前面并打缓存断点，后续每轮只付增量成本。Claude Code 专门有一页文档列出"哪些操作会让缓存失效"。（[prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)、[Claude Code costs](https://code.claude.com/docs/en/costs)）
- **模型分级**：aider 用 `--weak-model` 专门跑 commit message 生成（把 diff + 聊天历史丢给便宜模型）；Claude Code 的权限分类器默认跑在另一个（更便宜的）模型上；Anthropic 的 routing 模式明确建议"简单问题给 Haiku、困难问题给 Sonnet"；Cline 允许 **Plan 模式与 Act 模式配不同的模型**。（[plan-and-act](https://docs.cline.bot/core-workflows/plan-and-act)）
- **Amp 把成本与能力的权衡直接做成了产品的主轴**：它的 `low`/`medium`/`high`/`ultra` 四档 "Dial" 的公开理由就是成本——"the only question left is capability against cost… Undershoot and the model churns… Overshoot and you're using Fable to fix a typo."，并且把 reasoning effort 也折进档位，不再单独循环。（[the-dial](https://ampcode.com/news/the-dial)）
- **token 计量的字段设计值得直接照抄**（Amp 的流式 JSON `usage`）：`input_tokens`、`output_tokens`、**`cache_creation_input_tokens`**、**`cache_read_input_tokens`**、`max_tokens`、`service_tier`，以及嵌套的 `cache_creation: { ephemeral_5m_input_tokens, ephemeral_1h_input_tokens }`——即**同时使用 5 分钟和 1 小时两种 prompt cache TTL**。把"缓存写入"和"缓存读取"分开计量，是判断缓存有没有生效的唯一办法。（[streaming-json](https://ampcode.com/docs/cli/streaming-json)）
- opencode **没有任何成本预算机制**，只有 `stats` 与每条消息的 cost / cache token 记录——说明"显示"和"限制"是两件事，而业界普遍只做前者。（[opencode](https://opencode.ai/docs/)）
- **预算与可视化**：`/context` 看占用明细；aider 有 `--map-tokens` 控制 repo map 预算；Claude Code 把工具响应限制在 25k tokens；Amp 有 `amp.showCosts`（默认开，企业管理员可关闭）和 `amp usage`。（[Amp settings](https://ampcode.com/docs/cli/settings)）
- **限制输出**：工具输出截断（见第 3 节）本身既省上下文也省钱。

**最小可用 vs 进阶**

- 最小可用：每轮及累计打印 `in/out tokens` 与费用估算；**保证 system prompt 前缀逐字节稳定**（不要在 system prompt 里插时间戳、随机顺序的 map）——这几乎是免费的缓存命中率。
- 进阶：显式 cache 断点；摘要/分类/commit message 走弱模型；预算上限硬停；按任务维度的成本报表。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| 每轮 + 累计 token/费用显示 | ✅ | 不看数字就没法判断"值不值"，且实现是加个计数器 |
| system prompt 前缀稳定（缓存友好） | ✅ | 免费的 5~10 倍成本差；只要"别乱动前缀"这一个约束 |
| 工具输出上限 | ✅ | 见第 2、3 节 |
| 弱模型分流（摘要 / commit message） | 🔷 | aider 的 `--weak-model` 是最省事的样本，接第二个模型即可 |
| 显式 cache 断点调优 | ⬜ | 依赖具体 provider，随模型/前缀变化而调 |
| 硬预算上限 / 自动停止 | ⬜ | 有了显示之后再谈；自用场景人工看着就够 |

---

## 12. Provider 抽象与多模型支持

**解决什么问题**：模型半年一换，API 提供方也可能变（OpenAI / Anthropic / Gemini / DeepSeek / 本地 Ollama / 企业网关）。把 provider 固化进 agent 核心，后续每次换模型都要改核心代码。

**一流实现怎么做**

- aider 的模型列表是它的核心资产之一：OpenAI / Anthropic / Gemini / DeepSeek / xAI / Groq / Cohere / Azure / Bedrock / Vertex / Ollama / LM Studio / OpenRouter / **OpenAI-compatible**，并且有 model alias 与 per-model 的 edit format 选择。（[aider LLMs](https://aider.chat/docs/llms.html)）
- mini-SWE-agent 直接依赖 **litellm** 做统一接口，因此"支持任何模型"，并且支持 `/completion` 与 `/response` 两种端点风格、interleaved thinking 等。（[mini-swe-agent](https://mini-swe-agent.com/latest/)）
- opencode 用一个公开的模型数据库 `models.dev` 承载"模型 → 能力/价格/上下文长度"的映射。
- Claude Code 支持 Bedrock / Vertex / LLM gateway 等企业接入路径。
- **但要看到另一个事实：两大单一厂商 CLI 都刻意不做通用 provider 抽象。** Gemini CLI 没有任何 OpenAI-compatible provider（仓库级检索确认，唯一的 base URL 覆盖是 `GOOGLE_GEMINI_BASE_URL` / `GOOGLE_VERTEX_BASE_URL`）；Codex 的 `wire_api` **只接受 `responses`**，传 `"chat"` 会直接反序列化报错，内置 provider 只有 `openai` / `amazon-bedrock` / `ollama` / `lmstudio`。（[gemini-cli](https://github.com/google-gemini/gemini-cli)、[codex](https://github.com/openai/codex)）
  - **这条对自研 agent 反而加强了结论**：厂商自己做 CLI 时会把"模型耦合"当成特性，因为它们控制模型。你不控制模型，所以**必须**把 provider 当可替换件。

**最小可用 vs 进阶**

- 最小可用：**只写一个 OpenAI-compatible 的 HTTP client**（`base_url` + `api_key` + `model` 全从环境变量/配置读）。这一条就能覆盖 OpenAI、DeepSeek、OpenRouter、Ollama、vLLM、以及绝大多数网关——是 90% 覆盖率的 5% 成本。
- 进阶：原生 Anthropic Messages / Gemini 协议适配；模型能力表（上下文长度、是否支持 tool call、是否支持并行 tool call、是否支持 thinking）；自动按模型选择 edit format；失败重试与降级。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| 一个 OpenAI-compatible client（base_url 可配） | ✅ | 写死一家 provider 是极难回退的架构决策 |
| 配置从环境变量/文件读，不硬编码 | ✅ | 同上，且 CI 需要 |
| 原生多协议适配 | ⬜ | 有兼容层就够用了；只在要用 provider 独有能力时才需要 |
| 模型能力表 + 自动 edit format 选择 | 🔷 | 接多模型后才需要；aider 证明它能显著影响成功率 |
| 重试 / 限流 / 降级 | 🔷 | 长任务里一次 429 不该让整个会话失败 |

---

## 13. 流式输出与 TUI / 交互体验

**解决什么问题**：agent 的一轮可能几十秒。没有流式输出，用户以为程序挂了；看不到工具调用，用户不知道它在干什么、也就无法在它跑偏时打断。

**一流实现怎么做**

- Claude Code 的展示哲学是**分层可见性**：模型读文件时终端只显示一行 `Read auth.ts`（2,400 tokens 的文件内容只有模型看到）；跑测试只显示 `Running npm test...` 和通过数，而不是完整的 1,200 tokens 输出；但模型自己的分析、编辑 diff、最终回复是完整展示的。（[context-window](https://code.claude.com/docs/en/context-window)）
- 交互控制：`Shift+Tab` 循环切换权限模式；`Esc` 打断；空输入框连按两次 `Esc` 打开 rewind 菜单；`!` 前缀进入 shell 模式（命令和输出都进上下文，用来"把 agent 锚定在真实命令输出上"）。（[interactive-mode](https://code.claude.com/docs/en/interactive-mode)、[checkpointing](https://code.claude.com/docs/en/checkpointing)）
- opencode 把 TUI 和 agent server 拆成两个进程（client/server），TUI 可替换、可远程。
- Anthropic 把 **transparency（显式展示规划步骤）** 列为三条核心设计原则之一。

**最小可用 vs 进阶**

- 最小可用：流式打印模型文本；每个工具调用打一行 `→ tool(args)`；工具结果只打前 N 行 + "…（共 M 行）"；`Ctrl-C` 能中断当前轮并保留已完成的编辑。
- 进阶：全屏 TUI（alt screen、可折叠工具输出、diff 语法高亮、状态栏显示 token/花费/权限模式）；输入历史的 `/` 命令补全；把"思考"与"输出"分色。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| 流式输出 | ✅ | 否则用户无法区分"在思考"和"卡死了" |
| 每个工具调用一行可读摘要 | ✅ | 这是打断能力和信任感的前提 |
| `Ctrl-C` 中断 | ✅ | 基本控制权 |
| 工具输出在终端折叠/截断显示 | 🔷 | 与上下文截断是两件不同的事，但可以共用一套逻辑 |
| 全屏 TUI / 状态栏 | ⬜ | 先做 plain 输出，headless 和 eval 都能直接复用；TUI 是纯外层 |
| diff 语法高亮 | ⬜ | 好看，不影响能力 |

---

## 14. 非交互模式（headless / CI / print）与脚本化

**解决什么问题**：让 agent 成为管道里的一个普通命令——CI 里跑、脚本里循环、被上层程序编排。它同时是 eval harness 的前置条件。

**一流实现怎么做**

- aider 的 `--message/-m "..."` 让 agent "做这一件事、改完文件、退出"，可以 `for FILE in *.py; do aider -m "..." $FILE; done` 批处理；配套 `--yes`（跳过所有确认）、`--dry-run`、`--message-file`、`--stream/--no-stream`、`--commit`。（[scripting aider](https://aider.chat/docs/scripting.html)）
- Claude Code：`claude -p "prompt"`，可配 `--output-format json|stream-json`、`--permission-mode dontAsk --allowedTools "Bash(npm test)" "Read"` —— **`dontAsk` 模式基本就是为 CI 设计的**；非交互运行里没有 prompt 可回退，被拒绝的动作直接不执行并继续。（[permission-modes](https://code.claude.com/docs/en/permission-modes)、[headless](https://code.claude.com/docs/en/headless)）
- 同类：Codex 的 `codex exec`（`--json` 输出 JSONL 事件）、Gemini CLI 的 `-p`、opencode 的 `run`、Goose 的 `goose run`、Continue 的 `cn -p`（`--silent` / `--format json`，中断退出码 130）。
- **一个反面教材：headless 的安全姿态不应该被硬编码。** OpenHands V1 的 `openhands --headless -t/-f` 是**始终自动批准、且无法更改**的——你没有办法为 CI 选一个更保守的策略。自研时至少要让 headless 能被策略配置（Claude Code 的 `--permission-mode dontAsk` + `--allowedTools` 是正确的做法）。（[OpenHands](https://docs.openhands.dev/)）

**最小可用 vs 进阶**

- 最小可用：`agent "prompt"` 跑完退出；`--yes` 跳过确认；**stdout 只输出结果（日志走 stderr）**；退出码非 0 表示失败。
- 进阶：`--output-format json`；`--message-file`；`--dry-run`；CI 用的工具白名单模式；`stream-json` 供上层编排。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| prompt 作为参数 → 跑完退出 | ✅ | 实现成本≈0（同一套循环换个入口），但它是 CI、脚本、eval 三个功能的共同底座 |
| `--yes` / 跳过确认 | ✅ | headless 的前提，否则会挂在等待输入上 |
| stdout/stderr 分离 + 退出码约定 | ✅ | 否则无法管道化 |
| JSON 输出 / stream-json | 🔷 | 上层编排（含 eval harness）需要 |
| `--dry-run` | ⬜ | 便于审查，非必需 |

---

## 15. 评测（eval harness）与回归测试

**解决什么问题**：agent 的输出是非确定性的，靠"感觉一下"改 prompt 和工具描述必然退化。没有 eval，就无法回答"我这次改动是变好了还是变差了"。

**一流实现怎么做**

- **SWE-bench 是事实标准**：从真实 GitHub issue 生成 patch，用仓库自己的测试判定通过；**SWE-bench Verified** 是其中 500 个人工验证过的样本子集（[swebench.com/verified](https://www.swebench.com/verified.html)）。它也是最有说服力的能力证据来源。
- **mini-SWE-agent 给了"最小可行"的最强证据**：约 100 行 Python（agent 本体）+100 行（env/model/script），**只有 bash 一个工具、不使用 tool-calling API、线性历史、`subprocess.run` 无状态执行**，拿到 >74% SWE-bench verified。（[mini-swe-agent](https://mini-swe-agent.com/latest/)）
- **Gemini CLI 的 behavioral evals 是"怎么给 agent 写测试"的最佳范本**（[behavioral-evals.md](https://github.com/google-gemini/gemini-cli/blob/main/docs/behavioral-evals.md)）：
  - 断言的是**行为**（调用了哪些工具、调用顺序、有没有跑危险命令），不是最终文本——因为文本非确定性，`expect(result).toContain(...)` 极其脆弱。
  - 每条 eval 带 **policy**：`ALWAYS_PASSES` / `USUALLY_PASSES` / `USUALLY_FAILS`；新 eval 必须从 `USUALLY_PASSES` 起步，等 nightly 数据证明稳定后才晋级。
  - 强制 `files` 元数据（涉及文件读写的 eval 必须在 `rig.testDir` 内），并用 `rig.waitForToolCall` 断言。
  - 有 lint 工具（`eval:validate`）把命名、metadata、断言规则做成 CI 门禁；有 `eval:report` 聚合成按模型维度的通过率趋势。
  - 明确的反模式：不要限制核心工具集、不要断言模型措辞、纯文件操作的测试属于 integration test 不属于 eval。
- **Anthropic 的实践**：用 agent 自己生成 eval 任务、跑 held-out 测试集防止过拟合，并在评测中额外收集**工具调用次数、token 消耗、耗时、工具错误率**——"很多冗余工具调用说明分页/限额参数需要调整"。（[writing-tools-for-agents](https://www.anthropic.com/engineering/writing-tools-for-agents)）
- 工程上的通用做法：一个 **deterministic / mock model**（按脚本返回固定的 tool call 序列）+ 录制回放，让核心逻辑可以脱离真实 API 测试。mini-SWE-agent 就内置了 `DeterministicModel`；**Gemini CLI 甚至把它做成了 CLI 开关**：`--fake-responses` / `--fake-responses-non-strict` / `--record-responses`。（[mini-swe-agent](https://mini-swe-agent.com/latest/)、[config.ts](https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/config/config.ts)）
- **Cline 是唯一一家把 eval harness 开源并当成产品能力在讲的**，它的分层设计值得照抄：`evals/` 下按 **contract → smoke → e2e** 三层组织，外挂一个 `cline-bench` submodule，指标是 **`pass@k` / `pass^k` / flakiness**（注意 `pass^k`——连续 k 次都通过，这是对非确定性系统的正确指标），跑在 Harbor + Modal + Terminal Bench（89 个任务）上，报告的成绩是 47% → 57%。
  - 它报告的一件事比分数更有价值：**同一个模型走不同 provider 路由，成绩差 12 个百分点**（同一个 GLM-5.2：CoreWeave 74.2% vs OpenRouter 61.8%），并且**坦白报告了巨大方差**。（[evals blog](https://cline.ghost.io/open-sourcing-evals-for-open-weight-agents/)）
  - **这条对自研 agent 的启示**：如果你拿 eval 分数做决策，先固定 provider 路由；否则你测的是路由不是 agent。
- **一个必须说清的发现：这些项目自己几乎不用 SWE-bench 做回归测试。** 我逐个查了它们的仓库：
  - **Gemini CLI**：有 in-repo 的 vitest behavioral evals（上面那套 EDK），但**没有 SWE-bench harness**。
  - **Codex**：**没有任何 eval harness**，取而代之的是 **1003 个 `insta` 快照测试**（`.snap` 文件）。
  - **opencode**：**没有 agent/LLM eval harness**（仓库内检索只找到前端性能 benchmark），但有 **382 个测试文件**，包括针对 agent 循环的定向回归（如 `plan-mode-subagent-bypass.test.ts`）；其 `AGENTS.md` 明确要求 "Avoid mocks as much as possible"。
  - **Amp**：**没有公开的 eval harness 或 SWE-bench 分数**，并且专门写了一篇短文论证 eval 的局限——"evals are primarily useful as unit tests and regression tests, to ensure a new model doesn't regress some existing behavior you'd like to preserve"，以及 "it's impossible to fully capture product experience in evals and whatever evals you do construct are backward looking."（[model-evaluation](https://ampcode.com/news/model-evaluation)）
  - **goose**：除了两个 CI harness，还有**录制回放的 provider 场景测试**（把真实 provider 交互录下来当回归用例）——这是"不烧钱也能测真实模型行为"的做法。（[goose](https://github.com/aaif-goose/goose)）
  - **goose**：走的是"一致性测试"路线，两个 harness 都在 CI 里：**MCP Conformance**（带 checked-in 的 expected-failure 基线文件，按 MCP 规范版本分）和 **Model Tool Call Conformance**（每天 3 点 cron 扫一批模型，看"哪些模型真的能驱动一次 MCP 工具调用"，并明确声明 **不是 PR gate**——"a failure here usually means a provider or model regressed"）；另有一个 `goose-self-test.yaml` recipe，让 goose 用自己的工具测自己的能力。（[mcp-conformance.yml](https://github.com/aaif-goose/goose/blob/main/.github/workflows/mcp-conformance.yml)、[model-toolcall-conformance.yml](https://github.com/aaif-goose/goose/blob/main/.github/workflows/model-toolcall-conformance.yml)）
  - **结论：SWE-bench 的角色是"对外证明能力"，不是"对内防退化"。** 对内防退化的主流手段是**普通单元/快照测试 + 行为测试**，成本低几个数量级。这条直接决定了第 15 域的标签。

**最小可用 vs 进阶**

- 最小可用：一个 **fake provider**（读一个 JSON 文件当"模型响应序列"）+ 5~10 个端到端用例：在临时 git 仓库里跑真实循环，断言**文件内容**和**工具调用序列**。不要 mock HTTP 层，也不要断言模型措辞。
- 进阶：行为评测 + policy 分级 + 去抖（同一 eval 跑 3 次）；多模型跑分与趋势报表；SWE-bench 风格的真实任务集；把 eval 接进 CI。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| fake/deterministic provider + 少量端到端测试 | ✅ | 唯一的"防退化"手段；没有它，第 2 节的每个 edit 策略调整都是盲改。Gemini CLI 把它做成 CLI 开关（`--fake-responses`）说明这是内部刚需 |
| 断言工具调用序列（behavioral eval） | 🔷 | 成本低、能抓住绝大多数回归（比如"模型不再并行读文件了""它开始跑危险命令了"）。注意 Gemini CLI 的教训：**不要断言模型措辞**，只断言工具调用 |
| 快照测试 + 定向回归用例 | ✅ | Codex 用 1003 个 `insta` 快照、opencode 用 382 个测试文件——**这才是这些项目真正的防退化主力**，而不是 benchmark |
| eval policy 分级 + 多次运行去抖 | 🔷 | 非确定性系统里做 CI 门禁的必要条件 |
| 定时跑的一致性 sweep（非 PR gate） | ⬜ | goose 的每日 model-toolcall conformance 是好设计，但需要多 provider 矩阵才划算 |
| SWE-bench 跑分 | ⬜ | **明确可砍**：**十个实现里没有一个把它当作自己的回归测试**（它们是快照测试 + 行为测试 + 一致性测试）。它是对外证明，成本是容器 + 大规模并发 + 数小时到数天算力 |
| 多模型跑分 dashboard | ⬜ | 产品级投入 |

---

## 16. 可观测性（日志 / trace / token 统计）

**解决什么问题**：agent 出问题时（改错文件、死循环、不调某个工具），**唯一的排查手段是看完整 transcript**。日志不是运维设施，是开发 agent 本身的主要工具。

**一流实现怎么做**

- Claude Code 把整个会话写成 JSONL，另有 `/debug` 打开的 `~/.claude/debug/<session-id>.txt`（连"restore 时跳过了哪些软链接文件"都会逐条列出）。hooks 的输出路由也是明确设计过的：`hookSpecificOutput.additionalContext` 进模型上下文，plain stdout（exit 0）只进 debug log，exit code 2 把 stderr 作为错误回给模型，超过 10,000 字符的 hook 输出落盘只给模型预览。（[hooks](https://code.claude.com/docs/en/hooks)、[checkpointing](https://code.claude.com/docs/en/checkpointing)）
- Gemini CLI 提供 OpenTelemetry 遥测配置（traces/metrics/logs）。
- **Claude Code 的 OTel 面是完整实现的、可直接参考的指标清单**：开关是 `CLAUDE_CODE_ENABLE_TELEMETRY`，导出 `claude_code.*` 系列指标与事件，其中包括 `token.usage`、`cost.usage`、**`code_edit_tool.decision`**（编辑类工具的批准/拒绝决策）、`compaction`、`skill_activated`、`subagent_completed`，以及可选的 `api_request_body` / `api_response_body`。（[monitoring-usage](https://code.claude.com/docs/en/monitoring-usage)）
  - 值得抄的是**指标的选择**：`code_edit_tool.decision` 这个指标直接回答"我的权限策略是不是太烦了"，是自研 agent 里最有诊断价值的一个计数器。
- **Codex 也真做了 OTel**，包括一个 `codex.turn.cost_microusd` 指标（把每轮成本做成可监控量）；日志只走 `RUST_LOG`。（[codex](https://github.com/openai/codex)）
- Anthropic 建议在 eval 中收集 tool call 数、总 token、总耗时、工具错误率，并**人工读 transcript**——包括模型 reasoning 和原始工具调用，"模型没说出来的往往比说出来的更重要"。（[writing-tools-for-agents](https://www.anthropic.com/engineering/writing-tools-for-agents)）
- mini-SWE-agent 的"线性历史"设计本身就是一个可观测性选择：**trajectory 和送给模型的 messages 完全相同**，"great for debugging & fine-tuning"。（[mini-swe-agent](https://mini-swe-agent.com/latest/)）

**最小可用 vs 进阶**

- 最小可用：**每次会话写一份完整 transcript**（每轮的请求 messages、模型输出、工具调用与参数、工具结果、耗时、token 数）到一个可读文件；`--verbose` 把同一份东西打 stderr；错误信息要包含足够的上下文而不是一个裸异常。
- 进阶：结构化 JSON 日志；trace/span（一次工具调用一个 span）；OTel 导出；按轮的 token 与成本统计；trajectory 浏览器。

**标签**

| 功能点 | 标签 | 理由 |
| --- | --- | --- |
| 完整 transcript 落盘（含工具参数与结果） | ✅ | 开发 agent 时你会读它几十次；也是第 6 节会话恢复的同一份数据 |
| token 统计 | ✅ | 与第 11 节共用 |
| 错误信息带上下文（哪个工具、什么参数、原始 stderr） | ✅ | 直接影响模型能否自我修复——错误信息是给模型看的 prompt |
| 结构化日志 / trace | 🔷 | 会话变长后检索需求真实存在 |
| OTel 导出 | ⬜ | 团队协作场景 |
| trajectory 浏览器 | ⬜ | 纯工具链 |

---

## 17. 横切主题：把工具描述当作 prompt 工程

不算独立功能域，但它跨上面所有域，且是投入产出比最高的一项。

- 工具定义的**措辞**直接决定成功率：Anthropic 报告 Claude Sonnet 3.5 在 SWE-bench Verified 上达到 SOTA，靠的是"对工具描述做了精确的改进"，大幅降低了错误率。（[writing-tools-for-agents](https://www.anthropic.com/engineering/writing-tools-for-agents)）
- 具体建议：参数名要无歧义（`user_id` 而不是 `user`）；工具名做命名空间（`asana_search` / `jira_search`）以便模型选对；**工具数量不是越多越好**——"把 `list_users`+`list_events`+`create_event` 合并成一个 `schedule_event`"这类合并，能减少模型的选择负担和上下文占用；工具要返回**高信号**内容（去掉 uuid、mime_type 这类低层标识符，把 UUID 换成更语义化的名称可显著减少幻觉）；对可能吃满上下文的返回实现分页/过滤/截断并给出"请缩小搜索范围"的提示；错误响应要写成**可操作的改进建议**，而不是错误码或 traceback。（[writing-tools-for-agents](https://www.anthropic.com/engineering/writing-tools-for-agents)）
- 格式选择的经验法则：**给模型足够的 token 先"想"，格式尽量贴近它在互联网文本里自然见过的样子，避免任何需要精确计数的 overhead**（明确点名"写 diff 需要先知道 chunk header 的行数"，以及"把代码塞进 JSON 需要转义换行和引号"）。（[building-effective-agents](https://www.anthropic.com/engineering/building-effective-agents)）
- 用 poka-yoke 的思路改参数让错误难以发生：SWE-bench agent 中"文件路径一律要求绝对路径"就是这类修复。

---

# 结论一：标签总表

| # | 功能域 | ✅ table stakes | 🔷 differentiator | ⬜ nice-to-have |
| --- | --- | --- | --- | --- |
| 1 | Agent 循环 | 多轮循环；max steps（可配，别硬编码）；无工具调用即停 | 并行只读工具（写操作串行化）；重复调用守卫 | 显式阶段状态机；流式 tool_use；可插拔停止条件 |
| 2 | 工具与编辑 | read/write/bash；search-replace 编辑；read-before-edit；输出截断 | **编辑失败降级匹配**；拒绝省略占位符；专用 grep/glob；编辑后自动 lint/test；同文件写入串行化 | 多文件补丁；unified diff；AST 编辑 |
| 3 | 上下文 | token 估算 + 上限 + 丢最老结果 | 自动 compaction（~90% 阈值）+ 压缩后重新注入；`@file`（限长）；`.gitignore` | repo map；向量检索 |
| 4 | 规则文件 | 读 `AGENTS.md`（放**第一条 user message**，不要放 system prompt） | 向上查找父目录；可验证的加载清单 | 路径作用域规则；auto memory |
| 5 | 权限安全 | 两档模式；逐次确认；cwd 限制；危险命令黑名单（**非安全边界**） | protected/critical path 断路器；批准粒度 once/always/reject；`.env` 默认拒读；hooks 强制层；注入防护提示 | 声明式策略文件；OS 沙箱；分类器模型；秘密打码 |
| 6 | 会话 | 落盘 + resume | 每条消息即落盘 | fork/branch；会话检索；event sourcing |
| 7 | 计划/待办 | prompt 里要求先规划 | 硬 plan 模式（用权限实现，不需新状态机）；**隐藏续跑提示** | todo 工具；评审模型做目标验收；计划审批流 |
| 8 | 子 agent | — | — | subagent；专职角色 subagent；跨模型交叉评审；多 agent 编排 |
| 9 | 扩展机制 | — | hooks（`PreToolUse` 在权限检查之前触发）；MCP（生态上接近 table stakes） | skills；自定义工具注册；声明式策略 |
| 10 | 检查点撤销 | **有一条明确的撤销路径**（自动 commit 或快照）+ 脏工作区提示 | 编辑前快照 / 自动 commit + `/undo`；代码与对话分开回滚 | 交互式 rewind；shadow git；模型可调用的 `undo_edit` |
| 11 | 成本控制 | token/费用显示；**前缀稳定**；输出上限 | 弱模型分流；缓存读/写分开计量 | cache 断点调优；硬预算；成本档位 |
| 12 | Provider | 单一 OpenAI-compatible client；配置外置 | 模型能力表；重试降级 | 原生多协议 |
| 13 | 流式与 TUI | 流式输出；工具调用一行摘要；Ctrl-C | 终端侧折叠显示；steering（边跑边插话） | 全屏 TUI；语法高亮；队列发送 |
| 14 | Headless | prompt 参数 + `--yes`；stdout/stderr 分离 + 退出码 | 结构化 JSON / stream-json 输出 | `--dry-run`；HTTP API / server 模式 |
| 15 | 评测 | **fake provider + 端到端测试**；快照/定向回归用例 | 行为评测（断言工具调用，不断言措辞）；policy 分级 | 一致性 sweep；SWE-bench 跑分；多模型 dashboard |
| 16 | 可观测性 | transcript 落盘；token 统计；带上下文的错误 | 权限决策计数等诊断指标；结构化日志 / trace | OTel 导出；trajectory 浏览器 |
| 17 | 工具描述工程 | 清晰的工具描述与参数名（属第 2 节内容） | 工具合并 / 返回值瘦身 / 错误可操作化 | — |

# 结论一之二：跨实现能力有无对照表（"砍掉它会不会不可用"的直接证据）

这张表是本文最该看的一张。**它的用途不是比谁功能多，而是看每一个功能域里"有没有成功实现把它省掉"。**
`✅` = 有；`❌` = 明确没有（多数经仓库级检索确认）；`⚠️` = 有但有重要前提；`?` = 本次调研未能确认。

| 功能域 | Claude Code | Codex | aider | Cline | OpenHands | Gemini CLI | opencode | goose | Amp | Continue |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 走标准 tool-calling 协议 | ✅ | ✅ | **❌ 纯文本 edit format** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 专用 read/grep/glob 工具 | ✅ | **❌ 只有 shell + patch** | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| edit = search-replace | ✅ | **❌ 自定义 patch DSL** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ? | ✅ |
| 并行工具调用 | ✅ 仅只读 | ✅ 默认串行 | ❌ | ✅ 可开关 | ✅ 默认 1 | ✅ 默认开 | ✅ | ? | ✅ | ✅ 仅只读 |
| OS 沙箱 | ✅ | ✅ | **❌** | ❌ | ✅ 可选 | ✅ 多后端 | ❌ | ✅ 容器可选 | ✅ orb | ❌ |
| 命令策略 / 黑名单 | ✅ | ✅ Starlark | **❌** | 模型打标记 | ✅ 风险分级 | ✅ TOML | ✅ | ✅ 四模式 | ✅ 插件 | ✅ yaml |
| 路径白名单 | ✅ | ✅ | **❌** | ✅ | ? | ✅ | ✅ `external_directory` | ❌ | ✅ | ✅ |
| MCP | ✅ | ✅ | **❌** | ✅ | ✅ | ✅ | ✅ | ✅ 核心 | ✅ | ✅ |
| hooks | ✅ 33 事件 | ✅ 12 事件 | **❌** | ✅ `PreToolUse` | ? | ✅ | ✅ 插件 | ✅ | ✅ 插件 | ? |
| skills | ✅ | ? | **❌** | ? | ✅ | ✅ | ✅ | ✅ recipes | ✅ | ? |
| **内置 checkpoint / undo** | ✅ | **❌** | git only | ✅ shadow git | **❌** | ✅ | ✅ | **❌** | **❌ 主动删除** | **❌** |
| 会话 resume | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| 会话 fork | ✅ | ✅ | ❌ | **❌** | ✅ | **❌** | ✅ | ✅ | **❌ 已删** | ✅ |
| plan 模式 | ✅ | ✅ 需开启 | **❌** | ✅ Plan/Act | ✅ preset agent | ✅ 默认开 | ✅ 权限实现 | ✅ 映射为 chat 模式 | **❌** | ? |
| todo / 目标工具 | ✅ | ✅ 需开启 | **❌** | **❌ 已废弃** | ✅ `GoalController` | ✅ | ✅ | ✅ `todo_write` + 隐藏续跑 | ✅ | ? |
| subagent | ✅ | ✅ 需开启 | **❌** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ⚠️ beta |
| 自动加载 `AGENTS.md` | **❌ 用 CLAUDE.md** | ✅ | **⚠️ 需配置** | ? | ✅ 推荐 | **❌ 用 GEMINI.md** | ✅ | ✅ | ✅ | ✅ 源码验证 |
| 多 provider 抽象 | ✅ | **❌ 仅 `responses`** | ✅ 最广 | ✅ | ✅ litellm | **❌ 仅 Google** | ✅ | ✅ | ✅ | ✅ |
| headless 模式 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **官方 eval / 回归 harness** | ? | **❌ 用 1003 个快照** | 排行榜 | ✅ `evals/` | 独立 repo | ✅ behavioral evals | **❌ 用 382 个测试** | ✅ conformance | **❌ 公开反对** | **❌** |

**从这张表能直接读出的四个结论：**

1. **checkpoint/undo 是 5:5 平局**（Claude Code / Cline / Gemini / opencode / aider-git 有；Codex / goose / Amp / Continue / OpenHands 没有，Amp 还主动删）。→ **内置 checkpoint 不是门槛，"有撤销路径"才是。**
2. **aider 是"减法做到了极致仍然成功"的最好证据**：它**没有**沙箱、没有路径白名单、没有命令黑名单、没有 MCP、没有 subagent、没有 fork、没有 todo、没有 hooks、没有 OTel、甚至**不加载 AGENTS.md**（需在 `.aider.conf.yml` 里手写 `read: AGENTS.md`）。它是一个被广泛使用的成功 agent。
3. **"专用 read/grep/glob 工具"和"标准 tool-calling 协议"两项都有成功实现省略**（Codex 只有 shell+patch；aider 和 mini-SWE-agent 连 tool-calling 都不用，靠解析文本 edit format）。→ 这两项是**效率优化**，不是能力前提。
4. **官方 eval harness 的缺失是多数派**（Codex / opencode / Amp / Continue 明确没有；它们用快照测试和普通测试替代）。→ 见第 15 域。

**⚠️ 但也要看到表的另一面**：`❌` 最多的 aider 和 Continue，前者一直没做，后者**已经冻结停止维护**（`continuedev/continue` 仓库已 read-only，最终版 2.0.0，[README](https://raw.githubusercontent.com/continuedev/continue/main/README.md)）；而**功能最全的 Claude Code / Cline / opencode 反而是最活跃、用户增长最快的**。所以"能砍"不等于"砍了就一样好"——砍掉的是**必要性的证明**，不是**价值的证明**。



# 结论二：自用极简 agent 可以砍掉什么

明确**不做**，且不会让 agent "不可用"。凡是标注了「harness 贬值」的，都是"用 scaffold 弥补模型弱点"的类型，应该最后做、而且要先证明模型确实需要它：

1. **子 agent 与多 agent 编排** —— mini-SWE-agent 无 subagent、只有 bash，仍 >74% SWE-bench verified（[来源](https://mini-swe-agent.com/latest/)）。上下文压力用"截断 + 丢弃"就能顶住。Amp 更直接：harness 不再是瓶颈。（「harness 贬值」）
2. **MCP client** —— 纯增量能力；核心工具集自足。且工具一多模型更容易选错，反而有害。
3. **skills / 自定义工具注册** —— 有了 AGENTS.md 就够了。自用场景直接改代码比配扩展快。
4. **repo map / 符号检索 / 向量检索** —— 需要 tree-sitter + 图排序的独立工程；中小仓库里模型自己 grep 更可解释。
5. **AST / tree-sitter 编辑** —— **本次对照的 10 个实现里零个把它当编辑路径**（Gemini CLI 已作 verified absence）。
6. **unified diff 编辑格式** —— 只有 aider 在用（为迁就特定模型）。注意 Codex 的 `apply_patch` 是另一回事——那是它自己定义的 patch DSL，且代价是**连 read/write/grep/glob 都不提供**；search-replace 更省事。
7. **OS 级沙箱（seatbelt/landlock/容器）** —— 平台差异大（Gemini 的 seatbelt profile 就有 6 种）；自用 agent 在可信仓库里跑。
8. **权限分类器模型 / 完整 allow-deny DSL / 声明式策略文件** —— 三档模式 + 危险命令黑名单覆盖自用需求。
9. **todo / plan 工具** —— 先用 prompt 约定替代（"先给计划再动手"、"不要编辑任何文件"）。Cline 已把 Focus Chain 废弃且无替代品。（「harness 贬值」）
10. **全屏 TUI、状态栏、diff 语法高亮** —— plain 输出对 headless 和 eval 都更友好。
11. **会话 fork / branch、交互式 rewind 菜单、shadow git、自建快照系统** —— **对照表里 checkpoint/undo 是 5:5 平局**（十个实现里五个没有，Amp 还是主动删除的）；fork 更弱：Gemini 没有、Amp 加了又删、Cline 和 Continue 各只有一边有。"每次编辑自动 git commit" 就够了。
12. **SWE-bench 跑分与多模型 dashboard** —— **多数实现根本没做 eval harness**（Codex 用 1003 个快照、opencode 用 382 个测试、Amp 公开反对、Continue 的 `eval/` 只有一个 `.gitignore`）。这是"证明给别人看"，不是"自己用得更好"。
13. **OpenTelemetry / trajectory 浏览器 / 结构化日志** —— transcript 落盘 + grep 就够用。
14. **原生多 provider 协议（Anthropic Messages / Gemini）** —— OpenAI-compatible 一个 client 覆盖 90%。顺带一提：Gemini CLI 和 Codex 反过来只支持自家/极少数 provider，因为**它们控制模型**，你不控制。
15. **显式 prompt cache 断点调优、硬预算上限、弱模型分流** —— 保留"前缀稳定"这一条免费优化即可。
16. **图片 / PDF / notebook 读取、web 搜索、LSP、浏览器工具、秘密打码** —— 领域专用工具，与"读代码改代码跑命令"的核心闭环无关。
17. **IDE 集成、Slack/Web/移动端接入、会话分享** —— 形态扩展，不是能力。

**唯一"应该做但可以晚做"的一条**：`PreToolUse` hook。它被标为 🔷 而不是 ⬜，理由是三个实现各自独立地把它当作**真正的策略执行层**，而且 Claude Code 里它在**所有权限检查之前、所有模式下**触发——这给了用户一个权限规则语言本身给不了的"任意程序级收紧点"。用几十行换"安全策略 + 编辑后自动格式化 + 编辑后自动跑测试"三件事。**如果只保留一个扩展机制，保留它，而不是 MCP。**

**如果只能砍到剩三件事**：单轮循环 + read/edit/bash 三工具、工具输出截断、编辑前 git 快照。

# 结论三：最小可用功能切片（10 项，按依赖顺序）

| # | 切片 | 为什么在这个位置 | 可后置？ |
| --- | --- | --- | --- |
| 1 | **Provider client + 配置**：一个 OpenAI-compatible 流式 client；`base_url`/`api_key`/`model` 从 env + 配置文件读 | 一切的地基；先定接口（流式 chunk → 文本增量 / tool_call 增量） | 否 |
| 2 | **Agent 循环**：messages 数组 → 调用模型 → 执行 tool calls → 追加结果 → 循环；`max_steps` 兜底；模型不调工具即结束 | 这是 agent 本体。建议同时建立"发送给模型的 messages == 可落盘 trajectory"的不变量（[mini-SWE-agent 的理由](https://mini-swe-agent.com/latest/)） | 否 |
| 3 | **工具集 v1**：`read_file`（带行号、offset/limit）、`write_file`、`edit_file`（精确 search-replace，要求唯一匹配）、`run_bash`（独立进程 + timeout） | 最小闭环：能读代码、能改代码、能执行命令 | 否 |
| 4 | **输出截断与上下文预算**：工具结果上限（字符/token）+ 超预算时从最老的工具结果开始丢弃 | 紧接 3，因为 `cargo test` 一次输出就能撑爆窗口；先做粗暴版 | 否 |
| 5 | **权限门**：`readonly` / `ask` / `auto` 三档；写与命令在 `ask` 下逐次确认；路径限制在 cwd；危险命令黑名单 | 必须在"让它自动改文件"之前就位；`--yes` 也是第 8 项的前提 | 否 |
| 6 | **System prompt + `AGENTS.md`**：启动读 `./AGENTS.md`（不存在则静默跳过），作为**第一条 user message** 注入；prompt 中写清工具用法、编辑约定（必须先读再改）、先规划再动手 | 每轮都受益，10 行代码。放在 user message 而非 system prompt，可保住 system prompt 的缓存前缀（第 11 节） | 否 |
| 7 | **Git 集成**：检测工作区是否干净并在脏时提示；每次 AI 编辑前自动 commit 或快照；提供 `/undo` 或 `--no-git` | 有了它，第 5 项才敢用 `auto` 模式；20 行换"敢放手"。**不必自建快照系统**——十个实现里五个没有内置 undo（Amp 还主动删了） | 可稍后，但强烈建议同期 |
| 8 | **Headless 模式**：`agent "prompt"` 跑完退出；`--yes`；stdout 只放结果、日志走 stderr；退出码约定（失败非 0） | 与第 2 项共用循环，成本≈0，却同时解锁 CI/脚本/eval 三个场景 | 可后置（但很便宜，建议早做） |
| 9 | **Mock provider + 端到端测试**：fake 模型返回固定 tool-call 序列；在临时 git 仓库里断言文件内容与工具调用序列 | 建议与第 3 项同步开始：从这一刻起，每次改 prompt/工具描述都有回归保护。这是八家实现真正的防退化手段，比任何 benchmark 都实用 | 不可省，但可以先只有 3 个用例 |
| 10 | **会话落盘 + `--continue`**：messages 追加写 JSONL（每条消息立即落盘）；`--continue` 读最新会话恢复 | 长任务的中断恢复；也是第 16 域 transcript 的同一份数据 | ✅ 可以后置 |

**顺序上的三点说明**

- 1→2→3→4→5 是严格依赖链，中间任何一环缺失都会让 agent 直接不可用。
- 6、7、8、9 都可以与 3~5 并行推进，因为它们互不阻塞；其中 **9（测试）建议尽早**，因为它是唯一能防止后续所有调整退化的东西。
- 第 10 项以及"# 结论二"里的全部 17 条，都可以在第一版跑通并真正用起来之后再说。

**紧接着这 10 项之后最值得做、但都不在切片内的三件事（按 ROI 排序）**

1. **编辑失败的降级匹配**（第 2 节）：先只做两层——"每行 trim 后匹配"和"忽略行尾空白"——就能吃掉相当一部分无谓的编辑失败。opencode / cline / gemini-cli 三家独立实现同一机制，说明这是真实痛点。
2. **`PreToolUse` / `PostToolUse` hook**（第 9 节）：一次性解决"可编程安全策略 + 编辑后格式化 + 编辑后自动跑测试"。
3. 自动 compaction（~90% 阈值，第 3 节）：等到第一次真的爆窗再做也不迟。

---

## 参考来源

**Anthropic / Claude Code**
- [How Claude Code works（agentic loop、工具分类、会话与 fork）](https://code.claude.com/docs/en/how-claude-code-works)
- [Tools reference（Read/Write/Edit/Bash/Glob/Grep 的具体行为与限制）](https://code.claude.com/docs/en/tools-reference)
- [Explore the context window（启动加载项、compaction 后重新注入表）](https://code.claude.com/docs/en/context-window)
- [Choose a permission mode（六种权限模式、protected paths、critical paths）](https://code.claude.com/docs/en/permission-modes)
- [Checkpointing（`/rewind` 与它的五条局限）](https://code.claude.com/docs/en/checkpointing)
- [Hooks reference](https://code.claude.com/docs/en/hooks)
- [Store instructions and memories（CLAUDE.md 层级与 auto memory）](https://code.claude.com/docs/en/memory)
- [Manage sessions](https://code.claude.com/docs/en/sessions)
- [Subagents](https://code.claude.com/docs/en/sub-agents)
- [Sandboxing](https://code.claude.com/docs/en/sandboxing)
- [Costs / prompt caching](https://code.claude.com/docs/en/costs)
- [Building effective agents（工作流 vs agent、编排模式、ACI 与 poka-yoke）](https://www.anthropic.com/engineering/building-effective-agents)
- [Writing effective tools for agents（工具设计原则、25k token 上限、eval 驱动优化）](https://www.anthropic.com/engineering/writing-tools-for-agents)
- [Parallel tool use](https://platform.claude.com/docs/en/agents-and-tools/tool-use/parallel-tool-use)
- [Prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)
- [Permissions（deny>ask>allow，忽略 specificity）](https://code.claude.com/docs/en/permissions)
- [Agent SDK: agent loop（只读并发 / 有副作用串行）](https://code.claude.com/docs/en/agent-sdk/agent-loop)
- [Monitoring / OTel（`claude_code.*` 指标清单）](https://code.claude.com/docs/en/monitoring-usage)

**OpenHands**
- [仓库（注意：原 All-Hands-AI/OpenHands 已迁移）](https://github.com/OpenHands/OpenHands)、[文档（原 docs.all-hands.dev 已重定向）](https://docs.openhands.dev/)
- [Python agent SDK（V1 主体）](https://github.com/OpenHands/software-agent-sdk)
- [Benchmarks / eval harness（独立仓库，不在 SDK 内）](https://github.com/OpenHands/benchmarks)

**Cline（续）**
- [Tools reference（当前内置工具集；旧 XML 工具名已退役）](https://docs.cline.bot/tools-reference/all-cline-tools)
- [SDK events（停止条件枚举 completed / max_iterations / aborted / mistake_limit / error）](https://docs.cline.bot/sdk/events)
- [Hub-spoke 运行时架构](https://docs.cline.bot/sdk/architecture/hub-spoke)
- [开源 eval harness（contract→smoke→e2e、pass@k / pass^k / flakiness、provider 路由差异）](https://cline.ghost.io/open-sourcing-evals-for-open-weight-agents/)

**opencode（续）**
- [权限实现 `permission/index.ts`（last-match-wins，无匹配兜底 `ask`）](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/permission/index.ts)
- [命令 arity 字典 `arity.ts`（tree-sitter 解析 + 参数个数）](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/permission/arity.ts)
- [子 agent 权限继承 `subagent-permissions.ts`](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/agent/subagent-permissions.ts)
- [上下文溢出判定 `overflow.ts`（`limit.input − min(20_000, maxOutputTokens)`）](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/overflow.ts)
- [会话主循环 `prompt.ts` 与处理器 `processor.ts`](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/session/processor.ts)
- [快照实现 `snapshot/index.ts`](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/snapshot/index.ts)
- [Server / attach 模式](https://opencode.ai/docs/server/)

**goose（续）**
- [Developer extension（恰好 5 个工具；`edit` 用 `before`/`after`）](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/developer/mod.rs)
- [权限配置 `permission.rs`（`always_allow`/`ask_before`/`never_allow` + 主体区分）](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/config/permission.rs)
- [续跑命令 `execute_commands.rs`（`/goal`、`/grind`、`todo_write`）](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/execute_commands.rs)
- [subagent `summon.rs`（独立预算）](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/summon.rs)
- [会话分享 `nostr_share.rs`](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/session/nostr_share.rs)
- [agent 循环 `agent.rs`（`stream::select_all` 并行）](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/agent.rs)
- [ACP 模式映射 `acp/provider.rs`（plan → Chat）](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/acp/provider.rs)

**Continue**
- [README（确认仓库 read-only、停止维护）](https://raw.githubusercontent.com/continuedev/continue/main/README.md)
- [默认 system prompt（并行仅限只读工具）](https://raw.githubusercontent.com/continuedev/continue/main/core/llm/defaultSystemMessages.ts)

**aider**
- [Edit formats（whole / diff / udiff / editor-*）](https://aider.chat/docs/more/edit-formats.html)
- [Repository map（tree-sitter 符号 + 图排序 + `--map-tokens`）](https://aider.chat/docs/repomap.html)
- [Git integration（自动 commit、`/undo`、dirty file 处理）](https://aider.chat/docs/git.html)
- [Scripting aider（`--message`、`--yes`、`--dry-run`、Python API）](https://aider.chat/docs/scripting.html)
- [Linting and testing（`--lint-cmd` / `--test-cmd` / `--auto-test`）](https://aider.chat/docs/usage/lint-test.html)
- [Connecting to LLMs（provider 与 model alias）](https://aider.chat/docs/llms.html)

**其他实现与标准**
- [AGENTS.md 官方规范（60k+ 项目、最近优先、冲突解决）](https://agents.md/)
- [Model Context Protocol 规范 2025-06-18（JSON-RPC 2.0、tools/resources/prompts、用户同意为 MUST）](https://modelcontextprotocol.io/specification/2025-06-18)
- [mini-SWE-agent（100 行、bash-only、线性历史、>74% SWE-bench verified、litellm）](https://mini-swe-agent.com/latest/)
- [Gemini CLI behavioral evals（EDK、policy 分级、断言工具调用、去抖与 dashboard）](https://github.com/google-gemini/gemini-cli/blob/main/docs/behavioral-evals.md)
- [SWE-bench Verified](https://www.swebench.com/verified.html)

**OpenAI Codex CLI**
- [仓库](https://github.com/openai/codex)
- [并行工具执行 `parallel.rs`（默认串行 + `FuturesOrdered` + per-tool `RwLock`）](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/parallel.rs)

**Gemini CLI**
- [仓库](https://github.com/google-gemini/gemini-cli)
- [编辑工具 `edit.ts`（严格唯一匹配 + 模糊自愈入口）](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/edit.ts)
- [LLM 编辑修正器 `llm-edit-fixer.ts`（Levenshtein + `FixLLMEditWithInstruction` + SHA-256 外部修改检测）](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/utils/llm-edit-fixer.ts)
- [写入工具 `write-file.ts`（拒绝省略占位符）](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/write-file.ts)
- [工具声明 `base-declarations.ts`（完整工具表：`replace` / `write_todos` / `enter_plan_mode` 等）](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/definitions/base-declarations.ts)
- [确认结果与"批准前手改参数" `modifiable-tool.ts`](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/tools/modifiable-tool.ts)
- [会话存储 `storage.ts`](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/config/storage.ts)
- [CLI 配置（`--fake-responses` / `--record-responses` / `--output-format` / `--resume`）](https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/config/config.ts)
- [`/rewind` 文档（git snapshot；明确不回滚 shell 造成的改动）](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/rewind.md)
- [自动 checkpoint 与 `/restore`](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/checkpointing.md)
- [plan 模式（默认启用；`enter_plan_mode` / `exit_plan_mode`）](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/plan-mode.md)
- [memory 文档（确认无 `save_memory` 工具）](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/memory.md)

**Cline**
- [仓库](https://github.com/cline/cline)
- [Checkpoints（独立 shadow git 仓库、每次工具调用后提交、三种恢复选项及其代价）](https://docs.cline.bot/core-workflows/checkpoints)
- [Auto Approve（模型给命令打 `requires_approval` 标记，没有固定白名单）](https://docs.cline.bot/features/auto-approve)
- [Plan & Act（双模式、可各配模型、`/deep-planning`）](https://docs.cline.bot/core-workflows/plan-and-act)
- [Deprecations（Focus Chain 被废弃且无替代品）](https://docs.cline.bot/resources/deprecations)
- [`.clineignore` 明确不是安全边界](https://docs.cline.bot/customization/clineignore)

**opencode**
- [仓库（注意：原 sst/opencode 已迁移）](https://github.com/anomalyco/opencode)
- [Permissions（allow/ask/deny、last-match-wins、`external_directory`、`doom_loop`、`.env` 默认 deny）](https://opencode.ai/docs/permissions/)
- [Tools（`edit` / `apply_patch` / `todowrite` / `lsp`）](https://opencode.ai/docs/tools/)
- [编辑实现 `edit.ts`（10 层 fallback replacer + per-path Semaphore）](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/tool/edit.ts)
- [CLI（`run` / `--fork` / `--format json` / `serve`）](https://opencode.ai/docs/cli/)
- [Config（`snapshot` 内部 git 仓库与 `/undo`）](https://opencode.ai/docs/config/#snapshot)

**Goose**
- [仓库（注意：原 block/goose 已迁移）](https://github.com/aaif-goose/goose)、[文档](https://goose-docs.ai/docs/)（原 block.github.io/goose 已 404）
- [CLI 命令（`session --fork` / `--edit` YAML 编辑会话 / `run --output-format` / `--max-turns`）](https://goose-docs.ai/docs/guides/goose-cli-commands/)
- [权限模式（`auto` 为默认 / `approve` / `smart_approve` / `chat`）](https://goose-docs.ai/docs/guides/managing-tools/goose-permissions/)
- [MCP Conformance CI（带 expected-failure 基线）](https://github.com/aaif-goose/goose/blob/main/.github/workflows/mcp-conformance.yml)
- [Model Tool Call Conformance（每日 cron，明确非 PR gate）](https://github.com/aaif-goose/goose/blob/main/.github/workflows/model-toolcall-conformance.yml)
- [Developer extension（五个工具，无 undo）](https://github.com/aaif-goose/goose/blob/main/crates/goose/src/agents/platform_extensions/developer/mod.rs)

**Amp**
- [文档](https://ampcode.com/docs)
- [The Coding Agent Is Dead（"a simple tool called bash is often enough"）](https://ampcode.com/news/the-coding-agent-is-dead)
- [Neo 发布说明（默认不再询问；否定 `rm -rf` 静态检查；compaction 回归 90% 自动触发；删除文件回滚）](https://ampcode.com/news/neo)
- [用 400 行 Go 讲清 agent 循环](https://ampcode.com/notes/how-to-build-an-agent)
- [Context management（thread=上下文窗口；`@` 注入的截断限制）](https://ampcode.com/guides/context-management)
- [AGENTS.md 查找顺序与 `agents-md list`](https://ampcode.com/docs/customize/agents-md)
- [tool-level permissions（`allow` / `reject` / `ask` / `delegate`）](https://ampcode.com/news/tool-level-permissions)
- [Tool set（`oracle` / `undo_edit` / `Task` / `todo_*`）](https://ampcode.com/docs/tools)
- [Plugins（`session.start` → `agent.end` 事件即 hook 系统）](https://ampcode.com/docs/customize/plugins)
- [streaming-json（`stop_reason` / `num_turns` / `usage` 含 5m+1h 两级缓存）](https://ampcode.com/docs/cli/streaming-json)
- [execute 模式（`-x`、管道、`--stream-json`）](https://ampcode.com/docs/cli/execute-mode)
- [The Dial（用成本/能力档位取代旧模式名）](https://ampcode.com/news/the-dial)
- [model evaluation（明确质疑 eval 作为 oracle 的价值）](https://ampcode.com/news/model-evaluation)
- [Handoff（compaction 被替换的那次尝试）](https://ampcode.com/news/handoff)
- [fork 被移除](https://ampcode.com/news/stick-a-fork-in-it)
- [Security（secret redaction、audit trail）](https://ampcode.com/security)

**Continue**
- [仓库](https://github.com/continuedev/continue)
