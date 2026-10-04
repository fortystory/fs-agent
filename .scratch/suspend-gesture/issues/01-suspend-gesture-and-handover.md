# 挂起手势与终端交接：Ctrl-Z 停到后台，`fg` 回来重绘

Type: implement
Status: done
Blocked by: —

> 规格：[`.scratch/suspend-gesture/spec.md`](../spec.md) §1–§5（语义与范围、手势、交还与停止、恢复、SIGTSTP 处置）。
> 本票动渲染器与 pty 脚本：手势、终端交还与恢复、信号处置。文档与验收面归 [票 02](02-docs-and-acceptance.md)；`CONTEXT.md` 的**挂起**词条已在 spec 落盘那一刻写好，**本票不动它**。

## 目标

- TUI 里按一下 `Ctrl-Z`：交还终端 → 进程被 SIGTSTP 停住 → 在 shell 里干完活 `fg` 回来 → 重新进 alt screen、清屏、全量重绘。
- 空闲、忙碌、重放、详情覆盖层立着、问卷立着 —— 任何视图都拦不住它；**单下生效**，不做举手。
- 不碰 plain 的代码；plain 的天然行为在 pty 脚本里加一条回归钉住。
- 挂起不进事件流、不打回执、不写状态文件。

## 现状（2026-10-02 量的，改前先复核）

- `map_key`（`src/render/tui.rs:115-153`）的控制键分支只认 `c/d/a/e/u/k/w/p/n/g/j`，`z` 落到 `:131` 的 `_ => None`；调用点在 `terminal_event`（`:2258-2275`）的 `:2263-2265`。
- 终端进出链：`ratatui::init()`（`:333`，raw 模式 + alt screen + panic hook）→ `TerminalModes::enter(&first)`（`:337`，定义 `:470-490`）→ 收尾 `drop(modes)`（`:452`）→ `ratatui::restore()`（`:453`）；`disable_terminal_modes()` 在 `:498-506`，`TerminalModes::enter` 里那段包 panic hook 在 `:483-487`。
- 主循环：`tokio::select!`（`:365-402`，重放与非重放两个分支），绘制段 `:428-449`，`should_quit()` 在 `:447`。
- 终端标题：`state.sync_title()`（`:443-445` 消费）、`set_terminal_title`（`:461-463`）、`TerminalModes::enter` 里的 `CSI 22 t`（`:478`）与 `disable_terminal_modes` 里的 `CSI 23 t`（`:504`）。
- `bash` / 动态工具的子进程自带进程组（`src/tools/process.rs:125-127`），超时 `:152`、`killpg` `:251-256` —— **本票不动**。
- pty 脚本：`GESTURES`（`scripts/tui-startup-check.py:100`）、`tty_modes`（`:110`）、`read_once`（`:221`）、`capture`（`:265`）、`capture_busy`（`:376`）、`verdict`（`:452`）、`terminal_handed_back`（`:516`）。
- 全仓没有任何 SIGTSTP / SIGCONT / `raise` 代码；`libc::` 的先例在 `src/tools/process.rs:256`（`unsafe { libc::killpg(...) }`）与 `src/cli.rs:79`（`geteuid`）。`libc = "0.2"` 已是直接依赖。

## 落点

- `src/render/tui.rs`：`Key` 枚举、`map_key`、`TuiState`（挂起请求标志）、`Tui::run`（消费请求、执行交还/停/恢复）、`TerminalModes` 的拆分。
- `scripts/tui-startup-check.py`：TUI 与 plain 两条挂起路径。
- `tests/render_tui.rs`：手势与请求标志的单测。

## 具体行为

### 1. 手势（spec §2）

- `Key` 加 `CtrlZ`；`map_key` 的控制键分支加 `'z' => Some(Key::CtrlZ)`。
- `TuiState` 加挂起请求标志（形状实现定，例如 `suspend: bool` + `take_suspend_request() -> bool`）。
- `key(Key::CtrlZ)` 一律置位并 `mark_dirty()`（需要的话），**在任何视图守卫之前**：详情覆盖层立着、问卷立着、重放进行中、举手正举着，都照常挂起。注意别被「其它按键清旧举手」那条规则吃掉 —— Ctrl-Z 不是「其它按键」，它照常挂起（举手在恢复后由 deadline 自然作废）。
- **不做举手、不加 `/suspend`、不推任何 `FrontEndEvent`**。

### 2. `Tui::run` 消费请求并挂起（spec §3）

主循环里（绘制段附近、`should_quit` 判定之前或之后，实现定）加一段：请求置位就执行挂起，顺序固定：

