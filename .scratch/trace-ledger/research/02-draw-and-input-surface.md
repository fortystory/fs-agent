# 衡的绘制与输入面：这次改动能站在哪些件上

来源：[`../issues/02-research-draw-and-input-surface.md`](../issues/02-research-draw-and-input-surface.md)
（wayfinder research 票，只读调研）。读的是 2026-10-09 的 `src/render/`。

**行号约定**：写成 `:NNNN` 的短式行号一律指 `src/render/tui.rs`；别的文件与测试文件每次写全
路径。所有结论以代码为依据，推断处写明依据，拿不到事实的地方写「未证实」。

## 总形状

键盘归 `TuiState::key` 里**一条自上而下的守卫阶梯**（`src/render/tui.rs:4337`）：每一层「谁
占着键盘」都是提前 `return` 的一支，越靠前的层越独占；默认兜底是输入区的编辑器
（`editor_key`，`src/render/tui.rs:2776`）。**轨迹页今天不是这阶梯上的一层** —— 它只被「当前
显示的那一页」这一支认领三个键与滚轮（`page_current` / `current_page_to_bottom` /
`wheel_current`，`src/render/tui.rs:2995`、`:3003`、`:2987`）。

绘制是**无状态、每帧重建 + 记下这一帧真画了什么**：`draw_frame` 开头清空命中矩形、屏幕文本
层、两个视图的矩形与指示器（`src/render/tui.rs:5515`–`:5529`），各绘制点再重新填；指针只
回应上一帧记下来的东西。

行与块的对应走**两个下标**：块经 `paint_block`（`src/render/tui.rs:7699`）画成*源行*，源行
一格一格推进 `pane::Pane`；一块与它画出来的显示行之间没有直接指针 —— 显示行 → 源行由
`Pane::starts` / `source_at`（`src/render/pane.rs:140`）与每帧重建的 `Drawn.rows`
（`src/render/tui.rs:1665`）两处承担，源行 → 块内容由平行表 `trace_links`（`src/render/tui.rs:920`）
承担。**行身份是位置型下标，不是稳定行键** —— 见第 2 节最后一条。

## 1. 字符格里的绘制件

**今天唯一的数据条形先例**是用量页那两行的占比色条（`src/render/panel.rs:172` 的 `row()`）：
字形是 `wording::BAR_FULL = '▓'` / `BAR_EMPTY = '░'`（`src/render/wording.rs:1928`），填充数
`ceil(share × 条宽)` 并夹在条宽内（`src/render/panel.rs:198`），条宽上限 `BAR_COLUMNS = 10`
（`src/render/panel.rs:214`）。三条纪律值得照抄：条**占列**（值列先拿到自己需要的列，剩下的
才给条，`src/render/panel.rs:175`–`:184`）；只有两条有诚实分母的行才画条
（`src/render/panel.rs:102`–`:109`）；条本身穿静音档（`src/render/panel.rs:205`）。它是**横向
整格**铺的，没有纵向条、没有堆叠条、没有半格精度的先例。

**半格/子像素用过。** 左栏那块篆书标记是拿块元素拼的，注释写明「一格一个 2×2 的子像素」，
用的是半块（`▀` / `▄` / `█`）加十个四分之一块（U+2596–U+259F）（`src/render/wording.rs:1803`）。
但它是一份**写死的静态字形常量**（`src/render/wording.rs:1817`），不是运行期按数据算出来的
图 —— 数据图今天都停在「一格一个 `▓`」那一档。

**`Span` / `Line` 能表达什么。** `Span` 是一个 `content` 加一个 `Style`；`Line` 是
`spans` + 行级 `style` + `alignment`（三者的复制见 `src/render/pane.rs:470`）—— 对齐只有
左 / 右 / 居中三档，`Alignment::Right` 今天只服务气泡与它的名字行（`src/render/tui.rs:8172`）。
`Style` 是**前景 + 背景 + 一组 `Modifier`**。

**一个格里最多能塞几层语义。** 叠两层是既有做法：补丁正文把语法前景 `patch` 在 diff 背景上
（`src/render/tui.rs:8959`，两层分家的理由见 `src/render/highlight.rs:1`–`:10`；背景两档在
`src/render/palette.rs:142`、`:145`），用户气泡用同一套机制铺一块底色
（`src/render/palette.rs:49`、`src/render/tui.rs:8189`）。修饰符里 `BOLD` / `ITALIC` /
`CROSSED_OUT` / `UNDERLINED` 各由 markdown 行内标签给（`src/render/markdown.rs:224`–`:226`、
`:454`），`REVERSED` 有一条硬纪律：**全屏唯一的反显就是键盘所在**（`src/render/tui.rs:5450`），
它是「临时光标」那一档（菜单高亮 `src/render/tui.rs:7453`）。`DIM` **在界面绘制里已退场**
（`docs/render.md:642`），今天只出现在 nvim 查看器回放与外部 diff 的转义解析里
（`src/render/viewer.rs:415`、`src/render/changes.rs:636`）。

**推论（给 prototype 票）**：一格里可同时承载「字形 + 前景 + 背景 + 几个修饰符」，但要按既有
纪律分档 —— 常驻选中是 `ACCENT + BOLD`（`src/render/tui.rs:5866` 的 `files_line`、`:6117` 的
页签条），临时光标是 `REVERSED`（全屏只许一个），数据量本身用底色或字形。时间轴那种「一条
横轴上堆多种语义」在字符格里只能靠**换字形 + 换底色**，不能靠像素。

另有一条画覆盖层时才撞得到的机制：宽字形后面那一格会被后端跳过，所以覆盖层画之前要把被盖住
的半格抹成空格（`blank_half_covered_glyphs`，`src/render/tui.rs:6648`）—— 时间轴若与覆盖层
同屏，这条照旧适用。

## 2. 行的身份与寻址

