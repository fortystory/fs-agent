# fs-agent 运行时生命周期图（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 [`docs/agents/issue-tracker.md`](../../docs/agents/issue-tracker.md)
Charting: **已完成**（2026-10-03，两轮 grilling 共 15 问 + 三次只读取证）。本图只做**规划**：不画图、不写 `docs/lifecycle.md`、不写脚本、不动 `src/`。
**✅ 本图已完成（2026-10-03）**：5 张子票全部 resolved、`Not yet specified` 为空 ⇒ 路线 clear；下一步是 `/to-spec`（见 `## 进度`）。**不要再往这张图加票。**

> **交棒已发生（2026-10-03）**：十五条冻结项与五张票的 `## Answer` 已经折进 [`spec.md`](spec.md)
> —— 五张图（[`prototype/01-drafts.md`](prototype/01-drafts.md) v3）、虚实判据、对账脚本的七条校验、
> 文档骨架、ADR 0011 草稿。七步落地清单在 spec 的 `## Further Notes`。本图至此**只作决策存档**，
> 不再是待办；「做」由 `/to-tickets` 拆出的实现票与 `/implement` 接手。

## Destination

一份 **spec-ready 的「fs-agent 运行时生命周期图」设计** —— 交给 `/to-spec` 折成构建计划。它要
回答「把 fs-agent 从进程启动到退出画成 mermaid、放进 [`docs/lifecycle.md`](../../docs/lifecycle.md)」
所需的每一个拍板问题：分几层、每层哪些节点与哪些边、用哪种 mermaid 图类型、哪些边走虚线、图旁
配什么对账物、这份新文档怎么与语言护栏相处。

**范围 = CLI 实际会走的运行时生命周期**：交互式（新开 + `--continue`）与 `--discuss` 画全；
`probe` / `prune` / `sessions` 在鸟瞰里各占一个节点、不再展开；**可注入但 CLI 不注入**的
（hook、headless 渲染器）与**尚未实现**的（MCP）走虚线或旁注。

## Notes

- **领域**：fs-agent —— 自用 coding agent CLI（Rust）。三条先决事实：**事件流是唯一真相源**
  （`messages` 永远重算）、**工具表组装后不变**（它是缓存前缀的一部分）、**权限门在沙箱之外
  且是纯函数**。
- **共同基线**：[`research/01-runtime-lifecycle-facts.md`](research/01-runtime-lifecycle-facts.md)
  —— charting 阶段的只读侦察：进程 / 一次 turn / 委派 / 基础设施四条线的全部事实，每条带
  `文件:行号`，末尾「存疑」一节写明没读到的地方。**每张票开工前先读它**，不要重新考古。
- **要咨询的 skills**：`/grilling`（HITL 票默认）、`/domain-modeling`（图里的词一律取
  `CONTEXT.md` 的正式用词）、`/prototype`（草稿图）、`/research`（research 票）。
- **每张票的答案必须自足**：`/to-spec` 会在别处的会话里读它，看不到本图与 charting 对话。

### Tracker 事实与降级（本图适用）

- map = `.scratch/lifecycle-diagram/map.md`，child = `issues/NN-*.md`；阻塞 = 票面
  `Blocked by: NN`；claim = `Status: claimed`；resolve = 票底 `## Answer` + `Status: resolved`
  + 追加一行到本文 `Decisions so far`。
- **没有 native sub-issue / 依赖边**，所以回退到正文约定：本文的 `## 任务清单` 逐条引用子票
  （条目数 == 子票文件数），每张子票顶部写 `Part of: ../map.md`。
- 校验：`python3 scripts/wayfinder-check.py .scratch/lifecycle-diagram/map.md`。宣布图走完之前
  必须 PASS。

### 冻结项（charting 的两轮 grilling 定下，票里不得重开）

1. **destination 是一份 spec**，不是图本身，也不含落地。
2. **落点**：新建 `docs/lifecycle.md`；[`README.md`](../../README.md) 的「架构」一节加一行指向它，
   并补进它的「文档」表。
