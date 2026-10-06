# 文件页的详情覆盖层与语法高亮接线（票 03 的 findings）

> 这是 [`.scratch/files-page/issues/03-research-overlay-and-highlight.md`](../issues/03-research-overlay-and-highlight.md)
> 的 findings：把「文件内容落在既有的详情覆盖层上、带语法高亮与行号、由渲染器直接读盘并有界截断」
> 这条路上**代码今天长什么样**查清。**只读侦察，写就于 2026-10-06**，基线 commit `a09d62c`；
> 没有改 `src/` 下任何文件，也没有跑构建。下面每一条论断都带 `文件:行号`，查不到的写在文末
> 「查不到的条目」，不做推断。

## 结论摘要

1. **详情覆盖层是一份「打开那一刻排好版」的静态正文加一个 `top` 偏移**：正文由 `detail_body`
   （`src/render/tui.rs:6206`）排成 `Vec<DetailLine>`，用 `folded_text`（`tui.rs:6114`）→
   `pane::wrap_line`（`src/render/pane.rs:428`）折行 —— **不是** `pane::wrap_text`。绘制函数
   `draw_detail`（`tui.rs:6328`）只做裁剪与滚动算术，不再碰磁盘、不再折行。
2. **`DetailKind` 只有四处构造、一处消费**：变体定义在 `tui.rs:6040`，四个构造点在
   `tui.rs:2496 / 5636 / 5776 / 5849`，唯一的 switch 是 `detail_body`（`tui.rs:6208`）。加第五个
   变体是「定义 + 那个 match + 一条新入口调 `open_detail`」三处，但 `open_detail` / `close_detail`
   里有一处**打开方恒为轨迹页**的硬编码记账（`tui.rs:6151`、`6168`）要一起想清楚。
3. **高亮层的入口是「整段源码 + 语言名 → 逐行的 `Vec<Vec<Span>>`」**：`highlight_code`
   （`src/render/highlight.rs:344`），语言今天只由 markdown 围栏的 info string 第一个词给定
   （`src/render/markdown.rs:188-196`、`635`、`649`），产物是带 `Class` 的 span，`Class::style`
   （`highlight.rs:148`）给出 TUI 样式。**按扩展名挑语言的映射今天不存在**。
4. **行号只画第一条这件事有现成形状可借，但要借对那一层**：`stamp_lines`（`tui.rs:5693`）是
   「整个块只给第一条显示行插前缀」的块级形状；文件页要的是**逻辑行级**，而 `folded_text`
   先按 `\n` 拆逻辑行、再逐条 `wrap_line` 的顺序（`tui.rs:6114-6130`）正好允许在折行前插前缀，
   折出来的续行自然不带行号。另外 `trace-in-main/spec.md` §5 说的「续行补 9 个空格」在代码里
   **查不到**实现。
5. **「不进事件流、不进模型上下文」成立**：渲染层对文件系统的唯一调用是 `read_tool_body` 里
   那一次只读 `std::fs::read_to_string`（`tui.rs:6311`），且只在打开覆盖层那一刻发生
   （`tui.rs:6155`）；`outputs/` 的写全在工具结果进流的那条流水线里
   （`src/agent.rs:2075-2098` → `src/context.rs:351-355`），渲染器不 emit 事件、不碰 `Session`。
   有界读盘的既有件里，`read_file` 那一套**绑死在工具层**（要 `ToolContext`），能借的是渲染层
   自己的 `read_tool_body` + `DETAIL_MAX_CHARS`，但它只按字符封顶，且绑死 `outputs/<id>.txt`
   这条命名。

---

## 1. 详情覆盖层今天怎么画，加第五个变体要动哪几处

### 1.1 四个变体与三份结构

