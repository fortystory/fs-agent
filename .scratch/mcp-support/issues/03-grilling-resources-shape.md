# grilling：resources 在元工具方案下的形状

Type: grilling
Status: resolved
Part of: ../map.md
Blocked by: —

## Question

冻结项 5/6 把**工具**收进了两个元工具（`mcp_list` / `mcp_call`）。**资源**（resources）是
第二类原语，它的发起方本来不是模型 —— 但在 fs-agent 里，能让模型看见外部数据的路只有工具。

要定的是它在元工具方案下长什么样：

- `mcp_list` 顺带列资源（`server` 的 tools + resources 一起回）？还是
- 再开两个元工具（`mcp_resources` / `mcp_read`）？还是
- 一个都不开，资源靠「server 的工具」自己暴露（很多 server 已经这么做了）？

同时要碰的两件事：

1. **URI 与 `ReadSet`**：`read_paths` 是调用前的纯函数（[`src/tools/tool.rs`](../../../src/tools/tool.rs)），
   而资源的 URI 是运行时的 —— 「读过」这件事在资源上怎么算，直接关系到「写前必读」那条护栏。
2. **上限与截断**：一份资源可能很大；沿用既有的那条截断流水线（`context::truncate_result` +
   指针），还是需要自己的形状。

## Answer

**再加两个元工具**：`mcp_resources(server?)`（列某台 server 的资源）与 `mcp_read(server, uri)`
（读一份）。元工具从两个变成四个 —— 这**不与冻结项 6 冲突**：那一条讲的是「MCP 的**工具**不进
表」，而资源是另一类原语；它同样不进表，只是多了两个固定名字的入口。

两条由机制定死的边界（不是选择，是既成事实）：

1. **资源不登记进 `ReadSet`**：`ReadSet` 装的是**工作区路径**，而资源是 URI（`db://users/42`
   这类），语义上装不进去。所以「先读后写」那条护栏对资源不适用 —— 与 `grep` 命中文件同一种处理
   （不登记；`edit_file` 前仍要先 `read_file`）。
2. **资源与工作区路径是两套坐标系**：`read_paths` 是调用前的纯函数、走 `resolve_read` 的工作区
   解析；资源 URI 由 server 定义，完全在那一套之外。两者不互操作，也不该互操作。

**上限**：走既有那条唯一的截断流水线（`context::truncate_result` + 指针），不新开一套；
`mcp_read` 的结果与工具结果同一条路进流，所以 `sessions replay` 照样复盘。

**不可信标记**：资源的正文与工具结果一样带那句中文标记（冻结项 11）—— server 给的数据与 server
给的工具结果没有信任差别。
