//! 配置边界。
//!
//! 库代码从不读环境：这里的解析是对一张显式环境表（[`resolve`]）的纯函数，只有 `cli` 读
//! 进程环境，并把快照交进来。
//!
//! # 两层（spec §4）
//!
//! `[providers.*]` 放 `base_url` 与跟它配套的密钥；`[models.*]` 按名字引用一个 provider，
//! 并覆盖生成参数。模型段的键**就是**线级 model id，同时也是能力表的键 —— 所以「未登记的
//! model id」是 [`crate::provider::capability`] 里的一次查表失败，而不是静默降级。
//!
//! # 优先级
//!
//! 逐字段：`config.toml` > 导出的环境变量 > 内置缺省。项目里的 `.env` **永不**被加载：
//! [`Config::load`] 只读递给它的那个路径，以及递给它的那张环境表。
//!
//! # 用词
//!
//! **回合（Turn）** 是一次 provider 调用加上它的工具执行；`max_iterations` 数的是一个
//! agent 循环里的回合数。**轮次（round）** 是讨论协议的一步（§15）。[`Budget`] 是会话的
//! 累计 token 额度（§17）—— 别和 [`crate::context`] 里按 agent 各自算的**窗口**预算混了。

pub mod cost;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::events::{Decision, Redactor};
use crate::permissions::Mode;
// 方向与 `Mode` 那类配置枚举相反：制式是措辞，所以它定义在措辞层里，配置只是持有它
// （`.scratch/usage-stats-format/spec.md` §1）。
use crate::render::wording::NumberStyle;

pub use cost::{Budget, LandingPoint, PriceTable, Pricing, Routing};

/// 一个回合里 provider 调用的缺省上限（spec §3）。
pub const DEFAULT_MAX_ITERATIONS: u32 = 100;

/// 执行者一个回合里 provider 调用的缺省上限（spec §16）：即 goose 的
/// `GOOSE_SUBAGENT_MAX_TURNS`。执行者是来干一件活的，不是来跑马拉松的，它自己的上限正是
/// 拦住一个失控执行者吃掉整个会话回合预算的东西。
pub const DEFAULT_EXECUTOR_MAX_ITERATIONS: u32 = 25;

/// 一批工具调用里同时跑多少个执行者的缺省上限（spec §16）：即 goose 的
/// `GOOSE_MAX_BACKGROUND_TASKS`。
///
/// 这是成本与速率的闸门，不是安全的闸门：执行者之间的写互斥是共享路径锁的活，而这条上限
/// 存在是因为 N 个执行者乘上各自的回合预算、再乘上不断长大的上下文会放大。
pub const DEFAULT_MAX_PARALLEL_EXECUTORS: usize = 5;

/// 单条工具结果的缺省上限，按估计 token 计（spec §10，票 07）：Anthropic 记 Claude Code
/// 的工具响应上限默认是 25k。
pub const DEFAULT_MAX_TOOL_RESULT_TOKENS: u64 = 25_000;

/// 仓库地图的缺省预算，按估计 token 计（spec §9，票 09）：aider 为它自己的 `--map-tokens`
/// 记录的缺省值与此相同。
pub const DEFAULT_REPO_MAP_TOKENS: u64 = 1_024;

/// 配置出来的仓库地图预算的上限（spec §9，票 09）：aider 的源码就把 `--map-tokens` 夹在
/// 这里，本项目的配置也这么夹。固定预算才是重点 —— 模型没法按调用要一张更大的地图。
pub const MAX_REPO_MAP_TOKENS: u64 = 4_096;

/// 单次 `bash` 调用的缺省墙钟上限，按毫秒计（spec §7，票 20）：Claude Code 记录的缺省也
/// 是两分钟。
pub const DEFAULT_BASH_TIMEOUT_MS: u64 = 120_000;

/// `bash` 超时的上限，按毫秒计（spec §7，票 20）。模型可以按调用要得更短，永远要不到更长，
/// 所以没有任何一条命令能无限期攥着工作区级的 `Exclusive` 锁。
pub const MAX_BASH_TIMEOUT_MS: u64 = 600_000;

/// 沙箱的那一档（沙箱 spec §7）：把命令包进 bubblewrap，还是显式放弃这一层。
///
/// 它与权限模式的四档是**两件不同的事**：这一档决定「跑起来能碰到什么」，权限模式决定
/// 「跑不跑、要不要问」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    /// 命令包进 bubblewrap，越界由内核以 `EROFS` 打回。
    Bwrap,
    /// 显式放弃这一层：`wrap()` 退化成单位函数，探测根本不跑。
    Off,
}

impl SandboxMode {
    /// 配置与事件流里用的那个词（协议标记，英文）。
    pub fn as_str(self) -> &'static str {
        match self {
            SandboxMode::Bwrap => "bwrap",
            SandboxMode::Off => "off",
        }
    }
}

/// 除会话 cwd 之外，缺省可写的那些工具缓存目录。
///
/// **这是可用性决定，不是安全决定**：没有它们，`cargo build` 会因为写不了
/// `~/.cargo/.package-cache` 而失败。刻意不放 `~/.npm`、`~/.aws`、`~/.ssh`。
pub const DEFAULT_SANDBOX_WRITABLE_ROOTS: &[&str] = &["~/.cargo", "~/.rustup", "~/.cache"];

/// 被遮住的目录（写死，不给旋钮）：provider key 与 ssh 私钥都在这两个地方。
///
/// 表现是「目录还在，但是空的、且只读」——**不是「不存在」**。
const SANDBOX_MASKS: &[&str] = &["~/.config/fs-agent", "~/.ssh"];

/// 沙箱是否可用（沙箱 spec §3）。
///
/// 探测在组装期做**一次**，结果随会话配置携带；每条命令都不重探，也不理会 PATH 中途的
/// 变化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxAvailability {
    /// 还没探过。组装期会把它换成另外两个值之一（`mode = "off"` 时原样留着）。
    Untested,
    /// `bwrap` 在这里起得来。
    Available { bwrap: PathBuf },
    /// 探过，用不了。`reason` 是给人看的一句话。
    Unavailable { reason: String },
}

/// `[sandbox]` 这一节解析出来的值，外加组装期填进去的探测结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxSettings {
    pub mode: SandboxMode,
    /// 除会话 cwd 之外的可写根，`~` 已展开。不存在的那些会在拼装时被跳过。
    pub writable_roots: Vec<PathBuf>,
    /// 遮罩目录，`~` 已展开。
    pub masks: Vec<PathBuf>,
    /// 探测的结果（spec §3）。组装期填。
    pub availability: SandboxAvailability,
    /// 探测去哪里找 `bwrap`（就是 `PATH` 的原文）。库从不读进程环境，所以这份值由 `cli`
    /// 从它的环境快照里带进来；测试也从同一个口子注入一个假的 PATH。
    pub search_path: Option<OsString>,
}

impl SandboxSettings {
    /// 没有配置过沙箱的会话：这一层是关的。
    ///
    /// `cli` 的每一条组装路径都经 [`Config::session_config`] 拿到配置里的值（缺省
    /// `bwrap`），所以「默认开」是配置那一侧的事实；这里给的是「没有配置装配过」的值，
    /// 库内直接构造的会话（测试、以及别的调用方）因此不会凭空去跑一个探测。
    pub fn off() -> Self {
        Self {
            mode: SandboxMode::Off,
            writable_roots: Vec::new(),
            masks: Vec::new(),
            availability: SandboxAvailability::Untested,
            search_path: None,
        }
    }

    /// 组装期是不是该去探一次。
    pub fn needs_probe(&self) -> bool {
        self.mode == SandboxMode::Bwrap
            && matches!(self.availability, SandboxAvailability::Untested)
    }
}

/// 每一个动态声明的工具的线级名都以它开头（spec §14）。内建名永不含 `__`，所以「这个名字
/// 里有 `__`」是「这个工具来自配置」的词法可判定测试。
pub const CUSTOM_TOOL_PREFIX: &str = "custom__";

/// 命名空间与工具名之间的分隔符，以及内建名不得包含它的原因。
pub const CUSTOM_TOOL_SEPARATOR: &str = "__";

/// 单次动态声明的工具调用的缺省墙钟上限，按毫秒计。它的声明可以调低，也可以调高到下面那个
/// 上限为止。
pub const DEFAULT_CUSTOM_TOOL_TIMEOUT_MS: u64 = 30_000;

/// 动态声明的工具的超时上限：一份声明没法让某条命令无限期攥着工作区级的 `Exclusive` 锁。
pub const MAX_CUSTOM_TOOL_TIMEOUT_MS: u64 = 600_000;

/// 一个动态声明的工具的线级名（spec §14）：`custom__<namespace>__<tool>`。
pub fn custom_tool_name(namespace: &str, tool: &str) -> String {
    format!("{CUSTOM_TOOL_PREFIX}{namespace}{CUSTOM_TOOL_SEPARATOR}{tool}")
}

/// 一条解析并校验过的 `[tools.<namespace>.<tool>]` 声明。
///
/// 参数就是 provider 收到的线级形状，**原样** —— 没有翻译层，用户写的是什么，模型就收到
/// 什么。刻意没有副作用字段：「这一个其实只读」无处可说，每个动态工具都是 `Exclusive`
/// （spec §14）。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDeclaration {
    /// 线级名：`custom__<namespace>__<tool>`。
    pub name: String,
    pub namespace: String,
    pub tool: String,
    pub description: String,
    /// argv 模板。形如 `{name}` 的整个元素会被那个参数替换；别的都是字面量。
    pub command: Vec<String>,
    /// JSON Schema，原样发送。
    pub parameters: serde_json::Value,
    /// 解析后的墙钟上限，夹在
    /// [`MAX_CUSTOM_TOOL_TIMEOUT_MS`] 之内。
    pub timeout_ms: u64,
}

/// 没有配置也没有导出 `default_model` 时用的模型。
pub const DEFAULT_MODEL: &str = "kimi-k3";

