# Provider trait、多模型、能力表与流式 chunk

Type: grilling
Status: resolved

## Question

决定 provider 抽象层的接缝。范围：**只做一个 OpenAI-compatible client**（`base_url` 可配），不做原生 Anthropic / Gemini 协议。

**本票已按新目的地大幅改写**：多 agent 讨论要求**同时配置多个模型**，且 provider 之间**逐字段行为不同**（详见 `.scratch/multi-agent-architecture/research/02-provider-call-surface.md`，本票的硬事实全部来自它）。

要回答：

1. **`Provider` trait 的边界**：输入输出是什么形状？流式返回是不是两种事件的流——**文本增量**与 **tool_call 增量**？注意 `async fn` in trait **不 dyn-compatible**（map 的 Notes），所以这里必须选一条：`async-trait` / `trait-variant` / 泛型单态化 / 枚举分发。
2. **多模型配置**：讨论者 = KIMI + DeepSeek 两种异构模型，各自可能有不同的 base URL、key、参数默认值。这是 trait 的一部分、还是一条独立的"模型档案"配置？注意 **Kimi 的 key 有平台域隔离**（混用返回 401），所以 base URL 必须与 key 同域。
3. **模型能力表（本票的硬需求）。** 两家逐字段行为不同，至少这些必须进能力表，否则上层无法写出一份安全的请求：
   - `temperature` / `top_p`：Kimi **不能传**（传了报错）；DeepSeek 非 thinking 可传、thinking 无效
   - penalties：Kimi **传了报错**；DeepSeek 静默无效
   - `n`：两家都不可用 → 上层的"独立首轮"必须自己拆成 N 次调用
   - `seed`：两家都没有 → 不可复现
   - `tool_choice: required`：只有 `kimi-k3` 支持，其余传了报错；DeepSeek thinking 下 `required` 与具名选择 400
   - `response_format`：Kimi 有 `json_schema`(MFJS)，DeepSeek Chat 面只有 `json_object`
   - 请求体上限（Kimi 100 MB / DeepSeek 48 MiB）、欠费错误码（429 quota / **402**）
   能力表是**编译期常量**还是配置文件？它要不要为 Anthropic / OpenAI 预留（见 map 的 `Not yet specified`）？
4. **statelessness 的后果**：两家都没有服务端会话原语 → **每次调用必须全量重放历史**。这对 trait 的签名有什么要求（比如不能假设"继续上一次"）？
5. **流式解析的两处陷阱**：
   - **usage chunk 形状不同**：Kimi 单独发一个 `choices: []` 的统计 chunk；DeepSeek **不发单独 chunk**，usage 搭在最后一个 content chunk 上，且 `stream_options` 与 `stream:false` 同用直接 **400**
   - **`tool_calls[].index`**：Kimi 的 OpenAPI **漏声明**它，但 guide 依赖它 → Rust 结构体**必须给它留位**，否则并行工具调用会串错
6. **终止判断——沿用旧图那条，本票必须正面回答。** 综述第 1 节点名了一个"一旦写错就很难察觉、表现为**工具调用被静默丢弃**"的 bug：**绝不能直接信任 provider 返回的 `stop_reason`**。判据必须自己算（"本轮没有工具调用 **且** 没有待处理的工具结果"）。定成一条明确、可测的规则，并说明谁持有"待处理工具结果"这个状态。
7. **轮数上限**：综述的取舍是"有上限是防呆（对自研建议保留），但应做成可配参数而非硬编码"（goose 默认 1000、Gemini 100、Codex 没有）。默认值定多少？注意子 agent 有自己的预算（票 14）。

答案定到接口级即可（trait 签名 + 关键类型 + 能力表结构），不要实现细节。

## Answer

**已定（2026-09-12，grilling 与用户逐轮确认）。事实来源：`research/02-provider-call-surface.md`（逐字段）与票 08 的 Rust 生态结论。**

### 0. 两条被事实逼出来、不是选择的前提

