# grilling：渲染管线与交付节拍

Type: grilling
Status: resolved
Blocked by: 01, 03

## Question

现有 TUI 循环是为 **inline viewport** 写的：`hide_cursor` → `autoresize` → 在底部保留视口 → 插入 scrollback → `draw_live`，外面包一层 `BeginSynchronizedUpdate` / `EndSynchronizedUpdate`（commits `b64c9e1`、`723bd61`、`333d104`、`e25097e`）。全屏四分区之后这套形状不再适用。本票定下新管线的形状与节拍，并处理"渲染器被 provider 突发饿死"那条既有回归。

## 需要定

1. **渲染任务的形状**。现有循环是 `select!`(广播 / tick / 键盘)。新管线保留它，还是改成"事件只置脏标记 + 单一 draw 点"？键盘事件与绘制的关系（每键一绘？合并？）。
2. **同步更新的包裹点**。全屏 + 全量 buffer diff 之下还要不要 `BeginSynchronizedUpdate`？包在每一帧外面，还是包在一批事件处理外面？（票 01 会给出 ratatui draw 路径是否已自己包裹的事实。）
3. **节流**。一帧的实际重绘代价（票 01 给事实）决定要不要限帧（例如上限 30 或 60 Hz）、以及**没有脏数据时跳过 draw** 的判据。
4. **丢弃回归**。`tests/render_delivery.rs` 测的是 provider 突发把渲染器饿死（修法是 `sse_stream` 每 16 个解码事件 yield 一次）。新管线里这条语义还成立吗？测试要保留、改写还是替换？**写出新的断言**（不能只是删掉）。
5. **增量文本**。流式增量走传输层旁路、不进日志（spec §19）；`MessageCompleted` 到达时要替换已流式的临时块。在全屏面板里这个"临时块 → 正式块"的替换怎么做（占位块？重绘该块？）——现有 `transcript.rs` 怎么处理的，先读再定。
6. **resize**。`Event::Resize` 的处理与重 wrap 的耦合（与票 03 的锚点规则一致）；resize 风暴要不要合并。
7. **退出与崩溃**。`/quit`、Ctrl-C、以及 panic 时如何恢复终端（raw mode / alt screen / bracketed paste / 键盘增强协议是否都要显式撤销）。**管道化与 headless 不受影响**（plain / headless 两个渲染器不动 —— 确认即可）。

## 先读

- `src/render/tui.rs`（整个终端循环）
- `tests/render_delivery.rs`
- `.scratch/tui-layout/issues/01-*.md`、`03-*.md` 的答案

答案必须自足（`/implement` 在 `/clear` 后读它）。

## Answer

**已定（2026-09-21，grilling）。技术决策为主，逐条附既有代码位置。本票只产决策，不含实现。**

### 0. 删什么、留什么

**删**（都是 inline viewport 的产物）：

- 底部锚定 `MoveTo(0, rows - 1)`（`src/render/tui.rs` 的 `run()` 开头那一段）
- `insert_before` 插入循环与 `paint_scrollback`（`src/render/tui.rs` 的 `run()` 内、以及 `:773` 的自由函数）
- `src/render/mod.rs:52` 里 `paint_scrollback` 的**再导出**
- `Viewport::Inline(LIVE_HEIGHT)` 与 `LIVE_HEIGHT` / `LIVE_ROWS` 常量、每次 draw 前的 `hide_cursor()`

**留**：`TICK = 120ms`（`src/render/tui.rs:59`）、`LIVE_BUFFER = 4000`（流式尾缓冲上限）、`EventStream`、`port.recv()`、`select!` 骨架、`render_block`（块 → 行，转录面板继续复用）、`autoresize` 的调用点（改成每轮最多一次）。

### 1. 循环形状：`select!` + 整批 drain + 每轮一次 draw

