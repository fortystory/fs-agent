# 调研：两个 Pane 共享 `painted` 的改造面

> 一手取证来自 2026-10-05 的一次**只读**调研（未改 `src/`），逐条带 `文件:行号`。它服务
> [research：两个 Pane 共享 `painted` 的改造面](../issues/05-research-shared-painted-two-panes.md)，
> 是 [可行性调研](01-pane-and-two-view-feasibility.md) 的续篇：前者钉结构，本篇钉**改造面与测试影响**，
> 供 `/to-spec` 直接引用。冻结项以 [map](../map.md) 第 11 条与第 2/8/9/10 条为准。

## 结论（先给答案）

**能改，不必动 `Block` 与事件 schema**（分工的过滤是绘制侧的选择，`Block` 层照旧共享）。
但有一处必须先解决的结构账：`painted` 是共享的，可两个视口的**源行序列并不相同**
（冻结项 2 的分工：对话视图只留用户文本 + assistant 正文 + 四类例外），所以
`links` / `turn_rail` 不能再是**单套**、也不能靠「两个 CAP 碰巧同速」维持平行。

五问的短答：

1. **要拆。** 且必须带**目标掩码**：只 `clear` + 重放**宽度真的变了**的那个 pane；对没变的
   pane 重放会把它的源行推第二遍。
2. `push_source` **不自己分派两次**；分派点上移到 `emit_block`——它才是同时知道「画给谁」
   「按多宽画」的地方。`live` 尾巴**不需要**新的按宽度缓存：`Pane::view` 每帧本来就按传进来的
   宽度折（`src/render/pane.rs:138`），两个 pane 各调一次 `view` 就各自折一份。
3. 让 **pane 自己的 `evict` 当裁剪记账的唯一权威**（`push` 返回本次丢了几条源行），
   tui 侧按这个数裁平行表；今天 tui 侧 `prune_links` 独立数 `CAP`，与 pane 的 `evict` 只是
   **碰巧**同速（两边整数一样、每条源行各 push 一次）。分工之后两个 pane 的源行序列不同，
   「一套 `links` 平行于两个 pane」在数学上就不成立——平行表必须**每个 pane 一套**。
4. `drawn_rows` / `drawn_top` 是**只服务点击**的映射（链接命中），每视口一套；按指针落在
   `sidebar_page` 还是转录区来分派。`focused_turn`（rail 高亮）、`jump_to_unit`（rail 点击）、
   滚动条、指示器仍**只绑对话 pane**，rail 也只在对话 pane 上建、只按对话 pane 的裁剪数裁。
5. 测试面：`tests/render_tui.rs` **零命中**（它从不画帧、不碰 `pane`/宽度/`drawn_rows`，
   只用 `pane::wrap_text` 这个纯函数，`tests/render_tui.rs:829`）。`tests/render_layout.rs`
   有命中；**机器拆分本身（切片 0–3）应保持现有 147+53 全绿**，真正大面积翻红的是
   **过滤接上之后**（切片 4）——清单见下面「测试影响」。

基线（本次实跑，未改任何 `src/`）：

```
cargo test --test render_layout   → 147 passed; 0 failed; 0 ignored   (0.42s)
cargo test --test render_tui      →  53 passed; 0 failed; 0 ignored   (0.02s)
```

## 关键事实

### 现在的单 pane 管线（都是单数）

- **持有者**：`TuiState` 只有 `pane: Pane`（`src/render/tui.rs:670`）、`live: String`（`672`）、
  `painted: Vec<Painted>`（`678`）、`render_width: u16`（`680`）、`links`（`734`）、
  `turn_rail`（`731`）、`drawn_rows`（`737`）、`drawn_top`（`740`）。
- **喂入的唯一漏斗**：`push_source(line, link, user)`（`1853-1860`）→ `pane.push`（`1854`）+
  `links.push_back`（`1855`）+ 可选 `turn_rail.push_line`（`1856-1858`）+ `prune_links`（`1859`）。
  调用点只有三处：`emit_block`（`1799`）、`emit_painted` 的两个分支（`1840`、`1844`）、
  `open_thinking`（`2009`）。
