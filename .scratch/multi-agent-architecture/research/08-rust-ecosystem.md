# Rust 生态调研简报（票 08：crate 选择与当前惯用法）

> **调研日期**：2026-09-12（UTC）。本简报中所有「当前 / 最新」均以该日期为准（票面写作时的「2025 末 / 2026 初」已过时，实测日期是 2026-09-12）。
>
> **方法（一手来源）**：只用官方文档（`doc.rust-lang.org`、`blog.rust-lang.org`、WHATWG 规范、Tokio 官方文档站 `tokio.rs`）、`docs.rs` 生成的 API 文档与源码视图、crate 官方仓库的源码 / README / `Cargo.toml`、`crates.io` API 元数据（版本号、发布日期、依赖边）。第三方博客与教程不作为来源。
>
> **范围纪律**：只报告事实与来源；**不推荐方案、不选赢家**。仅当一手来源自身声明了限制、保证或稳定性时，才转述该声明。无法核实处显式标 **UNVERIFIED**。
>
> **依赖计数方法**：`cargo tree -e normal -q --prefix none`（默认特性 + 文中注明的手动特性），去重后统计包数（含传递依赖，不含 root、不含 dev/build 边），2026-09-12 实测。这是**测量值**，会随特性选择与时间变化；`crates.io` 的「直接依赖边」计数另列。

---

## 0. 与「目标 Rust 版本」有关的环境事实

- 本仓库**没有** `Cargo.toml`，也**没有** `rust-toolchain` / `rust-toolchain.toml`（2026-09-12 在工作区实测；仓库根只有 `AGENTS.md`、`docs/`、`.scratch/`、`LICENSE`、`README.md`、`.gitignore`）。因此仓库内**没有固定任何 Rust 版本或 edition**。
- 本机工具链：`rustc 1.94.0 (4a4ef493e 2026-03-02)`、`cargo 1.94.0 (85eff7c80 2026-01-15)`（本地 `rustc --version` / `cargo --version`）。
- Rust 当前 stable：**1.98.1（2026-09-03）** —— <https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/>；同时 `https://static.rust-lang.org/dist/channel-rust-stable.toml` 记录 `[pkg.rust] version = "1.98.1 (48a229cea 2026-09-01)"`、channel `date = "2026-09-03"`。
- 事实层面：本机 1.94.0 落后于 stable 1.98.1；第 2 题的结论对 1.75 及以上均成立。

---

## 1. 流式 HTTP + SSE 客户端

### 1.1 `reqwest` 0.13.5（手写 SSE 解析的底座）

| 项 | 事实 | 来源 |
| --- | --- | --- |
| 最新版本 / 日期 | **0.13.5 / 2026-09-08**（`max_stable_version` = `newest_version` = 0.13.5） | <https://crates.io/api/v1/crates/reqwest> |
| 仓库活跃度 | `seanmonstar/reqwest`，HEAD 最后提交 **2026-09-08** | <https://github.com/seanmonstar/reqwest/commits/HEAD.atom> |
| 直接依赖边 | 47 条 normal（其中 **21 条非可选**、26 条可选），15 条 dev | <https://crates.io/api/v1/crates/reqwest/0.13.5/dependencies> |
| 传递依赖闭包 | **103 个包**（默认特性 + `json` + `stream`，实测；含 `aws-lc-rs`/`aws-lc-sys` 等，因 0.13 的 `default-tls = ["rustls"]`） | 实测；特性定义见 <https://crates.io/api/v1/crates/reqwest/0.13.5> |

**流式 body 的两条 API（0.13.5 tag 源码）**：

- `pub async fn chunk(&mut self) -> crate::Result<Option<Bytes>>` —— **不受 feature 门控**；
- `#[cfg(feature = "stream")] pub fn bytes_stream(self) -> impl Stream<Item = Result<Bytes>>` —— 文档注明 *"This requires the optional `stream` feature to be enabled."*（docs.rs 页面同样标注 "Available on crate feature stream only."）
  - 源码：<https://raw.githubusercontent.com/seanmonstar/reqwest/v0.13.5/src/async_impl/response.rs>
  - docs.rs：<https://docs.rs/reqwest/latest/reqwest/struct.Response.html#method.bytes_stream>
- `stream` 特性 = `["tokio/fs", "dep:futures-util", "dep:tokio-util", "dep:wasm-streams"]` —— <https://docs.rs/crate/reqwest/0.13.5/features>、<https://crates.io/api/v1/crates/reqwest/0.13.5>

**事实**：`reqwest` 是纯 HTTP 传输层，**不含 SSE 解析、不含任何模型/provider 协议**。SSE 的帧解析若要手写，规范本体是 WHATWG HTML 的 Server-sent events 章节（`text/event-stream`；`data` / `event` / `id` / `retry` 四类字段、以行首 `:` 注释、空行 dispatch、字段名逐字面比较不做大小写折叠）：<https://html.spec.whatwg.org/multipage/server-sent-events.html>。

**关于「OpenAI 流式 tool_call 增量」的一手证据边界**：OpenAI 官方 API 参考页（<https://platform.openai.com/docs/api-reference/chat/streaming>）是 JS 渲染，本次**未能抓到正文**，故其原文措辞 **UNVERIFIED**。字段级证据来自兼容实现的源码类型（见 1.3 / 1.4）：流式响应是若干 JSON chunk，`choices[].delta.tool_calls[].function.arguments` 是**参数片段字符串**，需调用方按 `index` 累积；流以 `[DONE]` 结束。

### 1.2 现成 SSE 解析 crate（全部与 provider 协议无关）

| crate | 最新版本 / 发布日期 | 最后提交 | 依赖（normal 声明） | 定位 |
| --- | --- | --- | --- | --- |
| `eventsource-stream` | **0.2.3 / 2022-02-17** | 2022-02-17（事实上的休眠） | `futures-core`、`nom 7.1`、`pin-project-lite`（`reqwest ^0.11` 仅 dev-dependency） | 只做 parser：文档称 *"A basic building block for building an Eventsource from a Stream of bytes array like objects."* 提供 `Eventsource` trait、`EventStream`、`Event`；README 示例 `.bytes_stream().eventsource()` |
| `reqwest-eventsource` | **0.6.0 / 2024-03-29** | 2024-03-29（休眠） | 8 条 normal；**依赖 `reqwest ^0.12.0`**（而 reqwest 当前是 0.13.5） | reqwest 封装 + 自动重连：*"This crate uses eventsource_stream to wrap the underlying Bytes stream, and retries failed requests."* |
| `sse-stream` | **0.2.6 / 2026-09-03** | 2026-09-03（活跃） | `bytes`、`futures-util`、`http-body 1`、`http-body-util`、`pin-project-lite` | *"A SSE decoder/encoder for Http body"*，`SseStream::new(body)` 逐块产出 `Sse`；对 `http-body` 泛型，不绑定 reqwest |
| `eventsource-client` | **0.18.0 / 2026-08-10** | 2026-08-24（活跃，LaunchDarkly） | `base64`、`bytes`、`futures`、`http`、`launchdarkly-sdk-transport ^0.1`、`log`、`pin-project`、`rand`、`tokio` | *"This library provides SSE protocol support but requires you to bring your own HTTP transport."* |

