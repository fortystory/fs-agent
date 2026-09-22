# 多 agent 编排框架的轮次管理与终止条件：面向主来源的调查

- 调查日期：2026-09-12
- 范围：为 `fs-agent` 的「同一 session 内多 agent 分轮讨论 + 派发子 agent 执行」能力做事实底稿。本票对应 issue `22-research-orchestration-frameworks.md`。
- 本文**只报告事实与来源，不给方案、不做推荐**。
- 与已有调研的关系：
  - `research/01-debate-conformity-and-speaker-attribution.md` 覆盖了「共享上下文是否导致从众」与 AutoGen 的**消息归属编码**，并明确标注 CrewAI / LangGraph **本轮未验证**。本文补齐 CrewAI / LangGraph，并补齐 AutoGen 的**轮次管理与终止条件**。
  - `research/02-provider-call-surface.md` 覆盖 Kimi / DeepSeek 的参数面，本文不重复。
- **本文的判据是「谁在何时发言、讨论何时停」，不问「多 agent 讨论是否提升准确率」。**

## 来源分级

| 标记 | 含义 |
| --- | --- |
| ✅ 本人 | 我本人抓取了该 URL / 源文件，并引用其中原文 |
| ✅ 委派 | 由委派的研究子 agent 抓取并逐字回报；我未二次打开，或只做了局部复核 |
| 🟡 二手 | 只有二手转述或摘要级信息 |

**版本时点（重要）**：所有抓取发生在 **2026-09-12**。

| 框架 | 本文引用的版本 |
| --- | --- |
| Microsoft AutoGen **0.2** | `0.2` 分支源码 + `microsoft.github.io/autogen/0.2/` 文档站 |
| Microsoft AutoGen **0.4+（AgentChat）** | `stable` 文档站 + `main` 分支源码；抓取当日 PyPI `autogen-agentchat` 最新版为 **0.7.5**（<https://pypi.org/pypi/autogen-agentchat/json>） |
| CrewAI | 官方文档站当前版本 **v1.15.21**（<https://docs.crewai.com/v1.15.21/en/concepts/processes>） |
| LangGraph | **1.2.11**（`libs/langgraph/pyproject.toml` + PyPI，✅ 委派）；`langgraph-supervisor` 0.0.31；`langgraph-swarm` 0.1.0 |
| MetaGPT | `FoundationAgents/MetaGPT` `main` 分支（✅ 本人抓源码） |
| OpenAI Agents SDK | 官方文档站 + `openai/openai-agents-python` `main` 分支 |

⚠️ **AutoGen 0.2 与 0.4+ 是两套 API**。下文每个小节都标注所引版本；两者不可混用。

---

## 1. Microsoft AutoGen

### 1.1 版本分界（官方说明）

官方迁移指南原文：

> "This is a migration guide for users of the `v0.2.*` versions of `autogen-agentchat` to the `v0.4` version, which introduces a new set of APIs and features. **The `v0.4` version contains breaking changes.** … We still maintain the `v0.2` version in the `0.2` branch; however, we highly recommend you upgrade to the `v0.4` version."

<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/migration-guide.html>

迁移指南还明确：0.2 的「transforms」长上下文能力在 0.4 变成内建的 `ChatCompletionContext` 组件（见 §1.7）。

### 1.2 AutoGen **0.2**：发言者选择机制

`GroupChat` 的 `speaker_selection_method` 字段（源码 `autogen/agentchat/groupchat.py`，`0.2` 分支）：

```python
speaker_selection_method: Union[Literal["auto", "manual", "random", "round_robin"], Callable] = "auto"
```

源码 docstring 对四个取值的定义（逐字）：

> - "auto": the next speaker is selected automatically by LLM.
> - "manual": the next speaker is selected manually by user input.
> - "random": the next speaker is selected randomly.
> - "round_robin": the next speaker is selected in a round robin fashion, i.e., iterating in the same order as provided in `agents`.
> - a customized speaker selection function (Callable): the function will be called to select the next speaker.

<https://raw.githubusercontent.com/microsoft/autogen/0.2/autogen/agentchat/groupchat.py>（✅ 本人）

自定义函数的签名与返回语义（✅ 本人，源码 docstring + 0.2 官方指南）：

```python
def custom_speaker_selection_func(
    last_speaker: Agent, groupchat: GroupChat
) -> Union[Agent, str, None]:
```

返回三类值之一：一个 `Agent`（必须是参与者之一）、`['auto','manual','random','round_robin']` 之一（交给默认方法）、或 **`None`（即终止整场对话）**。
<https://microsoft.github.io/autogen/0.2/docs/topics/groupchat/customized_speaker_selection/>（✅ 本人）

0.2 指南明确推荐用这个函数做确定性工作流，即 **StateFlow** 模式，并给出示例：`state_transition(last_speaker, groupchat)` 在 `Scientist` 之后返回 `None` 结束。

**"auto" 的实现细节**（✅ 本人，0.2 API 参考页原文）：

> "Create a two-agent chat with a speaker selector agent and a speaker validator agent, like a nested chat … If a single agent is provided then we return it and finish. If not, we add an additional message to this nested chat in an attempt to guide the LLM to a single agent response … If we run out of turns and no single agent can be determined, **the next speaker in the list of agents is returned**"

`max_retries_for_selecting_speaker` 默认 **2**（0.2 源码/文档）。
<https://microsoft.github.io/autogen/0.2/docs/reference/agentchat/groupchat/>

**禁止连续同人发言 / 转移图**（0.2）：`allow_repeat_speaker`（默认 True，为向后兼容）、`allowed_or_disallowed_speaker_transitions` + `speaker_transitions_type`（`"allowed"` / `"disallowed"`），二者互斥，构造时会做图有效性校验。（✅ 本人，`groupchat.py` docstring）

### 1.3 AutoGen **0.2**：终止条件

0.2 的终止条件由**三个互相独立的旋钮**组成，官方教程把它们归为两类：

> "Currently there are two broad mechanism to control the termination of conversations between agents: 1. **Specify parameters in `initiate_chat`** … 2. **Configure an agent to trigger termination**"

<https://microsoft.github.io/autogen/0.2/docs/tutorial/chat-termination>

**(a) `GroupChat.max_round`**（组聊轮数上限）：

```python
max_round: int = 10   # "the maximum number of rounds"
```

<https://raw.githubusercontent.com/microsoft/autogen/0.2/autogen/agentchat/groupchat.py>（✅ 本人）

`GroupChatManager.run_chat` 的实现（✅ 本人）：

```python
for i in range(groupchat.max_round):
    self._last_speaker = speaker
    groupchat.append(message, speaker)
    for agent in groupchat.agents:          # 广播给除当前 speaker 外的所有人
        if agent != speaker:
            self.send(message, agent, request_reply=False, silent=True)
    if self._is_termination_msg(message) or i == groupchat.max_round - 1:
        break
    speaker = groupchat.select_speaker(speaker, self)
    reply = speaker.generate_reply(sender=self)
```

即：**每轮先广播，再检查终止消息，再选下一个 speaker**。

**(b) `ConversableAgent.is_termination_msg`**：签名 `Optional[Callable[[Dict], bool]] = None`，作用对象是**收到的消息**。默认值（源码逐字）：

```python
self._is_termination_msg = (
    is_termination_msg
    if is_termination_msg is not None
    else (lambda x: content_str(x.get("content")) == "TERMINATE")
)
```

⚠️ 关键细节：**0.2 默认是「整条消息内容严格等于 `TERMINATE`」**，不是 `endswith("TERMINATE")`。官方教程里的自定义例子才用 `lambda msg: "good bye" in msg["content"].lower()`。
<https://raw.githubusercontent.com/microsoft/autogen/0.2/autogen/agentchat/conversable_agent.py>（✅ 本人）

