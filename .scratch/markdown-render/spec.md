# TUI 里的 Markdown：换解析器，补表格与代码块

模型用 Markdown 作答，而转录里那份 Markdown 现在由一支手写的单趟扫描器渲染（[`src/render/markdown.rs`](../../src/render/markdown.rs)）。这次把**解析**交给 `pulldown-cmark`，**渲染**（到 ratatui 的 `Line`）仍然全是我们自己的；顺手把三个最刺眼的地方补上：**表格**、**代码块高亮**、**每条消息前面那 11 格缩进**。

来源：2026-10-01 一次 `/ask-matt` → `/research`（两份一手笔记）→ `/grilling`（14 个决定）的会话。三项要求由维护者提出，其余决定由访谈折出，逐条记在下面的实现决定里。

它**推翻**两处：

- [`src/render/markdown.rs`](../../src/render/markdown.rs) 开头那句「它刻意不是 CommonMark……**不依赖任何 parser**」——姿态改写成「不手写解析器」，见 [ADR 0008](../../docs/adr/0008-markdown-parsing-by-pulldown-cmark.md)；
- [`../tui-layout/spec.md`](../tui-layout/spec.md) §3 里「**assistant 的消息本来就是全文 Markdown，不动**」与「续行按 speaker 前缀显示宽度缩进」的**后半句**——非 assistant 那一半留着，assistant 那一半改成不缩进（§5）。

## 问题陈述

1. **表格画出来不是表格。** `table_line` 把各格用 `" │ "` 接起来，没有列宽、没有对齐；`is_table_separator` 认出的那条 `|---|---|` 被判为「不带内容」而整行丢掉——而它恰好是**唯一**能标出「上一行是表头」的信号。于是表头既认不出、也无从对齐，单元格里的行内 Markdown 也不渲染。
2. **代码块只有一种颜色。** `code_line` 给整行上 `Color::Yellow`，围栏上的语言标签被 `opening_fence` 丢掉（它只返回 `(字符, 长度)`）。而仓库里 [`src/render/highlight.rs`](../../src/render/highlight.rs) 有一整套 tree-sitter 语法高亮——**它没有生产消费者**，[`docs/highlight.md`](../../docs/highlight.md) 整篇记的就是这件事，其中「什么会让它回来」一节描述的正是这个场景。
3. **每条消息的每一行前面都有 11 格空格。** `attribute` 给块里**每一行**都加 `[speaker] ` 宽度的前导。对用户输入那样「一次发言」是对的；对 assistant 的回答——那是一份**文档**，结构由 Markdown 自己给（标题、列表、代码块）——这 11 格把结构整体推右，还吃掉主列约七分之一。

## 方案

- **解析层换血**（§1）：引入 `pulldown-cmark`（`default-features = false`，5 个纯 Rust 包）**只做解析**；`markdown.rs` 重写成事件驱动的渲染器。渲染决定（`Line`/span/列宽/折行/表头样式/代码块）一条都不外包。
- **表格**（§2）：表头加粗 + `─┼─` 分隔线；列宽按内容自适应、余量给最后一列；超宽时单元格内折行、**整行等高**；表格顶格。
- **代码块**（§3）：上方单独一行，语言名**右对齐到内容右缘**；代码 2 格缩进、逐字保留。
- **高亮**（§4）：接回 `render::highlight`，文法扩到 10 种语言，全部硬依赖。
- **缩进**（§5）：assistant 的续行不再缩进；非 assistant 保持现状。
- **行内与降级**（§6）：图片落成 `[图片] alt (url)`，内联 HTML 与 HTML 块原样透传。
- **明确不动**（§7）：流式期间仍是纯文本；换行仍归 `pane::wrap_line`。

## 用户故事

1. 作为读转录的人，我希望表格的表头与数据行分得开，这样我能一眼看出哪一行是列名。
2. 作为读转录的人，我希望表格的列对得齐，这样我能沿着列竖直读下去。
3. 作为读转录的人，我希望一张超宽的表格不要被截断，而是把装不下的格子折行，这样我不丢信息。
4. 作为读转录的人，我希望代码块有语法高亮，这样我能一眼看出关键字、字符串与注释。
5. 作为读转录的人，我希望知道代码块是什么语言，同时不希望那一行标签抢走正文的宽度。
6. 作为读转录的人，我希望回答的正文不要整体缩进 11 格，这样主列宽度都用得上。
7. 作为读转录的人，我希望**自己**输入的多行消息仍然缩进对齐，这样一次发言读起来是一块。
8. 作为读转录的人，我希望认不出的语言、没写语言的围栏、画不出来的图片都**降级成能读的文本**，而不是消失。
9. 作为维护者，我希望 Markdown 的解析交给一个被复核过的库，这样表格对齐、行内嵌套、转义这些边角不再靠手写扫描器扛。

