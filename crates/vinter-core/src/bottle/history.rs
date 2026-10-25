//! Settings history: a short journal of a bottle's settings changes, each
//! with the value before and after, so one can be put back. Kept as JSON
//! lines in `<bottle>/history.jsonl`, oldest first, at most [`KEEP`] entries.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{BottleOptions, Graphics, WindowsVersion, dir, get};
use crate::error::IoResultExt;
use crate::{AppPaths, Error, Result};

pub const HISTORY_FILE: &str = "history.jsonl";
/// Entries kept per bottle; older ones are dropped.
pub const KEEP: usize = 50;

/// One setting, before and after. Programs are named by their Windows path,
/// as the per-program stores key them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "setting", rename_all = "snake_case")]
pub enum Change {
    WindowsVersion {
        before: WindowsVersion,
        after: WindowsVersion,
    },
    Graphics {
        before: Graphics,
        after: Graphics,
    },
    Runtime {
        before: String,
        after: String,
    },
    Env {
        before: BTreeMap<String, String>,
        after: BTreeMap<String, String>,
    },
    DllOverrides {
        before: BTreeMap<String, String>,
        after: BTreeMap<String, String>,
    },
    Options {
        before: BottleOptions,
        after: BottleOptions,
    },
    Shim {
        program: String,
        shim: String,
        before: bool,
        after: bool,
    },
    FexSetting {
        program: String,
        name: String,
        before: Option<String>,
        after: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// RFC 3339 with microseconds.
    pub at: String,
    #[serde(flatten)]
    pub change: Change,
}

/// What the History section shows for one entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HistoryItem {
    pub id: String,
    /// RFC 3339.
    pub at: String,
    /// The setting, e.g. "Windows version".
    pub title: String,
    /// What changed, e.g. "Windows 10 → Windows 11".
    pub detail: String,
    /// Reverting runs Wine for a few seconds (Windows version, display
    /// options, graphics), so the app runs it as a job.
    pub slow: bool,
}

fn file(paths: &AppPaths, bottle_id: &str) -> Result<PathBuf> {
    Ok(dir(paths, bottle_id)?.join(HISTORY_FILE))
}

/// Every entry, oldest first. Lines that cannot be read are skipped.
pub fn read(paths: &AppPaths, bottle_id: &str) -> Result<Vec<Entry>> {
    let path = file(paths, bottle_id)?;
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e).at(&path),
    }
}

/// Appends a change, unless nothing changed. Past [`KEEP`] entries the file
/// is rewritten without the oldest.
pub fn record(paths: &AppPaths, bottle_id: &str, change: Change) -> Result<()> {
    if change.is_noop() {
        return Ok(());
    }
    let entry = Entry {
        id: uuid::Uuid::new_v4().to_string(),
        at: humantime::format_rfc3339_micros(std::time::SystemTime::now()).to_string(),
        change,
    };
    let path = file(paths, bottle_id)?;
    let line = serde_json::to_string(&entry).expect("an entry serializes");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .at(&path)?;
    writeln!(f, "{line}").at(&path)?;
    drop(f);

    let entries = read(paths, bottle_id)?;
    if entries.len() > KEEP {
        let text: String = entries[entries.len() - KEEP..]
            .iter()
            .map(|e| serde_json::to_string(e).expect("an entry serializes") + "\n")
            .collect();
        let tmp = path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, text).at(&tmp)?;
        std::fs::rename(&tmp, &path).at(&path)?;
    }
    Ok(())
}

/// Records a change without failing the setting it belongs to: the change
/// itself already happened.
pub(crate) fn note(paths: &AppPaths, bottle_id: &str, change: Change) {
    let _ = record(paths, bottle_id, change);
}

/// The entries, newest first, as the History section shows them.
pub fn list(paths: &AppPaths, bottle_id: &str) -> Result<Vec<HistoryItem>> {
    let bottle = get(paths, bottle_id)?;
    let program_name = |exe: &str| {
        bottle
            .programs
            .iter()
            .find(|p| p.exe == exe)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| exe.rsplit('\\').next().unwrap_or(exe).to_string())
    };
    Ok(read(paths, bottle_id)?
        .into_iter()
        .rev()
        .map(|e| {
            let (title, detail) = e.change.describe(&program_name);
            HistoryItem {
                slow: e.change.is_slow(),
                id: e.id,
                at: e.at,
                title,
                detail,
            }
        })
        .collect())
}

/// Finds an entry by id.
pub fn find(paths: &AppPaths, bottle_id: &str, entry_id: &str) -> Result<Entry> {
    read(paths, bottle_id)?
        .into_iter()
        .find(|e| e.id == entry_id)
        .ok_or_else(|| Error::InvalidSetting("that change is no longer in the history".into()))
}

