# 一手调研：把 MCP 接进 fs-agent

> 这份文件回答一个问题：**如果 fs-agent 要做一个 MCP client，事实是什么。** 写就日期 2026-10-03。
> 它只是调研，不是决定，也不是计划——凡是「要怎么做」的地方，都停在「需要人拍板」那一节。
>
> 核实过的仓库内位置：`.scratch/fs-agent-v1/spec.md`、`README.md`、`AGENTS.md`、`CONTEXT.md`、
> `src/tools/{mod,tool,registry,custom,process,sandbox,bash,ask_user}.rs`、`src/permissions.rs`、
> `src/events.rs`、`src/provider/projection.rs`、`src/config.rs`、`src/lib.rs`、`src/cli.rs`、
> `src/session/store.rs`、`docs/{sandbox,credentials}.md`、`docs/adr/0006`、`docs/adr/0007`、
> `docs/research/`（一手引文，一个字未改）。核实过的外部位置：MCP 规范 2026-07-28（含
> `changelog`、`deprecated`、`basic/transports`、`server/tools`、`server/resources`、
> `client/elicitation` 与 `schema/2026-07-28/schema.ts`）、`crates.io` 的 `rmcp`、官方
> `modelcontextprotocol/rust-sdk`。DSH 侧核实的是本机安装的
> `/usr/lib/node_modules/@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-mcp-client/`（`README.zh.md`
> 与 `lib/index.js`）与同级的 `dsh-mcp-resources/`。

## 结论摘要

1. **规范当前版本是 `2026-07-28`，而它把 MCP 改成了「无状态、每请求 `_meta`」**：删掉
   `initialize` / `notifications/initialized` 握手与协议级 session，新增 `server/discover`，
   并把 server 主动发起的请求改成 MRTR（结果里回 `resultType: "input_required"` +
   `inputRequests`，client 带 `inputResponses` 重试）。旧版（`2025-11-25` 及更早）仍用
   `initialize`，一个 client 要么选一代、要么做探测与回退。（规范 `changelog` 的 Major changes
   第 1、2、3、7 条）
2. **六类原语在 `2026-07-28` 的处境分三档**：tools / resources / prompts 是 server 侧常规能力；
   elicitation 是 client 侧常规能力；**sampling 与 roots（连 logging）在这一版被标为 deprecated**
   （SEP-2577，最早移除时间「2027-07-28 当天或之后发布的第一个修订版」）。也就是说，接 MCP 时
   「反向请求」这一整块要么按 MRTR 的新形状做，要么干脆不做。（规范 `deprecated` 页的表格）
3. **「工具列表会变」在规范里一直有位置**：`notifications/tools/list_changed` + server capability
   的 `tools.listChanged`（`prompts` / `resources` 同构，另有 `notifications/resources/updated`）。
   `2026-07-28` 把这些通知收进一条 `subscriptions/listen` 长连接的 opt-in 过滤条件
   （`toolsListChanged` / `promptsListChanged` / `resourcesListChanged` / `resourceSubscriptions`）。
   这与 fs-agent「工具表组装后不再变化」是正面冲突，不是边角料。
4. **MCP 的 tool 确实有 `annotations`，但撑不起 `Effect`**：`ToolAnnotations` 有 `title`、
   `readOnlyHint`（默认 `false`）、`destructiveHint`（默认 `true`）、`idempotentHint`、
   `openWorldHint`，而规范原文明说「all properties in `ToolAnnotations` are **hints**」并且
   「Clients should never make tool use decisions based on `ToolAnnotations` received from
   untrusted servers」。fs-agent 的 `Effect` 是调度器与权限门共用的**判据**，不是提示。
5. **DSH 有一份可直接对照的做法**：一台 server 一个插件实例、工具名
   `mcp__<serverName>__<rawName>`、在 harness 首个轮次之前完成发现并原子换「代」、server 指令
   作为系统提示词的一个 section；默认不启用（不写配置项就没有实例）；stdio 由 SDK 做
   `versionNegotiation: "auto"`（先起临时进程探测，再起正式进程）；子进程环境先按
   `/KEY|PASSWORD|SECRET|TOKEN/i` 与 `DSH_*` 清洗。它**不**支持 MCP prompts，`capabilities`
   是空对象（不声明 sampling / roots / elicitation）。
6. **Rust 侧有官方 `rmcp`（3.5.0，Apache-2.0，2026-09-28 发布，tokio 原生）**，支持 stdio /
   child-process / Streamable HTTP，自带 discover 与 legacy 两条生命周期与 MRTR 自动驱动，
   且**明确不提供** legacy HTTP+SSE。仓库现在 `Cargo.lock` 里零命中任何 MCP 相关包，引入是
   纯新增依赖。

## 1. MCP 规范要点（六类原语 · 传输 · 握手 · 变更通知）

### 1.1 版本与谱系

当前最新版是 **`2026-07-28`**（规范首页左上角的 `Version 2026-07-28 (latest)`），上一版是
`2025-11-25`（`changelog` 首句：「changes made to the Model Context Protocol (MCP) specification
since the previous revision, 2025-11-25」）。更早的修订版在 `deprecated` 页被点名：`2024-11-05`
（HTTP+SSE 传输的出处）、`2025-03-26`（HTTP+SSE 被 Streamable HTTP 取代的那一版）、
`2025-06-18`（本仓库 `docs/research/coding-agent-features.md:416` 引用的就是这一版）。
`2026-07-28` 不是小改：`changelog` 的 Major changes 有 9 条，其中第 1、2、3 条合起来等于
「协议级 session 与初始化握手整个拿掉」。

> 出处：<https://modelcontextprotocol.io/specification/2026-07-28> ·
> <https://modelcontextprotocol.io/specification/2026-07-28/changelog> ·
> <https://modelcontextprotocol.io/specification/2026-07-28/deprecated>

### 1.2 六类原语

规范首页把 server 提供的能力列为 Resources / Prompts / Tools，把 client 提供的能力列为
Elicitation；但左侧 **Client Features** 导航仍有 Roots / Sampling / Elicitation 三页，其中
Roots 与 Sampling 已在 `2026-07-28` 被标注 deprecated。六类原语的全貌如下（「发起方」按规范
的说法，`2026-07-28` 起 server 不再主动发 JSON-RPC 请求，见 1.3）：

| 原语 | 谁提供 / 谁发起 | 语义 | 主要方法 | `2026-07-28` 状态 |
|---|---|---|---|---|
| Tools | server → client，**model-controlled** | 模型可发现并调用的函数 | `tools/list`、`tools/call` | 常规 |
| Resources | server → client，**application-driven** | 用 URI 标识的数据（文件、DB schema、API 响应） | `resources/list`、`resources/templates/list`、`resources/read` | 常规 |
| Prompts | server → client，**user-controlled** | 带参数的消息 / 工作流模板 | `prompts/list`、`prompts/get` | 常规 |
| Sampling | client 提供，server 请求 | 请 client 用它的 LLM 跑一次补全 | `sampling/createMessage` | **deprecated（SEP-2577）** |
| Roots | client 提供，server 请求 | 告诉 server 它该操作哪些目录 / 文件 | `roots/list` | **deprecated（SEP-2577）** |
| Elicitation | client 提供，server 请求 | 让 server 在中途向用户要输入 | `elicitation/create` | 常规 |

出处：规范首页的 Features / Additional Utilities；各原语的 spec 页
（<https://modelcontextprotocol.io/specification/2026-07-28/server/tools>、
`.../server/resources`、`.../client/elicitation`、`.../client/sampling`、`.../client/roots`）；
`changelog` 的 Deprecated 第 1 条与 `deprecated` 页表格。

**capability 协商**落在 `ClientCapabilities` / `ServerCapabilities` 上。server 侧三类能力的
形状很直白：`prompts.listChanged?`、`resources.subscribe?` 与 `resources.listChanged?`、
`tools.listChanged?`（schema.ts 的 `ServerCapabilities`）；两侧还新增了 `extensions` 字段，
键是扩展标识（例如 `"io.modelcontextprotocol/tasks"`）。`notifications/roots/list_changed`
也已在 `2026-07-28` 被移除（`changelog` Major changes 第 5 条）。

### 1.3 传输

规范把传输定义成一种 **binding**：「Protocol semantics are identical on every transport.」
两个标准绑定：

- **stdio**：在 client 启动的子进程的标准流上跑 **newline-delimited** 的 JSON-RPC 消息
  （`basic/transports` 的 Messages 与绑定列表）。它也是自定义字节流传输应当复用的 framing。
- **Streamable HTTP**：每条消息是对**单一 MCP endpoint** 的一次 HTTP POST；回复要么是一个
  JSON 对象、要么是 request-scoped 的 SSE 流。

`2026-07-28` 对 HTTP 做了三处删减：**移除 `Mcp-Session-Id` 与协议级 session**（列表端点
不再逐连接变化）；**移除 HTTP GET 端点与 `resources/subscribe` / `resources/unsubscribe`**，
改用 `subscriptions/listen`；**移除 SSE 的可恢复性**（`Last-Event-ID` 与 SSE event id 都没了，
断掉的响应流会丢掉在途请求，client MUST 用新 request id 重发）。另外强制 Streamable HTTP
POST 带 `Mcp-Method` / `Mcp-Name` 标准头，tool 参数可以经 `x-mcp-header` 提升成自定义头
（`Mcp-Param-*`）。

