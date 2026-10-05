# 09 — 轨迹视图第一次活起来（tracer bullet）

Type: implement
Status: done
Part of: ../map.md
Blocked by: 07, 08

> 规格：[`../spec.md`](../spec.md) §1（两个视口）、§3（轨迹排版）、§5（滚动与跟随）；帧见 [`../prototype/trace-pages.txt`](../prototype/trace-pages.txt) 与 [`../prototype/split-view.txt`](../prototype/split-view.txt)。
> **这是这一轮的 tracer bullet**：最窄的一条贯穿每一层的路径。做完它，轨迹页第一次是一条真视图，而不是一句占位符。

## 目标

左栏 `轨迹` 页画**全量块**，有自己的滚动、贴底跟随与「N 条新行」指示器；点一行能开详情覆盖层；`--continue` 的重播跟着长出来。**此时对话视图仍画全量**（两个视口画同一份共享源）—— 过滤是下一张票的事，这里只证明「两个视口」这条路径通。

## 现状（改前先复核）

- 轨迹页今天只画一行占位文案；左栏页不持有滚动状态。
- 渲染管线到处写着「一个 pane、一个宽度」：单值的绘制宽度、无参的清空重放、行链接表与回合条与单个 pane 平行、每帧一套的命中索引。
- 块级排版（markdown 表格列宽、代码续行缩进）在进 pane **之前**就按宽度做过，所以共享源只能是那份宽度无关的块列表。
- 滚轮今天已经按指针位置分派；行级右对齐与折行保留对齐都是现成能力。

## 落点

`src/render/tui.rs`（两个 pane 持有者、按目标分派、每视口一套的点击与滚轮取数、指示器）。

## 具体行为

1. **两个 pane**：各持折行缓存、宽度、`top`、跟随与「新行」计数；宽度变化按**目标掩码**分别清空并重放同一份共享源（只清宽度真变了的那个）；流式尾巴由每个 pane 取景时各自折，不新增缓存。
2. **分派点**在块的绘制处：对每个目标各排版一次（发言者配色分配幂等），结果喂给各自的 pane。
3. **轨迹页排版**：消息画**首行 + `…`** 并挂上新的消息详情（正文全文进覆盖层）；前缀分档（宽档 `[名字] `、窄档去方括号）；markdown 照排（把左栏宽度喂给既有的按宽排版入口）。
4. **轨迹页交互**：滚轮按指针位置分派；「N 条新行」指示器可点回到底部并恢复跟随；切页保留滚动位置；键盘三键仍只作用于对话 pane；轨迹页里点一行开详情。
5. **重播**：`--continue` 的全量重播走同一条 push 路径，轨迹页跟着长出来。

## 验证

1. 帧断言：轨迹页在 120×24 里有内容且贴底；指针在左栏时滚轮滚轨迹页、转录不动；指示器在离开底部后出现且可点；切到调用量页再回来位置还在；轨迹页里点一行开得出详情。
2. 消息在轨迹页是首行 + `…`，点开能看全文。
3. 80×24 窄档的前缀不带方括号。
4. 重播一场有历史的会话：轨迹页与对话视图都长出内容。
5. `cargo test` / `cargo clippy` / `cargo fmt --check`。

## 不做什么

- 不做过滤：对话视图这时仍是全量。
- 不做降级退回全量（[11](11-fallback-when-sidebar-hidden.md)）、不做轮次底色（[12](12-trace-round-stripes.md)）、不做详情还原（[13](13-detail-returns-to-opener.md)）。
- 不新增键位。

## 作答（2026-10-05）

- **两个 `Pane` 常驻**：`TuiState` 从 `pane` 拆成 `conversation` 与 `trace`，各自持折行缓存、
  宽度、视口、跟随与「新行」计数；`links` / `drawn_rows` / `drawn_top` 各一套
  （`conversation_links` / `trace_links`、`conversation_drawn` / `trace_drawn`），
  两个指示器槽位也一样。轨迹 pane 只要**左栏在**就物化（宽度 = 左栏页宽 40 / 28，与当前
  显示哪一页无关）—— 这样切页、收起再叫回都不漏内容，发言者配色槽位也不随可见性变
  （`ctrl_o_takes_the_sidebar_away_and_brings_it_back` 是哨兵）。左栏不可见时
  `trace_width = 0`，它不被物化。
- **`Targets` 掩码**：`rerender_if_width_changed(conversation_width, trace_width)` 只清、只
  重放宽度真变了的那个视口（零也走「变了」那一支，好把内容清干净）；重放用的 `Targets`
  是那两位的掩码。`emit_block` 对每个被选中的目标各 `paint_block` 一次，`produced` 取两者
  的大；`push_block` 的「记不记 `painted`」改判据成**任一目标产出 > 0**；
  `turn_rail.close_unit()` 只看 `targets.conversation`。调用点从 `draw_transcript` 上移到
  `draw_frame` 的 `layout::plan` 之后（那时两个宽度都知道）。
- **`push_source` → `push_line(view, ...)`**：按视口选 pane 与平行表，按 `Pane::push` 报回来
  的丢弃数裁；回合条只喂对话视口。`prune_links` 与它那个独立数 `CAP` 的循环删掉（票 07 的
  契约在每一套表上成立）。
- **轨迹页排版**：`paint_block(block, colors, width, view)` 多一个视图参数；轨迹视图里
  `Block::Message` 走新 `trace_message_row` —— 按轨迹宽度照排（markdown 照旧），只取**首行**，
  还有更多行或首行超宽就 `ellipsize_line` 截到一行并加 `…`，整行挂 `DetailKind::Message`
  （全文进覆盖层，`wording::detail_message_section()` = `正文`）。前缀分档由 `PrefixStyle`
  承载：对话视图永远 `[名字] `，轨迹视图宽档（40）带方括号、窄档（28）去掉方括号
  （`layout::SIDEBAR_WIDE` 因此公开，判据只此一处）。
- **轨迹页交互**：`draw_sidebar_page` 在 `Tab::Trace` 分支取景、填轨迹自己的点击映射，并把
  这一帧真画出来的矩形记进 `trace_rect`（每帧重建）；`mouse` 按「指针在不在 `trace_rect` 里」
  分派滚轮与 `link_hit`，两个指示器各自可点回底。键盘三键、回合条、滚动条仍只绑对话视图。
- **思考行**：`open_thinking` / `settle_thinking` 对每个目标各写一遍（思考行的构造依赖视图
  宽度）；重放的 `Painted::Thought` 是**追加**、实时是**就地重写**（`in_place`）—— 这一条
  在拆开两套平行表之后才暴露出来。
- **重播**：`apply` 与重放仍走同一个 `emit_block` / `push_line`，宽度变化按掩码重放的路径与
  `--continue` 的重放是同一条。
- 新增 8 条帧断言：轨迹页贴底、滚轮按指针分派（且转录不动）、指示器出现且可点、切页保留
  位置、轨迹页点一行开详情、消息首行 + `…` 且点开看全文、窄档前缀无方括号、宽度变化把轨迹
  页一起重放。`clicking_a_tab_switches_the_sidebar_page` 随轨迹页不再是占位而改写。
- `cargo test` 全绿（159 + 其余）、`cargo clippy --all-targets` 无警告、`cargo fmt --check` 干净。
