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
fn a_discussion_reports_why_it_stopped_and_who_was_absent() {
    assert_eq!(
        wording::discussion_ended(StopReason::NoDivergence, 1, &[]),
        "讨论结束：无分歧（跑了 1 轮）"
    );
    // The absent side is named: a round with one answer is not a round of agreement.
    assert_eq!(
        wording::discussion_ended(
            StopReason::NoDivergence,
            2,
            &[SpeakerId::Debater("deepseek".into())]
        ),
        "讨论结束：无分歧（跑了 2 轮）；缺席：[deepseek]"
    );
    assert_eq!(wording::discussion_pair("保守", "激进"), "保守 × 激进");
    // A debater is named by its persona, with the model beside it when they differ.
    assert_eq!(
        wording::debater_label("保守", "deepseek-v4-pro"),
        "保守（deepseek-v4-pro）"
    );
    assert_eq!(
        wording::debater_label("kimi-k3", "kimi-k3"),
        "kimi-k3",
        "the shorthand says it once"
    );
    assert_eq!(
        wording::needs_two_debaters("保守"),
        "--debaters 需要两个名字（逗号分隔，例如 `--debaters 保守,激进`），得到 `保守`"
    );
    assert_eq!(
        wording::unknown_debater("激进", &["保守", "审查"]),
        "池子里没有叫 `激进` 的讨论者；可用：`保守`、`审查`"
    );
    assert_eq!(
        wording::discussion_replay("20260922T101500Z-ab12"),
        "会话 20260922T101500Z-ab12；复盘：fs-agent sessions show 20260922T101500Z-ab12"
    );
    // The two ways `discuss` refuses to start say what to do instead.
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
    // `/discuss` on a live session: which models, and which question.
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

    // One vendor — or one model twice — is allowed, and said out loud rather than
    // passing for the heterogeneous case the design assumes.
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
        wording::permission_prompt_with_context("write_file", "file_path=a.txt", "mode ask"),
        "权限询问：write_file（file_path=a.txt）？原因：mode ask [y] 允许 / [a] 总是允许 / [n] 拒绝 "
    );
    assert_eq!(
        wording::plan_conflict_prompt("/tmp/PLAN.md"),
        "/tmp/PLAN.md 已存在：[o] 覆盖 / [a] 追加 / [k] 保留 "
    );
}

