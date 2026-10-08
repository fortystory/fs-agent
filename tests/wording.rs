//! 措辞层在它自己的接缝上：给定一个领域值，
//! 出来的是哪一句中文。
//!
//! 这是唯一一个断言**精确文本**的层。「每个人类可见短语
//! 只有一个来源」正是这个特性的全部主张，而只有这里的
//! 断言能把「一个产出者」与「同一群字写了四份」分开
//! （spec §Testing Decisions）。画家那边断言的是语义。

use std::path::PathBuf;

use heng::events::{
    ContextSource, Decision, DecisionSource, HistoryReason, RoundMode, SpeakerId, StopReason, Usage,
};
use heng::permissions::{Escalation, Mode};
use heng::render::width::text_columns;
use heng::render::wording::{self, NumberStyle};

#[test]
fn a_round_section_names_its_number_and_mode_in_chinese() {
    assert_eq!(
        wording::round_section(2, RoundMode::Independent),
        "── 第 2 轮（独立首轮）──"
    );
    assert_eq!(
        wording::round_section(3, RoundMode::Targeted),
        "── 第 3 轮（定向第二轮）──"
    );
    assert_eq!(
        wording::round_section(4, RoundMode::Synthesis),
        "── 第 4 轮（合成）──"
    );
}

#[test]
fn every_round_mode_has_one_explicit_chinese_label() {
    assert_eq!(wording::round_mode(RoundMode::Independent), "独立首轮");
    assert_eq!(wording::round_mode(RoundMode::Targeted), "定向第二轮");
    assert_eq!(wording::round_mode(RoundMode::Synthesis), "合成");
}

#[test]
fn a_turn_start_and_end_read_in_chinese() {
    assert_eq!(wording::turn_started(1), "回合开始（第 1 次迭代）");
    assert_eq!(wording::turn_ended(StopReason::Completed), "回合结束：完成");
}

#[test]
fn every_stop_reason_has_one_explicit_chinese_phrase() {
    let cases = [
        (StopReason::Completed, "完成"),
        (StopReason::MaxIterations, "达到迭代上限"),
        (StopReason::Aborted, "已取消"),
        (StopReason::MistakeLimit, "达到错误上限"),
        (StopReason::Error, "出错"),
        (StopReason::Consensus, "达成共识"),
        (StopReason::NoDivergence, "无分歧"),
        (StopReason::RoundsExhausted, "轮次用尽"),
        (StopReason::BudgetExhausted, "预算用尽"),
    ];
    for (reason, phrase) in cases {
        assert_eq!(
            wording::stop_reason(reason),
            phrase,
            "对 {reason:?} 的映射是显式写下的"
        );
    }
}

#[test]
fn a_round_and_a_session_ending_read_in_chinese() {
    assert_eq!(
        wording::round_ended(2, StopReason::NoDivergence),
        "第 2 轮结束：无分歧"
    );
    assert_eq!(
        wording::session_ended(StopReason::BudgetExhausted),
        "会话结束：预算用尽"
    );
}

#[test]
fn a_discussion_reports_why_it_stopped_and_who_was_absent() {
    assert_eq!(
        wording::discussion_ended(StopReason::NoDivergence, 1, &[]),
        "讨论结束：无分歧（跑了 1 轮）"
    );
    // 缺席的那一方会被点名：一轮里只有一方作答，不是一轮一致。
    assert_eq!(
        wording::discussion_ended(
            StopReason::NoDivergence,
            2,
            &[SpeakerId::Debater("deepseek".into())]
        ),
        "讨论结束：无分歧（跑了 2 轮）；缺席：[deepseek]"
    );
    assert_eq!(wording::discussion_pair("保守", "激进"), "保守 × 激进");
    // 讨论者按人物命名，两者不同时把模型放在旁边。
    assert_eq!(
        wording::debater_label("保守", "deepseek-v4-pro"),
        "保守（deepseek-v4-pro）"
    );
    assert_eq!(
        wording::debater_label("kimi-k3", "kimi-k3"),
        "kimi-k3",
        "简写只说一遍"
    );
    assert_eq!(
        wording::needs_two_debaters("保守"),
        "--debaters 需要两个名字（逗号分隔，例如 `--debaters 保守,激进`），得到 `保守`"
    );
    assert_eq!(
        wording::unknown_debater("激进", &["保守", "审查"]),
        "池子里没有叫 `激进` 的讨论者；可用：`保守`、`审查`"
    );
    // 回执给的是**能直接续上**的那条命令：`-c <id>` 现在吃 id 了（那一行是终端交还之后给人粘的）。
    assert_eq!(
        wording::session_receipt("20260922T101500Z-ab12"),
        "会话 20260922T101500Z-ab12；接着跑：heng -c 20260922T101500Z-ab12"
    );
    // 按 id 续上了一场面**别的工作区**的会话：横幅之前先说一句目录跟着它走了。
    assert_eq!(
        wording::session_followed("/home/forty/other"),
        "接着跑的是 /home/forty/other 里的那场会话"
    );
    // `discuss` 拒绝启动的那两条路都会说清该改怎么做。
    assert!(
        wording::discussion_no_roster().contains("[discussion]"),
        "{}",
        wording::discussion_no_roster()
    );
    assert!(
        wording::discussion_no_roster().contains("debaters"),
        "{}",
        wording::discussion_no_roster()
    );
    assert_eq!(wording::question_prompt(), "问题> ");
    // 在活着的会话上跑 `/discuss`：哪些模型、哪个问题。
    assert_eq!(
        wording::discussion_starting(
            "保守（deepseek-v4-pro）",
            "激进（deepseek-flash）",
            "换个角度\n再说一次？"
        ),
        "开始讨论：保守（deepseek-v4-pro） × 激进（deepseek-flash）；题目：换个角度"
    );
    assert!(
        wording::discuss_needs_in_session_question().contains("/discuss 你的问题"),
        "{}",
        wording::discuss_needs_in_session_question()
    );

    // 同一家厂商 —— 或者同一个模型用两次 —— 是允许的，而且会明说
    // 出来，而不是冒充设计假设的那种异构。
    assert_eq!(
        wording::discussion_same_model("kimi-k3"),
        "提示：两个讨论者都是 kimi-k3——同一个模型问两遍，剩下的差异只有采样噪声"
    );
    assert_eq!(
        wording::discussion_one_vendor("kimi-k3", "k3"),
        "提示：两个讨论者来自同一厂商（kimi-k3 × k3），多样性比设计假设的弱"
    );
}

#[test]
fn a_tool_call_summary_reads_in_chinese() {
    assert_eq!(
        wording::tool_call("read_file", "path=src/lib.rs"),
        "调用 read_file(path=src/lib.rs)"
    );
}

#[test]
fn a_truncated_tool_result_says_so_in_chinese() {
    assert_eq!(wording::tool_output_preview("短", 10), "短");
    assert_eq!(
        wording::tool_output_preview("一二三四五", 3),
        "一二三…（结果已省略）"
    );
}

#[test]
fn placeholders_read_in_chinese() {
    assert_eq!(wording::no_tool_result(), "（流上没有结果）");
    assert_eq!(wording::no_message(), "（没有消息）");
}

#[test]
fn a_hook_line_and_its_point_read_in_chinese() {
    assert_eq!(
        wording::hook("pre_tool_use", "continue"),
        "钩子 工具调用前：continue"
    );
    assert_eq!(
        wording::hook("post_tool_use", "feedback: looks fine"),
        "钩子 工具调用后：feedback: looks fine"
    );
    assert_eq!(
        wording::hook_feedback("feedback: ok"),
        "[钩子] feedback: ok"
    );
}

#[test]
fn executor_and_divergence_narration_read_in_chinese() {
    assert_eq!(wording::executor_spawned("kimi-1"), "派出执行者 kimi-1");
    assert_eq!(
        wording::executor_finished("kimi-1", StopReason::Completed, "read the notes"),
        "执行者 kimi-1 收尾：完成 — read the notes"
    );
    assert_eq!(wording::divergence("the seam"), "分歧：the seam");
    assert_eq!(wording::agent_error("boom"), "错误：boom");
}

