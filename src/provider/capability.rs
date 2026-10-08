//! 能力表：模型 id -> 那个模型实际支持什么。
//!
//! 按模型 id 构建，只建模本 crate 实际对话的这三家厂商。它是 `#[non_exhaustive]`，所以新增一
//! 项能力不是破坏性变更；而没登记的 id 是**错误**，绝不是悄悄降级（spec §4）：适配器要么知道
//! 某个模型的形状，要么拒绝去猜。
//!
//! 这些数字的来源是厂商自己的 API 参考：Kimi（`kimi-k3`，1M 上下文，`max_completion_tokens`
//! 上限 1048576，`reasoning_effort` 取 low/high/max，`cached_tokens`，`prompt_cache_key`，缓
//! 存下限 256 个 prompt token）、DeepSeek（`deepseek-flash` / `deepseek-v4-pro`，1M 上下文，
//! 384K 最大输出，`prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`，没有
//! `prompt_cache_key`）与 MiniMax（`MiniMax-M3.1-Flash-Preview` / `MiniMax-M3`，1M 上下文，
//! `reasoning_split` 把思考拆到 `reasoning_content`，`reasoning_effort` 只对 M3.1 生效，缓存
//! 是自动的、512 个输入 token 起，命中报在 `prompt_tokens_details.cached_tokens`）。

use std::fmt;

use crate::config::Vendor;

/// 承载输出 token 上限的那个线上参数。Kimi 弃用了 `max_tokens`、改用
/// `max_completion_tokens`；DeepSeek 仍然记的是 `max_tokens`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxTokensField {
    MaxTokens,
    MaxCompletionTokens,
}

impl MaxTokensField {
    pub fn field_name(&self) -> &'static str {
        match self {
            MaxTokensField::MaxTokens => "max_tokens",
            MaxTokensField::MaxCompletionTokens => "max_completion_tokens",
        }
    }
}

/// 一个模型 id 支持什么。每个字段都是关于厂商的事实，不是本 crate 的策略。
///
/// 有些条目是更晚的票要读、而不是适配器要读的：spec 钉住的事实都只住在这一处（§10 的可用输入预
/// 算要用上下文窗口，§17 的记账要用缓存下限，§5 的投影要用推理重放契约），所以在那些地方不必
/// 再猜一遍。
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ModelCaps {
    pub vendor: Vendor,
    /// 总上下文窗口，输入加输出。
    pub context_window: u32,
    /// 输出 token 上限可以取的最大值。永不大于 `context_window`：那个上限是同一个窗口里切出
    /// 来的一片。
    pub max_output_tokens: u32,
    pub supports_tools: bool,
    /// 会返回 `reasoning_content`。
    pub supports_reasoning: bool,
    /// 接受顶层的 `reasoning_effort` 档位。
    pub supports_reasoning_effort: bool,
    /// 带工具的请求里，模型自己的 `reasoning_content` 是否必须重放；丢掉它是厂商侧的错误
    /// （DeepSeek 直接 400）。重放由投影（§5）负责，这个标志只记录它为什么不是可选项。
    pub requires_reasoning_replay: bool,
    pub supports_temperature: bool,
    pub supports_top_p: bool,
    /// 接受 `prompt_cache_key`（Kimi）；DeepSeek 的缓存是自动的，没有这个参数。
    pub supports_prompt_cache_key: bool,
    /// 接受 `stream_options: {include_usage: true}`，它会在 `[DONE]` 之前多给一个只带用量
    /// 的 chunk。
    pub supports_stream_options: bool,
    /// 接受 `reasoning_split: true`（MiniMax）：为真时思考走 `reasoning_content`，否则它会以
    /// `<think>` 标签留在 `content` 里。这是**输出格式**的事实，不是用户的旋钮 —— 正文里混着
    /// 标签会同时污染事件流、投影重放与对话视图。
    pub reasoning_split: bool,
    pub max_tokens_field: MaxTokensField,
    /// 低于这个 prompt token 数时厂商根本不缓存。
    pub min_cacheable_tokens: u32,
}

/// 本 crate 建模的全部模型 id，顺序稳定，便于诊断。
pub const KNOWN_MODELS: &[&str] = &[
    // Kimi Open Platform。
    "kimi-k3",
    // Kimi Code（编程套餐）；K3 在它那里以 `k3` / `k3-256k` 暴露。
    "k3",
    "k3-256k",
    "kimi-for-coding",
    "kimi-for-coding-highspeed",
    // DeepSeek。
    "deepseek-flash",
    "deepseek-v4-pro",
    // MiniMax（M3.1 走 M Plan，M3 两者皆可）。
    "MiniMax-M3.1-Flash-Preview",
    "MiniMax-M3",
];

