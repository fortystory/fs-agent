//! Rendering boundary.
//!
//! There is one renderer per process, chosen at startup (plain / TUI /
//! headless), and every renderer consumes the same broadcast channel. Incremental
//! text and logged events share that one channel so their relative order is
//! defined; incremental text never enters the event log.
//!
//! Ticket 01 ships the headless renderer only. Its purity is structural: it
//! writes to exactly two explicit sinks, and `stdout_result` receives only the
//! final product of a `TurnEnded { Completed }` turn.
//!
//! The `[speaker]` prefix below is the **human's** prefix. It is deliberately a
//! separate generator from the projection's model-side prefix (spec §5): this
//! one repeats on every line so an interleaved multi-agent log stays readable,
//! while the model's is written once per merged block.

use std::io::Write;

use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::events::{Event, EventPayload, Role, SpeakerId, StopReason};

/// How many render events may be buffered before a slow consumer starts losing
/// them. A lost delta degrades output, never correctness.
const RENDER_CHANNEL_CAPACITY: usize = 1024;

/// Text that bypasses the event log on its way to the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaKind {
    Text,
    Reasoning,
}

/// Anything a renderer may observe: incremental model output or a completed
/// unit that landed in the event log.
#[derive(Debug, Clone)]
pub enum RenderEvent {
    Delta {
        speaker: SpeakerId,
        kind: DeltaKind,
        text: String,
    },
    Logged(Event),
    /// Renderer-only narration that is not an event.
    Diagnostic(String),
}

/// The two explicit sinks of the headless renderer, injected at assembly time.
pub struct RenderSinks {
    /// Final products only.
    pub stdout_result: Box<dyn Write + Send>,
    /// Everything else: progress, diagnostics, event narration.
    pub stderr_diagnostic: Box<dyn Write + Send>,
}

/// Send side of the render channel. Cheap to clone.
#[derive(Clone)]
pub struct RenderHandle {
    sender: broadcast::Sender<RenderEvent>,
}

impl RenderHandle {
    pub fn text_delta(&self, speaker: &SpeakerId, text: &str) {
        let _ = self.sender.send(RenderEvent::Delta {
            speaker: speaker.clone(),
            kind: DeltaKind::Text,
            text: text.to_owned(),
        });
    }

    pub fn reasoning_delta(&self, speaker: &SpeakerId, text: &str) {
        let _ = self.sender.send(RenderEvent::Delta {
            speaker: speaker.clone(),
            kind: DeltaKind::Reasoning,
            text: text.to_owned(),
        });
    }

    pub fn logged(&self, event: &Event) {
        let _ = self.sender.send(RenderEvent::Logged(event.clone()));
    }

    /// A renderer-only diagnostic that is not an event.
    pub fn diagnostic(&self, message: &str) {
        let _ = self
            .sender
            .send(RenderEvent::Diagnostic(message.to_owned()));
    }
}

/// Spawn the headless renderer. The returned task ends when every
/// [`RenderHandle`] is dropped and the channel drains.
pub fn spawn_headless(sinks: RenderSinks) -> (RenderHandle, JoinHandle<()>) {
    let (sender, receiver) = broadcast::channel(RENDER_CHANNEL_CAPACITY);
    let task = tokio::spawn(run_headless(sinks, receiver));
    (RenderHandle { sender }, task)
}

async fn run_headless(mut sinks: RenderSinks, mut receiver: broadcast::Receiver<RenderEvent>) {
    let mut final_text = String::new();
    let mut in_reasoning = false;

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
                        final_text.clear();
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
                        final_text = text.clone();
                        let _ = writeln!(
                            sinks.stderr_diagnostic,
                            "\n[{}] message complete",
                            event.speaker_id
                        );
                    }
                    EventPayload::TurnEnded { reason } => {
                        if *reason == StopReason::Completed {
                            let _ = sinks.stdout_result.write_all(final_text.as_bytes());
                            let _ = sinks.stdout_result.write_all(b"\n");
                            let _ = sinks.stdout_result.flush();
                        }
                        let _ = writeln!(sinks.stderr_diagnostic, "[turn ended: {reason}]");
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
