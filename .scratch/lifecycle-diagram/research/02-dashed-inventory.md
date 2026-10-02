# 虚线清单：CLI 不走但存在的路径

本文件是票 02 的只读取证产物（2026-10-03）。**未改任何其它文件、未跑构建**；全部论断带
`文件:行号`，读不到的地方写「未找到」。事实基线是 [`01-runtime-lifecycle-facts.md`](01-runtime-lifecycle-facts.md)，
消费者是 [`../prototype/01-drafts.md`](../prototype/01-drafts.md) 的五张图。

## 0. 盘点：生产组装点只有三个

| 组装点 | 入口 | 走到的组装函数 | `hook` 字段 |
| --- | --- | --- | --- |
| 交互式会话 | `src/cli.rs:234` | `assemble`（`src/cli.rs:380`） | `None`（`src/cli.rs:396`） |
| `discuss` 子命令 | `src/cli.rs:545` | `assemble_discussion`（`src/cli.rs:700`） | `None`（`src/cli.rs:715`） |
| `probe` 子命令 | `src/cli.rs:2102`（`probe_model` 在 `:2186`） | `assemble`（`src/cli.rs:2214`） | `None`（`src/cli.rs:2232`） |

其余构造点全在测试里：`src/cli.rs` 内三处（`:4013`、`:4099`、`:4250`，`#[cfg(test)]` 自 `:3312` 起）
与 `tests/**`。`prune` / `sessions` 不组装 harness（`src/cli.rs:105-129`）。

## 1. 逐项比对

「分类」三档：**A 可注入·CLI 不注入**、**B 尚未实现**、**C 只有库调用方/测试**。

### 1.1 该画虚线（2 条）

| 能力 | 事实（`文件:行号`） | 分类 | 插上会怎样（哪条边变实线） | 该画什么 |
| --- | --- | --- | --- | --- |
| **hook**（`hook.pre` / `hook.post`） | 端口与挂载点完整：trait `src/hooks.rs:169-183`，`pre` 调用 `src/agent.rs:896-1005`，`post` 调用 `src/agent.rs:1229-1231`、`:2118-2176`。三个生产组装点全传 `None`（`:396`、`:715`、`:2232`）。`src/` 内**零** `impl Hook`（只有 `tests/support/hook.rs:89`）；`src/config.rs` 里 `hook` 零命中 → 无配置面。库调用方可挂：`tests/hook_mount_points.rs:47`、`:298` | A + C | 图 4 `pre -.-> hooks` 变实线；图 3 的 `hook.pre` / `hook.post` 两条消息从「CLI 下恒不发生」变成真发生；流上多出 `HookExecuted`（`src/agent.rs:2094`、`:2118`） | **虚线**（图 4 现有那条足够，`pre`/`post` 共用同一个 `None`，画一条）。图 3 那两行改为 `Note`：「CLI 恒为空，只有库调用方挂得上」 |
| **MCP**（`mcp_list` / `mcp_call` / `mcp_resources` / `mcp_read`） | `src/` 全仓 `mcp\|MCP` **零命中**；已定 spec 并拆出 9 张 `ready-for-agent` 实现票（`.scratch/mcp-support/spec.md:3`、`.scratch/README.md` 表格「9 resolved + 9 ready-for-agent」）。spec 定的唯一测试接缝是**库的组装入口** | B | 工具表组装点 `src/tools/mod.rs:104-110` 之后多一层注册，图 2 `tools` 节点多一条入边（MCP 连接层 → 工具表）；四个元工具成为新工具；连接层过沙箱 | **旁注优先**：图 2 `tools` 旁注「MCP 元工具：尚未实现（9 张票）」。若票 03 要求虚线，加 `mcp[MCP 连接层]` 与一条 `-.->` 到 `tools` |

### 1.2 实线、旁注或不画（逐项排除）

