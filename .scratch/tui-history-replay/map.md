# fs-agent 重新打开会话：历史加载与详情弹窗（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 `docs/agents/issue-tracker.md`
Charting: **已完成**（2026-09-23，两轮 grilling）。本图只做**规划**，不产实现代码。
**✅ 设计部分已完成（2026-09-23）**：5 张决策票全部 resolved、`Not yet specified` 为空 ⇒ 路线 clear；交棒产物是 `.scratch/tui-history-replay/spec.md`，`/to-tickets` 已在同一 `issues/` 下切出 **4 张实现票（`06`–`09`，`Type: implement`）**。**不要再往这张图加决策票**——实现票由 `/implement` 认领，wayfinder 会话跳过它们（见 `## Notes` 与 tracker 文档）。

## Destination

一份 **spec-ready 的「重新打开会话」决策集**，交给 `/to-spec` 折叠成实现计划。

**要解决的问题**：`--continue` 重新打开一个已存在的会话时，把**历史事件加载进 TUI 转录**（分帧、可回看），并让**历史里的行与 live 用同一套详情弹窗**；同时把**信息面板**与 **header 模式**从历史重建。

**现状（票 01 已证）**：`--continue` 只把 `log.jsonl` 读进内存会话，**不重播给渲染器**——全仓库唯一的 `render.logged` 在 `src/agent.rs:2148` 的 `append_event` 里，只覆盖本进程新追加的事件。所以重开后 TUI 转录**从空开始**：看不到上一段对话，也看不到其中的「思考完成」行与工具调用行，更点不开它们的详情。

**判据**：只动 TUI 与 CLI 组装；**不改 `events` schema**；plain / headless 不动。

## Notes

- **领域**：`fs-agent` —— 自用 coding agent CLI，Rust。本图与已完成的 `.scratch/tui-ux/` 图（`TUI 使用体验与视觉效果优化`）**紧邻**：那张图定义了折叠提示行与**详情覆盖层**的形态与命中契约，本图要让**历史块**也走同一套。**tui-ux 已在 2026-09-23 落地并提交**（八张票全实现，基线 `664 passed`）——本图要贴的接缝已经是代码，见下面「已落地的 tui-ux 接缝」。
- **本图承接的既有票据**：`.scratch/tui-history-replay/issues/01-continue-history-replay.md` 原本是一张独立票（由 tui-ux 图的 `grilling：折叠与详情覆盖层的交互契约` 判出 scope 后另立）。它现在是**本图的子票 01**，`## Question` 已改写为「接缝与调度」——「要不要做 / 重播多少 / 保真度 / 详情复用 / 派生信息」这些**范围级决定已在 charting 冻死**（见下）。
- **每张票的答案必须自足**：`/implement` 会在 `/clear` 之后的新会话里读它，看不到本图与 charting 对话。
- **要咨询的 skills**：`/grilling`（HITL 票默认）、`/domain-modeling`（若引入新词）、`/research`（research 票）、`/code-review`（核对实现时）。形态若真的需要看图，用 `/prototype`（复用 `.scratch/tui-ux/prototype/` 的探针套路）。

### Tracker 事实与降级（本图适用）

- 本仓库 issue tracker = **local markdown**：map = `.scratch/tui-history-replay/map.md`，child = `.scratch/tui-history-replay/issues/NN-*.md`，阻塞 = 票面 `Blocked by: NN`，claim = 票面 `Status: claimed`，resolve = `## Answer` + `Status: resolved` + 追加到本文件 `Decisions so far`。
- 该后端**没有 native sub-issue / 原生依赖边**，所以按既定规则**回退**：本文件用 `## 任务清单` 逐条引用子票（条目数 == 子票文件数），每张子票顶部写 `Part of: ../map.md`。**阻塞关系以正文 `Blocked by:` 为权威**。
- 校验：`python3 scripts/wayfinder-check.py .scratch/tui-history-replay/map.md`（数量 / `Part of` / `Blocked by` 解析 / `closed-total`；对不上非零退出）。
- **`Status:` 与 triage 共用**：triage 状态也记在 `Status:`（canonical 角色串），category 记 `Category:` 行。**frontier 判定 = 非 `resolved`/`done`/`closed` + unblocked + unclaimed + `Type` ≠ `implement`**——`06`–`09` 是实现票，虽在同一目录、`Status: ready-for-agent`，也**不是**本图的 frontier（`docs/agents/issue-tracker.md` 已写明这条）。

