# 种子材料：TUI 里渲染 mermaid

> **这不是 spec，也不是票。** 它是 2026-10-03 一轮 `/wayfinder`（[`lifecycle-diagram`](../lifecycle-diagram/map.md)）
> 里记下的一句话意向：「fs-agent 的 TUI 里能不能渲染 mermaid」，同日派了一份只读调研。
> 还没被访谈、也没有票；想推进时走 `/grill-with-docs` 把它折成 `spec.md`，再 `/to-tickets` 拆票。
> **写就于 2026-10-03**；下面《现状》一节核实于同一天。
>
> 它与 [`lifecycle-diagram`](../lifecycle-diagram/map.md) 选中 mermaid 只是**恰好同名**：那份 effort 要的是
> **文档里的图**，这一条要的是**产品功能** —— 所以它被写进那张图的 `Out of scope`。

## 它要什么

模型吐 mermaid 围栏代码块时，TUI 把它**画成图**，而不是今天这样当普通代码块。今天的事实是：
`mermaid` 不在 `canonical_language` 里，于是 `highlight_code` 返回 `None`、退化成不着色的原文。

## 现状（2026-10-03 核实）

- **今天的行为**：一行右对齐的灰色 `mermaid` 标签 + 两格缩进、不着色的源码；不报错、不消失、
  也不画（`src/render/highlight.rs:214-228`、`src/render/markdown.rs:641-672`）。
- **全仓库零 mermaid 代码 / 测试 / 文档**：`grep -rn mermaid` 只有一处命中，且是讲被否掉的
  `ratatui-markdown` 包体里有个 7.27 MB 的动图
  （`docs/adr/0008-markdown-parsing-by-pulldown-cmark.md:24`）。
- **管线形状**：`markdown::to_lines(text, width)` 是**纯函数**（`src/render/markdown.rs:39`）；宽度由
  TUI 绘制时给（`src/render/tui.rs:4432`、`:4450-4453`），无窗格的调用方默认 80；高度与滚动全在
  `Pane` / `layout`；高亮挂在代码块上，而且是「先高亮、再折行」两拍。
- **可用件已经存在**（2026-10 一手核实）：纯 Rust、无浏览器、输出 Unicode box-drawing 文本的
  crate 至少三个 —— `mermaid-text` 0.57.0（MIT、3 个正常依赖、`render_with_width(src, Some(80))`、
  MSRV 1.92）、`mmdflux` 2.6.1、`merman` 0.7.0（Zed 的 Rust Mermaid 后端）。`console-mermaid`
  太薄；`mermaid-rs`（iwillreku3206）内嵌 Chromium，**不可用**。
- **调研全文**：[`research/01-tui-mermaid-render.md`](research/01-tui-mermaid-render.md) —— 含 11 个 crate
  的逐个核实表、9 条与仓库既有约束的撞点、五种方案的排序、7 条存疑与一手来源清单。

## 待谈的分叉

1. **做到哪一档**：调研给的排序是 ① **结构化呈现**（按 `-->` 拆缩进列表 / 加一行「N 个节点 M
   条边」；几十行、零风险，当**降级地板**）→ ② **只加 mermaid 语法高亮**（本仓库已有 tree-sitter
   基建）→ ③ **引 `mermaid-text` 画文本图**（一次解决，代价中、需过 ADR）→ ④ 自研
   `flowchart TD/LR` 子集（上千行、没有终点）→ ⑤ 外部进程 mmdc / dot（要 Node + Chromium，
   **不建议**）。
2. **宽度三档怎么办**：120 列 → 77 可用列、80 列 → 49、40 列 → 38。图在 38 列里基本没救 ——
   落地必须写清「窄于多少就不画、退回原文」。
3. **与「换行归 `pane::wrap_line`」那条立场怎么相处**：`.scratch/markdown-render/spec.md:26`、
   `:103-106` 是明写的线。画图自带排版（图不能被逐字符硬折），要在 spec 层面为「图」开一个例外，
   还是明说「图的内部排版不算折行」—— 这不是实现细节，是那份 spec 上的一条线。
4. **单任务同步的代价**：`Tui::run` 由 `tokio::spawn` 起成**一个**任务，事件到达 → `paint_block`
   全在这一个任务里同步跑完才 `terminal.draw`（`src/render/tui.rs:331`、`:1700-1722`、`:455`）。
   在 `paint_block` 里做一次 10–100 ms 的布局就是整个 TUI 卡这么久 —— **按块缓存因此是必须的，
   不是优化**。
5. **要不要开第二条进程边界**：`docs/sandbox.md:11` 写着 `tools/process.rs::run()` 是唯一的 spawn
   处；(b) 那条要在渲染器侧再开一个口，那是重写一行的决定。附带一条未实测项：headless Chromium
   能不能在 `--unshare-user/-pid` + 每次空 `/tmp` 的 bwrap profile 里跑。
6. **依赖政策**：ADR 0008 的门槛在这里同样适用 —— 那次连引一个纯 Rust 的 `pulldown-cmark` 都写了
   ADR，并逐个核实候选的依赖闭包与 MSRV。
7. **`mermaid-text` 的真实输出没跑过**（本轮只读，没有建 crate）：38/49 列下会不会节点框重叠、
   能不能**严格**不超列预算（`indent` 还要再扣）、MSRV 1.92 / edition 2024 与仓库政策的关系 ——
   报告 §5 各写了确认方式。

## 一句话

**能画，而且不用浏览器**；真正的阻力不是解析器，是本仓库自己的三条线（`to_lines` 的纯函数性质、
折行归 `pane::wrap_line`、TUI 单任务同步）与一条 ADR 门槛。