## 实现决定

### §1 解析层：`pulldown-cmark` 只做解析

- `Cargo.toml` 加 `pulldown-cmark`，`default-features = false`。闭包实测 **5 个纯 Rust 包**（自己 + `bitflags` + `memchr` + `unicase`），无 C 构建，MSRV 1.71.1，MIT。**不开** `simd`（那会引入 `unsafe`）。
- 开三个旗标：`ENABLE_TABLES`、`ENABLE_TASKLISTS`、`ENABLE_STRICKETHROUGH`。**不开** `ENABLE_FOOTNOTES`，也**不靠** `ENABLE_GFM` —— 实测它不是那四件套的总开关（只开 GFM 时表格仍是段落）。
- `to_lines` 的签名改为 `to_lines(text: &str, width: u16) -> Vec<Line<'static>>`。宽度是**转录内容的可用列数**。理由：表格的列宽与超宽行的折行都需要它（§2、§3）。这推翻 `../tui-layout/spec.md` §3 里「源行缓冲**宽度无关，可复现**」那条性质的一半：宽度变化时，现在要**重跑 markdown 渲染**再重新折行，而不只是重新折行。代价可接受——每个块一次 `pulldown-cmark`，很快；且 §3 已经规定缓存按 `frame.area().width` 失效、宽度变化全量重算。
  - **实现注记（2026-10-01）**：TUI 走的是同一族的第二个入口 `to_lines_indented(text, width, indent)`，`to_lines(text, width)` 是 `indent = 0` 的包装。`indent` 是调用方会在**第一行**前面加的那个前缀（`[kimi] `）占的列数；**需要左边界对齐的块**（表格、代码块）整块从那一列起、宽度预算扣掉它，其余块照常从第 0 列吐。这样 §2 的「表格列对得齐」与 §3 的「语言名结束在第 `width` 列」在与 §5 的「第一行有前缀、续行顶格」同时成立时不会互相打脸：调用方给第一行加前缀时，把渲染器铺的那 `indent` 个空格**换成**前缀，两边的列数一模一样。
- 渲染器内部是一个**块级状态机**：表格与代码块需要整块缓冲（前者算列宽、后者定界），其余块逐行吐出。
- `Text` 事件带行尾 `\n`，渲染器要把它当行边界而不是文本内容。
- 认不出的构造一律**原样透传**——这是旧扫描器最值钱的一条性质，重写后必须保住。
- 别名映射（`rs` → rust、`js` → javascript）**归我们**，`pulldown-cmark` 只把 info string 原样交出来。
- **宽度变化时的重渲染路径**（本 spec 最不显然的一处代价）。块是在**到达时**展开成行的（[`tui.rs`](../../src/render/tui.rs) 里 `feed` 那一段的 `paint_block(&block, &mut self.colors)` → `pane.push`），那一刻不知道宽度；而 `Pane` 的换行缓存只在 `Pane::view(width, …)` 里按宽度重算。收宽度之后，**源行本身**开始依赖宽度，于是宽度变化时不能只重新折行——必须**重渲染那些宽度敏感的块**（表格、代码块）。落点是票 01：把「块 → 行 → `pane.push` + `links` + `turn_rail`」抽成一个 `push_block(block, width)`，`Tui` 保留一份块列表，宽度变化时清空并按新宽度重放。代价是 `Tui` 多留一份块（与 `pane` 已经持有的源行同量级）；换回来的是 resize 之后表格不会碎成一堆错位的列。

### §2 表格

- **表头**：加粗 + 下面一条 `─┼─` 分隔线（把 `|---|---|` 请回来当信号）。
- **列宽**：按每列的**内容最大显示宽度**自适应；没有超宽时，**余量给最后一列**，表格因此撑满可用宽度。
- **对齐**：`Tag::Table(Vec<Alignment>)` 给出每列的对齐方向（`:--` / `--:` / `:-:`），照它排；没有冒号的按左对齐。
- **超宽**：总宽超出 `width` 时削减**最宽的那一列**，把省下的额度让给它折行；削减到该列的自然宽度仍然装不下就继续按同一规则往下削。**不截断。**
- **单元格内折行 + 整行等高**：一个格折成三行，同行其余各格一起撑到三行高，网格竖线在每条续行上都连续。
- **整表缓冲**：列宽必须先收齐全表才能算——逐行扫描画不出对齐的列。
- **顶格**：表格不加缩进，它需要全部可用宽度。
- 单元格里的行内 Markdown（代码、粗体、链接）**要渲染**。

