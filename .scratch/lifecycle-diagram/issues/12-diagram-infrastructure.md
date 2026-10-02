# 12 — 图 5：基础设施回路

Type: implement
Status: done
Part of: ../map.md
Blocked by: 06

> 规格：[`../spec.md`](../spec.md) §2（图 5）。图源是
> [`../prototype/01-drafts.md`](../prototype/01-drafts.md) 的第五张，证据在
> [`../research/04-node-evidence.md`](../research/04-node-evidence.md) §5。

## 目标

`docs/lifecycle.md` 的「基础设施回路」一节有图 5（`flowchart TD`，18 节点 / 19 边）与它的证据表。
读者能回答：事件流怎么成为唯一真相源（写路径、投影、裁剪、持久化、重建各是谁），以及两类问询
发起者与三个渲染器怎么接在同一根通道上。

## 现状（2026-10-03 核实，改前先复核）

- **图已改过两处**：① `roll` 节点删掉了（阈值判定不是跨阶段节点，是 `goals::threshold_step` 在目标
  循环里的一步），改成边标签「过阈值」；② `headless` **从虚线改成实线** —— 它在 `probe` 子命令里真被
  构造（`src/cli.rs:2238`，在 `#[cfg(test)]` 之外），侦察报告原来那句「`cli.rs` 从不构造它」是错的。
- **两条回路要分开讲**：上面那条是**每回合**回路（事件流 → 投影 → 裁剪 → 请求），下面 `goal ↔ compact`
  是**跨会话**回路（过阈值才走）。画在一起容易读成一条。
- **已有的局部图**：[`docs/credentials.md:22-29`](../../../docs/credentials.md)（入流流水线树）与
  [`docs/render.md:95-103`](../../../docs/render.md)（TUI 外壳几何）—— 引用它们，**不要动**。
- `ask` 与 `uq` 是**两类发起者**，走两条不同端口（`Asker` 对 `QuestionnaireRequest`），合并成一个
  下游节点是简化，正文要说清。

## 落点

`docs/lifecycle.md` 的「基础设施回路」一节与附录里对应的一段。

## 具体行为

1. **图 5** 照图源落地（18 节点）。
2. **证据表**：节点表 + 边表，内容取 `research/04` §5。
3. **正文要写清四件事**：① 两条回路的区别（每回合 vs 跨会话）；② `append_event` 是唯一写路径、
   打码在其**前**（于是「流上文本 == 模型看到的文本」）；③ 两类问询发起者不共用接缝；④ `headless`
   的生产组装点只有 `probe`（不是虚线，也不要写成「CLI 不构造」）。
4. **「细则在哪」**指向：`docs/observability.md`、`docs/goals.md`、`docs/credentials.md`、
   `docs/render.md`、`docs/permissions.md`（询问）、`docs/skills.md`（钉住注入）。

## 验证

1. 图 5 在 GitHub 上渲染出来；节点数与图源一致（18）。
2. 每个节点在证据表里有且只有一行；边两端都在节点表里。
3. 图里**没有虚线**（两条虚线：一条在图 4，一条已被这条修正推翻）。
4. 正文里没有把 `headless` 说成「CLI 不构造」。

## 不做什么

- 不动图 1–4。
- 不写 §7（归票 13）。
- 不把 TUI 的绘制细节或目标循环的完整状态机展开（那是 `docs/render.md` 与 `docs/goals.md` 的事）。
- 不重画 `docs/credentials.md` 与 `docs/render.md` 里已有的局部图。