3. **读者**：onboarding 为主、变更影响分析为辅（排查定位归
   [`docs/observability.md`](../../docs/observability.md)，不重复）。
4. **范围 = 运行时**：进程被敲下 → 正常退出 / 取消 / 崩溃。构建、安装、配置不属于本图。
5. **形态**：一张鸟瞰总图 + 四张分层详图（进程启动与收尾 / 一次 turn / 委派与嵌套 / 基础设施
   回路）。
6. **格式：全 mermaid**。这是本仓库第一种 mermaid 图；代价（终端不可读、与 5 处 ASCII 邻居
   不一致）由票 05 与 ADR 0011 处理。
7. **虚实约定**：CLI 实际路径画实线；可注入但 CLI 不注入（hook、headless）与尚未实现（MCP）
   画虚线或旁注。
8. **分层切法**：进程启动与收尾（三条退出路径并入第一张） / 一次 turn / 委派与嵌套 / 基础设施
   回路（事件流 · goal · 问询 · 渲染）。**不**按 15 个顶层边界切，也**不**按三个前端切。
9. **节点粒度**：概念阶段（中文名 + 夹注英文标识符），不是函数调用图，也不是黑盒大块。
10. **mermaid 图类型**：鸟瞰与分层骨架用 `flowchart`；「一次 turn」用 `sequenceDiagram`；
    不引入 `stateDiagram`，权限四档用表。
11. **已有那 5 处 ASCII 局部图一个字不动**，新图在每张下面配「细则在哪」的指向表。
12. **对账**：图 + 一份「节点/边 → `文件:行号`」证据表 + 一个弱校验脚本
    `scripts/lifecycle-check.py`。
13. **语言护栏**：`docs/lifecycle.md` 加进 [`scripts/check-language.py`](../../scripts/check-language.py)
    的 `DOCS_MIN_RATIO`，下限按实测值单独压低（照 `AGENTS.md: 23` 的先例），并在脚本注释里写明
    理由。**「剥离代码块再算比例」是后续另开的一票**，不在本图。
14. **子命令范围**：交互式（新开 + `--continue`）与 `--discuss` 画全；`probe` / `prune` /
    `sessions` 只在鸟瞰里各占一个节点。
15. **写一条 ADR 0011**：docs 的流程图用 mermaid —— surprising、real trade-off、不易回头，
    三条判据都成立。

### 会撞的既有决定

- **`docs/lifecycle.md` 是新的**，所以 `DOCS_MIN_RATIO` 要加一行（冻结项 13）；[`README.md`](../../README.md)
  的「文档」表与「架构」一节各加一处指向（冻结项 2）；[`.scratch/README.md`](../README.md) 的
  feature 表也要加一行 —— 三处都不许漏。
- **[`README.md:221-223`](../../README.md) 那一行 ASCII** 是「工具调用固定顺序」（不变量 5）的既有
  落点：新图重画它，但**不删它**（冻结项 11）。
- **`grep -rn "mermaid" README.md CONTEXT.md docs/` 今天只有一处命中**，而且是讲某个 crate
  包体大小的旁白（`docs/adr/0008-markdown-parsing-by-pulldown-cmark.md:24`）：**仓库里没有任何
  一张 mermaid 图**。本图会开这个先例，所以 ADR 0011 必须解释「为什么只有这份文档不是 ASCII」。
- **`src/render/tui.rs` 已经会渲染代码块**（tree-sitter 高亮）：`docs/lifecycle.md` 里的 mermaid
  在 fs-agent 自己的 TUI 里只会显示成高亮的代码、**不会变成图** —— 这一点写进新文档的代价一节，
  指向 [`../tui-mermaid/`](../tui-mermaid/)。

### 基线与纪律

- 本图**不写** `docs/lifecycle.md`、**不写** `scripts/lifecycle-check.py`、**不动** `src/`。
- 图的内容以 [`research/01-runtime-lifecycle-facts.md`](research/01-runtime-lifecycle-facts.md)
  为准；它标了「存疑」的地方，票里要用代码复核后再画。
- 提交用限定路径：本工作区出现过并行 session 与全量暂存互相卷进无关改动的情况。
- 验收基线：`cargo test` 与 `python3 scripts/check-language.py` 当前全绿（2026-10-03 实测）。

