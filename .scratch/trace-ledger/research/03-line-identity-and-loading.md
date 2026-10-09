# 行身份与从磁盘加载：今天代码里的事实

**这是 [`../issues/10-grilling-history-and-selection.md`](../issues/10-grilling-history-and-selection.md)
（长历史与行选择）的事实票产物**，给那张 grilling 票用，不是决策。只读调研，没有改动任何
代码。读的是本仓库当前的 `src/render/`、`src/events.rs`、`src/session/`、`src/cli.rs`、
`docs/observability.md`。

前两张事实票在 [`01-data-surface.md`](01-data-surface.md)（数据面）与
[`02-draw-and-input-surface.md`](02-draw-and-input-surface.md)（绘制与输入面）。本张只补 10 票
点名要的五块：**行是不是一等对象**（`Pane` 的 API 与记账）、**屏幕上点到详情的链路**、
**选中态与焦点通道的先例**、**磁盘那条线**、**回合条与跟随状态机**。凡是那两张已经钉死的结论
本文只复述一行并指向它，不重证。

**行号约定**：每条结论都写成 `路径:行号`。`src/render/tui.rs` 有 10174 行，正文里为省事写成
`tui.rs:NNNN`；别的文件一律写全路径。

---

## 总形状

三句话先摆平：

1. **`Pane` 只有「追加到尾部」与「改最后一条」两种改法**（`src/render/pane.rs:80`、`:90`、
   `:102`），**没有任何按索引插入 / 替换 / 删除的入口**；它的十三个字段全是私有的
   （`src/render/pane.rs:30`–`:55`）。因此 09 票那个「折叠与过滤都是重放时推什么给 pane」的
   结论在代码上是唯一可行的形状，而不是一种偏好。
2. **位置型下标被两件事推翻**：`CAP` 裁剪让所有下标左移（`src/render/pane.rs:381`），
   宽度变化让整批源行重放、视口位置靠 `top_source` 一个整数兜（`src/render/pane.rs:328`）。
   行在屏幕上唯一稳定的地址今天就是**源行下标**，而它恰好是这两件事里最脆的那个。
3. **`Painted` 不受 `CAP` 影响**（`src/render/tui.rs:799`，一个只 `push` 不裁剪的 `Vec`），
   **但从磁盘读回的事件造 `Painted` 的机制今天已经存在且与实时路径逐字同一条** —— 缺的不是
   读盘，是**前置**（`Pane` 只有 `push_back`）。

---

## 1. `src/render/pane.rs` —— 窗格

### 1.1 `CAP` 的定义与生效点

```rust
/// 窗格在丢掉最旧的那些之前保留多少来源行（spec §3）。
pub const CAP: usize = 20_000;                                   // src/render/pane.rs:21
```

模块头已经把两种单位钉死了（`src/render/pane.rs:9`–`:11`）：**上限数的是来源行**，视口、
滚动条与「新内容」指示器数的是**显示行**。同区还有两个步长常量：翻页留下的重叠
`PAGE_OVERLAP = 2`（`src/render/pane.rs:24`）、一格滚轮的 `WHEEL_ROWS = 3`
（`src/render/pane.rs:27`）。

`CAP` 在代码里出现的地方只有三处：`evict` 的循环判据（`src/render/pane.rs:356`）、两条
单元测试（`src/render/pane.rs:497`、`:509`），以及一条端到端测试
`the_transcript_keeps_the_newest_twenty_thousand_source_lines`
（`tests/render_layout.rs:2997`，它断言翻到顶也带不回「第 0 行」）。**平行表那一侧没有一处
自己数 `CAP`** —— 它们一律数 pane 交回来的丢弃数（见 §1.2）。

### 1.2 `evict` 丢了什么：只是显示行，还是连带记账

**两者都丢，而且返回值就是「丢了连带账」的唯一权威。** `push` 的签名直接这么写着：

```rust
/// 追加一条来源行；到了 [`CAP`] 就把最旧的丢掉，并**报出丢了几条**。
///
/// 那个返回数是裁剪的**唯一权威**：平行表（行链接、每源行索引、回合条）全都按它裁，
/// 于是「平行表与窗格的源行窗口同进同出」是契约而不是巧合
pub fn push(&mut self, line: Line<'static>) -> usize {            // src/render/pane.rs:80
    self.lines.push_back(line);
    self.wrap_pending();
    self.evict()
}
```

`evict` 本身（`src/render/pane.rs:354`–`:385`）每丢一条来源行做六件事：`lines.pop_front()`、
`starts.pop_front()`、按那一条占的显示行数 `height` 从 `wrapped` 前端弹出、把剩余每个 `starts`
上移 `height`、把 `top` / `total` / `seen` 各减 `height`、把 `top_source` 减 1
（`src/render/pane.rs:367`–`:382`）。也就是说**窗格内部的一切下标都跟着左移**，而调用方
拿到的那个 `dropped` 就是「你的平行表也该丢这么多条」的凭据。

调用方怎么用（`src/render/tui.rs:2516`–`:2536`）：

```rust
Viewport::Trace => {
    let dropped = self.trace.push(line);
    // 链接表跟着窗格交回来的丢弃数裁，不自己数 `CAP`
    self.trace_links.push_back(link);
    for _ in 0..dropped { self.trace_links.pop_front(); }
    dropped
}
```

回合条同理（`src/render/tui.rs:2529`–`:2536`，`turn_rail.prune(dropped)` 在
`src/render/tui.rs:1735`，它额外把段头下标平移回来并让塌掉的单位退到最老的幸存行）。
「三者永远同长」有测试钉住：`the_link_table_keeps_pace_with_the_pane_at_the_cap`
（`src/render/tui.rs:9433`–`:9450`），断言 `trace_links.len() == trace.sources()` 且
`turn_rail.lines.len() == conversation.sources()`。

**要点的另一面**：`evict` 丢的是**窗格这一层**的东西。窗格上游那份共享源 `painted` 不丢
（`src/render/tui.rs:799`，注释说清代价是「`Tui` 多持一份块」，但没有任何裁剪路径），所以
**块级的东西（时刻、用量、块身份若加上）今天全都在**；丢掉的只是那一行已经排好版的
`Line<'static>` 加上它在两张平行表里的位置。

