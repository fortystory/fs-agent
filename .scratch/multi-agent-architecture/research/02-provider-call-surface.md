# fs-agent：Kimi / DeepSeek 的 chat completion 调用面（逐字段参考）

> 目的：为 fs-agent（Rust 自用 coding agent CLI，走 OpenAI 兼容 HTTP API）给出一份**以两家官方文档为准**的 chat completion 调用面事实清单。重点是「一次 LLM 调用到底能传什么、返回什么」，为「多 agent 在同一 session 里讨论」的架构决策提供地面真相。
>
> 抓取日期：**2026-09-12（UTC）**。全部为文档阅读，**未调用任何 API**。
>
> **本文不重复** `01-debate-conformity-and-speaker-attribution.md` 第 3.3 节的 provider role 事实；那一节已覆盖 Anthropic/OpenAI/DeepSeek/Kimi 的 role 语义。本文只在其之上补全**完整参数面**、**流式结构**、**限额**与**会话原语**。
>
> 来源分级：本文只使用 ✅ 官方一手文档。凡官方文档未写明者，一律标 ⚪ 未证实，**不做推断**。

---

## 0. 来源清单与文档页 ID

后文用 ID 引用；完整 URL 见第 8 节。

| ID | 文档页 | 用途 |
| --- | --- | --- |
| K-chat | Kimi《Chat Completions API》 | 请求/响应/流式 OpenAPI、Partial Mode、JSON Mode |
| K-models-param | Kimi《Model Parameter Reference》 | 逐模型参数是否可改、默认值 |
| K-modellist | Kimi《Model List》 | 模型 ID、上下文窗口、弃用列表 |
| K-limits | Kimi《Recharge and Rate Limiting》 | 并发/RPM/TPM/TPD |
| K-errors | Kimi《Common Error Codes》 | HTTP 错误码与 error type |
| K-intro | Kimi《Main Concepts》 | 超时、限流计算口径 |
| K-stream | Kimi《Use the Streaming Feature》 | SSE 结构、`n` 参数、usage chunk |
| K-tools | Kimi《Use Kimi API for Tool Calls》 | 流式 tool_calls 分片、`index` |
| K-partial | Kimi《Use Kimi API's Partial Mode》 | `partial` 语义与约束 |
| K-thinking | Kimi《Thinking Models》 | `reasoning_content`、Preserved Thinking |
| K-cache | Kimi《Use Context Caching》 | 缓存是否自动、有无参数 |
| K-multiturn | Kimi《Set Parameters for Multi-turn Chat》 | 无状态、历史拼接 |
| K-vision | Kimi《Configure Kimi Vision Models》 | 请求体大小上限 |
| K-responses | Kimi《Responses API》 | 是否存在会话原语 |
| K-messages | Kimi《Messages API》 | Anthropic 兼容面、Partial Mode |
| K-cn-chat | Moonshot 中国站《创建聊天补全》 | CN base URL 与同构性 |
| D-chat | DeepSeek《Chat Completions API》 | 请求/响应/流式 schema |
| D-pricing | DeepSeek《Models & Pricing》 | 模型 ID、上下文、最大输出 |
| D-ratelimit | DeepSeek《Rate Limit & Isolation》 | 并发上限、`user_id`、keep-alive |
| D-errors | DeepSeek《Error Codes》 | HTTP 错误码 |
| D-thinking | DeepSeek《Thinking Mode》 | thinking/effort、采样参数失效 |
| D-json | DeepSeek《JSON Output》 | JSON mode 约束 |
| D-tools | DeepSeek《Tool Calls》 | strict 模式、可否插入 tool call |
| D-prefix | DeepSeek《Chat Prefix Completion (Beta)》 | `prefix` 语义 |
| D-cache | DeepSeek《Context Caching》 | 缓存是否自动 |
| D-multiturn | DeepSeek《Multi-round Conversation》 | 无状态 |
| D-responses | DeepSeek《Using the Responses API》 | 是否存在会话原语 |
| D-vision | DeepSeek《Vision》 | 请求体 48 MiB 等 Limits |

---

## 1. 端点与模型

### 1.1 Kimi (Moonshot)

| 项 | 值 | 来源 |
| --- | --- | --- |
| 国际站 base URL（OpenAI 兼容） | `https://api.moonshot.ai/v1` | K-chat（OpenAPI `servers: https://api.moonshot.ai`） |
| 中国站 base URL | `https://api.moonshot.cn/v1` | K-cn-chat（cURL 与 Python 示例均为此值） |
| 端点 | `POST /chat/completions` | K-chat |
| 鉴权 | `Authorization: Bearer <MOONSHOT_API_KEY>` | K-chat |
| 其他面 | Responses API、Messages API（Anthropic 兼容）、Files、Batch | K-responses / K-messages / llms.txt 索引 |

**中国站与国际站内容同构**：K-cn-chat 抓取到的请求体示例、模型名（`kimi-k3`）、字段（`name`/`partial`/`strict`）与国际站 K-chat 一致，差别只在 base URL。⚠️ 但 K-errors 明确写了**平台级 key 隔离**：「Keys issued on `platform.kimi.ai` are independent from keys issued on other regional Kimi platforms. Mixing keys across platforms returns 401.」→ fs-agent 的 provider 配置里 **base URL 必须与 key 来源同域**。

### 1.2 Kimi 当前模型（K-modellist / K-models-param）

| model ID | 上下文窗口 | 定位 | 最大输出 |
| --- | --- | --- | --- |
| `kimi-k3` | **1M tokens** | 旗舰，K3 始终开启 thinking + Preserved Thinking | `max_completion_tokens` 默认 **131072**，上限 **1048576** |
| `kimi-k2.7-code` | **256K tokens** | 代码专精，thinking 恒开 | 未按模型单列（见下） |
| `kimi-k2.7-code-highspeed` | 256K | 与 `kimi-k2.7-code` **同一模型、参数约束完全相同**，仅输出速度不同（约 180 tok/s，短上下文可达 260 tok/s） | 同上 |
| `kimi-k2.6` | **256K tokens** | 多模态 + 可切 thinking / 非 thinking | 未按模型单列 |

- `max_completion_tokens` 的**逐模型默认值只文档化了 K3**：「The default varies by model: **for Kimi K3** it defaults to 131072 and can be set up to 1048576.」（K-chat）K2.6 / K2.7-code 的默认值官方未给 → ⚪ 未证实。
- 约束：「If input plus `max_completion_tokens` exceeds the model context window, the API returns `invalid_request_error`.」且该字段是**期望返回长度**，不是输入+输出总长。
- 已弃用（调用返回 404 model not found）：`kimi-k2.5`、`moonshot-v1-8k/32k/128k/auto`、`moonshot-v1-*-vision-preview`、`kimi-k2-0905-preview`、`kimi-k2-0711-preview`、`kimi-k2-turbo-preview`、`kimi-k2-thinking`、`kimi-k2-thinking-turbo`；`kimi-latest`、`kimi-thinking-preview` 亦已停用。（K-modellist）

### 1.3 DeepSeek

| 项 | 值 | 来源 |
| --- | --- | --- |
| OpenAI 格式 base URL | `https://api.deepseek.com` | D-pricing |
| Anthropic 格式 base URL | `https://api.deepseek.com/anthropic` | D-pricing |
| Beta base URL（prefix completion / strict tools） | `https://api.deepseek.com/beta` | D-prefix / D-tools |
| 端点 | `POST /chat/completions`（`/v1` 为兼容别名，见下） | D-chat |
| 鉴权 | `Authorization: Bearer <DeepSeek API Key>` | D-chat |

### 1.4 DeepSeek 当前模型（D-pricing）

| model ID | 版本 | 上下文 | 最大输出 | 能力 |
| --- | --- | --- | --- | --- |
| `deepseek-flash` | DeepSeek-V4.1-Flash | **1M** | **最大 384K** | JSON Output ✓、Tool Calls ✓、Responses API ✓、Anthropic API ✓、Chat Prefix Completion (Beta) ✓、FIM（仅非 thinking）✓、Vision ✓ |
| `deepseek-v4-pro` | DeepSeek-V4-Pro-0813 | **1M** | **最大 384K** | 同上，但 **Vision ✗** |

- 两者都「Supports both non-thinking and thinking (**default**) modes」。
- 旧名 `deepseek-v4-flash`、`deepseek-v4-flash-vision-exp` 仍被接受，但对应模型已退役，请求由 DeepSeek-V4.1-Flash 承接并按 Flash 计费。
- 模型 ID 是**硬枚举**：D-chat 的 `model` 字段 `Possible values: [deepseek-flash, deepseek-v4-pro]`。

---

## 2. Kimi Chat Completions 逐字段表

