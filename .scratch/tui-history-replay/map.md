# fs-agent 重新打开会话：历史加载与详情弹窗（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 `docs/agents/issue-tracker.md`
Charting: **已完成**（2026-09-23，两轮 grilling）。本图只做**规划**，不产实现代码。

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
- **`Status:` 与 triage 共用**：triage 状态也记在 `Status:`（canonical 角色串），category 记 `Category:` 行。**frontier 判定 = 非 `resolved`/`done`/`closed` + unblocked + unclaimed**。

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

<!-- 逐条引用子票；条目数必须等于 issues/ 下的子票文件数（scripts/wayfinder-check.py 校验）。Frontier 的权威查询仍是扫描 issues/ 里 open + unblocked + unclaimed 的票。 -->

- [ ] [grilling：重播的接缝、分帧与顺序契约](issues/01-continue-history-replay.md)
- [x] [research：分帧重播的接缝事实与成本实测](issues/02-research-replay-seam-and-cost.md)
- [ ] [grilling：保真度、面板与 header 的历史重建](issues/03-grilling-fidelity-and-derived-facts.md)
- [ ] [grilling：历史详情覆盖层的复用与降级](issues/04-grilling-history-detail-overlay.md)
- [ ] [grilling：测试与验证迁移](issues/05-grilling-test-and-verification-migration.md)

共 **5** 张子票，当前 **1 resolved / 4 open**。

## Decisions so far

<!-- 索引：每条一行，够判断相关性即可；细节住在票里，本文件不复述。按名字引用，不写裸编号。 -->

- [research：分帧重播的接缝事实与成本实测](issues/02-research-replay-seam-and-cost.md): 43 条 `file:line` 事实 + 一次成本实测。要点：① 生产代码**没有**把手能把历史注入正在跑的 `Harness`（`Harness.render` 私有、唯一公开出口是 `Harness::notice`），但 `RenderHandle::logged` 与 `render::channel()` 都是 `pub`；② `TuiOptions` 组装在 `assemble` **之前**，那时 CLI 只有 `StoredSession` 路径；组装后有 `Harness::events()`，文件侧有**无副作用**的 `read_events`（`EventLog::open` 会修复残尾、不是只读）；③ `--continue` 恢复会把悬空调用的合成失败结果**写进日志**，所以「先读文件」与「组装后取事件」看到的不是同一份；④ 重播成本由**源行**而非事件数决定，`Pane::evict` 在上限处 O(20 000)/行（release 8.87 µs vs 0.71 µs），50 000 条合成事件在 512 条/帧下 release **0.78 s**（7.94 ms/帧）、debug **5.8 s**，约 84% 的差额是逐出；⑤ 面板只吃 `Usage`/`TurnEnded`、模式只吃两条事件——重建就是逐条 `apply` 的自然结果；⑥ `outputs/` 随会话目录存亡、`--continue` 不删它，但本机 51 个会话里 **0 个 `.txt`**（降级分支是唯一可观测分支）。完整事实与 8 条 ⚪ 见 research 文件。

## Not yet specified

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票。不要预先切成票那么大。 -->

- **`Pane::evict` 在上限处的 O(20 000)/行是否值得优化**（环形缓冲 / 偏移代替整体平移）：票 02 实测它是重播与 live 的每行主成本（release 8.87 µs vs 0.71 µs）。它是否在目的地内，取决于 `grilling：重播的接缝、分帧与顺序契约` 对预算与可接受性的判定——若判为不够就毕业成票，否则记为已知代价。
- **重播期间的滚动交互**：历史还没铺完时用户上滚、以及重播完成后是否强制吸底——票 01/03 若给不出确定答案，就先以「吸底、不被上滚打断」为默认。

## Out of scope

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **新增恢复入口**（`--resume <id>` / 从 `sessions ls` 里挑一个）：本图只覆盖现有 `--continue`。
- **历史搜索 / 过滤 / 分页浏览**：与 tui-ux 图同一条排除。
- **历史编辑 / 分叉（fork、rewind、`/undo` 跨会话）**：超出目的地。
- **plain / headless 的历史呈现**：本图只 TUI。
- **事件 schema 改动 / 逐段增量落流**：沿用既有决定。
- **鼠标悬停反馈**：与 tui-ux 图同一条排除。
- **本图的执行**：本图只产决策与 spec-ready 结论。「做」发生在 `/to-spec` → `/to-tickets` → `/implement`。

## 进度

**20%** —— 已 resolved：`research：分帧重播的接缝事实与成本实测`（事实与实测见 `.scratch/tui-history-replay/research/02-replay-seam-and-cost.md`）。**1/5 resolved**。

**下一步**：前沿 = `grilling：重播的接缝、分帧与顺序契约`、`grilling：保真度、面板与 header 的历史重建`、`grilling：历史详情覆盖层的复用与降级`（三张已随票 02 解锁，可并行）。最后是 `grilling：测试与验证迁移`。解票时先 `Status: claimed`，答案落 `## Answer` + `Status: resolved`，并把 gist 追加到 `Decisions so far`。

**待确认（未到 95%，不得 close）**：重播接缝的归属（`TuiOptions` 注入 vs 新增 `Harness` accessor）与预算是否按源行加权，都要在票 01 里定；票 02 的实测数字（`evict` 的 O(20 000)）已作为输入交进去。