### 1.3 公开 API 与字段可见性

结构体 `Pane` 的**全部字段私有**（`src/render/pane.rs:30`–`:55`），没有 `pub(crate)` 也没有
`pub`。完整公开面：

| 方法 | 行号 | 形状 |
| --- | --- | --- |
| `Pane::new` / `Default` | `:58` / `:416` | 构造 |
| `push` | `:80` | `(Line<'static>) -> usize`，返回本次丢弃的源行数 |
| `replace_last` | `:90` | 整行重写**最后一条**，空窗格上无操作 |
| `append_to_last` | `:102` | 给**最后一条**行尾追加一个 `Span`（ADR 0016 的用量尾巴） |
| `clear` | `:128` | 清来源行，准备按新宽度重放 |
| `source_at` | `:140` | `(display_row) -> Option<usize>`，显示行 → 源行下标 |
| `view` | `:158` | `(width, height, live: &[Line<'static>]) -> Vec<Line<'static>>` |
| `scroll_to_source` | `:193` | `(source: usize)`，顶端对齐 |
| `scroll` | `:206` | `(rows: isize)`，负数往上 |
| `page` | `:222` | `(up: bool)`，步长 = `height - PAGE_OVERLAP` |
| `wheel` | `:228` | `(up: bool)`，一步 `WHEEL_ROWS` |
| `to_bottom` | `:237` | 回到底部并恢复跟随 |
| `restore` | `:248` | `(top: usize, follow: bool)`，夹回合法范围 |
| `following` | `:260` | `-> bool` |
| `set_holding` | `:269` | `(holding: bool)`，按住「新行」计数 |
| `set_following` | `:281` | `(follow: bool)` |
| `fresh` | `:290` | `-> usize`，离开底部之后到达的显示行数 |
| `total` / `sources` / `top` | `:299` / `:304` / `:309` | 显示行数 / 源行数 / 视口顶端显示行 |
| `wrap_text`（自由函数） | `:423` | `(text, width) -> Vec<Line<'static>>` |

私有辅助：`forget_last_wrap`（`:114`）、`ensure`（`:314`）、`wrap_pending`（`:337`）、
`evict`（`:354`）、`sync_top_source`（`:388`）、`window`（`:396`）。

**按索引插入 / 替换 / 删除：一个都没有。** 逐条对照需求：

- **插入**：只能 `push`（追加到尾部）。没有 `push_front`，没有 `insert(at, line)`。
- **替换**：只有 `replace_last` 与 `append_to_last`，两者都硬编码 `back_mut()`（`:91`、`:103`）。
  没有 `replace(at, line)`。
- **删除**：完全没有。唯一的「少掉一批」是 `evict` 从**前端**丢，且由 `CAP` 自动触发、不可
  由调用方请求。

这条约束直接决定了 10 票要拍的东西：**任何「把一段历史插到窗格前面」或「按行键换掉中间一行」
的形状，都必须先改 `Pane` 的 API**（或者反过来：设计上接受「一切都是重放时推什么」）。

### 1.4 `clear` 保留什么

```rust
pub fn clear(&mut self) {              // src/render/pane.rs:128
    self.lines.clear();
    self.starts.clear();
    self.wrapped.clear();
    self.wrapped_sources = 0;
    self.width = 0;
}
```

**保留**：`height`、`total`、`top`、`top_source`、`follow`、`seen`、`holding` —— 也就是
「视口的意图」全留着（模块注释 `src/render/pane.rs:123`–`:127` 明说：视口的**意图**留着，
重放之后下一帧 `view` 会照着它重新折行）。**清掉**：来源行、折行缓存、以及 `width`
（置零是为了让第一次 `view` 走一次全量重新折行）。

调用方 `rerender_if_width_changed`（`src/render/tui.rs:2441`）除 `pane.clear()` 之外还要清
**窗格外面**的两份平行状态：轨迹侧的 `trace_links` 与 `trace_flow`（`:2456`–`:2460`），
对话侧的 `conversation_flow` 与 `turn_rail`（`:2449`–`:2455`）。而 `painted` 自己
`std::mem::take` 出来重放完再放回（`:2465`–`:2473`）——**它不进 `clear`**。

### 1.5 `top_source` 与 `scroll_to_source`

`top_source` 的语义：**视口顶端落在哪条来源行里**，供下一次重新折行时把视口放回原处
（`src/render/pane.rs:47`–`:48`）。它由 `sync_top_source` 维护（`:388`）：

```rust
fn sync_top_source(&mut self) {
    self.top_source = match self.starts.binary_search(&self.top) {
        Ok(exact) => exact,
        Err(insert) => insert.saturating_sub(1),
    };
}
```

它在 `evict` 里被减 1（`:381`），在 `ensure` 里被读（`:328`）：

```rust
if !self.follow {
    // 每一个显示行都动了，所以行号现在指的是别的东西；一次重新折行之后活下来的
    // 是来源行（spec §4）
    self.top = self.starts.get(self.top_source).copied().unwrap_or(self.top);
}
```

**它是 09 票「宽度重放后选中留在哪」的现成兜底**：重放前后唯一被保住的位置语义就是它，而且
它已经扛过一次 `clear` + 整批重放（注释明说「来源行被整批换掉时（`clear` 之后的重放）它可能
已经不在了，那就留在原地，让 `view` 去夹」）。它是**位置型**的 —— 裁剪会让它漂，加载更早的
历史会整体推它。

`scroll_to_source` 是回合条那格跳转的实现（`src/render/pane.rs:193`–`:203`）：

```rust
pub fn scroll_to_source(&mut self, source: usize) {
    let row = self.starts.get(source).copied().unwrap_or(0);
    self.follow = false;
    let max_top = self.total.saturating_sub(self.height as usize);
    self.top = row.min(max_top);
    if self.top >= max_top { self.follow = true; self.seen = self.total; }
    self.sync_top_source();
}
```

