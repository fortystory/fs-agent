# research：TUI 库与语法高亮库的选型事实

Type: research
Status: resolved

## Question

本票为**票 13（渲染接缝：plain / TUI / headless）**提供外部事实。票面第 5 条点名了这个缺口，并且已核实：`ratatui` / `crossterm` / `tui` 在当时的 `.scratch/v1-architecture/research/`（那份笔记今在 `research/08-rust-ecosystem.md`；v1-architecture 图已删除，见 git 历史 `b510e6b`）、`.scratch/multi-agent-architecture/research/` 与 `docs/research/coding-agent-features.md` 里**全部零命中**。票面同时保留了"diff 语法高亮"（明确不做的只是"高亮之外的编辑器能力"），所以高亮库也要一并查。

判据：**只报告事实与来源，不推荐方案、不选赢家**。凡一手来源未写明者标 ⚪ 未证实，**不做推断**。

### 需要查明

1. **TUI 栈的现状**（crates.io API 元数据 + docs.rs + 官方仓库，照票 08 / 23 / 24 的方法）：
   - `ratatui`：最新版本与发布日期、`rust-version`（MSRV）、**normal 依赖闭包大小**（`cargo tree -e normal -q --prefix none`，去重、排除 root 与 dev/build 边）、仓库最后提交时间。
   - **后端**：`crossterm` / `termion` / `termwiz` 各自的最新版本、维护状态、**各自会引入多少依赖**；哪一个（若有）是默认特性；特性开关的名字。
   - **替代品**：Rust TUI 生态里还有哪些有实际维护的候选（例如 `cursive`；若 `tui-rs` 已废弃请给出废弃的一手依据），各自的最新版本与维护状态。**只要事实，不要比较优劣。**
2. **与异步运行时的配合方式**（官方文档 / 官方仓库示例为准）：
   - 官方有没有 **tokio 的示例**？它是"在异步任务里 draw"还是"用独立的线程 + channel"？
   - **事件循环的形状**：官方的标准模式是什么（poll 超时 + 重绘 tick？`crossterm::event::EventStream`？）——给出官方示例的出处。
   - 有没有官方文档/示例说明**终端后端是否需要独占一个线程**（因为 `Terminal` 的借用模型）。
3. **inline viewport**（对一个 coding agent CLI 很关键）：官方有没有**非 alt-screen、在光标处渲染固定高度区域**的能力（例如 `Viewport::Inline`）？给出 API 名与官方文档；以及它的已知限制（例如滚动、尺寸变化）有没有一手说明。
4. **语法高亮的候选**（票 13 保留了 diff 高亮）：
   - `syntect`（版本 / 维护 / 依赖闭包 / 它自带哪些语法定义、是否依赖 `onig` 还是纯 Rust 的 `fancy-regex`）。
   - `tree-sitter-highlight`（版本 / 维护 / 它与 `tree-sitter` 运行时的版本关系；票 23 的产物 `research/04` 已经测过 `tree-sitter` + `tree-sitter-rust` 的依赖闭包与解析耗时，**本票只需补 highlight 这一层**）。
   - 若还有其它有实际维护的候选（例如 `bat` 抽出来的库），给出名称与一手状态。
5. **终端能力探测**：有没有一手文档说明如何判断"当前 stdout 是不是 TTY""终端是否支持真彩色/alt screen"（例如 `crossterm` 或 `std::io::IsTerminal` 的官方说明）。这一条决定了 headless 模式如何自动降级。

**边界**：不要调研渲染架构、事件流消费方式、UI 布局——那些是票 13 的决策，不是事实。本票只报"有哪些东西、多重、官方怎么说"。

产出：`.scratch/multi-agent-architecture/research/06-tui-and-highlighting-crates.md`（中文；来源 ID 表 + 逐条引用 + ⚪ 缺口清单 + 完整 URL 清单；格式照 `research/02-provider-call-surface.md` 与 `research/04-tree-sitter-and-symbol-extraction.md`）。依赖闭包在 `/tmp` 探针 crate 里实测（本仓库没有 `Cargo.toml`），测完即删。

