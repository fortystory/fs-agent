# 语义色板：两版对照（prototype）

对应 [语义色板的形状与配色取舍](../issues/03-prototype-semantic-palette.md)。**这是一次性素材，不是实现**；用来看清取舍，选完就折进 spec。

对着真终端看：`python3 .scratch/tui-visual-language/prototype/palette-demo.py`（`a` / `b` 只看一版）。

## 依据（先读这四条，它们不是审美偏好而是事实）

1. **`DIM` 不能承重**：主流终端至少三套实现方向（VTE/xterm 前景 ×2/3、Windows Terminal ÷2、kitty 向背景混合、xterm.js 改背景），浅色主题下可能毫无变化；且 SGR 22 会同时清掉 `BOLD` 与 `DIM`。→ **层级主要靠结构（缩进 / 线型 / 位置），`BOLD` 当配角**。
2. **发送侧没有颜色降级**：`Color::Rgb` 直发 `38;2`；命名色也走 256 色形式 —— `Gray` = `38;5;7`、`DarkGray` = `38;5;8`（ratatui 文档表的 37/90 不是线缆内容）。索引 7/8 的实际 RGB **由终端主题决定**。
3. **`CHROME_LINE` 对白底 8.86:1（太重）、对 Alacritty 默认底 2.00:1**：固定真彩深灰只配做装饰线，不能承载信息。
4. **角色五色是冻结的**（[`tui-ux/map.md`](../../tui-ux/map.md) 冻结项 9）：讨论者 1 `LightCyan` / 讨论者 2 `LightMagenta` / 执行者 `LightYellow` / 用户 `LightGreen` / 系统 `Gray`。两版都不动它。

## 现状：撞色的硬证据

`src/render/` 里 `Color::` 出现 97 处，其中 `Color::DarkGray` **38 处**。真正的问题不是"多"，是**一个色值被多个语义共用**：

| 色值 | 今天同时承担 | 种数 |
| --- | --- | --- |
| `Yellow` | 行内代码、语法数字、`Severity::Warn`、诊断、hook 反馈、新内容指示器、问卷已选、菜单未选中文字、菜单选中行底色、模态边框/正文/按钮/键名 | **11** |
| `DarkGray` | 旁白、等待提示、工具描述、详情小节/页脚/空态、面板标签、状态行、提示行、问卷页脚、未选中页签、身份行、占位页、过小提示、滚动条、回合条普通格、语法注释…… | **38 处** |
| `Cyan` | `Severity::Note`、轮次开始、问卷表头、markdown H1/H2、语法类型 | 5 |
| `LightMagenta` | 讨论者 2、`@` 记号、选中页签、回合条聚焦、标记坡道顶 | 5 |
| `LightBlue` | 注入行、`/` 记号、语法函数（`Blue`） | 3 |

**要收的第一刀已经很明确**：`Yellow` 既是「行内代码」又是「警告」。

## A 版「最小色数」

**原则**：颜色只回答两个问题 ——「要不要注意」（`WARN` / `BAD`）与「有没有被选中」（`ACCENT`）。分类信息（谁说的、什么语法、标题还是正文）交给字形、结构与位置。

| 标识符 | 值 | 用在哪 |
| --- | --- | --- |
| `PLAIN` | `Reset`（默认前景） | 转录正文、markdown 正文、问卷题面、详情正文；**`Severity::Good` 与 `Note` 也归这里** |
| `MUTED` | `DarkGray`（索引 8） | 旁白、等待提示、工具描述、状态行、提示行、未选中页签、面板标签、详情页脚与小节、滚动条、回合条普通格、markdown 引用/链接/网格 |
| `CHROME` | `Rgb(0x4a,0x4a,0x4a)` | 只做装饰线：外壳竖/横线、页签条、单位分隔、浮层边框、菜单边框、markdown 分隔线 |
| `ACCENT` | `LightMagenta`（索引 13） | 选中页签、回合条聚焦格、菜单选中行、`/` 与 `@` 记号、**语法关键字** |
| `WARN` | `Yellow`（索引 3） | `Severity::Warn`、诊断、hook 反馈、新内容指示器 |
| `BAD` | `Red`（索引 1） | `Severity::Bad`、工具失败、会话错误 |
| `SPEAKER_*` | 冻结五色 | 名字前缀、详情边框 |
| `PROMPT` | 脉冲专色（不动） | `❱` |

**被删掉的颜色**：`Cyan`、`Green`、`Magenta`、`LightBlue`、`Gray`、`Blue`，以及 `Yellow` 的 11 种用途只剩 3 种。

