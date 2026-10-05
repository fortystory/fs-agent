# 时间：一个自带的 MCP server（`fs-agent-mcp-time`）

Status: 三张实现票 `done`（[`issues/01`](issues/01-time-server-bin.md)–
[`03`](issues/03-docs-and-index.md)，2026-10-06 由维护者的要求直接拆出、同日全部落地，不经决策图）

模型判断「现在」只能靠训练数据里的日期，而这个仓库里到处是带时间语义的东西：会话文件、目标
预算与翻页、`bash` 里的 `date`。让它知道当下时间有两条路 —— **给它一个工具**，或者**把时间写进
系统提示词**。

第二条路在这个仓库里走不通，这不是风格问题：`agent_identity()`（`src/agent.rs`）既是**缓存
前缀**的一部分，又是 `replay::identity_for()` 复现当时那次请求时**再调一次**的那个函数。往里塞
「现在几点」，它就从纯函数变成非纯 —— 跨秒复现立刻对不上（`tests/replay.rs` 那条「重放 == 当时
发出去的东西」），而且时间每变一次，整段前缀（技能正文、`AGENTS.md`、注入）在 provider 侧作废。

来源（维护者 2026-10-06 的要求 + 四问的答案）：

- 时间由**一个自带的 MCP server** 提供，走已有的 MCP 通路，不新增内建工具；
- 系统提示词里**只加一句静态指引**（该问谁），**不放时间的值**；
- 返回**本地时区**的时间：带 UTC 偏移与时区名，精确到秒；
- 先写 spec 与票，再照票实现。

## 问题陈述

1. **模型对「现在」只有训练数据。** 它不知道今天几号、星期几，也不知道会话是白天还是深夜。
   `bash` 里跑一句 `date` 凑得出来，但那把一次读时钟算成 `Exclusive`（shell 什么都能写），还要
   模型先记得有这条路。
2. **把时间的值写进身份是错的** —— 见上面那两条：replay 不变量与缓存前缀。
3. **现成的外部 time server 与这台机器的环境不匹配。** 上游那些要 `uvx` / `npx`：多一层运行期
   依赖、要能出网、沙箱里还得开可写根。自己写的东西该有自己的一个：**没有依赖、不写盘、不出网**。

## 方案

**一个产品二进制 + 一句静态指引 + 一份配置样例**，三层都不碰 fs-agent 的既有形状：

```text
配置      .mcp.json（或 config.toml 的 [mcp.servers.time]）里的一台 stdio server
   │
协议      newline-delimited JSON-RPC：server/discover · tools/list · tools/call
   │
二进制    fs-agent-mcp-time：读一次本地时钟，回一段中文
   │
身份      agent_identity() 里一句静态指引：要时间就问它，别猜
```

**工具表、四个元工具、`src/mcp/` 一个字不改**：这台 server 对 fs-agent 而言就是一台普通的外部
server，与 `github`、`jira` 没有区别 —— 这正是 MCP 那一层的设计目的。

## §1 `fs-agent-mcp-time`：一个手写的 stdio server

- **新 `[[bin]]`**：`name = "fs-agent-mcp-time"`、`path = "src/bin/mcp_time.rs"`（`src/bin/` 是
  Cargo 的默认 bin 目录，但照样显式写 `[[bin]]`：仓库里三个 bin 要一眼看全，`default-run` 那一行
  也才有意义）。
- **手写协议，不开 `rmcp` 的 server 侧**：`Cargo.toml` 里那行注释明写着「**不开** server」，而
  这里要的只是三条方法、约四十行。开了它，产品就多背一整套服务端实现。假 server
  （`tests/support/fake_mcp_server.rs`）已经证明手写这三条方法够用 —— 本票就是**照它的形状**写
  一个真的。
- **只认 `2026-07-28`（Discover 生命周期）**：与 client 侧同一条纪律，不做旧版 `initialize`
  回退。`server/discover` 回 `supportedVersions: ["2026-07-28"]`、`capabilities: { tools: {} }`。
- **只答三条方法**：`server/discover`、`tools/list`、`tools/call`；其余一律 JSON-RPC `-32601`。
  **没有 `id` 的帧（通知）不回响应** —— JSON-RPC 里通知本就不该有响应。
- **不写盘、不出网、不读配置、不看环境**：整个进程除了一次时钟读数与标准输出，什么也不做。于是
  它能过缺省的沙箱（只读根就够），也不需要 `writable_roots`。

## §2 工具：`get_current_time`

```json
{ "name": "get_current_time",
  "description": "报出这台机器当下的本地时间：年月日、时分秒、UTC 偏移、时区名与星期几。",
  "inputSchema": { "type": "object", "properties": {}, "required": [] } }
```

- **没有参数**。别的时间（某个时区、某种格式、时间戳换算）不做，见 §7。
- **返回一段中文**（ADR 0005：模型可见的散文走中文），一个 text block：

  ```text
  本机现在：2026-10-06 14:32:05 +08:00 星期二（Asia/Shanghai）
  ```

  - 本地时区来自 `chrono::Local`；偏移用 `%:z` 写成 `+08:00` 这个形状。
  - 时区名来自 `iana-time-zone`（0.1，`chrono` 已经把它拉进 lock，这里只是把它变成直接依赖）；
    **取不到时省掉尾部的括号**，而不是编一个名字，也不是报错。
  - 星期几是中文：`chrono::Weekday` 映射七个词。`%A` 出来的是英文，不用。
- **没有工具级失败**：这条路没有可失败的输入。唯一的失败是标准输出写不出去，那按 §3 退出。

