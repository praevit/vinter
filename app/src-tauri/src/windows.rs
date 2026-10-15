//! Small windows opened from the menus: the About panel and Software Update.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::startup;

const UPDATES: &str = "updates";

/// The standard macOS About panel (name, version, copyright, icon from the
/// app bundle). Elsewhere, the About page.
pub fn show_about(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.run_on_main_thread(macos::about);
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_specta::Event;
        startup::show_main_window(app);
        let _ = crate::events::Navigate {
            to: crate::events::NavigateTo::About,
        }
        .emit(app);
    }
}

/// A small window that checks for updates and can install one.
pub fn show_updates(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.run_on_main_thread(macos::activate);
    if let Some(w) = app.get_webview_window(UPDATES) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let result = WebviewWindowBuilder::new(app, UPDATES, WebviewUrl::App("index.html".into()))
        // `main.tsx` renders the update window instead of the app.
        .initialization_script("window.__VINTER_WINDOW__ = 'updates';")
        .title("Software Update")
        .inner_size(440.0, 170.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .center()
        .build();
    if let Err(e) = result {
        startup::notify(app, format!("Could not open the update window: {e}"));
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    /// Brings Vinter to the front, also while it only lives in the menu bar.
    pub fn activate() {
        if let Some(mtm) = MainThreadMarker::new() {
            // `activate()` needs macOS 14; Vinter supports 13.
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        }
    }

    pub fn about() {
        activate();
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).orderFrontStandardAboutPanel(None);
        }
    }
}
