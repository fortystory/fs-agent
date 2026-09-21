# grilling：右侧信息面板的内容、数据源与刷新节奏

Type: grilling
Status: resolved
Blocked by: —

## Question

中右面板要显示什么、每个数从哪里来、什么时候刷新。范围已被用户收窄为**模型 / token / 上下文用量 / 轮次**（见 map Notes 第 1、2 条），本票把它们逐个落到**代码位置**上。

## 需要定

1. **字段 → 数据源**（每一条都要给出**文件与符号**，不能只说"有"）：
   - **模型名**：来自 `SessionConfig` 还是 provider profile？provider 换了以后要不要立刻更新？
   - **token 用量**：`UsageRecorded` 的累计口径 —— 输入/输出分开显示还是合并？缓存命中（如果有）算不算？
   - **预算**：来自哪里（配置里的 budget？），单位是什么（token？）——若当前没有"预算"这个值，如实说明并给出替代口径。
   - **上下文用量**：`context::usable_input` 的口径 —— 显示"已用 / 可用"还是百分比？分子是什么（system prompt + 历史消息 + 工具结果？），分母是什么？
   - **轮次**：单 agent 会话里"轮"是什么（`TurnEnded` 计数？`RoundStarted`？）；多 agent / 讨论模式下又是什么。选一个定义并说明。
2. **刷新节奏**。只在回合结束刷新，还是每个 `UsageRecorded` 都刷（流式期间会跳字吗）？忙碌状态放哪 —— header 里的 spinner、还是面板里的一行？**注意别把 tick 定成 1 秒全量重绘**（节拍归票 07）。
3. **没有数据时显示什么**。首回合之前、provider 不返回 usage、上下文用量不可得 —— 各显示什么（"--"？"未知"？整行隐藏？）。
4. **排版**。标签文案走 `src/render/wording.rs`（**要新增哪几条？逐条列出建议的中文标签**）；长模型名怎么截断（右侧截断加省略号？折行？）；数值列右对齐还是左对齐。
5. **时钟**。时间日期在 header —— 粒度到分钟还是秒？格式（本地时区？`YYYY-MM-DD HH:MM`？）；刷新它需要 tick，定下来由谁触发（与票 07 对齐）。

## 产出必须包含

- 一张「字段 × 数据源（文件:符号）× 无数据时文案」的表。
- 需要新增的 `wording.rs` 条目清单（中文标签），供票 09 与 `/to-spec` 直接引用。

答案必须自足（`/implement` 在 `/clear` 后读它）。

## Answer

**已定（2026-09-21，grilling 逐轮与用户确认）。本票只产决策，不含实现。**

### 1. 数据怎么进渲染器：静态注入 + 动态走流

**注入一个结构化值**（暂名 `SessionFacts`；`/to-spec` 定名时照 `CONTEXT.md` 的词汇核对一次）。`TuiOptions`（`src/render/tui.rs:128`，**当前只有 `port` 一个字段**）加一个字段，构造点只有一个：`src/cli.rs:299` 的 `Renderer::tui(TuiOptions { port })`。**这些值在组装期全部已知**，不需要第二个注入点：

| 字段 | 来源 |
| --- | --- |
| 会话 id | `stored.id`（`src/cli.rs:244` 处创建，`:315` 已这样用） |
| cwd | `stored.dir` |
| 模型 | `model`（`src/cli.rs` 组装期变量） |
| 上下文窗口 | `context::usable_input(&caps)` = 窗口 − 输出预留（`src/context.rs:102`）；`caps` 由 `provider::capability::caps_for(&model)` 解析（`src/provider/capability.rs:91`） |
| 预算上限 | `session_config.budget.limit`，`Option<u64>`（`src/config/cost.rs:46-53`） |

**模式不进注入 —— 它是唯一会中途变化的字段，而两条转换路径都已经在流上**：

- 进入计划模式 → `ContextInjected { source: ContextSource::PlanMode }`（`src/lib.rs:603-607` → `src/render/transcript.rs:360-362` → 措辞 `src/render/wording.rs:324`「计划模式」）
- 离开 → `HistorySuperseded { reason: HistoryReason::ModeChange }`（`src/agent.rs:278`）

理由：交互式会话固定以 `Policy::for_mode(Mode::Ask)` 组装（`src/cli.rs:325`），注入的初值恒为「询问」，Shift+Tab 一按就过期；再开一条 loop→渲染器的「设置模式」通道，等于把流上已有的事实抄一份。

### 2. 字段 × 数据源 × 无数据文案

