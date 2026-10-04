# grilling：server 进程的环境与可写根

Type: grilling
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

冻结项 9 定了「随会话起、随会话停、过 bubblewrap 沙箱」。剩下两件具体的：

**1. 环境变量。** server 是我们的子进程，默认继承父进程的环境 —— 而 `bash` 用的是
`bash -lc`（继承**登录环境**），于是 `DEEPSEEK_API_KEY` / `MOONSHOT_API_KEY` 这些东西对 server
是可见的。DSH 的做法是 `scrubbedParentEnv()` 把 `/KEY|PASSWORD|SECRET|TOKEN/i` 与它自己的
`DSH_*` 一并删掉。fs-agent 要擦到什么程度？

（[票 02](02-grilling-sdk-or-own-client.md) 已定用 `rmcp`，而它**不做环境清洗** —— 所以这件事
必须由 fs-agent 自己做，SDK 那侧没有对应物；`stderr` 也默认 `inherit`，要显式
`.stderr(Stdio::piped())` 才拿得回句柄。）

- 照 DSH 的正则擦（宽，可能误伤 `MONKEY_...` 之类）
- 只擦**本会话用到的**那几把（`config` 里解析出来的 provider 密钥值 + 名字）
- 不擦（server 是用户自己配的，它要什么自己给）

**2. 可写根。** 沙箱默认「工作区与一列缓存目录可写、区外只读」。很多 MCP server 要写自己的
缓存（`~/.cache/<server>/`）或状态目录 —— 那会被沙箱挡。要不要：

- 在 server 配置里允许声明 `writable_roots`（逐台，照 `[sandbox] writable_roots` 的形状）
- 默认给一个 per-server 的临时目录（`/tmp` 每次调用都是新的，跨调用不保留）
- 什么都不给（server 写不了就是它的事）

## 作答

**环境走白名单**：`env_clear()` 之后只注入两样 —— server 配置里 `env` 显式声明的几项，加最小必需的
`PATH` / `HOME` / `LANG`（具体清单归实现票，原则是「不声明就没有」）。

- **不照 DSH 的正则黑名单**：黑名单是 fail-open 的（漏掉一个名字就漏一个密钥），而沙箱那一层的
  调性是 fail closed（[ADR 0006](../../../docs/adr/0006-sandbox-by-bubblewrap.md)）。
- 这条与票 02 的事实咬合：`rmcp` **不做环境清洗**，所以清洗必须发生在交给它**之前** —— 我们自己
  `env_clear()` + 注入后再 spawn。
- 代价如实记下：某些 server 依赖 `XDG_*`、`SSH_AUTH_SOCK` 这类变量 —— 那些要由人在 server 配置里
  显式声明，不默认继承。

**可写根逐台声明**：server 配置里加一个 `writable_roots`（形状照 `[sandbox] writable_roots`），
追加到这台 server 的沙箱可写集；不声明就只有「会话工作区 + 沙箱默认的那列缓存目录」。

- **不给默认的 per-server 临时目录**：那多一份要清理的东西，而且 server 的缓存该放哪本来就该由人定。
- 这条与冻结项 9（过沙箱）一起构成 server 的文件边界：**区外只读，要写就显式声明**。