- **`painted` 是共享源**：`Painted::{Block, Thinking, Thought}`（`1426-1436`），只在
  `push_block`（`1789`）、`open_thinking`（`2011`）、`settle_thinking`（`2090-2093`）里追加。
  **它今天没有上限、从不裁剪**（grep `self.painted` 只有追加与重放两处，`1818-1828`）。
- **`render_width` 是「源行按多宽排的」**，不是显示宽度：`paint_block(block, colors, width)`
  收宽度（`4656`），markdown 的表格列宽/超宽代码行按它排版（`4677`；
  `src/render/markdown.rs:546-580`、`676-701`）。三处使用：`new` 初值（`1479`）、
  `apply` 里 `push_block(block, self.render_width)`（`1779`）、`rerender_if_width_changed`
  （`1813-1817`）。**真正的显示宽度在 `Pane::width` 里**（`pane.rs:38`，由 `view`→`ensure`
  写），两者今天恰好相等，因为 `draw_transcript` 把同一个 `text_area.width` 既给
  `rerender_if_width_changed`（`4009`）又给 `pane.view`（`4012`）。
- **宽度变化 = 清空 + 全量重放**：`rerender_if_width_changed`（`1813-1830`）里
  `pane.clear()`（`1821`）+ `links.clear()`（`1822`）+ `turn_rail.clear()`（`1823`），
  `mem::take(painted)`（`1824`）后逐条 `emit_painted(item, width)`（`1825-1827`）再放回（`1828`）。
  唯一调用点 `draw_transcript`（`4009`）。`Pane::clear` 保留 `follow`/`holding`/`top_source`
  意图（`pane.rs:106-112`）。
- **重放与实时同路**：`apply`（`1696`）→ `push_block`（`1785`）→ `emit_block`（`1795`）→
  `push_source`；重放走 `emit_painted`（`1833-1847`）→ 同一个 `emit_block` / `push_source`。

### 平行表与裁剪（第 3 问的证据）

- `Pane::evict`（`pane.rs:306-333`）在 `push` 尾部被调（`pane.rs:77`），按 `CAP = 20_000`
  （`pane.rs:19`）丢最旧的源行，并把 `starts`/`wrapped`/`top`/`total`/`seen`/`top_source`
  整体上移（`325-331`）。
- tui 侧 `prune_links`（`2101-2110`）**另起一个循环**按同一个 `CAP` 裁 `links`，再用**它自己
  数出来的** `dropped` 调 `turn_rail.prune(dropped)`（`2108`；`TurnRail::prune` 见 `1388-1396`）。
- 两者今天相等纯属约定：`pane.rs` 与 `tui.rs` 各拿一份 `pane::CAP`，每个源行各 `push_back`
  一次。**没有任何共享计数器**把它们绑在一起——这正是「两个 pane 同步裁剪怎么保证不漂」的
  症结。
- `turn_rail` 的两个索引都是**源行下标**：`unit_of(source)`（`1408-1413`）与 `head(unit)`
  （`1416-1418`）。`focused_turn` 用 `pane.source_at(pane.top())` 取那个下标（`2337`），
  `jump_to_unit` 用 `pane.scroll_to_source(head)` 落点（`2349`）。

### 点击/滚轮/rail 的现状（第 4 问的证据）

- `drawn_rows`/`drawn_top` **只被 `link_hit` 读**（`2356-2357`），而 `link_hit` 只被 `mouse`
  的左键分支读（`2198`）。它们每帧在 `draw_transcript` 里重建：
  `drawn_top = text_area.y`（`4015`），`drawn_rows[offset] = pane.source_at(pane.top()+offset)`
  （`4016-4018`）。
- `focused_turn`（`2329-2339`）**不用** `drawn_rows`，直接 `pane.source_at`；rail 的命中矩形
  在 `draw_turn_rail` 里逐格记进 `regions.cells`（`4059-4062`），`mouse` 先查
  `regions.action_at`（`2191-2193`）再落到 `link_hit`。
- 滚轮：无问卷时直接 `pane.wheel`（`2188-2189`）；有问卷时按指针在不在问卷块里分派
  （`2168-2175`）。键盘的 `PageUp`/`PageDown`/`CtrlG` 只喂 `pane`（`2742-2744`）。
