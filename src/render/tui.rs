//! The ratatui interface (`.scratch/tui-sidebar/spec.md` §1–§2, ADR 0002).
//!
//! Three properties are structural, not stylistic:
//!
//! * **Alternate screen, one frame.** The TUI draws a fullscreen frame around a
//!   full-height sidebar and a main column: the sidebar holds the mark and the
//!   session's readings, the main column stacks the transcript (with the scrollbar
//!   and the rail at its right edge), the status row, the input and the hints. The
//!   transcript lives in its own buffer rather than in the terminal's scrollback —
//!   which is what removed the inline viewport's drifting cursor, since in
//!   fullscreen the pane origin is always `(0, 0)`.
//! * **The renderer owns the keyboard.** It is the only task reading terminal
//!   events, and it answers the loop's requests ([`ConsoleRequest`]) over the
//!   injected console channel. That is what keeps input and output from fighting.
//! * **`select!` over broadcast / keys.** Render events, the loop's requests and
//!   keyboard input are three independent sources; `select!` is how they are merged
//!   without a second channel whose ordering would be undefined. What is already
//!   queued is drained before the frame is drawn, so a bursting provider costs
//!   frames rather than events.

use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use ratatui::buffer::CellWidth;
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as CtEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block as WidgetBlock, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState,
};
use tokio::sync::broadcast;

use crate::events::{ContextSource, Event, HistoryReason, Role, StopReason, ToolCallId};
use crate::permissions::Mode;
use crate::questions::{UserAnswer, UserAnswers, UserQuestion};

use super::editor::{self, Input};
use super::input::{
    AnswerChoice, CatalogEntry, ConsolePort, ConsoleRequest, FrontEndEvent, Question,
};
use super::layout;
use super::pane::{self, Pane};
use super::panel::Panel;
use super::severity::Severity;
use super::transcript::{summarize_args, Block, ToolBlock, Transcript};
use super::width::{text_columns, truncate_columns};
use super::wording::{self, speaker_label};
use super::{DeltaKind, Render, RenderEvent};

/// How much streamed text is retained before it is trimmed to a tail. The
/// transcript does not need the whole message live: the completed `Message`
/// block re-renders it in full.
const LIVE_BUFFER: usize = 4_000;

/// How often the frame is redrawn even without an event.
///
/// It is the last remnant of the header's clock, which the shell no longer shows:
/// the arm that wakes on it puts nothing on screen any more (票 02 §8, confirmed
/// and removed in 票 05).
const TICK: Duration = Duration::from_millis(120);

/// A paste larger than this asks before it is taken (spec §7).
const PASTE_CONFIRM_CHARS: usize = 100_000;

/// How many queued render events one frame absorbs. A bounded drain keeps a
/// firehose from starving the keyboard for a whole frame's worth of work.
const DRAIN_LIMIT: usize = 4_096;

/// How many history events one replay batch applies.
///
/// Replaying a session is a frame-by-frame catch-up rather than a blocking load, so
/// each pass takes a bounded slice and draws. The event count alone is not enough of
/// a bound: a slice of 512 huge tool results would still be a slow frame, so the
/// batch also stops at [`REPLAY_BATCH_LINES`] source lines
/// (`.scratch/tui-history-replay/spec.md` §2).
const REPLAY_BATCH_EVENTS: usize = 512;

/// How many transcript source lines one replay batch may produce. The pane's cost
/// per line grows with its cap once the transcript is full, so the batch is
/// bounded by what it draws, not only by how many events it consumed.
const REPLAY_BATCH_LINES: usize = 2_000;

/// The keys the TUI acts on.
///
/// Deliberately its own vocabulary rather than crossterm's: the state machine is
/// then testable without a terminal, and a backend change cannot silently move a
/// binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    Delete,
    Enter,
    /// `Tab`, which is only ever the `/` menu's fill-in key (spec §6).
    Tab,
    Esc,
    BackTab,
    CtrlC,
    /// `Ctrl-D`: quit, behind a confirmation. Ignored while a run is in flight, so
    /// it is only ever the idle keyboard's gesture (票 06 §1).
    CtrlD,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// Emacs-style line editing: `Ctrl-A/E` move, `Ctrl-U/K/W` kill, `Ctrl-P/N`
    /// walk the prompt history.
    CtrlA,
    CtrlE,
    CtrlU,
    CtrlK,
    CtrlW,
    CtrlP,
    CtrlN,
    CtrlG,
    CtrlJ,
    PageUp,
    PageDown,
}

/// Translate one crossterm keypress into a [`Key`], or `None` for a key the TUI
/// ignores.
fn map_key(key: KeyEvent) -> Option<Key> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(ch) = key.code {
            return match ch.to_ascii_lowercase() {
                'c' => Some(Key::CtrlC),
                'd' => Some(Key::CtrlD),
                'a' => Some(Key::CtrlA),
                'e' => Some(Key::CtrlE),
                'u' => Some(Key::CtrlU),
                'k' => Some(Key::CtrlK),
                'w' => Some(Key::CtrlW),
                'p' => Some(Key::CtrlP),
                'n' => Some(Key::CtrlN),
                'g' => Some(Key::CtrlG),
                'j' => Some(Key::CtrlJ),
                _ => None,
            };
        }
    }
    match key.code {
        KeyCode::Esc => Some(Key::Esc),
        KeyCode::Tab => Some(Key::Tab),
        KeyCode::BackTab => Some(Key::BackTab),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Char(ch) => Some(Key::Char(ch)),
        KeyCode::Backspace => Some(Key::Backspace),
        KeyCode::Delete => Some(Key::Delete),
        KeyCode::Left => Some(Key::Left),
        KeyCode::Right => Some(Key::Right),
        KeyCode::Up => Some(Key::Up),
        KeyCode::Down => Some(Key::Down),
        KeyCode::Home => Some(Key::Home),
        KeyCode::End => Some(Key::End),
        KeyCode::PageUp => Some(Key::PageUp),
        KeyCode::PageDown => Some(Key::PageDown),
        _ => None,
    }
}

/// Which colour each speaker's name is drawn in (票 07 §1).
///
/// The palette and the roster are injected; the only thing kept here is the slot a
/// speaker first seen *after* assembly was given. A discussion can draw its pair at
/// assembly, but a `/discuss` typed mid-session names people the injection has never
/// heard of — they take the first unclaimed palette slot and keep it for the rest of
/// the session, so a name never changes colour under the reader.
///
/// The colour is the **painter's** business: [`wording::speaker_label`] stays plain
/// text, and nothing outside the transcript is tinted (票 07 §3, §4). Public only
/// because [`render_block`] takes it; the state machine owns how it is built.
pub struct SpeakerColors {
    /// The debaters in roster order: slot `n` of the palette belongs to `roster[n]`.
    roster: Vec<String>,
    /// The palette slots the roster did not claim, in palette order: the first unclaimed
    /// name seen during the session takes the first of these.
    free_slots: Vec<usize>,
    /// The names first seen during the session, in the order they appeared. This is the
    /// whole of the state: the colours themselves are derivable from it and the roster.
    extra: Vec<String>,
    /// No roster was injected, so there is nothing to colour and every name is grey.
    /// This is the plain half of the shared rendering, and it must stay neutral: an
    /// empty roster is not a session with one anonymous debater, it is a caller with no
    /// palette at all.
    uncoloured: bool,
}

/// The debaters' palette, in the order the roster hands the slots out (票 07 §1).
const DEBATER_PALETTE: [Color; 2] = [Color::LightCyan, Color::LightMagenta];

impl SpeakerColors {
    /// The palette for a roster: the debaters in the order their slots go out. A
    /// caller with no roster passes an empty one, and every name is then grey.
    pub fn new(roster: &[String]) -> Self {
        let roster = roster.to_vec();
        // The roster claims palette slots by position: the `n`-th debater gets slot
        // `n`. With more debaters than colours the extra slots wrap, which is why the
        // claim is `slot < len` and not the whole roster.
        let claimed: Vec<usize> = (0..roster.len().min(DEBATER_PALETTE.len())).collect();
        let free_slots = (0..DEBATER_PALETTE.len())
            .filter(|slot| !claimed.contains(slot))
            .collect();
        Self {
            uncoloured: roster.is_empty(),
            roster,
            free_slots,
            extra: Vec::new(),
        }
    }

    /// The colour for one speaker's name.
    ///
    /// The palette is fixed at assembly, so a session's name-to-colour map is stable
    /// for as long as it runs — including for a name that first appears mid-session
    /// (票 07 §1).
    fn of(&mut self, speaker: &crate::events::SpeakerId) -> Color {
        use crate::events::SpeakerId;
        if self.uncoloured {
            return Color::DarkGray;
        }
        match speaker {
            SpeakerId::Debater(id) => self.debater(id.as_str()),
            SpeakerId::Executor(_) => Color::LightYellow,
            SpeakerId::User => Color::LightGreen,
            SpeakerId::System => Color::Gray,
        }
    }

    fn debater(&mut self, id: &str) -> Color {
        if let Some(slot) = self.roster.iter().position(|name| name == id) {
            return DEBATER_PALETTE[slot % DEBATER_PALETTE.len()];
        }
        // A name the injected roster does not know, which is what a `/discuss` typed
        // mid-session produces. Its first appearance takes the first palette slot the
        // roster did not claim; the recollection below turns that into the same colour
        // on every later appearance. Grey once the palette is exhausted, because a
        // reused colour reads as the wrong speaker (票 07 §1).
        let slot = match self.extra.iter().position(|name| name == id) {
            Some(slot) => slot,
            None => {
                let slot = self.extra.len();
                self.extra.push(id.to_owned());
                slot
            }
        };
        match self.free_slots.get(slot) {
            Some(slot) => DEBATER_PALETTE[*slot],
            None => Color::Gray,
        }
    }
}

/// The session values the sidebar and the status row cannot read off the event
/// stream (spec §8).
///
/// Everything here is known at assembly time and injected as one value, because
/// that is the seam: the renderer never reaches for configuration. The status row
/// shows the model, the sidebar shows the counts, and the detail overlay reads
/// spilled tool output out of `session_dir`.
///
/// Anything that changes mid-session — the mode — is deliberately **not** here: an
/// injected copy would go stale the first time the user pressed Shift+Tab, and the
/// stream already carries both transitions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionFacts {
    /// The session this terminal is showing.
    pub session_id: String,
    /// The directory the session's own files live in: the detail overlay joins the
    /// spilled tool outputs out of it, and nothing on screen shows it (the working
    /// directory left the interface with the old header, spec §8).
    pub session_dir: String,
    /// The model the session answers with — or, in a discussion, the two debaters'
    /// models, because the status row has one field for it (spec §5).
    pub model: String,
    /// The input budget of that model's window, output reserve already removed.
    pub context_window: u64,
    /// The session's cumulative token allowance, when it has one.
    pub budget_limit: Option<u64>,
    /// The debaters of this session, in the order they were drawn. A single-agent
    /// session names its one profile; a discussion lists the pair the roster
    /// produced. It is what gives a speaker its colour, so it is injected at
    /// assembly for the same reason the rest of the facts are: the roster is not on
    /// the stream (票 07 §1).
    pub speaker_order: Vec<String>,
}

/// The TUI's injected values: the front end's end of the console channel, plus
/// the facts the sidebar and the status row display.
pub struct TuiOptions {
    pub port: ConsolePort,
    pub facts: SessionFacts,
    /// Whether this session was **reopened** (`--continue`), so a history replay is
    /// on its way over the console port.
    ///
    /// It is a flag, not the history: the events themselves ride
    /// [`ConsoleRequest::Replay`], because only the assembled harness has the
    /// post-recovery snapshot. The TUI needs the flag because it must not lay down a
    /// single render event — not even the banner — until that snapshot has arrived,
    /// or the recovery events assembly already emitted would be painted above the
    /// history they belong to and then painted again by the replay
    /// (`.scratch/tui-history-replay/spec.md` §1, §3).
    pub reopened: bool,
}

/// The TUI renderer.
pub struct Tui {
    options: TuiOptions,
}

impl Tui {
    pub fn new(options: TuiOptions) -> Self {
        Self { options }
    }

    pub async fn run(self, mut receiver: broadcast::Receiver<RenderEvent>) {
        let TuiOptions {
            mut port,
            facts,
            reopened,
        } = self.options;
        let mut state = TuiState::new(facts);

        // The alternate screen, raw mode, and a panic hook that restores them.
        // Mouse reporting and bracketed paste are ours: `ratatui::init` does not
        // touch either (spec §1).
        let mut terminal = ratatui::init();
        let modes = TerminalModes::enter();
        let mut keys = EventStream::new();
        let mut tick = tokio::time::interval(TICK);
        // The first tick fires immediately; soak it so the first frame is drawn
        // from state rather than from an empty buffer.
        tick.tick().await;

        // A reopened session waits for the replay before it renders anything. The
        // loop pushes it as its **first** console request, right after assembly and
        // before the banner; assembly has meanwhile emitted the recovery results on
        // the render channel, and those events are already in the replay's snapshot.
        // Waiting here is what keeps them from being painted above the history and
        // then painted again by the replay. A port that has gone drops the wait.
        if reopened {
            if let Some(request) = port.recv().await {
                state.request(request);
            }
        }

        loop {
            let mut closed = false;
            if state.replay_pending() {
                // A replay must not be throttled by the redraw tick — a large session
                // is dozens of batches, and waiting 120 ms between them would turn a
                // second into minutes — but the keyboard, the live stream and the
                // loop's requests still have to be answered between batches, or
                // `Ctrl-C` during a long replay would be a dead key. The last branch
                // is always ready, so this select never waits on anything and the
                // batch below runs as fast as the frame can be drawn
                // (`.scratch/tui-history-replay/spec.md` §2).
                tokio::select! {
                    biased;
                    received = receiver.recv() => closed = state.take_render_event(received),
                    maybe_event = keys.next() => state.terminal_event(maybe_event),
                    request = port.recv() => closed = state.port_request(request),
                    _ = std::future::ready(()) => {}
                }
                state.replay_batch();
            } else {
                tokio::select! {
                    received = receiver.recv() => closed = state.take_render_event(received),
                    maybe_event = keys.next() => state.terminal_event(maybe_event),
                    request = port.recv() => closed = state.port_request(request),
                    _ = tick.tick() => {}
                }
            }

            // Whatever is already queued joins this frame. A provider that bursts
            // a thousand deltas between two frames costs one frame instead of a
            // thousand, and still loses nothing (spec §11). During a replay these are
            // buffered rather than applied, exactly like the ones the select saw.
            let mut drained = 0usize;
            while drained < DRAIN_LIMIT {
                match receiver.try_recv() {
                    Ok(event) => state.live_event(event),
                    Err(broadcast::error::TryRecvError::Lagged(dropped)) => {
                        state.live_event(RenderEvent::Diagnostic(wording::renderer_dropped(
                            dropped,
                        )));
                    }
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Closed) => {
                        closed = true;
                        break;
                    }
                }
                drained += 1;
            }
            if closed {
                break;
            }

            for event in state.take_events() {
                port.emit(event);
            }
            if state.is_dirty() {
                // One frame in one write region. The synchronized update (DECSET
                // 2026) wraps the buffer diff alone — reading the keyboard has no
                // reason to be inside it — and a terminal without the pair simply
                // ignores it.
                let mut frame_out = std::io::stdout();
                let _ = execute!(frame_out, BeginSynchronizedUpdate);
                let _ = terminal.draw(|frame| draw_frame(frame, &mut state));
                let _ = execute!(frame_out, EndSynchronizedUpdate);
                state.mark_clean();
            }
            if state.should_quit() {
                break;
            }
        }

        drop(modes);
        ratatui::restore();
    }
}

/// Mouse reporting and bracketed paste: enabled on the way in, disabled on the way
/// out.
///
/// `ratatui::init` handles raw mode and the alternate screen only — its
/// `TerminalOptions` has no mouse switch at all — so these two are ours to undo,
/// on the normal path and on the panic path alike. Leaving them on would keep the
/// terminal from selecting text after fs-agent exited.
struct TerminalModes;

impl TerminalModes {
    fn enter() -> Self {
        let _ = execute!(std::io::stdout(), EnableMouseCapture, EnableBracketedPaste);
        // `init` installed a hook that restores raw mode and the alternate
        // screen; wrap it so a panic also gives the mouse and the paste mode back
        // before it runs.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            disable_terminal_modes();
            previous(info);
        }));
        Self
    }
}

impl Drop for TerminalModes {
    fn drop(&mut self) {
        disable_terminal_modes();
    }
}

fn disable_terminal_modes() {
    let _ = execute!(
        std::io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste
    );
}

#[async_trait]
impl Render for Tui {
    async fn consume(self: Box<Self>, receiver: broadcast::Receiver<RenderEvent>) {
        (*self).run(receiver).await;
    }
}

