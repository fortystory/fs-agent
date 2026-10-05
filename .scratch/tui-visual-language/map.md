# TUI 视觉语言：语义色板与字形语法（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 [`docs/agents/issue-tracker.md`](../../docs/agents/issue-tracker.md)
Charting: **已完成**（2026-10-05，两轮 grilling 共八问：终点形态 / 美化口径 / 当前痛点 / 色板层 / 色彩策略 / 字形语法 / effort 边界 / 在跑反馈）。本图只做**规划**，不产代码改动。
**✅ 本图已完成（2026-10-05）**：十张决策票全部 `resolved`、`wayfinder-check.py` PASS ⇒ 路线 clear；下一步是**交棒**（`/to-spec`，见 `## 进度`）。**不要再往这张图加票。**

> **交棒已发生（2026-10-05 补记）**：十张票的决定已经收束成 [`spec.md`](spec.md)（`/to-spec`），状态 `ready-for-agent`，50 条用户故事。那张 spec 是决定**收束后**的样子，本图的十张票是它们**当时的推理** —— 两者都留着；冲突时以 spec 为准并回改票。spec 的测试接缝只有一个：**逐格缓冲快照**（`TestBackend` 渲染整帧 → 读 `Buffer` 断言字符与 `Style`），不新增 seam。本图至此**只作决策存档**，下一步是 `/to-tickets` 从 spec 拆实现票。

## 目的地

一份**可执行的 spec**（交给 `/to-spec` 折成构建计划）：把 TUI 里**人眼看到的样式**从散落的 `Color::` 字面量收进两层 —— 一个**语义色板**（绘制代码只引用语义名，颜色值集中一处）与一套**字形语法**（框架一套虚线、内容一套实线）—— 并把色彩**收敛**（层级交给 `dim`/`bold` 与留白，颜色只留给需要预警与分类的语义）。它同时收口一路清理：退场未删的死代码与 `docs/render.md` 的 6 处文档-代码矛盾。