## 任务清单

<!-- 条目数必须等于 issues/ 下的子票数（scripts/wayfinder-check.py 校验）。frontier 的权威查询仍是扫 issues/ 里 open + unblocked + unclaimed 的票。 -->

**决策票**（wayfinder 的五张，全部 resolved）：

- [x] [prototype：五张图的草稿 mermaid](issues/01-prototype-diagram-drafts.md)
- [x] [task：虚线清单 —— CLI 不走但存在的路径](issues/02-task-dashed-paths.md)
- [x] [research：`lifecycle-check.py` 的可行设计](issues/03-research-lifecycle-check.md)
- [x] [task：逐节点证据表](issues/04-task-evidence-table.md)
- [x] [grilling：文档骨架、ADR 0011 与语言护栏](issues/05-grilling-doc-skeleton-and-adr.md)

**实现票**（交棒后拆出的构建切片，`Type: implement`；wayfinder 会话不认领它们）：

- [x] [文档骨架 + 画法约定 + 鸟瞰图（tracer bullet）](issues/06-doc-skeleton-and-overview-diagram.md)
- [x] [ADR 0011：文档里的流程图用 mermaid](issues/07-adr-diagrams-in-mermaid.md)
- [x] [`scripts/lifecycle-check.py` + 它的测试](issues/08-lifecycle-check-script.md)
- [x] [图 2：进程启动与收尾](issues/09-diagram-boot-and-shutdown.md)
- [x] [图 3：一次 turn（`sequenceDiagram`）](issues/10-diagram-one-turn.md)
- [x] [图 4：委派与嵌套](issues/11-diagram-delegation.md)
- [x] [图 5：基础设施回路](issues/12-diagram-infrastructure.md)
- [x] [§7「图上不能断的边」](issues/13-invariants-section.md)
- [x] [收口：全绿 + 索引核对 + 人工验收](issues/14-close-out.md)

共 **14** 张票（**5 决策 + 9 实现**），当前 **5 resolved / 8 done + 1 ready-for-walkthrough** ——
实现票全部落地；票 14 剩下的只有「人在 GitHub 上看一眼五张图渲染出来」（见 `## 进度`）。

## Decisions so far

<!-- 索引：每条一行，够判断相关性即可；细节住在票里。按名字引用，不写裸编号。 -->

