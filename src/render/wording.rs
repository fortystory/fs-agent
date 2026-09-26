//! The wording layer: every human-facing phrase, in one place.
//!
//! Pure functions from a domain value to a Chinese phrase. It knows nothing
//! about style (colour and bold stay in the painters) and nothing about
//! structure (the plain renderer's per-line prefix, the TUI's indentation and
//! `sessions show`'s round grouping are their callers'). Hard-coded Chinese, no
//! runtime locale: changing the language means changing this module
//! (spec §Implementation Decisions).
//!
//! **Model-visible text is not here** (ADR 0001): the debaters' and
//! synthesizer's system prompts, the projection's round prefix, `AgentError`'s
//! message and fs-agent's own tool-result text are frozen cache prefixes and must
//! not reach for this module.

use ratatui::buffer::CellWidth;

use crate::events::{
    hook_format, ContextSource, Decision, DecisionSource, HistoryReason, RoundMode, SpeakerId,
    StopReason, Usage,
};
use serde_json::Value;

use crate::permissions::Mode;
use crate::provider::FinishReason;

/// The human label for a round mode.
///
/// The discussion protocol's synthesis prompt uses the same words, so the
/// interface and the instructions name the rounds identically.
pub fn round_mode(mode: RoundMode) -> &'static str {
    match mode {
        RoundMode::Independent => "独立首轮",
        RoundMode::Targeted => "定向第二轮",
        RoundMode::Synthesis => "合成",
    }
}

/// The round section line both human painters print.
pub fn round_section(round: u32, mode: RoundMode) -> String {
    format!("── 第 {round} 轮（{}）──", round_mode(mode))
}

/// The narration that opens one agent turn.
pub fn turn_started(iteration: u32) -> String {
    format!("回合开始（第 {iteration} 次迭代）")
}

/// The narration that closes one agent turn, with the reason spelled out in
/// Chinese.
pub fn turn_ended(reason: StopReason) -> String {
    format!("回合结束：{}", stop_reason(reason))
}

/// A discussion round closing.
pub fn round_ended(round: u32, reason: StopReason) -> String {
    format!("第 {round} 轮结束：{}", stop_reason(reason))
}

/// What `fs-agent discuss` reports once the discussion is over and the screen is
/// back: why it stopped, how many rounds actually ran, and who was absent.
///
/// The absent side is named because it is the one thing the stream records but a
/// reader can miss: a round with one answer is not a round of agreement.
pub fn discussion_ended(reason: StopReason, rounds: u32, absent: &[SpeakerId]) -> String {
    let mut line = format!("讨论结束：{}（跑了 {rounds} 轮）", stop_reason(reason));
    if !absent.is_empty() {
        let names = absent
            .iter()
            .map(speaker_label)
            .collect::<Vec<_>>()
            .join("、");
        line.push_str(&format!("；缺席：{names}"));
    }
    line
}

/// Where a discussion can be read back from, for the line printed after the alt
/// screen has been restored: the TUI's transcript does not survive the process, so
/// the session id is the durable answer.
pub fn discussion_replay(session_id: &str) -> String {
    format!("会话 {session_id}；复盘：fs-agent sessions show {session_id}")
}

/// The pair that is debating, for a front end that has one field to name it in.
pub fn discussion_pair(first: &str, second: &str) -> String {
    format!("{first} × {second}")
}

/// One debater as a notice names it: `保守（deepseek-v4-pro）`.
///
/// The model is shown beside the name because the name is an identity the user chose
/// while the model is what actually answers — and with a pool the two can differ per
/// discussion. The shorthand case (a debater named after its model) says it once.
pub fn debater_label(name: &str, model: &str) -> String {
    if name == model {
        name.to_owned()
    } else {
        format!("{name}（{model}）")
    }
}

/// `--debaters` with the wrong shape.
pub fn needs_two_debaters(value: &str) -> String {
    format!("--debaters 需要两个名字（逗号分隔，例如 `--debaters 保守,激进`），得到 `{value}`")
}

/// `--debaters` naming something the pool does not have.
pub fn unknown_debater(name: &str, pool: &[&str]) -> String {
    format!(
        "池子里没有叫 `{name}` 的讨论者；可用：{}",
        pool.iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join("、")
    )
}

/// `/discuss` is about to run: which two models, and what they are being asked.
///
/// The question is named because it may not have been typed — a bare `/discuss` puts
/// the session's last question to the debaters, and the user should see which one that
/// was before two models start answering it.
pub fn discussion_starting(first: &str, second: &str, question: &str) -> String {
    format!(
        "开始讨论：{first} × {second}；题目：{}",
        first_non_empty_line(question)
    )
}

/// The first non-empty line of a block of text, trimmed — a question or a task can be
/// a paragraph, and a notice has one line.
fn first_non_empty_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

/// The advisory line for a roster whose two debaters are the same model.
///
/// Allowed — one expired subscription is not a reason to have no discussion at all —
/// but the design's premise is two independent judgements, and a single model asked
/// twice leaves only the sampling noise between them.
pub fn discussion_same_model(model: &str) -> String {
    format!("提示：两个讨论者都是 {model}——同一个模型问两遍，剩下的差异只有采样噪声")
}

/// The advisory line for a roster that is one vendor with two models.
pub fn discussion_one_vendor(first: &str, second: &str) -> String {
    format!("提示：两个讨论者来自同一厂商（{first} × {second}），多样性比设计假设的弱")
}

/// The prompt `fs-agent discuss` puts on a terminal when no question was given on
/// the command line.
pub fn question_prompt() -> &'static str {
    "问题> "
}

/// The whole session closing.
pub fn session_ended(reason: StopReason) -> String {
    format!("会话结束：{}", stop_reason(reason))
}

/// The Chinese phrase for a stopping point.
///
/// One explicit mapping, so a debug-formatted enum never reaches the interface
/// (spec §Implementation Decisions).
pub fn stop_reason(reason: StopReason) -> &'static str {
    match reason {
        StopReason::Completed => "完成",
        StopReason::MaxIterations => "达到迭代上限",
        StopReason::Aborted => "已取消",
        StopReason::MistakeLimit => "达到错误上限",
        StopReason::Error => "出错",
        StopReason::Consensus => "达成共识",
        StopReason::NoDivergence => "无分歧",
        StopReason::RoundsExhausted => "轮次用尽",
        StopReason::BudgetExhausted => "预算用尽",
    }
}

/// The one-line summary of a tool call. The tool name and its arguments are
/// structure and stay verbatim; only the verb is Chinese.
pub fn tool_call(tool: &str, args: &str) -> String {
    format!("调用 {tool}({args})")
}

/// A tool result trimmed to `max_chars`, marking the cut in Chinese so a person
/// can tell a complete short result from an elided long one.
pub fn tool_output_preview(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push_str("…（结果已省略）");
    out
}

/// The placeholder for a call whose result never arrived (a cancel).
pub fn no_tool_result() -> &'static str {
    "（流上没有结果）"
}

/// The placeholder for a failed call that carried no message of its own.
pub fn no_message() -> &'static str {
    "（没有消息）"
}

/// The pre-hook narration, with its mount point named in Chinese.
pub fn hook(point: &str, outcome: &str) -> String {
    format!("钩子 {}：{outcome}", hook_point(point))
}

/// The Chinese name of a hook mount point. An unknown point is passed through:
/// the outcome already carries the hook's own, unalterable text.
pub fn hook_point(point: &str) -> &str {
    match point {
        hook_format::POINT_PRE => "工具调用前",
        hook_format::POINT_POST => "工具调用后",
        other => other,
    }
}

/// A post-hook's feedback, merged into the call it annotates.
pub fn hook_feedback(outcome: &str) -> String {
    format!("[钩子] {outcome}")
}

/// A debater dispatching an executor.
pub fn executor_spawned(executor_id: &str) -> String {
    format!("派出执行者 {executor_id}")
}

/// An executor closing, with its reason and summary.
pub fn executor_finished(executor_id: &str, reason: StopReason, summary: &str) -> String {
    format!(
        "执行者 {executor_id} 收尾：{} — {summary}",
        stop_reason(reason)
    )
}

/// The heading of a divergence block.
pub fn divergence(topic: &str) -> String {
    format!("分歧：{topic}")
}

/// A speaker's own error, as narration. The message itself is frozen
/// (`AgentError.message` is model-visible, ADR 0001) and passed through.
pub fn agent_error(message: &str) -> String {
    format!("错误：{message}")
}

/// The one-line usage summary both human painters print.
pub fn usage_summary(usage: &Usage) -> String {
    format!(
        "用量 in={} out={} cached={} miss={}",
        usage.input_tokens, usage.output_tokens, usage.cached_tokens, usage.miss_tokens
    )
}

/// The marker in front of a streamed reasoning trace.
pub fn reasoning_marker() -> &'static str {
    "[思考] "
}

/// The plain transcript's reasoning label, appended to the speaker prefix.
pub fn reasoning_label() -> &'static str {
    "（思考）"
}

/// A completed message's boundary narration (the body already streamed).
pub fn message_complete() -> &'static str {
    "消息完成"
}

