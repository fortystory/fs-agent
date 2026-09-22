# fs-agent：事件 schema 的三个一手先例（OpenHands / Cline / Claude Code）

> 目的：为**票 15（事件流骨架与事件 schema）**提供外部事实输入——payload 枚举与信封该长什么样，别处已经趟过。本文件只报告**事实与来源**，不推荐方案、不选赢家、不下结论。
>
> 抓取日期：**2026-09-13（UTC）**。全部为官方文档、官方仓库源码、官方发布产物（npm 包 `.d.ts`）与官方 API 参考的阅读；**未运行任何工具、未调用任何 LLM API**。
>
> 来源分级：只使用 ✅ 一手来源（项目官方文档站、官方仓库 `raw.githubusercontent.com` 源码、官方 npm 发布包的 TypeScript 类型声明）。第三方博客 / 教程 / Stack Overflow 一概不用。凡一手来源未写明者一律标 **⚪ 未证实**，**不做推断**。
>
> 范围边界：本票**不**调研本项目的多 agent 讨论协议（票 16）与会话存储拓扑（票 07）；只报"别人长什么样"。

---

## 0. 来源清单与来源 ID

后文用 ID 引用；完整 URL 见第 6 节。

### OpenHands（Software Agent SDK，V1）

| ID | 来源 | 用途 |
| --- | --- | --- |
| OH-EVARCH | `docs.openhands.dev/sdk/arch/events.md` | 事件系统架构：append-only、事件类型分类图、source vs role、错误事件两类 |
| OH-EVAPI | `docs.openhands.dev/sdk/api-reference/openhands.sdk.event.md` | 事件类逐个字段（API 参考） |
| OH-CONVAPI | `docs.openhands.dev/sdk/api-reference/openhands.sdk.conversation.md` | `EventLog` / `EventsListBase` / `ConversationState` / `ConversationExecutionStatus` / `fork` 契约 |
| OH-FORKDOC | `docs.openhands.dev/sdk/guides/convo-fork.md` | `Conversation.fork()` 参数表、复制内容表、agent-server REST 端点 |
| OH-PERSIST | `docs.openhands.dev/sdk/guides/convo-persistence.md` | 落盘目录结构、event 文件命名、base_state 覆盖语义 |
| OH-BASE | 仓库源码 `openhands-sdk/openhands/sdk/event/base.py` | **`Event` 基类逐字段**（id / timestamp / source / parent_id）、`frozen=True`、`kind` |
| OH-TYPES | 仓库源码 `openhands-sdk/openhands/sdk/event/types.py` | `EventType` / `SourceType` / `EventID` / `ROOT_PARENT_ID` 字面量 |
| OH-INIT | 仓库源码 `openhands-sdk/openhands/sdk/event/__init__.py` | 事件类的**完整 `__all__` 清单** |
| OH-ACTION | 仓库源码 `openhands-sdk/openhands/sdk/event/llm_convertible/action.py` | `ActionEvent` 逐字段（`llm_response_id`、`tool_call_id`、thinking 等） |
| OH-DELTA | 仓库源码 `openhands-sdk/openhands/sdk/event/streaming_delta.py` | `StreamingDeltaEvent`：**增量不落盘**的明文声明 |
| OH-HOOK | 仓库源码 `openhands-sdk/openhands/sdk/event/hook_execution.py` | `HookExecutionEvent` 与 `HookEventType` 枚举 |
| OH-EVENTSTORE | 仓库源码 `openhands-sdk/openhands/sdk/conversation/event_store.py` | `EventLog.append` 的追加/去重/父事件校验、`path_to_root`、legacy 线性回退 |
| OH-LOCALCONV | 仓库源码 `openhands-sdk/openhands/sdk/conversation/impl/local_conversation.py` | **`fork(from_event_id=…)` 与 `navigate_to(event_id)` 的确切实现** |
| OH-MODELS | 仓库源码 `openhands-sdk/openhands/sdk/utils/models.py` | `DiscriminatedUnionMixin`：判别字段名 `kind` = 类名 |

### Cline（SDK / CLI）

| ID | 来源 | 用途 |
| --- | --- | --- |
| CL-EVENTS | `docs.cline.bot/sdk/events` | 两个事件面（`AgentRuntimeEvent` / `AgentEvent`）、事件类别表、流式与用法模式 |
| CL-EVREF | `docs.cline.bot/sdk/reference/events` | `AgentEvent` 事件清单、`AgentDoneEvent`、`AgentUsageEvent` |
| CL-TYPES | `docs.cline.bot/sdk/reference/types` | `AgentRunResult` / `AgentResult`（含 `finishReason`） |
| CL-AGENTREF | `docs.cline.bot/sdk/reference/agent` | `Agent` / `AgentRuntime` 方法（`run`/`continue`/`abort`/`restore`/`snapshot`） |
| CL-PROD | `docs.cline.bot/sdk/guides/going-to-production` | mistake limit、loop detection、`finishReason: "mistake_limit"` |
| CL-CKPT | `docs.cline.bot/core-workflows/checkpoints` | 检查点三种恢复动作（含"删除此点之后的消息"） |
| CL-README | 仓库 `cline/cline` `sdk/README.md` | SDK 分层（`@cline/agents` / `@cline/core` / `@cline/shared`）、会话落 SQLite |
| CL-ATYPES | npm `@cline/shared@0.0.82` `dist/agents/types.d.ts` | **完整 `AgentEvent` 联合、`AgentFinishReason`、`AgentEventMetadata`、mistake/loop 配置** |
| CL-ARTYPES | npm `@cline/shared@0.0.82` `dist/agent.d.ts` | **完整 `AgentRuntimeEvent` 联合、`AgentRuntimeStateSnapshot`、`AgentMessage` / `AgentMessagePart`、`AgentModelFinishReason`** |
| CL-RECORDS | npm `@cline/shared@0.0.82` `dist/session/records.d.ts` | `SessionLineage` / `SessionRuntimeRecordShape` / `SharedSessionStatus` |

### Claude Code（CLI + Agent SDK）

