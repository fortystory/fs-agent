# Markdown 渲染器的解析器接口与文法覆盖 —— 一手调研笔记

本文件记录的事实分三组：(A) `tui-markdown` 0.3.10 在关掉高亮器之后还剩哪些行为，(B)
`pulldown-cmark` 0.13 的依赖树与事件模型，以及 (C) 要扩展现有高亮器所需的十个 tree-sitter
文法 crate。它们全部只取自一手来源：crates.io 的 sparse index 与 JSON API、已发布的
`.crate` 归档（其中归一化后的 `Cargo.toml`、`bindings/rust/lib.rs` 与随包发布的 `.scm`
查询）、docs.rs 的 API 页面，以及精确 release tag 上的仓库。没有博客文章，没有二手评述。

本文件是这一轮的一手调研笔记：散文用中文，引文、标识符与命令保留原文（ADR 0004 /
ADR 0005）。它记录的是**事实，不是决定**；中间各节刻意不推荐任何路线。

凡是关于运行期行为而非文件文本的断言，都标记为 **实测**：`## Provenance` 一节中描述的两个
探针程序，是在本 workspace 里针对所点名的精确版本编译的，其输出会被引用。探针是行为的
证据，不是意图的证据；当某次测量与某条文档注释不一致时，两者都会被引用。

## 出处（Provenance）

| 主题 | 来源 | 版本 | 标识符 | 查阅日期 |
|---|---|---|---|---|
| `tui-markdown`（源码） | https://github.com/joshka/tui-markdown | 0.3.10 (2026-09-25) | 标签 `tui-markdown-v0.3.10` | 2026-10-01 |
| `tui-markdown`（行为） | 探针 `tuiprobe` — `tui-markdown = { version = "0.3.10", default-features = false }` | 解析为 0.3.10 | 下文引用的 `cargo run` 输出 | 2026-10-01 |
| `tui-markdown`（依赖） | 探针 `tuideps` — `cargo tree --edges normal` | 解析为 0.3.10；`pulldown-cmark` 0.13.4，`ratatui-core` 0.1.2 | 下文引用的 `cargo tree` 输出 | 2026-10-01 |
| `pulldown-cmark` | https://github.com/raphlinus/pulldown-cmark 与 https://crates.io/crates/pulldown-cmark | 0.13.4 (2026-05-20) | 来自 static.crates.io 的 `pulldown-cmark-0.13.4.crate` | 2026-10-01 |
| `pulldown-cmark`（行为） | 探针 `pdcprobe` — `pulldown-cmark = { version = "0.13", default-features = false }` | 解析为 0.13.4 | 下文引用的 `cargo run` 输出 | 2026-10-01 |
| 十个文法 crate | crates.io index + API、`.crate` 归档、`bindings/rust/lib.rs` | 版本见 §3.1 | 见 §3.1 的表格 | 2026-10-01 |
| 十个文法（行为） | 探针 `tsprobe` — `tree-sitter 0.27` + `tree-sitter-highlight 0.27` + 全部十个文法 | 解析版本见 §3.2 | 下文引用的 `cargo run` 输出 | 2026-10-01 |
| `tree-sitter-highlight` 内部 | docs.rs 源码页与本地解出的 registry 副本 | 0.27.0 | `src/highlight.rs` | 2026-10-01 |
| `tree-sitter-language` 内部 | 已发布的 `.crate` 归档 | 0.1.1, 0.1.5, 0.1.8 | `src/language.rs` / `language.rs` | 2026-10-01 |
| 本仓库 | `/home/forty/code/fortystory/fs-agent` 的工作树 | — | — | 2026-10-01 |

方法说明：

- **在本环境里能访问 crates.io 的 JSON API**（GitHub REST API 则不能：每个未认证请求都
  返回 `API rate limit exceeded`；同样的告诫见笔记 01）。sparse index 的行
  （`https://index.crates.io/<path>`）带有依赖与 feature，但**没有时间戳，也没有
  license**；时间戳/license/repository/owners 来自
  `https://crates.io/api/v1/crates/<name>/<version>`。
- **每一条标为「实测」的行为断言都来自实际执行代码**，因为在本 session 里，已发布的文档与
  docs.rs 上渲染出来的文档注释，至少在一处要紧的地方互相矛盾（§1.3、§1.2）。出现这种
  矛盾时，测量结果与两份文档措辞会被并列引用。
- 探针是在 workspace 本地的 `CARGO_HOME` 下运行的，因为在本 sandbox 里环境中的
  `~/.cargo/registry` 是只读的；这对依赖解析没有任何影响，唯一区别是用来构建文法解析器的
  `cc` 版本来自刚拉取的 index（`cc 1.2.67`），而不是环境缓存。
- §3.1 中的精确符号名取自各 crate 随包发布的 `bindings/rust/lib.rs`，不是取自文档。

---

# 1. 关掉高亮器的 `tui-markdown` 0.3.10

## 1.1 `default-features = false` 下的依赖

- **这个 crate 恰好只有一个默认 feature，`highlight-code`，而且 manifest 很短。**
  运行期依赖是 `itertools = "0.15"`、`pulldown-cmark = "0.13"`、
  `ratatui-core.workspace = true`（解析为 0.1.2）、`tracing = "0.1.37"`，外加三个可选依赖
  （`syntect = "5"`、`ansi-to-tui = "8"`、`document-features = "0.2.11"`），它们分别由默认
  feature 与 rustdoc 打开。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/Cargo.toml#L15-L34
- **实测：在 `default-features = false` 下，`cargo tree --edges normal` 列出 44 行 =
  本地探针 crate + 43 个 registry crate。** 完整的一阶依赖树，逐字如下：
  `itertools 0.15.0` → `either 1.18.0`；`pulldown-cmark 0.13.4` → `bitflags 2.13.2`、
  `getopts 0.2.24` → `unicode-width 0.2.2`、`memchr 2.8.3`、`pulldown-cmark-escape 0.11.0`、
  `unicase 2.9.0`；`ratatui-core 0.1.2` → `bitflags`、`compact_str 0.9.1` → `castaway 0.2.4`
  → `rustversion 1.0.23`、`cfg-if 1.0.5`、`itoa 1.0.18`、`rustversion`、`ryu 1.0.23`、
  `static_assertions 1.1.0`、`hashbrown 0.17.1` → `allocator-api2 0.2.21`、
  `equivalent 1.0.2`、`foldhash 0.2.0`、`itertools 0.14.0`、`kasuari 0.4.12` →
  `hashbrown 0.16.1`、`thiserror 2.0.21` → `thiserror-impl` → `proc-macro2 1.0.107` →
  `unicode-ident 1.0.26`、`quote 1.0.47`、`syn 3.0.6`/`syn 2.0.119`、`lru 0.18.5`、
  `strum 0.28.0` → `strum_macros 0.28.0` → `heck 0.5.0`、`thiserror 2.0.21`、
  `unicode-segmentation 1.13.3`、`unicode-truncate 2.0.1`、`unicode-width 0.2.2`；
  `tracing 0.1.44` → `pin-project-lite 0.2.17`、`tracing-attributes 0.1.31` → 同一批
  proc-macro crate、`tracing-core 0.1.36` → `once_cell 1.21.4`。注意 `pulldown-cmark` 的
  `getopts` 与 `pulldown-cmark-escape` 出现在闭包里，是因为 `tui-markdown` 是**带着默认
  feature** 引入 `pulldown-cmark` 的。
  实测于 2026-10-01；manifest 见
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/Cargo.toml#L27-L34
  已发布的依赖列表见 https://crates.io/api/v1/crates/tui-markdown/0.3.10/dependencies
