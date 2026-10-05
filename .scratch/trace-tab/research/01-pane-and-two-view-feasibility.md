# 调研：两个视口的可行性（pane 的结构、折行、宽度变化、点击链）

> 一手取证来自 2026-10-05 charting 期的一次只读调研（子代理），逐条带 `文件:行号`。它是 [research：两个 Pane 共享 `painted` 的改造面](../issues/05-research-shared-painted-two-panes.md) 的起点材料，**不是结论的最终形态**。

## 结论

**折行有两级，第一级在 `pane.push` 之前。**

- **块级排版在 push 前、且已依赖宽度**：`emit_block`（`src/render/tui.rs:1795-1800`）→ `paint_block(block, colors, width)`（`4656`，宽度参数文档在 `4654-4655`）→ markdown `to_lines_indented(text, width, indent)`（`4677`）；[`markdown.rs`](../../../src/render/markdown.rs) 的模块注释明说「源行不再宽度无关」（`:12-13`），表格列宽按 `width` 算（`546-580`）、超宽代码行按 `width` 折且续行保缩进（`676-701`）。
- **最终的按列硬折在 pane 内**：`push` 只收未折行（[`pane.rs:74-78`](../../../src/render/pane.rs)），真正折行在 `ensure` / `wrap_pending`（`266-303`）→ `wrap_line`（`392-412`），宽度来自每帧 `view(width, height, live)` 传进来的真实绘制宽度（`136-138`）。

⇒ 所以 **`RenderedLine` 不能当两个视图的共享源**。宽度无关的源是 `painted`（`Vec<Painted>`：`Block` / `Thinking` / `Thought`，`src/render/tui.rs:1426-1436`、`1789`）。

**「一份源 + 两个视口」的落点**：两个独立 `Pane` 各持自己的显示缓存（`wrapped` / `starts` / `width` / `top`），**共享同一份 `painted`**；宽度变化时各自 `clear` + 按自己宽度重放，事件只 `apply` 一次、`push_source` 分派两次。要动的面：

1. `render_width: u16` 单值（`src/render/tui.rs:680`、`1479`、`1779`、`1813-1817`）→ 每视口一个宽度；
2. `rerender_if_width_changed`（`1813-1830`）→ 每个视口按自己宽度各自 `clear` + 重放；
3. `push_source`（`1853-1860`）分派两处；
4. `live` 尾巴每帧按两个宽度各折一份（`pane.rs:138`、`tui.rs:4012`）；
5. `links`（`734`）与 `turn_rail`（`1336`）仍与**源行下标**一一对应，两个 pane 必须按同一批源行**同步裁剪**才能继续共享它们；`drawn_rows`（`737`）每视口一套。

**硬阻碍不是内存**，而是管线到处写着「一个 pane、一个宽度」：单值 `render_width`、无参重放的重建路径、`links` / `turn_rail` 与单 pane 平行、`drawn_rows` 单套。没有不可绕的结构障碍，改动横跨 `pane` / `tui` / `layout` 三层。

## 关键事实（逐条带证据）

- **`Pane` 是三份平行结构**：`lines: VecDeque<Line<'static>>`（未折的源行，`pane.rs:30`）、`starts`（每条源行起始的显示行，`32`）、`wrapped`（折行缓存，`34`）；另有 `wrapped_sources`(`36`)、`width`(`38`)、`height`(`40`)、`total`(`42`)、`top`(`44`)、`top_source`(`46`)、`follow`(`48`)、`seen`(`50`)、`holding`(`52`)。
- **裁剪**：`CAP = 20_000`（`pane.rs:19`）在 `evict`（`306-333`）执行，由 `push` 尾部调用（`77`）；裁掉**最旧的源行**（`317`）及其 `starts` 条目（`318`）与 `wrapped` 段（`319-322`），其余 `starts` / `top` / `total` / `seen` 整体上移（`325-331`）。tui 侧 `prune_links` 按同一 `CAP` 裁 `links` 并连带 `turn_rail.prune`（`2101-2110`）。
- **语义**：`top` = 视口顶显示行；`follow` = 贴底与否；`holding` = 详情覆盖层按住「新行」计数（`226-231`）；`fresh` = 离开底部后到达的显示行数（`247-253`）；`total` = 上一帧显示行数（`256-258`）。
- **两个映射各服务谁**：`source_at`（`118-129`，显示行 → 源行）服务 `drawn_rows`（链接点击，`tui.rs:4016-4018`）与 `focused_turn`（rail 高亮，`2337`）；`scroll_to_source`（`165-175`，源行 → 顶对齐 `top`）只服务 `jump_to_unit`（rail 点击，`2345-2350`）。滚动条两者都不用，它读 `total` / `top` / `following`（`4068-4082`）。
- **宽度变化 = 清空 + 按新宽度全量重放**：`rerender_if_width_changed`（`1813-1830`）里 `pane.clear()`(`1821`) + `links.clear()`(`1822`) + `turn_rail.clear()`(`1823`)，再整批 `emit_painted`（`1833-1847`）；唯一调用点 `4009`，宽度取 `panes.transcript_text().width`。`Pane::clear`（`pane.rs:106-112`）清三份缓存但保留 `follow` / `holding` / `top_source` 意图。
- **左栏几何**：`sidebar_page` = `x: sidebar.x`、`y: sidebar.y + kind.rows() + TAB_ROWS(3)`、宽 = 档位（40 / 28）、**高 = `fields`**（`layout.rs:349-356`；`fields` 最多 6、地板 3，`71-72`、`395-419`）。阈值 120 / 80（`50-53`、`367-378`），**40×10 下左栏整个人不存在**（`tier = None`，`308`、`349`）；80×10 时是 `(Hidden, 6)`。
- **点击链**：`RenderedLine { line, link }`（`tui.rs:4602-4609`）在 `push_source` 处被拆开——line 进 pane，link 进旁路 `links: VecDeque<Option<Detail>>`（`734`、`1853-1859`），与 pane 平行、按同一 `CAP` 裁。命中：`mouse`（`2187-2202`）→ `link_hit`（`2355-2359`）：`offset = row − drawn_top` → `drawn_rows[offset]` → `links[row]` → `open_detail`（`5109-5117`）。
- **`--continue` 全量重播与实时事件同一条路**：`cli.rs:439-441` → `input.rs:169-171` / `107` → `tui.rs:2463-2471` → `replay_batch`（`1898-1922`，每批 512 事件）→ `apply`（`1696`）→ `push_block`（`1785`）→ `emit_block`（`1795`）→ `push_source` → `pane.push`。

## 未证实 / 待定

- 轨迹视图在 28 / 40 列下是否仍走 markdown 排版，还是退成纯文本截断 —— 归[轨迹视图排版那张票](../issues/01-prototype-trace-page-layout.md)。
- 40×10 下左栏不存在，所以轨迹视图在那里没有落点 —— 已由冻结项 4（对话视图退回全量）覆盖。
- 以上均为静态阅读，未跑构建与测试；`Pane` 目前只有 `TuiState` 一个持有者（`tui.rs:1474`），没有别处复用可参照。