- 左栏那一页今天由 `draw_sidebar_page` 画（`3508-3521`）；`Tab::Trace` 还是占位文案
  （`3515-3518`），枚举在 `3624-3632`。

## 逐问作答

### 1. `render_width: u16` → 每视口一个宽度；`rerender_if_width_changed` 要不要拆

**要拆成「每个 pane 各自 `clear` + 按自己宽度重放同一份 `painted`」，且必须按目标掩码拆。**

理由与顺序：

- 每个 pane 的源行按**它自己的内容宽度**排版：对话 pane 用 `panes.transcript_text().width`
  （`layout.rs:147-154`），轨迹 pane 用 `panes.sidebar_page.width`（`layout.rs:349-356`，宽档
  40 / 窄档 28）。所以旧值也要**分开存**：`conversation_width` / `trace_width`（或 `[u16; 2]`），
  初值都取 `SHARED_RENDER_WIDTH`（沿用 `1477-1479` 的理由：首帧发现真宽度后重放）。
- **只重放变了的那个 pane**。`emit_painted` 是**追加**（`1833-1847` → `push_source`），对没
  `clear` 的 pane 再放一遍会把它整份重画第二遍。所以 `rerender_if_width_changed` 需要
  一个 `{conversation, trace}` 位掩码。
- **`turn_rail.clear()` 只在对话 pane 重放时做**（`1823` 今天的含义：源行下标要重建）。
- 建议形状（示意，非最终代码）：

  ```rust
  fn rerender_if_width_changed(&mut self, conversation_width: u16, trace_width: u16) {
      let conversation = conversation_width != self.conversation_width;
      let trace = trace_width != self.trace_width;
      if !conversation && !trace { return; }
      self.conversation_width = conversation_width;
      self.trace_width = trace_width;
      if conversation {
          self.conversation.clear();
          self.conversation_links.clear();
          self.turn_rail.clear();          // rail 的源行下标与对话 pane 平行
      }
      if trace {
          self.trace.clear();
          self.trace_links.clear();
      }
      if self.painted.is_empty() { self.dirty = true; return; }
      let painted = std::mem::take(&mut self.painted);
      for item in &painted {
          self.emit_painted(item, Targets { conversation, trace });
      }
      self.painted = painted;
      self.dirty = true;
  }
  ```

- **调用点上移**。今天在 `draw_transcript` 里（`4009`），因为那时才知道转录宽；轨迹宽要到
  `layout::plan` 之后才知道，而 `plan` 在 `draw_frame` 里就跑完了（`3372`）。所以把唯一调用点
  移到 `draw_frame` 的 `plan`（`3372`）之后、`draw_shell`（`3383`）之前：

  ```rust
  let text_area = panes.transcript_text();
  let trace_width = match state.tab {
      Tab::Trace => panes.sidebar_page.map_or(0, |page| page.width),
      _ => 0,
  };
  state.rerender_if_width_changed(text_area.width, trace_width);
  ```

  注意两点：(a) `0` 表示「这个 pane 这一帧不物化」，不参与绘制；(b) 同一帧里
  `pane.view` 必须收到与 paint 宽度**同一个** `Rect.width`，否则源行按 A 宽排、显示按 B 宽折。

### 2. `push_source` 分派两次的形状；`live` 尾巴每帧要不要按两个宽度各折一份

- **分派点不在 `push_source`，在 `emit_block`。** `push_source` 是单 pane 的漏斗
  （`1853-1860`），它不知道宽度、也不知道块。正确形状是让 `emit_block`（`1795-1807`）变成
  「对每个被选中的目标，`paint_block` 一次、把结果喂给那个目标的 `push_source`」：

  ```rust
  fn emit_block(&mut self, block: &Block, targets: Targets) -> usize {
      let mut produced = 0;
      if targets.conversation && selects(Viewport::Conversation, block) {
          let lines = paint_block(block, &mut self.colors, self.conversation_width);
          produced = lines.len();
          for r in lines { self.push_line(Viewport::Conversation, r.line, r.link, Some(is_user_message(block))); }
      }
      if targets.trace && selects(Viewport::Trace, block) {
          let lines = paint_block(block, &mut self.colors, self.trace_width);
          produced = produced.max(lines.len());
          for r in lines { self.push_line(Viewport::Trace, r.line, r.link, Some(is_user_message(block))); }
      }
      // 边界只算一次，而且只在对话目标活着时算——见下面切片 2。
      if targets.conversation && is_boundary(block, self.discussion()) {
          self.turn_rail.close_unit();
      }
      produced
  }
  ```

  `push_line(view, ...)` 是 `push_source` 的分目标版本：选那个 view 的 pane / links，
  只有 `Viewport::Conversation` 才 `turn_rail.push_line`。
