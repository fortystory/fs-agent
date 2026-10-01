# 用于 ratatui 的 Markdown 渲染器 —— 一手来源笔记

关于三个把 Markdown 转成 ratatui 文本的 Rust crate 的事实，只采自一手来源：crates.io API、
已发布的 `.crate` 归档（其中规范化后的 `Cargo.toml`、随包附带的 `Cargo.lock` 与随包源码）、
docs.rs 的源码页面，以及各个精确 release tag 上的 GitHub 源码。没有博客文章，没有二手评测，
没有 LLM 生成的摘要。

本文件是这一轮的一手调研笔记：散文用中文，引文、标识符与命令保留原文（ADR 0004 / ADR 0005）。
本文不给出该选哪个 crate 的结论；最后一节记录本仓库现有的约束，以便把上面的事实对照着它们来读。

## 来源与版本

| crate | 仓库 | 检查的版本 | Commit（tag） | 检查日期 |
|---|---|---|---|---|
| `tui-markdown` | https://github.com/joshka/tui-markdown | 0.3.10 (2026-09-25) | `e6e04858f54306820cdd913714293615230519d7` (tag `tui-markdown-v0.3.10`) | 2026-10-01 |
| `ratatui-markdown` | https://github.com/celestia-island/ratatui-markdown | 0.3.6 (2026-05-21) | `9f4a2c06927859247c1c69ec8cd428facd857e6d` (tag `v0.3.6`)；归档的 `.cargo_vcs_info.json` 记录为 `2609d035c42bb0c7c9b8ebf816c6717d496fb97a` **dirty** | 2026-10-01 |
| `markdown-ratatui` | https://github.com/karanabe/mira | 0.1.0 (2026-09-12) | `8803d31379644b414e185dec385aeeae640c4860` (tag `v0.1.0`；`.cargo_vcs_info.json` 中为同一个 SHA) | 2026-10-01 |

源码引用使用 `https://github.com/<owner>/<repo>/blob/<tag>/<path>` 的形式，让每条论断都钉在已发布的
版本上，而不是移动中的 `main`/`dev` 尖端。凡 main 分支状态与已发布状态不同的地方，都会明确点出。

写作时各仓库的 HEAD（仅供背景，不作引用）：

- `joshka/tui-markdown` — `9143707da309d4924938334697b8fad84d332dfe` (2026-09-28)
- `celestia-island/ratatui-markdown` — `000a97b1752841aa711203f082de4b75c5e1fd4b` (2026-09-26)
- `karanabe/mira` — `58377f20268d6ecfdb4fe10aa428a801040d79ef` (2026-09-12)

全文通用的一个方法学注意点：已发布 crate 在 registry 一侧的 `.cargo_vcs_info.json` 与 crates.io 的
依赖端点反映的是**打包进去的东西**，而 tarball 的 `Cargo.lock` 会把所有 feature 组合下的
dev-dependencies 都算进来（见 §1.1 的传递依赖注意点）。凡涉及精确的「仅运行时」闭包计数之处，
都标为 **unverified**。

**本次会话未核实：** GitHub 仓库的 star 数与贡献者数。本环境发出的每一次未认证请求，GitHub REST API
都返回 `API rate limit exceeded`。凡成熟度论断依赖这些数字之处，都标为 **unverified**；
crates.io 的下载数与发布时间可用，因此改用它们。

---

# 1. `tui-markdown` (joshka/tui-markdown) 0.3.10

## 1.1 依赖与版本兼容性

- **运行时依赖是 `ratatui-core ^0.1`，不是 `ratatui`。** 已发布的 manifest
  列的是 `[dependencies.ratatui-core] version = "0.1", default-features = false`；`ratatui 0.30`
  只作为 `[dev-dependencies]` 条目出现（`default-features = false`）。该 crate 返回
  `ratatui_core::text::Text`，由调用方通过 ratatui 0.30 渲染（其示例用的是
  `frame.render_widget(text, frame.area())`）。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/Cargo.toml ·
  https://crates.io/api/v1/crates/tui-markdown/0.3.10/dependencies ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/lib.rs
- **普通（运行时）依赖是四个无条件 crate，加上两个位于默认 feature 之后的可选 crate：**
  `itertools ^0.15`、`pulldown-cmark ^0.13`、`ratatui-core ^0.1`、
  `tracing ^0.1.37`，以及在默认 `highlight-code` feature 下的 `syntect ^5` 与 `ansi-to-tui ^8`。
  可选的 `document-features ^0.2.11` 只用于 rustdoc 的 feature 表。
  https://crates.io/api/v1/crates/tui-markdown/0.3.10/dependencies
- **随 crate 发布了一个 `Cargo.lock`，内含 149 个包**，其中包括
  `syntect 5.3.0`、`onig 6.5.1`、`onig_sys 69.9.1`、`ratatui 0.30.2`、`ratatui-core 0.1.2`，
  以及 dev-dependency 相关的机制（`insta`、`rstest`、`pretty_assertions`、`tracing-subscriber`）。
  由于该 lock 覆盖 dev-dependencies 与所有 feature，它是上界而非运行时闭包；
  **精确的「仅运行时」传递依赖计数未核实**（这需要用本仓库的 feature 选择跑 `cargo tree`）。
  https://docs.rs/crate/tui-markdown/0.3.10/source/Cargo.lock
