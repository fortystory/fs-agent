# 忙碌信号换成「转动的短横」，颜色环退到屏幕外

Type: implement
Status: done
Blocked by: 04

> 规格：`.scratch/tui-input-pulse/spec.md` §2（本次改写它）、§3、`Testing Decisions`。
> 来源：用户看过真机之后的第二句话 —— 「这次的颜色变换也不是很好看，变的很突然。先不让它变色了，代码留着。换一种方式，让 `fs-agent` 中的 `-` 旋转起来表示正在工作」。

## 目标

`fs-agent` 里那个短横**转起来**表示「正在工作」；颜色环**从屏幕上撤下来但代码留着**。

## 具体行为

1. **`DASH_TURN: [char; 4]`**（`wording.rs`）：短横的四个朝向，按顺时针 `['─', '╲', '│', '╱']`。用 box-drawing 而不是 ASCII `- \ | /`：对角是真对角、四个字形同一条基线，转起来不抖。
2. **`identity()` 不动**，新增 `identity_turning(phase)`：从 `identity()` 里把那个短横换成 `DASH_TURN[phase % 4]` —— 前缀与版本号只有一份拼法（启动检查脚本锚在这条串上）。
3. **mark 里那个短横也转**：`mark_lines(turning: Option<usize>)`。mark 拼的是 `fs-agent`，短横占第 10..=13 列、五行里那一格（现在是静止的 `▀▀▀▀`）。四个朝向各占的格子写在一张表里：`─` 是中间行四点、`╲` 与 `╱` 是对角四点、`│` 是中间列直立。**静止时就是 `─` 那一档**，所以「空闲的 mark」除短横字形从半块 `▀▀▀▀` 换成线 `────` 之外没有任何变化。
4. **颜色环退到屏幕外**：忙碌时 mark 不再变色 —— 静止的品红渐变照旧（上 4 行 `LightMagenta`、底行 `Magenta`）。`PULSE_PALETTE`、`PULSE_FRAME`、`tick()`、`select!` 那条守卫臂、运行结束归零**全部保留**，palette 的 rustdoc 写明「现在不在屏幕上，留着备用；色相环那条不闪的性质仍有测试盯着」。
5. **节拍 `PULSE_FRAME = 250ms`**：四个朝向每 250ms 换一个，转一圈 1 秒。
6. **两档都看得到**：宽档转 mark 里的短横，窄档（`80 ≤ w < 120`）转文字身份行 `fs-agent 0.1.0` 里的短横。无左栏（`w < 80`）仍然什么都没有。
7. **机制不动**：`pulse` 计数器、`if state.busy()` 守卫、`MissedTickBehavior::Delay`、归零规则一个字节不改 —— 这次只改「脉冲驱动什么」。

## 测试

`tests/render_layout.rs`：

1. `the_dash_turns_while_a_run_is_in_flight`：120×24 忙碌 + tick 1..=8，逐帧断言 mark 第 10..=13 列那几格是 `DASH_TURN[frame % 4]`（按 `DASH_CELLS` 的表逐格读），并且**mark 的颜色不随帧变化**（上 4 行 `LightMagenta`、底行 `Magenta` —— 这条是「不再变色」的回归断言）。
2. **窄档也转**：100×24 忙碌 + tick，文字身份行是 `fs{X}agent {版本}`，`X` 随帧变；空闲时那一行仍然是 `fs-agent 0.1.0`（启动检查脚本的锚点）。
3. `the_pulse_is_invisible_where_there_is_no_sidebar`：60×24 与 40×10 两帧逐格相等。
4. 既有的归零测试改成「短横的朝向」口径：跑 3 帧 → 朝向 3；运行结束 → 回到静止的横；下一轮第 1 帧 → 朝向 1。
5. `the_colour_ring_is_kept_off_screen_and_flicker_free`：palette 仍是 6 项、每项都是 `Light*`、且**屏幕上没有任何一个格子用它**（留着的代码不许偷偷回屏）。

`tests/wording.rs`：

6. `identity_turning` 四个朝向各差一个字符，且都是 `fs-agent <版本>` 的形状（前缀与版本号与 `identity()` 一致）。

## 文档

