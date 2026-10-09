//! 渲染接缝上的 plain 渲染器：往注入的通道里喂事件，
//! 然后断言那两个 sink（spec §19，Testing Decisions 第 10 类）。
//!
//! 这里没有任何东西去驱动一个真回合：渲染器的契约是压在那个事件
//! 序列上的，而这些测试给的正好就是它。

mod support;

use heng::events::{Event, EventPayload, Role, SpeakerId, StopReason, ToolCallId, hook_format};
use heng::render::{PlainOptions, RenderHandle, RenderSinks, Renderer, channel};
use support::CaptureBuf;

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

/// 在一个注入的通道上起 plain 渲染器，返回它的把手以及
/// 那两个被捕获的 sink。
fn renderer(
    color: bool,
) -> (
    RenderHandle,
    CaptureBuf,
    CaptureBuf,
    tokio::task::JoinHandle<()>,
) {
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();
    let (handle, receiver) = channel();
    let task = Renderer::plain(PlainOptions {
        sinks: RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        },
        color,
    })
    .spawn(receiver);
    (handle, stdout, stderr, task)
}

async fn run(events: &[Event], color: bool) -> (CaptureBuf, CaptureBuf) {
    let (handle, stdout, stderr, task) = renderer(color);
    for event in events {
        handle.logged(event);
    }
    drop(handle);
    task.await.unwrap();
    (stdout, stderr)
}

#[tokio::test]
async fn every_streamed_line_carries_the_speaker_prefix() {
    // 多 agent 的转录是交错的，所以一个块只出现一次的前缀
    // 会让它读不下去（spec §19）。
    let (handle, _stdout, stderr, task) = renderer(false);
    handle.text_delta(&kimi(), "first line\nsecond ");
    handle.text_delta(&kimi(), "line continues");
    drop(handle);
    task.await.unwrap();

    let text = stderr.text();
    assert!(text.contains("[kimi] first line"), "{text:?}");
    assert!(text.contains("[kimi] second line continues"), "{text:?}");
}

#[tokio::test]
async fn a_notice_reaches_the_diagnostic_sink_verbatim() {
    // 启动横幅与交互式循环的反馈都经过渲染器，而不是
    // 直接写终端：一旦渲染器占住了终端，第二个写者
    // 就落进它那块活区域里了（spec §19、§A.12）。
    let (handle, stdout, stderr, task) = renderer(false);
    handle.notice("heng: session abc · model m · mode ask · /tmp/x");
    drop(handle);
    task.await.unwrap();

    assert_eq!(stdout.text(), "");
    assert_eq!(
        stderr.text(),
        "heng: session abc · model m · mode ask · /tmp/x\n"
    );
}

#[tokio::test]
async fn a_message_without_deltas_is_still_shown() {
    // 一个不走流式的 provider，或者合成器那一整条消息，
    // 背后都没有增量 —— 完成的那条消息是唯一一份。
    let events = [Event::new(
        1,
        kimi(),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: "hello\nthere".to_owned(),
            reasoning: None,

            first_token_ms: None,
        },
    )];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    assert!(text.contains("[kimi] hello"), "{text:?}");
    assert!(text.contains("[kimi] there"), "{text:?}");
}

