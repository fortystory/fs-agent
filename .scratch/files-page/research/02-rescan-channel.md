# files-page 票 04 · findings：重扫的触发点落在哪一层

这是 [`.scratch/files-page/issues/04-research-rescan-and-bounds.md`](../issues/04-research-rescan-and-bounds.md)
的 findings。逐条回答那张票的五问，每条给 `文件:行号` 证据。读的是 HEAD
`a09d62c`（2026-10-06）的工作树；`src/` 一个字未改，也没跑 cargo —— 下面的结论全部是**读代码**
得出的，没有实跑验证（票里禁止跑构建）。

01 号票 §E 定下的判据（「任何**非只读**的工具调用之后」加「提交之后」，判据是既有的 `Effect`）
在本文件里当作输入，不重开。

---

## 一、`Effect` 在哪一层拿得到

**它在循环那一层第一次被算出来，唯一算它的地方是注册表。** `Tool::effect(&self, args) -> Effect`
的声明在 `src/tools/tool.rs:212`，三种取值 `ReadOnly | WritePaths | Exclusive` 在
`src/tools/tool.rs:24-33`。全仓库只有一个调用点：`Registry::facts`（`src/tools/registry.rs:102`）
里的 `src/tools/registry.rs:113`（`let effect = tool.effect(args);`），算完就打包进 `CallFacts`
（`src/tools/registry.rs:154-165`），`CallFacts.effect` 是 pub 字段（`src/tools/registry.rs:250`）。

**`Registry::facts` 的唯一调用者是 `resolve_facts`**（`src/agent.rs:1815`，调用在 `src/agent.rs:1823`），
而 `resolve_facts` 的三个调用点全在 `process_call` 体内：

- `src/agent.rs:911`：这次调用首次解析；
- `src/agent.rs:963`：`hook.pre` 的 `Rewrite` 改过参数之后重解析；
- `src/agent.rs:1046`：一次被放行的越界（`workspace` 档的区外写 / `outside_read`）放松收容之后重解析。

所以 `process_call`（`src/agent.rs:842`）从头到尾握着一个 `facts`，`effect` 就在它手里。

**别的层也碰得到 `effect`，但都在工具跑之前：**

- 权限门：`authorize` 收 `&facts`（调用点 `src/agent.rs:1025`），把 `effect: &facts.effect` 交给门
  （`src/agent.rs:1875`），门按它给裁决（`src/permissions.rs:128`、`src/permissions.rs:899`）。
- 前置钩子：`hook.pre` 的入参带 `effect`（`src/agent.rs:931`、类型在 `src/hooks.rs:141`）。
- 两者都拿不到这次调用的**结果**。

**一次调用完成之后，三者（工具名 / 结果 / effect）同时在手的只有 `process_call` 的调用体：**
`facts.effect`（`src/agent.rs:912` 起活着的 `facts`）、`pending.tool_name`（`src/agent.rs:884-890`）、
以及 `dispatch` 交回的 `outcome` —— 原地路径在 `src/agent.rs:1120-1135`，被推迟的 `task` 在
`run_deferred`（定义 `src/agent.rs:1161`，收尾在 `src/agent.rs:1185`）。

**收尾那一层不再有 effect。** `finish_call`（`src/agent.rs:1207`）只收 `pending` / `allowed` /
`started` / `dispatched` / `outcome`（结构 `src/agent.rs:815-823`，三个调用点 `src/agent.rs:1120`、
`1139`、`1185`）；`CallCompletion` 里没有 effect 字段。`AllowedCall`
（`src/tools/registry.rs:230-240`）把 effect 只剩一个投影：`exclusive: matches!(self.effect,
Effect::Exclusive)`（`src/tools/registry.rs:290`），所以拿着 `allowed` 只能判出 `Exclusive`，
`ReadOnly` 与 `WritePaths` 在它上面分不开（`src/tools/registry.rs:232` 的 `write_targets` 是另一条
事实，但它来自 `Effect::WritePaths` 的路径解析、不是 effect 本身）。`DeferredCall`
（`src/agent.rs:783-787`）同样只有 `pending` / `allowed` / `started`。

> 落点提示（只是事实，不是决定）：想在「调用完成之后」按 `Effect` 判一次，最短的位置是
> `process_call` 里那两处 `finish_call(...)` 的旁边（`src/agent.rs:1120-1135`、`1139-1150`），
> 那里 `facts.effect` 与 `outcome` 同时在手；想放进 `finish_call`（`src/agent.rs:1207`）就得把
> effect 一并传进去。

---

## 二、现有的前端通道有什么；今天「提交之后重扫」怎么走；该拉还是该推

**三条通道的现状：**

