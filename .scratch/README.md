# `.scratch/`：feature 索引

本仓库的 issue tracker 就是这里的 markdown（约定见 [`docs/agents/issue-tracker.md`](../docs/agents/issue-tracker.md)）。

一个 feature 一个目录，三种形态：**`spec.md`**（构建计划）、**`map.md`**（wayfinder 的决策图）、**`seed.md`**（种子材料，还没变成 spec）；票在 `issues/NN-<slug>.md`，一票一个文件，抬头的 `Status:` 记状态 —— 实现票走 `ready-for-agent` → `done`（能自动化的部分都做完、只剩人在真终端里逐项走查的走 `ready-for-walkthrough`），wayfinder 的决策票走 `claimed` → `resolved`。

数法：`ls .scratch/*/issues/*.md | wc -l` 与 `grep -h '^Status:' .scratch/*/issues/*.md | sort | uniq -c`。

**需求池**：16 条「有意向、不实现」的意向如今剩 10 条只有 `seed.md`（判据 = 下表里形态为 `seed`、且没有「已移交」或「已折成 spec」注记的那些行；`ask-user-question` 按字面也算了进来，**实际还能推进 9 条**）。想推进哪一条就走 `/grill-with-docs` 折成 spec，再 `/to-tickets` 拆票。
| 目录 | 形态 | 一句话 | 票 |
| --- | --- | --- | --- |
| [`fs-agent-v1/`](fs-agent-v1/spec.md) | spec | fs-agent v1：可扩展核心 + 多 agent 讨论 | 34/34 done |
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
| [`questionnaire-keys/`](questionnaire-keys/spec.md) | spec | 问卷手感：选项区 / 输入区、`j`/`k` 移动、答案并存、`Esc` 退出询问、折行 + 页脚三档阶梯；10-02 两批 18 问折成（理由在 [ADR 0010](../docs/adr/0010-questionnaire-keys-dispatch-by-zone.md)），10-04 第三批 13 问折成 §11（回车改「下一题 / 提交」） | 6 done + 1 ready-for-agent（票 08）+ 1 ready-for-walkthrough；seed 里另有一条**未访谈**的意向（输入区的 Emacs 编辑键） |
| [`terminal-title/`](terminal-title/spec.md) | spec | 终端标题带上工作内容：`<父>/<基名> · <状态> · <目标名>`、40 列封顶、进 TUI 时保存原标题并在退出（含 panic）时还原 | 2 done + 1 ready-for-walkthrough |
| [`continue-by-id/`](continue-by-id/spec.md) | spec | `-c` 吃一个可选的 id（`-c <id>`，另有 `--session <id>`）：先本桶、再全 store，命中别的工作区时工作目录跟着那场会话走；退出回执因此改成 `fs-agent -c <id>` | 1/1 done |
| [`suspend-gesture/`](suspend-gesture/spec.md) | spec | 挂起手势：TUI 里 `Ctrl-Z` 把进程 SIGTSTP 停到后台（单下、任何视图都拦不住）、`fg` 回来重进终端并全量重绘；plain 的同一按键由终端驱动天然处理，用一条 pty 回归与文档钉住 | 2/2 done |
| [`sidebar-toggle/`](sidebar-toggle/spec.md) | spec | 左栏开关：`Ctrl-O` 收起与叫回 —— 意愿与宽度档**相乘**（< 80 列叫不回来）、意愿只活在进程内、提示行最末加一条 `ctrl-o 左栏`；推翻 `tui-sidebar` spec §2 与 `CONTEXT.md` 左栏词条里「去留只由宽度决定」那半句（2026-10-02，一次十问的 grilling，不建图） | 2 done + 1 ready-for-walkthrough（票 03 剩 ㉖ 的真机走查） |
| [`context-injection-detail/`](context-injection-detail/seed.md) | seed | 注入的上下文（技能、`AGENTS.md`）可查看详情 | — |
| [`multi-role-view/`](multi-role-view/seed.md) | seed | 多角色输出分屏 / 分 tab（现在是单流按 speaker 上色） | — |
| [`desktop-notifications/`](desktop-notifications/seed.md) | seed | 桌面通知：回合完成、等审批这类时刻在 Linux 上提示 | — |
| [`interjection-flow/`](interjection-flow/seed.md) | seed | 运行中插入对话：排队等到边界，或立刻打断 | — |
| [`mcp-support/`](mcp-support/map.md) | map + spec | 接入 MCP：现行规范全集（tool / resource / prompt / elicitation + MRTR），**不扩工具表** —— 用四个元工具 `mcp_list` / `mcp_call` / `mcp_resources` / `mcp_read`；连接层取 `rmcp`，server 进程过沙箱 + 环境白名单，信任按能力逐台声明。逐面文档在 [`docs/mcp.md`](../docs/mcp.md)。 | 9 resolved + 9 done + 1 ready-for-walkthrough |
| [`context-compaction/`](context-compaction/seed.md) | seed | 压缩上下文：溢出前折成摘要继续跑（`HistoryReason::Compaction` 只是占位）—— **已移交 [`goal-loop`](goal-loop/seed.md)** | — |
| [`rag-vector-store/`](rag-vector-store/seed.md) | seed | RAG / 向量检索：按语义检索仓库或外部资料 | — |
| [`background-services/`](background-services/seed.md) | seed | 后台服务进程与定时任务 | — |
| [`web-search-tool/`](web-search-tool/spec.md) | spec | 两个内建联网工具 `web_search` / `web_fetch`，结构照 DSH 的 `ctx.web` 三层（工具 / 服务 / 后端）：搜索后端取 DeepSeek 的 Anthropic 兼容端点 + 原生服务器工具（零新密钥），抓取自己发 HTTP 并自带 SSRF 防护，结果带不可信标记与 URL 引用。逐面文档在 [`docs/web.md`](../docs/web.md)。 | 5 done + 1 ready-for-walkthrough |
| [`grep-tool/`](grep-tool/spec.md) | spec | `grep` 工具：只读、只扫工作区（`Effect::ReadOnly`，四档全放行），输出 `path:line:文本`；可选的 `glob` 只缩小文件范围、不放宽忽略规则；命中超过 500 条时先收一刀并在末尾如实写清省掉多少，token 溢出仍走统一的截断与指针；实现取自带 ripgrep 拆出的库（`ignore` + `grep-searcher` + `grep-regex`）—— 2026-10-02 由 seed 折成 spec、同日拆出 4 张实现票，**2026-10-03 四张全部落地**，逐面文档是 [`docs/grep.md`](../docs/grep.md) | 4/4 done |
| [`clear-command/`](clear-command/seed.md) | seed | `/clear` 命令：清上下文继续用 —— **已移交 [`goal-loop`](goal-loop/seed.md)**，意向也改成了「结束当前会话、开一个新的」 | — |
| [`loop-and-goals/`](loop-and-goals/seed.md) | seed | `/loop` 持续工作与跨轮目标 / 计划 —— **已移交 [`goal-loop`](goal-loop/seed.md)** | — |
| [`image-input/`](image-input/seed.md) | seed | 把图片交给模型：粘贴 / 路径 / 拖拽进来的图进请求 —— 三种读法（输入侧 / 真图显示 / 只当引用），渲染侧的口子由 `markdown-render` 票 06 留着 | — |
| [`git-worktree/`](git-worktree/seed.md) | seed | git worktree：会话级或执行者级的隔离工作区（Codex 有 `--worktree` 与 `/worktree`）；会牵动会话桶、权限档与沙箱的「工作区」定义 | — |
| [`lifecycle-diagram/`](lifecycle-diagram/map.md) | map + spec | **fs-agent 运行时生命周期图**（wayfinder 决策图）：把「进程启动 → 一次 turn → 委派 → 退出」画成 mermaid 放进 [`docs/lifecycle.md`](../docs/lifecycle.md) —— 一张鸟瞰 + 四张分层详图，配「节点/边 → `文件:行号`」证据表与 `scripts/lifecycle-check.py` 弱校验；全 mermaid 是本仓库第一种，留了 [ADR 0011](../docs/adr/0011-diagrams-in-mermaid.md)。 | 5 resolved + 8 done + 1 ready-for-walkthrough |
| [`tui-mermaid/`](tui-mermaid/seed.md) | seed | TUI 里渲染 mermaid：把模型输出的 mermaid 围栏块画成图（今天只是一行灰色语言名 + 不着色的原文）—— **调研结论：能画、且不用浏览器**（`mermaid-text` 0.57.0 等三个纯 Rust 件），真阻力是本仓库自己的三条线（`to_lines` 纯函数、折行归 `pane::wrap_line`、TUI 单任务同步）与 ADR 门槛；与 `lifecycle-diagram` 选 mermaid 只是恰好同名，它属产品功能 | — |
| [`docs-slim/`](docs-slim/map.md) | map + spec | **文档瘦身**（wayfinder 决策图 + 折出来的 spec，2026-10-04 建）：给 36 份活文档定「压表达」规则 —— 单元 ≤500 字符、只拆 + 只删「别处已有一份的复述」、不动 `DOCS_MIN_RATIO`；**不删任何文件**。图 7/7、实现票 5/5（[`issues/08`](docs-slim/issues/08-doc-size-guardrail.md)–[`12`](docs-slim/issues/12-remainder-and-close-out.md)，08 是护栏脚本那个 tracer bullet），护栏是 [`scripts/check-doc-size.py`](../scripts/check-doc-size.py)，36 份的违规已清零 | 7 resolved + 5 done |
