//! 措辞层：每一条给人看的短语，都在一处。
//!
//! 从一个领域值到一个中文短语的纯函数。它不知道风格（颜色与加粗留在画家那里），也不知道
//! 结构（plain 渲染器的逐行前缀、TUI 的缩进与 `sessions show` 的轮次分组，是它们各自
//! 调用方的事）。硬编码中文、没有运行时 locale：换一种语言就是改这个模块
//! （spec §Implementation Decisions）。
//!
//! **模型可见 / 进流的文本不在这里**：讨论者与合成器的 system prompt、投影的轮前缀、
//! `AgentError` 的 message，以及 fs-agent 自己那些工具结果文本，都由产生它们的地方写成
//! 中文（ADR 0005 起 —— 在那之前它们冻结在英文，见 ADR 0001），不靠这个模块拼。

use std::path::Path;

use ratatui::buffer::CellWidth;

use crate::events::{
    hook_format, ContextSource, Decision, DecisionSource, HistoryReason, RoundMode, SpeakerId,
    StopReason, Usage,
};
use serde_json::Value;

use crate::permissions::{Escalation, Mode};
use crate::provider::FinishReason;
use crate::render::width::{text_columns, truncate_columns};

/// 一个轮次模式给人看的标签。
///
/// 讨论协议的合成 prompt 用的是同一批词，所以界面与指令对轮次的命名是一致的。
pub fn round_mode(mode: RoundMode) -> &'static str {
    match mode {
        RoundMode::Independent => "独立首轮",
        RoundMode::Targeted => "定向第二轮",
        RoundMode::Synthesis => "合成",
    }
}

/// 两个人类画家都打的轮次分节行。
pub fn round_section(round: u32, mode: RoundMode) -> String {
    format!("── 第 {round} 轮（{}）──", round_mode(mode))
}

/// 开启一个 agent 回合的叙述。
pub fn turn_started(iteration: u32) -> String {
    format!("回合开始（第 {iteration} 次迭代）")
}

/// 收尾一个 agent 回合的叙述，原因用中文摊开来说。
pub fn turn_ended(reason: StopReason) -> String {
    format!("回合结束：{}", stop_reason(reason))
}

/// 一个讨论轮次收尾。
pub fn round_ended(round: u32, reason: StopReason) -> String {
    format!("第 {round} 轮结束：{}", stop_reason(reason))
}

/// `fs-agent discuss` 在讨论结束、屏幕变回来之后报的那段：为什么停、实际跑了几轮，以及
/// 谁缺席。
///
/// 点名缺席的那一方，是因为它是事件流记了、读的人却容易漏掉的那一件事：只有一方作答的
/// 一轮，不是达成一致的一轮。
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

/// 一场会话怎么再跑起来 —— 给 alt screen 恢复之后打的那一行：TUI 的转录活不过这个
/// 进程，所以会话 id 才是那个持久的答案。
///
/// `discuss` 与交互式退出共用它：两处都是「终端交还之后，给人一条能直接粘的命令」
/// （`.scratch/exit-gesture/spec.md` §5）。给的命令是 **`fs-agent -c <id>`** —— 它现在吃 id，
/// 于是这一行真的能直接粘回终端里（在那之前只有按 id 查的 `sessions show`）。
pub fn session_receipt(session_id: &str) -> String {
    format!("会话 {session_id}；接着跑：fs-agent -c {session_id}")
}

/// `-c <id>` 续上了一场面**别的工作区**的会话：这一趟的工作目录跟着那场会话走了。
pub fn session_followed(dir: &str) -> String {
    format!("接着跑的是 {dir} 里的那场会话")
}

/// 正在讨论的那一对，给只有一个字段能点名它们的前端。
pub fn discussion_pair(first: &str, second: &str) -> String {
    format!("{first} × {second}")
}

/// 一个讨论者在通告里的写法：`保守（deepseek-v4-pro）`。
///
/// 名字旁边显示模型，因为名字是用户挑的身份、而模型才是真正作答的那个 —— 有了池子之后，
/// 两者在一次讨论里可以不一样。简写那种情况（讨论者的名字就是它的模型）只说一遍。
pub fn debater_label(name: &str, model: &str) -> String {
    if name == model {
        name.to_owned()
    } else {
        format!("{name}（{model}）")
    }
}

/// 形状不对的 `--debaters`。
pub fn needs_two_debaters(value: &str) -> String {
    format!("--debaters 需要两个名字（逗号分隔，例如 `--debaters 保守,激进`），得到 `{value}`")
}

/// `--debaters` 点了池子里没有的名字。
pub fn unknown_debater(name: &str, pool: &[&str]) -> String {
    format!(
        "池子里没有叫 `{name}` 的讨论者；可用：{}",
        pool.iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join("、")
    )
}

/// `/discuss` 即将开跑：哪两个模型，以及要问它们什么。
///
/// 点名题目，因为它可能不是打进来的 —— 一个裸 `/discuss` 把会话的最后一个问题摆给讨论
/// 者，而用户应该先看到那是哪个问题，再让两个模型开始作答。
pub fn discussion_starting(first: &str, second: &str, question: &str) -> String {
    format!(
        "开始讨论：{first} × {second}；题目：{}",
        first_non_empty_line(question)
    )
}

/// 一段文本的第一条非空行、去掉首尾空白 —— 一个问题或一项任务可以是一整段，而通告只有
/// 一行。
fn first_non_empty_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

/// 一份两个讨论者是同一个模型的名册的提示行。
///
/// 允许 —— 一个订阅到期不该是完全没有讨论的理由 —— 但这个设计的前提是两个独立的判断，
/// 而同一个模型问两遍，中间只剩采样噪声。
pub fn discussion_same_model(model: &str) -> String {
    format!("提示：两个讨论者都是 {model}——同一个模型问两遍，剩下的差异只有采样噪声")
}

/// 一份同一厂商、两个模型的名册的提示行。
pub fn discussion_one_vendor(first: &str, second: &str) -> String {
    format!("提示：两个讨论者来自同一厂商（{first} × {second}），多样性比设计假设的弱")
}

/// 命令行上没给问题时，`fs-agent discuss` 在终端上打的那个提示。
pub fn question_prompt() -> &'static str {
    "问题> "
}

/// 整场会话收尾。
pub fn session_ended(reason: StopReason) -> String {
    format!("会话结束：{}", stop_reason(reason))
}

/// 一个停止点的中文短语。
///
/// 一份显式映射，于是 debug 格式化出来的枚举永远不会到达界面
/// （spec §Implementation Decisions）。
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

/// 一次工具调用的一行摘要。工具名与它的参数是结构、原样保留；只有动词是中文。
pub fn tool_call(tool: &str, args: &str) -> String {
    format!("调用 {tool}({args})")
}

/// 一条裁到 `max_chars` 的工具结果，切口用中文标出来，好让人分得清一条完整的短结果与
/// 一条被省略的长结果。
pub fn tool_output_preview(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push_str("…（结果已省略）");
    out
}

/// 结果从未到达的那次调用（一次取消）的占位。
pub fn no_tool_result() -> &'static str {
    "（流上没有结果）"
}

/// 失败、却没带自己的消息的那次调用的占位。
pub fn no_message() -> &'static str {
    "（没有消息）"
}

/// 前置钩子的叙述，挂载点用中文点名。
pub fn hook(point: &str, outcome: &str) -> String {
    format!("钩子 {}：{outcome}", hook_point(point))
}

/// 一个钩子挂载点的中文名。不认识的挂载点原样透传：outcome 里已经带着钩子自己那段无法
/// 改写的文本。
pub fn hook_point(point: &str) -> &str {
    match point {
        hook_format::POINT_PRE => "工具调用前",
        hook_format::POINT_POST => "工具调用后",
        other => other,
    }
}

/// 后置钩子的反馈，并进它所标注的那次调用。
pub fn hook_feedback(outcome: &str) -> String {
    format!("[钩子] {outcome}")
}

/// 一个讨论者派出执行者。
pub fn executor_spawned(executor_id: &str) -> String {
    format!("派出执行者 {executor_id}")
}

/// 一个执行者收尾，带上它的原因与小结。
pub fn executor_finished(executor_id: &str, reason: StopReason, summary: &str) -> String {
    format!(
        "执行者 {executor_id} 收尾：{} — {summary}",
        stop_reason(reason)
    )
}

/// 分歧块的小标题。
pub fn divergence(topic: &str) -> String {
    format!("分歧：{topic}")
}

/// 某个发言者自己的错误，作为叙述。消息本身由产生它的地方写成中文（`AgentError.message`
/// 模型可见，ADR 0005 起），这里只加中文的壳，正文原样透传。
pub fn agent_error(message: &str) -> String {
    format!("错误：{message}")
}

/// 两个人类画家都打的一行用量摘要。
pub fn usage_summary(usage: &Usage) -> String {
    format!(
        "用量 in={} out={} cached={} miss={}",
        usage.input_tokens, usage.output_tokens, usage.cached_tokens, usage.miss_tokens
    )
}

/// 流式推理轨迹前面的那条标记。
pub fn reasoning_marker() -> &'static str {
    "[思考] "
}

/// plain 转录的推理标签，接在发言归属前缀后面。
pub fn reasoning_label() -> &'static str {
    "（思考）"
}

/// 一条完成消息的边界叙述（正文已经流过去了）。
pub fn message_complete() -> &'static str {
    "消息完成"
}

/// 开启一段思考的那一行 —— 在正文到达之前：流还在跑，轨迹还没写完（票 03 §Answer）。
pub fn thinking_in_progress() -> &'static str {
    "… 正在思考"
}

/// 正文到达之后，一段思考定下来的那一行：轨迹冻结了，这一行变成进入它全文的入口
/// （票 03 §Answer）。
pub fn thinking_finished() -> &'static str {
    "✓ 思考完成"
}

/// 工具调用行打头的那个动词（票 02 §2）。参数跟在它后面，由
/// `transcript::summarize_args` 给出。
pub fn tool_call_label() -> &'static str {
    "调用"
}

/// 接在一次失败调用那一行的**末尾**，于是失败是一个后缀，而不是第二行（票 02 §2）。
pub fn tool_failed() -> &'static str {
    "失败"
}

/// 整条工具输出读不回来时详情视图的那句话 —— 指针指的文件没了，或者从来没写过
/// （票 02 §4）。
pub fn detail_output_unavailable() -> &'static str {
    "全文不可用"
}

/// 思考那一行没有完整轨迹可显时详情视图的那句话：合成器流式推理、却不把它记下来
/// （票 02 §1）。
pub fn detail_reasoning_unrecorded() -> &'static str {
    "本次未记录思考全文"
}

/// 正文长过读的人那个上限时详情视图的那句话。
pub fn detail_truncated() -> &'static str {
    "已截断"
}

/// 详情视图一节的小标题，画在一条横线里：`── 思考 ──`（票 03 §Answer）。
pub fn detail_section(name: &str) -> String {
    format!("── {name} ──")
}

/// 内建工具的名字（措辞层本地钉一份，与 `crate::tools` 里那份同名）。
///
/// 名字是**协议的一部分**（模型按它调用），而措辞层按名字挑动词 —— 与上面的
/// [`ASK_USER_QUESTION_TOOL`] 同一条规矩：这里不为此依赖工具层。
const READ_FILE_TOOL: &str = "read_file";
const WRITE_FILE_TOOL: &str = "write_file";
const EDIT_FILE_TOOL: &str = "edit_file";
const GREP_TOOL: &str = "grep";
const BASH_TOOL: &str = "bash";
const TODO_TOOL: &str = "todo";
const WEB_SEARCH_TOOL: &str = "web_search";
const WEB_FETCH_TOOL: &str = "web_fetch";
const TASK_TOOL: &str = "task";

/// 一次工具调用正在跑时，对话视图末尾那句话：按工具说它**在做什么**，而不是笼统的
/// 「正在思考」——写文件就说正在写哪一份，读文件就说正在看哪一份（2026-10-05 维护者的优化）。
pub fn working(tool: &str, args: &Value) -> String {
    let description = tool_description(tool, args);
    match working_verb(tool) {
        // shell 命令的描述**自带**动词（`运行 cargo test`），所以只加「正在」。
        None if !description.is_empty() => format!("正在{description}…"),
        None => format!("正在运行 {tool}…"),
        Some(verb) if !description.is_empty() => format!("正在{verb}{description}…"),
        Some(verb) => format!("正在{verb}{tool}…"),
    }
}

