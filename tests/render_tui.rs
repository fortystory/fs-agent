//! TUI 的状态机与共用的块渲染，不用终端就能测。
//!
//! 终端本身是 crossterm 的；本 crate 拥有的，是那个决定画什么、
//! 一个键是什么意思的状态。把它与终端拆开，正是让这件事
//! 可测的原因（spec §Testing Decisions）。

use fs_agent::events::{
    hook_format, Decision, DecisionSource, Event, EventPayload, Role, SpeakerId, StopReason,
    ToolCallId,
};
use fs_agent::permissions::{Answer, PermissionRequest};
use fs_agent::render::palette;
use fs_agent::render::{
    pane, render_block_uncoloured, AskRequest, Block, ConsoleRequest, DeltaKind, FrontEndEvent,
    Key, RenderEvent, SessionFacts, ToolBlock, ToolOutcome, Transcript, TuiState,
};
use ratatui::style::{Color, Modifier};

/// 一次关于 `tool_name` 的权限询问，形状与循环交给前端的一样。
fn permission_request(tool_name: &str) -> PermissionRequest {
    PermissionRequest {
        escalation: None,
        speaker: None,
        request_id: "r-1".to_owned(),
        tool_call_id: "c-1".to_owned(),
        tool_name: tool_name.to_owned(),
        args: serde_json::json!({}),
        reason: "mode ask".to_owned(),
    }
}

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

fn facts() -> SessionFacts {
    SessionFacts {
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: "~/code/fortystory/fs-agent".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        mode: fs_agent::permissions::Mode::Ask,
        budget_limit: Some(100_000),
        number_style: fs_agent::render::wording::NumberStyle::Cn,
        speaker_order: Vec::new(),
    }
}

fn new_state() -> TuiState {
    TuiState::new(
        facts(),
        std::path::PathBuf::from("/home/forty/code/fs-agent"),
        Some(std::path::PathBuf::from("/home/forty")),
    )
}

fn state_with_prompt() -> (TuiState, tokio::sync::oneshot::Receiver<Option<String>>) {
    let mut state = new_state();
    let (tx, rx) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    (state, rx)
}

/// 循环报为**在一次运行之内**的那个状态。
///
/// 这是进入其中的唯一途径：前端从不自己推断它，所以想要取消手势的测试
/// 必须像循环那样说出来。
fn state_running() -> TuiState {
    let mut state = new_state();
    state.request(ConsoleRequest::RunState { running: true });
    state
}

#[test]
fn the_pulse_runs_in_idle_too_and_only_the_status_glyph_moves() {
    // 时钟的那一帧既驱动提示符的色相，也驱动状态行那个字形循环
    // （`.scratch/tui-visual-language/spec.md` §30–§32）。票 09 曾让空闲的 tick「连一帧都不该
    // 要」；§32 推翻了它：动的不再是提示符（空闲时它仍歇在帧 0 的颜色上），而是状态词前面
    // 那个字形 —— 那是新的一条信息通道，不是给静态元素加装饰。代价是空闲不再零唤醒。
    let mut state = new_state();
    state.mark_clean();
    state.tick();
    assert!(state.is_dirty(), "空闲的一次 tick 也要下一帧：字形在动");

    state.request(ConsoleRequest::RunState { running: true });
    state.mark_clean();
    state.tick();
    assert!(state.is_dirty(), "运行时的一次 tick 会要下一帧");

    state.request(ConsoleRequest::RunState { running: false });
    state.mark_clean();
    state.tick();
    assert!(state.is_dirty(), "这台时钟不再随运行一起停");
}

/// 字形循环的速率：运行中每 2 帧换一格，空闲每 8 帧 —— 同一个时钟，两种速度。
///
/// 一轮是**八格月相**（`.scratch/tui-visual-language/spec.md` §30）：运行中 8 × 2 帧 ≈ 0.96 秒
/// 走完一轮，空闲 8 × 8 帧 ≈ 3.8 秒（2026-10-06 维护者把速率加快了一档）。
#[test]
fn the_status_glyph_changes_every_two_frames_running_and_eight_idle() {
    use fs_agent::render::wording;

    assert_eq!(wording::status_spinner(0, true), "🌑");
    assert_eq!(wording::status_spinner(1, true), "🌑");
    assert_eq!(wording::status_spinner(2, true), "🌒");
    assert_eq!(wording::status_spinner(4, true), "🌓");
    assert_eq!(wording::status_spinner(6, true), "🌔");
    assert_eq!(wording::status_spinner(8, true), "🌕");
    assert_eq!(wording::status_spinner(16, true), "🌑", "一轮之后回到起点");

    assert_eq!(wording::status_spinner(0, false), "🌑");
    assert_eq!(wording::status_spinner(7, false), "🌑", "空闲慢下来");
    assert_eq!(wording::status_spinner(8, false), "🌒");
    assert_eq!(wording::status_spinner(56, false), "🌘");
    assert_eq!(wording::status_spinner(64, false), "🌑");
}

#[test]
fn a_typed_line_is_submitted_to_the_loop() {
    let (mut state, mut answer) = state_with_prompt();
    for ch in "hello".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), Some("hello".to_owned()));
}

#[test]
fn backspace_edits_the_line() {
    let (mut state, mut answer) = state_with_prompt();
    state.key(Key::Char('a'));
    state.key(Key::Char('b'));
    state.key(Key::Backspace);
    state.key(Key::Char('c'));
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), Some("ac".to_owned()));
}

#[test]
fn an_empty_submission_is_an_empty_line_not_the_end_of_input() {
    // 提示通道上的 `None` 意味着 stdin 关了，循环会在它上面停下 ——
    // 所以对空草稿按 Enter 不许把它发出去。以前会：
    // 在空提示符下按 Enter 会退出会话。
    let (mut state, mut answer) = state_with_prompt();
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), Some(String::new()));
}

#[test]
fn a_permission_question_is_answered_by_key() {
    let mut state = new_state();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        request: PermissionRequest {
            escalation: None,
            speaker: None,
            request_id: "r-1".to_owned(),
            tool_call_id: "c-1".to_owned(),
            tool_name: "write_file".to_owned(),
            args: serde_json::json!({}),
            reason: "mode ask".to_owned(),
        },
        reply: tx,
    }));
    state.key(Key::Char('a'));
    assert_eq!(rx.try_recv().unwrap(), Answer::AlwaysAllow);
}

