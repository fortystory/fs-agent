//! The plain renderer: the human transcript for a pipe or a simple terminal.
//!
//! It paints the shared [`Block`]s (see [`super::transcript`]) as text, and adds
//! the two things a live transcript needs that a finished-stream query does not:
//! a speaker prefix repeated on **every** line, so interleaved debaters stay
//! readable, and a severity color on every stopping point, so `Completed` never
//! looks like `Aborted` or `Error` (spec §19).
//!
//! The final product still goes to `stdout` and the narration to `stderr`, the
//! same split the headless mode uses: `fs-agent --plain "q" 2>/dev/null` prints
//! the answer and nothing else, while a person at a terminal sees the whole
//! discussion.

use std::io::Write;

use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::events::{Role, SpeakerId};

use super::severity::Severity;
use super::transcript::{summarize_args, Block, ToolBlock, Transcript};
use super::wording::{self, speaker_label};
use super::{DeltaKind, Render, RenderEvent, RenderSinks};

/// How much of one tool result the plain transcript shows before eliding.
const TOOL_PREVIEW: usize = 2_000;

/// The plain renderer's values. `color` is decided by the front end, not read
/// from the environment here: the library never guesses at a terminal.
pub struct PlainOptions {
    pub sinks: RenderSinks,
    /// Whether to paint severity and reasoning with ANSI escapes.
    pub color: bool,
}

/// The plain renderer.
pub struct Plain {
    sinks: RenderSinks,
    color: bool,
    transcript: Transcript,
    /// The speaker whose text stream is open, so a speaker change starts a line.
    open_speaker: Option<SpeakerId>,
    /// Whether a new prefix is owed before the next character.
    at_line_start: bool,
    /// Whether the reasoning marker is open.
    in_reasoning: bool,
    /// Whether deltas already produced the current message's body.
    streamed: bool,
}

impl Plain {
    pub fn new(options: PlainOptions) -> Self {
        Self {
            sinks: options.sinks,
            color: options.color,
            transcript: Transcript::new(),
            open_speaker: None,
            at_line_start: true,
            in_reasoning: false,
            streamed: false,
        }
    }

    fn emit(&mut self, event: RenderEvent) {
        let blocks = self.transcript.push(event);
        for block in blocks {
            self.paint(block);
        }
    }

