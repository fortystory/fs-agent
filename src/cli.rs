//! CLI boundary: the only place that reads the environment.
//!
//! The binary is a thin shell around this module. Everything the library needs
//! is assembled here from `config.toml`, the exported environment and the
//! built-in defaults, then injected into [`crate::assemble`].
//!
//! `probe` is the ticket-02 manual acceptance tool: it drives one real turn
//! against each configured model, then a second turn in the same session, so
//! the usage line for the second request shows whether prefix caching hit. It
//! runs headless with throwaway session logs.
//!
//! `prune` and `sessions` both read the store ticket 12 built: `prune` removes a
//! workspace's session directories by hand, and `sessions` answers questions
//! about a **finished** session from its own stream — `ls`, the round-grouped
//! `show` (with `--files` as the workspace-object view), `replay` (the projection
//! recomputation, spec §18) and `stats`. They live here because the store's root
//! comes from the environment, which the library never reads; the queries
//! themselves are [`crate::session::observe`] and [`crate::agent::replay`].
//! The interactive renderers are still the one unwired piece (ticket 18).

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use ratatui::buffer::CellWidth;

use crate::agent::replay;
use crate::config::{self, Config, Debater, DiscussionRoster, EnvMap};
use crate::events::{
    read_events, total_usage, Event, EventPayload, SessionId, SpeakerId, StopReason, Usage,
};
use crate::permissions::{Mode, Policy};
use crate::provider::capability::caps_for;
use crate::provider::openai::{stderr_warnings, BuildError, OpenAiProvider};
use crate::provider::Message;
use crate::questions::UserQuestions;
use crate::render::{
    self, ConsoleAsker, ConsoleEvents, ConsoleHandle, ConsoleQuestions, FrontEndEvent,
    PlainOptions, RenderSinks, Renderer, SessionFacts, TuiOptions,
};
use crate::session::observe::{self, CostModel, Entry, Filter, Listing, Timeline};
use crate::session::{SessionStore, StoredSession};
use crate::tools::{self, PathLocks};
use crate::{
    assemble, assemble_discussion, AssemblyParts, DebaterParts, DiscussionHarness, DiscussionParts,
    Harness, SessionScaffold, SynthesizerParts,
};

/// Prompt for the second probe turn; keeps the transcript growing so the first
/// turn's prefix is what the cache has to match.
const PROBE_FOLLOW_UP: &str = "Reply with exactly: done";

/// How long to wait between the two probe turns.
///
/// DeepSeek builds its prefix cache on disk over "seconds"; asking again
/// immediately measures a cold cache and reports a false negative.
const CACHE_WARMUP: std::time::Duration = std::time::Duration::from_secs(10);

/// The first probe turn. Padded well past both vendors' cache floors (Kimi only
/// caches prompts above 256 tokens), so a second request can show a hit at all.
fn probe_prompt() -> String {
    const FILLER: &str =
        "The quick brown fox jumps over the lazy dog while the prefix cache warms up. ";
    let mut prompt = String::from(
        "Ignore the filler below; it only pads the prompt so prefix caching engages.\n",
    );
    while prompt.len() < 2_000 {
        prompt.push_str(FILLER);
    }
    prompt.push_str("\nReply with exactly: ok");
    prompt
}

/// Why this process refuses to start as root, if it is root (spec §20).
///
/// A pure function of the effective uid, on purpose: the refusal has nothing to
/// read but the uid, so there is no argument, environment variable or mode that
/// can turn it off. The guardrails in this project assume the worst case stays
/// inside the workspace; as root a single misjudgement is system-wide, which is
/// a different risk class than the one being managed.
pub fn root_refusal(euid: u32) -> Option<String> {
    (euid == 0).then(|| render::wording::root_refusal().to_owned())
}

/// Parse `argv` from the environment and run. This is the binary entry point.
pub fn main() -> ExitCode {
    // Checked before anything else — before arguments, before the runtime — so
    // there is no path into the program as root and no flag that skips the
    // check (spec §20).
    // SAFETY: `geteuid` only reads the calling process's uid and cannot fail.
    let euid = unsafe { libc::geteuid() };
    if let Some(message) = root_refusal(euid) {
        eprintln!("fs-agent: {message}");
        return ExitCode::FAILURE;
    }
    let env: EnvMap = std::env::vars().collect();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!(
                "fs-agent: {}",
                render::wording::startup_runtime(&error.to_string())
            );
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run(&args, &env))
}

async fn run(args: &[String], env: &EnvMap) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("--version") | Some("-V") => {
            println!("fs-agent {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help") | Some("-h") => {
            println!("{}", render::wording::help_main());
            ExitCode::SUCCESS
        }
        Some("probe") => probe(&args[1..], env).await,
        Some("discuss") => discuss(&args[1..], env).await,
        Some("prune") => prune(&args[1..], env),
        Some("sessions") => {
            let stdout = std::io::stdout();
            let stderr = std::io::stderr();
            let mut out = stdout.lock();
            let mut err = stderr.lock();
            run_sessions(&args[1..], env, &mut out, &mut err)
        }
        // No subcommand (or a bare flag) is the interactive session: the common
        // case is just running `fs-agent` in a workspace. `interactive` parses
        // its own arguments and rejects anything it does not know.
        _ => interactive(args, env).await,
    }
}

// ---------------------------------------------------------------------------
// The interactive front end (spec §19)
// ---------------------------------------------------------------------------

/// One parsed interactive invocation. The renderer is chosen here and injected
/// into the assembly, so exactly one mode runs.
#[derive(Debug, Default)]
struct InteractiveArgs {
    /// Force the plain renderer.
    plain: bool,
    /// Force the TUI renderer.
    tui: bool,
    /// Resume this workspace's newest session (spec §11).
    resume: bool,
    config: Option<PathBuf>,
    model: Option<String>,
    cwd: Option<PathBuf>,
}

fn parse_interactive(args: &[String]) -> Result<InteractiveArgs, String> {
    let mut parsed = InteractiveArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--plain" => parsed.plain = true,
            "--tui" => parsed.tui = true,
            "--continue" | "-c" => parsed.resume = true,
            flag @ ("--config" | "--model" | "--cwd") => {
                let flag = flag.to_owned();
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_value(&flag))?;
                match flag.as_str() {
                    "--config" => parsed.config = Some(PathBuf::from(value)),
                    "--model" => parsed.model = Some(value.clone()),
                    "--cwd" => parsed.cwd = Some(PathBuf::from(value)),
                    _ => unreachable!(),
                }
            }
            other => return Err(render::wording::unknown_argument(other)),
        }
        index += 1;
    }
    if parsed.plain && parsed.tui {
        return Err(render::wording::renderers_mutually_exclusive().to_owned());
    }
    Ok(parsed)
}

/// The interactive session: one workspace, one renderer, one keyboard.
///
/// The renderer is selected before assembly and injected, and the same console
/// port serves both the loop's prompts and the permission gate's questions — the
/// two things that come from the same keyboard (spec §19).
async fn interactive(args: &[String], env: &EnvMap) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", render::wording::help_interactive());
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_interactive(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("fs-agent: {message}");
            println!("{}", render::wording::help_interactive());
            return ExitCode::FAILURE;
        }
    };
    let config = match load_config(parsed.config.clone(), env) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("fs-agent: {}", render::wording::startup_config(&message));
            return ExitCode::FAILURE;
        }
    };
    // An unregistered model is a startup error, never a silent downgrade.
    if let Err(message) = validate_models(&config) {
        eprintln!("fs-agent: {message}");
        return ExitCode::FAILURE;
    }
    let model = parsed
        .model
        .clone()
        .unwrap_or_else(|| config.default_model.clone());
    let profile = match config.resolve_model(Some(&model)) {
        Ok((_, profile)) => profile.clone(),
        Err(error) => {
            eprintln!("fs-agent: {error}");
            return ExitCode::FAILURE;
        }
    };
    let Some(root) = config::sessions_dir(env) else {
        eprintln!("fs-agent: {}", render::wording::startup_no_session_store());
        return ExitCode::FAILURE;
    };
    let store = SessionStore::new(root);
    let cwd = match parsed.cwd.clone() {
        Some(cwd) => cwd,
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_cwd(&error.to_string())
                );
                return ExitCode::FAILURE;
            }
        },
    };
    let stored = if parsed.resume {
        match store.latest(&cwd) {
            Ok(Some(session)) => session,
            Ok(None) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_no_session_to_continue(&cwd.display().to_string())
                );
                return ExitCode::FAILURE;
            }
            Err(error) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_store_read(&error.to_string())
                );
                return ExitCode::FAILURE;
            }
        }
    } else {
        match store.create(&cwd) {
            Ok(session) => session,
            Err(error) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_store_create(&error.to_string())
                );
                return ExitCode::FAILURE;
            }
        }
    };

    let provider = match OpenAiProvider::build(&config, &model, stderr_warnings()) {
        Ok(provider) => provider,
        Err(error) => {
            eprintln!("fs-agent: {error}");
            return ExitCode::FAILURE;
        }
    };
    let session_config = match config.session_config(&model) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("fs-agent: {error}");
            return ExitCode::FAILURE;
        }
    };
    let home = env
        .get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);

    // The keyboard's two ends: the loop's handle and gesture receiver, and the
    // port the selected renderer (or the plain line reader) serves it through.
    let (console, port, mut events) = render::console();
    let use_tui = parsed.tui || (!parsed.plain && std::io::stdout().is_terminal());
    let renderer = if use_tui {
        // The header and the panel display these; none of them rides the event
        // stream, and the one value that does change at runtime — the mode — is
        // deliberately absent (the stream carries both of its transitions).
        // The window is the model's input budget. The provider above already
        // resolved this same table, so the failure below is belt-and-braces: it
        // keeps an unregistered model a startup error here too, rather than a
        // panic where the facts are built.
        let caps = match caps_for(&model) {
            Ok(caps) => caps,
            Err(error) => {
                eprintln!("fs-agent: {error}");
                return ExitCode::FAILURE;
            }
        };
        let facts = SessionFacts {
            session_id: stored.id.as_str().to_owned(),
            cwd: stored.dir.display().to_string(),
            model: model.clone(),
            context_window: crate::context::usable_input(&caps),
            budget_limit: session_config.budget.limit,
        };
        Renderer::tui(TuiOptions { port, facts })
    } else {
        // The plain front end reads stdin; it is line-buffered, so there is no
        // raw mode and no key events.
        render::spawn_plain_console(port);
        Renderer::plain(PlainOptions {
            sinks: RenderSinks {
                stdout_result: Box::new(std::io::stdout()),
                stderr_diagnostic: Box::new(std::io::stderr()),
            },
            color: std::io::stderr().is_terminal() && env.get("NO_COLOR").is_none(),
        })
    };
    let asker = Arc::new(ConsoleAsker::from_handle(&console));
    // The model's questions travel the same keyboard on their own port (spec §7).
    // Interactive assembly always has one — TUI or plain — so the tool table offers
    // `ask_user_question`, and the table is built from the port's presence rather
    // than from a second, drift-prone flag.
    let questions: Option<Arc<dyn UserQuestions>> =
        Some(Arc::new(ConsoleQuestions::from_handle(&console)));

    let mut harness = match assemble(AssemblyParts {
        scaffold: SessionScaffold {
            cwd: cwd.clone(),
            log_path: stored.log_path.clone(),
            session_id: stored.id.clone(),
            // The tool table is fixed here, at assembly: the built-ins plus
            // every dynamically declared tool (spec §14).
            tools: tools::with_dynamic(&config.tools, questions.is_some()),
            locks: PathLocks::new(),
            // Interactive sessions start in `ask`: writes ask, reads are allowed
            // (spec §12). A headless caller gets no answerer and downgrades.
            policy: Policy::for_mode(Mode::Ask),
            asker: Some(asker),
            questions,
            hook: None,
            home,
        },
        provider: Box::new(provider),
        speaker: SpeakerId::Debater(profile.name.clone().into()),
        config: session_config,
        renderer,
    })
    .await
    {
        Ok(harness) => harness,
        Err(error) => {
            eprintln!("fs-agent: {error}");
            return ExitCode::FAILURE;
        }
    };

    // User story A.12: say which model, mode and session this is, before the
    // first question. It goes through the renderer rather than to stderr: the
    // TUI started when the harness was assembled, and a second writer to the
    // terminal lands inside its live region, on top of the status line.
    harness.notice(&render::wording::banner(
        harness.session_id().as_str(),
        &model,
        harness.mode(),
        &stored.dir.display().to_string(),
        parsed.resume,
    ));

    // The `/` menu's names. The loop is what acts on a submission, so the loop is
    // what says which names exist: the built-ins it parses, then the skills this
    // session discovered — which is why this is sent here, after assembly and
    // before the first prompt, and not injected with the header's facts.
    console.catalog(
        render::wording::BUILT_IN_COMMANDS
            .iter()
            .map(|command| render::CatalogEntry::new(command.name, command.description))
            .chain(
                harness
                    .skill_catalog()
                    .into_iter()
                    .map(|(name, description)| render::CatalogEntry::new(name, description)),
            )
            .collect(),
    );

    let code = interactive_loop(&mut harness, &console, &mut events, &config).await;
    harness.shutdown().await;
    code
}

