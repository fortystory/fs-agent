# 02 — 用量读数、鉴权提示与文档

Type: implement
Status: done
Blocked by: 01

> 规格：[`../spec.md`](../spec.md) §5–§8。票 01 让 MiniMax 跑得起来，本票让它**读得对**：
> 缓存命中不再显示成零，401/403 给一句能直接照做的提示，README 把两条 profile 与两个模型
> 名字摆出来（含 TOML 里给带点号的 id 加引号那个坑）。

## 目标

一次 MiniMax 调用之后，界面与 `sessions stats` 里的缓存命中读数来自
`prompt_tokens_details.cached_tokens`；鉴权失败时错误正文里点明「订阅 Key 与按量 Key 不通用、
两个站的端点不通用」；README 的 provider 表与内置模型那一行都含 MiniMax，并有一段能直接抄的
配置样例。

## 现状（2026-10-08 核实，改前先复核）

- **用量归一化**：[`normalize_usage`](../../../src/provider/openai.rs) 在 `:780` 起，按
  `Vendor` 分两个分支（Kimi 读顶层 `cached_tokens`、DeepSeek 读 `prompt_cache_hit_tokens` /
  `prompt_cache_miss_tokens`），两者都回落过
  `prompt_tokens_details.cached_tokens`。`Usage` 的四个计数在 `src/events.rs`。
- **调用点**：`StreamDecoder` 在 `:550` 与 `:556` 用它，`vendor` 来自
  `self.caps.vendor`。
- **鉴权提示**：[`auth_hint`](../../../src/provider/openai.rs) 在 `:163-176`，按
  `profile.vendor` 返回一句话，并在 `with_auth_hint` 里拼进 401/403 的错误正文。
- **测试先例**：`tests/provider_adapter.rs:416` 附近有三家厂商的用量断言（Kimi 的
  `cached_tokens`、DeepSeek 的两个顶层字段）。
- **README**：[`README.md:70-78`](../../../README.md) 是 provider 表 + 内置模型 id 那一行，
  `:82-133` 是一份够用的配置样例（含 `[pricing.deepseek-flash]`）。
- **feature 索引**：[`.scratch/README.md`](../../README.md) 那张表，新增一行。

## 落点

`src/provider/openai.rs`（`normalize_usage`、`auth_hint`）、`tests/provider_adapter.rs`、
`README.md`、`.scratch/README.md`。

## 具体行为

1. **用量**：`normalize_usage` 加 `Vendor::MiniMax` 分支 —— `cached` 读
   `prompt_tokens_details.cached_tokens`（不回落顶层，MiniMax 不报那个字段），
   `miss = prompt_tokens − cached`（饱和减）。其余三项（输入、输出、推理）与今天同路。
2. **鉴权提示**：`auth_hint` 的 `Some(Vendor::MiniMax)` 分支说清三件事 —— 订阅 Key 与按量
   Key 不通用（`sk-cp-` 前缀是 M Plan 的）；国际站 `api.minimax.io` 与国内站 `api.minimax.cn`
   是两套端点，Key 与 base_url 不能互换；一次 401 也可能是这个套餐里没有你请求的那个 model。
3. **README**：provider 表加两行、内置模型 id 那一行加两个、配置样例里加一段注释掉的 MiniMax
   写法（基础写法 + 用国内站时把 `provider` 指到 `minimax-cn` + 带点号 id 的引号坑 +
   `[pricing."MiniMax-M3"]` 的形状）。
4. **索引**：`.scratch/README.md` 表格尾部加一行 `minimax-provider/`，形态 `spec`，一句话说
   清「MiniMax 作为第三个厂商：两条 profile + 两个 M3 系模型」。

## 验证

`cargo test` + `cargo clippy --all-targets` + `cargo fmt --check`，外加两道文档护栏
（`cargo test` 里的 doc 测试与 `scripts/check-doc-size.py`，若它管到 README）：

1. **嵌套缓存字段**：`normalize_usage(Vendor::MiniMax, …)` 喂一份
   `{"prompt_tokens": 1200, "completion_tokens": 300, "prompt_tokens_details": {"cached_tokens": 800}}`
   得到 `cached_tokens == 800`、`miss_tokens == 400`、`input_tokens == 1200`。
2. **没有嵌套字段时**：`cached == 0`、`miss == input`（不 panic、不猜）。
3. **顶层 `cached_tokens` 不作数**：给 MiniMax 喂顶层 `cached_tokens: 800` 而**不带**嵌套字段，
   断言 `cached == 0` —— 钉住「这里不兼容那套形状」。
4. **三家互不串**：既有的 Kimi / DeepSeek 用量断言不改一个字。
5. **鉴权提示：不新增测试。** 实现期核了一遍 —— `auth_hint` 的既有两条分支（Kimi / DeepSeek）
   今天也没有测试，而它只从 `post()` 的 401/403 路径到达，要在测试里走一遍就得真发一次 HTTP，
   而测试套件里没有 mock server 的先例（`tests/provider_adapter.rs` 从头到尾只驱动纯函数）。
   本轮不为它单开一条测试路径；文案的正确性靠 review 与真机 401 观察。这一条是**票面收缩**，
   不是漏做。
6. **README 与索引**：`README.md` 里能 grep 到两条 profile 名与两条 model id；
   `.scratch/README.md` 里能 grep 到 `minimax-provider`。

## 不做什么

- 不动价格表的机制（不加内置价，样例里给形状就够）。
- 不改 `classify_status`（限额说法仍走既有的 `looks_like_quota` 判据）。
- 不改任何界面措辞（用量与缓存的显示早已按 `Usage` 算）。
- 不为 MiniMax 新开逐面文档（`docs/` 里没有 provider 那一面，README 就是它那一面）。

## 评论

同日落地（2026-10-08）。落点：`src/provider/openai.rs`（`normalize_usage` 的
`Vendor::MiniMax` 分支、`auth_hint` 的 MiniMax 一句）、`tests/provider_adapter.rs`（2 条新
测试）、`README.md`（provider 表两行、内置模型 id 两个、配置样例一行、规模数字跟实测）、
`.scratch/README.md`（本 feature 的索引行）。

两处如实记下：

1. **缓存分支不回落顶层字段**：MiniMax 只报 `prompt_tokens_details.cached_tokens`，所以这一支
   只读嵌套那一个，另配一条测试钉住「顶层 `cached_tokens` 在这里不作数」——认得它的话，一次真
   调用里的「零缓存」会被悄悄读成一个命中。
2. **鉴权提示没有测试**（验证第 5 条已就地改成收缩说明）：`auth_hint` 只能从 `post()` 的
   401/403 路径到达，而测试套件里没有 mock server 的先例 —— 既有的 Kimi / DeepSeek 两句也是
   这样，不为 MiniMax 单开一条。
