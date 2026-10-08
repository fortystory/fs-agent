# 02 — 会话中途换模型与档位：`Session` 与 `Harness` 的换挡口

Type: implement
Status: done
Part of: ../spec.md
Blocked by: 01

给 `Session` 与 `Harness` 加两个方法，让会话中途能换 provider、`SessionConfig` 与注入事实。
这一票不碰渲染器 —— 它是后面三张票共同的那个落点。

规格见 [`spec.md` §2、§3、§4、§13](../spec.md)。

## 要做什么

### `Session::retarget`

`src/session/mod.rs` 的 `config` 字段是私有的。加：

```rust
pub fn retarget(&mut self, config: SessionConfig)
```

只换那一个字段，其余（流、工具表、策略、读集、取消信号）原样。**只有 `Harness` 调它**。

### `Harness::switch_model`

`src/lib.rs`。`Harness` 持有 `provider: Arc<dyn Provider>` 与 `session: Session`，而这两个
**必须同时变** —— 否则下一次请求会拿新模型 id 去问旧 caps（`build_body` 收的是 `provider`
建好时存的 `caps`）。所以一个动作做完：

```rust
pub fn switch_model(&mut self, provider: Arc<dyn Provider>, config: SessionConfig) -> Switched
```

`Switched` 是循环推进 `SessionUpdate` 要的那些值：`model`、`effort`、`context_window`
（`usable_input(&caps_for(&model))`）、`speakers`（新 profile 名，见 §4）。

同时换的还有 `self.speaker`：它是 `SpeakerId::Debater(profile.name)`，而**发言者名是 provider
profile 的名字**，所以换到另一个 profile 时名字跟着变。`SpeakerColors` 已经为这件事备好了路
（`tui.rs` 里 `SpeakerColors::extra` / `free_slots`，`/discuss` 中途加入的讨论者就走它），
所以渲染器那边不用改逻辑，只需要名册更新（§3 的 `SessionUpdate` 带上 `speakers`）。

`effort` 只改 `SessionConfig::params.reasoning_effort`，不动 provider（`OpenAiProvider` 不读
它 —— 它只从 `ChatRequest.params` 读，见 `build_body`）。所以**换档是一个纯配置动作**，
不需要重建 provider —— 这条要在代码注释里说清，否则后来的人会以为两个入口是同一个。

### 调用点在 `cli.rs`

`interactive` 那条路已经解析出 `config` 与 `OpenAiProvider::build`。把
「组装一个模型 → 拿到 provider + SessionConfig + 新事实」收成一个循环侧的函数，四个入口共用：

```rust
fn retarget_to(config: &Config, model: &str, effort: Option<ReasoningEffort>, warnings: …)
    -> Result<(Arc<dyn Provider>, SessionConfig, Switched), String>
```

它做的事：`OpenAiProvider::build(config, model, …)` → `config.session_config(model)` →
把 `effort` 灌进 `session_config.params` → `caps_for` 算 `usable_input` → 读 profile 名。

**保留**：`session_config` 里从配置读出来的 `budget`、`redactor`、`carried_tokens` 全部照
`Config::session_config` 给的来（换模型后预算按新模型的配置走，这是对的）。**丢弃**：
`executor_model` / `synthesizer_model` 两个覆盖 —— 见 spec「明确不做」，它们是组装期决定的
配置事实，而 `Config::session_config` 会把它们填上，切模型时不该跟着上一场的模型走。

### 回执

切换成功后 `harness.notice(...)`，文案带上 spec §13 那半句「前缀缓存重来」。

## 测试

`tests/` 下：

- `Session::retarget` 换完之后 `config()` 是新配置，而 `id()` / `cwd()` / `log_path()` /
  `tools()` 都不变。
- `Harness::switch_model` 换完之后：下一次 `run_turn` 的请求 `model` 是新 id、`params` 是新档位，
  并且 `build_body` 按**新模型的** caps 处理（比如换成 `MiniMax-M3` 之后 `reasoning_split` 仍在、
  `max_tokens_field` 仍按它来）。这条最要紧 —— 它钉住「两样必须同时变」。
- `Switched` 里的 `context_window` 等于新模型的 `usable_input`；`speakers` 是新 profile 的名。

怎么观察「下一次请求」：`tests/provider_adapter.rs` 里已经有构造 `ChatRequest` 打
`build_body` 的路子，沿用它，或者用一个假 provider 记下收到的 `ChatRequest`。

## 评论

2026-10-08 实现完毕，`Status: done`。

**一处签名上的偏差**：`Harness::switch_model(&mut self, provider, config, profile_name: &str)`。
票上写的是两个参数，而 `Switched::speakers`（新发言者的名字）**从 provider 与 config 里算不出来**
—— 名字是 provider profile 的名字，而 `Provider` trait 上只有 `caps()`。所以 profile 名由调用方
（循环，读得到配置的那一侧）给出，`switch_model` 据此换 `self.speaker` 并填 `speakers`。

**一处与票字面不同的取值**：`carried_tokens` **从当前会话继承**（`harness.carried_tokens()`），
不是照 `Config::session_config` 给的 0。累计额度是**会话**事实 —— 目标循环翻过页的会话已经花掉的
token 换模型不该被抹掉（`.scratch/goal-loop/spec.md` §8「翻页不重置额度」是同一条纪律）；预算与
打码器仍然照 `session_config` 给的来（那两条按新模型的配置走是对的）。

`retarget_to` 先问 `caps_for` 再 `OpenAiProvider::build`：没登记的 model id 那条错误把已知 id 列全了，
是 `/model <未知 id>` 最有用的一句。

测试：`tests/switch_model.rs`（两条换挡口 + 「两样必须同时变」）与 `cli.rs` 的 `mod tests`
（`retarget_to` 三条：重建 + 带住累计、换模型不认的档位落回默认、组装不成的错误点名环境变量）。
