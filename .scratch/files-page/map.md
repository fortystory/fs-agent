# 文件页：左栏「文件」页签（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 [`docs/agents/issue-tracker.md`](../../docs/agents/issue-tracker.md)
Charting: **已完成**（2026-10-06，五轮 grilling 共 22 问：终点形态 / 页内容 / 数据与刷新节奏 /
点击语义 / 行与树的画法 / 插入手势 / 键盘进出 / 弹窗读法与高亮 / 重扫的触发与进程模型）。
本图只做**规划**，不产代码改动。

> **交棒与收口（2026-10-06 建，2026-10-07 收）**：维护者在那轮 charting 之后直接要求折 spec，所以
> [`spec.md`](spec.md) 已经产出；同日六张实现票（05–10）全部落地，**2026-10-07 六张一并收成
> `done`**（验收逐条勾上、断言全绿）。**spec 是权威**（实现按它走）——
> [prototype：树与弹窗在两档宽度下的排版](issues/02-prototype-tree-layout.md) 没有单独跑原型：
> 它那六个可调数值在实现里已经取定，票底收成 `## 作答`（密度、焦点行、字形、弹窗列宽与空态，
> 外加窄档的正面回答），帧草图不再补。真机观感项仍列在
> [`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) ㉛。

## 目的地

一份**可执行的 spec**（交给 `/to-spec` 折成构建计划，再由 `/to-tickets` 拆实现票）：把左栏那个
到今天还只画一行占位的 **`文件` 页**定清楚 —— 页里列什么、怎么排、鼠标与键盘各做什么、文件内容
弹窗读多深、索引什么时候重扫。**走到这张图的尽头时，实现这一页不再有任何需要先决定的事**，
剩下的只是接线与断言。

**范围**：`src/render/`（页本身、`wording.rs`，必要时 `palette.rs` / `layout.rs`）、
`src/render/file_index.rs`（**只当消费方，遍历规则一个字不改**）、[`CONTEXT.md`](../../CONTEXT.md)
的相关词条、[`docs/render.md`](../../docs/render.md)、
[`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) 的锚点。
**事件 schema 不动；模型可见文本一个字节不动。**

## 笔记

- **领域**：fs-agent 的 TUI 外壳，与它那个会话级的文件索引。
- **起点**（charting 实测的三件事）：
  1. `Tab::Files` 是画出来的页签（`wording::TAB_FILES`，`tui.rs` 的 `draw_sidebar_page`），
     但它那一页只画一行 `wording::tab_placeholder()`：「此页尚未实现（另有票在跟）」。
  2. 数据源早就躺在那儿：`input-tokens` 票 01 做的**会话级 `FileIndex`**
     （`src/render/file_index.rs`，`Idle | Loading | Ready` 加 `candidates()` / `contains()`），
     票面写着「`@` 候选与**将来的文件页签**共用」。它已经接在渲染循环上：进 TUI 预热一次、
     每次提交后重扫一次，结果经 `mpsc` 回来。
  3. `tui.rs:3413` 那条注释留着**另一套**语义：「`Tab::Files` 的语义未定：这个会话碰过的文件。
     还没做」。冻结项第 1 条就是否决它。
- **要咨询的 skills**：`/prototype`（02）、`/research`（03、04）；折 spec 时 `/to-spec`。
- **每张票的答案必须自足**：`/to-spec` 会在别处的会话里读它，看不到本图与 charting 对话。

### 冻结项（charting 的 grilling 定下，票里不得重开）

理由与逐条细节在 [grilling：文件页的形态与手势](issues/01-page-shape.md)；这里只列不能动的：

1. **页内容 = 工作区文件索引**（与 `@` 同源），不是「这个会话碰过的文件」。
2. **形态是一棵树**：缩进 + 名字，目录可展开 / 收起，初始全部收起；同层目录在前。
3. **鼠标**：单击目录 = 展开 / 收起，单击文件 = 打开内容弹窗；**不给色**（层次交给结构与字形）。
4. **键盘**：点左栏任意处把键盘交给这一页（焦点行 = 点的那一行），`↑` / `↓` 移行、
   `Enter` 插入 `@路径`，`Esc` 只还键盘（**不**取消回合）。
