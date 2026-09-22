# fs-agent：消息级 `name` 字段的约束（Kimi / DeepSeek / OpenAI 兼容面）

> 目的：为票 17（发言归属与投影接缝）补 `research/02-provider-call-surface.md` **§4.2 未覆盖的缺口**——**消息级 `name` 的合法字符集、长度上限与行为性副作用**。§4.2 与 `01` 号文档已覆盖 `name` 的**语义**，本文不重复。
>
> 抓取日期：**2026-09-13（UTC）**。全部为官方文档 / OpenAPI schema 阅读，**未调用任何 API**。
>
> 来源分级：只用 ✅ 官方一手来源（官方 API 文档、官方 OpenAPI schema、官方仓库）。一手来源未写明者一律标 **⚪ 未证实**，**不做推断**；**不拿 OpenAI 约定去填两家官方未写明的空白**。

---

## 0. 来源清单与文档页 ID

后文用 ID 引用；完整 URL 见第 7 节。

| ID | 文档页 / schema | 用途 |
| --- | --- | --- |
| K-chat | Kimi《Chat Completions API》OpenAPI | `Message.name` 的完整 schema 定义（EN） |
| K-chat-cn | Moonshot 中国站《创建聊天补全》 | 同一 OpenAPI 的中文原文（页面内嵌 schema JSON） |
| K-partial | Kimi《Use Kimi API's Partial Mode》 | `name` 唯一的行为性说明（仅 Partial Mode） |
| K-messages | Kimi《Messages API》（Anthropic 兼容面） | 该面是否有消息级 `name` |
| D-chat | DeepSeek《Chat Completions API》 | 四个 role 变体的字段清单（EN） |
| D-chat-cn | DeepSeek 中国站同页 | 同一 schema 的中文原文 |
| O-ref | OpenAI 官方 API reference《Chat》 | ChatCompletion 消息 `name` 的字段定义 |
| O-spec | OpenAI 官方仓库 `openai/openai-openapi` `openapi.yaml`（`info.version: 2.3.0`） | 逐 schema 的机读定义（`pattern` / `maxLength` 的有无） |

---

## 1. Kimi 官方来源里消息级 `name` 的约束

K-chat 的 `Message` schema 把 `name` 定义为（逐字引用，YAML）：

```yaml
name:
  type: string
  default: null
  description: Optional name for the message sender
```

K-chat-cn 页面内嵌的同一 schema JSON（逐字引用）：

```json
"name":{"type":"string","default":null,"description":"消息发送者的名称（可选）"}
```

| 项 | 值 | 来源 |
| --- | --- | --- |
| 类型 | `string` | K-chat / K-chat-cn |
| 默认值 | `null` | K-chat / K-chat-cn |
| `pattern`（合法字符集） | **无**——schema 里没有 `pattern` 字段 | K-chat |
| 长度上限 | **无**——schema 里没有 `maxLength` / `minLength` | K-chat |
| 空格 / 点 / 连字符 / 中文是否允许 | 未写明 → **⚪ 未证实**（官方唯一示例是 ASCII 的 `"name": "Kelsier"`，K-partial） | K-chat / K-partial |
| 备注 | K-chat 全文只有两处 `pattern`/`minLength`：请求头 `X-Msh-Request-Nonce` 的 `minLength: 1`，以及工具函数名的 `pattern: ^[a-zA-Z_][a-zA-Z0-9-_]{0,127}$`；**均与消息 `name` 无关** | K-chat |

---

## 2. DeepSeek 官方来源里消息级 `name` 的约束

D-chat 的 `messages` 是四个 role 变体的 `oneOf`。`name` 只出现在 **system / user / assistant** 三个变体中，各变体字段定义完全相同（逐字引用，EN）：

> `name` **string**
> An optional name for the participant. Provides the model information to differentiate between participants of the same role.

D-chat-cn 同一字段（逐字引用）：

> **name** string
> 可以选填的参与者的名称，为模型提供信息以区分相同角色的参与者。