**(c) `max_consecutive_auto_reply`**：同一 sender 的连续自动回复上限。类常量 `MAX_CONSECUTIVE_AUTO_REPLY = 100`（✅ 本人，同文件 L63）。

**(d) 人类输入模式会改变终止是否立即生效**（官方原文）：

> "it is important to note that when a termination condition is triggered, the conversation may not always terminate immediately. The actual termination depends on the `human_input_mode` argument … when mode is `NEVER` the termination conditions above will end the conversations. But when mode is `ALWAYS` or `TERMINATE`, it will not terminate immediately."

<https://microsoft.github.io/autogen/0.2/docs/tutorial/chat-termination>

**小结（0.2）**：0.2 没有统一的终止对象；`max_round`（轮数）、`is_termination_msg`（消息内容）、`max_consecutive_auto_reply`（连续回复数）、自定义 `speaker_selection_method` 返回 `None`（选择函数即终止函数）各自独立生效。

### 1.4 AutoGen **0.4+（AgentChat）**：团队形态与发言者选择

0.4+ 把「组聊」拆成若干**内建 Team**，每个的发言者选择规则不同：

| Team | 谁决定下一个发言者 | 来源 |
| --- | --- | --- |
| `RoundRobinGroupChat` | 按注册顺序轮转 | <https://microsoft.github.io/autogen/stable/reference/python/autogen_agentchat.teams.html> |
| `SelectorGroupChat` | **模型**依据参与者 `name` + `description` 选择 | <https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/selector-group-chat.html> |
| `Swarm` | **最近一条 `HandoffMessage` 的 target**（由 agent 用 tool call 生成） | <https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/swarm.html> |
| `MagenticOneGroupChat` | **Orchestrator agent 依据 Progress Ledger 的 `next_speaker` 字段** | <https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/magentic-one.html> |
| `GraphFlow`（实验性） | **有向图** + 边上的 `condition` 函数 | <https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/graph-flow.html> |

`SelectorGroupChat` 的官方步骤（逐字）：

> "1. The team analyzes the current conversation context, including the conversation history and participants' `name` and `description` attributes, to determine the next speaker using a model. By default, the team will **not select the same speak consecutively** unless it is the only agent available. This can be changed by setting `allow_repeated_speaker=True`. You can also override the model by providing a custom selection function.
> 2. The team prompts the selected speaker agent to provide a response, which is then **broadcasted** to all other participants.
> 3. The termination condition is checked to determine if the conversation should end, if not, the process repeats from step 1."

<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/selector-group-chat.html>（✅ 本人）

**`selector_func` / `candidate_func` 的精确语义**（✅ 本人，`main` 分支 `_selector_group_chat.py` 源码 docstring）：

```python
SelectorFuncType = Union[
    Callable[[Sequence[BaseAgentEvent | BaseChatMessage]], str | None],
    Callable[[Sequence[BaseAgentEvent | BaseChatMessage]], Awaitable[str | None]],
]
```

> "A custom selector function that takes the conversation history and returns the name of the next speaker. **If provided, this function will be used to override the model** to select the next speaker. **If the function returns None, the model will be used to select the next speaker.** NOTE: `selector_func` is **not serializable** and will be ignored during serialization and deserialization process."

> "`candidate_func`: A custom function that takes the conversation history and returns a **filtered list of candidates** for the next speaker selection using model. If the function returns an empty list or `None`, `SelectorGroupChat` will raise a `ValueError`. This function is **only used if `selector_func` is not set**."

关键差异 vs 0.2：
- 0.2 的自定义选择函数返回 `None` = **终止对话**；0.4 的 `selector_func` 返回 `None` = **退回模型选择**（不再等于终止）。终止改由独立的 `TerminationCondition` 负责。
- 0.4 `allow_repeated_speaker` 默认 **False**；0.2 的 `allow_repeat_speaker` 默认 **True**。
- 0.4 `max_selector_attempts` 默认 **3**；失败后回退到上一个 speaker（或第一个参与者）。

`SelectorGroupChat` 的自定义选择函数官方示例（值得注意，因为它是一个**「直到对方认可才停」的循环**，见 §5）：

```python
def selector_func(messages: Sequence[BaseAgentEvent | BaseChatMessage]) -> str | None:
    if len(messages) == 1 or messages[-1].to_text() == "Incorrect!":
        return "Agent1"
    if messages[-1].source == "Agent1":
        return "Agent2"
    return None

termination = TextMentionTermination("Correct!")
```

<https://raw.githubusercontent.com/microsoft/autogen/main/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_selector_group_chat.py>（✅ 本人）

`GraphFlow` 的边条件（✅ 本人，官方文档）：

```python
builder.add_edge(reviewer, filtered_summarizer, condition=lambda msg: "APPROVE" in msg.to_model_text())
builder.add_edge(reviewer, generator,           condition=lambda msg: "APPROVE" not in msg.to_model_text())
```

并明确："Supports sequential, parallel, conditional, and looping behaviors." 同时标注 "**Warning:** `GraphFlow` is an **experimental feature**."

<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/graph-flow.html>

### 1.5 AutoGen **0.4+**：终止条件 API 的当前形状

基类 `TerminationCondition`（✅ 本人，`main` 分支 `base/_termination.py`）：

> "A **stateful** condition that determines when a conversation should be terminated. A termination condition is a **callable** that takes a sequence of `BaseChatMessage` objects **since the last time the condition was called**, and returns a `StopMessage` if the conversation should be terminated, or `None` otherwise. Once a termination condition has been reached, it must be `reset` before it can be used again. Termination conditions can be **combined using the AND and OR operators**."

```python
async def __call__(self, messages: Sequence[BaseAgentEvent | BaseChatMessage]) -> StopMessage | None
def __and__(self, other) -> TerminationCondition
def __or__(self, other)  -> TerminationCondition
```

官方文档补充的两条语义（✅ 本人）：

> "For group chat teams … the termination condition is called **after each agent responds**. While a response may contain multiple inner messages, the team calls its termination condition **just once for all the messages from a single response**. So the condition is called with the '**delta sequence**' of messages since the last time it was called."

<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/tutorial/termination.html>（✅ 本人）

**内建终止条件清单（0.4+，逐字）**：

1. `MaxMessageTermination` — "Stops after a specified number of messages have been produced, including both agent and task messages."
2. `TextMentionTermination` — "Stops when specific text or string is mentioned in a message (e.g., 'TERMINATE')."
3. `TokenUsageTermination` — "Stops when a certain number of prompt or completion tokens are used. This requires the agents to report token usage in their messages."
4. `TimeoutTermination` — "Stops after a specified duration in seconds."
5. `HandoffTermination` — "Stops when a handoff to a specific target is requested. … This is useful when you want to **pause the run and allow application or user to provide input** when an agent hands off to them."
6. `SourceMatchTermination` — "Stops after a specific agent responds."
7. `ExternalTermination` — "Enables programmatic control of termination from outside the run. This is useful for UI integration (e.g., 'Stop' buttons in chat interfaces)."
8. `StopMessageTermination` — "Stops when a `StopMessage` is produced by an agent."
9. `TextMessageTermination` — "Stops when a `TextMessage` is produced by an agent."
10. `FunctionCallTermination` — "Stops when a `ToolCallExecutionEvent` containing a `FunctionExecutionResult` with a matching name is produced by an agent."
11. `FunctionalTermination` — "Stop when a function expression is evaluated to `True` on the last delta sequence of messages."

<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/tutorial/termination.html>（✅ 本人）

