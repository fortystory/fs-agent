# 验收面收口：忙碌双击的 pty 回归与手工清单重写

Type: implement
Status: ready-for-agent
Blocked by: 03, 04

> 规格：`.scratch/exit-gesture/spec.md`「测试决定」的 pty 与手工两条、§3 那个缺陷的回归、§5 的回执。
> [票 01](01-gesture-and-hints.md) 改了三条空闲出口的手势，[票 03](03-ordered-exit-130.md) 修了忙碌第二下的终端清理，[票 04](04-exit-receipt.md) 加了回执 —— 这张票把它们在**真终端**上钉住。本票不该动 Rust 代码。

## 目标

- `scripts/tui-startup-check.py` 新增一条**忙碌态双击**路径：发 prompt、两次 `Ctrl-C`、断言退出码 **130**、termios 交还、`TEARDOWN` 序列齐全。
- `docs/tui-manual-checklist.md` ⑦ 重写为双击语义，并新增"忙碌双击退出后终端干净 + 回执打在 shell 里"。

## 现状（2026-10-01 量的，改前先复核）

- 脚本只跑三条**空闲**出口：`GESTURES`（`scripts/tui-startup-check.py:88`）里的 `ctrl-c`、`/quit`、`ctrl-d y`；这次它们都已是双击/正常路径（`ctrl-c` 与 `ctrl-d` 两条由票 01 改成连发两下）。
- `capture()`（`:249` 起）只发一个手势、只等一次退出；`verdict()`（`:321` 起）断言"进程结束、状态 **0**、termios 交还（`canonical` / `echo` / `signals`）、`TEARDOWN` 序列一个不缺、状态行干净"。
- 忙碌态从来没有被这个脚本验过（spec 问题陈述第 4 条）；脚本也从不发 prompt。
- `docs/tui-manual-checklist.md:109-112` ⑦ 第 1 条还写着旧的 `Ctrl-D` 确认框语义（`退出会话`、`[y] 退出` / `[n] 取消`）；⑦ 里没有"忙碌双击退出后终端干净 + 回执打在 shell 里"这一项。`:113-130` 的 termios / 鼠标 / 备用屏幕 / panic 说明 / `--continue` 重开各条保持。

## 落点

- `scripts/tui-startup-check.py`
- `docs/tui-manual-checklist.md` ⑦

## 具体行为

1. **忙碌双击路径**（spec「测试决定」）：
   - 新增一条独立的检查（不必塞进 `GESTURES` 那张表 —— 那张表的契约是"一个手势、等一次退出"）：pty 里先发一条 prompt（例如 `say hi\r`），**立刻**在同一个 `write` 里连发两个 `\x03`。
   - 断言：进程结束、退出状态是 **130**（不是 0）；`termios` 交还（`canonical` / `echo` / `signals` 三个都在）；`TEARDOWN` 序列一个不缺（备用屏幕、四种鼠标上报、括号粘贴）。
   - 这条正是 §3 那个缺陷的回归：旧实现里 `std::process::exit(130)` 跳过 `drop(modes)` 与 `ratatui::restore()`，于是这条会红在 TEARDOWN / termios 上，而不是红在退出码上。
   - 判定复用 `verdict()` 的终端部分，但退出码期望从硬编码的 `0` 参数化（或给这条路径单写一个判定）：**空闲那三条仍是 0，忙碌这条是 130**。
2. **手工清单 ⑦ 重写**：
   - 第 1 条整条换成双击语义：空闲 `Ctrl-C` 与 `Ctrl-D` 完全对等 —— 第一下举手（提示行出现 `再按一次 ctrl-c/ctrl-d 退出`），500 毫秒内第二下退出，超时不按则提示行恢复；忙碌 `Ctrl-C` 第一下只取消（提示行 `已取消 · 再按一次 ctrl-c 退出`）、第二下退出；忙碌 `Ctrl-D` **没有任何反应**；详情覆盖层打开时 `Ctrl-D` 是**关掉详情**，不弹任何确认（那个确认框已经删掉）。
   - 新增一条：**忙碌双击退出后终端干净 + 回执打在 shell 里** —— 起一个长回合，连按两下 `Ctrl-C`；退出后 `stty -a | grep -E 'icanon|echo|isig'` 三个都带 `-` 前缀的**反面**（即 `icanon` / `echo` / `isig`），且 shell 里出现一行 `会话 <id>；复盘：fs-agent sessions show <id>`（在 stderr）。
   - ⑦ 其余条目（`:113-130`）保持。
3. `cargo build` 之后跑 `python3 scripts/tui-startup-check.py target/debug/fs-agent`：三条空闲出口加新的忙碌双击路径都绿才算收工。

## 领票后先定的一件事（spec 没说清）

忙碌窗口在这条路径上是**时间竞态**：脚本对着真二进制发一条 prompt，而回合的寿命取决于配置里的 provider。要可靠地落进"一个回合进行中"，先确认这条路径在无网络 / 无凭据的机器上怎么跑。可用的选择：让这条路径用一份指向本地假 provider 的临时配置；或发一条注定很慢的 prompt、发出后立刻连发两次 `\x03`。**如果这个环境里无法稳定进入忙碌态，就把它写成可读的红**（打印实际退出码与屏幕尾部），而不是让它随机变绿 —— 这条路径的价值全在"它会红"，不能靠运气。

## 验收

- `python3 scripts/tui-startup-check.py`（`cargo build` 之后）全绿，输出里能看到新的忙碌双击那一行；空闲三条（改双击后的 `ctrl-c`、`/quit`、`ctrl-d`）仍然绿。
- 手工清单 ⑦ 按新条目逐条走过：忙碌双击那条能复现"130 + 终端干净 + stderr 回执"。
- `cargo test` 全绿（本票不该动 Rust 代码；跑一遍是确认没误伤）。

## 不做什么

- 不改 `capture()` 对既有三条出口的判定（除了退出码参数化）。
- 不把忙碌路径改写成 Rust 测试 —— pty 的事归 pty 脚本（spec「测试决定」的分工）。
- 不动 `docs/tui-manual-checklist.md` ⑦ 以外的节，也不动 `docs/research/`。
