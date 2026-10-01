# 忙碌双击退出：有序收尾后以 130 退出

Type: implement
Status: ready-for-agent
Blocked by: 01

> 规格：`.scratch/exit-gesture/spec.md` §3（忙碌态的退出与退出码）、§5（回执的时机前提）、§7。
> 现在忙碌的第二次 `Ctrl-C` 走 `std::process::exit(130)`，它跳过一切析构，终端留在 raw + alternate screen（spec 问题陈述第 4 条）。本票把三处收成有序收尾，并让已经存在却没有任何生产点的 `FrontEndEvent::Quit` 真正接上 CLI 的消费端。渲染器侧的生产点由 [票 01](01-gesture-and-hints.md) 建立；130 之后那行回执是 [票 04](04-exit-receipt.md)。

## 目标

- 忙碌第二下 = **有序收尾** + `ExitCode::from(130)`；空闲双击与 `/quit` 仍然 = 0。
- 三处 `std::process::exit(130)` 全删。
- `TerminalModes::drop`（`src/render/tui.rs:438-442`）与 `ratatui::restore()`（`src/render/tui.rs:413`）在 0 与 130 两条退出路径上都真的执行。

## 现状（2026-10-01 量的，改前先复核）

- 渲染器：忙碌第二下推 `FrontEndEvent::Quit` —— 变体声明在 `src/render/input.rs:119-120`，此前**没有任何生产点**；票 01 把它接上。
- CLI 侧目前只有消费，没有"退出"语义：
  - `run_one_turn`（`src/cli.rs:1889-1919`）：`Cancel` 且 `signal.is_cancelled()` 时 `std::process::exit(130)`（`:1908`）；`Quit | None` 走 `signal.cancel()`（`:1913`）。
  - `interactive_loop` 的提示等待（`src/cli.rs:1103-1106`）：`Quit | None` 直接 `return ExitCode::SUCCESS`。
  - `run_discussion`（`:1003` 起，`exit` 在 `:1017`）与 `discuss_in_session`（`:899` 起，`exit` 在 `:977`）各自也有一处 `process::exit(130)`。
- 收尾通路：`interactive()`（`src/cli.rs:201`）在 `harness.shutdown().await` 之后返回 `code`（`:420-435`）；渲染器 task 的 `drop(modes)` / `ratatui::restore()` 在 `src/render/tui.rs:405-414`。
- `docs/**` 里对 130 的现成描述只有 `docs/research/` 的引文（一手引文，一个字不改）。

## 落点

- `src/cli.rs`：`run_one_turn`、`interactive_loop`、`run_discussion`、`discuss_in_session`，以及它们到 `interactive()` 返回值的通路。
- 测试：`src/cli.rs` 的 `mod tests`（`:3079`）和/或 `tests/cancellation.rs` 的组装接缝。

## 具体行为

1. **`Quit` 的消费**：`run_one_turn` 收到 `Quit` 时**记下退出请求**（形状实现定：返回值带一个标志、调用方传 `&mut bool`、或换一个枚举），并像现状 `:1913` 那样 `signal.cancel()` —— 让回合拿到它的收尾事件。回合 future 落地后由 `interactive_loop` 返回 `ExitCode::from(130)`，`interactive()` 一路把它带出去，于是 `harness.shutdown()`、渲染器 task 收尾、`drop(modes)` 与 `ratatui::restore()` 都会跑。
2. **三处 `std::process::exit(130)` 全删**：
   - `:1908`（`run_one_turn`）：按上一条处理。
   - `:1017`（`run_discussion`，独立 `fs-agent discuss` 子命令）：改成 `signal.cancel()` + 记请求，等 run future 落地后由这条命令自己的收尾返回 `ExitCode::from(130)`；它的调用者在 `discuss`（`:500` 起）里，那里同样要在 `harness.shutdown()` 之后返回这个码。
   - `:977`（`discuss_in_session`，跑在交互式会话里）：同样处理，并把退出请求**冒泡回 `interactive_loop`**。它现在返回 `Result`，具体形状（返回值带上它、或调用点查一个共享值）实现定，但 130 必须从 `interactive()` 出去 —— 否则渲染器收尾仍然不会跑，这条路就白修了。
3. **退出码语义写死**：人主动退（空闲双击、`/quit`）是 0；忙碌中被打断而退是 130（与 `SIGINT` 的 128+2 惯例一致）。空闲双击**不许**"顺便"变成 130。`Esc` 取消之后继续跑不受影响。
4. **不改别的退出路径**：`None`（stdin 关）维持现状语义；`Cancel` 第一次的取消手势不变；只有"举手状态下的第二下"才产生 `Quit`（票 01）。
5. 复核并用一处小函数把判定钉住（也给测试一个落点），例如 `fn exit_code_after(quit: bool) -> ExitCode`，让 0 / 130 两档各只有一处定义。
6. `docs/**` 里若有对这条退出码的既有描述就同步；`docs/research/` 不动。

## 测试

- **130 那条路径现在没有测试**（spec「补充说明」），本票要补上：
  - 单元断言：`exit_code_after(false) == ExitCode::SUCCESS`、`exit_code_after(true) == ExitCode::from(130)`（放 `src/cli.rs` 的 `mod tests`）。
  - 端到端断言（`tests/cancellation.rs` 的 `FakeProvider` 组装接缝，或同形状的新测试文件）：一个正在跑的回合收到 `Quit` 后**不立即结束**，而是按取消收尾 —— 断言回合落下 `TurnEnded { Aborted }` 之类的取消形状，并且退出请求最终得到 `ExitCode::from(130)`；同一夹具里不发 `Quit` 的那次仍然是 0。
- 静态断言：`grep -rn "process::exit(130)" src/` 为空；三处旧落点都不在了。
- 真终端回归在 [票 05](05-acceptance.md)：忙碌双击后退出码 130、termios 交还、`TEARDOWN` 序列齐全。
- `cargo test` 全绿。

## 不做什么

- 不改空闲退出码（仍 0）、不改 `Cancel` 的语义、不动 `Esc` / `/quit` / 模式循环 / 详情覆盖层键位（spec §7）。
- 不碰举手状态机与提示文案（票 01）；不碰回执打印（票 04）。
- 不加 `exit` / `abort` 一类新命令，不给 `--plain` 加手势。
- 不为了让 130 走出去而保留任一处 `std::process::exit`。
