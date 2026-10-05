# 样式改动的测试与文档契约面（02 号研究票）

对应票：`.scratch/tui-visual-language/issues/02-research-style-change-surface.md`。
纯清点，不改任何既有文件。所有 `文件:行号` 以本次清点时的 `HEAD = cbfc2e6` 为准。

## 口径（先读这一节）

第 1 节的两类判定按票面给的口径：

- **钉语义**：断言说的是「哪个语义角色 / 哪条关系」。色板落地后把字面量换成色板常量即可，
  断言意图不变。
- **钉色值本身**：断言说的就是「这个值」。RGB、ANSI 字节、`Reset` 哨兵、退场色板的颜色清单、
  品牌坡道属于这一类；改色板或清理退场代码时这条必须专门重写。

另给一个**机械口径**作对照（凡出现 `Color::` / 十六进制 RGB / ANSI 字面量即算「色值」，
修饰符与关系断言算「语义」）：总数会从 80/9 变成 36/53。两种口径的总数都列在 1.6，方便
`/to-spec` 按自己需要的粒度取用。本报告正文用票面口径。

---

## 1. 测试里钉颜色与修饰符的断言

### 1.1 `tests/render_layout.rs`（42 处断言站点 / 45 处 `Color::`|`Modifier::` 匹配）

| 文件:行号 | 断言的是什么 | 类别 | 改法 |
| --- | --- | --- | --- |
| render_layout.rs:264–265 | 标记第一行前景 = `LightMagenta`（顶端是亮的那一头） | 语义 | 色板 `MARK_BRIGHT` |
| render_layout.rs:269–270 | 标记最后一行前景 = `Magenta`（暗的那一头） | 语义 | 色板 `MARK_DIM` |
| render_layout.rs:816–824 | 五行标记的颜色向量恒为「四行亮 + 一行暗」（不随脉冲变） | 语义 | 同上两个常量 |
| render_layout.rs:868–871 | 提示符静止色 = `Color::Rgb(216, 97, 97)` | **色值** | 脚本第 0 帧，精确 RGB 是契约；接回动画时连同 `prompt_colour` 一起处理 |
| render_layout.rs:943 | 提示符那一格前景 == `prompt_cell()` 取到的同一值（自洽） | 语义 | 关系断言，不动 |
| render_layout.rs:945–949 | 草稿 fg == `Color::Reset`（没被染色） | 语义 | 色板 `DEFAULT`/`PLAIN`（属性「无显式色」） |
| render_layout.rs:950–953 | 草稿保住了 `BOLD` | 语义 | 不动 |
| render_layout.rs:1021–1027 | 退场色环五色清单 `[LightBlue,LightCyan,LightGreen,LightYellow,LightRed]` | **色值** | 清理 `PULSE_PALETTE` 时一起走 |
| render_layout.rs:1030–1034 | 忙帧里没有任何一格穿上面那五色 | **色值** | 同上 |
| render_layout.rs:1271–1275 | 选中页签 fg = `LightMagenta` | 语义 | 色板 `TAB_SELECTED` |
| render_layout.rs:1276–1279 | 选中页签 `BOLD` | 语义 | 不动 |
| render_layout.rs:1282–1286 | 未选中页签 fg = `DarkGray` | 语义 | 色板 `MUTED`/`NARRATION` |
| render_layout.rs:1287–1290 | 未选中页签 `!BOLD` | 语义 | 不动 |
| render_layout.rs:1328 | 切页后选中页签仍是 `LightMagenta` | 语义 | 同 `TAB_SELECTED` |
| render_layout.rs:2356–2357 | 占比色条 bg = `DarkGray`（上下文行） | 语义 | 色板 `SHARE_BAR` |
| render_layout.rs:2359 | 两格之后不再涂 | 语义 | 关系断言 |
| render_layout.rs:2360 | 标签列不涂 | 语义 | 关系断言 |
| render_layout.rs:2364–2365 | 花销行色条 bg = `DarkGray` | 语义 | 同 `SHARE_BAR` |
| render_layout.rs:2367 | 五格之后不再涂 | 语义 | 关系断言 |
| render_layout.rs:2386–2388 | 没有分母则一处底色都不涂 | 语义 | 负例，不动 |
| render_layout.rs:2393–2394 | 上下文行自带分母，照涂 | 语义 | 不动 |
| render_layout.rs:2408 | 窄档也涂 | 语义 | 不动 |
| render_layout.rs:2409 | 窄档一列就够 | 语义 | 不动 |
| render_layout.rs:3821–3824 | 可兑现的 `/命令` fg == `TOKEN_COMMAND` | 语义 | **已经是命名常量**，是样板 |
| render_layout.rs:3834–3837 | 兑现不了的 `/tmp/x` fg == `Color::Reset` | 语义 | 色板 `PLAIN` |
| render_layout.rs:3847–3850 | 命中索引的 `@路径` fg == `TOKEN_REFERENCE` | 语义 | **已经是命名常量** |
| render_layout.rs:3859–3862 | 拼错的路径 fg == `Color::Reset` | 语义 | 色板 `PLAIN` |
| render_layout.rs:5081–5083 | 选项区高亮 `REVERSED` | 语义 | 不动 |
| render_layout.rs:5091–5093 | 输入区拿着键盘时高亮 `!REVERSED` | 语义 | 不动 |
| render_layout.rs:5095–5097 | 高亮降暗 `DIM` | 语义 | 不动 |
| render_layout.rs:5100–5102 | 自由文本行提亮 `BOLD` | 语义 | 不动 |
| render_layout.rs:5335–5339 | 思考行名字 fg = `LightCyan`（名册首位） | 语义 | 色板 `DEBATER_1` |
| render_layout.rs:5340–5343 | 思考状态词 fg = `DarkGray` | 语义 | 色板 `NARRATION` |
| render_layout.rs:5860–5863 | 调用行名字 fg = `LightCyan` | 语义 | 色板 `DEBATER_1` |
| render_layout.rs:5865–5868 | 调用描述 fg = `DarkGray` | 语义 | 色板 `NARRATION` |
| render_layout.rs:5902 | 详情覆盖层边框 fg = `LightCyan`（说话人色） | 语义 | 随发言者色板 |
| render_layout.rs:6532 | 注入前缀 fg = `LightBlue` | 语义 | 色板 `INJECTED` |
| render_layout.rs:6533 | 用户前缀 fg = `LightGreen` | 语义 | 色板 `USER` |
| render_layout.rs:6534 | 助手前缀 fg = `LightCyan` | 语义 | 色板 `DEBATER_1` |
| render_layout.rs:6535–6536 | 注入与用户、注入与助手互不相同 | 语义 | 关系断言，是这三色收敛时的护栏 |
| render_layout.rs:6721 | 左栏每一行 bg == `Color::Reset` | 语义 | 属性「不铺底色」 |
| render_layout.rs:6722 | 主列每一行 bg == `Color::Reset` | 语义 | 同上 |

