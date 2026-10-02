# 15 — MRTR：把 elicitation 接到问询端口

Type: implement
Status: ready-for-agent
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
