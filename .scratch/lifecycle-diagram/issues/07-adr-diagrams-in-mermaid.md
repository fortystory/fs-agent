# 07 — ADR 0011：文档里的流程图用 mermaid

Type: implement
Status: done
Part of: ../map.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §7。**完整草稿已经写在
> [票 05 的 `## 作答`](05-grilling-doc-skeleton-and-adr.md) 里** —— 本票是把它落成文件，
> 不是重新论证。

## 目标

`docs/adr/0011-diagrams-in-mermaid.md` 存在，未来读者能从中回答两件事：**为什么只有
`docs/lifecycle.md` 不是 ASCII**，以及这条决定付了什么代价。

## 现状（2026-10-03 核实，改前先复核）

- **仓库今天零 mermaid 图**：`grep -rn "mermaid" README.md CONTEXT.md docs/` 只有一处命中，且是讲
  某个 crate 包体大小的旁白（`docs/adr/0008-markdown-parsing-by-pulldown-cmark.md:24`）。
- **已有的五处 ASCII 图**：`README.md:221-223`、`docs/executor.md:10-21`、`docs/credentials.md:22-29`、
  `docs/web.md:14-22`、`docs/render.md:95-103`。它们各有一个读者，本票**不动它们**。
- **ADR 的格式与语言规矩**：照邻居 `docs/adr/0010-questionnaire-keys-dispatch-by-zone.md` 的四节
  （`## 决定` / `## 为什么` / `## 代价` / `## 被否决的替代方案`）；`check-language.py` 的 ③ 要求 ADR 的
  标题与小标题都是中文（`scripts/check-language.py:193-218`）；ADR 也进 `DOCS_MIN_RATIO`。
- **README 的 ADR 索引**是那段逐条罗列（`README.md:241`）。

## 落点

`docs/adr/0011-diagrams-in-mermaid.md`（新）、`README.md` 的 ADR 索引。

## 具体行为

1. **四节结构**，内容照票 05 的草稿：
   - Context：零 mermaid 现状 + 五处 ASCII 手工图 + 新图的规模（五张、最大 24 节点）。
   - 决定：`docs/` 的**流程图**用 mermaid；方言受约束、写在 `docs/lifecycle.md` §1；虚线只有一个
     语义；图与代码靠证据表 + `scripts/lifecycle-check.py` 对账；**已有五处 ASCII 一个字不改**。
   - 为什么：**首要理由是「图的节点集合是机器可读的」** —— ASCII 图没有可解析的结构，C4/C5 那条
     「表与图的节点集合相等」根本无从谈起。其次才是 GitHub 渲染与手绘成本。
   - 代价四条：终端读不到图 · `DOCS_MIN_RATIO` 要单独降低限 · 仓库第一次有第二种图语言 ·
     多一个护栏脚本要维护且它守不住语义。
   - 被否决的替代方案：继续手绘 ASCII · 引外部渲染器（mmdc / dot）· 以「TUI 能渲染 mermaid」为
     前置条件 · 不配证据表也不做脚本。
2. **代价第一条要写实**：`mermaid` 不在 `canonical_language` 里（`src/render/highlight.rs:214-228`），
   fs-agent 自己的 TUI 只把它当普通代码块渲染；要真正在终端画图是另一个 effort，指向
   [`../tui-mermaid/seed.md`](../../tui-mermaid/seed.md)。
3. `README.md` 的 ADR 索引加一条。

## 验证

1. `python3 scripts/check-language.py` 通过（ADR 的标题与小标题是中文；中文占比不低于它的下限）。
2. README 的 ADR 索引链接可达。
3. 四节齐全，且「为什么」的第一条是「机器可读」，不是「好看」。
4. 代价第一条指向 `.scratch/tui-mermaid/`，而不是含糊其辞。

## 不做什么

- 不改 ADR 0001–0010。
- 不在这一票里落地任何图（归票 06、09–12）。
- 不把实现细节写进 ADR（七条校验那类归 `docs/lifecycle.md` §1 与脚本）。
