# 09 — `/` 菜单：三类来源各一色，窗口宽到描述能读完

Type: implement
Status: done
Part of: ../spec.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §11。

## 目标

真机反馈（2026-10-07，用 `/undo` 时看见的）：

> 我在使用 `/undo` 时发现 `/` 的提示窗口没有区分哪些是命令、哪些是 skills，而且窗口偏小看不全
> 说明文字，用不同颜色区分，再加长窗口宽度。

## 现状（2026-10-07 核实，改前先复核）

- 菜单把三类来源按**同一个样子**排下来：来源信息在 `slash_catalog`（`src/cli.rs`）里拼进去之后
  就丢了 —— `CatalogEntry` 只有 `name` / `description`。
- 未选中行一律 `PLAIN`（`tui-visual-language` §27 的「菜单去黄」），于是命令与技能在屏幕上
  完全同形。
- 一窗 8 行（`layout::MENU_MAX_ROWS`）：**内建命令七条加边框刚好占满**，技能与 MCP 模板永远
  落在窗口外。
- 上限 72 列（`layout::MENU_MAX_WIDTH`），而更要紧的是**公式本身少算一列**：`widest` 写的是
  `2 + name_width + 2 + 描述`，而那一行实际要占 `name_width + 5 + 描述`（行尾还要空一列不碰
  边框）。于是最长的那一条**永远掉最后两个字** —— 光抬上限修不掉它。
- 测试里没有一条看过菜单的颜色；`menu_box` 这个 helper 还把 `find` 的**字节偏移**当列号用，
  菜单一变宽就真的索引到屏幕外面（本票下第一次触发）。

## 具体行为

- **类别进数据**：`render::CatalogKind { Command, Skill, Template }`，`CatalogEntry` 带 `kind`；
  `new` 拆成 `command` / `skill` / `template` 三个构造器，于是「这是哪一类」在组装点就说清楚。
- **类别只决定名字的颜色**：命令 `palette::TOKEN_COMMAND`（与草稿里那个名字同色）、技能
  `palette::MENU_SKILL`、模板 `palette::MENU_TEMPLATE`；描述与整行底子留在正文档。**光标行
  仍 `REVERSED`、不叠类别色**（§27 的另一半照旧）。
- `@` 的候选归 `MenuKind::Path`，颜色照旧 `PLAIN`。
- `MENU_MAX_ROWS` 8 → **12**、`MENU_MAX_WIDTH` 72 → **96**，宽度公式改成实际占列数
  （`name_width + 5 + 描述`）。

## 测试

- `tests/render_layout.rs`：`the_menu_colours_each_kind_of_name_differently`（三色各一条 + 描述
  归正文档 + 光标行不占色）、`a_long_description_gets_the_wider_menu_before_it_is_cut`（200 列
  终端上一条 80 列的描述整句在屏幕上、菜单宽过旧上限）。
- `src/render/palette.rs`：三色互不相同。
- fixture 多一条 MCP 模板（`db:user_report`），三色各有一条代表；`the_arrows_wrap_at_the_ends`
  的末条跟着改成模板。
- `menu_box` helper 按列算（不是按字节）。
- `cargo test` 全绿、`cargo fmt --check` 干净、`cargo clippy --all-targets` 不新增 warning。

## 不做什么

- **不加分组标题行**（「命令」「技能」那两行）：它们占窗口、又落不进键盘导航，而颜色已经把这件
  事说完了。
- 不给描述上色、不加图标或徽标。
- 不动次序、过滤、`Tab`/`Enter`/`Esc` 的语义（`.scratch/goal-loop/spec.md` §13、
  `.scratch/input-tokens/spec.md` §2）。
- 不动 `@` 菜单的颜色与宽度口径。

## 评论

- **落地（2026-10-07）**：`CatalogKind` 落在 `src/render/input.rs`（`CatalogEntry` 的老家），
  `MenuEntry` / `MenuKind` 落在 `src/render/tui.rs`（渲染器自己的形状，含 `@` 的 `Path`）；
  `menu_row` 拆成「名字段 + 描述段 + 行尾填充」三个 span 才谈得上两色。
- **宽度那一条的真正修法**：上限 72 → 96 只解决"有地方放"，**少算的那一列**才是长描述掉字的
  原因。公式写成 `name_width + 5 + 描述` 并在注释里逐项列清（内边距、名字列、两列间隔、描述、
  右内边距、行尾留白）。
- **顺手修的两处 test 基建**：`menu_box` 把字节偏移当列号（`find` 返回的是字节，中文一个字三
  字节），菜单变宽后第一次越界；`the_arrows_wrap_at_the_ends` 的末条断言硬编码了旧 fixture。
- **实测**：`cargo test` **1391 passed / 0 failed**（修前 1388，本票新增 3 条）；`cargo fmt
  --check` 干净；`cargo clippy --all-targets` 无新增（43 条 `collapsible_if` 全是既有的）。
- **留给真机**：⑪ 走查项里补了两条（三色能分、描述不再半句而止）。
