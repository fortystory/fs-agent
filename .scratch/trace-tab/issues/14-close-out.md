# 14 — 收口：文档、脚本锚点与全量复核

Type: implement
Status: done
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

## 作答（2026-10-05）

- **`docs/render.md`**：左栏一节改写成页高公式（**页区 = 内容行 − 身份 − 页签条**，身份先让位、
  页区保住 3 行地板，`SIDEBAR_FIELDS` 退休）；页签条一节改成「`轨迹` 是转录的第二个视图、
  只有 `文件` 还是占位」；新增 `## 两个视图` 一节，写清分工、两个 pane 各持什么、滚轮与键盘的
  分派、降级、以及两处形态例外（用户消息右对齐、三类前缀各一色）与轮次底色。左栏那一节
  （页签条、页高公式、两个视图的例外、轨迹页的排版与交互）。数字与 `layout.rs` / `tui.rs` 逐条
  对过。
- **`docs/tui-manual-checklist.md`**：新增 `㉙ 轨迹视图：全量块、滚轮与轮次底色`（10 条：分工、
  28 列的表格、前缀分档、消息首行 + `…`、指示器、滚轮分派、轮次底色与 `NO_COLOR`、降级、
  切页与详情、三种终端各走一遍），条目编号接在 ㉘ 之后，既有编号一个没动。
- **`scripts/tui-startup-check.py`**：锚点**不需要改** —— 它只断身份在场、虚线数量与终端交还，
  从不断某一页上写着什么；在这里加了一段注释说明页高解耦与轨迹页都不影响它，并明确
  「轨迹页有内容」不该写成硬断言（启动时它通常是空的）。实测 `0/15 red`。
- **`CONTEXT.md`**：`对话视图` 词条补全成实际的保留清单（错误、会话中断、权限裁决、失败的
  hook、`Notice` 整类、诊断）加回合 / 轮次边界行，并写上「用户的话在这里右对齐」；
  `轨迹视图` / `跟随` / `转录` 三个词条与代码逐条对过，一致。
- **`.scratch/README.md`**：`trace-tab` 那一行的票数从 `6 resolved + 8 ready-for-agent` 收到
  `6 resolved + 8 done`。
- **全量复核（2026-10-05）**：`cargo test` 全绿（63 个测试二进制、0 failed）、
  `cargo clippy --all-targets` 无警告、`cargo fmt --check` 无漂移、
  `python3 scripts/check-doc-size.py` OK（37 份、入口三份在预算内）、
  `python3 scripts/check-language.py` OK、`python3 scripts/tui-startup-check.py` **0/15 red**。
- **没有做真终端走查**：㉙ 的 10 条只有人答得了，这里如实留给人。已知边界两条：
  **28 列下 Markdown 表格的真实观感**（spec §3 明写「未证实」，退路是窄档退纯文本）与
  **256 色两块深灰在真实终端/主题下的可分辨度**（色值是「起步」，实现期没有真终端可对照）。
- `git status` 里除本轮的落点（`docs/render.md`、`docs/tui-manual-checklist.md`、
  `scripts/tui-startup-check.py`、`CONTEXT.md`、`.scratch/README.md` 与本 feature 目录）没有别的
  东西。