#[test]
fn escape_answers_a_question_with_the_non_acting_choice() {
    // 一次询问是在**一次运行之外**问的（前端停在循环自己抬起的那个
    // 问题上）：那里的 `Esc` 是「不做」那个答案，绝不是批准。
    let (mut state, _line) = state_with_prompt();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        request: PermissionRequest {
            escalation: None,
            speaker: None,
            request_id: "r-1".to_owned(),
            tool_call_id: "c-1".to_owned(),
            tool_name: "bash".to_owned(),
            args: serde_json::json!({}),
            reason: "mode ask".to_owned(),
        },
        reply: tx,
    }));
    state.key(Key::Esc);
    assert_eq!(rx.try_recv().unwrap(), Answer::Deny);
}

#[test]
fn escape_while_working_is_a_cancel_gesture() {
    // 「在干活」是循环自己的报告，不是流暗示出来的：一条增量说
    // 一次调用*开始了*，这与「在跑」不是一回事 —— 合成器的那
    // 唯一一次调用就是其中之一。
    let mut state = state_running();
    state.key(Key::Esc);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
}

#[test]
fn ctrl_c_quits_before_the_loop_has_asked_for_its_first_line() {
    // 这条测试钉住的 bug：`busy` 曾是 `prompt_reply.is_none()`，而
    // 从第一帧起、直到循环要第一行之前它都为真 —— 那是整个组装期，
    // TUI 都在原始模式下读键。那里的 `Ctrl-C` 变成了取消手势，而
    // 空闲的循环会丢掉这些手势，于是这个键什么都没做。一次 pty
    // 探针证明了它：进入约 20ms 时按一次 `Ctrl-C`，进程一直活到没人看。
    //
    // 现在那个键是**双击**（`.scratch/exit-gesture/spec.md` §1）：第一下只举手，
    // 让手快的人能收回那一下；第二下才真的走。
    let mut fresh = new_state();
    fresh.key(Key::CtrlC);
    assert!(!fresh.should_quit(), "第一下只举手，不退");
    assert!(fresh.take_events().is_empty(), "也没有手势发出去");
    fresh.key(Key::CtrlC);
    assert!(fresh.should_quit(), "没人读的键盘，第二下会退出");
    assert!(fresh.take_events().is_empty());
}

#[test]
fn an_idle_ctrl_c_double_taps_and_a_working_one_cancels_first() {
    // 空闲意味着循环在等一行，而它有两处说法：它已经要过一行，
    // 并且报过没有东西在跑（spec §6）。
    let (mut idle, _line) = state_with_prompt();
    idle.key(Key::CtrlC);
    assert!(!idle.should_quit(), "第一下只举手");
    // 两键共用同一把举手，所以混按也算第二下。
    idle.key(Key::CtrlD);
    assert!(idle.should_quit(), "Ctrl-C 之后 Ctrl-D 也算第二下");

    // 一次进行中的运行反着来：第一下是取消手势，第二下才退出 —— 而退出是推给 CLI 的
    // 一条手势（它走有序收尾、以 130 收尾，票 03），渲染器自己不设 `quit`。
    let mut state = state_running();
    state.key(Key::CtrlC);
    assert!(!state.should_quit());
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
    state.key(Key::CtrlC);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Quit]);
    assert!(!state.should_quit(), "退出的收尾归 CLI");
}

#[test]
fn a_run_that_never_ended_a_turn_still_leaves_ctrl_c_quitting() {
    // 这条测试钉住的 bug：`busy` 曾从渲染事件推断，而只有
    // `TurnEnded` 会清掉它。合成器的那唯一一次调用 —— 一场讨论
    // 的收尾调用 —— 流过增量并且**不**结束任何回合，所以一场讨论之后
    // TUI 继续相信自己还在干活，`Ctrl-C` 变成了取消手势，而空闲的
    // 循环忽略那些手势：这个会话退不出去。
    //
    // 下面这条流就是过去会把前端搁浅的那条；不同的是
    // 循环自己报出运行的结束，于是不必有任何回合结束，
    // 键盘就能回来。
    let mut state = state_running();
    state.apply(RenderEvent::Delta {
        speaker: kimi(),
        kind: DeltaKind::Text,
        text: "synthesizing".to_owned(),
    });
    // 讨论结束了，循环在等下一行。
    state.request(ConsoleRequest::RunState { running: false });

    state.key(Key::CtrlC);
    assert!(!state.should_quit(), "第一下只举手");
    state.key(Key::CtrlC);
    assert!(
        state.should_quit(),
        "即使从没有 `TurnEnded` 到达，空闲的双击也退得出去"
    );
    assert!(state.take_events().is_empty());
}

#[test]
fn a_gesture_raised_while_busy_still_quits_with_130_after_the_turn_ends() {
    // 第一下 `Ctrl-C` 取消了回合，而取消可能马上落地：第二下到达时渲染器已经空闲。那一把
    // 举手记得自己是在忙碌里举起的，所以第二次仍然推 `Quit` —— CLI 按 130 收尾 —— 而不是
    // 退回空闲语义的 0（`.scratch/exit-gesture/spec.md` §3）。
    let mut state = state_running();
    state.key(Key::CtrlC);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);

    // 回合收尾，循环报出它已经不在跑。
    state.request(ConsoleRequest::RunState { running: false });
    state.key(Key::CtrlC);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Quit]);
    assert!(!state.should_quit(), "退出的收尾归 CLI");
}

#[test]
fn shift_tab_is_the_mode_gesture() {
    let mut state = new_state();
    state.key(Key::BackTab);
    assert_eq!(state.take_events(), vec![FrontEndEvent::CycleMode]);
}

#[test]
fn ctrl_d_is_the_same_double_tap_as_ctrl_c() {
    // 空闲时两键完全对等（`.scratch/exit-gesture/spec.md` §1）：旧的那套 `y` / `Enter` /
    // `Esc` 确认框连同它的覆盖层一起拆了（§4），所以这里测的是双击与「别的键清举手」。
    let (mut idle, _line) = state_with_prompt();
    idle.key(Key::CtrlD);
    assert!(!idle.should_quit(), "第一下只举手");
    idle.key(Key::Enter);
    assert!(
        !idle.should_quit(),
        "Enter 不再是确认，而是一个清掉举手的普通键"
    );
    idle.key(Key::CtrlD);
    assert!(!idle.should_quit(), "举手清掉之后，这一下又是第一下");
    idle.key(Key::CtrlD);
    assert!(idle.should_quit(), "第二下退出");
}