### 冻结项（charting 两轮 grilling 定下，票里不得重开）

1. **只产决策**；交付物是 spec-ready 决策集，之后 `/to-spec` → `/to-tickets` → `/implement`。
2. **effort 目录复用 `.scratch/tui-history-replay/`**（不搬迁、不破坏 tui-ux 图里指向它的链接）。
3. **只覆盖现有入口 `--continue`**（取 cwd 桶里最近一个会话）。「按 id 恢复 / 从列表挑」是**新增入口**，不在本图。
4. **重播整条日志**；启动**分帧**，每帧 **≤ 512 条事件**；**进度显示在底部状态行**（临时替掉快捷键提示），完成后恢复。
5. **重播期间不接受提交**（草稿照样能打，`Enter` 不发送）；**live 事件缓冲**到重播完成后按 `seq` 顺序追加。
6. **原样重播全部块**（消息 / 工具 / 思考提示 / 权限询问与裁决 / hook / usage / 轮次分节），不做「只给人看的子集」过滤。
7. **重播同时重建信息面板**（token 累计 / 回合数）与 **header 的模式**（从 `ContextInjected{PlanMode}` / `HistorySuperseded{ModeChange}` 推）。
8. **历史详情与 live 用同一套覆盖层**：历史里的 `▸` 行同样可点；工具全文从 `outputs/<tool_call_id>.txt` 读、reasoning 从 `MessageCompleted.reasoning` 重建。
9. `--continue` 的启动 **banner 在重播完成之后**追加（作为最新一行）。
10. 不改 `events` schema、不动 plain / headless、不新增 CLI 入口。

### 已落地的 tui-ux 接缝（本图的实现前置，2026-09-23 已满足）

tui-ux 八张票**已实现并提交**（`7e437a0` / `940cd43` / `0641c57` / `b48871c` / `538c4aa` / `530a191`）。历史重播要贴的接缝现在是具体符号，不再是一张待实现的契约：

- **详情覆盖层与折叠提示行**：`src/render/tui.rs` 的 `Detail` / `DetailKind` / `DetailView` / `TuiState::open_detail` / `draw_detail` / `link_hit`。**历史行要可点，必须经 `TuiState::apply`**——它同时维护 `links`（与 pane 平行、同受 20 000 上限淘汰）、`thinking_open` / `thinking_done` 与 `colors`；绕过 `apply` 直接 `Pane::push` 的历史行**不会有 `▸` 命中**，这正是冻结项 8 要的东西。
- **提示行**：`wording::EXIT_HINT_IDLE` / `EXIT_HINT_BUSY` 与 `wording::exit_hint(busy)`；出口只有一个，`TuiState::status_line(width)`。
- **几何**：`layout::plan(area, draft_rows)`；进度行只换 `hints` 行的文案，不改几何。
- **名字配色**：`SpeakerColors`（名册来自注入的 `SessionFacts.speaker_order`），由 `apply` 内部的 `paint_block(&block, &mut self.colors)` 施加——走 `apply` 的历史自动继承。
- **票 02 成本表的一处过时**：tui-ux 把工具输出的 tree-sitter 高亮从转录路径上删掉了（工具输出改成折叠 + 详情里的纯文本），`src/render/highlight.rs` 现在**没有生产消费者**（只剩它自己的测试）。所以「单块大头是 `highlight.rs`」不再落在重播路径上，票 02 的 50 000 条成本实测是**上界**。`Pane::evict` 的 O(20 000)/行未变。
- **行号漂移**：票 02 记录的 blob 里，`pane.rs` / `transcript.rs` / `mod.rs` / `cli.rs` / `tui.rs` 都已因 tui-ux 提交而变（`agent.rs`、`lib.rs`、`events.rs`、`session/*`、`config.rs`、`context.rs`、`agent/history.rs` 未变）。引用这四个文件的行号前按函数名重新定位。

### 基线与纪律