1. **`reasoning_content` 必须建模并回放。** DeepSeek：带 `tools` 的请求**必须**把历轮 `reasoning_content` 全部回传，否则 **400**；Kimi `kimi-k2.7-code` / `kimi-k3` 恒 `keep:"all"`，同理。→ 流式事件有 reasoning 增量，`Message::Assistant` 有 `reasoning_content: Option<String>`。**回放规则（回放谁的、跨说话者怎么办）交接给票 17。**
2. **流的结束以 `data: [DONE]` 为准**，不是 `finish_reason`（Kimi 文档明文要求）。`finish_reason` 只作诊断与截断判断。

### 1. 分发机制：`async-trait` + `Box<dyn Provider>`

`#[async_trait::async_trait]`，trait 约束 `Send + Sync`，经由 `Box<dyn Provider>` 持有。

理由：只有**一个真实实现**（唯一那个 OpenAI-compatible client）——Kimi 与 DeepSeek 是**同一 client 的两个 profile**，不是两个 provider 类型。dyn 在这里不是为了"支持多 provider"，而是**为了注入 e2e 的 mock provider**（map Notes：没有 `seed` → 不可复现 → e2e 必须用 mock）。一次调用是网络往返，boxed future 的分配是噪声；dyn 让 `agent`/`Session`/`discussion` 保持无类型参数。

**跨票一致性**：建议 `Tool`（票 03）采用同一选择，否则两条接口各走一套分发。该决定权在票 03。

### 2. trait 签名与关键类型（接口级）

```rust
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// 一次调用，永远流式：Kimi 长非流式易 504，DeepSeek 10 分钟未开始推理即断连。
    async fn send(&self, req: ChatRequest) -> Result<EventStream, ProviderError>;
}
pub type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, ProviderError>> + Send>>;

pub struct ChatRequest {
    pub messages: Vec<Message>,   // 已由 projection（provider::projection）产出
    pub tools: Vec<ToolSpec>,     // 线级 JSON Schema（票 01：该类型归 provider 一侧）
    pub tool_choice: ToolChoice,
    pub params: GenerationParams, // 中立参数，由 adapter 按能力表过滤（见第 4 节）
    pub cache_key: Option<String>,// 见第 8 节
}

pub enum Message {
    System { content: String, name: Option<String> },
    User { content: String, name: Option<String> },
    Assistant {
        content: Option<String>,
        reasoning_content: Option<String>,   // 前提 1
        tool_calls: Vec<ToolCall>,
        name: Option<String>,
    },
    Tool { tool_call_id: String, content: String },
}

pub enum StreamEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallStarted { index: u32, id: String, name: String },
    ToolCallCompleted { index: u32, id: String, name: String, arguments: String }, // 已拼好的 JSON 串
    Usage(Usage),                                // 两家形状归一
    Finished { finish_reason: FinishReason },    // 仅诊断，不作循环终止信号
}

pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cached_tokens: Option<u32>,     // Kimi usage.cached_tokens / DeepSeek prompt_cache_hit_tokens
    pub reasoning_tokens: Option<u32>,  // DeepSeek completion_tokens_details.reasoning_tokens
}

pub enum FinishReason {
    Stop, Length, ToolCalls, ContentFilter,
    InsufficientSystemResource, Aborted,   // DeepSeek 独有
    Other(String),
}
```

**关键取舍：tool_call 分片的拼接与 usage 的归一都归 adapter。** 上层只看 `ToolCallStarted` → `ToolCallCompleted`，永远看不到 fragment。adapter 内部必须处理：

- `function.arguments` 是**分片追加**（append, never overwrite），只有流结束才可解析；
- 多个 call 靠 `index` 区分，**而 `index` 在 Kimi 的 OpenAPI 里没有声明**（guide 依赖它）→ 结构体给它留位，缺失时兜底为 `0`；
- `id` / `name` 只在某个 `index` 的**第一片**出现；
- **usage 的两种形状**：Kimi 在 `[DONE]` 前**单独发一个 `choices: []` 的统计 chunk**；DeepSeek **不发单独 chunk**，统计搭在最后一个 content chunk 上，且 `stream_options` 与 `stream:false` 同用是 **400**；
- SSE 解析允许噪声：DeepSeek 等待期会持续发 `: keep-alive` 注释行。

