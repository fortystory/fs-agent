# fs-agent：TUI 库与语法高亮库（事实简报）

> 目的：为**票 13（渲染接缝：plain / TUI / headless）**提供外部事实输入。本文件只报告**事实与来源**，不推荐方案、不选赢家、不下结论。
>
> 抓取日期：**2026-09-13（UTC）**。全部为 crates.io API 元数据、docs.rs、官方仓库源码 / README / `Cargo.toml` / CHANGELOG / 示例、官方文档站的阅读，外加一组**明确标注的本地测量**；未调用任何 LLM API。
>
> 来源分级：只使用 ✅ 一手来源（项目官方文档、官方仓库源码、官方 issue / CHANGELOG、crates.io / docs.rs 元数据、Rust 官方 std 文档）。凡一手来源未写明者一律标 **⚪ 未证实**，**不做推断**。
>
> 测量方法：依赖闭包照票 08 / 23 / 24 的方法 —— `cargo tree -e normal -q --prefix none`，**默认特性**（除非表中另有注明），去重后统计包数（含传递依赖，**不含 root、不含 dev/build 边**）。本仓库没有 `Cargo.toml`，因此所有 cargo 探针 crate 都建在 `/tmp` 下（仓库外），测完即删；仓库内除本文件外**未新增/修改任何文件**。测量环境：`cargo 1.94.0 (85eff7c80 2026-01-15)` / `rustc 1.94.0 (4a4ef493e 2026-03-02)`，`x86_64-unknown-linux-gnu`。`CARGO_HOME` 与 `CARGO_TARGET_DIR` 重定向到 `/tmp`（本机 `~/.cargo` 在沙箱中只读）。
>
> 已知偏差：`cargo tree -e normal` 只展开**当前目标平台**的依赖；Windows / macOS 专属依赖（`winapi`、`crossterm_winapi` 等）不在计数内。
>
> 克隆的仓库源码为各仓库**默认分支 HEAD**（克隆时间见各表）。`crossterm` 与 `tree-sitter` 的默认分支**领先于** crates.io 上的已发布版本，凡涉及"master 与已发布版本不同"之处均已单独标注。本地测量值**不是一手来源声明**，只作量级参考。

---

## 0. 来源清单与来源 ID

后文用 ID 引用；完整 URL 见第 7 节。

### ratatui 本体与官方示例

| ID | 来源 | 用途 |
| --- | --- | --- |
| RT-CRATE | `https://crates.io/api/v1/crates/ratatui` | 版本、发布日期、MSRV、edition、特性表 |
| RT-SUBCRATES | crates.io API：`ratatui-core` / `ratatui-widgets` / `ratatui-crossterm` / `ratatui-termion` / `ratatui-termwiz` / `ratatui-termina` / `ratatui-macros` | 子 crate 版本与特性 |
| RT-CARGO | `ratatui/Cargo.toml`（workspace）+ `ratatui/ratatui/Cargo.toml` | 特性开关的**逐条文档注释**、default 特性、workspace MSRV |
| RT-TERM | `ratatui-core/src/terminal.rs` | `Terminal<B>` / `TerminalOptions` 定义与文档、Inline Viewport 章节 |
| RT-VIEW | `ratatui-core/src/terminal/viewport.rs` | `Viewport` 三个变体的文档与限制 |
| RT-INLINE | `ratatui-core/src/terminal/inline.rs` | `Terminal::insert_before` 的 API 与行为 |
| RT-INIT | `ratatui/ratatui/src/init.rs` | `ratatui::init` / `init_with_options` / `run`、`DefaultTerminal` |
| RT-DOCS-TERM | `https://docs.rs/ratatui-core/0.1.2/ratatui_core/terminal/struct.Terminal.html` | 已发布 0.1.2 的 `Terminal` rustdoc：方法签名、auto trait（`Send` / `Sync`） |
| RT-EX-ASYNC | `ratatui/examples/apps/async-github/src/main.rs` + 同目录 `Cargo.toml` | 官方 tokio 示例 |
| RT-EX-INLINE | `ratatui/examples/apps/inline/src/main.rs` | 官方 inline viewport 示例 |
| RT-README | `ratatui/README.md` | Templates / Alternatives 章节 |
| RT-TEMPLATES | `ratatui/templates` 仓库：`simple-async/template/*`、`event-driven-async/template/*` | 官方异步模板 |
| RT-WEB-BACKENDS | `ratatui-website/src/content/docs/concepts/backends/comparison.md` | 官方的四个后端清单 |
| RT-WEB-EVENTS | `.../concepts/event-handling.md` | 官方对事件循环形态的分类 |
| RT-WEB-RECIPE | `.../recipes/apps/terminal-and-event-handler.md` | 官方 `Tui` + `tokio::spawn` + `EventStream` + channel 方案 |
| RT-WEB-ALT | `.../concepts/backends/alternate-screen.md` | alt screen 的官方说明 |
| RT-EX-INLINE-PAGE | `.../examples/Apps/inline.md` | inline 示例的官方页面 |
| RT-COMMITS | `https://github.com/ratatui/ratatui/commits/HEAD.atom` | 仓库最后提交时间 |

### 后端与替代品

| ID | 来源 | 用途 |
| --- | --- | --- |
| CT-CRATE | `https://crates.io/api/v1/crates/crossterm` | 版本、日期、MSRV、特性表 |
| CT-CARGO | `crossterm/Cargo.toml`（HEAD） | 依赖、特性定义逐条注释 |
| CT-README | `crossterm/README.md` | 特性清单、已测终端列表 |
| CT-CHANGELOG | `crossterm/CHANGELOG.md` | Unreleased 段的 MSRV 提升与 `IsTty` 移除 |
| CT-DOCS-ALL | `https://docs.rs/crossterm/0.29.0/crossterm/all.html` | **已发布 0.29.0** 的完整条目清单（`tty::IsTty`、`style::available_color_count` 等） |
| CT-STYLE | `crossterm/src/style.rs` | `available_color_count()` 的实现语义 |
| CT-STREAM | `crossterm/src/event/stream.rs` | `EventStream` 内部线程与 waker 机制 |
| CT-EX-TOKIO | `crossterm/examples/event-stream-tokio.rs` | 官方 tokio 示例 |
| CT-COMMITS | `https://github.com/crossterm-rs/crossterm/commits/HEAD.atom` | 仓库最后提交时间 |
| TI-CRATE | `https://crates.io/api/v1/crates/termion` | 版本、日期 |
| TI-GITLAB | `https://gitlab.redox-os.org/api/v4/projects/redox-os%2Ftermion[/repository/commits]` | 仓库最后提交 / 最后活动 |
| TW-CRATE | `https://crates.io/api/v1/crates/termwiz` | 版本、日期、特性 |
| WEZ-COMMITS | `https://github.com/wezterm/wezterm/commits/HEAD.atom` | termwiz 所属 monorepo 的最后提交 |
| TM-CRATE | `https://crates.io/api/v1/crates/termina` | 版本、日期、MSRV |
| CU-CRATE | `https://crates.io/api/v1/crates/cursive` | 版本、日期、特性、普通依赖 |
| CU-COMMITS | `https://github.com/gyscos/cursive/commits/HEAD.atom` | 仓库最后提交时间 |
| TUI-CRATE | `https://crates.io/api/v1/crates/tui` | 版本、日期、MSRV、特性 |
| TUI-README | `fdehau/tui-rs/README.md` | **废弃声明原文** |
| TUI-ARCHIVE | `https://github.com/fdehau/tui-rs`（HTML） | 仓库页显示 `Public archive` |
| IO-CRATE | `https://crates.io/api/v1/crates/iocraft` | 版本、日期、特性 |
| IO-COMMITS | `https://github.com/ccbrown/iocraft/commits/HEAD.atom` | 仓库最后提交时间 |

### 语法高亮

