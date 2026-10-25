//! The menu bar icon (macOS) or tray icon (Linux): open Vinter, show or stop
//! running programs, start recent or any installed program, quit. While it
//! exists, closing the window (and ⌘Q on macOS) only hides it, so Vinter
//! stays in the menu bar.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuEvent, MenuItem, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};
use vinter_core::bottle;

use crate::commands::programs::{RunInfo, launch_program};
use crate::startup;
use crate::state::AppState;

const TRAY_ID: &str = "vinter";
const OPEN: &str = "open";
const ABOUT: &str = "about";
const UPDATES: &str = "updates";
const QUIT: &str = "quit";
/// Program items have the id `run/<bottle>/<index>` (or `recent/…`).
const RUN_PREFIX: &str = "run/";
const RECENT_PREFIX: &str = "recent/";
/// Items for a running program: `show/<run id>` and `stop/<run id>`.
const SHOW_PREFIX: &str = "show/";
const STOP_PREFIX: &str = "stop/";
/// Programs in the Recent section.
const RECENT: usize = 5;

#[derive(Default)]
pub struct TrayState {
    /// What the menu was last built with; `None` if there is no tray icon
    /// (e.g. a Linux desktop without one).
    entries: Mutex<Option<Model>>,
    /// Turned off in Settings.
    hidden: AtomicBool,
    /// macOS: the ⌘Q item, whose label follows the setting.
    #[cfg(target_os = "macos")]
    quit_item: Mutex<Option<MenuItem<Wry>>>,
    /// The newer Vinter an automatic check found, named by the update items.
    update_version: Mutex<Option<String>>,
    /// "Check for Updates…" in the icon's menu and (macOS) the app menu.
    update_items: Mutex<UpdateItems>,
}

#[derive(Default)]
struct UpdateItems {
    tray: Option<MenuItem<Wry>>,
    #[cfg(target_os = "macos")]
    app_menu: Option<MenuItem<Wry>>,
}

fn update_label(version: Option<&str>) -> String {
    match version {
        Some(v) => format!("Update to Vinter {v}…"),
        None => "Check for Updates…".into(),
    }
}

/// Names a newer Vinter (or none) in the update items.
pub fn set_update(app: &AppHandle, version: Option<&str>) {
    let state = app.state::<TrayState>();
    *state.update_version.lock().unwrap() = version.map(str::to_string);
    let label = update_label(version);
    let items = state.update_items.lock().unwrap();
    if let Some(item) = &items.tray {
        let _ = item.set_text(&label);
    }
    #[cfg(target_os = "macos")]
    if let Some(item) = &items.app_menu {
        let _ = item.set_text(&label);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    label: String,
    bottle_id: String,
    index: usize,
}

/// What the menu shows. It is rebuilt only when this changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Model {
    /// Running programs: run id and label.
    running: Vec<(String, String)>,
    /// The most recently launched programs, newest first.
    recent: Vec<Entry>,
    /// Each ready bottle's name and its programs, by name.
    groups: Vec<(String, Vec<Entry>)>,
}

/// A program as the menu needs it.
struct Found {
    name: String,
    bottle_name: String,
    bottle_id: String,
    index: usize,
    last_launched: Option<String>,
}

/// Every program in every ready bottle, and what runs.
fn model(app: &AppHandle) -> Model {
    let state = app.state::<AppState>();
    let paths = &state.paths;
    let mut found = Vec::new();
    for summary in bottle::list(paths).unwrap_or_default() {
        if !summary.setup_complete {
            continue;
        }
        let Ok(b) = bottle::get(paths, &summary.id) else {
            continue;
        };
        for (index, program) in b.programs.iter().enumerate() {
            found.push(Found {
                name: program.name.clone(),
                bottle_name: b.name.clone(),
                bottle_id: b.id.clone(),
                index,
                last_launched: program.last_launched.clone(),
            });
        }
    }
    let runs: Vec<RunInfo> = state.runs.lock().unwrap().values().cloned().collect();
    build_model(&found, runs)
}

