# 04 — 删掉输入框的 `❱`

Type: implement
Status: done
Part of: ../spec.md

`❱` 的来由是「画家给这个字形的颜色绕着色相轮走」（`tui-input-pulse` §2b）。维护者点掉了这个
字形，于是草稿第一行不再有提示符、后续行也不再有那两列缩进。

规格见 [`spec.md`](../spec.md) 的问题陈述 4。

## 要做什么

- `wording::PROMPT` 与 `editor::prompt_columns()` 整套离场（字形只有一个归宿这条纪律的推导也
  一并撤掉）：`Input::view` 不再往每行前面塞那一个 span，`display_rows` 也不再按它算光标列。
- `layout::input_text_width` 不再减它 —— 输入行有多少列留给文字，就是主列的内容宽度。
- 屏幕上原来那个 `┆❱ ` 变成 `┆`：外壳的竖线与输入行之间的那一列还在，只是不再有字形。
- 提示行、`/` 菜单、`@` 引用、历史的命中矩形与它们无关，一个都不动。
- 手工清单里「提示符是个模糊宽度字符、终端可能画成两列」那条检查随之作废（`❱` 不在了），
  换成一条「输入行第一列就是草稿的第一个字」的走查项。

## 验收

- 布局断言改成「输入区第一行的文字从主列第一列起」，并删掉按 `PROMPT` 找格子的那几条。
- `Input::view` 的单元断言：第一行与后续行都不再带前缀，折行宽度因此多出两列。
- `cargo test` 里所有引用 `PROMPT` / `prompt_columns` 的断言改完，不留 `.unwrap()` 掉的
  旧写法。

## 评论

落地（2026-10-08）。`wording::PROMPT`、`editor::PROMPT` 的转发与 `editor::prompt_columns()`
整套删掉；`Input::view` 不再给每行塞前导 span，`display_rows` 的光标列不再加那个偏移，
`layout::input_text_width` 与 `content_width` 合并成一个（提示符消失之后两者相同）。

屏幕断言随之改成「输入区第一行是一份空草稿」「草稿从第一列起」；定位输入行的三处帮助函数
（`input_row` / `draft_colour` / 新的 `input_corner`）改问 `layout::plan`，不再找那个字形。
`tests/render_editor.rs` 里所有 `2 + n` 的列偏移归零。