| 东西 | 位置 | 形状 |
| --- | --- | --- |
| `DetailKind` | `src/render/tui.rs:6040-6061` | 四个变体：`Thinking { text: Option<String> }`、`Context { source, content }`、`Message { text }`、`Tool { tool_call_id, output, error, args, no_result }` |
| `Detail` | `src/render/tui.rs:6028-6036` | `title: String` + `color: Color` + `kind`。是 `pub struct` 但**字段私有**，只有本模块能构造 |
| `DetailView` | `src/render/tui.rs:6079-6088` | `detail`、`body: Vec<DetailLine>`、`top`、`height`。注释写明了：`body` 是**按它被打开时的宽度**排好的 |
| `DetailLine` | `src/render/tui.rs:6095-6108` | 一条显示行 + `folded: bool`（它是不是上一行折出来的续行）。`folded` 的唯一消费者是拖选复制 |

`Detail` 的四个构造点（每个都把「被点那一行自己的文字」当标题）：

- `tui.rs:2496-2501` `Thinking`（结算一条思考行时；`title = line_text(&line)`）
- `tui.rs:5636-5643` `Context`（上下文注入行；`title = 注入文本`，颜色 `palette::INJECTED`）
- `tui.rs:5776-5792` `Tool`（工具调用行；`title = line_text(调用行)`，`args` 原样带下去）
- `tui.rs:5849-5855` `Message`（轨迹页的消息行；`title = line_text(&head)`）

**唯一的消费点是 `detail_body` 的穷举 match**（`tui.rs:6208-6275`）—— 全仓库再没有第二处
match `DetailKind`（`detail_body` 之外只有文档注释提到它：`src/discussion.rs:103`、
`src/agent.rs:122`；`tests/` 下没有引用）。

### 1.2 正文从哪一层拿到可画的行 —— 不是 `wrap_text`

链条是：**`DetailKind` 里已经是字符串**（或 `Tool` 的 `output` 预览）→ 打开时
`detail_body(&detail, session_dir, width)`（`tui.rs:6206`）→ 每条正文段走
`folded_text(text, width)`（`tui.rs:6114-6130`）→ 每条逻辑行调 `pane::wrap_line`
（`src/render/pane.rs:428`）→ 得到 `Vec<DetailLine>`。

`pane::wrap_text`（`pane.rs:407-416`）是同一族的另一支：它把文本按 `\n` 拆成逻辑行各折一次，
但**丢掉「哪些是折出来的续行」这个信息**。它用在菜单与问卷那一带（`tui.rs:3809`、`3821`、
`4349-4363`），详情这条路刻意不用它。

两个小节标题与降级说明行**不经折行**，直接 `DetailLine::plain`（`tui.rs:6210`、`6216`、`6244`、
`6249`、`6253` 等）；正文里的换行按逻辑段判，不按显示行判（`tui.rs:6112-6113` 的注释）。

`Tool` 那一支的正文还多一道读盘：`read_tool_body`（`tui.rs:6293-6322`，见 §4）。

### 1.3 尺寸上限与滚动各在哪一层

| 层 | 位置 | 事实 |
| --- | --- | --- |
| 覆盖层的尺寸上限 | `src/render/layout.rs:483-492` | `DETAIL_MAX_WIDTH = 135`、`DETAIL_MIN_ROWS = 1`、`DETAIL_MARGIN_ROWS = 2` |
| 宽度怎么算 | `layout.rs:205-210` | `detail_width()` = 屏幕宽 − 4（`MODAL_MARGIN`，`layout.rs:481`），再 `min(135)`。基准是**整屏**而不是主列 |
| 框去哪儿 | `layout.rs:220-235` | `Regions::detail()`：在屏幕上居中，高度 = 屏幕高 − 3；太小时返回 `None`（那时视图干脆不打开） |
| 内边距与文本区 | `tui.rs:6351-6362` | `inner` 再各缩 `DETAIL_PADDING = 1`（`tui.rs:6133`）：`pad_x`、`pad_y` 只在空间够时让出一列/一行 |
| 一次能显示多少行 | `tui.rs:6365` + `6420` | `body_rows = text.height - 2`（标题一行、页脚一行），算完回写 `view.height` |
| 滚动位置 | `tui.rs:6085`（`top`）+ `6184-6190`（`detail_scroll`）+ `6366-6367`（绘制时 clamp） | `top` 是显示行下标，`max_top = body.len() - height` |
| 翻页步长 | `tui.rs:6193-6198` | 一页 = 它自己的高度减一行重叠，至少 1 |
| 键盘 | `tui.rs:3156-3167` | 覆盖层立着时独占键盘：`Esc`/`Ctrl-D` 关、`↑`/`↓`/`PageUp`/`PageDown` 滚，其余一律忽略 |
| 滚轮 | `tui.rs:2600-2604` | `wheel_at` 第一支就是它，滚动一格一行 |
| 点击 | `tui.rs:2691-2703` | 框内点击什么都不做（拖选另走一路），**框外点击关掉它**；`detail_rect` 是上一帧真画出来的矩形（`6347`） |

