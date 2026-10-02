# 16 — 资源：`mcp_resources` 与 `mcp_read`

Type: implement
Status: done
Part of: ../map.md
Blocked by: 10

> 规格：[`../spec.md`](../spec.md) §8（第二阶段）。这是「整个协议面」的第二类原语。

## 目标

模型能列某台 server 的资源、并按 URI 读一份 —— 与工具分开两条入口（形状不同）。

## 具体行为

1. **两个新元工具**：`mcp_resources(server?)`（可选 `server`，不传列全部）与
   `mcp_read(server, uri)`。形状照 `mcp_list` / `mcp_call`（`effect()` 都是 `ReadOnly`）。
2. **不进 `ReadSet`**：`ReadSet` 装的是工作区路径，而资源是 URI —— 读过资源之后 `edit_file` **仍
   要求先 `read_file`**（与 `grep` 命中文件同一种处理）。
3. **两套坐标系**：资源 URI 与 `resolve_read` 的工作区解析互不操作。
4. **上限走既有流水线**；正文带不可信标记（除非那台 server 开了 `trust_results`）。
5. **跟着 `[mcp] enabled` 同开同关**：四个元工具共用一个组装期开关。

## 验证

`cargo test` + `cargo clippy --all-targets` + `python3 scripts/check-language.py`：

1. **形状**：假连接声明两份资源，`mcp_resources` 列得出来、`mcp_read` 读得到、结果带标记。
2. **不影响 `ReadSet`**：读完一份资源后直接 `edit_file` 被拒，补一次 `read_file` 之后成功。
3. **失败**：未知 URI / 未知 server 都是可读的结构化错误。
4. **开关**：`enabled = false` 时两个工具也不在表里。

## 不做什么

- 不把资源塞进 `read_paths`、不与工作区路径互操作。
- 不做资源的缓存（`mcp_list` 那条「每次现问」的规矩同样适用）。

## Comments

- 2026-10-03 落地（`Status: done`）。落点：`src/tools/mcp_resources.rs`（新，两个工具）、
  `src/tools/mcp_args.rs`（新，四个元工具共用的参数读取：`optional_server` / `required_str` /
  `flatten`；`mcp_list` 与 `mcp_call` 一并改用，规则只剩一处）、`src/mcp/mod.rs`
  （`ResourceListing`、`list_resources`、`read_resource`、`trusts_results`、
  `MCP_UNKNOWN_RESOURCE`）、`src/tools/mod.rs`、`tests/mcp_resources.rs`（新，7 条）。
- 一处实现里定的边界：**`trust_results` 只管「内容」**——`mcp_call` 与 `mcp_read` 的成功结果；
  `mcp_list` / `mcp_resources` 的清单照旧带那句标记。清单是元信息（有哪些工具、URI 怎么拼），
  而「不要把它当指令」那句话防的是外部**内容**。一个开了 `trust_results` 的 server 也不会因此
  让它的工具描述变成指令来源。
- 验证 2 的走法：先用 `mcp_read` 读一份资源（`ReadSet` 不动），再 `edit_file` 一个没读过的
  工作区文件 → 被「改前先读」拒；补一次 `read_file` 之后同一个编辑成功。
- 验证：`cargo test`（新增 7 条全绿）· `cargo clippy --all-targets`（无 warning）·
  `cargo fmt` · `python3 scripts/check-language.py` 全通过。

- **审查后的修正（2026-10-03）**：三条元工具的错误结果统一成**不带**那句外部内容标记
  （`mcp_list` / `mcp_resources` 原来带、`mcp_call` / `mcp_read` 原来不带）——错误消息是我们
  自己写的中文，那句话说的是「下面这些字来自外面」；清单里**某台 server** 失败的那一段仍然带。
  另：`read_resource` 的「URI 不存在」现在两种错误码都认（`-32002` 与 2026-07-28 那一版的
  `INVALID_PARAMS(-32602)`），免得真 server 上落成 `MCP_PROVIDER_ERROR`。
