# 02 — 记号抽象与 `@` 补全

Type: implement
Status: ready-for-agent
Blocked by: 01

> 来源：[`../spec.md`](../spec.md) §2、§3。它把 `/` 菜单那条链路推广成「按前缀挑候选来源」，
> 于是 `@` 复用同一套浮层、键位与 `Esc` 优先级。

## 目标

- **一处纯函数**从草稿文本推出 `Token { prefix, start, end, query }`；`Input::slash_token()`
  并进它。
- **边界规则按前缀**（[`../spec.md`](../spec.md) §2 那张表）：`@` 是草稿**任意位置**的 token
  开头（前面是空白或草稿起点），`src/@foo` 不触发；`/` 走**同一条边界**（草稿任意行、任意位置）。
- 菜单按前缀挑来源：`/` → `TuiState.catalog`，`@` → 票 01 的索引。
- **`@` 的候选**：文件 + 目录；**前缀匹配、大小写不敏感**、路径字典序；**含空白的路径不进候选**。
- **裸 `@` 不列候选**（打第一个字符才出现）；索引未就绪时同样什么都不显示。
- **插入**：文件 → `@相对路径` 且关菜单；目录 → `@相对路径/` 且**菜单保持开着**（query 换成
  新前缀，接着过滤那一层）。
- **`Enter` 在 `@` 菜单里只接受，不发送** —— 插进去的路径只是句子的一部分。

## 现状（2026-10-04 核实，改前先复核）

- `slash_token()`（[`src/render/editor.rs:286-311`](../../../src/render/editor.rs)）：`line_start != 0
  || !text.starts_with('/')` 之外一律 `None`；名字延到第一个空白；光标越过名字也算 `None`。
- `complete_slash()`（[:317-325](../../../src/render/editor.rs)）：换掉整个区间、光标留在后面；
  光标不在记号里时返回 `false`（一份过时的菜单写不进走开的草稿）。
- `slash_menu()`（[`src/render/tui.rs:2706-2728`](../../../src/render/tui.rs)）：候选来自 `catalog`，
  `to_lowercase().starts_with()` 过滤，**空则返回 `None`**；`pending` 立着或 `dismissed` 时返回 `None`。
- 键位（[:2609-2620](../../../src/render/tui.rs)）：`↑`/`↓` 移动、`Tab` 接受、`Enter` 接受**并发送**、
  `Esc` 先关菜单（[:2568-2571](../../../src/render/tui.rs)）。
- `menu_accept()`（[:2749-2769](../../../src/render/tui.rs)）：一律 `dismissed = true` —— 目录那条
  插入要在这里分叉。
- `draw_menu()`（[:4160+](../../../src/render/tui.rs)）：画在光标处，`MENU_MAX_ROWS` 窗口、
  高亮始终可见。**这一票不动它的形状**。

## 收尾

- 测试：`@` 的触发边界（`帮我改 @x` 触发、`src/@x` 不触发）、`/` 与 `@` 同一条边界、裸 `@` 不画菜单、
  目录下钻之后菜单仍在且 query 是新前缀、`Enter` 在 `@` 菜单里不发送、含空白路径不进候选。
- 帧测试（[`tests/render_layout.rs`](../../../tests/render_layout.rs) 一类）：菜单仍画在光标处、
  窗口滚动照旧。
- `cargo test`、`python3 scripts/check-language.py` 通过。

## 不做什么

- **不做上色、不做 chip**（票 03）。
- **不改 `cli.rs` 的命令解析**（票 04）—— 这一票只让菜单能弹出来。
- 不上模糊匹配；不加 `$` 或别的记号。