三条性质对 10 票有用：**顶端对齐**（落在那一段的**第一条**源行上，不是一段中间）、**出界夹到
底部**（最后一个整屏之后的源行不需要特例）、**落到底部就自动恢复跟随**。唯一的调用方是
`jump_to_unit`（`src/render/tui.rs:4006`–`:4011`，`self.conversation.scroll_to_source(head)`），
也就是说**它今天只服务对话视图的回合条**，轨迹页一次都没调过。

### 1.6 一条源行怎么记账

窗格这一层只有三份平行结构，全部在 `src/render/pane.rs:30`–`:55`：

- `lines: VecDeque<Line<'static>>` —— 来源行本身，最旧的在前（`:31`–`:32`）。
- `starts: VecDeque<usize>` —— **每条来源行起始的显示行**，与 `lines` 平行（`:33`–`:34`）。
  「第几个源行 → 它从第几个显示行起」就是这一条。
- `wrapped: VecDeque<Line<'static>>` + `wrapped_sources: usize` —— 折行缓存与已折条数
  （`:35`–`:38`）。

反向是 `source_at`（`:140`），用 `starts.binary_search` 找出那个区间（`:144`–`:150`）。

**窗格自己完全不记块身份、不记行链接、不记回合**。那三样都在 `TuiState` 侧、与窗格平行：
`trace_links`（`src/render/tui.rs:920`）、`turn_rail.lines`（`src/render/tui.rs:1680`）、
`conversation_flow` / `trace_flow`（`src/render/tui.rs:6956`）。**「块 → 它画出来的源行」今天
没有任何记账** —— 01 票已钉死（[map 的已定决定](../map.md)第一条），本张再确认一次：连
`emit_block` 返回的那个 `produced = lines.len()`（`src/render/tui.rs:2384`、`:2408`）也只在
当帧用来判「要不要记进 `painted`」（`src/render/tui.rs:2338`–`:2342`），不落任何地方。

---

## 2. `src/render/tui.rs`

### 2.1 屏幕上点一下到详情的那条链路

四个件，各司其职：

| 件 | 定义 | 活多久 | 干什么 |
| --- | --- | --- | --- |
| `trace_links: VecDeque<Option<Detail>>` | `:920` | 与窗格同生共死 | **源行下标 → 那行的详情**；不是入口的行是 `None` |
| `trace_drawn: Drawn` | `:923` | **每帧重建** | **显示行下标 → 源行下标**，外加这个视图第一条行的屏幕 y |
| `trace_rect: Option<Rect>` | `:930` | 每帧重建 | 轨迹页这一帧画在哪，滚轮与点击靠它判分派 |
| `trace_link_at(row)` | `:4060` | 纯函数 | 屏幕行 → `Detail` |

```rust
#[derive(Default)]
struct Drawn {                        // src/render/tui.rs:1665
    /// 那个显示行对应的源行下标；不是任何入口的那些行是 `None`。
    rows: Vec<Option<usize>>,         // :1667
    /// 这个视图第一条被画出来的行的屏幕 y。
    top: u16,                         // :1669
}

fn trace_link_at(&self, row: u16) -> Option<Detail> {   // :4060
    let offset = (row.checked_sub(self.trace_drawn.top)?) as usize;
    let source = (*self.trace_drawn.rows.get(offset)?)?;
    self.trace_links.get(source)?.clone()
}
```

`trace_drawn` 每帧在画之前重建（`src/render/tui.rs:6881`–`:6884`）：

```rust
state.trace_drawn.top = text_area.y;
state.trace_drawn.rows = (0..rows.len())
    .map(|offset| state.trace.source_at(top + offset))
    .collect();
```

`draw_frame` 开头先把 `trace_rect` / `trace_indicator` / `conversation_rect` / `indicator`
清空（`src/render/tui.rs:5523`–`:5529`），「指针只回应上一帧真画出来的东西」这条纪律在这里
落地；同一句纪律的姊妹实现是 `soft_folds`（`src/render/tui.rs:6902`），它用相邻两显示行的
源行下标相等来判「软折续行」——**这是仓库里已有的、由 `source_at` 支撑的第二个行级判据**。

完整链路（`src/render/tui.rs:3282`–`:3300`）：指针 `(column, row)` → `trace_rect.contains` →
`trace_link_at(row)` → `open_detail(detail, width, DetailOpener::Trace { top, follow })` →
`close_detail` 还原（`:8642`–`:8646`）。`DetailOpener` 定义在 `:8502`–`:8518`，四个变体里
**只有 `Trace` 会冻与还原**。

### 2.2 `TuiState::key` 的守卫阶梯

`pub fn key(&mut self, key: Key)` 在 `src/render/tui.rs:4337`–`:4542`。逐层：

| 层 | 判据 | 行号 | 拿走什么 |
| --- | --- | --- | --- |
| L0 | `sync_tokens()` 先跑 | `:4341` | —— |
| L1 | `key == Key::CtrlZ` | `:4345` | 挂起手势 |
| L2 | `key == Key::Esc && self.drag.is_some()` | `:4351` | 取消一次拖选 |
| L3 | `self.replay.is_some()` | `:4358` | **全部按键**交给 `replay_key`（`:2754`），`Ctrl-C` 是退出 |
| L4 | `self.file_viewer.is_some() && self.viewer_key(key)` | `:4365` | 外来屏幕独占 |
| L5 | `self.picker.is_some()` | `:4371` | 模型/档位选择器独占 |
| L6 | `self.detail_open()` | `:4379` | 详情覆盖层独占：`Esc`/`Ctrl-D` 关（`:4383`），`↑`/`↓`/`PgUp`/`PgDn` 滚它（`:4384`–`:4387`），**别的全部忽略** |
| L7 | `self.sidebar_keyboard && self.sidebar_key(key)` | `:4394` | 键盘在左栏时 `↑`/`↓`/`←`/`→`/`Enter`/`Esc`/`r` 归左栏那页（`:3730`、`:3752`） |
| L8 | `key == Key::CtrlT` | `:4405` | 开选择器 |
| L9 | `key == Key::CtrlO` | `:4410` | 左栏开关（并顺手还键盘） |
| L10 | 举手过期 → `Ctrl-C`/`Ctrl-D` → `exit_key` | `:4421`、`:4428` | 双击退出 |
| L11 | `key == Key::Esc` | `:4432` | 问卷退出 → busy 时 `Cancel` / 目标循环问一句 → `pending` 退掉 → 记号菜单 → 多行草稿问一句 → **清草稿** |
| L12 | `self.pending.is_some()` | `:4471` | 问题占键盘（问卷自己分发，`:4478`；别的只吃 `Char`/`Enter`，`:4480`） |
| L13 | `muted && Char/Enter/Tab` | `:4487` | 禁言时编辑与提交进不来 |
| L14 | `key == Key::BackTab` | `:4490` | 模式循环 |
| L15 | `self.token_menu().is_some()` | `:4502` | 记号菜单吃 `↓`/`↑`/`Tab`/`Enter`（`:4504`–`:4525`），其余穿透 |
| L16 | `match key` | `:4529` | `Enter` 提交（`:4530`）；**`PgUp`/`PgDn`/`Ctrl-G` 三个键**（`:4533`–`:4535`）；其余 `editor_key`（`:4537`） |

