# 02 — 身份里那句静态指引

Type: implement
Status: done
Blocked by: 01

> 规格：[`../spec.md`](../spec.md) §5、§6.3。工具名与 server 名由 [票 01](01-time-server-bin.md)
> 定死（`time` / `get_current_time`），这句话才有东西可指。

## 目标

模型知道自己**可以问时间**，而身份里**没有任何时间的值** —— 于是 replay 仍是复现，缓存前缀也
不随时间去抖。

## 现状（2026-10-06 核实，改前先复核）

- **[`src/agent.rs`](../../../src/agent.rs) 的 `agent_identity()`** 现在拼四段：自我介绍、
  `todo` 规则段、`WEB_GUIDANCE`、`THINKING_IN_CHINESE`。新那句排在 `WEB_GUIDANCE` 之后。
- **`WEB_GUIDANCE` 的注释就是本票的取舍先例**：它**无条件**拼上，即使 `[web] enabled` 是关的
  —— 理由写在它的文档注释里（身份是缓存前缀，随配置抖动等于每次作废前缀）。本票照它办。
- **身份的消费点有两个**：`build_messages` 给请求打头，`replay::identity_for` 复现时再调一次
  [`agent_identity`](../../../src/agent/replay.rs)。这正是不放时间值的理由。
- **`tests/context_budget.rs` 不硬编码身份的长度**：它用
  `estimate_tokens(agent_identity())` 反算能力表，所以加一句不会红（改前再核一眼）。
- 三个身份**不拼**它：`discussion::debater_identity()`、`discussion::synthesizer_identity()`、
  `agent::executor::executor_identity()`。

## 落点

`src/agent.rs`、`tests/time_guidance.rs`（新）、`tests/executor.rs`（执行者那一段不在公开
API 上，只能从一次真派发里读）。

## 具体行为

1. **新增 `pub const TIME_GUIDANCE: &str`**，措辞照 spec §5：

   > 需要当下时间（几点、几号、星期几）时不要凭上下文猜：若会话里有提供时间的 MCP server
   > （本仓库自带 `fs-agent-mcp-time`），用 `mcp_call` 调它的 `get_current_time`。

   `pub` 的理由与 `THINKING_IN_CHINESE` 一样：让测试直接断言拼上了它，而不是在断言里再抄一遍。
2. **`agent_identity()` 拼上它**，排在 `WEB_GUIDANCE` 与 `THINKING_IN_CHINESE` 之间。
3. **文档注释写清三件事**：为什么不放时间的值（replay 不变量 + 缓存前缀）、为什么无条件拼
   （同 `WEB_GUIDANCE`）、为什么是条件式措辞（没配 MCP 的会话里这句话会落空，所以不能写成一句
   命令）。
4. **另外三段身份一个字不改。**

## 验证

`cargo test` + `cargo clippy --all-targets` + `cargo fmt` + `python3 scripts/check-language.py`：

1. `agent_identity()` 含 `TIME_GUIDANCE`（且点名 `get_current_time`）；`debater_identity()`、
   `synthesizer_identity()` 不含（`tests/time_guidance.rs`），执行者的身份也不含
   （`tests/executor.rs`，从真派发的请求里读）。
2. `agent_identity()` 的文本**不匹配** `\d{4}-\d{2}-\d{2}`：守住「不放时间的值」这条支点。
3. 回归：`tests/replay.rs` 与 `tests/e2e_single_turn.rs` 那条「重放 == 当时发出去的东西」照旧绿
   —— 身份仍是同一个纯函数的返回值。
