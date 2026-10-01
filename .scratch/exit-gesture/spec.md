# 退出手势：双击退出、提示行上的回执、退出时的会话回执

种子的三句话里有两句已经不成立，先把它们纠正掉（这也是这份 spec 的起点）：

- **「空闲态 `Ctrl-C` 还要再走一次确认」不对了。** 现在空闲态 `Ctrl-C` 直接退（[`src/render/tui.rs:2172-2179`](../../src/render/tui.rs)），只有 `Ctrl-D` 才弹那个「退出会话」覆盖层（[:2183-2188](../../src/render/tui.rs)）。两个键一个太急、一个太慢。
- **「退出时打 session id 好让 `--continue` 直接用」理由不成立。** `-c` 是「继续**本工作区最新**的会话」（[`src/render/wording.rs:2177`](../../src/render/wording.rs)），它不需要 id。id 真正的用处是**跨工作区精确指认**：`fs-agent sessions show <id>`。所以这次做的是"退出时给一个能直接粘的复盘命令"，不是"让 `-c` 能用"。

同时核出一条**既有缺陷**：忙碌时第二下 `Ctrl-C` 走的是 `std::process::exit(130)`（[`src/cli.rs:1908`](../../src/cli.rs)），而 `std::process::exit` **不运行任何析构** —— `TerminalModes::drop`（[:438-442](../../src/render/tui.rs)）与 `ratatui::restore()`（[:413](../../src/render/tui.rs)）都不会执行。也就是说**那条退出路径会把终端留在 raw mode + alternate screen**。这次动这条链路，一并修掉。

来源：2026-10-01 一次 `/ask-matt` 里记下的种子（[`seed.md`](seed.md)），同日经 `/grill-with-docs` 折成这份 spec（十一路决定：手势、窗口、提示行、重放、忙碌 `Ctrl-D`、退出码、确认框、时钟、回执、落点、详情覆盖层）。

## 问题陈述

1. **两个键不对称。** 空闲 `Ctrl-C` 一下就走，`Ctrl-D` 要过一层弹窗 —— 同一个意图，两套成本。
2. **忙碌态的手势没有可见的中间态。** 第一下 `Ctrl-C` 已经取消了回合，但屏幕上什么都不说；人不知道"刚才那下生效了没有"。
3. **退出后什么也不留。** `discuss` 结束时会打「会话 {id}；复盘：…」（[`src/cli.rs:693-696`](../../src/cli.rs)），交互式会话一条都不打。
4. **忙碌双击退出会把终端留在 raw + alt screen**（见上）。pty 脚本只跑三条**空闲**出口（[`scripts/tui-startup-check.py:88`](../../scripts/tui-startup-check.py)），所以这条路径一直没有被验过。
5. **那个确认覆盖层是渲染器侧唯一结束应用的问题**，为它维护着一整套文案、按钮、命中区与测试，而它挡的只是一个可以做成"第二下"的动作。

## 方案

- **手势**（§1）：空闲态 `Ctrl-C` 与 `Ctrl-D` **对等**，都是"第一下举手、500 毫秒内第二下退出"；两键共用同一把举手。
- **提示**（§2）：举手期间在**提示行**（不是弹窗）显示回执；超时恢复原样。
- **忙碌**（§3）：第一下仍是取消回合，但提示行说明；第二下退出，走**有序收尾**并以 130 退出。忙碌 `Ctrl-D` 维持忽略。
- **删除**（§4）：`Pending::Exit` 覆盖层连同它的文案、按钮、命中区一起拿掉；不做"有草稿就弹一次"的特判。
- **回执**（§5）：交互式退出（含 130 那条）在终端交还之后往 **stderr** 打一行能直接粘的复盘命令。
- **时钟**（§6）：主循环加**一个按需武装的 deadline 分支**，只在那 500 毫秒里存在。

## 用户故事

1. 作为手快的人，我希望空闲时按一下 `Ctrl-C` 不会立刻把我踢出去，这样我能收回那一下。
2. 作为想退出的人，我希望第二下就真的出去，不用再找 `y` 或 Enter。
3. 作为按了第一下的人，我希望提示行告诉我还差一下，而不是让我猜。
4. 作为刚取消了一个长回合的人，我希望屏幕说"已取消"，这样我知道那一下生效了。
5. 作为同时管多个工作区的人，我希望退出后有一行能直接粘的命令，把那场会话翻出来。
6. 作为用脚本/pty 驱动 fs-agent 的人，我希望两条退出路径都把终端交还干净，并且退出码是可预期的。
7. 作为看 `--continue` 重放的人，我希望重放中误按 `Ctrl-C` 不会直接把我踹出去。

