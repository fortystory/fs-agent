# 01 · 终端能力边界：DIM、制表符字形、颜色降级、亮背景

调研日期 2026-10-05，来源是 [`issues/01-research-terminal-capability-bounds.md`](../issues/01-research-terminal-capability-bounds.md)。票文件在本文写作期间落盘，写完已按实际票名核对影响面：**03 语义色板 / 04 字形语法 / 05 层级与选中 / 06 底部三条 / 07 转录右缘与左栏 / 08 覆盖层与问卷**。

**证据强度只有三档**，全文逐条标注：

- **文档明确** —— 项目自己的官方文档、源码、契约文本（源码属实但文档没有的，写「文档明确（源码）」）。
- **社区报告** —— 项目 issue / PR / discussion 里的说法，含维护者发言；不是规范。
- **未证实** —— 没找到一手来源，或只找到二手转述。

**方法**：本机 cargo registry 里的源码（ratatui 0.30.2 / ratatui-core 0.1.2 / ratatui-crossterm 0.1.2 / crossterm 0.29.0）逐条读；本机 fontconfig 与 Python `unicodedata`（UCD 16.0）实测；网络只取一手页面。凡是「未证实」都原样写出来，不补印象。

---

## 0. 结论速览

| # | 问题 | 结论 |
| --- | --- | --- |
| 1 | `Modifier::DIM` 是「变淡」吗 | **不是统一语义**。VTE/xterm 把前景通道 ×2/3；Windows Terminal 把前景 ÷2；kitty 向背景混合；xterm.js 把**背景**降不透明度。方向在不同终端甚至相反 |
| 2 | DIM 在 16/256/truecolor 下有区别吗 | 没有找到「按色深分支」的证据；各实现都作用在最终 RGB 上（**未证实**存在差异） |
| 3 | ratatui/crossterm 会为 DIM 降级吗 | 不会。`Modifier::DIM` 原样发 `\x1b[2m`（**文档明确（源码）**） |
| 4 | `┄ ┆ ─ │` 的宽度 | UCD 16.0 里四个**全是 Ambiguous（A）**；「等宽字体里必然 1 列」不成立（**文档明确（UCD）**） |
| 5 | ratatui `LIGHT_TRIPLE_DASHED` 用什么 | 只换了横竖：`┄`(U+2504) 与 `┆`(U+2506)；角与交叉沿用实线 `┌┐└┘├┤┬┴┼`（**文档明确（源码）**） |
| 6 | 常见等宽字体都有这些字形吗 | 不一定。本机实测 **Liberation Mono 有 U+2500、没有 U+2504/U+2506**；但 Alacritty 与 xterm.js 默认**自绘** U+2500–U+259F（**文档明确**） |
| 7 | `Color::Rgb` 在非 truecolor 终端会降级吗 | crossterm 与 ratatui 都**不做**能力探测，直接发 `38;2;r;g;b`；降级与否完全看终端（**文档明确（源码）**）。终端不支持时的行为无规范（**未证实**） |
| 8 | `DarkGray` / `Gray` 落到哪 | 文档表格说 90 / 37，**线缆上实际是 `38;5;8` / `38;5;7`**（经 crossterm 后端；**文档明确（源码）**） |
| 9 | 两者亮度差多少 | 无跨终端固定值（主题可改写）。Alacritty 默认下索引 8=`#6b6b6b`、索引 7=`#d8d8d8`，对比 3.74:1（本报告按 WCAG 公式算） |
| 10 | 亮背景终端上 `Rgb(0x4a,0x4a,0x4a)` 看不清吗 | **假设不成立**：它在白底上是 8.86:1（太黑太重），在 `#181818` 暗底上才是 2.00:1。真正脆的是 `DarkGray` 系与 DIM |

---

## 1. `Modifier::DIM`（SGR 2）

### 1.1 发送侧：ratatui 与 crossterm 实际做什么