/// The display-ready state, split from the terminal so it can be tested without
/// one.
pub struct TuiState {
    /// What the sidebar and the status row display, injected at assembly (spec §8).
    facts: SessionFacts,
    /// The mode the session is in. Seeded from the assembly-time default and kept
    /// current from the stream, because both transitions ride it: entering plan
    /// mode is a context injection and leaving it is a history supersession.
    mode: Mode,
    /// The event-to-block merger the plain renderer shares.
    transcript: Transcript,
    /// The conversation pane: every completed block's lines, wrapped at the drawn
    /// width, with its own viewport. The pane owns the transcript rather than the
    /// terminal's scrollback.
    pane: Pane,
    /// The streaming tail of the current message.
    live: String,
    /// Whether anything has changed since the last frame was drawn.
    dirty: bool,
    /// The draft and its cursor.
    editor: Input,
    /// The names a leading `/` can become, as the loop reported them. Empty until
    /// that report arrives, which is why the `/` menu opens only once it has.
    catalog: Vec<CatalogEntry>,
    /// The `/` menu's highlight, and which token it belongs to. The matches
    /// themselves are not kept: they are a pure function of the draft and the
    /// catalog, so only the choice — which is not derivable — lives here.
    slash: MenuSelection,
    /// The numbers the sidebar's usage page shows, counted off the stream.
    panel: Panel,
    /// Which sidebar page is showing. Renderer state, not an event: nothing about it
    /// belongs on the stream, and it dies with the process (spec §3).
    tab: Tab,
    /// Which colour each speaker's name is drawn in (票 07). Kept here rather than
    /// recomputed per line because a name first seen mid-session has to keep the slot
    /// it was given.
    colors: SpeakerColors,
    /// The reasoning deltas of the segment that is currently being thought through.
    /// It is the bridge between the two halves of a thinking line: the line opens on
    /// the first delta and is rewritten from this when the trace is finished, because
    /// the finished trace only exists on `MessageCompleted` (票 02 §1).
    reasoning: String,
    /// The speaker the open thinking line belongs to, so the finished line can be
    /// written with the same name.
    thinking_speaker: crate::events::SpeakerId,
    /// Whether a thinking line is open on screen right now — the one mutable row in
    /// the pane.
    thinking_open: bool,
    /// Whether this turn's message has already had its thinking line. The body's first
    /// delta settles the line long before `MessageCompleted` arrives, so the record
    /// has to outlive `thinking_open` or the completion would add a second line for
    /// the same thought (票 02 §1).
    thinking_done: bool,
    /// The rail's units and their segment heads.
    rail: Rail,
    /// The detail behind each source line of the pane, parallel to it and pruned by
    /// the pane's own cap so the two never drift apart. `None` for the lines that are
    /// not a way into anything.
    links: std::collections::VecDeque<Option<Detail>>,
    /// Where the last frame drew each of its display rows, so a click can be turned
    /// back into the source line it landed on. Rebuilt every frame, like the
    /// question overlay's hit regions, because only drawn rows answer the pointer
    /// (票 04 §1).
    drawn_rows: Vec<Option<usize>>,
    /// The screen row the transcript's first drawn row was on, so a mouse row — which
    /// is in screen coordinates — can be turned into an index into `drawn_rows`.
    drawn_top: u16,
    /// The detail overlay, while one is open.
    detail: Option<DetailView>,
    /// Where the last frame drew that overlay, so a click outside it can close it —
    /// the same "remember what the reader actually saw" rule the indicator follows
    /// (票 02 §4).
    detail_rect: Option<Rect>,
    /// Where the last frame drew a question's clickable parts.
    regions: Regions,
    /// The last frame's whole terminal area. The detail overlay's body is laid out
    /// when it opens, and the width that layout needs is a function of the terminal
    /// size — known before the overlay is drawn, so a click does not have to wait for
    /// a frame (票 04 §1).
    area: Rect,
    /// Where a `Prompt` request's answer goes.
    prompt_reply: Option<tokio::sync::oneshot::Sender<Option<String>>>,
    /// Whether the loop says it is inside a run. **Pushed by the loop**, never
    /// inferred here: see [`TuiState::busy`].
    running: bool,
    /// A question waiting for a keypress.
    pending: Option<Pending>,
    /// Gestures to hand back to the loop.
    events: Vec<FrontEndEvent>,
    /// Where the last frame drew the "back to bottom" indicator, so a click can be
    /// matched against what the user actually saw.
    indicator: Option<Rect>,
    /// The history replay in flight, if any. `Some` is a one-shot startup state: the
    /// keyboard, the pointer and the loop behave differently until it drains
    /// (`.scratch/tui-history-replay/spec.md` §2).
    replay: Option<Replay>,
    /// Live render events that arrived while a replay was in flight, in arrival
    /// order. They are applied — after the history and its seam — once the replay
    /// finishes, so the startup banner cannot be painted into the middle of history
    /// (spec §3).
    live_buffer: Vec<RenderEvent>,
    quit: bool,
}

/// A question waiting for an answer.
///
/// Two kinds live here. The loop's asks ([`Question`]) travel back over a one-shot
/// channel; the renderer's own asks — an oversized paste, a draft that Esc would
/// throw away — have nobody to answer to, so they hold what they need to do the
/// thing themselves once the user says yes (spec §7).
enum Pending {
    /// The loop is waiting on an answer.
    Loop {
        question: Question,
        reply: tokio::sync::oneshot::Sender<AnswerChoice>,
    },
    /// A paste too large to take without asking.
    Paste { text: String, chars: usize },
    /// A multi-line draft that `Esc` would clear.
    ClearDraft,
    /// `Ctrl-D`: the renderer's own "are you sure you want out" (票 06 §1).
    ///
    /// It is the fifth renderer-side question and the only one that ends the
    /// application. It has no one to send an answer to — saying yes sets
    /// [`TuiState::quit`] and the loop's `select!` sees it on the next pass.
    Exit,
    /// The model's questionnaire, owning the bottom input area (spec §7, §19).
    ///
    /// This is the one question kind that does **not** go through [`Pending::modal`]:
    /// the middle overlay suits a one-line confirmation, while a questionnaire is
    /// multi-row and paged, so it takes the input area instead.
    Questionnaire(Questionnaire),
}

/// The model-initiated questionnaire while it owns the bottom input area.
///
/// The state lives here rather than in the session because a questionnaire is
/// keyboard state, not session state: nothing about it is recorded, and the only
/// durable trace of the exchange is the tool call's arguments and its one result
/// (spec §7).
struct Questionnaire {
    /// The questions, in the order the model sent them.
    questions: Vec<UserQuestion>,
    /// Where the answers go when the questionnaire is submitted. Dropping it
    /// without sending is read by the port as "no answer", which is what a
    /// cancelled run means.
    reply: tokio::sync::oneshot::Sender<Result<UserAnswers, String>>,
    /// One draft per question, indexed the same as `questions`.
    drafts: Vec<QuestionDraft>,
    /// Which question is on screen. One at a time, `2 / 3` in the footer.
    index: usize,
    /// Whether the current question's free-text row has the cursor. It is entered by
    /// clicking that row or by typing into it, and it resets when the page turns
    /// (票 04 §5).
    custom_focused: bool,
}

/// What the user has done to one question so far.
#[derive(Default, Clone)]
struct QuestionDraft {
    /// The option labels picked, in the order they were picked.
    selected: Vec<String>,
    /// Free text typed. Single-select custom text overrides `selected`;
    /// multi-select custom text supplements it (spec §7).
    custom: String,
    /// Which option the highlight is on. `↑`/`↓` move it, `Enter`/`Space`
    /// confirm it. It is per draft, so paging away and back finds the highlight
    /// where it was left.
    highlight: usize,
    /// The user pressed the skip key and moved on. This is a deliberate "no
    /// answer", distinct from a question that was never reached.
    skipped: bool,
}

impl QuestionDraft {
    /// Whether this question has been answered or explicitly skipped. Every
    /// question must reach this state before the questionnaire may be submitted.
    fn handled(&self) -> bool {
        self.skipped || !self.selected.is_empty() || !self.custom.trim().is_empty()
    }
}

impl Questionnaire {
    /// Take one keypress. Returns `true` when the questionnaire is submitted.
    ///
    /// The keyboard is the decided one (ticket 32): `↑`/`↓` move the highlight,
    /// `Enter` or `Space` confirms it, `Tab` skips the question, `←`/`→` page,
    /// and every printable character edits the free-text field. Digits are not
    /// keys at all, so they are ordinary text everywhere and no option is out of
    /// reach.
    ///
    /// `Enter` continues while something is unfinished and submits once
    /// everything is handled, so an unhandled question simply refuses the key.
    /// On a question with nothing answered yet it confirms the highlighted
    /// option — choosing *is* the answer on a single-select one, so that also
    /// advances — while an already-answered question (chosen or typed) just
    /// moves on, so `Enter` never clobbers typed custom text. Confirming does
    /// **not** submit on that same press: the next `Enter` is the submit
    /// (spec §7, §19).
    fn press(&mut self, key: Key) -> bool {
        match key {
            Key::Enter => {
                if self.all_handled() {
                    return true;
                }
                if self.drafts[self.index].handled() {
                    self.advance();
                } else if self.has_options() {
                    self.confirm_highlight();
                    if !self.questions[self.index].multi_select {
                        self.advance();
                    }
                }
            }
            // `Space` confirms too, so a person can answer without the key that
            // also submits. On a free-text question there is no option to
            // confirm, so it is an ordinary space in the text.
            Key::Char(' ') if self.has_options() => {
                self.confirm_highlight();
                if !self.questions[self.index].multi_select {
                    self.advance();
                }
            }
            // `Tab` is the explicit "skip this one and move on".
            Key::Tab => {
                self.drafts[self.index].skipped = true;
                self.advance();
            }
            Key::Up => self.move_highlight(-1),
            Key::Down => self.move_highlight(1),
            Key::Left => self.back(),
            Key::Right => self.advance(),
            Key::Backspace => {
                self.drafts[self.index].custom.pop();
            }
            // Every printable character is free text, digits included.
            Key::Char(ch) => {
                self.custom_focused = true;
                self.type_custom(ch);
            }
            _ => {}
        }
        false
    }

    /// Whether the question on screen offers options to highlight.
    fn has_options(&self) -> bool {
        !self.questions[self.index].options.is_empty()
    }

    /// Confirm the highlighted option (spec §7).
    ///
    /// A single-select question keeps only that option and clears custom text,
    /// because the two are alternatives and custom text overrides a choice. A
    /// multi-select question toggles, because custom text supplements the
    /// choices and the user is not done picking yet.
    fn confirm_highlight(&mut self) {
        let index = self.drafts[self.index].highlight;
        let Some(label) = self.questions[self.index]
            .options
            .get(index)
            .map(|choice| choice.label.clone())
        else {
            return;
        };
        let multi_select = self.questions[self.index].multi_select;
        let draft = &mut self.drafts[self.index];
        if multi_select {
            match draft.selected.iter().position(|picked| picked == &label) {
                Some(at) => {
                    draft.selected.remove(at);
                }
                None => draft.selected.push(label),
            }
        } else {
            draft.selected = vec![label];
            draft.custom.clear();
        }
    }

    /// Move the highlight by `delta`, clamped to the options. The highlight is
    /// what `Enter`/`Space` act on, so it never goes past either end.
    fn move_highlight(&mut self, delta: isize) {
        let count = self.questions[self.index].options.len();
        if count == 0 {
            return;
        }
        let draft = &mut self.drafts[self.index];
        let next = draft.highlight as isize + delta;
        draft.highlight = next.clamp(0, count as isize - 1) as usize;
    }

    /// Add one character of free text.
    ///
    /// On a single-select question typing clears the chosen option, because the
    /// custom text is about to override it. On a multi-select one the choices
    /// stay, because custom text supplements them (spec §7).
    fn type_custom(&mut self, ch: char) {
        let multi_select = self.questions[self.index].multi_select;
        let draft = &mut self.drafts[self.index];
        if !multi_select {
            draft.selected.clear();
        }
        draft.custom.push(ch);
    }

    fn advance(&mut self) {
        if self.index + 1 < self.questions.len() {
            self.index += 1;
            // The page turned, so the cursor goes back to the question's own rows
            // (票 04 §5).
            self.custom_focused = false;
        }
    }

    fn back(&mut self) {
        if self.index > 0 {
            self.index -= 1;
            self.custom_focused = false;
        }
    }

    fn all_handled(&self) -> bool {
        self.drafts.iter().all(QuestionDraft::handled)
    }

    /// Pick the option at `index` in the current question, the way clicking its row
    /// does.
    ///
    /// A single-select question answers and moves on — the click *is* the answer —
    /// except on the last question, where it only answers: submitting is still the
    /// separate `Enter` (票 04 §4). A multi-select question only toggles, because the
    /// reader is not done picking.
    fn select_option(&mut self, index: usize) {
        if index >= self.questions[self.index].options.len() {
            return;
        }
        let multi_select = self.questions[self.index].multi_select;
        self.drafts[self.index].highlight = index;
        self.custom_focused = false;
        if multi_select {
            self.confirm_highlight();
        } else {
            self.confirm_highlight();
            self.advance();
        }
    }

    /// Hand the cursor to the free-text row.
    fn focus_custom(&mut self) {
        self.custom_focused = true;
    }

    /// Take it away when the page turns.
    fn unfocus_custom(&mut self) {
        self.custom_focused = false;
    }

    /// The answers as the tool's one result (spec §7): a skipped question is
    /// `selected: []` with no `custom`, and single-select custom text overrides
    /// the choice.
    ///
    /// The skip check comes first because skipping is a decision about the whole
    /// question: text typed before `Tab` is discarded, or `selected: []` plus a
    /// `custom` would read to the model as a deliberate custom answer instead of
    /// "the user chose not to answer" (spec §7).
    fn answers(&self) -> UserAnswers {
        UserAnswers {
            answers: self
                .questions
                .iter()
                .zip(&self.drafts)
                .map(|(question, draft)| {
                    if draft.skipped {
                        return UserAnswer {
                            id: question.id.clone(),
                            selected: Vec::new(),
                            custom: None,
                        };
                    }
                    let custom = draft.custom.trim();
                    let custom = (!custom.is_empty()).then(|| custom.to_owned());
                    let selected = if !question.multi_select && custom.is_some() {
                        Vec::new()
                    } else {
                        draft.selected.clone()
                    };
                    UserAnswer {
                        id: question.id.clone(),
                        selected,
                        custom,
                    }
                })
                .collect(),
        }
    }
}

impl Pending {
    /// The rows the overlay shows for this question, or `None` for a question
    /// that is not drawn as an overlay.
    ///
    /// The loop's asks and the renderer's own go over the transcript, so the draft
    /// stays where the user left it (spec §7, §9). The questionnaire does not: it
    /// is multi-row and paged, so it takes over the bottom input area instead
    /// (spec §19), and this answers `None` for it.
    fn modal(&self) -> Option<Modal> {
        let modal = match self {
            Pending::Loop {
                question: Question::Permission(request),
                ..
            } => Modal {
                title: wording::permission_title().to_owned(),
                // Then the same one-line description the folded transcript line
                // carries — and *then* the call as it will run, because approving is
                // the one moment the exact command has to be readable (2026-09-23).
                description: Some(wording::tool_call_line(&request.tool_name, &request.args)),
                detail: Some(wording::permission_call(
                    &request.tool_name,
                    &summarize_args(&request.args),
                )),
                choices: &wording::PERMISSION_CHOICES,
                actions: wording::PERMISSION_CHOICE_ANSWERS
                    .iter()
                    .map(|(_, answer)| HitAction::Answer(AnswerChoice::Permission(*answer)))
                    .collect(),
            },
            Pending::Loop {
                question: Question::PlanConflict(path),
                ..
            } => Modal {
                title: wording::plan_conflict_title().to_owned(),
                description: None,
                detail: Some(wording::plan_conflict_body(&path.display().to_string())),
                choices: &wording::PLAN_CHOICES,
                actions: wording::PLAN_CHOICE_ANSWERS
                    .iter()
                    .map(|(_, conflict)| HitAction::Answer(AnswerChoice::Plan(*conflict)))
                    .collect(),
            },
            Pending::Paste { chars, .. } => Modal {
                title: wording::paste_title().to_owned(),
                description: None,
                detail: Some(wording::paste_body(*chars)),
                choices: &wording::PASTE_CHOICES,
                actions: vec![HitAction::Paste, HitAction::Dismiss],
            },
            Pending::ClearDraft => Modal {
                title: wording::clear_draft_title().to_owned(),
                description: None,
                detail: Some(wording::clear_draft_body().to_owned()),
                choices: &wording::CLEAR_CHOICES,
                actions: vec![HitAction::ClearDraft, HitAction::Dismiss],
            },
            Pending::Exit => Modal {
                title: wording::exit_title().to_owned(),
                description: None,
                detail: Some(wording::exit_body().to_owned()),
                choices: &wording::EXIT_CHOICES,
                actions: vec![HitAction::Quit, HitAction::Dismiss],
            },
            Pending::Questionnaire(_) => return None,
        };
        Some(modal)
    }
}

/// The parts a question is asked in (spec §9).
///
/// The split is the point. A question used to be one wrapped paragraph, so a long
/// command pushed its keys past the right edge and the reader had to pick them out
/// of a sentence; here the title says what is being asked, the summary says what the
/// action *is*, the detail shows the concrete call, and the keys that decide it get a
/// row of their own and cannot be buried by any of it.
struct Modal {
    /// The title row, higher up and bolder than the rest: `权限询问：bash`.
    title: String,
    /// What the call is *for*, in the very words the folded transcript line uses
    /// (`调用 bash 查看 git status`), so the question and the line it is about read
    /// alike (2026-09-23, user request). Absent for a question that is not about a tool
    /// call.
    description: Option<String>,
    /// The one concrete thing the question is about: the call, the path, the size.
    detail: Option<String>,
    /// The keys that answer it, painted as one row of buttons.
    choices: &'static [wording::Choice],
    /// What a click on each of those buttons does. Empty means "answer with the key",
    /// which is what the loop's questions want; the renderer's own confirmations name
    /// themselves because they have no channel to answer over (票 04 §3).
    actions: Vec<HitAction>,
}

/// One clickable region of a question, recorded as it is painted.
///
/// The rectangle is in **screen** coordinates and its `action` is the whole of what a
/// click there means, so the pointer handler never has to re-derive the layout it is
/// looking at (票 04 §1).
#[derive(Clone)]
struct Region {
    rect: Rect,
    action: HitAction,
}

/// What a click on a question's painted part does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HitAction {
    /// Answer the loop's question with this choice, exactly as pressing its key would.
    Answer(AnswerChoice),
    /// Confirm the oversized paste.
    Paste,
    /// Confirm clearing the draft.
    ClearDraft,
    /// Confirm quitting.
    Quit,
    /// Close the question without doing anything: the safe answer, as `Esc` is.
    Dismiss,
    /// Step back one question.
    Previous,
    /// Step forward one question.
    Next,
    /// Submit the questionnaire.
    Submit,
    /// Show this sidebar page, as clicking its tab does (spec §3).
    SwitchTab(Tab),
    /// Jump to the start of this unit, as clicking its rail cell does (spec §4).
    RailUnit(usize),
}

/// A pointer gesture while a question owns it.
enum QuestionClick {
    /// A wheel notch: `true` is up.
    Wheel(bool),
    /// A left click, in screen coordinates.
    At(u16, u16),
}

/// What the last frame drew that a question's pointer can act on.
///
/// Everything here is rebuilt every frame and read by the next click, which is the
/// same mechanism the transcript's `indicator` has always used — and the reason a
/// scrolled-away option or a clipped button needs no invalidation of its own
/// (票 04 §1).
#[derive(Default, Clone)]
struct Regions {
    /// One region per question-overlay button, in the order they are drawn.
    cells: Vec<Region>,
    /// The questionnaire's visible option rows: `(row, index in the question)`.
    options: Vec<(u16, usize)>,
    /// The questionnaire's free-text row, when it was drawn.
    custom: Option<u16>,
}

impl Regions {
    /// Forget everything: called once per frame, before anything is painted.
    fn clear(&mut self) {
        self.cells.clear();
        self.options.clear();
        self.custom = None;
    }

    /// The option index a click on `row` landed on, if that row was an option. The
    /// whole row is the target, so the column does not narrow it (票 04 §4).
    fn option_at(&self, row: u16) -> Option<usize> {
        self.options
            .iter()
            .find(|(option_row, _)| *option_row == row)
            .map(|(_, index)| *index)
    }

    /// Whether `row` is the free-text row.
    fn custom_at(&self, row: u16) -> bool {
        self.custom == Some(row)
    }

    /// The action of the button a click landed on, if it landed on one.
    fn action_at(&self, column: u16, row: u16) -> Option<HitAction> {
        self.cells
            .iter()
            .find(|region| region.rect.contains((column, row).into()))
            .map(|region| region.action)
    }
}

