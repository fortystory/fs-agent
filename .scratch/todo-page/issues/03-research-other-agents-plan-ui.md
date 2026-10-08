# 03 — research：别的 coding agent 怎么呈现计划列表

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

[01 号票](01-charting-decisions.md) 已经把我们要怎么画定死了（摘要页 + 弹窗、折行、折叠完成项、
页顶进度行）。这一票问的是**别人怎么做的** —— 它不推翻任何一条冻结项，而是给「我们这样选是不是
异类」提供事实，并且在**细节**上（字形、按钮长什么样、快照还是跟随）提供可借鉴的先例。

材料在本地：[`docs/research/notes/`](../../../docs/research/notes)（Claude Code、Amp、Cline、
opencode / goose、aider / openhands）与
[`docs/research/coding-agent-features.md`](../../../docs/research/coding-agent-features.md)。
**先在这些文件里找，找不到再说要不要联网**；联网的话照 `/research` 的规矩给 URL。

要查清：

1. **画在哪**：那些 harness 把计划列表放在哪儿（TUI 左栏 / 主面板 / 独立文件 / 每轮重新注入
   prompt / 压根没有）？逐个记一句，并给出文件里那一段的位置。
2. **完成的项折不折**：有没有把 `completed` 折起来、默认折还是可展开、折起来之后给不给计数。
3. **长文本**：折行还是截断，截断用什么记号。
4. **进度表达**：有没有独立进度行、占比条、还是只写在系统提示里；文案长什么样。
5. **看全的入口**：有没有详情弹窗 / 独立视图 / `Ctrl-T` 之类的键；有没有「当前项高亮」。
6. **一次提交整份还是增量**：那些工具是每次重发整份列表，还是增量改（这一条对应我们
   replace-all 的既有形状，是抄来的，不重开）。
7. **那份「独立 todo 工具」的结论**：
   [`coding-agent-features.md:372`](../../../docs/research/coding-agent-features.md) 把独立
   todo 工具**下调**了标签，说 Cline 实测后废弃 Focus Chain、增益随模型变强而衰减。把那一轮的
   结论与它对**呈现**（不是「要不要这个工具」）的观察摘出来。
8. **抄不了的地方**：哪些是 TUI / 终端尺寸差别造成的（我们是 28/40 列的侧栏），哪些是它们自带的
   重量级 GUI 造成的 —— 结论要能直接被 04 票的帧草图用。

产物：一张对照表（每家一行：画在哪 / 折不折 / 折不折行 / 进度怎么表达 / 看全入口 / 整份还是增量）
+ 上面第 7、8 两条的结论，收进本票 `## 作答`。**每条结论后面带出处**（本地文件的
`文件:行号`，或 URL）。

## 作答

调研于 2026-10-08。本地材料（`docs/research/notes/`、`docs/research/coding-agent-features.md`）
覆盖了每一家的**工具与模式**这一侧，但几乎没记**呈现**（画在哪、折不折、长文本），
所以呈现侧的证据基本靠联网补第一方文档与源码，下面逐条给出 URL。凡是查不到的直接写
「未见记录」，不猜。

### 对照表