/// The line that opens a thinking segment, before any正文 has arrived: the stream
/// is running and the trace is not finished (票 03 §Answer).
pub fn thinking_in_progress() -> &'static str {
    "… 正在思考"
}

/// The line a thinking segment settles into once the正文 arrives: the trace is
/// frozen and the line becomes the way into its full text (票 03 §Answer).
pub fn thinking_finished() -> &'static str {
    "✓ 思考完成"
}

/// The verb a tool call line leads with (票 02 §2). The arguments follow it, by
/// `transcript::summarize_args`.
pub fn tool_call_label() -> &'static str {
    "调用"
}

/// Appended to the **end** of a failed call's line, so the failure is a suffix and
/// not a second line (票 02 §2).
pub fn tool_failed() -> &'static str {
    "失败"
}

/// The detail view's note when the whole tool output cannot be read back — the
/// pointer's file is gone, or was never written (票 02 §4).
pub fn detail_output_unavailable() -> &'static str {
    "全文不可用"
}

/// The detail view's note when the thinking line has no whole trace to show: the
/// synthesizer streams reasoning without recording it (票 02 §1).
pub fn detail_reasoning_unrecorded() -> &'static str {
    "本次未记录思考全文"
}

/// The detail view's note when a body was longer than the reader's limit.
pub fn detail_truncated() -> &'static str {
    "已截断"
}

/// A detail section's heading, drawn inside a rule: `── 思考 ──` (票 03 §Answer).
pub fn detail_section(name: &str) -> String {
    format!("── {name} ──")
}

/// The heading of the detail view's thinking section.
pub fn detail_thinking_section() -> &'static str {
    "思考"
}

/// The heading of the detail view's arguments section.
pub fn detail_args_section() -> &'static str {
    "参数"
}

/// The heading of the detail view's output section.
pub fn detail_output_section() -> &'static str {
    "输出"
}

/// The detail view's footer: where in the body the reader is, and how to leave.
pub fn detail_footer(position: usize, total: usize) -> String {
    format!("↕ {position}/{total} · esc 关闭")
}

/// One tool call's **description**: what the call was for, in place of its arguments.
///
/// The transcript shows `{label} 调用 {tool} {description}`; the concrete arguments and
/// the whole output live in the call's detail view. A line of raw arguments is a
/// debugger's view of a call — the reader wants to know what it *did* (票 02 §2，
/// 2026-09-23 修正).
///
/// The rules are deliberately few and mechanical, because the description is derived
/// from the arguments alone — nothing on the stream says what the model intended:
///
/// * the questionnaire tool describes itself by the question it asks;
/// * a tool that takes a path describes itself by that path;
/// * a shell command describes itself by a verb for its first recognised command plus
///   the first path-like word in it — `查询 .scratch/tui-history-replay`;
/// * anything else falls back to the call's argument summary, so a dynamic tool with
///   unknown arguments is never left blank.
pub fn tool_description(tool: &str, args: &Value) -> String {
    if let Some(described) = argument_description(tool, args) {
        return described;
    }
    if let Some(command) = args.get("command").and_then(Value::as_str) {
        return command_description(command);
    }
    let summary = crate::render::transcript::summarize_args(args);
    if summary.is_empty() {
        return String::new();
    }
    summary
}

/// The description a tool's own arguments give it: the field that says what the call
/// is about, in the order the tools in this repo name them.
fn argument_description(tool: &str, args: &Value) -> Option<String> {
    // A questionnaire is about the question, and its own `header` is the model's
    // one-phrase summary of it — which is exactly what a description is.
    if tool == ASK_USER_QUESTION_TOOL {
        return first_question_field(args);
    }
    // A call that names one path is about that path.
    for key in ["path", "file_path", "file", "target", "pattern", "query"] {
        if let Some(value) = args.get(key).and_then(Value::as_str) {
            let value = first_line(value);
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    // A dispatched task is about the task.
    for key in ["task", "description", "prompt", "brief"] {
        if let Some(value) = args.get(key).and_then(Value::as_str) {
            let value = first_line(value);
            if !value.is_empty() {
                return Some(cut(&value, DESCRIPTION_MAX_CHARS));
            }
        }
    }
    None
}

/// The questionnaire tool's name.
///
/// Matched by name because a description is wording, and wording is keyed on what the
/// call *is*: the tool table's `Tool` traits hold behaviour, not prose.
const ASK_USER_QUESTION_TOOL: &str = "ask_user_question";

/// The first question's `header`, else its `question`, as one line.
fn first_question_field(args: &Value) -> Option<String> {
    let questions = args.get("questions")?.as_array()?;
    let first = questions.first()?;
    for key in ["header", "question"] {
        if let Some(value) = first.get(key).and_then(Value::as_str) {
            let value = first_line(value);
            if !value.is_empty() {
                return Some(cut(&value, DESCRIPTION_MAX_CHARS));
            }
        }
    }
    None
}

/// A shell command's description: a verb for what it does, then what it does it to.
///
/// The rules are mechanical because the description is derived from the arguments
/// alone — nothing on the stream says what the model intended:
///
/// 1. `cd somewhere` is dropped: navigation is never what a call is *about*.
/// 2. The verb is the first word this layer recognises anywhere in the rest
///    ([`command_verb`]), so `ls -a; find .scratch` describes itself by what it does.
/// 3. The subject is the first word that looks like a path — `查询
///    .scratch/tui-history-replay` — because that is the token that tells two `查询`
///    calls apart. Failing that it is the first operand: `修改 build` from
///    `rm -rf build`.
/// 4. A program whose second word is a subcommand keeps both, because the subcommand
///    alone is ambiguous: `查看 git status`, `运行 cargo test`.
/// 5. A command with nothing recognisable in it describes itself by its first word:
///    `运行 env`.
fn command_description(command: &str) -> String {
    let command = first_line(command);
    let words: Vec<&str> = command
        .split([';', '|', '&', '\n'])
        .flat_map(str::split_whitespace)
        .map(clean_word)
        .filter(|word| !word.is_empty())
        .collect();
    let verb = words.iter().find_map(|word| command_verb(word));
    let subject = subject_word(&words);
    match (verb, subject) {
        (Some(verb), Some(subject)) => cut(&format!("{verb} {subject}"), DESCRIPTION_MAX_CHARS),
        (Some(verb), None) => cut(verb, DESCRIPTION_MAX_CHARS),
        (None, Some(subject)) => cut(&format!("运行 {subject}"), DESCRIPTION_MAX_CHARS),
        (None, None) => String::new(),
    }
}

/// Programs whose second word is a subcommand worth keeping: `git status` is a call,
/// `status` on its own is ambiguous.
const SUBCOMMAND_PROGRAMS: [&str; 6] = ["git", "cargo", "npm", "pnpm", "yarn", "go"];

/// The word a command is about: the first path-like operand, else the first operand.
///
/// Flags (`-rf`), redirections (`2>/dev/null`) and `cd`'s target are skipped — none of
/// them is what a call is about, and naming the wrong one on a destructive call is
/// worse than naming nothing.
fn subject_word(words: &[&str]) -> Option<String> {
    let mut operands: Vec<&str> = Vec::with_capacity(words.len());
    let mut skip_next = false;
    for word in words {
        let word = *word;
        if skip_next {
            skip_next = false;
            continue;
        }
        if word == "cd" {
            skip_next = true;
            continue;
        }
        if word.starts_with('-') || word.contains('>') || word.contains(' ') {
            continue;
        }
        operands.push(word);
    }
    if let Some(path) = operands
        .iter()
        .find(|word| word.contains('/') || word.contains('*') || word.contains('.'))
    {
        return Some((*path).to_owned());
    }
    let mut operands = operands.into_iter();
    let program = operands.next()?;
    match operands.next() {
        Some(subcommand) if SUBCOMMAND_PROGRAMS.contains(&program) => {
            Some(format!("{program} {subcommand}"))
        }
        Some(operand) => Some(operand.to_owned()),
        None => None,
    }
}

/// A command word with the punctuation a shell line wraps it in taken off.
fn clean_word(word: &str) -> &str {
    word.trim_start_matches('(')
        .trim_matches(|ch| ch == '"' || ch == '\'' || ch == '`' || ch == ';')
}

/// The whole folded call line's text: `调用 工具 描述`.
///
/// **One producer for both readers.** The transcript's folded line and the permission
/// question that asks about the same call show this same string, so the question you
/// answer and the line it is about cannot drift apart (2026-09-23, user request: the
/// permission popup should read like the call line).
pub fn tool_call_line(tool: &str, args: &Value) -> String {
    let description = tool_description(tool, args);
    if description.is_empty() {
        format!("{} {tool}", tool_call_label())
    } else {
        format!("{} {tool} {description}", tool_call_label())
    }
}

/// The verb a command's leading word earns: what the reader would say the call does.
///
/// Only three verbs, because a reader only needs three: look something up, look at
/// something, or change something. A program this layer does not know is *not* guessed
/// at — see [`command_description`].
fn command_verb(word: &str) -> Option<&'static str> {
    match word {
        "grep" | "rg" | "ag" | "find" | "fd" | "rgrep" => Some("查询"),
        "ls" | "cat" | "head" | "tail" | "wc" | "stat" | "file" | "tree" | "pwd" | "du"
        | "less" | "more" | "sed" | "awk" | "jq" | "git" => Some("查看"),
        "rm" | "mv" | "cp" | "mkdir" | "touch" | "chmod" | "chown" | "tee" | "ln" => Some("修改"),
        "cargo" | "make" | "npm" | "pnpm" | "yarn" | "go" | "pytest" | "python" | "python3"
        | "node" | "bash" | "sh" | "zsh" | "test" => Some("运行"),
        _ => None,
    }
}

/// `text`'s first line, trimmed of surrounding blanks.
fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// `text` cut to `max` characters, marked with an ellipsis.
///
/// Local to this layer on purpose: the description is wording, and the transcript's
/// own `truncate` is a presentation helper the wording layer has no business sharing.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

/// How long a call's description may be before it is cut.
///
/// The description exists to make the line readable at a glance; a long one would be
/// as unreadable as the arguments it replaced, and the pane would wrap it anyway.
const DESCRIPTION_MAX_CHARS: usize = 60;

/// A tool call that finished successfully. The result itself is printed by the
/// caller.
pub fn tool_completed() -> &'static str {
    "工具完成"
}