**SSE 解析用哪个 crate 属实现细节，本票不决定**（票 08 的对比：`sse-stream` 活跃、`eventsource-stream` 已休眠）。

### 3. 多模型配置：两级 `[providers.*]` + `[models.*]`

硬事实：Kimi 的 key 有**平台域隔离**，跨域混用返回 **401**。两级形状让"一把 key ↔ 一个 base_url"成为结构事实，配不错域。

```toml
[providers.kimi]                       # 传输身份
base_url = "https://api.moonshot.ai/v1"
api_key  = "..."                       # 可缺省，按 config > env > 内置默认 的优先级回退

[models.kimi]                          # 模型档案（名字就是讨论名册引用的标识）
provider = "kimi"
model    = "kimi-k3"
# [models.kimi.capabilities] —— 可选的逐字段覆盖，见第 4 节
```

**trait 不认识 profile**：`OpenAiCompatClient::new(provider_cfg, model_profile, effective_caps)`——profile 是**构造参数**，不是 `send` 的参数。配置优先级沿用 map Notes：`config.toml` > 已导出 env > 内置默认；**不**自动加载项目 `.env`。

### 4. 能力表：`provider` 持内置表，`config` 可覆盖；**adapter 是执行点**

- **只建模现有两家，只按 `model id` 建表，不为 Anthropic / OpenAI 预留字段。** 原生 Anthropic 协议已是**证据型砍掉项**（Out of scope）；"再加一家"的真实成本是**加一条静态表项**（数据），不是改结构。
- **按 `model id` 而不是 vendor 建表**：`tool_choice: required` 只有 `kimi-k3` 有；`temperature` K3 固定 1.0 而 K2.6 非 thinking 是 0.6；窗口 K3 1M / K2.x 256K。vendor 级粒度会错。
- **前向兼容靠机制，不靠预留字段**：`#[non_exhaustive]` + **未登记的 model id 在启动时报错**（不猜、不给保守默认）。config 可以**逐字段覆盖**；要完全自定义一个未登记模型，必须提供完整能力集，否则报错。
- **只有 adapter 内部能归一化的东西一律不进表**（usage chunk 形状、`index` 缺口、错误码 429/402、`stream_options` 的 400）。表只留"上层必须知道才能不发错请求"的事实。

```rust
#[non_exhaustive]
pub struct Capabilities {
    pub model_id: &'static str,
    pub vendor: Vendor,
    pub context_window: u32,                    // 预算用（票 06）
    pub max_output_tokens: u32,
    pub output_limit_field: OutputLimitField,   // MaxCompletionTokens(Kimi) | MaxTokens(DeepSeek)
    pub temperature: SamplingSupport,           // Fixed(f32) | Adjustable{min,max} | IgnoredWhenThinking
    pub top_p: SamplingSupport,
    pub penalties: ParamDisposition,            // Rejected(Kimi) | SilentlyIgnored(DeepSeek)
    pub n: NSampling,                           // FixedOne —— 两家都不可用
    pub seed: bool,                             // false —— 两家都没有
    pub tool_choice_required: bool,             // 仅 kimi-k3
    pub response_format: ResponseFormats,       // TEXT | JSON_OBJECT | JSON_SCHEMA
    pub reasoning: ReasoningSupport,            // AlwaysOn | Toggleable | EffortLevels{..}
    pub reasoning_replay: ReasoningReplay,      // Always | RequiredWithTools | Never
    pub parallel_tool_calls: ParallelSupport,   // AlwaysOn(DeepSeek 不可关) | Configurable | Unsupported
    pub prompt_cache_key: bool,                 // Kimi true / DeepSeek false
    pub max_request_body_bytes: u64,            // Kimi 100 MB / DeepSeek 48 MiB
}
```