| ID | 来源 | 用途 |
| --- | --- | --- |
| CC-SESSIONS | `code.claude.com/docs/en/sessions.md` | transcript 位置与格式声明、`--continue`/`--resume`/`--fork-session`/`/branch`、恢复内容 |
| CC-CLI | `code.claude.com/docs/en/cli-reference.md` | `--fork-session` 逐字定义、`--resume` / `--session-id` |
| CC-CKPT | `code.claude.com/docs/en/checkpointing.md` | `/rewind`（恢复对话/代码/摘要）、"原始消息仍留在 transcript" |
| CC-HEADLESS | `code.claude.com/docs/en/headless.md` | stream-json 事件字段（`system/api_retry`、`system/init`）、`parent_tool_use_id`、子 agent 转发 |
| CC-SDKSESS | `code.claude.com/docs/en/agent-sdk/sessions.md` | continue / resume / fork 语义、`forkSession`、transcript 即 JSONL |
| CC-TSREF | `code.claude.com/docs/en/agent-sdk/typescript.md` | `SessionMessage` 字段、`SDKSessionInfo` 字段、`Options` 中 `forkSession`/`resumeSessionAt`/`resumeDropsTurn` |
| CC-PYREF | `code.claude.com/docs/en/agent-sdk/python.md` | `SessionMessage` / `SDKSessionInfo` Python 侧字段（与 TS 对照） |
| CC-STREAM | `code.claude.com/docs/en/agent-sdk/streaming-output.md` | `StreamEvent` / `SDKPartialAssistantMessage` 逐字段、消息顺序、delta 语义 |

---

## 1. OpenHands 的 event stream

### 1.1 事件类枚举（payload 枚举）

OH-EVARCH 把事件分为 **LLM-Convertible**（会进模型上下文）与 **Internal**（不进模型）两类；OH-INIT 给出官方源码的**完整导出清单**（逐字）：

```
"ACPToolCallEvent", "Event", "LLMConvertibleEvent", "SystemPromptEvent",
"ActionEvent", "TokenEvent", "ObservationEvent", "ObservationBaseEvent",
"MessageEvent", "AgentErrorEvent", "UserRejectObservation", "RejectionSource",
"InterruptEvent", "PauseEvent", "StreamingDeltaEvent", "Condensation",
"CondensationRequest", "CondensationSummaryEvent", "ConversationStateUpdateEvent",
"HookExecutionEvent", "LLMCompletionLogEvent", "EventID", "ToolCallID",
"RESUME_CONTEXT_MARKER", "render_resume_transcript"
```

| 事件类（`kind` 值） | 基类 / 归类 | 关键字段（逐条引自一手来源） | 来源 |
| --- | --- | --- | --- |
| `MessageEvent` | LLMConvertibleEvent，`source` 可为 user/agent | `llm_message: Message`、`sender: str \| None`、`activated_skills: list[str]`、`extended_content: list[TextContent]`、`reasoning_content`、`thinking_blocks`、`llm_response_id: str \| None`、`critic_result` | OH-EVAPI, OH-EVARCH |
| `ActionEvent` | LLMConvertibleEvent，`source="agent"` | `thought: Sequence[TextContent]`、`reasoning_content`、`thinking_blocks`、`responses_reasoning_item`、`action: Action \| None`、`tool_name`、`tool_call_id`、`tool_call: MessageToolCall`、`llm_response_id: EventID`、`security_risk`、`critic_result`、`summary` | OH-ACTION |
| `ObservationBaseEvent` | LLMConvertibleEvent，工具响应基类 | `tool_call_id: str`、`tool_name: str` | OH-EVAPI |
| `ObservationEvent` | 继承 `ObservationBaseEvent` | `action_id: str`、`observation: Observation` | OH-EVAPI |
| `UserRejectObservation` | 继承 `ObservationBaseEvent` | `action_id`、`rejection_reason`、`rejection_source: Literal['user','hook']` | OH-EVAPI |
| `AgentErrorEvent` | 继承 `ObservationBaseEvent` | `error: str`；"Error triggered by the agent"，**不含 model thought/reasoning_content** | OH-EVAPI, OH-EVARCH |
| `ConversationErrorEvent` | 直接继承 `Event`（非 LLM-convertible） | 会话级运行失败；`source="environment"`；`code`、`detail`（OH-EVARCH 文字 + 源码用法） | OH-EVARCH, OH-LOCALCONV |
| `SystemPromptEvent` | LLMConvertibleEvent | `system_prompt: TextContent`、`dynamic_context: TextContent \| None`、`tools: list[ToolDefinition]` | OH-EVAPI, OH-EVARCH |
| `CondensationRequest` | Internal | `action` = `ActionType.CONDENSATION_REQUEST` | OH-EVAPI |
| `Condensation` | Internal | `forgotten_event_ids: list[EventID]`、`summary: str \| None`、`summary_offset: int \| None`、`llm_response_id`、`apply()` | OH-EVAPI |
| `CondensationSummaryEvent` | LLMConvertibleEvent | `summary: str` | OH-EVAPI |
| `ConversationStateUpdateEvent` | Internal | `key: str`、`value: Any`（websocket 状态同步） | OH-EVAPI |
| `PauseEvent` | Internal，`source` user | 标记用户暂停 | OH-EVAPI, OH-EVARCH |
| `InterruptEvent` | Internal | 异步中断时发出（OH-LOCALCONV `arun` docstring："On `CancelledError` the conversation transitions to `PAUSED` and emits an `InterruptEvent`"） | OH-LOCALCONV |
| `HookExecutionEvent` | Internal，`source="hook"` | `hook_event_type: HookEventType`、`hook_command`、`success`、`blocked`、`exit_code`、`stdout`、`stderr`、`reason`、`action_id`、`message_id` | OH-HOOK |
| `LLMCompletionLogEvent` | Internal | `filename`、`log_data`、`model_name`、`usage_id` | OH-EVAPI |
| `TokenEvent` | Internal | `prompt_token_ids: list[int]`、`response_token_ids: list[int]`（VLLM） | OH-EVAPI |
| `StreamingDeltaEvent` | Internal，**不落盘** | `content: str \| None`、`reasoning_content: str \| None` | OH-DELTA |
| `ACPToolCallEvent` | 导出清单中，OH-EVAPI 正文未列字段 | — | OH-INIT |