/// The provider's own terminal label, named in Chinese. It is diagnostic only,
/// but it still reaches the interface through a diagnostic line.
pub fn finish_reason(reason: &FinishReason) -> &str {
    match reason {
        FinishReason::Stop => "正常停止",
        FinishReason::Length => "达到长度上限",
        FinishReason::ToolCalls => "请求工具",
        FinishReason::ContentFilter => "内容被过滤",
        FinishReason::InsufficientSystemResource => "系统资源不足",
        FinishReason::Aborted => "已取消",
        FinishReason::Other(other) => other,
    }
}

/// The Chinese phrase for a stopping point recorded as its `as_str` name: a
/// `sessions stats` row carries the name, not the enum. An unknown name is shown
/// as itself.
pub fn stop_reason_name(name: &str) -> &str {
    match name {
        "Completed" => stop_reason(StopReason::Completed),
        "MaxIterations" => stop_reason(StopReason::MaxIterations),
        "Aborted" => stop_reason(StopReason::Aborted),
        "MistakeLimit" => stop_reason(StopReason::MistakeLimit),
        "Error" => stop_reason(StopReason::Error),
        "Consensus" => stop_reason(StopReason::Consensus),
        "NoDivergence" => stop_reason(StopReason::NoDivergence),
        "RoundsExhausted" => stop_reason(StopReason::RoundsExhausted),
        "BudgetExhausted" => stop_reason(StopReason::BudgetExhausted),
        other => other,
    }
}

/// The Chinese phrase for a permission verdict recorded as its `as_str` name.
pub fn decision_name(name: &str) -> &str {
    match name {
        "allow" => decision(Decision::Allow),
        "ask" => decision(Decision::Ask),
        "deny" => decision(Decision::Deny),
        other => other,
    }
}

/// A permission question on the transcript.
///
/// What a person needs is the tool and the concrete call it would make — the
/// command, the path, the body. The request and tool-call ids are the stream's
/// business; they are not shown.
pub fn permission_asked(tool_name: Option<&str>, args: &str) -> String {
    match (tool_name, args.is_empty()) {
        (Some(tool), true) => format!("权限询问：{tool}"),
        (Some(tool), false) => format!("权限询问：{tool}（{args}）"),
        (None, true) => "权限询问".to_owned(),
        (None, false) => format!("权限询问（{args}）"),
    }
}

/// A permission verdict, with the source of the decision named in Chinese. The
/// reason is the gate's own durable text and is passed through.
pub fn permission_decided(
    decision: Decision,
    source: DecisionSource,
    reason: Option<&str>,
) -> String {
    let suffix = reason
        .map(|reason| format!("：{reason}"))
        .unwrap_or_default();
    format!(
        "权限裁决：{}（{}）{suffix}",
        self::decision(decision),
        decision_source(source)
    )
}

/// The Chinese phrase for a permission verdict.
pub fn decision(decision: Decision) -> &'static str {
    match decision {
        Decision::Allow => "允许",
        Decision::Ask => "询问",
        Decision::Deny => "拒绝",
    }
}

/// The Chinese label for where a verdict came from. The precedent for this whole
/// module, and now one of its functions (spec §Implementation Decisions).
pub fn decision_source(source: DecisionSource) -> &'static str {
    match source {
        DecisionSource::User => "用户",
        DecisionSource::Hook => "钩子",
        DecisionSource::Policy => "策略",
    }
}

/// One key on a question's button row: the key that answers, and what that answer
/// means.
///
/// The TUI's overlay paints one `[key] label` per entry and the plain console joins
/// the same table into its single input line, so a question cannot grow a key on one
/// front end that the other does not offer (spec §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    /// The key that answers, shown literally.
    pub key: char,
    /// What that key answers.
    pub label: &'static str,
}

/// The keys that answer a permission question.
pub static PERMISSION_CHOICES: [Choice; 3] = [
    Choice {
        key: 'y',
        label: "允许",
    },
    Choice {
        key: 'a',
        label: "总是允许",
    },
    Choice {
        key: 'n',
        label: "拒绝",
    },
];

/// The keys that answer a plan-mode conflict.
pub static PLAN_CHOICES: [Choice; 3] = [
    Choice {
        key: 'o',
        label: "覆盖",
    },
    Choice {
        key: 'a',
        label: "追加",
    },
    Choice {
        key: 'k',
        label: "保留",
    },
];

/// The keys that answer the oversized-paste question.
pub static PASTE_CHOICES: [Choice; 2] = [
    Choice {
        key: 'y',
        label: "粘贴",
    },
    Choice {
        key: 'n',
        label: "取消",
    },
];

/// The keys that answer the clear-draft question.
pub static CLEAR_CHOICES: [Choice; 2] = [
    Choice {
        key: 'y',
        label: "清空",
    },
    Choice {
        key: 'n',
        label: "保留",
    },
];

/// The keys that answer a plan-mode conflict, paired with the answer each one sends.
pub static PLAN_CHOICE_ANSWERS: [(char, crate::permissions::PlanConflict); 3] = [
    ('o', crate::permissions::PlanConflict::Overwrite),
    ('a', crate::permissions::PlanConflict::Append),
    ('k', crate::permissions::PlanConflict::Keep),
];

/// The keys that answer a permission question, paired with the answer each one sends.
pub static PERMISSION_CHOICE_ANSWERS: [(char, crate::permissions::Answer); 3] = [
    ('y', crate::permissions::Answer::Allow),
    ('a', crate::permissions::Answer::AlwaysAllow),
    ('n', crate::permissions::Answer::Deny),
];

/// The keys that answer the exit confirmation (票 06 §2). The safe answer is the
/// first-looking one to a hand that reads the row: `n` cancels, and so does `Esc`.
pub static EXIT_CHOICES: [Choice; 2] = [
    Choice {
        key: 'y',
        label: "退出",
    },
    Choice {
        key: 'n',
        label: "取消",
    },
];

/// A button row as one line of text: `[y] 允许 / [a] 总是允许 / [n] 拒绝`.
///
/// This is what a plain console's input line uses. The TUI paints the same entries
/// as spans of its own, with the key picked out (spec §9).
pub fn choices_text(choices: &[Choice]) -> String {
    choices
        .iter()
        .map(|choice| format!("[{}] {}", choice.key, choice.label))
        .collect::<Vec<_>>()
        .join(" / ")
}

/// The permission overlay's title.
///
/// It no longer names the tool: the row under it opens with `调用 工具 …`, which is the
/// same sentence the folded transcript line carries, so the name is there once
/// (2026-09-23, user request: the popup repeated itself).
pub fn permission_title() -> &'static str {
    "权限询问："
}

/// The **body** row of a permission overlay: the concrete call the question is
/// about (`bash（command=rm -rf /）`). The tool name leads it so the row stands on
/// its own under the title.
pub fn permission_call(tool_name: &str, args: &str) -> String {
    if args.is_empty() {
        tool_name.to_owned()
    } else {
        format!("{tool_name}（{args}）")
    }
}

/// The plain console's permission input line, which also has room for the gate's
/// reason.
pub fn permission_prompt_with_context(tool_name: &str, args: &str, reason: &str) -> String {
    format!(
        "{}？原因：{reason} {} ",
        permission_asked(Some(tool_name), args),
        choices_text(&PERMISSION_CHOICES)
    )
}

/// The **title** row of the plan-mode conflict overlay.
pub fn plan_conflict_title() -> &'static str {
    "计划文件冲突"
}

/// The **body** row of the plan-mode conflict overlay: the path already there.
pub fn plan_conflict_body(path: &str) -> String {
    format!("{path} 已存在")
}

/// The plain console's plan-conflict input line.
pub fn plan_conflict_prompt(path: &str) -> String {
    format!(
        "{}：{} ",
        plan_conflict_body(path),
        choices_text(&PLAN_CHOICES)
    )
}