**范围**：`src/render/`（`tui.rs` / `layout.rs` / `panel.rs` / `markdown.rs` / `highlight.rs` / `severity.rs` / `editor.rs` / `wording.rs`）、渲染词条（[`CONTEXT.md`](../../CONTEXT.md)）、[`docs/render.md`](../../docs/render.md)、真终端手工清单 [`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) 与 [`scripts/tui-startup-check.py`](../../scripts/tui-startup-check.py) 的锚点。**事件 schema 不动；plain / headless 的可见输出不动**（`Severity::ansi` 只作为「要不要与 TUI 收敛到同一个源」的候选被 03 号票考察）。

## 笔记

- **领域**：fs-agent 的 TUI 外壳。栈 = `ratatui` + `crossterm`，不加新 crate。
- **这次的起点**（charting 实测）：`src/render/tui.rs` 一个文件 **79 处 `Color::` 字面量**；`DarkGray` **一个色被用在 38 处（`Color::DarkGray`，含 `ratatui::style::Color::` 写法）完全不同的语义**上（旁白、未选中页签、身份行、滚动条、状态行、提示行、问卷页脚、面板标签、详情页脚……）；框架字形**三套并存**（手画 `┄`/`┆`、ratatui `LIGHT_TRIPLE_DASHED`、markdown 实线 `─`）；间距**全是手写字面量**（按钮间 3 空格、hook 缩进 2、菜单内边距 1……）。**所以「不一致」是结构问题，不是几个色值调错。**
- **要咨询的 skills**：`/prototype`（03 / 04 / 06 / 07 / 08 五张形态票）、`/grilling` + `/domain-modeling`（05 / 09 / 10）、`/research`（01 / 02）。
- **每张票的答案必须自足**：`/to-spec` 会在别处的会话里读它，看不到本图与 charting 对话。

### 冻结项（charting 的 grilling 定下，票里不得重开）

1. **destination 是一份 spec**，由 `/to-spec` 折；本图只产决策。
2. **立一个语义色板模块**：绘制代码只引用语义名，颜色值集中一处。
3. **色彩策略是收敛**：减少颜色数，层级交给 `dim` / `bold` 与留白，颜色只留给需要预警与分类的语义。
4. **字形定两档**：框架（外壳、浮层、页签、菜单）一套虚线，内容（markdown、详情小节）一套实线。
5. **范围包含清理与文档矛盾**：退场未删的死代码与 `docs/render.md` 的矛盾在本 effort 内收口。
6. **「在跑」的反馈方式重开**（10 号票）：维护者要求重新看，允许碰 `tui-input-pulse/spec.md` 里「输入区以外任何忙碌动画都不取」那条 —— 但下落短横要正面对上「真机上不好看」这条记录。
7. **范围覆盖整屏**：charting 时维护者把六类痛点全勾了（转录 / 左栏 / 底部三条 / 颜色整体 / 动画 / 覆盖层与问卷），没有排除任何区域。
8. **一条判据垫底**：这是**自用**工具的视觉打磨，不是产品化主题系统 —— 没把握的地方选简单解，别为想象出来的终端付兼容成本。

### 会撞的既有决定（`/to-spec` 时回改，不在本图改）

- **[`tui-ux/map.md`](../tui-ux/map.md) 明确不做第 3 条**：「主题 / 配色配置项与 truecolor：角色配色用现有 ratatui 命名色，不引入主题系统」。冻结项 2 的色板是**代码里的语义层**，不是用户可配的主题 —— 这条边界不破；哪张票想破必须明说。
- **[`tui-ux/map.md`](../tui-ux/map.md) 冻结项 9**：角色五色（讨论者 1 `LightCyan` / 讨论者 2 `LightMagenta` / 执行者 `LightYellow` / 用户 `LightGreen` / 系统 `Gray`）是写下来的决定，05 号票不得重开。
- **[`tui-input-pulse/spec.md`](../tui-input-pulse/spec.md)**：提示符呼吸是当前**唯一**动画；「输入区以外任何忙碌动画都不取」。10 号票会正面碰它。
- **[`tui-chrome/spec.md`](../tui-chrome/spec.md)**：外壳只剩一条竖虚线 + 两条横虚线，外框已拆；浮层几何基准（模态主列居中 vs 详情屏幕居中）是写下来的决定。**`spec.md:196` 把「状态行与转录之间贴太挤」留作改 `CHROME` 一处的理由** —— 06 号票可以取用。
- **[`trace-tab/spec.md`](../trace-tab/spec.md)**：对话视图与轨迹视图的分工、前缀形态、用户右对齐与三色前缀、轨迹页隔行底色都是刚落的决定；07 号票只动「转录右缘那两列」与左栏各页的样式，不重开分工。
- **[`usage-stats-format/spec.md`](../usage-stats-format/spec.md)**：`[ui] number_style` 是既有的显示配置节；面板的占比色条是它的产物。
- **[`docs/render.md`](../../docs/render.md)**：文档即契约，改观感必须同步；它由 [`check-doc-size.py`](../../scripts/check-doc-size.py) 管着（单元 ≤500 字符）。
- **[`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md)**：真终端观感项 **①–㉙**，观感改动要在这里加或改锚点；纯观感重点节是 ③.8 / ⑩ / ⑮ / ⑯ / ⑳ / ㉑ / ㉘.4 / ㉙，其中 ⑳.4 与 ③.8 **随终端配色而变**。

### 基线与纪律

