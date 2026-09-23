//! The keyboard seam between the loop and whichever front end owns the
//! terminal (spec §19, user story 133).
//!
//! Two facts shape it:
//!
//! * **Input belongs to the renderer.** In TUI mode the terminal is in raw mode
//!   and one task owns every key; in plain mode a single reader task owns stdin.
//!   Either way the loop never touches the terminal directly.
//! * **The loop asks for what it needs.** A line is read only when the loop is
//!   ready for one, and an answer only when the gate has asked a question. A
//!   reader that read ahead would swallow the answer to a permission question as
//!   if it were the next prompt — so the traffic is request-driven, not a stream.
//!
//! The loop side is [`ConsoleHandle`]; the front end side is [`ConsolePort`].
//! [`ConsoleAsker`] implements the permission gate's [`Asker`] port on top of the
//! same handle, so a question and a prompt travel the one keyboard.

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use crate::events::Event;
use crate::permissions::{Answer, Asker, PermissionRequest, PlanConflict};
use crate::questions::{UserAnswer, UserAnswers, UserQuestion, UserQuestions};

/// A question the front end must put to the user.
#[derive(Debug, Clone, PartialEq)]
pub enum Question {
    /// The permission gate answered `Ask`.
    Permission(PermissionRequest),
    /// Plan mode is being entered and `PLAN.md` already exists (spec §13).
    PlanConflict(PathBuf),
}

/// The user's answer to a [`Question`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerChoice {
    Permission(Answer),
    Plan(PlanConflict),
}

impl AnswerChoice {
    pub fn as_permission(self) -> Answer {
        match self {
            AnswerChoice::Permission(answer) => answer,
            // A plan answer cannot arrive for a permission question: the two are
            // paired by construction. Deny is the safe reading if that is ever
            // violated, because it is the non-acting one.
            AnswerChoice::Plan(_) => Answer::Deny,
        }
    }

    pub fn as_plan(self) -> PlanConflict {
        match self {
            AnswerChoice::Plan(conflict) => conflict,
            // The non-destructive reading, for the same reason as above.
            AnswerChoice::Permission(_) => PlanConflict::Keep,
        }
    }
}

/// One question plus the one-shot channel its answer comes back on.
#[derive(Debug)]
pub struct AskRequest {
    pub question: Question,
    pub reply: oneshot::Sender<AnswerChoice>,
}

/// A model-initiated questionnaire the front end must put to the user (spec §7).
///
/// It has its own reply channel rather than a [`Question`] variant on purpose:
/// the permission gate's [`AnswerChoice`] cannot express a questionnaire answer,
/// and making the gate carry a shape it can never produce is what the third asker
/// exists to avoid (spec §19).
#[derive(Debug)]
pub struct QuestionnaireRequest {
    pub questions: Vec<UserQuestion>,
    /// The answers, or a model-readable reason none came back (input ended, or
    /// the run was cancelled). Dropping the sender is read as the same "no
    /// answer", so a cancelled run never leaves the tool hanging.
    pub reply: oneshot::Sender<Result<UserAnswers, String>>,
}

/// One name a leading `/` can become, and the line that says what it does.
///
/// The catalog is the **loop's** list, not the renderer's: the loop is what turns a
/// submission into an action, so the loop is what knows which names exist. A front
/// end only offers them — it never decides what one means (spec §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// The name without its slash, exactly as it must be typed.
    pub name: String,
    /// One line for a menu's second column. Empty when there is nothing to say.
    pub description: String,
}

impl CatalogEntry {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }
}