- **`paint_block` 画两遍是安全的**：`SpeakerColors::of` 幂等（`243-275`：名册命中的直接查表，
  中途出现的名字在 `extra` 里记住槽位），所以同一块按两个宽度各画一次不会分叉颜色。
- **`live` 不需要新机制。** `Pane::view(width, height, live)` 内部第一步就是
  `wrap_text(live, width)`（`pane.rs:138`），每帧、每个 pane、按各自宽度各折一份；`live`
  本身只有一份、按字符截到 `LIVE_BUFFER`（`tui.rs:1757-1766`），两 pane 读同一个
  `&state.live`。要决定的只是**轨迹视图显不显示流式尾巴**：冻结项 2 说轨迹画「assistant 的
  消息本身」，正文在 `MessageCompleted` 之前只以 `live` 存在，所以**显示**是自洽的；
  若 prototype 票 02 决定轨迹不画尾巴，就给它传 `""`。这是它唯一要选的地方，不需要缓存。
- **注意**：`Pane::view` 会写 `height`/`total`/`top`（`pane.rs:136-157`），所以**没被画出来
  的那个 pane 不要调手势方法**（它的 `height` 会是上一帧的）。切页/手势语义归
  [滚动语义那张票](../issues/04-grilling-scroll-and-follow.md)。

### 3. `links` / `turn_rail` 与源行下标一一对应；两个 pane 同步裁剪怎么保证不漂

**答案：把裁剪记账的唯一权威放到 pane 上，平行表跟着那个数走；并且平行表必须每 pane 一套。**

- 现在的漂移面：`Pane::evict`（`pane.rs:306-333`）数一份 `CAP`，`prune_links`
  （`2101-2110`）**再数一份**。两条计数今天相等，但那是「同一个常量 + 每条源行各 push 一次」
  的巧合，没有任何地方强制。分工之后两个 pane 收不同的源行子集，`links` 若还是一套，
  就必然对不上其中一个 pane。
- 具体改法：
  1. `Pane::push`（`pane.rs:74-78`）返回本次 `evict` 丢掉的源行数（0 或 1；返回 `usize`
     以便将来改变裁剪策略）。`evict` 保持私有，`CAP` 只在它里面用。
  2. 每个 pane 配自己的平行表：`conversation_links` + `turn_rail`（rail 与对话 pane 平行）、
     `trace_links`（轨迹 pane 的点击入口）。`push_line(view, ...)` 里对自己那个 pane
     拿 `dropped`，`for _ in 0..dropped { links.pop_front() }`；`turn_rail.prune(dropped)`
     只用**对话 pane** 的 `dropped`。
  3. `push_source`/`prune_links` 里那个独立数 `CAP` 的 `while` 循环删掉——它是漂移的来源。
- 这样「同步」的含义是**每个 pane 与它自己的平行表同步**，不是「两个 pane 之间同步」。
  **未证实 / 待定**：map 第 11 条的「两个 pane 同步裁剪」也可能被读成「两个 pane 用同一个
  CAP 计数」；若真要那样，就得有一个比源行下标更粗的**共享源 id**才能让两个不同子集的
  pane 谈「同一批源行」，那是更大的改动。谁触发、谁记账、`CAP` 是每视图一份还是每源一份，
  归 [滚动语义那张票](../issues/04-grilling-scroll-and-follow.md) 第 4 问；本调研只钉「机制上
  怎么不漂」=pane 当唯一权威 + 平行表分目标。
- `painted` 自身今天**无上限**（见上），所以「重放源」不会因为某个 pane 裁剪而缺行；重放
  时两个 pane 各自从 `painted` 重建自己的序列。

### 4. `drawn_rows` / `drawn_top` 每视口一套之后，点击与 rail 的取数怎么走