- **manifest 中任何地方都没有 `tree-sitter` 或 `tree-sitter-highlight` 依赖。** 它的语法高亮
  完全基于 syntect，因此 tree-sitter 版本的问题对这个 crate 不存在。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/Cargo.toml
- **MSRV：`rust-version = "1.88.0"`**，声明在 workspace manifest 中并由该 crate 继承。
  Edition 2021。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/Cargo.toml ·
  https://crates.io/api/v1/crates/tui-markdown/0.3.10

## 1.2 表格

- **表格会被渲染，且表头行在布局之前先被缓冲。** 模块文档说得很直接：
  "A table must be buffered before rendering because every cell can increase its
  column's terminal display width. `TableBuilder` collects the header and body rows, then
  renders their content, alignment, padding, and Unicode box-drawing borders once
  pulldown-cmark closes the table."
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L1-L8
- **表头在两处被视觉区分开来：** 表头单元格使用 `StyleSheet::table_header()` 样式
  （默认 `Style::new().bold().cyan()`），并且在表头与表体之间输出一条专用的 `├──┼──┤` 分隔线。
  边框使用 `StyleSheet::table_border()`（默认暗灰）；表体单元格使用 `table_cell()`（默认沿用
  周围的样式）。padding 计入表头/单元格样式，边框不计入。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L163-L190 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/style_sheet.rs#L176-L197
- **列宽自适应内容，另有一个可选的宽度预算。** `column_widths()` 取表头与表体各单元格在每一列上
  的最大值（下限为 1）。若设置了 `Options::table_width`，`fit_columns()` 会把预算一次一个单元格
  地花在最窄的未完成列上，且不会低于该列最宽的不可分割字素；不给宽度时，表格采用其自然内容宽度。
  Markdown 对齐（`:--`、`--:`、`:-:`）通过 `padding()` 生效。内容从不被截断，即使在宽度为零时
  也是如此。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L192-L245 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L462-L482 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/options.rs
- **是否需要整表上下文：是。** 渲染发生在 `TableBuilder::render` 中，由 `end_table` 在整张表
  结束后调用；它根据算出的 `column_widths` 构造 `TOP_BORDER`、表头、`HEADER_SEPARATOR`、
  表体各行与 `BOTTOM_BORDER`。高于一行的行会补齐到该行最高的单元格。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L60-L77 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L425-L460
- **单元格内的行内 Markdown 会被渲染。** `TextWriter::push_span` 把 span 送到当前活动的表格
  单元格，模块文档说明单元格会解析行内内容，且 pulldown-cmark 在 `TableCell` 内只会发出行内
  事件；行内处理器（code、bold、italic、links、images）写入同一个 sink。快照测试断言链接目标与
  带样式的内容能在单元格内保留下来，折行也能保住行内样式。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/mod.rs#L414-L434 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/snapshots/tui_markdown__renderer__table__tests__table_keeps_inline_features_in_cell.snap ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/snapshots/tui_markdown__renderer__table__tests__wrapping_preserves_inline_styles.snap
- **对齐行会被消费掉，不会被打印**（它转而驱动 `padding()`）；据该 crate 的文档，渲染出的对齐
  与 Markdown 的分隔行一致。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/lib.rs#L10-L16

## 1.3 代码高亮

- **默认开启；feature 名为 `highlight-code`，且它是唯一的默认 feature：**
  `default = ["highlight-code"]`、`highlight-code = ["dep:syntect", "dep:ansi-to-tui"]`。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/Cargo.toml#L15-L19