**内容域**：语法高亮降到两个色 + 两个修饰符 —— 关键字 = `ACCENT`、注释 = `MUTED` + `ITALIC`、其余（字符串、数字、函数、类型、标点）= `PLAIN`；行内代码 = `MUTED`（反引号本身就是标记）；markdown 标题 = `PLAIN` + `BOLD`。

**下发到终端的不同前景色值：约 10 个**（含 5 个角色色）。

**代价**：代码块明显变素，长代码里的字符串/数字/类型不再可分；`Good` 与正常文本一样，只有出问题时才有颜色。

## B 版「分域归一」

**原则**：不追求减色，追求**同一域内每个语义各归其位**。分两个域，**域间允许撞值**：

- **界面域**（chrome / 信号 / 角色）—— 域内不重复；
- **内容域**（markdown + 语法高亮）—— 自己一套。代码块内部上下文明确，`Green` 既是「成功」又是「字符串」不会被读混。

| 标识符 | 值 | 用在哪 |
| --- | --- | --- |
| `PLAIN` | `Reset` | 正文 |
| `MUTED` | `Gray`（索引 7） | 旁白、等待提示、状态行、提示行、详情页脚、未选中页签 |
| `FAINT` | `DarkGray`（索引 8） | 再退一档：面板标签、滚动条、回合条普通格、占位页 |
| `CHROME` | `Rgb(0x4a,0x4a,0x4a)` | 装饰线 |
| `ACCENT` | `LightMagenta` | 选中页签、回合条聚焦、菜单选中 |
| `MARKER` | `LightBlue` | 可点 / 记号：注入行、`/` 记号 |
| `WARN` / `BAD` / `GOOD` | `Yellow` / `Red` / `Green` | 严重度三档 + 工具失败 + 诊断 + hook |
| 内容域 | `Cyan` 标题 · `MUTED` 引用 · `Magenta` 关键字 · `Blue` 函数 · `Cyan` 类型 · `Green` 字符串 · `LightYellow` 数字 · `DarkGray+ITALIC` 注释 · `Gray` 标点 | 语法高亮全保留 |
| `SPEAKER_*` / `PROMPT` | 冻结 / 不动 | 同 A |

**相比现状只有三处实质改动**：① 行内代码不再用 `Yellow`（改 `MUTED`）；② 语法数字从 `Yellow` 改 `LightYellow`（消灭与警告撞色）；③ `DarkGray` 的 38 处拆成 `MUTED` / `FAINT` / `CHROME` 三支。

**下发到终端的不同前景色值：约 17 个。**

**代价**：颜色总数没降，只是各就各位；浅色主题下 `Gray`（索引 7）与 `DarkGray`（索引 8）都由用户主题决定，可能仍然贴背景。

## 折中路线的方向

两版真正的分歧只有一处：**内容域（代码与 markdown）要不要跟着收敛**。

- 界面域：两版差别不大（A 去掉 `GOOD` 与 `MARKER`，B 留着）；
- 内容域：A 收到 2 色，B 全保留。

「界面域走 A、内容域走 B」是一条现成的中间路线 —— 界面上的颜色只留给信号，代码块里保留语法分类。它的代价是两套规则并存，读代码的人要记住「内容色与界面色不是一个体系」。

## 待拍板

1. 走 A、走 B，还是「界面 A + 内容 B」。
2. `MUTED` 用 `DarkGray`(8)、`Gray`(7)，还是**默认前景 + `DIM`**？（最后一档的可靠性见 demo 结尾的实测。）
3. 语法高亮收敛到什么程度。
4. `Severity::Good` 要不要保留颜色（A 去掉、B 保留）。
5. 行内代码改成什么（两版都从 `Yellow` 移走，去向不同）。

## 色板模块的形状（供 spec 取用）

- 位置：`src/render/palette.rs`，与 `wording.rs` 同侪 —— 颜色值集中一处，绘制代码只引用语义名。
- 导出：以 `Color` 常量为主；需要组合的（如 `BAD` = 红 + `BOLD`）导出 `Style` 构造函数。
- 命名：扁平英文大写（`PLAIN` / `MUTED` / `CHROME` / `ACCENT` / `WARN` / `BAD`）—— 已在 `palette::` 命名空间下，再加族前缀是冗余。
- 与 `Severity` 的关系：色板提供 `fn style(self, severity: Severity) -> Style`，**语义归属仍由 `Severity::of` 一处决定**（今天 TUI 与 plain 各写一张表、没有任何绑定；收敛到共用语义表，但不动 plain 的输出）。
