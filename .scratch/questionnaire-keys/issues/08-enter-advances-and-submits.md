# 08 — 回车是「下一题 / 提交」：空格仍是唯一的选中键

Type: implement
Status: done
Blocked by: —

> 来源：[`../spec.md`](../spec.md) §11。这一票**推翻**同一份 spec §4 里「选项区里 `Enter` 与
> 空格完全一致」那半条 —— 它是 2026-10-02 那一轮定的，用了两天之后被否掉。ADR 0010 的两条
> 决定（键位按区域分派、答案一个形状）**不受影响**，所以没有新 ADR。

## 目标

把「回车」从「与空格等价的确认键」改成「处置这一题、往前走」，空格仍是唯一的选中键：

- **`Enter`**（选项区与输入区**同一条**，不再按区域分叉、也不再需要 `has_options()` 守卫）：
  1. 当前题**还没作答**就把它记成**跳过**；
  2. 然后若**每题都有着落**（`all_handled()`）→ **提交**，否则 `advance()`。
- **`→` 与页脚「下一题 →」**：当前题没作答就记跳过，然后 `advance()`；**末题上什么都不做**
  （不记、不前进、不提交）。
- **`←` 与页脚「上一题」**：只移动，**不记任何东西**。
- **作答即撤销跳过**：`confirm_highlight()`（空格确认与点击选项行都走它）与 `type_custom()`
  清掉该题的 `skipped`。**只清不回滚** —— 把文本删空不重新标记。
- **页脚**：当前题已跳过时，进度后面跟一句 `已跳过`；提示段跟着 `Enter` 实际会做什么变
  （会提交时 `回车 提交`，否则 `回车 下一题`），三档宽度阈值重算。
- **词表与文档**：`CONTEXT.md` 的「选项区」与「作答草稿」两条、
  [`docs/render.md`](../../../docs/render.md) 的「问卷：区域与键位」一节、
  [`docs/tui-manual-checklist.md`](../../../docs/tui-manual-checklist.md) 补 ㉗、
  [`.scratch/README.md`](../../README.md) 索引那行。

## 现状（2026-10-04 核实，改前先复核）

- `press` 的 `Key::Enter`（[`src/render/tui.rs:839-857`](../../../src/render/tui.rs)）：先
  `all_handled()` 提交；否则选项区走 `confirm_highlight()`（**与空格完全一致**）、输入区在
  该题已成立时才 `advance()`。
- 空格（[:862-864](../../../src/render/tui.rs)）：选项区里且有选项 → `confirm_highlight()`。
  **这一支不动。**
- `Tab`（[:866-869](../../../src/render/tui.rs)）：置 `skipped` + `advance()`。**这一支不动。**
- `←`/`→`（[:875-876](../../../src/render/tui.rs)）：`back()` / `advance()`，都不碰 `skipped`。
- `advance()`（[:952-958](../../../src/render/tui.rs)）在末题上是 **no-op**；今天「末题回车 =
  提交」是 `all_handled()` 那条抢先返回的结果，不是一条位置判据。
- **`skipped` 今天只有一个写点**（[:867](../../../src/render/tui.rs)）且**没有任何清除路径**；
  而 `answers()` 在它成立时**无条件**交回空答案（[:1011-1017](../../../src/render/tui.rs)），
  忽略 `selected` 与 `custom`。所以「记跳过」必须只发生在**当前题还没作答**时，否则已经选好的
  答案会被静默吞掉。
- 页脚：提示三档 `questionnaire_hint`（[`src/render/wording.rs:849-882`](../../../src/render/wording.rs)，
  阈值 `57/34/12` 是**实测宽度**）；绘制在 [:4076-4131](../../../src/render/tui.rs)。
  `questionnaire_submit()` 只在每题都有着落时才画。

## 收尾

- [`tests/ask_user_question_tui.rs`](../../../tests/ask_user_question_tui.rs) 里这几条**语义被推翻
  或扩写**，逐条处理：
  - `enter_matches_space_in_the_options_zone_and_pages_from_the_text_input`（[:354](../../../tests/ask_user_question_tui.rs)）
    —— **名字就是被推翻的那条决定**，改写成「选项区回车只前进、不改答案」与「两个区域同一条规则」；
  - `enter_keeps_typed_text_instead_of_re_confirming_an_option`（[:378](../../../tests/ask_user_question_tui.rs)）
    —— 复核后保留或改写；
  - `submit_is_refused_until_every_question_is_answered_or_skipped`（[:187](../../../tests/ask_user_question_tui.rs)）
    —— 提交判据没变，但「走到都有着落」的路径变了，复核；
  - `a_skipped_question_is_no_answer_even_after_typing`（[:238](../../../tests/ask_user_question_tui.rs)）
    —— **反过来写**：作答要撤销跳过；
  - 新增：「`→` 记跳过」「`←` 不记」「末题 `→` 无动作」「已作答的题往前走不被记成跳过」。
- [`tests/wording.rs`](../../../tests/wording.rs) 的
  `the_questionnaire_hint_drops_the_teaching_parts_first`（[:1143](../../../tests/wording.rs)）
  按新文案与**重算后的阈值**改写，并覆盖 `已跳过` 那一段。
- [`tests/render_layout.rs`](../../../tests/render_layout.rs)：页脚出现 `已跳过` 时的绘制与降级。
- `docs/tui-manual-checklist.md` 补一节，走查条目见 spec §11 与那一节本身。
- `python3 scripts/check-language.py` 通过。

