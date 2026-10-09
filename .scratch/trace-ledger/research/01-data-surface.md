# 衡的数据面：对齐 DSH 需要的数据有没有、缺的怎么拿

**结论一句话**：衡的数据面是「**一条事件流 + 一层绘制记录**」——事件流是唯一真相源、每条事件都带
UTC 时刻，绘制记录（`Painted`）把块与它的时刻、尾巴成对记住；能算的数字全是**墙钟跨度类**（模型
调用跨度、工具耗时、回合跨度、时间轴区间），算不出来的只有**以「首个 token」为分母**的那一族
（TTFT、生成时长、吞吐），因为模型增量文本绕开事件流、首 token 的时刻从未被任何人留下。

因此这次对齐里，「结构」「可寻址」「每行耗时」「时间轴」「用量分拆」几乎全都能在**不动事件流**
的前提下兑现；唯一一处真的缺数据的是 TTFT 那一族，它要么只在渲染层记（重放没有），要么给
`MessageCompleted` 加一个可选字段（双向兼容，但要一条 ADR）。

下面逐条给依据，都是 `文件:行号`。凡文档没明写、由代码推出来的，正文会点明依据。

---

## 1. 每块的时间事实

**三个层次，时刻只在两个层次上存在。**

1. **事件层**：`Event { seq, at, speaker_id, payload }`（`src/events.rs:814-820`）；`at` 由
   `EventLog::append` 在追加那一刻盖上（`src/events.rs:1039-1054`）。这是**唯一的权威时钟** ——
   `--continue` 与宽度变化重放读到的都是当初那一刻。
2. **块层**：`render::transcript::Block`（`src/render/transcript.rs:26-155`）**没有任何时刻字段**。
   时刻由 `TuiState::apply` 从收到的 `RenderEvent` 上取一次（`event_at`，`src/render/tui.rs:8000-8009`），
   与块分开传下去。**同一个事件产生的多个块共享同一个 `at`**（`src/render/tui.rs:2169-2171`、
   `2330-2343`）。
3. **绘制记录层**：`Painted`（`src/render/tui.rs:1800-1823`）三条记录都带 `at`：
   `Block { block, at, tail }`、`Thinking { speaker, at, tail }`、`Thought { speaker, trace, at, tail }`。
   它记下来的唯一目的是宽度变化时整批重排（`src/render/tui.rs:2441-2475`）。

**块 → 开始 / 结束 / 耗时**

| 块 | 开始时刻 | 结束时刻 | 耗时 |
| --- | --- | --- | --- |
| `Block::Message` | **无**（正文首个增量的时刻没留下） | 有：`MessageCompleted.at`（`src/agent.rs:745-755`） | 无 |
| `Painted::Thinking` / `Thought`（思考行，不是普通块） | 实时 = **首个推理增量到达那一刻**（`event_at` 给 `Delta` 取 `Utc::now()`，`src/render/tui.rs:8004`；`open_thinking`，`src/render/tui.rs:2809-2825`）；重放 = `MessageCompleted.at`（`src/render/tui.rs:2952-2953` 明写这一点） | 同左（定稿那一刻） | **只有实时路径算得出**（首增量 → 定稿）；重放算不出 |
| `Block::Tool` | **无**（`ToolCallStarted.at` 在流上有，但它没进块） | 有：`ToolCallCompleted.at` | 有：`ToolOutcome.duration_ms`（`src/render/transcript.rs:169-174`） |
| `Block::Usage` | —（即时） | `UsageRecorded.at` | — |
| `Block::TurnStarted` / `TurnEnded` | 各自的 `at` | 同左 | 相邻边界相减就得到迭代 / 发言的跨度 |
| `ContextInjected` / `Sandbox` / `History` / `CommandRun` / `RoundStarted` / `RoundEnded` / 权限 / 钩子 / 执行者 / 错误 | 各自的 `at` | 同左 | — |
| `Block::Diagnostic` / `Notice` | **不是事件**：时刻在源头打上（`src/render/mod.rs:94-111`、`src/render/tui.rs:8005`） | — | — |

两条补充事实：

- **时刻戳是块首行的属性**：`stamp_lines` 只给第一条渲染行插入 `HH:MM:SS`，固定九列
  （`src/render/tui.rs:8028-8036`、`src/render/layout.rs:58`），对话视图完全不盖戳
  （只有轨迹分支调它，`src/render/tui.rs:2393-2421`）。文档把这条写成纪律：
  `docs/render.md:357-370`。
- **正文增量不产生绘制记录**：`Block::Delta`（`DeltaKind::Text`）只累进活尾巴
  （`src/render/tui.rs:2236-2256`），推理增量才开出思考行。所以「正在流的那段正文」既不在
  `painted` 里、也没有时刻。

## 2. TTFT / 生成时长 / 吞吐

**增量文本的路**（四跳，无一处带时刻）：

