# 收尾：文档、词条与护栏

Type: implement
Status: done
Blocked by: 01, 03, 05, 07, 09

> 规格：`.scratch/goal-loop/spec.md` §14。
> 前面每一票都带自己的文档与测试；这一票做**跨面的那一层**：逐面文档、词汇表、索引与护栏。**最后做。**

## 目标

让仓库的文档面与这个 feature 对齐，并让「目标」这一族词在 `CONTEXT.md` 里立住。

## 落点

- `docs/goals.md`（新增）或既有逐面文档的一节
- `README.md` 的「文档」表
- `CONTEXT.md`
- `.scratch/README.md`、`.scratch/goal-loop/` 两份文档
- `docs/tui-manual-checklist.md`

## 具体行为

1. **逐面文档**：新增 `docs/goals.md`，讲**现在怎么工作**：清单文件的形状与位置、`/goal new`、`/loop` 的生命周期（启动边界、提醒、翻页、预算、停止、恢复）、`/clear`、`goal_note`。与其他逐面文档同一个口径 —— 讲现状，不讲当初为什么。
2. **`README.md` 的「文档」表**登记这一篇；若 README 的「这一版不做」里有一条被这个 feature 兑现了（例如 compaction 的实现），把它挪出去并在正文里说明。
3. **`CONTEXT.md` 立词条**（照邻居的格式，中文名 + 英文标识符）：
   - **目标**（`Goal`）：跨会话的工作单位；具名清单文件 + 从流派生的进度。`_Avoid_` 里注一句别和**落点**那条的「路由目标」混。
   - **翻页**（rollover）：结束当前会话、开一个新的那条内部动作；`/clear` 与阈值触发是它的两个入口。
   - **`goal_note`**：记「执行中冒出来的新工作」的工具，args 即真相。
   - **更新既有词条**：**待办列表**（加可选 `id`、跨会话派生）、**`/clear`** 若词表里有它则改成「结束当前会话、开一个新的」。
4. **`.scratch/README.md`**：`goal-loop` 那一行改成实际的票数与完成度；三条老种子的注记保持不变。
5. **`.scratch/goal-loop/`**：`seed.md` 的抬头指向 spec 与票；spec 的 `Further Notes` 若与实现有出入就回改（本仓库的规矩：**实现票若改了 spec 的任何决定，必须回改 spec**，别只在票的评论里交接）。
6. **手工清单**：`docs/tui-manual-checklist.md` 加一节（输入区禁用、`Esc` 确认框、翻页那一刻的观感）。
7. **护栏**：`python3 scripts/check-language.py` OK；`cargo clippy --all-targets` 干净；新增散文与模型可见文本走中文（[ADR 0005](../../docs/adr/0005-model-visible-text-in-chinese.md)）。
8. **ADR**：如果实现落地时形状已经不再动，补上 spec `Further Notes` 里点名的那一条 ——「目标不进会话状态，它的进度从各会话的 `todo` 派生」。

## 测试

- `python3 scripts/check-language.py` OK；
- 文档表里的每个链接都存在（`docs/goals.md`、各 ADR）；
- `CONTEXT.md` 的新词条有中文名 + 英文标识符，且没有实现细节（它是 glossary，不是 spec）；
- 手工清单那一节在真终端里过一遍（这是本票唯一的「必须真机验」项）。

## 不做什么

- 不写实现代码（这一票只动文档）。
- 不改任何票的历史记录。
- 不重述 spec 的决定 —— 逐面文档讲现状，理由留在 spec 与 ADR。
