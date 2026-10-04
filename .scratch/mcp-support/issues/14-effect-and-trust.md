# 14 — `Effect` 与信任三个位

Type: implement
Status: done
Part of: ../map.md
Blocked by: 10

> 规格：[`../spec.md`](../spec.md) §6。

## 目标

外来工具**默认最严**；人在配置里**逐台**按能力放宽 —— 三个位各自默认关。

## 具体行为

1. **`mcp_call` 的 `effect()` 缺省 `Exclusive`**（票 11 已落地这条），本票负责**放宽的那条路**。
2. **三个独立的信任位**（都在 server 记录里，各自默认关）：
   - **`trust_effects`**：允许按配置声明较宽的 `Effect`（例如某条工具标只读）；
   - **`trust_results`**：这台 server 的结果**不带**不可信标记；
   - **`sandbox`**：默认 `true`，设 `false` 才不过沙箱（与冻结项 9 相反，得单独声明）。
3. **一个位不影响另一个位**：只开 `trust_results` 不会让 `Effect` 放宽，反之亦然。
4. **不读 `ToolAnnotations`**：规范明文说不能据以做权限判断 —— 连读都不读。

## 验证

`cargo test` + `cargo clippy --all-targets` + `python3 scripts/check-language.py`：

1. **缺省**：`mcp_call` 在 `readonly` 档拒、`ask` 档问。
2. **`trust_effects`**：打开且某条工具标只读后，它在 `readonly` 档放行。
3. **`trust_results`**：打开后结果不带标记；缺省带。
4. **`sandbox = false`**：该 server 的进程不过 bwrap（用假 spawn 断言 argv 里没有它）。
5. **互不牵连**：逐个只开一个位，断言另外两处的行为不变。

## 不做什么

- 不给 `Effect` 加第四类；不动权限门的四档矩阵。
- 不做「按 `ToolAnnotations` 自动映射」。

## 评论

- 2026-10-03 落地（`Status: done`）。落点：`src/config.rs`（`McpServerConfig.read_only_tools`
  与 `[mcp.servers.*]` 的 `read_only_tools`、解析期的「配了等于没配」校验）、
  `src/mcp/mod.rs`（`McpService::is_tool_read_only`）、`src/tools/mcp_call.rs`（`effect()`
  按位与名单决定 `ReadOnly` / `Exclusive`，结果按 `trusts_results` 决定带不带标记）、
  `src/mcp/rmcp_client.rs`（`sandbox = false` 时改用 off 的沙箱）、
  `tests/support/fake_mcp_server.rs`（`ancestors` 工具）、`tests/mcp_trust.rs`（新，4 条）、
  `tests/mcp_process.rs`（+1 条 `sandbox` 位）。
- **一处形状决定**：`trust_effects` 的「按配置声明较宽的 `Effect`」落地成 **`read_only_tools`
  字符串名单**（不是一张子表）。理由是它只表达一件事（哪几条按只读处理），而 Effect 的第三类
  `WritePaths` 是**工作区**概念，对外部工具没有意义。
- **不由 server 自报**：`ToolAnnotations` 一个字都不读，名单是人写的（规范原文：clients should
  never make tool use decisions based on ToolAnnotations from untrusted servers）。
- **`sandbox` 位的验证怎么写**：断言假 server 的**直接父进程**是不是 `bwrap`（假 server 报
  `/proc` 里的祖先链）。只看链首 —— 测试进程自己可能跑在更外层的容器里，链尾出现 `bwrap`
  不代表这一层包了（这一轮就踩到了这一点）。
- **验证 5「互不牵连」**：`turning_on_results_does_not_widen_the_effect` 只开 `trust_results`
  断言副作用仍最严；`readonly_refuses_by_default_and_trust_effects_opens_exactly_the_named_tool`
  里同一个 server 上「名单外的工具照旧拒」，也就是放宽是逐条的。
- 验证：`cargo test --test mcp_trust --test mcp_process`（4 + 4 条全绿）·
  `cargo clippy --all-targets`（无 warning）· `cargo fmt` · `python3 scripts/check-language.py`
  全通过。