provider 适配器产出 `StreamEvent::TextDelta` / `ReasoningDelta`（`src/provider/mod.rs:113-132`）→
agent 循环转发 `render.text_delta` / `reasoning_delta`（`src/agent.rs:673-678`；合成器那条同形，
`src/agent.rs:1756-1760`）→ `RenderHandle` 把 `RenderEvent::Delta` 发进广播通道
（`src/render/mod.rs:166-180`）→ 渲染器 `live_event` / `apply`（`src/render/tui.rs:2682-2718`）→
转录出 `Block::Delta`（`src/render/transcript.rs:196-209`）。

**它为什么不进事件流**：文档与代码注释都写了 —— 增量绕过事件流、事后无法重推，所以转录只做
透传（`src/render/mod.rs:1-6`、`src/render/transcript.rs:10-11`、`docs/render.md:36-40`）。事件流
只收「完成的单位」。

**路上有没有任何一处知道「首个 token 是什么时候到的」**：

- provider 层：`StreamEvent` 一个时刻字段都没有（`src/provider/mod.rs:113-132`），适配器只把
  SSE 帧拆成完成单元（`src/provider/openai.rs:600-643`）。
- agent 循环：只把 delta 文本转发与累积，没有任何「这是第一次」的判断（`src/agent.rs:669-704`）。
- 渲染层：`event_at` 对 `Delta` 返回 `Utc::now()`，**「收到这条增量的时刻」在这里确实存在**
  （`src/render/tui.rs:8004`）—— 但它只喂给两处：思考行的 `at`（`open_thinking`）与活尾巴（不留）。
  正文的第一个增量不产生 `painted` 记录，于是那一刻**当场丢掉**。

**结论**：实时会话里「首个 token 时刻」在渲染层一瞬间存在过、没被记；重放路径（`--continue`、
宽度变化重排）上完全不存在。所以 TTFT 今天**拿不到**，不是「没画」而是「没留」。

**三种做法的代价**：

- **不动流**：TTFT / 生成时长 / 吞吐全拿不到（实时与重放都拿不到）。渲染层今天也没有把「首个
  增量的时刻」与「它属于哪次调用」对起来的记账。
- **只在渲染层记**：可行 —— 收到某次调用的第一个正文增量时记一次 `Utc::now()`，与那次调用的
  `TurnStarted.at`（就在 `Painted::Block { at }` 里）相减。代价是**重放没有**：`--continue` 与
  宽度变化重排都走不到增量事件，所以同一条流「实时看过有数字、重开就没有」，屏幕上会出现
  「有的会话有、有的没有」。宽度变化重排本身不会丢它（只要记在随 `TurnStarted`/`TurnEnded`
  重置的 state 上），丢的是跨进程的历史。
- **动流（`MessageCompleted` 加一个可选字段）**：一次字段足够推出三个数 —— 若字段是「距本次
  调用开始的毫秒」，则首 token 绝对时刻 = `TurnStarted.at + 它`，生成时长 =
  `MessageCompleted.at − 那个时刻`，吞吐 = `output_tokens / 生成时长`。产出点在主循环
  （`src/agent.rs:745-755`）、合成器（`src/agent.rs:1802`）与用户消息那一条（恒 `None`，
  `src/agent.rs:340-352`）。

**动流的兼容代价（逐项查过）**：

- `SessionStarted.schema_version` **只被写、从不被读**：写入在 `src/agent.rs:310`（常量在
  `src/events.rs:23`），唯一另一处命中是把它转发进钩子的封闭公开子集
  （`src/hooks.rs:196/237/241`）。`src/` 里**没有任何按版本分派的读取逻辑** —— 涨版本号在运行时
  没有任何效果。
- `read_events` 是 serde 直接反序列化每一行（`src/events.rs:1073-1095`）；`events` 里没有
  `deny_unknown_fields`（只有 `config` 用）。
- **加可选字段是双向兼容**：老流缺字段 → `None`（先例：`Usage.reasoning_tokens` 的
  `#[serde(default, skip_serializing_if = ...)]`，`src/events.rs:193-194`；`GoalStopped` 两个
  非 Option 字段用 `#[serde(default)]`，`src/events.rs:390/394`）；老二进制读新流会忽略未知字段。
- **加新事件变体是单向不兼容**：老二进制遇到未知变体反序列化失败，`read_events` 对非末行直接
  报 `InvalidData`（`src/events.rs:1083-1092`）→ `--continue`（`EventLog::open` → `read_events`，
  `src/events.rs:1005-1019`）与 `sessions show` / `sessions replay` 都会**拒读这条流**。所以
  「新增一条 TTFT 事件」比「加一个可选字段」贵得多。
- **编译面很小**：`MessageCompleted` 的读者里只有 `src/render/transcript.rs:349-359` 是穷尽列
  字段的匹配（要补 `..`）；其余都带 `..`（`src/provider/projection.rs:127`、
  `src/session/observe.rs:532`、`src/render/headless.rs:141`、`src/agent/replay.rs:224`、
  `src/lib.rs:751`、`src/context.rs:414`）。`redact` 那一处也带 `..`（`src/events.rs:569`）。
