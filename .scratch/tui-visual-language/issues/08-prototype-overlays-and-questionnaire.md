# 覆盖层与问卷

Type: prototype
Status: resolved
Part of: ../map.md
Blocked by: 03, 04, 05

## 问题

维护者点名「覆盖层 / 详情页 / 问卷」（[图](../map.md) 冻结项 7）。这些表面今天各有各的语法：

- **模态（权限 / 问题覆盖层）**：主列居中，边框与正文、按钮、键名**整块 `Yellow`**（`src/render/tui.rs:4020-4055`、`4111-4114`），框形用 ratatui `LIGHT_TRIPLE_DASHED`。
- **`/` 与 `@` 菜单**：锚在光标上方，边框 `CHROME_LINE`（`4774-4780`），选中行反色 `Black on Yellow`（`4809-4820`），未选中行 `Yellow`。
- **详情覆盖层**：屏幕居中、135 列上限，**边框取该行发言者的颜色**（`5936-5942`），小节标题与页脚 `DarkGray`（`5822-5827`、`5960-5966`），空态 `DarkGray`，正文无色。
- **问卷**：接管底部输入区，无边框。表头 `Cyan` + `BOLD`（`3548-3553`），已选 `Yellow` + `BOLD`（`3580-3583`），当前高亮在选项区 `REVERSED`、在输入区 `DIM`（`3587-3594`），自定义行标签随焦点在 `DarkGray` / `BOLD` 之间跳（`3610-3614`），页脚 `DarkGray`。

要定：

- 四种浮层表面各用什么**框架语法**：三套框色（`Yellow` / `CHROME_LINE` / 发言者色）是该统一、还是各有理由？它们与外壳那套 `┄`/`┆` 是同一档吗？（框形归 [字形语法](04-prototype-glyph-grammar.md)，本票定归属与例外。）
- 「**焦点在哪**」在问卷里的视觉表达：`REVERSED` 与 `DIM` 两种并存，在 [终端能力边界](01-research-terminal-capability-bounds.md) 的结论下哪个可用？
- 详情页内部的小节结构（小节线、标题、空态）在层级语言下的形态；发言者色框要不要保留（它今天把「这条详情是谁的」编码进了框色）。
- 模态整块 `Yellow` 是否过重（它同时是边框、正文、按钮与键名）。

## 产物

`.scratch/tui-visual-language/prototype/overlays-and-questionnaire.md`：每个表面的帧草图（可复用 TestBackend 快照）+ 决定 + 明确的例外清单（哪个表面为什么不跟大流）。

## 接受的边界

- 问卷的键位与「选项区 / 输入区」语义是 [`questionnaire-keys/spec.md`](../questionnaire-keys/spec.md) 与 [ADR 0010](../../docs/adr/0010-questionnaire-keys-dispatch-by-zone.md) 的契约，**只改视觉表达**；
- 浮层的几何基准（模态主列居中 vs 详情屏幕居中）是 [`tui-chrome/spec.md`](../tui-chrome/spec.md) 写下来的决定，不重开；
- 覆盖层立着时滚轮按指针位置分派是既有交互，不动。

## 已查明的硬约束（来自 [终端能力边界](01-research-terminal-capability-bounds.md)，2026-10-05）

- **问卷「输入区聚焦」今天用 `DIM`**（`src/render/tui.rs:3587-3594`）：`DIM` 在主流终端至少有三套实现方向（变暗前景 / 向背景混合 / 改背景不透明度），浅色主题下可能**毫无变化**；同一表面里已有的 `REVERSED` 兑现面更广（8 色终端也兑现）。本票要在「焦点在哪」上二选一或另找表达。
- **浮层的 `border::LIGHT_TRIPLE_DASHED` 只换了横竖，四角与 T 形交叉仍是实线**；它与外壳手画的 `┆`/`┄` 在横竖上本就是同一码位 —— 要让两者同档，要处理的是**角**。
- **`/`、`@` 菜单的边框用 `CHROME_LINE`**（`Rgb(0x4a,0x4a,0x4a)`）：发送侧无降级、直接发 `38;2`，且该值对浅色主题偏重（对白底 8.86:1）。

