# 06 — 文件页画出一棵能点开的树（tracer bullet）

Type: implement
Status: ready-for-walkthrough
Part of: ../map.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) 的实现决定 §1、§3、§4 与用户故事 A、B。**这是本轮的
> tracer bullet**：它把「会话级索引 → 树的排版 → 绘制 → 命中 → 状态 → 重绘」这条路径整个走通
> 一遍，后面每张票都照它的样子走 —— 所以它挑的是最窄的一条线，而不是最容易的一条。

**What to build:** 左栏 `文件` 页不再是一行「此页尚未实现」：它列出工作区里的文件与目录，画成
一棵**可以点开的树** —— 顶层摊开、点目录展开与收起、滚轮悬在这一栏上时滚这一页。名字超宽会截断，
索引还没就绪与工作区真的是空的各有各的说法。

## 验收

- [ ] `文件` 页画出工作区顶层（文件与目录），不再画占位行
- [ ] 缩进每层 2 列；同一层里目录排在文件前面、同类之内保持索引给的顺序
- [ ] 初始全部收起；目录行带尾斜杠与折叠字形、文件行没有；两者只靠结构区分，**不给颜色**
- [ ] 点目录展开、再点收起；展开状态在重绘与切页之后都活着
- [ ] 滚轮悬在左栏页区内时滚这一页，指针在主列时照旧滚主列（覆盖层与问卷的优先级不动）
- [ ] 名字超出宽度时截断，40 列与 28 列两档都读得下去
- [ ] 索引未就绪与工作区为空各画各的，都不显示空白或编出来的数据
- [ ] 文件行的点击动作**这一票不做**（由 [08 — 文件内容弹窗](08-file-content-overlay.md) 接上）
- [ ] 树的数据来自那份会话级索引，它的遍历规则一个字不改

## 评论

- **落地（2026-10-06）**：树的排版是 `src/render/files.rs` 里的一个纯函数（路径列表 + 展开集合
  → 可见行：同层目录在前、同类保索引序、每下一层多一档深度、收起目录的子项不出现）；绘制、
  命中与滚动在 `tui.rs` 的 `draw_files_page` / `files_click` / `files_wheel`。展开状态、滚动
  位置与「上一帧画出来的那些行」都住在 `TuiState` 里，不进事件流。
- **三态各说各的**：索引未就绪（`files_loading`）、工作区为空（`files_empty`）、有内容。窄档下
  超宽的名字用 `ellipsize_line` 收尾（`…`），而不是悄悄少几个字。
- 这一票不做文件行的点击动作（留给票 08），但点击仍然归这一页，不会落进转录那一套分派。
- 断言：`the_files_page_lists_the_workspace_top_level`、`the_tree_is_two_columns_per_level_and_puts_directories_first`、
  `clicking_a_directory_expands_and_collapses_it`、`the_tree_keeps_its_shape_across_repaints_and_a_tab_switch`、
  `the_files_page_names_the_two_empty_states`、`the_wheel_over_the_files_page_scrolls_the_tree`、
  `a_narrow_sidebar_still_reads_the_tree`；纯函数那几条在 `files.rs` 的单测里。