- [prototype：五张图的草稿 mermaid](issues/01-prototype-diagram-drafts.md): 五张图定形（鸟瞰 14 节点 / 启动与收尾 24 / 一次 turn 用 `sequenceDiagram` / 委派 18 / 基础设施 19），产物是 [`prototype/01-drafts.md`](prototype/01-drafts.md)。四处的裁决：**票 03 的白名单扩认 `-.->`**（虚线只表示「可注入但 CLI 不注入」与「尚未实现」，异常路径改用边标签）；**`sequenceDiagram` 不进 C4/C5 对账**，只有四张 `flowchart` 参与集合相等那条检查；**并发不做画法约定**（图只表达存在、不表达并发）。四张 flowchart 用票 03 的两条规则**实测**通过（id 唯一、先定义后引用）；没有实测渲染（仓库无 Node / `mmdc`）。
- [task：虚线清单 —— CLI 不走但存在的路径](issues/02-task-dashed-paths.md): **该画虚线的只有 2 条** —— `hook`（三个生产组装点全传 `None`、`src/` 零实现、无配置面；图 4 的 `pre -.-> hooks` 即它，pre/post 共用一个端口）与 **MCP**（`src/` 零命中但有 spec 与 9 张实现票，即「有一个已写下来的接缝」；图上先用旁注）。**headless 如实排除**（它在 `probe` 里真被构造）。生产组装点只有三个（交互式 `cli.rs:380`、`discuss` `:700`、`probe` `:2214`），其余 13 项全部实线 / 旁注 / 不画。**判据可直接抄进 `docs/lifecycle.md` §1**：实线 = 至少一个生产组装点会走到（含配置开关与 `None` 降级分支）；虚线只给「可注入但三处全 None 且 src 无实现」与「src 零命中但有 spec 接缝」；并写死四种不画（默认值+运行期补齐 / 前端差异 / 配置可选 / 测试替身）与「只有 `seed.md` 的 roadmap 不进图」。图据此修正三处（图 3 的 hook 两行降级成 Note、图 2 的 `tools` 加 MCP 旁注、图 4 保留）。
- [research：`lifecycle-check.py` 的可行设计](issues/03-research-lifecycle-check.md): **对结构宁严勿松、对位置宁松勿严** —— v1 守七条（文件存在 / 符号仍在 / README 仍引用 / 表与图的节点集合**相等**（双向）/ id 唯一 / 边两端显式定义 / 方言白名单，不认识就报红），行号精确性只降级为告警 + `--strict`（行号是结构性易漂量，严了会变噪音机器）。三条前提写进文档写作约定：受约束的 mermaid 方言、「先显式定义后引用」、固定节标题与表形状。v1 不做语义正确性、不做渲染验证、不接 CI（**仓库本来没有 CI 与 Makefile**，护栏靠 `README.md:257-262` 手工调用）。顺带查实 `check-language.py` 的既有缺口：`check_docs()` 只遍历 `DOCS_MIN_RATIO` 的键、不扫 `docs/*.md` —— 所以冻结项 13 的「加一行」是必须动作。**2026-10-03 由票 01 与票 04 修订三处**：白名单加 `-.->`；sequence 图不进 C4/C5；**C4/C5 按图分别比对**、不要求跨图唯一。
- [task：逐节点证据表](issues/04-task-evidence-table.md): 交付 [`research/04-node-evidence.md`](research/04-node-evidence.md)（343 行）—— 五张图逐节点（14 / 24 / 5 参与者 / 18 / 19，每个带 `文件:行号` + 符号）、每图边表、12 条不变量的边映射、五条存疑复核、一节诚实清单。**推翻侦察报告一处**：`probe` 子命令在 `src/cli.rs:2238` 真的构造 `Renderer::headless`（测试模块之外），所以图 5 的 headless 不再是虚线。**图已按诚实清单改成 v2**（九处改动，见 [`prototype/01-drafts.md`](prototype/01-drafts.md)），改完自检仍过。**另一条是检查器的错**：五张图 id 跨图撞名（`cmd` / `loop` / `ok` / `gate` / `red` / `goal`）会让 C4/C5 判不了 —— 裁决是**改检查器**（按图分别比对），不是给图加 `g1_` 前缀，已作为第三处修订记进票 03。
- [grilling：文档骨架、ADR 0011 与语言护栏](issues/05-grilling-doc-skeleton-and-adr.md): 六条全部采纳 —— 骨架是「§1 画法约定 → 鸟瞰 → 启动与收尾 → 一次 turn → 委派 → 基础设施 → §7 图上不能断的边（12 条不变量的映射）→ 附录逐图证据表」；图**编号**（图 1–5，别的文档可引用）；画法约定**只住在** `docs/lifecycle.md` §1，脚本 docstring 指向它、不做两份全文；`CONTEXT.md` **不收**「生命周期图」（那是文档名、不是领域概念）；文档**不拆**（脚本要同时读图和表，且篇幅远没到）。**ADR 0011 的完整草稿写在票里**：Context 是「仓库零 mermaid 图 + 5 处 ASCII 手工图」，Decision 是「`docs/` 的流程图用 mermaid + 受约束方言 + 证据表对账」，代价四条（终端读不到图、`DOCS_MIN_RATIO` 要单独降低限、两种图语言并存、护栏守不住语义）。选 mermaid 的**首要理由是图的节点集合机器可读**（ASCII 没有可解析结构，C4/C5 那条检查无从谈起），不是好看。

## Not yet specified

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票，不要预先切成票那么大。 -->

