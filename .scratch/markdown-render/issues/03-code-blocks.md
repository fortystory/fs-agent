# 代码块：语言名那一行，以及接回 Rust 高亮

Type: implement
Status: done

> 规格：`.scratch/markdown-render/spec.md` §3 与 §4 的前半。
> Blocked by: 01（它把 info string 解析出来存着了）。
> 本票只接 **Rust** 一种文法；10 种语言是票 04。

## 目标

代码块上方有一条右对齐的语言名，块里的代码有语法高亮。

## 落点

`src/render/markdown.rs` 的代码块那一支；`src/render/highlight.rs`（它要重新有生产消费者）；`tests/render_markdown.rs`、`tests/render_highlight.rs`。

## 具体行为

1. **语言名那一行**：代码块**上方**单独占一行，语言名**右对齐到第 `width` 列**（即结束在转录内容的右缘），该行**不缩进**。
2. **取第一个词、保留作者大小写**：`rust ignore` 只显示 `rust`；`JS` 显示 `JS`（不改写成 `javascript`）。
3. **光秃秃的 ``` **：**不画那一行**。
4. **认不出的语言**：**照样显示原文**（`brainfuck` 就是 `brainfuck`），代码退纯文本——认不出的是高亮，不是标签本身。
5. **代码内容**：2 格缩进，**逐字保留**。围栏里的 `**不是粗体**` 就是两个星号。
6. **超宽代码行**：由**渲染器自己**按 `width` 折行，**续行保持 2 格缩进**——不要交给 `pane::wrap_line` 硬折，那会把续行顶到最左边、跟代码块脱节。
7. **接回 `render::highlight`**：把它的 `Class::style()` 铺到代码块的 span 上。对齐口径要处理——`highlight_rust` 返回的是**按行**的 span 列表，而我们已经在渲染器里按宽度折过了：**先高亮、再折行**，折行时保留 span 样式（`pane::wrap_line` 已经是这个口径，可以直接复用它的逻辑或照它写一份）。
8. **高亮返回空/失败时退纯文本**，不要让代码块消失（`highlight_rust` 已经有这个降级，别破坏它）。
9. **`try_highlight` 复用 `Highlighter`**：现在每次调用都新建一个，`tree-sitter-highlight` 的文档建议复用。

## 测试

- **新增**：
  - ` ```rust\nfn main() {}\n``` ` 里，语言名那一行的内容结束在第 `width` 列；
  - 无标签的围栏**不**产生语言名那一行；
  - ` ```brainfuck ` 产生语言名那一行、且代码行没有非默认样式的 span；
  - ` ```rust ignore ` 那一行只显示 `rust`；
  - 代码行以**恰好 2 个空格**开头，且 `**不是粗体**` 原样出现；
  - 一条超过 `width` 的代码行折成多行，**每条续行都以 2 个空格开头**；
  - `fn` 的 span 是 `highlight::Class::Keyword`、字符串字面量是 `String`——**断言 `Class`，不要断言颜色**。
- `tests/render_highlight.rs` 里现有的高亮测试**一条都不改**，它们守的是模块本身。
- `a_fenced_block_is_kept_verbatim_and_never_parsed_as_markdown` 继续原样通过（现在它还要额外确认那 2 格缩进）。

## 验收

- [ ] `cargo test` 全绿。
- [ ] `cargo clippy --all-targets` 无新增告警。
- [ ] 真机：`cargo run`，让模型吐一段 Rust，关键字/字符串/注释颜色分得开，语言名贴在最右。

## Comments

- 2026-10-01 落地：语言名单独一行、右对齐到转录内容右缘（那一行不缩进）；取 info string 第一个词、保留作者大小写；光秃秃的 ``` 不画那一行；认不出的语言照原文显示、代码退纯文本。
- 高亮接回 `render::highlight::highlight_code`：**先高亮、再按宽度折行**，续行保持两格缩进（没有交给 `pane::wrap_line` 硬折）。`Highlighter` 改成 `thread_local!` 里复用一份。
- 测试：`tests/render_markdown.rs` 的代码块一节（语言名位置、无标签不画、未知语言、首个词、折行、`fn` 的 `Class`）；`tests/render_highlight.rs` 一条没改。
- 真机没跑，步骤在 `docs/tui-manual-checklist.md` ㉑ 第 3、4 条。