- OH-TYPES 定义了**另一个**、更粗的字面量（逐字）：`EventType = Literal["action", "observation", "message", "system_prompt", "agent_error"]` 与 `SourceType = Literal["agent", "user", "environment", "hook"]`。⚠️ 注意：该 `EventType` 只有 5 个值，**不覆盖**上面全部事件类；实际序列化判别字段见 1.2。
- **没有** `Action` / `Observation` 作为"事件子类"的层级：只有单个 `ActionEvent`（其 `action` 字段是**工具 schema 类型** `openhands.sdk.tool.schema.Action`）与 `ObservationEvent`（`observation: Observation`）。"Action/Observation 各自的子类"在本 SDK 的**事件层**不是一个存在的枚举；工具侧的 `Action`/`Observation` 子类未在本票范围内枚举（OH-ACTION 只显示基类导入）。

### 1.2 信封 / 公共字段（`Event` 基类，逐字）

OH-BASE 定义 `class Event(DiscriminatedUnionMixin, ABC)`，`model_config: ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)`，字段：

| 字段 | 类型 / 默认 | 官方 description（逐字） | 来源 |
| --- | --- | --- | --- |
| `id` | `EventID`，`default_factory=lambda: str(uuid.uuid4())` | "Unique event id (ULID/UUID)" | OH-BASE |
| `timestamp` | `str`，`default_factory=lambda: datetime.now().isoformat()` | "Event timestamp"（注释 `# consistent with V1`） | OH-BASE |
| `source` | `SourceType`，必填 | "The source of this event" | OH-BASE |
| `parent_id` | `EventID \| None`，默认 `None` | "Parent event id in the conversation tree. None for the root, or for legacy events predating the tree (see EventLog's effective-parent rule). Events sharing a parent_id are sibling branches." | OH-BASE |
| `kind` | computed field（`DiscriminatedUnionMixin`） | 返回 `self.__class__.__name__`；JSON schema 里是 `discriminator.propertyName = "kind"` | OH-MODELS |

- `EventID = str`；`ToolCallID = str`（OH-TYPES）。
- 保留哨兵（逐字）：`ROOT_PARENT_ID: Final = "__root__"`，用于"tree root created after `navigate_to(None)`"；`Event` 上有 validator 禁止任何 `id == ROOT_PARENT_ID`（OH-TYPES, OH-BASE）。
- **因果 id**：OpenHands 用多个字段表达，而非单一 `causation_id`——
  - `ActionEvent.llm_response_id: EventID`，官方 description："Completion or Response ID of the LLM response that generated this event… Can be used to group related actions from same LLM response."（OH-ACTION）
  - `ObservationEvent.action_id: str`（指向被观察的 action，OH-EVAPI）
  - `tool_call_id`（LLM 返回的工具调用 id，OH-ACTION）
- **来源 agent**：`source` 只有 `agent` / `user` / `environment` / `hook` 四值，**不是** agent 身份 id。OH-EVARCH 明确 `Event.source`（归属）与 LLM `role`（格式）**intentionally independent**，并警告 "Do not infer event origin from LLM role."。`MessageEvent.sender: str | None` 可携带发送者标识，官方说 "Can be used to track message origin in multi-agent scenarios"（OH-CONVAPI `send_message`）。
- 多 agent 子会话：OH-LOCALCONV 有 `prompt_cache_key` 参数说明 "Sub-conversations set this to the parent's ID to share the same cache shard."——即**子会话**通过父 conversation id 关联，而非事件字段。

### 1.3 是否只追加

| 事实 | 原文 / 依据 | 来源 |
| --- | --- | --- |
| 是 append-only 日志 | "Events form an append-only log that serves as both the agent's memory and the integration point for auxiliary services."；职责含 "**Append-Only Log** - Maintain immutable event history" | OH-EVARCH |
| 事件不可变 | `ConfigDict(extra="forbid", frozen=True)`；`events_to_messages` 注释 "events are immutable once created and appended" | OH-BASE |
| 追加有并发锁 + 幂等校验 | `EventLog.append()` "Append an event with locking for thread/process safety"；同 id 重复 → `ValueError`；显式 `parent_id` 不存在 → `ValueError`；返回日志分配的序号 `idx` | OH-EVENTSTORE |
| 每条事件一个文件、按序号命名 | `events/event-00000-<event-id>.json`；"Events are appended incrementally (one file per event), while base state is overwritten on each change." | OH-PERSIST |
| 旧事件无 `parent_id` 时的读法 | `_effective_parent_id`："Legacy events predating the tree have no `parent_id`; they fall back to the linear chain (event `idx - 1`)"；`idx == 0` 才是"genuine root" | OH-EVENTSTORE |
| 压缩不是删日志 | `Condensation.apply()` "removes events that are marked to be forgotten and returns a new list of events"（构造**视图**）；`forgotten_event_ids` 只是被遗忘 | OH-EVAPI |

### 1.4 `fork(from_event_id=…)` 的确切语义

⚠️ **两份一手来源不一致，必须以源码为准**：OH-FORKDOC 的 "API Reference" 代码块列出的 `fork()` 签名**没有** `from_event_id`（只有 `conversation_id` / `agent` / `title` / `tags` / `reset_metrics`），但当前 `main` 源码有。逐字源码（OH-LOCALCONV）：

```python
def fork(
    self,
    *,
    conversation_id: ConversationID | None = None,
    agent: AgentBase | None = None,
    title: str | None = None,
    tags: dict[str, str] | None = None,
    reset_metrics: bool = True,
    from_event_id: EventID | None = None,
) -> "LocalConversation":
```