// ---------------------------------------------------------------------------
// The discussion front end (spec §15)
// ---------------------------------------------------------------------------

/// One parsed `fs-agent discuss` invocation.
///
/// There is deliberately no `--model`: who debates is a configuration fact
/// (`[discussion] debaters`), and a flag that could replace one of the two would be a
/// second way to say it. What is left to choose is how the discussion is watched.
#[derive(Debug, Default)]
struct DiscussArgs {
    /// The question, in the words it was given with. Empty means "ask stdin".
    words: Vec<String>,
    /// `--debaters a,b`: which two of the pool debate. `None` draws a pair.
    debaters: Option<[String; 2]>,
    plain: bool,
    tui: bool,
    config: Option<PathBuf>,
    cwd: Option<PathBuf>,
}

fn parse_discuss(args: &[String]) -> Result<DiscussArgs, String> {
    let mut parsed = DiscussArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--plain" => parsed.plain = true,
            "--tui" => parsed.tui = true,
            flag @ ("--config" | "--cwd" | "--debaters") => {
                let flag = flag.to_owned();
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_value(&flag))?;
                match flag.as_str() {
                    "--config" => parsed.config = Some(PathBuf::from(value)),
                    "--cwd" => parsed.cwd = Some(PathBuf::from(value)),
                    "--debaters" => parsed.debaters = Some(split_names(value)?),
                    _ => unreachable!(),
                }
            }
            // Everything after `--` is the question, so a question may start with a
            // dash.
            "--" => {
                parsed.words.extend(args[index + 1..].iter().cloned());
                break;
            }
            other if other.starts_with("--") => {
                return Err(render::wording::unknown_argument(other))
            }
            other => parsed.words.push(other.to_owned()),
        }
        index += 1;
    }
    if parsed.plain && parsed.tui {
        return Err(render::wording::renderers_mutually_exclusive().to_owned());
    }
    Ok(parsed)
}

/// One question, two debaters, one synthesizer, one session.
///
/// The renderer is chosen exactly as the interactive path chooses it, so a discussion
/// watched in a terminal gets the TUI — the permission modal included, for whatever a
/// debater wants to run — and a piped one gets the plain transcript with the
/// synthesizer's product alone on stdout.
async fn discuss(args: &[String], env: &EnvMap) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", render::wording::help_discuss());
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_discuss(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("fs-agent: {message}");
            println!("{}", render::wording::help_discuss());
            return ExitCode::FAILURE;
        }
    };
    let config = match load_config(parsed.config.clone(), env) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("fs-agent: {}", render::wording::startup_config(&message));
            return ExitCode::FAILURE;
        }
    };
    // An unregistered model is a startup error, never a silent downgrade.
    if let Err(message) = validate_models(&config) {
        eprintln!("fs-agent: {message}");
        return ExitCode::FAILURE;
    }
    // No roster is not an error in the file — most configurations are for
    // single-agent sessions — but it is one for this subcommand.
    let Some(roster) = config.discussion.clone() else {
        eprintln!("fs-agent: {}", render::wording::discussion_no_roster());
        return ExitCode::FAILURE;
    };
    // Which two of the pool debate: the ones named on the command line, or a draw.
    let pair = match pick_debaters(&roster, parsed.debaters.clone(), discussion_seed()) {
        Ok(pair) => pair,
        Err(message) => {
            eprintln!("fs-agent: {message}");
            return ExitCode::FAILURE;
        }
    };
    for line in advisory_lines(&config, &pair) {
        eprintln!("fs-agent: {line}");
    }

    // The question comes before anything is assembled: reading it may block on a
    // terminal, and the terminal has to still be in cooked mode for that (assembly
    // puts the TUI into raw mode and owns the screen from then on).
    let Some(question) = question(&parsed.words) else {
        eprintln!("fs-agent: {}", render::wording::discuss_needs_question());
        return ExitCode::FAILURE;
    };

    let Some(root) = config::sessions_dir(env) else {
        eprintln!("fs-agent: {}", render::wording::startup_no_session_store());
        return ExitCode::FAILURE;
    };
    let cwd = match parsed.cwd.clone() {
        Some(cwd) => cwd,
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_cwd(&error.to_string())
                );
                return ExitCode::FAILURE;
            }
        },
    };
    let stored = match SessionStore::new(root).create(&cwd) {
        Ok(session) => session,
        Err(error) => {
            eprintln!(
                "fs-agent: {}",
                render::wording::startup_store_create(&error.to_string())
            );
            return ExitCode::FAILURE;
        }
    };
    let home = env
        .get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);

    // Every participant's provider is built before the renderer starts, so a missing
    // key is a startup error on a normal screen rather than a half-drawn interface.
    let (debaters, synthesizer) = match discussion_participants(&config, &pair) {
        Ok(parts) => parts,
        Err(message) => {
            eprintln!("fs-agent: {message}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "fs-agent: {}",
        render::wording::discussion_starting(
            &render::wording::debater_label(&pair[0].name, &pair[0].model),
            &render::wording::debater_label(&pair[1].name, &pair[1].model),
            &question,
        )
    );

    let (console, port, mut events) = render::console();
    let use_tui = parsed.tui || (!parsed.plain && std::io::stdout().is_terminal());
    let renderer = if use_tui {
        let caps = match caps_for(&pair[0].model) {
            Ok(caps) => caps,
            Err(error) => {
                eprintln!("fs-agent: {error}");
                return ExitCode::FAILURE;
            }
        };
        let facts = SessionFacts {
            session_id: stored.id.as_str().to_owned(),
            cwd: stored.dir.display().to_string(),
            // The panel has one model row and one context row, and a discussion has a
            // pair of debaters and no single window: the row names **both models**, and
            // the window is the first debater's, which is the approximation the panel
            // cannot help but be (spec §8). The names are the transcript's business
            // (`debater_label`); this row is about models, so it shows models.
            model: render::wording::discussion_pair(&pair[0].model, &pair[1].model),
            context_window: crate::context::usable_input(&caps),
            // `session_config` copies `[budget]` verbatim, so the file's value is the
            // session's.
            budget_limit: config.budget.limit,
        };
        Renderer::tui(TuiOptions { port, facts })
    } else {
        render::spawn_plain_console(port);
        Renderer::plain(PlainOptions {
            sinks: RenderSinks {
                stdout_result: Box::new(std::io::stdout()),
                stderr_diagnostic: Box::new(std::io::stderr()),
            },
            color: std::io::stderr().is_terminal() && env.get("NO_COLOR").is_none(),
        })
    };
    // This front end never reads a line — one question in, one discussion out — so it is
    // inside a run for its whole life, and says so before the loop would ever ask. Told
    // here rather than inferred, for the reason `ConsoleRequest::RunState` records.
    console.set_running(true);
    // The same keyboard answers the debaters' permission questions: a discussion is
    // still a session with tools in it.
    let asker = Arc::new(ConsoleAsker::from_handle(&console));
    // A debater is a main session, not an executor, so it may ask the user too
    // (spec §7); the port is the same keyboard the permission gate uses.
    let questions: Option<Arc<dyn UserQuestions>> =
        Some(Arc::new(ConsoleQuestions::from_handle(&console)));

    let mut harness = match assemble_discussion(DiscussionParts {
        scaffold: SessionScaffold {
            cwd,
            log_path: stored.log_path.clone(),
            session_id: stored.id.clone(),
            tools: tools::with_dynamic(&config.tools, questions.is_some()),
            locks: PathLocks::new(),
            policy: Policy::for_mode(Mode::Ask),
            asker: Some(asker),
            questions,
            hook: None,
            home,
        },
        debaters,
        synthesizer,
        max_rounds: roster.max_rounds,
        renderer,
    })
    .await
    {
        Ok(harness) => harness,
        Err(error) => {
            eprintln!("fs-agent: {error}");
            return ExitCode::FAILURE;
        }
    };

    let outcome = run_discussion(&mut harness, &mut events, &question).await;
    harness.shutdown().await;

    // The line is printed after the alt screen is restored, because the TUI's
    // transcript does not survive the process: whatever the user missed on screen, the
    // session id is the durable way back to it.
    match outcome {
        Ok(outcome) => {
            eprintln!(
                "fs-agent: {}",
                render::wording::discussion_ended(outcome.reason, outcome.rounds, &outcome.absent)
            );
            eprintln!(
                "fs-agent: {}",
                render::wording::discussion_replay(stored.id.as_str())
            );
            // A discussion that failed outright is a failure; a cancelled one is what
            // the user asked for, the way a cancelled turn is in an interactive
            // session (spec §6).
            if outcome.reason == StopReason::Error {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("fs-agent: {}", render::wording::error_report(&error));
            ExitCode::FAILURE
        }
    }
}

/// `/discuss [--debaters a,b] [问题]`: the command's own flags, and the question.
///
/// The flags only count at the front — the moment something else appears, the rest of
/// the line (newlines and all) is the question, so a question may start with a dash and
/// may be a paragraph.
#[derive(Debug)]
struct DiscussLine {
    debaters: Option<[String; 2]>,
    question: String,
}

fn parse_discuss_line(text: &str) -> Result<DiscussLine, String> {
    let mut rest = text.trim_start();
    let mut debaters = None;
    loop {
        let Some((word, tail)) = rest.split_once(char::is_whitespace) else {
            return Ok(DiscussLine {
                debaters,
                question: rest.to_owned(),
            });
        };
        match word {
            "--debaters" | "--debater" => {
                let tail = tail.trim_start();
                match tail.split_once(char::is_whitespace) {
                    Some((value, after)) => {
                        debaters = Some(split_names(value)?);
                        rest = after;
                    }
                    None => {
                        return Ok(DiscussLine {
                            debaters: Some(split_names(tail)?),
                            question: String::new(),
                        })
                    }
                }
            }
            "--" => {
                return Ok(DiscussLine {
                    debaters,
                    question: tail.trim_start().to_owned(),
                })
            }
            other if other.starts_with("--") => {
                return Err(render::wording::unknown_argument(other))
            }
            _ => {
                return Ok(DiscussLine {
                    debaters,
                    question: rest.to_owned(),
                })
            }
        }
    }
}

/// `a,b` as two debater names.
fn split_names(value: &str) -> Result<[String; 2], String> {
    let names: Vec<String> = value
        .split(',')
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect();
    <[String; 2]>::try_from(names).map_err(|_| render::wording::needs_two_debaters(value))
}

/// The two debaters a discussion runs with: the ones asked for by name, or a draw from
/// the pool.
///
/// A draw is the default because a pool exists to vary the pair; `seed` is injected (the
/// clock at run time, a constant in tests) so the choice is reproducible.
fn pick_debaters(
    roster: &DiscussionRoster,
    requested: Option<[String; 2]>,
    seed: u64,
) -> Result<[Debater; 2], String> {
    let lookup = |name: &str| {
        roster
            .debater(name)
            .cloned()
            .ok_or_else(|| render::wording::unknown_debater(name, &roster.names()))
    };
    match requested {
        Some([first, second]) => Ok([lookup(&first)?, lookup(&second)?]),
        None => {
            let (first, second) = crate::discussion::pick_pair(roster.debaters.len(), seed)
                .ok_or_else(|| render::wording::discussion_no_roster().to_owned())?;
            Ok([
                roster.debaters[first].clone(),
                roster.debaters[second].clone(),
            ])
        }
    }
}

/// What to say about the pair *before* it debates: that a pool member sharing a vendor
/// means two samples rather than two judgements.
///
/// Allowed — one subscription is not a reason to have no discussion at all — and said
/// out loud rather than passing for the design's case (spec §15).
fn advisory_lines(config: &Config, pair: &[Debater; 2]) -> Vec<String> {
    let [first, second] = pair;
    if first.model == second.model {
        return vec![render::wording::discussion_same_model(
            &render::wording::debater_label(&first.name, &first.model),
        )];
    }
    if config.debaters_share_a_vendor(&first.model, &second.model) {
        return vec![render::wording::discussion_one_vendor(
            &render::wording::debater_label(&first.name, &first.model),
            &render::wording::debater_label(&second.name, &second.model),
        )];
    }
    Vec::new()
}

/// A seed for drawing a pair: the clock, mixed the way a session id's suffix is.
///
/// Not a PRNG — this decides which two of a handful of debaters argue — but it has to
/// differ between two discussions started in the same second, which the nanos do.
fn discussion_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};

    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    hasher.write_u128(nanos);
    hasher.finish()
}

