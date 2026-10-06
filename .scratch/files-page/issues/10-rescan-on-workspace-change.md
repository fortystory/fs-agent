# 10 — 工作区变了就重扫

Type: implement
Status: ready-for-walkthrough
Part of: ../map.md
Blocked by: 06

> 规格：[`../spec.md`](../spec.md) 的实现决定 §2 与用户故事 E。前置是
> [06 — 文件页画出一棵能点开的树](06-files-page-tree.md)。

**What to build:** 模型每改过一次工作区，这一页就跟着变 —— 写文件、改文件、以及 `bash` 里的
`mv` / `git checkout` 都算；`/undo` 之后也算。连着几次改动只跑一次遍历。

## 验收

- [ ] 任何**非只读**的工具调用完成之后，文件页在一次遍历之内跟上（新建的文件看得见、删掉的消失）
- [ ] `/undo` 之后也跟上
- [ ] 提交之后照旧跟上（既有行为不回退）
- [ ] 只读调用（读文件、搜索、仓库地图一类）**不**触发
- [ ] 执行者与讨论者改的工作区也算（它们落在同一条流上）
- [ ] 连着几次改动只触发一次遍历：一次遍历在飞时新的触发不并发，且**不丢更新**（最多延后一轮）
- [ ] 遍历仍跑在既有的那个阻塞线程上，不新起进程

## 评论

- **落地（2026-10-06）**：`RenderEvent::WorkspaceChanged` 是一条**静默信号** —— 不画一行、不产
  任何块，plain 与 headless 把它当没看见；`RenderHandle::workspace_changed()` 发它，TUI 收到就
  置 `file_scan_wanted`，于是它与「提交之后」落在同一个位、同一次遍历上。
- **触发点**：`finish_call` 里按既有的 `Effect` 判（`touches_workspace`），所以 `bash` 里的
  `mv` / `mkdir` / `git checkout` 也算，而只读调用一次都不惊动；`CallCompletion` 与 `DeferredCall`
  因此各多带一个 `effect` 字段（执行者与讨论者的调用走同一条收尾路径）。`/undo` 不走派发，它在
  `agent::undo_last_edit` 里自己补一次。
- 合并与不丢更新归既有的 `take_file_scan` + `FileIndex::begin()` 守卫：一次遍历在飞时不并发，
  位留着、下一轮补发。
- 断言：`only_a_read_only_call_leaves_the_workspace_alone`（判据那个纯函数）、
  `a_workspace_change_asks_for_a_rescan_and_never_draws_a_line`（前端：静默、合并、补发）。
- **测试覆盖的边界（如实记）**：`Renderer` 只有三种实现，测试注入不了自定义渲染器，所以「一次
  真回合里非只读调用之后前端确实收到信号」没有端到端断言 —— 它由判据单测、前端集成测试与唯一
  那个调用点（`finish_call`）三样合起来兜着。真机走查里值得看一眼：模型改过一个文件之后，
  `文件` 页当场跟上。
