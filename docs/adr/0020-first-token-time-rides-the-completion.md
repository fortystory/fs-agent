# 0020. 首 token 的时刻搭在它那次调用的完成事件上

- 状态：接受（2026-10-09）
- 背景：[`.scratch/trace-ledger/issues/06-grilling-timeline.md`](../../.scratch/trace-ledger/issues/06-grilling-timeline.md)（时间轴与计时）；事实底账在 [`.scratch/trace-ledger/research/01-data-surface.md`](../../.scratch/trace-ledger/research/01-data-surface.md) §2。

## 决定

`MessageCompleted` 加一个可选字段 `first_token_ms: Option<u64>` —— **首个增量 token 距本次调用开始的毫秒数**。「首个」取 `TextDelta` 与 `ReasoningDelta` 先到者，因为屏幕上第一次动就是等待结束。

- 产出点在主循环与合成器两处（`src/agent.rs`）；**用户消息那一条恒 `None`**。
- `SCHEMA_VERSION` 跟着涨一档：它只是记号（`src/` 里没有按版本分派的读取逻辑），兼容性完全由 serde 形状决定。
- 可选的形状给出双向兼容：老流缺这个字段读成 `None`；老二进制读新流忽略未知字段。

## 为什么

1. **没有它，时间轴的模型泳道只能整段说「在等模型」。** 一格不足一秒的分辨率下，屏幕上唯一切得开的等待就是它 —— 整个模型调用是一个黑盒，而读者要问的恰恰是「这 7 秒里多少是在等第一个字」。
2. **一个字段推出三个数**：TTFT = 它；生成时长 = `MessageCompleted.at − (TurnStarted.at + 它)`；吞吐 = `output_tokens / 生成时长`。三处都只用现成字段。
3. **事件流是真相源**，只在渲染层记会让同一条流出现两种读数。
4. **首 token 的口径是「首个增量」，不是「首个正文增量」**：推理生成本来也是生成，而那个数要回答的问题是「等了多久才看见动静」。

## 代价

1. **事件 schema 动了** —— 这次 effort 里唯一的结构变化（模型可见文本仍然零变化：投影不读这个字段）。
2. **老流读不到**：时间轴的模型泳道整段画，检视器**不列** TTFT / 生成 / 吞吐那三行（不写「未知」占位符）。
3. **provider 侧要在首个增量到达时读一次时钟**，三处产点各一次。

## 被否决的替代方案

- **只在渲染层记**（`event_at(Delta)` 那个 `Utc::now()`）。**否决**：重放路径（`--continue`、宽度变化重排）走不到增量事件，同一条流「实时有、重开没有」—— 屏幕上会出现「有的会话有、有的没有」。
- **新增一条事件变体**（比如 `FirstTokenSeen`）。**否决**：新变体是**单向不兼容**，老二进制 `read_events` 遇到未知变体会拒读整条流（`--continue`、`sessions show`、`sessions replay` 全挂），比加一个可选字段贵得多。
- **拿调用总时长当「生成时长」的分母**。**否决**：那是把等首 token 的时间算进生成，是编造。
