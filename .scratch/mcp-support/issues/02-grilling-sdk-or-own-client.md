# grilling：自己写 client 还是用 `rmcp`

Type: grilling
Status: resolved
Part of: ../map.md
Blocked by: 01

## 问题

决定了冻结项 5–8、18、19（元工具方案、默认最严的 `Effect`、stdio + Streamable HTTP、只谈
2026-07-28）之后，最后一根是**谁来发那些请求**：

- **用 `rmcp`**：双生命周期与 MRTR 的驱动是白送的，代价是一个新依赖树、以及把它的异步模型
  接进 fs-agent（它是 server-first 的 crate，只做 client 要 `default-features = false`）。
- **自己写**：请求面其实很窄 —— 无状态 `_meta`、`server/discover`、`tools/list`、`tools/call`、
  stdio 帧与 Streamable HTTP 的会话头、MRTR 重试回路。仓库已经有 `reqwest`，而 stdio 那条
  可以复用 [`src/tools/process.rs`](../../../src/tools/process.rs) 的形状。

先读上一条票的答案（契合度事实），再在 live exchange 里拍板。拍板要连同**理由**一起写进
`## 作答`，因为这是一个以后不容易回头的选择（换 SDK 要重写 client 层）。

## 作答

**用 `rmcp` 3.5.0，`default-features = false`，TLS 选 `reqwest-native-tls`。** 理由（详据见
[`../research/02-rmcp-fit.md`](../research/02-rmcp-fit.md)）：① `ClientLifecycleMode::Discover`
就是「只走 `server/discover`、不回退」，与冻结项 19 逐字对齐（源码注释：「Discover mode does
not fall back; a legacy server is an error.」）；② `call_tool` 自带 MRTR 驱动，elicitation 落在
可 override 的 `create_elicitation` 上，正是冻结项 16 要的挂点；③ 依赖净增约 4 包（rmcp、
process-wrap、sse-stream、tokio-stream）。**TLS 必须选 `reqwest-native-tls`** —— 选 `reqwest`
会把它换成 rustls，等于推翻 [`Cargo.toml`](../../../Cargo.toml) 的既有决定。

三个取舍一并定下：

1. **子进程保留「杀整组」的纪律**：在 `CommandWrap` 上叠 process-wrap 的进程组包装，拿回
   `bash` 那条纪律（`process_group(0)` + `killpg`）。**落地第一步先验能不能叠**；叠不了就回到
   「自己 spawn、把句柄交给 rmcp」的接法（调研 §3 给了 `Sandbox::wrap` 拼接的可行形状）。
2. **问询端口改成可共享的**：`UserQuestions` 变成 `Arc<dyn UserQuestions + Send + Sync>`，
   `ToolContext.questions` 跟着换形状 —— 一处改动，**不给 MCP 单开平行通道**（两套实现会漂）。
3. **关闭走 Drop 兜底**：`RunningService` 的 Drop 自动 cancel，不去 await
   `close_with_timeout`（它要 `&mut self`，而 `Tool` 只给 `&self`）。与冻结项 14 的「不自动
   重连」一致。

**它约束了后面哪些票**：

- **票 06（server 进程环境）多一条硬输入**：`rmcp` **不做环境清洗**，DSH 那种
  `scrubbedParentEnv()` 在它这侧没有对应物 —— 擦洗必须由 fs-agent 自己做（`env_clear()` /
  白名单后再交给它）；`stderr` 默认 `inherit`，要 `.stderr(Stdio::piped())` 才拿得回句柄。
- **capabilities**：要让 server 真的发 elicitation，client 得在能力里 `enable_elicitation()`；
  `Discover` 生命周期下这些随每请求 `_meta` 送出。
- **降级出口**：`call_tool_once` 是「自己驱动 MRTR」的现成出口，某类 elicitation 一时接不上时
  它是退路。
- **落地先验**：feature 组合与体积**未编译验证**（调研禁 cargo），引入时先跑一遍。