| ID | 来源 | 用途 |
| --- | --- | --- |
| SY-CRATE | `https://crates.io/api/v1/crates/syntect` | 版本、日期、特性表、普通依赖 |
| SY-CARGO | `syntect/Cargo.toml` | 特性定义、onig / fancy-regex 可选依赖 |
| SY-README | `syntect/Readme.md` | 语法定义来源、onig 需要 C 库的原文 |
| SY-UTIL | `syntect/src/util.rs` | 终端转义输出 helper |
| SY-EASY | `syntect/src/easy.rs` | `HighlightLines` API |
| SY-COMMITS | `https://github.com/trishume/syntect/commits/HEAD.atom` | 仓库最后提交时间 |
| TSH-CRATE | `https://crates.io/api/v1/crates/tree-sitter-highlight` | 版本、日期、MSRV、普通依赖 |
| TSH-DOCS | `https://docs.rs/tree-sitter-highlight/0.27.0/tree_sitter_highlight/` | 0.27.0 的 crate 文档、条目清单、用法示例 |
| TSH-CARGO | `tree-sitter/crates/highlight/Cargo.toml` | 依赖声明（workspace 继承） |
| TS-CARGO | `tree-sitter/Cargo.toml` | workspace 版本 / MSRV（HEAD = 0.28.0） |
| TS-DOC-HL | `tree-sitter/docs/src/3-syntax-highlighting.md` | 官方高亮系统说明 |
| BAT-CRATE | `https://crates.io/api/v1/crates/bat` | 版本、日期、MSRV、特性 |
| BAT-README | `sharkdp/bat/README.md` | "作为库使用"的官方说明与特性要求 |
| BAT-CARGO | `https://crates.io/api/v1/crates/bat/0.26.1/dependencies` | `bat` 的普通依赖清单 |
| TF-CRATE | `https://crates.io/api/v1/crates/two-face` | syntect 额外语法/主题包 |

### 终端能力探测

| ID | 来源 | 用途 |
| --- | --- | --- |
| STD-ISTERM | `https://doc.rust-lang.org/std/io/trait.IsTerminal.html` | TTY 判定的 std 官方 API |
| AQ-DOCS | `https://docs.rs/anstyle-query/1.1.5/anstyle_query/` | 颜色能力查询函数清单 |
| AS-CRATE | `https://crates.io/api/v1/crates/anstream` | 颜色降级 IO 适配器 |
| TC-CRATE | `https://crates.io/api/v1/crates/terminal-colorsaurus` | 终端前景/背景色探测（`bat` 的依赖） |
| SC-CRATE | `https://crates.io/api/v1/crates/supports-color` | 第三方颜色支持探测 |

---

## 1. TUI 栈现状

### 1.1 `ratatui` 本体

| 项 | 值 | 来源 |
| --- | --- | --- |
| 最新稳定版 | `0.30.2` | RT-CRATE |
| 发布日期 | 2026-06-19 | RT-CRATE |
| `rust-version`（MSRV） | `1.88.0` | RT-CRATE, RT-CARGO（`[workspace.package] rust-version = "1.88.0"`） |
| edition | `2024` | RT-CRATE, RT-CARGO |
| license | MIT | RT-CRATE |
| 仓库最后提交 | 2026-09-10T20:41:19Z | RT-COMMITS |
| 上一线版本 | `0.30.1`（2026-06-05，MSRV 1.88.0）；`0.30.0`（2025-12-26，MSRV **1.86.0**） | RT-CRATE |
| default 特性 | `["all-widgets", "crossterm", "layout-cache", "macros", "underline-color"]` | RT-CARGO, RT-CRATE |
| `crossterm` 是否默认后端 | **是**（default 特性内含 `crossterm`） | RT-CARGO |
| 0.30 起的多 crate 拆分 | `ratatui-core 0.1.2` / `ratatui-widgets 0.3.2` / `ratatui-crossterm 0.1.2` / `ratatui-termion 0.1.2` / `ratatui-termwiz 0.1.2` / `ratatui-termina 0.1.0` / `ratatui-macros 0.7.2`（全部 2026-06-19 发布，MSRV 1.88.0） | RT-SUBCRATES |
| 依赖闭包（默认特性） | **70 包**（本文实测） | 本文实测 |

本票点名的 `rust-version` 与发布日期均有 crates.io 元数据直接支撑；仓库最后提交取 atom feed。

### 1.2 后端（官方四个）

`RT-WEB-BACKENDS` 原文列出的后端只有四个：

> As of now, `ratatui` supports four backends: Crossterm / Termion / Termwiz / Termina

`RT-CARGO` 对特性开关的原文说明：

> Generally an application will only use one backend, so you should only enable one of the following features:

| 后端特性名 | 官方注释（逐字） | 后端 crate | 该后端单独引入的包数（本文实测，默认特性） |
| --- | --- | --- | --- |
| `crossterm` | `## enables the [CrosstermBackend] backend and adds a dependency on [crossterm].` | `ratatui-crossterm` | `crossterm 0.29.0` = **27 包** |
| `termion` | `## enables the [TermionBackend] backend and adds a dependency on [termion].` | `ratatui-termion` | `termion 4.0.6` = **3 包** |
| `termwiz` | `## enables the [TermwizBackend] backend and adds a dependency on [termwiz].` | `ratatui-termwiz` | `termwiz 0.23.3` = **85 包** |
| `termina` | `## enables the [TerminaBackend] backend and adds a dependency on [termina].` | `ratatui-termina` | （`termina` 由本票的 ratatui+termina 探针间接覆盖） |

同一后端还有两个**版本选择**特性（`RT-CARGO`）：

- `crossterm_0_28`：`## selects the crossterm 0.28.x backend implementation`
- `crossterm_0_29`：`## selects the crossterm 0.29.x backend implementation (default)`

即：`ratatui-crossterm 0.1.2` 的 `default = ["crossterm_0_29", "underline-color"]`，两个 crossterm 版本以重命名依赖（`crossterm_0_28` / `crossterm_0_29`）并存，默认只启用其一（RT-SUBCRATES）。

#### 各后端事实

| 项 | `crossterm` | `termion` | `termwiz` |
| --- | --- | --- | --- |
| 最新稳定版 | `0.29.0` | `4.0.6` | `0.23.3` |
| 发布日期 | 2025-04-05 | 2025-11-21 | 2025-03-20 |
| crates.io 声明的 `rust-version` | `1.63.0` | 未声明（`null`） | 未声明（`null`） |
| edition（crates.io） | 2021 | 未声明 | 2018 |
| 特性（crates.io 元数据） | `default = ["bracketed-paste", "events", "windows", "derive-more"]`；可选 `event-stream`、`serde`、`use-dev-tty`、`osc52`、`document-features` | 无特性 | `widgets`、`use_image`、`use_serde`、`docs` |
| 仓库 / 最后提交 | github.com/crossterm-rs/crossterm，2026-08-21T00:06:18Z | gitlab.redox-os.org/redox-os/termion，2025-11-21 | wezterm/wezterm（monorepo），2026-09-12T11:38:23Z |
| 官方维护状态声明 | ⚪ 未证实（仓库/文档未见声明；只有发布日期与提交时间事实） | ⚪ 未证实 | ⚪ 未证实 |

`crossterm` 的 `event-stream` 特性定义（`CT-CARGO`，逐字）：

```toml
## Enables the [EventStream](event::EventStream) struct for async event reading.
event-stream = ["dep:futures-core", "events"]
```

`events` 特性：`events = ["dep:mio", "dep:signal-hook", "dep:signal-hook-mio"]`；`use-dev-tty` 特性：`use-dev-tty = ["filedescriptor", "rustix/process"]`（CT-CARGO）。

**注意 master 与已发布版本的差异**（`CT-CHANGELOG` 的 `# Unreleased` 段，逐字）：

> - Raise the minimum supported Rust version from 1.63 to 1.85.
> - Remove `IsTty` trait.
>   Use the standard library's [`std::io::IsTerminal`](https://doc.rust-lang.org/std/io/trait.IsTerminal.html) trait instead,
>   which provides equivalent functionality.
> - Migrate the crate to the Rust 2024 edition. This does not raise the MSRV beyond Rust 1.85.

因此：crates.io 上**已发布的 0.29.0** 仍是 edition 2021 / MSRV 1.63.0，且**仍含 `tty::IsTty`**（`CT-DOCS-ALL` 的 0.29.0 条目清单里存在 `tty::IsTty`）；默认分支（未发布）已提 MSRV 至 1.85 并删除该 trait。该改动的**发布版本号未在一手来源中给出** → ⚪。

### 1.3 替代品：`tui`、`cursive`、`iocraft`

