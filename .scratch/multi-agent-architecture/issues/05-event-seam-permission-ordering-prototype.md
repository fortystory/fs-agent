# 事件接缝与权限门的控制流次序（prototype）

Type: prototype
Status: resolved
Blocked by: 15

## Question

用**粗糙可跑的代码**回答一个问题：**工具调用前后的事件挂载点与权限门，谁拥有控制流？**

**本票已改写**：它现在跑在票 15 定下的事件流骨架之上（原旧图版本被 01、03 阻塞，现在改为被 15 阻塞，因为钩子的语义就是"在流的哪个位置插入"）。

必须回答的具体问题：

1. **次序。** 综述引 Claude Code 的规则：`PreToolUse` **在任何权限检查之前触发，且在所有模式下都触发（包括 `bypassPermissions`）**。原文是 "Hooks can tighten restrictions but not loosen them."——即 **hook 只能收紧、不能放松权限**。这个"谁在外层"在代码里怎么表达？
2. **`PostToolUse` 回灌。** 综述把它和 aider 的 lint/test 反省循环（`max_reflections = 3`）并列，标为 ROI 很高："把 verify 从模型的自觉变成 harness 的保证"。它的产物是**注入一条新事件/新消息**，还是**改写工具结果**？在多 agent 下，回灌给谁——只给发起调用的那个 agent，还是全体讨论者？
3. **权限门的返回值有几态。** 综述最小可用版是 `readonly` / `ask` / `auto` 三档，外加危险命令黑名单与 cwd 路径限制。但**多 agent 要求"权限沿委派链继承"**（票 20 重开了权限表达），所以三态够不够？要不要预留更细的 allow/deny/ask 表达？
4. **边界情况**：hook 失败、hook 超时、hook 返回"放松"的意图——各自怎么处理？（第 1 条已经说了"不能放松"，但那是策略，还要定失败行为。）
5. **与权限询问事件的交互**：权限询问本身是流上的一条事件（票 15），那么 hook 在询问之前还是之后？这决定了"hook 能不能拦下一次询问"。

**票 03 交接来的边界（2026-09-12）**：工具层**不做**权限与 hook——`ToolRegistry::dispatch` 只负责副作用分流、`read-before-edit` 与 per-path 写互斥；次序 `hook.pre → 权限门 → dispatch → hook.post` 由 `agent` 组装（票 03 第 6 节）。另外，**权限拒绝的产物必须是一条错误内容的工具结果**（票 02 §6 的不变量：每个 `tool_call` 恰好一条结果），**不得靠丢弃 tool call 来表达拒绝**——请在本票定下它由谁产生（权限门还是循环）。

**为什么用 prototype 而不是纸上争论**：这个次序很微妙，且错了要等到实现阶段才暴露。一个能跑的最小骨架（一两个 stub 工具 + 一个 stub 权限门 + 两个挂载点 + 票 15 的最小事件流）就能看出接缝对不对。

**产物形态**：丢弃式原型，**不是**正式代码。按综述/wayfinder 的约定，原型留在 `prototype/<name>` 分支上作为原始资料，并由本票链接；结论折回真实代码。

**先读**：`.scratch/multi-agent-architecture/research/01-debate-conformity-and-speaker-attribution.md` 第 3 节（真实项目怎么表达"工具调用前后各一个挂载点"——`rig-agent` 的 `AgentHook` 是最贴近的先例，**别从零发明**）。

**票 15 交接来的事实（2026-09-13）**：事件流骨架已定，原型可以照着搭。

- **hook 公开子集 = 一个封闭类型的 7 个变体**：`SessionStarted`、`SessionEnded`、`ToolCallStarted`、`ToolCallCompleted`、`PermissionAsked`、`PermissionDecided`、`AgentError`。判据是"hook 只能收紧、不能放松权限"（你第 1 条查证的那条）→ hook 是**策略**机制，它的面就该是**策略点**；**观察**面归票 19（它有另一条路：读整条流）。
- 第 5 条问的"hook 在询问之前还是之后"就落在两条事件的相对位置上：**`PermissionAsked { request_id, tool_call_id, request }`** 与 **`PermissionDecided { request_id, decision, source: User | Hook | Policy, reason? }`**——`source` 里那个 `Hook` 变体是给你第 1 条预备的（hook 收紧权限的落点）。
- `HookExecuted { point, command, outcome }` 的字段照 OpenHands 磨过的形状（`hook_event_type` / `command` / `success` / `blocked` / `exit_code` / `stdout` / `stderr` / `reason`）。
- **对第 2 条（`PostToolUse` 回灌形态）的直接约束**：日志只记**完成单元**，且**事件永不修改、永不截断**——所以"**改写工具结果**"这个选项已经被排除。回灌只能是一条**追加事件**：要么追加一条新的工具结果语义事件，要么用 `HistorySuperseded` 声明原结果被替代。请在原型里选一个并说明它如何保住票 02 那条"每个 `tool_call` 恰好一条结果"的不变量。