- **可以用 `default-features = false` 把它关掉。** 关掉该 feature 后，围栏代码仍会作为一个块
  着色，只是不做语法高亮：代码样式被压入行样式栈并应用到每一行代码，围栏行（``` ` ``` + the
  info string）也仍会输出。默认代码样式是 `Style::new().white().on_black()`，
  可通过 `StyleSheet::code` 配置。一个钉住该行为的测试快照只在 feature 关闭时参与编译。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L48-L84 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L215-L244 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/style_sheet.rs#L60-L62
- **引擎：syntect，且随包构建走的是 Oniguruma。** 高亮器是
  `syntect::easy::HighlightLines`，其底层是一个用
  `SyntaxSet::load_defaults_newlines()` 初始化的 `LazyLock<SyntaxSet>`。`syntect` 以
  `version = "5"` 加默认 feature 声明，而 syntect 的默认 feature 集是
  `default-onig = [..., "regex-onig"]`；该 crate 自己发布的 `Cargo.lock` 解析出
  `onig 6.5.1` + `onig_sys 69.9.1`。渲染通过 `ansi-to-tui` 把 syntect 的 24 位 ANSI 输出
  转回 ratatui span。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L16-L30 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L92-L108 ·
  https://github.com/trishume/syntect/blob/master/Cargo.toml (version 5.3.0, `default = ["default-onig"]`, `default-fancy`) ·
  https://docs.rs/crate/tui-markdown/0.3.10/source/Cargo.lock
- **使用方能否通过声明 feature 来避开 Oniguruma？不能。** Cargo feature 是可加的：
  `tui-markdown` 自己就是以默认 feature 请求 syntect 的，所以把
  `syntect = { version = "5", default-features = false, features = ["default-fancy"] }`
  作为直接依赖加进来，会合并成 *onig + fancy*，而 syntect 的 `regex_impl` 模块由
  `#[cfg(feature = "regex-onig")]` 选择（fancy 后端只在 `not(regex-onig)` 下编译）。
  syntect 自己的 Makefile 确认了切换引擎的预期方式：
  `cargo run --features default-fancy --no-default-features` —— 也就是说，*依赖 syntect 的那个
  crate* 必须去掉自己的默认 feature。`tui-markdown` 的 manifest 没给下游用户留下这样做的途径。
  此处未核实的绕行办法：对 manifest 做 `[patch]`/vendored fork，或上游改动。另注意
  `[dependencies.syntect] version = "5"` 不带 `default-features = false`，所以即便假设给
  `tui-markdown` 加一个 `syntect` feature 开关，也需要改 manifest。
  https://github.com/trishume/syntect/blob/master/src/parsing/regex.rs#L160-L170 ·
  https://github.com/trishume/syntect/blob/master/Cargo.toml (features block) ·
  https://github.com/trishume/syntect/blob/master/Makefile (`syntest-fancy`, `update-known-failures-fancy`)
- **语言覆盖与加载方式：syntect 自带的默认语法集，编译进去。** 这些语法就是 syntect 预构建的
  `default_newlines.packdump` 里的那些，在首次使用时惰性加载 —— 没有运行时文件加载，也没有
  按语言划分的 Cargo feature。syntect 的 Makefile 在发布时从一个 `testdata/DefaultPackage`
  checkout 生成该 dump。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L29-L30 ·
  https://github.com/trishume/syntect/blob/master/Makefile (`packs` target) ·
  https://github.com/trishume/syntect/blob/master/src/dumps.rs#L200-L219
- **围栏的语言标签被当作 syntect token 使用。** `start_codeblock` 取
  `CodeBlockKind::Fenced` 的 info string 并调用 `SYNTAX_SET.find_syntax_by_token(lang)`；命中则
  启动 `HighlightLines`，未命中则记录 `Could not find syntax for code block`，该块回退到
  `StyleSheet::code`。同一个字符串会打印在开头的围栏行上
  （`format!("{fence}{lang}")`）。缩进式代码块传入 `lang = ""`。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L48-L69 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L115-L130

## 1.4 接口形态

- **公开 API 是两个自由函数。** `pub fn from_str(input: &str) -> Text<'_>` 与
  `pub fn from_str_with_options<'a, S: StyleSheet>(input: &'a str, options: &Options<S>) -> Text<'a>`。
  同样公开的还有：`Options`、`ImageFallback`、`StyleSheet`、`DefaultStyleSheet`、`AlertKind`，
  以及（feature 门控的）`CodeTheme`、`BuiltinCodeTheme`、`CodeThemeLoadError`。返回类型是
  `ratatui_core::text::Text` —— 也就是说，调用方拿到的是 ratatui 的 line/span，而不是一个 widget。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/mod.rs#L53-L95 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/lib.rs#L52-L63
- **感知宽度的折行是调用方的事，只有一个例外。** 返回的文本可能借用输入；只有表格接受宽度
  （`Options::table_width`，且面板尺寸变化时需要重新渲染）。文档说其他 Markdown 块
  "are unaffected and can
  be wrapped by the consuming widget"。`Options` 里没有内容宽度的字段。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/lib.rs#L10-L16 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/options.rs (doc on `table_width`)
- **流式：对完整的 `&str` 做一次性解析；输出借用输入。** `from_str` 把整个字符串一次性喂给
  `Parser::new_ext`，并把事件循环跑到结束；返回的 `Text<'a>` 可以借用输入，因此除非输入是
  拥有的，它不是 `'static`。重新渲染增长中的文本意味着每次调用都要重新解析整个缓冲区。走缓冲
  路径的表格还意味着，在闭合事件到达之前无法输出表格。公开 API 里没有增量/流式入口。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/mod.rs#L72-L95 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L60-L77

## 1.5 成熟度与风险

- **发布历史：26 个版本，2024-02-27 → 2026-09-25。** 节奏不规则：头四天发了四个版本
  （0.1.0 → 0.2.1），2024 年基本按月维护，2025 年稀疏，然后在窗口最后三个月里
  0.3.8 → 0.3.9 → 0.3.10。0.3.10 本身迄今只有 913 次下载。
  https://crates.io/api/v1/crates/tui-markdown/versions · https://crates.io/api/v1/crates/tui-markdown