辅助/夹具（不是断言，但改色板时会一起动）：`mark_colours`（770）、`prompt_cell`（852–856）、
`draft_colour`（3800–3810）、`buffer` 里的 `fg` 读取（1030）。

### 1.2 `tests/render_tui.rs`（17 处）

| 文件:行号 | 断言的是什么 | 类别 | 改法 |
| --- | --- | --- | --- |
| render_tui.rs:488–498 | `Completed`=绿 / `Aborted`=黄 / `Error`=红 | 语义 | 严重度三档色板（与 `Severity` 对齐） |
| render_tui.rs:500 | 空名册时名字 = `DarkGray` | 语义 | 色板 `NARRATION` |
| render_tui.rs:522–531 | 讨论者 1 = `LightCyan` | 语义 | `DEBATER_1` |
| render_tui.rs:532–541 | 讨论者 2 = `LightMagenta` | 语义 | `DEBATER_2` |
| render_tui.rs:544–553 | 执行者 = `LightYellow` | 语义 | `EXECUTOR` |
| render_tui.rs:554–565 | 用户 = `LightGreen` | 语义 | `USER` |
| render_tui.rs:566–570 | `Notice` 无发言者 = `DarkGray` | 语义 | `NARRATION` |
| render_tui.rs:589 | 名册外的新人拿 `LightMagenta` 槽 | 语义 | 槽位分配逻辑，值随色板 |
| render_tui.rs:590–594 | 同一名字的行颜色不漂 | 语义 | 关系断言 |
| render_tui.rs:622–626 | 工具调用行不带 bg | 语义 | 属性断言 |
| render_tui.rs:704–711 | 答案里的 Markdown h1 = `Cyan` + `BOLD` | 语义 | 色板 `HEADING` |
| render_tui.rs:739–743 | 权限裁决 = `DarkGray` | 语义 | `NARRATION` |
| render_tui.rs:745–750 | 权限询问 = `DarkGray` | 语义 | `NARRATION` |
| render_tui.rs:759–763 | 答案正文 `!= DarkGray` | 语义 | 关系断言 |
| render_tui.rs:1178–1186 | 沙箱行 = `DarkGray` | 语义 | `NARRATION` |

### 1.3 `tests/render_markdown.rs`（20 处）

