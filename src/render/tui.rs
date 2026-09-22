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

use crate::events::{ContextSource, HistoryReason, Role, StopReason};
use crate::permissions::Mode;

use super::editor::{self, Input};
use super::highlight::{diff_tag, highlight_diff};
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
use super::{Render, RenderEvent};

/// How much streamed text is retained before it is trimmed to a tail. The
/// transcript does not need the whole message live: the completed `Message`
/// block re-renders it in full.
const LIVE_BUFFER: usize = 4_000;

/// How much of one tool result the TUI shows before eliding.
const TOOL_PREVIEW: usize = 4_000;

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
    /// Where a `Prompt` request's answer goes.
    prompt_reply: Option<tokio::sync::oneshot::Sender<Option<String>>>,
    /// Whether the loop says it is inside a run. **Pushed by the loop**, never
    /// inferred here: see [`TuiState::busy`].
    running: bool,    /// A question waiting for a keypress.
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
}

impl Pending {
    /// The rows the overlay shows for this question. It goes over the transcript, so
    /// the draft stays where the user left it (spec §7, §9).
    fn modal(&self) -> Modal {
        match self {
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
        }
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
            // shows what it says to the reader.
            self.panel.observe(&block);
            for line in render_block(&block) {
                self.pane.push(line);
            }
        }
    }

    /// Handle one mouse event.
    ///
    /// Only two things answer to the mouse: the wheel scrolls the transcript, and
    /// a click on the "back to bottom" indicator returns to the bottom. Every
    /// other click is ignored — the terminal's own selection is the user's, and
    /// nothing here takes focus (spec §4).
    pub fn mouse(&mut self, mouse: MouseEvent) {
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
                }
            }
            _ => {}
        }
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
                if !running && matches!(self.pending, Some(Pending::Loop { .. })) {
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

    /// Handle one keypress. Answers and submissions go out through the pending
    /// one-shot channels; gestures are queued for the loop.
    pub fn key(&mut self, key: Key) {
        self.dirty = true;
        match key {
            Key::CtrlC => {
                if self.busy() {
                    self.events.push(FrontEndEvent::Cancel);
                } else {
                    self.quit = true;
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
            if matches!(key, Key::Char(_) | Key::Enter) {
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
        }
    }

    /// What `Esc` means for a question: the non-acting answer, or nothing at all
    /// when the question was this renderer's own.
    fn decline(&mut self, pending: Pending) {
        if let Pending::Loop { question, reply } = pending {
            let _ = reply.send(default_choice(&question));
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
}

/// Draw one frame of the four-pane layout.
///
/// This is the seam the layout is tested through: a state goes in, a fixed-size
/// frame comes out, and no terminal is involved (spec §2).
pub fn draw_frame(frame: &mut ratatui::Frame, state: &mut TuiState) {
    let area = frame.area();
    if layout::below_minimum(area) {
        // Nothing is drawn that a click could land on.
        state.indicator = None;
        draw_too_small(frame, area);
        return;
    }
    // The draft's own height decides how much room the input takes: it grows with
    // the text up to the layout's cap and then scrolls internally (spec §5).
    let draft_rows = state.editor.height(layout::input_text_width(area));
    let panes = layout::plan(area, draft_rows);
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
}

/// The overlay a question is asked in (spec §9).
///
/// It sits over the middle block — transcript and panel both — so the question cannot
/// be outrun by new output, and it is **not** part of the transcript: the stream still
/// carries the `PermissionAsked` block for anyone reading back. It owns the pointer
/// while it is up, so the "back to bottom" rectangle is dropped.
fn draw_modal(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let Some(modal) = state.pending.as_ref().map(Pending::modal) else {
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
    // The info row is the mark's last header row; the geometry put it there.
    let info_y = content.y + height;
    if content.height <= height {
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
fn draw_bottom(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &TuiState,
) -> Option<editor::Placed> {
    draw_border(frame, panes.bottom);
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
fn attribute(speaker: &crate::events::SpeakerId, rows: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let prefix = format!("{} ", speaker_label(speaker));
    let indent = " ".repeat(prefix.as_str().cell_width() as usize);
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            let lead = if index == 0 {
                prefix.clone()
            } else {
                indent.clone()
            };
            let mut spans = vec![Span::styled(lead, Style::default().fg(Color::DarkGray))];
            spans.extend(row.spans);
            Line {
                spans,
                style: row.style,
                alignment: row.alignment,
            }
        })
        .collect()
}

/// Turn one finalized block into styled terminal lines.
///
/// This is the TUI half of the shared presentation layer: the block was decided
/// once by [`Transcript`], and only the painting happens here.
pub fn render_block(block: &Block) -> Vec<Line<'static>> {
    match block {
        Block::Message {
            speaker,
            role: Role::Assistant,
            text,
        } => {
            if text.is_empty() {
                return Vec::new();
            }
            // The answer is rendered as Markdown at full brightness; only the
            // speaker label repeats, in the narration grey.
            attribute(speaker, super::markdown::to_lines(text))
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
        ),
        Block::Delta { .. } => Vec::new(),
        Block::RoundStarted { round, mode } => vec![Line::from(Span::styled(
            wording::round_section(*round, *mode),
            Style::default()
                .fg(ratatui::style::Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))],
        Block::RoundEnded { round, reason } => vec![severity_line(
            *reason,
            wording::round_ended(*round, *reason),
        )],
        Block::Divergence { topic, positions } => {
            let mut lines = vec![Line::from(Span::styled(
                format!("!! {}", wording::divergence(topic)),
                Style::default()
                    .fg(ratatui::style::Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ))];
            for position in positions {
                lines.push(Line::from(format!("  - {position}")));
            }
            lines
        }
        Block::Tool(tool) => tool_lines(tool),
        Block::TurnStarted { speaker, iteration } => vec![narration(format!(
            "{} {}",
            speaker_label(speaker),
            wording::turn_started(*iteration)
        ))],
        Block::TurnEnded { speaker, reason } => vec![severity_line(
            *reason,
            format!(
                "{} {}",
                speaker_label(speaker),
                wording::turn_ended(*reason)
            ),
        )],
        Block::PermissionAsked {
            speaker,
            tool_name,
            args,
        } => vec![narration(format!(
            "{} {}",
            speaker_label(speaker),
            wording::permission_asked(tool_name.as_deref(), &summarize_args(args))
        ))],
        Block::PermissionDecided {
            speaker,
            decision,
            source,
            reason,
        } => vec![narration(format!(
            "{} {}",
            speaker_label(speaker),
            wording::permission_decided(*decision, *source, reason.as_deref())
        ))],
        Block::Hook {
            speaker,
            point,
            outcome,
        } => vec![narration(format!(
            "{} {}",
            speaker_label(speaker),
            wording::hook(point, outcome)
        ))],
        Block::ExecutorSpawned {
            speaker,
            executor_id,
        } => vec![narration(format!(
            "{} {}",
            speaker_label(speaker),
            wording::executor_spawned(executor_id.as_str())
        ))],
        Block::ExecutorFinished {
            executor_id,
            reason,
            summary,
        } => vec![severity_line(
            *reason,
            wording::executor_finished(executor_id.as_str(), *reason, summary),
        )],
        Block::Usage { speaker, usage } => vec![narration(format!(
            "{} {}",
            speaker_label(speaker),
            wording::usage_summary(usage)
        ))],
        Block::AgentError { speaker, message } => vec![severity_line(
            StopReason::Error,
            format!(
                "{} {}",
                speaker_label(speaker),
                wording::agent_error(message)
            ),
        )],
        Block::SessionError { code, detail } => vec![severity_line(
            StopReason::Error,
            wording::session_error(code, detail),
        )],
        Block::SessionEnded { reason } => {
            vec![severity_line(*reason, wording::session_ended(*reason))]
        }
        Block::ContextInjected { source } => {
            vec![narration(wording::context_injected(source.clone()))]
        }
        Block::History { reason, summary } => {
            vec![narration(wording::history(*reason, summary.as_deref()))]
        }
        Block::Diagnostic(message) => vec![Line::from(Span::styled(
            wording::diagnostic(message),
            Style::default().fg(ratatui::style::Color::Yellow),
        ))],
        Block::Notice(message) => vec![narration(message.clone())],
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
    let style = match Severity::of(reason) {
        Severity::Good => Style::default().fg(ratatui::style::Color::Green),
        Severity::Note => Style::default().fg(ratatui::style::Color::Cyan),
        Severity::Warn => Style::default().fg(ratatui::style::Color::Yellow),
        Severity::Bad => Style::default()
            .fg(ratatui::style::Color::Red)
            .add_modifier(Modifier::BOLD),
    };
    Line::from(Span::styled(text, style))
}

fn tool_lines(tool: &ToolBlock) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("{} ", speaker_label(&tool.speaker)),
            Style::default().fg(ratatui::style::Color::DarkGray),
        ),
        Span::styled(
            format!(
                "→ {}",
                wording::tool_call(&tool.tool, &summarize_args(&tool.args))
            ),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ])];
    match &tool.outcome {
        Some(outcome) if outcome.ok => {
            if let Some(output) = &outcome.output {
                if !output.trim().is_empty() {
                    lines.extend(highlighted(&wording::tool_output_preview(
                        output,
                        TOOL_PREVIEW,
                    )));
                }
            }
        }
        Some(outcome) => {
            let error = outcome
                .error
                .as_deref()
                .unwrap_or_else(|| wording::no_message())
                .to_owned();
            lines.push(Line::from(Span::styled(
                format!("  {error}"),
                Style::default().fg(ratatui::style::Color::Red),
            )));
        }
        None => lines.push(Line::from(format!("  {}", wording::no_tool_result()))),
    }
    if let Some(hook) = &tool.hook {
        lines.push(Line::from(Span::styled(
            format!("  {}", wording::hook_feedback(hook)),
            Style::default().fg(ratatui::style::Color::Yellow),
        )));
    }
    lines
}

/// Syntax highlighting and diff coloring, composed for one tool result.
///
/// The two layers are computed independently and patched together: the syntax
/// class is the foreground and the diff tag the background, so an added keyword
/// is both.
fn highlighted(text: &str) -> Vec<Line<'static>> {
    let classes = highlight_diff(text);
    text.split('\n')
        .zip(classes)
        .map(|(raw, spans)| {
            let tag = diff_tag(raw);
            let spans: Vec<Span> = if spans.is_empty() {
                vec![Span::raw(String::new())]
            } else {
                spans
                    .into_iter()
                    .map(|span| {
                        let style = span.class.style().patch(tag.style());
                        Span::styled(span.text, style)
                    })
                    .collect()
            };
            Line::from(spans)
        })
        .collect()
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
