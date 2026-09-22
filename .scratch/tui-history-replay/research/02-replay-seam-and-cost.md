# research：分帧重播的接缝事实与成本实测

Part of: ../map.md
Ticket: ../issues/02-research-replay-seam-and-cost.md
Status: resolved（本文件只报事实，不推荐方案、不选赢家）

本文件的每一行事实都来自本仓库源码的现读现查。行号以本文件写就时的文件为准；因为同一 workspace 里有并发的 tui-ux 实现改动（见「快照」节），本文件同时给出被引用文件的 `git hash-object`，便于 `git hash-object <file>` 复核行号是否漂移。

- 仓库 HEAD：`288e77c`
- 被测工作树：`src/render/tui.rs`、`src/render/layout.rs`、`src/render/wording.rs` 有**未提交**的 tui-ux 布局改动（去掉 airy 行）。这三个文件里只有 `tui.rs` 出现在本票的接缝链上，且改动不碰 `select!` / `apply` / `SessionFacts` / `TuiOptions`（改动在 `draw_mark` 与布局算式）。
- 引用的稳定文件（无未提交改动）的 blob：`pane.rs f4d33bfe`、`transcript.rs 508f89b2`、`panel.rs db339106`、`mod.rs aad01539`、`cli.rs fe2d20ef`、`lib.rs 372971c4`、`events.rs 78cbca49`、`session/mod.rs b4b184e3`、`session/store.rs 6d8373be`、`config.rs 2b70efd4`、`agent.rs bcedbc8d`、`agent/history.rs 4a443665`、`context.rs`（见下）。
- 引用中的 `src/render/tui.rs` blob：`2e740e1c`（2401 行）。若未来 `git hash-object src/render/tui.rs` 不再等于它，下面所有 `tui.rs` 行号按函数名/常量名重新定位。

---

## 来源清单

本机仓库（repo-relative）：

- `src/render/tui.rs` —— `Tui::run` 的 `select!` 主循环、`TuiState::apply`、`dirty` 标记、`SessionFacts` / `TuiOptions`、`render_block`
- `src/render/mod.rs` —— `RenderEvent` / `RenderHandle` / `RENDER_CHANNEL_CAPACITY` / `channel()`
- `src/render/transcript.rs` —— `Transcript::push` / `push_logged` / `flush`、`Block`
- `src/render/pane.rs` —— `Pane::push` / `view` / `ensure` / `wrap_pending` / `evict`、`CAP`
- `src/render/panel.rs` —— `Panel::observe` 的消费面
- `src/render/highlight.rs` —— 工具结果预览的 tree-sitter 高亮（单块大头成本）
- `src/cli.rs` —— `--continue` 解析、`SessionStore::latest`、`SessionFacts` / `TuiOptions` 组装点、`prune`
- `src/lib.rs` —— `OpenedSession::open` / `start`、`Harness::events` / `outputs_dir` / `notice`、plan mode 注入
- `src/events.rs` —— `Event` / `EventPayload`、`EventLog`（`open` / `events` / `append`）、`read_events`
- `src/session/mod.rs` —— `Session::events` / `log` / `outputs_dir` / `cwd`
- `src/session/store.rs` —— `StoredSession`、会话目录形状、`latest` / `delete` / `prune`、`cwd_slug`
- `src/config.rs` —— `sessions_dir` 的 `XDG_DATA_HOME` / `HOME` 规则
- `src/agent.rs` —— `emit_completed`、唯一写路径 `append_event`、plan mode 的 `retire_plan_instructions`
- `src/agent/history.rs` —— `recover_pending_calls`、`INTERRUPTED`
- `src/context.rs` —— `truncate_result` / `SpilledResult` / `outputs/<id>.txt` 的落盘点
- `tests/render_delivery.rs` —— 通道溢出行为的既有断言

成本实测的合成基准与运行脚本（本票新增的一次性产物，不是产品代码）：

- `.scratch/tui-history-replay/research/bench/Cargo.toml` —— 自己的 `[workspace]`，path 依赖 `../../../..`
- `.scratch/tui-history-replay/research/bench/src/main.rs` —— `replay` / `synth` / `evict` 三个模式
- `.scratch/tui-history-replay/research/bench/run-bench.sh` —— 一次跑完 build + 全部测量

未离开仓库：本票没有引用任何外部 URL。通道语义引用的是本机 cargo registry 里的 tokio 源码（`~/.cargo/registry/src/*/tokio-1.53.1/src/sync/broadcast.rs:41-53`），并已用仓库自己的 `tests/render_delivery.rs` 交叉印证。

---

## 事实

### A. 渲染循环骨架（问题 1）

1. `TICK = Duration::from_millis(120)`（`src/render/tui.rs:70`）；`DRAIN_LIMIT = 4_096`（`:77`）；`LIVE_BUFFER = 4_000`（`:62`）。约束票 01：分帧闸门必须放进这个 **120ms tick / 每次循环一次 draw** 的节奏里，「一帧」在这个循环里的自然含义是「一次 `select!` 轮 + 一次 draw」，不是「120ms」。tick 只是下限节拍，事件到达也会立刻推进一轮。

