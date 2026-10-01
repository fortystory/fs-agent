# 上下文行与 token 行的占比色条

Type: implement
Status: done
Blocked by: 02

> 规格：`.scratch/usage-stats-format/spec.md` §3（占比色条）与「测试决定」里的帧断言一条；§4 说了它**不**碰什么。
> 只涂两行：上下文行（分母 `SessionFacts::context_window`）与 token 行（分母 `SessionFacts::budget_limit`）。底色不占列，所以列宽与降级链一行都不动。

## 目标

- 在这两行的值列里，把左起 `N = ceil(占比 × 值列宽)` 列涂上 `Color::DarkGray` 底色；文字与右对齐不变，前景色不变。
- 没有分母的行、以及那两行里没有分母的时候，不涂。
- 用 `TestBackend` 读单元格 `bg` 断言色条落在哪些格子上。

## 现状

- `Panel::lines`（`src/render/panel.rs:67-113`）把六行攒成 `Vec<(&'static str, String, bool)>`（`87-107`），末尾由 `row(label, value, value_columns, right)`（`110-112`）消费。
- `row()`（`src/render/panel.rs:133-147`）现在把值 `fit` 后 `pad_left` 右贴齐，再拼成一个 `Span::raw(value)`；标签是 `Span::styled(label, Style::default().fg(Color::DarkGray))`，中间一个空格。
- 面板画在 `sidebar_page` 这个 `Rect` 里，左边从 `x = 0` 起（`src/render/layout.rs:341-348`，`sidebar.x = area.x`）；值列从 `label_columns() + 1 = 7` 列起（标签列 6：`上下文` 三字各 2 列，加一个空格）。120 列时值列 33 列，80 列时 21 列。
- 帧断言的入口是 `tests/render_layout.rs` 的 `buffer(width, height, state)`（`109-115`）；`ratatui` 的 `buffer::Cell` 有公开的 `bg` / `fg` 字段，所以 `frame[(x, y)].bg` 可以直接读。

## 落点

- `src/render/panel.rs`：`Panel::lines` 的行元组与 `row()`。
- `tests/render_layout.rs`：挨着 `the_panel_reads_its_numbers_off_the_stream`（`2000`）新开帧断言。

## 具体行为

1. **行元组加一个占比**：`Panel::lines` 的 `rows` 从 `(&'static str, String, bool)` 变成 `(&'static str, String, bool, Option<f64>)`，由 `row()` 消费。
   - 上下文行：`self.last_input.map(|used| used as f64 / facts.context_window.max(1) as f64)`；`last_input` 是 `None`（还没报过用量）时不涂。
   - token 行：`facts.budget_limit.map(|limit| self.total.total_tokens() as f64 / limit.max(1) as f64)`；`budget_limit` 是 `None`（没设额度）时不涂。
   - `回合`、`输入`、`输出`、`缓存` 四行一律 `None`（spec §3：它们没有诚实的占比）。
2. **`row(label, value, width, right, share)`**：
   - 先照现状 `fit`，再 `pad_left` / `pad_right` 到 `width`——**这两步与 `fit()` 的降级行为不动**。
   - `share` 是 `None` 时，拼法与现在完全一样（一个 `Span::raw`）。
   - `share` 是 `Some(s)` 时：`let n = (s.clamp(0.0, 1.0) * width as f64).ceil() as usize`，再把 `n` 夹到 `0..=width`；把补好的整串按**显示列**在 `n` 处切成前后两段（用 `super::width::truncate_columns` 那一套，别在宽字素中间切——`万` / `亿` / `（` 都是双列），前段 `Span::styled(head, Style::default().bg(Color::DarkGray))`，后段 `Span::raw(tail)`。
   - 标签那一段与中间那个空格**不涂**（只有值列）。右对齐的前导空格**算在值列里**，所以色条从值列左缘起、不跟着数字跑。
   - `n == 0` 时输出与不涂一样；`ceil` 保证占比 > 0 时至少 1 列，占比 ≥ 1 时正好涂满值列、不越出。