fn build_model(found: &[Found], mut runs: Vec<RunInfo>) -> Model {
    // A name found in several bottles gets the bottle name added.
    let label = |f: &Found| {
        if found.iter().filter(|o| o.name == f.name).count() > 1 {
            format!("{} ({})", f.name, f.bottle_name)
        } else {
            f.name.clone()
        }
    };
    let entry = |f: &Found, label: String| Entry {
        label,
        bottle_id: f.bottle_id.clone(),
        index: f.index,
    };

    runs.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    let running = runs
        .iter()
        .map(|r| {
            let program = found
                .iter()
                .find(|f| f.bottle_id == r.bottle_id && f.index == r.program_index as usize);
            (
                r.run_id.clone(),
                program.map_or_else(|| r.name.clone(), label),
            )
        })
        .collect();

    let mut launched: Vec<&Found> = found.iter().filter(|f| f.last_launched.is_some()).collect();
    // RFC 3339 times in UTC sort as text.
    launched.sort_by(|a, b| b.last_launched.cmp(&a.last_launched));
    let recent = launched
        .into_iter()
        .take(RECENT)
        .map(|f| entry(f, label(f)))
        .collect();

    let mut groups: Vec<(String, Vec<Entry>)> = Vec::new();
    for f in found {
        let item = entry(f, f.name.clone());
        match groups
            .iter_mut()
            .find(|(_, e)| e[0].bottle_id == f.bottle_id)
        {
            Some((_, entries)) => entries.push(item),
            None => groups.push((f.bottle_name.clone(), vec![item])),
        }
    }
    for (_, entries) in &mut groups {
        entries.sort_by_key(|e| e.label.to_lowercase());
    }
    groups.sort_by_key(|(name, _)| name.to_lowercase());

    Model {
        running,
        recent,
        groups,
    }
}

/// A greyed-out section title.
fn header(app: &AppHandle, text: &str) -> tauri::Result<MenuItem<Wry>> {
    MenuItem::new(app, text, false, None::<&str>)
}

fn build_menu(app: &AppHandle, model: &Model) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<TrayState>();
    let label = update_label(state.update_version.lock().unwrap().as_deref());
    let updates = MenuItem::with_id(app, UPDATES, label, true, None::<&str>)?;
    state.update_items.lock().unwrap().tray = Some(updates.clone());
    let mut menu = MenuBuilder::new(app).text(OPEN, "Show Vinter").separator();

    if !model.running.is_empty() {
        menu = menu.item(&header(app, "Running")?);
        for (run_id, label) in &model.running {
            let mut sub = SubmenuBuilder::new(app, label);
            // On Linux the desktop brings windows forward, not Vinter.
            if cfg!(target_os = "macos") {
                sub = sub.text(format!("{SHOW_PREFIX}{run_id}"), "Show");
            }
            let sub = sub.text(format!("{STOP_PREFIX}{run_id}"), "Stop").build()?;
            menu = menu.item(&sub);
        }
        menu = menu.separator();
    }

    if !model.recent.is_empty() {
        menu = menu.item(&header(app, "Recent")?);
        for e in &model.recent {
            menu = menu.text(
                format!("{RECENT_PREFIX}{}/{}", e.bottle_id, e.index),
                &e.label,
            );
        }
        menu = menu.separator();
    }

    if model.groups.is_empty() {
        menu = menu.item(&header(app, "No apps yet")?);
    } else {
        let mut all = SubmenuBuilder::new(app, "All Apps");
        for (i, (bottle_name, entries)) in model.groups.iter().enumerate() {
            if i > 0 {
                all = all.separator();
            }
            all = all.item(&header(app, bottle_name)?);
            for e in entries {
                all = all.text(format!("{RUN_PREFIX}{}/{}", e.bottle_id, e.index), &e.label);
            }
        }
        menu = menu.item(&all.build()?);
    }

    menu.separator()
        .text(ABOUT, "About Vinter")
        .item(&updates)
        .separator()
        .text(QUIT, "Quit Vinter")
        .build()
}