`pane::wrap_line` 折行**数的是显示列不是字节**（`pane.rs:418-427`），CJK 两列；续行从第 0 列起。

### 1.4 画它的那个函数

`draw_detail(frame, &panes, state)`（`tui.rs:6328-6422`），在 `draw_frame` 里**最后画**
（`tui.rs:3971`，在模态覆盖层之后、拖选反白之前）。它做的事按顺序是：拿不到矩形就整份关掉
（`6333-6338`）、冻住打开方（`6340-6345`）、**记 `state.detail_rect`**（`6347`）、算内边距与
文本区（`6351-6362`）、从 `view.body` 里 `skip(top).take(body_rows)` 取要画的窗口
（`6368-6374`）、算页脚（`6377-6380`）、画边框（`6391`）、标题（`6393-6399`，过
`truncate_columns`）、把这一屏的行推进屏幕文本层（`6410`，`note_rows`）、画正文（`6411`）、
画页脚（`6412-6418`）、最后把 `view.height` / `view.top` 回写（`6419-6421`）。

打开侧是 `open_detail(detail, width)`（`tui.rs:6148-6162`）：记 `ScrollMark`、**在这里读盘并排版**
（`6155`）、存进 `state.detail`。今天唯一的调用点在一次落在轨迹页上的点击里：
`tui.rs:2731-2733`，宽度传的是 `panes.detail_width()`。

### 1.5 加第五个变体要动哪几处

1. `DetailKind` 加变体（`tui.rs:6040-6061`）—— 它现在承载的是「正文 + 读出正文所需的元数据」，
   文件页需要的东西（路径、行号口径、截断提示）都得在这里加字段或复用现有字段。
2. `detail_body` 的 match 加一支（`tui.rs:6208-6275`），在那里决定正文怎么来、要不要行号、要不要
   截断说明；措辞按现有做法加在 `src/render/wording.rs:302-395` 那一族 `detail_*` 里。
3. 一条新入口调 `open_detail`（`tui.rs:6148`）：`Detail` 的字段私有，构造与打开都得在 `tui.rs`
   内部；文件页今天没有这样的入口，轨迹行是唯一的。
4. **打开方记账**：`open_detail` 无条件 `detail_opener = ScrollMark { trace.top(), trace.following() }`
   （`tui.rs:6151-6154`），`close_detail` 无条件 `self.trace.set_holding(false)` + 还原
   （`tui.rs:6168-6177`），`draw_detail` 也会 `trace.set_following(false)` / `set_holding(true)`
   （`tui.rs:6342-6345`）。注释明说「今天只有轨迹页会打开详情」。文件页成为第二个打开方时，
   这几处是它必须处理的既有假设（**不需要**动的是键盘/滚轮/点击三处归属：它们只看
   `detail_open()`，`tui.rs:2601`、`2692`、`3156`）。
5. 测试那一层：`tests/render_layout.rs:4750`（点击开关）、`4781`（键与滚轮滚正文）、
   `4824`（读 `outputs/` 里落盘的全文）与 `history_replay.rs:887/961/991/1010/1091` 是钉住
   这条路的既有帧测试；`TuiState::new(facts, cwd, home)` 在测试里被直接构造
   （`tests/render_layout.rs:4837-4852`），所以文件页的弹窗能在同一层断言。

### 1.6 一处既有落差（顺手记下，下游会踩）

