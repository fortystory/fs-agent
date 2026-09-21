//! The ratatui interface (spec §19).
//!
//! Three properties are structural, not stylistic:
//!
//! * **Inline viewport.** The terminal is never switched to the alternate
//!   screen, so the transcript lands in the real scrollback and stays scrollable
//!   and copyable. Finalized blocks are pushed above the live region with
//!   `Terminal::insert_before`; the live region only ever shows the streaming
//!   tail, the input line and the status line.
//! * **The renderer owns the keyboard.** It is the only task reading terminal
//!   events, and it answers the loop's requests ([`ConsoleRequest`]) over the
//!   injected console channel. That is what keeps input and output from fighting.
//! * **`select!` over broadcast / tick / keys.** Render events, a redraw tick and
//!   keyboard input are three independent sources; `select!` is how they are
//!   merged without a second channel whose ordering would be undefined.

use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::crossterm::event::{
    Event as CtEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use ratatui::{TerminalOptions, Viewport};
use tokio::sync::broadcast;

use crate::events::{Role, StopReason};

use super::highlight::{diff_tag, highlight_diff};
use super::input::{AnswerChoice, ConsolePort, ConsoleRequest, FrontEndEvent, Question};
use super::severity::Severity;
use super::transcript::{summarize_args, truncate, Block, ToolBlock, Transcript};
use super::wording::{self, speaker_label};
use super::{Render, RenderEvent};

/// How many rows the live region occupies: the streaming tail, the input line and
/// the status line.
const LIVE_HEIGHT: u16 = 8;

/// Rows of streaming text the live region keeps.
const LIVE_ROWS: usize = LIVE_HEIGHT as usize - 2;

/// How much streamed text is retained before it is trimmed to a tail. The
/// transcript does not need the whole message live: the completed `Message`
/// block re-renders it in full.
const LIVE_BUFFER: usize = 4_000;

/// How much of one tool result the TUI shows before eliding.
const TOOL_PREVIEW: usize = 4_000;

/// How often the live region is redrawn even without an event.
const TICK: Duration = Duration::from_millis(120);

/// The keys the TUI acts on.
///
/// Deliberately its own vocabulary rather than crossterm's: the state machine is
/// then testable without a terminal, and a backend change cannot silently move a
/// binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    Enter,
    Esc,
    BackTab,
    CtrlC,
}

/// Translate one crossterm keypress into a [`Key`], or `None` for a key the TUI
/// ignores.
fn map_key(key: KeyEvent) -> Option<Key> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Key::CtrlC);
    }
    match key.code {
        KeyCode::Esc => Some(Key::Esc),
        KeyCode::BackTab => Some(Key::BackTab),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Char(ch) => Some(Key::Char(ch)),
        KeyCode::Backspace => Some(Key::Backspace),
        _ => None,
    }
}

/// The TUI's injected values: the front end's end of the console channel.
pub struct TuiOptions {
    pub port: ConsolePort,
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
        let TuiOptions { mut port } = self.options;
        let mut state = TuiState::new();
        let mut terminal = ratatui::init_with_options(TerminalOptions {
            viewport: Viewport::Inline(LIVE_HEIGHT),
        });
        let mut keys = EventStream::new();
        let mut tick = tokio::time::interval(TICK);
        // The first tick fires immediately; soak it so the first frame is drawn
        // from state rather than from an empty buffer.
        tick.tick().await;

