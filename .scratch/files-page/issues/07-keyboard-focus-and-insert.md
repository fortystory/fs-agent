# 07 — 键盘焦点行与插 `@路径`

Type: implement
Status: done
Part of: ../map.md
Blocked by: 06

> 规格：[`../spec.md`](../spec.md) 的实现决定 §5 与用户故事 C。前置是
> [06 — 文件页画出一棵能点开的树](06-files-page-tree.md)。

**What to build:** 点一下左栏就能用键盘走这棵树：`↑` / `↓` 移焦点行（焦点行看得出来）、
`→` / `←` 展开与收起、`Enter` 把那一行的路径作为 `@路径` 插进草稿并顺手把键盘还给输入区，
`Esc` 只还键盘。

## 验收

- [x] 点**文件页**（页签条或页区）把键盘交给这一页，焦点行落在点的那一行；那一下同时仍是展开 / 开弹窗（**收口时收窄**：切到另外两页、或 `Ctrl-O` 收起左栏，都把键盘还给输入区 —— 见票底补记）
- [x] `↑` / `↓` 移焦点行，到顶 / 到底不越界；焦点行看得出来
- [x] `→` 展开目录、`←` 收起目录（文件行上的 `→` 由 [08](08-file-content-overlay.md) 接上）
- [x] `Enter` 把该行路径作为 `@路径` 插进草稿（目录带尾斜杠），并把键盘还给输入区
- [x] `Esc` 只把键盘还回去；**忙时按 `Esc` 不取消回合** —— 键盘回去之后再按一下才按既有分叉取消
- [x] 键盘在这一页时输入区收不到那些键；键盘还回去之后一切照旧（吃的是走树的那几个键加字符键，`Ctrl-*` / `Tab` / 翻页键照常穿透 —— 见票底）
- [x] 页签切换仍只靠点，不给键位
- [x] 焦点行与展开状态只活在进程内，不进事件流

## 评论

- **落地（2026-10-06）**：`TuiState::sidebar_keyboard` 与 `files_focus`。点左栏（页签条或页区）
  把键盘交给这一页并给焦点行（`take_sidebar_keyboard`），`↑` / `↓` / `→` / `←` / `Enter` / `Esc`
  归它；`Enter` 插入 `@路径` 之后键盘回输入区，`Esc` 只还键盘（一次手势一层，不取消回合）。
- **一处相对 spec §5 的收窄，走查时请看一眼**：spec 说这一页与详情覆盖层「同一族」（独占
  键盘）。实现里它吃掉的是走树的那几个键**加字符键**（打字不落草稿），而 `Ctrl-*`、
  `Tab` / `Shift+Tab` 与翻页键照常穿透 —— 于是点一下左栏不会让退出、挂起、左栏开关、模式循环
  失灵。要改成完全独占，删掉 `sidebar_key` 里那两行即可，代价是连 `Ctrl-C` 也一起挡住。
- 焦点行画成常驻选中（`ACCENT` + `BOLD`），是这一页唯一着色的东西（目录与文件之间仍然不给色）。
- 断言：`clicking_the_tree_hands_it_the_keyboard_and_the_arrows_walk_the_rows`、
  `the_arrows_open_and_collapse_a_directory_and_enter_inserts_the_path`、
  `the_tree_keys_do_not_reach_the_draft`、`escape_hands_the_keyboard_back_without_cancelling_the_turn`、
  `the_tab_row_also_hands_the_keyboard_to_the_sidebar`。

- **补记（收口时）**：代码审查指出原实现「点任意左栏页签都交键盘」会让 `调用量` / `todo` 两页
  把输入区静默扣死（那两页没有能用方向键走的东西）。现在的形状是：**键盘归属只对文件页成立**
  —— 点文件页签或它的页区才交键盘；切到另外两页、以及 `Ctrl-O` 收起左栏，都把键盘还给输入区
  （与「没地方画覆盖层就关掉它」同一条纪律）。断言：`the_keyboard_goes_back_when_the_sidebar_is_not_the_files_page`。

- **收口（2026-10-07）**：八条验收逐条勾上（第一条按实现收窄到文件页、第六条注明吃键范围）；
  票底点名的六条断言全在 `tests/render_layout.rs`，全量 `cargo test` 1366 passed / 0 failed。
