# 01 — `CommandRun` 从事件流一路走到三个前端

Type: implement
Status: done
Blocked by: —

> 来源：[`../spec.md`](../spec.md)；决定在
> [ADR 0019](../../../docs/adr/0019-command-records-are-log-only.md)。

## 目标

- `EventPayload::CommandRun { text }`：枚举 + `kind()` + `redact()`（原文是散文，要打码）。
- 投影把它当零（`provider::projection`，那张穷举表刻意没有 `_` 分支）。
- `Transcript::push` → `Block::CommandRun { text }`；`selects` 里它与 `Block::Answer` 同档
  （对话视图与轨迹视图都画）。
- 三个渲染器各画一行：`command_line`（`TOKEN_COMMAND` 蓝）、plain 一行 stderr、headless 一行。
- `observe`：`Entry::Command` 出现在 `kind()` / `speaker()` / `entry_of` / 统计里；
  `cli.rs` 的 `entry_line` 给它一句 `[命令] …`。
- `agent::record_command_run` + `Harness::record_command_run`；`interactive_loop` 在
  `match submission` 之前调它，判据是 `records_itself`。
- `wording::command_run`。

## 落实时发现的

- `/quit` 不记（人正在离开）、空行不记 —— `records_itself` 的三档。
- `请 /loop 一个目标` 读作 `Submission::Loop`，所以它**要记**：`/` 的提交语义是执行，模型那边
  一句话都不会有。
- 事件流那一处落点定在 `match` 之前，于是 `/clear` 的记录在**旧**会话里（它是那一场的最后一个
  手势）。

## 测试

见 [`../spec.md`](../spec.md) 的「测试」一节（实现时逐条对上）。