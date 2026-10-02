# fs-agent 的 TUI 能不能「渲染」mermaid

> 调研日期 2026-10-03，基线 commit `78aab39`（`git log -1`）。只读调研：没改任何代码，没跑
> `cargo build` / `cargo test`。外部事实一律取自一手来源（crates.io API、docs.rs、上游仓库
> README / Cargo.toml）；查不到的直接写「未找到」。

**一句话**：今天的 ` ```mermaid ` 就是一个**没登记进 `canonical_language` 的普通围栏块** ——
语言名照样显示、内容逐字原样、不上色、更不画图（`src/render/highlight.rs:214-228`）。
到 2026-10，「能不能画」已经不是问题：**纯 Rust、无浏览器、输出 Unicode box-drawing 文本**的
crate 至少三个可用。真正的阻力在本仓库自己的约束上：宽度与换行归 `pane::wrap_line`、渲染器是
纯函数、TUI 事件循环是单任务同步的、加依赖要过 ADR 那一关。

---

## 1. 现状

### 1.1 全仓库只有一处提到 mermaid

`grep -rn "mermaid\|Mermaid"` 在整个仓库只命中一处：
`docs/adr/0008-markdown-parsing-by-pulldown-cmark.md:24`，说的是被否掉的 `ratatui-markdown`
的**包体里有一个 7.27 MB 的 mermaid 动图**。也就是说：**今天不存在任何 mermaid 专用代码、
测试或文档**。模型若吐 ` ```mermaid `，走的就是普通围栏那条路。

### 1.2 这条渲染管线的形状

`markdown::to_lines` 是**纯函数**：`src/render/markdown.rs:39` 只收 `(text, width)`，
`:52` 的同族入口多一个 `indent`；两者都只构造 `Renderer` 跑一遍 `pulldown-cmark` 事件流再
`finish()`（`markdown.rs:53-63`）。没有 I/O、没有 time、没有外部进程。存在的进程内状态只有
两处缓存：语法 query 的 `OnceLock`（`src/render/highlight.rs:234-244`）与 per-thread
`Highlighter`（`highlight.rs:347-351`）—— 都是确定性的，不改变「同输入同输出」。

围栏块怎么走的：

1. `Tag::CodeBlock` 起一个 `CodeBuffer`，`lang` 取 info string 的第一个词（`markdown.rs:192-202`、`:633-635`）；
2. `Event::Text` 在缓冲开着时**原样追加**，不解析（`markdown.rs:346-350`）—— `tests/render_markdown.rs:84-101` 把这条钉成「围栏块逐字保留、永不按 Markdown 解析」；
3. `TagEnd::CodeBlock` 调 `write_code`（`markdown.rs:255-260`、`:509-530`）：先吐一行**右对齐到内容右缘的语言名**（灰色 `MUTED`），再吐 `code_lines`；
4. `code_lines`（`markdown.rs:641-661`）用 `highlight::highlight_code(lang, text)` 换按行 span，拿不到就退 `plain_rows`（`:663-672`，全部 `Class::Plain`），再把每行折到预算内（`wrap_code`，`:676-701`，续行保持 2 格缩进）。

