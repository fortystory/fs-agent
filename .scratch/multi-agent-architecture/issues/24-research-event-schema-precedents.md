# research：事件 schema 的三个一手先例（OpenHands / Cline / Claude Code）

Type: research
Status: resolved

## Question

本票为**票 15（事件流骨架与事件 schema）**提供外部事实：它的 payload 枚举（票面第 1 条）该长什么样，别处已经趟过。综述只给了结论（"OpenHands 以 event stream 为唯一真相源"），没给 schema；而枚举设计最怕凭空发明。

判据：**只报告事实与来源，不推荐方案、不选赢家**。凡一手来源未写明者标 ⚪ 未证实，**不做推断**。

需要查明（**逐个一手来源**：官方文档与官方仓库源码；第三方博客/教程不算）：

1. **OpenHands 的 event stream**：事件类型枚举（逐条列出，如 `Action` / `Observation` 各自有哪些子类）、**信封 / 公共字段**（有无事件 id、时间戳、因果 id、来源 agent、父事件）、事件是否只追加、以及 **`fork(from_event_id=…)` 的确切语义**（从哪里复制、新会话如何标记来源）。
2. **Cline SDK events**：事件类型清单；**停止条件枚举**（`completed | max_iterations | aborted | mistake_limit | error`）的确切定义与各自的触发条件；这些 reason 具体挂在哪个事件上。来源：`docs.cline.bot/sdk/events` 与官方仓库。
3. **Claude Code 的会话 JSONL**：一条记录的**字段形状**（逐字段列出）；哪些东西进日志（消息 / 工具调用 / 工具结果 / token 用量 / thinking）；**`--continue` / `--resume` / `--fork-session` / `/branch` 各自怎么做**（追加同一 session id，还是把历史复制到新 id）。来源：官方文档 + 综述里引的 `how-claude-code-works` / `sessions` 两页。
4. **增量文本进不进日志**：以上三家有没有明确表述（例如"流式增量只到 UI，落盘的是完整消息"）？没有明说 → ⚪。
5. **"重新生成 / 撤销"的机制**：以上三家如何表达（追加补偿事件？fork 新分支？覆盖重写？）——有一手依据就报，没有就 ⚪。

**边界**：不要调研本项目的多 agent 讨论协议（票 16）或会话存储拓扑（票 07）。本票只报"别人长什么样"。

产出：`.scratch/multi-agent-architecture/research/05-event-schema-precedents.md`（中文；来源 ID 表 + 逐条引用 + ⚪ 缺口清单 + 完整 URL 清单；格式照 `research/02-provider-call-surface.md` 与 `research/04-tree-sitter-and-symbol-extraction.md`）。

**本票不决定任何事**——它是票 15 的事实输入。

## Answer

**已解（2026-09-12，AFK，由 research 子代理执行）。**

**产物**：`.scratch/multi-agent-architecture/research/05-event-schema-precedents.md`（401 行：来源 ID 表 + 逐问表格 + ⚪ 清单 + 完整 URL 清单）。

对票 15 最相关的几条（细节与来源见简报）：

- **OpenHands（最接近本图的先例）**：`Event` 信封逐字为 `id`（uuid4/ULID）+ `timestamp`(isoformat) + `source: Literal["agent","user","environment","hook"]` + `parent_id`（**对话树**父事件，哨兵 `ROOT_PARENT_ID="__root__"`）；判别字段 `kind` = 类名；模型 `frozen=True`；`EventLog.append` **拒绝重复 id 与不存在的父事件**，append-only 明确。因果 id **不是单一字段**，而是 `llm_response_id` / `action_id` / `tool_call_id` 三个。共 21 个导出事件类。
- **增量不进日志（独立验证了票 15 第 1 条的判据）**：OpenHands 的 `StreamingDeltaEvent` docstring 明确如此。Cline 与 Claude Code 在这一点上是 ⚪。
- **fork 的确切形状**：`fork(from_event_id=…)` 只复制 `path_to_root(from_event_id)` 并把新会话 HEAD 设在该点；另有 `navigate_to(event_id)` **移动 HEAD、日志一条不删**。官方文档页的 API Reference **漏了 `from_event_id` 参数**（与源码不一致，简报已标注）。
- **Cline**：`AgentFinishReason = "completed" | "max_iterations" | "aborted" | "mistake_limit" | "error"`，挂在 `AgentDoneEvent.reason` 与 `AgentResult.finishReason`；只有 `mistake_limit` 有文档化触发条件（连续可恢复错误达 `maxConsecutiveMistakes`，默认 6），其余 reason 的判据未写明（⚪）。host 信封只有 `agentId/conversationId/parentAgentId`（**无**事件 id / 时间戳）。
- **Claude Code**：transcript 是 JSONL，但官方明说 **entry format is internal、随版本变化**；唯一的逐字段形状是 `SessionMessage{uuid, session_id, message, parent_tool_use_id, parent_agent_id}`。`--continue` / `--resume` **追加同一 session**（两个终端并行恢复会交错进同一 transcript）；`--fork-session` / `/branch` **把历史复制到新 session id**；SDK 另有**截断式** `resumeSessionAt` + `resumeDropsTurn`。
- ⚪ 缺口：Claude Code JSONL 的完整 schema / 记录类型枚举、thinking 与 token 用量是否落盘、`stream_event` 是否写盘；Cline 的增量是否落盘、非 `mistake_limit` 各 reason 的判据、`aborted` 与 `abort()` 的显式映射；`/rewind` 在记录层的实现。另：票面点名的 `how-claude-code-works` 页正文本次只抓到导航壳，未核对。
