//! `vinter`: a thin CLI over `vinter-core`.

use anyhow::Result;
use clap::{Parser, Subcommand};
use vinter_core::AppPaths;
use vinter_core::system::{CheckStatus, system_checks};

#[derive(Parser)]
#[command(
    name = "vinter",
    version,
    about = "Run Windows apps on macOS and Linux"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check whether this machine is ready to run Vinter.
    Check {
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Check { json } => check(json),
    }
}

fn check(json: bool) -> Result<()> {
    let status = system_checks(&AppPaths::discover()?);
    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!(
            "Vinter {} on {} ({})",
            status.version, status.os, status.arch
        );
        println!("Data folder: {}\n", status.data_dir);
        for c in &status.checks {
            let mark = match c.status {
                CheckStatus::Ok => "ok  ",
                CheckStatus::Warning => "warn",
                CheckStatus::Error => "FAIL",
                CheckStatus::Skipped => "skip",
            };
            println!("[{mark}] {:<12} {}", c.label, c.detail);
        }
    }
    if !status.is_ready() {
        std::process::exit(1);
    }
    Ok(())
}