/// One provider and one set of session values per participant, from the roster.
///
/// The two debaters answer with the roster's models; the synthesizer answers with the
/// routing table's landing point — with nothing routed, the first debater's model,
/// which is what `SessionConfig::model_for(Synthesizer)` resolves to inside the
/// assembly. Building its provider on exactly that model is what keeps the provider and
/// the config that is sent to it in agreement.
///
/// Used by both ways in: the `discuss` subcommand (a discussion of its own) and
/// `/discuss` (a discussion on the session the user is in).
fn discussion_participants(
    config: &Config,
    pair: &[Debater; 2],
) -> Result<(Vec<DebaterParts>, SynthesizerParts), String> {
    let build = |model: &str| -> Result<(config::SessionConfig, Box<dyn crate::provider::Provider>), String> {
        let values = config
            .session_config(model)
            .map_err(|error| error.to_string())?;
        let provider = OpenAiProvider::build(config, model, stderr_warnings())
            .map_err(|error| error.to_string())?;
        Ok((values, Box::new(provider)))
    };
    let [first, second] = pair;
    let (first_config, first_provider) = build(&first.model)?;
    let (second_config, second_provider) = build(&second.model)?;
    let synthesizer_model = config
        .routing
        .synthesizer_model
        .clone()
        .unwrap_or_else(|| first.model.clone());
    let (synthesizer_config, synthesizer_provider) = build(&synthesizer_model)?;
    Ok((
        vec![
            DebaterParts {
                // The debater's **name** is its identity on the stream — not the model,
                // which a pool can hand to two debaters at once (spec §5, §15).
                speaker: SpeakerId::Debater(first.name.as_str().into()),
                config: first_config,
                provider: first_provider,
                soul: first.soul.clone(),
            },
            DebaterParts {
                speaker: SpeakerId::Debater(second.name.as_str().into()),
                config: second_config,
                provider: second_provider,
                soul: second.soul.clone(),
            },
        ],
        SynthesizerParts {
            config: synthesizer_config,
            provider: synthesizer_provider,
        },
    ))
}

/// `/discuss [问题]`: run a discussion **on this session's stream** (spec §15).
///
/// The loop owns the keyboard and the session owns the log, so this reads the roster
/// out of the configuration, builds one provider per participant, and hands the
/// protocol the session the user is already in: the debaters are its siblings, so they
/// inherit its context and their rounds land in its stream. Everything the user needs
/// to know — which models, which question, and how it ended — goes through the
/// renderer rather than stderr, because the TUI owns the screen.
async fn discuss_in_session(
    harness: &mut Harness,
    events: &mut ConsoleEvents,
    config: &Config,
    asked: String,
) -> Result<(), crate::Error> {
    let Some(roster) = config.discussion.clone() else {
        harness.notice(&format!(
            "fs-agent: {}",
            render::wording::discussion_no_roster()
        ));
        return Ok(());
    };
    // `--debaters a,b` first, then the question: the flags belong to the command, and
    // what is left of the line is what the user is asking about.
    let line = match parse_discuss_line(&asked) {
        Ok(line) => line,
        Err(message) => {
            harness.notice(&format!("fs-agent: {message}"));
            return Ok(());
        }
    };
    // A bare `/discuss` puts the thing this session was just asked to two models: "what
    // we were talking about" is the question worth a second judgement.
    let question = if line.question.trim().is_empty() {
        match harness.last_question() {
            Some(last) => last,
            None => {
                harness.notice(&format!(
                    "fs-agent: {}",
                    render::wording::discuss_needs_in_session_question()
                ));
                return Ok(());
            }
        }
    } else {
        line.question
    };
    let pair = match pick_debaters(&roster, line.debaters, discussion_seed()) {
        Ok(pair) => pair,
        Err(message) => {
            harness.notice(&format!("fs-agent: {message}"));
            return Ok(());
        }
    };
    let (debaters, synthesizer) = match discussion_participants(config, &pair) {
        Ok(parts) => parts,
        Err(message) => {
            harness.notice(&format!("fs-agent: {message}"));
            return Ok(());
        }
    };
    for line in advisory_lines(config, &pair) {
        harness.notice(&format!("fs-agent: {line}"));
    }
    harness.notice(&format!(
        "fs-agent: {}",
        render::wording::discussion_starting(
            &render::wording::debater_label(&pair[0].name, &pair[0].model),
            &render::wording::debater_label(&pair[1].name, &pair[1].model),
            &question,
        )
    ));

    let signal = harness.cancel_signal();
    // The future borrows the harness for as long as it runs, so it lives in its own
    // scope: the notice below needs the harness back.
    let outcome = {
        let mut run =
            Box::pin(harness.discuss(&question, debaters, synthesizer, roster.max_rounds));
        loop {
            tokio::select! {
                result = &mut run => break result?,
                event = events.recv() => match event {
                    Some(FrontEndEvent::Cancel) => {
                        if signal.is_cancelled() {
                            std::process::exit(130);
                        }
                        signal.cancel();
                    }
                    // End of input or an explicit quit lets the discussion wind down the
                    // same way a cancel does, so the stream still gets its ending.
                    Some(FrontEndEvent::Quit) | None => signal.cancel(),
                    // Plan mode is a session gesture, and the session is mid-discussion:
                    // it waits until the rounds are over.
                    Some(FrontEndEvent::TogglePlan) => {}
                },
            }
        }
    };
    harness.notice(&format!(
        "fs-agent: {}",
        render::wording::discussion_ended(outcome.reason, outcome.rounds, &outcome.absent)
    ));
    Ok(())
}

/// Drive one discussion while still watching for the cancel gesture.
///
/// The same shape as [`run_one_turn`], for the same reason: the discussion owns the
/// session, so the loop cannot read the keyboard itself and listens for gestures
/// instead. A second press while a cancellation is already raised forces the process
/// down — the stream still gets its ending, because the first press asked for it.
async fn run_discussion(
    harness: &mut DiscussionHarness,
    events: &mut ConsoleEvents,
    question: &str,
) -> Result<crate::agent::DiscussionOutcome, crate::Error> {
    let signal = harness.cancel_signal();
    let mut run = Box::pin(harness.discuss(question));
    loop {
        tokio::select! {
            result = &mut run => return result,
            event = events.recv() => match event {
                Some(FrontEndEvent::Cancel) => {
                    if signal.is_cancelled() {
                        std::process::exit(130);
                    }
                    signal.cancel();
                }
                // End of input or an explicit quit lets the discussion wind down the
                // same way a cancel does, so the stream still gets its ending.
                Some(FrontEndEvent::Quit) | None => signal.cancel(),
                // Plan mode is a session gesture: a discussion has one question and no
                // prompt to return to, so there is nothing here to toggle it for.
                Some(FrontEndEvent::TogglePlan) => {}
            },
        }
    }
}

