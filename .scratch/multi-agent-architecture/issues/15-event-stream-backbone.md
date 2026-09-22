# 事件流骨架与事件 schema

Type: grilling
Status: resolved
Blocked by: 01, 03

## Question

**本票是新的，也是本图的承重墙。** 新目的地要求一个**共享只追加事件流**作为唯一真相源，而同一个机制要同时服务四件事：

1. **多 agent 讨论的介质**（每个 agent 的 `messages` 是它对该流的一次投影）
2. **hook 的挂载点**（`PreToolUse` / `PostToolUse`，见票 05）
3. **渲染来源**（plain / TUI / headless 三个消费者，见票 13）
4. **持久化的载体**（会话恢复与 transcript，见票 07）

**一个机制解四件事是本图最值得深挖的地方**——但前提是它的 schema 设计得对。选错了，这四处会各自长出一套事件类型。

要回答：

1. **事件的形状与枚举**。一次会话要发出哪些事件？至少：模型文本增量、工具调用开始/结束、权限询问与裁决、token/费用更新、发言归属、轮次边界、讨论分歧标记、子 agent 生命周期、循环终止原因。
2. **一条总线还是多条？** 这是本票与票 05、13 的交界：
   - 合成一条：hook 消费者会看到渲染细节与讨论内部状态，扩展面被污染
   - 合成多条：出现多套事件类型，且跨总线的顺序一致性要额外保证
   必须显式做出取舍，并说明理由。
3. **归属与可见性**：每条事件带不带 `speaker`？带不带可见性标记（"这条只给某个 agent 看/只给渲染看"）？——投影（票 17）与可观测性（票 19）都依赖这个答案。
4. **只追加与不变性**：事件一旦写入是否永不修改？"重新生成""撤销一轮讨论"这类操作如何表达——追加补偿事件，还是允许截断？这与票 07 的落盘策略直接相连。
5. **持久化形态**：JSONL 追加写（每家 LLM 的对话形状不同，但你自己的流形状是你定的）。注意 `std` 的 `OpenOptions::append` **不保证**跨线程追加不交错（map 的 Notes）——并发写多 agent 事件时这是硬约束。
6. **终止原因的表达**：综述推荐照抄 Cline 的显式枚举（`completed | max_iterations | aborted | mistake_limit | error`），比布尔"完成没完成"信息量大得多，也让日志与 eval 能区分"正常收尾"与"撞墙退出"。多 agent 下还要加讨论专属的原因（共识达成 / 轮数用尽 / 无分歧可收敛）。

**票 06 交接来的事件类型需求（2026-09-12）**：注入的内容（`AGENTS.md` + skills 一行描述）**必须能表达成流上的一条事件**——否则投影（票 17）的 `project` 就不是"流 + 规则"的函数，AGENTS.md 改一次重算结果就变（票 06 第 5 节）。同理，**将来 compaction 产出的摘要也必须记成事件**（票 06 第 6 节），否则裁剪不再是纯函数。本票的枚举需要为此留位，并且这直接压到本票第 4 条（只追加与不变性）：摘要"替换了一段历史"是补偿事件还是别的形态。

**这条接缝定完之后，票 05、07、13、16、17、19 才有落脚点**——这也是它们全部阻塞在本票上的原因。

## Answer

**已定（2026-09-13，grilling 四轮，每问均与用户逐条确认）。** 事实来源：`research/05-event-schema-precedents.md`（票 24 的产物；OpenHands / Cline / Claude Code 一手对照）。

### 1. 粒度：日志记录「完成单元」，不记录增量（票面第 1 条的前提）

事件的原子单位是**完成单元**：一条 assistant 消息、一次工具调用、一条工具结果、一次权限裁决、一个轮次边界。**模型文本增量不进日志**——它是传输层细节，直接转给渲染。

- **判据（能救整个 schema 的一句话）：日志记录「发生了什么」，不记录「看起来怎么样」。** 渲染细节（光标、spinner、逐字动画）与讨论中间态一旦进 schema，hook 消费者与持久化格式就会被 UI 牵着走。
- 若流中途失败，**已到达的文本随该单元的失败事件一起落盘**，不丢半截。
- 先例印证：OpenHands 的 `StreamingDeltaEvent` docstring 逐字写着 "**Not persisted to the conversation event log** … deltas are a UX affordance, not part of the durable conversation record"。
- 代价：`--continue` 重放不出"打字过程"——不需要。

