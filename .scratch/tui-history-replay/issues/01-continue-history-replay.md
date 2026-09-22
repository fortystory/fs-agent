# grilling：重播的接缝、分帧与顺序契约

Type: grilling
Status: resolved
Blocked by: 02
Part of: ../map.md

## Question

把「`--continue` 时把历史事件加载进 TUI 转录」的**接缝与调度**定到可实现级。

**范围级决定已在 charting 冻死**（见 `map.md` Notes 的冻结项）：只覆盖 `--continue`；重播整条日志；每帧 ≤ 512 条事件的分帧 + 底部状态行进度；重播期间不接受提交、live 事件缓冲到完成后按序追加；banner 在重播完成后追加；原样重播全部块。**本票只定「怎么做」**，不重开这些。

**本票的前身**：`.scratch/tui-history-replay/issues/01-continue-history-replay.md` 原本是 tui-ux 图判出 scope 后另立的独立票，它的「要不要做」已由本次 charting 回答；`## Question` 已改写为接缝与调度。

## 需要定

1. **历史从哪来、谁驱动**：渲染器现在只拿到 `TuiOptions { port, facts }`，**拿不到 `EventLog`**。是给 `TuiOptions` 注入一个历史来源（`Vec<Event>` / 只读 handle / 分页器），还是让 CLI 侧把历史事件经渲染通道推进去？给出一处接缝，并说明它为什么是**最高**的那一层（尽量不新增接缝）。
2. **分帧的闸门**：重播挂在哪一路（现有 `tokio::select!` 的 tick arm / 专用 arm / 每次 draw 前拉一批）；「一帧 ≤ 512 条」的预算怎么表达；**完成信号**谁发、谁复位状态行。（票 02 事实 23/27：单帧成本由**块与源行**决定，`Pane::evict` 在上限处是 O(20 000)/行——预算可能该按块 / 源行加权，而不是只数事件。）
3. **顺序保证**：live 事件缓冲放在哪（`TuiState`？通道？）、容量多大、重播完成后按什么键（`seq`）并入；「重播期间 `Enter` 不提交」具体拦在哪一步（`submit()` 之前？`prompt_reply` 的缺省？）。
4. **进度状态行**：`恢复历史 n/m` 的精确文案与位置（复用 `hints` 行、临时换掉提示集）、出现与清除的时机；与 `Ctrl-D`/`Ctrl-C` 的 exit 提示、查看行提示的关系；`40×10` 下怎么降级。
5. **边界**：空日志 / 只有一条 / 少于 512 条 / 恰好整除；重播期间 resize；重播期间 `Ctrl-C` 与 `Ctrl-D`（重播算不算「忙」？）；重播期间 `Esc`。
6. **失败**：日志读到最后一行损坏（spec §15 的「丢末行不完整记录」）时重播到哪；注入来源读失败时的降级。

## 先读

- `map.md` 的 Notes（冻结项与与 tui-ux 的接口）
- `src/render/tui.rs` 的 `select!` 主循环 / `apply` / `draw_frame` / `status_line` / `key`
- `src/agent.rs` 的 `append_event`（唯一写路径）
- `src/cli.rs` 的 `--continue` 组装点（`stored` / `Harness` / `Renderer::tui`）
- `src/render/mod.rs` 的 `RenderEvent` / `RenderHandle`
- `research：分帧重播的接缝事实与成本实测`（票 02）的答案
- `.scratch/tui-ux/issues/06-grilling-ctrl-d-exit-confirmation.md` 的 Answer（提示行两个 exit 常量）

## 答案落点

契约级：接缝选择 + 分帧状态机 + 顺序保证 + 进度文案与降级 + 边界表。不要写实现代码。

## 进度

**100%** —— 完成。一轮 grilling、6 条决定，票面 6 个「需要定」全部覆盖；契约见 `## Answer`。

**下一步**：无（已 resolved）。解锁 `grilling：保真度、面板与 header 的历史重建` 与 `grilling：历史详情覆盖层的复用与降级` 的实现细节引用。

## Answer

**契约（2026-09-23，6 条决定）。**

### §1 接缝：CLI 在组装后推一条新的 `ConsoleRequest`

- 载体：`ConsoleRequest` 新增 **`Replay { events: Vec<Event> }`**；`ConsoleHandle` 加一个 **`replay(events)`**，照 `ConsoleHandle::catalog`（`src/render/input.rs:173-175`）的样子 `send`。
- 发送点：**CLI 在 `assemble` 之后、与 `Catalog` 同一段**（`src/cli.rs:400-407`）推一次。
- **一处对选项措辞的修正**：入口**不是** `Harness::replay`——`Harness` 不持有 console handle（字段全表 `src/lib.rs:153-171`：`session` / `provider` / `speaker` / `render` / `cancel` / `plan_restore` / `render_task`），只有 CLI 有。`Catalog` 就是「组装后一次性推控制消息」的既有先例，照它做。
- 为什么是最高的一层：console 通道是 **unbounded mpsc**（`src/render/input.rs:222-223`），**无损**；`RenderEvent` / `Transcript` / plain / headless 一行不用动；推送发生在**恢复之后**，所以 `events` 取 `Harness::events()`（`src/lib.rs:840`，**含刚合成的悬空调用结果**），不是组装前的 `read_events` 快照。
- 否掉的两条：`TuiOptions` 注入会拿到 pre-recovery 快照；新 `RenderEvent` 变体会强制 `Transcript` 与 plain / headless 处理一个只有 TUI 消费的载荷。