对应 API 参考页参数（✅ 本人）：
- `MaxMessageTermination(max_messages: int, include_agent_event: bool = False)`
- `TextMentionTermination(text: str, sources: Sequence[str] | None = None)` — "Check only messages of the specified agents for the text to look for."
- `TokenUsageTermination(max_total_token=None, max_prompt_token=None, max_completion_token=None)` — 三者至少给一个，否则 `ValueError`
- `TimeoutTermination(timeout_seconds: float)`
- `HandoffTermination(target: str)`
- `SourceMatchTermination(sources: List[str])`
- `ExternalTermination()` + `set()`

<https://microsoft.github.io/autogen/stable/reference/python/autogen_agentchat.conditions.html>

**`max_turns` 与 `TerminationCondition` 是两套并行的机制**。0.4+ 各 Team 另有独立参数：

> "`max_turns` (int, optional): The maximum number of turns in the group chat before stopping. **Defaults to None, meaning no limit.**"

`BaseGroupChatManager._apply_termination_condition` 中的实现（✅ 本人）：

```python
if self._max_turns is not None:
    if self._current_turn >= self._max_turns:
        stop_message = StopMessage(content=f"Maximum number of turns {self._max_turns} reached.", ...)
```

<https://raw.githubusercontent.com/microsoft/autogen/main/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_base_group_chat_manager.py>

⇒ **差异**：0.4 默认**不限轮**（`max_turns=None`，且 `termination_condition=None`），官方 docstring 明说 "Without a termination condition, the group chat will run indefinitely."；**0.2 默认 10 轮**。

### 1.6 AutoGen：是否存在内建的「分歧 / 收敛」检测？

**没有。** 逐条核对 0.4+ 的 11 个内建条件（§1.5），没有任何一个涉及「参与者是否同意」「投票是否平局」「结论是否收敛」。官方给出的扩展方式只有两种：`FunctionalTermination`（用户写函数，作用在 delta 消息上）与「继承 `TerminationCondition` 写自定义类」（§1.5 教程的 `FunctionCallTermination` 示例）。

Magentic-One 里最接近的机制是**停滞检测（stall），不是分歧检测**。其 Progress Ledger 的字段为（✅ 本人，`_magentic_one_orchestrator.py`）：

```python
"is_request_satisfied",
"is_progress_being_made",
"is_in_loop",
"instruction_or_question",
"next_speaker",
```

计数逻辑（源码逐字）：

```python
if progress_ledger["is_request_satisfied"]["answer"]:
    await self._prepare_final_answer(progress_ledger["is_request_satisfied"]["reason"], cancellation_token)
# Check for stalling
if not progress_ledger["is_progress_being_made"]["answer"]:
    self._n_stalls += 1
elif progress_ledger["is_in_loop"]["answer"]:
    self._n_stalls += 1
else:
    self._n_stalls = max(0, self._n_stalls - 1)
# Too much stalling
if self._n_stalls >= self._max_stalls:
    ...
```

`MagenticOneGroupChat` 的默认值（✅ 本人，源码 docstring）：`max_turns` 默认 **20**，`max_stalls` 默认 **3**；官方文档对其外/内双循环的描述是：

> "If the Orchestrator finds that **progress is not being made for enough steps, it can update the Task Ledger and create a new plan**."

<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/magentic-one.html>
<https://raw.githubusercontent.com/microsoft/autogen/main/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_magentic_one/_magentic_one_orchestrator.py>

⇒ Magentic-One 的 `is_in_loop` / `is_progress_being_made` 是**进度判据**（是否卡住），触发的是**重规划**，不是「因为结论不一致而继续」。

### 1.7 AutoGen：多轮之后的上下文膨胀怎么处理？

**(a) 0.2**：官方迁移指南原文：

> "In `v0.2`, long context that overflows the model's context window can be handled by using the **`transforms`** capability that is added to a `ConversableAgent` after it is constructed."

组聊场景另有实验性的 `enable_clear_history`：在用户/agent 回复中写 `"clear history"`、`"clear history <agent_name>"`、`"clear history <n>"` 会清空（或保留最后 n 条）历史。源码 docstring 明确标注这是 "experimental feature"。
<https://raw.githubusercontent.com/microsoft/autogen/0.2/autogen/agentchat/groupchat.py>（✅ 本人）

**(b) 0.4+**：内建 `ChatCompletionContext` 抽象层，提供**消息历史的「虚拟视图」**：

> "In `v0.4`, we introduce the `ChatCompletionContext` base class that manages message history and provides a virtual view of the history. Applications can use built-in implementations such as `BufferedChatCompletionContext` to limit the message history sent to the model, or provide their own implementations that creates different virtual views."

<https://microsoft.github.io/autogen/stable/_sources/user-guide/agentchat-user-guide/migration-guide.md.txt>（✅ 本人）

可用的内建实现（✅ 本人，`autogen_core/model_context/__init__.py`）：

| 类 | 行为 | 来源 |
| --- | --- | --- |
| `UnboundedChatCompletionContext` | 不裁剪 | 同上 |
| `BufferedChatCompletionContext(buffer_size)` | "keeps a view of the **last n messages**" | `_buffered_chat_completion_context.py` |
| `HeadAndTailChatCompletionContext(head_size, tail_size)` | "keeps a view of the **first n and last m** messages" | `_head_and_tail_chat_completion_context.py` |
| `TokenLimitedChatCompletionContext(model_client, token_limit=None)` | "**(Experimental)** A token based chat completion context maintains a view of the context up to a token limit. … Added in v0.4.10." | `_token_limited_chat_completion_context.py` |

⚠️ **默认是不裁剪**（✅ 本人，源码）：
- `AssistantAgent.__init__`：`else: self._model_context = UnboundedChatCompletionContext()`
- `SelectorGroupChatManager.__init__`：`else: self._model_context = UnboundedChatCompletionContext()`

⇒ 官方机制是「**可配置的视图裁剪**」，不是自动摘要、也不是自动截断；`SelectorGroupChat` 的 `model_context` 只影响**选择 speaker 那次调用**看到的上下文。

`GraphFlow` 另提供**消息图（message graph）**作为独立于执行图的过滤层：

> "the execution graph does not control what messages an agent receives from other agents. **By default, all messages are sent to all agents in the graph.** Message filtering is a separate feature that allows you to filter the messages received by each agent and limiting their model context to only the relevant information."

并用 `MessageFilterAgent` + `PerSourceFilter(source=..., position="last", count=1)` 表达「只看某人的最后一条」。
<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/graph-flow.html>（✅ 本人）

---

## 2. CrewAI

引用版本：官方文档 **v1.15.21**。

### 2.1 sequential / hierarchical 怎么定义

`Process` 是一个只有两个值的枚举（官方原文）：

> "* **Sequential**: Executes tasks sequentially, ensuring tasks are completed in an orderly progression.
> * **Hierarchical**: Organizes tasks in a managerial hierarchy, where tasks are delegated and executed based on a structured chain of command. A manager language model (`manager_llm`) or a custom manager agent (`manager_agent`) must be specified in the crew to enable the hierarchical process, facilitating the creation and management of tasks by the manager."
>
> "The `Process` class is implemented as an enumeration (`Enum`), ensuring type safety and restricting process values to the defined types (`sequential`, `hierarchical`)."

<https://docs.crewai.com/en/concepts/processes.md>（✅ 本人）

- **Sequential**："Task execution follows the predefined order in the task list, with the output of one task serving as context for the next."
- **Hierarchical**："This agent oversees task execution, including planning, delegation, and validation. **Tasks are not pre-assigned; the manager allocates tasks to agents based on their capabilities, reviews outputs, and assesses task completion.**"

`Crew` 的 `process` 默认值为 `sequential`（官方属性表：Process "The process flow (e.g., sequential, hierarchical) the crew follows. Default is `sequential`."）。
<https://docs.crewai.com/en/concepts/crews.md>（✅ 本人）