| 问题 | 事实 | 来源 |
| --- | --- | --- |
| 从哪里复制 | `from_event_id: If set, copy only the branch up to this event (``path_to_root``) and set the fork's HEAD there. If ``None`` (default), copy the whole log and keep the source's HEAD.` | OH-LOCALCONV |
| 实现 | `source_events = self._state.events.path_to_root(from_event_id)`；逐条 `fork_conv._state.events.append(_copy_event_for_fork(event))`；`_copy_event_for_fork` 走 `Event.model_validate_json(event.model_dump_json(exclude_none=True))` | OH-LOCALCONV |
| `path_to_root` 语义 | "The active branch `leaf -> ... -> root`, returned **root-first**"；`leaf_id=None` 得 `[]`；遇环 raise；缺失 raise | OH-EVENTSTORE |
| 新会话如何标记来源 | **没有**"来源会话 id"字段：OH-LOCALCONV fork docstring 只说 "A new `LocalConversation` that shares the same event history but has its own identity"；新 id 为 `conversation_id or uuid.uuid4()`；`fork_conv._state.leaf_event_id = fork_leaf`；分支切片时 `head_is_empty = False`，整库复制时继承源的 `head_is_empty`。OH-FORKDOC 明确 "**Tags**: Fresh from kwargs; source tags are **not** inherited"、`Execution status: Always idle`、`Conversation ID: New UUID`。 | OH-LOCALCONV, OH-FORKDOC |
| 校验 | `if from_event_id is not None and from_event_id not in self._state.events: raise ValueError(f"Unknown from_event_id: {from_event_id}")` | OH-LOCALCONV |
| 复制内容 | 事件深拷贝（源不可变）、agent 深拷贝（或显式替换）、workspace 共享、agent_state 深拷贝、activated skills / path rules 复制、stats 默认重置（`reset_metrics`）、tags 来自 kwargs | OH-FORKDOC, OH-LOCALCONV |
| 远端 / REST | `POST /api/conversations/{id}/fork`，请求体 `{ "id", "title", "tags", "reset_metrics" }`；`RemoteConversation.fork(agent=…)` 明确不支持 | OH-FORKDOC |

---

## 2. Cline SDK events

### 2.1 两个事件面与事件类型清单

CL-EVENTS：`AgentRuntimeEvent`（来自 `@cline/agents`，`agent.subscribe(listener)`，low-level）与 `AgentEvent` / `CoreSessionEvent`（来自 `@cline/core`，host-facing）；"For event shapes, see Events reference"。

**`AgentEvent`（host-facing）** — CL-EVREF 文档清单 + CL-ATYPES 类型联合（逐字）。联合共 9 个成员：

```ts
export type AgentEvent = AgentContentStartEvent | AgentContentUpdateEvent | AgentContentEndEvent
  | AgentIterationStartEvent | AgentIterationEndEvent | AgentNoticeEvent
  | AgentUsageEvent | AgentDoneEvent | AgentErrorEvent;
```

| `type` | 关键字段（逐字） | 来源 |
| --- | --- | --- |
| `content_start` | `contentType: "text" \| "reasoning" \| "media" \| "tool"`、`text?`、`accumulated?`、`reasoning?`、`redacted?`、`toolName?`、`toolCallId?`、`input?`、`execution?: "client" \| "provider"` | CL-ATYPES |
| `content_update` | `contentType: "tool"`、`toolName?`、`toolCallId?`、`update: unknown` | CL-ATYPES |
| `content_end` | `contentType`、`text?`、`reasoning?`、`media?`、`toolName?`、`toolCallId?`、`output?`、`error?`、`durationMs?` | CL-ATYPES |
| `iteration_start` | `iteration: number`（1-based） | CL-ATYPES |
| `iteration_end` | `iteration`、`hadToolCalls: boolean`、`toolCallCount: number` | CL-ATYPES |
| `notice` | `noticeType: "recovery" \| "stop" \| "status"`、`message`、`displayRole?: "system" \| "status"`、`reason?: "api_error" \| "invalid_tool_call" \| "completion_without_submit" \| "tool_execution_failed" \| "mistake_limit" \| "auto_compaction" \| "manual_compaction" \| "compaction_budget_emergency"`、`metadata?` | CL-ATYPES |
| `usage` | `inputTokens`、`outputTokens`、`cacheReadTokens?`、`cacheWriteTokens?`、`cost?`、`totalInputTokens`、`totalOutputTokens`、`totalCacheReadTokens?`、`totalCacheWriteTokens?`、`totalCost?` | CL-EVREF, CL-ATYPES |
| `done` | `reason: AgentFinishReason`、`text: string`、`iterations: number`、`usage?: LegacyAgentUsage` | CL-EVREF, CL-ATYPES |
| `error` | `error: Error`、`errorClass?: ProviderErrorClass`、`recoverable: boolean`、`iteration: number` | CL-ATYPES |

文档侧给的分类表（CL-EVENTS）：Content `content_start`/`content_update`/`content_end`；Iterations `iteration_start`/`iteration_end`；Usage `usage`；Notices `notice`；Completion `done`/`error`。

**`AgentRuntimeEvent`（low-level，完整联合 15 个成员，逐字 `type`）** — CL-ARTYPES：

`"run-started"`、`"message-added"`、`"turn-started"`、`"assistant-text-delta"`、`"assistant-reasoning-delta"`、`"assistant-media"`、`"assistant-message"`、`"tool-started"`、`"tool-updated"`、`"tool-finished"`、`"usage-updated"`、`"turn-finished"`、`"status-notice"`、`"run-finished"`、`"run-failed"`。

- 每个成员都带 `snapshot: AgentRuntimeStateSnapshot`；delta 成员另带 `iteration`、`text`、`accumulatedText`（reasoning 版另带 `redacted?`、`metadata?`）。
- `AgentRuntimeStateSnapshot`（逐字）：`agentId`、`agentRole?`、`parentAgentId?: string | null`、`conversationId?`、`runId?`、`status: "idle"|"running"|"completed"|"aborted"|"failed"`、`iteration`、`messages`、`pendingToolCalls`、`usage`、`lastError?`、`lastErrorClass?`。
- CL-EVENTS 另举的运行时用法：`assistant-text-delta`（取 `event.text`）、`tool-started`（`event.toolCall.toolName`）、`tool-finished`、`run-finished`（`event.result.status`）、`usage-updated`。

### 2.2 停止条件枚举与触发条件

**枚举本体**（CL-ATYPES，逐字）：`export type AgentFinishReason = "completed" | "max_iterations" | "aborted" | "mistake_limit" | "error";`，同时有 `AgentFinishReasonSchema`（Zod enum，值相同）。`@cline/agents` 的 `AgentRunResult.status` 则是 `"completed" | "aborted" | "failed"`（CL-AGENTREF, CL-ARTYPES）。

**这些 reason 挂在哪个事件上**：

| 承载位置 | 字段 | 来源 |
| --- | --- | --- |
| host-facing `done` 事件 | `AgentDoneEvent.reason: AgentFinishReason`（CL-EVREF 逐字：`reason: "completed" \| "max_iterations" \| "aborted" \| "mistake_limit" \| "error"`） | CL-EVREF, CL-ATYPES |
| host-facing 结果 | `AgentResult.finishReason: AgentFinishReason`（CL-TYPES 逐字：`finishReason: "completed" \| "max_iterations" \| "aborted" \| "mistake_limit" \| "error"`） | CL-TYPES, CL-ATYPES |
| low-level 运行时 | `run-finished` 携带 `result: AgentRunResult`（`status`，**不是** `finishReason`）；`run-failed` 携带 `error`、`errorClass?` | CL-ARTYPES, CL-EVREF |

