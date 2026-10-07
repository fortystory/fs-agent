# 样式改动的测试与文档契约面

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

把样式收进色板、统一字形，会同时掀动一批**逐格断言**与**文档契约**。动手前要清点到「哪一条断言会因为这次改动而红、它是该改还是该留」这个粒度，否则 `/to-spec` 拆出的实现票会各自撞一遍测试才发现。

四件事：

1. **测试里钉住颜色与修饰符的断言**。扫 `tests/`（重点 `render_layout.rs`、`render_tui.rs`、`render_markdown.rs`、`render_highlight.rs`、`ask_user_question_tui.rs`、`history_replay.rs`、`wording.rs`、`todo.rs`，别的若有也收）。每处记：`文件:行号`、断言的是什么、以及它属于哪一类 —— **「钉语义」**（改色板时可改成引用色板常量，断言意图不变）还是**「钉色值本身」**（改了色板就必须改这条断言）。要一张表 + 按文件与类别的总数。
2. **公开契约**：`src/render/mod.rs` 导出的 `render_block` / `render_block_uncoloured` 有哪些调用方（含 `tests/`）；`Severity::ansi`（`src/render/severity.rs`）服务哪些路径，它与 TUI 里那份严重度映射（`src/render/tui.rs` 里 `severity_style` 附近）今天有没有任何一处显式绑定 —— 两套映射各写各的，是收敛的候选；`markdown::to_lines_indented` 的列预算契约。
3. **文档与脚本锚点**：`docs/render.md` 里哪些段落描述了具体颜色或字形（列小节标题与行号）；`docs/tui-manual-checklist.md` 的 ①–㉘ 里哪些是纯观感项；`scripts/tui-startup-check.py` 钉了哪些字形或颜色锚点。
4. **`docs/render.md` 与代码的矛盾复核**。charting 期已核出 6 处，逐条复核真假、给准确行号、写清正确的事实该是什么。已知的一处：文档仍写着 TUI 是「alt screen 上的一圈外框」，而外框在 [`tui-chrome/spec.md`](../../tui-chrome/spec.md) 已经拆掉。另有一类是注释与实现相左（如 `src/render/tui.rs:15` 说「循环里唯一的定时器是标记的脉冲」，同文件 `385-388` 与 `474-478` 却还有退出手势的 deadline）。

## 产物

一份中文报告，落在 `.scratch/tui-visual-language/research/02-style-change-surface.md`：表格 + 清单，多给 `文件:行号`。它同时是 08 号形态票与后续实现票的输入。

## 接受的边界

纯清点，不改测试、不改文档、不改代码。已退场的死代码（`PULSE_PALETTE`、`DASH_*`、`highlight::DiffTag`、`panel.rs` 不可达的降级路径）也在清点范围内 —— 它们各自有测试钉着，属于「清理时哪些断言会一起走」的一部分。

## 作答

**已解决（2026-10-05，AFK：research 子代理跑完，清点基线 `HEAD = cbfc2e6`）**。报告：[`research/02-style-change-surface.md`](../research/02-style-change-surface.md)（379 行）。

四条结论：

1. **测试面比想象的大，但只有 9 处真的钉色值。** 全仓 **89 处**颜色 / 修饰符断言站点（票面口径：钉语义 80 / 钉色值 9；机械口径则是 36 / 53）。按文件：`render_layout.rs` 42、`render_markdown.rs` 20、`render_tui.rs` 17、`wording.rs` 3、`render_highlight.rs` 3、`render_plain.rs` 3、`render_editor.rs` 1。**9 处色值**是：提示符静止 RGB（`render_layout.rs:870`）、退场色环清单（`1021-1034`）、diff ANSI（`render_highlight.rs:61-63`）、严重度 ANSI（`render_plain.rs:391/395/399`）。
2. **两个「公开」渲染函数没有生产调用方。** `render_block` / `render_block_uncoloured`（再导出在 `mod.rs:59`）今天只被 `tests/render_tui.rs` 的 20 个调用点使用；真正上屏的是私有的 `paint_block`（`tui.rs:1891` 对话 / `1904` 轨迹）。**色板改造从 `paint_block` 下手即可，不必改这两个包装的形状** —— 但那 20 个调用点会跟着受影响。
3. **严重度的两套映射今天没有任何直接绑定**，只有一处间接（两边都调 `Severity::of`）。而且已经漂移了一处：TUI 的 `Bad` 多一个 `Modifier::BOLD`（`tui.rs:5334-5336`），plain 的只有红色。这是色板收敛的现成落点 —— **但 `Severity::ansi` 是 plain 可见输出的一部分**（`tests/render_plain.rs:391-399`），收敛只能到「共用语义表」，不动 plain 的输出。
4. **`scripts/tui-startup-check.py` 一个颜色锚点都没有**（屏幕仿真在 L200 把 SGR 整段剥掉），所以**改色板不会让它红**；它钉的是字形与数量：`MARK_ROW = "▄▀▀█"`（L76、用于 L738）、`BORDER_H = "┄"`、`BORDER_V = "┆"`（L81-82、L740-746，判据各 ≥3）。**换字形或改虚线密度会直接红在这里** —— 这是 04 号票的硬约束。

**文档矛盾复核**：6 处落点、归 **4 类漂移**，**全部判真**（外框：`docs/render.md:15` 与 `layout.rs:7`；定时器数量：`tui.rs:15` 与 `docs/render.md:186-192`；`Tab::Trace` 注释仍写「还没做」；`docs/render.md:142` 把两条横线写成一条）。另给 **4 处「需澄清」候选**（`indent` 被写成普遍约定、问卷提示四档写成三档、对话视图未提系统消息、todo 带 id 的项）。

**修正两处 charting 期的事实**：

- 真终端手工清单实际是 **①–㉙**（㉙ 轨迹视图是后加的），不是 ①–㉘；纯观感重点节是 **③.8 / ⑩ / ⑮ / ⑯ / ⑳ / ㉑ / ㉘.4 / ㉙**，其中 ⑳.4（`CHROME_LINE` 深度）与 ③.8（角色配色）**随终端配色而变**，任何色值收敛都要在这两处留实测记录。
- `Color::DarkGray` 在 `src/render/` 是 **38 处**（`grep -oE '(ratatui::style::)?Color::DarkGray'`；多口径可到 42），不是 charting 时记的 32 处。

**给 `/to-spec` 的落点**：实现票拆到「改哪一条断言」的粒度时直接用报告 §1 的表；清理退场死代码时 §1.7 列了各自钉着的断言；改字形前先看 §3.3。
