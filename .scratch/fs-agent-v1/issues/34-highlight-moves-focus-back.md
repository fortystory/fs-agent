# 34 — 问卷里 `↑`/`↓` 移动高亮要把焦点带回选项区

Type: implement
Status: done
Blocked by: —

> 这一票没有 spec：它是 [`33`](33-space-in-the-questionnaire-custom-answer.md) 落地后暴露出来的
> 一处焦点缺口。来源是 2026-10-02 维护者看过票 33 的《已知后果》之后，确认「用 `↑`/`↓` 挑一个
> 选项、再按空格确认」这条路要保留。

## 目标

在问卷里，`↑`/`↓` 把高亮挪到选项上时，光标要跟着回到选项区 —— 与点选项行（`select_option`）
一致。否则人打过字之后焦点一直留在自由文本栏，而票 33 之后空格在那里只是文本。

## 现状（2026-10-02 核实，改前先复核）

- `Questionnaire::move_highlight`（`src/render/tui.rs:881-889`）只挪 `draft.highlight`，**不碰**
  `custom_focused`。
- 清焦点的地方只有四处：`advance`、`back`、`select_option`、`unfocus_custom` —— 移动高亮不在其中。
- 后果：打过字（`custom_focused = true`）之后按 `↑`/`↓`，焦点仍在文本框。票 33 之前这一点没有
  可观察后果（空格永远是确认），票 33 之后它就成了「打过字就再也按不出确认」。
- 测试里没有覆盖这条（`tests/ask_user_question_tui.rs` 里 `↑`/`↓` 只出现在选项窗口滚动那条）。

## 具体行为

- `move_highlight` 在**有选项**时把 `custom_focused` 置假：挪高亮就是「光标在选项区」。
- 没有选项的题（`count == 0`）照旧什么都不做，**也不碰焦点** —— 那里没有选项区可回，输入区
  是唯一的落点。
- `confirm_highlight`、`type_custom`、`Enter`、`Space` 的既有语义一律不动。

## 测试

- `tests/ask_user_question_tui.rs` 加一条：打字 → `↓` → 空格 ⇒ 空格确认的是挪过去的高亮，
  自由文本里不多出一个空格；提交后 `selected` 是挪过去那个、`custom` 为空（单选的既有语义）。
- `cargo test` 全绿、`cargo clippy --all-targets` 干净。

## 不做什么

- 不加 `j`/`k` 导航（那是 [`.scratch/questionnaire-keys/seed.md`](../questionnaire-keys/seed.md)
  的意向，要单独访谈）。
- 不动 `Esc` 的语义、不动单选的「自定义覆盖选择」、不动 footer 的文案与布局。
- 不引入「选项区 / 输入区」这套命名（同上，属于那次访谈）。

## Comments

- **落地（2026-10-02）**：`move_highlight` 在有选项时置 `custom_focused = false` —— 就一行，放在 `count == 0` 早退**之后**，所以没有选项的题不碰焦点。别的没动。
- **先红后绿**：`moving_the_highlight_takes_the_focus_back_to_the_options` 改前红（屏幕上是 `自定义：x`，空格被当成文本塞了进去），改后绿。问卷那个测试文件 15 → 16 条。
- **验收**：`cargo test` **979 passed / 0 failed**（修前 978）；`cargo clippy --all-targets` 干净；`cargo fmt`（跑完还原了它顺手改的 `tests/ask_user_question.rs` —— 那处与本票无关）。
- 票 33 的《已知后果》到此关闭：`↑`/`↓` 挑选项再按空格这条路回来了。