## §3 进程行为

- **一行 stderr 的自述**在启动时打：与假 server 同一条路 —— 那行会被 client 接住、交给诊断口
  （`[外部工具] …`），不砸进终端。
- **标准输出只出现协议帧**：一行一条、`flush` 之后才继续读。日志走 stderr，这是 stdio 传输的
  基本功。
- **stdin 结束就退出**（退出码 0）：父进程关掉管道就是「会话结束了」，不是错误。
- **不设超时、不做空闲退出**：client 侧有 `connect_timeout_ms` 与进程组清理，这里不需要第二套。

## §4 配置：怎么把它挂上

样例给两种来源，**项目级（`.mcp.json`，跟着仓库走）**是推荐的那个：

```json
{
  "mcpServers": {
    "time": { "command": ["fs-agent-mcp-time"] }
  }
}
```

配 `config.toml` 时，用户的样例长这样（`[mcp] enabled` 缺省关，打开才有那四个元工具）：

```toml
[mcp]
enabled = true

[mcp.servers.time]
command = ["fs-agent-mcp-time"]
trust_effects = true
read_only_tools = ["get_current_time"]
```

- **名字取 `time`**：它就是 `mcp_call` 的 `server` 参数，也是 §5 那句话里点名的那个词。
- **`command` 要能被找到**：`fs-agent-mcp-time` 在 `PATH` 上（`cargo install --path .`），或者
  写绝对路径（`target/release/fs-agent-mcp-time`）。`PATH` 是环境白名单里那三样之一，够用。
- **`trust_effects` + `read_only_tools` 建议开**：读时钟不是工作区副作用，不开的话它在 `readonly`
  档会被拒（`mcp_call` 缺省按最严的 `Exclusive` 算），只能靠每次人工批准。这两个位是**逐台**的，
  与其它 server 互不牵连。
- **`trust_results` 建议保持缺省**。它管的是「结果要不要带那句不可信标记」，而「这台是我们自己
  写的」按本仓库的纪律**不蕴含**「它的结果可以当指令读」。多那一行标记无害。

## §5 身份里那一句

`src/agent.rs` 新增一条 `pub const TIME_GUIDANCE: &str`，由 `agent_identity()` 拼上，位置在
`WEB_GUIDANCE` 之后、`THINKING_IN_CHINESE` 之前。措辞定成这样：

> 需要当下时间（几点、几号、星期几）时不要凭上下文猜：若会话里有提供时间的 MCP server
> （本仓库自带 `fs-agent-mcp-time`），用 `mcp_call` 调它的 `get_current_time`。

- **它无条件拼上**，与 `WEB_GUIDANCE` 同一条取舍：身份是缓存前缀的一部分，让它随配置抖动，等于
  每换一次配置就把每一个会话的前缀作废一次。代价是没配 MCP 的会话里这句话落空 —— 所以措辞是
  **条件式**（「若会话里有……」），而不是一句会指向不存在工具的命令。
- **不放时间的值**：这是本 spec 的支点，理由写在开头。
- **只在这一处**：讨论者、合成器、执行者三段身份**不拼**它 —— 合成器明写「没有工具」，讨论者与
  执行者的工具体系各自独立。要改这个判断，先改这一段。

## §6 测试

1. **二进制本身**（`tests/mcp_time_server.rs`，真 spawn 一个进程、按行读写 JSON-RPC）：
   `server/discover` 回的版本与能力；`tools/list` 里有 `get_current_time` 且无必填参数；
   `tools/call` 回的文本匹配 `本机现在：\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} [+-]\d{2}:\d{2} 星期.`
   这个形状；未知方法回 `-32601`；一条通知不回任何东西。
2. **端到端一次真调用**（同一个文件，照 `tests/mcp_stdio.rs` 那条路）：把 fs-agent 的
   `McpService` 指向 `env!("CARGO_BIN_EXE_fs-agent-mcp-time")`，先 `mcp_list` 看到
   `get_current_time`，再 `mcp_call` 拿到那段文本 —— **这是握手形状真正的验收**：手写的帧只要与
   client 的期望差一个字段，这条就红。
3. **身份**：断言拼上了/没拼上这句（扩展 `tests/thinking_language.rs` 那条先例），并且
   `agent_identity()` 的文本里**不含** `\d{4}-\d{2}-\d{2}`（守「不放时间的值」）。
4. **回归**：`cargo test`、`cargo clippy --all-targets`、`cargo fmt`、`python3
   scripts/check-language.py`、`python3 scripts/check-doc-size.py`。

## §7 明确不做

- **时区参数、时间戳换算、差值计算、日期加减**：这台 server 只回答「现在」。
- **内建工具**：不走 `current_time` 那条路，形状定了 MCP。
- **`rmcp` 的 server 侧**、**HTTP 传输**、**旧版 `initialize` 回退**、**`prompts` / `resources`
  原语**：这台 server 只有工具，一条资源一个模板都没有。
- **新 ADR**：本 feature 没有推翻任何既有决定，只是把已有的两条纪律（身份是缓存前缀、模型可见
  散文走中文）用在它身上。

## 代码落点

- 二进制：`src/bin/mcp_time.rs`（新）、`Cargo.toml`（第三个 `[[bin]]` + `iana-time-zone`）；
- 身份：`src/agent.rs`（`TIME_GUIDANCE`、`agent_identity()`）；
- 测试：`tests/mcp_time_server.rs`（新）、`tests/thinking_language.rs`；
- 文档：`docs/mcp.md`（新一节）、`README.md`、`.scratch/README.md`（feature 索引一行）。