**`Pane::push` 返回「这一次丢掉了几条源行」**（`src/render/pane.rs:80`，裁剪唯一权威在
`evict`，`src/render/pane.rs:354`）：上限 `CAP = 20_000` 条*源行*（`src/render/pane.rs:21`），
窗口、滚动条与「新内容」计数数的是*显示*行（`src/render/pane.rs:9`–`:11`）。**源行下标会随
`evict` 全体左移**（`src/render/pane.rs:375`–`:382`），所以 `trace_links` 与 `TurnRail.lines`
都按 `push` 报回来的那个数跟着 `pop_front` / `prune`（`src/render/tui.rs:2522`、`:2533`）
—— 「平行表与窗格同进同出」是契约而不是巧合。

**源行 ↔ 显示行的两跳。**
- 显示行 → 源行：`Pane::source_at(display_row)`（`src/render/pane.rs:140`，二分 `starts`）；
  轨迹页每帧把它按窗口铺成一张表 `Drawn { rows: Vec<Option<usize>>, top }`（`src/render/tui.rs:6876`
  – `:6884`，`top` 是第一条被画出来的行的屏幕 y）。表里是 `None` 的那些行不属于任何源行（正在
  流的尾巴）。
- 源行 → 显示行：只有 `Pane::scroll_to_source(source)` 一个入口（`src/render/pane.rs:193`），
  它顶对齐并把视口从那一条源行起画 —— 回合条格子跳转就是这样跳的（`src/render/tui.rs:4006` 的
  `jump_to_unit`）。**没有「源行此刻在第几个显示行」的公开读口**。

**一次点击从屏幕坐标回到块的整条路径**：`mouse`（`src/render/tui.rs:3015`）→ `release_at`
（`:3122`，拖选优先，没越过门槛才算点击）→ `click_at`（`:3174`）→ 覆盖层 / 问题 / 归还输入区
键盘 / 归还左栏键盘 / `regions.action_at`（`:3229`）→ 落点判 `trace_rect.contains`（`:3282`）
→ `trace_link_at(row)`（`:4060`）：屏幕行减 `trace_drawn.top` 得 offset → `trace_drawn.rows`
得源行下标 → `trace_links[source]` 得 `Option<Detail>` → `open_detail`（`:3299`）。

**`HitAction` 与命中矩形。** `Regions { cells: Vec<Region { rect, action }> }`（`src/render/tui.rs:1446`
、`:1495`），`HitAction` 是一个十三个变体的 enum（`:1453`，`Answer` / `Paste` / `ClearDraft` /
`StopGoal` / `Dismiss` / `Previous` / `Next` / `Submit` / `SwitchTab` / `SwitchMainTab` /
`TurnRailUnit` / `SwitchModel` / `SwitchEffort`），每帧 `clear` 后由绘制点
`push`（`:1506`），查询是**线性取第一个包含点的矩形**（`:1532`，也就是画得越晚越优先）。
**轨迹页的行不走这一套**：它靠 `trace_links` + `trace_rect` 那一对（`trace_link_at`，`:4060`），
因为一行要按源行寻址而不是按矩形。回合条与页签条走 `Regions`（`:6997`、`:6127`）—— 两种先例
都在，加行选择时该跟哪一种取决于「行是否需要在折行后仍对准」。

**要点给下游**：今天**没有**稳定的行身份。`Detail` 只在源行被画出来的那一刻克隆一份存进
`trace_links`（`src/render/tui.rs:8444` 的注释写明「主体是在这行被画出来时读的」），而源行
下标会被 `evict` 移动、被宽度变化整批重放（`rerender_if_width_changed`，`:2441`）。任何「折叠
状态 / 搜索命中 / 选中行」若按源行下标存，都会在这两件事上漂。

## 3. 覆盖层（详情）

**七种 `DetailKind`**（`src/render/tui.rs:8456`）：`Thinking` / `Context` / `Message` / `File` /
`Diff` / `Todo` / `Tool`。各自的正文在 `detail_body` 里一个 `match` 分派（`:8682`），**每个
变体自己决定画几段**：`Message` / `Thinking` / `Context` 是一个小节标题 + 折行正文；`Tool`
固定两段（`参数` + `输出`，`:8834`、`:8839`）；`File` / `Diff` 是带行号或带补丁底色的一页；
`Todo` 交给 `render::todo::detail_rows`。换句话说，今天「面」已经以**小节线**的形式存在于
Tool 那一种里（`section_header`，`:9003`），只是没有「切换」这件事。

**尺寸规则。** 宽度：屏宽减 4 列边距、封顶 135（`Regions::detail_width`，`src/render/layout.rs:239`），
正文的排版宽度再减两列边框与两侧内边距（`detail_text_width`，`src/render/layout.rs:251`，
判据与绘制路径共用 `detail_padding`，`src/render/layout.rs:624`）。高度：屏高减 4、夹在
`overlay_floor`（平时屏幕底边，问卷立着时问卷块顶边）之内，装不下正文就不开（`overlay_area`，
`src/render/layout.rs:286`）。正文可用的行数 = 文字区高度减 2（标题一行、页脚一行，
`src/render/tui.rs:9360`）。

**`detail_opener` 那份记账。** `DetailOpener` 四个变体（`src/render/tui.rs:8502`）：
`Trace { top, follow }` / `Files` / `Changes` / `Todo`。打开时存下（`open_detail`，`:8619`），
关上时按它还原 —— **只有 `Trace` 会冻与还**：`draw_detail` 每帧对轨迹页
`set_following(false)` + `set_holding(true)`（`:9336`–`:9338`），`close_detail` 对轨迹页
`restore(top, follow)`（`:8642`）。其余三种打开方什么都不冻、什么都不还。

**覆盖层自己的滚动、键位与页脚。** 状态是 `DetailView { detail, body, width, top, height }`
（`:8548`），`body` 是**打开那一刻**按宽度排好的 `Vec<DetailLine>`（`DetailLine` 多带一个
`folded` 供拖选拼回，`:8566`）。滚动 `detail_scroll`（`:8660`，夹在 `body.len() − height`），
翻页步长 `detail_page`（`:8669`，高减一行重叠）。键位只有五行：`Esc` / `Ctrl-D` 关，`↑`/`↓` 滚
一行，`PgUp`/`PgDn` 滚一页，**别的全忽略**（`:4379`–`:4391`；`Ctrl-C` 也被忽略，见
`tests/render_layout.rs:7377`）。页脚是 `↕ n/N · esc 关闭`（`wording::detail_footer`，
`src/render/wording.rs:425`）。

