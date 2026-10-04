# grilling：护栏脚本的契约

Type: grilling
Status: resolved
Part of: ../map.md
Blocked by: 03

## Question

冻结项 10 选了「照邻居走」：新增 `scripts/check-doc-size.py`，配 `scripts/tests/` 的单测，`README.md` 的「开发」一节加一行。**邻居的形态已经定了**（读 [`scripts/check-language.py`](../../../scripts/check-language.py) 与 `scripts/tests/`），本票要定的是**这份脚本的判据与边界** —— 也就是「什么算违规」被写死在哪：

1. **段落口径 —— 票 03 已裁决，本票只需确认**（冻结项 5 已按它改写）：口径是**「单元」**（散文段，或一个顶层清单项连同它的续行与嵌套子项；表格单元格单独算单元），**不是「空行分隔的整块」**。理由可复核：整块口径把本 scope 的 335 个清单项压成 79 块、其中 60 块是伪问题；单元口径下 ≥500 只有 **22**（整块 79）。**别把原来的「≥500 约 60」写进验收 —— 它在任何口径下都复现不出来。**
   剩下的细节要拍：
   - **确认这套单元定义**（[`research/03-long-paragraph-triage.md`](../research/03-long-paragraph-triage.md) §4.1 给了可直接实现的四步）。
   - **表格单元格的上限**：票 03 实测 1,529 格里 ≥500 有 **5** 个、最长 **1,241**（在 `README.md` 文档表）。给单元格单设一条线（也取 500？）还是并入单元指标？
   - **豁免**：票 03 给了 **R1 引文（引用行 ≥50%）/ R2 纯指针枚举** 两条可自动放行的规则，以及 **R3 单一长句 / R4 不可断因果链 / R5 次序步骤** 三条「只打印复核、不非零退出」的候选。今天实测只自动命中 `README.md` 文档表 2 个单元格，**白名单可以空着起步** —— 确认这个「规则进脚本、名单留空」的姿态。
   - **回归样本**：调阈值时拿票 03 §2 的 79 行表 + §4.3 的 `README.md` L141 假阳性当回归集。
2. **阈值**：500 字符是硬线（冻结项 5）。要不要分档（「>500 告警、>1500 报错」）还是单一硬线？
3. **覆盖哪些文件**：scope 的 36 份怎么表达 —— 硬编码一份字典（像 `DOCS_MIN_RATIO` 那样），还是按目录 glob？`.scratch/*/issues/` 与 `docs/research/` 要显式排除（冻结项 3）。
4. **净行数指标怎么进脚本**：入口三份的目标（票 04 的产物）是硬上限（**棘轮**，只许降）还是一份**快照**（每次打印、只与上一次对比）？棘轮会让以后每次往 README 追加一句就报红 —— 这是真实取舍，与 `Not yet specified` 第一条雾直接相关。
5. **与 `check-language.py` 的关系**：两份脚本会不会打架（拆段落加换行，总字符数上升、CJK 数不变 ⇒ 中文占比**略降**）？要不要在 `check-doc-size.py` 里顺手报一句「某份文档的占比余量已 < 1%」（[`docs/tui-manual-checklist.md`](../../../docs/tui-manual-checklist.md) 现在只剩 0.2%）？
6. **退出码与接线**：照 `check-language.py` 的形态（0 / 非 0），进 `python3 -m unittest`；`README.md` 的「开发」一节加一行。

**依赖**：豁免规则与表格单元格的量化来自 [票 03](03-research-long-paragraph-triage.md)。

## 需要人拍板的点

- 上面六条各选一个方案（引用块 / 清单项 / 单元格算不算；单线还是分档；字典还是 glob）。
- 这一条尤其要拍：**净行数是棘轮还是快照** —— 它决定以后还能不能往入口文档追加内容。

## Answer

**结论：`scripts/check-doc-size.py` 的判据、边界与首装状态全部拍定** —— 口径是「单元」（>500 违规）、阈值单一硬线、豁免规则进脚本而名单空着、覆盖范围是一份 36 条硬编码字典、入口三份按「实测起步的预算棘轮」，**脚本首装即绿**。（2026-10-04 与维护者的 live exchange：第一轮 Q1–Q6，第二轮 Q7 补一条我上一轮没说清的后果。）

### Q1 单元定义与单元格归属 = (a)，边界取「>500 违规」