- **三处重放都不受影响**：`sessions show` 用 `observe::timeline`、不显示时刻
  （`src/session/observe.rs:357-`）；`sessions replay` 从流重建 `messages`（`src/cli.rs:3624-3650`）；
  `--continue` 把流原样推给渲染器（`console.replay(harness.events())`，`src/cli.rs:440`），
  思考行在重放里仍取 `MessageCompleted.at`（`src/render/tui.rs:2952-2953`）。

## 3. 「迭代」的边界

- `TurnStarted { agent, iteration }` 在**每次迭代开头**发一条（`src/agent.rs:586-597`）；
  `TurnEnded { reason }` 在 `end_turn` 里发，**每个回合只一条**（`src/agent.rs:2270-2281`）。
- 回合继续的条件是「最后一条助手消息要了工具」：`last_assistant_has_tool_calls`
  （`src/events.rs:901-921`）为真就 `continue`（`src/agent.rs:799-804`）。
- 所以 **`TurnStarted` 与 `TurnEnded` 之间是一个完整的发言（回合），不是一次模型调用**；一次
  发言里有 N 条 `TurnStarted`（N = 模型调用次数，受 `max_iterations` 限制，`src/agent.rs:516/577`），
  最后才是一条 `TurnEnded`。
- `TurnStarted.iteration` 每次发言**从 1 重数**（`src/agent.rs:521/586`）。它就是 DSH「每行 `#N`
  请求编号」的现成对应物；全局递增的编号要自己数。
- **`合计`（ADR 0016）在渲染层算**：`TurnStarted` 到达时 `turn_calls += 1`（`src/render/tui.rs:2224`）、
  `UsageRecorded` 到达时 `turn_usage.accumulate`（`src/render/tui.rs:2286-2288`）、`TurnEnded` 时
  **`turn_calls >= 2`** 才给 `Tail::Total(turn_usage)`（`src/render/tui.rs:2301-2306`），随后清零。
  它只变成那一行尾巴上的一段文本（`wording::total_tail`，`src/render/wording.rs:276-278`），
  **没有留下任何结构**。
- **能从流上复原「第 N 次迭代从哪到哪、花了多久」**：能 —— 按发言者分组后取
  `TurnStarted(iteration=k).at` 到下一个同 speaker 的 `TurnStarted` 或该回合 `TurnEnded` 的 `.at`。
  两点注意：讨论里两个发言者的事件交错，必须按 speaker 作用域切；`TurnEnded` 也出现在异常路径上
  （取消 551、预算用尽 567、`max_iterations` 578、钩子停回合 783 等，`src/agent.rs:544-795` 一共 13 处），
  所以边界不等于「一定有 `MessageCompleted`」。
- 一次迭代内的顺序（都可在代码里对上）：`TurnStarted`（`src/agent.rs:590`）→ 组装 + 预算检查
  （`src/agent.rs:601-632`）→ provider 流（`src/agent.rs:669-`）→ `UsageRecorded`
  （`src/agent.rs:698-704`）→ `MessageCompleted`（`src/agent.rs:745-755`）→ 工具调用
  （`src/agent.rs:767-796`）→ 下一次 `TurnStarted` 或 `TurnEnded`。

## 4. 用量

- **字段全集**：`Usage { input_tokens, output_tokens, cached_tokens, miss_tokens,
  reasoning_tokens: Option<u64> }`（`src/events.rs:187-195`）。`cached_tokens` / `miss_tokens` 是
  `input_tokens` 的一次**拆分**、不是额外量；推理 token 供应商本来就算在 `output_tokens` 里
  （`src/events.rs:198-219`）。所以可分的桶只有五档，DSH 的「其他 / 内容」在衡**没有对应字段**。
- **到达时刻**：`UsageRecorded` 在一次调用之内、`MessageCompleted` **之前**。依据：适配器的
  `terminate()` 把 usage 排在 `Finished` 之前（`src/provider/openai.rs:633-643`），而
  `MessageCompleted` 是在流循环结束（拿到 `[DONE]`）之后才 emit（`src/agent.rs:731-755`）。
  它是**可选的**：适配器没拿到 usage 对象（`self.usage` 为 `None`）时根本不发这条事件
  （`src/provider/openai.rs:481/499/573-580`）。`reasoning_tokens` 更细一层：只有供应商在
  `completion_tokens_details.reasoning_tokens` 里报了才有（`src/provider/openai.rs:836`）。
- **会话累计**：
  - 权威是**流上的派生值**：`events::total_usage`（`src/events.rs:930-951`）、按发言者的
    `events::usage_of`（`src/events.rs:938-940`）。
  - 渲染层有**第二份**：`Panel.total`（`src/render/panel.rs:29`），由 `Block::Usage` 增量累加，
    注释明写它与 `total_usage` 是同一套算法（`src/render/panel.rs:48-53`）。它只暴露
    `last_input()`（`src/render/panel.rs:61`），**没有暴露分桶**。
  - 渲染层还有**第三份**（每次发言的、瞬时的）：`turn_usage` / `turn_calls`
    （`src/render/tui.rs:804-805`，`TurnEnded` 后清零）。
  - `SessionFacts` 里**没有任何用量**（`src/render/tui.rs:324-370`），所以 TUI 手里没有整条流、
    也没有会话累计的可读入口。