/// 一个工具在做的事该配哪个动词；`None` 表示它的描述自带动词。
fn working_verb(tool: &str) -> Option<&'static str> {
    match tool {
        READ_FILE_TOOL => Some("查看 "),
        WRITE_FILE_TOOL | EDIT_FILE_TOOL => Some("写 "),
        GREP_TOOL | WEB_SEARCH_TOOL => Some("搜索 "),
        BASH_TOOL => None,
        TODO_TOOL => Some("更新待办 "),
        WEB_FETCH_TOOL => Some("抓取 "),
        TASK_TOOL => Some("派活 "),
        _ => Some("调用 "),
    }
}

/// 模型还没吐出第一个字时，对话视图末尾那条会走的提示
/// （2026-10-05 维护者的优化）：点号每 8 帧挪一格 —— 帧是 60 ms，所以大约半秒一步。
pub fn waiting(frame: u64) -> String {
    let dots = 1 + (frame / 8) % 3;
    format!("正在思考{}", ".".repeat(dots as usize))
}

/// 详情视图思考那一节的小标题。
pub fn detail_thinking_section() -> &'static str {
    "思考"
}

/// 详情视图参数那一节的小标题。
pub fn detail_args_section() -> &'static str {
    "参数"
}

/// 详情视图输出那一节的小标题。
pub fn detail_output_section() -> &'static str {
    "输出"
}

/// 详情视图正文那一节的小标题：轨迹页里那条消息只画了首行，全文住在这里
/// （`.scratch/trace-tab/spec.md` §3）。
pub fn detail_message_section() -> &'static str {
    "正文"
}

/// 详情视图的页脚：读者读到全文的哪里，以及怎么离开。
pub fn detail_footer(position: usize, total: usize) -> String {
    format!("↕ {position}/{total} · esc 关闭")
}

/// 一次工具调用的**描述**：这次调用是干什么的，替代它那一行生参数。
///
/// 转录显示 `{label} 调用 {tool} {description}`；具体的参数与整条输出住在这次调用的
/// 详情视图里。一行生参数是调试器眼里的调用 —— 读的人想知道它*干了什么*（票 02 §2，
/// 2026-09-23 修正）。
///
/// 规则刻意少而机械，因为描述只从参数推出来 —— 流上没有任何东西说模型想干什么：
///
/// * 问卷工具用它问的那道题描述自己；
/// * 收路径的工具用那个路径描述自己；
/// * 一条 shell 命令用「它第一个认出来的命令所挣的动词」加上其中第一个像路径的词描述
///   自己 —— `查询 .scratch/tui-history-replay`；
/// * 其余的都落到这次调用的参数摘要上，于是一个参数未知的动态工具永远不会留空。
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

/// 工具自己的参数给它的描述：说这次调用是关于什么的那个字段，按这个仓库里各工具给它们
/// 命名的顺序找。
fn argument_description(tool: &str, args: &Value) -> Option<String> {
    // 问卷是关于那道题的，而它自己的 `header` 就是模型对它的一句概括 —— 那正是描述该有
    // 的东西。
    if tool == ASK_USER_QUESTION_TOOL {
        return first_question_field(args);
    }
    // 点名了一个路径的调用，就是关于那个路径的。
    for key in ["path", "file_path", "file", "target", "pattern", "query"] {
        if let Some(value) = args.get(key).and_then(Value::as_str) {
            let value = first_line(value);
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    // 派出去的任务，是关于那项任务的。
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

/// 问卷工具的名字。
///
/// 按名字匹配，因为描述是措辞，而措辞以这次调用*是*什么为键：工具表里的 `Tool` trait
/// 持有行为，不持有散文。
const ASK_USER_QUESTION_TOOL: &str = "ask_user_question";

/// 第一道题的 `header`，没有就用它的 `question`，排成一行。
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

/// 一条 shell 命令的描述：一个说它干什么的动词，然后是它拿什么去干。
///
/// 规则机械，因为描述只从参数推出来 —— 流上没有任何东西说模型想干什么：
///
/// 1. `cd somewhere` 被丢掉：导航永远不是一次调用*关于*的东西。
/// 2. 动词是这一层在余下部分里认出来的第一个词（[`command_verb`]），所以
///    `ls -a; find .scratch` 用它所干的事描述自己。
/// 3. 宾语是第一个看起来像路径的词 —— `查询 .scratch/tui-history-replay` —— 因为那才是
///    把两次 `查询` 分开的 token。没有的话就取第一个操作数：从 `rm -rf build` 得到
///    `修改 build`。
/// 4. 第二个词是子命令的程序两个词都留，因为单看子命令是含混的：`查看 git status`、
///    `运行 cargo test`。
/// 5. 里面一个词都认不出来的命令用它的第一个词描述自己：`运行 env`。
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

/// 第二个词是值得留下的子命令的那些程序：`git status` 是一次调用，光一个 `status` 是含混
/// 的。
const SUBCOMMAND_PROGRAMS: [&str; 6] = ["git", "cargo", "npm", "pnpm", "yarn", "go"];

/// 一条命令是关于的那个词：第一个像路径的操作数，没有就取第一个操作数。
///
/// 旗标（`-rf`）、重定向（`2>/dev/null`）与 `cd` 的目标都跳过 —— 它们都不是一次调用
/// 关于的东西，而在一次破坏性调用上点错名字，比什么都不点更糟。
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

/// 剥掉一行 shell 给它包上的标点之后的命令词。
fn clean_word(word: &str) -> &str {
    word.trim_start_matches('(')
        .trim_matches(|ch| ch == '"' || ch == '\'' || ch == '`' || ch == ';')
}

/// 整条折叠调用行的文本：`调用 工具 描述`。
///
/// **两个读者一个产出。** 转录的折叠行与就同一次调用发问的权限询问显示的是同一个字符串，
/// 于是你作答的那个问题与它所讲的那一行不会漂开（2026-09-23，用户要求：权限弹窗读起来该
/// 像调用行）。
pub fn tool_call_line(tool: &str, args: &Value) -> String {
    let description = tool_description(tool, args);
    if description.is_empty() {
        format!("{} {tool}", tool_call_label())
    } else {
        format!("{} {tool} {description}", tool_call_label())
    }
}

/// 一条命令开头的词挣来的动词：读的人会说这次调用在干什么。
///
/// 只有三个动词，因为读的人只需要三个：查点东西、看点东西，或者改点东西。这一层不认识的
/// 程序**不猜** —— 见 [`command_description`]。
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

/// `text` 的第一条非空行，去掉首尾空白。
fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// `text` 裁到 `max` 个字符，用省略号标出。
///
/// 刻意只留在这个层里：描述是措辞，而转录自己那个 `truncate` 是呈现助手，措辞层没有理由
/// 跟它共用。
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push_str(ELLIPSIS);
    out
}

/// 一次调用的描述在被裁掉之前可以有多长。
///
/// 描述的存在是为了让这一行一眼可读；长描述会与被它替掉的那些参数一样难读，而窗格反正也
/// 会折它。
const DESCRIPTION_MAX_CHARS: usize = 60;

/// 一次成功结束的工具调用。结果本身由调用方打印。
pub fn tool_completed() -> &'static str {
    "工具完成"
}

/// provider 自己那个终止标签，用中文点名。它只是诊断，但它仍然会经一条诊断行到达界面。
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

/// 一个以它 `as_str` 名字记下来的停止点的中文短语：`sessions stats` 的一行带的是名字、
/// 不是枚举。不认识的名字原样显示。
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

/// 一个以它 `as_str` 名字记下来的权限裁决的中文短语。
pub fn decision_name(name: &str) -> &str {
    match name {
        "allow" => decision(Decision::Allow),
        "ask" => decision(Decision::Ask),
        "deny" => decision(Decision::Deny),
        other => other,
    }
}

/// 转录上的一次权限询问。
///
/// 人需要的是那个工具与它将要做的具体调用 —— 命令、路径、正文。request 与 tool-call 的
/// id 是事件流的事；它们不显示。
pub fn permission_asked(tool_name: Option<&str>, args: &str) -> String {
    match (tool_name, args.is_empty()) {
        (Some(tool), true) => format!("权限询问：{tool}"),
        (Some(tool), false) => format!("权限询问：{tool}（{args}）"),
        (None, true) => "权限询问".to_owned(),
        (None, false) => format!("权限询问（{args}）"),
    }
}

/// 一个权限裁决，裁决来自哪里用中文点名。reason 是权限门自己那段持久文本，原样透传。
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

/// 一个权限裁决的中文短语。
pub fn decision(decision: Decision) -> &'static str {
    match decision {
        Decision::Allow => "允许",
        Decision::Ask => "询问",
        Decision::Deny => "拒绝",
    }
}

/// 一个裁决来自哪里的中文标签。这一整个模块的先例，如今是它的函数之一
/// （spec §Implementation Decisions）。
pub fn decision_source(source: DecisionSource) -> &'static str {
    match source {
        DecisionSource::User => "用户",
        DecisionSource::Hook => "钩子",
        DecisionSource::Policy => "策略",
    }
}

/// 一道题按钮行上的一个键：作答的那个键，以及那个答案是什么意思。
///
/// TUI 的覆盖层每个条目画一个 `[key] label`，plain 控制台把同一张表接进它那一条输入行，
/// 所以一道题不可能在一个前端长出一个另一个前端没提供的键（spec §9）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    /// 作答的那个键，原样显示。
    pub key: char,
    /// 那个键答的是哪个意思。
    pub label: &'static str,
}

/// 回答一次权限询问的那些键。
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

/// 回答过大粘贴那个问题的键。
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

/// 回答清空草稿那个问题的键。
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

/// 回答一次权限询问的那些键，配上每一个送出的答案。
pub static PERMISSION_CHOICE_ANSWERS: [(char, crate::permissions::Answer); 3] = [
    ('y', crate::permissions::Answer::Allow),
    ('a', crate::permissions::Answer::AlwaysAllow),
    ('n', crate::permissions::Answer::Deny),
];

/// 一行文本形式的按钮行：`[y] 允许 / [a] 总是允许 / [n] 拒绝`。
///
/// 这是 plain 控制台那条输入行用的东西。TUI 把同样的条目画成它自己的 span，并把键挑出来
/// （spec §9）。
pub fn choices_text(choices: &[Choice]) -> String {
    choices
        .iter()
        .map(|choice| format!("[{}] {}", choice.key, choice.label))
        .collect::<Vec<_>>()
        .join(" / ")
}

/// 权限覆盖层的标题。
///
/// 它不再点名工具：它下面那一行以 `调用 工具 …` 开头，那是与折叠转录行同一句话，于是名字
/// 只出现一次（2026-09-23，用户要求：弹窗在重复自己）。
pub fn permission_title() -> &'static str {
    "权限询问："
}

/// 权限覆盖层的**正文**行：这道题所讲的那个具体调用（`bash（command=rm -rf /）`）。
/// 工具名打头，于是这一行在标题下自己站得住。
pub fn permission_call(tool_name: &str, args: &str) -> String {
    if args.is_empty() {
        tool_name.to_owned()
    } else {
        format!("{tool_name}（{args}）")
    }
}

/// plain 控制台的权限输入行，它还给权限门的理由留了位置。
///
/// 一次**升级**询问在那之前多印几行（逐行同构）：先点名说话人（发起者不是主会话时）、
/// 再说清这是升级、理由与要放开的路径；命令行与按钮行仍然在最后
/// （`.scratch/workspace-mode/spec.md` §7）。
pub fn permission_prompt_with_context(
    tool_name: &str,
    args: &str,
    reason: &str,
    speaker: Option<&SpeakerId>,
    escalation: Option<&Escalation>,
) -> String {
    let mut prompt = String::new();
    if let Some(speaker) = speaker {
        prompt.push_str(&format!(
            "{}\n",
            permission_speaker_line(&permission_asked(Some(tool_name), args), speaker)
        ));
    }
    if let Some(escalation) = escalation {
        for line in permission_escalation_lines(escalation) {
            prompt.push_str(&format!("  {line}\n"));
        }
    }
    prompt.push_str(&format!(
        "{}？原因：{reason} {} ",
        permission_asked(Some(tool_name), args),
        choices_text(&PERMISSION_CHOICES)
    ));
    prompt
}

/// 一次权限询问的第一行，发起者不是主会话时把说话人括进那一行里
/// （`调用 bash（执行者 planner 发起）`）。
pub fn permission_speaker_line(call: &str, speaker: &SpeakerId) -> String {
    format!("{call}（{} 发起）", permission_speaker(speaker))
}