### 2. 信封（`Event`）

```rust
struct Event {
    seq: u64,               // 物理序 = JSONL 行号；规范身份
    at: Timestamp,          // 墙上时间，只作诊断，不参与任何逻辑
    speaker_id: SpeakerId,  // 必填
    payload: EventPayload,  // 判别 tag（= 下面说的「kind」）
}

enum SpeakerId { Debater(AgentId), Executor(AgentId), User, System }
```

- **`kind` 不是信封上独立的一个字段**，而是 `EventPayload` 的判别 tag（serde 内部标注）——OpenHands 的 `kind` 同样是 computed field（返回类名）。需要一个名字时用 `payload.kind()`。
- **一个标识符**：`seq` 既是物理位置（行号）也是规范身份。**不引入 ULID / EventId**——两个 id 会立刻引出"谁是 canonical"的经典 bug，而 JSONL 行号免费且稠密，直接充当 `HistorySuperseded.targets`、`trim` 的区间、`--resume` 的断点。
- **不带 `parent`**：v1 日志是**线性**的，物理序 = 逻辑序。分支 / fork 由票 07 用「**新会话 + 复制前缀**」表达（Claude Code 的 `--fork-session` 形状），不在一个文件里存多分支。OpenHands 的树形（`parent_id` + `navigate_to`）是为"任意点导航 + 多前端"付的复杂性，我们单进程单前端不需要。
- **不带任何可见性字段**（票面第 3 条的后半）：可见性**不是事件的属性，是投影规则的输出**。
  - 讨论第 1 轮的"互不可见"是**时间性**的：A 被调用时，B 的发言事件根本还不存在，不需要任何标记。
  - 揭示轮的"都能看见"由协议位置决定。
  - 执行者的可见性由「按 `speaker_id` + `kind` 过滤」算出，不需要事件自带声明。
  - **反过来才是要害**：把可见性写进事件，就等于**让事件携带协议状态**——协议一改（例如从"只在冲突处开第二轮"改成"每轮都开"），历史事件的语义就跟着变，`project(log, rules)` 不再是纯函数。这正撞票 06 的核心约束。
- `speaker_id` **必填、不是 `Option`**：
  - harness 自身产生的事件（`SessionStarted` / `SessionEnded` / `HistorySuperseded` / 策略驱动的 `PermissionDecided` / `HookExecuted`）挂 `System`；"谁都不属于"这个态在投影里不存在，写成 `Option` 只会变成 `match` 里到处出现的 `None` 臂。
  - 用 `Debater` / `Executor` 两个变体，**不用泛化的 `Agent`**：`CONTEXT.md` 已把两者定成类型名并明确禁止拿 `agent` 当类型名；且投影对二者的规则确实不同（执行者的过程默认不进讨论者上下文）。
  - **不引入第二条 `source` 轴**（OpenHands 的 `source: agent|user|environment|hook`）：它需要那条轴是因为它没有多 agent 身份需求；在我们这里"谁产生的"与"归属于谁"总是同一个答案。
  - **`ContextInjected` 挂 `User`**：票 06 已定注入内容以**第一条 user 消息**的身份进投影，归属跟着投影走，就不需要特例规则。
- 两条纪律：**身份（system prompt）不进流，永远私有**（Chart 期已定）；**归属永远来自事件本身，绝不从 provider 的 `role` 反推**（照抄 OpenHands 的明文警告 "Do not infer event origin from LLM role."）。

### 3. 一条总线 + 一个 payload 枚举（票面第 2 条）

**一条** append-only 流：一个信封 + 一个 payload 枚举；消费者靠**过滤**取自己那一份，**不物理分总线**。

- ① **顺序一致性免费**——多总线的代价正是"跨总线顺序要额外保证"，而讨论发言、工具执行、权限询问三者的相对顺序恰恰是最要紧的。
- ② **投影本来就是「流 + 规则」**（票 17），过滤是同一个机制，不需要第二套。
- ③ **fork / rewind 要求统一编号的事件序列**（OpenHands `fork(from_event_id=…)`、Claude Code `--fork-session` 都建立在这上面）。
- "hook 面被渲染细节污染"的担忧**用 schema 纪律解决**（第 1 节的判据），再加**对 hook 只暴露一个封闭的公开子集类型**（第 6 节）。

