# 挂起手势：Ctrl-Z 停到后台，`fg` 回来原地继续

来源：2026-10-02 一次 `/ask-matt` 里的一句话意向（「让 fs-agent 支持 ctrl-z 挂起到后台」），同日经 `/grill-with-docs` 折成这份 spec —— 九轮决定：语义、范围、忙碌态、手势形态、终端交还、恢复、SIGTSTP 处置、子进程与超时、plain 的钉法。原始意向与九问九答存在 [`seed.md`](seed.md)。

## 问题陈述

1. **TUI 里 Ctrl-Z 是死键。** `ratatui::init()` 让 crossterm 进 raw 模式，`cfmakeraw` 清掉 `ISIG`，于是终端驱动不再把 0x1A 转成 SIGTSTP：它作为普通字节到达应用、被解析成 `Ctrl+Z` 按键，而 `map_key` 的控制键分支只认 `c/d/a/e/u/k/w/p/n/g/j`（[`src/render/tui.rs:115-153`](../../src/render/tui.rs)），`z` 落到 `_ => None` 后按键被丢弃。想离开一下只能 `/quit` 退出再 `--continue` 回来 —— 会话接得回，但那次「我去去就回」的意图与画面位置都丢了。
2. **plain 里它天然有效，但没有任何东西钉住。** plain 前端走 `spawn_blocking` + `std::io::stdin().read_line`（[`src/render/input.rs:369-381`](../../src/render/input.rs)，注释里写着「stdin 是行缓冲的，所以没有 raw 模式也没有按键事件」），终端保持 canonical + ISIG，Ctrl-Z 由终端驱动直接产生 SIGTSTP、进程按默认处置停止、`fg` 继续。这条行为今天只是**碰巧**成立：谁在 plain 里加一次 raw 模式，它就会静默消失。
3. **crossterm / ratatui 没有现成的挂起原语。** 两者全树没有 suspend/resume（ncurses 那套 `def_prog_mode`/`reset_prog_mode` 在这里不存在），可用的只有底层原语 `disable_raw_mode` / `enable_raw_mode` / `LeaveAlternateScreen` / `EnterAlternateScreen`，以及仓库自己那层 `TerminalModes`（[`src/render/tui.rs:470-506`](../../src/render/tui.rs)）。

## 方案

- **语义**（§1）：Ctrl-Z = **真暂停**。进程被 SIGTSTP 停住、shell 拿回提示符、`fg` 回到原地继续；不是「画布让位、回合继续跑」。
- **范围**（§1）：只给 TUI 加显式实现；plain 的天然行为用 pty 回归与文档钉住，一行代码也不改。
- **手势**（§2）：单下 Ctrl-Z 直接挂起，不做举手、不加命令入口，任何视图都拦不住它。
- **交还与停止**（§3）：先按退出那条链把终端交还干净，再把 SIGTSTP 处置置回默认、给**进程组**发信号。
- **恢复**（§4）：`kill` 返回即已 `fg`；反向重进终端、清屏、全量重绘。
- **子进程与超时**（§5）：什么都不动 —— 子进程继续跑，deadline 照走。想连子进程一起停的人走「先 Ctrl-C、再 Ctrl-Z」。

## 用户故事

1. 作为在长回合里等结果的人，我希望按一下 Ctrl-Z 就把整个 fs-agent 停到后台，这样我能去 shell 里干别的，回来还在这块画面上。
2. 作为习惯了 job control 的人，我希望它**一下**就生效，而不是像退出那样要按两下 —— 挂起可逆，不该有中间态。
3. 作为挂起后回来的人，我希望看到的还是同一屏：转录位置、滚动位置、未发送的草稿都在。
4. 作为挂起后回到 shell 的人，我希望终端是干净的：没有 raw 模式、没有鼠标上报、标题是我原来那条。
5. 作为在 shell 里干完活 `fg` 回来的人，我希望窗口尺寸变过也能正常重排，而不是留下残影。
6. 作为正在跑 `bash` 的人，我希望挂起不会把那条命令弄坏（它继续跑，或按它自己的超时被收掉），而不是留下一个半死的进程树。
7. 作为被包装器（IDE、nix、CI）启动的人，我希望按 Ctrl-Z 不会落到「屏幕没了、进程还在跑」那种中间态。
8. 作为用 `--plain` 的人，我希望 Ctrl-Z 照旧能用，而且这件事有测试守着。
9. 作为维护这个仓库的人，我希望「挂起是终端层手势、不进事件流」这条边界写在词表里，别混成会话内容。