- ratatui 的 `Modifier::DIM` 在 crossterm 后端被翻成 `CrosstermAttribute::Dim`，没有条件、没有能力检查 —— `ratatui-crossterm-0.1.2/src/lib.rs:441-443`。**文档明确（源码）**
- crossterm 的 `Attribute::Dim` 定义即 SGR 2（`crossterm-0.29.0/src/style/types/attribute.rs:98`），`SetAttribute` 写出 `\x1b[2m`（同仓 `src/style.rs:338-341`）。**文档明确（源码）**
- **撤销与 BOLD 共用一条序列**：sub_modifier 里去掉 `DIM` 时 ratatui 发 `NormalIntensity`（`ratatui-crossterm-0.1.2/src/lib.rs:470-472`），crossterm 的 `NormalIntensity` 是 SGR 22；ratatui 的重置逻辑把 BOLD 与 DIM 一起当作 intensity 处理（同文件 `:550-552`）。Windows Terminal 的 faint 实现里写得更直白：「Turning off Bold and Faint must be handled at the same time, since there is only one sequence that resets both of them」（`pull/6873` 的 diff）。**文档明确（源码）**
- crossterm 自己的属性表把 `Dim` 在 Windows 与 UNIX 下都标 ✓（`attribute.rs:20-40`）——这只说明**它会发送**，不代表终端兑现。**文档明确（源码），但语义上不要外推**
- **`NO_COLOR` 只掐颜色、不掐属性**：`Colored` 的 `Display` 在 `ansi_color_disabled` 时直接返回空串（`colored.rs:96-102`），而 `SetAttribute` 的 `write_ansi` 不看这个开关（`style.rs:338-341`）。也就是说 `NO_COLOR=1` 时颜色全消失，但 `DIM` 与 `BOLD` 仍会发出。**文档明确（源码）**
- crossterm 有一个公开的能力探测函数 `available_color_count()`，按 `COLORTERM` / `TERM` 返回 8 / 256 / `u16::MAX`（`crossterm-0.29.0/src/style.rs:163-180`），但它**只在定义处与测试里出现**，内部写颜色时不调用；ratatui 三个 crate 里 grep `available_color_count` / `COLORTERM` / `truecolor` 零命中。**文档明确（源码）**

### 1.2 兑现侧：逐终端