**「当前显示那一页」那一支拿走了哪些键**：只有那三个 —— `PgUp` / `PgDn` / `Ctrl-G`，走
`page_current`（`:2995`）与 `current_page_to_bottom`（`:3003`），两者都按 `self.main_tab`
分派到 `conversation` 或 `trace` 的 pane。它是 L16 内部的一个分支，**不是独立的一层**。

**轨迹页今天在不在某一层里：不在。** 没有任何一层的判据读 `self.main_tab`。轨迹页今天能拿到的
键盘只有三件：

- **那三个键**，且是**和对话页共用**同一次判定（`main_tab` 一变，键位跟着变）。
- **滚轮**：也不在 `key` 里，在指针路径上 —— `wheel_current`（`src/render/tui.rs:2987`）被
  `src/render/tui.rs:3081` 调到，那是左栏页区判据都落空之后的兜底，同样按 `main_tab` 分派。
- **详情覆盖层**：`Esc` / `Ctrl-D` / `↑` / `↓` / `PgUp` / `PgDn` 在 L6，但它属于**浮层**，
  不属于轨迹页本身。

其余键在轨迹页上落到哪里，逐条（都落在 L16 之下）：

- `↑` / `↓` → `editor.up()` / `editor.down()`（`src/render/tui.rs:2788`–`:2789`），**动的是输入区草稿**。
- `j` / `k` / 任意字母 → `editor.insert_char`（`:2777`），**打进草稿**。
- `Enter` → `submit()`（`:4530` → `:4584`），**提交一轮**。
- `Esc` → L11 那一大串分叉，**最坏的一支是清草稿**（`:4467`）。
- `Space`、`n`、`N`、`r`、`f`、`?`、`{`、`}`、数字键、未绑的 Ctrl 字母、右键与中键：全部落
  空（右键 / 中键与双击：`mouse` 里根本没有识别分支，见 `src/render/tui.rs:3015` 起的分派）。

这条阶梯对 10 票的直接后果：**行选择必须在 L16 之前新插一层**，而插在哪一层是有代价的 ——
插在 L7 之后（`:4396` 与 `:4405` 之间）意味着 `Ctrl-T`/`Ctrl-O` 仍全局生效，与左栏那层的
形状一致；插在 L15 之后（`:4528` 与 `:4529` 之间）意味着 `/` 菜单与 `Space` 的归属要先说清
（09 票把 `/` 排在 L5 记号菜单之前，正是这一段的取舍）。

### 2.3 焦点与键盘归属：现有的概念

**仓库里没有 `Focus` 类型**（全仓 grep 无 `enum Focus` / `struct Focus` / `Focus::`）。
「键盘在谁那里」是用**三样互不相同的机制**表达的，没有统一词汇：

1. **`bool` 位**：`sidebar_keyboard: bool`（`src/render/tui.rs:874`）—— 键盘在不在左栏。
   `open_detail` 之外，它是唯一一处显式的「键盘归属」开关；`Ctrl-O` 收栏时会主动还键盘
   （`:4413`–`:4415` 的 `release_sidebar_keyboard`）。
2. **`Option<usize>` 焦点行**：`FilesPage.focus`（`src/render/tui.rs:6372`）与
   `ChangesPage.focus`（`:6392`）。两个结构体定义在 `:6355`–`:6373` 与 `:6381`–`:6395`，
   注释都写着同一句约定：**`None` 表示还没有一行拿过焦点**，画成常驻选中那一档
   （`ACCENT` + `BOLD`）。`todo` 页**没有焦点行**（`src/render/tui.rs:3308` 注释明说）。
3. **`REVERSED` 的隐式归属**：见下。

**「全屏唯一的反显」这条硬纪律**写在代码注释里：

```rust
if highlighted && options_focused {
    // 反显说的只有一件事：键盘在这里。全屏的 `REVERSED` 永远只允许有一个（§29）。
    style = style.add_modifier(Modifier::REVERSED);   // src/render/tui.rs:5449-5451
}
```

以及 `src/render/tui.rs:5466`–`:5467`：「键盘在输入区时反显的就是这一行：于是「键盘在哪」
永远只有一个答案，`DIM` 退场（§29）」。色板把它写进角色定义：`palette::ACCENT` 的注释是
「焦点：被系统认出来的 / 当前聚焦的 —— 常驻选中（+ `BOLD`）或临时光标（+ `REVERSED`）」
（`src/render/palette.rs:29`–`:31`）。

代码里真正画 `REVERSED` 的地方只有四类：`src/render/tui.rs:5451`（问卷选项）与 `:5471`
（问卷的自由文本行）、`src/render/tui.rs:7453` 与 `:7460`（`/` · `@` 记号菜单的被选行）、
`src/render/selection.rs:205`（拖选，直接改缓冲，注释在 `:178`）、`src/render/viewer.rs:424`
（nvim 回放外部转义）。

**值得 10 票知道的一条缺口**：**详情覆盖层（L6 那一层）今天不画反显** —— 键盘在里面（`↑`/`↓`
滚它），但屏幕上没有一处说「键盘在这里」。所以「全屏唯一的反显」今天有**两个**消费者
（问卷与菜单），而**没有第三个**；轨迹页若要引入行选中 + 光标，得自己决定走哪一档。

### 2.4 三键、滚轮与宽度重放

