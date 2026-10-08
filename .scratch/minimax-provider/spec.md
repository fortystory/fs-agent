# MiniMax：第三个厂商与 M3 系模型

Status: 2/2 done（2026-10-08 由维护者的要求直接折成 spec 并拆两张票，同日落地）

维护者订了 MiniMax 的 **M Plan**（订阅 Key 前缀 `sk-cp-`，只吃 M Plan 的额度，与按量计费的
API Key 不通用），想把它接进 heng。MiniMax 同时提供 **OpenAI 兼容**与 **Anthropic 兼容**两种
协议，所以先要回答的是「heng 这边要不要额外开发」。

答案是**要一点，但不写新适配器**：

- heng 的会话调用只有一套线上形状 —— OpenAI 兼容的 `<base>/chat/completions`
  （[`src/provider/openai.rs`](../../src/provider/openai.rs) 是唯一的适配器；Anthropic Messages
  在这个仓库里只出现在联网搜索后端，那是工具自己的出网，与会话 provider 无关）。所以
  MiniMax 的 `/anthropic` 入口对 heng **无效**，除非另写一个适配器。
- OpenAI 兼容那一侧形状对得上（SSE、`reasoning_content`、`max_completion_tokens`、
  `stream_options.include_usage`、`prompt_tokens_details.cached_tokens`、Bearer 认证），
  但**不是纯配置**：heng 有一条硬规矩 —— **未登记的 model id 是启动错误，绝不静默降级**
  （[`caps_for`](../../src/provider/capability.rs) + [`validate_models`](../../src/cli.rs)），
  而 `ModelCaps.vendor` 是只有 Kimi / DeepSeek 两档的枚举
  （[`Vendor`](../../src/config.rs)）。

于是这份 spec 是三件小事：**加第三个厂商**、**登记两个模型**、**补两处厂商特有的行为**
（输出格式开关与缓存用量字段），外加文档与一张 `config.toml` 样例。

来源与两个已定的选择（2026-10-08 维护者答复）：

- 两条内建 profile 都要：国际站 `https://api.minimax.io/v1`（`MINIMAX_API_KEY`）与国内站
  `https://api.minimax.cn/v1`（`MINIMAX_CN_API_KEY`）。M Plan 的订阅站是国内站。
- 本次只登记 `MiniMax-M3.1-Flash-Preview` 与 `MiniMax-M3`，两个都发 `reasoning_split: true`。

一手材料（MiniMax 官方文档，2026-10-08 读）：

- <https://platform.minimax.cn/docs/m-plan/quickstart>：M Plan 的订阅 Key 与按量 Key 不通用。
- <https://platform.minimax.cn/docs/api-reference/text-openai-api.md> / [`.io` 同一页](https://platform.minimax.io/docs/api-reference/text-openai-api.md)：
  两条 base_url、模型与上下文窗口、`reasoning_effort` 只对 M3.1 生效、`reasoning_split`、
  `stream_options.include_usage`、`max_completion_tokens`。
- <https://platform.minimax.io/docs/api-reference/text-prompt-caching.md>：缓存是**自动**的
  （没有 `prompt_cache_key` 这类显式参数），512 个输入 token 起才缓存，命中的读数是
  OpenAI 形状的 `usage.prompt_tokens_details.cached_tokens`。

## 问题陈述

1. **模型进不了门。** 能力表是本仓库唯一的「这个 model id 长什么样」的登记处，没登记就是
   启动错误。这条规矩是刻意的（宁可拒绝，也不猜一个厂商行为），所以接一个新厂商的第一件事
   永远是带数字进表。
2. **厂商是枚举的一档。** `ModelCaps.vendor` 不可能缺席 —— 用量的缓存字段按它分派
   （[`normalize_usage`](../../src/provider/openai.rs)）。把它填成 Kimi 或 DeepSeek 能编译，
   但错误提示、密钥域守卫、以及「两个讨论者是不是同厂商」都会跟着说错话。
3. **两处厂商差异要落地，不是配置能表达的。**
   - **思考的出口**：M3 在 OpenAI 兼容接口下默认把思考写进 `content` 的 `<think>` 标签里
     （M3.1 默认走 `reasoning_content`）。`<think>` 混在正文里会进事件流、进重放、也进对话
     视图 —— 那不是「思考」，是污染。厂商给了一个开关 `reasoning_split: true`，而
     `build_body` 今天没有发厂商特有顶层字段的出口。
   - **缓存的读数**：MiniMax 报 `prompt_tokens_details.cached_tokens`，没有顶层
     `cached_tokens` 也没有 `prompt_cache_miss_tokens`。少了这一条，一次命中的对话在界面
     上会读成「零缓存」，而 DayLedger 与用量页正是靠它说事的。
4. **两条系统要分开。** 与 Kimi 的国际/开放平台同形：`api.minimax.io` 与 `api.minimax.cn`
   是两套（账号与 Key 不通用），所以是**两条** profile，不是一条加 env 覆盖。跨域守卫按
   **Key 的出身**判（既有机制），于是把 MiniMax 的 Key 指到别家主机时会被结构性地拦住。

