# 各 wayfinder 图的「交棒已发生」补记

Type: implement
Status: done

> 规格：`.scratch/docs-tidy/spec.md`（同类问题：**文档的状态说法与实际不符**）。来源：用户问「各个 map 都清了么？」—— 一次核对发现票确实都清了，但两张图的抬头还停在「下一步是 `/to-spec`」。

## 目标

四张 wayfinder 图的**票面状态**与**交棒状态**都在图上读得出来，不再让人（或 agent）去猜「这张图到底做完没有、产物是哪一份」。

## 具体行为

1. **核（用现成脚本，不靠印象）**：`scripts/wayfinder-check.py` 逐张跑 —— 四张全 PASS；`closed/total` 分别是 25/25、16/16、8/8、9/9。
2. **`multi-agent-architecture/map.md`**：抬头下补一条带日期的「交棒已发生」引用块，写明折成 `.scratch/fs-agent-v1/spec.md`、并在其下切出 32 张实现票（全部 `done`），本图此后只作决策存档。原文一字不改。
3. **`tui-ux/map.md`**：同一条，写明折成 `.scratch/tui-layout/spec.md` §14（§2 几何与 Mark header 阈值按本图回改）、实现提交号（`7e437a0` / `940cd43` / `0641c57` / `b48871c` / `538c4aa` / `530a191`）。
4. **`.scratch/README.md`**：四张 map 的「一句话」里补上**交棒产物**（折成哪份 spec），并加一段「四张图的状态」小结（四张全 PASS、没有 open 决策票 / 没有 unblocked frontier 票）。
5. `tui-layout` 与 `tui-history-replay` 两张**本来就在抬头/进度里写明了产物**，只做核对、不改。

## 测试

- `python3 scripts/wayfinder-check.py .scratch/{multi-agent-architecture,tui-layout,tui-ux,tui-history-replay}/map.md` 四张仍全 PASS（只加了引用块，不动任务清单）。
- 补记里引用的路径与提交号都能落地：`ls` 那两个 spec、`git log --oneline | grep <sha>`。
- 全仓相对链接扫描零坏链。

## 不做什么

删任何图；给图加新票；改写图里的历史决算；动 `issues/` 里的任何文件。

## Comments

**完成（2026-09-26）**。落点：`multi-agent-architecture/map.md`、`tui-ux/map.md`、`.scratch/README.md`。

1. **四张图逐张跑过 `scripts/wayfinder-check.py`，全 PASS**：`multi-agent-architecture` 25/25、`tui-layout` 16/16（9 resolved + 7 done）、`tui-ux` 8/8、`tui-history-replay` 9/9（5 resolved + 4 done）。没有 open 的决策票，也没有 unblocked 的 frontier 票（`Type: implement` 的票由 `/implement` 认领，不算 frontier）。
2. **两张图的抬头补了「交棒已发生」（带日期，原文不改）**：`multi-agent-architecture` → `.scratch/fs-agent-v1/spec.md`（其下 32 张实现票全部 done）；`tui-ux` → `.scratch/tui-layout/spec.md` §14（§2 几何与 Mark header 阈值按本图回改，实现提交号列在补记里）。
3. **另外两张本来就说清了**：`tui-layout` 抬头写着「已折叠完成，本图关闭。产物：spec.md」；`tui-history-replay` 抬头写着 5 张决策票 resolved + 已在同一目录切出 4 张实现票。这两张只核对，未改。
4. **`.scratch/README.md`**：四张 map 行的「一句话」补上**交棒产物**，并新增一段「四张 wayfinder 图的状态」小结（含核对日期与四张的 closed/total），让索引这一处就能回答「图清了没有」。
5. **核对**：补记里的两个 spec 路径存在；六个提交号 `git log` 可查；全仓相对链接扫描零坏链（研究笔记里那条上游路径除外，已在 `docs/research/README.md` 说明）。