/// What the loop asks the front end for.
#[derive(Debug)]
pub enum ConsoleRequest {
    /// The next user line.
    ///
    /// **`None` means end of input**, and nothing else: an empty line is
    /// `Some(String::new())`. A front end whose input cannot end — the TUI, where
    /// Enter on an empty draft is just an empty draft — must never send `None`, or the
    /// loop reads it as a closed stdin and stops (spec §6).
    Prompt {
        reply: oneshot::Sender<Option<String>>,
    },
    /// Put a question to the user.
    Ask(AskRequest),
    /// Put the model's questionnaire to the user (spec §7). Its answer type is
    /// different from [`Ask`](Self::Ask)'s, so it travels on its own channel.
    Questionnaire(QuestionnaireRequest),
    /// The names a leading `/` can become.
    ///
    /// Pushed once, right after assembly, because the skills come from the session and
    /// nothing can list them earlier. A front end that draws no menu — the plain
    /// console — has nothing to do with it.
    Catalog { entries: Vec<CatalogEntry> },
    /// Whether the loop is **inside a run** — a turn, or a discussion it is driving.
    ///
    /// The loop is the only thing that knows this, so it says so rather than letting a
    /// front end infer it (spec §6). Inferring it from the render stream failed for
    /// anything that is not a turn — the synthesizer's single call emits deltas and ends
    /// no `TurnEnded`; inferring it from "no prompt is outstanding" failed at startup,
    /// before the loop has asked for its first line yet. Both mistakes turn `Ctrl-C`
    /// into a cancel gesture the idle loop discards, which reads as a dead keyboard.
    RunState { running: bool },
    /// The event stream a reopened session opened with, for the front end to lay
    /// out as history (`.scratch/tui-history-replay/spec.md` §1).
    ///
    /// Pushed once, right after assembly and before the startup banner, because the
    /// payload is the **assembled** snapshot — which includes the synthetic results
    /// `--continue`'s recovery wrote for dangling tool calls, and so cannot be taken
    /// before assembly. A front end that keeps no transcript — the plain console —
    /// has nothing to do with it.
    Replay { events: Vec<Event> },
}

/// A gesture, pushed by the front end on its own schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontEndEvent {
    /// The user interrupted the run (Esc / Ctrl-C).
    Cancel,
    /// Shift+Tab: enter plan mode, or leave it if already in it (spec §13).
    TogglePlan,
    /// The user asked to leave.
    Quit,
}

/// The loop's end of the keyboard.
///
/// The request sender and the gesture receiver are **two values**, not one: the
/// loop selects on a prompt and on an unsolicited gesture at the same time, and
/// one struct would be borrowed twice.
pub struct ConsoleHandle {
    requests: mpsc::UnboundedSender<ConsoleRequest>,
}

impl ConsoleHandle {
    /// Read the next user line, or `None` at end of input.
    pub async fn prompt(&self) -> Option<String> {
        let (reply, answer) = oneshot::channel();
        self.requests.send(ConsoleRequest::Prompt { reply }).ok()?;
        answer.await.ok().flatten()
    }

    /// Tell the front end which `/<name>`s exist.
    ///
    /// Fire and forget: a front end that has already gone is not an error, and there
    /// is no answer to wait for. The built-ins lead the list, then the session's
    /// skills, so a menu reads in the order the loop would try them.
    pub fn catalog(&self, entries: Vec<CatalogEntry>) {
        let _ = self.requests.send(ConsoleRequest::Catalog { entries });
    }

    /// Say whether the loop is inside a run.
    ///
    /// Fire and forget, like [`catalog`](Self::catalog): the fact is a notification,
    /// not a question, and a front end that has already gone is not an error. It has to
    /// be *pushed* — the loop is the only one that knows when a run starts and ends,
    /// and no side channel (the render stream, an outstanding prompt) says it for every
    /// kind of run.
    pub fn set_running(&self, running: bool) {
        let _ = self.requests.send(ConsoleRequest::RunState { running });
    }

    /// Hand the front end the history a reopened session assembled with.
    ///
    /// Fire and forget, like [`catalog`](Self::catalog): laying the history out is the
    /// front end's own business, and there is no answer to wait for. The payload is the
    /// **whole** assembled stream in `seq` order; an empty one means there is nothing to
    /// replay and the front end behaves as it always has.
    pub fn replay(&self, events: Vec<Event>) {
        let _ = self.requests.send(ConsoleRequest::Replay { events });
    }
}

/// The gestures the front end pushes on its own schedule.
pub struct ConsoleEvents {
    events: mpsc::UnboundedReceiver<FrontEndEvent>,
}

impl ConsoleEvents {
    /// The next gesture, or `None` once the front end is gone.
    pub async fn recv(&mut self) -> Option<FrontEndEvent> {
        self.events.recv().await
    }
}

/// The front end's end of the keyboard. Only one task should hold it.
pub struct ConsolePort {
    requests: mpsc::UnboundedReceiver<ConsoleRequest>,
    events: mpsc::UnboundedSender<FrontEndEvent>,
}