/// Puts the setting of an entry back to its value before the change. This is
/// itself a change, so it is recorded too (and can be reverted). Windows
/// version, display options and graphics run Wine; close the bottle's
/// programs first.
pub fn revert(paths: &AppPaths, bottle_id: &str, entry_id: &str) -> Result<()> {
    let entry = find(paths, bottle_id, entry_id)?;
    let index_of = |exe: &str| {
        get(paths, bottle_id)?
            .programs
            .iter()
            .position(|p| p.exe == exe)
            .ok_or_else(|| Error::InvalidSetting("that program is no longer in this bottle".into()))
    };
    match entry.change {
        Change::WindowsVersion { before, .. } => {
            super::set_windows_version(paths, bottle_id, before).map(drop)
        }
        Change::Graphics { before, .. } => {
            crate::graphics::apply(paths, bottle_id, before).map(drop)
        }
        Change::Runtime { before, .. } => super::set_runtime(paths, bottle_id, &before).map(drop),
        Change::Env { before, .. } => super::set_env(paths, bottle_id, before).map(drop),
        Change::DllOverrides { before, .. } => {
            super::set_dll_overrides(paths, bottle_id, before).map(drop)
        }
        Change::Options { before, .. } => super::set_options(paths, bottle_id, before).map(drop),
        Change::Shim {
            program,
            shim,
            before,
            ..
        } => crate::shims::set(paths, bottle_id, index_of(&program)?, &shim, before).map(drop),
        Change::FexSetting {
            program,
            name,
            before,
            ..
        } => crate::fex_config::set(
            paths,
            bottle_id,
            index_of(&program)?,
            &name,
            before.as_deref(),
        )
        .map(drop),
    }
}

impl Change {
    fn is_noop(&self) -> bool {
        match self {
            Self::WindowsVersion { before, after } => before == after,
            Self::Graphics { before, after } => before == after,
            Self::Runtime { before, after } => before == after,
            Self::Env { before, after } | Self::DllOverrides { before, after } => before == after,
            Self::Options { before, after } => before == after,
            Self::Shim { before, after, .. } => before == after,
            Self::FexSetting { before, after, .. } => before == after,
        }
    }

    /// Whether reverting it runs Wine.
    pub fn is_slow(&self) -> bool {
        matches!(
            self,
            Self::WindowsVersion { .. } | Self::Graphics { .. } | Self::Options { .. }
        )
    }

    /// Title and detail for the History section.
    fn describe(&self, program_name: &dyn Fn(&str) -> String) -> (String, String) {
        match self {
            Self::WindowsVersion { before, after } => (
                "Windows version".into(),
                arrow(windows_label(*before), windows_label(*after)),
            ),
            Self::Graphics { before, after } => {
                ("Graphics".into(), arrow(before.name(), after.name()))
            }
            Self::Runtime { before, after } => ("Runtime".into(), arrow(before, after)),
            Self::Env { before, after } => {
                ("Environment variables".into(), map_diff(before, after))
            }
            Self::DllOverrides { before, after } => {
                ("DLL overrides".into(), map_diff(before, after))
            }
            Self::Options { before, after } => {
                ("Display and keyboard".into(), options_diff(before, after))
            }
            Self::Shim {
                program,
                shim,
                after,
                ..
            } => (
                format!("Launcher fix for {}", program_name(program)),
                format!("{shim} {}", if *after { "on" } else { "off" }),
            ),
            Self::FexSetting {
                program,
                name,
                before,
                after,
            } => {
                let value = |v: &Option<String>| v.clone().unwrap_or_else(|| "default".into());
                (
                    format!("FEX setting for {}", program_name(program)),
                    format!("{name}: {}", arrow(&value(before), &value(after))),
                )
            }
        }
    }
}

fn windows_label(v: WindowsVersion) -> &'static str {
    match v {
        WindowsVersion::Win11 => "Windows 11",
        WindowsVersion::Win10 => "Windows 10",
        WindowsVersion::Win81 => "Windows 8.1",
        WindowsVersion::Win8 => "Windows 8",
        WindowsVersion::Win7 => "Windows 7",
        WindowsVersion::Winxp64 => "Windows XP",
    }
}