### 2.2 谁决定谁发言

| 模式 | 发言决定者 | 依据 |
| --- | --- | --- |
| Sequential | **任务列表顺序**（静态） | <https://docs.crewai.com/en/concepts/processes.md> |
| Hierarchical | **manager agent / manager_llm** | 同上 |

官方 tasks 文档：

> "**Sequential**: Tasks are executed in the order they are defined
> **Hierarchical**: Tasks are assigned to agents based on their roles and expertise"

> "Directly specify an `agent` for assignment or let the `hierarchical` CrewAI's process decide based on roles, availability, etc."

<https://docs.crewai.com/en/concepts/tasks.md>（✅ 本人）

⚠️ 关键差异（相对 AutoGen/LangGraph）：**CrewAI 的发言权不是「轮」，而是「任务」**。Sequential 下每个 task 绑定一个 agent（或由 process 分配），task 完成即进入下一个 task；**不存在一个「每轮重新选谁说话」的循环**。

### 2.3 有没有「每轮」的终止条件？

**没有轮级终止条件**——因为 CrewAI 的 Crew 没有「轮」这一层抽象。可查到的边界条件是：

| 层级 | 参数 | 官方语义 | 来源 |
| --- | --- | --- | --- |
| Task | `guardrail` / `guardrails` + `guardrail_max_retries`（默认 **3**） | 校验失败时把错误发回 agent 重试，直到通过或耗尽重试 | <https://docs.crewai.com/en/concepts/tasks.md> |
| Task | `human_input`（默认 False） | "Whether the task should have a human review the final answer of the agent." | 同上 |
| Task | `async_execution`、`context` | 依赖/异步，不是终止条件 | 同上 |
| Agent | `max_iter` | "Maximum iterations before the agent must provide its best answer. **Default is 20**."（文档表与正文均写 20） | <https://docs.crewai.com/en/concepts/agents.md> |
| Agent | `max_execution_time` | "Maximum time (in seconds) for task execution." | 同上 |
| Agent | `max_retry_limit` | "Maximum number of retries when an error occurs. Default is 2." | 同上 |
| Agent | `max_rpm` / Crew `max_rpm` | 速率上限，不是终止 | 同上 / crews.md |
| Agent | `respect_context_window`（默认 True） | 超窗时自动摘要；False 则报错停止 | agents.md |

> "If any guardrail fails, the error is sent back to the agent, and the task is retried up to `guardrail_max_retries` times."

<https://docs.crewai.com/en/concepts/tasks.md>（✅ 本人）

**条件分支/循环在 CrewAI 里属于 Flows，而不是 Crew。** 官方 Flows 文档：

- `@router()` — "allows you to define conditional routing logic based on the output of a method… If the boolean is `True`, the method returns `"success"` … `@listen("success")` / `@listen("failed")`"（✅ 本人，<https://docs.crewai.com/en/concepts/flows.md>）
- `@human_feedback(message=..., emit=[...])` — "pauses flow execution to collect feedback from a human… **requires CrewAI version 1.8.0 or higher**"；`emit` 指定时由 LLM 把人类自由文本折叠成给定选项之一（✅ 本人，同上）
- `@persist` — 状态持久化/恢复（同上）
- tasks 文档另提到 JSONC 配置里可用 `"type": "ConditionalTask"` 配 `condition` 字段（✅ 本人，tasks.md）

⇒ 即：**「是否重开一轮、何时停」在 CrewAI 里由用户在 Python 里写的 router / guardrail 决定，框架本身不提供轮级判据。**

### 2.4 上下文膨胀

`respect_context_window=True`（默认）时，官方描述的行为（✅ 本人，agents.md）：

> "⚠️ **Warning message**: `"Context length exceeded. Summarizing content to fit the model context window."`
> 🔄 **Automatic summarization**: CrewAI intelligently summarizes the conversation history
> ✅ **Continued execution**: Task execution continues seamlessly with the summarized context"

这与 AutoGen 0.4 的「视图裁剪」不同：CrewAI 是**自动摘要**；AutoGen 0.4 默认不裁剪、需要显式选 context 类。

---

## 3. LangGraph

引用版本：**langgraph 1.2.11**；`langgraph-supervisor` 0.0.31；`langgraph-swarm` 0.1.0（✅ 委派，来自 pyproject / PyPI）。

⚠️ **文档位置变动（事实，非评价）**：旧站 `langchain-ai.github.io/langgraph/concepts/multi_agent/` 现在只返回 JS 跳转页，`canonical` 指向 `https://docs.langchain.com/oss/python/langgraph/graph-api`；当日的多 agent 指南位于 **LangChain** 路径 `docs.langchain.com/oss/python/langchain/multi-agent/...`，LangGraph 路径下已无 `multi-agent` 页（`/oss/python/langgraph/multi-agent` → 404）。
（✅ 本人，抓取 `https://langchain-ai.github.io/langgraph/concepts/multi_agent/` 的 HTML 与多个 `.md` 端点确认）

### 3.1 轮次与条件边怎么表达

官方定义（✅ 委派，逐字，<https://docs.langchain.com/oss/python/langgraph/graph-api>）：

> "At its core, LangGraph models agent workflows as graphs. You define the behavior of your agents using three key components:"
> 1. State — "A shared data structure that represents the current snapshot of your application…"
> 2. Nodes — "Functions that encode the logic of your agents. They receive the current state as input, perform some computation or side-effect, and return an updated state."
> 3. Edges — "Functions that determine which `Node` to execute next based on the current state. They can be conditional branches or fixed transitions."
>
> "By composing `Nodes` and `Edges`, you can create complex, **looping** workflows that evolve the state over time."

条件边（✅ 本人，`graph-api.md`）：

> "If you want to **optionally** route to one or more edges (or optionally terminate), you can use the [`add_conditional_edges`](https://reference.langchain.com/python/langgraph/graph/state/StateGraph/add_conditional_edges) method. This method accepts the name of a node and a 'routing function' to call after that node is executed"

```python
graph.add_conditional_edges("node_a", routing_function, {True: "node_b", False: "node_c"})
```

⇒ **LangGraph 没有「轮（round）」这个一等对象**：一轮 = 一个 super-step；「讨论多轮」= 图里的环（edge 指回上游节点）。

### 3.2 步数上限：`recursion_limit`

官方原文（✅ 委派，逐字，`graph-api.md`）：

> "The **recursion limit** sets the maximum number of super-steps the graph can execute during a single execution. Once the limit is reached, LangGraph will raise `GraphRecursionError`. **Starting in version 1.0.6, the default recursion limit is set to 1000 steps.** The recursion limit can be set on any graph at runtime, and is passed to `invoke`/`stream` via the config dictionary."

> "The step counter is stored in `config["metadata"]["langgraph_step"]`. LangGraph increments this counter as the graph executes and raises a `GraphRecursionError` once the configured `recursion_limit` is exceeded."

错误类（✅ 委派，源码 `langgraph/errors.py`）：

```python
class GraphRecursionError(RecursionError):
    """Raised when the graph has exhausted the maximum number of steps. This prevents infinite loops. ..."""
```

⚠️ **文档与源码不一致（必须标明）**：文档写默认 **1000**，而 `main` 分支 `langgraph/_internal/_config.py` 实际常量为

```python
DEFAULT_RECURSION_LIMIT = int(getenv("LANGGRAPH_DEFAULT_RECURSION_LIMIT", "10007"))
```

（✅ 本人复核了该行；委派子 agent 另指出提交 `a5827c5` 的 diff 字面量是 `"10000"`，与 commit 说明中的 "1000" 也不一致。）**因此「默认值」一条以官方文档的 1000 为准陈述，并注明代码常量可被 `LANGGRAPH_DEFAULT_RECURSION_LIMIT` 覆盖、且当前为 `10007`。**

