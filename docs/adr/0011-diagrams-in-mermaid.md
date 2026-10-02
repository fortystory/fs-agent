# 文档里的流程图用 mermaid

`docs/` 今天没有一张 mermaid 图：五处流程 / 结构图**全是手绘 ASCII**（[`README.md:221-223`](../../README.md)
的「架构」一节、[`executor.md:10-21`](../executor.md)、[`credentials.md:22-29`](../credentials.md)、
[`web.md:14-22`](../web.md)、[`render.md:95-103`](../render.md)）。新写的
[`lifecycle.md`](../lifecycle.md) 要画 fs-agent 的运行时生命周期 —— 一张鸟瞰 + 四张分层详图，
最大一张 24 个节点。手绘 ASCII 撑不住这个规模。这一篇记的是那条线上的决定与它的代价。

规格在 [`.scratch/lifecycle-diagram/`](../../.scratch/lifecycle-diagram/spec.md)。

## 决定

> `docs/` 里的**流程图**用 mermaid 写；[`docs/lifecycle.md`](../lifecycle.md) 是第一种。

- 只针对**流程图**；ADR 的标题与小标题仍照 ADR 0004 用中文，不受影响。
- 图的**方言受约束**（`flowchart` + `id[文本]` / `id{文本}` + `-->` / `-.->` / `-->|文本|`），
  写在 `docs/lifecycle.md` §1；`scripts/lifecycle-check.py` 按它解析，不认识的构造报红。
- **虚线只有一个语义**：可注入但 CLI 不注入 / 尚未实现。异常路径（两下 `Ctrl-C`、panic）
  用边标签。
- 图与代码**对账**：图旁配「节点 / 边 → `文件:行号`」证据表，由 `scripts/lifecycle-check.py`
  校验（文件存在 · 符号仍在 · README 仍引用 · 表与图的节点集合**按图分别**相等 · id 唯一 ·
  边两端显式定义 · 方言白名单）。
- **已有的五处 ASCII 图一个字不改**：它们各有各的读者，新图引用它们、不取代。

## 为什么

1. **mermaid 的节点集合是机器可读的。** ASCII 图没有可解析的结构，`lifecycle-check.py` 那条
   「表与图的节点集合相等」的检查就无从谈起。图的**可对账性**是选它的首要理由，不是好看。
2. **ASCII 撑不住这个规模。** 24 个节点、跨四层的边，手绘的对齐与维护成本随节点数增长，
   而错一笔没人看得出来。
3. **GitHub 渲染 mermaid**，而这个仓库的读者主要在 GitHub 上读文档。

## 代价

1. **终端里读不到图。** `docs/lifecycle.md` 里的 mermaid 在 fs-agent 自己的 TUI 里只会显示成
   不着色的代码块（`mermaid` 不在 `canonical_language` 里，见 `src/render/highlight.rs:214-228`）。
   要真正在终端里画图是另一个 effort（[`.scratch/tui-mermaid/`](../../.scratch/tui-mermaid/seed.md)），
   本决定不依赖它。
2. **`check-language.py` 的中文占比要单独降低限。** 比例算的是整份文件原文、代码块不剥离，
   满屏 mermaid 关键字会把比例压到远低于邻居。新文档要加进 `DOCS_MIN_RATIO`，下限按实测值
   减 2 取（照 `AGENTS.md` 的先例），并在脚本注释里写明理由。
3. **仓库第一次有第二种图语言。** 已有的五处 ASCII 图不动，于是 `docs/` 里会同时存在两种画法；
   未来的**流程图**跟这一条走，**局部小图**（一行式流水线那种）仍可手绘 ASCII。
4. **多一个护栏脚本要维护**，而且它守不住语义正确性 —— 「通过 ≠ 图是对的」这句写进了它的
   docstring。

## 被否决的替代方案

- **继续手绘 ASCII。** 五张图里最大那张 24 个节点，手绘的对齐与连线靠眼睛保证。**否决。**
- **引外部渲染器（`mmdc` / graphviz `dot`）。** 要 Node + Chromium 或系统 `dot`，破「测试无网络、
  可复现」那条线，而且渲染成功与图对不对无关。**否决。**
- **让 fs-agent 的 TUI 渲染 mermaid 当作前置条件。** 那是产品功能，会被「折行归 `pane::wrap_line`」
  与「`to_lines` 是纯函数」两条线拦住；文档的图不该等它。**否决。**
- **不配证据表、不做脚本。** 图会在下一次重构后悄悄腐烂，而没人知道从哪一行开始。**否决。**