**正文的排版宽度用的是「框宽」，而实际文本区比它窄 4 列。** 调用点传的是
`panes.detail_width()`（`tui.rs:2732`），它就是覆盖层**框**的宽度（`layout.rs:205-210`、
`220-235`，含两列边框）；而正文实际画在 `text.width` 上：框内缩 1（`layout::inner`，
`layout.rs:459-466`）再左右各让 1 列内边距（`tui.rs:6355-6362`）—— 即框宽 − 4。`Paragraph`
不折行，所以按框宽排出来的行尾部最多有 4 列会被裁掉。这不是本票要决定的事，但文件页要往每一行
前面加行号时，宽度口径必须先对齐，否则行号本身也会被算错。

---

## 2. 高亮那一层的入口形状

**文件与入口**：`src/render/highlight.rs:344`：

```rust
pub fn highlight_code(language: &str, source: &str) -> Option<Vec<Vec<Span>>>
```

- **吃输入的单位是「整段源码」**，不是逐行：`try_highlight`（`highlight.rs:363-406`）把整段
  交给 `tree-sitter-highlight`，靠 `HighlightEvent::Source` 的切片里出现的 `\n` 自己切成行
  （`highlight.rs:388-401`）。跨行的字符串/注释因此能正确解析。
- **产物是「每行一串 span」**：`Span { text: String, class: Class }`（`highlight.rs:166-170`），
  `Class` 十一档（`highlight.rs:66-78`），每档的 TUI 样式是 `Class::style`（`highlight.rs:148-162`）、
  plain 终端的 ANSI 是 `Class::ansi`（`highlight.rs:129-141`）。**不是** ANSI 字符串，也不是
  ratatui 的 `Span` —— 转成 ratatui `Span` 是调用方的事（见下）。
- **语言怎么定**：今天**只有围栏 info string 一条路**。markdown 那边在
  `Tag::CodeBlock(CodeBlockKind::Fenced(info))` 处取 `first_word(&info)`（`markdown.rs:188-196`、
  `first_word` 在 `markdown.rs:635-637`），把语言名一路带到 `code_lines` → `highlight_code`。
  名字 → 文法的别名表是 `canonical_language`（`highlight.rs:224-238`）：`rust|rs`、`bash|sh|shell`、
  `json`、`toml`、`html`、`javascript|js`、`typescript|ts`、`php`、`sql`、`python|py`，**名单之外
  一律不上色**。**扩展名 → 语言名的映射在本仓库查不到**（`canonical_language` 只处理名字与别名；
  `src/render/file_index.rs` 只列路径，不做语言判断）。
- **markdown 代码块那条调用点**：`markdown.rs:523-528`（`write_code` 里）→ `code_lines`
  （`markdown.rs:643-663`）→ `highlight::highlight_code(lang, text)`（`markdown.rs:649`）。认不出
  语言或文法构建失败时退 `plain_rows`（`markdown.rs:652`、`665-674`）：代码退纯文本，**不消失**。
  每一行接着走 `wrap_code`（`markdown.rs:678-703`），它在折行时**保住代码块那两格缩进**。
- **`highlight_code` 返回 `Option`**：`None` 是「这门语言不上色」的正常降级（`highlight.rs:340-348`）。
- **另外两个入口今天都不是给文件页准备的**：`highlight_rust`（`highlight.rs:353-355`）只被
  `highlight_diff`（`highlight.rs:413-450`）调用，`highlight_diff` 只被 `ansi_line`
  （`highlight.rs:470-490`）调用 —— 也就是 diff/plain 输出那条路，**TUI 里没有调用方**
  （全仓库 `highlight_*` 的调用点只有 `markdown.rs:649` 与这三个函数之间的互调）。
- **文法按需编译**：每种语言一个 `OnceLock`（`highlight.rs:240-254`），查询构建失败按「这门语言
  没有高亮」处理（`highlight.rs:256-262`、`335-338`）。`highlight_code` 是纯同步函数，没有 IO、
  没有会话状态 —— 文件页拿到路径与语言名就能直接调它。

---

## 3. 行号与折行怎么共存：那一层的现成形状

