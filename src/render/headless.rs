//! headless 渲染器：机器模式（spec §19）。
//!
//! 它的纯粹性是结构性的 —— 它只往两个显式的写出口写。`stdout_result` 只收最终产物，
//! 别的什么都不收：要么是单 agent 会话里那个完成的回合，要么是讨论里合成器那条带
//! `System` 归属的产物（spec §15）。每个讨论者的回合也都以 `Completed` 收尾，所以
//! 「最后一个完成的回合」会把两个讨论者的作答都放上 stdout；把两者分开的是轮次边界。
//!
//! 执行者的回合永远不是会话的最终产物（spec §16、§19）：派发者自己的回合还在它周围
//! 敞着，所以执行者完成的回合既不能印到 `stdout`，也不能覆盖派发者的回合将要印出的
//! 文本。

use std::io::Write;

use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::events::{EventPayload, Role, SpeakerId, StopReason};

use super::transcript::summarize_args;
use super::wording::{self, speaker_label};
use super::{DeltaKind, Render, RenderEvent, RenderSinks};

/// 一个发言者是不是执行者。
fn is_executor(speaker: &SpeakerId) -> bool {
    matches!(speaker, SpeakerId::Executor(_))
}

/// 机器渲染器。
pub struct Headless {
    sinks: RenderSinks,
}

impl Headless {
    pub fn new(sinks: RenderSinks) -> Self {
        Self { sinks }
    }
}

