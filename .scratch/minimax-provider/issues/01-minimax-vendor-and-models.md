# 01 — MiniMax 作为第三个厂商：两条 profile 与两个模型

Type: implement
Status: done
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1–§4、§7。本票让 `heng --model MiniMax-M3` 起得来：厂商
> 一档、两条内建 profile、两条能力表条目，以及那个让思考走 `reasoning_content` 的
> `reasoning_split`。用量读数归 [票 02](02-usage-and-docs.md)。

## 目标

`[providers.minimax]` / `[providers.minimax-cn]` 两条内建 profile 存在（`base_url` 与 Key 环境
变量都对），`Vendor::MiniMax` 落位；`MiniMax-M3.1-Flash-Preview` 与 `MiniMax-M3` 进了
`KNOWN_MODELS`，启动校验与 `[discussion]` 池子都不再拒绝它们；发出去的 body 里带
`reasoning_split: true`，于是思考走 `reasoning_content` 而不是 `content` 里的 `<think>` 标签。

## 现状（2026-10-08 核实，改前先复核）

- **`Vendor`**：[`src/config.rs:207-211`](../../../src/config.rs) 只有 `Kimi` / `DeepSeek`，
  `#[non_exhaustive]`；`as_str()` 在 `:213-221`、`hosts()` 在 `:227-232`（注释先例：Kimi 一家
  三台主机）。
- **内建 profile**：[`src/config.rs:256-278`](../../../src/config.rs) 三条
  （`kimi` / `kimi-code` / `deepseek`），字段是 `name` / `vendor` / `base_url` / `key_env` /
  `alt_key_envs`。`env_prefix` 在 `:2289-2299`，把 `minimax-cn` 拼成 `MINIMAX_CN`。
- **跨域守卫**：`resolve_providers` 在 [`src/config.rs:1885-1959`](../../../src/config.rs)，判据
  在 `:1929-1943` —— 说话的是 Key 的**出身**（`vendor_of_key_env`，`:2301-2307`），不是段名。
- **内置模型表**：[`src/config.rs:1875-1883`](../../../src/config.rs) 的 `BUILTIN_MODELS`
  （7 条），`resolve_models` 在 `:1997` 起。
- **能力表**：[`src/provider/capability.rs`](../../../src/provider/capability.rs) 的
  `ModelCaps` 在 `:41-66`、`KNOWN_MODELS` 在 `:69-80`、`caps_for` 在 `:83-96`，三个构造函数
  在 `:102-155`。`caps_for` 的未知 id 分支返回 `UnknownModel`（不是默认值）。
- **启动校验**：`src/cli.rs` 的 `validate_models`（`caps_for` 逐条过一遍）与 `:343` 处的
  双保险 —— 未登记 id 在会话起来之前就报错。
- **请求体**：[`build_body`](../../../src/provider/openai.rs) 在 `:243` 起；`stream_options` 在
  `:271-276`，参数按能力位过滤在 `:289` 起。**今天没有任何厂商特有顶层字段的出口**。
- **全表自洽断言**：[`tests/provider_adapter.rs:67-95`](../../../tests/provider_adapter.rs) 三条
  遍历 `KNOWN_MODELS` / `BUILTIN_MODELS` 的测试会自动覆盖新条目，不用手写。
- **讨论者名字**：判据在 `src/config.rs:2186-2204`（非空、无空白、无控制字符、≤ 32 字符），
  所以 `MiniMax-M3` 这种带点号与短横线的 model id 能直接当短形式的名字。

## 落点

`src/config.rs`（`Vendor`、`BUILTIN_PROVIDERS`、`BUILTIN_MODELS`）、
`src/provider/capability.rs`（`ModelCaps` 新位、`KNOWN_MODELS`、`caps_for`、新的构造函数）、
`src/provider/openai.rs`（`build_body`）、`tests/config_profiles.rs`、
`tests/provider_adapter.rs`。

## 具体行为

1. **`Vendor::MiniMax`**：`as_str()` → `"MiniMax"`；`hosts()` →
   `&["api.minimax.io", "api.minimax.cn"]`。
2. **两条内建 profile**：`minimax`（`https://api.minimax.io/v1`，`MINIMAX_API_KEY`）、
   `minimax-cn`（`https://api.minimax.cn/v1`，`MINIMAX_CN_API_KEY`），`vendor` 都是
   `Vendor::MiniMax`，`alt_key_envs` 留空。
3. **两条内置模型**：`("MiniMax-M3.1-Flash-Preview", "minimax")`、`("MiniMax-M3", "minimax")`。
4. **能力表**：`ModelCaps` 加一个布尔位 `reasoning_split`（文档注释写清它是「输出格式开关：
   思考进 `reasoning_content` 还是留在 `content` 的 `<think>` 里」），三个既有构造函数都填
   `false`；新增一个 MiniMax 的构造函数，两条 id 都走它，差别只有
   `supports_reasoning_effort`（M3.1 = true，M3 = false）。数字照 spec §3 那张表，其中
   `max_output_tokens` 取窗口值并在注释里点明官方没给输出上限这处缺口。