#[tokio::test]
async fn rounds_are_sectioned_and_divergences_are_indented() {
    let events = [
        Event::new(
            1,
            SpeakerId::System,
            EventPayload::RoundStarted {
                round: 2,
                mode: heng::events::RoundMode::Targeted,
            },
        ),
        Event::new(
            2,
            SpeakerId::System,
            EventPayload::DivergenceRecorded {
                round: 2,
                topic: "the seam".to_owned(),
                positions: vec!["trace it".to_owned(), "map it".to_owned()],
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    // 断言的是语义而不是文本：一轮以一条带着它编号的分节行开场，
    // 而且没有任何 debug 格式的枚举到达界面。确切的字句
    // 归文案层自己的测试。
    assert!(
        text.lines()
            .any(|line| line.starts_with("── ") && line.contains('2') && line.ends_with("──")),
        "一条轮次分节行：{text:?}"
    );
    assert!(!text.contains("Targeted"), "没有 debug 枚举：{text:?}");
    assert!(
        text.lines()
            .any(|line| line.starts_with("!! ") && line.contains("the seam")),
        "带主题的分歧标题：{text:?}"
    );
    assert!(
        text.contains("  - trace it") && text.contains("  - map it"),
        "各方立场是缩进的：{text:?}"
    );
}

#[tokio::test]
async fn a_tool_result_and_its_post_hook_read_as_one_block() {
    // 钩子事件不带 `tool_call_id`，所以哪一行归哪条反馈，靠的是那次调用
    // 自己。这次调用由它的**结果**来画，不为钩子一直开着：
    // 一直开着会让这次调用在整个工具运行期间都看不见
    // （2026-09-23，票 02 §3）。读者看到的顺序没变 —— 头部、结果、
    // 反馈 —— 变的只是写下它们的时刻提前了。
    let id = ToolCallId::new("call-1");
    let events = [
        Event::new(
            1,
            kimi(),
            EventPayload::ToolCallStarted {
                tool_call_id: id.clone(),
                tool_name: "read_file".to_owned(),
                args: serde_json::json!({"path": "src/lib.rs"}),
            },
        ),
        Event::new(
            2,
            kimi(),
            EventPayload::ToolCallCompleted {
                tool_call_id: id.clone(),
                ok: true,
                output: Some("fn main() {}".to_owned()),
                error: None,
                duration_ms: 3,
            },
        ),
        Event::new(
            3,
            kimi(),
            EventPayload::HookExecuted {
                point: hook_format::POINT_POST.to_owned(),
                command: "check".to_owned(),
                outcome: hook_format::feedback("looks fine").to_owned(),
            },
        ),
        Event::new(
            4,
            kimi(),
            EventPayload::TurnEnded {
                reason: StopReason::Completed,
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    // 断的是语义：头部、结果与后置钩子的反馈按这个顺序出现在
    // 同一个块里。确切的字句是文案层的测试。
    let head = text.find("read_file(path=src/lib.rs)").expect("工具头部");
    let output = text.find("  fn main() {}").expect("工具输出");
    let hook = text.find("feedback: looks fine").expect("钩子反馈");
    assert!(head < output && output < hook, "{text:?}");
}

#[tokio::test]
async fn a_tool_call_is_printed_when_its_result_lands() {
    // 结果来画这次调用，不需要再来一个事件：plain 过去会一直押着
    // 这次调用，直到某个不相干的东西到了，而尾巴上那次工具调用
    // 因此直到流结束才走到页面上（票 02 §3）。下面这几个事件
    // 就是喂进去的全部 —— 没有收尾的回合结束 —— 所以仍然等一个的
    // 画法连头部都印不出来。
    let id = ToolCallId::new("call-early");
    let events = [
        Event::new(
            1,
            kimi(),
            EventPayload::ToolCallStarted {
                tool_call_id: id.clone(),
                tool_name: "bash".to_owned(),
                args: serde_json::json!({"command": "true"}),
            },
        ),
        Event::new(
            2,
            kimi(),
            EventPayload::ToolCallCompleted {
                tool_call_id: id,
                ok: true,
                output: Some("done".to_owned()),
                error: None,
                duration_ms: 1,
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    let head = text.find("bash(command=true)").expect("那条调用行");
    let output = text.find("  done").expect("它的输出");
    assert!(head < output, "头部然后输出：{text:?}");
}

/// 只有后置钩子那条反馈行，同一次画里没有调用的块。
#[tokio::test]
async fn a_post_hook_prints_under_the_call_it_annotates() {
    // 反馈现在是它自己的 `Block::ToolFeedback`，瞄着刚画过的那次调用，
    // 所以 plain 必须把它渲染成它一向那样的同一行缩进行 —— 而且
    // 它绝不能把钩子印两遍。
    let id = ToolCallId::new("call-feedback");
    let events = [
        Event::new(
            1,
            kimi(),
            EventPayload::ToolCallStarted {
                tool_call_id: id.clone(),
                tool_name: "bash".to_owned(),
                args: serde_json::json!({"command": "true"}),
            },
        ),
        Event::new(
            2,
            kimi(),
            EventPayload::ToolCallCompleted {
                tool_call_id: id,
                ok: true,
                output: Some("done".to_owned()),
                error: None,
                duration_ms: 1,
            },
        ),
        Event::new(
            3,
            kimi(),
            EventPayload::HookExecuted {
                point: hook_format::POINT_POST.to_owned(),
                command: "check".to_owned(),
                outcome: hook_format::feedback("looks fine").to_owned(),
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    assert_eq!(
        text.matches("feedback: looks fine").count(),
        1,
        "这条反馈正好印一次：{text:?}"
    );
    let output = text.find("  done").expect("那条输出");
    let hook = text.find("feedback: looks fine").expect("那条反馈");
    assert!(output < hook, "反馈跟在结果后面：{text:?}");
}

#[tokio::test]
async fn the_post_hook_is_not_printed_as_a_separate_event() {
    // 前置钩子占自己那一行；后置钩子被合进它注解的那次调用，
    // 而且不能再单独占一行出现。
    let id = ToolCallId::new("call-2");
    let events = [
        Event::new(
            1,
            kimi(),
            EventPayload::ToolCallStarted {
                tool_call_id: id.clone(),
                tool_name: "bash".to_owned(),
                args: serde_json::json!({"command": "true"}),
            },
        ),
        Event::new(
            2,
            kimi(),
            EventPayload::ToolCallCompleted {
                tool_call_id: id,
                ok: true,
                output: Some(String::new()),
                error: None,
                duration_ms: 1,
            },
        ),
        Event::new(
            3,
            kimi(),
            EventPayload::HookExecuted {
                point: hook_format::POINT_POST.to_owned(),
                command: "check".to_owned(),
                outcome: hook_format::feedback("ok").to_owned(),
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    // 反馈正好出现一次（合进了它那次调用），而没有任何
    // debug 格式的挂载点到达界面。
    assert_eq!(text.matches("feedback: ok").count(), 1, "{text:?}");
    assert!(!text.contains("post_tool_use"), "{text:?}");
}

#[tokio::test]
async fn the_synthesizers_message_is_the_only_thing_on_stdout() {
    let events = [
        Event::new(
            1,
            SpeakerId::Debater("kimi".into()),
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text: "a debater's answer".to_owned(),
                reasoning: None,

                first_token_ms: None,
            },
        ),
        Event::new(
            2,
            SpeakerId::System,
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text: "the option space".to_owned(),
                reasoning: None,

                first_token_ms: None,
            },
        ),
    ];
    let (stdout, _stderr) = run(&events, false).await;
    assert_eq!(stdout.text(), "the option space\n");
}

#[tokio::test]
async fn completed_and_aborted_do_not_render_the_same() {
    // 用户故事 135：跑完的一次运行与撞了墙的一次运行，
    // 必须一眼分得出来。
    let events = [
        Event::new(
            1,
            kimi(),
            EventPayload::TurnEnded {
                reason: StopReason::Completed,
            },
        ),
        Event::new(
            2,
            kimi(),
            EventPayload::TurnEnded {
                reason: StopReason::Aborted,
            },
        ),
        Event::new(
            3,
            kimi(),
            EventPayload::TurnEnded {
                reason: StopReason::Error,
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, true).await;
    let text = stderr.text();
    // 完成的回合是绿的，中断是黄的，错误是红的。这里断言的是
    // 严重程度那个颜色与归属，而不是那句话：
    // 文案有自己的精确文本测试。
    assert!(
        text.lines().any(|line| line.starts_with("\x1b[32m[kimi] ")),
        "好的收尾读起来是绿的：{text:?}"
    );
    assert!(
        text.lines().any(|line| line.starts_with("\x1b[33m[kimi] ")),
        "警告性的收尾读起来是黄的：{text:?}"
    );
    assert!(
        text.lines().any(|line| line.starts_with("\x1b[31m[kimi] ")),
        "错误的收尾读起来是红的：{text:?}"
    );
}

#[tokio::test]
async fn a_permission_decision_between_start_and_result_does_not_split_the_call() {
    // 循环为**每一次**调用都记一条 `PermissionDecided`，问过没问过
    // 都一样，而前置钩子的 `HookExecuted` 也会在 `ToolCallStarted` 与
    // 结果之间打一枪。两者都不能关掉那个开着的块，否则每一次
    // 活着经过的工具调用都会被渲染成一次无结果的调用再加一个合成的 `?`。
    let id = ToolCallId::new("call-4");
    let events = [
        Event::new(
            1,
            kimi(),
            EventPayload::ToolCallStarted {
                tool_call_id: id.clone(),
                tool_name: "write_file".to_owned(),
                args: serde_json::json!({"path": "a.txt"}),
            },
        ),
        Event::new(
            2,
            kimi(),
            EventPayload::HookExecuted {
                point: hook_format::POINT_PRE.to_owned(),
                command: "check".to_owned(),
                outcome: hook_format::OUTCOME_CONTINUE.to_owned(),
            },
        ),
        Event::new(
            3,
            kimi(),
            EventPayload::PermissionAsked {
                request_id: "r-1".to_owned(),
                tool_call_id: id.clone(),
                request: serde_json::json!({}),
            },
        ),
        Event::new(
            4,
            kimi(),
            EventPayload::PermissionDecided {
                request_id: "r-1".to_owned(),
                decision: heng::events::Decision::Allow,
                source: heng::events::DecisionSource::Policy,
                reason: Some("mode auto".to_owned()),
            },
        ),
        Event::new(
            5,
            kimi(),
            EventPayload::ToolCallCompleted {
                tool_call_id: id,
                ok: true,
                output: Some("wrote a.txt".to_owned()),
                error: None,
                duration_ms: 2,
            },
        ),
        Event::new(
            6,
            kimi(),
            EventPayload::TurnEnded {
                reason: StopReason::Completed,
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    assert!(
        text.contains("write_file(path=a.txt)"),
        "这次调用保住了它的参数：{text:?}"
    );
    assert!(text.contains("  wrote a.txt"), "以及它的结果：{text:?}");
    assert!(
        !text.contains("→ ?("),
        "这次完成绝不能变成第二个、不知名姓的块：{text:?}"
    );
    // 裁决仍然被叙述，只是不作为块的边界：这一行
    // 归属于那个发言者，并且原样带着权限门自己的理由。
    assert!(
        text.lines()
            .any(|line| line.starts_with("[kimi] ") && line.contains("mode auto")),
        "{text:?}"
    );
}

#[tokio::test]
async fn a_call_with_no_result_still_appears_when_the_stream_ends() {
    // 一次取消会留下一条没有结果的 `ToolCallStarted`；这个块在
    // 流结束时被冲刷出去，而不是被丢掉（spec §19）。
    let id = ToolCallId::new("call-3");
    let events = [Event::new(
        1,
        kimi(),
        EventPayload::ToolCallStarted {
            tool_call_id: id,
            tool_name: "bash".to_owned(),
            args: serde_json::json!({"command": "sleep 300"}),
        },
    )];
    let (_stdout, stderr) = run(&events, false).await;
    // 是冲刷出去而不是丢掉：这次调用仍然到了转录上。
    assert!(stderr.text().contains("sleep 300"), "{:?}", stderr.text());
}

#[tokio::test]
async fn a_permission_question_names_the_tool_and_the_call() {
    // 这个问句行过去只带 id，所以放行一次调用的那个人
    // 看不出它要跑什么。现在它会点名工具与具体的参数；
    // id 留在事件流里，不进人读的东西。
    let events = [Event::new(
        1,
        kimi(),
        EventPayload::PermissionAsked {
            request_id: "perm-1".to_owned(),
            tool_call_id: ToolCallId::new("call-5"),
            request: heng::events::permission_format::request(
                "write_file",
                &serde_json::json!({"file_path": "a.txt"}),
                "mode ask",
            ),
        },
    )];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    assert!(
        text.lines().any(|line| line.starts_with("[kimi] ")
            && line.contains("write_file")
            && line.contains("file_path=a.txt")),
        "这个问句点出了工具与那次调用：{text:?}"
    );
    assert!(!text.contains("perm-1"), "请求 id 没有露面：{text:?}");
    assert!(!text.contains("call-5"), "调用 id 没有露面：{text:?}");
}

#[tokio::test]
async fn the_sandbox_state_gets_one_narration_line_like_a_context_injection() {
    // 沙箱状态与上下文注入同一档：转录里一行、不给任何发言者说话。模型上下文里没有它
    // （log-only），但人在对话里看得见（沙箱 spec §8）。
    let events = [
        Event::new(
            1,
            SpeakerId::System,
            EventPayload::ContextInjected {
                source: heng::events::ContextSource::AgentsMd,
                content: "规矩".to_owned(),
            },
        ),
        Event::new(
            2,
            SpeakerId::System,
            EventPayload::SandboxStatus {
                mode: "bwrap".to_owned(),
                unavailable_reason: None,
            },
        ),
        Event::new(
            3,
            SpeakerId::System,
            EventPayload::SandboxStatus {
                mode: "bwrap".to_owned(),
                unavailable_reason: Some("PATH 上没有 `bwrap`".to_owned()),
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    assert!(text.contains("[上下文注入：AGENTS.md]"), "{text}");
    assert!(text.contains("[沙箱：bwrap]"), "{text}");
    assert!(
        text.contains("[沙箱：bwrap · 不可用：PATH 上没有 `bwrap`]"),
        "不可用时把原因一起摊开：{text}"
    );
}

#[tokio::test]
async fn an_answer_to_a_questionnaire_reads_as_the_user_speaking() {
    // 问卷的答案在这一侧同样是**用户说的**那一档（`.scratch/ui-trim/spec.md`）：同一个块，
    // 同一个 `[用户]` 前缀，逐行读下来就是他答的那句话。模型那一侧一个字节没变 —— 进上下文
    // 的仍是那次工具调用与它的结果。
    let events = [
        Event::new(
            1,
            kimi(),
            EventPayload::ToolCallStarted {
                tool_call_id: ToolCallId::new("call-1"),
                tool_name: "ask_user_question".to_owned(),
                args: serde_json::json!({
                    "questions": [
                        {
                            "id": "pick",
                            "header": "用哪个方案",
                            "question": "这两条路走哪一条？",
                            "options": [{"label": "A"}, {"label": "B"}]
                        }
                    ]
                }),
            },
        ),
        Event::new(
            2,
            kimi(),
            EventPayload::ToolCallCompleted {
                tool_call_id: ToolCallId::new("call-1"),
                ok: true,
                output: Some(
                    serde_json::json!({"answers": [{"id": "pick", "selected": ["A"]}]}).to_string(),
                ),
                error: None,
                duration_ms: 3,
            },
        ),
    ];
    let (_stdout, stderr) = run(&events, false).await;
    let text = stderr.text();
    assert!(
        text.contains("[用户] 用哪个方案：这两条路走哪一条？"),
        "题面完整摆出来：{text}"
    );
    assert!(text.contains("● A"), "选中的那一格在：{text}");
    assert!(text.contains("○ B"), "没选的那一格也在：{text}");
}

#[tokio::test]
async fn a_command_record_reaches_the_transcript_verbatim() {
    // 命令是手势：它不进模型上下文，而它在别处一个字都不留，所以这一行是读的人唯一能查
    // 「我刚才敲了什么」的地方（`.scratch/command-echo/spec.md`）。
    let events = [Event::new(
        1,
        SpeakerId::System,
        EventPayload::CommandRun {
            text: "/clear".to_owned(),
        },
    )];
    let (_stdout, stderr) = run(&events, false).await;
    assert_eq!(stderr.text(), "[命令] /clear\n");
}
