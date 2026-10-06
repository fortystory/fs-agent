# MCP 接入：四个元工具（工具层 · 服务层 · 连接层）

Status: 9 resolved + 9 done + 1 ready-for-walkthrough（实现票 [`issues/10`](issues/10-mcp-service-and-list.md)–[`19`](issues/19-mcp-catalog-in-context.md)，
2026-10-03 由 wayfinder 决策图 [`map.md`](map.md) 折成 —— 二十条冻结项 + 九张决策票；依赖边见各票抬头）

模型今天只能用内建的那几个工具加 `config.toml` 里静态声明的 `custom__*`。**外部能力进不来**：
想让它操作 GitHub、查内部的 Jira、读一份外部数据源，只能靠 `bash` 拼命令行 —— 而那把每次调用都
算成 `Exclusive`、还要求人先把命令行学一遍。MCP（Model Context Protocol）就是为这件事定的协议：
一个 agent 连上外部**进程或服务**（下面叫 **server**），把它提供的能力拿来用。

难点不在协议，而在**形状**。fs-agent 的工具表是**组装期**建好的、之后不再变化（它是缓存前缀的
一部分），而 MCP 的 server 有哪些工具**只有连上才知道**。这份 spec 用一个选择把这个撞点整个绕开：
**不把 server 的工具铺进表**，只加**四个固定名字的元工具**，让模型按 `server` + `tool` 两个参数
去调 —— 表永远不变，server 侧增删工具只影响 `mcp_list` 的**返回**。

来源与依据：

- 决策图 [`map.md`](map.md)：**二十条冻结项**（charting 的 grilling 定下）+ **九张决策票**
  （`issues/01`–`09`，全部 resolved）。每一条决定的完整理由与取舍都在票的 `## 作答` 里。
- 一手调研：[`research/01-mcp-client-implementation.md`](research/01-mcp-client-implementation.md)
  （规范要点、DSH 的做法、上游八个实现、fs-agent 侧的撞点）与
  [`research/02-rmcp-fit.md`](research/02-rmcp-fit.md)（`rmcp` 3.5.0 的契合度）。
- 范围修订已办：v1 的 `明确不做` 三处与 [README.md](../../README.md) 的「这一版不做」都加了
  带日期的补记（票 07），原文未改写。

## 问题陈述

1. **模型的手伸不出去。** 内建工具全是本地的（读、写、跑命令、搜代码），而「今天的活」经常在
   别处：一个 issue、一张工单、一份远端数据。今天唯一的出口是 `bash` + 某个命令行工具 —— 要求
   模型知道那个工具的存在与用法，而且每次调用都按「shell 什么都能写」计价。
2. **静态声明工具只解决了一半。** `config.toml` 的 `custom__*` 能声明 argv 模板，但它的
   `Effect` 恒为 `Exclusive`（与 `bash` 同理），而且是**人**事先写死的 —— 它表达不了「一个外部
   服务自己声明了哪些能力」。
3. **「把外部工具铺进工具表」这条路是封死的。** `tools` 数组属于请求前缀，组装后不再变化
   （`.scratch/fs-agent-v1/spec.md` §14：可见性 = 启动即全局可见、组装期固定）。任何「运行时往表里
   加工具」的做法都会废掉每一个会话的缓存前缀。
4. **外部东西的信任与副作用没有位置。** `Effect` 三类描述的全是**工作区**副作用；一个「查远端
   Jira」的工具既不是只读工作区、也不是写工作区。而 server 自报的 `ToolAnnotations` 按规范原文
   只是 hints，**不能**据以做权限判断。

## 方案

**三层，依赖单向向下**（与 `.scratch/web-search-tool/spec.md` 的三层同构）：

```text
工具层  四个固定名字的元工具：mcp_list / mcp_call / mcp_resources / mcp_read
        （schema、参数校验、结果形状、不可信标记、引用）
   │
服务层  src/mcp/：连接池 + 后端解析 + 带 code 的 McpError
   │
连接层  rmcp 3.5.0（default-features = false）：Discover 生命周期、stdio 与 Streamable HTTP
```

- **四个元工具全部进 `builtin()` 之后的组装期一步**（照 `with_web` 的形状），由 `[mcp] enabled`
  （缺省关）决定在不在；**server 的工具永远不进表**。
