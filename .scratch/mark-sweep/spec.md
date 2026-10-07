# 标记上的扫光

Status: 1/1 done（一次五问的 grilling，不建 wayfinder 图）

- **来源**：用户口头的需求「我想给这个 logo 添加一个动画，类似镜子反光，大概是从右下到左上，
  有一束光线扫过」+ 一轮五问（触发时机 / 光的形态 / 峰值颜色 / 记不记 ADR / 走不走图）。
- **术语**：`CONTEXT.md`（§渲染）新增 **扫光（LightSweep）**。
- **与旧决议的关系**：**推翻** [`.scratch/tui-input-pulse/spec.md`](../tui-input-pulse/spec.md) §2 与它票 08 的
  「标记静止」（`CONTEXT.md` 的**下落短横**词条里那句「别把它想回来」随之作废），理由与代价
  记在 [ADR 0015](../../docs/adr/0015-mark-light-sweep.md)。
- **落点**：`src/render/tui.rs`（`mark_sweep` / `mark_lines` / `paint_mark`）、
  `src/render/palette.rs`（`MARK_LIGHT`）、`tests/render_layout.rs`、`docs/render.md`、
  `docs/tui-manual-checklist.md`、`CONTEXT.md`。

## 问题陈述

1. **左栏那块标记是一幅静止的画。** 它立在那里说明「我是谁」，但一次运行正在进行时，界面上
   说这件事的只有主列的提示符色相与状态行的月相 —— 左栏没有份。
2. **要的是反光，不是又一次亮灭。** 用户点名的形态是「镜子反光，从右下到左上有一束光线扫
   过」：它得有**方向**、有**掠过**的意思，而不能是整块明暗呼吸。

## 决定

### §1 只在一次运行进行中扫

`sweep` 由 `TuiState::busy()` 与**脉冲**那一个计数器拼出来（`state.busy().then_some(state.pulse)`），
不为它另起时钟。`RunState` 的两个边沿都把计数器归零，所以一次运行总从光带在右下角进场那一
刻开始扫；空闲时 `None`，整块逐格不动。

### §2 几何与参数

投影 `s = (columns − 1 − column) + (rows − 1 − row)`：右下角 0、左上角最大，等值线垂直于光走
的方向。光带头 `head = -SWEEP_HALO + (frame % SWEEP_PERIOD) * SWEEP_STEP`，一格的距离
`s − head` 落在核心（±2）里是 `MARK_LIGHT`（白）、落在过渡（±6）里是 `MARK_BRIGHT`、否则不
上色（保持基线）。参数：`SWEEP_STEP = 3`、`SWEEP_PERIOD = 30`（60 ms 一帧 → **1.8 秒一轮**，
其中约 1.3 秒在扫、其余是两轮之间的空档）、`SWEEP_CORE = 2`、`SWEEP_HALO = 6`。

### §3 谁吃这束光

**只有非空格的格**。留白格改了颜色也没人看得见，却会让每一帧在逐格比对的测试里多出一段噪声。
拼音行照吃 —— 它从 `MARK_DIM` 被抬到亮档，于是光带的前后沿在那条细字上也有交代。

## 测试决定

- `the_light_sweeps_from_the_bottom_right_to_the_top_left`（AFK）：纯函数逐帧看白格的投影 ——
  前沿单调不减、整轮里确实从接近 0 走到接近最大值、每一帧的白格聚在一段连续投影里（不像乱扫）。
- `the_sweep_shows_up_on_the_mark_only_while_a_run_is_in_flight`（AFK）：空闲跑满一轮，标记里
  一格白都没有；运行中跑满一轮，某一帧必然有白格，且**第一帧出现的那批**在右下角附近（投影 < 12）。
- `the_mark_holds_still_while_nothing_runs`（AFK）：替换掉 `the_mark_does_not_move_while_a_run_is_in_flight`
  —— 空闲 12 帧，字形与基线色都不变。
- `a_pulse_frame_touches_the_prompt_and_the_status_glyph_and_nothing_else`：按排版给出的左栏矩形
  把标记筛掉，主列那一侧仍然恰好是提示符与状态字形。

## 明确不做

- **空闲时不扫。** 空闲的动必须说得出一件事，这里没有。
- **不做拖尾、不做多次反射、不做随机的光斑。** 一次运行里反复扫同一条斜带就够了。
- **不给它配置项。** 它是一个视觉决定，不是偏好；真机上看下来不对就改常数。
