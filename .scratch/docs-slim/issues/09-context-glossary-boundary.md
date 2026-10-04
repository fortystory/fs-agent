# 09 — `CONTEXT.md` 剥到「一句定义 + 指针」

Type: implement
Status: ready-for-agent
Part of: ../map.md
Blocked by: 08

> 规格：[`../spec.md`](../spec.md) §4。判据来自 [`CONTEXT.md` 的词条边界](07-grilling-context-boundary.md)
> 的 `## Answer`；逐词条的四类片段清点与「别处是否已有一份」的落点表在
> [`../research/02-context-implementation-details.md`](../research/02-context-implementation-details.md)。

## 目标

读术语表的人在一页里就能分清近义词：每个词条先给**一句「它是什么」**，细节想去再点 `docs/` 链接；表里
**不再出现任何 `§N` 与机制名**；那 6 条「别处没有一份」的实现细节在 `docs/` 落地。剥完这个文件的单元违规
从 **8 项降到 0**。

## 现状（2026-10-04 核实，改前先复核）

- **体量与欠账**：`CONTEXT.md` 348 行 / 44KB、15 节 **72 个词条**；纯实现细节 **246 处指称 + 193 条行为描述**，
  格式必需的英文槽位 **78 处**（约 1 : 3.2）；单元口径下 **8 项超长违规（全 scope 最多）**。
- **落点表**：`../research/02` §7.3 是按词条抽查过的 `docs/` 落点；§7.2 是 6 条 coverage `none` / partial 的证据。
- **14 个「会空掉」的词条**（`../research/02` §6 的 A 档）：渲染节占 8 个 —— 提示符色相 / 脉冲 / 下落短横 /
  状态行 / 输入区 / 选项区 / 焦点回合 / 回合条。
- **两条硬输入**：`PromptHue` / `FallingDash` / `StatusRow` 在 `src/` 查无此名（真实形态是
  `PROMPT_HUE_PER_SECOND` / `DASH_FALL` / `wording::status_row()`）；`.scratch/tui-input-pulse/spec.md`:6 证明
  「spec 主动往 `CONTEXT.md` 加词条」是**仍在生效的流程**（本票的 §6 规矩就是治它）。
- **增补 `docs/` 的占比额度很紧**：`docs/observability.md` 余量 2.53 点（纯英文额度约 163 字符）、
  `docs/render.md` 余量 1.87 点（约 543 字符）。**用中文叙述包住标识符**，别贴代码清单。

## 落点

`CONTEXT.md`、`docs/render.md`、`docs/observability.md`、`docs/discussion.md`。**不新增文件。**

## 具体行为

1. **目标形态**：`**中文名（English）**: <这个词是什么，一到两句>。细节见 <`docs/` 链接>`。
   删掉机制名、内部类型与事件变体、调用顺序、参数与阈值（246 处指称 + 193 条描述）。
2. **指针必须落到真实来源**：用 `../research/02` §7.3 的落点表；落不到的先补 `docs/`（见第 3 条）。
3. **6 条独有内容搬进 `docs/`**：问卷请求 / 问题选项（`none`）、作答草稿 / 问卷文案（partial）→
   `docs/render.md` 的问卷一节；价目表 → `docs/observability.md`；落点 → `docs/discussion.md`。
4. **9 处 `§N` 全部换掉**：`spec §5` / `§15`（讨论者）→ `docs/discussion.md`；6 处裸 `§N` 按落点表换；
   **`spec §7` 存疑 → 删**。换完 `grep -n '§' CONTEXT.md` **零命中**。
5. **14 个空壳词条各补一句纯领域定义**（它是什么、读者凭什么认出它），不许只剩标题 + 指针。
6. **硬约束句全留**（「不做 / 永远不 / 一律 / 绝不」），边界按**句子性质**分：留规范句、删描述句；
   一句两半都有的，留规范那半、删描述那半。
7. **第 10 行的规矩改一行**：英文槽位是代码里的标识符 / 类型名；**若这个概念只在 spec 与文档里立了名、
   代码里没有对应实体，槽位就写那个已被文档采用的写法**。四个造名槽位（`PromptHue` / `FallingDash` /
   `StatusRow` / `Pulse`）保留；它们的代码形态**不进正文**，随指针去 `docs/`。
8. **顺带清掉 `docs/render.md` 的两处**（本票已经在动它）：`docs/render.md` L279-288 那块复述
   （`../research/03` §3 判定「删，三处独有信息并回 README」）与它那个 ≥500 的清单项。
9. **拆掉剩下的超长单元**：剥离后仍 ≥500 的按顶层项 / 段落拆开。

## 验证

1. `grep -n '§' CONTEXT.md` 零命中；`grep -c '::' CONTEXT.md` 与改前对比大幅下降（机制名清掉）。
2. `python3 scripts/check-doc-size.py` 退出 0，且 `CONTEXT.md` 的违规数 **8 → 0**（把该文件的基线**显式收紧到 0**，
   在提交信息里写明理由）。
3. `python3 scripts/check-language.py` 全绿 —— 特别注意 `docs/observability.md` 与 `docs/render.md` 的占比余量。
4. 抽查三条落点：讨论者 → `docs/discussion.md`、权限三兄弟 → `docs/permissions.md`、渲染 → `docs/render.md`
   都能读到被剥掉的机制。
5. 14 个空壳词条各有一句定义（逐个点过去，不是抽样）。
6. `git status` 只看得到本票的四个落点。

## 不做什么

- 不动词条的中文名与格式（第 10 行只改英文槽位那半句）、不动 `_Avoid_` 行、不动第 14 行
  「`agent` 是泛称、类型名用 `Debater` / `Executor`」的规矩。
- 不新增文件；不动 `docs/research/` 与 `.scratch/*/spec.md`（冻结项 3）。
- 不在这张票里改入口三份、手工清单或 lifecycle（各有自己的票）。
