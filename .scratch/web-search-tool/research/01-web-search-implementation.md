# 一手调研：web 搜索工具的实现路线

> 这是给种子材料 [`.scratch/web-search-tool/seed.md`](../seed.md) 的第 1 号 implementation research：
> 为「给模型加一个 web 搜索工具」查清事实，不替人做决定。写就于 **2026-10-02**。
> 核实过的仓库位置逐条写在正文里（`文件:行`）；上游做法引 [`docs/research/`](../../../docs/research/README.md)
> 的五份笔记与横向对比（那是**上游正文引文、一个字不改**）；外部事实给 URL。
> 本轮没有碰任何源文件，只读了它们；唯一的产出就是这份 markdown。

## 结论摘要

1. **今天出网只有一条路：`bash`。** 它的 `Effect` 恒为 `Exclusive`（[`src/tools/bash.rs:112-115`](../../../src/tools/bash.rs)），
   于是 `readonly` 拒、`ask` 问、`workspace` / `auto` 放行（[`src/permissions.rs:127-184`](../../../src/permissions.rs)）；
   沙箱只管写边界、**不管网络**（[`src/tools/sandbox.rs:299-302`](../../../src/tools/sandbox.rs)、[`docs/sandbox.md:22`](../../../docs/sandbox.md)）。
   所以模型今天打一条 `curl` 就能出网，而且 `readonly` 之外的档位连问都不问（`workspace` / `auto`）。
2. **没有「网络副作用」这个代数位。** `Effect` 描述的是**工作区**副作用（[`src/tools/tool.rs:21-33`](../../../src/tools/tool.rs)），
   `src/` 里 `network` 一词零命中，`ModelCaps` 也没有 web 能力位（[`src/provider/capability.rs:41-66`](../../../src/provider/capability.rs)）。
   可复用的形状是 hooks 的 `Rewrite`（[`src/hooks.rs:35-49`](../../../src/hooks.rs)）与断路器（[`src/permissions.rs:643-654`](../../../src/permissions.rs)），
   但它们今天都不看网络。数据外泄**没有任何防线**：Redactor 只打码 provider key 的**值**（[`src/config.rs:586-596`](../../../src/config.rs)）。
