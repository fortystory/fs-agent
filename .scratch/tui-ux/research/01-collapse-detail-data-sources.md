# research：折叠与详情层的数据来源与命中接缝

Part of: ../../map.md
Ticket: ../issues/01-research-collapse-detail-data-sources.md
Status: resolved（本文件只报事实，不推荐方案、不选赢家）

本文件的每一行事实都来自本仓库源码的现读现查。行号以本次阅读时的文件为准。冻结约束见 `../map.md` Notes（尤其第 5、6、7、8 条）。

---

## 来源清单

本机仓库（repo-relative）：

- `src/events.rs` —— 事件 schema、序列化、打码
- `src/agent.rs` —— 回合循环、流消费、`MessageCompleted` 写入点、工具结果截断调用
- `src/agent/history.rs` —— `/undo` 的 `.before` 读取
- `src/render/mod.rs` —— `RenderEvent` / `DeltaKind` / `RenderHandle`
- `src/render/transcript.rs` —— `Block` / `Transcript` 构造
- `src/render/tui.rs` —— `TuiState` / `apply` / `mouse` / 全部绘制
- `src/render/pane.rs` —— 转录缓冲与视口、wrap 缓存
- `src/render/plain.rs` —— plain 渲染器
- `src/render/headless.rs` —— headless 渲染器
- `src/render/panel.rs` —— 右栏计数（也消费 `Block`）
- `src/render/input.rs` —— `ConsolePort` / `ConsoleHandle` / `ConsoleAsker` / `ConsoleQuestions`
- `src/render/layout.rs` —— `Regions` 与各区矩形
- `src/session/observe.rs` —— `sessions show` 的 `Entry` 时间线
- `src/session/store.rs` —— 会话目录 / `outputs/` 常量
- `src/context.rs` —— `truncate_result` / `preview` / `SpilledResult`
- `src/provider/projection.rs` —— 从日志重建 provider messages
- `src/provider/openai.rs` —— 流 delta 的产生顺序
- `src/tools/file.rs` —— `.before` 命名
- `src/tools/edit.rs` —— `/undo` 语义
- `src/lib.rs` —— 组装：`outputs_dir`、`resume`、唯一写路径
- `src/cli.rs` —— assembly、`TuiOptions` / `SessionFacts` 的组装点
- `tests/render_layout.rs`、`tests/render_tui.rs` —— 现成的状态注入 + 帧快照测试

仓库内既有研究（本票直接引用，未重查 DSH 产物）：

- `.scratch/ask-user-question/research/01-dsh-ask-user-question-and-composer-takeover.md` §3

未离开仓库：本票没有引用任何外部 URL。

---

## 事实

### A. `MessageCompleted.reasoning` 的全链路

1. `EventPayload::MessageCompleted` 定义在 `src/events.rs:316-320`，字段依次为 `role: Role`、`text: String`、`reasoning: Option<String>`（`src/events.rs:317-319`）。约束票 02：详情层要展示的「思考全文」就是这一个 `Option<String>`，没有独立事件、没有逐段增量。`src/events.rs:4-6` 的「单一事实来源」注释说明事件日志即真相。

2. 序列化用的是 serde derive 的**默认外部标签**表示：`EventPayload` 的 derive 在 `src/events.rs:282`，`reasoning` 字段没有 `skip_serializing_if`（该属性只出现在 `src/events.rs:166` 的 `Usage` 上），因此一条落地 JSONL 形如 `{"seq":N,"at":"...","speaker_id":"...","payload":{"MessageCompleted":{"role":"Assistant","text":"...","reasoning":"..."}}}`。约束票 02：详情层可以只靠读日志拿到 reasoning，无需新 schema。

3. `reasoning` 在**打码**里有明确分支：`EventPayload::redact` 的 `MessageCompleted` 分支对 `text` 调 `redactor.redact`，并对 `Some(reasoning)` 同样打码（`src/events.rs:442-449`）；该 match 是穷尽的（注释 `src/events.rs:412-419`）。约束票 02：详情层显示的是**打码后的** reasoning，与模型侧重放的一致。

4. 唯一写 assistant reasoning 的地方是 `src/agent.rs:568-572`：`emit(... EventPayload::MessageCompleted { role: Role::Assistant, text: text.clone(), reasoning: (!reasoning.is_empty()).then(|| reasoning.clone()) })`。写入条件是外层 `if !text.is_empty() || !reasoning.is_empty() || !tool_calls.is_empty()`（`src/agent.rs:563`）。约束票 02：**只有工具调用、没有正文也没有思考的 iteration 仍会写** `MessageCompleted`（`tool_calls` 非空即触发），此时 `text == ""` 且 `reasoning == None`。

5. 空 reasoning 被显式转成 `None`（`src/agent.rs:571` 的 `.then(...)`）；`reasoning` 累加器是本次 provider 调用内 `String::new()`（`src/agent.rs:474`），每个 `StreamEvent::ReasoningDelta` 追加（`src/agent.rs:494-497`）。约束票 02：「空」在事件里是 `None`，不是空串，折叠提示行要靠 `None`/`Some("")` 区分是做不到的（`Some("")` 不会被写出）。

6. 其余 `MessageCompleted` 写入点都不带 reasoning：用户自己的消息 `src/agent.rs:207-211`（`reasoning: None`）；讨论合成器的结果 `src/agent.rs:1660-1665`（`reasoning: None`）。`src/lib.rs:647-651` 只是**读取**（`Harness::last_question` 用 `MessageCompleted { role: Role::User, text, .. }` 取最近一条用户消息，`..` 忽略 reasoning），不是写入点。约束票 02/03：只有 assistant 回合的完成消息可能有 reasoning，用户行与合成行永远没有。