| 项 | 值 | 来源 |
| --- | --- | --- |
| 出现位置 | **仅 system / user / assistant** 三个变体 | D-chat / D-chat-cn |
| 类型 | `string` | D-chat |
| `pattern`（合法字符集） | **无** | D-chat |
| 长度上限 | **无**（该页全文没有 `maxLength` 属性；页内另有 `user_id`、`function.name` 的字符集/长度说明，但那是别的字段） | D-chat / D-chat-cn |
| 空格 / 点 / 连字符 / 中文是否允许 | 未写明 → **⚪ 未证实** | D-chat |
| tool 变体 | **没有 `name`**：tool 消息的字段为 `role` / `content` / `tool_call_id`（后两者 required） | D-chat / D-chat-cn |

> ⚠️ 与 `research/02 §4.2` 备注栏「System / User / Assistant（及 Tool）均有 `name`」不符：**一手来源里 DeepSeek 的 tool 消息没有 `name`**。本文以 schema 为准；未改动 02 号文档。

---

## 3. OpenAI 兼容面（OpenAI 官方）对消息 `name` 的约束

O-ref 对 system / user / developer（以及 assistant）消息给出（逐字引用）：

> `name` : **optional string**
> An optional name for the participant. Provides the model information to differentiate between participants of the same role.

O-spec 中 `ChatCompletionRequestSystemMessage` 等 schema 的机读定义（逐字引用）：

```yaml
name:
  type: string
  description: An optional name for the participant. Provides the model
    information to differentiate between participants of the same role.
```

| 项 | 值 | 来源 |
| --- | --- | --- |
| `pattern`（合法字符集） | **无**——O-spec 的 `ChatCompletionRequest{Developer,System,User,Assistant}Message` 里 `name` 只有 `type: string` 与 `description`，没有 `pattern` / `maxLength` / `minLength` | O-spec（v2.3.0） |
| 长度上限 | **无**（同上） | O-spec |
| 各 role 的字段清单（O-ref 原文） | `ChatCompletionDeveloperMessageParam object { content , role , name }`；`ChatCompletionSystemMessageParam object { content , role , name }`；`ChatCompletionUserMessageParam object { content , role , name }`；`ChatCompletionAssistantMessageParam object { role , audio , content , 4 more }`（含 `name`，O-spec）；`ChatCompletionToolMessageParam object { content , role , tool_call_id }`（**无 `name`**）；`ChatCompletionFunctionMessageParam object { content , name , role }`（`name` required，见第 6 节） | O-ref / O-spec |
| 空格 / 点 / 连字符 / 中文是否允许 | 未写明 → **⚪ 未证实**（不能据此推定两家行为） | O-ref / O-spec |
| OpenAI 官方是否给出 `function.name` 之类的字符集约束 | 是（同页其他字段有明确约束），但**消息 `name` 没有被给出任何 `pattern` 或长度** | O-spec |

---

## 4. `name` 的行为性副作用（普通 chat，非 Partial）

| | Kimi | DeepSeek | OpenAI |
| --- | --- | --- | --- |
| 普通 chat 下的行为说明 | **无**。K-chat 只说 "Optional name for the message sender"；K-chat / K-multiturn / K-thinking / K-tools 均未再提 `name` 对输出的作用 → **⚪ 未证实** | 只有 "Provides the model information to differentiate between participants of the same role."（信息性措辞，未描述更强效果） | 同一句措辞（O-ref） |
| 已知的**唯一**重语义 | K-partial 明说该语义属于 **Partial Mode**：「The `name` field in Partial Mode is a special field that enhances the model's understanding of its role, compelling it to output content in the voice of the specified character. **The `name` field is part of the output prefix.**」 | 无 | 无 |
| 副作用（是否影响风格 / 是否进前缀 / 是否影响缓存或计费） | 普通 chat 未文档化 → **⚪ 未证实**（明文的重语义只在 Partial Mode） | 未文档化 → **⚪ 未证实** | 未文档化 → **⚪ 未证实** |
| 来源 | K-chat / K-partial | D-chat | O-ref / O-spec |

---

## 5. `tool` role 的消息是否也接受 `name`