来源：K-chat 的 OpenAPI（`ChatRequestCommon` / `ChatRequestBase` / `KimiK3ChatRequest` / `KimiK27CodeChatRequest` / `KimiK26ChatRequest`），以及 K-models-param。

### 2.1 顶层请求字段

| 字段 | 类型 / 取值 | 默认 | 语义与约束 | 来源 |
| --- | --- | --- | --- | --- |
| `model` | `kimi-k3` \| `kimi-k2.7-code` \| `kimi-k2.7-code-highspeed` \| `kimi-k2.6` | 各 family 默认自身 | oneOf + discriminator，按 model 选 schema → **不同模型的参数约束不同** | K-chat |
| `messages` | array | — | 必需。role ∈ `system`/`user`/`assistant`/`tool`；`content` **must not be empty**；K3 还允许插入动态工具消息 | K-chat |
| `max_tokens` | integer | — | **deprecated**，请改用 `max_completion_tokens` | K-chat |
| `max_completion_tokens` | integer | K3: 131072 | 上限 1048576（K3）；达到即 `finish_reason="length"`；input + 该值 > 上下文窗口 → `invalid_request_error` | K-chat |
| `temperature` | number | K3/K2.7-code: **固定 1.0**；K2.6: **thinking 1.0 / 非 thinking 0.6** | **"Cannot be modified"**；传其他值**返回错误** | K-models-param |
| `top_p` | number | **固定 0.95**（全部模型） | **"Cannot be modified"**；传其他值返回错误 | K-models-param |
| `n` | integer | **固定 1**（全部模型） | **"Cannot be modified"**；`n>1` → **400 `invalid n: only 1 is allowed for this model`** | K-models-param / K-stream |
| `presence_penalty` | number | **固定 0** | **"Cannot be modified"** | K-models-param |
| `frequency_penalty` | number | **固定 0** | **"Cannot be modified"** | K-models-param |
| `stop` | string \| array[string] | null | **最多 5 个**，**每个 ≤ 32 bytes**；全匹配时停止且匹配串不输出 | K-chat |
| `stream` | boolean | `false` | — | K-chat |
| `stream_options.include_usage` | boolean | `false` | 见 5.1 | K-chat |
| `response_format` | `{type: text\|json_object\|json_schema}` | `{type:"text"}` | `json_schema` 需 `json_schema:{name, schema}`；`strict` 默认 `true`，schema 须符合 **MFJS**（Moonshot Flavored JSON Schema） | K-chat |
| `tools` | array | — | 仅 `type:"function"`；`function.name` 正则 `^[a-zA-Z_][a-zA-Z0-9-_]{0,127}$`；`parameters` 须符合 MFJS；`function.strict` **默认 true** | K-chat |
| `tool_choice` | `auto`\|`none`\|`required`\| `{type:"function",function:{name}}` | `auto` | **`required` 仅 `kimi-k3` 支持**；K2.6 / K2.7-code 传 `required` **报错** | K-chat / K-models-param |
| `logprobs` | boolean | `false` | true 时在响应 message 的 `logprobs` 返回每个输出 token 的对数概率 | K-chat |
| `top_logprobs` | integer **0–20** | — | **必须同时 `logprobs:true`** | K-chat |
| `prediction` | `{type:"content", content: string\|array}` | — | Predicted Output，用于「大段已知、只改一点」的重写场景 | K-chat |
| `prompt_cache_key` | string | null | 缓存命中优化；官方建议 coding agent 用 session id / task id，且**恢复会话时保持不变** | K-chat |
| `safety_identifier` | string | — | 稳定用户标识（建议哈希），用于滥用检测 | K-chat |
| `thinking` | `{type:"enabled"\|"disabled", keep: null\|"all"}` | K2.6 `{type:"enabled"}`；K2.7-code 恒 `{type:"enabled",keep:"all"}` | **K2.x 专属**；K2.7-code **不接受 `disabled`**（报错）；`keep` 只接受 `null` 或 `"all"`（K2.7-code 只接受 `"all"`） | K-chat / K-models-param |
| `reasoning_effort` | `"low"`\|`"high"`\|`"max"` | **`"max"`** | **K3 专属**（顶层字段）；切档会**破坏 prefix cache 命中** | K-chat / K-models-param |

**OpenAPI 未定义、但 models-overview 明确列出的字段**（重要文档不一致）：

`temperature`、`top_p`、`n`、`presence_penalty`、`frequency_penalty` **不在** K-chat 的 OpenAPI request schema 里（对 chat.md 全文检索 `temperature`/`top_p`/`presence_penalty`/`frequency_penalty`/`seed`/`top_k`/`logit_bias`/`parallel_tool_calls`/`repetition_penalty` 均为 **0 命中**），但 K-models-param 以「参数对照表 + 逐模型差异表」把它们列为**固定不可改**。两页的说明合起来才完整，且 K-models-param 加了一句遗留注释：

> "When `temperature` is close to 0, `n` can only be 1. Otherwise, the API returns `invalid_request_error`."

（这句与「temperature 完全不可改」在字面上有张力，疑为旧模型时代的遗留说明；对当前三个模型，以「不可传」为准。）

### 2.2 `messages` 元素

| 字段 | 类型 | 语义 | 来源 |
| --- | --- | --- | --- |
| `role` | `system` \| `user` \| `assistant` \| `tool` | 消息作者角色 | K-chat |
| `content` | string \| array | string 或 `[{type:text,text}, {type:image_url,image_url}, {type:video_url,video_url}]`；**不得为空** | K-chat |
| `name` | string | **"Optional name for the message sender"** | K-chat |
| `partial` | boolean | **只在最后一条 assistant 消息上**置 true 以启用 Partial Mode，默认 `false` | K-chat |
| `reasoning_content` | （响应字段；需原样回传） | thinking 模型的历史思考内容，见 K-thinking | K-thinking |
| `tool_call_id` | string | `role="tool"` 时必填，须等于请求里的 `tool_call.id` | K-chat |

**K3 专属：动态工具消息**（K-chat `KimiK3DynamicToolMessage`）

```json
{ "role": "system", "tools": [ /* ToolDefinition... */ ] }
```

> "you can insert a `{"role": "system", "tools": [...]}` message at **any conversation position** to dynamically load tools. A dynamic tool message **omits content** and only affects subsequent conversation turns."

即 **K3 允许在对话中途插入只带 `tools`、不带 `content` 的 system 消息**；`additionalProperties: false`。

---

## 3. DeepSeek Chat Completions 逐字段表

来源：D-chat 的请求/响应 schema，辅以 D-thinking / D-json / D-tools / D-prefix。

### 3.1 顶层请求字段

| 字段 | 类型 / 取值 | 默认 | 语义与约束 | 来源 |
| --- | --- | --- | --- | --- |
| `model` | `deepseek-flash` \| `deepseek-v4-pro` | — | 硬枚举 | D-chat |
| `messages` | object[] (`>= 1`) | — | oneOf：System / User / Assistant / Tool 四类 | D-chat |
| `thinking` | `{type: "enabled"\|"disabled"}` nullable | `enabled` | thinking / 非 thinking 开关 | D-chat |
| `reasoning_effort` | `"none"`\|`"low"`\|`"high"`\|`"max"` | **`high`** | `none` 关闭 thinking；`low/high/max` 开启。兼容映射：`minimal→low`，`medium`/`xhigh`→`high`；`ultra→max`（D-thinking） | D-chat / D-thinking |
| `max_tokens` | integer nullable | 非 thinking **8K**；thinking **64K**（`reasoning_effort="max"` 时 **128K**） | **范围 1 – 384K（393216）**；输入+输出受上下文长度限制 | D-chat |
| `response_format` | `{type: "text"\|"json_object"}` nullable | `text` | 仅这两种；**Chat Completions 没有 `json_schema`** | D-chat / D-json |
| `stop` | string \| array（`oneOf`）nullable | — | **最多 16 个序列** | D-chat |
| `stream` | boolean nullable | — | 分片以 data-only SSE 下发，以 `data: [DONE]` 结束 | D-chat |
| `stream_options` | object nullable | — | **必须与 `stream:true` 同用**，否则返回 **400**；子字段 `include_usage` | D-chat |
| `temperature` | number nullable | **1** | 取值 **≤ 2**（0–2）；**thinking 模式下无效**（不报错也不生效，D-thinking）；与 `top_p` 建议只改一个 | D-chat / D-thinking |
| `top_p` | number nullable | **1** | 须 **>0 且 ≤1**；**thinking 模式生效但下限 0.95**（低于 0.95 被抬到 0.95）；**非 thinking 模式固定 1.0，传入值被忽略** | D-chat / D-thinking |
| `tools` | object[] nullable | — | 仅 `type:"function"`；`function.name` 须为 a-z/A-Z/0-9/`_`/`-`，**最长 128**；`parameters` 为 JSON Schema object（省略即空参数）；`function.strict` 默认 **false**（**Beta**，需 `/beta` base URL） | D-chat / D-tools |
| `tool_choice` | `none`\|`auto`\|`required`\| `{type:"function",function:{name}}` | 无 tools → `none`；有 tools → `auto` | **`required` 与具名选择在 thinking 模式下不支持，返回 400**；需先关 thinking | D-chat |
| `logprobs` | boolean nullable | — | true 时返回 `content` 每个输出 token 的对数概率 | D-chat |
| `top_logprobs` | integer nullable **≤ 20** | — | 0–20；**须 `logprobs:true`** | D-chat |
| `user_id` | string | — | 正则 `[a-zA-Z0-9\-_]+`，**最长 512**；用于内容安全、**KVCache 隔离**、调度隔离 | D-chat / D-ratelimit |
| `frequency_penalty` | — | — | **deprecated**：「This parameter is no longer supported. **It will not take effect if you pass it to the API.**」 | D-chat |
| `presence_penalty` | — | — | **deprecated**：同上 | D-chat |

