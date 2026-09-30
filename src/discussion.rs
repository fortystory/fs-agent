//! 讨论协议边界（spec §15）。
//!
//! 这个模块是协议的**规则**：关于两个讨论者是否一致的机械裁决、谁参加了某一轮、一个轮被允许怎么
//! 收尾，以及协议由之构成的那些私有指令与合成器提示词。它是策略，而且是纯的。
//!
//! 施加这些规则的**控制流** —— 轮的循环、那两次并发的回合、收尾那次调用 —— 住在 `agent` 层，因为
//! 那一层是事件流的唯一写入者（经它唯一那条 `append_event` 路径）与 provider 的唯一调用方
//! （spec §3）。所以 `discussion` 永不碰 `provider`：它判断，`agent` 写。

pub mod protocol;

use std::collections::BTreeSet;

use crate::events::{Event, EventPayload, RoundMode, StopReason};
use protocol::round_attendance;

/// v1 跑几个讨论者。
///
/// 固定为两个，并在组装期强制：那个机械裁决是一对一的比较，而 N > 2 会重新打开「N = 2 不仲裁」那条
/// 决定（spec §15，Out of Scope）。
pub const DEBATERS: usize = 2;

/// 轮的条数上限：一轮独立轮，然后一轮定向轮。
pub const DEFAULT_MAX_ROUNDS: u32 = 2;

/// 一个辩论轮之后协议做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundPlan {
    /// 停止辩论，并以这个原因结束这一轮。
    Stop(StopReason),
    /// 开一轮定向轮：每一方看到对方的作答，并被要求回应分歧。
    TargetedRound,
}

/// 施加轮次规则：辩论到此停下吗，以哪个原因停？
///
/// spec 为 `RoundEnded` 定死的四个原因，恰好就是这个函数里那些 `Stop` 结果。`BudgetExhausted` 是
/// 那套词汇的第五个输入、归预算闸门（票 14）所有，它在问到这个问题*之前*就把辩论停掉。
pub fn plan_after_round(outcome: protocol::RoundOutcome, round: u32, max_rounds: u32) -> RoundPlan {
    use protocol::RoundOutcome;
    match outcome {
        // 定向轮之后的一致是收敛；第一轮里的一致意味着分歧从未出现过 —— 那是关于这场讨论的另一个
        // 事实，拿另一个原因。
        RoundOutcome::Agreed if round > 1 => RoundPlan::Stop(StopReason::Consensus),
        RoundOutcome::Agreed => RoundPlan::Stop(StopReason::NoDivergence),
        RoundOutcome::Diverged if round < max_rounds => RoundPlan::TargetedRound,
        RoundOutcome::Diverged => RoundPlan::Stop(StopReason::RoundsExhausted),
        // 少于两份作答：什么都没比较过，所以什么都没达成一致。这份缺席本来就以缺席那一方自己的
        // `TurnEnded { Error }` 在流上，所以这个原因才有资格说得这么少。
        RoundOutcome::Incomplete => RoundPlan::Stop(StopReason::NoDivergence),
    }
}

/// 一个讨论者的私有身份：协议指令。
///
/// 它住在这里 —— 不在流上 —— 因为否则「后一轮的 `messages` 能从流上重算」这条在这条指令一旦改变
/// 什么东西的那一刻就不成立了（spec §15）。
///
/// 它**整场讨论都不变**是有意的。定向轮那条规则在这里说一次，而不是逐轮注入：system prompt 是前缀的
/// 头部，在讨论中途重写它会让整个前缀缓存作废（spec §4）。讨论者处在哪一轮，从投影的
/// `[轮 N · 名字]` 前缀与对方揭示出来的作答里就能看到。
///
/// 末尾拼的是 [`crate::agent::THINKING_IN_CHINESE`]：讨论者的思考照样进流、进详情弹窗。
pub fn debater_identity(name: &str) -> String {
    format!(
        "你是本次讨论中的一位讨论者，代号「{name}」。同一次讨论里还有另一位讨论者，\
你们各自独立回答同一个问题，谁的判断都不从属于对方。\n\
\n\
作答规则：\n\
1. 先写出你的作答正文。\n\
2. 正文结束后另起一行，以 `{marker}` 开头写出**一行**结论。这一行是两个讨论者是否一致的\
唯一机械依据：必须是一行、要有信息量，不要写成整段，也不要在它之后再写任何内容。\n\
3. 如果这一轮你能看到另一位讨论者的作答，说明你们上一轮的结论冲突了：只针对分歧回应，\
不要复述对方的全文，并照常以一行结论收尾。\n\
4. 不要为了达成一致而改变判断，也不要替用户做最终决定或输出汇总——那由合成器负责。\n\
\n\
{rule}",
        name = name,
        marker = protocol::CONCLUSION_MARKER,
        rule = crate::agent::THINKING_IN_CHINESE,
    )
}