来源：<https://crates.io/api/v1/crates/eventsource-stream>、<https://docs.rs/eventsource-stream/latest/eventsource_stream/>、<https://raw.githubusercontent.com/jpopesculian/eventsource-stream/master/Cargo.toml>、<https://crates.io/api/v1/crates/reqwest-eventsource>、<https://docs.rs/reqwest-eventsource/0.6.0/reqwest_eventsource/>、<https://crates.io/api/v1/crates/sse-stream>、<https://docs.rs/sse-stream/latest/sse_stream/>、<https://crates.io/api/v1/crates/eventsource-client>、<https://docs.rs/eventsource-client/0.18.0/eventsource_client/>、<https://raw.githubusercontent.com/launchdarkly/rust-eventsource-client/main/README.md>。

其他检索到的同类 crate（版本 / 发布日期，均来自 `crates.io/api/v1/crates?q=...` 搜索）：`sse-codec` 0.3.3（2026-02-12）、`sse-core` 0.2.3（2026-07-31）、`reqwest-sse` 0.2.0（2026-05-08）、`aha-reqwest-eventsource` 0.1.0（2026-01-03，自述为 `jpopesculian/reqwest-eventsource` 的 fork）、`eventsource` 0.5.0（2020-04-22，休眠）、`tokio_sse_codec` 0.0.2（2023）。

**事实**：上述 crate 都只处理 SSE 协议层，**没有任何一个强绑某家 provider 协议**；`tokio-stream` 0.1.19（2026-07-22）只是 `Stream` 工具集，其 all-items 列表中**没有** SSE / event 相关项（<https://docs.rs/tokio-stream/0.1.19/tokio_stream/all.html>）。

### 1.3 `async-openai` 0.42.0

| 项 | 事实 | 来源 |
| --- | --- | --- |
| 最新版本 / 日期 | **0.42.0 / 2026-09-09** | <https://crates.io/api/v1/crates/async-openai> |
| 仓库活跃度 | `64bit/async-openai`，HEAD 最后提交 2026-09-09 | <https://github.com/64bit/async-openai/commits/HEAD.atom> |
| 直接依赖边 | 24 条 normal（3 条非可选：`serde`、`serde_json`、`getrandom`；其余按 feature 可选），3 条 dev | <https://crates.io/api/v1/crates/async-openai/0.42.0/dependencies> |
| 传递依赖闭包 | **95 个包**（默认特性，实测） | 实测 |
| MSRV | workspace `rust-version = "1.75"` | <https://raw.githubusercontent.com/64bit/async-openai/main/Cargo.toml> |
| 内部 SSE | 自身依赖 `eventsource-stream ^0.2`、`reqwest ^0.13` | <https://crates.io/api/v1/crates/async-openai/0.42.0/dependencies> |

**流式 tool_call 增量**（源码级证据）：

- `Chat::create_stream` 返回 `ChatCompletionResponseStream = StreamResponse<CreateChatCompletionStreamResponse>`，文档说明这是 *"a parsed SSE stream until a `[DONE]` is received from server"*：<https://raw.githubusercontent.com/64bit/async-openai/main/async-openai/src/chat.rs>（`create_stream` 定义在同一文件，文档注释见 61–75 行）。
- `ChatCompletionStreamResponseDelta { content, function_call(已废弃), tool_calls: Option<Vec<ChatCompletionMessageToolCallChunk>>, role, refusal }`：<https://raw.githubusercontent.com/64bit/async-openai/main/async-openai/src/types/chat/chat_.rs>（1161–1170 行）
- `ChatCompletionMessageToolCallChunk { index: u32, id, r#type, function: Option<FunctionCallStream> }`（1144 行）；`FunctionCallStream { name: Option<String>, arguments: Option<String> }`（1131–1141 行）——即**参数是逐块片段字符串**。docs.rs：<https://docs.rs/async-openai/0.42.0/async_openai/types/chat/struct.FunctionCallStream.html>、<https://docs.rs/async-openai/0.42.0/async_openai/types/chat/struct.ChatCompletionStreamResponseDelta.html>
- 0.42.0 中**不存在**名为 `ChatCompletionStream` 的类型（只有 `ChatCompletionStreamOptions`、`ChatCompletionStreamResponseDelta`、`CreateChatCompletionStreamResponse`）。

**provider 绑定（一手声明）**：README 称它是 *"unofficial Rust library for OpenAI"*，且 *"Even though the scope of the crate is official OpenAI APIs, it is very configurable to work with compatible providers."*，列出 "OpenAI Compatible Providers" / "Bring your own custom types" / "Microsoft Azure OpenAI Service"，**未声明 Anthropic 支持**：<https://raw.githubusercontent.com/64bit/async-openai/main/async-openai/README.md>。`base_url` 可配：`OpenAIConfig::api_base` 默认读环境变量 `OPENAI_BASE_URL`，否则 `https://api.openai.com/v1`；提供 `with_api_base()`、`with_api_key()`、`OPENAI_API_KEY`：<https://raw.githubusercontent.com/64bit/async-openai/main/async-openai/src/config.rs>（57–65、137–162 行）、<https://docs.rs/async-openai/0.42.0/async_openai/config/struct.OpenAIConfig.html>。

**命名空间事实**：仓库已是 Cargo workspace（`members = ["async-openai","async-openai-*","examples/*"]`），根 `README.md` 是内容为 `async-openai/README.md` 的指针文件，release tag 形如 `async-openai-v0.42.0`。crates.io 上并存多个第三方 fork：`async-openai-alt` 0.26.2（2025-02-20）、`async-openai-compat` 0.30.6（2026-01-12）、`async-openai-thinking` 0.41.5（2026-08-08，自述 *"Fork of async-openai 0.41.3"*）、`dynamo-async-openai` 1.0.2、`async-openai-wasm` 0.31.2。**「该 crate 曾更换维护者」这一说法 UNVERIFIED**：crates.io 记录的所有者始终是 `64bit`，未找到任何一手声明转移的文字。

### 1.4 `genai`（jeremychone/rust-genai）

| 项 | 事实 | 来源 |
| --- | --- | --- |
| 稳定版 / 日期 | **0.6.5 / 2026-06-06** | <https://crates.io/api/v1/crates/genai> |
| 最新（预发布） | **0.7.0-beta.23 / 2026-09-08** | 同上 |
| 仓库活跃度 | HEAD 最后提交 2026-09-11 | <https://github.com/jeremychone/rust-genai/commits/HEAD.atom> |
| 直接依赖边 | 0.6.5：21 条 normal（18 非可选 + 3 可选 `aws-config`/`aws-credential-types`/`aws-sigv4`），9 条 dev；0.7.0-beta.23：22 条 | <https://crates.io/api/v1/crates/genai/0.6.5/dependencies> |
| 传递依赖闭包 | **148 个包**（0.6.5，默认特性，实测） | 实测 |
| 内部 SSE | 依赖 `eventsource-stream ^0.2`、`reqwest ^0.13` | 依赖 API 同上 |