        loop {
            tokio::select! {
                received = receiver.recv() => match received {
                    Ok(event) => state.apply(event),
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        state.apply(RenderEvent::Diagnostic(wording::renderer_dropped(dropped)));
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                maybe_key = keys.next() => {
                    if let Some(Ok(CtEvent::Key(key))) = maybe_key {
                        if key.kind == KeyEventKind::Press {
                            if let Some(key) = map_key(key) {
                                state.key(key);
                            }
                        }
                    }
                }
                request = port.recv() => match request {
                    Some(request) => state.request(request),
                    None => break,
                },
                _ = tick.tick() => {}
            }

            for event in state.take_events() {
                port.emit(event);
            }
            for block in state.take_ready() {
                let lines = render_block(&block);
                if lines.is_empty() {
                    continue;
                }
                let height = lines.len().min(u16::MAX as usize) as u16;
                let _ = terminal.insert_before(height, |buf| paint_scrollback(&lines, buf));
            }
            let _ = terminal.draw(|frame| draw_live(frame, &state));
            if state.should_quit() {
                break;
            }
        }

        // The alternate screen was never entered, but raw mode still has to go.
        ratatui::restore();
    }
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
    transcript: Transcript,
    /// Completed blocks waiting to be pushed into scrollback.
    ready: Vec<Block>,
    /// The streaming tail of the current message.
    live: String,
    /// What the user has typed.
    input: String,
    /// Where a `Prompt` request's answer goes.
    prompt_reply: Option<tokio::sync::oneshot::Sender<Option<String>>>,
    /// A question waiting for a keypress.
    pending: Option<Pending>,
    /// Gestures to hand back to the loop.
    events: Vec<FrontEndEvent>,
    /// Whether a turn is in flight, which decides what Esc and Ctrl-C mean.
    busy: bool,
    quit: bool,
}

struct Pending {
    question: Question,
    reply: tokio::sync::oneshot::Sender<AnswerChoice>,
}

impl TuiState {
    pub fn new() -> Self {
        Self {
            transcript: Transcript::new(),
            ready: Vec::new(),
            live: String::new(),
            input: String::new(),
            prompt_reply: None,
            pending: None,
            events: Vec::new(),
            busy: false,
            quit: false,
        }
    }

    /// Feed one render event.
    pub fn apply(&mut self, event: RenderEvent) {
        for block in self.transcript.push(event) {
            match block {
                Block::Delta { text, .. } => {
                    self.busy = true;
                    self.live.push_str(&text);
                    if self.live.len() > LIVE_BUFFER {
                        let cut = self.live.len() - LIVE_BUFFER;
                        // Trim on a char boundary.
                        let cut = (cut..self.live.len())
                            .find(|index| self.live.is_char_boundary(*index))
                            .unwrap_or(self.live.len());
                        self.live.drain(..cut);
                    }
                }
                Block::Tool(_) => {
                    self.busy = true;
                    self.ready.push(block);
                }
                Block::Message { .. } => {
                    // The deltas were the live view; the block is the permanent
                    // one, so the tail can go.
                    self.live.clear();
                    self.ready.push(block);
                }
                Block::TurnEnded { .. } | Block::SessionEnded { .. } => {
                    self.busy = false;
                    self.ready.push(block);
                }
                other => self.ready.push(other),
            }
        }
    }

    /// Answer a request from the loop.
    pub fn request(&mut self, request: ConsoleRequest) {
        match request {
            ConsoleRequest::Prompt { reply } => self.prompt_reply = Some(reply),
            ConsoleRequest::Ask(ask) => {
                self.pending = Some(Pending {
                    question: ask.question,
                    reply: ask.reply,
                });
            }
        }
    }

    pub fn take_ready(&mut self) -> Vec<Block> {
        std::mem::take(&mut self.ready)
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
        match key {
            Key::CtrlC => {
                if self.busy {
                    self.events.push(FrontEndEvent::Cancel);
                } else {
                    self.quit = true;
                }
                return;
            }
            Key::Esc => {
                if self.busy {
                    self.events.push(FrontEndEvent::Cancel);
                } else if self.pending.is_some() {
                    self.answer(self.default_answer());
                } else {
                    self.input.clear();
                }
                return;
            }
            Key::BackTab => {
                self.events.push(FrontEndEvent::TogglePlan);
                return;
            }
            _ => {}
        }
        if self.pending.is_some() {
            self.answer_key(key);
            return;
        }
        match key {
            Key::Enter => {
                let line = self.input.trim().to_owned();
                self.input.clear();
                if let Some(reply) = self.prompt_reply.take() {
                    let _ = reply.send(if line.is_empty() { None } else { Some(line) });
                }
            }
            Key::Char(ch) => self.input.push(ch),
            Key::Backspace => {
                self.input.pop();
            }
            _ => {}
        }
    }