- **单元定义直接实现票 03 §4.1 的四步**：空行切块（表格行 / 围栏代码 / `#` 标题即断）→ 清单块按顶层项连同续行与嵌套子项各算一个单元、块首到第一个清单项之间的散文另算一个单元 → 其余整块算一个单元 → 表格单元格单独算一个单元。判据与现成代码片段见票 03 §4.1–4.3（原文 [`research/03-long-paragraph-triage.md`](../research/03-long-paragraph-triage.md) §4）。
- **表格单元格并入同一个 500 指标**，不单设线、不排除。今天 1,529 格里 ≥500 有 5 个：`README.md` 2 个（L242 / L244，纯指针枚举，被 R2 自动豁免）、`.scratch/README.md` 3 个（L43 729 / L53 564 / L47 562，「把逐日经过塞进索引表」的真病灶）。
- **边界：长度 ≤500 合格、免检；>500 才进豁免判定；判定不命中即违规。** 票 03 写的「≥500 进豁免判定」与冻结项 5 的「≤500」在恰好 500 处相抵，本票取后者。

### Q2 阈值 = 单一硬线，不分档

`>500` 违规，不设「告警 / 报错」两档。分档立刻引入「告警算不算过」的二义，而无人看的告警等于没有；真正的形状差别（**可拆** vs **是复述**）不是长度能判的。
补偿：`--list` 按长度排序打印**全部单元**（长度 + `文件:行` + 类型 + 是否命中豁免规则），调阈值时用它，不另存状态。

### Q3 豁免 = 规则进脚本、白名单空着起步

- **R1 引文（`>` 行字符 ≥50%）与 R2 纯指针枚举（链接跨度 ≥50% 且 `·`/`、` ≥4，只对清单项与单元格）自动放行**，判据照票 03 §4.2。
- **R3 单一长句 / R4 不可断因果链 / R5 次序步骤命中时打一行 `review:`、退出码照旧 0**（照 §4.3 的三个片段）。它们今天各命中 0。
- **不建独立白名单文件**；`--list` 必须打印每条规则的当日命中数与位置 —— 规则悄悄失效要能看见。

### Q4 覆盖范围 = 36 条硬编码字典 + 两条自检

字典（键是仓库相对路径，与 `DOCS_MIN_RATIO` 同形态）：

- 根与 tracker（4）：`README.md`、`CONTEXT.md`、`AGENTS.md`、`.scratch/README.md`
- `docs/` 逐面（18）：`bash` / `credentials` / `custom-tools` / `discussion` / `executor` / `goals` / `grep` / `highlight` / `lifecycle` / `mcp` / `observability` / `permissions` / `render` / `repo-map` / `sandbox` / `skills` / `tui-manual-checklist` / `web`（各 `.md`）
- `docs/adr/`（11）：`0001`–`0011`
- `docs/agents/`（3）：`domain` / `issue-tracker` / `triage-labels`

自检两条：① 清单里的文件不存在 → 非零退出（`check-language.py` 的既有形态）；② `docs/*.md` / `docs/adr/*.md` / `docs/agents/*.md` 下存在、但不在清单里的 `.md` → 打印提示、**不**非零退出。第 ② 条不是假想：`docs/adr/0008`（占比 39.9%）与 `docs/adr/0010`（50.3%）今天就漏在 `check-language.py` 的 `DOCS_MIN_RATIO` 之外（它的标题检查走 glob，所以只有占比这一条漏）。

### Q5 + Q7 入口三份的预算与首装状态 = 实测起步的棘轮（a1）

- **两张上限表硬编码在脚本里**：非空白字符数 ≤ 上限 + 行数 ≤ 上限（票 04 修正后的口径）。
- **初始值取今天实测，只许降**：`README.md` 19,954 / 286 行、`.scratch/README.md` 18,100 / 82 行、`AGENTS.md` 1,225 / 26 行。
- **票 04 的目标写进脚本注释当终点**：≤13,500 / ≤18,500 / ≤950 字符；行数 ≤100 / ≤300 / ≤30。
- **超长单元那一侧按文件记「违规计数基线」（a1），初值 = 脚本首次运行的实测**，只许降；脚本永远打印每一项违规的位置，失败判据是「某文件的违规数 > 该文件的基线」。清完一份就把该文件基线降到新实测值 —— **一次显式动作，在提交信息里写理由**（与 `MODEL_TEXT_FLOOR` / `COMMENT_FLOOR` 的注释同风格）。
- 因此 **`check-doc-size.py` 装上即绿**：它守的是「不许恶化」，不是「瘦身做完了没」—— 后者由 spec 与票记账。
- **已知弱点（a1 的代价，照实记下）**：同一文件里「清掉两项、又新增一项」总数不超时不会报红。它与 `MODEL_TEXT_FLOOR` 的弱点逐字同源，仓库已接受过这个取舍；真出问题再升级成 a2 的指纹基线（文件 + 单元首 20 字符 + 长度）。