1. `disable_terminal_modes()`（关鼠标、关括号粘贴、标题 `CSI 23 t` pop）。
2. `ratatui::restore()`（`disable_raw_mode` + `LeaveAlternateScreen`）。
3. 记下 SIGTSTP 处置、置 `SIG_DFL`。
4. `unsafe { libc::raise(libc::SIGTSTP) }` —— **同步**，这一行返回就是 `fg` 回来了。**不要用 `kill(0, SIGTSTP)`**：它异步，实测里返回之后当前线程又往前跑了半条恢复路径信号才被处理（spec §3 有完整理由）。
5. 这一行阻塞到 `fg`。

挂起前不做最终绘制、不打回执、不写 stderr。`kill` 失败（`-1`）不 panic，继续走恢复。

### 3. 恢复（spec §4）

4 返回后（已收到 SIGCONT），反向做一遍：

1. 还原 SIGTSTP 处置。
2. `enable_raw_mode()` + `EnterAlternateScreen`（crossterm 原语，**不调 `ratatui::init()`**）。
3. `CSI 22 t` push 标题，再写 `state.sync_title()` 的期望值。
4. 重开鼠标上报与括号粘贴。
5. `terminal.clear()` + `mark_dirty()`。

### 4. `TerminalModes` 拆成可反复进出的形状（spec §4）

- 现状 `enter()` 一次做四件事，其中装 panic hook 那段会 `take_hook` 再包一层 —— 反复调会把 hook 叠起来。
- 改法：hook 只在启动时装一次；`disable_terminal_modes()` 与一个新的「重进」函数（不含 hook）供挂起与恢复反复调用。`enter()` / `Drop` 的对外行为不变。

### 5. 处置的读写（spec §5）

- 挂起前读当前处置、置 `SIG_DFL`；恢复后还原成读到的那个（`SIG_IGN` 也要还原 —— 父进程若有意忽略它，我们借一次就还）。
- 发信号用 `libc::raise`，**不用** `libc::kill(0, …)`：后者异步，顺序会烂（spec §3）。
- 用 `libc::signal` 或 `libc::sigaction`；注意 `unsafe`，并像 `process.rs:254` 那样写清安全性理由。

### 6. pty 脚本（spec 测试决定）

- 加一个挂起路径函数（参考 `capture_busy` 的形状），**并为它准备一个真正的会话**：
  - **`pty.fork()` 用不了**。它 `setsid` 出来的子进程是「父不在同一会话」的会话首进程，正好落进**孤儿进程组** —— 内核在那里直接把 SIGTSTP 丢掉（POSIX 如此规定）。实测：`Ctrl-Z` 之后进程一直停在 `S`，像按了个空键。
  - 改成分两层：会话头 `setsid` 并 `TIOCSCTTY` 拿走控制终端，再 fork 出 fs-agent、`setpgid(0, 0)` 后 `tcsetpgrp` 设成前台进程组。**前台组是 plain 那条路的前提** —— 没有它终端驱动无处投递那个字节。会话头等孙进程结束，用自己的退出码把 fs-agent 的退出状态带回来（脚本 `waitpid` 不到孙进程，运行状态从 `/proc/<pid>/stat` 读）。
  - **TUI**：发 `\x1a`，断言状态字母变成 `T`，并断言**此刻终端已经交还**（`tty_modes` 回到 canonical/echo/ISIG，且 `TEARDOWN` 全部落在停止之前的输出里）；`SIGCONT` 恢复，断言 `CSI ?1049h` 重进、`CSI 2J` 清屏、标题被重新保存，再走正常出口并复查 TEARDOWN。
  - **plain**：同样发 `\x1a`，断言进 stopped、`SIGCONT` 后干净退出。
  - **不要照搬 `verdict` 的「横幅恰好一次」**：恢复时的全量重绘会把 `wording::banner` 再画一次，断言改成「停下之前恰好一次」+「恢复后清屏重绘」。
- 脚本的最终判定（`verdict` 那一段）要把新路径算进去，并在文件顶部的说明里补一句它断言什么。

## 测试

- `tests/render_tui.rs`：
  - `map_key` 把 `Ctrl+Z` 映射成 `Key::CtrlZ`（放进既有的映射断言里，`:4974-4990` 那片）。
  - 空闲、忙碌、重放三态各按一下 `CtrlZ`，`take_suspend_request()` 都为真，且 `!should_quit()`、`take_events()` 为空。
  - 详情覆盖层立着、问卷立着时同样置位。
  - 取走请求后按别的键不会重新置位。
- `scripts/tui-startup-check.py`：TUI 与 plain 两条挂起路径全绿；原有全部路径不被打破。
- `cargo test` 全绿；`cargo clippy --all-targets` 干净；`cargo fmt`。
- 实现完成后本机实跑一次 `python3 scripts/tui-startup-check.py`，把结果记进 Comments。

## 不做什么

