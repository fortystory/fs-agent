//! 取消的传播（spec §6，票 13）。
//!
//! 一个手势 —— 前端的 Esc —— 停下那个进行中的回合，
//! 并**向下**走到该回合派出的那些执行者。手势本身不是事件；
//! 可断言的契约看到的只有一次停止留下的形状：
//! `TurnEnded { Aborted }`、每条已经开始过的
//! `tool_call` 各一条合成结果，以及不留一条悬着的调用
//! 让后来的 `--continue` 去猜。
//!
//! 一切都走唯一那个组装接缝，配一个脚本化的假 provider，
//! 与这里其它端到端测试一样。

mod support;

use std::path::PathBuf;
use std::sync::Arc;

use fs_agent::config::SessionConfig;
use fs_agent::events::{
    pending_tool_calls, read_events, Event, EventPayload, SessionId, SpeakerId, StopReason, Usage,
};
use fs_agent::permissions::{Mode, Policy};
use fs_agent::provider::{FinishReason, StreamEvent, ToolSpec};
use fs_agent::render::{RenderSinks, Renderer};
use fs_agent::tools::{Effect, Registry, Tool, ToolContext, ToolError, ToolOutput};
use fs_agent::{assemble, AssemblyParts, Harness, SessionScaffold};
use support::{CaptureBuf, FakeProvider, Reply};
use tokio::sync::Notify;

struct Fixture {
    harness: Harness,
    provider: FakeProvider,
    stdout: CaptureBuf,
    stderr: CaptureBuf,
    log_path: PathBuf,
    cwd: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, config: SessionConfig) -> Fixture {
    fixture_with_tools(replies, config, fs_agent::tools::builtin(false)).await
}

async fn fixture_with_tools(
    replies: Vec<Reply>,
    config: SessionConfig,
    tools: Registry,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    let log_path = session.join("log.jsonl");
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();
    let provider = FakeProvider::new(replies);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider.clone()),
        speaker: kimi(),
        config,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        }),
        scaffold: SessionScaffold {
            cwd: cwd.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-cancel"),
            tools,
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness,
        provider,
        stdout,
        stderr,
        log_path,
        cwd,
        _dir: dir,
    }
}

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

/// 一个只要一次工具调用、此后再也不要的模型回合。
fn calls(id: &str, name: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: args.to_string(),
        },
        StreamEvent::Usage(Usage::default()),
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

/// 一个永远不会自己返回的工具。
///
/// 「取消一个进行中的工具」用的器具：注册表是在唯一那个组装接缝上
/// 注入的一个值，所以测试工具与内建工具是从同一处挂上去的；
/// 工具会宣告自己进场，于是测试在一个确定的时刻按下，
/// 而不是靠睡等。
struct StallingTool {
    started: Arc<Notify>,
}

#[async_trait::async_trait]
impl Tool for StallingTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "stall".to_owned(),
            description: "Never returns.".to_owned(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }
    }

    fn effect(&self, _args: &serde_json::Value) -> Effect {
        Effect::ReadOnly
    }

    async fn call(
        &self,
        _ctx: &ToolContext<'_>,
        _args: serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        self.started.notify_one();
        futures::future::pending::<()>().await;
        unreachable!("停住的工具永远不会自己返回")
    }
}

fn kinds(events: &[Event]) -> Vec<&'static str> {
    events.iter().map(|event| event.payload.kind()).collect()
}

