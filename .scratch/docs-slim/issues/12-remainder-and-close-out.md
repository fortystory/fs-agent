# 12 — 剩余单元清理与收口

Type: implement
Status: ready-for-agent
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
