# 02 — 记号抽象与 `@` 补全

Type: implement
Status: done
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

## 评论

- 2026-10-05 落地：新增 `src/render/token.rs`（`Token` + `tokens` / `token_at` / `first`
  三个纯函数，边界规则按前缀参数化）。`editor.rs` 的 `SlashToken` / `slash_token()` /
  `complete_slash()` 换成 `token()` / `complete_token(prefix, text)` —— 补全替换的是整段
  记号，与「一个句子里多个记号各算各的」同一条。
- 菜单：`slash_menu()` → `token_menu()`，`SlashMenu` → `TokenMenu`（带 `sigil`），
  `MenuSelection` 记 `(sigil, query)` 而不是只记 query —— 否则 `@a` 会接着 `/a` 的高亮。
  候选来源按前缀挑：`/` 走 `catalog`，`@` 走票 01 的索引（裸 `@` 与未就绪都不列）。
- 插入分叉：接受**目录**（候选以尾随斜杠结尾）时菜单保持开着、高亮回到第一个候选、query
  换成新前缀；文件与命令照旧关菜单。另外把「等于当前 query 的那一条」从候选里滤掉 ——
  `@src/` 下钻之后不必再列它自己。候选总量上限是 `TOKEN_MENU_CANDIDATES`（200）。
- 两处边界的复核：`query` 是**整段名字**（打 `/ask-matt` 时光标停在中间，query 仍是
  `ask-matt`，与「补全替换整段」一致）；`/` 的「任意位置」意味着 `src/@foo` 里的 `/foo`
  仍然是一个 `/` 记号，只是 `@` 那一侧不放宽。
- 测试：`tests/render_editor.rs` 的记号一组（触发边界、任意行、两个记号各算各的、补全），
  `tests/render_layout.rs` 新增三条（裸 `@` 不画菜单与未就绪、目录下钻保持开着且文件关掉、
  `@` 菜单回车只接受而 `/` 仍发送）。
- **`/code-review` 之后补的四处**：① 那个「滤掉等于当前 query 的候选」原来对**文件**也生效，
  于是 `@a`（索引里正好有个 `a`）菜单会整个消失 —— 现在只滤**目录**候选自己，文件照列；
  ② `MenuKey` 加上**位置** `start`：`Esc` 的记账按 spec §2 是「按 token」，只比
  `(前缀, query)` 时同一行里两个同名记号（`@a @a`）会互相牵连，`MenuSelection` 因此换成
  `Option<MenuKey>`（顺手消掉了 `'\0'` 那个哨兵，`prefix` 改名 `query` 也不再与 `Token.prefix`
  同名反义）；③ 零调用的 `token::first()` 删掉（提交解析要的是「最靠左的**命令**记号」，
  与「最靠左的 `/` 记号」不是一回事，它自己的 `find` 才是对的）；④ `editor::Highlight` 改名
  `editor::TokenSpan` —— 那个值装的是记号区间（吸附与整块删都读它），只讲「上色」的名字
  与 `CONTEXT.md` 的**记号（Token）**词条对不上。