/// The question: the words it was given with, or stdin.
///
/// A terminal gets a prompt — nothing has entered raw mode yet, so a cooked read still
/// works — and a pipe is read to the end, which is what `echo 问题 | fs-agent discuss`
/// needs.
fn question(words: &[String]) -> Option<String> {
    let typed = words.join(" ");
    if !typed.trim().is_empty() {
        return Some(typed);
    }
    if std::io::stdin().is_terminal() {
        eprint!("{}", render::wording::question_prompt());
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        return match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim().to_owned()).filter(|question| !question.is_empty()),
        };
    }
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut text).ok()?;
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Read a line, run it, repeat — until the user leaves or input ends.
async fn interactive_loop(
    harness: &mut Harness,
    console: &ConsoleHandle,
    events: &mut ConsoleEvents,
    config: &Config,
) -> ExitCode {
    loop {
        // The loop is the only thing that knows whether something is running, so it
        // tells the front end instead of letting it infer (spec §6). Nothing is running
        // until a submission is taken, and the front end has to read that way from the
        // first frame — including before this loop's first prompt.
        console.set_running(false);
        // Between turns the loop is only waiting for a prompt; a gesture that
        // arrives here is handled without a turn in flight.
        let line = loop {
            tokio::select! {
                line = console.prompt() => break line,
                event = events.recv() => match event {
                    Some(FrontEndEvent::Quit) | None => return ExitCode::SUCCESS,
                    Some(FrontEndEvent::TogglePlan) => toggle_plan(harness).await,
                    Some(FrontEndEvent::Cancel) => {}
                },
            }
        };
        let Some(submitted) = line else {
            return ExitCode::SUCCESS;
        };
        // A line was taken, so from here until the next turn of this loop the session is
        // running something — a turn, a discussion, an undo. `Ctrl-C` is the cancel
        // gesture for all of them.
        console.set_running(true);
        match submission(&submitted, |name| harness.has_skill(name)) {
            // An empty line: nothing to answer, so ask again. `None` from the prompt
            // is the only thing that ends input, and that is handled just above.
            Submission::Ignore => {}
            Submission::Quit => return ExitCode::SUCCESS,
            Submission::Undo => match harness.undo_last_edit().await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    harness.notice(&format!("fs-agent: {}", render::wording::nothing_to_undo()))
                }
                Err(error) => harness.notice(&format!(
                    "fs-agent: {}",
                    render::wording::error_report(&error)
                )),
            },
            Submission::Plan => enter_plan(harness).await,
            Submission::EndPlan => match harness.exit_plan_mode().await {
                Ok(true) => {
                    harness.notice(&format!("fs-agent: {}", render::wording::plan_exited()))
                }
                Ok(false) => {}
                Err(error) => harness.notice(&format!(
                    "fs-agent: {}",
                    render::wording::error_report(&error)
                )),
            },
            Submission::Unknown(line) => {
                let names = harness.skill_names();
                harness.notice(&format!(
                    "fs-agent: {}",
                    render::wording::unknown_command(line, &names)
                ));
            }
            // `/<skill> [task]` is the user-side skill invocation (spec §9): the
            // one path a `disable-model-invocation: true` skill reserves for the
            // user. The body goes into the context at the tail. A bare `/<skill>`
            // runs there and then — the body *is* the instruction — and a task, when
            // one was typed, follows it as an ordinary user message.
            Submission::Skill { name, task } => {
                if task.is_empty() {
                    harness.notice(&format!(
                        "fs-agent: {}",
                        render::wording::skill_started(name)
                    ));
                    if let Err(error) = run_one_turn(harness, events, TurnStart::Skill(name)).await
                    {
                        harness.notice(&format!(
                            "fs-agent: {}",
                            render::wording::error_report(&error)
                        ));
                    }
                } else if let Err(error) = harness.load_skill(name) {
                    harness.notice(&format!(
                        "fs-agent: {}",
                        render::wording::error_report(&error)
                    ));
                } else {
                    harness.notice(&format!(
                        "fs-agent: {}",
                        render::wording::skill_loaded(name)
                    ));
                    if let Err(error) =
                        run_one_turn(harness, events, TurnStart::Prompt(&task)).await
                    {
                        harness.notice(&format!(
                            "fs-agent: {}",
                            render::wording::error_report(&error)
                        ));
                    }
                }
            }
            // `/discuss [问题]`: a discussion **on this session's stream** (spec §15).
            // The debaters are siblings of this session, so they inherit its context and
            // append their rounds to its log; the user is back at the prompt afterwards.
            Submission::Discuss(question) => {
                if let Err(error) = discuss_in_session(harness, events, config, question).await {
                    harness.notice(&format!(
                        "fs-agent: {}",
                        render::wording::error_report(&error)
                    ));
                }
            }
            // Everything else is a prompt, newlines and all: the transcript shows what
            // the user wrote, as one message (spec §12).
            Submission::Prompt(text) => {
                if let Err(error) = run_one_turn(harness, events, TurnStart::Prompt(text)).await {
                    harness.notice(&format!(
                        "fs-agent: {}",
                        render::wording::error_report(&error)
                    ));
                }
            }
        }
    }
}

/// The task or question that follows a command's name on its first line: the rest of
/// that line, then every line below it, as written. Only trailing blank lines go, so a
/// continuation of nothing but blanks is not a task.
///
/// One function for `/<skill>` and `/discuss` both, because "what the user wrote after
/// the command" is one rule and two copies of it would drift.
fn task_of(inline: &str, rest: &str) -> String {
    let mut task = inline.to_owned();
    let rest = rest.trim_end_matches('\n');
    if !rest.trim().is_empty() {
        if !task.is_empty() {
            task.push('\n');
        }
        task.push_str(rest);
    }
    task
}

/// What one submission asks for (spec §12).
#[derive(Debug, PartialEq, Eq)]
enum Submission<'a> {
    /// Nothing to send: an empty line, or one that is blank once trimmed. The loop
    /// asks again without starting a turn — an Enter on an empty input is an empty
    /// line, never the end of input (spec §6).
    Ignore,
    Quit,
    Undo,
    Plan,
    EndPlan,
    /// A first line that opens with `/` and names nothing known, with nothing but
    /// blank lines after it: a typo, and the one case the user is told about.
    Unknown(&'a str),
    /// `/<skill>` and the task that follows it.
    Skill {
        name: &'a str,
        task: String,
    },
    /// `/discuss [问题]`: run a discussion **on this session's stream** (spec §15).
    /// The question is empty when the user typed no question — the loop then puts the
    /// session's last question to the debaters.
    Discuss(String),
    /// The whole submission, newlines and all, as one prompt.
    Prompt(&'a str),
}

/// Read one submission.
///
/// The **first line alone** decides whether this is a command, so `/<skill>` can be
/// followed by a multi-line brief — the rest of its first line and every line below it
/// become the task. A first line that opens with `/` but names something unknown is a
/// typo when it is the whole submission (told about, as it always was) and a pasted
/// paragraph when it is not.
///
/// A submission that is blank once trimmed is [`Submission::Ignore`]: there is nothing
/// to send, so the loop asks again instead of starting a turn.
///
/// The built-ins take no task, so they match only as the **whole** submission: a line
/// after `/plan` must not be dropped on the floor. Such a submission falls through to
/// the rules above — `/plan` names no skill, so with lines after it the whole thing is
/// read as a prompt, which at least shows the user what they sent.
///
/// Leading slashes are all stripped (`//undo` reads as `undo`), which is how the old
/// code read a skill name too. Trailing blank lines are dropped from a task; the rest
/// is kept as written.
fn submission<'a>(text: &'a str, has_skill: impl Fn(&str) -> bool) -> Submission<'a> {
    if text.trim().is_empty() {
        return Submission::Ignore;
    }
    let (first, rest) = match text.split_once('\n') {
        Some((first, rest)) => (first.trim(), rest),
        None => (text.trim(), ""),
    };
    // A built-in is the whole submission or it is nothing: it has no task to hold the
    // lines that follow, and dropping them would lose what the user wrote.
    let whole = rest.trim().is_empty();
    match first {
        "/quit" | "/exit" if whole => Submission::Quit,
        "/undo" if whole => Submission::Undo,
        "/plan" if whole => Submission::Plan,
        "/endplan" if whole => Submission::EndPlan,
        _ if first.starts_with('/') => {
            let rest_of_line = first.trim_start_matches('/');
            let (name, inline) = match rest_of_line.split_once(char::is_whitespace) {
                Some((name, task)) => (name, task.trim()),
                None => (rest_of_line, ""),
            };
            // `/discuss [问题]` is the one built-in with an argument, so it takes the
            // same shape a skill's task does: the rest of the line, then every line
            // below it. A bare `/discuss` leaves the question to the loop.
            if name == "discuss" {
                return Submission::Discuss(task_of(inline, rest));
            }
            if !has_skill(name) {
                if rest.trim().is_empty() {
                    return Submission::Unknown(first);
                }
                // A pasted paragraph that happens to open with `/` is a paragraph.
                return Submission::Prompt(text);
            }
            Submission::Skill {
                name,
                task: task_of(inline, rest),
            }
        }
        // Not a command at all: the whole text, however many lines, is the prompt.
        _ => Submission::Prompt(text),
    }
}

/// What starts the turn the loop is about to drive.
enum TurnStart<'a> {
    /// A prompt the user typed: it becomes a `user` message and the turn runs.
    Prompt(&'a str),
    /// A bare `/<skill>`: [`Harness::run_skill`] loads the body, which projects as a
    /// `user` message of its own, so no prompt is invented for it. The transcript
    /// must never show words the user did not type.
    Skill(&'a str),
}

/// Run one turn while still watching for the cancel gesture.
///
/// The turn owns the session, so the loop cannot read the keyboard itself; it
/// selects on the console's unsolicited events instead. A second press while a
/// cancellation is already raised forces the process down (spec §6) — the
/// session never needs to know how it died, because `--continue` closes whatever
/// the process left open.
async fn run_one_turn(
    harness: &mut Harness,
    events: &mut ConsoleEvents,
    start: TurnStart<'_>,
) -> Result<(), crate::Error> {
    let signal = harness.cancel_signal();
    let mut turn = Box::pin(async move {
        match start {
            TurnStart::Prompt(input) => harness.run_turn(input).await,
            TurnStart::Skill(name) => harness.run_skill(name).await,
        }
    });
    loop {
        tokio::select! {
            result = &mut turn => return result.map(|_| ()),
            event = events.recv() => match event {
                Some(FrontEndEvent::Cancel) => {
                    if signal.is_cancelled() {
                        std::process::exit(130);
                    }
                    signal.cancel();
                }
                // End of input or an explicit quit lets the turn wind down the
                // same way a cancel does, so the stream still gets its ending.
                Some(FrontEndEvent::Quit) | None => signal.cancel(),
                Some(FrontEndEvent::TogglePlan) => {}
            },
        }
    }
}

/// Enter plan mode, reporting any failure on the front end.
async fn enter_plan(harness: &mut Harness) {
    let already = harness.mode() == Mode::Plan;
    match harness.enter_plan_mode().await {
        Ok(_) if !already => {
            harness.notice(&format!("fs-agent: {}", render::wording::plan_entered()))
        }
        Ok(_) => {}
        Err(error) => harness.notice(&format!(
            "fs-agent: {}",
            render::wording::error_report(&error)
        )),
    }
}

/// Shift+Tab: enter plan mode, or leave it if it is already on (spec §13).
async fn toggle_plan(harness: &mut Harness) {
    if harness.mode() == Mode::Plan {
        match harness.exit_plan_mode().await {
            Ok(true) => harness.notice(&format!("fs-agent: {}", render::wording::plan_exited())),
            Ok(false) => {}
            Err(error) => harness.notice(&format!(
                "fs-agent: {}",
                render::wording::error_report(&error)
            )),
        }
    } else {
        enter_plan(harness).await;
    }
}

#[derive(Debug, Default)]
struct ProbeArgs {
    config: Option<PathBuf>,
    models: Vec<String>,
}

fn parse_probe(args: &[String]) -> Result<ProbeArgs, String> {
    let mut parsed = ProbeArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--config" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_path("--config"))?;
                parsed.config = Some(PathBuf::from(value));
            }
            "--model" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_model("--model"))?;
                parsed.models.push(value.clone());
            }
            other => return Err(render::wording::unknown_argument(other)),
        }
        index += 1;
    }
    Ok(parsed)
}