**分面加在哪一层**：今天「一份详情 = 一个 `body`」，要分面就得在 `DetailView`（`:8548`）
这一层给「当前面 + 每面各自那份 `Vec<DetailLine>`」，并把 `detail_scroll` / 页脚 / 键位表都
改成认「当前面」。绘制侧的现成件是够的：一条标签条可以复用 `draw_label_bar`（`:6104`，带
命中矩形）、边框与清屏走 `chrome_block` + `Clear`（`:9381`–`:9386`）、面的计数可以挂在现有页脚那
一行（`:9414`）。

## 4. 键位占用表

分层编号是 `TuiState::key` 里那一条守卫阶梯的次序（`src/render/tui.rs:4337`–`:4542`）：

- **L0 终端层**：一切守卫之前（`:4345`）。
- **L1 独占键盘的视图**：历史重放（`:4358`）、文件查看器（`:4365`）、选择器（`:4371`）、
  详情覆盖层（`:4379`）、待答问题（`:4471`，问卷自己分发）。
- **L2 左栏键盘归属**：`sidebar_keyboard` 为真时文件页 / 改动页（`:4394`）。
- **L3 全局视图手势**：`Ctrl-T`（`:4405`）、`Ctrl-O`（`:4410`）。
- **L4 出入与举手**：`Ctrl-C` / `Ctrl-D`（`:4428`）、`Esc`（`:4432`）。
- **L5 记号菜单**：`/` `@` 菜单立着时占 `↑`/`↓`/`Tab`/`Enter`（`:4502`）。
- **L6 当前显示的那一页**：`PgUp` / `PgDn` / `Ctrl-G`（`:4533`）与滚轮（`:2987`）。
- **L7 输入区编辑器**：兜底（`:4537`）。

| 键 / 手势 | 今天归谁 | 在哪一层生效 | 轨迹页这一层还空着吗 |
| --- | --- | --- | --- |
| `↑` / `↓` | 草稿光标上下（`editor_key` `:2788`）；文件页移焦点行（`:3735`）；选择器移高亮（`:3683`）；问卷选项区移动（`:1160`） | L7 / L2 / L1 | **不空**：归编辑器。冻结项 4 要把它给行选择，必须明确征用（或只在「轨迹页 + 键盘不在输入区」时认）|
| `←` / `→` | 草稿光标左右（`:2786`）；文件页展开 / 收起（`:3737`）；选择器；问卷选项区翻题 | L7 / L2 / L1 | 不空（同上）|
| `Home` / `End` / `Ctrl-A` / `Ctrl-E` | 草稿行首 / 行尾（`:2790`） | L7 | 不空 |
| `Backspace` / `Delete` | 草稿删字（`:2784`） | L7 | 不空 |
| `Ctrl-U` / `Ctrl-K` / `Ctrl-W` | 草稿删到行首 / 行尾 / 删词（`:2792`） | L7 | 不空 |
| `Ctrl-P` / `Ctrl-N` | 草稿历史上下条（`:2796`）；问卷选项区移动（`:1160`–`:1161`） | L7 / L1 | 不空 |
| `Ctrl-J` / `Shift+Enter` | 草稿换行（`map_key` `:186`、`editor_key` `:2783`） | L7 | 不空 |
| `Enter` | 提交草稿（`:4530`）；记号菜单接受（`:4512`）；文件页插 `@路径`（`:3739`）；改动页开 diff（`:3758`）；选择器选中（`:3693`）；问卷处置一题；中间模态确认（`:4480`） | L7 / L5 / L2 / L1 | 不空 |
| `Tab` | `/` 菜单补全（`:4512`）；问卷跳过并前进；选择器下一行（`:3683`）。**页签从不给键位**（`.scratch/trace-in-main/spec.md:135`）| L5 / L1 | 不空 |
| `Shift+Tab`（`BackTab`） | 模式循环（`:4490`，在 L3 与 L5 之间，属全局）；选择器立着时归选择器（L1，`:3688`）；`muted` 时仍生效（`:4487` 只拦 `Char`/`Enter`/`Tab`）| 全局 / L1 | 不空 |
| `Esc` | 取消拖选（`:4351`）；关详情覆盖层（`:4383`）；文件页 / 改动页还键盘（`:3734`、`:3755`）；选择器取消（`:3706`）；取消回合 / 清草稿 / 关菜单 / 退出询问（`:4432`–`:4470`）；查看器里转发给 nvim | 多层 | 不空（但**每一层都要有出口**这条纪律照旧）|
| `Ctrl-C` | 取消 + 举手、500ms 内第二下退出（`exit_key` `:2028`）；详情层**忽略**（`:4383` 只认 `Esc`/`Ctrl-D`）；查看器里关掉查看器（`:3478`）；重放里双击退出（`:2755`）| L4 | 不空 |
| `Ctrl-D` | 空闲双击退出（`:2035`）；忙碌或有问题立着时忽略；**详情层等于关覆盖层**（`:4383`）| L4 / L1 | 不空 |
| `Ctrl-Z` | 挂起（`:4345`，L0，一切守卫之前）| L0 | 不空（终端层，轨迹页拦不住也不该拦）|
| `Ctrl-O` | 左栏开关（`:4410`）| L3，被 L1（详情 / 重放）拦得住 | 不空 |
| `Ctrl-T` | 模型 / 档位选择器（`:4405`）| L3，选择器立着时归选择器（`:4371` 提前 return）| 不空 |
| `Ctrl-G` | 回**当前显示那一页**的底（`:4535` → `:3003`）| L6 | **已被轨迹页占用**（就是它现在的键）|
| `PgUp` / `PgDn` | 翻**当前显示那一页**（`:4533` → `:2995`）；详情立着时滚详情（`:4386`）| L6 / L1 | **已被轨迹页占用** |
| 滚轮 | 详情 → 问卷（指针在问卷块内）→ 左栏页（指针在页区内）→ 当前页（`wheel_at` `:3042`、`:3081`）；一格 3 行（`src/render/pane.rs:27`）| 按指针位置，L1 / L7 之间 | **已被轨迹页占用** |
| 左键单击（按下 + 抬起，位移 ≤ 2 格）| `click_at` 的整串分派（`:3174`）：选择器框外关 / 详情框外关 / 问卷块内 / 归还输入区键盘 / 归还左栏键盘 / 页签 / 状态行两格 / 回合条 / 指示器 / 文件页 / 改动页 / todo 页 / 对话链 / **轨迹页一行开详情**（`:3293`）| 按指针位置 | **已被轨迹页占用**（开详情；要谈「先选中再开」就得改这一条）|
| 拖选（位移 > 2 格）| `selection::Drag`（门槛 `DRAG_THRESHOLD = 2`，`src/render/selection.rs:135`），抬起即复制（OSC 52，`:259`）| 按指针位置，最上层那一块文本区（`block_at`，`:86`）| **已被占用**（拖选优先于点击）|
| 滚轮以外的鼠标键（右 / 中）| **无绑定**：`mouse` 只认 `ScrollUp`/`ScrollDown`/`Down(Left)`/`Drag(Left)`/`Up(Left)`（`:3028`–`:3034`）| — | **空着**（右击可以作为候选，但今天连事件都不接）|
| 双击 | **没有手势识别**：`DRAG_THRESHOLD` 只区分「点击 vs 拖选」，两次相邻单击就是两次 `click_at` —— 第二下若落在覆盖层框内**什么都不做**（`:3184`），落在框外则把刚开的覆盖层关掉（`:3187`）| — | **空着**，但要新增识别（时间窗 + 同一行）|
| 悬停 / 长按 | 无（`MouseEventKind::Moved` 不认；`.scratch/trace-ledger/map.md` 已把 500 ms hover 列入「范围之外」）| — | 空着，但**范围之外** |
| `/` | 普通字符 → 编辑器 → 记号菜单（`token_menu`，`:4502`）。票 09 已记「`/` 已归命令菜单」| L7 → L5 | 不空（菜单）；**这一键不能再用作搜索入口**，除非分层征用 |
| `@` | 同 `/`，候选换成文件路径 | L7 → L5 | 不空 |
| `j` / `k`（裸键）| **全局无绑定**：问卷里是选项移动（`:1160`）、选择器里是移高亮（`:3683`）、nvim 查看器里原样转发；编辑器里就是普通文本 | L1 / L7 | **空着**（`Key::Char` 会落进草稿，但轨迹页今天没有键盘归属，所以「先取得键盘、再用 `j`/`k`」是干净的）|
| `r` | 改动页手动重取（`:3762`，**只在左栏键盘归属下**）| L2 | **空着**（轨迹页这一层没有它）|
| `n` / `N` / `f` / `F` / `{` / `}` / `?` / 数字键 | 无绑定，普通字符进草稿 | L7 | **空着**（都可征用，代价是打字时不能再打这些字符 —— 只在轨迹页持有键盘时成立）|
| `Ctrl-F` / `Ctrl-B` / `Ctrl-V` / `Ctrl-Y` 等未被 `map_key` 认的键 | **完全丢弃**：`map_key` 只认 13 个 Ctrl 字母（`:160`–`:181`），其余返回 `None`，`terminal_event` 直接不看（`:4264`）| — | **空着**（要新增：`Key` 变体 + `map_key` 一格）|
| `Space` | 草稿插空格；问卷选项 toggle | L7 / L1 | 不空 |

