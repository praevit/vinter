//! `vinter`: a thin CLI over `vinter-core`.

use std::io::Write;
use std::path::PathBuf;

mod mcp;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::json;
use vinter_core::AppPaths;
use vinter_core::bottle::{self, NewBottle, Tool, WindowsVersion};
use vinter_core::doctor::{Fix, Severity};
use vinter_core::experimental::{self, ExperimentalFlag};
use vinter_core::graphics::{self, Backend};
use vinter_core::process::logs;
use vinter_core::programs;
use vinter_core::recipe::{self, ApplyOptions, RecipeProgress};
use vinter_core::runtime::{self, InstallProgress, InstallStage};
use vinter_core::system::readiness::ReadinessLevel;
use vinter_core::system::{CheckStatus, rosetta, system_checks};

#[derive(Parser)]
#[command(
    name = "vinter",
    version,
    about = "Run Windows apps on macOS and Linux"
)]
struct Cli {
    /// Print one JSON value on stdout instead of text (errors as `{"error": ...}`).
    #[arg(long, global = true)]
    json: bool,
    /// Turn an experimental feature on for this command only (repeatable).
    #[arg(long, global = true, value_name = "ID", value_parser = parse_flag)]
    experimental: Vec<ExperimentalFlag>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check whether this machine is ready to run Vinter.
    Check,
    /// Manage Wine runtimes.
    #[command(subcommand)]
    Runtime(RuntimeCommand),
    /// Manage bottles (Wine prefixes). Bottles can be named by id, id prefix or name.
    #[command(subcommand)]
    Bottle(BottleCommand),
    /// Windows components (Visual C++, .NET, DirectX parts, fonts) for a bottle.
    #[command(subcommand)]
    Component(ComponentCommand),
    /// Snapshots of a bottle, to roll back to. Snapshots can be named by id or id prefix.
    #[command(subcommand)]
    Snapshot(SnapshotCommand),
    /// App recipes: list them, check one, or set up an app from one.
    #[command(subcommand)]
    Recipe(RecipeCommand),
    /// Serve the Model Context Protocol on stdio, for AI agents. Changes they
    /// ask for must be allowed in the Vinter app.
    Mcp,
    /// Install Rosetta 2 (Apple Silicon only). Running this accepts Apple's license.
    InstallRosetta,
    /// Apple's D3DMetal, from your own copy of the Game Porting Toolkit.
    #[command(subcommand)]
    D3dmetal(D3dmetalCommand),
    /// Experimental features: off by default, and may be rough.
    #[command(subcommand)]
    Experimental(ExperimentalCommand),
    /// Diagnostics for bug reports.
    #[command(subcommand)]
    Diag(DiagCommand),
    /// Your own compatibility notes per app (experimental: compat-notes).
    #[command(subcommand)]
    Notes(NotesCommand),
    /// Show which CPU a Windows program (.exe, .dll) or installer (.msi) is
    /// built for, and how it would run in a bottle.
    Arch {
        file: PathBuf,
        /// Say how it runs in this bottle.
        #[arg(long)]
        bottle: Option<String>,
    },
}

#[derive(Subcommand)]
enum NotesCommand {
    /// List every note.
    List,
    /// Rate a bottle's program (by number from `bottle programs`).
    Set {
        bottle: String,
        program: usize,
        /// works, partly or broken.
        #[arg(value_parser = parse_rating)]
        rating: vinter_core::compat::Rating,
        /// What works or doesn't.
        #[arg(long, default_value = "")]
        note: String,
    },
    /// Remove a program's note.
    Remove { bottle: String, program: usize },
    /// Write every note as JSON.
    Export { file: PathBuf },
}