**执行点**：上层给中立 `GenerationParams`，**adapter 按表过滤**；丢弃**用户显式设置**的参数时**告警**（Kimi 传 `temperature` 是**报错**、DeepSeek 传 penalty 是**静默无效**——只有懂 vendor 的这一层能正确处理）。配置里写死的非法参数在**启动时校验报错**，不拖到第一次调用。

**交接**：`parallel_tool_calls` 在 DeepSeek 恒开、Kimi 也允许多个 → **写串行化只能由 harness 做**，归票 03。

### 5. 轮数上限：默认 100 `Turn`，可配

计数单位是 `CONTEXT.md` 的 `Turn`（一次 provider 调用 + 其工具执行）。默认 **100**（Gemini 的取值：够真实重构，又能兜住跑飞），配置可改。它是**防呆护栏，不是工作限额**——真正的闸门是成本（票 18）。计数器住在 `Session`（票 01）；Executor 的**更小**默认值留给票 14（GOOSE 先例 25）。

### 6. 终止规则（票面问题 6 的正面回答）

- **循环继续的充要条件**：投影出的**最后一条 assistant 消息含 ≥1 个 `tool_call`**。否则 Turn 结束。
- **硬不变量**：只要日志里存在**没有对应结果的 `tool_call`**，就**绝不允许再调 provider**——必须先为它产出结果（成功或显式失败）。这就是"待处理的工具结果"。
- **`FinishReason` / `stop_reason` 永不作为循环信号**（综述点名的 bug 是"工具调用被静默丢弃"）；流的结束只认 `[DONE]`。`Length` 仅用于判断截断。
- **归属**：pending **不是隐藏状态**，而是对 `EventLog` 的一次查询（`pending = 最后一条 assistant.tool_calls − 其后的 tool 消息`）。理由：map 的不变量是"流是唯一真相源、任何 agent 的 messages 都能从流重算"；藏在 Turn 局部变量里会让恢复会话无法重算。
- **交接**：恢复会话时遇到"悬空 tool_call"如何收尾（补合成结果 / 丢弃 / 重执行）→ **票 07**。

### 7. 错误分类与重试归属

```rust
pub enum ProviderError {
    Auth { detail: String },                       // 401/403：key 无效、跨域混用
    QuotaExhausted { detail: String },             // Kimi 429-quota / DeepSeek 402 —— 重试无意义
    RateLimited { retry_after: Option<Duration> }, // 429-rate / 503 / engine_overloaded —— 可退避
    InvalidRequest { detail: String },             // 400/422：超窗、参数不支持
    Transport { detail: String },                  // 连接、超时、body 超限
    Protocol { detail: String },                   // SSE 解析失败、缺 [DONE]、usage 缺失
}
```

- **两家欠费错误码不同**（Kimi 429 `exceeded_current_quota_error` / DeepSeek 402），所以 `QuotaExhausted` 必须与 `RateLimited` 分开——适配器不能统一按 429 处理。
- **adapter 拥有传输级重试**（chat completion 请求天然幂等）：只重试 `RateLimited` 与 `Transport`，有界次数、尊重 `Retry-After`；`QuotaExhausted` / `Auth` / `InvalidRequest` **永不重试**。终态错误抛回 agent Turn 决定这个 Turn 失败。
- **不决定的**：跨切面的失败呈现（429 怎么显示、讨论某轮失败是否重开、执行者失败如何回传）仍在 map 的雾里。（**该雾已清空**——由票 05/14/16 答掉；map 现在没有 `Not yet specified`）

### 8. 无状态的后果与唯一的会话级提示

