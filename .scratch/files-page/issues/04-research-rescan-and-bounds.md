# 04 — 重扫的触发点落在哪一层

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

[01 号票](01-page-shape.md) 决定：重扫的触发点是「任何**非只读**的工具调用之后」加「提交之后」，
判据是既有的 `Effect`，而它跑在 `spawn_blocking` 的线程上、不另起进程。这一票查清这条信号怎么从
派发那一侧走到渲染器，不写实现。

1. **`Effect` 在哪一层拿得到**：`Tool::effect(args)`（`src/tools/tool.rs`）由谁调 —— 派发器、
   会话循环，还是权限门？一次工具调用完成之后，哪一层手上同时有「工具名 / 结果 / effect」。
2. **现有的前端通道有什么**：渲染器吃的是 `RenderEvent`（`Logged` / `Diagnostic` / `Notice` …）
   与 `FrontEndEvent`，循环与前端之间还有 `ConsoleRequest`。今天「提交之后重扫」是怎么走的
   （`TuiState::submit()` 置 `file_scan_wanted`，循环调 `take_file_scan()`）—— 那是循环**拉**。
   「工具调用之后」要不要反过来**推**？既有通道里哪一条最合身，还是要新加一条。
3. **执行者与讨论者**：子 agent（执行者）的工具调用落在同一条父流上，讨论者的调用也在同一个会话里
   —— 它们的写算不算触发点（按 01 号票的判据「非只读」应该算），但要在事实层确认渲染器看不看得见。
4. **`/undo`**：它回滚上一次编辑 —— 走工具派发那条路，还是另一条（`HistorySuperseded` 加直接写盘）？
   它算不算一个触发点。
5. **`FileIndex::begin()` 的守卫在频繁触发下的行为**：连着几次非只读调用时，重扫请求怎么合并、
   有没有丢更新的可能（`Loading` 时置位、结果落地后再补发那条），在事实层确认。

## 收尾

findings 写进 `.scratch/files-page/research/02-rescan-channel.md`：逐条回答上面五问，每条给
`文件:行号` 证据；查不到的要明说「查不到」，不要推断。

## 作答

findings（逐条带 `文件:行号` 证据）在 [`../research/02-rescan-channel.md`](../research/02-rescan-channel.md)。

1. **`Effect` 在哪一层拿得到**：`Tool::effect` 全仓库只被 `Registry::facts`
   （`src/tools/registry.rs:113`）调用一次，而它唯一的上游是循环里的 `process_call`
   （`src/agent.rs:911` 等三处 `resolve_facts`）—— 所以 effect 在循环那一层第一次被算出来；
   一次调用完成之后，「工具名 / 结果 / effect」三者同时在手的只有 `process_call` 的调用体，
   收尾函数 `finish_call`（`src/agent.rs:1207`）手里已经没有 effect（`AllowedCall` 只留下
   `exclusive` 这一个投影，`ReadOnly` 与 `WritePaths` 分不开）。
2. **现有的前端通道有什么**：`RenderEvent` 里 `Diagnostic` / `Notice` 会各往转录画一行，
   `Logged` 里的 `ToolCallStarted` / `ToolCallCompleted` 不带 effect；`FrontEndEvent` 方向相反；
   只有 `ConsoleRequest`（循环→前端）具备「静默、发完就完」的形状（`Muted` / `RunState` 是现成
   先例）。今天「提交之后」是**前端置位、循环拉**（`TuiState::submit()` 置 `file_scan_wanted`，
   循环每轮问 `take_file_scan()`）；而渲染器自己算不出 effect（`TuiOptions` / `SessionFacts`
   里没有工具表），循环那一侧今天又不持有 `ConsoleHandle`（它只出现在 `src/cli.rs` 的
   `interactive_loop` 与 `run_goal_loop` 两处）。「哪一条最合身、要不要新加一条」代码里没有既存
   实现可对照 —— 查不到，只有上面这些事实边界。
3. **执行者与讨论者**：两者的工具调用都落在**同一条父流、同一个渲染器**上（执行者的
   `log` / `render` 取自父会话，`src/agent/executor.rs:110`、`131`；讨论者共享 `opened` 的
   log / render，`src/lib.rs:633`、`648-649`），渲染器看得见它们（还不分 speaker），但看不出
   effect 的差别；按 01 号票的判据两者都算触发点，而触发只能落在它们各自的 `process_call` 里。
4. **`/undo`**：**不走**工具派发 —— 直接 `std::fs::write` 写盘（`src/agent/history.rs:181`）加一条
   `HistorySuperseded`（`src/agent/history.rs:186-192`），没有 `ToolCallStarted` /
   `ToolCallCompleted`、没有权限门、没有工作区锁。它确实改了工作区，所以按判据算一个触发点，
   但流上没有任何「调用完成」那类事件可作判据，它需要在 `/undo` 那条分支
   （`src/cli.rs:1238-1246`）上单独触发。
5. **`FileIndex::begin()` 的守卫**：`Loading` 期间 `begin()` 返回 false，而 `take_file_scan()`
   先短路、**不清位**（`src/render/tui.rs:3368-3371`），位留到结果落地后的下一轮补发；
   `file_scan_wanted` 是 bool，所以 N 次触发合并成 1 次，且补发的那次遍历晚于所有触发 ⇒
   静态阅读下**不丢更新**，最多延后一轮。例外是遍历永不返回时状态钉在 `Loading`、此后每次触发
   都被吃掉（01 号票 §E 记下的那条弱点），那是永久陈旧，不是丢更新。

**查不到的两条**：第 2 问「哪条通道最合身、要不要新加一条」没有既存实现可对照；第 5 问的合并
行为是读代码路径得出的，没有实跑验证（本票不跑 cargo）。