三键见 §2.2。滚轮在指针路径上：`mouse`（`src/render/tui.rs:3015`）→ 问卷区判定 → 左栏页区
判定（`:3070`–`:3080`，文件页滚自己、改动页滚不动也不穿透）→ **兜底 `wheel_current(up)`**
（`:3081`）。重放进行中整个 `mouse` 早退（`:3018`–`:3019`）。

`rerender_if_width_changed`（`src/render/tui.rs:2441`–`:2475`）是宽度变化时的整批重放路径：

```rust
fn rerender_if_width_changed(&mut self, conversation_width: u16, trace_width: u16) {
    let conversation = conversation_width != self.conversation_width;
    let trace = trace_width != self.trace_width;
    if !conversation && !trace { return; }
    ...
    if conversation { self.conversation.clear(); self.conversation_flow = Flow::default();
                      self.turn_rail.clear(); }
    if trace { self.trace.clear(); self.trace_links.clear(); self.trace_flow = Flow::default(); }
    if self.painted.is_empty() { self.dirty = true; return; }
    let painted = std::mem::take(&mut self.painted);
    let replay = Targets { conversation, trace };
    for item in &painted { self.emit_painted(item, replay); }
    self.painted = painted;
    self.dirty = true;
}
```

三条性质：**只清、只重放宽度真变了的那个视口**（两个宽度同源，通常一起变）；**`painted` 全量
重放**（`emit_painted`，`:2479`），所以它必须**整场会话都在** —— 这正是 `painted` 不裁剪的
另一个理由；**重放期间不 draw**（`dirty` 在末尾置位）。

---

## 3. 选中态的先例

### 3.1 左栏那两页

「左栏那两页」今天是**文件树页**（`Tab::Files`）与**改动页**（`Tab::Changes`），不是「会话页」。
两页的画法逐字相同：

```rust
// files_line，src/render/tui.rs:5866-5872
let style = if focused {
    Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)
} else {
    Style::default().fg(palette::PLAIN)
};
```

```rust
// changes_line，src/render/tui.rs:5976-5983 —— 注释：「常驻选中那一档，与文件页同一个」
let style = if focused {
    Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)
} else {
    Style::default().fg(palette::PLAIN)
};
```

调用点：`files_line` 在 `:5844` 定义、focused 由 `state.files_page.focus == Some(index)` 之类
的判据喂（文件树那处在 `:5825` 之前，行映射记在 `state.files_page.rows`）；`changes_line`
在 `:5975` 定义，调用点 `:5933`–`:5938` 传 `state.changes.focus == Some(index)`。

页签条的选中是**同一个档**（`draw_label_bar`，`src/render/tui.rs:6117`–`:6124`，注释引
`.scratch/tui-visual-language/spec.md` §8）：左栏页签与主列页签条都用它。

配套的键位与移动：`sidebar_key`（`:3720`）分派到 `files_key`（`:3730`，`Esc`/`↑`/`↓`/`←`/`→`/
`Enter`）与 `changes_key`（`:3752`，`Esc`/`↑`/`↓`/`Enter`/`r`），移动是 `files_move_focus`
（`:3774`，两端夹住、`None` 且往下时落到 0），视口跟随是 `files_scroll_to_focus`（`:3790`，
焦点移出窗口就把 `scroll` 跟上去）。**两页都记「全部可见行」而不只是窗口里那几行**
（`src/render/tui.rs:5823` 与 `:5952` 的注释），因为键盘要能在窗口之外走。

### 3.2 命中高亮用底色：**今天没有这个先例**

我按要求找过「命中高亮用底色」的先例，结论是**界面上不存在**。全仓 `.bg(` 只有五处
（grep 结果）：

| 位置 | 是什么 | 属于 |
| --- | --- | --- |
| `src/render/tui.rs:8189` | `palette::BUBBLE` 用户消息气泡底色 | 转录内容域（`bubble()`，`:8168`–`:8192`） |
| `src/render/highlight.rs:202` | `DIFF_ADDED` 新增行底色 | diff 内容域 |
| `src/render/highlight.rs:203` | `DIFF_REMOVED` 删除行底色 | diff 内容域 |
| `src/render/changes.rs:630` | 外部 diff 的 `Sgr::style()` 解析 ANSI 背景 | 外部工具输出 |
| `src/render/viewer.rs:409` | nvim 回放外部转义的 `bgcolor` | 外来屏幕 |

也就是说：**底色今天全部用于「这是什么」的语义标注**（用户说的、补丁里新增的），而
`palette.rs:135`–`:145` 那段注释给出了为什么只能这么用：「新增与删除给的是**背景** —— 这样
它们与语法层的前景是叠加的，而不是互相打架」「两个值取得很暗：一块背景色要读得出「这一行
不一样」，又不能把代码本身压下去」。

**「选中 / 命中」这件事的既有通道只有三个**：`ACCENT + BOLD`（常驻选中，§3.1）、`REVERSED`
（临时光标，且全屏只许一个，§2.3）、以及拖选的缓冲直改（`src/render/selection.rs:205`）。
若 10 票要「区间底色」（06 票那档），那是**第四个通道且界面上没有先例** —— 这条要写进票里。

---

## 4. 磁盘那条线

### 4.1 `log.jsonl` 的路径与形状

**路径**（`src/config.rs:1455`–`:1457` + `src/session/store.rs:8`–`:12`、`:80`–`:83`）：

```
$XDG_DATA_HOME/heng/sessions/<cwd-slug>/<session-id>/log.jsonl
   （$XDG_DATA_HOME 没设时是 $HOME/.local/share/heng/sessions/…）
   同目录下还有 outputs/，存 <tool_call_id>.before 与 .txt
```

`LOG_FILE = "log.jsonl"`（`src/session/store.rs:32`）、`OUTPUTS_DIR = "outputs"`（`:34`）。
权限：会话目录与产物目录 `0700`、事件流 `0600`（模块头 `src/session/store.rs:20`）。

**形状**：一行一个 `Event` 的 JSON。信封在 `src/events.rs:815`–`:820`：

```rust
pub struct Event {
    pub seq: u64,
    pub at: DateTime<Utc>,
    pub speaker_id: SpeakerId,
    pub payload: EventPayload,
}
```