7. 完成态的 reasoning **没有**单独的渲染事件：`RenderEvent` 只有四个变体 `Delta` / `Logged(Event)` / `Diagnostic` / `Notice`（`src/render/mod.rs:74-90`），其中 `Delta` 携带 `DeltaKind`（`Text` 或 `Reasoning`，`src/render/mod.rs:66-70`）。reasoning 的**增量**走 `RenderHandle::reasoning_delta`（`src/render/mod.rs:121-127`），reasoning 的**全文**只能从 `RenderEvent::Logged` 里的 `EventPayload::MessageCompleted` 读出。约束票 02：详情层要全文只能走 `Logged` 路径，不能指望某个专门的「reasoning 完成」事件。

8. `Transcript::push_logged` 构造 `Block::Message` 时**丢掉了** `reasoning`：`src/render/transcript.rs:277-283` 的 `EventPayload::MessageCompleted { role, text, .. }` 用 `..` 忽略第三个字段，只填 `speaker` / `role` / `text`。`Block::Message` 本身也只有这三个字段（`src/render/transcript.rs:36-40`）。约束票 02：这是「reasoning 到不了 TUI」的**唯一断点**，位置就在这一处。

9. `TuiState::apply` 消费 `Block`：先 `self.transcript.push(event)`（`src/render/tui.rs:827`），再按块更新 `live` / `mode`（`src/render/tui.rs:828-855`），`Block::Message` 分支只做 `self.live.clear()`（`src/render/tui.rs:840-844`，注释「分 delta 是 live 视图，块才是永久的那份」），最后 `panel.observe(&block)` 并把 `render_block(&block)` 的每一行 `pane.push`（`src/render/tui.rs:856-861`）。约束票 02：`apply` 是「块 → 转录源行」的唯一入口，折叠提示行要么在这里产生、要么在 `render_block` 里产生。

10. `render_block` 对 `Block::Message` 的 assistant 分支用 Markdown 全亮渲染正文（`src/render/tui.rs:2105-2116`），非 assistant 分支逐行原样渲染（`src/render/tui.rs:2121-2126`）；assistant 且 `text.is_empty()` 时返回空行集（`src/render/tui.rs:2110-2112`）。约束票 03：**「只有工具调用、没有正文」的那条 `MessageCompleted` 目前在 TUI 里渲染成 0 行**，折叠提示行没有现成的空行可挂。

### B. 谁消费 `Block`，加字段谁必须动

11. `Block` 的消费者（`src/render/transcript.rs:27-117` 定义，`src/render/mod.rs:58` 再导出）共四处：`src/render/plain.rs:22`（`use ... Block`）、`src/render/tui.rs:54`（`use ... Block`）、`src/render/panel.rs:25`（`use ... Block`）；另有 `src/render/headless.rs:22` 只 `use ... summarize_args`（**不**消费 `Block`，见事实 14）。

12. `plain.rs` 对 `Block::Message` 的匹配**逐字段列出且没有 `..`**：`src/render/plain.rs:81-85`（`Block::Message { speaker, role, text } => self.message(...)`）。Rust 里这种模式若 variant 多出一个字段会编译失败（`missing field`）。约束票 02：给 `Block::Message` 加字段，**`plain.rs` 必须改**，哪怕 plain 根本不显示 reasoning——这是「plain 一个字节不动」判据下唯一会被编译器强制触碰的消费者。

13. `tui.rs` 有两处 `Block::Message`：assistant 分支 `src/render/tui.rs:2105-2109` 同样逐字段列出且无 `..`（会编译失败），其余角色分支 `src/render/tui.rs:2121` 用 `Block::Message { speaker, text, .. }`（带 `..`，静默忽略新字段）。约束票 02/03：TUI 必须改一处、可忽略一处。

14. `headless.rs` **完全不走 `Block`**：它直接 match `EventPayload`（`src/render/headless.rs:96-107` 起），`EventPayload::MessageCompleted { role: Role::Assistant, text, .. }`（`src/render/headless.rs:119-123`）与兜底 `{ .. }`（`src/render/headless.rs:149`）都用 `..`。它自己的 reasoning 观感只来自 `DeltaKind::Reasoning` 增量（`src/render/headless.rs:67-79`，`in_reasoning` 状态在 `:47`）。约束票 02：给 `Block` 加字段对 headless **零影响**。

15. `panel.rs` 只 match `Block::Usage` 与 `Block::TurnEnded`（`src/render/panel.rs:52-58`），不看 `Block::Message`。约束票 02：加字段对 panel 零影响。

16. `src/session/observe.rs`（`sessions show` 时间线）**不使用** `Block`；它自己把事件映射成 `Entry`：`entry_of` 在 `src/session/observe.rs:500`，`Entry::Message { speaker, role, text }` 定义在 `src/session/observe.rs:149-153`，构造点 `src/session/observe.rs:503-507` 用 `EventPayload::MessageCompleted { role, text, .. }` 丢掉 reasoning。计数点 `src/session/observe.rs:103` 与 `:1010` 也只数条数。约束票 02：observe 不因 `Block` 改动而变；若要让它显示 reasoning，那是另一处独立改动（本票不判必要性）。

17. `src/agent/replay.rs`（`sessions replay`）**不使用** `Block`，也不播转录：它把事件重建成 provider `Message` 列表（`src/agent/replay.rs:61-90`），切点取该 speaker 最后一次 `TurnStarted` 的 `seq`（`src/agent/replay.rs:140-156`），材料来自 `build_messages` 与投影。**reasoning 在投影里被保留**：`EventPayload::MessageCompleted { text, reasoning, .. }` 被送进 `PendingAssistant::new(text, reasoning, caps)`（`src/provider/projection.rs:132-138`），并按 `caps.requires_reasoning_replay` 决定是否放回 `reasoning_content`（`src/provider/projection.rs:390-401`）。约束票 02：reasoning 能「据此重建」的是**模型请求**，不是 TUI 的转录。