**触发条件（一手来源明确写到的部分）**：

| reason | 触发条件（逐字/转述） | 来源 |
| --- | --- | --- |
| `completed` | 正常结束。CL-EVREF 对 `done` 的描述只有 "Agent completed/aborted/failed"，**未逐 reason 给出判据** | CL-EVREF |
| `max_iterations` | `AgentConfig.maxIterations`："Maximum number of loop iterations / If undefined, no iteration cap is enforced."；`maxTurns`/`maxIterations` 在 Options/Types 中同名 | CL-ATYPES, CL-AGENTREF |
| `aborted` | `Agent.abort(reason?)`："Aborts the active run."；`AgentRunStatus` 含 `"aborted"`。文档**未**逐字写 "abort() ⇒ reason=\`aborted\`" | CL-AGENTREF, CL-ARTYPES |
| `mistake_limit` | CL-PROD 逐字："Core sessions can track consecutive recoverable mistakes and stop with `finishReason: "mistake_limit"`."；CL-ATYPES：`maxConsecutiveMistakes`（"Maximum consecutive internal mistakes before escalation. Mistakes include API turn failures, invalid/missing tool-call arguments, and iterations where every executed tool call fails."，**`@default 6`**）；`onConsecutiveMistakeLimitReached(context)` 返回 `{action:"stop", reason?}` 时可停止；`ConsecutiveMistakeLimitContext.reason` = `"api_error" \| "invalid_tool_call" \| "tool_execution_failed"`（**注意：这是 mistake 的 reason，不是 finish reason**） | CL-PROD, CL-ATYPES |
| `error` | `run-failed` 携带 `error: Error`；`AgentRunResult.error?: Error`。文档**未**逐字写其到 `finishReason:"error"` 的映射 | CL-ARTYPES, CL-AGENTREF |

**相关的循环/重复检测（一手事实）**：CL-ATYPES `AgentExecutionConfig.loopDetection`——"At `softThreshold`: injects a recovery notice urging a different approach. At `hardThreshold`: triggers the consecutive-mistake-limit decision path."；"The CLI enables this by default with `{ softThreshold: 3, hardThreshold: 5 }`." `reminderAfterIterations` 默认 `0`（关闭）。

### 2.3 信封 / 公共字段

| 项 | 事实 | 来源 |
| --- | --- | --- |
| host-facing `AgentEvent` 公共字段 | `AgentEventMetadata { agentId?: string; conversationId?: string; parentAgentId?: string \| null }`；**没有** event id、timestamp、sessionId | CL-ATYPES |
| low-level 运行时事件公共字段 | 只带 `snapshot: AgentRuntimeStateSnapshot`（内含 ids/status/iteration/messages/usage），**没有** event id/timestamp | CL-ARTYPES |
| transcript 记录字段 | `AgentMessage { id: string; role: "user"\|"assistant"\|"tool"; content: AgentMessagePart[]; createdAt: number; metadata?; modelInfo?; metrics? }` | CL-ARTYPES |
| 内容 part 枚举（逐字） | `AgentMessagePart = AgentTextPart \| AgentReasoningPart \| AgentImagePart \| AgentFilePart \| AgentMediaPart \| AgentToolCallPart \| AgentToolResultPart`；`AgentToolCallPart` 含 `toolCallId`、`toolName`、`input`、`execution?: "client" \| "provider"`；`AgentToolResultPart` 含 `toolCallId`、`toolName`、`output`、`isError?` | CL-ARTYPES |
| token 用量 | `AgentUsage extends AgentTokenUsage { totalCost? }`；`AgentTokenUsage { inputTokens, outputTokens, cacheReadTokens, cacheWriteTokens, reasoningTokenCount? }`；`AgentMessage.metrics?` 同形 | CL-ARTYPES |
| 会话血缘 | `SessionLineage { parentSessionId?, agentId?, parentAgentId?, conversationId?, isSubagent: boolean }`；`SessionRuntimeRecordShape` 另含 `status: SharedSessionStatus`（`["idle","running","pending","completed","failed","cancelled"]`）、`messagesPath?` 等 | CL-RECORDS |
| 持久化介质 | CL-README："persists sessions to SQLite"；`ClineCore` 默认 `~/.cline/data/workspaces/chat` | CL-README |

---

## 3. Claude Code 的会话 JSONL

### 3.1 存储位置与记录形状

| 项 | 事实（逐字优先） | 来源 |
| --- | --- | --- |
| 位置 | `~/.claude/projects/<project>/<session-id>.jsonl`；`<project>` = 工作目录路径把非字母数字替换为 `-`；超 200 字符则截断 + 路径 hash | CC-SESSIONS |
| 每行是什么 | "Each line is a JSON object for a message, tool use, or metadata entry. **The entry format is internal to Claude Code and changes between versions**, so scripts that parse these files directly can break on any release." | CC-SESSIONS |
| 官方建议 | "To build on session data, use `/export` or the [script interfaces] instead." | CC-SESSIONS |
| 是否可解析为字段 | 官方**逐字段**的 transcript 记录 schema **不存在**；能拿到的最接近的一手形状是 `getSessionMessages()` 返回的 `SessionMessage` | CC-SESSIONS, CC-TSREF |

**`SessionMessage`（官方 API 的逐字段形状，TS/Python 一致）** — CC-TSREF, CC-PYREF：

| 字段 | 类型 | 官方描述（逐字） |
| --- | --- | --- |
| `type` | `"user" \| "assistant"` | "Message role" |
| `uuid` | `string` | "Unique message identifier" |
| `session_id` | `string` | "Session this message belongs to" / "Session identifier" |
| `message` | `unknown` / `Any` | "Raw message payload from the transcript" / "Raw message content" |
| `parent_tool_use_id` | `string \| null` | "For subagent messages, the `tool_use_id` of the spawning `Agent` tool call. `null` for main-session messages and older sessions" |
| `parent_agent_id` | `string \| null` | "For messages from a nested subagent, the `agentId` of the subagent that spawned it. `null` for main-session messages, messages from top-level subagents, and older sessions." Requires v2.1.202+ |