- **请求类型里不存在 `store` / `previous_response_id` / `conversation` 任何字段**，trait 也**没有"继续上一次"的概念**：每次 `send` 都是独立、自足的全量调用，历史由 `projection` 从 `EventLog` 全量重放。两家文档都明说 stateless。
- **`prompt_cache_key`（Kimi 专有）**：`ChatRequest.cache_key` 的值 = **会话标识**（票 07 定它怎么生成与持久化；**`--continue` 后必须不变**）。官方对 coding agent 的建议正是"session id or task id"，且 Kimi Code Plan 下该字段必填。能力表用 `prompt_cache_key: bool` 控制，DeepSeek 侧由 adapter 丢弃。
- **DeepSeek 的 `user_id`（KVCache 隔离）v1 不发**：自用单租户无隔离需求。

### 本票明确不决定的（交接汇总）

- `Tool` 的分发机制与 trait 形状 → **票 03**；`parallel_tool_calls` 恒开导致的写串行化 → **票 03**。
- 投影的纯函数规则与 `reasoning_content` 的回放策略（含跨说话者）→ **票 17**。
- 悬空 `tool_call` 在会话恢复时的收尾 → **票 07**。
- 预算公式怎么用 `context_window` / `max_output_tokens` → **票 06**；成本闸门 → **票 18**。
- 跨切面的失败语义与呈现 → 仍在 map 的 `Not yet specified`。（**该雾已清空**——由票 05/14/16 答掉；map 现在没有 `Not yet specified`）
- SSE 解析 crate 选型、重试的具体次数与退避曲线 → 实现细节，`/to-spec` 之后。

**票 07 交接来的答案（2026-09-13）**：你在第 8 节与「明确不决定」里交给票 07 的两件事都关了。

- **`prompt_cache_key` 的会话标识**：会话 id 形状是 `<UTC 时间戳>-<短随机后缀>`（如 `20260913T203015-a7f3`），**持久化在会话目录名与 `SessionStarted { session_id }` 两处**；`--continue` / `--resume` 是**向同一个 `log.jsonl` 追加**，**id 绝不变**。所以你那条"`--continue` 后必须不变"的要求有保障了——**恢复不会丢前缀缓存**。
- **悬空 `tool_call` 在会话恢复时的收尾**：**补一条合成的失败结果事件** `ToolCallCompleted { tool_call_id, ok: false, error: "会话中断于此，结果未知" }`——不丢弃那条 assistant（破只追加）、不重执行（写工具可能已经改过文件）。**"每个 `tool_call` 恰好一条结果"因此跨恢复也成立**，你那条"存在无结果的 `tool_call` 时绝不允许再调 provider"的判据在恢复后能正常放行。
- 会话落盘形状（供你参考，不影响 provider 层）：`~/.local/share/fs-agent/sessions/<cwd-slug>/<session-id>/log.jsonl` + `outputs/`。

**票 18 交接来的一条要求（2026-09-13）**：**`Usage` 的归一里要带上 cache 命中 / 未命中**，这不是可选的展示字段。

- **两家的字段形状不同但语义清楚**（`research/02` §5）：Kimi 是 `usage.cached_tokens`；DeepSeek 是 `usage.prompt_cache_hit_tokens` / `usage.prompt_cache_miss_tokens`（且 `prompt_tokens = hit + miss`），另有 `prompt_tokens_details.cached_tokens`（= hit）。**归一成 `cached` / `miss` 两个数**即可。（`miss = prompt_tokens − cached_tokens`，**不必新增字段**——`Usage` 里的 `cached_tokens` 已经有了）
- **为什么它是必须的**：综述说这是"判断缓存有没有生效的**唯一办法**"（票 18 第 6 节）——你的 `Usage` 是上层唯一能拿到这个量的地方，别在归一的时候把它丢掉。
- ⚠️ **不要照抄 Anthropic 的 `cache_creation` / `cache_read` 四字段**：那套字段在这两家**不存在**，照抄会得到一个永远填不满的结构。
- 一条与你的能力表相关的既有事实（票 18 提醒）：**Kimi 的 `reasoning_effort` 切档会破坏 prefix-cache 命中**（官方明说"换档应在 session 开始前定好"）——所以它属于"会话开始前定死"的那一类参数，你的能力表应该把它标出来。
