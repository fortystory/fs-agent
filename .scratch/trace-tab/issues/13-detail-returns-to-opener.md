# 13 — 详情按视图还原

Type: implement
Status: done
Part of: ../map.md
Blocked by: 09, 10

> 规格：[`../spec.md`](../spec.md) §5（详情覆盖层）。

## 目标

打开详情**只冻结打开它的那个视图**，关掉后回到**打开前的位置** —— 打开前在回看时，不再被弹到底部。用户看得见：在轨迹页翻旧内容时点开一条详情，关掉之后还在原地。

## 现状（改前先复核）

- 今天 `close_detail` **无条件**把转录送回底部并恢复跟随；每帧的冻结那两行也无条件作用在转录的 pane 上。
- 打开详情时，视图被冻结（停止跟随、并按住「新行」计数）。
- 既有测试 `tests/history_replay.rs:1044-1055` 断言的正是**旧行为**（「`Esc` 关上它，视口回到最底下 …… 而视口又开始跟随了」）。

## 落点

`src/render/tui.rs`（打开 / 关闭详情与每帧的冻结）、`tests/history_replay.rs`（改写那条断言）。

## 具体行为

1. 打开时记下「**打开方是哪个视图**」与**打开前的滚动状态**（位置 + 跟随）；后者存在详情视图旁边，不要塞进覆盖层自己那份位置。
2. 关掉时把状态**还原给那个视图**；另一个视图完全不动。
3. 每帧的冻结只作用于打开方。
4. 行为差别的半径只有一种情形：打开前**在回看**时，今天会被弹到底部，改后回到原处；打开前**贴底**时，还原就等于跟随，行为与今天**逐字相同**。

## 验证

1. `tests/history_replay.rs:1044-1055` 随实现改写，改成断言「回到打开前的位置」。
2. 新断言：从轨迹页开详情 → 关掉后轨迹页回到原处、**对话视图没动**。
3. 从对话视图开详情 → 关掉后回到打开前的位置（回看态与贴底态各一条）。
4. `cargo test` / `cargo clippy` / `cargo fmt --check`。

## 不做什么

- 不改覆盖层自己的滚动（它那份位置照旧，与视图无关）。
- 不做跨视图联动（[`../spec.md`](../spec.md) 的「明确不做」）。

## 作答（2026-10-05）

- **打开方与它的位置**记在 `TuiState::detail_opener: Option<ScrollMark>`，`ScrollMark` 是
  `{ view, top, follow }` —— 它与覆盖层自己那份正文位置分开放（票面要求）。
  `open_detail(detail, width, view)` 从打开方那个 pane 读 `top()` / `following()`。
- **`Pane::restore(top, follow)`**：`follow` 为真直接回底（与改动前的
  `set_following(true)` 逐字相同），否则把 `top` 夹回合法范围、保持不跟随。
  `close_detail` 把 `set_holding(false)` 与 `restore` 都施加给**打开方**，另一个视图一个字
  都不动。
- **每帧的冻结只作用打开方**（`draw_detail` 按 `detail_opener.view` 分派）；
  `draw_indicator` 也只清打开方的指示器，另一个视图在回看时照旧显示它。
- **一处 spec 缺口，这里补上**：票 10 之后对话视图里**没有任何可点的行**（工具、思考、注入、
  消息的入口都在轨迹页），而本票验证 3 要求「从对话视图开详情」。所以对话视图的消息行也
  挂上 `DetailKind::Message`（新的 `message_line`）—— 对话视图画的是全文，这个入口不省任何
  东西，但它让「谁打开的详情」这条机制在两个视图上都成立。轨迹页那边一字不改。
- 测试：新增三条帧断言（从轨迹页开详情 → 关掉后轨迹页回原处、对话视图没动；从对话视图在
  **回看**态开 → 关掉后回原处；贴底态开 → 关掉后继续跟随）。`history_replay` 的
  `a_history_detail_freezes_the_viewport_and_releases_it` 措辞改成「回到打开前的位置」并
  指向这三条；`the_detail_overlay_freezes_the_transcript` 改用对话视图的消息行（原来用
  `open_trace_tab` 打开，冻结的就成了轨迹页）。
- `cargo test` 全绿（render_layout 172 + 其余）、`cargo clippy --all-targets` 无警告、
  `cargo fmt --check` 干净。