**会话级元数据 `SDKSessionInfo`（TS）** — CC-TSREF：`sessionId`、`summary`（"custom title, auto-generated summary, or first prompt"）、`lastModified`、`fileSize`（"Only populated for local JSONL storage"）、`customTitle`、`firstPrompt`、`gitBranch`、`cwd`、`tag`、`createdAt`（"Creation time in milliseconds since epoch, **from the first entry's timestamp**"）。Python 侧同名字段 snake_case（CC-PYREF）。

**stream-json 消息（与 transcript 未必同形，但为一手字段定义）**：

| 消息 | 字段（逐字） | 来源 |
| --- | --- | --- |
| `system` / `subtype: "api_retry"` | `type:"system"`、`subtype:"api_retry"`、`attempt`、`max_retries`、`retry_delay_ms`、`error_status`、`no_response?`、`error`（`authentication_failed`/`oauth_org_not_allowed`/`account_on_hold`/`billing_error`/`rate_limit`/`overloaded`/`invalid_request`/`model_not_found`/`server_error`/`max_output_tokens`/`cloud_credential_error`/`unknown`）、`uuid`、`session_id` | CC-HEADLESS |
| `system` / `subtype: "init"` | `plugins[]`（`name`,`path`）、`plugin_errors[]`、`mcp_servers[]`（`name`,`status`）、`mcp_server_errors[]`、`capabilities?` | CC-HEADLESS |
| `system` / `subtype: "plugin_install"` | `status: "started"\|"installed"\|"failed"\|"completed"`、`name?`、`error?`、`uuid`、`session_id` | CC-HEADLESS |
| `stream_event`（`SDKPartialAssistantMessage`） | `type:"stream_event"`、`event: BetaRawMessageStreamEvent`、`parent_tool_use_id: string\|null`、`uuid: UUID`、`session_id: string`、`ttft_ms?`、`user_message_uuid?` | CC-STREAM |
| Python `StreamEvent` | `uuid`、`session_id`、`event: dict`、`parent_tool_use_id`（"Always None"） | CC-STREAM |

### 3.2 哪些东西进日志

| 内容 | 事实 | 来源 |
| --- | --- | --- |
| 消息 / 工具调用 / 工具结果 | transcript 每行是 "a message, tool use, or metadata entry"；恢复会话时 "Conversation history: the full history, including tool calls and results." | CC-SESSIONS |
| 子 agent 的消息 | `parent_tool_use_id` 标识 spawning 的 Agent tool call；嵌套子 agent 用 `parent_agent_id`；`--forward-subagent-text` / `CLAUDE_CODE_FORWARD_SUBAGENT_TEXT` 才会转发子 agent 的 **text 与 thinking** 块（默认只转发 `tool_use`/`tool_result`） | CC-HEADLESS, CC-TSREF |
| thinking | 默认子 agent 的 thinking **不进** stream；开启 `forwardSubagentText` 后进入。主会话 thinking 是否进 transcript：**一手文档未就 transcript 明说** → ⚪ | CC-TSREF, CC-HEADLESS |
| token 用量 | 一手来源只在 **result / 使用统计** 面给出：`--output-format json` 的响应含 `total_cost_usd` 与 per-model cost；`/usage`、statusline 显示上下文与成本。**没有任何一手文档说用量字段写进 JSONL 记录** → ⚪ | CC-HEADLESS, CC-SESSIONS |
| 元数据条目 | transcript 明确含 "metadata entry"；`createdAt` 描述为 "from the first entry's timestamp"，说明存在带 timestamp 的条目；`renameSession` 是 "appending a custom-title entry"，`tagSession` 同理 → **追加式元数据条目** | CC-SESSIONS, CC-TSREF |

### 3.3 `--continue` / `--resume` / `--fork-session` / `/branch`

| 机制 | 事实（逐字优先） | 来源 |
| --- | --- | --- |
| `--continue` | "Reopens the most recent conversation in the current directory"；与 `--resume` 一样"pick up an existing session and **add to it**" | CC-SESSIONS, CC-SDKSESS |
| `--resume <id|name|path>` | 恢复指定 session；也可传 `.jsonl` transcript 绝对路径继续其中对话 | CC-SESSIONS, CC-CLI |
| 同一 session 追加 | "If you resume the same session in two terminals without forking, **messages from both interleave into one transcript**." → 追加到同一 session id / 同一文件 | CC-SESSIONS |
| `--fork-session` | 逐字："When resuming, **create a new session ID instead of reusing the original** (use with `--resume` or `--continue`)" | CC-CLI |
| `/branch` | "Branching creates a **copy of the conversation so far** and switches you into it, leaving the original intact."；"`/branch` copies the transcript and switches the running Claude Code process to write to it."；确认时打印两个 session id（新分支 + 原会话）；"The original is unchanged on disk and remains in the session picker" | CC-SESSIONS |
| 两者都产生新 id | "Sessions created with `/branch` or `--fork-session` get their own session IDs and appear as separate rows." | CC-SESSIONS |
| SDK 参数 | `forkSession`（Options）："When resuming with `resume`, **fork to a new session ID** instead of continuing the original session"（默认 `false`）；Python `ClaudeAgentOptions(resume=session_id, fork_session=True)` 示例中 `forked_id` 与 `session_id` 不同，原 session 未变 | CC-TSREF, CC-SDKSESS |
| 分支继承什么 | Conversation history "Copied into the branch up to the point you ran `/branch`"；"Allow for this session" 授权在同进程内保留，`--fork-session` 新进程不继承；进行中的 background subagents / Bash 继续跑，输出进新分支 | CC-SESSIONS |
| 跨目录查找 | `--resume <session-id>` 先在当前项目目录及其 git worktree 找，再全机查找；仅当恰有一个其他项目持有含消息的 transcript 才解析 | CC-SESSIONS |

---

## 4. 增量文本进不进日志