- **有过破坏性的 0.x 跳版。** `0.2.0`（2024-02-27）、`0.3.0`（2024-11-20，
  "Update compatibility to Ratatui 0.29"）与 `0.3.7`（2025-12-27，"Preserve heading metadata
  and update compatibility to Ratatui 0.30"）各自改动了兼容面。changelog 没有把其中任何一个
  标为 breaking，但 0.3.0 与 0.3.7 都是 ratatui 的大版本迁移。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/CHANGELOG.md ·
  https://crates.io/api/v1/crates/tui-markdown/versions
- **下载量与覆盖面：累计 498,387 次，近期窗口 149,790 次**（`recent_downloads`，
  即 crates.io 的 90 天数字）。
  https://crates.io/api/v1/crates/tui-markdown
- **维护状况：实际上是个单人项目。** workspace 的 authors 字段是
  `authors = ["Joshka"]`，仓库 owner 是 `joshka`，该 crate 创建于 2024-02-27。
  **贡献者数未核实**（本环境的 GitHub API 被限流）；0.3.8 → 0.3.10 的 changelog 条目几乎全是
  维护、依赖升级与重构。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/Cargo.toml ·
  https://crates.io/api/v1/crates/tui-markdown ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/CHANGELOG.md
- **打包后的 crate 大小：52,936 字节（0.05 MB）。** tarball 解包后约 392 KB，只包含
  `src/`、`Cargo.toml(.orig)`、`Cargo.lock`、`README.md`、`CHANGELOG.md`。
  https://crates.io/api/v1/crates/tui-markdown/0.3.10 · `tui-markdown-0.3.10.crate` from
  https://crates.io/api/v1/crates/tui-markdown/0.3.10/download
- **License：`MIT OR Apache-2.0`**，在已发布的 manifest 与仓库 workspace 中都是如此；
  仓库带有 `LICENSE-MIT` 与 `LICENSE-APACHE`。
  https://crates.io/api/v1/crates/tui-markdown/0.3.10 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/Cargo.toml

---

# 2. `ratatui-markdown` (celestia-island/ratatui-markdown) 0.3.6

## 2.1 依赖与版本兼容性

- **`ratatui ^0.29`，不是 0.30。** 已发布的 0.3.6 manifest 声明 `ratatui = "^0.29"`；
  该 crate 自己的 README 把 "ratatui 0.29" 列为前提，其随包的
  `Cargo.lock` 解析出 `ratatui 0.29.0`。仓库当前 main 分支此后已改为 `ratatui = "^0.30"` ——
  该改动在 0.3.6 **之后**，不适用于已发布的产物。
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6/dependencies ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml ·
  https://docs.rs/crate/ratatui-markdown/0.3.6/source/Cargo.lock ·
  https://github.com/celestia-island/ratatui-markdown/blob/main/Cargo.toml
- **运行时依赖：两个，另有 47 个可选的。** 始终开启：`ratatui ^0.29` 与
  `unicode-width ^0.2`。可选的、按 feature 分组的：`image 0.25`（`image`）、`pest` +
  `pest_derive`（`mermaid`、`highlight-pest`）、`serde_json` + `toml 0.8`（`tree`），以及
  `tree-sitter` 那一套（一个 `tree-sitter` 运行时加 39 个语法 crate）。
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6/dependencies
- **已发布的 `Cargo.lock` 含 264 个包**，其中包括整套可选的 tree-sitter 栈，以及仅 dev 用的
  `resvg`/`usvg`/`tiny-skia`/`font-kit`/`fontdb`/`ratatui-image` 机制。这是上界，不是运行时
  闭包；**精确的「仅运行时」传递依赖计数未核实。**
  https://docs.rs/crate/ratatui-markdown/0.3.6/source/Cargo.lock
- **是的，它依赖 `tree-sitter` 与 `tree-sitter-highlight`，版本为 `0.26`：**
  `tree-sitter = { version = "0.26", optional = true }` 与
  `tree-sitter-highlight = { version = "0.26", optional = true }`，二者由
  `highlight` feature 一起启用。随包的 lock 解析出 **`tree-sitter 0.26.9`** 与
  **`tree-sitter-highlight 0.26.9`**，外加 39 个语法 crate（`tree-sitter-rust 0.24.2`、
  `tree-sitter-python 0.25.0`、…，以及共用的 `tree-sitter-language 0.1.7`）。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L126-L127 ·
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6/dependencies ·
  https://docs.rs/crate/ratatui-markdown/0.3.6/source/Cargo.lock
- **与 `tree-sitter 0.27` 的共存：** `^0.26` 与 `^0.27` 是*不同的* semver 要求 —— 对 `0.x`
  crate，Cargo 把次版本号当作兼容边界 —— 因此 Cargo 会构建**两份** `tree-sitter`
  （`0.26.x` 与 `0.27.x`），而不会把它们统一。`tree-sitter-highlight 0.26` 同样无法与 `0.27`
  统一。当前 main 分支已经升到 `tree-sitter = "^0.27"` / `tree-sitter-highlight = "^0.27"`，
  那本可与本仓库的 `0.27` 统一 —— 但它尚未发布。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L126-L127 ·
  https://github.com/celestia-island/ratatui-markdown/blob/main/Cargo.toml