/// 一次询问的发起者，用中文点名。
pub fn permission_speaker(speaker: &SpeakerId) -> String {
    match speaker {
        SpeakerId::Debater(id) => format!("讨论者 {id}"),
        SpeakerId::Executor(id) => format!("执行者 {id}"),
        SpeakerId::User => "用户".to_owned(),
        SpeakerId::System => "系统".to_owned(),
    }
}

/// 一次升级询问多出来的那几行：这是升级、理由、要放开的路径。
///
/// 顺序照 `.scratch/workspace-mode/spec.md` §7；命令行由调用它的那一处保持最后一行 ——
/// 它是授权时唯一必须看得见确切命令的地方。
pub fn permission_escalation_lines(escalation: &Escalation) -> Vec<String> {
    let paths: Vec<String> = escalation
        .writable_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    vec![
        "被沙箱拒绝，申请写工作区之外".to_owned(),
        format!("理由：{}", escalation.justification),
        format!("要放开的路径：{}", paths.join("、")),
    ]
}

/// 过大粘贴那个问题的**标题**行。
pub fn paste_title() -> &'static str {
    "粘贴确认"
}

/// 过大粘贴那个问题的**正文**行。
pub fn paste_body(chars: usize) -> String {
    format!("粘贴 {chars} 字符")
}

/// 清空草稿那个问题的**标题**行。
pub fn clear_draft_title() -> &'static str {
    "清空输入"
}

/// 清空草稿那个问题的**正文**行。
pub fn clear_draft_body() -> &'static str {
    "草稿有多行，Esc 会把它们全部丢掉"
}

/// 问卷页脚里往回一题的路。
pub fn questionnaire_previous() -> &'static str {
    "← 上一题"
}

/// 问卷页脚里往前一题的路。
pub fn questionnaire_next() -> &'static str {
    "下一题 →"
}

/// 问卷页脚的提交按钮，只在每一题都有着落之后才画。
pub fn questionnaire_submit() -> &'static str {
    "提交"
}

/// 给问卷翻页的页脚：`2 / 3`。
pub fn questionnaire_progress(index: usize, total: usize) -> String {
    format!("{} / {}", index + 1, total)
}

/// 问卷页脚里进度段之后那句「这一题交回去的是什么」。
///
/// 往前走会给没作答的题记上跳过，所以翻回来时得看得见那一笔，不必靠回忆
/// （`.scratch/questionnaire-keys/spec.md` §11）。
pub fn questionnaire_skipped() -> &'static str {
    "已跳过"
}

/// 问卷页脚里那段键位提示，按**留给它的列数**降级
/// （`.scratch/questionnaire-keys/spec.md` §6、§11）。
///
/// 三档递减，按「先丢教学性的」排：Emacs 别名 → `j`/`k` 那一句 → 只留出口。`submits` 说的是
/// **回车这一下实际会做什么** —— 把当前题记成跳过之后每题都有着落就是「提交」，否则「下一
/// 题」；提示段跟着它变，因为回车是这一轮的主角。空串表示这一段连一句提示都放不下，页脚于是
/// 只剩进度与按钮。举手回执**不在**这条阶梯里 —— 它不参与降级，页脚一放得下就先画它（见
/// [`questionnaire_decline_raised`] 与 [`questionnaire_exit_raised`]）。
pub fn questionnaire_hint(room: usize, submits: bool) -> &'static str {
    if room >= QUESTIONNAIRE_HINT_WIDE {
        if submits {
            "j/k 移动 · 空格选中 · 回车 提交 · esc 退出询问 · ctrl-n/ctrl-p 同 j/k"
        } else {
            "j/k 移动 · 空格选中 · 回车 下一题 · esc 退出询问 · ctrl-n/ctrl-p 同 j/k"
        }
    } else if room >= QUESTIONNAIRE_HINT_MEDIUM {
        if submits {
            "j/k 移动 · 空格选中 · 回车 提交 · esc 退出询问"
        } else {
            "j/k 移动 · 空格选中 · 回车 下一题 · esc 退出询问"
        }
    } else if room >= QUESTIONNAIRE_HINT_NARROW {
        "esc 退出询问"
    } else {
        ""
    }
}

/// 问卷页脚里举手等第二下时的回执：**退出这次询问**那一把（`Esc`）。
///
/// 它**替换**页脚最后那一段键位提示，不是追加 —— 与退出手势替换提示行出口段同构
/// （`.scratch/exit-gesture/spec.md` §2）。它不参与宽度阶梯。
pub fn questionnaire_decline_raised() -> &'static str {
    "再按一次 esc 退出询问"
}

/// 问卷页脚里举手等第二下时的回执：**退出这次运行**那一把（`Ctrl-C`）。
///
/// 问卷立着时提示行整行被页脚替换，所以那句话只有落在这里才看得见
/// （`.scratch/questionnaire-keys/spec.md` §6）。
pub fn questionnaire_exit_raised() -> &'static str {
    "已取消 · 再按一次 ctrl-c 退出"
}

/// 键位提示那三档的边界，量的是**留给它的列数**（不是终端宽度）：三档文案里最宽的那个变体
/// 各自的实测宽度，否则会出现「判成宽档、句子却放不下」而被裁掉一截。
///
/// 2026-10-05 §11 把回车那一句加进来之后按新文案重算（`回车 下一题` 比 `回车 提交` 宽两列，
/// 阈值取两者之大）。
const QUESTIONNAIRE_HINT_WIDE: usize = 71;
const QUESTIONNAIRE_HINT_MEDIUM: usize = 48;
const QUESTIONNAIRE_HINT_NARROW: usize = 12;

/// 多选题在它的文本旁边带的那条标记，好让用户知道可以选中多于一个选项。
pub fn questionnaire_multi_marker() -> &'static str {
    "（可多选）"
}

/// 没有选项的题上，打的答案落在那一行的标签。
pub fn questionnaire_answer_label() -> &'static str {
    "回答："
}

/// 给了选项的题上，自定义文本落在那一行的标签。
pub fn questionnaire_custom_label() -> &'static str {
    "自定义："
}

/// plain 控制台给有选项的题打的那个提示。
///
/// 逐行前端没有可见的模式，所以提示得说清怎么选、怎么跳过：编号是选，别的东西是自定义
/// 文本，空行是跳过。多选题要说下面还跟着一行，因为那段补充是把 `selected` 与 `custom`
/// 一起答出来的唯一办法（spec §7）。
pub fn questionnaire_plain_options_prompt(multi_select: bool) -> &'static str {
    if multi_select {
        "输入编号（逗号分隔）选择，或输入文本；下一行补充；回车跳过 > "
    } else {
        "输入编号选择，或直接输入文本；回车跳过 > "
    }
}

/// plain 控制台给多选题的第二行：与所选选项搭配的那段可选补充（spec §7）。
pub fn questionnaire_plain_supplement_prompt() -> &'static str {
    "补充文本（可留空）> "
}

/// plain 控制台给没有选项的题打的那个提示。
pub fn questionnaire_plain_answer_prompt() -> &'static str {
    "输入回答；回车跳过 > "
}

/// 模型的题里的一个选项，两个人类前端都这么显示它：
/// `{number}. {label}{badge} — {description}`（spec §7）。
///
/// 这是选项行的**唯一**生成器，plain 的打印器与 TUI 的问卷画家都调它，所以「一个选项读
/// 起来是什么样」的改动不可能只落在其中一个上。调用方在这一行旁边加上它自己前端显示的那点
/// 状态 —— TUI 那个选中/高亮的标记 —— 因为那是两者唯一不共享的东西。
///
/// `(Recommended)` 后缀是一条显示约定：它在这里被 [`recommended_badge`] 换掉，而答案
/// 携带的值保留整条 label（[`recommended_label`]）。那个编号是阅读序号、不是键：定下来的
/// 键盘上没有它。
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

/// 模型推荐一个选项时接的后缀（spec §7）。
pub const RECOMMENDED_SUFFIX: &str = "(Recommended)";

/// label 以 [`RECOMMENDED_SUFFIX`] 结尾的选项上显示的那枚徽标。
pub fn recommended_badge() -> &'static str {
    "（推荐）"
}

/// 把模型给的选项 label 拆成「显示什么」与「是否被推荐」。
///
/// 这个后缀是**显示**约定：它被剥掉，好让选项读起来是个可选项、而不是一句话，而答案携带的
/// 值仍是原来那条 label、连标记一起（spec §7）。匹配区分大小写、而且只在结尾，所以一条
/// 只是提到这个词的 label 不会被动。
pub fn recommended_label(label: &str) -> (&str, bool) {
    match label.trim_end().strip_suffix(RECOMMENDED_SUFFIX) {
        Some(rest) => (rest.trim_end(), true),
        None => (label, false),
    }
}

/// 人的 `[speaker]` 前缀：一个生成器，每个面向人的渲染器都用它，而且刻意不是模型侧的
/// 投影前缀（spec §5）。
///
/// 讨论者保留自己的名字；没有归属的那些发言者拿一个中文标签。
pub fn speaker_label(speaker: &SpeakerId) -> String {
    match speaker {
        SpeakerId::Debater(id) => format!("[{id}]"),
        SpeakerId::Executor(id) => format!("[执行者 {id}]"),
        SpeakerId::User => "[用户]".to_owned(),
        SpeakerId::System => "[系统]".to_owned(),
    }
}

/// 一条钉住的上下文注入，来源被点名、而不是 debug 打印出来。
pub fn context_injected(source: ContextSource) -> String {
    format!("[上下文注入：{}]", context_source(&source))
}

/// 这一刻的沙箱状态，给 `sessions show` 的复盘用（沙箱 spec §8）。
///
/// 模式是协议标记、原样；不可用时把原因一起摊开 —— 审计者要判断的正是「当时是不是被关着、
/// 为什么」。
pub fn sandbox(mode: &str, unavailable_reason: Option<&str>) -> String {
    match unavailable_reason {
        Some(reason) => format!("[沙箱：{mode} · 不可用：{reason}]"),
        None => format!("[沙箱：{mode}]"),
    }
}

/// 一个上下文来源的中文名。
pub fn context_source(source: &ContextSource) -> String {
    match source {
        ContextSource::AgentsMd => "AGENTS.md".to_owned(),
        ContextSource::SkillsCatalog => "技能清单".to_owned(),
        ContextSource::McpCatalog => "MCP 加载".to_owned(),
        ContextSource::Skill => "技能".to_owned(),
        // 留着读**老**流：它点名的那一档模式已经没了，但在
        // `.scratch/todo-and-modes` 之前写下的会话仍然带着这条注入，复盘它时应该说出它
        // 当时是什么（ADR 0003）。
        ContextSource::PlanMode => "计划模式".to_owned(),
        // 唯一一条只属于**一个**参与者的注入，所以它说出是哪个：读转录的人应该看到谁被
        // 给了人物设定。
        ContextSource::Persona(name) => format!("人物：{name}"),
        ContextSource::Goal => "目标清单".to_owned(),
        ContextSource::Compaction => "压缩摘要".to_owned(),
        ContextSource::Reminder => "过半提醒".to_owned(),
    }
}

/// 一段不再权威的历史。summary 是事件流自己的文本，原样透传；没有它，这一行也仍然说得
/// 出发生了什么。
pub fn history(reason: HistoryReason, summary: Option<&str>) -> String {
    let text = summary.unwrap_or("历史已被取代");
    format!("[历史：{}] {text}", history_reason(reason))
}

/// 一个历史原因的中文名。
pub fn history_reason(reason: HistoryReason) -> &'static str {
    match reason {
        HistoryReason::Regenerate => "重新生成",
        HistoryReason::Undo => "撤销",
        HistoryReason::Compaction => "压缩",
        // 同样是只为老流留的：模式变更时一条钉住指令的退场。已经没有地方产出它了
        // （ADR 0003）。
        HistoryReason::ModeChange => "模式变更",
    }
}

/// 渲染器自己的诊断行。
pub fn diagnostic(message: &str) -> String {
    format!("[诊断] {message}")
}

/// 渲染器跟不上通道、丢了事件。
pub fn renderer_dropped(dropped: u64) -> String {
    format!("渲染器丢弃了 {dropped} 个事件")
}

/// 状态词：有没有一个回合进行中。
pub fn status_word(busy: bool) -> &'static str {
    if busy {
        "工作中"
    } else {
        "就绪"
    }
}

