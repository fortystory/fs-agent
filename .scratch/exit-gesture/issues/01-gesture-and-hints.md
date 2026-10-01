# 退出手势与提示：空闲/忙碌/重放的双击，以及拆掉退出确认覆盖层

Type: implement
Status: done
Blocked by: —

> 规格：`.scratch/exit-gesture/spec.md` §1（手势与窗口）、§2（提示行上的三句文案）、§4（删掉 `Pending::Exit`）、§7（明确不动的部分）。
> 本票只动渲染器侧：手势状态机、提示行文案、确认覆盖层的拆除，以及被这三件事带红的现有测试与验收面。CLI 侧的 130 收尾是 [票 03](03-ordered-exit-130.md)，按需武装的 deadline 分支是 [票 02](02-idle-deadline-clock.md)。

## 目标

- 空闲 `Ctrl-C` 与 `Ctrl-D` 完全对等：第一下**举手**，窗口内第二下退出；两键**共用同一把举手**。
- 忙碌第一下 `Ctrl-C` = 取消（现状不动）**并且**举手；忙碌 `Ctrl-D` 维持忽略。
- 重放中 `Ctrl-C` 也走双击；举手时它的进度行让位给举手文案。
- 三句举手文案进 `wording`，举手期间**出口段整段替换**（不是追加）。
- `Pending::Exit` 覆盖层连同 modal、命中区、文案一起删掉。

## 现状（2026-10-01 量的，改前先复核）

- `key()`：空闲 `Ctrl-C` 直接 `self.quit = true`（`src/render/tui.rs:2172-2179`）；空闲 `Ctrl-D` 打开 `Pending::Exit`（`:2183-2188`）；忙碌时 `Ctrl-C` 每一下都只推 `FrontEndEvent::Cancel`，`Ctrl-D` 忽略。
- 重放：`src/render/tui.rs:1562-1567`，`Ctrl-C` 直接 `quit`，`Ctrl-D` 与 `Esc` 无效。
- 详情覆盖层：`src/render/tui.rs:2158-2170`，`Ctrl-D` / `Esc` 关覆盖层、`Ctrl-C` 被忽略 —— **本票不动**。
- 提示行：`wording::EXIT_HINT_IDLE`（`src/render/wording.rs:1062`）、`EXIT_HINT_BUSY`（`:1066`）、`exit_hint`（`:1101`）、`status_line`（`:1086`）、`viewer_status_line`（`:1095`），阶梯在 `hint_line`（`:1108` 起）；重放进度行**整条替换**的先例在 `src/render/tui.rs:2486-2499`。
- `Pending::Exit` 的全部落点：变体与 modal 组装（`tui.rs:901-909`）、点击 match（`:1819`、`:1839`）、`own_answer`（`:1863`）、`answer_pending`（`:2450`）、`decline`（`:2480`）；文案 `wording::exit_title`（`wording.rs:825`）、`exit_body`（`:831`）、`EXIT_CHOICES`（`:698`）；`HitAction::Quit`。
- `agrees`（`src/render/tui.rs:1075`）**不是** Exit 专用：`Pending::Paste`（`:2437-2442`）与 `Pending::ClearDraft`（`:2443-2447`）也在用 —— **保留**（spec §4 让实现时先查，结论是留着）。
- `CONTEXT.md:106` 的「退出举手（ExitGesture）」词条已经写好，与本票行为一致，**不改**。

## 落点

- `src/render/tui.rs`：`TuiState`、`key()`、`replay_key()`、`status_line()`、`Pending` 及其组装 / 命中 / 作答分支。
- `src/render/wording.rs`：三句举手文案、`exit_hint`、`status_line` / `viewer_status_line`。
- 连带验收面：`tests/render_tui.rs`、`tests/render_layout.rs`、`tests/history_replay.rs`、`tests/wording.rs`、`docs/tui-manual-checklist.md`、`scripts/tui-startup-check.py`。

## 具体行为

### 1. 举手状态（`src/render/tui.rs`）

- `TuiState` 加 `exit_deadline: Option<Instant>`（`std::time::Instant`）；字段有值 = 正在举手。
- 常量 `EXIT_GESTURE_WINDOW: std::time::Duration = Duration::from_millis(500)`，与 `PULSE_FRAME`（`src/render/tui.rs:3224`）并列，**不做配置项**（spec 明确不做）。
- 三个方法（都 `pub`，渲染器状态本来就以 `pub` 方法露给循环与测试），时间可注入，测试不睡真实时间：
  - `fn raise_exit_gesture_at(&mut self, now: Instant)`：`exit_deadline = Some(now + EXIT_GESTURE_WINDOW)`、置 `dirty`；
  - `fn exit_gesture_raised(&self, now: Instant) -> bool`：`exit_deadline.is_some_and(|deadline| now < deadline)`；
  - `fn expire_exit_gesture(&mut self)`：清 `exit_deadline`、置 `dirty` —— 它是超时作废的**唯一**入口（主循环到点调它，见 [票 02](02-idle-deadline-clock.md)）。
  - `key()` 内部用 `Instant::now()` 调前两个。
