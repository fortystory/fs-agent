# 下落关掉；提示符改成 `❱` 并让它自己转色

Type: implement
Status: done
Blocked by: 07

> 规格：`.scratch/tui-input-pulse/spec.md` §2（本次改写它，并新增 §2b）。
> 来源：用户看过真机之后的第四句话 —— 「我看这个也不太好看，先不动这个 mark 了，这个下落先关了。我找了个脚本，修改文本输入提示 `>` 的颜色，先这么改吧，后续我再有想法的时候再改」，并附了一段 60fps 的 Python 脚本（HSV 色相每帧 +0.005、饱和度 `0.55 + 0.2·sin(t)` 呼吸、`v = 0.85`、字符 `❱ `）。

## 目标

1. **mark 不再下落**（代码留着），左栏回到完全静止。
2. 输入区的提示符从 `> ` 换成 **`❱ `**，它的**颜色**照用户脚本那样一直转：色相绕圈 + 饱和度呼吸，24 位真彩。

## 具体行为

### A. 下落关掉（mark 静止）

1. `draw_sidebar_identity` 两个分支都不再读 `state.pulse`：mark 传 `mark_lines(None)`、窄档恒用 `wording::identity()`。**mark 与文字身份行都静止**，与动画出现之前逐字节相同。
2. `mark_lines(frame)` / `DASH_BAR` / `wording::DASH_FALL` / `identity_falling` **全部保留**（用户要「先关了」，不是删），rustdoc 写明「现在不在屏幕上，留着备用」，并把下落那套的**逐帧断言搬进 `src/render/tui.rs` 的 `#[cfg(test)] mod tests`**（离开屏幕的代码也得有人看着，不能靠集成测试反过来驱动）。
3. 窄档的 `identity_falling` 同理：`tests/wording.rs` 里那条继续直接打函数（它是 pub 的），只是不再有人从渲染路径调它。

### B. 提示符 `❱ ` 与它的色相环

4. **`editor::PROMPT = "❱ "`**：`❱`（U+2771）在本宽度层里算 **1 列**，所以 `prompt_columns()` 仍是 2，布局预留与光标列算法一个字节不用改。**风险记进文档**：终端若把「模糊宽度」字符按双列画（部分 CJK 配置），提示符会多占一列 —— 真终端里看得见（草稿整体右移一格），手工清单里列一条。
5. **提示符单独成一个 span**：`Input::view` 现在把 `{lead}{text}` 拼成一个字符串；改成 `Line` 的两个 span（`lead` + 正文），画笔才有一处可以上色。字符内容与列数不变（`tests/render_editor.rs` 的 `rows()` 拼回整串，用例不用改）。
6. **颜色**（`tui.rs`，纯函数 `prompt_colour(frame: u64) -> Color`）：
   - 色相每秒走 **0.3**（脚本：每 1/60 秒 +0.005），绕一圈约 **3.3 秒**；
   - 饱和度呼吸每秒 **3.0** 弧度（脚本：每 1/60 秒 +0.05，`sin` 周期约 2.1 秒），`s = 0.55 + 0.2·sin(t)`；
   - `v = 0.85`；HSV→RGB 用与 `colorsys` 同样的取整（截断），所以帧 0 的 RGB 与脚本一致（`(216, 97, 97)`）；
   - 输出 `Color::Rgb(r, g, b)` —— **24 位真彩**，这是用户脚本的本意，也是仓库里第一处非 16 色的颜色。
7. **时钟改成常开**：`PULSE_FRAME` 250ms → **60ms**（约 16 帧/秒，肉眼看是连续变色而不是跳色）；`select!` 那条臂**去掉 `if state.busy()` 守卫**；`RunState { running: false }` 的**归零删掉**，`tick()` 不再判定忙碌 —— 被驱动的那个东西（提示符）在没有运行的时候也在屏幕上。
   **这是对票 02/03「空闲时没有任何定时器」的正式反转**，代价写进 spec：空闲时每 60ms 醒一次、重画 2 个格子（真彩 fg 变了就是脏格），换来的是一条一直在呼吸的提示符。`MissedTickBehavior::Delay` 与 `interval`（而不是每轮重建的 `sleep`）照旧。
8. **`PULSE_PALETTE` 仍留在屏幕外**；它今天的地位与 `DASH_BAR` 一样：留着、有人看着、别偷偷回屏。

## 测试

`tests/render_layout.rs`：

1. 提示符：`120×24` 空闲帧里输入区第一行以 `❱ ` 开头，且**那一格的前景是 `prompt_colour(pulse)`**（`Color::Rgb`），正文那一格的 fg **不是**提示符色（正文用默认色 + 粗体）。
2. 颜色在动：同一份状态 tick 两三次，输入区第一格的 fg 每次都不一样；`prompt_colour(0)` 与脚本帧 0 的 `(216, 97, 97)` 相同。
3. **mark 静止**（原下落用例改写）：`RunState { running: true }` 后 tick 若干次，mark 的五行颜色与短横那一格**逐帧相同**，并且等于 `logo_lines` 的第 10…13 列。
4. **窄档身份行静止**：100×24、忙碌 + tick，身份行恒为 `fs-agent {版本}`。
5. 色环离屏那条照旧。

`tests/render_tui.rs`：`tick()` 现在**无论忙闲都置脏并推进**（改写原「只在运行时」用例），并把「运行结束归零」删掉。

`tests/wording.rs`：`identity_falling` 那条保留（直接打函数），补一句「它今天不在渲染路径上」。

