//! Small windows opened from the menus: About and Software Update. They use
//! the same frontend bundle; `main.tsx` picks what to render.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::startup;

/// A small About window, like a Mac app's About panel but in Vinter's colors.
pub fn show_about(app: &AppHandle) {
    show_small(app, "about", "About Vinter", (300.0, 340.0));
}

/// A small window that checks for updates and can install one.
pub fn show_updates(app: &AppHandle) {
    show_small(app, "updates", "Software Update", (440.0, 190.0));
}

/// Shows the window `label` (`kind` for the frontend), creating it if needed.
fn show_small(app: &AppHandle, label: &str, title: &str, (width, height): (f64, f64)) {
    // Bring Vinter forward, also while it only lives in the menu bar.
    #[cfg(target_os = "macos")]
    let _ = app.run_on_main_thread(macos::activate);
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .initialization_script(format!("window.__VINTER_WINDOW__ = '{label}';"))
        .title(title)
        .inner_size(width, height)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .center();
    // Like Apple's panels: no title bar, the traffic lights over the content.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    if let Err(e) = builder.build() {
        startup::notify(app, format!("Could not open the {title} window: {e}"));
    }
}

/// A small "Opening …" window while a program started from a launcher or the
/// menu bar gets going. Returns its label, for [`close_launching`].
pub fn show_launching(app: &AppHandle, name: &str, icon: Option<String>) -> String {
    let label = format!("launch-{}", uuid::Uuid::new_v4().simple());
    let data = serde_json::json!({ "name": name, "icon": icon });
    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .initialization_script(format!(
            "window.__VINTER_WINDOW__ = 'launch'; window.__VINTER_LAUNCH__ = {data};"
        ))
        .title(format!("Opening {name}"))
        .inner_size(320.0, 92.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focused(false)
        .center();
    if let Err(e) = builder.build() {
        eprintln!("vinter: no launch window: {e}");
    }
    label
}

/// Closes a window from [`show_launching`] (if it is still open).
pub fn close_launching(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.destroy();
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    pub fn activate() {
        if let Some(mtm) = MainThreadMarker::new() {
            // `activate()` needs macOS 14; Vinter supports 13.
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        }
    }
}