- **第一阶段只做前两个**（`mcp_list` / `mcp_call`）；资源、提示词模板与边角按 §8 分阶段。
- **协议只谈 2026-07-28**（无状态 `_meta` + `server/discover`），不做旧版 `initialize` 回退 ——
  `rmcp` 的 `ClientLifecycleMode::Discover` 正好就是这个语义（源码注释：*Discover mode does not
  fall back; a legacy server is an error.*）。
- **默认全关**：不开 `[mcp] enabled`，会话行为逐字不变。

## 用户故事

1. 作为**模型**，我想要一份「这台 server 现在有哪些工具、各要什么参数」的清单，以便在不认识这个
   server 的情况下也能正确地调它。
2. 作为**模型**，我想要按名字调一个外部工具并把它的结果拿回来，以便把「写 issue」「查工单」这类活
   干完，而不用退回 `bash`。
3. 作为**模型**，我想要一份「这台 server 有哪些资源、URI 怎么拼」的清单，以便读到它暴露的数据。
4. 作为**模型**，我想要按 URI 读一份资源，以便把外部数据放进当前这轮工作。
5. 作为**模型**，我想要在 server 要人补一个输入时把问题转给人，以便那个调用能接着往下走，而不是
   在中间断掉。
6. 作为**人**，我想要在 `/` 菜单里看到某台 server 提供的提示词模板、挑一个并填上参数，以便把它的
   能力当场用上。
7. 作为**人**，我想要 server 返回的内容带着「这是外部数据、不是指令」的标记，以便我不会被一段网页
   或工单正文牵着走。
8. 作为**人**，我想要外来工具**默认**按最严的副作用处理（每次调用都当可能写东西），以便在我没逐条
   审过之前，它不会悄悄绕过工作区纪律。
9. 作为**人**，我想要在配置里**逐条**把某些工具放宽（或让某台 server 的结果不带标记），以便我信任
   的那些不必每次都问我。
10. 作为**人**，我想要 server 进程跑在沙箱里、并且**只拿到我在配置里显式声明的环境变量**，以便
    它不会顺手继承我的密钥。
11. 作为**人**，我想要给某台 server 显式声明它可写的目录，以便它的缓存能落盘而工作区外仍然只读。
12. 作为**人**，我想要配置既能在项目里（跟着仓库走）也能在用户目录里，并且**项目级盖住用户级**，
    以便同一个仓库的协作者共享一套 server 配置。
13. 作为**人**，我想要远端 server 的凭据写在配置里并**进打码器**，以便它不会以明文出现在流上。
14. 作为**人**，我想要某台 server 起不来时其余照常工作、它自己在 `mcp_list` 里报一条结构化错误，
    以便一个坏 server 不会拖垮整场会话。
15. 作为**人**，我想要**两台 server 同名时直接拒绝启动**，以便我不会以为两台都活着。
16. 作为**人**，我想要在讨论里两个讨论者都能调外部工具、执行者也能自己调，以便「查证」这件事不必
    靠一个人的转述。
17. 作为**维护者**，我想要 `sessions replay` 能复盘每一次外部调用（谁调的、哪个 server、哪个工具、
    什么结果），以便审计「它为什么这么答」。
18. 作为**维护者**，我想要 server 的结果与工具结果走**同一条**截断与落盘流水线，以便不新增第二套
    上限语义。
19. 作为**维护者**，我想要这台机器的 DNS 被 fake-IP 代理接管时 MCP 也能照常连（照 web 那一轮的
    `trust_proxy_dns` 先例），以便开发机与生产机行为一致。
20. 作为**维护者**，我想要整件事在 `[mcp] enabled = false` 时**零影响**，以便不做它的时候没有任何
    新增的启动成本与行为变化。

## 实现决定

### §1 三层与「元工具」这个选择

- **工具层的四个名字是固定的**：`mcp_list` / `mcp_call` / `mcp_resources` / `mcp_read`。它们进表的
  方式与 `repo_map` / `grep` 一样，由组装期的一步加进去（照 `with_web` 的形状，新增一个 `with_mcp`）。
- **server 的工具永远不进表。** 这条是整个设计的支点：规范里的
  `notifications/tools/list_changed` 因此对我们**无事发生** —— 表里只有四个固定条目，server 侧增删
  工具只改变 `mcp_list` 的返回。
- **服务层 `src/mcp/`**（新顶层模块）：一个 `McpService`，持有「已配置的 server → 连接」的映射，
  暴露 `list_tools(server)` / `call_tool(server, tool, args)` / `list_resources(server)` /
  `read_resource(server, uri)`；错误是带 code 的 `McpError`（code 保持英文、句子是中文，照 web 的
  `WebError`）。
