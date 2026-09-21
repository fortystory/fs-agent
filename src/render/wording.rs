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

/// The TUI's permission input line: what would run, and the keys that answer it.
pub fn permission_prompt(tool_name: &str, args: &str) -> String {
    format!(
        "{}？{PERMISSION_CHOICES} ",
        permission_asked(Some(tool_name), args)
    )
}

/// The plain console's permission input line, which also has room for the gate's
/// reason.
pub fn permission_prompt_with_context(tool_name: &str, args: &str, reason: &str) -> String {
    format!(
        "{}？原因：{reason} {PERMISSION_CHOICES} ",
        permission_asked(Some(tool_name), args)
    )
}

/// The plan-mode conflict input line.
pub fn plan_conflict_prompt(path: &str) -> String {
    format!("{path} 已存在：{PLAN_CHOICES} ")
}

/// The keys that answer a permission question. Key names stay literal.
const PERMISSION_CHOICES: &str = "[y] 允许 / [a] 总是允许 / [n] 拒绝";

/// The keys that answer a plan-mode conflict. Key names stay literal.
const PLAN_CHOICES: &str = "[o] 覆盖 / [a] 追加 / [k] 保留";

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
    format!("[上下文注入：{}]", context_source(source))
}

/// The Chinese name of a context source.
pub fn context_source(source: ContextSource) -> &'static str {
    match source {
        ContextSource::AgentsMd => "AGENTS.md",
        ContextSource::SkillsCatalog => "技能清单",
        ContextSource::Skill => "技能",
        ContextSource::PlanMode => "计划模式",
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

/// The live key hints, in the order they are shown: the most used first, the way
/// out last. Key names stay literal; only the action is Chinese.
const KEY_HINTS: [&str; 6] = [
    "enter 发送",
    "ctrl-j 换行",
    "esc 取消",
    "shift+tab 计划",
    "PgUp/PgDn 滚动",
    "ctrl-c 退出",
];

/// The hint that is never dropped: a terminal where the way out cannot be found
/// is worse than one that shows fewer hints.
const EXIT_HINT: &str = "ctrl-c 退出";

/// The status line for a terminal `width` **columns** wide.
///
/// The hints fill from the left with [`EXIT_HINT`] reserved at their end, and the
/// state word is placed in front of them only if it still fits — so a narrow
/// terminal keeps its way out *and* the hints that explain the keys, and gives up
/// `就绪` instead of `ctrl-j 换行`. The measured ladder is three items at 40
/// columns, four at 60, five at 80 and six at 120 (spec §10).
pub fn status_line(busy: bool, width: u16) -> String {
    let exit = EXIT_HINT.cell_width();
    let mut hints = String::new();
    for hint in &KEY_HINTS[..KEY_HINTS.len() - 1] {
        let candidate = if hints.is_empty() {
            (*hint).to_owned()
        } else {
            format!("{hints} · {hint}")
        };
        if candidate.cell_width() + " · ".cell_width() + exit > width {
            break;
        }
        hints = candidate;
    }
    let run = if hints.is_empty() {
        EXIT_HINT.to_owned()
    } else {
        format!("{hints} · {EXIT_HINT}")
    };
    let with_state = format!("{} · {run}", status_word(busy));
    if with_state.cell_width() <= width {
        with_state
    } else {
        run
    }
}

/// Everything a terminal smaller than the minimum shows, so the reason is a
/// sentence rather than an empty screen (spec §2).
pub fn too_small(width: u16, height: u16) -> String {
    format!("终端太小：至少 {width}×{height}")
}

/// The header's identity field: the program and the version it was built from.
pub fn identity() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

/// The header's mode field.
pub fn mode_field(mode: Mode) -> String {
    format!("模式 {}", mode_label(mode))
}

/// The header's clock, at the minute: a second hand would redraw the frame sixty
/// times a minute for no one (spec §10).
pub fn clock(now: &chrono::DateTime<chrono::Local>) -> String {
    now.format("%Y-%m-%d %H:%M").to_string()
}

/// The clock for a header with only one line, where the date is the first thing to
/// go (spec §2).
pub fn clock_short(now: &chrono::DateTime<chrono::Local>) -> String {
    now.format("%H:%M").to_string()
}

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

/// A slash-command the loop does not know. It names the built-ins, and — when
/// there are any — the skills the user can load by name, so `/` stays
/// discoverable.
pub fn unknown_command(command: &str, skills: &[&str]) -> String {
    const BUILT_INS: &str = "可用：/undo、/plan、/endplan、/quit";
    const LISTED: usize = 8;
    if skills.is_empty() {
        return format!("未知命令 {command}（{BUILT_INS}，或直接输入 /<技能名>）");
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
        "未知命令 {command}（{BUILT_INS}；技能：{}）",
        names.join("、")
    )
}

/// The user loaded a skill by name, with a task to run.
pub fn skill_loaded(name: &str) -> String {
    format!("已加载技能 {name}")
}

/// A bare `/<skill>`: the body is loaded and the loop waits for the task, so the
/// transcript never shows a user message the user did not type.
pub fn skill_loaded_waiting(name: &str) -> String {
    format!("已加载技能 {name}；请输入你的任务。")
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
         fs-agent probe [--config PATH] [--model ID]...\n         \
         fs-agent prune [--keep N] [--cwd PATH] [--dry-run]\n         \
         fs-agent sessions <ls|show|replay|stats> [options]\n\n  \
         不带子命令时，fs-agent 在当前工作区启动一个交互会话：终端上用 TUI 渲染，否则用 \
         plain 转录（--plain / --tui 可强制其一）。--continue 继续本工作区最新的会话。\
         probe 对每个已配置的模型驱动一次真实回合，并在同一会话里再跑一次，然后打印归一化\
         后的用量，以便看到前缀缓存是否命中。prune 手动删除本工作区的会话目录，保留最新的 \
         N 个（默认 1）。sessions 只从会话自己的事件流回答关于一个已结束会话的问题\
         （ls / show / replay / stats；见 `fs-agent sessions --help`）。配置位于 \
         ~/.config/fs-agent/config.toml（支持 XDG）；项目里的 .env 永远不会被加载。",
        env!("CARGO_PKG_VERSION")
    )
}

/// The interactive session's `--help`.
pub fn help_interactive() -> String {
    "fs-agent [options]\n\n  \
     在当前工作区启动一个交互会话。命令：/undo 回滚上一次编辑，/plan 与 /endplan 控制\
     硬计划模式，/quit 退出；输入 /技能名 直接加载一个技能（可带任务，例如 \
     `/ask-matt 帮我看一下`），包括标了 `disable-model-invocation: true` 的技能。\
     TUI 里 Esc 取消正在跑的回合；Shift+Tab 切换计划模式。\n\n  \
     --plain            使用 plain 转录（不进 raw 模式）\n  \
     --tui              使用终端界面（inline viewport）\n  \
     --continue, -c     继续本工作区最新的会话\n  \
     --config PATH      要加载的配置文件\n  \
     --model ID         要运行的模型（默认：配置里的 default_model）\n  \
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