**票里点到的那条先例确实存在，但它是「块」级的。** `stamp_lines`（`tui.rs:5693-5701`）给一个
块画出来的所有行里**只有第一条**插时刻前缀，续行原样；注释（`tui.rs:5687-5692`）说明了理由，
并点明「折行发生在窗格里，所以那些续行从第 0 列起」。它服务的是轨迹页：排版宽度先减掉
`layout::STAMP_COLUMNS`（9 列，`layout.rs:58`；用法 `tui.rs:1985-1998`），前缀由
`wording::stamp`（`src/render/wording.rs:1671-1673`，本地时区 `%H:%M:%S` 加一个尾空格）生成。
单行版本是 `stamped_line`（`tui.rs:5677-5685`），给「还开着的思考行」用。

**文件页要的是「逻辑行级」，对应的现成形状是 `folded_text` 自己的顺序**（`tui.rs:6114-6130`）：
它先 `text.split('\n')` 拆出逻辑行，再对每条调 `pane::wrap_line`，然后按 `index > 0` 标
`folded`。也就是说 —— **在 `wrap_line` 之前给每条逻辑行插一个行号 span，折出来的续行天然不带
行号**，正是想要的形状；不需要新写一个「折行后回溯首行」的机制。行号与正文之间要用同一套
宽度口径（见 §1.6），并且要保证 `folded` 标记仍然标在真正的续行上（`DetailLine::folded` 的
语义是「上一行折出来的」，`tui.rs:6090-6098`）。

**两处要知道的边界条件**：

1. **复制会把前缀一起带走。** 屏幕文本层记录的是每行**全部 span 的文本**（`line_text`，
   `tui.rs:6017-6022`；`note_rows` 在 `tui.rs:4665-4675`），拖选取文本时逐行切列（`selection.rs:199-230`），
   唯一的结构信息是 `folded`（`selection.rs:20-33`）—— 软折续行拼回上一条，硬换行保留。
   所以行号前缀会进剪贴板（轨迹页的时间戳前缀今天就如此）。要避免只有两条路：不把行号放进
   `note_rows` 那一份文本，或在 `line_text` 那一层把它剥掉。
2. **窗口变宽不会重排已打开的正文。** `DetailView.body` 是打开时按当时宽度排好的
   （`tui.rs:6082-6083`、`6155`），重放路径（`rerender_if_width_changed`，
   `tui.rs:2025-2056`）只重放转录的块，**不重排 `detail.body`**。行号是随宽度折行一起定的，
   所以它与正文共享这条限制。

**`trace-in-main/spec.md` §5 写着「同一个块的续行补 9 个空格对齐」—— 这一条在代码里查不到**
（见文末）。

---

## 4. 有界读盘的既有件：哪些能借、哪些绑死在工具层

### 4.1 `read_file` 那一套（工具层）

- 默认窗口 `DEFAULT_READ_LINES = 2000`（`src/tools/file.rs:37-42`）。
- 窗口语义 `ReadWindow { offset, limit }`（`file.rs:57-79`）：`offset` 是**文件里的** 1-based
  行号，留着 `Option` 是为了区分「没给」与「给了 1」（`file.rs:59-71`）；非法值报参数错而不是
  回退默认（`file.rs:81-95`）。
- 输出格式是 `{绝对路径}\n` + 每行 `{行号}\t{正文}\n`（`file.rs:239-242`），行号是文件行号。
- 未读完时末尾一行续读提示 `resume_note`（`file.rs:97-106`）：`（第 {first}–{last} 行，共 {total} 行；续读 offset={last+1}）`；
  它只在还有未读行时出现，与「这条结果被流水线裁过」是两回事（同一处注释）。
- 读窗口落到文件之外是一次**参数错**（`file.rs:225-232`），空文件不指行号时仍然读得动
  （`file.rs:233-234`）。

**为什么借不到渲染层**：它是 `Tool` 的实现，`call` 是 `async fn(&self, ctx: &ToolContext<'_>, args: Value)`
（`file.rs:214-215`），要 `ToolContext`（`src/tools/tool.rs:110-140`：`read_paths` 解析器、
`write_paths`、`outputs_dir`、`cwd`、`skills`、`repo_map`、`bash` 限额、`sandbox`、`executor`、
`question`……），产出还要经 `ToolOutput` 回流。渲染器手里没有这些东西，也不该有（那会把渲染
拉进权限门与工具注册表的世界）。

