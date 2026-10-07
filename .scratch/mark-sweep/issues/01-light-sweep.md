# 标记上的扫光：运行中一束反光从右下扫到左上

Type: implement
Status: done
Blocked by: —

> 规格：`.scratch/mark-sweep/spec.md`（§1–§3）。
> 来源：用户口头的需求 —— 「我想给这个 logo 添加一个动画，类似镜子反光，大概是从右下到左上，有一束光线扫过」，加一轮五问（只在运行中扫 / 斜亮带加过渡 / 峰值白 / 记 ADR / 本 session 做完）。

## 目标

一次运行进行中，左栏那块小篆标记上有一束反光从**右下**扫到**左上**；空闲时整块一个像素都
不动。

## 具体行为

1. **几何**（`src/render/tui.rs` 的 `mark_sweep`）：`s = (columns − 1 − column) + (rows − 1 − row)`，
   右下角 0；`head = -SWEEP_HALO + (frame % SWEEP_PERIOD) * SWEEP_STEP`；距离 ≤ `SWEEP_CORE` 是
   `palette::MARK_LIGHT`（白）、≤ `SWEEP_HALO` 是 `palette::MARK_BRIGHT`、否则不上色。
   参数 `STEP = 3` / `PERIOD = 30` / `CORE = 2` / `HALO = 6`（60 ms 一帧 → 1.8 秒一轮）。
2. **只在运行中**：`draw_sidebar_identity` 把 `state.busy().then_some(state.pulse)` 传下去；
   `RunState` 两个边沿都归零计数器，所以一轮运行总从右下角进场。
3. **上色**：`mark_lines` 返回**每行的若干段**（连续同色合并），`paint_mark` 逐格算；**留白格
   不吃扫光**（没有字形，改了也看不见，只给测试添噪声）。
4. **色板**：新增 `palette::MARK_LIGHT = Color::White`，注释写明「洋红族里没有更亮的档，反光
   只能过曝成白」。
5. **测试**：`the_light_sweeps_from_the_bottom_right_to_the_top_left`、
   `the_sweep_shows_up_on_the_mark_only_while_a_run_is_in_flight`、
   `the_mark_holds_still_while_nothing_runs`（替换掉 `the_mark_does_not_move_while_a_run_is_in_flight`）；
   `a_pulse_frame_touches_the_prompt_and_the_status_glyph_and_nothing_else` 按左栏矩形筛掉标记。

## 落地记录

- `src/render/tui.rs`：`mark_sweep`（pub，纯函数）、`SWEEP_PERIOD`（pub，测试要遍历一轮）、
  `SWEEP_STEP` / `SWEEP_CORE` / `SWEEP_HALO`（私有）、`mark_lines(kind, sweep)`、
  `paint_mark(rows, preamble, sweep)`、`draw_sidebar_identity` 多收一个 `&TuiState`。
- `src/render/palette.rs`：`MARK_LIGHT`。
- `tests/render_layout.rs`：三条新用例 + 一条改写；`sweep_progress` / `lit_at` 两个助手与画家
  共用同一份投影算术。
- 文档：`docs/adr/0015-mark-light-sweep.md`、`docs/render.md`、`docs/tui-manual-checklist.md`、
  `CONTEXT.md`（新增**扫光（LightSweep）**）。
- 真机：`cargo test` 全绿、`cargo clippy --all-targets` 干净、`scripts/tui-startup-check.py`
  15/15 绿；扫光的几帧另画成图对过（右下进场、斜带、左上出场）。
