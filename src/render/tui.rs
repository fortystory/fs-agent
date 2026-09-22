//! The ratatui interface (spec §1–§2, ADR 0002).
//!
//! Three properties are structural, not stylistic:
//!
//! * **Alternate screen, four panes.** The TUI draws a fullscreen header, a
//!   conversation pane, an information panel and a bottom block holding the input
//!   and the hints. The transcript lives in the pane's own buffer rather than in
//!   the terminal's scrollback — which is what removed the inline viewport's
//!   drifting cursor, since in fullscreen the pane origin is always `(0, 0)`.
//! * **The renderer owns the keyboard.** It is the only task reading terminal
//!   events, and it answers the loop's requests ([`ConsoleRequest`]) over the
//!   injected console channel. That is what keeps input and output from fighting.
//! * **`select!` over broadcast / tick / keys.** Render events, a redraw tick and
//!   keyboard input are three independent sources; `select!` is how they are
//!   merged without a second channel whose ordering would be undefined. What is
//!   already queued is drained before the frame is drawn, so a bursting provider
//!   costs frames rather than events.

use std::time::Duration;

use async_trait::async_trait;
use chrono::Local;
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

use crate::events::{ContextSource, HistoryReason, Role, StopReason, ToolCallId};
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

/// How often the frame is redrawn even without an event. The tick is what keeps
/// the header's clock honest, and what gives a pending question a chance to
/// appear while nothing else is happening.
const TICK: Duration = Duration::from_millis(120);

/// A paste larger than this asks before it is taken (spec §7).
const PASTE_CONFIRM_CHARS: usize = 100_000;

/// How many queued render events one frame absorbs. A bounded drain keeps a
/// firehose from starving the keyboard for a whole frame's worth of work.
const DRAIN_LIMIT: usize = 4_096;

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

/// The session values the header and the panel cannot read off the event stream
/// (spec §8).
///
/// Everything here is known at assembly time and injected as one value, because
/// that is the seam: the renderer never reaches for configuration. The header uses
/// `cwd`; the rest are carried for the information panel, which ticket 13 fills in.
///
/// Anything that changes mid-session — the mode — is deliberately **not** here: an
/// injected copy would go stale the first time the user pressed Shift+Tab, and the
/// stream already carries both transitions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionFacts {
    /// The session this terminal is showing.
    pub session_id: String,
    /// The directory the session is bound to.
    pub cwd: String,
    /// The model the session answers with — or, in a discussion, the two debaters'
    /// models, because the panel has one row for it (spec §8).
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
/// the facts the header and the panel display.
pub struct TuiOptions {
    pub port: ConsolePort,
    pub facts: SessionFacts,
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
        let TuiOptions { mut port, facts } = self.options;
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
        state.refresh_clock();

        loop {
            let mut closed = false;
            tokio::select! {
                received = receiver.recv() => match received {
                    Ok(event) => state.apply(event),
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        state.apply(RenderEvent::Diagnostic(wording::renderer_dropped(dropped)));
                    }
                    Err(broadcast::error::RecvError::Closed) => closed = true,
                },
                maybe_event = keys.next() => match maybe_event {
                    Some(Ok(CtEvent::Key(key))) => {
                        if key.kind == KeyEventKind::Press {
                            if let Some(key) = map_key(key) {
                                state.key(key);
                            }
                        }
                    }
                    Some(Ok(CtEvent::Paste(text))) => state.paste(&text),
                    Some(Ok(CtEvent::Mouse(mouse))) => state.mouse(mouse),
                    Some(Ok(CtEvent::Resize(..))) => state.mark_dirty(),
                    _ => {}
                },
                request = port.recv() => match request {
                    Some(request) => state.request(request),
                    None => closed = true,
                },
                _ = tick.tick() => state.refresh_clock(),
            }