**DeepSeek Chat Completions 请求体中不存在的字段**（对 D-chat 页面全文检索，命中数为 0）：

`n`、`seed`、`top_k`、`logit_bias`、`parallel_tool_calls`。

注意 `parallel_tool_calls` 在 **Responses API** 兼容表里出现了，但取值是「**Ignored（parallel tool calling is always enabled）**」（D-responses），**Chat Completions 完全没有这个字段**。

### 3.2 `messages` 元素（D-chat）

四个变体（System / User / Assistant / Tool）**都带 `name`**，原文：

> "**name**: An optional name for the participant. Provides the model information to differentiate between participants of the same role."

（对页面全文检索确认该句在 system / user / assistant 三个变体中各出现一次，共 3 处命中；tool 变体亦然。）

- System：`content` string（required）、`role: system`、`name`。
- User：`content` 支持 string 或 content block 数组（`text` / `image_url` / `file`(+`file_id` 或 `file_data`+`filename`)）。
- Assistant：`content` string nullable（required）、`role: assistant`、`name`；做前缀续写时最后一条额外带 `prefix`（见 4.10）。
- Tool：`tool_call_id` + `content`。

---

## 4. 关键字段逐项对照（两家显式并列，不做平均）

### 4.1 `n`（多个候选补全）—— 结论：两家都**不能**用 `n` 做多样本采样

| | Kimi | DeepSeek |
| --- | --- | --- |
| 是否在 OpenAPI request schema 中 | **否**（chat.md 无 `n` 属性） | **否**（D-chat 无 `n`） |
| 官方是否另有说明 | **有**，在 K-models-param 与 K-stream | **无，任何页面都没有 `n`** |
| 取值 / 语义 | **固定为 1，不可修改**；「`n` **Fixed at 1**」，「'Fixed' means the parameter **cannot be modified: passing any other value returns an error**, so do not pass it explicitly」 | 文档未定义 → ⚪ **传 `n` 的行为未证实**（既无支持声明，也无拒绝声明） |
| 超范围行为 | 明确：「Passing an `n` greater than 1 returns a **400** error (`invalid n: only 1 is allowed for this model`) for **both streaming and non-streaming** requests.」 | ⚪ 未证实 |
| 返回什么 | 只会是**单个** choice（`choices` 长度为 1） | 同理；响应 `choices` 数组语义上可容纳多 choice（有 `index` 字段），但**没有任何文档说能一次返回多个** |
| 附带遗留语 | K-models-param 有一句「When `temperature` is close to 0, `n` can only be 1. Otherwise, the API returns `invalid_request_error`.」 | — |

**对 fs-agent 的直接含义（事实层面，不含建议）**：想在**一次调用**里拿到同一 prompt 的多个独立样本（多 agent 讨论里「独立首轮采样」的关键），**这两家都做不到**。只能在一个 session 里发**多次串行请求**，或自己并发发多个 HTTP 请求（受 6.1/6.2 的并发上限约束）。唯一的规模化替代品是 Kimi 的 **Batch API**（见 7.3），但那是离线批处理，不是同步补全。

### 4.2 `name`（消息上的说话人标签）

| | Kimi | DeepSeek |
| --- | --- | --- |
| 支持 | ✅ `Message.name`，默认 `null`，"Optional name for the message sender" | ✅ System / User / Assistant（及 Tool）均有 `name` |
| 语义 | 字面只说「发送者可选名」；但 K-partial 给了**更重的语义**：在 Partial Mode 下「The `name` field is **part of the output prefix**」，并称其「**compelling it to output content in the voice of the specified character**」，官方把它列为 role-play 保角色的手段 | "Provides the model information to **differentiate between participants of the same role**." |
| 来源 | K-chat / K-partial | D-chat |
| 备注 | 与 K-partial 的「Maintain role name prefixes in role-play scenarios (combined with the `name` field)」一致 | 与 01 号文档 3.3 节的结论一致；此处不重复 |

### 4.3 `seed`

| | Kimi | DeepSeek |
| --- | --- | --- |
| 支持 | ❌ **两份文档（K-chat 全文、K-models-param）均无 `seed`** | ❌ **D-chat 全文无 `seed`** |
| 结论 | 未文档化 → 视为不支持；传了会怎样 ⚪ 未证实 | 同左 |

### 4.4 `logprobs` / `top_logprobs`

| | Kimi | DeepSeek |
| --- | --- | --- |
| `logprobs` | ✅ boolean，默认 `false`；true 时在响应 message 的 `logprobs` 字段返回每个输出 token 的对数概率 | ✅ boolean nullable；「returns the log probabilities of each output token returned in the **`content`** of `message`」（注意措辞只提 `content`） |
| `top_logprobs` | ✅ integer，**min 0 / max 20**；**须 `logprobs:true`** | ✅ integer，**≤ 20**；**须 `logprobs:true`** |
| 响应结构 | `Choice.logprobs`（K-chat 的 `ChatCompletionResponse` 含 logprob 信息） | `choices[].logprobs.content[]` 与 `top_logprobs[]`，每项 `{token, logprob, bytes}`；**token 不在 top 20 时 `logprob` 用 `-9999.0`** 表示「极不可能」 |
| 是否可用于 thinking 模式 | ⚪ 未文档化 | ⚪ 未文档化（D-chat 未就 thinking 与 logprobs 的交互做说明） |
| 来源 | K-chat | D-chat |

### 4.5 `top_k`

| | Kimi | DeepSeek |
| --- | --- | --- |
| 支持 | ❌ 无（K-chat 0 命中） | ❌ 无（D-chat 0 命中） |
| 结论 | 两家**都没有** `top_k`。Kimi 用固定的 `top_p=0.95` 代替；DeepSeek 只用 `top_p`（默认 1，thinking 下限 0.95） | 同左 |

### 4.6 `frequency_penalty` / `presence_penalty`

| | Kimi | DeepSeek |
| --- | --- | --- |
| 状态 | **固定为 0，不可修改**；传其他值**返回错误**（K-models-param） | **已废弃**：两字段都标 `deprecated`，原文「no longer supported. **It will not take effect if you pass it to the API.**」(D-chat)；D-thinking 再次确认 thinking 模式不支持，且「setting these parameters **will not trigger an error but will also have no effect**」 |
| 差异要点 | Kimi：**传了会报错** | DeepSeek：**传了不报错、但也无效**（静默忽略） |
| 对多样性的现实含义 | 两家都无法用 penalty 调多样性 | 同左 |

### 4.7 `stop`

| | Kimi | DeepSeek |
| --- | --- | --- |
| 类型 | string 或 array[string] | string 或 array（`oneOf` 两种形态） |
| 数量上限 | **5 个** | **16 个** |
| 长度上限 | **每个 ≤ 32 bytes** | 未文档化 |
| 语义 | 全匹配时停止，**匹配串本身不输出** | "Up to 16 sequences where the API will stop generating further tokens" |
| 来源 | K-chat | D-chat |

### 4.8 `response_format` / JSON 模式 / 结构化输出

| | Kimi | DeepSeek |
| --- | --- | --- |
| `text` | ✅ 默认 | ✅ 默认 |
| `json_object` | ✅ 「ensures output is a valid JSON object」，**必须在 prompt 里引导 JSON** | ✅ 「guarantees the message the model generates is valid JSON」，**必须在 system/user prompt 里出现 "json" 字样并给示例**（D-json） |
| `json_schema` 结构化输出 | ✅ **有**。`{type:"json_schema", json_schema:{name, schema, strict}}`；`strict` 默认 **true**；schema 须符合 **MFJS** 规范，官方提供 `walle` CLI 校验；`strict:false` 时只保证是合法 JSON object，不保证内部结构 | ❌ **Chat Completions 没有** `json_schema`（只 `text` / `json_object`）。**Responses API** 的 `text.format` 才「fully supported」JSON Schema（D-responses） |
| 已知坑 | ⚠️ **不要**把 Partial Mode 与 `response_format:{"type":"json_object"}` 混用（K-chat 明确 Warning） | ⚠️ JSON Output 下 API **偶尔返回空 content**（官方承认，仍在优化）；⚠️ 若 prompt 里没让它输出 JSON，「the model may generate an unending stream of **whitespace** until the generation reaches the token limit, resulting in a long-running and seemingly **"stuck"** request」；`finish_reason="length"` 时 JSON 可能被截断（D-chat / D-json） |
| 来源 | K-chat | D-chat / D-json / D-responses |