### §2 分帧：每轮迭代一批，双闸预算，**不等 tick**

- `TuiState` 新增 `replay: Option<Replay>`，`Replay` 至少含 `events: Vec<Event>` / `next: usize` / `produced_lines: usize`（字段名由实现定）。
- 主循环形状：`if state.replay_pending() { state.replay_batch(); } else { select! { … } }`，两路之后照常走 `port.emit` 与 `if dirty { draw }`。**重播未完成时不进 `select!`**——否则每帧被 120ms tick 卡住，98 帧 ≈ **12 s**（票 02 实测）。
- 预算：**同时**满足 `≤ 512 条事件` **且** `≤ 2000 源行`，先到先停。理由：票 02 事实 23/24/27——单帧成本由**块与源行**决定（工具输出预览的 tree-sitter 高亮、`Pane::evict` 在上限处 O(20 000)/行），只数事件会让「512 条里全是巨型工具输出」的帧爆掉。（**2026-09-23 补注**：tui-ux 落地后工具输出不再走 tree-sitter 高亮，那一项不再是重播成本；双闸仍保留，理由是 `Pane::evict` 的 O(20 000)/行与 wrap 成本。）
- 每条历史事件走 `state.apply(RenderEvent::Logged(event))`——与 live 同一条路径（票 02 事实 5/21）。
- **完成信号**：`next >= events.len()` → 清 `replay`、flush live 缓冲（§3）、`mark_dirty()`。`m == 0`（空日志）→ **完全不进重播态**、不显示进度行。
- 每批之后要显式 `mark_dirty()`（`apply` 自己会置脏，但进度行的 `n` 若只改状态不置脏就永远不重画；票 02 事实 6）。

### §3 顺序与输入

- **缓冲**：`TuiState` 加 `live_buffer: Vec<RenderEvent>`。重播期间收到的 live 事件——banner、诊断、以及恢复补写的 `ToolCallCompleted`——**先入缓冲**，重播完成后按**到达顺序** flush（到达顺序就是发送顺序，票 02 事实 7）。
- **不设上限**：重播期间循环是空闲的（没有回合在跑），真正会到的只有组装后的 banner / 诊断这类少数消息。
- **「`Enter` 不提交」拦在 `submit()` 之前**：`submit()` 开头 `if self.replay.is_some() { return; }`。草稿编辑与光标键照常。
- **滚动**：重播期间**吸底**，`PgUp` / `PgDn` / `Ctrl-G` / 滚轮一律忽略；完成后恢复正常滚动。（这条把 `map.md` 的 `Not yet specified` 里那条雾升成决定。）

### §4 进度状态行

- 文案：**`恢复历史 {n}/{m}`**（`m` = 事件总数、`n` = 已 apply 数），`DarkGray`，**复用 `hints` 行、临时替掉提示集**；完成后恢复 `status_line` / `viewer_status_line`。落 `wording.rs`（新一族 `replay_progress(n, m)`）。
- 降级：`40×10` → `恢复中 {n}/{m}`；再窄 → `恢复中`。
- 重播期间**不显示** `Ctrl-C` / `Ctrl-D` 的 exit 提示（此刻键盘只有 `Ctrl-C` 有意义，见 §5）。

### §5 边界表（重播是**一过性状态**，不复用 `busy()`）

- **不要复用 `busy()`**：它驱动 `Ctrl-C` 的「取消 vs 退出」与状态词，而重播期间 `Ctrl-C` 要**退出**（不是取消一个不存在的回合）。所以用独立的 `replay` 标志，并在 `key()` 里**优先**判：

| 键 | 重播期间 |
| --- | --- |
| `Ctrl-C` | **退出**（中断重播） |
| `Ctrl-D` | 忽略 |
| `Esc` | 忽略 |
| `Enter` | 不提交（草稿保留） |
| 可打印字符 / 编辑键 | 照常进草稿 |
| `PgUp`/`PgDn`/`Ctrl-G`/滚轮 | 忽略（吸底） |
| `resize` | 正常重排，分帧继续 |

- 空日志 / 1 条 / <512 / 恰好整除：无特殊处理；`m == 0` 走 §2 的跳过分支。

### §6 失败

- 注入来源是 `Harness::events()`（组装后），读的是已被 `EventLog::open` → `repair_before_append` 修好的日志（残尾截掉或补换行；`src/events.rs:986-1008`）。
- 若实现改从文件侧读：`read_events` **本来就**把不可解析的**末行**当残尾跳过、其余损坏才报错（`src/events.rs:959-981`）——残尾不是问题。
- **整体读失败**（文件消失 / 权限）→ **降级为不重播**：转录从空开始（与今天一致）+ 一条 `Diagnostic`，**不阻塞启动**，`--continue` 不因此失败。

### §7 给下游与 `/to-spec`

- 新增：`ConsoleRequest::Replay` + `ConsoleHandle::replay` + `wording::replay_progress` + `TuiState.replay` / `live_buffer`。不改 `events` schema、不动 plain / headless。
- 票 03 依赖：重播走 `apply`，所以面板与模式的重建是「逐条 apply 的自然结果」（票 02 事实 34-37）。
- 票 04 依赖：`Pane::evict` 会让显示行号整体平移（票 02 事实 28），历史行的命中矩形必须**按当帧重算**。
- spec 回改：`.scratch/fs-agent-v1/spec.md` 里 TUI 启动 / `--continue` 的段落；由 `/to-spec` 汇总。