### §3 代码块

- **语言名那一行**：代码块**上方**单独占一行，语言名**右对齐到转录内容右缘**（第 `width` 列），该行本身**不缩进**。它像一条右对齐的小标题，不是左侧的 `─ rust`。
- **语言名取 info string 的第一个词，保留作者的大小写**：`rust` 显示 `rust`、`JS` 显示 `JS`、`rust ignore` 只显示 `rust`（后面的词是给文档工具的指令，不是语言名）。
- **光秃秃的 ` ``` `**：**不画那一行**。
- **认不出的语言**：**照样显示原文**（`brainfuck` 就是 `brainfuck`），代码退纯文本——认不出的是高亮，不是标签本身。
- **代码内容**：2 格缩进，**逐字保留**。围栏里的 `**不是粗体**` 就是两个星号，永远不解析（现有测试 [`a_fenced_block_is_kept_verbatim_and_never_parsed_as_markdown`](../../tests/render_markdown.rs) 守的就是这条，重写后必须继续过）。
- **超宽代码行**：由**渲染器自己**按 `width` 折行，续行保持 2 格缩进——不交给 `pane::wrap_line` 硬折，那会把续行顶到最左边、跟代码块脱节。
- 缩进代码块（四个空格那种）按 `pulldown-cmark` 的 `CodeBlock` 事件处理，与围栏块同一条路径。

### §4 高亮：接回 `render::highlight`，扩到 10 种语言

- `render::highlight` 从「没有生产消费者」变回**代码块的消费者**。它的两层分工不变：语法层给前景，diff 层给背景（diff 层这一轮仍然没有调用方，它是为工具输出准备的）。
- 文法扩到 **10 种**：`rust`、`bash`、`json`、`toml`、`html`、`javascript`、`typescript`、`php`、`sql`、`python`。**全部硬依赖**，不做 feature 门控——没有 `#[cfg]` 分支，默认体验就是这 10 种。
- 每种语言一个 per-language `OnceLock<Option<HighlightConfiguration>>`，**延迟到第一次用到该语言才编译 query**（实测首次编译 json 0.05ms ～ php 70ms；全在启动时算会白付几百毫秒）。
- **已知坑，实现时照此处置**（一手核实见 [research/02](research/02-parser-interface-and-grammars.md)）：
  - `tree-sitter-bash` 与 `tree-sitter-javascript` 导出的是**单数** `HIGHLIGHT_QUERY`，其余八个是复数 `HIGHLIGHTS_QUERY`；
  - `toml` 必须用 **`tree-sitter-toml-ng`**（原版 `tree-sitter-toml` 停在 2022 年、锁 `^0.20`）；
  - `sql` 必须用 **`tree-sitter-sequel`**（`tree-sitter-sql` 停在 2021 年、锁 `^0.19.3`；活的那个名字与语言对不上）；
  - `typescript` 提供 `LANGUAGE_TYPESCRIPT` / `LANGUAGE_TSX` 两个常量，`php` 提供 `LANGUAGE_PHP` / `LANGUAGE_PHP_ONLY` 两个——这一轮各取第一个；
  - 现有 `CAPTURES` 对 html 的 `tag` / `tag.error`、php 的 `module` / `module.builtin` / `tag`、sequel 的 `conditional` / `field` / `float` / `parameter` / `storageclass` 没有对应项——**漏 = 不上色**，不是错。补齐它们，或明确记下不补。
  - typescript 的 `HIGHLIGHTS_QUERY` 只有 5 个 capture，JSX 那份在单独的 `JSX_HIGHLIGHT_QUERY`；这一轮**不拼接**，记在 §7。
- `try_highlight` 现在每次调用都新建 `Highlighter`；`tree-sitter-highlight` 的文档建议复用一个。改成复用。

### §5 缩进

- **assistant 的续行不再缩进**：`[speaker] ` 前缀只出现在块的第一行，其余行顶格。理由：回答是一份文档，结构由 Markdown 自己给；那 11 格把结构推右又吃掉主列宽度。
- **非 assistant（用户输入、系统行）保持现状**：续行仍按 `[speaker] ` 宽度缩进，它们的正文里没有任何结构可依赖。
- 于是 `attribute` 分叉：assistant 走「只给第一行加前缀」，其余走「每一行都加前导」。这不是一个开关，是两条名字清楚的路。