- 验收基线（charting 时**未跑**，`/to-spec` 之前先实测一次并写进 spec）：`cargo test --all-targets`、`cargo clippy --all-targets`、`cargo fmt --check`。
- 渲染相关的既有测试（颜色与修饰符断言站点）：`tests/render_layout.rs`（42）、`tests/render_markdown.rs`（20）、`tests/render_tui.rs`（17）、`tests/wording.rs`（3）、`tests/render_highlight.rs`（3，钉 ANSI 字节）、`tests/render_plain.rs`（3，钉 `Severity::ansi`）、`tests/render_editor.rs`（1）；`tests/ask_user_question_tui.rs` / `tests/history_replay.rs` / `tests/todo.rs` 在颜色与修饰符上 **0 处**（todo 只钉字形）。pty 启动检查在 `scripts/tui-startup-check.py`：**颜色锚点 0 个**、字形锚点 3 个。逐条归类的清单见 [research：样式改动的测试与文档契约面](issues/02-research-style-change-surface.md)。
- **提交用限定路径**（`git commit -F - -- <路径>`）：本工作区出现过并行 session 全量暂存把在制品卷进无关提交。
- 改了 spec 的决定就**回改 spec 正文**，不要只写在票的评论区。

### Tracker 事实与降级（本图适用）

- map = `.scratch/tui-visual-language/map.md`，child = `.scratch/tui-visual-language/issues/NN-*.md`；阻塞 = 票面 `Blocked by: NN`；claim = `Status: claimed`；resolve = 票底 `## 作答` + `Status: resolved` + 追加一行到本文 `已定的决定`。
- **没有 native sub-issue / 依赖边**，回退到正文约定：本文 `## 任务清单` 逐条引用子票（条目数 == 子票文件数），每张子票顶部写 `Part of: ../map.md`。
- 校验：`python3 scripts/wayfinder-check.py .scratch/tui-visual-language/map.md`。宣布图走完之前必须 PASS。

## 任务清单

<!-- 条目数必须等于 issues/ 下的子票数（scripts/wayfinder-check.py 校验）。frontier 的权威查询仍是扫 issues/ 里 open + unblocked + unclaimed 的票。 -->

**决策票**（十张）：

- [x] [research：终端能力边界（dim / 虚线字形 / 真彩色 / 亮背景）](issues/01-research-terminal-capability-bounds.md)
- [x] [research：样式改动的测试与文档契约面](issues/02-research-style-change-surface.md)
- [x] [prototype：语义色板的形状与配色取舍](issues/03-prototype-semantic-palette.md)
- [x] [prototype：字形语法的两档（含浮层框）](issues/04-prototype-glyph-grammar.md)
- [x] [grilling：层级语言与选中态](issues/05-grilling-hierarchy-and-selection.md)
- [x] [prototype：底部三条（状态行 / 提示行 / 输入区）](issues/06-prototype-bottom-three-rows.md)
- [x] [prototype：转录右缘与左栏页](issues/07-prototype-transcript-edge-and-sidebar.md)
- [x] [prototype：覆盖层与问卷](issues/08-prototype-overlays-and-questionnaire.md)
- [x] [grilling：间距与对齐的常量](issues/09-grilling-spacing-and-alignment.md)
- [x] [grilling：「在跑」的反馈方式](issues/10-grilling-busy-feedback.md)

**实现票**（`/to-tickets` 从 [`spec.md`](spec.md) 拆出的九张，2026-10-05）：

- [ ] [11 — 语义色板的骨架与状态行（tracer bullet）](issues/11-palette-skeleton-and-status-row.md)
- [ ] [12 — 提示行跨整屏、左栏让位、输入区去粗](issues/12-hint-line-spans-the-screen.md)
- [ ] [13 — 转录取色、换发言者空行、严重度收敛](issues/13-transcript-colours-and-turn-gap.md)
- [ ] [14 — 左栏与转录右缘](issues/14-sidebar-and-transcript-edge.md)
- [ ] [15 — 四个浮层表面与问卷](issues/15-overlays-and-questionnaire.md)
- [ ] [16 — 内容域：markdown 与语法高亮](issues/16-content-domain-markdown-and-syntax.md)
- [ ] [17 — 字形符号表与三个间距常量](issues/17-glyph-table-and-spacing.md)
- [ ] [18 — 「在跑」：状态词的字形循环与空闲时钟](issues/18-busy-spinner-and-idle-clock.md)
- [ ] [19 — 退场死代码与文档收口](issues/19-dead-code-and-docs.md)

