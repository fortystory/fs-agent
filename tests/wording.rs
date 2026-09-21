//! The wording layer at its own seam: given a domain value, which Chinese phrase
//! comes out.
//!
//! This is the one layer that asserts **exact text**. "One source for every
//! human-facing phrase" is the whole claim of the feature, and only an assertion
//! here can tell "one producer" apart from "four copies of the same words" (spec
//! §Testing Decisions). The painters assert semantics instead.

use fs_agent::events::{
    ContextSource, Decision, DecisionSource, HistoryReason, RoundMode, SpeakerId, StopReason, Usage,
};
use fs_agent::permissions::Mode;
use fs_agent::render::wording;

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
            "the mapping for {reason:?} is explicit"
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
fn a_permission_ask_and_verdict_read_in_chinese() {
    // The question shows the tool and the concrete call, never the ids.
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
        wording::permission_prompt("bash", "command=rm -rf /"),
        "权限询问：bash（command=rm -rf /）？[y] 允许 / [a] 总是允许 / [n] 拒绝 "
    );
    assert_eq!(
        wording::permission_prompt_with_context("write_file", "file_path=a.txt", "mode ask"),
        "权限询问：write_file（file_path=a.txt）？原因：mode ask [y] 允许 / [a] 总是允许 / [n] 拒绝 "
    );
    assert_eq!(
        wording::plan_conflict_prompt("/tmp/PLAN.md"),
        "/tmp/PLAN.md 已存在：[o] 覆盖 / [a] 追加 / [k] 保留 "
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
fn the_status_line_keeps_the_way_out_and_gives_up_the_state_word_when_narrow() {
    // Wide enough: the state word, then the key hints, with the way out last.
    let wide = wording::status_line(false, 200);
    assert!(wide.starts_with("就绪 · "), "{wide}");
    for hint in [
        "enter 发送",
        "ctrl-j 换行",
        "esc 取消",
        "shift+tab 计划",
        "PgUp/PgDn 滚动",
        "ctrl-c 退出",
    ] {
        assert!(wide.contains(hint), "{wide}");
    }
    assert!(wide.ends_with("ctrl-c 退出"), "{wide}");

    // 28 columns fit one hint once the way out is reserved.
    assert_eq!(wording::status_line(false, 28), "enter 发送 · ctrl-c 退出");
    // 44 fit two and still not the state word: it is what goes, so the newline key
    // stays visible on a narrow terminal.
    assert_eq!(
        wording::status_line(false, 44),
        "enter 发送 · ctrl-j 换行 · ctrl-c 退出"
    );
    // 45 is where `就绪 · ` fits in front of that run.
    assert_eq!(
        wording::status_line(false, 45),
        "就绪 · enter 发送 · ctrl-j 换行 · ctrl-c 退出"
    );
    // Narrower than any hint: the way out is all that is left.
    assert_eq!(wording::status_line(true, 8), "ctrl-c 退出");
    assert_eq!(wording::status_line(false, 3), "ctrl-c 退出");
}

#[test]
fn a_banner_labels_the_model_mode_and_session_in_chinese() {
    assert_eq!(
        wording::banner("s-1", "kimi-k3", Mode::Ask, "/tmp/ws", false),
        "fs-agent：会话 s-1 · 模型 kimi-k3 · 模式 询问 · /tmp/ws"
    );
    assert_eq!(
        wording::banner("s-1", "kimi-k3", Mode::Plan, "/tmp/ws", true),
        "fs-agent：会话 s-1 · 模型 kimi-k3 · 模式 计划 · /tmp/ws（已继续）"
    );
}

#[test]
fn interactive_feedback_reads_in_chinese() {
    assert_eq!(wording::nothing_to_undo(), "没有可撤销的修改");
    assert_eq!(
        wording::unknown_command("/nope", &[]),
        "未知命令 /nope（可用：/undo、/plan、/endplan、/quit，或直接输入 /<技能名>）"
    );
    assert_eq!(
        wording::unknown_command("/nope", &["ask-matt", "release"]),
        "未知命令 /nope（可用：/undo、/plan、/endplan、/quit；技能：/ask-matt、/release）"
    );
    assert_eq!(wording::skill_loaded("ask-matt"), "已加载技能 ask-matt");
    assert_eq!(
        wording::skill_loaded_waiting("ask-matt"),
        "已加载技能 ask-matt；请输入你的任务。"
    );
    assert_eq!(wording::plan_entered(), "已进入计划模式");
    assert_eq!(wording::plan_exited(), "已退出计划模式");
}

#[test]
fn the_short_help_texts_are_exact_chinese() {
    assert_eq!(
        wording::help_probe(),
        "fs-agent probe [--config PATH] [--model ID]...\n\n  \
         对每个模型在同一会话里发送两次真实回合，并打印每次的 input/output/cached/miss。\
         不带 --model 时探测每一个 provider 有密钥的模型。"
    );
    assert_eq!(
        wording::help_prune(),
        "fs-agent prune [--keep N] [--cwd PATH] [--dry-run]\n\n  \
         删除一个工作区（当前目录，或 --cwd）的会话目录。保留最新的 N 个会话（默认 1：即 \
         --continue 会继续的那个）。一个会话就是一个目录，所以删除是整会话的；--dry-run \
         只列出将删除的内容。除此之外没有任何东西会删除会话。"
    );
}

#[test]
fn the_long_help_texts_are_chinese_and_keep_their_structure() {
    let main = wording::help_main();
    assert!(main.contains("usage: fs-agent"), "{main}");
    assert!(main.contains("配置位于"), "{main}");
    assert!(main.contains("前缀缓存"), "{main}");

    let interactive = wording::help_interactive();
    assert!(interactive.contains("--plain"), "{interactive}");
    assert!(interactive.contains("硬计划模式"), "{interactive}");

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
    // An unknown code is shown as itself rather than dropped.
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
    // Recorded enum names are mapped too: no internal name reaches the view.
    assert_eq!(wording::stop_reason_name("Completed"), "完成");
    assert_eq!(wording::stop_reason_name("BudgetExhausted"), "预算用尽");
    assert_eq!(wording::stop_reason_name("future_reason"), "future_reason");
    assert_eq!(wording::decision_name("allow"), "允许");
    assert_eq!(wording::decision_name("deny"), "拒绝");
    assert_eq!(wording::decision_name("future"), "future");
}

#[test]
fn a_provider_finish_reason_reads_in_chinese() {
    use fs_agent::provider::FinishReason;
    assert_eq!(wording::finish_reason(&FinishReason::Stop), "正常停止");
    assert_eq!(wording::finish_reason(&FinishReason::ToolCalls), "请求工具");
    assert_eq!(
        wording::finish_reason(&FinishReason::Length),
        "达到长度上限"
    );
    // A vendor's own unrecognized label passes through.
    assert_eq!(
        wording::finish_reason(&FinishReason::Other("vendor_specific".to_owned())),
        "vendor_specific"
    );
}
