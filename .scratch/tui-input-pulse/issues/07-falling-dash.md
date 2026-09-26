# 忙碌信号改成「横线从上往下落」

Type: implement
Status: done
Blocked by: 06

> 规格：`.scratch/tui-input-pulse/spec.md` §2（本次改写它）。
> 来源：用户看过真机之后的第三句话 —— 「我看这个也不太好看，再换一下，改成 `▀▀▀▀` 这一『横』在从上到下下落，然后再从上面出现，再下落」。

## 目标

忙碌时 mark 里那一格是**同一条 `▀▀▀▀`**，它一行一行往下落：落到底再从上出现，循环。旋转那套四朝向撤掉。

## 具体行为

1. **mark 的短横 = 一条会落下的横线**：那一格有 **5 行**（就是 mark 的五行）。第 `n` 帧它画在第 `n % 5` 行，字形始终是 `▀` × 4 列 —— **形状不变，只有所在的行在变**。落到第 5 行（最下）之后回到第 1 行重新落。
2. **静止时仍是最中间那一行**（第 3 行），也就是 `logo_lines` 自己的 `▀▀▀▀` —— 空闲的 mark 与从前逐字节相同那条回归断言照旧成立（`None` 映射到 `rows.len() / 2`）。
3. **窄档的文字身份行也跟着落**：一行里没有五行可走，所以用三个位置表达同一段落：`DASH_FALL = ['▀', '▀', '█', '▄', '▄']`（高 → 高 → 满 → 低 → 低），下标与 mark 的帧**同一个**（都是 `phase % 5`），两档永远不在同一屏，但相位是同一套。
4. **`DASH_TURN` / `identity_turning` 改名**：`DASH_FALL` / `identity_falling`（四个朝向那套连表一起删掉，不留死代码）。`wording` 的 `identity()` 不动。
5. **机制不动**：`PULSE_FRAME = 250ms`（一落 1.25 秒）、`if state.busy()` 守卫、运行结束归零、`pulse` 计数器、色环仍在屏幕外。

## 测试

`tests/render_layout.rs`：

1. `the_dash_falls_while_a_run_is_in_flight`（原 `the_dash_of_fs_agent_turns_while_a_run_is_in_flight`）：五帧的字符画是「同一行 `▀▀▀▀` 从第 1 行挪到第 5 行」，两圈确认回到第一行；**形状每帧都一样**（这条把「旋转」彻底排除掉）；mark 的颜色全程不变。
2. 静止那条**不动**：空闲那一格仍逐字符等于 `logo_lines` 的第 10…13 列。
3. 窄档：忙碌时身份行是 `fs{X}agent {版本}`，`X` 依次是 `▀ ▀ █ ▄ ▄`；空闲仍是 `fs-agent {版本}`。
4. 归零 / 无左栏 / 色环离屏三条照旧跑绿。

`tests/wording.rs`：`identity_falling` 的五帧只差那一个字符，且都是 `fs-agent <版本>` 的形状。

## 文档

1. **spec 回改**：§2 从「转动」改写成「下落」，术语从 **转动短横（TurningDash）** 改为 **下落短横（FallingDash）**（`CONTEXT.md` 同改）。
2. `docs/render.md`、手工清单 ⑯ 第 4/6 条同步；README 左栏那句改成「一条横线在往下落」。
3. **README 的界面帧不用重 dump**：空闲那一格与现在逐字节相同。
4. 票 05 / 06 的 Comments 是历史记录，不改写。

## 不做什么

加缓冲帧（落到底停一拍）；改转速；回归左右移动 / 旋转 / 颜色；给色环做任何事。

## Comments

**实现完成（2026-09-26）**。落点：`src/render/wording.rs`（`DASH_FALL` / `identity_falling`，`DASH_TURN` / `identity_turning` 删掉）、`src/render/tui.rs`（`DASH_BAR` + `mark_lines` 按行落）、`tests/`、`README.md`、`docs/render.md`、`CONTEXT.md`、`docs/tui-manual-checklist.md`、`.scratch/tui-input-pulse/spec.md`（§2 重写、用户故事 7–15、Testing Decisions、Out of Scope）。

1. **mark 那一格 = 会落的横线**：那一格就是 mark 的五行（第 10…13 列在**每一行**都是空白 —— mark 是八个 4 列字形格、中间隔一列空格，短横独占第三格），所以帧 `n` 把同一行 `▀▀▀▀` 画在第 `n % 5` 行。真机帧已 dump 确认：第 1 行 → 第 2 行 → …… → 第 5 行 → 回第 1 行，**形状每帧都一样**。
2. **静止仍是原来那一行**（`rows.len() / 2` = 第 3 行 = `logo_lines` 自己那四列），所以空闲的 mark 与本轮所有改动之前逐字节相同；那条拿 `logo_lines` 第 10…13 列逐字符比对的回归断言照旧，并且依旧是本票最有价值的那条。
3. **窄档也落**：一行里没有五行可走，所以用 `DASH_FALL = ['▀', '▀', '█', '▄', '▄']`（高、高、满、低、低）表达同一段落，下标与 mark 同一套帧。旧表 `DASH_TURN` 与 `identity_turning`/`DASH_CELLS` **整段删除**，不留死代码。
4. **测试**：五帧字符画 + 两圈回原点 + 「每帧形状相同」+ mark 颜色全程不变；窄档五帧 `fs▀/█/▄/▄/▀agent` 且空闲仍是 `fs-agent`；归零那条改成「短横回到静止行、下一轮从最上面重新落」；无左栏、色环离屏两条照旧。`tests/wording.rs` 的用例改名成 `the_identities_dash_falls_without_moving_anything_else`，除了逐帧只差一个字符之外，还断言**五个高度就是那五个**（旋转或别的字形会失败）。
5. **文档**：`CONTEXT.md` 的 **转动短横（TurningDash）** 改名并改写为 **下落短横（FallingDash）**；`docs/render.md` 的 mark 段改成「同一条横线每帧下移一行」；README 左栏那句改了；手工清单 ⑯ 第 4 条改成「落、形状不变、空闲仍在原行」、第 6 条改成窄档下沉。**README 的帧不用重 dump** —— 空闲那一格与从前逐字节相同。票 05/06 的 Comments 保留为历史。
6. **基线**：`cargo test` **731 passed / 0 failed**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移。
