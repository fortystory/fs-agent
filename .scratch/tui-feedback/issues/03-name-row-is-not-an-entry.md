# 03 — 名字行不是详情入口

Type: implement
Status: ready-for-agent
Part of: ../spec.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §3。

## 目标

对话页里，一条消息的**名字行**（`[kimi]` / `[用户]` 独占的那一行）点下去不再弹详情覆盖层；正文
行照旧弹。用户看得见的是：名字只是标签，正文才是那一行可以打开的东西。

## 现状（改前先复核）

- `src/render/tui.rs` 的 `paint_block` 在两个分支里用 `named_rows` 生成「名字行 + 正文行」，然后
  把**每一行**都交给 `message_line`（`rows.into_iter().map(|line| message_line(...))`）：
  assistant 消息那支与「用户 / 非 assistant」那支各一处。
- `message_line` 给每一行都挂上同一个 `Detail`（`RenderedLine::linked`），所以名字行与正文行今天
  都是入口。
- 轨迹页走的是 `trace_message_row`（名字与首行在同一行上），**不受本票影响**。
- `named_rows` 产出的第一条行就是名字行（后面才是正文），`name == false` 时不产生名字行。

## 落点

`src/render/tui.rs`（`paint_block` 的两处映射）、`tests/render_layout.rs`。

## 具体行为

1. 两处 `named_rows` 之后的行映射改成：**第一条行**（名字行）用 `RenderedLine::from(line)`，
   其余行继续 `message_line`。写成一个小 helper（`named_body_rows(speaker, lines, text, colors)`）
   放在 `message_line` 旁边，两处共用一个实现，而不是各写一遍判断。
2. 判据是「这一行是不是名字行」，不是「这一行有没有字」：名字行的样式（发言者色）与正文行的
   不同，所以不能用样式猜。
3. 右对齐那一支（用户消息）同样只给正文行挂入口。

## 验收

- 新断言：一帧 120×24 的对话页里，点 `[kimi]` 那一格**不开**覆盖层（屏幕上不出现详情标题）；
  点它下面那一行正文的格子**开**（出现 `── 正文 ──`）。
- 同一对断言对用户消息（右对齐那一支）成立。
- 既有的「点正文行开详情」与「关闭后回到原处」两类断言照旧绿。
- `cargo test` 全绿。