/// The `/` menu's remembered half.
///
/// Everything else about the menu is derived: the token comes from the draft, the
/// matches from the catalog and that token. What cannot be derived is which row the
/// user picked, so that — and the prefix it was picked under — is what is kept.
#[derive(Debug, Default)]
struct MenuSelection {
    /// The prefix the highlight was made under. A different prefix means this is a
    /// different menu, so the highlight starts over and an `Esc` stops applying.
    prefix: String,
    /// Which match is highlighted, if any.
    ///
    /// **`None` on a bare `/`**, and that is the safety rule: nothing is picked until
    /// the user has typed a name or walked the list with an arrow, so a key pressed
    /// only to look at the menu cannot run a command nobody asked for (spec §7's
    /// reading of an incidental key).
    selected: Option<usize>,
    /// `Esc` closed the menu. It stays closed while the prefix it was closed under
    /// is what is still being typed.
    dismissed: bool,
}

/// The `/` menu as it stands right now: which token it belongs to, what matches it,
/// and which match is highlighted.
///
/// A value, built per frame and per keypress from the draft plus the catalog. The
/// menu is never state that can drift from what is on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SlashMenu {
    /// What has been typed after the slash.
    prefix: String,
    /// The matching entries, in catalog order, as `(name, description)`.
    entries: Vec<(String, String)>,
    /// Which entry is highlighted, clamped into range; `None` when none is.
    selected: Option<usize>,
}

/// The non-acting answer to a question the loop asked.
fn default_choice(question: &Question) -> AnswerChoice {
    match question {
        Question::PlanConflict(_) => AnswerChoice::Plan(crate::permissions::PlanConflict::Keep),
        Question::Permission(_) => AnswerChoice::Permission(crate::permissions::Answer::Deny),
    }
}

/// Whether a key means yes to a question this renderer asked itself.
///
/// Only `y`. `Esc` and `Enter` are the **safe** answer — "no" — because both are what
/// a hand reaches for without reading (spec §7, 票 06 §1).
fn agrees(key: Key) -> bool {
    matches!(key, Key::Char('y') | Key::Char('Y'))
}

/// The rail's bookkeeping: one unit per completed turn or round, and where each one
/// starts (`.scratch/tui-sidebar/spec.md` §4).
///
/// Two indexes and a counter, all derived from the stream — there is deliberately no
/// remembered selection, because a stored "current unit" would drift away from the
/// viewport the moment anything arrived.
#[derive(Default)]
struct Rail {
    /// The unit each source line belongs to, parallel to `links` and pruned with it.
    /// A line that arrived after the last boundary belongs to the unit that has not
    /// finished yet.
    of_line: std::collections::VecDeque<usize>,
    /// Whether each source line is the user's own message. The head of a unit is the
    /// first of those inside it.
    user: std::collections::VecDeque<bool>,
    /// The segment-head source line of each **completed** unit.
    heads: Vec<usize>,
    /// Source lines painted since the last boundary. Counted rather than remembered as
    /// an index because the cap drops lines from the front.
    lines_in_unit: usize,
}

impl Rail {
    /// How many units the session has completed.
    fn units(&self) -> usize {
        self.heads.len()
    }

    /// Note one painted source line.
    fn push_line(&mut self, user_message: bool) {
        // The unit a line belongs to is the one being built: `units()` is how many are
        // finished, so that is the index this line will take when its turn ends.
        self.of_line.push_back(self.heads.len());
        self.user.push_back(user_message);
        self.lines_in_unit += 1;
    }

    /// The current unit ended: record where its segment starts and open the next one.
    ///
    /// The head is the **first user message inside the unit**, which is what a cell
    /// click should land on: the reader asked the question, so that is where a turn
    /// begins. A discussion unit has no user message of its own (the debaters answer
    /// the one question the session already holds), so it falls back to the unit's own
    /// first line — which is the round's opening narration.
    fn close_unit(&mut self) {
        let start = self.of_line.len().saturating_sub(self.lines_in_unit);
        let head = (start..self.of_line.len())
            .find(|index| self.user[*index])
            .unwrap_or(start);
        self.heads.push(head);
        self.lines_in_unit = 0;
    }

    /// Drop the oldest source lines with the transcript's cap, and shift the heads
    /// that pointed past them. A unit whose whole span is dropped collapses onto the
    /// oldest surviving line, which is the closest thing left to jump to.
    fn prune(&mut self, dropped: usize) {
        for _ in 0..dropped {
            self.of_line.pop_front();
            self.user.pop_front();
        }
        for head in &mut self.heads {
            *head = head.saturating_sub(dropped);
        }
        self.lines_in_unit = self.lines_in_unit.saturating_sub(dropped);
    }

    /// The unit a source line belongs to.
    fn unit_of(&self, source: usize) -> usize {
        self.of_line
            .get(source)
            .copied()
            .unwrap_or_else(|| self.units())
    }

    /// Where a unit's segment starts.
    fn head(&self, unit: usize) -> Option<usize> {
        self.heads.get(unit).copied()
    }
}

/// A history replay in flight: the assembled event stream, how much of it has been
/// laid into the transcript, and how many source lines that produced.
///
/// The replay is a one-shot state of the renderer — it is not [`TuiState::busy`],
/// which is the loop's word for a run: there is nothing to cancel during a replay, so
/// `Ctrl-C` quits instead (`.scratch/tui-history-replay/spec.md` §2, §5).
struct Replay {
    /// The whole stream, in `seq` order, exactly as assembly left it.
    events: Vec<Event>,
    /// The next event to apply. Doubles as the `n` the progress line shows.
    next: usize,
    /// Source lines the replay has produced so far. Zero means the history drew
    /// nothing — an empty stream, or a skeleton of `SessionStarted` — and so there is
    /// no seam to mark (spec §6).
    lines: usize,
}

impl TuiState {
    pub fn new(facts: SessionFacts) -> Self {
        let colors = SpeakerColors::new(&facts.speaker_order);
        Self {
            facts,
            mode: Mode::Ask,
            transcript: Transcript::new(),
            pane: Pane::new(),
            live: String::new(),
            dirty: true,
            editor: Input::new(),
            catalog: Vec::new(),
            slash: MenuSelection::default(),
            panel: Panel::new(),
            tab: Tab::Usage,
            colors,
            reasoning: String::new(),
            thinking_speaker: crate::events::SpeakerId::System,
            thinking_open: false,
            thinking_done: false,
            rail: Rail::default(),
            links: std::collections::VecDeque::new(),
            drawn_rows: Vec::new(),
            drawn_top: 0,
            detail: None,
            detail_rect: None,
            regions: Regions::default(),
            area: Rect::default(),
            prompt_reply: None,
            // Idle until the loop says otherwise: before it asks its first line nothing
            // is running, and the keyboard has to read that way (spec §6).
            running: false,
            pending: None,
            events: Vec::new(),
            indicator: None,
            replay: None,
            live_buffer: Vec::new(),
            quit: false,
        }
    }

    /// Whether anything has changed since the last frame was drawn.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Bracketed paste arrives as text, not as keys.
    ///
    /// Three things happen here that crossterm leaves to us (research §6.3): line
    /// endings are normalised, control characters are dropped, and a paste too large
    /// to take on sight asks first. **None of it submits** — a pasted newline is a
    /// newline (spec §7).
    pub fn paste(&mut self, text: &str) {
        if self.pending.is_some() {
            // A question owns the keyboard while it is up: a paste must not answer
            // it, and must not land in a draft the user cannot see (spec §9).
            return;
        }
        let text = editor::normalize_paste(text);
        if text.is_empty() {
            return;
        }
        self.dirty = true;
        let chars = text.chars().count();
        if chars > PASTE_CONFIRM_CHARS {
            self.pending = Some(Pending::Paste { text, chars });
        } else {
            self.editor.insert_str(&text);
            // A paste can be a `/` command like any other keystroke: a pasted
            // `/ask-matt` opens the menu on the frame after it lands.
            self.sync_menu();
        }
    }

    /// Feed one render event, and answer how many transcript source lines it drew.
    ///
    /// The count is what the history replay's batch budget is measured in: a frame's
    /// cost is bounded by the text it lays down, not only by how many events it
    /// consumed (`.scratch/tui-history-replay/spec.md` §2). Live callers ignore it.
    pub fn apply(&mut self, event: RenderEvent) -> usize {
        self.dirty = true;
        let mut produced = 0usize;
        for block in self.transcript.push(event) {
            // The thinking line's lifecycle runs before the block is painted: a
            // reasoning delta opens it, the body's first delta freezes it in place,
            // and `MessageCompleted` settles whatever is still open (票 02 §1).
            if let Block::Delta {
                speaker,
                kind,
                text,
            } = &block
            {
                match kind {
                    DeltaKind::Reasoning => {
                        produced += usize::from(self.open_thinking(speaker.clone()));
                        self.reasoning.push_str(text);
                    }
                    DeltaKind::Text => self.freeze_thinking(),
                }
            }
            if let Block::Message {
                speaker,
                role,
                reasoning,
                ..
            } = &block
            {
                if matches!(role, Role::Assistant) {
                    // The message's whole trace settles whatever is open. When the
                    // provider sent no deltas at all the line is opened here instead,
                    // already finished; when one was already opened and settled (the
                    // body froze it mid-stream) nothing new is added, because one
                    // thinking segment is one line (票 02 §1). An absent trace settles
                    // an open line as unrecorded — the synthesizer's shape, deltas
                    // streamed and nothing written down.
                    match reasoning {
                        Some(text) => {
                            let text = text.clone();
                            if !self.thinking_open && !self.thinking_done {
                                produced += usize::from(self.open_thinking(speaker.clone()));
                            }
                            self.settle_thinking(Some(text));
                        }
                        None => {
                            if self.thinking_open {
                                self.settle_thinking(None);
                            }
                        }
                    }
                    self.thinking_done = true;
                }
            }
            // A new turn's thinking is a new segment, so the "already drawn" latch
            // clears with the turn (票 02 §1).
            if matches!(&block, Block::TurnStarted { .. }) {
                self.thinking_done = false;
            }
            match &block {
                // Reasoning is not part of the message body: it is folded into its own
                // line, so it never joins the live tail the body streams through
                // (票 02 §3).
                Block::Delta {
                    kind: DeltaKind::Reasoning,
                    ..
                } => {}
                Block::Delta { text, .. } => {
                    self.live.push_str(text);
                    if self.live.len() > LIVE_BUFFER {
                        let cut = self.live.len() - LIVE_BUFFER;
                        // Trim on a char boundary.
                        let cut = (cut..self.live.len())
                            .find(|index| self.live.is_char_boundary(*index))
                            .unwrap_or(self.live.len());
                        self.live.drain(..cut);
                    }
                }
                Block::Message { .. } => {
                    // The deltas were the live view; the block is the permanent
                    // one, so the tail can go.
                    self.live.clear();
                }
                // The two transitions that move a session between modes. Both
                // already ride the stream, which is why the mode is not injected.
                Block::ContextInjected {
                    source: ContextSource::PlanMode,
                } => self.mode = Mode::Plan,
                Block::History {
                    reason: HistoryReason::ModeChange,
                    ..
                } => self.mode = Mode::Ask,
                _ => {}
            }
            // The panel counts what this block says about the session; the pane
            // shows what it says to the reader. Names are tinted on the way in, so a
            // speaker's first line is what settles any name the injected roster did
            // not list (票 07).
            self.panel.observe(&block);
            let lines = paint_block(&block, &mut self.colors);
            produced += lines.len();
            for rendered in lines {
                let link = rendered.link;
                self.pane.push(rendered.line);
                self.links.push_back(link);
                self.rail.push_line(is_user_message(&block));
                self.prune_links();
            }
            // A turn's end closes a unit; so does a round's, in a discussion — where
            // the unit is the **round**, because that is the thing a discussion counts
            // (`CONTEXT.md` keeps 轮次 and 回合 apart, spec §4).
            if is_boundary(&block, self.discussion()) {
                self.rail.close_unit();
            }
        }
        produced
    }

    /// Whether this session counts **rounds** rather than turns.
    ///
    /// Injected rather than inferred: a discussion is a session with more than one
    /// debater, and that is part of what assembly already knows (spec §4).
    fn discussion(&self) -> bool {
        self.facts.speaker_order.len() > 1
    }

    /// Feed one **live** render event: an event that arrived while the renderer is
    /// running, as opposed to one the history replay is laying down.
    ///
    /// While a replay is in flight the event is held back, in arrival order, so
    /// history and the lines this session adds cannot interleave. A **logged** event
    /// is not held at all: the replay's snapshot is the assembled stream, so a logged
    /// event arriving during a replay is one the snapshot already holds — buffering it
    /// would paint the same tool call or message a second time. Only events that never
    /// enter the log — the banner, diagnostics, streaming deltas — are buffered for
    /// after the seam (`spec` §3).
    pub fn live_event(&mut self, event: RenderEvent) {
        if self.replay.is_some() {
            if !matches!(event, RenderEvent::Logged(_)) {
                self.live_buffer.push(event);
            }
            self.dirty = true;
        } else {
            self.apply(event);
        }
    }

    /// Whether the history replay still has events to lay down.
    pub fn replay_pending(&self) -> bool {
        self.replay.is_some()
    }

    /// Apply one batch of the history replay's events.
    ///
    /// The batch is bounded by **both** [`REPLAY_BATCH_EVENTS`] and
    /// [`REPLAY_BATCH_LINES`], whichever trips first: a slice of 512 events is not
    /// itself a bound on a frame when each one can be a huge tool result. Finishing
    /// the batch that consumes the last event also closes the replay — the seam, the
    /// buffered live events and the return to the bottom all happen here, before the
    /// next frame is drawn.
    pub fn replay_batch(&mut self) {
        let Some(mut replay) = self.replay.take() else {
            return;
        };
        let mut applied = 0usize;
        let mut produced = 0usize;
        while replay.next < replay.events.len()
            && applied < REPLAY_BATCH_EVENTS
            && produced < REPLAY_BATCH_LINES
        {
            let event = replay.events[replay.next].clone();
            replay.next += 1;
            applied += 1;
            produced += self.apply(RenderEvent::Logged(event));
        }
        replay.lines += produced;
        // `apply` sets the flag itself, but the progress line's `n` is state the frame
        // only sees if this pass says so — and the pass that changes nothing but the
        // count is exactly the one that would otherwise never be drawn.
        self.dirty = true;
        if replay.next >= replay.events.len() {
            self.finish_replay(replay);
        } else {
            self.replay = Some(replay);
        }
    }

    /// Close a finished replay: mark the seam, release the buffered live events, and
    /// return the viewport to the bottom.
    ///
    /// The divider is inserted only when the history actually drew something, so an
    /// empty stream or a bare `SessionStarted` skeleton gets no seam to nothing. It
    /// is a render-layer line rather than an event, so it never enters the log and
    /// the next `--continue` inserts a new one instead of replaying the old.
    fn finish_replay(&mut self, replay: Replay) {
        if replay.lines > 0 {
            self.apply(RenderEvent::Notice(wording::history_divider().to_owned()));
        }
        for event in std::mem::take(&mut self.live_buffer) {
            self.apply(event);
        }
        // The reader is caught up: the transcript is history and the viewport is at
        // its end, which is where a session that has just started belongs.
        self.pane.to_bottom();
        self.dirty = true;
    }

    /// Handle one keypress while the history replay is in flight.
    ///
    /// A replay is **not** a run, so none of the run's keys mean their run meaning:
    /// there is nothing to cancel, and `Ctrl-C` quits. The editor keeps working —
    /// waiting time is typing time — but `Enter` cannot submit and the transcript's
    /// scroll keys are ignored, because the history below is still being laid down
    /// and the viewport stays pinned to its end (`spec` §3, §5).
    fn replay_key(&mut self, key: Key) {
        if key == Key::CtrlC {
            self.quit = true;
            return;
        }
        // Everything the editor answers to still works; `Ctrl-D`, `Esc`, `Enter`, the
        // scroll keys and the plan-mode gesture are ignored outright.
        self.editor_key(key);
        self.sync_menu();
    }

    /// Apply the keys that edit the draft, wherever the draft is live — the resident
    /// editor and a replay share them, so the two cannot drift.
    fn editor_key(&mut self, key: Key) {
        match key {
            Key::Char(ch) => self.editor.insert_char(ch),
            // The one reliable newline key: Shift+Enter arrives as plain Enter on a
            // terminal without the keyboard-enhancement protocol, so it submits
            // (spec §6).
            Key::CtrlJ => self.editor.insert_char('\n'),
            Key::Backspace => self.editor.backspace(),
            Key::Delete => self.editor.delete_forward(),
            Key::Left => self.editor.left(),
            Key::Right => self.editor.right(),
            Key::Up => self.editor.up(),
            Key::Down => self.editor.down(),
            Key::Home | Key::CtrlA => self.editor.home(),
            Key::End | Key::CtrlE => self.editor.end(),
            Key::CtrlU => self.editor.kill_to_line_start(),
            Key::CtrlK => self.editor.kill_to_line_end(),
            Key::CtrlW => self.editor.kill_word(),
            // History is `Ctrl-P` / `Ctrl-N` alone; the arrows belong to the cursor.
            Key::CtrlP => self.editor.history_previous(),
            Key::CtrlN => self.editor.history_next(),
            _ => {}
        }
    }