## 方案

```text
配置      [providers.minimax] / [providers.minimax-cn]（内建，Key 从环境变量读）
   │
模型      [models."MiniMax-M3"]（只用来覆盖参数）
   │
能力表    KNOWN_MODELS + caps_for：窗口、工具、思考、max_completion_tokens、缓存地板
   │
适配器    build_body 发 reasoning_split · normalize_usage 读 prompt_tokens_details.cached_tokens
```

四层都不动既有的形状：没有新 trait、没有新适配器、没有新的线上协议分支。

## 实现决定

### §1 两条内建 profile

照 [`BUILTIN_PROVIDERS`](../../src/config.rs) 现有三条的写法加两条：

| profile | `base_url` | key 环境变量 |
| --- | --- | --- |
| `minimax` | `https://api.minimax.io/v1` | `MINIMAX_API_KEY` |
| `minimax-cn` | `https://api.minimax.cn/v1` | `MINIMAX_CN_API_KEY` |

两条都是 `Vendor::MiniMax`。`<PREFIX>_BASE_URL` 的环境变量覆盖照既有的 `env_prefix` 自动成立
（`minimax-cn` → `MINIMAX_CN_BASE_URL`），所以想换端点不必改配置文件里的 `base_url`。

Key 的环境变量名与 MiniMax 自己的文档一致（文档里的 `OPENAI_API_KEY` 是给通用工具用的通用
拼法，本仓库的惯例是「厂商名_API_KEY」，与 `MOONSHOT_API_KEY` / `DEEPSEEK_API_KEY` 同形）。

### §2 `Vendor::MiniMax`

[`Vendor`](../../src/config.rs) 加第三个变体，三处一起改：

- `as_str()` → `"MiniMax"`（诊断里出现）。
- `hosts()` → `&["api.minimax.io", "api.minimax.cn"]`。跨域守卫因此对两个站都放行、对别家
  主机一律拒 —— 与 Kimi 那条「一个厂商可以有多台主机」的先例同形。
- 枚举是 `#[non_exhaustive]`，但 crate 内的 match 仍要穷尽，所以编译器会把
  `normalize_usage` 与 `auth_hint` 两处一起点出来（这正是想要的）。

### §3 能力表条目

`KNOWN_MODELS` 加 `MiniMax-M3.1-Flash-Preview` 与 `MiniMax-M3`，各一条 `caps_for` 分支；
`BUILTIN_MODELS` 也各加一条（默认都指向 `minimax` 国际站；想用国内站在 `[models.…]` 里把
`provider` 改成 `minimax-cn`）。

两个模型的事实（MiniMax 官方 API 参考，2026-10-08）：

| 字段 | M3.1-Flash-Preview | M3 | 出处/理由 |
| --- | --- | --- | --- |
| `context_window` | 1_048_576 | 1_048_576 | 官方表：1,000,000 |
| `max_output_tokens` | 1_048_576 | 1_048_576 | **官方没给输出上限**，按「不比窗口更紧」记（与 `k3_caps` 同办），注释里点明这处缺口 |
| `supports_tools` | true | true | 官方支持 `tools` |
| `supports_reasoning` | true | true | 思考默认开启 |
| `supports_reasoning_effort` | true | **false** | 「`reasoning_effort` … 仅 M3.1-Flash-Preview 生效」 |
| `requires_reasoning_replay` | true | true | 多轮工具调用里要求原样回带思考（官方「特别注意」） |
| `supports_temperature` / `top_p` | true / true | true / true | 取值范围 [0,2] / [0,1]，默认 1 / 0.95 |
| `supports_prompt_cache_key` | **false** | **false** | 缓存是自动的，没有这个参数 |
| `supports_stream_options` | true | true | 官方记了 `stream_options.include_usage` |
| `max_tokens_field` | MaxCompletionTokens | MaxCompletionTokens | 官方：「新接入建议使用此字段」 |
| `min_cacheable_tokens` | 512 | 512 | 官方：「512 个输入 token 起才缓存」 |
| `reasoning_split`（新位） | true | true | 见 §4 |

不登记的 M2.x 一族（204.8K 窗口那一串）留到有人真用时再说（见「明确不做」）。

### §4 `reasoning_split`：思考走 `reasoning_content`

`ModelCaps` 加一个布尔位（`reasoning_split`），`build_body` 在它为真时发顶层
`"reasoning_split": true`。

- **为什么发**：M3 默认把思考留在 `content` 的 `<think>` 标签里。那会污染四层 —— 事件流里的
  正文、投影给下一次调用重放的内容、对话视图、以及把它当正文读的任何下游。开了之后思考走
  `reasoning_content`，于是 heng 既有的那套（`TextDelta` / `ReasoningDelta` 分流、重放
  `reasoning_content`）原样生效。
- **为什么对 M3.1 也发**：它默认已经分离，显式发一次是无害的幂等（官方只禁止把它设为
  `false`）。
- **为什么不是配置项**：它是这个厂商的**输出格式事实**，与 `max_tokens_field` 同类 —— 属于
  能力表，不该由用户每次去记。

