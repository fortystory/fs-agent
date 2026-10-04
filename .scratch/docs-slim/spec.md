# 文档瘦身：36 份活文档的压表达与一条护栏脚本

Status: 5 张实现票 `ready-for-agent`（[`issues/08`](issues/08-doc-size-guardrail.md)–[`12`](issues/12-remainder-and-close-out.md)，
2026-10-04 由 wayfinder 决策图 [`map.md`](map.md) 折成 —— 十条冻结项 + 七张决策票全部 `resolved`；依赖边见各票抬头。
三份 research 与四张 grilling 的完整答案在各票的 `## Answer` 里，本文件是它们的构建计划）

这 36 份活文档按「被读的方式」分层：`AGENTS.md` **每轮全文注入**（最贵）；`README.md` / `CONTEXT.md` /
`.scratch/README.md` **每次上手读一次**；`docs/` 逐面与 `docs/adr/` **按需读一份**；`.scratch/*/issues/`
与 `research/` 几乎不被读。分层决定了瘦身的收益落在哪一层 —— 也决定了哪些文档不能碰。

问题不是「文档太多」，而是**表达在长胖**：入口文档里有一批「别处已经有一份」的复述，术语表在写实现，
手工清单里还留着旧外壳的数字，而**没有任何机器判据管长度**——今天能自动化的只有中文占比与 lifecycle 的对账。

来源与依据：

- 决策图 [`map.md`](map.md)：**十条冻结项**（charting 四轮 grilling 定下）+ **七张决策票**（全部
  `resolved`）。每条决定的完整理由与取舍都在票的 `## Answer` 里：三张 research（[手工清单条目现状](issues/01-research-manual-checklist-entries.md)、
  [`CONTEXT.md` 实现细节清点](issues/02-research-context-implementation-details.md)、[超长文本块分类与豁免判据](issues/03-research-long-paragraph-triage.md)）
  与四张 grilling（[入口三份的重写形状](issues/04-grilling-entry-docs-shape.md)、[护栏脚本的契约](issues/05-grilling-guardrail-contract.md)、
  [清单与 lifecycle 的处置](issues/06-grilling-checklist-and-lifecycle.md)、[`CONTEXT.md` 的词条边界](issues/07-grilling-context-boundary.md)）。
- 三份只读取证：[`research/01`](research/01-manual-checklist-entries.md)（26 行逐条表 + 推翻链 + 被引用编号）、
  [`research/02`](research/02-context-implementation-details.md)（逐词条四类片段 + coverage）、
  [`research/03`](research/03-long-paragraph-triage.md)（四种口径基线 + 可执行的豁免代码 + 表格单元格量化）。
- 会撞的既有护栏：[`scripts/check-language.py`](../../scripts/check-language.py)（中文占比下限 + 三条棘轮）、
  [`scripts/lifecycle-check.py`](../../scripts/lifecycle-check.py)（`docs/lifecycle.md` 的图与证据表逐行对账）、
  [`scripts/tui-startup-check.py`](../../scripts/tui-startup-check.py)（TUI 启动冒烟）。仓库**没有 CI 入口**。

## 问题陈述

1. **入口文档在长胖，而且是流程在制造它。** `.scratch/README.md` 近 60 次提交里被改了 **26 次**（全仓最高
   churn），`README.md` 的「状态」段同理：每次 feature 收尾都往入口文档**追加**一段逐日经过，而那些经过在
   各 feature 自己的 `spec.md` 抬头与票里已经有一份。规模已经是 `.scratch/` 324 文件 / 36,273 行 / 3,530KB。
2. **最贵的文档反而没被读对。** `AGENTS.md` 是唯一每轮全文注入的散文，它的 `### Docs` 一节 601 字符里有约
   380 字符在复述 `README.md` 的「文档」一节 —— 同一条规则两处维护，就是漂移源。
3. **术语表违反自己立的界。** [`CONTEXT.md`](../../CONTEXT.md) 开头写「它不含实现决策」，实测 72 个词条里有
   **246 处实现指称 + 193 条实现行为描述**，而格式必需的英文槽位只有 **78 处**（约 1 : 3.2）。
