# 空闲时的退出手势时钟：一个按需武装的 deadline

Type: implement
Status: done
Blocked by: 01

> 规格：`.scratch/exit-gesture/spec.md` §6（时钟：一个按需武装的 deadline）、§7。
> 举手那 500 毫秒需要一次唤醒，这是领域词条「脉冲」里那句「空闲时那台时钟不存在」的唯一有界例外。举手状态、`EXIT_GESTURE_WINDOW` 与 `expire_exit_gesture()` 由 [票 01](01-gesture-and-hints.md) 建立；本票只把它接进主循环，并改掉那句现在会骗人的注释与词条。

## 目标

- 主循环多一个**只在举手期间武装**的 deadline 分支；其余时刻空闲会话照旧零唤醒。
- 到点调 `expire_exit_gesture()`：清字段、置 `dirty`、提示行恢复。
- 把 `src/render/tui.rs:331-336` 那段"唯一时钟"的注释改写成"两个，第二个有界"。
- 同步改 `CONTEXT.md` 的「脉冲」词条（它写着「空闲时那台时钟不存在」）。

## 现状（2026-10-01 量的，改前先复核）

- 主循环：`src/render/tui.rs:337-338` 建 `pulse = tokio::time::interval(PULSE_FRAME)` 并设 `MissedTickBehavior::Delay`；非重放那支 `select!` 里唯一带时间的分支是 `:368` 的 `_ = pulse.tick(), if state.busy() => state.tick(),`。
- `:331-336` 的注释明写"这个循环里唯一的定时器，而且只在一次运行进行中的时候才武装"，`:356-361` 补充"空闲时这个 `select!` 又只是三个来源"。
- `CONTEXT.md:238-240` 的「脉冲（Pulse）」词条末句是「**空闲时那台时钟不存在** —— 没有唤醒、没有重画」。
- 票 01 已经给 `TuiState` 加了 `exit_deadline: Option<std::time::Instant>`、常量 `EXIT_GESTURE_WINDOW`（500ms）与 `pub fn expire_exit_gesture()`；本票不改它们的状态机语义。

## 落点

- `src/render/tui.rs`：`TuiState` 的一个只读访问、`run` 的非重放 `select!`、`:331-336` 的注释、`:356-361` 的注释。
- `CONTEXT.md`：「脉冲」词条。

## 具体行为

1. `TuiState` 暴露一个只读访问（名字实现定，例如 `pub fn exit_deadline(&self) -> Option<std::time::Instant>`）——主循环用它武装分支，字段本身仍然只有 `TuiState` 自己改。
2. 非重放的 `select!`（`src/render/tui.rs:365` 那一段）与 `pulse` 并列加一个分支：

   ```rust
   _ = tokio::time::sleep_until(tokio::time::Instant::from(deadline)), if deadline.is_some() => state.expire_exit_gesture(),
   ```

   其中 `deadline` 在 `select!` **之前**从 `state.exit_deadline()` 取成值（`Instant` 是 `Copy`，取完借用就结束，分支里才借得到 `&mut state`）；未举手时给它一个占位时刻（`Instant::now()`），因为 `select!` 会先求值各分支的 future —— 关掉 poll 的是分支上的 `if`，与 `pulse` 现在的做法同构。
   - `tokio::time::sleep_until` 收 `tokio::time::Instant`；`std::time::Instant` 经 `From` 转过去。
   - 每轮循环重新建这个 future，所以举手后 deadline 变了下一轮就生效，不需要额外的重置逻辑。
   - 只有举手的那 500 毫秒里才有这次唤醒，其余时刻空闲会话照旧一次唤醒都没有（spec §6 的"放宽必须是有界的"）。