/// 一个人物的**灵魂**，框成这个讨论者以之作答的性格。
///
/// 描述由用户写；框法是一个常量，好让两边对「被告知了什么」有共识。它被记**在流上**（一条署名给
/// 这个讨论者的 `ContextInjected`），而不是折进私有身份，因为身份必须能从流上重算（spec §15）——
/// 也因为灵魂是用户写的文本，与项目规则一模一样，而项目规则本来就是这么走的。
pub fn persona_brief(soul: &str) -> String {
    format!(
        "你的性格设定（用户给的，整场讨论都照它来）：\n{}\n\
         保持这个性格，不要为了和对方一致而丢掉它，也不要替用户做最终决定。",
        soul.trim()
    )
}

/// 合成器的私有身份：画出选项空间，绝不收敛。
///
/// 末尾拼的是 [`crate::agent::THINKING_IN_CHINESE`]：合成器也有思考增量，而它只到渲染器、不进
/// 事件流（`src/render/tui.rs` 的 `DetailKind::Thinking`），所以在详情里被人读到的正是它。
pub fn synthesizer_identity() -> String {
    [
        "你是本次讨论的合成器。这是一次独立的单发调用：你不参与讨论、没有工具、不发表新观点。\n\
你的产出是把已有的回答整理成选项空间，供用户自己决定：\n\
1. 共识：双方都成立的部分，并写明各自的依据。\n\
2. 分歧：冲突点，以及**各自在什么前提下成立**。\n\
3. 未决：现有回答不足以判断的部分。\n\
不要收敛成一个答案，也不要放弃聚合——标出分叉点本身就是聚合。\n\
若有讨论者缺席，必须明确写出「该方缺席，只有一方作答」，绝不能把单方回答写成共识。\n\
只依据给出的作答正文，不要猜测任何未写出的推理过程。",
        "\n\n",
        crate::agent::THINKING_IN_CHINESE,
    ]
    .concat()
}

/// 合成器唯一的那条 user 消息：问题，然后是每一轮揭示出来的作答、以及每一次缺席。
///
/// 从流上派生、而不是从循环状态派生，所以「谁缺席了」与日志记下的是同一个事实。它揭示每个讨论者的
/// **发言**，绝不揭示它的私有推理（spec §15）：一份对推理的摘要等于 harness 替某一方改写论证。
///
/// `since_round` 把材料限制在**这一场**讨论内。一个会话可以承载不止一场讨论（`/discuss` 跑在活的
/// 流上），而轮次号接在流上已有的内容之后 —— 所以没有这条界，第二场讨论的合成器会拿到第一场的作答，
/// 并被要求把两场一起合成。
pub fn synthesis_prompt(question: &str, events: &[Event], since_round: u32) -> String {
    let mut prompt = String::from("问题：\n");
    prompt.push_str(question.trim());
    prompt.push('\n');

    for (round, mode) in debate_rounds(events)
        .into_iter()
        .filter(|(round, _)| *round >= since_round)
    {
        let attendance = round_attendance(events, round);
        prompt.push_str(&format!(
            "\n## 第 {round} 轮（{}）\n",
            crate::render::wording::round_mode(mode)
        ));
        for (speaker, answer) in &attendance.answers {
            prompt.push_str(&format!("\n### {speaker} 的作答\n{}\n", answer.trim()));
        }
        for speaker in &attendance.absent {
            prompt.push_str(&format!("\n### {speaker}\n这一轮没有作答（本轮缺席）。\n"));
        }
    }

    prompt.push_str("\n请按「共识 / 分歧（含各自成立的前提）/ 未决」三档输出。\n");
    prompt
}

