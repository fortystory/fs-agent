# 子 agent：独立预算与权限沿委派链的继承

Type: grilling
Status: closed
Blocked by: 01, 03, 05, 06

## Question

ratify 第 1 条翻案把「子 agent 与多 agent 编排」加进范围。综述原本的砍掉理由：「它是上下文管理的**替代方案**，不是前置条件；有它更好，没它 agent 也能用」，并用 mini-SWE-agent（无 subagent、只有 bash，仍 >74% SWE-bench verified）作证据。**翻案意味着你买的是一条上下文策略，不只是加一个工具。**

综述给的参考设计：

- **Claude Code**：独立上下文窗口、独立（更短的）system prompt、共享 `CLAUDE.md` / MCP / skills，但**默认不能再生成 subagent（防递归）**；**只有最终文本 + 一小段 token 元数据回到主会话**（例：subagent 读了 6.1k tokens，主上下文只增加 420）。
- **goose**：subagent 是**持久化的子会话**，两个硬预算 —— `GOOSE_SUBAGENT_MAX_TURNS` 默认 **25**、`GOOSE_MAX_BACKGROUND_TASKS` 默认 **5**。综述的结论：**子 agent 必须有独立预算**，否则一个跑飞的子 agent 会吃掉整个会话的轮数配额。
- **opencode**（权限继承的具体答案）：子 agent **只继承父级的 `deny` 规则和 `external_directory`**；`todowrite` 与 `task` **默认强制拒绝**，除非子 agent 自己的规则集里显式提到；`subagent_depth` 默认 **1**。综述的评价：**"继承拒绝、不继承允许"，且递归深度默认 1**。
- **Amp**：subagent 做成"专职角色 + 模型路由"（Search / Oracle / Librarian），且 **oracle 可配不同于主 agent 的模型** —— "so one model can review the other's reasoning"。

要回答：

1. **边界：先选一条路。** v1 的"子 agent"是**一个通用 `task` 工具**（输入 prompt、返回摘要、独立 messages 数组），还是**专职角色**（Search / Oracle / …，可配不同模型）？这两者是两条路，本票第一个问题就是选哪条。
2. **独立预算**：轮数上限取多少？（goose 是 25）。这要对上票 02 的"轮数上限可配、别硬编码常量"，以及票 06 的预算归属 —— 子 agent 的预算与主会话的预算如何隔离？
3. **权限继承**：确认用 opencode 的「继承拒绝、不继承允许」+ 递归深度 1？**本票必须在票 05 之后** —— 继承语义要在权限门的形状上表达。
4. **回传什么**：只有最终文本 + token 元数据，还是也回传它改了哪些文件？注意综述记录的一个坑：Claude Code 的 checkpoint 文档明确说 **subagent 的编辑一般不恢复** —— 那是个已知局限，别在 v1 里踩。
5. **编排的范围（边界问题）**：综述把「多 agent workflow / 编排 DSL」单列为 ⬜，理由是"需要一整套调度、错误传播、结果聚合的工程，远超极简可用"。**本票默认只做"一个子 agent"，不做编排。** 见 map 的 `Not yet specified`。

**明确不做**：多 agent workflow / 编排 DSL，除非你显式翻案（见第 5 点）。