票 03 的权威总量是 ≥500 单元 **22**、单元格 **5**（R1/R2 后豁免 2）。本轮按 §4.1 独立实算：**散文 / 清单侧 ≥500 有 20 个**（票 03 记 22，差 2，疑在清单顶层的判定）、**单元格侧 5 个**（与票 03 一致），R2 豁免 2 个后**有效违规 23 项**。逐文件近似基线：

| 文件 | 有效违规 |
| --- | ---: |
| `CONTEXT.md` | 8 |
| `.scratch/README.md` | 6 |
| `README.md` | 5 |
| `docs/render.md` | 1 |
| `docs/tui-manual-checklist.md` | 1 |
| `docs/adr/0003-plan-leaves-the-permission-modes.md` | 1 |
| `docs/adr/0008-markdown-parsing-by-pulldown-cmark.md` | 1 |

**基线初值以脚本首次运行的输出为准**，本表只作量级与分布核对（口径差 1–3 项，别拿它当验收）。分布本身与下游票吻合：两个大户正是 [票 07](07-grilling-context-boundary.md) 的 `CONTEXT.md` 与 [票 04](04-grilling-entry-docs-shape.md) 已定形状的 `.scratch/README.md`。

### Q6 与 `check-language.py` 的关系

- **顺带逐份算中文占比，余量 < 0.5 个百分点打一行 `warn:`，不非零退出。** 今天逼近的三份（本轮实算）：`docs/tui-manual-checklist.md` 46.19% / 下限 46（**余量 0.19**，距撞红只剩 115 个字符）、`docs/agents/triage-labels.md` 32.93% / 32（0.93）、`docs/goals.md` 39.95% / 39（0.95）。
- 拆段落（只加换行）几乎不伤占比 —— 那三份全部拆散也到不了额度；真正危险的是**删中文散文**（分子分母同减，而中文句的占比高于文档均值）。这条警告补的正是 `check-language.py` 目前**只在撞线后报红、不会提前警告**的缺口。
- 两份脚本不打架：`check-doc-size.py` 的覆盖严格更宽（多出 `CONTEXT.md` 与 `.scratch/README.md`），重叠部分方向一致。

### Q6 接线

- 退出码 0 / 非 0，`main()` 收集 `problems` 列表后统一打印（照 `check-language.py`）。
- 测试 `scripts/tests/test_check_doc_size.py`：**只断言 CLI**（退出码 + stdout 里有没有那一项），fixture 全写临时目录，跑法 `python3 -m unittest`（照 `scripts/tests/test_lifecycle_check.py`）。
- `README.md` 的「开发」一节加一行 `python3 scripts/check-doc-size.py`。
- **事实**：本仓库**没有 CI 入口**（无 `.github/workflows`、无 justfile / Makefile / pre-commit），所以接线就是这两处，没有「加进 CI」这一步；脚本守住 0/1 契约，以后有 CI 即插即用。

### 阈值调参的回归集（票 03 §4.4）

- [`research/03`](../research/03-long-paragraph-triage.md) §2 的 79 行整块表 + §4.3 的 `README.md` L141（530 字符）假阳性 —— R3 判据必须继续否掉它。
- 本票实算的 23 项与今天两条 R2 豁免（`README.md` L242 / L244）也要能复现。

### 交给 `/to-spec` 的执行要点

- 脚本是**新文件** `scripts/check-doc-size.py`，**不改** `check-language.py`。
- 判据全部来自本票，不要在 spec 里重开：口径（Q1）、阈值（Q2）、豁免（Q3）、字典（Q4）、预算与基线（Q5 / Q7）、接线（Q6）。
- `docs/adr/0008` / `0010` 漏在 `check-language.py` 的 `DOCS_MIN_RATIO` 之外，是**另一件事**（那份清单的维护），本图 scope 不动它；Q4 的第 ② 条自检在 `check-doc-size.py` 里防同类问题。
