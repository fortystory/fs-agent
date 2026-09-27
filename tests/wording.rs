//! 措辞层在它自己的接缝上：给定一个领域值，
//! 出来的是哪一句中文。
//!
//! 这是唯一一个断言**精确文本**的层。「每个人类可见短语
//! 只有一个来源」正是这个特性的全部主张，而只有这里的
//! 断言能把「一个产出者」与「同一群字写了四份」分开
//! （spec §Testing Decisions）。画家那边断言的是语义。

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
    assert_eq!(
        wording::discussion_replay("20260922T101500Z-ab12"),
        "会话 20260922T101500Z-ab12；复盘：fs-agent sessions show 20260922T101500Z-ab12"
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
        wording::permission_prompt_with_context("write_file", "file_path=a.txt", "mode ask"),
        "权限询问：write_file（file_path=a.txt）？原因：mode ask [y] 允许 / [a] 总是允许 / [n] 拒绝 "
    );
    // 三档模式，用的是状态行与 banner 那套词；第四档
    // （`计划`）随模式本身一起退场了（`.scratch/todo-and-modes`）。
    assert_eq!(wording::mode_label(Mode::Readonly), "只读");
    assert_eq!(wording::mode_label(Mode::Ask), "询问");
    assert_eq!(wording::mode_label(Mode::Auto), "自动");
    assert_eq!(wording::mode_field(Mode::Auto), "模式 自动");
    assert!(
        wording::unknown_mode("plan").contains("plan"),
        "这条拒绝引用了当时写下的东西"
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
fn the_status_line_keeps_the_way_out_and_gives_up_the_state_word_when_narrow() {
    // 够宽：先状态词，再键位提示，出路放最后。
    let wide = wording::status_line(false, 200);
    assert!(wide.starts_with("就绪 · "), "{wide}");
    for hint in [
        "enter 发送",
        "ctrl-j 换行",
        "esc 取消",
        "shift+tab 模式",
        "PgUp/PgDn 滚动",
        wording::EXIT_HINT_IDLE,
    ] {
        assert!(wide.contains(hint), "{wide}");
    }
    assert!(wide.ends_with(wording::EXIT_HINT_IDLE), "{wide}");

    // 28 列装得下状态词与出路；31 列时发送提示与出路装得下，
    // 而让位的是状态词 —— 出路比它过去宽了七列，
    // 正是这一点挪动了这一档。
    assert_eq!(wording::status_line(false, 28), "就绪 · ctrl-c/ctrl-d 退出");
    assert_eq!(
        wording::status_line(false, 31),
        "enter 发送 · ctrl-c/ctrl-d 退出"
    );
    // 45 列装得下前两条提示与出路、装不下状态词，所以窄终端上
    // 换行键仍然可见。
    assert_eq!(
        wording::status_line(false, 45),
        "enter 发送 · ctrl-j 换行 · ctrl-c/ctrl-d 退出"
    );
    // 80 列是状态词能装进那五条提示前面的地方；那条阶梯的
    // 渲染侧断言在 `tests/render_layout.rs` 里。
    assert_eq!(
        wording::status_line(false, 80),
        "就绪 · enter 发送 · ctrl-j 换行 · esc 取消 · shift+tab 模式 · ctrl-c/ctrl-d 退出"
    );
    // 比任何提示都窄：剩下的只有出路。
    assert_eq!(wording::status_line(true, 8), wording::EXIT_HINT_BUSY);
    assert_eq!(wording::status_line(false, 3), wording::EXIT_HINT_IDLE);
    // 只有空闲行会宣传 `ctrl-d`：一次运行在飞的时候它什么都不做，
    // 所以点它的名正是提示行绝不能做的那件事。
    assert!(
        !wording::status_line(true, 200).contains("ctrl-d"),
        "忙碌行不宣传一个什么都不做的键"
    );
    assert!(
        wording::status_line(false, 200).contains("ctrl-d"),
        "空闲行会宣传"
    );
}

#[test]
fn the_viewer_status_line_hints_only_at_what_a_viewer_can_do() {
    // 没有一行在被读（一次性的 `discuss`，或者一个在飞的回合），
    // 所以 `enter 发送` 与交互循环的模式手势都不在候选里。
    let wide = wording::viewer_status_line(false, 200);
    assert_eq!(
        wide, "就绪 · esc 取消 · PgUp/PgDn 滚动 · ctrl-c/ctrl-d 退出",
        "整条查看器行"
    );
    assert!(wide.ends_with(wording::EXIT_HINT_IDLE), "{wide}");

    // 同一条阶梯：出路活下来，状态词先让位。
    assert_eq!(
        wording::viewer_status_line(false, 28),
        "就绪 · ctrl-c/ctrl-d 退出"
    );
    assert_eq!(
        wording::viewer_status_line(false, 31),
        "esc 取消 · ctrl-c/ctrl-d 退出"
    );

    // 忙就读作忙 —— 而一次运行在飞的时候 `ctrl-d` 被忽略，所以
    // 查看器行回到朴素的 `ctrl-c 退出`。
    assert!(wording::viewer_status_line(true, 200).starts_with("工作中 · "));
    assert!(!wording::viewer_status_line(true, 200).contains("ctrl-d"));
    assert_eq!(
        wording::viewer_status_line(true, 3),
        wording::EXIT_HINT_BUSY
    );
}

#[test]
fn the_hint_ladder_is_the_one_the_prototype_measured() {
    // 原型用「只有一项的出路」量出来的那些宽度（§10，票 06 §4），
    // 于是优先级顺序一改就会在这里显出来，而不是在终端上。
    // `w=40` 是最小值：状态词、一条提示，加上出路。
    assert_eq!(
        wording::status_line(false, 40),
        "就绪 · enter 发送 · ctrl-c/ctrl-d 退出"
    );
    // 更宽的出路从 45 列起让状态词付出代价：那里第二条提示
    // 与退出都装得下，而它装不下。
    assert_eq!(
        wording::status_line(false, 60),
        "enter 发送 · ctrl-j 换行 · esc 取消 · ctrl-c/ctrl-d 退出"
    );
    assert_eq!(
        wording::status_line(false, 80),
        "就绪 · enter 发送 · ctrl-j 换行 · esc 取消 · shift+tab 模式 · ctrl-c/ctrl-d 退出"
    );
    // 忙把状态词与出路对调：60 列下输掉的那条提示仍然是
    // `shift+tab 模式`，而 `ctrl-c 退出` —— 不带 `ctrl-d` —— 在那里。
    assert_eq!(
        wording::status_line(true, 60),
        "工作中 · enter 发送 · ctrl-j 换行 · esc 取消 · ctrl-c 退出"
    );
    let full = "就绪 · enter 发送 · ctrl-j 换行 · esc 取消 · shift+tab 模式 · PgUp/PgDn 滚动 · ctrl-c/ctrl-d 退出";
    assert_eq!(wording::status_line(false, 120), full);
    // 在最大宽度上行是稳定的：再没什么可买的了。
    assert_eq!(wording::status_line(false, 174), full);
    // 忙对调的是状态词与退出，不是阶梯。
    assert_eq!(
        wording::status_line(true, 120).replace("工作中", "就绪"),
        full.replace("ctrl-c/ctrl-d 退出", "ctrl-c 退出")
    );
}

#[test]
fn no_hint_ever_names_shift_enter() {
    // 没有键盘增强协议时 `Shift+Enter` 与 `Enter` 分不开，而后者
    // 会提交 —— 所以点它名的提示会是一句假话，而那句假话
    // 在任何单一宽度下都看不出来（spec §10，用户故事 54）。把它们全扫一遍。
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
    // `--version`（`src/cli.rs` 打印 `fs-agent {version}`）。这两处写法
    // 写在两个地方，所以这条把它们钉在一起：改任何一处
    // 都会让检查变红，而不是悄悄什么都匹配不上。
    assert_eq!(
        wording::identity(),
        format!("fs-agent {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn the_identities_dash_falls_without_moving_anything_else() {
    // 窄档上的忙碌信号（`.scratch/tui-input-pulse/spec.md` §2）：`fs-agent`
    // 里那个短横是唯一会变的字符，正是这一点让这一行读起来像
    // 「在干活」，而不是像另一个字符串。版本与 crate 名都来自
    // `identity()`，所以启动检查的那个锚与这条下落的行
    // 不可能漂开。
    let identity = wording::identity();
    let mut seen = Vec::new();
    for phase in 0..wording::DASH_FALL.len() {
        let fallen = wording::identity_falling(phase);
        assert_eq!(
            fallen.replace(wording::DASH_FALL[phase], "-"),
            identity,
            "第 {phase} 帧与空闲身份只差那个短横：{fallen}"
        );
        seen.push(fallen);
    }
    // 一次下落五帧，而每一帧都是横条、不是转轮：高度
    // 从高走到低再走回高，一套旋转字形是过不了这一关的。
    let heights: Vec<char> = seen
        .iter()
        .map(|line| line.chars().nth(2).unwrap())
        .collect();
    assert_eq!(
        heights,
        wording::DASH_FALL.to_vec(),
        "这些帧按顺序下落：{seen:?}"
    );
    assert!(
        heights.iter().all(|glyph| matches!(glyph, '▀' | '█' | '▄')),
        "而每一帧都是某个高度的横条：{seen:?}"
    );
    // 下落是闭合的：越过末端的相位会绕回去而不是 panic，因为
    // 它来自的那个脉冲计数器只会增长。
    assert_eq!(
        wording::identity_falling(wording::DASH_FALL.len()),
        wording::identity_falling(0)
    );
}

#[test]
fn the_mark_is_five_rows_of_one_width() {
    // 左栏要么整块画标记、要么完全不画 —— `layout` 在任何东西被画出来
    // 之前就按 `LOGO_WIDTH` 定了这件事。一行宽度不同就会溜过
    // 那道闸、画到边框上去，所以这条契约钉在这里、钉在
    // 字符所在的地方，而不是在画家那边指望它。
    let rows = wording::logo_lines();
    assert_eq!(rows.len(), 5, "标记是五行：{rows:?}");
    for row in rows {
        assert_eq!(
            row.chars().count(),
            fs_agent::render::layout::LOGO_WIDTH as usize,
            "每一行都是布局预留的那个宽度：{row:?}"
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
        wording::banner("s-1", "kimi-k3", Mode::Auto, "/tmp/ws", true),
        "fs-agent：会话 s-1 · 模型 kimi-k3 · 模式 自动 · /tmp/ws（已继续）"
    );
}

#[test]
fn interactive_feedback_reads_in_chinese() {
    assert_eq!(wording::nothing_to_undo(), "没有可撤销的修改");
    assert_eq!(
        wording::unknown_command("/nope", &[]),
        "未知命令 /nope（可用：/undo、/discuss、/quit，或直接输入 /<技能名>）"
    );
    assert_eq!(
        wording::unknown_command("/nope", &["ask-matt", "release"]),
        "未知命令 /nope（可用：/undo、/discuss、/quit；技能：/ask-matt、/release）"
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
    // help 必须点名的那些模式入口：旗标，与手势。
    assert!(interactive.contains("--mode MODE"), "{interactive}");
    assert!(interactive.contains("只读 / 询问 / 自动"), "{interactive}");
    assert!(interactive.contains("Shift+Tab"), "{interactive}");

    // 讨论现在有自己的前端了，两份 help 都这么说。
    assert!(main.contains("fs-agent discuss"), "{main}");
    let discuss = wording::help_discuss();
    assert!(discuss.contains("fs-agent discuss"), "{discuss}");
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
    use fs_agent::provider::FinishReason;
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
        wording::token_pair(12_345, Some(100_000)),
        "12,345 / 100,000"
    );
    // 没有额度不是额度缺失：上限压根儿就不在那里。
    assert_eq!(wording::token_pair(12_345, None), "12,345");

    // 一次调用报出它的输入之前，窗口是未知的 —— 不是零。
    assert_eq!(
        wording::context_pair(None, 200_000, true),
        wording::PANEL_UNKNOWN
    );
    assert_eq!(
        wording::context_pair(Some(12_345), 200_000, false),
        "12,345 / 200,000"
    );
    // 占比是同一个字段的一部分，丢掉它就是这个字段降级的方式。
    assert_eq!(
        wording::context_pair(Some(12_345), 200_000, true),
        "12,345 / 200,000（6%）"
    );
    assert_eq!(wording::cache_pair(9_000, 3_345), "9,000 / 3,345");

    // 回合字段数的是*回合*，`CONTEXT.md` 把它与轮次分得很开。
    assert_eq!(wording::PANEL_TURNS, "回合");
    assert_eq!(wording::PANEL_UNKNOWN, "—");
}

#[test]
fn the_sidebar_names_its_pages_and_says_which_are_not_built() {
    // 三个页签，以及还没有内容的页会说什么 —— 而不是显示
    // 一块空白或编出来的数据（`.scratch/tui-sidebar/spec.md` §3）。
    assert_eq!(wording::TAB_USAGE, "调用量");
    assert_eq!(wording::TAB_TRACE, "轨迹");
    assert_eq!(wording::TAB_FILES, "文件");
    assert_eq!(wording::tab_placeholder(), "此页尚未实现（另有票在跟）");
    // 回合条的字形：一个普通单位、被聚焦的那个，以及这一列
    // 装不下的单位用的标记。
    assert_eq!(wording::RAIL_CELL, "┊");
    assert_eq!(wording::RAIL_FOCUS, "┃");
    assert_eq!(wording::RAIL_TRUNCATED, "⋮");
}

#[test]
fn the_status_row_gives_up_the_model_then_the_mode_and_never_itself() {
    // 短形永远带着自己的标签，所以不会凭空冒出一个
    // 没有解释的 `6%` —— 而一次调用报出它的输入之前，它会说出来，
    // 而不是显示一个零（spec §5）。
    assert_eq!(wording::context_share(Some(12_345), 200_000), "上下文 6%");
    assert_eq!(wording::context_share(None, 200_000), "上下文 —");

    let model = "claude-sonnet-4-5";
    let mode = wording::mode_field(Mode::Ask);
    let share = wording::context_share(Some(12_345), 200_000);
    // 三档，由主列真正拥有的宽度决定（spec §2）：
    // 全都要，然后丢掉模型，然后只剩占比 —— 而它就停在那里，
    // 因为那个足以拿掉这一行的宽度
    // 低于终端地板。
    assert_eq!(
        wording::status_row(model, &mode, &share, 77),
        " 模型 claude-sonnet-4-5 │ 模式 询问 │ 上下文 6% "
    );
    assert_eq!(
        wording::status_row(model, &mode, &share, 45),
        " 模式 询问 │ 上下文 6% ",
        "模型是第一个让位的字段"
    );
    assert_eq!(
        wording::status_row(model, &mode, &share, 11),
        " 上下文 6% ",
        "接着让位的是模式"
    );
    assert_eq!(
        wording::status_row(model, &mode, &share, 4),
        " 上下文 6% ",
        "没有哪一档会拿掉这一行：连占比都装不下的宽度归画家去截"
    );
}

#[test]
fn a_questionnaire_reads_in_chinese_and_pages() {
    // 页脚是页码指示加上那些键，所以读者总知道这是第几个问题、
    // 键盘都干什么（spec §19）。
    assert_eq!(wording::questionnaire_progress(1, 3), "2 / 3");
    // 只有每个问题都处理完之后，页脚才承诺 `提交`；在那之前
    // enter 是继续（spec §7）。
    assert_eq!(
        wording::questionnaire_status(1, 3, false),
        "2 / 3 · ↑↓ 选择 · enter 继续 · space 确认 · tab 跳过 · ←→ 换题"
    );
    assert_eq!(
        wording::questionnaire_hint(false),
        "↑↓ 选择 · enter 继续 · space 确认 · tab 跳过 · ←→ 换题"
    );
    assert_eq!(
        wording::questionnaire_status(1, 3, true),
        "2 / 3 · ↑↓ 选择 · enter 提交 · space 确认 · tab 跳过 · ←→ 换题"
    );
    assert_eq!(
        wording::questionnaire_hint(true),
        "↑↓ 选择 · enter 提交 · space 确认 · tab 跳过 · ←→ 换题"
    );
    assert_eq!(wording::questionnaire_multi_marker(), "（可多选）");
    assert_eq!(wording::questionnaire_answer_label(), "回答：");
    assert_eq!(wording::questionnaire_custom_label(), "自定义：");
}

#[test]
fn a_recommended_option_keeps_its_value_when_displayed() {
    // 这个标记是显示约定：选项读出来时丢掉那个后缀，而
    // 答案携带的值保留整段标签（spec §7）。
    assert_eq!(wording::recommended_badge(), "（推荐）");
    assert_eq!(
        wording::recommended_label("serde (Recommended)"),
        ("serde", true)
    );
    assert_eq!(wording::recommended_label("manual"), ("manual", false));
    // 只有在最末尾、分毫不差的那个后缀才算数。
    assert_eq!(
        wording::recommended_label("Recommended reading"),
        ("Recommended reading", false)
    );
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