- 不动 plain 的代码（`src/render/input.rs` / `src/render/plain.rs` 一行不碰）：它今天就是对的，本票只加回归。
- 不动 `src/tools/process.rs` 的进程组与超时（spec §6）。
- 不冻结任何 deadline、不给 provider 层加东西。
- 不加 `/suspend`、不做双击/举手、不给 headless 加手势。
- 不碰 `docs/`、`CONTEXT.md`、`.scratch/README.md`（归 [票 02](02-docs-and-acceptance.md)）。
- 不动 exit-gesture 的举手语义与 `Ctrl-C` / `Ctrl-D` 键位。

## 评论

- **落地**：`Key::CtrlZ`（`map_key` 的 CONTROL 分支加 `'z'`）；`TuiState` 加 `suspend` 与 `take_suspend_request()`；`key()` 在**一切守卫之前**接走 `CtrlZ`（重放、详情覆盖层、问卷、举手都拦不住它 —— 只置位，不推任何 `FrontEndEvent`）；`Tui::run` 在 `take_events()` 之后、绘制段之前消费请求并调 `suspend_and_resume`。
- **终端进出**：`TerminalModes::enter` 拆出可反复调用的 `enable_terminal_modes(title)`（push 标题、写标题、开鼠标与粘贴），panic hook 因此只在启动时装一次。`suspend_and_resume` 的顺序是 `disable_terminal_modes` → `ratatui::restore` → 处置 `SIG_DFL` → `raise` → 还原处置 → `enable_raw_mode` + `EnterAlternateScreen` → 强制重写标题 → `terminal.clear()` + `mark_dirty`。新增 `TuiState::retitle()`：交还时标题被 pop 回了用户那条，`last_title` 的记忆已经不作数，所以恢复必须强制重写一次。
- **两处与票面不同，都是实测逼出来的**（票面与 spec 已同步改写）：
  1. **`kill(0, SIGTSTP)` 换成 `raise(SIGTSTP)`。** `kill` 是异步的：探针显示它返回之后当前线程又跑完了恢复路径开头的 `enable_raw_mode` + `EnterAlternateScreen`（pty 里收到了 `CSI ?1049h`），信号才被处理 —— 于是「先交还、再停」在时间上并不成立（停止那一刻 termios 还是 raw）。`raise` 同步，这一行返回就是 `fg` 回来了。
  2. **pty 的挂起路径不能用 `pty.fork()`。** 它的 `setsid` 让子进程成了「父不在同一会话」的会话首进程，正好落进**孤儿进程组** —— 内核在那里把 SIGTSTP 直接丢掉（POSIX，免得停住的作业没人 `fg` 回来）：实测 `Ctrl-Z` 之后进程一直停在 `S`，像按了个空键。改成两层：会话头 `setsid` + `TIOCSCTTY` 拿走控制终端，再 fork 出 fs-agent、`setpgid(0, 0)` 后 `tcsetpgrp` 设成前台进程组；会话头等孙进程结束、用自己的退出码把它的退出状态带回来（脚本 `waitpid` 不到那个进程，运行状态从 `/proc` 读）。**前台组是 plain 那条路的前提**：没有它终端驱动无处投递那个字节，`--plain` 也永远停在 `S`。
- **测试**：`tests/render_tui.rs` 加两条（空闲/忙碌/问卷三处都置位且只被取走一次、不退出也不推手势；`retitle` 强制重写且写完之后比对重新成立）；`tests/history_replay.rs` 加一条（重放中也能挂起、不打断重放）；`tests/render_layout.rs` 的 `the_detail_overlay_ignores_every_key_but_its_own` 补上「`Ctrl-Z` 是那个『别的全部忽略』的唯一例外」；`src/render/tui.rs` 的 `map_key` 单测补 `Ctrl+Z`。
- **pty 脚本**：新增 `fork_with_controlling_tty` / `proc_state` / `capture_suspend` / `verdict_suspend` / `SuspendRun`，`main` 里加 TUI 与 `--plain` 两轮。`verdict_suspend` 断言的形状是：**交还序列全部落在停止之前**、停止那一刻 termios 已是 canonical/echo/ISIG、`CSI ?1049h` 与 `CSI 2J` 落在恢复之后、标题重新保存、出口干净。**没有照搬 `verdict` 的「横幅恰好一次」**：恢复的全量重绘会把 `wording::banner` 那一行再画一次，所以改成「停下之前恰好一次」+「恢复后清屏重绘」。
- **验收**：`cargo test` **976 passed / 0 failed / 1 ignored**；`cargo clippy --all-targets` 干净；`cargo fmt`（跑完还原了它顺手改的 `tests/ask_user_question.rs` —— 那处与本票无关）；`python3 scripts/tui-startup-check.py target/debug/fs-agent 1` **7/7 GREEN**（三条出口 + `--continue` + 忙碌双击 + 挂起 TUI + 挂起 plain）。
