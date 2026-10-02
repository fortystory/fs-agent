# grilling：文档骨架、ADR 0011 与语言护栏

Type: grilling
Status: resolved
Part of: ../map.md
Blocked by: 01

## Question

图定形之后，剩下的都是「这份文档怎么落地」的拍板问题。**HITL**：这些要跟维护者逐个过。

要定的：

1. **`docs/lifecycle.md` 的骨架**：几节、什么顺序、每节配什么；五张图怎么排（鸟瞰在前还是在后）；
   「细则在哪」的指向表长什么样；要不要给图编号（别的文档好引用「图 3」）。
2. **ADR 0011 的内容**：标题、Context（仓库零 mermaid 的现状 + 5 处 ASCII 邻居）、Decision
   （docs 的流程图用 mermaid）、Consequences（终端不可读、语言护栏要单独设线、未来文档跟这条走）。
   照 [`docs/adr/`](../../../docs/adr/) 里邻居的格式写。
3. **语言护栏的落法**（冻结项 13）：`DOCS_MIN_RATIO` 加一行、下限数值怎么定 —— 要等文档写出来才有
   实测值，所以这里定的是**方法**（实测值 − 2，照 `AGENTS.md: 23` 的先例），以及脚本注释写什么。
4. **`CONTEXT.md` 要不要收「生命周期图」这个词**（`/domain-modeling` 的口径）。
5. **README 三处落点**：「架构」一节那一行指向、`## 文档` 表那一行、
   [`.scratch/README.md`](../../README.md) 的 feature 行。
6. **`docs/lifecycle.md` 要不要拆**：见 map 的 `Not yet specified` 第一条。

## 要咨询的 skills

`/grilling`（本票的主技能）与 `/domain-modeling`（第 4 条）。

## 交付

答案落进票底 `## Answer`：骨架大纲、ADR 0011 的完整草稿、其余每条的决定。

## Answer

（2026-10-03，与维护者的 live exchange）六条全部采纳。图与证据表已经定形
（[`prototype/01-drafts.md`](../prototype/01-drafts.md) v2、[`research/04-node-evidence.md`](../research/04-node-evidence.md)）。

### 1. `docs/lifecycle.md` 的骨架

```
开头   这张文档是什么、怎么读、与 docs/ 逐面文档的分工（谁是细则、谁是骨架）
§1     画法约定（方言 · 虚实 · 先定义后引用 · 对账怎么做）
§2     鸟瞰（图 1）
§3     进程启动与收尾（图 2）
§4     一次 turn（图 3）
§5     委派与嵌套（图 4）
§6     基础设施回路（图 5）
§7     图上不能断的边 —— research/01 §5 那 12 条不变量 → 图上的边的映射
附录   逐图证据表（每张图一节：节点表 + 边表）
```

鸟瞰在最前（onboarding 读者先要全局），证据表在最后（查阅用），**§7 单独成节**是 Q3(b)「变更影响
分析」那半个用途的落点。每节除图之外还要有：一段「这张图在讲什么」+ 一张「细则在哪」的指向表
（指向 `docs/executor.md` / `docs/permissions.md` / `docs/sandbox.md` / `docs/goals.md` /
`docs/render.md` / `docs/observability.md` / `docs/credentials.md` / `docs/web.md` 的对应节）。

### 2. 图编号

正文给每张图**稳定编号**（图 1–5）+ 小标题，别的文档可以引用「见 `docs/lifecycle.md` 图 3」。
mermaid 自己不生成编号，编号写在图的标题行里。

### 3. 画法约定住在哪

写在 `docs/lifecycle.md` 的 **§1**（写文档的人要在视野里看到它）；
`scripts/lifecycle-check.py` 的 docstring 只写一句 + 指向该节。**不做两份全文** —— 两份必然漂移。

### 4. ADR 0011 的草稿

落地时写成 `docs/adr/0011-diagrams-in-mermaid.md`，照邻居（`docs/adr/0010-*.md`）的四节结构：