### 4.9 `tools` / `tool_choice` / `parallel_tool_calls`

| | Kimi | DeepSeek |
| --- | --- | --- |
| `tools` 类型 | 仅 `function` | 仅 `function`；「Tool names must be unique」 |
| 函数名约束 | 正则 `^[a-zA-Z_][a-zA-Z0-9-_]{0,127}$` | a-z/A-Z/0-9/`_`/`-`，**最大长度 128** |
| 参数 schema | `parameters` 须符合 **MFJS** | `parameters` 为 JSON Schema object；**省略即定义空参数** |
| `function.strict` | `strict` **默认 true**；true 时 schema 须符合 MFJS | `strict` **默认 false**；strict 模式是 **Beta**，需 `base_url=.../beta`，且**要求所有 function 都设 `strict:true`**，服务端会校验 schema，不支持的类型报错。支持的类型：object/string/number/integer/boolean/array/enum/anyOf，及 `$ref`/`$def`；**不支持** `minLength`/`maxLength`/`minItems`/`maxItems`（D-tools） |
| `tool_choice` 取值 | `auto`(默认) / `none` / `required` / 具名 | `none` / `auto`(有 tools 时默认) / `required` / 具名；无 tools 时默认 `none` |
| `required` 的限制 | **仅 `kimi-k3` 支持**；K2.6 / K2.7-code 传 `required` **报错** | **`required` 与具名选择在 thinking 模式不支持 → 400**，需先关 thinking |
| 一次返回多个 tool call | ✅ 文档明确：「the model **can choose to call multiple tools at once**, which can be different tools or the same tool with different parameters」；每个 call 有唯一 `id`（K-tools） | ✅ Responses API 侧写明「**parallel tool calling is always enabled**」（D-responses）；Chat Completions 侧未出现 `parallel_tool_calls` 字段 |
| `parallel_tool_calls` 参数 | ❌ 无（chat.md 0 命中） | ❌ Chat Completions **无**；Responses API 有该字段但状态是「**Ignored**（parallel tool calling is always enabled）」 |
| `max_tool_calls` | ❌ 无 | ❌ Chat Completions 无；Responses API **Ignored** |
| 中途插入 tool call | ⚪ 未文档化（未提 Chat Completions 可否） | Chat Completion（OpenAI 格式）**不支持**在对话中途插入 tool call 及其结果；但**支持中途插入 system 消息**。要中途插 tool call 得用 Anthropic API 或 Responses API（D-tools） |
| 来源 | K-chat / K-models-param / K-tools / K-stream | D-chat / D-tools / D-responses |

### 4.10 assistant prefill：DeepSeek `prefix` vs Kimi `partial`