## 实现决定

### §1 语义与范围

- **挂起 = 真暂停。** 目标状态是「进程被 SIGTSTP 停止、CPU 归零、`fg` 后原地继续」。**不做**「画布让位但回合继续跑」：在同一个终端里让 shell 可用而回合继续跑，需要 daemon 化加客户端重连，那是 `background-services` 那条种子的地盘。
- **TUI 显式实现；plain 不改代码。** plain 今天的行为由终端驱动与 shell 决定，应用没有插手的位置（除非进 raw 模式，那是倒退）。headless 没有键盘，不涉及。讨论会话（`discuss`）与交互会话共用同一个 `Tui::run`，自动一起覆盖。
- **复用它那唯一的终端进出链**（[`src/render/tui.rs:333`](../../src/render/tui.rs) 的 `ratatui::init()`、`:337` 的 `TerminalModes::enter` → `:452` 的 `drop(modes)`、`:453` 的 `ratatui::restore()`），不新造一套。

### §2 手势

- `map_key` 的控制键分支加 `'z' => Some(Key::CtrlZ)`（[`src/render/tui.rs:117-133`](../../src/render/tui.rs)）；`Key` 枚举加 `CtrlZ`。
- `TuiState` 加一个挂起请求标志（形状实现定，例如 `suspend: bool` 加 `take_suspend_request()`），`key(Key::CtrlZ)` 置位；`Tui::run` 在主循环里消费它并执行 §3 的动作 —— 与 `should_quit()` 是同一种「状态机请求、循环执行」的形状。
- **单下直接挂起，不做举手。** 挂起可逆、零损失，而 exit-gesture 那套举手是为不可逆动作设计的（提示行存在的理由正是「第一下没有可见后果」）。何况 plain 那边由终端驱动直接发信号、必然是单下 —— TUI 对齐它，`fs-agent` 才不会两种手感。
- **只认 Ctrl-Z，不加 `/suspend`。** `/quit` 那类命令入口是「明确说出口的意图」的例外，挂起有键位就够。
- **任何视图都拦不住它。** 详情覆盖层立着、问卷立着、重放进行中、举手正举着 —— Ctrl-Z 一律照常挂起：它是终端层手势，不属于任何一个视图的键位表。恢复后那个 500ms 举手因 `CLOCK_MONOTONIC` 照走而自然作废（主循环的 deadline 分支），不需要专门处理。
- **挂起不是会话事件。** 不进 `EventLog`、不写状态文件、不推 `FrontEndEvent` —— 与 `/undo` 的写回、模式循环同规矩。

### §3 挂起：交还，然后停

顺序固定，一步都不能换：

1. `disable_terminal_modes()`（[`:498-506`](../../src/render/tui.rs)）：关鼠标上报、关括号粘贴、用 `CSI 23 t` 把标题 pop 回用户原来那条。
2. `ratatui::restore()`：`disable_raw_mode()` 加 `LeaveAlternateScreen`（`CSI ?1049l`）。
3. 记下 SIGTSTP 当前处置，把它置为 `SIG_DFL`（§5）。
4. `libc::raise(SIGTSTP)` —— **同步**把信号投给当前线程并等它处理完，这一行返回就是 `fg` 回来了。**不要用 `kill(0, SIGTSTP)`**：它是异步的，实测里调用返回之后当前线程又往前跑了半条恢复路径（`enable_raw_mode` 与 `EnterAlternateScreen` 都发了出去）信号才被处理，于是「先交还、再停」在时间上并不成立。停止信号停的是整个线程组（也就是这个进程），所以「只发给当前线程」不影响「整个进程停住」；glibc 手册那条「发给进程组」针对的是一个作业里有多个进程的情形，而 fs-agent 的组里只有它自己（`bash` 与动态工具刻意各自成组）。
5. 这一行阻塞到进程收到 SIGCONT，也就是用户的 `fg`。