| 文件:行号 | 断言的是什么 | 类别 | 改法 |
| --- | --- | --- | --- |
| render_markdown.rs:55 | h1 `BOLD` | 语义 | 不动 |
| render_markdown.rs:56 | h1 fg = `Color::Cyan` | 语义 | 色板 `HEADING` |
| render_markdown.rs:60 | h3 `BOLD` | 语义 | 不动 |
| render_markdown.rs:61 | h3 `!= Cyan` | 语义 | 关系断言 |
| render_markdown.rs:71–72 | 行内 `BOLD` / `ITALIC` | 语义 | 不动 |
| render_markdown.rs:73 | 行内 code = `Color::Yellow` | 语义 | 色板 `CODE` |
| render_markdown.rs:77 | 链接 `UNDERLINED` | 语义 | 不动 |
| render_markdown.rs:99 | 代码块里的 `# not a heading` 不上 `Cyan` | 语义 | 负例 |
| render_markdown.rs:100 | 同上不 `BOLD` | 语义 | 负例 |
| render_markdown.rs:120 | 引用条 fg = `Color::Gray` | 语义 | 色板 `MUTED` |
| render_markdown.rs:139 | 词内下划线不 `ITALIC` | 语义 | 不动 |
| render_markdown.rs:178 / 184 | 表头 `BOLD`（整行每个 span） | 语义 | 不动 |
| render_markdown.rs:185 | 数据行 `!BOLD` | 语义 | 不动 |
| render_markdown.rs:265 | 未知语言代码 `style == Style::default()` | 语义 | 属性断言 |
| render_markdown.rs:308 | 关键字 span `== Class::Keyword.style()` | 语义 | **已经是命名常量**（高亮层） |
| render_markdown.rs:393 | 图片 alt `UNDERLINED` | 语义 | 不动 |
| render_markdown.rs:476 | 格内链接 `UNDERLINED` | 语义 | 不动 |
| render_markdown.rs:499 | 格内 code = `Color::Yellow` | 语义 | `CODE` |

### 1.4 其余文件

| 文件:行号 | 断言的是什么 | 类别 | 改法 |
| --- | --- | --- | --- |
| render_editor.rs:491 | 折行后 chip span 的 fg 仍是 `Magenta`（样式保持） | 语义 | 值来自夹具（387/480 也是 `Magenta`），测的是「保持」而不是那个值 |
| wording.rs:1396 | `TOKEN_COMMAND != TOKEN_REFERENCE` | 语义 | 已命名常量，是样板 |
| wording.rs:1397–1400 | `TOKEN_COMMAND != Color::Reset` | 语义 | 属性「上色不是空操作」 |
| wording.rs:1402–1405 | `TOKEN_REFERENCE != Color::Reset` | 语义 | 同上 |
| render_highlight.rs:61 | `ansi_line("+added", true)` 前缀 `\x1b[32m` | **色值** | diff 层的 ANSI 表（highlight.rs:175–181） |
| render_highlight.rs:62 | `-removed` 前缀 `\x1b[31m` | **色值** | 同上 |
| render_highlight.rs:63 | `@@ hunk @@` 前缀 `\x1b[36m` | **色值** | 同上 |
| render_plain.rs:391 | 好的收尾行前缀 `\x1b[32m[kimi] ` | **色值** | 走 `Severity::ansi`（见 §2.2） |
| render_plain.rs:395 | 中断行前缀 `\x1b[33m[kimi] ` | **色值** | 同上 |
| render_plain.rs:399 | 错误行前缀 `\x1b[31m[kimi] ` | **色值** | 同上 |

### 1.5 零命中的重点文件

- `tests/ask_user_question_tui.rs`：**0 处**颜色/修饰符断言（只有 `NumberStyle`、`KeyModifiers`）。
- `tests/history_replay.rs`：**0 处**（同上）。
- `tests/todo.rs`：**0 处**颜色/修饰符断言；但它与 `render_layout.rs:3119/3142/3184/3263` 一起
  钉住 `todo` 页的**字形**（`☐`/`▸`/`✓`，wording.rs:1643–1645）与降级（`＋N 项`、只有一行时只显示
  计数）——字形改造时这些会红，属于票面第 3 项的范围。
- `tests/render_console.rs`、`render_delivery.rs`：0 处。

### 1.6 总数统计

**按文件**（处 = 断言站点）：

| 文件 | 语义 | 色值 | 合计 |
| --- | --- | --- | --- |
| `tests/render_layout.rs` | 39 | 3 | 42 |
| `tests/render_markdown.rs` | 20 | 0 | 20 |
| `tests/render_tui.rs` | 17 | 0 | 17 |
| `tests/wording.rs` | 3 | 0 | 3 |
| `tests/render_highlight.rs` | 0 | 3 | 3 |
| `tests/render_plain.rs` | 0 | 3 | 3 |
| `tests/render_editor.rs` | 1 | 0 | 1 |
| `tests/ask_user_question_tui.rs` / `history_replay.rs` / `todo.rs` / 其余 | 0 | 0 | 0 |
| **合计** | **80** | **9** | **89** |