impl ConsolePort {
    /// Wait for the next thing the loop wants.
    pub async fn recv(&mut self) -> Option<ConsoleRequest> {
        self.requests.recv().await
    }

    /// Push a gesture the loop did not ask for (Esc, Shift+Tab, Ctrl-C).
    pub fn emit(&self, event: FrontEndEvent) {
        let _ = self.events.send(event);
    }
}

/// Create the pair. The handle and the events receiver go to the loop, the port
/// to the front end.
pub fn console() -> (ConsoleHandle, ConsolePort, ConsoleEvents) {
    let (request_tx, request_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    (
        ConsoleHandle {
            requests: request_tx,
        },
        ConsolePort {
            requests: request_rx,
            events: event_tx,
        },
        ConsoleEvents { events: event_rx },
    )
}

/// The permission gate's port, answered through the front end (spec §12).
///
/// The two questions the gate can ask are exactly the two [`Question`]s: the
/// gate's `Ask`, and the plan-mode gesture's "this file already exists".
pub struct ConsoleAsker {
    requests: mpsc::UnboundedSender<ConsoleRequest>,
}

impl ConsoleAsker {
    pub fn new(requests: mpsc::UnboundedSender<ConsoleRequest>) -> Self {
        Self { requests }
    }

    /// Build an asker from a handle, so the CLI wires one keyboard into both the
    /// loop and the permission gate.
    pub fn from_handle(handle: &ConsoleHandle) -> Self {
        Self::new(handle.requests.clone())
    }

    async fn put(&self, question: Question) -> AnswerChoice {
        let (reply, answer) = oneshot::channel();
        if self
            .requests
            .send(ConsoleRequest::Ask(AskRequest { question, reply }))
            .is_err()
        {
            return AnswerChoice::Permission(Answer::Deny);
        }
        answer
            .await
            .unwrap_or(AnswerChoice::Permission(Answer::Deny))
    }
}

#[async_trait]
impl Asker for ConsoleAsker {
    async fn ask(&self, request: &PermissionRequest) -> Answer {
        self.put(Question::Permission(request.clone()))
            .await
            .as_permission()
    }

    async fn ask_plan_conflict(&self, path: &std::path::Path) -> PlanConflict {
        self.put(Question::PlanConflict(path.to_path_buf()))
            .await
            .as_plan()
    }
}

/// The model-question port, answered through the front end (spec §7).
///
/// The mirror of [`ConsoleAsker`] for the model's questions: the loop side asks,
/// the front end answers. The error text is **model-facing** — it becomes the
/// `ask_user_question` call's result — so it is plain English and deliberately
/// does not go through the wording layer.
pub struct ConsoleQuestions {
    requests: mpsc::UnboundedSender<ConsoleRequest>,
}

impl ConsoleQuestions {
    pub fn new(requests: mpsc::UnboundedSender<ConsoleRequest>) -> Self {
        Self { requests }
    }

    /// Build the port from a handle, so the CLI wires one keyboard into both the
    /// loop and the model's questions.
    pub fn from_handle(handle: &ConsoleHandle) -> Self {
        Self::new(handle.requests.clone())
    }
}

#[async_trait]
impl UserQuestions for ConsoleQuestions {
    async fn ask(&self, questions: &[UserQuestion]) -> Result<UserAnswers, String> {
        let (reply, answer) = oneshot::channel();
        if self
            .requests
            .send(ConsoleRequest::Questionnaire(QuestionnaireRequest {
                questions: questions.to_vec(),
                reply,
            }))
            .is_err()
        {
            return Err("no questionnaire answerer is connected".to_owned());
        }
        match answer.await {
            Ok(result) => result,
            // The front end went away without answering — the run was cancelled,
            // or the input ended. Never hang waiting for a person who is gone.
            Err(_) => Err("the questionnaire was left unanswered".to_owned()),
        }
    }
}

/// A source of one input line, with `None` for end of input.
///
/// This is the plain front end's one input primitive. It is a value rather than a
/// direct `stdin` read so the whole line-oriented console — the prompt, the
/// permission questions, the questionnaire — can be driven from a script in a
/// test, the same way the TUI's keyboard is driven one event at a time.
pub type LineReader = Box<
    dyn FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send>>
        + Send,
>;

/// Run the line-oriented front end for plain mode.
///
/// stdin is line-buffered, so there is no raw mode and no key events: the port
/// reads one line per request, whether the loop wanted a prompt or the gate wants
/// an answer. Prompts go to stderr, never stdout — the final product is the only
/// thing stdout carries (spec §19).
pub fn spawn_plain_console(port: ConsolePort) -> tokio::task::JoinHandle<()> {
    spawn_plain_console_with(port, Box::new(|| Box::pin(read_stdin_line())))
}

/// [`spawn_plain_console`] with an injected line reader.
///
/// The reader is the whole input surface of this front end, so injecting it is what
/// makes the line-by-line path — including the model's questionnaire — testable
/// without a pipe.
pub fn spawn_plain_console_with(
    mut port: ConsolePort,
    mut reader: LineReader,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(request) = port.recv().await {
            match request {
                ConsoleRequest::Prompt { reply } => {
                    let _ = reply.send(next_line(&mut reader, "> ").await);
                }
                ConsoleRequest::Ask(ask) => {
                    let answer = answer_question(&mut reader, &ask.question).await;
                    let _ = ask.reply.send(answer);
                }
                // The model's questionnaire, answered one line per question. On
                // end of input it returns an error rather than looping for ever
                // (spec §19).
                ConsoleRequest::Questionnaire(request) => {
                    let answers = answer_questionnaire(&mut reader, &request.questions).await;
                    let _ = request.reply.send(answers);
                }
                // There is no menu on the line-oriented front end: the names are
                // discoverable through the unknown-command text instead.
                ConsoleRequest::Catalog { .. } => {}
                // Nothing on this front end reads key events, so there is no gesture to
                // turn into the wrong branch (spec §6).
                ConsoleRequest::RunState { .. } => {}
                // There is no transcript to lay history into: the line-oriented front
                // end prints each event as it arrives and keeps nothing
                // (`.scratch/tui-history-replay/spec.md` §1).
                ConsoleRequest::Replay { .. } => {}
            }
        }
    })
}