- **不做最终绘制。** 离开 alt screen 就够了，屏幕上的 shell 历史由终端自己恢复。
- **不打任何回执。** shell 自己会打 `[1]+ Stopped fs-agent`，多一句是噪音；挂起也不往 stderr 写东西。
- 终端操作失败（`restore` / `execute!` 报错）按现状一律 `let _ =` 吞掉；`kill` 失败也不 panic —— 挂不成就什么都不做，下一帧照常画。

### §4 恢复：重进，然后重绘

`kill` 返回（即已收到 SIGCONT）之后，反向做一遍：

1. 还原 SIGTSTP 处置（§5）。
2. `enable_raw_mode()` 加 `EnterAlternateScreen` —— **用 crossterm 底层原语，不调 `ratatui::init()`**。
3. `CSI 22 t` 重新 push 标题，再写当前标题（`state.sync_title()` 算出的期望值）。
4. 重新开鼠标上报与括号粘贴。
5. `terminal.clear()` 并置 dirty，让下一帧走正常绘制路径全量重画。

- **`TerminalModes` 要拆成可反复进出的形状。** 现状 `enter()` 一次做四件事（push 标题、写标题、开鼠标/粘贴、装 panic hook），而 hook 那段会 `take_hook` 再包一层（[`:483-487`](../../src/render/tui.rs)）—— 每次挂起/恢复都调它就会把 hook 叠起来。改法：hook 只在启动时装一次，`disable_terminal_modes()` 与一个新的「重进」函数（不含 hook）供挂起与恢复反复调用。
- **恢复后必然重绘。** 挂起期间用户可能在 shell 里改了窗口尺寸，alt screen 的内容也停在被挂起前那一刻；`clear()` 加全量重绘把两件事一起解决，`Resize` 事件照旧走 `mark_dirty()`。
- **恢复时不做任何提示。** `fg` 这个动作本身就是用户说的，画面回来就是回执。

### §5 SIGTSTP 的处置

- **发信号前把处置置为 `SIG_DFL`，恢复后还原。** `SIGTSTP` 可以被忽略（`SIGSTOP` 不行），而被忽略的处置会跨 `execve` 继承；shell 惯例会替子进程重置回 `SIG_DFL`，但包装器（IDE 终端、nix、某些 CI）可能把它留成 `SIG_IGN` —— 那时信号被丢弃、进程不停，而**终端已经被我们交还了**，于是落到「屏幕没了、进程还在跑」的中间态。三行 `sigaction`/`signal` 换掉这个状态很划算；shell 自己就是这么对待子进程的。
- **还原**回去：父进程若有意忽略它，那是父进程的意图，我们借一次就还。
- 「孤儿进程组里未处理的 SIGTSTP 也不停止」不在射程里：那种进程没有 tty，根本收不到 Ctrl-Z。

### §6 子进程与 deadline：明确不动

- **子进程继续跑。** `bash` 与动态工具用 `process_group(0)` 把命令放进**它自己的进程组**（[`src/tools/process.rs:125-127`](../../src/tools/process.rs)），发给 fs-agent 进程组的 SIGTSTP 到不了它；它的 stdin 是空的、stdout/stderr 是管道（`:122-124`），没人读时最多把 64KB 管道填满、阻塞在 `write`，恢复后自动继续。**不引入**「活跃子进程组」这条从工具层到手势层的注册表。
- **deadline 不冻结。** `bash` 的超时是 `tokio::time::Instant::now() + limit`（[`:152`](../../src/tools/process.rs)），走 `CLOCK_MONOTONIC`、挂起期间照走，到点就 `killpg` 整棵树（`:169-178`）。provider 那一层的超时在 reqwest 内部，我们根本碰不到 —— 所以「挂起期间冻结所有 deadline」只能冻结一半，而**只冻结一半比不冻结更难解释**。**接受**这个后果：挂得比剩余超时久，恢复后那个回合会以 provider 断流或命令超时收尾，走现有的错误呈现，不新造一套。
- **替代做法写进文档**：真想「停下来别动我的机器」的人，正确动作是**先 Ctrl-C 取消、再 Ctrl-Z** —— 取消路径本来就会 `killpg` 掉活跃工具（[`:251-256`](../../src/tools/process.rs) 的 `ProcessGroup`）。

