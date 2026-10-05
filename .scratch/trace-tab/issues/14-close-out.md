# 14 — 收口：文档、脚本锚点与全量复核

Type: implement
Status: ready-for-agent
Part of: ../map.md
Blocked by: 11, 12, 13

> 规格：[`../spec.md`](../spec.md) 全篇，尤其 §3 与 §4 里「会撞的既有决定」。

## 目标

把这次改动落到逐面文档、手工清单与启动脚本上，并做一次全量复核 —— 让下一轮读文档的人看到的界面与实际一致。

## 现状（改前先复核）

- `docs/render.md` 的左栏一节（页签条、页高 = 用量字段数、两个占位页）与转录一节都还是改动前的说法。
- `docs/tui-manual-checklist.md` 的 `/` 菜单与左栏条目里，轨迹页仍被当成占位页。
- `scripts/tui-startup-check.py` 的启动帧锚点会受页高与占位页变化影响。
- `CONTEXT.md` 的**轨迹视图 / 对话视图 / 跟随**三个词条在 charting 时已写入，本票只**复核**它们与代码一致。

## 落点

`docs/render.md`、`docs/tui-manual-checklist.md`、`scripts/tui-startup-check.py`、`CONTEXT.md`（复核）。

## 具体行为

1. `docs/render.md`：左栏一节改写（页高公式、轨迹页不再是占位符）；补一节说清两个视图的分工与两处形态例外（用户消息右对齐、三类前缀各一色、轨迹页轮次底色）。
2. `docs/tui-manual-checklist.md`：加左栏轨迹页的真机条目（28 列的表格可读性、前缀分档、指示器点击、滚轮分派、轮次底色的实际观感）。
3. `scripts/tui-startup-check.py`：启动帧的锚点跟上（页高与占位页都变了），别把「轨迹页有内容」写成硬断言（启动时它可能是空的）。
4. `CONTEXT.md`：复核三个词条与代码一致。
5. 全量复核：`cargo test`、`cargo clippy`、`cargo fmt --check`、`python3 scripts/check-doc-size.py`、`python3 scripts/check-language.py`。

## 验证

1. 上面五条各自跑过并记录结果（哪一条红了、为什么）。
2. `docs/tui-manual-checklist.md` 的新条目在真终端里逐条走一遍，走不通的写成已知边界。
3. `git status` 里除了本轮的落点没有别的东西。

## 不做什么

- 不改 `docs/render.md` 与手工清单之外的文档（`docs/permissions.md` 等与本次无关）。
- 不重排手工清单的条目编号（外部有引用）。
- 不新增 CI 配置（仓库没有 CI 入口）。