**取消**在两条绑定上不同：stdio 上 client 发 `notifications/cancelled`；Streamable HTTP 上
client **关闭该请求的响应流**（`basic/transports` 的 Cancellation 一节）。stdio 上 server 只在
终止 `subscriptions/listen` 流时发这条通知，不得拿它取消别的请求（schema.ts 的
`CancelledNotification` 注释）。

**每请求元数据**是 `2026-07-28` 的核心：`_meta` 里必须带
`io.modelcontextprotocol/protocolVersion`（必需）、`io.modelcontextprotocol/clientCapabilities`
（必需），SHOULD 带 `io.modelcontextprotocol/clientInfo`；server 在结果的 `_meta` 里 SHOULD 带
`io.modelcontextprotocol/serverInfo`；日志等级改由每请求的 `io.modelcontextprotocol/logLevel`
控制（旧版是 `logging/setLevel` RPC）。HTTP 上这些值会被镜像进 `MCP-Protocol-Version` 等头，
体是真相源、头与体不一致要报 `HeaderMismatchError`（`-32020`）。进度用 `_meta.progressToken`。

> 出处：<https://modelcontextprotocol.io/specification/2026-07-28/basic/transports> ·
> `changelog` 的 Major changes 第 1、4、5、9 条与 Minor changes 第 1、2、4、5、8 条 ·
> `schema.ts` 的 `RequestMetaObject`、`ResultMetaObject`、`CancelledNotification`、
> `HEADER_MISMATCH` / `MISSING_REQUIRED_CLIENT_CAPABILITY` / `UNSUPPORTED_PROTOCOL_VERSION`。

### 1.4 初始化握手（以及它在 `2026-07-28` 的消失）

- **旧版（`2025-11-25` 及更早）**：client 发 `initialize`，双方交换 `protocolVersion` 与
  capability，server 回 `serverInfo`，client 再发 `notifications/initialized`。
- **`2026-07-28`**：握手没了。每一次请求自己带协议版本与 client capabilities；版本不匹配时
  server 返回 `UnsupportedProtocolVersionError`（`-32022`，`data.supported` 列出它支持哪些版本）；
  缺 client capability 时返回 `MissingRequiredClientCapabilityError`（`-32021`）。
- **`server/discover`**：servers **MUST** 实现。它返回 `supportedVersions`、`capabilities`、
  `instructions`（自然语言指引，规范说可以放进 LLM 的系统提示词里帮它用好这些工具）以及缓存的
  `ttlMs` / `cacheScope`。client **MAY** 在其它请求之前调它做版本选择，或把它当成 STDIO 上的
  **向后兼容探测**。

「回退」因此是一个真实要写的机制：先试 discover，对端不认这条方法（或超时不答）就退回
`initialize`。官方 Rust SDK 把这个选择做成了显式的生命周期模式（见第 5 节）。

> 出处：`changelog` 的 Major changes 第 2、3 条；`basic/transports` 的 Backward Compatibility
> 一节；`schema.ts` 的 `DiscoverRequest` / `DiscoverResult`（`DiscoverResult.instructions` 的注释
> 原文就举了「included in a system prompt」这个用法）。

### 1.5 进度、取消与「工具列表会变」

- **进度**：`notifications/progress`，请求侧用 `_meta.progressToken` 声明要收；接收方没有义务
  一定发。`changelog` 明说 `notifications/progress` 与 `notifications/message` 这类**请求作用域**
  通知继续走「该请求自己的响应流」，不走 `subscriptions/listen`。
- **取消**：见 1.3。
- **列表变更**：`notifications/tools/list_changed`、`notifications/prompts/list_changed`、
  `notifications/resources/list_changed`，以及针对单个资源的 `notifications/resources/updated`
  （schema.ts 的 `ServerNotification` 联合类型把这四条都列了）。server 用 capability 里的
  `listChanged: true` 宣告它会发这些通知。
- **`2026-07-28` 的收口**：`subscriptions/listen` 是一条长期 POST 响应流，client 用
  `SubscriptionFilter` **逐类 opt-in**（`toolsListChanged`、`promptsListChanged`、
  `resourcesListChanged`、`resourceSubscriptions`），server 必须只发 client 要的那些类型，
  并在每条通知的 `_meta` 里带 `io.modelcontextprotocol/subscriptionId`。
- **缓存提示**：`tools/list`、`prompts/list`、`resources/list`、`resources/read`、
  `resources/templates/list` 的结果要带 `ttlMs`（新鲜度，毫秒）与 `cacheScope`
  （`"public"` / `"private"`）；规范还 SHOULD server 按**确定性顺序**返回 `tools/list`，理由是
  「enable client-side caching and improve LLM prompt cache hit rates」。这两条对 fs-agent 的
  前缀缓存讨论是直接相关的。
- **结果类型**：所有结果都带必需的 `resultType`；普通结果是 `"complete"`，MRTR 的中间结果是
  `"input_required"`。旧版 server 不带这个字段时，client **MUST** 当成 `"complete"`。

> 出处：`changelog` 的 Major changes 第 4、7、8 条与 Minor changes 第 3、5 条；
> `schema.ts` 的 `ToolListChangedNotification`、`SubscriptionFilter`、
> `Result.resultType`、`InputRequiredResult`。

### 1.6 `Tool` 与 `ToolAnnotations`（回答「annotations 有没有 readOnlyHint」）

`Tool` 对象（schema.ts 的 `Tool extends BaseMetadata, Icons`）有：`name`、`title`、
`description`、`inputSchema`（根必须是 `type: "object"`，其余可用 JSON Schema 2020-12 的任意
关键字，包括 `$ref`、`oneOf`、`if/then/else`）、`outputSchema`（`2026-07-28` 起可以是任意
JSON Schema 类型）、`icons`、以及 **`annotations?: ToolAnnotations`**。

`ToolAnnotations` 的字段与默认值（原文）：

- `title?: string` —— 给人看的标题。
- `readOnlyHint?: boolean` —— 「If true, the tool does not modify its environment.」**默认 `false`**。
- `destructiveHint?: boolean` —— 「If true, the tool may perform destructive updates…If false,
  the tool performs only additive updates.」**默认 `true`**，且只在 `readOnlyHint == false` 时有意义。
- `idempotentHint?: boolean` —— 同样只在 `readOnlyHint == false` 时有意义。
- `openWorldHint?: boolean` —— 是否与开放世界（外部实体）交互。

**关键的一句在定义之前**：「NOTE: all properties in `ToolAnnotations` are **hints**. They are not
guaranteed to provide a faithful description of tool behavior (including descriptive properties
like `title`). Clients should never make tool use decisions based on `ToolAnnotations` received
from untrusted servers.」

另外两条与安全直接相关的规范事实：`tools/call` 的结果用 `isError` 表示「工具跑了但失败了」；
Tools 页的 User Interaction Model 明确写「for trust & safety and security, there **SHOULD**
always be a human in the loop with the ability to deny tool invocations」，并让应用在调用工具时
给出清晰的视觉指示与确认提示。本仓库的上游引文里也记过这一条
（`docs/research/coding-agent-features.md:259`：「MCP 规范层面要求：host MUST obtain explicit user
consent before invoking any tool，并且工具描述（annotation）本身要当作不可信内容」）。

> 出处：`schema.ts` 的 `ToolAnnotations`、`Tool`、`CallToolResult` 定义
> （<https://raw.githubusercontent.com/modelcontextprotocol/specification/main/schema/2026-07-28/schema.ts>）；
> <https://modelcontextprotocol.io/specification/2026-07-28/server/tools>。

### 1.7 Resources 与 Elicitation 的数据形状

- **Resource** 由 URI 唯一标识，URI 方案不限于文件：规范给了 `https://`、`file://`、`git://`
  与自定义 scheme 的说明。`Resource` 带 `name`、`title`、`description`、`mimeType`、
  `size`、`icons`、`annotations`（`audience` / `priority` / `lastModified`）；内容是 text 或
  base64 blob。`resources/templates/list` 提供 URI 模板（例如 `users://{user_id}/profile`），
  client 展开成具体 URI 后再 `resources/read`。资源选择是 **application-driven**——规范不规定
  它怎么进入模型上下文。
- **Elicitation** 有两种模式：**form mode** 用 `requestedSchema`，「A restricted subset of JSON
  Schema. Only top-level properties are allowed, without nesting.」（只允许 `StringSchema` /
  `NumberSchema` / `BooleanSchema` / `EnumSchema` 这些 primitive），结果是
  `ElicitResult.action ∈ "accept" | "decline" | "cancel"` 加 `content`；**URL mode** 给一个
  `url` 让用户到浏览器里完成，用于**不得经过 MCP client** 的敏感交互。规范的安全条款是
  「Servers **MUST NOT** use form mode elicitation to request sensitive information such as
  passwords, API keys, access tokens, or payment credentials」，这类必须走 URL mode；client
  **MUST** 明确显示是哪个 server 在问、提供拒绝与取消、form mode 下允许用户改答案后再发。

> 出处：<https://modelcontextprotocol.io/specification/2026-07-28/server/resources> ·
> <https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation> ·
> `schema.ts` 的 `Resource`、`ResourceContents`、`Annotations`、`ElicitRequestFormParams`、
> `ElicitRequestURLParams`、`ElicitResult`、`PrimitiveSchemaDefinition`。

## 2. DSH 的做法