- **`RenderEvent`**（`src/render/mod.rs:79-100`）：`Delta` / `Logged(Event)` / `Diagnostic` /
  `Notice`，方向是循环 → 渲染器，载体是 broadcast（发送端 `RenderHandle`，`src/render/mod.rs:136-175`；
  收端是 TUI 的 `Tui::run`，`src/render/tui.rs:347`）。`Diagnostic` / `Notice` 不是事件，但它们
  **会被画出来**：`src/render/transcript.rs:193-202` 把它们变成 `Block::Diagnostic` / `Block::Notice`。
- **`ConsoleRequest`**（`src/render/input.rs:69-109`）：循环 → 前端，逐条是 `Prompt`（`75`）、
  `Ask`（`79`）、`Questionnaire`（`82`）、`Catalog`（`87`）、`Muted`（`93`）、`RunState`（`100`）、
  `Replay`（`107`）。循环侧那一端是 `ConsoleHandle`（`src/render/input.rs:127`），前端侧是
  `ConsolePort`（`src/render/input.rs:187`），TUI 在 `src/render/tui.rs:2907` 的 `request()` 里分支
  （`2910` / `2912` / `2917` / `2945` / `2960` / `2993` / `3000`）。
- **`FrontEndEvent`**（`src/render/input.rs:112-123`）：`Cancel` / `CycleMode` / `Quit`，方向相反
  （前端 → 循环），不承载这类事实。

**「提交之后重扫」是前端置位、循环拉：**

- `TuiState::submit()`（`src/render/tui.rs:3342`）在把草稿发出去之前置位：`self.file_scan_wanted = true;`
  （`src/render/tui.rs:3355`）。
- 渲染循环每轮开头问一次 `state.take_file_scan()`（`src/render/tui.rs:400`）；为真就
  `tokio::task::spawn_blocking` 跑 `file_index::scan`，结果经 mpsc 发回（`src/render/tui.rs:402-406`，
  通道建在 `370-371`）。
- 结果在 select 里被收下（`src/render/tui.rs:436`），交给 `files_loaded`
  （`src/render/tui.rs:3377`）。
- 进 TUI 那次预热就是同一个位：`file_scan_wanted` 初值 `true`（`src/render/tui.rs:1603`）。
- 这段意图的注释在 `src/render/tui.rs:396-399`。

**「工具调用之后」的前端看得见什么：看得见调用，看不见 effect。**

- 流上有 `ToolCallStarted { tool_call_id, tool_name, args }`（`src/events.rs:415-419`）与
  `ToolCallCompleted { tool_call_id, ok, output, error, duration_ms }`（`src/events.rs:420-426`）。
  **两者都不带 effect**，流上也没有第二种副作用分类。
- TUI 把这些事件落进转录：`apply`（`src/render/tui.rs:1826`）→ `transcript.push`
  （`src/render/transcript.rs:178-205`）→ `push_logged`（`src/render/transcript.rs:257` / `273`）；
  「现在在跑哪个工具」另有一处消费（`observe_running_tool`，`src/render/tui.rs:2171-2182`）。
- 但 TUI 手里没有工具表，因此自己算不出 effect：`TuiOptions` 的字段只有 `port` / `facts` / `cwd` /
  `reopened`（`src/render/tui.rs:317-335`），`SessionFacts` 只有 session_id / session_dir / model /
  context_window / mode / budget_limit / number_style / speaker_order（`src/render/tui.rs:285-314`）。
  算 effect 的唯一入口是 `Registry::facts`（`src/tools/registry.rs:102`），而注册表住在 `Session`
  （`src/session/mod.rs:189` 的 `tools()`、`197-199` 的 `shared_tools()`），从不进渲染器。

**「推」这件事在事实层的两条路：**

- `ConsoleRequest` 那种「循环 → 前端、发完就完、只改前端一个状态」的形状，现有先例是 `Muted`
  （`src/render/input.rs:93-99`，前端在 `src/render/tui.rs:2912-2916` 收下）与 `RunState`
  （`src/render/input.rs:100-106`，前端在 `src/render/tui.rs:2917-2944`）。今天**没有**任何变体承载
  「刚发生过一次非只读调用」这件事。
- `RenderEvent::Diagnostic` / `Notice` 不合身的事实依据是它们会往转录里各画一行
  （`src/render/transcript.rs:193-202`）—— 那是给人看的输出，不是静默信号。
