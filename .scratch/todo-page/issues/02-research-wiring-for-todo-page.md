# 02 — research：给左栏页接上命中区域与一种新详情，需要哪些接线

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

[01 号票](01-charting-decisions.md) 定死了这一页要有的东西：**页顶一条进度行 + 全部未完成项、
长文本折行、已完成项折掉、进度行右端一个能点开的按钮、点开是新增的一种详情**。而今天这一页是
左栏里**唯一**一页纯读数的东西 —— 没有滚动、没有命中区域、没有自己的 rect。

要查清的是**接线**，不是设计。这一票走 `/research`，全部是本仓库里的事实，答案带
`文件:行号` 收进本票 `## 作答`。

1. **给一页加命中区域**：文件页与改动页那一套到底是什么？`rect` 在哪里记、`rows` 记的是什么、
   鼠标点击怎么从屏幕坐标换成行下标（哪个函数、哪条分派）、焦点行由谁维护。`todo` 页要照抄哪几件、
   哪几件它照抄不了。**依据**：[`src/render/tui.rs:5710-5718`](../../../src/render/tui.rs) 那条
   `Tab::Files` / `Tab::Changes` 的「自己画并 `return`」分支，与
   [`src/render/todo.rs:61-93`](../../../src/render/todo.rs) 那个只吐 `Vec<Line>` 的 `lines`。
2. **指针分派现状**：左栏上今天的点击都归谁（页签条、文件树行、diff 行）？一条落在 `todo` 页
   矩形里的点击今天会掉进哪个分支 —— 是被吞掉，还是会误触发别的东西？分派表在哪个函数？
3. **加一种 `DetailKind`**：要动几处？`enum` 之外还有 `detail_body` 的 switch、标题怎么来、
   正文排版的入口、滚动、`Esc` / `q` 的处理、复制、`DetailOpener` 的匹配。**照着
   [`diff-page` 票 04 记的那条模板](../../../.scratch/diff-page/issues/04-prototype-layout.md) 核一遍**，
   并指出 2026-10-08 之后它有没有漂移。
4. **`DetailOpener` 该选哪个**：三种（Trace 冻轨迹视口、Files 与 Changes 什么都不冻）里这一页
   该落哪一种？它打开前左栏停在 `todo` 页、有滚动吗、覆盖层关掉之后要把什么还给谁？
   （这一问只查**现有形状与后果**，选哪一种留给 06 票。）
5. **弹窗正文的排版入口**：折行归调用方这件事在代码里是哪一行，宽度取
   `Regions::detail_text_width()` 还是别的；`pane::wrap_text` 的签名与折行预算怎么给；
   135 列那个上限在哪个常量。
6. **降暗落哪个语义色名**：[`src/render/palette.rs`](../../../src/render/palette.rs) 今天有哪些
   语义名（`dim` 与语义色的关系），「次要 / 已完成」这一档有没有现成的，没有的话最接近的是哪个 ——
   **不要自己起颜色**，这一票只报事实与既有先例。
7. **`note_rows` 与折行**：屏幕文本层登记的是**画出来的行**还是**逻辑项**？折行之后一条内容
   变 2–3 行，拖选复制会拿到什么。**依据**：`note_rows` 的签名与 `files_page` / `changes` 那两次调用。
8. **`wording` 的现状与惯例**：[`todo_count` / `todo_overflow` / `todo_glyph`](../../../src/render/wording.rs)
   今天在哪几行、字形常量（`☐` / `▸` / `✓`）定义在哪、那一族函数的命名习惯；新的几句文案
   （进度行、折叠提示、`＋N 项`、空态）按惯例该叫什么、放在哪。
9. **测试先例**：[`tests/render_layout.rs`](../../../tests/render_layout.rs) 里左栏页与详情覆盖层
   的断言是怎么写的（`apply_todo` 那套假会话、点击是怎么模拟的），[`tests/todo.rs`](../../../tests/todo.rs)
   里工具侧又写了什么。给 `/to-spec` 一份「这类改动照哪个模子写测试」的清单。
10. **几何现算**：按 [`layout.rs`](../../../src/render/layout.rs) 的算式，120×24 与 80×24 两档下
    页区各多少行（`diff-page` 票 04 量过一次，核一遍有没有漂移），以及**页区地板 3 行**
    （块字档 5 行）这一档下进度行 + 一条折行条目放不放得下。