- **实测：`--features highlight-code` 会把同一棵树撑到 77 行（76 个 registry crate）**，
  多出来的是 `syntect 5.3.0`（`bincode 1.3.3` → `serde 1.0.229` → `serde_core`、
  `flate2 1.1.10` → `crc32fast 1.5.2`、`miniz_oxide 0.9.1` → `adler2 2.0.1`、
  `simd-adler32 0.3.10`、`fnv 1.0.7`、`once_cell`、`onig 6.5.3` → `bitflags`、`once_cell`、
  `onig_sys 69.9.3`、`plist 1.10.1` → `base64 0.23.1`、`indexmap 2.14.2`、`quick-xml 0.42.0`、
  `serde`、`time 0.3.55` → `deranged 0.5.8`、`num-conv 0.2.2`、`powerfmt 0.2.0`、
  `time-core 0.1.9`、`regex-syntax 0.8.11`、`serde_derive`、`serde_json 1.0.151` → `itoa`、
  `memchr`、`serde_core`、`zmij 1.0.23`、`thiserror`、`walkdir 2.5.0` → `same-file 1.0.6`、
  `yaml-rust 0.4.5` → `linked-hash-map 0.5.6`）与 `ansi-to-tui 8.0.1`（`nom 8.0.0` →
  `memchr`、`ratatui-core`、`simdutf8 0.1.5`、`smallvec 1.16.2`、`thiserror`）。
  这就是本仓库已经决定不走的 Oniguruma 路线，笔记 01 的 §1.3 已经记下：Cargo feature 是
  累加的，下游消费者没有任何办法把它去掉。
  实测于 2026-10-01。
- **所以这个 feature 的代价，实测下来，是多出 33 个 crate**（76 − 43），其中
  `onig_sys` 是唯一需要构建 C（Oniguruma）的那个。两个闭包里其余 crate 都不是要构建 C
  或重度依赖 build script 的，除了 `pulldown-cmark` 的 `build.rs` —— 除非打开 `gen-tests`
  feature，它编译出来的 `main` 是空的（§2.1）。
  实测于 2026-10-01。
- **`tui-markdown` 当前随包引入的 `pulldown-cmark` 版本是 0.13.4**，要求是 `^0.13`。
  https://crates.io/api/v1/crates/tui-markdown/0.3.10/dependencies
- **MSRV 仍是 `1.88.0`**（workspace 的 `rust-version`，被继承）；这个 feature 不改动它。
  https://crates.io/api/v1/crates/tui-markdown/0.3.10

## 1.2 围栏代码块渲染成什么，以及语言标签是否保留