5. **shift 手势被否决**：终端会把它当本地选择的 override 键吞掉，「shift+点击插入」不做。
6. **内容弹窗**：渲染器直接读盘 + 有界截断 + 语法高亮 + 行号；**不进事件流、不进模型上下文**。
7. **重扫**：共用那一份 `FileIndex`（`spawn_blocking` 的线程，**不**另起进程），触发点是
   「任何非只读的工具调用之后」加「提交之后」。

### 会撞的既有决定（`/to-spec` 时回改，不在本图改）

- **[`trace-tab/spec.md`](../trace-tab/spec.md) 的「明确不做」**把「`文件` 页的内容」判给了另一个
  effort —— 那个 effort 就是本图。
- **[`tui-visual-language` 票 07](../tui-visual-language/issues/07-prototype-transcript-edge-and-sidebar.md)
  的「接受的边界」**同样只碰过它的占位行外观，内容留给别处。
- **[`input-tokens/spec.md`](../input-tokens/spec.md) §1**：`FileIndex` 的遍历规则（含 2026-10-05
  那次「放行隐藏**目录**、挡回隐藏**文件**与 `.git/`」的分家）是它的契约，本图**只消费、不改**。
- **[`sidebar-toggle/spec.md`](../sidebar-toggle/spec.md)**：左栏的两档宽度与 `Ctrl-O` 意愿不动。
- **[`trace-in-main/spec.md`](../trace-in-main/spec.md)**：左栏页签只剩 `调用量` / `todo` / `文件`；
  「页签是点出来的、从不给键位」那条规矩只约束**页签切换**，不约束页内容。
- **[`docs/render.md`](../../docs/render.md) 的「选区」一节**承诺「按住 shift 的终端原生选择照旧
  可用」—— shift 手势的否决正是守它，那条承诺一个字不动。
- **[`tui-feedback/spec.md`](../tui-feedback/spec.md) §5**：左键的三段式（按下只记起点、拖动越过
  门槛才是拖选、抬起才算点击）—— 左栏页新增的点击动作要落在它上面，不另起一套。
- **[`tui-visual-language/spec.md`](../tui-visual-language/spec.md)**：语义色板与字形语法是写下来
  的决定；本图新添的字形与取色进那两张表，不新起体系。

### Tracker 事实与降级（本图适用）

- map = `.scratch/files-page/map.md`，child = `.scratch/files-page/issues/NN-*.md`；
  阻塞 = 票面 `Blocked by: NN`；claim = `Status: claimed`；resolve = 票底 `## 作答` +
  `Status: resolved` + 追加一行到本文 `已定的决定`。
- **没有 native sub-issue / 依赖边**，回退到正文约定：本文 `## 任务清单` 逐条引用子票
  （条目数 == 子票文件数），每张子票顶部写 `Part of: ../map.md`。
- 校验：`python3 scripts/wayfinder-check.py .scratch/files-page/map.md`。
- **一处偏离要说在明处**：这次 charting 时维护者就在场，23 条决定是**当场拍板**的，不是走票得来
  的。它们收进 [grilling：文件页的形态与手势](issues/01-page-shape.md) 并标了 `resolved` —— 那些
  决定确实已经做完，而「细节只住一个地方」（图是 index，不是 store）比「charting 不 resolve 票」
  更硬。除此之外的票都按常规走。

## 任务清单

<!-- 条目数必须等于 issues/ 下的子票数（scripts/wayfinder-check.py 校验）。frontier 的权威查询仍是扫 issues/ 里 open + unblocked + unclaimed 的票。 -->

**决策票**（四张）：

- [x] [grilling：文件页的形态与手势（charting 当场拍板）](issues/01-page-shape.md)
- [x] [prototype：树与弹窗在两档宽度下的排版](issues/02-prototype-tree-layout.md)
- [x] [research：详情覆盖层与语法高亮的接线](issues/03-research-overlay-and-highlight.md)
- [x] [research：重扫通知通道与有界读盘](issues/04-research-rescan-and-bounds.md)


