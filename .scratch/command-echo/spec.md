# 命令留痕：一次手势也进事件流

Status: done

- **来源**：维护者 2026-10-09 的要求「单独用 `/` 命令也应该在用户下面有一条输出，现在是直接
  使用后用户下面什么也没有」。
- **决定**：留了 [ADR 0019](../../docs/adr/0019-command-records-are-log-only.md)（log-only 的
  理由与被否决的两条替代方案）。相邻的那条是
  [`ui-trim/spec.md`](../ui-trim/spec.md) §2 —— 技能那半边走的是**相反**的路。
- **术语**：命令记录（`CommandRun`）/ 用户发言 / 气泡 / 轨迹视图 / 对话视图 照
  [`CONTEXT.md`](../../CONTEXT.md)。

## 现状（2026-10-09 核实，改前已复核）

- 命令分支在 `interactive_loop` 里把提交消费掉（`src/cli.rs` 的 `match submission(...)`），
  **不发 user 消息**，所以转录里读不出自己敲了什么。
- 唯一的出声处是命令自己发的 `notice`，而**回执只有成功才有**：`/clear` 清完场什么也不说，
  `/quit` 直接退出，`/model` 失败时说的是另一件事。
- `Notice` 走的是 `RenderEvent::Notice`（渲染通道）：不落 `log.jsonl`、`--continue` 之后不重播，
  而且画成 `narration` 那一档静音。

## 方案

1. **新 payload**：`EventPayload::CommandRun { text }`（原文，散文，打码），与
   `GoalSelected` / `SandboxStatus` 同一档 **log-only**。
2. **判据只有一条：这一条提交会不会变成一句 user 消息**（`records_itself`）。会的（普通消息、
   **带任务**的 `/<skill>`）原文已经是用户气泡，不重复记；不会的（内建命令、MCP 模板、**裸**
   技能、一个错字）才另记。`请 /loop 一个目标` 也记 —— `/` 的提交语义是**执行**（ADR 0012），
   而它启动的那句话一个字都不留。空行与 `/quit` 不记。
3. **落点**：在 `match submission` **之前**落，于是 `/clear` 自己那一条留在**这场**会话里
   （它之后紧跟着翻页，而它是这场会话的最后一个手势）。
4. **画法**：转录里一行 `[命令] /clear`，用草稿里命令那个 `TOKEN_COMMAND` 蓝（不是 `Notice`
   那一档静音）；对话视图与轨迹视图都画（`selects` 里与 `Block::Answer` 同档）；plain 走同一
   份措辞；headless 走 stderr。
5. **复盘**：`sessions show` 列一行（`Entry::Command`），统计与 `--only-error` 都不算它。

## 明确不做

- **把命令原文作为 user 消息发进上下文** —— 那会让模型读到 `/clear`（ADR 0019 有理由）。
- **只留回执** —— 回执只在成功时说话，而账要包括没成功的那几次。
- **用 `RenderEvent::Notice`** —— 它不落盘、不重播，而 `.scratch/ui-trim` 明确说过重放要重现
  同样的转录。

## 测试

- `src/cli.rs`：`records_itself` 的判据表（含 `/quit`、空行、带任务的技能、`请 /loop …`）。
- `tests/projection_attribution.rs`：命令记录**不进任何一份投影**（三个发言者各一遍）。
- `tests/render_plain.rs`：那一行原样到 stderr。
- `tests/render_answer.rs`：对话视图里有它、且用的是命令那个颜色。
- `tests/observe_cli.rs`：`sessions show` 列一行。

## 手工清单

[`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) ㊴。