- **连接层是 `rmcp`**（见 §3）。服务层是唯一碰它的地方；工具层与它无关。

### §2 四个元工具的形状

模型可见的 schema（一次定死，进前缀缓存）：

```jsonc
// mcp_list
{ "server": "github" }                       // 可选；不传就列全部启用的 server
// mcp_call
{ "server": "github", "tool": "create_issue", "arguments": { /* 自由 JSON */ } }
// mcp_resources
{ "server": "db" }                           // 可选，同上
// mcp_read
{ "server": "db", "uri": "db://users/42" }
```

- **`arguments` 是一个自由 JSON 对象，fs-agent 不校验它** —— schema 在 server 那侧，我们只透传
  （`rmcp` 的 `CallToolRequestParams.arguments` 正是 `Option<JsonObject>`，零摩擦）。
- **上限与超时都不进 schema**：照 web 那一轮的规矩，它们是部署设置（`[mcp]` 段），模型给不了自己
  预算。
- **`mcp_list` 每次现问**（票 05）：不订阅 `toolsListChanged`、不做会话内缓存 —— 没有缓存就没有
  失效；连不上就是一条结构化错误。资源与工具分开列（`mcp_list` 列工具、`mcp_resources` 列资源），
  因为两者的形状不同。
- **`mcp_call` 的返回是「外部数据」**：带中文不可信标记（§7），除非这台 server 声明了
  `trust_results`（§6）。

### §3 连接层：`rmcp` 与生命周期

- **`rmcp` 3.5.0，`default-features = false`**，**TLS 选 `reqwest-native-tls`** —— 选 `reqwest`
  会把仓库的 TLS 换成 rustls，等于推翻 `Cargo.toml` 里那条既有决定。引入时先跑一遍编译，确认
  feature 组合与体积（调研时未编译验证）。
- **生命周期固定 `ClientLifecycleMode::Discover`**：只用 `server/discover`、不回退 —— 这正是
  「只谈 2026-07-28」的实现，且是零配置达成的。
- **传输**：stdio（本地子进程）+ Streamable HTTP（远端）。**不做 legacy HTTP+SSE**（`rmcp` 也不
  提供）。
- **连接归会话**：`[mcp] enabled` 打开时，`McpService` 在组装期建、随会话活；**关闭走 Drop 兜底**
  （`RunningService` 的 Drop 自动 cancel）—— 因为 `close` 要 `&mut self` 而 `Tool` 只给 `&self`。
- **不自动重连**：server 崩了之后同会话内不再重试（与 web 的失败语义同一条线）；重开会话才重连。
- **并发起、失败的跳过**（票 08）：组装期对配置里所有启用的 server 并发发连接，每台各自一个超时；
  谁失败就跳过谁，它在 `mcp_list` 里报成结构化错误。
- **不做 server 数量上限**；**重名是启动错误**（消息里点名是哪个键）。
- **MRTR**：server 回 `resultType: "input_required"` 时，`rmcp` 的 `call_tool` 会自动驱动重试回路
  （默认 10 轮），把每个请求交给本地 `ClientHandler`。我们把 `create_elicitation` 覆写成「转给人」
  —— **复用 `ask_user_question` 那条第三类发起者的通道**，不新开第四类。要让它真的发生，client 得
  在能力里 `enable_elicitation()`。
- **为此要动一个既有接口**：`ClientHandler` 要求 `'static + Send + Sync`，而问询端口今天是借在
  `ToolContext<'a>` 上的 `questions: Option<&'a dyn UserQuestions>` —— 把它改成
  `Arc<dyn UserQuestions + Send + Sync>`（`ToolContext.questions` 跟着换形状）。**不给 MCP 单开一条
  平行通道**（两套实现会漂）。

### §4 server 进程：沙箱、环境、可写根

- **server 进程随会话起、随会话停，过 bubblewrap 沙箱**（与 `bash` 同一层）：argv 仍从
  `Sandbox::wrap` 这个纯函数出来（它是纯函数，直接可用），再交给 `rmcp` 的 `TokioChildProcess`。
