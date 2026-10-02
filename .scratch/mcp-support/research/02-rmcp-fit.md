# 调研：`rmcp` 3.5.0 与 fs-agent 的契合度

> 回答 [票 01](../issues/01-research-rmcp-fit.md) 的六个问题：不是「`rmcp` 能不能用」，而是
> 「它嵌进 fs-agent 要付多少代价」。写就日期 **2026-10-03**。
>
> **方法与范围**。一手来源 = crates.io / docs.rs 上 `rmcp` 3.5.0 的页面、官方仓库
> `modelcontextprotocol/rust-sdk` 的源码与 `Cargo.toml`、`process-wrap` / `sse-stream` 的
> crates.io 依赖表；本地对照 = fs-agent 的 `Cargo.toml` / `Cargo.lock`、
> `src/tools/tool.rs`、`src/tools/process.rs`、`src/tools/sandbox.rs`、`src/cli.rs`。
> 下文凡是**源码行号**，均取自 2026-10-03 抓取的 `main` 分支文件（URL 各节给出）；凡是
> **docs.rs 行号**，取自版本锁定的 3.5.0 页面。**没有编译验证**（本次调研不允许
> `cargo add` / `cargo build`），所以 feature 组合与体积是纸面推算，落地第一件事是验一遍。
> 仓库内既有事实沿用 [`01-mcp-client-implementation.md`](01-mcp-client-implementation.md)。

## 1. 只做 client 时的依赖树与体积

**feature 面。** `rmcp` 3.5.0 共 28 个 feature、`default` 是
`["base64", "macros", "server"]`（`crates/rmcp/Cargo.toml` 的 `[features]`；docs.rs 的
feature 页面同样列出 6 个默认项）。只做 client 必须 `default-features = false`；client 侧的
最小集是：

```toml
rmcp = { version = "3.5.0", default-features = false, features = [
  "client",                                  # dep:tokio-stream；service/ClientHandler 的开关
  "transport-child-process",                 # dep:process-wrap + tokio/process
  "transport-streamable-http-client-reqwest",# 含 client-side-sse + transport-worker
  "reqwest-native-tls",                      # TLS 后端，理由见下
] }
```

出处：<https://docs.rs/crate/rmcp/3.5.0/features> ·
<https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/Cargo.toml>。
注意几个 feature 的**传递关系**是写死的：`transport-streamable-http-client-reqwest` →
`transport-streamable-http-client` → `client-side-sse` + `transport-worker`，而
`client-side-sse` 又会拉 `base64` / `bytes` / `http` / `sse-stream`；
`transport-child-process` → `transport-async-rw` + `tokio/process` + `process-wrap`。所以
「只开两条 transport」实际上会把 SSE 解析与 base64 也带进来。`elicitation` 只是
`["dep:url"]`，是 URL mode 的额外件；form mode 的类型在 `model` 里，不必开它。

**TLS 必须选 `reqwest-native-tls`。** `reqwest` 这个 feature 会把 `reqwest/rustls` 打开，
`reqwest-native-tls` 打开的是 `reqwest/native-tls`（docs.rs 的 feature 页）。fs-agent 的
`reqwest` 明确选了 `native-tls` 并写了理由（`Cargo.toml:20-22`：「`native-tls` 直接链系统的
OpenSSL，不去拉 aws-lc-rs 那套 C 构建」），而 Cargo 的 feature 是并集 —— 选
`reqwest`（rustls）等于把这条决定推翻，所以要对齐的是 `reqwest-native-tls`。

**依赖重叠。** `rmcp` 的**非 optional** 依赖是 `serde`（derive, rc）、`serde_json`、
`thiserror`、`tokio`（sync/macros/rt/time）、`futures`、`indexmap`（serde）、`tracing`、
`tokio-util`、`pin-project-lite`、`chrono`（`crates/rmcp/Cargo.toml` 的 `[dependencies]`；
crates.io 依赖表 <https://crates.io/api/v1/crates/rmcp/3.5.0/dependencies>）。这十项**每一项
都已经在 fs-agent 的 `Cargo.lock` 里**（实测：`Cargo.lock` 共 318 个 `[[package]]`，上列
十个名字全部命中）。再加 client 侧的 `tokio-stream`、`process-wrap`、`sse-stream`、
`reqwest`（版本 `^0.13.2`）与 `bytes`、`http`、`http-body`、`http-body-util`、
`futures-util`、`nix` —— **这些也全部命中现有 lock**，且锁定的版本都满足 `rmcp` 的约束
（`reqwest 0.13.5` ≥ `0.13.2`、`base64 0.23.1`、`http 1.5.0`、`chrono 0.4.45`、
`tokio-util 0.7.19`、`indexmap 2.14.2`）。

