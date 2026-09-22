# Provider trait、流式 chunk 形状与终止判断

Type: grilling
Status: open

## Question

决定 provider 抽象层的接缝。范围已定：**只做一个 OpenAI-compatible client**（`base_url` 可配），不做原生 Anthropic / Gemini 协议。

要回答：

1. **`Provider` trait 的边界**：输入输出是什么形状？流式返回是不是两种事件的流——**文本增量**与 **tool_call 增量**？
2. **模型元数据从哪来**：上下文长度（`limit.input`）、`maxOutputTokens`、是否支持并行工具调用、是否支持 thinking。这些是 trait 的一部分，还是一张独立的按模型名索引的能力表？
3. **终止判断——本票最关键的一条。** 综述第 1 节点名了一个"一旦写错就很难察觉、表现为**工具调用被静默丢弃**"的 bug，来自 opencode 的实现：**绝不能直接信任 provider 返回的 `stop_reason`**。判据必须自己算。opencode 的表达式是「只要还有待执行的 tool call，即使 provider 报了 `stop`，这一轮也要继续」。把这判据定成一条**明确、可测**的规则，并说明它在 trait 边界上如何表达（谁持有"待处理工具结果"这个状态）。
4. 综述第 1 节的另一个取舍：**轮数上限是防呆，但应该做成可配参数而非硬编码常量**（goose 默认 1000、Gemini 100、Codex 干脆没有）。默认值定多少？

答案定到接口级即可（trait 签名 + 关键类型），不要实现细节。