- `drawn_rows`/`drawn_top` 唯一消费者是 `link_hit`（`2356-2357`），唯一调用点是 `mouse`
  的左键分支（`2198`）。所以它们**只需每视口一套**，不需要动 rail。
- 具体：
  - `draw_transcript`（`4006-4023`）继续填**对话**的那一套（`4015-4018`）。
  - 新增的轨迹页绘制（`draw_sidebar_page` 的 `Tab::Trace` 分支，`3508-3521`）在 `view` 之后
    填**轨迹**的那一套，连同 `trace_links`。
  - `mouse`（`2190-2202`）在落到 `link_hit` 之前先判指针在哪个矩形：落在上一帧的
    `sidebar_page` 且 `tab == Trace` → 走轨迹的 `link_hit`，否则走对话的。**推荐把
    `sidebar_page` 的矩形像 `detail_rect`/`modal_rect` 一样在画出来时记下**（`3358-3361`、
    `743-752` 的那条纪律：「记住读的人真看到了什么」），而不是在 `mouse` 里用
    `layout::plan(self.area, 1, self.sidebar_wanted)` 重算（`2196-2197` 今天为 `detail_width`
    这么做；`sidebar_page` 的高度今天与 `draft_rows` 无关，重算碰巧也对，但那是巧合）。
  - 滚轮：`mouse` 的 `Scroll*` 分支（`2188-2189`）与问卷下的分支（`2174`）按同一个矩形判断，
    落到轨迹时喂 `trace.wheel`——这正是冻结项 13（轨迹只吃滚轮）。
  - **保持绑对话 pane 的**：`focused_turn`（`2337`）、`jump_to_unit`（`2349`）、
    `draw_scrollbar`（`4020`/`4068-4090`，函数已收 `&Pane`，可直接复用给轨迹）、
    `draw_indicator`（`4095-4127`）、以及 rail 的命中区域（`4059-4062`）。

### 5. 既有测试里哪些会红

**先说基线**：`tests/render_tui.rs` 53 条与 `tests/render_layout.rs` 147 条现在全绿（实跑，见上）。
`tests/render_tui.rs` **没有任何一条**依赖一个 pane / 一个 `render_width` / 单套 `drawn_rows`：
它只 `state.key(..)`/`state.apply(..)`/读状态，从不 `draw_frame`、不用 `mouse`/`wheel`；唯一
碰 `pane` 的是 `pane::wrap_text` 这个纯函数（`tests/render_tui.rs:829`，测 `src/render/pane.rs`
的折行，不涉及 TuiState 的 pane 实例）。唯一的例外面是 `render_block`/`Block` 共享层
（如 `a_notice_is_a_transcript_line_shown_as_it_is`）——那是 plain 侧，明确不动。

`tests/render_layout.rs` 的命中分三层，**务必分清是哪一层翻红**：

**(A) 机器拆分本身（切片 0–2）——预期全绿，是回归网**
- `a_resize_keeps_the_reader_on_the_same_line`（`tests/render_layout.rs:1952`）：120→80 触发
  `render_width` 变化 + 重放，钉「顶行是同一源行」。拆完之后对话 pane 的宽度逻辑不变，
  应仍绿；**过滤接上之后**它会红（内容用的是 `Notice`，见 C）。
- `narrowing_the_terminal_relays_a_table_out_by_the_new_width`（`:5965`）：120→70 触发
  表格按新宽度重排。内容是 assistant `Message`，留在对话视图，**应保持绿**，并且正是
  「每视口按自己宽度重放」这条不变量的现成测试。
- `ctrl_o_takes_the_sidebar_away_and_brings_it_back`（`:341`）：`open == reopened` 的逐格
  相等。切换会改对话 pane 宽度并触发重放；若重放确定（颜色槽位也稳定）应绿。**是颜色
  分配时机的哨兵**（见 C 末尾）。
- 其余几何/页签/菜单/问卷的用例与 pane 无关，不动。