- 一个结构事实：**循环那一侧今天拿不到 `ConsoleHandle`**。它只出现在 `src/render/input.rs` 的定义
  与 `src/cli.rs` 里持有它的两个函数：`interactive_loop`（签名 `src/cli.rs:1151-1153`）与
  `run_goal_loop`（签名 `src/cli.rs:1857-1859`）。执行回合的 `run_one_turn`
  （`src/cli.rs:2334-2339`）只收 `harness` 与 `events`；`Harness`（`src/lib.rs:152-167`）持有的是
  `opened: OpenedSession`（其中含 `RenderHandle`，`notice()` 见 `src/lib.rs:1053-1055`）；`src/agent.rs`
  整个文件不出现 `ConsoleHandle`。于是「推」要么把这条 handle（或它的一个能力）递进回合那一路，
  要么在 `RenderHandle` 上开一条新通道。

**「哪一条最合身、要不要新加一条」在代码里查不到答案** —— 那是设计决定，本票不含既存实现可对照。
事实层的边界是：判据（`Effect`）只在循环侧算得出来，而今天循环到前端的两条通道里，只有
`ConsoleRequest` 具备「静默、循环→前端、只改前端状态」的形状。

---

## 三、执行者与讨论者：调用落在同一条父流上吗；渲染器看得见吗

**执行者（`task` 派发的子 agent）：是同一条流、同一个渲染器。**

- `ExecutorPort` 拿的就是父会话那一份日志与那一个渲染句柄：`log: session.log().clone()`
  （`src/agent/executor.rs:110`；`EventLog` 是 `Arc<Mutex<Inner>>` 的共享句柄，`src/events.rs:942-952`）、
  `render: render.clone()`（`src/agent/executor.rs:131`）。
- 它的生命周期事件经 `append_event(&self.log, ..., &self.render, executor, ...)` 写流
  （`src/agent/executor.rs:150-166` 与 `230-245`），而 `append_event` 里就是 `render.logged(&event)`
  （`src/agent.rs:2236-2248`）。
- 执行者内部跑自己的回合：`run_turn`（`src/agent/executor.rs:184`），也就是同一个
  `process_call` 那套。所以执行者的每一次工具调用各自带着 `SpeakerId::Executor(<parent>-<n>)`
  落进父流，也各自有 `facts.effect`。

**讨论者：同样共享日志与渲染器，只是各自有 `Session`。**

- `discussion_participants`（`src/lib.rs:567-608`）用同一个 `opened.session(config, identity)`
  开每个讨论者与合成器，而 `opened` 的 log / render 就是这场会话的那一份
  （`assemble_discussion` 里 `OpenedSession::open` 在 `src/lib.rs:633`，`log` / `render` 从它取在
  `src/lib.rs:648-649`）。
- 讨论者的回合经 `run_turn(&mut debater.session, &debater.speaker, &debater.provider, render, ...)`
  跑（`src/agent.rs:1438-1449`）。合成器没有工具（注释与结构在 `src/agent.rs:1278-1285`）。

**渲染器看不看得见：看得见，且分工不辨。**

- 所有 `Logged` 事件都进转录（`src/render/tui.rs:1826` → `src/render/transcript.rs:178-205`），
  `targets()` 恒为「两个视图都收」（`src/render/tui.rs:1928-1935`），没有按 speaker 过滤。
- `observe_running_tool`（`src/render/tui.rs:2167-2182`）也不分 speaker。
- `TurnScope`（`Whole` / `Round` / `Executor`）只影响投影与 `repo_map` 的排序输入
  （`src/agent.rs:872-883`），不影响渲染通道。

**结论（按 01 号票的判据）：执行者与讨论者的非只读调用应当算触发点，而且它们的事件在渲染器眼里
与主 agent 的毫无区别。** 判据只在循环侧成立（见第一问），所以这三条路径上的触发各自发生在各自的
`process_call` 里；前端从流上分辨不出「这次调用是不是执行者/讨论者发的」「它是不是只读」。

---

## 四、`/undo`

**它不走工具派发那条路，是另一条。**

- 落点在交互循环：`Submission::Undo => harness.undo_last_edit()`（`src/cli.rs:1238-1246`）→
  `Harness::undo_last_edit`（`src/lib.rs:1070-1072`）→ `agent::undo_last_edit`
  （`src/agent/history.rs:143`）。
- 它做的每一步都不经注册表：从流上找最近一次成功且未被注销的 `edit_file`（`src/agent/history.rs:96`
  的 `last_undoable_edit`，判据在 `96-135`）；从流上重读那次调用的参数；读
  `outputs/<tool_call_id>.before`（`157-165`）；取那次编辑用的**同一把路径锁**
  （`src/agent/history.rs:166`，注释在 `137-142`）；然后 `std::fs::read_to_string`（`171`）+
  `edit::revert`（`173-179`）+ `std::fs::write`（`181`）直接写盘。