/// The **title** row of the oversized-paste question.
pub fn paste_title() -> &'static str {
    "粘贴确认"
}

/// The **body** row of the oversized-paste question.
pub fn paste_body(chars: usize) -> String {
    format!("粘贴 {chars} 字符")
}

/// The **title** row of the clear-draft question.
pub fn clear_draft_title() -> &'static str {
    "清空输入"
}

/// The **body** row of the clear-draft question.
pub fn clear_draft_body() -> &'static str {
    "草稿有多行，Esc 会把它们全部丢掉"
}

/// The title of the `Ctrl-D` exit confirmation (票 06 §2).
pub fn exit_title() -> &'static str {
    "退出会话"
}

/// The body of the exit confirmation. It says both things a person would want to
/// know before saying yes: the transcript on disk survives, the unsent draft does
/// not (票 06 §2).
pub fn exit_body() -> &'static str {
    "会话记录会保留；未发送的草稿会丢弃"
}

/// The questionnaire footer's way back one question.
pub fn questionnaire_previous() -> &'static str {
    "← 上一题"
}

/// The questionnaire footer's way forward one question.
pub fn questionnaire_next() -> &'static str {
    "下一题 →"
}

/// The questionnaire footer's submit button, drawn only once everything is handled.
pub fn questionnaire_submit() -> &'static str {
    "提交"
}

/// The footer that pages a questionnaire: `2 / 3`.
pub fn questionnaire_progress(index: usize, total: usize) -> String {
    format!("{} / {}", index + 1, total)
}

/// The keys a questionnaire offers, in the order they are shown.
///
/// `ready` is whether every question is handled, because that is what decides
/// whether `enter` submits or only continues (spec §7). Promising `enter 提交`
/// while it is still advancing would be a lie the interface tells on every
/// question but the last.
pub fn questionnaire_hint(ready: bool) -> &'static str {
    if ready {
        "↑↓ 选择 · enter 提交 · space 确认 · tab 跳过 · ←→ 换题"
    } else {
        "↑↓ 选择 · enter 继续 · space 确认 · tab 跳过 · ←→ 换题"
    }
}

/// The questionnaire's footer: which question is on screen, and what the keys do.
///
/// `ready` travels into [`questionnaire_hint`] so the footer says `提交` only
/// when the key really submits.
pub fn questionnaire_status(index: usize, total: usize, ready: bool) -> String {
    format!(
        "{} · {}",
        questionnaire_progress(index, total),
        questionnaire_hint(ready)
    )
}

/// The marker a multi-select question carries beside its text, so the user knows
/// more than one option may be picked.
pub fn questionnaire_multi_marker() -> &'static str {
    "（可多选）"
}

/// The label of the line a typed answer goes on, for a question with no options.
pub fn questionnaire_answer_label() -> &'static str {
    "回答："
}

/// The label of the line custom text goes on, for a question that offers options.
pub fn questionnaire_custom_label() -> &'static str {
    "自定义："
}

/// The plain console's prompt for a question that offers options.
///
/// The line-oriented front end has no visible mode, so the prompt has to say how
/// to pick and how to skip: a number picks, anything else is custom text, and an
/// empty line is a skip. A multi-select question says a second line follows,
/// because that supplement is the only way to answer `selected` and `custom`
/// together (spec §7).
pub fn questionnaire_plain_options_prompt(multi_select: bool) -> &'static str {
    if multi_select {
        "输入编号（逗号分隔）选择，或输入文本；下一行补充；回车跳过 > "
    } else {
        "输入编号选择，或直接输入文本；回车跳过 > "
    }
}

/// The plain console's second line for a multi-select question: the optional
/// supplement that goes with the chosen options (spec §7).
pub fn questionnaire_plain_supplement_prompt() -> &'static str {
    "补充文本（可留空）> "
}

/// The plain console's prompt for a question with no options.
pub fn questionnaire_plain_answer_prompt() -> &'static str {
    "输入回答；回车跳过 > "
}

/// One option of a model's question, as both human front ends show it:
/// `{number}. {label}{badge} — {description}` (spec §7).
///
/// This is the **one** generator of the option line, called by the plain printer
/// and by the TUI's questionnaire painter, so a change to how an option reads
/// cannot land in only one of them. The caller adds whatever state its front end
/// shows beside the line — the TUI's picked/highlighted marker — because that is
/// the one thing the two do not share.
///
/// The `(Recommended)` suffix is a display convention: it is replaced by
/// [`recommended_badge`] here, while the value an answer carries keeps the whole
/// label ([`recommended_label`]). The number is a reading index, not a key: the
/// decided keyboard has none.
pub fn questionnaire_option(number: usize, label: &str, description: Option<&str>) -> String {
    let (label, recommended) = recommended_label(label);
    let mut text = format!("{number}. {label}");
    if recommended {
        text.push_str(recommended_badge());
    }
    if let Some(description) = description
        .map(str::trim)
        .filter(|description| !description.is_empty())
    {
        text.push_str(" — ");
        text.push_str(description);
    }
    text
}

/// The suffix a model appends to recommend an option (spec §7).
pub const RECOMMENDED_SUFFIX: &str = "(Recommended)";

/// The badge shown for an option whose label ends in [`RECOMMENDED_SUFFIX`].
pub fn recommended_badge() -> &'static str {
    "（推荐）"
}

/// Split a model-supplied option label into what is shown and whether it is
/// recommended.
///
/// The suffix is a **display** convention: it is stripped so the option reads as
/// a choice rather than as a sentence, while the value the answer carries stays
/// the original label, marker and all (spec §7). The match is case-sensitive and
/// only at the end, so a label that merely mentions the word is left alone.
pub fn recommended_label(label: &str) -> (&str, bool) {
    match label.trim_end().strip_suffix(RECOMMENDED_SUFFIX) {
        Some(rest) => (rest.trim_end(), true),
        None => (label, false),
    }
}

/// The human's `[speaker]` prefix: one generator, used by every human-facing
/// renderer, and deliberately not the model-side projection prefix (spec §5).
///
/// A debater keeps its own name; the unattributed speakers get a Chinese label.
pub fn speaker_label(speaker: &SpeakerId) -> String {
    match speaker {
        SpeakerId::Debater(id) => format!("[{id}]"),
        SpeakerId::Executor(id) => format!("[执行者 {id}]"),
        SpeakerId::User => "[用户]".to_owned(),
        SpeakerId::System => "[系统]".to_owned(),
    }
}

/// A pinned context injection, with its source named rather than debug-printed.
pub fn context_injected(source: ContextSource) -> String {
    format!("[上下文注入：{}]", context_source(&source))
}

/// The Chinese name of a context source.
pub fn context_source(source: &ContextSource) -> String {
    match source {
        ContextSource::AgentsMd => "AGENTS.md".to_owned(),
        ContextSource::SkillsCatalog => "技能清单".to_owned(),
        ContextSource::Skill => "技能".to_owned(),
        ContextSource::PlanMode => "计划模式".to_owned(),
        // The one injection that belongs to **one** participant, so it says which:
        // a reader of the transcript should see who was given a persona.
        ContextSource::Persona(name) => format!("人物：{name}"),
    }
}

/// A history range that stopped being authoritative. The summary is the stream's
/// own text and passes through; without one the line still says what happened.
pub fn history(reason: HistoryReason, summary: Option<&str>) -> String {
    let text = summary.unwrap_or("历史已被取代");
    format!("[历史：{}] {text}", history_reason(reason))
}

/// The Chinese name of a history reason.
pub fn history_reason(reason: HistoryReason) -> &'static str {
    match reason {
        HistoryReason::Regenerate => "重新生成",
        HistoryReason::Undo => "撤销",
        HistoryReason::Compaction => "压缩",
        HistoryReason::ModeChange => "模式变更",
    }
}

/// The renderer's own diagnostic line.
pub fn diagnostic(message: &str) -> String {
    format!("[诊断] {message}")
}

/// The renderer fell behind the channel and lost events.
pub fn renderer_dropped(dropped: u64) -> String {
    format!("渲染器丢弃了 {dropped} 个事件")
}

/// The status word: whether a turn is in flight.
pub fn status_word(busy: bool) -> &'static str {
    if busy {
        "工作中"
    } else {
        "就绪"
    }
}

/// The live key hints, in the order they are shown: the most used first. The way
/// out is [`EXIT_HINT_IDLE`] / [`EXIT_HINT_BUSY`] (whichever [`exit_hint`] picks),
/// which is reserved rather than appended, so it survives every width.
const KEY_HINTS: [&str; 5] = [
    "enter 发送",
    "ctrl-j 换行",
    "esc 取消",
    "shift+tab 计划",
    "PgUp/PgDn 滚动",
];

/// The way out while the keyboard is idle: both gestures quit, and the line says
/// so. It is one item rather than two, because the two keys mean the same thing
/// here and a narrow terminal has only so many columns (票 06 §4).
pub const EXIT_HINT_IDLE: &str = "ctrl-c/ctrl-d 退出";

