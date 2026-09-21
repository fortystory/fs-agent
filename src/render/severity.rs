//! How a stop reason should read.
//!
//! `Completed` must not look like `Aborted` or `Error` (spec §19, user story
//! 135): a run that hit a wall and a run that finished are the two things a
//! person most needs to tell apart at a glance. The discussion's four terminal
//! reasons (`NoDivergence` / `Consensus` / `RoundsExhausted` / `BudgetExhausted`)
//! are likewise distinguishable rather than collapsed into "done".
//!
//! This module is the one place that classification lives; the plain renderer
//! turns it into ANSI codes and the TUI into a ratatui `Style`.

use crate::events::StopReason;

/// How prominently a reason should read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The loop did what it set out to do.
    Good,
    /// A normal end that is not a plain success (a consensus, an exhausted
    /// round budget).
    Note,
    /// The run stopped early for a reason that is not a failure (a cancel, a
    /// turn cap).
    Warn,
    /// Something went wrong, or a hard limit was hit.
    Bad,
}

impl Severity {
    /// Classify one reason.
    ///
    /// The grouping is deliberate: `Completed` / `Consensus` / `NoDivergence`
    /// are the ways a run *succeeds*; `Aborted` / `MaxIterations` /
    /// `RoundsExhausted` stopped early but were not errors; `Error` /
    /// `MistakeLimit` / `BudgetExhausted` are the reasons a person must not miss.
    pub fn of(reason: StopReason) -> Severity {
        match reason {
            StopReason::Completed | StopReason::Consensus | StopReason::NoDivergence => {
                Severity::Good
            }
            StopReason::RoundsExhausted => Severity::Note,
            StopReason::Aborted | StopReason::MaxIterations => Severity::Warn,
            StopReason::Error | StopReason::MistakeLimit | StopReason::BudgetExhausted => {
                Severity::Bad
            }
        }
    }

    /// A short, stable label, for a status line.
    pub fn label(self) -> &'static str {
        match self {
            Severity::Good => "ok",
            Severity::Note => "note",
            Severity::Warn => "warn",
            Severity::Bad => "error",
        }
    }

    /// The ANSI SGR prefix for this severity, or an empty string when color is
    /// off. `Good` is the terminal's default foreground; every other level is
    /// colored so the four never collapse into each other.
    pub fn ansi(self, color: bool) -> &'static str {
        if !color {
            return "";
        }
        match self {
            Severity::Good => "\x1b[32m",
            Severity::Note => "\x1b[36m",
            Severity::Warn => "\x1b[33m",
            Severity::Bad => "\x1b[31m",
        }
    }

    /// The ANSI reset that closes [`Severity::ansi`].
    pub fn ansi_reset(color: bool) -> &'static str {
        if color {
            "\x1b[0m"
        } else {
            ""
        }
    }
}