| 项 | `tui`（tui-rs） | `cursive` | `iocraft` |
| --- | --- | --- | --- |
| 最新稳定版 | `0.19.0`（2022-08-14） | `0.21.1`（2024-08-03） | `0.9.1`（2026-09-04） |
| MSRV | `1.56.1`（crates.io） | 未声明 | 未声明 |
| 仓库最后提交 | 2023-08-06T07:35:02Z | 2026-09-09T14:00:30Z | 2026-09-07T18:45:10-04:00 |
| 仓库页 `Public archive` | **是**（TUI-ARCHIVE） | 未出现该标记 | 未查 |
| 废弃/维护的一手依据 | `TUI-README` 首行逐字：`⚠️ **August 2023: This crate is no longer maintained. See https://github.com/ratatui-org/ratatui for an actively maintained fork.** ⚠️` | 无官方维护声明 → ⚪（仅有发布/提交时间事实） | 无官方维护声明 → ⚪ |
| default 特性 | `["crossterm"]` | `["crossterm-backend"]` | `["crossterm"]` |
| 后端特性名 | `crossterm`（默认）、`termion`、`curses` | `crossterm-backend`（默认）、`termion-backend`、`ncurses-backend`、`pancurses-backend`、`blt-backend` | `crossterm` |
| 依赖闭包（默认特性） | **19 包**（内含 `crossterm 0.25.0`） | **62 包**（内含 `crossterm 0.28.1`） | 未测 |
| 是否被 ratatui 官方列为替代品 | 否 | **是**（RT-README：`- [Cursive](...) - a ncurses-based TUI library.`） | **是**（RT-README：`- [iocraft](...) - a declarative TUI library.`） |

`ratatui` README 的 Alternatives 章节原文只列了 **Cursive 与 iocraft** 两个（RT-README）。

`tui` crate 的 README 同时说明它自身不含事件系统（TUI-README）：

> Moreover, the library does not provide any input handling nor any event system

### 1.4 依赖闭包实测（本文实测，汇总）

命令（每个探针 crate 各跑一次）：`cargo tree -e normal -q --prefix none`，去重后统计包数（不含 root / dev / build 边）。

| 探针（`/tmp` 下 `Cargo.toml`） | 包数 |
| --- | --- |
| `ratatui = "0.30.2"`（默认特性） | **70** |
| `ratatui = { version = "0.30.2", default-features = false }` | **41** |
| `ratatui` + `features = ["crossterm"]`（其余默认关） | **62** |
| `ratatui` + `features = ["termion"]` | **45** |
| `ratatui` + `features = ["termwiz"]` | **118** |
| `ratatui` + `features = ["termina"]` | **54** |
| `crossterm = "0.29.0"` | **27** |
| `termion = "4.0.6"` | **3** |
| `termwiz = "0.23.3"` | **85** |
| `ratatui-crossterm = "0.1"` | **59** |
| `ratatui-termion = "0.1"` | **42** |
| `ratatui-termwiz = "0.1"` | **109** |
| `cursive = "0.21.1"`（默认 = crossterm-backend） | **62** |
| `tui = "0.19.0"` | **19** |
| `syntect = "5.3.0"`（默认 = `default-onig`） | **42** |
| `syntect` `default-features = false, features = ["default-fancy"]` | **44** |
| `tree-sitter-highlight = "0.27.0"` | **15** |
| `bat = "0.26.1"`（默认） | **157** |
| `bat = { version = "0.26.1", default-features = false }` | **64** |

解析到的关键包版本（本文实测）：`crossterm 0.29.0`、`termion 4.0.6`、`termwiz 0.23.3`、`termina 0.3.3`（`ratatui-termina 0.1.0` 解析到 `termina` 的 **0.3.x**，而 crates.io 上 `termina` 最新为 `0.4.0`，2026-08-31）、`cursive_core 0.4.7` + `crossterm 0.28.1`、`tui` 内含 `crossterm 0.25.0`、`onig 6.5.3` + `onig_sys 69.9.3`、`fancy-regex 0.16.2`、`tree-sitter 0.27.0` + `tree-sitter-language 0.1.8`、`nix 0.29.0` + `terminfo 0.9.0`（termwiz）。

`ratatui` 默认特性比 `crossterm`-only 多出的 8 包主要来自 `macros`（`darling*`）、`layout-cache`（`critical-section`）、`all-widgets`（`time`、`deranged`、`num-conv`、`powerfmt`）、`underline-color` 等；逐包清单见实测输出（本文件不逐条罗列）。

---

## 2. 与异步运行时的配合（官方文档 / 官方示例）

### 2.1 crossterm 的官方 tokio 示例

`crossterm` 仓库有**官方 tokio 示例** `examples/event-stream-tokio.rs`，`Cargo.toml` 中声明 `required-features = ["event-stream", "events"]`（CT-CARGO）。示例的形状（CT-EX-TOKIO）：

```rust
use futures::{StreamExt, future::FutureExt, select};
use crossterm::event::EventStream;

async fn print_events() {
    let mut reader = EventStream::new();
    loop {
        let mut delay = Delay::new(Duration::from_millis(1_000)).fuse();
        let mut event = reader.next().fuse();
        select! { _ = delay => ..., maybe_event = event => ... };
    }
}
```

即官方示例的形态是 **`EventStream`（`futures_core::Stream`）+ `select!` 与定时器合并**，而不是独立线程 + channel（对使用者而言）。

**但 `EventStream` 内部确实另起线程**（CT-STREAM，`impl Default for EventStream`）：

```rust
let (task_sender, receiver) = mpsc::sync_channel::<Task>(1);
thread::spawn(move || { ... internal::poll(None, &EventFilter) ... });
```

`EventStream` 的 rustdoc（CT-STREAM）逐字：

> **This type is not available by default. You have to use the `event-stream` feature flag to make it available.**

### 2.2 ratatui 官方 tokio 示例（`examples/apps/async-github`）

依赖（RT-EX-ASYNC，同目录 `Cargo.toml`）：

```toml
crossterm = { workspace = true, features = ["event-stream"] }
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
tokio-stream.workspace = true
```

主循环形状（RT-EX-ASYNC，逐字节选）：

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let terminal = ratatui::init();
    let app_result = App::default().run(terminal).await;
    ratatui::restore();
    app_result
}

pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
    let mut interval = tokio::time::interval(period);   // FRAMES_PER_SECOND = 60.0
    let mut events = EventStream::new();
    while !self.should_quit {
        tokio::select! {
            _ = interval.tick() => { terminal.draw(|frame| self.render(frame))?; },
            Some(Ok(event)) = events.next() => self.handle_event(&event),
        }
    }
}
```

即**在异步任务里 `draw`**（`draw` 与事件读取在同一个 `tokio::select!` 里、同一个 task 内），并且用 **tick 定时器驱动重绘**。示例文档注释（RT-EX-ASYNC，逐字）明确划出边界：

> This example does not cover message passing between threads, it only demonstrates how to manage shared state between the main thread and a background task, which acts mostly as a one-shot fetcher. For more complex scenarios, you may need to use channels or other synchronization primitives.
>
> ... The main thread would then render the widgets with the latest data.

### 2.3 官方模板（`ratatui/templates`）

`ratatui/README.md` 的 Templates 章节（RT-README，逐字）：

> ```shell
> cargo generate ratatui/templates
> ```

仓库内与异步相关的模板为 `simple-async` 与 `event-driven-async`（RT-TEMPLATES）。`simple-async` 的依赖与主循环（RT-TEMPLATES，逐字）：

```toml
crossterm = { version = "0.29.0", features = ["event-stream"] }
futures = "0.3.32"
ratatui = "0.30.2"
tokio = { version = "1.52.3", features = ["full"] }
```

```rust
#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    let terminal = ratatui::init();
    let result = App::new().run(terminal).await;
    ratatui::restore();
    result
}

pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
    self.running = true;
    while self.running {
        terminal.draw(|frame| self.draw(frame))?;
        self.handle_crossterm_events().await?;   // EventStream::next().fuse().await
    }
}
```

### 2.4 官方网站的 `Tui` + `tokio::spawn` + channel 方案

`RT-WEB-RECIPE` 给出的可复制方案是**独立 `tokio::spawn` 任务读事件 + `tokio::sync::mpsc` 通道回传**（逐字）：

> - with key event `EventHandler` with `crossterm`'s `EventStream` support
> - and with `tokio`'s `select!`

```rust
pub struct Tui {
  pub terminal: ratatui::Terminal<Backend<std::io::Stderr>>,
  pub task: JoinHandle<()>,
  pub cancellation_token: CancellationToken,
  pub event_rx: UnboundedReceiver<Event>,
  pub event_tx: UnboundedSender<Event>,
  pub frame_rate: f64,
  pub tick_rate: f64,
  ...
}
...
    self.task = tokio::spawn(async move {
      let mut reader = crossterm::event::EventStream::new();
      let mut tick_interval = tokio::time::interval(tick_delay);
      let mut render_interval = tokio::time::interval(render_delay);
      ...
        tokio::select! { ... }
```

该页面同时指向 `ratatui/crates-tui` 仓库的 `src/tui.rs` / `src/events.rs`。

### 2.5 事件循环的"官方标准模式"

- `RT-WEB-EVENTS` 明确说明 ratatui 自身不做事件捕获，逐字：
  > Mostly because `ratatui` does not directly expose any event catching; the programmer will depend on the chosen backend's library.
  
  该页把形态分为三类：**Centralized event handling**（直接 `event::read()?` 后 match）、**Centralized catching, message passing**（一处 poll、再分发）、**Distributed event loops/segmented applications**（把 `Terminal` 与主循环交给子模块）。
- `RT-EX-ASYNC`（0.30.2 官方示例）：**`tokio::time::interval` tick + `EventStream`**。
- `RT-INIT`（`ratatui::init` 模块文档，逐字）给出的入门顺序是 `ratatui::run` → `run_with_options` → `init` / `restore` → `try_init` / `try_restore` → `init_with_options` → 手工 `Terminal`。
- `RT-TERM` 的初始化章节（逐字）：
  > `ratatui::run`: recommended for most applications. Provides a `ratatui::DefaultTerminal`, runs your closure, and restores terminal state on exit and on panic.
  > `ratatui::init` + `ratatui::restore`: like `run`, but you control the event loop and decide when to restore.
  
  并注明 `ratatui::run` 是 **0.30 引入**的。

### 2.6 终端后端是否需要独占一个线程

**⚪ 未证实**：在本次核查的 ratatui 官方文档（`ratatui.rs` 概念页 / 示例 / rustdoc）与 crossterm 官方文档中，**没有**"终端后端必须独占一个线程"（或"必须从单一线程访问"）的**声明**。可报告的一手事实只有：

| 事实 | 来源 |
| --- | --- |
| `Terminal::draw` 的签名为 `pub fn draw<F>(&mut self, render_callback: F) -> Result<CompletedFrame<'_>, B::Error>`——需要 `&mut self` | RT-DOCS-TERM, RT-INIT（源码 `render.rs:81`） |
| `insert_before`：`pub fn insert_before<F>(&mut self, height: u16, draw_fn: F) -> Result<(), B::Error>` | RT-DOCS-TERM |
| `Terminal<B>` 以值持有后端：`pub struct Terminal<B> where B: Backend { backend: B, buffers: [Buffer; 2], ... }` | RT-DOCS-TERM, RT-TERM |
| docs.rs 0.1.2 的 auto trait 列表包含 `Send` 与 `Sync`（`impl Send for Terminal<B>` / `impl Sync for Terminal<B>`，按 rustdoc 呈现） | RT-DOCS-TERM |
| 官方两处异步用法都在**同一个任务**里 `draw`（示例、模板各一处）；官方网站的独立任务方案只把**事件读取**放进 spawn，`Terminal` 留在主循环 | RT-EX-ASYNC, RT-TEMPLATES, RT-WEB-RECIPE |
| `ratatui::DefaultTerminal = Terminal<CrosstermBackend<Stdout>>` | RT-INIT |
| 官方 inline 示例用 `std::sync::mpsc` + `std::thread` 做后台 worker，事件与 draw 仍在主循环 | RT-EX-INLINE |
| `Terminal::flush` 不感知显示面切换（如离开 alt screen），可能把旧 diff 重放到新显示面 → 警告原文见 RT-DOCS-TERM | RT-DOCS-TERM |

"是否推荐用独立线程 + channel"的**推荐性声明不存在** → ⚪。

---

## 3. Inline viewport（非 alt-screen、在光标处渲染固定高度区域）

**官方有该能力。** API 名与官方文档：

| API | 形态 | 来源 |
| --- | --- | --- |
| `Viewport::Inline(u16)` | `enum Viewport` 的变体；高度以**行**为单位 | RT-VIEW, RT-DOCS-TERM |
| `Viewport::Fullscreen` / `Viewport::Fixed(Rect)` | 同 enum 的另两个变体（`Fullscreen` 为 `#[default]`） | RT-VIEW |
| `TerminalOptions { viewport }` | 传给 `Terminal::with_options` | RT-TERM, RT-DOCS-TERM |
| `Terminal::with_options(backend, options)` | 构造带 viewport 的 `Terminal` | RT-DOCS-TERM |
| `ratatui::init_with_options(TerminalOptions)` / `ratatui::try_init_with_options` | 官方 init helper 的 inline 版本 | RT-INIT, RT-TERM |
| `Terminal::insert_before(height, draw_fn)` | 在 inline viewport **上方**插入内容（`height` 行的 `Buffer`） | RT-DOCS-TERM, RT-INLINE |
| `Frame::area()` | inline 模式下**可能非 (0,0) 原点**，官方要求以它作为布局根矩形 | RT-DOCS-TERM |
| `scrolling-regions` 特性 | 让 `insert_before` 不闪屏 | RT-CARGO, RT-DOCS-TERM |

官方限制说明（RT-VIEW / RT-DOCS-TERM / RT-TERM，全部逐字或直接转述）：

1. **宽度固定**：`Inline viewports always span the full terminal width.`
2. **锚点**：`Ratatui anchors the viewport to the backend cursor row and always starts drawing at column 0.`（`Terminal::with_options` 文档补充：在**初始化时**锚定到当前光标行）
3. **高度钳制**：`The height is specified in rows and is clamped to the current terminal height.`
4. **可能触发滚动**：`To reserve vertical space for the requested height, Ratatui may append lines. When the cursor is near the bottom edge, terminals scroll; Ratatui accounts for that scrolling by shifting the computed viewport origin upward so the viewport stays fully visible.`
5. **resize 行为**：`Fullscreen` 与 `Inline` 在 `draw` / `try_draw` 期间**自动**重新计算；`Fixed` 不会（需显式 `Terminal::resize`）。
6. **`insert_before` 的两条路径**（RT-DOCS-TERM，逐字）：
   > When Ratatui is built with the `scrolling-regions` feature, this can be done without clearing and redrawing the viewport. Without `scrolling-regions`, Ratatui falls back to a more portable approach and clears the viewport so the next `Terminal::draw` / `Terminal::try_draw` repaints it.
7. **`insert_before` 在非 inline viewport 下无效**：`This has no effect when the viewport is not inline.`（RT-DOCS-TERM；源码中的对应测试注释：`Viewport is fullscreen (not inline), so insert_before() is a no-op.`）
8. **`insert_before` 的推移语义**（RT-DOCS-TERM，逐字）：`If the viewport isn't yet at the bottom of the screen, inserted lines will push it towards the bottom. Once the viewport is at the bottom of the screen, inserted lines will scroll the area of the screen above the viewport upwards.`
9. **`Terminal::clear` 在 inline 下的行为**（RT-DOCS-TERM，逐字）：`clears after the viewport's origin, leaving any content above the viewport untouched`，且当前实现是 `clearing runs from the viewport origin through the end of the visible display area, not just the viewport's rectangle. This is an implementation detail rather than a contract; do not rely on it.`

**与 alt screen 的关系**（RT-INIT，逐字）：`init` → `Creates a terminal with reasonable defaults including alternate screen and raw mode`；`init_with_options` / `run_with_options` → `Enables raw mode but not alternate screen`。

**官方 inline 示例**：`examples/apps/inline`（RT-EX-INLINE），入口为

```rust
let mut terminal = ratatui::init_with_options(TerminalOptions {
    viewport: Viewport::Inline(8),
});
```

并用 `std::sync::mpsc` + `std::thread` 跑后台 worker、主循环里 `event` 轮询。

**⚪ 缺口**：官方是否声明 inline viewport 在哪些终端/终端模拟器上**不可用**——本次核查的 ratatui 官方文档未见任何终端兼容性清单 → ⚪。

---

## 4. 语法高亮的候选

### 4.1 `syntect`

| 项 | 值 | 来源 |
| --- | --- | --- |
| 最新稳定版 / 发布日期 | `5.3.0` / 2025-09-27 | SY-CRATE |
| MSRV | 未声明（crates.io `rust_version = null`）；edition 2021 | SY-CRATE |
| 仓库最后提交 | 2026-04-28T19:52:59Z | SY-COMMITS |
| 默认特性 | `default = ["default-onig"]`，即**默认走 onig** | SY-CRATE, SY-CARGO |
| 纯 Rust 特性组 | `default-fancy = ["parsing","default-syntaxes","default-themes","html","plist-load","yaml-load","dump-load","dump-create","regex-fancy"]` | SY-CRATE, SY-CARGO |
| 正则引擎二选一 | `regex-onig = ["dep:onig"]` / `regex-fancy = ["dep:fancy-regex"]` | SY-CARGO |
| onig 依赖 | `onig = { version = "6.5.2", optional = true, default-features = false }` | SY-CARGO |
| fancy-regex 依赖 | `fancy-regex = { version = "0.18.0", optional = true }`（crates.io 上 5.3.0 声明为 `^0.16.2`；本文探针解析到 `fancy-regex 0.16.2`） | SY-CARGO, SY-CRATE |
| 是否依赖 `onig` | **默认依赖**；可用 `default-fancy` / `regex-fancy` 换成纯 Rust | SY-CRATE, SY-CARGO |
| 依赖闭包（默认 onig） | **42 包**（含 `onig 6.5.3` + `onig_sys 69.9.3`） | 本文实测 |
| 依赖闭包（`default-fancy`） | **44 包**（含 `fancy-regex 0.16.2`，不含 onig） | 本文实测 |
| 语法定义来源 | Sublime Text 语法定义：`syntect is a syntax highlighting library for Rust that uses Sublime Text syntax definitions` | SY-README |
| 默认语法集 | 特性 `default-syntaxes`（注释：`Enables inclusion of the default syntax packages.`）+ `default-themes`（`Enables inclusion of the default theme packages.`）；`ls assets/` 得到 4 个 dump 文件（`default_newlines.packdump`、`default_nonewlines.packdump`、`default_metadata.packdump`、`default.themedump`） | SY-CARGO, 本文实测（仓库文件清单） |
| onig 需要 C 工具链 | `The advantage of fancy-regex is that it does not require the onig crate which requires building and linking the Oniguruma C library. Many users experience difficulty building the onig crate, especially on Windows and Webassembly.` | SY-README |
| onig_sys 的构建方式 | `onig_sys/build.rs` 使用 `cc::Build::new()` 编译 `oniguruma/src` 下的 C 源；支持 `RUSTONIG_SYSTEM_LIBONIG` / pkg-config 走系统库 | 本文实测（`rust-onig` 仓库 `onig_sys/build.rs`） |
| 终端输出 helper | `pub fn as_24_bit_terminal_escaped(v: &[(Style, &str)], bg: bool) -> String`；`pub fn as_latex_escaped(...)`；模块注释：`Convenient helper functions for common use cases: * Printing to terminal ...` | SY-UTIL |
| 逐行高亮 API | `pub struct HighlightLines<'a>`、`HighlightLines::new(syntax, theme)`、`highlight_line(...)` | SY-EASY |
| `util` / `easy` / `html` 模块均存在 | `pub mod easy;` `pub mod html;` `pub mod util;` | SY-CARGO（`src/lib.rs`） |

**⚪**：`syntect` 自带语法定义的**条数/语言清单**在一手来源中未见明确数字（只说明来自 Sublime Text 默认包集；`Readme.md` 只有耗时描述 `~138ms to load and link all the syntax definitions in the default Sublime package set.`）→ ⚪。

### 4.2 `tree-sitter-highlight`

| 项 | 值 | 来源 |
| --- | --- | --- |
| 最新稳定版 / 发布日期 | `0.27.0` / 2026-08-30 | TSH-CRATE |
| MSRV / edition | `1.90` / 2024 | TSH-CRATE |
| 普通依赖 | `regex ^1.12.3`、`streaming-iterator ^0.1.9`、`thiserror ^2.0.18`、**`tree-sitter ^0.27.0`** | TSH-CRATE, TSH-DOCS |
| 与 `tree-sitter` 运行时的版本关系 | 同一 monorepo、同一版本序列：`tree-sitter-highlight 0.27.0` 要求 `tree-sitter ^0.27.0`；仓库 workspace 版本（HEAD，未发布）已为 `0.28.0`，MSRV 1.90 | TSH-CRATE, TSH-CARGO（`tree-sitter.workspace = true`）, TS-CARGO |
| 依赖闭包 | **15 包**（含 `tree-sitter 0.27.0`、`tree-sitter-language 0.1.8`） | 本文实测 |
| crate-type | `crate-type = ["lib", "staticlib"]`，`[lib] path = "src/highlight.rs"` | TSH-CARGO |
| 公开条目（0.27.0 rustdoc） | `Highlight`、`HighlightConfiguration`、`Highlighter`、`HtmlRenderer`、`HighlightEvent`、`Error`、`_QueryCaptures`；Re-export：`pub use c_lib as c;` | TSH-DOCS |
| 用法要点（官方文档） | `Highlighter::new()`；`HighlightConfiguration::new(language, name, HIGHLIGHT_QUERY, INJECTIONS_QUERY, LOCALS_QUERY)`；`config.configure(&highlight_names)`；`highlighter.highlight(&config, bytes, None, |_| None)` 返回 `HighlightEvent` 迭代器（`Source{start,end}` / `HighlightStart(s)` / `HighlightEnd`） | TSH-DOCS |
| 官方原文（并发） | `You need one of these for each thread that you're using for syntax highlighting:`（`Highlighter` 每线程一个） | TSH-DOCS |
| 官方定位 | `Tree-sitter has built-in support for syntax highlighting via the tree-sitter-highlight library, which is now used on GitHub.com for highlighting code written in several languages.` | TS-DOC-HL |
| 高亮配置数据来源 | 语法仓库的 `tree-sitter.json` 与 `queries/` 目录（`highlights.scm` 等）；`tree-sitter-highlight` 库用内存对象传入 | TS-DOC-HL, TSH-DOCS |
| 终端/ANSI 渲染器 | 0.27.0 的条目清单中**只有 `HtmlRenderer`**（HTML 输出），无终端/ANSI 渲染器条目（这是条目清单事实，非官方"不做"声明） | TSH-DOCS |

票 23 的产物 `research/04` 已测过 `tree-sitter`（0.27.0）与 `tree-sitter-rust` 的闭包与解析耗时；本表只补 highlight 这一层：**`tree-sitter-highlight 0.27.0` 的闭包 15 包，其中 `tree-sitter` 本体 8 包**（对照 `research/04` 的数字）。

### 4.3 `bat` 作为库

| 项 | 值 | 来源 |
| --- | --- | --- |
| 最新稳定版 / 发布日期 | `0.26.1` / 2025-12-02 | BAT-CRATE |
| MSRV | `1.87` | BAT-CRATE |
| default 特性 | `["application", "git"]`（`application = ["bugreport","build-assets","minimal-application"]`） | BAT-CRATE |
| 与高亮相关的特性 | `regex-onig = ["syntect/regex-onig"]`、`regex-fancy = ["syntect/regex-fancy"]`；对 `syntect` 的依赖为 `syntect ^5.3.0, default-features = false, features = ["parsing"]` | BAT-CRATE, BAT-CARGO |
| 官方"作为库"的说明 | `If you want to build an application that uses bat's pretty-printing features as a library, check out the API documentation. Note that you have to use either regex-onig or regex-fancy as a feature when you depend on bat as a library.` | BAT-README |
| 官方库文档 | `https://docs.rs/bat/` | BAT-README |
| 依赖闭包（默认） | **157 包** | 本文实测 |
| 依赖闭包（`default-features = false`） | **64 包** | 本文实测 |
| 与终端能力探测相关 | `bat` 的普通依赖含 `terminal-colorsaurus ^1.0`、`ansi_colours`、`console`；`application` 特性引入 `anstream` / `anstyle-query` 等 | BAT-CARGO, 本文实测 |

### 4.4 `two-face`（syntect 的额外语法/主题包）

| 项 | 值 | 来源 |
| --- | --- | --- |
| 最新版 | `0.5.2+bat-0.26.1`（2026-08-07） | TF-CRATE |
| MSRV | `1.79.0` | TF-CRATE |
| 定位 | `Extra syntax and theme definitions for syntect` | TF-CRATE |
| 特性 | `default = ["syntect-onig"]`；`syntect-fancy`、`syntect-onig`、`syntect-default-fancy`、`syntect-default-onig` | TF-CRATE |
| 仓库 | codeberg.org/CosmicHarper/two-face | TF-CRATE |

**其它候选**：本次核查范围内，ratatui README 的 Alternatives 只涉及 TUI（非高亮）；`bat` / `two-face` 之外的独立高亮库**未逐一体检** → 见第 6 节第 6 条。

---

## 5. 终端能力探测

### 5.1 当前 stdout 是否是 TTY

| API | 事实 | 来源 |
| --- | --- | --- |
| `std::io::IsTerminal` | `pub trait IsTerminal { fn is_terminal(&self) -> bool; }`，**1.70.0** 起稳定；`Trait to determine if a descriptor/handle refers to a terminal/tty.` | STD-ISTERM |
| 实现者 | `Stdin` / `Stdout` / `Stderr` / 各自的 `Lock` / `File`；Unix 侧 `BorrowedFd` / `OwnedFd`，Windows 侧 `BorrowedHandle` / `OwnedHandle` | STD-ISTERM |
| 返回值语义（逐字） | `On platforms where Rust does not know how to detect a terminal yet, this will return false. This will also return false if an unexpected error occurred, such as from passing an invalid file descriptor.` | STD-ISTERM |
| Windows 特别行为（逐字） | `On Windows, in addition to detecting consoles, this currently uses some heuristics to detect older msys/cygwin/mingw pseudo-terminals based on device name: devices with names starting with msys- or cygwin- and ending in -pty will be considered terminals.` | STD-ISTERM |
| `crossterm 0.29.0` 的等价物 | `tty::IsTty` 在 0.29.0 的条目清单中**存在**（`docs.rs/crossterm/0.29.0/crossterm/all.html` → Traits: `tty::IsTty`） | CT-DOCS-ALL |
| 未发布版本的替代 | `Remove IsTty trait. Use the standard library's std::io::IsTerminal trait instead, which provides equivalent functionality.` | CT-CHANGELOG |
| `IsTty` 在 0.29.0 的实现细节 | ⚪ 未证实（未读 0.29.0 的 `tty` 模块源码，只确认 trait 存在） | — |

### 5.2 真彩色能力

| API | 事实 | 来源 |
| --- | --- | --- |
| `crossterm::style::available_color_count() -> u16` | 0.29.0 中存在（条目清单） | CT-DOCS-ALL |
| 其实现语义（`CT-STYLE`，HEAD 源码） | Windows 下若 `ansi_support::supports_ansi()` 为真 → 直接 `u16::MAX`；否则读 `COLORTERM`，读不到再读 `TERM`；`DEFAULT = 8`；值含 `"24bit"` 或 `"truecolor"` → `u16::MAX`；含 `"256"` → `256`；否则 `8` | CT-STYLE |
| `crossterm::style::force_color_output(enabled: bool)` | 存在；文档：`crossterm supports NO_COLOR (<https://no-color.org/>) to disabled colored output. This API allows applications to override that behavior` | CT-DOCS-ALL, CT-STYLE |
| `anstyle-query` 1.1.5 的函数 | `clicolor`、`clicolor_force`、`is_ci`、`no_color`、`term_supports_ansi_color`、`term_supports_color`、`truecolor`（`Check COLORTERM for truecolor support`） | AQ-DOCS |
| `anstyle-query` 是否含 TTY 判定 | 其模块清单中**没有** `is_terminal` 类函数（TTY 判定由 `anstream` 等调用方负责） | AQ-DOCS |
| `anstream` 1.0.0 | `IO stream adapters for writing colored text that will gracefully degrade according to your terminal's capabilities.`；`default = ["auto", "wincon"]`，`auto = ["dep:anstyle-query"]` | AS-CRATE |
| `terminal-colorsaurus` 1.0.3 | `A cross-platform library for determining the terminal's background and foreground color. It answers the question «Is this terminal dark or light?»`；被 `bat 0.26.1` 作为普通依赖使用 | TC-CRATE, BAT-CARGO |
| `supports-color` 3.0.2 | `Detects whether a terminal supports color, and gives details about that support.`；最后发布 2024-11-26 | SC-CRATE |

### 5.3 alt screen 支持探测

| 事实 | 来源 |
| --- | --- |
| `crossterm::terminal::EnterAlternateScreen` / `LeaveAlternateScreen` 命令存在（0.29.0 条目清单 Structs） | CT-DOCS-ALL |
| crossterm README 特性清单包含 `Alternate screen`（`Terminal` 小节） | CT-README |
| `ratatui::init` 会进入 alternate screen + raw mode；`init_with_options` / `run_with_options` 只开 raw mode、**不开** alternate screen | RT-INIT |
| 官方 ratatui 概念页给出 alt screen 的手工用法（`EnterAlternateScreen` / `LeaveAlternateScreen`）与效果说明 | RT-WEB-ALT |
| **是否存在"探测终端是否支持 alt screen"的 API**：crossterm 0.29.0 的条目清单（Structs / Enums / Traits / Functions）中无此类函数；ratatui 亦无 | ⚪ 未证实（在本次核查范围内不存在，未见官方"不支持探测"的声明） |
| crossterm README 列出"已测终端"清单（Console Host、Windows Terminal、GNOME Terminal、Konsole、Kitty、Linux Mint、Alacritty、Crostini、macOS Monterey/Sonoma），并称 `This crate supports all UNIX terminals and Windows terminals down to Windows 7; however, not all of the terminals have been tested.` | CT-README |

---

## 6. ⚪ 无法从主来源验证的清单

以下条目**一手来源没有写明**，本文不推断、不作为事实使用：

1. **"终端后端必须独占一个线程"** —— ratatui 官方文档（rustdoc / 官网 / 示例 / 模板）与 crossterm 官方文档中**均无**此声明；第 2.6 节列出的是"`&mut self` 借用模型 + 官方用法示例"等事实，不足以证实该要求本身。
2. **"官方推荐用独立线程 + channel 驱动 TUI"** —— 无推荐性声明；官方不同来源给出的形态并不唯一（同任务 `select!`、spawn + mpsc、poll/read 循环、三类事件循环分类），本文不作取舍。
3. **alt screen 支持探测** —— crossterm 0.29.0 / ratatui 均无此类 API；是否存在官方"做不到/不打算做"的说明**未证实**。
4. **crossterm `tty::IsTty` 在 0.29.0 中为哪些类型实现** —— 只确认 trait 在 0.29.0 条目清单中存在；实现细节未读 → ⚪。
5. **crossterm 未发布 master 的 MSRV 1.85 / `IsTty` 移除将在哪个版本发布** —— CHANGELOG 只写在 `# Unreleased` 段，无版本号或日期 → ⚪。
6. **`syntect` / `tree-sitter-highlight` 之外的独立语法高亮候选** —— 本次只核查了 `syntect`、`tree-sitter-highlight`、`bat`（库）、`two-face`；crates.io 上其它同类 crate **未逐一体检** → ⚪。
7. **`syntect` 自带语法定义的数量与逐语言清单** —— 一手来源只说明来自 Sublime Text 默认包集与 `default-syntaxes` / `default-themes` 特性名，未给出条数 → ⚪。
8. **`tree-sitter-highlight` 是否有终端/ANSI 渲染能力** —— 0.27.0 条目清单中只有 `HtmlRenderer`（HTML），未见终端渲染器；官方是否有意提供终端渲染**未证实** → ⚪。
9. **`crossterm`、`termion`、`termwiz`、`cursive`、`iocraft` 的官方"维护状态"声明** —— 只有发布与提交时间事实（第 1.2 / 1.3 节）；`tui` 例外，其 README 有明确废弃声明（TUI-README）。
10. **ratatui 官方对 inline viewport 的终端兼容性清单**（哪些终端不可用 / 观感差异）—— 未见 → ⚪。
11. **`ratatui` 的 `rust-version` 与运行时行为差异**（例如 1.88 MSRV 是否因某具体特性）—— crates.io 与 workspace `Cargo.toml` 只声明版本号，未给理由 → ⚪。
12. **`bat` 作为库的 API 稳定性承诺** —— README 只指向 docs.rs，未见稳定性/语义化承诺 → ⚪。
13. **`termina` 本身的维护状态** —— 只有 crates.io 版本/日期事实（0.4.0，2026-08-31），无维护声明 → ⚪。
14. **各后端包数在 Windows / macOS 目标下的值** —— 本文实测只覆盖 `x86_64-unknown-linux-gnu`；跨目标数字**未证实** → ⚪。

---

## 7. 来源一览（全部一手来源，抓取于 2026-09-13 UTC）

### ratatui（元数据 / 源码 / 官方文档站）

| ID | URL |
| --- | --- |
| RT-CRATE | <https://crates.io/api/v1/crates/ratatui> |
| RT-SUBCRATES | <https://crates.io/api/v1/crates/ratatui-core> · <https://crates.io/api/v1/crates/ratatui-widgets> · <https://crates.io/api/v1/crates/ratatui-crossterm> · <https://crates.io/api/v1/crates/ratatui-termion> · <https://crates.io/api/v1/crates/ratatui-termwiz> · <https://crates.io/api/v1/crates/ratatui-termina> · <https://crates.io/api/v1/crates/ratatui-macros> |
| RT-CARGO | <https://raw.githubusercontent.com/ratatui/ratatui/main/Cargo.toml> · <https://raw.githubusercontent.com/ratatui/ratatui/main/ratatui/Cargo.toml> |
| RT-TERM | <https://raw.githubusercontent.com/ratatui/ratatui/main/ratatui-core/src/terminal.rs> |
| RT-VIEW | <https://raw.githubusercontent.com/ratatui/ratatui/main/ratatui-core/src/terminal/viewport.rs> |
| RT-INLINE | <https://raw.githubusercontent.com/ratatui/ratatui/main/ratatui-core/src/terminal/inline.rs> |
| RT-INIT | <https://raw.githubusercontent.com/ratatui/ratatui/main/ratatui/src/init.rs> |
| RT-DOCS-TERM | <https://docs.rs/ratatui-core/0.1.2/ratatui_core/terminal/struct.Terminal.html> · <https://docs.rs/ratatui/0.30.2/ratatui/struct.Terminal.html> · <https://docs.rs/ratatui/0.30.2/ratatui/enum.Viewport.html> |
| RT-EX-ASYNC | <https://raw.githubusercontent.com/ratatui/ratatui/main/examples/apps/async-github/src/main.rs> · <https://raw.githubusercontent.com/ratatui/ratatui/main/examples/apps/async-github/Cargo.toml> |
| RT-EX-INLINE | <https://raw.githubusercontent.com/ratatui/ratatui/main/examples/apps/inline/src/main.rs> |
| RT-README | <https://raw.githubusercontent.com/ratatui/ratatui/main/README.md> |
| RT-TEMPLATES | <https://github.com/ratatui/templates> · <https://raw.githubusercontent.com/ratatui/templates/main/simple-async/template/src/main.rs> · <https://raw.githubusercontent.com/ratatui/templates/main/simple-async/template/Cargo.toml> · <https://raw.githubusercontent.com/ratatui/templates/main/event-driven-async/template/src/main.rs> |
| RT-WEB-BACKENDS | <https://ratatui.rs/concepts/backends/comparison/> · <https://raw.githubusercontent.com/ratatui/ratatui-website/main/src/content/docs/concepts/backends/comparison.md> |
| RT-WEB-EVENTS | <https://raw.githubusercontent.com/ratatui/ratatui-website/main/src/content/docs/concepts/event-handling.md> |
| RT-WEB-RECIPE | <https://raw.githubusercontent.com/ratatui/ratatui-website/main/src/content/docs/recipes/apps/terminal-and-event-handler.md> · <https://github.com/ratatui/crates-tui> |
| RT-WEB-ALT | <https://raw.githubusercontent.com/ratatui/ratatui-website/main/src/content/docs/concepts/backends/alternate-screen.md> |
| RT-EX-INLINE-PAGE | <https://raw.githubusercontent.com/ratatui/ratatui-website/main/src/content/docs/examples/Apps/inline.md> |
| RT-COMMITS | <https://github.com/ratatui/ratatui/commits/HEAD.atom> |

### crossterm / termion / termwiz / termina

| ID | URL |
| --- | --- |
| CT-CRATE | <https://crates.io/api/v1/crates/crossterm> |
| CT-CARGO | <https://raw.githubusercontent.com/crossterm-rs/crossterm/master/Cargo.toml> |
| CT-README | <https://raw.githubusercontent.com/crossterm-rs/crossterm/master/README.md> |
| CT-CHANGELOG | <https://raw.githubusercontent.com/crossterm-rs/crossterm/master/CHANGELOG.md> |
| CT-DOCS-ALL | <https://docs.rs/crossterm/0.29.0/crossterm/all.html> · <https://docs.rs/crossterm/0.29.0/crossterm/event/struct.EventStream.html> · <https://docs.rs/crossterm/0.29.0/crossterm/style/fn.available_color_count.html> · <https://docs.rs/crossterm/0.29.0/crossterm/tty/trait.IsTty.html> |
| CT-STYLE | <https://raw.githubusercontent.com/crossterm-rs/crossterm/master/src/style.rs> |
| CT-STREAM | <https://raw.githubusercontent.com/crossterm-rs/crossterm/master/src/event/stream.rs> |
| CT-EX-TOKIO | <https://raw.githubusercontent.com/crossterm-rs/crossterm/master/examples/event-stream-tokio.rs> |
| CT-COMMITS | <https://github.com/crossterm-rs/crossterm/commits/HEAD.atom> |
| TI-CRATE | <https://crates.io/api/v1/crates/termion> |
| TI-GITLAB | <https://gitlab.redox-os.org/api/v4/projects/redox-os%2Ftermion> · <https://gitlab.redox-os.org/api/v4/projects/redox-os%2Ftermion/repository/commits?per_page=1> |
| TW-CRATE | <https://crates.io/api/v1/crates/termwiz> |
| WEZ-COMMITS | <https://github.com/wezterm/wezterm/commits/HEAD.atom> |
| TM-CRATE | <https://crates.io/api/v1/crates/termina> |

### 替代品

| ID | URL |
| --- | --- |
| CU-CRATE | <https://crates.io/api/v1/crates/cursive> · <https://crates.io/api/v1/crates/cursive/0.21.1/dependencies> |
| CU-COMMITS | <https://github.com/gyscos/cursive/commits/HEAD.atom> |
| TUI-CRATE | <https://crates.io/api/v1/crates/tui> |
| TUI-README | <https://raw.githubusercontent.com/fdehau/tui-rs/master/README.md> |
| TUI-ARCHIVE | <https://github.com/fdehau/tui-rs> |
| IO-CRATE | <https://crates.io/api/v1/crates/iocraft> |
| IO-COMMITS | <https://github.com/ccbrown/iocraft/commits/HEAD.atom> |

### 语法高亮

| ID | URL |
| --- | --- |
| SY-CRATE | <https://crates.io/api/v1/crates/syntect> · <https://crates.io/api/v1/crates/syntect/5.3.0/dependencies> |
| SY-CARGO | <https://raw.githubusercontent.com/trishume/syntect/master/Cargo.toml> |
| SY-README | <https://raw.githubusercontent.com/trishume/syntect/master/Readme.md> |
| SY-UTIL | <https://raw.githubusercontent.com/trishume/syntect/master/src/util.rs> |
| SY-EASY | <https://raw.githubusercontent.com/trishume/syntect/master/src/easy.rs> |
| SY-COMMITS | <https://github.com/trishume/syntect/commits/HEAD.atom> |
| TSH-CRATE | <https://crates.io/api/v1/crates/tree-sitter-highlight> |
| TSH-DOCS | <https://docs.rs/tree-sitter-highlight/0.27.0/tree_sitter_highlight/> |
| TSH-CARGO | <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/crates/highlight/Cargo.toml> |
| TS-CARGO | <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/Cargo.toml> |
| TS-DOC-HL | <https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/docs/src/3-syntax-highlighting.md> |
| BAT-CRATE | <https://crates.io/api/v1/crates/bat> · <https://crates.io/api/v1/crates/bat/0.26.1/dependencies> |
| BAT-README | <https://raw.githubusercontent.com/sharkdp/bat/master/README.md> |
| BAT-CARGO | <https://raw.githubusercontent.com/sharkdp/bat/master/Cargo.toml> |
| TF-CRATE | <https://crates.io/api/v1/crates/two-face> |
| ONIG-BUILD | <https://raw.githubusercontent.com/rust-onig/rust-onig/master/onig_sys/build.rs> |

### 终端能力探测

| ID | URL |
| --- | --- |
| STD-ISTERM | <https://doc.rust-lang.org/std/io/trait.IsTerminal.html> |
| AQ-DOCS | <https://docs.rs/anstyle-query/1.1.5/anstyle_query/> |
| AS-CRATE | <https://crates.io/api/v1/crates/anstream> |
| TC-CRATE | <https://crates.io/api/v1/crates/terminal-colorsaurus> |
| SC-CRATE | <https://crates.io/api/v1/crates/supports-color> |

---

## 8. 数字速查（全部事实，不含结论）

| 事实 | 值 | 来源 |
| --- | --- | --- |
| `ratatui` 最新版 / 日期 / MSRV | `0.30.2` / 2026-06-19 / 1.88.0（edition 2024） | RT-CRATE, RT-CARGO |
| `ratatui` 0.30.0 的 MSRV | 1.86.0 | RT-CRATE |
| `ratatui` 默认后端 | `crossterm`（default 特性之一） | RT-CARGO |
| ratatui 后端特性名 | `crossterm` / `termion` / `termwiz` / `termina`；版本选择 `crossterm_0_28` / `crossterm_0_29` | RT-CARGO |
| ratatui 仓库最后提交 | 2026-09-10T20:41:19Z | RT-COMMITS |
| `ratatui` 默认特性闭包 | **70 包** | 本文实测 |
| `ratatui` 关默认特性闭包 | **41 包** | 本文实测 |
| `ratatui`+`crossterm` / +`termion` / +`termwiz` / +`termina` | **62 / 45 / 118 / 54 包** | 本文实测 |
| `crossterm` 单独闭包 | **27 包** | 本文实测 |
| `termion` 单独闭包 | **3 包** | 本文实测 |
| `termwiz` 单独闭包 | **85 包** | 本文实测 |
| `ratatui-crossterm` / `ratatui-termion` / `ratatui-termwiz` 闭包 | **59 / 42 / 109 包** | 本文实测 |
| `crossterm` 最新版 / 日期 / crates.io MSRV | `0.29.0` / 2025-04-05 / 1.63.0（edition 2021） | CT-CRATE |
| crossterm master（未发布） | MSRV → 1.85、edition → 2024、移除 `IsTty` | CT-CHANGELOG |
| crossterm 仓库最后提交 | 2026-08-21T00:06:18Z | CT-COMMITS |
| `termion` 最新版 / 日期 / 仓库最后提交 | `4.0.6` / 2025-11-21 / 2025-11-21 | TI-CRATE, TI-GITLAB |
| `termwiz` 最新版 / 日期 | `0.23.3` / 2025-03-20 | TW-CRATE |
| wezterm（termwiz 所在仓库）最后提交 | 2026-09-12T11:38:23Z | WEZ-COMMITS |
| `termina` 最新版 / 日期 / MSRV | `0.4.0` / 2026-08-31 / 1.71 | TM-CRATE |
| `cursive` 最新版 / 日期 / 最后提交 / 闭包 | `0.21.1` / 2024-08-03 / 2026-09-09 / **62 包** | CU-CRATE, CU-COMMITS, 本文实测 |
| `tui` 最新版 / 日期 / 最后提交 / 闭包 | `0.19.0` / 2022-08-14 / 2023-08-06 / **19 包** | TUI-CRATE, TUI-README, 本文实测 |
| `tui` 维护状态一手依据 | README：`August 2023: This crate is no longer maintained.`；仓库页 `Public archive` | TUI-README, TUI-ARCHIVE |
| `iocraft` 最新版 / 日期 / 最后提交 | `0.9.1` / 2026-09-04 / 2026-09-07 | IO-CRATE, IO-COMMITS |
| crossterm `event-stream` 特性 | `event-stream = ["dep:futures-core", "events"]` | CT-CARGO |
| crossterm 官方 tokio 示例 | `examples/event-stream-tokio.rs`（`required-features = ["event-stream","events"]`） | CT-CARGO, CT-EX-TOKIO |
| ratatui 官方 tokio 示例 | `examples/apps/async-github`：`#[tokio::main]` + `EventStream` + `tokio::select!`（tick + event），在任务内 `draw` | RT-EX-ASYNC |
| ratatui 官方异步模板 | `ratatui/templates` 的 `simple-async` / `event-driven-async`；依赖 `ratatui 0.30.2` + `crossterm 0.29.0 (event-stream)` + `tokio 1.52.3 (full)` | RT-TEMPLATES |
| inline API | `Viewport::Inline(u16)`、`TerminalOptions{viewport}`、`Terminal::with_options`、`ratatui::init_with_options`、`Terminal::insert_before`、`Frame::area()` | RT-VIEW, RT-DOCS-TERM, RT-INIT |
| `init` vs `init_with_options` 的 alt screen | `init` 进 alternate screen + raw mode；`init_with_options` 只进 raw mode | RT-INIT |
| inline 的官方限制 | 全宽、锚定光标行 col 0、高度钳制到终端高、可能引发滚动、`insert_before` 非 inline 时无效、无 `scrolling-regions` 时需清屏重绘 | RT-VIEW, RT-DOCS-TERM |
| `syntect` 最新版 / 日期 / 最后提交 / 默认特性 | `5.3.0` / 2025-09-27 / 2026-04-28 / `default-onig` | SY-CRATE, SY-COMMITS |
| `syntect` 闭包（onig / fancy） | **42 / 44 包** | 本文实测 |
| `syntect` 终端 helper | `as_24_bit_terminal_escaped(v, bg) -> String` | SY-UTIL |
| `tree-sitter-highlight` 最新版 / 日期 / MSRV / 闭包 | `0.27.0` / 2026-08-30 / 1.90 / **15 包** | TSH-CRATE, 本文实测 |
| `tree-sitter-highlight` 对运行时的要求 | `tree-sitter ^0.27.0`；0.27.0 ↔ 0.27.0 同步 | TSH-CRATE, TSH-DOCS |
| `tree-sitter-highlight` 0.27.0 公开条目 | `Highlight`、`HighlightConfiguration`、`Highlighter`、`HtmlRenderer`、`HighlightEvent`、`Error` | TSH-DOCS |
| `bat` 最新版 / 日期 / MSRV / 闭包 | `0.26.1` / 2025-12-02 / 1.87 / **157 包**（关默认特性 **64 包**） | BAT-CRATE, 本文实测 |
| `bat` 作为库的要求 | 必须启用 `regex-onig` 或 `regex-fancy` 之一 | BAT-README |
| `two-face` | `0.5.2+bat-0.26.1` / 2026-08-07 / MSRV 1.79 | TF-CRATE |
| TTY 判定 | `std::io::IsTerminal::is_terminal()`（1.70.0 稳定） | STD-ISTERM |
| 真彩色判定 | `crossterm::style::available_color_count() -> u16`（COLORTERM → TERM；`24bit`/`truecolor` → `u16::MAX`；`256` → 256；默认 8） | CT-DOCS-ALL, CT-STYLE |
| 颜色能力查询（纯 env） | `anstyle_query::{truecolor, term_supports_color, term_supports_ansi_color, no_color, clicolor, clicolor_force, is_ci}` | AQ-DOCS |
| alt screen 探测 API | **不存在**（crossterm 0.29.0 / ratatui 条目清单中无） | ⚪（第 6 节第 3 条） |