- **MSRV：`rust-version = "1.74"`。** Edition 2021。这是三个候选中最低的 MSRV。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L1-L8 ·
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6

## 2.2 表格

- **当 `markdown` feature 开启时表格会被渲染（它在默认集合里）。**
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L17-L34
- **解析在块存在之前就把整张表缓冲起来。** `is_table_line` 把连续的竖线行累积进
  `table_buffer`；`flush_table` 要求有分隔行，然后把分隔行*之前*的那一行拆成 `headers`、
  其后的全部拆成 `rows`，并 push 一个 `MarkdownBlock::Table { headers, rows }`。所以解析器
  确实持有整表上下文，并且把单元格存成**普通 `String`**。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/parser.rs#L485-L578 ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/types.rs#L31-L44
- **渲染同样需要整表上下文。** `render_table(headers, rows, theme)` 在*所有*行上计算
  `header_widths`、每列的 `min_widths`（最长的单个 token）与
  `natural_widths`（最长的完整单元格），然后对照 `self.max_width` 分配宽度，之后才开始输出
  任何东西。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L659-L760
- **表头区分方式：表头行加粗，再加分隔线。** 第一行以
  `base_style = Style::default().fg(theme.get_text_color()).add_modifier(BOLD)` 渲染；表头之后
  跟一条 `├──┼──┤` 线，每个表体行之后也跟一条 `├──┼──┤` 线，最后一条改写为 `└──┴──┘`。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L793-L860
- **列宽：先自适应内容，再压缩到面板宽度。** 每列取自然宽度；若总和超过可用预算，就按比例
  压缩到 `min_widths`；若有盈余，盈余按自然宽度分配。当 `max_width` 小到放不下边框与 padding
  时，`available` 回退到 80 列的预算。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L659-L760
- **单元格内的行内 Markdown 会被渲染。** 表头与表体单元格都经过
  `parse_inline_formatting`，得到的 span 覆盖在行的基础样式之上，然后用
  `wrap_styled_spans_to_width` 折行。`parse_inline_formatting` 是公开的。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L793-L860 ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/mod.rs#L16-L20

## 2.3 代码高亮

- **默认关闭。** 默认 feature 集是 `["markdown", "scroll", "tree", "preview",
  "mermaid", "image", "viewer"]` —— `highlight` 与所有 `highlight-lang-*` feature 都需显式开启。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L17-L34
- **引擎：`tree-sitter`，经 `tree-sitter-highlight`。** `TreeSitterHighlighter` 包装一个
  `Mutex<Highlighter>` 与 `CodeColors`；`CodeHighlighter` trait 返回
  `Vec<StyleSegment>`；`HighlightHooks` 把它装成 `RenderHooks::render_code_block`
  的覆盖实现，并画出自己的 `╭─ lang` / `╰─` 框。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/highlight/treesitter.rs ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/highlight/hooks.rs
- **语言按 feature 选择并静态链接。** 每个语法都是 `highlight-lang-<name>` 之后的可选 crate
  （37 个语言 feature，外加 `highlight-lang-all`）；`get_lang` 匹配围栏标签（包括 `py`、`js`、
  `ts`、`c++`、`sh` 这类别名），并在编译期读取 `X::LANGUAGE` / `X::HIGHLIGHTS_QUERY`。
  另有一个 `highlight-pest` 逃生口，用于基于 pest 的自定义高亮器。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L35-L75 ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/highlight/treesitter.rs#L1-L120
- **高亮类别映射：** `HIGHLIGHT_NAMES` 是一份固定的 36 项列表，顺序与
  `tree-sitter-highlight` 发出类别索引的顺序一致，`highlight_to_style` 把每一项映射到一个
  `CodeColors` 槽位。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/highlight/config.rs
- **关掉 `highlight` 时，围栏代码是纯文本加一个框。** `CodeBlock` 渲染一条 `╭─ <lang>`
  表头行、折行到 `max_width` 的代码行，以及一条 `╰─` 脚注，全部用 `RichTextTheme` 的颜色 ——
  没有语法着色。除了 Cargo feature 之外，没有 `Options` 层面的开关。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L309-L360 ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L530-L560

## 2.4 接口形态

- **渲染器是字节级、感知最大宽度的一次性渲染：** `MarkdownRenderer::new(max_width)`，
  然后 `.parse(&str) -> Vec<MarkdownBlock>` 与 `.render(&[MarkdownBlock], &theme) -> Vec<Line<'static>>`。
  同样公开的还有：`markdown::RenderHooks` trait（按块覆盖）、`MarkdownBlock`、
  `RichTextTheme`，以及 `highlight` 模块的 `CodeHighlighter`/`TreeSitterHighlighter`/`HighlightHooks`。
  该 crate 另外还导出无关的 widget（`scroll`、`tree`、`preview`、`viewer`、
  `mermaid`、`text_input`）。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/lib.rs ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/parser.rs#L57-L62
- **它会替你折行，折到它存下的 `max_width`。** 段落/标题走
  `wrap_text_with_inline_formatting`，代码块走 `wrap_styled_spans_to_width`，表格从 `max_width`
  得到自己的预算；测试断言 "no line exceeds max width"。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/text.rs ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/tests.rs