**分派规则的结论**：`questionnaire-keys` 与 `questionnaire-reading` 两条线留下的规矩是
**「键盘跟着当前那一层」，不是永远归某一方**（`.scratch/questionnaire-reading/spec.md:30`、
`docs/render.md:651`）：浮层立着时归浮层，关掉之后**原样还给下面那一层**（作答、焦点、翻到
第几题都留着）。同一个键在两层之间换含义是这条规矩的应有之义（`j`/`k` 在浮层里是滚动、
在问卷里是高亮）。L1 的次序就是这条规矩的实现：谁的支靠前，谁拿键盘（`src/render/tui.rs:4355`
–`:4396` 的注释逐条解释了次序）。

**轨迹页这一层今天还空着的键位**（可直接引用）：裸 `j` / `k`、`n` / `N`、`r`、`f`、`?`、
`{` / `}`、数字键、未被 `map_key` 认的 Ctrl 字母（要新增 `Key` 变体）、右键与中键、双击
（要新增识别）、悬停（范围之外）。**已被占用**的是 `PgUp` / `PgDn` / `Ctrl-G` / 滚轮 / 左键
单击 / 拖选；**要征用就得推翻**的是 `↑` / `↓`（编辑器光标）、`Enter`（提交）、`Esc`、
`Space`、`Tab` / `Shift+Tab`。

## 5. 文本层的旁路

三条旁路各自记账，而且**记录点都在画那一行的同一处**（「记下来的与看见的不会漂开」）：

- **拖选与复制**（`selection`）：`ScreenText` 每帧重建，`block_at` 取**最后画的那一块**
  （覆盖层因此在转录之上，`src/render/selection.rs:86`）；`TextRow { text, folded, lead, hotspots }`
  （`:34`），`folded` 决定复制时是否拼回软折续行（`:246`），`lead` 是靠右排出来的行才有的起始
  列（`:26`）。反白是**绘制路径的最后一步**，只往缓冲上 `insert(Modifier::REVERSED)`
  （`:184`–`:205`）。写出走 OSC 52（`:259`）。
- **可点链接**（`links`）：识别发生在**画之前**，结果一分为二 —— 下划线就地铺在这一行上、
  候选跟着进 `TextRow.hotspots`（`src/render/tui.rs:6866`、`src/render/links.rs:378`）。
  **只有对话视图认**：轨迹页 `note_rows` 传的是空表（`src/render/tui.rs:6886`）。
