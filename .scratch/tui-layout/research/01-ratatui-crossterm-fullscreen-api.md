# fs-agent：ratatui 0.30 / crossterm 0.29 全屏 TUI 的一手 API 事实

> 目的：为**票 02（布局原型）**、**票 03（对话面板滚动）**、**票 04（多行输入）**、**票 07（渲染管线）**提供外部事实输入。本文件只报告**事实与来源**，不推荐方案、不选赢家、不下结论。
>
> 抓取日期：**2026-09-21（UTC）**。全部为**本机已 vendored 的 crates.io registry 源码**（离线、精确，就是 fs-agent `Cargo.toml` 解析到的那一份），另有一处官方仓库网页抓取（已单独标注 URL 与日期）。未调用任何 LLM API，未做编译实验，未创建探针 crate。
>
> 来源分级：**✅ 一手来源** = 本机 vendored 源码（含 crate 自带 README/docs/examples）、官方仓库源码、官方文档站。凡一手来源未写明者一律标 **⚪ 未证实**，**不做推断**。
>
> 路径基址：`$REG` = `/home/forty/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`。下文所有 `$REG/...` 行号都对应当前 vendored 文件。源码目录**只读**，本轮未修改任何依赖文件。

---

## 0. 来源清单

| ID | 路径 / URL | 版本 | 抓取日期 |
| --- | --- | --- | --- |
| RT-INIT | `$REG/ratatui-0.30.2/src/init.rs` | ratatui 0.30.2 | 2026-09-21 |
| RT-CARGO | `$REG/ratatui-0.30.2/Cargo.toml` | ratatui 0.30.2 | 2026-09-21 |
| RT-WIDGETS-REEXPORT | `$REG/ratatui-0.30.2/src/widgets.rs` | ratatui 0.30.2 | 2026-09-21 |
| RT-TERM | `$REG/ratatui-core-0.1.2/src/terminal.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-TERM-INIT | `$REG/ratatui-core-0.1.2/src/terminal/init.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-VIEW | `$REG/ratatui-core-0.1.2/src/terminal/viewport.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-RENDER | `$REG/ratatui-core-0.1.2/src/terminal/render.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-BUFFERS | `$REG/ratatui-core-0.1.2/src/terminal/buffers.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-RESIZE | `$REG/ratatui-core-0.1.2/src/terminal/resize.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-INLINE | `$REG/ratatui-core-0.1.2/src/terminal/inline.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-FRAME | `$REG/ratatui-core-0.1.2/src/terminal/frame.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-CURSOR | `$REG/ratatui-core-0.1.2/src/terminal/cursor.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-BACKEND | `$REG/ratatui-core-0.1.2/src/backend.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-BUFFER | `$REG/ratatui-core-0.1.2/src/buffer/buffer.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-DIFF | `$REG/ratatui-core-0.1.2/src/buffer/diff.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-TEXT | `$REG/ratatui-core-0.1.2/src/text/text.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-LINE | `$REG/ratatui-core-0.1.2/src/text/line.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-SPAN | `$REG/ratatui-core-0.1.2/src/text/span.rs` | ratatui-core 0.1.2 | 2026-09-21 |
| RT-CROSSTERM | `$REG/ratatui-crossterm-0.1.2/src/lib.rs` | ratatui-crossterm 0.1.2 | 2026-09-21 |
| RT-PARA | `$REG/ratatui-widgets-0.3.2/src/paragraph.rs` | ratatui-widgets 0.3.2 | 2026-09-21 |
| RT-SCROLLBAR | `$REG/ratatui-widgets-0.3.2/src/scrollbar.rs` | ratatui-widgets 0.3.2 | 2026-09-21 |
| RT-BLOCK | `$REG/ratatui-widgets-0.3.2/src/block.rs` | ratatui-widgets 0.3.2 | 2026-09-21 |
| RT-REFLOW | `$REG/ratatui-widgets-0.3.2/src/reflow.rs` | ratatui-widgets 0.3.2 | 2026-09-21 |
| RT-WIDGETS-LIB | `$REG/ratatui-widgets-0.3.2/src/lib.rs` | ratatui-widgets 0.3.2 | 2026-09-21 |
| RT-WIDGETS-CARGO | `$REG/ratatui-widgets-0.3.2/Cargo.toml` | ratatui-widgets 0.3.2 | 2026-09-21 |
| INSTAB | `$REG/instability-0.3.13/src/{lib.rs,unstable.rs}` | instability 0.3.13 | 2026-09-21 |
| CT-EVENT | `$REG/crossterm-0.29.0/src/event.rs` | crossterm 0.29.0 | 2026-09-21 |
| CT-PARSE | `$REG/crossterm-0.29.0/src/event/sys/unix/parse.rs` | crossterm 0.29.0 | 2026-09-21 |
| CT-STREAM | `$REG/crossterm-0.29.0/src/event/stream.rs` | crossterm 0.29.0 | 2026-09-21 |
| CT-TERM | `$REG/crossterm-0.29.0/src/terminal.rs` | crossterm 0.29.0 | 2026-09-21 |
| CT-CMD | `$REG/crossterm-0.29.0/src/command.rs` | crossterm 0.29.0 | 2026-09-21 |
| CT-SYS-UNIX | `$REG/crossterm-0.29.0/src/terminal/sys/unix.rs` | crossterm 0.29.0 | 2026-09-21 |
| CT-CARGO | `$REG/crossterm-0.29.0/Cargo.toml` | crossterm 0.29.0 | 2026-09-21 |
| CT-README | `$REG/crossterm-0.29.0/README.md` | crossterm 0.29.0 | 2026-09-21 |
| CT-EX-TOKIO | `$REG/crossterm-0.29.0/examples/event-stream-tokio.rs` | crossterm 0.29.0 | 2026-09-21 |
| FS-CARGO | `/home/forty/code/fortystory/fs-agent/Cargo.toml` | 本仓库 | 2026-09-21 |
| FS-TUI | `/home/forty/code/fortystory/fs-agent/src/render/tui.rs` | 本仓库 | 2026-09-21 |
| FS-MOD | `/home/forty/code/fortystory/fs-agent/src/render/mod.rs` | 本仓库 | 2026-09-21 |
| WEB-ALT | <https://raw.githubusercontent.com/ratatui/ratatui-website/main/src/content/docs/concepts/backends/alternate-screen.md> | 官方站点 main 分支 | 2026-09-21 |
| WEB-EX-ASYNC | <https://raw.githubusercontent.com/ratatui/ratatui/main/examples/apps/async-github/src/main.rs> | 官方仓库 main 分支 | 2026-09-21 |

`fs-agent` 当前依赖（FS-CARGO:33-34）：`ratatui = "0.30"`（默认特性）、`crossterm = { version = "0.29", features = ["event-stream"] }`。`ratatui` 默认特性为 `["all-widgets","crossterm","layout-cache","macros","underline-color"]`（RT-CARGO:74-80），**不含任何 `unstable-*`**。`crossterm` 默认特性为 `["bracketed-paste","events","windows","derive-more"]`，`event-stream` 额外引入 `futures-core` 并依赖 `events`（CT-CARGO:48-65）。

---

## 1. 全屏模式的初始化与拆卸