**流式 tool_call 增量**（源码级证据）：

- 事件枚举 `chat::ChatStreamEvent { Start, Chunk(StreamChunk), ReasoningChunk, ThoughtSignatureChunk, ToolCallChunk(ToolChunk), End(StreamEnd), Heartbeat }`：<https://raw.githubusercontent.com/jeremychone/rust-genai/main/src/chat/chat_stream.rs>（156–174 行）；`pub struct ToolChunk { pub tool_call: ToolCall }`（190 行）。
- OpenAI adapter 的 streamer 在收到含 `delta.tool_calls` 的 SSE 消息时逐条返回 `InterStreamEvent::ToolCallChunk(tool_call)`（即**逐 chunk 暴露**）：<https://raw.githubusercontent.com/jeremychone/rust-genai/main/src/adapter/adapters/openai/streamer.rs>（317–346 行）。
- 同文件的事实细节：adapter 内部用 `capture_tool_call` 按 `index` **合并参数片段**（100–131 行），并在 `End` 时汇总、尝试把累积字符串 `serde_json::from_str` 解析为 `fn_arguments`（162–206 行）；非 finish 分支对 `delta_tool_calls` 取 **`.first()`**（321–322 行），即一条 SSE 消息里数组的首个 tool call 对象；同一条消息同时含 `finish_reason` 与 `tool_calls` 时另有专门分支（240–312 行）。
- `ToolCall { call_id: String, fn_name: String, fn_arguments: Value, ... }`：<https://docs.rs/genai/0.7.0-beta.23/genai/chat/struct.ToolCall.html>。

**provider 绑定（一手声明）**：README 称其为 "native-protocol" 多 provider 库，列出 28 个内置 provider（`openai`、`openai_resp`、`anthropic`、`gemini`、`omlx`、`ollama`、`vertex`、`bedrock_*`、`groq`、`deepseek`、`xai`、`cohere`、`open_router` 等），并给出 OpenAI-compatible 自定义端点方式：*"Custom OpenAI-compatible endpoints can be accessed with the built-in `genai_n` adapter. Set `GENAI_{n}_ENDPOINT` and optionally `GENAI_{n}_API_KEY`, then use the `genai_{n}::model_name` namespace"*；另支持 `ServiceTargetResolver` 自定义端点与鉴权：<https://raw.githubusercontent.com/jeremychone/rust-genai/main/README.md>。

**一手声明的限制**：0.7.0 目前是 beta，README 列出明确的 breaking changes 清单与 *"v0.7.0 release target: first half of Sept 2026"*；另有 *"At this point, Ollama does not emit input/output tokens when streaming due to a limitation in the Ollama OpenAI compatibility layer."*（同 README）。

### 1.5 `rig`（0.42.0：`rig` / `rig-core` / `rig-agent`）

| 项 | 事实 | 来源 |
| --- | --- | --- |
| 版本 / 日期 | `rig` / `rig-core` / `rig-agent` 均 **0.42.0 / 2026-08-17** | <https://crates.io/api/v1/crates/rig-core>、<https://crates.io/api/v1/crates/rig-agent> |
| 仓库活跃度 | HEAD 最后提交 2026-09-07 | <https://github.com/0xPlaygrounds/rig/commits/HEAD.atom> |
| 直接依赖边（rig-core） | 34 条 normal（26 非可选 + 8 可选，含 `epub`/`lopdf`×2/`quick-xml`/`rayon`/`reqwest-middleware`/`rig-derive`/`tokio-tungstenite`），18 条 dev | <https://crates.io/api/v1/crates/rig-core/0.42.0/dependencies> |
| 传递依赖闭包（rig-core） | **149 个包**（默认特性，实测） | 实测 |
| 内部 SSE | 依赖 `eventsource-stream ^0.2.3`、`reqwest ^0.13` | 依赖 API 同上 |

**流式 tool_call 增量**：`streaming::StreamedAssistantContent { Text, ToolCall { tool_call, internal_call_id }, ToolCallDelta { internal_call_id, content: ToolCallDeltaContent }, Reasoning, ReasoningDelta, Final, Unknown }`；`ToolCallDeltaContent { Name(String), Delta(String) }`，文档原文 *"The content of a tool call delta - either the tool name or argument data"*：<https://docs.rs/rig-core/0.42.0/rig_core/streaming/enum.ToolCallDeltaContent.html>、<https://docs.rs/rig-core/0.42.0/rig_core/streaming/enum.StreamedAssistantContent.html>。

**定位**：README 称 *"Rig is a Rust library for building scalable, modular, and ergonomic LLM-powered applications"*，包含 agentic workflows、默认开启的 agent runtime、20+ provider、10+ vector store；运行时切分为 `rig-core`（provider 中立契约）与 `rig-agent`（builder、streaming traits、hooks、`AgentRun` 状态机）：<https://raw.githubusercontent.com/0xPlaygrounds/rig/main/README.md>。docs.rs 的 `rig_core::providers` 索引列出 anthropic / azure / cohere / deepseek / doubleword / gemini / groq / huggingface / hyperbolic / mira / moonshot / ollama / openai / openrouter / perplexity / together / voyageai / xai。OpenAI provider 的配置含 `base_url = OPENAI_API_BASE_URL` 与 `base_url_env_first = "OPENAI_BASE_URL"`：<https://docs.rs/rig-core/0.42.0/src/rig_core/providers/openai/client.rs.html>。

**一手声明的稳定性限制**：README 顶部警告 *"Here be dragons! … future updates **will** contain **breaking changes**."*（同 README 链接）。

### 1.6 依赖重量对照（2026-09-12 实测）

| 依赖目标（版本 + 手动特性） | 传递闭包包数 | 直接 normal 边（非可选） |
| --- | --- | --- |
| `reqwest 0.13.5`（`json`+`stream`） | **103** | 47（21） |
| `eventsource-stream 0.2.3` | **6** | 3（3） |
| `sse-stream 0.2.6` | **16** | 5（4） |
| `reqwest-eventsource 0.6.0` | **82** | 8（8） |
| `async-openai 0.42.0` | **95** | 24（3） |
| `genai 0.6.5` | **148** | 21（18） |
| `rig-core 0.42.0` | **149** | 34（26） |

方法：`cargo tree -e normal -q --prefix none`，去重、排除 root 与 dev/build 边。对照参考：`serde_json 1.0.151` = 5、`anyhow 1.0.104` = 1、`thiserror 2.0.20` = 8、`async-trait 0.1.92` = 7。数字随特性与时间变化，仅作量级参考。