- 验收基线：`cargo test --all-targets` 当前 **664 passed / 0 failed**（2026-09-23 实测，tui-ux 落地后）；`cargo clippy --all-targets` 干净；`cargo fmt --check` 除 `src/context/repo_map.rs` 的既有漂移外干净（**不要**顺手格式化）。
- **提交用限定路径**（`git commit -F - -- <路径>`）；改了 spec 的决定要回改 spec 正文。
- **并发实现警告（2026-09-23 解除）**：先前那个并行实现 tui-ux 的 session 已经提交完毕（见上一节）。本图后续的实现会直接改 `src/render/tui.rs` / `src/cli.rs`；引用票 02 的行号前按函数名重新定位（票 02 的 research 把每个被引文件的 blob hash 记在文首，可用 `git hash-object` 复核，漂移清单见上一节）。

## 任务清单

<!-- 逐条引用子票（决策票 + `/to-tickets` 切出的实现票）；条目数必须等于 issues/ 下的子票文件数（scripts/wayfinder-check.py 校验）。Frontier 的权威查询仍是扫描 issues/ 里 open + unblocked + unclaimed **且 `Type` ≠ `implement`** 的票。 -->

- [x] [grilling：重播的接缝、分帧与顺序契约](issues/01-continue-history-replay.md)
- [x] [research：分帧重播的接缝事实与成本实测](issues/02-research-replay-seam-and-cost.md)
- [x] [grilling：保真度、面板与 header 的历史重建](issues/03-grilling-fidelity-and-derived-facts.md)
- [x] [grilling：历史详情覆盖层的复用与降级](issues/04-grilling-history-detail-overlay.md)
- [x] [grilling：测试与验证迁移](issues/05-grilling-test-and-verification-migration.md)
- [x] [重开时把历史铺进转录（接缝、分帧与进度行）](issues/06-history-replay-seam-framing-and-progress.md)
- [x] [历史与 live 的接缝：分隔行、信息面板与 header 模式](issues/07-history-seam-divider-panel-and-mode.md)
- [x] [历史行可点：详情覆盖层的复用与四种降级](issues/08-history-detail-overlay-reuse-and-degradation.md)
- [x] [验证迁移：pty 路径、手工清单与文档回改](issues/09-verification-pty-checklist-and-docs.md)

共 **9** 张子票（**5** 张决策票全部 resolved + **4** 张实现票 2026-09-23 全部实现、`Status: done`）；实现票的依赖边 = `07 ← 06`、`08 ← 06`、`09 ← 06, 07, 08`。

## Decisions so far

<!-- 索引：每条一行，够判断相关性即可；细节住在票里，本文件不复述。按名字引用，不写裸编号。 -->