#[test]
fn any_other_key_clears_a_raised_exit_gesture() {
    // 半分钟前那一下不该莫名其妙地算数（`.scratch/exit-gesture/spec.md` §1）。
    let mut state = new_state();
    state.key(Key::CtrlC);
    assert!(!state.should_quit());
    state.key(Key::Char('a'));
    state.key(Key::CtrlC);
    assert!(!state.should_quit(), "旧举手被清掉了，这一下是新的第一下");
    state.key(Key::CtrlC);
    assert!(state.should_quit(), "举手重来之后，下一对照样退得出去");
}

#[test]
fn the_exit_gesture_lives_for_the_window_and_then_expires() {
    // 时间可注入，所以这条不睡真实时间（`.scratch/exit-gesture/spec.md` §1、§6）。
    use std::time::{Duration, Instant};

    let mut state = new_state();
    let t0 = Instant::now();
    state.raise_exit_gesture_at(t0);
    assert_eq!(
        state.exit_deadline(),
        Some(t0 + Duration::from_millis(500)),
        "寿命就是那个窗口"
    );
    assert!(state.exit_gesture_raised(t0 + Duration::from_millis(400)));
    assert!(!state.exit_gesture_raised(t0 + Duration::from_millis(600)));
    assert!(!state.should_quit(), "超时只是作废，不是退出");

    state.mark_clean();
    state.expire_exit_gesture();
    assert_eq!(state.exit_deadline(), None, "作废之后字段清掉");
    assert!(state.is_dirty(), "作废要重画提示行（它恢复原样）");
    assert!(!state.exit_gesture_raised(t0), "作废之后就不算数了");
    assert!(!state.should_quit());
    // 作废之后要重新按两下。
    state.key(Key::CtrlC);
    assert!(!state.should_quit());
}

#[test]
fn ctrl_d_is_ignored_while_a_run_is_in_flight() {
    // 忙时：`Ctrl-D` 什么都不做 —— 不退出、不取消（`.scratch/exit-gesture/spec.md` §1）。
    // 停下一次运行仍然靠 `Ctrl-C`，而误退的代价比误取消大。
    let mut state = state_running();
    state.key(Key::CtrlD);
    assert!(!state.should_quit());
    assert!(state.take_events().is_empty(), "也没有取消手势");

    // 而且它**不清**正在举的那把手：先 `Ctrl-C` 举手（那一下取消了回合），`Ctrl-D` 之后
    // 再按 `Ctrl-C` 仍然算第二下。
    state.key(Key::CtrlC);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
    state.key(Key::CtrlD);
    assert!(state.take_events().is_empty());
    state.key(Key::CtrlC);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Quit]);
}

#[test]
fn a_gesture_raised_while_idle_does_not_become_a_130_when_a_turn_starts() {
    // `exit-gesture` spec §3：空闲双击**不再**「顺便」变成 130 —— 哪怕那半秒窗口里恰好有一个
    // 回合开跑。第二下按的是**这一把手的出身**，不是按下那一刻的忙碌状态。
    let (mut state, _line) = state_with_prompt();

    state.key(Key::CtrlC);
    assert!(state.exit_deadline().is_some(), "空闲里举起了手");

    // 窗口还没过，一个回合开跑了。
    state.request(ConsoleRequest::RunState { running: true });

    state.key(Key::CtrlC);
    assert!(state.should_quit(), "按出身走：这是空闲那把，退 0");
    assert!(
        state.take_events().is_empty(),
        "不该推 `Quit` —— 那是「忙碌中被打断」那条 130 的路"
    );
}

#[test]
fn ctrl_d_is_ignored_while_a_question_is_up() {
    // 一个问题占着键盘，所以 `Ctrl-D` 归那个问题忽略。下面这个
    // 权限覆盖层是循环的，而它等的那个答案不受这个
    // 乱入的键影响。
    let (mut state, _line) = state_with_prompt();
    let (reply, mut answer) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        request: PermissionRequest {
            escalation: None,
            speaker: None,
            request_id: "r-1".to_owned(),
            tool_call_id: "c-1".to_owned(),
            tool_name: "bash".to_owned(),
            args: serde_json::json!({"command": "ls"}),
            reason: "mode ask".to_owned(),
        },
        reply,
    }));
    state.key(Key::CtrlD);
    assert!(!state.should_quit(), "这个键被忽略，而不是被照做");
    assert!(
        state.exit_deadline().is_none(),
        "而且没有举手：它是「被忽略」，不是「别的键」"
    );
    // 连按两下也不该在窗口内退出 —— 被忽略的键不该因为凑够次数就作数。
    state.key(Key::CtrlD);
    assert!(!state.should_quit(), "连按两下仍然被忽略");
    assert!(state.exit_deadline().is_none());
    state.key(Key::Char('y'));
    assert_eq!(
        answer.try_recv().expect("权限答案发出去了"),
        Answer::Allow,
        "而它为之被忽略的那个问题仍然可作答"
    );
}

#[test]
fn a_tool_call_is_painted_by_its_result_and_annotated_by_its_hook() {
    // 合成归转录。**结果**才是结束这条调用、画出它的东西，所以调用行
    // 在工具一结束时就准备好了；而后置 hook 不带
    // `tool_call_id`，它作为自己的块跟在刚画出的那条调用后面。
    // 反过来为 hook 把调用留着不画，会让它整段运行
    // 都藏着（票 02 §3）。
    let id = ToolCallId::new("call-1");
    let mut transcript = Transcript::new();
    let call = Event::new(
        1,
        kimi(),
        EventPayload::ToolCallStarted {
            tool_call_id: id.clone(),
            tool_name: "read_file".to_owned(),
            args: serde_json::json!({"path": "a.rs"}),
        },
    );
    let result = Event::new(
        2,
        kimi(),
        EventPayload::ToolCallCompleted {
            tool_call_id: id,
            ok: true,
            output: Some("fn main() {}".to_owned()),
            error: None,
            duration_ms: 2,
        },
    );
    let hook = Event::new(
        3,
        kimi(),
        EventPayload::HookExecuted {
            point: hook_format::POINT_POST.to_owned(),
            command: "check".to_owned(),
            outcome: hook_format::feedback("ok").to_owned(),
        },
    );

    assert!(
        transcript.push(RenderEvent::Logged(call)).is_empty(),
        "进行中的调用还没画"
    );
    let ready = transcript.push(RenderEvent::Logged(result));
    let tool = ready
        .iter()
        .find_map(|block| match block {
            Block::Tool(tool) => Some(tool.as_ref()),
            _ => None,
        })
        .expect("结果画出这条调用");
    assert_eq!(tool.tool, "read_file");
    assert!(tool.outcome.as_ref().unwrap().ok);

    let feedback = transcript.push(RenderEvent::Logged(hook));
    assert_eq!(feedback.len(), 1, "hook 是自己的块：{feedback:#?}");
    assert!(
        matches!(
            &feedback[0],
            Block::ToolFeedback { outcome, .. } if outcome == &hook_format::feedback("ok")
        ),
        "而它带着给这条调用作注的那条反馈：{feedback:#?}"
    );
}

