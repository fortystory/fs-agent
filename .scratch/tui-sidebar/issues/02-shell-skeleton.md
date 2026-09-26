# 外壳骨架：外框、分隔线、两档左栏、主列重排

Type: implement
Status: done
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

**实现完成（2026-09-26）**。落点：`src/render/layout.rs`（整段重写 `plan()` 与 `Regions`）、`src/render/tui.rs`（`draw_shell` / `draw_divide` / `draw_sidebar` / `draw_status` / `draw_transcript` / `draw_bottom`，删掉 header 一族）、`src/render/wording.rs`（新增 tab 标签 / 占位符 / `context_share` / `status_row` / 回合条字形，删 `clock` / `clock_short`）、`src/render/panel.rs`（去掉 `模型` 行）、`src/cli.rs`（facts 改名）、`tests/render_layout.rs` / `tests/wording.rs` / `tests/history_replay.rs` / `tests/ask_user_question_tui.rs`。

**与票面/规格不同的地方，逐条记下**：

1. **`SessionFacts.cwd` 改成保留并改名 `session_dir`，而不是删除**（票面 §8 的删除清单要求删）。理由是它**确实被读**：详情覆盖层要用它拼 `outputs/<tool_call_id>.txt` 读回工具全文（`read_tool_body`），删掉会让「全文」这条路径失效。所以这里做的是把「cwd 不再显示」这条决议写进类型本身（字段名就是它真正装的东西），并在 rustdoc 里写明工作目录已随旧顶栏离开界面。`cargo clippy` 双向确认没有留下没人读的字段。
2. **`Regions` 比票面多两个字段**：`main`（主列的整块，浮层与菜单的居中/边界都从它算）与 `sidebar_page`（左栏页面的矩形，它的**高度就是**高度阶梯留下的字段数）。票面列的是「新字段：sidebar / tabs / divide / rail / SidebarKind / status / …」，这两个是把「画什么」真的画得出来的最小补充，没有第二个来源。
3. **一条票面要求的测试换了个落点**：票面把「首帧画得出五区域」写在 `tests/render_tui.rs`，但那个文件是**状态机**测试（不起帧），全部几何断言在 `tests/render_layout.rs`（它才是 `draw_frame` 的接缝）。五区域、状态行三档、提示行宽度账都在后者。
4. **两条宽度退化路径改在 `Panel::lines` 上直接测**：左栏两个档（28 / 40）的值列分别是 21 / 33 列，v1 的「先丢 `（6%）`、再丢缓存行」两条**从外壳上够不到**（28 档丢百分比仍会在值长到 22 列时触发，缓存行则再也不触发）。这两条按 spec §3「保留为代码路径」留下，测试改成用 `Rect` 直接打 `Panel::lines`，并在测试里写明为什么不能从帧上打。
5. **一处措辞层的台阶数被实测钉住**：`status_row()` 的第三档（只剩 `上下文 n%`）返回**非空**，第 4 档「整行消失」按 spec §2 不实现 —— 触不到它需要主列内容宽 < 11，而地板是 40×10。宽度不够时由画家的 `truncate_columns` 兜底。

**实测数字（120×24 参考档）**：左栏 40（mark 居中，各 1 格 air）、主列 77、转录 16 行 × 75 文本列（滚动条 1 + 回合条 1 恒留）、状态行 `模型 claude-sonnet-4-5 │ 模式 询问 │ 上下文 —`、提示行 4 条 + 退出（放不下 `就绪`）。高度阶梯实测：`h = 16` 才有 mark、`h = 12` 有身份行、`h = 10` 身份行与「缓存」已让位 —— 与 spec §2 的档位表逐条对上。

**基线**：`cargo test` **708 passed / 0 failed**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移（未顺手格式化）。
