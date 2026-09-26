# 收尾：文档、旧 spec 回改、手工清单与基线

Type: implement
Status: done
Blocked by: 01, 02

> 规格：`.scratch/tui-input-pulse/spec.md` §1–§4、`Testing Decisions`。

## 目标

把这两条改动落到文档与旧 spec 上，并且**用实测把新的行数账钉一遍** —— 特别是「`select!` 没有定时器」这条旧决议的部分推翻，必须写在下一轮的人会读到的地方。

## 落点

`README.md`、`docs/render.md`、`docs/tui-manual-checklist.md`、`CONTEXT.md`、`.scratch/tui-sidebar/spec.md`、`scripts/tui-startup-check.py`（只改注释，若需要）。

## 具体行为

1. **`README.md`**：界面那一节的**真帧重新生成**（`draw_frame` 渲进 `TestBackend` 后 dump，120×24，不要手绘），输入区画成 3 行；正文里「输入区（1–10 行）」一类的说法改成 3–10 行，补一句「忙碌时左栏 mark 在色环上走」；降级阶梯那段里的转录行数照 §1 的表改。
2. **`docs/render.md`**：几何那一处「输入区 1–10 行」与 chrome 7 行的账改成新值；**事件源那一段必须改写** —— 它现在写着「三条来源、没有定时器」，改成「三条来源 + 一条**只在忙碌时 arm** 的脉冲定时器（100ms）」，并说明为什么不是每次循环一个 `sleep`。别忘了 mark 的颜色现在由脉冲决定。
3. **`docs/tui-manual-checklist.md`**：新增两条（放在 ① 光标与 ⑮ 外壳改版附近，按现有编号往下排）：
   - **3 行输入区**：空草稿时输入区就是 3 行（`> ` 在第 1 行、后两行空），敲到第 4 行才长高，10 行封顶；光标始终在正确格子。
   - **忙碌色环**：真终端里跑一个回合，看左栏 mark 是否在**变色**（而不是闪）；在慢终端 / 大量输出时是否仍然在动（这条是 100ms 定时器 + `MissedTickBehavior::Delay` 的验收点）。
4. **`CONTEXT.md`**（§渲染）新增词条 **忙碌脉冲（Pulse）**：中文是叙述用词、英文是代码里的 `pulse`；一句话说清它是「一次运行在飞时 mark 的颜色相位（12 帧、100ms 一帧、只在忙碌时存在）」，`_Avoid_`：动画、spinner、loading。
5. **`.scratch/tui-sidebar/spec.md` §8 回改**：`TICK` 那一条后面追加「**实现期修正（`.scratch/tui-input-pulse/spec.md` §4）**：定时器随忙碌脉冲回来，但只在一次运行在飞时 arm；空闲时这个 `select!` 仍然是三条来源」。**不要**改写原来的论证 —— 它记录了当时为什么删，修正要能看出前后。
6. **`scripts/tui-startup-check.py`**：跑一遍确认锚点仍绿（`MARK_ROW`、`ctrl-c`、`BANNER_ANCHOR`、`TEARDOWN`）；只有注释里出现「输入区 1 行」这类说法时才改注释，**锚点不动**。
7. **spec 回改**：实现期与本 spec 不一致的地方（比如某个数字、某个函数名）改回 `.scratch/tui-input-pulse/spec.md`，并把三张票的 `Status:` 改成 `done`。

## 测试

- `cargo test` 全绿；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只留 `src/context/repo_map.rs` 与 `tests/repo_map.rs` 两处既有漂移。
- `cargo build` 后 `python3 scripts/tui-startup-check.py`（三个手势 + 一个 `--continue`）全绿。
- 矩阵数字逐档与 `prototype/geometry-table.md`（`.scratch/tui-sidebar/`）对一遍：变的只有输入相关的那几档，不一致的要么改代码、要么在 spec 里写清为什么。

## 不做什么

不改任何键位；不给脉冲加配置项；不动 plain / headless 渲染器与事件 schema。

## Comments

**实现完成（2026-09-26）**。落点：`README.md`、`docs/render.md`、`docs/tui-manual-checklist.md`、`CONTEXT.md`、`.scratch/tui-sidebar/spec.md`、`.scratch/tui-sidebar/prototype/geometry-table.md`。

1. **README**：界面那一节的帧**重新 dump**（`draw_frame` 渲进 `TestBackend`，120×24，用一次性的 `zz_dump_readme_frame` 测试打印，取完即删 —— 帧里的会话、模型、用量与左栏读数都是同一份注入事实，所以不会自相矛盾）。正文三处改到新账：左栏那句补上忙碌色环（12 帧 / 10 帧每秒 / 停下回渐变）、主列那句写明输入区**最少三行**、降级阶梯那句把「120×24 转录 16 行」改成 14 行并写明 40×10 那档的优先级。
2. **`docs/render.md`**：mark 那一条补上脉冲（含「全局渐变的代价是有意的」）；chrome 那一条的 `转录行 = h − 7 − 输入行数` 补上「输入行数 clamp 到 3…10，转录最后一行优先」；**前端那一段「三条来源、没有定时器」改写**成「三条来源 + 一条只在运行在飞时存在的脉冲定时器」，并写清为什么是 `interval` 而不是每轮重建的 `sleep`。
3. **`docs/tui-manual-checklist.md`**：⑨ 的数字改到新账（10 行草稿 → 转录 7 行；删回空草稿 → 14 行）；⑩ 第 1 条点明「空闲是渐变、跑起来整块变色，见 ⑯」；**新增 ⑯「输入区三行与忙碌色环」**八条：三行地板与光标、40×10 只有 2 行是预期、色环读起来像在动还是像在闪、大量输出时仍要动（`interval` 那条）、空闲 CPU 应当接近 0（守卫没丢）、停下回渐变且下一轮从第 1 帧起、窄屏不该有任何变色。
4. **`CONTEXT.md`**（§渲染）新增两条词条：**输入区（InputArea）**（3–10 行、无自带边框、转录底线优先）与**忙碌脉冲（Pulse）**（12 帧色环、纯渲染器状态、空闲时那台时钟不存在、宽屏可见），各带 `_Avoid_`。
5. **`.scratch/tui-sidebar/spec.md` §8 回改**：`TICK` 那条后面加「实现期修正」引用块 —— 定时器随脉冲回来一条，边界是「只在一次运行在飞时 arm」；**原文不改写**，并在注释里说明为什么留着它（它是当时删掉的理由，改的是适用范围）。
6. **`.scratch/tui-sidebar/prototype/geometry-table.md`**：表头加一段「**这是历史记录，不要照着它改代码**」，点明空草稿那列已过时（输入区 1 → 3，24/174 行档转录各少 2 行，40×10 少 1 行），B 表与左栏列仍成立。表里数字一个没动。
7. **`scripts/tui-startup-check.py`**：脚本里没有任何「输入区 1 行」一类的说法（只有 `input` 出现在 tty flags 的注释里），所以**一个字节没改**；跑了一遍 `target/debug/fs-agent 1` → **4/4 GREEN**（三个手势 + 一个 `--continue`）。
8. **spec 回改**：`.scratch/tui-input-pulse/spec.md` 的实现决议与实现一致，无需回改；三张票的 `Status:` 都改成 `done`。
9. **基线**：`cargo test` **728 passed / 0 failed**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移。