#[test]
fn a_question_has_a_title_a_body_and_a_row_of_choices() {
    // The overlay's three parts, each named on its own: a title that says what is
    // being asked, a body that says what it is about, and the keys (spec §9).
    assert_eq!(wording::permission_title("bash"), "权限询问：bash");
    assert_eq!(
        wording::permission_call("bash", "command=rm -rf /"),
        "bash（command=rm -rf /）"
    );
    assert_eq!(wording::permission_call("read_file", ""), "read_file");
    assert_eq!(wording::plan_conflict_title(), "计划文件冲突");
    assert_eq!(
        wording::plan_conflict_body("/tmp/PLAN.md"),
        "/tmp/PLAN.md 已存在"
    );
    assert_eq!(wording::paste_title(), "粘贴确认");
    assert_eq!(wording::paste_body(120_000), "粘贴 120000 字符");
    assert_eq!(wording::clear_draft_title(), "清空输入");
    assert_eq!(
        wording::clear_draft_body(),
        "草稿有多行，Esc 会把它们全部丢掉"
    );
    // One table per question, and one join for any front end that has only a line:
    // the TUI paints the entries, the plain console prints this text.
    assert_eq!(
        wording::choices_text(&wording::PERMISSION_CHOICES),
        "[y] 允许 / [a] 总是允许 / [n] 拒绝"
    );
    assert_eq!(
        wording::choices_text(&wording::PLAN_CHOICES),
        "[o] 覆盖 / [a] 追加 / [k] 保留"
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
fn a_permission_summary_says_what_the_action_is() {
    // The row a reader gets when the arguments are a wall of text: one plain sentence
    // about the *action*, never a reading of the arguments.
    assert_eq!(
        wording::permission_summary("bash"),
        "在你的工作区里执行一条 shell 命令（可以读写文件、访问网络）"
    );
    assert_eq!(
        wording::permission_summary("write_file"),
        "写入一个文件（新建，或者整体覆盖已有的）"
    );
    assert_eq!(
        wording::permission_summary("read_file"),
        "读取一个文件的内容"
    );
    assert_eq!(
        wording::permission_summary("edit_file"),
        "修改一个文件里的一段内容"
    );
    assert_eq!(
        wording::permission_summary("custom__git__status"),
        "运行你在配置里声明的自定义工具 git/status"
    );
    // A tool this crate does not know still gets a sentence: the name is all there is.
    assert_eq!(wording::permission_summary("mystery"), "调用 mystery 工具");
    // Every built-in is *named* in the table; falling through to the generic tail
    // would be a missing sentence rather than an answer.
    for name in [
        "bash",
        "read_file",
        "write_file",
        "edit_file",
        "task",
        "skill",
        "repo_map",
    ] {
        let summary = wording::permission_summary(name);
        assert!(
            !summary.starts_with("调用 "),
            "{name} has no sentence of its own: {summary}"
        );
    }
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
fn the_viewer_status_line_hints_only_at_what_a_viewer_can_do() {
    // No line is being read (a one-shot `discuss`, or a turn in flight), so neither
    // `enter 发送` nor the interactive loop's plan gesture is on offer.
    let wide = wording::viewer_status_line(false, 200);
    assert_eq!(
        wide, "就绪 · esc 取消 · PgUp/PgDn 滚动 · ctrl-c 退出",
        "the whole viewer line"
    );
    assert!(wide.ends_with("ctrl-c 退出"), "{wide}");

    // The same ladder: the way out survives, the state word goes first.
    assert_eq!(
        wording::viewer_status_line(false, 26),
        "esc 取消 · ctrl-c 退出"
    );
    assert_eq!(
        wording::viewer_status_line(true, 26),
        "esc 取消 · ctrl-c 退出",
        "whatever the state word would have said"
    );

    // Busy reads as busy, and a terminal too narrow for anything still exits.
    assert!(wording::viewer_status_line(true, 200).starts_with("工作中 · "));
    assert_eq!(wording::viewer_status_line(true, 3), "ctrl-c 退出");
}

#[test]
fn the_hint_ladder_is_the_one_the_prototype_measured() {
    // The widths the spec recorded against the approved snapshots (§10, §13), so a
    // change to the priority order shows up here rather than on a real terminal.
    // `w=40` is the minimum: three items, no state word.
    assert_eq!(
        wording::status_line(false, 40),
        "enter 发送 · ctrl-j 换行 · ctrl-c 退出"
    );
    // Then the state word joins in front, and each step buys one more hint.
    assert_eq!(
        wording::status_line(false, 60),
        "就绪 · enter 发送 · ctrl-j 换行 · esc 取消 · ctrl-c 退出"
    );
    assert_eq!(
        wording::status_line(false, 80),
        "就绪 · enter 发送 · ctrl-j 换行 · esc 取消 · shift+tab 计划 · ctrl-c 退出"
    );
    // Busy swaps the word and nothing else: at 60 the hint that loses is still
    // `shift+tab 计划`, and `ctrl-c 退出` is still there.
    assert_eq!(
        wording::status_line(true, 60),
        "工作中 · enter 发送 · ctrl-j 换行 · esc 取消 · ctrl-c 退出"
    );
    let full = "就绪 · enter 发送 · ctrl-j 换行 · esc 取消 · shift+tab 计划 · PgUp/PgDn 滚动 · ctrl-c 退出";
    assert_eq!(wording::status_line(false, 120), full);
    // At the maximum the line is stable: there is nothing left to buy.
    assert_eq!(wording::status_line(false, 174), full);
    // Busy swaps the word, not the ladder.
    assert_eq!(
        wording::status_line(true, 120).replace("工作中", "就绪"),
        full
    );
}

#[test]
fn no_hint_ever_names_shift_enter() {
    // Without the keyboard-enhancement protocol `Shift+Enter` is indistinguishable
    // from `Enter`, which submits — so a hint that named it would be a lie, and the
    // lie is invisible at any single width (spec §10, user story 54). Scan them all.
    for busy in [false, true] {
        for width in 1..=200 {
            let line = wording::status_line(busy, width);
            assert!(
                !line.to_lowercase().contains("shift+enter"),
                "{width}: {line}"
            );
        }
    }
}

#[test]
fn the_scroll_indicator_and_the_minimum_explain_themselves() {
    // The indicator names what arrived and how to get there; without a count it is
    // only the way back (spec §4).
    assert_eq!(wording::new_content(12), "↓ 12 行新内容 · 点此到底");
    assert_eq!(wording::new_content(1), "↓ 1 行新内容 · 点此到底");
    assert_eq!(wording::back_to_bottom(), "点此到底");
    // A terminal below the minimum is told why it is empty, with the numbers it has.
    assert_eq!(wording::too_small(40, 10), "终端太小：至少 40×10");
}

#[test]
fn every_panel_label_is_the_chinese_the_prototype_shows() {
    assert_eq!(wording::PANEL_MODEL, "模型");
    assert_eq!(wording::PANEL_CONTEXT, "上下文");
    // `CONTEXT.md` has no Chinese word for a token, so it stays as it is elsewhere.
    assert_eq!(wording::PANEL_TOKENS, "token");
    assert_eq!(wording::PANEL_TURNS, "回合");
    assert_eq!(wording::PANEL_INPUT, "输入");
    assert_eq!(wording::PANEL_OUTPUT, "输出");
    assert_eq!(wording::PANEL_CACHE, "缓存");
}

#[test]
fn the_header_identity_is_the_crate_and_the_version_it_was_built_from() {
    // `scripts/tui-startup-check.py` anchors on this exact string to tell the new
    // four-pane layout apart from anything older, and its expectation comes from the
    // binary's own `--version` (`src/cli.rs` prints `fs-agent {version}`). The two
    // spellings are written in two places, so this pins them together: change either
    // and the check goes red rather than silently matching nothing.
    assert_eq!(
        wording::identity(),
        format!("fs-agent {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn the_mark_is_five_rows_of_one_width() {
    // The tall header shows the mark whole or not at all — `layout` decides that
    // from `LOGO_WIDTH` before anything is drawn. A row of a different width would
    // slip past that gate and paint over the border, so the contract is pinned here,
    // where the characters live, rather than trusted at the painter.
    let rows = wording::logo_lines();
    assert_eq!(rows.len(), 5, "the mark is five rows: {rows:?}");
    for row in rows {
        assert_eq!(
            row.chars().count(),
            fs_agent::render::layout::LOGO_WIDTH as usize,
            "every row is the width the layout reserved: {row:?}"
        );
    }
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
        "未知命令 /nope（可用：/undo、/plan、/endplan、/discuss、/quit，或直接输入 /<技能名>）"
    );
    assert_eq!(
        wording::unknown_command("/nope", &["ask-matt", "release"]),
        "未知命令 /nope（可用：/undo、/plan、/endplan、/discuss、/quit；技能：/ask-matt、/release）"
    );
    assert_eq!(wording::skill_loaded("ask-matt"), "已加载技能 ask-matt");
    assert_eq!(
        wording::skill_started("ask-matt"),
        "已加载技能 ask-matt，按技能正文开始"
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

    // The discussion has a front end of its own now, and both helps say so.
    assert!(main.contains("fs-agent discuss"), "{main}");
    let discuss = wording::help_discuss();
    assert!(discuss.contains("fs-agent discuss"), "{discuss}");
    assert!(discuss.contains("[discussion] debaters"), "{discuss}");
    assert!(
        discuss.contains("同厂商"),
        "one vendor is documented as allowed"
    );
    assert!(discuss.contains("CONCLUSION:"), "{discuss}");
    assert!(discuss.contains("3 次调用"), "{discuss}");
    assert!(discuss.contains("sessions show"), "{discuss}");
    assert!(
        !discuss.contains(" 的") && !discuss.contains("永 远"),
        "no space was left behind by a line continuation: {discuss}"
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

#[test]
fn the_two_renderer_confirmations_read_in_chinese() {
    // The questions the TUI asks itself: an oversized paste, and a multi-line draft
    // `Esc` would throw away. Both default to the safe answer (spec §7), and both get
    // the same three parts as a question from the loop — title, body, buttons.
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
        wording::token_pair(12_345, Some(100_000)),
        "12,345 / 100,000"
    );
    // No allowance is not a missing allowance: the cap is simply not there.
    assert_eq!(wording::token_pair(12_345, None), "12,345");

    // Before a call has reported its input, the window is unknown — not zero.
    assert_eq!(
        wording::context_pair(None, 200_000, true),
        wording::PANEL_UNKNOWN
    );
    assert_eq!(
        wording::context_pair(Some(12_345), 200_000, false),
        "12,345 / 200,000"
    );
    // The share is part of the same field, and dropping it is how that field degrades.
    assert_eq!(
        wording::context_pair(Some(12_345), 200_000, true),
        "12,345 / 200,000（6%）"
    );
    assert_eq!(wording::cache_pair(9_000, 3_345), "9,000 / 3,345");

    // The turns field is a *turn* count, which `CONTEXT.md` keeps apart from 轮次.
    assert_eq!(wording::PANEL_TURNS, "回合");
    assert_eq!(wording::PANEL_UNKNOWN, "—");
}
