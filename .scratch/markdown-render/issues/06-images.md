# 图片落成 `[图片] alt (url)`

Type: implement
Status: done

> 规格：`.scratch/markdown-render/spec.md` §6。
> Blocked by: 01（它先让 `Image` 走 `Link` 那条路）。

## 目标

`![alt](url)` 不再渲染成 `!alt (url)`（那个 `!` 是手写扫描器逐字符走时的残留噪声），而是明确告诉读者「这儿有一张图，终端暂时画不出来」。

## 落点

`src/render/markdown.rs` 里 `Tag::Image` 那一支；`tests/render_markdown.rs`。

## 具体行为

1. `![alt](url)` → **`[图片] alt (url)`**：`[图片]` 用 `MUTED`（灰），`alt` 用下划线（与链接标签一致），` (url)` 用 `MUTED`。
2. `alt` 为空时（`![](url)`）→ `[图片] (url)`，不留一个空的下划线 span。
3. url 与 alt 相同时不重复补 ` (url)`（与链接那条规则一致）。
4. **不做真图渲染**——那需要终端图像协议（kitty graphics / iTerm2 inline images / sixel），明确留在 spec 的 Out of Scope 里，等单独一轮。
5. 内联 HTML 与 HTML 块**原样透传**，顺手确认这条在票 01 的透传路径里是好的（同一张票里加个断言）。

## 测试

- `![图](https://example.com/a.png)` → 文本是 `[图片] 图 (https://example.com/a.png)`，且 `图` 那个 span 带 `UNDERLINED`。
- `![](https://example.com/a.png)` → `[图片] (https://example.com/a.png)`，没有空的下划线 span。
- `![same](same)` → `[图片] same`（不补 url）。
- `<b>x</b>` 原样出现。
- 围栏块里的 `![alt](url)` **仍然逐字保留**，不被当图片（`a_fenced_block_is_kept_verbatim_and_never_parsed_as_markdown` 的延伸）。

## 验收

- [ ] `cargo test` 全绿。
- [ ] `cargo clippy --all-targets` 无新增告警。
- [ ] 真机：让模型吐一段带图片语法的回答，看到的是 `[图片] …` 而不是 `!…`。

## Comments

- 2026-10-01 落地：`![alt](url)` → `[图片] alt (url)`（`[图片]` 与 ` (url)` 灰、`alt` 下划线）；`alt` 为空时不留空的下划线 span；`url == alt` 时不重复；内联 HTML 与 HTML 块原样透传（`<b>x</b>` 逐字出现在行里）。
- 「图片」那个词是给人看的，所以它住在 `wording::IMAGE_PLACEHOLDER`，不散在渲染器里。
- 测试：`tests/render_markdown.rs` 的图片四条 + 围栏里图片语法原样那一条。
- 真机没跑，步骤在 `docs/tui-manual-checklist.md` ㉑ 第 6 条。
