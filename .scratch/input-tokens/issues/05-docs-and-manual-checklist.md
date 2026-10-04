# 05 — 文档与手工清单

Type: implement
Status: ready-for-agent
Blocked by: 01, 02, 03, 04

> 来源：[`../spec.md`](../spec.md) §6。这一组最后一票：把四个新概念写进词表与逐面文档，
> 并给真终端留一份走查清单。

## 目标

- [`CONTEXT.md`](../../../CONTEXT.md) 新增**记号（Token）**一条：它是补全与呈现的单元，
  在**能兑现**时是**不可分割**的一块（chip）。
- [`docs/render.md`](../../../docs/render.md) 新增一节「输入框里的记号」，放在
  「问卷：区域与键位」之后：两个前缀的边界表、一套浮层与键位、上色判据、chip 的不变量、
  以及「区间由 `TuiState` 算好再同步进 `Input`」那条缝。
- [`docs/tui-manual-checklist.md`](../../../docs/tui-manual-checklist.md) 新增 ㉘，
  并在开头的「来源」清单补一行。
- [`.scratch/README.md`](../../README.md) 索引加 `input-tokens` 一行。
- **入口棘轮**：索引行是**新增**，`.scratch/README.md` 必然长过当前上限 —— 按
  [docs-slim 票 12](../../docs-slim/issues/12-remainder-and-close-out.md) 先例
  （「先加信息、再跟着调棘轮」的显式动作）**把它调到新实测值**，并在提交信息里写明理由。
  这是显式动作，不是偷偷上调。

## 现状（2026-10-04 核实，改前先复核）

- `CONTEXT.md` 的「渲染」节有一批钱表词条（渲染器 / 转录 / 终端端口 / 左栏 / 输入区 /
  提示符色相…），但没有任何关于**输入框里文本身份**的条目。
- `docs/render.md` 的节序是 `## 键盘` → `## 问卷：区域与键位` → `## 挂起与恢复` → …，
  新节插在问卷之后。
- `docs/tui-manual-checklist.md` 现有到 ㉗（问卷的回车），开头有「来源」清单。
- `.scratch/README.md` 的入口预算在 [`scripts/check-doc-size.py:253-257`](../../../scripts/check-doc-size.py)，
  **只许降**；增长是一次显式动作（票 12 那条先例）。

## 收尾

- `python3 scripts/check-doc-size.py` 退出 0（含更新后的棘轮）。
- `python3 scripts/check-language.py` 退出 0。
- ㉘ 的条目要能被真终端逐项走查；那一节键名密，注意中文占比余量。

## 不做什么

- 不在这一票里改任何代码。
- 不重写清单里已有的 ①–㉗。
- 不给 `Tab::Files` / `Tab::Trace` 写任何语义 —— 它们都还是「还没做」。