**本票不决定任何事**——它是票 13 的事实输入。

## Answer

**已解（2026-09-13，AFK，由 research 子代理执行）。**

**产物**：`.scratch/multi-agent-architecture/research/06-tui-and-highlighting-crates.md`（712 行：来源 ID 表 + 逐题表格 + ⚪ 清单 + 完整 URL 清单）。仓库内只新增该文件。

对票 13 最相关的几条（细节与来源见简报）：

- **TUI 栈现状**：`ratatui 0.30.2`（2026-06-19，**MSRV 1.88**、edition 2024；仓库最后提交 2026-09-10）。**默认特性已含 `crossterm`**；后端特性名 = `crossterm` / `termion` / `termwiz` / `termina`（另有 `crossterm_0_28` / `crossterm_0_29` 版本选择特性）。
- **依赖闭包（实测，Linux，`cargo tree -e normal -q --prefix none`）**：ratatui **默认 70** / 关默认 **41** / +crossterm **62** / +termion **45** / +termwiz **118** / +termina **54**；单独 `crossterm` 27、`termion` 3、`termwiz` 85；`cursive` **62**；`tui` **19**。
- **`tui`(tui-rs) 已废弃（硬证据）**：README 逐字「August 2023: This crate is no longer maintained」+ 仓库 **Public archive**。`cursive 0.21.1` 发布停在 2024-08-03，但仓库提交活跃到 2026-09-09（**无维护声明 → ⚪**）。
- **异步配合**：`crossterm` 官方有 `examples/event-stream-tokio.rs`（`EventStream` **内部自建线程 + SyncSender**，需 `event-stream` 特性）；**ratatui 官方示例与官方模板都在同一个 async 任务里 `select!(tick, EventStream)` 并 `draw`**，官网另有 `tokio::spawn` + mpsc 的 recipe。⚪ **「后端必须独占线程」无官方声明**——只有 `Terminal::draw(&mut self)` 的借用事实。
- **inline viewport 官方具备**：`Viewport::Inline(u16)` + `TerminalOptions` + `ratatui::init_with_options` + `Terminal::insert_before`；**`init_with_options` 只开 raw mode、不进 alt screen**。官方限制：全宽、锚定光标行 col 0、高度钳制、可能引发终端滚动、非 inline 时 `insert_before` 无效、无 scrolling-regions 时需清屏重绘。
- **高亮候选**：`syntect 5.3.0` 默认走 onig（`onig_sys` 用 `cc` 编译 Oniguruma C，**官方 Readme 承认构建困难**），可切 `default-fancy` 纯 Rust；闭包 42 / 44；自带 Sublime 默认语法集 + `syntect::util::as_24_bit_terminal_escaped`。`tree-sitter-highlight 0.27.0` 要求 **`tree-sitter ^0.27.0`**（与票 12 要拿的同一大版本），闭包 **15**，API = `Highlighter` / `HighlightConfiguration` / `HighlightEvent`，**但条目里只有 `HtmlRenderer`，没有终端渲染器**。`bat` 可作库但默认闭包 **157**。
- **终端能力探测**：`std::io::IsTerminal`（1.70 稳定）是官方 TTY 判定；`crossterm` 有 `tty::IsTty` 与 `style::available_color_count()`，但**未发布的 master 已删 `IsTty` 并建议改用 std**（无发布版本号 → ⚪）。**alt screen 的支持探测 API 不存在**（⚪）。
- ⚪ 缺口（简报第 6 节共 14 条）：后端线程模型无官方声明、alt screen 探测 API 不存在、crossterm 未发布改动的发布版本号、syntect 自带语法条数、非 Linux 目标的闭包数字、cursive / termion / termwiz / iocraft 的官方维护声明。
