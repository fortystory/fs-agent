# 15 — MRTR：把 elicitation 接到问询端口

Type: implement
Status: done
Part of: ../map.md
Blocked by: 10, 12

> 规格：[`../spec.md`](../spec.md) §3、§8。

## 目标

server 回 `input_required` 时，问题经**既有的**问询通道交给用户，答案拼回 `inputResponses` 重试
—— 不新开第四类发起者。

## 具体行为

1. **动一个既有接口**：问询端口从借用式（`questions: Option<&'a dyn UserQuestions>`）换成
   `Arc<dyn UserQuestions + Send + Sync>`，`ToolContext.questions` 跟着换形状 —— **不给 MCP 单开
   一条平行通道**（两套实现会漂）。改动点：`src/tools/tool.rs`、`src/lib.rs` 的
   `SessionScaffold.questions`、`src/tools/ask_user.rs` 与三处组装点。
2. **覆写 `ClientHandler::create_elicitation`**，把请求交给那个端口；先支持 form 模式。
3. **client 能力里 `enable_elicitation()`** —— 否则 server 不会发（`Discover` 下这些随每请求
   `_meta` 送出）。
4. **降级出口**：`call_tool_once` 是「自己驱动 MRTR」的现成出口，某类 elicitation 一时接不上时用它。

## 验证

`cargo test` + `cargo clippy --all-targets` + `python3 scripts/check-language.py`：

1. **往返一次**：假连接回 `input_required`，断言问题经 `ask_user_question` 那条通道发出（主会话
   接管底部输入区）、答案被拼回 `inputResponses` 并重试一次，最终拿到结果。
2. **用户拒绝**：`ElicitResult` 的 decline 语义 —— 调用以可读错误收尾，不是挂死。
3. **既有调用点回归**：`ask_user_question` 与权限询问在端口换形状之后行为不变。
4. **能力声明**：断言请求里带 elicitation 能力。

## 不做什么

- 不新开第四类发起者；不自己做 MRTR 重试回路（`rmcp` 的 `call_tool` 已经驱动）。
- 不做 URL 模式的 elicitation（先只 form）。

## Comments

- 2026-10-03 落地（`Status: done`）。落点：`src/tools/tool.rs`（`ToolContext.questions` 从
  `Option<&'a dyn UserQuestions>` 换成 `Option<Arc<dyn UserQuestions>>`）、
  `src/tools/registry.rs`（dispatch 里 `clone` 那个句柄）、`src/tools/ask_user.rs`（语法适配）、
  `src/mcp/rmcp_client.rs`（`ConnectOptions.questions`、handler 覆写 `create_elicitation`、
  握手能力里开 elicitation）、`src/cli.rs`（三处组装点把**同一个** `Arc` 递给 MCP 那侧，
  探针传 `None`）、`tests/support/fake_mcp_server.rs`（MRTR 的 `ask` 工具 + 落盘 discover 报文）、
  `tests/mcp_mrtr.rs`（新，3 条）。
- **端口为什么必须是 `Arc`**：`ClientHandler` 要求 `'static + Send + Sync`，handler 在连接建立时
  就得把端口握在手里；换成 `Arc` 之后 `ask_user_question` 与 MCP 那侧共用**同一个值**，不会漂成
  两套实现（票 02 的答案）。`SessionScaffold.questions` 本来就是 `Arc`，所以 `lib.rs` 一个字没改。
- **降级三条**（都是如实，不编答案）：没端口（无头）→ `decline`；端口报错（输入结束 / 本次运行
  被取消）→ `cancel`；有人答了但有必填属性空着 → `decline`。URL 模式的 elicitation 不在这一版
  范围，同样 `decline`。
- **form schema 怎么翻成题**：一个属性一道题，题号就是属性名，`message` 摆在题头上；这一版按自由
  文本题处理（`UserQuestion` 的选项面是「几个 label 里挑」，与带类型的字段硬对会在数字、布尔上
  失真）。答案按属性名编回 `ElicitResult.content`。
- **能力断言的写法**：假 server 把**整条** discover 请求落盘，测试断言报文里出现 `elicitation`。
  只挑 `_meta` 下某个键会更精确，但那是协议实现细节，测试关心的是「握手时报了这个能力」。
- 验证：`cargo test --test mcp_mrtr`（3 条全绿；MRTR 往返、取消 → 可读失败、能力声明）·
  `cargo clippy --all-targets` · `cargo fmt` · `python3 scripts/check-language.py` ·
  既有 `ask_user_question` / 权限询问测试（端口换形状之后的回归）。