- **保留「杀整组」的纪律**：`rmcp` 的默认清理（process-wrap 的 `kill`）只保证直接子进程被杀，
  而 fs-agent 现在是 `process_group(0)` + `killpg` 杀整棵树。**落地第一步先验能不能在 `CommandWrap`
  上叠 process-wrap 的进程组包装**；叠不了就回到「自己 spawn、把句柄交给它」的接法。
- **环境走白名单**（票 06）：`env_clear()` 之后只注入两样 —— server 配置里 `env` 显式声明的项，加
  最小必需的 `PATH` / `HOME` / `LANG`。不照 DSH 的正则黑名单：黑名单 fail-open，而沙箱这一层的
  调性是 fail closed（[ADR 0006](../../docs/adr/0006-sandbox-by-bubblewrap.md)）。**注意**：`rmcp`
  自己不做环境清洗，所以清洗必须发生在**交给它之前**。
- **可写根逐台声明**（票 06）：server 配置里的 `writable_roots` 追加到这台 server 的沙箱可写集；
  不声明就只有「会话工作区 + 沙箱默认的那列缓存目录」。**不给默认的 per-server 临时目录**。
- **`stderr` 显式 piped**（`rmcp` 默认 `inherit`），好让 server 的崩溃信息进得了日志。

### §5 配置

两个来源，**项目级盖住用户级**：

- **项目级**：仓库根的 `.mcp.json`（照上游惯例，可以随仓库走）。
- **用户级**：`config.toml` 的 `[mcp.servers.<名字>]`（与仓库「一处配置」的现状一致）。

server 一条记录的形状（字段名 snake_case，与 `config.toml` 的既有风格一致）：

```toml
[mcp]
enabled = false                 # 组装期读一次；不开就是零影响
connect_timeout_ms = 10_000

[mcp.servers.github]
transport = "stdio"             # stdio | http
command = ["npx", "-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_TOKEN = "…" }     # 白名单：不写就没有
writable_roots = ["~/.cache/github-mcp"]
trust_results = false
trust_effects = false
sandbox = true

# 远端形态：
# [mcp.servers.jira]
# transport = "http"
# url = "https://jira.example.com/mcp"
# headers = { Authorization = "Bearer …" }
```

- **凭据写在 `env` / `headers` 里，值接进打码器**（与 provider 密钥同一条路）—— 今天打码器只从
  `providers` 收集值，`[mcp]` 与 `.mcp.json` 里的密钥要接进去。
- **`deny_unknown_fields`**：server 记录与 `[mcp]` 段都不接受没见过的键（照 `[web]` 的先例）。
- **`.mcp.json` 的路径与优先级**要在文档里写死：仓库根、项目级盖用户级、**不做**「向上逐级找」。

### §6 信任与副作用

- **外来工具默认最严**（票 09 的 `trust_effects` 缺省关）：它的 `effect()` 一律 `Exclusive` ——
  与 `custom__*` 同一个先例。人在配置里把 `trust_effects` 打开后，才允许按配置声明较宽的 `Effect`。
- **不信 server 的 `ToolAnnotations`**：规范原文写着它们全是 hints，而且「clients should never make
  tool use decisions based on ToolAnnotations received from untrusted servers」。我们连读都不读。
- **信任声明是一组能力，不是一个布尔**（票 09）：`trust_results` / `trust_effects` / `sandbox`
  三个位**各自默认关**。「这台是我们自己写的」不蕴含「它的结果可以当指令读」，也不蕴含「它可以写
  任意路径」—— 捆成一个 `trusted = true` 会让人为了省一句标记把写权限一起放出去。
- **权限门照旧**：`Exclusive` 在四档模式下的待遇不变（`readonly` 拒、`ask` 问、`workspace`/`auto`
  放行），调度器照旧为它取工作区锁。

### §7 结果进上下文

- **不可信标记**：`mcp_call` / `mcp_read` 的结果以一句中文标记开头（除非这台 server 开了
  `trust_results`），与 `web_search` / `web_fetch` 用同一条规矩 —— 外部内容、当数据看、不要当指令。
- **server 的 `instructions` 不进系统提示词**（票 02 的 DSH 对照里明确排掉）：系统提示词是身份层
  （可信 + 缓存前缀），而 server 指令是**外部文本**。它放进 `mcp_list` 的结果里，带不可信标记。
- **上限只有一条流水线**：工具返回字符串，`context::truncate_result` + `emit_completed` 负责落盘与
  指针；**不新增按工具的预算字段**。
- **呈现**：元工具是普通工具，`sessions replay` 看到的是 `mcp_call(server, tool, …)` 与它的结果 ——
  不需要新的呈现通道。