/// The way out while a run is in flight: `Ctrl-C` cancels, and `Ctrl-D` is
/// deliberately ignored — hinting at a key that does nothing is the one thing the
/// hint row must never do (票 06 §4).
pub const EXIT_HINT_BUSY: &str = "ctrl-c 退出";

/// The hints a front end shows when it is **not** reading lines: a one-shot
/// `discuss`, or the stretch of an interactive session with a turn in flight.
///
/// Only what the keyboard really does then — stop the run, and read back what it
/// produced. No `enter 发送` (nothing would be sent) and no `shift+tab 计划` (that
/// gesture is the interactive loop's, and a discussion has no prompt to return to).
const VIEWER_HINTS: [&str; 2] = ["esc 取消", "PgUp/PgDn 滚动"];

/// The status line for a terminal `width` **columns** wide.
///
/// The hints fill from the left with the way out (`exit`) reserved at their end, and
/// the state word is placed in front of them only if it still fits — so a narrow
/// terminal keeps its way out *and* the hints that explain the keys, and gives up
/// `就绪` rather than `ctrl-j 换行`. The state word's placement is always the left
/// edge; what degrades is whether it appears at all.
///
/// Hint ladder, measured in the rendered frame: three items at 40 columns, four at
/// 60, five at 80, and all six plus the state word at 120. The way out is one item
/// now (`ctrl-c/ctrl-d 退出`), seven columns wider than the old `ctrl-c 退出`, and
/// that is why the state word disappears from 60 through 80 — at 40 it survives
/// because there is only one hint to pay for (票 06 §4).
pub fn status_line(busy: bool, width: u16) -> String {
    hint_line(status_word(busy), &KEY_HINTS, exit_hint(busy), width)
}

/// The status line for a front end that is not reading lines: the same ladder over
/// [`VIEWER_HINTS`].
///
/// The distinction is not cosmetic. The hints describe what the keyboard does, and a
/// session that is mid-turn — or a `discuss` run, which never asks for a line at all
/// — would otherwise promise `enter 发送` for a key that sends nothing (spec §6).
pub fn viewer_status_line(busy: bool, width: u16) -> String {
    hint_line(status_word(busy), &VIEWER_HINTS, exit_hint(busy), width)
}

/// The way-out item for a status line: the idle wording only while the keyboard can
/// really quit, which is exactly when nothing is running.
pub fn exit_hint(busy: bool) -> &'static str {
    if busy {
        EXIT_HINT_BUSY
    } else {
        EXIT_HINT_IDLE
    }
}

/// The ladder both status lines share: hints from the left, the way out reserved at
/// the end, and the state word leading only when it still fits.
fn hint_line(state: &str, hints: &[&str], exit: &str, width: u16) -> String {
    let exit_columns = exit.cell_width();
    let mut chosen = String::new();
    for hint in hints {
        let candidate = if chosen.is_empty() {
            (*hint).to_owned()
        } else {
            format!("{chosen} · {hint}")
        };
        if candidate.cell_width() + " · ".cell_width() + exit_columns > width {
            break;
        }
        chosen = candidate;
    }
    let run = if chosen.is_empty() {
        exit.to_owned()
    } else {
        format!("{chosen} · {exit}")
    };
    let with_state = format!("{state} · {run}");
    if with_state.cell_width() <= width {
        with_state
    } else {
        run
    }
}

// ---------------------------------------------------------------------------
// History replay (`.scratch/tui-history-replay/spec.md` §4, §6)
// ---------------------------------------------------------------------------

/// The name the replay goes by and the count it has reached: how many of the
/// history's events have been laid into the transcript.
///
/// Deliberately `history_*`, not `replay_*`: this module already has a whole
/// `replay_*` family for `sessions replay`, which **recomputes a projection** and
/// is a different thing from laying history out for a reader (spec §10).
pub fn history_progress(n: usize, m: usize) -> String {
    format!("恢复历史 {n}/{m}")
}

/// The same count once the hint row is too narrow for the whole phrase: the
/// minimum legal frame (`40×10`) leaves 38 columns of hints (票 06 §4).
pub fn history_progress_narrow(n: usize, m: usize) -> String {
    format!("恢复中 {n}/{m}")
}

/// The count with no numbers at all, for a hint row narrower than the minimum
/// frame can ever draw.
pub fn history_progress_minimal() -> &'static str {
    "恢复中"
}

/// The replay progress line for a hint row `width` columns wide.
///
/// The width is the **hint row's**, not the terminal's, exactly as
/// [`status_line`]'s is: the minimum frame (40 columns) leaves 38 columns of hints,
/// and that is the width the middle rung exists for. The ladder is here rather than
/// in the renderer for the same reason the hint ladder is: it is wording, and it is
/// measurable without a terminal.
pub fn history_progress_line(n: usize, m: usize, width: u16) -> String {
    if width < HISTORY_NARROW_MIN {
        history_progress_minimal().to_owned()
    } else if width < HISTORY_FULL_MIN {
        history_progress_narrow(n, m)
    } else {
        history_progress(n, m)
    }
}

/// The narrowest hint row that still carries the count: the minimum legal frame
/// less the two border columns.
const HISTORY_NARROW_MIN: u16 = super::layout::MIN_WIDTH - 2;

/// The hint ladder's next measured rung after the minimum: a 60-column terminal
/// buys the second hint, and it is where the full history phrase earns its columns.
const HISTORY_FULL_TERMINAL: u16 = 60;

/// The hint row from which the full phrase is worth its columns.
const HISTORY_FULL_MIN: u16 = HISTORY_FULL_TERMINAL - 2;

/// The line drawn between replayed history and what this session adds.
///
/// It is a **render-layer line, not an event**: it never enters the log, so the next
/// `--continue` inserts a fresh one on the new seam instead of replaying the old
/// (spec §6).
pub fn history_divider() -> &'static str {
    "── 以上为历史 ──"
}

/// The model's label: the status row's first segment (spec §5).
pub const PANEL_MODEL: &str = "模型";
/// The session's readings in the sidebar's usage page (spec §3).
pub const PANEL_CONTEXT: &str = "上下文";
/// Spelled the way the rest of the UI spells it; `CONTEXT.md` has no Chinese word
/// for it and the stats lines already say `token`.
pub const PANEL_TOKENS: &str = "token";
/// **Turn**, not round: the panel counts `TurnEnded` (`CONTEXT.md` keeps 轮次 and
/// 回合 apart).
pub const PANEL_TURNS: &str = "回合";
pub const PANEL_INPUT: &str = "输入";
pub const PANEL_OUTPUT: &str = "输出";
pub const PANEL_CACHE: &str = "缓存";

/// What a field shows when there is no number for it yet.
pub const PANEL_UNKNOWN: &str = "—";

/// A count with thousands separators: `12,345`.
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Two counts in one field: `12,345 / 100,000`.
fn pair(left: u64, right: u64) -> String {
    format!("{} / {}", thousands(left), thousands(right))
}

/// What the session has spent, against its allowance when it has one.
///
/// A session with no cap shows the spend alone: `12,345 / —` would read as a cap
/// that is missing rather than one that was never set (spec §8).
pub fn token_pair(used: u64, limit: Option<u64>) -> String {
    match limit {
        Some(limit) => pair(used, limit),
        None => thousands(used),
    }
}

/// How full the model's window is: `12,345 / 200,000（6%）`, or [`PANEL_UNKNOWN`]
/// before a call has reported its input tokens.
///
/// `with_share` is the panel saying the value column has room for the percentage.
/// It is a parameter rather than a second function because the pair and its share
/// are one field: `12,345 / 200,000（6%）` is what it says, and dropping the tail is
/// how it degrades.
pub fn context_pair(used: Option<u64>, usable: u64, with_share: bool) -> String {
    let Some(used) = used else {
        return PANEL_UNKNOWN.to_owned();
    };
    let pair = pair(used, usable);
    if with_share {
        format!("{}（{}%）", pair, used.saturating_mul(100) / usable.max(1))
    } else {
        pair
    }
}

/// The cache split of one call's input: what was served from the prefix cache and
/// what was not.
pub fn cache_pair(cached: u64, miss: u64) -> String {
    pair(cached, miss)
}

/// The indicator that says how much arrived while the viewport was scrolled away,
/// and that the block is the way back (spec §4).
pub fn new_content(rows: usize) -> String {
    format!("↓ {rows} 行新内容 · 点此到底")
}

/// The same indicator when nothing has arrived: it is only the way back.
pub fn back_to_bottom() -> &'static str {
    "点此到底"
}

/// Everything a terminal smaller than the minimum shows, so the reason is a
/// sentence rather than an empty screen (spec §2).
pub fn too_small(width: u16, height: u16) -> String {
    format!("终端太小：至少 {width}×{height}")
}

/// The program and the version it was built from: the sidebar's text identity, which is
/// what a terminal too narrow for the mark shows instead (spec §3).
pub fn identity() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

