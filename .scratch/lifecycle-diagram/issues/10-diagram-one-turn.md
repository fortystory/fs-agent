# 10 — 图 3：一次 turn（`sequenceDiagram`）

Type: implement
Status: done
Part of: ../map.md
Blocked by: 06

> 规格：[`../spec.md`](../spec.md) §2（图 3）。图源是
> [`../prototype/01-drafts.md`](../prototype/01-drafts.md) 的第三张，证据在
> [`../research/04-node-evidence.md`](../research/04-node-evidence.md) §3。

## 目标

`docs/lifecycle.md` 的「一次 turn」一节有图 3（`sequenceDiagram`，5 个参与者）与它的证据表。读者能
回答：一句提示进来之后，上下文怎么组装、provider 怎么调、工具调用按什么**固定顺序**走完、
回边长什么样。

## 现状（2026-10-03 核实，改前先复核）

- **图已改过两处**：① 消息方向 —— 门由**循环**在 `process_call` 里调、工具表在门之后，原稿写成
  `tools->>gate` 是反的；② `hook.pre` / `hook.post` **从两行消息降级为一条 `Note`** —— 两个挂载点
  在每个生产组装点都传 `None`，占两行会把「可注入但没人插」画成常规步骤。
- **这张图不进 C4/C5 对账**（`sequenceDiagram` 不在 v1 解析器的方言里，见 spec §6）；它的证据表仍然
  要写，但由人核。
- **已有的简版**：[`README.md:221-223`](../../../README.md) 那一行
  （`hook.pre → 权限门 → [询问] → dispatch → hook.post → 追加事件`）是同一件事的一行式版本 ——
  **引用它、不要删它**；本图是它的展开。
- 固定顺序是仓库的**不变量 5**，`docs/permissions.md` 与 `docs/hooks.rs` 的注释都提到它。

## 落点

`docs/lifecycle.md` 的「一次 turn」一节与附录里对应的一段。

## 具体行为

1. **图 3** 照图源落地（5 个参与者：人 / 循环 / provider / 权限门 / 工具表）。
2. **证据表**：5 个参与者 + 关键消息，内容取 `research/04` §3。
3. **正文要写清三件事**：① 固定顺序里两个 hook 步骤**在 CLI 下恒不发生**（表是 `None`，只有库
   调用方与测试挂得上）；② `权限门` 是**抽象参与者** —— 没有对象持有「询问 → 人答」这次往返，
   证据只落到 `authorize` 与 `ConsoleAsker`；③ 「还有挂着的 `tool_call` 就不调 provider」是**守卫**、
   不是步骤。
4. **「细则在哪」**指向：`docs/permissions.md`、`docs/observability.md`（投影与流）、
   `docs/executor.md`（`TurnScope` 那三种窗口）、`docs/bash.md`（工具侧）。

## 验证

1. 图 3 在 GitHub 上渲染出来；参与者数与图源一致（5）。
2. 每个参与者在证据表里有且只有一行；证据的符号真实存在。
3. 正文里出现「CLI 下两个 hook 步骤不发生」这句。
4. `README.md:221-223` 那一行**仍然在**，没有被本图取代。

## 不做什么

- 不动图 1、图 2、图 4、图 5。
- 不给 `sequenceDiagram` 写解析器（本图靠人读）。
- 不写 §7（归票 13）。
- 不把 provider 的错误码与流式细节画进图里（那是 `docs/observability.md` 的事）。
