# 数字制式：`compact` 与 `[ui] number_style`

Type: implement
Status: done
Blocked by: —

> 规格：`.scratch/usage-stats-format/spec.md` §1（单位制式与换算规则）、§2（配置项 `[ui] number_style`）、「测试决定」的前三条与「明确不做」。
> 这张票只做**纯函数与配置**：把 `compact` / `NumberStyle` 落好、把 `[ui]` 解析好、把 README 与测试写好。面板换用它（票 02）与色条（票 03）都不在这里。

## 目标

- 新增纯格式化函数 `wording::compact(value: u64, style: NumberStyle) -> String`：`Cn` 用万/亿，`Si` 用 k/M/G，`value < 10_000` 一律退回 `thousands`。
- 配置文件新增 `[ui] number_style = "cn" | "si"`，缺省 `cn`；非法值报错并指出键名；不新增 CLI 旗标、不读环境变量。
- README「配置」一节补一句说明与完整示例里的一行。

## 现状

- `src/render/wording.rs:1208` 的 `thousands(value)` 只做千分位（`12,345`）。它同时是**诊断通道**（`usage_summary`，`src/render/wording.rs:243-248`）与状态行的函数，**签名不能动**；`compact` 在它上面分派。
- `src/config.rs` 里每个顶层小节都走「`Raw*` → `resolve_*` → `Config` 字段」这一套。`[goals]` 是最新的样板：`RawGoals` 在 `src/config.rs:751-758`，`resolve_goals` 在 `448-499`，缺省常量在 `429-439`，字段在 `Config` 的 `398-399`，`resolve()` 里挂接在 `629`，构造在 `642-657`。
- 现在 `Config` 里没有任何 `[ui]` 一类的显示类小节（spec「问题陈述」第 3 条）。

## 落点

- `src/render/wording.rs`：`NumberStyle` 与 `compact`，放在 `thousands`（`1207-1218`）旁边。
- `src/config.rs`：§2 的那九个落点（下面逐条列出）。
- `README.md`：「配置」一节（46-119 行）。
- `tests/wording.rs`：`compact` 的纯函数测试。
- `tests/config_profiles.rs`：`[ui]` 的解析测试。

## 具体行为

### 1. `NumberStyle` 与 `compact`（`src/render/wording.rs`）

- `NumberStyle` **定义在 `src/render/wording.rs`**（spec §1 明写：它管的是「给人看的文本怎么写」；`config.rs` 只是持有它，方向与 `Mode` 那类配置枚举相反）。两值枚举 `Cn` / `Si`，派生 `Debug, Clone, Copy, PartialEq, Eq`，并 `impl Default` 返回 `Cn`（`UiSettings::default` 与 `SessionFacts` 的 `Default` 派生都要用它）。
- `pub fn compact(value: u64, style: NumberStyle) -> String`：
  - **门槛在先**：`value < 10_000` → `thousands(value)`，两套制式一样（`9_999` 就是 `9,999`）。
  - `Cn`：`[10^4, 10^8)` → 除以 `10_000`，后缀 `万`；`[10^8, ∞)` → 除以 `10^8`，后缀 `亿`。
  - `Si`：`[10^3, 10^6)` → 除以 `10^3`，后缀 `k`；`[10^6, 10^9)` → 除以 `10^6`，后缀 `M`；`[10^9, ∞)` → 除以 `10^9`，后缀 `G`。因为门槛在先，`Si` 的 `k` 档实际从 `10_000` 才开始（`1_000`…`9_999` 仍是千分位）。
  - **一位小数**：`format!("{:.1}")` 之后，**恰好是整数时去掉 `.0`**（`10_000` → `1万`，`12_345` → `1.2万`）。
  - **单位按原始 `value` 的量级选，不因为舍入进位换档**：`99_999_999` 是 `9999.9999`，格式化去零后写作 `10000万`（这是 §1「舍入用 `format!` 默认」的字面结果，别自作主张挪进「亿」档）。
  - `thousands` 原样保留（诊断通道还要用）。

  写测试时照这张表断言：

  | 输入 | `Cn` | `Si` |
  | --- | --- | --- |
  | `0` | `0` | `0` |
  | `999` | `999` | `999` |
  | `9_999` | `9,999` | `9,999` |
  | `10_000` | `1万` | `10k` |
  | `12_345` | `1.2万` | `12.3k` |
  | `1_234_567` | `123.5万` | `1.2M` |
  | `99_999_999` | `10000万` | `100M` |
  | `100_000_000` | `1亿` | `100M` |
  | `1_000_000_000` | `10亿` | `1G` |

### 2. 配置解析（`src/config.rs`）——九个落点

照 `[goals]` 那一套走：