    /// Paint one block into the diagnostic sink (the final product is the one
    /// thing that also reaches stdout).
    fn paint(&mut self, block: Block) {
        match block {
            Block::Delta {
                speaker,
                kind,
                text,
            } => self.delta(speaker, kind, text),
            Block::Message {
                speaker,
                role,
                text,
            } => self.message(speaker, role, &text),
            Block::RoundStarted { round, mode } => {
                self.end_line();
                self.line(&format!("\n{}", wording::round_section(round, mode)));
            }
            Block::RoundEnded { round, reason } => {
                let text = wording::round_ended(round, reason);
                self.line(&self.paint_severity(Severity::of(reason), &text));
            }
            Block::Divergence { topic, positions } => {
                self.end_line();
                self.line(&format!("!! {}", wording::divergence(&topic)));
                for position in positions {
                    self.line(&format!("  - {position}"));
                }
            }
            Block::Tool(tool) => self.tool(&tool),
            Block::TurnStarted { speaker, iteration } => {
                self.end_line();
                self.line(&format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::turn_started(iteration)
                ));
            }
            Block::TurnEnded { speaker, reason } => {
                self.end_line();
                let text = format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::turn_ended(reason)
                );
                self.line(&self.paint_severity(Severity::of(reason), &text));
            }
            Block::PermissionAsked {
                speaker,
                tool_name,
                request_id,
                tool_call_id,
            } => {
                self.line(&format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::permission_asked(
                        tool_name.as_deref(),
                        &request_id,
                        tool_call_id.as_str()
                    )
                ));
            }
            Block::PermissionDecided {
                speaker,
                decision,
                source,
                reason,
            } => {
                self.line(&format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::permission_decided(decision, source, reason.as_deref())
                ));
            }
            Block::Hook {
                speaker,
                point,
                outcome,
            } => {
                self.line(&format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::hook(&point, &outcome)
                ));
            }
            Block::ExecutorSpawned {
                speaker,
                executor_id,
            } => {
                self.line(&format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::executor_spawned(executor_id.as_str())
                ));
            }
            Block::ExecutorFinished {
                executor_id,
                reason,
                summary,
            } => {
                let text = wording::executor_finished(executor_id.as_str(), reason, &summary);
                self.line(&self.paint_severity(Severity::of(reason), &text));
            }
            Block::Usage { speaker, usage } => {
                self.line(&format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::usage_summary(&usage)
                ));
            }
            Block::AgentError { speaker, message } => {
                let text = format!(
                    "{} {}",
                    speaker_label(&speaker),
                    wording::agent_error(&message)
                );
                self.line(&self.paint_severity(Severity::Bad, &text));
            }
            Block::SessionError { code, detail } => {
                let text = wording::session_error(&code, &detail);
                self.line(&self.paint_severity(Severity::Bad, &text));
            }
            Block::SessionEnded { reason } => {
                let text = wording::session_ended(reason);
                self.line(&self.paint_severity(Severity::of(reason), &text));
            }
            Block::ContextInjected { source } => {
                self.line(&wording::context_injected(source));
            }
            Block::History { reason, summary } => {
                self.line(&wording::history(reason, summary.as_deref()));
            }
            Block::Diagnostic(message) => {
                self.line(&wording::diagnostic(&message));
            }
            Block::Notice(message) => {
                self.line(&message);
            }
        }
        self.flush_sink();
    }

    /// Stream incremental text with the speaker prefix repeated per line.
    fn delta(&mut self, speaker: SpeakerId, kind: DeltaKind, text: String) {
        match kind {
            DeltaKind::Text => {
                self.close_reasoning();
                self.stream(&speaker, &text);
                self.streamed = true;
            }
            DeltaKind::Reasoning => {
                if !self.in_reasoning {
                    self.end_line();
                    let prefix = speaker_label(&speaker);
                    let marker =
                        self.paint_reasoning(&format!("{prefix} {} ", wording::reasoning_label()));
                    self.write(&marker);
                    self.open_speaker = Some(speaker.clone());
                    self.at_line_start = false;
                    self.in_reasoning = true;
                }
                self.write(&text);
                self.flush_sink();
            }
        }
    }

    fn message(&mut self, speaker: SpeakerId, role: Role, text: &str) {
        self.close_reasoning();
        // A provider that did not stream (or a synthesizer message assembled
        // whole) still has to be shown: print the body only if no delta did.
        if !self.streamed && !text.is_empty() {
            self.stream(&speaker, text);
        }
        self.streamed = false;
        self.end_line();
        if speaker == SpeakerId::System && role == Role::Assistant {
            self.final_product(text);
        }
    }

    fn tool(&mut self, tool: &ToolBlock) {
        self.end_line();
        let head = format!(
            "{} → {}",
            speaker_label(&tool.speaker),
            wording::tool_call(&tool.tool, &summarize_args(&tool.args))
        );
        self.line(&head);
        match &tool.outcome {
            Some(outcome) if outcome.ok => {
                if let Some(output) = &outcome.output {
                    if !output.trim().is_empty() {
                        self.line(&indent(
                            &wording::tool_output_preview(output, TOOL_PREVIEW),
                            2,
                        ));
                    }
                }
            }
            Some(outcome) => {
                let error = outcome
                    .error
                    .as_deref()
                    .unwrap_or_else(|| wording::no_message())
                    .to_owned();
                let text = indent(&wording::tool_output_preview(&error, TOOL_PREVIEW), 2);
                self.line(&self.paint_severity(Severity::Bad, &text));
            }
            None => self.line(&format!("  {}", wording::no_tool_result())),
        }
        if let Some(hook) = &tool.hook {
            self.line(&indent(&wording::hook_feedback(hook), 2));
        }
    }

    /// Write `text` with a fresh `[speaker]` prefix at the start of every line.
    fn stream(&mut self, speaker: &SpeakerId, text: &str) {
        if self.open_speaker.as_ref() != Some(speaker) && !self.at_line_start {
            self.write("\n");
            self.at_line_start = true;
        }
        self.open_speaker = Some(speaker.clone());
        for ch in text.chars() {
            if self.at_line_start {
                self.at_line_start = false;
                if ch != '\n' {
                    let prefix = format!("{} ", speaker_label(speaker));
                    self.write(&prefix);
                }
            }
            if ch == '\n' {
                self.at_line_start = true;
            }
            self.write(&ch.to_string());
        }
    }

    fn close_reasoning(&mut self) {
        if self.in_reasoning {
            self.write("\n");
            self.in_reasoning = false;
            self.at_line_start = true;
        }
    }

    /// Finish the open line, if any.
    fn end_line(&mut self) {
        self.close_reasoning();
        if !self.at_line_start {
            self.write("\n");
            self.at_line_start = true;
        }
    }

    /// A whole line of narration, ending the current one first.
    fn line(&mut self, text: &str) {
        self.end_line();
        self.write(text);
        self.write("\n");
        self.at_line_start = true;
    }

    /// The session's final product also goes to stdout, like the headless mode.
    fn final_product(&mut self, text: &str) {
        let _ = self.sinks.stdout_result.write_all(text.as_bytes());
        let _ = self.sinks.stdout_result.write_all(b"\n");
        let _ = self.sinks.stdout_result.flush();
    }

    fn write(&mut self, text: &str) {
        let _ = self.sinks.stderr_diagnostic.write_all(text.as_bytes());
    }

    fn flush_sink(&mut self) {
        let _ = self.sinks.stderr_diagnostic.flush();
    }

    fn paint_severity(&self, severity: Severity, text: &str) -> String {
        if !self.color {
            return text.to_owned();
        }
        format!("{}{}{}", severity.ansi(), text, Severity::ANSI_RESET)
    }

    fn paint_reasoning(&self, text: &str) -> String {
        if !self.color {
            return text.to_owned();
        }
        format!("\x1b[2m{text}\x1b[0m")
    }
}

#[async_trait]
impl Render for Plain {
    async fn consume(self: Box<Self>, mut receiver: broadcast::Receiver<RenderEvent>) {
        let mut plain = *self;
        loop {
            match receiver.recv().await {
                Ok(event) => plain.emit(event),
                Err(broadcast::error::RecvError::Lagged(dropped)) => {
                    plain.emit(RenderEvent::Diagnostic(wording::renderer_dropped(dropped)));
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
        // A call whose result never arrived (a cancel) is still shown.
        for block in plain.transcript.flush() {
            plain.paint(block);
        }
        plain.end_line();
        let _ = plain.sinks.stderr_diagnostic.flush();
        let _ = plain.sinks.stdout_result.flush();
    }
}

/// Indent every line of `text`.
fn indent(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    text.lines()
        .map(|line| format!("{pad}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