2. `Tui::run` 里 `tokio::select!` 在 `src/render/tui.rs:220`，共 **4 路**：`receiver.recv()`（`:221`，广播渲染事件）、`keys.next()`（`:228`，crossterm 键盘/粘贴/鼠标/resize）、`port.recv()`（`:241`，循环的 `ConsoleRequest`）、`tick.tick()`（`:245`）。**没有 `biased`**（全文件 grep 无该关键字），即 tokio 默认在多个就绪分支间随机选，不保证优先级。约束票 01：重播不能靠「tick 分支优先」来做；任何分帧闸门要么复用 tick、要么新增一路，二者都与键盘/请求同级。

3. 每轮 `select!` 之后有一个**有界排空**：`while drained < DRAIN_LIMIT { receiver.try_recv() ... }`（`:251-265`），把已排队的渲染事件并入同一帧；这解释了文件头注释「a bursting provider costs frames rather than events」（`:13-17`）。约束票 01：批大小上限天然存在（每轮最多 `1 + 4096` 条渲染事件），512 的分帧预算在这个上限之下。

4. 排空之后顺序固定：`state.take_events()` → `port.emit(...)`（`:270-272`），然后 `if state.is_dirty()` 才 `terminal.draw(...)`（`:273-283`），画完 `state.mark_clean()`（`:282`）。约束票 01：重播的「完成信号」如果只写状态而不置脏，就永远不会有一帧；`apply` 本身置脏（见事实 6）。

5. `render_block` 之外，`apply` 是「块 → 转录源行」的唯一入口：`pub fn apply`（`:841`）。约束票 01/03：无论走哪条接缝，历史最终必须变成 `RenderEvent` 交给 `apply`（或等价地调 `Transcript::push` + `render_block` + `Pane::push`），否则 live 与历史会分叉成两套表现规则。

6. **脏标记落点**：字段 `dirty: bool`（`:360`），初值 `true`（`:774`）。置脏点：`mark_dirty()`（`:795-797`）、`refresh_clock()` 仅当渲染出来的分钟变了（`:804-809`）、`paste()`（`:828`）、`apply()` 开头（`:842`）、`mouse()`（`:893`）、`request()`（`:918`）、`key()` 开头（`:1030`）。清脏点只有 `mark_clean()`（`:799-802`），由主循环在 draw 之后调用（`:282`）。约束票 01：分帧重播若在 `apply` 之外改状态，必须显式 `mark_dirty()`，否则冻结项里的「进度行」不会重画。

7. `RenderEvent` 从广播到 `apply` 的完整路径：
   `RenderHandle::text_delta/reasoning_delta/logged/diagnostic/notice`（`src/render/mod.rs:113/121/129/134/145`）→ `broadcast::Sender::send`（同一个 sender，`mod.rs:109`）→ 通道由 `render::channel()` 建立（`mod.rs:211-213`，容量见事实 14），receiver 在 `OpenedSession::open` 里交给 `Renderer::spawn`（`src/lib.rs:238-239`）→ `Tui::consume` → `Tui::run(receiver)`（`tui.rs` 的 `Render for Tui` impl）→ `receiver.recv()`（`:221`）或 `receiver.try_recv()`（`:253`）→ `state.apply(event)`（`:222` / `:254`）。约束票 01：渲染通道是**唯一**已有的 live 入口，历史上行若走它注入，顺序由发送顺序定义（`mod.rs:6-8` 明说相对顺序有定义）；若另开接缝，就要自己定义与 live 的合并顺序。

8. 满/落后时的行为：`RecvError::Lagged(dropped)` 与 `TryRecvError::Lagged(dropped)` 都转成一条 `RenderEvent::Diagnostic(wording::renderer_dropped(dropped))` 然后 `apply`（`tui.rs:223-225`、`:255-257`）；文案是 `渲染器丢弃了 {dropped} 个事件`（`src/render/wording.rs:767-769`）。`tests/render_delivery.rs:26-67` 断言「解码一整个网络块（2×容量条 delta）时，只要生产循环让出，渲染器丢 0 条」。约束票 01：通道满不是错误而是丢弃最旧；如果重播走广播通道，**任何 >1024 条的批次都必须给渲染任务让出**，否则历史会被静默截断。

### B. 历史在组装期哪里可达（问题 2）

9. `--continue` / `-c` 只置一个布尔：`parsed.resume = true`（`src/cli.rs:168`）。约束票 01：入口就是这一个 flag，「按 id 恢复」不在本图（map 冻结项 3）。

10. `--continue` 分支拿到的是 `StoredSession`：`store.latest(&cwd)`（`src/cli.rs:251-252`），失败/空分别报 `startup_no_session_to_continue` / `startup_store_read`（`:254-267`）。`StoredSession` 的字段是 `id` / `dir` / `log_path` / `outputs_dir`（`src/session/store.rs:46-54`）。约束票 01：组装期 CLI 手里**只有路径**（`log_path`、`outputs_dir`、会话目录），没有事件。