#[test]
fn a_completed_turn_and_an_aborted_one_render_in_different_colors() {
    let good = render_block_uncoloured(&Block::TurnEnded {
        speaker: kimi(),
        reason: StopReason::Completed,
    });
    let aborted = render_block_uncoloured(&Block::TurnEnded {
        speaker: kimi(),
        reason: StopReason::Aborted,
    });
    let bad = render_block_uncoloured(&Block::TurnEnded {
        speaker: kimi(),
        reason: StopReason::Error,
    });
    // 这一行是两段 span 拼的 `[name] text`：名字带发言者色、
    // 正文带严重性，所以严重性断言在第二段上。`Good` 与 `Note` 退成静音档 —— 正常完成不再
    // 抢注意力，只有出问题时屏幕才亮（`.scratch/tui-visual-language/spec.md` §4、§6）。
    for (line, color) in [
        (&good[0], palette::MUTED),
        (&aborted[0], palette::WARN),
        (&bad[0], palette::BAD),
    ] {
        assert_eq!(
            line.spans[1].style.fg,
            Some(color),
            "正文保持严重性：{line:?}"
        );
    }
    // 名册为空时每个名字与「系统」同一种灰，绝不是严重性色。
    assert_eq!(good[0].spans[0].style.fg, Some(palette::SYSTEM));
}

#[test]
fn a_speakers_name_is_drawn_in_its_role_colour() {
    use fs_agent::render::{render_block, SpeakerColors};

    let name = |blocks: Vec<Block>, roster: &[&str]| {
        let roster: Vec<String> = roster.iter().map(|name| (*name).to_owned()).collect();
        let mut colors = SpeakerColors::new(&roster);
        let lines: Vec<ratatui::text::Line<'static>> = blocks
            .iter()
            .flat_map(|block| render_block(block, &mut colors))
            .collect();
        lines
            .first()
            .map(|line| line.spans[0].style.fg)
            .expect("一行")
    };

    // 两个讨论者按名册顺序取调色板，而调用方的顺序就是
    // 颜色：第一个是青，第二个是品红（票 07 §1）。
    assert_eq!(
        name(
            vec![Block::TurnStarted {
                speaker: SpeakerId::Debater("kimi".into()),
                iteration: 1,
            }],
            &["kimi", "claude"],
        ),
        Some(Color::LightCyan)
    );
    assert_eq!(
        name(
            vec![Block::TurnStarted {
                speaker: SpeakerId::Debater("claude".into()),
                iteration: 1,
            }],
            &["kimi", "claude"],
        ),
        Some(Color::LightMagenta)
    );

    // 执行者、用户与系统有固定角色，不论名册如何。
    assert_eq!(
        name(
            vec![Block::TurnStarted {
                speaker: SpeakerId::Executor("worker".into()),
                iteration: 1,
            }],
            &["kimi", "claude"],
        ),
        Some(Color::LightYellow)
    );
    assert_eq!(
        name(
            vec![Block::Message {
                speaker: SpeakerId::User,
                role: Role::User,
                text: "hello".to_owned(),
                reasoning: None,
            }],
            &["kimi", "claude"],
        ),
        Some(Color::LightGreen)
    );
    assert_eq!(
        name(vec![Block::Notice(String::new())], &["kimi", "claude"]),
        Some(Color::DarkGray),
        "没有发言者的行保持叙述灰"
    );
    assert_eq!(
        name(
            vec![Block::TurnStarted {
                speaker: SpeakerId::System,
                iteration: 1,
            }],
            &["kimi", "claude"],
        ),
        Some(palette::SYSTEM),
        "系统色是冻结的那一档"
    );
    // 「没有名字册」与「系统」用**同一种灰**（用户故事 5）：同一个语义不该在屏幕上出现
    // 两种样子。
    assert_eq!(
        render_block_uncoloured(&Block::TurnStarted {
            speaker: SpeakerId::Debater("kimi".into()),
            iteration: 1,
        })[0]
            .spans[0]
            .style
            .fg,
        Some(palette::SYSTEM),
        "没有名册时每个名字都用系统那一种灰"
    );

    // 名册没点名的讨论者 —— 会话中途 `/discuss` 那种情况 —— 取
    // 第一个没人认领的槽，而此后这个名字的每一行都沿用那个槽。
    let mut colors = SpeakerColors::new(&["kimi".to_owned()]);
    let first = render_block(
        &Block::TurnStarted {
            speaker: SpeakerId::Debater("newcomer".into()),
            iteration: 1,
        },
        &mut colors,
    );
    let second = render_block(
        &Block::TurnStarted {
            speaker: SpeakerId::Debater("newcomer".into()),
            iteration: 2,
        },
        &mut colors,
    );
    assert_eq!(first[0].spans[0].style.fg, Some(Color::LightMagenta));
    assert_eq!(
        second[0].spans[0].style.fg,
        Some(Color::LightMagenta),
        "而这个颜色不会在读者眼皮底下挪动"
    );
}

