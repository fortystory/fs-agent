# 01 — 开着的思考行也带时间戳

Type: implement
Status: done
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1–§5。本票是这份 spec 的全部：开着的行按上开始时刻、
> 定稿照旧跳、重放清单跟着带 `at`、排版与「明确不做」都不动。

## 目标

轨迹页里 `[deepseek] … 正在思考` 那一行的行首也是一个 `HH:MM:SS ` 戳，宽度变化重放之后
仍在；定稿那一刻它换成完成时刻（与今天 `✓ 思考完成` 的行为一致）。

## 现状（2026-10-06 核实，改前先复核）

- `paint_thinking_line`（`src/render/tui.rs:2352`）直接 `push_line`，不过 `stamped_line`；
  `thinking_in_progress_line`（`:2399`）构造 `[名字] … 正在思考` 两段 span。
- `open_thinking`（`:2336`）由 `apply`（`:1776`）里两条路径调用：推理增量（
  `DeltaKind::Reasoning`，`:1799`）与 `Block::Message` 那条「没有增量、直接有整段 trace」
  （`:1819`）。`at = event_at(&event)` 在 `apply` 开头就算好了（`:1781`）。
- `Painted::Thinking { speaker }`（`:1498`）刻意不带 `at`；`emit_painted`（`:2015`）重建它时
  调 `paint_thinking_line`。
- 定稿路径不动：`settle_thinking`（`:2465`）→ `paint_settled_thinking`（`:2368`）里
  `stamped_line(line, at)` + `replace_last`。
- 时间戳的既有测试组在 `tests/render_layout.rs:7280` 一带（`fixed_at` / `at_event` /
  `without_stamp` / `trace_page` / `open_trace_tab` 都是现成的 helper）。

## 落点

`src/render/tui.rs`（`Painted::Thinking`、`open_thinking`、`paint_thinking_line`、
`emit_painted`）、`tests/render_layout.rs`（时间戳那一组追加三条）。

## 具体行为

1. `open_thinking(speaker, at)` 与 `paint_thinking_line(speaker, at, targets)` 收下 `at`，
   画行时过 `stamped_line(line, at)`。
2. `Painted::Thinking { speaker, at }`；`emit_painted` 用它重建。
3. 两个调用点各传自己那一条的 `at`（`apply` 里已算好的那个值），不新取时钟。
4. 定稿路径一个字不改（`settle_thinking` 仍用 `replace_last` 整行重写，戳因此换成完成时刻）。
5. 排版宽度、`Block` 层、事件 schema、对话视图、`live` 尾巴都不动。

## 验证

`cargo test` + `cargo clippy --all-targets` + `cargo fmt --check` + 两道文档护栏：

1. **开着的行有戳**：应用一条推理增量后打开轨迹页，含 `… 正在思考` 的那一行行首是
   `HH:MM:SS ` 形状（`RenderEvent::Delta` 没有信封，所以不写死字符串，只钉形状），去掉
   `STAMP_COLUMNS` 之后恰好是 `[kimi] … 正在思考`。
2. **定稿时跳**：推理增量开行 + 一条带固定时刻的 `MessageCompleted` 定稿，断言 `✓ 思考完成`
   那一行行首正是那个固定时刻的 `wording::stamp`（`tests/render_layout.rs` 的 `fixed_at`）。
3. **宽度变化重放后仍在**：开着的行先按 120 列画一帧、再按 100 列画一帧（触发
   `rerender_if_width_changed` 的整批重放），重放出来的行仍有戳。
4. **对话视图仍没有戳**：既有断言（`the_conversation_page_carries_no_stamps`）不动、仍绿。
5. 既有的思考行状态机测试（`a_thinking_segment_opens_in_place_and_settles_in_place` 等）不改
   也仍绿 —— 它们断的是行内文本，戳加在行首。

## 不做什么

- 不给对话视图、plain / headless 渲染器加时刻。
- 不给 `live` 尾巴行加时刻。
- 不动事件 schema、`Event.at`、模型可见文本。
- 不新增时钟 seam：`at` 从 `apply` 里已有的那个值传下来。

## 评论

实现落点：`src/render/tui.rs`（`Painted::Thinking` 带上 `at`、`open_thinking` /
`paint_thinking_line` 收下 `at` 并过 `stamped_line`、`emit_painted` 解构它）、
`tests/render_layout.rs`（时间戳那一组追加三条 + 一个 `looks_like_a_stamp` helper）。

实现期收口的两处：

1. **测试不写死时刻**：`RenderEvent::Delta` 没有信封，所以开着的行的戳只能是「收到它的那一
   刻」，一条断言写死字符串就会在跨秒时偶发红。开着的行断言**形状**（`HH:MM:SS ` 那九列）+
   去掉戳后的内容；只有定稿那条用 `fixed_at` 精确断言（它的事件真有 `at`）。
2. **屏幕行会 padding**：`trace_page` 交回来的行按终端宽度补了尾随空格，所以「去掉戳后的内容」
   比较前先 `trim_end` —— 否则断的是一个屏幕宽度，而不是这一行的文本。

没动的：`settle_thinking` 的定稿路径（`replace_last` 整行重写，戳因此换成完成时刻）、对话视图、
`live` 尾巴、`Block` 层、事件 schema、`layout::STAMP_COLUMNS` 与轨迹排版宽度。文档跟上了
`docs/render.md` 的「时间戳」一节与 `trace-in-main/spec.md` 的补记（记明推翻）。