4. **手工清单里有错的信息。** [`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) 的 26 个条目
   全部还有活读者（0 条失效），但其中 **5 处引用着 `tui-chrome` 改外壳之前的旧数字**——走查人会按错的标准判回归。
5. **长度没有机器判据。** 39 份文档里没有一个脚本管「一段有多长」；最长的一个单元 7,429 字符，而冻结项 5 定的
   阈值是 500。口径本身也没定义过：按「空行分隔的整块」量，335 个顶层清单项会被压成 79 块、其中 60 块是伪问题。
6. **中文化护栏只在撞线后报红。** `check-language.py` 的 `DOCS_MIN_RATIO` 是**下限**，而
   [`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) 的余量只剩 **0.19 个百分点** ——
   瘦身要删中文散文时会直接撞红，今天没有任何东西会提前警告。

## 方案

**压表达，不减信息**：只拆段落，只删「别处已有一份的复述」，不动 `DOCS_MIN_RATIO`，**不删任何文件**。

1. **统一的密度指标**：以「单元」为单位（散文段，或一个顶层清单项连同它的续行与嵌套子项；表格单元格单独算），
   硬线 **>500 字符违规**。36 份文档用一份硬编码字典表达。
2. **一条新护栏**：`scripts/check-doc-size.py` + 单测 + `README.md` 的「开发」一节一行 —— 形态照
   [`scripts/check-language.py`](../../scripts/check-language.py) 与 [`scripts/tests/test_lifecycle_check.py`](../../scripts/tests/test_lifecycle_check.py)。
   它**首装即绿**：守「不许恶化」，不守「瘦身做完了没」。
3. **三份入口文档按定死的形状重写**（`.scratch/README.md` 极简 + 读法一段；`README.md` 的「状态」段压到
   250–390 字符；`AGENTS.md` 的 `### Docs` 压到 ≤250 且只留指针）。
4. **[`CONTEXT.md`](../../CONTEXT.md) 剥到「一句定义 + 可选 `docs/` 指针」**：删机制，留规范；
   6 个「别处没有一份」的词条的独有内容搬进 `docs/`。
5. **手工清单直接改正 5 处旧数字，⑨ 降级成指向测试的指针**（保留编号）；[`docs/lifecycle.md`](../../docs/lifecycle.md)
   只压一处同文件内部的复述，图与证据表一个字不动。
6. **新术语的落点当场定死**（定义进 `CONTEXT.md`、机制进 `docs/`），否则剥完会以同样的方式长回来。

## User Stories

1. 作为**刚接手这个仓库的开发者**，我想要入口文档只讲结论与指针，以便我上手时不必读一份逐日流水账。
2. 作为**每轮都在付注入成本的会话**，我想要 `AGENTS.md` 只留「索引在哪」而不复述语言约定，以便每轮的固定开销更小。
3. 作为**维护者**，我想要「同一条规则只有一处维护」，以便它不会在两次收尾之间漂移。
4. 作为**读术语表的人**，我想要每个词条先给我一句「它是什么」，细节想去再点链接，以便我能在一页里分清近义词。
5. 作为**读术语表的人**，我想要那些「它不是什么」的边界句留下，以便我不会把挂起与「回合继续跑」、讨论者与落点混为一谈。
6. 作为**未来的会话**，我想要新术语有一个写死的落点与写法（定义进术语表、机制进逐面文档），以便剥完的东西不会长回来。
7. 作为**照着手工清单走查的人**，我想要清单里的数字与今天的外壳一致，以便我看不见预期现象时能确信那是回归。
8. 作为**照着手工清单走查的人**，我想要已被自动化逐值钉住的条目不再占我的手，以便我只做真终端才能验的事。
9. 作为**维护者**，我想要 `CONTEXT.md` 里不再出现 `§N`，以便每个词条都能落到一份按需读得到的 `docs/` 文档。
10. 作为**维护者**，我想要那 6 个「文档层查无此物」的词条的独有内容落进 `docs/`，以便术语表剥完不丢信息。
11. 作为**改文档的人**，我想要一条命令告诉我「哪个单元超了 500」，以便我不靠感觉判断。
12. 作为**改文档的人**，我想要这条命令给出全部超长单元的长度与位置、以及每条豁免规则的当日命中数，以便调阈值时有依据。
13. 作为**改文档的人**，我想要引文与纯指针枚举被自动放行，以便索引表那种「拆开只会拆散指针」的单元格不被误报。
14. 作为**维护者**，我想要入口三份的体量有预算上限，以便「往入口文档追加」这件事会撞到一堵看得见的墙。
15. 作为**维护者**，我想要这份脚本装上就是绿的，以便它作为「不许恶化」的棘轮而不是一份长期红灯的待办。
16. 作为**清理一份文档的人**，我想要清完之后把该文件的基线显式收紧，以便进度是可见的、且收紧是一次写进提交信息的动作。
17. 作为**维护者**，我想要已经超长的那些单元被记成按文件的计数基线，以便新增的超长单元立刻报红、而存量清理不被一条红命令堵住。
18. 作为**改中文文档的人**，我想要脚本在中文占比余量不足 0.5 个百分点时打一行 `warn:`，以便我在撞红之前就知道。
19. 作为**维护者**，我想要新脚本的覆盖清单是具名的 36 条、并自检「清单里的文件不存在」与「`docs/` 下有清单外的 md」，
   以便它不会像 `DOCS_MIN_RATIO` 那样悄悄漏掉两份 ADR。
