# `.scratch/`：feature 索引

本仓库的 issue tracker 就是这里的 markdown（约定见 [`docs/agents/issue-tracker.md`](../docs/agents/issue-tracker.md)）：

- **一个 feature 一个目录**：`.scratch/<feature-slug>/`；
- **spec** 在 `spec.md`（一次 grilling 或 wayfinder 折出来的构建计划）；**决策图**在 `map.md`（wayfinder effort：票是**决策**，不是交付物）；少数目录只有 `seed.md`（种子材料，还没变成 spec）；
- **票**在 `issues/NN-<slug>.md`，**一票一个文件**，每票开头有 `Type:` 与 `Status:`（`ready-for-agent` / `done` / `ready-for-walkthrough`；wayfinder 的决策票是 `claimed` / `resolved`），`Blocked by:` 记阻塞边；
  - **`ready-for-walkthrough`**（2026-10-02 立）：这票能自动化的部分已经落地 —— 代码、测试、pty 脚本与手工清单 —— **剩下的只有人在真终端里逐项走查**。写 `done` 会把那一步说成已经做过，写 `ready-for-agent` 又看不出「等人」。它**不是分诊标签**：`docs/agents/triage-labels.md` 里的 `ready-for-human` 说的是「必须由人动手实现」，那是另一回事，别把这两个词换着用。
- **顺序**：blockers 先做；每票自包含，所以做完一票就可以把它的 context 丢掉。

下表由 `ls` / `grep '^Status:'` / `head -1` 核过（2026-09-26；`todo-and-modes` 的票数与状态在这一轮收尾时改过一次；`language-migration` 那一行按 2026-09-27 两张票的落地更新过，2026-09-30 补票 03、并把票 04 记成 `ready-for-agent`；2026-10-01 `sandbox` 由 seed 折成 spec、新增 `workspace-mode` 种子，同日五张票落地并改成 `5/5 done`；同日 `workspace-mode` 的种子也折成 spec、拆出两张票并双双落地；同日 TUI 外壳做了减法，`tui-chrome` 的八张票一次落地 —— 第一轮六张（去外框、虚线化、详情居中、按位置滚动），随后一轮真机反馈又补了两张（左栏身份下移一行、横线不再截断竖线）；2026-10-01 新增 `markdown-render`——一次 `/ask-matt` → `/research` ×2 → `/grilling` 的会话，spec 与七张票同日落地文件，紧接着同日落地实现（01–06 是代码、07 是文档收口，全部 `done`）；同日一轮 `/ask-matt` 记下 16 条只有 `seed.md` 的意向（需求池，见下表）；**同一轮讨论的后半段**又新建了 `goal-loop` —— 从「缺不缺一个跨会话的任务功能」一路走到「目标 + 翻页 + 压缩」，并把 `loop-and-goals`、`clear-command`、`context-compaction` 三条种子移交过去（那三条只留指向新目录的记录）；随后**同一轮 `/grill-with-docs`** 走完 30 个问题、关掉 14 条分叉，`goal-loop` 当天折成 [`spec.md`](goal-loop/spec.md) 并拆出 15 张票（`issues/01`–`15`，依赖边只向后、已核过）；**同一天 15 张全部落地**，那条难回头的决定（目标不进会话状态、进度从 `todo` 派生）记成 [ADR 0009](../docs/adr/0009-goals-are-files-and-progress-is-derived.md)，逐面文档是 [`docs/goals.md`](../docs/goals.md)。同日紧接着的一轮 `/ask-matt` → `/grill-with-docs` 把需求池里的三条（`usage-stats-format` / `terminal-title` / `exit-gesture`）折成了 spec —— 三份 `spec.md` 同日落文件，并各拆出票（`3` + `3` + `5`，共 11 张 `ready-for-agent`，依赖边只向后）。**2026-10-02 这 11 张全部落地**：9 张 `done`，2 张标 `ready-for-walkthrough`（`terminal-title` 票 03 与 `exit-gesture` 票 05 剩下的都是「在真终端里逐项看一遍」——能自动化的两条腿都绿，`scripts/tui-startup-check.py` 那轮在忙碌双击上还顺手挖出并修掉了两个缺陷）。同一天又补了一个小 feature：`continue-by-id` —— `-c` 吃一个可选的 id（`-c <id>`，另有显式拼写 `--session <id>`），指名的那一场在别的工作区时这一趟的工作目录跟着它走；一张票，同日落地。同日再一轮 `/ask-matt` → `/grill-with-docs` 把「让 fs-agent 支持 ctrl-z 挂起到后台」折成 [`suspend-gesture/spec.md`](suspend-gesture/spec.md)：`Ctrl-Z` = **真暂停**（SIGTSTP）—— 单下生效、任何视图都拦不住、`fg` 回来重新进 alt screen 并全量重绘；plain 的同一按键由终端驱动天然处理，用一条 pty 回归与文档钉住；停着的那段时间里子进程与各种 deadline 照旧走，所以「想连它们一起停」的动作是先 `Ctrl-C` 再 `Ctrl-Z`。九问的存档在 [`seed.md`](suspend-gesture/seed.md)，同一轮拆出两张票、**同日两张都落地**：`Ctrl-Z` 单下停到后台（任何视图都拦不住，`raise` 同步发信号所以「先交还、再停」在时间上成立），`fg` 回来重进 alt screen 并清屏全量重绘；pty 脚本加了 TUI 与 `--plain` 两条挂起路径 —— 挂起的测试得先给 pty 一个真会话与前台进程组，`pty.fork()` 的孤儿组会把 SIGTSTP 直接丢掉。逐面文档是 [`docs/render.md`](../docs/render.md) 的「挂起与恢复」，真机走查是 [`docs/tui-manual-checklist.md`](../docs/tui-manual-checklist.md) ㉔，`CONTEXT.md` 的「控制」一节加了**挂起**词条。随后一轮 **`/triage`** 收下维护者报的问卷 bug —— 有选项的题上，自定义答案里打空格会被当成「确认」：单选直接翻页**并清掉刚打进去的文本**（中文答案里夹英文词，如 `llm wiki`，最容易撞上）。triage 复现了它、定了修法（空格只在焦点不在自定义栏时才确认），落成 [`fs-agent-v1` 票 33](fs-agent-v1/issues/33-space-in-the-questionnaire-custom-answer.md)（`ready-for-agent`），并把票 32 里那条「Space 确认优先」的决定标了更正 —— 那其实是票 32 修 `Enter` 同一类 bug 时漏掉的一半。

