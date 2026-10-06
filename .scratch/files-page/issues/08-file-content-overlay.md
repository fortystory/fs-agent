# 08 — 文件内容弹窗：读盘与有界截断

Type: implement
Status: done
Part of: ../map.md
Blocked by: 05, 06

> 规格：[`../spec.md`](../spec.md) 的实现决定 §6 与用户故事 D。前置是
> [05 — 铺垫：详情覆盖层为第二个打开方铺路](05-overlay-prep.md) 与
> [06 — 文件页画出一棵能点开的树](06-files-page-tree.md)。

**What to build:** 点一个文件（或键盘在它那一行按 `→`），在既有的详情覆盖层里看见它的内容：
太长就截断并说清截断了，二进制只报一句，读不了给一句话而不是空白。关掉之后这一页停在原处。

## 验收

- [x] 点文件行打开内容弹窗，用的是既有的居中浮层（`Esc` 关掉）；键盘在文件行上按 `→` 也打开它
- [x] 正文按行的顺序画出来，长行按文本区宽折行
- [x] 行数、字节、行宽三个上限都生效；被截断时**明说截断了**，不是悄悄少一段
- [x] 二进制文件只报一句，不往屏幕上泼乱码
- [x] 读不了的（不存在 / 权限 / 非 UTF-8）各给一句话
- [x] 关掉之后文件页停在原处（滚动位置与展开状态不变）
- [x] 这个弹窗**不进事件流、不进模型上下文**：跑完之后 `sessions show` 里看不见它，模型也没多拿一个字节
- [x] 拖选复制在这一页仍然可用（新增的点击没有把它吃掉）

## 评论

- **落地（2026-10-06）**：`DetailKind::File`，以 `DetailOrigin::Files` 打开 —— 覆盖层立着时
  文件页既不冻也不还原，所以关掉它这一页停在原处（展开、滚动、焦点都不动）。读盘在
  `files::read`（`src/render/files.rs`）：字节上限 200 000、行数上限 2 000，截断落在字符边界上，
  截了就画 `detail_truncated` 那一句；二进制（前 8 KiB 里有 NUL）、读不了、非 UTF-8 各一句。
- 入口两条：点文件行，与键盘在文件行上按 `→`。
- `state.take_events()` 为空是这一票「不进事件流」的断言；「不进模型上下文」是结构性的
  （渲染器手里没有 `Session`），所以这条路上也没有打码。
- 断言：`a_workspace_file_opens_in_the_detail_overlay`、`the_file_key_opens_the_same_overlay`、
  `a_binary_file_only_gets_one_line`、`a_file_that_cannot_be_read_says_so`、
  `a_file_that_is_not_utf8_says_so`、`a_long_file_is_truncated_and_says_so`、
  `a_wide_line_wraps_to_the_text_area`；三个上限与二进制判据在 `files.rs` 的单测里。

- **补记（收口时）**：代码审查指出三个上限只交了两档 —— 现在补上**行宽**那一档
  （`MAX_LINE_CHARS = 4_000`，按行截断并把截断说出来）。断言：
  `the_line_width_cap_truncates_and_says_so`；拖选仍可用那一条也补了断言
  （`the_files_page_can_still_be_dragged_and_copied`）。

- **收口（2026-10-07）**：八条验收逐条勾上；票底点名的九条断言全在 `tests/render_layout.rs`
  与 `src/render/files.rs` 的单测里，全量 `cargo test` 1366 passed / 0 failed。