**净新增 lock 包 ≈ 4 个**：`rmcp` 自己 + `process-wrap` + `sse-stream` + `tokio-stream`。
`process-wrap 10.0` 的依赖只有 `indexmap`（必需）与 `nix` / `futures` / `tracing` / `tokio`
（optional，按 feature 开），`sse-stream 0.2.4` 的依赖是 `bytes` / `futures-util` /
`http-body` / `http-body-util` / `pin-project-lite`（皆已在 lock）
（<https://crates.io/api/v1/crates/process-wrap/10.0.0/dependencies> ·
<https://crates.io/api/v1/crates/sse-stream/0.2.4/dependencies>）。所以 318 → 大约 322，
**不会重排或替换现有任何一条依赖**（与 [`01`](01-mcp-client-implementation.md) 的
「纯新增」结论一致）。

**工具链**。`rmcp` 3.5.0 是 `edition 2024` / `rust-version 1.88`（workspace 的
`Cargo.toml`）；本机 `rustc 1.94.0`，够。fs-agent 自己是 `edition 2021`，不冲突。

## 2. API 形状

**两个入口、各约 3–5 行。** 官方 README 的 stdio 示例（<https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/README.md>）：

```rust
let transport = TokioChildProcess::new(Command::new("npx").configure(|cmd| {
    cmd.arg("-y").arg("@modelcontextprotocol/server-everything");
}))?;
let client = ClientConfig::default()
    .serve_with_lifecycle(transport, ClientLifecycleMode::Discover {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
    })
    .await?;
```

HTTP 只把 transport 换掉：`StreamableHttpClientTransport::from_uri("http://localhost:8000/mcp")`
（README 的 Streamable HTTP 一节）。`serve_with_lifecycle` 的签名是
`ClientServiceExt::serve_with_lifecycle<T, E, A>(self, transport: T, lifecycle: ClientLifecycleMode)
-> impl Future<Output = Result<RunningService<RoleClient, Self>, ClientInitializeError>>`
（<https://docs.rs/rmcp/3.5.0/rmcp/service/trait.ClientServiceExt.html>）。老的
`().serve(transport)` 就是 `ClientLifecycleMode::Initialize` 那条路
（`crates/rmcp/src/service/client.rs:721` 的 `serve_client_with_lifecycle(..., ClientLifecycleMode::Initialize, ct)`）。

**异步模型 = tokio 原生、不自建 runtime。** `service` 模块内部用的是 `tokio::spawn`
（无 `local` feature 时；`crates/rmcp/src/service.rs:1319-1327`），要求 future 是
`Send + 'static`，也就是说**调用者必须已经在 tokio runtime 上下文里**（fs-agent 是
`tokio::runtime::Builder::new_multi_thread()`，`src/cli.rs:89`），但 `rmcp` **不会**替你
`Builder::new_*`。`RunningService` 自己带一个后台服务任务，`Drop` 会取消它
（<https://docs.rs/rmcp/3.5.0/rmcp/service/struct.RunningService.html>）。

**`ClientHandler` 是那个 trait，不是 `Service`。** 客户端实现
`ClientHandler`（`crates/rmcp/src/handler/client.rs`），`rmcp` 给出
`impl<H: ClientHandler> Service<RoleClient> for H` 这层胶水；句柄是
`RunningService<RoleClient, S>`，它 `Deref<Target = Peer<RoleClient>>`（`service.rs:1066-1072`）。
`ClientHandler` 的超 trait 是 `Sized + Send + Sync + 'static`（非 `local` 构建；
`handler/client.rs` 的 trait 定义），方法是 native `async fn in trait`（返回
`impl Future + MaybeSendFuture`），**不是** `async_trait`。它给了一整套默认实现：
`ping`、`create_message`、`list_roots`、`create_elicitation`、`on_progress`、
`on_tool_list_changed`、`on_cancelled` 等。`impl ClientHandler for ()` 与 `for ClientConfig`
是现成的「零实现」形态。

**错误类型三层。**

