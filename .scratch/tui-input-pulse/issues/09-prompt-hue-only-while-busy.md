# 提示符只在 agent 工作时变色

Type: implement
Status: done
Blocked by: 08

> 规格：`.scratch/tui-input-pulse/spec.md` §2b（本次改写它）。
> 来源：用户看过真机之后的第五句话 —— 「优化下，在 agent 工作时才变色，在用户输入时不变色」。

## 目标

提示符的颜色**只在一次运行在飞时**走；轮到用户打字时它**停在静止色**，而且空闲时那台时钟**再次不存在**（票 08 那次「常开」的反转收回去）。

## 具体行为

1. **时钟重新带守卫**：`select!` 那条臂恢复 `if state.busy()`；`tick()` 内部也恢复忙碌判断（两条都要：前者保证空闲时不唤醒循环，后者保证一次误调也推不动它）。
2. **静止色 = 第 0 帧**：`RunState { running: false }` 时把 `pulse` **归零**（票 08 删掉的那条回来），所以空闲时提示符恒为脚本第 0 帧那个颜色 `(216, 97, 97)`；每次运行都从这个颜色开始走。**这是本轮唯一的取舍**：换成「停在运行时最后一色」（不归零）也只要删一行，但那样静止色每次都不一样，认不出「这是它休息的样子」。
3. **绘制的条件不变**：提示符那一格**始终**上色（只是空闲时颜色不动），正文仍然不上色。
4. 别的一个字节不改：颜色公式、60ms 帧长、`prompt_colour` 的纯函数形状、mark 静止、色环与下落仍在屏幕外。

## 测试

1. `tests/render_tui.rs`：`tick()` 只在 `running` 时推进 / 置脏（票 08 那条用例反向改回来，注释写清为什么来回：东西在屏幕上的时间变了）。
2. `tests/render_layout.rs`：
   - **静止**：空闲连拍若干帧（含 `tick()` 若干次），提示符那一格的 fg **逐帧相同**，且等于第 0 帧的颜色 —— 这条就是用户这次的诉求。
   - **动起来**：`RunState { running: true }` 后 tick 两三次，那一格每帧不同。
   - **一帧只动那两格**那条改成在**忙碌**状态下断言；空闲状态下连两帧**逐格相等**（整屏都不动）。
   - 归零：跑几帧 → 运行结束 → 提示符回到第 0 帧的颜色；再跑一个回合 → 又从第 0 帧开始。
3. `cargo test` / `cargo clippy --all-targets` / `cargo fmt --check` 基线同前。

## 文档

1. **spec 回改**：§2b 的「时钟常开」那条改回「只在运行在飞时 arm」，并把票 08 → 09 的来回写清楚（东西在屏幕上的时间决定了时钟该开多久）；用户故事补「轮到打字时不要动」。
2. `CONTEXT.md`（**提示符色相** / **脉冲** 两条）、`README.md`、`docs/render.md`、手工清单 ⑯ 第 4 / 8 / 9 条同步。
3. 票 08 的 Comments 是历史记录，不改写。

## 不做什么

改颜色公式；让静止色可配置；mark 的任何动法；把提示符在空闲时画成默认前景色（那会变成「有颜色 == 在工作」的另一种读法，用户没要）。

## Comments

**实现完成（2026-09-26）**。落点：`src/render/tui.rs`、`tests/render_layout.rs`、`tests/render_tui.rs`、`README.md`、`docs/render.md`、`CONTEXT.md`、`docs/tui-manual-checklist.md`、`.scratch/tui-input-pulse/spec.md`（§2b 与用户故事）。

1. **时钟重新带守卫**：`select!` 那条臂恢复 `if state.busy()`，`tick()` 内部恢复忙碌判断（前者保证空闲不唤醒循环，后者保证误调也推不动）；`RunState { running: false }` 恢复**归零**。于是空闲时提示符恒为第 0 帧的 `(216, 97, 97)`，每次运行从它开始走 —— **这是本轮唯一的取舍**（不归零则静止色每次不同，认不出「休息的样子」），写进了票与 spec。
2. **绘制零改动**：提示符那一格仍然始终上色（只是空闲时颜色不动），正文仍然不上色，颜色公式与 60ms 帧长一个字节没动。
3. **测试**：`the_prompt_is_an_angle_bracket_that_holds_still_while_you_type`（空闲 tick 五次，那一格 fg 逐次相同且等于第 0 帧）、`the_prompts_colour_walks_the_wheel_while_a_run_is_in_flight`（忙碌五帧各不相同、都不等于静止色；结束回静止；下一轮的第一帧与上一轮的第一帧相同）、`a_pulse_frame_touches_the_prompt_and_nothing_else`（三档尺寸，忙碌下变化的格子恰好是提示符那两格）；`tests/render_tui.rs` 那条反向改回「只在运行时推进」。**736 passed / 0 failed**。
4. **文档**：`CONTEXT.md` 的**提示符色相**补「只在一次运行在飞时动、静止色是第 0 帧」，**脉冲**改回「空闲时那台时钟不存在」（并注明票 08 的常开已收回）；`docs/render.md` 的前端那一段改回「带 `if state.busy()` 守卫」；README 那句写成「干活时走色、轮到你自己打字时停住」；手工清单 ⑯ 第 4 条改成**「空闲静止 / 发回合才走 / 结束回到静止」**，第 8 条改回「空闲不烧 CPU」，第 9 条写明「结束回静止是有意的一次切换」。README 的界面帧不用重 dump（空闲提示符正好是第 0 帧的颜色，与帧里那两格一致）。
5. **基线**：`cargo test` **736 passed / 0 failed**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移。
