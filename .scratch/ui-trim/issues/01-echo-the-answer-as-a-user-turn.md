# 01 — 问卷答完，答案以一条用户发言进对话

Type: implement
Status: done
Part of: ../spec.md

`ask_user_question` 的结果现在是那段 `UserAnswers` JSON，只在工具行点开的详情里。答完问卷
之后，转录里应当出现一条**用户发言**：把所选标签与自定义文本列出来。

规格见 [`spec.md`](../spec.md) 的问题陈述 1 与「定下来的四条」第一条。

## 要做什么

- `Block` 加一个变体（`UserAnswer`），——它由 `Transcript::push` 在**结果到达**时推出来，
  而不是由前端本地插一行：流上有那次工具调用与它的结果，所以 `--continue` 重放会重现它，
  TUI 与 plain 也自动是同一份呈现。
- 认哪一次调用：pending 的那次工具调用的名字是 `ask_user_question`（措辞层已经有一个同名的
  常量，别再写第二份字面量），结果的 `output` 能解析成 `UserAnswers` 且至少有一条答案。
- 画的形态：**用户发言**那一档 —— TUI 里走过 `bubble()` 那条路（与用户自己打的字同宽同底色，
  但不带名字行），plain 里走用户前缀那一档。
- 措辞进 `render/wording.rs`：跳过（`selected` 与 `custom` 都空）要说成「跳过」，而不是画一个
  空行；多选题的 `selected` 与 `custom` 并存，两个都要显示。
- 判据是**结果**，不是调用：一次被取消的问卷没有结果，也就没有回显；解析不出来时只画工具行
  （与今天一样），不 panic、不留半行。

## 验收

- 一条单选答案、一条多选答案 + 自定义文本、一条显式跳过，各有一条转录断言（TUI 与 plain
  各覆盖一条就够 —— 两者共用 `Transcript`，差别只在画法）。
- 重放同一段事件流（工具调用 + 结果）得到同一条发言。
- 不是 `ask_user_question` 的工具结果一个字节不变。

## 评论

落地（2026-10-08）。`Block::Answer { text }` 由 `Transcript::push` 在**结果**到达时推出来
（`answer_block`：名字是 `ask_user_question`、结果能按 `UserAnswers` 读出来、至少一条答案），
措辞在 `wording::answered`（一道题一行 `题面：答案`，题面从调用参数里取，两边都对不上时退
`id`）。两个前端各转发给「用户发言」那条路：TUI 里 `paint_block` 把它拼成一条
`Block::Message { speaker: User, role: User }` 再交给原分支，于是气泡、颜色、前缀一处不落；
plain 里同样转发给 `message`。`selects` / `block_speaker` / `is_user_message` 三处判据一并
跟上。

新文件 `tests/render_answer.rs` 六条：块的四条（正常、跳过、别的工具不给、结果读不出来）、
TUI 屏幕一条、气泡一条（靠右 + 底色）。

**偏差**：结果 JSON 里的 `selected` 是必填字段（`questions::UserAnswer` 上没有
`serde(default)`），所以手写测试数据时漏了它会整份解析失败 —— 不是这次要改的东西，真实
工具结果总是带上它。