/// Load configuration: an explicit `--config` must exist; the default path is
/// used only when it does.
fn load_config(explicit: Option<PathBuf>, env: &EnvMap) -> Result<Config, String> {
    match explicit {
        Some(path) => config::load(&path, env).map_err(|error| error.to_string()),
        None => {
            let path = config::default_path(env);
            if path.exists() {
                config::load(&path, env).map_err(|error| error.to_string())
            } else {
                config::resolve(None, env).map_err(|error| error.to_string())
            }
        }
    }
}

async fn probe(args: &[String], env: &EnvMap) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", render::wording::help_probe());
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_probe(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("fs-agent: {message}");
            println!("{}", render::wording::help_probe());
            return ExitCode::FAILURE;
        }
    };
    let config = match load_config(parsed.config, env) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("fs-agent: {}", render::wording::startup_config(&message));
            return ExitCode::FAILURE;
        }
    };

    // An unregistered model id is a startup error, never a silent downgrade:
    // check the whole table before probing anything.
    if let Err(message) = validate_models(&config) {
        eprintln!("fs-agent: {message}");
        return ExitCode::FAILURE;
    }

    let models: Vec<String> = if parsed.models.is_empty() {
        config
            .models_with_keys()
            .into_iter()
            .map(|model| model.id.clone())
            .collect()
    } else {
        parsed.models
    };
    if models.is_empty() {
        eprintln!("fs-agent: {}", render::wording::probe_no_key());
        return ExitCode::FAILURE;
    }

    let mut failed = false;
    // The library reads no environment, so the CLI hands it the one fact the
    // `rm` circuit breaker needs.
    let home = env
        .get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    for model in models {
        match probe_model(&config, &model, home.as_deref()).await {
            Ok(()) => {}
            Err(ProbeError::Skipped(message)) => eprintln!("fs-agent: skipping {model}: {message}"),
            Err(ProbeError::Failed(message)) => {
                eprintln!("fs-agent: {model}: {message}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Every configured model id must be in the capability table before anything
/// runs, so an unregistered id is a startup error rather than a surprise after
/// a turn has begun.
fn validate_models(config: &Config) -> Result<(), String> {
    for model in config.models.values() {
        caps_for(&model.id).map_err(|error| error.to_string())?;
    }
    Ok(())
}

enum ProbeError {
    Skipped(String),
    Failed(String),
}

impl ProbeError {
    fn failed(error: impl std::fmt::Display) -> Self {
        ProbeError::Failed(error.to_string())
    }
}

async fn probe_model(
    config: &Config,
    model_id: &str,
    home: Option<&Path>,
) -> Result<(), ProbeError> {
    let (_, profile) = config
        .resolve_model(Some(model_id))
        .map_err(ProbeError::failed)?;
    let provider = match OpenAiProvider::build(config, model_id, stderr_warnings()) {
        Ok(provider) => provider,
        Err(BuildError::MissingKey { hint, .. }) => {
            return Err(ProbeError::Skipped(format!("no API key (export {hint})")))
        }
        Err(error) => return Err(ProbeError::failed(error)),
    };

    let dir = probe_dir(model_id);
    std::fs::create_dir_all(&dir).map_err(ProbeError::failed)?;
    let log_path = dir.join("log.jsonl");
    let _ = std::fs::remove_file(&log_path);

    // Every configured value the session needs — generation parameters, the token
    // allowance, the price table, the routing overrides — arrives through one
    // function (spec §17), so this probe and any later front end assemble alike.
    let session_config = config
        .session_config(model_id)
        .map_err(ProbeError::failed)?;
    let mut harness = assemble(AssemblyParts {
        scaffold: SessionScaffold {
            cwd: dir,
            log_path: log_path.clone(),
            session_id: SessionId::new(format!("probe-{model_id}")),
            // The probe exercises real turns, so it gets the real tool table —
            // except for `ask_user_question`: headless has no answerer, and a tool
            // that can only fail wastes a model call (spec §19).
            tools: tools::with_dynamic(&config.tools, false),
            locks: PathLocks::new(),
            // The probe is headless and has no answerer, so the interactive
            // default `ask` refuses writes rather than hanging on a question
            // nobody can see.
            policy: Policy::for_mode(Mode::Ask),
            asker: None,
            questions: None,
            // Ticket 05 lands the mount points; wiring user-declared hooks into
            // the CLI is nobody's ticket yet, so the probe runs without one.
            hook: None,
            home: home.map(Path::to_path_buf),
        },
        provider: Box::new(provider),
        speaker: SpeakerId::Debater(profile.name.clone().into()),
        config: session_config,
        renderer: Renderer::headless(RenderSinks {
            // The probe prints its own report on stdout; the renderer narrates
            // to stderr only.
            stdout_result: Box::new(std::io::sink()),
            stderr_diagnostic: Box::new(std::io::stderr()),
        }),
    })
    .await
    .map_err(ProbeError::failed)?;

    println!(
        "model {model_id}  (provider {}, {})",
        profile.name, profile.base_url
    );
    let first = probe_prompt();
    let turns: [&str; 2] = [first.as_str(), PROBE_FOLLOW_UP];
    for (turn, prompt) in turns.iter().enumerate() {
        if turn > 0 {
            // Let the vendor persist the first turn's prefix before asking again.
            tokio::time::sleep(CACHE_WARMUP).await;
        }
        harness.run_turn(prompt).await.map_err(ProbeError::failed)?;
        // The stream the turn just wrote is the report's only source. If it
        // cannot be read the run is not silently reported as free; it says so.
        let events = match read_events(&log_path) {
            Ok(events) => events,
            Err(error) => {
                println!(
                    "  turn {}: cannot read the session stream: {error}",
                    turn + 1
                );
                continue;
            }
        };
        match last_usage(&events) {
            Some(usage) => println!(
                "  turn {}: input={} output={} cached={} miss={}",
                turn + 1,
                usage.input_tokens,
                usage.output_tokens,
                usage.cached_tokens,
                usage.miss_tokens,
            ),
            None => println!("  turn {}: no usage recorded", turn + 1),
        }
        // Money is display only (spec §17): the probe shows what the session has
        // cost when the model has a price, and says which table is missing when
        // it does not. `sessions stats` (ticket 17) is where this belongs long
        // term; the probe is the one report a person reads today.
        let spent = total_usage(&events);
        match config.pricing.cost(model_id, spent) {
            Some(cost) => println!("  session: {} tokens, ${cost:.6}", spent.total_tokens()),
            None => println!(
                "  session: {} tokens (no [pricing.{model_id}] entry, so no cost)",
                spent.total_tokens()
            ),
        }
    }
    harness.shutdown().await;
    Ok(())
}

fn last_usage(events: &[Event]) -> Option<Usage> {
    events.iter().rev().find_map(|event| match &event.payload {
        EventPayload::UsageRecorded { usage } => Some(*usage),
        _ => None,
    })
}

fn probe_dir(model_id: &str) -> PathBuf {
    let slug: String = model_id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    std::env::temp_dir().join("fs-agent-probe").join(slug)
}

#[derive(Debug)]
struct PruneArgs {
    /// How many of the most recent sessions to keep in the bucket.
    keep: usize,
    dry_run: bool,
    /// The workspace whose bucket is pruned; the current directory by default.
    cwd: Option<PathBuf>,
}

fn parse_prune(args: &[String]) -> Result<PruneArgs, String> {
    let mut parsed = PruneArgs {
        // Keep the session `--continue` would resume, remove the rest.
        keep: 1,
        dry_run: false,
        cwd: None,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--keep" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_value("--keep"))?;
                parsed.keep = value
                    .parse()
                    .map_err(|_| render::wording::needs_number("--keep", value))?;
            }
            "--dry-run" => parsed.dry_run = true,
            "--cwd" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_path("--cwd"))?;
                parsed.cwd = Some(PathBuf::from(value));
            }
            other => return Err(render::wording::unknown_argument(other)),
        }
        index += 1;
    }
    Ok(parsed)
}

/// `prune` removes session directories, by hand (spec §11: nothing prunes
/// automatically).
///
/// It works on one bucket — the workspace's — because that is the unit the store
/// is bound to, and it keeps the newest `--keep` sessions (default one, the
/// session `--continue` would resume). stdout carries the result; diagnostics go
/// to stderr.
fn prune(args: &[String], env: &EnvMap) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", render::wording::help_prune());
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_prune(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("fs-agent: {message}");
            println!("{}", render::wording::help_prune());
            return ExitCode::FAILURE;
        }
    };
    let cwd = match parsed.cwd {
        Some(path) => path,
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_cwd(&error.to_string())
                );
                return ExitCode::FAILURE;
            }
        },
    };
    let Some(root) = config::sessions_dir(env) else {
        eprintln!("fs-agent: {}", render::wording::startup_no_session_store());
        return ExitCode::FAILURE;
    };
    let store = SessionStore::new(root);

    if parsed.dry_run {
        return match store.list(&cwd) {
            Ok(sessions) => {
                for session in sessions.into_iter().skip(parsed.keep) {
                    println!(
                        "{}",
                        render::wording::prune_would_remove(
                            session.id.as_str(),
                            &session.dir.display().to_string()
                        )
                    );
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!(
                    "fs-agent: {}",
                    render::wording::startup_store_read(&error.to_string())
                );
                ExitCode::FAILURE
            }
        };
    }

    match store.prune(&cwd, parsed.keep) {
        Ok(removed) => {
            for session in removed {
                println!(
                    "{}",
                    render::wording::prune_removed(
                        session.id.as_str(),
                        &session.dir.display().to_string()
                    )
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "fs-agent: {}",
                render::wording::startup_store_prune(&error.to_string())
            );
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------
// `sessions`: the observability CLI (spec §18)
// ---------------------------------------------------------------------------

/// One parsed `sessions` invocation. The verb decides which fields are read.
#[derive(Debug, Default)]
struct SessionsArgs {
    verb: String,
    id: Option<String>,
    all: bool,
    files: bool,
    only_error: bool,
    json: bool,
    limit: Option<usize>,
    round: Option<u32>,
    speaker: Option<String>,
    kind: Option<String>,
    tool: Option<String>,
    model: Option<String>,
    config: Option<PathBuf>,
    cwd: Option<PathBuf>,
}

fn parse_sessions(args: &[String]) -> Result<SessionsArgs, String> {
    let mut parsed = SessionsArgs::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--all" => parsed.all = true,
            "--files" => parsed.files = true,
            "--only-error" => parsed.only_error = true,
            "--json" => parsed.json = true,
            "--round" | "--speaker" | "--kind" | "--tool" | "--model" | "--config" | "--cwd"
            | "--limit" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| render::wording::needs_value(arg))?;
                match arg {
                    "--round" => {
                        parsed.round = Some(
                            value
                                .parse()
                                .map_err(|_| render::wording::needs_number("--round", value))?,
                        )
                    }
                    "--speaker" => parsed.speaker = Some(value.clone()),
                    "--kind" => parsed.kind = Some(value.clone()),
                    "--tool" => parsed.tool = Some(value.clone()),
                    "--model" => parsed.model = Some(value.clone()),
                    "--config" => parsed.config = Some(PathBuf::from(value)),
                    "--cwd" => parsed.cwd = Some(PathBuf::from(value)),
                    "--limit" => {
                        parsed.limit = Some(
                            value
                                .parse()
                                .map_err(|_| render::wording::needs_number("--limit", value))?,
                        )
                    }
                    _ => unreachable!(),
                }
            }
            other if other.starts_with('-') => {
                return Err(render::wording::unknown_argument(other))
            }
            other if parsed.verb.is_empty() => parsed.verb = other.to_owned(),
            other if parsed.id.is_none() => parsed.id = Some(other.to_owned()),
            other => return Err(render::wording::extra_argument(other)),
        }
        index += 1;
    }
    Ok(parsed)
}

