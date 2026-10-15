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
use vinter_core::launcher;

use crate::commands::programs::launch_program;
use crate::events::Notice;

/// How long to wait for a launch deep link before showing the window.
/// macOS delivers the link shortly after the app finished starting.
const DEEP_LINK_GRACE: Duration = Duration::from_millis(700);

/// `.exe`/`.msi` files opened with Vinter that the interface has not taken yet.
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

fn is_windows_program(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("msi"))
}

/// `.exe`/`.msi` paths among the process arguments (Linux file managers
/// pass opened files this way).
fn file_args(args: &[String]) -> Vec<std::path::PathBuf> {
    args.iter()
        .skip(1)
        .map(std::path::PathBuf::from)
        .filter(|p| is_windows_program(p))
        .collect()
}

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
    let by_args = handle_args(app, &args);
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
        if !by_args && !startup.launched_program.load(Ordering::SeqCst) {
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

/// Deep links: `vinter://run/<bottle>/<program>`.
pub fn on_urls(app: &AppHandle, urls: Vec<String>) {
    for url in urls.into_iter().filter(|u| first_time(app, u)) {
        match launcher::parse_deep_link(&url) {
            Some((bottle_id, index)) => run(app, bottle_id, index),
            None => notify(app, format!("Vinter does not understand the link {url}.")),
        }
    }
}

/// Handles `--run <bottle> <program>`. Returns true if it was present.
fn handle_args(app: &AppHandle, args: &[String]) -> bool {
    let Some(pos) = args.iter().position(|a| a == "--run") else {
        return false;
    };
    match (
        args.get(pos + 1),
        args.get(pos + 2).and_then(|i| i.parse().ok()),
    ) {
        (Some(bottle_id), Some(index)) => run(app, bottle_id.clone(), index),
        _ => notify(app, "Usage: --run <bottle-id> <program-number>".into()),
    }
    true
}

fn run(app: &AppHandle, bottle_id: String, index: usize) {
    app.state::<Startup>()
        .launched_program
        .store(true, Ordering::SeqCst);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = launch_program(&app, bottle_id, index as u32) {
            notify(&app, format!("Could not start the program: {}", e.message));
        }
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