            // Whatever is already queued joins this frame. A provider that bursts
            // a thousand deltas between two frames costs one frame instead of a
            // thousand, and still loses nothing (spec §11).
            let mut drained = 0usize;
            while drained < DRAIN_LIMIT {
                match receiver.try_recv() {
                    Ok(event) => state.apply(event),
                    Err(broadcast::error::TryRecvError::Lagged(dropped)) => {
                        state.apply(RenderEvent::Diagnostic(wording::renderer_dropped(dropped)));
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
    /// What the header and the panel display, injected at assembly (spec §8).
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
    /// The header's clock, kept so a tick can tell whether the frame it would draw
    /// is any different from the one already on screen.
    clock: chrono::DateTime<Local>,
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
    /// The numbers the panel shows, counted off the stream.
    panel: Panel,
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
            Key::Char(ch) => self.type_custom(ch),
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
        }
    }

    fn back(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    fn all_handled(&self) -> bool {
        self.drafts.iter().all(QuestionDraft::handled)
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
                title: wording::permission_title(&request.tool_name),
                // The plain sentence comes first: a reader who cannot parse the
                // arguments still has to know what they are saying yes to.
                summary: Some(wording::permission_summary(&request.tool_name)),
                detail: Some(wording::permission_call(
                    &request.tool_name,
                    &summarize_args(&request.args),
                )),
                choices: &wording::PERMISSION_CHOICES,
            },
            Pending::Loop {
                question: Question::PlanConflict(path),
                ..
            } => Modal {
                title: wording::plan_conflict_title().to_owned(),
                summary: None,
                detail: Some(wording::plan_conflict_body(&path.display().to_string())),
                choices: &wording::PLAN_CHOICES,
            },
            Pending::Paste { chars, .. } => Modal {
                title: wording::paste_title().to_owned(),
                summary: None,
                detail: Some(wording::paste_body(*chars)),
                choices: &wording::PASTE_CHOICES,
            },
            Pending::ClearDraft => Modal {
                title: wording::clear_draft_title().to_owned(),
                summary: None,
                detail: Some(wording::clear_draft_body().to_owned()),
                choices: &wording::CLEAR_CHOICES,
            },
            Pending::Exit => Modal {
                title: wording::exit_title().to_owned(),
                summary: None,
                detail: Some(wording::exit_body().to_owned()),
                choices: &wording::EXIT_CHOICES,
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
    /// What the action is, in one plain sentence — the row a reader who cannot parse
    /// the arguments reads. Absent when the question is already plain enough.
    summary: Option<String>,
    /// The one concrete thing the question is about: the call, the path, the size.
    detail: Option<String>,
    /// The keys that answer it, painted as one row of buttons.
    choices: &'static [wording::Choice],
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

impl TuiState {
    pub fn new(facts: SessionFacts) -> Self {
        let colors = SpeakerColors::new(&facts.speaker_order);
        Self {
            facts,
            mode: Mode::Ask,
            transcript: Transcript::new(),
            pane: Pane::new(),
            live: String::new(),
            clock: Local::now(),
            dirty: true,
            editor: Input::new(),
            catalog: Vec::new(),
            slash: MenuSelection::default(),
            panel: Panel::new(),
            colors,
            reasoning: String::new(),
            thinking_speaker: crate::events::SpeakerId::System,
            thinking_open: false,
            links: std::collections::VecDeque::new(),
            drawn_rows: Vec::new(),
            drawn_top: 0,
            detail: None,
            area: Rect::default(),
            prompt_reply: None,
            // Idle until the loop says otherwise: before it asks its first line nothing
            // is running, and the keyboard has to read that way (spec §6).
            running: false,
            pending: None,
            events: Vec::new(),
            indicator: None,
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

    /// Re-read the wall clock. Only a new minute is worth a frame (spec §10).
    pub fn refresh_clock(&mut self) {
        let now = Local::now();
        if wording::clock(&now) != wording::clock(&self.clock) {
            self.clock = now;
            self.dirty = true;
        }
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

    /// Feed one render event.
    pub fn apply(&mut self, event: RenderEvent) {
        self.dirty = true;
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
                        self.open_thinking(speaker.clone());
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
                    // A recorded trace settles whatever is open — or opens the line
                    // itself, when the provider sent reasoning without deltas. An
                    // absent trace settles an open line as *unrecorded*, which is the
                    // synthesizer's shape: deltas streamed, nothing written down
                    // (票 02 §1).
                    match reasoning {
                        Some(text) => {
                            let text = text.clone();
                            self.open_thinking(speaker.clone());
                            self.settle_thinking(Some(text), false);
                        }
                        None => {
                            if self.thinking_open {
                                self.settle_thinking(None, true);
                            }
                        }
                    }
                }
            }
            match &block {
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
            for rendered in lines {
                let link = rendered.link;
                self.pane.push(rendered.line);
                self.links.push_back(link);
                self.prune_links();
            }
        }
    }

    /// Open the thinking line, unless one is already open.
    ///
    /// The line is a plain transcript row — it counts against the pane's cap and
    /// scrolls with everything else — and it is deliberately **not** clickable yet:
    /// the whole trace only exists on `MessageCompleted` (票 02 §1).
    fn open_thinking(&mut self, speaker: crate::events::SpeakerId) {
        if self.thinking_open {
            return;
        }
        self.thinking_open = true;
        self.thinking_speaker = speaker;
        self.reasoning.clear();
        let line = Line::from(Span::styled(
            format!(
                "{} {}",
                wording::speaker_label(&self.thinking_speaker),
                wording::thinking_in_progress()
            ),
            Style::default().fg(Color::DarkGray),
        ));
        self.pane.push(line);
        self.links.push_back(None);
        self.prune_links();
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
        self.settle_thinking(recorded.then_some(text), !recorded);
    }

    /// Settle the thinking line — recorded trace or not — and make it the way into
    /// its detail.
    ///
    /// `None` with `unrecorded` is the synthesizer's shape: deltas streamed, the log
    /// holds no whole text. `None` without it (and with no line open) is the
    /// ordinary no-reasoning turn, which adds nothing at all (票 02 §1).
    fn settle_thinking(&mut self, text: Option<String>, unrecorded: bool) {
        if !self.thinking_open {
            return;
        }
        self.reasoning.clear();
        self.thinking_open = false;
        let title = format!(
            "{} {}",
            wording::speaker_label(&self.thinking_speaker),
            wording::thinking_finished()
        );
        let detail = Detail {
            title: title.clone(),
            kind: DetailKind::Thinking { text, unrecorded },
        };
        // In place: one thinking segment is one line, from `正在思考` to `思考完成`
        // (票 02 §1). The leading `▸` is what says the line can be opened.
        self.pane.replace_last(Line::from(vec![
            Span::styled("▸ ", Style::default().fg(Color::DarkGray)),
            Span::styled(title, Style::default().fg(Color::DarkGray)),
        ]));
        if let Some(link) = self.links.back_mut() {
            *link = Some(detail);
        }
    }

    /// Drop the oldest links until this list is no longer than the pane's cap, which
    /// is the only way the two stay parallel: a source row means the same thing in
    /// both or neither (票 04 §1).
    fn prune_links(&mut self) {
        while self.links.len() > pane::CAP {
            self.links.pop_front();
        }
    }

    /// Handle one mouse event.
    ///
    /// Only two things answer to the mouse: the wheel scrolls the transcript, and
    /// a click on the "back to bottom" indicator returns to the bottom. Every
    /// other click is ignored — the terminal's own selection is the user's, and
    /// nothing here takes focus (spec §4).
    pub fn mouse(&mut self, mouse: MouseEvent) {
        // Three dispatches, in order of who owns the pointer. A detail overlay owns
        // it outright; otherwise a question does; otherwise the transcript does.
        // Nothing here ever scrolls the transcript behind something that is up
        // (票 04 §2).
        if self.detail_open() {
            self.dirty = true;
            match mouse.kind {
                MouseEventKind::ScrollUp => self.detail_scroll(-1),
                MouseEventKind::ScrollDown => self.detail_scroll(1),
                MouseEventKind::Down(MouseButton::Left) => {
                    // A second click on the line the overlay came from closes it;
                    // anywhere else is ignored (票 02 §4).
                    if self.detail_reselected(&mouse) {
                        self.close_detail();
                    }
                }
                _ => {}
            }
            return;
        }
        if self.pending.is_some() {
            // A question owns the pointer as well as the keyboard: the wheel must not
            // scroll the transcript behind it (spec §9).
            return;
        }
        self.dirty = true;
        match mouse.kind {
            MouseEventKind::ScrollUp => self.pane.wheel(true),
            MouseEventKind::ScrollDown => self.pane.wheel(false),
            MouseEventKind::Down(MouseButton::Left) => {
                if self.indicator_hit(mouse.column, mouse.row) {
                    self.pane.to_bottom();
                } else {
                    let width = layout::plan(self.area, 1).detail_width() as usize;
                    if let Some((row, detail)) = self.link_hit(&mouse) {
                        self.open_detail(row, detail, width);
                    }
                }
            }
            _ => {}
        }
    }

    /// The clickable link a click landed on: the source row and a copy of what it
    /// opens.
    ///
    /// The width the overlay will open at comes from the last frame, which is the
    /// only place the middle block's geometry is known (票 04 §1).
    fn link_hit(&self, mouse: &MouseEvent) -> Option<(usize, Detail)> {
        let offset = (mouse.row.checked_sub(self.drawn_top)?) as usize;
        let row = (*self.drawn_rows.get(offset)?)?;
        let detail = self.links.get(row)?.clone()?;
        Some((row, detail))
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
                }));
            }
            // The names the loop can act on. They arrive once, after assembly — the
            // skills come from the session — and nothing else carries them.
            ConsoleRequest::Catalog { entries } => self.catalog = entries,
        }
    }

