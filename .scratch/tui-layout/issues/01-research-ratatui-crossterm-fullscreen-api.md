# research：ratatui 0.30 / crossterm 0.29 全屏布局、滚动与键盘输入的 API 事实

Type: research
Status: resolved
Blocked by: —

## Question

为票 02（布局原型）、票 03（对话面板滚动）、票 04（多行输入）、票 07（渲染管线）提供**外部事实**。

判据：**只报告事实与来源，不推荐方案、不选赢家。** 凡一手来源未写明者标 ⚪ 未证实，**不做推断**。

**一手来源优先用本机已 vendored 的源码**——离线、精确、且就是我们编译的那一份（`Cargo.toml` 锁 `ratatui = "0.30"` / `crossterm = { version = "0.29", features = ["event-stream"] }`）：

- `~/.cargo/registry/src/index.crates.io-*/*/ratatui-0.30.2/`
- `~/.cargo/registry/src/index.crates.io-*/*/ratatui-core-0.1.2/`
- `~/.cargo/registry/src/index.crates.io-*/*/ratatui-crossterm-0.1.2/`
- `~/.cargo/registry/src/index.crates.io-*/*/ratatui-widgets-0.3.2/`
- `~/.cargo/registry/src/index.crates.io-*/*/crossterm-0.29.0/`

（registry 在本沙箱可能只读 —— 只读，不写。）只有源码没写清时才上 docs.rs / 官方仓库，并给出 URL 与抓取日期。

## 需要查明

1. **全屏模式的初始化与拆卸**：`ratatui::init` / `init_with_options` / `restore` 的精确签名与全部副作用。`TerminalOptions` 有哪些字段（`viewport`? `mouse_capture`? `is_raw`?），各自默认值是什么；`Viewport::Fullscreen` 是不是默认。alt screen 的进入/退出由谁发、发什么序列。panic hook 行为。bracketed paste 有没有被 `init` 顺带打开（若源码没有，直接回答"没有"）。
2. **全屏下 `Terminal::draw` 的重绘语义**：是 buffer diff（只写变化的 cell）还是整屏重写？`resize` / `clear` 在什么条件下发生？一次 draw 的写入量级。这直接决定票 07 的节拍与同步包裹点。
3. **`Viewport::Inline` 与 `Terminal::insert_before` 在当前版本里是否仍在**：确认我们要删的东西叫什么、签名是什么、删掉之后哪些 helper（`paint_scrollback` 一类）会变成死码。
4. **可滚动长文本**：`Paragraph` 的 `scroll` / `Wrap` / `Block` 当前签名；**能否拿到换行后的行数**——有没有 `line_count` / `height_for` 一类的一手 API，签名与返回语义是什么（这是票 03「20 000 行按什么单位算、能不能夹紧滚动」的前置事实）。`Scrollbar` / `ScrollbarState` 在 `ratatui-widgets` 里的存在性与签名。
5. **`Frame` 与光标**：Fullscreen 下 `Frame::area()` 的语义与坐标空间；`Frame::set_cursor_position` 的坐标空间（相对 area 还是绝对）；`Terminal::show_cursor` / `hide_cursor` 的现状。
6. **bracketed paste**：crossterm 0.29 里 `EnableBracketedPaste` / `DisableBracketedPaste` 的存在与所属模块；开启后到达的事件类型（是不是 `Event::Paste(String)`）；**crossterm 对粘贴内容做了什么处理**——是否剥掉 `\r`、是否过滤控制字符、有没有长度上限。以源码为准。
7. **Shift+Enter 能不能被区分**：crossterm 0.29 的 `supports_keyboard_enhancement` / `PushKeyboardEnhancementFlags` / `KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES` 的现状与签名；**没有增强协议时 Enter 会带着什么 modifier 到达**（源码里 `parse_event` 对 `\r` / `\n` 的处理）；官方列出了哪些支持该协议的终端。
8. **同步更新**：`BeginSynchronizedUpdate` / `EndSynchronizedUpdate` 在 crossterm 0.29 的现状与用法；ratatui 的 draw 路径是否已经自己包裹；官方示例里与 alt screen 一起用的形态。
9. **异步事件与 tick**：`EventStream` 在当前版本里的形态（feature `event-stream`）；`Event::Resize` / `Event::FocusGained` / `Event::FocusLost` 的可用性；官方示例里 tick + 事件 + 绘制的标准形状（给出文件路径）。
10. **alt screen 的一手取舍陈述**：官方文档 / README / 示例对 alt screen 与"滚轮 / 选择 / 复制 / scrollback"关系的原文；有没有官方的"退出 alt screen 后把内容打印出来"的 helper（例如 `restore`、`Terminal::clear`、`insert_before` 一类）。

