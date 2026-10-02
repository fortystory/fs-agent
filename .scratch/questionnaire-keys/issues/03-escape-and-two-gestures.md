# 03 — `Esc` 只管退出这次询问，取消运行归 `Ctrl-C`

Type: implement
Status: done
Blocked by: 02

> 来源：[`../spec.md`](../spec.md) §5。它把「这道题不答了」这条今天**只在不忙时才存在**的语义
> （[:2462-2463](../../../src/render/tui.rs)）提升成问卷里 `Esc` 的唯一意思，
> 并把「取消这次运行」明确交给 `Ctrl-C`。

## 目标

- **问卷立着时 `Esc` 不再走既有分叉**（busy / idle / 禁言四支，[:2445-2476](../../../src/render/tui.rs)），
  一律走问卷自己的区域逻辑——与运行状态无关。
- **选项区**：第一下**举手**（500ms 内第二下 = 退出询问）。
- **输入区**：那一下**同时做两件事**——回选项区（文本不清）**并且**举手，所以任何区域
  连按两下 `Esc` 都是退出询问。
- **退出询问 = `drop` 掉 `oneshot::Sender`**（今天的 `decline`，[:2462-2463](../../../src/render/tui.rs)）：
  工具读到 [`src/render/input.rs:295`](../../../src/render/input.rs) 那句「这份问卷没被作答就搁下了」，
  **模型继续跑**，当前回合**不**中止。
- **`Ctrl-C` 不变**：问卷里第一下 `Cancel` + 举手，第二下 `Quit`、退出码 130
  （[:1462-1466](../../../src/render/tui.rs)、[:1472-1476](../../../src/render/tui.rs)）。
- **两把举手互斥**：举 `Esc` 那把期间按 `Ctrl-C` **不作数**，走 `Ctrl-C` 自己的第一下
  （清旧手 → Cancel → 举起退出手）；反过来也一样。exit-gesture §1「窗口内任意一个键的第二下都退出」
  是**空闲态两键共用一把**的规矩，这里不适用。
- **实现形状**：`exit_deadline: Option<Instant>`（[:595](../../../src/render/tui.rs)）保留为**同一槽位**，
  旁边加一把「这是哪把」的标签（`Gesture { Exit, DeclineQuestion }`），互斥因此是结构性的。
  `exit_gesture_busy: bool`（[:601](../../../src/render/tui.rs)）不动。
- **窗口常量改名成中性名**（今天叫 `EXIT_GESTURE_WINDOW`，[:3504](../../../src/render/tui.rs)，
  测试钉在 [:4884](../../../src/render/tui.rs)），两把手势共用，值仍是 **500ms**，不做配置项。

## 现状（2026-10-02 核实，改前先复核）

- `Esc` 在 `TuiState::key` 里**先于** pending 分支处理（[:2445-2476](../../../src/render/tui.rs)），
  所以问卷的 `press` 从来收不到它。
- 今天问卷立着时的四支：busy 未禁言 → `Cancel`（[:2446-2448](../../../src/render/tui.rs)）；
  busy + 禁言 + 问卷 → `Cancel`（[:2455-2457](../../../src/render/tui.rs)）；
  busy + 禁言 + 无 pending → `GoalStop`（[:2459](../../../src/render/tui.rs)）；
  idle → `decline(pending)`（[:2462-2463](../../../src/render/tui.rs)）。
- **禁言那一支要保护的东西在这里不存在**：退出询问不掐回合（模型继续跑），所以问卷立着时
  禁言与否不该改变 `Esc` 的行为。
- 「任何别的键先清旧举手」（[:2436-2438](../../../src/render/tui.rs)）照旧——`j`/`k`/空格等键
  会清掉已举的那把。
- 被这次**推翻语义**的测试：`escape_still_cancels_the_run_and_never_answers_the_questionnaire`
  （[`tests/ask_user_question_tui.rs:502`](../../../tests/ask_user_question_tui.rs)）。

## 测试

- 改写上面那条 `:502`：新语义是「`Esc` 两下退出询问、**回合不中止**、模型继续跑」；
  第一下之后屏幕状态还没到退出（举手在）。
- 新增：输入区第一下 `Esc` 回选项区（文本不清）**且**举手；超时后第一下作废（直接调 `expire`，
  不等真实时间）。
- 新增：举 `Esc` 期间按 `Ctrl-C` 不作数（走 `Ctrl-C` 第一下）；举 `Ctrl-C` 期间按 `Esc` 同规矩。
- 新增：退出询问之后工具读到「没作答」那条 `Err`，且**没有** `Aborted`。
- `cargo test` 全绿、`cargo clippy --all-targets` 干净。

## 不做什么

- **不动问卷外**的 `Esc`（取消）、`Ctrl-C`、`Ctrl-D`、模式循环、`/quit`。
- 不动 [`exit-gesture/spec.md`](../../exit-gesture/spec.md) 本身（两把手势的关系记在 spec 与 ADR 里）。
- **页脚的回执显示留给票 04**：本票只保证状态与分派正确。
- 不做「给模型的合成跳过」（spec §5 已否决）。

## Comments

- **落地（2026-10-02）**：新增 `Gesture { Exit, DeclineQuestion }` 与 `TuiState::exit_gesture`
  标签，举手仍是**同一个槽位**（`exit_deadline`），所以两把的互斥是结构性的；`raise_exit_gesture_at`
  保留为「空闲里举退出手」的入口（重放与 `tests/render_tui.rs` 继续用它），新增
  `raise_gesture_at(now, gesture)` 与 `raised_gesture(now)`。常量 `EXIT_GESTURE_WINDOW` 改名
  `GESTURE_WINDOW`（两把手共用，值仍 500ms）。
- **`Esc` 分派**：问卷立着时先走新的 `questionnaire_escape()` —— 输入区那一下回选项区（文本不清）
  **并且**举手，举手期间第二下 `decline()`（drop sender）再作废槽位。它不再按 busy / idle /
  禁言分叉。`key()` 开头那条「别的键先清旧举手」把「问卷立着时的 `Esc`」排除在外，否则第二下
  会被自己清掉。
- **跨键不作数**：`exit_key` 只认 `Gesture::Exit`（举着「退出询问」时按 `Ctrl-C` 走第一下）；
  `status_line` 同理，只认退出手 —— 问卷的举手由它自己的页脚说（票 04）。
- **先红后绿**：新增四条（双击退出询问且不取消运行、输入区那一下回选项区并举手、超时作废、
  两把互斥），并删掉旧的那条「`Esc` 取消这次运行」——那是这次被推翻的语义。
- **验收**：`cargo test` 全绿（问卷 TUI 那个文件 27 条）、`cargo clippy --all-targets` 干净。

- **review 补记（2026-10-02）**：`questionnaire_escape` 原来无条件把区域置回 `Options`，于是
  **没有选项的题**按一下 `Esc` 之后卡在一个不存在的选项区里 —— 光标消失、也再打不进字，只能
  `Tab`/`→` 逃走（spec §1 说的是「没有选项的题 `Zone` 恒为 `Input`」）。改用 `reset_zone()`
  （有选项才回选项区），并补了一条回归测试；把修复临时改回去时那条测试确实红。