/// 一条 provider profile 对哪家厂商说话。只有这两家被建模；`#[non_exhaustive]` 让任何地方
/// 都不会假定存在第三家。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Vendor {
    Kimi,
    DeepSeek,
}

impl Vendor {
    /// 诊断里用的显示名。
    pub fn as_str(&self) -> &'static str {
        match self {
            Vendor::Kimi => "Kimi",
            Vendor::DeepSeek => "DeepSeek",
        }
    }

    /// 这家厂商的密钥允许与哪些主机配对。
    ///
    /// Kimi 有两套共享一个厂商的系统：开放平台（`api.moonshot.cn` / `api.moonshot.ai`）
    /// 与 Kimi Code，也就是 coding plan（`api.kimi.com`）。两者的密钥不能互换，但都是
    /// Kimi。
    pub fn hosts(&self) -> &'static [&'static str] {
        match self {
            Vendor::Kimi => &["api.moonshot.cn", "api.moonshot.ai", "api.kimi.com"],
            Vendor::DeepSeek => &["api.deepseek.com"],
        }
    }
}

/// 一条内置的 `[providers.*]` profile：端点，以及它的密钥从哪些环境变量读。Kimi 贡献两条
/// profile，因为它的开放平台与 coding plan 是密钥不同的两套系统。
pub struct BuiltinProvider {
    pub name: &'static str,
    pub vendor: Vendor,
    pub base_url: &'static str,
    pub key_env: &'static str,
    pub alt_key_envs: &'static [&'static str],
}

impl BuiltinProvider {
    /// 可能携带这条 profile 密钥的每一个环境变量，首选的在前。解析与错误提示共用同一张表。
    pub fn key_envs(&self) -> Vec<&'static str> {
        let mut names = vec![self.key_env];
        names.extend(self.alt_key_envs.iter().copied());
        names
    }
}

/// 内置的那些 profile。`kimi` 是开放平台，`kimi-code` 是 coding plan；`KIMI_API_KEY`
/// 属于后者，与 Kimi 自己那份第三方工具文档一致。
pub const BUILTIN_PROVIDERS: &[BuiltinProvider] = &[
    BuiltinProvider {
        name: "kimi",
        vendor: Vendor::Kimi,
        base_url: "https://api.moonshot.cn/v1",
        key_env: "MOONSHOT_API_KEY",
        alt_key_envs: &[],
    },
    BuiltinProvider {
        name: "kimi-code",
        vendor: Vendor::Kimi,
        base_url: "https://api.kimi.com/coding/v1",
        key_env: "KIMI_API_KEY",
        alt_key_envs: &["KIMI_CODE_API_KEY"],
    },
    BuiltinProvider {
        name: "deepseek",
        vendor: Vendor::DeepSeek,
        base_url: "https://api.deepseek.com",
        key_env: "DEEPSEEK_API_KEY",
        alt_key_envs: &[],
    },
];

fn builtin_provider(name: &str) -> Option<&'static BuiltinProvider> {
    BUILTIN_PROVIDERS
        .iter()
        .find(|builtin| builtin.name == name)
}

/// 推理档位。两家厂商都在请求顶层接受它，但只有部分 model id 真正照办（spec §4：Kimi K3 /
/// DeepSeek）。档位在会话开始时就定死：中途改它会扔掉前缀缓存。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    High,
    Max,
}

impl ReasoningEffort {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReasoningEffort::Low => "low",
            ReasoningEffort::High => "high",
            ReasoningEffort::Max => "max",
        }
    }
}

/// 中立的生成参数。provider 适配器拿模型能力表过滤它们，并在丢掉一个明确设置过的值时告警。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GenerationParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// 整个会话钉住；中途从不切换。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
}

/// 一个 provider 的密钥从哪来。来源决定这个密钥是否绑定厂商主机（spec §4：`base_url` 必须
/// 与密钥的出身一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// 写在 `config.toml` 里 `base_url` 旁边：配对是明写的。
    Config,
    /// 从这个环境变量读到。
    Env(String),
    /// 没找到；为它构造 provider 是错误。
    Missing,
}

impl KeySource {
    pub fn env_var(&self) -> Option<&str> {
        match self {
            KeySource::Env(name) => Some(name),
            _ => None,
        }
    }
}

/// 一条解析好的 provider profile：一个 `base_url` + 一个密钥。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderProfile {
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub key_source: KeySource,
    /// 这条 profile 从哪个环境变量读密钥（缺失时则是本该读的那个）。诊断里会点名它，这样
    /// 修法可以直接抄。
    pub key_env: String,
    /// 内置厂商 profile 是 `Some`，自定义条目是 `None`。
    pub vendor: Option<Vendor>,
}

/// 一条解析好的模型条目：一个线级 model id、它的 provider、以及它的参数。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelProfile {
    /// 线级 model id，作为 `model` 发送，并用作能力表的键。
    pub id: String,
    /// 服务这个模型的那条 [`ProviderProfile`] 的名字。
    pub provider: String,
    pub params: GenerationParams,
}

/// 完全解析好的配置。
#[derive(Debug, Clone)]
pub struct Config {
    pub default_model: String,
    pub providers: BTreeMap<String, ProviderProfile>,
    pub models: BTreeMap<String, ModelProfile>,
    /// 每个模型一百万 token 多少钱，作显示用（spec §17）。按 model id 作键；没有条目的模型
    /// 报作「无价格」，永不是免费。
    pub pricing: PriceTable,
    /// 会话开始时的那一档模式（spec §12；`.scratch/todo-and-modes/spec.md` §1）。会话对
    /// 「写」的立场，由**用户**选 —— `--mode` 为一次运行覆盖它，`Shift+Tab` 在会话内循环
    /// 四档。它从不进事件流，所以 `--continue` 回到的是这个值。
    pub mode: Mode,
    /// `[permissions] outside_read`：读目标落在会话 cwd 之外时给什么裁决（缺省 `deny`）。
    /// 它与档位正交、全局生效 —— `ask` 档配上 `"allow"` 恰好就是 DSH 的「读全放、写要问」
    /// （`.scratch/workspace-mode/spec.md` §2）。
    pub outside_read: Decision,
    /// 会话的累计 token 额度（spec §17），供显示，也供会话组装时用的那道闸门。
    pub budget: Budget,
    /// 两个落点各用哪个模型作答（spec §17）。缺省为空：v1 一切都跑在讨论的模型上。
    pub routing: Routing,
    /// 一个回合循环里 provider 调用的硬上限（spec §3）。这是个**按 agent** 的值，与会话的
    /// 预算不同：两个讨论者各跑自己的回合循环，所以不必在这一项上一致。
    pub max_iterations: u32,
    /// 执行者一个回合里 provider 调用的硬上限（spec §16）。执行者是用会话配置构造的，只是
    /// 用这个值替掉派发者自己的上限，所以它独立于 [`Config::max_iterations`] 是构造上的事实，
    /// 而不是靠一条要人记住的规则。
    pub executor_max_iterations: u32,
    /// 配置了 `[discussion]` 时谁参与辩论（spec §15）。缺席表示这份配置是给单 agent 会话用
    /// 的；`fs-agent discuss` 会照说，而不是凭空编一份名册。
    pub discussion: Option<DiscussionRoster>,
    /// 动态声明的那些工具（spec §14），按稳定的名字顺序。组装期就定死：没有任何东西会在
    /// 会话中途增删工具，因为工具数组是前缀缓存的一部分。
    pub tools: Vec<ToolDeclaration>,
    /// `[sandbox]`：包不包 bubblewrap、额外哪些目录可写（spec §7）。探测结果在组装期填进
    /// 会话配置里那一份。
    pub sandbox: SandboxSettings,
    /// `[goals]`：目标循环的那几个旋钮 —— 两个阈值（§6）与两条停止线（§9）。
    pub goals: GoalSettings,
    /// `[ui] number_style`：界面上那些计数用哪套书写制式
    /// （`.scratch/usage-stats-format/spec.md` §2）。缺省 `cn`（万 / 亿）。
    pub number_style: NumberStyle,
}

/// 目标循环的四个旋钮：两个阈值按**窗口**的百分比，两条停止线按次数。
///
/// 缺省 50 / 80：过半提醒一次（要模型把还没落流的东西落下来），过八成压缩并开一个新会话；
/// 再加上「连续几次翻页零完成就停下」与「provider 失败重试几次」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalSettings {
    /// 过这个百分比就注入一次提醒。
    pub remind_at: u8,
    /// 过这个百分比就压缩并翻页。
    pub compact_at: u8,
    /// 连续几次翻页零条目完成就停下报告（§9）。
    pub no_progress_rollovers: u32,
    /// provider 调用连着失败几次就停下报告（§9）。
    pub provider_retries: u32,
}

impl Default for GoalSettings {
    fn default() -> Self {
        Self {
            remind_at: DEFAULT_REMIND_AT,
            compact_at: DEFAULT_COMPACT_AT,
            no_progress_rollovers: DEFAULT_NO_PROGRESS_ROLLOVERS,
            provider_retries: DEFAULT_PROVIDER_RETRIES,
        }
    }
}

/// 缺省的提醒阈值。
pub const DEFAULT_REMIND_AT: u8 = 50;
/// 缺省的翻页阈值。
pub const DEFAULT_COMPACT_AT: u8 = 80;
/// 缺省的「连续几次翻页零完成就停下」。
pub const DEFAULT_NO_PROGRESS_ROLLOVERS: u32 = 3;
/// 缺省的 provider 重试次数。
///
/// 两次：一次重试盖得住一次抖动（限流、一次连接断掉），再多就是在拿无人值守的钱去赌一个大概
/// 不会好的东西。
pub const DEFAULT_PROVIDER_RETRIES: u32 = 2;
/// provider 重试次数的上限。
pub const MAX_PROVIDER_RETRIES: u32 = 10;