| | Kimi `partial` | DeepSeek `prefix` |
| --- | --- | --- |
| 字段位置 | `messages` 最后一个元素上的 `partial: boolean`（默认 `false`） | `messages` 最后一个元素上的 `prefix: boolean`（原文示例 `"prefix": True`） |
| 开启条件 | 「Append an `role="assistant"` message at the end of the `messages` array, and set `partial: true`」 | 「users **must** ensure that the **`role` of the last message** in the `messages` list is **`assistant`** and set the **`prefix` parameter of the last message to `True`**」 |
| 是否需要特殊 base URL | ❌ 不需要。K-chat 端点是正式 `/v1/chat/completions` | ✅ 需要：`base_url="https://api.deepseek.com/beta"`（**Beta 功能**） |
| 语义 | 模型**强制从给定 content 继续生成**（"the model is forced to start its reply with that content"） | 提供 assistant 前缀，让模型补完剩余部分（chat prefix completion） |
| 官方用例 | ① 固定开头格式（JSON 的 `{`、代码块 ` ```python `）② role-play 保角色（**配合 `name`**）③ `finish_reason="length"` 后**用同一前缀续写被截断内容** | 官方示例：给 `" ```python\n"` 前缀强制输出 Python 代码，并配 `stop=[" ```"]` 阻止多余解释 |
| 与 thinking 的交互 | ⚠️ thinking 模型下**必须把上一轮的 `reasoning_content` 一起回传**（示例显式写出）；且「thinking consumes `max_tokens` first ... with a small `max_tokens` the truncation point may fall inside the thinking phase — `content` is still empty while `finish_reason` is already `length`, and the continuation would **restart from scratch because the prefix is empty**」 | ⚪ 未文档化（D-prefix 未提 thinking 模式下的 `prefix` 行为） |
| 与其他功能的冲突 | ⚠️ **不要**与 `response_format:{"type":"json_object"}` 混用；要引导 JSON 就用 Structured Output，或单独 `partial:true` + 预填 `{` | ⚪ 未文档化 |
| Anthropic 兼容面也有 | ✅ K-messages：`MessagesRequest.messages` 说明「If the last message is from the assistant, the model continues from that content (**Partial Mode**)」 | ✅ 有 Anthropic 兼容端点（`/anthropic`），但 `prefix` 的等价语义 ⚪ 未在 prefix 页说明 |
| 来源 | K-partial / K-chat / K-messages | D-prefix |

### 4.11 `temperature` / `top_p` 的范围与「被忽略 / 被拒绝」

两家在这里**行为方向完全相反**，必须分开看：

| | Kimi | DeepSeek |
| --- | --- | --- |
| `temperature` | **不能改**。K3 固定 1.0；K2.7-code 固定 1.0；K2.6 thinking 固定 1.0、非 thinking 固定 0.6。「other values return an error」。官方明确：「**Do not pass `temperature` explicitly** when calling these models.」 | **可以传**，范围 0–2，默认 1；但 **thinking 模式下无效**（不报错，也不生效）。非 thinking 模式生效 |
| `top_p` | **不能改**，固定 0.95（三族全部） | **可以传**，默认 1，须 >0 且 ≤1；**thinking 模式生效但有下限 0.95**（<0.95 被抬到 0.95）；**非 thinking 模式固定 1.0，传入值被忽略** |
| 净效果 | **无法通过 temperature/top_p 调节随机性**（这是 fs-agent 在设计「多 agent 讨论制造多样性」时最硬的一条约束） | thinking 模式**只能通过 `top_p` 在 [0.95, 1] 内微调**；非 thinking 模式下 `top_p` 固定 1.0、`temperature` 可调 |
| 来源 | K-models-param / K-chat / K-thinking | D-chat / D-thinking |

### 4.12 reasoning / thinking budget 参数

| | Kimi | DeepSeek |
| --- | --- | --- |
| 开关 | K3：**无开关**（always reasons + Preserved Thinking always on）；K2.7-code：**无开关**（恒开，传 `disabled` 报错）；K2.6：`thinking: {"type": "enabled"\|"disabled"}`（默认 enabled） | `thinking: {"type": "enabled"\|"disabled"}`，默认 `enabled` |
| 预算 / 强度 | K3：顶层 `reasoning_effort ∈ {low, high, max}`，**默认 `max`**；K2.x：**不支持** `reasoning_effort` | `reasoning_effort ∈ {none, low, high, max}`，默认 `high`；映射表：`minimal→low`、`medium→high`、`high→high`、`xhigh→high`、`max→max`、`ultra→max`。`none` 关闭 thinking |
| 思考内容回传 | K2.6 `thinking.keep: null`（默认，忽略历史 thinking）/ `"all"`；K2.7-code 与 K3 **恒为 "all"**，必须把历史 assistant 消息连同 `reasoning_content` **原样回传** | 带 `tools` 的请求：**必须**把历轮 `reasoning_content` 全部回传，否则 **400**；不带 `tools`：`reasoning_content` 可回传但**会被忽略**、不拼进上下文 |
| 思考内容 token 归属 | `reasoning_content` 计入 token 消耗；其 token 也受 `max_tokens` 控制（reasoning + content ≤ max_tokens） | 响应 `usage.completion_tokens_details.reasoning_tokens` 单独给出思考 token 数 |
| 响应字段 | `choices[0].message.reasoning_content`（与 `content` 同级；OpenAI SDK 类型未声明，需 `hasattr`/`getattr`；流式下 `reasoning_content` **总在** `content` 之前） | `choices[0].message.reasoning_content`（与 `content` 同级） |
| 官方工程建议 | 「Set `max_tokens >= 16000`」以确保完整返回 reasoning + content；「**Do not set `temperature`**」；建议开 `stream=True` | ⚪ 未给类似数值建议 |
| 来源 | K-chat / K-models-param / K-thinking | D-chat / D-thinking |

### 4.13 prompt caching：是否自动、有无参数

| | Kimi | DeepSeek |
| --- | --- | --- |
| 是否自动 | ✅ **完全自动**：「Context Caching is **automatically enabled for all model requests**」；「**No manual creation**」「**No cache ID references**」「**No TTL management**」；K-cache 的标题就是「**No configuration needed: caching is automatic**」 | ✅ **默认开启**：「The DeepSeek API Context Caching on Disk Technology is **enabled by default for all users**, allowing them to benefit **without needing to modify their code**」 |
| 是否有参数 | ✅ **有可选参数** `prompt_cache_key`（K-chat）：用于提升相似请求的命中率；官方针对 coding agent 明确建议「typically a **session id or task id** representing a single session; if the session is exited and later resumed, **this value should remain the same**」；Kimi Code Plan 下该字段**必填** | ❌ **无缓存参数**。D-responses 兼容表明确：「`prompt_cache_key` / `prompt_cache_retention` **Not supported**. Context caching is managed automatically」 |
| 命中判据 | ⚠️ **门槛**：「A new request can hit the prefix cache only when the **previous request's prompt tokens exceed 256**. If ... below 256, the request is **not cached and is discarded**.」 | 前缀需**已持久化**且**完整匹配**某个 cache prefix unit。持久化时机：请求边界（用户输入末尾 / 模型输出末尾）、跨请求公共前缀检测、长输入/输出的固定 token 间隔。示例：round1=A+B、round2=A+B+C 可全命中；round2=A+C 不命中但会把公共前缀 A 持久化，round3=A+D 可命中 A。**Best-effort，不保证 100%**；缓存构建需数秒，不再使用时数小时到数天内自动清除 |
| 用量字段 | `usage.cached_tokens`（K-chat 非流式示例） | `usage.prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`；`usage.prompt_tokens_details.cached_tokens`（= hit tokens）；`prompt_tokens = hit + miss` |
| 对输出随机性 | ⚪ 未提 | ✅ 明确：「The hard disk cache only matches the **prefix part** of the user's input. The output is still generated through computation and inference, and it is influenced by parameters such as `temperature`, introducing randomness.」 |
| 会破坏缓存的操作 | `reasoning_effort` **切档**会破坏 prefix-cache 命中（K-models-param / K-messages：换档应在 session 开始前定好） | ⚪ 未列举 |
| 隔离手段 | ⚪ 未见 `user_id` 类隔离字段；有 `safety_identifier`（用户标识，非缓存隔离用途） | ✅ `user_id` 用于 **KVCache 隔离**（D-ratelimit） |
| 来源 | K-cache / K-chat / K-models-param | D-cache / D-chat / D-responses / D-ratelimit |

### 4.14 连续同 role 消息：接受 / 拒绝 / 合并？

**这是本节最需要小心的部分：两家官方文档都没有对「任意多条连续同 role」给出规则。**

| 问题 | Kimi | DeepSeek |
| --- | --- | --- |
| 是否声明「合并连续同 role」 | ❌ 无任何此类声明 | ❌ 无任何此类声明 |
| 是否声明「拒绝连续同 role」 | ❌ 无 | ❌ 无 |
| 文档能证实的相关事实 | ① `role` enum = `system`/`user`/`assistant`/`tool`，`content` **不得为空**。<br>② **Partial Mode 要求最后一条是 `assistant`** → 至少证明「末尾一条 assistant」被接受。<br>③ **K3 允许在任意位置插入 `{"role":"system","tools":[...]}`**（无 content）→ 证明 system role 不必只在开头。<br>④ 官方多轮示例全是严格交替 user/assistant（K-multiturn） | ① messages 的 oneOf = System/User/Assistant/Tool。<br>② **Prefix completion 要求最后一条是 `assistant`** → 末尾 assistant 被接受。<br>③ **Chat Completion 支持中途插入 system 消息**，但**不支持**中途插入 tool call（D-tools）。<br>④ 官方多轮示例严格交替（D-multiturn） |
| 结论 | ⚪ **「连续同 role 会被接受 / 被拒绝 / 被合并」在两家均未证实**。不要把 Anthropic 的「合并连续同 role」行为外推到这两家；也不要假设它们会报错 | ⚪ 同左 |

> 与 01 号文档 3.3 节的分工：那一节已确证 Anthropic **合并**连续同 role；**这两家是文档缺口**，不是「已知接受」。fs-agent 若要构建「多说话人共享 transcript」，不能依赖 provider 端的行为——只能靠自己保证 role 交替（如 AutoGen 的 per-agent 视角重写），或把说话人信息放进文本 / `name`。

---

## 5. 流式响应结构

### 5.1 Kimi（Chat Completions，SSE）

来源：K-chat 的 `ChatCompletionChunk` / `ChoiceDelta` schema 与流式示例；K-stream 的解析说明。

**传输层**

- `Content-Type: text/event-stream`。
- 每帧 `data: <JSON>`，**以两个换行 `\n\n` 结束**；以 `data: [DONE]` 收尾。
- ⚠️ 官方强调：「**always use `data: [DONE]` to determine whether the data has been fully transmitted, not `finish_reason` or any other means**」「until `data: [DONE]` arrives, the message should be considered **incomplete**」。
- 非流式与流式的对象类型不同：`object` 为 `chat.completion` vs `chat.completion.chunk`。

**文本 delta**（K-chat 示例，逐字对应）

```text
data: {"id":"cmpl-xxx","object":"chat.completion.chunk","created":1698999575,"model":"kimi-k2.6","choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":null}]}

data: {"id":"cmpl-xxx","object":"chat.completion.chunk","created":1698999575,"model":"kimi-k2.6","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

...

data: {"id":"cmpl-xxx","object":"chat.completion.chunk","created":1698999575,"model":"kimi-k2.6","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":19,"completion_tokens":13,"total_tokens":32,"cached_tokens":12}}