#[tokio::test]
async fn a_cancel_stops_an_in_flight_provider_stream_without_entering_the_log() {
    let opened = Arc::new(Notify::new());
    let mut fixture = fixture(
        vec![Reply::Stall(
            opened.clone(),
            vec![StreamEvent::TextDelta("half a thought".into())],
        )],
        SessionConfig::new("fake-model"),
    )
    .await;

    let cancel = fixture.harness.cancel_signal();
    // 回合的 future 借用了 harness，所以整个手势活在一个块里：
    // 它必须在 harness 关掉之前被丢掉。
    let outcome = {
        let turn = fixture.harness.run_turn("think out loud");
        tokio::pin!(turn);
        // 在流真的进行中的时候取消，不是它开之前、
        // 也不是它结束之后。
        tokio::select! {
            _ = opened.notified() => {}
            outcome = &mut turn => panic!("取消之前回合就结束了：{outcome:?}"),
        }
        cancel.cancel();
        turn.await.unwrap()
    };
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Aborted);
    assert_eq!(fixture.provider.requests().len(), 1);

    // 手势不在流上：一次取消留下的恰好是骨架、
    // 用户的问题、它停下的那个回合，以及它的中止。
    let events = read_events(&fixture.log_path).unwrap();
    assert_eq!(
        kinds(&events),
        vec![
            "SessionStarted",
            "MessageCompleted",
            "TurnStarted",
            "TurnEnded",
        ],
        "{events:#?}"
    );
    let EventPayload::TurnEnded { reason } = events.last().unwrap().payload else {
        unreachable!("最后一条事件就是回合结束");
    };
    assert_eq!(reason, StopReason::Aborted);

    // 一条从未走到 `[DONE]` 的流没有产出完成单元，所以
    // 不会有半截答案落进日志。
    assert!(
        !events.iter().any(
            |event| matches!(event.payload, EventPayload::MessageCompleted { .. })
                && event.speaker_id == kimi()
                && matches!(
                    event.payload,
                    EventPayload::MessageCompleted {
                        role: fs_agent::events::Role::Assistant,
                        ..
                    }
                )
        ),
        "被中止的那条流的半截文本不许被记下来"
    );
    assert!(pending_tool_calls(&events).is_empty());
    // 什么都没完成，所以没有东西到达最终产物那个 sink；
    // 停止的叙述改去诊断 sink。
    assert_eq!(fixture.stdout.text(), "");
    assert!(
        fixture.stderr.text().contains("被取消了"),
        "{}",
        fixture.stderr.text()
    );
}

#[tokio::test]
async fn a_cancel_during_a_tool_call_gives_that_call_its_one_result() {
    let started = Arc::new(Notify::new());
    let mut tools = Registry::new();
    tools.register(Box::new(StallingTool {
        started: started.clone(),
    }));
    let mut fixture = fixture_with_tools(
        vec![calls("call-1", "stall", serde_json::json!({}))],
        SessionConfig::new("fake-model"),
        tools,
    )
    .await;

    let cancel = fixture.harness.cancel_signal();
    let outcome = {
        let turn = fixture.harness.run_turn("stall for a while");
        tokio::pin!(turn);
        tokio::select! {
            _ = started.notified() => {}
            outcome = &mut turn => panic!("取消之前回合就结束了：{outcome:?}"),
        }
        cancel.cancel();
        turn.await.unwrap()
    };
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Aborted);
    let events = read_events(&fixture.log_path).unwrap();

    // 这条调用已经开始过了，所以它欠着恰好一条结果 —— spec §3 的
    // 第五条例外路径，也是手势落在工具中途时那条不变量
    // 仍然成立的原因。
    let completed: Vec<&Event> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ToolCallCompleted { .. }))
        .collect();
    assert_eq!(completed.len(), 1, "{events:#?}");
    let EventPayload::ToolCallCompleted {
        tool_call_id,
        ok,
        output,
        error,
        ..
    } = &completed[0].payload
    else {
        unreachable!("筛的是 ToolCallCompleted")
    };
    assert_eq!(tool_call_id.as_str(), "call-1");
    assert!(!ok);
    assert!(output.is_none());
    let error = error.as_deref().unwrap_or_default();
    assert!(error.contains("被取消了"), "{error:?}");
    assert!(pending_tool_calls(&events).is_empty());

    let endings: Vec<StopReason> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::TurnEnded { reason } => Some(*reason),
            _ => None,
        })
        .collect();
    assert_eq!(endings, vec![StopReason::Aborted]);
}