### 1.7 协议绑定小结（事实表，不含推荐）

| 依赖 | 是否强绑某家 provider 协议 | 一手依据 |
| --- | --- | --- |
| `reqwest` | 否（纯 HTTP 传输） | 源码只有 HTTP 语义；无模型/provider 类型 |
| `eventsource-stream` / `sse-stream` | 否（SSE 协议层，对字节流/`http-body` 泛型） | 见 1.2 引用 |
| `reqwest-eventsource` | 否，但**固定 `reqwest ^0.12`**（与 reqwest 0.13.x 不在同一版本线） | <https://crates.io/api/v1/crates/reqwest-eventsource/0.6.0/dependencies> |
| `eventsource-client` | 否，但依赖 LaunchDarkly 的 `launchdarkly-sdk-transport`，要求自备传输 | 同上 1.2 引用 |
| `async-openai` | **是 OpenAI 协议**（README 自述 scope 为 official OpenAI APIs），`base_url`/`api_key`/`model` 可配 | README + `OpenAIConfig` |
| `genai` | 否，多原生协议 + OpenAI-compatible 自定义端点（`genai_n::`） | README |
| `rig` | 否，多 provider 框架；OpenAI provider 支持 `OPENAI_BASE_URL` | docs.rs providers + 源码 |

---

## 2. `async fn` in trait 的当前状态

### 2.1 稳定性

- `async fn` in trait（AFIT）与 return-position `impl Trait` in trait（RPITIT）在 **Rust 1.75.0（2023-12-28）** 稳定。1.75 发布公告原文：*"Rust 1.75 supports use of `async fn` and `-> impl Trait` in traits. However, this initial release comes with some limitations…"* —— <https://blog.rust-lang.org/2023/12/28/Rust-1.75.0/>
- 专项公告（2023-12-21）：<https://blog.rust-lang.org/2023/12/21/async-fn-rpit-in-traits/>。

### 2.2 object safety / dyn 兼容性（决定 `Provider` / `Tool` 能否 `dyn`）

**事实：带 `async fn` 方法的 trait 不是 dyn-compatible，`dyn Trait` 会被拒绝。** 三个一手来源：

1. Rust Reference「Dyn compatibility」章节，dispatchable 关联函数必须 *"Not have an opaque return type; that is, **Not be an `async fn` (which has a hidden `Future` type)**. Not have a return position `impl Trait` type"*；同章还列出 *"The `AsyncFn`, `AsyncFnMut`, and `AsyncFnOnce` traits are not dyn-compatible."* 并注明该概念旧称 object safety：<https://doc.rust-lang.org/reference/items/traits.html#dyn-compatibility>
2. 1.75 专项公告原文：*"Traits that use `-> impl Trait` and `async fn` are not object-safe, which means they lack support for dynamic dispatch. We plan to provide utilities that enable dynamic dispatch in an upcoming version of the `trait-variant` crate."* —— <https://blog.rust-lang.org/2023/12/21/async-fn-rpit-in-traits/>
3. `async-trait` 文档复述：*"The stabilization of async functions in traits in Rust 1.75 did not include support for using traits containing async functions as `dyn Trait`."*，并给出 rustc E0038 报错 *"the trait `Trait` is not dyn compatible … because method `f` is `async`"* —— <https://docs.rs/async-trait/latest/async_trait/>

RPITIT 的附带限制（同公告）：对**公开** trait，`-> impl Trait` 仍被建议避免，因为调用方无法给返回类型加 bound；`async fn` 在公开 trait 中会给出 *"auto trait bounds cannot be specified"* 的警告（即 `Send` 边界问题）。

### 2.3 生态里实际使用的替代做法（按一手来源）

| 做法 | 一手事实 | 来源 |
| --- | --- | --- |
| `async-trait`（proc-macro） | **0.1.92 / 2026-08-08**；文档：*"Async fns get transformed into methods that return `Pin<Box<dyn Future + Send + 'async_trait>>` and delegate to an async block."*；`#[async_trait(?Send)]` 可去掉 Send 约束；crates.io 记录 `rust_version = "1.71"` | <https://crates.io/api/v1/crates/async-trait>、<https://docs.rs/async-trait/latest/async_trait/> |
| `trait-variant`（泛型/单态化） | **0.1.3 / 2026-07-22**，属 `rust-lang` 组织；1.75 公告推荐的 `#[trait_variant::make(HttpService: Send)]` 生成带 Send 边界的 trait 变体；当前 0.1.3 文档只文档化 `make`，**没有** dyn 相关工具（公告中「计划在 trait-variant 提供 dyn 工具」尚未落地） | <https://crates.io/api/v1/crates/trait-variant>、<https://docs.rs/trait-variant/latest/trait_variant/> |
| `dynosaur`（生成 dyn 包装） | **0.3.1 / 2026-07-03**；文档：*"Currently Rust does not support dynamic dispatch on traits that use `async fn` or methods returning `impl Trait`. Dynosaur is a proc macro that allows dynamic dispatch on these traits but uses static dispatch otherwise. It requires at least Rust 1.75."* 用法是写 `DynTrait` 取代 `dyn Trait` | <https://crates.io/api/v1/crates/dynosaur>、<https://docs.rs/dynosaur/latest/dynosaur/> |
| 手工脱糖 | 写 `-> Pin<Box<dyn Future<Output = T> + Send + '_>>`；被 2026 项目目标列为现有 workaround | 见下 |
| 枚举分发 | **UNVERIFIED**：在官方 async book（<https://rust-lang.github.io/async-book/>）与 Rust 官方博客中未找到把「enum dispatch」作为 AFIT 替代的官方陈述；async book 的 "Async traits" 章节仍是 TODO 占位（且全书带 "undergoing a rewrite" 横幅） | — |

### 2.4 未来进展（一手）

- Rust 官方 `rust-project-goals` 仓库 2026 年度目标 **"Native async fn dynamic dispatch in traits"**：Status = **Accepted**，Timespan = **2026-2027**；原文 *"Async fn in traits (AFIT) has been stable since Rust 1.75, but traits with async methods are not dyn-compatible."* 计划分 Phase 1（Reforming dyn traits，4 个月）与 Phase 2（Async through dynamic dispatch，8 个月），早期以 nightly-only 辅助宏 `std::preview::dyn_box!(client.fetch(url)).await` 形式暴露：<https://raw.githubusercontent.com/rust-lang/rust-project-goals/main/src/2026/afidt-box.md>
- 跟踪 issue 标题为 *"Tracking Issue for `async_fn_in_dyn_trait`"*（rust-lang/rust **#133119**）：<https://github.com/rust-lang/rust/issues/133119>。**其 open/closed 状态 UNVERIFIED**（GitHub API 403 限流；HTML 只返回导航；.atom 返回 406）。可核实的间接证据是该特性尚未稳定：rustc 的 feature-gate 测试仍在，且上述 2026 目标仍把它列为待办。
- 事实层面：截至 2026-09-12 的 stable（1.98.1）**没有**原生 dyn async trait。