| 家 | 画在哪 | 完成的项折不折 | 长文本 | 进度怎么表达 | 看全的入口 | 整份还是增量 | 出处 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Claude Code | 底部 **status area** 里的一张 task list（不是转录块，不是侧栏） | **可折**，`Ctrl+T` 开关；展开态会被记住（下次 `--resume` / `--continue` 恢复），**空列表时默认折**；完整视图里含 completed | 未见记录 | 无进度计数、无占比条，只有每项自己的 pending / in progress / complete 指示 | `Ctrl+T`；**上限五条**（"The display shows up to five tasks at a time"）；要看全或清空**得直接问 Claude**（"show me all tasks"） | 整份（`TodoWrite` 一次给全表） | [interactive-mode 的 Task list 段](https://code.claude.com/docs/en/interactive-mode.md)；本地 `notes/claude-code-amp.md:383-388`、`:74-81` |
| Gemini CLI | **输入框上方的 progress indicator**（全宽主列那一带） | 默认收起；`Ctrl+T` 展开的完整视图**含 completed / in-progress / pending 全量** | 未见记录（schema 只要求 `description` 非空，无长度上限） | 有一个 progress indicator + **当前项高亮**（`[IN_PROGRESS] …`）；无 `n/m` 计数 | `Ctrl+T`（官方原话就是"the full todo list might be hidden to save space"） | 整份覆盖（源码注释：`The full list of todos. This will overwrite any existing list.`） | [tools/todos](https://geminicli.com/docs/tools/todos/)、[task-planning](https://geminicli.com/docs/cli/tutorials/task-planning.md)、[write-todos.ts](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/tools/write-todos.ts)；本地 `notes/codex-gemini.md:327-331` |
| Codex | 主列转录里的一个工具单元格（tool cell） | 未见记录 | 未见记录 | 无 | 无独立视图 | 整份（`{explanation?, plan[] of {step, status}}`，至多一个 `in_progress`，**默认关闭**） | 本地 `notes/codex-gemini.md:782-788`、`:771-776` |
| opencode | TUI **侧栏里的一个 `todo` 分区**（内建分区名：`context` / `mcp` / `lsp` / `todo` / `files`） | **可折**，鼠标点分区头；折叠状态存 KV 并跨重启保留 —— 这条出自一个**尚未合并**的 PR | 未见记录 | 无 | 点自己的分区头（折/展），**不是**「另一个更大的视图」 | 整份 | [PR #51543](https://github.com/anomalyco/opencode/pull/51543)、[docs/tools 的 todowrite 段](https://opencode.ai/docs/tools/)；本地 `notes/opencode-goose.md:546-548` |
| Cline（Focus Chain，已废弃） | **压根没有 UI** —— 清单是嵌在别的工具调用参数里的 Markdown checkbox（`- [x] …`） | 不适用 | 不适用 | 无 | 无 | 整份（提示词原文要求"supply the **whole** checklist each time"） | 本地 `notes/cline-continue.md:587-634`；[deprecations](https://docs.cline.bot/resources/deprecations) |
| goose | **没有面向用户的呈现** —— `todo_write` 收一个 `content` 字符串（`GOOSE_TODO_MAX_CHARS` 默认 50000），存在 session 的 `extension_data` 里经 MOIM 回注上下文 | 不适用 | 不适用 | 无 | 无 | 整份（一整段文本） | 本地 `notes/opencode-goose.md:566-571`（本地明说它"在找到的用户文档页里没有描述"） |
| OpenHands | 重量级 GUI / web 前端里的 task tracker（另有 `GoalController` 评审 LLM） | 不适用（前端自由） | 不适用 | 不适用 | 不适用 | 整份 | 本地 `notes/aider-openhands.md:521-524`、`:1010` |
| aider | **压根没有** todo / goal 追踪 | — | — | — | — | — | 本地 `notes/aider-openhands.md:179` |
| Continue | **没有等价物**（只有 plan 模式 = 只读工具过滤） | — | — | — | — | — | 本地 `notes/cline-continue.md:2317-2321` |
| **衡（基线，非先例）** | 左栏 `todo` 页（**页签**，一屏只画一页） | 不折，全量画 | **截断**（`truncate_columns`），塞不下报 `＋N 项` | 末行 `已完成 3/11` | 无（页上点一行什么都不开） | 整份（`read_items(args)`，后落地的那条是真相） | `src/render/todo.rs:36-107`、`src/render/tui.rs:5707` |

**先例密度这件事本身**：九家里只有四家（Claude Code、Gemini、Codex、opencode）真的有终端呈现，
其中只有两家把「看全」做成了一次显式切换（`Ctrl+T`）。其余五家里两家压根没有这个工具，
两家把它埋在提示词或参数里，一家是重量级 GUI。**「摘要 + 看全入口」这个分工在别处是稀有的，
但「`Ctrl+T` 切换」是标配** —— 这是我们与先例最接近也最不一样的那一处。

### 那份「独立 todo 工具」的下调结论，以及它对**呈现**的观察

- **下调的是标签，不是呈现。**
  `coding-agent-features.md:372` 把「独立的 todo / plan 工具 + 每轮重新注入」从 🔷 降到 ⬜／🔷，
  理由是 Cline 实测后废弃 Focus Chain 且**没有替代品**（`:354` 引了官方原文
  "no longer providing enough additional benefit on top of the current harness"），
  结论一句话是**todo 清单的增益随模型变强而衰减，它是 harness 的补丁**。
  同一份文件 `:703` 的分档也把「todo 工具」放进 nice-to-have，只留「prompt 里要求先规划」当 table stakes。
- **对呈现的三条观察**（这一轮原本要记的东西）：
  1. Cline 当年的清单**根本没有 UI** —— 它是别的工具调用参数里的一段 Markdown checkbox，
     每 10 次 API 请求被提醒更新一次，靠重新注入对抗跑偏（`notes/cline-continue.md:615-633`）。
     也就是说「一份计划要看得见」这件事，**在很多实现里是靠模型上下文解决的，不是靠屏幕**。
  2. 反过来，被保下来的那几家把呈现当成了留存手段：Claude Code 明写清单
     **survives context compactions**，跨会话还能靠 `CLAUDE_CODE_TASK_LIST_ID` 共享一个目录
     （[interactive-mode](https://code.claude.com/docs/en/interactive-mode.md)）。
     换句话说，**呈现不是装饰，它是「重新注入」这条机制的人类可读面**。
  3. goose 是最省的样本：`todo_write` 用户文档里都没写，主要价值在回注上下文而不是给人看
     （`notes/opencode-goose.md:566-571`）。
- **对我们的直接影响**：那条下调针对的是「要不要这个工具」，本图的冻结项 2 已经把工具整个划在范围外，
  所以它不构成重开任何冻结项的理由。它只支撑本图已经选定的那条立场 ——
  **这一页是纯读的，不让前端改状态**（map 的「明确不做」第 2 条）。

### 抄不了的地方

**（一）终端尺寸预算造成的 —— 它们的数字不能搬**

1. **Claude Code 的「一次五条」是状态区的纵向预算，不是侧栏的**（[interactive-mode](https://code.claude.com/docs/en/interactive-mode.md)）。
   我们左栏页区的高度是「内容行减身份与页签条」（`src/render/layout.rs:528-556`；
   120×24 整块 40×15、80×24 整块 28×19，见 map 的「起点」第 3 条），
   它的 status area 就那么几行。**结论：那是本轮唯一一个公开的、可量化的呈现预算先例，
   但它给的锚是「几条」而不是「几行」** —— 04 票量帧草图时可以借这个问法（按条还是按行做上限），
   不能借那个数字 5。
2. **Gemini 的 progress indicator 是全宽主列那一带**（[tools/todos](https://geminicli.com/docs/tools/todos/)）。
   我们只有 28 列（窄档）：同一条「进度 + 一个开关」在这里会挤成两三行。
   **结论：04 票必须分别量 40 列与 28 列两档**，不能拿 Gemini 那张图当参照系。
3. **opencode 的「分区可折 / 可排序 / 可隐藏」对应的是我们的页签，不是我们的「折掉完成项」**
   （[PR #51543](https://github.com/anomalyco/opencode/pull/51543)）。
   它有 5 个分区争一块固定高度，所以必须给折叠与隐藏；我们左栏是页签，一屏只画一页，
   不存在分区竞争。**结论：别把「可折叠的 todo 分区」当成我们已完成项折叠的先例 —— 那是两件事。**
4. **Codex 的清单曾经活不到下一回合**（issue #18920，用户诉求是让清单在等用户输入时常驻，
   2026-07-21 以 completed 关闭）。那是个工具单元格里的一次性渲染问题；
   我们那份列表是渲染器状态、由事件流重算（`src/render/todo.rs:36-49`），
   **结构上不存在对应物，不必为此写一条我们的对策**。

**（二）重量级 GUI 造成的 —— 连形状都不能比**

5. **OpenHands 的 `task_tracker`、`GoalController`（评审 LLM 判达成、`max_iterations = 10`）、
   Cline 的 Kanban 依赖板与依赖链**（`notes/aider-openhands.md:1010`、`notes/cline-continue.md:636-638`）
   都活在浏览器 / IDE 前端里：可以有依赖图、拖拽、任意滚动、tooltip。
   纯 ratatui 的 TUI 一条都没有。**结论：这几家在对照表里只能进「机制」栏，不进「版式」栏。**
6. **Claude Code 的 `CLAUDE_CODE_TASK_LIST_ID` 跨会话共享目录**是存储策略，不是呈现。

**（三）反过来 —— 我们是真异类的三处，以及各自的现状**

7. **入口是鼠标按钮，没有一家这么做过。** Claude Code 与 Gemini 都用 `Ctrl+T` 这把**专门的键**
   （opencode 是点自己那一区的头，那是折/展不是「另一个更大的视图」）。
   我们冻结项 7 明说**不给专门的键**、只用鼠标 —— **这是实打实的异类**。
   它与本仓库自己的约定自洽（页签从不给键位，`CONTEXT.md:307`），但 spec 里要**明写理由**，
   别默认读者以为有先例。
8. **完成项默认藏起来，也没有一家这么彻底。** Gemini 与 Claude Code 都是「默认收起 + 一个开关看全」，
   而我们是**页上连展开开关都没有**、completed 只活在弹窗里（冻结项 6）。
   这一条比第 7 条更值得在 spec 里说清楚：页顶那条 `已完成 3/11` 是**页上唯一的完成度信息**。
9. **`n/m` 形式的进度文案，两家都只有半个。** Claude Code 有清单没计数；
   Gemini 有 progress indicator 但没有 `n/m` 文案；Codex 与 opencode 都没有。
   我们那条 `3/11 · 1 个在做`（冻结项 10）**是先例里的空格**，也是本票能给出的最具体的一条正面借鉴。
10. **快照 vs 跟随这一条没有先例，因为「摘要页 + 详情弹窗」这个分工本身就没有先例。**
    别家的都是「同一个可切换的视图」——切过去看的是活的那一份。我们是**页跟随、弹窗是打开那一刻的快照**，
    正好是它们的分工（常驻一份 + 你自己再去看）在一个更窄的容器里的压缩版。
    **结论：冻结项 8 不必改，但要意识到它是从别家的形状推不出来的那一条。**

**（四）一次提交整份还是增量 —— 抄来的那条被完全证实**

11. 九家里**没有一家做增量**。Gemini 的源码注释直说 `This will overwrite any existing list`；
    Codex 的 `plan[]` 是整份；Claude Code 的 `TodoWrite` 是整份；
    Cline 当年要求 "supply the **whole** checklist each time"；goose 是一整段 `content` 文本。
    我们的一次提交整份、列表活在 `tool_call` 的 args 里，是**满票一致**的做法。
    配套的那条也一致：Gemini 的渲染层拿的是 `returnDisplay: { todos }` 这个结构，
    **不是**从回给模型的 `llmContent` 文本里解析 —— 与我们「从不从结果文本里解析」的立场相同
    （`docs/render.md:40-43`）。