data: [DONE]
```

结构要点：

- **`role` 只在第一个 chunk 出现**，后续 chunk 不重复。
- `delta` 可承载**三类**增量数据（K-stream 原文）：`content`、`reasoning_content`（thinking 模型，**先于 content 和 tool_calls**）、`tool_calls`。
- `finish_reason` **只在最后一个 chunk** 出现；enum 见 K-chat `ChoiceDelta`：`stop` / `length` / `tool_calls` / `null`。

**tool-call delta：参数是分片追加的，且多 call 用 `index` 区分**

K-stream 原文：

> "`tool_calls`: tool calls. Fragments of the same tool call **share the same `index`**; `id`, `type`, and `function.name` appear **only once, in the first fragment**, while **`function.arguments` arrives as JSON string fragments that must be appended (never overwritten)** and parsed only after the stream ends."

K-tools 原文：

> "we will specify the `tool_call.id` and `tool_call.function.name` **in the initial data chunk**, and only `tool_call.function.arguments` will be output in subsequent chunks"
>
> "if Kimi returns multiple `tool_calls` at once, we will use an additional field called **`index`** to indicate the index of the current `tool_call`, so that you can correctly concatenate the `tool_call.function.arguments` parameters"

- ⚠️ **文档不一致**：K-chat 的 OpenAPI `ChoiceDelta.tool_calls[]` **没有声明 `index` 字段**（只有 `id`/`type`/`function.name`/`function.arguments`），但 K-tools 与 K-stream 的解析代码与文字都依赖 `index`。→ 以 guide 为准，**Rust 结构体里必须给 `tool_calls[].index` 留字段**（可 optional 兜底）。
- 官方示例的解析顺序建议：先检查 `delta.tool_calls` 是否存在来判断本轮是否含 tool call；**`delta.content` 会先输出、`delta.tool_calls` 后输出**，故须等内容输出完再判定 tool_calls。

**`stream_options.include_usage`（Kimi 行为，K-chat + K-stream）**

- 默认 `false`。
- true 时：**所有 chunk 都带 `usage` 字段**，但除最后一个之外值为 `null`；且**在 `data: [DONE]` 之前额外发一个统计 chunk，其 `choices` 是空数组 `[]`**，`usage` 为整次请求的总量。
- ⚠️ 原文提示：「**If the stream is interrupted, you may not receive the final usage chunk**」。
- 非流式响应的 `usage` 形如 `{prompt_tokens, completion_tokens, total_tokens, cached_tokens}`。

### 5.2 DeepSeek（Chat Completions，SSE）

来源：D-chat 的 `200 (Streaming)` schema（`object: chat.completion.chunk`）。

**对象结构与字段**（D-chat schema 原文）

```
id            string
choices       object[]  →  { delta, logprobs, finish_reason, index }
created       integer   "Each chunk has the same timestamp."
model         string
system_fingerprint string  "This fingerprint represents the backend configuration that the model runs with."
object        string    Possible values: [ chat.completion.chunk ]
```

`delta` 的字段（schema 给出的完整形态）：

```json
{
  "delta": {
    "content": "string",
    "reasoning_content": "string",
    "role": "assistant",
    "tool_calls": [
      {
        "index": 0,
        "id": "string",
        "type": "function",
        "function": { "name": "string", "arguments": "string" }
      }
    ]
  },
  "logprobs": { "content": [ { "token": "string", "logprob": 0, "bytes": [0], "top_logprobs": [ ... ] } ] }
}
```

要点：

- **`tool_calls[].index` 在 DeepSeek 的 schema 里是显式声明的**（与 Kimi OpenAPI 的缺口相反）。
- `delta.reasoning_content` 与 `delta.content` 同级；D-thinking 的流式示例写法是 `chunk.choices[0].delta.reasoning_content` / `.delta.content`。
- `finish_reason` enum 比 Kimi 多两个：`stop` / `length` / `content_filter` / `tool_calls` / **`insufficient_system_resource`**（推理系统资源不足被中断）/ **`aborted`**（生成被中断）。
- `logprobs` 也在 chunk 的 choice 级别出现。

**`stream_options.include_usage`（DeepSeek 行为，D-chat）** —— 与 Kimi 有一处**明确不同**：

> "**Must be set together with `stream: true`; if `stream` is not set to `true`, the API returns a `400` error.**"
>
> "If set to `true`, all chunks in the stream will include a `usage` field, whose value is `null` on every chunk except the last one. If omitted or set to `false`, the `usage` field is **absent** from all chunks except the last one."
>
> "Either way, the last chunk before the `data: [DONE]` message carries the token usage statistics for the entire request in its `usage` field. **Note that no separate usage-only chunk is emitted**: the statistics ride on the last **content** chunk, whose `choices` array always contains **exactly one element** that carries no new content and a non-null `finish_reason`."

| 差异点 | Kimi | DeepSeek |
| --- | --- | --- |
| `stream_options` 与 `stream:false` 同用 | ⚪ 未文档化 | **400** |
| `include_usage:true` 时 usage 出现在哪 | **单独的统计 chunk，`choices: []`** | **没有单独 chunk**；统计搭在最后一个 content chunk 上，该 chunk `choices` 恰好一个元素、无新内容、`finish_reason` 非空 |
| `include_usage` 缺失/false 时 | ⚪ 未明确（示例里最后一个 content chunk 也带 `usage`） | 最后一个 chunk **之外**的 chunk **没有** `usage` 字段；最后一个仍有 |

**DeepSeek 非流式响应对象**（D-chat）—— 供对照：`{id, choices[{finish_reason, index, message{content, reasoning_content, tool_calls[{id,type,function{name,arguments}}], role}, logprobs}], created, model, system_fingerprint, object:"chat.completion", usage{...}}`。`usage` 含 `completion_tokens`、`prompt_tokens`、`prompt_tokens_details{cached_tokens, prompt_cache_hit_tokens, prompt_cache_miss_tokens}`、`total_tokens`、`completion_tokens_details.reasoning_tokens`。

---

## 6. 错误码与限额

### 6.1 Kimi

来源：K-limits / K-errors / K-intro / K-vision。

**速率限制按「累计充值额」分档，且在 user 级、跨所有模型共享**（K-limits / K-intro）：

| Tier | 累计充值 | 并发 | RPM | TPM | TPD |
| --- | --- | --- | --- | --- | --- |
| Tier0 | \$1 | 1 | 3 | 500,000 | 1,500,000 |
| Tier1 | \$10 | 15 | 100 | 2,000,000 | Unlimited |
| Tier2 | \$20 | 40 | 100 | 3,000,000 | Unlimited |
| Tier3 | \$100 | 50 | 200 | 3,000,000 | Unlimited |
| Tier4 | \$1,000 | 60 | 200 | 4,000,000 | Unlimited |
| Tier5 | \$3,000 | 100 | 300 | 5,000,000 | Unlimited |

- 「**Rate limits are enforced at the user level, not the key level**」「Currently, we **share rate limits across all models**」。
- **限流的计算口径很特别**（K-intro）：网关按请求里的 `max_completion_tokens` 预估来算「是否触顶」，「we will use this parameter to calculate the rate limit ... **regardless of the actual number of Tokens generated**」；**计费**才按实际生成 token。→ fs-agent 把 `max_completion_tokens` 设很大会**提前撞限流**。
- 至少充值 \$1 才能开始使用；累计 \$5 送 \$5 代金券；代金券不计入累计充值；风控限流**触发后不可解除**。

**错误码**（K-errors，`error.type` + 典型 message）

| HTTP | error type | 说明 |
| --- | --- | --- |
| 400 | `content_filter` | 输入或输出触发内容安全审查 |
| 400 | `invalid_request_error` | 请求格式 / 缺参 / 类型错；**input token 超上下文**；**prompt tokens + max_tokens 超模型规格**；文件相关错误（purpose 非法、>100MB、0 字节、文件数过多） |
| 401 | `invalid_authentication_error` / `incorrect_api_key_error` | key 无效或缺失；**跨平台 key 混用**（K-errors 明确） |
| 403 | `permission_denied_error` | API 未开放 / 越权取用户信息 / IP 不在白名单 |
| 404 | `resource_not_found_error` | 模型名错或账号无权（也用于调用已停用模型） |
| 429 | `engine_overloaded_error` | 服务节点高负载（如高峰），按 `Retry-After` 退避；「topping up or upgrading your tier **does not resolve it**」 |
| 429 | `exceeded_current_quota_error` | 余额不足 / 账号被禁用 / Token 额度不足 |
| 429 | `rate_limit_reached_error` | 组织级**并发** / **RPM** / **TPM** / **TPD** 触顶 |
| 499 | `client_closed_request` | 客户端提前断开（流式被代理切断 / 用户取消） |
| 500 | `server_error` / `unexpected_output` | 内部错误，带 `request_id` |
| 503 | `server_unavailable` | 临时不可用（扩缩容/维护） |
| 504 | `504 Gateway Time-out` | 900 秒无响应，网关返回 HTML 超时页；「Common with **long non-streaming requests**. Use streaming output (`stream: true`)」 |

- **超时**：K-intro「Generally, we set a **2 hours** timeout. If a single request exceeds this time, we will return a **504**」。
- **请求体大小**：K-vision「Image quantity: The Vision model has **no limit on the number of images**, but ensure that the **request body size does not exceed 100M**」→ 文档化的请求体上限是 **100 MB**；另有「Due to our overall request body size limitations, very large videos must be processed using the file upload method」。
- 其余视觉上限（K-vision）：**不建议**图像分辨率 > 4k(4096×2160)，视频 > FHD(1920×1080)；SVG **不支持**（含 base64）；**不支持 URL 形式图片**（只支持 base64 与 file ID）。
- 并发语义（K-limits）：「**Concurrency**: The maximum number of requests from you that we can process **at the same time**」；「when the cluster load reaches its capacity limit, we may take **temporary measures** to adjust the rate limits」。

### 6.2 DeepSeek

来源：D-ratelimit / D-errors / D-vision / D-pricing。

**并发限制（账号级，与 API Key 无关）**

| | `deepseek-flash` | `deepseek-v4-pro` |
| --- | --- | --- |
| 并发上限 | **2500** | **500** |

- 「A request counts as **one concurrent connection from the time it is sent until the model response is complete**」。
- 「Concurrency limits are calculated at the **account level**, regardless of which API Key is used」。
- 「within the concurrency limit will receive a response; when the concurrency limit is exceeded, you will receive an **HTTP 429**」。
- 可申请扩容，**不额外收费**（官方表单链接见 D-ratelimit）。
- ⚠️ **注意：DeepSeek 文档没有 RPM/TPM 的表述，只有并发**；这与 Kimi 的 RPM/TPM/TPD 是**不同的限流模型**。

**`user_id` 隔离**（D-ratelimit）

- 三个用途：**Content Safety Isolation**、**KVCache Isolation**、**Scheduling Isolation**。
- 对普通 API 用户：「all `user_id` values are **combined** for concurrency limit calculation」。
- 对已扩容用户：账号总量限 + **每个 `user_id` 再限**（空 id 视为一个特殊 `user_id`），每 `user_id`：flash 2500 / v4-pro 500；超限该 `user_id` 的请求收 **429**。
- 格式：`[a-zA-Z0-9\-_]+`，最长 512。

**Keep-alive / 超时**（D-ratelimit）

- 等待响应期间：**非流式请求持续返回空行**；**流式请求持续返回 SSE keep-alive 注释 `: keep-alive`**。→ 自己解析 HTTP 时必须能容忍这两种噪声。
- 「If the request has **not started inference after 10 minutes**, the server will **close the connection**.」

**错误码**（D-errors）

| CODE | 说明 |
| --- | --- |
| 400 | Invalid Format：请求体格式非法 |
| 401 | Authentication Fails：API key 错 |
| 402 | Insufficient Balance：余额耗尽 |
| 422 | Invalid Parameters：请求含非法参数 |
| 429 | Rate Limit Reached：「You are sending requests too quickly. Please pace your requests reasonably. We also advise users to **temporarily switch to the APIs of alternative LLM service providers, like OpenAI**」 |
| 500 | Server Error：重试后联系 |
| 503 | Server Overloaded：高流量过载，稍后重试 |

注意：DeepSeek **有 402（余额）**，Kimi 把余额不足放在 **429 `exceeded_current_quota_error`** → **两家的「欠费」错误码不同，Rust 客户端不能共用一套重试分类**。

**请求体大小与视觉上限**（D-vision《Limits》表）

| Limit | Value |
| --- | --- |
| Supported formats | JPEG, PNG, GIF, WebP |
| External URL length | 8192 字符 |
| **Request body size** | **48 MiB** |
| Max single image size (base64 / external URL) | 32 MiB |
| Max single image size (Files API `file_id`) | 64 MiB |
| Max images per request | 600 |
| Max total image size per request | 64 MiB（不含 `file_id` 图）～ 200 MiB（含 `file_id` 图） |
| Max image dimension | 8192 px/边；请求内 ≥15 张图时降为 4096 px/边 |

→ **文档化的请求体上限：DeepSeek 48 MiB，Kimi 100 MB**（两家不同，须分别设 `max_body` 类配置）。

---

## 7. 多智能体 / 会话状态原语：确认**两家都没有**

### 7.1 结论

**两家都**：
1. **没有** Assistants / Threads API（OpenAI Assistants 那套）。
2. **没有**任何 provider 侧保存会话历史、可跨请求续接的 primitive。
3. **都**在文档里**明确写着 API 是 stateless**。
4. 虽然有 **Responses API**（OpenAI 的 Responses 格式），但它的 `store` / `previous_response_id` / `conversation` **都被明确置空 / 不支持**——只是**请求/响应格式兼容层**，不是状态层。

→ 对 fs-agent：「多 agent 在一个 session 里讨论」的**全部会话状态必须由 fs-agent 自己持有并在每次请求里全量重放**。provider 侧没有可依赖的 thread/assistant 对象。

### 7.2 Kimi 的证据

**Chat Completions 明确 stateless**（K-chat "Multi-turn Conversations" 与 K-multiturn）：

> "**The Kimi API is stateless and does not retain conversation history.** To implement multi-turn dialogue, append the previous assistant reply (and any tool results, if applicable) back into the `messages` array before sending the next request."
>
> "the Kimi API is **stateless** and has **no memory of its own**: across multiple requests, the model doesn't know what you asked in a previous request and won't remember any context"

**Responses API（K-responses）—— 存在，但对会话状态一律「恒为空」**：

- 端点只支持 `kimi-k3`：「This endpoint currently supports `kimi-k3`.」
- `ResponsesRequest` 的字段是：`model`、`input`、`instructions`、`stream`、`max_output_tokens`、`reasoning.effort`、`text.format`、`tools`、`tool_choice`、`include`、`prompt_cache_key`、`safety_identifier`。
  → **请求里根本没有** `store` / `previous_response_id` / `conversation` / `parallel_tool_calls`。
- `ResponsesResponse` 里这些字段的 schema 描述是硬编码常量：
  - `store`：`description: Always false.`
  - `background`：`description: Always false.`
  - `previous_response_id`：`description: **Always `null`.**`
  - `conversation`：`description: **Always `null`.**`
- `input` 的语义也印证无状态：「An array holds ordered typed items and may contain **conversation history**, tool calls, and tool results」→ 历史由**调用方**放进 `input`。

**Messages API（Anthropic 兼容，K-messages）**：`MessagesRequest` = `model` + `messages` + `max_tokens`(required) + 顶层 `system` + `stream` + `stop_sequences` + `tools` + `tool_choice` + `metadata.user_id` + `output_config.{effort, format}`。**没有任何 session/thread 字段**；`messages` 的说明仍是「If the last message is from the assistant, the model continues from that content (Partial Mode)」——即靠**调用方回放历史 + prefill**，不是 provider 状态。

**API 面清单（来自 llms.txt 的 API Reference 段）**：Chat Completions、Responses、Messages、List Models、Estimate Tokens、Check Balance、Verify Request Signature、Files（upload/list/retrieve/delete/content）、Batch（create/list/retrieve/cancel）。
→ **没有 Assistants / Threads / Runs / Vector Stores。**

### 7.3 与 fs-agent 相关的两个「准状态」能力（都不是会话原语）

1. **Kimi `prompt_cache_key`（K-chat）** —— 只是一个**缓存路由键**，不是会话对象。官方对 coding agent 的建议是「typically a **session id or task id** ... if the session is exited and later resumed, this value should **remain the same**」。它是**无状态 API 上的性能优化**，不改变「必须全量重放 messages」的事实。
2. **Kimi Batch API（llms.txt 索引）** —— 离线批处理（create/list/retrieve/cancel），**不是**同步多补全，不能替代 `n`。
3. **Kimi K3 动态工具消息 / DeepSeek 中途插入 system 消息** —— 允许在**一次请求的 messages 内部**改变 system 上下文的位置，属于请求构造能力，**不是**跨请求状态。

### 7.4 DeepSeek 的证据

**Chat Completions 明确 stateless**（D-multiturn）：

> "The DeepSeek `/chat/completions` API is a **"stateless" API**, meaning **the server does not record the context of the user's requests**. Therefore, the user must concatenate all previous conversation history and pass it to the chat API with each request."

**Responses API（D-responses）兼容表 —— 直接逐条否定状态原语**：

| 参数 | 支持状态（D-responses 原文） |
| --- | --- |
| `previous_response_id` | **Not supported (stateless API)** |
| `conversation` | **Not supported (stateless API)** |
| `store` | **Not supported. The response always carries `store: false`** |
| `background` | Not supported |
| `metadata` | Not supported |
| `include` | Not supported |
| `prompt` | Not supported |
| `truncation` | Not supported. Requests exceeding the context window return a 400 error |
| `service_tier` | Not supported |
| `safety_identifier` | Not supported |
| `prompt_cache_key` / `prompt_cache_retention` | Not supported. Context caching is managed automatically |
| `context_management` | Not supported |
| `stream_options` | Not supported |
| `parallel_tool_calls` | **Ignored**（parallel tool calling is always enabled） |
| `max_tool_calls` | **Ignored** |

- 「**Unsupported parameters are silently ignored and do not cause errors**, so existing Responses API clients can connect without modification.」→ 对 Rust 客户端而言，**「传了没报错」不等于「生效」**。
- 响应对象同理：「Fields that depend on unsupported capabilities always take fixed values (e.g. `store: false`, `previous_response_id: null`, `parallel_tool_calls: true`)」。
- 该 Responses API 的**存在动机是 Codex 兼容**：「To meet the demand for **Codex**, our API now supports the Responses API format」。
- `input` 支持 `message` / `function_call` / `function_call_output` / `reasoning` / `custom_tool_call`(`apply_patch`) 等 item 类型，**全部由调用方在 `input` 里回放**。

**API 面清单（D-chat 侧边栏 API Reference）**：Chat Completions、Responses、FIM Completion (Beta)、Lists Models、Get User Balance、Files。
→ **没有 Assistants / Threads / Runs / Vector Stores。**

**顺带一条与「多 agent 共享 transcript」直接相关的差异**（D-tools）：DeepSeek 三种 API 格式对「中途插入消息」的能力不同——
- Anthropic API `/messages` 与 Responses API：**支持**中途插入 tool call 消息，**也支持**中途插入 system 消息。
- **Chat Completion API：不支持中途插入 tool call**，但**支持中途插入 system 消息**。

→ 如果 fs-agent 想在共享 transcript 中间插入别的 agent 的 tool 调用记录，**DeepSeek 的 Chat Completions 做不到**（官方明确说改用 Anthropic 或 Responses 面）。

---

## 8. ⚪ 无法从主来源验证的清单

以下条目**官方一手文档没有写**，本文不推断、不作为事实使用：

1. **DeepSeek 传 `n` 会怎样** —— D-chat 全文无 `n`。既不支持也无拒绝声明；是忽略、报 422、还是接受，**未证实**。（Kimi 侧已证实：`n>1` → 400。）
2. **DeepSeek `seed` 的可复现性** —— 无任何 `seed` 相关文档；也不存在「同 seed 同输出」的承诺。
3. **两家「连续多条同 role 消息」的行为** —— 接受 / 拒绝 / 合并，**两家均未文档化**。已知的只有边界事实：两家都接受**最后一条为 assistant**（prefill）；DeepSeek Chat Completion 支持中途插 system、不支持中途插 tool call；Kimi K3 支持任意位置插动态工具 system 消息。
4. **`logprobs` 在 thinking/thinking 类模型下是否可用** —— 两家都未说明与 thinking 的交互。
5. **DeepSeek thinking 模式下 `stop` / `logprobs` / `response_format` 的限制** —— D-thinking 只说明了 `temperature`/`presence_penalty`/`frequency_penalty` 无效、`top_p` 有下限、`tool_choice` 的 `required`/具名不支持；其余字段与 thinking 的交互**未文档化**。
6. **Kimi K2.6 / K2.7-code 的 `max_completion_tokens` 默认值与上限** —— K-chat 只给了 K3 的（131072 / 1048576）。
7. **Kimi `stream_options` 与 `stream:false` 同用的行为** —— 未文档化（DeepSeek 明确是 400，Kimi 没说）。
8. **DeepSeek `prefix`（Beta）与 thinking 模式的交互、与 `response_format` 的交互** —— D-prefix 页未提。
9. **DeepSeek Chat Completions 是否有文档外的 `parallel_tool_calls`** —— 字段不存在于 D-chat；只能确定 Responses API 侧「parallel tool calling is always enabled」。
10. **DeepSeek 的 RPM / TPM 限制** —— 官方只文档化了**并发数**（2500/500），未见 RPM/TPM/TPD 表述；是否有文档外的 RPM 限制未证实。
11. **Kimi 的 `n` 在**非当前**模型（已弃用的 `moonshot-v1` 等）上的取值** —— 模型已 404，未验证。
12. **Kimi / DeepSeek 是否提供任何未在文档索引中列出的会话/assistant primitive** —— 本文只核查了两家官方文档站与 llms.txt 索引；不能排除闭源未文档化的企业功能。
13. **Kimi OpenAPI 与 guide 的 `tool_calls[].index` 不一致**（OpenAPI 未声明，guide 依赖之）—— 已记录为不一致；**实际线上是否总带 `index`** 未验证（未调用 API）。

---

## 9. 来源一览（全部一手官方文档，抓取于 2026-09-12 UTC）

### Kimi / Moonshot

| ID | URL |
| --- | --- |
| K-chat | <https://platform.kimi.ai/docs/api/chat> · Markdown: <https://platform.kimi.ai/docs/api/chat.md> |
| K-models-param | <https://platform.kimi.ai/docs/api/models-overview> · <https://platform.kimi.ai/docs/api/models-overview.md> |
| K-modellist | <https://platform.kimi.ai/docs/models> · <https://platform.kimi.ai/docs/models.md> |
| K-limits | <https://platform.kimi.ai/docs/pricing/limits> · <https://platform.kimi.ai/docs/pricing/limits.md> |
| K-errors | <https://platform.kimi.ai/docs/api/errors> · <https://platform.kimi.ai/docs/api/errors.md> |
| K-intro | <https://platform.kimi.ai/docs/introduction> · <https://platform.kimi.ai/docs/introduction.md> |
| K-stream | <https://platform.kimi.ai/docs/guide/utilize-the-streaming-output-feature-of-kimi-api> · `.md` |
| K-tools | <https://platform.kimi.ai/docs/guide/use-kimi-api-to-complete-tool-calls> · `.md` |
| K-partial | <https://platform.kimi.ai/docs/guide/use-partial-mode-feature-of-kimi-api> · `.md` |
| K-thinking | <https://platform.kimi.ai/docs/guide/use-thinking-models> · `.md` |
| K-cache | <https://platform.kimi.ai/docs/guide/use-context-caching-feature-of-kimi-api> · `.md` |
| K-multiturn | <https://platform.kimi.ai/docs/guide/engage-in-multi-turn-conversations-using-kimi-api> · `.md` |
| K-vision | <https://platform.kimi.ai/docs/guide/use-kimi-vision-model> · `.md` |
| K-responses | <https://platform.kimi.ai/docs/api/responses> · <https://platform.kimi.ai/docs/api/responses.md> |
| K-messages | <https://platform.kimi.ai/docs/api/messages> · <https://platform.kimi.ai/docs/api/messages.md> |
| K-cn-chat | <https://platform.moonshot.cn/docs/api/chat>（中国站，同构，base URL `https://api.moonshot.cn/v1`） |
| （索引） | <https://platform.kimi.ai/docs/llms.txt> |