**(B) rail 与 tab 命中——机制正确时应绿，但有两处必须照做**
- rail 组：`the_rail_grows_one_cell_per_turn_and_keeps_the_newest_at_the_foot`（`:1425`）、
  `the_truncation_mark_appears_only_where_units_were_cut`（`:1453`）、
  `the_rail_window_follows_the_focus_wherever_the_viewport_is`（`:1502`）、
  `the_focus_is_the_unit_the_top_row_belongs_to`（`:1530`）、
  `clicking_a_rail_cell_jumps_to_that_turns_question`（`:1561`）、
  `a_rail_cell_jump_at_the_end_clamps_to_the_bottom`（`:1591`）。
  它们用 `turns()`（`user_message`+`turn_started`+`assistant message`+`turn_ended`，`:1404-1418`）。
  `TurnStarted`/`TurnEnded` 属于过程行、会离开对话视图，**所以 `emit_block` 里的
  `turn_rail.close_unit()` 必须在块被过滤掉时也照调一次**（见切片 2 的 `targets.conversation`
  条件）；照做了这组才绿。否则 rail 不再长格。
- `a_discussion_counts_rounds_where_a_session_counts_turns`（`:1617`）：后半段点击 rail 后
  断言顶行是 `── 第 1 轮`（`RoundStarted` 行，`:1718`）。`RoundStarted`/`RoundEnded` 离开
  对话视图后这行不在屏幕上 → **会红**，需要改断言或改去轨迹页看。
- `clicking_a_tab_switches_the_sidebar_page`（`:1223`）：断言 `轨迹` 页是
  `wording::tab_placeholder()`（`:1238-1241`）。轨迹页变成真视图后 **必红**，要重写成
  「切到轨迹页看到轨迹内容」。
- `only_the_tab_labels_answer_a_click`（`:1264`）、`the_hidden_sidebar_has_no_tabs_to_click`
  （`:463`）、`a_terminal_with_no_sidebar_has_no_tabs_to_click`（`:1329`）、
  `the_four_tab_labels_fit_at_the_narrow_width`（`:3118`）：不碰轨迹页，**绿**。

**(C) 过滤接上之后——大面积翻红的主因（冻结项 2/8/9/10）**
对话视图只剩「用户文本 + assistant 正文 + 错误 / 中断 / 权限裁决 / hook 拒绝」，以下**今天
断言这些块出现在主转录里**，都会红（要么改去轨迹页断言，要么改用例外类块）：

- 提示/诊断行（`Notice`）——`Notice` 不在四类例外里，归轨迹：
  - `the_transcript_pane_shows_both_the_notices_and_the_streaming_tail`（`:742`，断言提示行在
    窗格里）**必红**。
  - `the_pane_scrolls_back_through_the_transcript_and_returns_to_the_bottom`（`:1725`）
  - `the_transcript_keeps_the_newest_twenty_thousand_source_lines`（`:1814`）
  - `the_indicator_counts_what_arrived_and_the_wheel_moves_three_rows`（`:1862`）
  - `the_scrollbar_column_is_reserved_and_filled_only_when_there_is_more_to_read`（`:1989`）
  - `a_resize_keeps_the_reader_on_the_same_line`（`:1952`）
  - `the_wheel_over_the_transcript_scrolls_it_while_a_questionnaire_is_up`（`:5027`，用
    `first_notice` 量滚动）
  - `the_detail_overlay_freezes_the_transcript`（`:5105`，用提示行当冻结的内容）
  - `a_settling_thinking_line_keeps_the_history_before_it`（`:5573`）
  - 用「`Notice` 只当垫料、断言别的东西」的那几条——`a_permission_question_lands_in_the_middle_as_a_covered_overlay`（`:2631`）、`the_menu_keeps_its_corners_over_text_that_is_not_ascii`（`:3546`）、`the_wheel_follows_the_pointer_while_a_question_is_up`（`:3843`）、
    `the_cursor_comes_back_to_the_draft_once_a_question_is_answered`（`:3939`）——**可能仍绿**，
    但转录会空掉，要逐条复看。