保留 `tokio::select!`（广播 / 键盘 / console port / tick 四路，与官方示例同形，research §9.3）。改动是**在每轮末尾画之前，把广播里已经排队的消息全部取走**：

```text
select! { ... }
loop { match receiver.try_recv() { Ok(e) => state.apply(e), Err(Empty) => break, Err(Lagged(n)) => diagnostic } }
port 的事件一次性 emit
draw once
```

- 这样一批 provider 突发只产生**一帧**，而不是每个事件一帧。
- `try_recv` 的 drain 加上限（例如单轮最多取 4096 条）以防饿死键盘；到上限就留给下一轮。
- `Lagged` 的既有处理不变（`wording::renderer_dropped` 进转录，票 06）。

### 2. 进 alt screen：用 `ratatui::init()`

`init_with_options` **不**进 alt screen（research §1.2、§12.3），而现在用的正是它。改成 `ratatui::init()`：它做 raw mode + `EnterAlternateScreen`（`CSI ?1049h`）+ panic hook（先 `restore()` 再调原 hook）（research §1.1、§1.5）。`Viewport::Fullscreen` 是 `TerminalOptions` 的默认值，所以不需要再传 options。

### 3. 同步更新只包 `draw`

ratatui 的 draw 路径**不**包裹 synchronized update（research §8、§12.5），所以自己发；但**只包住那一次 `terminal.draw(...)`**：

- 现在它包住「插入 + draw」两个写阶段（`BeginSynchronizedUpdate` … `EndSynchronizedUpdate`，`src/render/tui.rs:207-223`）。插入没了，所以只剩 draw。
- **不要把事件读取包进去**：读键盘不该被关进同步区。

### 4. 脏标记与节流

- **脏的来源**：任何 `state.apply` / 任何键 / `Event::Resize` / 时钟的**分钟**变化 / 模态出现或消失。
- **无脏不画**：tick 每 120ms 醒来只做一件事 —— 比较当前 `HH:MM` 与上一次画的，**变了才置脏**（票 05 定了时钟粒度是分钟）。
- **不额外加帧率上限**：帧成本现在是「一屏 buffer diff + 可见区几条行」（research §2.2），而 `live` 尾缓冲上限 4000 字符（`LIVE_BUFFER`），重排成本可忽略；合并靠 §1 的 drain。**若日后 profile 显示 CPU 偏高**，再加 30 Hz 上限，不预先加。
- tick 仍然保留：它同时是「没有事件时也会醒来一次」的兜底（模态、时钟）。

### 5. 增量文本与「临时块 → 正式块」

- 流式增量走传输层旁路、不进日志（spec §19）；`TuiState` 继续维护 `live` 尾缓冲（上限 4000 字符），转录面板把它当**最后一个临时块**渲染。
- `Block::Message` 到达时**清空 `live`**（现状，`src/render/tui.rs:317-322`）并把该块作为**永久副本**追加；因为票 03 定了「消息正文不再截断」，**正式副本是全文**，尾缓冲只是过程中的临时视图。
- `Block::Delta` 的 `Block` 变体仍然不参与持久转录（`render_block` 对它返回空，现状）。

### 6. resize 与 wrap 缓存

- `Event::Resize` 只置脏；**不**在事件路径里重排。
- **换行缓存按宽度失效**（票 03）：在 `draw` 的闭包里用 `frame.area().width` 与缓存里记的宽度比较，不等就重建 —— 这样 resize、首帧、以及任何宽度变化都走同一条路，不需要单独的 resize 分支。
- 同一批里多次 resize：只有最后一个宽度会被看到（`draw` 用当前 `frame.area()`），天然合并。
- `autoresize()` 每轮最多调一次（现状是每轮一次，可以留着；`draw` 内部也会检测尺寸变化，research §2.3）。

### 7. 丢弃回归（`tests/render_delivery.rs`）