/// The mark the wide sidebar carries, five rows of block shading.
///
/// The characters are all text; the colour ramp that makes them read as letters is
/// the painter's business ([`crate::render::tui`]), exactly as it is for every other
/// phrase in this module. A sidebar too narrow for the whole mark never asks for these
/// rows at all — [`crate::render::layout`] decides that up front, so nothing here has
/// to think about clipping.
///
/// The mark spells `fs` — a forked synthesis, "two forks, one stem" (see
/// `CONTEXT.md`), and the pixel grid is the one the maintainer picked.
pub fn logo_lines() -> [&'static str; 5] {
    [
        "▄▀▀█ ▄▀▀█      ▄▀▀▄ ▄▀▀▀ ▄▀▀█ █  █ ▀█▀",
        "▓▄▄  ▓         ▓▄▄▓ ▓ ▀▓ ▓▄▄  ▓▄ ▓  ▓ ",
        "▒     ▀▀▄ ▀▀▀▀ ▒  ▒ ▒  ▒ ▒    ▒ ▀▒  ▒ ",
        "░    ░  ░      ░  ░ ░  ░ ░  ▄ ░  ░  ░ ",
        "▀    ▀▀▀       ▀  ▀  ▀▀▀  ▀▀▀ ▀  ▀  ▀ ",
    ]
}

/// The mode as a field: what the status row shows, and what the old header's facts line
/// used to.
pub fn mode_field(mode: Mode) -> String {
    format!("模式 {}", mode_label(mode))
}

// ---------------------------------------------------------------------------
// The shell: the sidebar's tabs, the status row and the rail
// (`.scratch/tui-sidebar/spec.md` §3, §5, §6)
// ---------------------------------------------------------------------------

/// The sidebar's tab labels, in the order they are drawn (spec §3).
pub const TAB_USAGE: &str = "调用量";
pub const TAB_TRACE: &str = "轨迹";
pub const TAB_FILES: &str = "文件";

/// What a tab whose page is not built yet says. A sentence rather than a blank
/// panel, so the reader knows it is not done rather than broken, and naming the
/// ticket makes the reason checkable (spec §3).
pub fn tab_placeholder() -> &'static str {
    "此页尚未实现（另有票在跟）"
}

/// The status row's short form of how full the model's window is: `上下文 6%`, or
/// `上下文 —` before a call has reported its input tokens.
///
/// Short because the status row shares its one line with the model and the mode:
/// the pair, the ceiling and the percentage in brackets are the sidebar's field
/// (spec §5).
pub fn context_share(used: Option<u64>, usable: u64) -> String {
    match used {
        Some(used) => format!(
            "{} {}%",
            PANEL_CONTEXT,
            used.saturating_mul(100) / usable.max(1)
        ),
        None => format!("{} {}", PANEL_CONTEXT, PANEL_UNKNOWN),
    }
}

/// The status row: `模型 … │ 模式 … │ 上下文 …%`, with the width ladder folded in.
///
/// Three rungs, and the order is the point: the **model** goes first (it is the
/// longest segment and does not change within a session), then the **mode**, and
/// what is left is the one reading that answers "how much room is there" — still
/// carrying its label, so a bare `6%` never appears unexplained (spec §2, §5).
///
/// There is deliberately **no** rung that takes the row away. The width that would
/// take is narrower than [`super::layout::MIN_WIDTH`], so the row is always drawn;
/// a `width` too small even for the last rung is the painter's to truncate.
pub fn status_row(model: &str, mode: &str, share: &str, width: usize) -> String {
    let segment = |text: &str| format!(" {text} ");
    let full = format!(
        "{}│{}│{}",
        segment(&format!("{PANEL_MODEL} {model}")),
        segment(mode),
        segment(share)
    );
    if usize::from(full.cell_width()) <= width {
        return full;
    }
    let two = format!("{}│{}", segment(mode), segment(share));
    if usize::from(two.cell_width()) <= width {
        return two;
    }
    segment(share)
}

/// The rail's three glyphs: an ordinary unit, the focused unit, and the mark for
/// the units the column had no room for (spec §4).
pub const RAIL_CELL: &str = "┊";
pub const RAIL_FOCUS: &str = "┃";
pub const RAIL_TRUNCATED: &str = "⋮";

/// The `Mode` a session runs under, named in Chinese.
pub fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Readonly => "只读",
        Mode::Ask => "询问",
        Mode::Auto => "自动",
        Mode::Plan => "计划",
    }
}

/// The startup banner: what this session is, in Chinese labels.
pub fn banner(session: &str, model: &str, mode: Mode, dir: &str, continued: bool) -> String {
    let tail = if continued { "（已继续）" } else { "" };
    format!(
        "fs-agent：会话 {session} · 模型 {model} · 模式 {} · {dir}{tail}",
        mode_label(mode)
    )
}

/// `/undo` with nothing to roll back.
pub fn nothing_to_undo() -> &'static str {
    "没有可撤销的修改"
}

/// A slash command as a menu offers it and a hint names it.
///
/// This is the **human-facing** list, the one the `/` menu and the unknown-command
/// text are built from. What a submission *means* stays the loop's parser; a name
/// here that the parser did not know would be a bug, not a policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// The name without its slash, exactly as it must be typed.
    pub name: &'static str,
    /// One line saying what it does.
    pub description: &'static str,
}

/// The built-in slash commands, in the order every list shows them.
pub static BUILT_IN_COMMANDS: [Command; 5] = [
    Command {
        name: "undo",
        description: "回滚上一次编辑",
    },
    Command {
        name: "plan",
        description: "进入硬计划模式",
    },
    Command {
        name: "endplan",
        description: "退出硬计划模式",
    },
    Command {
        name: "discuss",
        description: "起一场多角色讨论（用本会话的上下文）",
    },
    Command {
        name: "quit",
        description: "退出会话",
    },
];

/// The built-ins as one hint line: `可用：/undo、/plan、/endplan、/discuss、/quit`.
///
/// One generator, so the menu's list and the unknown-command text cannot drift
/// apart.
pub fn built_in_names() -> String {
    format!(
        "可用：{}",
        BUILT_IN_COMMANDS
            .iter()
            .map(|command| format!("/{}", command.name))
            .collect::<Vec<_>>()
            .join("、")
    )
}

/// A slash-command the loop does not know. It names the built-ins, and — when
/// there are any — the skills the user can load by name, so `/` stays
/// discoverable.
pub fn unknown_command(command: &str, skills: &[&str]) -> String {
    let built_ins = built_in_names();
    const LISTED: usize = 8;
    if skills.is_empty() {
        return format!("未知命令 {command}（{built_ins}，或直接输入 /<技能名>）");
    }
    let mut names: Vec<String> = skills
        .iter()
        .take(LISTED)
        .map(|name| format!("/{name}"))
        .collect();
    if skills.len() > LISTED {
        names.push("…".to_owned());
    }
    format!(
        "未知命令 {command}（{built_ins}；技能：{}）",
        names.join("、")
    )
}

/// The user loaded a skill by name, to run right away.
pub fn skill_loaded(name: &str) -> String {
    format!("已加载技能 {name}")
}

/// A bare `/<skill>`: the body is loaded **and the turn starts**, because the body
/// is the instruction. A skill that waited for a task would be a command the user
/// had to invoke twice.
pub fn skill_started(name: &str) -> String {
    format!("已加载技能 {name}，按技能正文开始")
}

/// `/plan` succeeded.
pub fn plan_entered() -> &'static str {
    "已进入计划模式"
}

/// `/endplan` succeeded.
pub fn plan_exited() -> &'static str {
    "已退出计划模式"
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

/// An argument the command does not recognize.
pub fn unknown_argument(arg: &str) -> String {
    format!("未知参数 {arg}")
}

/// A flag that was given without the value it needs.
pub fn needs_value(flag: &str) -> String {
    format!("{flag} 需要一个值")
}

/// A flag that needs a number but got something else.
pub fn needs_number(flag: &str, value: &str) -> String {
    format!("{flag} 需要一个数字，得到 {value}")
}

/// A flag that needs a path.
pub fn needs_path(flag: &str) -> String {
    format!("{flag} 需要一个路径")
}

/// A flag that needs a model id.
pub fn needs_model(flag: &str) -> String {
    format!("{flag} 需要一个模型 id")
}

/// More positional arguments than the command takes.
pub fn extra_argument(arg: &str) -> String {
    format!("多余的参数 {arg}")
}

/// `--plain` and `--tui` asked for at once.
pub fn renderers_mutually_exclusive() -> &'static str {
    "--plain 与 --tui 互斥：每个进程只有一个渲染器"
}

/// `sessions` with no verb.
pub fn sessions_needs_verb() -> &'static str {
    "sessions 需要一个子命令"
}

/// A `sessions` verb that does not exist.
pub fn unknown_sessions_verb(verb: &str) -> String {
    format!("未知的 sessions 子命令 {verb}")
}

/// `sessions show` without an id.
pub fn show_needs_id() -> &'static str {
    "sessions show 需要会话 id"
}

/// `sessions replay` without an id.
pub fn replay_needs_id() -> &'static str {
    "sessions replay 需要会话 id"
}

