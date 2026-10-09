# 衡的数据面：对齐 DSH 需要的数据有没有、缺的怎么拿

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

要把 DSH 的轨迹能力逐条对齐到衡的轨迹页上（[图](../map.md) 冻结项 2、3），第一件事是知道
**衡手里到底有哪些数据**：哪一档是「已经在流上、只是没画」，哪一档是「根本没有、要新增」，
以及后者各自的代价。没有这份底账，[时间轴与计时](06-grilling-timeline.md) 与
[检视器的面与内容](08-grilling-inspector.md) 就只能在猜。

**这是只读调研，不改任何代码。** 结论一律给 `文件:行号`。对齐矩阵见 [`../parity.md`](../parity.md)。

要清点八件事：

1. **每块的时间事实**。`Event` 信封上的 `at`（`src/events.rs`）覆盖了哪些块；`Block` 层
   （`src/render/transcript.rs`）在把事件变成块时**丢掉了哪些时刻**；`Painted::Block/Thought/
   Thinking`（`src/render/tui.rs`）手里留着什么。要一张「块 → 它的开始时刻 / 结束时刻 / 耗时有
   没有」的表。
2. **TTFT / 生成时长 / 吞吐**。增量文本（`Delta`）从 provider 到渲染器走哪条路，为什么**不进
   事件流**；路上有没有任何一处知道「第一个 token 是什么时候到的」；要拿到它需要新增什么
   （`MessageCompleted` 加一个可选字段？新增一条事件？只在渲染层记？），以及**每一种做法的
   兼容代价**：`SessionStarted.schema_version`（`src/events.rs:342`）今天怎么用、有没有任何按
   版本分派的读取逻辑、serde 可选字段能不能让老流照读、`sessions show` / `sessions replay` /
   `--continue` 重放会不会受影响。
3. **「迭代」的边界**。`TurnStarted { iteration }` 与 `TurnEnded` 之间到底是什么（一次模型调用
   还是多次）；一次发言跨多次调用时，屏幕上那条 `合计`（ADR 0016）是在哪里算的；能不能从流上
   复原「第 N 次迭代从哪到哪、花了多久」。
4. **用量**。`UsageRecorded` 的字段全集（输入 / 缓存 / 推理，见 `src/events.rs:188` 的 `Usage`）；
   它的到达时刻与 `TurnStarted` 的先后；会话累计在哪算、渲染层有没有第二份。
5. **工具**。`ToolBlock` / `ToolOutcome`（`src/render/transcript.rs:159`）今天带着什么；
   `duration_ms` 为什么没进 `DetailKind::Tool`（`src/render/tui.rs:8485`）；工具输出落盘
   （`outputs/<id>.txt`）的截断规则与读取路径；输出本来是结构化 JSON 时，流上保留的是不是
   结构化（还是已经变成字符串）。
6. **注入与压缩**。`ContextInjected` 的 `ContextSource` 全部分支与正文长度；`HistorySuperseded`
   （`HistoryReason::Compaction`）在轨迹页里今天是哪一块、正文是什么；两次相邻注入能不能做
   一次差异（DSH 的「差异」面）。
7. **`painted` 与行链接**。`painted` 每一条带着什么（块、`at`、尾巴 `Tail`）；`trace_links`
   的记法；一块与它画出来的行之间怎么对应（宽度变化重放之后还成立吗）。
8. **结论表**：`parity.md` 里每一条「补齐 / 变形做」需要的数据 → 衡有没有 → 缺的怎么拿
   （不动流 / 动流 / 只在渲染层记）→ 代价与风险。**这一节是本票的产物主体。**

## 产物

`research/01-data-surface.md`（本目录下）。写完在票底给 `## 作答`，并把结论里会影响设计的
那几条一句话记进票底（供[时间轴与计时](06-grilling-timeline.md) 与
[检视器的面与内容](08-grilling-inspector.md) 直接引用）。

## 作答

**在手（不动流）**：块级绝对时刻（事件信封 `at`，随 `Painted` 记住）；模型调用跨度可推
（`TurnStarted.at` → 下一个同 speaker 边界）；工具 `duration_ms`（调用墙钟，含钩子与排队）；
用量五个桶 + 渲染层会话累计（`Panel.total`，无 getter）；注入全文 / 消息原文 / 结构化 `args`；
压缩的 `summary` 全文。
**没有**：TTFT、生成时长、吞吐 —— 首 token 时刻在 provider 与 agent 层不存在，渲染层拿到即丢
（`src/render/tui.rs:8004`）；parity 的「输入」泳道同样没有数据源，别用 `ContextInjected.at` 冒充。
**动流只有一种划算做法**：给 `MessageCompleted` 加一个可选「距调用开始的毫秒」——一个字段推
出三个数；可选字段老流老码双向兼容，涨不涨 `SCHEMA_VERSION` 在运行时无差别（没有按版本分派的
读取逻辑）但要一条 ADR；**新增事件变体是另一档**：老二进制 `read_events` 会拒读整条流
（`src/events.rs:1083-1092`）。
**通用结论**：时刻停在块级，「块 → 它画出来的源行」今天没有记账 —— 折叠 / 行选择 / 区间聚焦
都要在渲染层补这一层。
产物：[`research/01-data-surface.md`](../research/01-data-surface.md)。