**按类别**：钉语义 80、钉色值本身 9。
9 处色值是：`render_layout.rs:870`（提示符 RGB）、`render_layout.rs:1021–1034`（退场色环清单，2 处）、
`render_highlight.rs:61–63`（diff ANSI）、`render_plain.rs:391/395/399`（严重度 ANSI）。

**机械口径对照**（凡出现 `Color::`/RGB/ANSI 字面量即算色值）：语义 36、色值 53。
分歧集中在 §1.1/§1.2/§1.3 里那些「角色色」的相等断言（如助手 == `LightCyan`）——票面口径下它们
是语义（改成色板常量即可），机械口径下它们是色值。

> 提醒：`tests/render_plain.rs:391–399` 是 **plain 可见输出**的一部分，map §11 冻结「plain /
> headless 的可见输出不动」。色板收敛若要改 `Severity::ansi`，这三条会红，且属于需要重新确认的
> 边界，不要顺手改。

### 1.7 退场死代码与钉着它的断言（票面「接受的边界」要求）

| 死代码 | 位置 | 无生产消费者？ | 钉它的断言/测试 |
| --- | --- | --- | --- |
| `PULSE_PALETTE` | tui.rs:4223（仅 mod.rs:60 再导出） | 是 | render_layout.rs:1001–1037（环是 6 个亮色 + 屏幕上没人穿） |
| `mark_lines` 的 `frame` 参数 | tui.rs:4286；生产只传 `None`（3785） | 参数分支死 | tui.rs:6391–6421（单测 `Some(frame)`）；tui.rs:6397–6421 的大段注释 |
| `DASH_FALL` / `identity_falling` | wording.rs:1589 / 1597；3802 只是注释 | 是 | tests/wording.rs:668–704（phase 循环 + 周期端点） |
| `DiffTag` / `diff_tag` / `highlight_diff` / `ansi_line` | highlight.rs:164–206 / 403–470 | 是（highlight.rs:9–10 自己也这么写） | tests/render_highlight.rs:49–94 |
| `panel.rs` 的两条宽度降级路 | panel.rs:80–118（`context_pair` 降一档、`cache_pair` 放不下就不画） | 是（panel.rs:8–12 注释承认「到不了」） | render_layout.rs:2462 `a_cache_split_too_wide_for_its_column_is_left_out`、2644 `a_number_too_wide_for_the_value_column_no_longer_needs_the_bare_form` |

---

## 2. 公开契约

### 2.1 `render_block` / `render_block_uncoloured` 的调用方

定义与再导出：

- `src/render/tui.rs:5022` `pub fn render_block`；`tui.rs:5031` `pub fn render_block_uncoloured`
  （内部转调 `render_block`，5032）。
- `src/render/mod.rs:59` 把两者与 `SpeakerColors` 一起再导出。
- 两者都只是 `paint_block(..., SHARED_RENDER_WIDTH=80, Viewport::Conversation)`（tui.rs:4975、5023）。

调用方：

| 调用方 | 位置 | 说明 |
| --- | --- | --- |
| `tests/render_tui.rs` | 13（import）、474/478/482、609/629/645/660/683/696/733/745/752/770/786/811/946/1179（`render_block_uncoloured`） | 测试 |
| `tests/render_tui.rs` | 505（import）、512、575、582（`render_block`） | 测试 |
| `src/render/tui.rs:5032` | `render_block_uncoloured` → `render_block` | 内部 |
| TUI 真实绘制 | `paint_block` 在 tui.rs:1891（对话）、1904（轨迹）被直接调用 | **不是**这两个公开函数 |

**结论**：`render_block` / `render_block_uncoloured` 今天**没有任何生产调用方**——真正上屏的是私有的
`paint_block`。这两个公开函数事实上是一层「给测试用的共享渲染」。色板/字形改造若从 `paint_block`
下手，`tests/render_tui.rs` 的 20 个调用点会一起受影响；若保留这两个包装，它们的形状可以不动。

### 2.2 `Severity::ansi` 服务哪些路径；与 TUI `severity_style` 的绑定

- 定义：`src/render/severity.rs:46–53`（Good `\x1b[32m`、Note `\x1b[36m`、Warn `\x1b[33m`、
  Bad `\x1b[31m`），复位常量 `severity.rs:56`。
- **唯一生产消费者**：`src/render/plain.rs:356`（`paint_severity` 包 `severity.ansi()` +
  `Severity::ANSI_RESET`）。plain.rs 的调用点是 92/120/172/187/191/195/284。
- 注意不要混淆：`render/highlight.rs` 里的 `Class::ansi`（124）与 `DiffTag::ansi`（175）是**另一套
  独立的 ANSI 表**，服务 `ansi_line`（460–470），与 `Severity::ansi` 无关。
