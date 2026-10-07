# 两个 Pane 共享 `painted` 的改造面

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

冻结项 11 定了形态：**两个 `Pane` 共享同一份宽度无关的 `painted` 源**（`Block` 列表，`src/render/tui.rs:1426-1436`），各自折行与滚动，**同步裁剪**。本票把**改造面**钉死成可排期的切片：

1. `render_width: u16` 是单值字段（`src/render/tui.rs:680`）→ 每视口一个宽度；`rerender_if_width_changed`（`1813-1830`）要不要拆成「每个 pane 各自 `clear` + 按自己宽度重放同一份 `painted`」。
2. `push_source`（`1853-1860`）分派两次的形状；`live` 尾巴（流式增量）每帧要不要按两个宽度各折一份。
3. `links`（`734`）与 `turn_rail`（`1336`）今天与主 pane 的源行下标一一对应；两个 pane **同步裁剪**（`prune_links`，`2101-2110`）怎么保证不漂。
4. `drawn_rows` / `drawn_top`（每帧由 `pane.source_at` 重建，`4015-4018`）每视口一套之后，点击与 rail 的取数怎么走。
5. **既有测试**里哪些会红：`tests/render_layout.rs` 与 `tests/render_tui.rs` 里 pane、跳转、tab 命中那几组。

## 已有材料

[两个视口的可行性调研](../research/01-pane-and-two-view-feasibility.md)（charting 期，2026-10-05）已把 `pane.rs` 的结构、折行的两级、宽度变化路径、左栏几何、点击链与 `--continue` 重播路径逐条取证，并给出「共享 `painted`」这条最省的形态。本票在它之上只做一件事：把**改造面与测试影响**钉成切片。

## 产物

一份实现切片建议（动哪些函数、按什么顺序、哪些测试要改），供 `/to-spec` 直接引用。

## 作答

**结论**：能改，`Block` 与事件 schema 不动（分工只是绘制侧的选择）。有一处必须先解决的
结构账：`painted` 共享，但两个视口的**源行序列不同**（冻结项 2 的分工），所以
`links` / `turn_rail` 不能再是单套，也不能靠「两个 CAP 碰巧同速」维持平行。基线：
`cargo test --test render_layout` 147 绿、`--test render_tui` 53 绿（未改任何 `src/`）。

1. **`render_width` 一分为二、`rerender_if_width_changed` 要拆**，且必须带**目标掩码**：
   只 `clear` + 重放**宽度真的变了**的 pane（`emit_painted` 是追加，对没清的 pane 重放会
   推第二遍）。每 pane 存自己的「源行按多宽排的」宽度（对话 = `transcript_text().width`、
   轨迹 = `sidebar_page.width`），初值取 `SHARED_RENDER_WIDTH`；`turn_rail.clear()` 只在
   对话 pane 重放时做。调用点从 `draw_transcript`（`tui.rs:4009`）上移到 `draw_frame` 的
   `plan` 之后（`tui.rs:3372-3383`），因为轨迹宽要到 `plan` 之后才知道。同一帧里
   `pane.view` 与 paint 必须用**同一个** `Rect.width`。
2. **分派点不在 `push_source`，在 `emit_block`**：对每个被选中的目标各 `paint_block` 一次
   （`SpeakerColors::of` 幂等，`tui.rs:243-275`，画两遍不分叉颜色），结果喂给该目标的
   `push_source`（改成 `push_line(view, ...)`，只有对话目标 `turn_rail.push_line`）。
   `live` **不需要**按宽度缓存：`Pane::view` 每帧内部就 `wrap_text(live, width)`
   （`pane.rs:138`），两 pane 各 `view` 一次就各折一份，读同一个 `&state.live`。唯一的
   选择是轨迹视图显不显示流式尾巴（按冻结项 2 应显示）。