3. **不参与列宽降级**：`value_columns` 的计算（`68`）、上下文百分比是否丢的那两处判定（`73-81`）、缓存行是否加入（`105-107`）、`fit()`（`153-164`）、标签列宽与行序，**都不动**。窄档丢掉 `（6%）` 时色条照涂。
4. **颜色只用 `Color::DarkGray`**：与标签的 `Style::default().fg(Color::DarkGray)` 同一个暗档；不引入主题系统、不新增配置项。值那一段的前景色不动。

## 测试

- `tests/render_layout.rs` 新开一条帧断言，位置放在 `the_panel_reads_its_numbers_off_the_stream`（`2000`）旁边：
  - 用 `state()`（`context_window = 200_000`、`budget_limit = Some(100_000)`）`apply(usage(1, 9_000, 3_345, 5_000, 4_000))`，取 `buffer(120, 24, &mut state)`。
  - 行位置：`y = sidebar_page(&rows) + offset`（`offset 0` 是上下文、`1` 是 token），x 从 `7` 起（与现有 `panel_field` 同一套坐标）。
  - 上下文行：占比 `9_000 / 200_000 = 0.045`，值列 33 → `N = ceil(1.485) = 2` → `frame[(7, y)].bg == Color::DarkGray`、`frame[(8, y)].bg == Color::DarkGray`、`frame[(9, y)].bg != Color::DarkGray`。
  - token 行：占比 `12_345 / 100_000 = 0.12345` → `N = ceil(4.07) = 5` → `x = 7..=11` 是 `DarkGray`，`x = 12` 不是。
  - 同一帧里顺带断言文字没被色条改掉：这两行的文本仍含 `9,000 / 20万（4%）` 与 `1.2万 / 10万`（值列仍右对齐）。
  - 标签列不涂：`frame[(6, y)].bg != Color::DarkGray`。
- 「没有预算时不涂」：用 `state_without_budget()`（`2039-2044`，`budget_limit = None`）`apply` 同一条 usage 后：
  - token 行（offset 1）在 `x = 7..40` 之间没有任何 `DarkGray`；
  - 上下文行（offset 0）**照涂**（它有分母 `context_window`）：`x = 7, 8` 仍是 `DarkGray`。
- 回归：窄档（80 列、值列 21 列）色条也在——窄档丢的是百分比，不是色条；`cargo test` 全绿。

## 不做什么

- 不涂「回合」「输入」「输出」「缓存」四行（spec §3 第一条）。
- 不给状态行加色条；不动 `usage_summary`；不动降级链与列宽计算。
- 不新增主题/配置项，不用 `DarkGray` 以外的底色。

## Comments

- **落地**：`Panel::lines` 的行元组加了第四个字段 `Option<f64>`（占比），`row()` 多收一个 `share`；底色只涂值列左起 `N = ceil(占比 × 值列宽)` 列，切段走 `truncate_columns`（按显示列，不从 `万` / `（` 中间劈开），`clamp(0.0, 1.0)` 保证撞顶时正好涂满、不越出值列。标签与中间那个空格不涂，右对齐的前导留白算在值列里 —— 色条从值列左缘起。
- **只有两行有占比**：上下文按 `context_window`、花销按 `budget_limit`；`last_input` 或 `budget_limit` 为 `None` 时不涂（`回合` / `输入` / `输出` / `缓存` 一律 `None`）。`value_columns` 的计算、百分比是否丢的判定、缓存行是否加入、`fit()`、行序与标签列宽**一行未改**。
- **测试**（`tests/render_layout.rs`）：`the_context_and_spend_rows_carry_a_share_bar` 读 `TestBackend` 缓冲的 `bg` —— 上下文行 `x = 7, 8` 是 `DarkGray`、`x = 9` 不是，花销行 `x = 7..=11` 是、`x = 12` 不是，标签列 `x = 6` 不是，同帧断言两行文字仍是 `9,000 / 20万（4%）` 与 `1.2万 / 10万`；`a_row_without_a_denominator_carries_no_share_bar`（没额度时花销一行不涂、上下文照涂）；`the_share_bar_survives_the_narrow_sidebar`（80 列 21 列值列下涂一格）。
- **附带**：`NumberStyle` / `UiSettings` 的 `Default` 按 clippy 的 `derivable_impls` 改成派生（`#[default]` 标在 `Cn` 上），缺省仍然只写一处。
