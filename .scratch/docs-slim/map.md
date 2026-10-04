# 文档瘦身（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 [`docs/agents/issue-tracker.md`](../../docs/agents/issue-tracker.md)
Charting: **已完成**（2026-10-04，四轮 grilling 共十问：终点 / 判据 / 范围 / 验收 / 矛盾 / 门槛 / AGENTS.md / 删的边界 / 两个大户 / 体量指标）。本图只做**规划**，不产文档改动。
交棒：**已折成 [`spec.md`](spec.md)**（2026-10-04，`/to-spec` —— 七张票的 `## Answer` 就是它的来源；那一条测试 seam 已与维护者确认：只测 `scripts/check-doc-size.py` 的 CLI）。**实现票已由 `/to-tickets` 拆出五张**：08 护栏落地（tracer bullet，frontier）→ 09 / 10 / 11（并行，都被 08 block）→ 12 收口（被 09–11 block）；依赖边见各票抬头。

## Destination

一份**可执行的文档瘦身 spec** —— 交给 `/to-spec` 折成构建计划。它要给 36 份活文档定下统一的「压表达」规则（单元 ≤500 字符、只拆 + 只删「别处已有一份的复述」、不动 `DOCS_MIN_RATIO`），指名每份该改哪里，并留下一条可复核的护栏脚本。

**范围**：入口三份（[`README.md`](../../README.md) / [`CONTEXT.md`](../../CONTEXT.md) / [`AGENTS.md`](../../AGENTS.md)）+ [`.scratch/README.md`](../README.md) + `docs/*.md` 逐面 + `docs/adr/`（`docs/agents/*` 与 `docs/highlight.md` 并入「docs 逐面」）。

**不删任何文件。** 「减存量」在本轮被重新定义为「删掉入口文档里的复述」，不是删除过程产物。

## Notes

- **领域**：fs-agent —— 自用 coding agent CLI（Rust）。文档体系的现状见 [`README.md`](../../README.md) 的「文档」一节与 [`.scratch/README.md`](../README.md)。
- **文档按「被读的方式」分层**（本图的首要发现，实测自 `src/context.rs` / `src/lib.rs`）：
  - **每轮全文注入**：只有 `AGENTS.md`（`ContextInjected { source: AgentsMd }`，`src/lib.rs` 的 `record_skeleton`）加技能清单（`name: description`，封顶 3k token）。[`docs/skills.md`](../../docs/skills.md) 自己写着「`AGENTS.md` 每轮全文注入，所以它越长越贵」。
  - **每次上手读一次**：`README.md` / `CONTEXT.md` / `.scratch/README.md`。
  - **按需读一份**：`docs/*.md` 逐面、`docs/adr/*`、`.scratch/*/spec.md`。
  - **几乎不被读**：`.scratch/*/issues/`、`.scratch/*/research/`、`docs/research/`。
  - 推论：**按 token 算，瘦身的收益不在 `AGENTS.md`**（它已经是全仓最瘦的大文档：26 行 / 1,317 字符）；收益最大的是「每次上手读」的那三份，胖点也最集中在那里。