### 3.3 `interrupt` / checkpoint 与「暂停讨论等人类输入」

官方原文（✅ 本人，`interrupts.md`）：

> "**Interrupts allow you to pause graph execution at specific points and wait for external input before continuing.** This enables human-in-the-loop patterns where you need external input to proceed. When an interrupt is triggered, LangGraph saves the graph state using its persistence layer and **waits indefinitely** until you resume execution."

> "Unlike static breakpoints (which pause before or after specific nodes), interrupts are **dynamic**: they can be placed anywhere in your code and can be conditional based on your application logic."

使用条件（逐字）：

> "To use `interrupt`, you need:
> 1. A **checkpointer** to persist the graph state (use a durable checkpointer in production)
> 2. A **thread ID** in your config so the runtime knows which state to resume from
> 3. To call `interrupt()` where you want to pause (payload must be JSON-serializable)"

恢复（✅ 本人）：

> "The value passed to `Command(resume=...)` becomes the return value of the `interrupt()` call."

> "The node **restarts from the beginning of the node** where the `interrupt` was called when resumed, so any code before the `interrupt` runs again"

静态断点的现状（✅ 本人）：

> "Static interrupts are triggered at defined points either before or after a node executes. You can set these by specifying `interrupt_before` and `interrupt_after` when compiling the graph." … "**Static interrupts are not recommended for human-in-the-loop workflows. Use the `interrupt` function instead.**"

版本沿革（✅ 委派，来自官方 in-repo 概念文档）："As of LangGraph **0.2.57**, the recommended way to set breakpoints is using the `interrupt` function as it simplifies human-in-the-loop patterns."；当前 HITL 文档补充 "As of v1.0, `interrupt` is the recommended way to pause a graph. **`NodeInterrupt` is deprecated and will be removed in v2.0.**"

checkpointer 的官方职责（✅ 委派）："Checkpointers are required for the following features: … **Human-in-the-loop**: Checkpointers facilitate human-in-the-loop workflows by allowing humans to inspect, interrupt, and approve graph steps."
<https://docs.langchain.com/oss/python/langgraph/checkpointers>

⇒ 对「暂停讨论等人类输入」而言：LangGraph 提供的是**通用暂停/恢复原语**（`interrupt()` + checkpointer + `Command(resume=)`），不是「讨论投票暂停」这类领域机制。

### 3.4 官方多 agent 模式：supervisor / swarm

**(a) 当前官方模式表**（✅ 本人，<https://docs.langchain.com/oss/python/langchain/multi-agent>）：

| 模式 | 官方描述 |
| --- | --- |
| Subagents | "A main agent coordinates subagents as tools. **All routing passes through the main agent**, which decides when and how to invoke each subagent." |
| Handoffs | "Behavior changes dynamically based on state. Tool calls update a state variable that triggers routing or configuration changes, switching agents or adjusting the current agent's tools and prompt." |
| Skills | 单 agent 按需加载上下文 |
| Router | "A routing step classifies input and directs it to one or more specialized agents. **Results are synthesized** into a combined response." |
| Custom workflow | "Build bespoke execution flows with LangGraph, mixing deterministic logic and agentic behavior." |

**(b) `langgraph-supervisor`（官方库）**：

> "The supervisor controls all communication flow and task delegation, making decisions about which agent to invoke based on the current context and task requirements."
> "🛠️ **Tool-based agent handoff mechanism** for communication between agents"

源码结构（✅ 委派）：`builder.add_node(supervisor_agent, destinations=tuple(agent_names) + (END,))`，每个 worker agent 都有一条 `builder.add_edge(agent.name, supervisor_agent.name)` → **每个 worker 说完都把控制权交回 supervisor**；`END` 是 supervisor 的合法目的地。默认 handoff 工具名为 `transfer_to_<agent_name>`。
<https://raw.githubusercontent.com/langchain-ai/langgraph-supervisor-py/main/README.md>

⚠️ **重要官方状态变化（逐字）**：

> "**Note**: We now recommend using the **supervisor pattern directly via tools** rather than this library for most use cases. … We're making this library compatible with LangChain 1.0 to help users upgrade their existing code."

并且迁移指南写："The `langgraph-supervisor` package is **no longer actively maintained**. Instead use the [subagents] pattern."（✅ 委派，<https://docs.langchain.com/oss/python/migrate/langgraph-supervisor>）

⚠️ **终止条件**：`create_supervisor` 的签名只有 agents / model / tools / prompt / output_mode / **pre_model_hook** 等，**没有 max_turns、没有 FINISH 哨兵**（✅ 委派，对照 reference 页与源码）。唯一的步数上限是 LangGraph 自己的 `recursion_limit`。

**(c) `langgraph-swarm`（官方库）**：

> "A swarm is a type of multi-agent architecture where agents **dynamically hand off control to one another** based on their specializations. The system remembers which agent was last active, ensuring that on subsequent interactions, the conversation resumes with that agent."

`SwarmState` 带 `active_agent`；`create_handoff_tool` 默认工具名 `transfer_to_<agent_name>`，返回 `Command(goto=agent_name, graph=Command.PARENT, update={"messages": [...], "active_agent": agent_name})`；`create_swarm` 只接受 `agents` / `default_active_agent` / `state_schema` / `context_schema` —— **没有终止/轮数参数**（✅ 委派 + ✅ 本人读 README）。
<https://raw.githubusercontent.com/langchain-ai/langgraph-swarm-py/main/README.md>

**(d) Handoffs 模式的终止是用户写的**（✅ 委派，<https://docs.langchain.com/oss/python/langchain/multi-agent/handoffs>）：官方示例里的 `route_after_agent` 注释为 "# Check the last message - if it's an AIMessage without tool calls, we're done"，即没有内建 `FINISH` 常量。

### 3.5 上下文膨胀

**没有自动裁剪/摘要。**（✅ 委派）

- `langgraph-supervisor` 提供 `pre_model_hook`，但它**只作用于 supervisor agent、且必须用户提供**："An optional node to add before the LLM node in the supervisor agent … Useful for managing long message histories (e.g., message trimming, summarization, etc.)."
- LangChain 侧把裁剪/摘要列为**用户配置的模式**："Common solutions are: Trim messages / Delete messages / Summarize messages / Custom strategies"（<https://docs.langchain.com/oss/python/langchain/short-term-memory>）
- 存在内建中间件但**必须显式挂载**：`SummarizationMiddleware` — "Automatically summarize conversation history when approaching token limits, preserving recent messages while compressing older context."（<https://docs.langchain.com/oss/python/langchain/middleware/built-in>）
- 另有存储侧的 `DeltaChannel`（langgraph>=1.2, beta）："stores only incremental deltas instead of the full accumulated value, substantially reducing checkpoint size for append-heavy channels."——这是 **checkpoint 体积**优化，不是模型上下文裁剪。

### 3.6 LangGraph 有没有内建「分歧 / 收敛」检测？

**没有。**（✅ 委派）搜索了官方文档索引与 `langgraph-supervisor` / `langgraph-swarm` 的 README/源码/reference：**不存在** vote、tie-breaker、consensus、"agents disagree"、或「supervisor 决定是否继续讨论」的任何原语。最接近的官方构造是：

- **Evaluator-optimizer**（<https://docs.langchain.com/oss/python/langgraph/workflows-agents>）："one LLM call creates a response and the other evaluates that response. If the evaluator or a human-in-the-loop determines the response needs refinement, feedback is provided and the response is recreated. This loop continues until an acceptable response is generated."——用户自建循环。
- **`RubricMiddleware`**（beta，`deepagents>=0.6.5`）："Apply LLM-as-a-judge grading so agents self-evaluate and iterate until a rubric is satisfied." / "…iterate until the rubric is satisfied **or a maximum iteration cap is hit**."——**单 agent 自评**，不是 agent 间投票。