**决策这条路走完了**（十张全 `resolved`），实现票已从 [`spec.md`](spec.md) 拆出。

**实现 frontier** = [11 语义色板的骨架与状态行](issues/11-palette-skeleton-and-status-row.md)、[17 字形符号表与三个间距常量](issues/17-glyph-table-and-spacing.md)、[19 退场死代码与文档收口](issues/19-dead-code-and-docs.md) —— 三张无阻塞，可以立刻开；**11 是本轮的 tracer bullet**（它把「色板 → 绘制 → 测试 → 真终端」这条路径整个走通一遍）。

## 已定的决定

<!-- 索引：每条一行，够判断相关性即可；细节住在票里。按名字引用，不写裸编号。 -->

- [research：终端能力边界（dim / 虚线字形 / 真彩色 / 亮背景）](issues/01-research-terminal-capability-bounds.md) — `DIM` **没有统一语义**，方向在不同终端甚至相反（VTE/xterm ×2/3、WT ÷2、kitty 向背景混合、xterm.js 改背景），且 SGR 22 同时清掉 bold 与 dim ⇒ **层级不能压在 `DIM` 上**，承重的必须是留白、缩进与线型；外壳的 `┆`/`┄` 与浮层 `LIGHT_TRIPLE_DASHED` 在横竖上**本是同一码位**，差别只在浮层那四个实线角；发送侧**没有颜色降级**，`Gray`/`DarkGray` 实际是 `38;5;7`/`38;5;8`（文档表的 37/90 不是线缆内容）、RGB 由终端主题定；`CHROME_LINE` 对白底是 8.86:1（太黑太重）而非看不清，脆的是 DIM 与 DarkGray 系。8 条未证实项与逐条来源见报告。
- [research：样式改动的测试与文档契约面](issues/02-research-style-change-surface.md) — 89 处颜色 / 修饰符断言里只有 **9 处钉色值**（其余 80 处钉语义，换成色板常量即可）；`render_block` / `render_block_uncoloured` **没有生产调用方**（真上屏走私有 `paint_block`），改造从绘制路径下手即可；严重度的两套映射**没有直接绑定**、TUI 的 `Bad` 还比 plain 多一个 `BOLD`（收敛落点，但 `Severity::ansi` 属 plain 输出、不动）；**`tui-startup-check.py` 颜色锚点 0 个**（改色板不会让它红），它钉的是 `▄▀▀█` / `┄` / `┆` 与虚线各 ≥3 —— 改字形才会红。另修正两处 charting 事实：手工清单实为 ①–㉙、`Color::DarkGray` 在 `src/render/` 实为 38 处。`docs/render.md` 的 6 处矛盾**全部判真**、归 4 类漂移。
- [prototype：语义色板的形状与配色取舍](issues/03-prototype-semantic-palette.md) — 主路线 **「界面 A + 内容 B」**：界面域的颜色只回答「要不要注意」（`WARN` / `BAD`）与「有没有被选中」（`ACCENT`），分类交给结构与字形；内容域（markdown + 语法高亮）保留自己的分类色，**允许与界面域撞值**（代码块内部上下文明确）。界面色板定稿七条：`PLAIN`(Reset) / `MUTED`(`DarkGray` 索引 8，**静音只有一档**) / `CHROME`(Rgb 0x4a4a4a，只做装饰线) / `ACCENT`(`LightMagenta`，「被系统认出来的 / 当前聚焦的」) / `WARN`(`Yellow`) / `BAD`(`Red`)，外加冻结的角色五色与提示符专色；**`Good` 与 `Note` 归 `PLAIN`、不再有颜色**。第一刀是行内代码与语法数字不再用 `Yellow` —— 它今天同时当「代码」与「警告」用、共 11 种语义。产物：[配色表](prototype/palette.md) 与可跑的真 ANSI 对照 `prototype/palette-demo.py`。
- [prototype：字形语法的两档（含浮层框）](issues/04-prototype-glyph-grammar.md) — 调研查明外壳的 `┄`/`┆` 与浮层的 `LIGHT_TRIPLE_DASHED` **本就是同一码位**，所以越界的只有两处：**浮层的角**（今天借了内容档的实线 `┌┐└┘`）与**详情小节线**。定案：浮层四角改成**空格**（框架档字符全部来自 U+2500 块、不需自绘；代价是四角开口）、详情小节线**留实线**（分档看「这根线在分什么」，不看它在哪个容器里）；立一张**符号表**，`▸` **不拆**（todo 的「进行中」与转录的标记靠区域区分），且语义定为「**这里有折起来的东西**」，因此**不给**注入行与消息行加它。`tui-startup-check.py` 的三个锚点字符一个都没动。产物：[两档与符号表](prototype/glyph-grammar.md) 与 `prototype/glyph-demo.py`。
- [grilling：层级语言与选中态](issues/05-grilling-hierarchy-and-selection.md) — **判据一句话：过程退后、内容保持、信号着色**（依据是 `narration` 那句既有注释「用暗色，好让模型那个以全亮度渲染的回答成为显眼的东西」）；**静音只有一档**、再退只能用结构，`DIM` 不参与。**选中态收敛成两种**：常驻选中 = `ACCENT`+`BOLD`，临时光标 = `REVERSED`（问卷当前项不再用 `DIM`）；问卷「已选」与菜单文字**去掉黄色**（菜单不是警告），已选只靠 `BOLD` + `[x]`/`●`。**`BOLD` = 「这一行领起一块」**，输入区草稿的整段粗不算层级（交 06 去掉）。三处边界：`Notice` 那类 → `PLAIN`、分歧行 → `PLAIN`+`BOLD`、轮次开始 → `MUTED`+`BOLD`。**两处例外**：上下文注入行**保留专色**（`trace-tab` §2 例外二是写下来的契约，色板因此新增 `INJECTED = LightBlue`）、「N 行新内容」指示器 → `ACCENT`（黄从此只留给诊断 / hook 反馈 / `Warn`）。**并修正两处前票**（都已在原票留补记）：`Good`/`Note` 改归 `MUTED`（推翻 03 原案）、`▸` 的判据改为「谁把内容折起来谁就有」（注入行与轨迹页消息行都要，04 答）。
- [prototype：底部三条（状态行 / 提示行 / 输入区）](issues/06-prototype-bottom-three-rows.md) — 核心是**算出来**的：全部 6 条键位提示 + 出口 + 状态词要 **111 列**，而 120 列屏给提示行只有 79 列（41 列被左栏吃掉）、80 列屏只有 51 列 —— 于是 `ctrl-o 左栏`（左栏自己的开关）按设计排在最末、**永远不出现**。定案：**提示行跨整屏**，左栏从「全高」改成「到提示行为止」（**代价：左栏页区少 1 行，120×24 下 15→14；这是本图第一次推翻既有 spec 的决定** —— 要回改 `sidebar-toggle` 与 `trace-tab` 的「全高」）。状态行三段分层（标签 `MUTED` / 值 `PLAIN` / `│` 用 `CHROME`）；**不取用** `tui-chrome:196` 留的那行空白（`CHROME` 保持 4）；上下文占比**不加**阈值色（它是读数不是事件）；输入区去掉整段 `BOLD`（执行 05）。产物：[帧草图与四个待拍板](prototype/bottom-three-rows.md) 与 `prototype/bottom-rows-demo.py`。
- [prototype：转录右缘与左栏页](issues/07-prototype-transcript-edge-and-sidebar.md) — 右缘那两条并排的细竖线（滚动条轨道 + 回合条）里，**轨道不再画**（位置靠滑块表达），但 `TRAILING_COLUMNS = 2` 的预留**不动** —— 它的理由防的是「转录长高后文字重新折行」；**轨迹页补滚动条、不补回合条**（它有独立滚动，正文因此 40/28 → **38/26**）；占比条从 `bg(DarkGray)`（不占列）改成**字符条 `▓▓▓░░░░`**（占列，`panel.rs` 的列预算要重算）—— 这正是 05 说的「再退一档靠结构」；身份标记的品红坡道保留，但进色板当 `MARK_BRIGHT` / `MARK_DIM`。产物：[对照与四个待拍板](prototype/transcript-edge-and-sidebar.md) 与 `prototype/edge-and-sidebar-demo.py`。
- [prototype：覆盖层与问卷](issues/08-prototype-overlays-and-questionnaire.md) — 票面问的「三套框色统一吗」，答案是**框色只剩 `CHROME` 一个**（问卷没有框）。模态的**整块黄拆成三件事**（边框 `CHROME`+空角 / 正文 `PLAIN` / 按钮行 `ACCENT`），黄回到信号档；菜单**去黄**（正文 `PLAIN`、光标行 `REVERSED`）；详情的**发言者色从整个框缩到标题行**、边框归 `CHROME`+空角 —— 这**推翻 2026-09-23 的一条**（意图没丢：标题仍在框内第一行）；问卷**全屏唯一的 `REVERSED` 就是键盘所在**（选项区聚焦 → 选项行，输入区聚焦 → 自定义行），**不再有 `DIM`**、不再两处同时表达焦点。产物：[四表面对照](prototype/overlays-and-questionnaire.md) 与 `prototype/overlays-demo.py`。
- [grilling：间距与对齐的常量](issues/09-grilling-spacing-and-alignment.md) — 先纠正票面一处事实：**结构尺寸早有常量**（`TAB_ROWS` / `TRAILING_COLUMNS` / `SIDEBAR_TOP_GAP` / `DETAIL_PADDING`…），缺的只是**字符级**那一层 —— 所以决定 1 是**只立三个**（`INDENT` 2 格：附属行缩进；`GAP` 3 格：并列控件之间；`SEP` `" · "`：一行里的条目分隔），住 `wording.rs`，不新起体系；决定 2：**两套居中机制都留** —— 它们不是一个需求的两份实现，`centred_inset` 还担着「命中测试与绘制必须是同一份算术」，只把理由写成显式说明；决定 3：转录**换发言者时空一行**（同一人连发的块不被拆散），这是减色之后补回来的唯一结构分段手段，实现落在块序列生成期。
- [grilling：「在跑」的反馈方式](issues/10-grilling-busy-feedback.md) — **本图推翻既有决定最狠的一张**：它划掉 `tui-input-pulse/spec.md` 的两条「明确不做」——「输入区以外任何忙碌动画都不取」与「空闲时一次唤醒都没有」。定案：**状态词从提示行移到状态行末尾**（提示行不再打头状态词；状态行的降级顺序重排为「先丢模型、再丢模式，最后剩**上下文 + 状态词**」，因为「在跑」最后才该丢），并带一个**字形循环**（原选 `◐◓◑◒`；**2026-10-06 维护者改定为月相 `🌑…🌘`**，八格、Emoji、2 列 —— 见 [票 18](issues/18-busy-spinner-and-idle-clock.md) 的补记；不靠颜色、不靠 `DIM`，退路是 ASCII `|/-\`）；**空闲时也动但更慢**（运行约 0.5 秒一圈、空闲约 1.9 秒）。提示符呼吸保留，**下落短横仍不上屏**（「真机上不好看」那条记录没有被推翻）。代价写明：空闲不再零唤醒；且 `tui-input-pulse` 那条「变化的格子必须恰好是提示符两格」的断言必须改成「提示符两格 + 状态行状态词那几格」。

## 尚未明确

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票。不要预先切成票那么大。 -->

<!-- 本图走完时剩下的三条雾，去向已定： -->

- **详情覆盖层里正文的排版**：本图只覆盖了它的框架与层级；正文本身（markdown 在 135 列上限里怎么排）留给实现期按需处理。
- **浅背景终端适配的边界**：色板已选 `DarkGray` 当静音档并写下了代价；「浅色主题下可读」要不要写成验收项仍是未定项 —— 交棒时带进 spec 的「待定」。
- **`scripts/tui-startup-check.py` 与手工清单的锚点**：观感改动全部落定后才有意义，属实现期。

<!-- 一条雾已毕业：「左栏身份标记算什么」由 07 号票答清 —— 品牌标记、不参与语义体系，颜色值归色板当 `MARK_BRIGHT` / `MARK_DIM`。 -->
<!-- 一条雾已毕业：「输入区草稿整段 BOLD 是否有意」由 05 号票判定为「不构成层级」、06 号票落地去掉。 -->
- **详情覆盖层里正文的排版**：本图覆盖覆盖层的框架与层级，正文本身的排版（markdown 在 135 列上限里怎么排）没说清。
- **浅背景终端适配的边界**：色板已经选了 `DarkGray`（索引 8）当静音档并把代价写进 [它的作答](issues/03-prototype-semantic-palette.md)，[终端能力边界](issues/01-research-terminal-capability-bounds.md) 也给了低成本做法；但「浅色主题下可读」要不要写成验收项，还是只作为取值时的偏好，尚未定。
- **`tui-startup-check.py` 与手工清单要新增哪些锚点**：观感改动落定后才说得清。

## 明确不做

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **主题 / 配色配置项与 truecolor**：[`tui-ux/map.md`](../tui-ux/map.md) 明确不做的第 3 条仍在；色板是代码里的语义层，不是运行期可配的主题。
- **角色五色的重开**：[`tui-ux/map.md`](../tui-ux/map.md) 冻结项 9 定的五色不动。
- **plain / headless 的呈现改动**：本图判据是只动 TUI；管道化输出保持逐行、无颜色。
- **下落短横（`▀`）接回渲染路径**：[`tui-input-pulse/spec.md`](../tui-input-pulse/spec.md) 的决定；10 号票若重新考虑它，必须正面对上「真机上看下来不好看」这条记录，而不是顺手接回。
- **`文件` 页签的内容**：[`trace-tab/spec.md`](../trace-tab/spec.md) 已判给另一个 effort。
- **键位、手势与命中行为**：本图只改视觉表达，不改键盘与鼠标行为。
- **本图的执行**：本图只产决策与 spec-ready 结论。「做」发生在 `/to-spec` → 实现票 → `/implement`。

## 进度

**决策 10/10（2026-10-05）。** 十张决策票全部 resolved，`wayfinder-check.py` PASS。**交棒已完成**：决定收束成 [`spec.md`](spec.md)，`/to-tickets` 又拆出九张实现票（11–19）。

十张票的决定是咬合的：**01 / 02** 两张调研给了事实底线（`DIM` 不可承重、颜色不降级、89 处断言里只有 9 处钉色值）；**03**（色板）与 **04**（字形）给了词汇；**05**（层级）把它们落成一句可判定的判据；**06 / 07 / 08** 三张形态票把判据铺到屏幕的每一块；**09** 收了间距；**10** 关了最后一条。中途有三处**回头修正**前票（`Good`/`Note` 的归属、`▸` 的判据、两处既有 spec 的决定），每一处都在原票留了补记，没有静默覆盖。

**下一步 = 交棒**：`/to-spec` 把互链的决定收束成 `spec.md`，再由 `/to-tickets` 拆实现票；此后每票一次 `/implement`（fresh session、票间 `/clear`），收尾走 `/code-review` 双轴。实现期要回改的既有 spec（`sidebar-toggle` 的全高、`tui-input-pulse` 的两条明确不做、`usage-stats-format` 的占比条、`docs/render.md` 的若干节）已在各票「给下游 / spec 的落点」里逐条列出。