/// 「在跑」的字形循环：**一轮月相**，一格一个相位
/// （`.scratch/tui-visual-language/spec.md` §30–§31）。
///
/// **不靠界面色板、不靠变暗** —— 相位由字形自己说出来。但要注意它们与框架线**不是**同一档
/// 宽度：月相是 Emoji（East Asian Width = Wide，**占 2 列**，而且由终端的 emoji 字体上色），
/// 所以状态行那一段比原来四个 1 列的 `◐` 系码位宽一格。真终端里要确认它在所用字体里是否
/// 等宽不跳（手工清单 ⑮）；退路是把这一集换回四格的 ASCII `| / - \`，`scripts/tui-startup-check.py`
/// 的两个锚点与它无关。
pub const PULSE_GLYPHS: [&str; 8] = ["🌑", "🌒", "🌓", "🌔", "🌕", "🌖", "🌗", "🌘"];

/// 空闲时每换一格要几帧。运行中是它的四分之一 —— 同一个时钟，两种速度（§31）。
///
/// 一轮是八格：运行中 8 × 8 帧 ≈ 3.8 秒，空闲 8 × 32 帧 ≈ 15.4 秒。
const IDLE_FRAMES_PER_GLYPH: u64 = 32;

/// 状态词前那个字形，取第 `frame` 帧。
///
/// 空闲每 32 帧（≈1.9 秒）换一格，运行中每 8 帧（≈0.5 秒）—— 于是「在跑」有自己的信息
/// 通道，而不是又一种颜色（§30）。
pub fn status_spinner(frame: u64, busy: bool) -> &'static str {
    let step = if busy {
        IDLE_FRAMES_PER_GLYPH / 4
    } else {
        IDLE_FRAMES_PER_GLYPH
    };
    PULSE_GLYPHS[(frame / step) as usize % PULSE_GLYPHS.len()]
}

/// 活着的键位提示，按它们显示的先后：最常用的在前。出口是 [`EXIT_HINT_IDLE`] /
/// [`EXIT_HINT_BUSY`]（由 [`exit_hint`] 挑），它是**预留**的、不是追加的，所以任何宽度
/// 下它都活下来。
///
/// **位置就是优先级**：[`hint_line`] 从前往后填、超宽就停，所以排在最末的那条是窄档最先
/// 丢掉的一条。左栏开关因此挂在最后 —— 滚动比它常用得多
/// （`.scratch/sidebar-toggle/spec.md` §4）。
const KEY_HINTS: [&str; 6] = [
    "enter 发送",
    "ctrl-j 换行",
    "esc 取消",
    "shift+tab 模式",
    "PgUp/PgDn 滚动",
    "ctrl-o 左栏",
];

/// 键盘空闲时的出口：两个手势都退出，这一行也这么说。它是一个条目而不是两个，因为这两个
/// 键在这里意思相同，而窄终端就只有那么多列（票 06 §4）。
pub const EXIT_HINT_IDLE: &str = "ctrl-c/ctrl-d 退出";

/// 一次运行进行中时的出口：`Ctrl-C` 取消，而 `Ctrl-D` 刻意被忽略 —— 提示一个什么都不做
/// 的键，是提示行绝不能做的那件事（票 06 §4）。
pub const EXIT_HINT_BUSY: &str = "ctrl-c 退出";

/// 空闲、**已经举手**时的出口：再按一下就走，两键都算
/// （`.scratch/exit-gesture/spec.md` §2）。
pub const EXIT_HINT_IDLE_RAISED: &str = "再按一次 ctrl-c/ctrl-d 退出";

/// 忙碌、已经举手时的出口。它把两件事都说出来：回合确实停了、再按会退出 —— 这正是它比只
/// 说「退出」值钱的地方（spec §2）。
pub const EXIT_HINT_BUSY_RAISED: &str = "已取消 · 再按一次 ctrl-c 退出";

/// 重放、已经举手时的出口：重放里只有 `Ctrl-C` 管用，而进度行临时让位给这一句
/// （spec §2）。
pub const EXIT_HINT_REPLAY_RAISED: &str = "再按一次 ctrl-c 退出";

/// 前端**没有**在读行时显示的提示：一次性 `discuss`，或者交互式会话里一个回合进行中的那
/// 一段。
///
/// 只有那时键盘真会做的事 —— 停下这次运行，以及读回它产出的东西。没有 `enter 发送`
/// （没有东西会被发出去），也没有 `shift+tab 模式`（那个手势是交互式循环的，而讨论没有
/// 可回去的提示行）。
const VIEWER_HINTS: [&str; 2] = ["esc 取消", "PgUp/PgDn 滚动"];

/// 一个 `width` **列**宽终端的提示行。
///
/// **状态词不在这里**：它搬到了状态行的最后一段，因为那里永远在场，而提示行的阶梯会把它丢掉
/// （`.scratch/tui-visual-language/spec.md` §18）。这一行只剩键位提示与出口。
///
/// 提示从左边填，出口（`exit`）预留在它们末尾 —— 于是窄终端保住它的出口*以及*排在前面的那些
/// 提示，让掉的是排在最末的 `ctrl-o 左栏`。位置就是优先级：`ctrl-o 左栏` 挂最后，因为滚动
/// 比它常用得多（`.scratch/sidebar-toggle/spec.md` §4）。
pub fn status_line(busy: bool, width: u16, raised: bool) -> String {
    hint_line(&KEY_HINTS, exit_hint(busy, raised), width)
}

/// 前端没有在读行时的提示行：同样的阶梯，铺在 [`VIEWER_HINTS`] 上。
///
/// 这条区分不是装饰。提示描述的是键盘会做什么，而一个回合跑到一半的会话 —— 或一次
/// `discuss` 运行，它压根不会要一行输入 —— 否则就会为一个什么都不发的键承诺
/// `enter 发送`（spec §6）。
pub fn viewer_status_line(busy: bool, width: u16, raised: bool) -> String {
    hint_line(&VIEWER_HINTS, exit_hint(busy, raised), width)
}

/// 状态行里那个出口条目：没举手时按忙闲挑一句，举手之后换成那一档的催促。
///
/// 举手换掉的是**出口那一段**，不是往后追加 —— 屏幕上不许出现「旧出口文案 + 举手文案」
/// 那种形态（`.scratch/exit-gesture/spec.md` §2）。
pub fn exit_hint(busy: bool, raised: bool) -> &'static str {
    match (busy, raised) {
        (true, true) => EXIT_HINT_BUSY_RAISED,
        (true, false) => EXIT_HINT_BUSY,
        (false, true) => EXIT_HINT_IDLE_RAISED,
        (false, false) => EXIT_HINT_IDLE,
    }
}

/// 两条提示行共用的阶梯：提示从左来，出口预留在末尾。提示从前往后填，装不下下一条就停 ——
/// 位置就是优先级。
fn hint_line(hints: &[&str], exit: &str, width: u16) -> String {
    let exit_columns = exit.cell_width();
    let mut chosen = String::new();
    for hint in hints {
        let candidate = if chosen.is_empty() {
            (*hint).to_owned()
        } else {
            format!("{chosen}{SEP}{hint}")
        };
        if candidate.cell_width() + SEP.cell_width() + exit_columns > width {
            break;
        }
        chosen = candidate;
    }
    if chosen.is_empty() {
        exit.to_owned()
    } else {
        format!("{chosen}{SEP}{exit}")
    }
}

// ---------------------------------------------------------------------------
// 历史重放（`.scratch/tui-history-replay/spec.md` §4、§6）
// ---------------------------------------------------------------------------

/// 重放所用的名字与它数到的进度：历史的事件里有多少已经铺进转录。
///
/// 刻意叫 `history_*`、不叫 `replay_*`：这个模块已经有一整套给 `sessions replay` 用的
/// `replay_*` 家族，那个**重算一次投影**，与把历史铺给读的人看是两回事（spec §10）。
pub fn history_progress(n: usize, m: usize) -> String {
    format!("恢复历史 {n}/{m}")
}

/// 提示行窄到放不下整句之后的那个计数：最小合法帧（`40×10`）留下 38 列提示（票 06 §4）。
pub fn history_progress_narrow(n: usize, m: usize) -> String {
    format!("恢复中 {n}/{m}")
}

/// 一个数字都没有的计数，给比最小帧能画出的还窄的提示行。
pub fn history_progress_minimal() -> &'static str {
    "恢复中"
}

/// 一条 `width` 列宽的提示行的重放进度行。
///
/// 这个宽度是**提示行的**、不是终端的，和 [`status_line`] 完全一样：最小帧（40 列）留下
/// 38 列提示，而中间那一档就是为这个宽度存在的。阶梯放在这里而不是渲染器里，理由与提示
/// 阶梯相同：它是措辞，而且不用终端就量得出来。
pub fn history_progress_line(n: usize, m: usize, width: u16) -> String {
    if width < HISTORY_NARROW_MIN {
        history_progress_minimal().to_owned()
    } else if width < HISTORY_FULL_MIN {
        history_progress_narrow(n, m)
    } else {
        history_progress(n, m)
    }
}

/// 仍然带着计数的、最窄的提示行：最小合法帧减去两条边框列。
const HISTORY_NARROW_MIN: u16 = super::layout::MIN_WIDTH - 2;

/// 提示阶梯在最小帧之后下一档实测出来的位置：60 列终端买到第二条提示，而整句历史短语正是
/// 在那里挣到它的列数。
const HISTORY_FULL_TERMINAL: u16 = 60;

/// 从这一档提示行起，整句短语值得占那些列。
const HISTORY_FULL_MIN: u16 = HISTORY_FULL_TERMINAL - 2;

/// 画在重放的历史与这场会话新增的内容之间的那条线。
///
/// 它是**渲染层的一条线，不是一条事件**：它从不进日志，所以下一次 `--continue` 会在新的
/// 接缝上插一条新的，而不是把旧的也重放出来（spec §6）。
pub fn history_divider() -> &'static str {
    "── 以上为历史 ──"
}

/// 模型的标签：状态行的第一段（spec §5）。
pub const PANEL_MODEL: &str = "模型";
/// 左栏调用量页上的会话读数（spec §3）。
pub const PANEL_CONTEXT: &str = "上下文";
/// 按界面其他地方的拼法拼；`CONTEXT.md` 里它没有中文词，而统计行已经在写 `token`。
pub const PANEL_TOKENS: &str = "token";
/// **回合**，不是轮次：这一页数的是 `TurnEnded`（`CONTEXT.md` 把轮次与回合分开）。
pub const PANEL_TURNS: &str = "回合";
pub const PANEL_INPUT: &str = "输入";
pub const PANEL_OUTPUT: &str = "输出";
pub const PANEL_CACHE: &str = "缓存";

/// 一个字段还没有数字可显时显示的东西。
pub const PANEL_UNKNOWN: &str = "—";

/// 一个计数用哪套书写制式写给人看（`.scratch/usage-stats-format/spec.md` §1）。
///
/// 它住在这里、而不是 `config`：它管的是「给人看的文本怎么写」，`[ui] number_style` 只是
/// 持有这个选择（与 `Mode` 那类配置枚举的方向相反 —— 制式是措辞，不是行为）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NumberStyle {
    /// 万 / 亿：中文界面里更大的数字这么读更快。这是缺省，也是唯一写缺省的地方。
    #[default]
    Cn,
    /// k / M / G：那些按 SI 前缀读数字的人习惯的那套。
    Si,
}

/// 按 `style` 那套制式写一个计数（spec §1）。
///
/// **小于 `10_000` 一律退回 [`thousands`]**，两套制式都一样：换算只在真省列的时候才划算，
/// `1.2万` 未必比 `12,345` 好读，而 `9,999` 换成 `10.0k` 连精度都赔进去了。
///
/// 档位由**原值**决定，不由格式化后的结果决定：`99_999_999` 写成 `10000万`，而不是
/// `1亿`。舍入之后再检查一次区间，会让每个边界多一轮判断，而那里的读法并没有更好。
///
/// 一位小数，恰好是整数时去掉 `.0`：`10_000` → `1万`、`12_345` → `1.2万`。没有「万亿 /
/// 兆」那一档 —— token 计数到不了，规则越少越好。
pub fn compact(value: u64, style: NumberStyle) -> String {
    if value < 10_000 {
        return thousands(value);
    }
    let (unit, suffix) = match style {
        NumberStyle::Cn if value < 100_000_000 => (10_000, "万"),
        NumberStyle::Cn => (100_000_000, "亿"),
        NumberStyle::Si if value < 1_000_000 => (1_000, "k"),
        NumberStyle::Si if value < 1_000_000_000 => (1_000_000, "M"),
        NumberStyle::Si => (1_000_000_000, "G"),
    };
    let scaled = format!("{:.1}", value as f64 / unit as f64);
    let scaled = scaled.strip_suffix(".0").unwrap_or(&scaled);
    format!("{scaled}{suffix}")
}