#[tokio::test]
async fn a_cancel_closes_a_deferred_task_call_with_the_result_it_owes() {
    let started = Arc::new(Notify::new());
    let mut tools = fs_agent::tools::builtin(false);
    tools.register(Box::new(StallingTool {
        started: started.clone(),
    }));
    let mut fixture = fixture_with_tools(
        vec![Reply::Stream(vec![
            StreamEvent::ToolCallCompleted {
                index: 0,
                id: "call-task".to_owned(),
                name: "task".to_owned(),
                arguments: serde_json::json!({"brief": "do the thing"}).to_string(),
            },
            StreamEvent::ToolCallCompleted {
                index: 1,
                id: "call-stall".to_owned(),
                name: "stall".to_owned(),
                arguments: "{}".to_owned(),
            },
            StreamEvent::Finished {
                finish_reason: FinishReason::ToolCalls,
            },
        ])],
        SessionConfig::new("fake-model"),
        tools,
    )
    .await;

    let cancel = fixture.harness.cancel_signal();
    let outcome = {
        let turn = fixture.harness.run_turn("delegate, then stall");
        tokio::pin!(turn);
        tokio::select! {
            _ = started.notified() => {}
            outcome = &mut turn => panic!("取消之前回合就结束了：{outcome:?}"),
        }
        cancel.cancel();
        turn.await.unwrap()
    };
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Aborted);
    let events = read_events(&fixture.log_path).unwrap();

    /// 一次调用得到的那条唯一结果，作为 `(ok, text)`。
    fn result_of(events: &[Event], want: &str) -> (bool, String) {
        let found: Vec<&Event> = events
            .iter()
            .filter(|event| {
                matches!(
                    &event.payload,
                    EventPayload::ToolCallCompleted { tool_call_id, .. }
                        if tool_call_id.as_str() == want
                )
            })
            .collect();
        assert_eq!(found.len(), 1, "{want} 恰好该有一条结果");
        let EventPayload::ToolCallCompleted {
            ok, output, error, ..
        } = &found[0].payload
        else {
            unreachable!("筛的是 ToolCallCompleted")
        };
        (
            *ok,
            output.clone().or_else(|| error.clone()).unwrap_or_default(),
        )
    }

    // `task` 那条调用被记成已开始、然后被延后，所以它欠着
    // 恰好一条结果，尽管它的执行者根本没跑过。
    let (task_ok, task_text) = result_of(&events, "call-task");
    assert!(!task_ok);
    assert!(task_text.contains("工具没有跑"), "{task_text:?}");

    // 真正进行中的那条调用说的是另一回事：工具的 future
    // 被丢掉了，所以工作区可能变了、也可能没变。
    let (stall_ok, stall_text) = result_of(&events, "call-stall");
    assert!(!stall_ok);
    assert!(
        stall_text.contains("进行中时回合被取消了"),
        "{stall_text:?}"
    );

    // 被延后的那个执行者从没被派出过……
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::ExecutorSpawned { .. })),
        "{events:#?}"
    );
    assert!(pending_tool_calls(&events).is_empty());
    // ……而停止发生在另一次模型调用之前。
    assert_eq!(fixture.provider.requests().len(), 1);
}

// ---------------------------------------------------------------------------
// 一场讨论：手势向下走到执行者，从不向上。
// ---------------------------------------------------------------------------

struct DiscussionFixture {
    harness: fs_agent::DiscussionHarness,
    kimi: FakeProvider,
    deepseek: FakeProvider,
    synthesizer: FakeProvider,
    stdout: CaptureBuf,
    stderr: CaptureBuf,
    log_path: PathBuf,
    _dir: tempfile::TempDir,
}

async fn discussion_fixture(
    kimi_replies: Vec<Reply>,
    deepseek_replies: Vec<Reply>,
    synthesizer_replies: Vec<Reply>,
) -> DiscussionFixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    let log_path = session.join("log.jsonl");
    let stdout = CaptureBuf::default();
    let stderr = CaptureBuf::default();
    let kimi_provider = FakeProvider::new(kimi_replies);
    let deepseek_provider = FakeProvider::new(deepseek_replies);
    let synthesizer_provider = FakeProvider::new(synthesizer_replies);

    let harness = fs_agent::assemble_discussion(fs_agent::DiscussionParts {
        scaffold: SessionScaffold {
            cwd,
            log_path: log_path.clone(),
            session_id: SessionId::new("s-cancel-discussion"),
            tools: fs_agent::tools::builtin(false),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
        debaters: vec![
            fs_agent::DebaterParts {
                speaker: kimi(),
                config: SessionConfig::new("fake-model"),
                provider: Box::new(kimi_provider.clone()),
                soul: None,
            },
            fs_agent::DebaterParts {
                speaker: SpeakerId::Debater("deepseek".into()),
                config: SessionConfig::new("fake-model"),
                provider: Box::new(deepseek_provider.clone()),
                soul: None,
            },
        ],
        synthesizer: fs_agent::SynthesizerParts {
            config: SessionConfig::new("fake-model"),
            provider: Box::new(synthesizer_provider.clone()),
        },
        max_rounds: Some(2),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(stdout.clone()),
            stderr_diagnostic: Box::new(stderr.clone()),
        }),
    })
    .await
    .unwrap();

    DiscussionFixture {
        harness,
        kimi: kimi_provider,
        deepseek: deepseek_provider,
        synthesizer: synthesizer_provider,
        stdout,
        stderr,
        log_path,
        _dir: dir,
    }
}

