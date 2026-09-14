//! CLI boundary: the only place that reads the environment.
//!
//! The binary is a thin shell around this module. Real provider assembly
//! (`config.toml`, API keys, renderer selection) lands in ticket 02; until then
//! there is nothing to run headlessly, so this reports that plainly instead of
//! failing mysteriously.

use std::process::ExitCode;

/// Parse `argv` from the environment and run. This is the binary entry point.
pub fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version") | Some("-V") => {
            println!("fs-agent {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help") | Some("-h") | None => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("fs-agent: unknown argument {other:?}");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    eprintln!(
        "fs-agent {}\n\n  \
         usage: fs-agent [--help] [--version]\n\n  \
         Provider configuration and the interactive renderers are not wired into\n  \
         this build yet; the library assembly entry (`fs_agent::assemble`) is the\n  \
         working seam.",
        env!("CARGO_PKG_VERSION")
    );
}