于是 ` ```mermaid ` 的实际观感是：**一行右对齐的灰色 `mermaid`，下面是两格缩进、不着色的源码**
（`Class::Plain` 的 `style()` 是 `Style::default()`，`highlight.rs:141`）。不报错、不消失，也不画。

宽度从哪来：调用方给。TUI 在绘制时把**转录文字区的宽度**递下去 ——
`paint_block(block, colors, width)`（`src/render/tui.rs:4432`）→
`to_lines_indented(text, width, indent)`（`tui.rs:4450-4453`），其中 `indent` 是
`[name] ` 前缀占的列数。没有窗格的调用方（plain 的那一半、纯文字测试）拿默认 80
（`tui.rs:4403-4407`、`:4415-4420`）。宽度预算是 `width - indent`
（`markdown.rs:511`、`:557`）。

高度与滚动：**都不在 markdown 层**。`Pane` 按显示行算 —— `view(width, height, live)`
（`src/render/pane.rs:136-157`）用当帧高度求 `max_top`、吸底或夹住视口，`scroll` / `page` /
`wheel` 都在那里（`pane.rs:178-206`）；来源行上限 `CAP = 20_000`（`pane.rs:19`）。窗格高度
来自终端几何的纯函数 `layout::plan`（`src/render/layout.rs:296-360`），转录行数就是
`h − 4 − 输入行数`（`layout.rs:25`、`:303-306`；`docs/render.md:138-141`）。

高亮挂在哪：挂在**代码块**上，而且是「先高亮、再按宽度折行」两拍
（`docs/render.md:82-84`）。`highlight_code` 是唯一生产调用点；它认十种语言，别名表在
`canonical_language`（`highlight.rs:214-228`），**`mermaid` 不在表里 → `None` → 纯文本降级**。

### 1.3 宽度分档

终端宽度决定左栏：`≥120` 时 40 列、`80–119` 时 28 列、**更窄整栏隐藏**，主列拿走全部
（`layout.rs:46-53`、`:367-378`、`:384-387`）。对转录正文而言，可用列数因此是
`终端宽 − 左栏(29 或 41) − 2`（右缘永远留滚动条与回合条两列，`layout.rs:42`、`:145-154`）。
换算成实际预算：120 列 → 77 列，80 列 → 49 列，40 列 → 38 列。**任何按宽度排版的东西都得
接受这个范围**。

---

## 2. 技术路径

### (a) 纯 Rust：解析 + 布局 + box-drawing 文本

**当下确实有可用 crate**，而且不止一个（crates.io 搜索 `mermaid` 有 2660 条，下面是我逐个
打开 API / README 核实过的）。按与本仓库的贴合度排：

| crate | 最新版 / 日期 | 许可证 | 输出 | 依赖 | 关键事实 |
| --- | --- | --- | --- | --- | --- |
| `mermaid-text` | 0.57.0 / 2026-07-21 | MIT | **Unicode box-drawing 文本**（可选 ASCII） | `ascii-dag`、`chrono`、`unicode-width`（3 个正常依赖） | `mermaid_text::render(&str) -> Result<String>`；`render_with_width(src, Some(80))` 直接收列宽预算；输出保证**无 ANSI 转义**（颜色要显式 opt-in）。81 个版本、近期下载 ~6 万。flowchart/state/sequence/pie/er/class/gantt/journey/timeline/gitGraph/mindmap/sankey/xychart… 覆盖面很宽。MSRV 1.92、edition 2024 |
| `mmdflux` | 2.6.1 / 2026-08-09 | MIT | 文本 + ASCII + SVG + MMDS JSON | 默认 `cli` feature 拉 clap 等，可关 | `render_diagram(src, OutputFormat::Text, &RenderConfig)`；自研 `flux-layered` 正交路由；flowchart/class/sequence/state。仓库 9.3 万行 Rust，体量大 |
| `merman` | 0.8.0-alpha.7（稳定 0.7.0）/ 2026-09-30 | MIT OR Apache-2.0 | SVG 为主，`ascii` feature 给终端文本 | 大（`merman-core` / `-render` / `-ascii`…） | 「headless Rust Mermaid」，对齐上游 mermaid@12；**Zed 的 Rust Mermaid 后端**；`RenderRequest::ascii()`。默认 feature 会拖进 ELK（EPL-2.0 notice），要 `default-features = false, features = ["all-diagrams","ascii"]` |
| `console-mermaid` | 0.1.0 / 2026-05-08 | MIT | Unicode 或 ASCII 文本 | clap、regex、env_logger… | Go 项目 `mermaid-ascii` 的 Rust 移植；CLI 为主（也有 lib）。只有 0.1.0 一个版本、总下载 27 次 —— **太新太薄** |
| `graphs-tui` | 0.4.0 / 2026-02-11 | 未核实（README 未取） | 「Mermaid 与 D2 的终端渲染器」，Unicode/ASCII | 未核实 | crates.io 描述含 flowchart / state / pie |
| `mermansi` | 0.1.6 / 2026-08-04 | 未核实 | 「纯 Rust 终端渲染库 + CLI」 | 未核实 | 描述自称 production-quality，只有 5 个版本、204 次下载 |
| `rs-rich-mermaid` | 0.0.2 / 2026-09-29 | MIT | flowchart 走文本，别的图型走 `mmdc`（Sixel/象限块） | `rs-rich`（整个 rich 移植） | 设计目标其实是 Python `rich`，不是我们这种 `Line`/`Span` 管线；`mmdc` 那条要外部进程 |
| `mermaid-svg` / `mermaid-rs-renderer` / `flowmaid` | 0.7.0 / 0.3.1 / 0.27.0 | 未逐个核实 | **SVG**（23 种图型） | — | 能画但画成 SVG：终端里还得再想办法显示，不在本次「画成图」的直接路上 |
| `mermaid-core`（cmwright） | 0.1.5 / 2026-07-13 | MIT OR Apache-2.0 | SVG / PNG / **Ascii**（`OutputFormat::Ascii`，`ascii` 是默认 feature） | pest、petgraph、`mermaid-dagre`… | README 说支持 flowchart/sequence/gantt/class，**但同一仓库的 GitHub README 写着 class/ER「Not Yet Implemented」**，两处自相矛盾（见「存疑」） |
| `mermaid-rs`（iwillreku3206） | 0.1.1 / 2024-10-27 | MIT | SVG | 内嵌 2 MB JS + 一个 Chromium | 是 Mermaid **JS** 的绑定，README 自己在讲「embedded Chromium 初始化失败」。**不可用** |

基础件单独看：

- **布局引擎**：`mermaid-dagre`（0.1.5，1:1 dagre JS 移植，5 509 行）、`dagre-dgl-rs`
  （0.1.1 / 2026-05-22，MIT，13 076 行，`petgraph` + `indexmap`，输出 node 坐标与 edge
  waypoints，**不渲染**）。两者都只给几何，画字符还是自己的事。
- **graphviz 绑定**：`graphviz-rust` 0.9.9（2026-09-14，crates.io 的 license 字段是
  **`non-standard`**）**默认 feature 叫 `graphviz-exec`，README 的 Caveats 明说「command
  client 必须预先装好」** —— 它是让外部 `dot` 干活，不是纯 Rust。
- `layout-rs` 0.1.3（MIT、2025-04-24）是纯 Rust 的 Graphviz 类布局，但输出 SVG。

**代价**：`mermaid-text` 是最小的一步 —— 3 个正常依赖、MIT、API 就是一个
`&str -> Result<String>`。**风险**：它输出的是**已经排好版的整段文本**，自带宽度压缩
（`render_with_width`），这和本仓库「折行归 `pane::wrap_line`」的立场正面冲突，但方向上
其实吻合 —— 本仓库对**宽度敏感的块**本来就是「按宽度重渲染」而不是「重新折行」
（`tui.rs:1728-1745`）。另外它 81 个版本、0.57 的版本号说明 API 还在快速动；它自己的
README 列了四条限制（点线交叉口变实线、RL/BT 子图内部不翻转、深层嵌套 direction 覆盖不
完全、极窄列下长标签可能重叠）。

### (b) 调外部进程（`mmdc` / `dot` / mermaid-cli）

`@mermaid-js/mermaid-cli` 最新 **12.0.0**，MIT，`engines.node >= 22.13.0`，
**`peerDependencies: puppeteer ^25.0.0`**（npm registry）。也就是要 Node + 一个 headless
Chromium；`graphviz-rust` 那条则要系统里先有 `dot`。

与本仓库既有约束的关系，逐条：

- **进程模型**：`docs/sandbox.md:11` 写着 `tools/process.rs` 的 `run()` 是 `bash` 与动态工具
  **唯一**的 spawn 处，超时与进程组 kill 也都在那。渲染器侧再开一个 spawn 口，等于在沙箱这
  一层之外新开第二条路径 —— 那是要重写一行的决定，不是加个函数。实际 spawn 证据：
  `src/tools/process.rs:116-132`（`Command::new` + `process_group(0)`），全仓库其它
  `Command::new` 只有沙箱探测那一条（`src/tools/sandbox.rs:54`）。
- **沙箱**：包出来的 argv 是 `--ro-bind / /`、`--tmpfs /tmp`、`--unshare-user/-pid/-ipc/-uts`、
  可写根、两张遮罩（`src/tools/sandbox.rs:273-324`）。它**不管网络**（`docs/sandbox.md:22`、
  `:92`），也不做资源限额。`~/.config/fs-agent` 被遮成空且只读（`docs/sandbox.md:33-36`）。
  mmdc 的 Chromium 是另一个沙箱逃逸面；能不能在 `--unshare-user/-pid` 的嵌套命名空间里跑
  headless Chromium，**我没有本机实测，不写结论**（见「存疑」）。`/tmp` 每次调用都是新的空
  tmpfs（`docs/sandbox.md:31`），mmdc 写临时目录这条路是通的。
- **测试无网络、可复现**：`README.md:264` 明说测试只测外部行为、provider 全是假 provider、
  **没有网络依赖**；`docs/sandbox.md:5` 的一手材料也是「本机实测」。一个需要 Node + Chromium
  的外部渲染器会让 `cargo test` 多出一条环境依赖，或者把这条路径整个排除在测试外。
- **凭据打码**：打码发生在事件**追加之前**（`docs/credentials.md:12`、`:16`），所以渲染器看到
  的 mermaid 源码**已经是洗过的**。这一条对 (b) 不构成额外风险（送到外部进程的文本里不会有
  已配置的 key），但也意味着「图里画的是什么」和原文可能已经不同。**打码是值级的、精确的**
  （`docs/credentials.md:49-51`），不认识的密钥它不管 —— 所以外部进程不是新增泄漏面，也不是
  防护面。

**代价**：一条全新的进程边界 + 一个 Node/Chromium 运行期依赖 + 与「测试无网络」的冲突。
**风险**：高。收益是「和上游 mermaid 1:1」，但那不是本仓库要的（它要的是可读的转录）。

### (c) 自己写一个子集（只支持 `flowchart TD/LR` 的布局与画线）

工作量集中在三块：分枝/分层的排布（Sugiyama 的 rank + 层内排序 + 长边 dummy 节点）、边的
正交路由（A\* 或简单的 L 形折线）、字符网格与方向掩码到 box-drawing 字形的映射。
`mermaid-text` 的 README「How It Works」把这三块写得很清楚，可以当作规格参考 —— 它承认
「方向位掩码画布、重心启发式的常数、子图边框留白」是从 Python 的 `termaid` 借鉴的。

**代价**：这是三到四个模块、上千行的工作，而且**没有终点** —— 每多一种节点形状、每多一种边
样式都要再写一遍。**风险**：中高（收尾与边角）；但它换回来的是零新依赖、完全可控的宽度语义、
可测的纯函数，而且**字体宽度的问题本仓库已经解过**（`src/render/width.rs` 的
`text_columns` / `char_columns`，CJK 两列）。

### (d) 不画图：只做结构化呈现

- 保留现有围栏路径（语言名 + 逐字原文）；
- 或者按 `-->` 把边拆成列表（`A --> B` 一行一条），缩进表示子图；
- 或者只给一行提示（例如「mermaid 图，N 个节点、M 条边」）。

**代价**：最小 —— 可以只动 `markdown.rs` 的 `write_code`，加一个 `lang == "mermaid"` 分支，
纯函数性质、测试方式、宽度语义、依赖表**一个都不动**。**风险**：几乎没有，但用户要的
「画成图」并没有被满足。它是**兜底**，不是答案。

### (e) 其它路径（调研中发现）

1. **SVG 路线 + 终端图片协议**：`mermaid-svg` / `mermaid-rs-renderer` / `mermaid-core` 都能
   吐 SVG，再用 Sixel / iTerm2 / Kitty 图形协议贴出来 —— `rs-rich-mermaid` 的 `mmdc` feature
   正是这个套路（`rs-rich-art`，Sixel 或象限块）。但这要求终端支持图片协议，且 **ratatui 的
   `Line`/`Span` 缓冲根本没有「图片」这个单元**，得改动绘制层与 `Pane` 的显示行模型。**判为
   与本仓库的渲染边界不相容**。
2. **只解析、复用现成布局、自己画字符**：拿 `mermaid-dagre` / `dagre-dgl-rs` 的坐标，配自己
   的网格渲染。省掉 (c) 里最难的一半（rank + 排序），但边的正交路由仍要自己写，而且要先有
   一个 mermaid 语法解析器 —— 这一步 `mermaid-text` 已经把前面全做完了，没有理由只买一半。
3. **`tree-sitter-mermaid`**：`Latias94/merman` 出的容错文法，Rust/Node/browser 都能用。本仓库
   已经有 `tree-sitter` 0.27 与 `tree-sitter-highlight`（`Cargo.toml:38-43`），所以**给
   ` ```mermaid ` 上语法高亮**是成本最低的一项「变好看」——但它不是画图。

