# `goal_note`：执行中冒出来的新工作

Type: implement
Status: ready-for-agent
Blocked by: 03

> 规格：`.scratch/goal-loop/spec.md` §11（`goal_note`）。
> 清单是**封闭**的（执行中不能往里加条目），所以「我发现还需要做 X」必须有另一个出口 —— 否则它只能活在模型的上下文里，而上下文过八成就要被压掉。

## 目标

给模型一个记录「新工作」的工具，**与 `todo` 完全同构**。

## 落点

- `src/tools/goal_note.rs`（新增）
- `src/tools/mod.rs`：注册
- `src/render/wording.rs`：回执文案
- `tests/goal_note.rs`（新增）

## 具体行为

1. **形状照抄 `todo`**：一次调用提交一条或一批 note；`effect()` 是 **`ReadOnly`** —— 它不碰工作区（`todo` 的先例：正因为不碰工作区，权限门从不为它发问），所以两次调用可以并发、无人值守时不会停下来等人。
2. **args 即真相**：note 活在那条 `tool_call` 的参数里，**不为它新增任何事件、不改 schema**。于是 `sessions replay` 能重算、`--continue` 后自然重建 —— 与 `todo` 一模一样。
3. **解析严格、错误可读**：照 `src/tools/todo.rs` 的手写解析 —— 非对象、未知字段、空的 note 都是模型能据以行动的一句话。
4. **结果是一句回执**（`goal_note：记下 2 条`），不是列表本身。
5. **挂载面与 `todo` 一致**：主会话、讨论者、执行者、headless。
6. **消费者是票 05 的汇总**：「执行中冒出来但没进清单的新工作」那一项从这里读 args。
7. **不进侧栏**：它只在转录里占一行（像执行者的 `todo` 那样）。

## 测试

- 合法调用的回执与 args 形状；
- 非对象 / 未知字段 / 空 note 各自的错误文本；
- `effect() == ReadOnly`；
- 挂载面：主会话、讨论者的表里有它；执行者的表里**也有它**（不像 `task` 那样被拿掉）；headless 也挂；
- 端到端：假 provider 起真会话，模型调 `goal_note`，断言流上那条 `tool_call` 有且只有一条结果，且票 05 的汇总读到的是 args 里的 note；
- 不做权限询问（`ReadOnly` ⇒ 门不问）。

## 不做什么

- 不把这些 note 写进清单文件（清单封闭、不带状态）。
- 不做 note 的编辑 / 删除 / 编号。
- 不新增事件变体（这是这一票最关键的一条「不做」）。
