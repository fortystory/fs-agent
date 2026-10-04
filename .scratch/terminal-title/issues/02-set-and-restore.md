# 把标题发给终端：保存、写入与还原，绘制路径上比对

Type: implement
Status: done
Blocked by: 01

> 规格：`.scratch/terminal-title/spec.md` §4（搭 `TerminalModes` 的车：`\x1b[22;0t` 保存 / OSC 0 写入 / `\x1b[23;0t` 还原，含 panic 路径）、§5（`TuiState` 加 `last_title`，在绘制路径上比对，变了才写）、§6（只给 TUI 设、标题不进事件流）。
> 票 01 给的是纯函数；这一票是它的调用方：终端序列、`TuiState` 的新状态、目标名与 cwd 的注入、以及 pty 脚本里的两条序列检查。

## 目标

- 进 TUI 时保存原标题、写第一版标题；退出（含 panic）时还原。
- 标题只在**真的变了**的时候重写：把期望值算在绘制路径上，与 `last_title` 比对。
- 目标名与 cwd 从渲染器已有的东西里取，不为标题新增会话事件（spec §6）。

## 现状

- [`TerminalModes`](../../../src/render/tui.rs) 住在 `src/render/tui.rs:422-450`：`enter()`（`:425-435`）开鼠标与括号粘贴（`:426`）、把 ratatui 的 panic hook 包一层（`:429-433`）；`Drop`（`:438-442`）与 panic hook 都调 `disable_terminal_modes()`（`:444-450`）。**这就是 spec §4 说的那辆车。**
- 进入/退出顺序：`ratatui::init()` 在 `TerminalModes::enter()` 之前（`:317-318`），`drop(modes)` 在 `ratatui::restore()` 之前（`:412-413`）；标题是窗口属性，早一步晚一步都不影响 alt screen（spec §4）。
- 原始序列的先例是 `src/render/tui.rs:401-404` 的 `execute!(... BeginSynchronizedUpdate)`；仓库当前锁 crossterm 0.29（`Cargo.toml:34`）。**crossterm 0.29 有 `SetTitle`**（`crossterm::terminal::SetTitle`，`impl Command`，发出的正是 `\x1B]0;{}\x07`），所以写 OSC 用这个命令；`\x1b[22;0t` / `\x1b[23;0t` 没有对应命令，直接写序列（`crossterm::style::Print`）。spec §4 说「若 crossterm 提供了 `SetTitle` 一类命令，优先用它」——它提供了。
- `TuiState` 的字段在 `src/render/tui.rs:460-571`：`running`（`:548`）、`pending`（`:554`）、`replay`（`:561`）、`dirty`（`:483`）；`busy()` 在 `:2297-2299`，`is_dirty()` / `mark_clean()` 在 `:1256-1266`。
- **目标名并不在渲染器手里**：`RenderEvent::Logged(Event)` 会到达 `TuiState::apply`（`src/render/tui.rs:1312`），但 [`src/render/transcript.rs:411-415`](../../../src/render/transcript.rs) 把 `GoalSelected` / `GoalCompleted` / `GoalStopped` 直接丢掉，`TuiState` 里也没有任何字段接它。spec §6 说的「渲染器已有的事实」目前只到「事件到了 `apply`」这一层，得在这里自己留一份。
- cwd 也不在 TUI 手里：`SessionFacts`（`:256-280`）没有工作目录（`session_dir` 是会话存储目录，不是 cwd）。CLI 组装处**已经有** `cwd` 变量（`src/cli.rs:242-254` 与 `:553-565`：`parsed.cwd` 或 `std::env::current_dir()`），两处 `TuiOptions` 构造在 `src/cli.rs:339-345` 与 `:627-631`。
- pty 脚本 [`scripts/tui-startup-check.py`](../../../scripts/tui-startup-check.py) 已经有「退出时交还终端」的序列清单 `TEARDOWN`（`:76-83`），判定在 `verdict` 里：`missing = [seq for seq in TEARDOWN if seq not in run.raw]`（`:373-377`）。它抓的是原始字节流（`capture`，`:249-318`），而屏幕仿真在 `:169` 那条正则里就把 `\x1b]…\x07` 吃掉了，标题不会污染仿真屏幕 —— 那里**不用**改。