**payload 枚举（20 个变体，按「谁在跑」分层）**：

```rust
EventPayload =
  // —— 会话骨架 ——
  SessionStarted    { session_id, cwd, schema_version }
  ContextInjected   { source: AgentsMd | SkillsCatalog | PlanMode, content }   // ← 票 06 / 票 10 交接
  SessionEnded      { reason: StopReason }

  // —— 讨论协议（票 16 定「何时发」，本票只给槽位）——
  RoundStarted      { round, mode: Independent | Targeted | Synthesis }
  RoundEnded        { round, reason: StopReason }
  DivergenceRecorded{ round, topic, positions }                     // ← 分歧标记

  // —— 单个 agent 的一次 Turn（票 02/03 的循环）——
  TurnStarted       { agent, iteration }
  MessageCompleted  { role, text, reasoning? }                      // ← 完成单元
  ToolCallStarted   { tool_call_id, tool_name, args }
  ToolCallCompleted { tool_call_id, ok, output | error, duration_ms }
  UsageRecorded     { usage }                                       // ← 票 02 的 Usage
  TurnEnded         { reason: StopReason }

  // —— 权限（票 05 / 10 / 20）——
  PermissionAsked   { request_id, tool_call_id, request }
  PermissionDecided { request_id, decision, source: User | Hook | Policy, reason? }

  // —— hook（票 05）——
  HookExecuted      { point, command, outcome }

  // —— 子 agent（票 14）——
  ExecutorSpawned   { executor_id, parent, brief }
  ExecutorFinished  { executor_id, reason: StopReason, summary }

  // —— 错误：分两种，因为可见性不同 ——
  AgentError        { message, recoverable }   // 模型要看到并纠正
  SessionError      { code, detail }           // 会话级运行失败，模型看不到

  // —— 历史操作（第 4 节）——
  HistorySuperseded { targets: [Seq], reason: Regenerate | Undo | Compaction, summary? }
```

- **`HistorySuperseded` 一条事件干三件事**：重新生成、撤销、compaction（compaction 就是 `reason: Compaction` + 带 `summary`）。OpenHands 拆成 `Condensation` + `CondensationSummaryEvent` 两条；一条变体更少活动件。
- **边界**：`RoundStarted` / `RoundEnded` / `DivergenceRecorded` 只是**槽位**——"何时开第二轮""什么算分歧"是票 16 的事，本票**不定协议**。
- `HookExecuted` 的字段照抄 OpenHands（`hook_event_type` / `command` / `success` / `blocked` / `exit_code` / `stdout` / `stderr` / `reason`）——已被一个真实实现磨过。
- `UsageRecorded` 携带 provider 的 usage 原样（票 02 归一后的形状）；**累计与费用上限是票 18 的事**。

**四处收口（2026-09-13，审计后补；都不新增变体）**：

- **合成器的产出 = 一条 `speaker_id = System` 的 `MessageCompleted`**（票 16 定了"合成 = 一次独立单发调用"，但枚举里没有它的家——这是审计找出来的缺口）。不新增变体：它不是 agent、不参与轮次，`System` 正好是"harness 自己产生的事件"那一档（第 2 节）。渲染侧把它当**最终产物**（票 13），投影侧它就是一条"他人的发言"。
- **`ContextInjected` 的 `source` 加 `PlanMode`**（上面枚举里已改）：进入 plan 模式时注入的那条短指令（票 10 第 4 节）此前没有取值可用。
- **讨论里"某一方本轮缺席"不需要 `RoundEnded` 的新字段**：那次调用失败会以**该 agent 自己的 `TurnEnded { reason: Error }`** 落流（`speaker_id` 就在信封上，票 02 已定失败的 Turn 以 `Error` 收尾）。所以查询口径是"这一轮里有没有 `TurnEnded{Error}` 且没有对应的 `MessageCompleted`"——**票 16 里"在 `RoundEnded` 上标出来"这句要按本条理解**。
- **字段名是 `PermissionDecided.reason`**，不是 `.why`（上面枚举即定义）。票 05 里那个 `why` 是**裁决**上的诊断字段（"外加一个 `why` 供诊断与展示"），不是事件字段——散见的 `.why` 是笔误。
- 被否掉的替代：粗粒度六类 + 万能 `Notice { notice_type, reason }`（Cline 形状）。它把权限裁决、hook 结果、协议边界压进一个 notice，**消费者又得靠 `reason` 字符串分支**——等于丢掉类型安全，而 hook 恰恰是最需要稳定类型面的消费者。