- 工具行与思考行（冻结项 2 归轨迹；思考「归轨迹」是明写的）：
  - 详情入口组（都 `click_row(.., "调用 bash")` 或点 `思考完成`）：
    `a_click_opens_the_detail_and_a_second_click_closes_it`（`:4217`）、
    `the_detail_body_scrolls_with_the_keys_and_the_wheel`（`:4247`）、
    `the_detail_overlay_reads_the_spilled_tool_output`（`:4289`）、
    `a_missing_spilled_file_degrades_to_the_preview`（`:4344`）、
    `a_question_in_the_way_keeps_the_collapsed_lines_unclickable`（`:4369`）、
    `ctrl_o_is_ignored_while_the_detail_overlay_is_up`（`:411`）、
    `the_detail_overlay_freezes_the_transcript`（`:5105`）、
    `a_question_closes_the_detail_overlay_instead_of_stacking_on_it`（`:5150`）、
    `ctrl_d_closes_the_detail_overlay`（`:5266`）、
    `a_tool_body_over_the_reading_limit_is_cut_and_says_so`（`:5288`）、
    `the_detail_overlay_ignores_every_key_but_its_own`（`:5346`）、
    `a_tool_call_is_on_screen_as_soon_as_its_result_arrives`（`:5395`）、
    `the_detail_overlay_is_wider_than_a_question`（`:5443`）、
    `a_click_outside_the_detail_overlay_closes_it`（`:5519`）、
    `a_complete_result_does_not_claim_its_text_is_unavailable`（`:5614`）、
    `a_cut_result_still_says_when_the_whole_text_is_gone`（`:5648`）、
    `the_detail_overlay_wears_the_speakers_colour_and_keeps_a_cell_of_air`（`:5751`）、
    `the_detail_footer_counts_the_last_row_on_screen`（`:5842`）。
    （逐字命中清单可由 `grep -n 'tool_started(' tests/render_layout.rs` 得到，共 23 条，
    含 `the_cursor_comes_back_to_the_draft_once_a_question_is_answered` 等垫料用法。）
  - 工具行与思考行本身的内容断言：
    `a_tool_result_is_folded_into_its_call_line`（`:4163`）、
    `a_tool_call_line_describes_the_call_and_folds_the_arguments_away`（`:5675`）、
    `the_call_line_wears_the_narration_grey_after_its_speakers_name`（`:5719`）。
  - 思考行组（`grep reasoning_delta(`，共 8 条）：
    `a_thinking_segment_opens_in_place_and_settles_in_place`（`:4082`）、
    `a_synthesizer_trace_streams_but_records_nothing`（`:4141`）、
    `one_message_never_gets_two_thinking_lines`（`:5071`）、
    `reasoning_never_joins_the_message_body`（`:5091`）、
    `a_thinking_line_tints_its_speakers_name`（`:5213`）、
    `reasoning_that_interleaves_opens_a_new_line_per_segment`（`:5241`）、
    `a_settling_thinking_line_keeps_the_history_before_it`（`:5573`）。
    （`a_turn_with_no_reasoning_adds_no_thinking_line`（`:4130`）是「什么都不加」，
    大概率仍绿。）
- 会**保持绿**的：assistant `Message` 的 markdown 组（`a_table_at_the_head_of_an_answer_lines_up_with_its_header`（`:6009`）、`a_code_block_is_highlighted_within_the_transcript`（`:6040`））、
  用户消息在对话视图，所以 rail 落点那几条的用户消息断言照旧。

**(D) 不属于本票、但同一批会动的（冻结项 14 页高）**
`every_size_in_the_matrix_draws_the_regions_its_budget_allows`（`:1039`）与
`the_sidebar_gives_up_its_identity_then_its_fields_as_it_shrinks`（`:1126`）钉的是「页高 =
`fields`」的那条阶梯（`layout.rs:349-356`、`395-419`）。页高解耦后必改，归
[页高那张票](../issues/03-prototype-sidebar-page-height.md)，这里只标出来免得记到本票账上。

## 实现切片建议（/to-spec 可直接引）

顺序按「先纯重构、后行为、再过滤」，每片都能单独跑测试：

**切片 0：pane 当裁剪记账的唯一权威（纯重构，测试应全绿）**
- 改 `Pane::push` 返回 `usize`（`src/render/pane.rs:74-78`）；`evict` 不变。
- 改 `push_source`（`src/render/tui.rs:1853-1860`）：用返回值裁 `links` 与 `turn_rail.prune`；
  删掉 `prune_links`（`2101-2110`）里独立数 `CAP` 的 `while`。
- 不新增测试；`cargo test --test render_layout` 应仍 147 绿。