## 作答

**已解决（2026-10-05，HITL：维护者看完四个表面的对照后拍板）**。产物：[`prototype/overlays-and-questionnaire.md`](../prototype/overlays-and-questionnaire.md) 与 `prototype/overlays-demo.py`。

### 决定

1. **模态拆成三件事**：边框 `CHROME` + **空角**、正文 `PLAIN`、按钮行 `ACCENT`（键名 `BOLD`）。
   依据：`Clear` + 主列居中已经让模态「跳出来」了，框色不必再承担一次；而**黄要留给诊断、hook 反馈与 `Severity::Warn`** —— 一个阻塞式询问不是警告。
2. **菜单去黄**：未选中行 `PLAIN`，光标行 `REVERSED`。菜单是临时的选择器，不值得给它一个专属颜色；光标行要表达的是「回车会选它」，正是 05 给**临时光标**定的手段。
3. **详情的边框归 `CHROME` + 空角，发言者色退到标题行**。这**推翻 2026-09-23 的一个决定**（`tui.rs:5934-5940`：「边框穿发言者的颜色……应该在你读它一个字之前就看得出来」）—— 但那条的**意图没丢**：标题行本来就在框内第一行、紧挨边框。换来的是四个表面共用一套框架语法。
4. **问卷：全屏唯一的 `REVERSED` 就是键盘所在**。选项区聚焦 → 当前选项行反显；输入区聚焦 → 自定义行反显。**不再有 `DIM`**，也不再有两处同时表达焦点。
   - 依据是问卷代码注释里那句本意：「屏幕上于是只有一个焦点」——同一个问题不该有两个答案。
   - 问卷**已选去黄**、表头进内容档（`PLAIN` + `BOLD`）、页脚 `MUTED`：这三条是 05 定的，本票只执行。

### 四个表面统一后的框架语法

| 表面 | 框架 | 焦点 / 强调 |
| --- | --- | --- |
| 模态 | `CHROME` 虚线 + **空角** | 按钮行 `ACCENT`，键名 `BOLD` |
| `/` `@` 菜单 | `CHROME` 虚线 + **空角** | 光标行 `REVERSED` |
| 详情 | `CHROME` 虚线 + **空角** | 标题行 = 发言者色 + `BOLD`；页脚 `MUTED` |
| 问卷 | 无边框（接管输入区） | 唯一的 `REVERSED` = 键盘所在 |

**「三套框色」的答案就是这一条**：框色只剩 `CHROME` 一个（问卷没有框）；发言者色与 `Yellow` 都从框架上退场，前者变成标题行的信息、后者回到信号档。

### 给下游 / spec 的落点

- **`tui.rs`**：`draw_modal`（`4024` / `4038` / `4053` / `4112-4114` 四处 `Yellow` 全拆）、`menu_row`（`4809-4816`）、`draw_detail`（`5940` 的 `view.detail.color` 移到标题行 `5947`）、问卷的 `options_focused` 分支（`3587-3594` 的 `DIM` 与 `3610-3614` 的跳色）。
- **`docs/render.md`**：`/` 菜单、模态、详情、问卷四节，以及 `docs/tui-manual-checklist.md` 的 ②/⑪/⑫/㉕ 四项观感。
- **测试**：`tests/render_layout.rs` 里菜单反色、问卷高亮、详情边框色的断言；`tests/ask_user_question_tui.rs`；`tests/render_tui.rs` 的详情边框断言（`render_layout.rs:5902`）会动。
- **[09 间距与对齐](09-grilling-spacing-and-alignment.md)**：按钮之间那 3 个空格、菜单内边距 1 格、`centred_inset` 两处手算居中 —— 都留给那张票。

### 未证实

模态在**深色终端**下失去黄色后，`Clear` + 居中是否仍足够「跳出来」，要在手工清单 ② 与 ⑫ 留实测记录；如果不够，回退的空间是**给模态一个 `ACCENT` 边框**（而不是把黄请回来）。
