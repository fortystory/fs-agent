//! 成本与会话的花费上限（spec §17）。
//!
//! # 两笔预算，永远别混
//!
//! [`crate::context::usable_input`] 是**窗口**：一次调用最多能带多少输入，按 agent 各自的
//! 模型算出，靠丢弃最旧的可丢材料来满足。[`Budget`] 是**会话的累计额度**：讨论者、合成器
//! 以及它们派发的每一个执行者共用的一个硬停，靠降级收尾来满足。
//!
//! # 钱只作显示
//!
//! 闸门数的是 **token**。[`Pricing`] 存在的意义是让人看见这些 token 值多少钱，它从不决定
//! 任何事（spec §17）。价格按每百万 token 报 —— 两家供应商都这么公布 —— 而 `cached` 与
//! `miss` 分开计价，因为前缀缓存命中通常比未命中便宜一个数量级。
//!
//! 这里的一切都是从配置解析出来的**值**，从不是状态：拿 [`Budget`] 去比的花费是从事件流
//! 求和出来的（[`crate::events::total_usage`]），所以没有锁看着它，`--continue` 也丢不掉
//! 它（spec §10）。

use std::collections::BTreeMap;

use crate::events::Usage;

/// 缺省的预检宽容度（spec §17）。
///
/// 只有估计的体积超过 `remaining * margin` 时才会拒掉一次调用。估计值取 `chars / 4`，
/// 在代码上、在非拉丁文本上都会差出几十个百分点；把它直接和剩下的量比（`margin = 1.0`）
/// 会拒掉本来装得下的调用，而那正是 spec 点名的那种失败。真正当闸门的仍然是观测到的累计
/// 和，所以大于一的 margin 用「偶尔让一次调用超一点」换「更少的假拒绝」—— 超出的部分
/// 下一道闸门会抓住。
pub const DEFAULT_ESTIMATE_MARGIN: f64 = 1.5;

/// 会话的累计 token 额度（spec §17）。
///
/// `limit: None` 表示完全无上限，这是 v1 的缺省：机制在那儿，数字等着数据。执行者的额度
/// **不是**它自己的 —— 「独立预算」指的是它的回合上限，从来不是它的钱（spec §16）—— 所以
/// 这个值原样进入每一个嵌套会话。
#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    /// 会话累计 token 的硬上限，从流上每一条
    /// `UsageRecorded` 求和得来。
    pub limit: Option<u64>,
    /// 预检宽容度，以剩余量的倍数表示。见
    /// [`DEFAULT_ESTIMATE_MARGIN`]。
    pub estimate_margin: f64,
}

impl Budget {
    /// 无上限，用缺省的宽容度。
    pub fn new() -> Self {
        Self {
            limit: None,
            estimate_margin: DEFAULT_ESTIMATE_MARGIN,
        }
    }

    /// 把会话上限设为 `tokens`。零是合法的，它会在第一次调用之前就停下，这正是让
    /// 「合成器是那唯一一次不能被跳过的调用」可测的原因。
    pub fn with_limit(mut self, tokens: u64) -> Self {
        self.limit = Some(tokens);
        self
    }

    /// 设置预检宽容度。
    pub fn with_estimate_margin(mut self, margin: f64) -> Self {
        self.estimate_margin = margin;
        self
    }

    /// 额度还剩下多少；没有上限时是 `None`。
    pub fn remaining(&self, spent: u64) -> Option<u64> {
        self.limit.map(|limit| limit.saturating_sub(spent))
    }

    /// 硬停：这个会话是不是已经把额度花掉了？
    ///
    /// `spent` 是整条流的求和 —— 讨论者、合成器、执行者全都算 —— 从不是一个估计值。比较
    /// 用的是 `>=`，所以正好停在上限上的会话也算收工。
    pub fn is_exhausted(&self, spent: u64) -> bool {
        self.limit.is_some_and(|limit| spent >= limit)
    }

    /// 预检规则：一次估计为 `estimate` token 的调用还装得下吗？
    ///
    /// 在这里拒绝，代价是一次调用的工作量；不拒绝，代价是一次超支，而累计那道闸门会在
    /// 下一轮抓住它。
    pub fn admits_estimate(&self, spent: u64, estimate: u64) -> bool {
        let Some(remaining) = self.remaining(spent) else {
            return true;
        };
        let threshold = (remaining as f64 * self.estimate_margin).floor();
        (estimate as f64) <= threshold
    }

    /// 额度用尽时硬停要叙述的那句话；还有余地时是 `None`。
    ///
    /// 各道闸门各自追加它对此做了什么 —— 收束这一轮、拒绝这次派发 —— 所以事实本身只
    /// 措辞一次。
    pub fn exhausted_note(&self, spent: u64) -> Option<String> {
        self.is_exhausted(spent).then(|| {
            format!(
                "会话 token 额度已用尽：已经花掉 {spent} token，{}",
                self.cap_text()
            )
        })
    }

    /// 预检估计拒掉一次调用时的那句话。
    pub fn estimate_refusal_note(&self, estimate: u64) -> String {
        format!(
            "会话 token 额度：一次估计约 {estimate} token 的调用装不下 —— {}",
            self.cap_text()
        )
    }