---

## 3. 撞点（与仓库既有约束逐条对照）

1. **「这一版不做」清单**：`README.md:268-270` 列了 AST 编辑、向量检索、两进程渲染、内置
   编辑器、交互式 transcript 浏览器、网络隔离等。**没有任何一条点名 mermaid 或图表渲染**。
   v1 spec 的 `Out of Scope` 同样没有（`.scratch/fs-agent-v1/spec.md:636-652`），但里面
   **有「两进程渲染」**（`spec.md:646`）—— 若 (b) 的形态是「渲染器 fork 一个外部进程」，
   它至少与这一条的**精神**接壤，需要显式判定。同一份 spec 的
   `:671` 写着「实现者请勿顺手改进……要动它们，先改这张 spec」。结论：mermaid **不在禁区**，
   但按 `README.md:276` 的规矩，要动就得先改 spec（或另起一个 effort）。
2. **TUI 是单任务同步的**：`Tui::run` 是一个 `async fn`，由 `tokio::spawn` 起成**一个**任务
   （`src/render/mod.rs:184-199`、`src/render/tui.rs:331`）。事件到达 → `state.apply` →
   `push_block` → `paint_block` 全在这一个任务里**同步**跑完（`tui.rs:1700-1722`、
   `:4415-4432`），然后才 `terminal.draw`（`tui.rs:455`）。两处 `select!` 的分支里没有任何
   `spawn_blocking`（`tui.rs:384-413`）。**含义**：在 `paint_block` 里做一次 10–100 ms 的
   布局，就是整个 TUI 卡 10–100 ms，键盘与流都停。`mermaid-text` 自己的性能说明是「约 100 个
   节点远低于 10 ms」，但**没有上限保证**；若走这条路，按块缓存的策略（同一块不重复算）是
   必须的，而不是优化。