### §8 后三个阶段（资源、提示词、边角）

**资源**（票 03）：

- `mcp_resources(server?)` 与 `mcp_read(server, uri)` 两个元工具（第二阶段）。
- **资源不登记进 `ReadSet`**：它装的是**工作区路径**，而资源是 URI —— 语义上装不进去。所以「先读
  后写」那条护栏对资源不适用（与 `grep` 命中文件同一种处理）。
- **资源与工作区路径是两套坐标系**：`read_paths` 走 `resolve_read` 的工作区解析，资源 URI 由 server
  定义，两者不互操作。
- 上限走 §7 那条流水线；正文同样带不可信标记。

**提示词模板**（票 04）：

- **发起者是人**：模板**接进 `/` 菜单**，也就是说这一阶段把「`/` 菜单支持运行时条目 + 一个填参数的
  界面」纳入范围 —— 今天的菜单条目是常量，而模板清单是运行时从 server 拉的。
- 菜单的动态部分**不进工具表、不进前缀缓存**；server 不可用时它的模板条目**不出现**（而不是出现后
  点开报错）。
- 模型**不自发调用**模板（这条在图与本文的《明确不做》里）。

**边角**（第四阶段）：

- **MRTR / elicitation**（§3 已给形状）：先支持 form 模式这一类；`call_tool_once` 是「自己驱动 MRTR」
  的降级出口，某类接不上时用它。
- **logging 通知**、其余 capability：按需要再加，不预先铺。

### §9 谁能用

- **四个元工具对主会话、讨论者、执行者都可用**（`delegable()` 保持 `true`）。
- **执行者的外部调用跑在派发者的模式之下**（既有纪律：委派链向下不继承允许），加上 §6 的默认最严 ——
  一次委派永远不会更宽松。

### §10 文档与索引

- **新增 `docs/mcp.md`**：三层结构、四个元工具的形状、`[mcp]` 与 `.mcp.json` 的字段、沙箱与环境
  白名单做了什么、信任三个位的含义、以及两条边界（**server 的工具不进表**；**模板由人发起**）。
- **`README.md` 的「文档」一节加一行**；架构一节的顶层边界数（当前 14）跟着加 `src/mcp/`。
- **`CONTEXT.md` 加词条**：这次引入的名词要进词汇表 —— **元工具**、**原语**（tool / resource /
  prompt / elicitation）、**server**；措辞在实现票里定。

## 测试决定

**接缝两个：既有的组装入口，加一个真 stdio 的集成测试。**

- e2e 测试沿用 `assemble` / `assemble_discussion` —— 那是仓库里**唯一**的端到端接缝（假 provider、
  注入式 sink 与配置）。
- **MCP 的连接层在组装期注入**（照 `src/web/` 那一轮 `SearchProvider` / `FetchProvider` 的形状）：
  默认给一个**进程内假连接**，零进程、零网络，覆盖工具层与服务层的行为。
- **另加一个真 stdio 的集成测试**（2026-10-03 追加）：起一个**测试用的假 MCP server 二进制**
  （仓库自己的 `tests/` 辅助程序），走 `TokioChildProcess` + `Sandbox::wrap` 那条**真路径**，
  覆盖「spawn、沙箱包装、协议帧、进程组清理」这几件假连接盖不到的事。它进 `cargo test`，但
  **不连任何真实 server、不发网络**。

**好测试的判据**（照仓库规矩）：只断言**外部行为** —— 事件流的 payload（JSONL 的 `seq` + payload）
与工作区副作用；断言工具结果字符串与权限门裁决，**不**断言内部结构；不断言 `at` 时间戳。**零网络**：
所有 provider 与 MCP 连接都是假的。

新增用例（覆盖到票级）：

1. **组装期开关**：`[mcp] enabled = false` 时四个工具都不在表里；打开后都在；**表的内容与 server
   可用性无关**（server 全起不来时工具仍在，调用回结构化错误）。
2. **`mcp_list`**：返回某台 server（或不传时全部）的工具清单与 `instructions`；**每次现问**（假连接
   记调用次数）；server 不可用时是一条可读的结构化错误。
3. **`mcp_call`**：透传 `arguments`（不校验）；结果带不可信标记；`trust_results` 打开时**不带**。
4. **Effect 与权限**：默认 `Exclusive` —— `readonly` 档拒、`ask` 档问；配置里把某条工具放宽后，
   它在 `readonly` 档放行。反向锚是 `bash` 的同类行为。
