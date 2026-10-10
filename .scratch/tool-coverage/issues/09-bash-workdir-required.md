# 09 — `bash` 的站位必传与 `cwd:` 回显（tracer bullet）

Type: implement
Status: done
Part of: ../map.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §2。来源票：
> [bash 的 workdir 必传与 cwd 反馈](03-grilling-bash-workdir-required.md)。
> **与 [08 — grep 的上下文行](08-grep-context-lines.md) 同批发布**。

## 目标

每次跑命令都要显式声明站位（根写 `.`），结果首行就能看到命令实际落在哪个目录；
带升级申请的重试不再因为"两个字段同现"被自己的参数校验打死。

**可 demo**：跑一条 `pwd`，结果第一行是 `cwd: .`（或子目录的相对路径）。

## 现状（改前先复核）

- `workdir` 参数 2026-10-09 就上线了，但**可选**，spec 明文写着 `required` 仍是 `["command"]`；
  两天里 3175 次调用只有 1 次用它（0.01%），而 86% 的调用以 `cd` 开头。
- 结果报告今天没有站位这一行。
- `workdir` 与 `escalation` 同现是参数错误 —— 必传之后这条会打死**每一次**升级调用。

## 验收

- [x] `required` 变成 `["command", "workdir"]`；省略时的错误文案点明"根写 `.`"
- [x] `.` 合法并解析成工作区根；空字符串 / 不存在 / 工作区外都是参数错误
- [x] 结果**首行**固定 `cwd: <相对工作区的路径>`（根写作 `.`），**无条件**加；
      退出码、stdout、stderr 一个字不动
- [x] 删掉 `workdir` 与 `escalation` 的互斥；带 `escalation` 的重试能正常走到沙箱那一档
- [x] 站位描述常量改写成"必填 + 根写 `.` + 用它代替在命令里写 `cd`"
- [x] `cd` 冗余**只改描述**：不加结果提示、不在工具层拒绝
- [x] `docs/bash.md` 与 bash-workdir 那份 spec 的 `required` 明文、§2 的互斥条同步，
      并留下推翻依据（3175 次调用只有 1 次带它）
- [x] 扩展既有 bash 工具测试：省略 / 空串 / `.` / 不存在 / 区外 的裁决与文案、
      结果首行形状、`escalation` 重试不再被互斥挡住