### 4. 只追加与不变性（票面第 4 条）

**永不修改、永不截断。** "重新生成 / 撤销一轮 / compaction"一律表达为追加一条补偿事件 `HistorySuperseded { targets, reason, summary? }`，由**投影规则**把被替代的区间排除。

- 这不是权宜，是这套架构的直接红利：`project(log, rules)` 已是纯函数，所以"让某区间不生效" = **规则 + 一条新事件**，历史一条不动。
- **票 06 交接来的两件事因此不需要另发明形态**：注入内容 = `ContextInjected`；将来 compaction 的摘要 = `HistorySuperseded { reason: Compaction, summary }`。
- 先例：OpenHands 的 `Condensation.forgotten_event_ids` + `apply()` 同样是"构造新列表（视图）、不删日志"。
- **fork 是另一个操作**（想同时保留两条路时才用），形态归票 07。

### 5. 持久化形态与写纪律（票面第 5 条）

- **一个会话一个文件，一行一条事件（JSONL）**；`seq` = 行号；`--resume` 的断点 = 最后一行。
- **单一写入者**：`EventLog` 的 writer **独占文件句柄**，其余一切（渲染、hook、别的 agent）通过 channel 送事件。这**直接绕开** research 08 那条硬约束（`std` 的 `OpenOptions::append` **不保证**跨线程追加不交错）——不用锁，也不赌 `O_APPEND` 的运气。同时符合 `CONTEXT.md` 的"`Session` 是唯一持有可变状态的结构"。
- **每条 flush，不每条 fsync**：崩溃时最坏丢尾部若干条；读取时**丢弃最后一行不完整的记录**（JSONL 的天然容错）。综述的判据同此（"格式本身不重要，**每条消息立即落盘**才重要"）。
- **版本记在 `SessionStarted { schema_version }` 上，且不承诺跨版本兼容**：版本对不上时**明确报错并拒绝误读**（允许"另存为新会话"），**不做迁移**。理由：Claude Code 的 JSONL 官方就说 "entry format is internal, changes between versions"——自用工具照此。
- **落盘布局 / 命名 / 轮转 / `--continue` 的选择器归票 07**；本票只定「编码 + 写纪律」。

### 6. 可见性的两分：LLM 可见 与 hook 公开

- **哪些事件进模型上下文** = **投影侧的一张穷尽 match 表**（写在 `project()` 旁边，**无 `_` 臂**）。**不放在 payload 上**：`CONTEXT.md` 已把 `Projection` 判给 provider 适配器侧，把这份知识塞进 `events` 会让 DAG 斜过来（`provider → events` 才是向下）。Rust 的穷尽 match 给出与类型属性同等的保证强度——新增变体时编译器同样逼你表态。
- **hook 公开子集**（一个封闭类型，**7 个变体**）：`SessionStarted`、`SessionEnded`、`ToolCallStarted`、`ToolCallCompleted`、`PermissionAsked`、`PermissionDecided`、`AgentError`。
  - 判据：票 05 已查清 Claude Code 的规则是"**hook 只能收紧、不能放松权限**"——hook 因此是一个**策略**机制，它的面就该是**策略点**，不是观察面。
  - **观察**讨论进度是票 19 的事：它有另一条路——读整条流。
  - **不进**：`RoundStarted` / `RoundEnded` / `DivergenceRecorded`（讨论协议是内部状态）、`UsageRecorded` / `HistorySuperseded` / `ContextInjected`（记账与投影内部）、`ExecutorSpawned` / `ExecutorFinished`。

### 7. 终止原因的表达（票面第 6 条）

照抄 Cline 的五类 + 讨论三类，**一个 `StopReason` 枚举被三层循环共用**：

```rust
enum StopReason {
    // turn loop（执行者 / 会话）
    Completed, MaxIterations, Aborted, MistakeLimit, Error,
    // round loop（讨论）
    Consensus, NoDivergence, RoundsExhausted,
}
```

