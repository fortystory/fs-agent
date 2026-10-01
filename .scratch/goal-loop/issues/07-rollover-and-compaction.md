# 翻页与压缩：rollover 与摘要

Type: implement
Status: done
Blocked by: —

> 规格：`.scratch/goal-loop/spec.md` §7。
> 「结束当前会话、开一个新的」要变成一个**内部动作**（rollover），因为 `/clear`（票 09）与阈值触发的翻页（票 08）共用它 —— **两段入口、一段机制**。

## 目标

- 收掉当前会话、组装一个新的（同进程、同渲染器与终端）；
- 把历史压成摘要，随翻页带过去。

## 落点

- 组装层：rollover 动作（`assemble` 那一侧）
- `src/events.rs`：`HistorySuperseded { reason: Compaction }` 的产出者、`ContextSource` 新变体
- `src/context/`：摘要生成（一次模型调用）
- `tests/goal_loop.rs`

## 具体行为

1. **rollover 内部动作**：收掉当前会话（写收尾）→ 用**同一个渲染器与终端**组装一个新会话 → 继续。它是「一段机制」，两个入口分别是票 08（阈值）与票 09（`/clear`）。
   - 会话是「唯一持有可变状态的结构」，所以这一步是**重新组装一次会话**，不是清几个变量。照 `assemble` 既有的参数注入方式做，不要把状态塞进渲染器。
2. **压缩**：把当前历史折成摘要，落 **既有的** `HistorySuperseded { targets, reason: Compaction, summary }` —— 形状早已钉死在 schema 里，`summary` 字段都在，不需要新变体。
   - `targets` 指向被摘要替代的那批事件。
3. **摘要由一次模型调用生成**：把要折叠的历史交给 provider 压成一段散文。
   - **照记 `UsageRecorded`** —— 它就是一次 provider 调用，所以自然落进目标预算（票 10）。
   - 这次调用本身不许触发又一次翻页（判据要用压缩前的量）。
4. **摘要注入新会话**：`ContextInjected` + **一个新的 `ContextSource` 变体**（`Compaction` / `GoalHandoff`），与技能注入、人物注入同一个形状 —— 于是它进流、可重放，`--continue` 后还在。
5. **压缩与翻页在这个动作里总是成对发生**：不做「压缩后看空间够不够再决定翻不翻」那条分支。少一条路径、少一种状态要测。
6. **前缀缓存的代价如实记账**：压缩必然打掉前缀缓存，在转录里留一行（措辞层），不要让它悄悄发生。
7. **翻页只能发生在回合边界**：不能在 `tool_call` 还挂着结果的时候翻。
8. **旧会话留在磁盘上**：rollover 不删文件、不合并文件。`--continue` 打开的是**最新的**那个。

## 测试

- 端到端：造一个高用量的会话 → 触发 rollover → 断言
  - 流上有 `HistorySuperseded { reason: Compaction, summary: Some(..) }`；
  - 新会话的第一批事件里有 `ContextInjected`，内容是那段摘要；
  - 旧会话文件**仍在**磁盘上，且它的流是完整的；
  - `sessions replay` 对新旧两个会话都能跑通。
- 摘要那次调用落了一条 `UsageRecorded`；
- 压缩的判据用的是压缩前的量（不会自我触发第二次）；
- 回合边界：在一个 `tool_call` 未出结果时不翻页。

## 不做什么

- 不做阈值与提醒（票 08）、不做 `/clear` 的命令入口（票 09）。
- 不改压缩后「重新注入已加载技能 / 仓库地图」那套规则 —— 三处注释（`src/context.rs`、`context/skills.rs`、`context/repo_map.rs`）指的那件事，按规格只要求摘要进流，其余按现状（它们本来就从流派生）。
- 不做网络 / 多进程的会话切换。
