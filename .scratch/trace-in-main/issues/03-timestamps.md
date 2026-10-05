# 03 — 轨迹行的时间戳

Type: implement
Status: done
Part of: ../spec.md
Blocked by: 02

> 规格：[`../spec.md`](../spec.md) §5。

## 目标

轨迹页里每个块的第一行以 `HH:MM:SS ` 开头（本地时区、静音档），续行对齐；`--continue` 重放出来
的是当初那些时刻。用户看得见：`04:16:53 [bash] ▸ 正在运行 cargo test…`。

## 现状（改前先复核）

- `Event` 的信封上早就有 `at: DateTime<Utc>`（`src/events.rs`），`EventLog::append` 每次追加时盖一次。
- `Painted::Block(Block)` 不带时间；`Painted::Thinking { speaker }` / `Thought { speaker, trace }`
  也不带。`emit_block(&Block, Targets)` 是块 → 行的唯一分派点。
- `RenderEvent::Diagnostic(String)` / `Notice(String)` 没有时刻（它们不是事件）。
- `settle_thinking(text: Option<String>)` 由 `MessageCompleted` 驱动，那里手上有事件。
- 轨迹视图的排版宽度今天是 `self.trace_width`。

## 落点

`src/render/mod.rs`（`RenderEvent` 两个变体、`Renderer::diagnose` / `notice` 两个出口）、
`src/render/headless.rs` / `plain.rs` / `tui.rs`（构造点）、`src/render/tui.rs`（`Painted`、
`emit_block`、`emit_painted`、`paint_block` 的轨迹分支、`settle_thinking`）、`src/render/wording.rs`
（时间戳的格式函数）、`tests/render_layout.rs`、`tests/history_replay.rs`。

## 具体行为

1. `RenderEvent::Diagnostic` / `Notice` 各加一个 `at: DateTime<Utc>` 字段；两个出口
   （`Renderer::diagnose` / `Renderer::notice`）在发的时候 `Utc::now()` 一次。所有构造点跟着改。
2. `Painted::Block { block, at }`、`Painted::Thought { speaker, trace, at }` 带上时刻；
   `Painted::Thinking` 不带（那一刻还不知道这个想法属于哪条消息）。`push_block` 与
   `emit_painted` 的签名跟着走 —— `at` 从 `apply` 那条路来：`RenderEvent::Logged(event)` 给
   `event.at`，`Diagnostic` / `Notice` 给它们自己那个字段，`Delta` 派生出来的行**不记**。
3. `settle_thinking` 收下那条 `MessageCompleted.at`，写进 `Painted::Thought`。
4. 时间戳的生成只有一处：`wording` 里一个函数把 `DateTime<Utc>` 变成 `HH:MM:SS` 的本地字符串
   （`chrono::Local`），格式常量 `STAMP_COLUMNS = 9`（8 列 + 一个空格）住在 `layout` 或 `wording`
   里，与它的用处相邻。
5. 绘制：轨迹视图里每个块的**第一条**行前插时间戳 span（`palette::MUTED`），续行补
   `STAMP_COLUMNS` 个空格；轨迹内容的排版宽度 = 主列内容宽 − `STAMP_COLUMNS`，所以前缀 + 内容
   正好铺满而 markdown 不与时间戳抢列。对话视图**不插**。
6. `live` 尾巴行在轨迹页里不画时间戳。
7. 测试不写死时区：期望值用 `event.at.with_timezone(&Local).format("%H:%M:%S")` 现算。

## 验收

- 一条帧断言：轨迹页上某一行的开头等于那条事件的 `at` 按本地时区算出来的 `HH:MM:SS`（用注入的
  固定 `at` 构造事件，不读真时钟）。
- 一条帧断言：对话页里同一段文本前面**没有**时间戳。
- 一条帧断言：`--continue` 重放的历史块带着流上那些 `at`（构造两条不同时刻的事件，屏幕上出现两个
  不同的时刻串）。
- 一条单测：格式函数的秒级与补零（`04:06:07` 那一类）。
- 一条帧断言：块的第一行有时间戳、续行对齐（同一块的两行左边界相同 —— 用列号量）。
- 既有 `tests/render_layout.rs` 里那些对轨迹页做整行文本比较的断言改成忽略行首时间戳（或按
  `STAMP_COLUMNS` 跳过去），不要写死一个时区相关的串。