- **markdown 行内样式**：`Tag::Emphasis/Strong/Strikethrough` 各加一个修饰符、链接加
  `UNDERLINED`（`src/render/markdown.rs:224`–`:226`、`:454`），也是画之前就落在 `span.style` 上。

**会不会打架，谁优先**（结论，依据是上面的绘制次序）：

1. **`REVERSED` 是唯一真正冲突的通道**。`selection::paint` 在最后一步无条件 `insert`
   `REVERSED`（`src/render/selection.rs:205`），而全屏纪律是「唯一的反显就是键盘所在」
   （`src/render/tui.rs:5450`）。所以**搜索命中高亮、行选中态都不要用反显**：行选中用常驻
   选中那一档 `ACCENT + BOLD`（先例 `src/render/tui.rs:5866`、`:6117`、`:6980`），命中高亮
   用**底色**（先例 `palette::DIFF_ADDED` 那两档与气泡底色，`src/render/palette.rs:49`、
   `:142`）。底色与拖选能共存（一个是 `bg`、一个是 `Modifier`）。
2. **下划线通道归链接，而且是对话视图专属**。轨迹页的链接候选表是空的
   （`src/render/tui.rs:6886`），所以轨迹页这一层可以把 `UNDERLINED` 拿去表达别的东西而不撞
   —— 但**别用在「命中」上**：`links::mark` 是「画之前铺、记候选」，而命中态是「画之后才知道
   的过滤结果」，两者出处不同；重叠时以 `block_at` 的最上层与绘制次序为准。
3. **`lead` 与命中列的算术要一起算**：`hotspots` 的列区间是从这一行第 0 列起的（不含
   `lead`，`src/render/links.rs:32`），命中时两者相加才是屏幕列 —— 任何「按列数出来的高亮」
   都得跟着这条规矩走。
4. **软折会切开高亮**：一条命中若落在一行长文本里，它会被 `pane::wrap_line` 折成两片
   （`src/render/pane.rs:447`），所以命中必须先按**源行的列**算、再逐显示行切；`soft_folds`
   已经提供了「这一行是不是上一行的续行」的判据（`src/render/tui.rs:6902`）。
5. **折叠标记与 `▸` 撞车**：`▸` 今天的意思是「这一行点得开（有折起来的内容）」
   （`wording::FOLDABLE`，`src/render/wording.rs:1906`），`▾` 是「摊开了」`UNFOLDED`（`:1910`），
   两者作为**字形列**用在文件页（`src/render/tui.rs:5845`–`:5851`）。轨迹页今天在工具行、消息
   行、注入行上也打 `▸`（`:8104`、`:8240`、`:7958`）—— 所以**折叠态不能再用同一枚字形**去表达，
   否则「点得开」与「折起来了」在同一行上读作两件事。

## 6. 测试面

**帧测试的接缝**是 `draw_frame`：`Terminal::new(TestBackend::new(w, h))` 画一帧，把 buffer
克隆出来（`tests/render_layout.rs:118`）。在这之上有一小套 helper，全部按**人看得见的东西**
断言（文件头 `tests/render_layout.rs:1`–`:8` 写明这条纪律）：

- `screen(w, h, state) -> Vec<String>`：一帧一行一段文本（`:68`）；`row_text` / `cells` 按
  显示宽度前进，宽字素不会被读成空格（`:78`、`:90`）；`cell_of` 找屏幕上第一次出现某串文本的
  格子（`:6375`）。
- 指针事件是手搓的 `MouseEvent`：`press` / `release` / `drag_to`（`:5909`、`:5920`、`:5931`）
  与 `wheel_at`（`:8101`）；`click` = 按下 + 抬起（`:5942`），`click_text` / `click_in_row` 先
  画一帧再点（`:6386`、`:6399`）。
- **颜色与修饰符可以直接断言格子**：`frame[(column, row)].fg` / `.modifier`（先例
  `tests/render_layout.rs:1645`–`:1653`，选中页签是 `LightMagenta` + `BOLD`）。
- **轨迹页那一组**：`open_trace_tab`（`:8160`，点主列页签条**并再画一帧** —— 因为
  `trace_rect` 记的是上一帧）、`transcript_rows` 量出转录的高度（`:106`）、`trace_page`
  去空行（`:8124`）、`without_stamp` 砍掉行首 9 列（`:2537`）。典型断言：包含 / 不包含某串
  文本、切页后两页各自记自己的位置（`:8273`）、宽度变化重放（`:8373`）。
- **覆盖层那一组**：`a_click_opens_the_detail_and_a_second_click_closes_it`（`:6090`）、
  `the_detail_body_is_wrapped_to_the_real_text_width`（`:6121`）、
  `the_detail_body_scrolls_with_the_keys_and_the_wheel`（`:6146`）、
  `the_detail_overlay_ignores_every_key_but_its_own`（`:7377`）、
  `closing_a_trace_detail_returns_to_where_it_was_opened`（`:9256`）、
  `a_click_outside_the_detail_overlay_closes_it`（`:7567`）。
- **tab 命中那一组**：`only_the_tab_labels_answer_a_click`（`:2402`，逐列去找填线与分隔符，
  点它们不许切页）、`clicking_the_main_tab_switches_the_page_in_the_main_column`（`:8919`）、
  `a_terminal_with_no_sidebar_has_no_tabs_to_click`（`:2464`）。
- **按帧快照的先例**：仓库里**没有** `insta` 或写文件式快照。最接近的两件是
  「画一帧 → 读回 `Vec<String>` → 断言包含关系」，以及一条更重的：真 crossterm 后端 + `vt100`
  回放，专门断言 `TestBackend` 看不见的终端物理光标（`tests/render_layout.rs:5650`–`:5762`）。
  prototype 票要省力的话，`buffer` + `row_text` 那一对就是现成的「帧读取器」，把 `Vec<String>`
  在测试里手写成期望的几行即可。

## 7. 结论表

`parity.md` 每一条 → TUI 里用哪个件表达 → 有没有先例 → 要新增什么。行号指代码里的件。

