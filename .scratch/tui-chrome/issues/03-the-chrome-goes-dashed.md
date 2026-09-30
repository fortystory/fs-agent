# 剩余框架虚线化，颜色压深

Type: implement
Status: done
Blocked by: 01

> 规格：`.scratch/tui-chrome/spec.md` §3。
> 依赖票 01：那套端点交叉符（`├ ┤ ┬ ┴`）已经在票 01 里删干净了，本票把它们原来的位置换成虚线字形。

## 目标

结构性框线（竖分隔列、两条横线、页签条的上下线与其分隔符、`/` 菜单的框）统一换成**浅三重虚线** `┆` / `┄`，颜色从 `DarkGray` 换成一个更沉的真彩暗灰常量 `CHROME_LINE`。问题覆盖层与详情覆盖层也换虚线边框，但**保留各自的语义色**。

## 落点

`src/render/tui.rs`（新增 `CHROME_LINE` 常量、`paint_rule`、`draw_divide`、`draw_tab_bar`、`draw_menu`、`draw_modal`、`draw_detail`）；`tests/render_layout.rs` 里所有把 `│` / `─` 当锚点的断言。

## 具体行为

1. **新增常量**（放在 `tui.rs` 的颜色常量区，与 `prompt_colour` 的 rustdoc 相邻）：

   ```rust
   /// 结构性框线的颜色：比 `DarkGray` 再沉一档（`DarkGray` 在常见配色里 ≈ `#808080`）。
   /// 深色终端上这是「框架退到内容后面」的取值；亮背景终端可能需要在这一点上调。
   const CHROME_LINE: Color = Color::Rgb(0x4a, 0x4a, 0x4a);
   ```

   rustdoc 要写明它为什么是真彩色：16 色 ANSI 里比 `DarkGray` 更暗的只有 `Black`，那在深色背景上等于消失。

2. **`paint_rule`**：`Style::default().fg(Color::DarkGray)` → `CHROME_LINE`；每一格的字形从 `─` 换成 `┄`。
3. **`draw_divide`**：颜色换 `CHROME_LINE`；整列字形从 `│` 换成 `┆`。
4. **`draw_tab_bar`**：
   - 上下两条线走 `paint_rule`，自动跟着变；
   - 标签之间的分隔符 `Span::styled("│", dim)` 的 `dim` 换成 `CHROME_LINE`，字形换成 `┆`；
   - 末尾填充的 `"─"` 换成 `"┄"`，颜色同样换。
5. **`draw_menu`** 的 `Block`：加 `.border_set(ratatui::symbols::border::LIGHT_TRIPLE_DASHED)`，颜色 `DarkGray` → `CHROME_LINE`。
6. **`draw_modal`** 的 `Block`：加同样的 `border_set`，**颜色保持 `Color::Yellow`**。
7. **`draw_detail`** 的 `Block`：加同样的 `border_set`，**颜色保持 `view.detail.color`**。
8. **不动**：回合条的 `┊` / `┃` / `⋮`、滚动条、语法高亮、提示符的色相脉冲、`status_line` / `draw_status` 的文字色（那是**文字**不是框线）。

   > 边界写清楚：「剩余框架」= 画在屏幕上的**线**。状态行、提示行、页脚那些 `DarkGray` 的**文字**不在本轮范围里，它们要留着自己的可读性。

9. `use` 那一行要补 `ratatui::symbols::border`（或按现有风格 `use ratatui::symbols::border::LIGHT_TRIPLE_DASHED`）。

## 测试

- **新增**：外壳帧里凡是 `┄` / `┆` 的格子，前景色都是 `CHROME_LINE`；且**没有**任何框线格是 `DarkGray`。
- **新增**：`┄` 至少出现在两条横线上、`┆` 至少出现在分隔列上；`│` 与 `─` 在**外壳**里一个都不出现。
- **新增**：回合条的非焦点格仍然是 `┊`（别被顺手改成 `┆`）。
- **新增**：问题覆盖层的边框格是 `Yellow` 且字形属于虚线集；详情覆盖层同理、颜色是发言者色。
- **改写**：`tests/render_layout.rs` 里那些用 `│` 定位的辅助函数与断言（`a_wide_terminal_...`、`the_selected_tab_...`、`only_the_tab_labels_answer_a_click`、`draw_scrollbar` 相关的、`the_wide_sidebar_...` 的 `(41, y)`）——把 `'│'` 换成 `'┆'`。**逐条过一遍**，别只改编译不过的那些：有些断言用的是 `trim_matches(['│', ' '])`，编译得过但语义已经错了。
- `trim_matches` 里出现的字符集（如 `['│', ' ']`）要一并换成 `['┆', ' ']`。

## 验收

- [ ] `cargo test` 全绿。
- [ ] `cargo clippy --all-targets` 无新增告警；`cargo fmt --check` 只留既有漂移。
- [ ] 真机：三层虚线（`┄` / `┆` / 回合条的 `┊`）在同一屏上读起来分得开；`CHROME_LINE` 的深度在自己背景色上是否合适——不合适就调那一个常量，并在票 06 的手工清单里记下实测值。

## Comments

- 2026-10-01 落地，**与票 01、02 同批**：端点交叉符删掉之后，`paint_rule` / `draw_divide` 的字形与颜色紧接着在同一批里换掉，免得对同一批断言返工两次。
- 字形取**三重**虚线（`┄` / `┆`），与回合条已有的四重 `┊` 分开。`Block` 那边用现成的 `ratatui::symbols::border::LIGHT_TRIPLE_DASHED`；它的四个角仍从 `NORMAL` 继承，所以框角还是 `┌┐└┘` —— Unicode 的框线区没有虚线角，这是现成的取舍。
- 颜色落在 `tui.rs` 的 `CHROME_LINE = Color::Rgb(0x4a, 0x4a, 0x4a)`。**弹窗保留语义色**（问题 `Yellow`、详情发言者色），只换虚线 —— 「更深」那条只作用于本来就是灰的结构线。
- 「线」与「字」在页签行上分开取色：未选中的标签仍是 `DarkGray`（它得读得出来），两条线与它们之间的分隔符才是 `CHROME_LINE`。
- 测试那边逐条过了 `'│'` / `'─'` 的锚点，包括那些编译得过但语义已经错了的（`trim_matches(['│', ' '])`、`starts_with('├')`、面板页的探针，以及 `panel_text` 里「页到哪儿结束」的那个守卫 —— 主列的横线会把分隔列盖成 `┄`，所以两者都算证据）。
- `CHROME_LINE` 在维护者背景色上的深度留给真机：`docs/tui-manual-checklist.md` ⑳ 第 4 条。
