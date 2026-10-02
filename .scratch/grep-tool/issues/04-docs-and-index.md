# 04 — 逐面文档与索引

Type: implement
Status: ready-for-agent
Blocked by: 01, 02, 03

> 规格：[`../spec.md`](../spec.md) §7。这一票不写代码，只把已经落地的工具写进文档索引与其
> 唯一一处逐面文档；它被前三张票 block，因为文档要写的正是它们落地后的行为。

## 目标

`grep` 这条工具有一份可核对的逐面文档，且 [README.md](../../../README.md) 的「文档」一节
（全仓库唯一的文档索引）指向它；`.scratch/README.md` 的 feature 索引反映实际进度。

## 现状（2026-10-02 核实，改前先复核）

- 逐面文档的先例与模板是 [`docs/repo-map.md`](../../../docs/repo-map.md)：一个内建工具一份
  文档，讲它的声明形状、范围、上限与「为什么是这样」。
- 索引在 [`README.md`](../../../README.md) 的「文档」一节（一张表，一行一个面）。
- `.scratch/README.md` 的 feature 表里 `grep-tool` 那一行现在写着票数占位值。
- `CONTEXT.md` **不收**工具名（它只收领域词汇，见 [`CONTEXT.md:8`](../../../CONTEXT.md)
  的「不收两类东西」）。

## 落点

`docs/grep.md`（新）、`README.md`、`.scratch/README.md`。

## 具体行为

1. **新增 `docs/grep.md`**，照 `docs/repo-map.md` 的结构写：
   - 工具声明的形状（`pattern` 必填、`glob` 可选）与那句劝阻拼 shell 的描述；
   - 范围与忽略规则（只扫会话 cwd、遵守 `.gitignore`、跳过隐藏）；
   - 输出形状与上限（`path:line:文本`、条数收尾、超限落盘 + 指针）；
   - **「为什么不 spawn `rg`」**：把调研 §4 的对照压成几段（沙箱的「整机可读」与只读工作区
     不是一回事、三条运行期依赖、输出可控性）。
2. **`README.md` 的「文档」一节加一行**指向 `docs/grep.md`。
3. **`.scratch/README.md`** 的 `grep-tool` 那行改成实际结果（票数与状态）。
4. **`CONTEXT.md` 不动**：`grep` 是工具名，不是领域概念。

## 验证

1. `python3 scripts/check-language.py` 通过（`docs/` 的中文占比有下限）。
2. `README.md` 文档表里新加那一行的链接可达（相对路径从仓库根算）。
3. **文档与实现一致**：拿实现里的实际行为逐条对一遍 —— 尤其忽略规则、条数上限、落盘路径
   这三处，是最容易写成「应该如此」而不是「就是如此」的地方。

## 不做什么

- 不重写 `docs/` 里其它任何一份文档。
- 不在这一票里改代码（发现实现与文档不一致时，改文档或另开一条 bug，不顺手改实现）。
- 不给 `grep` 建 `CONTEXT.md` 词条。