### A. 账本的结构

| parity.md 的一条 | TUI 里用哪个件表达 | 有没有先例 | 要新增什么 |
| --- | --- | --- | --- |
| 回合之间分隔线 + `#N` 角标（当前回合高亮）| 分隔线已有（`trace_rule`，`:8039`）；角标画在单位第一条源行的行首 | 虚线先例 `:2419`；高亮用 `ACCENT + BOLD`（`:6117`）| 单位的第一条源行下标（`TurnRail.head` 已有，`:1763`，但它在**对话** pane 上，`:2533` 只在对话侧裁）；轨迹页那一侧的「单位边界」表 |
| turn → `Message` / `Step N` 两级分组 | 组头一行 + 成员行缩进；结构在**源行生成期**插 | 缩进 `wording::INDENT`（`:1936`）；空行分段先例 `push_view_lines`（`:2548`）| 轨迹页的「块 → 单位 / 迭代」归属表（事件流里 `TurnStarted.iteration` 有事实，渲染层没有表）|
| 组头摘要（墙钟跨度 + 工具直方图）| 组头行的文案 | 文案生成先例一整套在 `wording.rs`；跨度可算（`Event.at`）| 每单位一份累加器（今天只有 `turn_usage` / `turn_calls` 两个计数器，`:2286`–`:2287`）|
| 每行 `#N` 请求编号 | 行首 9 列（`STAMP_COLUMNS`，`src/render/layout.rs:58`）里腾位 | `stamp_lines`（`:8028`）与 `layout::STAMP_COLUMNS`；块层已有 `Block::TurnStarted.iteration`（绘制点 `:7835`）| 与时间戳争列的取舍（归票 04）；「把 iteration 画成行首角标」这件事渲染层没有 |
| 失败：行上给错误 code | 工具行末尾已有红色 `失败`（`:8116`–`:8121`），可扩成 `失败 · <code>` | `palette::BAD` + `tool_failed()` | **数据不在块里**：`ToolOutcome` 只有 `ok` / `output` / `error` / `duration_ms`（`src/render/transcript.rs:168`–`:173`），没有错误 code —— code 只在 `SessionError { code, detail }` 那条路出现（`:7944`）。工具错误要不要带 code，归数据面票 |
| 工具行 `name · args → 结果` | 一行（今天 `wording::tool_call_line`，`:8110`）+ 结果摘要可加 | `:8092` 的 `tool_block_lines` 整块 | 结果摘要的截断宽度（今天结果只在详情里）|
| `Between turns` 区段 | `Block::History` 已是一条叙述行（`:7980`）| `narration()`（`:8048`）| 要不要自己的小标题（归票 04）|
| 子调用缩进 / 工具变更提示 | —（不做，`map.md` 范围之外）| — | — |

### B. 时间

| parity.md 的一条 | TUI 里用哪个件表达 | 有没有先例 | 要新增什么 |
| --- | --- | --- | --- |
| 每行耗时（补齐）| 行尾追加（像用量尾巴那样）或行首块；`DetailKind::Tool` 加一段 | **行尾追加的现成接头**：`Pane::append_to_last`（`src/render/pane.rs:102`）+ `Tail`（`:1773`），ADR 0016 那条路 | `ToolOutcome.duration_ms` 上屏（字段在 `src/render/transcript.rs:173`，`DetailKind::Tool` 与绘制路径都没取它）|
| TTFT / 生成 / 吞吐 | —（等数据面票）| 流式中只画开始标记的纪律已有：思考行定稿才补完成时刻（`:2954`、`:8000`）| 数据源本身（票 01）|
| timing overview 三泳道 + 投影模式 | 一条横向块字符条（`▓`/`░`，`:1928`）铺在固定几行上 | `panel::row`（`src/render/panel.rs:172`）是唯一先例；刻度感可借 `wording::BAR_*` 与四分之一块（`:1803`）| 「时间 → 列」的映射、三种投影的算法、以及与 pane 窗口的锚定（归票 05/06）|
| 流式不编造时长 | 当约束用 | 思考行（`:2954`）| — |
| 本地时间 / Unix 秒切换 | —（不做）| `wording::stamp`（`:1893`）固定本地 | — |

### C. 检视器

| parity.md 的一条 | TUI 里用哪个件表达 | 有没有先例 | 要新增什么 |
| --- | --- | --- | --- |
| 分面切换（补齐）| 覆盖层里一条标签条 + 每面一份正文 | `draw_label_bar`（`:6104`）的标签条与命中矩形；`section_header`（`:9003`）的单面分段 | `DetailView` 加「当前面」与逐面 `Vec<DetailLine>`；`detail_scroll` / 页脚 / 键位表认当前面 |
| 用量面（变形做）| 分面里的一页：两组数字 + 分桶 | 行尾 `in=… out=…` 不变（ADR 0016）；桶的读数在 `events::Usage`（`cached_tokens` / `miss_tokens` / `reasoning_tokens`，`src/events.rs:191`–`:194`；`src/render/panel.rs:96`–`:99` 已经在读前两个）| 每**次调用**的用量快照（今天只在收尾时贴最后一次，`:2286`–`:2287`）|
| 计时面（补齐）| 同上一页的键值对 | 详情里 `参数` / `输出` 两段的行式（`:8834`）| 计时字段（同上，等数据面）|
| Raw / Summary 二分（变形做）| 同一份正文的两种读法：Markdown 渲染（今天）vs 逐块原文 | 渲染版走 `markdown::to_lines_indented`（`:7722`）；原文版可以照 `folded_text` 直接排（`:8586`）| 「这一面是哪种读法」的状态；`Message` 面今天只有渲染那一档 |
| 参数面 + 代码面（变形做）| 参数是 JSON 时两种画法（缩进 vs 键值对）| 今天 `serde_json::to_string_pretty` + 折行（`:8837`）| 摊成键值对的排版（新件）；`run_code` 那一档无对应物（不做）|
| 结果面（变形做）| 完整 JSON 要不要树 vs 原文 + 缩进；错误首行标红 | 今天按文本画（`:8849`）；红色先例 `palette::BAD`（`:8121`）| JSON 探测与首行标红（新件）|
| Schema 面（变形做）| 一页只读正文 | 同 `folded_text` | schema 的来源（工具描述里的文本）|
| system prompt / 差异面（变形做）| 注入块已可点开（`DetailKind::Context`，`:7967`）| `section_header`（`:9003`）+ `folded_text` 折行（`:8586`）| 前后 prompt 的差异面（**未证实**：`DetailKind::Context` 只带一份 `source` + `content`，`:8461`，没有任何「前一份」）|
| 层级跳转（补齐）| 打开详情时顺手记下「所属源行 / 单位」，再用 `Pane::scroll_to_source` 跳 | `scroll_to_source`（`src/render/pane.rs:193`）+ `jump_to_unit`（`:4006`）| `Detail` 里加「我来自哪一条源行 / 哪个单位」；`DetailKind::Tool` 已有 `tool_call_id`（`:8487`）可当锚 |
| 图像 / thinking 排版 / 拖拽调宽 | —（不做）| — | — |

