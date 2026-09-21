# 16: 测试与验证迁移：既有用例归类、pty 脚本判定与手工清单

**What to build:** 把验证面收口：`tests/render_tui.rs` 的既有用例逐个归类迁移，`scripts/tui-startup-check.py` 的判定跟着新布局改（锚点能留、`endswith` 必须改），`tests/wording.rs` 补新措辞断言，并把只在真终端能验的项目写成可照做的手工清单。**不留任何只在旧 inline 布局下成立的测试。**

Blocked by: 10, 11, 12, 13, 14, 15

Status: ready-for-agent

**参考:** spec §Testing Decisions（全部）、§10（提示行）、§13（基线）

- [x] `render_block` 那批用例**保留**（票 10 已过一遍）：`a_tool_call_and_its_hook_become_one_ready_block`（**改为直接驱动 `Transcript`**，不再经过已删除的 `take_ready`）、`a_completed_turn_and_an_aborted_one_render_in_different_colors`、`a_diff_line_gets_a_background_from_the_diff_layer`、`the_synthesizers_product_renders_with_the_system_speaker`、`the_answer_block_is_rendered_as_markdown`、`intermediate_narration_is_dim_and_the_answer_is_not`、`a_message_continuation_indents_by_the_label_display_width`、`the_live_tail_wraps_on_display_columns_not_bytes`
- [x] ~~`a_wide_grapheme_leaves_no_blank_cell_after_it`~~ —— **票 10 连带删除**：它测的是 `paint_scrollback`，而那个宽字符 bug 只存在于 `insert_before` 的 scratch buffer；全屏改走 ratatui 自己的 buffer diff，没有自家代码可测。**不要**为它另造一个测试。
- [x] `a_notice_is_a_scrollback_line_shown_as_it_is` **已改名**为 `a_notice_is_a_transcript_line_shown_as_it_is`（"scrollback" 随 inline 视口一起没了），断言收在 `render_block` 上；「通知落在转录里而不是流式尾巴里」由 `tests/render_layout.rs` 的 `the_transcript_pane_shows_both_the_notices_and_the_streaming_tail` 覆盖
- [ ] 编辑器四个用例改写为对着 `Input` 断言：`a_typed_line_is_submitted_to_the_loop`、`backspace_edits_the_line`、`an_empty_submission_closes_the_prompt`、`the_cursor_column_counts_a_wide_character_as_two`
- [ ] 键位语义五个用例保留：`a_permission_question_is_answered_by_key`、`escape_answers_a_question_with_the_non_acting_choice`、`escape_while_working_is_a_cancel_gesture`、`an_idle_ctrl_c_quits_and_a_working_one_cancels`、`shift_tab_is_the_plan_gesture`
- [ ] 辅助函数：`state_with_prompt()` 已跟着新 `TuiState` 改签名（票 10 新增 `facts()` 与 `new_state()` 两个 helper；**不要**把 helper 命名成 `state()`，会和测试里满地的 `let mut state` 撞名）
- [x] `scripts/tui-startup-check.py` 的**判定**：锚点 `STATUS_ANCHOR = "ctrl-c"` 与 `BANNER_ANCHOR = "fs-agent："` **确实保留**；判定已按预测改掉 —— 票 10 落地后脚本先红在一次 `endswith("退出")`（新底部块带边框，`退出` 后面跟着 `│`），改成先 `rstrip(" │")` 再判尾。**票 10 的实现已顺手修掉这一处并实测 `3/3 GREEN`**（banner 只出现一次、提示行完整），剩下的「新增 header 版本串锚点 + 边框存在性断言」仍留本票
- [ ] 脚本**新增锚点**：header 的版本串（确认新布局起来了），再加一条粗断言「屏幕上出现 ≥3 处边框字符」防退化
- [ ] 脚本继续负责：启动不崩、首帧正确、**退出后终端干净**（无 raw mode 残留 / 鼠标捕获 / bracketed paste）
- [ ] `tests/wording.rs` 补断言：新提示集六条、面板中文标签、`↓ {n} 行新内容 · 点此到底` / `点此到底`、`终端太小：至少 40×10`、两个确认文案
- [ ] 加一条**防回归**断言：任何提示文案里都不出现 `shift+enter`
- [ ] `tests/e2e_single_turn.rs`、`tests/replay.rs`、`tests/render_markdown.rs` 确认不受影响（预期不改）
- [ ] 手工清单写成仓库里可照做的文字（真终端）：①光标 7 条 ②权限模态 ③鼠标（滚轮 / 点「点此到底」 / **Shift+拖拽复制**）④resize 锚点与不撕裂 ⑤粘贴（多行不提交、超长确认）⑥`Ctrl-J` 与 `Shift+Enter` 的实际行为 ⑦退出后终端干净（`stty`）⑧忙碌 `Ctrl-C` 取消、空闲退出 ⑨`120×24` 输入 10 行 → 右栏消失
- [ ] 基线：`cargo test` 全绿（489 + 新增）、`cargo clippy --all-targets` 干净、`cargo fmt --check` **只留** `src/context/repo_map.rs` 与 `tests/repo_map.rs` 的既有漂移（**不要**顺手格式化那两个）

## Comments