| 终端 | SGR 2 的实际行为 | 强度 | 来源 |
| --- | --- | --- | --- |
| VTE / GNOME Terminal | 前景各通道 **×2/3**（照抄 xterm 的公式），**不是**换细字重；且「黑字白底下默认黑前景不会被弄得更淡」 | 社区报告（VTE 维护者 Egmont Koblinger 的 issue 正文） | [GNOME/vte#2462](https://gitlab.gnome.org/GNOME/vte/-/work_items/2462)（原 [Bugzilla 791596](https://bugzilla.gnome.org/show_bug.cgi?id=791596)） |
| xterm | 同一套 ×2/3 公式 | 社区报告（VTE issue 里对 xterm 的描述） | 同上 |
| Windows Terminal | `IsFaint()` 时前景 RGB **各分量除以 2**、背景不变；SGR 22 同时清 bold+faint | 文档明确（源码/PR diff） | [microsoft/terminal#6873](https://github.com/microsoft/terminal/pull/6873) |
| kitty | 0.10.0 起支持，「make text blend into the background」（向背景混合） | 文档明确（CHANGELOG） | [kitty CHANGELOG 提交 a63682b](https://browse.dgit.debian.org/kitty.git/commit/?id=a63682b16072039d2f1f5bcce33495e096a6d806) |
| xterm.js（VS Code 集成终端的内核） | 缓冲层有 `isDim()`；**DOM renderer 把 dim 作用在背景**：`bgOverride = color.multiplyOpacity(resolvedBg, 0.5)`，并把最小对比度阈值减半 | 文档明确（源码） | [`DomRendererRowFactory.ts`](https://raw.githubusercontent.com/xtermjs/xterm.js/master/src/browser/renderer/dom/DomRendererRowFactory.ts)、[`AttributeData.ts`](https://raw.githubusercontent.com/xtermjs/xterm.js/master/src/common/buffer/AttributeData.ts) |
| Alacritty | 有专门的 `colors.primary.dim_foreground`（默认 `#828482`）与整套 `colors.dim`（8 色，未设时基于 normal 自动算），说明 dim 是**独立配色**而非字体变换 | 文档明确（man page 的配置项语义）；「SGR 2 触发它」是合理推断，man 未直说 | [alacritty(5)](https://man.archlinux.org/man/alacritty.5.en) |
| WezTerm | 用户报告它用**更细的字重**渲染，且 dim+bold 不能共存（想改成降不透明度） | 社区报告（用户陈述；该 discussion 的答案正文未取到） | [wezterm discussion #4026](https://github.com/wezterm/wezterm/discussions/4026) |
| iTerm2 | **未证实**：官方「专有转义码」页只讲 OSC，没讲 SGR 2 的兑现 | 未证实 | [iTerm2 escape codes](https://iterm2.com/documentation-escape-codes.html) |
| tmux 内 | tmux **认识** `dim` 这个属性（`GRID_ATTR_DIM`，字符串名 `dim`，与 `bright`/`bold` 并列）；但它向下游/上游怎么转发、目标终端不支持时怎么降级，**未证实** | 文档明确（源码，属性存在）；转发行为未证实 | [`tmux/attributes.c`](https://raw.githubusercontent.com/tmux/tmux/master/attributes.c)、[tmux#135](https://github.com/tmux/tmux/issues/135)（未能取到正文） |

**要点**：DIM 至少有三套互不兼容的实现方向 —— 变暗前景（VTE/xterm/WT）、向背景混合（kitty）、改背景不透明度（xterm.js）。任何「靠 DIM 让次级文本淡下去」的设计都在赌终端实现。

### 1.3 色深模式差异

没有找到任何终端「只在 16 色或只在 truecolor 下兑现 SGR 2」的一手证据；上述实现都作用在解析后的 RGB 上，**很可能与色深无关，但这是未证实**。唯一确证的是 1.1 里那条：降级与探测在 crossterm/ratatui 侧根本不存在。

---

## 2. 字形宽度与显示

### 2.1 Unicode 属性

用本机 `unicodedata`（Unicode 16.0.0）实测：

| 码位 | 字符 | East Asian Width |
| --- | --- | --- |
| U+2500 | `─` | **A（Ambiguous）** |
| U+2502 | `│` | **A** |
| U+2504 | `┄` | **A** |
| U+2506 | `┆` | **A** |

**文档明确（UCD 实测）**。官方数据文件：[EastAsianWidth.txt](https://www.unicode.org/Public/UCD/latest/ucd/EastAsianWidth.txt)（U+2500–U+257F 整段归 A），定义见 [UAX #11](https://www.unicode.org/reports/tr11/)：Ambiguous 字符的宽窄**由上下文决定**，因此「在等宽字体里必然占 1 列」不是 Unicode 层面的保证，只是多数终端/字体的选择。

这条属性在终端里有开关：

- WezTerm 有配置项 `treat_east_asian_ambiguous_width_as_wide`（URL 锚点显示默认 false）。**文档明确** —— [配置页](https://wezterm.org/config/lua/config/treat_east_asian_ambiguous_width_as_wide.html)
- kitty 上有实际错位报告：「incorrect handling of CJK ambiguous width characters」。**社区报告** —— [kitty#6560](https://github.com/kovidgoyal/kitty/issues/6560)
- iTerm2 允许在 Unicode 8 / 9 的宽度表之间切换（`OSC 1337 ; UnicodeVersion=`）。**文档明确** —— [iTerm2 escape codes](https://iterm2.com/documentation-escape-codes.html)

注意语境差别：这些开关主要被**东亚宽字符（CJK）**的歧义触发；在纯 ASCII/拉丁内容的界面里，绝大多数终端按窄渲染 box drawing。真正要防的是「用户/系统开了 ambiguous=wide」这一档。

### 2.2 ratatui `LIGHT_TRIPLE_DASHED` 的实际字符集

`ratatui-core-0.1.2/src/symbols/line.rs`：

```rust
pub const LIGHT_TRIPLE_DASH_VERTICAL: &str = "┆";    // U+2506
pub const LIGHT_TRIPLE_DASH_HORIZONTAL: &str = "┄";  // U+2504
pub const LIGHT_TRIPLE_DASHED: Set = Set {
    vertical: LIGHT_TRIPLE_DASH_VERTICAL,
    horizontal: LIGHT_TRIPLE_DASH_HORIZONTAL,
    ..NORMAL                      // 角与交叉仍是 ┌┐└┘├┤┬┴┼
};
```

（`line.rs:6,16,144-148`；`border.rs:139` 把它转成 border set。）**文档明确（源码）**。仓库里有 3 处使用：`src/render/tui.rs:4023`、`4777`、`5939`。

结论：**只有横线变虚线，四个角与 T 形交叉是实线**；`┄` 与 `┆` 的交叉点靠相邻的 `─`/`│` 接上，虚线节奏在拐角处天然断一次。

**这直接回答 04 票的疑点**：外壳手画的 `┆`/`┄`（`src/render/tui.rs:3736-3755`）与浮层用的 `border::LIGHT_TRIPLE_DASHED` 在**横竖两个码位上完全同一个字符**（源码确证），差别只在浮层还带四个实线角与 T 形交叉，而外壳的转角是手画的。要让两者「同一档」，不需要换字符；需要决定的是**角与交叉**要不要也虚化（ratatui 没有 triple-dash 的角，虚角要自己补）。

### 2.3 终端会自绘 box drawing

- Alacritty 默认 `font.builtin_box_drawing = true`：对 **U+2500–U+259F**、legacy computing 与 powerline 码位「使用内置字体绘制」。**文档明确** —— [alacritty(5)](https://man.archlinux.org/man/alacritty.5.en)
- xterm.js：`isBoxOrBlockGlyph(codepoint) = 0x2500..=0x259F`，且 `treatGlyphAsBackgroundColor()` 对这些码位返回 true（当背景色处理，避免字缝）。**文档明确（源码）** —— [`RendererUtils.ts`](https://raw.githubusercontent.com/xtermjs/xterm.js/master/src/browser/renderer/shared/RendererUtils.ts)

含义有两面：好的一面是**字体缺字形/断线的风险在现代终端里被终端自己兜住了**；坏的一面是这些码位的最终笔形与粗细由终端决定，界面无法保证 `┄` 与 `─` 的相对观感一致。

### 2.4 字体覆盖（本机 fontconfig 实测）

| 字体（本机已装） | U+2500 | U+2504 | U+2506 |
| --- | --- | --- | --- |
| Liberation Mono | 有 | **无** | **无** |
| Noto Sans Mono | 有 | 有 | 有 |
| Adwaita Mono | 有 | 有 | 有 |
| JetBrains Mono | 有 | 有 | 有 |
| Hack | 有 | 有 | 有 |
| FiraCode Nerd Font / FiraMono Nerd Font | 有 | 有 | 有 |

（`fc-list :charset=2504` 等，本机实测。）**关键反例**：Liberation Mono 覆盖基础制表符却不覆盖 triple dash 系列 —— 「字体有 `─` 就一定有 `┄`」是错的。DejaVu Sans Mono、Menlo、Consolas、SF Mono、Ubuntu Mono、Fira Code 本体在本机**未安装**，无法实测，它们的覆盖情况属**未证实**（本机只装了它们的 Nerd Font 变体）。

不过按 2.3，前三个终端会在渲染层自绘，Liberation Mono 这类缺口未必暴露；它的实际影响取决于终端是否启用自绘、以及用户是否换了字体。

---

## 3. 颜色降级

### 3.1 `Color::Rgb`：没有降级，一路直发

- ratatui → crossterm 是逐变体直译：`Self::Rgb(r,g,b) => CrosstermColor::Rgb{r,g,b}`、`Self::Indexed(i) => AnsiValue(i)`（`ratatui-crossterm-0.1.2/src/lib.rs:410-428`）。**文档明确（源码）**
- crossterm 写颜色时直接产出：`Color::Rgb{r,g,b} => write!(f, "2;{r};{g};{b}")`，外层加 `38;`/`48;`/`58;`（`crossterm-0.29.0/src/style/types/colored.rs:96-148`）。**文档明确（源码）**
- **连 16 个命名色也走 256 色形式**：`Color::DarkGrey => "5;8"`、`Grey => "5;7"`、`Red => "5;9"` …（`colored.rs:131-148`）。所以这个界面在 16 色终端上也依赖 `38;5;N`，而不是 `30–37 / 90–97`。**文档明确（源码）**
- ratatui 侧没有任何 `TERM`/`COLORTERM` 探测或就近取色（grep 零命中）。**文档明确（源码）**
- 终端收到 `38;2;…` 而自身只有 8/16/256 色时**怎么处理没有规范**（退回默认前景？就近取色？当未知序列忽略？），这是**未证实**，也正是「不可依赖」的理由本身。

### 3.2 `DarkGray` 与 `Gray` 的落点

- ratatui 的 `Color` 文档表写：`gray` → 前景 37 / 背景 47，`darkgray` → **90 / 100**（`ratatui-core-0.1.2/src/style/color.rs:14-24`）。**文档明确**
- 但经 crossterm 后端时，`ratatui Color::Gray` → `CrosstermColor::Grey` → 字符串 `5;7`，`Color::DarkGray` → `DarkGrey` → `5;8`（`ratatui-crossterm-0.1.2/src/lib.rs:417-418`；`colored.rs:131-148`）。**文档明确（源码）**

**即：文档说 90，字节是 `38;5;8`。** 审阅或写测试时不要拿文档表推断线缆内容。crossterm 自己的 `parse_ansi` 也把 256 色索引 7 叫 `Grey`、8 叫 `DarkGrey`（`color.rs:128-148`）。

### 3.3 索引 7 / 8 的实际亮度

**没有跨终端的固定 RGB**：16 个 ANSI 索引的颜色由终端主题定义、并可被程序改写 —— iTerm2 的 `OSC 1337 ; SetColors=` 与 `OSC 4` 都直接操作这些槽位（**文档明确**，[iTerm2 escape codes](https://iterm2.com/documentation-escape-codes.html)）；Alacritty 把 `colors.normal` / `colors.bright` 全部暴露成配置（**文档明确**，[alacritty(5)](https://man.archlinux.org/man/alacritty.5.en)）。

以 Alacritty 默认主题为例（文档明确）：

| 槽位 | 默认值 | 相对亮度 L | 与对方的对比度 |
| --- | --- | --- | --- |
| 索引 7（`normal.white`） | `#d8d8d8` | 0.687 | 3.74:1 |
| 索引 8（`bright.black`） | `#6b6b6b` | 0.147 | — |

（对比度是本报告按 WCAG 相对亮度公式算的；xterm 传统值 `#c0c0c0` / `#808080` 算出来是 2.17:1。）两者的「亮度差」在 Alacritty 默认下并不大 —— 1.5 倍左右的对比度差，**不足以单独承担两级信息层级**。

---

## 4. 亮背景（浅色主题）

### 4.1 先纠正一个假设

仓库里的固定深灰是 `CHROME_LINE = Color::Rgb(0x4a,0x4a,0x4a)`（`src/render/tui.rs:212-218`，注释里已写明它「刻意是真彩色」、亮背景终端要另调）。按 WCAG 计算：

- `#4a4a4a` 对白底 `#ffffff`：**8.86:1** —— 在浅色主题下它**不是看不清，而是太黑、太重**，一根线会压过内容。
- `#4a4a4a` 对 Alacritty 默认底色 `#181818`：**2.00:1** —— 暗色主题下这才是它的边界地带。

所以「固定深灰在浅色主题下看不清」这个担心方向反了：真正会「看不清」的是**浅色主题下的浅灰**（`DarkGray` 落到主题定义的浅色槽位时），以及 **DIM 作用后的文字**。

### 4.2 真正脆的两处

1. **DIM 在浅色主题不一定变淡，方向由终端定**：
   - VTE/xterm 的 ×2/3 对「白底上的黑字」几乎不产生变化（维护者原话，见 1.2）→ 想靠 DIM 做次级层级，在浅色主题上可能**什么都没有发生**。
   - kitty 的 blend-into-background 会让深色文字变淡（有效，但可能过淡）。
   - Windows Terminal 的 ÷2 让深色文字更黑（对比更强）。
   - xterm.js 把 dim 做在背景上（半透明背景）→ 浅色主题下观感不可预期。

   **对本 effort 最直接的两处**：问卷输入区拿 `DIM` 当「当前高亮」（`tui.rs:3587-3594`，08 票点名的疑点），以及 05 票打算把「次级」整层压到 `DIM` 上。这两处在上述四种实现下会得到四种不同观感；`REVERSED` 是更稳的焦点表达（crossterm 属性表 Windows/UNIX 都 ✓，**文档明确**，`attribute.rs:20-40`；8 色终端也兑现）。
2. **`DarkGray` 系（= 索引 8）的语义完全交给主题**：仓库里它是用得最多的颜色 —— 95 处命名色字面量里 38 处是 `Color::DarkGray`（本报告实测：`highlight.rs` / `panel.rs` / `tui.rs` 三个文件，无测试行；03 与 05 票里写的是「32 处」，口径可能不同，以票自身的清单为准）。在浅色主题里索引 8 常被定义成「比正文更浅的灰」，用作说明文字时对比度可能不足以阅读。

### 4.3 不引入用户可配主题系统的低成本做法

按证据强弱排列，**没有一条是万能的**：

1. **把层级交给留白、缩进、线型与内容本身**，颜色只做点缀 —— 不依赖任何 SGR 2 / 灰阶假设。这是唯一在所有终端都成立的做法。
2. **`Modifier::BOLD` + 默认前景** 承担「主」，默认前景不加修饰承担「次」。`BOLD` 是 crossterm 表格里 Windows/UNIX 都 ✓ 的属性，兑现面最广（**文档明确**，`attribute.rs:20-40`）。注意两点：`BOLD` 与 `DIM` 共用 SGR 22 撤销（1.1）；部分终端/配置会把 bold + 低索引色提亮（Alacritty `draw_bold_text_with_bright_colors` 默认 **false**，xterm.js DOM renderer 里同名选项控制 `fg += 8`，**文档明确**）。
3. **用 `Color::Reset`（默认前景）而不是 `Gray`/`DarkGray`**：crossterm 对 `Reset` 发 `39`（`colored.rs:106-108`），文字跟随用户主题，浅深皆可读。代价是失去「灰」这一档，需要别的手段表达次级。
4. **固定深灰只用于装饰性线条**，且优先挑向背景收敛的暗度；绝不用于承载信息的前景。仓库 `CHROME_LINE` 已经是这个用法，但它没有主题适配，浅色主题下会偏重（4.1）。
5. **同时提供明暗两套固定值需要用户配置**，超出本轮范围；若真要单值兜底，`DarkGray`（索引色）比 `Rgb` 更容易被用户的终端主题接管 —— 这是取舍，不是保证。

---

## 5. 「在主流终端上不可靠的做法」清单

| # | 做法 | 为什么不可靠（依据） | 影响票 |
| --- | --- | --- | --- |
| 1 | 靠 `Modifier::DIM` 表达「次级文本」的明暗 | 三套互不兼容实现：VTE/xterm ×2/3、WT ÷2、kitty blend、xterm.js 改背景（§1.2）。语义不统一 | **05（层级）**、**03（色板）** |
| 2 | 假设 DIM 在亮背景主题下也会变淡 | VTE 维护者明确：黑字白底下默认黑前景不会变淡（§1.2） | **05**、**08**（问卷输入区拿 DIM 当高亮，`tui.rs:3587-3594`） |
| 3 | 同一个 cell / 相邻样式同时用 `BOLD` 与 `DIM` 表示两级 | SGR 22（crossterm `NormalIntensity`）把两者一起清掉；ratatui 的重置逻辑也把它们当同一档 intensity（§1.1） | **05**、**06**（输入区整段 BOLD，`tui.rs:4514-4517`） |
| 4 | 把 `Color::Rgb(...)` 固定深灰当「跨终端稳定的深灰」 | 无能力探测、无降级，字节直发 `38;2`（§3.1）；固定值无法同时适配明暗主题（§4.1） | **03**、**08**（`/`、`@` 菜单边框用 `CHROME_LINE`，`tui.rs:4774-4780`） |
| 5 | 用 `Color::DarkGray` 当「中灰」承担层级 | 落到 `38;5;8`，实际 RGB 由主题定（§3.2/§3.3）；浅色主题下可能贴近背景 | **03**、**05**、**07**（占比条与标签同为 DarkGray，「对比过低」） |
| 6 | 按 ratatui 文档表以为 `Gray`=37 / `DarkGray`=90 | 经 crossterm 后端实际是 `38;5;7` / `38;5;8`（§3.2） | **03** |
| 7 | 假设 `┄`/`┆` 一定与 `─`/`│` 同宽、占 1 列 | UCD 把四者都归 Ambiguous；存在 ambiguous=wide 开关与错位报告（§2.1） | **04（字形）**、**07**（转录右缘两列竖线）、**08**（浮层框） |
| 8 | 假设所有等宽字体都有 `┄`/`┆` | Liberation Mono 有 U+2500、无 U+2504/U+2506（本机实测，§2.4）；不过 Alacritty/xterm.js 会自绘，风险被终端兜住一部分 | **04** |
| 9 | 假设 `LIGHT_TRIPLE_DASHED` 的四角也是虚线 | ratatui 的 `Set` 只换横竖，角沿用 `NORMAL` 实线（§2.2）。手画外壳与浮层的横竖**已是同一码位**，差别在这个角 | **04**、**08**（三处浮层均用该边框） |
| 10 | 用 `DIM` 当「焦点在哪」的唯一表达 | 与第 1/2 条同因；同一表面里已有 `REVERSED` 可用，后者兑现面更广（§4.2） | **08**、**05** |
| 11 | 在 tmux 内假设 DIM / truecolor 原样到达下游 | tmux 有独立的 dim 属性模型，转发/降级行为未证实（§1.2）；这是**存疑**项，不是已证的不行 | **03**、**05** |
| 12 | 依赖固定索引色 7/8 的绝对亮度做对比设计 | 索引色的 RGB 可被主题与 OSC 改写，无跨终端固定值（§3.3）；Alacritty 默认下两者对比仅 3.74:1 | **03**、**05** |

---

## 6. 未能证实 / 存疑

1. **iTerm2 的 SGR 2 兑现行为** —— 官方文档没写，未找到源码或 issue 证据。§1.2 里它只能算空白。
2. **tmux 对 SGR 2 的转发与降级** —— 只确证 tmux 内部有 `GRID_ATTR_DIM`/`dim` 属性（源码），转发策略与目标终端不支持时的行为未证实；`tmux#135` 正文因抓取限制未取到。
3. **终端收到 `38;2` 而自身不支持 truecolor 时的处理** —— 没有规范可引。crossterm/ratatui 不降级是确证的，「终端怎么办」是空白。
4. **DIM 是否与色深（16/256/truecolor）有关** —— 未找到任何一手证据；各实现都作用在解析后的 RGB 上，推测无关，但没证实。
5. **未安装字体的码位覆盖**（DejaVu Sans Mono、Menlo、Consolas、SF Mono、Ubuntu Mono、Fira Code 本体）—— 本机没有，无法实测；只能确认本机装了的那些。
6. **VS Code 集成终端具体用哪个 xterm.js renderer** —— 本文引的是 DOM renderer 源码；VS Code 是否默认 DOM、WebGL/canvas addon 的 dim 实现是否一致，未证实。
7. **WezTerm 的权威行为** —— 只取到用户陈述（细字重、dim+bold 不能共存），discussion 的选定答案正文未取到。
8. **16 色终端对 `38;5;N` 的兜底** —— crossterm 连命名色都发 256 色形式（源码确证），但「只有 16 色的终端是否一定认 `38;5`」没有一手来源。

---

## 7. 来源清单

**本地源码（最硬的一手）**

- `crossterm-0.29.0/src/style/types/attribute.rs`（`Dim = 2`、支持表）
- `crossterm-0.29.0/src/style/types/colored.rs`（`Rgb → 2;r;g;b`、`DarkGrey → 5;8`、`Reset → 39`、`NO_COLOR` 短路）
- `crossterm-0.29.0/src/style.rs`（`SetAttribute`、`available_color_count`、`force_color_output`）
- `crossterm-0.29.0/src/style/types/color.rs`（`parse_ansi` 索引表）
- `ratatui-crossterm-0.1.2/src/lib.rs`（`Modifier`/`Color` → crossterm 的直译、`NormalIntensity` 重置）
- `ratatui-core-0.1.2/src/symbols/line.rs`、`symbols/border.rs`（`LIGHT_TRIPLE_DASHED` 字符集）
- `ratatui-core-0.1.2/src/style/color.rs`（文档表 37/90 与实际字节的差异）
- 仓库：`src/render/tui.rs:212-218`（`CHROME_LINE`）、`:4023/4777/5939`（虚线边框）、`:246`（`DarkGray`）、颜色用量 96 处

**网络一手来源**

- GNOME VTE：[#2462 Thoughts about faint (SGR 2)](https://gitlab.gnome.org/GNOME/vte/-/work_items/2462)、[Bugzilla 791596](https://bugzilla.gnome.org/show_bug.cgi?id=791596)
- Windows Terminal：[PR #6873 Add support for the "faint" graphic rendition attribute](https://github.com/microsoft/terminal/pull/6873)
- kitty：[CHANGELOG 提交 a63682b（0.10.0 支持 SGR faint）](https://browse.dgit.debian.org/kitty.git/commit/?id=a63682b16072039d2f1f5bcce33495e096a6d806)、[issue #6560 ambiguous width](https://github.com/kovidgoyal/kitty/issues/6560)
- WezTerm：[discussion #4026](https://github.com/wezterm/wezterm/discussions/4026)、[treat_east_asian_ambiguous_width_as_wide](https://wezterm.org/config/lua/config/treat_east_asian_ambiguous_width_as_wide.html)
- xterm.js：[AttributeData.ts](https://raw.githubusercontent.com/xtermjs/xterm.js/master/src/common/buffer/AttributeData.ts)、[DomRendererRowFactory.ts](https://raw.githubusercontent.com/xtermjs/xterm.js/master/src/browser/renderer/dom/DomRendererRowFactory.ts)、[RendererUtils.ts](https://raw.githubusercontent.com/xtermjs/xterm.js/master/src/browser/renderer/shared/RendererUtils.ts)
- Alacritty：[alacritty(5) 配置手册](https://man.archlinux.org/man/alacritty.5.en)
- tmux：[attributes.c](https://raw.githubusercontent.com/tmux/tmux/master/attributes.c)、[issue #135](https://github.com/tmux/tmux/issues/135)
- iTerm2：[专有转义码文档](https://iterm2.com/documentation-escape-codes.html)
- Unicode：[UAX #11 East Asian Width](https://www.unicode.org/reports/tr11/)、[EastAsianWidth.txt](https://www.unicode.org/Public/UCD/latest/ucd/EastAsianWidth.txt)（本机 UCD 16.0 实测对照）