/// `sessions replay` without `--speaker`.
pub fn replay_needs_speaker() -> &'static str {
    "sessions replay 需要 --speaker"
}

/// `sessions stats` without an id.
pub fn stats_needs_id() -> &'static str {
    "sessions stats 需要会话 id"
}

/// A session id that no bucket holds. The id and the search label stay verbatim.
pub fn no_session(id: &str, where_: &str) -> String {
    format!("{where_} 中没有会话 {id}")
}

/// The label for the bucket `sessions` searched (and the scan that widens it).
pub fn session_search_label(cwd: &str) -> String {
    format!("{cwd}（或任何其他桶）")
}

/// A stored file that could not be read. The path and detail stay verbatim.
pub fn cannot_read(path: &str, detail: &str) -> String {
    format!("无法读取 {path}：{detail}")
}

// ---------------------------------------------------------------------------
// Startup refusals and failures
// ---------------------------------------------------------------------------

/// The refusal to run as root (spec §20). The detail stays English: it names the
/// system call and the uid, which is the diagnostic clue.
pub fn root_refusal() -> &'static str {
    "拒绝以 root（euid 0）启动：本工具的每一条护栏都假定最坏情况留在你的工作区内，\
     而 root 的一次误判是系统级的。请用你的普通用户运行；没有绕过开关。"
}

/// The async runtime could not be built.
pub fn startup_runtime(detail: &str) -> String {
    format!("无法启动异步运行时：{detail}")
}

/// `prune --dry-run` naming a session it would remove. The id and path stay
/// verbatim.
pub fn prune_would_remove(id: &str, dir: &str) -> String {
    format!("将删除 {id}（{dir}）")
}

/// `prune` naming a session it removed.
pub fn prune_removed(id: &str, dir: &str) -> String {
    format!("已删除 {id}（{dir}）")
}

/// Configuration could not be read.
pub fn startup_config(detail: &str) -> String {
    format!("无法读取配置：{detail}")
}

/// Neither `XDG_DATA_HOME` nor `HOME` is set, so a session has nowhere to live.
pub fn startup_no_session_store() -> &'static str {
    "既没有设置 XDG_DATA_HOME 也没有设置 HOME，会话无处存放"
}

/// The session store could not be read.
pub fn startup_store_read(detail: &str) -> String {
    format!("无法读取会话存储：{detail}")
}

/// The session store could not be created in.
pub fn startup_store_create(detail: &str) -> String {
    format!("无法创建会话：{detail}")
}

/// The session store could not be pruned.
pub fn startup_store_prune(detail: &str) -> String {
    format!("无法清理会话存储：{detail}")
}

/// There is no session in `dir` to continue.
pub fn startup_no_session_to_continue(dir: &str) -> String {
    format!("{dir} 中没有可继续的会话")
}

/// The current directory could not be determined.
pub fn startup_cwd(detail: &str) -> String {
    format!("无法确定当前目录：{detail}")
}

/// No configured provider has a key.
pub fn probe_no_key() -> &'static str {
    "没有任何已配置的 provider 带密钥。请导出 MOONSHOT_API_KEY 和/或 \
     DEEPSEEK_API_KEY，或在 config.toml 的 [providers.*] 下设置 `api_key`。"
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A session-level failure (spec §2): the code is explained in Chinese, the
/// durable detail is passed through verbatim. The detail is a diagnostic clue and
/// the stream only appends, so it is frozen at write time (ADR 0001).
pub fn session_error(code: &str, detail: &str) -> String {
    format!("[会话错误：{}] {detail}", session_error_code(code))
}

/// The Chinese explanation of a session error code. An unknown code is shown as
/// itself: a new code must not be silently dropped.
pub fn session_error_code(code: &str) -> &str {
    match code {
        "discussion_failed" => "讨论失败",
        "synthesis_failed" => "合成失败",
        other => other,
    }
}

/// A loop-level failure, with the variant named in Chinese and the detail passed
/// through.
pub fn error_report(error: &crate::Error) -> String {
    match error {
        crate::Error::Io(error) => format!("事件流读写失败：{error}"),
        crate::Error::Discussion(detail) => format!("讨论无法组装：{detail}"),
        crate::Error::Undo(detail) => format!("撤销失败：{detail}"),
        crate::Error::Skill(detail) => format!("技能加载失败：{detail}"),
    }
}

// ---------------------------------------------------------------------------
// `sessions` output
// ---------------------------------------------------------------------------

/// The `sessions ls` column names, in order. Padded by the caller, which knows
/// the terminal's column budget.
pub fn ls_columns() -> [&'static str; 7] {
    ["标识", "工作区", "开始", "用量", "轮次", "消息", "结束"]
}

/// `sessions ls` found nothing in the bucket.
pub fn no_sessions() -> &'static str {
    "本桶中没有会话"
}

/// The `ls` ending column for a session that never ended.
pub fn open_session() -> &'static str {
    "未结束"
}

/// A round-grouped section line in `sessions show`.
pub fn round_group(round: u32, mode: Option<RoundMode>) -> String {
    match mode {
        Some(mode) => round_section(round, mode),
        None => format!("── 第 {round} 轮 ──"),
    }
}

/// The pre-round section of `sessions show`.
pub fn session_group() -> &'static str {
    "── 会话 ──"
}

/// A single round's label in the `--files` view.
pub fn round_label(round: u32) -> String {
    format!("第 {round} 轮")
}

/// A failed tool call's reason, as narration.
pub fn tool_error(error: &str) -> String {
    format!("错误：{error}")
}

/// The `sessions replay` label for a system message.
pub fn replay_system() -> &'static str {
    "[系统]"
}

/// The `sessions replay` label for a user message, naming the speaker when the
/// projection recorded one.
pub fn replay_user(name: Option<&str>) -> String {
    match name {
        Some(name) => format!("[用户 {name}]"),
        None => "[用户]".to_owned(),
    }
}

/// The `sessions replay` label for an assistant message.
pub fn replay_assistant() -> &'static str {
    "[助手]"
}

/// The `sessions replay` label for a tool result.
pub fn replay_tool(tool_call_id: &str) -> String {
    format!("[工具 {tool_call_id}]")
}

/// A tool call inside a replayed assistant message. The arguments are the wire's
/// own JSON and stay verbatim.
pub fn replay_tool_call(name: &str, arguments: &str) -> String {
    format!("→ 调用 {name}({arguments})")
}

// --- `sessions stats` labels ---

/// The session-wide totals line.
pub fn stats_session(
    tokens: u64,
    calls: usize,
    messages: usize,
    rounds: usize,
    cost: &str,
) -> String {
    format!("会话：{tokens} token，{calls} 次调用，{messages} 条消息，{rounds} 轮{cost}")
}

/// The priced tail of the session line.
pub fn stats_cost(cost: f64, model: &str) -> String {
    format!("，${cost:.6}（按 {model} 计价）")
}

/// The unpriced tail, naming the missing price entry: an unregistered model shows
/// as having no price, never as zero (CONTEXT.md: PriceTable).
pub fn stats_no_cost(model: &str) -> String {
    format!("，无价格（没有 [pricing.{model}] 条目）")
}

/// One speaker's spend row.
pub fn stats_speaker(speaker: &str, tokens: u64, calls: usize, hit: &str, cost: &str) -> String {
    format!("  {speaker} {tokens} token  {calls} 次调用  命中 {hit}{cost}")
}

/// The absence picture across the debate rounds.
pub fn stats_absence(one_sided: usize, rounds: usize, rate: &str) -> String {
    format!("缺席：{rounds} 轮辩论中有 {one_sided} 轮只有一方作答{rate}")
}

/// One speaker's absence count.
pub fn stats_absent(name: &str, count: usize) -> String {
    format!("  缺席：{name} ×{count}")
}

/// The edit ladder's outcome.
pub fn stats_edits(succeeded: usize, failed: usize) -> String {
    format!("编辑：{succeeded} 次成功，{failed} 次匹配失败")
}

/// One match-ladder downgrade level.
pub fn stats_match_level(name: &str, count: usize) -> String {
    format!("  匹配层级 {name}：{count}")
}

/// The two guardrails' refusal counts.
pub fn stats_guards(read_before_write: usize, invalidated: usize) -> String {
    format!("护栏：写前必读 {read_before_write}，读集失效 {invalidated}")
}

/// Executors dispatched and closed.
pub fn stats_executors(spawned: usize, finished: usize) -> String {
    format!("执行者：派出 {spawned}，收尾 {finished}")
}

/// One executor stopping reason.
pub fn stats_executor_reason(name: &str, count: usize) -> String {
    format!("  收尾 {name}：{count}")
}

/// What the mounted hooks did.
pub fn stats_hooks(
    executed: usize,
    pre: usize,
    post: usize,
    feedback: usize,
    failed: usize,
) -> String {
    format!("钩子：执行 {executed}（前 {pre}，后 {post}），反馈 {feedback}，失败 {failed}")
}

/// Permission questions asked, with an already-assembled decisions tail.
pub fn stats_permissions(asked: usize, decided: &str) -> String {
    format!("权限：询问 {asked}{decided}")
}