11. **CLI 在 `TuiOptions` 组装时不持有历史事件 `Vec`。** 组装点是 `let facts = SessionFacts {...}; Renderer::tui(TuiOptions { port, facts })`（`src/cli.rs:320-327`），它发生在 `assemble(...)`（`:348`）**之前**；此时 CLI 只持有 `stored`。约束票 01：若历史要从渲染器外部注入，注入值必须在 `assemble` 之前从 `stored.log_path` 现读，或者接缝放到 `assemble` 之后（`Harness` 是可得的，见事实 13）。

12. `assemble` 之后 CLI 持有 `harness`（`src/cli.rs:348-377`），并马上用它发 banner：`harness.notice(&render::wording::banner(...))`（`:383-389`）。约束票 01：banner 现在是**组装后立即**经 `Notice` 进渲染通道的；冻结项 9 要求它在重播完成后追加，这条现状是票 01 必须处理的顺序点。

13. 只读事件访问器**都已存在**：`EventLog::events(&self) -> Vec<Event>`（`src/events.rs:918-920`，clone 快照）、`Session::events()`（`src/session/mod.rs:163-165`）、`Session::log() -> &EventLog`（`:169-171`）、`Harness::events() -> Vec<Event>`（`src/lib.rs:840-842`）、`DiscussionHarness::events()`（`:890-892`）。另有一个**纯只读**的文件加载器 `pub fn read_events(path) -> io::Result<Vec<Event>>`（`src/events.rs:959-981`），`sessions` / `ledger` / `observe` 已经在用它（如 `src/cli.rs:2020`、`src/session/observe.rs:84`）。约束票 01：接缝候选至少有两个现成面（库侧 `Harness::events()`、文件侧 `read_events`），不需要新造事件读取能力。

14. 注意 `EventLog::open` **不是**只读：它 `read_events` 之后还调 `repair_before_append`（`src/events.rs:888-891`），后者会把无法解析的残尾 `set_len` 截掉（`:988-1008`）。`read_events` 没有这个副作用。约束票 01：如果「重播前先读一遍日志」，用 `read_events` 才是真正的只读；`EventLog::open` 会改文件，而 `--continue` 的组装路径本来就会走 `EventLog::open`（`src/lib.rs:245-249`）。

15. `TuiOptions` 只有两个字段：`port: ConsolePort` 与 `facts: SessionFacts`（`src/render/tui.rs:187-190`）。`SessionFacts` 五个字段是 `session_id` / `cwd` / `model` / `context_window` / `budget_limit`（`:171-185`），两处组装点都在 `src/cli.rs`（交互 `:320-327`、讨论 `:590-604`），讨论路径把两个模型名拼成一串（`:596-599`）。约束票 01：要给渲染器历史，`TuiOptions` 是现成的注入位；票 03 还要决定 `SessionFacts` 在重开时是否需要新字段（当前它不含 outputs 路径）。

16. `SessionFacts.cwd` 装的是**会话目录**，不是工作目录：两处都是 `cwd: stored.dir.display().to_string()`（`src/cli.rs:322`、`:592`）。真正的 workspace cwd 走 `SessionScaffold.cwd`（`:350`）→ `Session::cwd()`（`src/session/mod.rs:177-179`）→ 记进 `SessionStarted.cwd`。约束票 01/03/04：`facts.cwd.join("outputs")` 恰好等于 `stored.outputs_dir`，但字段名叫 `cwd`，票 03/04 引用时不能把它当工作目录。

### C. `RenderEvent` / `RenderHandle` 的形状（问题 3）

17. `RenderEvent` 四个变体：`Delta { speaker, kind, text }` / `Logged(Event)` / `Diagnostic(String)` / `Notice(String)`（`src/render/mod.rs:75-90`）。约束票 01/03：`Logged(Event)` 是「一条历史事件」的唯一既有载体；`Notice` 是唯一「不speak任何事件」的文本行（`:84-89`），banner 走它。

18. **有公开的注入方法**：`RenderHandle::logged(&self, event: &Event)` 是 `pub fn`（`src/render/mod.rs:129-131`），会 clone 事件后 `send`。`RenderHandle` 由 `pub fn channel()` 创建（`:211-213`），两个都是 `pub`，`render` 模块也是 `pub mod`（`src/lib.rs:39`）。约束票 01：库外代码可以自己建一条通道注入 `Logged`（tests 就是这样：`tests/render_plain.rs:44`、`tests/discussion.rs:1129`）。

19. **但注入进一个正在跑的 `Harness` 没有公开路径**：`Harness` / `DiscussionHarness` 的 `render: RenderHandle` 字段私有（`src/lib.rs:160`、`:178`），没有 `render()` / `handle()` 访问器（`Harness` 的 `pub fn` 全表里没有；对通道的唯一公开出口是 `Harness::notice()`，`src/lib.rs:831-833`，而它发的是 `Notice`）。生产代码里 `render.logged` 全仓只有一处调用：`src/agent.rs:2148`，在唯一写路径 `append_event` 里（`:2138-2150`）。约束票 01：clap 侧要「把历史经渲染通道推给正在跑的 TUI」，当前**没有**现成把手；要么新增 accessor，要么改走 `TuiOptions` 注入。