| 层 | 类型 | 形状 |
|---|---|---|
| 连接期 | `ClientInitializeError` | `#[non_exhaustive]`；`ExpectedInitResponse` / `ConnectionClosed` / `TransportError` / `JsonRpcError(ErrorData)` / `NoCompatibleProtocolVersion` / `NoPreferredProtocolVersion` / `Cancelled` / `LegacyFallbackFailed`（`crates/rmcp/src/service/client.rs:66-90`） |
| 调用期 | `ServiceError` | `#[non_exhaustive]`；`McpError(ErrorData)` / `TransportSend` / `TransportClosed` / `UnexpectedResponse` / `SubscriptionLagged` / `Cancelled` / `Timeout` / `InputRequiredRoundsExceeded`（<https://docs.rs/rmcp/3.5.0/rmcp/service/enum.ServiceError.html>，定义在 `service.rs:79-97`） |
| 协议错误本体 | `ErrorData` | JSON-RPC 的 code/message/data（`lib.rs` 的 `pub use error::{ErrorData, RmcpError}`） |

对 fs-agent 的直接含义：`Tool::call` 只认 `ToolError`（`src/tools/tool.rs:49-74`），所以每个
MCP 错误都要在那个边界上换成一条带 `code` 的中文句子 —— 这正是「`mcp_call` 的错误码形状」
那条雾要定的事（`map.md` 的 `Not yet specified`）。

## 3. 与 fs-agent 的接法

**「调用一个工具」已经是现成的 `async fn`。** 高层 helper 是
`RunningService<RoleClient, S>::call_tool(&self, params: CallToolRequestParams) -> Result<CallToolResult, ServiceError>`
（docs.rs 的 `RunningService` 方法表；实现见 `crates/rmcp/src/service/client.rs:1966-1972`）。
参数结构是 `CallToolRequestParams { meta, name: Cow<'static, str>, arguments: Option<JsonObject>,
input_responses, request_state }`（`crates/rmcp/src/model.rs:4247-4264`）。`arguments` 是
自由的 `serde_json::Map`，`rmcp` 不校验它 —— 与冻结项 7「`arguments` 是一个自由 JSON 对象、
fs-agent 不校验、只透传」**逐字对齐**，零摩擦。

**`Send + Sync` 不冲突。** fs-agent 的 `Tool` 是 `#[async_trait] pub trait Tool: Send + Sync`
（`src/tools/tool.rs:201-202`），`RunningService` 的自动 trait 里 `Send` / `Sync` 都成立
（docs.rs 的 `Auto Trait Implementations`），所以 `Arc<RunningService<...>>` 可以直接放进一个
工具结构体，`call(&self, …)` 直接用。两边一个用 `async_trait`（boxed future）、一个用
RPITIT，**是两种独立写法，不需要统一，也不互相限制**。唯一要留意的细节是
`RunningService::close()` / `close_with_timeout()` 要 `&mut self`、`cancel()` 消费 `self`
（docs.rs）—— 而 `Tool` 只给 `&self`：工具持 `Arc` 时没法 await 一次优雅关闭。能兜底的只有
`RunningService` 的 `Drop`（自动取消，不等清理）。冻结项 9「server 进程随会话起、随会话停」
仍然成立，但「停」是 Drop 语义还是 await 语义，归票 02 / 06 定。

**进程管理：`rmcp` 自己 spawn，但可以把包好的 argv 交给它。**
`TokioChildProcess::new(command: impl Into<CommandWrap>)` 内部就是 `cmd.spawn()`
（`crates/rmcp/src/transport/child_process.rs` 的 `TokioChildProcessBuilder::spawn`），
`CommandWrap` 来自 `process-wrap`。fs-agent 的 `process::run` 整条都不能复用 —— 它假设
「一次调用一个进程、stdout/stderr 读干当结果、超时 `killpg` 整组」
（`src/tools/process.rs:105-133`、`:232-267`），而 MCP server 是常驻的、stdout 要跑协议
（[`01`](01-mcp-client-implementation.md) §4.3 已记）。**可行接法**是把
`Sandbox::wrap` 这个纯函数（`src/tools/sandbox.rs:273-324`）的输出重新拼成一个
`tokio::process::Command`：

```rust
let argv = sandbox.wrap(&configured_argv, cwd)?;          // 纯函数，原样可用
let cmd = Command::new(&argv[0]).args(&argv[1..]);        // bwrap … -- real-server
let transport = TokioChildProcess::new(cmd)?;
```