- **流式：一次性全文解析，但输出是拥有的（`Vec<Line<'static>>`）。** 没有增量解析器，
  也没有部分文档 API；每次调用都重新解析整个字符串。因为渲染出的行是拥有的，把它们与一个
  增长中的模型缓冲区一并存放是可行的，而 `tui-markdown` 借用的 `Text<'_>` 做不到这一点。
  `max_width` 在构造时固定，所以重新调整尺寸需要一个新的 `MarkdownRenderer`。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/mod.rs ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/src/markdown/render.rs#L78-L92

## 2.5 成熟度与风险

- **发布历史：12 个版本，全都在八天内发布（2026-05-14 → 2026-05-21）。**
  0.1.0、0.1.1、0.2.0、0.2.1、0.2.2，然后是 2026-05-20/21 的 0.3.0 … 0.3.6 —— 一天发好几个
  版本。此后约 4.5 个月没有再发布任何东西。
  https://crates.io/api/v1/crates/ratatui-markdown/versions
- **下载量：累计 6,002 次，近期 4,794 次；其中 5,615 次是 0.3.6 本身的**（也就是说，近期窗口
  几乎全是这个最新补丁，更像是 CI/镜像流量而非长尾使用）。
  https://crates.io/api/v1/crates/ratatui-markdown
- **维护状况：单一作者。** `authors = ["langyo <langyo.china@gmail.com>"]`；仓库位于
  `celestia-island` 组织下。**贡献者数未核实**（GitHub API 被限流）。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml#L1-L8
- **打包后的 crate 大小：8,369,402 字节（≈8.0 MiB）** —— 已发布 tarball 的内容显示了大头在哪。
  解包后是 9.3 MB，其中 7.9 MB 是 `examples/`：
  `examples/screenshots/mermaid-image.gif` 光这一个就有 **7,271,378 字节**，另外还有
  `examples/demo.webp`（160,490）、`examples/screenshots/*.webp`（每个约 30–135 KB）与
  `examples/logo.webp`（98,074）。`src/` 是 716 KB，`docs/` 是 552 KB。所以体积来自随包的
  截图/GIF，不是代码。
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6 ·
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6/download (`ratatui-markdown-0.3.6.crate`)
- **在当前各分支尖端，workspace 的 license 与已发布的 license 不一致，但对 0.3.6 不是这样。**
  `v0.3.6` tag 声明 `license = "MIT OR Apache-2.0"`，crates.io 对 0.3.6 报告的也是同一个；
  仓库当前的 `main` Cargo.toml 此后已改为
  `license = "SySL-1.0"`。任何钉住 0.3.6 的人拿到的都是 MIT OR Apache-2.0（随包 tarball 的
  `LICENSE` 是 Apache-2.0 文本）。
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml ·
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6 ·
  https://github.com/celestia-island/ratatui-markdown/blob/main/Cargo.toml
- **打包注意点：0.3.6 的归档是从一棵 dirty 的工作树构建的。**
  `.cargo_vcs_info.json` 记录为 `"sha1": "2609d035c42bb0c7c9b8ebf816c6717d496fb97a", "dirty": true`，
  它*不是* `v0.3.6` tag 的 commit（`9f4a2c06…`）。把已发布的源码与 tag 做 diff 显示，这里相关
  的文件（`src/lib.rs`、`src/markdown/{render,parser,inline}.rs`）逐字节相同，但不能假定每个
  文件都如此。
  https://docs.rs/crate/ratatui-markdown/0.3.6/source/.cargo_vcs_info.json

---

# 3. `markdown-ratatui` (karanabe/mira) 0.1.0

## 3.1 依赖与版本兼容性

- **运行时用 `ratatui-core ^0.1.2`，从不用完整的 `ratatui`。** 已发布的 manifest
  声明 `ratatui-core = { version = "0.1.2", default-features = false }`；`ratatui 0.30`
  （带 `crossterm`，关闭 default-features）只是给示例/测试用的 dev-dependency。README 把这当作
  一个设计点（"Its normal dependency graph uses `ratatui-core`, not a
  terminal backend or the complete `ratatui` application crate"）。
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0/dependencies ·
  https://docs.rs/crate/markdown-ratatui/0.1.0/source/Cargo.toml ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/README.md#L28-L36
- **运行时依赖有四个：** `markdown-model ^0.1.0`、`ratatui-core ^0.1.2`、
  `unicode-segmentation ^1.12`、`unicode-width ^0.2`。`markdown-model 0.1.0` 又只依赖
  `pulldown-cmark ^0.13.4`（关闭默认 feature）。`markdown-ratatui` 上**没有任何 Cargo feature**。
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0/dependencies ·
  https://crates.io/api/v1/crates/markdown-model/0.1.0/dependencies ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-model/Cargo.toml