/// `[ui]`：显示层的选择（`.scratch/usage-stats-format/spec.md` §2）。
///
/// 这一节是**为以后留的口子**：颜色、密度、数字制式这类「只是给人看」的选择都住在这里，
/// 而不是散进 `[permissions]` 那种语义小节。缺省只有一个值，所以 `Default` 就是那处缺省
/// —— [`NumberStyle::default`] 说的那套制式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UiSettings {
    /// 界面上那些计数用哪套书写制式（万 / 亿，或 k / M / G）；缺省由
    /// [`NumberStyle`] 自己说。
    pub number_style: NumberStyle,
}

/// 把 `[goals]` 解析成那四个旋钮。
///
/// 校验都是为了挡住一个「看起来配好了、实际不会发生」的组合：两个阈值都得落在 1..=100，提醒
/// 必须在翻页之前（反过来的话提醒永远轮不到，压缩先来了），`no_progress_rollovers` 至少是 1，
/// 而重试次数有上限 —— 无人值守的重试是有代价的。
fn resolve_goals(raw: Option<&RawGoals>) -> Result<GoalSettings, ConfigError> {
    let mut goals = GoalSettings::default();
    let Some(raw) = raw else {
        return Ok(goals);
    };
    if let Some(remind_at) = raw.remind_at {
        goals.remind_at = remind_at;
    }
    if let Some(compact_at) = raw.compact_at {
        goals.compact_at = compact_at;
    }
    if let Some(rollovers) = raw.no_progress_rollovers {
        goals.no_progress_rollovers = rollovers;
    }
    if let Some(retries) = raw.provider_retries {
        goals.provider_retries = retries;
    }
    for (field, value) in [
        ("remind_at", goals.remind_at),
        ("compact_at", goals.compact_at),
    ] {
        if value == 0 || value > 100 {
            return Err(ConfigError::InvalidGoals {
                reason: format!("`{field}` 是窗口的百分比，取值落在 1..=100，拿到的是 {value}"),
            });
        }
    }
    if goals.remind_at >= goals.compact_at {
        return Err(ConfigError::InvalidGoals {
            reason: format!(
                "`remind_at`（{}）必须在 `compact_at`（{}）之前，否则提醒永远轮不到 \
                 压缩先来了",
                goals.remind_at, goals.compact_at
            ),
        });
    }
    if goals.no_progress_rollovers == 0 {
        return Err(ConfigError::InvalidGoals {
            reason: "`no_progress_rollovers` 至少是 1：0 会让循环在第一次翻页之前就认输".to_owned(),
        });
    }
    if goals.provider_retries > MAX_PROVIDER_RETRIES {
        return Err(ConfigError::InvalidGoals {
            reason: format!(
                "`provider_retries` 最多 {MAX_PROVIDER_RETRIES} 次；拿到的是 {} —— \
                 无人值守的重试是有代价的",
                goals.provider_retries
            ),
        });
    }
    Ok(goals)
}

/// 把 `[ui]` 解析成显示层的选择（spec §2）。
///
/// 一个不认识的词是启动错误，而不是静默回退到 `cn`：写 `number_style = "wan"` 的人以为
/// 自己配好了，屏幕上却是另一套读法，那种「配了等于没配」只有报错才说得清。
fn resolve_ui(raw: Option<&RawUi>) -> Result<UiSettings, ConfigError> {
    let Some(written) = raw.and_then(|raw| raw.number_style.as_deref()) else {
        return Ok(UiSettings::default());
    };
    match written {
        "cn" => Ok(UiSettings {
            number_style: NumberStyle::Cn,
        }),
        "si" => Ok(UiSettings {
            number_style: NumberStyle::Si,
        }),
        other => Err(ConfigError::UnknownNumberStyle {
            value: other.to_owned(),
        }),
    }
}

impl Config {
    pub fn model(&self, id: &str) -> Option<&ModelProfile> {
        self.models.get(id)
    }

    pub fn provider(&self, name: &str) -> Option<&ProviderProfile> {
        self.providers.get(name)
    }

    /// 两个配置好的模型是否已知来自**同一家厂商**。
    ///
    /// 仅供参考，而且是就真正在辩论的那一对来问：协议需要的是两个*身份*，不是两家厂商，但
    /// 来自同一家厂商的一对是两个样本而不是两个独立判断，所以前端会把这件事说出来
    /// （spec §15）。未知模型答 `false` —— 模型检查发生得更早。
    pub fn debaters_share_a_vendor(&self, first: &str, second: &str) -> bool {
        same_vendor(&self.models, &self.providers, first, second)
    }

    /// 服务 `model_id` 的那条 provider profile。
    pub fn provider_for(&self, model_id: &str) -> Option<&ProviderProfile> {
        let model = self.models.get(model_id)?;
        self.providers.get(&model.provider)
    }

    /// 一个 `model_id` 的会话开局用哪些按 agent 的值：模型的生成参数、会话的额度、它用来
    /// 显示花费的价目表、它可以花的回合上限（spec §3），以及路由覆盖（spec §17）。
    ///
    /// 做成一个函数，而不是在每个组装点重复那同样的四行，这样「`[budget]` 表管着会话」在
    /// 任何组装会话的地方都成立，而不只在有人记得抄它的地方成立。
    ///
    /// 打码器搭车同行也是同一个理由：每一条经过这里的组装路径都拿到配置里的密钥值去替换，
    /// 不必记得去要；而 `assemble_discussion` 会拒绝参与者在这一项上不一致的名册，所以没有
    /// 哪条流是只打了一半码的。
    pub fn session_config(&self, model_id: &str) -> Result<SessionConfig, ConfigError> {
        let (model, _) = self.resolve_model(Some(model_id))?;
        let mut config = SessionConfig::new(model_id).with_params(model.params.clone());
        config.pricing = self.pricing.clone();
        config.budget = self.budget.clone();
        config.redactor = self.redactor();
        config.max_iterations = self.max_iterations;
        config.executor_max_iterations = self.executor_max_iterations;
        config.sandbox = self.sandbox.clone();
        self.routing.apply(&mut config);
        Ok(config)
    }

    /// 绝不能进事件流的那些值（spec §20）：每一条解析出来的 provider 密钥。
    ///
    /// 这些密钥就是这个进程被配置时的那些秘密 —— 来自 `config.toml` 或导出的环境 —— 这正是
    /// 值级打码诚实的范围。用户手里别处存的密钥，这里并不知道，也猜不出来。
    pub fn redactor(&self) -> Redactor {
        Redactor::new(
            self.providers
                .values()
                .filter_map(|provider| provider.api_key.clone()),
        )
    }

    /// provider 有可用密钥的那些模型。CLI 的探测用它来判定自己实际能跑什么。
    pub fn models_with_keys(&self) -> Vec<&ModelProfile> {
        self.models
            .values()
            .filter(|model| {
                self.providers
                    .get(&model.provider)
                    .is_some_and(|provider| provider.api_key.is_some())
            })
            .collect()
    }

    /// 解析出要跑的模型，缺省用 `default_model`。
    pub fn resolve_model(
        &self,
        requested: Option<&str>,
    ) -> Result<(&ModelProfile, &ProviderProfile), ConfigError> {
        let id = requested.unwrap_or(&self.default_model);
        let model = self
            .models
            .get(id)
            .ok_or_else(|| ConfigError::UnknownModel {
                model: id.to_owned(),
            })?;
        let provider =
            self.providers
                .get(&model.provider)
                .ok_or_else(|| ConfigError::UnknownProvider {
                    model: id.to_owned(),
                    provider: model.provider.clone(),
                })?;
        Ok((model, provider))
    }
}

/// 环境快照。库从不读进程环境，所以由调用方交进来；测试直接构造一张。
pub type EnvMap = BTreeMap<String, String>;

/// 从内存里的输入解析配置。
///
/// `file_text` 是 `config.toml` 的内容（`None` = 没有文件）。`env` 是导出的环境。别的什么
/// 都不去查，这正是让「项目里的 `.env` 不会被加载」成为构造性事实的原因。
pub fn resolve(file_text: Option<&str>, env: &EnvMap) -> Result<Config, ConfigError> {
    let raw = match file_text {
        Some(text) => toml::from_str::<RawConfig>(text).map_err(|source| ConfigError::Parse {
            source: Box::new(source),
        })?,
        None => RawConfig::default(),
    };

    let providers = resolve_providers(&raw, env)?;
    let models = resolve_models(&raw, &providers)?;
    let pricing = resolve_pricing(&raw, &models)?;
    let mode = resolve_mode(raw.permissions.as_ref())?;
    let outside_read = resolve_outside_read(raw.permissions.as_ref())?;
    let budget = resolve_budget(raw.budget.as_ref())?;
    let routing = resolve_routing(raw.routing.as_ref(), &models)?;
    let max_iterations = raw
        .turn
        .as_ref()
        .and_then(|turn| turn.max_iterations)
        .unwrap_or(DEFAULT_MAX_ITERATIONS);
    let executor_max_iterations = raw
        .turn
        .as_ref()
        .and_then(|turn| turn.executor_max_iterations)
        .unwrap_or(DEFAULT_EXECUTOR_MAX_ITERATIONS);
    let discussion = resolve_discussion(raw.discussion.as_ref(), &models)?;
    let tools = resolve_tools(&raw.tools)?;
    let sandbox = resolve_sandbox(raw.sandbox.as_ref(), env)?;
    let goals = resolve_goals(raw.goals.as_ref())?;
    let ui = resolve_ui(raw.ui.as_ref())?;

    let default_model = raw
        .default_model
        .clone()
        .or_else(|| env.get("FS_AGENT_MODEL").filter(|v| !v.is_empty()).cloned())
        .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
    if !models.contains_key(&default_model) {
        return Err(ConfigError::UnknownModel {
            model: default_model,
        });
    }

    Ok(Config {
        default_model,
        providers,
        models,
        pricing,
        mode,
        outside_read,
        budget,
        routing,
        max_iterations,
        executor_max_iterations,
        discussion,
        tools,
        sandbox,
        goals,
        number_style: ui.number_style,
    })
}

