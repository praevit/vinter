//! Window visibility, `vinter://` deep links and `--run` arguments.
//!
//! The main window starts hidden. Normally it is shown right after startup;
//! when Vinter was started by a launcher (deep link on macOS, `--run` on
//! Linux) it stays hidden, launches the program, and quits when it exits.

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_specta::Event;
use vinter_core::launcher::{self, DeepLink};

use crate::commands::programs::launch_program_opening;
use crate::events::{ApprovalRequested, Notice, ShowBottle};

/// How long to wait for a launch deep link before showing the window.
/// macOS delivers the link shortly after the app finished starting.
const DEEP_LINK_GRACE: Duration = Duration::from_millis(700);

/// Files opened with Vinter (`.exe`, `.msi`, `.vinterbottle`) that the
/// interface has not taken yet.
#[derive(Default)]
pub struct OpenedFiles(Mutex<Vec<String>>);

/// Files opened with Vinter: shows the window, which asks which bottle to use.
pub fn open_files(app: &AppHandle, files: Vec<std::path::PathBuf>) {
    let files: Vec<String> = files
        .into_iter()
        .filter(|f| is_windows_program(f) && f.is_file())
        .map(|f| f.display().to_string())
        .collect();
    if files.is_empty() {
        return;
    }
    app.state::<OpenedFiles>().0.lock().unwrap().extend(files);
    show_main_window(app);
    let _ = crate::events::OpenFiles.emit(app);
}

/// Hands the opened files to the interface (once).
pub fn take_opened_files(app: &AppHandle) -> Vec<String> {
    std::mem::take(&mut *app.state::<OpenedFiles>().0.lock().unwrap())
}

/// `.exe`/`.msi`/MSIX to install or run, or a `.vinterbottle` backup to restore.
fn is_windows_program(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|e| {
        [
            "exe",
            "msi",
            "msix",
            "msixbundle",
            "appx",
            "appxbundle",
            vinter_core::bottle::backup::EXTENSION,
        ]
        .iter()
        .any(|ext| e.eq_ignore_ascii_case(ext))
    })
}

/// `.exe`/`.msi` paths among the process arguments (Linux file managers
/// pass opened files this way). Files after `--run` are for that program.
fn file_args(args: &[String]) -> Vec<std::path::PathBuf> {
    args.iter()
        .skip(1)
        .take_while(|a| *a != "--run")
        .map(std::path::PathBuf::from)
        .filter(|p| is_windows_program(p))
        .collect()
}

/// Passed by "Start at login": stay in the menu bar, window hidden.
pub const HIDDEN_ARG: &str = "--hidden";

pub struct Startup {
    /// A launcher started a program (deep link or `--run`).
    launched_program: AtomicBool,
    /// The main window has been shown at least once.
    window_shown: AtomicBool,
    /// Links handled during startup. The link that launched Vinter can come
    /// both as an event and from `get_current()`; it must run only once.
    /// `None` once startup is over.
    startup_urls: Mutex<Option<HashSet<String>>>,
}

impl Default for Startup {
    fn default() -> Self {
        Self {
            launched_program: AtomicBool::new(false),
            window_shown: AtomicBool::new(false),
            startup_urls: Mutex::new(Some(HashSet::new())),
        }
    }
}

/// True the first time a link is seen during startup (always true afterwards).
fn first_time(app: &AppHandle, url: &str) -> bool {
    match app.state::<Startup>().startup_urls.lock().unwrap().as_mut() {
        Some(seen) => seen.insert(url.to_string()),
        None => true,
    }
}

/// True if Vinter was started only to run a program and never showed its window.
pub fn was_headless(app: &AppHandle) -> bool {
    let s = app.state::<Startup>();
    s.launched_program.load(Ordering::SeqCst) && !s.window_shown.load(Ordering::SeqCst)
}

pub fn show_main_window(app: &AppHandle) {
    app.state::<Startup>()
        .window_shown
        .store(true, Ordering::SeqCst);
    // Back in the Dock while the window is open (see `hide_main_window`).
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Hides the window; Vinter keeps running in the menu bar. On macOS it also
/// leaves the Dock until the window is shown again.
pub fn hide_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

/// Called once from `setup` with the process arguments.
pub fn on_start(app: &AppHandle, args: Vec<String>) {
    open_files(app, file_args(&args));
    let hidden = args.iter().any(|a| a == HIDDEN_ARG);
    let by_args = handle_args(app, &args);
    // Launchers made before program ids point at indexes; rebuild them.
    let paths = app.state::<crate::state::AppState>().paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Ok(target) = launcher_target() {
            let _ = launcher::upgrade_legacy(&paths, &target);
        }
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // When macOS launches Vinter for a link, the link may arrive before the
        // `on_open_url` listener exists; the plugin keeps it in `get_current()`.
        let step = Duration::from_millis(100);
        let mut waited = Duration::ZERO;
        while waited < DEEP_LINK_GRACE {
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                on_urls(&app, urls.iter().map(|u| u.to_string()).collect());
            }
            tokio::time::sleep(step).await;
            waited += step;
        }
        let startup = app.state::<Startup>();
        *startup.startup_urls.lock().unwrap() = None;
        let shown = startup.window_shown.load(Ordering::SeqCst);
        if hidden && !shown && crate::tray::is_shown(&app) {
            hide_main_window(&app);
        } else if !by_args && !startup.launched_program.load(Ordering::SeqCst) {
            show_main_window(&app);
        }
    });
}

