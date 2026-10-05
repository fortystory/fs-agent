# 终端能力边界（dim / 虚线字形 / 真彩色 / 亮背景）

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

本 effort 要把 TUI 的样式收进「语义色板 + 字形语法」两层（[图](../map.md) 冻结项 2、4），而这两层都押在「终端实际能兑现什么」上。特别是冻结项 3 那条**收敛**策略 —— 颜色只留给需要预警与分类的语义、层级交给 `dim`/`bold` 与留白 —— 等于把大量层级压到 `Modifier::DIM` 这一个属性上。它要是在常见终端里不可靠，整条策略就得换，所以这条必须先查、且要查得能当依据。

四件事：

1. **`Modifier::DIM`（SGR 2）的真实行为**。主流终端各是什么表现：iTerm2、WezTerm、kitty、Alacritty、VTE / GNOME Terminal、Windows Terminal、tmux 内、VS Code 集成终端。它是真变暗、被静默忽略、还是被当成别的属性？16 色 / 256 色 / truecolor 三种模式下有区别吗？ratatui / crossterm 有没有为它做降级或特别处理？
2. **字形宽度与显示**：`┄`(U+2504)、`┆`(U+2506)、`─`(U+2500)、`│`(U+2502)，以及 ratatui `border::LIGHT_TRIPLE_DASHED` 实际用的字符集。它们在等宽字体里是否都占 1 列？有没有常见终端字体缺这些码位、或渲染成双宽的情况？（今天的框架线就是这几个字符，字形票 04 要在此基础上统一。）
3. **颜色降级**：`Color::Rgb` 在非 truecolor 终端里 crossterm / ratatui 怎么处理 —— 真降级还是直接指望终端？`DarkGray` 与 `Gray` 在 16 色与 256 色下各落到什么索引、两者亮度差多少？（今天的 `DarkGray` 一人扛 32 处语义，收敛后要靠它和 `DIM` 分工。）
4. **亮背景（浅色主题）终端**：`CHROME_LINE = Rgb(0x4a,0x4a,0x4a)`（`src/render/tui.rs:218`）这类固定深灰、以及 `DarkGray` 系，在浅色主题下是否到看不清的程度？在**不引入用户可配主题系统**的前提下（这是 [`tui-ux/map.md`](../../tui-ux/map.md) 明确不做的第 3 条），有哪些低成本做法（例如只用 `DIM` + 默认前景色）？

## 产物

一份中文调研报告，落在 `.scratch/tui-visual-language/research/01-terminal-capability-bounds.md`：

- 逐条结论 + 依据（URL 链接；代码相关的给路径或文档位置）；
- **「在主流终端上不可靠的做法」清单**，每条标出它影响哪张票（[色板](03-prototype-semantic-palette.md) / [字形](04-prototype-glyph-grammar.md) / [层级](05-grilling-hierarchy-and-selection.md) / 06–08 形态票）；
- **「没能证实 / 存疑」**单列一节。

每条证据标强度档：**文档明确 / 社区报告 / 未证实**。只写有来源的结论，不凭印象断言。

## 接受的边界

纯调研，不改代码、不改文档。结论供 03、04、05 三张票取用；它本身不替它们做决定。

## 作答

**已解决（2026-10-05，AFK：research 子代理跑完）**。报告：[`research/01-terminal-capability-bounds.md`](../research/01-terminal-capability-bounds.md)（253 行，逐条标证据强度：**文档明确 / 社区报告 / 未证实**）。

四条改变下游决定的结论：

1. **`Modifier::DIM` 没有统一语义，方向在不同终端甚至相反。** VTE / xterm 把前景通道 ×2/3（VTE 维护者明确：黑字白底下的默认黑前景**不会**变淡）、Windows Terminal 把前景 ÷2、kitty 向背景混合、xterm.js（VS Code 集成终端内核）把 dim 做在**背景**上。ratatui / crossterm 不做能力探测也不降级，原样发 `\x1b[2m`。**而且 SGR 22 会同时清掉 bold 与 dim** —— 两级不能靠 `BOLD` / `DIM` 并存。→ 05 若把「次级」整层压在 `DIM` 上就是在赌终端；层级必须主要落在**留白、缩进、线型**上，`BOLD` 只能当配角。08 的问卷「输入区聚焦」今天用 `DIM`（`src/render/tui.rs:3587-3594`），`REVERSED` 的兑现面更广。
2. **04 的疑点已答。** 外壳手画的 `┆`/`┄`（`tui.rs:3736-3755`）与三处浮层的 `border::LIGHT_TRIPLE_DASHED`（`4023` / `4777` / `5939`）在横竖上**本就是同一个码位**（`ratatui-core/src/symbols/line.rs:6,16,144-148`）；差别只在浮层还带四个**实线角**与 T 形交叉 —— ratatui 没有 triple-dash 的角。要让两者「同一档」不需要换字符，要决定的是**角与交叉**要不要自己补虚化。
3. **发送侧没有颜色降级，且 ratatui 文档表与线缆字节不符。** crossterm 直发 `38;2;r;g;b`，**连命名色也走 256 色形式** —— `Color::Gray` 是 `38;5;7`、`Color::DarkGray` 是 `38;5;8`（文档表写的 37 / 90 不是线缆内容）。索引 7 / 8 的实际 RGB 由终端主题决定（Alacritty 默认 `#d8d8d8` / `#6b6b6b`，对比仅 3.74:1）。→ 03 的配色表不能拿 ratatui 文档表当依据；`DarkGray` 不适合当「中灰」承重（05 / 07 的「占比条对比过低」同源）。
4. **亮背景的假设方向反了。** `CHROME_LINE`（`#4a4a4a`）对白底是 **8.86:1** —— 不是看不清，是**太黑太重**；对 Alacritty 默认底色 `#181818` 才是 2.00:1 的边界地带。真正脆的是 `DIM` 与 `DarkGray` 系。不引主题系统的低成本做法（按证据强弱）：**留白 / 缩进 / 线型承层级** → `BOLD` + 默认前景 → 用 `Color::Reset`(39) 而非 Gray → 固定深灰只做装饰线。

**字形宽度**：`┄ ┆ ─ │` 在 UCD 16.0 里**全是 Ambiguous（A）**（本机 `unicodedata` 实测），「等宽字体里必然占 1 列」不是 Unicode 保证；存在 ambiguous=wide 开关（WezTerm）与错位报告（kitty #6560）。字体反例：本机 **Liberation Mono 有 U+2500、没有 U+2504 / U+2506**；但 Alacritty 与 xterm.js 默认自绘 U+2500–U+259F，风险被终端兜住一部分。

**未证实（8 条）**：iTerm2 的 SGR 2 兑现、tmux 的转发与降级、终端收到 `38;2` 而不支持时的行为、DIM 是否与色深相关、未安装字体的码位覆盖、VS Code 具体用哪个 xterm.js renderer、WezTerm 的权威答案、16 色终端是否一定认 `38;5`。逐条在报告第 6 节。

**给 `/to-spec` 的落点**：冻结项 3（层级交给 `dim`/`bold` 与留白）按第 1 条**收紧** —— 承重的是留白与线型，颜色与 `DIM` 只做点缀；色板（03）取值以第 3 条为准；字形（04）按第 2 条处理角与交叉；形态票（06 / 07 / 08）碰 `DarkGray` 与 `DIM` 时都以本票为准。
