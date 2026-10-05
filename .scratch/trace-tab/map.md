# 轨迹视图与对话视图的分工（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 [`docs/agents/issue-tracker.md`](../../docs/agents/issue-tracker.md)
Charting: **已完成**（2026-10-05，两轮 grilling 共十四问：终点 / 分工 / 几何 / 降级 / 交互 / 历史 / 术语 / 例外清单 / 讨论归属 / 执行者 / 两个视口 / 范围 / 键盘 / 页高）。本图只做**规划**，不产代码改动。

## 目的地

一份**可执行的 spec**（交给 `/to-spec` 折成构建计划）：把 TUI 的转录拆成两个视图 —— 左栏 `轨迹` 页升格为**轨迹视图**、画**全量块**；主列转录收窄为**对话视图**、只画用户文本 + assistant 正文 + 四类例外。它要同时定下三个实现面：两个 `Pane` 共享 `painted` 源的切片、左栏页高从「用量字段数」解耦、轨迹视图自己的滚动与跟随语义。

**范围**：`src/render/`（`tui.rs` / `layout.rs` / `pane.rs` / `transcript.rs` / `wording.rs`）、渲染词条（`CONTEXT.md`）、[`docs/render.md`](../../docs/render.md)、手工清单与 [`scripts/tui-startup-check.py`](../../scripts/tui-startup-check.py) 的锚点。**事件 schema 不动。**

## 笔记

- **领域**：fs-agent 的 TUI 外壳。左栏现状（页签条、两个占位页、宽度两档、页高 = 用量字段数）见 [`tui-sidebar/spec.md`](../tui-sidebar/spec.md) §3 与 [`docs/render.md`](../../docs/render.md)；块层见 [`src/render/transcript.rs`](../../src/render/transcript.rs)；滚动结构见 [`src/render/pane.rs`](../../src/render/pane.rs)。
- **术语**（冻结项 7）：`转录（Transcript）` 仍是「事件流 → 展示单元的**共享层**」，不动；本 effort 新增两个词 —— **轨迹视图（Trace）**（左栏 `轨迹` 页，画全量块）与**对话视图（Conversation）**（主列那块区域，只画对话）。词条已进 `CONTEXT.md`。
- **要咨询的 skills**：`/prototype`（三张 prototype 票）、`/grilling` + `/domain-modeling`（滚动语义那张）、`/research`（共享 `painted` 那张）。
- **每张票的答案必须自足**：`/to-spec` 会在别处的会话里读它，看不到本图与 charting 对话。

### 冻结项（charting 的 grilling 定下，票里不得重开）

1. **destination 是一份 spec**，由 `/to-spec` 折；本图只产决策。
2. **分工**：轨迹视图 = **全量块**（含用户 / 助手的消息本身）；对话视图 = 用户文本 + assistant 正文 + 例外清单（第 8 条）；assistant 的**思考归轨迹**；讨论者 / 执行者 / 合成器的输出归属见第 9、10 条。
3. **列宽不动**：轨迹视图就在 40 列（宽档）/ 28 列（窄档）里画，长内容截断，**看全文走详情覆盖层**（仍居中主列）。
4. **左栏不可见时对话视图退回全量**：`w < 80` 或 `Ctrl-O` 收起时，那些块仍要有地方显示。
5. **轨迹视图有自己的滚动与贴底跟随**，点行能开详情覆盖层。
6. **历史重播跟着长出来**：`--continue` 的重播与实时事件走同一条 push 路径，不新增管道。
7. **术语**见上一条「术语」。
8. **对话视图的例外清单**：错误（`AgentError` / `SessionError`）、会话中断（`SessionEnded`）、权限裁决（`PermissionAsked` / `PermissionDecided`）、hook 拒绝（`Hook` 的失败）—— 这四类**留在对话视图**，其余过程行进轨迹。**这只是起点**：`Block::Notice` 那一类（命令回执、启动横幅、错误报告、目标与重试提示）显然是说给用户听的，最终清单由 [grilling：对话视图的保留清单](issues/06-grilling-conversation-view-keep-list.md) 定。
9. **讨论会话**：**讨论者的发言留在对话视图**（它们是说给人听的），讨论的**过程行**（轮次边界、divergence 之类）进轨迹；合成器的结论留在对话视图。
10. **执行者**：`ExecutorSpawned` / `ExecutorFinished` 与执行者自己的回合**全归轨迹**。
11. **两个视口共享同一个源**：源是宽度无关的 `painted`（`Block` 列表），**两个 `Pane`** 各持折行缓存与滚动位置，**同步裁剪** —— `links` / `turn_rail` 继续与源行下标一一对应。`RenderedLine` **不能**当共享源（markdown 排版在 push 前就按宽度做了）。
12. **范围**：四条排除项见 `明确不做`。
13. **键盘**：轨迹视图**只吃滚轮**；`PageUp` / `PageDown` / `Ctrl-G` 仍归对话视图。这个代价写进 spec。
14. **页高**：`sidebar_page.height` 从 `fields` 改成**撑满左栏剩余高度**（120×24 → 15 行、80×24 → 19 行、80×10 → 6 行）；调用量页仍只画它的 6 个字段。

