# 忙碌时 mark 的彩虹色环（脉冲）

Type: implement
Status: ready-for-agent
Blocked by: 01

> 规格：`.scratch/tui-input-pulse/spec.md` §2、§4、`Testing Decisions`（用户故事 7–13）。

## 目标

一次运行在飞时，左栏顶部的 mark **整块**在 12 帧的彩虹色环上走（100ms 一帧，1.2 秒一圈）；空闲时回到原来的静态渐变，并且**空闲时没有任何定时器在跑**。

## 落点

`src/render/tui.rs`、`tests/render_layout.rs`、`tests/render_tui.rs`。

## 具体行为

1. **色环**：`const PULSE_PALETTE: [Color; 12]`，六个色相 × 明/暗：`LightMagenta, Magenta, LightBlue, Blue, LightCyan, Cyan, LightGreen, Green, LightYellow, Yellow, LightRed, Red`。第 0 帧是 `LightMagenta`（mark 空闲态的顶部色），注释写清「绕色环、不是随机配色」以及「第一帧与空闲态相接」。
2. **画笔**：`mark_lines(pulse: Option<u64>)` —— `None` 是今天的静态渐变（上 4 行 `LightMagenta`、底行 `Magenta`），`Some(n)` 是整块 `PULSE_PALETTE[n as usize % PULSE_PALETTE.len()]`。宽度断言（38 列）照旧 `debug_assert`。调用方（`draw_sidebar_identity`）从 `state` 取 `state.busy().then_some(state.pulse)`，所以它的签名要带上 `state`。
3. **状态**：`TuiState` 新增 `pulse: u64`（初始 0）；新增 `fn tick(&mut self)`：**只在忙碌时**推进 `pulse` 并置脏，空闲时什么都不做（`is_dirty()` 保持原值）。rustdoc 写明它是脉冲的那一格，不是通用的重画钩子。
4. **归零**：`ConsoleRequest::RunState { running: false }` 时把 `pulse` 归零（下一次运行从色环第一帧开始）。`running: true` 时不重置（同一次运行的多次 RunState 不重来）。
5. **节拍与守卫**：`const PULSE_FRAME: Duration = Duration::from_millis(100)`；`Tui::run` 里建一个常驻 `tokio::time::interval(PULSE_FRAME)`（`MissedTickBehavior::Delay`），普通分支的 `select!` 增加一条 `_ = pulse.tick(), if state.busy() => state.tick()`。**守卫是重点**：空闲时那条臂不 arm、不唤醒，循环仍是三条来源；重播那条分支不加（重播不是忙碌，它有自己的批处理节拍）。
6. **注释回改**：`Tui::run` 里那段「Three sources and no timer」的注释必须改写 —— 定时器回来了，但**只为脉冲存在、且只在忙碌时 arm**，并把「为什么每次循环一个 `sleep` 不行」写进去（事件洪流会把 sleep 反复重置，脉冲再也不前进）。不改注释就等着下一个人重新删掉它。
7. **可见性**：`w < 120` 或 `h < 16` 没有 mark，脉冲照旧 tick 但不改任何像素 —— 不要为窄屏加任何动画。

## 测试

`tests/render_layout.rs`（`TestBackend` 接缝）：

1. `RunState { running: true }` 后 `tick()` 一次 → 120×24 的 mark 5 行**全部**是 `PULSE_PALETTE[1]`（逐格读 `frame[(x, y)].fg`，行 1–5）。
2. tick 满 12 次回到 `PULSE_PALETTE[0]`（色环闭合）。
3. 空闲态：顶部 `LightMagenta` / 底行 `Magenta` —— 既有的 `the_mark_is_lit_from_above_and_only_on_the_wide_rung` 原样保留，作为**空闲**的断言。
4. 窄屏（`100×24`、`60×24`、`40×24`）：忙碌 + tick 之后帧里没有任何 mark 字形、颜色也不变（「不画不变」）。
5. `tick()` 在忙碌时确实置脏（`is_dirty()` 从 false 变 true）。

`tests/render_tui.rs`（状态机）：

6. 空闲时 `tick()` 不推进、不置脏。
7. `RunState { running: false }` 之后 `pulse` 归零：忙碌跑几帧 → 结束 → 再开始，第一帧仍是 `PULSE_PALETTE[0]`（可通过重画一帧断言颜色）。

## 不做什么

mark 之外的任何动画（状态字转轮 / 输入框律动 / 转录底部动效）；窄档身份行变色；脉冲的可配置化；事件流里的任何新东西（脉冲是纯渲染器状态）。