    fn answer_key(&mut self, key: Key) {
        let choice = match (&self.pending.as_ref().map(|p| &p.question), key) {
            (Some(Question::Permission(_)), Key::Char('y')) => {
                AnswerChoice::Permission(crate::permissions::Answer::Allow)
            }
            (Some(Question::Permission(_)), Key::Char('a')) => {
                AnswerChoice::Permission(crate::permissions::Answer::AlwaysAllow)
            }
            (Some(Question::PlanConflict(_)), Key::Char('o')) => {
                AnswerChoice::Plan(crate::permissions::PlanConflict::Overwrite)
            }
            (Some(Question::PlanConflict(_)), Key::Char('a')) => {
                AnswerChoice::Plan(crate::permissions::PlanConflict::Append)
            }
            (Some(Question::PlanConflict(_)), Key::Char('k')) => {
                AnswerChoice::Plan(crate::permissions::PlanConflict::Keep)
            }
            _ => self.default_answer(),
        };
        self.answer(choice);
    }

    /// The non-acting answer: deny a permission, keep a plan file.
    fn default_answer(&self) -> AnswerChoice {
        match self.pending.as_ref().map(|p| &p.question) {
            Some(Question::PlanConflict(_)) => {
                AnswerChoice::Plan(crate::permissions::PlanConflict::Keep)
            }
            _ => AnswerChoice::Permission(crate::permissions::Answer::Deny),
        }
    }

    fn answer(&mut self, choice: AnswerChoice) {
        if let Some(pending) = self.pending.take() {
            let _ = pending.reply.send(choice);
        }
    }

    /// The streaming tail, wrapped to `width` **columns** and capped to the live
    /// rows.
    pub fn live_lines(&self, width: u16) -> Vec<String> {
        let width = width.max(1) as usize;
        let mut lines: Vec<String> = Vec::new();
        for raw in self.live.split('\n') {
            if raw.is_empty() {
                lines.push(String::new());
                continue;
            }
            let mut rest = raw;
            while !rest.is_empty() {
                let take = wrap_take(rest, width);
                lines.push(rest[..take].to_owned());
                rest = &rest[take..];
            }
        }
        if lines.len() > LIVE_ROWS {
            lines.split_off(lines.len() - LIVE_ROWS)
        } else {
            lines
        }
    }

    /// The column the cursor rests on: the two prompt cells, then the input's
    /// **display width** — a CJK character occupies two columns.
    ///
    /// Here rather than inline in [`draw_live`] so it can be asserted without a
    /// terminal, like the rest of the display state.
    pub fn cursor_column(&self) -> u16 {
        2 + self.input.as_str().cell_width()
    }

