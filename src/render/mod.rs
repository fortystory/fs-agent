//! Rendering boundary (spec §19).
//!
//! There is **one** renderer per process, chosen at startup, and the three
//! implementations are mutually exclusive — never concurrent subscribers. All
//! three consume the **same** broadcast channel, and that channel is created and
//! injected at assembly time ([`channel`] + [`Renderer::spawn`]). Incremental
//! text and logged events travel on that one channel so their relative order is
//! defined; incremental text never enters the event log.
//!
//! The three implementations:
//!
//! * [`headless`] — the machine mode. Its purity is structural: it writes to
//!   exactly two explicit sinks, and `stdout` receives the final product and
//!   nothing else (ticket 01's regression assertion).
//! * [`plain`] — the human transcript for a pipe or a simple terminal.
//! * [`tui`] — the ratatui interface: an inline viewport (no alternate screen, so
//!   the transcript stays scrollable) that owns the keyboard.
//!
//! Plain and TUI share one [`transcript`] layer: the same events become the same
//! [`transcript::Block`]s, and only the painting differs. That is what keeps this
//! from being three renderers each re-deriving the same presentation rules.
//!
//! The `[speaker]` prefix is the **human's** prefix. It is deliberately a
//! separate generator from the projection's model-side prefix (spec §5): this one
//! repeats on every line so an interleaved multi-agent log stays readable, while
//! the model's is written once per merged block.

pub mod headless;
pub mod highlight;
pub mod input;
pub mod plain;
pub mod severity;
pub mod transcript;
pub mod tui;

use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::events::{Event, SpeakerId};

pub use headless::Headless;
pub use input::{
    console, spawn_plain_console, AnswerChoice, AskRequest, ConsoleAsker, ConsoleEvents,
    ConsoleHandle, ConsolePort, ConsoleRequest, FrontEndEvent, Question,
};
pub use plain::{Plain, PlainOptions};
pub use severity::Severity;
pub use transcript::{Block, ToolBlock, ToolOutcome, Transcript};
pub use tui::{paint_scrollback, render_block, Key, Tui, TuiOptions, TuiState};

/// How many render events may be buffered before a slow consumer starts losing
/// them. A lost delta degrades output, never correctness.
pub const RENDER_CHANNEL_CAPACITY: usize = 1024;

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
    /// A front-end line that speaks for no event: the startup banner and the
    /// interactive loop's plain feedback.
    ///
    /// Not a [`RenderEvent::Diagnostic`]: a diagnostic is the system reporting
    /// something, and is labelled as such, while a notice is the line itself.
    Notice(String),
}

/// The two explicit sinks of the headless renderer, injected at assembly time.
pub struct RenderSinks {
    /// Final products only.
    pub stdout_result: Box<dyn std::io::Write + Send>,
    /// Everything else: progress, diagnostics, event narration.
    pub stderr_diagnostic: Box<dyn std::io::Write + Send>,
}

impl std::fmt::Debug for RenderSinks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RenderSinks")
    }
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

    /// A front-end line shown verbatim.
    ///
    /// The seam exists so that a caller holding a harness never has to print:
    /// once a renderer owns the terminal, a second writer lands inside the
    /// live region (spec §19).
    pub fn notice(&self, message: &str) {
        let _ = self.sender.send(RenderEvent::Notice(message.to_owned()));
    }
}

/// One consumer of the render channel.
///
/// The three implementations are started one at a time, never side by side: the
/// selection *is* [`Renderer`], and it is made once, at assembly. A renderer owns
/// whatever resources its mode needs (the machine mode's sinks, the TUI's
/// terminal) and returns when every [`RenderHandle`] has been dropped and the
/// channel has drained.
#[async_trait]
pub trait Render: Send {
    async fn consume(self: Box<Self>, receiver: broadcast::Receiver<RenderEvent>);
}

/// The startup renderer selection: exactly one of the three modes.
///
/// A value rather than a trait object because the choice has to be made where
/// the front end is assembled, and because "mutually exclusive" is then a
/// property of the type rather than a convention.
pub enum Renderer {
    /// The machine mode: two explicit sinks, `stdout` carries only the final
    /// product.
    Headless(RenderSinks),
    /// The human transcript for a pipe or a simple terminal.
    Plain(PlainOptions),
    /// The ratatui interface; it owns the keyboard.
    Tui(Box<TuiOptions>),
}

impl Renderer {
    pub fn headless(sinks: RenderSinks) -> Self {
        Renderer::Headless(sinks)
    }

    pub fn plain(options: PlainOptions) -> Self {
        Renderer::Plain(options)
    }

    pub fn tui(options: TuiOptions) -> Self {
        Renderer::Tui(Box::new(options))
    }

    /// Start the selected renderer on `receiver`, which the assembly created.
    pub fn spawn(self, receiver: broadcast::Receiver<RenderEvent>) -> JoinHandle<()> {
        match self {
            Renderer::Headless(sinks) => {
                let renderer = Box::new(Headless::new(sinks));
                tokio::spawn(async move { renderer.consume(receiver).await })
            }
            Renderer::Plain(options) => {
                let renderer = Box::new(Plain::new(options));
                tokio::spawn(async move { renderer.consume(receiver).await })
            }
            Renderer::Tui(options) => {
                let renderer = Box::new(Tui::new(*options));
                tokio::spawn(async move { renderer.consume(receiver).await })
            }
        }
    }
}

/// Create the one render channel. The assembly owns the send side (as a
/// [`RenderHandle`]); the consumer end is handed to exactly one [`Renderer`].
pub fn channel() -> (RenderHandle, broadcast::Receiver<RenderEvent>) {
    let (sender, receiver) = broadcast::channel(RENDER_CHANNEL_CAPACITY);
    (RenderHandle { sender }, receiver)
}
