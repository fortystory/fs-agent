//! 上下文边界：可用输入的核算、单条结果的裁剪，以及按预算驱动的丢弃（spec §10）。
//!
//! 两个机制，在两个不同的时刻，作用于两组不同的数据：
//!
//! * **单条结果的裁剪**发生在**事件被追加之前**：一条超大的工具结果溢出落盘，流上则携带一条预览
//!   加一个指针。指针只是增强 —— 溢出文件缺失会降级成内联预览，绝不失败。
//! * **超预算的丢弃**发生在 [`trim`] 里、在投影*之后*，而且是**只读的**：事件流永远不动，动的只
//!   是发给 provider 的 `messages`。丢弃顺序是固定的（spec §10）：旧的普通工具结果 → 旧的技能
//!   正文 → 旧的整轮 → 这一回合硬失败。
//!
//! 两者都是值上的纯函数，所以「一个 agent 的 `messages` 能从流 + 溢出文件 + 投影规则重算出来」
//! 成立，而窗口层不需要锁：当前预算是值，不是状态。
//!
//! 丢一条工具结果是**把它的正文替成桩**，而不是删掉那条消息：provider 要求每条 `tool_call` 恰好
//! 对应一条 `tool` 消息（spec §5），所以那条配对的 `tool` 消息必须带着更短的正文活下来。只有整轮
//! 丢弃才会删消息，而且它把一轮的助手消息与它的结果一起删掉，配对因此保持完整。
//!
//! [`skills`] 与 [`repo_map`] 是相邻的两个关切（spec §9）。技能保留描述清单、按需加载正文；已加载
//! 技能正文的聚合上限在这里强制，甚至早于窗口预算被看一眼。`repo_map` 为按需的 `repo_map` 工具
//! 抽取并排序工作区符号，它的产物与其他任何东西一样是一条工具结果。

pub mod repo_map;
pub mod skills;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::provider::capability::ModelCaps;
use crate::provider::Message;
use skills::MAX_LOADED_SKILL_TOKENS;

/// 为模型自己的输出预留的输入空间（spec §10）。
///
/// 预留量是 `min(20_000, max_output_tokens)`，所以输出上限低于 20k 的模型只预留它真能写出来的量。
pub const OUTPUT_RESERVE_TOKENS: u32 = 20_000;

/// v1 的粗略估计器：四个字符一个 token（spec §10）。
const CHARS_PER_TOKEN: usize = 4;

/// 内联预览比单条结果的上限小多少倍。
const PREVIEW_DIVISOR: u64 = 10;

/// 预览绝不缩到这个值以下，于是一条很小的上限也藏不住一条结果的形状。
const MIN_PREVIEW_CHARS: usize = 200;

/// 一条被丢掉的工具结果会替换成的正文。
///
/// 消息本身留下，因为线级契约把一条 `tool` 消息与一条 `tool_call` 配成一对（spec §5）；被丢掉的
/// 只有它的正文。
pub const DROPPED_TOOL_RESULT: &str =
    "[dropped: this old tool result was removed from the context to fit the budget]";

/// 项目规则文件，启动时读一次，作为第一条 `user` 消息注入（spec §10）。
pub const AGENTS_MD: &str = "AGENTS.md";

/// 一个 agent 的可用输入预算，按**它自己**的模型算出来（spec §10）。刻意没有会话级预算。
///
/// 用饱和运算是有意的：窗口很小的能力表也仍然有一个定义良好的预算，而不是在一次减法里 panic。
pub fn usable_input(caps: &ModelCaps) -> u64 {
    let reserve = caps.max_output_tokens.min(OUTPUT_RESERVE_TOKENS);
    u64::from(caps.context_window.saturating_sub(reserve))
}

/// 一个字符串的 v1 粗略估计：字符数 / 4 向上取整，所以任何非空文本至少花一个 token。
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(CHARS_PER_TOKEN as u64)
}

/// 一次已投影请求的估计大小。
pub fn estimate_messages_tokens(messages: &[Message]) -> u64 {
    messages.iter().map(estimate_message_tokens).sum()
}

fn estimate_message_tokens(message: &Message) -> u64 {
    match message {
        Message::System { content, .. } | Message::User { content, .. } => estimate_tokens(content),
        Message::Assistant {
            content,
            reasoning_content,
            tool_calls,
            ..
        } => {
            option_tokens(content.as_deref())
                + option_tokens(reasoning_content.as_deref())
                + tool_calls
                    .iter()
                    .map(|call| estimate_tokens(&call.name) + estimate_tokens(&call.arguments))
                    .sum::<u64>()
        }
        Message::Tool { content, .. } => estimate_tokens(content),
    }
}