DSH 把 MCP 拆成两个包：`@deepseek-ai/dsh-mcp-client`（连接、工具、指令）与
`@deepseek-ai/dsh-mcp-resources`（按需读资源的三个共享工具）。下面这张表是
`dsh-mcp-client/README.zh.md` 与 `dsh-mcp-client/lib/index.js` 的摘录；凡是我从代码推断而非
README 明说的，都标了「推断」。

| 维度 | DSH 的做法 | 出处 |
|---|---|---|
| 配置形状 | **每台 server 一条 `cordis.yml` 配置项**，`name: '@deepseek-ai/dsh-mcp-client'` + `config`。一个插件实例连一台 server，多台就写多条 | `dsh-mcp-client/README.zh.md:12`、`:32`；`lib/index.js:748-750` |
| stdio 字段 | `transport: stdio`（必填）、`serverName`（必填，`[A-Za-z0-9_-]{1,32}`、同作用域唯一）、`command`、`args`、`env`、`cwd` | `README.zh.md:55-59`；`lib/index.js:780-790` |
| HTTP 字段 | `transport: streamable-http`、`serverName`、`url`、`headers` | `README.zh.md:57-60`；`lib/index.js:791-800` |
| 其余字段 | `toolCallTimeoutMs` 默认 `60000`、`maxInstructionBytes` 默认 `32768`、`failOnStartupError` 默认 `false`、`reconnect.{enabled,initialDelayMs,maxDelayMs,maxAttempts}` | `README.zh.md:61-67`；`lib/index.js:774-800` |
| **默认不启用** | 没有任何 `enabled` 布尔：**不写配置项就没有插件实例**，于是没有连接、没有工具、没有提示词文本。「调用方作用域为空时，不添加 MCP 工具或提示词文本」 | `README.zh.md:12`；共享工具侧同义：`dsh-mcp-resources/README.zh.md:12`、`:34` |
| 传输选择与回退 | `transport` 必填、二选一（`stdio` / `streamable-http`）；**协议版本**由 SDK 负责：「官方 SDK 优先选择可用的 2026-07-28 协议，并回退到支持的旧版协议」 | `README.zh.md:28`；代码里是 `versionNegotiation: { mode: "auto" }`，`lib/index.js:596-609` |
| stdio 的「临时探测进程」 | README 原文：「stdio 协商会先启动临时探测进程，再启动实际服务进程」。含义**推断**为：`2026-07-28` 与旧版的生命周期不同（前者要 `server/discover`，后者要 `initialize`），必须在真正建立会话之前先问一次对端是哪一代；对 stdio 而言这一次问话必须真的把子进程起起来，所以先起临时进程、探测完关掉，再起正式的服务进程。DSH 的 README 与 `lib/index.js` 都没有展开这一步的实现——它在 MCP SDK 里（`@modelcontextprotocol/client`） | `README.zh.md:28`；`lib/index.js:601`、`:596-609` |
| 工具命名 | `mcp__<serverName>__<rawName>`，例如 `mcp__github__create_issue`；要满足 DeepSeek 函数名约定（≤64 字符、`[A-Za-z0-9_-]`），有损规范化时把 `(serverName, rawName)` 的 SHA-256 取 12 位十六进制接在后面，保证不同身份不会折叠；**rawName 只上线（`tools/call`），公开名从不解析回原始名**；两台 server 可以各有 `search`，以 `mcp__github__search` / `mcp__web__search` 共存 | `README.zh.md:75`、`:107-109`；`lib/index.js:68-72`、`:96-102` |
| 重复与冲突 | 两条配置项用同一 `serverName`：加载时明确报错；server 在 `tools/list` 里重复列同一工具：整份列表按无效拒绝、上一代工具保持可用；更新与已有工具名冲突：整代回滚，绝不出现该 server 的部分工具集 | `README.zh.md:78-81`；`lib/index.js:127-161` |
| **工具进 `ctx.tools` 的时机** | `apply()` 里 `startConnection()` → `connectGeneration(true)` → `await generation.connect(transport)` → 排队做初次 `syncTools`（原子「先取齐、再交换」两阶段），然后 `await connection.ready`。行为上是「harness 开始首个轮次之前」工具就位；更新时先 fetch 新一代、成功后才 dispose 旧一代并注册新一代，失败保留旧一代 | `README.zh.md:91`、`:110`、`:128-130`；`lib/index.js:127-161`、`:517-525`、`:657`、`:677-683`、`:831` |
| 服务器指令进系统提示词 | `ctx.inject(["systemPrompt"], …)` 注册一个 section：`name: "mcp:<server>"`、`order: systemPrompt.getSectionOrder("MCP_SERVERS")`、`interpolate: false`、`text: () => connection.instructions()`；文本本身是 `### MCP server: <serverName>\n\n<instructions>`；空白指令不加段落；超过 `maxInstructionBytes`（默认 32768，含归属头）让连接失败 | `lib/index.js:735-742`、`:654-656`；`README.zh.md:62`、`:192` |
| 认证 | stdio 走配置的 `env`（README 示例 `GITHUB_TOKEN: !!js process.env.GITHUB_TOKEN`）；streamable-http 走配置的 `headers`（示例 `Authorization: !!js '`Bearer ${process.env.MCP_TOKEN}`'`）。即**凭据由部署方的配置表达式从环境里取**，包自己不存 token | `README.zh.md:42-52`；`lib/index.js:40-47` |
| 环境清洗（stdio） | 子进程环境以 `scrubbedParentEnv()` 为基座：**删除匹配 `/KEY\|PASSWORD\|SECRET\|TOKEN/i` 的环境名与所有 `DSH_*` 名**，再合并显式 `env`（显式覆盖保留）。实际 spawn 由 MCP SDK 负责，本包只共享清洗定义 | `README.zh.md:136-138`；`lib/index.js:26-31` |
| 进程生命周期 | 连接监督器拥有一「代」client/transport；所有同步（初始、通知、重连）串行到同一条队列，避免两次同步交错；dispose 会取消待重连、关闭传输、等尝试与队列停稳，再注销当前代 | `README.zh.md:128-130`；`lib/index.js:481-717` |
| 崩溃与重连 | 断线自动重连：延迟从 `500 ms` 起逐次翻倍、上限 `30000 ms`；每次中断共享一个尝试预算，连续失败 `maxAttempts`（默认 `10`）后**注销该 server 的工具并停止重连**，直到重载配置或重启；连接持续超过 `maxDelayMs` 会重置预算。中断期间最后已知的工具仍列出、但调用失败。`reconnect.enabled: false` 可关 | `README.zh.md:64-67`、`:93`；`lib/index.js:557-584`、`:564` |
| 超时 | 每次 `tools/call` 或资源请求 `toolCallTimeoutMs`（默认 60000）；连接与发现**没有单独超时**，用 SDK 默认的 60 秒请求超时与页数上限；关闭确认另有 5000 ms 的 `GENERATION_CLOSE_TIMEOUT_MS` | `README.zh.md:61`、`:210`；`lib/index.js:442` |
| 错误码 / 错误语义 | MCP 的 `isError` 结果在图片持久化之前**抛出**（模型不会看到虚假的成功）；协议层要求基于任务的工具直接抛「不支持」；格式错误的返回由 SDK 的类型校验拒绝 | `README.zh.md:85`、`:134`、`:213-214`；`lib/index.js:229-236` |
| **对前缀缓存的影响** | 「已发现工具集合及其 schema 不变时，工具定义前缀保持稳定。增加、移除、重命名或更改工具的重新同步会替换定义，并可能使从第一个变化的 schema token 起的复用失效；恢复未变列表的重连会生成完全相同的定义，前缀保持稳定。」服务器指令变化则改变下一次组装的系统消息及其可复用前缀 | `README.zh.md:170-172`、`:198-200` |
| 工具结果投影 | 规范值保留完整 MCP JSON 块与 `structuredContent`；文本块按序合并；`resource_link` 变成文本 `Resource link: <name> (<uri>)`；图片在模型确有能力时落成持久附件，否则降级为诊断文字；音频与嵌入资源只留诊断；未知块给 `[unsupported MCP content type: …]` | `README.zh.md:85-87`、`:178-182`、`:212`；`lib/index.js:361-414` |
| 资源（另一个包） | 三个共享工具：`list_mcp_resources`、`list_mcp_resource_templates`、`read_mcp_resource`，都要求显式 `server` 参数，**调用时才读**；没有已配置 server 时，工具与提示词段落都不存在；**不支持资源订阅与更新通知**；二进制不投影成原生图片/音频，只给 base64 长度说明 | `dsh-mcp-resources/README.zh.md:12`、`:34-40`、`:92`、`:122-126` |
| prompts / sampling / roots / elicitation | **MCP prompts 模板不受支持**；client `capabilities: {}`——不声明 sampling、roots、elicitation；README 也没有 elicitation 的段落 | `README.zh.md:12`、`:209`、`:227`；`lib/index.js:600` |

DSH 值得单独抄的两条**思路**（不是代码）：命名是 `(serverName, rawName)` 的纯函数、`serverName`
是**本地配置**而绝不采用远程 `serverInfo.name`（远程名不可信、跨部署不唯一、升级会变，
`README.zh.md:107`）；以及「要么完整世代、要么没有」的原子交换（`:110`）。

## 3. 上游八个实现怎么做