/// Another Vinter was started (Linux `--run`, or the user opened it again).
pub fn on_second_instance(app: &AppHandle, args: Vec<String>) {
    open_files(app, file_args(&args));
    if !handle_args(app, &args) {
        show_main_window(app);
    }
}

/// Deep links: `vinter://run/<bottle>/<program>[?file=…]`,
/// `vinter://stop/<bottle>/<program>`, `vinter://open-bottle/<bottle>`, and
/// `vinter://approve/<id>` from `vinter mcp` (the request itself is read
/// from the data folder).
pub fn on_urls(app: &AppHandle, urls: Vec<String>) {
    for url in urls.into_iter().filter(|u| first_time(app, u)) {
        if url.starts_with("vinter://approve/") {
            show_main_window(app);
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                // Give a freshly shown window a moment to subscribe to events.
                tokio::time::sleep(Duration::from_millis(800)).await;
                let _ = ApprovalRequested.emit(&app);
            });
            continue;
        }
        match launcher::parse_deep_link(&url) {
            Some(DeepLink::Run {
                bottle,
                program,
                files,
            }) => run(app, bottle, program, files),
            Some(DeepLink::Stop { bottle, program }) => stop(app, &bottle, &program),
            Some(DeepLink::OpenBottle { bottle }) => open_bottle(app, bottle),
            None => notify(app, format!("Vinter does not understand the link {url}.")),
        }
    }
}

/// Handles `--run <bottle> <program> [files…]`. Returns true if it was present.
fn handle_args(app: &AppHandle, args: &[String]) -> bool {
    let Some(pos) = args.iter().position(|a| a == "--run") else {
        return false;
    };
    match (
        args.get(pos + 1),
        args.get(pos + 2)
            .and_then(|p| launcher::ProgramRef::parse(p)),
    ) {
        (Some(bottle_id), Some(program)) => {
            let files = args
                .iter()
                .skip(pos + 3)
                .map(std::path::PathBuf::from)
                .collect();
            run(app, bottle_id.clone(), program, files)
        }
        _ => notify(app, "Usage: --run <bottle-id> <program> [files…]".into()),
    }
    true
}

/// The index of a program named in a link or `--run`.
fn resolve(
    app: &AppHandle,
    bottle_id: &str,
    program: &launcher::ProgramRef,
) -> Result<usize, String> {
    let paths = &app.state::<crate::state::AppState>().paths;
    let bottle = vinter_core::bottle::get(paths, bottle_id).map_err(|e| e.to_string())?;
    program.resolve(&bottle).ok_or_else(|| {
        format!(
            "That program is no longer in the bottle \"{}\". Pin it again to make a new launcher.",
            bottle.name
        )
    })
}

fn run(
    app: &AppHandle,
    bottle_id: String,
    program: launcher::ProgramRef,
    files: Vec<std::path::PathBuf>,
) {
    app.state::<Startup>()
        .launched_program
        .store(true, Ordering::SeqCst);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let index = match resolve(&app, &bottle_id, &program) {
            Ok(index) => index,
            Err(message) => return notify(&app, format!("Could not start the program: {message}")),
        };
        if let Err(e) = launch_program_opening(&app, bottle_id, index as u32, true, files) {
            notify(&app, format!("Could not start the program: {}", e.message));
        }
    });
}

/// Stops a program Vinter started (every run of it).
fn stop(app: &AppHandle, bottle_id: &str, program: &launcher::ProgramRef) {
    let index = match resolve(app, bottle_id, program) {
        Ok(index) => index as u32,
        Err(message) => return notify(app, format!("Could not stop the program: {message}")),
    };
    let pids: Vec<u32> = app
        .state::<crate::state::AppState>()
        .runs
        .lock()
        .unwrap()
        .values()
        .filter(|r| r.bottle_id == bottle_id && r.program_index == index)
        .map(|r| r.pid)
        .collect();
    for pid in pids {
        if let Err(e) = vinter_core::process::terminate(pid) {
            notify(app, format!("Could not stop the program: {e}"));
        }
    }
}

/// Shows the window on a bottle's page.
fn open_bottle(app: &AppHandle, bottle_id: String) {
    show_main_window(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Give a freshly shown window a moment to subscribe to events.
        tokio::time::sleep(Duration::from_millis(800)).await;
        let _ = ShowBottle { bottle_id }.emit(&app);
    });
}

/// Shows the window with a message (a background launch has no other way to report).
pub(crate) fn notify(app: &AppHandle, message: String) {
    show_main_window(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Give a freshly shown window a moment to subscribe to events.
        tokio::time::sleep(Duration::from_millis(800)).await;
        let _ = Notice { message }.emit(&app);
    });
}

/// Where this app writes launchers. On Linux they run this binary (or the
/// AppImage it came from) with `--run`.
pub fn launcher_target() -> vinter_core::Result<launcher::Target> {
    let exe = std::env::var("APPIMAGE")
        .ok()
        .filter(|p| !p.is_empty())
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_else(|| "vinter-app".into());
    launcher::Target::for_this_os(vec![exe])
}
