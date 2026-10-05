# 对话视图的保留清单（`Notice` 那一类）

Type: grilling
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

冻结项 8 给了对话视图四类例外（错误 / 中断 / 权限裁决 / hook 拒绝），但[research：两个 Pane 共享 `painted` 的改造面](05-research-shared-painted-two-panes.md)的测试清点暴露出一类**没被点名、却显然是说给用户听的行**：`Block::Notice`。

它今天承载的是（`src/render/mod.rs:144` 的 `RenderHandle::notice` 与它的调用方）：

- 启动横幅与「接着哪场会话」（`src/cli.rs:453`、`:463`）；
- **全部错误报告**（`src/cli.rs:1007-1054`）；
- **命令回执**：`/undo` 的成败、`/goal` / `/loop` / `/clear` 的反馈（`src/cli.rs:1173-1376`）；
- 目标与重试的提示：`goal_completed_notice`、`goal_reminded`、`goal_rolled_over`、`provider_retry`（`src/cli.rs:1950-2074`）；
- 历史分隔线（`src/render/tui.rs:1931`）。

按字面执行冻结项 2 + 8，这些**都会离开对话视图**，而它们正是「系统在回答用户」。要定的是**保留清单的最终形态**：

- 把 `Notice` 整类留下；
- 还是按语义拆（命令回执与错误留、目标提醒进轨迹）；
- 还是把清单从枚举改成一条判据（「因用户动作而起的、或需要用户据以行动的」）。

顺带过一遍其余没被点名的块，确认它们真的该走：`Usage`、`Divergence`、`RoundStarted` / `RoundEnded`、`Sandbox`、`History`（压缩 / 撤销那类叙述）。

## 产物

一份**保留清单**（枚举，或一条判据加边界例），写进 `/to-spec` 的 spec；[prototype：对话视图瘦身之后的形态](02-prototype-conversation-view.md) 按它画形态。

## 咨询的 skills

`/grilling` + `/domain-modeling`（若清单从枚举变成判据，那判据要么进 `CONTEXT.md` 的**对话视图**词条，要么在 `docs/render.md` 里立一节）。

## 作答

**决定（2026-10-05，HITL：维护者直接给了答案）**：**`Notice` 整类留在对话视图。**

于是对话视图的保留清单是**枚举出来的六类**：

1. 用户消息与 assistant 正文（本来就是对话）；
2. `AgentError` / `SessionError`（错误）；
3. `SessionEnded`（会话中断）；
4. `PermissionAsked` / `PermissionDecided`（权限裁决）；
5. 失败的 `Hook`（hook 拒绝）；
6. **`Notice` 整类** —— 命令回执（`/undo` / `/goal` / `/loop` / `/clear` 的反馈）、启动横幅与「接着哪场会话」、全部错误报告、目标与重试提示、历史分隔线。

**进轨迹的**（本票顺带过了一遍）：`Usage`、`Divergence`、`Sandbox`、`History`（压缩 / 撤销那类叙述）、`ContextInjected`、`Tool` / `ToolFeedback`、思考行、执行者的全部行。**回合 / 轮次边界行（`TurnStarted` / `TurnEnded` / `RoundStarted` / `RoundEnded`）不在其中** —— 它们是对话的分段线，由[对话视图形态那张票](02-prototype-conversation-view.md)的决定留在对话视图（维护者：「都留着，也先不动」）。

**判据的形态**：保留清单是**枚举**，不是判据 —— 维护者的口径是「先整类留下看看效果」，所以那条判据（「因用户动作而起的、或需要用户据以行动的」）留作日后的收紧方向，不进本 spec。

**代价（显式写下来）**：`Notice` 整类里混着过程性的提示（目标提醒、重试提示、历史分隔线），它们会留在对话视图里。这是「先看效果」换来的；日后若要收窄，本票是那条线上的第一个点。

**未证实**：`Notice` 整类里各条的实际出现频率没数过。若它在长会话里太吵，「按语义拆」是现成的退路（本票问题里的第二个选项）。