20. 通道容量 `RENDER_CHANNEL_CAPACITY: usize = 1024`（`src/render/mod.rs:63`），`channel()` 用 `broadcast::channel(RENDER_CHANNEL_CAPACITY)`（`:212`）。满时 tokio 的语义是**环形覆盖最旧值**、`send` 不报错；落后接收方下一次 `recv`/`try_recv` 得到 `Lagged(n)`（tokio 1.53.1 `src/sync/broadcast.rs:41-53`；本仓库对应处理见事实 8）。`send` 只在**无接收者**时返回 `Err`，而 `RenderHandle` 的所有方法都 `let _ = self.sender.send(...)`（`src/render/mod.rs:114/122/130/136/146`），即渲染器已退出时静默丢弃。约束票 01：任何「先灌历史再让 TUI 读」的通道方案都受这 1024 条环形缓冲约束。

### D. 单事件成本（问题 4）

21. `Transcript::push`（`src/render/transcript.rs:157-184`）对每条 `RenderEvent` 做一次 match，返回 `Vec<Block>`：`Delta` / `Diagnostic` / `Notice` 分支先 `flush()`（把开着的 `ToolBlock` 吐出来）再压一个新块；`Logged` 走 `push_logged`（`:195-372`）。`ToolCallStarted` 只把块挂在 `pending_tool` 上，**不产块**（`:226-240`）；`ToolCallCompleted` 配对成功也**不产块**（`:241-258`），所以一次工具调用是「两个事件 → 一个块」。约束票 01/03：`Transcript` 是有状态的流式合并器，历史块形状 = 按 `seq` 顺序喂 `Logged` 得到的块；「原样重播」在实现上就是逐条 `push`，不需要另写。

22. `push_logged` 里有一类事件被特意判为「在工具调用内部」，不 flush：`ToolCallCompleted` / `PermissionAsked` / `PermissionDecided` / `HookExecuted`（`src/render/transcript.rs:213-219`）。每条 `Logged` 至少分配一个 `Vec<Block>`（`:220-224`、`:371`）。约束票 03：权限询问与裁决、post-hook 反馈不是独立块，它们分别变成 `Block::PermissionAsked`/`PermissionDecided` 或并进 `ToolBlock.hook`——保真度以 `Transcript` 的既有口径为准，不另定。

23. `TuiState::apply` 每条事件的成本构成（`:841-879`）：一次 `transcript.push`；对产物里**每个块**：模式分支只做两次匹配（`:863-870`），`panel.observe(&block)`（`:874`），`render_block(&block)`（`:875`），然后对返回的**每一行** `pane.push(line)`（`:876-877`）。约束票 01/04：成本 = 事件数 × O(1) + 块数 × render_block + **源行数 × pane.push**；分帧预算若按事件条数计（冻结项 4 的 512 条），实际工作量仍由块与源行决定，长消息/大工具结果会让同一 512 条贵得多（见实测）。

24. `render_block`（`src/render/tui.rs:2142-2279`，`pub`）里最贵的分支是 tool：`tool_lines`（`:2303-2348`）在非空成功输出上走 `highlighted()`（`:2355-2375`），后者调 `highlight_diff` → `highlight_rust` → tree-sitter Rust 语法解析（`src/render/highlight.rs:213-218`，配置是 `OnceLock` 只建一次，`:190-206`）。assistant 正文走 `markdown::to_lines`（`:2144-2155`），是纯逐行扫描、不带语法高亮。约束票 01：单块成本的大头是**工具输出预览的 tree-sitter 高亮**，它与事件条数无关、与文本大小有关；分帧预算该按「块/源行」加权而不是只数事件。

25. `Pane::push`（`src/render/pane.rs:77-81`）三步：`lines.push_back`、`wrap_pending()`、`evict()`。约束票 01：单条源行 push 的常数成本很小，真正的非线性在 `evict`（事实 27）。

26. wrap 缓存失效与重建：`ensure(width)`（`pane.rs:178-193`）在**宽度变化**时清空 `wrapped`/`starts`、`wrapped_sources = 0` 并重绕当前所有源行，再用 `top_source` 把视口挪回去；`wrap_pending()`（`:196-211`）只绕「还没绕过的」源行（`wrapped_sources..lines.len()`）；**width == 0（首帧之前）时 `wrap_pending` 直接返回**（`:197-201`），所以首帧前堆积的源行全部欠账，第一次 `view(width)` 一次性补齐。`view` 每帧还会重绕 live 尾巴（`:89-93`）。约束票 01：重播若在首帧之前把整条日志灌进 `Pane`，wrap 是 O(总源行) 的一次性尾部成本；若在已有宽度的 live TUI 里增量灌，则每条新行当场绕（实测的 evict 表就是后者）。

27. `evict`（`pane.rs:214-242`）在 `lines.len() > CAP` 时逐条丢最老的行；每丢一条，除了 `pop_front` 还要 **`for start in self.starts.iter_mut()` 把剩余每条源行的显示行号整体下移**（`:234-236`），这是 O(`starts.len()`) ≈ O(20000) 的内层循环。`CAP = 20_000` 源行（`:23`）。注释自己写明「at most once per completed block, once the transcript is at its cap」（`:232-233`）。约束票 01：转录满 20 000 源行之后，**每多一行源行的成本是 O(20 000)**；这是「整条日志重播」最坏情况的主成本（实测见 G 节 evict 表）。

