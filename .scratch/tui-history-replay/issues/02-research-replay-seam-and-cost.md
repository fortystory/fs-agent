# research：分帧重播的接缝事实与成本实测

Type: research
Status: resolved
Blocked by: —
Part of: ../map.md

## Question

为 `grilling：重播的接缝、分帧与顺序契约`、`grilling：保真度、面板与 header 的历史重建`、`grilling：历史详情覆盖层的复用与降级` 提供**本仓库源码级事实与一次成本实测**。

判据：**只报告事实与来源，不推荐方案、不选赢家。** 每条事实给 `文件:行号`；一手来源未写明者标 ⚪ 未证实，不做推断。

## 需要查明

1. **渲染循环骨架**：`src/render/tui.rs` 的 `tokio::select!` 有哪几路、`biased` 与否、tick 周期、脏标记与「无脏不画」的落点；`RenderEvent` 从广播到 `apply` 的完整路径。
2. **历史在组装期哪里可达**：`--continue` 路径（`src/cli.rs`）拿到什么（`StoredSession` / `Harness` / log 路径）；`EventLog` / `Session` 有没有暴露**只读事件**的公开方法；`TuiOptions` 的组装点在哪、现有字段是什么。CLI 是否已经持有全部历史事件的 `Vec`。
3. **`RenderEvent` / `RenderHandle` 的形状**：有没有「注入一条 `Logged` 事件」的公开方法；`render.logged` 的可见性与调用点；广播通道的**容量**（`broadcast::channel(N)` 的 N）与满时的行为。
4. **单事件成本**：`Transcript::push` / `Pane::push` 一次的成本构成；20 000 源行的 `evict` 代价；wrap 缓存何时失效（宽度变化 / 流式增量）。
5. **悬空 `tool_call` 在日志里的形状**：`--continue` 恢复时合成的失败结果是**写进日志**还是只在投影里合成？给出写入点或明确「只在投影」。
6. **面板与模式的重建输入**：`render/panel.rs` 的 `observe` 吃哪些块；`Mode` 在流上的两条事件（`ContextInjected{PlanMode}` / `HistorySuperseded{ModeChange}`）与 TUI 的更新点。
7. **成本实测（本机）**：在**真实会话目录**上统计 ① 事件条数 ② `log.jsonl` 字节数 ③ 用现有 `Transcript` + `Pane` 把整条日志喂进去得到的源行数 ④ 按「每帧 512 条」分批重播的总耗时（debug 与 `--release` 各记一次）。若本机没有足够大的会话，如实说明，并给一个合成基准（例如 5 万条事件）与生成方式。
8. **`outputs/` 的存续**：`--continue` 之后 `outputs/<tool_call_id>.txt` 是否还在（会话目录未删就还在？`prune` 之后呢）；会话目录的位置与 `SessionFacts.cwd` 的关系。

## 产出

`.scratch/tui-history-replay/research/02-replay-seam-and-cost.md`：

- 来源清单（本机路径 + 行号）
- 逐条事实，每条一句话说明它**约束了哪张票的哪个决定**
- 第 7 条的成本数字单独成表
- 结尾「⚪ 未证实 / 查不到」如实列出

**不写建议、不选赢家。** 答案必须自足（`/implement` 在 `/clear` 后读它）。

## 先读

- `map.md` 的 Notes（冻结项）
- `src/agent.rs` 的 `append_event`（唯一写路径）
- `src/session/`（store / ledger / observe）
- `.scratch/tui-ux/research/01-collapse-detail-data-sources.md`（同一套源码的既有事实，避免重复）

## Answer

**完整事实 + 来源行号 + 成本表在 [`../research/02-replay-seam-and-cost.md`](../research/02-replay-seam-and-cost.md)**（自足；每条事实末尾标注它约束哪张票）。这里只摘最承重的几条。

### 接缝（约束票 01）

- 渲染循环：`tokio::select!` 在 `src/render/tui.rs:220`，四路（`receiver.recv()` / `keys.next()` / `port.recv()` / `tick.tick()`），**无 `biased`**；`TICK = 120ms`（`:70`）；每轮 select 后有一个 `DRAIN_LIMIT = 4096` 的有界排空（`:251-265`），然后 `is_dirty()` 才 draw、再 `mark_clean()`（`:273-283`）。`apply` 开头置脏（`:842`）。
- 唯一写路径 `append_event`（`src/agent.rs:2138-2150`）里 `log.append` + `render.logged`（`:2148`）。全仓生产代码只有这一处 `render.logged`。
- **有公开注入方法，但没有注入进正在跑的 `Harness` 的公开路径**：`RenderHandle::logged` 是 `pub fn`（`src/render/mod.rs:129-131`），`channel()` 也是 `pub`（`:211-213`），但 `Harness` 的 `render` 字段私有且无 accessor；`Harness` 对通道的唯一公开出口是 `notice()`（`src/lib.rs:831-833`），发的是 `Notice`。
- **`--continue` 组装期不持有历史 `Vec`**：`store.latest` 只给 `StoredSession`（`id`/`dir`/`log_path`/`outputs_dir`，`src/session/store.rs:46-54`），`TuiOptions { port, facts }` 在 `assemble` **之前**组装（`src/cli.rs:320-327`）。`assemble` 之后 `Harness::events() -> Vec<Event>`（`src/lib.rs:840-842`）才有全量快照；纯只读的文件加载器是 `read_events`（`src/events.rs:959-981`，注意 `EventLog::open` 会修残尾、不是只读）。
- 通道容量 **1024**（`src/render/mod.rs:63`），满时环形覆盖最旧值、接收方下一次得到 `Lagged(n)`，TUI 把它转成 `渲染器丢弃了 N 个事件` 诊断（`src/render/tui.rs:223-225`、`:255-257`）。