## 作答

下面十条全部是本仓库里查到的**事实**与**既有先例**，不含设计决定。行号核到 2026-10-09 的
`40ec32c`（`src/render/tui.rs` 当时 10095 行）。

### 1. 给一页加命中区域：文件页与改动页那一套

**那套东西是四件，每件都住在「画它的那一处」**（纪律见 `src/render/tui.rs:6291-6294`
`FilesPage` 的文档注释：「状态由画它的那一处顺手记下，只有真画出来的行才回应指针与键盘」）：

1. **一个页面状态值**：`FilesPage`（`src/render/tui.rs:6296-6314`，字段 `expanded` /
   `scroll` / `rows: Vec<files::Row>` / `rect: Option<Rect>` / `focus: Option<usize>`）、
   `ChangesPage`（`src/render/tui.rs:6322-6336`，字段 `files` / `state` / `loading` /
   `rows: Vec<changes::Item>` / `focus` / `rect`），各自挂在 `TuiState` 上
   （`src/render/tui.rs:857`、`862`）。
2. **「自己画并 `return`」的分支**：`draw_sidebar_page` 的 `Tab::Files` / `Tab::Changes`
   两支各自调 `draw_files_page` / `draw_changes_page` 然后 `return`，于是那句 `note_rows`
   与 `render_widget` 只服务那两页纯读数的一支（`src/render/tui.rs:5710-5718`）。
   反过来说：一旦这一页要自己画，它就得接管 `note_rows` + `render_widget` 这一对。
3. **落笔顺序**：`draw_files_page` 头两句就是 `state.files_page.rect = Some(page)` 与
   `state.changes.rect = None`（`src/render/tui.rs:5736-5738`）；`draw_changes_page` 对称
   （`src/render/tui.rs:5850-5852`）。**清别人的 `rect` 是这一对函数互相写的，不是画页
   那支统一清的** —— `draw_sidebar_page` 只在纯读数那一支末尾清两个（`5720`、`5723`），
   而 `draw_sidebar` 在整栏没画出来时只清 `files_page.rect`（`5628`，漏了 `changes.rect`，
   那是既有的一个不对称的角落，与本票无关但照抄时别照错）。
4. **屏幕行 → 行下标的换算**：`files_index_at`（`src/render/tui.rs:3334-3341`，
   `index = scroll + (row - page.y)`，越界返回 `None`）、`changes_index_at`
   （`src/render/tui.rs:4836-4843`，这一页无滚动所以 `index = (row - page.y)`，且**只
   接受文件行**，`Item::Header` 返回 `None`）。落点是否在页内由
   `files_page_contains`（`3324-3328`）/ `changes_page_contains`（`4826-4830`）判，
   两者都是 `rect.is_some_and(|page| page.contains(..))`。

**焦点行由谁维护**：由点击与键盘两边写，样式那一档在画行的时候取：
`files_line(.., state.files_page.focus == Some(index))`（`src/render/tui.rs:5760`）、
`changes_line(.., state.changes.focus == Some(index))`（`5878`），焦点档都是
`ACCENT + BOLD`（`5807-5813` / `5917-5923`）。谁写它：`files_click`（`3312`）、
`changes_click`（`4819`）、`take_sidebar_keyboard`（`3527`、`3533`）、
`files_move_focus`（`3731-3744`）、`changes_move_focus`（`4675-4702`，跳过分组标题）、
切页签时的预置焦点（`3237-3243`）。

**`todo` 页要照抄哪几件**：① 自己画并 `return` 的分支（`Tab::Todo` 那一支今天走的是
`state.todo.lines(page)` 然后落到公共的 `note_rows` + `render_widget`，`5707`、`5726-5727`）；
② 一个页面状态值挂上 `TuiState`；③ 记 `rect`，并且**在切走时清掉 `files_page.rect` 与
`changes.rect`**；④ `rows` 记「画出来的行 ↔ 逻辑项」的映射（文件页记 `Vec<files::Row>`、
改动页记 `Vec<changes::Item>`，都是**逻辑行**，不是 `Line`）。

**照抄不了 / 不该照抄的几件**（都有依据）：