/// 讨论者写下的一个答案。
fn answered(body: &str, conclusion: &str) -> Reply {
    Reply::text(&format!("{body}\nCONCLUSION: {conclusion}"))
}

/// 每一条 `RoundEnded` 的 `(round, reason)`，按顺序。
fn round_endings(events: &[Event]) -> Vec<(u32, StopReason)> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::RoundEnded { round, reason } => Some((*round, *reason)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_cancel_reaches_a_running_executor_and_the_discussion_is_not_an_error() {
    let opened = Arc::new(Notify::new());
    // 名册顺序在这里要紧，而它就是 `join_all` 轮询的顺序：
    // 第一个讨论者正常作答，整个回合在第一次轮询里就走完，
    // 于是手势到来时，进行中的只剩另一侧那个停住的
    // 执行者这一件事。
    let mut fixture = discussion_fixture(
        vec![answered("KIMI 正文", "先做甲")],
        vec![
            // 派发者要一个执行者……
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "do the thing"}),
            ),
            // ……而执行者自己的那条流才是手势打断的东西。
            Reply::Stall(
                opened.clone(),
                vec![StreamEvent::TextDelta("working on it".into())],
            ),
        ],
        // 合成器从没被调用过。
        vec![],
    )
    .await;

    let signal = fixture.harness.cancel_signal();
    let outcome = {
        let discuss = fixture.harness.discuss("这件事该怎么做？");
        tokio::pin!(discuss);
        tokio::select! {
            _ = opened.notified() => {}
            outcome = &mut discuss => panic!("取消之前讨论就结束了：{outcome:?}"),
        }
        signal.cancel();
        discuss.await.unwrap()
    };
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Aborted);
    assert_eq!(outcome.synthesis, "");
    // 手势停下这一轮；它不会开启那一次收尾调用。
    assert_eq!(fixture.kimi.requests().len(), 1);
    assert_eq!(fixture.deepseek.requests().len(), 2);
    assert_eq!(fixture.synthesizer.requests().len(), 0);

    let events = read_events(&fixture.log_path).unwrap();

    // 走完全程的那一侧确实作答了：这一轮是被停下的，
    // 而不只是没完成。
    assert!(
        events.iter().any(|event| {
            event.speaker_id == kimi()
                && matches!(
                    &event.payload,
                    EventPayload::MessageCompleted {
                        role: fs_agent::events::Role::Assistant,
                        ..
                    }
                )
        }),
        "{events:#?}"
    );

    // 执行者是慢慢收尾，而不是被丢掉：它拿到一条终点，
    // 而它说的是 `Aborted` —— 不是 `Error`（spec §6）。
    let finished: Vec<(String, StopReason)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ExecutorFinished {
                executor_id,
                reason,
                ..
            } => Some((executor_id.to_string(), *reason)),
            _ => None,
        })
        .collect();
    assert_eq!(
        finished,
        vec![("deepseek-1".to_owned(), StopReason::Aborted)]
    );

    // `task` 那条调用已经开始了，所以它恰好保有一条结果，而
    // 派发者把这次取消读成一条普通的失败工具结果。
    assert!(pending_tool_calls(&events).is_empty());
    let results: Vec<&EventPayload> = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::ToolCallCompleted { .. }))
        .map(|event| &event.payload)
        .collect();
    assert_eq!(results.len(), 1, "{events:#?}");
    let EventPayload::ToolCallCompleted { ok, error, .. } = results[0] else {
        unreachable!("筛的是 ToolCallCompleted")
    };
    assert!(!ok);
    let error = error.as_deref().unwrap_or_default();
    assert!(error.contains("Aborted"), "{error:?}");

    // 被取消的一轮不是一次讨论结果，而那个被停下的讨论者
    // 也没有变成「缺席被读成共识」。
    assert_eq!(
        round_endings(&events),
        vec![(1, StopReason::Aborted)],
        "{events:#?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::SessionError { .. })),
        "被取消的讨论不是一次会话失败"
    );
    assert!(
        !events.iter().any(
            |event| matches!(event.payload, EventPayload::RoundStarted { .. })
                && matches!(
                    event.payload,
                    EventPayload::RoundStarted {
                        mode: fs_agent::events::RoundMode::Synthesis,
                        ..
                    }
                )
        ),
        "合成器从不开场"
    );

    // 什么都没完成，所以最终产物那个 sink 保持空的；停止的
    // 叙述改去诊断 sink。
    assert_eq!(fixture.stdout.text(), "");
    assert!(
        fixture.stderr.text().contains("被取消了"),
        "{}",
        fixture.stderr.text()
    );
}

