# 02 — DeepSeek 搜索后端：Anthropic Messages + 原生 `web_search`

Type: implement
Status: ready-for-walkthrough
Blocked by: 01

> 规格：[`../spec.md`](../spec.md) §4。事实依据：官方兼容表与定价页（2026-10-03 抓取），
> 以及 [`../research/01-web-search-implementation.md`](../research/01-web-search-implementation.md) §3。

## 目标

把 [票 01](01-web-search-skeleton.md) 的假后端换成真的：用**已有的** `DEEPSEEK_API_KEY` 调
`https://api.deepseek.com/anthropic/v1/messages`，带原生 `web_search` 服务器工具跑一次，
**只从 `web_search_tool_result` 结构化块里取来源** —— 绝不从回复文本里抓 URL。

落地后这张票还剩「发一次真调用」要人做（自动化测试零网络），那时把它标成
`ready-for-walkthrough`。

## 现状（2026-10-03 核实，改前先复核）

- 官方兼容表：`server_tool_use` 与 `web_search_tool_result` 都 **Supported**；
  `anthropic-version` 与 `cache_control` **Ignored**；未知模型名会被自动映射到 `deepseek-flash`。
  **没有 `web_fetch_tool_result`** —— 抓取只能自己发 HTTP。
- 仓库已有 `reqwest 0.13`（`Cargo.toml:22`）；provider 的 client 建法与 `Retry-After` 解析可
  参考（`src/provider/openai.rs:114`、`:177-199`、`:895-905`）。
- 密钥形状：`[providers.*]` 的 `base_url` + `key_env`（`src/config.rs:239-275`，DeepSeek 是
  `https://api.deepseek.com` / `DEEPSEEK_API_KEY`）；打码器从 `providers` 收集值
  （`src/config.rs:590-596`）。**注意**：provider 的 base url 是 OpenAI 格式，Anthropic 格式是
  它加 `/anthropic` —— 所以搜索端点用 `[web] search_base_url` 独立字段（spec §9）。
- 费用事实（官方定价页）：一次搜索 = 一个完整 Messages 轮次，**没有单独的搜索费**；
  `deepseek-flash` 每 1M token：cache miss 输入 $0.15 / $0.30、输出 $0.60 / $1.20
  （off-peak / peak）。peak 是 UTC 01:00–04:00 与 06:00–10:00 的工作日，换算过来正是北京的
  09:00–12:00 与 14:00–18:00。

## 落点

`src/web/search_deepseek.rs`（新）、`src/config.rs`（`search_base_url` 字段与打码器）、`tests/`。

## 具体行为

1. **请求**：`POST <search_base_url>/v1/messages`，body 用 Anthropic 形状 —— `model`
   （默认 `deepseek-flash`）、`max_tokens`（4096）、`max_uses`（5）、
   `tools: [{"type": "web_search…"}]`、一条 user 消息
   （`Perform a web search for the query: <query>`）；头部 `x-api-key`。
2. **解析**：取 `web_search_tool_result` 里的 `web_search_result` 条目，`url` / `title` 直接取，
   日期取 `page_age`；**响应里没有那个块 → `WEB_PROVIDER_ERROR`，不降级**（DSH 的严格模式立场，
   spec §4）。提供方文本不作为答案（`content` 省略）。
3. **凭据**：复用 DeepSeek 的密钥来源（环境变量或 `config.toml`）；解析不到 →
   `WEB_PROVIDER_CREDENTIAL_MISSING`，消息里要写清「配哪一把」。
4. **打码**：新的密钥来源接进 `Redactor` 的收集 —— 今天它只认 `providers` 的值。
5. **错误映射**：HTTP / 传输 / 超时失败 → `WEB_PROVIDER_ERROR`，消息里带上解析出来的端点
   （端点配错是最常见的一种失败）。
6. **不新增密钥**：这是选它的首要理由 —— 本票不允许引入第二把第三方密钥。

## 验证

`cargo test` + `cargo clippy --all-targets` + `python3 scripts/check-language.py`：

1. **请求体**：对构造出的 JSON 断言（模型名、`max_tokens`、工具声明、user 文本、没有多余字段）。
2. **解析**：喂一份含 `web_search_tool_result` 的 fixture，断言 `SearchOutcome` 的 url / title /
   日期；再喂一份**没有那个块**的响应，断言 `WEB_PROVIDER_ERROR`。
3. **凭据缺失**：不注入密钥时返回 `WEB_PROVIDER_CREDENTIAL_MISSING`。
4. **打码**：把密钥值经一次调用参数带出去，断言流上的文本是 `[redacted]`。
5. **零网络**：所有自动化测试都不发真请求（fixture + 纯函数）。

**人工走查（落地后标 `ready-for-walkthrough`）**：

- 配好密钥、`[web] enabled = true`，在真会话里搜一次；
- **读这次调用的 `usage`，钉死「搜索结果算输入 token 还是输出 token」** —— 这是 spec §4 记下的
  不确定点（两者差 4 倍价），结果要回改 spec；
- 看一次真实搜索结果（来源条数、片段有无、日期有无），据此决定 §7 的格式要不要微调。

## 不做什么

- 不做抓取（票 03/04）、不写文档（票 05）。
- 不做多后端可配、不做自动降级、不做缓存。
- 不引入第二把密钥。
- 不动 provider 层「只做一个 OpenAI-compatible client」那条决定：这是**工具内部的 HTTP**，
  不是会话的 LLM 适配器。
