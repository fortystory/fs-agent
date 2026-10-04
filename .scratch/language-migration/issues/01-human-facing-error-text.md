# 最后一类：只给人看的错误与诊断文本翻成中文

Type: implement
Status: done

> 规格：`.scratch/language-migration/spec.md`（方法与验收）；决定：`docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md`。
> 这是语言迁移的**最后一批内容**。与前面各批的唯一差别：它**会动代码里的字符串字面量**，所以允许（并要求）跑 `cargo test`，且要同步改断言这些消息的测试。

## 目标

把**只给人看**的英文错误 / 诊断 / 输出文本翻成中文：它们不进事件流、模型永远看不到（启动期错误、`sessions replay` 的报错、渲染器诊断、probe 的报告行）。**同时一个都不许翻错**：凡进 `messages` 或进事件流的英文串必须原样 —— 这一批的难点全在分类。

> **注（2026-09-30）**：「一个都不许翻错」是当时的口令 —— [ADR 0005](../../../docs/adr/0005-model-visible-text-in-chinese.md) 把模型可见 / 进流那一侧**反过来**了（那一侧的散文现在也写中文）；本文件下面「不许翻（翻了就是回归）」那段之下的注是同一件事的完整记录。

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

> **注（2026-09-30）**：上面那条「不许翻（翻了就是回归）」的执行令现在是**历史记录** —— [ADR 0005](../../../docs/adr/0005-model-visible-text-in-chinese.md)（2026-09-30）把其中一大半接管并**反过来**了：`src/tools/*` 的工具声明与描述、`ToolOutput` / `ToolError::message`、`src/permissions.rs` 的 `reason`、`record_session_error` 的 `detail`、以及 `src/context*` / `src/discussion*` / `src/provider/projection.rs` 注入给模型的散文，现在都写中文（逐条见 ADR 0005 的「进度与收口」）。**当时把它列成「不许翻」是对的**：那一版的语言线（ADR 0004）画在「谁读它」上，本票只动只给人看的那一类，模型可见 / 进流的串一个都不许翻；ADR 0005 是从「词性」重画这条线，不是本票翻错了。
> 仍然不翻的：上游返回的原文（HTTP body / 厂商错误文本）、以及 `looks_like_quota` 的匹配词 —— 与 Comments 第 4 条刻意的分类一致。

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

## 评论

**实现完成（2026-09-27）**。提交 `76a7eb1`（17 个文件：10 个 `src/` + 7 个测试）。落点与逐条判断如下。

1. **`--remaining` 6,196 → 90 字符**，剩下的 90 一点都不该翻：`custom_tool_name` 的格式模板
   `{CUSTOM_TOOL_PREFIX}{namespace}{CUSTOM_TOOL_SEPARATOR}{tool}` 与一条内置 `base_url`。
2. **`--remaining` 漏了两条，本票一并翻了**：`src/config.rs` 里 `` `{name}` cannot be a debater's
   name … ``（句子里嵌着 `[轮 N · 名字]`）与 `` two debaters are both called `{}` … ``（嵌着
   `name = "甲"`）。两条都含中文示例，于是被它那条「含 CJK 即视为已翻」的判定放过了 ——
   按这个清单收尾时别只信这个脚本。
3. **比票面清单多翻的四类**（都是同一句里断不掉、或同族漏下的部分）：
   - `src/config/cost.rs` 的 `cap_text()`（`a cap of {limit} tokens` / `no cap`）—— 它嵌在那两句
     额度诊断里，不翻会出现「…，a cap of 1000 tokens」这种中英拼盘；
   - `src/agent/history.rs` 的另外三条 `Error::Undo` 消息（`cannot read {}: {error}`、
     `cannot restore {}: {error}`）—— 与票面点名的两条同族、同一条路径；
   - `src/cli.rs` probe 报告里的 `turn {}: no usage recorded` 与 `session: {} tokens, ${cost:.6}`
     —— 与票面点名的两行印在同一份报告里；
   - `src/render/tui.rs` 的两条 `expect("…")`（`a question is up`、`just checked`）—— 与既有的
     `expect("策略互斥锁已中毒")`（`src/lib.rs`）同类，是前几批漏下的。
4. **刻意不动的四类**（这批的难点全在这里）：
   - **前缀与字段名**：`fs-agent:`、`[budget] {reason}`、`[discussion] {reason}`、
     `config.toml: {source}`、`input=`/`output=`/`cached=`/`miss=`、probe 的
     `model {model_id} (provider {}, {})` 那一行 —— 它们是标签，不是句子；
   - **API 名与 TOML 键**：`readonly`/`ask`/`auto`、`[models.*]`、`max_tokens`、
     `looks_like_quota` 的匹配词、`probe_prompt()` 那两条发给模型的提示词；
   - **`undo edit_file on {}`**（`src/agent/history.rs`）：它是 `HistorySuperseded` 的 `summary`，
     **进事件流、要永久回放**；投影（`src/provider/projection.rs:221`）不读它，读它的是
     `transcript.rs` / `headless.rs` 这两个给人看的画家，而且都要过措辞层（`wording::history`）。
     所以按 ADR 0004「进事件流要永久回放的文本留英文」，它原样不动 —— 翻译该发生在措辞层，
     不在写入时。同理，`SessionError.detail`（`no debater answered this round` 等，住在
     `src/agent.rs`）本来就在冻结面里；
   - **上游返回的原文**（HTTP body / 厂商错误文本）—— 原样透传。
5. **`check-translation-batch.py --diff` 对这批不适用**：它只放行注释行与断言消息，而这一批动的
   正是字符串字面量，所以它必然报「动了 284 行非注释代码」。改用一条只看改动行的核对：把
   `git diff -U0` 的每一行过一遍，确认落点要么在字面量里（82 行）、要么是 `cargo fmt` 把多行
   调用收回一行（其余）。没有一行是逻辑改动。
6. **验收**：`cargo test` **757 passed / 0 failed**（条数未变）——13 处断言这些消息的片段同步
   改成中文：`tests/config_profiles.rs` 6、`tests/discussion.rs` 2，`tests/credentials.rs`、
   `tests/custom_tools.rs`、`tests/provider_adapter.rs`、`tests/session_store.rs`、
   `tests/e2e_single_turn.rs` 各 1（票面预告的 `tests/replay.rs` 与 `tests/observe*.rs` 实测
   没有断言这些文本，未改）。`cargo clippy --all-targets` 干净、`cargo fmt --check` 零漂移
   （翻完跑过一次 `cargo fmt`：中文更短，6 处折行被收回，已含在本提交里）、
   `scripts/check-language.py` OK（冻结面与那 12 条混住字面量都没被碰到）、
   `scripts/tui-startup-check.py` 12/12 GREEN。
7. **一条留给维护者的观察（不是本票的活）**：`crate::Error` 那四个前缀（`事件流 i/o 错误：` 等）
   其实印不到人前 —— `wording::error_report` 早已把四个变体各译了一遍；本次一并翻了，只是让
   「同一句话住几个地方」少一个分叉。若将来要把 `ConfigError` / `ProviderError` 这类错误文案
   也收进措辞层，那是一次独立决定（会牵动它们的 Display 与全部调用点），本票按票面要求就地翻。