- **已发布的 `Cargo.lock` 含 95 个包**，该数字也把 dev-dependencies 算在内
  （crossterm、signal-hook、palette、parking_lot 等）。运行时闭包很小 ——
  `markdown-ratatui` + `markdown-model` + `pulldown-cmark`（+ `pulldown-cmark-escape`）+
  `ratatui-core`/`ratatui-widgets` + 三个 unicode crate。
  https://docs.rs/crate/markdown-ratatui/0.1.0/source/Cargo.lock
- **完全没有 `tree-sitter`，也没有 `tree-sitter-highlight` 依赖。** 该 crate 没有语法高亮引擎，
  因此共存问题不存在。
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0/dependencies
- **MSRV：该 crate 是 `rust-version = "1.88"`**，而它所在的 Mira workspace
  声明 `rust-version = "1.98"`，应用 README 说完整应用需要 1.98。Edition **2024**。
  https://docs.rs/crate/markdown-ratatui/0.1.0/source/Cargo.toml ·
  https://github.com/karanabe/mira/blob/v0.1.0/Cargo.toml ·
  https://github.com/karanabe/mira/blob/v0.1.0/CONTRIBUTING.md

## 3.2 表格

- **表格被解析成一个持有整张表的结构化模型。** `markdown-model`
  在消费 pulldown-cmark 事件时构造 `BlockKind::Table(Table { alignments, header, rows })`；
  `TableHead` 被单独识别，单元格存成**行内内容**，而不是扁平字符串
  （`TableCell(Vec<Inline>)`）。
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-model/src/parse.rs#L256-L278 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-model/src/model.rs#L352-L400
- **布局同样需要预先拿到整张表。** `Layout::table` 在输出任何一行之前遍历表头与*每一*行以
  计算每列宽度，然后在两种布局之间做选择。
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L555-L600
- **表头区分方式：自己的主题样式加一条分隔行。**
  `Theme::heading`（默认青色 + 加粗）用于表头行，`Theme::text` 用于表体行；表头之后，以
  `─┼─` 拼接的横线作为单独一行输出。
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L605-L646
- **列宽：先自适应纯文本内容，放不下时回退到堆叠布局。** 每列起始于表头及各单元格净化后
  纯文本的显示宽度（下限 1）；若
  `sum(widths) + 3·(columns−1) + prefix.width() > layout.width`，或设置了
  `TablePolicy::Stacked`，表格就*不*画成网格 —— 表头用
  `" / "` 连接，每个表体单元格输出为一行带标签的 `"<header>: <cell>"`。默认是
  `TablePolicy::Auto`；另一个变体是 `Stacked`。对齐（`Alignment::Left/Right/Center`）
  在网格分支中驱动 padding。
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L55-L65 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L555-L600
- **单元格内的行内 Markdown 会被渲染**，经由 `inlines()`：`InlineKind::Code` 取
  `theme.code`，`Emphasis` 加上 ITALIC，`Strong` 加上 BOLD，链接/选区由同一个 run builder
  处理。一个测试断言对齐加堆叠回退能保住内容
  （`wide.contains("L    │ R")`、`stacked.contains("L: xxxx")`）。
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L661-L700 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/tests/render.rs#L101-L121

## 3.3 代码高亮

- **没有，也没有对应的 feature。** manifest 没有声明任何 Cargo feature，也没有高亮依赖；
  README 的策略清单涵盖 `Theme`、`CodePolicy`、`TablePolicy`、`LineLimit`，从未提到语法高亮。
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/README.md#L173-L178
- **最接近的旋钮是 `CodePolicy`：** `Wrap`（默认，在单元格边界处折行过长的代码行）或
  `Clip`（裁剪到可用宽度）。代码行除此之外使用 `Theme::code`（默认黄色）作为单一的扁平样式。
  围栏上的语言标签没有任何用途 —— 该 crate 里没有语言查找表。
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L40-L52 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L16-L33

## 3.4 接口形态

- **API：一个准备固定宽度布局的 view 类型，外加一个有状态 widget 及其 state。**
  `MarkdownView::new(&str) -> Result<Self, ParseError>` 解析一次；
  `MarkdownView::prepare(width: u16) -> Result<&Layout, LayoutError>` 保留一份缓存的
  布局；`Layout::widget() -> MarkdownWidget` 实现
  `StatefulWidget<State = ViewState>`；`Layout` 暴露 `headings()`、`links()`、`width()`、
  `line_count()`、`plain_lines()`。`LayoutOptions { theme, code, tables, line_limit }` 与
  `LineLimit::new(n)`（上限 200_000，默认 100_000）是公开的，`Document`、
  `HeadingId`、`LinkId`、`ParseError`、`LinkPosition`、`CellRange`、`DocumentRow`、
  `sanitize_terminal_text` 也是。
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/lib.rs ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/widget.rs ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/layout.rs#L100-L150
- **折行是这个 crate 的事，但只针对你交给它的那个宽度：** `prepare(width)` 恰好按该宽度
  布局；`MarkdownWidget::render` 裁进更窄的区域且不会重新折行。"Call `MarkdownView::prepare` with the width of the exact area passed to the widget."
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/README.md#L162-L165 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/widget.rs#L80-L100
- **流式：明确就是「每份文档、每个宽度只布局一次」。** `prepare` 缓存一份布局，
  `set_document` 无条件丢弃它；`ViewState` 的滚动/选区 "never parse or
  lay out content"。没有增量解析，也没有免于宽度变化的 API —— 但有一条干净的
  "replace the document and re-layout" 路径，解析出的 `Document` 可以通过 `Arc`
  在多个 view 之间共享。
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/src/lib.rs#L30-L90
- **刻意的非目标：** README 声明该 crate 不启用 raw mode/备用屏幕、不读输入、不挑选键位、
  不打开链接、不碰文件/网络、不启动 async 运行时，也从不终止进程。
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/README.md#L8-L10