| | Kimi | DeepSeek | OpenAI |
| --- | --- | --- | --- |
| OpenAI 兼容 chat 面 | **schema 层面接受**：四种 role 共用同一个 `Message` schema（`role: system/…/tool`），`name` 挂在该 schema 上，OpenAPI 未按 role 排除；但**没有** tool 消息带 `name` 的示例或语义说明 → 行为 **⚪ 未证实** | **不接受**：tool 变体字段为 `role` / `content` / `tool_call_id`，无 `name` | **不接受**：`ChatCompletionToolMessageParam object { content , role , tool_call_id }` |
| 反例（同页） | K3 的 `KimiK3DynamicToolMessage`（`role: system` + `tools`）标了 `additionalProperties: false`，其属性只有 `role` / `tools`，**没有 `name`** | — | `function` role 的 `name` required，但语义是「要调用的函数名」，见第 6 节 |
| Anthropic 兼容面（旁证） | K-messages 里 `name` 只出现在 tool 定义（`Tool name. Must match the regular expression: …`），**未出现消息级 `name`** | 未核查 → **⚪** | — |
| 来源 | K-chat / K-messages | D-chat / D-chat-cn | O-ref / O-spec |

---

## 6. 是否被标 deprecated / 仅兼容保留

| | 结论 | 来源 |
| --- | --- | --- |
| Kimi `Message.name` | **没有**标 deprecated / 不推荐 | K-chat |
| DeepSeek system/user/assistant `name` | **没有**标 deprecated（同页 `frequency_penalty` / `presence_penalty` 才标 `deprecated`） | D-chat |
| OpenAI 消息 `name` | **没有**标 deprecated | O-ref / O-spec |
| 注意：OpenAI `function` role | 该 **role** 整体 `deprecated: true`；其 `name` required 且语义是 "The name of the function to call."，**与「参与者名」不是同一语义** | O-spec / O-ref |

---

## 7. ⚪ 无法从一手来源验证的清单

以下条目**官方一手来源没有写**，本文不推断、不作为事实使用：

1. **两家的合法字符集** —— Kimi / DeepSeek 的 schema 均无 `pattern`；是否允许空格、点、连字符、中文**完全未文档化**。
2. **两家的长度上限** —— Kimi / DeepSeek 的消息 `name` 均无 `maxLength` / `minLength`；截断、报错还是原样接受**未证实**。
3. **OpenAI 的合法字符集与长度上限** —— OpenAI 官方 reference 与 OpenAPI spec 同样只给 `type: string` + 描述，**没有** `pattern` 或长度；不能把 OpenAI 的「字段存在」当成对字符集的约定。
4. **普通 chat（非 Partial）下 `name` 的行为副作用** —— 两家都只有「区分同一 role 的参与者」这一信息性措辞；是否影响输出风格、是否作为前缀、是否影响缓存/计费**未证实**。
5. **Kimi tool role 上 `name` 的行为** —— schema 允许，但官方无示例、无语义说明，**未证实**。
6. **非法字符 / 超长值被拒绝时的错误表现** —— 未文档化（两家均未给出 `name` 的校验错误）。
7. **DeepSeek 的 Responses / Anthropic 兼容面上是否有消息级 `name`** —— 本文只核查了 OpenAI 兼容 chat 面，该问题**未核查**（票面边界外）。
8. **`name` 是否影响 KVCache / 计费口径** —— 未文档化。

---

## 8. 来源一览（全部一手官方来源，抓取于 2026-09-13 UTC）

### Kimi / Moonshot

| ID | URL |
| --- | --- |
| K-chat | <https://platform.kimi.ai/docs/api/chat> · Markdown: <https://platform.kimi.ai/docs/api/chat.md> |
| K-chat-cn | <https://platform.moonshot.cn/docs/api/chat>（页面内嵌 OpenAPI schema JSON） |
| K-partial | <https://platform.kimi.ai/docs/guide/use-partial-mode-feature-of-kimi-api> · `.md` |
| K-messages | <https://platform.kimi.ai/docs/api/messages> · <https://platform.kimi.ai/docs/api/messages.md> |

### DeepSeek

| ID | URL |
| --- | --- |
| D-chat | <https://api-docs.deepseek.com/api/create-chat-completion> |
| D-chat-cn | <https://api-docs.deepseek.com/zh-cn/api/create-chat-completion> |

### OpenAI（OpenAI 兼容面的一手来源）

| ID | URL |
| --- | --- |
| O-ref | <https://developers.openai.com/api/docs/api-reference/chat>（`platform.openai.com/docs/api-reference/chat/create` 对非浏览器请求返回 403，故以官方新域名 reference 为准） |
| O-spec | <https://github.com/openai/openai-openapi> · raw: <https://raw.githubusercontent.com/openai/openai-openapi/master/openapi.yaml>（`info.version: 2.3.0`） |
