# grilling：测试与验证策略的迁移

Type: grilling
Status: resolved
Blocked by: 02, 04

## Question

新布局会让一批现有测试和脚本失效或变味。本票定下**验证策略**：什么用 `TestBackend` 断言、什么必须上 pty、什么必须人眼在真终端里看。

## 需要定

1. **`tests/render_tui.rs`（26 个，含编辑器与光标用例）**。逐个归类：保留 / 改写 / 删除；多行编辑器要新增哪些用例（软换行、宽字符、跨行退格、跨软换行移动、粘贴多行不提交、Ctrl-J 与 Shift+Enter、历史与光标上下的冲突）。
2. **布局本身怎么测**。`TestBackend` 固定尺寸渲染 + 断言 buffer 文本（而不是 pty 截图）够不够？要覆盖哪些尺寸 —— **至少覆盖 40×10（最小档）与一档正常尺寸**；断言什么（关键锚点字符串？区域边界？）。给出用例清单。
3. **`scripts/tui-startup-check.py`**。它现在用 pty + 一个宽字符感知的 VT emulator，锚点是 `ctrl-c` / `fs-agent：` / `STATUS_TAIL = 退出`。新布局里这些串都会变 —— 新锚点是什么（新 header 里的稳定串？footer 的快捷键提示串？）。这个脚本还测什么（启动不崩、首帧正确、退出干净），迁移后它还能测出什么。
4. **端到端**。`tests/e2e_single_turn.rs`、`tests/replay.rs`、`tests/wording.rs`、`tests/render_markdown.rs` 是否受影响（预期：前两个不碰 TUI，后两个只跑纯函数 —— **确认**，并指出任何真的需要改的地方）。
5. **手工验证清单**。真终端里必须人眼确认、自动化测不到的项：光标位置（票 04 第 6 条给的可判定检查）、终端原生选择与复制、resize、多行粘贴、Ctrl-J 与 Shift+Enter 的实际到达、退出后终端是否干净（无残留 raw mode）。写成一份可照做的清单。
6. **不许变红的基线**：`cargo test` 489 passed / 0 failed、`cargo clippy --all-targets` 干净、`cargo fmt --check` 除 `src/context/repo_map.rs` 与 `tests/repo_map.rs` 的既有漂移外干净。说明新用例加进去之后这个数字怎么变。

## 先读

- `tests/render_tui.rs`、`tests/render_delivery.rs`、`scripts/tui-startup-check.py`
- `.scratch/tui-layout/issues/02-*.md`、`04-*.md` 的答案

答案必须自足（`/implement` 在 `/clear` 后读它）。

## Answer

**已定（2026-09-21，grilling）。本票只产决策，不含实现。**

### 0. 不许变红的基线

`cargo test` 489 passed / 0 failed → **489 + 新增用例**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 除 `src/context/repo_map.rs` 与 `tests/repo_map.rs` 的既有漂移外干净（**不要**顺手格式化那两个文件）。

### 1. `tests/render_tui.rs` 逐个归类

**保留不动**（测 `render_block`，块渲染没变）：

- `a_tool_call_and_its_hook_become_one_ready_block`
- `a_completed_turn_and_an_aborted_one_render_in_different_colors`
- `a_diff_line_gets_a_background_from_the_diff_layer`
- `the_synthesizers_product_renders_with_the_system_speaker`
- `the_answer_block_is_rendered_as_markdown`
- `intermediate_narration_is_dim_and_the_answer_is_not`
- `a_message_continuation_indents_by_the_label_display_width`
- `a_wide_grapheme_leaves_no_blank_cell_after_it`
- `the_live_tail_wraps_on_display_columns_not_bytes`

**保留但改名**：

- `a_notice_is_a_scrollback_line_shown_as_it_is` —— "scrollback" 这个词随 inline 视口一起没了（转录现在是一条面板里的行）；行为断言不变，改个名。

**保留不动（键位语义没变，只是渲染换成模态）**：

- `a_permission_question_is_answered_by_key`
- `escape_answers_a_question_with_the_non_acting_choice`
- `escape_while_working_is_a_cancel_gesture`
- `an_idle_ctrl_c_quits_and_a_working_one_cancels`
- `shift_tab_is_the_plan_gesture`

**改写**（编辑器搬进 `Input`，断言对象换成 `Input`）：

- `a_typed_line_is_submitted_to_the_loop`
- `backspace_edits_the_line`
- `an_empty_submission_closes_the_prompt`
- `the_cursor_column_counts_a_wide_character_as_two`

**辅助函数**：`kimi()` 不动；`state_with_prompt()` 必须跟着新 `TuiState`（多一个 `Input` 字段、多一个 `SessionFacts`）改签名。

### 2. 新增：布局用例（`TestBackend`，无 pty）

票 02 已证 `TestBackend` 在默认特性下可用（`ratatui-0.30.2/src/lib.rs:505`、`ratatui-core-0.1.2/src/backend.rs:112-113`），所以**布局可以进 `cargo test`**。尺寸矩阵照票 02：`40×10`、`40×12`、`60×24`、`80×16`、`80×24`、`120×24`、`174×50`、`39×24`。

