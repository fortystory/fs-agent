# 认出并画出来：对话视图里的 URL 与工作区路径变成带下划线的热区

Type: implement
Status: done
Blocked by: —

> 规格：[`.scratch/clickable-links/spec.md`](../spec.md) §2（识别）与 §4（视觉）。
> 本票**只做「认出来 + 记下来 + 画出来」**：一行代码都不接点击。打开是
> [票 02](02-open-on-click.md) 的事。`CONTEXT.md` 的**链接热区**词条已在 spec 落盘那一刻
> 写好，**本票不动它**。

## 目标

- 新模块 `render/links.rs`：一个**纯函数**，从一条画出来的显示行文本里找出候选（URL /
  路径）的**列区间**，并有一个能把列区间切成下划线 span 的纯函数。
- 记录点与绘制点同处：`note_rows` 记 `TextRow` 的同一处把候选也记进
  `selection::ScreenText`，于是「认出来的」与「看见的」不会漂开（规格 §2）。
- 有候选的那几列在屏幕上显示为**下划线**（颜色沿用 `palette::CODE_QUIET`，
  与 Markdown 链接今天的 URL 部分同色）。
- 软折的续行按复制那套拼回再识别，命中区间映射回屏幕。
- 不做 IO：候选的**存在性**不在这一层判。

## 现状（2026-10-06 量的，改前先复核）

- 屏幕文本层：[`src/render/selection.rs:26-60`](../../../src/render/selection.rs) 的 `TextRow`
  （`text` / `folded` / `lead`）与 `ScreenText`（`push` / `block_at` / `clear`）。
- 记录点：[`src/render/tui.rs:5372-5391`](../../../src/render/tui.rs) 的 `note_rows`（每块一次
  `push`）与 `row_lead`（靠右排出来的行才有非零 `lead`）。
- 每帧的填与清：`state.screen_text.clear()` 在 `:4486`，`selection::paint` 在 `:4552`。
- 一条显示行的文本从哪来：`note_rows` 里的 `line_text(line)`（拼 span 的 content）。
- 折行：`src/render/pane.rs:418+` 的 `wrap_line`（**数的是列**，不是字节），
  `TextRow.folded` 的判据就是「这一片属于上一片」。
- 链接今天的样子：`render::markdown` 的 `open_frame` / `close_link`
  （[`src/render/markdown.rs:449-477`](../../../src/render/markdown.rs)）—— 标签下划线 +
  灰色 ` (url)`；`palette::CODE_QUIET` 在 [`src/render/palette.rs:96`](../../../src/render/palette.rs)。
- 宽度工具：`render::width` 的 `text_columns` / `char_columns`。

## 落点

- `src/render/links.rs`（新）：候选识别、列区间、按列切 span 加样式。
- `src/render/selection.rs`：`TextRow` 多带一份候选（形状实现定：字段或平行索引）。
- `src/render/tui.rs`：`note_rows` 调用识别并把结果记进去；绘制路径上给候选区间加上下划线。
- `src/render/mod.rs`：挂上新模块。
- `tests/render_tui.rs`（或 `links.rs` 内的 `#[cfg(test)]`）：纯函数用例。

## 具体行为

### 1. 识别（规格 §2 的表）

- 输入：一条显示行的文本；输出：若干 `(列区间, 候选)`，列按 `render::width` 数。
- URL：以 `http://` 或 `https://` 起头，到空白 / 引号 / 全角标点 / 行尾止；句末的 `.`、`。`、
  `）`、`」` 等收尾标点**不在**区间里。
- 路径：含 `/` 的 token（`./x`、`a/b`、绝对路径），或含扩展名的 token（`README.md`）；
  被全角/半角括号或引号包着时，外层括号不在区间里。
- 明确的**不是**候选：`path:line`（`src/render/tui.rs:2867`）、没有 `/` 与扩展名的裸词、
  其它 scheme（`mailto:` 等）。
- 一条行里可以有多个候选。

### 2. 记录（规格 §2）

