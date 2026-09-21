# 22: transcript 里每个宽字符后面多一个空格

**What to build:** 让 scrollback（`Terminal::insert_before`）这条路径不要把宽字符的**尾格**当空格写出去。修在 `paint_scrollback` 里，配一条接缝上的回归测试。

Blocked by: None

Status: done

**参考:** spec §19（渲染器与组装）、票 18（渲染接缝）、票 21（同一片区域的另一个症状）

## 现象

一轮中文对话之后，transcript 里每个汉字后面都多一个空格，ASCII 部分正常：

```
[user] 你 好 , 你 是 谁 ?
[deepseek] ... 我 是 一 个 在 你 这 台 机 器 上 工 作 的 编 码 代 理 （ coding agent） 。
```

`coding agent` 是紧凑的、`（` 和 `）` 后面有空格——**空格只贴着宽度为 2 的字符出现**。这一条就排除了「模型/用户输入里本来就有空格」。

## 根因（已复现，且是库的行为，不是终端复制伪影）

raw 字节流里 app 写出去的就是空格：

```
<ESC>[4;1H<ESC>[;m[user] <ESC>[;m你 好 世 界          ...
```

链条（都是 primary source，本机 ratatui 0.30.2 → ratatui-core 0.1.2）：

1. `Buffer::set_stringn` 把宽字素写进 cell x，并把**尾格** `reset()` 成 `Cell::EMPTY`（`ratatui-core/src/buffer/buffer.rs`）。
2. `Cell::EMPTY.symbol()` 返回 `" "` —— `symbol: None` 时 `map_or(" ", ...)`（`buffer/cell.rs`）。
3. `Terminal::insert_before` 在没开 `scrolling-regions`（**不是默认 feature**，本仓库没开）时走 `insert_before_no_scrolling_regions` → `draw_lines`，它把 scratch buffer 的**每一个 cell** 原样交给 backend，**不像 `Buffer::diff` 那样按 `cell_width` 跳过宽字符尾格**（`terminal/inline.rs`）。
4. `CrosstermBackend::draw` 对每个 cell 无条件 `Print(cell.symbol())`，只在位置不相邻时补 `MoveTo`（`ratatui-crossterm/src/lib.rs`）。尾格于是被打印成一个真的空格。

对照：`Terminal::draw`（live region、流式 tail）走 `Buffer::diff_iter`，那里显式 `self.pos += cell_width - 1` 跳过尾格（`buffer/diff.rs`）——所以**输入行里的中文一直是对的**，坏的只有 transcript。

字节流里两条路径可以直接对比：

```
live region ：<ESC>[10;3H你  <ESC>[10;5H好     ← 跳过了尾格，对
transcript  ：[user] 你 好 世 界                ← 尾格被当空格写出去
```

## 复现

pty harness 里输入 4 个汉字再回车，然后看 app 写出去的原始字节：

- 修复前：`spaced('你 好 世 界') = 1`，`adjacent('你好世界') = 0`
- 修复后：`spaced = 0`，`adjacent = 1`（`[user] 你好世界` 连续）

接缝上的回归测试：`tests/render_tui.rs::a_wide_grapheme_leaves_no_blank_cell_after_it`——断言渲染完 `[user] 你好` 之后，尾格是 `""` 而不是 `" "`。**这条测试是 red-capable 的**：先把原闭包原样抽成函数、加测试跑出红（`left: ["你"," ","好"," "]`），再加修复转绿。

## 修复

`src/render/tui.rs` 的 `paint_scrollback`（从 `insert_before` 的闭包抽出来，公开以便测试）：画完行之后，扫一遍 buffer，把宽度 > 1 的字素的尾格 `set_symbol("")`。backend 就什么都不打印；它仍然会走到下一个 cell，而终端画完宽字素后光标**已经**在那里了，所以位置不需要 `MoveTo` 修正。

没有选择开 `scrolling-regions` feature（那会让 `insert_before` 走 `diff` 路径，从而「正确地」修掉）：它换的是终端滚动协议（DECSTBM），对不支持它的终端是更大的风险，而这里的问题可以就地解决。

## 验证

- `cargo test`：**439 passed / 0 failed**
- `cargo clippy --all-targets`：零 warning；`cargo fmt --check` 对本次改动文件干净
- e2e pty：宽字符行从 `你 好 世 界` 变成 `你好世界`

## 未验证 — 请勿当成已修

报告里状态行的 `esc  ancel` / `shift+ ab plan`（两个字符被空格顶掉）**我没能复现**，因此**不能**说这个修复解决了它。

试过：pty 里 80 列终端 + 输入 35 个汉字（70 列）触发回车，transcript 行确实带着多余空格溢出并换行了，但状态行完好（`working · enter send · esc cancel · shift+tab plan · ctrl-c quit`，逐字正确）。所以「宽字符让行溢出」**不足以**解释那个症状。

还没试过的条件：用户那边插入的是**一整条多行长回复**，会走进 `insert_before` 的滚动分支（`while buffer_height + viewport_height > screen_height` → `scroll_up`）。如果 `last_known_area` 的记账被软换行带偏，live region 的绝对定位重绘就可能压到状态行的某一列上——这与「两个字符被换成空格」相符。要定论需要一条能产出多行长中文回复的路径（真 provider，或把 fake provider 接进二进制）。

## Comments

- 顺带发现、**本次未动**的两个宽度 bug（同一类「全程没有 display width 概念」，但症状不同，需要各自的复现）：
  - `TuiState::live_lines` 按**字节下标**当列宽折行（`take_while(|(index, _)| *index < width)`），中文会在约 1/3 宽度处提前折行。
  - `draw_live` 的光标列 `2 + state.input.chars().count()` 按**字符数**算，输入中文时光标每字少 1 列。
- 仓库里现在有三种 unicode 宽度相关的东西，值得一次收口：`live_lines` 的折行、`draw_live` 的光标列、`paint_scrollback` 的尾格。`ratatui::buffer::CellWidth` 已经够用，不需要新增依赖。
