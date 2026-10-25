//! Windows opened from the menus: About, Software Update and (macOS)
//! Settings. They use the same frontend bundle; `main.tsx` picks what to
//! render.

use tauri::{AppHandle, Manager, Theme, WebviewUrl, WebviewWindowBuilder};
use vinter_core::settings::Appearance;

use crate::startup;
use crate::state::AppState;

/// A small About window, like a Mac app's About panel but in Vinter's colors.
pub fn show_about(app: &AppHandle) {
    show_small(app, "about", "About Vinter", (300.0, 340.0));
}

/// A small window that checks for updates and can install one.
pub fn show_updates(app: &AppHandle) {
    show_small(app, "updates", "Software Update", (440.0, 190.0));
}

/// Settings in its own window, like a Mac app's (⌘,).
pub fn show_settings(app: &AppHandle) {
    show(app, "settings", "Settings", (720.0, 680.0), true);
}

/// Light, dark or the system's appearance for every window, now and later.
pub fn apply_appearance(app: &AppHandle, appearance: Appearance) {
    app.set_theme(theme(appearance));
}

fn theme(appearance: Appearance) -> Option<Theme> {
    match appearance {
        Appearance::System => None,
        Appearance::Light => Some(Theme::Light),
        Appearance::Dark => Some(Theme::Dark),
    }
}

fn show_small(app: &AppHandle, label: &str, title: &str, size: (f64, f64)) {
    show(app, label, title, size, false);
}

/// Shows the window `label` (`kind` for the frontend), creating it if needed.
fn show(app: &AppHandle, label: &str, title: &str, (width, height): (f64, f64), resizable: bool) {
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
        .resizable(resizable)
        .maximizable(false)
        .minimizable(resizable)
        .theme(theme(
            vinter_core::settings::load(&app.state::<AppState>().paths).appearance,
        ))
        .center();
    let builder = if resizable {
        builder.min_inner_size(width * 0.75, height * 0.6)
    } else {
        builder
    };
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
        .inner_size(360.0, 104.0)
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
    // macOS: the system's frosted HUD material, with rounded corners.
    #[cfg(target_os = "macos")]
    let builder = builder.effects(
        tauri::window::EffectsBuilder::new()
            .effect(tauri::window::Effect::HudWindow)
            .state(tauri::window::EffectState::Active)
            .radius(20.0)
            .build(),
    );
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
