# 解析层换血：`pulldown-cmark` 进场，渲染器重写成事件驱动

Type: implement
Status: done

> 规格：`.scratch/markdown-render/spec.md` §1 与 §7。
> 落地 [ADR 0008](../../../docs/adr/0008-markdown-parsing-by-pulldown-cmark.md)。
> **这一票只换发动机，不动外观**：表格暂时仍是 `│` 连接、代码块暂时仍是单色、缩进照旧——02 / 03 / 05 / 06 才逐项替换它们。

## 目标

[`src/render/markdown.rs`](../../../src/render/markdown.rs) 从手写的单趟扫描器变成**事件驱动的渲染器**；`to_lines` 开始接收可用宽度；宽度变化时那些宽度敏感的块能被重渲染。外观与今天等价。

## 落点

- `Cargo.toml`：加 `pulldown-cmark = { version = "0.13", default-features = false }`（**不要**开 `simd`，那会引入 `unsafe`）。
- `src/render/markdown.rs`：重写。这些手写判定全部删除：`is_rule`、`blockquote`、`list_item`、`task_box`、`heading`、`opening_fence`、`closes_fence`、`table_row`、`is_table_separator`、`inline_spans`、`strong_marker`、`emphasis_marker`、`flush`。
- `src/render/tui.rs`：`paint_block`（第 3644 行起）与它唯一的调用点（第 1332 行那一段）。
- `tests/render_markdown.rs`：14 处 `to_lines` 调用补上宽度参数。

## 具体行为

1. **旗标**：`Options::ENABLE_TABLES | ENABLE_TASKLISTS | ENABLE_STRICKETHROUGH`。**不要**加 `ENABLE_GFM`——实测它不是那三件的总开关（只开它时表格仍是段落）。**不要**加 `ENABLE_FOOTNOTES`。
2. **签名**：`pub fn to_lines(text: &str, width: u16) -> Vec<Line<'static>>`。`width` 是转录内容的可用列数。
3. **事件驱动的块级状态机**：遍历 `Parser::new_ext` 的 `Event`，维护一个 Tag 栈。`Start(Tag::Table)` 与 `Start(Tag::CodeBlock)` 进入**缓冲态**（整块收齐才产出），其余块逐行产出。`Text` 事件带行尾 `\n`——那是**行边界**，不是文本内容。
4. **本票内各类块的产出照旧**（后续票会替换）：
   - 标题：`heading_line` 的逻辑照搬（`#` / `##` 青色 + 粗体，更深的只粗体）；
   - 段落：行内 span 直出；
   - 列表：`• ` / `N. ` / `☐ ` / `☑ `；缩进按**嵌套深度 × 2 格**（解析器给的是结构，没有原始缩进）；
   - 引用：`│ ` + 灰色；
   - 分隔线：`─` × 24 灰色；
   - **表格**：暂时照今天的做法 `cells.join(" │ ")`，并**照样丢掉表头信号**（票 02 替换这一段）；
   - **代码块**：暂时照今天的做法——每行 `"  "` + 整行 `Color::Yellow`；但**要把 info string 解析出来存着**（票 03 要用）。
5. **行内**：`Code` → 黄色；`Strong` → bold；`Emphasis` → italic；`Strikethrough` → crossed out；`Link` → 下划线标签 + 灰色 ` (url)`（url 与标签相同时不补）。`Image` 这一票先照 `Link` 走（票 06 修）。
6. **透传**：不认识的 Tag（HTML、脚注、定义列表）**原样输出其文本内容**，一个字不丢。这是旧扫描器最值钱的性质，重写后必须保住。
7. **`push_block(block, width)`**：把第 1332–1340 行那一段（`paint_block` → `pane.push` → `links.push_back` → `turn_rail.push_line` → `prune_links`）抽成一个函数。`Tui` 保留 `Vec<Block>`；**当 `draw` 发现当前的转录内容宽度与上次渲染用的宽度不同**时，清空 `pane` / `links` / `turn_rail`，按新宽度重放全部块。
   - 宽度与 `Pane::view` 的入参同源：从 `frame.area()` 经 `layout::plan` 算出的转录内容宽度。
   - 内存提醒：`Block::Tool` 那支可能不小（工具输出 preview）。评估是否只保留**宽度敏感**的块，但**先按「全量重放」实现**——简单可靠优先。
8. **`Pane` 的源行不再宽度无关**：这一点要写进 `Pane` 或 `Tui` 的文档注释，并交叉引用 spec §1。它推翻 [`.scratch/tui-layout/spec.md`](../../tui-layout/spec.md) §3 的「源行缓冲……宽度无关，可复现」。

## 测试

- `tests/render_markdown.rs` 全部补宽度参数（建议 `const W: u16 = 78;`）。
- **三条现有测试必须原样通过**，它们是这次重写的安全网：`a_fenced_block_is_kept_verbatim_and_never_parsed_as_markdown`、`malformed_markdown_degrades_to_plain_text_rather_than_vanishing`、`quotes_rules_and_tables_render_as_structure`（本票里表格行为不变，「分隔那一行被丢掉」那条断言仍然成立）。
- **逐字等价断言**（旧扫描器的行为，重写后一条都不能变）：`# Title` → `Title`；`### Note` 只粗体；`# C#` 保住 `C#`；`## Title ##` → `Title`；`a **bold** and *italic* and \`code\`` 的 span 样式；`[docs](https://example.com/x)` 的下划线 + 灰 url；`snake_case_word` **不**被读成斜体。
- **新增**：`"  - nested"` 得到「2 格缩进 + `• `」、`"    - deep"` 得到 4 格——结果与今天相同，但来源从「原始缩进」变成「嵌套深度」。
- **新增（resize）**：宽度从 78 变到 50 之后，一个含表格的块被重渲染——断言重放后表格的行数与列位置按新宽度算，而不是留着一份按 78 排好的旧行。

## 验收

- [ ] `cargo test` 全绿。
- [ ] `cargo clippy --all-targets` 无新增告警。
- [ ] `cargo fmt --check` 只留既有的漂移。
- [ ] `Cargo.lock` 里 `pulldown-cmark` 只出现一份。
- [ ] 真机：`cargo run` 起 TUI，普通回答渲染正常；拖动终端宽度，表格跟着重排而不是碎成一堆错位的列。

## Comments

- 2026-10-01 落地：`src/render/markdown.rs` 重写成事件驱动的块级渲染器（表格与代码块整块缓冲，其余逐行吐），`to_lines(text, width)` 收可用列数；`tui.rs` 抽出 `push_block` / `emit_block`，并在转录宽度变化时清空窗格、按新宽度重放。
- 与票面不同的一点：**思考行**也是源行、却不来自 `Block`，所以重放清单里记的是 `Painted::Block | Thinking | Thought` 三种。只记块会让一次 resize 把「思考完成」整行连它的详情一起抹掉 —— `tests/render_layout.rs` 里 `a_synthesizer_trace_streams_but_records_nothing` 与 `one_message_never_gets_two_thinking_lines` 抓的正是这个。
- 宽度在 TUI 里传的是**减掉 `[name] ` 前缀**之后的列数：第一行是前缀加内容，两者相加正好是转录宽度，不溢出。
- 测试：`tests/render_markdown.rs` 29 条（含三条重写过的安全网）、`tests/render_layout.rs` 的 `narrowing_the_terminal_relays_a_table_out_by_the_new_width`。
- 真机那两格没跑（这台机器上没有 provider key），步骤写进了 `docs/tui-manual-checklist.md` ㉑。
