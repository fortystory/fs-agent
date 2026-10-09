# 02 — 文档、词汇与索引收口

Type: implement
Status: done
Blocked by: 01

> 规格：[`../spec.md`](../spec.md) §3（三条不变式）、§4（可观察性）、§7（明确不做）。
> 这一票**基本不写 Rust**：它把 `workdir` 的行为写进逐面文档、词汇表与 feature 索引。
> 散文的大部分在 grilling 阶段就已先落盘（下面逐份点名），本票做的是**按落地的实现复核**
> 每一句、补索引、把两张票的状态收成实际完成度。

## 目标

一个人在 `docs/bash.md` 里读到 `bash` 的四个参数，知道 `workdir` 换的是站位而不是边界，
并且在 [`docs/sandbox.md`](../../../docs/sandbox.md) 与 `CONTEXT.md` 里查得到「边界锚在会话工作区、
工作区不因工具参数而变」这句话。`.scratch/README.md` 的 feature 索引反映实际进度。

## 已落盘的散文（grilling 阶段先写，实施后**逐句复核**）

| 文件 | 已写的内容 | 复核什么 |
| --- | --- | --- |
| [`docs/bash.md`](../../../docs/bash.md) | 形状表加 `workdir` 与「站位」两行；新增「## `workdir`：这条命令站在哪」一节；「它不包含什么」加一条会话中途换工作区；「代码住在哪」加三行 | 参数名与 `required` 与实现一致；四条错误文案与实现逐字一致；那句模型可见描述与 `spec()` 里的一致 |
| [`docs/sandbox.md`](../../../docs/sandbox.md) | 「## 边界」末尾一段：三条清单锚在会话工作区、`wrap()` 拆两参、以及「若当初传同一个值会怎样」那条推理 | `wrap()` 的参数名与调用点一致 |
| [`CONTEXT.md`](../../../CONTEXT.md) | 「会话存储」节新增**工作区（workspace）**一条，`_Avoid_` 点名 `workdir` 是工具参数 | 与「会话桶」「会话目录」那两条不重复也不冲突；护栏 `scripts/check-doc-size.py` 仍绿 |
| [`docs/permissions.md`](../../../docs/permissions.md) | **刻意没动** | spec §7 说它那节说的是「会话工作区」，正是本票保持不变的东西——确认无需补 |

## 还要做的

1. **feature 索引**：[`.scratch/README.md`](../../README.md) 的表里 `bash-workdir` 那一行
   （形态 `spec`、一句话、票数与状态）跟着票 01 一起更新；形态列的取值照表里既有写法
   （`spec` / `map + spec` / `seed`）。
2. **收口**：[`../spec.md`](../spec.md) 抬头的 `Status:` 改成实际完成度（照
   [`docs/agents/issue-tracker.md`](../../../docs/agents/issue-tracker.md)：票全落地后改成
   `2/2 done` 这种实际值，不要停在 `ready-for-agent`）。
3. **护栏**：跑 `python3 scripts/check-doc-size.py` 与 `scripts/check-language.py`（若有），
   改动不得让任何文件的新违规数超过基线。
4. **不需要**真机走查：`workdir` 落在 `args` 之后，plain 下 `bash（command=…，workdir=…）`
   一眼可见；本票的判据是文档与实现一致，不是观感。

## 不做

- 不新增 ADR（grilling 的决定是：写进 `docs/bash.md` 即可）。
- 不改 `docs/research/` 与 `docs/adr/` 里任何既有文件。
- 不动 `README.md` 的「文档」一节（`docs/bash.md` 早就在那张表里，这一票不给它加新面）。

## 评论

2026-10-09 收口。逐句复核的结果：

- `docs/bash.md`：首行补回漏写的 `escalation?`；`## workdir` 一节补上 `WORKDIR_NOTE` 的逐字
  引文与四条错误各自的落点；「边界」那条与「代码住在哪」按实现改了措辞（`run()` 收边界与站位、
  `wrap()` 只收边界）。
- `docs/sandbox.md`：同一句照同一个形状改。
- `CONTEXT.md` 的**工作区**一条与「会话目录」「会话桶」不重叠也不冲突，不动。
- `docs/permissions.md` 确认无需补：它说的就是「会话工作区」，正是这次保持不变的东西。
- `.scratch/README.md` 那一行与 `../spec.md` 抬头的状态都收到 `2/2 done`。

验证：`python3 scripts/check-doc-size.py` 与 `python3 scripts/check-language.py` 都绿。