18. `--continue` 与会话恢复**不重播历史到渲染通道**：`--continue` 只是 `store.latest(&cwd)` 取最近会话目录（`src/cli.rs:251-268`），`Session::open` 里已存在的 `log.jsonl` 由 `EventLog::open` 读入内存（`src/lib.rs:245-253`），但渲染通道里没有历史：全仓库只有一处 `render.logged(...)`，在**唯一写路径** `append_event` 里（`src/agent.rs:2138-2149`），即只有「本进程新追加的事件」才会进渲染通道。约束票 02：`--continue` 后 TUI 的 `pane` 从空开始，历史 reasoning 不会被重建成转录行；详情层若要覆盖历史，需要另找数据源（日志本身）。

### C. 增量到达顺序

19. reasoning delta 与 text delta 走同一个 `broadcast` 通道、同一个 `RenderEvent::Delta` 形状（`src/render/mod.rs:113-127`），只是 `kind` 不同；因此**相对顺序 = 发送顺序**（`src/render/mod.rs:6-8` 明说「增量文本与事件同走一条通道，相对顺序有定义」）。约束票 02：折叠提示行按到达顺序插进转录是可行的。

20. 发送点都在回合循环里、按 provider 流的下一条 `StreamEvent` 逐条处理：`TextDelta → render.text_delta`（`src/agent.rs:490-493`），`ReasoningDelta → render.reasoning_delta`（`src/agent.rs:494-497`）。约束票 02：同一 iteration 内 text 与 reasoning **可以交错**，因为它们各自是流上的独立 item。