| 面板字段 | 来源 | 无数据时 |
| --- | --- | --- |
| 模型 / cwd / 上下文分母 | 注入（见 §1） | 不会缺 |
| **上下文分子** | **最近一条** `UsageRecorded.usage.input_tokens`（`src/events.rs:161-168`） | `—` |
| token 已用 | 累加全部 `UsageRecorded`，取 `Usage::total_tokens()` = input + output（`src/events.rs:176`） | `0` |
| token 上限 | 注入 `budget_limit`；`None` → 只显示已用，**不显示** `/ —` | 整段不显示 |
| 输入 / 输出 | 同一累加器分列 | `0 / 0` |
| 缓存明细 | 同一累加器的 `cached_tokens` / `miss_tokens` | `0 / 0` |
| 回合 | 数 `EventPayload::TurnEnded`（`src/events.rs:330`） | `0` |
| 时钟 | `chrono::Local`（已是依赖），**粒度分钟**，格式 `YYYY-MM-DD HH:MM` | 不会缺 |

**两条口径纪律**：

1. **累加口径必须与 `events::total_usage`（`src/events.rs:807`）逐字一致**：`cached_tokens` / `miss_tokens` 是 `input_tokens` 的**拆分**，加进合计就是把同一段 prompt 数两遍；`reasoning_tokens` 已含在 `output_tokens` 内（`src/events.rs:171-178` 的注释就是为此写的）。渲染器自己累加时不许换算法。
2. **分子用 provider 实测值，不用估算**。`estimate_tokens` 是 `chars / 4`（`src/context.rs:109`），对中文与代码偏差数十个百分点；面板显示它会把「快满了」报错。

### 3. 花费不显示（明确否决）

用户在本轮明确没选它。`Pricing` / `PriceTable`（`src/config/cost.rs:145`、`:194`）只服务 `fs-agent cost` 一类结算路径（`src/cli.rs:760-767`）。日后若要加，**必须**遵守既有的「未登记 ≠ 0」规则（`src/config/cost.rs:188-193`）：没有 `[pricing.<model>]` 条目就整项不显示，而不是显示 `$0`。

### 4. 刷新节奏

- 面板是 `(SessionFacts, Counters, Mode)` 的**纯函数**，渲染时算，不缓存字符串。
- `UsageRecorded` / `TurnEnded` 到达 → 更新计数器；**何时真绘由票 07 定**（本票只要求"更新计数器"，不要求"每事件一绘"）。
- 忙碌态（`wording::status_word`，`src/render/wording.rs:357`）**不在面板里重复**；它的落位（含 `status_line` 的去留）归票 06。
- 时钟由既有 `TICK`（`src/render/tui.rs`）驱动，**不新增秒级 tick**。

### 5. 术语：标签是「回合」，不是「轮次」

`CONTEXT.md` 明确区分 **轮次（Round，`:99`）** 与 **回合（Turn，`:103`）**，而 `TurnEnded` 数的是**回合**。面板标签用**「回合」**。讨论模式下流上另有显式编号 `RoundStarted { round }`（`src/events.rs:292`）——是否同时显示「轮」**是排版问题，归票 02 按栏宽定**；字段语义（数 `TurnEnded`）在此固定。

### 6. 需要新增的 `wording.rs` 条目（提议清单）

```rust
pub const PANEL_MODEL: &str = "模型";
pub const PANEL_CONTEXT: &str = "上下文";
pub const PANEL_TOKENS: &str = "token";   // 与既有 stats 文案一致，wording.rs:726 已用英文 token
pub const PANEL_TURNS: &str = "回合";
pub const PANEL_INPUT: &str = "输入";
pub const PANEL_OUTPUT: &str = "输出";
pub const PANEL_CACHE: &str = "缓存";
pub const PANEL_UNKNOWN: &str = "—";

pub fn token_pair(used: u64, limit: Option<u64>) -> String;    // "12,345" / "12,345 / 100,000"
pub fn context_pair(used: Option<u64>, usable: u64) -> String; // "—" / "12,345 / 200,000（6%）"
pub fn cache_pair(cached: u64, miss: u64) -> String;           // "9,000 / 3,345"
```

- 千分位用**半角逗号**，全面板一致。
- 这些是**渲染层文案**，不在 ADR 0001 的冻结清单上（冻结的是模型可见文本），新增安全。
- 数值右对齐；长模型名用既有 `truncate_columns`（`src/render/tui.rs`）右侧截断加 `…`。

### 7. 栏内降级顺序

四个核心字段（模型 / token / 上下文 / 回合）**永不栏内丢弃** —— 放不下就该走票 02 的「整栏隐藏」。宽度不足时先丢**细节行**，顺序：**缓存明细 → 输入/输出 → 上下文百分比**（保留 `12,345 / 200,000` 的数值本身）。