- **`scroll`**：`todo.rs:58-61` 的文档写死「没有滚动：高度阶梯给出行数，计数行先被留出来」，
  冻结项 4 也定「不加滚动」。
- **`focus` 与 `↑`/`↓` / `Enter`**：`sidebar_key` 对非 `Files`/`Changes` 直接
  `return false`（`src/render/tui.rs:3677-3684`），注释明说「另外两页没有能用方向键走
  的东西：键盘本来也不该扣在它们上面」（`3681-3682`）；而本图的「明确不做」已经写下
  不给键盘遍历。
- **`rows` 里那套「行 ↔ 行」的等价**：文件页与改动页之所以能用 `scroll + (row - page.y)`
  一句换算，正因为**每个逻辑项恰好占一行**。本票的冻结项 5 让一条内容占 2–3 行，
  于是「屏幕行 ↔ 逻辑项」不再是双射 —— 换算函数要另设计（这是票 06 的活）。
- **`sidebar_page_rect` 与 `take_sidebar_keyboard` 的键盘归属**：`sidebar_page_rect`
  （`4846-4852`）只认 `Files` / `Changes`，`Tab::Todo` 落到 `_ => None`。

### 2. 指针分派现状：一条落在 `todo` 页矩形里的点击今天掉进哪个分支

**分派表在 `TuiState::click_at`**（`src/render/tui.rs:3174-3298`），顺序按「谁占着指针」：

1. `picker_click`（`3177-3179`，选择器**不**独占指针）
2. 详情覆盖层（`3180-3193`）：框外点击关掉它
3. 问卷 / 中间三种确认（`3203-3217`）
4. `take_input_keyboard`（`3221-3223`）
5. `take_sidebar_keyboard`（`3227`）
6. `self.regions.action_at(column, row)`（`3229-3255`）—— `HitAction` 表在
   `src/render/tui.rs:1453-1480`，登记点只有四处：页签条标签（`6068-6071`）、
   主列页签条标签（`6045` 那个共用函数）、状态行两格（`6137`、`6140`）、回合条格
   （`6940-6943`）。**左栏页区不在这张表里**。
7. 兜底 `_` 支（`3256-3296`）：`files_click`（`3260-3262`）→ `changes_click`
   （`3265-3267`）→ `follow_link`（`3271-3273`）→ 轨迹页的 `over_trace` 闸门
   （`3277-3286`，不在轨迹页内容区里就 `return`）。

**一条落在 `todo` 页矩形里的点击今天会怎样**：第 5 步里 `sidebar_page_rect()` 对
`Tab::Todo` 返回 `None`（`4850` 的 `_ => None`），于是 `take_sidebar_keyboard` 在
`3513-3515` 立刻返回；第 6 步 `action_at` 找不到（页区没登记）；第 7 步 `files_click`
的第一句就是 `if self.tab != Tab::Files` → `false`（`3306`），`changes_click` 同
（`4815`），`follow_link` 第一句就问 `conversation_rect.contains`（`3983-3985`，
左栏不在主列矩形里）→ `false`，`over_trace` 为假 → `return`（`3284-3286`）。

**结论：被吞掉，什么都不发生**；不会误触发别的东西。既有测试
`clicking_a_todo_row_opens_nothing`（`tests/render_layout.rs:4389-4430`）正是钉这一条
（它特意把一条可点开的消息排到与左栏页区同一横行的位置，证明按屏幕行去取详情会取到
另一个视图的东西）。**接上命中区域时这条测试必须改写** —— 给 `/to-spec` 的提醒。

注意**拖选与点击是两条路**：左栏页区在 `5726` 登记进屏幕文本层，于是 `press_at` 落在
它上面照样能起一次拖选（`src/render/tui.rs:3100-3103`），`release_at` 按 `drag.selecting`
决定是复制还是 `click_at`（`3122-3131`）。文件页「点开新增动作不该吃掉拖选」那条测试就是
这个形状（`tests/render_layout.rs:2354-2379`）。

### 3. 加一种 `DetailKind` 要动几处（照 diff-page 票 04 的模板核过）

今天有六种（`src/render/tui.rs:8397-8427`：`Thinking` / `Context` / `Message` /
`File` / `Diff` / `Tool`）与三种 `DetailOpener`（`8436-8449`）。照票 04 记的模板
（`.scratch/diff-page/issues/04-prototype-layout.md` 与
`.scratch/diff-page/spec.md` §6），2026-10-09 的改动页提交（`a90755a`）就是这么做的，
**核完没有漂移**，模板仍然成立：

