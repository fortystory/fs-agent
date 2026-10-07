# 03 — 测试、文档与两处既有错位的收口

Type: implement
Status: ready-for-walkthrough
Blocked by: 01, 02

> 规格：[`../spec.md`](../spec.md) §7–§8。票 [`01`](01-usage-tail-on-call-row.md) /
> [`02`](02-turn-total-tail.md) 落地之后，这一票做测试、文档与两条回归。

## 目标

新行为有断言看着，两处既有错位有回归断言看着，`docs/` 与人工走查表跟上。

## 现状（2026-10-07 核实，改前先复核）

- 现有帧测试里**没有一条**断言独立的 `用量 in=…` 行出现在屏幕上（全 `tests/` 无命中），所以
  「用量行消失」这件事今天没有任何测试守着。
- 思考行那一侧有约 15 处断言（`tests/render_layout.rs:5616`、`6868`、`8088`、`8827`、
  `tests/history_replay.rs:923` 等）。多数是 `contains("[kimi] ▸ ✓ 思考完成")` 这类**子串**
  断言，尾巴加在行尾不会让它们红；会受影响的是按列算术或整行相等的那几处（例如
  `a_thinking_line_tints_its_speakers_name` 的 `cell_of(..., "✓ 思考完成")` 与
  `a_settling_thinking_line_jumps_to_the_moment_it_finished` 的 `starts_with(stamp)`）。
- 两处既有错位：`settle_thinking` 的 `replace_last` 顶掉刚 push 的用量行；宽度变化重放时清单里
  夹着 `Painted::Block{Usage}` 会让定稿走追加、留一行「正在思考」。
- `docs/render.md` 有轨迹视图与「时间戳」两节，没有用量行的段落；`docs/tui-manual-checklist.md`
  有轨迹页那一组走查项。

## 落点

`tests/render_layout.rs`、`tests/wording.rs`、`docs/render.md`、`docs/tui-manual-checklist.md`、
`.scratch/README.md`。

## 具体行为

1. **尾巴落在正确宿主行**：一次工具调用之后，用量贴在该工具行末尾；没有工具时贴在思考行末尾。
2. **跨调用配对**：一次发言里两次调用，两笔用量分别落在各自调用的行上，顺序不乱。
3. **反向断言**：屏幕上不再出现独立的 `用量 in=…` 行；合成器与「只留下消息行」的调用仍出现。
4. **合计**：只调用一次时无合计；≥2 次时合计等于各笔之和。
5. **窄档折行**：窄终端下宿主行按既有规矩折行，不省略、不消失。
6. **回归 ①**：「只推理、不产出正文」的一次调用之后，用量仍在，且没有被顶掉。
7. **回归 ②**：宽度变化重放之后，不出现多余的「正在思考」行。
8. `wording::usage_tail` / `total_tail` 各一条单测（`tests/wording.rs`），并确认
   `a_usage_line_reads_in_chinese` 仍然绿。
9. `docs/render.md`：轨迹视图与「时间戳」两节各补一句 —— 用量长在产生它的那次调用的行尾、
   回合收尾可能有合计。
10. `docs/tui-manual-checklist.md`：轨迹页那一组加一条人工走查项。
11. `.scratch/README.md` 的 feature 表加一行。

## 验收

`cargo test` 全绿；`cargo clippy --all-targets` 与 `cargo fmt --check` 干净；人工走查项写进表里
（状态 `ready-for-walkthrough`，由人在真终端里走）。

## 评论

- **2026-10-07 落地（剩真机走查）。** 九条新断言加在 `tests/render_layout.rs` 末尾（含跨调用
  配对、合计、两处回归、窄档折行、详情不变），`tests/wording.rs` 加一条措辞单测；
  `docs/render.md` 的轨迹视图与「时间戳」两节各补一句，`docs/tui-manual-checklist.md` 加第 9 条
  人工走查项。`cargo test` 全绿。**回归二**在新实现下不再是缺陷路径：用量不再进 `painted`
  清单，定稿不会被夹在中间（spec §7）。
