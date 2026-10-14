//! `vinter`: a thin CLI over `vinter-core`.

use std::io::Write;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use vinter_core::AppPaths;
use vinter_core::bottle::{self, Graphics, NewBottle, Tool, WindowsVersion};
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
    /// Manage bottles (Wine prefixes). Bottles can be named by id, id prefix or name.
    #[command(subcommand)]
    Bottle(BottleCommand),
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

#[derive(Subcommand)]
enum BottleCommand {
    /// List bottles.
    List,
    /// Create a bottle and set up its Wine prefix.
    Create {
        name: String,
        /// Runtime id (see `vinter runtime list`).
        #[arg(long)]
        runtime: String,
        /// win11, win10, win81, win8, win7 or winxp64.
        #[arg(long, default_value = "win10", value_parser = parse_windows_version)]
        windows: WindowsVersion,
    },
    /// Show a bottle's settings.
    Show { bottle: String },
    /// Rename a bottle.
    Rename { bottle: String, name: String },
    /// Set the Windows version.
    SetWindows {
        bottle: String,
        #[arg(value_parser = parse_windows_version)]
        version: WindowsVersion,
    },
    /// Switch the runtime a bottle uses.
    SetRuntime { bottle: String, runtime: String },
    /// Copy a bottle under a new name.
    Duplicate { bottle: String, name: String },
    /// Move a bottle to the Trash.
    Delete { bottle: String },
    /// Open a built-in tool: winecfg, regedit, taskmgr, control, notepad, explorer.
    Tool {
        bottle: String,
        #[arg(value_parser = parse_tool)]
        tool: Tool,
    },
    /// Stop every Wine process in a bottle.
    Kill { bottle: String },
    /// Print the path of the bottle's C: drive.
    DriveC { bottle: String },
}

fn parse_windows_version(s: &str) -> Result<WindowsVersion, String> {
    WindowsVersion::parse(s).ok_or_else(|| {
        let all: Vec<_> = WindowsVersion::ALL
            .iter()
            .map(|v| v.winecfg_name())
            .collect();
        format!("expected one of {}", all.join(", "))
    })
}

fn parse_tool(s: &str) -> Result<Tool, String> {
    Tool::parse(s).ok_or_else(|| {
        let all: Vec<_> = Tool::ALL.iter().map(|t| t.program()).collect();
        format!("expected one of {}", all.join(", "))
    })
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
        Command::Bottle(cmd) => bottle_command(&paths, cmd),
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
    if rt.check_can_run().is_err() {
        anyhow::bail!(
            "{} needs Rosetta 2, which is not installed. \
             Install it with `vinter install-rosetta`.",
            rt.name
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

fn bottle_command(paths: &AppPaths, cmd: BottleCommand) -> Result<()> {
    let id = |key: &str| bottle::resolve(paths, key).map(|b| b.id);
    match cmd {
        BottleCommand::List => {
            let bottles = bottle::list(paths)?;
            if bottles.is_empty() {
                println!(
                    "No bottles yet. Create one with `vinter bottle create <name> --runtime <id>`."
                );
            }
            for b in bottles {
                let state = if b.setup_complete {
                    ""
                } else {
                    "  (setup incomplete)"
                };
                println!(
                    "{}  {:<24} {:<28} {}{state}",
                    &b.id[..8],
                    b.name,
                    b.runtime,
                    b.windows_version.winecfg_name()
                );
            }
        }
        BottleCommand::Create {
            name,
            runtime,
            windows,
        } => {
            let new = NewBottle {
                name,
                runtime,
                windows_version: windows,
                graphics: Graphics::default(),
            };
            let b = bottle::create(paths, new, |step| println!("{step}…"))?;
            println!("Created {} ({})", b.name, b.id);
        }
        BottleCommand::Show { bottle: key } => {
            let b = bottle::get(paths, &id(&key)?)?;
            println!("{}", serde_json::to_string_pretty(&b)?);
        }
        BottleCommand::Rename { bottle: key, name } => {
            let b = bottle::rename(paths, &id(&key)?, &name)?;
            println!("Renamed to {}.", b.name);
        }
        BottleCommand::SetWindows {
            bottle: key,
            version,
        } => {
            bottle::set_windows_version(paths, &id(&key)?, version)?;
            println!("Windows version set to {}.", version.winecfg_name());
        }
        BottleCommand::SetRuntime {
            bottle: key,
            runtime,
        } => {
            bottle::set_runtime(paths, &id(&key)?, &runtime)?;
            println!("Runtime set to {runtime}.");
        }
        BottleCommand::Duplicate { bottle: key, name } => {
            let b = bottle::duplicate(paths, &id(&key)?, &name)?;
            println!("Created {} ({})", b.name, b.id);
        }
        BottleCommand::Delete { bottle: key } => {
            bottle::delete(paths, &id(&key)?)?;
            println!("Moved to the Trash.");
        }
        BottleCommand::Tool { bottle: key, tool } => {
            let pid = bottle::run_tool(paths, &id(&key)?, tool)?;
            println!("Started {} (pid {pid}).", tool.program());
        }
        BottleCommand::Kill { bottle: key } => {
            bottle::kill_all(paths, &id(&key)?)?;
            println!("Stopped all Wine processes.");
        }
        BottleCommand::DriveC { bottle: key } => {
            println!("{}", bottle::drive_c(paths, &id(&key)?)?.display());
        }
    }
    Ok(())
}

fn mb(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_000_000.0)
}