---

## 3. 「工具调用前后各一个挂载点」在真实项目里的表达方式

票面问的是 typed enum / trait object / channel 三选。**实测结果：找到的真实项目全部落在「trait（或 trait object）+ typed enum 返回值」这一族；在 Rust agent 项目中未找到以 `tokio::sync::mpsc` 作为 hook 挂载机制的例子**（channel 只在 Tokio 官方文档里作为 actor/共享状态方案出现，见第 5 题）。

### 3.1 `rig-agent` 0.42.0 —— RPITIT trait + typed action enum（**当前发布版**）

`rig-agent` 0.42.0（2026-08-17）的 `agent::hook::AgentHook`：

```rust
pub trait AgentHook: WasmCompatSend + WasmCompatSync {
    fn on_tool_call(&self, _ctx: &HookContext, _event: ToolCall<'_>)
        -> impl Future<Output = ToolCallAction> + WasmCompatSend { ... }
    fn on_tool_result(&self, _ctx: &HookContext, _event: ToolResultEvent<'_>)
        -> impl Future<Output = ToolResultAction> + WasmCompatSend { ... }
    // 另有 on_tool_call_delta / on_model_select / on_completion_call / on_text_delta
    // / on_reasoning_delta / on_invalid_tool_call / on_stream_response_finish 等
}
```

- 文档对 `on_tool_call` 的说明：*"Runs before a valid tool call is executed. The hook may rewrite the current arguments, skip execution, or stop the run."*；对 `on_tool_result`：*"Runs after a tool call resolves and before its presentation is sent to the model."*
- source：<https://docs.rs/rig-agent/0.42.0/rig_agent/agent/hook/trait.AgentHook.html>；同 crate 还有 `HookContext`、`HookStack`、`StepEventKind`。
- 官方文档站 `rig.rs` 的 hooks 页描述的是 main 分支的 `AgentHook` / `StepEvent` / `Flow`，并明确写：*"This page documents the `AgentHook`, `StepEvent`, and `Flow` APIs on Rig's main branch. If docs.rs latest does not yet show these symbols, use the linked GitHub source for exact signatures."* 还声明 runner 是 *"fail-closed"*（hook 返回该事件无法执行的动作时结束 run 并给诊断，而不是静默忽略）：<https://rig.rs/docs/concepts/hooks>。注意：截至 2026-09-12 `rig-agent` 0.42.0 的 docs.rs 已能看到 `AgentHook`，该页措辞偏保守。
- **版本沿革（docs.rs all-items 实测）**：`rig-core 0.37.0` 有 `PromptHook` + `ToolCallHookAction`（trait + enum，见 <https://docs.rs/rig-core/0.37.0/rig_core/agent/prompt_request/hooks.rs.html>）；`rig-core 0.40.0` 的 `agent::hook` 模块含 `Flow`、`StepEvent`、`StepEventKind`、`HookContext`、`HookStack`、`InvalidToolCallHookAction`；`rig-core 0.41.0` / `0.42.0` 的 rig-core 已无这些项，hook 归属 `rig-agent`（<https://docs.rs/rig-core/0.40.0/rig_core/all.html>、<https://docs.rs/rig-core/0.42.0/rig_core/all.html>、<https://docs.rs/rig-agent/0.42.0/rig_agent/all.html>）。
- doc-stated 约束：trait 要求 `WasmCompatSend + WasmCompatSync`。

### 3.2 `swiftide` 0.32.1 —— trait object（`Box<dyn Fn…>` 风格）

`swiftide::agents::hooks` 提供：

```rust
pub trait BeforeToolFn: for<'a> Fn(&'a Agent, &ToolCall)
    -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>
    + Send + Sync + DynClone { }
pub trait AfterToolFn: for<'tool> Fn(&'tool Agent, &ToolCall, &'tool mut Result<ToolOutput, ToolError>)
    -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'tool>>
    + Send + Sync + DynClone { }
```

并提供 `Hook` 枚举与 `HookTypes`、`MessageHookFn`、`MockHook`（<https://docs.rs/swiftide/latest/swiftide/all.html>、<https://docs.rs/swiftide/latest/swiftide/agents/hooks/trait.BeforeToolFn.html>）。**一手声明的限制**（模块文档）：*"Since rust does not have async closures, hooks have to return a boxed, pinned async block themselves."* 以及 *"Rust has a long outstanding issue where it captures outer lifetimes when returning an impl that also has lifetimes"*（指向 <https://github.com/rust-lang/rust/issues/42940>）—— <https://docs.rs/swiftide/latest/swiftide/agents/hooks/index.html>。版本：0.32.1 / 2025-11-15（<https://crates.io/api/v1/crates/swiftide>）；仓库 HEAD 最后提交 2026-08-10。

### 3.3 `AutoAgents` 0.4.0 —— `#[async_trait]` + typed enum

`crates/autoagents-core/src/agent/hooks.rs`（<https://github.com/liquidos-ai/AutoAgents/blob/main/crates/autoagents-core/src/agent/hooks.rs>）：

```rust
#[async_trait]
pub trait AgentHooks: AgentDeriveT + Send + Sync {
    async fn on_tool_call(&self, _tool_call: &ToolCall, _ctx: &Context) -> HookOutcome { HookOutcome::Continue }
    async fn on_tool_result(&self, _tool_call: &ToolCall, _result: &ToolCallResult, _ctx: &Context) {}
}
pub enum HookOutcome { Continue, Abort }
```

文档注释："Run the hook before executing the tool_call giving ability to Abort or Continue"。crate 版本 **0.4.0 / 2026-07-08**（<https://crates.io/api/v1/crates/autoagents>）。doc-stated 约束：supertrait `+ Send + Sync`。

### 3.4 `adk-rust` / `adk-plugin` 2.2.0 —— 结构体里的 `Option<Box<dyn Fn…>>` 回调 + 插件 trait

README（<https://github.com/zavora-ai/adk-rust/blob/main/adk-plugin/README.md>）列出 `EnhancedPlugin` trait（*"implement only the hooks you need"*）、`before_tool`（"Called before tool execution"）、`after_tool`（"Called after tool execution"）、`on_tool_error`（"Called on tool error, can provide fallback"），并声明 *"Priority-based pipeline execution (lower values run first)"*；回调形如 `before_model: Some(Box::new(|ctx, request| { Box::pin(async move { ... }) }))`。crate `adk-plugin` **2.2.0 / 2026-09-01**（<https://crates.io/api/v1/crates/adk-plugin>）。

### 3.5 官方 Rust MCP SDK `rmcp` 3.3.0 —— 逐方法 handler trait（不是通用 hook 管道）