#[test]
fn a_tool_block_paints_one_line_and_folds_the_rest() {
    // 折叠的工具块只有那条调用行。输出正文根本不再画 —— 它住在
    // 调用行的详情视图后面 —— 而失败是同一行上的一个后缀，
    // 不是自己单独一行（票 02 §3）。
    let tool = |outcome: Option<ToolOutcome>| ToolBlock {
        speaker: kimi(),
        tool_call_id: ToolCallId::new("call-2"),
        tool: "read_file".to_owned(),
        args: serde_json::json!({"path": "a.rs"}),
        outcome,
    };
    let ok = render_block_uncoloured(&Block::Tool(Box::new(tool(Some(ToolOutcome {
        ok: true,
        output: Some("fn main() {}".to_owned()),
        error: None,
        duration_ms: 1,
    })))));
    assert_eq!(ok.len(), 1, "成功的调用是一行：{ok:#?}");
    let call: String = ok[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(call, "[kimi] ▸ 调用 read_file a.rs");
    assert!(
        !ok[0].spans.iter().any(|span| span.style.bg.is_some()),
        "也不带输出正文：{:#?}",
        ok[0]
    );

    // 失败是同一行末尾带 `失败`；错误正文不在这里。
    let failed = render_block_uncoloured(&Block::Tool(Box::new(tool(Some(ToolOutcome {
        ok: false,
        output: None,
        error: Some("no such file".to_owned()),
        duration_ms: 1,
    })))));
    assert_eq!(failed.len(), 1, "失败的调用也是一行：{failed:#?}");
    let call: String = failed[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(call, "[kimi] ▸ 调用 read_file a.rs 失败");

    // 后置 hook 的反馈是政策，不是输出：它留在屏幕上，作为关于
    // 刚画出的那条调用自己的块。
    let hooked = render_block_uncoloured(&Block::ToolFeedback {
        outcome: "formatted with rustfmt".to_owned(),
    });
    assert_eq!(hooked.len(), 1, "一条反馈行：{hooked:#?}");
    assert!(
        hooked[0]
            .spans
            .iter()
            .any(|span| span.content.contains("formatted with rustfmt")),
        "而它带着 hook 的话：{hooked:#?}"
    );
}

#[test]
fn the_synthesizers_product_renders_with_the_system_speaker() {
    let lines = render_block_uncoloured(&Block::Message {
        speaker: SpeakerId::System,
        role: Role::Assistant,
        text: "consensus".to_owned(),
        reasoning: None,
    });
    // 第一行是发言者前缀**独占的一行**（2026-10-05 的排版修订）。它的字面用词归措辞层；
    // 这里它只需要是一个带方括号的归属。
    let prefix = lines[0].spans[0].content.as_ref();
    assert!(
        prefix.starts_with('[') && prefix.ends_with(']'),
        "一个带方括号的发言者前缀：{prefix:?}"
    );
    assert_eq!(lines[1].spans[0].content.as_ref(), "consensus");
}

#[test]
fn a_notice_is_a_transcript_line_shown_as_it_is() {
    // 启动 banner 是一条通知，不是诊断：不加 `[diag]` 标签，而它
    // 属于转录 —— 在那里它待着不动，而不是随着它刻意
    // 避开的流式尾巴一起滚走
    // （spec §A.12、§3）。
    let banner = "fs-agent: session abc · model m · mode ask · /tmp/x";
    let lines = render_block_uncoloured(&Block::Notice(banner.to_owned()));
    let text: String = lines[0]
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(text, banner);
}

#[test]
fn the_answer_block_is_rendered_as_markdown() {
    // 定稿的答案走 Markdown 渲染器，所以标题就是标题、
    // 列表项就是列表项 —— 转录里可读的那一半。
    let lines = render_block_uncoloured(&Block::Message {
        speaker: kimi(),
        role: Role::Assistant,
        text: "# 标题\n\n- 一\n- 二\n".to_owned(),
        reasoning: None,
    });
    // 第一行是名字，标题在它下面一行（2026-10-05 的排版修订）。
    let heading = &lines[1];
    assert!(
        heading
            .spans
            .iter()
            .any(|span| span.style.fg == Some(Color::Cyan)
                && span.style.add_modifier.contains(Modifier::BOLD)),
        "第一行就是渲染出来的标题：{heading:?}"
    );
    assert_eq!(heading.spans.last().unwrap().content.as_ref(), "标题");

    let rendered: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect();
    assert!(
        rendered.iter().any(|line| line.contains("• 一")),
        "画出来的是列表符号，不是原始的那个连字符：{rendered:?}"
    );
}

#[test]
fn intermediate_narration_is_dim_and_the_answer_is_not() {
    // 每条叙述行读起来都是同一档暗灰，不论它叙述的是哪条事件，
    // 于是模型的答案 —— 满亮度 —— 才是显出来的那个。
    let verdict = render_block_uncoloured(&Block::PermissionDecided {
        speaker: kimi(),
        decision: Decision::Allow,
        source: DecisionSource::User,
        reason: Some("mode ask".to_owned()),
    });
    // 叙述读的是**正文那一段**（第二段）；名字那一段是发言者色，不是叙述色
    // （`.scratch/tui-visual-language/spec.md` §6）。
    assert_eq!(
        verdict[0].spans[1].style.fg,
        Some(palette::MUTED),
        "权限裁决的正文是叙述"
    );

    let asked = render_block_uncoloured(&Block::PermissionAsked {
        speaker: kimi(),
        tool_name: Some("bash".to_owned()),
        args: serde_json::json!({"command": "ls"}),
    });
    assert_eq!(asked[0].spans[1].style.fg, Some(palette::MUTED));

    let answer = render_block_uncoloured(&Block::Message {
        speaker: kimi(),
        role: Role::Assistant,
        text: "正文".to_owned(),
        reasoning: None,
    });
    // 第一行是名字，正文在它下面一行（2026-10-05 的排版修订）。
    assert_ne!(
        answer[1].spans[0].style.fg,
        Some(palette::MUTED),
        "答案正文没有调暗"
    );
}

#[test]
fn a_message_body_starts_on_its_own_line() {
    // 名字独占一行，话从下一行起、顶格 —— 用户输入与非 assistant 的系统行走这条路
    // （2026-10-05 的排版修订）。
    let lines = render_block_uncoloured(&Block::Message {
        speaker: SpeakerId::User,
        role: Role::User,
        text: "one\ntwo".to_owned(),
        reasoning: None,
    });
    assert_eq!(lines.len(), 3, "名字一行，正文每行各占一行");
    assert_eq!(lines[0].spans[0].content.as_ref(), "[用户]");
    assert_eq!(lines[1].spans[0].content.as_ref(), "one");
    assert_eq!(lines[2].spans[0].content.as_ref(), "two");
}

#[test]
fn the_answers_continuation_starts_at_the_left_edge() {
    // 回答是一份文档，结构由 Markdown 自己给（标题、列表、代码块）；`[name] ` 那 11 格
    // 前缀会把它整体推右、又吃掉主列约七分之一（spec §5）。
    let lines = render_block_uncoloured(&Block::Message {
        speaker: kimi(),
        role: Role::Assistant,
        text: "# 标题\n\n正文\n第二行".to_owned(),
        reasoning: None,
    });
    assert!(lines.len() >= 3, "多行答案：{lines:?}");
    for line in &lines[1..] {
        assert!(
            line.spans
                .first()
                .is_none_or(|span| !span.content.starts_with(' ')),
            "除第一行外都顶格：{line:?}"
        );
    }
    assert!(
        lines[0].spans[0].content.starts_with('['),
        "第一行仍然由 `[name] ` 引领：{:?}",
        lines[0]
    );
}

#[test]
fn a_single_line_answer_is_a_name_row_and_a_body_row() {
    // 边界：单行答案也占两行 —— 名字一行、正文一行（2026-10-05 的排版修订）。
    let lines = render_block_uncoloured(&Block::Message {
        speaker: kimi(),
        role: Role::Assistant,
        text: "正文".to_owned(),
        reasoning: None,
    });
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].spans[0].content.as_ref(), "[kimi]");
    assert_eq!(lines[1].spans[0].content.as_ref(), "正文");
}

