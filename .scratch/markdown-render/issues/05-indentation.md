# assistant 的续行不再缩进

Type: implement
Status: done

> 规格：`.scratch/markdown-render/spec.md` §5。
> **不依赖票 01**——这一票只动 `attribute` 与它的两个调用点，与解析器无关，可以并行。
> 它推翻 [`.scratch/tui-layout/spec.md`](../../tui-layout/spec.md) §3 里「**assistant 的消息本来就是全文 Markdown，不动**」以及「续行按 speaker 前缀显示宽度缩进」这半句（非 assistant 那一半留着）。

## 目标

assistant 的回答正文不再整体缩进 11 格（`[deepseek] ` 的宽度），主列宽度都用来放正文；**用户自己输入的多行消息仍然缩进对齐**。

## 落点

`src/render/tui.rs` 的 `attribute`（第 3550 行起）与它的两个调用点（第 3658 行 assistant 那支、第 3665 行其余那支）；`tests/render_tui.rs`。

## 具体行为

1. `attribute` 现在对**每一行**都加前导：`index == 0` 用 `[speaker] ` 前缀，其余用等宽空格。把它拆成**两条名字清楚的路**，不是一个布尔开关：
   - **文档那一支**（assistant 的 `Block::Message`）：只有第一行有 `[speaker] ` 前缀，**其余行顶格**——零前导，不是空 span。
   - **一次发言那一支**（用户输入、非 assistant 的系统行）：维持现状，续行仍按 `[speaker] ` 的显示宽度缩进。
2. 两条路的**名字样式**（`name_style`）与正文样式照旧——变的只有前导。
3. `plain` 渲染器不经过 `attribute`，**不受影响**；确认一下别顺手改到它。

## 测试

- `the_answer_block_is_rendered_as_markdown` 要跟着改：它现在断言的是渲染出来的标题行，注意前缀变化。
- **新增两条**：
  - assistant 的多行消息（比如 `"# 标题\n\n正文\n第二行"`），**除第一行外每一行的第一个 span 都不是空白**（顶格）；
  - 用户的多行消息，**续行以 `[name] ` 宽度的空格开头**（缩进仍然在）。
- 一条**边界**：assistant 单行消息的输出与今天**逐字相同**（第一行的前缀没变）。

## 验收

- [ ] `cargo test` 全绿。
- [ ] `cargo clippy --all-targets` 无新增告警。
- [ ] 真机：`cargo run` 问一句长的，回答的每一行都从最左边起；自己按 `Ctrl-J` 敲一条多行消息，续行仍然对齐缩进。

## Comments

- 2026-10-01 落地：`attribute` 拆成两条路 —— `attribute_document`（assistant：`[name] ` 只引领第一行，其余行**零前导**）与 `attribute_speech`（用户输入与非 assistant 系统行：续行仍按 `[name] ` 的显示宽度缩进）。名字样式与正文样式没动；`plain` 渲染器本来就不走 `attribute`。
- 与票面不同的一点：assistant 的 Markdown 按 `width − [name] 的列宽` 排版，否则第一行会溢出、被窗格折行。代价是**表格的首行相对数据行右移一个前缀宽**（表内各列仍然彼此对齐，测试钉住的是这一点）—— 取舍记在 `paint_block` 的注释里。
- 测试：`tests/render_tui.rs` 的 `the_answers_continuation_starts_at_the_left_edge`、`a_single_line_answer_keeps_the_same_speaker_prefix`，以及改走 `Role::User` 的 `a_message_continuation_indents_by_the_label_display_width`。
- 真机没跑，步骤在 `docs/tui-manual-checklist.md` ㉑ 第 7 条。