### 成本（约束票 01；数字见研究文件 H 节）

- 本机最大真实会话只有 **291 事件 / 410,426 B / 4,115 源行**，且 51 个会话的 `outputs/` 下**一个 artifact 都没有**；另做了 **50,000 事件 / 9.49 MB / 66,665 源行**的合成基准（生成器与命令记录在 `.scratch/tui-history-replay/research/bench/`）。
- 每帧 512 条、release：合成 50k 条 ≈ **777.8 ms 总（7.94 ms/帧，98 帧）**，debug ≈ **5,841 ms（59.6 ms/帧）**。
- 真正的非线性在 `Pane::evict`：转录满 `CAP = 20_000` 源行后，每多一行要整体下移 `starts`（O(20,000)，`src/render/pane.rs:214-242`）。实测每行 **release 8.87 µs vs 未满时 0.71 µs（≈12.5×）**、debug 95.7 vs 3.66 µs（≈26×）。合成集 46,665 条逐出 ≈ 381 ms，占 release 差额的 ~84%。
- 单块大头是工具输出的 tree-sitter 高亮（`src/render/highlight.rs:213-218`），所以成本由**文本大小/源行数**决定，事件条数不是好代理（真实 291 条要 307 µs/条，合成 50k 条只要 6.5 µs/条）。

### 保真度输入（约束票 03）

- `Panel::observe` 只吃 `Block::Usage`（累加 + `last_input`）与 `Block::TurnEnded`（`turns`）（`src/render/panel.rs:52-61`），对应 `UsageRecorded` / `TurnEnded`。
- 模式只由两条块驱动：`ContextInjected{PlanMode} → Mode::Plan`、`History{ModeChange} → Mode::Ask`（`src/render/tui.rs:863-869`）；初值 `Mode::Ask`（`:769`），`SessionFacts` 故意不含会变的模式（`:166-168`）。
- 注意重开时恢复路径会补写事件：`recover_pending_calls` → `emit_completed`（`src/agent/history.rs:46-67`、`src/agent.rs:1959-2005`），所以「重开前读文件」与「assemble 后取 `harness.events()`」拿到的历史不同（前者缺合成结果）。

### 悬空 `tool_call`（约束票 01/04）

- 形状 = 只有 `ToolCallStarted`、没有 `ToolCallCompleted`。`--continue` 时 `recover_pending_calls` 合成的失败结果**写进日志**（`emit_completed` → 唯一写路径 `append_event` 的 `log.append`），**不是只在投影**，同时经 `render.logged` 进渲染通道（`src/lib.rs:307`、`src/agent/history.rs:56-63`、`src/agent.rs:1993-2004`、`:2147-2148`）。文案是 `INTERRUPTED`（`history.rs:33-35`），`ok:false`、`duration_ms≈0`、不落 `outputs/*.txt`。

### `outputs/` 存续（约束票 04）

- 会话目录 = `<root>/<cwd-slug>/<id>/`，含 `log.jsonl` + `outputs/`（`src/session/store.rs:1-11`、`:34-36`）；root = `$XDG_DATA_HOME/fs-agent/sessions` 否则 `$HOME/.local/share/fs-agent/sessions`（`src/config.rs:493-510`）。`SessionFacts.cwd` 装的就是 `stored.dir`（会话目录，不是工作目录，`src/cli.rs:322`）。
- `--continue` 路径不删任何东西；只有 `prune`（CLI 子命令）→ `SessionStore::delete` → `remove_dir_all(&session.dir)`（`src/session/store.rs:149-165`）。所以**未 prune 时 `.txt` 仍在，prune 后整目录连同 `outputs/` 消失**。本机无 `.txt` 样本，端到端读回未验证（见研究文件结尾的 ⚪ 清单）。

### 明确的「⚪ 未证实 / 查不到」

见研究文件末尾 8 条：本机无 `outputs/*.txt` 样本、未真跑 prune、未真恢复悬空调用、`select!` 随机公平性未实验、悬空调用是否都在文件尾部未核对、帧成本是下界（`TestBackend` 非真 tty/未测内存）等。