1. **四区都在**：每档断言 header / 转录 / 右栏 / 底部块的关键锚点出现在预期行（几何数字取自票 02 的 `geometry-table.md`）。
2. **降级阶梯**：`w < 80` 右栏消失；`w < 60` header 压成 1 行；`w < 40 或 h < 10` → 只显示「终端太小」；**`40×10` 不是「太小」**（票 02 的结论，防回归）。
3. **airy 在 `h ≤ 11` 被丢掉**；`40×12` / `60×13` 起才恒在。
4. **提示行条目数**：`40 → 3`、`60 → 4`、`80 → 5`、`120 → 6`，且 **`ctrl-c 退出` 在任何宽度都在**。
5. **提示集里绝不出现 `shift+enter`**（票 04 / 06；防回归）。
6. **右栏**：无 usage 时显示 `—`；无上限时**只显示已用、不显示 `/ —`**；`120×24` 草稿涨到 10 行时**右栏整栏消失**（票 02 那条最反直觉的实测）。

### 3. 新增：编辑器用例（对着 `Input` 断言，不需要终端）

软换行的行列映射（含宽字符）、`↑/↓` 的 goal column、`Home/End/Ctrl-A/Ctrl-E/Ctrl-U/Ctrl-K/Ctrl-W` **全部按当前逻辑行**、跨行 `Backspace`/`Delete`、`Ctrl-J` 插入 `\n`、`Enter` 提交、**`↑/↓` 不动历史而 `Ctrl-P/N` 动历史**、粘贴归一 `\r\n` 且**不提交**、超 100 000 字符走确认、**多行 `Esc` 先确认而单行直接清空**。

**斜杠命令只看第一行**这条在 `cli` 层（`src/cli.rs:383`），用例放在解析那一侧，不放这里。

### 4. 新增：滚动、吸底、鼠标（`TestBackend` + 直接喂事件）

- `PgUp`/`PgDn` 步长 = 整页减 2 行重叠；`Ctrl-G` 一次到底。
- 上滚立即脱离吸底；**脱离后新内容不改变视口**；**提交无条件回到底部**。
- 20 000 **源行**上限丢最旧（超限后最旧的内容不在转录里）。
- resize 后锚点保持（吸底态仍吸底；脱离态保持顶部那条源行）。
- **滚轮每格 3 行**；点击「点此到底」的命中矩形生效；**其它区域点击无效**（直接构造 `CtEvent::Mouse` 喂进渲染循环，不需要 pty）。
- **两种新模态**（超长粘贴 / Esc 清空）的键位与默认答案，以及「模态画出后背景被遮住」。

### 5. 新增：管线用例（票 07）

- 突发合并：一批事件只产生**远少于事件数的帧**（票 07 的 `draws << events`）。
- 无脏不画；时钟**分钟**变化才置脏。
- `sse_stream` 的 `YIELD_EVERY` 与 `render_delivery.rs` 的「不丢弃」断言**保持**（票 07 §7）。

### 6. `scripts/tui-startup-check.py`

**锚点基本能活下来，但判定逻辑必须改**（这是本票最具体的一处）：

- `STATUS_ANCHOR = "ctrl-c"` ✅ 仍有效（新提示行恒含 `ctrl-c 退出`）。
- `STATUS_TAIL = "退出"` ⚠️ **`row.endswith(STATUS_TAIL)` 会失败**：新的底部块带边框，`退出` 后面跟着 `│`。改成「该行**包含** `ctrl-c 退出`，且其后只允许边框/空白字符」。
- `BANNER_ANCHOR = "fs-agent："` ✅ 仍有效（`wording::banner` 文案没改，仍经 `Notice` 进转录）。
- **新增锚点**：header 的版本串（`fs-agent 0.1.0`）用来确认新布局真的起来了；再加一条粗断言「屏幕上出现 ≥3 处边框字符」，防止退化成旧的 inline 布局。
- 脚本继续负责的三件事不变：启动不崩、首帧正确、**退出后终端干净**（无残留 raw mode / 鼠标捕获 / bracketed paste）。

### 7. 其它测试文件

`tests/e2e_single_turn.rs`、`tests/replay.rs` **不受影响**（不碰 TUI，走库入口）—— 确认即可。
`tests/wording.rs` **要加**：新提示集的条目、票 05 的面板中文标签、「点此到底」两种文案。
`tests/render_markdown.rs` **不受影响**（Markdown 渲染器没动）。

### 8. 手工清单（自动化测不到、必须在真终端做）

1. **光标**：票 04 §6 那 7 条。
2. **权限模态**：出现、回答、消失后焦点回到输入区。
3. **鼠标**：滚轮滚转录；点「点此到底」；**按住 Shift 拖拽能选中并复制**（这是开鼠标捕获的代价，必须实测确认可用）。
4. **resize**：拖窗口过程中四区重排不撕裂、锚点保持。
5. **粘贴**：多行不提交；超长弹确认。
6. **`Ctrl-J` 换行**；**`Shift+Enter` 在不支持协议的终端上就是提交**（确认它没被当成换行）。
7. **退出干净**：退出后终端恢复原生选择、`stty` 无 raw mode 残留、alt screen 不留屏。
8. **取消**：忙碌时 `Ctrl-C` 取消、空闲时 `Ctrl-C` 退出。
9. `120×24` 把输入写到 10 行 → **右栏消失**（预期行为，不是 bug）。

### 9. 交接出去的事

- **票 09（ADR）**：本票的验证面（`TestBackend` 进 `cargo test`、pty 脚本改判定）可作为「代价可控」的证据引用。