/// Adds the icon. On failure Vinter works as before, without one.
pub fn create(app: &AppHandle) {
    let entries = model(app);
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
        Ok(_) => *app.state::<TrayState>().entries.lock().unwrap() = Some(entries),
        Err(e) => return eprintln!("vinter: no menu bar icon: {e}"),
    }
    #[cfg(target_os = "macos")]
    match app_menu::install(app) {
        Ok((quit, updates)) => {
            let state = app.state::<TrayState>();
            *state.quit_item.lock().unwrap() = Some(quit);
            state.update_items.lock().unwrap().app_menu = Some(updates);
        }
        Err(e) => eprintln!("vinter: could not change the app menu: {e}"),
    }
    let show = vinter_core::settings::load(&app.state::<AppState>().paths).menu_bar_icon;
    set_visible(app, show);
}

/// Shows or hides the icon (the "Menu bar icon" setting). Without it,
/// closing the window and ⌘Q quit as usual.
pub fn set_visible(app: &AppHandle, visible: bool) {
    let state = app.state::<TrayState>();
    state.hidden.store(!visible, Ordering::SeqCst);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(visible);
    }
    #[cfg(target_os = "macos")]
    if let Some(item) = state.quit_item.lock().unwrap().as_ref() {
        let _ = item.set_text(if visible {
            "Keep in Menu Bar"
        } else {
            "Quit Vinter"
        });
    }
}

/// macOS: ⌘Q hides Vinter to the menu bar instead of quitting. "Quit Vinter"
/// (⌥⌘Q, or in the menu bar icon's menu) still quits.
#[cfg(target_os = "macos")]
mod app_menu {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::{AppHandle, Wry};

    use crate::events::NavigateTo;

    const ABOUT: &str = "app-menu-about";
    const HIDE: &str = "app-menu-hide";
    const UPDATES: &str = "app-menu-updates";
    const SETTINGS: &str = "app-menu-settings";
    const NEW_BOTTLE: &str = "app-menu-new-bottle";
    const PALETTE: &str = "app-menu-palette";
    const QUIT: &str = "app-menu-quit";

    /// Returns the ⌘Q item and the update item.
    pub fn install(app: &AppHandle) -> tauri::Result<(MenuItem<Wry>, MenuItem<Wry>)> {
        // Tauri's standard menu, with the app menu's last item (Quit) replaced.
        let menu: Menu<Wry> = Menu::default(app)?;
        let hide = MenuItem::with_id(app, HIDE, "Keep in Menu Bar", true, Some("CmdOrCtrl+Q"))?;
        let updates = MenuItem::with_id(app, UPDATES, "Check for Updates…", true, None::<&str>)?;
        if let Some(app_submenu) = menu.items()?.first().and_then(|i| i.as_submenu().cloned()) {
            // Vinter's own About window instead of the standard panel.
            app_submenu.remove_at(0)?;
            app_submenu.insert(
                &MenuItem::with_id(app, ABOUT, "About Vinter", true, None::<&str>)?,
                0,
            )?;
            // After "About Vinter".
            app_submenu.insert(&updates, 1)?;
            app_submenu.insert(&PredefinedMenuItem::separator(app)?, 2)?;
            app_submenu.insert(
                &MenuItem::with_id(app, SETTINGS, "Settings…", true, Some("CmdOrCtrl+,"))?,
                3,
            )?;
            let last = app_submenu.items()?.len().saturating_sub(1);
            app_submenu.remove_at(last)?;
            app_submenu.append(&hide)?;
            app_submenu.append(&PredefinedMenuItem::separator(app)?)?;
            app_submenu.append(&MenuItem::with_id(
                app,
                QUIT,
                "Quit Vinter",
                true,
                Some("Alt+CmdOrCtrl+Q"),
            )?)?;
        }
        // File menu: the window's own shortcuts, so they show in the menu.
        if let Some(file) = menu.items()?.get(1).and_then(|i| i.as_submenu().cloned()) {
            file.insert(
                &MenuItem::with_id(app, NEW_BOTTLE, "New Bottle…", true, Some("CmdOrCtrl+N"))?,
                0,
            )?;
            file.insert(
                &MenuItem::with_id(app, PALETTE, "Quick Open…", true, Some("CmdOrCtrl+K"))?,
                1,
            )?;
            file.insert(&PredefinedMenuItem::separator(app)?, 2)?;
        }
        app.set_menu(menu)?;
        app.on_menu_event(|app, event| match event.id().as_ref() {
            ABOUT => crate::windows::show_about(app),
            HIDE if super::is_shown(app) => crate::startup::hide_main_window(app),
            HIDE => app.exit(0),
            UPDATES => crate::windows::show_updates(app),
            SETTINGS => navigate(app, NavigateTo::Settings),
            NEW_BOTTLE => navigate(app, NavigateTo::NewBottle),
            PALETTE => navigate(app, NavigateTo::Palette),
            QUIT => app.exit(0),
            _ => {}
        });
        Ok((hide, updates))
    }