### DeepSeek

| ID | URL |
| --- | --- |
| D-chat | <https://api-docs.deepseek.com/api/create-chat-completion> |
| D-pricing | <https://api-docs.deepseek.com/quick_start/pricing> |
| D-ratelimit | <https://api-docs.deepseek.com/quick_start/rate_limit> |
| D-errors | <https://api-docs.deepseek.com/quick_start/error_codes> |
| D-thinking | <https://api-docs.deepseek.com/guides/thinking_mode> |
| D-json | <https://api-docs.deepseek.com/guides/json_mode> |
| D-tools | <https://api-docs.deepseek.com/guides/tool_calls> |
| D-prefix | <https://api-docs.deepseek.com/guides/chat_prefix_completion> |
| D-cache | <https://api-docs.deepseek.com/guides/kv_cache> |
| D-multiturn | <https://api-docs.deepseek.com/guides/multi_round_chat> |
| D-responses | <https://api-docs.deepseek.com/guides/responses_api> |
| D-vision | <https://api-docs.deepseek.com/guides/vision> |

（DeepSeek 文档站同步提供中文版，将 URL 中的路径前缀替换为 `/zh-cn/` 即可，如 <https://api-docs.deepseek.com/zh-cn/api/create-chat-completion>。）

---

## 10. 一页速查：两家差异（不平均）