#[tokio::test]
async fn a_cancel_that_stops_both_debaters_is_still_not_a_discussion_failure() {
    let kimi_opened = Arc::new(Notify::new());
    let deepseek_opened = Arc::new(Notify::new());
    let mut fixture = discussion_fixture(
        vec![Reply::Stall(
            kimi_opened.clone(),
            vec![StreamEvent::TextDelta("kimi half an answer".into())],
        )],
        vec![Reply::Stall(
            deepseek_opened.clone(),
            vec![StreamEvent::TextDelta("deepseek half an answer".into())],
        )],
        vec![],
    )
    .await;

    let signal = fixture.harness.cancel_signal();
    let outcome = {
        let discuss = fixture.harness.discuss("这件事该怎么做？");
        tokio::pin!(discuss);
        tokio::select! {
            _ = async {
                tokio::join!(kimi_opened.notified(), deepseek_opened.notified());
            } => {}
            outcome = &mut discuss => panic!("取消之前讨论就结束了：{outcome:?}"),
        }
        signal.cancel();
        discuss.await.unwrap()
    };
    fixture.harness.shutdown().await;

    // 没有人作答，而这一轮仍然不是「没人作答」在轮次单纯
    // 失败时会是的那种 Error：停下它的是
    // 那个手势（spec §6）。
    assert_eq!(outcome.reason, StopReason::Aborted);
    assert_eq!(outcome.rounds, 1);
    assert_eq!(outcome.synthesis, "");

    let events = read_events(&fixture.log_path).unwrap();
    assert_eq!(round_endings(&events), vec![(1, StopReason::Aborted)]);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::SessionError { .. })),
        "{events:#?}"
    );
    assert!(pending_tool_calls(&events).is_empty());
    assert_eq!(fixture.synthesizer.requests().len(), 0);
    assert_eq!(fixture.stdout.text(), "");
}

#[tokio::test]
async fn a_cancel_that_reaches_the_synthesizer_ends_the_discussion_without_a_product() {
    let opened = Arc::new(Notify::new());
    let mut fixture = discussion_fixture(
        vec![answered("KIMI 正文", "复用事件流")],
        vec![answered("DEEPSEEK 正文", "复用事件流")],
        // 收尾调用才是停住的那一次：讨论本身已经结束了。
        vec![Reply::Stall(
            opened.clone(),
            vec![StreamEvent::TextDelta("共识：复用".into())],
        )],
    )
    .await;

    let signal = fixture.harness.cancel_signal();
    let outcome = {
        let discuss = fixture.harness.discuss("要不要复用事件流？");
        tokio::pin!(discuss);
        tokio::select! {
            _ = opened.notified() => {}
            outcome = &mut discuss => panic!("取消之前讨论就结束了：{outcome:?}"),
        }
        signal.cancel();
        discuss.await.unwrap()
    };
    fixture.harness.shutdown().await;

    assert_eq!(outcome.reason, StopReason::Aborted);
    assert_eq!(outcome.synthesis, "");
    assert_eq!(fixture.synthesizer.requests().len(), 1);

    let events = read_events(&fixture.log_path).unwrap();
    // 辩论阶段确实是以 `NoDivergence` 收的尾；手势停下的
    // 是收尾那一轮。
    assert_eq!(
        round_endings(&events),
        vec![(1, StopReason::NoDivergence), (2, StopReason::Aborted)]
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.payload, EventPayload::SessionError { .. })),
        "被停下的合成器不是一次 `synthesis_failed`：{events:#?}"
    );
    // 半截产物从不落地，也没有东西到达最终产物那个 sink。
    assert!(
        !events.iter().any(
            |event| matches!(event.payload, EventPayload::MessageCompleted { .. })
                && event.speaker_id == SpeakerId::System
        ),
        "{events:#?}"
    );
    assert_eq!(fixture.stdout.text(), "");
}