这样 server 仍在 `--ro-bind / /` + 只有会话 cwd / `writable_roots` 可写的边界之内
（冻结项 9 与 `docs/sandbox.md` 的那一层），代价是要自己保证 argv 的构造与 `bash` 走同一套
（`src/tools/bash.rs` 那条路）。

**两个必须写下来的差异。**（a）**清理语义变弱**：`TokioChildProcess` 的 `Drop` 走 process-wrap
的 `ChildWrapper::kill()`（`child_process.rs` 的 `ChildWithCleanup`），fs-agent 现在是
`process_group(0)` + `libc::killpg` 杀整棵树（`src/tools/process.rs:127`、`:251-258`）。
**推断**（源码未展开 process-wrap 的默认 wrapper）：默认路径只保证直接子进程被杀，server
自己再起的孙进程不在内 —— 若要组语义得看能否在 `CommandWrap` 上叠 process-wrap 的进程组
包装。（b）**stderr 归属不同**：`TokioChildProcessBuilder` 默认 `stderr(Stdio::inherit())`、
stdin/stdout piped（`child_process.rs`），可以 `.stderr(Stdio::piped())` 拿回句柄；`rmcp`
自己不做环境清洗 —— 冻结项 12 / 13 与 DSH 的 `scrubbedParentEnv()`（[`01`](01-mcp-client-implementation.md) §2）
在 `rmcp` 这一侧没有对应物，要自己 `env_clear()` / 白名单后再交给它。

## 4. MRTR 与 elicitation

**「自动驱动」到什么程度：回调是有的，而且挂点就是 elicitation。** 高层
`call_tool` / `get_prompt` / `read_resource` 会循环处理
`resultType: "input_required"`：把 `InputRequiredResult.inputRequests` 里**每一个**请求交给本地
`ClientHandler`（`fulfill_input_request` 的 `match`：`CreateMessage` → `create_message`、
`Elicitation` → `create_elicitation`、`ListRoots` → `list_roots`），拿回
`inputResponses` + 原样回显的 `requestState`，然后重试原请求，上限
`DEFAULT_MRTR_MAX_ROUNDS`（10），可用 `call_tool_with_mrtr_max_rounds` 改；手动模式是
`call_tool_once`（`crates/rmcp/src/service/client.rs` 的 `call_tool` / `prepare_input_required_retry`
/ `fulfill_input_request`；README 的 Multi-Round-Trip Requests 一节）。

**能挂回调。** `ClientHandler::create_elicitation(&self, request: ElicitRequestParams,
context: RequestContext<RoleClient>) -> impl Future<Output = Result<ElicitResult, ErrorData>>`
（`crates/rmcp/src/handler/client.rs`）。它**默认一律 `Decline`**，文档注释原话
「The default implementation automatically declines all elicitation requests. Real clients
should override this to provide user interaction.」。`ElicitRequestParams` 分 form / URL
两种模式，结果是 `ElicitResult { action: accept | decline | cancel, content }` —— 与规范
`2026-07-28` 的形状（[`01`](01-mcp-client-implementation.md) §1.7）一致。

**与冻结项 16 的关系：形状对得上，接缝在 `'static`。** 冻结项 16 要的是「把
`input_required` 接到 `ask_user_question` 那条第三类发起者的路上」。`rmcp` 给的正是一个
可 override 的回调，所以**不需要新开第四类发起者，也不需要自己写重试回路** —— 这是一个
实打实的契合点。但有一个必须解决的约束：`ClientHandler` 要求 `'static` + `Send + Sync`
（§2），而 fs-agent 的问询端口现在是**借用**在 `ToolContext<'a>` 上的
`questions: Option<&'a dyn UserQuestions>`（`src/tools/tool.rs:133-135`，
`SessionScaffold.questions` 在 `src/lib.rs:85-89`）。连接的生命周期比一次 `Tool::call` 长，
所以实现 `ClientHandler` 的那个类型**不能借**这个端口，只能持有一个 `'static` 的句柄
（`Arc<dyn UserQuestions>` 之类）——这要求端口提供方改成可共享的形状，或者给 MCP 连接一条
平行的端口。**这一条是票 02 的取舍点，不是 `rmcp` 的缺口。**