/// 读取并解析 `path` 处的 `config.toml`。文件缺失在这里是错误；把缺失当作「只用缺省」的
/// 调用方给 [`resolve`] 传 `None`。
pub fn load(path: &Path, env: &EnvMap) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.display().to_string(),
        source,
    })?;
    resolve(Some(&text), env)
}

/// 缺省的配置路径：`$XDG_CONFIG_HOME/fs-agent/config.toml`，否则
/// `$HOME/.config/fs-agent/config.toml`。
pub fn default_path(env: &EnvMap) -> PathBuf {
    let base = env
        .get("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env.get("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("fs-agent").join("config.toml")
}

/// 缺省的会话存储根目录：`$XDG_DATA_HOME/fs-agent/sessions`，否则
/// `$HOME/.local/share/fs-agent/sessions`（spec §11）。
///
/// 两个变量都没设时是 `None`：CLI 会照说，而不是凭空造一个目录把会话写进去。存储本身把根
/// 目录当参数收，所以库从不读它。
pub fn sessions_dir(env: &EnvMap) -> Option<PathBuf> {
    Some(data_dir(env)?.join("fs-agent").join("sessions"))
}

/// 缺省的目标清单目录：`$XDG_DATA_HOME/fs-agent/goals`，否则
/// `$HOME/.local/share/fs-agent/goals`（`.scratch/goal-loop/spec.md` §1）。
///
/// 与 [`sessions_dir`] 同一个数据根：清单与它跨过的那些会话属于同一个人的同一批数据。两个
/// 变量都没设时是 `None` —— 与 `sessions_dir` 一样，CLI 会照说，而不是凭空造一个目录。
pub fn goals_dir(env: &EnvMap) -> Option<PathBuf> {
    Some(data_dir(env)?.join("fs-agent").join("goals"))
}

/// `$XDG_DATA_HOME`，否则 `$HOME/.local/share`；都没有时是 `None`。
fn data_dir(env: &EnvMap) -> Option<PathBuf> {
    env.get("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env.get("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".local").join("share"))
        })
}

// --- 原始 TOML 形状 -----------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    default_model: Option<String>,
    #[serde(default)]
    providers: BTreeMap<String, RawProvider>,
    #[serde(default)]
    models: BTreeMap<String, RawModel>,
    /// `[pricing.<model-id>]`：一百万 token 多少钱（spec §17）。只作显示 ——
    /// 闸门数的是 token。
    #[serde(default)]
    pricing: BTreeMap<String, RawPricing>,
    /// `[permissions]`：会话开始时的那一档模式，也是这张表唯一的旋钮（spec §12）。
    permissions: Option<RawPermissions>,
    budget: Option<RawBudget>,
    routing: Option<RawRouting>,
    /// `[turn]`：回合上限（spec §3，story 11）。
    turn: Option<RawTurn>,
    /// `[discussion]`：谁参与辩论（spec §15）。
    discussion: Option<RawDiscussion>,
    /// `[tools.<namespace>.<tool>]`：动态声明的工具（spec §14）。
    #[serde(default)]
    tools: BTreeMap<String, BTreeMap<String, RawTool>>,
    /// `[sandbox]`：这一层包不包，以及额外哪些目录可写（spec §7）。
    sandbox: Option<RawSandbox>,
    /// `[goals]`：目标循环的两个阈值（`.scratch/goal-loop/spec.md` §6）。
    goals: Option<RawGoals>,
    /// `[ui]`：显示层的选择（`.scratch/usage-stats-format/spec.md` §2）。
    ui: Option<RawUi>,
}

/// 一张 `[goals]` 表：目标循环在窗口的哪个位置提醒、哪个位置翻页，以及两条停止线。
///
/// 前两个数是**窗口的百分比**（状态行那个 `上下文 n%` 的同一个数），不是预算 —— 预算
/// 是累计 token，跨会话认到目标上去（§8）；后两个是次数。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGoals {
    remind_at: Option<u8>,
    compact_at: Option<u8>,
    no_progress_rollovers: Option<u32>,
    provider_retries: Option<u32>,
}

/// 一张 `[ui]` 表：目前只有一个键，数字用哪套书写制式。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUi {
    /// `"cn"`（缺省，万 / 亿）或 `"si"`（k / M / G）。
    number_style: Option<String>,
}

/// 一张 `[sandbox]` 表。
///
/// 两个旋钮，只有两个：遮罩目录与保护路径是安全默认，写死在代码里，不该被人为了顺手改松。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSandbox {
    /// `"bwrap"`（缺省）或 `"off"`。
    mode: Option<String>,
    /// 除会话 cwd 之外可写的目录；`~` 会展开。缺省是 [`DEFAULT_SANDBOX_WRITABLE_ROOTS`]。
    writable_roots: Option<Vec<String>>,
}

/// 一张 `[tools.<namespace>.<tool>]` 表。
///
/// 字段就是线级声明本身：`description` 与 `parameters` 原样发送，而 `command` 是 argv
/// 模板，从来不是一段 shell 字符串。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTool {
    description: String,
    /// argv 模板。形如 `{name}` 的整个元素会被那个参数替换；参数缺席时它被省略。
    command: Vec<String>,
    /// JSON Schema，发送给 provider 时的样子。
    parameters: serde_json::Value,
    /// 可选的墙钟上限；缺省是 [`DEFAULT_CUSTOM_TOOL_TIMEOUT_MS`]，
    /// 并夹在 [`MAX_CUSTOM_TOOL_TIMEOUT_MS`] 之内。
    timeout_ms: Option<u64>,
}

/// 解析并校验每一条声明的工具。
///
/// 校验刻意放在启动期：一份声明点名了一个它永远收不到的参数，或者一个命名谓词没法重新解析
/// 出来的命名空间，都该在一个回合开始之前就失败，而不是等到模型第一次调用。
fn resolve_tools(
    raw: &BTreeMap<String, BTreeMap<String, RawTool>>,
) -> Result<Vec<ToolDeclaration>, ConfigError> {
    let mut declarations = Vec::new();
    for (namespace, tools) in raw {
        for (tool, declaration) in tools {
            declarations.push(resolve_tool(namespace, tool, declaration)?);
        }
    }
    // 稳定顺序：工具数组是缓存前缀的一部分。
    declarations.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(declarations)
}

fn resolve_tool(
    namespace: &str,
    tool: &str,
    raw: &RawTool,
) -> Result<ToolDeclaration, ConfigError> {
    for (label, part) in [("namespace", namespace), ("tool", tool)] {
        if part.is_empty()
            || !part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
            || part.contains(CUSTOM_TOOL_SEPARATOR)
        {
            return Err(ConfigError::InvalidTool {
                tool: custom_tool_name(namespace, tool),
                reason: format!(
                    "{label} `{part}` 必须非空，且只能用 ASCII 字母、数字、`-` 或 `_`\
                     （而且不能含 `__`，那是名字各部分之间的分隔符）"
                ),
            });
        }
    }
    if raw.command.is_empty() {
        return Err(ConfigError::InvalidTool {
            tool: custom_tool_name(namespace, tool),
            reason: "`command` 不能为空：它是真正要跑的那份 argv".to_owned(),
        });
    }

    // 程序名是唯一不能是占位符的那个元素：不然一次没有参数的调用就没有东西可执行。
    if parameter_placeholder(&raw.command[0]).is_some() {
        return Err(ConfigError::InvalidTool {
            tool: custom_tool_name(namespace, tool),
            reason: "`command` 的第一个元素是程序本身，必须是字面量，不能是 \
                     `{parameter}` 占位符"
                .to_owned(),
        });
    }

    let properties = raw
        .parameters
        .get("properties")
        .and_then(serde_json::Value::as_object);
    for element in &raw.command {
        let Some(name) = parameter_placeholder(element) else {
            continue;
        };
        let declared = properties.is_some_and(|properties| properties.contains_key(name));
        if !declared {
            return Err(ConfigError::InvalidTool {
                tool: custom_tool_name(namespace, tool),
                reason: format!(
                    "`{{{name}}}` 出现在 `command` 里，但参数 schema 在 \
                     `parameters.properties` 下没有声明 `{name}`"
                ),
            });
        }
    }

    Ok(ToolDeclaration {
        name: custom_tool_name(namespace, tool),
        namespace: namespace.to_owned(),
        tool: tool.to_owned(),
        description: raw.description.clone(),
        command: raw.command.clone(),
        parameters: raw.parameters.clone(),
        timeout_ms: raw
            .timeout_ms
            .unwrap_or(DEFAULT_CUSTOM_TOOL_TIMEOUT_MS)
            .clamp(1, MAX_CUSTOM_TOOL_TIMEOUT_MS),
    })
}

/// 当一个 argv 元素整体是 `{name}` 占位符时，它代表哪个参数名。别的都是字面量。
///
/// 替换以**整个 argv 元素**为单位（spec §14）：`--path={p}` 不是占位符，因为替换的单位是
/// 一个元素，而局部拼接正是 argv 形状漂移的来路。
///
/// crate 内可见，是因为真正替换 argv 的那个工具（[`crate::tools::CustomTool`]）必须与
/// 校验器用同样的方式解析一个元素，否则一份声明可能校验通过、替换时却换了个样子。
pub(crate) fn parameter_placeholder(element: &str) -> Option<&str> {
    let inner = element.strip_prefix('{')?.strip_suffix('}')?;
    if inner.is_empty() || inner.contains(['{', '}', ' ']) {
        return None;
    }
    Some(inner)
}

/// `[routing]` 表（spec §17）：更便宜的模型可被路由到的那两个落点。没有给讨论者的键，这
/// 正是重点。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRouting {
    synthesizer_model: Option<String>,
    executor_model: Option<String>,
}