fn option_tokens(text: Option<&str>) -> u64 {
    text.map(estimate_tokens).unwrap_or(0)
}

/// [`trim`] 读的丢弃策略。它是值，不是状态。
#[derive(Debug, Clone)]
pub struct TrimPolicy {
    /// 结果正文比普通工具结果更黏的那些工具名（spec §10：一份已加载的技能正文属于模型当前正在做
    /// 的工作）。票 08 挂上这里点名的那个 `skill` 工具。
    pub sticky_tool_names: Vec<String>,
    /// 一次请求里已加载技能正文的聚合上限（spec §9）。
    ///
    /// 与窗口无关：总量一超过它，最旧的那些正文就被替成桩，无论窗口预算怎么说。普通工具结果永远
    /// 不被这趟预扫碰到。
    pub loaded_skill_budget: u64,
}

impl Default for TrimPolicy {
    fn default() -> Self {
        Self {
            sticky_tool_names: vec![skills::SKILL_TOOL.to_owned()],
            loaded_skill_budget: MAX_LOADED_SKILL_TOKENS,
        }
    }
}

/// 一次裁剪为什么没能装进预算。
///
/// 这是「真的溢出」那个信号（spec §10）：每一个可丢的类别都耗尽了，这同时也是将来一次压缩会挂着
/// 的那个触发点。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TrimError {
    #[error(
        "context budget exceeded: ~{estimated} estimated tokens against a budget of {budget}, \
         and nothing droppable is left"
    )]
    OverBudget { budget: u64, estimated: u64 },
}

/// 把一次已投影的请求装进 `budget` 个 token，按固定顺序丢（spec §10）：旧的普通工具结果，然后
/// 旧的技能正文，然后旧的整轮。此后仍然超预算，就是这一回合的硬失败。
///
/// 在做这些之前，先强制已加载技能的聚合预算（spec §9）：一旦它们总量超过
/// [`TrimPolicy::loaded_skill_budget`]，最旧的技能正文就被替成桩，哪怕窗口预算很宽裕。
///
/// 对事件流而言是只读的：它只重写交到手里的那个 `messages` 值。每一条被钉住的消息（私有身份，
/// 以及每一条 `ContextInjected`，无论它坐在哪）与当前正在组装的那个回合，永远不会被丢掉。
pub fn trim(
    mut messages: Vec<Message>,
    budget: u64,
    policy: &TrimPolicy,
) -> Result<Vec<Message>, TrimError> {
    // 技能正文预算与窗口丢弃顺序都要用这些名字。
    let names = tool_names(&messages);
    stub_skill_bodies_over_budget(&mut messages, &names, policy);

    if fits(&messages, budget) {
        return Ok(messages);
    }

    // 第 1、2 类：旧的工具结果正文，最旧的先走，普通结果在技能正文之前。同一种收缩，一次施加在
    // 一个黏性类别上 —— 正是这一点让顺序是严格的。
    for sticky in [false, true] {
        for index in old_result_indices(&messages, &names, policy, sticky) {
            if fits(&messages, budget) {
                return Ok(messages);
            }
            stub_tool_result(&mut messages[index]);
        }
    }

    // 第 3 类：旧的整轮，最旧的先走。当前回合与每一条被钉住的消息都留下：模型必须还拿着它正在
    // 回答的那个问题，以及交给它的那些 harness 内容（spec §10、§13）。
    while !fits(&messages, budget) {
        let starts = round_starts(&messages);
        if starts.len() <= 1 {
            break;
        }
        // 整轮被丢掉，除了它里面任何被钉住的消息 —— 一次会话中途的注入坐在一轮中间，会比那一轮
        // 活得久。至少有一条没被钉住的消息总会走（`starts` 的两个起点都是发言），所以这个循环一定
        // 有进展。
        let (from, to) = (starts[0], starts[1]);
        let mut index = 0;
        messages.retain(|message| {
            let in_round = index >= from && index < to;
            index += 1;
            !in_round || is_pinned(message)
        });
    }

    if fits(&messages, budget) {
        Ok(messages)
    } else {
        Err(TrimError::OverBudget {
            budget,
            estimated: estimate_messages_tokens(&messages),
        })
    }
}

fn fits(messages: &[Message], budget: u64) -> bool {
    estimate_messages_tokens(messages) <= budget
}