/// 按 id 查模型。不认识的 id 是错误，永远不给默认值。
pub fn caps_for(model: &str) -> Result<ModelCaps, UnknownModel> {
    let caps = match model {
        // 同一个 K3 模型，一个是 Open Platform 的 id，两个是编程套餐的 id。
        "kimi-k3" | "k3" => k3_caps(1_048_576),
        "k3-256k" => k3_caps(262_144),
        // Kimi Code 的 K2.x 模型。`kimi-for-coding` 是 K2.8 Preview（接受一个 effort
        // 档位）；`kimi-for-coding-highspeed` 是 K2.7 Code，思考常开、没有档位。
        "kimi-for-coding" => kimi_code_k2_caps(1_048_576, true),
        "kimi-for-coding-highspeed" => kimi_code_k2_caps(262_144, false),
        "deepseek-v4-pro" | "deepseek-flash" => deepseek_caps(),
        // MiniMax 的 M3 系：`reasoning_effort` 只对 M3.1 生效，其余一模一样。
        "MiniMax-M3.1-Flash-Preview" => minimax_caps(true),
        "MiniMax-M3" => minimax_caps(false),
        other => return Err(UnknownModel::new(other)),
    };
    Ok(caps)
}

/// K3，无论走 Open Platform 还是 Kimi Code 进来。
///
/// K3 没有记下任何比窗口更紧的输出上限，并且把采样固定在 temperature 1.0 / top_p 0.95，同时
/// 要求不要把它们发过去。
fn k3_caps(context_window: u32) -> ModelCaps {
    ModelCaps {
        vendor: Vendor::Kimi,
        context_window,
        max_output_tokens: context_window,
        supports_tools: true,
        supports_reasoning: true,
        supports_reasoning_effort: true,
        requires_reasoning_replay: true,
        supports_temperature: false,
        supports_top_p: false,
        supports_prompt_cache_key: true,
        supports_stream_options: true,
        reasoning_split: false,
        max_tokens_field: MaxTokensField::MaxCompletionTokens,
        min_cacheable_tokens: 257,
    }
}

fn kimi_code_k2_caps(context_window: u32, supports_reasoning_effort: bool) -> ModelCaps {
    ModelCaps {
        vendor: Vendor::Kimi,
        context_window,
        max_output_tokens: context_window,
        supports_tools: true,
        supports_reasoning: true,
        supports_reasoning_effort,
        requires_reasoning_replay: true,
        supports_temperature: true,
        supports_top_p: true,
        supports_prompt_cache_key: true,
        supports_stream_options: true,
        reasoning_split: false,
        max_tokens_field: MaxTokensField::MaxCompletionTokens,
        min_cacheable_tokens: 257,
    }
}

fn deepseek_caps() -> ModelCaps {
    ModelCaps {
        vendor: Vendor::DeepSeek,
        context_window: 1_048_576,
        // DeepSeek 记下的输出上限是硬性的 384K，在它 1M 的窗口之下。
        max_output_tokens: 393_216,
        supports_tools: true,
        supports_reasoning: true,
        supports_reasoning_effort: true,
        requires_reasoning_replay: true,
        supports_temperature: true,
        supports_top_p: true,
        supports_prompt_cache_key: false,
        supports_stream_options: true,
        reasoning_split: false,
        max_tokens_field: MaxTokensField::MaxTokens,
        min_cacheable_tokens: 0,
    }
}

/// MiniMax 的 M3 系（`MiniMax-M3.1-Flash-Preview` 与 `MiniMax-M3`），走 OpenAI 兼容端点。
///
/// `max_output_tokens` 取窗口值：**官方没有记下输出上限**，所以照「不比窗口更紧」办（与 K3
/// 同一条先例）。这是一处出处缺口 —— 真撞上厂商侧更小的界时会得到一次 `InvalidRequest`，比在这里
/// 猜一个更小的数字诚实。
///
/// 缓存是自动的（没有 `prompt_cache_key` 这类参数），512 个输入 token 起才缓存，命中的读数报在
/// `prompt_tokens_details.cached_tokens`。`reasoning_effort` 只有 M3.1 认；`reasoning_split`
/// 两条都发，于是思考一律走 `reasoning_content` —— 默认形态会把 `<think>` 留在正文里。
fn minimax_caps(supports_reasoning_effort: bool) -> ModelCaps {
    ModelCaps {
        vendor: Vendor::MiniMax,
        context_window: 1_048_576,
        max_output_tokens: 1_048_576,
        supports_tools: true,
        supports_reasoning: true,
        supports_reasoning_effort,
        requires_reasoning_replay: true,
        supports_temperature: true,
        supports_top_p: true,
        supports_prompt_cache_key: false,
        supports_stream_options: true,
        reasoning_split: true,
        max_tokens_field: MaxTokensField::MaxCompletionTokens,
        min_cacheable_tokens: 512,
    }
}

/// 没登记的模型 id。这里刻意做得响：新模型必须带着有出处的数字进表才能跑。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownModel {
    pub model: String,
}

impl UnknownModel {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
        }
    }
}

impl fmt::Display for UnknownModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "未知的 model `{}`：它不在能力表里，而 heng 从不对能力做猜测。\
             已知的 model id：{}",
            self.model,
            KNOWN_MODELS.join(", ")
        )
    }
}

impl std::error::Error for UnknownModel {}