/// The `sessions` subcommand family: `ls`, `show`, `replay`, `stats`.
///
/// stdout carries the result and stderr the diagnostics, whatever the verb; every
/// view has a `--json` form so a pipeline can read it. There is no index: a
/// session is found by scanning its bucket, and `--continue`'s "newest first"
/// order is the order `ls` shows.
pub fn run_sessions(
    args: &[String],
    env: &EnvMap,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_sessions_help(out);
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_sessions(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            let _ = writeln!(err, "fs-agent: {message}");
            print_sessions_help(err);
            return ExitCode::FAILURE;
        }
    };

    let result = match parsed.verb.as_str() {
        "ls" => sessions_ls(&parsed, env, out, err),
        "show" => sessions_show(&parsed, env, out),
        "replay" => sessions_replay(&parsed, env, out),
        "stats" => sessions_stats(&parsed, env, out),
        "" => {
            let _ = writeln!(err, "fs-agent: {}", render::wording::sessions_needs_verb());
            print_sessions_help(err);
            return ExitCode::FAILURE;
        }
        other => {
            let _ = writeln!(
                err,
                "fs-agent: {}",
                render::wording::unknown_sessions_verb(other)
            );
            print_sessions_help(err);
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(err, "fs-agent: {message}");
            ExitCode::FAILURE
        }
    }
}

/// The session store, or the refusal that there is nowhere to look.
fn open_store(env: &EnvMap) -> Result<SessionStore, String> {
    let root = config::sessions_dir(env)
        .ok_or_else(|| render::wording::startup_no_session_store().to_owned())?;
    Ok(SessionStore::new(root))
}

/// The workspace whose bucket is searched first: `--cwd`, else the current dir.
fn search_cwd(parsed: &SessionsArgs) -> Result<PathBuf, String> {
    match &parsed.cwd {
        Some(path) => Ok(path.clone()),
        None => std::env::current_dir()
            .map_err(|error| render::wording::startup_cwd(&error.to_string())),
    }
}

/// Find one session by id (or by the path of its directory).
fn find_session(store: &SessionStore, cwd: &Path, id: &str) -> Result<StoredSession, String> {
    let direct = PathBuf::from(id);
    if direct.join(crate::session::store::LOG_FILE).is_file() {
        return Ok(session_at(direct));
    }
    // The id is globally unique, but the owning bucket is the fast path; the
    // store-wide scan is what makes `sessions show` work from any directory.
    let mut candidates = store.list(cwd).unwrap_or_default();
    candidates.extend(store.list_all().unwrap_or_default());
    candidates
        .into_iter()
        .find(|session| session.id.as_str() == id)
        .ok_or_else(|| render::wording::no_session(id, &store_list_label(cwd)))
}

fn store_list_label(cwd: &Path) -> String {
    render::wording::session_search_label(&cwd.display().to_string())
}

fn session_at(dir: PathBuf) -> StoredSession {
    let id = dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    StoredSession {
        id: SessionId::new(id),
        outputs_dir: dir.join(crate::session::store::OUTPUTS_DIR),
        log_path: dir.join(crate::session::store::LOG_FILE),
        dir,
    }
}

/// Parse `--speaker`: `user`, `system`, `executor:<id>`, or a debater's name.
fn parse_speaker(raw: &str) -> SpeakerId {
    match raw {
        "user" => SpeakerId::User,
        "system" => SpeakerId::System,
        other => match other.strip_prefix("executor:") {
            Some(id) => SpeakerId::Executor(id.into()),
            None => SpeakerId::Debater(other.into()),
        },
    }
}

