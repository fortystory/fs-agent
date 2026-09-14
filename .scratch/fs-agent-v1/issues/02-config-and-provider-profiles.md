# 02: 配置与两个真 provider profile

**What to build:** 让 `fs-agent` 真的连上 KIMI 与 DeepSeek 两家，并把两家的差异在适配器里抹平——用户配好自己的 key 之后，一次回合能对两家各跑通，事件流里的用量、缓存计量与错误分类在两家之间**形状一致**。

Blocked by: 01 · 骨架与唯一接缝

Status: done

**参考:** spec §4（provider 与能力表）、§17（缓存计量）

- [x] 配置两级：`[providers.*]`（`base_url` + key）→ `[models.*]`（引用某个 provider + 覆盖）
- [x] 优先级 `config.toml` > 已导出 env > 内置默认；项目 `.env` **不**被自动加载
- [x] `base_url` 必须与 key 来源同域（Kimi 跨域混用返回 401 时给出可读诊断）
- [x] 能力表按 model id 建、只建模两家、`#[non_exhaustive]`、**未登记即报错**（不静默降级）；显式传入但被判定不支持而丢弃的参数**产生告警**
- [x] 两家的 `tool_call` 分片（含 `index`）在适配器内拼装完成，上层看不到分片
- [x] usage 形状归一：`cached` / `miss` 是事件里的一等字段（Kimi `cached_tokens`；DeepSeek `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`）
- [x] `reasoning_content` 建模并在带 tools 的请求里回放（DeepSeek 不回传自己的推理直接 400）
- [x] `ProviderError` 六分类，其中 `QuotaExhausted` 与 `RateLimited` **分开**（Kimi 429 / DeepSeek 402）；传输级有界重试归适配器，上层不重跑
- [x] `prompt_cache_key` = 会话 id；不发 `user_id`；Kimi 的推理档位在会话开始前定死（中途切档会废掉前缀缓存）
- [x] 手工验收：用自己的 key 对两家各跑通一次真实回合；第二次请求的 usage 显示缓存命中

## Comments

实现完成（agent）。落点：

- `src/config.rs` — `[providers.*]` / `[models.*]` 两级，字段级优先级 `config.toml` > env > 内置；纯函数 `resolve(file_text, env)`，库入口不读环境，`.env` 无从被加载；跨域 key/base_url 在解析期即报错（诊断里写明 401 与期望 host）。
- `src/provider/capability.rs` — 按 model id 建的 `#[non_exhaustive]` 能力表（Kimi Open Platform `kimi-k3`；Kimi Code `k3` / `k3-256k` / `kimi-for-coding` / `kimi-for-coding-highspeed`；DeepSeek `deepseek-flash` / `deepseek-v4-pro`），未登记即 `UnknownModel`。
- `src/provider/openai.rs` — 一个 OpenAI-compatible client：`build_body` 按能力表过滤参数并产出告警、`StreamDecoder` 拼装两家 `tool_call` 分片（含 `index`）并把 `[DONE]` 作为唯一结束信号、`normalize_usage` 归一 `cached`/`miss`、`classify_status` 六分类（429 与 403 靠 body 分开欠费/限速/鉴权）、有界重试只覆盖传输与限速。
- `src/cli.rs` — 组装 provider；新增 `fs-agent probe`：对每个有 key 的模型跑同一会话的两个真实回合并打印 input/output/cached/miss，即本票的手工验收工具。
- 测试：`tests/config_profiles.rs`（19）、`tests/provider_adapter.rs`（30）、`tests/e2e_single_turn.rs` 增补推理档位钉死用例；`cargo test --offline` 64 passed，`cargo clippy --all-targets -D warnings` 干净。
- 两轴 review（standards / spec）后修掉：跨域守卫改由 **key 来源**决定而非 section 名（`[providers.kimi] api_key_env = "DEEPSEEK_API_KEY"` 现在会被拒）；`kimi-k3` 的 `max_output_tokens` 不再大于 `context_window`（并加了一条全表自洽断言）；未登记模型在 CLI **启动时**整表校验报错（不再等到某次 provider 构造）；`StreamDecoder` 的文档不再自称「纯函数」（它是单次响应的传输态，不是会话状态）；`key_hint` 与 `resolve_key` 的环境变量候选表合并为 `ProviderProfile.key_env`。

**手工验收（通过）**：两个 vendor 都用真实 key 跑通并观测到缓存命中。

DeepSeek（`deepseek-*`）：

| 模型 | turn 1 | turn 2 |
| --- | --- | --- |
| `deepseek-flash` | input=476 cached=0 miss=476 | input=486 **cached=256** miss=230 |
| `deepseek-v4-pro` | input=529 cached=0 miss=529 | input=539 **cached=512** miss=27 |

Kimi（Kimi Code / coding plan，`kimi-code` / `k3-256k`）：

| 模型 | turn 1 | turn 2 |
| --- | --- | --- |
| `k3-256k` | input=532 cached=0 miss=532 | input=594 **cached=512** miss=82 |

这同时验证了：usage 归一化读对了 DeepSeek 的 `prompt_cache_hit_tokens`/`prompt_cache_miss_tokens` 与 Kimi 的 `cached_tokens`；`prompt_cache_key` 发给 Kimi、不发给 DeepSeek 都正确；缓存计量随 turn 增长。

**中途的一段弯路（值得记录）**：Kimi 首次验收 401，当时的结论「key 无效」是**错的**。真正原因是 **Kimi 的两套系统**——Kimi Code（coding plan，`sk-kimi-` key，`https://api.kimi.com/coding/v1`）与 Kimi Open Platform（`platform.kimi.com`，`https://api.moonshot.cn/v1`）不是同一套凭据，而当时把 coding key 指到了 Open Platform 的 host。修正后 Kimi 一次通过。据此补的实现：

- `api.kimi.com` 加入 Kimi 的允许 host；`kimi-code` 作为**独立内置 provider**（key env `KIMI_API_KEY`），与 `kimi`（`MOONSHOT_API_KEY`）并存，避免「把 `kimi` 指向 coding 端点会顺带废掉 `kimi-k3`」。
- 能力表补 4 个 coding 模型 id：`k3`（1M）、`k3-256k`（262144）、`kimi-for-coding`（K2.8 Preview，1M）、`kimi-for-coding-highspeed`（262144，thinking-on 无档位）。
- `classify_status` 对 403 按 body 细分：Kimi Code 的额度窗口（5 小时 / 每周 / 每月）→ `QuotaExhausted`，并发上限 → `RateLimited`，其余 403 才是 `Auth`（此前 403 一律算 Auth，会把「额度用尽」误报成鉴权失败）。
- 401 的诊断改为按 vendor 给可执行提示，Kimi 的那条直接写明两套系统的 base_url 与 key 前缀。

首轮 probe 还曾给出假阴性（`cached=0`），根因是 probe 自身：prompt 只有 35 token（低于 Kimi 的 >256 缓存门槛）、且两回合之间不等待（DeepSeek 明说 cache 构建要数秒）。已修：首轮 prompt 填充到 ~500 token，两回合之间等 10s。跑法：

```
cargo run --release -- probe            # 探测所有有 key 的模型
cargo run --release -- probe --model k3-256k
```

若失败，`probe` 会按 provider 打印可读诊断（缺 key / 401 跨域或档位不足 / 402 会员状态 / 403 额度 / 429 限速）。