- **实测：一个围栏会变成三行纯文本，info string 紧贴在起始围栏上，中间没有空格。** 输入
  `"before\n\n```rust\nfn main() { let x = 1; }\n```\n\nafter\n"`
  并带上 `default-features = false` 时，产生（方括号里是显示宽度）：

  ```
  [  6] "before"
  [  0] ""
  [  7] "```rust"
  [ 24] "fn main() { let x = 1; }"
  [  3] "```"
  [  0] ""
  [  5] "after"
  ```

  ```` ```json ```` 同理得到 `"```json"` / `"{\"a\": 1}"` / `"```"`，未知标签
  ```` ```zzz-unknown-lang ```` 得到 `"```zzz-unknown-lang"` / `"body"` /
  `"```"`。无论起始围栏用什么字符、多长，结束围栏永远是 `"```"`。来源：
  `start_codeblock` 接收 `CodeBlockKind::Fenced(lang)`，用 `lang` 做高亮器查找，然后发出
  `format!("{fence}{lang}")`，其中 `fence` 是
  `StyleSheet::code_block_fence()`，默认值为 `"```"` —— 所以是 `"```rust"`，不是
  `"``` rust"`。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L48-L69 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/style_sheet.rs#L103-L105
- **语言标签只能靠对这一行做字符串抓取来获得；没有任何公开的结构化 API 会说「这是一段
  代码块，语言是 X」。** 公开接口是
  `pub use crate::renderer::{from_str, from_str_with_options}`，加上 `Options`、
  `ImageFallback`、`StyleSheet`、`DefaultStyleSheet`、`AlertKind` 以及（受 feature 门控的）
  `CodeTheme`、`BuiltinCodeTheme`、`CodeThemeLoadError`；返回类型是
  `ratatui_core::text::Text<'a>`，一个朴素的 `Line` 列表。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/lib.rs#L52-L63 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/mod.rs#L53-L95
  在内部，这个事实在边界处就被销毁了：`start_codeblock` 把
  `CodeBlockKind::Fenced(lang)` 匹配成一个 `&str`，随即把它格式化进围栏行，所以任何字段都
  不再保留它。本笔记 A 组问题的 §15 结论是：**info string 仍可作为文本取到；块结构则完全
  取不到。**
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L52-L68
- **缩进代码块也会被变成围栏，语言为空，而且它的各行会被直接拼接、不换行。** 实测：输入
  `"text\n\n    indented code\n    more\n"`
  产生 `[4] "text"`、`[0] ""`、`[3] "```"`、`[17] "indented codemore"`、`[3] "```"` ——
  也就是 `"indented code" + "more"` 在同一行里。这就是 `start_codeblock` 中的
  `CodeBlockKind::Indented => ""` 分支；这种拼接与「文本事件按行逐个到达，而无 feature 的
  路径交给 `line_styles`、不自己推入行」是一致的。同样的输入在 feature 打开时没问题，
  因为 `push_highlighted_text` 会按 `LinesWithEndings` 切分。`src/renderer/code.rs` 被引用
  的是那个 match 分支与样式栈的 push；拼接本身是在这里实测的，没有任何 crate 文档提到它。
  实测于 2026-10-01；
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L52-L58
- **无 feature 路径不会丢掉代码样式：** `self.line_styles.push(self.styles.code())`
  只在 `#[cfg(not(feature = "highlight-code"))]` 下发生，而默认代码样式是
  `Style::new().white().on_black()`。所以「关掉高亮」指的是白字黑底的平铺代码，而不是无样式
  文本。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/code.rs#L57-L58 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/style_sheet.rs#L60-L62

## 1.3 `Options::table_width` 的精确语义

- **这个设置自己的文档注释说它会折行："Wraps table cells to fit within `width`
  terminal columns."** 字段是 `Options<S>` 上的 `pub(crate) table_width: Option<u16>`，
  通过 `Options::table_width(width)` 设置；"the budget includes borders, cell
  padding, and enclosing list or blockquote prefixes"；resize 时要重新渲染。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/options.rs#L103-L124
- **实测：它是一个列宽预算，实现按整字素列来花这笔预算；它不是截断。** `end_table` 计算
  `width = table_width − (prefix_width + indent)` 并交给 `TableBuilder::render`；
  `fit_columns` 计算 `budget = width.saturating_sub(3 * columns + 1)`，若自然宽度已经放得下
  就提前返回，否则把每一列设为它的 `minimum_width()`（最宽不可分割字素，下限 1），然后
  在 `remaining = budget.saturating_sub(sum) > 0` 期间，一次一列地加宽最窄的未完成列；
  当 `budget < sum` 时 `for _ in
  0..remaining` 循环什么都不做，所以即使列最小值超出面板，它们也仍然成立。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L60-L77 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L192-L219
- **实测，用 crate 自带测试里那张 2 列、129 列宽的表：** 请求
  `table_width(40)` 得到每一行正好 40 列宽，`table_width(19)`（3 列对齐测试）得到每一行
  正好 19，`table_width(6)` 得到 **9**，`table_width(1)` 与 `table_width(0)` 也得到 **9**。
  所以「内容从不截断」成立，但**输出可以超出请求的预算**，而且这个下限是列数的函数、不是
  常数：当 `columns = n` 时，最窄可能的表是 `Σ minimum_width + 3n + 1` 列，也就是当每个
  单元最宽的字素都是 1 列时为 `4n + 1`。针对这种情况，文档注释的措辞是："If the budget
  cannot fit the borders, padding, and one grapheme per column, the table uses that minimum
  width instead. Content is never truncated, including at width zero."
  实测于 2026-10-01；
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/options.rs#L108-L111
- **实测：`table_width` 是唯一的宽度感知旋钮，其余一切都原样不折行地返回。** 一个 200 字符
  的段落返回为**一个**宽度 200 的 `Line`，围栏内一行 59 字符的代码返回为宽度 59 的一行
  （见 §1.2）。这与 options 文档（"Other Markdown blocks are unaffected and can be wrapped
  by the consuming widget"）以及笔记 01 §1.4 一致。
  实测于 2026-10-01；
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/options.rs#L103-L108
- **因此，对本仓库的要求「凡是我们引入的渲染器都必须交回未折行的行」而言：** 这对段落、
  标题、列表、引用与代码成立，对表格则**不**成立 —— 表格会在单元内折行到你传入的宽度
  （或者在你什么都不传时保持自然宽度）。

## 1.4 单元内折行支持：有，以及它的精确算法

- **有。** 当单元已经放得下时，`TableCell::wrap(width)` 返回一个克隆的 `TableCell`；否则把
  单元的 span 拆成 `StyledGrapheme` 并用 `cell_line_end` 逐个走，后者优先选择仍放得下的
  最后一个空白边界，否则在字素边界处切开；`from_graphemes` 会把样式相同的相邻片段重新合并，
  这样折行后的单元不会变成一个字符一个 span；折行边界处的空白会被丢弃，而不是带到下一行。
  行随后会被撑到该行最高的单元（`height = wrapped.iter().map(Vec::len).max()`），较矮的单元
  渲染为带样式的空白填充。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L306-L355 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L376-L391 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L425-L452
- **实测单元确实会折行，且对齐保持不变**；对
  `| L | R | C |` / `| :-- | --: | :-: |` / `| a bb ccc | a bb ccc | a bb ccc |` 施加
  `table_width(19)` 得到
  `│ L   │   R │  C  │`、`│ a   │   a │  a  │`、`│ bb  │  bb │ bb  │`、`│ ccc │ ccc │ ccc │`，
  而 crate 自己的测试正好钉住了这一点。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L515-L538
- **折行是按字素做的，经由 `ratatui-core` 的
  `Span::styled_graphemes`/`Span::width` 使用 `unicode-segmentation`/`unicode-width`**，所以
  CJK 宽度由 ratatui 处理，而不是靠一个临时凑的计数；crate 的测试里有专门的 CJK 与 emoji
  用例。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L815-L830 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L888-L910

## 1.5 表格会丢掉 `:---:` / `---:` 对齐吗？

- **不会 —— 对齐从 `pulldown-cmark` 一路传过来，并作为 padding 应用。** 表格从
  `Vec<Alignment>` 开始（`start_table(alignments)`），每个单元由
  `render_spans(width, alignment, style)` 渲染，`padding()` 把 `Left | None → (0, rest)`、
  `Right → (rest, 0)`、`Center → (left, rest−left)` 映射过去。对齐那一行本身不会被打印，
  因为 `pulldown-cmark` 从不把它作为文本发出。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L28-L34 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L454-L470
- **实测：渲染出的 padding 与分隔行一致**（上面那段 `:--`/`--:`/`:-:` 依次是左/右/居中），
  crate 的测试 `wrapped_rows_keep_alignment_and_padding` 断言了这一点。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L515-L538
- **表头与表体的区分，为完整起见：** 表头单元取 `StyleSheet::table_header()`（默认粗体青色），
  表体单元取 `table_cell()`；表头与表体之间通过 `HEADER_SEPARATOR` 字形集发出一行
  `├──┼──┤`，表格四周由 `┌─┬─┐`/`└─┴─┘` 框住。
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/renderer/table.rs#L17-L21 ·
  https://github.com/joshka/tui-markdown/blob/tui-markdown-v0.3.10/tui-markdown/src/style_sheet.rs#L176-L197

---

# 2. `pulldown-cmark` 0.13

## 2.1 依赖树、纯粹性、MSRV、license、版本

- **最新发布是 0.13.4，发布于 2026-05-20**；0.13.0 发布于 2025-02-12。
  `max_version = max_stable_version = 0.13.4`。这个 crate 在 sparse index 里共有 55 个已发布
  版本（50 个未被 yank），其中最早的一个在 crates.io 上可追溯到 2015 年。
  https://crates.io/api/v1/crates/pulldown-cmark ·
  https://crates.io/api/v1/crates/pulldown-cmark/0.13.4
- **它是纯 Rust，没有 C 构建步骤。** manifest 声明了 `build = "build.rs"`，但那个脚本的
  `main` 调用 `generate_tests_from_spec()`，而后者除非打开 `gen-tests` 否则是空函数（"If
  the `gen-tests` feature is absent, this function will be compiled down to nothing"）。
  `src/lib.rs` 带有 `#![cfg_attr(not(feature = "simd"), forbid(unsafe_code))]`，crate 里唯一
  的 `unsafe` 是 `src/firstpass.rs` 中可选开启的 SIMD 块，外加 `main.rs` 这个二进制 —— 它
  自身也是 `#![forbid(unsafe_code)]`。
  https://docs.rs/crate/pulldown-cmark/0.13.4/source/Cargo.toml ·
  https://docs.rs/crate/pulldown-cmark/0.13.4/source/build.rs ·
  https://docs.rs/crate/pulldown-cmark/0.13.4/source/src/lib.rs ·
  https://docs.rs/crate/pulldown-cmark/0.13.4/source/src/firstpass.rs ·
  https://docs.rs/crate/pulldown-cmark/0.13.4/source/src/main.rs
- **实测：在 `default-features = false` 下整个闭包是 5 个包** —— `pdc-probe`、
  `pulldown-cmark 0.13.4`、`bitflags 2.13.2`、`memchr 2.8.3`、`unicase 2.9.0`。没有 `cc`，
  没有 C 编译器调用。带上默认 feature `getopts` + `html` 后，它多出 `getopts 0.2.24`
  （→ `unicode-width 0.2.2`）与 `pulldown-cmark-escape 0.11.0`，即总共 8 个 registry crate。
  实测于 2026-10-01；声明的依赖见
  https://crates.io/api/v1/crates/pulldown-cmark/0.13.4/dependencies
- **普通依赖恰好是** `bitflags ^2`、`memchr ^2.5`、`unicase ^2.6`、可选 `getopts ^0.2`、
  可选 `pulldown-cmark-escape ^0.11`、可选 `serde ^1.0`（带 `derive`）；
  `default = ["getopts", "html"]` 而 `html = ["pulldown-cmark-escape"]`。
  `simd = ["pulldown-cmark-escape?/simd"]` 是唯一的另一个 feature。
  https://docs.rs/crate/pulldown-cmark/0.13.4/source/Cargo.toml
- **MSRV：`rust-version = "1.71.1"`，edition 2021**，已发布 manifest 与 README 里都是如此
  （"Rustc 1.71.1 or newer is required to build the crate"）。这远低于本仓库的工具链（本
  环境中 rustc 1.94.0），也低于 `tree-sitter 0.27` 自己的 1.90。
  https://crates.io/api/v1/crates/pulldown-cmark/0.13.4 ·
  https://github.com/pulldown-cmark/pulldown-cmark/blob/v0.13.4/README.md
- **License：`MIT`**（没有 `OR Apache-2.0`），registry 元数据与 manifest 里都是如此。
  https://crates.io/api/v1/crates/pulldown-cmark/0.13.4
- **Owners / 维护：** crates.io owners 是 `raphlinus`、`marcusklaas`、`Martin1887`；仓库仍是
  `raphlinus/pulldown-cmark`；README 列出的 workspace 成员是 `bench`、`dos-fuzzer`、
  `fuzz`、`pulldown-cmark`、`pulldown-cmark-escape`。
  https://crates.io/api/v1/crates/pulldown-cmark/owners ·
  https://github.com/pulldown-cmark/pulldown-cmark/blob/v0.13.4/Cargo.toml
- **下载量：生命周期 163,652,346 次 / crates.io 近期窗口 49,747,849 次**；仅 0.13.4 就有
  25,708,877。https://crates.io/api/v1/crates/pulldown-cmark

## 2.2 表格事件模型

- **`Tag::Table(Vec<Alignment>)` 每列携带一个对齐**，表头/表体的嵌套是
  `Table → TableHead → TableRow → TableCell`，没有 `TableBody` 标签（"the table body starts
  immediately after the closure of the `TableHead` tag"）；`TableCell` 则 "contain inline
  tags"。`Alignment` 是 `None | Left | Center | Right`。四个表格相关标签在各自的文档里都以
  `Options::ENABLE_TABLES` 为门控。
  https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/enum.Tag.html#variant.Table ·
  https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/enum.Tag.html#variant.TableHead
- **实测事件流（四个标签全部启用），输入为
  `| **bold** | \`code\` and [link](https://x.test/) |` / `| :-- | --: |` /
  `| ~~del~~ | ![alt](i.png) |`：**

  ```
    0 Start(Table([Left, Right]))
    1 Start(TableHead)
    2 Start(TableCell)
    3 Start(Strong)
    4 Text(Borrowed("bold"))
    5 End(Strong)
    6 End(TableCell)
    7 Start(TableCell)
    8 Code(Borrowed("code"))
    9 Text(Borrowed(" and "))
   10 Start(Link { link_type: Inline, dest_url: Borrowed("https://x.test/"), title: Borrowed(""), id: Borrowed("") })
   11 Text(Borrowed("link"))
   12 End(Link)
   13 End(TableCell)
   14 End(TableHead)
   15 Start(TableRow)
   16 Start(TableCell)
   17 Text(Borrowed("~~del~~"))
   18 End(TableCell)
   19 Start(TableCell)
   20 Start(Image { link_type: Inline, dest_url: Borrowed("i.png"), title: Borrowed(""), id: Borrowed("") })
   21 Text(Borrowed("alt"))
   22 End(Image)
   23 End(TableCell)
   24 End(TableRow)
   25 End(Table)
  ```

  由此得出两个事实：单元内的行内事件确实就是普通的行内事件；而即使这次运行**没有**设置
  `ENABLE_STRIKETHROUGH`，文本 `~~del~~` 仍以普通 `Text` 到达 —— 关于这个标志单独的情形，
  见下一条。
  实测于 2026-10-01；事件类型见
  https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/enum.Event.html
- **遍历事件的官方示例是 `examples/events.rs`**，随已发布的 crate 一起提供，这里逐字转载
  （它打印每一个事件，按嵌套深度缩进）：

  ```rust
  use std::io::Read;

  use pulldown_cmark::{Event, Parser};

  /// Show all events from the text on stdin.
  fn main() {
      let mut text = String::new();
      std::io::stdin().read_to_string(&mut text).unwrap();

      eprintln!("{text:?} -> [");
      let mut width = 0;
      for event in Parser::new(&text) {
          if let Event::End(_) = event {
              width -= 2;
          }
          eprintln!("  {:width$}{event:?}", "");
          if let Event::Start(_) = event {
              width += 2;
          }
      }
      eprintln!("]");
  }
  ```

  已发布的 crate 里有八个示例：`broken-link-callbacks.rs`、`event-filter.rs`、`events.rs`、
  `footnote-rewrite.rs`、`normalize-wikilink.rs`、`parser-map-event-print.rs`、
  `parser-map-tag-print.rs`、`string-to-string.rs`。那个把每个表格分支都点名的 `match`
  示例，是 rustdoc 从 tag-printing 示例抓来的副本，可以在 `Options` 文档里看到；docs.rs
  把它链接为 `examples/parser-map-tag-print.rs`。
  `pulldown-cmark-0.13.4.crate`（static.crates.io） ·
  https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/struct.Options.html

## 2.3 围栏的语言标签

- **它是 `Tag::CodeBlock(CodeBlockKind::Fenced(lang))`，而 `lang` 是 `CowStr<'a>`**，也就是
  说：当 info string 原样出现时从输入借用，必须构造时才拥有。`CodeBlockKind` 是
  `Indented | Fenced(CowStr<'a>)`，带有辅助方法 `is_indented()`、`is_fenced()`、
  `into_static()`。该变体的文档对载荷说得很明确："The value contained in the tag describes
  the language of the code, which may be empty."
  https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/enum.CodeBlockKind.html ·
  https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/enum.Tag.html#variant.CodeBlock
- **实测：标签就是整个 info string，逐字保留，没有别名处理，也没有归一化。**
  ```` ```rust ignore ```` 得到 `Start(CodeBlock(Fenced(Borrowed("rust ignore"))))`；
  ```` ``` ```` 得到 `Fenced(Borrowed(""))`；缩进块得到 `CodeBlock(Indented)`；
  `~~~python title="x.py"` 得到 `Fenced(Borrowed("python title=\"x.py\""))`；大小写会保留
  （````JS` → `Some("JS")`）。因此别名解析（`rs` → rust、`sh` → bash、`js` → javascript）
  **完全是消费者的责任**；crate 不带任何形式的语言表。
  实测于 2026-10-01。
- **实测：代码文本以一个 `Text` 事件到达，其载荷带一个结尾换行**
  （`Text(Borrowed("fn main() {}\n"))`），所以逐行处理就是对这份载荷做切分。

## 2.4 哪个标志门控哪个 GFM 构造

| 构造 | 标志 | 证据 |
|---|---|---|
| 表格 | `Options::ENABLE_TABLES` | 实测：`Options::empty()` + `ENABLE_GFM` 都让 `| A | B |` 保持为段落；`ENABLE_TABLES` 产生 `Start(Table([None, None]))` |
| 任务列表 | `Options::ENABLE_TASKLISTS` | 实测：没有它时 `- [x] done` 以三个独立的 `Text` 事件 `"["`、`"x"`、`"]"` 到达……；有它时则是 `Event::TaskListMarker(true)` / `(false)` |
| 删除线 | `Options::ENABLE_STRIKETHROUGH` | 实测：没有它时 `a ~~b~~ c` 是一个 `Text`；有它时是 `Text("a ")`、`Start(Strikethrough)`、`Text("b")`、`End(Strikethrough)`、`Text(" c")` |
| 脚注 | `Options::ENABLE_FOOTNOTES`（或蕴含它的 `ENABLE_OLD_FOOTNOTES`） | 实测：没有它时 `ref[^1]` 被解析为 `Link { link_type: Shortcut, dest_url: "note", id: "^1" }`；有它时是 `Event::FootnoteReference("1")` 与 `Tag::FootnoteDefinition("1")` |
| GFM 引用块告警（`> [!NOTE]`） | `Options::ENABLE_GFM` | 实测：`ENABLE_GFM` 产生 `Start(BlockQuote(Some(Note)))`；`Tag::BlockQuote` 的文档说，没有它时 kind 是 `None` |

- **`ENABLE_GFM` *不是*这四个扩展的打包开关。** 按位测试：
  `Options::ENABLE_GFM.contains(Options::ENABLE_TABLES) == false`，而且只设 `ENABLE_GFM`
  的表格仍保持为段落（上文已引）。它自己的文档把它描述为 "Misc GitHub Flavored Markdown
  features not supported in CommonMark"。
  实测于 2026-10-01；https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/struct.Options.html
- **0.13.4 里的 `Options::all()` 是这 15 个标志**
  `ENABLE_TABLES | ENABLE_FOOTNOTES | ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS |
  ENABLE_SMART_PUNCTUATION | ENABLE_HEADING_ATTRIBUTES | ENABLE_YAML_STYLE_METADATA_BLOCKS |
  ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS | ENABLE_OLD_FOOTNOTES | ENABLE_MATH | ENABLE_GFM |
  ENABLE_DEFINITION_LIST | ENABLE_SUPERSCRIPT | ENABLE_SUBSCRIPT | ENABLE_WIKILINKS`，`Debug`
  会按它们的名字打印（它们是 `u32` 上的 `bitflags` 2 值）。
  实测于 2026-10-01；https://docs.rs/pulldown-cmark/0.13.0/pulldown_cmark/struct.Options.html

## 2.5 每个候选使用的 pulldown-cmark 版本

- **`tui-markdown` 0.3.10 要求带默认 feature 的 `pulldown-cmark ^0.13`**，它的 `getopts` +
  `pulldown-cmark-escape` 就是从这里来的。
  https://crates.io/api/v1/crates/tui-markdown/0.3.10/dependencies
- **`markdown-ratatui` 0.1.0 → `markdown-model 0.1.0` → 带
  `default-features = false` 的 `pulldown-cmark ^0.13.4`**，声明在 model crate 自己的
  manifest 里（`pulldown-cmark = { version = "0.13.4", default-features = false }`）。
  https://crates.io/api/v1/crates/markdown-model/0.1.0/dependencies ·
  https://github.com/karanabe/mira/blob/v0.1.0/crates/markdown-model/Cargo.toml
- **`ratatui-markdown` 0.3.6 根本不用 `pulldown-cmark`。** 它完整的普通依赖列表是
  `ratatui ^0.29`、`unicode-width ^0.2`，以及可选分组
  `image`/`pest`/`pest_derive`/`serde_json`/`toml`/`tree-sitter`+39 个文法 —— 没有
  `pulldown-cmark` 条目。它自带一个行扫描器（`src/markdown/parser.rs`），笔记 01 的 §2.2
  已经描述过。
  https://crates.io/api/v1/crates/ratatui-markdown/0.3.6/dependencies ·
  https://github.com/celestia-island/ratatui-markdown/blob/v0.3.6/Cargo.toml
- **所以这个对比里两个使用 `pulldown-cmark` 的 crate 都要求 `0.13`；手写解析器的是
  `ratatui-markdown`。** 本仓库目前完全没有 `pulldown-cmark` 依赖（`Cargo.toml` 列的是
  `toml = "0.8"`，不是 `pulldown-cmark`）。

---

# 3. 把 `src/render/highlight.rs` 扩展到十种语言

## 3.1 十个文法 crate，一种语言一种语言地看

十个文法都在探针 `tsprobe` 中针对 `tree-sitter 0.27.0` 与 `tree-sitter-highlight 0.27.0`
一起解析并编译（运行输出见 §3.2）。下面的版本、license、owners、发布日期与仓库来自
crates.io API；符号名来自各 crate 随包发布的 `bindings/rust/lib.rs`；对
`tree-sitter-language` 的依赖要求来自 sparse index 的行
（`https://index.crates.io/tr/ee/<name>`）。

| 语言 | Crate | 版本（已发布） | Rust 符号（精确） | 声明的 `tree-sitter-language` 要求 | 有 C 解析器？ | 仓库 / owner |
|---|---|---|---|---|---|---|
| rust | `tree-sitter-rust` | 0.24.2 (2026-03-27) | `LANGUAGE`, `HIGHLIGHTS_QUERY`, `INJECTIONS_QUERY`, `TAGS_QUERY`, `NODE_TYPES` | `^0.1` | 有，`src/parser.c` + `src/scanner.c` | https://github.com/tree-sitter/tree-sitter-rust · `dcreager`, `maxbrunsfeld` |
| bash | `tree-sitter-bash` | 0.25.1 (2025-12-02) | `LANGUAGE`, **`HIGHLIGHT_QUERY`**（单数）, `NODE_TYPES` | `^0.1` | 有，`src/parser.c` + `src/scanner.c` | https://github.com/tree-sitter/tree-sitter-bash · `dcreager`, `maxbrunsfeld` |
| json | `tree-sitter-json` | 0.24.8 (2024-11-11) | `LANGUAGE`, `HIGHLIGHTS_QUERY`, `NODE_TYPES` | `^0.1` | 有，`src/parser.c` | https://github.com/tree-sitter/tree-sitter-json · `maxbrunsfeld`, `sergey-sign` |
| toml | `tree-sitter-toml-ng` | 0.7.0 (2024-12-03) | `LANGUAGE`, `HIGHLIGHTS_QUERY`, `NODE_TYPES` | `^0.1` | 有，`src/parser.c` + `src/scanner.c` | https://github.com/tree-sitter-grammars/tree-sitter-toml · `ObserverOfTime`, `github:tree-sitter-grammars:crates` |
| html | `tree-sitter-html` | 0.23.2 (2024-11-11) | `LANGUAGE`, `HIGHLIGHTS_QUERY`, `INJECTIONS_QUERY`, `NODE_TYPES` | `^0.1` | 有，`src/parser.c` + `src/scanner.c` | https://github.com/tree-sitter/tree-sitter-html · `maxbrunsfeld`, `amaanq` |
| javascript | `tree-sitter-javascript` | 0.25.0 (2025-09-01) | `LANGUAGE`, **`HIGHLIGHT_QUERY`**（单数）, `INJECTIONS_QUERY`, `JSX_HIGHLIGHT_QUERY`, `LOCALS_QUERY`, `TAGS_QUERY`, `NODE_TYPES` | `^0.1` | 有，`src/parser.c` + `src/scanner.c` | https://github.com/tree-sitter/tree-sitter-javascript · `dcreager`, `maxbrunsfeld` |
| typescript | `tree-sitter-typescript` | 0.23.2 (2024-11-11) | **`LANGUAGE_TYPESCRIPT`**, **`LANGUAGE_TSX`**, `HIGHLIGHTS_QUERY`, `LOCALS_QUERY`, `TAGS_QUERY`, `TYPESCRIPT_NODE_TYPES`, `TSX_NODE_TYPES` | `^0.1` | 有，两个文法：`typescript/src/parser.c`+`scanner.c`、`tsx/src/parser.c`+`scanner.c` | https://github.com/tree-sitter/tree-sitter-typescript · `dcreager`, `maxbrunsfeld`, `patrickt` |
| php | `tree-sitter-php` | 0.24.2 (2025-08-18) | **`LANGUAGE_PHP`**, **`LANGUAGE_PHP_ONLY`**, `HIGHLIGHTS_QUERY`, `INJECTIONS_QUERY`, `TAGS_QUERY`, `PHP_NODE_TYPES`, `PHP_ONLY_NODE_TYPES` | `^0.1` | 有，两个文法：`php/src/parser.c`+`scanner.c`、`php_only/src/parser.c`+`scanner.c` | https://github.com/tree-sitter/tree-sitter-php · `maxbrunsfeld` |
| sql | `tree-sitter-sequel` | 0.3.11 (2025-10-01) | `LANGUAGE`（由 C 符号 `tree_sitter_sql` 提供）, `HIGHLIGHTS_QUERY`, `NODE_TYPES` | `^0.1` | 有，`src/parser.c` + `src/scanner.c` | https://github.com/derekstride/tree-sitter-sql · `DerekStride` |
| python | `tree-sitter-python` | 0.25.0 (2025-09-11) | `LANGUAGE`, `HIGHLIGHTS_QUERY`, `TAGS_QUERY`, `NODE_TYPES` | `^0.1` | 有，`src/parser.c` + `src/scanner.c` | https://github.com/tree-sitter/tree-sitter-python · `dcreager`, `maxbrunsfeld` |

表里放不下的按语言说明：

- **`HIGHLIGHT_QUERY` 与 `HIGHLIGHTS_QUERY` 是这份清单里最尖锐的 API 陷阱。**
  `tree-sitter-bash` 与 `tree-sitter-javascript` 导出的是**单数**名；其余八个导出复数名。
  两个 crate 都随包发布 `queries/highlights.scm` —— 不同的只是 Rust 常量名。这是对照每个已
  发布 tarball 里的 `bindings/rust/lib.rs` 核实的，不是对照文档。
  `tree-sitter-bash-0.25.1.crate` · `tree-sitter-javascript-0.25.0.crate` (static.crates.io)
- **`toml`：用于*仍在维护*的 TOML 文法的 crate 是 `tree-sitter-toml-ng`，不是
  `tree-sitter-toml`。** `tree-sitter-toml` 这个名字只有一个版本 0.20.0，发布于
  **2022-01-05**，它直接依赖 `tree-sitter ^0.20`，无法与 0.27 统一；`tree-sitter-toml-ng`
  0.7.0 依赖 `tree-sitter-language ^0.1`。
  https://index.crates.io/tr/ee/tree-sitter-toml ·
  https://crates.io/api/v1/crates/tree-sitter-toml ·
  https://index.crates.io/tr/ee/tree-sitter-toml-ng
- **`sql`：名字与语言相符的那个 crate 已经死了；活着的那叫 `tree-sitter-sequel`。**
  `tree-sitter-sql` 有两个已发布版本，最新的 0.0.2 来自 **2021-06-05**，它直接依赖
  `tree-sitter ^0.19.3`；它没有被 yank，但已经五年没有发布。另外三个偏 SQL 的 crate 作为
  通用 SQL 文法候选被查过并否掉：`tree-sitter-sql-bigquery` 0.8.0（BigQuery 方言，
  `tree-sitter >=0.19, <0.23`）、`tree-sitter-sqlite3` 0.1.0（SQLite 方言，发布于
  2026-05-03，生命周期下载量 92，license `CC0-1.0`），以及 `sqlparser` 0.63.0（一个纯 Rust
  SQL 解析器，根本不是 tree-sitter 文法）。
  https://index.crates.io/tr/ee/tree-sitter-sql ·
  https://crates.io/api/v1/crates/tree-sitter-sql ·
  https://crates.io/api/v1/crates/tree-sitter-sql-bigquery ·
  https://crates.io/api/v1/crates/tree-sitter-sqlite3 ·
  https://crates.io/api/v1/crates/sqlparser
- **`tree-sitter-sequel` 0.3.11 发布于 2025-10-01，也就是本笔记写作的当天**；它上一个版本
  是 0.3.10。它是清单里唯一一个由 `tree-sitter` org 之外的个人维护者（`DerekStride`）维护的
  文法。它 887,739 字节的 tarball 是十个里第二大的。
  https://crates.io/api/v1/crates/tree-sitter-sequel
- **十个里有七个受 `tree-sitter` GitHub org 治理**（`rust`、`bash`、`json`、`html`、
  `javascript`、`typescript`、`php`、`python` —— 其实是八个），反复出现的 crates.io owners
  是 `dcreager` / `maxbrunsfeld`。`toml-ng` 在 `tree-sitter-grammars` 下，owner 不同
  （`ObserverOfTime`），而 `sequel` 是个人项目。所以「一个组织、统一节奏」*大体上*成立，
  但对生态历来头疼的那两门语言不成立。
  https://crates.io/api/v1/crates/tree-sitter-rust/owners ·
  https://crates.io/api/v1/crates/tree-sitter-toml-ng/owners ·
  https://crates.io/api/v1/crates/tree-sitter-sequel/owners
- **清单里没有任何文法声明 MSRV**（十个的 `rust_version` 都是 `null`）；十个都是
  `edition = "2021"` 且 `license = "MIT"`，除了 `tree-sitter-sqlite3`（`CC0-1.0`，未被
  选中）。把它们绑到工具链上的是 `tree-sitter-language` 的 `^0.1` 要求，而
  `tree-sitter-language 0.1.8` 本身位于 `tree-sitter 0.27.0` 自己的 `^0.1.8` 要求之内。
  https://index.crates.io/tr/ee/tree-sitter-rust · https://crates.io/api/v1/crates/tree-sitter/0.27.0/dependencies
- **十个里的每一个都通过 `cc` build script 构建一个 C 解析器**（十个都把 `cc ^1.1`/`^1.2`
  列为 build dependency；`ratatui-markdown` 对同一路线的存在性证明见笔记 01 §2.3）。在这里
  编译它们的是 `gcc 16.2.1`。所以「闭包里没有 C 构建步骤」对十个里的任何一个都*不*成立
  —— 这与本仓库在 `Cargo.toml` 里已经为 `tree-sitter-rust` 明确做过的取舍是同一个。

## 3.2 与 `tree-sitter 0.27` 的兼容性 —— 靠编译验证，不是靠阅读

- **实测：十个文法全部能编译、链接，并针对 `tree-sitter 0.27.0` /
  `tree-sitter-highlight 0.27.0` 产出可用的 `HighlightConfiguration`，用的是本仓库
  `rust_config()` 的精确调用形式**（`HighlightConfiguration::new(lang,
  name, highlights, injections, locals)`，`LanguageFn` 用 `LANGUAGE.into()`）。探针 `tsprobe` 跑了十二个配置
  —— 十个语言加上 `tsx` 与 `php_only` —— 每一个都返回 `ok`：

  ```
  rust       ok           first-config-ms=55.23
  bash       ok           first-config-ms=13.27
  json       ok           first-config-ms=0.05
  toml       ok           first-config-ms=0.27
  html       ok           first-config-ms=0.14
  javascript ok           first-config-ms=38.37
  typescript ok           first-config-ms=12.60
  tsx        ok           first-config-ms=11.82
  php        ok           first-config-ms=70.22
  php_only   ok           first-config-ms=64.12
  sql        ok           first-config-ms=38.61
  python     ok           first-config-ms=13.81
  ```

  探针是 debug 构建；因此这些数字是 release 构建的上界，但*相对*次序以及「每个查询都能
  解析」这一点才是要点。
  实测于 2026-10-01。
- **实测：整个十文法集合解析出的 lockfile 里恰好有一个 `tree-sitter`、一个
  `tree-sitter-highlight` 和一个 `tree-sitter-language`**：`tree-sitter 0.27.0`、
  `tree-sitter-highlight 0.27.0`、`tree-sitter-language 0.1.8`、`tree-sitter-rust 0.24.2`、
  `tree-sitter-bash 0.25.1`、`tree-sitter-json 0.24.8`、`tree-sitter-toml-ng 0.7.0`、
  `tree-sitter-html 0.23.2`、`tree-sitter-javascript 0.25.0`、`tree-sitter-typescript 0.23.2`、
  `tree-sitter-php 0.24.2`、`tree-sitter-sequel 0.3.11`、`tree-sitter-python 0.25.0`，外加
  `cc 1.2.67`。总共 38 个包。没有重复的文法运行期，没有版本错位。
  实测于 2026-10-01。
- **为什么能统一：`LanguageFn` 自 0.1.1 起 ABI 与 API 就没变过。** 它的定义是
  `#[repr(transparent)] pub struct LanguageFn(unsafe extern "C" fn() -> *const ())`，带有
  `pub const unsafe fn from_raw` 与 `pub const fn into_raw`；从 0.1.1 到 0.1.8 的差异只有
  文档注释标点、一处新增的 `#[must_use]`，以及一个 `build.rs`。`tree-sitter 0.27.0` 自身
  要求 `tree-sitter-language ^0.1.8`，而 `tree-sitter-language 0.1.8` 是 `edition
  2024` 与 `rust-version = "1.90"`（所以 0.27 的闭包把工具链下限抬到 1.90，本仓库的 rustc 1.94.0
  已经满足）。
  `tree-sitter-language-0.1.1.crate` 与 `tree-sitter-language-0.1.8.crate` (static.crates.io) ·
  https://crates.io/api/v1/crates/tree-sitter/0.27.0/dependencies ·
  https://crates.io/api/v1/crates/tree-sitter-language/0.1.8
- **转换不在 `tree-sitter-language` 里，而是 `tree-sitter` crate 里的
  `impl From<LanguageFn> for Language`**，这就是为什么调用处的 `*.LANGUAGE.into()` 能把文法
  的 `^0.1` 要求与运行期的次版本解耦。
  https://docs.rs/tree-sitter/0.27.0/tree_sitter/struct.Language.html
- **`HighlightConfiguration::new` 接受一个 `Language`（按值），其精确签名是：**
  `pub fn new(language: Language, name: impl Into<String>, highlights_query: &str,
  injection_query: &str, locals_query: &str) -> Result<Self, QueryError>`。内部它把三个查询
  字符串拼接起来，对合并后的查询调用一次 `Query::new`（编译步骤），对 injections 查询再
  调用一次，然后扫描 pattern 属性设置里的 `injection.combined` 与 `local`。
  `configure(recognized_names)` 随后遍历查询的捕获名，在点分组件上挑最长的已识别匹配，
  构造出一个 `Vec<Option<Highlight>>` —— 这正是 `Class::of` 所假设的「最长前缀胜出」行为。
  https://docs.rs/tree-sitter-highlight/0.27.0/src/tree_sitter_highlight/highlight.rs.html ·
  （`tree-sitter-highlight-0.27.0/src/highlight.rs` 的本地副本）
- **初始化成本，实测，是每个语言的查询编译：** 0.05 ms（json）到 70 ms（php）、55 ms
  （rust），在 debug 构建下。`Highlighter::new()` 很便宜（`Parser::new()` + 空的 `Vec`），
  其文档说每个线程复用一个；本仓库当前的 `try_highlight` 每次调用都构造一个新的
  `Highlighter`，只在配置上依赖 `OnceLock`。十个 crate 里没有任何东西改变这一形态。
  实测于 2026-10-01；`tree-sitter-highlight-0.27.0/src/highlight.rs` 的本地副本
  （`Highlighter::new`，以及文档注释 "For the best performance `Highlighter` values should
  be reused between syntax highlighting calls. A separate highlighter is needed for each
  thread that is performing highlighting.")
- **按本仓库想要的粒度做懒加载是可行的：** 现有模式是每个语言一个
  `OnceLock<Option<HighlightConfiguration>>`（`rust_config()`），而由于
  `HighlightConfiguration` 是 `Send + Sync`（"This struct is immutable and can be shared
  between threads"），十个这样的 `OnceLock` 就能给出按语言懒编译，除 `OnceLock` 之外不需要
  额外加锁。`HighlightConfiguration` 的字段是 `language`、`language_name`、`query`、可选的
  合并 injections 查询、各个偏移，以及捕获索引向量 —— 一个已配置语言的常驻成本就是解析后
  的 `Query` 加上几个向量。
  `src/render/highlight.rs#L190-L206` ·
  `tree-sitter-highlight-0.27.0/src/highlight.rs` 的本地副本
- **injection 按语言可选，留空是合法的。** Rust、HTML、JavaScript 与 PHP 随包提供
  `INJECTIONS_QUERY`；bash、json、toml-ng、typescript、sequel 与 python 没有。构造器的文档
  说 injections 查询 "can be empty if no injections are desired"，而探针对这六个传了 `""`，
  没有报错。若为比如 HTML 传入非空 injections 查询，还额外要求接上 `highlight` 回调的
  injection 处理（`Highlighter::highlight` 接受
  `injection_callback: impl FnMut(&str) ->
  Option<&'a HighlightConfiguration>`），而本仓库
  的 `try_highlight` 目前用 `|_| None` 满足它。
  `src/render/highlight.rs#L219-L224` ·
  https://docs.rs/tree-sitter-highlight/0.27.0/src/tree_sitter_highlight/highlight.rs.html

## 3.3 捕获名相对本仓库 `CAPTURES` 的覆盖率

本仓库的 `CAPTURES` 列表（`src/render/highlight.rs#L24-L52`）由
`tree-sitter-highlight` 的点分组件规则来匹配，所以 `punctuation.special` 映射到
`punctuation`、`type.qualifier` 映射到 `type`，而没有任何已识别组件的捕获映射到
`Class::Plain`，只是不上色。这是对照每个文法自己的 `queries/highlights.scm` 中出现的每一个
`@name` 实测的：

| 文法 | 不同捕获数 | 本仓库列表匹配不上的捕获 |
|---|---|---|
| rust | 21 | 0 |
| bash | 9 | 0 |
| json | 6 | 0 |
| toml-ng | 10 | 0 |
| html | 7 | `tag`, `tag.error` |
| javascript | 19 | 0 |
| typescript | 5 | 0 |
| php | 19 | `module`, `module.builtin`, `tag` |
| sequel | 21 | `conditional`, `field`, `float`, `parameter`, `spell`, `storageclass` |
| python | 17 | 0 |

- 十个文法里有六个对本仓库的词表覆盖完美；漏掉的是 HTML 标签（对本产品来说最显眼的一处）、
  PHP 命名空间/`?>` 标记，以及 SQL 的
  `conditional`/`field`/`float`/`parameter`/`storageclass`/`spell`。注意
  `tree-sitter-sequel` 的查询用的是 `conditional` 与 `keyword.operator`，而 Rust 的会用
  `keyword`/`operator`；前者落入 `Plain`。
  实测于 2026-10-01；查询来自已发布的 tarball（`tree-sitter-html-0.23.2.crate`、
  `tree-sitter-php-0.24.2.crate`、`tree-sitter-sequel-0.3.11.crate`, static.crates.io）
- **`tree-sitter-typescript` 的五个捕获只是 TypeScript 特有的补充**；这门语言的完整高亮来自
  TypeScript 查询引用 JavaScript 的节点名，而 `tree-sitter-javascript` 文法自己的 query/JSX
  query 是分开的常量。因此任何 TypeScript 配置都要决定是否拼接
  `JSX_HIGHLIGHT_QUERY`/`highlights-jsx.scm`（它不属于 `HIGHLIGHTS_QUERY`）。
  `tree-sitter-typescript-0.23.2.crate` · `tree-sitter-javascript-0.25.0.crate`

## 3.4 兼容性风险表

行是十个目标语言；「锁定的（运行期）」是 crate 对运行期声明的要求（来自它最新版本的
sparse-index 行），「与 `tree-sitter 0.27` 兼容」是 §3.2 的实测结果，最后一列是需要注意什么。

| 语言 | crate | 版本 | 锁定的（运行期） | 与 `tree-sitter 0.27` | 有 `HIGHLIGHTS_QUERY`？ | 风险 |
|---|---|---|---|---|---|---|
| rust | `tree-sitter-rust` | 0.24.2 | `tree-sitter-language ^0.1` | **兼容** | 有 | 已是本仓库依赖；风险最低 |
| bash | `tree-sitter-bash` | 0.25.1 | `tree-sitter-language ^0.1` | **兼容** | 有，**名为 `HIGHLIGHT_QUERY`** | 常量名与其余九个不同 |
| json | `tree-sitter-json` | 0.24.8 | `tree-sitter-language ^0.1` | **兼容** | 有 | 最后发布 2024-11；文法已冻结但稳定 |
| toml | `tree-sitter-toml-ng` | 0.7.0 | `tree-sitter-language ^0.1` | **兼容** | 有 | 避开 `tree-sitter-toml` 这个名字（0.20.0 → `tree-sitter ^0.20`，自 2021 年起已死）；toml-ng 在 tree-sitter org 之外，只有 2 个发布 |
| html | `tree-sitter-html` | 0.23.2 | `tree-sitter-language ^0.1` | **兼容** | 有 | 它 7 个捕获里有 2 个（`tag`、`tag.error`）对本仓库的 `CAPTURES` 不可见 |
| javascript | `tree-sitter-javascript` | 0.25.0 | `tree-sitter-language ^0.1` | **兼容** | 有，**名为 `HIGHLIGHT_QUERY`** | 还随包发布 `JSX_HIGHLIGHT_QUERY`，它*不*在主查询里；常量名不同 |
| typescript | `tree-sitter-typescript` | 0.23.2 | `tree-sitter-language ^0.1` | **兼容** | 有 | 两个 `Language`（`LANGUAGE_TYPESCRIPT`、`LANGUAGE_TSX`）；crate 自己的查询只有 5 个捕获，所以它依赖 JavaScript 文法的节点名；最后发布 2024-11 |
| php | `tree-sitter-php` | 0.24.2 | `tree-sitter-language ^0.1` | **兼容** | 有 | 两个 `Language`（`LANGUAGE_PHP`、`LANGUAGE_PHP_ONLY`）；3 个捕获未映射；70 ms 是实测中最贵的一次首次配置 |
| sql | `tree-sitter-sequel` | 0.3.11 | `tree-sitter-language ^0.1` | **兼容** | 有 | crate 名 ≠ 语言名；单一维护者；6 个未映射捕获；同名的 `tree-sitter-sql` 不可用；替代品要么是方言专用的，要么根本不是 tree-sitter |
| python | `tree-sitter-python` | 0.25.0 | `tree-sitter-language ^0.1` | **兼容** | 有 | 未观察到任何问题 |

- **任务预期的「文法锁住一个旧 tree-sitter」风险在这十个 crate 上没有出现，但对那些凭直觉
  会去拿的 crate 名却是真实存在的：** `tree-sitter-toml`（0.20.0，2022-01-05 →
  `tree-sitter ^0.20`）、`tree-sitter-sql`（0.0.2，2021-06-05 → `tree-sitter ^0.19.3`）、
  `tree-sitter-markdown`（0.7.1，2021-04-18 → `tree-sitter ^0.19`）、
  `tree-sitter-sql-bigquery`（→ `tree-sitter >=0.19, <0.23`）。把活文法与死文法区分开的正是
  `tree-sitter-language ^0.1` 这层垫片。
  https://index.crates.io/tr/ee/tree-sitter-toml ·
  https://crates.io/api/v1/crates/tree-sitter-toml ·
  https://index.crates.io/tr/ee/tree-sitter-sql ·
  https://index.crates.io/tr/ee/tree-sitter-markdown ·
  https://crates.io/api/v1/crates/tree-sitter-markdown ·
  https://index.crates.io/tr/ee/tree-sitter-sql-bigquery
- **另一个真实风险是 C 工具链，而不是版本错位：** 十个 `cc` build script 就是首次构建时十次
  C 解析器编译。本仓库已经为一个文法接受了这一点（`Cargo.toml`："这个文法要编一个 C
  parser，所以首次构建比纯 Rust 依赖慢"），而 `ratatui-markdown` 为其中 39 个做了同样的
  取舍；但笔记 01 里与 `pulldown-cmark` 的对比（总共 5 个纯 Rust crate，§2.1）才是诚实的
  那个。
  `Cargo.toml` · https://crates.io/api/v1/crates/ratatui-markdown/0.3.6/dependencies

---

# 4. 本仓库现有的约束（背景，不是建议）

| 约束 | 来源 |
|---|---|
| `ratatui = "0.30"`、`crossterm = "0.29"`（带 `event-stream`） | `Cargo.toml` |
| `tree-sitter = "0.27"`、`tree-sitter-highlight = "0.27"`、`tree-sitter-rust = "0.24"`；lockfile 钉住 0.27.0 / 0.27.0 / 0.24.2 / `tree-sitter-language 0.1.8` | `Cargo.toml`, `Cargo.lock` |
| 有意做出的决定：**不走 syntect 的 Oniguruma 路线**；接受 tree-sitter Rust 文法的 C 解析器 | `Cargo.toml` 注释；`docs/render.md`；`docs/highlight.md` |
| 渲染器接口是 `pub fn to_lines(text: &str) -> Vec<Line<'static>>` —— 对 `text.split('\n')` 做一遍单向遍历，带一个围栏状态机；表格逐行渲染（`table_row()`/`table_line()` 用 `" │ "` 连接单元，`is_table_separator()` 丢掉分隔行），**没有列宽计算，也没有整表缓冲** | `src/render/markdown.rs#L20-L75`, `#L96-L112` |
| 当前的折行器是 `pane::wrap_text(text, width) -> Vec<Line<'static>>`（不是 `wrap_line`），列宽算术住在 `render/width.rs`（`text_columns`、`char_columns`、`truncate_columns`，构建在 `ratatui::buffer::CellWidth` 之上） | `src/render/pane.rs#L353`, `src/render/width.rs` |
| 现有高亮器没有生产消费者；它的配置是只给 Rust 用的单个 `OnceLock<Option<HighlightConfiguration>>`（`rust_config()`），而 `CAPTURES` 是那份 27 个名字的列表 | `docs/highlight.md`, `src/render/highlight.rs#L24-L52`, `#L190-L206` |

以上与这些约束相关的事实，不带建议地重述如下：

- `tui-markdown` 在 `default-features = false` 下会让代码围栏、段落与所有其他块保持不折行，
  并交回 `Text<'_>`；唯一的宽度感知路径是表格，它在设置了 `Options::table_width` 时在单元内
  折行，而当每列的字素下限放不下时，可以超出该预算（§1.1–§1.3）。
- `pulldown-cmark` 0.13.4 在 `default-features = false` 下是 5 个纯 Rust crate，把围栏语言
  标签作为 `CowStr` 携带，按列暴露表格对齐，并要求消费者自备别名表；四个 GFM 构造各自需要
  自己的标志，而 `ENABLE_GFM` 不蕴含它们（§2.1–§2.5）。
- 十个目标文法全部能针对本仓库的 `tree-sitter 0.27` / `tree-sitter-highlight 0.27` 编译，
  并导出 highlights 查询，其中有两个常量名例外、两个 crate 各自导出两个 `Language`，最坏
  情况下有六个未映射的捕获名（§3.1–§3.4）。