28. `evict` 不改 wrap 缓存的有效性：它按被逐行占用的显示行数 `pop_front` 掉 `wrapped` 的对应行，并同步 `top`/`total`/`seen`/`top_source`（`pane.rs:225-240`）。`sync_top_source` 用 `starts.binary_search`（`:245-250`）。约束票 04：源行被逐出后显示行号整体平移，任何「绘制时记录命中矩形」的映射会随重播漂移——票 04 的命中表必须考虑这一点。

### E. 悬空 `tool_call` 的日志形状（问题 5）

29. `--continue` 恢复时，`OpenedSession::start` 只在 `resuming` 为真时调 `agent::recover_pending_calls`（`src/lib.rs:303-312`）；`resuming` = 日志里已有 `SessionStarted`（`src/lib.rs:250-253`）。约束票 01/04：新会话不受影响；历史里那条合成结果只可能出现在被重开的日志里。

30. `recover_pending_calls`（`src/agent/history.rs:46-67`）对每个 `pending_tool_calls`（无结果的 `ToolCallStarted`）查出发起者（`started_by`，`:70-77`），然后 `emit_completed(session, render, &speaker, tool_call_id, Err(ToolError::message(INTERRUPTED)), Instant::now())`（`:56-63`）。文案常量 `INTERRUPTED` 在 `:33-35`（「the session was interrupted while this call was in flight, so its result is unknown. It was not re-run; ...」）。约束票 04：悬空调用的日志形状是**只有 `ToolCallStarted`、没有 `ToolCallCompleted`**；恢复会补一条。

31. **合成的失败结果是写进日志的，不是只在投影里**：`emit_completed`（`src/agent.rs:1959-2005`）构造 `EventPayload::ToolCallCompleted { tool_call_id, ok: false, output: None, error: Some(preview), duration_ms }`（`:1988-2004`），其中 `preview` 来自 `context::truncate_result`（`:1981-1987`）；它经 `emit` → `emit_returning`（`:2165-2178`）→ **唯一写路径** `append_event`（`:2138-2150`），后者先 `log.append(...)`（events.rs:923-938，写 JSONL 一行并 `flush`）再 `render.logged(&event)`（agent.rs:2148）。所以合成结果**同时**落进 `log.jsonl` 与渲染通道。约束票 01：「先 `read_events` 读历史」与「`assemble` 后取 `harness.events()`」拿到的历史**不一样**：前者不含合成结果，后者含（合成发生在 `opened.start`，`src/lib.rs:369`，在 `assemble` 返回之前）。约束票 04：悬空调用在重开日志里其实已经被补完，详情层看到的是 `ok: false` + `INTERRUPTED` 文本的失败结果。

32. 小注：`emit_completed` 里 `duration_ms = started.elapsed()`（`agent.rs:1967`），而恢复路径传的 `started` 是刚取的 `Instant::now()`（`history.rs:62`），所以合成结果的 `duration_ms` ≈ 0；`INTERRUPTED` 很短，`truncate_result` 在 `estimate_tokens <= max_tokens` 时直接原样返回（`src/context.rs:414-421`），**不会**写出新的 `<id>.txt`。约束票 04：合成结果没有溢出文件，详情层对它的「全文」就是事件里的那段 `INTERRUPTED`。

33. 实测佐证（本机 51 个真实会话日志）：共 6 条 `ToolCallStarted` 无对应 `ToolCallCompleted`（悬空形状真实存在，都是被杀的进程留下的 bash 调用）；日志里 `error` 含 `interrupted while this call was in flight` 的 `ToolCallCompleted` 为 **0** 条——即本机这些悬空调用**还没有被任何一次 `--continue` 恢复过**，所以看不到合成结果的实际落盘行。约束票 01/04：合成是「下一次重开时发生」的事件，不是历史日志里预先存在的。

### F. 面板与模式的重建输入（问题 6）

34. `Panel::observe`（`src/render/panel.rs:52-61`）只吃两种块：`Block::Usage { usage, .. }` → `self.total.accumulate(*usage); self.last_input = Some(usage.input_tokens);`（`:54-57`），和 `Block::TurnEnded { .. }` → `self.turns += 1`（`:58`）。其它块一律忽略（`:59`）。约束票 03：面板重建的**全部**输入就是历史里的 `UsageRecorded` 与 `TurnEnded`；逐条 `apply` 历史会自动得到与 live 同口径的 `total` / `last_input` / `turns`（`Panel` 字段私有，`panel.rs:36-39`）。

35. 事件到这两个块的映射：`EventPayload::UsageRecorded` → `Block::Usage`（`src/render/transcript.rs:348-350`）、`EventPayload::TurnEnded` → `Block::TurnEnded`（`:298-300`）。约束票 03：重播不需要为面板另设累加器；`SessionStarted` 等骨架事件不产块（`:369`）。