### §6 行内与降级

- **保持**：行内代码（`Color::Yellow`）、粗体、斜体（词边界上的 `_`）、删除线、链接。链接的画法不变：下划线标签 + 灰色 ` (url)`。
- **图片** `![alt](url)` → **`[图片] alt (url)`**。现在的输出是 `!alt (url)`——那个 `!` 是残留噪声（手写扫描器逐字符走，`!` 不匹配任何分支就被当普通字符吐了出来）。改成 `[图片]` 是明确告诉读者「这儿有一张图，终端暂时画不出来」，同时给将来接图像协议留了位置。
- **内联 HTML 与 HTML 块原样透传**。终端画不出 HTML，剥标签等于替作者做了一次有损翻译；透传至少保真，也正是「认不出的东西一律原样透传」这条原则。
- **嵌套列表每层 2 格**。旧扫描器用的是原始缩进（作者写几个空格就是几个），换解析器之后拿到的是嵌套**结构**，缩进归我们定：主列只有约 80 格，4 格一层的话三层就吃掉六分之一。
- **任务框**保持 `☐` / `☑`，**项目符号**保持 `• `，**有序列表**保持 `N. ` 并按 `Start` 事件的起始编号。

### §7 明确不动

- **流式期间仍显示纯文本**。模型边写边出的是状态机画的纯文本，只有消息**定稿**那一刻才整体过一遍 markdown（`Block::Delta` 在 `paint_block` 里返回空）。表格要整表缓冲、代码块要闭合才能定界，流式期间反复重渲染不仅白烧 CPU，没闭合的 `**` 和半截表格还会来回闪。定稿跳一次，比全程抖好看。
- **换行仍归 `pane::wrap_line`**（逐字符、CJK 2 列、列宽算术在 [`src/render/width.rs`](../../src/render/width.rs)）。渲染器只负责把行按 §2/§3 的规则**折到宽度内**，不再往下管。
- **`is_rule` / `blockquote` / `list_item` / `task_box` / `heading` 这些手写判定全部随解析器一起删除**。重写后的 `markdown.rs` 只留渲染，不留扫描。
- **JSX 的 `JSX_HIGHLIGHT_QUERY` 不拼接**：typescript 那一份只上 `HIGHLIGHTS_QUERY` 的 5 个 capture，`.tsx` 里的标签不上色——它是「漏 = 不上色」，不是错。

### §8 文档