- 分类（谁 → 哪一档）：`Severity::of`（severity.rs:31–42）是唯一的判据。
- TUI 那份映射：`severity_style`（tui.rs:5329–5338），消费者是 `severity_line`（5312）与
  `severity_speaker_line`（5318–5326）。

**今天有没有显式绑定？** 有**一处间接**绑定、**没有**任何直接绑定：

- 间接：两边都调 `Severity::of`（severity.rs:31 / tui.rs:5330），所以「哪个 `StopReason` 属于哪一档」
  是共用的。
- 直接：**没有**。`Severity::ansi`（severity.rs:48–51）与 `severity_style`（tui.rs:5331–5337）是两张
  各写各的字面量表，既没有共享常量，也没有一条测试把两边对上。二者已经有一处**可见的漂移候选**：
  TUI 的 `Bad` 多一个 `Modifier::BOLD`（tui.rs:5334–5336），plain 的 `Bad` 只有红色。这是色板收敛
  （把「一档严重度」收成一个语义条目、再由两个画家各自着色）的现成落点。

### 2.3 `markdown::to_lines_indented` 的列预算契约

定义与文档：`src/render/markdown.rs:43–52`；`to_lines(text, width)` 是 `indent = 0` 的包装（39–41）。

契约（逐字）：

- `width` = 转录内容的**可用列数**（markdown.rs:12–13、57–58 `Renderer::new(width.max(1), indent)`）。
- `indent` = 调用方会在**第一行**前面加的前缀占的列数（markdown.rs:43、120）。
- **需要左边界对齐的块**（表格、代码块）每一行都从第 `indent` 列起，宽度预算 = `width - indent`；
  调用方把第一行那 `indent` 个空格**换成**前缀（markdown.rs:47–49）。
- **其余块**（段落、标题、列表、引用）照常从第 0 列吐，续行顶格、占满整个 `width`（markdown.rs:50–51）。

实际传参：

| 调用方 | 位置 | 传的 `indent` |
| --- | --- | --- |
| TUI 轨迹视图（窄/宽档） | tui.rs:5060–5063 | `prefix_columns(speaker, style)`，即 `[name] `/`name ` 的列数 |
| TUI 对话视图 | tui.rs:5084–5086 | **`0`**（2026-10-05 排版修订后名字独占一行，前缀不再占正文列） |
| 测试 | render_markdown.rs:436、454 | 7 |

**契约边角**：`indent` 大于 `width` 时 `width - indent` 靠 `saturating`/`max(1)` 兜底（`width.max(1)`；
表格列宽另有下限）。契约本身没写「`indent ≥ width` 会怎样」，改造时如果要收紧，先把这条补上。

---

## 3. 文档与脚本锚点

### 3.1 `docs/render.md` 里描述具体颜色或字形的小节

| 小节（标题行号） | 描述颜色/字形的行 | 具体内容 |
| --- | --- | --- |
| 「一个 trait，三个实现」L7 | L15 | TUI 描述含「一圈外框」（**矛盾点，见 §4 C1**） |
| 「一个呈现层，两个画家」L26 | L47–49、L55–57 | `[name]` 前缀独占一行 vs 跟首行；`to_lines_indented` 的 `indent` |
| 「严重度」L59 | L61–64 | plain→ANSI、TUI→ratatui `Style`；四档归属 |
| 「高亮」L66 | L78–84 | diff 标记剥贴、前景/背景两个身份、退纯文本「不上色不消失」 |
| 「外壳」L90 | L93–95、L97–105、L114–116、L117–124、L136–138、L142 | 竖虚线 + 两条横虚线；ASCII 图 `┆`/`┄`；logo 五行字形与色相坡道；下落短横/色相环/`PULSE_PALETTE`；todo 字形 `☐`/`▸`/`✓` 与 `＋3 项`；外壳 4 行（**枚举残缺，见 §4 C6**） |
| 「两个视图」L149 | L159–162、L169–170 | 轨迹页宽档 40/窄档 28、`首行 + …`；三类前缀色 `LightBlue`/`LightGreen`/助手=调色板首位 |
| 「键盘」L178 | L187–188 | `❱` 脉冲与 `PROMPT_HUE_PER_SECOND`（**计时器数量错误，见 §4 C3**） |
| 「问卷：区域与键位」L216 | L274–277 | 页脚三档降级、`回车 提交/下一题`、`已跳过` 可见性 |
| 「输入框里的记号」L280 | L283–286、L291–294 | 「能兑现」判据同时管上色与 chip；`/` vs `@` 边界表 |
| 「挂起与恢复」L325 | L334、L338 | `CSI 23 t`、`ratatui::restore()` 等转义序列 |
| 「重新打开会话：历史重播」L353 | L366–368、L372 | `恢复历史 n/m` 三档；`── 以上为历史 ──` |
| 「交互式 CLI」L384 | — | 无具体颜色/字形 |