/// 一条消息是不是被钉住的：harness 内容，裁剪永不丢它，它也永不开启一个可丢弃的轮。
///
/// 私有身份按种类钉住（它也是第一条消息），而一条 `ContextInjected` 投影无论坐在哪都按种类钉住。
/// 钉住必须是消息自身的一个性质、而不是「开头连续多少条」这样一个长度，因为用户加载的一份
/// `Skill` 正文是在会话中途注入的，必须能在周围那些轮被丢掉时活下来（spec §9、§10）。
///
/// 一条普通的 `user` 消息 —— 一句发言，或者模型必须纠正的那个错误 —— 刻意*不*钉住：一条没有名字
/// 的普通消息仍然是一个轮的边界。
fn is_pinned(message: &Message) -> bool {
    match message {
        Message::System { .. } => true,
        Message::User { injected, .. } => *injected,
        _ => false,
    }
}

/// 一个轮从哪里开始的那些下标：每一条没被钉住的 `user` 消息。一个轮从它的起点跑到下一个起点
/// （或者末尾），所以丢掉整轮会把它的那些助手回合与它们的工具结果一起带走。
fn round_starts(messages: &[Message]) -> Vec<usize> {
    messages
        .iter()
        .enumerate()
        .filter(|(_, message)| matches!(message, Message::User { .. }) && !is_pinned(message))
        .map(|(index, _)| index)
        .collect()
}

/// 投影发出的每一次调用的 `tool_call_id -> 工具名`，用来把一条普通结果与一份技能正文区分开。持有
/// 所有权，这样裁剪在查这张表的同时还能改那些消息。
fn tool_names(messages: &[Message]) -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    for message in messages {
        if let Message::Assistant { tool_calls, .. } = message {
            for call in tool_calls {
                names.insert(call.id.clone(), call.name.clone());
            }
        }
    }
    names
}

/// 活的（还没被替成桩的）工具结果，最旧的先，到 `limit` 之前停。
///
/// 两个丢弃机制扫的是同一个形状；区别两者的就是 `limit`：窗口顺序在活跃轮处停（模型留着它正在
/// 回答的那个问题），而聚合技能预算扫整份请求。
///
/// 这次扫描自己不需要钉住判定：一条被钉住的消息要么是身份、要么是一次注入，两者都绝不会是 `tool`
/// 消息。
fn live_tool_indices(messages: &[Message], limit: usize) -> Vec<usize> {
    (0..limit.min(messages.len()))
        .filter(|&index| {
            matches!(&messages[index], Message::Tool { content, .. } if content.as_str() != DROPPED_TOOL_RESULT)
        })
        .collect()
}

/// 活跃的（最后一个）轮从哪里开始；没有可区分的轮时就是末尾。
fn active_round_start(messages: &[Message]) -> usize {
    let starts = round_starts(messages);
    starts.last().copied().unwrap_or(messages.len())
}

/// 某一个类别里那些旧的（非活跃轮的）工具结果，最旧的先。
fn old_result_indices(
    messages: &[Message],
    names: &BTreeMap<String, String>,
    policy: &TrimPolicy,
    sticky: bool,
) -> Vec<usize> {
    live_tool_indices(messages, active_round_start(messages))
        .into_iter()
        .filter(|&index| is_sticky_result(&messages[index], names, policy) == sticky)
        .collect()
}

/// 一条工具结果是否属于黏性类别：它的调用来自 [`TrimPolicy::sticky_tool_names`] 里点名的某个工具
/// （默认就是一份已加载的技能正文）。聚合技能预算与窗口丢弃顺序都靠它。
fn is_sticky_result(
    message: &Message,
    names: &BTreeMap<String, String>,
    policy: &TrimPolicy,
) -> bool {
    match message {
        Message::Tool { tool_call_id, .. } => names
            .get(tool_call_id.as_str())
            .is_some_and(|name| policy.sticky_tool_names.iter().any(|tool| tool == name)),
        _ => false,
    }
}

/// 独立于窗口地强制已加载技能的聚合预算（spec §9）：只要活的技能正文总量超过策略允许的值，就把
/// 最旧的替成桩。
///
/// 这是对整份请求（含当前回合）的一条上限：一个轮如果自己加载得比预算还多，它会丢掉自己最旧的
/// 正文（并且可能会重新加载一次）。它是一份与窗口分开的预算，所以它不等窗口溢出。
fn stub_skill_bodies_over_budget(
    messages: &mut [Message],
    names: &BTreeMap<String, String>,
    policy: &TrimPolicy,
) {
    let candidates: Vec<usize> = live_tool_indices(messages, messages.len())
        .into_iter()
        .filter(|&index| is_sticky_result(&messages[index], names, policy))
        .collect();

    let mut total: u64 = candidates
        .iter()
        .map(|&index| estimate_message_tokens(&messages[index]))
        .sum();
    for index in candidates {
        if total <= policy.loaded_skill_budget {
            break;
        }
        total -= estimate_message_tokens(&messages[index]);
        stub_tool_result(&mut messages[index]);
    }
}