- `note_rows` 在造 `TextRow` 时顺带调用识别；候选跟着那一行进 `ScreenText`。
- **软折续行拼回**：`folded` 为真的行属于上一片 —— 拼成一条再识别，然后把区间按列映射回去
  （第一片的起点 + 片宽；`lead` 也算进起点）。拼回逻辑与 `selection::text` 同源，
  但**不要**把两份实现合成一份：那边拼的是复制出去的字符串，这边拼的是给识别看的文本，
  两处的取舍（是否保留行尾空格、是否带 `lead`）不同，合起来会让两边都变脆。
- 每帧重建：`clear` 之后由下一次 `push` 重新认一遍。**不缓存**。

### 3. 画出来（规格 §4）

- 候选区间画 `Modifier::UNDERLINED`，`fg` 沿用 `palette::CODE_QUIET`。
- 加样式的办法是**按列切 span**（`Line` 的 span 逐段算宽、在边界处切开），实现在
  `links.rs` 里、纯函数、单测覆盖。绘制路径只负责调用。
- Markdown 链接今天已有的下划线**保持不变**：两处叠在同一段文本上时不要画出双份样式或
  把 span 拆坏。
- **不做 hover**：指针移动不改变画面。

## 验收

- `cargo test`：新纯函数的用例最少覆盖规格 §2 那张表的每一行，外加「一行两个候选」
  「URL 被折成两片仍能拼回」「行尾就是 URL（没有收尾标点）」。
- 一帧带链接的对话页（沿用现有状态脚手架）里，`ScreenText` 记下的候选与画出来的文本对得上
  （列区间落在正确的那几格上）。
- 手工看一眼：跑一次 `/eli5`，正文里那条 `.html` 路径与回答里的 URL 都带下划线。
- `cargo fmt` / `cargo clippy` 干净。

## 评论

- **落地**：新增 `src/render/links.rs`（`Target` / `Hotspot` / `hotspots()` / `mark()` / `underline()`，全部纯函数，15 条单测在文件内）。`selection::TextRow` 加 `hotspots: Vec<Hotspot>`（并进了 `TextRow::plain` 与三处测试构造）。`note_rows` 加第 5 个参数 `hotspots`（8 处调用点，7 处传 `&[]`）。对话页那一段在 `view()` 之后、`render_widget` 之前调 `links::mark(&mut rows, &folded)`：一次识别，一半铺下划线、一半进屏幕文本层。
- **一处实测逼出来的修正**（票面与 spec 没写到这么细）：`trim()` 的**头部**不能收尾标点。第一版把头部的 `.` 也当标点剥，于是 `.scratch/sandbox/eli5-sandbox.html` 认出来是 `scratch/sandbox/...`（少一个点、列区间右移一格）—— 单元测试当场抓住。头部现在只剥**孤立的开括号**，`.` 留着（它是路径的一部分）。
- **与票面不同的一处**：票面写 `DRAG_THRESHOLD` 当前是 1、spec 也照抄了；实测是 **2**（`selection.rs:135`，当日从 1 提上去的，`tui-feedback` §9 那句「回到一格」因此与代码漂开了 —— 那条漂移归票 03 顺手记一句）。票面与 spec 都已改成 2，含义对本 feature 是**更宽的两格容差**，不是更窄。
- **两处留着没做**：`links::row_text` 与 `tui::line_text` 各有一份三行实现 —— 它们看着一样，但一处服务「复制出去的字」、一处服务「给识别看的文本」，把两套取舍绑一起会让两边都变脆（票面 §2 对拼回逻辑说的是同一句话）。另外识别**只挂在对话页**：左栏、轨迹页、详情覆盖层的点击各有语义，`note_rows` 的其余 7 处照旧传空表。
- **验收**：`cargo test` **1355 passed / 0 failed**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 通过。新测试三条：`links.rs` 的 15 条纯函数用例（含折行拼回与「一行两个候选」）、`tests/render_layout.rs` 的 `the_addresses_in_the_conversation_are_drawn_underlined`（按列断言屏幕上**只有**那两段带下划线）、`src/render/tui.rs` 的 `note_rows_keeps_the_hotspots_it_just_marked`（记进屏幕文本层的列区间与那一行文本对齐）。
- **留给票 02 的**：候选记下来之后**没有读者** —— 点击命中、解析、打开与回执都是 [票 02](02-open-on-click.md) 的事；这一票交付的是「认得出、画得出、记得下」。