### 4.2 `bash` 输出的截断与 `outputs/` 落盘

- **`bash` 工具自己不截断**：`BashTool::call` 结束后只是 `outcome.report()`（`src/tools/bash.rs:141-142`），
  而 `report()`（`src/tools/process.rs:57-78`）把退出码、stdout、stderr 全量拼起来。
- 截断与落盘只有**一条流水线**，在工具结果进事件流之前：`emit_completed`（`src/agent.rs:2075-2098`）
  先 `session.redacted(&text)` 打码，再 `context::truncate_result(&text, tool_call_id, session.outputs_dir(), max_tokens)`。
- `truncate_result`（`src/context.rs:335-371`）：估算 token 超过 `max_tool_result_tokens`
  （缺省 `25_000`，`src/config.rs:58`）才落盘，写到 `<outputs_dir>/<tool_call_id>.txt`
  （`context.rs:351-355`，目录常量在 `src/session/store.rs:34`）。
- 流上留下的是「头 + `TRUNCATED_MARKER` + 尾」的预览（`context.rs:373-396`），标记是
  `TRUNCATED_MARKER = "[已截断："`（`context.rs:398-403`）—— 它是「这条结果被裁过、磁盘上才有全文」
  的**唯一**判据（同一处注释）。
- 渲染器正是按这条判据工作的：见下。

### 4.3 渲染层自己的有界读盘件（能借的那一件）

`read_tool_body`（`tui.rs:6293-6322`）+ `DETAIL_MAX_CHARS = 200_000`（`tui.rs:6138-6142`）：

- 预览里**没有** `TRUNCATED_MARKER` 时直接返回预览，**根本不去读盘**（`tui.rs:6297-6299`）；
- 有标记才拼 `session_dir/outputs/<tool_call_id>.txt`（`tui.rs:6302-6304`）去读（`6311`）；
- 三条降级：文件缺失（`6305-6313`）、文件为空（`6314-6316`）→ 「预览 + 全文不可用」；超过
  `DETAIL_MAX_CHARS` 字符 → `chars().take(200_000)` 加一句 `wording::detail_truncated()`
  （`tui.rs:6317-6321`、`6262-6267`）。

**借到「任意文件」上要剥掉两个假设**：① 路径是从 `session_dir` + `outputs/` + `tool_call_id`
拼死的；② 「该不该去找落盘文件」靠 `TRUNCATED_MARKER` 判断。另外它封的是**字符数**，01 票
D 节要的「行数、字节、行宽都封顶」在渲染层**没有现成件**（`DEFAULT_READ_LINES` 在工具层）。

### 4.4 二进制检测今天在哪一层

- **渲染层没有**：这一层唯一与文件内容有关的代码就是上面那次 `read_to_string`。
- **`read_file` 也没有专门的检测**：它直接 `std::fs::read_to_string(&path)`（`file.rs:219-220`），
  非 UTF-8 会变成一条「无法读取 …：stream did not contain valid UTF-8」的错误 ——隐式拒绝，
  不是「二进制只报一句」。
- **`bash` 输出不检测**：`process.rs:211-212` 用 `String::from_utf8_lossy`，坏字节变成替换字符。
- **真正的二进制检测只有一处，在 `grep` 工具里**：`grep.rs:157-161` 显式
  `.binary_detection(BinaryDetection::quit(0))`（见到 NUL 就放弃该文件）。另一处以「二进制」措辞
  处理的是 MCP 资源正文（`src/mcp/rmcp_client.rs:600-607`，把二进制说成一句而不展开），与本地
  文件无关。

**所以 01 票 D17 说的「二进制只报一句」在渲染器这条路上要新写**，没有可借的函数。

### 4.5 顺手确认：渲染器知道工作区根

`TuiState` 里有注入的 `cwd`（`tui.rs:645`，读出口 `pub fn cwd()` 在 `tui.rs:3412`），文件索引的
遍历也用它当 root（`tui.rs:401-405` → `file_index::scan`）。也就是说「按相对路径读工作区文件」
不需要新造一个「工作区根从哪来」的答案。

---