- `rmcp::handler::server::ServerHandler: Sized + Send + Sync + 'static`，逐 MCP 方法一个异步方法（`call_tool`、`list_tools`、`get_tool`、`on_custom_request`、`on_cancelled`、`on_progress` 等）：<https://docs.rs/rmcp/latest/rmcp/handler/server/trait.ServerHandler.html>；`tower-service ^0.3` 是可选依赖；另导出 `service::Service` / `ServiceExt`。
- **UNVERIFIED**：在 rmcp 中未找到独立的「middleware」trait，能确认的 hook 形接缝只有上述 handler trait 与 `service::Service`。

### 3.6 `tower` 0.5.3 —— 通用 middleware 抽象（`Service` + `Layer`）

```rust
pub trait Service<Request> {
    type Response; type Error;
    type Future: Future<Output = Result<Self::Response, Self::Error>>;
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;
    fn call(&mut self, req: Request) -> Self::Future;
}
```

- 文档明确 **"This trait **is** dyn compatible."**；声明 `Layer` 用于包裹服务、与协议解耦。doc-stated 注意事项：*"Services are permitted to panic if `call` is invoked without obtaining `Poll::Ready(Ok(()))` from `poll_ready`. You should therefore be careful when cloning services…"* —— <https://docs.rs/tower/latest/tower/trait.Service.html>
- 版本 0.5.3 / docs.rs 页面 2026-06-20。

### 3.7 其他候选（存在性 / 是否有 hook）

- `llm-chain` 0.12.0（2023-05-31）：有 `Tool` trait / `ToolCollection`，**未发现** before/after tool-call hook 抽象（<https://docs.rs/llm-chain/0.12.0/llm_chain/tools/index.html>）。
- `kalosm`、`swarms-rs`、`wangnov/rust-genai`：仓库存在；**hook 抽象 UNVERIFIED**。
- `mcp-agent-rs`、`rskagents`：**未能确认存在 → UNVERIFIED**。

---

## 4. 几个小件的当前惯用法

### 4.1 CLI 参数解析

| crate | 最新版本 / 发布日期 | 一手要点 | 来源 |
| --- | --- | --- | --- |
| `clap` | **4.6.6 / 2026-08-06** | `derive` **不是**默认特性（`default = ["std","color","help","usage","error-context","suggestions"]`，`derive = ["dep:clap_derive"]`）；derive 参考文档：*"To derive `clap` types, you need to enable the `derive` feature flag."*；crate 根同时提供 "Derive tutorial and reference" 与 "Builder tutorial and reference"，并有 "Mixing Builder and Derive APIs" 一节说明两者可混用；存在 `unstable-v5` 特性（即 v5 未稳定）；crates.io 记录 `rust_version = "1.85"`、edition 2024，而 crate 根文档写 "MSRV, currently 1.74"（两处一手来源不一致，原样并列） | <https://crates.io/api/v1/crates/clap>、<https://docs.rs/clap/latest/clap/>、<https://docs.rs/clap/latest/clap/_derive/index.html> |
| `bpaf` | **0.9.27 / 2026-07-29** | 有可选 `derive` 特性 | <https://crates.io/api/v1/crates/bpaf> |
| `lexopt` | **0.3.2 / 2026-02-28** | 极小依赖、手写循环风格（版本历史共 6 个版本） | <https://crates.io/api/v1/crates/lexopt> |
| `argh` | **0.1.19 / 2026-03-16** | 描述为 *"Derive-based argument parsing optimized for code size"*，Google 维护 | <https://crates.io/api/v1/crates/argh> |

（版本均为 `max_stable_version`。）

### 4.2 错误处理：`anyhow` vs `thiserror` 的分工（一手原文）

- `anyhow` **1.0.104 / 2026-07-18**（<https://crates.io/api/v1/crates/anyhow>）。README「Comparison to thiserror」原文：*"Use Anyhow if you don't care what error type your functions return, you just want it to be easy. This is common in application code. Use thiserror if you are a library that wants to design your own dedicated error type(s) so that on failures the caller gets exactly the information that you choose."* 同一 README 另有：*"We do not bundle a `derive(Error)` macro but you can write the impls yourself or use a standalone macro like thiserror."* —— <https://raw.githubusercontent.com/dtolnay/anyhow/master/README.md>
- `thiserror` **2.0.20 / 2026-08-08**（<https://crates.io/api/v1/crates/thiserror>）。README「Comparison to anyhow」原文：*"Use thiserror if you care about designing your own dedicated error type(s) so that the caller receives exactly the information that you choose in the event of failure. This most often applies to library-like code. Use Anyhow if you don't care what error type your functions return, you just want it to be easy. This is common in application-like code."* docs.rs 还写：*"Thiserror deliberately does not appear in your public API… switching from handwritten impls to thiserror or vice versa is not a breaking change."* —— <https://raw.githubusercontent.com/dtolnay/thiserror/master/README.md>、<https://docs.rs/thiserror/latest/thiserror/>
- 依赖重量实测：`anyhow` = 1 个传递包，`thiserror` = 8 个（含 proc-macro2/quote/syn 系）。

### 4.3 追加写 JSONL

- `serde_json` **1.0.151 / 2026-07-20**（<https://crates.io/api/v1/crates/serde_json>）。其完整 item 列表**没有** JSONL / line-delimited 专用 API（<https://docs.rs/serde_json/latest/serde_json/all.html>）；逐行写只能自己用 `to_writer`（*"Serialize the given data structure as JSON into the I/O stream."*，并保证只向 writer 送入合法 UTF-8 —— <https://docs.rs/serde_json/latest/serde_json/fn.to_writer.html>）或 `Serializer::new` / `with_formatter` / `into_inner`（<https://docs.rs/serde_json/latest/serde_json/struct.Serializer.html>）后自行补 `\n`。
- 第三方 crate `serde-jsonlines`（import 名 `serde_jsonlines`）：**0.7.0 / 2025-01-14**，owner 是 `jwodder`（**不是** serde-rs 官方）；提供 `json_lines()`、`write_json_lines()`、`append_json_lines()`、`JsonLinesReader`/`Writer`，以及 `async` 特性下的 tokio `AsyncJsonLinesReader`/`Writer`/`Stream`/`Sink`：<https://crates.io/api/v1/crates/serde_jsonlines>、<https://docs.rs/serde-jsonlines/latest/serde_jsonlines/>。README 带 repostatus "Active" 徽章（<https://raw.githubusercontent.com/jwodder/serde-jsonlines/master/README.md>）；**仓库 archived 状态与最后提交日期 UNVERIFIED**（GitHub 限流）。
- **std 对追加速率/交错的一手声明**（`std::fs::OpenOptions::append`，std 1.98.1）：
  - 保证：*"Append mode guarantees that writes will be positioned at the current end of file, even when there are other processes or threads appending to the same file. This is unlike `seek(SeekFrom::End(0))` followed by `write()`, which has a race between seeking and writing during which another writer can write, with our `write()` overwriting their data."*
  - 限制：*"Keep in mind that this does not necessarily guarantee that data appended by different processes or threads does not interleave. The amount of data accepted a single `write()` call depends on the operating system and file system. A successful `write()` is allowed to write only part of the given data… If you rely on the filesystem accepting the message in a single write, make sure that all data that belongs together is written in one operation. This can be done by concatenating strings before passing them to `write()`."*
  - 另有 *"This function doesn't create the file if it doesn't exist"*（需配 `create`）与「read+append 混用时读位置可能在文件尾」的说明 —— <https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.append>