---

## 4. 终止判据的「种类」清单（跨框架，均附来源）

先看 OpenAI Agents SDK（OpenAI Swarm 的官方继任者）对「编排」的官方定义，它把「谁在何时发言」这个问题本身说得很清楚（✅ 本人，逐字）：

> "Orchestration refers to the flow of agents in your app. **Which agents run, in what order, and how is the next step decided?** There are two main ways to orchestrate agents: **Allowing the LLM to make decisions** … **Orchestrating via code**: determining the flow of agents via your code."

该页的两种核心模式是 "Agents as tools"（manager agent 保持控制权并调用专家）与 "Handoffs"（triage agent 把会话路由给专家，专家成为该轮的 active agent）。**该文档全文检索 `debate|discussion|vote|consensus` 命中 0 次。**
<https://openai.github.io/openai-agents-python/multi_agent/>（✅ 本人）

下表把各框架实际使用的终止判据按**种类**归类。同一框架可能同时用多种。

| # | 判据种类 | 框架（版本） | 具体 API / 事实 | 来源 |
| --- | --- | --- | --- | --- |
| 1 | 固定轮数 | AutoGen 0.2 | `GroupChat.max_round`，默认 10 | `groupchat.py` L115 |
| 2 | 固定轮数 | AutoGen 0.4+ | Team `max_turns`，**默认 None**；Magentic-One 默认 20 | `_base_group_chat_manager.py`；`_magentic_one_group_chat.py` |
| 3 | 固定消息数（计 delta） | AutoGen 0.4+ | `MaxMessageTermination(max_messages, include_agent_event=False)` | conditions 参考页 |
| 4 | 固定步数（super-step） | LangGraph 1.x | `recursion_limit`，文档默认 1000（1.0.6 起）；超限抛 `GraphRecursionError` | graph-api.md |
| 5 | 固定轮数 | MetaGPT | `Team.run(n_round=3, ...)`："Run company until target round or no money" | `metagpt/team.py` L123 |
| 6 | 显式终止文本 | AutoGen 0.2 | `is_termination_msg`，默认 `content == "TERMINATE"`（严格相等） | `conversable_agent.py` L149-152 |
| 7 | 显式终止文本 | AutoGen 0.4+ | `TextMentionTermination(text, sources=None)` | conditions 参考页 |
| 8 | 显式终止消息对象 | AutoGen 0.4+ | `StopMessageTermination`；GraphFlow 结束时发 `StopMessage` "Digraph execution is complete" | termination 教程；graph-flow 页 |
| 9 | 特定 agent 说完就停 | AutoGen 0.4+ | `SourceMatchTermination(sources)` | conditions 参考页 |
| 10 | 某个 tool/函数被调用 | AutoGen 0.4+ | `FunctionCallTermination(function_name)` | termination 教程 |
| 11 | 任意函数为真 | AutoGen 0.4+ | `FunctionalTermination`；`TerminationCondition` 子类化 | termination 教程 |
| 12 | token 上限 | AutoGen 0.4+ | `TokenUsageTermination(max_total_token / max_prompt_token / max_completion_token)` | conditions 参考页 |
| 13 | 时间上限 | AutoGen 0.4+ | `TimeoutTermination(timeout_seconds)` | conditions 参考页 |
| 14 | 时间上限 | CrewAI | Agent `max_execution_time`（秒） | agents.md |
| 15 | **成本上限** | MetaGPT | `Team.invest(investment)` → `cost_manager.max_budget`；`_check_balance()` 在 `total_cost >= max_budget` 时抛 `NoMoneyException`；默认 `investment=10.0`、`CostManager.max_budget=10.0` | `team.py` L92-100；`utils/cost_manager.py` |
| 16 | 停滞 / 死循环检测 | AutoGen 0.4+ Magentic-One | Progress Ledger 的 `is_progress_being_made` / `is_in_loop` → `_n_stalls`；`max_stalls` 默认 3 → 重规划 | `_magentic_one_orchestrator.py` |
| 17 | 系统空闲（所有人都没事做） | MetaGPT | `Team.run` 内 `if self.env.is_idle: break`（"All roles are idle."） | `team.py` L128-130 |
| 18 | 外部程序化停止 | AutoGen 0.4+ | `ExternalTermination()` + `.set()`（UI 的 "Stop" 按钮） | termination 教程 |
| 19 | 回合耗尽转错误/兜底输出 | OpenAI Agents SDK | `max_turns`（`DEFAULT_MAX_TURNS = 10`；`None` 关闭）超限抛 `MaxTurnsExceeded`；可用 `error_handlers={"max_turns": ...}` 转成受控 final output | `running_agents` 文档；`run_config.py` L45 |
| 20 | 「最终输出」判据 | OpenAI Agents SDK | "The rule for whether the LLM output is considered as a 'final output' is that it produces text output with the desired type, and there are no tool calls." | `running_agents` 文档 |
| 21 | handoff 到人类 → 暂停 | AutoGen 0.4+ | `HandoffTermination(target="user")` | conditions 参考页；swarm 页 |
| 22 | 裁判模型 / 评分阈值 | LangChain/LangGraph 生态（Deep Agents，beta） | `RubricMiddleware`："Apply LLM-as-a-judge grading so agents self-evaluate and iterate until a rubric is satisfied" 或触及最大迭代上限 | middleware/built-in |
| 23 | 裁判模型作为验证门（失败则重开） | CrewAI | LLM 型 guardrail（字符串描述）返回 `(False, feedback)` → 回传给 agent 重试，上限 `guardrail_max_retries`（默认 3） | tasks.md |

（第 24 类候选「所有参与方都无话可说」在本次核查的任何框架中**均无内建实现**，只能由用户自己维护状态，见 §5。）

**「终止判据种类」的归纳（事实层面）**：可观测到的判据共 **8 类**——(a) 轮/消息/步计数；(b) 显式终止文本或终止消息对象；(c) 特定来源 agent 或 tool 调用；(d) 任意用户函数（含调用裁判模型）；(e) 资源上限（token / 时间 / **成本**）；(f) 停滞或死循环检测；(g) 系统空闲；(h) 外部程序化停止。**没有一类是「参与者结论是否一致」。**

---

## 5. 「只在结论不一致时才重开一轮」有没有先例？

**结论：没有找到任何框架提供内建的「分歧触发继续」机制。这是一个已核实的缺失（verified absence）。**

核查范围（均为一手来源）：

| 框架 | 查了什么 | 结果 |
| --- | --- | --- |
| AutoGen 0.4+ | 全部 11 个内建 `TerminationCondition` + `FunctionalTermination` + 自定义子类教程 + SelectorGroupChat / Swarm / MagenticOne / GraphFlow 文档 | 无投票、无平局、无「是否同意」判据 |
| AutoGen 0.2 | `GroupChat` 全部 speaker selection / 终止字段 + StateFlow 指南 | 无 |
| CrewAI v1.15.21 | processes / crews / tasks / agents / flows | 无；只有 router 与 guardrail（用户写） |
| LangGraph 1.2.11 | graph-api / interrupts / persistence / multi-agent / subagents / handoffs / supervisor 迁移页 + `langgraph-supervisor` / `langgraph-swarm` README 与源码 | 无 |
| MetaGPT | `team.py` / `roles/role.py` | 无 |
| OpenAI Agents SDK | running_agents / handoffs / multi_agent | 无 |

**最接近的 5 个官方构造（都不是「分歧触发」，但容易被误当成先例）：**