/// 带千位分隔符的计数：`12,345`。
///
/// 这个签名不能动：诊断通道（[`usage_summary`] 那一族与 `sessions stats`）也读它。显示层
/// 要制式就调 [`compact`]，它在上面分派。
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

/// 一个字段里的两个计数：`1.2万 / 10万`（制式见 [`compact`]）。
fn pair(left: u64, right: u64, style: NumberStyle) -> String {
    format!("{} / {}", compact(left, style), compact(right, style))
}

/// 这场会话花了多少，对上它的额度 —— 当它有额度时。
///
/// 没有上限的会话只显示花掉的那部分：`1.2万 / —` 会被读成一个缺失的上限，而不是一个从来
/// 没设过的上限（spec §8）。
pub fn token_pair(used: u64, limit: Option<u64>, style: NumberStyle) -> String {
    match limit {
        Some(limit) => pair(used, limit, style),
        None => compact(used, style),
    }
}

/// 模型的窗口有多满：`12,345 / 200,000（6%）`，或者在还没有一次调用报出输入 token 之前
/// 是 [`PANEL_UNKNOWN`]。
///
/// `with_share` 是这一页在说值那一列还放得下百分比。它是一个参数而不是第二个函数，因为那
/// 一对与它的占比是同一个字段：`12,345 / 200,000（6%）` 才是它说的话，而丢掉尾巴就是它
/// 退化的方式。
pub fn context_pair(
    used: Option<u64>,
    usable: u64,
    with_share: bool,
    style: NumberStyle,
) -> String {
    let Some(used) = used else {
        return PANEL_UNKNOWN.to_owned();
    };
    let pair = pair(used, usable, style);
    if with_share {
        format!("{}（{}%）", pair, used.saturating_mul(100) / usable.max(1))
    } else {
        pair
    }
}

/// 一次调用输入的缓存拆分：多少由前缀缓存供给、多少不是。
pub fn cache_pair(cached: u64, miss: u64, style: NumberStyle) -> String {
    pair(cached, miss, style)
}

/// 那条指示器：视口滚走期间到了多少，以及这个块就是回去的路（spec §4）。
pub fn new_content(rows: usize) -> String {
    format!("↓ {rows} 行新内容 · 点此到底")
}

/// 什么都没有到达时的同一条指示器：它只是回去的路。
pub fn back_to_bottom() -> &'static str {
    "点此到底"
}

/// 比最小尺寸还小的终端上显示的一切，好让原因是一句话、而不是一块空屏（spec §2）。
pub fn too_small(width: u16, height: u16) -> String {
    format!("终端太小：至少 {width}×{height}")
}

/// 程序名与它构建自的版本：左栏那段文字身份 —— 窄到画不下标记的终端显示的就是它
/// （spec §3）。
pub fn identity() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

/// 终端标题整条有多宽（显示列），超过就从右往左丢
/// （`.scratch/terminal-title/spec.md` §3）。
///
/// 写死、不做配置：标题没有宽度反馈，任何「看情况缩」的规则都没法测。
pub const TITLE_COLUMNS: usize = 40;

/// 标题里状态那一段取哪个词（spec §2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleState {
    /// 空闲：**没有**状态段。
    Idle,
    /// 回合在跑。
    Running,
    /// 有东西立着等人回答：审批、问卷、目标停确认。
    Waiting,
    /// 正在重放历史。
    Replaying,
}

/// 状态段那一个词；空闲没有词 —— 省略整段，而不是写一个空串（spec §2）。
pub fn title_word(state: TitleState) -> Option<&'static str> {
    match state {
        TitleState::Idle => None,
        TitleState::Running => Some("运行中"),
        TitleState::Waiting => Some("等你"),
        TitleState::Replaying => Some("重放中"),
    }
}

/// 把渲染器那三个布尔收成标题里的一个状态，优先级只住在这里一处（spec §2）。
///
/// 重放压过等你、等你压过运行中：对人来说「这个终端在等你」比「它在跑」更值得先看见，而
/// 重放是一种连输入都不接的时刻。`muted`（禁言）不参与 —— 它是运行中的一种。
pub fn title_state(replaying: bool, pending: bool, busy: bool) -> TitleState {
    if replaying {
        TitleState::Replaying
    } else if pending {
        TitleState::Waiting
    } else if busy {
        TitleState::Running
    } else {
        TitleState::Idle
    }
}

/// 标题里的路径段（spec §1）。
///
/// 两条分支，按 `$HOME` 划：落在 `$HOME` 之下就写 `~` 加相对路径（`~/code/fs-agent`，
/// 认人的家目录比认父目录基名有用）；其余写「父目录基名 / 当前基名」（`fortystory/fs-agent`）。
/// 前缀按**路径分量**比（[`Path::strip_prefix`]），不做字符串前缀 —— `~/code2` 不是
/// `~/code` 的子路径。
///
/// `home` 是参数、不在这里读 `$HOME`：这一层是纯函数，测试要能钉死。
pub fn title_path(cwd: &Path, home: Option<&Path>) -> String {
    if cwd == Path::new("/") {
        return "/".to_owned();
    }
    if let Some(home) = home {
        if cwd == home {
            return "~".to_owned();
        }
        if let Ok(rest) = cwd.strip_prefix(home) {
            return format!("~/{}", rest.to_string_lossy().replace('\\', "/"));
        }
    }
    let base = title_base(cwd);
    match cwd.parent().and_then(Path::file_name) {
        Some(parent) => format!("{}/{}", parent.to_string_lossy(), base),
        None => base,
    }
}

/// 只剩基名的路径段：40 列封顶的第三步退到这里（spec §3）。
///
/// `$HOME` 仍然缩成 `~`（它本身就是那个基名），`/` 仍然是 `/`。
pub fn title_path_base(cwd: &Path, home: Option<&Path>) -> String {
    if cwd == Path::new("/") {
        return "/".to_owned();
    }
    if home.is_some_and(|home| cwd == home) {
        return "~".to_owned();
    }
    title_base(cwd)
}

/// 路径最后一段的文字；拿不到时退回整条路径的文字（那只可能是根）。
fn title_base(cwd: &Path) -> String {
    cwd.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| cwd.to_string_lossy().into_owned())
}

/// 一整条终端标题：`<路径> · <状态> · <目标名>`，40 列封顶（spec §1、§3）。
///
/// 超了**从右往左丢**，一次一段，顺序固定：先目标名、再状态词、再退成只剩基名的路径、
/// 最后硬截断。规则是固定的而不是「看情况缩」的，因为标题没有宽度反馈 —— 一条会变的
/// 规则没法测，也没法在三种终端上复现同一件事。
pub fn terminal_title(
    cwd: &Path,
    home: Option<&Path>,
    state: TitleState,
    goal: Option<&str>,
) -> String {
    let path = title_path(cwd, home);
    let full = join_title(&path, title_word(state), goal);
    if text_columns(&full) <= TITLE_COLUMNS {
        return full;
    }
    let without_goal = join_title(&path, title_word(state), None);
    if text_columns(&without_goal) <= TITLE_COLUMNS {
        return without_goal;
    }
    let path_only = join_title(&path, None, None);
    if text_columns(&path_only) <= TITLE_COLUMNS {
        return path_only;
    }
    let base_only = join_title(&title_path_base(cwd, home), None, None);
    if text_columns(&base_only) <= TITLE_COLUMNS {
        return base_only;
    }
    truncate_columns(&base_only, TITLE_COLUMNS)
}

/// 把有名有姓的那几段用 ` · ` 接起来；缺席的段不留下多余的分隔符。
fn join_title(path: &str, word: Option<&str>, goal: Option<&str>) -> String {
    let mut parts = Vec::with_capacity(3);
    parts.push(path.to_owned());
    if let Some(word) = word {
        parts.push(word.to_owned());
    }
    if let Some(goal) = goal {
        parts.push(goal.to_owned());
    }
    parts.join(SEP)
}

/// 宽档左栏带的那个标记，五行块状明暗。
///
/// 这些字符全是文本；让它们读起来像字母的那道颜色渐变是画家的事
/// （[`crate::render::tui`]），与这个模块里其他每一条短语完全一样。窄到放不下整个标记的
/// 左栏压根不会要这几行 —— [`crate::render::layout`] 事先就定了，所以这里不用想裁剪的
/// 事。
///
/// 标记拼出 `fs-agent` —— 前面是分叉合成的 `fs`（「两叉一茎」，见 `CONTEXT.md`），后面
/// 跟程序名 —— 像素网格是维护者挑的那一个。中间那条短横是它八个字形单元里的第三个。
pub fn logo_lines() -> [&'static str; 5] {
    [
        "▄▀▀█ ▄▀▀█      ▄▀▀▄ ▄▀▀▀ ▄▀▀█ █  █ ▀█▀",
        "▓▄▄  ▓         ▓▄▄▓ ▓ ▀▓ ▓▄▄  ▓▄ ▓  ▓ ",
        "▒     ▀▀▄ ▀▀▀▀ ▒  ▒ ▒  ▒ ▒    ▒ ▀▒  ▒ ",
        "░    ░  ░      ░  ░ ░  ░ ░  ▄ ░  ░  ░ ",
        "▀    ▀▀▀       ▀  ▀  ▀▀▀  ▀▀▀ ▀  ▀  ▀ ",
    ]
}

// ---------------------------------------------------------------------------
// 外壳：左栏的页签、状态行与回合条
// （`.scratch/tui-sidebar/spec.md` §3、§5、§6）
// ---------------------------------------------------------------------------

/// 左栏的页签标签，按它们画出来的先后（spec §3）。
///
/// `todo` 排第二，而且是唯一一个不是常在那儿的标签：会话有了列表之后它才出现
/// （`.scratch/todo-and-modes/spec.md` §4）。四个标签加它们的间隔，在窄档仍然放得下。
pub const TAB_USAGE: &str = "调用量";
pub const TAB_TODO: &str = "todo";
pub const TAB_TRACE: &str = "轨迹";
pub const TAB_FILES: &str = "文件";

// ---------------------------------------------------------------------------
// 符号表与字符级间距
// （`.scratch/tui-visual-language/spec.md` §12–§15）
//
// 一个语义一个具名常量。**字形一个都不改**：表里每一个都早已在仓库里上过屏。
// ---------------------------------------------------------------------------

/// **有折起来的内容**：这一行点得开。思考行、工具行、上下文注入行、轨迹视图的消息行都有；
/// **对话视图的消息行不给** —— 它已经把全文显出来了。
///
/// **双义**：同一个字形在 todo 页里是「进行中」（[`TODO_IN_PROGRESS`]）。两处靠**区域**区分
/// —— todo 页整页都是 todo 行，不会与转录混。这条写在表里，别让它当暗知识。
pub const FOLDABLE: &str = "▸";

/// 一条 `todo` 项那一行开头的三个字形：等待、在做、做完。
pub const TODO_PENDING: &str = "☐";
pub const TODO_IN_PROGRESS: &str = FOLDABLE;
pub const TODO_COMPLETED: &str = "✓";

/// 提示符。它后面那个空格算在这个常量里 —— 布局留出的列数就是它的宽度
/// （[`crate::render::editor::prompt_columns`]），所以不另立一个「提示符 + 空格」。
pub const PROMPT: &str = "❱ ";

/// 回合条三格：焦点、普通、这一列没地方放的单位。
pub const RAIL_FOCUS: &str = "┃";
pub const RAIL_CELL: &str = "┊";
pub const RAIL_TRUNCATED: &str = "⋮";

/// 问卷选项的四个标记：多选已选 / 多选未选 / 单选已选 / 单选未选。光标（`>`）与它们正交 ——
/// 光标说 `Enter` 会确认哪个，标记说哪些被选中了。
pub const CHOICE_CHECKED: &str = "[x]";
pub const CHOICE_UNCHECKED: &str = "[ ]";
pub const CHOICE_PICKED: &str = "●";
pub const CHOICE_UNPICKED: &str = "○";