| 能力 | 事实（`文件:行号`） | 分类 | 插上会怎样 | 该画什么 |
| --- | --- | --- | --- | --- |
| **headless 渲染器** | `Renderer::headless`（`src/render/mod.rs:171-173`）在 `probe` 子命令**真被构造**（`src/cli.rs:2238`，在 `#[cfg(test)]` 之外；另三处 `:4013`、`:4099`、`:4250` 在测试模块内） | 生产路径 | —（已实线） | **不画虚线**；保持图 5 的实线 + 旁注「生产组装点只有 `probe`」 |
| **asker** | 交互式 `Some`（`src/cli.rs:373`、`:394`）；discuss `Some`（`:694`、`:713`）；probe `None`（`:2229`）。生产实现只有 `ConsoleAsker`（`src/render/input.rs:223-261`）；测试替身 `tests/support/asker.rs:17`、`:57`。`None` 的语义是 `Ask` 降级 `Deny`（`src/lib.rs:86-88`） | 生产路径（含 `None` 分支） | — | **不画**；旁注「无头前端没有应答者」 |
| **questions / `UserQuestions`** | 交互式 `Some`（`src/cli.rs:377-378`、`:395`）；discuss `Some`（`:697-698`、`:714`）；probe `None`（`:2230`）；执行者内部 `None`（`src/agent/executor.rs:175`）。工具表按 `questions.is_some()` 决定是否宣告 `ask_user_question`（`src/cli.rs:387`、`:706`、`:2222`；`src/tools/mod.rs:90-92`、`:104`） | 生产路径 | — | **不画**；旁注「`ask_user_question` 只在交互式与 discuss 入表，probe 与执行者没有」 |
| **沙箱** | `SessionConfig.sandbox` 由 `config.session_config` 拷入（`src/config.rs:696-706`）；`availability` 缺省 `Untested`（`src/config.rs:1300`），CLI **不注入**探测结果，库惰性探一次并缓存（`src/lib.rs:315-322`）；`mode="off"` 或已有结果则不探；`workspace` 档无沙箱在组装期拒（`src/lib.rs:398-414`、`:457`） | 生产路径（默认值 + 运行期补齐） | — | **不画**。这是判据的反例：缺省值由库补齐 ≠ CLI 传了 `None` |
| **`goals_dir` / 目标循环** | 由 env 派生（`src/cli.rs:319`、`src/config.rs:866-868`），装进 `GoalSetup` 交给交互式循环（`src/cli.rs:451-456`、`:1527`、`:1107`）；`/loop` 用 `TurnStart::Injected` 驱动（`src/cli.rs:1626`）。`probe` / `discuss` **没有**目标循环入口（`src/cli.rs:105-129`）；`dir=None` 时 `/loop` 直接报错（`src/cli.rs:1463-1464`） | 前端能力差异 | — | **不画**；旁注「目标循环只在交互式前端」 |
| **Provider** | 端口 `src/lib.rs:104`（`Box<dyn Provider>`）；三处都传真实 `OpenAiProvider`（`src/cli.rs:299`、`:908`、`:2194`）。`src/` 内唯一实现是 `src/provider/openai.rs:230`；`FakeProvider` 只在 `tests/support/fake_provider.rs:132` | 生产路径 | — | **不画**；测试替身不是「CLI 不注入」 |
| **`PathLocks`** | 三处都构造 `PathLocks::new()`（`src/cli.rs:390`、`:709`、`:2225`），经 scaffold 进 `OpenedSession`（`src/lib.rs:83`、`:260-261`）并共享给每个会话（`src/lib.rs:286`、`src/agent/executor.rs:128`） | 生产路径 | — | **不画** |
| **渲染 sink（`RenderSinks`）** | `plain` 传真实 stdout/stderr（`src/cli.rs:365-371`、`:681-687`）；headless 在 probe 传 `std::io::sink()` + stderr（`:2238-2242`）；TUI 用控制台端口、不用 sink | 生产路径 | — | **不画** |
| **`SessionFacts`** | 只在 TUI 两条分支构造（`src/cli.rs:341-352`、`:654-670`）；`plain` / headless 不构造（`PlainOptions` 无此字段） | 前端专属输入 | — | **不画**；可选旁注「面板事实只喂 TUI」 |
| **动态工具（`custom__*`）** | 三处都 `tools::with_dynamic(&config.tools, can_ask)`（`src/cli.rs:387`、`:706`、`:2222`；`src/tools/mod.rs:104-110`）。声明为空则不挂，是组装期输入 | 生产路径（配置驱动） | — | **不画** |
| **`web_service` / 联网工具** | 三处都传真实服务（`src/cli.rs:388`、`:707`、`:2223`，定义 `:2306-2335`）。挂不挂由 `[web] enabled` 决定（`src/tools/mod.rs:119-130`），缺省 `false` | 生产路径（配置驱动） | — | **不画**；如要提示，旁注「缺省关」 |
| **执行者的 `questions: None`** | 明确置 `None` 并写明理由（`src/agent/executor.rs:173-175`）；`ask_user_question` 与 `task` 都因 `delegable() == false` 被 `for_executor` 滤掉（`src/tools/registry.rs:70-78`、`src/tools/ask_user.rs:200-204`、`src/tools/task.rs:59-61`） | 生产路径的结构性缺席 | — | **不画**；图 4 的 `table` 节点已经是这条实线事实 |
| **`discuss` 子命令没有 `--mode`** | `DiscussArgs` 无 `mode`（`src/cli.rs:492-501`），讨论一律读 `config.mode`（`:663`、`:712`）；交互式有 `--mode`（`:201-206`） | CLI 旗标差异 | — | **不画**；旁注可选 |

### 1.3 不进图：只有种子的需求池