1. **AutoGen 官方 `selector_func` 示例——「直到对方认可才停」**（✅ 本人，`_selector_group_chat.py` docstring）。逻辑是：`messages[-1].to_text() == "Incorrect!"` 就把 Agent1 再叫回来；`Agent2` 说 `"Correct!"` 时由 `TextMentionTermination("Correct!")` 结束。**这是「不一致就继续」的等价物，但它是用户写的示例代码，不是内建机制**，且判据是「对方的对错判断」，不是「双方结论是否相同」。
2. **CrewAI guardrail 重试**：校验失败 → 打回重做，上限 `guardrail_max_retries`。（tasks.md）
3. **LangGraph evaluator-optimizer**："This loop continues until an acceptable response is generated."——由 evaluator 或人来判断，不是投票。（workflows-agents）
4. **Deep Agents `RubricMiddleware`**：LLM-as-judge 按 rubric 打分，未达标就继续迭代，直到满足 rubric 或触及最大迭代上限。（middleware/built-in）
5. **Magentic-One 停滞重规划**：`is_progress_being_made=False` 或 `is_in_loop=True` 累计到 `max_stalls` → 换计划。（magentic-one 文档 + orchestrator 源码）

⇒ 这 5 条的共同点是「**失败/停滞/未达标 → 继续**」，而票 16 问的是「**不一致 → 继续**」。**前者有官方先例（且多由用户逻辑实现），后者在本次核查范围内没有先例。**

---

## 6. 裁判 / 聚合器：谁做最终合成

### 6.1 各框架的默认归属

| 框架 | 谁产出最终答案 | 来源 |
| --- | --- | --- |
| AutoGen 0.4+（一般 Team） | **框架不给 synthesizer**；`run()` 返回 `TaskResult(messages=[...])`，由调用方取用 | termination 教程的 `TaskResult` 输出 |
| AutoGen Magentic-One | **Orchestrator agent 自己写最终答案**：`is_request_satisfied` 为真时调用 `_prepare_final_answer(reason, ...)`，用 `ORCHESTRATOR_FINAL_ANSWER_PROMPT` 再调一次模型 | `_magentic_one_orchestrator.py` L478-496（✅ 本人） |
| CrewAI（Sequential/Hierarchical） | **最后一个 task 的输出就是 crew 的输出** | tasks.md："It's also important to note that the output of the final task of a crew becomes the final output of the actual crew itself." |
| CrewAI Hierarchical | manager agent "reviews outputs, and assesses task completion" | processes.md |
| LangGraph（Router 模式） | 官方图里有独立的 **Synthesize 节点**："Results are synthesized into a coherent response" | multi-agent 模式表 + router.md（✅ 本人） |
| LangGraph supervisor / swarm 库 | **无专门 synthesizer**；supervisor 可选择 `END` 结束，最后由谁产出内容取决于用户 prompt | supervisor README / swarm README |
| Anthropic 多 agent 研究系统（官方工程实践，非框架） | LeadResearcher "synthesizes these results and decides whether more research is needed"；最后由 CitationAgent 处理引用 | <https://www.anthropic.com/engineering/multi-agent-research-system>（✅ 本人） |

### 6.2 官方对裁判偏见的说明与缓解

**（a）MT-Bench / LLM-as-a-judge 论文（一手论文，非二手）** — Zheng et al., arXiv 2306.05685（✅ 本人读 arXiv/ar5iv 原文）：

- 摘要逐字："We examine the usage and limitations of LLM-as-a-judge, including **position, verbosity, and self-enhancement biases**, as well as limited reasoning ability, and propose solutions to mitigate some of them."
- 位置偏见的定义："**Position bias** is when an LLM exhibits a propensity to favor certain positions over others."
- **位置偏见的实测（Table 2，swap 顺序后判断一致的比例）**：Claude-v1 default **consistency 23.8%**、**biased toward first 75.0%**；改用 "rename" prompt 后 consistency 升到 **56.2%**。GPT-3.5 default consistency 46.2%；**GPT-4 default consistency 65.0%**（biased toward first 30.0%）。
- 原文对 GPT-4 的评述："Only GPT-4 outputs consistent results in more than 60% of cases."；并说明该测试用的是「同一模型以 temperature 0.7 生成的两个相似答案」，对人和模型都很难。
- 缓解手段（论文提出的）：**交换候选顺序（swap positions）**、重命名/改写提示（"rename"）、few-shot 参考、CoT（Table 4 显示数学题上的失败率 Default 14/20 → CoT 6/20 → Reference 3/20）。
- **自我偏好（self-enhancement bias）**："GPT-4 favors itself with a **10% higher win rate**; Claude-v1 favors itself with a **25% higher win rate**." 但作者同时明确限定：**"our study cannot determine whether the models exhibit a self-enhancement bias."**（⚠️ 引用时必须带上这句限定。）
<https://arxiv.org/abs/2306.05685>

**（b）Anthropic 官方工程博客的多 agent 裁判实践**（✅ 本人，逐字）：

> "We used an LLM judge that evaluated each output against criteria in a **rubric**: factual accuracy …, citation accuracy …, completeness …, source quality …, and tool efficiency …. We **experimented with multiple judges** to evaluate each component, but found that a **single LLM call with a single prompt outputting scores from 0.0-1.0 and a pass-fail grade was the most consistent and aligned with human judgements**."

> "Human evaluation catches what automation misses. People testing agents find edge cases that evals miss. These include hallucinated answers on unusual queries, system failures, or **subtle source selection biases**."

<https://www.anthropic.com/engineering/multi-agent-research-system>

⇒ 官方给出的缓解方向是：**单次调用 + 明确 rubric + 0–1 分与 pass/fail 并存 + 人工复核兜底**；而「多个裁判取平均」被其自身实验否定。

**（c）本仓库已有覆盖**：`research/01` §4.7 已记录 Zheng et al. 的位置偏见表述、verbosity bias、以及 Liang et al. 关于「judge 偏向与自己同 backbone 的一方」（"the judge shows a preference to the side with the same LLM as the backbone"）。本节只补充框架层的裁判/合成归属，**不复述** `research/01` 的内容。

**（d）未能在官方文档中找到的**：Google Vertex AI 的生成式 AI 评估文档（`evaluation-overview`、`determine-eval`，2026-09-12 抓取）中**没有** position bias / order bias / swap 顺序之类的表述（✅ 本人：对两页全文正则检索 `position bias|order bias|positional bias|swap|self-preference`，命中 0）。LangSmith 的 `llm-as-judge` 指南（<https://docs.langchain.com/langsmith/llm-as-judge.md>）也未提及裁判偏见。⇒ **「平台官方文档中关于裁判偏见的缓解说明」并未找到**（见 §8）。

---

## 7. 三家的关键差异（不取平均）