#[test]
fn the_live_tail_wraps_on_display_columns_not_bytes() {
    // 一个 CJK 字符两列宽、三个字节长。按字节下标折行
    // 会把流式尾巴切在宽度的三分之一处 —— 那是唯一一处
    // 中文明明每格都对、看起来却坏掉的地方（spec §3）。
    let rows = pane::wrap_text("你好世界五六", 10);
    let texts: Vec<String> = rows
        .iter()
        .map(|row| row.spans.iter().map(|span| span.content.as_ref()).collect())
        .collect();
    assert_eq!(texts, vec!["你好世界五", "六"]);
}

#[test]
fn the_arrows_and_emacs_keys_edit_the_line_in_place() {
    let (mut state, mut answer) = state_with_prompt();
    for ch in "helo".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Left); // hel|o
    state.key(Key::Char('l')); // hell|o
    state.key(Key::Home);
    state.key(Key::Delete); // 去掉开头那个 'h' -> ell|o
    state.key(Key::End);
    state.key(Key::Backspace); // ell|
    state.key(Key::Char('o'));
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), Some("ello".to_owned()));
}

#[test]
fn ctrl_a_e_u_k_edit_like_emacs() {
    let (mut state, mut answer) = state_with_prompt();
    for ch in "abcdef".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::CtrlA); // |abcdef
    state.key(Key::CtrlK); // 向前清空 -> |
    state.key(Key::Char('x')); // x|
    state.key(Key::Char('y')); // xy|
    state.key(Key::CtrlA); // |xy
    state.key(Key::Delete); // |y
    state.key(Key::CtrlE); // y|
    state.key(Key::CtrlU); // 向后清空 -> |
    state.key(Key::Char('z'));
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), Some("z".to_owned()));
}

#[test]
fn ctrl_w_erases_the_word_before_the_cursor() {
    let (mut state, mut answer) = state_with_prompt();
    for ch in "cargo test --lib".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::CtrlW);
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), Some("cargo test".to_owned()));
}

#[test]
fn ctrl_p_and_ctrl_n_walk_the_prompt_history() {
    let (mut state, mut first) = state_with_prompt();
    for ch in "first".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert_eq!(first.try_recv().unwrap(), Some("first".to_owned()));

    let (tx, mut second) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    for ch in "second".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert_eq!(second.try_recv().unwrap(), Some("second".to_owned()));

    let (tx, mut third) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    state.key(Key::CtrlP); // 最新的：second
    state.key(Key::CtrlP); // 更旧的：first
    state.key(Key::CtrlP); // 已经是最旧的：停住
    state.key(Key::CtrlN); // 回到 second
    state.key(Key::Enter);
    assert_eq!(third.try_recv().unwrap(), Some("second".to_owned()));
}

#[test]
fn the_arrows_move_the_cursor_while_ctrl_p_and_ctrl_n_walk_the_history() {
    let (mut state, mut first) = state_with_prompt();
    for ch in "kept".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert_eq!(first.try_recv().unwrap(), Some("kept".to_owned()));

    let (tx, mut second) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    for ch in "draft".chars() {
        state.key(Key::Char(ch));
    }
    // 方向键现在归光标：这里的 `Up` 不许把草稿换成上一次
    // 提交 —— 所以提交出去的东西仍然以打进去的内容开头。
    state.key(Key::Up);
    state.key(Key::Char('!'));
    state.key(Key::Enter);
    assert_eq!(second.try_recv().unwrap(), Some("draft!".to_owned()));

    let (tx, mut third) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    state.key(Key::CtrlP); // 最新的一次提交
    state.key(Key::CtrlN); // 再回到那份新草稿，它是空的
    state.key(Key::Char('x'));
    state.key(Key::Enter);
    assert_eq!(third.try_recv().unwrap(), Some("x".to_owned()));
}

#[test]
fn a_users_message_keeps_its_lines_and_its_length() {
    // 转录是为了让人读回被问过什么。一段粘贴进来的多行提示词
    // 必须整段活下来：它以前会被压成一行、并在 500 个
    // 字符处被切断（spec §3）。
    let long = "x".repeat(600);
    let text = format!("第一行\n{long}\n第三行");
    let lines = render_block_uncoloured(&Block::Message {
        speaker: SpeakerId::User,
        role: Role::User,
        text,
        reasoning: None,
    });

    let rendered: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect();
    assert_eq!(rendered.len(), 4, "名字一行，消息每一行各占一行");
    assert_eq!(rendered[0], "[用户]");
    assert!(rendered[1].ends_with("第一行"), "{:?}", rendered[1]);
    assert!(rendered[2].contains(&long), "什么都没被省略");
    assert!(rendered[3].ends_with("第三行"), "{:?}", rendered[3]);
}

#[test]
fn a_paste_never_submits_and_its_line_endings_are_normalised() {
    let (mut state, mut answer) = state_with_prompt();
    // CRLF、一个光秃秃的 CR 与一个乱入的控制字符：全都作为文本
    // 到达，而其中没有一个会按下 Enter（spec §7）。
    state.paste("第一行\r\n第二行\r第三行\x07");
    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().unwrap(),
        Some("第一行\n第二行\n第三行".to_owned())
    );
}

