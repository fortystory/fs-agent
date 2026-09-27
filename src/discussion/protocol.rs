//! 讨论协议里纯的那一半（spec §15）。
//!
//! 「两个讨论者下的是不是同一个结论」这个机械裁决，是它们两份作答上的纯函数，而且必须保持是纯函数：
//! spec 的 Testing Decisions 把它列在「自己没有 mock 接缝」的那批函数里。有两条性质扛着这份重量：
//!
//! * **零次额外调用** —— 没有裁判、没有 `response_format`、没有受控的极性标签。那两句一行结论*就是*
//!   分歧记录。
//! * **不许假阴性** —— 用不同侧重、标点或大小写拼出来的相同结论，仍然必须比较相等，这就是为什么这次
//!   比较归一化得很重，并且还接受任一方向的子串包含。代价是一份有界的假阳性，而那个界就是轮的条数
//!   上限（spec §15）。
//!
//! 那个标记是一个共用常量，因为协议指令（讨论者的私有身份）与这个解析器绝不能漂：一个提示词教了、而
//! 解析器不认识的标记，会让每一轮都无声地变成有分歧。这与 hook 结果用的是同一个「约定文本配一个共用
//! 常量」的形状（spec §18）。

use crate::events::{Event, EventPayload, SpeakerId, StopReason};

/// 起始一个讨论者那一行自由结论的分隔符。
///
/// 刻意与语言无关：作答用什么语言写，取决于问题用什么语言问，所以这个分隔符不能是其中任何一种语言里
/// 的一个词。
pub const CONCLUSION_MARKER: &str = "CONCLUSION:";

/// 在一行结论里不承载任何命题重量的字符。
///
/// Markdown 强调、引号、括号与句读全都是同一个结论周围的装饰，所以它们中的任何一个都不许决定「是否
/// 一致」。
const IGNORED: &[char] = &[
    '*', '_', '`', '"', '\'', '“', '”', '‘', '’', '(', ')', '（', '）', '[', ']', '【', '】', '<',
    '>', '#', '-', '+', '~', '。', '．', '.', '，', ',', '、', '；', ';', '：', ':', '！', '!',
    '？', '?', '…', '—', '|',
];

/// 模型可能会把标记裹在里面、而并无它意的装饰：列表圆点、引用标记、强调、缩进。
const MARKER_LEAD: &[char] = &['*', '_', '`', '-', '+', '>', '#', ' ', '\t'];

/// 讨论者那一行自由结论：最后一条以 [`CONCLUSION_MARKER`] 开头、并且后面有东西的行。
///
/// 从末尾倒着扫，而不是要求这个标记必须在最后一行，是为了不让一句客套的收尾行抹掉模型*确实*写下了的
/// 那句结论。容忍标记周围的装饰，与比较归一化得很重是同一个理由：把 `**CONCLUSION:** x` 读成「没有
/// 结论」会是一次假阴性，而一次假阴性白白换来第二轮。一份真的没有标记的作答就是没有结论，协议必须把
/// 它当作「无法证明一致」，而不是当作一致（spec §15）。
pub fn conclusion_of(answer: &str) -> Option<&str> {
    answer
        .lines()
        .rev()
        .filter_map(|line| {
            line.trim()
                .trim_start_matches(MARKER_LEAD)
                .strip_prefix(CONCLUSION_MARKER)
                .map(|rest| rest.trim().trim_matches(IGNORED).trim())
        })
        .find(|rest| !rest.is_empty())
}

/// 把一句结论折成比较可以看的文本。
///
/// 空白折成单个空格，[`IGNORED`] 里的字符消失，其余的转小写。
pub fn normalize(conclusion: &str) -> String {
    let mut normalized = String::with_capacity(conclusion.len());
    let mut pending_space = false;
    for character in conclusion.chars() {
        if character.is_whitespace() {
            pending_space = !normalized.is_empty();
            continue;
        }
        if IGNORED.contains(&character) {
            continue;
        }
        if pending_space {
            normalized.push(' ');
            pending_space = false;
        }
        normalized.extend(character.to_lowercase());
    }
    normalized
}