fn sessions_ls(
    parsed: &SessionsArgs,
    env: &EnvMap,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<(), String> {
    let store = open_store(env)?;
    let cwd = search_cwd(parsed)?;
    let listings = observe::list(&store, (!parsed.all).then_some(cwd.as_path()))
        .map_err(|error| render::wording::startup_store_read(&error.to_string()))?;
    let mut listings: Vec<Listing> = listings;
    if let Some(limit) = parsed.limit {
        listings.truncate(limit);
    }

    if parsed.json {
        return write_json(out, &listings);
    }
    if listings.is_empty() {
        let _ = writeln!(err, "fs-agent: {}", render::wording::no_sessions());
        return Ok(());
    }
    // The header is padded by **display columns**: a Chinese column name is
    // narrower in characters than in columns, so `{:<36}` would shift it two
    // columns left of the row it labels.
    let columns = render::wording::ls_columns();
    let _ = writeln!(
        out,
        "{}  {}  {}  {}  {}  {}  {}",
        pad_end(columns[0], 36),
        pad_end(columns[1], 28),
        pad_end(columns[2], 20),
        pad_start(columns[3], 10),
        pad_start(columns[4], 6),
        pad_start(columns[5], 5),
        columns[6],
    );
    for listing in listings {
        let _ = writeln!(
            out,
            "{:<36}  {:<28}  {:<20}  {:>10}  {:>6}  {:>5}  {}",
            listing.id,
            listing.cwd.as_deref().unwrap_or("-"),
            listing
                .started
                .map(|at| at.format("%Y-%m-%d %H:%M:%SZ").to_string())
                .unwrap_or_else(|| "-".to_owned()),
            listing.tokens,
            listing.rounds,
            listing.messages,
            listing
                .ended
                .map(render::wording::stop_reason)
                .unwrap_or_else(render::wording::open_session),
        );
    }
    Ok(())
}

/// Pad `text` on the right to `width` **display columns**.
fn pad_end(text: &str, width: usize) -> String {
    let used = text.cell_width() as usize;
    if used >= width {
        return text.to_owned();
    }
    format!("{text}{}", " ".repeat(width - used))
}

/// Pad `text` on the left to `width` **display columns**.
fn pad_start(text: &str, width: usize) -> String {
    let used = text.cell_width() as usize;
    if used >= width {
        return text.to_owned();
    }
    format!("{}{text}", " ".repeat(width - used))
}

fn sessions_show(parsed: &SessionsArgs, env: &EnvMap, out: &mut dyn Write) -> Result<(), String> {
    let store = open_store(env)?;
    let cwd = search_cwd(parsed)?;
    let id = parsed
        .id
        .as_deref()
        .ok_or_else(render::wording::show_needs_id)?;
    let session = find_session(&store, &cwd, id)?;
    let events = read_events(&session.log_path).map_err(|error| {
        render::wording::cannot_read(&session.log_path.display().to_string(), &error.to_string())
    })?;

    if parsed.files {
        let changes = filter_changes(observe::file_history(&events), parsed);
        if parsed.json {
            return write_json(out, &changes);
        }
        for change in changes {
            let _ = writeln!(
                out,
                "{:<18}  {:<14}  {}",
                change
                    .round
                    .map(render::wording::round_label)
                    .unwrap_or_else(|| render::wording::session_group().to_owned()),
                change.speaker,
                change.path,
            );
        }
        return Ok(());
    }

    let filter = Filter {
        round: parsed.round,
        speaker: parsed.speaker.as_deref().map(parse_speaker),
        kind: parsed.kind.clone(),
        tool: parsed.tool.clone(),
        only_error: parsed.only_error,
    };
    let timeline = observe::timeline(&events);
    let timeline = if filter.is_empty() {
        timeline
    } else {
        timeline.filtered(&filter)
    };
    if parsed.json {
        return write_json(out, &timeline);
    }
    print_timeline(out, &timeline, &filter);
    Ok(())
}

/// `--files` honors `--round`, `--speaker` and `--tool`; the entry-shaped
/// filters (`--kind`, `--only-error`) have no meaning for a file-change row.
fn filter_changes(
    changes: Vec<observe::FileChange>,
    parsed: &SessionsArgs,
) -> Vec<observe::FileChange> {
    let speaker = parsed.speaker.as_deref().map(parse_speaker);
    changes
        .into_iter()
        .filter(|change| {
            parsed.round.is_none_or(|round| change.round == Some(round))
                && speaker
                    .as_ref()
                    .is_none_or(|speaker| &change.speaker == speaker)
                && parsed.tool.as_ref().is_none_or(|tool| &change.tool == tool)
        })
        .collect()
}

fn sessions_replay(parsed: &SessionsArgs, env: &EnvMap, out: &mut dyn Write) -> Result<(), String> {
    let store = open_store(env)?;
    let cwd = search_cwd(parsed)?;
    let id = parsed
        .id
        .as_deref()
        .ok_or_else(render::wording::replay_needs_id)?;
    let speaker = parsed
        .speaker
        .as_deref()
        .ok_or_else(render::wording::replay_needs_speaker)?;
    let session = find_session(&store, &cwd, id)?;
    let events = read_events(&session.log_path).map_err(|error| {
        render::wording::cannot_read(&session.log_path.display().to_string(), &error.to_string())
    })?;

    let config = load_config(parsed.config.clone(), env)?;
    let model = parsed
        .model
        .clone()
        .unwrap_or_else(|| config.default_model.clone());
    let speaker = parse_speaker(speaker);
    // Caps follow routing, exactly as the live call did: a routed synthesizer
    // or executor may answer on a model whose capability facts differ, and
    // reproducing the request under the debater's caps would be a silent lie
    // (spec §17, §18).
    let routing = CostModel::new(&model, config.pricing.clone()).with_routing(&config.routing);
    let caps = caps_for(routing.model_for(&speaker)).map_err(|error| error.to_string())?;

    let messages = replay::replay(&events, &speaker, parsed.round, &caps)
        .map_err(|error| error.to_string())?;
    if parsed.json {
        return write_json(out, &messages);
    }
    print_messages(out, &messages);
    Ok(())
}

fn sessions_stats(parsed: &SessionsArgs, env: &EnvMap, out: &mut dyn Write) -> Result<(), String> {
    let store = open_store(env)?;
    let cwd = search_cwd(parsed)?;
    let id = parsed
        .id
        .as_deref()
        .ok_or_else(render::wording::stats_needs_id)?;
    let session = find_session(&store, &cwd, id)?;
    let events = read_events(&session.log_path).map_err(|error| {
        render::wording::cannot_read(&session.log_path.display().to_string(), &error.to_string())
    })?;

    // Money needs a model and the stream carries none, so the model is named
    // here: `--model`, else the configuration's default. The human view says
    // which model it priced at, so the number is never silently attributed.
    let config = load_config(parsed.config.clone(), env)?;
    let model = parsed
        .model
        .clone()
        .unwrap_or_else(|| config.default_model.clone());
    let cost = CostModel::new(model.clone(), config.pricing.clone()).with_routing(&config.routing);
    let stats = observe::stats(&events, Some(&cost));

    if parsed.json {
        return write_json(out, &stats);
    }
    print_stats(out, &stats, &model);
    Ok(())
}

fn write_json<T: serde::Serialize>(out: &mut dyn Write, value: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    writeln!(out, "{text}").map_err(|error| error.to_string())
}

fn print_timeline(out: &mut dyn Write, timeline: &Timeline, filter: &Filter) {
    // Usage rows are the stats view's material; the default transcript skips
    // them unless they were asked for by name.
    let kind = filter.kind.as_deref().map(observe::canonical_kind);
    let show_usage = kind.as_deref() == Some("usagerecorded");
    for group in &timeline.groups {
        match group.round {
            Some(round) => {
                let _ = writeln!(out, "\n{}", render::wording::round_group(round, group.mode));
            }
            None => {
                let _ = writeln!(out, "\n{}", render::wording::session_group());
            }
        }
        for entry in &group.entries {
            if matches!(entry, Entry::RoundStarted { .. }) {
                continue;
            }
            if matches!(entry, Entry::Usage { .. }) && !show_usage {
                continue;
            }
            let _ = writeln!(out, "{}", render_entry(entry));
        }
    }
}

fn render_entry(entry: &Entry) -> String {
    let label = |speaker: &SpeakerId| render::wording::speaker_label(speaker);
    match entry {
        Entry::Message { speaker, text, .. } => format!("{} {text}", label(speaker)),
        Entry::Tool {
            speaker,
            tool,
            args,
            ok,
            output,
            error,
            hook,
            ..
        } => {
            let mut lines = vec![format!(
                "{} → {}",
                label(speaker),
                render::wording::tool_call(tool, &args.to_string())
            )];
            match (ok, output, error) {
                (Some(true), Some(output), _) => {
                    lines.push(indent(
                        &render::wording::tool_output_preview(output, PREVIEW),
                        2,
                    ));
                }
                (Some(false), _, error) => {
                    let message = error
                        .clone()
                        .unwrap_or_else(|| render::wording::no_message().to_owned());
                    lines.push(indent(&render::wording::tool_error(&message), 2));
                }
                _ => lines.push(indent(render::wording::no_tool_result(), 2)),
            }
            if let Some(hook) = hook {
                lines.push(indent(&render::wording::hook_feedback(hook), 2));
            }
            lines.join("\n")
        }
        Entry::RoundStarted { round, mode } => render::wording::round_section(*round, *mode),
        Entry::RoundEnded { round, reason } => render::wording::round_ended(*round, *reason),
        Entry::SessionEnded { reason } => render::wording::session_ended(*reason),
        Entry::TurnStarted { speaker, iteration } => {
            format!(
                "{} {}",
                label(speaker),
                render::wording::turn_started(*iteration)
            )
        }
        Entry::TurnEnded { speaker, reason } => {
            format!(
                "{} {}",
                label(speaker),
                render::wording::turn_ended(*reason)
            )
        }
        Entry::Divergence {
            topic, positions, ..
        } => {
            let mut lines = vec![format!("!! {}", render::wording::divergence(topic))];
            for position in positions {
                lines.push(indent(&format!("- {position}"), 2));
            }
            lines.join("\n")
        }
        Entry::PermissionAsked {
            speaker, request, ..
        } => format!(
            "{} {}",
            label(speaker),
            render::wording::permission_asked(
                crate::events::permission_format::tool_name(request),
                &render::transcript::summarize_permission_target(request)
            )
        ),
        Entry::PermissionDecided {
            speaker,
            decision,
            source,
            reason,
            ..
        } => format!(
            "{} {}",
            label(speaker),
            render::wording::permission_decided(*decision, *source, reason.as_deref())
        ),
        Entry::Hook {
            speaker,
            point,
            outcome,
            ..
        } => format!(
            "{} {}",
            label(speaker),
            render::wording::hook(point, outcome)
        ),
        Entry::ExecutorSpawned {
            speaker,
            executor_id,
            ..
        } => format!(
            "{} {}",
            label(speaker),
            render::wording::executor_spawned(executor_id.as_str())
        ),
        Entry::ExecutorFinished {
            executor_id,
            reason,
            summary,
            ..
        } => render::wording::executor_finished(executor_id.as_str(), *reason, summary),
        Entry::Usage { speaker, usage } => {
            format!(
                "{} {}",
                label(speaker),
                render::wording::usage_summary(usage)
            )
        }
        Entry::AgentError {
            speaker, message, ..
        } => format!(
            "{} {}",
            label(speaker),
            render::wording::agent_error(message)
        ),
        Entry::SessionError { code, detail, .. } => render::wording::session_error(code, detail),
        Entry::History {
            reason, summary, ..
        } => render::wording::history(*reason, summary.as_deref()),
        Entry::Context { source, .. } => render::wording::context_injected(source.clone()),
    }
}

fn indent(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    text.lines()
        .map(|line| format!("{pad}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// How much of one tool result `sessions show` prints before eliding.
const PREVIEW: usize = 500;

fn print_messages(out: &mut dyn Write, messages: &[Message]) {
    for message in messages {
        match message {
            Message::System { content, .. } => {
                let _ = writeln!(out, "{}\n{content}\n", render::wording::replay_system());
            }
            Message::User { content, name, .. } => {
                let label = render::wording::replay_user(name.as_deref());
                let _ = writeln!(out, "{label}\n{content}\n");
            }
            Message::Assistant {
                content,
                tool_calls,
                ..
            } => {
                let _ = writeln!(out, "{}", render::wording::replay_assistant());
                if let Some(content) = content {
                    let _ = writeln!(out, "{content}");
                }
                for call in tool_calls {
                    let _ = writeln!(
                        out,
                        "{}",
                        render::wording::replay_tool_call(&call.name, &call.arguments)
                    );
                }
                let _ = writeln!(out);
            }
            Message::Tool {
                tool_call_id,
                content,
            } => {
                let _ = writeln!(
                    out,
                    "{}\n{content}\n",
                    render::wording::replay_tool(tool_call_id)
                );
            }
        }
    }
}

fn print_stats(out: &mut dyn Write, stats: &observe::Stats, model: &str) {
    let cost = match stats.session.cost {
        Some(cost) => render::wording::stats_cost(cost, model),
        None => render::wording::stats_no_cost(model),
    };
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_session(
            stats.session.tokens.total_tokens(),
            stats.session.calls,
            stats.session.messages,
            stats.session.rounds,
            &cost,
        )
    );
    for speaker in &stats.speakers {
        let hit = speaker
            .hit_rate
            .map(|rate| format!("{:.0}%", rate * 100.0))
            .unwrap_or_else(|| "-".to_owned());
        let cost = speaker
            .cost
            .map(|cost| format!("  ${cost:.6}"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{}",
            render::wording::stats_speaker(
                &pad_end(&speaker.speaker.to_string(), 16),
                speaker.tokens.total_tokens(),
                speaker.calls,
                &hit,
                &cost,
            )
        );
    }
    let rate = stats
        .absence
        .rate
        .map(|rate| format!(" ({:.0}%)", rate * 100.0))
        .unwrap_or_default();
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_absence(stats.absence.one_sided, stats.absence.rounds, &rate)
    );
    for agent in &stats.absence.per_speaker {
        let _ = writeln!(
            out,
            "{}",
            render::wording::stats_absent(&agent.name, agent.count)
        );
    }
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_edits(stats.edits.succeeded, stats.edits.failed_matches)
    );
    for level in &stats.edits.levels {
        let _ = writeln!(
            out,
            "{}",
            render::wording::stats_match_level(&level.name, level.count)
        );
    }
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_guards(
            stats.guards.read_before_write,
            stats.guards.invalidated_reads
        )
    );
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_executors(stats.executors.spawned, stats.executors.finished)
    );
    for reason in &stats.executors.by_reason {
        let _ = writeln!(
            out,
            "{}",
            render::wording::stats_executor_reason(
                render::wording::stop_reason_name(&reason.name),
                reason.count
            )
        );
    }
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_hooks(
            stats.hooks.executed,
            stats.hooks.pre,
            stats.hooks.post,
            stats.hooks.feedback,
            stats.hooks.failed
        )
    );
    let decisions = stats
        .permissions
        .decided
        .iter()
        .map(|count| {
            render::wording::stats_decision(
                count.count,
                render::wording::decision_name(&count.name),
            )
        })
        .collect::<Vec<_>>()
        .join("、");
    let decided = if decisions.is_empty() {
        String::new()
    } else {
        format!("，已裁决 {decisions}")
    };
    let _ = writeln!(
        out,
        "{}",
        render::wording::stats_permissions(stats.permissions.asked, &decided)
    );
    let rate = stats
        .divergence_rate
        .map(|rate| format!(" ({:.0}%)", rate * 100.0))
        .unwrap_or_default();
    let rounds = stats
        .rounds
        .iter()
        .map(|round| {
            let ended = round
                .ended
                .map(render::wording::stats_round_ended)
                .unwrap_or_default();
            render::wording::stats_round(round.round, round.mode, round.calls, &ended)
        })
        .collect::<Vec<_>>()
        .join("; ");
    let head = format!(
        "{}{}",
        render::wording::stats_divergences(stats.divergences, stats.absence.rounds, &rate),
        render::wording::stats_rounds_prefix()
    );
    let _ = writeln!(out, "{head}{rounds}");
    for stop in &stats.stops {
        let _ = writeln!(
            out,
            "{}",
            render::wording::stats_stop(render::wording::stop_reason_name(&stop.name), stop.count)
        );
    }
}

fn print_sessions_help(out: &mut dyn Write) {
    let _ = writeln!(out, "{}", render::wording::help_sessions());
}

#[cfg(test)]
mod tests {
    use super::{submission, Submission};

    /// The skills a session knows about in these tests.
    fn has_skill(name: &str) -> bool {
        matches!(name, "ask-matt" | "review")
    }

