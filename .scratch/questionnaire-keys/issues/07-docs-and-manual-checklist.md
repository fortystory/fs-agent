# 07 — 逐面文档与问卷手工清单

Type: implement
Status: ready-for-agent
Blocked by: 06

> 来源：[`../spec.md`](../spec.md) §9。票 33、34 两个真机 bug 都落在「真终端手感」这一类，
> 而这份清单里至今没有问卷那一节——这一票把它补上，并把这次改掉的键位写进逐面文档。

## 目标

- [`docs/render.md`](../../../docs/render.md) 补**问卷的键位 / 区域**一小节：区域是什么、
  两区里各键做什么、两把手势怎么分工。今天它只在 [:160](../../../docs/render.md)、
  [:179](../../../docs/render.md) 两处顺带提到问卷。
- [`docs/tui-manual-checklist.md`](../../../docs/tui-manual-checklist.md) 补**问卷一节**：
  1. 分页页脚在窄终端里的三档降级（先丢 Emacs 别名 → 再砍键位提示 → 保住进度、按钮、回执）；
  2. 超长选项折行、以及高亮项跨多行时仍完整可见；
  3. 自由文本行的光标位置（打到行尾、超过列宽时）；
  4. 区域高亮：同一时刻只有一处是亮的；
  5. `Esc` 举手回执，以及 `Ctrl-C` 举手回执（今天后者看不见，这次补上）；
  6. 「输入区里 `j`/`k` 是文本、选项区里是移动」这条分界，以及选项区吞掉可打印字符。
- [`.scratch/README.md`](../../README.md) 索引里 `questionnaire-keys` 那行的进度改成实际结果
  （做完的票数）。

## 现状（2026-10-02 核实，改前先复核）

- `docs/tui-manual-checklist.md` 有 603 行、小节 ①–㉔，问卷只作子项出现
  （[:61](../../../docs/tui-manual-checklist.md)、[:234](../../../docs/tui-manual-checklist.md)、
  [:237](../../../docs/tui-manual-checklist.md)、[:476-477](../../../docs/tui-manual-checklist.md)、
  [:558](../../../docs/tui-manual-checklist.md)），**没有问卷专节**。
- 索引在 [`README.md:240`](../../../README.md)。

## 收尾

- 能自动化的部分做完之后，这张票只剩「在真终端里逐项走一遍」——那时把它标成
  `ready-for-walkthrough`（与 `exit-gesture` 票 05 同规矩）。
- `python3 scripts/check-language.py` 通过。

## 不做什么

- 不重写清单里已有的 ①–㉔ 各节。
- 不在这一票里改任何代码。