    pub fn take_events(&mut self) -> Vec<FrontEndEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn should_quit(&self) -> bool {
        self.quit
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
        // The detail overlay is a view mode of its own: it owns the keyboard while it
        // is up, and the transcript underneath is frozen where the reader left it
        // (票 02 §4).
        if self.detail_open() {
            match key {
                Key::CtrlC => {
                    if self.busy() {
                        self.events.push(FrontEndEvent::Cancel);
                    } else {
                        self.quit = true;
                    }
                }
                // The one exception to "everything else is ignored": `Ctrl-D` closes
                // the overlay rather than quitting, let alone asking (票 06 §5).
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
            Key::PageUp => self.pane.page(true),
            Key::PageDown => self.pane.page(false),
            Key::CtrlG => self.pane.to_bottom(),
            _ => {}
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

/// Draw one frame of the four-pane layout.
///
/// This is the seam the layout is tested through: a state goes in, a fixed-size
/// frame comes out, and no terminal is involved (spec §2).
pub fn draw_frame(frame: &mut ratatui::Frame, state: &mut TuiState) {
    let area = frame.area();
    // The pointer is answered between frames, and opening a detail needs the width
    // this frame was drawn at.
    state.area = area;
    if layout::below_minimum(area) {
        // Nothing is drawn that a click could land on.
        state.indicator = None;
        draw_too_small(frame, area);
        return;
    }
    // The draft's own height decides how much room the input takes: it grows with
    // the text up to the layout's cap and then scrolls internally (spec §5). A
    // questionnaire replaces that with its own height, so the bottom block grows to
    // hold the question (spec §19).
    let content_rows = state.bottom_rows(area);
    let panes = layout::plan(area, content_rows);
    draw_header(frame, &panes, state);
    draw_transcript(frame, &panes, state);
    let anchor = draw_bottom(frame, &panes, state);
    // The `/` menu floats over the pane, under the cursor it belongs to — and under a
    // question, which owns the keyboard and so has no menu to offer (spec §6, §9).
    if let Some(anchor) = anchor {
        draw_menu(frame, &panes, state, anchor);
    }
    // Last, so it is on top of the pane it is asking about.
    draw_modal(frame, &panes, state);
    // The detail overlay goes over all of it. It cannot be up at the same time as a
    // question — opening one needs an idle keyboard — so the order between the two
    // is a formality (票 02 §4).
    draw_detail(frame, &panes, state);
}

/// The overlay a question is asked in (spec §9).
///
/// It sits over the middle block — transcript and panel both — so the question cannot
/// be outrun by new output, and it is **not** part of the transcript: the stream still
/// carries the `PermissionAsked` block for anyone reading back. It owns the pointer
/// while it is up, so the "back to bottom" rectangle is dropped.
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
    let rows_available = panes.middle.height.saturating_sub(2) as usize;
    if rows_available == 0 {
        return;
    }
    let mut rows: Vec<Line<'static>> = pane::wrap_text(modal.title.trim(), inner);
    for row in &mut rows {
        row.style = Style::default().add_modifier(Modifier::BOLD);
    }
    // What the action is, then the call itself: the sentence a reader can act on
    // first, the exact arguments under it.
    if let Some(summary) = modal.summary.as_deref() {
        rows.extend(pane::wrap_text(summary.trim(), inner));
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
    rows.push(choices_row(modal.choices));
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
    frame.render_widget(
        Paragraph::new(rows)
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center),
        layout::inner(area),
    );
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

/// The overlay's last row: one `[y] 允许` per key, the key itself picked out so the
/// row reads as buttons rather than as one more sentence to parse.
fn choices_row(choices: &[wording::Choice]) -> Line<'static> {
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
    Line::from(spans)
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
/// session this is, where it is, what mode it runs in, and when.
///
/// Which of the three is drawn is the layout's call ([`layout::HeaderKind`]), not a
/// size test repeated here.
fn draw_header(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &TuiState) {
    draw_border(frame, panes.header);
    match panes.header_kind() {
        layout::HeaderKind::Mark => draw_mark(frame, panes, state),
        // Both text kinds come out of `header_lines`, which reads the height itself.
        layout::HeaderKind::TextOneLine | layout::HeaderKind::TextTwoLines => {
            frame.render_widget(
                Paragraph::new(header_lines(panes.header_content, state)),
                panes.header_content,
            );
        }
    }
}

/// The mark, and the line of facts that survives under it.
///
/// The mark says *what this is*; the line under it says *where and when*, the two
/// things the tall header has no room to spell out: the directory on the left, then
/// the mode and the clock against the right edge. The name and the version are what
/// the mark itself is, so [`wording::identity`] is the one field the tall header
/// gives up.
fn draw_mark(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &TuiState) {
    let content = panes.header_content;
    let lines: Vec<Line<'static>> = mark_lines()
        .iter()
        .map(|(text, color)| {
            Line::from(Span::styled(
                (*text).to_owned(),
                Style::default().fg(*color),
            ))
        })
        .collect();
    let height = lines.len() as u16;
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(content.x, content.y, content.width, height),
    );
    // A blank row, then the facts: the mark says what this is, the air lets it
    // land, and the line under it says where and when. The geometry put the facts
    // on the content's last row, so the blank is what the two leave between them.
    let info_y = content.y + height + layout::LOGO_GAP_ROWS;
    if content.height <= height + layout::LOGO_GAP_ROWS {
        return;
    }
    let info = Line::from(edges(
        &state.facts.cwd,
        &format!(
            "{} · {}",
            wording::mode_field(state.mode),
            wording::clock(&state.clock)
        ),
        content.width as usize,
    ));
    frame.render_widget(
        Paragraph::new(info),
        Rect::new(content.x, info_y, content.width, layout::LOGO_INFO_ROWS),
    );
}

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

/// The header's fields.
///
/// Two lines hold identity and clock, then directory and mode — each pair pushed
/// to the opposite edges so the eye can find them. One line has room for a single
/// run of fields, and drops the directory: the mode matters more (spec §2).
fn header_lines(content: Rect, state: &TuiState) -> Vec<Line<'static>> {
    let width = content.width as usize;
    if content.height <= 1 {
        // The date is the first thing to go when the header is a single line —
        // the time is what a glance is looking for.
        let single = format!(
            "{} · {} · {}",
            wording::identity(),
            wording::mode_field(state.mode),
            wording::clock_short(&state.clock)
        );
        return vec![Line::from(truncate_columns(&single, width))];
    }
    vec![
        Line::from(edges(
            &wording::identity(),
            &wording::clock(&state.clock),
            width,
        )),
        Line::from(edges(
            &state.facts.cwd,
            &wording::mode_field(state.mode),
            width,
        )),
    ]
}

/// One header line: `left` against the left edge, `right` against the right, and
/// whatever space is left between them. When they cannot both fit, the identity
/// survives and the other field is what gets cut.
fn edges(left: &str, right: &str, width: usize) -> String {
    let left_columns = text_columns(left);
    let right_columns = text_columns(right);
    if left_columns + right_columns >= width {
        return truncate_columns(left, width);
    }
    let mut line = String::from(left);
    line.push_str(&" ".repeat(width - left_columns - right_columns));
    line.push_str(right);
    line
}

/// The conversation pane: the transcript's window onto its own scroll buffer,
/// plus the two things that say where the viewport is (spec §3, §4).
fn draw_transcript(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    draw_border(frame, panes.middle);
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
    frame.render_widget(Paragraph::new(rows), panes.transcript);
    draw_scrollbar(frame, panes.scrollbar(), &state.pane);
    draw_indicator(frame, text_area, state);
    if let Some(panel) = panes.panel {
        // The panel's numbers, beside the transcript.
        frame.render_widget(
            Paragraph::new(state.panel.lines(&state.facts, panel)),
            panel,
        );
    }
    if let Some(seam) = panes.seam() {
        // The two panes share one column rather than each drawing a border. Its
        // ends join the middle block's borders instead of crossing them.
        draw_seam(frame, panes.middle, seam);
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
    if state.pane.following() || area.width == 0 || area.height == 0 {
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

/// The shared seam between the conversation pane and the panel.
fn draw_seam(frame: &mut ratatui::Frame, middle: Rect, x: u16) {
    let style = Style::default().fg(Color::DarkGray);
    let top = middle.y;
    let bottom = middle.y + middle.height - 1;
    let buffer = frame.buffer_mut();
    for y in top..=bottom {
        let symbol = if y == top {
            "┬"
        } else if y == bottom {
            "┴"
        } else {
            "│"
        };
        buffer[(x, y)].set_symbol(symbol).set_style(style);
    }
}

/// The bottom block: the input line, and under it the hints that say what the keys
/// do.
///
/// Returns where the cursor was put, so whatever floats over the pane can anchor
/// itself to it — the `/` menu follows the cursor (spec §6). `None` while a question
/// is up, because there is no cursor then.
///
/// A questionnaire replaces the input line with itself. That is the whole point of
/// this kind of question: the middle overlay suits a one-line confirmation, while a
/// questionnaire is several rows and pages, so it takes the area built for typing
/// (spec §19).
fn draw_bottom(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &TuiState,
) -> Option<editor::Placed> {
    draw_border(frame, panes.bottom);
    if let Some(questionnaire) = state.questionnaire() {
        draw_questionnaire(frame, panes, questionnaire);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                wording::questionnaire_status(
                    questionnaire.index,
                    questionnaire.questions.len(),
                    questionnaire.all_handled(),
                ),
                Style::default().fg(Color::DarkGray),
            ))),
            panes.hints,
        );
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
fn draw_questionnaire(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    questionnaire: &Questionnaire,
) {
    let rows = questionnaire_window(
        &questionnaire.questions[questionnaire.index],
        &questionnaire.drafts[questionnaire.index],
        panes.input.width as usize,
        panes.input.height as usize,
    );
    frame.render_widget(Paragraph::new(rows), panes.input);
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
    /// A line with nothing behind it.
    fn plain(line: Line<'static>) -> Self {
        Self { line, link: None }
    }

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
        Self::plain(line)
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
                .map(RenderedLine::plain)
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
        .map(RenderedLine::plain)
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

/// A finished tool call, folded to two things: the **call line** that stays in the
/// transcript, and, behind it, the whole output (票 02 §3).
///
/// A failure is the same line with `失败` at its **end** — not a second line — and
/// the error body moves into the detail. The post-hook's feedback stays on screen:
/// it is policy feedback, not tool output, so it has to be readable without a click
/// (票 02 §3).
fn tool_block_lines(tool: &ToolBlock, colors: &mut SpeakerColors) -> Vec<RenderedLine> {
    let failed = matches!(&tool.outcome, Some(outcome) if !outcome.ok);
    let mut call = vec![
        // The marker is what says the line can be opened; it is paint, not wording,
        // so it is not part of the sentence (票 03 §Answer).
        Span::styled("▸ ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{} ", speaker_label(&tool.speaker)),
            name_style(&tool.speaker, colors),
        ),
        Span::styled(
            format!(
                "{} {} {}",
                wording::tool_call_label(),
                tool.tool,
                summarize_args(&tool.args)
            ),
            Style::default().add_modifier(Modifier::BOLD),
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
    let mut lines = vec![RenderedLine::linked(Line::from(call), detail)];
    if let Some(hook) = &tool.hook {
        lines.push(
            Line::from(Span::styled(
                format!("  {}", wording::hook_feedback(hook)),
                Style::default().fg(Color::Yellow),
            ))
            .into(),
        );
    }
    lines
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
    kind: DetailKind,
}

/// The two things a detail view can be about.
#[derive(Clone)]
enum DetailKind {
    /// A finished thinking segment. `text` is the whole trace when the stream
    /// recorded one; `unrecorded` is the synthesizer's case, where deltas arrived
    /// and the log holds no text (票 02 §1).
    Thinking {
        text: Option<String>,
        unrecorded: bool,
    },
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
    /// The row the overlay was opened from, so a second click there closes it.
    row: usize,
    /// What is being shown.
    detail: Detail,
    /// The body, laid out at the width it was opened at.
    body: Vec<Line<'static>>,
    /// The first body row on screen.
    top: usize,
    /// Body rows the overlay can show at once.
    height: usize,
}

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
    fn open_detail(&mut self, row: usize, detail: Detail, width: usize) {
        let body = detail_body(&detail, &self.facts.cwd, width);
        self.detail = Some(DetailView {
            row,
            detail,
            body,
            top: 0,
            height: 0,
        });
    }

    /// Close it, wherever it was opened from.
    fn close_detail(&mut self) {
        self.detail = None;
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

    /// The display row a click again landed on, when it is the row the overlay was
    /// opened from.
    fn detail_reselected(&self, mouse: &MouseEvent) -> bool {
        let Some(view) = self.detail.as_ref() else {
            return false;
        };
        let Some(offset) = mouse.row.checked_sub(self.drawn_top) else {
            return false;
        };
        match self.drawn_rows.get(offset as usize) {
            Some(Some(row)) => *row == view.row,
            _ => false,
        }
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
        DetailKind::Thinking { text, unrecorded } => {
            rows.push(section_header(wording::detail_thinking_section()));
            match text {
                Some(text) if !text.trim().is_empty() => {
                    rows.extend(pane::wrap_text(text.trim_end(), width));
                }
                // Both an absent trace and an unrecorded one say the same thing; the
                // flag is kept so a later change can tell the two apart (票 02 §1).
                _ => {
                    let _ = unrecorded;
                    rows.push(Line::from(Span::styled(
                        wording::detail_reasoning_unrecorded(),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
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
/// available.
fn read_tool_body(tool_call_id: &ToolCallId, preview: &str, session_dir: &str) -> (String, bool) {
    // `SessionFacts.cwd` holds the **session directory**, so the outputs directory
    // is one join away — the same arithmetic the harness does (票 01 事实 56).
    let path = std::path::Path::new(session_dir)
        .join(crate::session::store::OUTPUTS_DIR)
        .join(format!("{tool_call_id}.txt"));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (
            format!("{preview}\n{}", wording::detail_output_unavailable()),
            false,
        );
    };
    if text.chars().count() <= DETAIL_MAX_CHARS {
        return (text, false);
    }
    let cut: String = text.chars().take(DETAIL_MAX_CHARS).collect();
    (cut, true)
}

/// Paint the detail overlay over the middle block.
///
/// It owns the keyboard and the wheel while it is up, and the transcript stays
/// frozen where it was — a reading position, not a moving one (票 02 §4).
fn draw_detail(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let Some(area) = panes.detail() else {
        // Nowhere to draw it: leaving it open would keep the keyboard captured for a
        // view nobody can see.
        state.detail = None;
        return;
    };
    let Some(view) = state.detail.as_ref() else {
        return;
    };
    let inner = layout::inner(area);
    let height = inner.height as usize;
    let body_rows = height.saturating_sub(2);
    let max_top = view.body.len().saturating_sub(body_rows);
    let top = view.top.min(max_top);
    let rows: Vec<Line<'static>> = view
        .body
        .iter()
        .skip(top)
        .take(body_rows)
        .cloned()
        .collect();
    let footer = wording::detail_footer(top + 1, view.body.len().max(1));
    let title = view.detail.title.clone();

    blank_half_covered_glyphs(frame, area);
    frame.render_widget(Clear, area);
    frame.render_widget(
        WidgetBlock::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
        area,
    );
    // The title row is the clicked line's own text, so the reader knows which line
    // they opened.
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_columns(&title, inner.width as usize),
            Style::default().add_modifier(Modifier::BOLD),
        ))),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    // The body takes everything between the title and the footer; the footer is
    // pinned to the overlay's last inner row, so the two cannot overlap (票 03
    // §Answer).
    let body = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(2),
    );
    frame.render_widget(Paragraph::new(rows), body);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            footer,
            Style::default().fg(Color::DarkGray),
        ))),
        Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
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
