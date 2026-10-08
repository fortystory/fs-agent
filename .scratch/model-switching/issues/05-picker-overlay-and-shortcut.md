# 05 — 选择器浮层与三个入口

Type: implement
Status: ready-for-walkthrough
Part of: ../spec.md
Blocked by: 04

画选择器（`ConsoleRequest::Picker` 的前端那一半），并把 `ctrl-t` 与 `/model` `/effort` 的
键盘路径接上。

规格见 [`spec.md` §9、§10、§12](../spec.md)。

## 要做什么

### `TuiState` 上一个独立的 `picker` 状态

`picker: Option<Picker>`，`Picker { request: PickerRequest, highlight: usize, rows: Vec<…> }`。

**不进 `questionnaire()` 那条路径** —— 问卷是模型发起的、答完就是那条工具调用的结果；选择器是
人发起的会话属性，答完要回循环。理由写进代码注释（spec §10）。

键盘归属按「谁立着谁拿」判（与 `detail`、`sidebar_keyboard` 同一套分派，见 `TuiState::key`）：

| 键 | 选择器立着时 |
| --- | --- |
| `j` / `k` / `↓` / `↑` | 移动高亮 |
| `Enter` | 选中（`enabled: false` 的那几行不响应） |
| `Esc` | 取消 → 回 `None` |
| 其它可打印字符 | 不进文本（这一格被它占着） |

**指针不被它独占**：框外点击关掉它（与详情覆盖层同一条），框内点击一行等于选中那行，其余点击
照旧分派。立着的时候输入区禁言（与问卷同一条纪律）。

### 绘制

一块浮层，居中，宽度取最长一行加余量、高度按候选数（有上限，候选多时内部滚动 —— 与
`DetailView` 的 `height` 同一套）。标题行（`模型` / `思考强度`）、当前项一个记号、禁用项用
`palette::MUTED`、高亮用 `ACCENT` + `BOLD`（照**焦点行**那条：`src/render/files-page` 与
`FilesPage` 的画法）。页脚给键位提示：`j/k 移动 · 回车 选择 · esc 取消`，走
`wording` 层。

**不**用 `.scratch/tui-visual-language` 的问卷符号集 —— 那是问卷的四符。选择器要的是
「当前项」与「禁用项」两枚，加进 `wording.rs` 的符号表。

### `ctrl-t`

`map_key`（`tui.rs:150`）加一格 `'t' => Some(Key::CtrlT)`，`Key` 枚举加 `CtrlT`。它不归任何
视图管，所以在 `CtrlZ` 那一档旁边 —— 但**在选择器立着时归选择器**（先判 picker）。忙闲由循环
判（票 03），前端只管上行。

理由写在代码注释里：`Ctrl-M` **不能**用，它在终端里是回车（`\r`），crossterm 报成
`KeyCode::Enter`，与提交正面撞车。`tui.rs:134` 的 `Newline` 注释已经记过同一件事。

### 收到 `Picker` 时

`TuiState::request` 加一格：存进 `self.picker`，`highlight` 落在 `current: true` 的那一行（没有
就落第一行），请一帧。回答案在关闭的那一刻做（`reply.send(Some(index))` 或 `None`）——
**只做一次**，所以 `Picker` 存的那个 `reply` 取出即 `take()`（与 `prompt_reply` 同一写法）。

## 测试

`tests/render_tui.rs`（造一个假 `oneshot::Sender` 或一个 `PickerRequest` 的构造器）：

- 收到 `Picker` → 高亮落在 `current` 那行。
- `j`/`k` 在两端停住（不绕回）。
- `Enter` 在禁用行上不响应；在可用行上回 `Some(index)` 且**弹窗关闭**。
- `Esc` → 回 `None`，且键盘还回输入区（下一下按键能被输入区吃到）。
- `Esc` 之后再按一次 `Esc` 是举手退出那两下的一部分 —— 别把它吃掉（举手槽位是渲染器状态，
  选择器关掉之后要照旧能用）。
- 框外点击 → 回 `None`；框内点一行 → 回那行。
- 候选超过浮层高度时滚动，`Enter` 回的是**高亮那一行**而不是第几行。
- `ctrl-t` 上行 `OpenPicker(Model)`（默认开模型选择器）；选择器立着时 `ctrl-t` 不再上行
  （它归选择器或者什么都不做）。
- 选择器立着时输入区不落字。

## 评论

2026-10-08 实现完毕，`Status: ready-for-walkthrough`（自动化能做的都做了：浮层的画法、键盘、
命中与滚动都有测试；剩下的是**手感** —— 两格的点按容差与浮层在真终端上的观感，进
`docs/tui-manual-checklist.md` 的 ㊱）。

- `Picker` 是 `TuiState` 上的独立状态（不进 `questionnaire()`），键盘在它立着时归它、指针不被
  它独占，与票一致。
- 两枚符号（`PICKER_CURRENT` `▸` / `PICKER_DISABLED` `×`）进 `wording.rs` 的符号表。
- `Ctrl-T`：`Key::CtrlT` + `map_key` 的 `'t'`，排在 `Ctrl-O` 那一档（不归任何视图管）；文件查看器
  立着时它进 nvim（`0x14`，transpose）—— 那一档独占键盘，而 `key_bytes` 对它有穷尽 `match`。
- 浮层宽度取最长一行 + 4、封顶主列减 4；高度按候选数、上限是主列装得下的那些，多了就在内部滚动
  （`top` 跟着高亮走），于是回车回的永远是高亮那一行。

**一处与票不同的细节**：票说「框外点击关掉它」，而举手退出那两下（`Ctrl-C`）照旧要能用 ——
所以选择器关掉之后不留下任何会吃掉下一个手势的状态（`picker` 取走即答，只发一次）。

### 代码审查之后的修正（2026-10-08）

`/code-review` 的两份报告落下来五处：

1. **记号写反了（真 bug）**：`picker_row` 原来按 `enabled` 选记号，于是**每个可选项**前面都有
   `▸`。改成两枚各说一件事：`current` → `▸`、`!enabled` → `×`、其余留空格（列仍然对齐）。
2. **「固定」点它给一句说明**（spec §8 + 票 06 走查第 5 条）原来没实现：`ask_picker` 在
   `PickerKind::Effort` 且 `fixed_effort()` 时给一句，而不是开一块空浮层。那句**必须短** ——
   提示行要把键位提示与出口也留在同一行，放不下的回执读起来就是「点了没反应」（第一版文案就是
   这么丢掉的，现在 `wording::effort_fixed_detail` 只有一行）。
3. `wording::picker_current_detail()` 零调用 —— 删掉（记号已经由 `PICKER_CURRENT` 说了）。
4. `open_picker` 注释说「清单重算」，实现却用开清单前抓的 `current_model` / `current_effort`：
   改成等完答案之后**现读 `harness`** 再算一遍，于是等待期间换过模型也不会切到错档位。
5. 补上票里点名而原来没有的三条测试：框内点一行等于选中那行、候选超过浮层高度时内部滚动且
   `Enter` 回高亮那一行、关掉之后举手退出那两下照旧。
