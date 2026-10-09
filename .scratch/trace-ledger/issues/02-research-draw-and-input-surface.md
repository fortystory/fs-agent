# 衡的绘制与输入面：这次改动能站在哪些件上

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

轨迹页要升格成**可寻址的账本**（[图](../map.md) 冻结项 4）：行要能选中，要能被折叠、被搜索
命中、被时间轴区间命中；检视器要分面；时间轴要在字符格里画。这些都得站在衡已有的绘制与输入
件上。**先把这些件清点清楚**，后面的 prototype 票才不至于发明一套平行的机制。

**这是只读调研，不改任何代码。** 结论一律给 `文件:行号`。

要清点七件事：

1. **字符格里的绘制件**。今天有没有画过条形 / 迷你图的先例（用量那两行的占比色条在
   `src/render/` 哪里、怎么算的）；`Span` / `Line` 能表达什么（半格字符用过没有、`DIM` 与
   底色的用法）；一个字符格里最多能塞几种语义（前景色 + 背景色 + 修饰符）。
2. **行的身份与寻址**。`pane::Pane` 的行模型（`push` 返回什么、`CAP` 怎么裁）；一块（`Painted`）
   与它画出来的显示行的对应关系怎么记（`trace_links` / `RenderedLine::linked` /
   `trace_link_at`）；一次点击从屏幕坐标回到块的整条路径；`HitAction` 与命中矩形是怎么记的。
3. **覆盖层（详情）**。`DetailKind` 七种各自的绘制入口与尺寸规则；`detail_opener` 那份
   「打开方 + 打开前的滚动状态」记账；覆盖层自己的滚动、键位与页脚。分面要加在哪一层。
4. **键位占用表**（这张表是 [搜索与折叠](09-grilling-search-and-folding.md) 与
   [长历史与行选择](10-grilling-history-and-selection.md) 的前置）：TUI 里**已经**被占用的键
   全集（方向键、`PgUp` / `PgDn` / `Ctrl-G` / `Ctrl-O` / `Ctrl-T` / `Ctrl-Z` / `Ctrl-C` /
   `Ctrl-D` / `Tab` / `Shift+Tab` / `Esc` / `/` / `@` / 鼠标滚轮与点击），以及「键盘跟着当前那一层」
   的分派规则（`questionnaire-keys` → `questionnaire-reading` 那条线）。哪些键在轨迹页这一层
   还是空的。
5. **文本层的旁路**。拖选与复制（`selection` 的 `TextRow.lead`、OSC 52）、可点链接（`links`）、
   markdown 的行内样式 —— 搜索命中的高亮、行选中态、折叠标记要是也往行上叠，会不会与这三样
   打架（各自由谁记账、谁的优先级高）。
6. **测试面**。帧测试层（`TestBackend` + 「画一帧、读屏幕」的 helper）今天能断言什么；
   `tests/render_layout.rs` 里轨迹页那组、覆盖层那组、tab 命中那组各自是怎么写的；
   有没有「按帧快照」的先例可以给 prototype 票省力。
7. **结论表**：`parity.md` 里每一条「补齐 / 变形做」→ TUI 里用哪个件表达 → 有没有先例 →
   要新增什么。**这一节是本票的产物主体。**

## 产物

`research/02-draw-and-input-surface.md`（本目录下）。写完在票底给 `## 作答`，其中**键位占用表**
与**「要新增什么」那一栏**必须能直接被 [搜索与折叠](09-grilling-search-and-folding.md) 与
[长历史与行选择](10-grilling-history-and-selection.md) 引用。

## 作答

（下文写成 `:NNNN` 的短式行号一律指 `src/render/tui.rs`。）

- **绘制件**：今天唯一的数据条形先例是用量页那条横向块字符条（`▓` / `░`，`src/render/panel.rs:172`，
  条宽上限 10 列 `src/render/panel.rs:214`）；一个格里能叠的语义是「字形 + 前景 + 背景 + 若干
  修饰符」，两层叠加的先例是补丁的语法前景 `patch` 在 diff 背景上（`:8959`）；半格只有左栏那块
  静态字形用过（`src/render/wording.rs:1803`，一格 2×2 子像素）。
- **绘制在哪一层**：`draw_frame` 每帧重建三份记账（命中矩形 `Regions`、屏幕文本层 `ScreenText`、
  行映射 `Drawn`，`src/render/tui.rs:5515`–`:5529`），指针只回应上一帧记下来的东西；**轨迹页的行
  不走 `Regions`**，它靠 `trace_links`（`:920`）+ `trace_rect` 这一对。
- **行的寻址路径**：屏幕行减 `trace_drawn.top` → `Drawn.rows` 得源行下标 → `trace_links[source]`
  得 `Detail`（`trace_link_at`，`:4060`）；反向只有 `Pane::scroll_to_source` 一个入口
  （`src/render/pane.rs:193`）。**行身份是位置型下标**：`CAP` 裁剪会让全体左移
  （`src/render/pane.rs:375`），宽度变化会整批重放（`:2441`）—— 折叠 / 命中 / 选中要活过这两件事，
  得自带稳定身份（今天没有）。
- **覆盖层的接口**：面加在 `DetailView` 这一层（`src/render/tui.rs:8548`）—— 今天一个覆盖层一个
  `body`，加面就是「当前面 + 逐面 `Vec<DetailLine>`」，并让 `detail_scroll` / 页脚 / 那五行键位认
  当前面；标签条可复用 `draw_label_bar`（`:6104`），尺寸算术只有 `detail_text_width` /
  `detail_padding` 一处（`src/render/layout.rs:251`、`:624`）。
- **还空着的键位**：裸 `j` / `k`（全局无绑定）、`n` / `N`、`r`、`f`、`?`、`{` / `}`、数字键、
  未被 `map_key` 认的 Ctrl 字母（要加 `Key` 变体）、右键与中键（`mouse` 只认左键三相 + 滚轮）、
  双击（**没有识别机制**，要新增时间窗）、悬停（范围之外）。`PgUp` / `PgDn` / `Ctrl-G` / 滚轮 /
  左键单击 / 拖选已被轨迹页占用；`↑` / `↓`（编辑器光标）、`Enter`（提交）、`Esc`、`Space`、
  `Tab` / `Shift+Tab` **要征用就得推翻**。**轨迹页今天不是键盘归属的一层**（`key` 的守卫阶梯
  `:4337`–`:4542` 里没有它），行选择或搜索入口要先回答「谁拿着键盘」。
- 完整事实与逐条结论表见 [`research/02-draw-and-input-surface.md`](../research/02-draw-and-input-surface.md)。
