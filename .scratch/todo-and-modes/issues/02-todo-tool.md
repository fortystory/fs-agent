# `todo` 工具：模型自己维护的待办列表

Type: implement
Status: done
Blocked by: 01

> 规格：`.scratch/todo-and-modes/spec.md` §2、§3、`测试决定`。
> 依赖票 01：`plan` 退场之后，「计划」这件事才由这个工具独占（否则两套机制同时在讲同一件事）。

## 目标

一个内建工具 `todo(list)`，让模型一次提交整份待办列表；列表就活在那条 `tool_call` 的 args / result 里（**零 schema 改动**），并在规则段里被引导去用它。

## 落点

`src/tools/todo.rs`（新增）+ `src/tools/mod.rs`（注册与工具表）、`src/lib.rs`（组装：主会话 / 执行者 / 三个渲染器）、`src/agent.rs`（规则段 `agent_identity()`）、`CONTEXT.md`、`docs/render.md`、`tests/todo.rs`（新增）。

## 具体行为

1. **契约**：`todo(items)`
   - `items: [ { content: string, status: "pending" | "in_progress" | "completed" } ]`；
   - **一次提交整份**（replace-all）：缺省或空数组 = **清空**列表；
   - `content` 必须非空、`status` 限三档，越界返回**模型可读的错误结果**（不是拒绝运行、也不是 panic）；
   - 工具描述（英文，模型可见）写清这三条 + 「一次交整份」这条语义。
2. **结果**：一条英文回执（例如 `todo: 5 items (2 completed)`）；**真相是 args**，结果只做回执 —— 侧栏与将来的读者都从 args 重算，不从结果文本里解析。
3. **`effect()` = `ReadOnly`**：它不碰工作区。于是同一条助手消息里的两次并发调用由 `seq` 定序，**后落地的那条是真相** —— 写进测试，别让它成为「随机」。
4. **挂载面**：主会话、讨论者、**执行者**都挂；**headless 也挂**（这个不需要人，与 `ask_user_question` 的分界就在这里）。工具表是**前缀**的一部分，所以这仍然是组装期的决定，不做「按阶段增删」。
5. **规则段**：`agent::agent_identity()` 里加一条英文指令 —— 开工前先立待办、每完成一项就更新、收尾时用一次全 `completed` 的更新收尾。**只增不改**（ADR 0001：进 `messages` 的一侧），并在 ADR 0003 里记一笔「这条改动动了缓存前缀」。
6. **不强制**（spec §3 的选择）：不做首轮 `ToolChoice::Tool("todo")`，不做门层强制。
7. `CONTEXT.md` 新增 **待办列表（Todo）** 与 **待办工具（`todo`）** 两条词条；`docs/render.md` 的工具那一节补上它。

> **注（2026-09-30）**：本节里三处写「英文」的地方 —— 第 1 条的「工具描述（英文，模型可见）」、第 2 条的「一条英文回执」、第 5 条的「加一条英文指令」—— 都由 [ADR 0005](../../../docs/adr/0005-model-visible-text-in-chinese.md) 接管：模型可见与进流的**散文**也走中文，英文只留给标识符、schema 值与协议标记。现状：`src/tools/todo.rs` 的工具描述、三条错误结果与回执都是中文（回执是 `todo：{} 项（{} 项已完成）`，空列表是 `todo：已清空`，见 `src/tools/todo.rs:170`），规则段是中文（`src/agent.rs:151`）。
> 第 5 条的另一半理由没变：这仍是**缓存前缀的改动**，ADR 0001 的「进 `messages` 的一侧只增不改」照样最硬，ADR 0003 里那笔账照记。

## 测试

新建 `tests/todo.rs`：

- 合法调用返回回执，且 args 形状与输入一致（逐字断言 JSON）；
- 空数组 / 缺省 `items` = 清空；
- `content` 为空串、`status` 非法值、`items` 不是数组 → 模型可读的错误结果；
- `effect()` 是 `ReadOnly`；同一条消息里两次并发调用按 `seq` 定序（后落地者赢）；
- 工具表：主会话有、执行者有、headless 有、讨论者会话有；
- 端到端：假 provider 起真会话，模型调 `todo`，断言那条 `tool_call` 有且只有一条结果；
- 规则段：`agent_identity()` 含那条指令。