/// `[discussion]` 表：哪两个模型参与辩论，以及协议最多能跑多远（spec §15）。
///
/// 名册住在自己那张表里而不是 `[routing]` 之下，因为两者说的是相反的事：routing 把一个
/// 参与者交给*更便宜*的模型，而讨论者是唯一永不被路由的参与者。把「谁参与辩论」留在
/// routing 表之外，正是让这件事成为结构性事实、而不是一条要记住的规则的原因。
///
/// 合成器**不**在这里：它是 routing 表里的另一个落点（`[routing].synthesizer_model`），
/// 而一个值只有一种拼写才是重点。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDiscussion {
    /// 讨论从哪个池子里抽它的两个讨论者，按配置顺序：要么只给一个 model id，要么是一个具名
    /// 的讨论者。
    debaters: Option<Vec<RawDebater>>,
    /// 可选的轮次上限；缺席表示用
    /// [`crate::discussion::DEFAULT_MAX_ROUNDS`]。
    max_rounds: Option<u32>,
}

/// 一条 `[[discussion.debaters]]` 条目。
///
/// 两种拼法，因为名字是*额外的*信息，而不是说模型的另一种方式：
/// `debaters = ["kimi-k3", "deepseek-v4-pro"]` 是短形式，适用于 model id 本身就够当名
/// 字的时候；而两个讨论者本来没法区分时（同一个模型来两次就是它为之存在的情况），用一张表
/// 补上名字。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawDebater {
    Model(String),
    Named(RawNamedDebater),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNamedDebater {
    /// 这个讨论者的身份：流上、转录里、以及模型可见的 `[轮 N · 名字]` 前缀里用的名字。
    name: String,
    /// 它作答所用的模型。
    model: String,
    /// 这个讨论者的**人物**，用用户的原话说：它扮演什么性格来辩论。它作为只发给这个讨论者
    /// 的注入落在流上（spec §15）。
    soul: Option<String>,
}

/// 一个讨论可以抽到的讨论者：它的**名字**，以及它作答所用的模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Debater {
    /// 这个参与者在流上的身份。同一场讨论的两个讨论者绝不能共用它：每一次投影都是
    /// `speaker_id` 的函数，所以一个名字会让每一方把对面的回答当成自己的（spec §5）。
    pub name: String,
    /// 这个讨论者作答所用的模型。
    pub model: String,
    /// 用户给这个讨论者写的**人物**（如果写了的话）：它扮演什么性格来辩论，用用户的原话。
    pub soul: Option<String>,
}

/// 解析好的 `[discussion]` 名册：讨论从中抽两个讨论者的那个**池子**（spec §15）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscussionRoster {
    /// 讨论可能抽到的每一个讨论者，按配置顺序。至少两个；多于两个意味着不同的讨论可以要不同
    /// 的对。
    pub debaters: Vec<Debater>,
    /// 轮次上限；`None` 用协议的缺省值。
    pub max_rounds: Option<u32>,
}

impl DiscussionRoster {
    /// 叫 `name` 的那个池子成员，如果有的话。
    pub fn debater(&self, name: &str) -> Option<&Debater> {
        self.debaters.iter().find(|debater| debater.name == name)
    }

    /// 每个讨论者的名字，按配置顺序 —— 错误消息里列的就是它。
    pub fn names(&self) -> Vec<&str> {
        self.debaters
            .iter()
            .map(|debater| debater.name.as_str())
            .collect()
    }
}

/// 讨论者名字的最大长度。名字在线上是一个 `name` 字段，在正文里是一个前缀，所以它短到两边
/// 都够用（投影自己那条上限在 [`crate::provider::projection`] 里，这里是配置侧的那条界）。
pub const MAX_DEBATER_NAME: usize = 32;

/// 讨论者 `soul` 的最大长度，按字符计。
///
/// 灵魂是一次注入，而注入是钉住的：它是这个讨论者参与的每一场讨论的每一轮的指令，所以它像
/// 别的钉住文本一样被限长（spec §9、§10），而不是放着让它吃掉窗口。
pub const MAX_DEBATER_SOUL: usize = 2000;

/// 一张 `[pricing.<model-id>]` 表，单位是每百万 token 的 USD。
///
/// 三个价格都是必需的：缺一个会静默把一类 token 定价成零，而「无价格」本来就有自己诚实的
/// 拼法（整张表都不写）。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPricing {
    /// 缓存未命中的输入 token。
    miss_input: f64,
    /// 缓存命中的输入 token。
    cached_input: f64,
    /// 输出 token，含推理。
    output: f64,
}

/// `[turn]` 表（spec §3，story 11）：一个 agent 的回合循环在 provider 调用上最多花多少，
/// 以及它派发的执行者换到的是什么（spec §16）。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTurn {
    /// 一个回合里 provider 调用的硬上限。
    max_iterations: Option<u32>,
    /// 一个**执行者**的回合里 provider 调用的硬上限（spec §16），独立于派发者的那一份来数。
    executor_max_iterations: Option<u32>,
}

/// `[permissions]` 表（spec §12；`.scratch/todo-and-modes/spec.md` §1、
/// `.scratch/workspace-mode/spec.md` §2）。
///
/// 两个字段：会话开始时的那一档模式，以及区外**读**那一条与档位正交的旋钮。其余的权限机制
/// —— 规则代数、断路器、`.env` 地板、路径限制 —— 都是权限门自己的缺省值而不是配置，所以
/// 这里没什么可说的。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPermissions {
    /// `readonly` | `ask` | `workspace` | `auto`；缺席表示 [`Mode::Ask`]。
    mode: Option<String>,
    /// `deny`（缺省）| `ask` | `allow`：读目标落在 cwd 之外时怎么办。
    outside_read: Option<String>,
}

/// 把 `[permissions]` 解析成会话开始时的那一档模式（spec §12）。
///
/// 一个不认识的词是启动错误，而不是静默回退到 `ask`：一份写着 `mode = "plan"` 的配置
/// （这张表曾经有过的第四档）否则会以一个作者从没要过的权限立场把会话启动起来。
fn resolve_mode(raw: Option<&RawPermissions>) -> Result<Mode, ConfigError> {
    let Some(written) = raw.and_then(|raw| raw.mode.as_deref()) else {
        return Ok(Mode::Ask);
    };
    Mode::parse(written).ok_or_else(|| ConfigError::UnknownMode {
        mode: written.to_owned(),
    })
}

/// 把 `[permissions] outside_read` 解析成那条区外读裁决（缺省 `deny`）。
///
/// 同样是一个不认识的词就是启动错误：写错的人以为自己放开了区外读、而实际仍然拒着，比
/// 反过来更糟 —— 他会去别处找原因。缺省永远是 `deny`：这条地板是策略级的，
/// **写下来才算放弃**（`.scratch/workspace-mode/spec.md` §2）。
fn resolve_outside_read(raw: Option<&RawPermissions>) -> Result<Decision, ConfigError> {
    match raw.and_then(|raw| raw.outside_read.as_deref()) {
        None | Some("deny") => Ok(Decision::Deny),
        Some("ask") => Ok(Decision::Ask),
        Some("allow") => Ok(Decision::Allow),
        Some(other) => Err(ConfigError::UnknownOutsideRead {
            value: other.to_owned(),
        }),
    }
}

/// 把 `[sandbox]` 解析成组装期要用的那一份值（spec §7）。
///
/// 一个不认识的 `mode` 是启动错误，而不是静默回退到 `bwrap`：写错了模式名的人以为这层关着，
/// 而默认值恰恰是开着。
///
/// `~` 用配置解析拿到的那张环境表里的 `HOME` 展开；没有 `HOME` 时带 `~` 的项保持字面形式，
/// 拼装时因为「目标不存在」被跳过 —— 那与「没有这个目录」的结果一样。
fn resolve_sandbox(raw: Option<&RawSandbox>, env: &EnvMap) -> Result<SandboxSettings, ConfigError> {
    let mode = match raw.and_then(|raw| raw.mode.as_deref()) {
        None | Some("bwrap") => SandboxMode::Bwrap,
        Some("off") => SandboxMode::Off,
        Some(other) => {
            return Err(ConfigError::UnknownSandboxMode {
                mode: other.to_owned(),
            })
        }
    };
    let home = env
        .get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let roots = raw
        .and_then(|raw| raw.writable_roots.clone())
        .unwrap_or_else(|| {
            DEFAULT_SANDBOX_WRITABLE_ROOTS
                .iter()
                .map(|root| (*root).to_owned())
                .collect()
        });
    Ok(SandboxSettings {
        mode,
        writable_roots: roots
            .iter()
            .map(|root| expand_home(root, home.as_deref()))
            .collect(),
        masks: SANDBOX_MASKS
            .iter()
            .map(|mask| expand_home(mask, home.as_deref()))
            .collect(),
        availability: SandboxAvailability::Untested,
        search_path: env
            .get("PATH")
            .filter(|value| !value.is_empty())
            .map(OsString::from),
    })
}

/// 展开配置里写下的 `~` / `~/…`；别的写法原样返回。
///
/// crate 内可见：`[sandbox] writable_roots` 与升级申请里的路径都要同一个展开，而两份
/// `~` 处理迟早会漂开。
pub(crate) fn expand_home(raw: &str, home: Option<&Path>) -> PathBuf {
    if raw == "~" {
        return home
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(raw));
    }
    match raw.strip_prefix("~/") {
        Some(rest) => match home {
            Some(home) => home.join(rest),
            None => PathBuf::from(raw),
        },
        None => PathBuf::from(raw),
    }
}

/// `[budget]` 表（spec §17）。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBudget {
    /// 会话累计 token 的硬上限。
    session_tokens: Option<u64>,
    /// 预检宽容度，以剩余量的倍数表示（缺省 1.5）。
    estimate_margin: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    base_url: Option<String>,
    api_key: Option<String>,
    /// 密钥从哪个环境变量读，当它不是该厂商的缺省变量时。
    api_key_env: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModel {
    provider: Option<String>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    max_output_tokens: Option<u32>,
    reasoning_effort: Option<ReasoningEffort>,
}

