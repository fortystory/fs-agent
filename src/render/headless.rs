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
                        let _ = sinks.stderr_diagnostic.write_all(b"[reasoning] ");
                        in_reasoning = true;
                    }
                    let _ = sinks.stderr_diagnostic.write_all(text.as_bytes());
                }
                Ok(RenderEvent::Diagnostic(message)) => {
                    if in_reasoning {
                        let _ = sinks.stderr_diagnostic.write_all(b"\n");
                        in_reasoning = false;
                    }
                    let _ = writeln!(sinks.stderr_diagnostic, "[diag] {message}");
                    let _ = sinks.stderr_diagnostic.flush();
                }
                Ok(RenderEvent::Logged(event)) => {
                    if in_reasoning {
                        let _ = sinks.stderr_diagnostic.write_all(b"\n");
                        in_reasoning = false;
                    }
                    match &event.payload {
                        EventPayload::TurnStarted { iteration, .. } => {
                            if !is_executor(&event.speaker_id) {
                                final_text.clear();
                            }
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "\n[{}] turn iteration {iteration}",
                                event.speaker_id
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
                            if !is_executor(&event.speaker_id) {
                                final_text = text.clone();
                            }
                            // The harness's own completed message is the
                            // discussion's final product — the synthesizer's option
                            // space. It is the one thing that belongs on stdout
                            // without a turn: the synthesizer has no turn (spec
                            // §15).
                            if event.speaker_id == SpeakerId::System {
                                let _ = sinks.stdout_result.write_all(text.as_bytes());
                                let _ = sinks.stdout_result.write_all(b"\n");
                                let _ = sinks.stdout_result.flush();
                            }
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "\n[{}] message complete",
                                event.speaker_id
                            );
                        }
                        EventPayload::TurnEnded { reason } => {
                            if *reason == StopReason::Completed
                                && !in_round
                                && !is_executor(&event.speaker_id)
                            {
                                let _ = sinks.stdout_result.write_all(final_text.as_bytes());
                                let _ = sinks.stdout_result.write_all(b"\n");
                                let _ = sinks.stdout_result.flush();
                            }
                            let _ = writeln!(sinks.stderr_diagnostic, "[turn ended: {reason}]");
                        }
                        // A round boundary is narrated with its number and its
                        // reason spelled out: the four terminal reasons
                        // (`NoDivergence` / `Consensus` / `RoundsExhausted` /
                        // `BudgetExhausted`) have to stay distinguishable to the
                        // person reading the terminal, which is the whole point of
                        // having four of them (spec §15).
                        EventPayload::RoundStarted { round, mode } => {
                            in_round = true;
                            let _ =
                                writeln!(sinks.stderr_diagnostic, "\n[round {round}: {mode:?}]");
                        }
                        EventPayload::RoundEnded { round, reason } => {
                            in_round = false;
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "[round {round} ended: {reason}]"
                            );
                        }
                        payload => {
                            let _ = writeln!(
                                sinks.stderr_diagnostic,
                                "[{}] {}",
                                event.speaker_id,
                                payload.kind()
                            );
                        }
                    }
                    let _ = sinks.stderr_diagnostic.flush();
                }
                Err(broadcast::error::RecvError::Lagged(dropped)) => {
                    let _ = writeln!(sinks.stderr_diagnostic, "[render] dropped {dropped} events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }

        let _ = sinks.stderr_diagnostic.flush();
        let _ = sinks.stdout_result.flush();
    }
}
