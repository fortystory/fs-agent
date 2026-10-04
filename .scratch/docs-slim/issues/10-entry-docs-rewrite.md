# 10 — 入口三份重写（含 `README.md` 的剩余超长单元）

Type: implement
Status: ready-for-agent
Part of: ../map.md
Blocked by: 08

> 规格：[`../spec.md`](../spec.md) §3。形状与数值目标的完整理由在
> [入口三份的重写形状与净行数目标](04-grilling-entry-docs-shape.md) 的 `## Answer` 里；
> 复述块的判定在 [`../research/03-long-paragraph-triage.md`](../research/03-long-paragraph-triage.md) §3。

## 目标

三份入口文档落进预算，且**只删别处已有一份的复述**：`.scratch/README.md` 只剩「读法一段 + 那张表」；
`README.md` 的「状态」段只留结论与复核方式；`AGENTS.md` 的 `### Docs` 只留指针。`README.md` 里另外
**4 处超长单元与 2 块复述**一并处理（它们没有别的票管）。

## 现状（2026-10-04 核实，改前先复核）

- **三份的实测与目标**：`.scratch/README.md` 18,100 → ≤13,500 字符、82 → ≤100 行；`README.md`
  19,954 → ≤18,500、286 → ≤300 行；`AGENTS.md` 1,225 → ≤950、26 → ≤30 行（合计 39,279 → ≤32,950）。
- **`.scratch/README.md` 的病灶**：L5-9 是六条约定（权威在 `docs/agents/issue-tracker.md`）；第 11 行是一段
  **7,429 字符**的逐日流水账（约六成是复述）；三个单元格 729 / 564 / 562 也是逐日经过。**表格结构不动**。
  读法一段要留：三种形态（`spec` / `map` / `seed`）+ `Status:` 词表 + 数法命令 + 压成一行的需求池账；
  要删：已过期的「四张 wayfinder 图状态」小结与两条一次性记录。
- **`README.md` 的病灶**：「状态」段（L26，2,664 字符）逐条记 10-02 / 10-03 的落地；另外 L141（530）、
  L174（703）、L217（674）、L251（512）四处也 ≥500，其中 L174 与 L217 被判定为**复述**（分别压成指针）。
- **`AGENTS.md` 的病灶**：`### Docs` 601 字符（全文件最大的一节），其中约 380 是复述 `README.md`「文档」一节
  的语言线；**它没有任何 ≥500 的单元**，所以它只受本票的净指标约束，判据是「同一规则两处维护」。
- **占比安全性**：`AGENTS.md` 余量 6.9 点（最宽，删中文安全）；**`.scratch/README.md` 与 `README.md` 都不在
  `DOCS_MIN_RATIO` 清单里**，不受占比下限约束。
- **别混**：`.scratch/README.md` 的 feature 表里 `docs-slim` 自己那一行由 [票 12](12-remainder-and-close-out.md) 更新，不在本票。

## 落点

`.scratch/README.md`、`README.md`、`AGENTS.md`。

## 具体行为

1. **`.scratch/README.md`**：删 L5-9 与第 11 行那段流水账；补「读法一段」（四种内容见「现状」）；
   **只精简**表格里那三个超长单元格（表格结构、列数与行序不动）；把 L59-74、L76 之类的大段按单元拆开。
2. **`README.md` 的「状态」段压到 250–390 字符**：留「v1 的 34 张票全部 `done`」这个结论 + 怎么复核
   （`cargo test` / `wc -l`）；规模数字**只留一处**并写明数法（`docs-tidy` 立的规矩）。
3. **`README.md` 的另外四处**：L174 与 L217 压成指针（复述）；L141 与 L251 拆到单元合格。
4. **`AGENTS.md` 的 `### Docs` 压到 ≤250 字符**：留两个指针（`.scratch/README.md` 是 feature 索引；
   「新增文档前先读这两处、照邻居写」）与那段讲**本文件自身**小标题语言的引用块；语言线复述换成一句指针。
5. **口径**：硬线是「**非空白字符数 ≤ 上限 + 行数 ≤ 上限**」——「行数不得增」不成立，拆段落必然加行。
6. 改完把三份的基线按实测**显式收紧**（提交信息里写理由）。

## 验证

1. `python3 scripts/check-doc-size.py` 退出 0，且三份都在预算内（字符与行数各对一次）。
2. `python3 scripts/check-language.py` 全绿（尤其 `AGENTS.md` 删完中文后重跑）。
3. **逐条核对没删信息量**：三份里被删的每一处，都能在别处（各 feature 的 `spec.md` 抬头 / 票 / `docs/`）
   找到同一份内容；`README.md` 的「文档」表与 `AGENTS.md` 的指针**仍然指向真实存在的东西**。
4. `AGENTS.md` 的**五个英文小标题一字未动**（`grep -c '^### ' AGENTS.md` 与改前一致）。
5. `.scratch/README.md` 的表格仍是 43 行含 `|`（结构未动）、三个超长单元格变短。
6. `git status` 只看得到本票的三个落点。

## 不做什么

- 不重排 `.scratch/README.md` 的表格、不改 `README.md` 与 `AGENTS.md` 的其它章节。
- 不动 `AGENTS.md` 的五个英文小标题（技能工具链的锚点）。
- 不更新 `docs-slim` 自己那一行索引（归票 12）。