这一节全部来自 `docs/research/`（本仓库对上游的一手引文，一个字未改）。`docs/research/README.md`
自己声明「这些是材料，不是结论」「笔记不加维护」。下面的行号就是那些文件的行号。

| 实现 | 传输 | 配置形状 | 工具进入上下文的时机 | 权限 / 审批 | 资源与提示词 | server 挂了 |
|---|---|---|---|---|---|---|
| Claude Code | `http`（`streamable-http` 是别名）、`sse`、`stdio`、`ws`；v2.1.265 起 `--transport http` 先试 HTTP、不被接受时切 SSE | `.mcp.json` / `claude mcp add` 系列命令；`--mcp-config`、`--strict-mcp-config`；「有 `url` 但没有 `type` 的条目是配置错误，因为没有 `type` 会被读成 stdio server」 | **默认只列工具名、schema 按需加载**；MCP 工具可以被 **tool search** 推迟加载、`alwaysLoad` 豁免某台 server——官方明说这个选择直接影响 prompt-cache | 权限规则里工具名支持 `mcp__*`；并行判定只看 `readOnlyHint`（「Read-only tools (like Read, Glob, Grep, and MCP tools marked as read-only) can run concurrently」）；`--permission-prompt-tool` 可以是一个 MCP 工具来回答权限提示 | MCP 工具名 `mcp__<server>__<tool>`，插件再套一层 `mcp__plugin_<plugin>_<server>__<tool>` | 未在这份笔记里记录 |
| Codex | **推断而非声明**：`command` → stdio，`url` → streamable HTTP；**不支持 SSE** | `mcp_servers.<id>`：`command`/`args`/`env`/`env_vars`/`cwd`/`url`/`auth`(`oauth`\|`chatgpt`)/`bearer_token_env_var`/`http_headers`/`enabled`/`required`/`startup_timeout_sec`(默认 10)/`tool_timeout_sec`(默认 60)/`enabled_tools`/`disabled_tools`/`default_tools_approval_mode`/per-tool `tools.<tool>.approval_mode`/`scopes`/`supports_parallel_tool_calls`；全局 `mcp_optional_startup_grace_ms` 默认 1000 | 全量进工具表（MCP 工具与 `tool_search`、资源工具并列在 `spec_plan.rs` 的清单里） | `default`/`auto_edit`/`plan` 三档与 `granular`（`mcp_elicitations` 是 `GranularApprovalConfig` 的一个字段）；MCP 工具默认**可以**并行（`tool_supports_parallel` 的显式加入者） | 有 `list_mcp_resources` / `list_mcp_resource_templates` / `read_mcp_resource` 三个资源工具 | **`required = true` 的 server 初始化失败会让 `codex exec` 直接报错退出**；事件流里 `mcp_tool_call` 是一等 item 类型 |
| Gemini CLI | 三种：stdio(`command`)、SSE(`url`)、streamable HTTP(`httpUrl`) | 顶层 `mcpServers` map；per-server `args`/`headers`/`env`(`$VAR`/`${VAR}`/`%VAR%`)/`cwd`/`timeout`(默认 600000 ms)/`trust`/`includeTools`/`excludeTools`(排除优先)/`targetAudience`；OAuth 走 `oauth.*`，token 落 `~/.gemini/mcp-oauth-tokens.json`，`/mcp auth <server>` | 全量；MCP 工具与内建并列 | **policy engine** 的规则字段里有 `mcpName` 与 `toolAnnotations`；`deny` 会把工具整个移出模型工具集 | 有 `read_mcp_resource` / `list_mcp_resources`；subagent frontmatter 可以带 `mcpServers`，`tools` 支持 `mcp_*` | 未在这份笔记里记录 |
| Cline | local(STDIO) 与 remote(`type: "streamableHttp"` \| `"sse"`)；**省略 `type` 默认是 legacy `sse`** | 顶层 `mcpServers`；local：`command`/`args`/`env`/`disabled`/`autoApprove`(工具名数组)；remote：`type`/`url`/`headers`/`disabled`/`autoApprove`。配置文件三处官方页说法冲突：`~/.cline/mcp.json`（CLI）、`.cline/mcp.json`（工具参考与 CLI 参考）、`~/.cline/data/settings/cline_mcp_settings.json`（配置页） | 「loaded alongside built-ins」 | 全局开关「Use MCP servers」（机器键 `use_mcp`）+ 每 server 的 `autoApprove` 数组；文档建议「Limit `autoApprove` to safe tools」 | 资源在「Use MCP servers」这一类之下；工具集含 `use_mcp_tool` / `access_mcp_resource` | 遥测有 `task.mcp_tool_called`（started/success/error、tool_name、server_name）；文档没写失败策略 |
| Continue | 未在这份笔记里分传输 | 未在这份笔记里写 MCP server 配置；上下文提供者体系已废弃，官方迁移建议就是「用 MCP server」 | 六步 handshake：第 4 步才「calls the tool using built-in functionality or the MCP server that offers that particular tool」 | 第 3 步是「The user gives permission. This step is skipped if the policy for that tool is set to `Automatic`」 | 工具身份是 URI `mcp://<encodeURIComponent(mcpId)>/<encodeURIComponent(toolName)>`，参数按 JSON schema 强转（`coerceArgsToSchema`）；非 text/resource 内容产生 "MCP Item Error" 上下文项 | 超时来自 `client.options.timeout`；`isError === true` 时抛出序列化后的内容 |
| aider | **没有 MCP**（源码树大小写不敏感 grep `mcp` 只命中一个测试 fixture，「there is no MCP client code」） | — | — | — | — | — |
| OpenHands | V0：`MCPConfig` 有 `sse_servers` / `stdio_servers` / `shttp_servers`，从 `[mcp]` TOML 段配；V1 SDK：`mcp_config` 字典按 **FastMCP client 配置格式**，支持 `stdio` / `http` / `streamable-http` / `sse`，含工具过滤与 OAuth | V0 还支持 microagent frontmatter 的 `mcp_tools`，但 0.62.0 那条路径**只允许 stdio** server；V1 的 SDK 设置形状**不是** `.mcp.json` 的 wrapper 格式（插件文件才用 wrapper） | 全量 | Tool framework 里有 `ToolAnnotations`（明说就是「MCP-spec hints: readOnly/destructive/idempotent/openWorld」） | 未在这份笔记里单列资源工具 | 未在这份笔记里记录 |
| opencode | local（`"type": "local"`, `command: [...]`, `cwd`, `environment`, `enabled`, `timeout` 默认 5000 ms）与 remote（`"type": "remote"`, `url`, `headers`, `enabled`），含 OAuth（自动与预注册） | `opencode mcp add\|list\|auth\|logout\|debug <name>`；**per-agent 启用**：全局关掉、在 agent 的 `tools` 块里再打开 | 全量 | 权限按 agent 配，`tools` 的 glob 可以整台打开/关掉（MCP 工具名带 server 前缀，所以 `"mymcpservername_*"` 关一整台，`*`/`?` 都支持） | 未在这份笔记里单列资源 | 未在这份笔记里记录 |
| goose | extension 就是 MCP server（内建的、外部的、自定义的统一走 MCP） | 未在这份笔记里给字段表 | 全量 | 四档 `auto`/`approve`/`smart_approve`/`chat`；另有 `permission.yaml` 的 `always_allow`/`ask_before`/`never_allow`（按 `user` 与 `smart_approve` 两个主体分别配）；`GOOSE_ALLOWLIST` 限制可用 extension | 未在这份笔记里单列资源 | **错误作为 tool response 回给模型而不是中断循环**（「invalid JSON, missing tools, etc. are sent back to the model as tool responses」）；CI 有 **MCP Conformance**（带按 MCP 规范版本分的 checked-in expected-failure 基线）与每日 **Model Tool Call Conformance**（明确不是 PR gate） |

此外，这批材料里还有两条对本次调研直接有用的横向结论：

- **命名约定在事实上统一了**：Amp 与 Claude Code 都用 `mcp__<server>__<tool>`，Anthropic 明确
  建议做 namespacing，理由是「工具一多，模型就选错」
  （`docs/research/coding-agent-features.md:425`）。
- **「工具越多，模型越容易选错」是多家共识**：Amp 官方建议**不要把 MCP server 全局挂着**，
  而应打包进 skill，因为 skill 提供的工具在 skill 加载前对模型不可见
  （`coding-agent-features.md:423`；`notes/claude-code-amp.md:1122-1127`）。同一份横评给 MCP
  的标签是「🔷（生态上接近 ✅，自用可砍）」（`coding-agent-features.md:436`），而在「可以砍掉
  什么」里排第 2（`:759`）。

## 4. fs-agent 侧的撞点（逐条给代码位置）

这一节是本次调研的重点。每一条都先给现状的确切位置，再给「接 MCP 会撞到哪里」。

### 4.1 工具表不变 vs 运行时发现

**现状。**

- 工具表只有一个组装点：`src/tools/mod.rs:104-110` 的 `with_dynamic(declarations, can_ask)`，
  它调 `builtin(can_ask)`（`:78-94`）后逐条 `registry.register(Box::new(CustomTool::new(...)))`。
  模块顶部注释把话说死了：「注册表是会话携带的运行时值（绝不是全局静态），所以动态工具有了
  挂载点，而**工具表在前缀缓存的整个生命周期里保持不变**」（`src/tools/mod.rs:1-10`）。
