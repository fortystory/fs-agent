# 12 — 连接层：`rmcp` + 真 stdio / Streamable HTTP

Type: implement
Status: done
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

## 评论

- 2026-10-03 落地（`Status: done`）。落点：`src/mcp/rmcp_client.rs`（新：`connect_all` +
  `RunClient`）、`src/mcp/mod.rs`（`pub mod rmcp_client`、`McpService::with_unavailable` /
  `unavailable_reason`，于是「跳过的那台」报错时带上原因）、`src/cli.rs`（`mcp_service` 变
  async、新增 `mcp_sandbox`：MCP 连接在组装之前建，沙箱可用性得自己探一次）、`Cargo.toml`
  （`rmcp` / `process-wrap` / `http`，以及测试辅助的 `[[bin]] fake-mcp-server`）、
  `tests/support/fake_mcp_server.rs`（新）、`tests/mcp_stdio.rs`（新，5 条）、
  `tests/mcp_list.rs`（+1 条重名）。
- **三件先验的结论**：
  - (a) 进程组包装叠得上：`CommandWrap::from(command)` + `wrap.wrap(ProcessGroup::leader())`，
    `TokioChildProcess::new` 收 `impl Into<CommandWrap>`。实测 `drop(service)` 之后假 server 与它
    自己 spawn 的 `sh` 循环**都停了**（心跳文件的 mtime 不再前进），也就是 `killpg` 收掉了整组。
  - (b) `default-features = false` +
    `["client", "transport-child-process", "transport-streamable-http-client-reqwest",
    "reqwest-native-tls"]` 编译通过（`cargo build` 31 秒）。注意 `reqwest-native-tls` 是**独立的
    一位**：没有 `transport-streamable-http-client-reqwest-native-tls` 这个 feature 名。
  - (c) `RunningService` 的 Drop 是异步取消、不保证同步清理，但实测在这一层够用（见 (a)）。
    `close` 要 `&mut self`，而工具只拿 `&self`，所以没有显式关闭点。
- **生命周期**：固定 `ClientLifecycleMode::Discover { preferred_versions: [LATEST] }`。假 server
  只认旧版时（对 `server/discover` 回 -32601）连接是**硬失败、不回退** —— 测试
  `a_legacy_only_server_is_refused_not_silently_downgraded` 钉住这条。
- **环境传递**：本票只做 `command.envs(&config.env)`（配置里声明的变量进子进程）。`env_clear()`
  + `PATH` / `HOME` / `LANG` 的**白名单**与 `writable_roots`、`stderr` piped 是票 13 的落点。
- **一处实现细节**：`mcp_list` 的 `instructions` 取自握手（`ServerPeerInfo.instructions`），不是
  `tools/list` 的结果 —— 后者在 2026-07-28 里没有这个字段。
- **验证 2 的覆盖范围**：测试是「两台、其中一台起不来」（不是三条），断言好的那台可用、坏的那台
  在 `mcp_list` 里报 `MCP_SERVER_UNAVAILABLE`；「并发起」由 `FuturesUnordered` 实现，测试不测时序。
  验证 5「零网络」：整个 `tests/mcp_stdio.rs` 只起本地子进程。
- **本机环境备注（不进仓库）**：这台沙箱里 `~/.cargo/registry` 是只读挂载，`nix 0.31.3`
  （`process-wrap` 的依赖）解不出来，所以本轮所有 cargo 命令都带
  `CARGO_HOME=$PWD/.git/cargo-home`（把已有缓存复制进工作区一次）。真实终端里不需要这一步。
- 验证：`cargo test --test mcp_list --test mcp_stdio`（13 + 5 条全绿）·
  `cargo clippy --all-targets`（无 warning）· `cargo fmt` · `python3 scripts/check-language.py`
  全通过。

- **审查后的修正（2026-10-03）**：票面要的「三个假 server（一个起不来）」原来只写了两台，
  现在 `one_dead_server_does_not_take_the_others_down` 是三台（两台好的 + 一台起不来的）；
  跨来源重名是覆盖、**同一处**重名是启动错误这条，两个来源各自都验到了（JSON 那一半见票 10）。
