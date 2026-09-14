//! CLI boundary: the only place that reads the environment.
//!
//! The binary is a thin shell around this module. Everything the library needs
//! is assembled here from `config.toml`, the exported environment and the
//! built-in defaults, then injected into [`crate::assemble`].
//!
//! `probe` is the ticket-02 manual acceptance tool: it drives one real turn
//! against each configured model, then a second turn in the same session, so
//! the usage line for the second request shows whether prefix caching hit. It
//! runs headless with throwaway session logs and does not touch the session
//! store (ticket 12) or the interactive renderers (ticket 18).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::config::{self, Config, EnvMap, SessionConfig};
use crate::events::{read_events, EventPayload, SessionId, SpeakerId, Usage};
use crate::provider::capability::caps_for;
use crate::provider::openai::{stderr_warnings, BuildError, OpenAiProvider};
use crate::render::RenderSinks;
use crate::{assemble, AssemblyParts};

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

/// Parse `argv` from the environment and run. This is the binary entry point.
pub fn main() -> ExitCode {
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
        Some("--help") | Some("-h") | None => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("probe") => probe(&args[1..], env).await,
        Some(other) => {
            eprintln!("fs-agent: unknown argument {other:?}");
            print_help();
            ExitCode::FAILURE
        }
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
    for model in models {
        match probe_model(&config, &model).await {
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

async fn probe_model(config: &Config, model_id: &str) -> Result<(), ProbeError> {
    let (model, profile) = config
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

    let session_config = SessionConfig::new(model_id).with_params(model.params.clone());
    let mut harness = assemble(AssemblyParts {
        provider: Box::new(provider),
        speaker: SpeakerId::Debater(profile.name.clone().into()),
        cwd: dir,
        log_path: log_path.clone(),
        session_id: SessionId::new(format!("probe-{model_id}")),
        config: session_config,
        sinks: RenderSinks {
            // The probe prints its own report on stdout; the renderer narrates
            // to stderr only.
            stdout_result: Box::new(std::io::sink()),
            stderr_diagnostic: Box::new(std::io::stderr()),
        },
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
        match last_usage(&log_path) {
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
    }
    harness.shutdown().await;
    Ok(())
}

fn last_usage(log_path: &Path) -> Option<Usage> {
    let events = read_events(log_path).ok()?;
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

fn print_help() {
    println!(
        "fs-agent {}\n\n  \
         usage: fs-agent [--help] [--version]\n         \
         fs-agent probe [--config PATH] [--model ID]...\n\n  \
         The interactive renderers are not wired into this build yet. \
         `probe` drives one real turn against each configured model and a second \
         turn in the same session, then prints the normalized usage so you can \
         see prefix caching hit. Configuration lives in \
         ~/.config/fs-agent/config.toml (XDG aware); a project .env is never loaded.",
        env!("CARGO_PKG_VERSION")
    );
}

fn print_probe_help() {
    println!(
        "fs-agent probe [--config PATH] [--model ID]...\n\n  \
         Sends two real turns per model in one session and prints input/output/cached/miss \
         for each. Without --model it probes every model whose provider has a key."
    );
}