- 调用点三处，都是 `tools::with_web(tools::with_dynamic(...))`：`src/cli.rs:386-389`（交互式）、
  `src/cli.rs:705-708`（另一条交互式路径）、`src/cli.rs:2221-2224`（headless 探针）；
  另有 `src/cli.rs:4021`、`:4107`、`:4258` 用 `tools::builtin(false)`。
- 建完就冻结：`Registry` 变成只读共享值 `Arc<Registry>`（`src/lib.rs:260`），此后每个会话拿
  `Arc::clone`（`src/lib.rs:285`）。
- 表进请求的形状是 `Registry::specs()`：`src/tools/registry.rs:88-93`，注释原话「稳定顺序要紧：
  **工具数组是缓存前缀的一部分**」；`Registry::for_executor()`（`:70-78`）是「已组装工具表的一个
  纯函数」快照，只为每个执行者过滤 `delegable()`。
- spec 里钉住这条的地方：`.scratch/fs-agent-v1/spec.md:448`（「**可见性 = 启动即全局可见、
  组装期固定**（否决「挂在 skill 上」：`tools` 数组属于**前缀**，中途增删会废掉前缀缓存）」）、
  `:426`（「**deny 不从上下文移除工具**（移除会废掉前缀缓存）——要「不存在」就在组装期不注册」）、
  `:119`（愿望 70：「动态工具在启动时全局可见、组装期固定，以便工具表不中途变化、前缀缓存不被
  废掉」）、`:138`（愿望 85）、`:374`（「把工具打包进 skill」不做，理由就是这条冲突）；
  `src/tools/mod.rs:112-118` 把 `with_web` 也声明成组装期的一步（「不是运行期开关……改它要重开
  会话」）。

**MCP 撞上来的是哪一条。** 规范允许 server 在运行时改工具表并通知 client（`1.5` 的
`notifications/tools/list_changed` 与 `subscriptions/listen` 的 `toolsListChanged`）。于是有一个
二选一，而且它不是一个实现细节：

- **A. 组装期全量发现一次、此后冻结。** 保住现有全部不变量；代价是 server 运行中换工具表
  时 fs-agent 看不到（要么忽略通知、要么把它当成「需要重开会话」的事实记进流）。DSH 没有选
  这一条——它做自动重同步。
- **B. 允许运行时换表。** 那就要改的东西是具体的：`Registry` 现在是 `BTreeMap<String, Arc<dyn Tool>>`
  加一个 `register`（`src/tools/registry.rs:46-59`），共享出去的是 `Arc<Registry>`（`src/lib.rs:260`、
  `:285`）——要换表就得给它一个可变 / 可替换的形状（`Arc<RwLock<Registry>>` 之类），而这会
  一路传到 `Registry::specs()` 的调用方、`for_executor()` 的快照语义、以及 `Session` 里那份
  `Arc<Registry>`。更要紧的是它直接推翻 `src/tools/mod.rs:1-10` 与 spec.md:448 的一句话。
- **C. 不把 MCP 工具放进 `tools` 数组。** 这是 Claude Code 的 tool search 与 Amp 的「打包进
  skill」那一类做法（`docs/research/notes/claude-code-amp.md:511-513`、
  `coding-agent-features.md:423`）：模型先用一个检索 / 加载工具把外部工具拉进来。fs-agent 现在
  有 `skill`（`src/tools/skill.rs`）这个「只有拿到才进上下文」的先例，但 spec 明确否决了
  「把工具打包进 skill」（spec.md:374），所以这条路要动的是 spec，不是代码。

### 4.2 Effect 与权限：外来工具怎么定 `Effect`

**现状。** `Effect` 只有三类：`ReadOnly` / `WritePaths(Vec<PathBuf>)` / `Exclusive`
（`src/tools/tool.rs:21-33`）。它是 **args 的纯函数**（`Tool::effect`，`tool.rs:206-207`）。它的
语义是「一次已规划调用的**工作区**副作用」（`tool.rs:3-5`），调度器按它分区、权限门按它裁决：
`src/permissions.rs:127-184` 是四档模式的 `stance` 表，其中 `(Mode::Workspace, Effect::Exclusive)`
直接 `Allow`、`(Mode::Workspace, Effect::WritePaths(_))` 按写目标在不在 cwd 内分 `Allow`/`Ask`
（`:154-177`）。规则可以按工具名 glob 覆盖模式缺省（`Scope::Tool`，`permissions.rs:252-271`；
`decide` 在 `:531-600`）。

`CustomTool` 是「外来工具」的现成先例：`src/tools/custom.rs:73-77` 注释写「**永远是 `Exclusive`**
（spec §14）：声明里没有效果类别的字段，所以「这个其实只读」没有地方可说。调度器的保守路径
不需要特例。」代价在同一文件顶部写明：真正只读的动态工具也会被工作区级串行化，且
`read-before-edit` 覆盖不到它。

**MCP 撞上来的是哪一条。** MCP 声明里**有**一个看起来能用的字段，但它不能用：

- `Tool.annotations.readOnlyHint` / `destructiveHint` / `idempotentHint` / `openWorldHint`
  （见 `1.6`）确实存在，而且 `destructiveHint` 默认就是最保守的 `true`。但规范明说它们全是
  **hints**、且「Clients should never make tool use decisions based on `ToolAnnotations` received
  from untrusted servers」。fs-agent 的 `Effect` 不是提示而是**判据**——它决定要不要取工作区级锁
  （`registry.rs:178-185`）、决定权限门的裁决、决定 `read-before-edit` 能不能看见写集合
  （`registry.rs:271-293`）。拿它去映射 hints，等于把一条安全边界交给外部 server 的一句话。
- **三类 `Effect` 够不够表达「外部 server 的工具」？** 我的读法是「不够，但缺口不是第四类，
  而是『工作区之外还有副作用』这件事没有词汇」。`ReadOnly` 的定义是「只读**工作区**，什么都不写」，
  一个查 Jira / 发消息 / 改远端数据库的 MCP 工具在这个词下会被标成 `ReadOnly`，于是被当成可以
  并发跑、不取锁、也不问——这正是 `Effect` 分类想防的形态。反过来把它们一律当 `Exclusive`
  是安全的（`CustomTool` 的先例），代价是工作区级串行 + `read-before-edit` 失效。
- 无论选哪种，权限门那一侧**不需要新词汇**：规则语言已经能按 `Scope::Tool` 的 glob 覆盖
  `mcp__*`（`permissions.rs:252-271`），这与 Amp 的 `mcp__*` 权限 glob 是同一个形状
  （`notes/claude-code-amp.md:976-982`）。所以「外来工具不可管」不成立，「外来工具能否被声明为
  只读」才是缺口。

### 4.3 进程与沙箱：server 进程该不该过 bubblewrap

**现状。**

- fs-agent 只有**一处** spawn 会把 argv 包进沙箱：`src/tools/process.rs:105-111` 的
  `process::run` 第一句就是 `let argv = sandbox.wrap(argv, cwd)?;`，而 `wrap` 是纯函数
  （`src/tools/sandbox.rs:273-324`）：`--ro-bind / /`、`--tmpfs /tmp`、每个可写根一条 `--bind`
  （`sandbox.rs:327-341` 的 `writable_roots`）、可选遮罩、保护路径的 `--ro-bind`、然后 `--`
  接原 argv。
- **沙箱不管网络**：`docs/sandbox.md:22` 的表格一行就是「**不管** | **网络**——`curl` 带着 key
  出去这一层拦不住」，`docs/sandbox.md:92` 还写着「**网络隔离**：`--unshare-net`、域名白名单
  代理都不做」。代码印证：`sandbox.rs:294-303` 只 `--unshare-user` / `--unshare-pid` /
  `--unshare-ipc` / `--unshare-uts`，**没有** `--unshare-net`。
- **子进程继承环境**：`process::run` 直接 `Command::new(program).args(rest).current_dir(cwd)`
  （`process.rs:116-129`），没有 `env_clear()`、没有白名单；stdin 是 null、有自己进程组、
  `kill_on_drop`。而 `bash` 是 `bash -lc`（`src/tools/bash.rs:56-57`，工具描述 `:69` 也这么说），
  `-l` 意味着登录 shell 会读 profile。所以一个经 `process::run` 起的东西，拿到的是 fs-agent
  进程的环境 **加上** 登录 shell 可能带来的那些。
- 超时与进程树：`process.rs:150-206` 的 `ProcessGroup` + `process_group(0)`（`:127`）+
  `killpg`（`:251-258`），注释写明「超时不是调用提前结束的唯一方式：用户取消时循环会把进行中的
  工具丢掉（spec §6）。drop 时杀」。

**MCP 撞上来的是哪几条。**

- **过不过 `wrap`。** 如果 MCP server 由 `process::run` 起，它自动获得 `--ro-bind / /` 与「只有
  会话 cwd + `[sandbox] writable_roots` 可写」的边界；如果由 rmcp 的 `TokioChildProcess` 或自建
  transport 直接 spawn，它**绕过** `wrap`，也就绕过这一层。后者正是 Codex 的选择——它的
  `mcp_servers.<id>` 里没有沙箱字段，而 OpenHands 的 V1 设计原则干脆明说「unify agent and tool
  execution within a single process by default, **aligning with MCP's local-execution model**」
  （`docs/research/notes/aider-openhands.md:669-671`）。