- 没有 `Registry::dispatch`、没有工作区锁、没有权限门、也没有任何 `ToolCallStarted` /
  `ToolCallCompleted` —— 这一点可以硬证：`Registry::dispatch` 全仓库只有两个调用点
  （`src/agent.rs:1118` 与 `src/agent.rs:1177`），都在回合里；`/undo` 那条路径不经过它们。
- 它进流的唯一痕迹是一条 `HistorySuperseded { reason: HistoryReason::Undo, .. }`
  （`src/agent/history.rs:186-192`；payload 定义 `src/events.rs:472`）。渲染器看得见它：转录把它画成
  `Block::History { reason, summary }`（`src/render/transcript.rs:410-412`），`wording` 里有
  「撤销」那一支（`src/render/wording.rs:1111`）。

**算不算触发点：事实是它确实改了工作区，但流上没有任何「调用完成」那类事件可以当判据** —— 只有一条
`HistorySuperseded`。所以若把触发点定成「非只读调用之后」，`/undo` 需要自己那次触发；它发生的位置
与 `ConsoleHandle` 在同一个函数里（`src/cli.rs:1151-1153` 的 `interactive_loop` 内的 `1238-1246`），
而回合内部的工具调用走的是另一条路（`src/cli.rs:2334` 的 `run_one_turn`，不传 console handle）。

---

## 五、`FileIndex::begin()` 的守卫在频繁触发下怎么合并

**守卫本身：** `FileIndex::begin()`（`src/render/file_index.rs:46`）—— `Loading` 时返回 `false` 且
**不动状态**（`48`）；其余状态置成 `Loading` 并返回 `true`（`49-53`）。它是 `Loading` 这个状态的唯一
读者，`file_index.rs:42-45` 的注释写明它保证「结果永远不会被两次并发遍历互相盖掉」。

**唯一的消费点是 `TuiState::take_file_scan()`**（`src/render/tui.rs:3367`），合并行为全在那四行里：

```
if !self.file_scan_wanted || !self.files.begin() {   // src/render/tui.rs:3368
    return false;
}
self.file_scan_wanted = false;                       // src/render/tui.rs:3371
```

短路顺序是关键：一次遍历还在飞时 `begin()` 返回 `false`，于是**早退，`file_scan_wanted` 不被清掉**，
位留到结果落地之后的下一轮再问一次。`files_loaded`（`src/render/tui.rs:3377-3381`）把索引变回 `Ready`
（`src/render/file_index.rs:57`），下一轮 `begin()` 因此返回 `true`，补发的那一次遍历才出去。这段意图
的注释就在 `src/render/tui.rs:3364-3366`。

**频繁触发的结论（静态阅读）：**

- `file_scan_wanted` 是 `bool`（`src/render/tui.rs:724`），不是计数：飞着的那一次之上的 N 次触发只留下
  一个「还要再扫一次」的标记，**N 合并成 1 次补发**。
- 补发的遍历开始于所有触发之后（结果落地之后），所以它扫的是最新状态 —— 按代码路径推，**没有丢更新
  的地方**，最坏情形是延后一轮。
- 触发点本身是幂等的置位（今天只有 `submit` 一处：`src/render/tui.rs:3355`）。
- 每次触发都要等 TUI 循环的下一轮才被看见：`take_file_scan()` 在 `loop` 开头、`if state.replay_pending()`
  **之前**（`src/render/tui.rs:400-406` 早于 `407`），所以历史重放期间也会照常发遍历。

**唯一的例外是「卡住」而不是「丢更新」**（01 号票 §E 已经记下的那条弱点，这里是它的事实依据）：
`files` 只有 `files_loaded` 一个写入口（`src/render/tui.rs:3378` 调 `src/render/file_index.rs:57`），
`self.files` 在 `tui.rs` 其余出现处都是只读（`3396` 的 `contains`、`3446` 的 `candidates`），
`begin()` 之外没有任何超时、取消或重置路径。一次永不返回的遍历会把状态钉在 `Loading`，此后每次触发
都被 `begin()` 吃掉，位留着也永远发不出去 —— 索引永久陈旧。

---

## 查不到的条目（明确说明）

- **「既有通道里哪一条最合身，还是要新加一条」**：代码里查不到答案，因为它没有既存实现可对照。
  能给的是事实边界（见第二问）：三通道里只有 `ConsoleRequest` 具备「静默、循环→前端、只改前端状态」
  的形状，而判据（`Effect`）只在循环侧算得出来，循环侧今天又不持有 `ConsoleHandle`。
- **运行时的行为验证**：本票是 research（且票面禁止跑 cargo），第五问的合并结论是**读代码路径**得出的，
  没有实跑观测。
