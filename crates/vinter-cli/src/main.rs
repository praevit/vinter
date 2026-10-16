//! `vinter`: a thin CLI over `vinter-core`.

use std::io::Write;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use vinter_core::AppPaths;
use vinter_core::bottle::{self, Graphics, NewBottle, Tool, WindowsVersion};
use vinter_core::graphics::{self, Backend};
use vinter_core::process::logs;
use vinter_core::programs;
use vinter_core::recipe::{self, ApplyOptions, RecipeProgress};
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
    /// App recipes: list them, check one, or set up an app from one.
    #[command(subcommand)]
    Recipe(RecipeCommand),
    /// Install Rosetta 2 (Apple Silicon only). Running this accepts Apple's license.
    InstallRosetta,
    /// Apple's D3DMetal, from your own copy of the Game Porting Toolkit.
    #[command(subcommand)]
    D3dmetal(D3dmetalCommand),
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
        /// macOS: ⌘ works as Ctrl, ⌥ as Alt.
        #[arg(long)]
        command_as_ctrl: Option<bool>,
        /// Frame rate overlay in games.
        #[arg(long)]
        fps: Option<bool>,
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

fn d3dmetal_command(paths: &AppPaths, cmd: D3dmetalCommand) -> Result<()> {
    use vinter_core::graphics::d3dmetal;
    match cmd {
        D3dmetalCommand::Status => match d3dmetal::installed(paths) {
            Some(d) => println!("D3DMetal {} (imported {})", d.version, d.imported_at),
            None => println!("Not imported."),
        },
        D3dmetalCommand::Import { source, accept } => {
            if !accept {
                let found = d3dmetal::inspect(&source)?;
                println!("{}\n", found.license.trim());
                println!(
                    "This is Apple's license for D3DMetal {}. Run again with --accept to agree and import it.",
                    found.version
                );
                return Ok(());
            }
            let d = d3dmetal::import(paths, &source)?;
            println!("Imported D3DMetal {}.", d.version);
        }
        D3dmetalCommand::Remove => {
            d3dmetal::remove(paths)?;
            println!("Removed.");
        }
    }
    Ok(())
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
        Command::Bottle(cmd) => bottle_command(&paths, cmd).await,
        Command::Recipe(cmd) => recipe_command(&paths, cmd).await,
        Command::D3dmetal(cmd) => d3dmetal_command(&paths, cmd),
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

async fn bottle_command(paths: &AppPaths, cmd: BottleCommand) -> Result<()> {
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
                id: None,
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
        BottleCommand::Install {
            bottle: key,
            installer,
            args,
        } => {
            let id = id(&key)?;
            println!("Running {} …", installer.display());
            let code = programs::run_installer(paths, &id, &installer, &args, |_| {})?;
            match code {
                Some(0) => println!("Installer finished."),
                Some(c) => println!("Installer exited with code {c} (it may have been cancelled)."),
                None => println!("Installer was stopped."),
            }
            print_discovered(&programs::discover(paths, &id)?);
        }
        BottleCommand::Discover { bottle: key, add } => {
            let id = id(&key)?;
            let found = programs::discover(paths, &id)?;
            print_discovered(&found);
            if add {
                let new: Vec<_> = found
                    .iter()
                    .filter(|p| !p.already_added)
                    .map(|p| p.to_def())
                    .collect();
                let count = new.len();
                programs::add(paths, &id, new)?;
                println!("Added {count} program(s).");
            }
        }
        BottleCommand::Programs { bottle: key } => {
            let b = bottle::get(paths, &id(&key)?)?;
            if b.programs.is_empty() {
                println!("No programs. Use `vinter bottle discover {key} --add`.");
            }
            for (i, p) in b.programs.iter().enumerate() {
                println!("{i}  {:<28} {}", p.name, p.exe);
            }
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
                |_, line| println!("{line}"),
                move |code| {
                    let _ = tx.send(code);
                },
            )?;
            println!("Started {} (pid {}).", launched.name, launched.pid);
            let code = rx.recv().unwrap_or(None);
            println!("{} exited with {:?}.", launched.name, code);
        }
        BottleCommand::Graphics {
            bottle: key,
            backend,
        } => {
            let b = bottle::get(paths, &id(&key)?)?;
            let rt = runtime::get(paths, &b.runtime)?;
            match backend {
                None => {
                    for info in graphics::backends(paths, &rt) {
                        let mark = if info.backend == b.graphics { "*" } else { " " };
                        let state = match (&info.reason, &info.component, info.component_installed)
                        {
                            (Some(reason), _, _) => format!("unavailable: {reason}"),
                            (None, Some(c), false) => {
                                format!("available ({} download, {})", mb(c.size), c.name)
                            }
                            (None, Some(c), true) => format!("available ({})", c.name),
                            (None, None, _) => "available (built in)".into(),
                        };
                        println!("{mark} {:<10} {state}", info.backend.id());
                    }
                }
                Some(backend) => {
                    let info = graphics::backends(paths, &rt)
                        .into_iter()
                        .find(|i| i.backend == backend)
                        .expect("every backend is listed");
                    if let Some(c) = info.component.filter(|_| info.reason.is_none()) {
                        println!("Getting {} ({})", c.name, c.url);
                        graphics::component::ensure(paths, &c, |p: InstallProgress| {
                            print_progress(p)
                        })
                        .await?;
                        println!();
                    }
                    graphics::apply(paths, &b.id, backend)?;
                    println!("{} now uses {}.", b.name, backend.name());
                }
            }
        }
        BottleCommand::TestGraphics { bottle: key } => {
            let r = graphics::test(paths, &id(&key)?)?;
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
        }
        BottleCommand::Backup { bottle: key, file } => {
            bottle::backup::export(paths, &id(&key)?, &file, |_, _| {})?;
            println!("Saved {}", file.display());
        }
        BottleCommand::Restore { file } => {
            let b = bottle::backup::import(paths, &file, |_, _| {})?;
            vinter_core::launcher::sync_bottle(paths, &b.id, &launcher_target()?)?;
            println!("Restored {} ({})", b.name, b.id);
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
            println!("{}: {} {}", p.name, p.exe, p.args.join(" "));
        }
        BottleCommand::Options {
            bottle: key,
            retina,
            command_as_ctrl,
            fps,
        } => {
            let id = id(&key)?;
            let mut options = bottle::get(paths, &id)?.options;
            options.retina_mode = retina.unwrap_or(options.retina_mode);
            options.command_as_ctrl = command_as_ctrl.unwrap_or(options.command_as_ctrl);
            options.show_fps = fps.unwrap_or(options.show_fps);
            let b = bottle::set_options(paths, &id, options)?;
            println!(
                "Retina mode: {}\n⌘ as Ctrl: {}\nFrame rate overlay: {}",
                on_off(b.options.retina_mode),
                on_off(b.options.command_as_ctrl),
                on_off(b.options.show_fps)
            );
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
            for (k, v) in &bottle::set_env(paths, &b.id, env)?.env {
                println!("{k}={v}");
            }
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
            for (dll, mode) in &bottle::set_dll_overrides(paths, &b.id, map)?.dll_overrides {
                println!("{dll}={mode}");
            }
        }
        BottleCommand::Pin {
            bottle: key,
            program,
        } => {
            let target = launcher_target()?;
            let b = vinter_core::launcher::set_pinned(paths, &id(&key)?, program, true, &target)?;
            println!(
                "Pinned {}: launcher in {}",
                b.programs[program].name,
                target.dir.display()
            );
            if cfg!(target_os = "macos") {
                println!("It opens through Vinter.app, which must be installed.");
            }
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
            println!("Unpinned {}.", b.programs[program].name);
        }
        BottleCommand::Report { bottle: key } => {
            print!("{}", vinter_core::report::debug_report(paths, &id(&key)?)?);
        }
        BottleCommand::Logs { bottle: key, log } => {
            let dir = bottle::dir(paths, &id(&key)?)?;
            match log {
                Some(log) => print!("{}", logs::read(&dir, &log)?),
                None => {
                    for l in logs::list(&dir)? {
                        println!(
                            "{}  {:<24} {:>8} B  {}",
                            l.started_at, l.label, l.size, l.id
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

async fn recipe_command(paths: &AppPaths, cmd: RecipeCommand) -> Result<()> {
    let show_progress = |p: RecipeProgress| {
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
            for r in recipes {
                let s = r.summary();
                let origin = if s.builtin { "built-in" } else { "yours" };
                println!(
                    "{:<20} {:<16} {:<9} {}",
                    s.id, s.name, origin, s.description
                );
            }
            for b in broken {
                println!("BROKEN {}: {}", b.file, b.error);
            }
        }
        RecipeCommand::Show { id } => {
            let r = recipe::find(paths, &id)?;
            println!(
                "{} ({})\n{}\n",
                r.recipe.name, r.recipe.id, r.recipe.description
            );
            for (i, step) in r.recipe.steps.iter().enumerate() {
                println!("{:>2}. {}", i + 1, step.label());
            }
        }
        RecipeCommand::Check { file } => {
            let text = std::fs::read_to_string(&file)?;
            let r = recipe::parse(&text, recipe::Origin::User(file.clone()))?;
            println!("{} is valid ({} steps).", r.recipe.id, r.recipe.steps.len());
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
            println!(
                "\nDone: bottle {} ({}) with {} program(s).",
                b.name,
                b.id,
                b.programs.len()
            );
            print_launchers(paths, &b.id)?;
        }
        RecipeCommand::Resume {
            bottle: key,
            installer,
            accept_downloads,
        } => {
            let id = bottle::resolve(paths, &key)?.id;
            let b = recipe::resume(paths, &id, installer, accept_downloads, show_progress).await?;
            println!("\nDone: {} with {} program(s).", b.name, b.programs.len());
            print_launchers(paths, &b.id)?;
        }
    }
    Ok(())
}

/// Creates the launchers for a bottle's pinned programs (a recipe pins its apps).
fn print_launchers(paths: &AppPaths, bottle_id: &str) -> Result<()> {
    for path in vinter_core::launcher::sync_bottle(paths, bottle_id, &launcher_target()?)? {
        println!("Launcher: {}", path.display());
    }
    Ok(())
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
