# Markdown 的解析交给 `pulldown-cmark`，渲染仍是我们的

[`src/render/markdown.rs`](../../src/render/markdown.rs) 从第一天起是一支**手写的单趟扫描器**，文件开头把这件事写成了姿态：「它刻意不是 CommonMark：一支手写的扫描器，只扫 coding agent 真会吐出的那些构造，**不依赖任何 parser**。」

这次把**解析**那一半交出去：用 `pulldown-cmark`（`default-features = false`）把 Markdown 变成事件流，**渲染**——到 ratatui `Line` 的每一处决定：列宽、折行、表头样式、代码块、语言名那一行——仍然全是我们自己的。姿态因此改写成「**不手写解析器**」：目标没变（认不出的东西一律原样透传），换掉的是达成它的手段。

完整规格在 [`.scratch/markdown-render/spec.md`](../../.scratch/markdown-render/spec.md)，一手材料是那轮的 [`.scratch/markdown-render/research/`](../../.scratch/markdown-render/research/)（01 是三个候选 crate 的依赖与接口，02 是 `pulldown-cmark` 的事件模型与 10 个 grammar 的兼容性实测）。

## 为什么手写到了尽头

手写扫描器不是坏掉，是**三处缺口它结构上补不了**：

1. **表格要对齐，就得先收齐全表。** 现在是逐行 `for raw in text.split('\n')`，碰到 `|` 就 `join(" │ ")`。列宽必须知道整张表——这不是多写几行能补的，是扫描器的**形状**问题（一遍变两遍）。
2. **表头信号被丢掉了。** `|---|---|` 那条对齐行被 `is_table_separator` 判为「不带内容」整行丢开——而它是**唯一**能标出「上一行是表头」的东西。丢它和「认不出表头」是同一个 bug。
3. **围栏的语言标签被丢掉了。** `opening_fence` 只返回 `(字符, 长度)`，`rust` 三个字当场消失。没有它就没法选文法，高亮无从谈起。

往后的边角（嵌套列表、行内嵌套、反斜杠转义、跨行行内代码）每一条都要单独手写一遍判定，而它们全都是 CommonMark 已经定义清楚、且**已经有参考实现**的东西。

## 为什么不是某个现成的 ratatui markdown 渲染器

三个候选都被一手核实否掉了（细节与来源在 [research/01](../../.scratch/markdown-render/research/01-markdown-crate-selection.md)）：

- **`tui-markdown` 0.3.10**：关掉高亮仍有 **43 个 registry crate**；语言标签**只能靠字符串抠**（公开 API 没有任何块结构或语言元数据）；缩进代码块会把多行粘成一行；而且 `Options::table_width` 的折行**只对表格生效**——200 字符的段落回来仍是 1 行 200 列。它违反的是本项目那条硬约束：**换行归我们**（`pane::wrap_line` + `render/width.rs`），渲染器必须把**未换行**的行交回来。
- **`ratatui-markdown` 0.3.6**：已发布版本要求 `ratatui ^0.29`（我们已经是 0.30），并锁 `tree-sitter 0.26`——在 0.x 语义下与我们的 `0.27` 不兼容，Cargo 会**同时编译两份 tree-sitter**。包体 8.37 MB（其中 7.27 MB 是一个 mermaid 动图）。
- **`markdown-ratatui` 0.1.0**：**完全没有高亮**，只有 `CodePolicy::Wrap|Clip`；首发三周、总下载 84。

更根本的一条：**渲染层的工作这三条路都逃不掉**。我们要的观感（超宽单元格折行、列宽余量给最后一列、表头分隔线、代码块上方右对齐的语言名、接自己的 tree-sitter 高亮）没有任何现成渲染器会给。既然渲染要自己写，那么能外包的只剩解析——而解析恰恰是手写最容易出错的那一层。

## 代价

1. **一个新依赖**：闭包 5 个纯 Rust 包（`pulldown-cmark` + `bitflags` + `memchr` + `unicase`），无 C 构建，MSRV 1.71.1，MIT。**不开** `simd`（那会引入 `unsafe`）。与此对照，`tui-markdown` 关掉高亮是 43 个包、开高亮是 76 个（多出来的 33 个里就有 `onig_sys`——正是 spec §19 排除的那条 Oniguruma 路径）。
2. **`to_lines` 开始依赖宽度**。签名从 `to_lines(text)` 变成 `to_lines(text, width)`，因为表格的列宽与超宽代码行的折行都需要知道可用列数。这推翻 [`.scratch/tui-layout/spec.md`](../../.scratch/tui-layout/spec.md) §3 里「源行缓冲……**宽度无关，可复现**」那半句：宽度变化时，现在要**重跑 markdown 渲染**再重新折行，而不只是重新折行。代价判定为可接受——每个块一次 `pulldown-cmark` 很快，而 §3 本来就规定缓存按 `frame.area().width` 失效、宽度变化全量重算。
3. **`markdown.rs` 被重写**：`is_rule` / `blockquote` / `list_item` / `task_box` / `heading` / `inline_spans` 这些手写判定全部删除，换成事件驱动的渲染。`tests/render_markdown.rs` 里断言「分隔行被丢掉」的那条要反过来写；两条降级测试（围栏逐字保留、畸形输入不消失）**一字不改**地留着当安全网。

## 被否决的替代方案

- **继续手写、只补表格与标签**。唯一零新依赖的路，也被认真考虑过——现有的「认不出就透传」很稳。但它要求自己实现「整表缓冲 + 列宽分配 + 单元格折行 + 对齐解析 + info string 解析」，而这些正是解析器的全部存在理由；往后每遇到一个 CommonMark 边角还要再写一遍。**否决的理由是覆盖面，不是难度。**
- **引 `pulldown-cmark` 但只用于表格**。会在同一个文件里留下两套解析（一套给表格、一套给其余），两套对「什么是一行」的理解迟早分叉。**否决。**
- **让渲染器吐出「结构化逻辑行」，把表格对齐推迟到折行阶段**，从而绕开「`to_lines` 收宽度」这条代价。那要求表格在 `Line` 之外再造一种中间表示，而 `Line` 是所有下游（转录缓存、`pane`、plain 渲染）共用的一条边界。加一个 `width` 参数比新造一种中间类型便宜得多。**否决。**
