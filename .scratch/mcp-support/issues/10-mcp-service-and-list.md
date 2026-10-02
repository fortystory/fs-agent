# 10 — 服务层骨架 + `mcp_list`（tracer bullet）

Type: implement
Status: done
Part of: ../map.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1–§3、§5、§7。本票打通「工具层 → 服务层 → 假连接」整条链；
> `mcp_call` 归 [票 11](11-mcp-call.md)，真连接归 [票 12](12-rmcp-connection.md)。

## 目标

`[mcp] enabled = true` 且挂上一个（假的）连接时，模型能调 `mcp_list(["github"])` 或 `mcp_list([])`，
拿回那台（或全部）server 声明的**工具清单与 `instructions`**。`enabled = false`（缺省）时四个元工具
都不进表，会话行为逐字不变。

## 现状（2026-10-03 核实，改前先复核）

- **组装期加工具的先例是 `with_web`**：[`src/tools/mod.rs`](../../../src/tools/mod.rs) 的
  `builtin()` / `with_dynamic()` / `with_web()`；组装点在三处前端（`src/cli.rs`）。
- **配置段与 `deny_unknown_fields` 的先例**：`[web]` 的 `RawWeb` / `WebSettings` / `resolve_web`
  （`src/config.rs`）。
- **服务层 trait 与组装期注入的先例**：`src/web/` 的 `SearchProvider` / `FetchProvider` 与
  `web_service()`。
- **带 code 的错误先例**：`WebError`（code 英文、句子中文）。
- **`Tool` trait 与 `ToolContext`**：`src/tools/tool.rs`（`effect()`、`read_paths()` 默认空、
  `ToolOutput` 只有 `text`）。
- 配置的逐字段优先级与「项目里的 `.env` 永不加载」：`src/config.rs` 顶部注释。

## 落点

`src/mcp/mod.rs`（新模块）、`src/tools/mcp_list.rs`（新）、`src/tools/mod.rs`、`src/config.rs`、
`src/cli.rs`、`tests/`。

## 具体行为

1. **`src/mcp/`**：`McpService`（「server 名 → 连接」的映射）、`McpConnection` trait
   （`list_tools` / `call_tool` / `list_resources` / `read_resource`；本票只落地接口与前者的假实现）、
   `McpError`（带 code：`MCP_UNKNOWN_SERVER` / `MCP_SERVER_UNAVAILABLE` / `MCP_PROVIDER_ERROR` …；
   code 英文、句子中文）。
2. **`mcp_list(server?)`**：`server` 可选；不传就列全部启用的 server。返回每台的工具（名字 + 参数
   schema 的摘要）与它的 `instructions`。
3. **每次现问**：每次调用都向连接问一次 —— 不缓存、不订阅（决策票 05 的答案）。
4. **`effect()` 恒 `Effect::ReadOnly`**（列清单不碰工作区）；`read_paths` 不重写。
5. **`[mcp] enabled`（缺省 `false`）在组装期决定注册**：四个元工具同开同关，新增一个 `with_mcp`
   与 `with_web` 并列。**连接可用性与它无关**（全都连不上时工具仍在，调用回结构化错误）。
6. **两条配置来源**：项目根的 `.mcp.json` + `config.toml` 的 `[mcp.servers.<名字>]`，**项目级盖
   用户级**；两处都不接受没见过的键。本票只需把它们解析出来（真正用它们是票 12 与 13）。
7. **假连接注入**：`McpService` 在 `src/cli.rs` 组装期建；测试经组装参数注入一个进程内假连接。

## 验证

`cargo test` + `cargo clippy --all-targets` + `cargo fmt` + `python3 scripts/check-language.py`：

1. **开关**：`enabled = false` 时表里没有 `mcp_list`；打开后有；**连接全不可用时它仍在**（假连接
   总是失败 → 调用回结构化错误）。
2. **list 的形状**：假连接声明两个工具，断言两个名字都在、`instructions` 在、且带那句中文不可信
   标记。
3. **每次现问**：连调两次，假连接记到两次。
4. **未知 server**：`mcp_list(["nope"])` 回可读的 `MCP_UNKNOWN_SERVER`，流上恰好一条
   `ToolCallCompleted`。
5. **配置优先级**：同名 server 在 `.mcp.json` 与 `config.toml` 都有时项目级生效；没见过的键被拒。
6. **权限**：`mcp_list` 在 `readonly` 档放行、在 `ask` 档不产生询问。

## 不做什么

- 不做 `mcp_call`（票 11）、不接真连接（票 12）、不做资源与提示词（票 16 / 17）。
- 不做 server 进程、沙箱与环境（票 12 / 13）；不做 `Effect` 与信任的放宽（票 14）——
  本票的元工具是 `ReadOnly`。

## Comments

- 2026-10-03 落地（`Status: done`）。落点：`src/mcp/mod.rs`（新）、`src/tools/mcp_list.rs`（新）、
  `src/tools/mod.rs`（`with_mcp`）、`src/config.rs`（`[mcp]` 段与 `McpSettings` / `McpServerConfig` /
  `McpTransport`、`Config::load_project_mcp`、打码器收 `env` / `headers` 的值）、`src/cli.rs`
  （三处组装点 + `mcp_service` / `open_connections` 挂点）、`src/lib.rs`、`tests/mcp_list.rs`（新，12 条）、
  `tests/credentials.rs`（+1 条）。
- 两处 spec 没写死、在本票定下来的细节：
  - `.mcp.json` 的外层键取上游惯例的 `mcpServers`，里面**每台 server 的记录与
    `[mcp.servers.<名字>]` 逐字同形**（snake_case、`command` 是 argv 数组）。两个来源共用一个
    反序列化结构，于是不存在第二套字段语义；两边都 `deny_unknown_fields`。
  - 项目级的读取落在 `Config::load_project_mcp(&cwd)`，由前端在 cwd 定下来之后调用；
    `[mcp] enabled = false`（缺省）时**连文件都不 stat**，一份坏掉的 `.mcp.json` 影响不到
    不做 MCP 的会话。
- `McpConnection` 的 `call_tool` / `list_resources` / `read_resource` 暂时是默认实现，返回
  `MCP_UNSUPPORTED`：票 11 与票 16 会把它们换掉；trait 的四个方法形状本票一次定死。
- 验证：`cargo test`（新增 12 + 1 条全绿）· `cargo clippy --all-targets`（无 warning）·
  `cargo fmt --check` · `python3 scripts/check-language.py` 全通过。

- **审查后的修正（2026-10-03）**：`.mcp.json` 内部同名 server 是启动错误这条原来只挡了 TOML
  那一半 —— `serde_json` 默认「后者胜」。现在 `RawProjectMcp.mcpServers` 用了一个手写的
  `UniqueServerMap` visitor（`src/config.rs`），重复键直接被拒；`tests/mcp_list.rs` 里补了
  JSON 那一半的用例。另：`docs/mcp.md` 与这三个工具文件已纳入 `scripts/check-language.py`
  的两条棘轮（实测：中文串 11 / 6 / 8 / 2，英文散文上限 27 → 29 全是假阳性）。
