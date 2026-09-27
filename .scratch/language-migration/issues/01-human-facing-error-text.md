# 最后一类：只给人看的错误与诊断文本翻成中文

Type: implement
Status: ready-for-agent

> 规格：`.scratch/language-migration/spec.md`（方法与验收）；决定：`docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md`。
> 这是语言迁移的**最后一批内容**。与前面各批的唯一差别：它**会动代码里的字符串字面量**，所以允许（并要求）跑 `cargo test`，且要同步改断言这些消息的测试。

## 目标

把**只给人看**的英文错误 / 诊断 / 输出文本翻成中文：它们不进事件流、模型永远看不到（启动期错误、`sessions replay` 的报错、渲染器诊断、probe 的报告行）。**同时一个都不许翻错**：凡进 `messages` 或进事件流的英文串必须原样 —— 这一批的难点全在分类。

## 落点与逐条清单

用 `python3 scripts/check-translation-batch.py --remaining` 现查（它已排除冻结面与断言消息）。当前清单是 **6,196 字符 / 10 个文件**：

| 文件 | 字符 | 是什么 | 判断 |
| --- | --- | --- | --- |
| `src/config.rs` | 2,712 | `ConfigError` 的 15 个 `#[error("…")]` + `resolve_*` 里拼 reason 的句子（debaters/name/soul/max_rounds/tool 声明/budget margin/mode） | **翻**：启动期打印给人看，不进流也不进模型 |
| `src/provider/openai.rs` | 1,269 | 组装期错误（`has no API key: …`、`could not build the HTTP client: …`）、key 与 base_url 的提示句、能力告警（`model … does not support tools; dropped …`、`fixes temperature; dropped …`）、SSE/protocol 解析错误 | **翻**：`stderr_warnings()` 打到 stderr；`ProviderError` 的 Display 只走 `render.diagnostic`（见 `src/agent.rs:397`）。**但上游返回的原文**（HTTP body / 厂商错误文本）与 `looks_like_quota` 的匹配词（`"insufficient"`、`欠费`…）原样 |
| `src/lib.rs` | 995 | `Error` 枚举 4 条；`validate_roster` 的 7 条 `Error::Discussion(…)`；`resumed session: closed N interrupted tool call(s)…` 那条诊断 | **翻**：全是 CLI / 诊断侧 |
| `src/agent/replay.rs` | 467 | `sessions replay` 的 7 条报错（`this session has no round {0} to replay` 等） | **翻**：CLI 输出 |
| `src/cli.rs` | 224 | probe 的报告行（`fs-agent: skipping {model}: …`、`no API key (export {hint})`、`turn {}: input=… output=…`、`session: … tokens (no [pricing.…] entry, so no cost)`） | **翻**：probe 打印给人看。**别碰** probe 的提示词（`The quick brown fox…`、`Ignore the filler below…`）—— 那是发给模型的，护栏正面清单里钉着 |
| `src/agent/history.rs` | 136 | `cannot read the snapshot for {}: {error}`、`cannot undo the edit to {}: {error}` | **翻**。**别碰** `INTERRUPTED` 那条合成工具结果（模型可见，护栏钉着） |
| `src/config/cost.rs` | 132 | `exhausted_note` / `estimate_refusal_note` 两句话 | **翻**：它们只经 `render.diagnostic` |
| `src/provider/capability.rs` | 130 | `unknown model \`{}\`: it is not in the capability table…` | **翻**：启动期错误 |
| `src/render/tui.rs` | 76 | 剩下的那条给人看的消息（非模型可见的那条） | **翻**，但 `a questionnaire needs at least one question` 是模型可读的（护栏钉着），别动 |
| `src/session/store.rs` | 55 | `could not allocate a unique session id after 8 attempts` | **翻** |

**不许翻的（翻了就是回归）**：`src/tools/*` 的一切字符串（工具声明 `description`/`parameters`、`ToolOutput`、`ToolError::message`，含 `edit.rs` 的 `EditError` 全部消息）；`src/permissions.rs` 的 `reason: "mode readonly: …"`；`src/agent.rs` 的 `HOOK_STOPPED_TURN` / `CANCELLED_BEFORE_RUN` / `CANCELLED_IN_FLIGHT` / `BUDGET_NO_NEW_EXECUTOR`、`record_session_error` 的 detail、`agent_identity()`；`src/context*`、`src/discussion*`、`src/provider/projection.rs` 里注入给模型的文本与协议标记；`src/render/input.rs` 的三条问卷端口错误。这批串的清单在 `python3 scripts/check-language.py --list` 里逐条列着。

## 测试同步

翻完全局跑 `cargo test`，把断言这些消息的测试改成中文的那个词。已知要改的（至少）：

- `tests/config_profiles.rs`：`contains("at least two")`、`contains("one word")`、`contains("longer than")`、`contains("empty \`soul\`")`、`contains("both called \`kimi-k3\`")` → 改成对应中文；只断言标识符 / 表名 / model id 的（`"mystery"`、`"base_uri"`、`"max_rounds"`、`"estimate_margin"`、`"cached_input"`、`"api.deepseek.com"`）保持原样。
- 其它文件里断言这些消息的（`tests/provider_adapter.rs` 的告警、`tests/replay.rs` 的报错、`tests/observe*.rs`）一并改。

## 验收

- `python3 scripts/check-language.py` **必须 OK**。若它报「冻结面里出现了中文串」或「模型可见 / 进流的字面量被翻成了中文」，说明翻错了地方 —— **改回去，不许动脚本里的白名单**。
- `cargo test`：**757 passed / 0 failed**（条数不能变）。
- `cargo clippy --all-targets` 干净；`cargo fmt --check` 零漂移（翻完跑一次 `cargo fmt`）。
- 提交：一次 `git commit`（中文信息，例如 `feat(cli,config,provider): 只给人看的错误与诊断文本翻成中文`），逐文件 `git add`，别 `-A`；**不要新建文件**（临时脚本写 `/tmp/`）。

## 不做什么

- 不动模型可见 / 进流的那一侧（ADR 0001 的冻结面，见上）。
- 不动 `docs/research/`。
- 不往 `scripts/check-language.py` 的白名单里加东西。
