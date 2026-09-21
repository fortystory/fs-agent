# 16: 测试与验证迁移：既有用例归类、pty 脚本判定与手工清单

**What to build:** 把验证面收口：`tests/render_tui.rs` 的既有用例逐个归类迁移，`scripts/tui-startup-check.py` 的判定跟着新布局改（锚点能留、`endswith` 必须改），`tests/wording.rs` 补新措辞断言，并把只在真终端能验的项目写成可照做的手工清单。**不留任何只在旧 inline 布局下成立的测试。**

Blocked by: 10, 11, 12, 13, 14, 15

Status: done

**参考:** spec §Testing Decisions（全部）、§10（提示行）、§13（基线）

- [x] `render_block` 那批用例**保留**（票 10 已过一遍）：`a_tool_call_and_its_hook_become_one_ready_block`（**改为直接驱动 `Transcript`**，不再经过已删除的 `take_ready`）、`a_completed_turn_and_an_aborted_one_render_in_different_colors`、`a_diff_line_gets_a_background_from_the_diff_layer`、`the_synthesizers_product_renders_with_the_system_speaker`、`the_answer_block_is_rendered_as_markdown`、`intermediate_narration_is_dim_and_the_answer_is_not`、`a_message_continuation_indents_by_the_label_display_width`、`the_live_tail_wraps_on_display_columns_not_bytes`
- [x] ~~`a_wide_grapheme_leaves_no_blank_cell_after_it`~~ —— **票 10 连带删除**：它测的是 `paint_scrollback`，而那个宽字符 bug 只存在于 `insert_before` 的 scratch buffer；全屏改走 ratatui 自己的 buffer diff，没有自家代码可测。**不要**为它另造一个测试。
- [x] `a_notice_is_a_scrollback_line_shown_as_it_is` **已改名**为 `a_notice_is_a_transcript_line_shown_as_it_is`（"scrollback" 随 inline 视口一起没了），断言收在 `render_block` 上；「通知落在转录里而不是流式尾巴里」由 `tests/render_layout.rs` 的 `the_transcript_pane_shows_both_the_notices_and_the_streaming_tail` 覆盖
- [x] 编辑器四个用例的落点：纯编辑器语义对着 `Input` 断言（`tests/render_editor.rs`：宽字符占两列 = `a_wide_character_takes_two_columns_in_the_draft`，退格并行 = `backspace_and_delete_join_lines_at_the_edges`），并给 `Input::submitted()` 补了 `submitting_clears_the_draft_and_an_empty_one_is_an_empty_line`（trim / 清空 / 空行不进历史 / 去重）。剩下两条断言的是**跨 seam 的东西**（提交给循环、空提交），`Input` 看不见那条通道，所以留在 `tests/render_tui.rs`：`a_typed_line_is_submitted_to_the_loop`、`an_empty_submission_is_an_empty_line_not_the_end_of_input` —— 旧名 `an_empty_submission_closes_the_prompt` 断言的正是那个 bug（空草稿发 `None` = stdin 结束）
- [x] 键位语义五个用例保留：`a_permission_question_is_answered_by_key`、`escape_answers_a_question_with_the_non_acting_choice`、`escape_while_working_is_a_cancel_gesture`、`an_idle_ctrl_c_quits_and_a_working_one_cancels`、`shift_tab_is_the_plan_gesture`（五个都在 `tests/render_tui.rs`）
- [x] **接缝形状**：`draw_frame` 取 `&mut TuiState`，`tests/render_layout.rs` 的 `screen()` / `buffer()` 也跟着收 `&mut`；`live_lines` 与 `take_ready` 在 `tests/` 与 `src/` 里都已无引用；CJK 折行断言现在打在 `pane::wrap_text` 上
- [x] 辅助函数：`facts()`、`new_state()`、`state_with_prompt()` 都在；`render_tui.rs` 里没有名为 `state()` 的 helper（`render_layout.rs` 那个 `state()` 是自己的，不与 `let mut state` 撞名）
- [x] `scripts/tui-startup-check.py` 的**判定**：锚点 `STATUS_ANCHOR = "ctrl-c"` 与 `BANNER_ANCHOR = "fs-agent："` **确实保留**；判定已按预测改掉 —— 票 10 落地后脚本先红在一次 `endswith("退出")`（新底部块带边框，`退出` 后面跟着 `│`），改成先 `rstrip(" │")` 再判尾。**票 10 的实现已顺手修掉这一处并实测 `3/3 GREEN`**（banner 只出现一次、提示行完整）
- [x] 脚本**新增锚点**：header 的身份串。锚点取自**二进制自己的** `--version`（`binary_identity()`），再在重建后的屏幕上找它 —— 它在原始字节流里并不连续（header 的 span 被拆开写），票里原计划的 raw 匹配不成立。边框防退化断言按**横 / 竖分开数**（各 ≥3）：转录与面板之间的接缝是一条独立竖线，`draw_border` 全删它仍在，混在一起数会把无边框的屏判绿（突变实测 `0 横 / 18 竖`）。两处都做了突变验证（改 header 身份 → RED；删 `draw_border` → RED）
- [x] 脚本继续负责：启动不崩、首帧正确、**退出后终端干净**。判定现在同时要求：进程真的退出且状态 0、`\x1b[?1049l` + `?1000l/1002l/1003l/1006l` + `?2004l` 全在、退出后 termios 的 `ICANON` / `ECHO` / `ISIG` 都回来了（raw mode 是 termios 标志而不是转义序列，只能读 fd）。三处各自突变验证：不撤销鼠标 / 括号粘贴 → `the terminal was not given back: …`；不调 `ratatui::restore()` → `the tty was left raw: Modes(canonical=False, echo=False, signals=False)`
- [x] 脚本**再新增**：空回车不许结束会话（`survived_empty_enter`）。这是本票开工前用户报的 bug 的永久红线 —— `reply.send(None)` 被循环读成 stdin 结束，进程以 0 退出。突变回旧写法 → `RED -- an empty Enter ended the session`
- [x] `tests/wording.rs` 补断言：新提示集六条与按宽度丢条的阶梯（`40` / `60` / `80` / `120` 四条实测线）、七个面板中文标签、`↓ {n} 行新内容 · 点此到底` / `点此到底`、`终端太小：至少 40×10`、两个确认文案（已有）、header 身份串
- [x] 加一条**防回归**断言：`no_hint_ever_names_shift_enter` 扫 busy × 宽度 `1..=200`，任何一条提示里出现 `shift+enter` 都红 —— 单点宽度的 `contains` 挡不住它
- [x] `tests/e2e_single_turn.rs`、`tests/replay.rs`、`tests/render_markdown.rs` 确认不受影响：文件未改，`14 / 7 / 7` 全过
- [x] 手工清单写成仓库里可照做的文字：`docs/tui-manual-checklist.md`（九项，每项都是「怎么做 + 该看见什么」）。顺带修掉 `docs/render.md` 里「Inline viewport」那一行 —— ADR 0002 之后它已经不成立 —— 并从那里指向清单
- [x] 基线：`cargo test` **549 passed / 0 failed**（37 个测试目标；spec 记的 489 + 新增 60）；`cargo clippy --all-targets` 干净；`cargo fmt --check` **只剩** `src/context/repo_map.rs` 的既有漂移（5 处）；`python3 scripts/tui-startup-check.py target/debug/fs-agent 3` → `3/3 GREEN`

## Comments

- **本票没有再补布局 / 降级用例**：spec §Testing Decisions 点的那批（尺寸矩阵、提示行条目数、`ctrl-c 退出` 恒在、右栏无数据文案、`120×24` 草稿 10 行右栏消失）已随实现落在 `tests/render_layout.rs`，这里补的是断言的**另外两面**：措辞层与 pty 层。
- **`cargo fmt --check` 的教训**：别 `head -N` 看它 —— 既有漂移正好占满前五行，会把新文件的漂移截掉。本票就是这么漏掉了 `51f4da4`（现 `ac4ea65`）里一行没折行的断言，已回 amend。
- **空回车那个 bug 与本票的关系**：用户报「在输入框里直接按回车会退出」。根因是 `submit()` 用 `Some` / `None` 兼职表示 EOF，而 TUI 从不需要用返回值表示输入结束（退出走 `state.quit` / `FrontEndEvent::Quit`）。修法是永远发 `Some(line)`，空行由循环 `trim().is_empty()` 丢掉 —— 前端只如实报告发生了什么，怎么处理是循环的事。
