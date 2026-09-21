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

use crate::agent::replay;
use crate::config::{self, Config, EnvMap};
use crate::events::{read_events, total_usage, Event, EventPayload, SessionId, SpeakerId, Usage};
use crate::permissions::{Mode, Policy};
use crate::provider::capability::caps_for;
use crate::provider::openai::{stderr_warnings, BuildError, OpenAiProvider};
use crate::provider::Message;
use crate::render::{
    self, ConsoleAsker, ConsoleEvents, ConsoleHandle, FrontEndEvent, PlainOptions, RenderSinks,
    Renderer, TuiOptions,
};
use crate::session::observe::{self, CostModel, Entry, Filter, Listing, Timeline};
use crate::session::{SessionStore, StoredSession};
use crate::tools::{self, PathLocks};
use crate::{assemble, AssemblyParts, Harness, SessionScaffold};

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
    (euid == 0).then(|| {
        "refusing to start as root (euid 0): every guardrail in this tool assumes the worst \
         case stays inside your workspace, and as root one misjudgement is system-wide. Run \
         it as your normal user; there is no bypass flag."
            .to_owned()
    })
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
            eprintln!("fs-agent: cannot start the async runtime: {error}");
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
            print_help();
            ExitCode::SUCCESS
        }
        Some("probe") => probe(&args[1..], env).await,
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
                    .ok_or_else(|| format!("{flag} needs a value"))?;
                match flag.as_str() {
                    "--config" => parsed.config = Some(PathBuf::from(value)),
                    "--model" => parsed.model = Some(value.clone()),
                    "--cwd" => parsed.cwd = Some(PathBuf::from(value)),
                    _ => unreachable!(),
                }
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
        index += 1;
    }
    if parsed.plain && parsed.tui {
        return Err(
            "--plain and --tui are mutually exclusive: there is one renderer per process"
                .to_owned(),
        );
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
        print_interactive_help();
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_interactive(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("fs-agent: {message}");
            print_interactive_help();
            return ExitCode::FAILURE;
        }
    };
    let config = match load_config(parsed.config.clone(), env) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("fs-agent: {message}");
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
        eprintln!("fs-agent: neither XDG_DATA_HOME nor HOME is set, so a session cannot be stored");
        return ExitCode::FAILURE;
    };
    let store = SessionStore::new(root);
    let cwd = match parsed.cwd.clone() {
        Some(cwd) => cwd,
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!("fs-agent: cannot determine the current directory: {error}");
                return ExitCode::FAILURE;
            }
        },
    };
    let stored = if parsed.resume {
        match store.latest(&cwd) {
            Ok(Some(session)) => session,
            Ok(None) => {
                eprintln!("fs-agent: no session in {} to continue", cwd.display());
                return ExitCode::FAILURE;
            }
            Err(error) => {
                eprintln!("fs-agent: cannot read the session store: {error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        match store.create(&cwd) {
            Ok(session) => session,
            Err(error) => {
                eprintln!("fs-agent: cannot create a session: {error}");
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
        Renderer::tui(TuiOptions { port })
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

    let mut harness = match assemble(AssemblyParts {
        scaffold: SessionScaffold {
            cwd: cwd.clone(),
            log_path: stored.log_path.clone(),
            session_id: stored.id.clone(),
            tools: tools::builtin(),
            locks: PathLocks::new(),
            // Interactive sessions start in `ask`: writes ask, reads are allowed
            // (spec §12). A headless caller gets no answerer and downgrades.
            policy: Policy::for_mode(Mode::Ask),
            asker: Some(asker),
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
    // first question.
    eprintln!(
        "fs-agent: session {} · model {model} · mode {} · {}{}",
        harness.session_id(),
        harness.mode(),
        stored.dir.display(),
        if parsed.resume { " (continued)" } else { "" },
    );

    let code = interactive_loop(&mut harness, &console, &mut events).await;
    harness.shutdown().await;
    code
}

/// Read a line, run it, repeat — until the user leaves or input ends.
async fn interactive_loop(
    harness: &mut Harness,
    console: &ConsoleHandle,
    events: &mut ConsoleEvents,
) -> ExitCode {
    loop {
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
        let Some(line) = line else {
            return ExitCode::SUCCESS;
        };
        let command = line.trim();
        if command.is_empty() {
            continue;
        }
        match command {
            "/quit" | "/exit" => return ExitCode::SUCCESS,
            "/undo" => match harness.undo_last_edit().await {
                Ok(Some(_)) => {}
                Ok(None) => eprintln!("fs-agent: nothing to undo"),
                Err(error) => eprintln!("fs-agent: {error}"),
            },
            "/plan" => enter_plan(harness).await,
            "/endplan" => {
                if let Err(error) = harness.exit_plan_mode().await {
                    eprintln!("fs-agent: {error}");
                }
            }
            other if other.starts_with('/') => {
                eprintln!("fs-agent: unknown command {other} (try /undo, /plan, /endplan, /quit)")
            }
            _ => {
                if let Err(error) = run_one_turn(harness, events, &line).await {
                    eprintln!("fs-agent: {error}");
                }
            }
        }
    }
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
    input: &str,
) -> Result<(), crate::Error> {
    let signal = harness.cancel_signal();
    let mut turn = Box::pin(harness.run_turn(input));
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

/// Enter plan mode, reporting any failure on stderr.
async fn enter_plan(harness: &mut Harness) {
    if let Err(error) = harness.enter_plan_mode().await {
        eprintln!("fs-agent: {error}");
    }
}

/// Shift+Tab: enter plan mode, or leave it if it is already on (spec §13).
async fn toggle_plan(harness: &mut Harness) {
    if harness.mode() == Mode::Plan {
        if let Err(error) = harness.exit_plan_mode().await {
            eprintln!("fs-agent: {error}");
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
                    .ok_or_else(|| "--config needs a path".to_owned())?;
                parsed.config = Some(PathBuf::from(value));
            }
            "--model" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--model needs a model id".to_owned())?;
                parsed.models.push(value.clone());
            }
            other => return Err(format!("unknown probe argument {other:?}")),
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
        print_probe_help();
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_probe(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("fs-agent: {message}");
            print_probe_help();
            return ExitCode::FAILURE;
        }
    };
    let config = match load_config(parsed.config, env) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("fs-agent: {message}");
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
        eprintln!(
            "fs-agent: no configured provider has a key. Export MOONSHOT_API_KEY and/or \
             DEEPSEEK_API_KEY, or set `api_key` under [providers.*] in config.toml."
        );
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
            // The probe exercises real turns, so it gets the real tool table.
            tools: tools::builtin(),
            locks: PathLocks::new(),
            // The probe is headless and has no answerer, so the interactive
            // default `ask` refuses writes rather than hanging on a question
            // nobody can see.
            policy: Policy::for_mode(Mode::Ask),
            asker: None,
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
                    .ok_or_else(|| "--keep needs a number".to_owned())?;
                parsed.keep = value
                    .parse()
                    .map_err(|_| format!("--keep needs a number, got {value:?}"))?;
            }
            "--dry-run" => parsed.dry_run = true,
            "--cwd" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--cwd needs a path".to_owned())?;
                parsed.cwd = Some(PathBuf::from(value));
            }
            other => return Err(format!("unknown prune argument {other:?}")),
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
        print_prune_help();
        return ExitCode::SUCCESS;
    }
    let parsed = match parse_prune(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("fs-agent: {message}");
            print_prune_help();
            return ExitCode::FAILURE;
        }
    };
    let cwd = match parsed.cwd {
        Some(path) => path,
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(error) => {
                eprintln!("fs-agent: cannot determine the current directory: {error}");
                return ExitCode::FAILURE;
            }
        },
    };
    let Some(root) = config::sessions_dir(env) else {
        eprintln!(
            "fs-agent: neither XDG_DATA_HOME nor HOME is set, so the session store cannot be found"
        );
        return ExitCode::FAILURE;
    };
    let store = SessionStore::new(root);

    if parsed.dry_run {
        return match store.list(&cwd) {
            Ok(sessions) => {
                for session in sessions.into_iter().skip(parsed.keep) {
                    println!("would remove {} ({})", session.id, session.dir.display());
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("fs-agent: cannot read the session store: {error}");
                ExitCode::FAILURE
            }
        };
    }

    match store.prune(&cwd, parsed.keep) {
        Ok(removed) => {
            for session in removed {
                println!("removed {} ({})", session.id, session.dir.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("fs-agent: cannot prune the session store: {error}");
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
                    .ok_or_else(|| format!("{arg} needs a value"))?;
                match arg {
                    "--round" => {
                        parsed.round = Some(
                            value
                                .parse()
                                .map_err(|_| format!("--round needs a number, got {value:?}"))?,
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
                                .map_err(|_| format!("--limit needs a number, got {value:?}"))?,
                        )
                    }
                    _ => unreachable!(),
                }
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown sessions argument {other:?}"))
            }
            other if parsed.verb.is_empty() => parsed.verb = other.to_owned(),
            other if parsed.id.is_none() => parsed.id = Some(other.to_owned()),
            other => return Err(format!("unexpected extra argument {other:?}")),
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
            let _ = writeln!(err, "fs-agent: sessions needs a verb");
            print_sessions_help(err);
            return ExitCode::FAILURE;
        }
        other => {
            let _ = writeln!(err, "fs-agent: unknown sessions verb {other:?}");
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
    let root = config::sessions_dir(env).ok_or_else(|| {
        "neither XDG_DATA_HOME nor HOME is set, so the session store cannot be found".to_owned()
    })?;
    Ok(SessionStore::new(root))
}

/// The workspace whose bucket is searched first: `--cwd`, else the current dir.
fn search_cwd(parsed: &SessionsArgs) -> Result<PathBuf, String> {
    match &parsed.cwd {
        Some(path) => Ok(path.clone()),
        None => std::env::current_dir()
            .map_err(|error| format!("cannot determine the current directory: {error}")),
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
        .ok_or_else(|| format!("no session {id:?} in {}", store_list_label(cwd)))
}

fn store_list_label(cwd: &Path) -> String {
    format!("{} (or any other bucket)", cwd.display())
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
        .map_err(|error| format!("cannot read the session store: {error}"))?;
    let mut listings: Vec<Listing> = listings;
    if let Some(limit) = parsed.limit {
        listings.truncate(limit);
    }

    if parsed.json {
        return write_json(out, &listings);
    }
    if listings.is_empty() {
        let _ = writeln!(err, "fs-agent: no sessions in this bucket");
        return Ok(());
    }
    let _ = writeln!(
        out,
        "{:<36}  {:<28}  {:<20}  {:>10}  {:>6}  {:>5}  ENDED",
        "ID", "CWD", "STARTED", "TOKENS", "ROUNDS", "MSGS"
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
                .map(|reason| reason.as_str())
                .unwrap_or("(open)"),
        );
    }
    Ok(())
}

fn sessions_show(parsed: &SessionsArgs, env: &EnvMap, out: &mut dyn Write) -> Result<(), String> {
    let store = open_store(env)?;
    let cwd = search_cwd(parsed)?;
    let id = parsed
        .id
        .as_deref()
        .ok_or("sessions show needs a session id")?;
    let session = find_session(&store, &cwd, id)?;
    let events = read_events(&session.log_path)
        .map_err(|error| format!("cannot read {}: {error}", session.log_path.display()))?;

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
                    .map(|round| format!("round {round}"))
                    .unwrap_or_else(|| "session".to_owned()),
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
        .ok_or("sessions replay needs a session id")?;
    let speaker = parsed
        .speaker
        .as_deref()
        .ok_or("sessions replay needs --speaker")?;
    let session = find_session(&store, &cwd, id)?;
    let events = read_events(&session.log_path)
        .map_err(|error| format!("cannot read {}: {error}", session.log_path.display()))?;

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
        .ok_or("sessions stats needs a session id")?;
    let session = find_session(&store, &cwd, id)?;
    let events = read_events(&session.log_path)
        .map_err(|error| format!("cannot read {}: {error}", session.log_path.display()))?;

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
                let _ = writeln!(
                    out,
                    "\n── round {round}{} ──",
                    group
                        .mode
                        .map(|mode| format!(" ({mode:?})"))
                        .unwrap_or_default()
                );
            }
            None => {
                let _ = writeln!(out, "\n── session ──");
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
    match entry {
        Entry::Message { speaker, text, .. } => format!("[{speaker}] {text}"),
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
            let mut lines = vec![format!("[{speaker}] → {tool}({args})")];
            match (ok, output, error) {
                (Some(true), Some(output), _) => {
                    lines.push(indent(&truncate_preview(output), 2));
                }
                (Some(false), _, error) => {
                    lines.push(indent(
                        &format!("error: {}", error.as_deref().unwrap_or("(no message)")),
                        2,
                    ));
                }
                _ => lines.push(indent("(no result on the stream)", 2)),
            }
            if let Some(hook) = hook {
                lines.push(indent(&format!("[hook] {hook}"), 2));
            }
            lines.join("\n")
        }
        Entry::RoundStarted { round, mode } => format!("round {round} started ({mode:?})"),
        Entry::RoundEnded { round, reason } => format!("[round {round} ended: {reason}]"),
        Entry::SessionEnded { reason } => format!("[session ended: {reason}]"),
        Entry::TurnStarted { speaker, iteration } => {
            format!("[{speaker}] turn iteration {iteration}")
        }
        Entry::TurnEnded { speaker, reason } => format!("[{speaker}] turn ended: {reason}"),
        Entry::Divergence {
            topic, positions, ..
        } => {
            let mut lines = vec![format!("!! divergence: {topic}")];
            for position in positions {
                lines.push(indent(&format!("- {position}"), 2));
            }
            lines.join("\n")
        }
        Entry::PermissionAsked {
            speaker,
            request_id,
            ..
        } => format!("[{speaker}] permission asked ({request_id})"),
        Entry::PermissionDecided {
            speaker,
            decision,
            source,
            reason,
            ..
        } => format!(
            "[{speaker}] permission {} ({}){}",
            decision.as_str(),
            decision_source(source),
            reason
                .as_deref()
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default()
        ),
        Entry::Hook {
            speaker,
            point,
            outcome,
            ..
        } => format!("[{speaker}] hook {point}: {outcome}"),
        Entry::ExecutorSpawned {
            speaker,
            executor_id,
            ..
        } => format!("[{speaker}] dispatched executor {executor_id}"),
        Entry::ExecutorFinished {
            executor_id,
            reason,
            summary,
            ..
        } => format!("[executor {executor_id}] finished: {reason} — {summary}"),
        Entry::Usage { speaker, usage } => format!(
            "[{speaker}] usage in={} out={} cached={} miss={}",
            usage.input_tokens, usage.output_tokens, usage.cached_tokens, usage.miss_tokens
        ),
        Entry::AgentError {
            speaker, message, ..
        } => format!("[{speaker}] error: {message}"),
        Entry::SessionError { code, detail, .. } => format!("[session error {code}] {detail}"),
        Entry::History {
            reason, summary, ..
        } => format!(
            "[history: {reason:?}] {}",
            summary.as_deref().unwrap_or("(superseded)")
        ),
        Entry::Context { source, .. } => format!("[context injected: {source:?}]"),
    }
}

fn decision_source(source: &crate::events::DecisionSource) -> &'static str {
    match source {
        crate::events::DecisionSource::User => "user",
        crate::events::DecisionSource::Hook => "hook",
        crate::events::DecisionSource::Policy => "policy",
    }
}

fn indent(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    text.lines()
        .map(|line| format!("{pad}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_preview(text: &str) -> String {
    const MAX: usize = 500;
    let mut preview: String = text.chars().take(MAX).collect();
    if text.chars().count() > MAX {
        preview.push('…');
    }
    preview
}

fn print_messages(out: &mut dyn Write, messages: &[Message]) {
    for message in messages {
        match message {
            Message::System { content, .. } => {
                let _ = writeln!(out, "[system]\n{content}\n");
            }
            Message::User { content, name, .. } => {
                let label = name
                    .as_deref()
                    .map(|name| format!(" user:{name}"))
                    .unwrap_or_default();
                let _ = writeln!(out, "[{label}]\n{content}\n");
            }
            Message::Assistant {
                content,
                tool_calls,
                ..
            } => {
                let _ = writeln!(out, "[assistant]");
                if let Some(content) = content {
                    let _ = writeln!(out, "{content}");
                }
                for call in tool_calls {
                    let _ = writeln!(out, "→ {}({})", call.name, call.arguments);
                }
                let _ = writeln!(out);
            }
            Message::Tool {
                tool_call_id,
                content,
            } => {
                let _ = writeln!(out, "[tool {tool_call_id}]\n{content}\n");
            }
        }
    }
}

fn print_stats(out: &mut dyn Write, stats: &observe::Stats, model: &str) {
    let _ = writeln!(
        out,
        "session: {} tokens, {} calls, {} messages, {} rounds{}",
        stats.session.tokens.total_tokens(),
        stats.session.calls,
        stats.session.messages,
        stats.session.rounds,
        stats
            .session
            .cost
            .map(|cost| format!(", ${cost:.6} (priced at {model})"))
            .unwrap_or_else(|| format!(", no cost (no [pricing.{model}] entry)")),
    );
    for speaker in &stats.speakers {
        let _ = writeln!(
            out,
            "  {:<16} {} tokens  {} calls  hit {}{}",
            speaker.speaker,
            speaker.tokens.total_tokens(),
            speaker.calls,
            speaker
                .hit_rate
                .map(|rate| format!("{:.0}%", rate * 100.0))
                .unwrap_or_else(|| "-".to_owned()),
            speaker
                .cost
                .map(|cost| format!("  ${cost:.6}"))
                .unwrap_or_default(),
        );
    }
    let _ = writeln!(
        out,
        "absence: {}/{} debate rounds one-sided{}",
        stats.absence.one_sided,
        stats.absence.rounds,
        stats
            .absence
            .rate
            .map(|rate| format!(" ({:.0}%)", rate * 100.0))
            .unwrap_or_default(),
    );
    for agent in &stats.absence.per_speaker {
        let _ = writeln!(out, "  absent: {} x{}", agent.name, agent.count);
    }
    let _ = writeln!(
        out,
        "edits: {} succeeded, {} failed matches",
        stats.edits.succeeded, stats.edits.failed_matches
    );
    for level in &stats.edits.levels {
        let _ = writeln!(out, "  match level {}: {}", level.name, level.count);
    }
    let _ = writeln!(
        out,
        "guards: read-before-write {}, read-set invalidations {}",
        stats.guards.read_before_write, stats.guards.invalidated_reads
    );
    let _ = writeln!(
        out,
        "executors: {} spawned, {} finished",
        stats.executors.spawned, stats.executors.finished
    );
    for reason in &stats.executors.by_reason {
        let _ = writeln!(out, "  finished {}: {}", reason.name, reason.count);
    }
    let _ = writeln!(
        out,
        "hooks: {} executed ({} pre, {} post), {} feedback, {} failed",
        stats.hooks.executed,
        stats.hooks.pre,
        stats.hooks.post,
        stats.hooks.feedback,
        stats.hooks.failed
    );
    let decisions = stats
        .permissions
        .decided
        .iter()
        .map(|count| format!("{} {}", count.count, count.name))
        .collect::<Vec<_>>()
        .join(", ");
    let decided = if decisions.is_empty() {
        String::new()
    } else {
        format!(", {decisions} decided")
    };
    let _ = writeln!(
        out,
        "permissions: {} asked{decided}",
        stats.permissions.asked,
    );
    let _ = writeln!(
        out,
        "divergences: {}/{}{}; rounds: {}",
        stats.divergences,
        stats.absence.rounds,
        stats
            .divergence_rate
            .map(|rate| format!(" ({:.0}%)", rate * 100.0))
            .unwrap_or_default(),
        stats
            .rounds
            .iter()
            .map(|round| format!(
                "#{} {} {} calls{}",
                round.round,
                round.mode_str(),
                round.calls,
                round
                    .ended
                    .map(|reason| format!(" -> {reason}"))
                    .unwrap_or_default()
            ))
            .collect::<Vec<_>>()
            .join("; ")
    );
    for stop in &stats.stops {
        let _ = writeln!(out, "  stop {}: {}", stop.name, stop.count);
    }
}

trait RoundModeLabel {
    fn mode_str(&self) -> &'static str;
}

impl RoundModeLabel for observe::RoundStats {
    fn mode_str(&self) -> &'static str {
        match self.mode {
            crate::events::RoundMode::Independent => "independent",
            crate::events::RoundMode::Targeted => "targeted",
            crate::events::RoundMode::Synthesis => "synthesis",
        }
    }
}

fn print_sessions_help(out: &mut dyn Write) {
    let _ = writeln!(
        out,
        "fs-agent sessions <verb> [options]\n\n  \
         ls [--all] [--cwd PATH] [--limit N] [--json]\n      \
         List this workspace's sessions (--all scans every bucket), newest first.\n  \
         show <id> [--round N] [--speaker X] [--kind K] [--tool T] [--only-error] [--files] [--json]\n      \
         The round-grouped transcript, tool calls merged with their results; --files\n      \
         shows the workspace-object view instead (it honors --round/--speaker/--tool).\n  \
         replay <id> --speaker X [--round N] [--model ID] [--json]\n      \
         Recompute what one call sent to the provider, from the stream alone.\n  \
         stats <id> [--model ID] [--json]\n      \
         The fixed metric set (tokens, cost, absence rate, edit-ladder downgrades).\n\n  \
         stdout carries the result and stderr the diagnostics."
    );
}

fn print_help() {
    println!(
        "fs-agent {}\n\n  \
         usage: fs-agent [--plain|--tui] [--continue] [--config PATH] [--model ID] [--cwd PATH]\n         \
         fs-agent probe [--config PATH] [--model ID]...\n         \
         fs-agent prune [--keep N] [--cwd PATH] [--dry-run]\n         \
         fs-agent sessions <ls|show|replay|stats> [options]\n\n  \
         With no subcommand, `fs-agent` starts an interactive session in the \
         current workspace: it renders with the TUI on a terminal and with the \
         plain transcript otherwise (--plain / --tui force one). `--continue` \
         resumes this workspace's newest session. `probe` drives one real turn \
         against each configured model and a second turn in the same session, \
         then prints the normalized usage so you can see prefix caching hit. \
         `prune` removes this workspace's session directories, keeping the \
         newest N (default 1). `sessions` answers questions about a finished \
         session from its own event stream (ls / show / replay / stats; see \
         `fs-agent sessions --help`). Configuration lives in \
         ~/.config/fs-agent/config.toml (XDG aware); a project .env is never loaded.",
        env!("CARGO_PKG_VERSION")
    );
}

fn print_interactive_help() {
    println!(
        "fs-agent [options]\n\n  \
         Starts an interactive session in the current workspace. Commands: \
         /undo rolls back the last edit, /plan and /endplan control the hard plan \
         mode, /quit leaves. Esc cancels the running turn in the TUI; Shift+Tab \
         toggles plan mode.\n\n  \
         --plain            the plain transcript (no raw mode)\n  \
         --tui              the terminal interface (inline viewport)\n  \
         --continue, -c     resume this workspace's newest session\n  \
         --config PATH      configuration file to load\n  \
         --model ID         the model to run (default: config default_model)\n  \
         --cwd PATH         the workspace (default: the current directory)"
    );
}

fn print_probe_help() {
    println!(
        "fs-agent probe [--config PATH] [--model ID]...\n\n  \
         Sends two real turns per model in one session and prints input/output/cached/miss \
         for each. Without --model it probes every model whose provider has a key."
    );
}

fn print_prune_help() {
    println!(
        "fs-agent prune [--keep N] [--cwd PATH] [--dry-run]\n\n  \
         Removes session directories for one workspace (the current directory, \
         or --cwd). The newest N sessions are kept (default 1: the one \
         `--continue` would resume). A session is a directory, so removal is \
         whole-session; --dry-run lists what would go. Nothing else ever \
         deletes sessions."
    );
}