## 实现决定

### §1 手势与窗口

- **空闲态**：`Ctrl-C` 与 `Ctrl-D` 完全对等。第一下**举手**（记下起手时刻，不退出），窗口内任意一个键的第二下都退出（**两键共用同一把举手**，与提示文案 `ctrl-c/ctrl-d` 一致）。
- **窗口 = 提示的寿命 = 500 毫秒**（§6）。超时后手势作废、提示行恢复；下次要重新按两下。
- **任何其它按键**在新手势按下前先清掉旧举手，再照常处理（避免"半分钟前那一下"莫名其妙地算数）。
- **忙碌态**：第一下 `Ctrl-C` = 取消当前回合（推 `FrontEndEvent::Cancel`，现状不动）**并且**举手；第二下退出（§3）。忙碌态 `Ctrl-D` **维持忽略**（[`src/render/tui.rs:2183-2188`](../../src/render/tui.rs)）——忙碌时误退的代价更大，而「取消」已经有一个入口了。
- **重放态**：`Ctrl-C` 也走双击（第一下提示让位，见 §2），`Ctrl-D` 与 `Esc` 在重放里维持"无效"（[`src/render/tui.rs:1562-1567`](../../src/render/tui.rs)）。
- **详情覆盖层**维持现状：`Ctrl-D` 是关覆盖层、`Ctrl-C` 被忽略（[:2158-2170](../../src/render/tui.rs)）。那是另一个视图自己的键位，不参与退出。
- `/quit` 维持一下退出（[`src/cli.rs:1122`](../../src/cli.rs)）——它本来就是明确说出口的意图。

### §2 提示行上的三句文案

提示行的**出口那一段是预留宽度**（[`src/render/wording.rs:1049-1050`](../../src/render/wording.rs) 明说"它是预留的、不是追加的"），所以举手文案替换 `EXIT_HINT_IDLE` / `EXIT_HINT_BUSY` 那一段，**不挤动** `KEY_HINTS` 里别的条目：

| 时刻 | 提示行出口段 |
| --- | --- |
| 空闲，已举手 | `再按一次 ctrl-c/ctrl-d 退出` |
| 忙碌，已举手 | `已取消 · 再按一次 ctrl-c 退出` |
| 重放，已举手 | `再按一次 ctrl-c 退出`（**让位**给提示，重放进度行暂时收起） |
| 未举手（现状） | 空闲 `ctrl-c/ctrl-d 退出`；忙碌 `ctrl-c 退出` |

- 文案进 [`src/render/wording.rs`](../../src/render/wording.rs)。
- 忙碌那句把两件事都说出来：回合确实停了、再按会退出。这正是它比"只说退出"值钱的地方。
- 举手期间**替换出口段那一项**（`hint_line` 的阶梯不动，`KEY_HINTS` 照旧逐条试加）；**重放中**照进度行的先例**整条替换**（进度行让位）。两种形态都不追加条目，所以一段文案再长也不会把 `KEY_HINTS` 挤掉，这与"出口永不丢、状态词最先丢"的现有规则同向。
- **草稿不做特判**：输入区有未发送草稿时也照样双击即退，提示文案不变。这是明确接受的取舍（见"明确不做"）。

### §3 忙碌态的退出与退出码

- **空闲双击退出 → 0**（现状的 `quit` → `ExitCode::SUCCESS` 路径）。
- **忙碌第二下 → 130**，但**改走有序收尾**：不再 `std::process::exit(130)`。
- 实现形状（三处 `std::process::exit(130)`——[:977](../../src/cli.rs)、[:1017](../../src/cli.rs)、[:1908](../../src/cli.rs)——一起收）：
  - 渲染器在忙碌第二下推 **`FrontEndEvent::Quit`**。这个变体现在就存在、**却没有任何生产点**（[`src/render/input.rs:119-120`](../../src/render/input.rs) 只有声明，`cli.rs` 四处只有消费）——这次正好把它用起来。
  - CLI 收到 `Quit` 后**记下退出请求**并按取消处理（让回合拿到它的收尾，现状 [:1913](../../src/cli.rs) 就是这么做的），等回合落地后由 `interactive_loop` 返回 `ExitCode::from(130)`。
  - 于是 `Drop(modes)` 与 `ratatui::restore()` 都会执行，终端干净，回执也打得出来。