20. 作为**维护者**，我想要 `docs/lifecycle.md` 的图、证据表与 §1 的方言规则被写成显式禁改项，以便实现者不会顺手去改那张逐行对账的图。
21. 作为**维护者**，我想要手工清单的 `①–㉖` 与 `⑫ / ⑯ / ⑱ / ⑳` 的二级编号都不重排，以便 8 处外部引用（含「⑱ 第 3 条」这类）不失效。
22. 作为**读 `docs/` 逐面文档的人**，我想要那 6 条从术语表搬过来的实现细节读起来仍是中文散文，以便中文占比不因此被拉低。

## Implementation Decisions

### 1. 指标与口径（冻结项 5 + [超长文本块分类与豁免判据](issues/03-research-long-paragraph-triage.md)）

- **单元定义**（四步，可直接实现）：① 按空行切块，遇到表格行、围栏代码块、`#` 标题即断；② 块若是清单块，
  每个**顶层清单项**连同它的续行与嵌套子项算一个单元，块首到第一个清单项之间的散文另算一个单元；
  ③ 其余整块算一个单元；④ **表格单元格单独算一个单元**。
- **硬线**：长度 **≤500 合格、免检**；**>500 才进豁免判定**，判定不命中即违规。表格单元格**并入同一个
  500 指标**（不单设线）。今天 1,529 个非空单元格里 ≥500 有 5 个，其中 `README.md` 的 2 个是纯指针枚举（被 R2 豁免）、
  `.scratch/README.md` 的 3 个（729 / 564 / 562）是「把逐日经过塞进索引表」的真病灶。
- **基线口径修正**：**不要**把「≥500 约 60」写进验收 —— 那是「空行整块」口径的产物（79 个整块里 60 个是清单块）。
  按单元口径，≥500 是 **22 个**（整块 79）；单元格 ≥500 是 5 个、最长 1,241。
- **只拆不删**：拆段落只加换行；删中文散文会撞 `DOCS_MIN_RATIO` 的下限（冻结项 6 决定不动那个下限）。
  **只删「别处已有一份的复述」**（各 feature 的 `spec.md` / 票里已经写过的逐日经过）。

### 2. 护栏脚本 `scripts/check-doc-size.py` 的契约（[护栏脚本的契约](issues/05-grilling-guardrail-contract.md)）

- **单一口径、单一硬线**：不分「告警 / 报错」两档；`--list` 按长度排序打印**全部单元**（长度 + `文件:行` + 类型 +
  是否命中规则），调阈值时用它，不另存状态。
- **豁免规则进脚本、白名单空着起步**：
  - **R1 引文**：单元里以 `>` 开头的行占单元字符 ≥50% → 自动放行。
  - **R2 纯指针枚举**：链接跨度占单元 ≥50% **且** `·` / `、` ≥4（只对清单项与单元格）→ 自动放行。
  - **R3 单一长句 / R4 不可断因果链 / R5 次序步骤**：命中时打一行 `review:`，**退出码不受影响**。
  - 判据的代码片段与校准样本见 [`research/03`](research/03-long-paragraph-triage.md) §4.2–4.3；`--list` 要打印
    **每条规则当日的命中数与位置**（规则悄悄失效要能看见）。