- [research：分帧重播的接缝事实与成本实测](issues/02-research-replay-seam-and-cost.md): 43 条 `file:line` 事实 + 一次成本实测。要点：① 生产代码**没有**把手能把历史注入正在跑的 `Harness`（`Harness.render` 私有、唯一公开出口是 `Harness::notice`），但 `RenderHandle::logged` 与 `render::channel()` 都是 `pub`；② `TuiOptions` 组装在 `assemble` **之前**，那时 CLI 只有 `StoredSession` 路径；组装后有 `Harness::events()`，文件侧有**无副作用**的 `read_events`（`EventLog::open` 会修复残尾、不是只读）；③ `--continue` 恢复会把悬空调用的合成失败结果**写进日志**，所以「先读文件」与「组装后取事件」看到的不是同一份；④ 重播成本由**源行**而非事件数决定，`Pane::evict` 在上限处 O(20 000)/行（release 8.87 µs vs 0.71 µs），50 000 条合成事件在 512 条/帧下 release **0.78 s**（7.94 ms/帧）、debug **5.8 s**，约 84% 的差额是逐出；⑤ 面板只吃 `Usage`/`TurnEnded`、模式只吃两条事件——重建就是逐条 `apply` 的自然结果；⑥ `outputs/` 随会话目录存亡、`--continue` 不删它，但本机 51 个会话里 **0 个 `.txt`**（降级分支是唯一可观测分支）。完整事实与 8 条 ⚪ 见 research 文件。
- [grilling：重播的接缝、分帧与顺序契约](issues/01-continue-history-replay.md): 契约 6 条——接缝 = **CLI 在 `assemble` 后经新的 `ConsoleRequest::Replay { events }`**（照 `Catalog` 先例；console 通道 unbounded 无损；**含恢复事件**；`RenderEvent`/plain/headless 不动）。分帧 = **每轮迭代一批、不进 `select!` 等 tick**（否则 120ms × 98 帧 ≈ 12 s），预算 = **512 条事件 且 ≤2000 源行**（先到先停）；完成后清态、flush 缓冲、置脏。顺序 = live 事件进 `TuiState` 的 `Vec<RenderEvent>` 缓冲，完成后按到达顺序 flush；`Enter` 拦在 `submit()` 之前；重播期间**吸底**、滚动忽略。进度 = `恢复历史 {n}/{m}` 临时替掉提示行，`40×10` 降为 `恢复中 {n}/{m}`。边界 = 重播是**一过性状态、不复用 `busy()`**：`Ctrl-C` 退出、`Ctrl-D`/`Esc` 忽略、可打印字符照常进草稿、`resize` 正常。失败 = 读失败降级为不重播 + 诊断，不阻塞启动。
- [grilling：保真度、面板与 header 的历史重建](issues/03-grilling-fidelity-and-derived-facts.md): 契约 4 条——「原样」= 逐条 `apply(Logged)`，块由 `Transcript` 唯一决定；**只有已记录事件能重播**（`Notice`/`Diagnostic` 不落日志，不重建；`SessionStarted` 不产块；合成器的 reasoning 历史里没有）。**历史分隔行**（用户选择）：重播结束、flush live 之前插一条 `Notice`，`wording::history_divider()` = `── 以上为历史 ──`，**仅当至少产出一个块**才插，顺序固定为 `[历史块] → [分隔行] → [缓冲的 banner / 诊断]`。**面板**：逐条 apply 自然累加一次（`last_input` = 末条 usage 的 input、`turns` = `TurnEnded` 数），与 live 衔接**不重置**；`context_window`/`budget_limit` 取当前配置、如实显示不调和。**模式**：以历史最后一条模式事件为准（含恢复补写的 `ModeChange`）——被杀在 plan 的会话重开后显示「询问」，与 harness 实际策略一致。**落点**：吸底（`follow=true`、`seen=total`、无指示条）。
- [grilling：历史详情覆盖层的复用与降级](issues/04-grilling-history-detail-overlay.md): 契约 4 条——**不新增形态**：历史详情就是 tui-ux 票 02/03 的覆盖层，命中沿用 tui-ux 票 04 的**绘制时当帧记录**（天然吸收 `evict` 的显示行平移）。可点范围 = 历史 `✓ 思考完成` 与工具行带 `▸`；**分隔行不可点**；**重播期间鼠标一律不响应**。工具全文的判据 = 事件文本里有没有 **`full output at <path>`** 注记：有 → 读 `<会话目录>/outputs/<id>.txt`，读不到 / 空 → 预览 + `全文不可用`；**没有注记 → 事件文本就是全文**（不误报不可用，悬空调用的 `INTERRUPTED` 结果走这条）。思考详情只来自 `MessageCompleted.reasoning`；合成器 `None` 的历史没有思考行、也就没有可点的思考详情（不伪造）。覆盖层行为与 live 完全一致（`Esc`/再点关闭、内滚、打开时视口冻结、关闭后恢复吸底）。
- [grilling：测试与验证迁移](issues/05-grilling-test-and-verification-migration.md): 分层定死——**行为进 `cargo test`**（`tests/support/` 加 session fixture：`tempfile::tempdir()` + `append(log_path, speaker, payload)`；多数断言直接构造 `Vec<Event>` 喂 `ConsoleRequest::Replay`）、**终端归属进 pty**（加 `--continue` 路径：先造会话再重开，断言不崩 / 收敛 / 退出交还干净）、**手感进手工清单**（新增 ⑫「`--continue` 重开」+ ⑦ 补「重开后退出」；**编号勘误：实际落 ⑭**，见 `## 进度`）。逐条列了：内容与顺序（`历史块 → 分隔行 → banner`）、分帧中途（部分历史 + 进度行）、面板 / 模式（含 `PlanMode`→`ModeChange` 的两个例子）、四种文件状态的详情、`Enter` 不提交与 live 缓冲顺序、空 / 1 / 512 / 513 边界、吸底。既有测试只改**编译期被逼改**的 `ConsoleRequest` match（TUI `request()` + plain 侧），现有断言预期不变、基线 **664**（tui-ux 落地后）开工前复核。spec 回改清单交给 `/to-spec`（`fs-agent-v1/spec.md`、`docs/render.md`、手工清单、pty 脚本、新文案）。实现顺序应在 `tui-ux` 实现之后。