### 3.2 `docs/tui-manual-checklist.md` 的观感项

文档实际有 **①–㉙**（票面写的是 ①–㉘；`㉙ 轨迹视图` 是后来加的，标题在 L755）。判定分三档：
**纯观感**＝整节只有眼睛能判；**观感为主**＝节里有成块的纯观感条目；**功能为主**。

| 小节 | 标题（行号） | 判定 | 纯观感条目（真终端重走的最小集） |
| --- | --- | --- | --- |
| ① | 光标 L30 | 功能为主 | 无（①.5 的颜色指向 ⑯） |
| ② | 权限模态 L45 | 功能为主 | ②.2 候选键行水平居中 |
| ③ | 鼠标 L57 | 功能为主 | **③.8 角色配色**（五色 + 正文语义色） |
| ④ | resize L87 | 功能 | 无 |
| ⑤ | 粘贴 L102 | 功能 | 无 |
| ⑥ | Ctrl-J 与 Shift-Enter L110 | 功能 | 无 |
| ⑦ | 退出后终端干净 L117 | 功能 | 无 |
| ⑧ | 忙碌 Ctrl-C L152 | 功能 | 无 |
| ⑨ | 120×24 的几何（已自动化）L159 | 功能（明确不手工走） | 无 |
| ⑩ | 左栏：mark / 文字身份 / 隐藏三档 L163 | **观感为主** | ⑩.1 标记字形/渐变/居中；⑩.4 浅色主题无背景色块 |
| ⑪ | `/` 菜单 L181 | 混合 | ⑪.2 高亮反白（黄底黑字）；⑪.6 框的形状/位置 |
| ⑫ | 折叠提示与详情覆盖层 L208 | 混合 | ⑫.2 覆盖层屏幕居中、宽度、边框色、留 1 格 |
| ⑬ | 鼠标作答的边界 L238 | 功能 | 无 |
| ⑭ | `--continue` 重开 L246 | 混合 | ⑭.5 大会话启动手感 |
| ⑮ | 外壳改版：左栏 tab、状态行、回合条 L270 | **观感为主** | ⑮.1 tab 亮品红/暗灰；⑮.4 回合条 `┊`/`┃` 字形与焦点色；⑮.6 提示行读起来够不够；⑮.7 宽度跳变 |
| ⑯ | 输入区三行、提示符色相与静止的左栏 L303 | **观感为主** | ⑯.4 提示符颜色/呼吸、「不该闪」；⑯.5 `❱` 模糊宽度字形；⑯.6 左栏完全静止；⑯.9 切换不跳 |
| ⑰ | 模式循环与 `todo` 标签 L339 | 混合 | ⑰.3 `☐`/`▸`/`✓` 字形；⑰.5 窄档读起来清不清楚 |
| ⑱ | 沙箱：内核真的拦住了 L367 | 功能 | 无 |
| ⑲ | `workspace` 档与升级手势 L421 | 混合 | ⑲.3 弹窗那几行读起来够不够；⑲.9 状态行 |
| ⑳ | 外壳收干净 L459 | **观感为主** | ⑳.1 四边无框线；⑳.2 横线两条不是三条；⑳.3 三层虚线分得清；⑳.4 `CHROME_LINE` 深度；⑳.5 详情屏幕居中；⑳.9 左栏顶上那一行空行；⑳.10 竖线贯通 |
| ㉑ | 转录里的 Markdown L494 | **观感为主** | ㉑.1 表头粗体/列对齐/撑满；㉑.3 语言名右对齐 + 两格缩进；㉑.4 高亮分色 + 续行缩进；㉑.5 十种语言都有非默认色；㉑.7 续行顶格 |
| ㉒ | 目标循环 L526 | 混合 | ㉒.5 翻页那一刻的观感 |
| ㉓ | 终端标题 L551 | 功能 | 无 |
| ㉔ | 挂起 L580 | 功能 | 无 |
| ㉕ | 问卷：区域、折行与两把举手 L611 | 混合 | ㉕.2 输入区时高亮降暗/不反显；㉕.5 长选项折行与续行对齐 |
| ㉖ | 左栏开关 L644 | 混合 | ㉖.2「那 41 列换回来的值不值」；㉖.6 提示行最末 `ctrl-o 左栏` 出不出现 |
| ㉗ | 问卷的回车 L673 | 功能为主 | ㉗.8 `已跳过` 与举手回执在任何宽度下都在（可读性） |
| ㉘ | 输入框里的记号 L710 | 观感为主 | ㉘.4 命令色/引用色/普通文本色分得清、变色时机对 |
| ㉙ | 轨迹视图 L755 | **观感为主** | ㉙.2 28 列读不读得下去；㉙.7 轮次分隔线与无底色；㉙.11 名字独占一行；㉙.12 等待提示的形状 |

