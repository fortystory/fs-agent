# 08 — 回车是「下一题 / 提交」：空格仍是唯一的选中键

Type: implement
Status: ready-for-agent
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