- 归属：`UsageRecorded` 由说话的发言者 `emit`（`src/agent.rs:698-704`），所以按参与者分账是
  现成的（`usage_of`）。

## 5. 工具

- **块带着什么**：`ToolBlock { speaker, tool_call_id, tool, args: Value, outcome }`
  （`src/render/transcript.rs:158-165`）、`ToolOutcome { ok, output: Option<String>,
  error: Option<String>, duration_ms }`（`src/render/transcript.rs:168-174`）。**一个块，由结果绘制**：
  结果到达时才 push（`src/render/transcript.rs:304-331`）。
- **`duration_ms` 的口径**：从 `process_call` 里的 `Instant::now()`（`src/agent.rs:956`，在
  `hook.pre` 与权限门**之前**）到结果落流（`emit_completed`，`src/agent.rs:2135-2143`）。被推迟的
  `task` 调用把同一个 `started` 带进 `DeferredCall`（`src/agent.rs:826-833`、`1243`），所以
  **并发上限下的排队时间也算在里面** —— 组头摘要写「墙钟跨度」时要按这个口径说。
- **为什么没进 `DetailKind::Tool`**：那个变体的字段只有 `tool_call_id / output / error / args /
  no_result`（`src/render/tui.rs:8484-8493`）；构造点拿得到整个 `outcome`，只是没取那个字段
  （`src/render/tui.rs:8126-8139`）。所以「工具行耗时」是**只在渲染层补一个字段**的事。
- **落盘与截断**：`context::truncate_result` 在结果进流前跑（`src/agent.rs:2144-2165`）：估计 token
  超过 `max_tool_result_tokens`（默认 25 000，`src/config.rs:58`）就溢写到
  `<outputs_dir>/<tool_call_id>.txt`（`src/context.rs:351`；`OUTPUTS_DIR = "outputs"`，
  `src/session/store.rs:34`），流上只留**头尾各半**的预览加一句
  `[已截断：N 字符，约 M token；全文在 <path>]`（`src/context.rs:373-395`、`403`）。预览长度的算法：
  上限的 1/10 再乘 4 字符、最少 200 字符（`src/context.rs:39-45`、`376-378`）。写不出落盘文件、或
  「那句说明比正文还长」时不留全文（`src/context.rs:352-365`）。
- **读取路径**：详情覆盖层的 `read_tool_body`（`src/render/tui.rs:9016-9047`）—— 预览里没有截断
  标记就直接用预览；有标记才去 `session_dir/outputs/<tool_call_id>.txt` 读；读不出来或空文件降级
  成「预览 + 一句全文不可用」；读出来的正文再按 `DETAIL_MAX_CHARS = 200_000` 字符截
  （`src/render/tui.rs:8608`）。`session_dir` 来自注入的 `SessionFacts`（`src/render/tui.rs:327-329`）。
- **结构化还是字符串**：`ToolCallCompleted.output` 是 `Option<String>`（`src/events.rs:436-442`），
  即便内容是 JSON，流上留的也是**字符串**。参数相反：`ToolCallStarted.args` 是
  `serde_json::Value`（`src/events.rs:431-435`），进流就是结构化树。已知结果是 JSON 的：`ask_user_question`
  写 `serde_json::to_string(&answers)`（`src/tools/ask_user.rs:218-223`）、MCP 的工具结果原样转发
  服务端文本（`src/tools/mcp_call.rs:80-90`）。所以「结果走树」只能在渲染层试一次
  `serde_json::from_str`；被截断过的结果（含 `[已截断：`）**解析不了** —— 头尾拼接不是合法 JSON。
- **错误**：`ok=false` 时 `error: Some(预览)`、`output: None`（`src/agent.rs:2149-2165`）。**没有 code
  字段**：`ToolError` 只有 `Message(String)` 与 `InvalidatesReads { message, path }` 两档
  （`src/tools/tool.rs:48-56`），进流时只剩 `error.to_string()`（`src/agent.rs:2151`）。所以 DSH 的
  「行上给错误 code」要么动流、要么从文本里解析（脆弱，且 `InvalidatesReads` 的 `path` 连进文本
  都不一定在）。

## 6. 注入与压缩

- **`ContextSource` 的全部分支（十支）**：`AgentsMd`、`SkillsCatalog`、`McpCatalog`、`Skill`、
  `Identity`、`PlanMode`（已退场、只为老流保留）、`Persona(ParticipantId)`、`Goal`、`Compaction`、
  `Reminder`（`src/events.rs:223-267`）。`Identity` 与 `Persona` 各有一条特殊说明写在定义处。
- **正文长度**：`ContextInjected { source, content: String }`（`src/events.rs:344-347`），`content` 是
  **全文、无上限**（`record_context_injection`，`src/agent.rs:354-389`）。写入点：AGENTS.md 与技能
  清单（`src/lib.rs:391-405`）、用户载入的技能全文（`src/lib.rs:794-804`）、压缩摘要
  （`src/lib.rs:913`）、MCP 目录（`src/cli.rs:444-449`）、目标清单（`src/cli.rs:2306/2460`）、
  过半提醒（`src/cli.rs:2422-2428`）。