**纯观感重点节（改造后必须重走）**：③.8、⑩、⑮、⑯、⑳、㉑、㉘.4、㉙。
注意 ⑳.4 与 ③.8 是**随终端配色而变**的两处，任何色值收敛都要在这两处留下实测记录。

### 3.3 `scripts/tui-startup-check.py` 钉了哪些字形/颜色锚点

字形锚点（会被色板/字形改造打破）：

| 锚点 | 定义 | 使用 | 判据 |
| --- | --- | --- | --- |
| `MARK_ROW = "▄▀▀█"` | L76 | L738 | 左栏身份：文字身份**或**该标记至少出现一种 |
| `BORDER_H = "┄"` | L81 | L740–746 | 全屏横虚线总数 ≥ 3 |
| `BORDER_V = "┆"` | L82 | L740–746 | 全屏竖虚线总数 ≥ 3 |

文本锚点（不是字形/颜色，改造不该动）：`STATUS_ANCHOR = "ctrl-c"`（L65）、
`BANNER_ANCHOR = "fs-agent："`（L68）、`REPLAY_PROGRESS_ANCHOR = "恢复"`（L71）。

协议锚点（转义序列，不是颜色）：`TEARDOWN`（L90–98）、`TITLE_SAVE`（L101）、`TITLE_SET`（L102）、
`ALT_ENTER`（L106）、`CLEAR_ALL`（L109）。

**颜色锚点：一个都没有。** 屏幕仿真在 L200 的正则里把 `\x1b[...m`（SGR）整段剥掉，之后没有任何一处
读颜色。也就是说：**改色板不会让这个脚本红**（除非同时改了 `┄`/`┆`/标记字形或虚线数量）；
换字形/改虚线密度则会直接红在 L738–746。

---

## 4. `docs/render.md` 与代码的矛盾复核

charting 说 6 处。复核结果是 **6 处落点、归 4 类漂移**：其中 2 处文档、2 处源码注释、1 处 tab
注释、1 处行数枚举。全部判定为**真**。

| # | 位置 | 文档/注释原话 | 代码事实 | 正确的事实 | 判定 |
| --- | --- | --- | --- | --- | --- |
| C1 | **docs/render.md:15** | 「ratatui 界面：备用屏幕（alt screen）上的一圈**外框**，框住一条全高左栏与一条主列」 | 外框已拆：`layout::plan` 的 `screen` 就是终端（layout.rs:302「内容区就是终端：外框已经离开」、338 `screen: area`）；tui.rs:3710「**外框已经不在**」；tui.rs:5–6 模块文档也这么写 | 「alt screen 上一条全高左栏 + 一条主列，中间一条竖虚线，主列里两条横虚线；没有外框」 | **真** |
| C2 | **src/render/tui.rs:15** | 「循环里**唯一的定时器**是标记的脉冲」 | 循环里有**两个**按需武装的定时器：60 ms 脉冲（tui.rs:397、`PULSE_FRAME` 4241、`prompt_colour` 4163）与 500 ms 退出手势 deadline（tui.rs:392、`GESTURE_WINDOW` 4256、`exit_deadline()` 437–439）。同文件 385–396、426–432 已把两个都写清楚 | 「循环里有两个按需武装的定时器：运行中的脉冲，与举着手时的退出手势 deadline」 | **真** |
| C3 | **docs/render.md:186–192** | 「在 broadcast / console port / 键盘上加**一个计时器**」「没有别的东西在等着被注意到……空闲时那条分支被守掉，这个 `select!` 又只剩三个源」 | 同上，两个定时器；正确说法在 tui.rs:426–432。空闲时确实只剩三个源（两个分支各自有 `if` 守卫），所以后半句对、前半句错 | 「加**两个**按需武装的计时器（脉冲 + 退出手势 deadline）；两者都只在各自条件下 arm，所以空闲时仍只有三个源」 | **真**（与 C2 同源） |
| C4 | **src/render/layout.rs:7** | 「外壳是**一圈外框**、一条全高左栏、一条主列：外框是终端的边框」 | 同一文件 29「外框已经离开（spec §1），内容区就是终端本身」、302、387；`BORDER_COLUMNS` 只服务浮层（29–32、427–428） | 「外壳是一条全高左栏 + 一条主列；外框已拆，内容区就是终端本身」 | **真**（与 C1 同源，注释内部自相矛盾） |
| C5 | **src/render/tui.rs:3957** | `Tab::Trace` 的文档注释：「/// 调用轨迹。还没做。」 | 轨迹页已实现：`draw_sidebar_page` 的 `Tab::Trace` 分支（tui.rs:3823–3836）、页签条 entries（3884）、`selects`（5481）、以及 `tests/render_layout.rs` 的轨迹页断言（6236–6442）。其上的 3816/3944 也早就改成「还没做出来的页面」 | 「调用轨迹页：转录的第二个视图」；对照 `Tab::Files`（3959）才真是「还没做」 | **真** |
| C6 | **docs/render.md:142** | 「外壳占 4 行：**主列的分隔线**、状态行与提示行」 | 4 行的构成是**两条**横虚线 + 状态行 + 提示行：`CHROME = 4` 的文档（layout.rs:20–26）明说「主列的两条分隔线、状态行与提示行」；plan 里 status 与 hints 各上方留一行（layout.rs:323–329）；docs/render.md:93 自己写「主列里那两条横虚线」；tui-manual-checklist.md:471–472「那两条横线还在（一共两条，不是三条）」 | 「外壳占 4 行：主列的**两条**横虚线、状态行与提示行」 | **真**（枚举漏了「两条」） |