| 家 | 事实 | 结论 | 来源 |
| --- | --- | --- | --- |
| OpenHands | `StreamingDeltaEvent` docstring 逐字："Transient LLM token delta for real-time WebSocket delivery. **Not persisted to the conversation event log**: these events are published directly to PubSub, bypassing the callback chain that writes to `ConversationState.events`. Clients reconnecting mid-stream will receive the final `MessageEvent` from history but none of the deltas that produced it — deltas are a UX affordance, not part of the durable conversation record." | ✅ **明确不进** | OH-DELTA |
| OpenHands（另一路） | 另有 `token_callbacks`（"invoked for streaming deltas"）与 `stream_callbacks`（"stream-progress frames minted by `StreamContext`"），与 `callbacks`（事件）分开 | ✅ 增量走独立回调面 | OH-CONVAPI, OH-LOCALCONV |
| Cline | low-level 运行时确实有逐 token 事件 `assistant-text-delta`（`text` + `accumulatedText`）与 `assistant-reasoning-delta`；`content_start` 也带 `text?`/`accumulated?`。但一手文档**未说明**这些 delta 是否写入会话记录/SQLite；`prepareTurn` 的文档只说明它"are **not persisted** as session history"（指请求投影，不是 delta） | ⚪ **未证实** | CL-ARTYPES |
| Claude Code | `include_partial_messages` / `includePartialMessages` 为 `true` 时额外产出 `StreamEvent` / `stream_event`（"raw Claude API streaming events … not accumulated text"）；官方**未说明**这些流事件是否写入 JSONL；transcript 格式本身"internal … changes between versions" | ⚪ **未证实** | CC-STREAM, CC-SESSIONS |

---

## 5. "重新生成 / 撤销"的机制

| 家 | 机制 | 事实 | 来源 |
| --- | --- | --- | --- |
| OpenHands | **移动 HEAD（不新建会话、不删日志）** | `navigate_to(event_id)`："Move the conversation HEAD within this conversation (no new fork). Re-roots the active branch: the agent's next context becomes `path_to_root(event_id)`. All branches stay on disk — appending after navigating creates a sibling; **abandoned events stay in the log but drop out of `state.view`**."；`event_id=None` 表示空树 | OH-LOCALCONV |
| OpenHands | **分支复制** | `fork(from_event_id=…)` 只复制 `path_to_root` 并把 HEAD 设在该点（见 1.4） | OH-LOCALCONV |
| OpenHands | **压缩（视图级遗忘）** | `Condensation.apply()` 移除 `forgotten_event_ids` 并（若有 summary 元数据）插入 `CondensationSummaryEvent`；是构造新列表，不删除日志文件 | OH-EVAPI |
| Cline | **检查点三选一** | "Restore Files"（回滚文件、保留对话）；"Restore Task Only"："**Deletes messages after this point**, does not affect files"；"Restore Files & Task"（两者都回退）。文件快照放在独立的 shadow Git 仓库，每次工具使用后提交 | CL-CKPT |
| Cline | **消息编辑** | "When you edit a previous message and select 'Restore All,' Cline restores your files to the checkpoint at that point before resubmitting your edited message." | CL-CKPT |
| Cline | 记录级机制 | "删除此点之后的消息"是文档原话；但**如何**在记录层实现（重写文件 / 移动 HEAD / 新建分支）一手文档未写 → ⚪ | CL-CKPT |
| Claude Code | **`/rewind` 菜单** | 动作：`Restore code and conversation`、`Restore conversation`（"rewind to that message while keeping current code"）、`Restore code`、`Summarize from here`、`Summarize up to here`、`Never mind`；选中并恢复对话后，该 prompt 回到输入框可重发/编辑 | CC-CKPT |
| Claude Code | **摘要不动消息** | "Summarizing doesn't change files on disk, and **the original messages stay in the session transcript**, so Claude can still reference the details."；出现 `Summarized conversation` 标记 | CC-CKPT |
| Claude Code | **截断式 resume** | `resumeSessionAt: "Resume session at a specific message UUID"`；`resumeDropsTurn`（with `resumeSessionAt`）："the prompt UUID of the turn the truncating resume intends to discard. Claude Code refuses the resume when the discarded range contains anything not attributable to that turn…"（v2.1.223+） | CC-TSREF |
| Claude Code | 检查点范围 | "Every prompt you send that starts a turn creates a new checkpoint"；保留最近 **100** 个检查点的文件快照；`/clear` 后可用 rewind 菜单的 `previous session` 条目回到清空前会话 | CC-CKPT |
| 三家共同点（仅就来源所述） | 三家都**没有**一手文档描述"对已落盘记录做原地重写/覆盖"；OpenHands 明说是移动 HEAD + 保留日志，Cline 明说是"删除消息"，Claude Code 只说行为不说记录层实现 | — | OH-LOCALCONV, CL-CKPT, CC-CKPT |

---

## 6. ⚪ 无法从一手来源验证的清单

1. **OpenHands `Action` / `Observation` 各自的事件子类清单** —— 事件层只有单个 `ActionEvent` / `ObservationEvent`（其 `action` / `observation` 字段属于工具 schema）；工具侧 `Action`/`Observation` 的具体子类未在本票一手来源中枚举 → **未证实**。
2. **OpenHands `EventType` 字面量与 21 个事件类的对应关系** —— OH-TYPES 的 `EventType` 只有 5 个值（`action`/`observation`/`message`/`system_prompt`/`agent_error`），与 OH-INIT 的完整类清单不能一一对应；一手来源未解释该字面量的使用位置 → **未证实**。
3. **OpenHands fork 的"来源会话"标记字段** —— 一手源码与文档都只给新 `conversation_id` + `title`/`tags`，**没有** `forked_from` / `source_conversation_id` 之类的字段 → 该字段**不存在**（在本次核查范围内）。
4. **OH-FORKDOC 与源码的签名差异原因**（文档 API Reference 缺 `from_event_id`）—— 无一手说明 → **未证实**。
5. **Cline 各 `AgentFinishReason` 的逐条触发判据** —— 只有 `mistake_limit` 有一手明文（consecutive recoverable mistakes）；`completed` / `max_iterations` / `aborted` / `error` 与具体代码路径的映射文档未逐条写明 → **未证实**。
6. **Cline 的 `aborted` 是否只由 `abort()` 触发** —— `abort()` 与 `AgentRunStatus."aborted"` 均有定义，但无一手文字把二者显式连到 `finishReason` → **未证实**。
7. **Cline `AgentEventMetadata` 之外的公共信封** —— host-facing 事件无 event id / timestamp / sessionId；运行时事件只有 `snapshot`；是否存在更外层的传输信封（hub / RPC）本票未核查 → **未证实**。
8. **Cline 增量 delta 是否落盘** —— 见第 4 节 → **未证实**。
9. **Claude Code JSONL 记录的完整字段 schema 与记录类型枚举** —— 官方明说 "internal … changes between versions"，只给出 `SessionMessage`（uuid / session_id / message / parent_tool_use_id / parent_agent_id）这一层的形状；record `type` 的完整枚举、thinking / token 用量是否落盘 → **未证实**。
10. **Claude Code 流式增量是否写入 JSONL** —— 见第 4 节 → **未证实**。
11. **Claude Code `/rewind` "Restore conversation" 在记录层如何实现**（截断重写 / HEAD 移动 / 追加补偿事件）—— 文档只描述行为，未描述记录层 → **未证实**。
12. **Claude Code `--fork-session` 复制时是否新建文件 / 复制哪些行** —— 文档只说 "create a new session ID"，未说复制机制 → **未证实**。
13. **票面点名的 `how-claude-code-works` 页正文** —— 本次抓取到的该页 HTML 只含导航壳（正文未随抓取返回），其与 `sessions` 页重叠的陈述一律以 CC-SESSIONS / CC-HEADLESS 为准；该页是否另有 transcript 字段声明**未核对**。
14. **Cline / Claude Code 官方仓库中与 npm 发布产物不一致的实现细节** —— Cline 本次以官方文档 + 官方 npm 包 `.d.ts` 为准，未逐文件核对 `cline/cline` 仓库 `sdk/` 源码；Claude Code 为闭源，无源码可核。