3. **`to_lines` 的纯函数性质**：今天它是 `(text, width) -> Vec<Line>`（`markdown.rs:39`），
   只有确定性的进程内缓存。接一个纯 Rust 渲染库可以保持这条；接 mmdc / `dot` 会**当场破掉**
   它（同输入可能因环境不同而不同或失败），而这条性质正是 `tests/` 里那 44 处 `to_lines` /
   `to_lines_indented` 调用（43 处在 `tests/render_markdown.rs`、1 处在
   `tests/render_layout.rs`）能成立的前提（`tests/render_markdown.rs:9`、`:15`）。
4. **宽度与换行归 `pane::wrap_line`**：`.scratch/markdown-render/spec.md:26` 与 `:103-106` 是
   明写的立场：「换行仍归 `pane::wrap_line`……渲染器只负责把行按 §2/§3 的规则折到宽度内」。
   任何画图方案都会**自带排版**（图不能被逐字符硬折），所以要在 spec 层面为「图」开一个例外，
   或者明确「图的内部排版不算折行」。这不是实现细节，是那份 spec 上的一条线。
5. **宽度三档**：见 §1.3。图在 38 列（40 列终端）里基本没救 —— 只能降级成 (d)。落地时必须
   写清「窄于多少就不画、退回原文」。