| 维度 | Kimi | DeepSeek |
| --- | --- | --- |
| 当前模型 | `kimi-k3`(1M) / `kimi-k2.7-code`(256K) / `kimi-k2.7-code-highspeed`(256K) / `kimi-k2.6`(256K) | `deepseek-flash`(1M) / `deepseek-v4-pro`(1M)，最大输出 384K |
| `n` | **固定 1**，`n>1` → 400 | **无该字段**（行为未证实） |
| `seed` | 无 | 无 |
| `top_k` | 无 | 无 |
| `temperature` | **不能传**（固定 1.0 / K2.6 非 thinking 0.6） | 可传 0–2（默认 1），**thinking 下无效** |
| `top_p` | **不能传**（固定 0.95） | 可传，默认 1；thinking 下限 0.95；非 thinking 固定 1.0 |
| penalties | **固定 0，传了报错** | **deprecated，传了静默无效** |
| `logprobs`/`top_logprobs` | ✅ / 0–20 | ✅ / ≤20 |
| `stop` | ≤5 个，每个 ≤32 bytes | ≤16 个 |
| `response_format` | `text`/`json_object`/**`json_schema`(MFJS)** | `text`/`json_object`（无 json_schema；Responses API 才有） |
| tools `strict` | 默认 **true** | 默认 **false**，Beta（需 `/beta`） |
| `tool_choice: required` | 仅 `kimi-k3` | Chat 面支持，但 **thinking 下 400** |
| `parallel_tool_calls` | 无该字段 | Chat 面无；Responses 面 **Ignored（恒开）** |
| prefill | `partial:true`（正式端点） | `prefix:true`（**Beta** base URL） |
| prefill + thinking | 必须回传 `reasoning_content` | 未文档化 |
| thinking 控制 | K3: `reasoning_effort`(默认 max)；K2.x: `thinking` | `thinking.type` + `reasoning_effort`(默认 high) |
| 缓存 | 自动 **+ `prompt_cache_key`**（>256 tokens 才缓存） | 自动，**无参数**；`user_id` 做 KVCache 隔离 |
| 限流模型 | 并发 + **RPM/TPM/TPD**（按充值分档，user 级） | **只有并发** 2500/500（账号级）+ 每 `user_id` |
| 欠费错误码 | 429 `exceeded_current_quota_error` | **402** |
| 请求体上限 | **100 MB** | **48 MiB** |
| Responses API | 有（仅 `kimi-k3`），`store`/`previous_response_id`/`conversation` **恒 false/null** | 有（Codex 兼容），三者**明确 Not supported** |
| Assistants/Threads | **无** | **无** |
| 状态 | **stateless（文档明说）** | **stateless（文档明说）** |