#[tokio::test]
async fn a_killed_cancelled_session_resumes_and_closes_the_call_it_left_open() {
    let opened = Arc::new(Notify::new());
    let fixture = fixture(
        vec![
            calls(
                "call-1",
                "task",
                serde_json::json!({"brief": "do the thing"}),
            ),
            Reply::Stall(
                opened.clone(),
                vec![StreamEvent::TextDelta("working".into())],
            ),
        ],
        SessionConfig::new("fake-model"),
    )
    .await;
    let Fixture {
        mut harness,
        log_path,
        cwd,
        _dir,
        ..
    } = fixture;

    let signal = harness.cancel_signal();
    {
        let turn = harness.run_turn("delegate the thing");
        tokio::pin!(turn);
        tokio::select! {
            _ = opened.notified() => {}
            outcome = &mut turn => panic!("取消之前回合就结束了：{outcome:?}"),
        }
        signal.cancel();
        // 取消期间按第二次会把进程压下去：
        // 回合在半途被丢掉，什么都不会收尾（spec §6）。
    }
    harness.shutdown().await;

    // 被杀掉的进程留下的那条流里有一条没有结果的
    // `tool_call`。`--continue` 在同一个 id 下打开同一份日志，
    // 而收尾它的是恢复流程。
    let resumed = assemble(AssemblyParts {
        provider: Box::new(FakeProvider::new(vec![Reply::text("resumed")])),
        speaker: kimi(),
        config: SessionConfig::new("fake-model"),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd,
            log_path: log_path.clone(),
            session_id: SessionId::new("s-cancel"),
            tools: fs_agent::tools::builtin(false),
            locks: fs_agent::tools::PathLocks::new(),
            policy: Policy::for_mode(Mode::Auto),
            asker: None,
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .expect("带一条未闭合调用的流是可续的");
    assert_eq!(resumed.session_id().as_str(), "s-cancel");
    resumed.shutdown().await;

    let events = read_events(&log_path).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventPayload::SessionStarted { .. }))
            .count(),
        1,
        "续跑永不记下第二个会话头"
    );
    assert!(pending_tool_calls(&events).is_empty());
    let recovered = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::ToolCallCompleted {
                tool_call_id,
                ok: false,
                error: Some(error),
                ..
            } if tool_call_id.as_str() == "call-1" => Some(error.clone()),
            _ => None,
        })
        .expect("恢复收尾了进程死时那条调用");
    assert!(recovered.contains("会话被中断了"), "{recovered}");
}

#[tokio::test]
async fn a_gesture_is_scoped_to_one_run_so_a_cancelled_session_stays_usable() {
    let mut fixture = fixture(
        vec![
            Reply::text("a fresh answer"),
            Reply::text("a second answer"),
        ],
        SessionConfig::new("fake-model"),
    )
    .await;

    let cancel = fixture.harness.cancel_signal();
    assert!(!cancel.is_cancelled());
    // 在什么都没跑的时候抬起：没有活可停……
    cancel.cancel();
    assert!(cancel.is_cancelled());

    // ……而下一个回合不会继承它。少了这一条，一次 Esc 就会
    // 把同一进程里此后每个问题的会话都卡死。
    let outcome = fixture.harness.run_turn("do something").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(fixture.provider.requests().len(), 1);
    assert!(!cancel.is_cancelled(), "一次运行从一个干净的手势开始");

    let outcome = fixture.harness.run_turn("and another thing").await.unwrap();
    assert_eq!(outcome.reason, StopReason::Completed);
    assert_eq!(fixture.provider.requests().len(), 2);
    fixture.harness.shutdown().await;
}