## 不做什么

- **不写新 ADR**：[ADR 0010](../../../docs/adr/0010-questionnaire-keys-dispatch-by-zone.md) 的两条
  决定都还站着，这一票只动 spec §4 的一半。
- **不动 `plain`**（[`src/render/input.rs`](../../../src/render/input.rs)）：一次一行表达不了
  「跳过」与「没走到」的分野（spec §10 第一条）。
- 不动空格、`Tab`、`Esc`/`Ctrl-C` 两把举手、`j`/`k` 的移动分派、折行与选项窗口滚动。
- 不给末题的 `→` 加提交 —— 提交不可逆，不该由一个移动键承担。
- 不做「撤销跳过」的专门按钮或快捷键：撤销就是**作答**。

## 评论

- 2026-10-05 落地：`Questionnaire::press` 里的 `Key::Enter` 不再按区域分叉 —— 一律
  `skip_if_unanswered()`，然后每题都有着落就提交、否则 `advance()`。`Key::Right` 走同一条
  「往前走」的规则，但外面套了 `if self.index + 1 < self.questions.len()`：末题上**什么都不
  做**（不记、不前进、不提交）。`←` 照旧只 `back()`。
- 新增 `skip_if_unanswered()`（只动没作答的题，答过的不被改成跳过）与 `enter_submits()`（把
  当前题记成跳过之后是不是每题都有着落 —— 页脚据此选文案）。`confirm_highlight()` 与
  `type_custom()` 各加一句 `skipped = false`，即「作答撤销跳过、只清不回滚」。`advance()`
  与 `Tab`、空格三处一个字没动。
- 页脚：`draw_questionnaire_footer` 的进度段在 `skipped` 时变成 `1 / 2 已跳过`（属于进度段，
  不参与降级），尾段改成 `questionnaire_hint(room, enter_submits())`。
- 文案与阈值：新增 `questionnaire_skipped()`；`questionnaire_hint(room, submits)` 多一个
  参数，两档含 `回车 提交` / `回车 下一题` 两种写法，阈值按新文案**实测重算**为
  71 / 48 / 12（`回车 下一题` 比 `回车 提交` 宽两列，取两者之大）。`tests/wording.rs` 那条
  改成用 `text_columns()` 自己量出边界再断言 —— 阈值与文案宽度不再可能悄悄脱节。
- 撞上的既有测试比票里预想的多两条，逐条按新语义改写（意图保留）：
  `enter_matches_space_in_the_options_zone_and_pages_from_the_text_input` 改名并重写为
  `enter_advances_in_both_zones_and_never_changes_a_selection`；
  `enter_keeps_typed_text_instead_of_re_confirming_an_option` 去掉多余的一次回车，改名
  `..._instead_of_marking_the_question_skipped`；`submit_is_refused_until_every_question_is_answered_or_skipped`
  改用空格作答并加一条「末题 `→` 不提交」；`a_skipped_question_is_no_answer_even_after_typing`
  反过来写成 `answering_undoes_a_skip_so_the_text_travels_back`。另外五条只是因为「回车过去
  兼作确认」而受影响，把确认那一下换成空格：`typing_and_a_single_select_choice_travel_back_together`、
  `the_option_window_scrolls_so_the_highlighted_option_stays_visible`、
  `the_recommended_marker_is_display_only`、`a_space_is_text_once_the_custom_answer_has_focus`、
  `a_space_still_confirms_while_nobody_is_typing`、`the_arrows_page_between_questions_and_the_footer_says_where_we_are`；
  `a_question_with_no_options_is_answered_with_free_text` 拆成「回车 = 空答案（跳过）」与
  「打字 = 文本答案」两半。
- 新增：`moving_forward_marks_a_question_skipped_but_moving_back_never_does`（`→` 记跳过、
  `←` 不记、答过的题不被记、末题 `→` 无动作）、`tests/render_layout.rs` 的
  `the_questionnaire_footer_says_what_enter_would_do_and_what_was_skipped`（文案跟着
  `enter_submits` 变、`已跳过` 不参与降级）。
- 文档（`CONTEXT.md` 的「作答草稿」与「选项区」两条、`docs/render.md` 的问卷节、清单 ㉗）
  在 2026-10-04 拆票时已按 §11 写好，本轮逐条核对与实现一致；`.scratch/README.md` 那一行
  的计数改成 `7 done + 1 ready-for-walkthrough`。`check-language.py` 与 `check-doc-size.py`
  都退出 0。
- **`/code-review` 抓到一个真漏**：页脚那个「下一题 →」**按钮**原来只 `advance()`，没走
  `skip_if_unanswered()` —— 键盘 `→` 做了、点击没做，而 spec §11 与 `docs/render.md` 都写着
  这三个手势是同一个动作。后果不只在提示：点着走过一道没作答的题，`all_handled()` 仍为假，
  末题回车会被 `skip_if_unanswered()` 记过之后卡在无处可去的 `advance()` 上。已补上，并加了
  `tests/render_layout.rs` 的 `clicking_next_marks_the_question_skipped_like_the_arrow_key`
  （点过去再翻回来，页脚该写 `已跳过`）—— 原来这条没有回归网。`docs/render.md` 也补了一句
  「末题上的 `→` 什么都不做」，那是实现里有、文档里漏掉的例外。