## 落点

- [`src/render/tui.rs`](../../../src/render/tui.rs)：`TerminalModes` / `disable_terminal_modes`、`TuiState` 字段与两个方法、`Tui::run` 里的写点、`apply` 里的目标名。
- [`src/cli.rs`](../../../src/cli.rs)：`TuiOptions` 的 `cwd` 字段，两个构造点。
- [`tests/render_tui.rs`](../../../tests/render_tui.rs)、[`tests/history_replay.rs`](../../../tests/history_replay.rs)：`TuiState::new` 的调用点。
- [`scripts/tui-startup-check.py`](../../../scripts/tui-startup-check.py)：模块 docstring 与 `TEARDOWN` / `verdict`。

## 具体行为

1. **序列（spec §4）。** 在 `src/render/tui.rs` 里落一个模块级
   `fn set_terminal_title(title: &str) { let _ = execute!(std::io::stdout(), SetTitle(title)); }`
   （`use ratatui::crossterm::terminal::SetTitle;`；错误照现有写法吞掉）。
   - `TerminalModes::enter(title: &str) -> Self`：保留现有的鼠标 / 括号粘贴与 panic hook 包裹（`:426-433`），先 `let _ = execute!(std::io::stdout(), Print("\x1b[22;0t"));`（`use ratatui::crossterm::style::Print;`），再 `set_terminal_title(title)`。**保存在前、写在后的顺序照 spec §4。**
   - `disable_terminal_modes()`：把 `Print("\x1b[23;0t")` 并进现有那条 `execute!`（`:445-449`）。它被 `Drop`（`:438-442`）与 panic hook（`:431`）共用，所以 panic 退出也还原；被调两次是两次 no-op 式的重复序列，无害。
   - `enter()` 的签名变了，调用点 `:318` 一起改。
2. **注入 cwd 与 home（本票补判，spec 没写）。** spec §补充说明 明说不要把标题相关的东西塞进 `SessionFacts`，所以：
   - `TuiOptions`（`:283-295`）加 `pub cwd: std::path::PathBuf`，两个构造点（`src/cli.rs:339-345`、`:627-631`）传组装处已有的 `cwd` —— `--cwd` 已经在那两个函数里解析过，标题用的就是**同一个**工作目录。
   - `home` 在 `Tui::run` 里读一次：`let home = std::env::var_os("HOME").map(std::path::PathBuf::from);`。
   - `TuiState::new(facts, cwd: PathBuf, home: Option<PathBuf>)`：`TuiState` 存 `cwd` / `home` 两个字段，`title()` 因此是纯函数，测试能注入固定值（不让测试随开发机的 `HOME` 漂）。
   - 签名变的调用点共五处：`src/render/tui.rs:313`、`src/render/tui.rs:4628`（文件内单测的 `state()`）、`tests/render_tui.rs:49`、`tests/history_replay.rs:40` 与 `:46`。
3. **目标名（spec §6 的「已有的事实」）。** `TuiState` 加 `goal: Option<String>`；在 `apply`（`:1312`）开头、`self.transcript.push(event)` **之前**看 `RenderEvent::Logged(event)` 的 `event.payload`：
   - `EventPayload::GoalSelected { goal }` → `self.goal = Some(goal.clone())`；
   - `EventPayload::GoalStopped { .. } | EventPayload::GoalCompleted { .. }` → `self.goal = None`；
   - 其余不动。
   重放走的是同一个 `apply`（`replay_batch`，`:1514-1536`），所以 `--continue` 的目标名自然重建，不需要第二套路径。**补判**：spec §1 说「没有正在推进的目标时省略」，而 `events::current_goal`（`src/events.rs:824-829`）只认最后一条 `GoalSelected`、目标停下后仍会返回名字 —— 标题以「正在推进」为准，所以停下 / 完成这两条都清。