- `tokio::io::AsyncWriteExt`（tokio 1.53.1，feature `io-util`）提供 async `write` / `write_all` / `flush` 等；`write` 可能只写入部分缓冲（*"It is not considered an error if the entire buffer could not be written"*）、`write_all` 循环写，且 `write` 是 cancel-safe 而 `write_all` **不是**；**它本身不提供 append 打开语义**（打开用 `tokio::fs::OpenOptions`/`File`），也没有跨进程 append 保证 —— <https://docs.rs/tokio/latest/tokio/io/trait.AsyncWriteExt.html>

---

## 5. tokio 里「按路径 key 串行化并发写」的惯用做法与坑

### 5.1 `tokio::sync::Semaphore`（计数信号量）

- 类型：`pub struct Semaphore { … }` —— *"Counting semaphore performing asynchronous permit acquisition."*；文档：*"A semaphore maintains a set of permits… A semaphore differs from a mutex in that it can allow more than one concurrent caller to access the shared resource at a time."*
- 签名：`acquire(&self) -> Result<SemaphorePermit<'_>, AcquireError>`、`acquire_many(n)`、`acquire_owned(self: Arc<Self>)`、`acquire_many_owned`。
- **doc-stated 公平性**：*"This `Semaphore` is fair, which means that permits are given out in the order they were requested. This fairness is also applied when `acquire_many` gets involved, so if a call to `acquire_many` at the front of the queue requests more permits than currently available, this can prevent a call to `acquire` from completing, even if the semaphore has enough permits complete the call to `acquire`."*（队头阻塞）
- **doc-stated 坑**：`MAX_PERMITS = usize::MAX >> 3`，*"Exceeding this limit typically results in a panic."*；`new` 在超限时 panic，`add_permits` 超限 panic；*"Cancelling a call to `acquire` makes you lose your place in the queue."*；permit 在 drop 时归还，`SemaphorePermit::forget` 会故意泄漏 permit。
- 内存序声明：*"If a task writes some data and then releases a permit, any task that later acquires a permit is guaranteed to see that data."*
- 来源：<https://docs.rs/tokio/latest/tokio/sync/struct.Semaphore.html>（tokio 1.53.1 / 2026-07-20）。

### 5.2 `tokio::sync::Mutex`

- 文档「Which kind of mutex should you use?」原文：*"Contrary to popular belief, it is ok and often preferred to use the ordinary `Mutex` from the standard library in asynchronous code."* … *"The primary use case for the async mutex is to provide shared mutable access to IO resources such as a database connection. If the value behind the mutex is just data, it's usually appropriate to use a blocking mutex such as the one in the standard library or `parking_lot`."* … *"when you do want shared access to an IO resource, it is often better to spawn a task to manage the IO resource, and to use message passing to communicate with that task."*
- 公平性：*"Tokio's Mutex operates on a guaranteed FIFO basis. This means that the order in which tasks call the `lock` method is the exact order in which they will acquire the lock."*；*"Cancelling a call to `lock` makes you lose your place in the queue."*
- **doc-stated 坑**：*"in contrast to `std::sync::Mutex`, this implementation does not poison the mutex when a thread holding the `MutexGuard` panics. In such a case, the mutex will be unlocked. If the panic is caught, this might leave the data protected by the mutex in an inconsistent state."*
- 来源：<https://docs.rs/tokio/latest/tokio/sync/struct.Mutex.html>

### 5.3 按 key 的锁：真实项目与现成 crate

- **真实项目**：`vinhnx/VTCode`，`vtcode-core/src/tools/pty/manager.rs`（commit `a154162f`）用「全局 map + per-key tokio Mutex」：

  ```rust
  /// Per-workspace command locks to serialize long-running toolchain commands.
  static WORKSPACE_COMMAND_LOCKS: Lazy<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> =
      Lazy::new(|| Mutex::new(HashMap::new()));
  ```

  注释原文：*"Per-workspace command locks to serialize long-running toolchain commands. Keyed by canonicalized workspace path to prevent lockfile contention. This is more granular than a global lock - different workspaces can run concurrently."*（`Mutex` 为 `parking_lot::Mutex`，`HashMap` 为 `hashbrown::HashMap`；用 `Entry` get-or-insert，`lock.lock().await` 跨 await 持有）。来源：<https://github.com/vinhnx/VTCode/blob/a154162f1f195011fb722fd443394ceb212eea94/vtcode-core/src/tools/pty/manager.rs>
- **`keyed-lock` 0.2.3 / 2025-07-23**：`pub struct KeyedLock<K: Eq + Hash + Clone>`，`lock(&self, key) -> Guard<'_, K>`、`lock_owned(self: &Arc<Self>, key) -> OwnedGuard<K>`；文档页面**没有**声明额外限制：<https://docs.rs/keyed-lock/latest/keyed_lock/sync/struct.KeyedLock.html>
- **`async-lock` 3.4.2 / 2025-12-21**：**不存在** `KeyedMutex` / `KeyedLock`（原语只有 `Barrier`、`Mutex`、`RwLock`、`Semaphore`、`OnceCell` 及 guard）。文档声明：*"You need to hold a lock across an `.await` point. (Holding an `std::sync` lock guard across an `.await` will make your future non-`Send`, and is also highly likely to cause deadlocks.)"*，并建议 *"In general, you should consider using `std::sync` types over types from this crate."* —— <https://docs.rs/async-lock/latest/async_lock/index.html>
- **`dashmap` 6.2.1 / 2026-05-17**：`pub struct DashMap<K, V, S = RandomState>`，*"DashMap tries to be very simple to use and to be a direct replacement for `RwLock<HashMap<K, V>>`."* 多数方法（`insert`/`remove`/`get`/`get_mut`/`iter`/`iter_mut`/`retain`/`clear`/`entry` 等）都带同一条 **doc-stated 死锁警告**：*"**Locking behaviour:** May deadlock if called when holding any sort of reference into the map."* 另有 `try_entry`（*"Returns None if the shard is currently locked."*）、`try_get`/`try_get_mut`（`TryResult::Locked`）、shard 数必须是 2 的幂否则 panic、`alter`/`alter_all`/`view` 的闭包 panic 会 abort 进程 —— <https://docs.rs/dashmap/latest/dashmap/struct.DashMap.html>

### 5.4 actor + `tokio::sync::mpsc`（Tokio 官方教程的一手表述）