- **覆盖范围 = 36 条硬编码字典**（同 `DOCS_MIN_RATIO` 的形态）：`README.md`、`CONTEXT.md`、`AGENTS.md`、
  `.scratch/README.md`，`docs/` 逐面 18 份（`bash` / `credentials` / `custom-tools` / `discussion` / `executor` /
  `goals` / `grep` / `highlight` / `lifecycle` / `mcp` / `observability` / `permissions` / `render` / `repo-map` /
  `sandbox` / `skills` / `tui-manual-checklist` / `web`），`docs/adr/` 11 份（0001–0011），`docs/agents/` 3 份
  （`domain` / `issue-tracker` / `triage-labels`）。两条自检：**清单里的文件不存在 → 非零退出**；
  **`docs/` 下存在但不在清单里的 `.md` → 打印提示、不非零退出**。
- **入口三份的预算**：两张表都硬编码 —— **非空白字符数 ≤ 上限** + **行数 ≤ 上限**。
  **初始值取今天实测，只许降**：`README.md` 19,954 / 286 行、`.scratch/README.md` 18,100 / 82 行、
  `AGENTS.md` 1,225 / 26 行。**票 04 的目标写进脚本注释当终点**：≤13,500 / ≤18,500 / ≤950 字符，
  行数 ≤100 / ≤300 / ≤30（合计 39,279 → ≤32,950）。
- **超长项按文件记「违规计数基线」**：脚本永远打印每一项违规的位置，失败判据是「某文件的违规数 > 该文件的基线」。
  基线初值以脚本**首次运行的实测**为准（本轮独立实算：有效违规 23 项 —— 散文 / 清单侧 20、单元格侧 5、R2 豁免 2；
  分布 `CONTEXT.md` 8 / `.scratch/README.md` 6 / `README.md` 5 / `docs/render.md` 1 / `docs/tui-manual-checklist.md` 1 /
  `docs/adr/0003` 1 / `docs/adr/0008` 1；口径差 1–3 项，**别拿这张表当验收**）。清完一份就把该文件基线降到新实测值 ——
  一次**显式动作**，在提交信息里写理由（与 `MODEL_TEXT_FLOOR` / `COMMENT_FLOOR` 的注释同风格）。
- **首装即绿**：它守的是「不许恶化」。已知弱点照实接受：同一文件里「清掉两项、又新增一项」总数不超时不报红
  （与 `MODEL_TEXT_FLOOR` 的弱点同源）。