36. 模式的两条流上事件与 TUI 更新点：`Block::ContextInjected { source: ContextSource::PlanMode }` → `self.mode = Mode::Plan`（`src/render/tui.rs:863-865`）；`Block::History { reason: HistoryReason::ModeChange, .. }` → `self.mode = Mode::Ask`（`:866-869`）；其余块不动模式（`:870`）。写入侧：进 plan mode 时 `ContextInjected { source: PlanMode }`（`src/lib.rs:760-765`）；离开时 `retire_plan_instructions` 写一条 `HistorySuperseded { targets, reason: HistoryReason::ModeChange, .. }`（`src/agent.rs:288-297`）。约束票 03：header 模式是历史事件流的纯函数，按 `seq` 顺序重播即可；「进过又出来」的例子就是 `PlanMode` 注入后跟一条 `ModeChange`。

37. 模式的初值来自组装默认，不在 `SessionFacts` 里：`TuiState::new` 里 `mode: Mode::Ask`（`tui.rs:769` 一带，`TuiState::new` 在 `:766`），`SessionFacts` 文档明确「mid-session 会变的东西故意不在这里」（`:166-168`）。`--continue` 的组装传 `policy: Policy::for_mode(Mode::Ask)`（`src/cli.rs:359`），注释说重开会话「assembles a fresh harness and starts from the configured mode」（`src/lib.rs:164-169`）。约束票 03：历史重播**不会**把 live 模式设成 plan；只有历史最后一条模式事件是 `PlanMode` 时 `apply` 才会改成 plan，否则保持 `Ask`。注意恢复路径还会 `retire_plan_instructions` 给历史补一条 `ModeChange`（`src/lib.rs:317-324`），所以「被杀时在 plan mode」的会话重开后历史尾部恰好是 `ModeChange` → `Ask`。

### G. `outputs/` 的存续与会话目录（问题 8）

38. 会话目录形状：`<root>/<cwd-slug>/<session-id>/`，内含 `log.jsonl` + `outputs/`（`src/session/store.rs:1-11` 文档块），目录名常量 `LOG_FILE = "log.jsonl"`（`:34`）、`OUTPUTS_DIR = "outputs"`（`:36`）。`StoredSession` 结构化携带 `dir` / `log_path` / `outputs_dir`（`:46-54`）；新建时 `outputs_dir = dir.join(OUTPUTS_DIR)`（`:87-88`）；扫描已有会话时同样 `dir.join(OUTPUTS_DIR)`（`:191`）。约束票 04：`outputs/` 是会话目录的一部分，**跟随会话目录**存亡。

39. root 的位置：`sessions_dir(env)` = `$XDG_DATA_HOME/fs-agent/sessions`，否则 `$HOME/.local/share/fs-agent/sessions`（`src/config.rs:493-510`）；两个变量都没有则 `None`，CLI 直接报错（`src/cli.rs:233-236`）。本机 `XDG_DATA_HOME` 未设置，所以实测目录在 `~/.local/share/fs-agent/sessions/`。约束票 04：详情层要按 `<会话目录>/outputs/<tool_call_id>.txt` 取全文时，根位置由这个环境规则决定，不在渲染层。

40. `--continue` 路径**不删任何东西**：全路径只是 `store.latest`（`src/cli.rs:252`）→ `assemble`（`:348`）→ `OpenedSession::open` 打开日志追加（`src/lib.rs:245-249`）。`SessionStore` 的删除只有 `delete`（`remove_dir_all(&session.dir)`，`src/session/store.rs:149-152`）和它之上的 `prune`（`:158-165`）。`prune` 由 CLI 子命令 `fs-agent prune` 触发（`src/cli.rs:1666`、`:1722`）。约束票 04：**会话目录未删时 `--continue` 后 `outputs/<tool_call_id>.txt` 仍在**（没有任何代码删除或重写它）；**`prune` 之后整个会话目录连同 `outputs/` 一起消失**，`--continue` 也只能找到桶里下一个最新会话（排序见 `store.rs:199-207`）。

41. 全文落盘的确切拼法与触发条件：`truncate_result` 在 `estimate_tokens(text) > max_tokens` 时 `let pointer = outputs_dir.join(format!("{tool_call_id}.txt"))` 并 `write_owner_only`（`src/context.rs:415-427`）；**指针从不进事件**，进事件的只有 `.preview`（`src/agent.rs:1981-1987` 取 `.preview`，`EventPayload::ToolCallCompleted { output/error: Some(preview) }`，`:1988-2004`）。`Session` 侧的 `outputs_dir` = `log_path.parent()/outputs`（`src/lib.rs:231-234`），`Harness::outputs_dir()` 公开（`:855-857`），`Session::outputs_dir()` 公开（`src/session/mod.rs:224-226`）。约束票 04：详情层按 `tool_call_id` 现拼路径是可行的；「文件在 / 不在」的判定完全落在文件系统，事件里没有可用的路径字符串（虽然有 `full output at ...` 的 preview 注记，`src/context.rs:462-466`，但那是文本不是字段——这一点 tui-ux 研究事实 24-34 已详查，本票不重复）。

42. 实测的反面事实：本机 51 个会话目录里，`outputs/` 下的**文件数为 0**（逐个 `find` 过），包括最大的那个 291 事件会话。所以「重开后 `.txt` 还在」在这台机器上**没有可观测样本**，只能给源码事实（事实 40/41）。约束票 04：降级分支（文件不在）在这台机器上是**唯一可观测分支**，票 04 的表格里它不该是边角。

