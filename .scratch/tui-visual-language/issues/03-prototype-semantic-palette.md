# 语义色板的形状与配色取舍

Type: prototype
Status: resolved
Part of: ../map.md
Blocked by: 01

## 问题

[图](../map.md) 冻结项 2 已经定了「立一个语义色板模块」，但**色板本身长什么样**还是设计问题，而且它是这次 effort 的地基 —— 后面 05（层级）、06/07/08（三组形态票）都长在它上面。

要定：

- **形状**：模块放哪（`src/render/palette.rs`？还是并进 `wording.rs` 的同侪）？导出的是 `Color` 常量、`Style`、还是一个 `fn 语义() -> Style`？命名用扁平的英文常量（`MUTED` / `CHROME` / `WARN`）还是分族（`chrome::LINE` / `severity::WARN`）？后者更长但更好找。**标识符用英文**（[ADR 0004](../../../docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md)）。
- **粒度**：语义表怎么切？charting 期的穷举给出了这些族 —— 框架线、静音/次要文字、名字（五个角色）、选中态、警示与诊断、严重度四档、markdown（行内代码/引用/网格/标题）、语法高亮七类、浮层、问卷、输入区记号、在跑信号。哪些合、哪些分？
- **收敛到什么程度**：今天屏幕上有多少种不同的前景色？收敛之后目标是多少？（冻结项 3：颜色只留给需要预警与分类的语义，层级交给 `dim`/`bold` 与留白。）**这个数字本身就是本票要拍的。**
- **哪些不能动**：[`tui-ux/map.md`](../../tui-ux/map.md) 冻结项 9 的角色五色是写下来的决定；`docs/render.md` 与 `docs/tui-manual-checklist.md` 里有没有钉死的色值（02 号票会给出清单）。
- **与 plain 的关系**：`Severity::ansi`（服务 plain / headless）与 TUI 那份映射要不要收敛到同一个源？这决定色板是 TUI 私有还是渲染层共用。**注意**：收敛「源」不等于改 plain 的输出 —— 输出不变是硬约束。

## 产物

`.scratch/tui-visual-language/prototype/` 下的**配色表**（语义 → 色 / 修饰符 → 一句话理由），加**至少两版对照**（例如「最小色数版」与「保守重排版」），并说明各自在 120×24 与 80×24 下读起来差在哪。形态是文档 + 表，**不必写渲染器代码**；但两版要能对着真终端看。

## 接受的边界

- **不引入用户可配的主题**（[`tui-ux/map.md`](../../tui-ux/map.md) 明确不做第 3 条仍在）；
- 色板是**代码里的语义层**，不是运行期配置；
- 色值取 ratatui 命名色或 `Rgb` 均可，但必须落在 [终端能力边界](01-research-terminal-capability-bounds.md) 的结论范围内（哪个色在亮背景不可用、`DIM` 可不可靠，用它当依据）。

## 已查明的硬约束（来自 01 与 02，2026-10-05）

- **色板改动的测试噪音比想象的小**：89 处断言站点里只有 9 处钉色值（提示符静止 RGB、退场色环清单、diff ANSI、严重度 ANSI），其余 80 处钉的是语义、换成色板常量即可。清单见 [样式改动面](02-research-style-change-surface.md) 的 `## 作答` 与报告 §1.6。
- **`render_block` / `render_block_uncoloured` 没有生产调用方**（真正上屏的是私有 `paint_block`），色板可以从绘制路径直接下手，不必改这两个包装的形状；但 `tests/render_tui.rs` 的 20 个调用点会受影响。
- **严重度的两套映射没有任何直接绑定**（只有「两边都调 `Severity::of`」这层间接），且 TUI 的 `Bad` 比 plain 多一个 `BOLD`。收敛成「一个语义条目、两个画家各自着色」是现成落点 —— **但 `Severity::ansi` 是 plain 可见输出的一部分**，[图](../map.md) 明确不做它，收敛只到「共用语义表」为止。
- **`docs/render.md` 里有几处写死了颜色名**（三类前缀色、logo 的色相坡道、`CHROME_LINE`），色板落地后这些句子要跟着改；清单见 [样式改动面](02-research-style-change-surface.md) 报告 §3.1。

## 作答

**已解决（2026-10-05，HITL：维护者看完两版对照后拍板）**。产物：

- 配色表与两版取舍：[`prototype/palette.md`](../prototype/palette.md)
- 可跑的真 ANSI 对照：`python3 .scratch/tui-visual-language/prototype/palette-demo.py`（`a` / `b` 只看一版；结尾有一段 `DIM` 实测）

### 决定

1. **主路线 = 界面 A + 内容 B**：界面域按 A 版（颜色只回答「要不要注意」与「有没有被选中」），内容域按 B 版（markdown 与语法高亮保留自己的分类色）。**理由**：代码块内部的颜色是帮读的**分类**，界面上的颜色是**信号**，两者不是一回事。**代价**（原样写进 spec）：两套规则并存，读代码的人要记住「内容色与界面色不是一个体系」。
2. **静音只有一档，取 `DarkGray`（索引 8）**，不再细分。**代价**：它的实际 RGB 由终端主题决定，浅色主题下可能贴背景 —— 这条也原样写进 spec 的已知取舍。
3. **内容域允许与界面域撞值**：`Green` 既是语法字符串又不再是界面信号、`Cyan` 既是语法类型也是标题，都不会被读混（位置与上下文分开了）。所以内容域**不参与**界面域的减色。