### §5 用量：`prompt_tokens_details.cached_tokens`

`normalize_usage` 加 `Vendor::MiniMax` 分支：`cached = prompt_tokens_details.cached_tokens`，
`miss = prompt_tokens - cached`。

不写「复用 Kimi 分支的 fallback」那种省事的拼法：Kimi 分支先看顶层 `cached_tokens`，那在
MiniMax 的响应里根本不存在，留着会让人读成「这里也兼容顶层形状」。分支各自诚实。

### §6 401/403 的提示

`auth_hint` 加一条 MiniMax：订阅 Key（`sk-cp-` 前缀）只吃 M Plan 的额度、与按量 Key 不通用；
国际站与国内站是两套端点，Key 与 base_url 不能互换。这是既有先例（Kimi 那条就是为同一个坑
写的）。

`classify_status` 一个字不动：六个类别是按 HTTP 语义分的，与厂商无关，而 MiniMax 的限额
说法（429 / 403）落在既有的 `looks_like_quota` 判据里。

### §7 model id 里的点号与 TOML

`MiniMax-M3.1-Flash-Preview` 含点号，在 `config.toml` 里写它必须给键加引号：
`[models."MiniMax-M3.1-Flash-Preview"]`。不加引号会被 TOML 解析成嵌套表
（`models.MiniMax-M3."1-Flash-Preview"`），于是变成一个没配置的模型 id。这一条写进 README 的
样例，因为它不是能靠读错误消息猜出来的坑。

当作讨论者名字用时没有这条限制：`debaters = ["MiniMax-M3", "kimi-k3"]` 里的短形式名字就是
model id，而名字的判据只要求「非空、无空白、无控制字符、≤ 32 字符」。

### §8 测试与文档

- `tests/config_profiles.rs`：两条内建 profile 的 `base_url` 与 `key_env`；`Vendor::MiniMax`
  在 `ProviderProfile` 上落位；**跨域守卫**（`MINIMAX_API_KEY` 配到 `api.deepseek.com` 会被
  拒）。
- `tests/provider_adapter.rs`：`caps_for` 两条 id 的关键位；`build_body` 发出的 body 里有
  `reasoning_split: true`、`max_completion_tokens`、**没有** `prompt_cache_key`；
  `normalize_usage(Vendor::MiniMax, …)` 从嵌套字段读出缓存与 miss。
  既有的两条遍历断言（`every_builtin_model_id_has_a_capability_entry`、
  `every_registered_model_has_a_self_consistent_window`）会自动覆盖新条目。
- `README.md`：provider 表加两行、内置模型 id 那一行加两个、给一段 MiniMax 的配置样例
  （含 §7 的引号坑与价目表示例）。

## 明确不做

- **不写 Anthropic Messages 适配器。** MiniMax 那条 `/anthropic` 入口要能用，得实现一整套
  新的线上形状（`messages` 数组、`tool_use` / `tool_result` / `thinking` 内容块、SSE 事件名、
  缓存 `cache_control`），还会引出「同一个 harness 两种协议」的配置面。M Plan 走 OpenAI 兼容
  端点已经够了。
- **不登记 M2.x 一族**（`MiniMax-M2.7` / `-highspeed` / `M2.5` / `M2.1` / `M2`）。它们窗口是
  204.8K、思考形态与 `reasoning_effort` 支持面都不同，而现在没有人用。真要用时按同一张表
  带数字进来（`reasoning_split` 那个位就是为它们准备的）。
- **不做 `thinking` / `service_tier` / `reasoning_effort` 的新档位。** heng 的推理档位只有
  `low` / `high` / `max`（会话开始定死），MiniMax 的 `medium` / `xhigh` 不进来；`thinking`
  与 `service_tier` 没有旋钮（默认值就是我们要的）。
- **不接多模态输入。** M3.1 / M3 支持图片与视频，但 heng 的 `Message` 里还没有图片内容块
  （那是 [`image-input`](../image-input/seed.md) 那条种子的事）。
- **不加内置价目表。** 价格是配置（`[pricing.…]`，只作显示），README 给样例即可。
- **不动讨论池的缺省值。** 默认仍是 Kimi + DeepSeek；要不要把 MiniMax 放进池子是用户配置的
  事。

## 决定速查

| 问 | 落点 |
| --- | --- |
| Anthropic 端点能用吗 | 不能，会话侧只有 OpenAI 兼容（`明确不做`） |
| 要改哪些文件 | `src/config.rs`、`src/provider/capability.rs`、`src/provider/openai.rs`（§1–§5） |
| 端点与 Key | 两条 profile：`minimax` / `minimax-cn`（§1） |
| 登记哪两个模型 | `MiniMax-M3.1-Flash-Preview`、`MiniMax-M3`（§3） |
| M3 的思考 | 发 `reasoning_split: true`，走 `reasoning_content`（§4） |
| 缓存读数 | `prompt_tokens_details.cached_tokens`（§5） |
| TOML 里怎么写 | 带点号的 id 要给键加引号（§7） |
