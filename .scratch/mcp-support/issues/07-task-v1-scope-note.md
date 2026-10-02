# task：v1 的范围补记

Type: task
Status: resolved
Part of: ../map.md
Blocked by: —

## Question

这不是一个要 decide 的问题，而是一件**必须先做掉的手续**：MCP client 在 v1 里被三处明文划出去，
而在实现之前，那三处都要有一条**带日期的补记**（原文不改写 —— 它是当时的理由，照
[`README.md`](../../../README.md) 里 `sandbox` 的先例）。

要动的落点（2026-10-03 核实）：

1. [`.scratch/fs-agent-v1/spec.md`](../../fs-agent-v1/spec.md) 的 `:449`（「必须有超时与进程树
   终止。明确不做：MCP client 本身。」）2. 同一份的 `:636`（「**证据型砍掉项**（判据变化不翻它们）：……**MCP client**……」）
3. 同一份的 `:667`（「**实现者请勿「顺手改进」**：向量检索、MCP……要动它们，先改这张 spec」）
4. [`README.md`](../../../README.md) 的「这一版不做」清单里 `MCP client` 那一处

补记要写清四件事：**什么时候**、**因为什么变了**（本图的 Destination 与那条「另起 effort」的
决定）、**范围到哪**（tool / resource / prompt / elicitation + MRTR，不含已 deprecated 的
sampling / roots）、以及**指向本图**。

做完之后：票底记下四处补记的确切位置（供后续票引用），并把本图 `## Decisions so far` 追加一行。

## Answer

四处落点都已改完（2026-10-03）：

1. [`fs-agent-v1/spec.md`](../../fs-agent-v1/spec.md) **§14 自定义工具**那一节的「明确不做：MCP
   client 本身」—— 其后加了一条带日期的补记，指向本图。
2. 同一份的 `Out of Scope` · **证据型砍掉项** —— 在那一串里点名 MCP client 之后加补记：它不再属于
   这份清单，其余各项照旧、「判据变化不翻它们」对剩下的项仍然有效。
3. 同一份的 `Further Notes` · **实现者请勿「顺手改进」** —— 补记说明 MCP 已经走了正门，这条禁令
   对剩下的几项（向量检索、裁判、进程沙箱升级）仍然成立。
4. [`README.md`](../../../README.md) 的「这一版不做」—— 清单里**移除**了 `MCP client`，并在
   `compaction` 那段的邻居位置新加一段说明（照同一个先例）。

**原文一个字没有改写**：三处 spec 的原句都留着，补记是追加的 —— 它们是当初排除 MCP 的理由，
抹掉就丢了那段推理。
