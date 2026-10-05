# 10 — 对话视图的过滤与形态

Type: implement
Status: ready-for-agent
Part of: ../map.md
Blocked by: 09

> 规格：[`../spec.md`](../spec.md) §2（分工与保留清单）与 §3 的两处例外。

## 目标

主列只剩**用户文本、assistant 正文、保留清单六类、回合 / 轮次边界行**；用户消息**右对齐**；注入 / 用户 / 助手三类前缀**各一色**。用户看得见：主列变成「聊天」，而过程行只剩左栏那一份。

## 现状（改前先复核）

- 对话视图与轨迹视图此时都画全量（上一张票的结果）。
- 前缀的颜色今天由发言者调色板给：用户 `LightGreen`、助手（单 agent 下是配置里的发言者名）`LightCyan`；**分不开的是注入** —— 它是无前缀的叙述行，与别的叙述行同为灰。
- 行级右对齐与折行保留对齐是现成能力。

## 落点

`src/render/tui.rs`（分工的纯函数与对话视图的绘制分支）、`src/render/wording.rs`（若前缀分档需要一个新入口）。

## 具体行为

1. **分工是一个纯函数**（块 → 目标集合），住在绘制侧；块层一个字节不改。
2. **保留清单（枚举六类）**：用户与 assistant 正文、`AgentError` / `SessionError`、`SessionEnded`、`PermissionAsked` / `PermissionDecided`、失败的 `Hook`、**`Notice` 整类**；外加**回合 / 轮次边界行**（`TurnStarted` / `TurnEnded` / `RoundStarted` / `RoundEnded`）—— 它们是分段线。
3. **进轨迹的**：执行者（`SpeakerId::Executor`）的全部行、`Usage`、`Divergence`、`Sandbox`、`History`、`ContextInjected`、`Tool` / `ToolFeedback`、思考行。
4. **用户消息右对齐**，只对话视图；助手与轨迹页仍左对齐。
5. **三类前缀各一色**：给注入行一个专色（用户与助手不动）。

## 验证

1. 帧断言：同一场会话里，过程行不在主列、在轨迹页；错误、`Notice`、边界行在主列。
2. 用户那行靠右、助手仍左对齐；注入行的颜色与用户、助手都不同。
3. 分工的纯函数**穷举单测**：每一类块至少一条。
4. 既有测试大面积要改（工具行与思考行的详情入口与内容断言）—— 这是预期的，改完必须全绿。

## 不做什么

- 不改块层（`Block` 的产生）、不改 plain / headless。
- 不做降级退回全量（[11](11-fallback-when-sidebar-hidden.md)）。
- 不给轨迹页加底色（[12](12-trace-round-stripes.md)）。