### 1.1 `init` / `try_init` 的精确签名与副作用

| 事实 | 来源 |
| --- | --- |
| `pub fn init() -> DefaultTerminal`；实现为 `try_init().expect("failed to initialize terminal")` | RT-INIT:365-367 |
| `pub fn try_init() -> io::Result<DefaultTerminal>`；实现依次为 `set_panic_hook(); enable_raw_mode()?; execute!(stdout(), EnterAlternateScreen)?; let backend = CrosstermBackend::new(stdout()); Terminal::new(backend)` | RT-INIT:397-403 |
| `pub type DefaultTerminal = Terminal<CrosstermBackend<Stdout>>` | RT-INIT:213 |
| `init` 的文档列出副作用：后端 `CrosstermBackend` 写 `Stdout`、开 raw mode、进 alternate screen、安装 panic hook | RT-INIT:332-337 |
| `init` 在 raw mode / 进 alt screen / 计算终端尺寸三者任一失败时 panic | RT-INIT:352-358 |

→ **约束：票 07（渲染管线）**——`ratatui::init()` 一步就同时给出 Fullscreen viewport + raw mode + alt screen + panic hook，管线接入点就是这个返回值。

### 1.2 `init_with_options` / `try_init_with_options` 的精确签名与副作用

| 事实 | 来源 |
| --- | --- |
| `pub fn init_with_options(options: TerminalOptions) -> DefaultTerminal`；`try_init_with_options(options) -> io::Result<DefaultTerminal>` | RT-INIT:449-451, 492-497 |
| 实现依次为 `set_panic_hook(); enable_raw_mode()?; let backend = CrosstermBackend::new(stdout()); Terminal::with_options(backend, options)`——**不执行 `EnterAlternateScreen`** | RT-INIT:492-497 |
| 文档逐字：`Unlike init, this function does not enter the alternate screen buffer as this may not be desired in all cases. If you need the alternate screen buffer, you should enable it manually after calling this function.` | RT-INIT:416-418, 464-466 |

→ **约束：票 07 / 票 02**——`Viewport::Fullscreen` 是 `TerminalOptions` 的默认值（见 1.3），但走 `init_with_options` 只得到 Fullscreen 的几何、**没有 alt screen**；要 alt screen 必须用 `init()` 或自己 `execute!(EnterAlternateScreen)`。

### 1.3 `TerminalOptions` 字段与默认值

| 事实 | 来源 |
| --- | --- |
| `pub struct TerminalOptions { pub viewport: Viewport }`——**只有 `viewport` 一个字段**；`#[derive(Debug, Default, Clone, Eq, PartialEq, Hash)]` | RT-TERM:464-471 |
| 源码中**不存在** `mouse_capture` / `is_raw` 字段（全文件仅 487 行，`TerminalOptions` 定义处仅此一字段） | RT-TERM:460-471 |
| `pub enum Viewport { #[default] Fullscreen, Inline(u16), Fixed(Rect) }`——`Fullscreen` 带 `#[default]`，即 `Viewport::default() == Fullscreen` | RT-VIEW:61-119（`#[default]` 在 77-78 行） |
| `Terminal::new(backend)` 等价于 `with_options(backend, TerminalOptions { viewport: Viewport::Fullscreen })` | RT-TERM-INIT:56-63 |
| Fullscreen 的 `Frame::area()` 始终从 `(0, 0)` 开始 | RT-VIEW:72 |

→ **约束：票 02（布局原型）**——fs-agent 不需要构造 `TerminalOptions` 就能得到全屏；`Viewport::Fullscreen` 是缺省，且没有鼠标/ raw 的开关可配。

### 1.4 alt screen 的进入/退出由谁发、发什么序列

| 事实 | 来源 |
| --- | --- |
| `ratatui::init` 通过 `crossterm::terminal::EnterAlternateScreen` 进入（`execute!(stdout(), EnterAlternateScreen)`） | RT-INIT:204-206, 400 |
| `ratatui::restore` / `try_restore` 通过 `LeaveAlternateScreen` 退出 | RT-INIT:554-560 |
| `EnterAlternateScreen::write_ansi` 写 `CSI ?1049h`（`csi!("?1049h")`） | CT-TERM:220-223 |
| `LeaveAlternateScreen::write_ansi` 写 `CSI ?1049l` | CT-TERM:258-261 |
| `try_restore` 先 `disable_raw_mode()?` 再 `execute!(stdout(), LeaveAlternateScreen)?`；源码注释说明「先关 raw mode 很重要，因为它副作用更多」 | RT-INIT:554-559 |
| `restore()` 版本：`try_restore` 出错时向 stderr 打印 `Failed to restore terminal: {err}`，不 panic | RT-INIT:524-529 |
| `ratatui-crossterm` 的 `CrosstermBackend` **自己不动** raw mode / alt screen，文档逐字：`This is not done automatically by the backend because it is possible that the application may want to use the terminal for other purposes (like showing help text) before entering alternate screen mode.` | RT-CROSSTERM:113-118 |

→ **约束：票 07**——alt screen 的进/出完全由 `ratatui::init` / `ratatui::restore`（或调用方手工 execute）负责；`Terminal`/`CrosstermBackend` 不会替你切面。

### 1.5 panic hook 行为

| 事实 | 来源 |
| --- | --- |
| `set_panic_hook()`：`let hook = std::panic::take_hook(); std::panic::set_hook(Box::new(move |info| { restore(); hook(info); }))`——**先 restore 再调用原 hook** | RT-INIT:566-572 |
| `try_init` 与 `try_init_with_options` 都会 `set_panic_hook()` | RT-INIT:398, 493 |
| 文档要求：必须在应用安装其它 panic hook **之后**再调用这些 init 函数，否则终端不会被优先恢复 | RT-INIT:192-197, 335-337 |
| `Terminal::new`/`with_options` **不**安装 panic hook；其文档提醒未安装时 panic 信息会打在 alt screen 上、终端可能不可用 | RT-TERM-INIT:17-19 |

→ **约束：票 07**——如果渲染管线自己手搓 `Terminal::new`，panic 恢复责任转移给调用方；用 `init()` 则自动接管。

### 1.6 bracketed paste 有没有被 `init` 顺带打开

**答案：没有。** 证据：
- `ratatui-0.30.2/src/init.rs` 对 crossterm 的导入只有 `EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode`（RT-INIT:204-206），全文不含 `EnableBracketedPaste` / `BracketedPaste` 字样。
- `try_init` / `try_init_with_options` 的执行体只有 raw mode +（init 时）alt screen（RT-INIT:397-403, 492-497）。

→ **约束：票 04（多行输入）**——粘贴保护必须由 fs-agent 自己 `execute!(EnableBracketedPaste)` / `DisableBracketedPaste`（crossterm 默认特性已含 `bracketed-paste`，见 6.1）。

### 1.7 raw mode 的副作用（crossterm 原文档）

`crossterm` 文档逐字列出开 raw mode 后：输入不再被转发到屏幕、回车不再被处理、行缓冲关闭（逐字节进入输入缓冲）、backspace/Ctrl+C 等不再由终端驱动处理、换行不被处理因此不能用 `println!`（CT-TERM:53-59）。

