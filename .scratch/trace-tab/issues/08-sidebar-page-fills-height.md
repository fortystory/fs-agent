# 08 — 左栏页高撑满

Type: implement
Status: done
Part of: ../map.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §4（左栏页高）。实测帧在 [`../prototype/frames.txt`](../prototype/frames.txt)（`python3 .scratch/trace-tab/prototype/frames.py` 可重跑）。

## 目标

左栏页区高度 = **内容行 − 身份行 − 页签条**，内容贴顶。用户看得见：轨迹页从 6 行变成 **15 / 19 行**；极矮终端（120×10 / 80×10）那行**文字身份回来了**。

## 现状（改前先复核）

- 页区高度今天直接取「用量字段数」（恒 6、地板 3）—— 那是给调用量页量身定的耦合，其余页跟着受害。
- 调用量页不看高度、最多返 6 行并从顶部画；todo 页**已经**按页高自适应（计数行占最后一行、条目拿剩下的、塞不下才画一行 `＋N 项`）。
- 极矮档的实测：今天身份被阶梯让掉换 6 行页；改成页地板 3 之后身份保住、页区 5 行（调用量丢「缓存」—— 那正是它字段序里第一个丢的）。

## 落点

`src/render/layout.rs`。

## 具体行为

1. 页区高度 = `content_rows − kind.rows() − TAB_ROWS`，其中 `content_rows = area.height − SIDEBAR_TOP_GAP`。
2. 「用量字段数」那个常数**退休**；阶梯判据改成「身份 + 页签条 + 3 ≤ 内容行」，地板常数改名成「页的地板」。
3. 调用量页与 todo 页零代码改动。
4. 页签条的 y 与命中矩形、顶留白、宽度两档、`Ctrl-O` 意愿那一层都不动。

## 验证

1. 帧断言三档：**120×24 页区 15 行**、**80×24 19 行**、**80×10 5 行且文字身份行在**（今天它不在）。
2. 调用量页在 15 行里仍是贴顶 6 行；todo 页在 15 行里画得下更多条目（不再画 `＋N 项`）。
3. `cargo test` / `cargo clippy` / `cargo fmt --check`。

## 不做什么

- 不改左栏宽度、档位门槛与 `Ctrl-O` 的行为。
- 不给轨迹页写任何内容 —— 它此时还是占位符。

## 作答（2026-10-05）

- `src/render/layout.rs`：`SIDEBAR_FIELDS = 6` 退休；`SIDEBAR_MIN_FIELDS` 改成
  `SIDEBAR_MIN_PAGE_ROWS = 3`（页的地板）；`sidebar_content` 的判据改成
  `kind.rows() + TAB_ROWS + SIDEBAR_MIN_PAGE_ROWS <= content_rows`，返回的第二个值从
  「字段数」变成「页区行数 = 内容行 − 身份 − 页签条」。`plan` 与 `Regions::sidebar_page`
  的注释跟着改。调用量页与 todo 页**零代码改动**。
- 新增三条帧/几何断言：`the_sidebar_page_fills_the_height_the_identity_and_tabs_leave`
  （120×24 → 15、80×24 → 19、80×10 → 5）、`the_tiny_terminal_keeps_its_text_identity`、
  `the_usage_page_still_draws_its_six_fields_from_the_top`、
  `the_todo_page_fits_more_items_now_that_the_page_fills_the_height`（12 项全部画下、无溢出行）。
- 两条既有测试随新阶梯改写：`the_sidebar_gives_up_its_identity_then_its_fields_as_it_shrinks`
  → `the_sidebar_gives_up_its_identity_before_the_page_floor`（12 行正好是页地板、11 行让出标记
  换回 6 行页）；`the_todo_page_shows_what_fits_then_says_how_many_more_there_are` 的项数从
  12 提到 30（页高 15 之后 12 项装得下了）。
- `cargo test` 全绿、`cargo clippy --all-targets` 无警告、`cargo fmt --check` 干净、
  `python3 scripts/tui-startup-check.py` 仍是 0/15 red。