## Not yet specified

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票。不要预先切成票那么大。 -->

<!-- 当前**没有**未指定的雾：本图 5 张票全部 resolved，原有的 `Pane::evict` 优化问题已判出 scope（见 `## Out of scope`）。 -->

## Out of scope

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **新增恢复入口**（`--resume <id>` / 从 `sessions ls` 里挑一个）：本图只覆盖现有 `--continue`。
- **历史搜索 / 过滤 / 分页浏览**：与 tui-ux 图同一条排除。
- **历史编辑 / 分叉（fork、rewind、`/undo` 跨会话）**：超出目的地。
- **plain / headless 的历史呈现**：本图只 TUI。
- **事件 schema 改动 / 逐段增量落流**：沿用既有决定。
- **鼠标悬停反馈**：与 tui-ux 图同一条排除。
- **`Pane::evict` 在上限处的 O(20 000)/行优化**（环形缓冲 / 偏移代替整体平移）：票 02 实测 release 下 8.87 µs/行，512 条/帧的 50 000 事件会话约 **0.78 s**，判为可接受，所以本图**不优化**；若实现后实测观感不够，另开 effort（不是本图的 resumption）。
- **本图的执行**：本图只产决策与 spec-ready 结论。「做」发生在 `/to-spec` → `/to-tickets` → `/implement`。

## 进度

**决策部分 100%** —— **本图完成（2026-09-23）**：5/5 张**决策票**全部 resolved、`Not yet specified` 为空 ⇒ 通往 destination 的决策已 clear。随后 `/to-tickets` 又在同一目录切出 **4 张实现票**（`06`–`09`）：它们计入 `## 任务清单` 的条目数，但**不计入本图的路线完成度**。

**下一步 = handoff，不是 build**：`/to-spec` 把 5 张票的 decisions 折成可建计划（按 `grilling：测试与验证迁移` §7 回改 `fs-agent-v1/spec.md`、`docs/render.md`、手工清单与 pty 脚本）→ `/to-tickets` → 每票一次 `/implement`（fresh session、票间 `/clear`）→ `/code-review` 双轴。**`tui-ux` 已落地（2026-09-23），本图实现可直接开始**；**历史行必须经 `TuiState::apply`**（`links`）才有 `▸` 命中。**本图不再加决策票**（实现票是实现交棒的产物，见 `## 进度` 末两段）。

**2026-09-23 handoff 已落地**：`/to-spec` 的产物是 **`.scratch/tui-history-replay/spec.md`**（`Status: ready-for-agent`，含 5 张票的全部契约 + 勘误两条），并按 §7 回改了 `fs-agent-v1/spec.md` 的 **§19**（历史重播一条）与 **§11**（恢复结果同时进转录）——顺带修掉 §19 里早已被 tui-layout 与 ADR 0002 推翻、正文却没跟的 `inline viewport` 一行。**两处勘误**：设计票说的手工清单 ⑫ 实际是 **⑭**（`tui-ux` 已占 ⑫/⑬）；设计票说的回改落点 §7 实际是 §19/§11。`docs/render.md`、`docs/tui-manual-checklist.md`、`scripts/tui-startup-check.py` 的回改**刻意留给实现落地时做**（那些文档描述的是已实现的系统）。**下一步曾 = `/to-tickets`**；它已在 2026-09-23 落地（见下）。

**2026-09-23 `/to-tickets` 已落地**：按 spec 在同一个 `issues/` 下切出 **4 张实现票 `06`–`09`**（`Type: implement`、`Status: ready-for-agent`），依赖边 `07 ← 06`、`08 ← 06`、`09 ← 06, 07, 08`——**`/implement` 的开工点 = `06`**。`07` 与 `08` 互不阻塞，但都会往同一个新测试文件里加断言，建议串行。工具宽相：`wayfinder-check.py` 的 `TYPES` 纳入 `implement`（实现票不计入 wayfinder frontier），`docs/agents/issue-tracker.md` 同步了这条与「地图设计票全结即完成」的口径。**实现票由 `/implement` 认领，wayfinder 会话不要动它们；本图的决策路线到此为止。**

**待确认**：无。五张票的决定都已在 live exchange 里由维护者拍板；剩下的只是执行。