| 目录 | 形态 | 一句话 | 票 |
| --- | --- | --- | --- |
| [`fs-agent-v1/`](fs-agent-v1/spec.md) | spec | fs-agent v1：可扩展核心 + 多 agent 讨论 | 32 done + 1 ready-for-agent |
| [`multi-agent-architecture/`](multi-agent-architecture/map.md) | map | fs-agent：自用 · 可扩展核心 · 多 agent 讨论（wayfinder 决策图）—— **已折成 [`fs-agent-v1/spec.md`](fs-agent-v1/spec.md)** | 25/25 resolved |
| [`tui-layout/`](tui-layout/spec.md) | map + spec | 四分区全屏 TUI：布局、多行输入与信息面板（**外壳已被 `tui-sidebar` 推翻**；`tui-ux` 的增量折在它的 §14） | 9 resolved + 7 done |
| [`tui-sidebar/`](tui-sidebar/spec.md) | spec | 全高左栏、回合条与状态行：TUI 外壳改版 | 1 resolved + 4 done |
| [`tui-history-replay/`](tui-history-replay/spec.md) | map + spec | 重新打开会话：历史重播与历史详情 —— **已折成本目录的 spec** | 5 resolved + 4 done |
| [`tui-ux/`](tui-ux/map.md) | map | TUI 使用体验与视觉效果优化（wayfinder 决策图）—— **已折成 [`tui-layout/spec.md`](tui-layout/spec.md) §14** | 8/8 resolved |
| [`tui-input-pulse/`](tui-input-pulse/spec.md) | spec | 输入区三行 + 提示符色相（忙碌信号试了六版，最后落在提示符的色相上） | 9/9 done |
| [`tui-chrome/`](tui-chrome/spec.md) | spec | TUI 外壳收干净：拆掉四周外框、状态行上方那条线离场、剩余框架虚线化并压深、详情覆盖层屏幕居中、覆盖层立着时滚轮按指针位置分派（各推翻 `tui-sidebar` spec §1 与 §7 的一半）；§6 是随后一轮真机反馈的两条微调（左栏身份下移一行、横线不再截断竖线） | 8/8 done |
| [`chinese-ui/`](chinese-ui/spec.md) | spec | 界面中文化：给人看的文本收进一个措辞层 | 8/8 done |
| [`ask-user-question/`](ask-user-question/seed.md) | seed | 种子材料：模型发起的「选择工具」与底部问卷接管 —— **已落成 [`fs-agent-v1` 票 32](fs-agent-v1/issues/32-ask-user-question-tool.md)（done）**，本目录不会再有 spec | — |
| [`todo-and-modes/`](todo-and-modes/spec.md) | spec | 计划从「权限模式」改成模型自己的 `todo` 工具；模式回到 `readonly`/`ask`/`auto` 三档并补齐入口；侧栏加 `todo` 标签（推翻 `fs-agent-v1` §13，留了 [ADR 0003](../docs/adr/0003-plan-leaves-the-permission-modes.md)） | 4/4 done |
| [`sandbox/`](sandbox/spec.md) | spec | 基于 bubblewrap 的进程级沙箱：`bash` 与动态工具的写边界由内核担保（默认开 + fail closed；网络不在这一层，留了 [ADR 0006](../docs/adr/0006-sandbox-by-bubblewrap.md)）—— 2026-10-01 由 seed 折成 spec、同日落地（五张票全 done，真机清单在 [`docs/tui-manual-checklist.md`](../docs/tui-manual-checklist.md) ⑱）；五份一手调研与两份图解都在目录里 | 5/5 done |
| [`workspace-mode/`](workspace-mode/spec.md) | spec | 第四档权限模式 `workspace`：区内自动、区外要问，外加被沙箱拒之后的一次升级批准（[ADR 0007](../docs/adr/0007-workspace-permission-mode.md)）—— 2026-10-01 由 seed 折成 spec、同日两张票落地；决策地图在 [`docs/permissions.md`](../docs/permissions.md)，真机清单在 [`docs/tui-manual-checklist.md`](../docs/tui-manual-checklist.md) ⑲ | 2/2 done |
| [`docs-tidy/`](docs-tidy/spec.md) | spec | 文档整理：索引、陈旧数字、孤儿文档 + 各图的交棒补记（本轮） | 4/4 done |
| [`language-migration/`](language-migration/spec.md) | spec | 语言迁移：散文一律中文（[ADR 0004](../docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md)）—— 它那半句「模型可见 / 进流的文本留英文」已由 [ADR 0005](../docs/adr/0005-model-visible-text-in-chinese.md)（2026-09-30）推翻：模型可见与进流的**散文**也走中文，英文只留给标识符、schema 值与协议标记；注释、docs、测试断言消息、只给人看的错误文本全翻完，棘轮与数字已收到实测值；ADR 那一批（小标题 + 行话夹注 + `docs/adr` 护栏）于 2026-09-30 补收（票 03） | 3 done + 1 ready-for-agent（票 04：`.scratch` 的英文小标题中文化） |
| [`markdown-render/`](markdown-render/spec.md) | spec | TUI 里的 Markdown：解析交给 `pulldown-cmark`、渲染仍由我们自己写 —— 表格从「join 竖线」变成真网格、代码块接回 tree-sitter 高亮并扩到 10 种语言、assistant 续行不再缩进（[ADR 0008](../docs/adr/0008-markdown-parsing-by-pulldown-cmark.md)；两份一手调研在目录里） | 7/7 done |
| [`goal-loop/`](goal-loop/spec.md) | spec | **目标**（跨会话的工作单位）+ `/loop` 无人值守持续工作 + **翻页**（压缩、`/clear`、预算认到目标上）—— 吞并 `loop-and-goals` / `clear-command` / `context-compaction` 三条种子；同日一轮 grilling（30 问）关掉 14 条分叉后折成构建计划，并拆出 15 张自包含的票（来源与关闭记录在 [`seed.md`](goal-loop/seed.md)）；**同一天 15 张全部落地**，逐面文档在 [`docs/goals.md`](../docs/goals.md) | 15/15 done |
| [`exit-gesture/`](exit-gesture/spec.md) | spec | 退出手势：空闲 `Ctrl-C` / `Ctrl-D` 双击退出、举手期间提示行给回执（半秒后作废）、退出时打一行能直接粘的复盘命令；顺带修掉忙碌双击走 `exit(130)` 跳过终端恢复的缺陷 | 4 done + 1 ready-for-walkthrough |
| [`usage-stats-format/`](usage-stats-format/spec.md) | spec | 右侧统计的书写制式（`[ui] number_style`，`cn` / `si`，默认 `cn`）与两行的占比色条；诊断通道与状态行一个字不动 | 3/3 done |
| [`questionnaire-keys/`](questionnaire-keys/seed.md) | seed | 问卷手感：`j` / `k` 选择、移到自定义项才进文本输入、超长选项折行 | — |
| [`terminal-title/`](terminal-title/spec.md) | spec | 终端标题带上工作内容：`<父>/<基名> · <状态> · <目标名>`、40 列封顶、进 TUI 时保存原标题并在退出（含 panic）时还原 | 2 done + 1 ready-for-walkthrough |
| [`continue-by-id/`](continue-by-id/spec.md) | spec | `-c` 吃一个可选的 id（`-c <id>`，另有 `--session <id>`）：先本桶、再全 store，命中别的工作区时工作目录跟着那场会话走；退出回执因此改成 `fs-agent -c <id>` | 1/1 done |
| [`suspend-gesture/`](suspend-gesture/spec.md) | spec | 挂起手势：TUI 里 `Ctrl-Z` 把进程 SIGTSTP 停到后台（单下、任何视图都拦不住）、`fg` 回来重进终端并全量重绘；plain 的同一按键由终端驱动天然处理，用一条 pty 回归与文档钉住 | 2/2 done |
| [`context-injection-detail/`](context-injection-detail/seed.md) | seed | 注入的上下文（技能、`AGENTS.md`）可查看详情 | — |
| [`multi-role-view/`](multi-role-view/seed.md) | seed | 多角色输出分屏 / 分 tab（现在是单流按 speaker 上色） | — |
| [`desktop-notifications/`](desktop-notifications/seed.md) | seed | 桌面通知：回合完成、等审批这类时刻在 Linux 上提示 | — |
| [`interjection-flow/`](interjection-flow/seed.md) | seed | 运行中插入对话：排队等到边界，或立刻打断 | — |
| [`mcp-support/`](mcp-support/seed.md) | seed | 接入 MCP：外部 server 的工具进工具表（与「工具表建完不变」直接相撞） | — |
| [`context-compaction/`](context-compaction/seed.md) | seed | 压缩上下文：溢出前折成摘要继续跑（`HistoryReason::Compaction` 只是占位）—— **已移交 [`goal-loop`](goal-loop/seed.md)** | — |
| [`rag-vector-store/`](rag-vector-store/seed.md) | seed | RAG / 向量检索：按语义检索仓库或外部资料 | — |
| [`background-services/`](background-services/seed.md) | seed | 后台服务进程与定时任务 | — |
| [`web-search-tool/`](web-search-tool/seed.md) | seed | web 搜索工具 | — |
| [`grep-tool/`](grep-tool/seed.md) | seed | `grep` / `rg` 工具：把只读搜索从 `bash` 的 `Exclusive` 里救出来 | — |
| [`clear-command/`](clear-command/seed.md) | seed | `/clear` 命令：清上下文继续用 —— **已移交 [`goal-loop`](goal-loop/seed.md)**，意向也改成了「结束当前会话、开一个新的」 | — |
| [`loop-and-goals/`](loop-and-goals/seed.md) | seed | `/loop` 持续工作与跨轮目标 / 计划 —— **已移交 [`goal-loop`](goal-loop/seed.md)** | — |