/// 行内截断：一行放不下时由它收尾。
pub const ELLIPSIS: &str = "…";

/// 占比条的满格与空格。它归符号表，因为它是屏幕上的一格字形，而不是某个模块私有的画法。
pub const BAR_FULL: char = '▓';
pub const BAR_EMPTY: char = '░';

/// 状态行四段之间的那条线。**是框架那套虚线**（`┆`），不是内容那套实线 —— 它按
/// `.scratch/tui-visual-language/spec.md` §12 归符号表，也归 [`CHROME`](super::palette::CHROME)。
pub const STATUS_SEPARATOR: &str = "┆";

/// 附属行的缩进：2 格。界面上只有这一个字符级的「往后退一档」。
pub const INDENT: &str = "  ";

/// 并列控件之间的距离：3 格（按钮之间，以及问卷页脚的进度与第一个按钮之间）。
pub const GAP: &str = "   ";

/// 一行里各条目的分隔。
pub const SEP: &str = " · ";

/// 一项那一行开头的字形。一张 [`mode_label`] 那样的表，于是「哪个字形是什么意思」只有
/// 一个归宿，左栏那一页自己一个都不留。
pub fn todo_glyph(status: crate::tools::todo::Status) -> &'static str {
    match status {
        crate::tools::todo::Status::Pending => TODO_PENDING,
        crate::tools::todo::Status::InProgress => TODO_IN_PROGRESS,
        crate::tools::todo::Status::Completed => TODO_COMPLETED,
    }
}

/// `todo` 页的计数行：`已完成 2/5`。
pub fn todo_count(completed: usize, total: usize) -> String {
    format!("已完成 {completed}/{total}")
}

/// 顶替那些这一页没地方放的项的那一行：`＋3 项`。
pub fn todo_overflow(hidden: usize) -> String {
    format!("＋{hidden} 项")
}

/// 页还没建出来的页签说的话。是一句话而不是一块空面板，于是读的人知道它是**没做完**、
/// 不是坏了，而点出票号让这个原因可以核对（spec §3）。
pub fn tab_placeholder() -> &'static str {
    "此页尚未实现（另有票在跟）"
}

/// 状态行里模型窗口有多满的那个**值**：`6%`，或者在还没有一次调用报出输入 token 之前是
/// `—`。标签由 [`status_row`] 补。
///
/// 短，是因为状态行那一条线要与模型、模式、状态词共享：带上限与括号里百分比的完整那一对是
/// 左栏的字段（spec §5）。
pub fn context_share_value(used: Option<u64>, usable: u64) -> String {
    match used {
        Some(used) => format!("{}%", used.saturating_mul(100) / usable.max(1)),
        None => PANEL_UNKNOWN.to_owned(),
    }
}

/// 状态行里一段的角色（`.scratch/tui-visual-language/spec.md` §17）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    /// 段里的标签：退后一档。
    Label,
    /// 段里的值：正文档。
    Value,
    /// 段与段之间那条线。
    Separator,
}

/// 状态行里的一段：`kind` 说它该长什么样，`text` 是它写什么。
///
/// 分成两件事，是因为「标签退后、值靠前、分隔符只是线」这条层级只有画家能兑现，而宽度阶梯
/// 只有这里知道 —— 一个纯文本的 `String` 到不了前者（§17）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusPart {
    pub kind: StatusKind,
    pub text: String,
}

impl StatusPart {
    fn label(text: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Label,
            text: text.into(),
        }
    }

    fn value(text: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Value,
            text: text.into(),
        }
    }

    fn separator(text: impl Into<String>) -> Self {
        Self {
            kind: StatusKind::Separator,
            text: text.into(),
        }
    }
}

/// 状态行：`模型 X │ Y │ 上下文 n% │ 状态词`，宽度阶梯折在里面。
///
/// 四段、三档角色：标签退后、值靠前、分隔符只是线（§17）。**标签只有 `模型` 与 `上下文`
/// 两个**：模式的名字自己就是一个值（prototype 的 `模型 kimi │ 询问 │ 上下文 42%`）。
///
/// 降级顺序是**先丢模型、再丢模式，最后剩「上下文 + 状态词」** —— 「在跑」最后才丢，它住在
/// 永远在场的那一行上（§16–§18）。
///
/// 刻意**没有**一档是把整行拿走。那需要的宽度比 [`super::layout::MIN_WIDTH`] 还窄，所以
/// 这一行永远会画；一个连最后一档都放不下的 `width` 由画家去截。
pub fn status_row(
    model: &str,
    mode: Mode,
    share: &str,
    word: &str,
    width: usize,
) -> Vec<StatusPart> {
    // 一段 = 标签 + 值；模式与状态词两段只有值。段与段之间是 ` │ `，整行前面留一格。
    let segments: [Vec<StatusPart>; 4] = [
        vec![
            StatusPart::label(format!("{PANEL_MODEL} ")),
            StatusPart::value(model),
        ],
        vec![StatusPart::value(mode_label(mode))],
        vec![
            StatusPart::label(format!("{PANEL_CONTEXT} ")),
            StatusPart::value(share),
        ],
        vec![StatusPart::value(word)],
    ];
    let row = |segments: &[Vec<StatusPart>]| {
        let mut parts = vec![StatusPart::value(" ")];
        for (index, segment) in segments.iter().enumerate() {
            if index > 0 {
                parts.push(StatusPart::separator(format!(" {STATUS_SEPARATOR} ")));
            }
            parts.extend(segment.iter().cloned());
        }
        parts
    };
    for rung in [&segments[..], &segments[1..], &segments[2..]] {
        let parts = row(rung);
        let columns: usize = parts
            .iter()
            .map(|part| part.text.cell_width() as usize)
            .sum();
        if columns <= width {
            return parts;
        }
    }
    row(&segments[2..])
}

/// 一场会话跑在其下的 `Mode`，用中文点名。
pub fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Readonly => "只读",
        Mode::Ask => "询问",
        Mode::Workspace => "工作区",
        Mode::Auto => "自动",
    }
}

/// 启动横幅：这场会话是什么，用中文标签。
pub fn banner(session: &str, model: &str, mode: Mode, dir: &str, continued: bool) -> String {
    let tail = if continued { "（已继续）" } else { "" };
    format!(
        "fs-agent：会话 {session} · 模型 {model} · 模式 {} · {dir}{tail}",
        mode_label(mode)
    )
}

/// 没有可回滚的东西时的 `/undo`。
pub fn nothing_to_undo() -> &'static str {
    "没有可撤销的修改"
}

/// 一条斜杠命令在菜单里被提供、在提示里被点名时的样子。
///
/// 这是**面向人**的那张列表，`/` 菜单与未知命令那两处文本都由它建出来。一次提交*意味着*
/// 什么仍然归循环的解析器；这里有一个解析器不认得的名字会是 bug，不是策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// 不带斜杠的名字，与必须打出来的完全一致。
    pub name: &'static str,
    /// 一句话说明它做什么。
    pub description: &'static str,
}

/// 内建的斜杠命令，按每一份列表显示它们的顺序。
///
/// 这张表管两处：`/` 菜单列出的名字（`src/cli.rs` 直接从这里建目录），以及未知命令那段文本
/// 点名的东西。**循环解析哪些名字是另一回事** —— 各自的参数形状不同，那里只能是它自己的
/// `match` —— 所以两边的一致性由 `src/cli.rs` 的一条测试钉住：表里的每一个名字都被解析成
/// 一条内建命令，而解析认的每一个名字也都在表里。
///
/// 顺序是固定的**声明顺序**，不按字母：`quit` 与 `exit` 是一对别名，挨着排。
///
/// **一族命令用连字符写成一条**（`/goal-new`，将来 `/goal-list` / `/goal-show` / `/goal-rm`
/// 照走）：空格形状的子命令在 `/` 菜单里补不出来（菜单按命令名的前缀过滤，`new` 是第二个词），
/// 而 `/goal-` 一个前缀就能把这一族全列出来。
pub static BUILT_IN_COMMANDS: [Command; 7] = [
    Command {
        name: "undo",
        description: "回滚上一次编辑",
    },
    Command {
        name: "discuss",
        description: "起一场多角色讨论（用本会话的上下文）",
    },
    Command {
        name: "goal-new",
        description: "从一批票生成一份目标清单：`/goal-new <名字> <来源>…`",
    },
    Command {
        name: "loop",
        description: "选定目标并连续工作：`/loop <名字>`",
    },
    Command {
        name: "clear",
        description: "结束当前会话、开一个新的",
    },
    Command {
        name: "quit",
        description: "退出会话",
    },
    Command {
        name: "exit",
        description: "退出会话（`/quit` 的别名）",
    },
];

/// 一条诊断行的程序名前缀。
///
/// 它住在措辞层，于是这个前缀只有一个归宿 —— CLI 那些 `fs-agent: …` 的行都从这里出去，而
/// 语言护栏那一侧也不必把同一个前缀数上二十几遍。
pub fn fs_agent(message: &str) -> String {
    format!("fs-agent: {message}")
}

/// `/goal-new` 的用法：参数不对时说的那句。
pub fn goal_usage() -> &'static str {
    "用法：/goal-new <名字> <来源>… [--force]；来源是 feature 目录或票路径，多个来源用空格分开"
}

/// 一份目标清单生成了。
pub fn goal_created(name: &str, entries: usize, path: &str) -> String {
    format!("目标 {name} 已写入 {path}，{entries} 条")
}

/// 没有数据目录时，`/goal-new` 与 `/loop` 都无从下手。
pub fn no_goal_dir() -> &'static str {
    "找不到数据目录（`XDG_DATA_HOME` 或 `HOME` 都没设），目标清单没地方放"
}

/// `/loop` 的用法。
pub fn loop_usage() -> &'static str {
    "用法：/loop <目标名字>"
}

/// `/loop` 的三条启动边界（`.scratch/goal-loop/spec.md` §4）。每一条各说各的人话。
pub fn loop_unknown_goal(name: &str) -> String {
    format!("找不到目标 {name}；先用 `/goal-new {name} <feature 目录或票路径>` 生成一份清单")
}

/// 目标已经全部完成：没活可干。
pub fn loop_no_work(name: &str) -> String {
    format!("目标 {name} 的条目已经全部完成，没活可干")
}

/// 这个会话已经有一个 loop 在跑。
pub fn loop_already_running(name: &str) -> String {
    format!("一个 loop 正在跑（{name}）；要停就按 Esc")
}

/// 清单文件在、但读不出来时说的话：它是人手改过的文件。
pub fn loop_broken_manifest(message: &str) -> String {
    format!("目标清单读不了：{message}")
}

/// `/loop` 开跑了：目标名，以及从流派生出来的进度。
pub fn goal_loop_started(name: &str, completed: usize, total: usize) -> String {
    format!("开始做目标 {name}（已完成 {completed}/{total}）；要停就按 Esc")
}

/// 过半时注入给模型的那段话（§6）。
///
/// 措辞是「把还没落流的东西落下来」，**不是**「你该收敛了」：再过一会儿历史会被压成摘要，
/// 而摘要只留得住它读得到的东西 —— 只在脑子里、没写进 `todo`、没写成结论的推理会丢。这条
/// 提醒是给它一次自救的机会。
pub fn goal_reminder() -> &'static str {
    "上下文已经过半。把还没落流的东西落下来：还没写进 `todo` 的下一步、还没写成结论的发现、\
     还没写下来的文件与命令。再过一会儿历史会被压成摘要，而摘要只留得住它读得到的东西 —— \
     只在脑子里、没落进流里的推理会丢。"
}

/// 提醒注入了（§6）：说给用户听的一行，与给模型的那段话是两件事。
pub fn goal_reminded(percent: u64) -> String {
    format!("上下文已到 {percent}%，已提醒模型把还没落流的东西落下来")
}

/// 翻页那一下的记账（§7）：它必然打掉前缀缓存，所以别让它悄悄发生。
pub fn goal_rolled_over(from: &str, to: &str, compacted: bool) -> String {
    let what = if compacted {
        "历史压成摘要"
    } else {
        "清场"
    };
    format!("{what}：会话 {from} → {to}（这一下会打掉前缀缓存，它是低频的）")
}

/// 目标循环跑着的时候按 `Esc`，问的那一句（§5）。
///
/// 安全的那一个答案排在前面，而且它也是 `Enter` 与 `Esc`：要停得**明确选**。
pub fn goal_stop_title() -> &'static str {
    "目标循环正在跑"
}

