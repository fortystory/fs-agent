# 外壳骨架：外框、分隔线、两档左栏、主列重排

Type: implement
Status: ready-for-agent
Blocked by:

> 规格：`.scratch/tui-sidebar/spec.md` §1 / §2 / §3 / §6 / §8。帧（真实渲染）在 `.scratch/tui-sidebar/prototype/frames/`。

## 目标

把 TUI 从「三块纵切 + 右栏」换成**一圈外框 + 一条全高左栏 + 一条主列**，并且**这一张票结束时界面必须是完整可用的**：转录、输入区、提示行都在主列里正常工作，左栏已经有 mark、tab 条与「调用量」页的真实字段，状态行已经在画。

## 落点

- `src/render/layout.rs`：整段重写 `plan()` 与 `Regions`。
- `src/render/tui.rs`：`draw_frame` / `draw_header` / `draw_mark` / `header_lines` / `edges` / `draw_seam` / `draw_transcript` / `draw_bottom`。
- `src/render/panel.rs`：**去掉 `模型` 行**（它搬到状态行），其余字段与累加口径一字不改。
- `src/render/wording.rs`：见 §6。
- `tests/render_layout.rs`、`tests/render_tui.rs`。

## 具体行为

1. **几何（spec §1）**：`Regions` 新字段 `sidebar` / `tabs` / `divide` / `rail` / `SidebarKind` / `status`；删 `header` / `header_content` / `middle` / `panel` / `seam()` / `header_kind()` / `HeaderKind`。外框仍用 `Block` 画；分隔线**后画**、按行写字形（外框顶 `┬`、外框底 `┴`、左栏自己的横线 `┤`、主列的横线 `├`、其余 `│`）。**固定 chrome = 7 行**，`转录行 = h − 7 − 输入行数`。
2. **阶梯（spec §2）**：左栏两档 + 隐藏（`w ≥ 120` → 40 内容宽画 mark 居中；`80 ≤ w < 120` → 28 画文字身份行；`w < 80` → 隐藏）。左栏内部按高度先丢 mark → 身份行 → 字段尾部（缓存 → 输出 → 输入），底线 tab 条(3) + 上下文/token/回合(3)。**高度不决定左栏去留。**
3. **左栏（spec §3）**：mark 5 行（颜色渐变照旧）或 `wording::identity()`；tab 条 3 行（上下横线 + 标签行，标签 `调用量` / `轨迹` / `文件`，选中亮品红加粗、未选中暗灰，余下用 `─` 填到右端）；下面画「调用量」页字段（标签列 6 列左对齐、数值右对齐）。**这一票只画 tab 条与「调用量」页**，切换与占位页留给下一张票（标签先按「调用量」选中画）。
4. **状态行（spec §5）**：`模型 <model> │ 模式 <mode> │ 上下文 <n>%`，段宽 = 文本宽 + 左右各 1 格；阶梯三档（三段 → 丢模型 → 只剩上下文）。`上下文 <n>%` 用新措辞 `wording::context_share`（分子 = 最近一条 `UsageRecorded.input_tokens`，分母 = 注入的 `context_window`，无用量时 `—`）。**三段都不可点。**
5. **主列重排**：转录（右缘恒留 1 列滚动条）→ 状态行 → 输入区 → 提示行，全部改到主列坐标。**提示行宽度取主列内容宽**（不是终端宽）。回合条那一列这一票**先留空**（`regions.rail` 算好，画什么都不画），下一张票填。
6. **覆盖层与菜单（spec §1）**：问题/详情覆盖层改为**居中于主列**；`menu()` / `menu_room()` 的横向边界从 `header` 改到主列。
7. **删除（spec §8）**：`HeaderKind` 与 header 一族常量、`draw_header` / `header_lines` / `edges` / `draw_seam`（改写成 `draw_divide`）、`TuiState.clock`、`SessionFacts.cwd`、`wording::clock` / `clock_short`。**`TICK` / `interval` 臂这一票先留着**（它现在只剩置脏的壳，删它单独在 `05` 确认，别把两件事混在一个 diff 里）。

## 测试

- `tests/render_layout.rs`：尺寸矩阵 `40×10` / `60×24` / `80×14` / `80×24` / `100×24` / `120×24` / `174×50` 断言左栏档位、状态行档位、转录行数、`divide` 列、`rail` 列；高度档（`h = 10` 丢身份行与缓存、`h ≥ 12` 有身份行、`h ≥ 16` 有 mark）；状态行三档；提示行 120 列空闲态 = 4 条 + 退出（钉住）。
- `tests/render_tui.rs`：首帧画得出五区域；`模型` 不再出现在左栏；转录/输入/提示都在主列里；覆盖层居中于主列。
- 删掉 v1 的 header 三档、cwd、时钟、`seam` 用例。
- `scripts/tui-startup-check.py` 只改**注释**（「header」→「左栏的 mark」），锚点不动。

## 不做什么

tab 切换与占位页（`03`）、回合条（`04`）、`TICK` 删除与文档收尾（`05`）；键位表零改动；plain / headless 零改动。

## Comments

（实现时把偏离 spec 的地方记在这里）