- **占比余量警告**：顺带逐份算中文占比，**余量 < 0.5 个百分点打一行 `warn:`、不非零退出**。今天最紧的三份：
  [`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) 46.19% / 下限 46（**余量 0.19**，距撞红约 115 字符）、
  [`docs/agents/triage-labels.md`](../../docs/agents/triage-labels.md) 32.93% / 32（0.93）、
  [`docs/goals.md`](../../docs/goals.md) 39.95% / 39（0.95）。**拆段落几乎不伤占比，删中文散文才危险**。
- **退出码与接线**：0 / 非 0；测试进 `python3 -m unittest`（发现 `scripts/tests/`）；
  `README.md` 的「开发」一节加一行 `python3 scripts/check-doc-size.py`。仓库没有 CI 入口，**没有「加进 CI」这一步**。
- **不改 `check-language.py`**。

### 3. 入口三份的重写形状与目标（[入口三份的重写形状](issues/04-grilling-entry-docs-shape.md)）

- **`.scratch/README.md`：极简 + 读法一段。** 删掉 L5-9 的六条约定（权威在 `docs/agents/issue-tracker.md`）与第 11 行
  那段 **7,429 字符**的逐日流水账（约六成是复述）；保留「读法一段」= 三种形态（`spec` / `map` / `seed`）+
  `Status:` 词表 + 数法命令 + 压成一行的需求池账；**删**已过期的四张图状态小结与两条一次性记录。
  **表格结构不动**，只精简 3 个写满逐日经过的单元格（729 / 564 / 562）。
- **`README.md` 的「状态」段：2,664 → 250–390 字符。** 只留「v1 的 34 张票全部 `done`」这个结论 + 怎么复核
  （`cargo test` / `wc -l`）；规模数字只留一处并写明数法（`docs-tidy` 立的规矩）。
- **`AGENTS.md` 的 `### Docs`：601 → ≤250 字符。** 留两个指针（`.scratch/README.md` 是 feature 索引；新增文档前
  先读这两处、照邻居写）与那段讲**本文件自身**小标题语言的引用块；约 380 字符的语言线复述换成一句指针。
  **它那五个英文小标题是技能工具链的锚点，不动**；注意它**没有任何 ≥500 的单元**，唯一护栏是冻结项 7 的净指标，
  判据是「同一规则两处维护」。
- **数值目标表**（上限是「不得超过」而不是「必须压到」）：

  | 文档 | 非空白字符：现在 → 上限 | 行数：现在 → 上限 |
  | --- | --- | --- |
  | `.scratch/README.md` | 18,100 → **≤13,500** | 82 → **≤100** |
  | `README.md` | 19,954 → **≤18,500** | 286 → **≤300** |
  | `AGENTS.md` | 1,225 → **≤950** | 26 → **≤30** |

  **「行数不得增」不成立**（拆段落必然加行）——口径是「非空白字符数 ≤ 上限 + 行数 ≤ 上限」。

### 4. `CONTEXT.md` 的剥离线（[`CONTEXT.md` 的词条边界](issues/07-grilling-context-boundary.md)）

- **目标形态**：`**中文名（English）**: <这个词是什么，一到两句>。细节见 <docs 链接>`。删掉 **246 处指称 + 193 条
  实现行为描述**（机制名、内部类型与事件变体、调用顺序、参数与阈值）；**指针必须落到一份真实存在的来源**
  （[`research/02`](research/02-context-implementation-details.md) §7.3 是按词条抽查过的落点表）。
- **14 个「会空掉」的词条不许只剩标题**：给每个补**一句纯领域定义**（它是什么、读者凭什么认出它），再接指针。
  渲染节占其中 8 个。
- **6 个「别处没有一份」的词条，独有内容搬进 `docs/`**（**不新增文件**）：问卷请求 / 问题选项（均 coverage `none`）、
  作答草稿 / 问卷文案（partial）→ `docs/render.md` 的问卷一节；价目表 → `docs/observability.md`；落点 → `docs/discussion.md`。
  搬完术语表只留定义 + 指针。
- **9 处 `§N` 全部换成 `docs/` 链接**（`spec §5` / `§15` → `docs/discussion.md`；6 处裸 `§N` 各按落点表；**`spec §7` 存疑 → 删**）。
  换完 `CONTEXT.md` 里**不再出现任何 `§N`**。
- **硬约束句全部留下**（「不做 / 永远不 / 一律 / 绝不」算定义的一部分）。可执行的边界按**句子性质**分：
  **留规范句**（它**应当**怎样），**删描述句**（它**事实上**怎么搭起来）；一句两半都有的，留规范那半、删描述那半。
- **与其余 35 份同受单元 ≤500 管**：不豁免。今天它有 **8 项超长违规（全 scope 最多）**，剥离机制后多数自然变短，
  剩下的按顶层项 / 段落拆开。
- **四个「造名」槽位保留**（`PromptHue` / `FallingDash` / `StatusRow` / `Pulse`），第 10 行的规矩改为
  「英文槽位是代码里的标识符 / 类型名；**若这个概念只在 spec 与文档里立了名、代码里没有对应实体，槽位就写那个
  已被文档采用的写法**」。它们的真实代码形态（`PROMPT_HUE_PER_SECOND` / `DASH_FALL` / `wording::status_row()`）
  **不进 `CONTEXT.md` 正文**，随指针去 `docs/`。

### 5. 手工清单与 lifecycle（[清单与 lifecycle 的处置](issues/06-grilling-checklist-and-lifecycle.md)）

- **26 个条目一个不删**（票 01 判 0 条失效）；**过时数字直接改正**，并在抬头加一行带日期的校正记录
  （**不逐条保留错误原文** —— 这是操作清单，不是历史记录）：
  - ④.5「转录被压到只剩 1 行」→ **10 行**（`转录 = h − 4 − 输入行数`）。
  - ⑨ 的标题与 ⑨.2「7 行」、⑨.3「回到 14 行」→ 见下（⑨ 降级，这两个数字不再出现在手工步骤里）。
  - ⑯.3「输入区只有 2 行、转录只剩 1 行」→ **输入区 3 行**（`MIN_INPUT_ROWS` 的地板）、**转录 3 行**。
  - ⑫.2「居中于主列，宽度 `min(主列宽 − 4, 135)`（120 列下 73）」+「左栏不被压住」→ **屏幕居中**、宽度
    `min(屏幕宽 − 4, 135)`（120 列下 **116**；139 列以上才封顶）、**压住左栏与输入区左边那几列**。
    **宽度基准也错**（从主列改成屏幕）——这是票 01 没点出来的一处，依据是 `src/render/layout.rs` 的 `detail_width()`。
- **⑨ 降级成指针、保留编号位置**：标题改成「⑨ 120×24 的几何（已自动化）」，正文写成「几何由
  `a_tall_draft_costs_the_transcript_and_never_the_sidebar` 与
  `the_input_area_holds_three_rows_before_it_grows_and_the_transcript_pays_for_it` 逐值钉住，不手工走」；
  抬头「下面 ①③⑤⑥⑨ 需要……」去掉 ⑨。
- **编号不重排**：`①–㉖` 与 `⑫ / ⑯ / ⑱ / ⑳` 的**二级编号**都不动（实测 8 个引用点，含「⑱ 第 3 条」「⑳ 第 5 条」这类）。
- **`docs/lifecycle.md`**：**禁改**五张 mermaid 图、246 行证据表、§1 的方言与七条对账规则（受
  `scripts/lifecycle-check.py` 逐行对账）；**唯一改动**是把「三处必须说清的地方」那一块压成一句图注（权威版本在
  同文件 §1.3 与 §5）。**段落指标 0 项**：实测它没有一个 ≥500 的单元（最长 352），**不需要拆段**（票面的「最长段 799」
  复现不出来，以脚本口径为准）。
- **清单不设独立的体量目标**：目标是**该文件的单元违规归零**（今天 1 项 = 抬头「来源：……」那段 737 字符的枚举，
  按行拆开即达标）；`spec` 里不写字符数 / 行数上限。

### 6. 新术语的落点当场定死（[`CONTEXT.md` 的词条边界](issues/07-grilling-context-boundary.md)）

> **新术语：定义进 `CONTEXT.md`（一句 + 可选指针），机制进 `docs/` 逐面。不许在 `CONTEXT.md` 里写机制。**

它治的是 `.scratch/tui-input-pulse/spec.md`:6 那条**仍在生效**的流程（「术语：`CONTEXT.md`（§渲染）新增提示符色相 /
脉冲 / 下落短横」）——不加这条规矩，剥完会以同样的方式长回来。

### 7. 会撞的既有决定（实现时按此处理，不另开决定）

- **`scripts/check-language.py` 的 `DOCS_MIN_RATIO`**：中文占比**下限**，删中文散文会让它报红。本 spec 选「只拆 + 只删复述」，
  所以**每一份被改的文档改完都要重跑它**；三份余量最紧的已列在 §2。
- **增补 `docs/observability.md` 与 `docs/render.md` 时**（§4 的 6 条搬运）：余量分别只有 2.53 点（约 163 字符的纯英文额度）
  与 1.87 点（约 543 字符）。**用中文叙述包住标识符**，别把代码清单直接贴进去，改完重跑 `check-language.py`。
- **`scripts/lifecycle-check.py`**：逐行对账 `docs/lifecycle.md` 的图与证据表 → 见 §5 的禁改项。
- **`scripts/tui-startup-check.py` 与 `docs/adr/*` / `docs/*.md`**：按 `§` 号引用 `.scratch/*/spec.md` → 这也是
  `.scratch/*/spec.md` 与 `map.md` 在 `明确不做` 里的原因。
- **`docs/tui-manual-checklist.md` 的条目编号被外部引用**至少 8 个点（`docs/render.md` 的 ⑭、`docs/goals.md` 的 ㉒、
  `.scratch/sandbox/spec.md` 的 ⑱ ×3、`.scratch/tui-chrome/spec.md` 的 ⑳ ×2、`.scratch/sidebar-toggle/spec.md` 的 ㉖）→ 见 §5 的编号禁改。

## Testing Decisions

**只在一个 seam 上测：`scripts/check-doc-size.py` 的命令行接口（退出码 + stdout）。** 这是该脚本对外的唯一接口，
也是这个 feature 全部自动可验部分的承担者 —— 文档改动本身没有别的可自动断言的面。

- **测什么**：断言只打在**退出码**与 **stdout 里有没有那一项**上，不打内部函数（实现可以随意重构）。
  逐条覆盖：单元 >500 报红且 =500 合格；豁免的三类（R1 / R2 自动放行、R3–R5 只打 `review:` 且退出码为 0）；
  `--list` 打印全部单元与每条规则的当日命中数；36 条字典的存在性自检与「`docs/` 下有清单外的 md」提示；
  入口三份的字符 / 行数上限与违规计数基线（**首装即绿**：全部 fixture 在基线内时退出码为 0）；
  中文占比余量 < 0.5 点时打 `warn:` 而退出码不变；清单外文件的提示不改变退出码。
- **fixture**：全部写在临时目录里，**不依赖真实仓库文件**（真实文档的状态由人工走查与下面这条复核）。
- **prior art**：[`scripts/tests/test_lifecycle_check.py`](../../scripts/tests/test_lifecycle_check.py) —— 同一形态
  （只断言 CLI、fixture 在 `tempfile`、跑法是 `python3 -m unittest`）。
- **不做单元测试的部分**：所有文档改动（重写入口三份、剥 `CONTEXT.md`、改正手工清单、压 lifecycle 一处）。
  它们的验收是**命令**而不是断言：`python3 scripts/check-language.py`（中文占比与三条棘轮）、
  `python3 scripts/check-doc-size.py`（单元 / 预算 / 基线）、`python3 scripts/wayfinder-check.py .scratch/docs-slim/map.md`
  （本图的票与清单一致）、以及人工走查清单本身（改完之后 26 条照旧走一遍）。
- 测试模块的写法遵守仓库惯例：注释与断言消息用中文（ADR 0004 / 0005）。

## 明确不做

- **不删任何文件。** `.scratch/*/issues/` 的 222 张已收尾票（15,247 行）、`.scratch/*/spec.md` 与 `map.md`
  （被 `scripts/*.py` 与 `docs/` 按 `§` 号引用）、`.scratch/*/seed.md` 的意向池、`docs/research/` 的 792KB 一手引文
  （纪律是「一个字不改」）、`.scratch/*/research/` 的 28 份一手调研 —— 全部不动。
- **不动 `scripts/check-language.py` 的 `DOCS_MIN_RATIO` 下限**：瘦身只「拆 + 删复述」，不调这条线。
- **不重排目录、不改文件名**（`docs-tidy` 那一轮已经排除过）。
- **不翻译任何文档**（散文一律中文这条线已定）。
- **不动 `src/` 里的模型可见文本**（那是代码，归 `check-language.py` 的棘轮管）。
- **不维护 `check-language.py` 的 `DOCS_MIN_RATIO` 清单**：实测 `docs/adr/0008`（39.9%）与 `docs/adr/0010`（50.3%）
  从未被占比检查；那是那份清单自己的事。本 spec 只在 `check-doc-size.py` 里加同类自检。
- **不动 `docs/lifecycle.md` 的图、246 行证据表与 §1 规则**；**不动手工清单的 26 个条目与全部编号**
  （含二级编号）；**不动 `AGENTS.md` 的五个英文小标题**。
- **不处理两条比本 spec 更宽的流程问题**：① 每次 feature 收尾「往入口文档追加逐日经过」该改成什么（本 spec 的
  预算棘轮只让它变得可见）；② 入口文档的「状态」段该不该由 `.scratch/` 派生而不是手写。两者留给后续 effort。

## Further Notes

- **这份 spec 不重复细节**：每条决定的完整理由、取舍与被否掉的替代方案都在七张票的 `## Answer` 里；[`map.md`](map.md)
  的 `Decisions so far` 是一行式索引。本文件是构建计划，票是决策记录。
- **这一轮到目前为止只产决策**：`docs-slim` 走完七张票的过程中**没有改动任何一份文档**，改的只有 `.scratch/docs-slim/` 里的票与那张图。
- **验收顺序建议**：先落 `scripts/check-doc-size.py` 与它的测试（首装即绿，成为后面每一步的尺子），
  再改 `CONTEXT.md`（最大的一块），再改三份入口文档（有明确数值目标），最后是手工清单与 lifecycle 的两处小改动；
  每落一份就重跑 `check-language.py` 与 `check-doc-size.py`，并按 §2 收紧该文件的基线。
- **本 spec 的落点**：`.scratch/docs-slim/spec.md`；实现票由 `/to-tickets` 从这个文件与各票拆出。