#[test]
fn an_oversized_paste_asks_first_and_only_yes_takes_it() {
    let (mut state, mut answer) = state_with_prompt();
    // 拒绝会留下一个空草稿，而在空草稿上按 Enter 提交的是一个
    // 空行 —— 绝不是输入结束。这里到达的就是那个空行；它
    // 不开启回合是循环的事（`cli.rs` 会裁掉它并再问一次）。
    let empty = Some(String::new());
    let huge = "x".repeat(100_001);

    state.paste(&huge);
    state.key(Key::Char('n'));
    state.key(Key::Enter);
    assert_eq!(answer.try_recv().unwrap(), empty, "拒绝了：文本没了");

    let (tx, mut second) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    state.paste(&huge);
    state.key(Key::Esc); // 安全的答案是否，所以 Esc 也拒绝
    state.key(Key::Enter);
    assert_eq!(second.try_recv().unwrap(), empty, "Esc 不是同意");

    let (tx, mut enter) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    state.paste(&huge);
    state.key(Key::Enter); // 另一个安全答案：Enter 同样拒绝
    state.key(Key::Enter); // 而这一次提交了，因为问题已经没了
    assert_eq!(enter.try_recv().unwrap(), empty, "那次巨大的粘贴被拒了");

    let (tx, mut third) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    state.paste(&huge);
    state.key(Key::Char('y'));
    state.key(Key::Enter);
    let pasted = third.try_recv().unwrap().expect("粘贴被收下了");
    assert_eq!(pasted.chars().count(), 100_001, "全部，一整块");
}

#[test]
fn a_paste_is_ignored_while_a_question_is_up() {
    let (mut state, mut answer) = state_with_prompt();
    let huge = "x".repeat(100_001);
    // 粘贴不许回答一个问题，也不许覆盖它：超大粘贴会
    // 发问，而它问着的时候第二次粘贴什么都改不了（spec §9、§7）。
    state.paste(&huge);
    // 问题挂着的时候第二次粘贴必须什么都不改：不改问题，
    // 也不改它背后的草稿。
    state.paste("small");
    state.key(Key::Char('y'));
    state.key(Key::Enter);
    let pasted = answer
        .try_recv()
        .unwrap()
        .expect("这个问题活过了第二次粘贴");
    assert_eq!(pasted.chars().count(), 100_001, "它的文本也是");
    assert!(!pasted.contains("small"), "那次小粘贴没有到达草稿");
}

#[test]
fn a_tab_in_a_paste_becomes_spaces_rather_than_breaking_the_column_count() {
    let (mut state, mut answer) = state_with_prompt();
    state.paste("fn main() {\n\tbody\n}");
    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().unwrap(),
        Some("fn main() {\n    body\n}".to_owned())
    );
}

#[test]
fn esc_asks_before_it_throws_away_a_multi_line_draft() {
    let (mut state, mut answer) = state_with_prompt();
    state.paste("第一行\n第二行");
    state.key(Key::Esc);
    state.key(Key::Char('n'));
    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().unwrap(),
        Some("第一行\n第二行".to_owned()),
        "草稿活下来了"
    );

    // 单行仍然当场清掉：没什么可丢的，而紧随其后的那次
    // Enter 提交的是它留下的空草稿 —— 那是循环会丢掉的一行，
    // 不是关掉的输入。
    let (tx, mut second) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply: tx });
    for ch in "one line".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Esc);
    state.key(Key::Enter);
    assert_eq!(
        second.try_recv().unwrap(),
        Some(String::new()),
        "不问就清掉了"
    );
}

#[test]
fn submitting_trims_the_ends_and_keeps_the_lines_between_them() {
    let (mut state, mut answer) = state_with_prompt();
    state.paste("\n  第一行\n\n第二行  \n");
    state.key(Key::Enter);
    assert_eq!(
        answer.try_recv().unwrap(),
        Some("第一行\n\n第二行".to_owned())
    );
}

#[test]
fn escape_while_working_is_the_cancel_gesture_even_with_a_question_up() {
    // 键位表的顺序就是 spec 的顺序：一个进行中的回合让 `Esc` 成为取消
    // 手势，而一个待答的问题改不了这一点（spec §6）。这个问题
    // 等它自己的某个键 —— `Ctrl-C` 是另一条出路。
    let mut state = state_running();
    let (tx, mut asked) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        request: PermissionRequest {
            escalation: None,
            speaker: None,
            request_id: "r-1".to_owned(),
            tool_call_id: "c-1".to_owned(),
            tool_name: "write_file".to_owned(),
            args: serde_json::json!({}),
            reason: "mode ask".to_owned(),
        },
        reply: tx,
    }));

    state.key(Key::Esc);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
    assert!(asked.try_recv().is_err(), "这个问题仍然在等它自己的键");
}

#[test]
fn a_cancelled_run_takes_its_unanswered_question_with_it() {
    // 这条测试钉住的 bug：一次运行进行中的时候 `Esc` 是取消手势，
    // 不是答案，所以运行留着的问题从没被作答 —— 而循环那次询问随抬起它的
    // 那次运行一起死掉。前端不管怎样都留着覆盖层，于是下一次按键
    // 交给了**没人在等**的问题：它静默失败，
    // 读起来像个死键（spec §6、§9）。
    let mut state = state_running();
    let (tx, mut asked) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        request: PermissionRequest {
            escalation: None,
            speaker: None,
            request_id: "r-1".to_owned(),
            tool_call_id: "c-1".to_owned(),
            tool_name: "write_file".to_owned(),
            args: serde_json::json!({}),
            reason: "mode ask".to_owned(),
        },
        reply: tx,
    }));

    // 手势取消运行；它不回答那个问题。
    state.key(Key::Esc);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
    assert!(asked.try_recv().is_err(), "那个问题没被回答");

    // 运行结束，循环会说出来 —— 这正是让那个问题变陈旧的东西。
    state.request(ConsoleRequest::RunState { running: false });

    // 于是下一个键又是普通键：它编辑草稿，而不是去回答一个
    // 来自已经结束的运行的问句。
    let (reply, mut line) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Prompt { reply });
    state.key(Key::Char('x'));
    state.key(Key::Enter);
    assert_eq!(line.try_recv().unwrap(), Some("x".to_owned()));
    assert!(asked.try_recv().is_err(), "运行已结束的问题没有谁来回答");
}