另外两件事：要让 server 真的发 elicitation，client 得在 capabilities 里声明它
（`ClientCapabilities::builder().enable_elicitation().build()`，`crates/rmcp/src/model.rs:1194`
的文档示例）；`Discover` 生命周期下这些能力随每请求 `_meta` 送出。以及
`call_tool_once` 是「自己驱动 MRTR」的出口，如果票 02 决定不要把 elicitation 接到问询端口
（例如先只支持 form mode 里最简单的一类），它是现成的降级路径。

## 5. `server/discover` 与只谈最新版

**能，而且开关就是 lifecycle mode。** `ClientLifecycleMode::Discover { preferred_versions }`
是「直接用 `server/discover`」：不发 `initialize`、不发 `notifications/initialized`，启动由
discovery 完成，之后每个请求自带协议版本 / client info / capabilities
（README 的 Client lifecycle modes 一节）。fs-agent 的「只谈 2026-07-28」= 传
`vec![ProtocolVersion::V_2026_07_28]`。

**而且它不会偷偷回退。** 源码注释写得很直白：
`ClientLifecycleMode::Discover` 那一支里一行注释是
`// Discover mode does not fall back; a legacy server is an error.`
（`crates/rmcp/src/service/client.rs:783-795`）；只有 `ClientLifecycleMode::Auto` 才会
「probe the discover lifecycle and fall back when a legacy server reports that
`server/discover` is not implemented or does not respond within **10 seconds**」
（README 同节；实现与 `LegacyFallbackFailed` 变体在 `client.rs:799-826`）。所以
冻结项 19「不做旧版 `initialize` 回退」在 `rmcp` 里**不需要额外配置**，只要不选
`Auto` / `Initialize` 就行；对端只会旧版时，报错落在
`ClientInitializeError::JsonRpcError` / `NoCompatibleProtocolVersion` 一类变体上，是一条
可翻成中文的启动失败，而不是静默降级。

两个附带事实。`ClientLifecycleMode` 是 `#[non_exhaustive]`（`client.rs:640-650`），未来加
模式不破坏编译。`preferred_versions` 为空会直接报
`ClientInitializeError::NoPreferredProtocolVersion`（`client.rs:66-90` 的变体表）。

## 6. 不用的代价：自己写要自己实现哪几件

按「只谈 2026-07-28 + 只做 tools + stdio」和「补齐 HTTP / MRTR」两档估。以下每一项都是
`rmcp` 现成的东西：

1. **JSON-RPC 2.0 的帧与 id 关联。** stdio 上是 newline-delimited 的 JSON、每条消息一行；
   请求/响应/通知三种、错误对象、id 关联（[`01`](01-mcp-client-implementation.md) §1.3）。
   HTTP 上是单 endpoint 的
   POST + request-scoped 的 SSE 流，取消靠关流。
2. **每请求 `_meta`。** `io.modelcontextprotocol/protocolVersion`、
   `clientCapabilities`（必需）、`clientInfo`（SHOULD）；HTTP 上还要镜像成
   `Mcp-Method` / `Mcp-Name` / `MCP-Protocol-Version` 头，并做「头体不一致要报错」的校验。
3. **`server/discover`。** 启动第一步；解析 `supportedVersions` / `capabilities` /
   `instructions` 与缓存提示，并自己决定版本不匹配时的错误语义。
4. **`tools/list` 与 `tools/call`。** 前者含 `next_cursor` 分页、`ttlMs` / `cacheScope` 的
   新鲜度语义；后者含 `isError` / `structuredContent` / 各 `ContentBlock` 的投影，以及
   `arguments` 的自由透传。
5. **MRTR 重试回路。** `input_required` 的识别、`inputRequests` 的 fulfill、`requestState`
   原样回显、轮次上限，以及「只有 `requestState` 没有 input 请求」那类空转轮之间的小退避
   （`rmcp` 里是 `sleep_state_only_round`，50 ms 起、上限 250 ms）。
6. **进度与取消。** `_meta.progressToken` + `notifications/progress`；stdio 上发
   `notifications/cancelled`，HTTP 上关掉该请求的响应流。这两条在 fs-agent 里都没有落点
   （工具调用是「一次 await 出结果」），做不做是产品问题。
7. **HTTP 的并发与控制队列。** `rmcp` 客户端允许 16 个普通 POST 并发，取消 / 回复走单独的
   队列、默认 5 秒超时（README 的 Streamable HTTP 一节）—— 自写就得自己定这些数。

