# 硬 plan 模式：用权限档位实现，不新增状态机

Type: grilling
Status: open
Blocked by: 01, 05

## Question

ratify 第 9 条以"**模式 ≠ 工具**"解决了综述的内部矛盾：保留**硬 plan 模式**，砍掉 todo 工具。本票把实现路径定死。

综述（第 7 节）的证据链指向一个具体结论，值得完整引用：

> plan 模式正在变成**基础设施**而不是产品功能：Gemini CLI 的 plan 模式**默认启用**（`--approval-mode=plan`、`/plan [goal]`、Shift+Tab 循环，并用 `enter_plan_mode` / `exit_plan_mode` 两个**工具**进出）；opencode 的 plan 模式**干脆就是权限配置的产物**（把 `edit` 与 `bash` 设为 `ask`）。结论：**"禁写"这件事的正确实现位置是工具层的权限判定，而不是一个新状态机。**
>
> goose 走得更省：它把 ACP 的 `plan` 模式**映射到已有的 `Chat` 模式（工具全关）**，CLI 侧是 `/plan` 与 `/endplan`。

要回答：

1. **用哪条路线**：opencode 式（权限档位的一次预设）还是 goose 式（映射到已有的全关模式）？综述的结论是**不需要新状态机** —— 请确认这个结论，并说明它如何复用票 05 的权限门。
2. **进出方式**：工具（Gemini 的 `enter_plan_mode` / `exit_plan_mode`）还是手势（Shift+Tab / `/plan`）？还是两者都要？
3. **计划本身落在哪**。两个候选：**goose 的隐藏续跑提示** —— `/goal` 与 `/grind` 会往对话里注入**隐藏的 continuation nudge**（模型看到、用户不显式看到），综述称这是"想防跑偏但不想做 todo 工具时最便宜的做法，值得作为首选替代方案"；或 **OpenHands 的预设 agent** —— 一个只带 glob + grep + 一个只能写计划的 editor 的 agent，产出 `PLAN.md`。
4. **计划在 compaction 后是否重新注入**？Claude Code 会从磁盘重新注入。注意自动 compaction 是后置项。

**不要**做：`todo_write` 式结构化清单、每轮重新注入清单、计划审批流、用评审模型做目标验收（综述：OpenHands 的 `GoalController` 用一个 judge LLM + `max_iterations = 10`，那是比 todo 更强的一层，本图不做）。