#[test]
fn a_second_question_does_not_displace_the_one_on_screen() {
    // 循环一次问一个问题然后等，所以第二次询问意味着哪里
    // 出了错；让用户看得见的那个问题保持可作答，是安全的读法。
    let mut state = new_state();
    let (first_tx, mut first) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        request: permission_request("write_file"),
        reply: first_tx,
    }));
    let (second_tx, mut second) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        request: permission_request("edit_file"),
        reply: second_tx,
    }));
    assert!(second.try_recv().is_err(), "迟到的那个问题被丢掉了");

    state.key(Key::Char('n'));
    assert_eq!(first.try_recv().unwrap(), Answer::Deny);
}

#[test]
fn a_sandbox_block_is_one_dim_narration_line() {
    let lines = render_block_uncoloured(&Block::Sandbox {
        mode: "bwrap".to_owned(),
        unavailable_reason: None,
    });
    assert_eq!(lines.len(), 1, "一行：{lines:#?}");
    let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text, "[沙箱：bwrap]");
    assert_eq!(lines[0].spans[0].style.fg, Some(Color::DarkGray));
}

// ---------------------------------------------------------------------------
// 终端标题：状态词、目标名与「变了才写」（`.scratch/terminal-title/spec.md` §2、§5）
// ---------------------------------------------------------------------------

#[test]
fn the_title_reads_the_working_directory_and_a_state_word() {
    let mut state = new_state();
    assert_eq!(state.title(), "~/code/fs-agent", "空闲没有状态词");

    state.request(ConsoleRequest::RunState { running: true });
    assert_eq!(state.title(), "~/code/fs-agent · 运行中");
}

#[test]
fn a_question_makes_the_title_say_it_is_waiting() {
    let mut state = new_state();
    let (tx, _rx) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        request: permission_request("write_file"),
        reply: tx,
    }));
    assert_eq!(state.title(), "~/code/fs-agent · 等你");
}

#[test]
fn a_replay_outranks_a_question_which_outranks_a_running_turn() {
    let mut state = new_state();
    state.request(ConsoleRequest::RunState { running: true });
    let (tx, _rx) = tokio::sync::oneshot::channel();
    state.request(ConsoleRequest::Ask(AskRequest {
        request: permission_request("bash"),
        reply: tx,
    }));
    assert_eq!(state.title(), "~/code/fs-agent · 等你", "等你压过运行中");

    // 重放要有东西可放才立得起来（空的 `events` 什么也不做）。
    state.request(ConsoleRequest::Replay {
        events: vec![Event::new(
            1,
            SpeakerId::System,
            EventPayload::SessionStarted {
                session_id: fs_agent::events::SessionId::new("01J8ZQ4K7M"),
                cwd: "/workspace".to_owned(),
                schema_version: 1,
            },
        )],
    });
    assert_eq!(state.title(), "~/code/fs-agent · 重放中", "重放压过等你");
}

#[test]
fn a_goal_in_the_stream_lands_in_the_title_and_leaves_when_it_ends() {
    let mut state = new_state();
    state.request(ConsoleRequest::RunState { running: true });
    state.apply(RenderEvent::Logged(Event::new(
        1,
        SpeakerId::System,
        EventPayload::GoalSelected {
            goal: "修文档索引".to_owned(),
        },
    )));
    assert_eq!(state.title(), "~/code/fs-agent · 运行中 · 修文档索引");

    // 目标停下或做完，名字就不该再挂在标题上 —— 标题说的是**正在推进**的那一件。
    state.apply(RenderEvent::Logged(Event::new(
        2,
        SpeakerId::System,
        EventPayload::GoalCompleted {
            goal: "修文档索引".to_owned(),
            summary: "收尾".to_owned(),
        },
    )));
    assert_eq!(state.title(), "~/code/fs-agent · 运行中");
}

#[test]
fn a_title_is_only_written_when_it_changes() {
    let mut state = new_state();
    assert!(state.sync_title().is_some(), "第一版总是给出去");
    assert!(state.sync_title().is_none(), "没变就不再写");

    state.request(ConsoleRequest::RunState { running: true });
    assert_eq!(
        state.sync_title().expect("状态变了就该给新标题"),
        "~/code/fs-agent · 运行中"
    );
    assert!(state.sync_title().is_none(), "再问一次还是没变");
}

#[test]
fn ctrl_z_asks_for_a_suspend_and_is_taken_once() {
    // 挂起是终端层手势：状态机只置位，真正的终端动作（交还、SIGTSTP、恢复重绘）由
    // `Tui::run` 做，所以这里能断言的只有「请求被举起」与「取走一次就没了」
    // （`.scratch/suspend-gesture/spec.md` §2、§3）。
    let mut idle = new_state();
    idle.key(Key::CtrlZ);
    assert!(idle.take_suspend_request(), "空闲时按一下就要挂起");
    assert!(!idle.take_suspend_request(), "取走之后就没了，不会连挂两次");

    let mut busy = state_running();
    busy.key(Key::CtrlZ);
    assert!(busy.take_suspend_request(), "回合跑着的时候也允许挂起");
    assert!(!busy.should_quit(), "挂起不是退出");
    assert!(busy.take_events().is_empty(), "它也不推任何手势给循环");

    // 问题占着底部输入区的时候也一样：挂起不归任何视图管。
    let (mut asked, _answer) = state_with_prompt();
    asked.key(Key::CtrlZ);
    assert!(asked.take_suspend_request(), "问卷立着也能挂起");

    // 别的键不会顺手举起它。
    let mut other = new_state();
    other.key(Key::Char('z'));
    assert!(!other.take_suspend_request(), "一个光秃秃的 `z` 是文字");
}

#[test]
fn a_suspend_rewrites_the_title_even_when_it_did_not_change() {
    // 交还终端时标题被 pop 回了用户原来那条，而 `last_title` 还记着挂起前那一版 ——
    // 所以恢复后必须**强制**重写一次，否则标题会停在用户那条上
    // （`.scratch/suspend-gesture/spec.md` §4）。
    let mut state = new_state();
    assert!(state.sync_title().is_some(), "第一版总是给出去");
    assert!(state.sync_title().is_none(), "没变就不再写");
    assert_eq!(state.retitle(), "~/code/fs-agent", "恢复后照旧写一遍");
    assert!(state.sync_title().is_none(), "写完之后比对重新成立");
}
