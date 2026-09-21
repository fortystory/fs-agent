# 23: TUI 的宽度计算把字节数 / 字符数当成了列宽

**What to build:** streaming tail 按**显示列**折行，输入光标按**显示列**定位。用 ratatui 已经导出的 `CellWidth`，**不新增依赖**。

Blocked by: None

Status: done

**参考:** spec §19（渲染与组装）、票 18（渲染接缝）、票 22（transcript 的宽字符尾格）

## 现象（两个症状，同一根因）

1. **流式输出一出现中文，live tail 会在约三分之一宽度处提前折行**——一个汉字 3 字节、2 列。
2. **输入中文时光标每字少 1 列。**

和票 22 合起来是同一件事：**渲染层没有 display width 这个概念**。票 22 修的是 transcript 的尾格，本票修的是另外两处。

## 根因

- `live_lines`（`src/render/tui.rs`）：`take_while(|(index, _)| *index < width)` —— `index` 是 `char_indices` 给的**字节下标**，而 `width` 是**列数**。两者单位不同。
- `draw_live`：`2 + state.input.chars().count() as u16` —— **字符数**当列数。

## 复现 / loop

这次**有真的 seam**：`TuiState` 本来就是无终端可测的，所以不需要 pty。两个单测就是 loop。

- `the_live_tail_wraps_on_display_columns_not_bytes`：宽度 10、内容 `你好世界五六`（12 列）
  - 修复前红：`left: ["你好世界", "五六"]`（在第 8 列就折了，因为下标 12 已经 ≥ 10 的判据是字节的）
  - 期望：`["你好世界五", "六"]`
- `the_cursor_column_counts_a_wide_character_as_two`：输入 `你好`
  - 修复前红：`left: 4`
  - 期望：`6`（两个提示列 + 2 字 × 2 列）

先把两处抽成可测函数（**行为原样**、bug 保留）、提成 `pub`，跑出红；再修。和票 22 一样的 TDD 形。

## 修复

- **`wrap_take(text, width)`**：按字符累加 `cell_width()`，用栈上的 `[0u8; 4]` 承载 `encode_utf8` 以避免每个字符一次分配；**至少取一个字符**（否则一个比整行还宽的字符会让外层 `while` 空转）。
- **没有用 `Span::styled_graphemes`**：它内部 `.filter(|g| !g.contains(char::is_control))`，过滤掉控制字符后 symbol 就拼不回精确的字节偏移了。代价是 emoji 那种多字符字素会被算得偏宽（只是折得早一点），注释里写明了。
- **`cursor_column()`**：`2 + self.input.as_str().cell_width()`。
- **没有新增依赖**：`unicode-width` 只是 ratatui 的传递依赖，这里用的是 `ratatui::buffer::CellWidth` 已经 re-export 的那个 trait。好处是 app 的宽度算法和 ratatui 布局用的**是同一个函数**。
- 两个函数从私有提成 `pub`，纯粹为了能像 `render_block` 一样无终端断言。

## 验证

- 两个单测先红后绿，红灯值即上面所列
- `cargo test`：**441 passed / 0 failed**；`cargo clippy --all-targets` 零 warning；`cargo fmt --check` 对本次改动文件干净（`src/context/repo_map.rs` 与 `tests/repo_map.rs` 在 HEAD 上就不干净，未动）
- **pty 端到端**：输入「你好」后光标停在**第 7 列**（x = 6 = 2 + 4）；修复前是第 5 列。这是 `cursor_column` 在真终端上的可见证据

## 顺带发现 — **与下一个工作直接相关**，本次未动

1. **状态行在窄终端会被截断。** 它是 62 列；在 40 列的终端里 ratatui 按 `max_width` 停在边界上，于是 `ctrl-c quit` 这几个字**根本不会被写出来**（我用 40 列做冒烟时就是这么被误导的）。译成中文后约 55 列，窄终端仍然放不下——**中文 UI 那批应当顺带做「按宽度决定显示哪几条提示」**。
2. **`tui.rs:543` 的续行缩进是 `chars().count()`**：`" ".repeat(speaker_label(speaker).chars().count() + 1)`。`[user]` 是 6 字符 6 列，今天**巧合正确**；换成 `[用户]` 是 4 字符但 **6 列**，缩进会少 2 列。**中文 UI 改 `speaker_label` 的那一刻必须同时改这里。**
3. `truncate(text, 500)` 按**字符**截（`transcript.rs:427`）。它是用 `chars().take()` 实现的，所以安全、不会 panic；而且它是**内容预算**不是布局宽度，行为可以接受——但同样的字符数下中文占两倍列宽，块会高一倍。要不要改成按列，属于中文 UI 那批要定的口径。

## Comments

- 票 22 结尾记的「仓库里现在有三种 unicode 宽度相关的东西，值得一次收口」到本票为止**全部关掉**：`paint_scrollback` 的尾格（票 22）、`live_lines` 的折行、`cursor_column` 的光标列（本票）。
- 本票执行的是 `/grill-with-docs` 会话敲定的 Q4：**先修这两个宽度 bug，再上中文 UI**。上面三条发现就是这次排序买到的——如果不先修，它们会和「刚换成中文」混在一起，分不清是谁的锅。
