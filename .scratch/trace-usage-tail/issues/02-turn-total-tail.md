# 02 — 回合收尾的合计

Type: implement
Status: done
Blocked by: 01

> 规格：[`../spec.md`](../spec.md) §5–§6。尾巴的落点与重放一致归 [`01`](01-usage-tail-on-call-row.md)。

## 目标

一次发言里模型被调用 ≥2 次时，在 `回合结束：…` 那一行的末尾加一条 `合计 in=… out=…`，
口径是这条发言内全部模型调用之和。只调用一次时不加。

## 现状（2026-10-07 核实，改前先复核）

- `TurnStarted` **在循环内**，每轮迭代发一次（`src/agent.rs:553-561`）；`TurnEnded` **在循环
  外**的 `end_turn` 里发一次（`src/agent.rs:2242`）。所以一次发言可以跨多次模型调用。
- `Block::TurnEnded` 画成 `severity_speaker_line`（`src/render/tui.rs:6543-6552`）：发言者前缀
  拿发言者色，其余部分拿严重度色；正常完成退成 `MUTED`。它**不带 `▸`、不可点、没有详情**。
- 文案是 `wording::turn_ended(reason)`（`src/render/wording.rs:49-51`）。
- 轨迹页里 `TurnEnded` 之后还会画一条分隔线（`is_boundary`，`src/render/tui.rs:2097-2100`、
  `:7002-7008`），它不进 `painted`。

## 落点

`src/render/wording.rs`（合计措辞）、`src/render/tui.rs`（计数与累计、`emit_block` 的尾巴）、
`tests/render_layout.rs`。

## 具体行为

1. `wording::total_tail(&Usage)` 交回 `合计 in={input} out={output}`。
2. 渲染器新增状态 `turn_calls: usize` 与 `turn_usage: Usage`：
   - `Block::TurnStarted` → `turn_calls += 1`；
   - `Block::Usage` → `turn_usage.accumulate(usage)`（`Usage::accumulate` 已有）；
   - `Block::TurnEnded` → 读完之后两条都清零。
3. `TurnEnded` 画行时：`turn_calls >= 2` 就把 `Tail::Total(turn_usage)` 并在那一行末尾，否则
   什么都不加。合计文本必须随 `Painted` 条目走，否则宽度变化重放会拿到清零后的状态。
4. 合计与尾巴同一档颜色（`MUTED`），随该行自己的严重度色；`▸` 仍然不给 —— 这一行没有折起来的
   内容。

## 验收

- 一次发言只调用一次模型时，`回合结束` 行上没有合计。
- 一次发言跨两次调用时，合计等于两笔尾巴之和，并与第二次调用的尾巴同现。
- 宽度变化重放之后合计不消失、数值不变。
- 合成器那次调用的用量既不进尾巴、也不进合计（票 01 §4 已定）。

## 评论

- **2026-10-07 落地。** `wording::total_tail`、`turn_calls` / `turn_usage` 两个计数，以及
  `TurnEnded` 条目上的 `Tail::Total`。合计随绘制记录走，所以宽度变化重放拿到的是当初那个数。
