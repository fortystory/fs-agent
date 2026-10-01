# 面板六行改用数字制式

Type: implement
Status: done
Blocked by: 01

> 规格：`.scratch/usage-stats-format/spec.md` §3（底色不占列、既有降级链一行不改——本票只做前半）、§4（明确不动的三处）与「测试决定」里的回归一条。
> 票 01 已给好 `wording::compact` / `NumberStyle` 与 `[ui] number_style` 的解析；这一票把**面板那六行**从 `thousands` 换到 `compact`，别的通道一个字都不动。

## 目标

- `Panel::lines`（`src/render/panel.rs:67-113`）那六行的数字改用 `wording::compact(…, facts.number_style)`。
- 让面板拿到制式：`SessionFacts` 加 `number_style`，两处构造 `SessionFacts` 的地方填上配置值。
- 既有断言 `thousands` 输出的测试跟着改成 `compact` 的输出。

## 现状

- `Panel::lines` 现在攒出六行：`上下文`（`wording::context_pair`，`73` / `77` / `80`）、`token`（`token_pair`，`82`）、`回合`（`thousands`，`90`）、`输入`（`thousands`，`97`）、`输出`（`thousands`，`102`）、`缓存`（`cache_pair`，`83`）。
- `context_pair` / `token_pair` / `cache_pair`（`src/render/wording.rs:1242` / `1229` / `1255`）里的私有 `pair()`（`1221`）走 `thousands`。这三个函数**只被 `panel.rs` 调用**（全仓只有 `src/render/panel.rs:73-83` 与 `tests/wording.rs`），所以给它们加一个制式参数碰不到诊断通道。
- `SessionFacts` 在 `src/render/tui.rs:255-280`，由 `src/cli.rs` 两处构造：单 agent 会话 `328-338`、讨论 `609-624`（两处作用域里都有 `config`）。测试夹具 `facts()` 在 `tests/render_layout.rs:21-33`，是结构体字面量——加字段会让所有用它的测试编译不过。

## 落点

- `src/render/wording.rs`：`pair` / `token_pair` / `context_pair` / `cache_pair` 加 `style` 参数。
- `src/render/panel.rs`：`Panel::lines` 的六行调用点。
- `src/render/tui.rs`：`SessionFacts::number_style`。
- `src/cli.rs`：两处 `SessionFacts { … }` 字面量。
- `tests/render_layout.rs`、`tests/wording.rs`：夹具与断言。

## 具体行为

1. **`wording` 那三个 pair 函数加 `style`**：`token_pair(used, limit, style)`、`context_pair(used, usable, with_share, style)`、`cache_pair(cached, miss, style)`；私有 `pair(left, right, style)` 改用 `compact`。`token_pair` 的 `None` 分支（`thousands(used)`）与 `context_pair` 的 `PANEL_UNKNOWN` 分支照旧。
   - `thousands` 与 `usage_summary` 不动。签名变了，但调用点只有面板与 `tests/wording.rs`。
2. **`SessionFacts` 加 `pub number_style: NumberStyle`**（`src/render/tui.rs`）。它派生 `Default`，所以 `NumberStyle` 必须 `Default`（票 01 已给）。文档注释一句话：这一页数字的书写制式，组装时注入。两处 `SessionFacts { … }`（`src/cli.rs:328-338`、`609-624`）补 `number_style: config.number_style`。
3. **面板六行**（`src/render/panel.rs`）：
   - 上下文三处 `context_pair(…)` 加 `facts.number_style`；`token_pair`、`cache_pair` 同；
   - `回合` / `输入` / `输出` 三处 `thousands(x)` → `compact(x, facts.number_style)`。
   - `value_columns` 的计算（`68`）、上下文百分比是否丢的那两处判定（`73-81`）、缓存行是否加入（`105-107`）、`fit()`（`153-164`）、标签列宽与行序**一行都不改**（spec §3）。
   - 注意一个直接后果：降级判定算的是 `compact` 之后的串（数变短了），但**代码本身不动**。
4. **测试跟着改**（默认制式 `cn`，换算见票 01 的表）：
   - `tests/render_layout.rs` 的 `facts()`（`21-33`）加 `number_style: wording::NumberStyle::Cn`。
   - `the_panel_reads_its_numbers_off_the_stream`（`2000-2037`）：`9,000 / 200,000（4%）` → `9,000 / 20万（4%）`；`12,345 / 100,000` → `1.2万 / 10万`；`输入 9,000` / `输出 3,345` / `缓存 5,000 / 4,000` 都在门槛之下，不变。
   - `the_panel_pads_its_labels_and_aligns_its_values_like_the_snapshot`（`2229-2261`）：`panel[0]` 的期望串改成 `9,000 / 20万（4%）`，它前面的空格数按 `text_columns` 重算（值列 33 列）；`2245-2250` 那个 `ends_with("000") || ends_with("345")` 的循环把 `panel[1]` 去掉或放宽（它现在以 `万` 结尾）。
   - `the_context_numerator_is_the_last_call_while_the_spend_accumulates`（`2175-2203`）：`15,000 / 200,000（7%）` → `1.5万 / 20万（7%）`；`27,000 / 100,000` → `2.7万 / 10万`；`24,000` → `2.4万`；`3,000` 不变（小于门槛）。
   - `a_session_with_no_allowance_shows_its_spend_alone`（`2137-2146`）：`12,345` → `1.2万`，那条 `!text.contains("12,345 / ")` 跟着改成 `1.2万 / `。
   - `tests/wording.rs` 的 `the_panel_texts_read_like_the_prototype`（`905-937`）：`token_pair` / `context_pair` / `cache_pair` 的调用各加 `NumberStyle::Cn`，期望值按票 01 的表换算；`thousands` 的三条断言（`907-909`）**留着**。