- **两次相邻注入能不能做差异**：**能** —— 两侧都是全文，渲染层拿得到（`Block::ContextInjected`
  把 `content` 一起带着，`src/render/transcript.rs:133-138`、`437-443`；详情摊开全文，
  `src/render/tui.rs:8460-8464`）。
- **例外是 `Identity`**：它**不进事件流**，是组装期推给渲染器的一条 `RenderEvent::Identity`
  （`src/cli.rs:456` → `src/lib.rs:1139-1141` → `src/render/mod.rs:112-119`），转录把它画成一条
  `ContextInjected { source: Identity, content: 当前拼法 }`（`src/render/transcript.rs:220-230`）。
  正文是**按当前代码拼的那一份**，所以「前后 system prompt 的差异」在流上只有**当前**这一份，
  历史那些根本不存在（`docs/render.md:342-352` 也这么写）。
- **压缩**：`HistorySuperseded { targets: Vec<u64>, reason, summary: Option<String> }`
  （`src/events.rs:487-492`）。`reason = Compaction` 由 `DiscussionHarness::compact` 发
  （`src/lib.rs:949-967`）：`targets` 是压缩前**整条流**的每个 seq，`summary` 是那次单发模型调用
  的产物；摘要随后作为 `ContextInjected { source: Compaction }` 注入新会话（`src/lib.rs:913`）。
  轨迹页上它今天就是**一块**：`Block::History { reason, summary }`（`src/render/transcript.rs:148-151`、
  `445-449`），正文由 `wording::history` 拼成 `[历史：压缩] <summary>`（`src/render/wording.rs:1250-1253`），
  画成一条**不可点开**的叙述行（没有 `link`）。
- 附带一条要点：`HistorySuperseded.targets` 是「哪些 seq 被退掉」的唯一权威
  （`superseded_seqs`，`src/events.rs:834-846`），压缩记录之外的 `Regenerate` / `Undo` 也走它。

## 7. `painted` 与行链接

- **`painted` 每条记录带着什么**（`src/render/tui.rs:1800-1823`）：块（或思考行）+ `at` + 可选尾巴
  （`Tail::Usage(Usage)` / `Tail::Total(Usage)`，`src/render/tui.rs:1773-1789`）。不产生行的块不留
  （`push_block` 只在 `produced > 0` 时 push，`src/render/tui.rs:2330-2344`）；正文增量从不进
  （它只累活尾巴）。它**不记**「这块画成了哪几条源行」。
- **`trace_links` 的记法**：`VecDeque<Option<Detail>>`，与**轨迹 pane 的源行一一对应**；裁法是
  按 `Pane::push` 报回来的「这次丢了几条」裁，不自己数 `CAP`（`src/render/tui.rs:915-920`、
  `2516-2527`；`Pane::push` 的返回数是裁剪的唯一权威，`src/render/pane.rs:75-84`）。
- **块 → 行 → 详情 的对应链**（三段，中间那段缺）：
  1. 块 → 源行：`emit_block` 里 `paint_block` 出一个 `Vec<RenderedLine>`，每条经 `push_line`
     成为**一条源行**（`src/render/tui.rs:2350-2431`、`2506-2530`）。这条对应**没有记账**，只有
     顺序（同一次 emit 推的源行连续）；
  2. 源行 → 显示行：`Pane.starts` 与每帧重建的 `Drawn.rows`（`src/render/pane.rs:33-35`、
     `140-151`；`src/render/tui.rs:1661-1670`、`6879-6885`）；
  3. 显示行 → 详情：`trace_link_at`（`src/render/tui.rs:4056-4063`）。
- **宽度变化重放之后还成立吗**：**成立** —— pane、`trace_links`、`turn_rail` 一起清掉，再按
  `painted` 整批重放（`src/render/tui.rs:2441-2475`、`2456-2460`），「平行表与源行窗口同进同出」
  是写在 `Pane::push` 注释里的契约（`src/render/pane.rs:75-79`）。**但**任何要按**块**折、选、
  过滤的新功能都得自己新加一层「块 ↔ 源行区间」的记账：`painted` 不存这个，`Detail` 也不存时刻
  （`Detail { title, color, kind }`，`src/render/tui.rs:8445-8455`）。
- 今天唯一的平行表是 `trace_links` 与 `turn_rail.lines`（`src/render/tui.rs:1672-1690`）。所以
  「源行 → 时刻」今天走不通：时刻在块级的 `painted` 里，而「块 → 源行」这一步没有记账。

## 8. 结论表

判定列按票面要求分三档：**不动流** / **动流** / **只在渲染层记**。表里每行都能直接照着做设计。

### A. 账本结构