**同源关系**：C1/C4 是同一类漂移（外框）；C2/C3 是同一类漂移（计时器数量）。如果 `/to-spec`
按「漂移」而不按「落点」计数，这是 4 类；按落点是 6 处。

### 4.1 复核为「需澄清」的候选（未计入 6 处，供 `/to-spec` 决定收不收）

| 位置 | 疑点 | 现状 | 建议 |
| --- | --- | --- | --- |
| docs/render.md:55–57 | 「`indent` 是消息第一行那个 `[name] ` 占的列数」 | 只有**轨迹视图**那样传（tui.rs:5060–5063）；**对话视图**在同一轮排版修订后传 `0`（tui.rs:5084–5086，注释明说「前缀不再占正文的列」）。函数契约（markdown.rs:43–52）本身没错，错在把一种调用方的约定写成了普遍用法 | 改成「调用方按自己的前缀形态传：轨迹视图传 `[name] ` 的列数，对话视图传 0」 |
| docs/render.md:274–275 | 「窄终端按**三档**先丢 ctrl-n/ctrl-p 别名、再砍到只剩 `esc 退出询问`」 | `wording::questionnaire_hint`（wording.rs:913–931）实为**四档**：≥71 全量（带别名）、≥48 丢别名、≥12 只剩 `esc 退出询问`、<12 **空串** | 补上第四档「再窄就什么都不画」，或明确「三档」不含空串 |
| docs/render.md:155–158 | 对话视图「只画用户文本、assistant 正文」 | `selects`（tui.rs:5481–5509）对**任何非执行者的 `Block::Message`** 都放行，含 `SpeakerId::System` 的合成器消息；文档自己在 L49 也承认「别的系统行」 | 写成「用户文本、assistant 正文与系统消息」 |
| docs/render.md:130–138 | todo 项行「`☐`/`▸`/`✓` 加上内容」 | 带 `id` 的项画成 `✓ 03 补测试`（todo.rs:100–107 `item_line`） | 补一句「有 id 的项多一段 id」 |

### 4.2 复核为「不矛盾」（曾疑似）

- docs/render.md:70–71「diff 层仍然没有调用方」：属实，`diff_tag`/`highlight_diff`/`ansi_line` 在
  `src/` 里除 highlight.rs 自身外无调用方（见 §1.7）。
- docs/render.md:117–124「两者都在它们所在的地方有单元测试」：属实（`mark_lines` 在
  tui.rs:6391–6421；`DASH_FALL`/`identity_falling` 在 tests/wording.rs:668–704）。
- docs/render.md:127–128「占位页上，状态行的 `上下文 n%` 是唯一剩下的读数」：与 tui.rs:3947–3949
  的注释逐字一致。
- docs/render.md:366–368 的 `恢复历史 n/m` 三档：与 wording.rs:1259–1296 完全一致。
- docs/render.md:107–113 的宽度阈值（120→40、80→28、<80 隐藏）、41 列、`SIDEBAR_FIELDS` 已退休：
  与 layout.rs:49–58、76、372–392 一致。

---

## 附：本次清点的边界

- 只读了 `tests/`、`src/render/`、`docs/render.md`、`docs/tui-manual-checklist.md`、
  `scripts/tui-startup-check.py` 及少量交叉引用，未跑 `cargo test`（票面说验收基线在 `/to-spec`
  之前实测）。
- 第 1 节只覆盖**颜色与修饰符**（票面口径）；字形断言（`☐`/`▸`/`✓`、logo、`┄`/`┆`、`❱`）
  散落在 wording/todo/render_layout 与脚本里，已在 §1.5、§3.3 点名，供 04 号字形票接。
- 第 4 节的 6 处落点是按票面已知的两处 + 本次核查补出的四处给出的；若 charting 当时手上另有一份
  清单，请以本表的 `文件:行号` 为准逐条对账。