| 维度 | AutoGen 0.2 | AutoGen 0.4+ | CrewAI v1.15.21 | LangGraph 1.2.11 |
| --- | --- | --- | --- | --- |
| 「轮」的抽象 | `max_round` 循环 | Team 循环 + `max_turns` | **无「轮」**，只有 task 序列 | **无「轮」**，只有 super-step / 图环 |
| 默认是否限轮 | 是（10） | **否**（`max_turns=None`，条件为 None 时"run indefinitely"） | 不适用（task 有限集合） | 是（`recursion_limit`，文档 1000） |
| 谁决定发言 | auto/manual/random/round_robin/自定义函数 | 因 Team 而异（轮转 / 模型 / handoff / ledger / 图） | 任务顺序（sequential）或 manager（hierarchical） | 图边 + 路由函数（用户写） |
| 自定义发言函数返回 None | **终止对话** | **退回模型选择**（不再等于终止） | 不适用 | 不适用 |
| 终止条件的表达 | 3~4 个独立旋钮 | 一等对象 `TerminationCondition`（可 AND/OR 组合） | 无统一对象；guardrail / human_input / max_iter 分散 | `recursion_limit` + 用户节点里的 return END / interrupt |
| 内建分歧检测 | 无 | 无 | 无 | 无 |
| 上下文膨胀 | transforms（外挂）+ 实验性 clear history | **可配置消息视图**（Buffered / TokenLimited / HeadAndTail），默认不裁剪 | **自动摘要**（`respect_context_window=True`） | **不自动处理**；`SummarizationMiddleware` / `pre_model_hook` 需显式配置 |
| 人类介入暂停 | `human_input_mode` | `HandoffTermination(target="user")` | Task `human_input`；Flow `@human_feedback`（≥1.8.0） | `interrupt()` + checkpointer + `Command(resume=)` |
| 裁判/合成归属 | 无归属约定 | 一般 Team 无；Magentic-One 由 Orchestrator 写最终答案 | 最后一个 task 的输出 | 无；Router 模式画了独立 Synthesize 节点 |

---

## 8. 明确**未能**从一手来源验证的清单

1. **LangGraph 默认 `recursion_limit` 的确切值**：官方文档写 "Starting in version 1.0.6, the default recursion limit is set to 1000 steps"，而 `main` 分支 `_config.py` 的常量字面量是 `"10007"`（可被 `LANGGRAPH_DEFAULT_RECURSION_LIMIT` 覆盖），相关 commit 的 diff 又是 `"10000"`。**文档与代码互相矛盾**，本文只如实并列，不裁定。
2. **`langgraph-swarm` 是否记录过任何终止语义**：README / reference / 源码中均未发现 max-turn 或 FINISH 约定；「终止」只能靠 active agent 不再 handoff 这种隐含行为——**框架未明说**。
3. **平台官方文档中关于裁判偏见的缓解说明**：Vertex AI 生成式 AI 评估文档（`evaluation-overview`、`determine-eval`）全文检索 `position bias / order bias / swap / self-preference` **命中 0**；LangSmith `llm-as-judge` 指南也未提及偏见。⇒ **「某平台官方文档明确写如何缓解裁判位置偏见」这一点未找到**（只找到论文层面的 swap/few-shot/CoT 缓解与 Anthropic 的单裁判 rubric 实践）。
4. **CrewAI `Agent.max_iter` 的默认值**：官方文档表与正文都写 "Default is 20"，但我**没有**在本次核查中打开 CrewAI 的 Python 源码逐字确认（0.2 时期的历史值为 25，未在本版本核实）。以**文档值 20** 为准陈述。
5. **AutoGen `stable` 文档站对应的确切版本号**：文档 footer 只写 "© Copyright 2024, Microsoft"，页面内没有可见的版本号字符串；本文以「`.stable` 文档站（抓取于 2026-09-12）+ PyPI 当日最新 0.7.5」并列标注，**不能断言 `.stable` 就等于 0.7.5**。
6. **AutoGen 0.2 `max_round` 的精确计数单位**：源码循环 `for i in range(groupchat.max_round)` 中每轮包含「广播 + 选 speaker + 一次 `generate_reply`」，但一次 speaker 回复可能包含多条消息（含 tool call）。官方文档只写 "the maximum number of rounds"，**未逐字定义一轮是否等于一次 `generate_reply`**。
7. **`langgraph-supervisor` / `langgraph-swarm` 的 GitHub Release 说明原文**：委派子 agent 报告 GitHub API 返回 403（rate limit），**未能**取得 release notes；`langgraph 0.2.57` 这一版本号是通过官方 in-repo 概念文档的原文确认的，不是通过 release notes。
8. **本票要求的「在 issue 里留指针」**：本次任务的写入范围被限定为**仅** `research/03-...md` 这一个文件，因此**未修改** `issues/22-research-orchestration-frameworks.md`；指针需由上层处理。

---

## 9. 主要来源索引

**AutoGen**
- 0.2 源码：<https://raw.githubusercontent.com/microsoft/autogen/0.2/autogen/agentchat/groupchat.py>、<https://raw.githubusercontent.com/microsoft/autogen/0.2/autogen/agentchat/conversable_agent.py>
- 0.2 文档：<https://microsoft.github.io/autogen/0.2/docs/reference/agentchat/groupchat/>、<https://microsoft.github.io/autogen/0.2/docs/topics/groupchat/customized_speaker_selection/>、<https://microsoft.github.io/autogen/0.2/docs/tutorial/chat-termination>
- 0.4+ 文档：<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/tutorial/termination.html>、<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/selector-group-chat.html>、<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/swarm.html>、<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/magentic-one.html>、<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/graph-flow.html>、<https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/migration-guide.html>、<https://microsoft.github.io/autogen/stable/reference/python/autogen_agentchat.conditions.html>
- 0.4+ 源码：<https://raw.githubusercontent.com/microsoft/autogen/main/python/packages/autogen-agentchat/src/autogen_agentchat/base/_termination.py>、`teams/_group_chat/_selector_group_chat.py`、`teams/_group_chat/_base_group_chat_manager.py`、`teams/_group_chat/_magentic_one/_magentic_one_orchestrator.py`、`teams/_group_chat/_magentic_one/_magentic_one_group_chat.py`、`agents/_assistant_agent.py`、<https://raw.githubusercontent.com/microsoft/autogen/main/python/packages/autogen-core/src/autogen_core/model_context/__init__.py>
- PyPI：<https://pypi.org/pypi/autogen-agentchat/json>

**CrewAI**
- <https://docs.crewai.com/en/concepts/processes.md>、<https://docs.crewai.com/en/concepts/crews.md>、<https://docs.crewai.com/en/concepts/tasks.md>、<https://docs.crewai.com/en/concepts/agents.md>、<https://docs.crewai.com/en/concepts/flows.md>

**LangGraph / LangChain**
- <https://docs.langchain.com/oss/python/langgraph/graph-api>（`.md` 版）、<https://docs.langchain.com/oss/python/langgraph/interrupts>、<https://docs.langchain.com/oss/python/langgraph/checkpointers>、<https://docs.langchain.com/oss/python/langgraph/workflows-agents>
- <https://docs.langchain.com/oss/python/langchain/multi-agent>、`.../multi-agent/handoffs`、`.../multi-agent/router`、`.../multi-agent/subagents`、<https://docs.langchain.com/oss/python/langchain/supervisor>、<https://docs.langchain.com/oss/python/langchain/middleware/built-in>、<https://docs.langchain.com/oss/python/langchain/short-term-memory>
- <https://github.com/langchain-ai/langgraph-supervisor-py>、<https://github.com/langchain-ai/langgraph-swarm-py>
- <https://docs.langchain.com/oss/python/migrate/langgraph-supervisor>

**MetaGPT**
- <https://raw.githubusercontent.com/FoundationAgents/MetaGPT/main/metagpt/team.py>、<https://raw.githubusercontent.com/FoundationAgents/MetaGPT/main/metagpt/roles/role.py>、<https://raw.githubusercontent.com/FoundationAgents/MetaGPT/main/metagpt/utils/cost_manager.py>

**OpenAI Agents SDK**
- <https://openai.github.io/openai-agents-python/running_agents/>、<https://openai.github.io/openai-agents-python/handoffs/>、<https://raw.githubusercontent.com/openai/openai-agents-python/main/src/agents/run_config.py>、<https://raw.githubusercontent.com/openai/openai-agents-python/main/src/agents/run.py>

**裁判/合成**
- <https://arxiv.org/abs/2306.05685>（MT-Bench / LLM-as-a-judge）、<https://www.anthropic.com/engineering/multi-agent-research-system>、<https://docs.langchain.com/langsmith/llm-as-judge.md>