数法：`ls .scratch/*/issues/*.md | wc -l` 与 `grep -h '^Status:' .scratch/*/issues/*.md | sort | uniq -c`。**`resolved` 是 wayfinder 决策票的收尾状态，`done` 是实现票的** —— 同一个 feature 里两种都可能出现（图走完折成 spec 之后接实现票）。

**需求池（2026-10-01）**：一轮 `/ask-matt` 里记下 16 条「有意向、不实现」的功能，一律只有
`seed.md`、没有票。其中 3 条（`loop-and-goals` / `clear-command` / `context-compaction`）当天
就被移交给了活跃 effort [`goal-loop`](goal-loop/seed.md)；同日另一轮 `/grill-with-docs` 又把三条
（[`usage-stats-format`](usage-stats-format/spec.md) / [`terminal-title`](terminal-title/spec.md) /
[`exit-gesture`](exit-gesture/spec.md)）折成了 spec，所以**池子里现在剩 10 条** —— 也就是
上表里形态为 `seed`、且没有「已移交」或「已折成 spec」注记的那些行。每条的抬头都自述「这不是
spec，也不是票」，正文有《现状》一节（写的时候核实过源码）与《待谈的分叉》；想推进任何一条时
走 `/grill-with-docs` 把它折成 spec，再照常 `/to-tickets` 拆票。

**四张 wayfinder 图的状态（2026-09-26 用 `scripts/wayfinder-check.py` 逐张核过，四张全 PASS）**：票都清了 —— `multi-agent-architecture` 25/25、`tui-layout` 16/16、`tui-ux` 8/8、`tui-history-replay` 9/9，没有 open 的决策票，也没有 unblocked 的 frontier 票。四张图的**交棒产物**都写在表里那一列；哪张图的抬头还写着「下一步是 `/to-spec`」而实际已经折完的，抬头下都补了一条带日期的「交棒已发生」。

**两条如实记录**：

- `.scratch/call-rationale/`（磁盘上只有一个**空的** `issues/`、不在 git 里、没有任何 spec/map/票，所以不列进上表）**已按这里写过的办法 `rmdir` 掉了**；本索引曾如实记过它存在（2026-09-26 复核时它已经不在了）。
- `map` 与 `spec` 可以同时存在：wayfinder 的图走完之后会被折成 spec（`tui-layout`、`tui-history-replay` 就是这样），图留着当决策记录。
