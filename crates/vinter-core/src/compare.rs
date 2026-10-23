//! Engine A/B comparison (the `engine-ab-compare` experimental flag): run
//! the same app in two bottles, typically one on Rosetta and one on Apple
//! Silicon Wine, and record how long it took to show a window and whether
//! it crashed. Results are kept in `engine-compare.json` in the data folder.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::engine::EngineKind;
use crate::error::IoResultExt;
use crate::experimental::{ExperimentalFlag, Flags};
use crate::{AppPaths, Error, Result, bottle, programs, runtime};

const FILE: &str = "engine-compare.json";
/// Comparisons kept.
const KEEP: usize = 50;
/// Wine's error messages, plus window traces to see when the first window
/// appears (`dlls/win32u/window.c`, channel `win`).
const TRIAL_WINEDEBUG: &str = "-all,err+all,trace+win";
/// `WS_VISIBLE`.
const WS_VISIBLE: u32 = 0x1000_0000;

/// What one run of the app did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Trial {
    pub bottle_id: String,
    pub bottle: String,
    pub program: String,
    pub engine: EngineKind,
    pub runtime: String,
    /// Seconds from start until it first showed a window, if it did.
    pub seconds_to_window: Option<f64>,
    /// It exited with an error code or Wine reported an unhandled exception.
    pub crashed: bool,
    /// It exited by itself before the trial ended.
    pub exited: bool,
    pub exit_code: Option<i32>,
    /// How long the trial watched it, in seconds.
    pub watched_seconds: f64,
    /// Frames per second, when the graphics backend reports them in the log.
    /// None of the current backends do, so this stays empty for now.
    pub fps: Option<f64>,
    /// Log of the run, for the Logs tab.
    pub log_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Comparison {
    /// RFC 3339.
    pub at: String,
    pub a: Trial,
    pub b: Trial,
}

fn ensure_enabled(paths: &AppPaths) -> Result<()> {
    if Flags::load(paths).is_enabled(ExperimentalFlag::EngineAbCompare) {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "comparing engines is experimental; turn on {} first",
            ExperimentalFlag::EngineAbCompare
        )))
    }
}

/// Runs program `a` in its bottle, then program `b` in its bottle, each for
/// up to `watch` (or until it exits), and saves the result. Each bottle's
/// programs are stopped after its run so the next one starts clean.
/// Blocks for up to twice `watch`.
pub fn compare(
    paths: &AppPaths,
    a: (&str, usize),
    b: (&str, usize),
    watch: Duration,
) -> Result<Comparison> {
    ensure_enabled(paths)?;
    let first = trial(paths, a.0, a.1, watch)?;
    let second = trial(paths, b.0, b.1, watch)?;
    let result = Comparison {
        at: humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
        a: first,
        b: second,
    };
    let mut all = read(paths)?;
    all.insert(0, result.clone());
    all.truncate(KEEP);
    write(paths, &all)?;
    Ok(result)
}

/// Past comparisons, newest first.
pub fn history(paths: &AppPaths) -> Result<Vec<Comparison>> {
    ensure_enabled(paths)?;
    read(paths)
}