### D. 检索与折叠

| parity.md 的一条 | TUI 里用哪个件表达 | 有没有先例 | 要新增什么 |
| --- | --- | --- | --- |
| 全文搜索索引 + 查询（补齐）| 索引建在**块**上（`painted` 是现成的全量记录，`:799`），不是屏上文字 | `Painted { Block, Thinking, Thought }`（`:1800`）就是「每条记录」的现成容器 | 索引结构、失效时机、查询；输入位置（`/` 与 `@` 已占，见第 4 节）|
| 命中后只剩匹配行（变形做）| `Pane` 加一层「过滤集」：显示行由源行的过滤结果决定 | 今天 `Pane` 只有 `lines/starts/wrapped`（`src/render/pane.rs:32`–`:38`），**没有任何过滤概念** | 过滤集 + `view()` 的取窗口改成「第 n 个可见源行」（`window`，`src/render/pane.rs:396`）；与 `Drawn.rows` / `soft_folds` / `source_at` 的配合 |
| 时间轴上未命中淡出 | 时间轴条的空档 | 同上 | 与过滤集共用同一个集合（归票 09 拍）|
| 折叠：整个 turn 折一行摘要（补齐）| 折起来的那一行在**源行生成期**替换掉成员行 | 折叠字形 `FOLDABLE` / `UNFOLDED`（`:1906`、`:1910`）与文件页的字形列（`:5845`）| 折叠状态表 + 「折起来时这一块还生成几行」的规则（牵动 `painted` 重放，`:2479`）|
| 折叠：连续工具调用折一行（补齐）| 同上一行，判据是「同发言者 + 连续 `Block::Tool`」| 连续性判据今天只在 `Flow` 里（`:2559`–`:2568`），是**流式去重**用的 | 一条「连续工具段」的识别与计数 |
| 全折 / 全展按钮、双击行折叠（变形做）| 点页面上的按钮（`Regions` + `HitAction` 新变体）或双击 | 按钮先例 `HitAction` 十三个变体（`:1453`）；双击**没有识别**（第 4 节）| 新 `HitAction` 变体；双击的时间窗与「同一行」判据 |
| 选择 / 折叠 / 搜索只覆盖已加载窗口 | 已对齐（`Pane` 本来只有一段）| `CAP = 20_000`（`src/render/pane.rs:21`）| — |

### E. 长历史与导航

| parity.md 的一条 | TUI 里用哪个件表达 | 有没有先例 | 要新增什么 |
| --- | --- | --- | --- |
| 虚拟化 | —（不做，平台）| — | — |
| 分页 / 「加载更早的历史」（变形做）| 从 `log.jsonl` 读出更早的块，**前插**进 `painted` 再重放 | 重放通路现成：`emit_painted`（`:2479`）与 `rerender_if_width_changed`（`:2441`）| 前插会与 `trace_links` / `TurnRail` / 折叠状态全部错位（它们都是位置型，第 2 节）；要一套能活过前插的行身份 |
| 聚合条数（变形做）| 账本顶部或状态行一句「共 N 行 / 已加载 M 行」| 计数行先例：详情页脚 `↕ n/N`（`wording.rs:425`）；`Pane::total()` / `sources()` 两个读口（`src/render/pane.rs:299`、`:304`）| 一句文案 + 它画在哪（页签条那一行已被页签占）|
| 初始停尾 / 向上滚暂停跟随 | 已对齐 | `follow` / `fresh` / `set_holding`（`src/render/pane.rs:260`、`:290`、`:269`）| — |
| **行选择**（补齐）| 焦点行 + 视口跟随 + `Enter` 开详情 | 焦点行与「跟着焦点滚窗口」的整套先例在左栏两页：`files_move_focus`（`:3774`）+ `files_scroll_to_focus`（`:3790`）、`changes_move_focus`（`:4718`）；选中态 `ACCENT + BOLD`（`:5866`）| **一个新键盘归属**（轨迹页今天不在 `key` 的分派链上，第 4 节）；「选中行 → 源行」的稳定指代（第 2 节）；只有「源行 → 显示行」一个方向（`scroll_to_source`），**反方向要新增读口** |
| 键盘可达（变形做）| `Esc` 每一层都有出口（既有纪律，`docs/render.md:651`）| 多层 `Esc`（`:4351`、`:4432`）| 轨迹页那一层的 `Esc` 语义要定（清选中？还是什么都不做）|
| 深链接 / 选择持久化 | —（不做）| — | — |
| 时间轴区间聚焦（变形做）| 与搜索过滤同一套「过滤集」| 无 | 区间 → 命中行的映射（归票 06 / 09）|

## 给下游票的话

喂给三张票，硬事实如下。

**给 [搜索与折叠](../issues/09-grilling-search-and-folding.md)**：