3. **把裁剪记账的唯一权威放到 pane 上**：`Pane::push` 返回本次 `evict` 丢的源行数，
   tui 侧按这个数裁平行表；删掉 `prune_links`（`tui.rs:2101-2110`）里独立数 `CAP` 的
   `while`——今天它与 `Pane::evict`（`pane.rs:306-333`）只是**碰巧**同速。平行表**每 pane
   一套**（对话 `links` + `turn_rail`；轨迹自己的 links），`turn_rail.prune` 只用对话 pane 的
   dropped。「同步裁剪」在此读作「每个 pane 与其自身平行表同步」；若要读成「两个 pane 共用
   一个 CAP 计数」，需要一个更粗的共享源 id，未设计（归滚动语义那张票第 4 问）。
4. **`drawn_rows` / `drawn_top` 每视口一套**（唯一消费者是 `link_hit`，`tui.rs:2356-2357`）：
   画对话时填对话那套（`tui.rs:4015-4018`），画轨迹页时填轨迹那套。`mouse` 在落到
   `link_hit` 前按指针是否落在上一帧的 `sidebar_page` 分派（建议像 `detail_rect` 一样记下
   真画出来的矩形）；滚轮同理（冻结项 13）。`focused_turn`（`2337`）、`jump_to_unit`
   （`2349`）、滚动条、指示器、rail 命中区**仍只绑对话 pane**。
5. **测试**：`tests/render_tui.rs` 零命中（从不画帧/不碰 pane 实例，只用纯函数
   `pane::wrap_text`）。`tests/render_layout.rs` 的命中分三层：**(A)** 机器拆分本身应全绿，
   其中 `a_resize_keeps_the_reader_on_the_same_line`（`:1952`）与
   `narrowing_the_terminal_relays_a_table_out_by_the_new_width`（`:5965`）是现成的宽度重放
   回归网；**(B)** rail 组（`:1425/:1453/:1502/:1530/:1561/:1591`）机制正确时应绿，前提是
   `close_unit` 在块被过滤掉时也照调一次；`a_discussion_counts_rounds_where_a_session_counts_turns`
   （`:1617`）与 `clicking_a_tab_switches_the_sidebar_page`（`:1223`，断言轨迹页是占位文案）
   **必红要改**；**(C)** 过滤接上后大面积翻红的主因：提示行组（`Notice` 不属四类例外）里
   `the_transcript_pane_shows_both_the_notices_and_the_streaming_tail`（`:742`）等，以及
   工具行/思考行的详情入口与内容断言（`tool_started(` 命中 23 条、`reasoning_delta(` 命中
   8 条，逐条清单见 findings）。**(D)** `every_size_in_the_matrix...`（`:1039`）与
   `the_sidebar_gives_up...`（`:1126`）因冻结项 14 页高而红，不记在本票账上。

**实现切片顺序**（每片可单独跑测试）：0) `Pane::push` 返回 dropped、`prune_links` 改按它裁
（纯重构，应全绿）；1) 两个 pane 持有者 + 两组宽度（机械，应全绿）；2) `Targets` 掩码重放 +
`close_unit` 只在对话目标时调 + `push_block` 的「记不记 `painted`」改为任一目标产出 >0 就记
（否则只在轨迹出行的块重放会丢）+ `produced` 两目标取大；3) 每视口点击/滚轮；4) 过滤接上
（测试改动最大）+ 颜色分配与绘制解耦（`colors.of` 现在在 `paint_block` 里调，轨迹 pane 若
按 tab 懒物化会让槽位顺序随可见性变，`ctrl_o_takes_the_sidebar_away_and_brings_it_back`
`:341` 是哨兵）；5) 页高（不属本票）。

**未证实**：「同步裁剪」的语义与 CAP 归属归滚动语义票；轨迹 pane 常驻与否；`live` 是否进
轨迹（以 prototype 票 02 为准）；`produced` 合成口径；「哪些测试会红」是按冻结项语义的推演，
本次只实跑了全绿基线，未落地 `src/` 改动。

**context pointer**：逐条证据、代码形状示意与完整测试清单见
[research/02-shared-painted-change-surface.md](../research/02-shared-painted-change-surface.md)。
