# 14 — `Effect` 与信任三个位

Type: implement
Status: ready-for-agent
Part of: ../map.md
Blocked by: 10

> 规格：[`../spec.md`](../spec.md) §6。

## 目标

外来工具**默认最严**；人在配置里**逐台**按能力放宽 —— 三个位各自默认关。

## 具体行为

1. **`mcp_call` 的 `effect()` 缺省 `Exclusive`**（票 11 已落地这条），本票负责**放宽的那条路**。
2. **三个独立的信任位**（都在 server 记录里，各自默认关）：
   - **`trust_effects`**：允许按配置声明较宽的 `Effect`（例如某条工具标只读）；
   - **`trust_results`**：这台 server 的结果**不带**不可信标记；
   - **`sandbox`**：默认 `true`，设 `false` 才不过沙箱（与冻结项 9 相反，得单独声明）。
3. **一个位不影响另一个位**：只开 `trust_results` 不会让 `Effect` 放宽，反之亦然。
4. **不读 `ToolAnnotations`**：规范明文说不能据以做权限判断 —— 连读都不读。

## 验证

`cargo test` + `cargo clippy --all-targets` + `python3 scripts/check-language.py`：

1. **缺省**：`mcp_call` 在 `readonly` 档拒、`ask` 档问。
2. **`trust_effects`**：打开且某条工具标只读后，它在 `readonly` 档放行。
3. **`trust_results`**：打开后结果不带标记；缺省带。
4. **`sandbox = false`**：该 server 的进程不过 bwrap（用假 spawn 断言 argv 里没有它）。
5. **互不牵连**：逐个只开一个位，断言另外两处的行为不变。

## 不做什么

- 不给 `Effect` 加第四类；不动权限门的四档矩阵。
- 不做「按 `ToolAnnotations` 自动映射」。