## 产出

`.scratch/tui-layout/research/01-ratatui-crossterm-fullscreen-api.md`：

- 来源清单（ID + URL 或本机路径 + 版本 + 抓取日期）
- 逐条事实，每条附**文件路径与行号**（本机源码）或 URL
- 每条事实一句话说明它**约束了哪张票的哪个决定**
- 结尾一节「⚪ 未证实 / 查不到」如实列出

**不写建议、不选赢家。** 答案必须自足（`/implement` 在 `/clear` 后读它）。

## Comments

- 2026-09-14 charting：本票由 charting 会话的后台 research subagent 认领。启动时前沿只剩本票与票 05（grilling，需要用户在场）。

## Answer

完整事实与逐条来源（文件路径 + 行号 / URL + 抓取日期）见
`.scratch/tui-layout/research/01-ratatui-crossterm-fullscreen-api.md`（抓取 2026-09-21，全部为本机 vendored 源码）。

10 个问题的确定答案概要：

1. `ratatui::init()` = `DefaultTerminal`（`CrosstermBackend<Stdout>`）+ raw mode + `EnterAlternateScreen`（`CSI ?1049h`）+ panic hook（`restore()` 后调原 hook）；`init_with_options` **不进** alt screen。`TerminalOptions` 只有 `viewport` 一个字段，`Viewport::Fullscreen` 是其默认值。`init` **不**开 bracketed paste。
2. `Terminal::draw` 是 **buffer diff**（`BufferDiff` 只 yield 变化 cell），不是整屏重写；尺寸变化才触发 `ClearType::All` + 后缓冲 reset（全量重画）。
3. `Viewport::Inline(u16)` 与 `Terminal::insert_before(height, FnOnce(&mut Buffer))` **都还在**；`insert_before` 对非 inline viewport 是 no-op。
4. `Paragraph::scroll((y, x))` 的 y 单位是换行后行数，且**不夹紧**。`Paragraph::line_count(width) -> usize` 存在且语义正确，但被 `#[instability::unstable(feature = "rendered-line-info")]` 挡成 `pub(crate)`——fs-agent 当前默认特性下**不可调用**（与票面假设相反，见第 12 节）。`ScrollbarState` 以 `usize` 计 content_length/position/viewport_content_length。
5. Fullscreen 下 `Frame::area()` = `(0,0,w,h)`；`Frame::set_cursor_position` 经 `Backend` 原样 `MoveTo(x,y)`（终端绝对坐标），不设置则帧末隐藏；`Terminal::show_cursor`/`hide_cursor` 仍在但会被下次 draw 覆盖。
6. `EnableBracketedPaste`/`DisableBracketedPaste` 在 `crossterm::event`（默认特性已启用），到达 `Event::Paste(String)`；crossterm **不剥 `\r`、不过滤控制字符、无长度上限**，只做 UTF-8 lossy。
7. 无增强协议时 `\r` → `Enter` + 空 modifier，**Shift+Enter 与 Enter 不可区分**；裸 `\n` 在 raw mode 下是 `Ctrl+J`。启用 `DISAMBIGUATE_ESCAPE_CODES` 后 Shift+Enter = `Enter` + `SHIFT`；`supports_keyboard_enhancement()` 会阻塞/2s 超时。
8. `BeginSynchronizedUpdate`(`CSI ?2026h`)/`EndSynchronizedUpdate`(`CSI ?2026l`) 存在，另有公开 trait `SynchronizedUpdate::sync_update`；**ratatui 的 draw 路径不包裹它**。
9. `EventStream`（feature `event-stream`）是 `Stream<Item = io::Result<Event>>`，内部起线程；`Event::Resize` 必有、焦点事件需 `EnableFocusChange`；官方 main 分支示例用 `tokio::time::interval` + `EventStream` 放同一 `tokio::select!`。
10. alt screen 官方一手陈述只有「无 scroll-back area」与「退出即清除」；**没有**「退出后把内容打印出来」的 helper，`restore` 只切面，`insert_before` 在 Fullscreen 下无效。

⚪ 未证实项已在 research 文件第 11 节逐条列出（含：alt screen 与原生选择/滚轮的官方关系、官方 alt screen + sync 示例、`Frame::set_cursor_position` 的 "absolute" 措辞、以及几处**需编译验证**的可见性结论）。