#[test]
fn a_usage_line_reads_in_chinese() {
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 2,
        cached_tokens: 6,
        miss_tokens: 4,
        reasoning_tokens: None,
    };
    assert_eq!(
        wording::usage_summary(&usage),
        "用量 in=10 out=2 cached=6 miss=4"
    );
    assert_eq!(wording::reasoning_marker(), "[思考] ");
    assert_eq!(wording::reasoning_label(), "（思考）");
    assert_eq!(wording::message_complete(), "消息完成");
    assert_eq!(wording::tool_completed(), "工具完成");
}

#[test]
fn a_usage_tail_keeps_only_the_two_numbers_it_is_read_for() {
    // 轨迹页那一段只放最常读的两项，缓存那一半仍归 `usage_summary` 那条通道
    // （ADR 0016、`.scratch/trace-usage-tail/spec.md` §3、§5）。
    let usage = Usage {
        input_tokens: 10,
        output_tokens: 2,
        cached_tokens: 6,
        miss_tokens: 4,
        reasoning_tokens: None,
    };
    assert_eq!(wording::usage_tail(&usage), "in=10 out=2");
    assert_eq!(wording::total_tail(&usage), "合计 in=10 out=2");
}

#[test]
fn a_permission_ask_and_verdict_read_in_chinese() {
    // 问题显示工具与那次具体的调用，从不显示 id。
    assert_eq!(
        wording::permission_asked(Some("bash"), "command=rm -rf /"),
        "权限询问：bash（command=rm -rf /）"
    );
    assert_eq!(
        wording::permission_asked(Some("read_file"), ""),
        "权限询问：read_file"
    );
    assert_eq!(
        wording::permission_asked(None, "command=ls"),
        "权限询问（command=ls）"
    );
    assert_eq!(
        wording::permission_decided(Decision::Allow, DecisionSource::Policy, Some("mode ask")),
        "权限裁决：允许（策略）：mode ask"
    );
    assert_eq!(
        wording::permission_decided(Decision::Deny, DecisionSource::User, None),
        "权限裁决：拒绝（用户）"
    );
}

#[test]
fn every_permission_decision_and_source_has_an_explicit_chinese_phrase() {
    assert_eq!(wording::decision(Decision::Allow), "允许");
    assert_eq!(wording::decision(Decision::Ask), "询问");
    assert_eq!(wording::decision(Decision::Deny), "拒绝");
    assert_eq!(wording::decision_source(DecisionSource::User), "用户");
    assert_eq!(wording::decision_source(DecisionSource::Hook), "钩子");
    assert_eq!(wording::decision_source(DecisionSource::Policy), "策略");
}

#[test]
fn the_input_line_prompts_read_in_chinese() {
    assert_eq!(
        wording::permission_prompt_with_context(
            "write_file",
            "file_path=a.txt",
            "mode ask",
            None,
            None
        ),
        "权限询问：write_file（file_path=a.txt）？原因：mode ask [y] 允许 / [a] 总是允许 / [n] 拒绝 "
    );
    // 四档模式，用的是状态行与 banner 那套词；`计划` 随模式本身一起退场了
    // （`.scratch/todo-and-modes`），第四档是 `workspace`（`.scratch/workspace-mode`）。
    assert_eq!(wording::mode_label(Mode::Readonly), "只读");
    assert_eq!(wording::mode_label(Mode::Ask), "询问");
    assert_eq!(wording::mode_label(Mode::Workspace), "工作区");
    assert_eq!(wording::mode_label(Mode::Auto), "自动");
    assert!(
        wording::unknown_mode("plan").contains("plan"),
        "这条拒绝引用了当时写下的东西"
    );
}

#[test]
fn an_escalation_prompt_spells_out_the_reason_and_the_paths() {
    let escalation = Escalation {
        justification: "构建产物要写到 ~/.npm 的缓存目录".to_owned(),
        writable_paths: vec![PathBuf::from("/home/ada/.npm")],
    };
    let lines = wording::permission_escalation_lines(&escalation);
    assert_eq!(lines[0], "被沙箱拒绝，申请写工作区之外");
    assert_eq!(lines[1], "理由：构建产物要写到 ~/.npm 的缓存目录");
    assert_eq!(lines[2], "要放开的路径：/home/ada/.npm");

    // 发起者不是主会话时点名说话人；命令行与按钮行仍在最后一行。
    let prompt = wording::permission_prompt_with_context(
        "bash",
        "command=echo x",
        "沙箱升级（一次批准）",
        Some(&SpeakerId::Executor("planner".into())),
        Some(&escalation),
    );
    assert!(prompt.contains("执行者 planner 发起"), "{prompt}");
    assert!(prompt.contains("理由：构建产物"), "{prompt}");
    assert!(prompt.contains("要放开的路径：/home/ada/.npm"), "{prompt}");
    assert!(
        prompt.trim_end().ends_with("[n] 拒绝"),
        "按钮行在最后：{prompt}"
    );
}

#[test]
fn a_question_has_a_title_a_body_and_a_row_of_choices() {
    // 覆盖层的三个部分，各有各的名字：说清在问什么的标题、
    // 说清它关于什么的正文，以及那些键（spec §9）。
    assert_eq!(wording::permission_title(), "权限询问：");
    assert_eq!(
        wording::permission_call("bash", "command=rm -rf /"),
        "bash（command=rm -rf /）"
    );
    assert_eq!(wording::permission_call("read_file", ""), "read_file");
    assert_eq!(wording::paste_title(), "粘贴确认");
    assert_eq!(wording::paste_body(120_000), "粘贴 120000 字符");
    assert_eq!(wording::clear_draft_title(), "清空输入");
    assert_eq!(
        wording::clear_draft_body(),
        "草稿有多行，Esc 会把它们全部丢掉"
    );
    // 每个问题一张表，而只有一行可用的前端走同一次拼接：
    // TUI 画那些条目，plain 控制台打出这段文本。
    assert_eq!(
        wording::choices_text(&wording::PERMISSION_CHOICES),
        "[y] 允许 / [a] 总是允许 / [n] 拒绝"
    );
    assert_eq!(
        wording::choices_text(&wording::PASTE_CHOICES),
        "[y] 粘贴 / [n] 取消"
    );
    assert_eq!(
        wording::choices_text(&wording::CLEAR_CHOICES),
        "[y] 清空 / [n] 保留"
    );
}

#[test]
fn a_speaker_label_names_user_and_system_in_chinese_and_keeps_names() {
    assert_eq!(
        wording::speaker_label(&SpeakerId::Debater("kimi".into())),
        "[kimi]"
    );
    assert_eq!(wording::speaker_label(&SpeakerId::User), "[用户]");
    assert_eq!(wording::speaker_label(&SpeakerId::System), "[系统]");
    assert_eq!(
        wording::speaker_label(&SpeakerId::Executor("kimi-1".into())),
        "[执行者 kimi-1]"
    );
}

#[test]
fn bracketed_hints_read_in_chinese_with_their_enums_explained() {
    assert_eq!(
        wording::context_injected(ContextSource::AgentsMd),
        "[上下文注入：AGENTS.md]"
    );
    assert_eq!(
        wording::context_injected(ContextSource::SkillsCatalog),
        "[上下文注入：技能清单]"
    );
    assert_eq!(
        wording::context_injected(ContextSource::Skill),
        "[上下文注入：技能]"
    );
    assert_eq!(
        wording::context_injected(ContextSource::PlanMode),
        "[上下文注入：计划模式]"
    );
    assert_eq!(
        wording::history(HistoryReason::Regenerate, Some("dropped 3 events")),
        "[历史：重新生成] dropped 3 events"
    );
    assert_eq!(
        wording::history(HistoryReason::Undo, None),
        "[历史：撤销] 历史已被取代"
    );
    assert_eq!(wording::history_reason(HistoryReason::Compaction), "压缩");
    assert_eq!(
        wording::history_reason(HistoryReason::ModeChange),
        "模式变更"
    );
    assert_eq!(wording::diagnostic("boom"), "[诊断] boom");
    assert_eq!(wording::renderer_dropped(5), "渲染器丢弃了 5 个事件");
}