`src/render/tui.rs` 单元测试：下落四帧的形状（`mark_lines(Some(n))` 的短横落在第 `n % 5` 行、形状恒为 `▀▀▀▀`）。

## 文档

1. **spec 回改**：§2 改成「mark 静止 + 下落代码留着」；新增 **§2b 提示符色相**（脚本的公式、常开时钟、真彩、代价）；用户故事补「我想让提示符 `❱ ` 一直在转色」。
2. `CONTEXT.md`：新增 **提示符（Prompt）** 与 **提示符色相（PromptHue）**；**忙碌脉冲（Pulse）** 改成 **脉冲（Pulse）**（它不再只属于忙碌态），**下落短横（FallingDash）** 标注「已退出屏幕」。
3. `README.md`（界面帧要重 dump：提示符那两格变了）、`docs/render.md`、`docs/tui-manual-checklist.md`（删掉下落的条目、补提示符颜色与「模糊宽度终端会不会错位」两条）。
4. 票 05–07 的 Comments 是历史记录，不改写。

## 不做什么

删掉下落 / 色环的代码；给提示符颜色加配置项；改 mark 的静止字形；动 plain / headless。

## Comments

**实现完成（2026-09-26）**。落点：`src/render/editor.rs`、`src/render/tui.rs`、`src/render/wording.rs`（只改 rustdoc）、`tests/render_editor.rs`、`tests/render_layout.rs`、`tests/render_tui.rs`、`README.md`、`docs/render.md`、`CONTEXT.md`、`docs/tui-manual-checklist.md`、`.scratch/tui-input-pulse/spec.md`。

1. **下落关掉**：`draw_sidebar_identity` 传 `mark_lines(None)` 并用 `wording::identity()`；mark 与窄档身份行都静止。下落那套逐帧断言**搬进 `src/render/tui.rs` 的单元测试**（`mark_lines(Some(n))` 每帧落一行、形状恒为 `▀▀▀▀`、算上 `None` 落在 `logo_lines` 那一行），集成侧反过来加了「它没有偷偷回屏」（跑一个回合 tick 若干帧，短横那格与五行颜色逐帧相同）。
2. **提示符 `❱ `**：`editor::PROMPT` 一个常量改掉；`❱`（U+2771）在本宽度层算 1 列，`prompt_columns()` 仍是 2 —— 布局预留、缩进、光标列全都不用动（`prompt_columns() == 2` 有断言盯着）。**模糊宽度**的风险（部分 CJK 终端按双列画）写进 rustdoc 与手工清单，真出现了只改这一个常量。
3. **一处可上色的接缝**：`Input::view` 把每行的引导（提示符 / 等宽缩进）与正文拆成两个 span；`tests/render_editor.rs` 的 `rows()` 拼回整串，用例只需把 `> ` 换成 `P`（= `editor::PROMPT`）。画笔只给**内容等于 `PROMPT` 的那个 span** 上色 —— 缩进是空格，不该跟着染色；草稿滚到中部、提示符不在屏幕上时也就没有颜色。
4. **颜色**：`prompt_colour(frame)` + `hsv_to_rgb`，参数就是用户脚本的**每秒**速率（色相 0.3 圈/秒、呼吸 3.0 弧度/秒、`s = 0.55 + 0.2·sin`、`v = 0.85`），与 `colorsys` 同样截断取整。单元测试拿脚本在 0 / 0.3 / 0.6 / 1.8 秒时的输出逐字节比对（第 5 / 10 / 30 帧 = `(216,146,63)` / `(203,216,55)` / `(131,196,216)`），另有 `hsv_to_rgb` 的单色与色轮闭合。**24 位真彩**是仓库里第一处。
5. **时钟常开**：`PULSE_FRAME` 250ms → **60ms**；`select!` 那条臂去掉 `if state.busy()` 守卫；`tick()` 不再判定忙碌；`RunState { running: false }` 的归零删掉（换色不该每个回合回零）。`tests/render_tui.rs` 的用例反向改写为「无论忙闲都推进」。**代价**写进 spec 与文档：空闲时每 60ms 醒一次、最多重画两个格子 —— 手工清单第 8 条专门让人看一眼这个占用是否仍然很小。
6. **测试**：新用例 `the_prompt_is_an_angle_bracket_whose_colour_walks_the_wheel`、`the_prompts_colour_stays_out_of_the_draft`、`the_only_thing_a_pulse_frame_touches_is_the_prompt`（三档尺寸逐格比对，变化的格子必须**恰好**是提示符那两格）、`the_mark_does_not_move_while_a_run_is_in_flight`、`the_mark_stays_still_on_the_narrow_rung_too`；`tests/render_layout.rs` 里所有写死 `"> "` 的断言改成按 `editor::PROMPT` 取（提示符那格按屏幕搜索定位，不按矩形）。**735 passed / 0 failed**。
7. **文档**：README 的帧**重新 dump**（提示符那两格变了）并把左栏那段改成「界面里唯一在动的是提示符」；`docs/render.md` 改 mark 段与「定时器只为脉冲存在」那段；`CONTEXT.md` 新增 **提示符色相（PromptHue）** 与 **脉冲（Pulse）**（原**忙碌脉冲**改名并改写），**下落短横（FallingDash）** 标为已退出屏幕；手工清单 ⑯ 整节重写成「输入区三行、提示符色相与静止的左栏」九条（含模糊宽度与空闲 CPU 两条）。票 05–07 的 Comments 保留为历史。
8. **基线**：`cargo test` **735 passed / 0 failed**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移。
