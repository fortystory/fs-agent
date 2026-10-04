# 12 — 剩余单元清理与收口

Type: implement
Status: done
Part of: ../map.md
Blocked by: 09, 10, 11

> 规格：[`../spec.md`](../spec.md) 的 Testing Decisions 与 Further Notes 的验收顺序。这一票不写新内容，
> 只把**最后两处散落的超长单元**清掉、把基线收到实测、把索引对齐，并把整件事验完。

## 目标

**36 份活文档全部单元 ≤500、11 块复述全部处理完**；护栏的基线收到实测值；四处索引与实际一致；
一次全量复核留下记录。做完这一票，这个 effort 从 tracker 的角度就算走完了。

## 现状（2026-10-04 核实，改前先复核）

- **本票之前应当已经清完的**：`CONTEXT.md`（8 项）、`.scratch/README.md`（6 项）、`README.md`（5 项）、
  `docs/tui-manual-checklist.md`（1 项）、`docs/render.md`（1 项）—— 分别归票 [09](09-context-glossary-boundary.md) /
  [10](10-entry-docs-rewrite.md) / [11](11-checklist-and-lifecycle.md)。
- **剩下这两处没有别的票管**（票 03 §2 的逐块表里，它们是仅剩的 ≥500 单元）：
  - `docs/adr/0003-plan-leaves-the-permission-modes.md` 的正文首段（约 524 字符）。
  - `docs/adr/0008-markdown-parsing-by-pulldown-cmark.md` 的一个编号项（约 551 字符）。
- **11 块复述的分配**：`.scratch/README.md` L5-9 / `README.md` L26 / `README.md` L174 / `README.md` L217 → 票 10；
  `CONTEXT.md` 的 5 处 → 票 09；`docs/render.md` L279-288 → 票 09；`docs/lifecycle.md` 那一块 → 票 11。
- **四处索引**：`.scratch/README.md` 的 `docs-slim` feature 行、`README.md`（若「状态」段留了数字）、
  `../spec.md` 的抬头、`../map.md` 的进度段。
- **两条命令**：`python3 scripts/check-doc-size.py`（票 08）与 `python3 scripts/check-language.py`；
  另外 `python3 scripts/lifecycle-check.py`、`python3 scripts/wayfinder-check.py .scratch/docs-slim/map.md`
  与 `cargo test` 都不能因为这一轮变红。
- **没有 CI**：验收是人手工跑的，命令写在 `README.md` 的「开发」一节。

## 落点

`docs/adr/0003-plan-leaves-the-permission-modes.md`、`docs/adr/0008-markdown-parsing-by-pulldown-cmark.md`、
`.scratch/README.md`、`README.md`（只在核对时修偏差）、`../spec.md`、`../map.md`。

## 具体行为

1. **拆掉最后两处**：两份 ADR 各一个 ≥500 的单元（只加换行 / 拆句，不改信息量；ADR 是散文，
   删中文会撞 `DOCS_MIN_RATIO` —— `docs/adr/0003` 余量 1.39 点，**优先拆**）。
2. **确认 36 份 0 违规**：`python3 scripts/check-doc-size.py --list` 逐个文件核过。
3. **基线收到实测**：把每份清完的文件基线显式收紧（本票之后 36 份应当全为 0）。
4. **索引对齐**：`.scratch/README.md` 的 `docs-slim` 行写成本轮的实际结果（形态 `map + spec`、
   `7 resolved + 5 done`、一句话）；`../spec.md` 抬头改成实际完成数；`../map.md` 的进度段补一句
   「已交棒、实现票落地情况」。
5. **手工破坏两次**：把某份文档的一个单元拉过 500 → 脚本退出 1 并指出位置；删掉某个既有词条的指针 → 该文件的
   基线仍按实测判定。两次都改回来。
6. **全量复核**：`python3 scripts/check-doc-size.py`、`python3 -m unittest`、`python3 scripts/check-language.py`、
   `python3 scripts/lifecycle-check.py`、`python3 scripts/wayfinder-check.py .scratch/docs-slim/map.md` 全绿；
   `cargo test` 仍绿（这一轮不该碰 `src/`）。

## 验证

1. 上面六条逐条有结论，写在本票底部的「结果」一节（照 [lifecycle-diagram 的收口票](../lifecycle-diagram/issues/14-close-out.md) 的写法）。
2. `python3 scripts/check-doc-size.py` 退出 0，且 `--list` 里 **36 份全部 0 违规**。
3. `git status` 只看得到本轮该动的文件；**没有误改 `src/`**。
4. **没有事件流层面的验收**：本 feature 不改运行时行为（`src/` 一个字节不动），所以 `sessions replay` 那类核对
   **不适用** —— 这一点要在票底写明，免得下一个人去找。
5. 人工走查清单本身照旧跑一遍（26 条，票 11 改正之后的版本）。

## 不做什么

- 不在收口票里改运行时行为，不重新论证任何决定（有异议就改对应的决策票或另开一条 bug）。
- 不把 `../research/` 里的材料搬进 `docs/`（那是材料，不是结论）。
- **不为了让脚本通过而放松脚本** —— 宁可改文档。
- 不动 `docs/research/` 与 `.scratch/*/spec.md`（冻结项 3）。

## Comments

### 结果（2026-10-04）

逐条对应票面的「具体行为」与「验证」。

