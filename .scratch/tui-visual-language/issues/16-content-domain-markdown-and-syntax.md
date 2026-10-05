# 16 — 内容域：markdown 与语法高亮

Type: implement
Status: ready-for-walkthrough
Part of: ../map.md
Blocked by: 11

> 规格：[`../spec.md`](../spec.md) 实现决定 §3（内容域与界面域分家）与 §36 里内容域那几条。
> **矩阵里的「最刺眼的一处」就在这张票**：`Yellow` 今天同时当「markdown 行内代码」与「警告」。

## 目标

代码块与正文里的颜色**不再与界面信号抢同一个色值**：行内代码不再用警告黄，语法数字不再与 `Warn` 同色；而**代码块内部的语法分类照旧保留**（它是帮读的分类，不是信号）。内容域与界面域**允许撞值**这条边界，写进代码注释。

## 现状（改前先复核）

- 行内代码用 `Yellow` —— 与诊断、hook 反馈、`Severity::Warn` **完全同色**。
- 语法高亮里数字/常量也用 `Yellow`，同一个问题。
- 标题用 `Cyan`+`BOLD`（与 `Severity::Note` 撞）、字符串用 `Green`（与 `Severity::Good` 撞）、类型用 `Cyan`；注释用 `DarkGray`+`ITALIC`、标点用 `Gray`、引用与网格用 `Gray`。
- 内容域的常量（代码 / 弱化 / 网格）今天住在 markdown 模块里，与界面域的颜色没有任何分区说明。

## 落点

- `src/render/markdown.rs`、`src/render/highlight.rs`：取色。
- `src/render/palette.rs`：**加一个内容域分节**，与界面域在文件里明确分开。

## 具体行为

1. **行内代码从 `Yellow` 改静态档** —— 「第一刀」就是这一处。
2. **语法数字从 `Yellow` 改 `LightYellow`**，把「代码里的数字」与「界面上的警告」分开。
3. 语法**标点与注释复用静态档**（不再单立两个灰值）。
4. 标题保留 `Cyan`+`BOLD`、字符串保留 `Green`、关键字与函数保留现有色 —— 它们与界面域撞值**是允许的**（代码块内部上下文明确，不会被读混）。
5. 在色板里用一条注释写明**分家规则**：界面域回答「要不要注意 / 有没有被选中」，内容域回答「这是什么」。这是本 spec 唯一的「两套规则并存」，代价显式接受。

## 验证

1. 逐格缓冲快照：行内代码、语法数字、标题、引用、网格各一条，断言**引用色板常量**。
2. `tests/render_markdown.rs`（20 处，全是钉语义的）与 `tests/render_highlight.rs` 按新常量改写 —— 调研说这两处的断言大多是钉语义的，改起来轻。
3. `tests/render_highlight.rs` 里那三处**钉 ANSI 字节**的 diff 断言**不许动**（它们测的是另一套独立的 ANSI 表）。
4. 真终端：手工清单 **㉑**（代码块高亮分色、续行缩进）。

## 不做什么

- 不给代码高亮加新语言，也不改高亮的**分类粒度**。
- 不动 diff 那套独立的 ANSI 表（它是 plain 与未来的 diff 视图共用的）。
- 不动 markdown 的**排版**（折行、表格列宽、缩进都不变）。

## 实现记录（2026-10-06）

**已落地**（自动化部分全绿：`cargo test --all-targets` 1216 passed / 0 failed、`cargo clippy --all-targets` 与 `cargo fmt --check` 干净、`scripts/check-doc-size.py` 与 `check-language.py` 绿、`scripts/tui-startup-check.py` 15/15 绿）。

内容域在色板里单开一节；行内代码与语法注释/标点走静音档、语法数字改 `LightYellow`。**真终端**：手工清单 ㉑。
