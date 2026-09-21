//! The cancel gesture's plumbing (spec §6).
//!
//! A cancellation is a **gesture**, not an event: raising it never enters the
//! stream, exactly like `/undo`'s write-back and `/plan`'s override. All this
//! module carries is the signal that a turn should stop where it stands — the
//! stream records only what the agents then did about it (`TurnEnded { Aborted }`,
//! the synthesized results, an executor's own `ExecutorFinished`).
//!
//! The two ends are deliberately different types. [`CancelSignal`] is held by
//! whoever owns the gesture — the front end — and is the only end that can raise
//! one. [`CancelObserver`] is what a turn is handed, and it is cloned down to
//! every executor that turn dispatches. So "cancellation propagates down the
//! delegation chain and never up it" (spec §6) is a property of the types rather
//! than a rule to remember: a turn holds nothing it could cancel with, so an
//! executor being stopped cannot end its dispatcher's turn as a failure.
//!
//! A gesture is scoped to **one run**: the harness resets the signal when a turn
//! (or a discussion) begins, so a press that arrived while nothing was running
//! stops nothing, and the session stays usable for the next question.

use tokio::sync::watch;

/// The gesture's own end: the one handle that can raise a cancellation.
///
/// It lives at the front end, which also decides what a **second** press means
/// (spec §6: it forces the process down); this type only records that the first
/// one happened.
#[derive(Debug, Clone)]
pub struct CancelSignal {
    cancelled: watch::Sender<bool>,
}

/// A turn's view of the gesture: read-only, and cheap to clone into every nested
/// turn.
#[derive(Debug, Clone)]
pub struct CancelObserver {
    cancelled: watch::Receiver<bool>,
}

impl CancelSignal {
    /// A signal with no observers yet. Each turn mints its own with
    /// [`observer`](Self::observer).
    pub fn new() -> Self {
        Self {
            cancelled: watch::channel(false).0,
        }
    }

    /// Raise the gesture. Idempotent, and effective for the rest of the run even
    /// if it is raised in a window with no observer alive. [`reset`](Self::reset)
    /// is what ends its effect, when the next run begins.
    pub fn cancel(&self) {
        // `send_replace`, not `send`: `send` is a no-op when nobody is watching,
        // which would silently swallow a gesture raised in such a window.
        self.cancelled.send_replace(true);
    }

    /// Start a fresh gesture, dropping one that was raised earlier.
    ///
    /// The harness calls this when a run begins, which is what keeps a gesture
    /// scoped to one run: a press that arrived while nothing was running — or
    /// the gesture of a turn that has already ended — cannot stop a later one.
    /// Only the signal end can do this; a running turn holds an observer.
    pub fn reset(&self) {
        self.cancelled.send_replace(false);
    }

    /// Whether a cancellation has been raised. The front end reads this to tell
    /// the first press (cancel) from the second (exit).
    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }

    /// A view for one turn. Minted per turn rather than shared, so each turn
    /// owns the `&mut` its wait needs.
    pub fn observer(&self) -> CancelObserver {
        CancelObserver {
            cancelled: self.cancelled.subscribe(),
        }
    }
}

impl Default for CancelSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelObserver {
    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }

    /// Resolve when the gesture is raised.
    ///
    /// Resolves immediately when it already is. If the signal end is gone,
    /// nobody can raise the gesture any more, so this wait parks forever — which
    /// is what keeps "the sender was dropped" from being read as "cancelled".
    pub async fn cancelled(&mut self) {
        if self.is_cancelled() {
            return;
        }
        loop {
            match self.cancelled.changed().await {
                Ok(()) => {
                    if self.is_cancelled() {
                        return;
                    }
                }
                Err(_) => std::future::pending::<()>().await,
            }
        }
    }
}