    /// 上限本身，按上面那几条诊断的读法。
    fn cap_text(&self) -> String {
        match self.limit {
            Some(limit) => format!("上限为 {limit} token"),
            None => "无上限".to_owned(),
        }
    }
}

impl Default for Budget {
    /// 无上限，用缺省的宽容度 —— **不是**零 margin，那样会把一切都拒掉。
    fn default() -> Self {
        Self::new()
    }
}

/// 一个模型的价目表，单位是每百万 token 的 USD。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pricing {
    /// 缓存**未命中**的输入 token：一条全新提示词的开销。
    pub miss_input_per_mtok: f64,
    /// 缓存**命中**的输入 token。把它们定价成零，是在说这家供应商对命中不计费，而不是
    /// 这个值缺失。
    pub cached_input_per_mtok: f64,
    /// 输出 token，含推理。
    pub output_per_mtok: f64,
}

impl Pricing {
    pub fn new(miss_input_per_mtok: f64, cached_input_per_mtok: f64, output_per_mtok: f64) -> Self {
        Self {
            miss_input_per_mtok,
            cached_input_per_mtok,
            output_per_mtok,
        }
    }

    /// 一条用量记录花了多少钱，单位 USD。只作显示（spec §17）。
    pub fn cost(&self, usage: Usage) -> f64 {
        let (cached, miss) = self.billed_input(usage);
        (cached as f64 * self.cached_input_per_mtok
            + miss as f64 * self.miss_input_per_mtok
            + usage.output_tokens as f64 * self.output_per_mtok)
            / 1_000_000.0
    }

    /// 输入 token，按两家供应商区分的那两类拆开。
    ///
    /// 适配器会把 `cached + miss == input` 归一化，但一个只带总数的 `Usage` —— 测试里，
    /// 或者供应商不报缓存明细时 —— 绝不能被当成免费：只要报上来的 `miss` 比未缓存的余量
    /// 小，那部分余量就按未命中价计费。
    fn billed_input(&self, usage: Usage) -> (u64, u64) {
        let cached = usage.cached_tokens.min(usage.input_tokens);
        let miss = usage
            .miss_tokens
            .max(usage.input_tokens.saturating_sub(cached));
        (cached, miss)
    }
}

/// 配置好的价目表，按线级 model id 作键（spec §17）。
///
/// 按 model id 而不是按 provider profile 作键，因为两个讨论者就是不同的模型，而能力表与
/// `[models.*]` 本来也是按 model id 作键的。没有条目的模型是**没有**费用而不是费用为零：
/// 报告里「无价格」与「免费」不能长得一样。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PriceTable {
    entries: BTreeMap<String, Pricing>,
}

impl PriceTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, model: impl Into<String>, pricing: Pricing) {
        self.entries.insert(model.into(), pricing);
    }

    pub fn with(mut self, model: impl Into<String>, pricing: Pricing) -> Self {
        self.set(model, pricing);
        self
    }

    pub fn pricing(&self, model: &str) -> Option<&Pricing> {
        self.entries.get(model)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `usage` 在 `model` 上花了多少钱；该模型没有价格时是 `None`。
    pub fn cost(&self, model: &str, usage: Usage) -> Option<f64> {
        self.pricing(model).map(|pricing| pricing.cost(usage))
    }
}

/// 更便宜的模型可以被路由到的位置（spec §17）。
///
/// 全系统只有两个：合成器那一次收尾调用，以及讨论者派发的执行者。**讨论者绝不被路由** ——
/// 异构是这套协议手里最强的多样性杠杆（spec §15），而同一模型上的两方已经不再是异构的 ——
/// 所以这里刻意没有第三个变体，也没有任何讨论者形状的地方会调
/// [`crate::config::SessionConfig::model_for`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandingPoint {
    /// 合成器那一次调用，由 [`crate::assemble_discussion`] 组装。
    Synthesizer,
    /// `task` 派发的那些嵌套会话（`crate::agent::executor`）。
    Executor,
}

/// 每个落点按配置分别用哪个模型作答（spec §17）。
///
/// 就是会话级的 `[routing]` 表。两个值缺省都是 `None`，也就是 v1 的行为：在有数据可依
/// 之前，一切都用讨论的模型作答 —— 机制已就位，数字还没来。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Routing {
    /// 合成器作答所用的模型。
    pub synthesizer_model: Option<String>,
    /// 执行者作答所用的模型。
    pub executor_model: Option<String>,
}

impl Routing {
    /// 两个落点是不是都没有被路由，这是 v1 的缺省。
    pub fn is_empty(&self) -> bool {
        self.synthesizer_model.is_none() && self.executor_model.is_none()
    }

    /// 把配置里的覆盖应用到某一个 agent 的值上。
    ///
    /// 注意这里**没有**什么：讨论者的模型。既没有那个字段，也没有任何会去读它的调用点。
    pub fn apply(&self, config: &mut crate::config::SessionConfig) {
        config.synthesizer_model = self.synthesizer_model.clone();
        config.executor_model = self.executor_model.clone();
    }
}
