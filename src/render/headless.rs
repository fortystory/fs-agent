//! The headless renderer: the machine mode (spec §19).
//!
//! Its purity is structural — it writes to exactly two explicit sinks.
//! `stdout_result` receives the final product and nothing else: either the
//! completed turn of a single-agent session, or, inside a discussion, the
//! synthesizer's `System`-attributed product (spec §15). Every debater turn also
//! ends `Completed`, so "the last completed turn" would put both debaters'
//! answers on stdout; a round boundary is what tells the two apart.
//!
//! An executor's turn is never the session's final product (spec §16, §19): the
//! dispatcher's own turn is still open around it, so an executor's completed turn
//! must neither print to `stdout` nor overwrite the text the dispatcher's turn
//! will print.

use std::io::Write;

use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::events::{EventPayload, Role, SpeakerId, StopReason};

use super::transcript::summarize_args;
use super::wording::{self, speaker_label};
use super::{DeltaKind, Render, RenderEvent, RenderSinks};

/// Whether a speaker is an executor.
fn is_executor(speaker: &SpeakerId) -> bool {
    matches!(speaker, SpeakerId::Executor(_))
}

/// The machine renderer.
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
        // Whether a discussion round is open. Inside one, a completed turn is one
        // debater's answer rather than the session's final product: every debater
        // turn also ends `Completed`, so "the last completed turn" would put both
        // debaters' answers on stdout (spec §15).
        let mut in_round = false;

        loop {
            match receiver.recv().await {
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
                Ok(RenderEvent::Diagnostic(message)) => {
                    if in_reasoning {
                        let _ = sinks.stderr_diagnostic.write_all(b"\n");
                        in_reasoning = false;
                    }
                    let _ = writeln!(sinks.stderr_diagnostic, "{}", wording::diagnostic(&message));
                    let _ = sinks.stderr_diagnostic.flush();
                }
                Ok(RenderEvent::Notice(message)) => {
                    if in_reasoning {
                        let _ = sinks.stderr_diagnostic.write_all(b"\n");
                        in_reasoning = false;
                    }
                    let _ = writeln!(sinks.stderr_diagnostic, "{message}");
                    let _ = sinks.stderr_diagnostic.flush();
                }
                Ok(RenderEvent::Logged(event)) => {
                    if in_reasoning {
                        let _ = sinks.stderr_diagnostic.write_all(b"\n");
                        in_reasoning = false;
                    }
                    // Progress narration is derived from the events themselves —
                    // the machine mode keeps its own event-shaped narration rather
                    // than going through the shared `Block` presentation type —
                    // but every phrase comes from the wording layer.
                    let speaker = &event.speaker_id;
                    match &event.payload {
                        EventPayload::SessionStarted { .. } => {}
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
                            // An executor's turn is work, not the session's
                            // product: keeping its text out of `final_text` is what
                            // stops it reaching stdout, and it stops the executor's
                            // turn from clearing (or overwriting) the turn it is
                            // working for.
                            if !is_executor(speaker) {
                                final_text = text.clone();
                            }
                            // The harness's own completed message is the
                            // discussion's final product — the synthesizer's option
                            // space. It is the one thing that belongs on stdout
                            // without a turn: the synthesizer has no turn (spec
                            // §15).
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
                        // A round boundary is narrated with its number and its
                        // reason spelled out: the four terminal reasons
                        // (`NoDivergence` / `Consensus` / `RoundsExhausted` /
                        // `BudgetExhausted`) have to stay distinguishable to the
                        // person reading the terminal, which is the whole point of
                        // having four of them (spec §15).
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
                                wording::context_injected(*source)
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