4. **标题的两个方法。**
   - `pub fn title(&self) -> String`：`wording::terminal_title(&self.cwd, self.home.as_deref(), wording::title_state(self.replay.is_some(), self.pending.is_some(), self.busy()), self.goal.as_deref())`。优先级三布尔只在票 01 的 `title_state` 里判一次。
   - `pub fn sync_title(&mut self) -> Option<String>`：算一次 `title()`，与 `self.last_title` 相同返回 `None`；不同就把快照换成新值并返回 `Some`。
5. **第一版标题写在哪（spec §4 与 §5 的重叠，本票补判）。** §4 要 `enter()` 里先保存再写第一版，§5 又要绘制路径比对后写；两处都写会重复，快照也会不同步。本票钉成：**`enter()` 写第一版、`run` 把快照一起定下来；绘制路径只管后续变化**。`Tui::run`（`:307-414`）里：
   ```
   let first = state.sync_title().expect("首帧之前 last_title 是空的");
   let modes = TerminalModes::enter(&first);
   ```
   位置在 `:317-318`（`ratatui::init()` 之后），并且**在 `:321-329` 等重放之前** —— `--continue` 的第一帧可能压在整段重放之后，标题不该跟着等那么久。
   - 之后在 `if state.is_dirty()` 那段（`:397-406`）里、`state.mark_clean()` 之后加：
     `if let Some(title) = state.sync_title() { set_terminal_title(&title); }`
     这样「只在状态变化时更新」自动成立，不需要枚举事件源（spec §5）；不逐 token 写（spec §5 末）。
6. **只给 TUI 设（spec §6）。** `Renderer::plain` / `headless` 不经过 `TerminalModes`，天然不设；**不特意为它们加代码**。

## 测试

- **状态机**（[`tests/render_tui.rs`](../../../tests/render_tui.rs)，不碰真实终端）。`new_state()` 注入固定 `cwd = /home/forty/code/fs-agent`、`home = Some(/home/forty)`，新增：
  - 空闲：`title()` 得 `~/code/fs-agent`；
  - `ConsoleRequest::RunState { running: true }` 后得 `~/code/fs-agent · 运行中`（`RunState` 在 `src/render/tui.rs:1978`）；
  - 一次 `ConsoleRequest::Ask(...)`（审批）后得 `~/code/fs-agent · 等你`；
  - `ConsoleRequest::Replay { events: vec![…] }` 后得 `~/code/fs-agent · 重放中`（`:2052`）；
  - 喂一条 `RenderEvent::Logged(Event::new(1, SpeakerId::System, EventPayload::GoalSelected { goal: "修文档索引".into() }))` 且 `running = true`（`apply` 的入参形状见 `src/render/tui.rs:1312`，构造见 `tests/history_replay.rs:64-66`）：得 `~/code/fs-agent · 运行中 · 修文档索引`；再喂一条 `GoalStopped`，目标名消失；
  - 优先级：`replay` 与 `pending` 同时立着时输出以 `重放中` 收尾；只有 `pending` 与 `busy` 时以 `等你` 收尾。
  - `sync_title()` 的语义：连续两次调用，第二次是 `None`；中间改一次 `RunState`，第二次是 `Some`（这条是 spec §5 的「变了才写」在状态机这一层的断言）。
- **终端序列**（[`scripts/tui-startup-check.py`](../../../scripts/tui-startup-check.py)，`TestBackend` 不执行 `execute!`，OSC 只有 pty 看得见）：
  - 在 `verdict`（`:321-389`）里加一条**进入侧**检查：`run.raw` 里至少有一条 `\x1b]0;…\x07`，其内容含 cwd 基名。基名在 `main()`（`:400-427`）里用 `os.path.basename(os.getcwd())` 算好、作为参数传进 `verdict`（子进程继承脚本的 cwd，`capture` 只改 `XDG_DATA_HOME`，`:264-266`）。**任一**一条命中即可，因为重放那一轮中间还会写 `重放中`。
  - 在 `TEARDOWN`（`:76-83`）里加 `"\x1b[23;0t"`：退出侧的 `missing = [seq for seq in TEARDOWN if seq not in run.raw]`（`:373-377`）自动把三条退出路径（`Ctrl-C` / `/quit` / `Ctrl-D y`）和 `--continue` 轮都覆盖上。
  - 建议顺带断言 `"\x1b[22;0t"` 也在 `run.raw` 里（spec §4 的保存那一半）；它不是硬要求，但只多一行。
  - 更新脚本 docstring（`:1-35`）与 `TEARDOWN` 上方那段注释（`:76-83`）：它现在守的两头里多了「进 TUI 时设标题、退出时还原」。
  - 跑法：`cargo build && python3 scripts/tui-startup-check.py`；四类出口 × 每轮 + `--continue` 全绿。