- Shared state 页：*"There are a couple of different ways to share state in Tokio. 1. Guard the shared state with a Mutex. 2. Spawn a task to manage the state and use message passing to operate on it. Generally you want to use the first approach for simple data, and the second approach for things that require asynchronous work such as I/O primitives."* —— <https://tokio.rs/tokio/tutorial/shared-state>
- Channels 页：*"The pattern involves spawning a dedicated task to manage the `client` resource. Any task that wishes to issue a request sends a message to the `client` task."*；*"the channel works as a buffer. Operations may be sent to the `client` task while the `client` task is busy."*；*"It is not possible to clone the receiver of an `mpsc` channel."*；有界通道容量 32，满时 `send(...).await` 睡眠等待 —— <https://tokio.rs/tokio/tutorial/channels>

### 5.5 doc-stated 坑（汇总）

1. **`std::sync::MutexGuard` 跨 `.await`**（Tokio 教程）：*"if Tokio suspends your task at an `.await` while the task is holding the lock, some other task may be scheduled to run on the same thread, and this other task may also try to lock that mutex, which would result in a deadlock…"*，并警告 *"some mutex crates implement `Send` for their MutexGuards. In this case, there is no compiler error… The code compiles, but it deadlocks!"* —— <https://tokio.rs/tokio/tutorial/shared-state>
2. **tokio Mutex 不 poison**（5.2 引用）。
3. **DashMap 持引用时再调用会死锁**（5.3 引用）。
4. **无界通道内存无上界**：`unbounded_channel` 文档：*"If the receiver falls behind, messages will be arbitrarily buffered. **Note** that the amount of available system memory is an implicit bound to the channel. Using an `unbounded` channel has the ability of causing the process to run out of memory. In this case, the process will be aborted."* —— <https://docs.rs/tokio/latest/tokio/sync/mpsc/fn.unbounded_channel.html>
5. **队列必须有界**（Tokio 教程）：*"Unbounded queues will eventually fill up all available memory and cause the system to fail in unpredictable ways."*、*"When using `mpsc::channel`, pick a manageable channel capacity."* —— channels 页同上。
6. Semaphore 取消会丢失排队位置、`acquire_many` 队头阻塞（5.1 引用）。

---

## 附录 A：版本与日期总表（截至 2026-09-12）

| 名称 | 版本 | 日期 | 来源 |
| --- | --- | --- | --- |
| Rust stable | 1.98.1 | 2026-09-03 | <https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/> |
| 本机 rustc | 1.94.0 (4a4ef493e) | 2026-03-02 | 本地 `rustc --version` |
| AFIT / RPITIT 稳定 | Rust 1.75.0 | 2023-12-28 | <https://blog.rust-lang.org/2023/12/28/Rust-1.75.0/> |
| reqwest | 0.13.5 | 2026-09-08 | crates.io API |
| async-openai | 0.42.0 | 2026-09-09 | crates.io API |
| genai（stable / beta） | 0.6.5 / 0.7.0-beta.23 | 2026-06-06 / 2026-09-08 | crates.io API |
| rig / rig-core / rig-agent | 0.42.0 | 2026-08-17 | crates.io API |
| eventsource-stream | 0.2.3 | 2022-02-17 | crates.io API |
| reqwest-eventsource | 0.6.0 | 2024-03-29 | crates.io API |
| sse-stream | 0.2.6 | 2026-09-03 | crates.io API |
| eventsource-client | 0.18.0 | 2026-08-10 | crates.io API |
| async-trait | 0.1.92 | 2026-08-08 | crates.io API |
| trait-variant | 0.1.3 | 2026-07-22 | crates.io API |
| dynosaur | 0.3.1 | 2026-07-03 | crates.io API |
| clap | 4.6.6 | 2026-08-06 | crates.io API |
| bpaf / lexopt / argh | 0.9.27 / 0.3.2 / 0.1.19 | 2026-07-29 / 2026-02-28 / 2026-03-16 | crates.io API |
| anyhow / thiserror | 1.0.104 / 2.0.20 | 2026-07-18 / 2026-08-08 | crates.io API |
| serde_json | 1.0.151 | 2026-07-20 | crates.io API |
| serde-jsonlines | 0.7.0 | 2025-01-14 | crates.io API |
| tokio | 1.53.1 | 2026-07-20 | crates.io API / docs.rs |
| tower | 0.5.3 | 2026-06-20（docs.rs 页面） | docs.rs |
| dashmap | 6.2.1 | 2026-05-17 | crates.io API |
| async-lock | 3.4.2 | 2025-12-21 | crates.io API |
| keyed-lock | 0.2.3 | 2025-07-23 | crates.io API |
| swiftide | 0.32.1 | 2025-11-15 | crates.io API |
| autoagents | 0.4.0 | 2026-07-08 | crates.io API |
| adk-plugin | 2.2.0 | 2026-09-01 | crates.io API |
| rmcp | 3.3.0 | 2026-09-10 | crates.io API |

## 附录 B：本简报中明确 UNVERIFIED 的条目

1. OpenAI 官方 streaming 文档正文（页面 JS 渲染，未抓到）；其措辞未核实，字段级证据来自 `async-openai` 源码类型与 `genai` 的 `/delta/tool_calls` 解析路径。
2. 「`async-openai` 曾更换维护者」：crates.io 记录的所有者始终是 `64bit`，未找到一手声明。
3. `async-openai` 流类型的历史改名细节（只能确认 0.42.0 中不存在 `ChatCompletionStream`）。
4. rust-lang/rust#133119 的 open/closed 状态（GitHub API 403 限流、HTML 截断、atom 406）；标题已核实。
5. `eventsource-stream` / `reqwest-eventsource` 的仓库 archived 标志：未发现 archive 横幅，只有「发布与提交都停在 2022 / 2024」这一事实。
6. `serde-jsonlines` 仓库 archived 状态与最后提交日期（只核实了发布 0.7.0 / 2025-01-14 与 README 的 "Active" 徽章）。
7. 「枚举分发」作为 AFIT 官方替代：官方 async book 与 Rust 官方博客中未找到陈述（async book 的 Async traits 章节仍是 TODO 占位）。
8. `rmcp` 是否有独立 middleware trait（只确认 handler trait 与 `service::Service`）。
9. `kalosm` / `swarms-rs` / `wangnov/rust-genai` 是否有 hook 抽象；`mcp-agent-rs` / `rskagents` 是否存在。
10. `async-lock` 的 `KeyedMutex`：确认在 3.4.2 中**不存在**（此题设本身不成立）。
11. `clap` 的 MSRV：crate 根文档写 1.74，crates.io 4.6.6 元数据写 1.85，两处一手来源不一致。
12. 各 crate 传递依赖数是 2026-09-12 用固定命令的**实测值**，非文档声明；随特性组合与时间会变化。

---

*本文件由票 08 的研究子代理撰写；仓库内其他文件未改动（含本票自身——按当前任务约束，未在票内追加指针；如需回填「产物指针」，由主代理执行）。*
