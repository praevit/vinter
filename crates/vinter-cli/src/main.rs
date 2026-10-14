//! `vinter`: a thin CLI over `vinter-core`.

use std::io::Write;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use vinter_core::AppPaths;
use vinter_core::runtime::{self, InstallProgress, InstallStage};
use vinter_core::system::{CheckStatus, rosetta, system_checks};

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
    /// Manage Wine runtimes.
    #[command(subcommand)]
    Runtime(RuntimeCommand),
    /// Install Rosetta 2 (Apple Silicon only). Running this accepts Apple's license.
    InstallRosetta,
}

#[derive(Subcommand)]
enum RuntimeCommand {
    /// List installed runtimes and the ones available to install.
    List,
    /// Download, verify and install a runtime.
    Install { id: String },
    /// Delete an installed runtime.
    Remove { id: String },
    /// Run a program with a runtime's bin folder first on PATH,
    /// e.g. `vinter runtime exec <id> -- wine --version`.
    Exec {
        id: String,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = AppPaths::discover()?;
    match cli.command {
        Command::Check { json } => check(&paths, json),
        Command::Runtime(cmd) => match cmd {
            RuntimeCommand::List => runtime_list(&paths),
            RuntimeCommand::Install { id } => runtime_install(&paths, &id).await,
            RuntimeCommand::Remove { id } => {
                runtime::remove(&paths, &id)?;
                println!("Removed {id}.");
                Ok(())
            }
            RuntimeCommand::Exec { id, command } => runtime_exec(&paths, &id, &command),
        },
        Command::InstallRosetta => {
            println!("Installing Rosetta 2. macOS may ask for an administrator password.");
            rosetta::install()?;
            println!("Rosetta 2 is installed.");
            Ok(())
        }
    }
}

fn check(paths: &AppPaths, json: bool) -> Result<()> {
    let status = system_checks(paths);
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

fn runtime_list(paths: &AppPaths) -> Result<()> {
    let installed = runtime::installed(paths)?;
    println!("Installed:");
    if installed.is_empty() {
        println!("  (none)");
    }
    for r in &installed {
        println!("  {:<28} {:<20} {}", r.id, r.name, mb(r.size));
    }
    println!("\nAvailable:");
    for r in runtime::manifest::available() {
        let mark = if installed.iter().any(|i| i.id == r.id) {
            " (installed)"
        } else {
            ""
        };
        println!("  {:<28} {:<20} {}{mark}", r.id, r.name, mb(r.size));
    }
    Ok(())
}

async fn runtime_install(paths: &AppPaths, id: &str) -> Result<()> {
    let info = runtime::manifest::find(id)?;
    println!("Installing {} from {}", info.name, info.url);
    let mut last_stage = None;
    let rt = runtime::install(paths, &info, |p: InstallProgress| {
        if last_stage != Some(p.stage) {
            if last_stage.is_some() {
                println!();
            }
            last_stage = Some(p.stage);
        }
        print_progress(p);
    })
    .await?;
    println!("\nInstalled {} at {}", rt.id, rt.path.display());
    Ok(())
}

fn print_progress(p: InstallProgress) {
    let line = match (p.stage, p.fraction()) {
        (InstallStage::Finishing, _) => p.stage.label().to_string(),
        (_, Some(f)) => format!(
            "{:<20} {:>5.1}%  {}",
            p.stage.label(),
            f * 100.0,
            mb(p.done)
        ),
        (_, None) => format!("{:<20} {}", p.stage.label(), mb(p.done)),
    };
    print!("\r{line:<60}");
    let _ = std::io::stdout().flush();
}

fn runtime_exec(paths: &AppPaths, id: &str, command: &[String]) -> Result<()> {
    let rt = runtime::get(paths, id)?;
    if rosetta::is_needed() && !rosetta::is_installed() {
        anyhow::bail!(
            "Rosetta 2 is not installed, and Wine needs it on Apple Silicon. \
             Install it with `vinter install-rosetta`."
        );
    }
    let (program, args) = command.split_first().context("no command given")?;
    let status = rt
        .command(program)
        .args(args)
        .status()
        .with_context(|| format!("could not run {program}"))?;
    std::process::exit(status.code().unwrap_or(1));
}

fn mb(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_000_000.0)
}