- **环境继承是比沙箱更急的一条。** `docs/credentials.md` 的边界一节自己承认：「打码不是沙箱」、
  打码器只认识**配置过的**那些 provider key（`docs/credentials.md:55-59`、`:117-118`）。一个继承
  登录环境的 MCP server 能拿到 agent 进程能拿到的每一个 key——包括 `docs/credentials.md` 没配进
  打码器的那些。DSH 的对策是 `scrubbedParentEnv()`（删 `/KEY|PASSWORD|SECRET|TOKEN/i` 与
  `DSH_*`，`dsh-mcp-client/README.zh.md:136-138`）；fs-agent 现在**没有对应机制**。
- **生命周期形状不同。** `process::run` 是「一次调用一个进程、超时杀组、drop 杀组」；MCP
  server 是**常驻**的（DSH 的 supervisor 管「代」与重连，
  `dsh-mcp-client/README.zh.md:128-130`、`:93`）。这套超时 / 进程组机制不能直接复用，要另写一套
  「常驻 + 关闭确认 + 退避重连」。
- **长连接还有一个别处没有的问题**：`process::run` 把 stdout / stderr 都读干当成命令结果
  （`process.rs:137-148`、`:221-230`）。MCP 的 stdio 传输要用 stdout 跑协议，这跟「输出即结果」
  是两套模型。

### 4.4 资源与 `ReadSet`：纯函数撞运行时 URI

**现状。**

- `Tool::read_paths(&self, args) -> Vec<PathBuf>` 是**调用前**的纯函数（`src/tools/tool.rs:209-215`，
  默认空）；`Registry::facts` 把它逐条拿去 `paths.resolve_read(&path)`，解析不了的读变成
  `PathError`（`src/tools/registry.rs:134-142`），而 `CallFacts::guardrails` 再用解析后的
  `write_targets` 做「改前先读」（`registry.rs:271-293`）。`ReadSet` 里装的是 `PathBuf`
  （`tool.rs:163-195`）。
- 读路径的边界由 `SessionPaths` + 权限档决定：`outside_read` 是「区外读」的那一条裁决
  （`src/permissions.rs:564-581`，`Policy::with_outside_read` 在 `:374-381`；ADR 0007 解释了
  「为什么区外读仍默认拒绝，却给一个旋钮」）。

**MCP 撞上来的是哪一条。** MCP resource 的身份是**运行时的 URI**（`resources/list` 才知道有哪些，
URI 方案可以是 `https://`、`git://` 或 server 自定义，见 `1.7`），不是调用前的 `PathBuf`。
两处都对不上：URI 不是路径，`resolve_read` 没法把它对着 cwd 解析（它只会把 cwd 之外的东西判成
`outside_read`）；而 `ReadSet` / `read-before-edit` 整套假设「读的是文件系统上的一个路径」。
DSH 的落法是**不建 ReadSet**：资源走三个共享工具（`list_mcp_resources` /
`list_mcp_resource_templates` / `read_mcp_resource`），显式 `server` + `uri` 参数、按需读
（`dsh-mcp-resources/README.zh.md:34-40`、`:92`）。把资源表达成普通工具（URI 当参数）能完全绕开
`ReadSet` 的冲突，代价是「资源」这个概念在模型眼里退化成「一个查询工具」。

### 4.5 sampling 的预算与事件流：反向的模型调用落谁的账

**现状。**

- 「只有循环写事件流」是三条不变量之一（spec.md:664 的三条不变量之 (3)；`src/agent.rs:3`
  「`agent` 层是事件流的唯一写者，而循环是唯一调用 provider 的地方」；`src/session/mod.rs:9`
  「以 `agent` 层是事件流的唯一写者；工具、钩子、权限与讨论都写不了」；`src/discussion.rs:7`
  同义）。写路径是 `EventLog::append`（`src/events.rs:1009`），日志是 `agent` 层经它唯一那条
  `append_event` 路径到达的（`src/agent.rs:2206` 的注释）。
- `UsageRecorded { usage }` 只带 `usage`（`src/events.rs:421`），**不带模型、不带发起者**；
  `src/session/observe.rs:897` 的注释把这个当已知事实写下来：「`UsageRecorded` 不携带模型 ——
  名册住在配置里、不在流上」。预算就是把这些事件加起来（`src/session/ledger.rs:30`、`:71`；
  `src/events.rs:893` 的查询；`src/lib.rs:859`；`CONTEXT.md:181` 的「累计 token 上限」一段），
  跨会话靠 `SessionConfig.carried_tokens` 承载（`src/config.rs:1935`、`:2043-2045`）。
- 投影侧对「新 payload」的态度是穷举的：`project()` 的 `match` 刻意没有 `_` 分支
  （`src/provider/projection.rs:62-64`：「新的 payload 必须在这里被分类，而不是悄悄默认成不可见」）。

**MCP 撞上来的是哪一条。** sampling 是「server 请 client 跑一次 LLM」。在 fs-agent 里这一句话
要落成三件事：(a) **谁调用 provider** —— 按不变量只有 `agent` 层能调，而触发者是 transport 层的
一个异步请求，不是循环；(b) **落谁的 `UsageRecorded`** —— 这条事件没有发起者字段，而现在唯一
的「非回合调用」先例是合成器（它仍是 `agent` 层发起的单发调用，spec.md:483 提到它「只发 `Delta` /
`Message` / `Usage`，没有回合边界」）；(c) **算不算预算** —— 预算是会话级共享值、跨执行者原样
继承（spec.md:529），而「非回合的 provider 调用照样进预算」已有先例：压缩摘要那一次单发调用
「照记 `UsageRecorded` —— 它就是一次 provider 调用，所以自然落进目标预算」（`src/lib.rs:915-920`）。
真正的差别在**发起者**：那一次与合成器都由 `agent` 层发起，而 sampling 由 transport 层的一个
异步请求触发，落在「只有循环写流、循环是唯一 provider 调用方」的外面。放宽一点看，
这件事的难点不在「能不能调 provider」，而在「一次不是循环发起的调用怎么在不破坏『只有循环写流』
的前提下进流」。`2026-07-28` 已把 sampling 标为 deprecated（`1.2`），DSH 直接不声明这个 capability
（`2` 节末行），所以「不做」是一条有依据的选项。

### 4.6 roots 与 `workspace` 档、沙箱可写根、`SessionStarted.cwd`

**现状。** 「这场会话开在哪」只有两个权威来源：`SessionStarted.cwd`（`src/events.rs:327-331`，
`src/session/store.rs:12-15` 说「权威绑定是 `SessionStarted` 里记下的 `cwd`」）和注入的
`SessionPaths`。沙箱可写的东西是「会话 cwd + `[sandbox] writable_roots`」（`sandbox.rs:327-341`；
`docs/sandbox.md` 的边界一节写默认 `~/.cargo`、`~/.rustup`、`~/.cache`，并明说「**这是可用性决定，
不是安全决定**」）。权限侧 `workspace` 档的判据是「写目标落不落在 cwd 之内」（`permissions.rs:125-126`、
`:154-177`；ADR 0007）。

**MCP 撞上来的是哪一条。** roots 是 client 告诉 server「你该操作这些目录」。fs-agent 的
「能操作哪些目录」不是常量：它由权限档（`workspace` 档下区外要问一次）、沙箱的 `writable_roots`
配置、以及升级手势共同决定。所以有三个都不满意的选项：只报 `SessionStarted.cwd`——与「可写 =
cwd + writable_roots」不一致，server 会以为自己不能碰 `~/.cargo`；把 writable_roots 也报出去——
等于把一条安全边界的值交给外部进程；不声明 roots capability——那 server 就得靠自己的配置。
`2026-07-28` 已经把 roots 标为 deprecated、并移除了 `notifications/roots/list_changed`
（`changelog` Major changes 第 5 条），所以「不做」同样有依据。

### 4.7 elicitation 与 `ask_user_question`：同类还是第四类

**现状：三处「问人」的地方，各有各的语义。**

- `Asker`：权限门答 `Ask` 时的询问端口，写 `PermissionAsked` / `PermissionDecided`
  （`src/events.rs:427-437`），`SessionScaffold.asker` 为 `None` 时循环把 `Ask` 降级成 `Deny`
  （`src/lib.rs:82-84`）。它问的是「**跑不跑**」。
- `ask_user_question`：**第三类发起者**（`CONTEXT.md:133` 明说「模型发起的问句，答案是上下文……
  与**询问（Ask）**的分界是『答案决定去留』还是『答案是模型继续干活的输入』——所以这是**第三类
  发起者**，`Asker` 那条接缝不扩展」）。它也带两条只属于它的性质：`effect` 是 `ReadOnly`
  （`ask_user.rs:195-198`，理由是「提问不碰任何工作区路径」），`delegable() == false`
  （`:200-204`，所以执行者的工具表里没有它）。答案就是那条 `tool_call` 的**唯一结果**
  （`ask_user.rs:206-222`），端口是 `ToolContext.questions: Option<&dyn UserQuestions>`
  （`src/tools/tool.rs:133-135`）。
- `ToolContext.questions` 与 `SessionScaffold.questions` 是同一个端口的两端
  （`src/lib.rs:85-89`）。