/// "added A=1, removed B, C: x → y".
fn map_diff(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> String {
    let mut parts = Vec::new();
    for (k, v) in after {
        match before.get(k) {
            None => parts.push(format!("added {k}={v}")),
            Some(old) if old != v => parts.push(format!("{k}: {old} → {v}")),
            Some(_) => {}
        }
    }
    for k in before.keys().filter(|k| !after.contains_key(*k)) {
        parts.push(format!("removed {k}"));
    }
    if parts.is_empty() {
        "no change".into()
    } else {
        parts.join(", ")
    }
}

fn options_diff(before: &BottleOptions, after: &BottleOptions) -> String {
    let on_off = |on: bool| if on { "on" } else { "off" };
    let mut parts = Vec::new();
    for (name, a, b) in [
        ("Retina mode", before.retina_mode, after.retina_mode),
        ("⌘ as Ctrl", before.command_as_ctrl, after.command_as_ctrl),
        ("⌥ as Alt", before.option_as_alt, after.option_as_alt),
        ("Frame rate", before.show_fps, after.show_fps),
    ] {
        if a != b {
            parts.push(format!("{name} {}", on_off(b)));
        }
    }
    if before.wine_debug != after.wine_debug {
        parts.push(format!(
            "Wine log: {}",
            arrow(before.wine_debug.id(), after.wine_debug.id())
        ));
    }
    if parts.is_empty() {
        "no change".into()
    } else {
        parts.join(", ")
    }
}

fn arrow(a: &str, b: &str) -> String {
    format!("{a} → {b}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bottle::{Bottle, ProgramDef};

    fn setup() -> (tempfile::TempDir, AppPaths) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let d = dir(&paths, "b").unwrap();
        std::fs::create_dir_all(&d).unwrap();
        let mut b: Bottle = toml::from_str(
            "id = \"b\"\nname = \"B\"\nruntime = \"rt\"\ncreated_at = \"2026-10-23T00:00:00Z\"\n",
        )
        .unwrap();
        b.programs.push(ProgramDef {
            name: "Game".into(),
            exe: r"C:\Games\game.exe".into(),
            args: vec![],
            icon: None,
            pinned: false,
            arm64_build: None,
            last_launched: None,
            launch_count: 0,
        });
        b.save(&d).unwrap();
        (tmp, paths)
    }

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn setters_record_and_revert_puts_the_value_back() {
        let (_tmp, paths) = setup();
        super::super::set_env(&paths, "b", map(&[("DXVK_HUD", "fps")])).unwrap();
        super::super::set_env(&paths, "b", map(&[("DXVK_HUD", "full"), ("A", "1")])).unwrap();
        // Saving the same value again is not a change.
        super::super::set_env(&paths, "b", map(&[("DXVK_HUD", "full"), ("A", "1")])).unwrap();
        super::super::set_dll_overrides(&paths, "b", map(&[("d3d11", "n,b")])).unwrap();

        let items = list(&paths, "b").unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].title, "DLL overrides");
        assert_eq!(items[0].detail, "added d3d11=n,b");
        assert_eq!(items[1].title, "Environment variables");
        assert_eq!(items[1].detail, "added A=1, DXVK_HUD: fps → full");
        assert!(!items[1].slow);

        revert(&paths, "b", &items[1].id).unwrap();
        let b = get(&paths, "b").unwrap();
        assert_eq!(b.env, map(&[("DXVK_HUD", "fps")]));
        // The revert is a change of its own.
        let items = list(&paths, "b").unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].detail, "DXVK_HUD: full → fps, removed A");

        assert!(revert(&paths, "b", "nope").is_err());
    }

    #[test]
    fn keeps_the_newest_entries() {
        let (_tmp, paths) = setup();
        for i in 0..KEEP + 7 {
            record(
                &paths,
                "b",
                Change::Runtime {
                    before: format!("rt{i}"),
                    after: format!("rt{}", i + 1),
                },
            )
            .unwrap();
        }
        let entries = read(&paths, "b").unwrap();
        assert_eq!(entries.len(), KEEP);
        assert_eq!(
            entries.last().unwrap().change,
            Change::Runtime {
                before: format!("rt{}", KEEP + 6),
                after: format!("rt{}", KEEP + 7),
            }
        );
    }

    #[test]
    fn format_is_one_json_object_per_line() {
        let (_tmp, paths) = setup();
        record(
            &paths,
            "b",
            Change::FexSetting {
                program: r"C:\Games\game.exe".into(),
                name: "Multiblock".into(),
                before: None,
                after: Some("1".into()),
            },
        )
        .unwrap();
        let text = std::fs::read_to_string(file(&paths, "b").unwrap()).unwrap();
        assert_eq!(text.lines().count(), 1);
        let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(v["setting"], "fex_setting");
        assert_eq!(v["after"], "1");
        assert!(v["before"].is_null());
        // A broken line is skipped, not fatal.
        std::fs::write(file(&paths, "b").unwrap(), format!("{text}not json\n")).unwrap();
        let items = list(&paths, "b").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "FEX setting for Game");
        assert_eq!(items[0].detail, "Multiblock: default → 1");
    }
}