    /// The input line: a question prompt while one is pending, else the prompt.
    fn input_line(&self) -> (String, Style) {
        match &self.pending {
            Some(Pending {
                question: Question::Permission(request),
                ..
            }) => (
                wording::permission_prompt(&request.tool_name, &summarize_args(&request.args)),
                Style::default().fg(ratatui::style::Color::Yellow),
            ),
            Some(Pending {
                question: Question::PlanConflict(path),
                ..
            }) => (
                wording::plan_conflict_prompt(&path.display().to_string()),
                Style::default().fg(ratatui::style::Color::Yellow),
            ),
            None => (
                format!("> {}", self.input),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        }
    }

    fn status_line(&self, width: u16) -> String {
        wording::status_line(self.busy, width)
    }
}

impl Default for TuiState {
    fn default() -> Self {
        Self::new()
    }
}

/// How many bytes of `text` fit into `width` terminal columns.
///
/// Columns, not bytes: a CJK character is three bytes wide and two columns, so
/// [counting bytes](TuiState::live_lines) wrapped the streaming tail at roughly a
/// third of the terminal width.
///
/// Always takes at least one character, even one wider than the whole line: the
/// caller loops until the remainder is empty, so a zero-length take would spin.
/// Such a line still overflows its width — this only keeps the loop moving.
///
/// Per character rather than per grapheme because `Span::styled_graphemes` drops
/// control characters, whose bytes could then not be turned back into an offset.
/// The cost is that an emoji sequence built from several characters counts as
/// wider than it draws, which only wraps it earlier than it had to.
fn wrap_take(text: &str, width: usize) -> usize {
    let mut used = 0;
    let mut buf = [0u8; 4];
    for (index, ch) in text.char_indices() {
        let columns = ch.encode_utf8(&mut buf).cell_width() as usize;
        if used + columns > width {
            return if index == 0 { ch.len_utf8() } else { index };
        }
        used += columns;
    }
    text.len()
}

/// Draw the live region: the streaming tail, the input line and the status line.
fn draw_live(frame: &mut ratatui::Frame, state: &TuiState) {
    let area = frame.area();
    let mut lines: Vec<Line> = state
        .live_lines(area.width)
        .into_iter()
        .map(Line::from)
        .collect();
    while lines.len() < LIVE_ROWS {
        lines.insert(0, Line::from(""));
    }
    let (input, style) = state.input_line();
    lines.push(Line::from(Span::styled(input, style)));
    lines.push(Line::from(Span::styled(
        state.status_line(area.width),
        Style::default().fg(ratatui::style::Color::DarkGray),
    )));
    frame.render_widget(Paragraph::new(lines), area);
    // The cursor sits at the end of the typed input, on the input line.
    frame.set_cursor_position((
        state.cursor_column().min(area.width.saturating_sub(1)),
        area.y + LIVE_ROWS as u16,
    ));
}

/// Paint `lines` into the scratch buffer [`Terminal::insert_before`] hands us.
///
/// That buffer is not diffed on the way out: `insert_before` walks every cell
/// and the backend prints each cell's symbol, where [`ratatui::buffer::Buffer`]'s
/// own diff would have skipped the trailing columns of a wide grapheme. Those
/// trailing cells hold `Cell::EMPTY`, whose symbol is a space, so a CJK
/// transcript printed a blank column after every wide character and each line
/// drew wider than the width its cells were laid out for.
///
/// Blanking those cells to the empty string stops the backend printing anything
/// there. It still walks on to the next cell, and the cursor is already there:
/// printing the wide grapheme moved the terminal two columns.
pub fn paint_scrollback(lines: &[Line<'_>], buf: &mut Buffer) {
    for (index, line) in lines.iter().enumerate() {
        let area = Rect {
            x: 0,
            y: index as u16,
            width: buf.area.width,
            height: 1,
        };
        line.clone().render(area, buf);
    }
    for y in 0..buf.area.height {
        let mut x = 0;
        while x < buf.area.width {
            let width = buf[(x, y)].cell_width().max(1);
            let end = x.saturating_add(width).min(buf.area.width);
            for trailing in x.saturating_add(1)..end {
                buf[(trailing, y)].set_symbol("");
            }
            x = x.saturating_add(width);
        }
    }
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
            let prefix = format!("{} ", speaker_label(speaker));
            let indent = " ".repeat(prefix.as_str().cell_width() as usize);
            super::markdown::to_lines(text)
                .into_iter()
                .enumerate()
                .map(|(index, line)| {
                    let lead = if index == 0 { &prefix } else { &indent };
                    let mut spans = vec![Span::styled(
                        lead.clone(),
                        Style::default().fg(ratatui::style::Color::DarkGray),
                    )];
                    spans.extend(line.spans);
                    Line::from(spans)
                })
                .collect()
        }
        Block::Message { speaker, text, .. } => vec![Line::from(vec![
            Span::styled(
                format!("{} ", speaker_label(speaker)),
                Style::default().fg(ratatui::style::Color::DarkGray),
            ),
            Span::raw(truncate(text, 500)),
        ])],
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
            vec![narration(wording::context_injected(*source))]
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
