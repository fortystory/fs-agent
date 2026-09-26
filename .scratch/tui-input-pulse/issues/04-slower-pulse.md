# 脉冲改慢、去掉明暗交替

Type: implement
Status: done
Blocked by: 01, 02

> 规格：`.scratch/tui-input-pulse/spec.md` §2（本次改写它）、`Testing Decisions`。
> 来源：用户看过真机之后的一句话 —— 「这个 mark 闪烁的太快了，只变色就行不用闪烁」。

## 目标

让忙碌脉冲读起来**只是在换颜色**，而不是**在闪**。

## 问题

票 02 的色环是「六个色相 × 明/暗两支 = 12 帧、100ms 一帧」。两个毛病叠在一起：

1. **明暗交替**：亮品红 → 品红 → 亮蓝 → 蓝 …… 每 100ms 亮度跳一次，读起来就是闪；
2. **太快**：整圈 1.2 秒，色相每 200ms 换一次，眼睛来不及跟。

## 具体行为

1. **色环只留亮色相**：`PULSE_PALETTE` 从 12 项收成 **6 项** —— `LightMagenta, LightBlue, LightCyan, LightGreen, LightYellow, LightRed`（绕色环、不再有明暗两支）。第 0 帧仍是 `LightMagenta`（空闲态顶部色），所以从空闲切进忙碌的第一帧照样不跳色。
2. **节拍改 `PULSE_FRAME = 400ms`**：一圈 **2.4 秒**，每帧换一个色相 —— 比「安静」还安静一档，但仍在动。
3. **注释与 rustdoc 跟着改**：`PULSE_PALETTE` 的「六个色相 × 明/暗」、`PULSE_FRAME` 的「每秒十帧 / 1.2 秒一圈」、`mark_lines` 与本模块顶部的说法，一处都不能留着旧数字。
4. **机制一个字节不动**：`select!` 里那条带 `if state.busy()` 守卫的 `interval`、运行结束归零、窄屏不画 —— 全都照旧。这次只改颜色表与节拍。

## 测试

1. `the_mark_walks_the_pulse_ring_while_a_run_is_in_flight`：改成两圈共 `PULSE_PALETTE.len() * 2 = 12` 帧，逐帧断言 5 行同色且等于 `PULSE_PALETTE[frame % 6]`；**新增一条**：色环里**没有暗色**（每一项都是 `Color::Light*`）—— 那就是「不闪」的回归断言。
2. `a_finished_run_puts_the_pulse_back_at_the_rings_first_frame`：中途帧改用环上真的存在的一帧（3 → 仍是 `PULSE_PALETTE[3]`，现在落在亮绿）。
3. 既有的「空闲渐变更亮在下」与「窄屏不画不变」两条不动，跑一遍确认。
4. `cargo test` 全绿；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只留既有漂移。

## 文档

1. **spec 回改**：`.scratch/tui-input-pulse/spec.md` §2 与用户故事 8 改成 6 帧 / 400ms / 2.4 秒，并写明**为什么不再有明暗两支**（那一版读起来是在闪）。
2. `README.md`、`docs/render.md`、`CONTEXT.md`、`docs/tui-manual-checklist.md` ⑯ 里的帧数、间隔、圈时长同步改掉；手工清单里补一条「色相换的时候亮度不应跳」。
3. 票 02 / 03 的 Comments 是历史记录，**不改写**。

## 不做什么

改机制（守卫、归零、窄屏不画）；24 位真彩平滑渐变（用户没选）；窄档身份行跟着变色。

## Comments

**实现完成（2026-09-26）**。落点：`src/render/tui.rs`、`tests/render_layout.rs`、`README.md`、`docs/render.md`、`CONTEXT.md`、`docs/tui-manual-checklist.md`、`.scratch/tui-input-pulse/spec.md`（回改 §2 与用户故事 8）。

1. **`PULSE_PALETTE` 12 → 6 项，只留亮色相**：`LightMagenta, LightBlue, LightCyan, LightGreen, LightYellow, LightRed`。第 0 帧仍是 `LightMagenta`，所以从空闲切进忙碌照样不跳色。rustdoc 里把「第一版为什么读成闪」写下来了 —— 明暗交替时亮度每帧跳一档，眼睛跟的是亮度不是色相。
2. **`PULSE_FRAME` 100ms → 400ms**：一圈 2.4 秒、每帧一个色相。机制一个字节没动（`select!` 那条带 `if state.busy()` 守卫的 `interval`、运行结束归零、窄屏不画、`MissedTickBehavior::Delay`）。
3. **测试**：`the_mark_walks_the_pulse_ring_while_a_run_is_in_flight` 改成两圈 12 帧，并**新增一条回归断言** —— 环上每一项都必须是 `Light*`（「不闪」这条性质从此有人盯着，往环里塞暗色会红）。既有的「空闲渐变更亮在下」「窄屏两帧逐格相等」「归零」三条不动，全绿。
4. **文档**：`README.md`（6 帧 / 400ms / 2.4 秒，并点明不带明暗交替）、`docs/render.md`（帧数与理由）、`CONTEXT.md`（**不要给它加明暗交替** 写成词条里的显式警告）、手工清单 ⑯ 第 4 条改成「像在变色而不是在闪，每帧亮度应当看起来同一档」、第 5/6 条的数字跟着改。票 02 / 03 的 Comments 是历史记录，未改写。
5. **基线**：`cargo test` **728 passed / 0 failed**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移。