    fn read(text: &str) -> Submission<'_> {
        submission(text, has_skill)
    }

    #[test]
    fn a_blank_submission_is_ignored_rather_than_sent_or_read_as_the_end_of_input() {
        // What an Enter on an empty input produces. The loop used to decide this
        // itself, so nothing in `cargo test` pinned it -- and the TUI once sent the
        // empty draft as the end-of-input sentinel, which quit the whole session.
        assert!(matches!(read(""), Submission::Ignore));
        assert!(matches!(read("   "), Submission::Ignore));
        assert!(matches!(read("\n\n\t\n"), Submission::Ignore));
        // Nor is it a prompt: an empty message must not reach the model.
        assert!(!matches!(read("  \n  "), Submission::Prompt(_)));
    }

    #[test]
    fn a_single_line_still_reads_exactly_as_it_did() {
        assert_eq!(read("/quit"), Submission::Quit);
        assert_eq!(read("/exit"), Submission::Quit);
        assert_eq!(read("/undo"), Submission::Undo);
        assert_eq!(read("/plan"), Submission::Plan);
        assert_eq!(read("/endplan"), Submission::EndPlan);
        assert_eq!(read("  /quit  "), Submission::Quit, "trimmed, as before");
        assert_eq!(read("/nope"), Submission::Unknown("/nope"));
        assert_eq!(
            read("/ask-matt 帮我看一下"),
            Submission::Skill {
                name: "ask-matt",
                task: "帮我看一下".to_owned(),
            }
        );
        assert_eq!(
            read("/ask-matt"),
            Submission::Skill {
                name: "ask-matt",
                task: String::new(),
            },
            "a bare skill only loads"
        );
        assert_eq!(read("hello"), Submission::Prompt("hello"));
    }

    #[test]
    fn a_multi_line_brief_follows_the_skill_named_on_the_first_line() {
        assert_eq!(
            read("/ask-matt\n第一行\n第二行"),
            Submission::Skill {
                name: "ask-matt",
                task: "第一行\n第二行".to_owned(),
            }
        );
        // The task may start on the skill's own line and carry on below it.
        assert_eq!(
            read("/ask-matt 第一行\n第二行"),
            Submission::Skill {
                name: "ask-matt",
                task: "第一行\n第二行".to_owned(),
            }
        );
    }

    #[test]
    fn a_pasted_paragraph_that_opens_with_a_slash_is_a_prompt() {
        // Anything with lines after it is read as the paragraph it is, so pasting a
        // path or a snippet does not earn an "unknown command" it never meant.
        assert_eq!(
            read("/usr/bin/env cargo test\n第二行"),
            Submission::Prompt("/usr/bin/env cargo test\n第二行")
        );
        // On one line it is still a typo, and one the user is told about.
        assert_eq!(read("/usr/bin/env"), Submission::Unknown("/usr/bin/env"));
        // Blank lines after it are still "nothing after it".
        assert_eq!(read("/nope\n   "), Submission::Unknown("/nope"));
    }

    #[test]
    fn a_built_in_takes_no_task_so_a_line_after_it_is_not_dropped() {
        // `/plan` followed by anything else is not the command: the lines after it must
        // not vanish, so the whole submission is read by the rules for a `/`-opening
        // line — `/plan` names no skill, so it is a prompt.
        assert_eq!(
            read("/plan\n把 X 改成 Y"),
            Submission::Prompt("/plan\n把 X 改成 Y")
        );
        assert_eq!(read("/plan"), Submission::Plan);
        assert_eq!(
            read("/plan  "),
            Submission::Plan,
            "trailing blanks are fine"
        );
    }

    #[test]
    fn a_skill_followed_only_by_blanks_still_only_loads() {
        assert_eq!(
            read("/review\n   \n"),
            Submission::Skill {
                name: "review",
                task: String::new(),
            }
        );
    }

    #[test]
    fn anything_else_is_the_whole_message() {
        let text = "第一行\n第二行\n第三行";
        assert_eq!(read(text), Submission::Prompt(text));
        // Kept as written: the message is what the user typed, not a trimmed version.
        assert_eq!(
            read("  第一行\n第二行  "),
            Submission::Prompt("  第一行\n第二行  ")
        );
    }

    // --- the in-session discussion (spec §15) -------------------------------

    #[test]
    fn a_session_discussion_takes_a_question_or_leaves_it_to_the_loop() {
        // The question is the rest of the line plus whatever follows it, exactly as a
        // skill's task is: a discussion question can be a paragraph.
        assert_eq!(
            read("/discuss 要不要换掉权限模型"),
            Submission::Discuss("要不要换掉权限模型".to_owned())
        );
        assert_eq!(
            read("/discuss 第一行\n第二行\n\n"),
            Submission::Discuss("第一行\n第二行".to_owned())
        );
        // Bare: the loop puts the session's last question to the debaters.
        assert_eq!(read("/discuss"), Submission::Discuss(String::new()));
        assert_eq!(read("  /discuss   "), Submission::Discuss(String::new()));
        // Only that name is a command: a skill called `discussion` is a skill.
        assert_eq!(read("/discussion x"), Submission::Unknown("/discussion x"));
    }

    // --- the pool a discussion draws from (spec §15) -------------------------

    use super::{parse_discuss_line, pick_debaters, split_names};
    use crate::config::{Debater, DiscussionRoster};

    fn pool() -> DiscussionRoster {
        DiscussionRoster {
            debaters: vec![
                Debater {
                    name: "保守".to_owned(),
                    model: "kimi-k3".to_owned(),
                    soul: Some("保守".to_owned()),
                },
                Debater {
                    name: "激进".to_owned(),
                    model: "deepseek-v4-pro".to_owned(),
                    soul: None,
                },
                Debater {
                    name: "审查".to_owned(),
                    model: "deepseek-flash".to_owned(),
                    soul: None,
                },
            ],
            max_rounds: None,
        }
    }

    fn names(pair: &[Debater; 2]) -> [String; 2] {
        [pair[0].name.clone(), pair[1].name.clone()]
    }

    #[test]
    fn a_discussion_draws_a_pair_or_takes_the_one_it_is_given() {
        // Named: exactly those two, in the order asked for.
        let picked =
            pick_debaters(&pool(), Some(["审查".to_owned(), "保守".to_owned()]), 0).unwrap();
        assert_eq!(names(&picked), ["审查", "保守"]);
        assert_eq!(picked[0].model, "deepseek-flash");

        // Drawn: two distinct pool members, and the seed decides which.
        let first = pick_debaters(&pool(), None, 0).unwrap();
        assert_eq!(names(&first), ["保守", "激进"]);
        let second = pick_debaters(&pool(), None, 2).unwrap();
        assert_eq!(names(&second), ["保守", "审查"], "the seed moves the pair");

        // A name the pool does not have says what it does have.
        let error =
            pick_debaters(&pool(), Some(["保守".to_owned(), "没有".to_owned()]), 0).unwrap_err();
        assert!(error.contains("没有"), "{error}");
        assert!(error.contains("`审查`"), "{error}");
    }

    #[test]
    fn a_discussion_command_reads_its_flags_before_the_question() {
        let line = parse_discuss_line("--debaters 保守,激进 换个角度再看").unwrap();
        assert_eq!(line.debaters, Some(["保守".to_owned(), "激进".to_owned()]));
        assert_eq!(line.question, "换个角度再看");

        // No question: the loop supplies the session's last one.
        let bare = parse_discuss_line("--debaters 保守,激进").unwrap();
        assert_eq!(bare.question, "");
        assert_eq!(bare.debaters.unwrap(), ["保守", "激进"]);

        // A question with no flags, newlines and all.
        let plain = parse_discuss_line("第一行\n第二行").unwrap();
        assert_eq!(plain.debaters, None);
        assert_eq!(plain.question, "第一行\n第二行");

        // Anything after `--` is the question, so a question may look like a flag.
        let after = parse_discuss_line("-- --看起来像参数的题目").unwrap();
        assert_eq!(after.question, "--看起来像参数的题目");

        assert!(parse_discuss_line("--nope x")
            .unwrap_err()
            .contains("--nope"));
        assert!(split_names("保守").unwrap_err().contains("两个名字"));
        assert_eq!(
            split_names("保守, 激进").unwrap(),
            ["保守".to_owned(), "激进".to_owned()],
            "whitespace after the comma is fine"
        );
    }

    #[test]
    fn the_discuss_subcommand_takes_the_same_selection() {
        let parsed = discuss_args(&["--debaters", "保守,激进", "问题"]).unwrap();
        assert_eq!(parsed.debaters.unwrap(), ["保守", "激进"]);
        assert_eq!(parsed.words, vec!["问题".to_owned()]);
        assert!(discuss_args(&["--debaters", "只有一个", "问题"])
            .unwrap_err()
            .contains("两个名字"));
    }

    // --- the discussion's arguments (spec §15) ------------------------------

    use super::{parse_discuss, question, DiscussArgs};

    fn discuss_args(args: &[&str]) -> Result<DiscussArgs, String> {
        parse_discuss(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn a_discussion_question_is_every_word_that_is_not_a_flag() {
        let parsed = discuss_args(&["把", "X", "换掉", "风险在哪"]).unwrap();
        assert_eq!(parsed.words.join(" "), "把 X 换掉 风险在哪");
        assert_eq!(
            question(&parsed.words).as_deref(),
            Some("把 X 换掉 风险在哪")
        );
        // Nothing to ask: the caller reads stdin instead.
        assert_eq!(question(&[]), None);
        assert_eq!(question(&["   ".to_owned()]), None);
    }

    #[test]
    fn a_discussion_reads_the_renderer_and_the_paths_it_is_given() {
        let parsed =
            discuss_args(&["--tui", "--cwd", "/tmp/x", "--config", "/tmp/c.toml", "q"]).unwrap();
        assert!(parsed.tui);
        assert!(!parsed.plain);
        assert_eq!(parsed.cwd.unwrap().to_str(), Some("/tmp/x"));
        assert_eq!(parsed.config.unwrap().to_str(), Some("/tmp/c.toml"));
        assert_eq!(parsed.words, vec!["q".to_owned()]);
    }

    #[test]
    fn a_discussion_rejects_what_it_cannot_honour() {
        // Two renderers, one process.
        assert!(discuss_args(&["--plain", "--tui", "q"])
            .unwrap_err()
            .contains("互斥"));
        // A flag that does not exist, and a flag missing its value.
        assert!(discuss_args(&["--model", "kimi-k3"])
            .unwrap_err()
            .contains("--model"));
        assert!(discuss_args(&["--cwd"]).unwrap_err().contains("--cwd"));
        // There is no `--continue`: a discussion is one question, one harness.
        assert!(discuss_args(&["--continue"])
            .unwrap_err()
            .contains("--continue"));
    }

    #[test]
    fn a_question_that_looks_like_a_flag_goes_after_a_double_dash() {
        let parsed = discuss_args(&["--", "--这段以横线开头"]).unwrap();
        assert_eq!(parsed.words, vec!["--这段以横线开头".to_owned()]);
    }
}