→ **约束：票 04**——raw mode 下 `\n` 不再等价于提交，这与 7.3 的 Ctrl+J 语义直接相关。

---

## 2. 全屏下 `Terminal::draw` 的重绘语义

### 2.1 draw 的完整步骤

| 事实 | 来源 |
| --- | --- |
| `pub fn draw<F>(&mut self, render_callback: F) -> Result<CompletedFrame<'_>, B::Error> where F: FnOnce(&mut Frame)` | RT-RENDER:81-89 |
| `draw` 只是把回调包成 `try_draw` | RT-RENDER:85-88 |
| `try_draw`：`self.autoresize()?` → `let mut frame = self.get_frame()` → `render_callback(&mut frame)` → `self.apply_buffer_with_cursor(frame.cursor_position)` | RT-RENDER:189-205 |
| `apply_buffer_with_cursor`：`flush()` → 按 cursor 需求 `hide_cursor()` 或 `show_cursor()+set_cursor_position()` → `swap_buffers()` → `backend.flush()` → 返回 `CompletedFrame` | RT-RENDER:288-320 |
| 回调返回错误时：后端、缓冲、光标状态、frame_count 都不变 | RT-RENDER:124-130（并有测试 RT-RENDER:516-555） |

→ **约束：票 07（渲染管线）**——一次 `draw` 的写入发生在这 5 个内部步骤里，任何「同步包裹」只能包住整个 `draw` 或 `backend.flush` 之后的窗口。

### 2.2 是 buffer diff 还是整屏重写