- 举手是**纯渲染器状态**，不进事件流；`should_quit()` 只反映 `self.quit`。

### 2. `key()` 与 `replay_key()` 重写

- `key()` 一开始（详情覆盖层守卫与问题守卫**之前**）：若 key 不是 `Ctrl-C` / `Ctrl-D`，先 `expire_exit_gesture()` 清掉旧举手，再照常处理 —— 避免半分钟前那一下莫名其妙地算数。
- `Ctrl-C` / `Ctrl-D` 共用一把举手，按当前视图分派：
  - **详情覆盖层立着**：照现状 —— `Ctrl-D` 关覆盖层、`Ctrl-C` 忽略，不参与退出（`src/render/tui.rs:2158-2170`，本票不动）。
  - **重放中**（`replay_key`，`src/render/tui.rs:1562-1567`）：`Ctrl-C` 第一下举手、第二下 `self.quit = true`；`Ctrl-D` 与 `Esc` 维持"无效"。
  - **忙碌**（`self.busy()`）：`Ctrl-C` 第一下推 `FrontEndEvent::Cancel` 并举手；举手状态下第二下推 `FrontEndEvent::Quit`（该变体已存在，`src/render/input.rs:119-120`）。CLI 侧的消费与 130 收尾是 [票 03](03-ordered-exit-130.md)；本票落地后 `Quit` 暂时只被 `run_one_turn` 当取消处理，不会退错。`Ctrl-D` 在举手与否时都**忽略**，且忽略时**不清**举手（这一条要显式写，因为"其它按键清举手"会把忙碌的举手误清）。
  - **空闲**：`Ctrl-C` 与 `Ctrl-D` 第一下举手、举手状态下第二下 `self.quit = true`；混按（`Ctrl-C` 然后 `Ctrl-D`）也算第二下。
- `/quit` 维持一下退出（`src/cli.rs:1122`，本票不动）。

### 3. 提示行（`src/render/wording.rs`）

三句举手文案（spec §2 的表格，一字不改）：

| 时刻 | 出口段 |
| --- | --- |
| 空闲，已举手 | `再按一次 ctrl-c/ctrl-d 退出` |
| 忙碌，已举手 | `已取消 · 再按一次 ctrl-c 退出` |
| 重放，已举手 | `再按一次 ctrl-c 退出` |

- 新增三个常量（名字实现定，例如 `EXIT_HINT_IDLE_RAISED` / `EXIT_HINT_BUSY_RAISED` / `EXIT_HINT_REPLAY_RAISED`），与 `EXIT_HINT_IDLE` / `EXIT_HINT_BUSY` 并列；未举手那两句**保留**，它们仍要出现在未举手的状态行里。
- `exit_hint(busy)` 扩展成能返回举手那档（例如 `exit_hint(busy, raised)`，或新增 `exit_hint_raised(...)` 由调用点挑）；`status_line` 与 `viewer_status_line` 各多接一个 `raised: bool` 参数，内部照旧走 `hint_line`。
- **整段替换 = 替换出口段那一项**，不是在旧出口段后面拼接：屏幕上不许出现"旧出口文案 + 举手文案"那种追加形态。`hint_line` 的阶梯规则（出口永不丢、状态词最先丢、其余按宽度让位）**不动**；宽终端里 `KEY_HINTS` 的条目照旧都在。
- **重放举手**：`TuiState::status_line()`（`src/render/tui.rs:2486-2499`）现在重放时直接返回 `history_progress_line`；改成举手优先 —— 举手期间返回那句 `再按一次 ctrl-c 退出`，进度行**暂时收起**（spec §2 的「让位」）。
- 举手开始与结束都要重画提示行（`raise_exit_gesture_at` / `expire_exit_gesture` 都置 `dirty`）。

### 4. 删掉 `Pending::Exit`（spec §4）

- 删：枚举变体 `Pending::Exit`、modal 组装（`src/render/tui.rs:901-909`）、所有 match 分支（`:1819`、`:1839`、`:1863`、`:2450`、`:2480`）、`HitAction::Quit`，以及 `wording::exit_title` / `exit_body` / `EXIT_CHOICES`（`src/render/wording.rs:825` / `:831` / `:698`）。删完各 match 保持穷尽。
- **保留** `agrees`（见「现状」）。
- 不做「有未发送草稿就弹一次」的特判：草稿会丢是明确接受的取舍（spec「明确不做」）。