| 要动的地方 | 位置 | 现状 |
| --- | --- | --- |
| `enum DetailKind` 加一支 | `src/render/tui.rs:8397-8427` | 六个变体 |
| `detail_body` 的 `match` 加一支 | `8610-8788` | **唯一**的 switch（`grep DetailKind::` 在 `tui.rs` 内的全部出现点核对过） |
| 一个 `open_*_detail` 入口 | `open_file_detail` `3482-3496`；`open_changes_detail` `4725-4758` | 两个现成模子，都先 `close_file_viewer()` 互斥，再自算宽度，再调 `open_detail` |
| `DetailOpener` 加一支 | `8436-8449` | `close_detail` 的 match（`8573-8579`）**穷尽**，加一支必须同时改；`draw_detail` 的 `matches!(.. Trace)`（`9257`）只冻轨迹页，加别的变体不用动 |
| 标题与颜色 | `Detail { title, color, kind }` `8386-8393`；画在 `9310-9322`，超宽用 `ellipsize_line` 收尾（`9311-9318`） | 打开方给标题，颜色用 `palette::PLAIN`（文件页 `3491`、改动页 `4751`） |

**正文排版 / 滚动 / `Esc` / 复制**（四种新变体都一样，不在 `match` 里）：

- 宽度由**打开方**算好传进来：`open_detail(detail, width, opener)`（`8550-8552`）→
  `detail_body(&detail, &self.facts.session_dir, width)`。三个调用点各算各的：
  `layout::plan(self.area, 1, self.sidebar_wanted).detail_text_width() as usize`
  （`3294`、`3494`、`4741`）。
- 折行**归调用方**：`detail_body` 内部那一支自己折，画的时候 `Paragraph::new(lines)`
  **不折行**（`9334`），所以折行必须在 `body` 生成之前做完。正文那一族实际用的不是
  `pane::wrap_text` 而是它的带折行标记版 `folded_text`（`8517-8533`：按 `\n` 切逻辑段、
  每段 `pane::wrap_line`、第 `i` 片标 `folded = i > 0`）。`pane::wrap_text` 的签名是
  `pub fn wrap_text(text: &str, width: usize) -> Vec<Line<'static>>`
  （`src/render/pane.rs:423-432`），`wrap_line` 是 `pub(crate)`
  （`src/render/pane.rs:444-464`）；**数的是显示列不是字节**，续行从第零列起。
- 滚动：`detail_scroll`（`8588-8594`）按 `Vec<DetailLine>` 的下标，`height` 每帧由
  `draw_detail` 回填（`9343-9344`）。键盘 `Up/Down/PageUp/PageDown`
  （`4341-4344`）、滚轮（`3043-3045`，**详情开着时滚轮无条件归它，不看指针位置**）。
- 关闭：`Key::Esc | Key::CtrlD => self.close_detail()`（`4340`），其余键 `_ => {}`
  被吞（`4345`）。**`q` 不关弹窗**。
- 复制：详情正文整块进屏幕文本层，**带着 `folded`**（`9333`），所以折出来的续行在复制
  时拼回一条（`src/render/selection.rs:246-248`）。

**还有一处票 04 没列、但新变体绕不开的事实**：`DetailKind::Diff` 有一条**事后重排**的路
—— `diff_loaded` 按 `serial` 回填并 `view.body = detail_body(.., view.width)`
（`src/render/tui.rs:4797-4809`）。`todo` 那一支不需要它（内容是打开那刻就在内存里的），
但它说明「正文在 `open_detail` 里一次性算完、此后不重算」是既有形状 —— **冻结项 8 的
「打开那一刻的快照」不需要额外机制**：`Detail` 里带上那一份 items 的拷贝，
`detail_body` 从它算出正文即可，`state.todo` 之后被 `observe` 改写（`2257`）不影响
已经开着的弹窗。

### 4. `DetailOpener` 该选哪个：只报形状与后果

三种变体的事实（`src/render/tui.rs:8436-8449`）：