---

## 7. 来源一览（全部一手来源，抓取于 2026-09-13 UTC）

### OpenHands（官方文档站 + 官方仓库 `OpenHands/software-agent-sdk`）

| ID | URL |
| --- | --- |
| OH-EVARCH | <https://docs.openhands.dev/sdk/arch/events.md> |
| OH-EVAPI | <https://docs.openhands.dev/sdk/api-reference/openhands.sdk.event.md> |
| OH-CONVAPI | <https://docs.openhands.dev/sdk/api-reference/openhands.sdk.conversation.md> |
| OH-FORKDOC | <https://docs.openhands.dev/sdk/guides/convo-fork.md> |
| OH-PERSIST | <https://docs.openhands.dev/sdk/guides/convo-persistence.md> |
| OH-BASE | <https://raw.githubusercontent.com/OpenHands/software-agent-sdk/main/openhands-sdk/openhands/sdk/event/base.py> |
| OH-TYPES | <https://raw.githubusercontent.com/OpenHands/software-agent-sdk/main/openhands-sdk/openhands/sdk/event/types.py> |
| OH-INIT | <https://raw.githubusercontent.com/OpenHands/software-agent-sdk/main/openhands-sdk/openhands/sdk/event/__init__.py> |
| OH-ACTION | <https://raw.githubusercontent.com/OpenHands/software-agent-sdk/main/openhands-sdk/openhands/sdk/event/llm_convertible/action.py> |
| OH-DELTA | <https://raw.githubusercontent.com/OpenHands/software-agent-sdk/main/openhands-sdk/openhands/sdk/event/streaming_delta.py> |
| OH-HOOK | <https://raw.githubusercontent.com/OpenHands/software-agent-sdk/main/openhands-sdk/openhands/sdk/event/hook_execution.py> |
| OH-EVENTSTORE | <https://raw.githubusercontent.com/OpenHands/software-agent-sdk/main/openhands-sdk/openhands/sdk/conversation/event_store.py> |
| OH-LOCALCONV | <https://raw.githubusercontent.com/OpenHands/software-agent-sdk/main/openhands-sdk/openhands/sdk/conversation/impl/local_conversation.py> |
| OH-MODELS | <https://raw.githubusercontent.com/OpenHands/software-agent-sdk/main/openhands-sdk/openhands/sdk/utils/models.py> |

### Cline（官方文档站 + 官方仓库 `cline/cline` + 官方 npm 发布产物）

| ID | URL |
| --- | --- |
| CL-EVENTS | <https://docs.cline.bot/sdk/events> · <https://docs.cline.bot/sdk/events.md> |
| CL-EVREF | <https://docs.cline.bot/sdk/reference/events.md> |
| CL-TYPES | <https://docs.cline.bot/sdk/reference/types.md> |
| CL-AGENTREF | <https://docs.cline.bot/sdk/reference/agent.md> |
| CL-PROD | <https://docs.cline.bot/sdk/guides/going-to-production.md> |
| CL-CKPT | <https://docs.cline.bot/core-workflows/checkpoints.md> |
| CL-README | <https://raw.githubusercontent.com/cline/cline/main/sdk/README.md> |
| CL-ATYPES | <https://cdn.jsdelivr.net/npm/@cline/shared@0.0.82/dist/agents/types.d.ts> |
| CL-ARTYPES | <https://cdn.jsdelivr.net/npm/@cline/shared@0.0.82/dist/agent.d.ts> |
| CL-RECORDS | <https://cdn.jsdelivr.net/npm/@cline/shared@0.0.82/dist/session/records.d.ts> |

### Claude Code（官方文档站 `code.claude.com`）

| ID | URL |
| --- | --- |
| CC-SESSIONS | <https://code.claude.com/docs/en/sessions.md> |
| CC-CLI | <https://code.claude.com/docs/en/cli-reference.md> |
| CC-CKPT | <https://code.claude.com/docs/en/checkpointing.md> |
| CC-HEADLESS | <https://code.claude.com/docs/en/headless.md> |
| CC-SDKSESS | <https://code.claude.com/docs/en/agent-sdk/sessions.md> |
| CC-TSREF | <https://code.claude.com/docs/en/agent-sdk/typescript.md> |
| CC-PYREF | <https://code.claude.com/docs/en/agent-sdk/python.md> |
| CC-STREAM | <https://code.claude.com/docs/en/agent-sdk/streaming-output.md> |

### 附：本仓库综述里点到的入口（非来源，仅为定位）

本票第 5 步点名的 `docs/research/coding-agent-features.md` 第 15、43、316–318、336、838 行分别指向 OpenHands 仓库、Cline SDK events、Claude Code `how-claude-code-works` / `sessions`、以及 Cline SDK events 的官方 URL；这些 URL 均已在上面逐条落为 ID。