43. `--continue` 的 banner 会用到会话目录：`banner(..., &stored.dir.display().to_string(), parsed.resume)`（`src/cli.rs:383-389`）。`SessionFacts.cwd` 同样是 `stored.dir`（事实 16）。约束票 04：`facts.cwd` 与 `outputs_dir` 的关系是 `facts.cwd == outputs_dir.parent()`（当 `SessionFacts` 由 CLI 组装时）。

---

## H. 成本实测（问题 7）

### 生成方式（先记，可复现）

环境：本机 `HOME=/home/forty`，`XDG_DATA_HOME` 未设置 → 会话根 = `/home/forty/.local/share/fs-agent/sessions/`（与 `src/config.rs:499-510` 一致）。真实会话根下 51 个 `log.jsonl`，最大的是
`-home-forty-code-fortystory-fs-agent-986aee694bc228c4/20260922T134602Z-36f60515/log.jsonl`（410,426 B / 291 行）——本机**没有**足够大的真实会话（291 条事件、0 个 outputs 文件），所以按票面要求另给了合成基准。

真实会话：直接读上面的 `log.jsonl`，每行 `serde_json::from_str::<Event>`。

合成 50 000 事件（生成命令：`replay-bench synth 50000 /tmp/synth-50000.jsonl`，生成器源码 `.scratch/tui-history-replay/research/bench/src/main.rs` 的 `synth()`）：一个重复的 6 事件回合形状——`TurnStarted`、3 行正文的 assistant `MessageCompleted`、`UsageRecorded`、`ToolCallStarted`（`read_file`，1 行参数的 `{"path":"src/lib.rs"}`）、`ToolCallCompleted`（1 行输出）、`TurnEnded`——重复到 50 000 条（`SessionStarted` 打头）。6 事件产 8 源行，所以总源行越过 20 000 上限，会打满 `evict`。生成文件 9,492,607 B（≈9.49 MB）。

被测路径就是产品现成代码：源行数 = 逐条 `Transcript::push(RenderEvent::Logged(event))` 后对每个 `Block` 求 `render_block(&block).len()` 之和（这正是 `TuiState::apply` 里 `pane.push` 的调用次数，`tui.rs:875-877`）；耗时跑两个变体——(`apply_only`) 只逐条 `TuiState::new(facts).apply(...)`，不 draw；(`apply+draw`) 每 512 条 `apply` 完之后 `Terminal::<TestBackend>::draw(|f| draw_frame(f, &mut state))` 一次，模拟 `select!` 每轮一批 + 一帧。`TestBackend` 固定 120×40。

构建与运行：`CARGO_TARGET_DIR=/tmp/fs-agent-replay-bench-target`，`cargo build --offline` / `--release --offline`，脚本 `.scratch/tui-history-replay/research/bench/run-bench.sh`。注意本环境每次 bash 调用挂的是**私有 /tmp**，所以 build 与 measure 必须在同一个进程里跑（脚本就是这么写的）。

### 主表

| 数据集 | 事件数 | `log.jsonl` 字节 | 源行数（`Transcript`+`render_block`） | `Pane` 实际保留 | 批大小 / 帧数 | debug `apply` 仅 | debug `apply`+每批 draw | release `apply` 仅 | release `apply`+每批 draw |
|---|---|---|---|---|---|---|---|---|---|
| 真实最大会话 | 291 | 410,426 | 4,115 | 4,115（未到 CAP） | 512 / 1 | 270.3 ms（928.9 µs/事件） | 292.3 ms（1004.6 µs/事件） | 89.4 ms（307.1 µs/事件） | 93.0 ms（319.6 µs/事件） |
| 合成 50 000 | 50,000 | 9,492,607 | 66,665 | 20,000（46,665 条被逐出） | 512 / 98 | 1,102.7 ms（22.1 µs/事件） | 5,841.3 ms（116.8 µs/事件；59.6 ms/帧） | 324.3 ms（6.49 µs/事件） | 777.8 ms（15.56 µs/事件；7.94 ms/帧） |

读表要点（都是实测，不是推断）：

- 事件条数不是成本的好代理：真实会话 291 条却要 307 µs/事件，合成 50 000 条只要 6.49 µs/事件。原因是真实会话的 233 个块里有大工具结果，`render_block` 要对每个非空工具输出跑一次 tree-sitter 高亮（事实 24）；合成基准的工具输出只有一行。
- `apply_only`（不 draw、首帧前 `width==0`）在合成集上比 `apply+draw` 快 **2.4×（release）/ 5.3×（debug）**，差额主要落在 wrap 缓存建立 + 每 push 的 `evict`（事实 26/27）；在真实集上两者几乎相同（未到 CAP，wrap 总量小）。
- 冻结项 4 的「每帧 ≤512 条」在 release 下的大会话成本 ≈ **7.9 ms/帧**（合成 50 000 条 / 98 帧），debug ≈ **59.6 ms/帧**。也就是说 50 000 条事件的历史在 release 下约 **0.78 秒**铺完，debug 下约 **5.8 秒**。