3. 注释改写：`:331-336` 从"这个循环里唯一的定时器"改成"两个定时器：`pulse`（60ms、只在运行中）与 `exit_deadline`（500ms、只在举手时）"；`:356-361` 那句"空闲时这个 `select!` 又只是三个来源"改成"空闲时是三个来源，举手时多一个有界的 deadline"。**留着一句错注释比多一个定时器更贵**（spec §6）。
4. `CONTEXT.md` 的「脉冲」词条：把「**空闲时那台时钟不存在** —— 没有唤醒、没有重画」改成"空闲时的唯一例外是那个有界的退出手势 deadline（500ms，只在举手期间武装）"；其余句（消费者是提示符色相、纯渲染器状态、不进事件流）不动。
5. 500ms 继续写死在 `EXIT_GESTURE_WINDOW`，**不做配置项**（spec「明确不做」）。

## 测试

- `tests/render_tui.rs`：
  - `EXIT_GESTURE_WINDOW` == `Duration::from_millis(500)`（与既有的 `PULSE_FRAME.as_millis() == 60` 那条并列，`src/render/tui.rs:4608` 附近）。
  - 用可注入时间断言"寿命 = 窗口"：`raise_exit_gesture_at(t0)` 后 `TuiState::exit_deadline()` 是 `Some(t0 + 500ms)`；`exit_gesture_raised(t0 + 499ms)` 为真、`t0 + 501ms` 为假。
  - `expire_exit_gesture()` 后 deadline 是 `None`、`dirty` 被置起、`!should_quit()`（提示行恢复原样）。
- 主循环的接线本身在 `cargo test` 里跑不了终端（要真 pty）：把"该不该到点"抽成纯判定（例如 `fn exit_gesture_due(deadline: Option<Instant>, now: Instant) -> bool`）加一条单元断言，剩下的交给 [票 05](05-acceptance.md) 的 pty 回归与手工清单；**不要**为了测它把主循环拆出假终端。
- `cargo test` 全绿。

## 不做什么

- 不动 `PULSE_FRAME` 的 60ms 与它分支上的 `if state.busy()` 守卫（spec §7）。
- 不改举手状态机、三句文案、`Pending::Exit` 的拆除（票 01 的活）。
- 不给窗口加配置项，不加别的常驻定时器，不把 deadline 做成 `interval`。
- 不改 `CONTEXT.md` 的「退出举手」词条（它已经写好了）。

## 评论

- **落地**：`TuiState::exit_deadline()` 只读访问；非重放那支 `select!` 与 `pulse` 并列加一支 `_ = tokio::time::sleep_until(deadline), if deadline.is_some() => state.expire_exit_gesture()`，`deadline` 在 `select!` 之前从 `state.exit_deadline()` 取成值（`Instant` 是 `Copy`），未举手时给它 `Instant::now()` 占位。每轮重新建 future，所以举手之后下一轮就生效，不需要重置逻辑。
- **注释与词条**：`src/render/tui.rs` 那段「唯一的定时器」改写成「两个定时器，都按需武装」（`pulse` 60 ms / 退出 deadline 500 ms），`select!` 上方那段也改成「三个来源 + 两个按需武装的定时器」；`CONTEXT.md` 的「脉冲」词条把「空闲时那台时钟不存在」改成「空闲时那台时钟不存在 ……（唯一例外是那个有界的退出手势 deadline）」。
- **纯判定**：`fn exit_gesture_due(deadline, now) -> bool` 抽出来了，`exit_gesture_raised` 委托给它（`有 deadline && !到点`），于是它不是死代码；`src/render/tui.rs` 的 `mod tests` 里加了一条单元断言（`None` 永不到点、未到点、正好到点、已过）。
- **测试**：`tests/render_tui.rs` 的窗口那条补了 `exit_deadline()` 的断言（`Some(t0 + 500ms)`、作废后 `None`、`expire` 置 `dirty`）。
- **实跑**：`python3 scripts/tui-startup-check.py`（本机用 `FS_AGENT_MODEL=kimi-for-coding`）12/12 GREEN —— 双击手势、举手超时之后提示行恢复、退出交还终端这几条都在真 pty 上过了一遍；主循环这根接线本身没有别的自动化面（票里说的）。
- `cargo test` 全绿（965 条）；`cargo clippy --all-targets` 无警告。