/// 内置的模型条目：线级 model id -> provider profile 名。这里的每一个 id 也都必须存在于
/// 能力表中（有测试断言这一点）。
///
/// K3 系列刻意出现两次：`kimi-k3` 是开放平台的 id，而 `k3` / `k3-256k` 是同一个模型在
/// Kimi Code（coding plan）下的 id。
pub const BUILTIN_MODELS: &[(&str, &str)] = &[
    ("kimi-k3", "kimi"),
    ("k3", "kimi-code"),
    ("k3-256k", "kimi-code"),
    ("kimi-for-coding", "kimi-code"),
    ("kimi-for-coding-highspeed", "kimi-code"),
    ("deepseek-v4-pro", "deepseek"),
    ("deepseek-flash", "deepseek"),
];

fn resolve_providers(
    raw: &RawConfig,
    env: &EnvMap,
) -> Result<BTreeMap<String, ProviderProfile>, ConfigError> {
    let mut providers = BTreeMap::new();

    let mut names: Vec<String> = BUILTIN_PROVIDERS
        .iter()
        .map(|builtin| builtin.name.to_owned())
        .collect();
    for name in raw.providers.keys() {
        if !names.iter().any(|known| known == name) {
            names.push(name.clone());
        }
    }

    for name in names {
        let builtin = builtin_provider(&name);
        let section = raw.providers.get(&name);

        // base_url：config.toml > 导出的环境变量 > 内置缺省。
        let env_base_url = env
            .get(&format!("{}_BASE_URL", env_prefix(&name)))
            .filter(|value| !value.is_empty())
            .cloned();
        let base_url = section
            .and_then(|section| section.base_url.clone())
            .or(env_base_url)
            .or_else(|| builtin.map(|builtin| builtin.base_url.to_owned()))
            .ok_or_else(|| ConfigError::ProviderWithoutBaseUrl {
                provider: name.clone(),
            })?;

        // key：config.toml > 导出的环境变量 > 内置缺省。内置 profile 还会认
        // 自己的备用环境变量拼法。
        let (api_key, key_source, key_env) = resolve_key(&name, section, builtin, env);

        // 从结构上拦住跨厂商的 401：一个来自环境变量的厂商密钥只允许指向该厂商的
        // 主机。说话的是密钥的出身，不是段名 —— `[providers.kimi]` 配上
        // `api_key_env = "DEEPSEEK_API_KEY"` 就是一个 DeepSeek 密钥。
        let host = host_of(&base_url).ok_or_else(|| ConfigError::InvalidBaseUrl {
            provider: name.clone(),
            base_url: base_url.clone(),
        })?;
        let key_vendor = key_source
            .env_var()
            .and_then(vendor_of_key_env)
            .or_else(|| builtin.map(|builtin| builtin.vendor));
        if let (Some(vendor), KeySource::Env(key_env)) = (key_vendor, &key_source) {
            if !vendor.hosts().contains(&host.as_str()) {
                return Err(ConfigError::CrossVendorKey {
                    provider: name.clone(),
                    host,
                    key_env: key_env.clone(),
                    vendor: vendor.as_str(),
                    expected: vendor.hosts().join(" or "),
                });
            }
        }

        providers.insert(
            name.clone(),
            ProviderProfile {
                name,
                base_url,
                api_key,
                key_source,
                key_env,
                vendor: builtin.map(|builtin| builtin.vendor),
            },
        );
    }

    Ok(providers)
}

/// 返回 `(key, source, recommended_env_var)`。哪怕密钥缺失也会点名推荐的那个变量，这样
/// 错误消息能准确告诉用户该导出什么。
fn resolve_key(
    name: &str,
    section: Option<&RawProvider>,
    builtin: Option<&BuiltinProvider>,
    env: &EnvMap,
) -> (Option<String>, KeySource, String) {
    let recommended = section
        .and_then(|section| section.api_key_env.clone())
        .or_else(|| builtin.map(|builtin| builtin.key_env.to_owned()))
        .unwrap_or_else(|| format!("{}_API_KEY", env_prefix(name)));

    if let Some(key) = section.and_then(|section| section.api_key.clone()) {
        if !key.is_empty() {
            return (Some(key), KeySource::Config, recommended);
        }
    }

    let mut candidates: Vec<String> = Vec::new();
    if let Some(explicit) = section.and_then(|section| section.api_key_env.clone()) {
        candidates.push(explicit);
    } else if let Some(builtin) = builtin {
        candidates.extend(builtin.key_envs().into_iter().map(str::to_owned));
    } else {
        candidates.push(recommended.clone());
    }

    for candidate in candidates {
        if let Some(value) = env.get(&candidate).filter(|value| !value.is_empty()) {
            return (Some(value.clone()), KeySource::Env(candidate), recommended);
        }
    }
    (None, KeySource::Missing, recommended)
}

fn resolve_models(
    raw: &RawConfig,
    providers: &BTreeMap<String, ProviderProfile>,
) -> Result<BTreeMap<String, ModelProfile>, ConfigError> {
    let mut models = BTreeMap::new();

    for (id, provider) in BUILTIN_MODELS {
        let section = raw.models.get(*id);
        models.insert(
            (*id).to_owned(),
            ModelProfile {
                id: (*id).to_owned(),
                // 内置 id 仍然可以被重新指向一个自定义 provider（代理，比如）；
                // 没有覆盖时它保留自己的厂商。
                provider: section
                    .and_then(|section| section.provider.clone())
                    .unwrap_or_else(|| (*provider).to_owned()),
                params: params_of(section),
            },
        );
    }

    for (id, section) in &raw.models {
        if models.contains_key(id) {
            continue;
        }
        let provider = section
            .provider
            .clone()
            .ok_or_else(|| ConfigError::ModelWithoutProvider { model: id.clone() })?;
        models.insert(
            id.clone(),
            ModelProfile {
                id: id.clone(),
                provider,
                params: params_of(Some(section)),
            },
        );
    }

    for model in models.values() {
        if !providers.contains_key(&model.provider) {
            return Err(ConfigError::UnknownProvider {
                model: model.id.clone(),
                provider: model.provider.clone(),
            });
        }
    }

    Ok(models)
}

/// 把 `[pricing.*]` 解析成显示用的价目表（spec §17）。
///
/// 给一个没有配置的模型定价是启动错误：这里的键是 model id，在那里打错一个字会静默地把每一
/// 笔费用变成「未知」。
fn resolve_pricing(
    raw: &RawConfig,
    models: &BTreeMap<String, ModelProfile>,
) -> Result<PriceTable, ConfigError> {
    let mut table = PriceTable::new();
    for (model, price) in &raw.pricing {
        if !models.contains_key(model) {
            return Err(ConfigError::UnknownPricedModel {
                model: model.clone(),
            });
        }
        for (field, value) in [
            ("miss_input", price.miss_input),
            ("cached_input", price.cached_input),
            ("output", price.output),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(ConfigError::InvalidPrice {
                    model: model.clone(),
                    field,
                });
            }
        }
        table.set(
            model,
            Pricing::new(price.miss_input, price.cached_input, price.output),
        );
    }
    Ok(table)
}

/// 把 `[budget]` 解析成会话的额度（spec §17）。
fn resolve_budget(raw: Option<&RawBudget>) -> Result<Budget, ConfigError> {
    let mut budget = Budget::new();
    let Some(raw) = raw else {
        return Ok(budget);
    };
    if let Some(tokens) = raw.session_tokens {
        budget = budget.with_limit(tokens);
    }
    if let Some(margin) = raw.estimate_margin {
        if !margin.is_finite() || margin <= 0.0 {
            return Err(ConfigError::InvalidBudget {
                reason: format!("estimate_margin 必须是正的倍数，拿到的是 {margin}"),
            });
        }
        budget = budget.with_estimate_margin(margin);
    }
    Ok(budget)
}

/// 把 `[routing]` 解析成两个落点的覆盖（spec §17）。
///
/// 被路由到的模型必须是配置过的，与价格的键同理：打错一个字会静默地把这个参与者留在讨论的
/// 模型上。至于它是不是**派发者的 provider** 能服务的模型，在构造执行者的地方检查
/// （spec §16），因为那取决于组装时选定的模型，而不是这个文件。
fn resolve_routing(
    raw: Option<&RawRouting>,
    models: &BTreeMap<String, ModelProfile>,
) -> Result<Routing, ConfigError> {
    let Some(raw) = raw else {
        return Ok(Routing::default());
    };
    for model in [&raw.synthesizer_model, &raw.executor_model]
        .into_iter()
        .flatten()
    {
        if !models.contains_key(model) {
            return Err(ConfigError::UnknownRoutedModel {
                model: model.clone(),
            });
        }
    }
    Ok(Routing {
        synthesizer_model: raw.synthesizer_model.clone(),
        executor_model: raw.executor_model.clone(),
    })
}