3. **供应商内置检索对本仓库现在是死路。** Kimi 有内置检索，但 `$web_search` 走非标准的 `type: "builtin_function"`
   且 **2026-10-20 退役**，替代品是独立 REST（`POST /v1/tools/search|search_pro|fetch`）或需 `POST /v1/formulas/{uri}/fibers`
   的 official tools —— 都不是「一个 OpenAI-compatible client」能顺带拿到的。DeepSeek 官方文档明说内置工具被忽略
   （[Responses API 指南](https://api-docs.deepseek.com/zh-cn/guides/responses_api)）。OpenAI 的 `web_search` / Anthropic 的
   `web_search_*` 都是 Responses / Messages 专有协议。
4. **推荐路线：自建 `web_search` + `web_fetch` 两个工具，后端接一个付费搜索 API，正文提取用 Rust crate。**
   仓库已有 `reqwest`（[`Cargo.toml:22`](../../../Cargo.toml)），结果进上下文可以直接复用
   `truncate_result` 的「预览 + 指针」约定（[`src/context.rs:336-396`](../../../src/context.rs)）与 `repo_map` 那种
   「工具自己收一刀并如实报告」（[`docs/repo-map.md:31-33`](../../../docs/repo-map.md)）。
5. **要人拍板的点有 8 条**（见 §8），核心三条：工具要不要默认可用、出网要不要一个独立开关、`readonly` 档给不给联网。

## 1. 今天的网络出口与权限代数

### 1.1 provider 的 HTTP client 怎么建

- **构造点**：[`src/provider/openai.rs:114-120`](../../../src/provider/openai.rs) ——
  `reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).build()`。
  只显式设了**连接**超时 10s，没有设整体请求超时；`reqwest::Client` 自带连接池与默认的 idle 复用行为
  （[`ClientBuilder` 文档](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html)），代码没有去调 `pool_*`。
- **TLS**：`native-tls`（feature）——[`Cargo.toml:22`](../../../Cargo.toml)：
  `reqwest = { version = "0.13", default-features = false, features = ["json", "stream", "native-tls"] }`。
- **重试/退避**：[`src/provider/openai.rs:47-87`](../../../src/provider/openai.rs) —— `RetryPolicy` 默认
  `max_attempts: 3`、`base_delay: 250ms`、`max_delay: 4s`；`backoff` 是 `base * 2^(attempt-1)` 夹在 `max_delay`。
  **只有两类错误重试**：`RateLimited`（优先用响应里的 `Retry-After`，再夹到 `max_delay`）与 `Transport`。
  重试发生在**单次 `send`** 内（[`src/provider/openai.rs:177-226`](../../../src/provider/openai.rs)），
  注释明说「上层不重跑一个回合」。
- **密钥进请求**：`bearer_auth(key)`（[`src/provider/openai.rs:187`](../../../src/provider/openai.rs)），key 取自
  `profile.api_key`（`config.toml` > 环境变量 > 缺省，[`src/config.rs:14-18`](../../../src/config.rs)；
  项目里的 `.env` **永不**加载）。缺 key 时连 client 都建不出来（`BuildError::MissingKey`，
  [`src/provider/openai.rs:108-113`](../../../src/provider/openai.rs)）。
- **打码在哪一层**：在**事件流**那一层、追加之前——`EventPayload::redact` 逐字段走
  （[`src/events.rs:504-576`](../../../src/events.rs)），`ToolCallCompleted` 的 `output` / `error` 也打
  （[`src/events.rs:553-560`](../../../src/events.rs)）。Redactor 的值集就是「配置里解析出来的每一条 provider 密钥」
  （[`src/config.rs:586-596`](../../../src/config.rs)）；`CONTEXT.md:278-280` 把它定义成
  「入流前的**值级、best-effort** 替换……而工具执行仍拿真值」。调用点在
  [`src/agent.rs:2040-2049`](../../../src/agent.rs)（打码 → 裁剪 → 溢出落盘）与
  [`src/agent.rs:2195-2209`](../../../src/agent.rs)。

### 1.2 错误怎么分类

`ProviderError` 六类（[`src/provider/mod.rs:146-162`](../../../src/provider/mod.rs)）：`Auth` / `QuotaExhausted` /
`RateLimited` / `InvalidRequest` / `Transport` / `Protocol`；`QuotaExhausted` 与 `RateLimited` **刻意分开**，
理由是厂商对两者的信号不同。HTTP 状态到类别的映射在 `classify_status`
（[`src/provider/openai.rs:824-850`](../../../src/provider/openai.rs)）：401/403 → `Auth`；402 → `QuotaExhausted`；
429 → 看文案在 `QuotaExhausted` 与 `RateLimited` 之间分流；400/404/409/422 → `InvalidRequest`；
408/425 → `Transport`；5xx → `Transport`；其余 → `Protocol`。401/403 还会被补一句跨厂商诊断
（Kimi Code 与 Kimi Open Platform 两套密钥不通用，[`src/provider/openai.rs:147-175`](../../../src/provider/openai.rs)）。

### 1.3 一次出网在权限门那里经历什么

`decide` 是一个纯函数（[`src/permissions.rs:530-640`](../../../src/permissions.rs)），顺序是：

1. **断路器先行**（[`src/permissions.rs:534`](../../../src/permissions.rs)）：`rm` 冲根/家目录、
   往 `.git` / `.ssh` / shell rc 写，硬 `Deny`，任何规则与钩子都翻不了（[`src/permissions.rs:642-654`](../../../src/permissions.rs)）。
2. **模式立场 `stance`**（[`src/permissions.rs:127-184`](../../../src/permissions.rs)）——下面这张矩阵。
3. **规则的上确界**：`deny > ask > allow`，**不看专指程度**（[`src/permissions.rs:540-562`](../../../src/permissions.rs)）。
4. **地板**：路径上限（读看 `outside_read`，写看档位）、升级申请、`never_auto_approved`、`.env` 一族
   （[`src/permissions.rs:564-629`](../../../src/permissions.rs)）。

四档 × 三类 `Effect` 的裁决矩阵（`stance` 的逐分支，[`src/permissions.rs:127-184`](../../../src/permissions.rs)）：

| 档位 | `ReadOnly` | `WritePaths`（区内 / 区外） | `Exclusive` |
| --- | --- | --- | --- |
| `readonly` | `Allow` | `Deny`（**地板**，规则降不下去） | `Deny`（同上） |
| `ask` | `Allow` | `Ask` / `Ask`（区外写另有路径地板 → `Deny`） | `Ask` |
| `workspace` | `Allow` | `Allow` / `Ask`（只这一次调用） | `Allow` |
| `auto` | `Allow` | `Allow` / `Allow`（区外写另有路径地板 → `Deny`） | `Allow` |

人类可读版同义：[`docs/permissions.md:13-32`](../../../docs/permissions.md) 的四档表。
**对出网而言关键的一格是 `Exclusive` 那一列**：`bash` 恒为 `Exclusive`（[`src/tools/bash.rs:112-115`](../../../src/tools/bash.rs)），
所以今天「联网」的成败完全等于「这条 `bash` 跑不跑」——`readonly` 拒、`ask` 问、`workspace` / `auto` 直接放行。
门判的是**工作区副作用**，没有 argv 之外的网络维度可看：`Scope::CommandPrefix` 匹配的是
`["bash", "-lc", command]` 这条 argv（[`src/permissions.rs:256`](../../../src/permissions.rs)、
[`src/permissions.rs:269-283`](../../../src/permissions.rs)），而门自己也承认 shell 的这套扫描
「是**词法、best-effort** 的：断路器存在是为了拦住事故，不是为了圈禁对手」
（[`src/permissions.rs:682-687`](../../../src/permissions.rs)）。

生效裁决还要和前置钩子取上确界（`Allow < Ask < Deny`），合并点只有一处
（[`src/hooks.rs:94-101`](../../../src/hooks.rs)、调用点 [`src/agent.rs:1855`](../../../src/agent.rs)）。

### 1.4 出网在今天的代数里有没有位置

**答：没有。** 三条证据：

- `Effect` 的文档第一句就划了界：「`effect` 描述的是**工作区**副作用，而不是「有没有副作用」」
  （[`src/tools/tool.rs:3-5`](../../../src/tools/tool.rs)）。`ReadOnly` 的含义是「只读工作区」，
  一个只发 HTTP 请求的工具会落进这一格 —— 它在今天的词汇里**看起来是最无害的那一档**。
- `src/` 里 `network` / `联网` / `outbound` / `net_` 零命中（本轮 `grep -rni` 复核）；
  `ModelCaps` 的字段全是上下文窗口、推理回放、缓存参数一类（[`src/provider/capability.rs:41-66`](../../../src/provider/capability.rs)），
  没有 `supports_web_search` 这样的位。
- 网络这件事在文档里被明确**排除在沙箱之外**：「**不管** | **网络**——`curl` 带着 key 出去这一层拦不住」
  （[`docs/sandbox.md:22`](../../../docs/sandbox.md)）；ADR 0006 的「为什么网络不在这一层」写：
  「bubblewrap 的 `--unshare-net` 能隔离网络，但加上之后 `cargo build` / `npm install` 会一起断……
  文件与网络是两套判据、两种代价，混在一版里会让两边都不可控，所以网络隔离单独排期」
  （[`docs/adr/0006-sandbox-by-bubblewrap.md:24-26`](../../../docs/adr/0006-sandbox-by-bubblewrap.md)）。
  `bwrap` 参数里确实只有 `--unshare-user/pid/ipc/uts`（[`src/tools/sandbox.rs:299-302`](../../../src/tools/sandbox.rs)）。

**可复用的形状**（这一节只列形状，不推荐选哪个）：

| 既有机制 | 位置 | 对 web 工具能不能用 |
| --- | --- | --- |
| 前置钩子 `Constraint::Rewrite(args)` | [`src/hooks.rs:39-41`](../../../src/hooks.rs) | 能改写 `query` / `url`，是**唯一**能改参数的地方；`PreHookCall` 能看到 `effect`、路径与 argv（[:136-148](../../../src/hooks.rs)） |
| 钩子只能收紧（`Tightening` 里没有 `Allow`） | [`src/hooks.rs:76-101`](../../../src/hooks.rs) | 「出网要问」可以是一个钩子；但类型上**只能是收紧**，松不回去 |
| 断路器（规则之前、无通道） | [`src/permissions.rs:642-654`](../../../src/permissions.rs) | 形状适合「域名黑名单/内网地址」这类不许批准的硬拒 |
| `Scope::Tool` / `Scope::Path` | [`src/permissions.rs:252-265`](../../../src/permissions.rs) | 可以写「`web_search` 一律 deny」或按 URL 路径 glob；但 `Scope` 里没有「域名」这一维 |
| `CommandPrefix` | [`src/permissions.rs:256`](../../../src/permissions.rs) | 对非命令类工具**不适用**：它读的是 `Call::argv`，`Tool::command` 默认 `None`（[`src/tools/tool.rs:222-224`](../../../src/tools/tool.rs)） |
| 升级手势（声明式重试 + 一次批准） | [`src/permissions.rs:586-617`](../../../src/permissions.rs)、[`docs/permissions.md:57-70`](../../../docs/permissions.md) | 形状是「放开**沙箱挂载**」，与网络无关；改名复用会扭曲它的语义 |

**数据外泄今天没有任何防线。** 打码只管「配置里解析出来的密钥值」（[`src/config.rs:586-596`](../../../src/config.rs)），
不管工作区内容；`docs/permissions.md:44-50` 也把打码定性为「**值级 best-effort**」，并说
`outside_read` 缺省是 `deny` 正是为了保住 `~/.config/fs-agent/config.toml`。也就是说：
**今天把一段工作区代码贴进 `query` 发出去，没有任何一层会看一眼。**

## 2. 上游怎么做

以下全部出自 [`docs/research/`](../../../docs/research/README.md)（上游正文引文，一个字不改；行号即该文件的当前行号）。
注意 `docs/research/README.md:18-21` 的提醒：这些笔记**不加维护**，是当时读到了什么，不是现状的描述。

### Claude Code（`notes/claude-code-amp.md`）

- 内建工具里有 `WebFetch` 与 `WebSearch`，与 `Bash`、`Read`、`Agent` 等并列
  （[`docs/research/notes/claude-code-amp.md:71-75`](../../../docs/research/notes/claude-code-amp.md)）；
  横向对比里把它归为「领域专用工具」（[`docs/research/coding-agent-features.md:95`](../../../docs/research/coding-agent-features.md)）。
- **权限规则按域名匹配**：规则语法是 `Tool(pattern)`，其中就有 `WebFetch(domain:host)`
  （[`notes/claude-code-amp.md:236-239`](../../../docs/research/notes/claude-code-amp.md)）——
  这是本轮看到的**唯一**一个把域名做进权限谓词的实现。
- 匹配「工具的主内容字段」被**拒绝**，理由对 web 工具同样成立：
  <blockquote>

  Matching a tool's *primary content* field is rejected: `Bash(command:rm *)` "would be bypassable by a
  compound command, so Claude Code ignores it and emits a startup warning." Primary fields are
  `command` (Bash/PowerShell), `file_path` (Read/Edit/Write), `path` (Grep/Glob), `notebook_path`,
  `url`.
  </blockquote>

  （[`notes/claude-code-amp.md:243-247`](../../../docs/research/notes/claude-code-amp.md)，
  即 `url` 是 web 工具的主内容字段。）
- 收紧档 `--restricted` 会**移除 WebFetch**（[`notes/claude-code-amp.md:315-318`](../../../docs/research/notes/claude-code-amp.md)）。
- 沙箱的凭据保护是独立一层（`sandbox.credentials.envVars` 等），且「independent of the filesystem layer」
  （[`notes/claude-code-amp.md:301-307`](../../../docs/research/notes/claude-code-amp.md)）。
- **必须走工具、不许裸 curl** 这条硬约束：笔记里没有这条强制；能确认的只有
  「WebFetch 可以被权限规则按域名管」与「`--restricted` 里它整个消失」。

### Amp（同一份笔记的 §2）

- 内建工具表的权威列表里同时有 `read_web_page` 与 `web_search`
  （[`notes/claude-code-amp.md:846-849`](../../../docs/research/notes/claude-code-amp.md)）。
- 搜索后端**不是自家**：Amp 公布的基础设施里写「Parallel powers web search/retrieval」
  （[`notes/claude-code-amp.md:1304`](../../../docs/research/notes/claude-code-amp.md)）。
- 计价：非模型工具（如 web search）也消耗额度（[`notes/claude-code-amp.md:1256-1258`](../../../docs/research/notes/claude-code-amp.md)）。
- 防注入姿态是「纵深防御」并且**明说免责**：Parallel 提供 web 上下文、自动密钥打码、线程审计轨迹等，
  而 prompt-injection 报告「explicitly **out of scope for bug bounties** "due to LLMs' inherent nature
  and Amp's code execution capabilities"」（[`notes/claude-code-amp.md:1018-1021`](../../../docs/research/notes/claude-code-amp.md)）。
- 它默认不问，且自己承认「Untrusted repositories, MCP servers, and other external inputs can
  influence what Amp does」（[`notes/claude-code-amp.md:972-974`](../../../docs/research/notes/claude-code-amp.md)）。

### Codex（`notes/codex-gemini.md` §2）

- `web_search` 在发给模型的工具表里，但**形状不是 function tool**：
  <blockquote>

  `update_plan`, `view_image`, `web_search` (**a hosted/server-side tool spec, not a function tool**),
  </blockquote>

  （[`notes/codex-gemini.md:591-593`](../../../docs/research/notes/codex-gemini.md)。）
- provider 配置里有一个能力位 `supports_standalone_web_search`
  （[`notes/codex-gemini.md:995-998`](../../../docs/research/notes/codex-gemini.md)）——
  「provider 支不支持独立 web search」是**配置面**上的一等事实。
- 事件流里 `web_search` 是 item 类型之一（[`notes/codex-gemini.md:927-930`](../../../docs/research/notes/codex-gemini.md)）。
- Bedrock 那条 provider 明确不支持 web search（[`notes/codex-gemini.md:1019-1021`](../../../docs/research/notes/codex-gemini.md)）。
- **网络是被写进策略引擎的**：Codex 的执行策略用 Starlark，有 `prefix_rule` **与 `network_rule`**，
  判定为 `Allow`/`Prompt`/`Forbidden`——「策略即数据，可以在不重编译的情况下改」
  （[`docs/research/coding-agent-features.md:272`](../../../docs/research/coding-agent-features.md)）。
  这是本轮唯一一个**把网络单列成一条规则**的实现。

### Gemini CLI（`notes/codex-gemini.md` §1）

- 工具表（来自上游 `base-declarations.ts`）：`google_web_search` 的参数是 `query`，
  `web_fetch` 的参数是 **`prompt`**（[`notes/codex-gemini.md:96-102`](../../../docs/research/notes/codex-gemini.md)）——
  注意 `web_fetch` 收的不是 URL 而是 prompt，说明它是「取页面并让模型先处理」的形状（笔记只给了参数名，
  没展开语义）。
- **plan 模式会拦下 web fetch**：`PLAN_MODE_TOOLS` 是只读白名单，「web fetch requires explicit confirmation in plan mode」
  （[`notes/codex-gemini.md:322-326`](../../../docs/research/notes/codex-gemini.md)）。
- 权限另有 TOML 策略引擎（tier 1–5，动词 `allow`/`deny`/`ask_user`）
  （[`coding-agent-features.md:274`](../../../docs/research/coding-agent-features.md)）。

### aider（`notes/aider-openhands.md` §1）

- **没有工具协议上的 web 工具**；web 抓取是一个**用户调用的斜杠命令**：
  「`/web` (scrape page → markdown)」（[`notes/aider-openhands.md:83-86`](../../../docs/research/notes/aider-openhands.md)）。
  换句话说：模型自己不能联网，人可以。

### OpenHands（同一份笔记的 §2/§3）

- V0：`BrowserTool` 是一个真工具，配置门 `enable_browsing` **默认 True**
  （[`notes/aider-openhands.md:405-418`](../../../docs/research/notes/aider-openhands.md)）。
- 有专门的 `browser_output_condenser` —— 浏览器输出是**被单独压缩的一类上下文**
  （[`notes/aider-openhands.md:427-431`](../../../docs/research/notes/aider-openhands.md)）。
- V1 SDK 的工具表里有 `browser_use`（[`notes/aider-openhands.md:715`](../../../docs/research/notes/aider-openhands.md)）。

### Cline（`notes/cline-continue.md`）

- 浏览器是一个**逐工具类别的 auto-approve 开关**：「Use the browser | Browser tool for web fetching and searching」，
  机器可读键是 `use_browser`（[`notes/cline-continue.md:421-427`](../../../docs/research/notes/cline-continue.md)、
  [`notes/cline-continue.md:432-436`](../../../docs/research/notes/cline-continue.md)）。
- 子代理是**只读的**，明确「Forbidden: … using the browser, accessing MCP servers, **performing web searches**」
  （[`notes/cline-continue.md:1279-1283`](../../../docs/research/notes/cline-continue.md)）——
  「联网」在这家被当作与「写文件」同级的能力来禁。

### Continue（同一份笔记）

- 本轮没有检索到 Continue 的 web 搜索/抓取能力条目；笔记里与 web 相关的命中的是安装脚本的 `curl`
  （[`notes/cline-continue.md:2625`](../../../docs/research/notes/cline-continue.md)）。
  按「没找到就当没有」记。

### opencode（`notes/opencode-goose.md` §2）

- 内建工具表里有 `webfetch` 与 `websearch`
  （[`notes/opencode-goose.md:120-123`](../../../docs/research/notes/opencode-goose.md)）。
- **后端与门控**：
  <blockquote>

  `websearch` is only available with the OpenCode provider or when `OPENCODE_ENABLE_EXA` /
  `OPENCODE_ENABLE_PARALLEL` is truthy; it hits a hosted MCP service with no API key.
  </blockquote>

  （[`notes/opencode-goose.md:163-165`](../../../docs/research/notes/opencode-goose.md)。）
  即：`websearch` 由 Exa 或 Parallel 供能，而且**要环境变量或特定 provider 才出现**——
  这是一条「工具表按条件组装」的先例。
- **权限键里单列了它们**：`webfetch`（按 URL 匹配）、`websearch`（按 query 匹配）、`external_directory`、`doom_loop`
  （[`notes/opencode-goose.md:374-377`](../../../docs/research/notes/opencode-goose.md)）；
  agent frontmatter 里可以写 `permission: {…, webfetch: deny}`
  （[`notes/opencode-goose.md:404-407`](../../../docs/research/notes/opencode-goose.md)）。
- `doom_loop` 的形状值得抄：「fires when the same tool call repeats 3 times with identical input」
  （[`notes/opencode-goose.md:398-400`](../../../docs/research/notes/opencode-goose.md)）。

### goose（同一份笔记 §2/§5）

- Developer 扩展**只有五个工具**：`write`、`edit`、`shell`、`tree`、`read_image`
  （[`notes/opencode-goose.md:169-180`](../../../docs/research/notes/opencode-goose.md)）——
  **没有 web 工具**；联网在 goose 里要么走 shell，要么走 MCP 扩展。
- 但它有安全件：`adversary mode`、**prompt-injection detection**、classification API
  （[`notes/opencode-goose.md:449-455`](../../../docs/research/notes/opencode-goose.md)）；
  smart-approval 的分类器自称是「injection-aware read-only judge」
  （[`notes/opencode-goose.md:444-447`](../../../docs/research/notes/opencode-goose.md)）。

### 归纳：上游在这件事上的四个公约数

1. **要么内建一对 `web_search` + `web_fetch`，要么干脆没有**（aider、goose 没有；Claude Code、Amp、
   Gemini CLI、opencode、OpenHands 有）。Codex 有，但是 hosted 形状。
2. **搜索后端几乎都是第三方**：Amp→Parallel、opencode→Exa/Parallel、Codex/OpenAI→自家 hosted。
   没有一家自己从零建索引。
3. **联网能力被单列进权限/配置**：Claude Code 的 `WebFetch(domain:…)`、opencode 的 `webfetch`/`websearch`
   权限键、Cline 的 `use_browser` 开关、Gemini 的 plan 模式确认、Codex 的 `network_rule` 与
   `supports_standalone_web_search`。**没有一家把联网塞进「文件写权限」里顺带管。**
4. **「必须走工具、不许裸 curl」这条强制，本轮没有在任何一家里找到明文**；能确认的只是
   「工具存在 + 工具可以被权限管住」。反过来，Cline 的子代理禁网、Gemini 的 plan 模式确认，
   说明「禁/问」是逐工具的，不是靠禁止 shell。

## 3. 供应商内置检索

**这一节对本仓库最重要的一句结论：能用的内置检索都要求非 `/v1/chat/completions` 的协议面，
或者要求供应商专有端点。** 本仓库只做「一个 OpenAI-compatible client」（`src/provider/openai.rs` 只有一个
`chat_completions_url(base_url)`，[`src/provider/openai.rs:254`](../../../src/provider/openai.rs)），
所以这些事实要如实计入成本。

### Kimi / Moonshot

| 通道 | 形态 | 参数面 | 计价 | 状态 |
| --- | --- | --- | --- | --- |
| `$web_search`（legacy） | `tools: [{"type": "builtin_function", "function": {"name": "$web_search"}}]`，模型返回 `tool_calls`，调用方把 `arguments` **原样**回填成 `role: tool` | 调用方不给参数；模型自己生成，`arguments` 里额外带 `usage.total_tokens` 报告搜索结果占了它多少 token | **$0.005/call**，且搜索内容另按 token 计（`total_tokens = prompt_tokens + search_tokens + completion_tokens`） | **2026-10-20 退役** |
| 官方 tool `web-search`（Formula API） | 先 `GET /v1/formulas/{uri}/tools` 拿声明（`uri` 如 `moonshot/web-search:latest`），再当**标准 `function` tool** 发给 `POST /v1/chat/completions`，执行走 `POST /v1/formulas/{uri}/fibers` | 声明里给；执行时 `name` + `arguments` 原样透传 | 按 call 计（页面说 official tools 目前限时免费，「except `web-search`, which is billed per call」） | 推荐路径，但多了两个**专有端点** |
| 独立 REST | `POST /v1/tools/search`（Basic）、`POST /v1/tools/search_pro`（Pro）、`POST /v1/tools/fetch` | Basic 返回 title/url/snippet，`include_content=true` 时带页面正文；Pro 返回**按 query 排好序的段落**（`chunks`）并带 `authority` 字段，支持 `sites`（限定站点）与 `time_window`（时间范围）；fetch 返回 title + Markdown 正文 | Basic **$0.002/call**、Pro **$0.003/call**、URL Fetch **$0.002/call**；成功且非空才计费 | 「Long-term support」，官方推荐新集成用它 |

出处：[Use Web Search with the `$web_search` Built-in Tool](https://platform.kimi.ai/docs/guide/use-web-search)、
[WebSearch Pricing](https://platform.kimi.ai/docs/pricing/websearch)、
[Best Practices for Web Search](https://platform.kimi.ai/docs/guide/best-practices-for-web-search)、
[How to Use Official Tools in Kimi API](https://platform.kimi.ai/docs/guide/use-official-tools)（2026-10-02 抓取）。

要点：**`$web_search` 的「原样回填 arguments」是一个很聪明的兼容设计**——调用方不实现搜索也能跑通，
换成自己的实现只需改 `search_impl`（同一页的「Switch to your own search implementation」一节）。
对本仓库来说，这条设计正好是「自建后端」的接口模板；但协议标记 `type: "builtin_function"` 不是
OpenAI 的标准值，今天这个 client 不会发出它（`build_body` 只把 `ToolSpec` 转成 `function` 形状，
[`src/provider/openai.rs:417`](../../../src/provider/openai.rs)）。另外注意 Kimi Code（编程套餐）与
Kimi Open Platform 是两套密钥、两套 base URL（[`src/provider/openai.rs:165-171`](../../../src/provider/openai.rs)），
上面的检索能力属于 Open Platform 那一套。

### DeepSeek

- **Chat Completions 面没有内置检索**；2024-12 的旧新闻页写得很直白：「目前，API 暂不支持搜索功能」
  （[DeepSeek V2 系列收官，联网搜索上线官网](https://api-docs.deepseek.com/zh-cn/news/news1210/)）。
  那一条讲的是网页端（`chat.deepseek.com` 的「联网搜索」开关），**不是 API**。
- **Responses API 面也没有**：兼容性表里 Tools 一节写 `web_search` / `file_search` / `code_interpreter` /
  `computer_use` / `mcp` 等内置工具「**忽略**」，`tools` 的 `Possible values` 只有 `function`
  （[Responses API](https://api-docs.deepseek.com/zh-cn/api/create-response)、
  [使用 Responses API](https://api-docs.deepseek.com/zh-cn/guides/responses_api)）。
  DeepSeek 从 2026-07 起原生支持 Responses API 并适配 Codex（[更新日志 2026-07-31 / 2026-08-13](https://api-docs.deepseek.com/zh-cn/updates)），
  但那是**格式兼容**，不含 hosted 检索。
- 一个旁证：Responses 指南里有一句「`input` 中回传的 `web_search_call` item（例如旧模型此前请求产生的搜索结果）
  仍会被还原并拼接进上下文」——即服务端**认这个 item 形状**，但当前工具表把它忽略。
  （同一页；这条与「内置工具被忽略」并列时，应按后者理解：客户端可以继续回传，服务端不产生新的搜索。）
- 网上的第三方 issue/PR 声称给 DeepSeek 加了「hosted web_search」（例如
  [oh-my-pi#7794](https://github.com/can1357/oh-my-pi/issues/7794)、
  [DeepSeek-Reasonix#7466](https://github.com/esengine/DeepSeek-Reasonix/pull/7466)），
  这些**不是** DeepSeek 官方文档，按「未证实」对待。

**结论**：今天在 DeepSeek 上要联网，只能**自建工具**（或经 `bash` 跑 `curl`）。

### Anthropic（专有协议，作为形态参照）

- 工具名带版本：`web_search_20250305`（basic）、`web_search_20260209`（加 dynamic filtering）、
  `web_search_20260318`（加 response inclusion）
  （[Web search tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-search-tool)）。
- 参数面：`max_uses`（硬上限，「For a hard constraint, use `max_uses` to cap the number of searches
  for each request」）、`allowed_domains` / `blocked_domains`、`user_location`、`allowed_callers`。
- 结果回填：搜索结果变成输入 token，引用以 citations 形式带在回复里（文档在同一页；
  计价页也说明「Web search results retrieved throughout a conversation are counted as input tokens」）。
- 计价：**$10 per 1,000 searches**，每次搜索算一次「use」，「regardless of the number of results
  returned」；搜索出错不计费（[Pricing § Web search tool](https://platform.claude.com/docs/en/about-claude/pricing)）。
- 对照组：**web fetch 不收工具费**，只付 token，并且有 `max_content_tokens` 兜住超大页面；
  文档还给了经验值：平均网页 10 kB ≈ 2,500 token，大文档页 100 kB ≈ 25,000 token，论文 500 kB ≈ 125,000 token
  （同一页）。**这两个数字直接可用在我们自己的预算设定上。**
- 这是 **Messages API 的 server tool**，不是 OpenAI 形状。本仓库的 client 伸不到。

### OpenAI（专有协议 + 一个 Chat Completions 的窄门）

- 新集成走 **Responses API** 的 hosted tool `{"type": "web_search"}`；老名字 `web_search_preview`
  仍在但不支持 `filters` / `external_web_access` / `return_token_budget`
  （[Web search](https://developers.openai.com/api/docs/guides/tools-web-search)）。
- 参数面（比 Anthropic 更宽）：`filters.allowed_domains` / `blocked_domains`（各最多 100 个，
  写域名不带协议、含子域）、`search_context_size`（`low`/`medium`/`high`）、
  `return_token_budget`（`default`/`unlimited`）、`external_web_access`（`false` = 只用缓存索引）、
  `user_location`、`include: ["web_search_call.action.sources"]`（拿**全部**访问过的 URL，
  比 inline citation 全）、图片搜索（`search_content_types` / `image_settings`）。
- 结果形状：`web_search_call` output item（`action` 是 `search`/`open_page`/`find_in_page`）+
  `message` 里的 `url_citation` 注解（有 `start_index`/`end_index`/`url`/`title`）。
  文档还硬性要求：「When displaying web results or information contained in web results to end users,
  inline citations must be made clearly visible and clickable in your user interface.」（同一页）
- **Chat Completions 的限制（对本仓库是决定性的）**：
  <blockquote>

  The Chat Completions API supports only specialized search models for web search. These models do not
  support Responses API `web_search` features such as domain filters, complete source lists, live-access
  control, and returned-token budget control.
  </blockquote>

  也就是只能换成 `gpt-5-search-api` 这种**专用模型**（`gpt-4o-search-preview` /
  `gpt-4o-mini-search-preview` 已在 2026-07-23 关停），而「Chat Completions search models always search
  before responding; Responses search is a tool」（同一页）。
  **在一个 `chat/completions` client 里塞 hosted `web_search` 工具是不成立的**；能做的只有
  「把模型换成 search 专用模型」，那是另一条产品决策，不是加工具。
- 计价见 [Built-in tools](https://developers.openai.com/api/docs/pricing#built-in-tools)（本轮没有逐字核实数字）。

### Google Gemini（参照）

- Grounding with Google Search 的计价（检索到的定价页摘要）：免费额度最多 500 RPD（与 Flash-Lite 共用），
  付费档 1,500 RPD 之后**每 1,000 个 grounded prompts $35**
  （[Gemini Developer API pricing](https://ai.google.dev/gemini-api/docs/pricing)）。
  这条是按「grounded prompt」而不是按「search call」计价，与 Anthropic/OpenAI 的口径不同。

### OpenAI-compatible 生态里的通行形态

- **`/v1/chat/completions` 上没有 host 检索，只有 function tool。** 通行做法是把搜索做成一个普通的
  `function` 工具交给模型，然后**由客户端执行**——Kimi 文档的「Switch to your own search implementation」
  一节把这条路写成了官方推荐的两步（改成自己的 `name`/`description`/`parameters`，再实现 `search` 与 `crawl`）
  （[Use Web Search](https://platform.kimi.ai/docs/guide/use-web-search)）。
- 这也正是 Anthropic 官方文档在讲 MCP 之外的另一条路：`/docs/en/build-with-claude/search-results` 是把
  **自己的**搜索结果按 citations 形状喂给模型（不在本轮展开）。
- **服务端代跑型**：OpenRouter 的 `:online` 这类「模型名后缀即联网」属于网关侧聚合，本轮没有一手核实，
  只作为已知形态列出，**不作为事实引用**。

## 4. 搜索 API 对照

口径：**本仓库只做 OpenAI-compatible client**，所以「接一个搜索 API」永远是**客户端自己发 HTTP**，
与 provider 无关；密钥要自己存、自己打码。

| 供应商 | 密钥获取 | 免费额度 / 计价（2026-10 文档口径） | 结果形状 | 备注 |
| --- | --- | --- | --- | --- |
| **Brave Search API** | [注册](https://api-dashboard.search.brave.com/register)（有 dashboard，无需企业审批） | **$5/1,000 requests**；每月自动给 **$5 免费额度**（≈1,000 次）；Search 50 QPS，Answers 2 QPS | Search 端点返回完整结果（URL、text、news、images），另有 `llm/context` 端点专门为 AI 优化；「Extra alternate snippets」「Schema-enriched results」 | 认证头 `X-Subscription-Token`，`GET https://api.search.brave.com/res/v1/llm/context?q=…`；有官方 MCP server。见 [brave.com/search/api](https://brave.com/search/api/) |
| **Tavily** | [app.tavily.com](https://app.tavily.com) 自助注册，**不要信用卡** | **1,000 credits/月免费**；PAYGO **$0.008/credit**；月付档 $30/4,000 起；Search `basic` = 1 credit，`advanced` = 2 credits | Search 返回结果列表；另有 Extract / Map / Crawl / Research 端点，Extract 按 URL 抽正文（每 5 个成功 URL 1 credit，basic） | 有 keyless 试用档。见 [Credits & Pricing](https://docs.tavily.com/documentation/api-credits) |
| **SearXNG（自建）** | 无密钥，自己部署 | 软件免费（AGPL）；成本是**服务器**与你的维护 | `format=json` 返回聚合结果；参数有 `q`、`categories`、`language`、`pageno`、`time_range`（`day`/`month`/`year`）、`safesearch` | **`format=json` 必须在 `settings.yml` 的 `search:` 一节里启用，否则 403 Forbidden**；「many public instances have these formats disabled」。见 [Search API](https://docs.searxng.org/dev/search_api.html) |
| **Google Programmable Search（Custom Search JSON API）** | 需要 API key + Search Engine ID | **对 2026 年的新客户已不可用**；存量客户：**100 查询/天免费**，超出 **$5/1,000**，上限 10,000/天，**服务 2027-01-01 关停**，存量客户要在此之前迁移 | 返回 JSON（web 或 image 搜索） | 这条**不该再选**。见 [Custom Search JSON API](https://developers.google.com/custom-search/v1/overview)（页面注记：not available for new customers；existing customers have until 2027-01-01） |
| **Exa** | [dashboard.exa.ai](https://dashboard.exa.ai) 自助注册 | 免费档 **$10 credits/月** + $10 onboarding bonus（「over $120 in credits per year」）；Search **$4/1k requests** 起（Instant），Fast/Auto $7、Deep $12、Deep-Reasoning $15，超出 10 条结果每条再 $1/1k；Contents **$1/1k pages**；AI page summaries 再 $1/1k pages | Search 返回网页文本与 highlights，可配延迟（180ms–1s）；Contents 端点专门取全文 | 免费档 10 QPS。见 [Pricing](https://exa.ai/pricing)、[Billing & Rate Limits](https://exa.ai/docs/admin/billing) |
| **Kimi 独立 REST**（如果人愿意把搜索也交给 Kimi） | 已有 Moonshot 密钥 | Basic $0.002/call、Pro $0.003/call、fetch $0.002/call，**失败或空结果不计费** | Basic：title/url/snippet；Pro：与 query 相关的**段落**（`chunks`）+ `authority`；fetch：title + Markdown | 支持 `sites` / `time_window`（Pro）。见 §3 的链接 |

**对照与推荐（这只是推荐，不是决定）**：

- 若只想要「一个能用的搜索 + 现成的正文摘要」：**Tavily** 的免费额度够自用，`advanced` 直接返回可喂模型的
  内容，工程量最小；**Brave** 在「结果质量/速度/单价」上最像通用搜索，`llm/context` 端点是给 agent 用的。
- 若要**不依赖第三方、不被计价绑定**：**SearXNG 自建**，代价是要自己维护、并且上游（Google/Bing 等）
  可能封 IP —— 这一点是通行风险，SearXNG 自己的文档目录里就有 [Answer CAPTCHA from server's IP](https://docs.searxng.org/admin/answer-captcha.html)
  与 [Limiter](https://docs.searxng.org/admin/searx.limiter.html) 两节，说明这不是假想问题。
- 若要做**语义/代码检索**而不是「搜网页」：**Exa** 的索引与 highlights 更贴（它自己宣传的 use case 第一条
  就是 coding agents）。
- **Google CSE 直接从候选里划掉**（新客户无门、2027 关停）。
- 许可与条款：本轮只核实到各家的**计价页**，没有逐家读 ToS 里「能否缓存结果 / 能否用于训练」的条款；
  这一项**未决**，写在 §8。

## 5. 自建抓取这条路

「自己抓」= `reqwest` 取 HTML + 抽正文 + 转 Markdown/纯文本。crate 的版本与更新时间是 2026-10-02
从 crates.io API 读的。

| crate | 最新稳定版 | 更新时间 | 用途 |
| --- | --- | --- | --- |
| [`scraper`](https://crates.io/crates/scraper) | 0.27.0 | 2026-05-11 | CSS 选择器解析/查询（HTML parsing and querying with CSS selectors） |
| [`dom_smoothie`](https://crates.io/crates/dom_smoothie) | 0.18.2 | **2026-09-21** | 「extracting relevant content from web pages」——Readability 系里维护最活跃的一个 |
| [`readability`](https://crates.io/crates/readability) | 0.3.0 | 2023-12-20 | arc90 Readability 的 Rust 移植；**三年多没动** |
| [`article_scraper`](https://crates.io/crates/article_scraper) | 2.3.1 | 2026-03-01 | 整条链：下载 + 正文抽取（fivefilters full-text-feed 配置 + mozilla readability） |
| [`html2text`](https://crates.io/crates/html2text) | 0.17.1 | 2026-04-19 | HTML → 纯文本（也有 Markdown 渲染器） |
| [`select`](https://crates.io/crates/select) | 0.6.1 | 2025-03-19 | 从 HTML 抽数据（老牌） |
| [`lol_html`](https://crates.io/crates/lol_html) | 3.0.1 | 2026-07-29 | Cloudflare 的流式 HTML 重写器（要改/删节点时用） |
| [`ammonia`](https://crates.io/crates/ammonia) | 4.2.0 | 2026-09-17 | HTML 净化（如果要把抓来的 HTML 渲染进 TUI） |
| [`robotstxt`](https://crates.io/crates/robotstxt) | 0.3.0 | **2021-02-13** | Google robots.txt 解析器的移植；旧 |
| [`texting_robots`](https://crates.io/crates/texting_robots) | 0.2.2 | **2023-03-29** | 另一个 robots.txt 解析器，单元测试很足；也旧 |
| [`chardetng`](https://crates.io/crates/chardetng) | 1.0.0 | 2026-03-30 | 遗留网页的字符编码探测 |
| [`encoding_rs`](https://crates.io/crates/encoding_rs) | 0.8.42 | 2026-09-24 | Encoding Standard 的实现，与 `chardetng` 配套 |

**自己抓必须自己处理的东西**（不是 crate 能替你决定的）：

1. **robots.txt**：抓之前读 `https://host/robots.txt`、按 UA 判 Allow/Disallow。协议是
   [RFC 9309](https://www.rfc-editor.org/rfc/rfc9309.html)；两个 Rust 解析器都年久（见上表），
   所以这一条要么接受旧 crate，要么自己写一个子集。
2. **限流与退避**：按 host 排队、串行化、尊重 `Retry-After`。**本仓库已有这个形状可以抄**——
   `RetryPolicy` + `parse_retry_after`（[`src/provider/openai.rs:47-87`](../../../src/provider/openai.rs)、
   [:895-905](../../../src/provider/openai.rs)）。另外要定「一次 `web_fetch` 最多跟几个重定向」。
3. **重定向**：`reqwest` 默认跟 10 跳；要显式限制，并且**每一跳都要重跑 SSRF 判据**（见下）。
4. **压缩**：`gzip`/`br`/`zstd` 需要开 feature（今天的 `reqwest` 特性只有 `json`/`stream`/`native-tls`，
   [`Cargo.toml:22`](../../../Cargo.toml)）——**这是要改 `Cargo.toml` 的**。
5. **编码**：`Content-Type` 的 charset 优先，缺了用 `chardetng` 探测，再交给 `encoding_rs` 解码。
6. **正文提取**：`scraper` + `dom_smoothie`（或 `article_scraper`）；要有「抽不出正文时退化成
   `<title>` + 纯文本」的分支。**转成什么进上下文**是 §6 的约定问题（见下一节）。
7. **JS 渲染**：**这条路默认没有**。要跑 headless browser 就是引入 Chromium 级别的依赖，
   与「自用 CLI」的体量不符。UI 明确写「这条工具看不见 JS 渲染出来的内容」。
8. **UA 与身份**：要写一个诚实的 UA（含项目名/版本），不要伪装浏览器。
9. **内容长度上限**：Anthropic 给的经验值可以直接拿来定预算——10 kB ≈ 2.5k token、
   100 kB ≈ 25k token、500 kB ≈ 125k token（[Pricing § Web fetch tool](https://platform.claude.com/docs/en/about-claude/pricing)）。
10. **SSRF 防护**：**这是自建抓取里最不能省的一条。** 模型给的 URL 可以指向
    `127.0.0.1`、`169.254.169.254`（云元数据）、局域网网段或内网域名。通行做法是：
    解析 DNS 后校验目标 IP 不在私网/环回/链路本地/保留段；禁止非 `http(s)` scheme；
    **重定向后的目标也要重新校验**；不要在返回内容里回显内网响应。参考
    [OWASP SSRF Prevention Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html)
    与 [SSRF Prevention in Node.js](https://owasp.org/www-community/pages/controls/SSRF_Prevention_in_Nodejs.html)
    （后者虽是 Node 视角，但「先解析再校验、每跳都校验」这条路子是一样的）。
    对本仓库还有一层：「沙箱不管网络」（[`docs/sandbox.md:22`](../../../docs/sandbox.md)），
    所以 SSRF 没有第二道网。

## 6. 结果进上下文的既有约定

这套约定**已经存在**，web 工具没有理由另起一套：

- **裁剪 + 溢出落盘 + 指针**：`truncate_result` 在结果进流**之前**跑，超预算就把全文写到
  `<outputs_dir>/<tool_call_id>.txt`（**带打码**），流上只留「头 + `[已截断：N 字符，约 M token；全文在 …]` + 尾」
  （[`src/context.rs:336-396`](../../../src/context.rs)，标记常量 `TRUNCATED_MARKER`
  [`src/context.rs:403`](../../../src/context.rs)）。两条诚实性细节值得照抄：正文小到
  「预览加指针说明比正文还长」时**整条留下**；溢出写不出去时降级成「只有预览」
  （[`src/context.rs:334-335`](../../../src/context.rs)）。
- **预算字段**：`SessionConfig::max_tool_result_tokens`（缺省 `DEFAULT_MAX_TOOL_RESULT_TOKENS = 25_000`，
  [`src/config.rs:57`](../../../src/config.rs)、[`src/config.rs:1738-1739`](../../../src/config.rs)）与
  `repo_map_tokens`（缺省 1_024、上限 `MAX_REPO_MAP_TOKENS`，
  [`src/config.rs:1740-1742`](../../../src/config.rs)）。同一份配置里还有 `bash_timeout_ms` /
  `max_bash_timeout_ms`（[:1743-1748](../../../src/config.rs)）——
  **「工具的上限来自会话配置，不是模型参数」是本仓库的既有立场**，web 工具的长度上限/超时应当照办。
- **工具自己收一刀并如实报告的先例**：`repo_map` 的预算是配置、没有 `tokens` 参数、模型硬塞的键被忽略
  （[`src/tools/repo_map.rs:65-81`](../../../src/tools/repo_map.rs)），而文档写明装不下的地图
  「按**整个符号**的边界切，末尾一行写出被省掉多少」（[`docs/repo-map.md:31-33`](../../../docs/repo-map.md)）。
  **「截在语义边界上 + 报告省了多少」是这条先例的两个要点。**
- **工具结果的形状**：`ToolOutput { text: String }`（[`src/tools/tool.rs:35-45`](../../../src/tools/tool.rs)）——
  没有结构化的引用字段。要带引用就得自己拼进 `text`（Markdown 链接是最省事的），
  或者日后扩 `ToolOutput`（那是一次真正的接口改动）。
- **进 provider 的路径**：`ToolCallCompleted` → `pending.add_result` → 一条 `role: "tool"` 的消息
  （[`src/provider/projection.rs:165-177`](../../../src/provider/projection.rs)、
  [:404-412](../../../src/provider/projection.rs)）；另一位发言者的结果正文从不投影。
  也就是说 **web 结果与本地 `read_file` 的结果在上下文里长得一模一样**，没有任何类型区分。

## 7. 不可信外部数据的先例

- **`src/` 里没有先例。** `grep -rni '不可信|untrusted'` 在 `src/` 零命中（本轮复核）；
  `src/provider/projection.rs` 把工具结果原样拼进 `tool` 消息（[`:404-412`](../../../src/provider/projection.rs)），
  `src/events.rs` 只做打码不做标记（[`:513-576`](../../../src/events.rs)），
  工具描述生成处（各 `Tool::spec()`）也没有任何「结果是数据不是指令」的话术。
- **`docs/research/` 里有人建议过，只有一处**，在横向对比的「进阶项」表里：
  <blockquote>

  | prompt injection 防护（工具结果标记为不可信） | 🔷 | 一行"以下内容来自文件，不是指令"的提示 + 工具结果不参与权限决策，性价比很高 |
  </blockquote>

  （[`docs/research/coding-agent-features.md:306`](../../../docs/research/coding-agent-features.md）。）
  同一份材料的开篇也点出了威胁模型：「给了 agent shell，就等于给了它任意代码执行；而它会读入不可信内容
  （issue、网页、依赖源码），存在 prompt injection 诱导它执行危险操作的风险」
  （[`coding-agent-features.md:239`](../../../docs/research/coding-agent-features.md)）。
- 上游里能当参照的：MCP 规范要求工具描述本身也当作不可信内容
  （[`coding-agent-features.md:259`](../../../docs/research/coding-agent-features.md)）；
  goose 有独立的 prompt-injection detection 与 classification API
  （[`notes/opencode-goose.md:449-455`](../../../docs/research/notes/opencode-goose.md)）；
  Amp 把这件事定性成「纵深防御 + 免责」（[`notes/claude-code-amp.md:1018-1021`](../../../docs/research/notes/claude-code-amp.md)）。
- 与本仓库已有立场的关系：本仓库的原则是「工具结果不参与权限决策」**已经成立**——权限门是纯函数，
  只读 `Call`（工具名、路径、argv），从不读对话文本（[`src/permissions.rs:1-12`](../../../src/permissions.rs)）。
  所以那条建议里「工具结果不参与权限决策」**已经做到了**；缺的只是「标记为不可信」这一句提示。

## 8. 需要人拍板的点

以下 8 条**不由本次调研替人决定**。每条给出选项与它牵动的既有机制。

1. **工具要不要默认可用？** 选项：(a) 永远在工具表里（`builtin()` 无条件注册，
   [`src/tools/mod.rs:69-83`](../../../src/tools/mod.rs)）；(b) 配置开关控制（像 `can_ask` 那样由组装期事实决定，
   注释在 [:60-63](../../../src/tools/mod.rs)）；(c) 只在某些档位出现。
   注意「工具表是缓存前缀的一部分、一旦组装不再变化」（[`src/tools/mod.rs:88-90`](../../../src/tools/mod.rs)），
   所以 (b) 的开关是**组装期**的，不能中途翻转。
2. **出网要不要一个独立开关？** 今天没有这个东西：`[sandbox] mode = "off"` 关的是文件沙箱、不是网络
   （[`docs/sandbox.md:22`](../../../docs/sandbox.md)）。要加就要定它叫什么、默认值是什么、
   与 `bash` 已有的出网能力是什么关系（**注意：关掉 web 工具并不等于关掉出网，`bash` 仍能 `curl`**）。
3. **`readonly` 档给不给联网？** 这一条的答案取决于 `Effect` 怎么归类：
   若新工具 `effect()` 答 `ReadOnly`，`readonly` 档**自动放行**（[`src/permissions.rs:129-133`](../../../src/permissions.rs)）；
   若要它在 `readonly` 档被拒，就必须给它一个非 `ReadOnly` 的 `Effect`，而那会同时改变它在
   `ask`/`workspace`/`auto` 下的待遇与调度器的并发分区（[`src/tools/tool.rs:21-33`](../../../src/tools/tool.rs)）。
   **不存在「网络只读」这个既有分类**，这正是 §1.4 说的缺口。
4. **`Effect` 要不要加第四档？** 例如一个 `Network` 变体，让权限门有一维可看。代价：`Effect` 是
   调度器与权限门共用的词汇，加一个变体要在两处（以及所有 `match`）决定语义；而且 `docs/permissions.md`
   的四档矩阵要重画。**这是本次调研里最大的一次结构性决定。**
5. **搜索后端选谁，密钥存哪？** 密钥要进 `config.toml` 还是环境变量（[`src/config.rs:14-18`](../../../src/config.rs)）？
   选它就意味着**多一条要打码的值**（`Redactor::new` 从 `providers` 收集，[`src/config.rs:590-596`](../../../src/config.rs)）——
   打码器现在只认 provider 密钥，新密钥要单独接进去。
6. **结果进上下文要不要沿用 25k token 的通用上限？** 还是像 `repo_map` 那样给一个**独立预算**
   （[`src/config.rs:1740-1742`](../../../src/config.rs)）与「省掉了多少」的诚实报告
   （[`docs/repo-map.md:31-33`](../../../docs/repo-map.md)）？
7. **缓存、robots.txt、限流做到哪一档？** 完整做法（按 host 排队 + robots.txt + 缓存 + 退避）与最小做法
   （不做缓存、不做 robots、只做超时）差着一个数量级的工程量。**缓存尤其有条款问题**：各家 ToS 对
   「能不能存搜索结果」的口径本轮没核实（见 §4 末尾的未决项）。
8. **引用格式定成什么？** `ToolOutput` 只有 `text`（[`src/tools/tool.rs:35-45`](../../../src/tools/tool.rs)），
   所以引用只能拼进文本。要不要在工具描述里强制「每条结论后面必须带 URL」，
   以及要不要在结果里给一个统一的 `标题｜URL` 头？Anthropic 与 OpenAI 都把「引用必须可见」写成硬要求
   （见 §3），可以照这个立场。

另外两条**不属于拍板、但必须知道**的事实：

- `.scratch/fs-agent-v1/spec.md:634-647` 的 `明确不做` 一节**没有** web 搜索这一项
  （被明文排除的是 MCP、RAG、AST 编辑、进程级沙箱等），所以这件事**没有被 v1 的 spec 挡在门外**
  ——与 MCP、RAG 不同。
- 上游材料里的日期属于它们自己写就的那一天；[`docs/research/README.md:18-21`](../../../docs/research/README.md)
  明说这批笔记「**不加维护**」。本文件里的外部事实全部核实于 **2026-10-02**，
  供应商计价与配额是**易变**的，落地前应重读上面的官方链接。

## 来源

### 仓库内文件（`文件:行`）

- [`src/tools/mod.rs`](../../../src/tools/mod.rs) —— 工具表 `builtin()`（:69-83）、组装期固定的理由（:60-63、:88-96）
- [`src/tools/tool.rs`](../../../src/tools/tool.rs) —— `Effect` 三类与「工作区副作用」的定义（:21-33）、
  `ToolOutput`（:35-45）、`ToolContext`（:109-138）、`Tool::command` 默认 `None`（:222-224）
- [`src/tools/bash.rs`](../../../src/tools/bash.rs) —— `effect()` 恒 `Exclusive`（:112-115）、
  argv 形状 `["bash","-lc",command]`（:127-129、:148-158）、`SANDBOX_NOTE` / `ESCALATION_NOTE`（:37-52）
- [`src/tools/sandbox.rs`](../../../src/tools/sandbox.rs) —— bwrap 参数只有 `--unshare-user/pid/ipc/uts`（:299-302）
- [`src/tools/repo_map.rs`](../../../src/tools/repo_map.rs) —— 「预算是配置、模型参数被忽略」（:61-81）
- [`src/permissions.rs`](../../../src/permissions.rs) —— 模块契约（:1-12）、四档 × 三类 Effect 的 `stance`（:127-184）、
  `Scope`（:252-297）、`decide` 四段（:530-640）、断路器（:642-654）、`rm_breaker` 与 best-effort 扫描（:656-693）、
  `Escalation`（:483-489）、`Call`（:493-518）、升级裁决（:586-617）
- [`src/hooks.rs`](../../../src/hooks.rs) —— 流水线顺序与「只能收紧」（:1-22）、`Constraint`（:35-49）、
  `Tightening`（:76-92）、`effective_verdict`（:94-101）、`PreHookCall`（:136-148）
- [`src/agent.rs`](../../../src/agent.rs) —— `authorize` 与那唯一一次合并（:1827-1884）、打码流水线（:2040-2049、:2195-2209）
- [`src/events.rs`](../../../src/events.rs) —— `EventPayload::redact` 穷尽匹配（:504-576）
- [`src/config.rs`](../../../src/config.rs) —— `.env` 永不加载与优先级（:14-18）、预算常量（:57、:61）、
  `SessionConfig` 字段（:1730-1748）、`redactor()`（:586-596）
- [`src/context.rs`](../../../src/context.rs) —— `SpilledResult` / `truncate_result` / `preview`（:319-403）
- [`src/provider/openai.rs`](../../../src/provider/openai.rs) —— `RetryPolicy` 与 `retry_delay`（:47-87）、
  client 构建（:108-128）、`key_source_description` / `with_auth_hint` / `auth_hint`（:139-175）、
  `post` 的重试循环（:177-226）、`classify_status`（:824-850）、`parse_retry_after`（:895-905）
- [`src/provider/mod.rs`](../../../src/provider/mod.rs) —— `ProviderError` 六类（:146-162）
- [`src/provider/capability.rs`](../../../src/provider/capability.rs) —— `ModelCaps` 字段（:41-66）
- [`src/provider/projection.rs`](../../../src/provider/projection.rs) —— 工具结果进 `tool` 消息（:165-177、:404-412）
- [`Cargo.toml`](../../../Cargo.toml) —— `reqwest` 与 TLS 特性（:22）
- [`docs/sandbox.md`](../../../docs/sandbox.md) —— 「不管网络」（:20-24）
- [`docs/adr/0006-sandbox-by-bubblewrap.md`](../../../docs/adr/0006-sandbox-by-bubblewrap.md) —— 「为什么网络不在这一层」（:24-26）
- [`docs/permissions.md`](../../../docs/permissions.md) —— 四档表（:13-32）、打码是值级 best-effort（:44-50）、升级手势（:57-70）
- [`docs/repo-map.md`](../../../docs/repo-map.md) —— 预算与「末尾一行写出被省掉多少」（:24-37）
- [`CONTEXT.md`](../../../CONTEXT.md) —— 「打码（Redactor）」（:278-280）
- [`.scratch/fs-agent-v1/spec.md`](../../fs-agent-v1/spec.md) —— `明确不做`（:634-647）
- [`.scratch/web-search-tool/seed.md`](../seed.md) —— 本轮的种子材料（:1-24）

### `docs/research/`（上游正文引文，一个字不改）

- [`docs/research/README.md`](../../../docs/research/README.md) —— 这批笔记的性质与「不加维护」（:1-21）
- [`docs/research/coding-agent-features.md`](../../../docs/research/coding-agent-features.md) ——
  `WebFetch`/`WebSearch` 归类（:95）、威胁模型（:239）、MCP 的工具描述不可信（:259）、
  Codex 的 `network_rule`（:272）、权限档位对比（:246-274）、prompt injection 防护建议（:306）
- [`docs/research/notes/claude-code-amp.md`](../../../docs/research/notes/claude-code-amp.md) ——
  工具列表（:71-75）、`WebFetch(domain:host)` 与主内容字段（:236-247）、`--restricted` 移除 WebFetch（:315-318）、
  凭据保护独立成层（:301-307）、Amp 的工具表（:846-849）、Amp 的注入免责（:1018-1021）、
  Amp 的「外部输入会影响它」（:972-974）、Parallel 供能（:1304）、web search 计费（:1256-1258）
- [`docs/research/notes/codex-gemini.md`](../../../docs/research/notes/codex-gemini.md) ——
  Gemini 工具参数（:96-102）、plan 模式拦 web fetch（:322-326）、Codex `web_search` 是 hosted 形状（:591-593）、
  `supports_standalone_web_search`（:995-998）、Bedrock 不支持 web search（:1019-1021）、
  `web_search` 是事件 item（:927-930）
- [`docs/research/notes/aider-openhands.md`](../../../docs/research/notes/aider-openhands.md) ——
  aider 的 `/web` 是斜杠命令（:83-86）、OpenHands V0 工具与 `enable_browsing` 默认 True（:405-418）、
  `browser_output_condenser`（:427-431）、V1 的 `browser_use`（:715）
- [`docs/research/notes/cline-continue.md`](../../../docs/research/notes/cline-continue.md) ——
  Cline 的 `Use the browser` 与 `use_browser`（:421-436）、子代理禁网（:1279-1283）
- [`docs/research/notes/opencode-goose.md`](../../../docs/research/notes/opencode-goose.md) ——
  opencode 的 `webfetch`/`websearch`（:120-123）、Exa/Parallel 门控（:163-165）、
  权限键含 `webfetch`/`websearch`（:374-377）、agent frontmatter（:404-407）、`doom_loop`（:398-400）、
  goose 的五个工具（:169-180）、goose 的注入检测（:449-455）

### 外部 URL（核实于 2026-10-02）

供应商文档：

- Kimi：[Use Web Search（`$web_search`）](https://platform.kimi.ai/docs/guide/use-web-search) ·
  [WebSearch Pricing](https://platform.kimi.ai/docs/pricing/websearch) ·
  [Best Practices for Web Search](https://platform.kimi.ai/docs/guide/best-practices-for-web-search) ·
  [How to Use Official Tools](https://platform.kimi.ai/docs/guide/use-official-tools)
- DeepSeek：[Responses API](https://api-docs.deepseek.com/zh-cn/api/create-response) ·
  [使用 Responses API](https://api-docs.deepseek.com/zh-cn/guides/responses_api) ·
  [更新日志](https://api-docs.deepseek.com/zh-cn/updates) ·
  [V2.5-1210 新闻（「API 暂不支持搜索功能」）](https://api-docs.deepseek.com/zh-cn/news/news1210/)
- Anthropic：[Web search tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-search-tool) ·
  [Pricing（web search $10/1,000、web fetch 免费、token 经验值）](https://platform.claude.com/docs/en/about-claude/pricing)
- OpenAI：[Web search 指南](https://developers.openai.com/api/docs/guides/tools-web-search) ·
  [Built-in tools 计价](https://developers.openai.com/api/docs/pricing#built-in-tools)
- Google：[Gemini Developer API pricing](https://ai.google.dev/gemini-api/docs/pricing)

搜索 API：

- Brave：[Brave Search API（$5/1,000、每月 $5 免费额度）](https://brave.com/search/api/) ·
  [注册](https://api-dashboard.search.brave.com/register)
- Tavily：[Credits & Pricing（1,000 credits/月免费、$0.008/credit）](https://docs.tavily.com/documentation/api-credits) ·
  [Rate Limits](https://docs.tavily.com/documentation/rate-limits)
- SearXNG：[Search API（`format=json` 需在 settings.yml 启用）](https://docs.searxng.org/dev/search_api.html) ·
  [Answer CAPTCHA from server's IP](https://docs.searxng.org/admin/answer-captcha.html) ·
  [Limiter](https://docs.searxng.org/admin/searx.limiter.html)
- Google：[Custom Search JSON API（新客户不可用、2027-01-01 关停、100/天免费 + $5/1,000）](https://developers.google.com/custom-search/v1/overview)
- Exa：[Pricing（$10 credits/月、Search $4/1k 起、Contents $1/1k pages）](https://exa.ai/pricing) ·
  [Billing & Rate Limits](https://exa.ai/docs/admin/billing)

crate 与规范：

- [crates.io](https://crates.io/) 上逐个 crate 的 API：`scraper` / `dom_smoothie` / `readability` /
  `article_scraper` / `html2text` / `select` / `lol_html` / `ammonia` / `robotstxt` / `texting_robots` /
  `chardetng` / `encoding_rs`（版本与时间见 §5 的表）
- [reqwest `ClientBuilder`](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html)（连接池与超时的默认行为）
- [RFC 9309: Robots Exclusion Protocol](https://www.rfc-editor.org/rfc/rfc9309.html)
- [OWASP SSRF Prevention Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html) ·
  [OWASP: SSRF Prevention in Node.js](https://owasp.org/www-community/pages/controls/SSRF_Prevention_in_Nodejs.html)

第三方（**非一手，仅作旁证，不作为事实引用**）：

- [oh-my-pi#7794](https://github.com/can1357/oh-my-pi/issues/7794)（声称给 DeepSeek 加 hosted `web_search`）
- [DeepSeek-Reasonix#7466](https://github.com/esengine/DeepSeek-Reasonix/pull/7466)（同上）