| parity.md 里的一条需要的数据 | 衡有没有 | 缺的怎么拿 | 代价与风险 |
| --- | --- | --- | --- |
| 每行 `#N` 请求编号 | **有**：`TurnStarted.iteration`（`src/events.rs:422-425`，产出 `src/agent.rs:586-597`） | 不动流 | 它每次发言从 1 重数；要跨发言的全局编号得自己数 |
| 回合 → 迭代两级分组的边界 | **有**：`TurnStarted` / `TurnEnded`（`src/agent.rs:2270-2281`） | 不动流 | 讨论里两个发言者交错，切边界要按 speaker 作用域 |
| 组头摘要的墙钟跨度 | **有**：两个 `at` 相减 | 不动流 | 无 |
| 组头摘要的工具直方图 | **有**：`ToolCallStarted.tool_name`（`src/events.rs:431-435`） | 不动流 | 无 |
| 组头摘要的工具耗时 | **有**：`ToolCallCompleted.duration_ms` | 不动流 | 口径是「调用墙钟」（含钩子/权限/排队，`src/agent.rs:956`、`1243`），不是工具本体 |
| 失败行上的错误 **code** | **没有**：只有 `error: String`（`src/events.rs:436-442`） | 动流（把 `ToolError` 的结构带进 payload）或从文本解析 | 动流 = 形状变化，且 `ToolError` 今天只有两档（`src/tools/tool.rs:48-56`）；解析 = 脆弱 |
| 工具行 `name · args` → 结果摘要 | **有**：`args` 是结构化 `Value`、结果在 `output` / `error` | 不动流 | 被截断过的结果只能给头尾预览 |
| `Between turns` 区段（压缩记录） | **有**：`HistorySuperseded { reason: Compaction, summary, targets }` | 不动流 | 「不属于任何回合」要靠 seq 位置判；摘要全文在 `summary` 里 |

### B. 时间：每行的耗时与总览

| parity.md 里的一条需要的数据 | 衡有没有 | 缺的怎么拿 | 代价与风险 |
| --- | --- | --- | --- |
| 每行耗时（工具行） | **有**：`ToolOutcome.duration_ms`，只是没上屏也没进详情 | 不动流（渲染层补进 `DetailKind::Tool`） | 无 |
| 每行耗时（模型调用） | **有**（可推）：`TurnStarted.at` 到下一个边界 | 不动流 | 边界要按 speaker 取；跨度含极少量的组装与预算检查时间（`src/agent.rs:590-632`） |
| **TTFT** | **没有**：provider / agent / 事件流三层都没有首 token 时刻 | **动流**：`MessageCompleted` 加一个可选字段；或**只在渲染层记**（重放没有） | 动流：见第 2 节的兼容代价（可选字段双向兼容，要一条 ADR）；只在渲染层记：同一条流「实时有、重开没有」 |
| **生成时长** | **没有**（需首 token 时刻） | 同 TTFT | 同 TTFT |
| **吞吐** | **没有**（`output_tokens` 有，但缺「生成时长」这个分母） | 同 TTFT | 拿「调用总时长」当分母是编造（把等首 token 的时间算进生成），不要 |
| timing overview 的「输入」泳道 | **没有**：请求组装无事件、注入是瞬时的 | 要么不做这一泳道，要么拿「上一次调用结束 → 这一次调用开始」的间隙近似 | 用 `ContextInjected.at` 冒充输入耗时会说谎（它只发生在会话开场 / 载技能 / 跨阈值） |
| timing overview 的「模型」泳道 | **有**（调用跨度，可推） | 不动流 | 同「每行耗时（模型调用）」 |
| timing overview 的「工具」泳道 | **有**：`ToolCallStarted.at` / `ToolCallCompleted.at` + `duration_ms` | 不动流 | 并发的工具会重叠，泳道要能叠（`src/agent.rs:1212-1250`） |
| 流式中只画开始标记、不编造时长 | **已对齐**，当约束用（先例：思考行开始时不写完成时刻，`src/render/tui.rs:2840-2842`） | 不动流 | 无 |
| 开始时刻切「本地时间 / Unix 秒」（parity 判不做） | **有**（`at` 就是 UTC），但显示写死成本地时区 | 不动流 | `wording::stamp` 固定 `%H:%M:%S` 本地时区（`src/render/wording.rs:1893-1895`）；改它纯属措辞 |

### C. 检视器（详情覆盖层）