/// 同一个问题的那行说明：说清哪个键是哪个意思。
pub fn goal_stop_body() -> &'static str {
    "误按一下不该掐掉一个已经跑了很久的目标。要继续跑：`Enter` 或 `Esc`；要停下并收尾：`s`。"
}

/// 回答那个问题的键。
pub static GOAL_STOP_CHOICES: [Choice; 2] = [
    Choice {
        key: 'c',
        label: "继续跑",
    },
    Choice {
        key: 's',
        label: "停下",
    },
];

/// 档位不够时说的人话（`.scratch/goal-loop/spec.md` §5）。
pub fn loop_needs_unattended_mode(mode: &str) -> String {
    format!(
        "{mode} 档下跑不了无人值守的目标循环：第一次写就会停下来等人。换 `workspace` 或 `auto` \
         档（`--mode` 或 `[permissions] mode`）再试"
    )
}

/// 输入区禁言时的那一行提示（§5）。
pub fn input_muted() -> &'static str {
    "无人值守的目标循环跑着，输入区在这段时间里禁言；要停就按 Esc"
}

/// 恢复时说的那一行（§10）：接着说。
pub fn resumed_goal(name: &str) -> String {
    format!("上次是异常中断，接着做目标 {name}")
}

/// 恢复时说的那一行（§10）：上次是正常收尾，所以停在这里。
pub fn resumed_closed_goal(name: &str) -> String {
    format!("上次的目标 {name} 是正常收尾，停在这里等人")
}

/// `/clear` 干完了：说出会话换成了哪一个（旧的那个还在磁盘上）。
pub fn cleared(from: &str, to: &str) -> String {
    format!("已结束会话 {from}，开了新的会话 {to}（旧的还在磁盘上，`--continue` 打开最新那个）")
}

/// 循环跑着的时候 `/clear` 打不出来（§5、§12）：输入区在那段时间里禁言。
pub fn clear_while_looping() -> &'static str {
    "一个 loop 正在跑，输入区在那段时间里禁言；要停就按 Esc"
}

/// provider 调用失败、还要再试一次（§9）。
pub fn provider_retry(failures: u32) -> String {
    format!("provider 调用失败（第 {failures} 次），等一会儿再驱动这个回合")
}

/// 「停下并报告」的那一段（§5、§9）：说什么停了、为什么、卡在哪儿。
///
/// 它是给人读的，而**同一条事实也落了流**（`GoalStopped`）—— 报告不能只在屏幕上刷过去。
pub fn goal_stopped(
    name: &str,
    reason: crate::events::GoalStopReason,
    count: u32,
    stuck: &[String],
) -> String {
    use crate::events::GoalStopReason;
    let why = match reason {
        GoalStopReason::NoProgress => {
            format!("连续 {count} 次翻页没有任何条目完成，目标 {name} 卡住了，已经停下")
        }
        GoalStopReason::ProviderFailed => {
            format!("provider 调用连着失败 {count} 次，目标 {name} 已停下，不再重试")
        }
        GoalStopReason::UserStopped => format!("目标 {name} 被主动停下"),
        GoalStopReason::BudgetExhausted => {
            format!("目标 {name} 撞上了 token 额度，已降级收尾")
        }
    };
    if stuck.is_empty() {
        why
    } else {
        format!("{why}；还卡在这些条目上：{}", stuck.join("、"))
    }
}

/// 目标完成：条目情况与跨了几个会话，一行说完。汇总本身落在流上，也由那次调用写进转录。
pub fn goal_completed_notice(
    name: &str,
    completed: usize,
    total: usize,
    sessions: usize,
) -> String {
    format!("目标 {name} 完成：{completed}/{total} 条，跨 {sessions} 个会话")
}

/// `todo` 引用了清单里没有的 id：忽略它，但**不静默** —— 沉默会让模型以为它记下了。
pub fn goal_unknown_ids(name: &str, ids: &[String]) -> String {
    format!(
        "目标 {name} 的清单里没有这些条目 id：{}；它们被忽略了，但清单是封闭的，\
         新工作请用 `goal_note` 记下",
        ids.join("、")
    )
}

/// 内建命令作为一行提示：`可用：/undo、/discuss、/quit`。
///
/// 一个生成器，于是菜单的列表与未知命令那处文本不会漂开。
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

/// 循环不认识的一条斜杠命令。它点名内建命令，以及 —— 有的话 —— 用户可以按名字载入的
/// 技能，于是 `/` 一直可发现。
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
        names.push(ELLIPSIS.to_owned());
    }
    format!(
        "未知命令 {command}（{built_ins}；技能：{}）",
        names.join("、")
    )
}

/// 用户按名字载入了一个技能，立刻就要跑。
pub fn skill_loaded(name: &str) -> String {
    format!("已加载技能 {name}")
}

/// 一个裸 `/<skill>`：正文**载入，而且回合开跑**，因为正文就是指令。等任务才动的技能，
/// 会是一条用户得调两次的命令。
pub fn skill_started(name: &str) -> String {
    format!("已加载技能 {name}，按技能正文开始")
}

// ---------------------------------------------------------------------------
// 参数解析
// ---------------------------------------------------------------------------

/// 命令不认识的一个参数。
pub fn unknown_argument(arg: &str) -> String {
    format!("未知参数 {arg}")
}

/// `--mode` 的值点不出任何模式。它列出那四档，因为最可能到这里的词是 `plan` —— 这个
/// 版本已经没有的那一档模式。
pub fn unknown_mode(mode: &str) -> String {
    format!(
        "--mode 只认 readonly / ask / workspace / auto，收到 `{mode}`；从前的 plan 模式\
         已经取消，计划交给模型自己的 todo 工具"
    )
}

/// 给了旗标、却没给它需要的值。
pub fn needs_value(flag: &str) -> String {
    format!("{flag} 需要一个值")
}

/// 需要数字、却拿到别的东西的旗标。
pub fn needs_number(flag: &str, value: &str) -> String {
    format!("{flag} 需要一个数字，得到 {value}")
}

/// 需要一个路径的旗标。
pub fn needs_path(flag: &str) -> String {
    format!("{flag} 需要一个路径")
}

/// 需要一个模型 id 的旗标。
pub fn needs_model(flag: &str) -> String {
    format!("{flag} 需要一个模型 id")
}

/// 位置参数比这条命令该收的多。
pub fn extra_argument(arg: &str) -> String {
    format!("多余的参数 {arg}")
}

/// 同时要了 `--plain` 与 `--tui`。
pub fn renderers_mutually_exclusive() -> &'static str {
    "--plain 与 --tui 互斥：每个进程只有一个渲染器"
}

/// 没有动词的 `sessions`。
pub fn sessions_needs_verb() -> &'static str {
    "sessions 需要一个子命令"
}

/// 不存在的 `sessions` 动词。
pub fn unknown_sessions_verb(verb: &str) -> String {
    format!("未知的 sessions 子命令 {verb}")
}

/// 没有 id 的 `sessions show`。
pub fn show_needs_id() -> &'static str {
    "sessions show 需要会话 id"
}

/// 没有 id 的 `sessions replay`。
pub fn replay_needs_id() -> &'static str {
    "sessions replay 需要会话 id"
}

/// 没有 `--speaker` 的 `sessions replay`。
pub fn replay_needs_speaker() -> &'static str {
    "sessions replay 需要 --speaker"
}

/// 没有 id 的 `sessions stats`。
pub fn stats_needs_id() -> &'static str {
    "sessions stats 需要会话 id"
}

/// 哪个桶里都没有的一个会话 id。id 与查找标签原样保留。
pub fn no_session(id: &str, where_: &str) -> String {
    format!("{where_} 中没有会话 {id}")
}

/// `sessions` 搜过的那个桶的标签（以及把它放宽的那次扫描）。
pub fn session_search_label(cwd: &str) -> String {
    format!("{cwd}（或任何其他桶）")
}

/// 一个读不了的已存文件。路径与 detail 原样保留。
pub fn cannot_read(path: &str, detail: &str) -> String {
    format!("无法读取 {path}：{detail}")
}

// ---------------------------------------------------------------------------
// 启动期的拒绝与失败
// ---------------------------------------------------------------------------

/// 拒绝以 root 跑（spec §20）。detail 保持英文：它点名系统调用与 uid，那是诊断线索。
pub fn root_refusal() -> &'static str {
    "拒绝以 root（euid 0）启动：本工具的每一条护栏都假定最坏情况留在你的工作区内，\
     而 root 的一次误判是系统级的。请用你的普通用户运行；没有绕过开关。"
}

/// 异步运行时建不起来。
pub fn startup_runtime(detail: &str) -> String {
    format!("无法启动异步运行时：{detail}")
}

/// `prune --dry-run` 点名一个它将会删掉的会话。id 与路径原样保留。
pub fn prune_would_remove(id: &str, dir: &str) -> String {
    format!("将删除 {id}（{dir}）")
}

/// `prune` 点名一个它删掉的会话。
pub fn prune_removed(id: &str, dir: &str) -> String {
    format!("已删除 {id}（{dir}）")
}

/// 配置读不了。
pub fn startup_config(detail: &str) -> String {
    format!("无法读取配置：{detail}")
}

/// `/` 菜单里一条模板的第二列（票 17）：模板自己没写说明时，把参数名列一遍。
pub fn mcp_prompt_description(server: &str, names: &[&str]) -> String {
    if names.is_empty() {
        format!("{server} 的提示词模板")
    } else {
        format!("{server} 的提示词模板：{}", names.join("、"))
    }
}

/// 点名了一个这一轮清单里没有的模板（清单刷新过之后才会出现）。
pub fn mcp_prompt_unknown(server: &str, prompt: &str) -> String {
    format!("`/{server}:{prompt}` 不在这一轮的模板清单里；重开会话再看一眼菜单")
}

/// 命令行给的参数比模板声明的多。
pub fn mcp_prompt_too_many_arguments(name: &str) -> String {
    format!("`/{name}` 的参数比它声明的多；把多出来的那几个去掉")
}

/// 必填参数还缺。
pub fn mcp_prompt_missing_arguments(name: &str, missing: &[&str]) -> String {
    format!("`/{name}` 还缺必填参数：{}", missing.join("、"))
}

/// MCP server 自己写到 stderr 的一行（`.scratch/mcp-support/spec.md` §4）。
///
/// 前缀点明这句话是**外部工具**说的、不是 fs-agent 说的：server 的崩溃信息要进得了日志，但不能
/// 被读成我们自己的诊断。
pub fn mcp_server_stderr(line: &str) -> String {
    format!("[外部工具] {line}")
}

/// `XDG_DATA_HOME` 与 `HOME` 都没设，于是会话无处存放。
pub fn startup_no_session_store() -> &'static str {
    "既没有设置 XDG_DATA_HOME 也没有设置 HOME，会话无处存放"
}

/// 会话存储读不了。
pub fn startup_store_read(detail: &str) -> String {
    format!("无法读取会话存储：{detail}")
}

/// 会话存储里建不了东西。
pub fn startup_store_create(detail: &str) -> String {
    format!("无法创建会话：{detail}")
}

/// 会话存储清理不了。
pub fn startup_store_prune(detail: &str) -> String {
    format!("无法清理会话存储：{detail}")
}

/// `dir` 里没有可继续的会话。
pub fn startup_no_session_to_continue(dir: &str) -> String {
    format!("{dir} 中没有可继续的会话")
}

/// 当前目录定不下来。
pub fn startup_cwd(detail: &str) -> String {
    format!("无法确定当前目录：{detail}")
}

/// 没有任何已配置的 provider 带着密钥。
pub fn probe_no_key() -> &'static str {
    "没有任何已配置的 provider 带密钥。请导出 MOONSHOT_API_KEY 和/或 \
     DEEPSEEK_API_KEY，或在 config.toml 的 [providers.*] 下设置 `api_key`。"
}

// ---------------------------------------------------------------------------
// 错误
// ---------------------------------------------------------------------------

/// 一个会话级失败（spec §2）：code 用中文解释，持久的 detail 原样透传。detail 是诊断
/// 线索，而事件流只追加，所以它在写下时就定稿了 —— ADR 0005 起新流里那段文本是中文的，
/// 升级前的老流里仍是英文。
pub fn session_error(code: &str, detail: &str) -> String {
    format!("[会话错误：{}] {detail}", session_error_code(code))
}

