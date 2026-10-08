# 02 — `/skill 任务`：技能名不再从发给模型的文本里被删掉

Type: implement
Status: done
Part of: ../spec.md

`submission()` 认出 `/<skill>` 之后，送给模型的任务是记号**两侧**的字（`task_around`），技能
名本身被丢掉 —— 于是 `/research 帮我查 X` 在对话里长成 `帮我查 X`。

规格见 [`spec.md`](../spec.md) 的问题陈述 2 与「定下来的四条」第二条。

## 要做什么

- `Submission::Skill` 的 `task` 改成**整条原文**（尾部换行去掉），不再走 `task_around`。
- **裸调用**的判据随之显式写出来：整条草稿只有那一个记号（`whole`，内建命令已经在用的那个
  判断），此时仍走「正文就是指令」那条路，不多发一条 user 消息。`/research` 与
  `/research   ` （尾巴是空白）都算裸调用；`/research 查 X` 不算。
- 技能名前面的字不许丢：`请用 /research 帮我查 X` 送出去的就是这一整条。
- 内建命令（`/discuss`、`/goal-new`、`/loop`、`/model`、`/effort`）与 MCP 模板的参数语义
  一个字不动 —— 它们的参数是记号旁边的字。

## 验收

- `submission` 的单元断言：`/research 帮我查 X` 的 `task` 是整条原文；`/research` 是裸调用；
  `请用 /research 帮我查 X` 一个字不丢。
- 技能正文照旧加载（`load_skill` 那条路不变）。

## 评论

落地（2026-10-08）。`Submission::Skill` 多了一个 `bare`，`task` 改成**整条原文**
（`text.trim_end()`）；裸调用的判据从「去掉记号后为空」换成显式的 `only_token`（整条草稿只有
那一个记号，`//review` 也算），于是「正文就是指令」那条路一个字节没变，而 `/research 帮我查 X`
发出去的就是这一条。`whole` 保留给内建命令（它还要 `token.query == name`）。

断言：`/ask-matt 帮我看一下`、裸 `/ask-matt`、`//review`、多行、以及新加的
`a_skill_keeps_the_words_around_its_name`（记号**前面**的字也不丢）。
