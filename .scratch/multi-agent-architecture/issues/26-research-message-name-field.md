# research：消息级 `name` 字段的约束

Type: research
Status: resolved

## Question

本票为**票 17（发言归属与投影接缝）**提供外部事实。票面第 5 条要求核实 `name` 的"合法字符集与长度限制"，并预判"若文件里没有，需要补一次 research"。

**已核实**：`.scratch/multi-agent-architecture/research/02-provider-call-surface.md` **§4.2 已经覆盖了 `name` 的语义**——Kimi `Message.name` 默认 `null`、"Optional name for the message sender"；DeepSeek 四个 role 变体都带 `name`、"Provides the model information to differentiate between participants of the same role"；且 Kimi 的 Partial Mode 给了更重的语义（`name` 是 output prefix 的一部分）。**但它没有给字符集与长度限制**——本票只补这个缺口。

判据：**只报告事实与来源，不推荐方案**。凡一手来源未写明者标 ⚪ 未证实，**不做推断**。

### 需要查明

1. **Kimi 与 DeepSeek 的官方来源**（官方文档 / OpenAPI schema / 官方仓库）里，**消息级 `name`** 有没有写：合法字符集（pattern）、长度上限、是否允许空格 / 点 / 连字符 / 中文。**逐字引用**。
2. **它们跟随的 OpenAI 兼容面**：两家都自称 OpenAI-compatible，所以 OpenAI 官方 API reference 对 **ChatCompletion 消息的 `name`** 的约束是什么？以 OpenAI 官方文档为一手来源，逐字引用（含 pattern 与长度）。
3. **`name` 有没有行为性副作用**（不只是标签）的一手说明——已知 Kimi 的 Partial Mode 说它是 output prefix 的一部分并"compelling it to output content in the voice of the specified character"（research/02 §4.2 已记）。请核实**普通 chat（非 partial）**下有没有类似说明；有就引用，没有就标 ⚪。
4. **`tool` role 的消息**在两家是否也接受 `name`（research/02 记了 DeepSeek "四个变体都带 `name`"，请核实 Kimi 一侧与 tool role 的具体情况）。
5. 若任一家把它标成 **deprecated / 不推荐 / 仅兼容保留**，如实报告。

**边界**：不要调研投影策略、合并算法、前缀格式——那些是票 17 的决策。本票只报"这个字段的约束是什么"。

产出：`.scratch/multi-agent-architecture/research/07-message-name-field.md`（中文；来源 ID 表 + 逐条引用 + ⚪ 缺口清单 + 完整 URL 清单；格式照 `research/02-provider-call-surface.md` 与 `research/04-tree-sitter-and-symbol-extraction.md`）。

**本票不决定任何事**——它是票 17 的事实输入。

## Answer

**已解（2026-09-13，AFK，由 research 子代理执行）。**

**产物**：`.scratch/multi-agent-architecture/research/07-message-name-field.md`（来源：Kimi chat OpenAPI EN+CN、Kimi Partial guide、Kimi Messages API、DeepSeek chat 文档 EN+CN、OpenAI 官方 reference、官方 `openai-openapi` spec v2.3.0）。

- **字符集与长度：三家都没写。** Kimi 的 `Message.name` 是 `type: string, default: null` + "Optional name for the message sender"；DeepSeek 的 system/user/assistant 同形 + "Provides the model information to differentiate between participants of the same role"；**OpenAI 官方 reference 与 spec 也只有 `type: string` 加同一句话**。**一律没有 pattern、没有 maxLength。** → ⚪ 未证实，**不推断**。
- **一条对 `research/02` §4.2 的纠正**：**DeepSeek 的 `tool` 消息没有 `name` 字段**（字段只有 `role` / `content` / `tool_call_id`，EN+CN 双证）；**OpenAI 的 `ChatCompletionToolMessageParam` 同样没有**。Kimi 只是因为四个 role **共用一个 `Message` schema** 才"语法上允许"，且**没有 tool + name 的语义或示例**。
- **行为性副作用**：唯一被文档化的重语义是 Kimi 的 **Partial Mode**（`name` 是 output prefix 的一部分、"compelling it to output content in the voice of the specified character"）。**普通 chat（非 partial）下三家都没有文档化的行为影响**，只有那句信息性的 "differentiate participants"。
- **没有任何一家标它 deprecated**。（OpenAI 的 `function` role 已废弃，但它的 `name` 是**函数名**，语义不同。）
- ⚪ 缺口：合法字符集 / 是否允许空格、点、连字符、中文（三家都未声明）；长度上限（三家都未声明）；超长或非法值的行为；普通 chat 下的行为副作用；**Kimi 的 tool-role `name` 行为**；DeepSeek 的 Responses / Anthropic 兼容面未查（超出票面范围）。