/// 流上持有的最大轮次号；一个都没有时是零。
///
/// 一场讨论把它的轮次号编在**它所写入的那条流**之后，正是这一点让 `round` 在一个会话里唯一：
/// `/discuss` 可以在一个会话里跑两次，而 `RoundStarted { round }` 必须说得出它属于哪一场讨论，否则
/// 每一个关于轮次的查询（`round_attendance`、`sessions show --round N`）都会把它们混起来。
pub fn last_round(events: &[Event]) -> u32 {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::RoundStarted { round, .. } => Some(*round),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// 池子里两个不同的成员，由一个种子抽出，按池子顺序。
///
/// 池子存在是为了让不同的讨论能问不同的组合；抽取是*由种子确定的*，好让一个会话能靠钉住种子复现，
/// 也让测试能断言那个选择、而不是分布。池子完全撑不起一场讨论时（成员少于两个）是 `None`。
///
/// 按池子顺序，因为顺序决定了哪个讨论者替这场讨论记录。一个便宜的混合就够了：这挑的是哪两个模型来
/// 争论，不是一次密码学抽取，而两个下标不可能撞上，因为第二个是从剩下那 `len - 1` 个槽位里抽的。
pub fn pick_pair(len: usize, seed: u64) -> Option<(usize, usize)> {
    if len < 2 {
        return None;
    }
    let len = len as u64;
    let first = seed % len;
    let mut second = (seed / len) % (len - 1);
    if second >= first {
        second += 1;
    }
    let (first, second) = (first as usize, second as usize);
    Some(if first < second {
        (first, second)
    } else {
        (second, first)
    })
}

/// 一次位于 `synthesis_round` 的合成所收尾的那个辩论阶段的第一轮。
///
/// 这就是合成器的材料被限制在**一场**讨论里的方式。轮次号编在它们所写入的那条流之后（所以一个会话
/// 里第二次 `/discuss` 从第一场停下的地方接着数），这意味着「日志里的每一轮」不再等于「这场讨论的
/// 轮次」。
///
/// 这条规则是结构性的：一个辩论阶段是一串连续的轮，其中唯一可能的 `RoundEnded` 落在它的最后一轮上
/// —— 一个结束了辩论的轮收掉这个阶段（后面有没有合成都一样），而同一条流上下一个辩论轮开启一个新
/// 阶段。所以按顺序走过那些辩论轮，每一个「结束了某个阶段、且后面还跟着另一个辩论轮」的轮都开启一个
/// 阶段。
///
/// 实跑的现场与 `sessions replay` 都会问这个问题，所以「合成器收到了什么」与「一次重放重算出来的是
/// 什么」不可能漂开。
pub fn debate_phase_start(events: &[Event], synthesis_round: u32) -> u32 {
    let mut debate: Vec<u32> = Vec::new();
    let mut ended: BTreeSet<u32> = BTreeSet::new();
    for event in events {
        match &event.payload {
            EventPayload::RoundStarted { round, mode }
                if *mode != RoundMode::Synthesis && *round < synthesis_round =>
            {
                debate.push(*round);
            }
            EventPayload::RoundEnded { round, .. } if *round < synthesis_round => {
                ended.insert(*round);
            }
            _ => {}
        }
    }
    debate.sort_unstable();
    debate.dedup();
    let mut start = debate.first().copied().unwrap_or(1);
    for pair in debate.windows(2) {
        if ended.contains(&pair[0]) {
            start = pair[1];
        }
    }
    start
}

/// 某一方在分歧记录里的立场。
///
/// 就是它给出的那句结论。当它违反协议、什么都没给时，用它的答案第一行顶上：一份冲突绝不能把它两侧
/// 中的一侧从记录里悄悄丢掉。
pub fn position_of(answer: &str) -> String {
    match protocol::conclusion_of(answer) {
        Some(conclusion) => conclusion.to_owned(),
        None => first_non_empty_line(answer),
    }
}

/// 一次分歧是*关于什么*的：问题文本的第一非空行。
pub fn divergence_topic(question: &str) -> String {
    first_non_empty_line(question)
}

/// 一段文本的第一非空行，trim 过。
fn first_non_empty_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// 辩论实际跑过的那些轮，按顺序，形如 `(轮次, 模式)`。
///
/// 合成那一轮不是辩论轮：它是收尾的那次调用，所以它永不作为可合成的材料出现。
fn debate_rounds(events: &[Event]) -> Vec<(u32, RoundMode)> {
    let mut seen = BTreeSet::new();
    let mut rounds = Vec::new();
    for event in events {
        if let EventPayload::RoundStarted { round, mode } = &event.payload {
            if *mode != RoundMode::Synthesis && seen.insert(*round) {
                rounds.push((*round, *mode));
            }
        }
    }
    rounds
}