- `Trace { top, follow }`（`8438-8443`）：打开时记 `self.trace.top()` 与
  `self.trace.following()`（`3290-3293`）；`draw_detail` 里冻结
  （`set_following(false)` + `set_holding(true)`，`9257-9260`）；`close_detail` 里
  `set_holding(false)` + `trace.restore(top, follow)`（`8574-8577`）。
- `Files`（`8445`）与 `Changes`（`8448`）：**什么都不冻、什么都不还**，两者在
  `close_detail` 里合并成同一支（`8578`：`Some(Files) | Some(Changes) | None => {}`）。

`todo` 页那一侧的事实：`TodoPanel` 只有 `seen` 与 `items` 两个字段
（`src/render/todo.rs:27-33`），**没有 `scroll`、没有 `focus`、没有 `rect`**；
`lines` 自己按页高算放几行（`todo.rs:61-93`），文档明写「没有滚动」（`58-60`）。
所以：**打开前左栏停在 `todo` 页、有滚动吗 —— 没有滚动**；关掉覆盖层之后要把什么还给
谁 —— 按 `Files`/`Changes` 那一支的形状，**什么都不用还**（页面上没有任何会因覆盖层关掉
而需要复原的东西）。选哪一种（或干脆复用 `Files`）留给 06 票；这一票只把后果摆出来。

### 5. 弹窗正文的排版入口

- **折行归调用方**这件事在代码里就是 `open_detail` 的第三个参数 → `detail_body` 的
  `width`（`src/render/tui.rs:8550-8552`），以及画的时候不折行的那句
  `Paragraph::new(lines)`（`9334`）。
- **宽度取 `Regions::detail_text_width()`**：`detail_text_width`
  （`src/render/layout.rs:251-254`）= `inner(detail_width)` 再减两侧内边距，而
  `inner` 与 `draw_detail` 的绘制路径**共用** `layout::detail_padding`
  （`layout.rs:624-628` 与 `tui.rs:9272`；注释 `layout.rs:619-623` 明说这是为了
  「行号也会被算错」才收成一处）。三个打开点都取它（`tui.rs:3294`、`3494`、`4741`），
  **不是框宽**。
- **`pane::wrap_text` 的签名**：`pub fn wrap_text(text: &str, width: usize) ->
  Vec<Line<'static>>`（`src/render/pane.rs:423`）。折行预算 = `width`（显示列），
  数的是列不是字节；细节在 `wrap_line` 的文档注释（`src/render/pane.rs:434-443`）：
  续行从第零列起，一个宽字符独占一列宽时照样溢出。详情正文那一族走的是
  `folded_text`（`tui.rs:8517-8533`）—— 与 `wrap_text` 同一套折法，多带一个
  「这是哪个逻辑段折出来的第几片」。
- **135 列那个上限**：`const DETAIL_MAX_WIDTH: u16 = 135`（`src/render/layout.rs:607`），
  经 `Regions::detail_width()`（`239-244`）取 `min(screen.width - MODAL_MARGIN(4),
  135)`，边距常量在 `layout.rs:602`，边框两列 `BORDER_COLUMNS = 2`（`layout.rs:39`），
  内边距 `DETAIL_PADDING = 1`（`layout.rs:614`，空间不够时让出来）。
  算下来：120 列屏 ⇒ 框 116、正文 **112**；80 列屏 ⇒ 框 76、正文 **72**；
  ≥139 列屏 ⇒ 封顶 135 框 / **131** 正文（`detail_padding` 的 `pad_x` 判据是
  `inner.width > DETAIL_PADDING * 3`，`layout.rs:625`）。

### 6. 降暗落哪个语义色名

`src/render/palette.rs` 今天的界面域一共这些：`PLAIN`（19）、`MUTED`（23，`DarkGray`）、
`CHROME`（27）、`ACCENT`（31）、`WARN`（35）、`BAD`（38）、`INJECTED`（42）、
`BUBBLE`（49）、`TOKEN_COMMAND` / `TOKEN_REFERENCE`（56、59）、`MENU_SKILL` /
`MENU_TEMPLATE`（65、69），加冻结的角色五色（76-85）与品牌标记三档（94-103）。
内容域另有 `CODE_QUIET`（113）等。