| parity.md 里的一条需要的数据 | 衡有没有 | 缺的怎么拿 | 代价与风险 |
| --- | --- | --- | --- |
| 分面切换本身 | 无数据依赖（`DetailKind` 七种，`src/render/tui.rs:8456-8493`） | 不动流 | 无 |
| 用量面：本次调用的分桶 | **有**：`Block::Usage { usage }` 的五个字段 | 不动流 | 推理桶常是 `None`（供应商不一定报，`src/provider/openai.rs:836`）；DSH 的「其他 / 内容」桶无对应字段 |
| 用量面：会话累计 | **有**：`Panel.total`（渲染层，无 getter）；权威在流上 `events::total_usage` | 不动流（给 `Panel` 加 getter） | 渲染器手里没有整条流，重算不了权威值；两份账要守同算法（`src/render/panel.rs:48-53`） |
| 一次发言跨多次调用的 `合计` | **有**（算过就丢）：`turn_usage` / `turn_calls`（`src/render/tui.rs:2224`、`2286-2288`、`2301-2306`） | 不动流（把它记到该回合的绘制记录上，或按边界重算） | 今天只变成尾巴文本（`src/render/wording.rs:276-278`）；重放路径重算要按 speaker 切边界 |
| 计时面：开始时刻 | **有**：块级 `at`（`Painted`） | 不动流 | 点开一条行时要找回它的 `at`：`Detail` 里没有时刻（`src/render/tui.rs:8445-8455`），且「块 ↔ 源行」没有记账（第 7 节） |
| 计时面：总时长 | 工具**有**；模型调用**可推** | 不动流 | 同 B 节 |
| 计时面：TTFT / 生成 / 吞吐 | 同 B 节 | 同 B 节 | 同 B 节 |
| 计时面的「计时来源」 | **没有**这个概念 | 不动流：写死成「事件信封的 `at`」 | 若 TTFT 走渲染层记，来源必须分开标，否则一处数字两个出处 |
| Raw / Summary 二分 | **有**：`Block::Message.text` 是原文（画出来的是 markdown 渲染） | 不动流 | 无 |
| 参数面 | **有**：`ToolCallStarted.args: Value`（结构化） | 不动流 | 无 |
| 结果面（JSON 走树 / 否则原文） | **半有**：`output` 是字符串 | 不动流：渲染层试 `serde_json::from_str`，失败退回原文 | 被截断过的结果解析不了（判据可复用 `TRUNCATED_MARKER`，`src/render/tui.rs:9021-9023`） |
| 错误首行标红 | **有**：`error` 文本 + `ok=false` | 不动流 | 没有 code（见 A 节末行） |
| Schema 面（工具参数 schema） | **半有**：工具描述在组装期就绪（`session.tools().specs()`，`src/agent.rs:630`），但**不进 TUI** | 不动流：把工具描述作为一种新的注入面传进去 | `SessionFacts` 今天没有它（`src/render/tui.rs:324-370`）；新注入面归 02 票查 |
| system prompt / 工具目录面 | **半有**：注入全文都有；system prompt 只有**当前**那一份（不进流） | 不动流（相邻同类注入可 diff） | 历史 system prompt **不存在**，别拿它假装「前后差异」 |
| 前后 prompt 的差异面 | **可算**：流上有投影所需的全部事件 | 不动流：用 `replay::replay` 从流重算每个请求的 `messages`（`sessions replay` 就是它，`src/cli.rs:3624-3650`） | 重算不便宜（要 caps 与模型路由）；但这是唯一诚实来源 |
| 层级跳转（跳到所属请求 / 上面那条用户消息） | **半有**：没有父指针，但有顺序与 `TurnStarted`/`TurnEnded` 边界 | 不动流（按边界推） | 「上面那条用户消息」已有现成查询形态（`src/lib.rs:743-757`） |

### D. 检索与折叠

| parity.md 里的一条需要的数据 | 衡有没有 | 缺的怎么拿 | 代价与风险 |
| --- | --- | --- | --- |
| 搜索所依据的文本 | **有**：各块的文本都在（`Block::Message.text` / `Tool.args·output·error` / 注入 `content`） | 不动流 | 增量流过的正文在 `MessageCompleted.text` 里，重放可得 |
| 折叠：整个回合折成一行摘要 | **有**：回合边界 + 步骤/工具计数可推 | 不动流 | 需要新增「块 ↔ 源行区间」的记账（第 7 节） |
| 折叠：assistant 下连续工具调用折成一行 | **有**：工具名与顺序 | 不动流 | 同上 |
| 命中后账本只剩匹配行 | **有**：与折叠同一套记账需求 | 不动流 | 同上 |
| 时间轴区间聚焦 → 未命中行淡出 | **有**：时刻在块级 | 不动流 | 「源行 → 时刻」缺一层记账（第 7 节） |
| 全折 / 全展、双击行 | 无数据依赖 | 不动流 | 无 |

### E. 长历史与导航

| parity.md 里的一条需要的数据 | 衡有没有 | 缺的怎么拿 | 代价与风险 |
| --- | --- | --- | --- |
| 行选择 | 无数据依赖，但行要有**稳定身份** | 不动流：在渲染层补一层行键 | `painted` 的记录里没有 id、也没有产生它的 `seq`（只有顺序）；折叠/过滤之后要靠稳定键找回 |
| 更早的历史从磁盘按需读 | **有**：`log.jsonl` 在盘上（`read_events`，`src/events.rs:1073-1095`）；`CAP = 20_000` 丢的只是内存里最旧的源行（`src/render/pane.rs:21`） | 不动流（渲染层或会话层开口子） | 与「事件流是唯一真相源」一致；要处理「读到的那段与已在内存里的那段拼接」 |
| 聚合条数 | **有**：`Pane::sources()` / `Pane::total()`（`src/render/pane.rs:298-306`） | 不动流 | 无 |
| 键盘可达、初始贴底 / 向上滚暂停跟随（变形做 / 已对齐） | 无数据依赖：键位、焦点与 `Follow` 都是视图本地状态 | 不动流 | 行选中要落地时得先有 E 节第一行的「稳定行键」 |

---

## 9. 给下游票的话

