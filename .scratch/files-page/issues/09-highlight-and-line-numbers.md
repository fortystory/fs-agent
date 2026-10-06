# 09 — 弹窗里的语法高亮与行号

Type: implement
Status: ready-for-walkthrough
Part of: ../map.md
Blocked by: 08

> 规格：[`../spec.md`](../spec.md) 的实现决定 §6 与用户故事 D23。前置是
> [08 — 文件内容弹窗：读盘与有界截断](08-file-content-overlay.md)。

**What to build:** 弹窗里的代码读起来像代码：按扩展名认语言、走既有的那一层高亮，并且每一个
逻辑行前面带行号（折出来的续行不带）。

## 验收

- [ ] 按扩展名认语言；认不出的按纯文本画，不报错、不画错色
- [ ] 高亮走既有的那一层（与 markdown 代码块同一个源），窄档下不溢出、不挤掉正文
- [ ] 行号只画在**逻辑行的第一行**上，折出来的续行不带
- [ ] 行号与正文的列宽口径对齐（行号不会把正文挤出文本区）
- [ ] 高亮与行号只画在弹窗里，不牵动转录、不进流

## 评论

- **落地（2026-10-06）**：`files::language_for`（扩展名 → 十种语言那一份名单里认的名字，认不出
  就退纯文本、不报错）与 `file_body_lines`（整段源码交给 `highlight::highlight_code`，逐行加
  行号前缀）。**行号插在折行之前** —— 折出来的续行自然不带行号；行号那一列从折行预算里扣掉，
  所以窄档下也挤不掉正文。
- 行号会跟着拖选被复制（与轨迹页的时间戳前缀同一个待遇），这一条写在 `docs/render.md` 里。
- 断言：`the_extension_picks_the_language_the_highlighter_knows`（单测）、
  `the_overlay_highlights_the_code_it_knows`、`a_language_it_does_not_know_is_still_drawn_as_text`、
  `line_numbers_sit_on_the_first_display_line_of_a_logical_line`。