5. **降级测试的处置（本票的判断，写进 Comments）**：§3 说 `the_narrow_sidebar_keeps_six_fields_and_drops_the_percentage_when_it_must` 与 `a_number_too_wide_for_the_value_column_loses_its_separators_before_its_digits`「必须仍然绿」。它们断言的是具体字符串，换制式后必然要改；**要保的是那几档降级行为，不是那几个字面量**：
   - `a_cache_split_too_wide_for_its_column_is_left_out`（`2087-2134`）：`1,234,567 / 9,876,543` → `123.5万 / 987.7万`（16 列）；窄档值列 13 列仍放不下 → 整行走，宽档 22 列放得下 → 回来。降级行为不变。
   - `a_number_too_wide_for_the_value_column_loses_its_separators_before_its_digits`（`2263-2298`）：fixture 的 `input 1,234,567` 在 `compact` 下变成 `123.5万 / 20万` 这种短串，`fit()` 的**「去千分位」那一档再也触达不到**——面板里 `thousands` 只在 `< 10000` 时出现，最长 `9,999`，而真实值列最少 16 列。按 §3「降级链一行不改」，`fit()` 原样保留；这条测试改成断言 `compact` 之后的值，并在注释里如实写明那一档现在没有回退路径可达（在票的 Comments 里记下来，供 spec 作者决定要不要另开票）。
   - `the_narrow_sidebar_keeps_six_fields_and_drops_the_percentage_when_it_must`（`2046-2084`）：fixture 是 `last_input = 12_345` / `context_window = 200_000`，`cn` 下上下文对是 `1.2万 / 20万（6%）`，18 列 **放得进** 21 列的值列，所以「丢掉百分比」的前提不再成立。处置：把这条改成断言**百分比留着**（相应改名/改注释）；上下文行的降级仍由上面两条在面板层守着。**不要**去改 `context_window` 之类的事实值把它凑成溢出——那不是这条测试在测的东西。
6. **不动**（spec §4）：`usage_summary` 与它的四个调用点（`src/render/plain.rs`、`src/render/headless.rs`、`src/render/tui.rs`、`src/cli.rs` 的 `sessions stats`）；状态行的 `context_share`（`src/render/wording.rs:1381`）与 `status_row`；`thousands` 本身。

## 测试

- `cargo test` 全绿，含第 4、5 条改过的断言。
- 断言面：值列里读到的文本（现有 `screen` / `panel_text` / `panel_field` 那套）；`--plain` / headless / `sessions stats` 的 `usage_summary` 输出**一个字都不变**（现有测试应原样通过）。
- 专门的回归：`the_panel_reads_its_numbers_off_the_stream` 与三条降级测试按新制式通过。

## 不做什么

- 不加色条（票 03）；`row()` 的行形状本票不动。
- 不动降级链的代码、面板行序、标签列宽与高度裁剪。
- 不给 `usage_summary` / `sessions stats` / headless 换单位，不给状态行加任何东西。

## Comments

- **落地**：`pair` / `token_pair` / `context_pair` / `cache_pair` 各加一个 `style` 参数（`thousands` 与 `usage_summary` 未动）；`SessionFacts` 加 `number_style`，两处 `SessionFacts` 字面量（`src/cli.rs`）与四处测试夹具（`render_layout.rs` ×3、`render_tui.rs`、`history_replay.rs`、`ask_user_question_tui.rs`）跟着补；面板六行的 `thousands` → `compact`，`value_columns` 的计算、百分比是否丢的判定、缓存行是否加入、`fit()`、行序与标签列宽**一行未改**。
- **降级测试的处置**（票「具体行为 5」要求的判断），三条都只改字面量、不改降级行为：
  - `the_narrow_sidebar_keeps_six_fields_and_drops_the_percentage_when_it_must` → 改名 `..._and_their_percentage`：`1.2万 / 20万（6%）` 是 18 列、放得进 21 列的值列，「必须丢百分比」的前提不再成立，所以改成断言百分比留着（没有去动 `context_window` 之类的事实值凑溢出）。
  - `a_cache_split_too_wide_for_its_column_is_left_out`：期望串换成 `123.5万 / 987.7万`（17 列）；窄档 13 列仍放不下 → 整行走，宽档 22 列放得下 → 回来，降级行为不变。
  - `a_number_too_wide_for_the_value_column_loses_its_separators_before_its_digits` → 改名 `..._no_longer_needs_the_bare_form`。
- **留给 spec 作者的一件事**：`fit()` 的「去千分位」那一档在面板里**已经不可达** —— 面板里 `thousands` 只剩 `< 10000` 的数字（最长 `9,999`，13 列），而真实值列最少 16 列。按 spec §3「降级链一行不改」保留了代码，测试改成断言 `compact` 之后的形态并在注释里写明这一档没有回退路径可达。要不要为 `fit()` 另开一票（删掉那一档或给它一个新用途），由 spec 作者决定。
- **对齐断言**：`the_panel_pads_its_labels_and_aligns_its_values_like_the_snapshot` 的前导空格数不再手写，改成分别断言「标签列六列」「一个空格后值右贴齐」「整行 40 列」。
- **测试**：`cargo test` 全绿；`--plain` / headless / `sessions stats` 的 `usage_summary` 断言原样通过（一个字没动）。