## 测试

- `tests/render_tui.rs`：
  - 改写 `ctrl_c_quits_before_the_loop_has_asked_for_its_first_line`（`:175`）：组装期第一下 `Ctrl-C` **不**退出、只举手；再一下才退出。
  - 改写 `an_idle_ctrl_c_quits_and_a_working_one_cancels`（`:188`）：空闲第一下不退、第二下退；`Ctrl-C` → `Ctrl-D` 混按也算第二下；忙碌第一下 `take_events() == [Cancel]` 且 `!should_quit()`，第二下 `== [Quit]`。
  - 改写 `a_run_that_never_ended_a_turn_still_leaves_ctrl_c_quitting`（`:203`）：没有 `TurnEnded` 到达时，空闲双击照样退得出去。
  - 改写 `ctrl_d_asks_before_it_quits_and_the_safe_answer_is_no`（`:238`）为双击语义：`y` / `Enter` / `Esc` 那套确认断言整条删掉。
  - 保留 `ctrl_d_is_ignored_while_a_run_is_in_flight`（`:263`）与 `ctrl_d_is_ignored_while_a_question_is_up`（`:273`）：忙碌、问题立着时 `Ctrl-D` 仍然什么都不做。
  - 新增：其它按键清旧举手（`Ctrl-C` → `Char('a')` → `Ctrl-C` 不退出，仍是第一下）。
  - 新增：可注入时间的超时 —— `raise_exit_gesture_at(t0)` 后 `exit_gesture_raised(t0 + 400ms)` 为真、`t0 + 600ms` 为假；`expire_exit_gesture()` 后为假且 `!should_quit()`；`EXIT_GESTURE_WINDOW` 是 500ms。
- `tests/history_replay.rs`：改写 `ctrl_c_quits_during_a_replay_and_ctrl_d_and_esc_are_inert`（`:432`）—— 重放中两次 `Ctrl-C` 才退，`Ctrl-D` / `Esc` 仍然 inert。
- `tests/render_layout.rs`：
  - `the_renderer_confirmations_answer_by_click`（`:3934`）整条删除（它测的就是 Exit 的点击作答，`[n] 取消` 那半随覆盖层一起没了）；如果实现选择保留一条负向断言（"渲染器不再提退出确认"），也可以，但文件要保持绿。
  - `ctrl_d_closes_the_detail_overlay_rather_than_asking_to_quit`（`:4388`）改名（例如 `ctrl_d_closes_the_detail_overlay`），删掉 `:4407` 那句 `退出会话` 断言；覆盖层开合的断言保留。
  - 新增帧断言：举手后提示行的出口段是那句新文案（空闲 `再按一次 ctrl-c/ctrl-d 退出`、忙碌 `已取消 · 再按一次 ctrl-c 退出`），**且宽终端（120 列）里 `KEY_HINTS` 的条目仍在**（这是"整段替换"的可测形态）；未举手时照旧；重放举手时进度行让位。
  - `:4465` 那段详情覆盖层里 `Ctrl-C` 被忽略的断言**不动**。
- `tests/wording.rs`：三句举手文案与未举手两句（`EXIT_HINT_IDLE` / `EXIT_HINT_BUSY`）都有断言；`status_line` / `viewer_status_line` 的新参数在各调用点同步。
- `docs/tui-manual-checklist.md:109-112` ⑦ 第 1 条整条重写为双击语义（空闲两键对等、忙碌第一下取消第二下退、忙碌 `Ctrl-D` 忽略、详情覆盖层 `Ctrl-D` 关覆盖层）。
- `scripts/tui-startup-check.py`：`GESTURES`（`:88`）里 `("ctrl-d y", b"\x04y")` 改成双击 `b"\x04\x04"`；**同一行里的 `("ctrl-c", b"\x03")` 也必须改成 `b"\x03\x03"`** —— 新的空闲语义下单击不再退出，不改这一条脚本必红（spec §4 只点了 `ctrl-d y`，这是本票补上的必然连带面）。两次按键要落在 500 毫秒窗口内，同一个 `write` 一次发两个字节即可。
- `cargo test` 全绿；`cargo build` 后 `python3 scripts/tui-startup-check.py` 的三条空闲出口全绿（忙碌双击那条在 [票 05](05-acceptance.md)）。

## 不做什么