#[test]
fn the_status_line_keeps_the_way_out_and_fills_hints_from_the_front() {
    // 够宽：六条键位提示都在，出口放最后 —— 而状态词**不在**这里，它住在状态行的最后一段
    // （`.scratch/tui-visual-language/spec.md` §18）。
    let wide = wording::status_line(false, 200, false);
    for hint in [
        "enter 发送",
        "ctrl-j 换行",
        "esc 取消",
        "shift+tab 模式",
        "PgUp/PgDn 滚动",
        "ctrl-o 左栏",
        wording::EXIT_HINT_IDLE,
    ] {
        assert!(wide.contains(hint), "{wide}");
    }
    assert!(wide.ends_with(wording::EXIT_HINT_IDLE), "{wide}");
    assert!(!wide.contains("就绪"), "状态词不在这里：{wide}");

    // 阶梯按画出来的宽度实测：空闲的出口是 18 列，一条提示连起来要 31 列。
    assert_eq!(
        wording::status_line(false, 30, false),
        wording::EXIT_HINT_IDLE
    );
    assert_eq!(
        wording::status_line(false, 31, false),
        "enter 发送 · ctrl-c/ctrl-d 退出"
    );
    assert_eq!(
        wording::status_line(false, 45, false),
        "enter 发送 · ctrl-j 换行 · ctrl-c/ctrl-d 退出"
    );
    // 只有空闲行会宣传 `ctrl-d`：一次运行进行中的时候它什么都不做，
    // 所以点它的名正是提示行绝不能做的那件事。
    assert!(
        !wording::status_line(true, 200, false).contains("ctrl-d"),
        "忙碌行不宣传一个什么都不做的键"
    );
    assert!(
        wording::status_line(false, 200, false).contains("ctrl-d"),
        "空闲行会宣传"
    );
    // 比任何提示都窄：剩下的只有出路。
    assert_eq!(
        wording::status_line(true, 8, false),
        wording::EXIT_HINT_BUSY
    );
    assert_eq!(
        wording::status_line(false, 3, false),
        wording::EXIT_HINT_IDLE
    );
}

#[test]
fn the_sidebar_switch_is_hinted_at_the_end_of_the_line() {
    // 它排在最末 —— 位置就是优先级，所以只有最宽的档位看得见它
    // （`.scratch/sidebar-toggle/spec.md` §4）。
    let wide = wording::status_line(false, 200, false);
    assert!(wide.contains("ctrl-o 左栏"), "{wide}");
    assert!(
        wide.ends_with(wording::EXIT_HINT_IDLE),
        "出口仍在最末：{wide}"
    );
    assert!(
        wide.find("ctrl-o 左栏").unwrap() > wide.find("PgUp/PgDn 滚动").unwrap(),
        "新条目排在所有旧条目之后：{wide}"
    );
    // 它是窄档最先丢掉的那一条。
    let narrow = wording::status_line(false, 100, false);
    assert!(
        !narrow.contains("ctrl-o 左栏"),
        "100 列还看不到它：{narrow}"
    );

    // 忙碌行的出口短八列（`ctrl-c 退出` 对 `ctrl-c/ctrl-d 退出`），所以在同一个列宽上
    // 它比空闲行更早看到这一条 —— 同一条阶梯，两条出口。
    let busy = wording::status_line(true, 99, false);
    assert!(busy.contains("ctrl-o 左栏"), "忙碌行在 99 列上：{busy}");
    let idle_here = wording::status_line(false, 99, false);
    assert!(
        !idle_here.contains("ctrl-o 左栏"),
        "空闲行同宽还看不到：{idle_here}"
    );
}

#[test]
fn the_viewer_status_line_hints_only_at_what_a_viewer_can_do() {
    // 没有一行在被读（一次性的 `discuss`，或者一个进行中的回合），
    // 所以 `enter 发送` 与交互循环的模式手势都不在候选里。
    let wide = wording::viewer_status_line(false, 200, false);
    assert_eq!(
        wide, "esc 取消 · PgUp/PgDn 滚动 · ctrl-c/ctrl-d 退出",
        "整条查看器行"
    );
    assert!(wide.ends_with(wording::EXIT_HINT_IDLE), "{wide}");

    // 同一条阶梯：提示从前往后填，出口永远预留，装不下下一条就停。
    assert_eq!(
        wording::viewer_status_line(false, 28, false),
        wording::EXIT_HINT_IDLE
    );
    assert_eq!(
        wording::viewer_status_line(false, 29, false),
        "esc 取消 · ctrl-c/ctrl-d 退出"
    );

    // 忙就读作忙 —— 而一次运行进行中的时候 `ctrl-d` 被忽略，所以
    // 查看器行回到朴素的 `ctrl-c 退出`。
    assert!(!wording::viewer_status_line(true, 200, false).contains("ctrl-d"));
    assert_eq!(
        wording::viewer_status_line(true, 3, false),
        wording::EXIT_HINT_BUSY
    );
}

#[test]
fn the_hint_ladder_is_the_one_the_prototype_measured() {
    // 原型量出来的那些宽度（§10，票 06 §4），于是优先级顺序一改就会在这里显出来，
    // 而不是在终端上。状态词在 `.scratch/tui-visual-language/spec.md` §18 搬去状态行之后，
    // 这一行只填键位提示 —— 于是同一个宽度上比从前多看得见一条。
    //
    // `ctrl-o 左栏` 排在最末：位置就是优先级，所以窄档最先丢的就是它
    // （`.scratch/sidebar-toggle/spec.md` §4）。
    assert_eq!(
        wording::status_line(false, 40, false),
        "enter 发送 · ctrl-c/ctrl-d 退出"
    );
    assert_eq!(
        wording::status_line(false, 60, false),
        "enter 发送 · ctrl-j 换行 · esc 取消 · ctrl-c/ctrl-d 退出"
    );
    assert_eq!(
        wording::status_line(false, 80, false),
        "enter 发送 · ctrl-j 换行 · esc 取消 · shift+tab 模式 · ctrl-c/ctrl-d 退出"
    );
    // 忙那一档的出口短八列，所以同一个列宽上它多装得下一条。
    assert_eq!(
        wording::status_line(true, 60, false),
        "enter 发送 · ctrl-j 换行 · esc 取消 · ctrl-c 退出"
    );
    let full = "enter 发送 · ctrl-j 换行 · esc 取消 · shift+tab 模式 · PgUp/PgDn 滚动 · ctrl-o 左栏 · ctrl-c/ctrl-d 退出";
    assert_eq!(wording::status_line(false, 120, false), full);
    // 在最大宽度上行是稳定的：再没什么可买的了。
    assert_eq!(wording::status_line(false, 174, false), full);
    // 忙只换出口那一段，不换阶梯。
    assert_eq!(
        wording::status_line(true, 120, false).replace("ctrl-c 退出", "ctrl-c/ctrl-d 退出"),
        full
    );
}

#[test]
fn the_raised_exit_hints_say_the_second_press() {
    // 没举手那两句仍然在：它们出现在没举手的状态行里
    // （`.scratch/exit-gesture/spec.md` §2）。
    assert_eq!(wording::EXIT_HINT_IDLE, "ctrl-c/ctrl-d 退出");
    assert_eq!(wording::EXIT_HINT_BUSY, "ctrl-c 退出");

    // 举手那三句：空闲两键对等、忙碌把「回合停了」与「再按会退出」一起说出来、重放里只有
    // `Ctrl-C` 管用。
    assert_eq!(
        wording::EXIT_HINT_IDLE_RAISED,
        "再按一次 ctrl-c/ctrl-d 退出"
    );
    assert_eq!(
        wording::EXIT_HINT_BUSY_RAISED,
        "已取消 · 再按一次 ctrl-c 退出"
    );
    assert_eq!(wording::EXIT_HINT_REPLAY_RAISED, "再按一次 ctrl-c 退出");

    // 举手换掉的是**出口那一段**，四档各有一句。
    assert_eq!(wording::exit_hint(false, false), wording::EXIT_HINT_IDLE);
    assert_eq!(
        wording::exit_hint(false, true),
        wording::EXIT_HINT_IDLE_RAISED
    );
    assert_eq!(wording::exit_hint(true, false), wording::EXIT_HINT_BUSY);
    assert_eq!(
        wording::exit_hint(true, true),
        wording::EXIT_HINT_BUSY_RAISED
    );

    // 换掉之后，别的键位提示照旧在，而旧的出口段不再作为一个条目出现。
    let wide = wording::status_line(false, 200, true);
    assert!(wide.ends_with(wording::EXIT_HINT_IDLE_RAISED), "{wide}");
    assert!(wide.contains("enter 发送"), "{wide}");
    assert!(!wide.contains("· ctrl-c/ctrl-d 退出"), "{wide}");
    assert!(
        wording::status_line(true, 200, true).ends_with(wording::EXIT_HINT_BUSY_RAISED),
        "忙碌那一档"
    );
    assert!(
        wording::viewer_status_line(false, 200, true).ends_with(wording::EXIT_HINT_IDLE_RAISED),
        "查看器那条也一样"
    );
}

