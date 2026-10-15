//! Bringing newly opened Wine windows to the front.
//!
//! Each Wine program is its own macOS app. Since macOS 14, an app started in
//! the background may not activate itself unless the active app (Vinter,
//! right after the user clicked) hands over activation. Wine also re-parents
//! the program, so it is found by its executable living inside the runtime.

use std::path::{Path, PathBuf};

use tauri::AppHandle;

/// Remembers which Wine apps already run, then (after a launch) waits for a
/// new one from `runtime_dir` and brings it to the front. Call [`Watch::start`]
/// before launching and [`Watch::focus_new_app`] after.
pub struct Watch {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    app: AppHandle,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    runtime_dir: PathBuf,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    before: Vec<i32>,
}

impl Watch {
    pub fn start(app: &AppHandle, runtime_dir: &Path) -> Self {
        let runtime_dir = runtime_dir
            .canonicalize()
            .unwrap_or_else(|_| runtime_dir.to_path_buf());
        #[cfg(target_os = "macos")]
        let before = {
            let dir = runtime_dir.clone();
            on_main(app, move || macos::wine_app_pids(&dir)).unwrap_or_default()
        };
        #[cfg(not(target_os = "macos"))]
        let before = Vec::new();
        Self {
            app: app.clone(),
            runtime_dir,
            before,
        }
    }

    /// Polls for up to 15 s in the background; returns immediately.
    pub fn focus_new_app(self) {
        self.focus_new_app_then(60, |_| {});
    }

    /// Like [`Self::focus_new_app`], polling up to `polls` × 250 ms, then calls
    /// `done` with whether a new app appeared (on Linux right away, with false).
    pub fn focus_new_app_then(self, polls: u32, done: impl FnOnce(bool) + Send + 'static) {
        #[cfg(target_os = "macos")]
        std::thread::spawn(move || {
            for _ in 0..polls {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let (dir, before) = (self.runtime_dir.clone(), self.before.clone());
                let focused = on_main(&self.app, move || macos::focus_first_new(&dir, &before));
                if focused != Some(false) {
                    return done(focused.unwrap_or(false));
                }
            }
            done(false);
        });
        #[cfg(not(target_os = "macos"))]
        {
            let _ = polls;
            done(false);
        }
    }
}

/// Runs `f` on the main thread and waits for its result.
#[cfg(target_os = "macos")]
fn on_main<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .ok()?;
    rx.recv().ok()
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::Path;

    use objc2::rc::Retained;
    use objc2::runtime::NSObjectProtocol;
    use objc2::sel;
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy,
        NSRunningApplication, NSWorkspace,
    };
    use objc2_foundation::MainThreadMarker;

    /// Regular (Dock-visible) apps whose executable is inside `runtime_dir`.
    fn wine_apps(runtime_dir: &Path) -> Vec<Retained<NSRunningApplication>> {
        let apps = NSWorkspace::sharedWorkspace().runningApplications();
        apps.iter()
            .filter(|a| a.activationPolicy() == NSApplicationActivationPolicy::Regular)
            .filter(|a| {
                a.executableURL()
                    .and_then(|url| url.path())
                    .is_some_and(|p| Path::new(&p.to_string()).starts_with(runtime_dir))
            })
            .collect()
    }

    pub fn wine_app_pids(runtime_dir: &Path) -> Vec<i32> {
        wine_apps(runtime_dir)
            .iter()
            .map(|a| a.processIdentifier())
            .collect()
    }

    /// Activates the first Wine app not in `before`. True if one was found.
    pub fn focus_first_new(runtime_dir: &Path, before: &[i32]) -> bool {
        let Some(target) = wine_apps(runtime_dir)
            .into_iter()
            .find(|a| !before.contains(&a.processIdentifier()))
        else {
            return false;
        };
        if let Some(mtm) = MainThreadMarker::new() {
            let ns_app = NSApplication::sharedApplication(mtm);
            // macOS 14+: let the Wine app take over activation from us.
            if ns_app.respondsToSelector(sel!(yieldActivationToApplication:)) {
                ns_app.yieldActivationToApplication(&target);
            }
        }
        target.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        true
    }
}
