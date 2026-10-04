# research：`rmcp` 3.5.0 的契合度

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

要用官方 Rust SDK（`rmcp` 3.5.0）来接 MCP，还是自己写 client？在拍板之前先把**契合度**查清
—— 不是「它能不能用」，而是「它嵌进 fs-agent 要付多少代价」。

已知（来自 [`../research/01-mcp-client-implementation.md`](../research/01-mcp-client-implementation.md)）：
`rmcp` 3.5.0、Apache-2.0、2026-09-28 发布、tokio 原生、stdio + Streamable HTTP、自带
`server/discover` + legacy 双生命周期与 MRTR 自动驱动、不提供 legacy HTTP+SSE、**默认 features
会拉 server 侧，只做 client 要 `default-features = false`**。仓库 `Cargo.lock` 里 MCP 相关零命中。

要查清并写下来的：

1. **只做 client 时的依赖树与体积**：`default-features = false` 之后拉进哪些 crate、大致多少个
   lock 包、与仓库现有依赖（`tokio` / `reqwest` / `serde` / `serde_json`）有多少重叠。
2. **API 形状**：连接一台 stdio server 与一台 Streamable HTTP server 各要几行、异步模型是什么
   （`ClientHandler` trait？`Service`？）、能不能**不**起它自己的 runtime、错误类型是什么形状。
3. **与 fs-agent 的接法**：把「调用一个工具」变成一次 `async fn call(server, tool, args)` 的
   难度；`Tool` trait 是 `async_trait`（[`src/tools/tool.rs`](../../../src/tools/tool.rs)），
   两者的 `Send + Sync` 要求冲不冲突；server 的进程管理归谁（`rmcp` 自己 spawn 还是我们给它
   一个 `Child`）。
4. **MRTR 与 elicitation**：它「自动驱动」到什么程度 —— 能不能挂一个回调把
   `input_required` 接到 fs-agent 的问询端口上（冻结项 16 选了这条）？
5. **`server/discover` 与只谈最新版**：能不能只走 2026-07-28 那条路、显式关掉 legacy 回退
   （冻结项 19 选了只谈最新版）？
6. **不用的代价**：若自己写，要自己实现哪几件（无状态 `_meta` 请求、`server/discover`、
   `tools/list` 与 `tools/call`、stdio 帧与 Streamable HTTP 的会话头、MRTR 的重试回路、
   进度/取消通知）—— 给一个「大概多少代码」的量级估计，别估精确行数。

## 作答

推荐 `rmcp` 3.5.0（`default-features = false`）。① `Discover` 只用 `server/discover`、不回退，
合冻结项 19；② `call_tool` 自带 MRTR，elicitation 走 `create_elicitation` 回调，但要
`'static`，与借用的 `questions` 端口不兼容；③ 依赖几乎全在 lock，净增 4 包，stdio 仍可过
`Sandbox::wrap`。见 [`../research/02-rmcp-fit.md`](../research/02-rmcp-fit.md)。