`.scratch/README.md` 的表格里，形态为 `seed` 的那些（`image-input`、`git-worktree`、
`background-services`、`rag-vector-store`、`interjection-flow`、`desktop-notifications`、
`multi-role-view`、`context-injection-detail`、`tui-mermaid`）**连类型都没有**：没有 spec 的接缝、
没有端口、没有组装点。把它们画成虚线会把生命周期图变成路线图。**MCP 是例外**：它有 spec 与
已定的接缝（库的组装入口），所以够格进 §1.1；其余最多在正文「不在本图范围」一句带过。

## 2. 判据（可直接抄进 `docs/lifecycle.md` §1「画法约定」）

> **实线**：这条路径在**至少一个 CLI 生产组装点**上会被真实走到 —— 三个组装点是交互式
> （`src/cli.rs:380`）、`discuss`（`:700`）、`probe`（`:2214`）。「会被走到」不只算主路径：
> 由配置开关打开的（`[web] enabled`、`--tui`）、以及一条 **`None` 的降级分支**（probe 的
> `asker: None` 让 `Ask` 降级为 `Deny`），都算走到了。
>
> **虚线**：只表示下面两类，别的一律不画虚线。
> 1. **可注入但 CLI 不注入**：类型与端口都在、调用点也在，但**每一个** CLI 生产组装点传的都是
>    `None`，且 `src/` 内没有该 trait 的生产实现（只有 `tests/` 的测试替身）—— hook 是今天
>    唯一一条。
> 2. **尚未实现**：`src/` 里零命中，但 spec 已定并拆出实现票，也就是说它有一个**已写下来的
>    接缝**，不是一句愿望 —— MCP 是今天唯一一条。
>
> **不该画虚线的四种情形**（画实线，必要时加旁注）：
> - **默认值 + 运行期补齐**：CLI 交类型缺省值、库随后自己填 —— 沙箱的 `availability` 从
>   `Untested` 到 `lib.rs:315-322` 探一次，生产路径真的走了那件事。
> - **前端差异**：某个前端或子命令不接某能力（probe 没有 `asker` / `questions`，`discuss`
>   没有 `--mode`，非交互式没有目标循环）—— 只要另一个生产组装点接它、或不接就是那条前端的
>   定义，那就是实线分支 + 旁注。
> - **配置可选**：`[web] enabled`、`custom__*` 声明、`--tui` 都是组装期输入，缺省关不等于
>   路径不存在。
> - **测试替身**：`FakeProvider`、`ScriptedHook`、`AlwaysAllow` 只在 `tests/`；它们证明端口可
>   注入，不证明生产走得到。反过来，trait 存在、CLI 有字段、而三处全 `None`，才是虚线。
>
> **没有代码落点的 roadmap 不进图**：只有 `seed.md` 的意向连类型都没有，画进去会把生命周期图
> 变成路线图；用正文一句「不在本图范围」交代。
>
> **画法**：虚线用 `-.->`（票 03 白名单已扩认）；节点仍须先显式定义。能用旁注说清的（前端差异、
> 缺省关、演进注记）优先旁注，避免把「这条前端不接」误读成「尚未实现」。

## 3. `research/01`「存疑」五条的核对

| # | 存疑 | 核对结果 | 属本清单？ |
| --- | --- | --- | --- |
| 1 | headless 渲染器的真实组装点 | 已查实：`probe` 经 `probe_model` 在 `src/cli.rs:2238` 真的构造，`src/` 内只有这一处生产构造（另三处在 `#[cfg(test)]` 内） | **否**，存疑关闭 |
| 2 | hook 在交互式路径上永远是 `None` | 确认，且扩大到三个生产组装点全 `None`，`src/` 零实现、`config.rs` 零配置面 | **是**，§1.1 第一条 |
| 3 | `docs/web.md` 与 `dsh web` 的关系 | `src/web/` 只做出网工具后端；三个组装点都传 `web_service`，是生产实线 | **否** |
| 4 | `context/` 与 `config/` 目录边界 | 纯命名事实（`src/lib.rs:28-42` 的边界清单为准），与虚实无关 | **否** |
| 5 | `tui.rs` 等未逐行读全 | 其中的 `Renderer::headless` 已单独查实（第 1 条）；编辑器按键、`edit_file` 匹配梯降级是实线路径的内部细节，不进本清单 | **部分**，不影响虚实判定 |

## 4. 结论

- **该画虚线的只有 2 条**：`hook`（一条能力、两个挂载点，图 4 现有的 `pre -.-> hooks` 即它）
  与 **MCP**（尚未实现，建议先用旁注）。
- 其余 13 项（headless、asker、questions、沙箱、goals_dir、Provider、PathLocks、渲染 sink、
  `SessionFacts`、动态工具、`web_service`、执行者 `questions: None`、`--mode` 缺席）
  **全部实线 / 旁注 / 不画**；`FakeProvider` 一类测试替身同样不画。
- 图 3 需要一处修正：`hook.pre` / `hook.post` 两行在 CLI 下恒不发生，应改成 `Note`。
