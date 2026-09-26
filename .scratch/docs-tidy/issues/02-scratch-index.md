# `.scratch/README.md`：feature 索引

Type: implement
Status: ready-for-agent
Blocked by: 01

> 规格：`.scratch/docs-tidy/spec.md`（Problem Statement 3、Solution 2、User Story 3）。

## 目标

`.scratch/` 下有 9 个被 git 跟踪的 feature 目录，形态各不相同（spec / map / seed / 只有票）。给它们一张表，让人（和 agent）一眼看出**哪个是决策记录、哪个是已完成的工作、做完了几张票**。

## 落点

`.scratch/README.md`（新增）。README 的《文档》表已由票 01 指向它。

## 具体行为

1. 文件开头一段说清楚：这里一个 feature 一个目录、spec 是 `.scratch/<feature>/spec.md`、图是 `map.md`、票在 `issues/NN-*.md`（每票一个 `Status:` 行），并指向 [`docs/agents/issue-tracker.md`](../docs/agents/issue-tracker.md) 作为权威约定。
2. **一张表**，一行一个 feature，四列：
   - **目录**（链接到 spec 或 map）；
   - **形态**：spec / map（wayfinder 决策图）/ seed（种子材料）/ 只有票；
   - **一句话**：取 spec 或 map 的第一行标题（已核过，见下）；
   - **票**：`done/总数`（`Status:` 行统计；没有票的写 `—`）。
3. 数字与标题都要**当场核**（不要照抄本票）：`ls .scratch/*/issues/*.md | wc -l`、`grep -h '^Status:' .scratch/*/issues/*.md | sort | uniq -c`、`head -1`。
4. **如实记两条**：
   - `.scratch/call-rationale/` 是**本地空目录**（不在 git 里、一个文件都没有）—— 表里**不列**，在表下用一句话说明它在磁盘上存在但不在仓库里；
   - `ask-user-question/` 只有 `seed.md` + `research/`，没有 spec —— 形态写 `seed`。
5. **不重写任何 spec 或 map 的内容**；这张表只是索引。

## 测试

- 表里每一行的链接都要能打开。
- 票数与完成度与上面两条命令的输出一致。
- `.scratch/README.md` 自己也放进 README 的《文档》表（票 01 已做，核对一下）。

## 不做什么

给 feature 目录改名 / 合并；补写缺失的 spec；动 `issues/` 里的任何文件。
