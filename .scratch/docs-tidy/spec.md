# 文档整理：索引、陈旧数字、孤儿文档

Status: done（一次 grilling 的折叠；实现票 `01`–`03` 全部完成）

- **来源**：用户的一句「整理下各个文档」+ 一轮摸底（spec 里的 discovery 一节就是那一轮的结果）+ 两个拍定的范围问题（做哪一层、语言怎么办）。
- **范围**（用户拍定）：**索引 + 陈旧数字 + 孤儿文档**。**不动路径**（`docs/` 不重排）、**不改语言**（英文的设计文档不翻译，中文的清单/词汇表也不翻译），只把**没有写下来的约定写下来**。
- **落点**：`README.md`、`.scratch/README.md`（新增）、`AGENTS.md`、`docs/highlight.md`（只在必要时加一行状态）。

## Problem Statement

摸底一轮查出来的，都是**可验证**的出入，不是风格意见：

1. **README 的《文档》表漏了东西**：`docs/highlight.md`（一个「有意留下的无消费者模块」的记录）、`docs/research/`（四份一手调研笔记，含 700KB 的笔记正文）、`docs/agents/`（三份 agent 约定）都不在表里 —— 有文档没有任何入口指向。
2. **README 里的数字过期，而且自相矛盾**：第 25 行写「29 张实现票全部 `done`……597 测试」，第 237 行写「550+ 测试」。实测：v1 票 **32 张、全部 done**（含原文说「尚未做」的 30/31），测试 **736 条**，`src/` **31,196** 行、`tests/` **28,632** 行（README 写的是「约 27k / 23k」）。
3. **`.scratch/` 那一行严重低估**：README 只说「`multi-agent-architecture/` 是决策地图，`fs-agent-v1/issues/` 是实现票」，实际有 **9 个**被 git 跟踪的 feature 目录（还有 1 个本地空目录，见下），形态各不相同（spec / map / seed / 只有票）。
4. **语言约定没写下来**：`docs/` 下大多是英文设计文档，`highlight.md` 与 `tui-manual-checklist.md` 是中文，README / `CONTEXT.md` / spec / ADR 是中文 —— 看上去是有意的分工，但没有一处写明，新来的人（或 agent）只能猜。
5. **一条「死链」是假警报**：`docs/research/notes/cline-continue.md` 里那个 `/sdk/plugins` 是**研究笔记引用的上游路径**，不是本仓库的链接 —— 不动，但写进 spec 免得下一个人再查一遍。
6. **一处本地残留**：`.scratch/call-rationale/issues/` 是**空目录**、不在 git 里（`git ls-files` 没有它）—— 本轮**不擅自删**，记在这里，用户要删一句话的事。

## Solution

1. **README 的《文档》表补全**，并把 `.scratch/` 的那一行改成指向一份新的 **`.scratch/README.md`**（feature 索引）。
2. **`.scratch/README.md`（新增）**：一张表，一行一个 feature —— 目录名、形态（spec / map / seed / 只有票）、一句话（取 spec 或 map 的第一行）、票数与完成度（`done`/总数）。
3. **陈旧数字按实测改**：`29 → 32`、`597 / 550+ → 736`、`约 27k/23k 行 → 31,196 / 28,632`，并**在数字旁写明它是怎么数出来的**（`cargo test` 的 passed 合计 / `wc -l`），这样下一个改的人知道怎么复核。
4. **把文档约定写进 README 的《文档》一节**（见下），并在 `AGENTS.md` 里加一句指向它，让 agent 也知道该往哪写。
5. **不动**：任何路径、任何文档的语言、`docs/research/` 的正文、`.scratch/*/spec.md` 的历史决算。

## 文档约定（写进 README，也就是本轮唯一新增的规则）

- **写中文的**：`README.md`、`CONTEXT.md`、`.scratch/` 下的 spec / map / 票、`docs/adr/`、`docs/tui-manual-checklist.md` —— 面向**使用与流程**：怎么说、怎么验、为什么这么定。
- **写英文的**：`docs/*.md` 的逐面设计文档（`bash` / `credentials` / `custom-tools` / `discussion` / `executor` / `observability` / `plan-mode` / `render` / `repo-map` / `skills`）与 `docs/agents/*` —— 面向**代码内部**：模块边界、不变量、代码在哪。标识符一律英文。
- **例外**：`docs/highlight.md` 是中文 —— 它记录的是「这个模块为什么留着、什么会让它回来」，读者是将来接手的人而不是改那个模块的人。
- **新增文档跟邻居走**：先看它旁边那份是怎么写的；跨语言的引用保持标识符英文。

## User Stories

1. 作为读者，我想从 README 一处就找到**所有**文档的入口，以便不必靠 `find` 才知道有 `docs/research/`。
2. 作为读者，我想看到**准确**的数字（票数、测试数、行数），并且知道怎么复核，以便不信一个会过期的数。
3. 作为读者，我想从一处看到 `.scratch/` 下每个 feature 是 spec 还是 map、做了几张票，以便知道那是决策记录还是已完成的工作。
4. 作为将来写文档的人（或 agent），我想看到**语言约定**，以便新文档不用猜该写中文还是英文。
5. 作为维护者，我想让这些数字的**出处**写在旁边，以便下次它们过期时改的人知道数什么。

## Testing Decisions

- 核对用命令（写进 spec 与票里，供复核）：
  - 测试数：`cargo test 2>&1 | grep -E '^test result' | awk -F'[ ;]' '{p+=$4} END {print p}'`
  - 行数：`find src -name '*.rs' | xargs wc -l | tail -1`（`tests/` 同理）
  - 票数：`ls .scratch/*/issues/*.md | wc -l`；完成度：`grep -h '^Status:' .scratch/*/issues/*.md | sort | uniq -c`
  - 索引漏项：README 的表里出现的每个 `docs/...` 都要能落到文件；`docs/` 下每个 `.md` 都要在表里或明确属于某个表项（`docs/research/`、`docs/agents/`）
- 链接：本轮的改动只动 markdown 链接目标，改完全仓 `python3` 扫一遍相对链接（那一小段脚本写进票 03 的 Comments，供下次复用）。
- `cargo test` / `cargo clippy` / `cargo fmt` 的基线不变（本轮不碰 Rust）。

## Out of Scope

- **重排目录**（`docs/tui/`、`docs/tools/` 之类）与规范化文件名。
- **翻译任何文档**（英文不译中、中文不译英）。
- **改 `.scratch/*/spec.md` 的内容**（它们是当时的决算记录，包括已经推翻的那些）。
- **删 `.scratch/call-rationale/`**（本地空目录；要删由用户说）。
- **给文档加生成器 / 检查器**（比如让 CI 校验索引完整）；本轮是手工整理一次。
