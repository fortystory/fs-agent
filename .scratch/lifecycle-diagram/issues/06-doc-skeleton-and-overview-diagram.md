# 06 — 文档骨架 + 画法约定 + 鸟瞰图（tracer bullet）

Type: implement
Status: done
Part of: ../map.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1、§2（图 1）、§3、§4、§8。本票打通「一份新文档 → 第一张图 →
> 它的证据表 → 两个索引 + 语言护栏」整条路；其余四张图归[票 09](09-diagram-boot-and-shutdown.md)–
> [12](12-diagram-infrastructure.md)，护栏脚本归[票 08](08-lifecycle-check-script.md)。

## 目标

`docs/lifecycle.md` 存在，读者打开它能看到：这张文档是什么、怎么读、图按什么约定画；以及
**第一张图 —— 鸟瞰总图**，配一份能点到代码的节点/边证据表。`README.md` 的「架构」一节与
`## 文档` 表各指向它。`python3 scripts/check-language.py` 仍然全绿。

## 现状（2026-10-03 核实，改前先复核）

- **图源已定形**：[`../prototype/01-drafts.md`](../prototype/01-drafts.md) 的图 1（14 节点 / 19 边）。
- **逐面文档的先例**：`docs/executor.md` / `docs/render.md` —— 一个面一份，讲它怎么工作、以及为什么。
- **README 有两处索引要动**：「架构」一节（`README.md:219-232`）与 `## 文档` 表（`README.md:234-244`）；
  tracker 侧的第三处在 `.scratch/README.md` 的 feature 表。
- **语言护栏**：`scripts/check-language.py:151-180` 的 `DOCS_MIN_RATIO`；注意 `check_docs()` **只遍历
  这个字典的键、不扫 `docs/*.md`**（`scripts/check-language.py:345-355`）—— 不登记就完全不受管。
- **「细则在哪」的目标**：`docs/observability.md`、`docs/executor.md`、`docs/goals.md`、
  `docs/permissions.md`、`docs/sandbox.md`、`docs/render.md`、`docs/web.md`、`docs/credentials.md`。

## 落点

`docs/lifecycle.md`（新）、`README.md`、`scripts/check-language.py`、`.scratch/README.md`。

## 具体行为

1. **骨架**（照 spec §8）：开头一段（这张文档是什么 / 怎么读 / 与逐面文档的分工）→ §1 画法约定 →
   图 1 一节 → 其余四节的占位标题 → §7 的占位标题 → 附录（图 1 的节点表与边表）。
2. **§1 画法约定**：受约束的方言（逐条列出禁止项）、先显式定义后引用、虚实判据、虚线只表示哪两类、
   四种不该画虚线的情形、只有 `seed.md` 的 roadmap 不进图、图编号。**判据全文抄
   [`../research/02-dashed-inventory.md`](../research/02-dashed-inventory.md) §2。**
3. **图 1**：照图源落地（`flowchart TD`，14 节点）；节点用 `CONTEXT.md` 的正式用词。
4. **证据表**：图 1 的节点表（节点 id · 一句话 · `路径:行号` · 符号）与边表，内容取
   [`../research/04-node-evidence.md`](../research/04-node-evidence.md) §1。
5. **每节除图之外**要有「这张图在讲什么」+ 一张「细则在哪」的指向表。
6. **`README.md` 两处**：「架构」一节加一行指向它；`## 文档` 表加一行。
7. **`DOCS_MIN_RATIO` 加一行**：下限按**实测值 − 2**（照 `AGENTS.md: 23` 的先例），并在脚本注释里写明
   理由（这份文档图密、mermaid 关键字密）。实测值在写完之后量。
8. **`.scratch/README.md`** 的 feature 行改成「已折成 spec、实现票 06–14」。

## 验证

1. `python3 scripts/check-language.py` 通过（新文档已进清单，且中文占比不低过下限）。
2. README 两处链接可达：`grep -n "docs/lifecycle.md" README.md` 至少两处命中。
3. 图 1 的每个节点在证据表里都有且只有一行；每行的 `路径:行号` 与符号都真实存在。
4. 图 1 在 GitHub 上渲染出来（人看；语法是 mermaid 标准的）。

## 不做什么

- 不写另外四张图（归票 09–12）。
- 不写 `scripts/lifecycle-check.py`（归票 08）—— 本票的证据表靠人核。
- 不写 ADR（归票 07）。
- §7 只留占位标题，正文归票 13。
- 不动 `docs/` 里任何已有文档。
