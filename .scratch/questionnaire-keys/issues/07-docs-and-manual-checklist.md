# 07 — 逐面文档与问卷手工清单

Type: implement
Status: ready-for-walkthrough
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

## Comments

- **落地（2026-10-02）**：`docs/render.md` 新增「问卷：区域与键位」一节 —— 两个区域里各键做
  什么、答案只有一个形状、两把举手的分工表、以及「回执只能落在页脚」这条。来源段与索引都更新。
- `docs/tui-manual-checklist.md` 新增 **㉕ 问卷：区域、折行与两把举手**（七条：区域分派、
  进/出输入区、折行、页脚降级、两把举手、三种终端各走一遍），文档开头的来源段补了这一轮。
- `.scratch/README.md` 的进度列改成实际结果。
- **中文占比撞了一次下限**：新加的 ㉕ 里键名与终端名密，`check-language.py` 报 45.8% < 46%；
  把重复的 `Ctrl-N`/`Ctrl-P` 换成中文说法、并补两句散文之后回到线上。
- **收尾状态是 `ready-for-walkthrough`**：能自动化的部分（文案、帧、状态机）都有测试钉住，
  只剩第 7 条「三种终端各走一遍」要人做。

- **review 补记（2026-10-02）**：㉕ 补上「自由文本行的光标」（打到行尾、超过列宽时跟着折行）
  与「输入区自己也折行」两条走查 —— 前者是这张票目标里点名要的，第一版漏了。