1. `use crate::render::wording::NumberStyle;`（方向与 `Mode` 相反，spec §1 已定）。
2. `pub struct UiSettings { pub number_style: NumberStyle }`，`impl Default` → `Cn`。
3. `struct RawUi { number_style: Option<String> }`，派生 `Debug, Default, Deserialize` 并 `#[serde(deny_unknown_fields)]`。
4. `RawConfig` 加 `ui: Option<RawUi>`（`RawConfig` 在 `src/config.rs:718-745`）。
5. `fn resolve_ui(raw: Option<&RawUi>) -> Result<UiSettings, ConfigError>`：`None` → `Default`；`"cn"` → `Cn`；`"si"` → `Si`；别的 → `ConfigError::UnknownNumberStyle { value }`。新错误变体挨着 `UnknownMode` / `UnknownSandboxMode`（`ConfigError` 在 `src/config.rs:1604-1674`）写，文案要**点出键名** `[ui] number_style` 并列出两个合法值。
6. `resolve()` 里挂一行，照 `let goals = resolve_goals(raw.goals.as_ref())?;`（`src/config.rs:629`）。
7. `Config` 结构加 `pub number_style: NumberStyle`（`Config` 在 `src/config.rs:361-400`）。
8. `Ok(Config { … })` 的构造里填 `number_style: ui.number_style`（构造在 `642-657`）。
9. 缺省只说一次 `Cn`：`impl Default for NumberStyle` 就是那处缺省（`UiSettings::default` 复用它；不要再写第二份字面量）。

**不新增 CLI 旗标**、不读环境变量（spec §2 与「明确不做」）。

### 3. README（`README.md` 46-119 行）

- 「配置」一节正文补一句：数字默认写中文制式（万/亿），小于 `10000` 的仍写千分位；`[ui] number_style = "si"` 换成 `k` / `M` / `G`。
- 完整示例（`60-108` 行那份 toml 代码块）里加一段 `[ui]`，照邻居的注释密度写：

  ```toml
  [ui]
  number_style = "cn"          # "cn" 万/亿（默认）或 "si" k/M/G；小于 10000 仍是千分位
  ```

## 测试

- `tests/wording.rs`：新增一条 `compact` 测试（挨着 `the_panel_texts_read_like_the_prototype`，`905` 一带），对上面那张表逐档断言，含边界 `9_999` / `10_000` / `99_999_999` / `100_000_000`、去零（`10_000` → `1万`、`100_000_000` → `1亿`）与门槛（`9_999` 两套都是 `9,999`）。
  - **不要**改 `thousands` 的既有断言（`tests/wording.rs:907-909`）——它没变。
- `tests/config_profiles.rs`：照 `[goals]` 那一组（`930-1007`）新开一组 `[ui]`：
  - `resolve(None, &env(&[])).unwrap().number_style == NumberStyle::Cn`（缺省是 `cn`）；
  - `resolve(Some("[ui]\nnumber_style = \"si\"\n"), …).number_style == NumberStyle::Si`；
  - `"CN"`（大小写不合）与 `"wan"` 都 `unwrap_err()`，错误串里含 `number_style`；
  - `[ui]\nnumber_style = "cn"\nnumber_ways = 1` 这类未知键被 `deny_unknown_fields` 拒（错误串含未知键名）。
- 跑 `cargo test`；这一票不该动任何渲染行为。

## 不做什么

- 不改 `thousands`、`usage_summary`（`src/render/wording.rs:243-248`），也不改 `context_pair` / `token_pair` / `cache_pair` 的现有签名与输出（票 02 才动面板调用点）。
- 不改 `src/render/panel.rs`（票 02）、不加色条（票 03）。
- 不做「万亿 / 兆」、不做自定义单位表、不读环境变量、不加 CLI 旗标。

## 评论

- **落地**：`NumberStyle` / `compact` 落在 `src/render/wording.rs` 的 `thousands` 旁边，`thousands` 未动（它的三条既有断言也原样留着）。`[ui]` 的九个落点照 `[goals]` 那一套走；缺省只在 `impl Default for NumberStyle` 一处写字面量，`UiSettings::default` 复用它。非法值走新变体 `ConfigError::UnknownNumberStyle`，文案点出 `[ui] number_style` 与两个合法值。
- **测试**：`tests/wording.rs` 新增 `a_count_is_written_in_wan_and_yi_or_in_si_prefixes`（门槛两套一致、逐档代表值、`99_999_999` / `100_000_000` 两个边界、整数去零）；`tests/config_profiles.rs` 新增三条（缺省 `cn`、`[ui]` 取值、非法值与未知键）。
- **README**：「配置」一节补了一句（默认中文制式、小于 10000 仍是千分位、`si` 换 k/M/G），完整示例里 `[budget]` 之后加了一段 `[ui]`。