<!-- 当前**没有**未指定的雾：原有的四条各自有了归宿 —— 「文档会不会大到该拆」与「`CONTEXT.md` 要不要
收这个词」由票 05 定死（**不拆**、**不收**）；「MCP 落地后虚线怎么转实线」由票 02 收窄成一句话（它属
「尚未实现」那一类，落地时把旁注换成实线即可，不需要单独的票）；「五张 mermaid 在 GitHub 上的实际
渲染」不是待决事项，而是**实现阶段的验收项**（写进 spec 的落地清单，见票 05 的 `## Answer`）。 -->

## Out of scope

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **TUI 里渲染 mermaid**（把模型的 ```mermaid 代码块画成图）：那是产品功能，不是这份文档的图；
  已另立 [`tui-mermaid`](../tui-mermaid/) 的意向与一份调研。
- **开发 / 构建 / 安装生命周期**：范围是运行时（冻结项 4）。
- **重画或删除已有那 5 处 ASCII 局部图**：冻结项 11。
- **从代码自动生成图**：本图是手写 + 证据表 + 弱校验，不做 codegen。
- **mermaid 之外的图语言**（graphviz / d2 / ASCII 版）：冻结项 6。
- **本图的执行**：只产决策；「做」发生在 `/to-spec` → 实现票 → `/implement`。

## 进度

**charting 100%（2026-10-03）**：destination 与 15 条冻结项已定，5 张子票已建。

**100% —— 本图完成（2026-10-03）**：5/5 张子票全部 resolved，`Not yet specified` 为空 ⇒ 通往
destination 的路线已 clear。

**五个决定 + 三份取证 + 一份定形的图** = 那份 spec-ready 的设计：

- 图：[`prototype/01-drafts.md`](prototype/01-drafts.md) 的 **v3** —— 五张，票 04 与票 02 的修正都已落进去。
- 证据：[`research/04-node-evidence.md`](research/04-node-evidence.md)（逐节点 `文件:行号` + 12 条不变量的边映射）。
- 判据：[`research/02-dashed-inventory.md`](research/02-dashed-inventory.md) §2（虚实 + 方言 + 对账，可直接抄进 §1）。
- 脚本：[`research/02-lifecycle-check-design.md`](research/02-lifecycle-check-design.md)（七条校验 + 告警）。
- 文档：票 05 的 `## Answer`（骨架 + ADR 0011 草稿 + 落地清单）。
- 底料：[`research/01-runtime-lifecycle-facts.md`](research/01-runtime-lifecycle-facts.md)（侦察；其中「headless 从不构造」一条已被票 04 推翻）。

**交棒已完成（2026-10-03）**：上面这些折成 [`spec.md`](spec.md)（313 行），并由 `/to-tickets` 拆出
**9 张实现票**（`issues/06`–`14`，`Type: implement`、全部 `ready-for-agent`）—— frontier 是
[票 06（骨架 + 画法约定 + 鸟瞰图）](issues/06-doc-skeleton-and-overview-diagram.md) 与
[票 07（ADR 0011）](issues/07-adr-diagrams-in-mermaid.md)。下一步是 `/implement`，不在本图里。

**实现已落地（2026-10-03）**：9 张实现票落地（8 张 `done`，票 14 是 `ready-for-walkthrough` ——
剩下的只有人在 GitHub 上看一眼五张图）。产物是
[`docs/lifecycle.md`](../../docs/lifecycle.md)（骨架 + §1 画法约定 + 五张图 + §7 那 12 条不变量 +
附录证据表）、[`docs/adr/0011-diagrams-in-mermaid.md`](../../docs/adr/0011-diagrams-in-mermaid.md)、
[`scripts/lifecycle-check.py`](../../scripts/lifecycle-check.py) 与
[`scripts/tests/test_lifecycle_check.py`](../../scripts/tests/test_lifecycle_check.py)（18 条用例），
外加 `README.md` 的两处索引、`scripts/check-language.py` 的一行下限、`.scratch/README.md` 的
feature 行。护栏全绿：`lifecycle-check.py`（74 节点 / 78 边对账）、`python3 -m unittest`、
`check-language.py`、`cargo test`。本 feature **不改运行时行为**，所以没有事件流层面的验收。

**待确认**：无。五张票的决定都已在 live exchange 或只读取证里落定。