### 会撞的既有决定（`/to-spec` 时回改，不在本图改）

- **[`tui-sidebar/spec.md`](../tui-sidebar/spec.md) §2 / §3**：页高那条写的是 `fields`（「高度定内容」），冻结项 14 把它解耦 —— spec 里那两句要改。它「明确不做」里的「`轨迹` 与 `文件` 两个 tab 的内容，单独开票」正是本 effort，`文件` 仍留给另一个 effort。
- **[`docs/render.md`](../../docs/render.md)**：左栏那几段（页签条、页高、占位页）要跟着改；它是 [`check-doc-size.py`](../../scripts/check-doc-size.py) 管的 37 份之一。
- **[`scripts/tui-startup-check.py`](../../scripts/tui-startup-check.py)**：启动帧的锚点要跟着左栏改（宽档 40 列画 mark 那条不变，但页高与占位页会变）。

### Tracker 事实与降级（本图适用）

- map = `.scratch/trace-tab/map.md`，child = `.scratch/trace-tab/issues/NN-*.md`；阻塞 = 票面 `Blocked by: NN`；claim = `Status: claimed`；resolve = 票底 `## 作答` + `Status: resolved` + 追加一行到本文 `已定的决定`。
- **没有 native sub-issue / 依赖边**，回退到正文约定：本文的 `## 任务清单` 逐条引用子票（条目数 == 子票文件数），每张子票顶部写 `Part of: ../map.md`。
- 校验：`python3 scripts/wayfinder-check.py .scratch/trace-tab/map.md`。宣布图走完之前必须 PASS。

## 任务清单

<!-- 条目数必须等于 issues/ 下的子票数（scripts/wayfinder-check.py 校验）。frontier 的权威查询仍是扫 issues/ 里 open + unblocked + unclaimed 的票。 -->

**决策票**（wayfinder 的六张，全部 `resolved`）：

- [x] [prototype：轨迹视图在 40/28 列里的排版与密度](issues/01-prototype-trace-page-layout.md)
- [x] [prototype：对话视图瘦身之后的形态](issues/02-prototype-conversation-view.md)
- [x] [prototype：页高撑满之后左栏三页的形态](issues/03-prototype-sidebar-page-height.md)
- [x] [grilling：两个视图的滚动与跟随语义](issues/04-grilling-scroll-and-follow.md)
- [x] [research：两个 Pane 共享 `painted` 的改造面](issues/05-research-shared-painted-two-panes.md)
- [x] [grilling：对话视图的保留清单（`Notice` 那一类）](issues/06-grilling-conversation-view-keep-list.md)

**实现票**（`/to-tickets` 从 [`spec.md`](spec.md) 拆出的八张，2026-10-05）：

- [x] [07 — 把裁剪记账归还给 pane（prefactor）](issues/07-pane-evict-accounting.md)
- [x] [08 — 左栏页高撑满](issues/08-sidebar-page-fills-height.md)
- [ ] [09 — 轨迹视图第一次活起来（tracer bullet）](issues/09-trace-page-alive.md)
- [ ] [10 — 对话视图的过滤与形态](issues/10-conversation-view-filter.md)
- [ ] [11 — 降级：左栏不可见时对话视图退回全量](issues/11-fallback-when-sidebar-hidden.md)
- [ ] [12 — 轨迹页按轮次隔行底色](issues/12-trace-round-stripes.md)
- [ ] [13 — 详情按视图还原](issues/13-detail-returns-to-opener.md)
- [ ] [14 — 收口：文档、脚本锚点与全量复核](issues/14-close-out.md)

**决策这条路走完了**（六张 `resolved`）。实现票的 **frontier 是 [07](issues/07-pane-evict-accounting.md) 与 [08](issues/08-sidebar-page-fills-height.md)** —— 两张都能立刻开；其余各被自己的 blockers 挡着，依赖边看每张票的抬头。

## 已定的决定

<!-- 索引：每条一行，够判断相关性即可；细节住在票里。按名字引用，不写裸编号。 -->