- **实测基线（2026-10-04，`wc -l` 与脚本）**：
  - 语料（排除 `.cargo-home` 的依赖文档）：366 个 md。`.scratch/` 324 文件 / 36,273 行 / 3,530KB；`docs/` 39 / 13,678；根 3 / 658。
  - `.scratch/` 构成：issues 230 文件 / 15,805 行（**已收尾 222 张 / 15,247 行**）、research 28 / 12,091、spec 25 / 5,401、map 6 / 840、seed 25 / 1,167、prototype 9 / 889。
  - scope 内 36 份文档的超长块（[票 03](issues/03-research-long-paragraph-triage.md) 的四种口径实测，可复现）：

    | 口径 | ≥500 | ≥800 | ≥1500 | 最长 |
    | --- | ---: | ---: | ---: | ---: |
    | **A 整块**（空行分隔，排除表格 / 代码 / 标题） | 79 | 31 | 8 | 7,429 |
    | **E 单元**（散文段 + 顶层清单项，**票 03 采用**） | **22** | 4 | 2 | 7,429 |

    A 与 E 的差全是清单：79 个整块里 **60 个是清单块**（含 335 个顶层清单项），整块口径把它们压成 60 个数。**「≥500 约 60」在任何口径下都复现不出来**（A=79、剥 markdown 标记=63、去空白=66），不要当验收基线；≥800=31、≥1500=8 与整块口径吻合。另：1,529 个表格单元格里 ≥500 有 **5**（最长 1,241）、引用块 11 块**全部 < 330**。
  - 最长的几块（口径 A）：`.scratch/README.md` **7,429**、`README.md` **2,664**、[`docs/render.md`](../../docs/render.md) 1,897、[`docs/discussion.md`](../../docs/discussion.md) 1,838、[`docs/adr/0002`](../../docs/adr/0002-fullscreen-alt-screen-tui.md) 1,789、[`docs/adr/0003`](../../docs/adr/0003-plan-leaves-the-permission-modes.md) 1,599、[`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) 1,530。**8 块 ≥1500 里 6 块其实是清单块**（按项加空行即散），只有 2 块是真散文要重写：`.scratch/README.md` L11 与 `README.md` L26。
  - 两个行数大户的形态**完全不同**：[`docs/lifecycle.md`](../../docs/lifecycle.md) 796 行 = 图 193 + **证据表 246** + 散文 208；`docs/tui-manual-checklist.md` 668 行 = **26 条共 534 行块内容**（散文 368 + 列表 164 + 代码 2，票 01 的可复现口径；13 块 ≥800）。
  - **跨文档重复**：scope 内的活文档之间 **0 条**（≥60 字的句子无一同时出现在两个文件里）；`.scratch/` 内 **127 条**，全是模板句（19 个 `seed.md` 共用「待访谈」抬头、issues 之间共用验收 / 顺序 / 参考句）—— 后者不在 scope。
- **要咨询的 skills**：`/grilling`（HITL 票默认）、`/domain-modeling`（`CONTEXT.md` 的词条边界，见票 07）、`/research`（research 票）。
- **每张票的答案必须自足**：`/to-spec` 会在别处的会话里读它，看不到本图与 charting 对话。

### 冻结项（charting 的 grilling 定下，票里不得重开）

1. **destination 是一份 spec**，由 `/to-spec` 折成；本图只产决策，不写文档改动。
2. **范围**（Q3）：入口三份 + `.scratch/README.md` + `docs/*.md` 逐面 + `docs/adr/`。
3. **不删任何文件**（Q5）：`.scratch/*/issues/`、`spec.md`、`map.md`、`seed.md`、`docs/research/`、`.scratch/*/research/` 一律不动 —— 归本文 `Out of scope`。
4. **「瘦」= 压表达**（Q2 → Q5）：**只删「别处已有一份的复述」**（各 feature 的 spec / 票里已经写过的逐日经过），其余**只拆段落**、不改信息量（Q8）。
5. **单元 ≤500 字符**（Q6）是唯一的内容密度指标，口径按 [票 03](issues/03-research-long-paragraph-triage.md) §4.1 修正为**「单元」**：散文段，或一个顶层清单项连同它的续行与嵌套子项；表格单元格单独算单元。**不是「空行分隔的整块」** —— 那个口径把本 scope 的 335 个清单项压成 79 块，其中 60 块是伪问题。可复现基线：**单元 ≥500 有 22、≥800 有 4、≥1500 有 2；单元格 ≥500 有 5、≥200 有 14**（整块口径的 79 / 31 / 8 只用于对账）。豁免按票 03 的规则：**R1 引文（引用行 ≥50%）与 R2 纯指针枚举（链接跨度 ≥50% 且 `·`/`、` ≥4）自动放行；R3 单一长句 / R4 不可断因果链 / R5 次序步骤只打印复核、不非零退出**。今天全语料只自动豁免 `README.md` 文档表的 2 个单元格，**整段零豁免、白名单空着起步**。
6. **只拆不删、不动 `DOCS_MIN_RATIO`**（Q6）：拆段落只加换行；删中文散文会撞红中文占比下限（`docs/tui-manual-checklist.md` 实测 46.2% / 下限 46，**余量 0.2%**；[`docs/agents/triage-labels.md`](../../docs/agents/triage-labels.md) 0.9%；[`docs/goals.md`](../../docs/goals.md) 1.0%；`docs/adr/0003` 1.4%；[`docs/render.md`](../../docs/render.md) 1.9%）。**这几份文档里「删复述」都可能直接撞红，优先拆、不优先删。**
7. **净行数指标只对入口三份设**（Q10）：`README.md` / `.scratch/README.md` / `AGENTS.md` —— 只有这三份有明确可删的复述（`README.md` 的「状态」段 2,664 字符、`.scratch/README.md` 第 11 行 7,429 字符、`AGENTS.md` 的 `### Docs` 节 601 字符）。
8. **`AGENTS.md` 的 `### Docs` 留指针、删复述**（Q7）：删掉它复述 `README.md` 语言约定的那些长句（同一条规则两处维护 = 漂移源，约 380 字符），保留「索引在哪」这个指针。它那五个英文小标题是技能工具链的锚点，**不动**。**注意（票 03 实测）**：`AGENTS.md` **没有任何 ≥500 的单元**（601 字符是「节」不是「单元」）—— 这条改动**不受段落指标约束**，唯一的护栏是冻结项 7 的净行数指标，判据是「同一规则两处维护」。
9. **`docs/tui-manual-checklist.md` 纳入压表达；`docs/lifecycle.md` 只按段落指标顺带**（Q9）—— **不动它的 mermaid 图与 246 行证据表**（[`scripts/lifecycle-check.py`](../../scripts/lifecycle-check.py) 逐行对账）。票 03 实测：那张 246 行表的**最长单元格只有 106 字符**，确实不需要动；它只有 L299-307 那一块是复述。
10. **护栏照邻居走**（Q4）：新增 `scripts/check-doc-size.py`，配 `scripts/tests/` 的单测，`README.md` 的「开发」一节加一行 —— 与 `check-language.py` / `lifecycle-check.py` 同一个形态。

### 会撞的既有决定（`/to-spec` 时回改，不在本图改）

- **[`scripts/check-language.py`](../../scripts/check-language.py) 的 `DOCS_MIN_RATIO`**：那是中文占比**下限**，删中文散文会让它报红。冻结项 6 选了不动下限，所以瘦身只能「拆段落 + 删复述」；票 03 已给出全部余量（最小 0.2%、其次 0.9% / 1.0% / 1.4% / 1.9%）。
- **[`scripts/lifecycle-check.py`](../../scripts/lifecycle-check.py)**：逐行对账 `docs/lifecycle.md` 的图与证据表，且「方言、虚实判据与七条对账规则住在 `docs/lifecycle.md` §1」—— 冻结项 9 把这张表划出范围就是为了不碰它。
- **[`scripts/tui-startup-check.py`](../../scripts/tui-startup-check.py) 与 `docs/adr/*` / `docs/*.md`**：按 `§` 号引用 `.scratch/*/spec.md`（如 `exit-gesture/spec.md §1`）—— 这也是 spec 被排除的原因。
- **`docs/tui-manual-checklist.md` 的条目编号被外部引用至少 5 处**（2026-10-04 票 01 实测）：[`docs/render.md`](../../docs/render.md) 的 ⑭、[`docs/goals.md`](../../docs/goals.md):190 的 ㉒、`.scratch/sandbox/spec.md`:175 / :185 的 ⑱（×2）、`.scratch/tui-chrome/spec.md`:206 的 ⑳、`.scratch/sidebar-toggle/spec.md`:4 的 ㉖ —— 票 06 已按「26 条全部还有活读者、不删条目」把编号默认冻结。

### Tracker 事实与降级（本图适用）

- map = `.scratch/docs-slim/map.md`，child = `.scratch/docs-slim/issues/NN-*.md`；阻塞 = 票面 `Blocked by: NN`；claim = `Status: claimed`；resolve = 票底 `## Answer` + `Status: resolved` + 追加一行到本文 `Decisions so far`。
- **没有 native sub-issue / 依赖边**，所以回退到正文约定：本文的 `## 任务清单` 逐条引用子票（条目数 == 子票文件数），每张子票顶部写 `Part of: ../map.md`。
- 校验：`python3 scripts/wayfinder-check.py .scratch/docs-slim/map.md`。宣布图走完之前必须 PASS。

## 任务清单

<!-- 条目数必须等于 issues/ 下的子票数（scripts/wayfinder-check.py 校验）。frontier 的权威查询仍是扫 issues/ 里 open + unblocked + unclaimed 的票。 -->

**决策票**（wayfinder 的七张，全部 `resolved`）：

- [x] [research：手工清单的条目现状](issues/01-research-manual-checklist-entries.md)
- [x] [research：`CONTEXT.md` 的实现细节清点](issues/02-research-context-implementation-details.md)
- [x] [research：超长文本块的豁免分类](issues/03-research-long-paragraph-triage.md)
- [x] [grilling：入口三份的重写形状与净行数目标](issues/04-grilling-entry-docs-shape.md)
- [x] [grilling：护栏脚本的契约](issues/05-grilling-guardrail-contract.md)
- [x] [grilling：清单与 lifecycle 的处置](issues/06-grilling-checklist-and-lifecycle.md)
- [x] [grilling：`CONTEXT.md` 的词条边界](issues/07-grilling-context-boundary.md)

**实现票**（`/to-tickets` 从 [`spec.md`](spec.md) 拆出的五张，2026-10-04）：

- [ ] [08 — 护栏落地：`scripts/check-doc-size.py` 与它的测试](issues/08-doc-size-guardrail.md)
- [ ] [09 — `CONTEXT.md` 剥到「一句定义 + 指针」](issues/09-context-glossary-boundary.md)
- [ ] [10 — 入口三份重写（含 `README.md` 的剩余超长单元）](issues/10-entry-docs-rewrite.md)
- [ ] [11 — 手工清单与 lifecycle 的处置](issues/11-checklist-and-lifecycle.md)
- [ ] [12 — 剩余单元清理与收口](issues/12-remainder-and-close-out.md)

共 **12** 张票（3 research + 4 grilling + 5 implement），当前 **7/12** —— 七张决策票全部 `resolved`（**图已走完**）；
五张实现票 `ready-for-agent`，**frontier = [08 — 护栏落地](issues/08-doc-size-guardrail.md)**（09 / 10 / 11 被它 block，
12 被 09–11 block）。决策票走完这张图就算走完了 —— 实现票住在同一个目录里，由 `/implement` 认领，不由 wayfinder 会话认领。

## Decisions so far

<!-- 索引：每条一行，够判断相关性即可；细节住在票里。按名字引用，不写裸编号。 -->

- [research：手工清单的条目现状](issues/01-research-manual-checklist-entries.md) — **26 条全部还有活读者、0 条整体失效**（功能都在 `src/`：1 条全自动、22 条混合、3 条纯手工）；要动的是 **4 处引用失效的旧外壳数字**（④.5 / ⑨ / ⑯.3 / ⑫.2，全是 `tui-chrome` 之前的），那属于「信息本身错了」、冻结项 4 覆盖不到 —— 归票 06。附带更正：写「光标、鼠标、缩放留手工」这条分界的是 `scripts/tui-startup-check.py` 的 docstring 与 `docs/render.md`:295，**不是** `docs/skills.md`。
- [research：`CONTEXT.md` 的实现细节清点](issues/02-research-context-implementation-details.md) — 72 个词条里**纯实现细节 246 处指称 + 193 条描述**，而格式必需的名字只有 **78 处**（约 1 : 3.2）；被剥内容 **92% 在别处已有一份**（66 full + 4 partial，仅 2 个 none）；剥完 **14 个词条会空掉或只剩标题**（渲染节占 8 个）。两条硬输入：`PromptHue` / `FallingDash` / `StatusRow` 三个「格式必需」槽位在 `src/` **查无此名**；`.scratch/tui-input-pulse/spec.md`:6 证明 **spec 主动往 `CONTEXT.md` 加词条是仍在生效的流程**。
- [research：超长文本块的豁免分类](issues/03-research-long-paragraph-triage.md) — **先修一个数**：冻结项 5 的「空行整块」把 335 个清单项压成 79 块，其中 60 块是伪问题；按**单元口径** ≥500 只有 **22**（整块 79）。分类：**可拆 68 / 是复述 11 / 需豁免 0**。豁免只有两条自动规则（R1 引文 ≥50%、R2 纯指针枚举），实测只命中 `README.md` 文档表 2 个单元格，**整段零豁免、白名单可空着起步**；1,529 个单元格里 ≥500 只有 5 个（最长 1,241）。**`AGENTS.md` 没有任何 ≥500 的单元** —— 冻结项 8 的删复述看不见，得走「同一规则两处维护」的判据。
- [grilling：入口三份的重写形状与净行数目标](issues/04-grilling-entry-docs-shape.md) — 三份形状定死：`.scratch/README.md` **极简 + 读法一段**（删 L5-9 约定复述与 7,429 字符流水账；表格结构不动、只精简 3 个超长单元格；读法留「三种形态 + `Status:` 词表 + 数法」与压成一行的需求池账，删掉已过期的四张图状态小结与两条一次性记录）；`README.md` 的「状态」段压到 **250–390**（只留结论 + 复核方式，数字一处）；`AGENTS.md` 的 `### Docs` 压到 **≤250 字符**（留指针与自身 schema 的引用块，删语言线复述）。**指标口径修正**：「行数不得增」不成立（拆段落必然加行），改为**非空白字符数 ≤ 上限 + 行数 ≤ 上限**；目标 18,100→≤13,500 / 19,954→≤18,500 / 1,225→≤950 字符，行数 ≤100 / ≤300 / ≤30。
- [grilling：护栏脚本的契约](issues/05-grilling-guardrail-contract.md) — `scripts/check-doc-size.py` 的判据全部拍定：口径是**单元**、**>500 违规**（=500 合格）、**表格单元格并入同一 500 指标**；阈值**单一硬线**不分档（配 `--list` 按长度打印全部单元）；豁免**规则进脚本、白名单空着起步**（R1 引文 / R2 纯指针枚举自动放行，R3–R5 打 `review:` 而不非零退出，`--list` 要打印每条规则当日命中数）；覆盖范围是**36 条硬编码字典** + 两条自检（清单内文件缺失报红、`docs/` 下有清单外的 `.md` 只提示）；入口三份走**实测起步的预算棘轮**（初值 = 今天实测 19,954 / 18,100 / 1,225 字符，票 04 的目标写进注释当终点），超长项按文件记**违规计数基线**（a1）；**脚本首装即绿**，它守「不许恶化」而不守「瘦身做完了没」；顺带在中文占比余量 < 0.5 点时打 `warn:`（今天最紧三份：`docs/tui-manual-checklist.md` 余量 0.19 点）。事实底账：scope 恰好 **36 份**（实算与 map 一致）；仓库**没有 CI 入口**；按 §4.1 实算今天有效违规 **23 项**（散文 / 清单侧 20、单元格侧 5、R2 豁免 2），分布 `CONTEXT.md` 8 / `.scratch/README.md` 6 / `README.md` 5 / 另四份各 1 —— 基线初值以脚本首跑实测为准。附注（本图 scope 外）：`docs/adr/0008` 与 `docs/adr/0010` 漏在 `check-language.py` 的 `DOCS_MIN_RATIO` 之外，从未被占比检查。
- [grilling：清单与 lifecycle 的处置](issues/06-grilling-checklist-and-lifecycle.md) — 26 条**一个不删**（票 01 判 0 条失效）；过时数字**直接改正**并在抬头留一行带日期的校正记录（不保留错误原文——这是操作清单不是历史记录）：④.5 → 转录 **10** 行、⑨.3 → **17** 行、⑯.3 → 输入区 **3** 行 / 转录 **3** 行、⑫.2 → **屏幕居中**且宽度基准由主列变屏幕（`min(屏幕宽−4,135)`，120 列下 **116** 而非 73；139 列才封顶）——**最后这处宽度票 01 没点出来**；⑨ 降级成指向两条测试的指针但**保留编号位置**（抬头的 `①③⑤⑥⑨` 去掉 ⑨）；`docs/lifecycle.md` 只压一处**同文件内部**的复述（L299-307 → 一句图注，权威在 §1.3 / §5），图 / 246 行证据表 / §1 规则是显式禁改项，且实测单元口径 **0 项**超长（票面「最长段 799」复现不出来）；清单**不设独立体量目标**（目标就是单元违规归零：今天 1 项 = 抬头 L7 的 737 字符枚举）；`①–㉖` 与 `⑫/⑯/⑱/⑳` 的二级编号**都不重排**（实测 8 个引用点，含「⑱ 第 3 条」「⑳ 第 5 条」这类条内引用）。
- [grilling：`CONTEXT.md` 的词条边界](issues/07-grilling-context-boundary.md) — 剥到**「一句定义 + 可选 `docs/` 指针」**（删 246 处指称 + 193 条描述，指针必须落到票 02 §7.3 那批真实来源）；**14 个「会空掉」的词条各补一句纯领域定义**，不许只剩标题；6 个别处没有的词条（问卷请求 / 问题选项 / 作答草稿 / 问卷文案 → `docs/render.md`，价目表 → `docs/observability.md`，落点 → `docs/discussion.md`）**把独有内容搬进 `docs/`**，不新增文件；9 处 `§N` 换成 `docs/` 链接（`spec §7` 存疑那处删），换完**归零**；**硬约束句全留**（维护者选了「全留」而非「按区分性拆」）——边界因此改为按**句子性质**分：留规范句、删描述句，一句两半就留规范那半；与其余 35 份**同受单元 ≤500 管**（实算 8 项违规，全 scope 最多）；四个「造名」槽位（`PromptHue` / `FallingDash` / `StatusRow` / `Pulse`）保留，改第 10 行的规矩为「也接受文档已采用的概念名」；**新术语规矩当场定死**：定义进 `CONTEXT.md`、机制进 `docs/`，不许在术语表里写机制。

## Not yet specified

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票。不要预先切成票那么大。 -->

- **收尾时「往哪写」还没有规矩**（原「流程本身在制造胖点」的后半，前半已随票毕业）。证据仍在：`.scratch/README.md` 近 60 次提交里被改了 **26 次**（全仓最高 churn），README 的「状态」段同理 —— 每次 feature 收尾都往入口文档**追加**一段逐日经过；[grilling：护栏脚本的契约](issues/05-grilling-guardrail-contract.md) 的预算棘轮只把这件事变成**可见**，没有改变它。**术语那一半已由 [grilling：`CONTEXT.md` 的词条边界](issues/07-grilling-context-boundary.md) 毕业**（新术语：定义进 `CONTEXT.md`、机制进 `docs/`，不许在术语表里写机制）。剩下这半要动的是**收尾流程本身**，比本图的 destination（一份文档瘦身 spec）更宽 —— 本图不处理，留给后续 effort。
- **入口文档的「状态」该由什么承担。** README 的「状态」段在替代一份本可以从 `.scratch/` 派生出来的索引（票数 / 完成度 / 测试数都能用命令数出来）。要不要让它是派生的、而不是手写的，还没想清 —— 同样比本图的 destination 更宽。

## Out of scope

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **`.scratch/*/issues/` 的 222 张已收尾票**（15,247 行）：2026-10-04 明确决定**不删**。「历史减存量」被重新定义为删入口文档里的复述（冻结项 4）。
- **`.scratch/*/spec.md` 与 `map.md`**：被 `scripts/*.py` 与 `docs/` 按 `§` 号引用，动它们要同步全仓引用。
- **`.scratch/*/seed.md` 的 10 条意向池**：只 1,167 行，删了收益小。
- **`docs/research/` 的 792KB 一手引文**：纪律是「一个字不改」（[`docs/research/README.md`](../../docs/research/README.md)、[`AGENTS.md`](../../AGENTS.md)）。
- **`.scratch/*/research/` 的 28 份一手调研**（1.2MB）：同上，是材料不是结论。
- **重排目录与改文件名**：`docs-tidy` 那一轮已经排除过。
- **翻译任何文档**：`docs/research/` 的引文之外，散文一律中文，这条线已经定了。
- **`src/` 里的模型可见文本**：那是代码不是文档，归 `check-language.py` 的棘轮管。
- **`check-language.py` 的 `DOCS_MIN_RATIO` 清单维护**：2026-10-04 票 05 实测 `docs/adr/0008`（占比 39.9%）与 `docs/adr/0010`（50.3%）从未被占比检查（该脚本的标题检查走 glob，所以只有占比这一条漏）。那是那份清单自己的事，本图只在新的 `scripts/check-doc-size.py` 里加一条同类自检（`docs/` 下存在但不在字典里的 `.md` 就提示）。

## 进度

**决策票 7/7，实现票 5/5** —— 七张决策票全部 `resolved`（2026-10-04；其中三张 HITL 票在同一个 session 里连走：护栏脚本的契约、清单与 lifecycle 的处置、`CONTEXT.md` 的词条边界），并已折成 [`spec.md`](spec.md)、由 `/to-tickets` 拆出五张实现票（08–12）。**两条路都走完了**（同日）：08 护栏落地（`scripts/check-doc-size.py` + 15 条只打 CLI 的测试 + `README.md` 接线）→ 09 / 10 / 11 并行（`CONTEXT.md` 剥到「一句定义 + 指针」、入口三份重写、手工清单与 lifecycle 的处置）→ 12 收口（两份 ADR 与 `docs/skills.md` 的最后三处、基线清零、四处索引对齐、全量复核）。收口时 36 份文档**单元违规全为 0**、入口三份都在预算内。

**决策这条路走完了**：36 份文档各自该改哪里、改到什么程度、用什么护栏复核，以及三处最容易出错的地方（手工清单的 5 处旧数字、lifecycle 的禁改边界、术语表的剥离线）都已写死在票的 `## Answer` 里。

**本图只产决策**：charting 与七张决策票的整个过程没有改动任何一份文档，改的只是 `.scratch/docs-slim/` 里的票、spec 与这张图。