## 测试决定

- **单测**（`tests/render_tui.rs`，与 exit-gesture 同一片，prior art 是它的 `map_key` 断言）：`Ctrl+Z` 映射成 `Key::CtrlZ`；空闲、忙碌、重放三态各按一下都置挂起请求；详情覆盖层与问卷立着时也置位；请求取走后状态照旧（不退出、不推 `FrontEndEvent`）。**不**测终端 —— 那是 pty 的地盘。
- **pty**（`scripts/tui-startup-check.py`，prior art 是它现有的 `GESTURES` / `capture_busy` / `terminal_handed_back`）：
  - **两条路都要求 pty 上有一个真正的会话与前台进程组**，而且 fs-agent 所在的进程组不能是**孤儿组**：前者不然终端驱动无处投递那个字节（plain 的 `\x1a` 就是它在投），后者内核直接把 SIGTSTP 丢掉（POSIX 如此规定，免得停住的作业没人能 `fg` 回来）。`pty.fork()` 的 `setsid` 恰好落进孤儿组，所以挂起路径改用两层结构：会话头 `setsid` 并拿走控制终端，再 fork 出 fs-agent、把它设成前台进程组；会话头等孙进程结束，用自己的退出码把它的退出状态带回来 —— 于是脚本虽然 `waitpid` 不到那个进程（状态从 `/proc` 读），仍然有一个可断言的退出码。
  - **TUI**：发 `\x1a`（raw 模式下它是**按键**，由 fs-agent 自己发信号），断言进程进了 stopped 状态、**而且此刻终端已经交还**（termios 回到 canonical/echo/ISIG，交还序列全部落在停止之前）；`SIGCONT` 恢复，断言备用屏幕重进、清屏重绘、标题被重新保存，再走一次正常出口并复查 TEARDOWN。
  - **plain**：同样发 `\x1a`（这里它是**信号**，由终端驱动产生），断言进 stopped、`SIGCONT` 后干净退出 —— 这一条就是「plain 不进 raw 模式」的回归：谁给 plain 加了 raw 模式，它必红。
  - **横幅的计数不能照搬退出路径**：恢复时的全量重绘会把 `wording::banner` 那一行再画一次，所以断言是「停下之前恰好一次」加「恢复后清屏重绘」，不是「全程恰好一次」。
- **手工**（`docs/tui-manual-checklist.md`）：挂起 → 在 shell 里跑两条命令 → `fg` → 画面完整无残影、标题正确、转录滚动位置没丢；再加一条「忙碌态挂起，回来看到超时/断流的正常收尾」。
- **文档**（`docs/render.md`）：新开一节「挂起与恢复」，写 TUI 的交还/重进顺序与 plain 的天然行为；§6 的「先 Ctrl-C 再 Ctrl-Z」落在同一节。
- **词表**（`CONTEXT.md`）：「控制」一节加**挂起**词条 —— 在 spec 落盘这一刻就写（domain-modeling 的纪律是术语一解决就进词表，不攒到最后）。

## 明确不做

- 不做「后台继续跑」（daemon 化加重连）：那是另一个 feature。
- 不做 `/suspend` 一类的命令入口。
- 不做双击、举手、提示行中间态。
- 不跟踪、不暂停子进程组。
- 不冻结 `bash` 或 provider 的 deadline。
- 不给 headless 加手势（没有键盘）。
- 挂起与恢复都不打回执、不写事件流、不写状态文件。
- 不把挂起做成可配置（没有超时、没有键位配置）。

## 补充说明

- **为什么不复用 `ratatui::init()`**：它每次都会 `set_panic_hook()`（`take_hook` 再包一层），反复挂起会把 hook 叠成一串。
- **`ProcessGroup` / `killpg` 不动**：`bash` 的进程组清理与挂起无关，两条路径互不影响。
- **已知取舍**：挂起久了，恢复后回合可能以 provider 断流或 `bash` 超时收尾；恢复后的画面是重绘出来的、不是冻结的像素 —— 滚动位置、草稿、转录都在渲染器状态里，所以看起来与挂起前一致。
