# 05 — 选项折行，窗口改按渲染行数算

Type: implement
Status: done
Blocked by: 04

> 来源：[`../spec.md`](../spec.md) §7。它是 10-01 那批里的第三条意向，与区域模型正交，
> 但落在同一片绘制代码里，所以排在这条链上。

## 目标

- **选项与自定义行都折行**（今天 `truncate_columns`，[:2904](../../../src/render/tui.rs)、
  [:2917](../../../src/render/tui.rs)），折行宽度按渲染时的可用列宽算。
- **窗口从「选项条数」改成「渲染行数」**：`option_window_start`（[:2832-2842](../../../src/render/tui.rs)）
  与 `draw_questionnaire` 里重复的那一遍计算（[:3789-3802](../../../src/render/tui.rs)）
  **一起改**——今天两处都假设「一个选项占一行」。
- **高亮项保证完整可见**；它自己就超过可视高度时，从它的**头部**开始画。
- **注释一起改**：[:2804](../../../src/render/tui.rs)、[:2830-2831](../../../src/render/tui.rs)
  明写着「一个选项占一行」这个前提，改完代码留着那两句就是错的。

## 现状（2026-10-02 核实，改前先复核）

- `questionnaire_parts`（[:2848-2920](../../../src/render/tui.rs)）把每个选项压成**一个 `Line`**，
  文本走 `truncate_columns(&text, width)`；header / question 已经走 `pane::wrap_text`
  （[:2860](../../../src/render/tui.rs)、[:2871](../../../src/render/tui.rs)）。
- 滚动窗口：`room = height-(prefix+1)`（[:2821](../../../src/render/tui.rs)、[:3797](../../../src/render/tui.rs)），
  `start = if highlight < room { 0 } else { highlight + 1 - room }` 再 clamp 到 `count - room`
  （[:2836-2841](../../../src/render/tui.rs)）。
- 票 01 之后 `highlight` 只在选项区里，本票不动这条语义。
- 受影响的帧测试：`tests/ask_user_question_tui.rs:339`（选项窗口滚动）。

## 测试

- [`tests/render_layout.rs`](../../../tests/render_layout.rs)：一条长到折成三行的选项，
  在高亮落到它上面时**三行都在窗口里**（而不是只剩第一行）；选项多于可视高度时窗口跟着高亮走。
- `tests/ask_user_question_tui.rs:339` 那条滚动测试跟着新单位改写。
- `cargo test` 全绿、`cargo clippy --all-targets` 干净。

## 不做什么

- 不做「整页滚动」「翻页键」：窗口跟着高亮走，这是今天的模型，只换单位。
- 不动区域与键位（票 01–03）、不动页脚（票 04）。

## 评论

- **落地（2026-10-02）**：`questionnaire_parts` 改成返回 `Vec<Vec<Line>>`（每个选项一组行），
  新增 `wrap_with_lead` —— 前缀（`> ○ ` 这种）算进折行宽度、续行用等宽空格缩进，所以每一行都
  不超过窗格宽；自定义行走同一条路（空文本仍然占一行，它是输入区）。
- **窗口换成按行算**：`option_window_start` 改收 `heights: &[usize]`，做法是把高亮那一项的底部
  贴在窗口底部、再回退到包含那个行号的选项起点；高亮项自己超过一屏时从它的头部画。
  `questionnaire_window` 与 `draw_questionnaire` 的 regions 都跟着改成行单位——折成几行就登记
  几行，每一行都映射回同一个选项下标。两处「一个选项占一行」的注释一并改掉。
- **先红后绿**：新增两条（长选项整块都在窗口里、折行之后点**第二行**也命中同一个选项）；
  两条都对「退回截断」敏感（截断只剩一行，两条都会红）。
- **验收**：`cargo test` 全绿（问卷 TUI 28 条、render_layout 129 条）、
  `cargo clippy --all-targets` 干净。

- **review 补记（2026-10-02）**：自定义行折了行，但选项窗口仍按「输入区占一行」算预留 ——
  续行会被截掉、装得下那一支还会把多出的行画到窗格外面盖住页脚。`option_window_geometry`
  现在把 `custom_rows` 算进预留，两个分支最后都按 `height` 收口；选项区域登记的底界与光标列
  （改成跟着窗口最后一行）一并跟上。补了一条输入区折行的测试。