```markdown
# 文档里的流程图用 mermaid

`docs/` 今天没有一张 mermaid 图：五处流程 / 结构图**全是手绘 ASCII**（`README.md:221-223`、
`docs/executor.md:10-21`、`docs/credentials.md:22-29`、`docs/web.md:14-22`、`docs/render.md:95-103`）。
新写的 `docs/lifecycle.md` 要画 fs-agent 的运行时生命周期 —— 一张鸟瞰 + 四张分层详图，最大一张
24 个节点。手绘 ASCII 撑不住这个规模。这一篇记的是那条线上的决定与它的代价。

规格在 `.scratch/lifecycle-diagram/`。

## 决定

> `docs/` 里的**流程图**用 mermaid 写；`docs/lifecycle.md` 是第一种。

- 只针对**流程图**；ADR 的标题与小标题仍照 ADR 0004 用中文，不受影响。
- 图的**方言受约束**（`flowchart` + `id[文本]` / `id{文本}` + `-->` / `-.->` / `-->|文本|`），
  写在 `docs/lifecycle.md` §1；`scripts/lifecycle-check.py` 按它解析，不认识的构造报红。
- **虚线只有一个语义**：可注入但 CLI 不注入 / 尚未实现。异常路径（两下 `Ctrl-C`、panic）用边标签。
- 图与代码**对账**：图旁配「节点 / 边 → `文件:行号`」证据表，由 `scripts/lifecycle-check.py` 校验
  （文件存在 · 符号仍在 · README 仍引用 · 表与图的节点集合**按图分别**相等 · id 唯一 ·
  边两端显式定义 · 方言白名单）。
- **已有的五处 ASCII 图一个字不改**：它们各有各的读者，新图引用它们、不取代。

## 为什么

1. **ASCII 撑不住这个规模**：24 个节点、跨四层的边，手绘的维护成本随节点数平方增长，而错一笔
   没人看得出来。
2. **GitHub 渲染 mermaid**，而这个仓库的读者主要在 GitHub 上读文档。
3. **mermaid 的节点集合是机器可读的**：ASCII 图没有可解析的结构，`lifecycle-check.py` 那条
   「表与图的节点集合相等」的检查就无从谈起。图的**可对账性**是选它的首要理由，不是好看。

## 代价

1. **终端里读不到图。** `docs/lifecycle.md` 里的 mermaid 在 fs-agent 自己的 TUI 里只会显示成
   不着色的代码块（`mermaid` 不在 `canonical_language` 里，见 `src/render/highlight.rs:214-228`）。
   要真正在终端里画图是另一个 effort（`.scratch/tui-mermaid/`），本决定不依赖它。
2. **`check-language.py` 的中文占比要单独降低限。** 比例算的是整份文件原文、代码块不剥离
   （`scripts/check-language.py:352`），满屏 mermaid 关键字会把比例压到远低于邻居。新文档要加进
   `DOCS_MIN_RATIO`，下限按实测值 − 2 取（照 `AGENTS.md: 23` 的先例），并在脚本注释里写明理由。
3. **仓库第一次有第二种图语言。** 已有的五处 ASCII 图不动，于是 `docs/` 里会同时存在两种画法；
   未来的**流程图**跟这一条走，**局部小图**（一行式流水线那种）仍可手绘 ASCII。
4. **多一个护栏脚本要维护**，而且它守不住语义正确性 —— 「通过 ≠ 图是对的」这句要写进它的 docstring。

## 被否决的替代方案

- **继续手绘 ASCII。** 五张图里最大那张 24 个节点，手绘的对齐与连线靠眼睛保证。**否决。**
- **引外部渲染器（`mmdc` / graphviz `dot`）。** 要 Node + Chromium 或系统 `dot`，破「测试无网络、
  可复现」那条线，而且渲染成功与图对不对无关。**否决。**
- **让 fs-agent 的 TUI 渲染 mermaid 当作前置条件。** 那是产品功能，会被「换行归
  `pane::wrap_line`」与「`to_lines` 是纯函数」两条线拦住；文档的图不该等它。**否决。**
- **不配证据表、不做脚本。** 图会在下一次重构后悄悄腐烂，而没人知道从哪一行开始。**否决。**
```

### 5. `CONTEXT.md` 不收「生命周期图」

它是一份**文档的名字**，不是领域概念。`CONTEXT.md` 是词汇表，文档索引在 `README.md`；
收进去会让它变成第二个索引。**决定：不收**（这一条没有要改的文件）。

### 6. 文档不拆

五张图 + 证据表在同一份 `docs/lifecycle.md` 里。`lifecycle-check.py` 要同时读图和表才能比 C4/C5，
拆开就得跨文件解析；而篇幅问题目前**不存在**（估：五张图 + 表约 300–400 行，与
`docs/executor.md`（111 行）同量级偏大，但远没到要拆的地步）。

### 落地清单（交给 `/to-spec` 之后）

1. 新建 `docs/lifecycle.md`（§1 骨架 + 五张图 + §7 + 附录证据表）。
2. `README.md` 的「架构」一节加一行指向它；`## 文档` 表加一行。
3. `.scratch/README.md` 的 feature 行改成「已折成 spec」。
4. `docs/adr/0011-diagrams-in-mermaid.md`（上面那份草稿）。
5. `scripts/lifecycle-check.py`（设计见[票 03](03-research-lifecycle-check.md)）。
6. `scripts/check-language.py` 的 `DOCS_MIN_RATIO` 加一行（实测值 − 2）。