- [research：两个 Pane 共享 `painted` 的改造面](issues/05-research-shared-painted-two-panes.md) — 每 pane 一个绘制宽度与一套平行表（`links`/`drawn_rows`；rail 只绑对话 pane），pane 自己的 `evict` 当裁剪记账的唯一权威，`rerender_if_width_changed` 拆成按目标掩码各自 clear + 重放同一份 `painted`；`live` 由每个 pane 的 `view` 各自折，无需新缓存；过滤接上后详情入口/提示行/思考与工具行的既有测试大面积要改。
- [prototype：页高撑满之后左栏三页的形态](issues/03-prototype-sidebar-page-height.md) — 页区高度 = **内容行 − 身份 − 页签条**，内容**贴顶**；`SIDEBAR_FIELDS = 6` 退休、阶梯地板改用 `SIDEBAR_MIN_FIELDS = 3`。实测 120×24 页高 6→15、80×24 6→19，而极矮档（120×10 / 80×10）反而**保住身份行**、页区 6→5（丢「缓存」）。调用量页与 todo 页**一行代码不改**（前者本来就贴顶 + 尾裁，后者已经按 `area.height` 自适应）；帧见 [`prototype/frames.txt`](prototype/frames.txt)。
- [prototype：轨迹视图在 40/28 列里的排版与密度](issues/01-prototype-trace-page-layout.md) — 消息正文只画**首行 + `…`**（新增 `DetailKind::Message`）；前缀分档（宽档 `[名字] `、窄档去方括号）；markdown 照排。工具 / 思考 / 注入 / 全部叙述行**今天就已经各占一行**，所以真正要决定的只有消息正文 —— 帧见 [`prototype/trace-pages.txt`](prototype/trace-pages.txt)。**示例之后维护者追加：轨迹页按轮次隔行底色**（单位级、只在轨迹视图，见 [`prototype/split-view.txt`](prototype/split-view.txt)）。
- [grilling：两个视图的滚动与跟随语义](issues/04-grilling-scroll-and-follow.md) — 两个视图的滚动状态**完全独立**（切页保留位置、各自的 `follow` / `fresh`、轨迹页也画「回到最新」指示器）；裁剪是**每个 pane 与它自己的平行表同步**、两个窗口不要求相同，`painted` 照旧全量保留；**不做跨视图联动**；详情关掉后**还原打开前的位置**（打开前在回看时不再被弹到底部）—— 既有测试 `tests/history_replay.rs:1044-1055` 要跟着改。
- [grilling：对话视图的保留清单（`Notice` 那一类）](issues/06-grilling-conversation-view-keep-list.md) — 保留清单是**枚举的六类**：用户与 assistant 正文、错误（`AgentError` / `SessionError`）、会话中断、权限裁决、失败的 `Hook`、**`Notice` 整类**（命令回执、启动横幅、错误报告、目标与重试提示、历史分隔线）。口径是「先整类留下看效果」，判据式收紧留给日后；代价（目标提醒那类过程性提示也会留在对话里）已显式写下。
- [prototype：对话视图瘦身之后的形态](issues/02-prototype-conversation-view.md) — **形态一律照旧、只做过滤**：回合 / 轮次边界行留着当分段线，speaker 标签与颜色不动，`▸` / `⋮` 照旧，错误行不为它新增显眼规则，rail 段首规则不变，合成器结论照旧。实现是「一条过滤 + 照旧渲染」；**示例之后维护者追加了两处例外 —— 用户消息在对话视图里右对齐、三类前缀各一色**（见 [`prototype/split-view.txt`](prototype/split-view.txt)）。本票落地时没有 prototype 产物，这两条来自示例反馈。

## 尚未明确

- **轨迹视图里块的骨架与分组**：要不要按回合分段，要不要序号或时刻。
- **未读提示**：轨迹页在别的页时（它不在屏幕上），新内容要不要在**页签**上提示 —— 轨迹页**内部**那个「N 条新行」指示器已由[两个视图的滚动与跟随语义](issues/04-grilling-scroll-and-follow.md)定下。
- **搜索 / 过滤**：只看工具调用？只看某个文件碰过的行？
- **`tui-startup-check.py` 与手工清单**要新增哪些锚点（左栏轨迹页在脚本里的第一帧）。

## 明确不做

- **`文件` tab 的内容**：另一个 effort。
- **plain / headless 渲染器**：它们没有左栏，拆分是 TUI 专属。
- **`sessions replay` / `sessions show` 的输出**：不动。
- **键位表新增**：`Tab` 仍归 `/` 菜单、`Shift+Tab` 仍归模式循环；冻结项 13 的键盘代价照单接受。
- **左栏的宽度档与 `Ctrl-O` 意愿那一层**：[`sidebar-toggle/spec.md`](../sidebar-toggle/spec.md) 的契约不动（只有页高要改，列宽不改）。
- **讨论会话 rail 的「轮次」语义**：照旧。
- **轨迹视图的持久化**：它只是视图，不进事件流、不落盘、不跨会话。
- **跨视图联动**（点轨迹里的某行把对话视图滚到那一回合，或反向）：[两个视图的滚动与跟随语义](issues/04-grilling-scroll-and-follow.md) 明确不做；从对话跳回某回合仍走**回合条**。

## 进度

**决策 6/6，实现 2/8**（2026-10-05）。六张决策票全部 resolved，并已折成 [`spec.md`](spec.md)（`/to-spec`）；`/to-tickets` 从 spec 拆出八张实现票（07–14），frontier 现在是 [09 — 轨迹视图第一次活起来](issues/09-trace-page-alive.md)（它的两个 blocker [07](issues/07-pane-evict-accounting.md) 与 [08](issues/08-sidebar-page-fills-height.md) 都已落地）。决策票的产物：`prototype/frames.txt`、`prototype/trace-pages.txt`、`prototype/split-view.txt`、`research/01-pane-and-two-view-feasibility.md`、`research/02-shared-painted-change-surface.md`。**保留清单那张是 charting 收尾时从 research 的测试清点里长出来的** —— `Block::Notice` 承载命令回执、错误报告与启动横幅，按冻结项 2 + 8 字面执行会把它挤出对话视图。**下一步由 `/implement` 认领 frontier 那两张。**
