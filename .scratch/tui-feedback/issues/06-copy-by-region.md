# 06 — 复制：按区域取行、软折拼回、OSC 52 与提示行回执

Type: implement
Status: done
Part of: ../spec.md
Blocked by: 05

> 规格：[`../spec.md`](../spec.md) §6。选择手势与屏幕文本层在票 05。

## 目标

松开手，选区里的文本进系统剪贴板（OSC 52），提示行给一句回执。取文本按**那块区域**的宽度与
行结构来：TUI 自己软折出来的续行拼回一条长行，区域自己的硬换行保留。于是复制一段被折过的命令
得到的是原来那一条命令，而不是屏幕上那两截。

## 现状（改前先复核）

- 票 05 交回来的东西：`ScreenText`（`(rect, kind, rows: Vec<TextRow{text, folded}>)`）、
  `Drag { block, anchor, head, selecting }`、反白绘制。
- 宽字符的列算术在 `src/render/width.rs`（`text_columns`、`truncate_columns`）与
  `src/render/pane.rs` 的 `char_columns`（私有）：取值要按**显示列**切，不能按字节。
- 提示行的内容由 `TuiState::hint_line(width)` 产出（`src/render/tui.rs`），出口段由
  `wording::status_line` / `viewer_status_line` 的阶梯拼；举手回执由 `raised_gesture` 换掉。
- 脉冲 tick 一直在走（`tui-visual-language` §32），退出手势的 deadline 就是靠它过期的 —— 回执
  的过期走同一个 tick。
- `Cargo.toml` 里没有直接的 `base64` 依赖（`Cargo.lock` 里的那些是传递依赖）；不为这一处引新依赖。

## 落点

`src/render/selection.rs`（取值、软折拼回、OSC 52 串、base64 编码）、`src/render/width.rs`
（按列切一段的新 helper）、`src/render/tui.rs`（`Up` 时复制、提示行回执、tick 过期）、
`tests/render_layout.rs`、`tests/selection.rs`（新建，或并进 `tests/wording.rs` 的纯函数组）。

## 具体行为

1. **按列切一段**：`width::slice_columns(text, from, to) -> String` —— 按显示列切，宽字符不切半
   （切到边界内侧）。单测覆盖 CJK 与 ASCII 混排。
2. **取文本**（`selection::text(&ScreenText, &Drag) -> String`）：
   - 只在 `drag.block` 那块区域内做；把 `anchor`/`head` 规范化成 `(首行, 首列)` … `(末行, 末列)`；
   - 每一显示行取 `[列区间)` 那一段，去掉**行首与行尾**的填充空白（行内空白保住）；
   - 拼接：一行之后若下一行是它的软折续行（`rows[i+1].folded`），直接接上（不加换行）；否则加
     `\n`。首行不做行尾裁剪之外的加工，末行的列上限用末列 + 1；
   - 末尾去掉最后那个多余的换行与整体右侧空白。
3. **OSC 52**：`selection::osc52(payload) -> String` 产出 `ESC ] 52 ; c ; <base64> BEL`；base64 是
   `selection.rs` 里一个 20 行的纯函数（单测钉住 RFC 4648 的几个向量与中文 UTF-8）。
   `Up` 时用 `execute!(stdout, Print(osc52(&text)))` 写出去 —— 与既有 `SetTitle` 同一条路。
4. **回执**：`TuiState` 新增 `copied: Option<(std::time::Instant, usize, usize)>`（字、行、时刻），
   `hint_line` 在**提示集合之前**放一句 `wording::copied(chars, lines)`（例如 `已复制 128 字 · 3 行`），
   举手时仍让位给举手那句；`tick()` 里超过 3 秒就清掉。选区为空（取出的文本是空的）时不复制、
   不留回执。
5. **不动 shift 原生选择**：crossterm 的鼠标捕获配置一个字不改。

## 验收

- 纯函数：`slice_columns` 的宽字符边界；`osc52` 的字节串（前缀、`BEL`、base64 内容）；软折拼回
  （造一个 `folded = [false, true]` 的两行，拼出来是一行）与硬换行保留（`folded = [false, false]`）。
- 帧层：`Down` + `Drag` + `Up` 之后提示行里出现 `已复制`；`tick` 走过 3 秒之后它消失。
- 帧层：拖选一段被折过的正文，抓到的那串文本（用 `selection::text` 直接断言）是拼回来的一条，
  而不是两截。
- 空选区（没拖过任何字符就抬手）不留回执。
- `cargo test` 全绿；`cargo clippy --all-targets` 无警告。
