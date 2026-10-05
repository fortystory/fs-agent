# 10 — 对话视图的过滤与形态

Type: implement
Status: done
Part of: ../map.md
Blocked by: 09

> 规格：[`../spec.md`](../spec.md) §2（分工与保留清单）与 §3 的两处例外。

## 目标

主列只剩**用户文本、assistant 正文、保留清单六类、回合 / 轮次边界行**；用户消息**右对齐**；注入 / 用户 / 助手三类前缀**各一色**。用户看得见：主列变成「聊天」，而过程行只剩左栏那一份。

## 现状（改前先复核）

- 对话视图与轨迹视图此时都画全量（上一张票的结果）。
- 前缀的颜色今天由发言者调色板给：用户 `LightGreen`、助手（单 agent 下是配置里的发言者名）`LightCyan`；**分不开的是注入** —— 它是无前缀的叙述行，与别的叙述行同为灰。
- 行级右对齐与折行保留对齐是现成能力。

## 落点

`src/render/tui.rs`（分工的纯函数与对话视图的绘制分支）、`src/render/wording.rs`（若前缀分档需要一个新入口）。

## 具体行为

1. **分工是一个纯函数**（块 → 目标集合），住在绘制侧；块层一个字节不改。
2. **保留清单（枚举六类）**：用户与 assistant 正文、`AgentError` / `SessionError`、`SessionEnded`、`PermissionAsked` / `PermissionDecided`、失败的 `Hook`、**`Notice` 整类**；外加**回合 / 轮次边界行**（`TurnStarted` / `TurnEnded` / `RoundStarted` / `RoundEnded`）—— 它们是分段线。
3. **进轨迹的**：执行者（`SpeakerId::Executor`）的全部行、`Usage`、`Divergence`、`Sandbox`、`History`、`ContextInjected`、`Tool` / `ToolFeedback`、思考行。
4. **用户消息右对齐**，只对话视图；助手与轨迹页仍左对齐。
5. **三类前缀各一色**：给注入行一个专色（用户与助手不动）。

## 验证

1. 帧断言：同一场会话里，过程行不在主列、在轨迹页；错误、`Notice`、边界行在主列。
2. 用户那行靠右、助手仍左对齐；注入行的颜色与用户、助手都不同。
3. 分工的纯函数**穷举单测**：每一类块至少一条。
4. 既有测试大面积要改（工具行与思考行的详情入口与内容断言）—— 这是预期的，改完必须全绿。

## 不做什么

- 不改块层（`Block` 的产生）、不改 plain / headless。
- 不做降级退回全量（[11](11-fallback-when-sidebar-hidden.md)）。
- 不给轨迹页加底色（[12](12-trace-round-stripes.md)）。

## 作答（2026-10-05）

- **分工的纯函数**是 `src/render/tui.rs` 的 `selects(view, block)`：轨迹视图一律 `true`，
  对话视图只留用户文本、assistant 正文、`AgentError` / `SessionError` / `SessionEnded` /
  `PermissionAsked` / `PermissionDecided`、**失败的** `Hook`（`outcome` 以
  `hook_format::FAILED_PREFIX` 开头）、`Notice` 整类、`Diagnostic`，以及回合 / 轮次边界行
  （`TurnStarted` / `TurnEnded` / `RoundStarted` / `RoundEnded`）。`Block::Message` 里
  `SpeakerId::Executor` 的全部行、工具与工具反馈、用量、分歧、沙箱、历史、上下文注入、
  执行者进出与流式增量都归轨迹。`emit_block` 按它过滤；`turn_rail.close_unit()` 仍只看
  `targets.conversation`（边界块在对话视图里可能一行都不留，而那一格照旧要长出来）。
- **思考行只进轨迹**：`paint_thinking_line` / `paint_settled_thinking` 的对话分支删掉。
- **例外一**：`Block::Message` 里 `SpeakerId::User` 的行在**对话视图**设
  `Alignment::Right`（折行时保留）；轨迹视图与助手仍左对齐。
- **例外二**：`Block::ContextInjected` 那一行与它的详情边框从叙述灰改成
  `Color::LightBlue`；用户 `LightGreen`、助手 `LightCyan` 不动。
- **测试改动**（预期的大面积）：`tests/render_layout.rs` 30 条与
  `tests/history_replay.rs` 9 条工具行 / 思考行用例改成「先把左栏切到轨迹页，再点/断言」
  —— 新增两个文件各自的 `open_trace_tab` helper；`a_tool_call_line_describes_the_call_...`
  里那句 40 列下会折成两行，断言放宽成两段；`a_click_outside_the_detail_overlay_closes_it`
  改点覆盖层上方那条边距（覆盖层在 120×40 下占 2..38 行，主列的旧落点已被它盖住）。
  新增：`selects` 的**穷举单测**（30 条 case，每一类块一条）与三条帧断言
  （过程行不在对话、在轨迹；用户靠右 / 助手靠左；三类前缀三色）。
- `cargo test` 全绿（render_layout 162 + history_replay 31 + 其余）、
  `cargo clippy --all-targets` 无警告、`cargo fmt --check` 干净。
- **已知缺口（票 11 的活）**：左栏不可见（`w < 80` 或 `Ctrl-O` 收起）时轨迹视图不物化，
  过程行此时没有任何去处 —— 降级判据归下一张票。