**MCP 撞上来的是哪一条。** elicitation 的发起者是 **server**，不是模型、也不是 harness。它要的
输入既**不决定去留**（不像 `Asker`），也**不属于任何 `tool_call`**（不像 `ask_user_question`：
在 `2026-07-28` 里它是 server 返回的 `InputRequiredResult.inputRequests` 之一，client 用
`inputResponses` **重试原请求**，答案最终变成那次重试的普通结果，而不是一条独立结果）。所以：
形状上它最接近 `questions` 端口，但**归属**对不上——`ask_user_question` 的答案挂在一条
`ToolCallCompleted` 上，而 elicitation 的答案应当挂在「原 MCP 工具调用的结果」上，中间没有
`tool_call` 可用。要落流就需要新的 payload，或者把 `PermissionAsked` 硬套（语义不合：那会让
一次 elicitation 看起来像一次权限裁决）。规范侧的形状已经核实：form mode 的 `requestedSchema`
只允许顶层 primitive、无嵌套；结果是 `action ∈ accept|decline|cancel`；密码 / API key 一类
**MUST NOT** 走 form mode 而要走 URL mode（`1.7`）。所以这是一条真正需要人拍板的边界：
**第四类发起者，还是不声明 elicitation capability。**

## 5. Rust 侧可用的件

### 5.1 `rmcp`：官方 Rust SDK

- **身份**：`modelcontextprotocol/rust-sdk` 仓库，crate 名 `rmcp`（另有 `rmcp-macros`）。
  README 首句：「An official Rust Model Context Protocol SDK implementation with tokio async
  runtime.」
- **版本 / 许可 / 活跃度**（crates.io API，核实于 2026-10-03）：最新 `3.5.0`，发布于
  **2026-09-28**；**license `Apache-2.0`**；`edition 2024`、`rust-version 1.88`；累计下载
  31,317,528、近 90 天 16,668,718；共 67 个版本；首次发布 2025-03-16。上一个版本 `3.4.1`
  在 2026-09-23，`3.4.0` 在 2026-09-15——即**一周内有多次发布**。
  > 出处：<https://crates.io/crates/rmcp> · <https://github.com/modelcontextprotocol/rust-sdk> ·
  > <https://crates.io/api/v1/crates/rmcp>
- **规范覆盖**：README 明说「This SDK implements the stable MCP **`2026-07-28`** specification
  while remaining fully compatible with the **`2025-11-25`** release and earlier versions.」
- **生命周期两头都做**：`().serve(transport)` 是 legacy `initialize` 流程；`ClientServiceExt::serve_with_lifecycle`
  可以显式选 `ClientLifecycleMode::Discover { preferred_versions }`（直接用 `server/discover`）
  或 `ClientLifecycleMode::Auto { preferred_versions, legacy_version }`——后者「probe the discover
  lifecycle and fall back when a legacy server reports that `server/discover` is not implemented
  or does not respond within **10 seconds**」。这正好是 DSH README 里「先起临时探测进程」那件事
  的实现形态。
- **传输**：`transport-io`（stdio）、`transport-child-process`（`TokioChildProcess`）、
  `transport-streamable-http-client-reqwest`（基于 reqwest）、`transport-worker`（进程内）。
  对 HTTP 的取消 / 并发有明确数字：客户端允许最多 16 个普通 POST 并发，控制请求（取消、回复）
  走单独队列、默认超时 5 秒。**明确不提供 legacy HTTP+SSE**（README 标题就是「Legacy HTTP+SSE
  transport (`2024-11-05`) — intentionally not provided」）。
- **与 fs-agent 的契合点**：
  - tokio 原生（fs-agent 已经依赖 tokio，`Cargo.toml` 的 `tokio = { version = "1", features =
    ["rt-multi-thread", "macros", "sync", "net", "time", "process", "io-util"] }`）。
  - HTTP 客户端可以选 TLS 后端：feature 里有 `reqwest`（rustls）与 `reqwest-native-tls`；
    fs-agent 的 `reqwest` 明确选了 `native-tls`（`Cargo.toml` 的注释：「`native-tls` 直接链系统
    的 OpenSSL，不去拉 aws-lc-rs 那套 C 构建」），所以要对齐应当选 `reqwest-native-tls`。
  - MRTR 自动驱动：`call_tool` / `get_prompt` / `read_resource` 会自己 fulfill 内嵌请求并重试，
    上限 `DEFAULT_MRTR_MAX_ROUNDS`（10）；也有 `call_tool_once` 的手动模式。
- **成本与注意**：`rmcp` 3.5.0 的 `default` features 是 `["base64", "macros", "server"]`——
  也就是说**默认会把 server 侧拉进来**（连带 `schemars`、`pastey`、`uuid`、`tower` 等）。只做
  client 时应当 `default-features = false` 再显式挑 `client` + `transport-child-process` +
  `transport-streamable-http-client-reqwest` + TLS feature。具体 feature 组合没有在本次调研里
  编译验证（不允许 `cargo add` / `cargo build`），落地时必须先验一遍。

### 5.2 其它候选

本次调研**没有**对 `mcp-client`、`mcp-core`、`rust-mcp-sdk` 这类第三方 crate 做同等核实，
因此不给结论。搜索过程中出现的 `turul-mcp-protocol-2026-07-28` 一类是**协议类型** crate，
不是 client，不要混。要不要考虑它们，取决于人是否认为「非官方」本身是个问题——这一点在
「需要人拍板」一节里没有单列，因为它不影响接入的机制，只影响依赖选择。

### 5.3 仓库现在的传递依赖

`Cargo.lock` 全文 grep `mcp` 与 `modelcontextprotocol`（大小写不敏感）**零命中**；`Cargo.toml`
的 `[dependencies]` 里也没有相关条目。所以引入 MCP client 是**纯新增**依赖，不会替换或冲突
现有任何一条。需要留意的只有 `reqwest` 的 TLS 后端选择（见 5.1）与 rmcp 默认 features 会把
server 侧拉进来这两点。

## 6. 需要人拍板的点

每一条给「选项」与「它牵动的既有机制」。这里不替人做决定。

1. **工具表：运行时发现还是组装期冻结。** 选项：(a) 组装期全量发现一次、此后冻结（守住
   `src/tools/mod.rs:1-10` 与 spec.md:448，代价是看不到 `listChanged`）；(b) 允许运行时换表
   （要改 `Registry` 的形状与共享方式——`src/lib.rs:260`、`:285`，并推翻 spec.md:448、`:426`）；
   (c) 只做 tools、明确忽略 `listChanged`（等于把 (a) 写成特性而不是限制）。牵动：前缀缓存
   不变量、`Registry::for_executor()` 的快照语义（`registry.rs:70-78`）、`.scratch/fs-agent-v1/spec.md`
   的三处明文。
2. **外来工具的 `Effect`。** 选项：(a) 恒 `Exclusive`（= `CustomTool` 的先例，`src/tools/custom.rs:73-77`）；
   (b) 恒 `ReadOnly`（并发但会把外部副作用误标，与 `Effect` 的定义冲突）；(c) 按
   `ToolAnnotations` 映射（规范明说 hints 不可信，见 `1.6`）；(d) 给 `Effect` 加第四类（动调度器
   分区、权限表的九条 `stance`、`AllowedCall.exclusive` 的推导）。牵动：`src/permissions.rs:127-184`
   的裁决表、`Registry::facts` / `guardrails`、`read-before-edit` 的覆盖面。
3. **MCP server 进程的沙箱与环境。** 选项：(a) 不过 `wrap`、全继承环境（= 现在 `bash -lc`
   的形状，`src/tools/process.rs:116-129`）；(b) 过 `wrap`（要解决「常驻进程 ≠ 一次调用」，
   `sandbox.rs:273-324` 的 argv 包法假设后者）；(c) 只做环境清洗不过沙箱（学 DSH 的
   `scrubbedParentEnv()`，`dsh-mcp-client/README.zh.md:136-138`）。牵动：`docs/credentials.md`
   已声明的「打码不是沙箱」边界（`:117-118`）、`docs/sandbox.md:22` 的「不管网络」、以及
   `SandboxMode` 的 fail-closed 语义（`docs/sandbox.md:14`）。
4. **远端 server 的凭据从哪来、要不要打码。** 选项：(a) 配置里明文 / 引用环境变量（DSH 的
   `!!js process.env.X` 那种表达式）；(b) 命令行交互式登录（Claude Code 的 `claude mcp login`、
   Gemini 的 `/mcp auth`、opencode 的 `opencode mcp auth`）；(c) OAuth 流程（rmcp 有
   `auth` / `oauth` features）。牵动：`config.toml` 的形状（`src/config.rs`）、`docs/credentials.md`
   的打码器（它只认识**配置过的** provider key，`:55-59`）、以及 `ToolCallStarted.args` 会把
   工具参数原样记进流（`src/events.rs:409-413`）——如果凭据能出现在参数里，那是一条新的泄漏面。
5. **资源怎么表达。** 选项：(a) 三个共享工具、URI 当参数、调用时才读（DSH 形，
   `dsh-mcp-resources/README.zh.md:34-40`）；(b) 每个资源 / 模板变成一条工具声明；(c) 给
   `ReadSet` 加一套非路径身份（动 `src/tools/tool.rs:163-195` 与 `read_paths` 的纯函数契约）。
   牵动：`read_paths` 的纯函数性（`tool.rs:209-215`）、`SessionPaths::resolve_read` 的 cwd 上限
   （`registry.rs:134-142`）、`outside_read`（`permissions.rs:564-581`）。