5. **`build_body`**：`caps.reasoning_split` 为真时插顶层 `"reasoning_split": true`。
6. **诊断**：`UnknownModel` 的错误正文会列出 `KNOWN_MODELS`，新 id 自动出现在里面。

## 验证

`cargo test` + `cargo clippy --all-targets` + `cargo fmt --check`；重点断言：

1. **两条 profile 的端点与 Key 变量**：`resolve(None, &env(&[]))` 之后
   `providers["minimax"].base_url == "https://api.minimax.io/v1"`、`key_env == "MINIMAX_API_KEY"`；
   `minimax-cn` 同理（`https://api.minimax.cn/v1` / `MINIMAX_CN_API_KEY`），且两者的
   `vendor == Some(Vendor::MiniMax)`。
2. **两条 id 都进得了门**：`caps_for("MiniMax-M3.1-Flash-Preview")` 与 `caps_for("MiniMax-M3")`
   都 `Ok`，`vendor == Vendor::MiniMax`、窗口 1_048_576、`supports_tools`、
   `requires_reasoning_replay`、`max_tokens_field == MaxCompletionTokens`、
   `min_cacheable_tokens == 512`、`supports_prompt_cache_key == false`。
3. **M3.1 与 M3 的差别只有推理档位**：前者 `supports_reasoning_effort == true`，后者 `false`；
   两条都 `reasoning_split == true`。
4. **`build_body` 带上开关**：`build_body(&request("MiniMax-M3", …), caps_for("MiniMax-M3"))`
   的 body 里 `reasoning_split == true`、`max_completion_tokens` 按 `max_output_tokens` 走、
   **没有** `prompt_cache_key`（哪怕 `request.cache_key` 是 `Some`）；Kimi 那条 body 里
   **没有** `reasoning_split`（钉住「新位默认 false」）。
5. **跨域守卫仍然结构性地拦人**：`[providers.minimax-cn] api_key_env = "DEEPSEEK_API_KEY"`
   报 `CrossVendorKey`；`MINIMAX_API_KEY` 配到 `https://api.deepseek.com` 同样被拒。
   （`MINIMAX_API_KEY` 落在 `api.minimax.cn` 主机上**放行** —— 两套系统都是 MiniMax。）
6. **讨论池收得下**：`[discussion] debaters = ["MiniMax-M3", "kimi-k3"]` 解析成功、两个名字
   就是两个 model id。
7. **既有的三条遍历断言**（内置模型都有能力条目、窗口自洽、已知 id 都在错误正文里）自动覆盖
   新条目且必须仍然通过。

## 不做什么

- 不写 Anthropic Messages 适配器（spec「明确不做」）。
- 不登记 M2.x 一族。
- 不给 `thinking` / `service_tier` / 新推理档位加旋钮。
- 不动 `classify_status`、投影、事件 schema、权限那几层。

## 评论

同日落地（2026-10-08）。落点：`src/config.rs`（`Vendor::MiniMax` + `hosts()`、两条
`BUILTIN_PROVIDERS`、`BUILTIN_MODELS` 两条）、`src/provider/capability.rs`（`ModelCaps` 新位
`reasoning_split`、`KNOWN_MODELS` 两条、`caps_for` 两条分支、新函数 `minimax_caps`）、
`src/provider/openai.rs`（`build_body` 发 `reasoning_split`）、`tests/config_profiles.rs`
（3 条新测试）、`tests/provider_adapter.rs`（2 条新测试）。

实现期如实记下的三处：

1. **能力表那位叫 `reasoning_split`**（票面只写「一个布尔位」）：与线上字段同名，落点在
   `ModelCaps` 的 `supports_stream_options` 与 `max_tokens_field` 之间。
2. **顺手改了两处硬编码计数**：`tests/config_profiles.rs` 里 `models.len() == 7` → `9`、
   `providers.len() == 3` → `5`（内建表长了，这两条断言是它们的影子）。
3. **入口文档的体量棘轮显式上调一次**：`README.md` 的 provider 表加两行、内置模型 id 加两个、
   配置样例加一行（`sk-cp-` 前缀与带点号 id 的 TOML 引号坑），实测 18,211 / 319 越过了原先的
   17,928 / 316。按 `scripts/check-doc-size.py` 里既有的先例（该文件允许「显式动作 + 理由 +
   记一条带日期的注」）上调并记了注；同一天新增的 `.scratch/minimax-provider/` 索引行也按常规
   上调 `11,334 / 67`。`cargo test`（1,398 条）、`cargo fmt --check`、`cargo clippy
   --all-targets`（新代码零告警；`collapsible_if` 那几条是 Rust 1.94 起的存量）、
   `scripts/check-doc-size.py`、`scripts/check-language.py` 全绿。