21. provider 侧对单个 chunk 固定先 reasoning 后 content：`src/provider/openai.rs:578-584`（注释「Reasoning always precedes content in a delta (Kimi's documented order)」）。约束票 02：常见形状是「一条消息先思考后正文」，但不是一条 delta 一个字段，跨 chunk 仍交错。

22. 每个 iteration 消费完一条流后**各自**发一条 `MessageCompleted`（若满足事实 4 的条件），然后才处理本 iteration 的 `tool_calls`（`src/agent.rs:563-613`），结束时若最后一条 assistant 消息带 tool_calls 就 `continue` 进下一个 iteration（`src/agent.rs:615-619`）。约束票 03：一个回合里「工具调用 iteration」和「最终正文 iteration」会各产生一条 `MessageCompleted`，因此转录里会有多组「思考提示 / 工具调用行」，折叠模型必须按 iteration 分组而不是按回合分组。

23. 回合循环里另有一处 reasoning 发送：`src/agent.rs:1616` 对 `SpeakerId::System` 发 reasoning delta——这是讨论合成器那条路径（无 `TurnStarted`，见 `src/agent/replay.rs:20-23` 的注释）。约束票 02：System speaker 也可能有 reasoning 增量而没有对应的 `MessageCompleted.reasoning`（其写入点是 `src/agent.rs:1663` 的 `reasoning: None`，与事实 6 一致）。

### D. 工具输出的取数与指针

24. 截断发生在**入流之前**，类型是 `SpilledResult`（`src/context.rs:387-398`）：`preview: String`（「流上携带的、自身完整的文本」）、`pointer: Option<PathBuf>`（「溢出文件，写成功时才有」）、`truncated: bool`。约束票 02：事件里能拿到的只有 preview。

25. 落盘路径的确切拼法：`let pointer = outputs_dir.join(format!("{tool_call_id}.txt"))`（`src/context.rs:423`），随后 `create_dir_all(outputs_dir)` + `write_owner_only`（`src/context.rs:424-426`），失败则 `pointer` 为 `None`（`src/context.rs:427`）。约束票 02：`outputs/<tool_call_id>.txt` 是**由 tool_call_id 现算**出来的，不是从事件文本里解析的。

26. 触发落盘的条件是 `estimate_tokens(text) > max_tokens`（`src/context.rs:415-421`）；若 preview 加指针注记比原文还长，则整体放弃溢出、保留全文且 `pointer: None`（`src/context.rs:429-438`）。约束票 02：指针不一定存在，`truncated == false` 时正文就是全文。

27. `preview()` 的确切格式：`format!("{head}\n[truncated: {total_chars} chars, ~{total_tokens} tokens; {note}]\n{tail}")`（`src/context.rs:446-467`），其中 `note` 为 `format!("full output at {}", path.display())`（有指针，`src/context.rs:462-463`）或固定串 `"full output could not be spilled to disk"`（无指针，`src/context.rs:464`）。**指针字符串确实含有 `outputs/<tool_call_id>.txt` 的完整路径**，技术上可以解析；但同一函数在无指针时给出的是另一句固定文案，解析必须区分这两种形状。约束票 02：详情层有两条取数路线（按 id 拼路径 / 解析 preview 文本），事实只记录两者都存在与格式。

28. `truncate_result` 的调用点是 `src/agent.rs:1981-1987`，传入的是 `session.outputs_dir()`（`src/agent.rs:1984`）与 `session.config().max_tool_result_tokens`（`src/agent.rs:1973`）；取出的**只有 `.preview`**（`src/agent.rs:1987`），`pointer` 从未进事件。结果写进 `ToolCallCompleted { output: Some(preview) }` 或 `error: Some(preview)`（`src/agent.rs:1988-2004`）。约束票 02：指针不随事件旅行。

29. `outputs/` 目录名的常量在 `src/session/store.rs:36`（`pub const OUTPUTS_DIR: &str = "outputs";`），会话目录形状记录在同文件 `:3-11` 的文档块（`log.jsonl` + `outputs/`）。`StoredSession` 结构化地携带 `outputs_dir`（`src/session/store.rs:46-54`），新建时 `dir.join(OUTPUTS_DIR)`（`src/session/store.rs:87`），扫描已有会话时 `dir.join(OUTPUTS_DIR)`（`src/session/store.rs:191`）。约束票 02：outputs 目录是会话目录的一部分，跟随会话可搬移。

30. 库里组装期就知道 `outputs_dir`：`let outputs_dir = log_path.parent()...join("outputs")`（`src/lib.rs:231-234`），随后作为 `Session` 的字段（`src/lib.rs:263`）暴露为 `Harness::outputs_dir()` / `Session::outputs_dir()`（`src/lib.rs:854-856`、`src/session/mod.rs:223-225`）。它也被塞进每个工具调用上下文 `ToolContext.outputs_dir`（`src/tools/tool.rs:121`、`src/tools/registry.rs:190,298`）。约束票 02：`outputs_dir` 在**库侧**随时可得，但**渲染层现在拿不到它**（见事实 42）。

31. CLI 组装期另有一处独立点：`session_at(dir)` 在按目录找会话时手工拼 `outputs_dir: dir.join(crate::session::store::OUTPUTS_DIR)`（`src/cli.rs:1910-1921`），用于 `sessions` 子命令。约束票 02：非 TUI 路径也单独知道 outputs 位置。

32. `.before` 的命名约定在 `src/tools/file.rs:47-49`：`pub fn before_artifact(tool_call_id: &str) -> String { format!("{tool_call_id}.before") }`；写点在编辑工具里 `ctx.outputs_dir.join(before_artifact(ctx.tool_call_id))`（`src/tools/file.rs:316-317`）。`/undo` 读取时同样 `session.outputs_dir().join(before_artifact(...))`（`src/agent/history.rs:172-176`）。约束票 02：`outputs/` 下不止 `.txt`，同一 tool_call_id 还可能有 `.before`；详情层若按 id 拼 `.txt`，对 edit 类调用可能同时命中 `.before`。

33. `/undo` 找不到快照时是**硬错误**而非降级：`std::fs::read_to_string(...).map_err(|error| Error::Undo(format!("cannot read the snapshot for {}: {error}", ...)))`（`src/agent/history.rs:172-182`）。约束票 02：`.before` 的缺失语义是失败，与 `.txt` 预览的降级语义不同。

34. 指针失效时的现有降级只写在文档与截断逻辑里：`src/context.rs:6-9`（「指针是增强项——溢出文件缺失时降级为内联 preview，绝不失败」）与 `src/context.rs:406-407`（「写不出溢出就降级为 preview only（spec §11：死指针降级为 preview）」）。代码里**没有**任何「按指针读文件、读失败则回退」的运行时路径——因为指针从未进事件，运行时不读它。约束票 02：所谓「指针失效降级」目前是**设计声明**，实现位置并不存在；详情层若引入读取，需要自己定义这个降级（且 map.md 冻结项 6 已把它定为「降级为事件里的 head/tail 预览」）。

### E. 转录的可命中结构（`Pane`）与 indicator 先例

35. `Pane` 的字段与语义（`src/render/pane.rs:33-57`）：`lines: VecDeque<Line<'static>>` = **源行**（最老在前，长度不超过 `CAP`，`:34-35`）；`starts: VecDeque<usize>` = 每条源行开始的**显示行号**，与 `lines` 平行（`:36-37`）；`wrapped: VecDeque<Line<'static>>` = 在 `width` 下换行后的**显示行**（`:38-39`）；`wrapped_sources: usize` = `wrapped` 已覆盖多少源行（`:40-41`）；`width: u16` = 上次换行用的宽度，首帧前为 0（`:42-43`）；`height: u16` = 上帧面板高度，供按键/滚轮算步长（`:44-45`）；`total: usize` = 上帧显示行总数（源行 + live 尾，`:46-47`）；`top: usize` = 视口顶部的**显示行**号（`:48-49`）；`top_source: usize` = 视口顶落在的**源行**，重换行时保住眼睛位置（`:50-51`）；`follow: bool` = 视口是否吸底（`:52-53`）；`seen: usize` = 上一次吸底时的 `total`，指示块用它算「之后来了多少」（`:54-56`）。约束票 04：显示行 → 源行的映射必然经过 `starts`，点击要落到某个「块」需要另有一张「块 → 源行」表。

36. `CAP = 20_000` 源行（`src/render/pane.rs:23`）；`PAGE_OVERLAP = 2`（`:27`）；`WHEEL_ROWS = 3`（`:30`）。约束票 04：命中结构必须容忍源行被逐出（`evict`，`src/render/pane.rs:214-242` 会同时移动 `starts` / `top` / `total` / `seen` / `top_source`）。

37. `view()` 的映射路径（`src/render/pane.rs:89-110`）：`ensure(width)` 保证换行缓存（`:90`）→ `wrap_text(live, width)` 得到 live 尾的显示行（`:91`）→ `self.total = self.wrapped.len() + live_rows.len()`（`:93`）→ 若 `follow` 则 `top = max_top`（`:97-98`）→ 若 `top >= max_top` 则 `follow = true; seen = total`（`:104-107`）→ `sync_top_source()`（`:108`）→ `window(height, &live_rows)`（`:109`）。约束票 04：`pane.total()` / `pane.top()` 只在**一帧 `view()` 之后**才反映本帧；命中判定必须发生在帧绘制之后（indicator 就是这么做的，见事实 41）。

38. `window()` 的映射（`src/render/pane.rs:254-271`）：从 `self.top` 走到 `self.total`，`index < wrapped.len()` 取 `wrapped[index]`，否则取 `live[index - wrapped.len()]`，凑满 `height` 行即止。约束票 04：一行显示行的屏幕 y = `panes.transcript_text().y + (index - pane.top())`；`view` 返回的 `Vec<Line>` 顺序与 `top..total` 一一对应。

39. `sync_top_source()`（`src/render/pane.rs:245-250`）用 `starts.binary_search(&top)`：命中则取该下标，否则取插入点的前一位（`insert.saturating_sub(1)`）。`ensure()`（`:178-193`）在宽度变化时清空并重算 `wrapped`/`starts`，再用 `self.starts.get(self.top_source)` 把 `top` 移回去（`:191`）。约束票 04：重换行会让显示行号整体失效，只有源行稳定；任何持久化的命中映射应以源行为键。

40. `fresh()`（`src/render/pane.rs:158-164`）在吸底时返回 0，否则 `total - seen`；`following()`（`:153-155`）直接返回 `follow`。约束票 04：指示块显示与否是 `!following()`。

41. **indicator 的完整调用链**（现有唯一「绘制时记矩形、`mouse()` 里命中」的先例）：
    - 状态字段：`TuiState.indicator: Option<Rect>`，注释「上一帧把『回到到底』指示块画在哪，好让点击对上用户真正看到的东西」（`src/render/tui.rs:377-379`），在 `TuiState::new` 初始化为 `None`（`src/render/tui.rs:769`）。
    - 清空点 1：终端低于最小尺寸时 `state.indicator = None`（`src/render/tui.rs:1498-1502`，注释「不画任何点击能落到的东西」）。
    - 清空点 2：`draw_modal` 一进入就 `state.indicator = None`（`src/render/tui.rs:1532-1535`，注释「从这儿起问题在上面，覆盖层盖住了指示块，点击它原来的位置必须无效，哪怕覆盖层最后画不下」）。
    - 绘制：`draw_frame`（`src/render/tui.rs:1496`）→ `draw_transcript`（`:1511` 调用，定义 `:1781`）→ `pane.view(text_area.width, text_area.height, &state.live)`（`:1784-1786`）→ `draw_scrollbar`（`:1788`）→ `draw_indicator(frame, text_area, state)`（`:1789`，定义 `:1835`）。
    - `draw_indicator` 内部：吸底或零面积则置 `None` 并返回（`src/render/tui.rs:1836-1839`）；文案 = `wording::back_to_bottom()` 或 `wording::new_content(fresh)`（`:1840-1845`）；`rect = Rect::new(area.right() - width, area.bottom() - 1, width, 1)`（`:1849-1854`）；画完把 `state.indicator = Some(rect)`（`:1864`）。
    - 命中：`TuiState::mouse`（`src/render/tui.rs:871`）先因 `self.pending.is_some()` 整体早退（`:872-876`，注释「问题同时占有指针和键盘」），再 `MouseEventKind::Down(MouseButton::Left)` → `self.indicator_hit(mouse.column, mouse.row)` → 命中即 `self.pane.to_bottom()`（`:881-885`）；`indicator_hit`（`:891-898`）做标准的半开区间包含判断。
    - 测试先例：`tests/render_layout.rs:664-673` 用 `find_cell(&frame, ...)` 找到「点」字所在格再投递鼠标事件，并断言它「在滚动条左边」；`tests/render_layout.rs:606` 覆盖计数与滚轮 3 行。
    约束票 04：这是可以直接照抄的结构（一个 `Option<Rect>` 字段 + 绘制时记录 + `mouse` 里包含判断），但注意它面对的是**单一矩形**；折叠行/选项行是**多目标**命中。

42. 渲染层**不读文件系统**：`src/render/tui.rs`、`src/render/input.rs`、`src/render/panel.rs`、`src/render/layout.rs` 里没有任何 `outputs` / `outputs_dir` / `read_to_string` / `.before` 引用（逐文件 grep 均为 0 命中）。约束票 02：详情层要「取工具全文」，要么把目录注入进 `TuiOptions`/`SessionFacts`，要么新开一个端口；照现在这层没有现成的 IO。

### F. 两种问答绘制的行坐标

43. 覆盖层 `draw_modal`（`src/render/tui.rs:1528-1584`）的几何：行集按 title（`:1546-1549`）、summary（`:1552-1554`）、detail（`:1555-1557`）依次 `pane::wrap_text(..., inner)` 拼出；分离空行只在 `rows_available >= 3 && !rows.is_empty()` 时插入（`:1561-1565`）；**候选键行永远最后一行**（`:1566` `rows.push(choices_row(modal.choices))`，定义 `:1604-1618`）；`area = panes.modal(rows.len())`（`:1567`）；内容画在 `layout::inner(area)`（`:1578-1583`）。约束票 04：覆盖层里「每一行」的 y 坐标在绘制时**可得**（`layout::inner(area).y + index`），但 `draw_modal` 目前只把 `area` 用于画，**没有任何字段记录选项行的位置**。

44. 覆盖层几何本身可得：`Regions::modal(rows)`（`src/render/layout.rs:163-175`）把 `rows + BORDER_ROWS` 高的框居中于 `middle`，宽度 = `modal_width()`（`:154-159`，`middle.width - MODAL_MARGIN` 且不超过 `MODAL_MAX_WIDTH`），高度超过 `middle.height` 时返回 `None`；`layout::inner` = `(x+1, y+1, w-2, h-2)`（`src/render/layout.rs:357-364`）。约束票 04：候选键行的屏幕 y = `modal_area.y + 1 + rows.len() - 1 = modal_area.bottom() - 2`——但当前代码没把它存下来。

45. 覆盖层的候选键是一条**文本行**而不是多个矩形：`choices_row` 把 `[y] 允许   [a] 总是允许` 拼成一整个 `Line`（`src/render/tui.rs:1604-1618`），每对键/标签的**列区间**由 `Span` 顺序决定，代码里没有按 span 起点切片、也没有记录每个选项的列范围。约束票 04：中段覆盖层的「每个选项的矩形」当前**不存在**，要么从 span 宽度现算，要么改成逐项绘制。

46. 底部问卷 `draw_questionnaire`（`src/render/tui.rs:1952-1964`）：调 `questionnaire_window(&questions[index], &drafts[index], panes.input.width, panes.input.height)`（`:1957-1962`），把返回的 `Vec<Line>` 整体 `Paragraph::new(rows)` 画在 `panes.input`（`:1963`）。约束票 04：问卷的行坐标基准是 `panes.input`（`src/render/layout.rs:96`，由 `inside(bottom, input_rows)` 得到，`:319`）。

47. `questionnaire_parts`（`src/render/tui.rs:1418-1490`）把一道题拆成三段：`prefix`（header 换行行 + 题面换行行，`:1423-1441`）、`options`（每个选项一行，`:1443-1477`）、`custom`（一行「自定义/答案」输入行，`:1479-1488`）。每行文本已在函数内拼好（`{cursor} {marker} {label}`，`:1459-1462`），高亮 `index == draft.highlight`（`:1445`），选中用 `draft.selected` 判定（`:1446-1449`）。约束票 04：`options[i]` 的文本、是否高亮、是否选中在这里都是现成的；**每行对应的 y 坐标在这个函数里没有算**，只有顺序。

48. `questionnaire_window`（`src/render/tui.rs:1372-1395`）：若 `prefix.len() + options.len() < height` 则全量（prefix + 全部 options + custom，`:1379-1384`）；否则 `room = height - (prefix.len() + 1)`（`:1388`），`start = option_window_start(draft.highlight, options.len(), room)`（`:1389`），再把 `options.skip(start).take(room)` 夹在 prefix 与 custom 之间（`:1390-1393`），最后 `truncate(height)`（`:1393`）。约束票 04：可见 option 的**显示行号** = `prefix.len() + (i - start)`（`prefix` 与 `custom` 固定不动），所以「视觉第几行」在绘制时可算，屏幕 y = `panes.input.y + 该行号`。

49. `option_window_start`（`src/render/tui.rs:1400-1410`）：`room == 0 || count <= room` 返回 0；否则 `highlight < room ? 0 : highlight + 1 - room`，再 `min(count - room)`。约束票 04：窗口随高亮滚动，因此**同一个选项在不同帧可能落在不同 y**；命中必须用**当帧**的 `start` 反算，不能用固定偏移。

50. `Questionnaire` 的三个字段与选项窗口的关系：`questions: Vec<UserQuestion>`（`src/render/tui.rs:414-415`）、`reply`（`:416-419`）、`drafts: Vec<QuestionDraft>` 与 `questions` 同序（`:420-421`）、`index: usize` 当前题（`:422-423`）；每题的 `QuestionDraft` 持有 `selected: Vec<String>`、`custom: String`、`highlight: usize`、`skipped: bool`（`:427-441`）。只有**当前题的当前 draft** 参与绘制与 `option_window_start`（`src/render/tui.rs:1958-1959`、`:1389`），其它题的 `highlight` 被记住但不在屏幕上（`:433-437` 注释）。约束票 04：点击命中只对当前 `index` 的可见选项有意义；切题后必须重算。

51. 行数预算也是同一个算式：`TuiState::bottom_rows` 用 `questionnaire_lines(...).len()` 向布局要高度（`src/render/tui.rs:1331-1341`，`questionnaire_lines` 定义 `:1353-1362`，即 prefix + 全部 options + custom 的**全量**长度），而 `questionnaire_window` 才做裁剪（`:1347-1349` 注释明说这一点）。约束票 04：`panes.input.height` 与可见行数是同一来源，绘制期拿它与 `prefix.len()` / `start` 即可反算任意可见选项的 y。

52. 问卷的状态机（供命中后调用）：`press(key)`（`src/render/tui.rs:468-509`）里 `Enter` 在全部已答时提交（`:470-473`）、已答则前进（`:474-475`）、未答则有选项时 `confirm_highlight` 且单选再 `advance`（`:476-481`）；`Space` 同 `Enter` 的确认分支（`:486-491`）；`Tab` 置 `skipped` 并前进（`:493-496`）；`confirm_highlight`（`:522-544`）单选**替换** `selected` 并清空 `custom`、多选**切换**；`type_custom`（`:563-570`）单选时清空 `selected`。提交与拒绝都经 `questionnaire_key`（`src/render/tui.rs:999-1009`）→ `reply.send(Ok(answers))`；`answers()` 的编码规则在 `:594-623`（skipped → `selected: []` 且无 custom；单选有 custom → `selected: []`）。约束票 04：单击若要「立即作答/前进」，语义已由这些函数定义好，点击只需喂等价动作。

53. 中段覆盖层的应答路径：`TuiState::key` 先处理 `CtrlC`/`Esc`（`src/render/tui.rs:1013-1043`），有 `pending` 时问卷走 `questionnaire_key`、其余只接受 `Key::Char(_) | Key::Enter` 走 `answer_key`（`:1044-1060`），`answer_key`（`:1256-1299`）按 `(question, key)` 映射到 `AnswerChoice` 并 `reply.send`；`Esc` → `decline` 发 `default_choice`（`:1303-1313`）。约束票 04：覆盖层的点击若走「合成按键」，等价入口就是 `answer_key` / `decline`；若要直接发送答案，则需要拿到 `Pending` 里的 `oneshot::Sender`（当前是私有状态）。

### G. 注入面与状态

54. `SessionFacts`（`src/render/tui.rs:166-179`）字段为 `session_id: String`、`cwd: String`、`model: String`、`context_window: u64`、`budget_limit: Option<u64>`，derive `Default + PartialEq + Eq`（`:166`）；文档注释说明「这里的一切都是组装期已知、作为一个值注入的，因为那就是接缝：渲染器从不自己去够配置」（`:156-161`），并强调「中途会变的东西（模式）故意不在这里」。约束票 02：若详情覆盖层要 `outputs/` 或会话目录，`SessionFacts` 是现成的注入位；`cwd` 当前实际被塞的是**会话目录**而非工作目录（见事实 56）。

55. `TuiOptions` 只有两个字段：`port: ConsolePort` 与 `facts: SessionFacts`（`src/render/tui.rs:181-186`）。约束票 02：新增「取全文」能力的最小注入面就是这两个之一。

56. `SessionFacts` 的两个组装点都在 `src/cli.rs`，且 `cwd` 传的是会话目录：交互路径 `SessionFacts { session_id: stored.id.as_str().to_owned(), cwd: stored.dir.display().to_string(), model: model.clone(), context_window: crate::context::usable_input(&caps), budget_limit: session_config.budget.limit }` 后接 `Renderer::tui(TuiOptions { port, facts })`（`src/cli.rs:320-327`）；讨论路径同形（`src/cli.rs:590-604`），其 `model` 是「两个模型的串」（`:598`）。`stored.dir` 的定义在 `src/session/store.rs:46-54`，而 `outputs_dir = dir.join("outputs")`（同文件 `:87`/`:191`）。约束票 02：`facts.cwd.join("outputs")` 就能拼出 outputs 目录，**不需要新字段**——但要注意该字段名叫 `cwd` 而值是会话目录，容易误用。

57. `TuiState` 的可变状态全表（`src/render/tui.rs:337-381`）：`facts`、`mode`、`transcript: Transcript`、`pane: Pane`、`live: String`、`clock`、`dirty`、`editor: Input`、`catalog: Vec<CatalogEntry>`、`slash: MenuSelection`、`panel: Panel`、`prompt_reply: Option<oneshot::Sender<Option<String>>>`、`running: bool`、`pending: Option<Pending>`、`events: Vec<FrontEndEvent>`、`indicator: Option<Rect>`、`quit: bool`；初值在 `TuiState::new`（`:749-772`）。约束票 02/04：详情覆盖层的开合与滚动状态、命中矩形自然落在这一类字段里；`pending` 同时被键盘与指针用来「一道题独占」。

58. `Pending` 是四态私有枚举（`src/render/tui.rs:389-405`）：`Loop { question, reply }`、`Paste`、`ClearDraft`、`Questionnaire(Questionnaire)`；`Pending::modal()`（`:626-675`）把前三者映射成 `Modal`、问卷返回 `None`（`:671`），`Modal` 定义在 `:684-694`。约束票 04：中段覆盖层只有前三态会画，问卷是另一套绘制；map.md 冻结项 6 的「详情覆盖层不可叠在待答问题之上」在结构上正对应 `pending.is_some()` 这个闸门（`:872`、`:1044`）。

59. 可复用的接缝一：`ConsolePort`（`src/render/input.rs:202-217`）只有 `recv()`（`:209-211`）与 `emit(FrontEndEvent)`（`:214-216`）；`FrontEndEvent` 是三个 `Copy` 变体 `Cancel` / `TogglePlan` / `Quit`（`:141-149`）。它是**单向**的：前端 → 循环的请求由 `ConsoleRequest` 走 `ConsolePort::recv`，前端的主动手势走 `emit`。约束票 02：若「取全文」是**纯只读本地文件**，走端口会把渲染层变成请求方，方向与现有 `ConsolePort`（循环问、前端答）相反。

60. 可复用的接缝二：`Asker`（`src/permissions.rs:867-879`）是一个 `#[async_trait]` 的 `Send + Sync` 端口，两个方法 `ask(&PermissionRequest) -> Answer`、`ask_plan_conflict(&Path) -> PlanConflict`；`ConsoleAsker` 在 `src/render/input.rs:240-285` 基于 `ConsoleHandle` 实现它。`ConsoleAsker::from_handle(&console)` 在 `src/cli.rs:340`（交互）与 `:621`（讨论）组装。约束票 02：这是「渲染器不读环境/配置、能力由上层实现并通过 trait 注入」的**唯一既有范式**；一个只读「取全文」trait 可以照它的形状（trait + 组装点注入），但 `Asker` 本身是异步问/答，与同步只读读文件不同。

61. `ConsoleQuestions` / `UserQuestions` 是第三个注入范式：模型的问题走同一键盘但自己的端口（`src/render/input.rs:287-315`，`impl UserQuestions for ConsoleQuestions` 在 `:308`），CLI 侧 `Some(Arc::new(ConsoleQuestions::from_handle(&console)))`（`src/cli.rs:345-346`、`:624-625`），工具表是否提供 `ask_user_question` 由「端口在不在」决定而非第二个开关（`src/cli.rs:341-346` 注释）。约束票 02：注入纪律是「能力的有无由组装期注入决定，运行期不去够」。

62. `TuiState` 被测试直接构造（`pub struct TuiState` + `pub fn new`，`src/render/tui.rs:337`、`:750`），`tests/render_tui.rs:23-34` 与 `tests/render_layout.rs:17-28` 都是「造 `SessionFacts` → `TuiState::new(facts)` → 喂 `RenderEvent` / 按键」的模式，`draw_frame` 是布局测试的接缝（`tests/render_layout.rs:95`、`:1762`）。约束票 03：任何新状态如果加进 `TuiState`，这两个测试文件的 helper 会立刻看到它，不需要终端。

### H. DSH 点击作答先例（引用既有研究 §3，不重查 DSH 产物）

63. 单选单击即选中并**自动前进到下一题**：`.scratch/ask-user-question/research/01-dsh-ask-user-question-and-composer-takeover.md:67`（「单选的 `choose` 会自动前进到下一题（`:526`）」）。约束票 04：map.md 冻结项 8 的「单击即选中并立即作答/前进」与之一致。

64. 多选是**切换**勾选、仍需提交：同上 `:67`（多问题分页 `index` + `drafts`）与 `:68`（选项是 `<button>`，单选 `role="radio"`、多选 `role="checkbox"`，无显式方向键导航，`onKeyDown` 只在 `Enter` **且所有题都 completed** 时提交）。约束票 04：多选单击切换、提交仍是独立动作。

65. 自定义文本：`AnswerField` 是 `<textarea rows=1>`，`Shift+Enter` 换行、`Enter`（非 IME composing）→ `continueFlow`；且**单选下打字会清空已选选项**、多选下保留（同上 `:69`）。这与本仓库 `type_custom`（事实 52，`src/render/tui.rs:563-570`）语义相同。约束票 04：点击作答必须与「打字清空单选」共存。

66. `Esc` 是安全答案/取消：卡片右上 close → `pending.cancel()`，reject `"the user cancelled ask_user_question"`、code `ASK_CANCELLED`，并 `clear` 草稿（同上 `:73`）。约束票 04：`Esc` 的既有语义与本仓库 `decline`（事实 53）同向，但两者的答案值不同（一个是取消，一个是 `default_choice`）。

67. 答案取**原始 label**：点击调 `choose(option.label)` 用的是原始 label，选中判定 `draft.selected.includes(option.label)` 也是原始 label；`parseRecommendedLabel` 只用于**显示**（badge + `aria-label`），剥掉 `(Recommended)` 后缀不改变答案值（同上 `:65`）。约束票 04：点击把 label 原样送进 `selected`，展示用的修饰（本仓库的 `questionnaire_option` 序号与 recommended 徽标，`src/render/tui.rs:1459-1462`）不得混入答案值。

68. 一次只有一个问题实例拥有输入区：pending-interaction 表按 Session 只留一个（并列按 `precedence` 取大），README 记「One request owns the composer at a time」（同上 `:62`）。本仓库的对应物是 `pending: Option<Pending>` + `request()` 里「已有 pending 就丢掉新问题」（`src/render/tui.rs:931-949`）。约束票 04：命中实现可以假定「至多一道题在屏上」。

---

## ⚪ 未证实 / 查不到

1. **（复核后移出本节）** `src/lib.rs:647` 的 `MessageCompleted` 已查明是 `Harness::last_question` 的**读取**分支（`src/lib.rs:643-655`），不是写入点，故不再是未证实项。此条保留痕迹以说明它被查过。

2. **`Some("")` 这种 reasoning 是否可能出现**：`src/agent.rs:571` 用 `(!reasoning.is_empty()).then(...)` 把空串转成 `None`，所以**本仓库自己写的**事件不会有 `Some("")`；但日志可以由旧版本或手写产生，反序列化层没有校验。未查任何反序列化校验代码，故「详情层不会见到 `Some("")`」**未证实**。

3. **reasoning 增量与 text delta 在同一 iteration 内的精确交错模式**：只证实了「顺序 = 发送顺序」（事实 19-21）与 provider 单 chunk 内先 reasoning 后 content（`src/provider/openai.rs:578-584`）。**没有**查到任何保证「一个 iteration 的所有 reasoning 都在所有 text 之前」的代码或规范文本；Kimi 的文档顺序只是 `src/provider/openai.rs:578-579` 的注释转述，未读原始文档。故「同一 iteration 内不会交错」**未证实**，且从代码看**可以交错**。

4. **`regions.modal` / `panes.input` 在绘制时的实际 y 是否与鼠标事件的坐标系一致**：本票没有读 crossterm/ratatui 的鼠标坐标约定，也没有查 `tests/render_layout.rs` 的 `find_cell` 实现细节。indicator 的命中（`src/render/tui.rs:881-898`）用 `mouse.column/row` 与 `Rect` 直接比较，且有测试（`tests/render_layout.rs:664-673`），可**间接**支持坐标系一致，但没有专门核实。故「鼠标 row 与 `Rect.y` 同坐标系」是**有间接证据、未直接证实**。

5. **`Pane` 是否有把「显示行」映射回「块」的现成结构**：字段全表（事实 35）里只有 `lines` / `starts` 这类**行级**结构，没有块级索引；`Block` 与 `pane.push` 之间没有任何 id 或边界记录（`src/render/tui.rs:856-861` 只是逐行 push）。这是从「没找到」得出的结论，**不是**从某条注释或文档证实的设计声明。

6. **pointer 字符串是否在实践中被任何人都解析过**：全仓 grep 只找到 `src/context.rs` 的生成侧与 `src/agent.rs:1981-1987` 的取 `.preview` 侧，没有读取/解析 `"full output at ..."` 的调用点。故「有别的消费者依赖该格式」**查不到**（倾向于没有）。

7. **`outputs/` 下的文件是否只有 `.txt` 与 `.before` 两类**：只核实了这两个命名（事实 25、32）。没有穷举所有写 `outputs_dir` 的工具，故「没有第三类 artifact」**未证实**。

8. **`TuiState::mouse` 在问卷/覆盖层上的现有豁免是否会阻止未来命中**：`src/render/tui.rs:872-876` 是「有 pending 就整体早退」，`src/render/tui.rs:881-885` 只有 indicator 一个目标。没有查任何已经为「问题上的点击」预留的开关或字段；按现读代码，**当前不存在**任何点击作答的接缝。

9. **DSH 研究 §3 的行内引用（如 `:526`、`:649-694`）指向的文件本身**：按本票要求未重查 DSH 产物，这些引用**转述自** `.scratch/ask-user-question/research/01-dsh-ask-user-question-and-composer-takeover.md`，其准确性以该文件为准，本票未独立复核。