- [`docs/render.md`](../../docs/render.md)：`[speaker]` 前缀那一节要改（assistant 与非 assistant 现在分叉）；渲染边界一节要提 `to_lines` 收宽度这件事。
- [`docs/highlight.md`](../../docs/highlight.md)：**整篇作废重写**。它现在的主题是「这个模块没有生产消费者」，重写后主题变成「它是代码块的高亮提供者，管 10 种语言」；「什么会让它回来 / 什么会让它消失」两节要相应改写。
- [`README.md`](../../README.md) 的文档表：加 ADR 0008 的条目。
- [`../tui-layout/spec.md`](../tui-layout/spec.md) §3：把「assistant 的消息本来就是全文 Markdown，**不动**」与「续行按 speaker 前缀显示宽度缩进」两句改成指向本 spec §5 的交叉引用，并改掉「宽度无关」那半句。
- [`../../docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md)：加一条真机项（表格、代码块高亮、10 种语言各看一眼）。

## 测试决定

- **`tests/render_markdown.rs` 是主战场**，它已经建在自己的接缝上（直接调 `to_lines`）。现有测试：
  - `quotes_rules_and_tables_render_as_structure` 里那条「分隔那一行被丢掉」的断言**必须反过来写**——分隔线现在是表头信号；
  - `a_fenced_block_is_kept_verbatim_and_never_parsed_as_markdown` **一字不改**，它是这次重写最重要的安全网；
  - `malformed_markdown_degrades_to_plain_text_rather_than_vanishing` 同理不动；
    - **实现注记（2026-10-01）**：这条测试的**形状**没动（畸形输入不消失、且留下能读的文字），但用例集换掉了两个：原清单里的 `#` 与 `>` 在 CommonMark 下是**合法但空**的结构（空标题、空引用），渲染成空本来就是对的，留在「不消失」的清单里会逼渲染器为它们编一行文本。换成了 `1.` 与 `<b` 这两个同样是畸形输入、但确实该留下文字的用例。
  - 所有调用点补上 `width` 参数。
- **新增断言**（宁可断言结构，不要断言整份文本快照）：
  - 表格：表头 span 带 `BOLD`；分隔线存在；各列起点列号一致（对齐）；超宽时某格折成多行且同行其余格同高；表格首列从第 0 列开始（顶格）。
  - 代码块：语言名那一行的语言名**结束在第 `width` 列**；无标签时不产生那一行；认不出的语言照样产生那一行；代码行以 2 格缩进开头且 `**` 原样。
  - 高亮：`rust` 围栏里 `fn` 的 span 是 `Keyword`、字符串是 `String`（断言 `Class`，不是颜色）；10 种语言各来一个最小样例，确认都拿到过非 `Plain` 的 span——这一条是 10 个 grammar 的接线测试。
  - 图片：`![alt](url)` 渲染成 `[图片] alt (url)`；HTML 原样出现。
- **`tests/render_tui.rs`**：`the_answer_block_is_rendered_as_markdown` 要跟着 `attribute` 的分叉改；新增一条断言 assistant 续行**不以空格开头**、而用户消息续行**以缩进对齐**。
- **真机**：`docs/tui-manual-checklist.md` 那一条新项——表格、代码块、10 种语言至少各看一次，因为语法高亮与 CJK 折行的组合只有真终端看得出来。
- **回归风险最高的一处**是 `to_lines` 的签名变化：所有调用点（`tui.rs` 的 `paint_block`、`plain.rs`、测试）都要跟着走。

## 明确不做

- **终端图像协议的真图渲染**（kitty graphics / iTerm2 inline images / sixel）。维护者的终端支持显示图片，这是明确留到后面做的一件事；这一轮图片只落成 `[图片] alt (url)`。
- **脚注**（`ENABLE_FOOTNOTES`）。终端里脚注需要另设计一套「引用标记 + 文末列表」的呈现，不该混在修表格与高亮里。
- **GFM 的其余扩展**：smart punctuation、标题属性、定义列表。
- **表格横向滚动**。超宽表格靠折行，不做横向滚动条。
- **`ratatui-markdown` / `tui-markdown` / `markdown-ratatui` 三个现成渲染器**。全部出局，理由与一手事实在 [research/01](research/01-markdown-crate-selection.md)：`tui-markdown` 关掉高亮仍有 43 个 registry crate、语言标签只能靠字符串抠、且 `Options::table_width` **只对表格折行**（200 字符段落回来仍是 1 行 200 列），违反「渲染器必须交回未换行的行」这条约束；`ratatui-markdown` 的已发布版本是 `ratatui ^0.29` 且锁 `tree-sitter 0.26`（与我们的 0.27 会双份共存），包体 8.37 MB；`markdown-ratatui` 完全没有高亮，且首发三周、84 次下载。
- **Rust 之外的高亮文法裁剪 / feature 门控**。10 种全部硬依赖。

## 补记

- **两份一手笔记**：[research/01-markdown-crate-selection.md](research/01-markdown-crate-selection.md)（三个 crate 的依赖、表格、高亮、接口形状）、[research/02-parser-interface-and-grammars.md](research/02-parser-interface-and-grammars.md)（`pulldown-cmark` 的事件模型、10 个 grammar 的兼容性实测）。
- **10 个 grammar 全部与 `tree-sitter 0.27` / `tree-sitter-highlight 0.27` 兼容**，已实测编译：12 个配置（10 语言 + tsx + php_only）全部构造成功，lockfile 里只有一份 `0.27.0` + 一份 `tree-sitter-language 0.1.8`。原因是 grammar 只锁 `tree-sitter-language ^0.1`，而 `LanguageFn` 从 0.1.1 到 0.1.8 结构未变。
- **规模认知**：这不是「修三个显示点」，而是**换解析器 + 重写渲染层 + 接 10 语言高亮**。三个投诉点里两个（表格、代码块）落在重写后的渲染层上，第三个（缩进）在 `tui.rs` 的 `attribute`，与解析器无关。
- **`to_lines` 收宽度这件事没有免费选项**：另一条路是让渲染器吐出「结构化逻辑行」、把表格对齐推迟到折行阶段，但那要求表格在 `Line` 之外再造一种中间表示，而 `Line` 是所有下游（转录缓存、pane、plain 渲染）共用的一条边界。加一个 `width` 参数比新造一种中间类型便宜得多。