5. **多台 server 与重名**：并发起（假连接记启动顺序）、失败的跳过、重名时**启动报错**。
6. **环境白名单**：父进程导出的一个假密钥**不出现在**子进程环境里，而配置 `env` 里声明的出现
   （用假 spawn 记录 argv/env，真进程留给冒烟脚本）。
7. **MRTR**：假连接回 `input_required`，断言问题经既有问询端口发出、答案被拼回 `inputResponses`
   并重试一次。
8. **资源**：`mcp_resources` / `mcp_read` 的结果带标记；**资源不影响 `ReadSet`**（读过资源之后
   `edit_file` 仍要求先 `read_file`）。
9. **`.mcp.json` 与 `config.toml` 的优先级**：项目级盖住用户级；两处都没有时零影响。
10. **真 stdio 路径**（假 server 二进制）：起得来、`mcp_list` 拿得到它声明的工具、`mcp_call` 调得动
    并拿回结果、**会话结束时那个进程被收掉**（整组，不是只杀直接子进程）。

**测试的 prior art**：`tests/web_search.rs` / `tests/web_fetch.rs`（组装期注入假后端、断言工具结果
与权限门）、`tests/config_profiles.rs`（配置解析与 `deny_unknown_fields`）、
`tests/executor.rs`（委派链的权限不继承）、`tests/ask_user_question.rs`（问询端口那条通道）。

## 明确不做

- **把 server 的工具铺进工具表**：这是整个设计的支点，永不做。
- **已 deprecated 的原语**：sampling / roots / logging（SEP-2577，最早 2027-07-28 后移除）。
- **旧版协议回退**：只谈 2026-07-28 的无状态形态；不做 `initialize` 握手回退 —— 代价是连不上只支持
  旧版的 server。
- **legacy HTTP+SSE 传输**：`rmcp` 也不提供。
- **MCP server 端实现**：fs-agent 只做 client。
- **订阅 `toolsListChanged`**：`mcp_list` 每次现问，不做订阅流与缓存。
- **server 的 `instructions` 进系统提示词**（DSH 的做法）：系统提示词是身份层。
- **模板的模型自发调用**：模板的发起者是人。
- **按 `ToolAnnotations` 映射 `Effect`**：规范明文说不能这么做。
- **自动重连**：崩了就报错，重开会话才重连。
- **server 数量上限**：写进配置的就是人要的。
- **给模型上限与超时参数**：它们是部署设置。
- **资源进 `ReadSet`**、**资源与工作区路径互操作**：两套坐标系。
- **本 spec 的执行**：它只产决策；「做」发生在 `/to-tickets` → 实现票 → `/implement`。

## 补记

- **分阶段落地**：§1–§7、§9 是**第一阶段**（工具）；§8 的资源、提示词、边角各自是一个后续阶段。
  拆票时按这个顺序，依赖边只向后。
- **三件由别的决定逼出来的前置工作**，实现票里要正面处理：
  1. **`/` 菜单动态化**（票 04 的答案带来）—— 菜单从常量条目变成「常量 + 运行时」；
  2. **`UserQuestions` 端口换成 `Arc<dyn UserQuestions + Send + Sync>`**（票 02 的答案带来）——
     一处既有接口换形状；
  3. **`rmcp` 的三处落地先验**：能不能在 `CommandWrap` 上叠进程组包装、feature 组合与体积、
     `RunningService` 的 Drop 语义是否符合预期。
- **fake-IP 代理的机器**：MCP 的连接（尤其 Streamable HTTP）会踩到与 web 那一轮同一个坑 ——
  `.scratch/web-search-tool/issues/06-fake-ip-proxy.md` 的 `trust_proxy_dns` 是那条先例；MCP 侧
  要不要一个同样的开关，在实现票里定（届时 `[web] trust_proxy_dns` 的形状可以直接照用）。
- **两份调研是材料，不是结论**：`research/` 里的事实以写就当天（2026-10-03）的外部文档为准；
  落地前重读供应商与协议侧的链接（计价、feature 名、协议版本都可能漂）。
- **决策图留着当记录**：[`map.md`](map.md) 的二十条冻结项与九张票的 `## 作答` 是这份 spec 的
  依据；实现若改变了其中任何一条，**回改 spec 与本图**，而不是只写在票的评论区里。