#[test]
fn no_hint_ever_names_shift_enter() {
    // 没有键盘增强协议时 `Shift+Enter` 与 `Enter` 分不开，而后者
    // 会提交 —— 所以点它名的提示会是一句假话，而那句假话
    // 在任何单一宽度下都看不出来（spec §10，用户故事 54）。把它们全扫一遍。
    for busy in [false, true] {
        for width in 1..=200 {
            let line = wording::status_line(busy, width, false);
            assert!(
                !line.to_lowercase().contains("shift+enter"),
                "{width}: {line}"
            );
        }
    }
}

#[test]
fn the_scroll_indicator_and_the_minimum_explain_themselves() {
    // 指示器点名到达了什么、怎么去那里；没有计数时
    // 它只剩回到底部（spec §4）。
    assert_eq!(wording::new_content(12), "↓ 12 行新内容 · 点此到底");
    assert_eq!(wording::new_content(1), "↓ 1 行新内容 · 点此到底");
    assert_eq!(wording::back_to_bottom(), "点此到底");
    // 低于下限的终端会被告诉它为什么是空的，用的还是它有的那组数字。
    assert_eq!(wording::too_small(40, 10), "终端太小：至少 40×10");
}

#[test]
fn every_panel_label_is_the_chinese_the_prototype_shows() {
    assert_eq!(wording::PANEL_MODEL, "模型");
    assert_eq!(wording::PANEL_CONTEXT, "上下文");
    // `CONTEXT.md` 里 token 没有中文名，所以它在这里与别处一样。
    assert_eq!(wording::PANEL_TOKENS, "token");
    assert_eq!(wording::PANEL_TURNS, "回合");
    assert_eq!(wording::PANEL_INPUT, "输入");
    assert_eq!(wording::PANEL_OUTPUT, "输出");
    assert_eq!(wording::PANEL_CACHE, "缓存");
}

