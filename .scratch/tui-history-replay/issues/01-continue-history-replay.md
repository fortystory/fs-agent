# grilling：重播的接缝、分帧与顺序契约

Type: grilling
Status: claimed
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
