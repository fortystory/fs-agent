# 03 — 记号的上色与 chip

Type: implement
Status: done
Blocked by: 02

> 来源：[`../spec.md`](../spec.md) §4。这是这一组里最大的一张：它给草稿里的记号上色，并让
> **能兑现**的记号变成一块跳不过去、删得掉、改不了的文本。

## 目标

- **判据只有一条**：能兑现 —— `/` 命中命令表、`@` 命中票 01 的索引。
  **上色与 chip 共用同一条判据**，所以打字过程中它忽隐忽现，补全成功那一刻才凝固。
- **上色**：命令蓝、引用紫（常量进 `wording`/主题层，不散写在绘制里）；命中不了的一律保持
  普通文本色。
- **chip：光标永不落在记号内部。**
  - 所有移动（`←`/`→`/`↑`/`↓`/`Home`/`End`/`Ctrl-A`/`Ctrl-E`/鼠标点击）把目标位置**吸附**到
    最近的记号边界；
  - 所有删除（`Backspace`/`Delete`/`kill_word`/`kill_to_line_start`/`kill_to_line_end`）命中
    记号时**整块删**；只删记号本身，不额外吞掉旁边的空白；
  - 内部插不进字 —— 由第一条天然成立。
- **粘贴一视同仁**：粘进来的文本按同一条推导，含有效记号就成 chip。
- **缝（本节的重点）**：区间由 `TuiState` 算好、同步进 `Input`（与今天 `slash.prefix` 那种
  「记着的值」同构），`Input` 只做**纯几何**的吸附与整块删 —— 它继续不认识命令表与文件系统。

## 现状（2026-10-04 核实，改前先复核）

- `Input`（[`src/render/editor.rs`](../../../src/render/editor.rs)，539 行）持有 `String` 与
  `cursor: usize`；移动与编辑各是一个方法：`left`/`right`/`up`/`down`/`home`/`end`/
  `backspace`/`delete_forward`/`kill_word`/`kill_to_line_start`/`kill_to_line_end`/
  `insert_char`/`insert_str`。
- `view()`（[:116](../../../src/render/editor.rs)）画行并给出 `Placed`（光标位置），折行由它自己算。
- 粘贴的规范化在 [:526](../../../src/render/editor.rs)（`normalize_paste`）。
- 编辑器键的分派在 [`src/render/tui.rs:1882`](../../../src/render/tui.rs) 的 `editor_key` 一带。
- **`Input` 不认识命令表与文件索引**：两者都在 `TuiState` 上
  （[`src/render/tui.rs:653`](../../../src/render/tui.rs) 的 `catalog`）—— 这条约束是本节
  「缝」那一款的由来。

## 收尾

- 单元测试（`editor.rs`）：吸附（`←`/`→` 跨过记号）、整块删（三种删法）、粘贴之后成 chip、
  **无效记号不是 chip**（`看 /tmp/x` 仍能一格一格改）。
- 帧测试：两种颜色各一条；记号跨折行时样式连续。
- `cargo test`、`python3 scripts/check-language.py` 通过。

## 不做什么

- **不把 `Input` 改写成片段序列**：chip 靠吸附维持，`String` + `cursor` 的模型不动。
- 不动历史（`Ctrl-P`/`Ctrl-N`）与折行数学、不动 `submitted()` 的形状。
- 不做「删记号时顺手吃掉旁边的空白」。
- 不给无效记号另一种淡色：命中不了就是普通文本。

## 评论

- 2026-10-05 落地：`editor::Highlight { start, end, style }` 加 `Input::set_highlights()`
  就是 spec §4 说的那条缝 —— 判据要查命令表与文件索引，而编辑器两样都不认识，所以区间与
  样式一起从 `TuiState::sync_tokens()` 同步进来。
- **吸附分两种**：`←`/`→` 用**方向性**吸附（`snap_left`/`snap_right`，目标落在记号内部就
  去它那一侧的边界），`Home`/`End`/上下行用**最近**边界（`snap_nearest`）。方向性是必须
  的：从记号右边界按 `←` 若用「最近边界」会原地不动，一个跨不过去的记号。
- **整块删**：`expand_to_tokens(from, to)` 把删除范围扩到与它相交的每个记号的完整范围，
  不碰记号旁边的空白。`Backspace`/`Delete`/`Ctrl-W`/`Ctrl-U`/`Ctrl-K` 五处都走它。
- **上色**：`TOKEN_COMMAND`（`LightBlue`）与 `TOKEN_REFERENCE`（`LightMagenta`）两个常量
  在 `tui.rs` 的主题常量区，判据与 chip 共用（`/` 精确命中 `catalog` 里的名字、`@` 精确
  命中索引里的路径）。同步时机：`key()` 开头、`editor_key()` 末尾（编辑后立即刷新，否则
  键入的最后一个字符不会当场凝固）、`paste()`/问卷粘贴确认、`menu_accept()`、`files_loaded()`
  与 `Catalog` 请求各一次。
- **鼠标点击**那一条在现状下是构造性的：输入区今天根本不接受点击落光标（`mouse()` 里没有
  这条路径），所以没有一条路能违反它 —— 不新建一条鼠标定位光标的功能。
- 测试：`tests/render_editor.rs` 四条（吸附、方向性跨块、跨行吸附、三种删法 +
  无记号照旧；外带一条跨折行样式连续），`tests/render_layout.rs` 两条（四种判据各一条颜色、
  粘贴进来的记号同样是 chip），`tests/wording.rs` 一条（两个颜色是两个、且都不是 `Reset`）。