读回来只有两个函数：`events::read_events(path) -> io::Result<Vec<Event>>`
（`src/events.rs:1073`–`:1095`，逐行 `serde_json::from_str`，**空行跳过、最后一行坏掉视为截断**）
与 `Session::events(&self) -> Vec<Event>`（`src/session/mod.rs:141`–`:143`，日志句柄的快照）。

### 4.2 `sessions show` / `sessions replay` 是哪条线

两者都在 **CLI 层**，都住在 `src/cli.rs` 的 `sessions` 子命令族里（分发在 `:3326`），**不进
渲染层**：

- `sessions_show`：`src/cli.rs:3553`，读盘在 `:3561`（`read_events(&session.log_path)`），
  然后按 `--round` / `--speaker` / `--kind` / `--tool` / `--only-error` / `--files` 过滤。
  数据源是 `src/session/observe.rs`（`:78` 也是 `read_events`）。
- `sessions_replay`：`src/cli.rs:3624`，读盘 `:3636`，交给 `crate::agent::replay::replay`
  （`:3651`）**重算某一次调用的 `messages`**。它复现的是「那次请求发出去时」的窗口，切点是
  发言归属在这一轮里的最后一条 `TurnStarted`（`docs/observability.md`「`replay`：重算的到底是
  哪些东西」一节）。
- `sessions_stats`：同一族。

**这两条线都不是 TUI 的历史加载器** —— 它们输出到 stdout，与 `TuiState` 零耦合。

### 4.3 TUI 真的那条读盘路径：`ConsoleRequest::Replay`

真正把磁盘历史铺回界面的是**另一条**，只在 `-c` / `--continue` 重开一场会话时走：

```
read_events(log.jsonl)                       src/events.rs:1073
  → EventLog::open 读到 events 并算 next_seq    src/events.rs:1007-1019
  → Session::events() 快照                    src/session/mod.rs:141
  → harness.events() → console.replay(...)     src/cli.rs:439-441（条件 parsed.resume.is_some()）
  → ConsoleRequest::Replay { events }         src/render/input.rs:259-261
  → TuiState::replay = Some(Replay{events, next: 0, lines: 0})   src/render/tui.rs:4208-4219
  → replay_batch() 每帧一批                    src/render/tui.rs:2704-2728
  → apply(RenderEvent::Logged(event))          src/render/tui.rs:2717
  → transcript.push(event) → Vec<Block>        src/render/transcript.rs:195 / :253
  → trace_block → push_block → Painted::Block  src/render/tui.rs:2279 / :2332 / :2341
  → emit_block → paint_block → push_view_lines → push_line → Pane::push
```

批次由两个上限**都**定界、谁先到算谁：`REPLAY_BATCH_EVENTS = 512`（`src/render/tui.rs:97`）
与 `REPLAY_BATCH_LINES = 2_000`（`:101`），注释在 `:96` 与 `:2700`。重放期间：键盘被 L3 整个
接管（`:4358`）、指针整个早退（`:3018`）、**进流的事件不缓冲**（快照已经含有它们，
`src/render/tui.rs:2683`–`:2687`）、跑完之后插一条接缝 `Notice` 并把两个视口送回末尾
（`finish_replay`，`:2735`–`:2747`）。

### 4.4 `Painted` / `Block` 能不能由磁盘读回的事件构造 —— **能，而且与实时路径逐字同一条**

这是 10 票第 4 条最要紧的一条事实。签名清单：

```rust
pub fn push(&mut self, event: RenderEvent) -> Vec<Block>          // src/render/transcript.rs:195
pub fn push_logged(&mut self, event: Event) -> Vec<Block>         // src/render/transcript.rs:253
pub fn apply(&mut self, event: RenderEvent) -> usize               // src/render/tui.rs:2151
pub fn live_event(&mut self, event: RenderEvent)                   // src/render/tui.rs:2682
pub fn replay(&self, events: Vec<Event>)                           // src/render/input.rs:259
pub fn read_events(path: impl AsRef<Path>) -> io::Result<Vec<Event>>  // src/events.rs:1073
pub fn events(&self) -> Vec<Event>                                 // src/session/mod.rs:141
```

`RenderEvent` 有一个变体就是「一条已落盘的事件」：`RenderEvent::Logged(Event)`
（`src/render/mod.rs:91`）。`apply` 对它与实时事件**没有分叉**：先取时刻
`let at = event_at(&event)`（`src/render/tui.rs:2166`，实现 `:8000`–`:8010`，对 `Logged` 就是
`event.at` —— **落盘那一刻被原样保留**），再 `self.transcript.push(event)`（`:2172`），
后面全是同一条画法。

**内容够不够**：`push_logged` 从磁盘事件造出的块覆盖 `ToolCallStarted` + `ToolCallCompleted`
合成一个 `Block::Tool`（`src/render/transcript.rs:288`–`:301`，`ToolBlock` 带着
`tool_call_id`）、`MessageCompleted` → `Block::Message { text, reasoning }`
（`src/events.rs:426`–`:430`，**正文全文就在事件里**）、`PermissionAsked/Decided`、`Hook`、
`ExecutorSpawned/Finished`、`Usage` 等（`src/render/transcript.rs:26`–`:157` 的整个 `Block`
枚举）。`--continue` 重放今天画得出来，就是这件事的现成证明。

**真正缺的只有一样东西，而且不是读盘**：加载更早的历史要**前置**，而 `Pane` 只有 `push_back`
（§1.3）。`apply` → `push_line` → `Pane::push` 只能往后加。所以 10 票要拍的是：

- 要不要给 `Pane` 开一个前置入口（`push_front` / `insert` / 或者「分两个窗格：一段只读的前缀
  窗格 + 一段可滚的当前窗格」）；
- 或者反过来，把「加载更早」定义成「重建整段 `painted` + 重放」，那样它天然复用
  `rerender_if_width_changed` 的形状（`src/render/tui.rs:2441`）—— 代价是一次全量重排。

### 4.5 块身份能不能从事件里稳定拿到：**原料在手，今天没传下去**