**`dim` 与语义色的关系**：`Modifier::DIM` 在渲染层**只出现两次**，且都不在语义色板里
—— `src/render/changes.rs:636`（外部 diff 工具吐的 SGR，属于外来的输出，
[ADR 0018](docs/adr/0018-external-diff-viewer-colours-sit-outside-the-palette.md)
把那类颜色划在色板之外）与 `src/render/viewer.rs:415`（一屏外来 nvim 屏幕的 modifier）。
**色板里没有「dim」这个语义名，也没有任何一档是「半档亮度」**；`MUTED` 的文档反而明说
「全屏只有这一档静音；再退一档只能靠**结构**（缩进、线型、留白），`DIM` 不参与层级」
（`palette.rs:21-23`）。

**「次要 / 已完成」这一档有没有现成的**：没有专门的语义名。两个既有先例，
都指向同一个值（`DarkGray`）：

- 界面域的 `MUTED`（`palette.rs:23`）—— 现成的用法是「一句降级说明」：页内的一句
  （`draw_page_note`，`tui.rs:5835-5838`）、详情里的降级句（`8641-8644`、
  `8666-8669`）、页脚（`9336-9339`）、改动页的「还有 M 处改动」（`5888-5891`）。
- 内容域的 `CODE_QUIET`（`palette.rs:113`），注释明说「值与界面域的 `MUTED` 相同是
  刻意的」。

所以「已完成项降暗」**不必也不该新起颜色**：语义上它属于「过程 / 退后」那一档，落点
`MUTED`。这一条只是事实登记，颜色怎么取由 05 票定。

### 7. `note_rows` 与折行

**签名**：`fn note_rows(state: &mut TuiState, rect: Rect, rows: &[Line<'static>],
folded: &[bool], hotspots: &[Vec<links::Hotspot>])`（`src/render/tui.rs:6861-6879`）。
`folded` 与 `rows` **平行，短了当没有软折**（`6873` 的
`folded.get(index).copied().unwrap_or(false)`），`hotspots` 同理（`6875`）。

**登记的是画出来的行**：`rows` 逐条映射成 `selection::TextRow { text, folded, lead,
hotspots }`（`6868-6877`），也就是**屏幕上那一行**。左栏三处调用全部传空表：
`files_page`（`5767`）、`changes`（`5895`）、纯读数那一支（`5726`）——
所以左栏今天**一条软折都没有**，拖选复制时每两行之间插 `\n`
（`src/render/selection.rs:246-248`）。

**折行之后拖选复制会拿到什么**：一条内容画成 2–3 行 ⇒ 屏幕文本层里就是 2–3 条
`TextRow`。若照今天左栏的写法传 `&[]`，它们在复制时被当成三条独立行（各自之间换行）；
若按 `folded` 标出续行，它们会**拼回一条**（`selection.rs:246`：`if written > 0 &&
!row.folded` 才插 `\n`）。两栏之外的先例就是 `draw_detail`（`9300`、`9333`：把
`DetailLine::folded` 摊成与行平行的表再传下去）。选区**不跨区域**
（`selection.rs:97-100`、`220-225`：选区只在按下那一刻落进的那一块里延伸），所以相邻
两项不会被粘进来 —— 但同一条目的续行会不会被粘在一起，取决于 `folded` 有没有标。

### 8. `wording` 的现状与惯例

**今天在哪几行**：`todo_glyph`（`src/render/wording.rs:1978-1984`，一张
`Status → 字形` 表）、`todo_count`（`1987-1989`，`已完成 2/5`）、
`todo_overflow`（`1992-1994`，`＋3 项`）。三句都在「符号表与字符级间距」小节
（`1897-1902`）之后、`files_loading`（`1998`）之前 —— **没有自己的小节标题**，改动页
有（`2008-2011`）。唯一的调用点是 `src/render/todo.rs:66-69`、`86-89`、`101`。

**字形常量**：`TODO_PENDING = "☐"`、`TODO_IN_PROGRESS = FOLDABLE`、
`TODO_COMPLETED = "✓"`（`src/render/wording.rs:1916-1919`），住在符号表小节里。
`FOLDABLE`（`▸`）的双义登记在 `1904-1909`：同一个字形在 todo 页里是「进行中」，两处
**靠区域区分**。`TODO_IN_PROGRESS` 就是 `FOLDABLE` 的别名（`1918`）。