6. **sampling / roots / elicitation 三个 client capability 各做不做。** 选项：(a) 全不做
   （`capabilities: {}`，DSH 的做法）；(b) 只做 elicitation（牵动「第四类发起者」的判断与是否
   新增事件 payload）；(c) 做 sampling（牵动「谁调 provider」「谁的 `UsageRecorded`」「算不算
   预算」三条，见 4.5）；(d) 做 roots（牵动 `SessionStarted.cwd`、沙箱可写根、`workspace` 档的
   三者关系，见 4.6）。注意规范已把 sampling 与 roots 标为 deprecated。
7. **协议版本策略。** 选项：(a) 只谈 `2026-07-28`（无 `initialize`、要 `server/discover`，与
   大部分现成 server 可能不在同一代）；(b) 同时兼容 `2025-11-25` 及更早（要做探测与回退；
   rmcp 的 `ClientLifecycleMode::Auto` 现成，但要定探测超时与失败语义）；(c) 还要不要考虑
   legacy HTTP+SSE（rmcp 明确不提供，等于要自研或换库）。牵动：启动延迟（探测要等对端答，
   rmcp 默认 10 秒超时）、「工具表在首轮之前就位」的可行性、以及 server 起不来时的降级语义
   （Codex 的 `required = true` 直接让 `codex exec` 失败，DSH 的 `failOnStartupError` 默认
   `false`——两种都有先例）。
8. **先改 spec 还是另起 effort。** `.scratch/fs-agent-v1/spec.md:449` 写着「明确不做：MCP client
   本身」、`:636` 把它列进「证据型砍掉项（判据变化不翻它们）」、`:667` 写着「实现者请勿『顺手
   改进』……要动它们，先改这张 spec」，`README.md:270` 的「这一版不做」清单也列着。选项：
   (a) 直接改这三处 + README；(b) 学 `.scratch/sandbox/spec.md` 的先例，另起一个 effort，把
   MCP 从「这一版不做」里按一份自己的 spec 拿出去（`README.md:270` 那段结尾自己就举了这个先例：
   「或者像 [`sandbox`](.scratch/sandbox/spec.md) 那样另起一个 effort」）。牵动：所有后续票的
   依据、以及 `.scratch/README.md` 的 feature 索引。

## 来源

### 仓库内（文件:行）

- `.scratch/fs-agent-v1/spec.md:449`（「明确不做：MCP client 本身」）、`:636`（证据型砍掉项里的
  MCP client）、`:667`（实现者请勿顺手改进）；同文件的 `:119`、`:138`、`:426`、`:448`、`:374`、
  `:483`、`:529`、`:665` 附近（三条不变量）。
- `README.md:270`（「这一版不做」清单，含 MCP client 与「另起一个 effort」的先例）。
- `src/tools/mod.rs:1-10`、`:78-94`、`:104-110`、`:112-118`、`:119-130`。
- `src/tools/tool.rs:3-5`、`:21-33`、`:109-138`（`ToolContext`）、`:163-195`（`ReadSet`）、
  `:202-249`（`Tool` trait，含 `:206-207` 的 `effect`、`:209-215` 的 `read_paths`）。
- `src/tools/registry.rs:1-5`、`:46-59`、`:70-78`、`:88-93`、`:102-164`（`facts`）、
  `:170-216`（`dispatch`）、`:242-262`（`CallFacts`）、`:271-293`（`guardrails`）。
- `src/tools/custom.rs:1-13`、`:27-29`、`:73-77`、`:86-97`。
- `src/tools/process.rs:1-17`、`:105-133`、`:150-206`、`:232-267`。
- `src/tools/sandbox.rs:19`、`:31-42`、`:273-324`（`wrap`）、`:327-341`（`writable_roots`）。
- `src/tools/bash.rs:1-14`、`:56-57`、`:69`。
- `src/tools/ask_user.rs:1-14`、`:195-204`、`:206-222`。
- `src/permissions.rs:57-64`（`Mode`）、`:120-184`（`stance` 表）、`:202-209`（`Subject`）、
  `:246-271`（`Scope`，含 `Scope::Tool`）、`:353-381`（`Policy` / `outside_read`）、
  `:426`、`:494`、`:531-600`（`decide`）。
- `src/events.rs:1-8`、`:327-331`（`SessionStarted.cwd`）、`:405-445`（`ToolCallStarted` /
  `ToolCallCompleted` / `UsageRecorded` / `PermissionAsked` / `PermissionDecided`）、
  `:893`、`:931-1009`（`EventLog` 与 `append`）。
- `src/provider/projection.rs:1-21`、`:46`、`:62-64`。
- `src/lib.rs:70-96`（`SessionScaffold`）、`:98-110`（`AssemblyParts`）、`:245-300`（`Arc<Registry>`
  与 `session()`）、`:855-862`（`carry_usage`）、`:915-920`（压缩摘要那次调用照记
  `UsageRecorded`）。
- `src/cli.rs:380-398`、`:705-708`、`:2212-2232`、`:4021`、`:4107`、`:4258`。
- `src/config.rs:160-198`（`CUSTOM_TOOL_*` / `ToolDeclaration`）、`:382`、`:1920-1956`、`:2043-2049`。
- `src/session/store.rs:1-45`；`src/session/mod.rs:9`；`src/agent.rs:3`、`:2206`；`src/discussion.rs:7`；
  `src/session/observe.rs:897`；`src/session/ledger.rs:30`、`:71`；`CONTEXT.md:115`、`:133`、`:181`。
- `docs/sandbox.md:14`、`:22`、`:92`；`docs/credentials.md:55-59`、`:117-118`；
  `docs/adr/0006-sandbox-by-bubblewrap.md`、`docs/adr/0007-workspace-permission-mode.md`。
- `Cargo.toml`（`reqwest` 的 `native-tls` 注释、tokio features）；`Cargo.lock`（零命中）。

### 本机 DSH 安装（`/usr/lib/node_modules/@deepseek-ai/dsh/node_modules/@deepseek-ai/`）

- `dsh-mcp-client/README.zh.md:12`、`:28`、`:32-71`、`:75-93`、`:107-111`、`:128-130`、
  `:136-138`、`:164`、`:170-172`、`:188-200`、`:209-214`、`:222-228`。
- `dsh-mcp-client/lib/index.js:26-31`、`:68-72`、`:96-102`、`:127-161`、`:361-414`、`:433-471`、
  `:481-717`、`:596-609`、`:654-657`、`:677-683`、`:735-742`、`:748-750`、`:774-800`、`:809-833`。
- `dsh-mcp-resources/README.zh.md:12`、`:34-40`、`:57-59`、`:92`、`:98-100`、`:122-126`。

### `docs/research/`（本仓库对上游的一手引文）

- `coding-agent-features.md:155`、`:248-271`、`:405-439`、`:725-764`。
- `notes/claude-code-amp.md:41-43`、`:255`、`:494-529`、`:843-864`、`:968-996`、`:1009-1012`、
  `:1112-1137`。
- `notes/codex-gemini.md:105-116`、`:230-243`、`:344-353`、`:560-601`、`:674-697`、`:826-855`、
  `:925-937`。
- `notes/cline-continue.md:119-132`、`:420-437`、`:855-899`、`:1069`、`:1728-1743`、`:1830-1874`。
- `notes/aider-openhands.md:190-203`、`:449-468`、`:496-504`、`:544-553`、`:664-675`、`:718-735`、
  `:869-882`、`:1012`。
- `notes/opencode-goose.md:80-91`、`:116-127`、`:596-617`、`:660-673`、`:706-717`、`:1128-1141`、
  `:1240-1246`。

### 外部 URL

- MCP 规范首页 / 版本：<https://modelcontextprotocol.io/specification/2026-07-28>
- 关键变化（含版本谱系与 deprecated 清单）：<https://modelcontextprotocol.io/specification/2026-07-28/changelog>
- Deprecated 特性注册表：<https://modelcontextprotocol.io/specification/2026-07-28/deprecated>
- 传输总览（stdio / Streamable HTTP / 取消 / 每请求 `_meta` / 向后兼容）：
  <https://modelcontextprotocol.io/specification/2026-07-28/basic/transports>
- stdio：<https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio>
- Streamable HTTP：<https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http>
- Tools（含 human in the loop）：
  <https://modelcontextprotocol.io/specification/2026-07-28/server/tools>
- Resources：<https://modelcontextprotocol.io/specification/2026-07-28/server/resources>
- Elicitation：<https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation>
- Sampling：<https://modelcontextprotocol.io/specification/2026-07-28/client/sampling>
- Roots：<https://modelcontextprotocol.io/specification/2026-07-28/client/roots>
- 规范 schema（`ToolAnnotations`、`Tool`、`ServerCapabilities`、`SubscriptionFilter`、
  `InputRequiredResult`、`RequestMetaObject` 等定义的原文）：
  <https://raw.githubusercontent.com/modelcontextprotocol/specification/main/schema/2026-07-28/schema.ts>
- 官方 Rust SDK：<https://github.com/modelcontextprotocol/rust-sdk>
- `rmcp` crates.io 页面与 API（版本 / 许可 / 下载量 / features）：
  <https://crates.io/crates/rmcp> · <https://crates.io/api/v1/crates/rmcp>
- `rmcp` README（生命周期模式、MRTR、传输表、缓存、标准 HTTP 头）：
  <https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/README.md>