/// 两份作答说的是不是同一个结论？
///
/// 精确相等，或者任一方向的子串包含。两侧都必须真的*有*结论：空的归一化结论是一切字符串的子串，
/// 所以没有这道守卫，一份没有标记的作答就会与会话里每一份作答都一致 —— 那正是 spec 在这一条路径上
/// 单独点名的、最危险的误读：「一份作答被读成共识」。
pub fn answers_agree(left: &str, right: &str) -> bool {
    let (Some(left), Some(right)) = (conclusion_of(left), conclusion_of(right)) else {
        return false;
    };
    let (left, right) = (normalize(left), normalize(right));
    if left.is_empty() || right.is_empty() {
        return false;
    }
    left == right || left.contains(&right) || right.contains(&left)
}

/// 一轮在流上的那一段说明了谁参加了。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoundAttendance {
    /// 落在这一轮里的讨论者作答，按 `seq` 顺序，带完整的揭示文本。私有推理不携带：协议揭示发言，
    /// 绝不揭示它背后的推理（spec §15）。
    pub answers: Vec<(SpeakerId, String)>,
    /// 这一轮里回合以 `Error` 收尾、**并且**没留下任何作答的那些讨论者。
    ///
    /// 这是 spec 说的缺席查询，从流上派生而不是存进字段。它要紧，因为另一种读法 ——「只回来了一份
    /// 作答，那它就是共识」—— 是这条路径上最危险的误读，而派生查询是唯一不会变陈旧的形状：没有哪个
    /// 字段会被忘记设置。
    pub absent: Vec<SpeakerId>,
}

/// 谁在 `round` 里作答了，哪个讨论者缺席了。
///
/// 只数讨论者。一个执行者的回合落在同一条流上、在派出它的那一轮内部（spec §16），而合成器的产物是
/// `System`（spec §2）；把其中任何一个当成讨论者的作答，都会让协议在一个讨论者与它自己的执行者之间
/// 找出「一致」。
pub fn round_attendance(events: &[Event], round: u32) -> RoundAttendance {
    let mut current: Option<u32> = None;
    let mut answers: Vec<(SpeakerId, String)> = Vec::new();
    let mut failed: Vec<SpeakerId> = Vec::new();

    for event in events {
        match &event.payload {
            EventPayload::RoundStarted { round: started, .. } => current = Some(*started),
            EventPayload::RoundEnded { .. } => current = None,
            _ => {
                if current != Some(round) {
                    continue;
                }
                let SpeakerId::Debater(_) = &event.speaker_id else {
                    continue;
                };
                match &event.payload {
                    EventPayload::MessageCompleted { text, .. } => {
                        answers.push((event.speaker_id.clone(), text.clone()));
                    }
                    EventPayload::TurnEnded {
                        reason: StopReason::Error,
                    } if !failed.contains(&event.speaker_id) => {
                        failed.push(event.speaker_id.clone());
                    }
                    _ => {}
                }
            }
        }
    }

    let absent = failed
        .into_iter()
        .filter(|speaker| !answers.iter().any(|(answered, _)| answered == speaker))
        .collect();

    RoundAttendance { answers, absent }
}

/// 对一个辩论轮的机械裁决。
///
/// 这里没有任何东西决定*停不停* —— 那是 [`crate::discussion::plan_after_round`]。这里只是那两份
/// （或更多）结论互相说了什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundOutcome {
    /// 至少两份作答，而且每一对相邻的都说同一个结论。
    Agreed,
    /// 至少两份互相冲突的作答。
    Diverged,
    /// 少于两份作答：什么都没比较过，所以什么都没达成一致。缺席的是 [`RoundAttendance::absent`]
    /// 点名的那些。
    Incomplete,
}

/// 从某一轮的参加情况上读出裁决。
///
/// 只比较一次，因为名册就是两个讨论者，而组装期拒掉别的规模（spec §15：N > 2 会重新打开「N = 2
/// 不仲裁」）。少于两份作答不是一个裁决，而是一次缺席。
pub fn round_outcome(attendance: &RoundAttendance) -> RoundOutcome {
    let [first, second, ..] = &attendance.answers[..] else {
        return RoundOutcome::Incomplete;
    };
    if answers_agree(&first.1, &second.1) {
        RoundOutcome::Agreed
    } else {
        RoundOutcome::Diverged
    }
}