1. **最后三处超长单元**（只加换行 / 拆段，不改信息量）：`docs/adr/0003` 的正文首段 524 → 两段；
   `docs/adr/0008` 的「`markdown.rs` 被重写」551 → 拆成两项；
   **外加第三处**：`docs/skills.md:39` 778 字符（块首是散文行、`- **disable-model-invocation…**` 起才是清单项）
   —— 票面与本票「现状」都只说两处 ADR，这是票 05 底账漏掉的那 1–3 项口径差之一（另一处
   `docs/tui-manual-checklist.md:126` 已在票 11 清掉）。加一个空行即拆开。
2. **36 份全部 0 违规**：`python3 scripts/check-doc-size.py` 退出 0，
   输出里**一条 `note:`（超长单元位置）都没有**，只剩一条 `warn:`（见下）。
   **11 块复述逐块核过**（research/03 §3 的清单）：`.scratch/README.md` L5-9 与 `README.md`
   L26 / L174 / L217 → 票 10；`CONTEXT.md` 的 5 处 → 票 09；`docs/render.md` L279-288 → 票 09
   （三处独有信息并回 README，见第 7 条）；`docs/lifecycle.md` L299-307 → 票 11。
   外加 `.scratch/README.md` 那 3 个 ≥500 的表格单元格 → 票 10。
3. **基线收到实测**：`VIOLATION_BASELINE` 已清空（首跑实测 25 项 → 收口后 0），注释写明
   「从今往后任何一份文档新增一个 >500 的单元都会立刻报红，不再有存量欠账兜着」。
4. **四处索引对齐**：`.scratch/README.md` 的 `docs-slim` 行改成「图 7/7、实现票 5/5、护栏是
   `scripts/check-doc-size.py`、36 份违规已清零」+ `7 resolved + 5 done`；本文件抬头 `spec.md`
   改成「5 张实现票全部 `done`」；`map.md` 的 `## 进度` 改成「决策票 7/7、实现票 5/5」并写下
   08 → 09/10/11 → 12 的实际路径（原「frontier」那一段随实现完成退场）；`README.md` 的规模数字
   按实测改成 **`src/` 41,921 行 / `tests/` 41,807 行 / 1,146 条测试**（票面说「只在核对时修偏差」——
   旧值是 41,781 / 41,575 / 1,140，行数与测试条数都已漂）。
5. **手工破坏两次**（改完都复原）：① 往 `docs/bash.md` 尾塞一个 524 字符段落 → 退出 **1**，
   指出 `docs/bash.md:162`，并报「违规 1 项 > 基线 0（不许恶化）」；② 删掉 `CONTEXT.md` 里一处
   「细节见 docs/render.md」→ 退出码仍为 **0**（没产生超长单元，基线按实测判定，不是无脑报红）。
6. **全量复核**（六条命令逐条有结论）：

   | 命令 | 结果 |
   | --- | --- |
   | `python3 scripts/check-doc-size.py` | 退出 0；36 份 0 违规，0 条 note |
   | `python3 -m unittest` | **33 条全绿**（原有 18 + 新 15） |
   | `python3 scripts/check-language.py` | 绿（中文占比与三条棘轮都未回退） |
   | `python3 scripts/lifecycle-check.py` | **PASS**（图与 246 行证据表一字未动；只有既有的行号漂移提示） |
   | `python3 scripts/wayfinder-check.py .scratch/docs-slim/map.md` | **PASS**（12 张子票与任务清单一致） |
   | `cargo test` | 退出 0，**1,146 条 passed、0 failed**（这一轮没碰 `src/`） |

7. **一处收口时才发现的缺口已补上**：`docs/render.md` 删掉「交互式 CLI」那块（research/03 §3 判为复述）
   时留了指向 `README.md`「跑」一节的指针，但那块有**三处 README 没有的信息**（`--tui` 与 `--plain`
   互斥、交互会话的 `--config`、`--continue` 的「先在本桶找、再全 store」）—— research 的原话是
   「三处独有信息并回 README」，而票 09 / 10 的票面都没认领这一步。已并回「跑」一节。
   因此 `README.md` 的预算在票 10 之后**再按实测收紧一次**：16,902 / 290 → **16,993 / 292**
   （仍远低于票 04 的 ≤18,500 / ≤300 终点；这是本轮唯一一次「先加信息、再跟着调棘轮」的显式动作）。
8. **唯一一条 `warn:`** 是 `docs/tui-manual-checklist.md` 的中文占比余量 **0.11 点**（46.11% / 下限 46）——
   票 11 已如实记账（⑨ 降级删中文散文的代价）；`warn:` 不改退出码。
9. **没有事件流层面的验收**：本 feature 一个字节的运行时行为都没改（`git status` 确认 **`src/` 无改动**），
   所以 `sessions replay` 那类核对**不适用** —— 下一个人不必去找。
10. **人工走查清单本身**：26 个条目、`①–㉖` 与二级编号都在（票 11 只改内容与数字），
    `docs/tui-manual-checklist.md` 的 5 处校正见票 11 的 Comments。清单本身「在真终端里逐项走一遍」
    仍是人的动作，不在本票的自动化范围内。

### 2026-10-04 `/code-review` 后的修正

两轴（Standards + Spec）的发现与处置：本票的收口记录原先用顶级 `## 结果`，违反
`docs/agents/issue-tracker.md`「评论与对话历史一律追加到文件底部、放在 `## Comments` 之下」——
已改成 `## Comments` + 本 `### 结果` 子标题（`lifecycle-diagram` 的收口票用的是顶级 `## 结果`，
那是它的破例，不跟着学）。`.scratch/README.md:3` 的相对链接补回 `../`（重写时漏了，成了断链）。
另外两处跨票发现落在票 08（单元第 ② 步、`--list` 自检、`--root`）与票 09（词条与 `docs/` 的重复）。