- **它不是一条会话级的记录，而是挂在每个循环的结束事件上**：`TurnEnded` / `RoundEnded` / `SessionEnded`。嵌套循环（执行者的 turn loop 在讨论的 round loop 里）因此各自留下自己的原因，诊断力不丢。
- 执行者的 turn loop 只会产生 Cline 那五类。
- 值由 **harness 自己算**（票 02 已定：终止判断是"本轮没有工具调用 **且** 没有待处理的工具结果"，**不能信 provider 的 `stop_reason`**）——所以这个枚举是**我们的**，不受两家 provider 字段形状影响；`finish_reason` 只作诊断。
- 先例：Cline `AgentFinishReason = "completed" | "max_iterations" | "aborted" | "mistake_limit" | "error"`，挂在 `AgentDoneEvent.reason`。

### 明确不做

- 每条事件一个文件（OpenHands 的形状）、websocket 状态同步事件（`ConversationStateUpdateEvent`）、vLLM 的 token id 事件（`TokenEvent`）——都没有对应需求。
- 树形日志 / `navigate_to`（第 2 节）、多物理总线（第 3 节）、跨版本迁移（第 5 节）。

### 交接

- **票 05**：hook 公开子集就是第 6 节那 7 个变体；权限询问 / 裁决的事件名是 `PermissionAsked` / `PermissionDecided`。
- **票 07**：持久化按第 5 节（JSONL / `seq` = 行号 / 单一写入者 / 版本在 `SessionStarted`）；**fork = 新会话复制前缀**，树形不在本票。
- **票 13**：渲染消费者的事件来源 = `MessageCompleted` / `ToolCallStarted` / `ToolCallCompleted` / `UsageRecorded` + `StopReason`；**增量文本不经日志**，走传输层旁路（第 1 节）。
- **票 14**：执行者可见性的**机制**已定——`SpeakerId::Executor(AgentId)` + 投影规则过滤，**不需要事件带可见性标记**；只剩**策略**那一半。
- **票 17**：`speaker_id` 的域是第 2 节那个四变体枚举；**可见性是投影规则、不是字段**。
- **票 19**：事件流本身**就是**结构化日志（每条带 `seq` / `speaker_id` / `kind` / payload），所以"结构化日志"是免费的——本票的产出是**查询与索引**，不是新日志格式。

**票 13 交接来的答案（2026-09-13）**：你在第 2 节把消费者写成"靠过滤取用"，第 3 节又留了那个岔口（"订阅两个来源"还是"writer 落盘前广播"）——答案取了**后者**，并且加了一条你没写但必须如此的：**增量也投进同一个广播**。

- **形状**：`EventLog` 的 writer 在追加后**发布**到一条**组装期注入**的广播通道；`agent` 循环把 provider 的**增量**转发到**同一条**通道。渲染器只订阅一个来源。
- **为什么增量必须同通道**：分两条的话，增量与日志事件的**相对顺序就无定义了**——而渲染要的恰恰是"先看到文字流、再看到它落成一条 `MessageCompleted`"这个次序。这正是你第 3 节自己那条"跨总线顺序要额外保证"的论证，只是这次被用在增量 vs 持久事件之间。
- **一个实现后果**：writer 需要一个"**追加后发布**"的动作，而**通道是注入的**——所以 `events` 依然不认识 `render`（票 01 的 DAG 不斜，已交接票 01）。
- 另：渲染器**不直接读日志文件**（那要轮询 + 重复解析，且增量根本不在文件里），它只消费这条通道。

**票 18 交接来的一处枚举补充（2026-09-13）**：**`StopReason` 加第 9 个值 `BudgetExhausted`。**

- **加值是加法，不是改语义**：你第 7 节那个枚举是"一个枚举被三层循环共用"，加一个值**不影响任何既有值的含义**。
- **它为什么必须独立成一个值**：票 13 已要求"撞墙值要带上**撞的是哪个上限**"，而预算是一个确定的、用户可调的上限，与 `MaxIterations`（轮数）/ `RoundsExhausted`（讨论轮数）同类。用 `Aborted` 会让它变成垃圾桶（用户主动中断与花钱撞顶是两件完全不同的事），用 `Error` 语义错。
- **使用者是 `RoundEnded` 与 `SessionEnded`**：执行者不会因预算被拦——票 18 的硬停是"降级并收尾"（不再开新轮/不再派新执行者，**已在跑的让它跑完**）。
- 于是 `RoundEnded` 的讨论级取值从三个变成四个：`Consensus` / `NoDivergence` / `RoundsExhausted` / **`BudgetExhausted`**。