- 退出码语义写死：**人主动退 = 0；忙碌中被打断而退 = 130**（与 `SIGINT` 的 128+2 惯例一致）。空闲双击不再"顺便"变成 130。
- **为什么不是"全 0"**：那条语义已经存在（忙碌中被两次 `Ctrl-C` 打断 = 128 + `SIGINT`），这次要修的是它的**清理路径**，不是它的含义；改语义不在这一票的射程里。

### §4 删掉 `Pending::Exit`

- 删：枚举变体 `Pending::Exit`、它的 modal 组装（[:901-909](../../src/render/tui.rs)）、所有 match 分支（[:1819](../../src/render/tui.rs)、[:1839](../../src/render/tui.rs)、[:1863](../../src/render/tui.rs)、[:2450](../../src/render/tui.rs)、[:2480](../../src/render/tui.rs)）、`HitAction::Quit`，以及 `wording::exit_title` / `exit_body` / `EXIT_CHOICES`（[:825](../../src/render/wording.rs)、[:831](../../src/render/wording.rs)、[:698](../../src/render/wording.rs)）。
- `agrees` 如果只被 Exit 用则一并删；若 `ClearDraft` 还在用就留着（实现时先查）。
- 连带要改的验收面：
  - [`tests/render_tui.rs`](../../tests/render_tui.rs) 的 `ctrl_d_asks_before_it_quits_and_the_safe_answer_is_no`（:238）删掉/改写成"双击"；**另外三条按旧"单击即退"语义写的也会红**，要一并改：`ctrl_c_quits_before_the_loop_has_asked_for_its_first_line`（:175）、`an_idle_ctrl_c_quits_and_a_working_one_cancels`（:188）、`a_run_that_never_ended_a_turn_still_leaves_ctrl_c_quitting`（:203）；
  - [`tests/history_replay.rs`](../../tests/history_replay.rs) 里「重放中单击 `Ctrl-C` 就退出」那条（:432）改成双击；
  - [`tests/render_layout.rs`](../../tests/render_layout.rs) 的 `the_renderer_confirmations_answer_by_click`（:3934）去掉 Exit 那半；`ctrl_d_closes_the_detail_overlay_rather_than_asking_to_quit`（:4388）改名（"rather than quitting" 的前提没了）；
  - [`docs/tui-manual-checklist.md:109-112`](../../docs/tui-manual-checklist.md) ⑦ 第 1 条整条重写；
  - [`scripts/tui-startup-check.py:88`](../../scripts/tui-startup-check.py) 的**两条**手势都要改：`("ctrl-c", b"\x03")` → `b"\x03\x03"`、`("ctrl-d y", b"\x04y")` → 双击。单击不再退出，只改一条脚本必红。
- **保留**它挡住的那件事的**信息**：退出后打回执行（§5），而且举手期间提示行在说话——不是静默退出。

### §5 退出回执

- 内容照 `discuss` 那句的模板：`会话 {id}；复盘：fs-agent sessions show {id}`。为此把 `wording::discussion_replay`（[:74-76](../../src/render/wording.rs)）**改名/抽成通用**（`session_receipt`），discuss 与交互式退出共用同一个生成器——它的文档注释本来就写着"给 alt screen 恢复之后打的那一行"。
- **通道是 stderr**。理由不是"它是诊断"，而是仓库已立的规矩：stdout 只承载最终产物（[`src/render/plain.rs:337-350`](../../src/render/plain.rs)、[`docs/render.md`](../../docs/render.md)）。走 stderr，`--plain` 与管道下天然安全。
- **时机是终端交还之后**：落点在 `interactive()` 的尾部——`interactive_loop` 返回、`harness.shutdown()` 之后那段（[`src/cli.rs:420-435`](../../src/cli.rs)），与 discuss 刻意在 shutdown 之后才 `eprintln!`（[:685-696](../../src/cli.rs)）同规矩。
- **打几次、给谁**：交互式会话（TUI 或 `--plain`）正常结束时打一行；**启动不打**（横幅 [:404-410](../../src/cli.rs) 已经有 id）；`discuss` / `probe` / `sessions` 一族不打（discuss 有自己那行）；**不**打 `-c` 的提示——它不需要 id，写了反而误导。
- 两条路径都打：正常退出（0）与忙碌双击（130）。

### §6 时钟：一个按需武装的 deadline