### 给 [时间轴与计时](../issues/06-grilling-timeline.md)（时间轴与计时）

硬事实，按优先级：

1. **TTFT / 生成时长 / 吞吐在流上不存在，在 provider 与 agent 层也不存在**（`StreamEvent` 没有
   任何时刻字段，`src/provider/mod.rs:113-132`）。渲染层里「首个增量到达那一刻」**一度存在**
   （`event_at` 对 `Delta` 取 `Utc::now()`，`src/render/tui.rs:8004`），但正文增量不产生绘制记录，
   当场丢掉。所以这不是「有数据没画」，是「没数据」。
2. **算得出来的**：模型调用跨度（`TurnStarted.at` 到下一个边界，按 speaker 切）、工具耗时
   （`duration_ms`，含钩子与排队）、全回合跨度、每个块的绝对开始时刻。**算不出来的**：一切以
   首 token 为分母的数。
3. **一个字段就够推三个数**：若给 `MessageCompleted` 加「距本次调用开始的毫秒」，则
   TTFT = 它、生成时长 = `MessageCompleted.at − (TurnStarted.at + 它)`、吞吐 =
   `output_tokens / 生成时长`。别为此新增一条**事件** —— 新变体是单向不兼容（老二进制
   `read_events` 会拒读整条流，`src/events.rs:1083-1092`）。
4. **涨不涨 `SCHEMA_VERSION` 在运行时没有区别**：`src/` 里没有任何按版本分派的读取逻辑，
   它只被写（`src/agent.rs:310`）与转发进钩子公开子集（`src/hooks.rs:196`）。兼容性完全由 serde
   形状决定：可选字段双向兼容，新变体单向不兼容。按 `src/events.rs:20-23` 的纪律「payload 形状
   一变它就涨」，涨是记号、不是机制；要不要为它写 ADR 是设计选择（先例：ADR 0009 为可选字段
   写过一段代价）。
5. **「输入」泳道没有数据源**：请求组装无事件、`ContextInjected` 只发生在会话开场 / 载技能 /
   跨阈值三类时刻。别用它的 `at` 冒充输入耗时。
6. **时间轴要按行着色 / 过滤时，缺一层记账**：时刻在块级（`Painted::Block.at`），而「块 →
   它画出来的源行」今天没有对应表（第 7 节）。这层记账不动流，但必须在设计里明写它落在哪
   （与 `trace_links` 平行的第四张表，还是并进它）。
7. `parity.md` B 节里「流式中不编造时长」这条纪律在衡已有先例（思考行开始时只写开始时刻），
   应当当约束沿用。

### 给 [检视器的面与内容](../issues/08-grilling-inspector.md)（检视器的面与内容）

硬事实：

1. **`Detail` 今天缺三个字段**：`duration_ms`（在 `ToolBlock.outcome` 里有）、块的 `at`、以及
   用量的分桶（在 `Block::Usage` 里有）。前两个都在构造 `Detail` 的那一刻手边（`src/render/tui.rs:8126-8139`），
   补它们是纯渲染层的事。
2. **用量面**：本次调用的五个桶在 `Block::Usage` 上；会话累计在 `Panel.total`（**没有 getter**，
   `src/render/panel.rs:29/61`），流上是 `events::total_usage`；`合计` 只在 `TurnEnded` 那行的尾巴
   文本里（`src/render/wording.rs:276-278`），ADR 0016 没有把它留成结构 —— 检视器要显示它就得
   按边界重算，或先把它记进绘制记录。
3. **Raw 面**：`Block::Message.text` 与 `ContextInjected.content` 都是原文（渲染是后一步），可以
   老实给原文；工具结果是「截断预览 + 落盘全文」两条路（`src/render/tui.rs:9016-9047`），
   **拿不到「当时那条没截断的结果」的第二种读法** —— 截断过的结果在流上只有头尾。
4. **参数面有结构化 `args`；结果面要在渲染层判** `serde_json::from_str`（`ask_user_question` 与
   MCP 的输出常常是 JSON 文本，`src/tools/ask_user.rs:220-223`）。截断过的结果别硬解析。
5. **Schema 面要一个新的注入面**：工具描述在组装期就有（`session.tools().specs()`），但 `SessionFacts`
   里没有它。这是「不动流、但要新增一条从组装进 TUI 的事实」——与 `speaker_order` / `number_style`
   同档（`src/render/tui.rs:324-370`）。
6. **差异面只能说一半**：同类相邻注入可以 diff（正文全文都在）；historical system prompt
   **根本不存在**（`Identity` 不进流，`src/cli.rs:456`）。要「前后 prompt 差异」只能从流重算
   `messages`（`replay::replay`）。
7. **层级跳转**没有父指针可用，只能按 `TurnStarted` / `TurnEnded` 边界推；`#N` 这一类编号用
   `TurnStarted.iteration`（每次发言从 1 重数）。
8. **错误没有 code**（只有文本，`src/events.rs:436-442`）；若要 DSH 那种「行上给 code」，那是
   动流，得先回[时间轴与计时](../issues/06-grilling-timeline.md) 那条口径上拍。