/// 把 `[discussion]` 解析成讨论从中抽取的池子（spec §15）。
///
/// 是**池子**，不是一对：一场讨论恰好跑两个讨论者，但抽哪两个是逐场讨论决定的 —— 命令行上
/// 点名，或者什么都没点名时从池子里随机抽。这里拒绝的是服务不了这件事的池子：成员少于两个、
/// 模型没配置过、名字当不了身份（空、带空格、过长），以及两个成员共用一个名字。
///
/// **来自同一家厂商的两个讨论者 —— 哪怕同一个模型来两次 —— 是允许的。** 设计假定讨论者是
/// 异构的，但一个到期的订阅不该让讨论跑不起来；而调用同一个模型两次，在采样有差异时依然会
/// 不一致。这件事针对真正在辩论的那一对来判定（[`Config::debaters_share_a_vendor`]），这样
/// 前端能在多样性弱于设计预期时说出来。
fn resolve_discussion(
    raw: Option<&RawDiscussion>,
    models: &BTreeMap<String, ModelProfile>,
) -> Result<Option<DiscussionRoster>, ConfigError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let Some(entries) = raw.debaters.as_ref() else {
        return Err(ConfigError::InvalidDiscussion {
            reason: "需要 `debaters`：讨论从这里抽两位讨论者，写法是 \
                     `debaters = [\"模型\", \"另一个模型\"]`，或者带 `name` 与 `model` 的 \
                     `[[discussion.debaters]]` 表"
                .to_owned(),
        });
    };
    if entries.len() < crate::discussion::DEBATERS {
        return Err(ConfigError::InvalidDiscussion {
            reason: format!(
                "`debaters` 只点名了 {} 个池中成员；一次讨论固定跑两位\
                 （spec §15），所以池子至少要两个",
                entries.len()
            ),
        });
    }
    let mut debaters: Vec<Debater> = Vec::with_capacity(entries.len());
    for entry in entries {
        let (name, model, soul) = match entry {
            RawDebater::Model(model) => (model.clone(), model.clone(), None),
            RawDebater::Named(named) => (
                named.name.clone(),
                named.model.clone(),
                named.soul.clone().map(|soul| soul.trim().to_owned()),
            ),
        };
        if !models.contains_key(&model) {
            return Err(ConfigError::InvalidDiscussion {
                reason: format!(
                    "`debaters` 点名的 model `{model}` 没有配置；用内置 id，\
                     或者先在 [models.{model}] 下声明它"
                ),
            });
        }
        // 名字是模型被告知的身份，也是投影写进一行前缀里的东西，所以它必须是一个不断开的
        // 词。
        if name.is_empty()
            || name.chars().any(char::is_whitespace)
            || name.chars().any(char::is_control)
        {
            return Err(ConfigError::InvalidDiscussion {
                reason: format!(
                    "`{name}` 不能当讨论者的名字：名字是一个不断开的词\
                     （它是这个参与者的身份，而投影会把它写进 `[轮 N · 名字]` 前缀）"
                ),
            });
        }
        if name.chars().count() > MAX_DEBATER_NAME {
            return Err(ConfigError::InvalidDiscussion {
                reason: format!(
                    "`{name}` 超过 {MAX_DEBATER_NAME} 个字符；讨论者的名字是模型\
                     在每一条轮次前缀里都会读到的身份"
                ),
            });
        }
        // 灵魂在整场讨论里钉住、从不裁剪，所以它和别的钉住文本一样被限长（spec §10）。
        if let Some(soul) = &soul {
            if soul.is_empty() {
                return Err(ConfigError::InvalidDiscussion {
                    reason: format!(
                        "`{name}`：空的 `soul` 什么都没说；去掉这个字段，或者写下\
                         这个讨论者是什么样的人"
                    ),
                });
            }
            if soul.chars().count() > MAX_DEBATER_SOUL {
                return Err(ConfigError::InvalidDiscussion {
                    reason: format!(
                        "`{name}`：`soul` 超过 {MAX_DEBATER_SOUL} 个字符；它是每一轮\
                         都钉住的上下文，所以有限长"
                    ),
                });
            }
        }
        debaters.push(Debater { name, model, soul });
    }

    // 同一场讨论的两个讨论者就是两个参与者，而每一次投影都是 `speaker_id` 的函数
    // （spec §5）—— 所以池子里的名字必须唯一。这里抓住的情况是同一个模型的两个成员都没给
    // 名字：model id 就是名字，而一个 model id 当不了两个身份。
    for (index, debater) in debaters.iter().enumerate() {
        if debaters[..index]
            .iter()
            .any(|earlier| earlier.name == debater.name)
        {
            return Err(ConfigError::InvalidDiscussion {
                reason: format!(
                    "两位讨论者都叫 `{}`；给其中一个起个名字 —— \
                     `{{ name = \"甲\", model = \"{}\" }}` —— 因为名字是参与者\
                     在流上的身份",
                    debater.name, debater.model
                ),
            });
        }
    }

    if raw.max_rounds == Some(0) {
        return Err(ConfigError::InvalidDiscussion {
            reason: "`max_rounds` 至少是 1：一轮都没有的讨论没有东西可合成".to_owned(),
        });
    }
    Ok(Some(DiscussionRoster {
        debaters,
        max_rounds: raw.max_rounds,
    }))
}

/// 这两个讨论者是否已知是同一家厂商的判断来了两次。
///
/// 信号是 provider profile 归类出的 [`Vendor`]。表里没归类过的一条 profile（用户自己声明
/// 的）改按它指向的主机比较，与跨厂商密钥守卫读的是同一个信号。
fn same_vendor(
    models: &BTreeMap<String, ModelProfile>,
    providers: &BTreeMap<String, ProviderProfile>,
    first: &str,
    second: &str,
) -> bool {
    let profile = |model: &str| providers.get(&models.get(model)?.provider);
    let (Some(a), Some(b)) = (profile(first), profile(second)) else {
        return false;
    };
    match (a.vendor, b.vendor) {
        (Some(one), Some(other)) => one == other,
        (None, _) | (_, None) => a.name == b.name || host_of(&a.base_url) == host_of(&b.base_url),
    }
}

fn params_of(section: Option<&RawModel>) -> GenerationParams {
    match section {
        Some(section) => GenerationParams {
            temperature: section.temperature,
            top_p: section.top_p,
            max_output_tokens: section.max_output_tokens,
            reasoning_effort: section.reasoning_effort,
        },
        None => GenerationParams::default(),
    }
}

fn env_prefix(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// 一个密钥环境变量属于哪家厂商（如果有的话）。这让跨域守卫取决于密钥的出身，而不是段名。
fn vendor_of_key_env(name: &str) -> Option<Vendor> {
    BUILTIN_PROVIDERS
        .iter()
        .find(|builtin| builtin.key_envs().contains(&name))
        .map(|builtin| builtin.vendor)
}

fn host_of(base_url: &str) -> Option<String> {
    url::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
}

/// 配置失败。这里每一个都是启动错误：没有一个会静默降级。
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config.toml: {source}")]
    Parse {
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error("读不了 {path}：{source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("provider `{provider}` 既没有 `base_url`，也没有内置默认值")]
    ProviderWithoutBaseUrl { provider: String },
    #[error("provider `{provider}`：`{base_url}` 不是合法的 URL（base_url 需要 scheme 与 host）")]
    InvalidBaseUrl { provider: String, base_url: String },
    #[error(
        "model `{model}` 引用了未知的 provider `{provider}`；把它声明成 \
         [providers.{provider}]，或者把这个 model 指向一个已存在的 provider"
    )]
    UnknownProvider { model: String, provider: String },
    #[error("model `{model}` 没有 `provider`，也不是内置的 model id")]
    ModelWithoutProvider { model: String },
    #[error("未知的 model `{model}`；设置 `default_model`，或者用 --model 指定一个已配置的 model")]
    UnknownModel { model: String },
    #[error(
        "[pricing.{model}] 点名的 model 没有配置；价格按 model id 索引，所以用内置 id，\
         或者先在 [models.{model}] 下声明它"
    )]
    UnknownPricedModel { model: String },
    #[error(
        "[routing] 点名的 `{model}` 不是已配置的 model；routing 只能指向 [models.*] \
         或内置表里存在的 model id"
    )]
    UnknownRoutedModel { model: String },
    #[error("[pricing.{model}] {field}：价格是每百万 token 的美元数，不能为负")]
    InvalidPrice { model: String, field: &'static str },
    #[error("[budget] {reason}")]
    InvalidBudget { reason: String },
    #[error(
        "未知的模式 `{mode}`；`[permissions] mode`（或 `--mode`）只接 `readonly`、`ask`、`workspace`、`auto`"
    )]
    UnknownMode { mode: String },
    #[error(
        "未知的 outside_read 值 `{value}`；`[permissions] outside_read` 只接 `deny`（缺省）、`ask` 或 `allow`"
    )]
    UnknownOutsideRead { value: String },
    #[error("未知的沙箱模式 `{mode}`；`[sandbox] mode` 只接 `bwrap`（缺省）或 `off`")]
    UnknownSandboxMode { mode: String },
    #[error("未知的 number_style 值 `{value}`；`[ui] number_style` 只接 `cn`（缺省，万 / 亿）或 `si`（k / M / G）")]
    UnknownNumberStyle { value: String },
    #[error("[discussion] {reason}")]
    InvalidDiscussion { reason: String },
    #[error(
        "provider `{provider}`：base_url 的 host `{host}` 与密钥的出身对不上 \
         （`{key_env}` 是 {vendor} 的密钥，而 {vendor} 的密钥对应 {expected}）。\
         把密钥与另一家的 base_url 混用会返回 401。请把 `base_url` 指向 {expected}，\
         或者在 config.toml 里显式设置 `api_key` —— 如果你确实要这么配。"
    )]
    CrossVendorKey {
        provider: String,
        host: String,
        key_env: String,
        vendor: &'static str,
        expected: String,
    },
    #[error("工具 `{tool}`：{reason}")]
    InvalidTool { tool: String, reason: String },
    #[error("[goals] {reason}")]
    InvalidGoals { reason: String },
}