## 5. 核实「不进事件流、不进模型上下文」

**成立，三条证据：**

1. **渲染层对文件系统的唯一调用是一次只读**：`std::fs::read_to_string`（`tui.rs:6311`）。整个
   `src/render/` 目录下没有 `fs::write` / `File::create` / `create_dir` / `OpenOptions`
   （`grep` 过 `std::fs::`，只有这一处）。
2. **读盘只发生在打开那一刻，绘制不再读**：`open_detail` → `detail_body` → `read_tool_body`
   （`tui.rs:6155`）；`draw_detail` 只从已经排好的 `view.body` 里取窗口（`6368-6374`）。窗口
   缩放的重放（`2025-2056`）会重建各行与它们的 `Detail`（`emit_block` → `paint_block`），但
   **不会重排一个已经打开着的覆盖层正文**。
3. **不 emit、不碰 `Session`**：`open_detail` / `close_detail` / `detail_open` / `detail_scroll` /
   `detail_page`（`tui.rs:6144-6199`）与 `draw_detail` 只读写 `TuiState` 与 `Frame`；渲染器
   手里根本没有 `Session`（它的输入是 `RenderEvent` 广播与 `SessionFacts`，`tui.rs:285-314`）。
   `outputs/` 的写全在 `src/agent.rs:2075-2098`，那一条路根本不经过渲染器。

**两条要如实报上来的既有副作用**（不是写盘，但会改状态）：

- 打开任何详情都会记 `detail_opener` 并把**轨迹页**冻住（`tui.rs:6151-6154`、`6342-6345`），
  关掉时无条件把冻结还给轨迹页（`tui.rs:6168-6177`）。文件页作为第二个打开方，要么复用、要么
  扩展这套记账 —— 这是一个设计面，不是一个能忽略的细节（与 §1.5 第 4 条同一处）。
- `note_rows` 会把详情正文（连同任何前缀）记进屏幕文本层（`tui.rs:6410`），也就是**可以被拖选
  复制**的那份文本。它不是「进流」，但如果文件页的行号或截断提示不该被复制，就要在那一层处理。

**打码层的事实**：`outputs/<id>.txt` 那份落盘是**打过码的**（`agent.rs:2075-2086` 先
`session.redacted`）；而渲染层拿不到打码器（`SessionFacts`，`tui.rs:285-314` 里没有），也没有
`Renderer` 级别的打码出口。因此「渲染器直接读工作区文件」这条路上不存在打码 —— 与 01 票 D17
「这个弹窗不进事件流、不进模型上下文，所以那条打码纪律不适用」一致，这里只是把代码事实钉住。

---

## 查不到的条目

| 票里的说法 | 结论 |
| --- | --- |
| `trace-in-main/spec.md` §5「同一个块的续行补 9 个空格对齐」 | **查不到实现**。代码里只有 `stamp_lines`（`tui.rs:5693-5701`）给第一条显示行插前缀，续行原样（`pane::wrap_line` 的续行从第 0 列起，`pane.rs:426-427`）；`STAMP_COLUMNS` 的用法只有轨迹排版宽度那一处（`tui.rs:1988`）。spec 与代码在这一句上不一致，按代码执行的是「不补空格」 |
| 「按扩展名定语言」的映射 | **查不到**。`canonical_language`（`highlight.rs:224-238`）只认名字与别名；今天唯一的语言来源是 markdown 围栏的 info string（`markdown.rs:188-196`、`649`） |
| 渲染层可借的二进制检测 | **查不到**。这一层没有；`read_file` 靠 `read_to_string` 隐式要求 UTF-8（`file.rs:219-220`），唯一显式检测在 `grep` 工具（`grep.rs:157-161`） |
| 渲染层可借的「行数上限 / 字节上限」 | **查不到**。渲染层只有按**字符**封顶的 `DETAIL_MAX_CHARS`（`tui.rs:6142`、`6320`）；行数上限只存在于工具层（`file.rs:42`） |
| 详情滚动的独立上限常量 | **查不到，也不需要**：`max_top` 是现算的（`body.len() - height`，`tui.rs:6188`、`6366-6367`），没有第二个数字 |