- `Painted::Block { block, at, tail }`（`src/render/tui.rs:1803`–`:1807`）—— **没有 `seq`，
  没有任何身份字段**。
- `apply` 手上有 `Event.seq`（`src/events.rs:816`），但它只取了 `at`（`src/render/tui.rs:2166`），
  `seq` **在进 `transcript.push` 之前就被丢掉了**。
- `Block` 本身：`Block::Tool(Box<ToolBlock>)` 有 `tool_call_id`（`src/render/transcript.rs:159`–`:165`，
  类型是 `ToolCallId`，注释「在它所属的会话里唯一」，`src/events.rs:78`–`:81`），
  **但 `Block::Message`（`:35`–`:43`）、`Block::TurnStarted`、`Block::TurnEnded`、
  `Block::ContextInjected` 都没有任何身份字段**。
- `UsageRecorded` 带 `speaker` 不带 `tool_call_id`（08 票已钉）。

所以准确的答案是：**事件信封的 `seq` 与工具调用的 `tool_call_id` 都在盘上、都稳定，
但要把它们接到 `Painted` 上得改渲染层 —— 一条事件还可能产出零个块或多个块**（`push_logged`
里 `inside_a_call` 那一支返回空数组，`src/render/transcript.rs:275`–`:286`），**「一个 seq ↔
一个块」并不成立**。这是 10 票设计行键时必须先解决的映射问题。

---

## 5. 回合条、`N 条新行` 与跟随状态机

### 5.1 回合条 `TurnRail`

它只与**对话 pane** 平行（`src/render/tui.rs:2529`–`:2536`），**只画在对话页上**
（`draw_turn_rail` 的唯一调用点在 `MainTab::Conversation` 那一支，`src/render/tui.rs:6873`；
`docs/render.md:216` 把这条写成规格）。定义 `:1678`–`:1686`，每条源行一个 `TurnRailLine`
（`:1689`–`:1698`：`unit` / `user` / `blank` 三个 `bool` 与 `usize`）。四个方法：

- `push_line(user_message, blank)`（`:1707`）—— 一行属于正在建的那个单位。
- `close_unit()`（`:1723`）—— **头是单位里第一条用户消息**；没有就退到第一条非空行。
- `prune(dropped)`（`:1735`）—— 跟着 `CAP` 裁，段头下标平移，塌掉的单位退到最老的幸存行。
- `clear()`（`:1748`）—— 宽度重放时跟着对话 pane 一起清。

**亮格是「推出来」的，从不保存**（`focused_turn`，`src/render/tui.rs:3990`–`:4000`）：

```rust
fn focused_turn(&self) -> Option<usize> {
    let units = self.turn_rail.units();
    if units == 0 { return None; }
    if self.conversation.following() { return Some(units - 1); }
    let source = self.conversation.source_at(self.conversation.top())?;
    Some(self.turn_rail.unit_of(source).min(units - 1))
}
```

画法：`draw_turn_rail`（`:6967`–`:7003`），焦点格 `RAIL_FOCUS = '┃'` + `ACCENT + BOLD`
（`:6986`–`:6988`），其余 `RAIL_CELL = '┊'` + 静音，被裁掉的一端 `RAIL_TRUNCATED = '⋮'`
（字形在 `src/render/wording.rs:1913`–`:1915`）。窗口怎么排由 `turn_rail_rows`
（`src/render/tui.rs:8377`–`:8430`）纯算，两条性质写在注释里（`:8366`–`:8376`）：聚焦格永远在
其中、被裁掉的那一端用一个 `⋮` 说出来。每一格记一个 `HitAction::TurnRailUnit(unit)`
命中矩形（`:6996`–`:7001`），点它 → `jump_to_unit`（`:4006`）→ `scroll_to_source`（§1.5）。

### 5.2 「回到最新」指示器与「N 条新行」

它有两个实例，各一个视口一个：`state.indicator`（`src/render/tui.rs:1022`）与
`state.trace_indicator`（`:1024`），都在 `draw_frame` 开头被清空（`:5528`–`:5529`）。
画法在 `draw_indicator`（`:7041`–`:7086`）：

```rust
let frozen = state.detail_open() && view == Viewport::Trace;     // :7049
if pane.following() || frozen || area.width == 0 || area.height == 0 { /* 清空矩形并返回 */ }
let fresh = pane.fresh();                                        // :7057
let text = if fresh == 0 { wording::back_to_bottom().to_owned() }
           else { wording::new_content(fresh) };                  // :7058-7061
```

文案在 `src/render/wording.rs`：`new_content(rows) = "↓ {rows} 行新内容 · 点此到底"`
（`:1634`）、`back_to_bottom() = "点此到底"`（`:1639`）。**注意它数的是显示行不是源行**
（`Pane::fresh` 拿 `total` 算，`src/render/pane.rs:290`–`:296`）。样式 `ACCENT + BOLD`
（`src/render/tui.rs:7076`–`:7078`，注释说「新内容指示器归焦点色」）。整块是点击目标
（`:7040`），点击 → `to_bottom()`（`src/render/tui.rs:3252`–`:3255`，两个视口各一条），
命中判定 `indicator_hit`（`:4067`）。

**轨迹页今天有这一个指示器**（`src/render/tui.rs:6892`，`MainTab::Trace` 那一支调
`draw_indicator(..., Viewport::Trace)`），但**没有回合条**。

### 5.3 滚动与跟随的状态机

全部在 `Pane` 的四个字段上（`src/render/pane.rs:45`–`:54`）：

| 字段 | 含义 |
| --- | --- |
| `top: usize` | 视口顶端那一个**显示行** |
| `top_source: usize` | 视口顶端落在哪条**来源行**里（重新折行的锚） |
| `follow: bool` | 视口跟不跟底部 |
| `seen: usize` | 上一次跟底那帧的 `total`，「N 条新行」从那儿起数 |
| `holding: bool` | 计数被按住（详情覆盖层开着时） |

转移点只有四个，都在 `view` 里（`src/render/pane.rs:158`–`:185`）：

```rust
if self.follow { self.top = max_top; }
else if self.top > max_top { self.top = max_top; }   // 转录在视口下面缩了
if self.top >= max_top { self.follow = true; self.seen = self.total; }
self.sync_top_source();
```

