# 12 — 连接层：`rmcp` + 真 stdio / Streamable HTTP

Type: implement
Status: ready-for-agent
Part of: ../map.md
Blocked by: 10

> 规格：[`../spec.md`](../spec.md) §3。事实依据：[`../research/02-rmcp-fit.md`](../research/02-rmcp-fit.md)
> （`rmcp` 3.5.0 的契合度与三处未确认项）。

## 目标

把票 10 的假连接换成真的：引入 `rmcp`，用 `Discover` 生命周期连上本地 stdio server 与远端
Streamable HTTP server；多台并发起、失败的跳过、重名在启动时拒绝。

## 具体行为

1. **三件先验（先做，调研标了未确认）**：
   - (a) 能不能在 `CommandWrap` 上叠 process-wrap 的**进程组包装**，拿回 `process_group(0)` +
     `killpg` 的整组语义；叠不了就回到「自己 spawn、把句柄交给它」的接法；
   - (b) `default-features = false` 的 feature 组合与体积（编译验证）；
   - (c) `RunningService` 的 Drop 语义是否符合预期（见第 5 条）。
2. **引入**：`rmcp` 3.5.0、`default-features = false`、**TLS 用 `reqwest-native-tls`** —— 选
   `reqwest` 会把它换成 rustls，等于推翻 `Cargo.toml` 里那条既有决定。
3. **生命周期固定 `ClientLifecycleMode::Discover`**：只走 `server/discover`、不回退（旧版 server
   直接报错）。
4. **传输**：stdio 把 `Sandbox::wrap` 包好的 argv 交给 `TokioChildProcess`；远端走 Streamable HTTP。
   **不做 legacy HTTP+SSE**。
5. **关闭走 Drop 兜底**（`close` 要 `&mut self`，而 `Tool` 只给 `&self`）；**不自动重连**（崩了同
   会话内不重试）。
6. **并发起、失败跳过、不做数量上限、重名是启动错误**（消息里点名那个键）。
7. **真 stdio 集成测试**：一个测试用的**假 MCP server 二进制**（仓库自己的测试辅助程序），走真
   spawn + 沙箱包装 + 协议帧，覆盖假连接盖不到的那条路。

## 验证

`cargo test` + `cargo clippy --all-targets` + `python3 scripts/check-language.py`：

1. **Discover**：假 server 走 `server/discover`；一个只认旧版 `initialize` 的假 server 被**明确
   拒绝**（不是静默降级）。
2. **并发起与失败跳过**：三个假 server（一个起不来），断言其余两个可用、失败那台在 `mcp_list` 里
   报结构化错误。
3. **重名**：配置里两个同名 server → 启动报错。
4. **真 stdio**：假 server 起得来、`mcp_list` 拿到它声明的工具、`mcp_call` 调得动并拿回结果、
   **会话结束时那个进程被收掉**（整组，不是只杀直接子进程）。
5. **零网络**：除 stdio 假 server 外不发任何真实请求。

## 不做什么

- 不做环境白名单与可写根（票 13）、不做 `Effect` 与信任（票 14）、不做 MRTR（票 15）。
