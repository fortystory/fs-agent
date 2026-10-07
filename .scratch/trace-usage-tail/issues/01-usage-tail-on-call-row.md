# 01 — 用量尾巴并进该次调用的行

Type: implement
Status: done
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1–§4、§6–§7。本票只做「尾巴」：合并的落点、时机、写法、
> 缺位与重放一致；合计归 [`02`](02-turn-total-tail.md)。

## 目标

每次模型调用的用量不再独占一行，改为贴在**它到达那一刻这次调用最后画出的那条过程行**的
末尾；没有可承载的宿主时保留独立用量行（与今天逐字相同）。

## 现状（2026-10-07 核实，改前先复核）

- `Block::Usage` 由 `EventPayload::UsageRecorded` 产出（`src/render/transcript.rs:404`），在
  `apply` 里经 `push_block` 画成一行（`src/render/tui.rs:6610`）：`speaker_line` + `MUTED`，
  `link: None`、无 `▸`。
- `UsageRecorded` 的 emit 在 `src/agent.rs:658-665`，固定在流末尾、`Finished` 之前；
  `MessageCompleted` 在其后（`src/agent.rs:706-716`）；工具行（`Block::Tool`）更晚 —— 它由
  `ToolCallCompleted` 产出（`src/agent.rs:718` 起）。所以用量到达时该次调用的工具行**还没出现**。
- `Painted` 只有三种、不带用量（`src/render/tui.rs:1602-1619`）；宽度变化重放走
  `rerender_if_width_changed` → `emit_painted`（`src/render/tui.rs:2120-2172`）。
- `Pane` 有 `push` / `replace_last`（`src/render/pane.rs:80`、`:90`），没有「给最后一行追加一段」。
- 合成器那次调用没有 `TurnStarted` / `TurnEnded`（`src/agent.rs:1640-1644`），但仍发
  `UsageRecorded`（`:1727-1731`）。

## 落点

`src/render/wording.rs`（新措辞）、`src/render/tui.rs`（状态、`Painted`、`apply`、
`emit_block`、`emit_painted`、`settle_thinking`）、`src/render/pane.rs`（追加一段到末行）、
`tests/render_layout.rs`。

## 具体行为

1. `wording::usage_tail(&Usage)` 交回 `in={input} out={output}`；`usage_summary` 一字不改，
   它仍是诊断通道那一份。
2. 新增 `Tail`（`Usage(Usage)` / `Total(Usage)`，后者归票 02）；`Painted` 的三个变体各带
   `tail: Option<Tail>`。
3. 渲染器新增状态 `pending_usage: Option<Usage>` 与 `in_call: bool`。
4. `apply` 处理 `Block::Usage`：`in_call` 为真**且**窗格最后一条能承载时，把尾巴写进 `painted`
   最后一条并给窗格最后一行追加一段（`Pane::append_to_last`），**不画行**；否则走老路画独立
   用量行（合成器情形）。
5. `usage_host_exists()`：`painted` 最后一条是思考行，或除 `Block::Message` / `Block::Usage`
   以外的块时为真 —— 用量到达那一刻它正是这次调用画出的最后一条（§1）。
6. `emit_block` 收一个 `tail: Option<Tail>`；`push_block` 传 `None`，`emit_painted` 传条目
   自己的 `tail` —— 画完宿主行再追加到最后一行，于是宽度变化与 `--continue` 重放与实时同源。
7. `settle_thinking` 把 `Painted::Thinking` 换成 `Painted::Thought` 时，`tail` 跟着搬过去
   —— 定稿是整行重写，不搬就等于把这笔用量吞掉。

## 验收

- 轨迹页上不再出现独立的 `用量 in=…` 行 —— 合成器那次调用除外（票 03 断言）。
- 一次工具循环的两次调用，两笔用量各落在各自调用的行上（后一笔在后一次调用的行上）。
- 宽度变化重放之后落点与实时一致，且没有重复的尾巴。
- 宿主行的详情与可点性不变；尾巴本身不可点、不带 `▸`。

## 评论

- **2026-10-07 落地。** `wording::usage_tail`、`Painted` 上的 `tail`、`Pane::append_to_last`、
  `trace_block` / `usage_host_exists` 全部到位；`cargo test` 全绿。实现时改掉了一处设计：**到达
  就贴**，不等到那次调用收尾 —— 收尾时最后一行已经是消息行，最常见的纯对话调用会贴不上
  （spec §2、ADR 0016 的「被否决的替代方案」）。