1. **spec 回改**：§2 改写成「忙碌信号 = 转动短横」，颜色环标为「已退出屏幕、代码保留」；用户故事改成「我想让短横转起来」。
2. `README.md`、`docs/render.md`、`CONTEXT.md`、`docs/tui-manual-checklist.md` ⑯ 同步；`CONTEXT.md` 新增词条 **转动短横（TurningDash）**，**忙碌脉冲（Pulse）**改成「那个帧计数器，它驱动短横的朝向」。
3. 票 02 / 03 / 04 的 Comments 是历史记录，不改写。

## 不做什么

删掉色环代码（用户明确要留）；给两个渲染器（plain / headless）加任何东西；给短横加配置项；改脉冲的机制。

## Comments

**实现完成（2026-09-26）**。落点：`src/render/wording.rs`、`src/render/tui.rs`、`tests/render_layout.rs`、`tests/wording.rs`、`README.md`、`docs/render.md`、`CONTEXT.md`、`docs/tui-manual-checklist.md`、`.scratch/tui-input-pulse/spec.md`（§2 重写 + 用户故事 7–14 + Testing Decisions + Out of Scope）。

1. **短横的四个朝向**：`wording::DASH_TURN = ['─', '╲', '│', '╱']`（顺时针，从静止的平横起）+ `identity_turning(phase)` —— 后者从 `identity()` 里换掉那一个短横，所以前缀与版本号只有一份拼法（启动检查脚本锚在那条串上）。用 box-drawing 而非 ASCII `- \ | /`：对角是真对角、四个字形同一条基线。
2. **mark 那一格**：mark 拼的是 `fs-agent`（八个 4 列字形格、中间隔一列空格），短横是第三格（第 10…13 列、五行里那一格）。`mark_lines(turning: Option<u64>)` 先把那格清空再按一张四朝向的格子表画线：`─` 平铺中行、`╲`/`╱` 走对角、`│` 立中间列。4 宽 5 高的格子里，四格长的线没法真的绕一点转 —— 所以让**长度不变、中点留着**，读起来是「一根针在转」。**接受的代价**：空闲的 mark 里那一格从半块 `▀▀▀▀` 换成线 `────`（四个朝向要用同一族字形）。
3. **两档都转**：宽档转 mark 里那一格，窄档（80–119 列）转文字身份行里那一格；`w < 80` 无左栏、仍然没有信号。
4. **颜色撤出屏幕**：忙碌时 mark 保持静止渐变（上 4 行 `LightMagenta`、底行 `Magenta`）。`PULSE_PALETTE` 6 项原样保留（用户要求「代码留着」），rustdoc 写明它为什么在屏幕外，并**新增一条测试盯着它没有偷偷回屏**（174×50 的忙碌帧里逐格断言没有任何 ring 色）。`PULSE_FRAME` 400ms → **250ms**（四个朝向、1 秒一圈）。机制（`pulse`、`if state.busy()` 守卫、`MissedTickBehavior::Delay`、归零）一个字节没动。
5. **测试**：`the_dash_of_fs_agent_turns_while_a_run_is_in_flight`（四朝向的**形状写成四段字符画**、两圈回到原点、同时逐帧盯住 mark 颜色不变）、`the_narrow_rungs_text_identity_turns_its_dash_too`（四个朝向各出现一次、且不会同时出现两种拼法）、`a_finished_run_puts_the_dash_back_to_still`、`the_pulse_is_invisible_where_there_is_no_sidebar`（60×24 与 40×10 两帧逐格相等）、`the_colour_ring_is_kept_off_screen`、`tests/wording.rs` 的 `the_identities_dash_turns_without_moving_anything_else`。旧的色环测试（变色、归零按颜色）整段删除。
6. **文档**：README 的界面帧**重新 dump**（本票之后空闲 mark 里那一格是 `────`），左栏那段改成「短横转、颜色不参与」；`docs/render.md` 的 mark 段与「定时器只为脉冲存在」那句改写；`CONTEXT.md` 新增 **转动短横（TurningDash）** 词条、**忙碌脉冲（Pulse）** 改成「那个帧计数器，它的消费者是短横的朝向；色环已退出屏幕、不要再接回渲染路径」；手工清单 ⑯ 的 4–9 条改成转动口径（新增「颜色不动」与「窄档也转」两条）。票 02/03/04 的 Comments 未改写。
7. **基线**：`cargo test` **731 passed / 0 failed**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移。