/// 注入给某一个 agent 回合循环的配置值。
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// provider 调用所用的模型标识。
    pub model: String,
    /// 一个回合里 provider 调用的硬上限。
    pub max_iterations: u32,
    /// 生成参数，含钉住的推理档位。
    pub params: GenerationParams,
    /// 单条工具结果的上限，按估计 token 计。过大的结果在进流之前就被裁剪（spec §10）。
    pub max_tool_result_tokens: u64,
    /// 仓库地图的上限，按估计 token 计（spec §9）。固定预算，从不是模型给的参数；
    /// [`MAX_REPO_MAP_TOKENS`] 是上限。
    pub repo_map_tokens: u64,
    /// 单次 `bash` 调用的墙钟上限，按毫秒计（spec §7）。模型不传 `timeout_ms` 时用它；
    /// `bash` 是 `Exclusive`，所以这同时也是一条命令能攥着工作区级锁的最长时间。
    pub bash_timeout_ms: u64,
    /// 模型所给 `bash` `timeout_ms` 的硬上限（spec §7）：一次调用可以要得比缺省更短，永远
    /// 要不到更长。
    pub max_bash_timeout_ms: u64,
    /// 这个 agent 派发的执行者的回合上限（spec §16）。独立于
    /// [`SessionConfig::max_iterations`] —— 「失控的执行者绝不能吃掉会话的回合」是需求，
    /// 不是优化 —— 而它花掉的 token 仍然计进会话总数。
    pub executor_max_iterations: u32,
    /// 执行者作答所用的模型（spec §16、§17）。`None` 继承派发者的模型，这是缺省、也是 v1
    /// 的行为；那个覆盖是 §17 留在原地的机制，等到有数据可依时用来把活路由给更便宜的模型。
    ///
    /// **client** 在这里不可覆盖：执行者在它派发者的 provider 上作答，所以这里点名的必须是
    /// 那个 client 能服务的模型（同一个 provider profile 下的模型）。给执行者换 provider 是
    /// 组装期的决定，不是按会话的值。
    ///
    /// 更便宜的模型可被路由到的两个**落点**之一；通过 [`SessionConfig::model_for`] 读它，
    /// 那是路由规则唯一的家。
    pub executor_model: Option<String>,
    /// 一批里可以同时跑多少个执行者（spec §16）。
    pub max_parallel_executors: usize,
    /// 合成器作答所用的模型（spec §17）。`None` 继承讨论的模型，这是缺省、也是 v1 的行为。
    ///
    /// 另一个落点。**讨论者绝不被路由** —— 没有那个字段，也没有任何会去读它的调用点。
    pub synthesizer_model: Option<String>,
    /// 每个模型一百万 token 多少钱，作**显示**用（spec §17）。闸门数的是 token；这张表
    /// 从不决定任何事。
    pub pricing: PriceTable,
    /// 会话的累计 token 额度（spec §17）。同一条流上的每个参与者共用它，这一点由
    /// `assemble_discussion` 强制。
    pub budget: Budget,
    /// 这个会话在每条事件被追加之前要从它里面打掉的那些值（spec §20）。这是同一条流上每个
    /// agent 共用的会话级事实，带在这里是因为 [`Config::session_config`] 正是配置变成注入值
    /// 的那一处，于是用它的组装路径都不必额外传东西。讨论还更进一步：它会**拒绝**打码器互相
    /// 不一致的名册，因为一条流对「什么算秘密」给出两个答案，就会有些事件被打码、有些没有。
    pub redactor: Redactor,
    /// 沙箱那一层的配置与探测结果（沙箱 spec §7）。配置在解析期填好，探测结果由 `lib` 组装期
    /// 填进来 —— 于是工具的上下文里带着的是一个定下来的值，而不是一件每次调用都要问的事。
    pub sandbox: SandboxSettings,
    /// 这个 agent 接手之前，**别处**已经花掉、而这个额度要一起数的 token
    /// （`.scratch/goal-loop/spec.md` §8）。
    ///
    /// 闸门的求和取自整条流，而每条流只看得见自己 —— 一个目标跨过的那些更早的会话不在里面。
    /// 这个数就是那个缺口：循环按归属算出它，填进来，于是翻页开的新会话带着同一个累计继续，
    /// **翻页不重置额度**。它是值，不是状态文件：与日账本一样，跨会话的账是派生的。
    pub carried_tokens: u64,
}

impl SessionConfig {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
            params: GenerationParams::default(),
            max_tool_result_tokens: DEFAULT_MAX_TOOL_RESULT_TOKENS,
            repo_map_tokens: DEFAULT_REPO_MAP_TOKENS,
            bash_timeout_ms: DEFAULT_BASH_TIMEOUT_MS,
            max_bash_timeout_ms: MAX_BASH_TIMEOUT_MS,
            executor_max_iterations: DEFAULT_EXECUTOR_MAX_ITERATIONS,
            executor_model: None,
            max_parallel_executors: DEFAULT_MAX_PARALLEL_EXECUTORS,
            synthesizer_model: None,
            pricing: PriceTable::new(),
            budget: Budget::new(),
            redactor: Redactor::default(),
            sandbox: SandboxSettings::off(),
            carried_tokens: 0,
        }
    }

    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// 覆盖执行者跑着的那个回合上限（spec §16）。
    pub fn with_executor_max_iterations(mut self, executor_max_iterations: u32) -> Self {
        self.executor_max_iterations = executor_max_iterations;
        self
    }

    /// 覆盖一批里可以同时跑多少个执行者（spec §16）。
    pub fn with_max_parallel_executors(mut self, max_parallel_executors: usize) -> Self {
        self.max_parallel_executors = max_parallel_executors.max(1);
        self
    }

    /// 把这个 agent 的执行者路由到另一个模型（spec §16、§17）。
    pub fn with_executor_model(mut self, executor_model: impl Into<String>) -> Self {
        self.executor_model = Some(executor_model.into());
        self
    }

    /// 覆盖单条结果的裁剪上限（spec §10）。
    pub fn with_max_tool_result_tokens(mut self, max_tool_result_tokens: u64) -> Self {
        self.max_tool_result_tokens = max_tool_result_tokens;
        self
    }

    /// 给这个会话一份打码值，在有任何东西被追加到流上之前就用它（spec §20）。
    pub fn with_redactor(mut self, redactor: Redactor) -> Self {
        self.redactor = redactor;
        self
    }

    /// 设置仓库地图预算，夹在 [`MAX_REPO_MAP_TOKENS`] 之内（spec §9）：没有任何配置能让
    /// 一次 `repo_map` 调用无界。
    pub fn with_repo_map_tokens(mut self, repo_map_tokens: u64) -> Self {
        self.repo_map_tokens = repo_map_tokens.min(MAX_REPO_MAP_TOKENS);
        self
    }

    /// 设置模型不传 `timeout_ms` 时 `bash` 调用的墙钟上限（spec §7）。永远不会高过天花板，
    /// 所以配置出来的缺省活不过 [`MAX_BASH_TIMEOUT_MS`]。
    pub fn with_bash_timeout_ms(mut self, bash_timeout_ms: u64) -> Self {
        // 天花板在构造上至少是 1（它自己的 builder 会把它垫起来），所以剩下的唯一一次夹取
        // 是对着配置出来的天花板。
        self.bash_timeout_ms = bash_timeout_ms.clamp(1, self.max_bash_timeout_ms);
        self
    }

    /// 设置模型所给 `bash` `timeout_ms` 的天花板（spec §7），新的天花板更低时把缺省一起
    /// 拉下来。
    pub fn with_max_bash_timeout_ms(mut self, max_bash_timeout_ms: u64) -> Self {
        self.max_bash_timeout_ms = max_bash_timeout_ms.max(1);
        self.bash_timeout_ms = self.bash_timeout_ms.min(self.max_bash_timeout_ms);
        self
    }

    pub fn with_params(mut self, params: GenerationParams) -> Self {
        self.params = params;
        self
    }

    /// 把推理档位在整个会话里钉住（spec §4）。
    pub fn with_reasoning_effort(mut self, effort: ReasoningEffort) -> Self {
        self.params.reasoning_effort = Some(effort);
        self
    }

    /// 把合成器那一次收尾调用路由到另一个模型（spec §17）。
    pub fn with_synthesizer_model(mut self, synthesizer_model: impl Into<String>) -> Self {
        self.synthesizer_model = Some(synthesizer_model.into());
        self
    }

    /// 带上这个会话用来显示花费的价目表（spec §17）。
    pub fn with_pricing(mut self, pricing: PriceTable) -> Self {
        self.pricing = pricing;
        self
    }

    /// 带上「别处已经花掉的 token」（`.scratch/goal-loop/spec.md` §8）。
    pub fn with_carried_tokens(mut self, tokens: u64) -> Self {
        self.carried_tokens = tokens;
        self
    }

    /// 给这个会话它的累计 token 额度（spec §17）。
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// 给会话的累计 token 封顶，保留缺省的宽容度。
    pub fn with_session_token_limit(mut self, tokens: u64) -> Self {
        self.budget = self.budget.with_limit(tokens);
        self
    }

    /// 设置预检宽容度，以剩余量的倍数表示（spec §17）。
    pub fn with_estimate_margin(mut self, margin: f64) -> Self {
        self.budget = self.budget.with_estimate_margin(margin);
        self
    }

    /// `point` 作答所用的模型：配置里的覆盖，否则是本配置已经带着的那个模型（spec §17）。
    ///
    /// 这是全系统**唯一**一条路由规则，而且恰好只从两个落点调用 —— 执行者端口，以及合成器
    /// 的会话。讨论者绝不被路由：它用 [`SessionConfig::model`] 作答，没有任何路由调用能挪动
    /// 它。
    pub fn model_for(&self, point: LandingPoint) -> &str {
        let override_model = match point {
            LandingPoint::Synthesizer => self.synthesizer_model.as_deref(),
            LandingPoint::Executor => self.executor_model.as_deref(),
        };
        override_model.unwrap_or(&self.model)
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
            params: GenerationParams::default(),
            max_tool_result_tokens: DEFAULT_MAX_TOOL_RESULT_TOKENS,
            repo_map_tokens: DEFAULT_REPO_MAP_TOKENS,
            bash_timeout_ms: DEFAULT_BASH_TIMEOUT_MS,
            max_bash_timeout_ms: MAX_BASH_TIMEOUT_MS,
            executor_max_iterations: DEFAULT_EXECUTOR_MAX_ITERATIONS,
            executor_model: None,
            max_parallel_executors: DEFAULT_MAX_PARALLEL_EXECUTORS,
            synthesizer_model: None,
            pricing: PriceTable::new(),
            budget: Budget::new(),
            redactor: Redactor::default(),
            sandbox: SandboxSettings::off(),
            carried_tokens: 0,
        }
    }
}