#[test]
fn the_header_identity_is_the_crate_and_the_version_it_was_built_from() {
    // `scripts/tui-startup-check.py` 拿这个精确字符串当锚，用来区分新的
    // 外壳与任何更旧的东西，而它的期望来自二进制自己的
    // `--version`（`src/cli.rs` 打印 `heng {version}`）。这两处写法
    // 写在两个地方，所以这条把它们钉在一起：改任何一处
    // 都会让检查变红，而不是悄悄什么都匹配不上。
    assert_eq!(
        wording::identity(),
        format!("heng {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn both_forms_of_the_mark_are_one_width_and_the_rows_the_layout_budgeted() {
    // 左栏要么整块画标记、要么完全不画 —— `layout` 在任何东西被画出来
    // 之前就按 `LOGO_WIDTH` 与行数定了这件事。一行宽度不同就会溜过
    // 那道闸、画到边框上去，所以这条契约钉在这里、钉在
    // 字形所在的地方，而不是在画家那边指望它。
    //
    // 两版一起量：篆书那版二十三行，收起来那版五行（矮终端拿到的就是它），而行数由 `layout`
    // 的常数说，两处不能各写一遍。
    let block = wording::logo_lines();
    assert_eq!(
        block.len(),
        heng::render::layout::LOGO_ROWS as usize,
        "块字是布局预留的那些行：{block:?}"
    );
    let compact = wording::logo_lines_compact();
    assert_eq!(
        compact.len(),
        heng::render::layout::LOGO_COMPACT_ROWS as usize,
        "收起来那版是布局预留的那些行：{compact:?}"
    );
    for row in block.iter().chain(compact.iter()) {
        assert_eq!(
            heng::render::width::text_columns(row),
            heng::render::layout::LOGO_WIDTH as usize,
            "每一行都是布局预留的那个宽度：{row:?}"
        );
    }

    // 两版的形状各钉一句：块字拼出来之后里面一个汉字都没有，收起来那版仍是拼音加汉字
    // —— 它们不是同一张图的两种高度，不能互相顶替。
    assert!(
        block.iter().all(|row| !row.contains('衡')),
        "块字那版是点阵，不该含汉字：{block:?}"
    );
    assert!(
        compact[1].contains("héng") && compact[2].contains('衡'),
        "收起来那版还是拼音与汉字：{compact:?}"
    );
}

#[test]
fn a_banner_labels_the_model_mode_and_session_in_chinese() {
    assert_eq!(
        wording::banner("s-1", "kimi-k3", Mode::Ask, "/tmp/ws", false),
        "heng：会话 s-1 · 模型 kimi-k3 · 模式 询问 · /tmp/ws"
    );
    assert_eq!(
        wording::banner("s-1", "kimi-k3", Mode::Auto, "/tmp/ws", true),
        "heng：会话 s-1 · 模型 kimi-k3 · 模式 自动 · /tmp/ws（已继续）"
    );
}

#[test]
fn interactive_feedback_reads_in_chinese() {
    assert_eq!(wording::nothing_to_undo(), "没有可撤销的修改");
    assert_eq!(
        wording::undone(&PathBuf::from("src/a.rs")),
        "已撤销对 src/a.rs 的这次编辑"
    );
    assert_eq!(
        wording::unknown_command("/nope", &[]),
        "未知命令 /nope（可用：/undo、/discuss、/goal-new、/loop、/model、/effort、/clear、/quit、/exit，或直接输入 /<技能名>）"
    );
    assert_eq!(
        wording::unknown_command("/nope", &["ask-matt", "release"]),
        "未知命令 /nope（可用：/undo、/discuss、/goal-new、/loop、/model、/effort、/clear、/quit、/exit；技能：/ask-matt、/release）"
    );
    assert_eq!(wording::skill_loaded("ask-matt"), "已加载技能 ask-matt");
    assert_eq!(
        wording::skill_started("ask-matt"),
        "已加载技能 ask-matt，按技能正文开始"
    );
}

#[test]
fn the_short_help_texts_are_exact_chinese() {
    assert_eq!(
        wording::help_probe(),
        "heng probe [--config PATH] [--model ID]...\n\n  \
         对每个模型在同一会话里发送两次真实回合，并打印每次的 input/output/cached/miss。\
         不带 --model 时探测每一个 provider 有密钥的模型。"
    );
    assert_eq!(
        wording::help_prune(),
        "heng prune [--keep N] [--cwd PATH] [--dry-run]\n\n  \
         删除一个工作区（当前目录，或 --cwd）的会话目录。保留最新的 N 个会话（默认 1：即 \
         --continue 会继续的那个）。一个会话就是一个目录，所以删除是整会话的；--dry-run \
         只列出将删除的内容。除此之外没有任何东西会删除会话。"
    );
}

#[test]
fn the_long_help_texts_are_chinese_and_keep_their_structure() {
    let main = wording::help_main();
    assert!(main.contains("usage: heng"), "{main}");
    assert!(main.contains("配置位于"), "{main}");
    assert!(main.contains("前缀缓存"), "{main}");

    let interactive = wording::help_interactive();
    assert!(interactive.contains("--plain"), "{interactive}");
    // `-c` 除了「续最新」，还吃一个可选的 id（也可以给 `--session`）—— 帮助里得写得出来。
    assert!(interactive.contains("--continue, -c [ID]"), "{interactive}");
    assert!(interactive.contains("--session ID"), "{interactive}");
    // help 必须点名的那些模式入口：旗标，与手势。
    assert!(interactive.contains("--mode MODE"), "{interactive}");
    assert!(
        interactive.contains("只读 / 询问 / 工作区 / 自动"),
        "{interactive}"
    );
    assert!(interactive.contains("Shift+Tab"), "{interactive}");

    // 讨论现在有自己的前端了，两份 help 都这么说。
    assert!(main.contains("heng discuss"), "{main}");
    let discuss = wording::help_discuss();
    assert!(discuss.contains("heng discuss"), "{discuss}");
    assert!(discuss.contains("[discussion] debaters"), "{discuss}");
    assert!(discuss.contains("同厂商"), "同厂商被记录为允许");
    assert!(discuss.contains("CONCLUSION:"), "{discuss}");
    assert!(discuss.contains("3 次调用"), "{discuss}");
    assert!(discuss.contains("sessions show"), "{discuss}");
    assert!(
        !discuss.contains(" 的") && !discuss.contains("永 远"),
        "行续没有留下空格：{discuss}"
    );

    let sessions = wording::help_sessions();
    assert!(sessions.contains("ls [--all]"), "{sessions}");
    assert!(sessions.contains("标准输出只放结果"), "{sessions}");
}

#[test]
fn argument_errors_read_in_chinese() {
    assert_eq!(wording::unknown_argument("--nope"), "未知参数 --nope");
    assert_eq!(wording::needs_value("--config"), "--config 需要一个值");
    assert_eq!(
        wording::needs_number("--keep", "many"),
        "--keep 需要一个数字，得到 many"
    );
    assert_eq!(wording::needs_path("--cwd"), "--cwd 需要一个路径");
    assert_eq!(wording::needs_model("--model"), "--model 需要一个模型 id");
    assert_eq!(wording::extra_argument("x"), "多余的参数 x");
    assert_eq!(
        wording::renderers_mutually_exclusive(),
        "--plain 与 --tui 互斥：每个进程只有一个渲染器"
    );
    assert_eq!(wording::sessions_needs_verb(), "sessions 需要一个子命令");
    assert_eq!(
        wording::unknown_sessions_verb("explain"),
        "未知的 sessions 子命令 explain"
    );
    assert_eq!(wording::show_needs_id(), "sessions show 需要会话 id");
    assert_eq!(
        wording::replay_needs_speaker(),
        "sessions replay 需要 --speaker"
    );
    assert_eq!(wording::stats_needs_id(), "sessions stats 需要会话 id");
}

#[test]
fn startup_failures_read_in_chinese_and_keep_the_detail() {
    assert!(wording::root_refusal().contains("root"));
    assert_eq!(
        wording::startup_runtime("no reactor"),
        "无法启动异步运行时：no reactor"
    );
    assert_eq!(
        wording::startup_config("bad toml at line 2"),
        "无法读取配置：bad toml at line 2"
    );
    assert!(wording::startup_no_session_store().contains("HOME"));
    assert_eq!(
        wording::startup_store_read("permission denied"),
        "无法读取会话存储：permission denied"
    );
    assert_eq!(
        wording::startup_store_create("disk full"),
        "无法创建会话：disk full"
    );
    assert_eq!(
        wording::startup_no_session_to_continue("/tmp/ws"),
        "/tmp/ws 中没有可继续的会话"
    );
    assert_eq!(wording::startup_cwd("gone"), "无法确定当前目录：gone");
}

#[test]
fn a_session_error_explains_the_code_and_passes_the_detail_through() {
    assert_eq!(
        wording::session_error("discussion_failed", "no debater answered this round"),
        "[会话错误：讨论失败] no debater answered this round"
    );
    assert_eq!(
        wording::session_error(
            "synthesis_failed",
            "the synthesizer's call produced no product"
        ),
        "[会话错误：合成失败] the synthesizer's call produced no product"
    );
    // 未知的 code 原样显示，而不是被丢掉。
    assert_eq!(wording::session_error_code("future_code"), "future_code");
}

#[test]
fn the_sessions_list_header_and_show_labels_read_in_chinese() {
    assert_eq!(
        wording::ls_columns(),
        ["标识", "工作区", "开始", "用量", "轮次", "消息", "结束"]
    );
    assert_eq!(wording::no_sessions(), "本桶中没有会话");
    assert_eq!(wording::open_session(), "未结束");
    assert_eq!(
        wording::round_group(2, Some(RoundMode::Targeted)),
        "── 第 2 轮（定向第二轮）──"
    );
    assert_eq!(wording::round_group(2, None), "── 第 2 轮 ──");
    assert_eq!(wording::session_group(), "── 会话 ──");
    assert_eq!(wording::round_label(3), "第 3 轮");
    assert_eq!(wording::tool_error("boom"), "错误：boom");
}

#[test]
fn the_replay_labels_read_in_chinese() {
    assert_eq!(wording::replay_system(), "[系统]");
    assert_eq!(wording::replay_user(None), "[用户]");
    assert_eq!(wording::replay_user(Some("kimi")), "[用户 kimi]");
    assert_eq!(wording::replay_assistant(), "[助手]");
    assert_eq!(wording::replay_tool("call-1"), "[工具 call-1]");
    assert_eq!(
        wording::replay_tool_call("read_file", "{\"path\":\"a\"}"),
        "→ 调用 read_file({\"path\":\"a\"})"
    );
}

#[test]
fn the_history_replay_progress_reads_in_chinese_and_degrades_by_width() {
    // 进度行的三档，以及每一档从哪个宽度接管：
    // 38 列是最小帧的提示行，而整句从下一档量出来的宽度起
    // 才对得起它占的列（`.scratch/tui-history-replay/spec.md` §4）。
    assert_eq!(wording::history_progress(12, 345), "恢复历史 12/345");
    assert_eq!(wording::history_progress_narrow(12, 345), "恢复中 12/345");
    assert_eq!(wording::history_progress_minimal(), "恢复中");
    assert_eq!(wording::history_progress_line(12, 345, 38), "恢复中 12/345");
    assert_eq!(wording::history_progress_line(12, 345, 37), "恢复中");
    assert_eq!(
        wording::history_progress_line(12, 345, 58),
        "恢复历史 12/345"
    );
    assert_eq!(wording::history_divider(), "── 以上为历史 ──");
}

#[test]
fn the_stats_labels_read_in_chinese() {
    assert_eq!(
        wording::stats_session(120, 2, 3, 4, &wording::stats_no_cost("mystery")),
        "会话：120 token，2 次调用，3 条消息，4 轮，无价格（没有 [pricing.mystery] 条目）"
    );
    assert_eq!(
        wording::stats_cost(0.0013, "deepseek-flash"),
        "，$0.001300（按 deepseek-flash 计价）"
    );
    assert_eq!(
        wording::stats_absence(1, 1, " (100%)"),
        "缺席：1 轮辩论中有 1 轮只有一方作答 (100%)"
    );
    assert_eq!(
        wording::stats_match_level("line-trim", 1),
        "  匹配层级 line-trim：1"
    );
    assert_eq!(
        wording::stats_round(
            1,
            RoundMode::Independent,
            2,
            &wording::stats_round_ended(StopReason::NoDivergence)
        ),
        "#1 独立首轮 2 次调用，结束于 无分歧"
    );
    // 记下来的枚举名也做了映射：没有任何内部名到达视图。
    assert_eq!(wording::stop_reason_name("Completed"), "完成");
    assert_eq!(wording::stop_reason_name("BudgetExhausted"), "预算用尽");
    assert_eq!(wording::stop_reason_name("future_reason"), "future_reason");
    assert_eq!(wording::decision_name("allow"), "允许");
    assert_eq!(wording::decision_name("deny"), "拒绝");
    assert_eq!(wording::decision_name("future"), "future");
}

#[test]
fn a_provider_finish_reason_reads_in_chinese() {
    use heng::provider::FinishReason;
    assert_eq!(wording::finish_reason(&FinishReason::Stop), "正常停止");
    assert_eq!(wording::finish_reason(&FinishReason::ToolCalls), "请求工具");
    assert_eq!(
        wording::finish_reason(&FinishReason::Length),
        "达到长度上限"
    );
    // 厂商自己那个不认识的标签原样透传。
    assert_eq!(
        wording::finish_reason(&FinishReason::Other("vendor_specific".to_owned())),
        "vendor_specific"
    );
}

#[test]
fn the_two_renderer_confirmations_read_in_chinese() {
    // TUI 自己问自己的那些问题：一次超大粘贴，以及一份
    // `Esc` 会丢掉的草稿。两者都默认走安全答案（spec §7），而两者
    // 都有与循环来的问题一样的三部分 —— 标题、正文、按钮。
    assert_eq!(
        format!(
            "{}｜{}｜{}",
            wording::paste_title(),
            wording::paste_body(120_000),
            wording::choices_text(&wording::PASTE_CHOICES)
        ),
        "粘贴确认｜粘贴 120000 字符｜[y] 粘贴 / [n] 取消"
    );
    assert_eq!(
        format!(
            "{}｜{}｜{}",
            wording::clear_draft_title(),
            wording::clear_draft_body(),
            wording::choices_text(&wording::CLEAR_CHOICES)
        ),
        "清空输入｜草稿有多行，Esc 会把它们全部丢掉｜[y] 清空 / [n] 保留"
    );
}

#[test]
fn the_panel_texts_read_like_the_prototype() {
    assert_eq!(wording::thousands(999), "999");
    assert_eq!(wording::thousands(12_345), "12,345");
    assert_eq!(wording::thousands(1_234_567), "1,234,567");

    assert_eq!(
        wording::token_pair(12_345, Some(100_000), NumberStyle::Cn),
        "1.2万 / 10万"
    );
    // 没有额度不是额度缺失：上限压根儿就不在那里。
    assert_eq!(wording::token_pair(12_345, None, NumberStyle::Cn), "1.2万");

    // 一次调用报出它的输入之前，窗口是未知的 —— 不是零。
    assert_eq!(
        wording::context_pair(None, 200_000, true, NumberStyle::Cn),
        wording::PANEL_UNKNOWN
    );
    assert_eq!(
        wording::context_pair(Some(12_345), 200_000, false, NumberStyle::Cn),
        "1.2万 / 20万"
    );
    // 占比是同一个字段的一部分，丢掉它就是这个字段降级的方式。
    assert_eq!(
        wording::context_pair(Some(12_345), 200_000, true, NumberStyle::Cn),
        "1.2万 / 20万（6%）"
    );
    // 门槛之下仍是千分位，所以这一对不受制式影响。
    assert_eq!(
        wording::cache_pair(9_000, 3_345, NumberStyle::Cn),
        "9,000 / 3,345"
    );

    // 回合字段数的是*回合*，`CONTEXT.md` 把它与轮次分得很开。
    assert_eq!(wording::PANEL_TURNS, "回合");
    assert_eq!(wording::PANEL_UNKNOWN, "—");
}

#[test]
fn a_count_is_written_in_wan_and_yi_or_in_si_prefixes() {
    // 门槛在先：两套制式下小于 10000 的数字都是千分位 —— `1.2万` 未必比 `12,345`
    // 好读，而 `9,999` 换成 `10.0k` 连精度都赔进去了。
    for value in [0, 999, 9_999] {
        assert_eq!(
            wording::compact(value, NumberStyle::Cn),
            wording::thousands(value),
            "cn：{value}"
        );
        assert_eq!(
            wording::compact(value, NumberStyle::Si),
            wording::thousands(value),
            "si：{value}"
        );
    }

    // 逐档一个代表值，外加 `cn` 的两个上界（`99_999_999` / `100_000_000`）。
    let cases = [
        (10_000, "1万", "10k"),
        (12_345, "1.2万", "12.3k"),
        (1_234_567, "123.5万", "1.2M"),
        (99_999_999, "10000万", "100M"),
        (100_000_000, "1亿", "100M"),
        (1_000_000_000, "10亿", "1G"),
    ];
    for (value, cn, si) in cases {
        assert_eq!(wording::compact(value, NumberStyle::Cn), cn, "cn：{value}");
        assert_eq!(wording::compact(value, NumberStyle::Si), si, "si：{value}");
    }

    // 恰好是整数时去掉 `.0`：规则上是一套写法，屏幕上不该多一个小数点。
    assert_eq!(wording::compact(10_000, NumberStyle::Cn), "1万");
    assert_eq!(wording::compact(100_000_000, NumberStyle::Cn), "1亿");
    assert_eq!(wording::compact(1_000_000_000, NumberStyle::Si), "1G");
}

#[test]
fn the_sidebar_names_its_pages() {
    // 三个页签都有内容了，而左栏那两句空态各有各的说法
    // （`.scratch/files-page/spec.md` §1）。
    assert_eq!(wording::TAB_USAGE, "调用量");
    assert_eq!(wording::TAB_TRACE, "轨迹");
    assert_eq!(wording::TAB_CONVERSATION, "对话");
    assert_eq!(wording::TAB_FILES, "文件");
    assert_eq!(wording::files_loading(), "正在读取工作区…");
    assert_eq!(wording::files_empty(), "工作区里没有文件");
    // 回合条的字形：一个普通单位、被聚焦的那个，以及这一列
    // 装不下的单位用的标记。
    assert_eq!(wording::RAIL_CELL, "┊");
    assert_eq!(wording::RAIL_FOCUS, "┃");
    assert_eq!(wording::RAIL_TRUNCATED, "⋮");
}

#[test]
fn a_stamp_is_seconds_in_the_local_zone_and_nine_columns_wide() {
    use chrono::TimeZone;

    // 时刻按**本地时区**显示、精确到秒，而且连它后面那个空格一起正好是
    // `layout::STAMP_COLUMNS` 那九列（`.scratch/trace-in-main/spec.md` §5）。
    let at = chrono::Utc
        .with_ymd_and_hms(2026, 10, 6, 4, 6, 7)
        .single()
        .expect("那是一个真实的时刻");
    let stamp = wording::stamp(at);
    assert_eq!(
        stamp,
        format!("{} ", at.with_timezone(&chrono::Local).format("%H:%M:%S")),
        "本地时区、秒级，外加一个分隔的空格"
    );
    assert_eq!(
        text_columns(&stamp),
        heng::render::layout::STAMP_COLUMNS as usize,
        "宽度就是那个常数"
    );
    assert!(stamp.ends_with(' '), "空格在最后：{stamp:?}");
}

/// 状态行的纯文本：段落按顺序连起来。
fn status_text(parts: &[wording::StatusPart]) -> String {
    parts.iter().map(|part| part.text.as_str()).collect()
}

#[test]
fn the_status_row_gives_up_the_model_with_the_effort_then_the_mode_and_never_itself() {
    // 短形永远带着自己的标签，所以不会凭空冒出一个
    // 没有解释的 `6%` —— 而一次调用报出它的输入之前，它会说出来，
    // 而不是显示一个零（spec §5）。
    assert_eq!(wording::context_share_value(Some(12_345), 200_000), "6%");
    assert_eq!(wording::context_share_value(None, 200_000), "—");

    let model = "claude-sonnet-4-5";
    let share = wording::context_share_value(Some(12_345), 200_000);
    // 五段，降级顺序是**先丢模型 + 档位、再丢模式，最后剩「上下文 + 状态词」**
    // —— 模型与档位是同一件事的两半（这一场会话由谁答、答多深），所以它们同生共死；
    // 「在跑」最后才丢（spec §11、§12）。
    // 模型与档位之间**不**放 `┆` —— 那会把同一格说成两件事，所以它们之间是段内的 `·`。
    assert_eq!(
        status_text(&wording::status_row(
            model,
            "high",
            Mode::Ask,
            &share,
            "就绪",
            90
        )),
        " 模型 claude-sonnet-4-5 · high ┆ 询问 ┆ 上下文 6% ┆ 就绪"
    );
    assert_eq!(
        status_text(&wording::status_row(
            model,
            "high",
            Mode::Ask,
            &share,
            "就绪",
            50
        )),
        " 询问 ┆ 上下文 6% ┆ 就绪",
        "模型与档位一起让位"
    );
    assert_eq!(
        status_text(&wording::status_row(
            model,
            "high",
            Mode::Ask,
            &share,
            "就绪",
            20
        )),
        " 上下文 6% ┆ 就绪",
        "接着让位的是模式，最后剩的读数与状态词都在"
    );
    assert_eq!(
        status_text(&wording::status_row(
            model,
            "high",
            Mode::Ask,
            &share,
            "就绪",
            4
        )),
        " 上下文 6% ┆ 就绪",
        "没有哪一档会拿掉这一行：连这一档都装不下的宽度归画家去截"
    );
    // 没有旋钮的模型显示 `固定`，它同样在那半格里（spec §8）。
    assert_eq!(
        status_text(&wording::status_row(
            "MiniMax-M3",
            wording::effort_fixed(),
            Mode::Ask,
            &share,
            "就绪",
            90
        )),
        " 模型 MiniMax-M3 · 固定 ┆ 询问 ┆ 上下文 6% ┆ 就绪"
    );
}

#[test]
fn the_status_row_marks_labels_values_and_separators() {
    use wording::StatusKind::{Label, Separator, Value};
    // 行内三档：标签退后、值靠前、分隔符只是线（§17）。
    // 整行前面那一格是留白，算一个值段。
    // 段内那个 `·` 走**值**那一档（它是内容的一部分，不是段与段之间的框架线），
    // 而段与段之间才是 `┆`（Separator）。
    let parts = wording::status_row("m", "high", Mode::Ask, "6%", "就绪", 80);
    let kinds: Vec<wording::StatusKind> = parts.iter().map(|part| part.kind).collect();
    assert_eq!(
        kinds,
        vec![
            Value, Label, Value, Value, Value, Separator, Value, Separator, Label, Value,
            Separator, Value
        ],
        "五段、三档：{parts:?}"
    );
}

#[test]
fn a_questionnaire_reads_in_chinese_and_pages() {
    // 页脚是页码指示加上那些键，所以读者总知道这是第几个问题、
    // 键盘都干什么（spec §19）。
    assert_eq!(wording::questionnaire_progress(1, 3), "2 / 3");
    assert_eq!(wording::questionnaire_multi_marker(), "（可多选）");
    assert_eq!(wording::questionnaire_answer_label(), "回答：");
    assert_eq!(wording::questionnaire_custom_label(), "自定义：");
    // 被削掉的题面自己说一声（`.scratch/questionnaire-keys/spec.md` §7 的补记）。
    assert_eq!(
        wording::questionnaire_question_clipped(7),
        "…（题面还有 7 行未显示）"
    );
}

#[test]
fn the_questionnaire_hint_drops_the_teaching_parts_first() {
    // 三档递减：先丢 Emacs 别名，再丢 `j`/`k` 那一句，最后只剩出口；再窄就整段不画
    // （`.scratch/questionnaire-keys/spec.md` §6）。回车那一段跟着它**实际会做什么**变：
    // 每题都有着落时是「回车 提交」，否则「回车 下一题」（§11）。
    const WIDE_NEXT: &str =
        "j/k 移动 · 空格选中 · 回车 下一题 · esc 退出询问 · ctrl-n/ctrl-p 同 j/k";
    const WIDE_SUBMIT: &str =
        "j/k 移动 · 空格选中 · 回车 提交 · esc 退出询问 · ctrl-n/ctrl-p 同 j/k";
    const MEDIUM_NEXT: &str = "j/k 移动 · 空格选中 · 回车 下一题 · esc 退出询问";
    const MEDIUM_SUBMIT: &str = "j/k 移动 · 空格选中 · 回车 提交 · esc 退出询问";
    assert_eq!(wording::questionnaire_hint(120, false), WIDE_NEXT);
    assert_eq!(wording::questionnaire_hint(120, true), WIDE_SUBMIT);
    assert_eq!(wording::questionnaire_hint(60, false), MEDIUM_NEXT);
    assert_eq!(wording::questionnaire_hint(60, true), MEDIUM_SUBMIT);
    assert_eq!(wording::questionnaire_hint(20, false), "esc 退出询问");
    assert_eq!(wording::questionnaire_hint(8, false), "");

    // **阈值就是文案自己的实测宽度**，不是一个另外抄下来的数字：判成某一档却放不下，会被
    // 页脚裁掉一截。边界拿「下一题」那个更宽的变体算，两个变体共用同一条阈值。
    let wide = text_columns(WIDE_NEXT);
    assert_eq!(wording::questionnaire_hint(wide, false), WIDE_NEXT);
    assert_eq!(wording::questionnaire_hint(wide - 1, false), MEDIUM_NEXT);
    let medium = text_columns(MEDIUM_NEXT);
    assert_eq!(wording::questionnaire_hint(medium, false), MEDIUM_NEXT);
    assert_eq!(
        wording::questionnaire_hint(medium - 1, false),
        "esc 退出询问"
    );
    let narrow = text_columns("esc 退出询问");
    assert_eq!(wording::questionnaire_hint(narrow, false), "esc 退出询问");
    assert_eq!(wording::questionnaire_hint(narrow - 1, false), "");

    // 进度段后面那句「这一题交回去的是什么」。
    assert_eq!(wording::questionnaire_skipped(), "已跳过");

    // 举手那两句是**替换**段尾那段提示，不在阶梯里。
    assert_eq!(
        wording::questionnaire_decline_raised(),
        "再按一次 esc 退出询问"
    );
    assert_eq!(
        wording::questionnaire_exit_raised(),
        "已取消 · 再按一次 ctrl-c 退出"
    );
}

#[test]
fn a_recommended_option_keeps_its_value_when_displayed() {
    // 这个标记是显示约定：选项读出来时丢掉那个后缀，而
    // 答案携带的值保留整段标签（spec §7）。
    assert_eq!(wording::recommended_badge(), "（推荐）");
    assert_eq!(wording::recommended_label("serde (推荐)"), ("serde", true));
    assert_eq!(wording::recommended_label("manual"), ("manual", false));
    // 只有在最末尾、分毫不差的那个后缀才算数。
    assert_eq!(wording::recommended_label("推荐阅读"), ("推荐阅读", false));
    assert_eq!(
        wording::recommended_label("serde（推荐）"),
        ("serde（推荐）", false),
        "全角括号那条是**徽标**，不是后缀"
    );
}

#[test]
fn the_old_english_suffix_still_earns_its_badge() {
    // 约定改成中文之后（`.scratch/tui-feedback/spec.md` §1），已经写进事件流的历史 label
    // 与旧会话里的 `(Recommended)` 照旧被剥掉 —— 否则那些选项会带着一串英文后缀显示出来。
    assert_eq!(
        wording::recommended_label("serde (Recommended)"),
        ("serde", true)
    );
    assert_eq!(
        wording::questionnaire_option(1, "serde (Recommended)", None),
        "1. serde（推荐）"
    );
    // 新旧两种在同一个生成器下产出同一行。
    assert_eq!(
        wording::questionnaire_option(1, "serde (推荐)", None),
        wording::questionnaire_option(1, "serde (Recommended)", None)
    );
    // 只是提到这个词的 label 一律不动。
    assert_eq!(
        wording::recommended_label("Recommended reading"),
        ("Recommended reading", false)
    );
    assert_eq!(
        wording::recommended_label("推荐 (推荐) 吗"),
        ("推荐 (推荐) 吗", false),
        "中间出现的不算数"
    );
}

#[test]
fn the_copy_receipt_leads_the_hint_row_and_never_evicts_the_way_out() {
    // 回执排在最前，而**出口永远保住**（`.scratch/tui-feedback/spec.md` §6；那条契约在
    // `exit-gesture` §2 与 `sidebar-toggle` §4 里）。
    assert_eq!(wording::copied(12, 2), "已复制 12 字 · 2 行");
    // 打开的那两句与它同一个形状、同一条寿命（`.scratch/clickable-links/spec.md` §4）。
    assert_eq!(
        wording::opened("https://example.com/x"),
        "已打开 https://example.com/x"
    );
    assert_eq!(
        wording::open_failed("No such file or directory"),
        "打开失败：No such file or directory"
    );
    let opened = wording::status_line_with(Some("已打开 .scratch/x.html"), false, 120, false);
    assert!(
        opened.starts_with("已打开 .scratch/x.html · "),
        "打开的回执也打头：{opened}"
    );
    let wide = wording::status_line_with(Some("已复制 12 字 · 2 行"), false, 120, false);
    assert!(
        wide.starts_with("已复制 12 字 · 2 行 · enter 发送"),
        "回执打头、提示跟在后面：{wide}"
    );
    assert!(wide.ends_with("ctrl-c/ctrl-d 退出"), "出口在末尾：{wide}");

    // 45 列：放得下回执与出口，放不下任何一条提示 —— 提示让位，出口不让。
    let middle = wording::status_line_with(Some("已复制 12 字 · 2 行"), false, 45, false);
    assert_eq!(
        middle, "已复制 12 字 · 2 行 · ctrl-c/ctrl-d 退出",
        "{middle}"
    );

    // 20 列：连回执都放不下，于是退回那条不带回执的提示行 —— 出口照旧在场。
    let narrow = wording::status_line_with(Some("已复制 12 字 · 2 行"), false, 20, false);
    assert_eq!(narrow, wording::status_line(false, 20, false), "{narrow}");
    assert!(narrow.contains("ctrl-c/ctrl-d 退出"), "{narrow}");
}

#[test]
fn the_plain_console_asks_a_questionnaire_in_chinese() {
    assert_eq!(
        wording::questionnaire_plain_options_prompt(false),
        "输入编号选择，或直接输入文本；回车跳过 > "
    );
    // 多选提示说了第二行会跟上来，于是那份补充
    // （已选与自定义合在一起）是可发现的，而不是秘密。
    assert_eq!(
        wording::questionnaire_plain_options_prompt(true),
        "输入编号（逗号分隔）选择，或输入文本；下一行补充；回车跳过 > "
    );
    assert_eq!(
        wording::questionnaire_plain_supplement_prompt(),
        "补充文本（可留空）> "
    );
    assert_eq!(
        wording::questionnaire_plain_answer_prompt(),
        "输入回答；回车跳过 > "
    );
}

#[test]
fn the_sandbox_line_names_the_mode_and_why_it_was_unavailable() {
    assert_eq!(wording::sandbox("bwrap", None), "[沙箱：bwrap]");
    assert_eq!(wording::sandbox("off", None), "[沙箱：off]");
    assert_eq!(
        wording::sandbox("bwrap", Some("PATH 上没有 `bwrap`")),
        "[沙箱：bwrap · 不可用：PATH 上没有 `bwrap`]"
    );
}

// --- 终端标题：形状、状态词与 40 列封顶（`.scratch/terminal-title/spec.md` §1–§3） --

#[test]
fn a_title_is_path_status_and_goal_joined_by_middle_dots() {
    let cwd = std::path::Path::new("/x/fortystory/heng");
    assert_eq!(
        wording::terminal_title(cwd, None, wording::TitleState::Running, None),
        "fortystory/heng · 运行中"
    );
    // 目标名取 3 个汉字：`fortystory/heng · 运行中 · 修文档` 是 37 列，留在 40 列里。
    // spec §1 举的例子用的是 `修文档索引`，那条整串是 41 列 —— 按 §3 的封顶它会被丢掉，
    // 所以这里不照抄那个例子（见这一票的 Comments）。
    assert_eq!(
        wording::terminal_title(cwd, None, wording::TitleState::Running, Some("修文档")),
        "fortystory/heng · 运行中 · 修文档"
    );
}

#[test]
fn an_idle_title_omits_the_status_segment() {
    assert_eq!(
        wording::terminal_title(
            std::path::Path::new("/x/fortystory/heng"),
            None,
            wording::TitleState::Idle,
            None
        ),
        "fortystory/heng"
    );
}

#[test]
fn a_title_without_a_goal_omits_the_goal_segment() {
    assert_eq!(
        wording::terminal_title(
            std::path::Path::new("/x/fortystory/heng"),
            None,
            wording::TitleState::Waiting,
            None
        ),
        "fortystory/heng · 等你"
    );
}

#[test]
fn the_home_directory_is_abbreviated_to_a_tilde() {
    let home = std::path::Path::new("/home/forty");
    assert_eq!(
        wording::title_path(std::path::Path::new("/home/forty/code/heng"), Some(home)),
        "~/code/heng"
    );
    assert_eq!(wording::title_path(home, Some(home)), "~");
    // 前缀按路径分量比：`forty2` 不是 `forty` 下面的东西，所以走「父/基」那条。
    assert_eq!(
        wording::title_path(std::path::Path::new("/home/forty2/code"), Some(home)),
        "forty2/code"
    );
}

#[test]
fn the_root_directory_and_a_home_less_run_have_their_own_paths() {
    assert_eq!(wording::title_path(std::path::Path::new("/"), None), "/");
    assert_eq!(
        wording::title_path(std::path::Path::new("/home/forty/code/heng"), None),
        "code/heng"
    );
}

#[test]
fn a_title_longer_than_forty_columns_drops_the_goal_first() {
    // 19（路径）+ 3 + 6（状态）+ 3 + 40（目标）= 71 列；丢掉目标就只剩 28 列。
    let goal = "修".repeat(20);
    let title = wording::terminal_title(
        std::path::Path::new("/x/fortystory/heng"),
        None,
        wording::TitleState::Running,
        Some(&goal),
    );
    assert_eq!(title, "fortystory/heng · 运行中");
}

#[test]
fn a_still_too_long_title_drops_the_status_next() {
    // 路径本身 39 列：加上「 · 运行中」是 48 列，只剩路径才放得下。
    let home = std::path::Path::new("/home/forty");
    let long = "a".repeat(28);
    let cwd = std::path::PathBuf::from(format!("/home/forty/{long}/heng"));
    let title = wording::terminal_title(&cwd, Some(home), wording::TitleState::Running, None);
    assert_eq!(title, format!("~/{long}/heng"));
    assert!(!title.contains("运行中"), "状态词被丢掉了：{title}");
}

#[test]
fn a_still_too_long_path_falls_back_to_its_base_name() {
    // 路径 41 列（`~/` + 34 个 a + `/heng`）；退化的那一步丢的是上面的目录，不是 `~`。
    let home = std::path::Path::new("/home/forty");
    let long = "a".repeat(34);
    let cwd = std::path::PathBuf::from(format!("/home/forty/{long}/heng"));
    let title = wording::terminal_title(&cwd, Some(home), wording::TitleState::Running, None);
    assert_eq!(title, "heng");
}

#[test]
fn a_path_that_cannot_fit_is_truncated_to_forty_columns() {
    // 基名本身 50 列（25 个汉字）；40 列就是它的前 20 个汉字 —— 宽字符不从中间劈开。
    let base = "超长目录名".repeat(5);
    let cwd = std::path::PathBuf::from(format!("/x/{base}"));
    let title = wording::terminal_title(&cwd, None, wording::TitleState::Idle, None);
    assert_eq!(title, "超长目录名".repeat(4));
    assert_eq!(
        heng::render::width::text_columns(&title),
        wording::TITLE_COLUMNS
    );
}

#[test]
fn replay_outranks_a_pending_question_which_outranks_a_running_turn() {
    use wording::TitleState::{Idle, Replaying, Running, Waiting};
    assert_eq!(wording::title_state(true, true, true), Replaying);
    assert_eq!(wording::title_state(false, true, true), Waiting);
    assert_eq!(wording::title_state(false, false, true), Running);
    assert_eq!(wording::title_state(false, false, false), Idle);
}

#[test]
fn an_idle_session_has_no_status_word() {
    use wording::TitleState::{Idle, Replaying, Running, Waiting};
    assert_eq!(wording::title_word(Idle), None);
    assert_eq!(wording::title_word(Running), Some("运行中"));
    assert_eq!(wording::title_word(Waiting), Some("等你"));
    assert_eq!(wording::title_word(Replaying), Some("重放中"));
}

#[test]
fn the_forty_column_cap_counts_display_columns_not_bytes() {
    // `x/heng · 运行中` 是 15 列；再加 ` · ` 与目标，22 列的目标正好落在 40 列上，
    // 24 列的就超了。按字节算的话 11 个汉字（33 字节）早就超了 —— 那会让这条测试红。
    let cwd = std::path::Path::new("/x/heng");
    let just_fits = "目".repeat(11);
    let title = wording::terminal_title(cwd, None, wording::TitleState::Running, Some(&just_fits));
    assert!(title.ends_with(&just_fits), "22 列的目标留得住：{title}");

    let too_wide = "目".repeat(12);
    let title = wording::terminal_title(cwd, None, wording::TitleState::Running, Some(&too_wide));
    assert_eq!(title, "x/heng · 运行中", "24 列的目标挤不下：{title}");
}

#[test]
fn the_command_and_reference_colours_are_two_distinct_named_colours() {
    // 草稿里两个前缀的颜色是主题层给的两个常量，不散写在绘制里
    // （`.scratch/input-tokens/spec.md` §4）。这里钉的是「它们是两个、而且都真的在着色」——
    // 一个被改成 `Reset`、或者两个被合成同一个值，都该报红。
    let command = heng::render::TOKEN_COMMAND;
    let reference = heng::render::TOKEN_REFERENCE;
    assert_ne!(command, reference, "命令蓝与引用紫是两种颜色");
    assert_ne!(
        command,
        ratatui::style::Color::Reset,
        "上色不该是一个空操作"
    );
    assert_ne!(
        reference,
        ratatui::style::Color::Reset,
        "上色不该是一个空操作"
    );
}