- `sse_stream` 每 16 个解码事件 yield 一次（`YIELD_EVERY`）**保留不动** —— 那是**事件源**侧的公平性修复，新管线不改变它。
- 测试的断言改成两条：①**不丢弃**（既有的 `Lagged` 不得出现）；②**帧数远小于事件数**（给 `Tui` 的 apply/draw 计数，断言 `draws << events`），这条才真正测到「合并突发」这个新行为。
- **不能只保留旧断言**：旧断言在「每事件一帧」的实现下也会过。

### 8. 退出与崩溃

- **正常退出**（`/quit`、空闲 `Ctrl-C`、stdin EOF）：先 `DisableMouseCapture` + `DisableBracketedPaste`（**我们开的，我们自己关**，research §1.3/§1.6），再 `ratatui::restore()`（raw mode + alt screen）。
- **panic**：`ratatui::init()` 装的 panic hook 只 `restore()` 终端面，**不管鼠标与 bracketed paste**（research §1.5）→ 自己再装一层 hook：先 `DisableMouseCapture` / `DisableBracketedPaste`，再调 ratatui 装的那个（或直接 `restore()` 后调原 hook）。
- **不 dump 转录**（票 06）。
- **plain / headless 两个渲染器一行不动** —— 它们不碰终端状态（`Renderer` 三选一，spec §19）。

### 9. `TuiOptions` 与注入

`TuiOptions` 从 `{ port }` 变成 `{ port, facts }`（票 05 的 `SessionFacts`），构造点仍是 `src/cli.rs:299`。**模式不在 facts 里**（票 05）。

### 10. 交接出去的事

- **票 08**：合并突发的帧数断言；`TestBackend` 作为无 pty 的集成后端（票 02 已证可用）。
- **票 09（ADR）**：`EnableMouseCapture` / `EnableBracketedPaste` 的成对撤销与 panic hook 要写进「后果」一节。

**票 01 交接来的事实（2026-09-21）**：

- `Terminal::draw` 是 **buffer diff**，不是整屏重写；只有**尺寸变化**才触发 `ClearType::All` + 后缓冲 reset（research §2.2-2.3）→ 全屏 ≠ 每帧全量写，节流压力比预期小；但 resize 那一帧是全量。
- **ratatui 的 draw 路径不包裹 synchronized update**（全树无匹配，§8、§12.5）→ 现有的 `BeginSynchronizedUpdate` / `EndSynchronizedUpdate` 包裹**必须自建**，而且应当只包「一次 draw」，不要把「读一次事件」也关进去（现在 `src/render/tui.rs` 是包在插入那段外面的）。
- `ratatui::init()` = raw mode + `EnterAlternateScreen`（`CSI ?1049h`）+ panic hook（先 `restore()` 再调原 hook）；**`init_with_options` 不进 alt screen**（§1.1-1.2、§1.5）→ 现在用的正是 `init_with_options(Inline)`，切全屏时**必须显式进 alt screen**，别以为换个 `Viewport` 就够了。
- `TerminalOptions` **只有 `viewport` 一个字段**，没有鼠标捕获开关（§1.3）→ 「不开鼠标捕获」是**默认行为**，不需要也不该加代码。
- `init` **不**开 bracketed paste（§1.6）→ 要自己 `EnableBracketedPaste`，并在 `/quit`、Ctrl-C、panic 三条退出路径上都 `DisableBracketedPaste`（panic hook 只 restore 终端面，不管这个）。
- **没有**官方的「退出 alt screen 后把内容打印出来」的 helper（§10.2；`restore` 只切面，`insert_before` 在 Fullscreen 下无效）→ 票 06 第 7 问若答「要 dump」，机制得自己写。
- 官方示例形状 = `tokio::time::interval` + `EventStream` 放进同一个 `tokio::select!`（§9.3）—— 与现有循环同形，事件源不必重写。
- `Event::Resize` 必有；焦点事件需 `EnableFocusChange`（§9.2）—— 本 effort 不用焦点事件就别开。