    /// Open the thinking line, unless one is already open. `true` when it drew a row.
    ///
    /// The line is a plain transcript row — it counts against the pane's cap and
    /// scrolls with everything else — and it is deliberately **not** clickable yet:
    /// the whole trace only exists on `MessageCompleted` (票 02 §1).
    fn open_thinking(&mut self, speaker: crate::events::SpeakerId) -> bool {
        if self.thinking_open {
            return false;
        }
        self.thinking_open = true;
        self.thinking_speaker = speaker;
        self.reasoning.clear();
        // The name is a `speaker_label`, so it takes the speaker's colour — the same
        // rule every other line with one follows (票 07 §2).
        let name = wording::speaker_label(&self.thinking_speaker);
        let color = self.colors.of(&self.thinking_speaker);
        let name_style = Style::default().fg(color);
        let line = Line::from(vec![
            Span::styled(format!("{name} "), name_style),
            Span::styled(
                wording::thinking_in_progress(),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        self.pane.push(line);
        self.links.push_back(None);
        self.prune_links();
        true
    }

    /// Freeze the open thinking line where it stands: the body's first delta means
    /// the model has stopped thinking and started answering, so the line settles
    /// (票 02 §1).
    fn freeze_thinking(&mut self) {
        if !self.thinking_open {
            return;
        }
        let text = std::mem::take(&mut self.reasoning);
        let recorded = !text.is_empty();
        self.settle_thinking(recorded.then_some(text));
    }

    /// Settle the thinking line — recorded trace or not — and make it the way into
    /// its detail.
    ///
    /// `Some` is a recorded trace; `None` is the synthesizer's shape — deltas
    /// streamed, the log holds no whole text — and its detail says so. A turn with no
    /// thinking line open at all adds nothing (票 02 §1).
    fn settle_thinking(&mut self, text: Option<String>) {
        if !self.thinking_open {
            return;
        }
        self.reasoning.clear();
        self.thinking_open = false;
        self.thinking_done = true;
        let name = wording::speaker_label(&self.thinking_speaker);
        let color = self.colors.of(&self.thinking_speaker);
        let name_style = Style::default().fg(color);
        // In place: one thinking segment is one line, from `正在思考` to `思考完成`
        // (票 02 §1). The `▸` after the name is what says the line can be opened — it
        // trails the speaker so every line still starts with who is speaking
        // (票 03 §Answer，2026-09-23 修正).
        let line = Line::from(vec![
            Span::styled(format!("{name} "), name_style),
            Span::styled("▸ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                wording::thinking_finished(),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        let detail = Detail {
            // The overlay's title is the clicked line's own text (票 02 §4).
            title: line_text(&line),
            color,
            kind: DetailKind::Thinking { text },
        };
        self.pane.replace_last(line);
        if let Some(link) = self.links.back_mut() {
            *link = Some(detail);
        }
    }

    /// Drop the oldest links until this list is no longer than the pane's cap, which
    /// is the only way the two stay parallel: a source row means the same thing in
    /// both or neither (票 04 §1).
    ///
    /// The rail's per-line index is pruned in the same breath, and for the same
    /// reason: a source line's unit is looked up by the index the pane hands back.
    fn prune_links(&mut self) {
        let before = self.links.len();
        while self.links.len() > pane::CAP {
            self.links.pop_front();
        }
        let dropped = before - self.links.len();
        if dropped > 0 {
            self.rail.prune(dropped);
        }
    }

    /// Handle one mouse event.
    ///
    /// Only two things answer to the mouse: the wheel scrolls the transcript, and
    /// a click on the "back to bottom" indicator returns to the bottom. Every
    /// other click is ignored — the terminal's own selection is the user's, and
    /// nothing here takes focus (spec §4).
    pub fn mouse(&mut self, mouse: MouseEvent) {
        // A replay owns the pointer by ignoring it: history is still being laid down
        // under a viewport pinned to the bottom, so neither a wheel notch nor a click
        // may move it or open a line that has not finished arriving (`spec` §5).
        if self.replay.is_some() {
            return;
        }
        // Three dispatches, in order of who owns the pointer. A detail overlay owns
        // it outright; otherwise a question does; otherwise the transcript does.
        // Nothing here ever scrolls the transcript behind something that is up
        // (票 04 §2).
        self.dirty = true;
        // 1. The detail overlay owns the pointer outright. Its whole body scrolls, and
        // a second click on the line it came from closes it (票 02 §4).
        if self.detail_open() {
            match mouse.kind {
                MouseEventKind::ScrollUp => self.detail_scroll(-1),
                MouseEventKind::ScrollDown => self.detail_scroll(1),
                MouseEventKind::Down(MouseButton::Left) => {
                    // A click outside closes it — the line it came from, the
                    // transcript, the footer, anything (票 02 §4；2026-09-23 修正，
                    // 原先只认「再点同一行」). A click inside is the overlay's own and
                    // does nothing, because it has no buttons of its own.
                    let inside = self
                        .detail_rect
                        .is_some_and(|rect| rect.contains((mouse.column, mouse.row).into()));
                    if !inside {
                        self.close_detail();
                    }
                }
                _ => {}
            }
            return;
        }
        // 2. A question owns the pointer next: the wheel must not scroll the
        // transcript behind it, and a click answers it where it is answerable. What
        // is answerable is whatever the last frame recorded as a region, so a key that
        // was clipped or scrolled away simply has no region (spec §9, 票 04 §2).
        if self.pending.is_some() {
            match mouse.kind {
                MouseEventKind::ScrollUp => self.question_click(QuestionClick::Wheel(true)),
                MouseEventKind::ScrollDown => self.question_click(QuestionClick::Wheel(false)),
                MouseEventKind::Down(MouseButton::Left) => {
                    self.question_click(QuestionClick::At(mouse.column, mouse.row))
                }
                _ => {}
            }
            return;
        }
        // 3. Otherwise the frame's own parts, in the order who owns the pointer: the
        // sidebar's tabs, then the transcript — whose wheel, indicator and collapsed
        // lines answer to it. A tab is a control, so it is asked before the text around
        // it is (spec §7).
        match mouse.kind {
            MouseEventKind::ScrollUp => self.pane.wheel(true),
            MouseEventKind::ScrollDown => self.pane.wheel(false),
            MouseEventKind::Down(MouseButton::Left) => {
                match self.regions.action_at(mouse.column, mouse.row) {
                    Some(HitAction::SwitchTab(tab)) => self.tab = tab,
                    Some(HitAction::RailUnit(unit)) => self.jump_to_unit(unit),
                    _ if self.indicator_hit(mouse.column, mouse.row) => self.pane.to_bottom(),
                    _ => {
                        let width = layout::plan(self.area, 1).detail_width() as usize;
                        if let Some(detail) = self.link_hit(&mouse) {
                            self.open_detail(detail, width);
                        }
                    }
                }
                self.dirty = true;
            }
            _ => {}
        }
    }

    /// Act on a click or a wheel notch while a question owns the pointer.
    fn question_click(&mut self, click: QuestionClick) {
        match self.pending.as_mut() {
            // The middle overlay: each `[key] label` interval is one button, and a
            // click runs exactly the answer that key would have (票 04 §3).
            Some(
                Pending::Loop { .. } | Pending::Paste { .. } | Pending::ClearDraft | Pending::Exit,
            ) => {
                let QuestionClick::At(column, row) = click else {
                    // The wheel does nothing over a one-line question.
                    return;
                };
                let Some(action) = self.regions.action_at(column, row) else {
                    return;
                };
                let pending = self.pending.take().expect("a question is up");
                // The loop's questions send the answer the button was built with —
                // the same answer its key sends — and the renderer's own
                // confirmations answer themselves.
                match (pending, action) {
                    (Pending::Loop { reply, .. }, HitAction::Answer(choice)) => {
                        let _ = reply.send(choice);
                    }
                    (
                        pending @ (Pending::Paste { .. } | Pending::ClearDraft | Pending::Exit),
                        action,
                    ) => self.own_answer(pending, action),
                    // A region that is not this question's own. The sidebar's tabs are
                    // in the same table of what the frame painted, so a click on one
                    // arrives here; the question owns the pointer, so the click does
                    // nothing — and above all it must not close the question the reader
                    // has not answered (spec §7, §9).
                    (pending, _) => self.pending = Some(pending),
                }
            }
            // The questionnaire owns the bottom input area: option rows, the custom
            // line, and the footer's paging buttons (票 04 §4).
            Some(Pending::Questionnaire(_)) => self.questionnaire_click(click),
            None => {}
        }
    }

    /// Answer one of the renderer's own confirmations, by the action a click carried.
    fn own_answer(&mut self, pending: Pending, action: HitAction) {
        match (pending, action) {
            (Pending::Paste { text, .. }, HitAction::Paste) => {
                self.editor.insert_str(&text);
                self.sync_menu();
            }
            (Pending::ClearDraft, HitAction::ClearDraft) => self.editor.clear(),
            (Pending::Exit, HitAction::Quit) => self.quit = true,
            // `Dismiss`, and any pairing that cannot arise, is the safe answer: the
            // question closes and nothing happens, which is what `Esc` does.
            _ => {}
        }
    }

    /// Act on a click or a wheel notch while the questionnaire owns the input area.
    fn questionnaire_click(&mut self, click: QuestionClick) {
        let regions = self.regions.clone();
        let mut submitted = false;
        let Some(Pending::Questionnaire(questionnaire)) = self.pending.as_mut() else {
            return;
        };
        match click {
            // The wheel moves the option window, which follows the highlight — so a
            // notch is exactly one option's worth of movement (票 04 §4).
            QuestionClick::Wheel(up) => questionnaire.move_highlight(if up { -1 } else { 1 }),
            QuestionClick::At(column, row) => {
                // The option rows and the free-text row are recorded by screen row;
                // the footer's buttons are recorded as rectangles like every other
                // button, so all of them are looked up through the same table
                // (票 04 §1).
                if let Some(option) = regions.option_at(row) {
                    questionnaire.select_option(option);
                    return;
                }
                if regions.custom_at(row) {
                    // Clicking the free-text row hands it the cursor; the keyboard
                    // focus otherwise stays where the last key left it (票 04 §5).
                    questionnaire.focus_custom();
                    return;
                }
                match regions.action_at(column, row) {
                    Some(HitAction::Previous) => {
                        questionnaire.back();
                        questionnaire.unfocus_custom();
                    }
                    Some(HitAction::Next) => {
                        questionnaire.advance();
                        questionnaire.unfocus_custom();
                    }
                    Some(HitAction::Submit) => {
                        // The button submits exactly as `Enter` does once everything
                        // is handled, so it goes through the same path that drops the
                        // takeover and answers the tool (票 04 §4).
                        if questionnaire.all_handled() {
                            submitted = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        if submitted {
            self.questionnaire_key(Key::Enter);
        }
    }

    /// The unit the viewport's top row belongs to — the rail's bright cell.
    ///
    /// A **derived** quantity, deliberately: the viewport is the state, and a stored
    /// "current unit" would drift the first time anything arrived or the reader
    /// scrolled. At the bottom it is the newest unit, which is what "I am following the
    /// conversation" means even when the whole transcript fits on one screen
    /// (spec §4).
    fn focused_unit(&self) -> Option<usize> {
        let units = self.rail.units();
        if units == 0 {
            return None;
        }
        if self.pane.following() {
            return Some(units - 1);
        }
        let source = self.pane.source_at(self.pane.top())?;
        Some(self.rail.unit_of(source).min(units - 1))
    }

    /// Jump to the start of a unit: what clicking its cell does.
    ///
    /// The landing is **top-aligned**, so every jump lands where the eye expects; the
    /// newest unit clamps to the bottom instead, which is the same rule read at the
    /// end of the transcript rather than a special case (spec §4).
    fn jump_to_unit(&mut self, unit: usize) {
        let Some(head) = self.rail.head(unit) else {
            return;
        };
        self.pane.scroll_to_source(head);
    }

    /// The clickable link a click landed on, as a copy of what it opens.
    ///
    /// The width the overlay will open at comes from the last frame, which is the
    /// only place the middle block's geometry is known (票 04 §1).
    fn link_hit(&self, mouse: &MouseEvent) -> Option<Detail> {
        let offset = (mouse.row.checked_sub(self.drawn_top)?) as usize;
        let row = (*self.drawn_rows.get(offset)?)?;
        self.links.get(row)?.clone()
    }

    /// Whether a click landed on the "back to bottom" indicator.
    fn indicator_hit(&self, column: u16, row: u16) -> bool {
        self.indicator.is_some_and(|rect| {
            column >= rect.x
                && column < rect.x.saturating_add(rect.width)
                && row >= rect.y
                && row < rect.y.saturating_add(rect.height)
        })
    }

    /// Answer a request from the loop.
    pub fn request(&mut self, request: ConsoleRequest) {
        self.dirty = true;
        match request {
            ConsoleRequest::Prompt { reply } => self.prompt_reply = Some(reply),
            // The loop's own account of whether it is running something. Nothing else
            // in this state may stand in for it.
            ConsoleRequest::RunState { running } => {
                self.running = running;
                // A question belongs to the run that raised it, so the end of that run is
                // what makes it stale: the loop is no longer waiting for an answer, and
                // its ask died with the run. Leaving the overlay up would send the next
                // keypress to a question nobody is waiting for — a silent failure that
                // reads as a dead key (spec §6, §9). Dropping the sender is the honest
                // reading of "no one is waiting": a held question would be denied.
                //
                // Only the loop's questions go with the run. The renderer's own — an
                // oversized paste, a draft `Esc` would clear — are not the run's to
                // withdraw, and they can only be up while the loop is idle anyway.
                // The questionnaire is the loop's too: it is the model's ask, and a
                // cancelled run leaves it with no one waiting and no answer to give.
                if !running
                    && matches!(
                        self.pending,
                        Some(Pending::Loop { .. } | Pending::Questionnaire(_))
                    )
                {
                    self.pending = None;
                }
            }
            ConsoleRequest::Ask(ask) => {
                // A question may not be drawn over the detail overlay: the overlay is
                // not a `pending`, so nothing else would stand it down, and the modal
                // underneath would be unanswerable (票 02 §4).
                self.close_detail();
                if self.pending.is_some() {
                    // The loop asks one question at a time and waits for the answer, so
                    // this cannot happen. If it ever did, dropping the *new* question
                    // keeps the one on screen answerable; dropping its sender denies it,
                    // which is the safe reading of an orphaned ask.
                    return;
                }
                self.pending = Some(Pending::Loop {
                    question: ask.question,
                    reply: ask.reply,
                });
            }
            ConsoleRequest::Questionnaire(request) => {
                // Same reason as `Ask`: the overlay stands down for the question.
                self.close_detail();
                if self.pending.is_some() {
                    // One question owns the keyboard at a time, exactly as for the
                    // loop's asks: dropping the new one keeps the one on screen
                    // answerable, and its dropped sender denies the orphaned ask.
                    return;
                }
                // The tool refuses an empty questionnaire before it reaches a port,
                // so this cannot come from the model. A question with no questions
                // would have nothing to draw and nothing to index, so it is refused
                // rather than allowed to panic the renderer.
                if request.questions.is_empty() {
                    let _ = request
                        .reply
                        .send(Err("a questionnaire needs at least one question".to_owned()));
                    return;
                }
                let drafts = request
                    .questions
                    .iter()
                    .map(|_| QuestionDraft::default())
                    .collect();
                self.pending = Some(Pending::Questionnaire(Questionnaire {
                    questions: request.questions,
                    reply: request.reply,
                    drafts,
                    index: 0,
                    custom_focused: false,
                }));
            }
            // The names the loop can act on. They arrive once, after assembly — the
            // skills come from the session — and nothing else carries them.
            ConsoleRequest::Catalog { entries } => self.catalog = entries,
            // The history a reopened session assembled with. An empty stream is not a
            // replay: entering the state would show a progress line for an operation
            // that lays nothing down and marks no seam (`spec` §2).
            ConsoleRequest::Replay { events } => {
                if !events.is_empty() {
                    self.replay = Some(Replay {
                        events,
                        next: 0,
                        lines: 0,
                    });
                }
            }
        }
    }

    pub fn take_events(&mut self) -> Vec<FrontEndEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// Take one broadcast receive. `true` means the channel is gone.
    fn take_render_event(
        &mut self,
        received: Result<RenderEvent, broadcast::error::RecvError>,
    ) -> bool {
        match received {
            Ok(event) => {
                self.live_event(event);
                false
            }
            // A dropped renderer delta degrades output, never correctness, so it is
            // narrated like any other live event — and buffered during a replay for the
            // same reason the banner is.
            Err(broadcast::error::RecvError::Lagged(dropped)) => {
                self.live_event(RenderEvent::Diagnostic(wording::renderer_dropped(dropped)));
                false
            }
            Err(broadcast::error::RecvError::Closed) => true,
        }
    }

    /// Take one terminal event.
    fn terminal_event(&mut self, event: Option<std::io::Result<CtEvent>>) {
        match event {
            Some(Ok(CtEvent::Key(key))) => {
                if key.kind == KeyEventKind::Press {
                    if let Some(key) = map_key(key) {
                        self.key(key);
                    }
                }
            }
            Some(Ok(CtEvent::Paste(text))) => self.paste(&text),
            Some(Ok(CtEvent::Mouse(mouse))) => self.mouse(mouse),
            // A resize is a repaint, and the replay keeps batching: the next frame is
            // laid out at the new size (`.scratch/tui-history-replay/spec.md` §5).
            Some(Ok(CtEvent::Resize(..))) => self.mark_dirty(),
            _ => {}
        }
    }

    /// Take one request from the loop. `true` means the port is gone.
    fn port_request(&mut self, request: Option<ConsoleRequest>) -> bool {
        match request {
            Some(request) => {
                self.request(request);
                false
            }
            None => true,
        }
    }

    /// The model's questionnaire, while it owns the bottom input area.
    fn questionnaire(&self) -> Option<&Questionnaire> {
        match &self.pending {
            Some(Pending::Questionnaire(questionnaire)) => Some(questionnaire),
            _ => None,
        }
    }

    /// Feed one keypress to the questionnaire.
    ///
    /// The takeover stays up while the questionnaire has questions left; the one
    /// keypress that submits it drops it, which is what hands the bottom input
    /// area back to the resident editor.
    fn questionnaire_key(&mut self, key: Key) {
        let Some(Pending::Questionnaire(mut questionnaire)) = self.pending.take() else {
            return;
        };
        if questionnaire.press(key) {
            let answers = questionnaire.answers();
            let _ = questionnaire.reply.send(Ok(answers));
        } else {
            self.pending = Some(Pending::Questionnaire(questionnaire));
        }
    }

    /// Handle one keypress. Answers and submissions go out through the pending
    /// one-shot channels; gestures are queued for the loop.
    pub fn key(&mut self, key: Key) {
        self.dirty = true;
        // A replay owns the keyboard before anything else does — including the detail
        // overlay, which cannot be open this early — because its boundaries are its
        // own: `Ctrl-C` quits, `Ctrl-D` and `Esc` are inert, and the editor still
        // works (`spec` §5).
        if self.replay.is_some() {
            self.replay_key(key);
            return;
        }
        // The detail overlay is a view mode of its own: it owns the keyboard while it
        // is up, and the transcript underneath is frozen where the reader left it
        // (票 02 §4).
        if self.detail_open() {
            match key {
                // The one exception to "everything else is ignored": `Ctrl-D` closes
                // the overlay rather than quitting, let alone asking (票 06 §5).
                // `Ctrl-C` is **not** an exception: it is one of the ignored keys
                // (票 02 §4).
                Key::Esc | Key::CtrlD => self.close_detail(),
                Key::Up => self.detail_scroll(-1),
                Key::Down => self.detail_scroll(1),
                Key::PageUp => self.detail_scroll(-(self.detail_page() as isize)),
                Key::PageDown => self.detail_scroll(self.detail_page() as isize),
                _ => {}
            }
            return;
        }
        match key {
            Key::CtrlC => {
                if self.busy() {
                    self.events.push(FrontEndEvent::Cancel);
                } else {
                    self.quit = true;
                }
                return;
            }
            // `Ctrl-D` is the quit-with-a-confirmation gesture, and every one of its
            // guards comes before the question guard below: while the loop is running
            // it is ignored outright, and while any question is up it is that
            // question's to ignore (票 06 §1, §3).
            Key::CtrlD => {
                if !self.busy() && self.pending.is_none() {
                    self.pending = Some(Pending::Exit);
                }
                return;
            }
            Key::Esc => {
                if self.busy() {
                    self.events.push(FrontEndEvent::Cancel);
                } else if let Some(pending) = self.pending.take() {
                    self.decline(pending);
                } else if self.slash_menu().is_some() {
                    // The `/` menu is the smallest thing on screen, so `Esc` closes it
                    // before it starts throwing away a draft (spec §6).
                    self.slash.dismissed = true;
                } else if self.editor.has_multiple_lines() {
                    // Esc on a draft this long would throw away real work, so it
                    // asks first — and the safe answer is "no" (spec §7).
                    self.pending = Some(Pending::ClearDraft);
                } else {
                    self.editor.clear();
                }
                return;
            }
            _ => {}
        }
        if self.pending.is_some() {
            // A question owns the keyboard: its own keys answer it, `Ctrl-C` and `Esc`
            // above are the ways out, and nothing else gets through — not a stray
            // character, not the plan-mode gesture (spec §9).
            //
            // The questionnaire answers to a wider keyboard than the one-key
            // questions — the arrows move and page, `Tab` skips, `Enter`/`Space`
            // confirm — so it gets every key and routes its own. The other kinds
            // keep the narrow rule, which is what makes a stray character unable
            // to allow a write.
            if matches!(self.pending, Some(Pending::Questionnaire(_))) {
                self.questionnaire_key(key);
            } else if matches!(key, Key::Char(_) | Key::Enter) {
                self.answer_key(key);
            }
            return;
        }
        if key == Key::BackTab {
            self.events.push(FrontEndEvent::TogglePlan);
            return;
        }
        // While the `/` menu is up it owns the four keys that would otherwise edit or
        // submit: `↑`/`↓` walk the matches, `Tab` fills one in, `Enter` fills one in and
        // sends it. Everything else falls through to the editor, which is what filters
        // the matches as the user keeps typing.
        if self.slash_menu().is_some() {
            match key {
                Key::Down => {
                    self.menu_move(1);
                    return;
                }
                Key::Up => {
                    self.menu_move(-1);
                    return;
                }
                Key::Tab | Key::Enter => {
                    // `Tab` fills in the highlighted name and stops there; `Enter` fills
                    // it in **and submits**, so `/ask` + Enter runs the skill the menu
                    // was pointing at. A bare `/` has nothing highlighted: `Enter` sends
                    // it as typed, and the loop answers with the list of names.
                    self.menu_accept();
                    if key == Key::Enter {
                        self.submit();
                    }
                    return;
                }
                _ => {}
            }
        }
        match key {
            Key::Enter => self.submit(),
            Key::PageUp => self.pane.page(true),
            Key::PageDown => self.pane.page(false),
            Key::CtrlG => self.pane.to_bottom(),
            // Every other key the editor answers to; the two paths share them.
            _ => self.editor_key(key),
        }
        // A key that changed the draft (or only moved the cursor inside the token) may
        // have widened or narrowed the menu. Fold that in once, here, rather than at
        // each of the arms above.
        self.sync_menu();
    }

    /// Whether the loop is **inside a run**: a turn, or a discussion it is driving.
    ///
    /// The loop says so over [`ConsoleRequest::RunState`]; nothing here infers it. Two
    /// inferences both failed. From the render stream: only `TurnEnded` cleared the old
    /// flag, and the synthesizer's single call ends no turn, so after a discussion the
    /// TUI believed it was working for ever. From "no prompt is outstanding": that
    /// predicate is true before the loop asks its *first* line, so a keyboard that was
    /// idle during assembly read as working. Both mistakes turned `Ctrl-C` into a cancel
    /// gesture the idle loop discards — a dead keyboard.
    fn busy(&self) -> bool {
        self.running
    }

    /// Send the typed draft to the loop and remember it.
    ///
    /// Submitting also returns the transcript to the bottom: the user has just asked
    /// for something and wants to watch the answer, whatever they were reading
    /// (spec §4).
    ///
    /// An empty draft is sent as an **empty line**. The channel's sentinel for a closed
    /// stdin is `None` (see [`ConsoleRequest::Prompt`]), and pressing Enter never means
    /// that; quitting is `Ctrl-C` (the flag below) or `/quit` (a line like any other).
    ///
    /// With **no line being read** — a turn in flight, or a one-shot `discuss`, which
    /// never asks for one — Enter does nothing at all rather than throwing the draft
    /// away: the loop asks for a line when it is ready for one (spec §6), and until
    /// then that draft is the only copy of what the user typed.
    fn submit(&mut self) {
        // A replay is not a conversation: `Enter` must not fire a turn into the middle
        // of history. The draft stays exactly where it is, and the key is simply not
        // the submit it looks like (`spec` §3).
        if self.replay.is_some() {
            return;
        }
        let Some(reply) = self.prompt_reply.take() else {
            return;
        };
        self.pane.to_bottom();
        let line = self.editor.submitted();
        let _ = reply.send(Some(line));
    }

    /// The `/` menu as the draft calls for it right now, or `None` when there is
    /// nothing to offer.
    ///
    /// Derived, never stored: the draft and the loop's catalog are the whole input.
    /// Nothing is shown while a question is up, because a question owns the keyboard
    /// — a menu would be offering keys that answer something else (spec §9).
    fn slash_menu(&self) -> Option<SlashMenu> {
        if self.pending.is_some() || self.slash.dismissed {
            return None;
        }
        let token = self.editor.slash_token()?;
        // Filter on case, but offer the name as it is catalogued: `/Ask` finds
        // `ask-matt`, and Tab writes the spelling the loop will recognise.
        let typed = token.prefix.to_lowercase();
        let entries: Vec<(String, String)> = self
            .catalog
            .iter()
            .filter(|entry| entry.name.to_lowercase().starts_with(&typed))
            .map(|entry| (entry.name.clone(), entry.description.clone()))
            .collect();
        if entries.is_empty() {
            return None;
        }
        Some(SlashMenu {
            prefix: token.prefix,
            selected: self.slash.selected.map(|at| at.min(entries.len() - 1)),
            entries,
        })
    }

    /// Move the highlight by `delta`, wrapping at both ends.
    fn menu_move(&mut self, delta: isize) {
        let Some(menu) = self.slash_menu() else {
            return;
        };
        let len = menu.entries.len() as isize;
        self.slash.selected = Some(match menu.selected {
            Some(at) => (at as isize + delta).rem_euclid(len) as usize,
            // Nothing was highlighted: `↓` takes the first row and `↑` the last, so the
            // arrows walk the list in the order it is drawn.
            None if delta > 0 => 0,
            None => (len - 1) as usize,
        });
        self.slash.prefix = menu.prefix;
    }

    /// Fill the highlighted name into the draft.
    ///
    /// Does nothing when nothing is highlighted — a bare `/` is a list to look at, not
    /// a choice that has been made.
    fn menu_accept(&mut self) {
        let Some(menu) = self.slash_menu() else {
            return;
        };
        let Some(selected) = menu.selected else {
            return;
        };
        let name = menu.entries[selected].0.clone();
        if !self.editor.complete_slash(&name) {
            return;
        }
        // The remembered prefix moves with the draft, or the next sync would read the
        // fill-in as a change and reopen what this just closed.
        self.slash.prefix = self
            .editor
            .slash_token()
            .map(|token| token.prefix)
            .unwrap_or_default();
        self.slash.selected = None;
        self.slash.dismissed = true;
    }

    /// Fold the draft's current token into the menu's remembered selection.
    ///
    /// A prefix that changed is a different menu: the highlight starts over — on the
    /// first match once a name has been typed, on nothing at all while the token is
    /// just a slash — and an `Esc` that closed the old one stops applying.
    fn sync_menu(&mut self) {
        let prefix = self
            .editor
            .slash_token()
            .map(|token| token.prefix)
            .unwrap_or_default();
        if prefix != self.slash.prefix {
            self.slash.selected = (!prefix.is_empty()).then_some(0);
            self.slash.prefix = prefix;
            self.slash.dismissed = false;
        }
    }

    /// Answer a question with a keypress.
    ///
    /// The loop's questions have their own vocabularies, and an unrecognised key
    /// falls back to the non-acting answer, so a stray character can never allow a
    /// write. The renderer's own questions take `y` (or Enter) and nothing else.
    fn answer_key(&mut self, key: Key) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.answer_pending(pending, key);
    }

    /// The body of [`TuiState::answer_key`], for the caller that already holds the
    /// question — the pointer path, which has to inspect it before answering it.
    fn answer_pending(&mut self, pending: Pending, key: Key) {
        match pending {
            Pending::Loop { question, reply } => {
                let choice = match (&question, key) {
                    (Question::Permission(_), Key::Char('y')) => {
                        AnswerChoice::Permission(crate::permissions::Answer::Allow)
                    }
                    (Question::Permission(_), Key::Char('a')) => {
                        AnswerChoice::Permission(crate::permissions::Answer::AlwaysAllow)
                    }
                    (Question::PlanConflict(_), Key::Char('o')) => {
                        AnswerChoice::Plan(crate::permissions::PlanConflict::Overwrite)
                    }
                    (Question::PlanConflict(_), Key::Char('a')) => {
                        AnswerChoice::Plan(crate::permissions::PlanConflict::Append)
                    }
                    (Question::PlanConflict(_), Key::Char('k')) => {
                        AnswerChoice::Plan(crate::permissions::PlanConflict::Keep)
                    }
                    _ => default_choice(&question),
                };
                let _ = reply.send(choice);
            }
            Pending::Paste { text, .. } => {
                if agrees(key) {
                    self.editor.insert_str(&text);
                    self.sync_menu();
                }
            }
            Pending::ClearDraft => {
                if agrees(key) {
                    self.editor.clear();
                }
            }
            // Yes quits; every other reachable key is the safe answer — "no" — and
            // so is `Esc`, which never gets here (票 06 §2).
            Pending::Exit => {
                if agrees(key) {
                    self.quit = true;
                }
            }
            // Unreachable: `key` routes a questionnaire to `questionnaire_key`
            // before this, because it answers to a wider keyboard. Dropping it
            // here would refuse the tool, so it is only kept to keep the match
            // total.
            Pending::Questionnaire(_) => {}
        }
    }

    /// What `Esc` means for a question: the non-acting answer, or nothing at all
    /// when the question was this renderer's own.
    fn decline(&mut self, pending: Pending) {
        match pending {
            Pending::Loop { question, reply } => {
                let _ = reply.send(default_choice(&question));
            }
            // A questionnaire belongs to a run, so `Esc` while one is up is the
            // cancel gesture and never reaches here (spec §19). If it ever did,
            // dropping the sender is the honest "no answer". The exit confirmation
            // is the renderer's own and `Esc` is its safe answer: decline, stay in.
            Pending::Questionnaire(_)
            | Pending::Paste { .. }
            | Pending::ClearDraft
            | Pending::Exit => {}
        }
    }

    fn status_line(&self, width: u16) -> String {
        // A replay speaks for itself: it temporarily replaces the hint set with its
        // own progress, and there is no exit hint to give — `Ctrl-D` is ignored and
        // the exit `Ctrl-C` may or may not perform is not a hint a person needs
        // (`spec` §4).
        if let Some(replay) = &self.replay {
            return wording::history_progress_line(replay.next, replay.events.len(), width);
        }
        // The hints describe what the keyboard does *now*. With no line being read —
        // a turn in flight, or a one-shot `discuss` — `enter 发送` would be a promise
        // this session does not keep (spec §6).
        if self.prompt_reply.is_some() {
            wording::status_line(self.busy(), width)
        } else {
            wording::viewer_status_line(self.busy(), width)
        }
    }

    /// How many content rows the bottom block wants this frame.
    ///
    /// The resident editor's draft decides it normally; while a questionnaire owns
    /// the input area, the questionnaire does — that is what makes the bottom block
    /// grow to hold the question and its options (spec §19).
    fn bottom_rows(&self, area: Rect) -> u16 {
        match self.questionnaire() {
            Some(questionnaire) => questionnaire_lines(
                &questionnaire.questions[questionnaire.index],
                &questionnaire.drafts[questionnaire.index],
                layout::content_width(area) as usize,
            )
            .len() as u16,
            None => self.editor.height(layout::input_text_width(area)),
        }
    }
}

/// One question's rows: its header, its text, its numbered options, and the line
/// an answer is typed on.
///
/// The length of this is what [`TuiState::bottom_rows`] asks the layout for, so
/// it is the **full** list; the painter clips and scrolls it through
/// [`questionnaire_window`]. The options are numbered for reading only — the
/// decided keyboard has no digit keys — and a recommended option gets a display
/// badge while its underlying label, the value the answer carries, is left
/// untouched (spec §7).
fn questionnaire_lines(
    question: &UserQuestion,
    draft: &QuestionDraft,
    width: usize,
) -> Vec<Line<'static>> {
    let (mut rows, options, custom) = questionnaire_parts(question, draft, width);
    rows.extend(options);
    rows.push(custom);
    rows
}

/// The rows of one question that fit in `height`, scrolling the option window so
/// the highlighted option is always visible (spec §7).
///
/// The header and the question text are pinned: they say what is being asked, so
/// losing them to a scroll would make the options unreadable. The typed-answer
/// line is pinned at the bottom for the same reason. The options in between are
/// the window, and it follows the highlight: moving down past the clip scrolls
/// the tail into view instead of leaving the highlight off screen.
fn questionnaire_window(
    question: &UserQuestion,
    draft: &QuestionDraft,
    width: usize,
    height: usize,
) -> Vec<Line<'static>> {
    let (prefix, options, custom) = questionnaire_parts(question, draft, width);
    if prefix.len() + options.len() < height {
        let mut rows = prefix;
        rows.extend(options);
        rows.push(custom);
        return rows;
    }
    // The prefix and the answer line are reserved; whatever is left is the
    // window. A degenerate terminal with no room for either simply shows the
    // prefix, which is the part that must not be lost.
    let room = height.saturating_sub(prefix.len() + 1);
    let start = option_window_start(draft.highlight, options.len(), room);
    let mut rows = prefix;
    rows.extend(options.into_iter().skip(start).take(room));
    rows.push(custom);
    rows.truncate(height);
    rows
}

/// The first option to draw so that `highlight` is inside a window of `room`
/// options. The list does not wrap: once the highlight is past the window the
/// window follows it one row at a time.
fn option_window_start(highlight: usize, count: usize, room: usize) -> usize {
    if room == 0 || count <= room {
        return 0;
    }
    let start = if highlight < room {
        0
    } else {
        highlight + 1 - room
    };
    start.min(count - room)
}

/// Split one question into its pinned prefix (header and text), its option rows,
/// and the typed-answer row.
///
/// The split exists for the scrolling window; the composition of each row lives
/// here once, so the full list and the window cannot disagree about what an
/// option reads as.
fn questionnaire_parts(
    question: &UserQuestion,
    draft: &QuestionDraft,
    width: usize,
) -> (Vec<Line<'static>>, Vec<Line<'static>>, Line<'static>) {
    let mut prefix: Vec<Line<'static>> = Vec::new();
    if let Some(header) = question
        .header
        .as_deref()
        .map(str::trim)
        .filter(|header| !header.is_empty())
    {
        for mut row in pane::wrap_text(header, width) {
            row.style = Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD);
            prefix.push(row);
        }
    }
    let mut title = question.question.clone();
    if question.multi_select {
        title.push_str(wording::questionnaire_multi_marker());
    }
    prefix.extend(pane::wrap_text(title.trim(), width));

    let mut options: Vec<Line<'static>> = Vec::with_capacity(question.options.len());
    for (index, choice) in question.options.iter().enumerate() {
        let highlighted = index == draft.highlight;
        let picked = draft
            .selected
            .iter()
            .any(|selected| selected == &choice.label);
        let marker = match (question.multi_select, picked) {
            (true, true) => "[x]",
            (true, false) => "[ ]",
            (false, true) => "●",
            (false, false) => "○",
        };
        // The cursor says which option `Enter`/`Space` would confirm; the marker
        // says which are picked. They are different facts and can differ.
        let cursor = if highlighted { ">" } else { " " };
        let text = format!(
            "{cursor} {marker} {}",
            wording::questionnaire_option(index + 1, &choice.label, choice.description.as_deref())
        );
        let mut style = if picked {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        if highlighted {
            style = style.add_modifier(Modifier::REVERSED);
        }
        options.push(Line::from(Span::styled(
            truncate_columns(&text, width),
            style,
        )));
    }

    let label = if question.options.is_empty() {
        wording::questionnaire_answer_label()
    } else {
        wording::questionnaire_custom_label()
    };
    let room = width.saturating_sub(text_columns(label));
    let custom = Line::from(vec![
        Span::styled(label, Style::default().fg(Color::DarkGray)),
        Span::raw(truncate_columns(&draft.custom, room)),
    ]);
    (prefix, options, custom)
}

/// Draw one frame of the shell.
///
/// This is the seam the layout is tested through: a state goes in, a fixed-size
/// frame comes out, and no terminal is involved (spec §2).
pub fn draw_frame(frame: &mut ratatui::Frame, state: &mut TuiState) {
    let area = frame.area();
    // The pointer is answered between frames, and opening a detail needs the width
    // this frame was drawn at.
    state.area = area;
    // Whatever the last frame recorded is what the pointer could hit; this frame
    // starts from nothing and records only what it really paints (票 04 §1).
    state.regions.clear();
    if layout::below_minimum(area) {
        // Nothing is drawn that a click could land on.
        state.indicator = None;
        state.detail_rect = None;
        draw_too_small(frame, area);
        return;
    }
    // The draft's own height decides how much room the input takes: it grows with
    // the text up to the layout's cap and then scrolls internally (spec §2). A
    // questionnaire replaces that with its own height, so the input area grows to
    // hold the question (spec §19).
    let content_rows = state.bottom_rows(area);
    let panes = layout::plan(area, content_rows);
    draw_shell(frame, &panes, state, area);
    draw_transcript(frame, &panes, state);
    draw_status(frame, &panes, state);
    let anchor = draw_bottom(frame, &panes, state);
    // The `/` menu floats over the main column, under the cursor it belongs to — and
    // under a question, which owns the keyboard and so has no menu to offer
    // (spec §6, §9).
    if let Some(anchor) = anchor {
        draw_menu(frame, &panes, state, anchor);
    }
    // Last, so it is on top of the transcript it is asking about.
    draw_modal(frame, &panes, state);
    // The detail overlay goes over all of it. It cannot be up at the same time as a
    // question — opening one needs an idle keyboard — so the order between the two
    // is a formality (票 02 §4).
    draw_detail(frame, &panes, state);
}

/// The parts of the shell that are not regions of their own: the frame, the divider
/// column, the sidebar and the main column's three rules.
///
/// The order is the painting order and it is why the junctions come out whole: the
/// frame first, then the divider down the sidebar's right edge, then the sidebar —
/// whose tab bar writes `├` and `┤` over the two of them — and last the main
/// column's rules, which write `├` into the divider column at their own rows
/// (spec §1).
fn draw_shell(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &mut TuiState,
    area: Rect,
) {
    draw_border(frame, area);
    draw_divide(frame, panes, area);
    draw_sidebar(frame, panes, state);
    // The rules above the status row, the input and the hints. Left of them is the
    // divider — or, with no sidebar, the frame's own left border — and right of them
    // the frame's right border.
    let style = Style::default().fg(Color::DarkGray);
    let right = area.right().saturating_sub(1);
    for y in [panes.status.y - 1, panes.input.y - 1, panes.hints.y - 1] {
        let left = panes.divide.unwrap_or(area.x);
        let buffer = frame.buffer_mut();
        buffer[(left, y)].set_symbol("├").set_style(style);
        for x in left + 1..right {
            buffer[(x, y)].set_symbol("─").set_style(style);
        }
        buffer[(right, y)].set_symbol("┤").set_style(style);
    }
}

/// The column the sidebar and the main column share: one vertical rule from the
/// frame's top border to its bottom one, with the frame's own junctions at the ends
/// rather than a second border (spec §1).
///
/// The tab bar paints its own junctions over it at the two rows its rules occupy.
fn draw_divide(frame: &mut ratatui::Frame, panes: &layout::Regions, area: Rect) {
    let Some(divide) = panes.divide else {
        return;
    };
    let style = Style::default().fg(Color::DarkGray);
    let buffer = frame.buffer_mut();
    for y in area.y..area.bottom() {
        let symbol = if y == area.y {
            "┬"
        } else if y == area.bottom() - 1 {
            "┴"
        } else {
            "│"
        };
        buffer[(divide, y)].set_symbol(symbol).set_style(style);
    }
}

/// The sidebar: the mark or the text identity at the top, then the tab bar, then
/// the page the tab selects (spec §3).
///
/// Everything here is drawn from the layout's decisions — which identity, which
/// page rows — never from a size test of its own, so the ladder has one home. The tab
/// labels record a hit rectangle each as they are painted, so a click can only land on
/// a tab that is really on screen.
fn draw_sidebar(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let (Some(sidebar), Some(tabs)) = (panes.sidebar, panes.tabs) else {
        return;
    };
    let dim = Style::default().fg(Color::DarkGray);
    match panes.sidebar_kind {
        layout::SidebarKind::Mark => {
            // The mark is 38 columns and the wide rung is 40, so it sits centred
            // with a column of air on each side; a rung narrower than the mark never
            // asks for these rows at all (spec §2).
            let offset = sidebar.width.saturating_sub(layout::LOGO_WIDTH) / 2;
            let lines: Vec<Line<'static>> = mark_lines()
                .into_iter()
                .map(|(text, color)| {
                    Line::from(Span::styled(text.to_owned(), Style::default().fg(color)))
                })
                .collect();
            let rows = lines.len() as u16;
            frame.render_widget(
                Paragraph::new(lines),
                Rect::new(
                    sidebar.x + offset,
                    sidebar.y,
                    sidebar.width.saturating_sub(offset),
                    rows,
                ),
            );
        }
        layout::SidebarKind::Text => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(wording::identity(), dim))),
                Rect::new(sidebar.x, sidebar.y, sidebar.width, 1),
            );
        }
        layout::SidebarKind::Hidden => {}
    }
    // The tab bar: two rules with the labels between them. Both rules start at the
    // frame's left border and end at the divider column, so the sidebar reads as one
    // compartment rather than as a block of its own (spec §3).
    for y in [tabs.y - 1, tabs.y + 1] {
        let buffer = frame.buffer_mut();
        buffer[(sidebar.x - 1, y)].set_symbol("├").set_style(dim);
        for x in sidebar.x..sidebar.right() {
            buffer[(x, y)].set_symbol("─").set_style(dim);
        }
        if let Some(divide) = panes.divide {
            buffer[(divide, y)].set_symbol("┤").set_style(dim);
        }
    }
    // The labels, one separator between them and the rest of the row filled with a
    // rule, so the row reads as a bar rather than as three stranded words.
    let entries = [
        (Tab::Usage, wording::TAB_USAGE),
        (Tab::Trace, wording::TAB_TRACE),
        (Tab::Files, wording::TAB_FILES),
    ];
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0u16;
    for (index, (tab, label)) in entries.iter().enumerate() {
        let selected = *tab == state.tab;
        let style = if selected {
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD)
        } else {
            dim
        };
        let width = text_columns(label) as u16;
        // One region per label's own text, recorded as it is painted: the pointer can
        // only hit what is really there, and the rule that fills the rest of the row is
        // not a tab (spec §3).
        if used + width <= sidebar.width {
            state.regions.cells.push(Region {
                rect: Rect::new(tabs.x + used, tabs.y, width, 1),
                action: HitAction::SwitchTab(*tab),
            });
        }
        spans.push(Span::styled((*label).to_owned(), style));
        used += width;
        if index + 1 < entries.len() {
            spans.push(Span::styled("│", dim));
            used += 1;
        }
    }
    spans.push(Span::styled(
        "─".repeat(sidebar.width.saturating_sub(used) as usize),
        dim,
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), tabs);
    // The page itself. The rows come from the layout's height ladder, so a squeezed
    // sidebar loses fields from the tail rather than clipping the three readings that
    // matter (spec §2). A page that is not built yet says so in one row rather than
    // showing made-up data.
    let Some(page) = panes.sidebar_page else {
        return;
    };
    let rows = match state.tab {
        Tab::Usage => state.panel.lines(&state.facts, page),
        Tab::Trace | Tab::Files => vec![Line::from(Span::styled(
            truncate_columns(wording::tab_placeholder(), page.width as usize),
            dim,
        ))],
    };
    frame.render_widget(Paragraph::new(rows), page);
}

/// The status row: which model, which mode, and how full the window is (spec §5).
///
/// The three segments are painted, not clickable: nothing on this row is a control,
/// so nothing here records a hit region. The width ladder lives in
/// [`wording::status_row`]; the row itself is always drawn, and a width too narrow
/// even for its last rung is truncated rather than dropped (spec §2).
fn draw_status(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &TuiState) {
    let share = wording::context_share(state.panel.last_input(), state.facts.context_window);
    let width = panes.status.width as usize;
    let text = wording::status_row(
        &state.facts.model,
        &wording::mode_field(state.mode),
        &share,
        width,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_columns(&text, width),
            Style::default().fg(Color::DarkGray),
        ))),
        panes.status,
    );
}