6. **测试只测外部行为、无网络**：`README.md:264`。纯 Rust 渲染器可以在这条线下照测
   （`to_lines` 的断言方式原样可用）；外部进程不行。
7. **沙箱进程模型**：`docs/sandbox.md:11` 的「唯一 spawn 处」。见 (b)。
8. **渲染通道容量**：`RENDER_CHANNEL_CAPACITY = 1024`，丢了增量只降级不报错
   （`src/render/mod.rs:61-63`）。对画图无直接冲突，但「按块缓存」的键要能扛住宽度变化 ——
   这正是 `Tui::painted` 那份绘制记录已经在做的事（`tui.rs:1700-1706`、`:1736-1744`）。
9. **ADR 0008 的门槛**：那次连引一个纯 Rust 的 `pulldown-cmark` 都写了 ADR，并且逐个核实
   候选 crate 的依赖闭包与 MSRV（`docs/adr/0008-markdown-parsing-by-pulldown-cmark.md:23-31`）。
   加 `mermaid-text`（3 个依赖、MIT、MSRV 1.92）在这个标准下是**可过的**，但必须走同一条流程。

---

## 4. 结论（按可行性排序）

| # | 方案 | 代价 | 风险 | 要动哪些文件 |
| --- | --- | --- | --- | --- |
| 1 | **(d) 结构化呈现**：`lang == "mermaid"` 时把源码按 `-->` 拆成缩进列表 / 加一行「N 个节点 M 条边」 | 小：一个分支，几十行 | 极低 | `src/render/markdown.rs`（`write_code`，`:509-530`）、`tests/render_markdown.rs`、`src/render/wording.rs`（提示文案） |
| 2 | **(e)(3) 只加语法高亮**：用已有的 `tree-sitter` 基建给 mermaid 源码上色 | 小到中：一份 query 或一份简单映射 | 低 | `src/render/highlight.rs`（`canonical_language` + `config_for`）、`Cargo.toml` |
| 3 | **(a) 引 `mermaid-text` 画文本图** | 中：一个新依赖 + 在纯函数里过一次布局 + 宽度变化时的重渲染 | 中：依赖年轻（0.57）、MSRV 1.92、与「换行归 pane」的立场冲突、单任务下可能卡帧 | `Cargo.toml`、`src/render/markdown.rs`、`src/render/tui.rs`（宽度变化的重渲染路径 `:1728-1745`）、`tests/render_markdown.rs`、`docs/render.md`、ADR |
| 4 | **(c) 自研 `flowchart TD/LR` 子集** | 大：布局 + 路由 + 网格，上千行 | 中高：没有终点 | 新模块（如 `src/render/mermaid/`）+ `markdown.rs` + 测试 + spec |
| 5 | **(b) 外部进程（mmdc / dot）** | 很大：新进程边界 + Node/Chromium 运行期依赖 | 高：破纯函数、破唯一 spawn 处、与无网络测试冲突 | `tools/process.rs` 那条边界、`render`、`docs/sandbox.md`、spec |

