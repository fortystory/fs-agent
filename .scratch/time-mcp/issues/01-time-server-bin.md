# 01 — `fs-agent-mcp-time`：一个手写的 stdio server（tracer bullet）

Type: implement
Status: done
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1–§3、§6.1–§6.2。本票打通「新二进制 → 真 spawn → 握手 →
> 一次 `get_current_time`」整条链。身份那句归 [票 02](02-identity-time-guidance.md)，文档与索引
> 归 [票 03](03-docs-and-index.md)。

## 目标

`cargo build` 之后多出一个 `fs-agent-mcp-time`：它被 fs-agent 自己当普通 stdio server 连上时，
`mcp_list` 里有 `get_current_time`，`mcp_call` 拿得回一行当下本地时间（含 UTC 偏移、时区名、
星期几）。

## 现状（2026-10-06 核实，改前先复核）

- **手写对端的先例**：[`tests/support/fake_mcp_server.rs`](../../../tests/support/fake_mcp_server.rs)
  —— 385 行、三条方法、结果形状（`text_result` / `failed_result` / `error`）。本票照它的**帧
  形状**写，但**不复用那个文件**：它是测试辅助，不是产品。
- **真 spawn 的测试先例**：[`tests/mcp_stdio.rs`](../../../tests/mcp_stdio.rs)：走 `McpService` +
  沙箱包装 + 协议帧 + 进程组清理。
- **`Cargo.toml` 的两个 `[[bin]]`**（`fs-agent`、`fake-mcp-server`）连同它们的注，以及
  `default-run` 那一行 —— 加第三个 bin 时那两处注释要跟着改（现在写的是「仓库里有**两个** bin」）。
- **`chrono` 已是依赖**（`features = ["serde"]`，`clock` 是缺省），
  **`iana-time-zone` 0.1.65 已在 `Cargo.lock`**（`chrono` 的传递依赖）—— 本票只把它提成直接依赖。
- **`rmcp` 只开了 client**（`Cargo.toml` 里那行注：不开 server）。本票不碰它。

## 落点

`src/bin/mcp_time.rs`（新）、`Cargo.toml`、`tests/mcp_time_server.rs`（新）。

## 具体行为

1. **`Cargo.toml`**：`[[bin]] name = "fs-agent-mcp-time"` / `path = "src/bin/mcp_time.rs"`；加
   `iana-time-zone = "0.1"`；把那两处「两个 bin」的注释改成三个，并写明第三个是什么、为什么手写。
2. **`src/bin/mcp_time.rs`**（一个文件，`main` + 一个可测的纯函数）：
   - 启动往 stderr 打一行自述（它会经 client 的诊断口出来，不砸终端）；
   - 逐行读 stdin、逐行解析 JSON-RPC、逐行 `flush` 应答；**stdin 结束就退出（0）**；
   - `server/discover` → `resultType` / `supportedVersions: ["2026-07-28"]` /
     `capabilities: { tools: {} }` / `ttlMs` / `cacheScope`（照假 server；协议只谈 `2026-07-28`，
     不做旧版 `initialize` 回退）；
   - `tools/list` → 一个工具，名字 `get_current_time`，参数 schema 是空对象、无必填；
   - `tools/call` → `name == "get_current_time"` 时回一个 text block；别的名字回 `-32601`；
   - 其余方法一律 `-32601`；**没有 `id` 的帧不回任何东西**（通知）；
   - 时间行由一个纯函数拼：`本机现在：2026-10-06 14:32:05 +08:00 星期二（Asia/Shanghai）`
     —— 偏移用 `%:z`，星期几由 `chrono::Weekday` 映射七个中文词，时区名取
     `iana_time_zone::get_timezone()`、**取不到就省掉尾部括号**（不编名字、不报错）。
3. **测试 `tests/mcp_time_server.rs`** —— 两条路都走：
   - **裸协议**：spawn `env!("CARGO_BIN_EXE_fs-agent-mcp-time")`，手写帧读写一遍（握手 / 工具
     清单 / 一次调用 / 未知方法 / 一条通知），断言 §6.1 那条正则；
   - **端到端**：把 `McpService` 配成这台二进制（照 `tests/mcp_stdio.rs`），`mcp_list` 看到工具、
     `mcp_call` 拿到时间行 —— 手写的帧与 client 的期望差一个字段，这条就红，**它才是握手形状的
     真验收**。

## 验证

`cargo test` + `cargo clippy --all-targets` + `cargo fmt` + `python3 scripts/check-language.py`：

1. 时间行的形状：匹配 `^本机现在：\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} [+-]\d{2}:\d{2} 星期.$`。
2. 端到端那一次调用拿到的文本与上面同形，且结果**带**那句不可信标记（缺省 `trust_results`）。
3. 未知方法回 `-32601`；通知不回任何字节（读超时即通过）。
4. 进程行为：stdin 关掉之后进程自己退出，退出码 0；stderr 那行自述确实被 client 接住。
5. 回归：`[mcp] enabled = false`、以及没配这台 server 的会话，行为逐字不变。