/// Which sidebar page is showing (spec §3).
///
/// Clicked, never keyed: `Tab` belongs to the `/` menu and `Shift+Tab` to plan mode,
/// and this repo does not enable the keyboard-enhancement protocol. A page that is
/// not built yet shows [`wording::tab_placeholder`] rather than made-up data.
///
/// The accepted cost of that: on a placeholder page the session's readings are not on
/// screen at all, so the status row's `上下文 n%` is the only one left. It is not a
/// bug — there is no second copy of the numbers to fall back on — and the alternative
/// (a keyboard route through the tabs) is not available here anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    /// The session's readings: what the old information panel held.
    Usage,
    /// The call trace. Not built yet.
    Trace,
    /// The files this session touched. Not built yet.
    Files,
}

/// The overlay a question is asked in (spec §9).
///
/// It sits in the middle of the main column so the question cannot be outrun by new
/// output, and it is **not** part of the transcript: the stream still carries the
/// `PermissionAsked` block for anyone reading back. Centring it on the main column
/// rather than the whole terminal keeps the sidebar's readings visible while a
/// question is up (spec §1). It owns the pointer while it is up, so the "back to
/// bottom" rectangle is dropped.
fn draw_modal(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let Some(modal) = state.pending.as_ref().and_then(Pending::modal) else {
        return;
    };
    // From here on a question is up, and the overlay covers the indicator: a click
    // where it used to be must not act, even if the overlay itself turns out to have
    // no room to be drawn.
    state.indicator = None;
    let inner = panes.modal_width().saturating_sub(2) as usize;
    if inner == 0 {
        return;
    }
    // The rows, in order. Anything long wraps onto another line rather than losing
    // the keys; the overlay still leaves the middle block's own borders showing.
    let rows_available = panes.main.height.saturating_sub(2) as usize;
    if rows_available == 0 {
        return;
    }
    let mut rows: Vec<Line<'static>> = pane::wrap_text(modal.title.trim(), inner);
    for row in &mut rows {
        row.style = Style::default().add_modifier(Modifier::BOLD);
    }
    // What the action is, then the call itself: the sentence a reader can act on
    // first, the exact arguments under it.
    // What the call is for, then the call as it will run: orientation, then the thing
    // being approved.
    if let Some(description) = modal.description.as_deref() {
        rows.extend(pane::wrap_text(description.trim(), inner));
    }
    if let Some(detail) = modal.detail.as_deref() {
        rows.extend(pane::wrap_text(detail.trim(), inner));
    }
    // The button row is budgeted first: it is the one row a question cannot do
    // without. The blank that sets it apart costs a row too, but only when there is
    // both room for it and something above to separate it from.
    let separator = usize::from(rows_available >= 3 && !rows.is_empty());
    rows.truncate(rows_available.saturating_sub(1 + separator));
    if separator == 1 {
        rows.push(Line::default());
    }
    // The body is what is above the button row, and the buttons take the last row the
    // overlay will have, so both are settled before the rectangle is asked for.
    let body_rows = rows.len() as u16;
    rows.push(Line::default());
    let buttons = modal.choices;
    let Some(area) = panes.modal(rows.len() as u16) else {
        return;
    };
    blank_half_covered_glyphs(frame, area);
    frame.render_widget(Clear, area);
    frame.render_widget(
        WidgetBlock::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
        area,
    );
    // The body and the button row are painted apart so the buttons' columns can be
    // recorded exactly: the question owns them, and a click has to land on the one it
    // looks like it landed on (票 04 §3).
    let inner_area = layout::inner(area);
    let body = Rect::new(
        inner_area.x,
        inner_area.y,
        inner_area.width,
        body_rows.min(inner_area.height),
    );
    frame.render_widget(
        Paragraph::new(rows)
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center),
        body,
    );
    // The button row is centred **by its own width**, not by the body's: the body is
    // centred text and the buttons are a shorter line, so inheriting the body's inset
    // left them stranded on the left (2026-09-23, user report).
    let (line, regions) = buttons_row(buttons, &modal.actions);
    let buttons_width = regions.iter().map(|(start, width, _)| start + width).max();
    let buttons_area = Rect::new(
        inner_area.x + centred_inset(inner_area.width, buttons_width),
        inner_area.bottom().saturating_sub(1),
        inner_area.width,
        1,
    );
    frame.render_widget(
        Paragraph::new(line).style(Style::default().fg(Color::Yellow)),
        buttons_area,
    );
    state
        .regions
        .cells
        .extend(regions.into_iter().filter_map(|(start, width, action)| {
            // A button wider than the overlay is not clickable past its border: the
            // part that was not drawn has no region (票 04 §3).
            let x = buttons_area.x + start as u16;
            let room = buttons_area.right().saturating_sub(x).min(width as u16);
            (room > 0).then_some(Region {
                rect: Rect::new(x, buttons_area.y, room, 1),
                action,
            })
        }));
}