- 现状：主循环（[`src/render/tui.rs`](../../src/render/tui.rs) 的 `tokio::select!`）里**只有一个**定时分支 `pulse`（60ms），而且带 `if state.busy()` 守卫；空闲会话一次时钟唤醒都没有（[:356-361](../../src/render/tui.rs) 的注释——那里连「空闲时那台时钟不存在」都写进了领域词条）。
- 这正是这条需求要放宽的约束，**放宽必须是有界的**：
  - `TuiState` 加 `exit_deadline: Option<Instant>`；
  - 主循环加一个分支 `_ = tokio::time::sleep_until(deadline), if state.exit_deadline.is_some()`；
  - 到点调 `state.expire_exit_gesture()`：清字段、置 `dirty`、提示行恢复。
- 只有举手的那 500 毫秒里才有这个唤醒，其余时刻空闲会话照旧零唤醒。**实现时要把 [`src/render/tui.rs:331-336`](../../src/render/tui.rs) 那段"唯一时钟"的注释改写成"两个，第二个有界"**——留着一句错注释比多一个定时器更贵。
- 500 毫秒写成一个常量（与 `PULSE_FRAME` 并列），不做配置项。
- 落地时**同步改 [`CONTEXT.md`](../../CONTEXT.md) 的「脉冲」词条**：它现在写着「空闲时那台时钟不存在」，加上这条之后就有一处有界的例外，那句话必须跟着改，否则下一轮读代码的人会以为多出来的唤醒是个 bug。

### §7 明确不动的部分

- `/quit`、`Esc` 取消、模式循环、目标停确认框：都不动。
- 详情覆盖层的键位（`Ctrl-D` 关覆盖层、`Ctrl-C` 忽略）：不动。
- 忙碌 `Ctrl-D`：不动（维持忽略）。
- `ConsoleEvents` / `FrontEndEvent` 的其余语义：不动，只是把已经存在的 `Quit` 用起来。

## 测试决定

- **状态机**（[`tests/render_tui.rs`](../../tests/render_tui.rs)，时间要可注入）：
  - 空闲第一下**不**退、第二下退；两键混按（`Ctrl-C` 然后 `Ctrl-D`）也算；
  - 超时后第一下的举手作废（直接调 `expire_exit_gesture()`，不等真实时间）；
  - 忙碌第一下推 `Cancel`、第二下推 `Quit`；忙碌 `Ctrl-D` 仍然什么都不做；
  - 重放中第一下不退、第二下退；
  - 其它按键会清掉旧举手。
- **文案**（[`tests/wording.rs`](../../tests/wording.rs)）：三句举手文案与两句恢复文案；删掉 Exit 那两句的断言。
- **帧**（[`tests/render_layout.rs`](../../tests/render_layout.rs)）：举手期间提示行的出口段是那句新文案、且 `KEY_HINTS` 没被挤掉（这是"整段替换"的可测形态）。
- **pty 脚本**（[`scripts/tui-startup-check.py`](../../scripts/tui-startup-check.py)）：三条出口里的 `ctrl-d y` 改成双击；加一条**忙碌态双击**路径（发一个 prompt，立刻两次 `Ctrl-C`，断言进程结束、退出码 130、termios 交还、TEARDOWN 序列齐全）——这条正是 §3 那个缺陷的回归测试。
- **手工**（[`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md)）：⑦ 重写为双击语义；新增"忙碌双击退出后终端干净 + 回执打在 shell 里"。
- **回执**：打印点要落在**可测的收尾函数**上，参照 [`tests/render_console.rs`](../../tests/render_console.rs) 的端口接缝（`interactive_loop` 直接 `eprintln!` 的话没人能断言它）。至少一条断言覆盖"正常退出打了回执、内容含 id 与 `sessions show`"。

## 明确不做

- 不做"有未发送草稿就弹一次确认"的特判：**草稿会丢**是明确接受的取舍。手势本身已经有两下，举手的提示行也说了"再按一次退出"。
- 不给窗口加可配时长（不做 `exit_gesture_ms` 一类配置）。
- 不把空闲双击也改成 130。
- 不删 `/quit`、不改 `Esc`、不改详情覆盖层键位。
- 不在 `--plain` 里加手势（它是行读取，没有举手这回事）。

## 补充说明

- **`SessionFacts.session_id` 是个死字段**：它被注入渲染器（[`src/render/tui.rs:258`](../../src/render/tui.rs)、[`src/cli.rs:329`](../../src/cli.rs)）却从未被读。这次的回执在 CLI 侧用 `harness.session_id()`，**不需要**碰它；是否顺手删掉留给别的收尾，不塞进这条需求。
- 空手站在 [`tests/`](../../tests) 里的两条缺口，这次一并补上：`exit(130)` 那条路径**没有测试**，`--continue` 的旗标解析也**没有测试**（[`src/cli.rs`](../../src/cli.rs) 的 `mod tests` 只测 `--mode`）。
