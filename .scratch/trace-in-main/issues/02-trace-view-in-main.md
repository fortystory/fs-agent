# 02 — 轨迹视图搬进主列

Type: implement
Status: done
Part of: ../spec.md
Blocked by: 01

> 规格：[`../spec.md`](../spec.md) §3、§4。

## 目标

主列页签切到 `轨迹` 时，那块内容区画**全量块**（宽 79 列），有它自己的滚动、跟随、滚动条、
「N 条新行」指示器与可点开的行；`对话` 页照旧（回合条仍只在它那一页）。用户看得见：收起左栏
（`Ctrl-O`）或把终端缩到 60 列，轨迹页照样在。

## 现状（改前先复核）

- 轨迹 pane 的物化条件是 `trace_width != 0`，而 `trace_width = panes.sidebar_page_text()` 的宽度
  —— 左栏不可见时为零，内容也不推给它（`Targets { trace: self.trace_width != 0 }`）。
- `draw_sidebar_page` 的轨迹分支负责取景、填 `trace_drawn`、置 `trace_rect`、画滚动条与指示器。
- `mouse` 里的 `over_trace` 由 `trace_rect` 推出来；滚轮与点击按它分派。键盘三键仍只喂
  `self.conversation`。
- `prefix_style(Viewport::Trace, self.trace_tier_width)` 用**左栏页宽**（40 / 28）选带不带方括号。
- `draw_transcript` 画对话 pane、滚动条、回合条与对话指示器。

## 落点

`src/render/tui.rs`（`targets`、`rerender_if_width_changed` 的入参说明、`draw_transcript`、
`draw_sidebar_page`、`mouse`、`key`、`prefix_style` 的调用处）、`tests/render_layout.rs`、
`tests/history_replay.rs`。

## 具体行为

1. **常驻**：`trace_width` 的值改成 `panes.transcript_text().width`（主列内容宽），于是它永不为零，
   `Targets::trace` 恒真；`TuiOptions`/构造里那份为零的初值照旧（第一帧会走「变了」那一支）。
2. **绘制分派**：`draw_transcript` 按 `state.main_tab` 分派 —— `Conversation` 走今天那条路
   （对话 pane + `panes.scrollbar()` + 回合条 + 对话指示器 + `conversation_live`）；`Trace` 走原
   `draw_sidebar_page` 里那段的等价物（`trace.view(...)`、填 `trace_drawn`、`trace_rect =
   Some(text_area)`、`draw_scrollbar(frame, panes.scrollbar(), &state.trace)`、
   `draw_indicator(..., Viewport::Trace)`），**不画回合条**。
3. 前缀分档：`prefix_style(Viewport::Trace, self.trace_tier_width)` 里的 `trace_tier_width` 改成
   `panes.main.width`（主列页宽）。主列恒 ≥ 40 列，所以恒带方括号 —— 这一支**留着**，不在本票里删。
4. **滚轮**：不论指针在主列还是左栏，都滚当前显示的那一页（覆盖层 / 问卷 / 详情那三条优先级照旧，
   一个字不改）。删掉 `over_trace` 在滚轮上的分派。
5. **键盘三键**：`Key::PageUp` / `PageDown` / `CtrlG` 按 `state.main_tab` 分派到 `trace` 或
   `conversation`。
6. **点击**：`over_trace` 仍在（`trace_rect` 的判据不变：这一帧真画了轨迹页才有值），
   `link_hit(Viewport::Trace, ...)` 与指示器命中照旧按当前页取；详情关闭仍还原给 `detail_opener`。
7. 轨迹页里 `live`（流式尾巴）照旧画，等待提示照旧只在对话页。
8. `trace_rule(width)` 与消息行截断用的宽度都换成主列内容宽（下一票会再减去时间戳那 9 列）。

## 验收

- 120×24：切到 `轨迹` 后屏幕上有过程行（`调用 bash` / 思考行 / 注入行都在），且**对话正文不在**
  轨迹页断言之外的位置消失（对话页切回去仍在）。
- **左栏收起后轨迹页仍在**（`Ctrl-O` 一帧之后切到 `轨迹`，屏幕上仍有过程行）；60 列终端同理
  ——这两条正是对 `trace-tab` §6 的推翻。
- 两页滚动位置独立：在轨迹页往上滚，切到对话页再切回来，轨迹页的位置还在。
- 回合条只在对话页：切到轨迹页后 `panes.rail` 那一列上没有格子（对话页上仍有）。
- 键盘三键在轨迹页上滚动的是轨迹（`PageUp` 之后轨迹 pane 的 `top` 变了、对话 pane 的没变）。
- 既有轨迹页断言（`tests/history_replay.rs`、`tests/render_layout.rs` 的轨迹组）改成在**主列**上量。