fn trial(paths: &AppPaths, bottle_id: &str, index: usize, watch: Duration) -> Result<Trial> {
    let b = bottle::get(paths, bottle_id)?;
    let rt = runtime::get(paths, &b.runtime)?;
    let started = Instant::now();
    let shown: Arc<Mutex<Option<Duration>>> = Arc::default();
    let crashed_line = Arc::new(Mutex::new(false));
    let (tx, rx) = std::sync::mpsc::channel();

    let (shown_w, crashed_w) = (shown.clone(), crashed_line.clone());
    let launched = programs::launch_with(
        paths,
        bottle_id,
        index,
        &[("WINEDEBUG", TRIAL_WINEDEBUG)],
        move |_, line| {
            if is_window_shown(line) {
                shown_w.lock().unwrap().get_or_insert(started.elapsed());
            }
            if line.contains("Unhandled exception") || line.contains("Unhandled page fault") {
                *crashed_w.lock().unwrap() = true;
            }
        },
        move |code| {
            let _ = tx.send(code);
        },
    )?;

    let exit = rx.recv_timeout(watch).ok();
    let watched = started.elapsed();
    if exit.is_none() {
        let _ = crate::process::terminate(launched.pid);
    }
    // Stop anything the program left running, so the next trial starts clean.
    let _ = bottle::kill_all(paths, bottle_id);

    let exit_code = exit.flatten();
    let crashed = *crashed_line.lock().unwrap() || exit_code.is_some_and(|c| c != 0);
    Ok(Trial {
        bottle_id: b.id.clone(),
        bottle: b.name.clone(),
        program: launched.name,
        engine: b.engine,
        runtime: rt.name,
        seconds_to_window: shown
            .lock()
            .unwrap()
            .map(|d| (d.as_secs_f64() * 10.0).round() / 10.0),
        crashed,
        exited: exit.is_some(),
        exit_code,
        watched_seconds: (watched.as_secs_f64() * 10.0).round() / 10.0,
        fps: None,
        log_id: launched.log_id,
    })
}

/// A Wine `win` trace line saying a window became visible: `show_window`
/// with any command but `SW_HIDE` (0), or a window created with `WS_VISIBLE`.
fn is_window_shown(line: &str) -> bool {
    if let Some(rest) = line.split_once(":trace:win:show_window ").map(|(_, r)| r) {
        return rest
            .split_once("cmd=")
            .and_then(|(_, r)| r.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|n| n.parse::<u32>().ok())
            .is_some_and(|cmd| cmd != 0);
    }
    if let Some(rest) = line
        .split_once(":trace:win:NtUserCreateWindowEx ")
        .map(|(_, r)| r)
    {
        return rest
            .split_once(" style ")
            .and_then(|(_, r)| r.split(',').next())
            .and_then(|s| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
            .is_some_and(|style| style & WS_VISIBLE != 0);
    }
    false
}

fn read(paths: &AppPaths) -> Result<Vec<Comparison>> {
    let file = paths.data_dir().join(FILE);
    match std::fs::read_to_string(&file) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| Error::Config {
            path: file,
            message: e.to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e).at(&file),
    }
}

fn write(paths: &AppPaths, all: &[Comparison]) -> Result<()> {
    let dir = paths.data_dir();
    std::fs::create_dir_all(dir).at(dir)?;
    let file = dir.join(FILE);
    let tmp = dir.join(format!(".{FILE}.tmp"));
    std::fs::write(&tmp, serde_json::to_string_pretty(all).expect("serializes")).at(&tmp)?;
    std::fs::rename(&tmp, &file).at(&file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_the_first_visible_window() {
        // As Wine 11.10's win32u prints them.
        assert!(is_window_shown(
            "0024:trace:win:show_window hwnd=0x10050, cmd=5, was_visible 0"
        ));
        assert!(!is_window_shown(
            "0024:trace:win:show_window hwnd=0x10050, cmd=0, was_visible 1"
        ));
        assert!(is_window_shown(
            "0024:trace:win:NtUserCreateWindowEx ex_style 0, class_name L\"Notepad\", version (null), window_name L\"\", style 0x10cf0000, x 0, y 0"
        ));
        assert!(!is_window_shown(
            "0024:trace:win:NtUserCreateWindowEx ex_style 0, class_name L\"x\", version (null), window_name L\"\", style 0x84000000, x 0"
        ));
        assert!(!is_window_shown(
            "err:module:import_dll Library x not found"
        ));
    }

    #[test]
    fn needs_the_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        assert!(history(&paths).is_err());
        assert!(compare(&paths, ("a", 0), ("b", 0), Duration::from_secs(1)).is_err());
    }
}