### 界面域色板（定稿）

| 标识符 | 值 | 用在哪 |
| --- | --- | --- |
| `PLAIN` | `Reset`（默认前景） | 正文、问卷题面、详情正文，**以及 `Severity::Good` 与 `Note`** |
| `MUTED` | `DarkGray`（索引 8） | 旁白、等待提示、工具描述、状态行、提示行、未选中页签、面板标签、详情小节与页脚、滚动条、回合条普通格、占位页、过小提示、问卷页脚 |
| `CHROME` | `Rgb(0x4a,0x4a,0x4a)` | 只做装饰线：外壳竖/横线、页签条、单位分隔、浮层边框、菜单边框 |
| `ACCENT` | `LightMagenta`（索引 13） | 选中页签、回合条聚焦格、菜单选中行、`/` 与 `@` 记号 —— 共同语义是「**被系统认出来的 / 当前聚焦的**」 |
| `WARN` | `Yellow`（索引 3） | `Severity::Warn`、诊断、hook 反馈、新内容指示器 |
| `BAD` | `Red`（索引 1） | `Severity::Bad`、工具失败、会话错误 |
| `SPEAKER_*` | 冻结五色 | 名字前缀、详情边框（[`tui-ux/map.md`](../../tui-ux/map.md) 冻结项 9，不动） |
| `PROMPT` | 脉冲专色 | `❱`（不动） |

**收掉的**：`LightBlue`、`Green`、`Cyan`、`Magenta`、`Gray`（界面内），以及 `Yellow` 今天 11 种用途里的 8 种。`Good` 与 `Note` 不再有颜色 —— 正常完成只靠文字判断。

### 内容域色板（定稿）

- **markdown**：标题 = `Cyan` + `BOLD`；行内代码 = **`MUTED`**（← 从 `Yellow` 移走，这是第一刀）；引用 / 链接 / 网格 / 分隔线 = `MUTED`；正文 = `PLAIN`。
- **语法高亮**：关键字 `Magenta`、函数 `Blue`、类型 `Cyan`、字符串 `Green`、数字 **`LightYellow`**（← 从 `Yellow` 移走）、注释 `MUTED` + `ITALIC`、标点 `Gray`、普通标识符 = `PLAIN`。
- 内容域的次要灰统一复用界面的 `MUTED`，不再单立两个灰值。

### 色板模块的形状（供 `/to-spec` 取用）

- 位置 `src/render/palette.rs`，与 `wording.rs` 同侪；导出以 `Color` 常量为主，组合（如 `BAD` = 红 + `BOLD`）导出 `Style` 构造函数。
- 命名扁平英文大写 —— 已在 `palette::` 命名空间下，再加族前缀是冗余。
- `Severity` 的**语义归属仍只有 `Severity::of` 一处**；色板提供 TUI 用的 `fn style(severity) -> Style`，而 plain 的 `Severity::ansi` **输出不动**（[图](../map.md) 明确不做）。

### 给下游票的落点

- **[层级语言](05-grilling-hierarchy-and-selection.md)**：静音只有一档，手段只剩四种 —— `PLAIN` / `MUTED` / `BOLD` / 结构（缩进、线型、位置）。`Good` 与 `Note` 现在没有颜色，那张票要确认「正常完成」是否只靠文字就够，或给它一个**非颜色**的标记。
- **06 / 07 / 08 三张形态票**：直接引用 `palette::` 的常量；占比条、浮层框、问卷高亮的取值都以本票为准。
- **`/to-spec`**：`docs/render.md` 里写死颜色名的几处要跟着改。

### 留给实现期

- 内容域新增的 `LightYellow`（索引 11）与界面域 `WARN` 的 `Yellow`（索引 3）在 16 色终端下是否仍可区分，实现时实测一次。
- 浅色主题下的实际观感要在手工清单 **⑳.4**（`CHROME_LINE` 深度）与 **③.8**（角色配色）留实测记录。

### 补记（2026-10-05，[层级语言与选中态](05-grilling-hierarchy-and-selection.md) 关闭后回改）

两处对本票色板的修正，**以本节的版本为准**：

1. **`Severity::Good` 与 `Note` 改归 `MUTED`**（原案是 `PLAIN`）：它们是过程行，正常完成不该抢注意力。`Warn` / `Bad` 保持颜色不变。
2. **界面域新增一个条目 `INJECTED = LightBlue`（索引 12）**，只给上下文注入行。理由不是审美，是 [`trace-tab/spec.md`](../../trace-tab/spec.md) §2 例外二明写的契约：「注入行与别的叙述行同灰，于是注入 / 用户 / 助手在轨迹页上分不开」。**界面域因此是 7 个条目**，不是 6 个。