/// The column something `width` columns wide starts at inside an area `room` wide, so
/// that it is centred there.
fn centred_inset(room: u16, width: Option<usize>) -> u16 {
    let width = width.unwrap_or(0).min(room as usize) as u16;
    room.saturating_sub(width) / 2
}

/// The keys that answer a question as one row of `(offset, width, action)` triples,
/// where the offset is in **columns** from the start of that text and the width is
/// the text's display width.
///
/// One `[y] 允许` per choice, three spaces between them: the same row the overlay has
/// always drawn, now paired with what a click on it means, because who paints it and
/// who hit-tests it must be one function or the two drift apart (票 04 §7).
fn button_regions(
    choices: &[wording::Choice],
    actions: &[HitAction],
) -> Vec<(usize, usize, HitAction)> {
    let mut regions = Vec::with_capacity(choices.len());
    let mut offset = 0usize;
    for (index, choice) in choices.iter().enumerate() {
        if index > 0 {
            offset += text_columns("   ");
        }
        let width = text_columns(&button_text(choice));
        // The action list comes from the same place the choices do, so the two cannot
        // disagree about which button means what (票 04 §7).
        let action = actions.get(index).copied().unwrap_or(HitAction::Dismiss);
        regions.push((offset, width, action));
        offset += width;
    }
    regions
}

/// One button's text, unchanged from what the overlay has always painted.
fn button_text(choice: &wording::Choice) -> String {
    format!("[{}] {}", choice.key, choice.label)
}

/// The button row as a styled line, and its clickable columns.
fn buttons_row(
    choices: &[wording::Choice],
    actions: &[HitAction],
) -> (Line<'static>, Vec<(usize, usize, HitAction)>) {
    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(Color::Yellow);
    let mut spans = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(format!("[{}]", choice.key), key_style));
        spans.push(Span::styled(format!(" {}", choice.label), label_style));
    }
    (Line::from(spans), button_regions(choices, actions))
}

/// Blank the wide glyph a floating box is about to draw over.
///
/// A wide glyph owns two cells, and the cell after it is **skipped** when a frame is
/// diffed to the terminal — so a border drawn on that second cell is silently dropped
/// and the box loses a corner over anything that is not ASCII. Half a glyph cannot be
/// drawn anyway: the glyph goes and the border stays whole.
fn blank_half_covered_glyphs(frame: &mut ratatui::Frame, area: Rect) {
    let buffer = frame.buffer_mut();
    let last = area.bottom().min(buffer.area.bottom());
    for y in area.y..last {
        if area.x > buffer.area.left() && buffer[(area.x - 1, y)].symbol().cell_width() > 1 {
            buffer[(area.x - 1, y)].set_symbol(" ");
        }
    }
}

/// Everything a terminal below the minimum gets: one centred sentence saying so,
/// rather than four panes crushed into each other.
fn draw_too_small(frame: &mut ratatui::Frame, area: Rect) {
    let row = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
    frame.render_widget(
        Paragraph::new(wording::too_small(layout::MIN_WIDTH, layout::MIN_HEIGHT))
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center),
        row,
    );
}

/// Frame a block: dim, so the border frames the content instead of competing with
/// it.
fn draw_border(frame: &mut ratatui::Frame, area: Rect) {
    frame.render_widget(
        WidgetBlock::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

/// The header: the mark when the terminal is big enough for it, otherwise what
/// The mark's rows and their colours.
///
/// The text is [`wording::logo_lines`]'s; the ramp that makes it read as glyphs lives
/// here, where the rest of the painting does. Rows brighten towards the top, so the
/// mark reads as lit from above. Foreground only, and deliberately no background: the
/// mark sits on whatever background the user's theme already has, and filling the
/// half-shade rows would fight that theme on as many terminals as it matched.
fn mark_lines() -> Vec<(&'static str, Color)> {
    let rows = wording::logo_lines();
    debug_assert!(
        rows.iter()
            .all(|row| text_columns(row) == layout::LOGO_WIDTH as usize),
        "the mark is drawn whole or not at all, so its width is the layout's contract"
    );
    rows.iter()
        .enumerate()
        .map(|(row, text)| {
            let color = if row < rows.len() - 1 {
                Color::LightMagenta
            } else {
                Color::Magenta
            };
            (*text, color)
        })
        .collect()
}

/// The transcript: its window onto the pane's scroll buffer, the scrollbar and the
/// rail at its right edge, and the indicator that says where the viewport is
/// (spec §1, §3, §4).
fn draw_transcript(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let text_area = panes.transcript_text();
    let rows = state
        .pane
        .view(text_area.width, text_area.height, &state.live);
    // What a click can hit is what this frame actually drew, row by row. The pane
    // answers which source line each drawn display row belongs to, and the source
    // line is what the click's link is keyed by (票 04 §1).
    state.drawn_top = text_area.y;
    state.drawn_rows = (0..rows.len())
        .map(|offset| state.pane.source_at(state.pane.top() + offset))
        .collect();
    frame.render_widget(Paragraph::new(rows), text_area);
    draw_scrollbar(frame, panes.scrollbar(), &state.pane);
    draw_rail(frame, panes, state);
    draw_indicator(frame, text_area, state);
}

/// The rail: one cell per turn, or per round in a discussion, down the transcript's
/// right edge (spec §4).
///
/// The cell the viewport is in is the bright one, and it is **derived** from what the
/// pane is showing — never stored — so it cannot drift from the reader's position. Each
/// cell records where it was painted, so a click can only land on a cell that is really
/// on screen.
fn draw_rail(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let rows = panes.rail.height as usize;
    if rows == 0 || panes.rail.width == 0 {
        return;
    }
    let units = state.rail.units();
    if units == 0 {
        // An empty session has an empty column: no cells, and no `⋮` pretending there
        // is history above (spec §4).
        return;
    }
    let focus = state.focused_unit().unwrap_or(units - 1);
    let style = Style::default().fg(Color::DarkGray);
    let focus_style = Style::default()
        .fg(Color::LightMagenta)
        .add_modifier(Modifier::BOLD);
    for (offset, slot) in rail_rows(rows, units, focus).into_iter().enumerate() {
        let (symbol, style, unit) = match slot {
            RailRow::Blank => continue,
            RailRow::Cut => (wording::RAIL_TRUNCATED, style, None),
            RailRow::Unit(unit) if unit == focus => (wording::RAIL_FOCUS, focus_style, Some(unit)),
            RailRow::Unit(unit) => (wording::RAIL_CELL, style, Some(unit)),
        };
        let y = panes.rail.y + offset as u16;
        let buffer = frame.buffer_mut();
        buffer[(panes.rail.x, y)]
            .set_symbol(symbol)
            .set_style(style);
        if let Some(unit) = unit {
            state.regions.cells.push(Region {
                rect: Rect::new(panes.rail.x, y, 1, 1),
                action: HitAction::RailUnit(unit),
            });
        }
    }
}

/// The transcript's scrollbar: drawn only when there is more than a pane's worth,
/// in the column the layout always reserves for it.
fn draw_scrollbar(frame: &mut ratatui::Frame, track: Rect, pane: &Pane) {
    if track.width == 0 || pane.total() <= track.height as usize {
        return;
    }
    // Following the bottom and reading history look different, so the position is
    // legible without reading a number.
    let thumb = if pane.following() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    };
    let mut scrollbar = ScrollbarState::new(pane.total())
        .position(pane.top())
        .viewport_content_length(track.height as usize);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .track_style(Style::default().fg(Color::DarkGray))
            .thumb_style(thumb),
        track,
        &mut scrollbar,
    );
}

/// The "what arrived, and the way back" indicator at the bottom right of the pane.
///
/// Its whole block is the click target, so the rectangle is remembered on the pane
/// — a click can only land on what the last frame drew.
fn draw_indicator(frame: &mut ratatui::Frame, area: Rect, state: &mut TuiState) {
    // While the detail overlay holds the transcript, the way back is the overlay's own
    // footer: the indicator's count is paused and its click belongs to nobody
    // (票 02 §4).
    if state.pane.following() || state.detail_open() || area.width == 0 || area.height == 0 {
        state.indicator = None;
        return;
    }
    let fresh = state.pane.fresh();
    let text = if fresh == 0 {
        wording::back_to_bottom().to_owned()
    } else {
        wording::new_content(fresh)
    };
    // `area` is already the text area — the scrollbar's column is not in it — so a
    // wide character at the right edge cannot shadow the scrollbar away.
    let width = (text_columns(&text) as u16).min(area.width);
    let rect = Rect::new(
        area.right().saturating_sub(width),
        area.bottom().saturating_sub(1),
        width,
        1,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_columns(&text, width as usize),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))),
        rect,
    );
    state.indicator = Some(rect);
}