**切片 1：两个 pane 持有者 + 两组宽度（机械，测试应全绿）**
- `TuiState`：`pane`→`conversation`，新增 `trace`；`render_width`→两组；`links`/`drawn_rows`/
  `drawn_top`→每视口一套；`TuiState::new`（`1462-1519`）两宽度初值 `SHARED_RENDER_WIDTH`。
- 让两个 pane 暂时都收全部源行，验证拆分本身无害。

**切片 2：按目标重放 + 边界只算一次**
- `rerender_if_width_changed(conversation_width, trace_width)` + `Targets` 掩码（见 §1）。
- `emit_painted`/`emit_block` 收 `Targets`；`close_unit` 只在 `targets.conversation` 时调
  （见 §2、§5-B）。
- `push_block` 的「记不记 `painted`」改判据：**任一目标产出 > 0 就记**（否则只在轨迹里出行的
  块会在重放里丢）；`apply`/`replay` 的 `produced` 改为两目标取大（给 `replay_batch` 预算，
  `1898-1922`；`replay.lines` 决定是否插接缝，`1930`）。
- 调用点上移到 `draw_frame`（见 §1）。
- 新增测试：两个 pane 各自宽度变化时的重放正确性；不重放没变的 pane（源行不被推两遍）。

**切片 3：每视口点击/滚轮**
- 画轨迹页时填轨迹的 `drawn_*`（`draw_sidebar_page` 的 `Tab::Trace` 分支，`3508-3521`）；
  `mouse` 按 `sidebar_page` 矩形分派 `link_hit`/`wheel`（`2188-2202`）；记录上一帧真的画了
  `sidebar_page` 的矩形（`3358-3361` 的纪律）。
- `focused_turn`/`jump_to_unit`/滚动条/指示器保持绑对话 pane。
- 新增测试：轨迹页点击开详情、轨迹页滚轮只滚轨迹、对话 pane 的 `drawn_rows` 不被覆盖。

**切片 4：过滤接上（测试面最大）**
- 加一个**纯函数**选择器 `selects(view, block)`（live 与 replay 共用，保证重放一致）。
- 思考行只进轨迹：`open_thinking`/`settle_thinking`（`2008-2011`、`2070-2093`）的
  `push_source`/`replace_last`/`links.back_mut()` 改指轨迹。
- 按 §5-C 改测试：详情入口组改从轨迹页点；提示行组改用例外类块或改去轨迹页断言；
  `clicking_a_tab_switches_the_sidebar_page` 重写。
- **颜色分配时机风险**：`SpeakerColors::of` 是在 `paint_block` 里被调的（`4585`、`4890`），
  所以「谁先被画」决定中途出现的名字拿哪个槽位。若轨迹 pane 采用「只在 `tab==Trace` 时
  物化」的省法，槽位顺序会随 tab 可见性变化。**建议在 `apply` 里对每个块先 `colors.of`
  一次**（或让轨迹 pane 常驻），把颜色分配与绘制解耦；`ctrl_o_takes_the_sidebar_away_and_brings_it_back`
  （`:341`）的逐格相等是这条的哨兵。**未证实**：今天没有测试专门盯这个，需新加。

**切片 5（不属本票）**：`layout.rs` 页高与 `fields`（`349-356`、`395-419`）→ 页高票。

## 未证实 / 待定

- **「同步裁剪」的语义**：本调研按「每个 pane 与其自身平行表同步」实现（且推荐如此）；
  「两个不同子集的 pane 用同一份 CAP 计数」需要共享源 id，未设计。归滚动语义票第 4 问。
- **`CAP` 每视图一份还是每源一份**：未定；机制对两种都留了口（pane 返回 dropped）。
- **轨迹 pane 是否常驻**：常驻 = 每块画两遍 + 最多 2×20 000 源行内存；只在 `tab==Trace`
  物化 = 省，但颜色槽位与切页滚动位置的语义要处理。归滚动语义票。
- **`live` 是否进轨迹视图**：本文按冻结项 2 判断「进」，最终以 prototype 票 02 为准。
- **`produced` 的合成口径**（取大 / 求和）：本文取大，未跑过重放性能对照。
- 以上均为静态阅读 + 基线测试实跑，**没有落地任何 `src/` 改动**，所以「哪些测试会红」是
  按冻结项语义做的推演，不是实测红。