- 不碰 CLI 侧：`run_one_turn` 对 `Quit` 的消费、三处 `std::process::exit(130)` 的收尾、130 的返回路径全归 [票 03](03-ordered-exit-130.md)。
- 不加主循环的 deadline 分支、不改 `src/render/tui.rs:331-336` 的"唯一时钟"注释、不改 `CONTEXT.md` 的「脉冲」词条（都归 [票 02](02-idle-deadline-clock.md)）。
- 不删也不改 `CONTEXT.md:106` 已经写好的「退出举手」词条。
- `/quit`、`Esc` 取消、模式循环、目标停确认框、详情覆盖层键位、忙碌 `Ctrl-D`、`ConsoleEvents` / `FrontEndEvent` 的其余语义：都不动（spec §7）。
- 不给窗口加配置项；不把空闲双击改成 130；不在 `--plain` 里加手势（spec「明确不做」）。
- 不碰 `SessionFacts.session_id` 这个死字段（spec「补充说明」）。

## Comments

- **落地**：`TuiState` 加 `exit_deadline: Option<Instant>`；常量 `EXIT_GESTURE_WINDOW`（500 ms，与 `PULSE_FRAME` 并列，内部单测钉住）；三个方法 `raise_exit_gesture_at` / `exit_gesture_raised` / `expire_exit_gesture`（时间可注入）；`exit_key` 是 `Ctrl-C` / `Ctrl-D` 共用的那一把举手 —— 空闲两键对等（第一下举手、第二下退，混按也算），忙碌只有 `Ctrl-C` 参与（第一下 `Cancel` + 举手，第二下 `FrontEndEvent::Quit`），忙碌 `Ctrl-D` 忽略且**不清**举手。`key()` 里「不是这两键就先 `expire_exit_gesture()`」放在详情覆盖层守卫之后、问题守卫之前；`replay_key()` 里 `Ctrl-C` 同样双击，别的键先清举手。
- **提示**：`wording` 新增 `EXIT_HINT_IDLE_RAISED` / `EXIT_HINT_BUSY_RAISED` / `EXIT_HINT_REPLAY_RAISED`，`exit_hint(busy, raised)` 四档；`status_line` / `viewer_status_line` 多一个 `raised` 参数（内部照旧走 `hint_line`，阶梯一行未改）。`TuiState::status_line` 在重放时举手优先于进度行（进度行让位，不追加）。
- **拆覆盖层**：`Pending::Exit`、它的 modal 组装、四处 match 分支、`HitAction::Quit`，以及 `wording::exit_title` / `exit_body` / `EXIT_CHOICES` 全删；`agrees` 保留（`Paste` 与 `ClearDraft` 在用）。
- **测试**：`tests/render_tui.rs` 改写四条（组装期、空闲/忙碌、没有 `TurnEnded`、`Ctrl-D` 那条换成双击）并新增两条（别的键清举手、可注入时间的窗口与作废）；`tests/history_replay.rs` 改写重放那条并新增「举手让位给进度行」；`tests/render_layout.rs` 删掉 `the_renderer_confirmations_answer_by_click`、`ctrl_d_closes_the_detail_overlay_rather_than_asking_to_quit` 改名并去掉「退出会话」断言、新增两条帧断言（举手替换出口段且 `enter 发送` / `esc 取消` 仍在、旧出口段不在了）；`tests/wording.rs` 新增一组举手文案断言。
- **验收面**：`docs/tui-manual-checklist.md` ⑦ 第 1 条整条重写为双击语义（含「那个确认框已经拆了」），并新增第 5 条「忙碌双击退出后终端干净 + 回执打在 shell 里」。
- **pty 脚本的一处连带面**（票只点了 `ctrl-d y`）：`GESTURES` 的两条都改成双击，**`--continue` 那一轮也要跟着改** —— 它走的是 `capture` 的默认手势 `b"\x03"`，改成双击之后才退得出去（只改 `GESTURES` 会让四条 `--continue` 全红，实测确认）。本机实跑 12/12 GREEN。
- `cargo test` 全绿（964 条）；`cargo clippy --all-targets` 干净。
- **补记（票 05 收口时改的一处行为）**：票 01 写的是「空闲：举手状态下第二下 `self.quit = true`」，而忙碌那一把举手在**第一下取消已经落地、`busy()` 变成假之后**会掉进这条空闲分支 —— 那时退出码会退成 0，与 spec §3「忙碌中被打断而退 = 130」相冲（pty 实测就是这么红的）。修法：`TuiState` 多一个 `exit_gesture_busy`，忙碌里举的举手记得自己的出身，第二下即使渲染器已经空闲也推 `FrontEndEvent::Quit`；空闲举的那把仍然 `self.quit = true`。回归测试见 `tests/render_tui.rs`，票 05 的 Comments 有完整记录。
