# 05 — 选项折行，窗口改按渲染行数算

Type: implement
Status: ready-for-agent
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