/// The main column's foot: the input line, and under it the hint row that says what
/// the keys do.
///
/// Returns where the cursor was put, so whatever floats over the main column can
/// anchor itself to it — the `/` menu follows the cursor (spec §6). `None` while a
/// question is up, because there is no cursor then.
///
/// A questionnaire replaces the input line with itself. That is the whole point of
/// this kind of question: the middle overlay suits a one-line confirmation, while a
/// questionnaire is several rows and pages, so it takes the area built for typing
/// (spec §19).
fn draw_bottom(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &mut TuiState,
) -> Option<editor::Placed> {
    if state.questionnaire().is_some() {
        // The questionnaire is lifted out and put back, so the painters can record
        // their hit regions in the same state the pointer will read them from. It has
        // no clone: its reply channel is the one thing about it that must stay single.
        let Some(Pending::Questionnaire(questionnaire)) = state.pending.take() else {
            return None;
        };
        let cursor = draw_questionnaire(frame, panes, &questionnaire, state);
        if let Some(cursor) = cursor {
            frame.set_cursor_position((
                (panes.input.x + cursor.column).min(panes.input.right().saturating_sub(1)),
                panes.input.y + cursor.row,
            ));
        }
        draw_questionnaire_footer(frame, panes, &questionnaire, state);
        state.pending = Some(Pending::Questionnaire(questionnaire));
        return None;
    }
    let (rows, cursor) = state
        .editor
        .view(layout::input_text_width(frame.area()), panes.input.height);
    frame.render_widget(
        Paragraph::new(rows).style(Style::default().add_modifier(Modifier::BOLD)),
        panes.input,
    );
    // The draft stays visible under a question — it is what the user was writing — but
    // the cursor goes: the keyboard is answering, not editing (spec §9). The cursor is
    // placed from the rows just drawn, never from state kept between frames, which is
    // what let the inline viewport's cursor wander (ADR 0002).
    let anchor = state.pending.is_none().then_some(cursor);
    if anchor.is_some() {
        frame.set_cursor_position((
            (panes.input.x + cursor.column).min(panes.input.right().saturating_sub(1)),
            panes.input.y + cursor.row,
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            state.status_line(panes.hints.width),
            Style::default().fg(Color::DarkGray),
        ))),
        panes.hints,
    );
    anchor
}

/// The questionnaire in the bottom input area: one question's rows, scrolled to
/// the room the layout gave.
///
/// The layout caps the bottom block's height, so a question with more rows than
/// fit is windowed rather than clipped: the header and the question stay put and
/// the option window follows the highlight (spec §7, §19). The footer still says
/// which question it is, and the cap keeps the transcript visible.
///
/// The rows are drawn one at a time so each one's screen row can be recorded: the
/// option rows are clickable, and which option a row holds depends on the window the
/// highlight is currently driving (票 04 §4, §6).
fn draw_questionnaire(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    questionnaire: &Questionnaire,
    state: &mut TuiState,
) -> Option<editor::Placed> {
    let question = &questionnaire.questions[questionnaire.index];
    let draft = &questionnaire.drafts[questionnaire.index];
    let width = panes.input.width as usize;
    let (prefix, options, _custom) = questionnaire_parts(question, draft, width);
    let window = questionnaire_window(question, draft, width, panes.input.height as usize);
    let full = prefix.len() + options.len() + 1;
    let start = if full < panes.input.height as usize {
        0
    } else {
        option_window_start(
            draft.highlight,
            options.len(),
            (panes.input.height as usize).saturating_sub(prefix.len() + 1),
        )
    };
    let visible = options
        .len()
        .min((panes.input.height as usize).saturating_sub(prefix.len() + 1));
    // The custom row is the last row the window drew, whether it was pinned there by
    // a clip or simply ended the list.
    let custom_row = window.len().saturating_sub(1);
    for (row, line) in window.iter().enumerate() {
        frame.render_widget(
            Paragraph::new(line.clone()),
            Rect::new(
                panes.input.x,
                panes.input.y + row as u16,
                panes.input.width,
                1,
            ),
        );
    }
    for offset in 0..visible.min(window.len().saturating_sub(prefix.len())) {
        state.regions.options.push((
            panes.input.y + (prefix.len() + offset) as u16,
            start + offset,
        ));
    }
    state.regions.custom = Some(panes.input.y + custom_row as u16);
    // A focused custom row shows the cursor, the way the resident editor's does: it
    // is the only row that can be typed into (票 04 §5).
    if questionnaire.custom_focused {
        let label = if question.options.is_empty() {
            wording::questionnaire_answer_label()
        } else {
            wording::questionnaire_custom_label()
        };
        let column = text_columns(label) + text_columns(&draft.custom);
        let column = column.min(panes.input.width.saturating_sub(1) as usize) as u16;
        Some(editor::Placed {
            row: custom_row as u16,
            column,
        })
    } else {
        None
    }
}

/// The questionnaire's footer: which question it is, then only the buttons that are
/// really available.
///
/// The unavailable ones are not drawn, so they have no click region — the pointer and
/// the eye see the same set (票 04 §4). The three footer labels are one-per-question
/// wording, and the region is recorded from the same layout that painted them.
fn draw_questionnaire_footer(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    questionnaire: &Questionnaire,
    state: &mut TuiState,
) {
    let total = questionnaire.questions.len();
    // The progress counter and the three-column gap before the first button are one
    // span, so the column the first button starts at is the span's own width — not a
    // second guess at it (票 04 §7).
    let mut cursor = text_columns(&wording::questionnaire_progress(questionnaire.index, total)) + 3;
    let mut spans: Vec<Span<'static>> = vec![Span::styled(
        format!(
            "{}   ",
            wording::questionnaire_progress(questionnaire.index, total)
        ),
        Style::default().fg(Color::DarkGray),
    )];
    let items = [
        (
            questionnaire.index > 0,
            wording::questionnaire_previous(),
            HitAction::Previous,
        ),
        (
            questionnaire.index + 1 < total,
            wording::questionnaire_next(),
            HitAction::Next,
        ),
        (
            questionnaire.all_handled(),
            wording::questionnaire_submit(),
            HitAction::Submit,
        ),
    ];
    let mut drawn = false;
    for (available, label, action) in items {
        // An unavailable button is not drawn and has no region, so the next one closes
        // the gap: the footer reads as a list of what is really on offer.
        if !available {
            continue;
        }
        // One gap *between* drawn buttons, and it is **painted** rather than only
        // counted: a region derived from a gap that is not on screen is a region that
        // points three columns off the button (票 04 §7).
        if drawn {
            spans.push(Span::raw("   "));
            cursor += 3;
        }
        let width = text_columns(label);
        if let Some(rect) = hint_region(panes.hints, cursor, width) {
            state.regions.cells.push(Region { rect, action });
        }
        spans.push(Span::styled(
            label.to_owned(),
            Style::default().fg(Color::DarkGray),
        ));
        cursor += width;
        drawn = true;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), panes.hints);
}

/// The screen rectangle of a run of `columns` starting `offset` columns into `row`,
/// or `None` when it falls off the end of the terminal.
fn hint_region(row: Rect, offset: usize, columns: usize) -> Option<Rect> {
    let offset = u16::try_from(offset).ok()?;
    let columns = u16::try_from(columns).ok()?;
    let x = row.x.checked_add(offset)?;
    if x.checked_add(columns)? > row.right() {
        return None;
    }
    Some(Rect::new(x, row.y, columns, 1))
}

/// The `/` menu: the names a leading `/` can become — the built-ins the loop handles
/// and the skills this session discovered — floating at the cursor and filtered by
/// what has been typed after the slash (spec §6).
///
/// It is a **hint**, not a question: it never takes a key away from the draft, and a
/// key it does claim (`↑`, `↓`, `Tab`, `Enter`) is only claimed while it is up.
fn draw_menu(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &mut TuiState,
    anchor: editor::Placed,
) {
    let Some(menu) = state.slash_menu() else {
        return;
    };
    // The window of matches to draw: the highlight stays visible, and the rows above
    // it are given up first as it walks down the list.
    let room = panes
        .menu_room(anchor)
        .min(layout::MENU_MAX_ROWS)
        .min(menu.entries.len() as u16) as usize;
    if room == 0 {
        return;
    }
    let highlighted = menu.selected.unwrap_or(0);
    let first = if highlighted >= room {
        highlighted + 1 - room
    } else {
        0
    };
    let visible = &menu.entries[first..first + room];

    // One column of padding either side, the name column as wide as the widest name,
    // then two spaces, then whatever description fits.
    let name_width = visible
        .iter()
        .map(|(name, _)| text_columns(name) + 1)
        .max()
        .unwrap_or(0);
    let widest = visible
        .iter()
        .map(|(_, description)| 2 + name_width + 2 + text_columns(description))
        .max()
        .unwrap_or(0);
    let width = (widest as u16).min(layout::MENU_MAX_WIDTH);
    let inner = width.saturating_sub(2) as usize;
    let Some(area) = panes.menu(anchor, width, room as u16) else {
        return;
    };

    let rows: Vec<Line<'static>> = visible
        .iter()
        .enumerate()
        .map(|(offset, (name, description))| {
            let selected = menu.selected == Some(first + offset);
            menu_row(name, description, inner, name_width, selected)
        })
        .collect();
    blank_half_covered_glyphs(frame, area);
    frame.render_widget(Clear, area);
    frame.render_widget(
        WidgetBlock::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
        area,
    );
    frame.render_widget(Paragraph::new(rows), layout::inner(area));
}

/// One menu row: `/<name>`, padded to the name column, then the description.
///
/// The highlighted row is painted reversed so it reads as the button `Enter` would
/// press, rather than as one more line of text.
fn menu_row(
    name: &str,
    description: &str,
    inner: usize,
    name_width: usize,
    selected: bool,
) -> Line<'static> {
    let label = format!("/{name}");
    let mut text = label.clone();
    // The description column, when there is room for a description and the row it
    // would sit on. Too narrow and the name has the row to itself, which is still a
    // complete hint.
    let gap = name_width.saturating_sub(text_columns(&label)) + 2;
    if !description.is_empty() && text_columns(&label) + gap + 2 <= inner {
        text.push_str(&" ".repeat(gap));
        text.push_str(description);
    }
    // One leading column of padding, then the row, then whatever is left — so the
    // words never touch the border, and the highlight covers the whole row.
    let body = truncate_columns(&text, inner.saturating_sub(1));
    let padding = inner.saturating_sub(1 + text_columns(&body));
    let style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Yellow)
    };
    Line::from(vec![
        Span::styled(format!(" {body}"), style),
        Span::styled(" ".repeat(padding), style),
    ])
}

/// Attribute a message's rows to its speaker: the label leads the first row and
/// the rest hang under the body of it, so a wrapped or multi-line message reads as
/// one utterance (spec §3).
///
/// The name takes the speaker's own colour and the body keeps the row's — that
/// split is the whole of the colouring rule: the name identifies, the body means
/// what its severity says (票 07 §2).
fn attribute(
    speaker: &crate::events::SpeakerId,
    rows: Vec<Line<'static>>,
    colors: &mut SpeakerColors,
) -> Vec<Line<'static>> {
    let prefix = format!("{} ", speaker_label(speaker));
    let indent = " ".repeat(prefix.as_str().cell_width() as usize);
    let name_style = name_style(speaker, colors);
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            let lead = if index == 0 {
                prefix.clone()
            } else {
                indent.clone()
            };
            let mut spans = vec![Span::styled(lead, name_style)];
            spans.extend(row.spans);
            Line {
                spans,
                style: row.style,
                alignment: row.alignment,
            }
        })
        .collect()
}

/// The style a speaker's `[name]` prefix is drawn in.
///
/// With no palette the label keeps the narration grey it has always had, which is
/// what the plain half of the shared rendering wants: only the TUI tints names,
/// and `plain` never passes a palette (票 07 §4).
fn name_style(speaker: &crate::events::SpeakerId, colors: &mut SpeakerColors) -> Style {
    Style::default().fg(colors.of(speaker))
}

/// One narration line whose text begins with a speaker's `[name]` prefix: the name
/// takes the speaker's colour, the rest the caller's style (票 07 §2).
fn speaker_line(
    speaker: &crate::events::SpeakerId,
    text: String,
    body: Style,
    colors: &mut SpeakerColors,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(speaker_label(speaker), name_style(speaker, colors)),
        Span::styled(format!(" {text}"), body),
    ])
}

/// One painted source line, and where clicking it leads.
///
/// The link is optional because most lines are not a way into anything. A line
/// that is carries the whole detail, read at the moment the line is painted, so a
/// click never has to reach back into the event stream for it (票 02 §4, 票 04 §1).
pub struct RenderedLine {
    pub line: Line<'static>,
    pub link: Option<Detail>,
}

impl RenderedLine {
    /// A line whose whole row opens `detail`.
    fn linked(line: Line<'static>, detail: Detail) -> Self {
        Self {
            line,
            link: Some(detail),
        }
    }
}

impl From<Line<'static>> for RenderedLine {
    fn from(line: Line<'static>) -> Self {
        Self { line, link: None }
    }
}

/// Turn one finalized block into styled terminal lines.
///
/// This is the TUI half of the shared presentation layer: the block was decided
/// once by [`Transcript`], and only the painting happens here.
///
/// `colors` is the transcript's name palette. A caller with no roster to hand —
/// `plain`'s half of this rendering, and the tests that only care about text —
/// passes an empty one through [`render_block_uncoloured`], which draws every name
/// in the narration grey.
pub fn render_block(block: &Block, colors: &mut SpeakerColors) -> Vec<Line<'static>> {
    paint_block(block, colors)
        .into_iter()
        .map(|rendered| rendered.line)
        .collect()
}

/// Paint one block with no roster: every speaker name in the narration grey. This
/// is what the shared rendering looked like before names had colours, kept for the
/// callers that have no roster to draw one from.
pub fn render_block_uncoloured(block: &Block) -> Vec<Line<'static>> {
    render_block(block, &mut SpeakerColors::new(&[]))
}

