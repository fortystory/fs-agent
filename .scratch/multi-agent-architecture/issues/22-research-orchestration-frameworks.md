# research：多 agent 编排框架的轮次管理与终止条件

Type: research
Status: resolved

## Question

本票为**票 16（讨论协议与轮次）**提供外部事实。它是 AFK 的：由子代理读一手文档，产出带来源链接的简报，**不给方案**。

**为什么需要它**：已有两份研究覆盖了"讨论会不会塌"（`research/01`）与"两家 provider 能调什么"（`research/02`），但**都承认一个空白**——`research/01` 第 3.2 节明确标注 **CrewAI / LangGraph 本轮未验证**，而 AutoGen 只覆盖了它的**消息归属编码**，**没有覆盖它的轮次管理与终止条件**。而票 16 要定的正是"谁在何时发言、讨论何时停"。

需要查明（判据：只报告事实与来源，不下结论）：

1. **Microsoft AutoGen**（重点，它是"多 agent 对话"的经典实现）：
   - `GroupChat` / `GroupChatManager` 的**发言者选择**机制有哪些（round-robin / 自动选择 / 手写函数 / `selector_func`）？各自的文档语义是什么？
   - **终止条件**怎么表达？`max_round` / `is_termination_msg` / `TerminationCondition` 的当前 API 形状与语义。
   - 有没有"检测分歧 / 收敛"的内建机制，还是完全交给用户写终止函数？
   - 它怎么处理**多轮之后上下文膨胀**（有没有内建的裁剪）？
   - 注意版本差异：0.2 与 0.4+ 的 API 差别很大，请标明你引的是哪个版本。
2. **CrewAI**：任务的顺序 / 层级（hierarchical）执行怎么定义？有没有轮的终止条件？谁决定发言？
3. **LangGraph**：它的图结构如何表达"轮次"与"条件边"？`interrupt` / checkpoint 机制与"暂停讨论等人类输入"的关系。它在多 agent（supervisor / swarm）上给出的官方模式是什么？
4. **终止条件的常见形状**：这三家（以及任何你找到的其他一手来源，如 MetaGPT、OpenAI Swarm 的继任者）实际用的终止判据是什么——最大轮数、显式终止消息、收敛阈值、裁判模型、还是成本上限？把**判据种类**列全。
5. **"定向重开一轮"有没有先例**：任何框架里有"只在参与者结论不一致时才继续"的机制吗（例如投票平局才继续、或 supervisor 判断需要更多讨论）？如果没有，明确说没有。
6. **裁判/聚合器的官方做法**：谁做最终合成——最后一个 agent、专门的 synthesizer 角色、还是外部代码？有没有对裁判偏见（位置、自我偏好）的官方缓解说明？

**产物**：一份带来源链接的中文简报，写到 `.scratch/multi-agent-architecture/research/03-orchestration-frameworks-turn-management.md`，并在本票留下指针。

**约定**：wayfinder 原本要求研究产物落在 `research/<name>` 一次性分支上；本仓库的 tracker 是本地 markdown，所以改用文件形式，不建分支。

**明确不要**：不要报告"多 agent 讨论是否提升准确率"（`research/01` 已覆盖），也不要报告 provider 参数（`research/02` 已覆盖）。

## Answer

**已解决**（2026-09-12，research 子代理）。产物：`.scratch/multi-agent-architecture/research/03-orchestration-frameworks-turn-management.md` —— 729 行、68 条来源链接（11 个唯一域名），全篇标注框架版本。主代理已核实文件存在且关键论断全部命中。

### 最高价值的结论：本票第 5 点被核实为「缺失」

> **没有任何被查框架提供内建的「只在结论不一致时才继续」机制。**
> 最接近的 5 个官方构造**全部是「失败 / 停滞 / 未达标才继续」**，而不是「分歧才继续」：AutoGen 的 `selector_func` 官方 `Correct!/Incorrect!` 示例、CrewAI 的 guardrail 重试、LangGraph 的 evaluator-optimizer、Deep Agents 的 `RubricMiddleware`、Magentic-One 的停滞重规划。

**意义**：用户选定的「定向第二轮」在编排框架里**没有先例**。它是**设计**，不是**选型**——不会在任何来源里找到证实，只能靠自己的判断加后来实测。票 16 已据此更新。

### 其他可直接使用的事实

1. **终止条件做成可组合的一等对象有强先例**（AutoGen 0.4+）：`TerminationCondition` **对增量消息求值**、返回 `StopMessage|None`、可用 `&` / `|` 组合，内建 11 种（max messages / text / token / timeout / handoff / source match / external / stop msg / text msg / function call / functional）。**这个形状天然吻合 fs-agent 的事件流（票 15）**：终止 = 对事件流求值的可组合谓词。
2. **一个真实的 API 语义反转陷阱**：`selector_func` 返回 `None`，在 AutoGen **0.2 意味着终止整场**，在 **0.4+ 意味着退回模型选择**。同名同形、语义相反 → 教训：**终止语义必须显式建模，不能靠返回值巧合**。
3. **默认值不可信**：AutoGen 0.2 `max_round` 默认 10、`is_termination_msg` 默认是**严格字符串等于 `"TERMINATE"`**；0.4+ `max_turns` 默认 **None（不限轮）**。LangGraph 的 `recursion_limit` **文档写 1000、main 代码常量是 `"10007"`（文档/代码冲突，文件已如实并列）**。
4. **上下文膨胀的三种姿态**（影响票 06 / 17）：AutoGen 0.4 **默认不裁剪**（`UnboundedChatCompletionContext`），可选的 Buffered / TokenLimited / HeadAndTail 是**视图级**（不自动摘要）；CrewAI **自动摘要**（`respect_context_window=True`）；LangGraph **不自动处理**（需自配 SummarizationMiddleware / pre_model_hook）。**AutoGen 的「视图级」措辞正好印证 fs-agent 的投影设计（票 17）**：上下文策略是一种视图，不是对历史的破坏性改写。
5. **裁判 / 合成的先例**：AutoGen 一般 Team **不产出 synthesizer**（Magentic-One 是例外，Orchestrator 自己写最终答案）；CrewAI 用**最后一个 task 的输出**；LangGraph 的 Router 模式画了一个**独立的 Synthesize 节点**（用户自己写）。
6. **裁判偏见的硬数字与官方缓解**（影响票 16 第 1、3 问）：MT-Bench 原文——GPT-4 位置一致性 **65.0%**，Claude-v1 仅 **23.8%** 且 **75% 偏向第一个**；缓解为 **swap / few-shot / CoT**。**Anthropic 的官方实践是「单次调用 + rubric + 0-1 分与 pass/fail」，且其自身实验发现多裁判反而更差** → 尽量**别**引入独立裁判；若引入，单次调用 + rubric，并注意 swap 会让调用数翻倍（与票 18 的成本上限冲突）。

### 未能从一手来源验证（详见文件第 7 节）

LangGraph `recursion_limit` 默认值（文档 1000 vs 代码 10007）；`langgraph-swarm` 是否记录任何终止语义（无）；**平台官方文档中关于裁判偏见的缓解说明**（Vertex AI eval 两页全文检索 position / order bias / swap / self-preference 命中 **0**，LangSmith 亦未提——所以缓解建议只有 MT-Bench 论文，**没有平台背书**）；CrewAI `max_iter` 默认值只取文档值 20（未核源码）；AutoGen `.stable` 文档站的确切版本号；AutoGen 0.2 `max_round` 一轮的精确计数单位。
