//! Messages for things that finish in the background: a toast in the window
//! when it is in front, a system notification otherwise (if enabled).

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_specta::Event;

use crate::events::Notice;
use crate::state::AppState;

/// Vinter's main window is visible and focused.
pub fn main_window_in_front(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
}

pub fn send(app: &AppHandle, title: &str, body: &str) {
    if main_window_in_front(app) {
        let _ = Notice {
            message: format!("{title}. {body}"),
        }
        .emit(app);
        return;
    }
    if !vinter_core::settings::load(&app.state::<AppState>().paths).notifications {
        return;
    }
    let _ = app.notification().builder().title(title).body(body).show();
}