| 事实 | 来源 |
| --- | --- |
| `Terminal::flush` 取 `previous = buffers[1 - current]`、`current = buffers[current]`，用 `previous.diff_iter(current)`，只把**变化的 cell** 交给 `Backend::draw` | RT-BUFFERS:97-114 |
| `Buffer::diff_iter` 返回 `BufferDiff`，文档：`Builds a minimal sequence of coordinates and Cells necessary to update the UI from self to other.` | RT-BUFFER:475-508 |
| `BufferDiff` 只 yield `next` 中与 `prev` **不同**的 cell；相等 cell 直接跳过 | RT-DIFF:89-199（相等跳过在 150-154） |
| `Terminal` 文档：`Terminal::flush` diffs the current buffer against the previous buffer and sends only the changed cells to the backend.` | RT-TERM:340-349 |
| `swap_buffers` 会 `reset()` 掉「下一个」缓冲并翻转 `current` | RT-BUFFERS:121-124 |
| `with_options` 初始化两个**空**缓冲：`buffers: [Buffer::empty(viewport_area), Buffer::empty(viewport_area)]` | RT-TERM-INIT:134-144（136 行） |
| 文档明确「每个渲染 pass 都从空缓冲开始，回调必须完整重画整个 viewport」 | RT-TERM:346-349（`Paragraph`/回调不完整会留下脏 buffer，见 RT-RENDER:43-47） |

**结论（源码事实）：是 buffer diff，不是整屏重写。** 第一次 `draw` 的前一缓冲是 `Buffer::empty`，所以首帧只写出与「默认空白 cell」不同的 cell（纯空格且默认样式的位置不会被写）；此后每帧与上一帧 diff。

→ **约束：票 07**——高频 tick 下每帧写入量正比于实际变化量；只要回调「完整重画」语义被遵守，同步包裹点应覆盖 `draw` 内部（尤其 `flush`+`backend.flush`）。

### 2.3 `resize` / `clear` 在什么条件下发生

| 事实 | 来源 |
| --- | --- |
| `autoresize()` 只对 `Fullscreen | Inline` 生效；每帧比较 `self.size()` 与 `last_known_area`，不同才 `resize(area)` | RT-RESIZE:64-73 |
| `Fixed` 不被 autoresize | RT-RESIZE:65-66, 20-22 |
| `resize()` 对 Fullscreen：`next_area = area`；若**宽度缩小**则先把 `next_area.y = 0` 并 `clear_region(ClearType::All)`（避免折行残留）；随后 `set_viewport_area(next_area)` + `clear_viewport()` | RT-RESIZE:23-55（宽度缩小分支 41-45） |
| Fullscreen 的 `clear_viewport()` = `backend.clear_region(ClearType::All)`，并 `reset()` 后缓冲以强制下次全量重画 | RT-BUFFERS:158-174（Fullscreen 分支 160，reset 172） |
| `Terminal::clear()`：保存当前光标位置 → `clear_viewport()` → 恢复光标；文档「force a full redraw on the next draw call」 | RT-BUFFERS:126-152 |
| Fullscreen 的 `clear` 清**整个**终端；Inline 只从 viewport 原点往后清；Fixed 只清 viewport 区域 | RT-BUFFERS:126-137, 158-174 |

→ **约束：票 07 / 票 02**——任何终端尺寸变化都会触发**整屏 clear + 全量重画**；此外 `ClearType` 只作用于可见显示面，`No guarantees are made about scrollback, history, or off-screen buffers.`（RT-BACKEND:115-122, 276-279）。

### 2.4 一次 draw 的写入量级（源码可判定部分）

- 稳态（无 resize、无 clear）：写入 cell 数 = `diff(上一帧, 本帧)` 的变化 cell 数（RT-BUFFERS:102-107）。
- 首帧或 resize/clear 之后：后缓冲被 `reset()`（RT-BUFFERS:172）或首帧前缓冲为空（RT-TERM-INIT:136），因此写入量 = 本帧所有「非默认空白」cell 数。
- 每次写入的字节内容由 `CrosstermBackend::draw` 决定：按 cell 逐个 `MoveTo`（仅当不是上一格右侧时才发）+ 颜色/属性 diff + `Print(cell.symbol())`，最后统一 reset 前景/背景/属性（RT-CROSSTERM:232-292）。

→ **约束：票 07**——写入量级按「变化 cell 数」而非「屏幕大小」计；但每次变宽/变色都会额外发 `MoveTo`/`SetColors`，节拍设计要考虑这一点。

---

## 3. `Viewport::Inline` 与 `Terminal::insert_before` 是否仍在

| 事实 | 来源 |
| --- | --- |
| `Viewport::Inline(u16)` **仍在**，高度以行为单位、宽度固定为全终端宽 | RT-VIEW:79-99（「Inline viewports always span the full terminal width.」91 行） |
| `pub fn insert_before<F>(&mut self, height: u16, draw_fn: F) -> Result<(), B::Error> where F: FnOnce(&mut Buffer)` **仍在** | RT-INLINE:109-120 |
| 非 inline viewport 下是 **no-op**：`insert_before` 的 match 分支 `_ => Ok(())` | RT-INLINE:113-119（118 行） |
| 文档逐字：`Insert some content before the current inline viewport. This has no effect when the viewport is not inline.` | RT-INLINE:7-8 |
| `insert_before` 的两条实现路径由 `scrolling-regions` 特性切换：有该特性用滚动区域，否则回落为清屏重画；fs-agent **未启用** `scrolling-regions` | RT-INLINE:113-117, RT-RENDER 对应文档 17-20 |
| `Terminal::flush` **不感知显示面切换**：`If you leave the alternate screen and then call Terminal::flush, Ratatui may replay a diff that was computed for the alternate screen onto the main screen.` | RT-BUFFERS:84-90 |

**fs-agent 侧的名称与死码候选：**

| 事实 | 来源 |
| --- | --- |
| `paint_scrollback` 定义在 `src/render/tui.rs:773`：`pub fn paint_scrollback(lines: &[Line<'_>], buf: &mut Buffer)` | FS-TUI:761-773 |
| 它当前被 `terminal.insert_before(height, |buf| paint_scrollback(&lines, buf))` 调用 | FS-TUI:220 |
| 它被 `src/render/mod.rs:52` 再导出：`pub use tui::{paint_scrollback, render_block, Key, Tui, TuiOptions, TuiState};` | FS-MOD:52 |
| `src/render/tui.rs` 的模块注释写明 inline 依赖 `Terminal::insert_before` | FS-TUI:8 |

→ **约束：票 02 / 票 07**——要删的东西名字是 `Viewport::Inline(u16)` 与 `Terminal::insert_before(height, draw_fn)`（后者在纯 Fullscreen 下本身就是 no-op）；删除 inline 路径后，fs-agent 自己的 `paint_scrollback`（FS-TUI:773，FS-MOD:52 再导出）会失去唯一调用点（**需编译验证**是否有其它调用点/测试引用——本轮只做了 grep）。

---

## 4. 可滚动长文本：`Paragraph` / 换行行数 / `Scrollbar`

### 4.1 `Paragraph` 当前签名

| API | 签名 | 来源 |
| --- | --- | --- |
| `new` | `pub fn new<T>(text: T) -> Self where T: Into<Text<'a>>` | RT-PARA:149-163 |
| `block` | `pub fn block(mut self, block: Block<'a>) -> Self` | RT-PARA:175-178 |
| `style` | `pub fn style<S: Into<Style>>(mut self, style: S) -> Self` | RT-PARA:199-202 |
| `wrap` | `pub const fn wrap(mut self, wrap: Wrap) -> Self` | RT-PARA:216-219 |
| `scroll` | `pub const fn scroll(mut self, offset: (Vertical, Horizontal)) -> Self`；`Vertical = u16`、`Horizontal = u16`；**元组顺序是 (y, x)**，注释明确「different from general convention across the crate」 | RT-PARA:127-128, 221-239 |
| `alignment` | `pub const fn alignment(mut self, alignment: Alignment) -> Self`，另有 `left_aligned/centered/right_aligned` | RT-PARA:255-306 |
| `Wrap` | `pub struct Wrap { pub trim: bool }` | RT-PARA:121-125 |

→ **约束：票 03（对话面板滚动）**——`Paragraph::scroll` 的 y 单位是「**换行后的行**」（文档：offset applied after the text is wrapped and aligned，RT-PARA:223-225）。

### 4.2 有没有 `line_count` / `height_for` 一类一手 API

**有 `line_count`，没有 `height_for`；但 `line_count` 在 fs-agent 当前构建里不可调用（被 unstable 特性挡成 `pub(crate)`）。**

| 事实 | 来源 |
| --- | --- |
| `pub fn line_count(&self, width: u16) -> usize` | RT-PARA:332-365 |
| 语义：`wrap` 为 `Some` 时用 `WordWrapper` 逐行数**换行后**的行数；`wrap` 为 `None` 时返回 `self.text.height()`（即 `Line` 条数，不换行）；最后加上 `Block` 的 `vertical_space()` 上下占用（含边框/内边距/标题） | RT-PARA:332-365（block 部分 337-341, 362-364） |
| `width < 1` 时返回 `0` | RT-PARA:333-335 |
| 它带 `#[instability::unstable(feature = "rendered-line-info", issue = "https://github.com/ratatui/ratatui/issues/293")]` | RT-PARA:328-331 |
| `line_width(&self) -> usize` 同样存在、同样带该 `unstable` 属性，语义是最宽的 unwrapped 行宽 + block 水平占用 | RT-PARA:384-399 |
| **没有** `height_for` / `rendered_height` 之类 API（`ratatui-widgets` 全 crate 内 `line_count`/`height_for` 的匹配只出现在 `paragraph.rs`） | RT-PARA 全文 + 全 crate grep |

`instability::unstable` 的真实语义（源码）：

| 事实 | 来源 |
| --- | --- |
| 属性把 `pub` 项改成 `pub(crate)`，除非对应 crate 特性被启用；被 gate 的特性名是 `unstable-` + `feature`，即 `feature = "rendered-line-info"` → 门是 `unstable-rendered-line-info` | INSTAB/lib.rs:36-45, 105-108；INSTAB/unstable.rs:73-105, 140-143 |
| 展开形态：启用时 `#[cfg(feature = "unstable-rendered-line-info")] pub fn ...`；未启用时 `#[cfg(not(...))] pub(crate) fn ...` | INSTAB/unstable.rs:84-105 |
| `ratatui-widgets` 定义 `unstable = ["unstable-rendered-line-info"]`、`unstable-rendered-line-info = []` | RT-WIDGETS-CARGO:68-69 |
| `ratatui` 侧 `unstable-rendered-line-info = ["ratatui-widgets/unstable-rendered-line-info"]`，且默认特性不含它 | RT-CARGO:127-138, 74-80 |
| fs-agent 用 `ratatui = "0.30"`（默认特性），**没有**开启 `unstable-rendered-line-info` | FS-CARGO:33 |

→ **约束：票 03**——`line_count` 的一手语义**正好**是「给定内容宽度下的换行总行数（含 Block 上下占用）」，可用来夹紧 `scroll`；但在 fs-agent 当前依赖下它编译不可见，想用必须先开启 `ratatui` 的 `unstable-rendered-line-info` 特性（**需编译验证**开启后可见性；本轮未动 `Cargo.toml`，未编译）。

### 4.3 当前依赖下**稳定可用**的替代事实（同为一手）

| API | 语义 | 来源 |
| --- | --- | --- |
| `Text::height(&self) -> usize` | `self.lines.len()`，即 unwrapped 的 `Line` 条数 | RT-TEXT:293-305 |
| `Text::width(&self) -> usize` | 最宽行的显示宽度 | RT-TEXT:280-291 |
| `Line::width(&self) -> usize` | 单行宽度 | RT-LINE:441 |
| `Span::width(&self) -> usize` | 单 span 宽度 | RT-SPAN:271 |
| `WordWrapper`（换行算法本体） | `pub struct WordWrapper`、`pub const fn new(lines, max_line_width, trim)`、`next_line()`；**但所在模块是私有的**：`mod reflow;`（非 `pub mod`），且 `ratatui` 的 `widgets.rs` 未再导出 | RT-REFLOW:15, 31, 54-73；RT-WIDGETS-LIB:134；RT-WIDGETS-REEXPORT:667-691 |

→ **约束：票 03**——不启用 unstable 特性时，fs-agent 能稳定拿到的只有「unwrapped 行数 / 行宽」；换行后的行数没有稳定公开入口（`reflow` 模块私有，**需编译验证**是否有其它 re-export 路径——本轮 grep 未发现）。

### 4.4 `Paragraph::scroll` 不做夹紧

| 事实 | 来源 |
| --- | --- |
| 带 `wrap` 时：`for _ in 0..self.scroll.y { if line_composer.next_line().is_none() { return; } }`——scroll 超过总行数时直接渲染空白 | RT-PARA:431-439 |
| 不带 `wrap` 时：`styled.skip(self.scroll.y as usize)`——超过就得到空迭代器 | RT-PARA:441-445 |
| 渲染循环在 `y >= area.height` 时停止 | RT-PARA:450-459 |
| `scroll` 的 y/x 都是 `u16`，没有上界校验的代码 | RT-PARA:233-239 |

→ **约束：票 03**——`Paragraph` 不替调用方夹紧 scroll；「吸底 / 上滚不抢」的夹紧逻辑属于 fs-agent 自己的状态机。

### 4.5 `Scrollbar` / `ScrollbarState` 的存在性与签名

| 事实 | 来源 |
| --- | --- |
| `pub struct Scrollbar<'a>`；`pub enum ScrollbarOrientation`；`pub struct ScrollbarState`；`pub enum ScrollDirection { Forward, Backward }` | RT-SCROLLBAR:83, 106, 145, 163 |
| `ScrollbarState::new(content_length: usize) -> Self`（`position` 与 `viewport_content_length` 默认 0） | RT-SCROLLBAR:415-425 |
| fluent setters：`position(usize)`、`content_length(usize)`、`viewport_content_length(usize)` | RT-SCROLLBAR:433, 445, 454 |
| 状态操作：`prev(&mut self)`、`next(&mut self)`、`first(&mut self)`、`last(&mut self)`、`scroll(&mut self, ScrollDirection)`、`get_position() -> usize` | RT-SCROLLBAR:460-496 |
| `impl StatefulWidget for Scrollbar<'_>`（即 `render(area, buf, &mut ScrollbarState)`） | RT-SCROLLBAR:501 |
| `Scrollbar::new(ScrollbarOrientation)`；`impl Default for Scrollbar` 用 `ScrollbarOrientation::default()` | RT-SCROLLBAR:171-190 |
| `ratatui` 再导出：`ScrollDirection, Scrollbar, ScrollbarOrientation, ScrollbarState` | RT-WIDGETS-REEXPORT:684-686 |
| `viewport_content_length` 默认 0，文档说 0 时用 track 尺寸当作 viewport 长度 | RT-SCROLLBAR:140-154 |

→ **约束：票 03**——滚动条是 `StatefulWidget`，状态 `ScrollbarState` 以 `usize` 计「content_length / position / viewport_content_length」，与 `Paragraph::scroll` 的 `u16` 之间需要显式换算。

---

## 5. `Frame` 与光标

| 事实 | 来源 |
| --- | --- |
| `pub const fn area(&self) -> Rect { self.viewport_area }`，文档保证渲染期间不变 | RT-FRAME:60-70 |
| `size()` 已 `#[deprecated = "use area() instead"]` | RT-FRAME:72-82 |
| Fullscreen 下 `viewport_area` = 后端报告的终端尺寸，原点 `(0, 0)`；`with_options` 内 `Viewport::Fullscreen => (area, Position::ORIGIN)` | RT-TERM-INIT:122-133 |
| `Frame::area()` 在 Fullscreen 下始终 `(0,0)` 起；在 Inline/Fixed 下可能非零原点，文档要求以 `Frame::area()` 作为布局根 | RT-VIEW:20-22, 71-72, 110-112 |
| `pub fn set_cursor_position<P: Into<Position>>(&mut self, position: P)`；不调用则帧末隐藏光标 | RT-FRAME:154-168 |
| `set_cursor(&mut self, x: u16, y: u16)` 已 deprecated | RT-FRAME:170-183 |
| 光标在帧 buffer diff 被 flush **之后**才应用 | RT-FRAME:157；RT-RENDER:296-303 |
| 坐标空间链条：`Frame::set_cursor_position` → `Terminal::set_cursor_position` → `Backend::set_cursor_position` → crossterm `MoveTo(x, y)` | RT-FRAME:166-168 → RT-RENDER:299-302 → RT-CURSOR:80-85 → RT-CROSSTERM:308-311 |
| `Terminal::hide_cursor(&mut self) -> Result<(), B::Error>` / `show_cursor` 存在，二者都同时更新后端与 `hidden_cursor` | RT-CURSOR:15-34 |
| `draw` 无事请求光标时隐藏；请求时 `show_cursor()` + `set_cursor_position()` | RT-RENDER:296-303 |
| `Terminal` 的 `Drop` 会在 `hidden_cursor` 为真时尝试 `show_cursor()` | RT-TERM:473-487 |
| 文档警告 `hide_cursor`/`show_cursor`/`set_cursor_position` 会被下一次成功 draw 覆盖，建议二选一 | RT-CURSOR:8-14, 22-29；RT-FRAME:159-161 |

**坐标空间说明（严格按源码）：** 源码链条显示 `Frame::set_cursor_position` 传入的值最终原样进入 crossterm `MoveTo(x, y)`，即**终端绝对坐标**；但 rustdoc **没有**逐字写 "absolute/relative" 的说明 → 该措辞本身标 ⚪（见第 11 节）。

→ **约束：票 04 / 票 07**——多行输入的光标必须用 `Frame::set_cursor_position`（绝对终端坐标），否则每帧末会被自动隐藏；`Frame::area()` 可用于把输入区矩形换算成绝对坐标。

---

## 6. bracketed paste

### 6.1 命令的存在性、所属模块与特性门

| 事实 | 来源 |
| --- | --- |
| `crossterm::event::EnableBracketedPaste`（`#[cfg(feature = "bracketed-paste")]`），`write_ansi` 写 `CSI ?2004h` | CT-EVENT:413-427 |
| `crossterm::event::DisableBracketedPaste`（同特性门），写 `CSI ?2004l` | CT-EVENT:438-447 |
| 二者都在 **`crossterm::event` 模块**（不在 `terminal`） | CT-EVENT:413-447 |
| `bracketed-paste` 是 crossterm **默认特性**之一 | CT-CARGO:48-56 |
| fs-agent 用 `crossterm = { version = "0.29", features = ["event-stream"] }`，未关默认特性 → `bracketed-paste` 已启用 | FS-CARGO:34；CT-CARGO:48-55 |

→ **约束：票 04**——命令名与模块已确定，导入路径是 `crossterm::event::{EnableBracketedPaste, DisableBracketedPaste}`。

### 6.2 开启后到达的事件类型

| 事实 | 来源 |
| --- | --- |
| `Event::Paste(String)` 变体，带 `#[cfg(feature = "bracketed-paste")]`；文档：`A string that was pasted into the terminal. Only emitted if bracketed paste has been enabled.` | CT-EVENT:559-562 |
| 解析入口：`parse_event` 在 CSI 且前缀为 `ESC[200~` 时调用 `parse_csi_bracketed_paste` | CT-PARSE:196-198 |
| `parse_csi_bracketed_paste`：要求以 `ESC[200~` 开头、以 `ESC[201~` 结尾；未闭合时返回 `None`（等待更多字节），完整时构造 `Event::Paste` | CT-PARSE:812-823 |

→ **约束：票 04**——粘贴不会退化成逐字 `Key` 事件，而是单个 `Event::Paste(String)`。

### 6.3 crossterm 对粘贴内容做了什么处理（以源码为准）

| 事实 | 来源 |
| --- | --- |
| 内容 = `String::from_utf8_lossy(&buffer[6..buffer.len() - 6]).to_string()`，即剥掉 `ESC[200~`(6 字节) 与 `ESC[201~`(6 字节) 后**原样**取中间字节 | CT-PARSE:815-822 |
| **不剥 `\r`**：源码中没有对 `\r` 的任何处理 | CT-PARSE:812-823 |
| **不过滤控制字符**：官方测试断言内部转义序列被保留：`parse_event(b"\x1B[200~o\x1B[2D\x1B[201~", false)` → `Event::Paste("o\x1B[2D")` | CT-PARSE:1054-1072 |
| **无长度上限**：源码无长度检查，`String::from_utf8_lossy` 不截断 | CT-PARSE:812-823 |
| 唯一的「处理」是 UTF-8 容错：非法字节会被替换为 `U+FFFD`（`from_utf8_lossy` 语义） | CT-PARSE:820 |

→ **约束：票 04**——粘贴的多行文本会**带着 `\r`/`\n` 原文**到达 `Event::Paste`，所以「粘贴不得触发提交」必须由 fs-agent 在事件层保证（因为 paste 事件根本不是 `Key` 事件）；控制字符需要自己决定是否清洗。

---

## 7. Shift+Enter 能不能被区分

### 7.1 增强协议相关 API 的现状与签名

| API | 签名 | 来源 |
| --- | --- | --- |
| `supports_keyboard_enhancement` | `pub fn supports_keyboard_enhancement() -> io::Result<bool>`（`crossterm::terminal` 再导出） | CT-TERM:102；CT-SYS-UNIX:188-190 |
| 探测实现 | 向 `/dev/tty`（失败则 stdout）写 `ESC[?u` + `ESC[c`，以 2000ms 超时轮询；收到 KeyboardEnhancementFlags 响应才算支持 | CT-SYS-UNIX:222-267 |
| 阻塞警告 | 文档逐字：`On unix systems, this function will block and possibly time out while crossterm::event::read or crossterm::event::poll are being called.` | CT-SYS-UNIX:183-186 |
| `PushKeyboardEnhancementFlags` | `pub struct PushKeyboardEnhancementFlags(pub KeyboardEnhancementFlags)`，写 `CSI > {bits} u` | CT-EVENT:492-498 |
| `PopKeyboardEnhancementFlags` | `pub struct PopKeyboardEnhancementFlags`，写 `CSI < 1 u` | CT-EVENT:516-527 |
| `KeyboardEnhancementFlags` | `bitflags! { pub struct KeyboardEnhancementFlags: u8 { DISAMBIGUATE_ESCAPE_CODES = 0b0000_0001, REPORT_EVENT_TYPES = 0b0000_0010, REPORT_ALTERNATE_KEYS = 0b0000_0100, REPORT_ALL_KEYS_AS_ESCAPE_CODES = 0b0000_1000 } }` | CT-EVENT:284-311 |
| `KeyEventState` 填充条件 | 文档：`state` 仅在 `DISAMBIGUATE_ESCAPE_CODES` 启用后设置；`kind` 仅在 Unix 的 `REPORT_EVENT_TYPES`（或 Windows 上始终）设置 | CT-EVENT:942-952 |

→ **约束：票 04**——增强协议是可选的运行时能力探测 + 需要 push/pop 成对；探测本身在事件循环并发时会阻塞。

### 7.2 没有增强协议时 Enter 带什么 modifier

| 事实 | 来源 |
| --- | --- |
| 裸字节 `b'\r'` → `Event::Key(KeyCode::Enter.into())` | CT-PARSE:92-94 |
| `impl From<KeyCode> for KeyEvent`：`modifiers: KeyModifiers::empty(), kind: KeyEventKind::Press, state: KeyEventState::empty()` | CT-EVENT:1025-1034 |
| 因此**没有增强协议时，Enter 的 modifiers 为空**；Shift+Enter 若终端不上报协议，到达的 `KeyEvent` 与普通 Enter **完全一致** | CT-PARSE:92-94 + CT-EVENT:1025-1034 |
| `b'\n'`：仅在 **raw mode 未启用**时才映射为 `KeyCode::Enter`；源码注释解释 raw mode 会禁用终端的 `\r`→`\n` 转换，因此 `\n` 保留给 Ctrl+J | CT-PARSE:95-101 |
| raw mode 下裸字节 `\n` 落入 `c @ b'\x01'..=b'\x1A'` 分支 → `KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)` | CT-PARSE:106-109 |
| `char_code_to_event`：大写字符补 `SHIFT` modifier（只对 `KeyCode::Char` 生效，对 `Enter` 不生效） | CT-PARSE:128-135 |

→ **约束：票 04**——「Ctrl-J 换行」在 raw mode 下是天然可区分的（`Char('j') + CONTROL`）；「Shift+Enter 换行」在无协议终端上**不可区分**，因此 fs-agent 的「尽力而为」措辞与源码一致。

### 7.3 有增强协议时 Shift+Enter 的形态

| 事实 | 来源 |
| --- | --- |
| CSI-u 解析：`modifiers` 由 `parse_modifiers(mask)` 得出，`modifier_mask = mask.saturating_sub(1)`，bit0→`SHIFT`、bit1→`ALT`、bit2→`CONTROL`、bit3→`SUPER`、bit4→`HYPER`、bit5→`META` | CT-PARSE:303-325 |
| 码点 `'\r'` → `KeyCode::Enter` | CT-PARSE:545-548 |
| 因此启用 `DISAMBIGUATE_ESCAPE_CODES` 后，Shift+Enter 到达为 `KeyCode::Enter` + `KeyModifiers::SHIFT` | CT-PARSE:303-325 + 545-548 |

→ **约束：票 04**——若启用增强协议，判定条件可写成 `Enter && modifiers.contains(SHIFT)`；不启用时该分支永远不成立，与「退化成 Enter 即提交」一致。

### 7.4 官方列出支持该协议的终端

`PushKeyboardEnhancementFlags` 的 rustdoc 逐字列出（CT-EVENT:483-491）：kitty terminal、foot terminal、WezTerm、alacritty、notcurses library、neovim text editor、kakoune text editor、dte text editor。

⚠ 该列表是 **0.29.0 源码里的静态文本**，未声称穷尽或时效性 → 列表之外的支持情况标 ⚪（第 11 节）。

→ **约束：票 04**——官方名单不含 GNOME Terminal / Konsole / Windows Terminal，与「Shift+Enter 尽力而为」的假设方向一致，但 fs-agent 是否在这些终端上运行需实测（本轮无终端实验）。

---

## 8. 同步更新（synchronized update）

| 事实 | 来源 |
| --- | --- |
| `crossterm::terminal::BeginSynchronizedUpdate`（单元结构体），`write_ansi` 写 `CSI ?2026h` | CT-TERM:432-437 |
| `crossterm::terminal::EndSynchronizedUpdate`，写 `CSI ?2026l` | CT-TERM:485-490 |
| 文档语义：开启后「following render calls will keep rendering the last rendered state」，关闭后终端取最新 screen buffer，用于避免 tearing | CT-TERM:406-414, 459-467 |
| 存在公开便利 trait：`pub trait SynchronizedUpdate { fn sync_update<T>(&mut self, operations: impl FnOnce(&mut Self) -> T) -> io::Result<T>; }`，对所有 `W: std::io::Write` 实现；实现为 `queue(BeginSynchronizedUpdate)` → 执行闭包 → `execute(EndSynchronizedUpdate)` | CT-CMD:186-191, 245-249 |
| **ratatui 的 draw 路径没有自己包裹同步更新**：在 `ratatui-0.30.2/src`、`ratatui-core-0.1.2/src`、`ratatui-crossterm-0.1.2/src`、`ratatui-widgets-0.3.2/src` 内检索 `SynchronizedUpdate` / `2026` **无任何匹配** | 全树 grep（见 §0 路径） |
| ratatui 的 flush 只调 `Backend::draw` + `Backend::flush`；`CrosstermBackend::flush` 就是 `self.writer.flush()` | RT-BUFFERS:97-114；RT-RENDER:307-308；RT-CROSSTERM:358-360 |
| 文档小瑕疵（事实记录）：`BeginSynchronizedUpdate` 的 Notes 写「Use EndSynchronizedUpdate to leave the entered **alternate screen**」，`EndSynchronizedUpdate` 的 Notes 反向写成「enter the alternate screen」——是文档复制错误，不是 alt screen 行为 | CT-TERM:404, 457 |

→ **约束：票 07**——同步包裹必须由 fs-agent 自己做，且必须在 `execute!`/`queue!` 层把 Begin/End 放到覆盖 `terminal.draw(...)`（以及其内部 `backend.flush`）的位置；ratatui 不提供接入点。

**官方示例里与 alt screen 一起用的形态：** 本机 vendored 的 crossterm 0.29.0 源码 docs 只给出独立用法（`execute!(BeginSynchronizedUpdate)` … `execute!(EndSynchronizedUpdate)`，CT-TERM:416-430），**未**出现与 alt screen 组合的示例；ratatui 0.30.2 的 crate 包内**不含 `examples/` 目录**（`$REG/ratatui-0.30.2/` 只有 `src/`、`tests/`、`README.md` 等），故「ratatui 官方 alt screen + sync 示例」本轮**未取得一手文件** → ⚪（第 11 节）。

---

## 9. 异步事件与 tick

### 9.1 `EventStream` 的形态

| 事实 | 来源 |
| --- | --- |
| `pub struct EventStream`，文档逐字：`This type is not available by default. You have to use the event-stream feature flag to make it available.` | CT-STREAM:21-38 |
| `impl Stream for EventStream { type Item = io::Result<Event>; }`——Item 是 **Result** | CT-STREAM:101-103 |
| `pub fn new() -> EventStream` 即 `Default`；`Default` 内 `thread::spawn` 跑 `poll_internal(None, &EventFilter)`，用一个 `SyncSender<Task>` 唤醒 executor | CT-STREAM:40-75 |
| `Drop` 会置 shutdown 标志并唤醒内部线程 | CT-STREAM:140-146 |
| 特性定义：`event-stream = ["dep:futures-core", "events"]`；`events = ["dep:mio", "dep:signal-hook", "dep:signal-hook-mio"]` | CT-CARGO:57-65 |
| fs-agent 已启用 `event-stream` | FS-CARGO:34 |

→ **约束：票 07**——`EventStream::next()` 的 item 需要 `Some(Ok(event))` 两层解构；它内部有线程，Drop 时会自清理。

### 9.2 `Event::Resize` / `FocusGained` / `FocusLost` 的可用性

| 事实 | 来源 |
| --- | --- |
| `Event` 变体：`FocusGained`、`FocusLost`、`Key(KeyEvent)`、`Mouse(MouseEvent)`、`Paste(String)`（feature 门）、`Resize(u16, u16)` | CT-EVENT:550-566 |
| `Resize` 文档：`An resize event with new dimensions after resize (columns, rows). **Note** that resize events can occur in batches.` | CT-EVENT:563-565 |
| 焦点事件需先 `EnableFocusChange`（写 `CSI ?1004h`）/ `DisableFocusChange`（`CSI ?1004l`）才会被终端发出；Windows 上焦点事件始终启用 | CT-EVENT:377-411 |
| ratatui 侧要求：resize 事件只当作「再渲染一次」的信号，布局以 `Frame::area()` 为准（因为尺寸事件可能合并/丢失/早到） | RT-TERM:233-241 |

→ **约束：票 07**——`Resize` 一定可用（Unix 侧由 signal-hook 提供，见 CT-CARGO:61-65 的 `signal-hook` 依赖）；焦点事件要显式 `EnableFocusChange`。

### 9.3 官方示例里 tick + 事件 + 绘制的形状

| 事实 | 来源 |
| --- | --- |
| crossterm 官方 tokio 示例文件：`$REG/crossterm-0.29.0/examples/event-stream-tokio.rs`；形状是 `select!` 合并 `futures_timer::Delay` 与 `reader.next().fuse()`，但它是**打印示例**、没有 `Terminal::draw` | CT-EX-TOKIO 全文 |
| ratatui 官方异步示例（main 分支）文件：`examples/apps/async-github/src/main.rs`；`tokio::time::interval(period)`（`FRAMES_PER_SECOND = 60.0`）与 `EventStream::new()` 放进 `tokio::select!`：`_ = interval.tick() => terminal.draw(...)`、`Some(Ok(event)) = events.next() => self.handle_event(...)`；`main` 为 `ratatui::init()` → run → `ratatui::restore()` | WEB-EX-ASYNC（抓取 2026-09-21） |
| ratatui 0.30.2 **已发布 crate 包内不含 examples 目录**，故该示例只能以官方仓库 main 分支为准（版本可能领先于 0.30.2） | 本机 `$REG/ratatui-0.30.2/` 目录清单 |

→ **约束：票 07**——「tick + 事件 + draw 在同一 `select!`」有官方一手先例；注意示例用 `tokio::time::interval`（fs-agent 已启用 tokio `time`，FS-CARGO:26），而 crossterm 那个例子用 `futures_timer`（fs-agent 无此依赖）。

---

## 10. alt screen 的一手取舍陈述

### 10.1 官方对 alt screen 与 scrollback / 选择 / 复制 / 滚轮的原文

| 陈述（逐字或直译） | 来源 |
| --- | --- |
| alt screen「**has the exact dimensions of the terminal window, without any scroll-back area**」 | CT-TERM:26-32（31-32 行逐字） |
| crossterm 只提供 main/alternate 两面切换，且「the main screen will stay intact and will have the original data as we performed all operations on the alternative screen」 | CT-TERM:18-41 |
| ratatui：alt screen 是一个独立 buffer；退出时「the terminal will switch back to the main screen, and **the contents of the alternate screen will be cleared**」；用途是「use the full terminal window without disrupting the command line or other terminal content」 | RT-BACKEND:61-78 |
| 「**not all terminal emulators support the alternate screen**, and even those that do may handle it differently」 | RT-BACKEND:75-78 |
| 官方站点同段声明与上条一致，并附 with/without alt screen 的对照 demo；**全页未提**滚轮 / 文本选择 / 复制 / scrollback | WEB-ALT（抓取 2026-09-21） |
| `ratatui::init` 的文档：`init` → `Creates a terminal with reasonable defaults including alternate screen and raw mode`；`init_with_options`/`run_with_options` → `Enables raw mode but not alternate screen` | RT-INIT:52-58, 182-188 |

**⚪ 未证实**：本轮在本机 vendored 源码（crossterm README、`docs/know-problems.md`、ratatui README、ratatui-core/crossterm 全部源码）与官方 alternate-screen 页面中，**没有找到**任何关于「alt screen 与终端原生鼠标滚轮 / 文本选择 / 复制」关系的官方陈述。唯一与 scrollback 直接相关的一手原文是上述「without any scroll-back area」。

→ **约束：票 02 / 票 03**——能作为事实的是「alt screen 无 scrollback 区、退出即清除」；「原生选择只覆盖可见区」这一前提**没有**一手来源支撑（第 11 节）。

### 10.2 官方的「退出 alt screen 后把内容打印出来」的 helper

| 事实 | 来源 |
| --- | --- |
| `try_restore()` 只做两件事：`disable_raw_mode()?` + `execute!(stdout(), LeaveAlternateScreen)?`——**不打印任何内容** | RT-INIT:554-560 |
| `restore()` 只是 `try_restore()` 的错误打印壳 | RT-INIT:524-529 |
| `Terminal::clear()` 在 Fullscreen 下清**整个**终端并强制下次全量重画 | RT-BUFFERS:126-152, 158-174 |
| `Terminal::insert_before(height, draw_fn)` 是唯一「把内容写进终端历史/上方」的 helper，但**只在 inline viewport 下有效**，Fullscreen 下是 no-op | RT-INLINE:7-8, 109-120 |
| crossterm 有 `ClearType::Purge`（`All plus history` 枚举注释），但 ratatui 的 `ClearType` 映射里没有 `Purge`（只映射 All/AfterCursor/BeforeCursor/CurrentLine/UntilNewLine） | CT-TERM:271-279；RT-CROSSTERM:317-328 |

**结论（源码事实）：ratatui/crossterm 没有「退出 alt screen 后把 TUI 内容重新打印到主屏」的 helper。** `restore` 只切面；`insert_before` 在 Fullscreen 下无效。

→ **约束：票 02 / 票 07**——若需要「退出后保留内容」，一手 API 里没有现成手段；`ClearType::Purge` 这种清历史的终端能力 ratatui 未暴露。

---

## 11. ⚪ 未证实 / 查不到

1. **`Frame::set_cursor_position` 坐标空间的官方措辞**——源码链条（RT-FRAME:166-168 → RT-CURSOR:80-85 → RT-CROSSTERM:308-311）显示传入值原样进入 crossterm `MoveTo(x, y)`，即终端绝对坐标；但 rustdoc 未逐字写 "absolute"（只说「put it at the specified (x, y) coordinates」）→ 措辞层面 ⚪。
2. **alt screen 与终端原生鼠标滚轮 / 文本选择 / 复制的官方关系**——见 10.1；本轮未找到任何一手陈述 → ⚪。
3. **官方 ratatui「alt screen + synchronized update」示例**——0.30.2 crate 包不含 `examples/`；官网 alternate-screen 页也没有该组合 → ⚪。
4. **`Paragraph::line_count` 在开启 `unstable-rendered-line-info` 后从 fs-agent 可见**——属性展开规则支持该结论（INSTAB/unstable.rs:84-105），但**需编译验证**。
5. **`paint_scrollback` 删除后是否真的无引用**——本轮只对 `src/` 做了 grep（FS-TUI:220 是唯一内部调用点，FS-MOD:52 是再导出）；`tests/` 与其它 crate 内的引用**未逐一核查**，需编译验证。
6. **Shift+Enter 在各终端上的真实行为**——源码只给出「无协议即不可区分」与官方支持名单；fs-agent 所在终端（如 GNOME Terminal / Konsole / Windows Terminal）的实际上报行为**未实测** → ⚪。
7. **`supports_keyboard_enhancement()` 在 fs-agent 事件循环中的实际返回值与超时表现**——源码写明会阻塞/2s 超时（CT-SYS-UNIX:183-190, 222-267），但本机未跑探测 → ⚪。
8. **键盘增强协议支持名单的时效性**——列表是 crossterm 0.29.0 源码内静态文本（CT-EVENT:483-491），是否覆盖当前主流终端 ⚪。
9. **crossterm `Event::Paste` 内容的最大长度 / 内存行为**——源码无长度上限（CT-PARSE:812-823），但超大粘贴在 fs-agent 侧的实际驻留内存**未测量** → ⚪。
10. **ratatui `Terminal` / `draw` 是否必须在单一线程**——本轮未在 vendored 源码中找到「必须独占线程」的声明；`Terminal` 需要 `&mut self`（RT-RENDER:81）是事实，但线程约束本身 ⚪。
11. **窗口宽度 < `u16::MAX` 的极端环境下 `Paragraph::line_count` 的计数上限行为**——未读 `WordWrapper` 的完整换行实现（只读了前 90 行），换行算法细节（对齐、grapheme 边界）未逐行核对 → 计数精度 ⚪。

---

## 12. 交叉核对：与票面/地图既有假设的出入（纯事实）

1. **票面问「有没有 `line_count`」——有，但默认不可调用。** `Paragraph::line_count(width) -> usize` 确实存在且语义正好是换行行数（RT-PARA:332-365），但带 `#[instability::unstable(feature = "rendered-line-info")]`（RT-PARA:328-331），在 fs-agent 当前 `ratatui = "0.30"` 默认特性下会被展开成 `pub(crate)`（INSTAB/unstable.rs:84-105）。**这直接改变票 03 的前置条件**：要么开 `unstable-rendered-line-info`，要么放弃用该 API。
2. **`insert_before` 没有被移除**，仍在 `ratatui-core 0.1.2`，且对 Fullscreen 是 no-op（RT-INLINE:109-120）。地图里说要删的 `insert_before` / `paint_scrollback` 名字都对得上；`paint_scrollback` 是 fs-agent 自己的函数（FS-TUI:773），不是 ratatui API。
3. **`Viewport::Fullscreen` 是 `TerminalOptions` 默认值**（RT-VIEW:77-78），但 `init_with_options` 不自动进 alt screen（RT-INIT:416-418, 492-497）——「用了 Fullscreen viewport 就等于 alt screen」不成立。
4. **`Paragraph` 不会夹紧 `scroll`**（RT-PARA:431-445）——票 03 的「能不能夹紧」由调用方负责。
5. **ratatui 不包裹 synchronized update**（全树无匹配）——票 07 的同步包裹必须自建。
6. **`Event::Paste` 内容不做 `\r` 剥离/控制字符过滤/长度限制**（CT-PARSE:812-823）——票 04 的「粘贴保护」不能依赖 crossterm 预清洗。