/// 一个会话错误码的中文解释。不认识的 code 原样显示：新的 code 不能被悄悄丢掉。
pub fn session_error_code(code: &str) -> &str {
    match code {
        "discussion_failed" => "讨论失败",
        "synthesis_failed" => "合成失败",
        other => other,
    }
}

/// 一个循环级失败，变体名用中文，detail 原样透传。
pub fn error_report(error: &crate::Error) -> String {
    match error {
        crate::Error::Io(error) => format!("事件流读写失败：{error}"),
        crate::Error::Discussion(detail) => format!("讨论无法组装：{detail}"),
        crate::Error::Undo(detail) => format!("撤销失败：{detail}"),
        crate::Error::Skill(detail) => format!("技能加载失败：{detail}"),
        crate::Error::WorkspaceWithoutSandbox(detail) => {
            format!("这一档模式在这里不可用：{detail}")
        }
    }
}

// ---------------------------------------------------------------------------
// `sessions` 的输出
// ---------------------------------------------------------------------------

/// `sessions ls` 的列名，按顺序。由知道终端列预算的调用方补齐宽度。
pub fn ls_columns() -> [&'static str; 7] {
    ["标识", "工作区", "开始", "用量", "轮次", "消息", "结束"]
}

/// `sessions ls` 在这个桶里什么都没找到。
pub fn no_sessions() -> &'static str {
    "本桶中没有会话"
}

/// 一场从未结束的会话在 `ls` 结束那一列上显示的东西。
pub fn open_session() -> &'static str {
    "未结束"
}

/// `sessions show` 里按轮次分组的分节行。
pub fn round_group(round: u32, mode: Option<RoundMode>) -> String {
    match mode {
        Some(mode) => round_section(round, mode),
        None => format!("── 第 {round} 轮 ──"),
    }
}

/// `sessions show` 轮次之前那一节。
pub fn session_group() -> &'static str {
    "── 会话 ──"
}

/// `--files` 视图里单个轮次的标签。
pub fn round_label(round: u32) -> String {
    format!("第 {round} 轮")
}

/// 一次失败工具调用的原因，作为叙述。
pub fn tool_error(error: &str) -> String {
    format!("错误：{error}")
}

/// 系统消息在 `sessions replay` 里的标签。
pub fn replay_system() -> &'static str {
    "[系统]"
}

/// 用户消息在 `sessions replay` 里的标签，投影记下了发言者时就点名。
pub fn replay_user(name: Option<&str>) -> String {
    match name {
        Some(name) => format!("[用户 {name}]"),
        None => "[用户]".to_owned(),
    }
}

/// 助手消息在 `sessions replay` 里的标签。
pub fn replay_assistant() -> &'static str {
    "[助手]"
}

/// 工具结果在 `sessions replay` 里的标签。
pub fn replay_tool(tool_call_id: &str) -> String {
    format!("[工具 {tool_call_id}]")
}

/// 一条重放的助手消息里的一次工具调用。参数是线级自己的 JSON，原样保留。
pub fn replay_tool_call(name: &str, arguments: &str) -> String {
    format!("→ 调用 {name}({arguments})")
}

// --- `sessions stats` 的标签 -----------------------------------------------

/// 会话级的总计行。
pub fn stats_session(
    tokens: u64,
    calls: usize,
    messages: usize,
    rounds: usize,
    cost: &str,
) -> String {
    format!("会话：{tokens} token，{calls} 次调用，{messages} 条消息，{rounds} 轮{cost}")
}

/// 会话行带价格的那个尾巴。
pub fn stats_cost(cost: f64, model: &str) -> String {
    format!("，${cost:.6}（按 {model} 计价）")
}

/// 无价格的那个尾巴，点名缺失的价目条目：没登记的模型显示为没有价格，绝不是 0
/// （CONTEXT.md：PriceTable）。
pub fn stats_no_cost(model: &str) -> String {
    format!("，无价格（没有 [pricing.{model}] 条目）")
}

/// 一个发言者的花费行。
pub fn stats_speaker(speaker: &str, tokens: u64, calls: usize, hit: &str, cost: &str) -> String {
    format!("  {speaker} {tokens} token  {calls} 次调用  命中 {hit}{cost}")
}

/// 辩论轮次里的缺席图景。
pub fn stats_absence(one_sided: usize, rounds: usize, rate: &str) -> String {
    format!("缺席：{rounds} 轮辩论中有 {one_sided} 轮只有一方作答{rate}")
}

/// 一个发言者的缺席次数。
pub fn stats_absent(name: &str, count: usize) -> String {
    format!("  缺席：{name} ×{count}")
}

/// 编辑匹配梯的结果。
pub fn stats_edits(succeeded: usize, failed: usize) -> String {
    format!("编辑：{succeeded} 次成功，{failed} 次匹配失败")
}

/// 匹配梯上的一档降级。
pub fn stats_match_level(name: &str, count: usize) -> String {
    format!("  匹配层级 {name}：{count}")
}

/// 两条护栏的拒绝次数。
pub fn stats_guards(read_before_write: usize, invalidated: usize) -> String {
    format!("护栏：写前必读 {read_before_write}，读集失效 {invalidated}")
}

/// 派出的与收尾的执行者。
pub fn stats_executors(spawned: usize, finished: usize) -> String {
    format!("执行者：派出 {spawned}，收尾 {finished}")
}

/// 一个执行者的收尾原因。
pub fn stats_executor_reason(name: &str, count: usize) -> String {
    format!("  收尾 {name}：{count}")
}

/// 挂着的钩子做了什么。
pub fn stats_hooks(
    executed: usize,
    pre: usize,
    post: usize,
    feedback: usize,
    failed: usize,
) -> String {
    format!("钩子：执行 {executed}（前 {pre}，后 {post}），反馈 {feedback}，失败 {failed}")
}

/// 问出去的权限询问次数，带上一段已经拼好的裁决尾巴。
pub fn stats_permissions(asked: usize, decided: &str) -> String {
    format!("权限：询问 {asked}{decided}")
}

/// 权限尾巴里的一个裁决。
pub fn stats_decision(count: usize, name: &str) -> String {
    format!("{count} {name}")
}

/// 辩论轮次里的分歧率。
pub fn stats_divergences(divergences: usize, rounds: usize, rate: &str) -> String {
    format!("分歧：{divergences}/{rounds}{rate}")
}

/// 逐轮列表之前的前缀。
pub fn stats_rounds_prefix() -> &'static str {
    "；轮次："
}

/// 逐轮列表里的一轮。`ended` 已经格式化好了，或者为空。
pub fn stats_round(round: u32, mode: RoundMode, calls: usize, ended: &str) -> String {
    format!("#{round} {} {calls} 次调用{ended}", round_mode(mode))
}

/// 逐轮列表里一轮的收尾原因。
pub fn stats_round_ended(reason: StopReason) -> String {
    format!("，结束于 {}", stop_reason(reason))
}

/// 一个停止原因的计数。
pub fn stats_stop(name: &str, count: usize) -> String {
    format!("  停止 {name}：{count}")
}

// ---------------------------------------------------------------------------
// 帮助
// ---------------------------------------------------------------------------
//
// help 返回一个字符串、而不是直接打印，于是措辞层盖得住它、测试也断言得了；打印归
// `main`。

/// 顶层的 `--help`。
pub fn help_main() -> String {
    format!(
        "fs-agent {}\n\n  \
         usage: fs-agent [--plain|--tui] [--continue [ID]] [--config PATH] [--model ID] [--cwd PATH]\n         \
         fs-agent discuss [--plain|--tui] [--config PATH] [--cwd PATH] \"问题\"\n         \
         fs-agent probe [--config PATH] [--model ID]...\n         \
         fs-agent prune [--keep N] [--cwd PATH] [--dry-run]\n         \
         fs-agent sessions <ls|show|replay|stats> [options]\n\n  \
         不带子命令时，fs-agent 在当前工作区启动一个交互会话：终端上用 TUI 渲染，否则用 \
         plain 转录（--plain / --tui 可强制其一）。--continue 继续本工作区最新的会话（`--continue <ID>` 或 `--session <ID>` 续指名的那一场，ID 也可以是它的会话目录）。\
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

/// 没有 `[discussion]` 表时的 `fs-agent discuss`：该写什么。
///
/// 刻意没有缺省名册：替别人挑两个模型，等于把他的钱花在一份他从没选过的配置上。
pub fn discussion_no_roster() -> &'static str {
    "config.toml 里没有 [discussion]：讨论需要两个讨论者，加 `[discussion]` 与 \
     `debaters = [\"kimi-k3\", \"deepseek-v4-pro\"]`（至少两个池子成员；不同厂商最好， \
     同厂商甚至同一个模型也能跑，只是多样性会弱），见 `fs-agent discuss --help`"
}

/// 没有问题可问的 `fs-agent discuss`。
pub fn discuss_needs_question() -> &'static str {
    "discuss 需要一个问句：`fs-agent discuss \"问题\"`，或者把问题从 stdin 传进来"
}

/// `/discuss` 自己没带题、*而且*会话里也还没有题的时侯。
///
/// 一个裸 `/discuss` 讨论用户最后问的那件事；在一场他们什么都还没问的会话里，没有东西可
/// 讨论。
pub fn discuss_needs_in_session_question() -> &'static str {
    "`/discuss` 没有可讨论的题目：写成 `/discuss 你的问题`，或先在这个会话里问一句，\
     不带题目的 `/discuss` 会拿最后一个问题去讨论"
}

/// 交互式会话的 `--help`。
pub fn help_interactive() -> String {
    "fs-agent [options]\n\n  \
     在当前工作区启动一个交互会话。命令：/undo 回滚上一次编辑，/quit 退出；输入 /技能名 直接运行一个技能（可带任务，例如 \
     `/ask-matt 帮我看一下`），包括标了 `disable-model-invocation: true` 的技能。\
     输入 / 会弹出补全窗口，列出全部命令与技能。\
     `/discuss [--debaters A,B] [问题]` 起一场多角色讨论：两个讨论者用**本会话的上下文**\
     各自作答，只在结论冲突时开一轮定向第二轮，最后由合成器画出共识 / 分歧 / 未决；\
     `--debaters 保守,激进` 指定抽池子里的哪两个（不写就随机抽两个），不带问题就用本会话\
     最后一个问题。讨论的事件写进同一个会话，`sessions show` 能一起复盘。\
     TUI 里 Esc 取消正在跑的回合（或正在跑的讨论）；Shift+Tab 在只读 / 询问 / 工作区 / \
     自动四档权限模式之间循环（按严格度排），当前档位显示在状态行。\n\n  \
     --plain            使用 plain 转录（不进 raw 模式）\n  \
     --tui              使用终端界面（全屏外壳）\n  \
     --continue, -c [ID]  接着跑：不写 ID 就是本工作区最新的会话，写了就续 ID 那一场\n  \
     --session ID         同上，显式拼写；ID 也可以是那场会话的目录\n  \
     --config PATH      要加载的配置文件\n  \
     --model ID         要运行的模型（默认：配置里的 default_model）\n  \
     --mode MODE        权限模式：readonly / ask / workspace / auto（默认：配置里的 [permissions] mode）\n  \
     --cwd PATH         工作区（默认：当前目录）"
        .to_owned()
}

/// `discuss --help`。
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

/// `probe --help`。
pub fn help_probe() -> String {
    "fs-agent probe [--config PATH] [--model ID]...\n\n  \
     对每个模型在同一会话里发送两次真实回合，并打印每次的 input/output/cached/miss。\
     不带 --model 时探测每一个 provider 有密钥的模型。"
        .to_owned()
}

/// `prune --help`。
pub fn help_prune() -> String {
    "fs-agent prune [--keep N] [--cwd PATH] [--dry-run]\n\n  \
     删除一个工作区（当前目录，或 --cwd）的会话目录。保留最新的 N 个会话（默认 1：即 \
     --continue 会继续的那个）。一个会话就是一个目录，所以删除是整会话的；--dry-run \
     只列出将删除的内容。除此之外没有任何东西会删除会话。"
        .to_owned()
}

/// `sessions --help`。
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

/// Markdown 里一张图片在终端里的落点。
///
/// 终端暂时画不出图（真图渲染要终端图像协议，留在 spec 的「明确不做」那一节里），
/// 但「这儿有一张图」得说出来 —— 旧的 `!alt (url)` 里那个 `!` 只是手写扫描器的残留噪声
/// （spec §6）。
pub const IMAGE_PLACEHOLDER: &str = "[图片]";