    fn navigate(app: &AppHandle, to: NavigateTo) {
        use tauri_specta::Event;
        crate::startup::show_main_window(app);
        let _ = crate::events::Navigate { to }.emit(app);
    }
}

/// True if the icon exists, so hiding the window keeps Vinter reachable.
pub fn is_shown(app: &AppHandle) -> bool {
    let state = app.state::<TrayState>();
    !state.hidden.load(Ordering::SeqCst) && state.entries.lock().unwrap().is_some()
}

/// Rebuilds the menu if the programs, recents or running programs changed.
/// Reads every bottle: call it off the main thread.
pub fn refresh(app: &AppHandle) {
    let entries = model(app);
    let state = app.state::<TrayState>();
    let mut last = state.entries.lock().unwrap();
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
        id if id.starts_with(SHOW_PREFIX) => {
            let (app, run_id) = (app.clone(), id[SHOW_PREFIX.len()..].to_string());
            tauri::async_runtime::spawn_blocking(move || crate::focus::show_run(&app, &run_id));
        }
        id if id.starts_with(STOP_PREFIX) => {
            let run_id = &id[STOP_PREFIX.len()..];
            let pid = app
                .state::<AppState>()
                .runs
                .lock()
                .unwrap()
                .get(run_id)
                .map(|r| r.pid);
            if let Some(Err(e)) = pid.map(vinter_core::process::terminate) {
                startup::notify(app, format!("Could not stop the program: {e}"));
            }
        }
        id => {
            let Some((bottle_id, index)) = id
                .strip_prefix(RUN_PREFIX)
                .or_else(|| id.strip_prefix(RECENT_PREFIX))
                .and_then(|rest| rest.split_once('/'))
                .and_then(|(b, i)| Some((b.to_string(), i.parse::<u32>().ok()?)))
            else {
                return;
            };
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(e) = launch_program(&app, bottle_id, index, true) {
                    startup::notify(&app, format!("Could not start the program: {}", e.message));
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(name: &str, bottle: &str, index: usize, last: Option<&str>) -> Found {
        Found {
            name: name.into(),
            bottle_name: bottle.to_uppercase(),
            bottle_id: bottle.into(),
            index,
            last_launched: last.map(str::to_string),
        }
    }

    #[test]
    fn model_has_running_recent_and_apps_by_bottle() {
        let found = [
            found("Word", "office", 0, Some("2026-10-02T10:00:00Z")),
            found("Excel", "office", 1, None),
            found("Setup", "games", 0, Some("2026-10-03T10:00:00Z")),
            found("Setup", "office", 2, Some("2026-10-01T10:00:00Z")),
        ];
        let run = RunInfo {
            run_id: "r1".into(),
            bottle_id: "office".into(),
            program_index: 0,
            name: "Word".into(),
            pid: 1,
            started_at: "2026-10-02T10:00:00Z".into(),
            log_id: "l".into(),
        };
        let m = build_model(&found, vec![run]);
        assert_eq!(m.running, vec![("r1".to_string(), "Word".to_string())]);
        let recent: Vec<&str> = m.recent.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(recent, ["Setup (GAMES)", "Word", "Setup (OFFICE)"]);
        let groups: Vec<(&str, Vec<&str>)> = m
            .groups
            .iter()
            .map(|(b, e)| (b.as_str(), e.iter().map(|e| e.label.as_str()).collect()))
            .collect();
        assert_eq!(
            groups,
            [
                ("GAMES", vec!["Setup"]),
                ("OFFICE", vec!["Excel", "Setup", "Word"])
            ]
        );
    }
}