## Answer

**已定（2026-09-13，prototype + grilling，5 问逐条确认）。**

**先读材料的更正**：票面写的"research/01 第 3 节（rig-agent 的 `AgentHook`）"是笔误——那份简报第 3 节讲的是说话人归属；rig-agent 的事实实际在 **`.scratch/multi-agent-architecture/research/08-rust-ecosystem.md` §3.1**：`on_tool_call` *"Runs before a valid tool call is executed. The hook may **rewrite the current arguments, skip execution, or stop the run**."*；`on_tool_result` *"Runs **after a tool call resolves and before its presentation is sent to the model**."*；runner 是 **fail-closed**；同节还确认**全 Rust 生态没有拿 channel 当 hook 机制的**（一律 trait + typed enum 返回值）。

**产物（原型）**：`.scratch/multi-agent-architecture/prototype/05-hook-permission-ordering.html` —— 单文件、双击即开、无依赖的可按压状态机；7 个带剧本的演练（顺利路径 / hook 收紧成询问 / 黑名单 / hook 拦截询问 / pre 失败 fail-closed / PostToolUse 回灌 / 跳过执行）；右侧事件流用的正是**票 15 的事件名**。

> **落点偏离（要看）**：票面说原型留在 `prototype/<name>` 分支。本仓库只有 `master` 一个提交，整个图都是工作区里**未跟踪**的文件——切分支会让共享 worktree 里的并发会话撞车，所以原型以**文件**形式留在工作区，由本票链接，结论照常折回代码。
>
> **原型已 headless 验证**：7 个剧本全部跑通；不变量「每个 `tool_call` 恰好一条结果」在**权限拒绝 / hook 跳过 / hook.pre 失败**三条路径上都成立（各合成一条结果）；非法阶段的动作无副作用；"hook 放松"被决策格挡住（门 = `deny`、hook = `ask`、生效 = `deny`）。

### 1. 谁拥有控制流（票面第 1 条）

**`agent` 层的循环拥有控制流。** hook 与权限门都不是控制流的拥有者，而是被循环**按次序调用的纯值变换**：

```
循环 ──① hook.pre（策略：读事件 → 返回动作）──▶
     ──② 权限门（纯函数：策略 + 工具 → 裁决）──▶
     ──③ [询问用户] ──▶
     ──④ ToolRegistry::dispatch（票 03：只管副作用分流 / read-before-edit / 写互斥）──▶
     ──⑤ hook.post（策略：读结果 → 返回反馈）──▶
     ──⑥ 循环追加事件（**合成的结果也在这一步**）
```

**"hook 先跑"与"只能收紧"如何同时成立**：hook.pre **先跑**（Claude Code 明文："`PreToolUse` 在任何权限检查之前触发，且在所有模式下都触发"），但它的输出**不是裁决，是约束**：

- 权限门产出裁决：`Verdict = Allow | Ask | Deny`（纯函数：不读环境、不与人交互、不追加事件）。
- hook.pre 的返回值是一个 typed 枚举：`Continue | Rewrite(args) | Tighten(Ask | Deny) | Skip | Stop`。
- **生效裁决 = 两者在决策格 `Allow < Ask < Deny` 上取上确界。**

⇒ **"hook 只能收紧、不能放松"不是运行时检查，是代数性质**：返回值类型里根本没有"放松"这个变体，而取上确界是单调的。原型自由操作区那个虚线按钮演示了这一点——它永远不可点。

- **为什么不是"门先算、hook 再收紧"**：`Rewrite(args)` 必须发生在门**之前**，否则门是对旧参数裁决的。
- **为什么不是"hook 与门合成一条按优先级排序的 pipeline"**（adk-rust 的形状）：hook 能改写 / 跳过 / 停止，门不能——两者不是同一种东西，合并成一条管道只会让类型胀大。

### 2. PostToolUse 回灌（票面第 2 条）

**产物 = 一条追加事件**：`HookExecuted { point: PostToolUse, outcome: 反馈 }`。**不改写 `ToolCallCompleted`**——事件永不修改（票 15 第 4 节）。

- 日志里 **tool 结果与 hook 反馈是两条事件**，各自发生在自己的时刻。
- **合并成一条 tool 消息是投影（票 17）的事**。而且这不是风格选择：**provider 只允许一个 `tool_call` 对应一条 tool 消息**，所以反馈**必须**并进那一条，不能另起一条。
- 投影因此需要一条**通用**规则（不是 hook 特例）：*对每个 `tool_call`，按 `seq` 收集它的**所有结果承载事件**，拼成一条 tool 消息*。

