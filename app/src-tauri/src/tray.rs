//! The menu bar icon (macOS) or tray icon (Linux): open Vinter, start any
//! installed program, quit. While it exists, closing the window (and ⌘Q on
//! macOS) only hides it, so Vinter stays in the menu bar.

use std::sync::Mutex;

use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuEvent, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};
use vinter_core::bottle;

use crate::commands::programs::launch_program;
use crate::startup;
use crate::state::AppState;

const TRAY_ID: &str = "vinter";
const OPEN: &str = "open";
const ABOUT: &str = "about";
const UPDATES: &str = "updates";
const QUIT: &str = "quit";
/// Program items have the id `run/<bottle>/<index>`.
const RUN_PREFIX: &str = "run/";

/// The programs the menu was last built with; `None` if there is no tray icon
/// (e.g. a Linux desktop without one).
#[derive(Default)]
pub struct TrayState(Mutex<Option<Vec<Entry>>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    label: String,
    bottle_id: String,
    index: usize,
}

/// Every program in every ready bottle, by name. A name found in several
/// bottles gets the bottle name added.
fn entries(app: &AppHandle) -> Vec<Entry> {
    let paths = &app.state::<AppState>().paths;
    let mut found = Vec::new();
    for summary in bottle::list(paths).unwrap_or_default() {
        if !summary.setup_complete {
            continue;
        }
        let Ok(b) = bottle::get(paths, &summary.id) else {
            continue;
        };
        for (index, program) in b.programs.iter().enumerate() {
            found.push((program.name.clone(), b.name.clone(), b.id.clone(), index));
        }
    }
    let mut entries: Vec<Entry> = found
        .iter()
        .map(|(name, bottle_name, bottle_id, index)| {
            let shared = found.iter().filter(|(n, ..)| n == name).count() > 1;
            Entry {
                label: if shared {
                    format!("{name} ({bottle_name})")
                } else {
                    name.clone()
                },
                bottle_id: bottle_id.clone(),
                index: *index,
            }
        })
        .collect();
    entries.sort_by_key(|e| e.label.to_lowercase());
    entries
}

fn build_menu(app: &AppHandle, entries: &[Entry]) -> tauri::Result<Menu<Wry>> {
    let mut menu = MenuBuilder::new(app)
        .text(OPEN, "Show Vinter")
        .separator()
        .item(&MenuItem::new(
            app,
            if entries.is_empty() {
                "No apps yet"
            } else {
                "Apps"
            },
            false,
            None::<&str>,
        )?);
    for e in entries {
        menu = menu.text(format!("{RUN_PREFIX}{}/{}", e.bottle_id, e.index), &e.label);
    }
    menu.separator()
        .text(ABOUT, "About Vinter")
        .text(UPDATES, "Check for Updates…")
        .separator()
        .text(QUIT, "Quit Vinter")
        .build()
}

/// Adds the icon. On failure Vinter works as before, without one.
pub fn create(app: &AppHandle) {
    let entries = entries(app);
    let result = build_menu(app, &entries).and_then(|menu| {
        TrayIconBuilder::with_id(TRAY_ID)
            .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
            .icon_as_template(true)
            .tooltip("Vinter")
            .menu(&menu)
            .on_menu_event(on_menu_event)
            .build(app)
    });
    match result {
        Ok(_) => *app.state::<TrayState>().0.lock().unwrap() = Some(entries),
        Err(e) => return eprintln!("vinter: no menu bar icon: {e}"),
    }
    #[cfg(target_os = "macos")]
    if let Err(e) = app_menu::install(app) {
        eprintln!("vinter: could not change the app menu: {e}");
    }
}

/// macOS: ⌘Q hides Vinter to the menu bar instead of quitting. "Quit Vinter"
/// (⌥⌘Q, or in the menu bar icon's menu) still quits.
#[cfg(target_os = "macos")]
mod app_menu {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::{AppHandle, Wry};

    const HIDE: &str = "app-menu-hide";
    const UPDATES: &str = "app-menu-updates";
    const QUIT: &str = "app-menu-quit";

    pub fn install(app: &AppHandle) -> tauri::Result<()> {
        // Tauri's standard menu, with the app menu's last item (Quit) replaced.
        let menu: Menu<Wry> = Menu::default(app)?;
        if let Some(app_submenu) = menu.items()?.first().and_then(|i| i.as_submenu().cloned()) {
            // After "About Vinter".
            app_submenu.insert(
                &MenuItem::with_id(app, UPDATES, "Check for Updates…", true, None::<&str>)?,
                1,
            )?;
            let last = app_submenu.items()?.len().saturating_sub(1);
            app_submenu.remove_at(last)?;
            app_submenu.append(&MenuItem::with_id(
                app,
                HIDE,
                "Keep in Menu Bar",
                true,
                Some("CmdOrCtrl+Q"),
            )?)?;
            app_submenu.append(&PredefinedMenuItem::separator(app)?)?;
            app_submenu.append(&MenuItem::with_id(
                app,
                QUIT,
                "Quit Vinter",
                true,
                Some("Alt+CmdOrCtrl+Q"),
            )?)?;
        }
        app.set_menu(menu)?;
        app.on_menu_event(|app, event| match event.id().as_ref() {
            HIDE => crate::startup::hide_main_window(app),
            UPDATES => crate::windows::show_updates(app),
            QUIT => app.exit(0),
            _ => {}
        });
        Ok(())
    }
}

/// True if the icon exists, so hiding the window keeps Vinter reachable.
pub fn is_shown(app: &AppHandle) -> bool {
    app.state::<TrayState>().0.lock().unwrap().is_some()
}

/// Rebuilds the menu if the programs changed. Reads every bottle: call it
/// off the main thread.
pub fn refresh(app: &AppHandle) {
    let entries = entries(app);
    let state = app.state::<TrayState>();
    let mut last = state.0.lock().unwrap();
    if last.is_none() || last.as_ref() == Some(&entries) {
        return;
    }
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(menu) = build_menu(app, &entries) {
        if tray.set_menu(Some(menu)).is_ok() {
            *last = Some(entries);
        }
    }
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        OPEN => startup::show_main_window(app),
        ABOUT => crate::windows::show_about(app),
        UPDATES => crate::windows::show_updates(app),
        QUIT => app.exit(0),
        id => {
            let Some((bottle_id, index)) = id
                .strip_prefix(RUN_PREFIX)
                .and_then(|rest| rest.split_once('/'))
                .and_then(|(b, i)| Some((b.to_string(), i.parse::<u32>().ok()?)))
            else {
                return;
            };
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(e) = launch_program(&app, bottle_id, index) {
                    startup::notify(&app, format!("Could not start the program: {}", e.message));
                }
            });
        }
    }
}