推荐顺序：先做 **1**（立刻可读、无风险），把它当作**降级地板**；再评估 **3** —— 如果
`mermaid-text` 在 40/80/120 三档下的真实输出能过目，它一次性把「画成图」这件事解决掉；**2**
可以作为 3 的补充（图之外的 mermaid 源码仍需要可读）。**4** 只有在 3 被否决（依赖政策、
MSRV、宽度语义）时才值得开工。**5** 不建议。

---

## 5. 存疑

1. **`mermaid-text` 在 80 列、尤其是 38 列下的真实输出我没有跑过**（本轮禁止 `cargo build`）。
   README 只说「extremely narrow max_width may produce overlapping node boxes」。→ 确认方式：
   在 `/tmp` 里建一个一次性 crate，`cargo add mermaid-text`，喂
   `graph LR; A[Build] --> B[Test] --> C[Deploy]`，分别用 `Some(80)` / `None` 打出来看；
   或直接用它的 CLI（`cargo install mermaid-text`）。
2. **`mermaid-text` 与 `pane` 的宽度语义能不能对齐**：它收的是「总列预算」，而本仓库的块还要
   扣 `indent`（`[name] ` 的宽度）。→ 需要实测 `render_with_width(src, Some(width - indent))`
   的输出宽度是否**严格**不超过预算（README 没说这条保证）。