**量级**（不给精确行数）：只做 stdio + `tools/list` + `tools/call` 的最小可用 client，
**500–900 行**（类型定义 + 帧读写 + 一次 discover + 两个方法 + 错误映射）；再补
Streamable HTTP（含 SSE 解析）与 MRTR / 取消 / 进度，**1500–2500 行**。对照之下，
用 `rmcp` 的接入成本是「1 条依赖 + 一个 `ClientHandler` impl + 两个 transport 构造」，
再加上 §3 那几处自己必须做的适配（沙箱 argv、环境清洗、错误映射、`'static` 端口）。

## 推荐

**推荐用 `rmcp` 3.5.0，`default-features = false`，feature 组合照 §1。** 三条最硬的理由：

1. **协议策略零摩擦**：`ClientLifecycleMode::Discover` 就是「只谈最新版、不回退」，与冻结项
   19 逐字对齐，且有源码注释为证（§5）。
2. **MRTR 的挂点正是冻结项 16 要的形状**：`ClientHandler::create_elicitation` 是一个可
   override 的异步回调，自动重试回路现成（§4）。
3. **依赖成本极低**：`rmcp` 的直接依赖与 fs-agent 现有依赖高度重叠，净新增 lock 包约 4 个
   （§1），stdio server 还能继续过 `Sandbox::wrap`（§3）。

**保留的三个真取舍，都归票 02：**

- `process-wrap` 的默认清理弱于现在的 `killpg` 整组（§3a）——要组语义就得确认能否叠 wrapper，
  或者接受「只杀直接子进程」。
- `ClientHandler` 的 `'static` 与借用式 `questions` 端口不兼容（§4）——要接 elicitation
  就得先把问询端口改成可共享的形状。
- `RunningService::close` 的 `&mut self` / `cancel` 的消费语义与 `Arc<dyn Tool>` 有摩擦
  （§3）——优雅关闭 vs Drop 兜底是个选择。

**推荐不是决定，决定归 [票 02](../issues/02-grilling-sdk-or-own-client.md)。** 本文件只把
事实与代价摆出来。

## 来源

### 外部

- crates.io：<https://crates.io/crates/rmcp> · 版本依赖表
  <https://crates.io/api/v1/crates/rmcp/3.5.0/dependencies> ·
  <https://crates.io/api/v1/crates/process-wrap/10.0.0/dependencies> ·
  <https://crates.io/api/v1/crates/sse-stream/0.2.4/dependencies>
- docs.rs（版本锁定 3.5.0）：feature 页 <https://docs.rs/crate/rmcp/3.5.0/features> ·
  `ServiceError` <https://docs.rs/rmcp/3.5.0/rmcp/service/enum.ServiceError.html> ·
  `RunningService` <https://docs.rs/rmcp/3.5.0/rmcp/service/struct.RunningService.html> ·
  `ClientServiceExt` <https://docs.rs/rmcp/3.5.0/rmcp/service/trait.ClientServiceExt.html>
- 官方仓库 `modelcontextprotocol/rust-sdk`（main，2026-10-03 抓取）：
  `Cargo.toml`、`crates/rmcp/Cargo.toml`、`README.md`、
  `crates/rmcp/src/lib.rs`、`.../src/model.rs`、`.../src/service.rs`、
  `.../src/service/client.rs`、`.../src/handler/client.rs`、
  `.../src/transport/child_process.rs`
- 规范侧沿用 [`01`](01-mcp-client-implementation.md) 的 URL（`2026-07-28` 的 transports / tools /
  elicitation / changelog）。

### 仓库内

- `Cargo.toml:20-22`（`reqwest` 的 `native-tls` 与理由）、`:26`（tokio features）。
- `Cargo.lock`：318 个包；§1 列出的重叠项逐个命中（2026-10-03 实测）。
- `src/tools/tool.rs:49-74`（`ToolError`）、`:109-138`（`ToolContext`，含 `:133-135` 的
  `questions`）、`:201-249`（`Tool` trait）。
- `src/tools/process.rs:105-133`（`run` 的 spawn 与沙箱包装）、`:127`（`process_group(0)`）、
  `:232-267`（`ProcessGroup` / `killpg`）。
- `src/tools/sandbox.rs:273-324`（`Sandbox::wrap`，纯函数）。
- `src/tools/custom.rs:73-77`（「外来工具恒 `Exclusive`」的先例）。
- `src/cli.rs:89`（`tokio::runtime::Builder::new_multi_thread()`）。
- `src/lib.rs:85-89`（`SessionScaffold.questions`）。