/// Paint one block, keeping each line's link.
fn paint_block(block: &Block, colors: &mut SpeakerColors) -> Vec<RenderedLine> {
    match block {
        Block::Message {
            speaker,
            role: Role::Assistant,
            text,
            // The reasoning is painted by the state machine, not from the block: it
            // has already become a thinking line, and painting it here as well would
            // show the same thought twice (票 02 §1).
            reasoning: _,
        } => {
            if text.is_empty() {
                return Vec::new();
            }
            // The answer is rendered as Markdown at full brightness; only the
            // speaker label is tinted.
            attribute(speaker, super::markdown::to_lines(text), colors)
                .into_iter()
                .map(RenderedLine::from)
                .collect()
        }
        // The user's own input — and the non-assistant system lines — shown as they
        // were written: every line, nothing elided, and no Markdown, because this
        // is not a document. Continuations line up under the body of the first line
        // (spec §3).
        Block::Message { speaker, text, .. } => attribute(
            speaker,
            text.split('\n')
                .map(|raw| Line::from(raw.to_owned()))
                .collect(),
            colors,
        )
        .into_iter()
        .map(RenderedLine::from)
        .collect(),
        Block::Delta { .. } => Vec::new(),
        Block::RoundStarted { round, mode } => vec![Line::from(Span::styled(
            wording::round_section(*round, *mode),
            Style::default()
                .fg(ratatui::style::Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .into()],
        Block::RoundEnded { round, reason } => {
            vec![severity_line(*reason, wording::round_ended(*round, *reason)).into()]
        }
        Block::Divergence { topic, positions } => {
            let mut lines: Vec<RenderedLine> = vec![Line::from(Span::styled(
                format!("!! {}", wording::divergence(topic)),
                Style::default()
                    .fg(ratatui::style::Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ))
            .into()];
            for position in positions {
                lines.push(Line::from(format!("  - {position}")).into());
            }
            lines
        }
        Block::Tool(tool) => tool_block_lines(tool, colors),
        // The post-hook's feedback, about the call just painted: a plain indented
        // line, yellow because it is policy talking rather than the tool.
        Block::ToolFeedback { outcome, .. } => vec![Line::from(Span::styled(
            format!("  {}", wording::hook_feedback(outcome)),
            Style::default().fg(Color::Yellow),
        ))
        .into()],
        Block::TurnStarted { speaker, iteration } => vec![speaker_line(
            speaker,
            wording::turn_started(*iteration),
            Style::default().fg(ratatui::style::Color::DarkGray),
            colors,
        )
        .into()],
        Block::TurnEnded { speaker, reason } => {
            vec![
                severity_speaker_line(speaker, *reason, wording::turn_ended(*reason), colors)
                    .into(),
            ]
        }
        Block::PermissionAsked {
            speaker,
            tool_name,
            args,
        } => vec![speaker_line(
            speaker,
            wording::permission_asked(tool_name.as_deref(), &summarize_args(args)),
            Style::default().fg(ratatui::style::Color::DarkGray),
            colors,
        )
        .into()],
        Block::PermissionDecided {
            speaker,
            decision,
            source,
            reason,
        } => vec![speaker_line(
            speaker,
            wording::permission_decided(*decision, *source, reason.as_deref()),
            Style::default().fg(ratatui::style::Color::DarkGray),
            colors,
        )
        .into()],
        Block::Hook {
            speaker,
            point,
            outcome,
        } => vec![speaker_line(
            speaker,
            wording::hook(point, outcome),
            Style::default().fg(ratatui::style::Color::DarkGray),
            colors,
        )
        .into()],
        Block::ExecutorSpawned {
            speaker,
            executor_id,
        } => vec![speaker_line(
            speaker,
            wording::executor_spawned(executor_id.as_str()),
            Style::default().fg(ratatui::style::Color::DarkGray),
            colors,
        )
        .into()],
        Block::ExecutorFinished {
            executor_id,
            reason,
            summary,
        } => vec![severity_line(
            *reason,
            wording::executor_finished(executor_id.as_str(), *reason, summary),
        )
        .into()],
        Block::Usage { speaker, usage } => vec![speaker_line(
            speaker,
            wording::usage_summary(usage),
            Style::default().fg(ratatui::style::Color::DarkGray),
            colors,
        )
        .into()],
        Block::AgentError { speaker, message } => vec![severity_speaker_line(
            speaker,
            StopReason::Error,
            wording::agent_error(message),
            colors,
        )
        .into()],
        Block::SessionError { code, detail } => {
            vec![severity_line(StopReason::Error, wording::session_error(code, detail)).into()]
        }
        Block::SessionEnded { reason } => {
            vec![severity_line(*reason, wording::session_ended(*reason)).into()]
        }
        Block::ContextInjected { source } => {
            vec![narration(wording::context_injected(source.clone())).into()]
        }
        Block::History { reason, summary } => {
            vec![narration(wording::history(*reason, summary.as_deref())).into()]
        }
        Block::Diagnostic(message) => vec![Line::from(Span::styled(
            wording::diagnostic(message),
            Style::default().fg(ratatui::style::Color::Yellow),
        ))
        .into()],
        Block::Notice(message) => vec![narration(message.clone()).into()],
    }
}

/// One **intermediate** narration line: dim, so the model's answer — rendered at
/// full brightness — is the thing that stands out. A line that carries a severity
/// keeps its own colour instead (see [`severity_line`]).
fn narration(text: String) -> Line<'static> {
    Line::from(Span::styled(
        text,
        Style::default().fg(ratatui::style::Color::DarkGray),
    ))
}

fn severity_line(reason: StopReason, text: String) -> Line<'static> {
    Line::from(Span::styled(text, severity_style(reason)))
}

/// A severity line that names a speaker: the name keeps the speaker's colour, the
/// rest of the line keeps the severity's (票 07 §2). That is how an error still
/// reads as an error without the reader losing who made it.
fn severity_speaker_line(
    speaker: &crate::events::SpeakerId,
    reason: StopReason,
    text: String,
    colors: &mut SpeakerColors,
) -> Line<'static> {
    speaker_line(speaker, text, severity_style(reason), colors)
}

/// The colour a stopping point paints its line in.
fn severity_style(reason: StopReason) -> Style {
    match Severity::of(reason) {
        Severity::Good => Style::default().fg(ratatui::style::Color::Green),
        Severity::Note => Style::default().fg(ratatui::style::Color::Cyan),
        Severity::Warn => Style::default().fg(ratatui::style::Color::Yellow),
        Severity::Bad => Style::default()
            .fg(ratatui::style::Color::Red)
            .add_modifier(Modifier::BOLD),
    }
}

/// A finished tool call, folded to one line: the **call** the reader can open, with
/// the whole output behind it (票 02 §3).
///
/// A failure is the same line with `失败` at its **end** — not a second line — and
/// the error body moves into the detail. The post-hook's feedback is its own block
/// and stays on screen: it is policy feedback, not tool output, so it has to be
/// readable without a click (票 02 §3).
fn tool_block_lines(tool: &ToolBlock, colors: &mut SpeakerColors) -> Vec<RenderedLine> {
    let failed = matches!(&tool.outcome, Some(outcome) if !outcome.ok);
    let color = colors.of(&tool.speaker);
    let mut call = vec![
        // The name leads, so every transcript line starts with who is speaking; the
        // marker after it is what says the line can be opened. It is paint, not
        // wording, so it is not part of the sentence (票 03 §Answer)。
        Span::styled(
            format!("{} ", speaker_label(&tool.speaker)),
            Style::default().fg(color),
        ),
        Span::styled("▸ ", Style::default().fg(Color::DarkGray)),
        // What the call was *for*, in the narration grey the thinking line wears — the
        // arguments themselves are one click away (票 02 §2，2026-09-23 修正）。
        Span::styled(
            wording::tool_call_line(&tool.tool, &tool.args),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if failed {
        call.push(Span::styled(
            format!(" {}", wording::tool_failed()),
            Style::default().fg(Color::Red),
        ));
    }
    let detail = Detail {
        title: line_text(&Line::from(call.clone())),
        color,
        kind: DetailKind::Tool {
            tool_call_id: tool.tool_call_id.clone(),
            output: tool
                .outcome
                .as_ref()
                .and_then(|outcome| outcome.output.clone()),
            error: tool
                .outcome
                .as_ref()
                .and_then(|outcome| outcome.error.clone()),
            args: tool.args.clone(),
            no_result: tool.outcome.is_none(),
        },
    };
    vec![RenderedLine::linked(Line::from(call), detail)]
}

/// Whether a source line is the user's own message, which is what a rail cell's jump
/// aims at (spec §4).
fn is_user_message(block: &Block) -> bool {
    matches!(
        block,
        Block::Message {
            speaker: crate::events::SpeakerId::User,
            ..
        }
    )
}

/// Whether this block ends the unit the rail counts.
///
/// A discussion counts its **rounds** and an interactive session its turns; the two
/// boundaries both exist in a discussion's stream, so which one counts is a property
/// of the session rather than of the block (spec §4).
fn is_boundary(block: &Block, discussion: bool) -> bool {
    if discussion {
        matches!(block, Block::RoundEnded { .. })
    } else {
        matches!(block, Block::TurnEnded { .. })
    }
}

/// One row of the rail's column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RailRow {
    /// The unit with this index, counted from the oldest.
    Unit(usize),
    /// Units were left out at this end: `⋮`.
    Cut,
    /// Nothing is drawn here.
    Blank,
}

/// The rail's slots for a transcript `rows` rows tall, `units` units long, with the
/// viewport on `focus`.
///
/// Two properties, and both are the spec's (§4):
///
/// * the focus cell is always among them — that is the whole point of the column;
/// * whichever end had units cut says so with a `⋮`.
///
/// The window is bottom-anchored whenever it can be, and slides up only as far as the
/// focus needs: a viewport at the bottom shows the newest units, and one parked in the
/// middle shows the units around it. Keeping only the newest N — the first thing this
/// was written as — left a viewport parked on an old unit with **no** bright cell at
/// all, which is the frame `prototype/frames/120x24-rail-30-units-focus-12-gap.txt`
/// records.
fn rail_rows(rows: usize, units: usize, focus: usize) -> Vec<RailRow> {
    let mut out = vec![RailRow::Blank; rows];
    if rows == 0 || units == 0 {
        return out;
    }
    let focus = focus.min(units - 1);
    if units <= rows {
        // Every unit fits: newest at the bottom, blank above.
        for index in 0..units {
            out[rows - units + index] = RailRow::Unit(index);
        }
        return out;
    }
    // More units than rows. One slot is the focus, and each end that had units cut
    // spends another; a terminal so short that even those do not fit keeps the focus
    // cell and gives the marks up.
    let above = focus;
    let below = units - 1 - focus;
    let mut budget = rows - 1;
    let mut top_cut = above > 0;
    let mut bottom_cut = below > 0;
    if top_cut {
        if budget > 0 {
            budget -= 1;
        } else {
            top_cut = false;
        }
    }
    if bottom_cut {
        if budget > 0 {
            budget -= 1;
        } else {
            bottom_cut = false;
        }
    }
    // Split what is left between the two sides, giving each no more than it has and
    // handing the remainder back to the older side: a viewport at the bottom takes
    // everything above it, one in the middle comes out roughly centred.
    let mut above_taken = above.min(budget / 2);
    let below_taken = below.min(budget - above_taken);
    above_taken += (above - above_taken).min(budget - above_taken - below_taken);

    let mut cells: Vec<RailRow> = Vec::with_capacity(rows);
    if top_cut {
        cells.push(RailRow::Cut);
    }
    let first = focus - above_taken;
    for index in first..first + above_taken + 1 + below_taken {
        cells.push(RailRow::Unit(index));
    }
    if bottom_cut {
        cells.push(RailRow::Cut);
    }
    out[rows - cells.len()..].copy_from_slice(&cells);
    out
}

/// The text of a painted line, for a title.
fn line_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

/// What a clickable transcript line opens: a frozen thought, or a tool call's
/// arguments and full output (票 02 §4).
///
/// The body is read **when the line is painted**, so what the overlay shows cannot
/// disagree with what was on screen when the reader clicked it.
#[derive(Clone)]
pub struct Detail {
    /// The clicked line's own text, used as the overlay's title.
    title: String,
    /// The speaker's colour for the line this detail belongs to: the overlay's border
    /// wears it, so the box says whose line you are reading before you read a word of
    /// it (2026-09-23).
    color: Color,
    kind: DetailKind,
}

/// The two things a detail view can be about.
#[derive(Clone)]
enum DetailKind {
    /// A finished thinking segment. `text` is the whole trace when the stream
    /// recorded one, and `None` is the synthesizer's case — deltas arrived and the
    /// log holds no text — which the detail says out loud (票 02 §1).
    Thinking { text: Option<String> },
    /// A tool call: its arguments, and whatever the call produced.
    Tool {
        /// The id that names the spilled output file, `outputs/<id>.txt`.
        tool_call_id: ToolCallId,
        output: Option<String>,
        error: Option<String>,
        args: serde_json::Value,
        no_result: bool,
    },
}

/// The detail overlay's open state (票 02 §4).
///
/// It is a **view mode, not a pending question**: the transcript is frozen where it
/// was, the keyboard and the wheel belong to the body until it is closed, and no
/// `pending` is set — which is exactly what keeps the question guard from swallowing
/// the wheel aimed at the overlay.
struct DetailView {
    /// What is being shown.
    detail: Detail,
    /// The body, laid out at the width it was opened at.
    body: Vec<Line<'static>>,
    /// The first body row on screen.
    top: usize,
    /// Body rows the overlay can show at once.
    height: usize,
}

/// The cell of air the detail overlay keeps between its border and its words.
const DETAIL_PADDING: u16 = 1;

/// The rows the overlay's own text needs before padding is worth having: a title row,
/// two body rows, and the footer.
const DETAIL_MIN_TEXT_ROWS: u16 = 4;

/// The most characters a detail body will read from a spilled tool output.
///
/// A tool result is capped before it reaches the log, but the spilled file is not:
/// this is the reader's own limit, past which the body ends with
/// [`wording::detail_truncated`] (票 02 §4).
const DETAIL_MAX_CHARS: usize = 200_000;

impl TuiState {
    /// Open the detail overlay for a line the reader clicked.
    ///
    /// The body is read here, at open time, and laid out at the width the overlay
    /// will be drawn at, so scrolling is pure arithmetic from then on.
    fn open_detail(&mut self, detail: Detail, width: usize) {
        let body = detail_body(&detail, &self.facts.session_dir, width);
        self.detail = Some(DetailView {
            detail,
            body,
            top: 0,
            height: 0,
        });
    }

    /// Close it, wherever it was opened from.
    ///
    /// A no-op when nothing is open, which matters because the request handlers call it
    /// unconditionally: releasing a freeze that was never taken would yank a reader who
    /// had scrolled up back to the bottom (票 02 §4).
    fn close_detail(&mut self) {
        if self.detail.take().is_some() {
            // The reading position was the overlay's; letting go of it returns the
            // transcript to the bottom, and the count to measuring from there.
            self.pane.set_holding(false);
            self.pane.set_following(true);
        }
    }

    fn detail_open(&self) -> bool {
        self.detail.is_some()
    }

    /// Scroll the open detail body by `rows` display rows; negative is up.
    fn detail_scroll(&mut self, rows: isize) {
        let Some(view) = self.detail.as_mut() else {
            return;
        };
        let max_top = view.body.len().saturating_sub(view.height);
        view.top = (view.top as isize + rows).clamp(0, max_top as isize) as usize;
    }

    /// One page of the detail body: its own height, minus a row of overlap so the
    /// reader keeps the thread across a jump.
    fn detail_page(&self) -> usize {
        self.detail
            .as_ref()
            .map(|view| view.height.saturating_sub(1).max(1))
            .unwrap_or(1)
    }
}

/// The body of a detail view, wrapped to `width`: the sections, in the order they
/// are decided, each under a rule (票 03 §Answer).
///
/// An absent body is not an error: each one has a sentence that says so, because a
/// click that opened a blank box is worse than one that never opened.
fn detail_body(detail: &Detail, session_dir: &str, width: usize) -> Vec<Line<'static>> {
    let mut rows: Vec<Line<'static>> = Vec::new();
    match &detail.kind {
        DetailKind::Thinking { text } => {
            rows.push(section_header(wording::detail_thinking_section()));
            match text {
                Some(text) if !text.trim().is_empty() => {
                    rows.extend(pane::wrap_text(text.trim_end(), width));
                }
                // No recorded trace — the synthesizer's shape — so the body says so
                // rather than opening blank (票 02 §1).
                _ => rows.push(Line::from(Span::styled(
                    wording::detail_reasoning_unrecorded(),
                    Style::default().fg(Color::DarkGray),
                ))),
            }
        }
        DetailKind::Tool {
            tool_call_id,
            output,
            error,
            args,
            no_result,
        } => {
            rows.push(section_header(wording::detail_args_section()));
            let args = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
            rows.extend(pane::wrap_text(&args, width));
            rows.push(section_header(wording::detail_output_section()));
            if *no_result {
                rows.push(Line::from(Span::styled(
                    wording::no_tool_result(),
                    Style::default().fg(Color::DarkGray),
                )));
            } else if let Some(error) = error {
                rows.extend(pane::wrap_text(error, width));
            } else if let Some(output) = output {
                let (body, truncated) = read_tool_body(tool_call_id, output, session_dir);
                rows.extend(pane::wrap_text(&body, width));
                if truncated {
                    rows.push(Line::from(Span::styled(
                        wording::detail_truncated(),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            } else {
                rows.push(Line::from(Span::styled(
                    wording::detail_output_unavailable(),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }
    rows
}

/// A section heading, drawn as a rule: the words in a run of `─`.
fn section_header(name: &str) -> Line<'static> {
    Line::from(Span::styled(
        wording::detail_section(name),
        Style::default().fg(Color::DarkGray),
    ))
}

/// The whole body of a tool result, when the spilled file can be read.
///
/// The event carries only the head/tail **preview**; the full text is what was
/// spilled to `outputs/<tool_call_id>.txt`, and the call id is what names that
/// file — never the preview's own prose (票 02 §4). A missing file is the
/// documented degradation: the preview, and a sentence saying the full text was not
/// available. An **empty** file is the same degradation: there is no full text to
/// show, and the preview plus the sentence is the honest answer rather than a blank
/// body (票 08 §8).
fn read_tool_body(tool_call_id: &ToolCallId, preview: &str, session_dir: &str) -> (String, bool) {
    // An uncut result has no spilled file to look for, and its preview is the whole
    // body: show it as it is. Only a result the stream had to *cut* has a file on
    // disk, so only that kind can be missing one (spec §11；2026-09-23，用户报告
    // 短输出的详情不该写着「全文不可用」).
    if !preview.contains(crate::context::TRUNCATED_MARKER) {
        return (preview.to_owned(), false);
    }
    // `SessionFacts.cwd` holds the **session directory**, so the outputs directory
    // is one join away — the same arithmetic the harness does (票 01 事实 56).
    let path = std::path::Path::new(session_dir)
        .join(crate::session::store::OUTPUTS_DIR)
        .join(format!("{tool_call_id}.txt"));
    let unavailable = || {
        (
            format!("{preview}\n{}", wording::detail_output_unavailable()),
            false,
        )
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return unavailable();
    };
    if text.is_empty() {
        return unavailable();
    }
    if text.chars().count() <= DETAIL_MAX_CHARS {
        return (text, false);
    }
    let cut: String = text.chars().take(DETAIL_MAX_CHARS).collect();
    (cut, true)
}

/// Paint the detail overlay over the main column.
///
/// It owns the keyboard and the wheel while it is up, and the transcript stays
/// frozen where it was — a reading position, not a moving one (票 02 §4).
fn draw_detail(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    if state.detail.is_none() {
        state.detail_rect = None;
        return;
    }
    let Some(area) = panes.detail() else {
        // Nowhere to draw it: leaving it open would keep the keyboard captured for a
        // view nobody can see.
        state.detail = None;
        state.detail_rect = None;
        return;
    };
    // The transcript is frozen where it was: the reader is looking at a line, and a
    // burst of output must not pull it away — nor make the "N new rows" count climb
    // under the overlay they are reading (票 02 §4).
    state.pane.set_following(false);
    state.pane.set_holding(true);
    // What a click outside can hit only exists once this is recorded.
    state.detail_rect = Some(area);
    let Some(view) = state.detail.as_ref() else {
        return;
    };
    let inner = layout::inner(area);
    // A cell of air inside the border, so the words do not touch the frame. It is given
    // **up** rather than eating the body: below the height that leaves the body two rows
    // plus the title and the footer, the padding would hide the very content the reader
    // opened the overlay for (2026-09-23).
    let pad_x = u16::from(inner.width > DETAIL_PADDING * 3);
    let pad_y = u16::from(inner.height >= DETAIL_PADDING * 2 + DETAIL_MIN_TEXT_ROWS);
    let text = Rect::new(
        inner.x + DETAIL_PADDING * pad_x,
        inner.y + DETAIL_PADDING * pad_y,
        inner.width.saturating_sub(DETAIL_PADDING * 2 * pad_x),
        inner.height.saturating_sub(DETAIL_PADDING * 2 * pad_y),
    );
    // Everything below is sized from the **padded** rect: a body window one row taller
    // than the box that shows it clips the last rows off the end, which is how the
    // padding first ate a line of the very body it was making room for.
    let body_rows = text.height.saturating_sub(2) as usize;
    let max_top = view.body.len().saturating_sub(body_rows);
    let top = view.top.min(max_top);
    let rows: Vec<Line<'static>> = view
        .body
        .iter()
        .skip(top)
        .take(body_rows)
        .cloned()
        .collect();
    // The footer counts the **last row on screen**, not the first: a reader who has
    // scrolled to the bottom is at the bottom, whatever row the window happens to start
    // at (2026-09-23, user report: it read `94/154` with the last row visible).
    let footer = wording::detail_footer(
        (top + body_rows).min(view.body.len()).max(1),
        view.body.len().max(1),
    );
    let title = view.detail.title.clone();

    blank_half_covered_glyphs(frame, area);
    frame.render_widget(Clear, area);
    // The border wears the speaker's colour: the box belongs to one line, and whose
    // line it is should be legible before a word of it is read (2026-09-23).
    frame.render_widget(
        WidgetBlock::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(view.detail.color)),
        area,
    );
    // The title row is the clicked line's own text, so the reader knows which line
    // they opened.
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_columns(&title, text.width as usize),
            Style::default().add_modifier(Modifier::BOLD),
        ))),
        Rect::new(text.x, text.y, text.width, 1),
    );
    // The body takes everything between the title and the footer; the footer is
    // pinned to the overlay's last text row, so the two cannot overlap (票 03
    // §Answer).
    let body = Rect::new(
        text.x,
        text.y + 1,
        text.width,
        text.height.saturating_sub(2),
    );
    frame.render_widget(Paragraph::new(rows), body);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            footer,
            Style::default().fg(Color::DarkGray),
        ))),
        Rect::new(text.x, text.y + text.height - 1, text.width, 1),
    );
    let view = state.detail.as_mut().expect("just checked");
    view.height = body_rows;
    view.top = top;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `map_key` is the one place crossterm's vocabulary becomes this renderer's,
    /// and a key that misses here is a key that silently does nothing — which no
    /// rendering test can see, because they all start from [`Key`].
    #[test]
    fn the_keys_the_pane_answers_to_map_from_crossterm() {
        let plain = |code| map_key(KeyEvent::new(code, KeyModifiers::empty()));
        assert_eq!(plain(KeyCode::PageUp), Some(Key::PageUp));
        assert_eq!(plain(KeyCode::PageDown), Some(Key::PageDown));
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL)),
            Some(Key::CtrlG)
        );
        // The newline key: the one the whole multi-line editor hangs on.
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
            Some(Key::CtrlJ)
        );
        // A bare `g` is text, not a gesture.
        assert_eq!(plain(KeyCode::Char('g')), Some(Key::Char('g')));
    }
}