- **回归**：`cargo test` 全绿（`TuiState::new` 签名变化会牵动 `tests/render_tui.rs` 与 `tests/history_replay.rs`，两处都要跟着补 `cwd` / `home` 实参）。

## 不做什么

- **不给 `--plain` / headless 加任何代码**，它们不该多发一个转义序列（spec §6、§明确不做）。
- 标题**不进事件流**、不进 `log.jsonl`、`--continue` 不重建它（spec §6）；不为它新增会话事件，也不读 `Harness` / `SessionFacts` 去要目标名 —— 目标名从 `apply` 收到的那条事件里留（spec §6）。
- **不读终端响应、不查询原标题**（spec §明确不做）。
- **不包 tmux passthrough 序列**（`\x1bPtmux;…`）（spec §明确不做）。
- 不加配置项、不做标题模板语法；不改状态行 / 提示行里已有的任何文本（spec §明确不做）。
- 不逐 token 更新、不为 OSC 加动画 / 重绘循环（spec §5 末）。
- 不改票 01 定下的措辞与 40 列规则；不改 `src/render/wording.rs`（除了调用它的新函数）。

## 评论

- **落地**：`set_terminal_title`（crossterm 的 `SetTitle`，发的就是 `OSC 0`）与 `TerminalModes::enter(title)`（先 `CSI 22 t` 保存、再写第一版）／`disable_terminal_modes`（并进 `CSI 23 t`）。`TuiOptions` 加 `cwd`（两个构造点传 `--cwd` 那一个）；`$HOME` 在 `Tui::run` 里读一次；`TuiState::new(facts, cwd, home)`，五个字段新增：`cwd` / `home` / `goal` / `last_title`。`title()` 是纯函数，`sync_title()` 做比对；第一版在 `ratatui::init()` 之后、等重放之前写，绘制路径在 `mark_clean()` 之后补后续变化。
- **目标名**：`observe_goal` 在 `apply` 开头看 `RenderEvent::Logged` 的 payload —— `GoalSelected` 留名、`GoalStopped` / `GoalCompleted` 清名；重放走同一个 `apply`，所以 `--continue` 自然重建。
- **测试**：`tests/render_tui.rs` 新增 5 条（空闲 / `运行中` / `等你` / `重放中`、优先级两条、目标名来去、`sync_title` 的「变了才写」）；`TuiState::new` 的调用点全部补参（`render_layout.rs`、`render_tui.rs`、`history_replay.rs`、`ask_user_question_tui.rs`、`tui.rs` 内部单测）。
- **pty 脚本**：`TEARDOWN` 加 `\x1b[23;0t`，`verdict` 新增进入侧两条（`\x1b[22;0t` 在场、`OSC 0` 的内容含 `os.path.basename(os.getcwd())`），docstring 与 `TEARDOWN` 上方注释同步。**实跑**（本机沙箱里用 `FS_AGENT_MODEL=kimi-for-coding`，因为默认模型那个 provider 在这台机器上没有 key）：12/12 GREEN —— 三条空闲出口加 `--continue` 共四类，各自都验到标题的保存与还原序列。
- **没做**：真终端里「标题看起来对不对」是 [票 03](03-manual-and-docs.md) 的手工面，见那一票的 Comments。
- `cargo test` 全绿（959 条）；`python3 scripts/check-language.py` 通过。