/// One decision in the permissions tail.
pub fn stats_decision(count: usize, name: &str) -> String {
    format!("{count} {name}")
}

/// The divergence rate across the debate rounds.
pub fn stats_divergences(divergences: usize, rounds: usize, rate: &str) -> String {
    format!("分歧：{divergences}/{rounds}{rate}")
}

/// The prefix before the per-round list.
pub fn stats_rounds_prefix() -> &'static str {
    "；轮次："
}

/// One round in the per-round list. `ended` is already formatted, or empty.
pub fn stats_round(round: u32, mode: RoundMode, calls: usize, ended: &str) -> String {
    format!("#{round} {} {calls} 次调用{ended}", round_mode(mode))
}

/// A round's closing reason in the per-round list.
pub fn stats_round_ended(reason: StopReason) -> String {
    format!("，结束于 {}", stop_reason(reason))
}

/// One stopping reason count.
pub fn stats_stop(name: &str, count: usize) -> String {
    format!("  停止 {name}：{count}")
}

// ---------------------------------------------------------------------------
// Help
// ---------------------------------------------------------------------------
//
// Help returns a string instead of printing, so the wording layer covers it and a
// test can assert it; `main` owns the printing.

/// The top-level `--help`.
pub fn help_main() -> String {
    format!(
        "fs-agent {}\n\n  \
         usage: fs-agent [--plain|--tui] [--continue] [--config PATH] [--model ID] [--cwd PATH]\n         \
         fs-agent discuss [--plain|--tui] [--config PATH] [--cwd PATH] \"问题\"\n         \
         fs-agent probe [--config PATH] [--model ID]...\n         \
         fs-agent prune [--keep N] [--cwd PATH] [--dry-run]\n         \
         fs-agent sessions <ls|show|replay|stats> [options]\n\n  \
         不带子命令时，fs-agent 在当前工作区启动一个交互会话：终端上用 TUI 渲染，否则用 \
         plain 转录（--plain / --tui 可强制其一）。--continue 继续本工作区最新的会话。\
         discuss 起一次多角色讨论：两个讨论者各自独立作答，只在结论冲突时开一轮定向第二轮，\
         最后由合成器画出共识 / 分歧 / 未决（见 `fs-agent discuss --help`）。\
         probe 对每个已配置的模型驱动一次真实回合，并在同一会话里再跑一次，然后打印归一化\
         后的用量，以便看到前缀缓存是否命中。prune 手动删除本工作区的会话目录，保留最新的 \
         N 个（默认 1）。sessions 只从会话自己的事件流回答关于一个已结束会话的问题\
         （ls / show / replay / stats；见 `fs-agent sessions --help`）。配置位于 \
         ~/.config/fs-agent/config.toml（支持 XDG）；项目里的 .env 永远不会被加载。",
        env!("CARGO_PKG_VERSION")
    )
}

/// `fs-agent discuss` with no `[discussion]` table: what to write instead.
///
/// There is deliberately no default roster: picking two models for someone would
/// spend their money on a configuration they never chose.
pub fn discussion_no_roster() -> &'static str {
    "config.toml 里没有 [discussion]：讨论需要两个讨论者，加 `[discussion]` 与 \
     `debaters = [\"kimi-k3\", \"deepseek-v4-pro\"]`（至少两个池子成员；不同厂商最好， \
     同厂商甚至同一个模型也能跑，只是多样性会弱），见 `fs-agent discuss --help`"
}

/// `fs-agent discuss` with nothing to ask.
pub fn discuss_needs_question() -> &'static str {
    "discuss 需要一个问句：`fs-agent discuss \"问题\"`，或者把问题从 stdin 传进来"
}

/// `/discuss` with no question of its own *and* no question in the session yet.
///
/// A bare `/discuss` discusses the last thing the user asked; in a session where they
/// have not asked anything, there is nothing to discuss.
pub fn discuss_needs_in_session_question() -> &'static str {
    "`/discuss` 没有可讨论的题目：写成 `/discuss 你的问题`，或先在这个会话里问一句，\
     不带题目的 `/discuss` 会拿最后一个问题去讨论"
}

/// The interactive session's `--help`.
pub fn help_interactive() -> String {
    "fs-agent [options]\n\n  \
     在当前工作区启动一个交互会话。命令：/undo 回滚上一次编辑，/plan 与 /endplan 控制\
     硬计划模式，/quit 退出；输入 /技能名 直接运行一个技能（可带任务，例如 \
     `/ask-matt 帮我看一下`），包括标了 `disable-model-invocation: true` 的技能。\
     输入 / 会弹出补全窗口，列出全部命令与技能。\
     `/discuss [--debaters A,B] [问题]` 起一场多角色讨论：两个讨论者用**本会话的上下文**\
     各自作答，只在结论冲突时开一轮定向第二轮，最后由合成器画出共识 / 分歧 / 未决；\
     `--debaters 保守,激进` 指定抽池子里的哪两个（不写就随机抽两个），不带问题就用本会话\
     最后一个问题。讨论的事件写进同一个会话，`sessions show` 能一起复盘。\
     TUI 里 Esc 取消正在跑的回合（或正在跑的讨论）；Shift+Tab 切换计划模式。\n\n  \
     --plain            使用 plain 转录（不进 raw 模式）\n  \
     --tui              使用终端界面（全屏外壳）\n  \
     --continue, -c     继续本工作区最新的会话\n  \
     --config PATH      要加载的配置文件\n  \
     --model ID         要运行的模型（默认：配置里的 default_model）\n  \
     --cwd PATH         工作区（默认：当前目录）"
        .to_owned()
}

/// `discuss --help`.
pub fn help_discuss() -> String {
    "fs-agent discuss [--plain|--tui] [--config PATH] [--cwd PATH] [--debaters A,B] \"问题\"\n\n  \
     起一次多角色讨论：两个讨论者从配置的 `[discussion] debaters` **池子**里抽——\
     `--debaters 保守,激进` 指定抽哪两个（名字来自池子里的 `name`），不写就随机抽两个。\
     池子成员是「名字 + 模型」：不同厂商最好，同厂商甚至同一个模型也允许，只是多样性会弱\
     （同一个模型的两位必须各起一个名字，名字就是它在流上的身份）。抽到的两位就同一个问题各自独立作答，各自给一行 `CONCLUSION:`；只有结论冲突时才再开\
     一轮定向第二轮；最后合成器做一次单发调用，产出「共识 / 分歧（含各自成立的前提）\
     / 未决」——它画出选项空间，不替你收敛。一次讨论是 3 次调用（无分歧）或 5 次\
     （有分歧）。合成器沿用 `[routing].synthesizer_model`（没配就用第一个讨论者的模型）；\
     讨论者永远不会被路由到弱模型。\n\n  \
     问句没写在命令行上时：stdin 是终端就问你要一行，是管道就读到结尾（`echo 问题 | \
     fs-agent discuss`）。讨论落在真会话里，`fs-agent sessions show <id>` 可以复盘（TUI \
     退出后转录不留，完整记录在会话日志里）。\n\n  \
     --plain            使用 plain 转录（讨论过程走 stderr，合成产物走 stdout）\n  \
     --tui              使用终端界面（终端上默认就是它）\n  \
     --config PATH      要加载的配置文件\n  \
     --cwd PATH         工作区（默认：当前目录）"
        .to_owned()
}

/// `probe --help`.
pub fn help_probe() -> String {
    "fs-agent probe [--config PATH] [--model ID]...\n\n  \
     对每个模型在同一会话里发送两次真实回合，并打印每次的 input/output/cached/miss。\
     不带 --model 时探测每一个 provider 有密钥的模型。"
        .to_owned()
}

/// `prune --help`.
pub fn help_prune() -> String {
    "fs-agent prune [--keep N] [--cwd PATH] [--dry-run]\n\n  \
     删除一个工作区（当前目录，或 --cwd）的会话目录。保留最新的 N 个会话（默认 1：即 \
     --continue 会继续的那个）。一个会话就是一个目录，所以删除是整会话的；--dry-run \
     只列出将删除的内容。除此之外没有任何东西会删除会话。"
        .to_owned()
}

/// `sessions --help`.
pub fn help_sessions() -> String {
    "fs-agent sessions <verb> [options]\n\n  \
     ls [--all] [--cwd PATH] [--limit N] [--json]\n      \
     列出本工作区的会话（--all 扫描每个桶），最新的在前。\n  \
     show <id> [--round N] [--speaker X] [--kind K] [--tool T] [--only-error] [--files] [--json]\n      \
     按轮次分组的转录，工具调用与结果合并；--files 改为显示工作区对象视图\n      \
     （它遵守 --round/--speaker/--tool）。\n  \
     replay <id> --speaker X [--round N] [--model ID] [--json]\n      \
     只从事件流重算某一次调用发给 provider 的内容。\n  \
     stats <id> [--model ID] [--json]\n      \
     固定的指标集（token、费用、缺席率、编辑匹配层级的降级）。\n\n  \
     标准输出只放结果，诊断走标准错误。"
        .to_owned()
}