/// Write `prompt` to stderr and pull the next line. `None` at end of input.
async fn next_line(reader: &mut LineReader, prompt: &str) -> Option<String> {
    use std::io::Write;
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    reader().await
}

/// Read one line from stdin. `None` at EOF.
async fn read_stdin_line() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_owned()),
            Err(_) => None,
        }
    })
    .await
    .ok()
    .flatten()
}

/// Put one question on the terminal and read the answer.
///
/// Anything that is not an explicit yes is read the non-acting way: an answer
/// typed by accident must not approve a write.
async fn answer_question(reader: &mut LineReader, question: &Question) -> AnswerChoice {
    match question {
        Question::Permission(request) => {
            let prompt = crate::render::wording::permission_prompt_with_context(
                &request.tool_name,
                &crate::render::transcript::summarize_args(&request.args),
                &request.reason,
            );
            match next_line(reader, &prompt).await.as_deref() {
                Some("y") | Some("yes") => AnswerChoice::Permission(Answer::Allow),
                Some("a") | Some("always") => AnswerChoice::Permission(Answer::AlwaysAllow),
                _ => AnswerChoice::Permission(Answer::Deny),
            }
        }
        Question::PlanConflict(path) => {
            let prompt = crate::render::wording::plan_conflict_prompt(&path.display().to_string());
            match next_line(reader, &prompt).await.as_deref() {
                Some("o") | Some("overwrite") => AnswerChoice::Plan(PlanConflict::Overwrite),
                Some("a") | Some("append") => AnswerChoice::Plan(PlanConflict::Append),
                _ => AnswerChoice::Plan(PlanConflict::Keep),
            }
        }
    }
}

/// Put the model's questionnaire on the terminal, one question per line, and
/// encode what was read (spec §7, §19).
///
/// There is no paging here: every question is printed and read in turn, so the
/// "one screen at a time" of the TUI becomes "one prompt at a time". An empty line
/// is a skip; a number picks an option; anything else is custom text. A
/// multi-select question with options reads a second, optional line so that
/// `selected` and `custom` can both be answered — the supplement spec §7
/// requires. End of input is not an answer, so it fails the call instead of
/// waiting.
async fn answer_questionnaire(
    reader: &mut LineReader,
    questions: &[UserQuestion],
) -> Result<UserAnswers, String> {
    let mut answers = Vec::with_capacity(questions.len());
    for (index, question) in questions.iter().enumerate() {
        print_questionnaire_question(index, questions.len(), question);
        let Some(answer) = read_plain_answer(reader, question).await else {
            return Err("input ended before the questionnaire was answered".to_owned());
        };
        answers.push(answer);
    }
    Ok(UserAnswers { answers })
}