fn stub_tool_result(message: &mut Message) {
    if let Message::Tool { content, .. } = message {
        *content = DROPPED_TOOL_RESULT.to_owned();
    }
}

/// 经流前裁剪流水线之后的一条工具结果（spec §10）：进事件的那段文本（一条预览加一个指针），以及
/// 全文落到了哪。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpilledResult {
    /// 流上携带的东西 —— 它自己永远是完整的。
    pub preview: String,
    /// 那个溢出文件，当它能被写出来时。
    pub pointer: Option<PathBuf>,
    /// 文本有没有超过上限。
    pub truncated: bool,
}

/// 在一条工具结果进流之前裁剪它：把溢出的部分落盘到 `<outputs_dir>/<tool_call_id>.txt`，并返回
/// 一条带头尾、携带那个指针的预览。
///
/// 绝不失败，也绝不让流变大：一条小到「预览加指针说明比正文本身还长」的正文会整个留下（它本来就
/// 已经小到撑不破窗口）。一次写不出去的溢出降级成「只有预览」（spec §11：一个死指针降级成预览）。
pub fn truncate_result(
    text: &str,
    tool_call_id: &str,
    outputs_dir: &Path,
    max_tokens: u64,
) -> SpilledResult {
    let total_chars = text.chars().count();
    if estimate_tokens(text) <= max_tokens {
        return SpilledResult {
            preview: text.to_owned(),
            pointer: None,
            truncated: false,
        };
    }

    let pointer = outputs_dir.join(format!("{tool_call_id}.txt"));
    let spilled = std::fs::create_dir_all(outputs_dir)
        .and_then(|()| crate::tools::paths::write_owner_only(&pointer, text.as_bytes()))
        .is_ok();
    let pointer = spilled.then_some(pointer);
    let preview = preview(text, max_tokens, pointer.as_deref());
    if preview.chars().count() >= total_chars {
        // 指针说明的代价比正文省下的还多：留着正文严格更好，而这条上限存在是为了给流设界，不是
        // 为了执行一个数字。
        return SpilledResult {
            preview: text.to_owned(),
            pointer: None,
            truncated: false,
        };
    }
    SpilledResult {
        preview,
        pointer,
        truncated: true,
    }
}

fn preview(text: &str, max_tokens: u64, pointer: Option<&Path>) -> String {
    let total_chars = text.chars().count();
    let total_tokens = estimate_tokens(text);
    let wanted = ((max_tokens / PREVIEW_DIVISOR) as usize)
        .saturating_mul(CHARS_PER_TOKEN)
        .max(MIN_PREVIEW_CHARS);
    // 让头与尾不会覆盖同一批字符。
    let preview_chars = wanted.min(total_chars.saturating_sub(1));
    let head_chars = preview_chars / 2;
    let tail_chars = preview_chars - head_chars;

    let head: String = text.chars().take(head_chars).collect();
    let mut tail: Vec<char> = text.chars().rev().take(tail_chars).collect();
    tail.reverse();
    let tail: String = tail.into_iter().collect();

    let note = match pointer {
        Some(path) => format!("full output at {}", path.display()),
        None => "full output could not be spilled to disk".to_owned(),
    };
    format!(
        "{head}\n{TRUNCATED_MARKER}{total_chars} chars, ~{total_tokens} tokens; {note}]\n{tail}"
    )
}

/// 一段被切开的结果正文在切口处携带的标记。
///
/// 是公开的，因为这是读完一个事件的人区分两种 `output` 的唯一办法：一条没被切的结果，它的预览
/// **就是**全文，而一条被切的有一条头、这个标记、一条尾 —— 并且只有被切的那种才有一个溢出文件
/// 可找。事件对两者只带一个字段（spec §11），所以这个标记就是那个判别依据。
pub const TRUNCATED_MARKER: &str = "[truncated: ";

/// 读项目的 `AGENTS.md`，如果它存在且不是空白。
///
/// 不存在不是错误：一个没有项目规则的仓库就是没有钉住的注入而已。读不出来的文件行为相同，而不是
/// 在会话开始之前就把它停掉。
pub fn load_agents_md(cwd: &Path) -> Option<String> {
    let text = std::fs::read_to_string(cwd.join(AGENTS_MD)).ok()?;
    (!text.trim().is_empty()).then_some(text)
}