1. **轨迹页今天不是键盘归属的一层。** `key` 的守卫阶梯（`src/render/tui.rs:4337`–`:4542`）
   里没有它的位置；它只在「当前显示的那一页」这一支拿到 `PgUp` / `PgDn` / `Ctrl-G`
   （`:4533`）与滚轮（`:2987`）。要一个搜索入口键或折叠键，**先得回答「谁拿着键盘」** ——
   要么像 `sidebar_keyboard` 那样新增一位（先例 `:3555`），要么把 `current_page_*` 那一族
   扩成「当前页的完整键表」。
2. **空着的键**（可直接用）：裸 `j` / `k`（全局无绑定）、`n` / `N`、`r`、`f`、`?`、`{` / `}`、
   数字键、`Ctrl-F` 一类未被 `map_key` 认的 Ctrl 字母（要加 `Key` 变体 + `map_key` 一格，
   `:160`）、右键与中键（`mouse` 只认左键三相 + 滚轮，`:3028`）、双击（**没有识别机制**，要
   新增时间窗）。**`/` 与 `@` 已归记号菜单、`Tab` 归菜单补全、`Shift+Tab` 归模式循环**
   （`.scratch/trace-in-main/spec.md:135`），不要动。
3. **命中高亮不要用反显**：`selection::paint` 在绘制最后一步无条件 `insert(REVERSED)`
   （`src/render/selection.rs:205`），而「全屏唯一的反显就是键盘所在」是一条既有纪律
   （`src/render/tui.rs:5450`）。用**底色**（先例 `palette::BUBBLE` `:49`、`DIFF_ADDED`
   `:142`）；行选中沿用 `ACCENT + BOLD`（`:5866`）。
4. **折叠标记不能再用 `▸`**：它今天的语义是「这一行点得开」（`wording.rs:1906`），轨迹页已在
   工具行 / 消息行 / 注入行上打它（`:8104`、`:8240`、`:7958`）。
5. **索引建在块上、不是屏上文字**：`painted`（`:799`）就是现成的全量记录，`Painted` 三个
   变体（`:1800`）覆盖块、思考中、思考完成。过滤集要新增在 `Pane` 上（今天它只有
   `lines/starts/wrapped`，`src/render/pane.rs:32`），并牵动 `view()` 的取窗口（`:396`）、
   `source_at`（`:140`）、`soft_folds`（`:6902`）。

**给 [长历史与行选择](../issues/10-grilling-history-and-selection.md)**：

1. **行是位置型下标，不是稳定键。** 源行下标会被 `evict` 全体左移（`src/render/pane.rs:375`），
   也会被宽度变化整批重放（`src/render/tui.rs:2441`）。`trace_links`（`:920`）与
   `TurnRail.lines`（`:1680`）都靠 `push` 报回来的丢弃数跟着裁（`:2522`、`:2533`）——
   选中行、折叠状态、搜索命中要活过这两件事，就得自己带一套稳定身份（今天没有）。
2. **行的寻址路径是两跳**：屏幕行 − `trace_drawn.top` → `Drawn.rows[offset]` → 源行下标 →
   `trace_links[source]`（`:4060`）。反向只有 `Pane::scroll_to_source` 一个入口
   （`src/render/pane.rs:193`），**没有「源行在第几个显示行」的读口** —— 「把选中行滚进视野」
   要新增它。
3. **焦点行与跟随的整套先例在左栏两页**：`files_move_focus`（`:3774`）+ `files_scroll_to_focus`
   （`:3790`）+ 选中态 `ACCENT + BOLD`（`files_line`，`:5866`）+ 点击把键盘交给这一页
   （`take_sidebar_keyboard`，`:3555`）+ 点别处归还（`take_input_keyboard`，`:3609`）。
   轨迹页照抄这套结构能省最多力气。
4. **`↑` / `↓` 今天是编辑器光标**（`:2788`），`Enter` 是提交（`:4530`）。冻结项 4 已拍
   `↑` / `↓` + `Enter` 给行选择，所以这一票要明确写下**推翻的代价**（轨迹页上光标移动 / 提交
   失效），并给出「什么时候归轨迹页」的判据（建议照 `sidebar_keyboard` 的做法记一位归属，
   而不是全局改键）。
5. **`CAP` 之后还有 `log.jsonl`**，而前插会让上面第 1 条里那几张平行表（`trace_links`、
   `TurnRail.lines`，以及窗口内部的 `starts`）全部错位 —— 若这一票决定开「加载更早」这个
   口子，先把行身份定下来。

**给 [检视器分面](../issues/07-prototype-inspector.md)**：

1. **面加在 `DetailView` 这一层**（`src/render/tui.rs:8548`）：今天一个覆盖层一个 `body`
   （打开那一刻按宽度排好的 `Vec<DetailLine>`），加面就是「当前面 + 逐面 body」，并让
   `detail_scroll`（`:8660`）、`detail_page`（`:8669`）、页脚（`wording.rs:425`）与五行键位
   （`:4379`–`:4391`）都认当前面。
2. **标签条有现成画法**：`draw_label_bar`（`:6104`）画「标签 + `┆` 分隔 + `┄` 填满 + 命中
   矩形」，`ACCENT + BOLD` 是选中档；覆盖层里已有 `chrome_block` + `Clear`（`:9381`）。
   面也可以不切换、改成 `section_header`（`:9003`）式的分段堆叠 —— 今天 `DetailKind::Tool`
   就是这种形状（`参数` / `输出` 两段，`:8834`）。
3. **尺寸的算术只有一处事实源**：`detail_text_width`（`src/render/layout.rs:251`）与绘制路径
   共用 `detail_padding`（`:624`）。面标签条要占行的话，改这里，**别在绘制里再算一遍** ——
   那条不等的两处实现正是「行号被算错」的旧账。
4. **覆盖层是 L1，独占键盘**：`Esc` / `Ctrl-D` 关，`↑` / `↓` / `PgUp` / `PgDn` 滚，**别的全
   忽略**（`:4379`，`tests/render_layout.rs:7377` 钉着）。加「切面键」就是往这一支里加，而
   「面 2 / 4」那句话可以挂在现有页脚那一行（`:9414`）。问卷立着时浮层底边被
   `overlay_floor` 抬住（`src/render/layout.rs:286`），分面不许压住那道题。