**命名习惯**：`<页面>_<句子>`；带参数的是函数返 `String`（`format!`），不带参数的返
`&'static str` —— `changes_more(remaining)`（`2048-2050`）、`changes_empty()`
（`2033-2035`）、`files_loading()` / `files_empty()`（`1998-2006`）、
`detail_footer(position, total)`（`425`）、`detail_section(name)`（`351-353`，
`── 名字 ──`）。

**按惯例新几句该怎么叫、放在哪**：进度行 / 折叠提示 / `＋N 项` / 空态这几句都属于
`todo` 页 → 前缀 `todo_`，带计数的那句是函数返 `String`
（对照 `changes_more`）。位置建议照改动页那一族：**给这一族起一个独立小节**
（`wording.rs:2008-2011` 那种带 spec 引用的分隔注释），而不是继续堆在 `1978-1994` 那一
段里。**分隔符用现成的**：`wording::SEP = " · "`（`1951`）已经是「并列两段之间」那一个，
进度行文案 `3/11 · 1 个在做` 与它一致；续行缩进用 `wording::INDENT = "  "`（`1945`，
两格，注释说「界面上只有这一个字符级的往后退一档」）。

### 9. 测试先例

**左栏页那几样（`tests/render_layout.rs`）**：

- 假会话入口：`apply_todo(state, id, speaker, args)`（`4063-4090`，一前一后两条事件
  —— `ToolCallStarted` 带参数、`ToolCallCompleted` 让块画出来），参数由 `todo_args`
  （`4044-4051`）造，说话人 `kimi()` / `executor()`（`4053-4059`）。
- 取帧：`buffer(w, h, &mut state)`（`118`）、`screen(w, h, &mut state) -> Vec<String>`
  （`68`）、`cells(&frame, y, from, to)`（`78`）、`row_text(&frame, y, width)`（`90`）。
- **断言左栏页必须读左栏那几列**：文档在 `4102-4104` 明说转录很可能含同样的词，
  所以有 `sidebar_rows(state, w, h)`（`4105-4110`，`SIDEBAR_COLUMNS = 40`，
  `4114`）与 `tab_bar_row`（`4117-4122`）。
- 点击：`click(state, column, row)` = `press` + `release`（`5579-5582`；
  `press` `5546`、`release` `5557`、`drag_to` `5568`）；找坐标
  `cell_of(&frame, w, h, needle)`（`6001-6009`，按**显示列**折算，注释解释为什么不能
  按字节）、`click_in_row(state, w, h, row, needle)`（`6025-6033`）、
  `click_text` / `click_row`（`6012-6018` / `5977-5979`）、`tab_cell`（`1634`）。
  「列不是无所谓的」这条纪律写在 `5974-5976`。
- 拖选复制：`the_files_page_can_still_be_dragged_and_copied`（`2354-2379`）
  —— `press` / `drag_to` / `release` 三步 + 断言 `state.take_clipboard()` 等于
  `selection::osc52(...)`。

**详情覆盖层那几样**：

- `tests/render_layout.rs`：`a_click_opens_the_detail_and_a_second_click_closes_it`
  （`5727-5755`，点开 → 断言 `── 参数 ──` / `── 输出 ──` / `esc 关闭` → `Key::Esc` →
  断言关上）、`the_detail_body_is_wrapped_to_the_real_text_width`（`5758-5780`，
  造一条 112 列 + `TAIL` 的行，断言尾巴折到下一行而不是被框裁掉）、
  `the_detail_body_scrolls_with_the_keys_and_the_wheel`（`5783-…`）。
- **`tests/render_changes.rs:602-641` 有一整套现成的弹窗取景 helper**，新弹窗最该照抄：
  `overlay_area(w, h)`（自己按 `layout::inner` + `detail_padding` 算文本区，602-614）、
  `overlay(state, w, h) -> Vec<String>`（617-627）、`overlay_cell(frame, w, h, needle)`
  （630-641）。
- 点开弹窗的断言模子：`clicking_a_row_opens_the_diff_and_enter_does_the_same`
  （`tests/render_changes.rs:661-696`）—— 点一行 → 断言 `esc 关闭` + 标题 + 占位句；
  另一份状态用 `Enter` 走同一条路。