#[async_trait]
impl Render for Headless {
    async fn consume(self: Box<Self>, mut receiver: broadcast::Receiver<RenderEvent>) {
        let Headless { mut sinks } = *self;
        let mut final_text = String::new();
        let mut in_reasoning = false;
        // 一场讨论的轮次有没有敞着。在一轮里面，一个完成的回合是某位讨论者的作答，而不
        // 是会话的最终产物：每个讨论者的回合也都以 `Completed` 收尾，所以「最后一个完成
        // 的回合」会把两个讨论者的作答都放上 stdout（spec §15）。
        let mut in_round = false;

        loop {
            match receiver.recv().await {
                // 拼好的系统提示词：机器模式不养转录，也没有人读它。
                Ok(RenderEvent::Identity { .. }) => {}
                Ok(RenderEvent::Delta {
                    kind: DeltaKind::Text,
                    text,
                    ..
                }) => {
                    if in_reasoning {
                        let _ = sinks.stderr_diagnostic.write_all(b"\n");
                        in_reasoning = false;
                    }
                    let _ = sinks.stderr_diagnostic.write_all(text.as_bytes());
                }
                Ok(RenderEvent::Delta {
                    kind: DeltaKind::Reasoning,
                    text,
                    ..
                }) => {
                    if !in_reasoning {
                        let _ = sinks
                            .stderr_diagnostic
                            .write_all(wording::reasoning_marker().as_bytes());
                        in_reasoning = true;
                    }
                    let _ = sinks.stderr_diagnostic.write_all(text.as_bytes());
                }
                Ok(RenderEvent::Diagnostic { message, .. }) => {
                    if in_reasoning {
                        let _ = sinks.stderr_diagnostic.write_all(b"\n");
                        in_reasoning = false;
                    }
                    let _ = writeln!(sinks.stderr_diagnostic, "{}", wording::diagnostic(&message));
                    let _ = sinks.stderr_diagnostic.flush();
                }
                Ok(RenderEvent::Notice { message, .. }) => {
                    if in_reasoning {
                        let _ = sinks.stderr_diagnostic.write_all(b"\n");
                        in_reasoning = false;
                    }
                    let _ = writeln!(sinks.stderr_diagnostic, "{message}");
                    let _ = sinks.stderr_diagnostic.flush();
                }
                // 机器模式没有文件索引要养：这条静默信号在这里什么都不是。
                Ok(RenderEvent::WorkspaceChanged) => {}
                Ok(RenderEvent::Logged(event)) => {
                    if in_reasoning {
                        let _ = sinks.stderr_diagnostic.write_all(b"\n");
                        in_reasoning = false;
                    }
                    // 进度叙述是从事件本身推出来的 —— 机器模式养着自己这一套事件形状的
                    // 叙述，而不是走共享的 `Block` 呈现类型 —— 但每一句话都来自措辞层。
                    let speaker = &event.speaker_id;
                    match &event.payload {
                        EventPayload::SessionStarted { .. } => {}
                        // 归属只说这个会话在为谁干活，没有给人看的进展可报 —— 清单与进度在
                        // 交互式那一侧；这里与上下文注入同一档，什么都不说。
                        EventPayload::GoalSelected { .. } => {}
                        // 汇总已经作为一条助手消息叙述过了（写它的那次单发调用记的）；这一条
                        // 是流上的记录，不再多说一遍。
                        EventPayload::GoalCompleted { .. } => {}
                        // 停下并报告由循环说出来，这里不重复。
                        EventPayload::GoalStopped { .. } => {}
                        // 与上下文注入同一档：一行诊断（模型上下文里没有它）。
                        EventPayload::SandboxStatus {
                            mode,
                            unavailable_reason,
                        } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{}",
                                wording::sandbox(mode, unavailable_reason.as_deref())
                            );
                        }
                        EventPayload::TurnStarted { iteration, .. } => {
                            if !is_executor(speaker) {
                                final_text.clear();
                            }
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "\n{} {}",
                                speaker_label(speaker),
                                wording::turn_started(*iteration)
                            );
                        }
                        EventPayload::MessageCompleted {
                            role: Role::Assistant,
                            text,
                            ..
                        } => {
                            // 执行者的回合是干活，不是会话的产物：把它的文本挡在
                            // `final_text` 外面，就是不让它上 stdout，也顺带不让执行者的
                            // 回合清掉（或覆盖）它正在为之干活的那个回合。
                            if !is_executor(speaker) {
                                final_text = text.clone();
                            }
                            // harness 自己那条完成消息就是讨论的最终产物 —— 合成器的
                            // 选项空间。它是唯一不需要回合就属于 stdout 的东西：合成器
                            // 没有回合（spec §15）。
                            if *speaker == SpeakerId::System {
                                let _ = sinks.stdout_result.write_all(text.as_bytes());
                                let _ = sinks.stdout_result.write_all(b"\n");
                                let _ = sinks.stdout_result.flush();
                            }
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "\n{} {}",
                                speaker_label(speaker),
                                wording::message_complete()
                            );
                        }
                        EventPayload::MessageCompleted { .. } => {}
                        EventPayload::TurnEnded { reason } => {
                            if *reason == StopReason::Completed
                                && !in_round
                                && !is_executor(speaker)
                            {
                                let _ = sinks.stdout_result.write_all(final_text.as_bytes());
                                let _ = sinks.stdout_result.write_all(b"\n");
                                let _ = sinks.stdout_result.flush();
                            }
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{}",
                                wording::turn_ended(*reason)
                            );
                        }
                        // 一条轮次边界连着它的编号与写明白的理由一起叙述：那四个终止原因
                        // （`NoDivergence` / `Consensus` / `RoundsExhausted` /
                        // `BudgetExhausted`）对读终端的人来说必须分得开，而这正是有四
                        // 个的全部意义（spec §15）。
                        EventPayload::RoundStarted { round, mode } => {
                            in_round = true;
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "\n{}",
                                wording::round_section(*round, *mode)
                            );
                        }
                        EventPayload::RoundEnded { round, reason } => {
                            in_round = false;
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{}",
                                wording::round_ended(*round, *reason)
                            );
                        }
                        EventPayload::ContextInjected { source, .. } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{}",
                                wording::context_injected(source.clone())
                            );
                        }
                        EventPayload::DivergenceRecorded { topic, .. } => {
                            let _ =
                                writeln!(sinks.stderr_diagnostic, "{}", wording::divergence(topic));
                        }
                        EventPayload::ToolCallStarted {
                            tool_name, args, ..
                        } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{} {}",
                                speaker_label(speaker),
                                wording::tool_call(tool_name, &summarize_args(args))
                            );
                        }
                        EventPayload::ToolCallCompleted { ok, error, .. } => {
                            let detail = if *ok {
                                wording::tool_completed().to_owned()
                            } else {
                                wording::tool_error(
                                    error.as_deref().unwrap_or(wording::no_message()),
                                )
                            };
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{} {detail}",
                                speaker_label(speaker)
                            );
                        }
                        EventPayload::UsageRecorded { usage } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{} {}",
                                speaker_label(speaker),
                                wording::usage_summary(usage)
                            );
                        }
                        EventPayload::PermissionAsked { request, .. } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{} {}",
                                speaker_label(speaker),
                                wording::permission_asked(
                                    crate::events::permission_format::tool_name(request),
                                    &super::transcript::summarize_permission_target(request)
                                )
                            );
                        }
                        EventPayload::PermissionDecided {
                            decision,
                            source,
                            reason,
                            ..
                        } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{} {}",
                                speaker_label(speaker),
                                wording::permission_decided(*decision, *source, reason.as_deref())
                            );
                        }
                        EventPayload::HookExecuted { point, outcome, .. } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{} {}",
                                speaker_label(speaker),
                                wording::hook(point, outcome)
                            );
                        }
                        EventPayload::ExecutorSpawned { executor_id, .. } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{} {}",
                                speaker_label(speaker),
                                wording::executor_spawned(executor_id.as_str())
                            );
                        }
                        EventPayload::ExecutorFinished {
                            executor_id,
                            reason,
                            summary,
                        } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{}",
                                wording::executor_finished(executor_id.as_str(), *reason, summary)
                            );
                        }
                        EventPayload::AgentError { message, .. } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{} {}",
                                speaker_label(speaker),
                                wording::agent_error(message)
                            );
                        }
                        EventPayload::SessionError { code, detail } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{}",
                                wording::session_error(code, detail)
                            );
                        }
                        EventPayload::SessionEnded { reason } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{}",
                                wording::session_ended(*reason)
                            );
                        }
                        EventPayload::HistorySuperseded {
                            reason, summary, ..
                        } => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "{}",
                                wording::history(*reason, summary.as_deref())
                            );
                        }
                    }
                    let _ = sinks.stderr_diagnostic.flush();
                }
                Err(broadcast::error::RecvError::Lagged(dropped)) => {
                    let _ = writeln!(
                        sinks.stderr_diagnostic,
                        "{}",
                        wording::renderer_dropped(dropped)
                    );
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }

        let _ = sinks.stderr_diagnostic.flush();
        let _ = sinks.stdout_result.flush();
    }
}