## 3.5 成熟度与风险

- **发布历史：只有一个版本 0.1.0，发布于 2026-09-12**（与切 tag 同一天）。
  `markdown-model 0.1.0` 早 21 秒发布，属于同一批。
  https://crates.io/api/v1/crates/markdown-ratatui/versions ·
  https://crates.io/api/v1/crates/markdown-model/versions
- **下载量：`markdown-ratatui` 累计 84 次（近期 84 次）**；`markdown-model` 累计同样是 89 次。
  写作时这是个全新的、基本没人用的 crate。
  https://crates.io/api/v1/crates/markdown-ratatui ·
  https://crates.io/api/v1/crates/markdown-model
- **维护状况：单一作者 `karanabe`**，在 `mira` 仓库里工作（该 crate 是三个 workspace member
  之一；仓库的默认 member 是 `mira-viewer` 应用）。**贡献者数未核实**（GitHub API 被限流）。
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/Cargo.toml ·
  https://github.com/karanabe/mira/blob/v0.1.0/Cargo.toml
- **打包后的 crate 大小：26,462 字节（0.025 MB）。** tarball 包含 `src/`（4 个文件，
  约 1.2k 行）、`tests/render.rs`、两个示例、两份 license 文件，以及一个 `Cargo.lock`。
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0 ·
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0/download
- **License：`MIT OR Apache-2.0`**，tarball 中随附 `LICENSE-MIT` 与 `LICENSE-APACHE`；
  仓库把 license 文本提交进已发布的 crate（HEAD 的 commit message：
  "mira-viewer: add license texts to published crate"）。
  https://crates.io/api/v1/crates/markdown-ratatui/0.1.0 ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-ratatui/Cargo.toml

---

# 4. 本仓库现有的约束（仅作对照）

于 2026-10-01 从工作树 `/home/forty/code/fortystory/fs-agent` 读得。

| 约束 | 来源 |
|---|---|
| `ratatui = "0.30"`, `crossterm = "0.29"` (with `event-stream`) | `Cargo.toml` |
| `tree-sitter = "0.27"`, `tree-sitter-highlight = "0.27"`, `tree-sitter-rust = "0.24"` | `Cargo.toml` |
| 刻意决定：**不走 syntect 的 Oniguruma 路径**（不引入 C 构建依赖）；tree-sitter 的 Rust 语法同样会编译一个 C 解析器，这一点被接受 | `Cargo.toml` 注释；`docs/render.md`（Highlighting 一节）；`docs/highlight.md` |
| 已有的手写渲染器：`pub fn to_lines(text: &str) -> Vec<Line<'static>>`，对 `text.split('\n')` 做一次前向扫描，带一个围栏状态机 | `src/render/markdown.rs` |
| 该扫描器逐行渲染表格：`table_row()` 把一行竖线拆成单元格，`table_line()` 用 `" │ "` 把它们拼接起来；`is_table_separator()` 丢弃分隔行；**没有列宽计算，也没有整表缓冲** | `src/render/markdown.rs` |
| 已有的高亮模块目前**没有生产环境的消费者**；它使用 `tree-sitter-highlight`，当前只带 Rust 语法 | `docs/highlight.md`；`src/render/highlight.rs` |
| 渲染边界是 `src/render/`，有三种实现（headless / plain / TUI）；TUI 是 alt screen 里跑在 crossterm 上的 ratatui | `docs/render.md` |

上述与这些约束直接相关的事实，在此重述，不带任何推荐：

- 只有 `tui-markdown` 用 syntect；它随包的 feature 集会拉入 Oniguruma 后端
  （它自己的 `Cargo.lock` 中的 `onig`/`onig_sys`），而下游使用方无法通过声明 feature 去掉它，
  因为 Cargo feature 是可加的，且其 manifest 硬启用了 syntect 的默认项（§1.3）。
- 只有 `ratatui-markdown` 用 `tree-sitter-highlight`，在已发布的 0.3.6 里是 `0.26` ——
  与本仓库的 `0.27` 是不同的 semver 要求，所以两个版本都会被构建（§2.1）。其 main 分支已经
  移到 `0.27`。
- `markdown-ratatui` 没有高亮引擎，manifest 是三者中最小的；它要求 `edition = "2024"` 与
  `rust-version = "1.88"`（§3.1）。
- 三者都只在拿到整张表之后才计算表格列宽；没有一个能在纯粹逐行扫描的情况下产出对齐正确的列
  （§1.2、§2.2、§3.2）。
