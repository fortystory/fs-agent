# 16 — 资源：`mcp_resources` 与 `mcp_read`

Type: implement
Status: ready-for-agent
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
