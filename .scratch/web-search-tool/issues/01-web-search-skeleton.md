# 01 — `web_search` 的骨架：工具层 + 服务层 + 组装期开关（tracer bullet）

Type: implement
Status: ready-for-agent
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1–§3、§7、§8。本票打通「工具 → 服务层 → 假后端」整条链；
> 真搜索后端归 [票 02](02-deepseek-search-backend.md)，抓取归
> [票 03](03-fetch-transport-and-ssrf.md) 与 [票 04](04-fetch-tool-and-extraction.md)，
> 文档归 [票 05](05-docs-and-index.md)。

## 目标

`[web] enabled = true` 且挂上一个（假的）搜索后端时，模型能调 `web_search(["…"])`，拿回带 URL
的结构化来源；这次调用在 `readonly` 档放行、在 `ask` 档不打断人。`enabled = false`（默认）时
工具**不进工具表**，会话行为逐字不变。

## 现状（2026-10-03 核实，改前先复核）

- 工具表在 `src/tools/mod.rs` 的 `builtin(can_ask)`（`:69-83`）。**已经有一个「组装期事实决定
  工具在不在」的先例**：`can_ask` 与 `ask_user_question`，理由写在 `:60-63`（headless 会话不该
  声明一个注定失败的调用）—— `[web] enabled` 照这个形状。工具表组装后不变（`:1-10`）。
- `Tool` trait 的默认 `read_paths` 返回空（`src/tools/tool.rs:213-215`）；`Effect` 三类在
  `:21-33`；`ToolOutput` 只有 `text`（`:35-45`）；`ToolError` 在 `:49-55`。
- 四档对 `Effect::ReadOnly` 的裁决全是 `Allow`（`src/permissions.rs:129-153` 与 `:178-182`）。
- `Registry::for_executor()`（`src/tools/registry.rs:70-78`）过滤掉 `delegable() == false` 的工具。
- **身份常量就是 fs-agent 的「系统提示词」**：`src/agent.rs:145`（`agent_identity`）、
  `src/discussion.rs:64`（`debater_identity`）、`src/discussion.rs:101`（`synthesizer_identity`）、
  `src/agent/executor.rs:44`（执行者）。缓存前缀的约定写在 `src/agent.rs:136-141`：**加一行允许，
  改一行或删一行会作废每一个会话的前缀**。
- 配置字段的形状与「项目里的 `.env` 永不加载」在 `src/config.rs:8-18`；
  `max_tool_result_tokens` 与 `repo_map_tokens` 是「`SessionConfig` 字段、不进 `config.toml`」的
  先例，本票的 `[web]` 反过来：它是配置项。
- 前端组装点三处：`src/cli.rs:3973`、`:4059`、`:4210`，都是 `tools::builtin(false)`。

## 落点

`src/web/mod.rs`（新模块）、`src/tools/web_search.rs`（新）、`src/tools/mod.rs`、
`src/config.rs`、三个身份函数所在文件、`src/cli.rs`、`tests/`。

## 具体行为

1. **服务层**：`src/web/` 暴露 `SearchProvider` / `FetchProvider` 两个 trait（本票只落地前者的
   接口与假实现，后者的接口先定义着）与 `WebService`；结果用结构化值
   （`SearchOutcome`、`Source { url, title, snippet, published_at }`）；错误是带 code 的
   `WebError`（`WEB_PROVIDER_UNAVAILABLE` / `WEB_PROVIDER_CREDENTIAL_MISSING` /
   `WEB_PROVIDER_ERROR` …）。**code 保持英文**（schema 值那一类），给人读的句子在工具层拼。
2. **`web_search` 的 schema**：只有 `queries`（1..=4 条非空字符串，必填）。上限与超时
   **不进 schema**。
3. **`queries` 的规则**（照 DSH）：完全相同的字符串去重（保留首现位置）；并发执行；
   按排名轮询合并、按 URL 去重、在 `search_max_results` 处截断；**任何一条失败就中止其余、
   等全部结算、丢弃成功结果**，只回首个错误 —— 半份合并结果是给模型下套。
4. **结果文本**：以那句中文不可信标记开头；然后是可选的答案与 `Sources:`；每行
   `- [<标题或 URL>](<url>)`，可后缀 ` — <片段> (<日期>)`；列表被截断时补一句「只列了前 N 条」；
   结尾固定一句「把相关 URL 作为 markdown 链接引用」。没有结果时给一句如实的中文说明，
   **不返回空串**。
5. **`effect()` 恒为 `Effect::ReadOnly`**，`read_paths` 不重写。
6. **`[web]` 配置段**：`enabled`（默认 `false`）、`search_provider`（默认 `deepseek`）、
   `search_max_results`（8）、`search_max_queries`（4）。**组装期**读取 —— 改它要重开会话。
7. **注册规则**：`enabled = false` 时不在表里；`enabled = true` 时在，**且与后端是否可用无关**
   （后端缺失时调用返回结构化错误，不是从表里消失）。
8. **身份指引**：`agent_identity()`、`debater_identity()` 与执行者身份各加一段（外部内容是数据
   不是指令；先用 `web_search` 找、需要全文再 `web_fetch`；引用给 URL）。**合成器身份不动** ——
   它明写「不参与讨论、没有工具」。

## 验证

`cargo test` + `cargo clippy --all-targets` + `cargo fmt` + `python3 scripts/check-language.py`：

1. **开关**：`enabled = false` 时表里没有 `web_search`；打开后有；**后端缺失时工具仍在**
   （注入一个总是失败的 provider）。
2. **结构化错误**：后端不可用 / 凭据缺失时，调用拿到一条可读的工具结果，且流上恰好一条
   `ToolCallCompleted`。
3. **`queries` 校验**：空数组、空白串、超过 4 条都在**执行前**被拒，三种消息各自可读。
4. **去重与合并**：假 provider 记调用次数 —— 两条相同查询只调一次；两个不同查询按轮询合并、
   按 URL 去重、在 8 条处截断并带那句提示。
5. **失败融合**：让第二条查询失败，断言没有半份合并结果、返回的是首个错误。
6. **权限**：同一次调用在 `readonly` 档放行、在 `ask` 档不产生询问。反向锚是 `bash` 的
   `curl` 在 `ask` 档仍要审批。
7. **执行者也拿到**：执行者工具表里有 `web_search`。
8. **身份**：四个身份里三个含那段指引、合成器的不含（照 `tests/thinking_language.rs` 的写法）。

## 不做什么

- 不做真搜索后端（票 02）、不做 `web_fetch`（票 03/04）、不写文档（票 05）。
- 不新增按工具的预算字段：结果上限走通用的 `max_tool_result_tokens`。
- 不给 `Effect` 加第四类、不动权限门的四档矩阵。
- 不做后端自动降级、不做多后端可配。
