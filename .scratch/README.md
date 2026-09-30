# `.scratch/`：feature 索引

本仓库的 issue tracker 就是这里的 markdown（约定见 [`docs/agents/issue-tracker.md`](../docs/agents/issue-tracker.md)）：

- **一个 feature 一个目录**：`.scratch/<feature-slug>/`；
- **spec** 在 `spec.md`（一次 grilling 或 wayfinder 折出来的构建计划）；**决策图**在 `map.md`（wayfinder effort：票是**决策**，不是交付物）；少数目录只有 `seed.md`（种子材料，还没变成 spec）；
- **票**在 `issues/NN-<slug>.md`，**一票一个文件**，每票开头有 `Type:` 与 `Status:`（`ready-for-agent` / `done`；wayfinder 的决策票是 `claimed` / `resolved`），`Blocked by:` 记阻塞边；
- **顺序**：blockers 先做；每票自包含，所以做完一票就可以把它的 context 丢掉。

下表由 `ls` / `grep '^Status:'` / `head -1` 核过（2026-09-26；`todo-and-modes` 的票数与状态在这一轮收尾时改过一次；`language-migration` 那一行按 2026-09-27 两张票的落地更新过，2026-09-30 补票 03、并把票 04 记成 `ready-for-agent`；2026-10-01 `sandbox` 由 seed 折成 spec、新增 `workspace-mode` 种子，同日五张票落地并改成 `5/5 done`）。

| 目录 | 形态 | 一句话 | 票 |
| --- | --- | --- | --- |
| [`fs-agent-v1/`](fs-agent-v1/spec.md) | spec | fs-agent v1：可扩展核心 + 多 agent 讨论 | 32/32 done |
| [`multi-agent-architecture/`](multi-agent-architecture/map.md) | map | fs-agent：自用 · 可扩展核心 · 多 agent 讨论（wayfinder 决策图）—— **已折成 [`fs-agent-v1/spec.md`](fs-agent-v1/spec.md)** | 25/25 resolved |
| [`tui-layout/`](tui-layout/spec.md) | map + spec | 四分区全屏 TUI：布局、多行输入与信息面板（**外壳已被 `tui-sidebar` 推翻**；`tui-ux` 的增量折在它的 §14） | 9 resolved + 7 done |
| [`tui-sidebar/`](tui-sidebar/spec.md) | spec | 全高左栏、回合条与状态行：TUI 外壳改版 | 1 resolved + 4 done |
| [`tui-history-replay/`](tui-history-replay/spec.md) | map + spec | 重新打开会话：历史重播与历史详情 —— **已折成本目录的 spec** | 5 resolved + 4 done |
| [`tui-ux/`](tui-ux/map.md) | map | TUI 使用体验与视觉效果优化（wayfinder 决策图）—— **已折成 [`tui-layout/spec.md`](tui-layout/spec.md) §14** | 8/8 resolved |
| [`tui-input-pulse/`](tui-input-pulse/spec.md) | spec | 输入区三行 + 提示符色相（忙碌信号试了六版，最后落在提示符的色相上） | 9/9 done |
| [`chinese-ui/`](chinese-ui/spec.md) | spec | 界面中文化：给人看的文本收进一个措辞层 | 8/8 done |
| [`ask-user-question/`](ask-user-question/seed.md) | seed | 种子材料：模型发起的「选择工具」与底部问卷接管 —— **已落成 [`fs-agent-v1` 票 32](fs-agent-v1/issues/32-ask-user-question-tool.md)（done）**，本目录不会再有 spec | — |
| [`todo-and-modes/`](todo-and-modes/spec.md) | spec | 计划从「权限模式」改成模型自己的 `todo` 工具；模式回到 `readonly`/`ask`/`auto` 三档并补齐入口；侧栏加 `todo` 标签（推翻 `fs-agent-v1` §13，留了 [ADR 0003](../docs/adr/0003-plan-leaves-the-permission-modes.md)） | 4/4 done |
| [`sandbox/`](sandbox/spec.md) | spec | 基于 bubblewrap 的进程级沙箱：`bash` 与动态工具的写边界由内核担保（默认开 + fail closed；网络不在这一层，留了 [ADR 0006](../docs/adr/0006-sandbox-by-bubblewrap.md)）—— 2026-10-01 由 seed 折成 spec、同日落地（五张票全 done，真机清单在 [`docs/tui-manual-checklist.md`](../docs/tui-manual-checklist.md) ⑱）；五份一手调研与两份图解都在目录里 | 5/5 done |
| [`workspace-mode/`](workspace-mode/seed.md) | seed | 种子材料：`workspace` 权限模式（区内自动、区外问）—— 2026-10-01 从沙箱那轮访谈的 Q7 分出来，shell 那一半依赖沙箱已落地 | — |
| [`docs-tidy/`](docs-tidy/spec.md) | spec | 文档整理：索引、陈旧数字、孤儿文档 + 各图的交棒补记（本轮） | 4/4 done |
| [`language-migration/`](language-migration/spec.md) | spec | 语言迁移：散文一律中文（[ADR 0004](../docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md)）—— 它那半句「模型可见 / 进流的文本留英文」已由 [ADR 0005](../docs/adr/0005-model-visible-text-in-chinese.md)（2026-09-30）推翻：模型可见与进流的**散文**也走中文，英文只留给标识符、schema 值与协议标记；注释、docs、测试断言消息、只给人看的错误文本全翻完，棘轮与数字已收到实测值；ADR 那一批（小标题 + 行话夹注 + `docs/adr` 护栏）于 2026-09-30 补收（票 03） | 3 done + 1 ready-for-agent（票 04：`.scratch` 的英文小标题中文化） |

数法：`ls .scratch/*/issues/*.md | wc -l` 与 `grep -h '^Status:' .scratch/*/issues/*.md | sort | uniq -c`。**`resolved` 是 wayfinder 决策票的收尾状态，`done` 是实现票的** —— 同一个 feature 里两种都可能出现（图走完折成 spec 之后接实现票）。

**四张 wayfinder 图的状态（2026-09-26 用 `scripts/wayfinder-check.py` 逐张核过，四张全 PASS）**：票都清了 —— `multi-agent-architecture` 25/25、`tui-layout` 16/16、`tui-ux` 8/8、`tui-history-replay` 9/9，没有 open 的决策票，也没有 unblocked 的 frontier 票。四张图的**交棒产物**都写在表里那一列；哪张图的抬头还写着「下一步是 `/to-spec`」而实际已经折完的，抬头下都补了一条带日期的「交棒已发生」。

**两条如实记录**：

- `.scratch/call-rationale/`（磁盘上只有一个**空的** `issues/`、不在 git 里、没有任何 spec/map/票，所以不列进上表）**已按这里写过的办法 `rmdir` 掉了**；本索引曾如实记过它存在（2026-09-26 复核时它已经不在了）。
- `map` 与 `spec` 可以同时存在：wayfinder 的图走完之后会被折成 spec（`tui-layout`、`tui-history-replay` 就是这样），图留着当决策记录。
