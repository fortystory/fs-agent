# 收口：文档、手工清单与基线

Type: implement
Status: done
Blocked by: 01, 02, 03

> 规格：`.scratch/todo-and-modes/spec.md` 全篇。

## 目标

这次改了两件事的性质（权限少一档、模型多一个工具、界面上多一个标签），文档不能落后：把该改的正文改到，把该删的删掉，把基线钉住。

## 落点

`README.md`、`CONTEXT.md`、`docs/render.md`、`docs/executor.md`、`docs/tui-manual-checklist.md`、`docs/adr/0003-*.md`（核对票 01 写的版本）、`.scratch/fs-agent-v1/spec.md`（§12/§13 的回改是否到位）、`.scratch/README.md`（本 feature 一行）。

## 具体行为

1. **README**：《安全模型》的模式表从四行改三行；`Shift+Tab` 的描述从「切计划模式」改成「循环三档模式」；`架构` 一节里那句「模式硬编码」的暗示要与新的入口一致；界面那一节补 `todo` 标签一段。
2. **`CONTEXT.md`**：删 **硬计划模式（Plan mode）**；**询问（Ask）** 词条里拿 `PlanConflict` 举例的地方改掉；**待办列表（Todo）** / **待办工具（`todo`）** 两条（票 02 已加）核对措辞；**模式（Mode）** 若在词表里，改成三档。
3. **`docs/render.md`**：工具那一节补 `todo`；「侧栏页签」那一节补 `todo` 的出现条件（**一旦出现过就常驻**）与「执行者的列表不上侧栏」。
4. **`docs/executor.md`**：补一句「执行者也能用 `todo`，它的列表是它自己的，不上侧栏」。
5. **`docs/plan-mode.md` 已删**（票 01）；README/其它文档里指向它的链接全部改到 ADR 0003。
6. **手工清单**：新增一节 —— `Shift+Tab` 循环三档（状态行依次 `只读`/`询问`/`自动`，且**不再有**计划弹窗）；`todo` 标签的出现与常驻；`＋N 项` 在 28 格宽度下读起来是否清楚；执行者调 `todo` 时侧栏不动。
7. **`.scratch/README.md`**：本 feature 加一行（形态 spec，票数与状态）。
8. **回改 spec**：实现期与本 spec 不一致的地方（名字、文案、阶梯数字）改回 `.scratch/todo-and-modes/spec.md`，并把四张票的 `Status:` 收成 `done`。

## 测试

- `cargo test` 全绿；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只留既有漂移。
- `cargo build` 后 `python3 scripts/tui-startup-check.py` 全绿（它的锚点只认状态行与 mark，不受本轮影响，但要跑一遍确认）。
- 全仓 markdown 相对链接扫一遍（`docs/plan-mode.md` 删了，别再有人指向它）。