3. **`mermaid-text` 的 MSRV 1.92 / edition 2024 与本仓库的关系**：本机 `rustc 1.94.0` 能编，
   但仓库 `Cargo.toml` 没有 `rust-version`，也没有 CI 说明。→ 确认方式：看
   `scripts/` 与 README 的开发一节，或直接问维护者要不要钉一个 MSRV。
4. **headless Chromium 能不能在 fs-agent 的 bwrap profile 里跑**（若考虑 (b)）：`--unshare-user`
   `--unshare-pid` + 每次新的空 `/tmp` + `--dev /dev` 的组合我没有实测。→ 确认方式：在装了
   Node 22 与 puppeteer 的机器上，用 `tools/sandbox.rs::wrap` 拼同样的 argv 跑一次
   `mmdc`。这一条**不影响 (a)/(c)/(d)**。
5. **`mermaid-core` 到底支持不支持 class 图**：crate README 说支持（含 class），同一项目的
   GitHub README 说 class/ER「Not Yet Implemented」。→ 确认方式：读
   `docs.rs/mermaid-core/latest` 的源码页 `src/`，或跑一次。
6. **`graphs-tui` / `mermansi` / `mermaid-render` / `flowmaid` 的许可证与维护状态我只有
   crates.io 的元数据，没逐个读 README**。→ 若要认真评估，按 (a) 表格里的方式补。
7. **本仓库没有 mermaid 相关 spec 或票**：`.scratch/tui-mermaid/` 在本次调研前是空的（只有
   `research/` 目录）。所以「要不要做、做到哪一档」目前**没有任何已记录的判据**，本文件不是
   判据，只是材料。

### 外部来源（一手）

- crates.io API：`mermaid-text`、`mmdflux`、`merman`、`mermaid-dagre`、`console-mermaid`、
  `rs-rich-mermaid`、`mermaid-core`、`dagre-dgl-rs`、`graphviz-rust`、`layout-rs`、`mermaid-rs`
- https://static.crates.io/readmes/mermaid-text/mermaid-text-0.57.0.html
- https://static.crates.io/readmes/mmdflux/mmdflux-2.6.1.html
- https://github.com/Latias94/merman （README，`main`）
- https://raw.githubusercontent.com/AlextheYounga/console-mermaid/main/README.md
- https://static.crates.io/readmes/rs-rich-mermaid/rs-rich-mermaid-0.0.2.html
- https://raw.githubusercontent.com/cmwright/mermaid-rs/main/README.md
- https://docs.rs/crate/mermaid-core/latest/source/README.md 与
  https://docs.rs/mermaid-core/latest/mermaid_core/diagram/enum.OutputFormat.html
- https://raw.githubusercontent.com/rinfimate/dagre-dgl-rs/main/README.md
- https://raw.githubusercontent.com/besok/graphviz-rust/master/README.md
- https://registry.npmjs.org/@mermaid-js/mermaid-cli/latest