### `evict` 在上限处的代价（单独 micro-bench）

方法：`Pane::new()` 先 `view(120, 40, "")` 把宽度定下来（否则 `wrap_pending` 因 `width==0` 直接返回、`evict` 只走 `wrapped_sources==0` 的廉价分支，事实 26），然后用同一行 ~60 列的源行做两次计时。baseline = `pre=0, post=20_000`（永远不超 CAP，因此零逐出）；at-cap = `pre=20_000, post=20_000`（CAP=20_000，每 push 逐出一条）。命令：`replay-bench evict 0 20000` 与 `replay-bench evict 20000 20000`。

| 条件 | debug 总耗时 / 每条 | release 总耗时 / 每条 |
|---|---|---|
| 未到上限（pre=0 → 20,000 条，零逐出） | 73.2 ms / **3.66 µs** | 14.2 ms / **0.71 µs** |
| 已在上限（pre=20,000，计时再 20,000 条，每条逐一出） | 1,913.9 ms / **95.7 µs** | 177.5 ms / **8.87 µs** |
| 逐出带来的增量 | **+92.0 µs/条（≈26×）** | **+8.16 µs/条（≈12.5×）** |

这与主表对得上：合成集 46,665 条逐出 × 8.16 µs ≈ **381 ms**，占 release `apply+draw` 与 `apply` 仅差额（777.8 − 324.3 = 453.5 ms）的约 **84%**；余下是 wrap 缓存建立 + 98 帧的固定绘制。

### ⚪ 实测层面的「未证实」（方法学）

- **合成基准不是真实分布**：它是我按「6 事件 / 8 源行」造的形状，工具输出只有一行；真实会话的成本大头（tree-sitter 高亮）在它身上被严重低估。
- **`TestBackend` ≠ 真终端**：draw 走内存 buffer，I/O 到真 tty 的耗时不在数字里；`BeginSynchronizedUpdate`/`EndSynchronizedUpdate` 那两次 `execute!` 也不在（bench 直接调 `terminal.draw`）。所以 7.9 ms/帧是**下界**。
- **未测内存峰值 / RSS**：本票没有量重播 50 000 条事件时的堆占用；只测了时间。
- **未测「历史走 1024 容量广播通道」的端到端耗时**：bench 是直接 `apply`，没有真的把 50 000 条 `send` 进 `RenderEvent` 通道再让 TUI 收。通道满的丢事件语义见事实 20，但那种方案的耗时/丢帧数本票没测。
- **未在真实 `--continue` 进程里测**：bench 用的是现成库函数，不是 `fs-agent --continue` 的完整启动路径（组装、banner、TUI 初始化都不含）。
- **并发编辑**：测量时 `src/render/tui.rs` / `layout.rs` / `wording.rs` 有未提交的 tui-ux 改动；被测的重播链（`transcript.rs` / `pane.rs` / `panel.rs` / `mod.rs`）没有改动，但 `draw_frame` 的几何已经变了（去掉 airy 行），帧成本数字对应的是这个工作树。

---

## ⚪ 未证实 / 查不到

1. **本机没有任何 `outputs/<tool_call_id>.txt` 样本**：51 个会话、`outputs/` 下 0 个文件。事实 40/41 的「`.txt` 在 `--continue` 后仍在」是源码事实（没有任何删除路径），**没有**在本机跑过一次「溢出 + 重开 + 读回」的端到端验证。
2. **`prune` 之后 `.txt` 消失**同样是源码事实（`remove_dir_all`），未在本机真实执行 `prune` 验证（会删用户数据，本票没有做）。
3. **合成结果的日志行没有实测样本**：本机 6 条悬空调用全部从未被 `--continue` 恢复过（日志里 0 条 `interrupted while this call was in flight`）。「恢复时写进日志」由 `history.rs:56-63 → agent.rs:1993-2004 → 2138-2150` 的调用链确证，但我**没有**真的重开一个含悬空调用的会话去看新行。
4. **`tokio::select!` 无 `biased` 时的精确公平性**：仓库源码里只证实「无 `biased` 关键字」；「默认在就绪分支间随机」转述自 tokio 的文档/实现（`~/.cargo/registry/src/*/tokio-1.53.1/src/sync/broadcast.rs` 附近与 `select!` 宏文档），本票没有写一个实验去测分支分布。
5. **真实会话里「最后一条事件是悬空 `ToolCallStarted`」的会话占比**：只统计到 6 条悬空调用分布在 6 个文件（每个 1 条），没有核对它们是否都是该文件的最后一条事件。
6. **`Pane` 是否被别的路径以非 `push` 方式改过**：本票只读了 `pane.rs` 的全部方法；`Pane` 的字段私有、方法都在这一个文件里，但没有穷举全仓对 `Pane` 的调用点来确认「只有 `tui.rs:876` 一处 push」。
7. **`SessionFacts` 是否在讨论之外的第三条组装路径**：grep 到两处（`src/cli.rs:320`、`:590`）和测试里若干处；测试里的构造未逐一看。
8. **重播耗时与终端真实带宽**：见 H 节末的「实测层面的未证实」。