#[derive(Subcommand)]
enum DiagCommand {
    /// Zip a bottle's logs, settings and system checks (no documents from its
    /// C: drive), with your home folder hidden.
    Export {
        bottle: String,
        /// Where to write the zip: a folder (default: the current one) or a file name.
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ExperimentalCommand {
    /// List every experimental feature with its id, state, risk and description.
    List,
    /// Turn a feature on (saved, like the switch in Settings).
    Enable {
        #[arg(value_parser = parse_flag)]
        id: ExperimentalFlag,
    },
    /// Turn a feature off.
    Disable {
        #[arg(value_parser = parse_flag)]
        id: ExperimentalFlag,
    },
}

#[derive(Subcommand)]
enum D3dmetalCommand {
    /// Show whether D3DMetal is imported, and which version.
    Status,
    /// Import it from the Game Porting Toolkit (or its evaluation environment):
    /// a .dmg or an opened folder. Shows Apple's license; --accept agrees to it.
    Import {
        source: std::path::PathBuf,
        #[arg(long)]
        accept: bool,
    },
    /// Remove the imported copy.
    Remove,
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
    /// e.g. `vinter runtime exec <id> -- wine --version`. Its output is passed
    /// through as is, even with --json.
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
        /// Runtime id (see `vinter runtime list`). Default: the one chosen in
        /// Settings, else the newest Apple Silicon runtime this Mac can run,
        /// else the newest one.
        #[arg(long)]
        runtime: Option<String>,
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
    /// Run an installer (.exe or .msi) in a bottle, then list what it installed.
    Install {
        bottle: String,
        installer: std::path::PathBuf,
        /// Extra installer arguments, e.g. `-- /S` for a silent install.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// List programs installed in a bottle that can be added to it.
    Discover {
        bottle: String,
        /// Add every program found that is not added yet.
        #[arg(long)]
        add: bool,
    },
    /// List a bottle's programs.
    Programs { bottle: String },
    /// Start a program (by number from `programs`) and wait for it to exit.
    Run { bottle: String, program: usize },
    /// List a bottle's logs, or print one.
    Logs { bottle: String, log: Option<String> },
    /// Look for known problems in a log (the newest one by default) and say how to fix them.
    Doctor { bottle: String, log: Option<String> },
    /// Show the graphics backends, or switch to one (downloading it if needed).
    Graphics {
        bottle: String,
        /// wined3d, dxvk, dxmt or d3dmetal.
        #[arg(value_parser = parse_backend)]
        backend: Option<Backend>,
    },
    /// Run a small Direct3D 11 program in the bottle and report what answered.
    TestGraphics { bottle: String },
    /// Show or change display, keyboard and overlay options.
    Options {
        bottle: String,
        /// macOS: full Retina resolution.
        #[arg(long)]
        retina: Option<bool>,
        /// macOS: ⌘ works as Ctrl.
        #[arg(long)]
        command_as_ctrl: Option<bool>,
        /// macOS: ⌥ works as Alt (off keeps ⌥ for typing characters like @).
        #[arg(long)]
        option_as_alt: Option<bool>,
        /// Frame rate overlay in games.
        #[arg(long)]
        fps: Option<bool>,
        /// How much Wine logs: off, errors or verbose.
        #[arg(long, value_parser = parse_wine_debug)]
        wine_debug: Option<bottle::WineDebug>,
    },
    /// Set environment variables (KEY=VALUE), or remove them (KEY=).
    Env { bottle: String, vars: Vec<String> },
    /// Set DLL overrides (dll=mode, e.g. d3d11=n,b), or remove them (dll).
    Override {
        bottle: String,
        overrides: Vec<String>,
    },
    /// Rename a program, set its launch arguments or its icon.
    EditProgram {
        bottle: String,
        program: usize,
        #[arg(long)]
        name: Option<String>,
        /// Launch arguments (replaces the current ones); give none to clear.
        #[arg(long, num_args = 0.., allow_hyphen_values = true)]
        args: Option<Vec<String>>,
        /// An image (.png, .jpg, .ico) or .exe to take the icon from.
        #[arg(long)]
        icon: Option<std::path::PathBuf>,
    },
    /// Add a launcher for a program (~/Applications/Vinter on macOS, the app menu on Linux).
    Pin { bottle: String, program: usize },
    /// Remove a program's launcher.
    Unpin { bottle: String, program: usize },
    /// Print a debug report for bug reports.
    Report { bottle: String },
    /// Save a bottle as one .vinterbottle file.
    Backup {
        bottle: String,
        file: std::path::PathBuf,
    },
    /// Restore a .vinterbottle backup as a new bottle.
    Restore { file: std::path::PathBuf },
}

#[derive(Subcommand)]
enum ComponentCommand {
    /// List the components and which are installed in a bottle.
    List { bottle: String },
    /// Install components with winetricks, e.g. `vcrun2022 corefonts`.
    Install {
        bottle: String,
        #[arg(required = true)]
        verbs: Vec<String>,
        /// Agree to downloads from Microsoft and others, under their own licenses.
        #[arg(long)]
        accept_downloads: bool,
    },
}

#[derive(Subcommand)]
enum SnapshotCommand {
    /// List a bottle's snapshots, newest first.
    List { bottle: String },
    /// Snapshot a bottle. Close its programs first.
    Create {
        bottle: String,
        /// What it is for, e.g. "before mods".
        #[arg(long, default_value = "Snapshot")]
        label: String,
    },
    /// Put a bottle back as it was in a snapshot (the current state is
    /// snapshotted first). Stops the bottle's programs.
    Restore { bottle: String, snapshot: String },
    /// Delete a snapshot.
    Delete { bottle: String, snapshot: String },
}

#[derive(Subcommand)]
enum RecipeCommand {
    /// List recipes (built-in and your own).
    List,
    /// Show a recipe's steps.
    Show { id: String },
    /// Validate a recipe file and print any problems.
    Check { file: std::path::PathBuf },
    /// Create a bottle from a recipe and set the app up.
    Apply {
        id: String,
        /// Name of the new bottle (default: the recipe's name).
        #[arg(long)]
        name: Option<String>,
        /// Installer file (needed for user-provided installers).
        #[arg(long)]
        installer: Option<std::path::PathBuf>,
        /// Agree to winetricks downloads (may include Microsoft software).
        #[arg(long)]
        accept_downloads: bool,
    },
    /// Continue a recipe that stopped in a bottle.
    Resume {
        bottle: String,
        #[arg(long)]
        installer: Option<std::path::PathBuf>,
        #[arg(long)]
        accept_downloads: bool,
    },
}

/// Where command results go: text for people, or one JSON value with --json.
#[derive(Clone, Copy)]
struct Out {
    json: bool,
}

impl Out {
    /// Prints `value` as JSON, or calls `text` to print it for people.
    fn print<T: Serialize + ?Sized>(self, value: &T, text: impl FnOnce()) -> Result<()> {
        if self.json {
            println!("{}", serde_json::to_string_pretty(value)?);
        } else {
            text();
        }
        Ok(())
    }

    /// A status line for people. JSON output leaves it out.
    fn note(self, line: impl std::fmt::Display) {
        if !self.json {
            println!("{line}");
        }
    }
}

fn d3dmetal_command(paths: &AppPaths, out: Out, cmd: D3dmetalCommand) -> Result<()> {
    use vinter_core::graphics::d3dmetal;
    match cmd {
        D3dmetalCommand::Status => {
            let d = d3dmetal::installed(paths);
            out.print(&d, || match &d {
                Some(d) => println!("D3DMetal {} (imported {})", d.version, d.imported_at),
                None => println!("Not imported."),
            })
        }
        D3dmetalCommand::Import { source, accept } => {
            if !accept {
                let found = d3dmetal::inspect(&source)?;
                let value = json!({
                    "accepted": false,
                    "version": found.version,
                    "license": found.license,
                });
                return out.print(&value, || {
                    println!("{}\n", found.license.trim());
                    println!(
                        "This is Apple's license for D3DMetal {}. Run again with --accept to agree and import it.",
                        found.version
                    );
                });
            }
            let d = d3dmetal::import(paths, &source)?;
            out.print(&d, || println!("Imported D3DMetal {}.", d.version))
        }
        D3dmetalCommand::Remove => {
            d3dmetal::remove(paths)?;
            out.print(&json!({ "removed": true }), || println!("Removed."))
        }
    }
}

fn experimental_command(paths: &AppPaths, out: Out, cmd: ExperimentalCommand) -> Result<()> {
    match cmd {
        ExperimentalCommand::List => {
            let flags = experimental::list(paths);
            out.print(&flags, || {
                for f in &flags {
                    let state = match (f.enabled, f.saved) {
                        (true, true) => "on",
                        (true, false) => "on (this command)",
                        _ => "off",
                    };
                    let later = if f.available { "" } else { " (not built yet)" };
                    println!(
                        "{:<26} {:<18} {:<6} {}{later}",
                        f.flag.id(),
                        state,
                        f.risk.label(),
                        f.description
                    );
                }
            })
        }
        ExperimentalCommand::Enable { id } => {
            experimental::set(paths, id, true)?;
            if id.risk() == experimental::Risk::High {
                out.note(format_args!(
                    "Warning: {id} is high risk. Back up bottles you care about before using it."
                ));
            }
            if !id.available() {
                out.note(format_args!(
                    "{id} is not built yet, so this changes nothing for now."
                ));
            }
            out.print(&json!({ "flag": id, "enabled": true }), || {
                println!("Turned on {id}.")
            })
        }
        ExperimentalCommand::Disable { id } => {
            experimental::set(paths, id, false)?;
            out.print(&json!({ "flag": id, "enabled": false }), || {
                println!("Turned off {id}.")
            })
        }
    }
}

fn arch_command(
    paths: &AppPaths,
    out: Out,
    file: &std::path::Path,
    key: Option<&str>,
) -> Result<()> {
    use vinter_core::programs::arch;
    let pe = arch::detect(file).with_context(|| {
        format!(
            "{} is not a Windows program or installer Vinter can read",
            file.display()
        )
    })?;
    let run = match key {
        Some(key) => {
            let b = bottle::resolve(paths, key)?;
            Some(arch::run_path(pe, b.engine).map(|p| p.describe()))
        }
        None => None,
    };
    let value = json!({
        "arch": pe,
        "label": pe.label(),
        "runs_as": run.as_ref().map(|r| r.as_ref().ok()),
        "error": run.as_ref().and_then(|r| r.as_ref().err()),
    });
    out.print(&value, || {
        println!("{pe:?} ({})", pe.label());
        match &run {
            Some(Ok(how)) => println!("Runs as {how}."),
            Some(Err(why)) => println!("Cannot run in that bottle: {why}."),
            None => {}
        }
    })
}

fn parse_flag(s: &str) -> Result<ExperimentalFlag, String> {
    ExperimentalFlag::parse(s).ok_or_else(|| {
        let all: Vec<_> = experimental::all().iter().map(|f| f.id()).collect();
        format!("expected one of {}", all.join(", "))
    })
}

fn notes_command(paths: &AppPaths, out: Out, cmd: NotesCommand) -> Result<()> {
    use vinter_core::compat;
    match cmd {
        NotesCommand::List => {
            let notes = compat::list(paths)?;
            out.print(&notes, || {
                if notes.is_empty() {
                    println!("No notes yet.");
                }
                for n in &notes {
                    println!(
                        "{:<7} {:<24} {:<16} {:<9} {}",
                        n.rating.id(),
                        n.app,
                        n.engine.id(),
                        n.backend.id(),
                        n.note
                    );
                }
            })
        }
        NotesCommand::Set {
            bottle: key,
            program,
            rating,
            note,
        } => {
            let b = bottle::resolve(paths, &key)?;
            let n = compat::set(paths, &b.id, program, rating, &note)?;
            out.print(&n, || println!("Noted {} as {}.", n.app, n.rating.id()))
        }
        NotesCommand::Remove {
            bottle: key,
            program,
        } => {
            let b = bottle::resolve(paths, &key)?;
            compat::remove(paths, &b.id, program)?;
            out.print(&json!({ "removed": true }), || println!("Removed."))
        }
        NotesCommand::Export { file } => {
            compat::export(paths, &file)?;
            out.print(&json!({ "path": file }), || {
                println!("Wrote {}", file.display())
            })
        }
    }
}

fn parse_rating(s: &str) -> Result<vinter_core::compat::Rating, String> {
    vinter_core::compat::Rating::parse(s).ok_or_else(|| "expected works, partly or broken".into())
}

fn parse_wine_debug(s: &str) -> Result<bottle::WineDebug, String> {
    bottle::WineDebug::parse(s).ok_or_else(|| "expected off, errors or verbose".into())
}

fn parse_backend(s: &str) -> Result<Backend, String> {
    Backend::parse(s).ok_or_else(|| "expected wined3d, dxvk, dxmt or d3dmetal".into())
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
async fn main() {
    let cli = Cli::parse();
    let out = Out { json: cli.json };
    for flag in cli.experimental {
        experimental::enable_for_session(flag);
    }
    if let Err(e) = run(out, cli.command).await {
        if out.json {
            println!("{}", json!({ "error": format!("{e:#}") }));
        } else {
            eprintln!("Error: {e:?}");
        }
        std::process::exit(1);
    }
}

async fn run(out: Out, command: Command) -> Result<()> {
    let paths = AppPaths::discover()?;
    match command {
        Command::Check => check(&paths, out),
        Command::Runtime(cmd) => match cmd {
            RuntimeCommand::List => runtime_list(&paths, out),
            RuntimeCommand::Install { id } => runtime_install(&paths, out, &id).await,
            RuntimeCommand::Remove { id } => {
                runtime::remove(&paths, &id)?;
                out.print(&json!({ "removed": id }), || println!("Removed {id}."))
            }
            RuntimeCommand::Exec { id, command } => runtime_exec(&paths, &id, &command),
        },
        Command::Bottle(cmd) => bottle_command(&paths, out, cmd).await,
        Command::Component(cmd) => component_command(&paths, out, cmd).await,
        Command::Snapshot(cmd) => snapshot_command(&paths, out, cmd),
        Command::Recipe(cmd) => recipe_command(&paths, out, cmd).await,
        Command::D3dmetal(cmd) => d3dmetal_command(&paths, out, cmd),
        Command::Experimental(cmd) => experimental_command(&paths, out, cmd),
        Command::Notes(cmd) => notes_command(&paths, out, cmd),
        Command::Diag(DiagCommand::Export {
            bottle: key,
            output,
        }) => {
            let b = bottle::resolve(&paths, &key)?;
            let dest = output.unwrap_or_else(|| PathBuf::from("."));
            let zip = vinter_core::diag::export(&paths, &b.id, &dest)?;
            out.print(&json!({ "path": zip }), || {
                println!("Wrote {}", zip.display())
            })
        }
        Command::Arch { file, bottle } => arch_command(&paths, out, &file, bottle.as_deref()),
        Command::Mcp => mcp::serve(paths).await,
        Command::InstallRosetta => {
            out.note("Installing Rosetta 2. macOS may ask for an administrator password.");
            rosetta::install()?;
            out.print(&json!({ "installed": true }), || {
                println!("Rosetta 2 is installed.")
            })
        }
    }
}

fn check(paths: &AppPaths, out: Out) -> Result<()> {
    let status = system_checks(paths);
    out.print(&status, || {
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
    })?;
    if !status.is_ready() {
        std::process::exit(1);
    }
    Ok(())
}

fn runtime_list(paths: &AppPaths, out: Out) -> Result<()> {
    let installed = runtime::installed(paths)?;
    let available = runtime::manifest::available(paths);
    let value = json!({ "installed": installed, "available": available });
    out.print(&value, || {
        println!("Installed:");
        if installed.is_empty() {
            println!("  (none)");
        }
        for r in &installed {
            let note = if r.unavailable.is_some() {
                "  (cannot run here)"
            } else {
                ""
            };
            println!("  {:<30} {:<26} {}{note}", r.id, r.name, mb(r.size));
        }
        println!("\nAvailable:");
        for r in &available {
            let mark = if installed.iter().any(|i| i.id == r.id) {
                " (installed)"
            } else {
                ""
            };
            println!("  {:<30} {:<26} {}{mark}", r.id, r.name, mb(r.size));
        }
    })
}

async fn runtime_install(paths: &AppPaths, out: Out, id: &str) -> Result<()> {
    let info = runtime::manifest::find(paths, id)?;
    out.note(format_args!("Installing {} from {}", info.name, info.url));
    let mut last_stage = None;
    let rt = runtime::install(paths, &info, |p: InstallProgress| {
        if out.json {
            return;
        }
        if last_stage != Some(p.stage) {
            if last_stage.is_some() {
                println!();
            }
            last_stage = Some(p.stage);
        }
        print_progress(p);
    })
    .await?;
    out.print(&rt, || {
        println!("\nInstalled {} at {}", rt.id, rt.path.display())
    })
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

async fn bottle_command(paths: &AppPaths, out: Out, cmd: BottleCommand) -> Result<()> {
    let id = |key: &str| bottle::resolve(paths, key).map(|b| b.id);
    match cmd {
        BottleCommand::List => {
            let bottles = bottle::list(paths)?;
            out.print(&bottles, || {
                if bottles.is_empty() {
                    println!(
                        "No bottles yet. Create one with `vinter bottle create <name> --runtime <id>`."
                    );
                }
                for b in &bottles {
                    let state = match (b.setup_complete, b.legacy_32bit) {
                        (false, _) => "  (setup incomplete)",
                        (true, true) => "  (legacy 32-bit bottle)",
                        (true, false) => "",
                    };
                    let readiness = match b.readiness.level {
                        ReadinessLevel::Green => "ready",
                        ReadinessLevel::Yellow => "can move",
                        ReadinessLevel::Red => "at risk",
                        ReadinessLevel::NotApplicable => "",
                    };
                    println!(
                        "{}  {:<24} {:<28} {:<8} {:<16} {readiness}{state}",
                        &b.id[..8],
                        b.name,
                        b.runtime,
                        b.windows_version.winecfg_name(),
                        b.engine.id(),
                    );
                }
                if bottles
                    .iter()
                    .any(|b| b.readiness.level != ReadinessLevel::NotApplicable)
                {
                    println!(
                        "\nRosetta readiness: \"ready\" runs without Rosetta; \"can move\" uses Rosetta, and this Mac can run\nApple Silicon bottles; \"at risk\" uses Rosetta, which macOS 28 mostly removes. `--json` has the details."
                    );
                }
            })
        }
        BottleCommand::Create {
            name,
            runtime,
            windows,
        } => {
            let runtime = match runtime {
                Some(id) => id,
                None => runtime::default_for_new_bottles(paths)?
                    .context(
                        "no runtime is installed; install one with `vinter runtime install <id>`",
                    )?
                    .id,
            };
            let new = NewBottle {
                id: None,
                name,
                runtime,
                windows_version: windows,
                graphics: None,
            };
            let b = bottle::create(paths, new, |step| out.note(format_args!("{step}…")))?;
            out.print(&b, || println!("Created {} ({})", b.name, b.id))
        }
        BottleCommand::Show { bottle: key } => {
            let b = bottle::get(paths, &id(&key)?)?;
            // People get the full settings as JSON too: there is no shorter form.
            println!("{}", serde_json::to_string_pretty(&b)?);
            Ok(())
        }
        BottleCommand::Rename { bottle: key, name } => {
            let b = bottle::rename(paths, &id(&key)?, &name)?;
            out.print(&b, || println!("Renamed to {}.", b.name))
        }
        BottleCommand::SetWindows {
            bottle: key,
            version,
        } => {
            let b = bottle::set_windows_version(paths, &id(&key)?, version)?;
            out.print(&b, || {
                println!("Windows version set to {}.", version.winecfg_name())
            })
        }
        BottleCommand::SetRuntime {
            bottle: key,
            runtime,
        } => {
            let b = bottle::set_runtime(paths, &id(&key)?, &runtime)?;
            out.print(&b, || println!("Runtime set to {runtime}."))
        }
        BottleCommand::Duplicate { bottle: key, name } => {
            let b = bottle::duplicate(paths, &id(&key)?, &name)?;
            out.print(&b, || println!("Created {} ({})", b.name, b.id))
        }
        BottleCommand::Delete { bottle: key } => {
            let id = id(&key)?;
            bottle::delete(paths, &id)?;
            out.print(&json!({ "deleted": id }), || {
                println!("Moved to the Trash.")
            })
        }
        BottleCommand::Tool { bottle: key, tool } => {
            let pid = bottle::run_tool(paths, &id(&key)?, tool)?;
            let value = json!({ "tool": tool.program(), "pid": pid });
            out.print(&value, || {
                println!("Started {} (pid {pid}).", tool.program())
            })
        }
        BottleCommand::Kill { bottle: key } => {
            let id = id(&key)?;
            bottle::kill_all(paths, &id)?;
            out.print(&json!({ "stopped": id }), || {
                println!("Stopped all Wine processes.")
            })
        }
        BottleCommand::DriveC { bottle: key } => {
            let path = bottle::drive_c(paths, &id(&key)?)?;
            out.print(&json!({ "path": path }), || println!("{}", path.display()))
        }
        BottleCommand::Install {
            bottle: key,
            installer,
            args,
        } => {
            let id = id(&key)?;
            out.note(format_args!("Running {} …", installer.display()));
            let code = programs::run_installer(paths, &id, &installer, &args, |_| {})?;
            let found = programs::discover(paths, &id)?;
            let value = json!({ "exit_code": code, "discovered": found });
            out.print(&value, || {
                match code {
                    Some(0) => println!("Installer finished."),
                    Some(c) => {
                        println!("Installer exited with code {c} (it may have been cancelled).")
                    }
                    None => println!("Installer was stopped."),
                }
                print_discovered(&found);
            })
        }
        BottleCommand::Discover { bottle: key, add } => {
            let id = id(&key)?;
            let found = programs::discover(paths, &id)?;
            let mut added = 0;
            if add {
                let new: Vec<_> = found
                    .iter()
                    .filter(|p| !p.already_added)
                    .map(|p| p.to_def())
                    .collect();
                added = new.len();
                programs::add(paths, &id, new)?;
            }
            let value = json!({ "discovered": found, "added": added });
            out.print(&value, || {
                print_discovered(&found);
                if add {
                    println!("Added {added} program(s).");
                }
            })
        }
        BottleCommand::Programs { bottle: key } => {
            let b = bottle::get(paths, &id(&key)?)?;
            out.print(&b.programs, || {
                if b.programs.is_empty() {
                    println!("No programs. Use `vinter bottle discover {key} --add`.");
                }
                for (i, p) in b.programs.iter().enumerate() {
                    println!("{i}  {:<28} {}", p.name, p.exe);
                }
            })
        }
        BottleCommand::Run {
            bottle: key,
            program,
        } => {
            let (tx, rx) = std::sync::mpsc::channel();
            let launched = programs::launch(
                paths,
                &id(&key)?,
                program,
                move |_, line| out.note(line),
                move |code| {
                    let _ = tx.send(code);
                },
            )?;
            out.note(format_args!(
                "Started {} (pid {}).",
                launched.name, launched.pid
            ));
            let code = rx.recv().unwrap_or(None);
            let value = json!({
                "name": launched.name,
                "pid": launched.pid,
                "log_id": launched.log_id,
                "exit_code": code,
            });
            out.print(&value, || match code {
                Some(code) => println!("{} exited with code {code}.", launched.name),
                None => println!("{} was stopped.", launched.name),
            })
        }
        BottleCommand::Graphics {
            bottle: key,
            backend,
        } => {
            let b = bottle::get(paths, &id(&key)?)?;
            let rt = runtime::get(paths, &b.runtime)?;
            match backend {
                None => {
                    let infos = graphics::backends(paths, &rt);
                    let value = json!({ "selected": b.graphics, "backends": infos });
                    out.print(&value, || {
                        for info in &infos {
                            let mark = if info.backend == b.graphics { "*" } else { " " };
                            let state =
                                match (&info.reason, &info.component, info.component_installed) {
                                    (Some(reason), _, _) => format!("unavailable: {reason}"),
                                    (None, Some(c), false) => {
                                        format!("available ({} download, {})", mb(c.size), c.name)
                                    }
                                    (None, Some(c), true) => format!("available ({})", c.name),
                                    (None, None, _) => "available (built in)".into(),
                                };
                            println!("{mark} {:<10} {state}", info.backend.id());
                        }
                    })
                }
                Some(backend) => {
                    let info = graphics::backends(paths, &rt)
                        .into_iter()
                        .find(|i| i.backend == backend)
                        .expect("every backend is listed");
                    if let Some(c) = info.component.filter(|_| info.reason.is_none()) {
                        out.note(format_args!("Getting {} ({})", c.name, c.url));
                        graphics::component::ensure(paths, &c, |p: InstallProgress| {
                            if !out.json {
                                print_progress(p)
                            }
                        })
                        .await?;
                        out.note("");
                    }
                    let b = graphics::apply(paths, &b.id, backend)?;
                    out.print(&b, || println!("{} now uses {}.", b.name, backend.name()))
                }
            }
        }
        BottleCommand::TestGraphics { bottle: key } => {
            let r = graphics::test(paths, &id(&key)?)?;
            out.print(&r, || {
                println!("Backend:       {}", r.backend.name());
                println!("Direct3D 11:   {}", if r.ok { "works" } else { "failed" });
                if let Some(draws) = r.draws {
                    println!(
                        "Draws:         {}",
                        if draws {
                            "yes"
                        } else {
                            "no (cannot draw to a window)"
                        }
                    );
                }
                println!(
                    "Answered by:   {}",
                    r.renderer.as_deref().unwrap_or("unknown")
                );
                println!(
                    "Feature level: {}",
                    r.feature_level.as_deref().unwrap_or("-")
                );
                println!("GPU:           {}", r.adapter.as_deref().unwrap_or("-"));
                if !r.matches_backend {
                    println!(
                        "Warning: the bottle is set to {}, but it did not answer.",
                        r.backend.name()
                    );
                }
                println!("Log:           {}", r.log_id);
            })
        }
        BottleCommand::Backup { bottle: key, file } => {
            bottle::backup::export(paths, &id(&key)?, &file, |_, _| {})?;
            out.print(&json!({ "file": file }), || {
                println!("Saved {}", file.display())
            })
        }
        BottleCommand::Restore { file } => {
            let b = bottle::backup::import(paths, &file, |_, _| {})?;
            vinter_core::launcher::sync_bottle(paths, &b.id, &launcher_target()?)?;
            out.print(&b, || println!("Restored {} ({})", b.name, b.id))
        }
        BottleCommand::EditProgram {
            bottle: key,
            program,
            name,
            args,
            icon,
        } => {
            let id = id(&key)?;
            let patch = programs::ProgramPatch {
                name,
                args,
                icon_from: icon.map(|p| p.display().to_string()),
            };
            let b = programs::update(paths, &id, program, patch)?;
            if b.programs[program].pinned {
                vinter_core::launcher::sync_bottle(paths, &id, &launcher_target()?)?;
            }
            let p = &b.programs[program];
            out.print(p, || println!("{}: {} {}", p.name, p.exe, p.args.join(" ")))
        }
        BottleCommand::Options {
            bottle: key,
            retina,
            command_as_ctrl,
            option_as_alt,
            fps,
            wine_debug,
        } => {
            let id = id(&key)?;
            let mut options = bottle::get(paths, &id)?.options;
            options.retina_mode = retina.unwrap_or(options.retina_mode);
            options.command_as_ctrl = command_as_ctrl.unwrap_or(options.command_as_ctrl);
            options.option_as_alt = option_as_alt.unwrap_or(options.option_as_alt);
            options.show_fps = fps.unwrap_or(options.show_fps);
            options.wine_debug = wine_debug.unwrap_or(options.wine_debug);
            let b = bottle::set_options(paths, &id, options)?;
            out.print(&b.options, || {
                println!(
                    "Retina mode: {}\n⌘ as Ctrl: {}\n⌥ as Alt: {}\nFrame rate overlay: {}\nWine logging: {}",
                    on_off(b.options.retina_mode),
                    on_off(b.options.command_as_ctrl),
                    on_off(b.options.option_as_alt),
                    on_off(b.options.show_fps),
                    b.options.wine_debug.id()
                )
            })
        }
        BottleCommand::Env { bottle: key, vars } => {
            let b = bottle::get(paths, &id(&key)?)?;
            let mut env = b.env.clone();
            for var in vars {
                let (k, v) = var
                    .split_once('=')
                    .context("use KEY=VALUE, or KEY= to remove")?;
                if v.is_empty() {
                    env.remove(k);
                } else {
                    env.insert(k.to_string(), v.to_string());
                }
            }
            let env = bottle::set_env(paths, &b.id, env)?.env;
            out.print(&env, || {
                for (k, v) in &env {
                    println!("{k}={v}");
                }
            })
        }
        BottleCommand::Override {
            bottle: key,
            overrides,
        } => {
            let b = bottle::get(paths, &id(&key)?)?;
            let mut map = b.dll_overrides.clone();
            for o in overrides {
                match o.split_once('=') {
                    Some((dll, mode)) => map.insert(dll.to_string(), mode.to_string()),
                    None => map.remove(&o),
                };
            }
            let map = bottle::set_dll_overrides(paths, &b.id, map)?.dll_overrides;
            out.print(&map, || {
                for (dll, mode) in &map {
                    println!("{dll}={mode}");
                }
            })
        }
        BottleCommand::Pin {
            bottle: key,
            program,
        } => {
            let target = launcher_target()?;
            let b = vinter_core::launcher::set_pinned(paths, &id(&key)?, program, true, &target)?;
            let name = &b.programs[program].name;
            let value = json!({ "program": name, "launcher_dir": target.dir });
            out.print(&value, || {
                println!("Pinned {name}: launcher in {}", target.dir.display());
                if cfg!(target_os = "macos") {
                    println!("It opens through Vinter.app, which must be installed.");
                }
            })
        }
        BottleCommand::Unpin {
            bottle: key,
            program,
        } => {
            let b = vinter_core::launcher::set_pinned(
                paths,
                &id(&key)?,
                program,
                false,
                &launcher_target()?,
            )?;
            let name = &b.programs[program].name;
            out.print(&json!({ "program": name }), || println!("Unpinned {name}."))
        }
        BottleCommand::Report { bottle: key } => {
            let report = vinter_core::report::debug_report(paths, &id(&key)?)?;
            out.print(&json!({ "report": report }), || print!("{report}"))
        }
        BottleCommand::Doctor { bottle: key, log } => {
            let id = id(&key)?;
            let log = match log {
                Some(log) => log,
                None => {
                    logs::list(&bottle::dir(paths, &id)?)?
                        .into_iter()
                        .next()
                        .context("this bottle has no logs yet")?
                        .id
                }
            };
            let findings = vinter_core::doctor::diagnose_log(paths, &id, &log)?;
            let value = json!({ "log_id": log, "findings": findings });
            out.print(&value, || {
                println!("Log: {log}\n");
                if findings.is_empty() {
                    println!("No known problems found.");
                }
                for f in &findings {
                    let mark = match f.severity {
                        Severity::Error => "error",
                        Severity::Warning => "warn ",
                    };
                    println!("[{mark}] {}\n        {}", f.title, f.detail);
                    match &f.fix {
                        Fix::Components { verbs } => {
                            println!(
                                "        Fix: vinter component install {key} {} --accept-downloads",
                                verbs.join(" ")
                            )
                        }
                        Fix::Graphics { backend } => {
                            println!("        Fix: vinter bottle graphics {key} {}", backend.id())
                        }
                        Fix::RestoreSnapshot => {
                            println!("        Fix: vinter snapshot list {key}, then restore one")
                        }
                        Fix::Manual => {}
                    }
                    for line in &f.evidence {
                        println!("        > {line}");
                    }
                    println!();
                }
            })
        }
        BottleCommand::Logs { bottle: key, log } => {
            let dir = bottle::dir(paths, &id(&key)?)?;
            match log {
                Some(log) => {
                    let text = logs::read(&dir, &log)?;
                    out.print(&json!({ "id": log, "text": text }), || print!("{text}"))
                }
                None => {
                    let entries = logs::list(&dir)?;
                    out.print(&entries, || {
                        for l in &entries {
                            println!(
                                "{}  {:<24} {:>8} B  {}",
                                l.started_at, l.label, l.size, l.id
                            );
                        }
                    })
                }
            }
        }
    }
}

async fn component_command(paths: &AppPaths, out: Out, cmd: ComponentCommand) -> Result<()> {
    use vinter_core::components;
    match cmd {
        ComponentCommand::List { bottle: key } => {
            let list = components::list(paths, &bottle::resolve(paths, &key)?.id)?;
            out.print(&list, || {
                for c in &list {
                    let mark = if c.installed { "installed" } else { "" };
                    println!("{:<16} {:<36} {mark}", c.verb, c.name);
                }
            })
        }
        ComponentCommand::Install {
            bottle: key,
            verbs,
            accept_downloads,
        } => {
            let id = bottle::resolve(paths, &key)?.id;
            out.note(format_args!(
                "Installing {} with winetricks. This can take a while.",
                verbs.join(", ")
            ));
            components::install(paths, &id, &verbs, accept_downloads, |p| {
                if !out.json {
                    print_progress(p)
                }
            })
            .await?;
            let installed: Vec<_> = components::list(paths, &id)?
                .into_iter()
                .filter(|c| verbs.contains(&c.verb))
                .collect();
            out.print(&installed, || {
                println!();
                for c in &installed {
                    let state = if c.installed {
                        "installed"
                    } else {
                        "not recorded by winetricks"
                    };
                    println!("{}: {state}", c.name);
                }
            })
        }
    }
}

fn snapshot_command(paths: &AppPaths, out: Out, cmd: SnapshotCommand) -> Result<()> {
    use bottle::snapshot;
    // A snapshot by id or unique id prefix.
    let find = |bottle_id: &str, key: &str| -> Result<snapshot::Snapshot> {
        let all = snapshot::list(paths, bottle_id)?;
        let mut matches = all
            .into_iter()
            .filter(|s| s.id == key || (key.len() >= 4 && s.id.starts_with(key)));
        match (matches.next(), matches.next()) {
            (Some(one), None) => Ok(one),
            (None, _) => anyhow::bail!("no snapshot {key:?}"),
            (Some(_), Some(_)) => anyhow::bail!("{key:?} matches more than one snapshot"),
        }
    };
    match cmd {
        SnapshotCommand::List { bottle: key } => {
            let snaps = snapshot::list(paths, &bottle::resolve(paths, &key)?.id)?;
            out.print(&snaps, || {
                if snaps.is_empty() {
                    println!("No snapshots. Make one with `vinter snapshot create {key}`.");
                }
                for s in &snaps {
                    let auto = if s.automatic { "auto" } else { "    " };
                    println!("{}  {}  {auto}  {}", &s.id[..8], s.created_at, s.label);
                }
            })
        }
        SnapshotCommand::Create { bottle: key, label } => {
            let id = bottle::resolve(paths, &key)?.id;
            let s = snapshot::create(paths, &id, &label, false)?;
            out.print(&s, || println!("Snapshot {} ({})", s.label, &s.id[..8]))
        }
        SnapshotCommand::Restore {
            bottle: key,
            snapshot: snap,
        } => {
            let id = bottle::resolve(paths, &key)?.id;
            let s = find(&id, &snap)?;
            let b = snapshot::restore(paths, &id, &s.id)?;
            sync_launchers(paths, &b.id)?;
            out.print(&b, || {
                println!("{} is back to \"{}\" ({}).", b.name, s.label, s.created_at)
            })
        }
        SnapshotCommand::Delete {
            bottle: key,
            snapshot: snap,
        } => {
            let id = bottle::resolve(paths, &key)?.id;
            let s = find(&id, &snap)?;
            snapshot::delete(paths, &id, &s.id)?;
            out.print(&json!({ "deleted": s.id }), || {
                println!("Deleted {}.", s.label)
            })
        }
    }
}

async fn recipe_command(paths: &AppPaths, out: Out, cmd: RecipeCommand) -> Result<()> {
    let show_progress = |p: RecipeProgress| {
        if out.json {
            return;
        }
        let pct = p.percent.map(|x| format!(" {x:>3.0}%")).unwrap_or_default();
        print!(
            "\r[{}/{}] {:<44} {:<40}{pct}",
            p.task, p.tasks, p.label, p.detail
        );
        let _ = std::io::stdout().flush();
    };
    match cmd {
        RecipeCommand::List => {
            let (recipes, broken) = recipe::list(paths);
            let summaries: Vec<_> = recipes.iter().map(|r| r.summary()).collect();
            let value = json!({ "recipes": summaries, "broken": broken });
            out.print(&value, || {
                for s in &summaries {
                    let origin = if s.builtin { "built-in" } else { "yours" };
                    println!(
                        "{:<20} {:<16} {:<9} {}",
                        s.id, s.name, origin, s.description
                    );
                }
                for b in &broken {
                    println!("BROKEN {}: {}", b.file, b.error);
                }
            })
        }
        RecipeCommand::Show { id } => {
            let r = recipe::find(paths, &id)?;
            let steps: Vec<_> = r.recipe.steps.iter().map(|s| s.label()).collect();
            let value = json!({ "summary": r.summary(), "steps": steps });
            out.print(&value, || {
                println!(
                    "{} ({})\n{}\n",
                    r.recipe.name, r.recipe.id, r.recipe.description
                );
                for (i, step) in steps.iter().enumerate() {
                    println!("{:>2}. {step}", i + 1);
                }
            })
        }
        RecipeCommand::Check { file } => {
            let text = std::fs::read_to_string(&file)?;
            let r = recipe::parse(&text, recipe::Origin::User(file.clone()))?;
            let value = json!({ "id": r.recipe.id, "steps": r.recipe.steps.len() });
            out.print(&value, || {
                println!("{} is valid ({} steps).", r.recipe.id, r.recipe.steps.len())
            })
        }
        RecipeCommand::Apply {
            id,
            name,
            installer,
            accept_downloads,
        } => {
            let r = recipe::find(paths, &id)?;
            let opts = ApplyOptions {
                bottle_id: None,
                bottle_name: name.unwrap_or_else(|| r.recipe.name.clone()),
                installer,
                accept_downloads,
            };
            let b = recipe::apply(paths, &r, opts, show_progress).await?;
            let launchers = sync_launchers(paths, &b.id)?;
            let value = json!({ "bottle": b, "launchers": launchers });
            out.print(&value, || {
                println!(
                    "\nDone: bottle {} ({}) with {} program(s).",
                    b.name,
                    b.id,
                    b.programs.len()
                );
                print_launchers(&launchers);
            })
        }
        RecipeCommand::Resume {
            bottle: key,
            installer,
            accept_downloads,
        } => {
            let id = bottle::resolve(paths, &key)?.id;
            let b = recipe::resume(paths, &id, installer, accept_downloads, show_progress).await?;
            let launchers = sync_launchers(paths, &b.id)?;
            let value = json!({ "bottle": b, "launchers": launchers });
            out.print(&value, || {
                println!("\nDone: {} with {} program(s).", b.name, b.programs.len());
                print_launchers(&launchers);
            })
        }
    }
}

/// Creates the launchers for a bottle's pinned programs (a recipe pins its apps).
fn sync_launchers(paths: &AppPaths, bottle_id: &str) -> Result<Vec<PathBuf>> {
    Ok(vinter_core::launcher::sync_bottle(
        paths,
        bottle_id,
        &launcher_target()?,
    )?)
}

fn print_launchers(launchers: &[PathBuf]) {
    for path in launchers {
        println!("Launcher: {}", path.display());
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// Launchers made by the CLI. On Linux they start the Vinter app, which
/// packages install as `vinter-app`.
fn launcher_target() -> Result<vinter_core::launcher::Target> {
    Ok(vinter_core::launcher::Target::for_this_os(vec![
        "vinter-app".into(),
    ])?)
}

fn print_discovered(found: &[programs::DiscoveredProgram]) {
    if found.is_empty() {
        println!("No programs found.");
    }
    for p in found {
        let added = if p.already_added { " (added)" } else { "" };
        let icon = if p.icon.is_some() { "icon" } else { "    " };
        println!("{icon}  {:<28} {}{added}", p.name, p.exe);
    }
}

fn mb(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_000_000.0)
}