**实现票**（`/to-tickets` 从 [`spec.md`](spec.md) 拆出的六张，2026-10-06）：

- [x] [05 — 铺垫：详情覆盖层为第二个打开方铺路](issues/05-overlay-prep.md)
- [x] [06 — 文件页画出一棵能点开的树（tracer bullet）](issues/06-files-page-tree.md)
- [x] [07 — 键盘焦点行与插 `@路径`](issues/07-keyboard-focus-and-insert.md)
- [x] [08 — 文件内容弹窗：读盘与有界截断](issues/08-file-content-overlay.md)
- [x] [09 — 弹窗里的语法高亮与行号](issues/09-highlight-and-line-numbers.md)
- [x] [10 — 工作区变了就重扫](issues/10-rescan-on-workspace-change.md)

**frontier** = 空（2026-10-07）：决策四张、实现六张全部关闭 —— 六张实现票收成 `done`（能自动化的
部分都做完；真机观感项见 [`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) ㉛），
prototype 票收成 `resolved`（问题由实现回答，见它的 `## 作答`）。

## 已定的决定

<!-- 索引：每条一行，够判断相关性即可；细节住在票里。按名字引用，不写裸编号。 -->

- [grilling：文件页的形态与手势](issues/01-page-shape.md) — charting 那 23 问的答案：这一页列的
  是**工作区文件树**（与 `@` 同一份 `FileIndex` 快照，只在「非只读的工具调用之后」与「提交之后」
  重扫，跑在 `spawn_blocking` 线程上）；**树**是缩进 + 名字、初始全收起、同层目录在前；
  鼠标单击目录展开 / 收起、单击文件开内容弹窗，**不给色**；键盘由「点左栏任意处」接管、
  焦点行即点的那一行，`↑`/`↓` 移行、`Enter` 往草稿插 `@路径`、`Esc` 只还键盘；
  滚动按指针位置归这一页；不做过滤（找具体文件仍走 `@`）；内容弹窗由渲染器**直接读盘 + 有界
  截断**、带语法高亮与行号，**不进事件流、不进模型上下文**；**shift 手势被否决**（终端会吞）。

- [research：详情覆盖层与语法高亮的接线](issues/03-research-overlay-and-highlight.md) — 覆盖层是「打开那一刻排好版的静态正文 + 一个 `top` 偏移」（`detail_body` → `folded_text` → `pane::wrap_line`，**不是** `wrap_text`），加第五个变体是三处活儿；高亮层 `highlight_code` 吃「整段源码 + 语言名」、吐逐行 span，而**按扩展名挑语言的映射今天不存在**；行号可借「先拆逻辑行、再逐条折行」的顺序在折行前插前缀（`stamp_lines` 那个块级形状不对）；**行数上限、字节上限、二进制检测在渲染层都没有现成件**（`read_file` 那套绑死在工具层），是新的活；「不进事件流、不进模型上下文」成立（渲染层对文件系统的唯一调用是一次只读 `read_to_string`，不 emit、不碰 `Session`）。另查明两处既有落差：正文排版宽度用的是**框宽**而实际文本区窄 4 列；打开详情会把**轨迹页**冻住，且「打开方恒为轨迹页」是硬编码。
- [research：重扫的触发点落在哪一层](issues/04-research-rescan-and-bounds.md) — `Effect` 只在循环那一层被算出来（`Registry::facts` 的唯一调用者，上游是 `process_call`），而收尾函数手里已经没有它（`AllowedCall` 只剩 `exclusive` 这一个投影，`ReadOnly` 与 `WritePaths` 分不开）⇒ 触发点只能落在每次 `process_call`；`ConsoleRequest` 是唯一合身的「循环→前端、静默」通道（`Muted` / `RunState` 是先例）；执行者与讨论者的调用都落在**同一条父流、同一个渲染器**上；**`/undo` 不走工具派发**（直接 `fs::write` 加一条 `HistorySuperseded`），要在它自己那条分支上单独触发；`begin()` 的守卫在「bool 置位、`Loading` 时不清位」的形状下把 N 次触发合并成 1 次且**不丢更新**（最多延后一轮），例外仍是遍历永不返回时钉死 `Loading`。

- [prototype：树与弹窗在两档宽度下的排版](issues/02-prototype-tree-layout.md) — **没有单独跑原型**：
  六个可调数值在实现里取定 —— 缩进每层 2 列、不封顶；折叠字形收起 `▸` / 展开 `▾`；焦点行取
  常驻选中（`ACCENT` + `BOLD`）；名字超宽用 `…` 收尾；两条空态文案 `正在读取工作区…` 与
  `工作区里没有文件`；弹窗按文本区宽排版、行号占「最大行号位数 + 1」列且续行顶格。

## 尚未明确

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票。不要预先切成票那么大。 -->

- **弹窗的边界值**：多少行 / 多少字节算「太大」、二进制怎么判、截断那句话怎么写 —— 没有要
  决策的分叉，只有要选一个数；留给 spec。
- **弹窗里要不要也给一条「插入 `@路径`」的出路**：树上按 `Enter` 已经能插，弹窗里再放一个是
  冗余还是顺手，还没说清。
- **焦点行与展开状态在几个边沿上的收敛**：切到别的页签、`Ctrl-O` 收起左栏、一次重扫之后
  （路径可能已经不在索引里）各自怎么办 —— 实现期说得清，不必先定。
- **索引 `Loading` / 空工作区时这一页画什么**：预热很快，但那一瞬与「工作区真的空」是两种
  意思，文案还没定。

## 明确不做

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **「这个会话碰过的文件」那套语义**：[`tui-visual-language` 票 07](../tui-visual-language/issues/07-prototype-transcript-edge-and-sidebar.md)
  时代的旧想法（`tui.rs:3413` 的注释）；要找「这一场改过哪些文件」，既有出路是
  `sessions show --files`。
- **过滤 / 模糊查找**：`@` 补全已经承担了「知道要找什么」的场合，这一页只回答「工作区长什么样」。
- **给这一页键位做页签切换**：页签仍是点出来的（`Tab` 归 `/` 菜单、`Shift+Tab` 归模式循环）。
- **`FileIndex` 的遍历规则**：忽略规则、字典序、目录带尾斜杠都是 `input-tokens` 的契约。
- **独立进程去重扫**：进程模型买不到东西，而它要重想一整套 IPC、生命周期与沙箱边界。
- **模型可见的一切**：这一页与它的弹窗都不进 `messages`、不进事件流。
- **本图的执行**：本图只产决策。「做」发生在 `/to-spec` → 实现票 → `/implement`。

## 进度

**决策 4/4；实现 6/6（2026-10-06 落地，2026-10-07 收口）。** [grilling：文件页的形态与手势](issues/01-page-shape.md) 是
charting 当场拍板的结果；两张 research 票已由子代理解决；[prototype：树与弹窗在两档宽度下的排版](issues/02-prototype-tree-layout.md)
没有单独跑原型 —— 它那六个可调数值由实现取定，票底收成 `## 作答`。spec 折出来了（[`spec.md`](spec.md)），
`/to-tickets` 又拆出六张实现票（05–10）—— 同一天六张全部落地，**2026-10-07 收成 `done`**（验收逐条
勾上、断言全绿）。实现期有两处如实记在票里的取舍（票 07 的吃键范围收窄到文件页、票 10 的端到端测试
边界），以及一处既有落差（票 05：窗口 resize 不重排**已经打开**的正文）。

六张票是一条链：**05** 是 prefactor（不修它，弹窗那张票就要在一个既有缺陷上动手），**06** 是
tracer bullet（索引 → 排版 → 绘制 → 命中 → 状态），**07 / 10** 各挂在它后面补上键盘与重扫，
**08** 接上弹窗的读法，**09** 再把弹窗里的代码画好。
