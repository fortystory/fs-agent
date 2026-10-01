# 种子材料：接入 MCP

> **这不是 spec，也不是票。** 它是 2026-10-01 在一轮 `/ask-matt` 里记下的意向：支持接入 MCP
> server，把它们的能力变成 fs-agent 的工具。
> 还没被访谈、也没有票；想推进时走 `/grill-with-docs` 把它折成 `spec.md`，再 `/to-tickets` 拆票。
> **写就于 2026-10-01**；下面《现状》一节核实于同一天。

## 它要什么

模型能调用外部 MCP server 提供的工具/资源，而不是只能用手写的那几个内建工具。

## 现状

没有：`grep '\bmcp\b|MCP'` 在 `src/` 里零命中。已有的挂载点是**动态声明工具**：配置里的
`ToolDeclaration`（`src/config.rs`）经 `src/tools/custom.rs` 变成工具，唯一的建表点在
`src/tools/mod.rs` 的 `with_dynamic`。

**要正面处理的那条不变量**：工具表是缓存前缀的一部分，建表之后**不再变化**（`src/tools/mod.rs`
顶部注释）。MCP server 的工具如果只能在运行时才发现，就和这条撞上了。

## 待谈的分叉

1. MCP 工具在启动时就全量进表，还是延迟发现（延迟就要动上面那条不变量，或者接受缓存前缀失效）。
2. 外来工具的副作用分类（`Effect`）与权限怎么给 —— 默认最严还是按 server 声明？
3. MCP server 进程归谁管：生命周期、崩溃重连、要不要过沙箱（`src/tools/sandbox.rs`）。
4. 配置形状：`.mcp.json` 那种独立文件，还是并进现有配置。