加上 `scroll`（`:206`，**只有往上是「离开底部」那一刻才清 `follow`**，`seen` 留在上一帧放的
地方，所以指示器量的是「那之后到达的」，见 `:209`–`:211` 注释）、`to_bottom`（`:237`）、
`restore(top, follow)`（`:248`）、`set_following`（`:281`）、`set_holding`（`:269`，按住的那
一帧重新取基准 `seen = total`）。

**跟随与详情的耦合**（10 票第 2 条要处理的那类交互已经有一份先例）：打开时
`state.trace.set_following(false); state.trace.set_holding(true)`
（`src/render/tui.rs:9336`–`:9338`），关掉时 `set_holding(false)` + `restore(top, follow)`
（`:8643`–`:8646`），打开方是 `DetailOpener::Trace { top, follow }`（`:8503`–`:8509`，
在点击时由 `:3295`–`:3298` 记下）。

---

## 总结：三到五条一句话的结论

1. **位置型下标被哪两件事推翻**：`CAP` 裁剪让窗格内**一切**下标（含 `trace_links`、回合条、
   `top`、`total`、`seen`、`top_source`）统一左移（`src/render/pane.rs:367`–`:382`，调用方照
   返回的 `dropped` 裁，`src/render/tui.rs:2518`–`:2536`）；宽度变化让整批源行重放、视口只靠
   `top_source` 这一个整数兜回来（`src/render/pane.rs:328`）。行要在屏幕上稳定，必须自带
   行键 —— 而 `Pane` 现有的唯一地址就是那个会被这两件事推翻的下标。

2. **块身份能不能从事件里稳定拿到：原料在手，但没接上，且映射不平凡。** `Event.seq` 与
   `ToolCallId` 都稳定（`src/events.rs:816`、`src/events.rs:78`–`:81`），但 `apply` 只取 `at`
   就把 `seq` 丢了（`src/render/tui.rs:2166`），`Painted::Block`（`:1803`）与除 `Block::Tool`
   之外的 `Block` 都没有身份字段；而一条事件可以产出零个或多个块
   （`src/render/transcript.rs:275`–`:286`），所以「一个 seq ↔ 一个块」今天不成立 —— 这是 10
   票设计行键时要先定的映射。

3. **`painted` 里的块不受 `CAP` 影响。** 它是 `TuiState` 上的 `Vec<Painted>`
   （`src/render/tui.rs:799`），只有 `push`（`:2341`、`:2820`、`:2976`）与宽度重放时的
   `mem::take`/放回（`:2465`–`:2473`），**没有任何裁剪路径**。`CAP` 只裁窗格与两张平行表
   （`trace_links`、`turn_rail`）。所以 06 票「整趟跨度增量累积」那条是成立的，也是 10 票
   「加载更早」最省事的形状（重放一份更长的 `painted`）的立足点。

4. **从磁盘恢复历史时 `Painted` 是怎么造出来的：与实时路径逐字同一条。**
   `read_events`（`src/events.rs:1073`）→ `Session::events()`（`src/session/mod.rs:141`）→
   `ConsoleHandle::replay`（`src/render/input.rs:259`）→ `ConsoleRequest::Replay`
   （`src/render/tui.rs:4208`）→ `replay_batch()` 逐条 `apply(RenderEvent::Logged(event))`
   （`:2717`）→ `transcript.push`（`src/render/transcript.rs:195`）→ `Block` →
   `Painted::Block`（`src/render/tui.rs:2341`），落盘的 `at` 由 `event_at` 原样取回
   （`:8002`）。**「加载更早的历史」缺的只有「前置」** —— `Pane` 只有 `push_back`
   （`src/render/pane.rs:81`），而 `apply` 只能追加。

5. **行选择在键盘阶梯里没有位置，得新插一层；且界面上没有「命中底色」这个通道。**
   `TuiState::key`（`src/render/tui.rs:4337`）的十六层里**没有任何一层读 `main_tab`**
   （§2.2 的表），轨迹页今天只被 L16 内部的三个键（`PgUp`/`PgDn`/`Ctrl-G`）与指针路径上的
   `wheel_current`（`:2987`）认领，`↑`/`↓`/`j`/`k`/`Enter` 全部落进输入区。左栏两页的
   `ACCENT + BOLD` 常驻选中（`:5866`、`:5976`）与页签条（`:6117`）是唯一的先例，而「命中用
   底色」在界面上**零先例** —— 底色今天全是内容语义（`BUBBLE` `tui.rs:8189`、
   `DIFF_ADDED`/`DIFF_REMOVED` `highlight.rs:202`–`:203`）。

## 查不到 / 未证实的部分

- **「加载更早的历史」有没有既有设计残迹**：`.scratch/` 下与 `log.jsonl` 按需加载相关的只有
  map 的「尚未明确」第三条（它把这件事整个交给 10 票），代码里没有任何 `Pane` 前置、
  `painted` 分段或读取窗口的实现痕迹。**未证实**：一次会话的事件流在典型使用下能有多长、
  `log.jsonl` 的行均值 —— 这决定「加载更早」是读 50 个块还是读整个文件，只能实测。
- **`Pane` 的字段全是私有的这一事实，在 `src/render/` 之外是否还有别的消费者**：我只查了
  `src/render/`、`tests/` 里的 `pane::` 引用（`pane::CAP` 出现在
  `tests/render_layout.rs:2997` 与 `src/render/tui.rs:9433`–`:9450`），没有穷举 `tests/` 目录
  全部引用。
- **`docs/observability.md` 里没有 `log.jsonl` 的路径**：那份文档只讲四个动词的语义（88 行），
  路径与目录树在 `src/session/store.rs:8`–`:12` 与 `src/config.rs:1455`。10 票若要引用路径，
  引用处应该是代码与 `docs/lifecycle.md:385`。
- **双击 / 右键 / 中键**：`mouse`（`src/render/tui.rs:3015` 起）的分派里没有这些分支，
  「今天没有识别机制」这条与 02 票一致，本张未再深挖 `Drag` 的门槛常量。
- **多窗口（两个终端同时开同一场会话）时的 `painted` 冲突**：不在 10 票范围内，未查。
