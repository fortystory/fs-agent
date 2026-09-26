# README：补全《文档》表、按实测改正数字

Type: implement
Status: done

> 规格：`.scratch/docs-tidy/spec.md`（Problem Statement 1–2、Solution 1/3/4、User Stories 1/2/4）。

## 目标

README 是文档的唯一入口，所以它自己不能漏项、不能有会过期的数。

## 落点

`README.md`。`AGENTS.md` 的一句指向留给票 03。

## 具体行为

1. **《文档》表补全**（现在只有 CONTEXT / spec / `docs/` 的一份列表 / ADR / `.scratch/` / AGENTS.md）：
   - `docs/` 那份列表里补上 **`highlight`**（现状：`docs/` 下的中文例外，讲一个有意留下的无消费者模块）；
   - 新增一行 **`docs/research/`**：四份一手调研笔记（含 `notes/` 里 700KB 的正文），说明它是**材料**而不是结论；
   - 新增一行 **`docs/agents/`**：agent 在本仓库工作的三份约定（issue tracker / triage labels / domain docs），并说明它由 `AGENTS.md` 指向。
2. **`.scratch/` 那一行改成指向新增的 `.scratch/README.md`**（票 02 产出），不再点名两个目录了事。
3. **陈旧数字按实测改**，并在数字**旁边**写出复核方式（一句话，不展开命令）：
   - 「29 张实现票」→ **32 张**，且**全部 `done`**（原文说 30/31「尚未做」，实测已完成；32 张都在）；
   - 「597 测试」与「550+ 测试」→ **736**（`cargo test` 的 passed 合计），**两处只留一处**：状态行里给数字，`开发` 一节的命令列表里不再重复数字（那一行本来就是命令）；
   - 「约 27k 行 `src/`、23k 行 `tests/`」→ **31,196 / 28,632**（`wc -l`）。
4. **文档约定**：在《文档》这一节末尾加一小段「约定」，内容照 spec 的《文档约定》一节（谁写中文、谁写英文、例外是谁、新文档跟邻居走）。
5. **`架构` 一节那句「>= 120 列时左栏 40 列画标记」以外的描述照旧**；本轮不碰别的内容。

## 测试

- 表里出现的每个 `docs/...` 路径都要能在磁盘上找到（手点一遍）。
- `docs/` 下每个 `.md` 都必须落在表里，或在表里被某一项覆盖（`docs/research/notes/*` 由 `docs/research/` 那一项覆盖；`docs/tui-manual-checklist.md` 已在原列表里）。
- 数字复核一遍：`cargo test`、`wc -l`、`ls .scratch/*/issues/*.md | wc -l`。
- markdown 链接扫一遍（相对路径都能落地）。

## 不做什么

改 README 的其它章节；给文档加自动校验；翻译任何东西。

## Comments

**实现完成（2026-09-26）**。落点：`README.md`。

1. **《文档》表补全**：`docs/` 那一串里加了 `highlight`；新增两行 —— `docs/research/`（写清是**材料不是结论**：`coding-agent-features.md` 是横向对比、`notes/` 五份上游正文、合计 796KB）与 `AGENTS.md`（指向 `docs/agents/` 三份细目：issue tracker / triage labels / domain docs）。
2. **`.scratch/` 那一行**改成指向新建的 `.scratch/README.md`（票 02 产出）。
3. **数字按实测改**：`32 张` 实现票（含收尾票 30/31/32，全部 `done`）、`src/ 31,196` 行 / `tests/ 28,632` 行（`wc -l`）、`736` 条测试（`cargo test` passed 合计），并把**复核方式**写在同一句里；`开发` 一节那行 `550+ 测试` 换成「全量测试（条数见上面的「状态」）」—— 一条数字只留一处，免得再分叉。
4. **新增「约定」小节**（README 的《文档》一节末尾）：谁写中文、谁写英文、`docs/highlight.md` 为什么是例外，以及两条推论（新文档跟邻居走、跨语言引用保留标识符英文）。
5. **核对**：表里每个 `docs/...` 都落地；`docs/` 下每个 `.md` 都落在表里（`notes/*` 由 `docs/research/` 那一项覆盖）；数字三条命令各跑一遍。