**回灌给谁 = 跟着那条工具结果的可见性走。** 不新增广播规则：谁的投影包含这条工具结果，谁就同时看到它的 hook 反馈。理由：反馈是对"这次调用"的评注，脱离调用没有意义；而"别人的工具往返投不投"本来就归票 17 定。

**被否掉的替代**：hook.post 在结果入流**之前**跑、把反馈直接并进 `ToolCallCompleted` 的 payload。它少一条事件、投影不需要合并规则；但代价是 **tool 结果在 hook 跑完前不入流**——渲染器看不到，一个挂住的 hook 能把结果无限期藏起来，而且日志会就"结果是什么时候产生的"撒谎。

### 3. 权限门的裁决态（票面第 3 条）

**裁决是封闭三态** `Allow | Ask | Deny`（外加一个 `why` 供诊断与展示）。

- **它必须封闭**：决策格需要**全序**才能定义"更严"；多一个态，"只能收紧"就失去定义。
- **打开的是策略语言，不是裁决**：`Policy → Verdict` 是纯函数，其**输入侧**（模式 `readonly` / `ask` / `auto`、危险命令黑名单、cwd 路径限制、**规则表达式**、**沿委派链继承**）全是**票 20** 的地盘。
- ⇒ 回答"三态够不够"：**作为裁决够；不够的是表达力**——而表达力是门的**输入**，不是门的**输出**。这也把票 20 的规模钉在"规则语言"上。
- 顺带澄清：**"允许一次 / 总是允许"不是第四态**。"总是允许"是**用户的回答修改了策略**，之后裁决仍然是 `Allow`；把它做成第四态，等于把可变状态塞进一个纯函数。

### 4. 失败、超时与"放松意图"（票面第 4 条）

- **"放松"意图：类型上不存在**（第 1 节）。它不是一个需要处理的边界情况，是**非法状态不可表示**。若 hook 进程的输出无法解析成 `PreToolAction`，归入"失败"。
- **失败 / 超时 = fail-closed，但两个方向不对称——这是这个接缝最重要的事实**：
  - **`hook.pre` 失败 / 超时 → 阻止动作**：工具不运行 + 追加 `SessionError` 诊断 + **合成一条错误结果**（保住不变量）。rig 的 runner 同样 fail-closed（"返回该事件无法执行的动作时结束 run 并给诊断，而不是静默忽略"）。
  - **`hook.post` 失败 / 超时 → 只能丢反馈**：**世界已经改变，无法撤销**。追加 `HookExecuted{failed}` + `SessionError` 诊断，反馈丢失。fail-closed 在 post 侧**买不到任何安全**。
- **超时算失败**：hook 同步拦在关键路径上，挂住它就是挂住整个 turn。超时阈值是配置项。
- **失败后循环继续**（该 turn 以一条错误结果收尾），而不是像 rig 那样结束整个 run——自用工具里写坏一个 hook 不该把会话打死，用户看到诊断后可以修。
- **不采用 Claude Code 的退出码分档**（exit 2 阻断、其他非零仅告警）：退出码是 shell 约定，不是我们的类型系统。将来若要"报错但不阻断"，应该用**显式 outcome** 表达，而不是魔法值。

### 5. 与权限询问事件的相对位置（票面第 5 条）

hook.pre **在询问之前**：

- **能阻止一次询问发生**：收紧到 `Deny` ⇒ 询问根本不发生，用户看不到任何提示（原型剧本 4 演示）。
- **永远不能绕过一次询问**：`Ask → Allow` 不存在。
- **v1 不给"询问本身"另设 hook 点**（不做 Claude Code 的 `PermissionRequest` hook）。两条理由：① 它与 pre 点一样"只能收紧"，表达力重复；② **hook 能看见的事件集就是它能挂的点集的来源**——票 15 的 hook 公开子集里关于权限的只有 `PermissionAsked` 与 `PermissionDecided`，多一个挂载点就要多一套次序约定。

**票 03 交接来的问题（"权限拒绝的产物由谁产生"）**：**由 `agent` 层的循环合成**，不是权限门、更不是工具。门是纯函数（不能追加事件），工具根本没被调用（不可能产出结果）——所以只能是循环。它在离开权限门时追加 `PermissionDecided { decision: Deny }` 与一条 `ToolCallCompleted { ok: false, error: "权限被拒绝" }`，**"每个 `tool_call` 恰好一条结果"因此成立**。同一条规则覆盖另外三条路径：**用户拒绝 / hook 跳过 / hook.pre 失败**（各合成一条）。

### 明确不做

- `PermissionRequest` 挂载点（第 5 节）、退出码分档（第 4 节）、细粒度规则表达式（第 3 节，归票 20）、用 post-hook 反馈改写工具结果（第 2 节）。
