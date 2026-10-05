# 19 — 退场死代码与文档收口

Type: implement
Status: ready-for-walkthrough
Part of: ../map.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) 实现决定 §34–§35。

## 目标

留下的是决定，不是遗迹：退场的代码删掉、钉着它们的测试一起走；与观感无关的文档矛盾修掉，让文档当契约读时是可信的。

## 现状（改前先复核）

**退场但留着的代码**（各自有测试钉着）：

- `PULSE_PALETTE` 色环两版 —— 都在真机上被否掉，代码留着，另有一个测试钉「屏幕上没有任何东西穿这套调色板」。
- 下落短横：`mark_lines(Some(frame))` 的分支（生产只传 `None`）、`DASH_*` 常量、`identity_falling` —— 配一个单元测试。
- （**不在本票**）用量面板里那两条按宽度丢的路径留给[左栏与右缘](14-sidebar-and-transcript-edge.md)：实测其中**上下文百分比**那条在窄档 28 列、占比到三位数（如 617%）时**可达**，删掉它会把「丢掉百分比」变成 `…` 截断，所以去留随票 14 的列预算重算一起定。

> **`highlight::DiffTag` 整套不删**（2026-10-06 定）：本票原把它列进删除清单，与 [`../spec.md`](../spec.md) §测试决定里「diff 的三处 ANSI 字节」被明确保留、以及[内容域](16-content-domain-markdown-and-syntax.md)「不动 diff 那套独立的 ANSI 表（它是 plain 与未来的 diff 视图共用的）」相矛盾。它确实没有生产调用方，但那两处把它当契约留着，于是保留；spec §34 已同步回改。

**与观感无关的文档矛盾**：

- `tui.rs` 模块文档说「循环里**唯一**的定时器是标记的脉冲」，而同文件里还有退出手势的 deadline —— 两处同源的错（模块文档 + `docs/render.md`）。
- `Tab::Trace` 的文档注释还写着「还没做」，而轨迹页早已实现。
- `docs/render.md` 把外壳的 4 行写成「主列的分隔线、状态行与提示行」，漏了「**两条**」。
- `docs/render.md` 与 `layout.rs` 里各有一处还写着「外壳是**一圈外框**」—— 外框早就拆了。

## 落点

`src/render/tui.rs`、`src/render/wording.rs`、`src/render/layout.rs`、`src/render/mod.rs`、`docs/render.md`。

## 具体行为

1. **删掉上面三类死代码，以及各自钉着它们的测试。** 删测试是**预期结果**，不是回归 —— 在提交信息里写清「删掉的测试钉的是已退场的代码」。
2. **修与观感无关的矛盾**：计时器数量（两处同源）、`Tab::Trace` 的注释、4 行枚举漏「两条」、外框那两处。
3. **不碰随观感走的文档章节**：外壳几何与提示阶梯（[12](12-hint-line-spans-the-screen.md)）、严重度与转录（[13](13-transcript-colours-and-turn-gap.md)）、浮层（[15](15-overlays-and-questionnaire.md)）、内容域（[16](16-content-domain-markdown-and-syntax.md)）、提示符与状态行（[18](18-busy-spinner-and-idle-clock.md)）。**本票只收没人认领的那几处。**

## 验证

1. `cargo test --all-targets` 全绿（**删掉的测试要列出清单**，并对每一处说明它钉的是哪段已退场代码）。
2. `cargo clippy --all-targets` 干净；`grep` 确认删掉的符号、常量与再导出**零残留**（包括 `mod.rs` 的再导出）。
3. `docs/render.md` 改动后跑一次文档体量护栏（它由仓库的文档脚本管着）。
4. 与[内容域](16-content-domain-markdown-and-syntax.md) 同改 `highlight.rs` —— 两票之间注意 rebase。**本票不碰 `highlight.rs`**（`DiffTag` 保留），该注意项随之作废。

## 不做什么

- 不删**还在用**的近似名字符串（例如某个字形在别处也上屏，只是它的常量名看着像退场）。
- 不动任何**上屏行为**：本票唯一的用户可见变化是「文档不再骗人」。
- **不删 `DiffTag`**：见上面那条注。它留作 plain 与未来 diff 视图共用的表。
- 不动 `scripts/tui-startup-check.py`（它的三个锚点都还在用）。

## 实现记录（2026-10-06）

**已落地**（自动化部分全绿：`cargo test --all-targets` 1216 passed / 0 failed、`cargo clippy --all-targets` 与 `cargo fmt --check` 干净、`scripts/check-doc-size.py` 与 `check-language.py` 绿、`scripts/tui-startup-check.py` 15/15 绿）。

色环、下落短横与它们钉着的测试已删；文档矛盾（计时器数量、`Tab::Trace`、4 行枚举、外框 ×2）已修。**`DiffTag` 按 2026-10-06 的决定保留**（spec §34 的注）。**真终端**：无。