**`tests/todo.rs` 写了什么**：工具契约那几件（回执文案 `todo：3 项（1 项已完成）`、
清空 `todo：已清空`、非法参数逐条点名、三个表都挂上它、规则段在身份里）
—— 加上**两个直接调面板的单元测试**：`the_sidebar_shows_the_id_before_the_content`
（`308-340`）与 `a_real_session_keeps_the_id_in_the_arguments_and_the_sidebar_reads_it_back`
（`560-612`），两者都直接
`panel.lines(Rect::new(0, 0, 28, 4))` 再把 `Line` 收成 `Vec<String>` 比对 ——
**面板级单测的模子**，本次改动给「页与面板」加断言时照它写。

**给 `/to-spec` 的清单（一句话）**：页侧照 `apply_todo` + `sidebar_rows` + `cell_of` /
`click_in_row` 三件；弹窗侧照 `render_changes.rs` 那三个 `overlay_*` helper；
面板级照 `tests/todo.rs:308-340` 的 `lines(Rect)` 直接比对；**并且 `clicking_a_todo_row_opens_nothing`
（`tests/render_layout.rs:4389-4430`）要改写** —— 它的断言（「点 todo 行什么都不动」）
与本页新增的命中区域正面冲突。

### 10. 几何现算（按 `layout.rs` 的算式重算，核过票 04）

算式：`sidebar_rows = height - SIDEBAR_TOP_GAP(1)`（`layout.rs:414`，常量 `80`）；
身份档按 `sidebar_content` 的阶梯选（`528-557`）：宽档先 `Mark`(23 行) → `MarkCompact`
(5) → `Text`(1) → `Hidden`，判据是 `kind.rows() + TAB_ROWS(3) + floor <= content_rows`，
地板 `SIDEBAR_MIN_PAGE_ROWS = 3`、`BLOCK_LOGO_MIN_PAGE_ROWS = 5`（`111`、`102`）；
`page_rows = content_rows - (kind.rows() + TAB_ROWS)`（`555`），页区矩形在
`474-481`。宽度档：≥120 ⇒ `SIDEBAR_WIDE = 40`，≥80 ⇒ `SIDEBAR_NARROW = 28`
（`66`、`67`、`70`、`73`、`499-510`）；意愿与宽度相乘，意愿为假整栏没有。

- **120×24**：`content_rows = 23`；`Mark` 要 23+3+5=31 > 23 ⇒ 退 `MarkCompact`，
  5+3+3=11 ≤ 23 ⇒ 停在它 ⇒ **页区 40 列 × 15 行**。
- **80×24**：`tier = 28 < LOGO_WIDTH = 38`（`83`）⇒ 直接 `Text`，1+3+3=7 ≤ 23 ⇒
  **页区 28 列 × 19 行**。

**与票 04 完全一致，没有漂移**（票 04 与 `.scratch/diff-page/prototype/frame.py:12-13`
记的正是 `40×15` 与 `28×19`）。

**页区地板那一档**（`below_minimum` 的门是 `width < 40 || height < 10`，`layout.rs:200-202`、
常量 `18-19`）：

- **3 行地板只出现在宽档**：要让 `page_rows = 3`，`MarkCompact` 档需
  `content_rows = 5+3+3 = 11` ⇒ **终端 120×12**（`content_rows = 11`，页区 **40×3**）。
  窄档要到 3 行需要 `content_rows = 7` ⇒ 终端高 8 行，**过不了 `MIN_HEIGHT = 10` 那道门**，
  所以窄档的最小页高是 80×10 的 **28×5**（`content_rows = 9`）。
- **块字档 5 行地板**：要到 `Mark` 档需 `content_rows ≥ 23+3+5 = 31` ⇒ **终端 120×32**，
  此时页区 **40×5**（`31-26`）。块字一出现就是 5 行地板，不会更低（与
  [ADR 0002](docs/adr/0002-fullscreen-alt-screen-tui.md) 补记里「终端从 32 行起才画篆书」
  一致）。

**进度行 + 一条折行条目放不放得下**（按冻结项 5 的「一条内容占 2–3 行」）：

- **40×3**：进度行 1 行 + 一条折行条目最少 2 行 = 3 行，**正好放满**，没有余量给
  `＋N 项`；一条折成 3 行的条目就放不下。
- **28×5**（窄档最矮）与 **40×5**（块字档最矮）：进度行 1 行 + 条目 2~3 行 = 3~4 行，
  余 1~2 行。