/// Read one question's answer, or `None` at end of input.
///
/// A multi-select question that offers options takes **two** lines: the numbers
/// (or custom text) and then an optional supplement. That second line is the only
/// way a line-oriented front end can say "this choice, plus this text" (spec §7);
/// every other question takes one line, which keeps the common case short.
async fn read_plain_answer(reader: &mut LineReader, question: &UserQuestion) -> Option<UserAnswer> {
    if question.multi_select && !question.options.is_empty() {
        let selection = next_line(
            reader,
            crate::render::wording::questionnaire_plain_options_prompt(true),
        )
        .await?;
        let supplement = next_line(
            reader,
            crate::render::wording::questionnaire_plain_supplement_prompt(),
        )
        .await?;
        return Some(encode_plain_answer(question, &selection, Some(&supplement)));
    }
    let prompt = if question.options.is_empty() {
        crate::render::wording::questionnaire_plain_answer_prompt()
    } else {
        crate::render::wording::questionnaire_plain_options_prompt(false)
    };
    let line = next_line(reader, prompt).await?;
    Some(encode_plain_answer(question, &line, None))
}

/// Print one question and its numbered options to stderr.
fn print_questionnaire_question(index: usize, total: usize, question: &UserQuestion) {
    if let Some(header) = question
        .header
        .as_deref()
        .map(str::trim)
        .filter(|header| !header.is_empty())
    {
        eprintln!("{header}");
    }
    eprintln!(
        "[{}] {}",
        crate::render::wording::questionnaire_progress(index, total),
        question.question.trim()
    );
    for (at, choice) in question.options.iter().enumerate() {
        eprintln!(
            "  {}",
            crate::render::wording::questionnaire_option(
                at + 1,
                &choice.label,
                choice.description.as_deref(),
            )
        );
    }
}

/// Encode one plain-console answer.
///
/// A blank first line is a skip (`selected: []`, no `custom`). A line that is a
/// list of valid option numbers picks those options; anything else is custom
/// text, which on a single-select question means the choice is empty because
/// custom text overrides it (spec §7). `supplement` is the multi-select question's
/// second line: when present it is added to the custom text, so `selected` and
/// `custom` travel together.
fn encode_plain_answer(
    question: &UserQuestion,
    line: &str,
    supplement: Option<&str>,
) -> UserAnswer {
    let line = line.trim();
    let supplement = supplement
        .map(str::trim)
        .filter(|supplement| !supplement.is_empty());

    let mut selected = Vec::new();
    let mut custom: Option<String> = None;
    if question.options.is_empty() {
        if !line.is_empty() {
            custom = Some(line.to_owned());
        }
    } else if let Some(chosen) = chosen_options(question, line) {
        selected = chosen;
    } else if !line.is_empty() {
        custom = Some(line.to_owned());
    }
    if let Some(supplement) = supplement {
        custom = Some(match custom {
            Some(first) => format!("{first} {supplement}"),
            None => supplement.to_owned(),
        });
    }
    UserAnswer {
        id: question.id.clone(),
        selected,
        custom,
    }
}

/// The labels a line of option numbers names, or `None` when the line is not a
/// list of valid numbers (which makes it custom text).
///
/// A single-select question takes exactly one number; a multi-select one takes a
/// comma-separated list.
fn chosen_options(question: &UserQuestion, line: &str) -> Option<Vec<String>> {
    let mut selected = Vec::new();
    let tokens: Vec<&str> = if question.multi_select {
        line.split(',').collect()
    } else {
        vec![line]
    };
    for token in tokens {
        let number: usize = token.trim().parse().ok()?;
        if number == 0 || number > question.options.len() {
            return None;
        }
        let label = question.options[number - 1].label.clone();
        if !selected.contains(&label) {
            selected.push(label);
        }
    }
    (!selected.is_empty()).then_some(selected)
}
