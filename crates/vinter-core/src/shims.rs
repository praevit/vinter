//! Launcher shims (the `launcher-shims` experimental flag): known
//! workarounds for game launchers, turned on per app. A shim only adds
//! launch arguments that the launcher's own vendor documents; it never
//! changes the program's saved settings, and each launch lists what it
//! added in its log.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::bottle::{self, ProgramDef};
use crate::error::IoResultExt;
use crate::experimental::{ExperimentalFlag, Flags};
use crate::{AppPaths, Error, Result};

const FILE: &str = "launcher-shims.json";

/// One known workaround.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Shim {
    pub id: &'static str,
    pub launcher: &'static str,
    pub title: &'static str,
    pub why: &'static str,
    /// Arguments added at launch.
    pub args: Vec<&'static str>,
    /// Where the arguments are documented.
    pub source: &'static str,
    /// File names (lower case) of the launcher's program.
    #[serde(skip)]
    pub exe_names: &'static [&'static str],
}

/// Every shim Vinter knows. Each must cite the vendor's documentation.
pub fn all() -> Vec<Shim> {
    vec![Shim {
        id: "steam-cef-no-gpu",
        launcher: "Steam",
        title: "Draw Steam's window without the GPU",
        why: "Steam's window is a built-in web browser (CEF), which often shows blank or \
              freezes under Wine. Software rendering avoids that, at some speed cost in \
              the store and library pages.",
        args: vec!["-cef-disable-gpu", "-cef-disable-gpu-compositing"],
        source: "https://developer.valvesoftware.com/wiki/Command_line_options_(Steam)",
        exe_names: &["steam.exe"],
    }]
}

/// Shims that fit a program, by its file name.
pub fn for_program(program: &ProgramDef) -> Vec<Shim> {
    let name = program
        .exe
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    all()
        .into_iter()
        .filter(|s| s.exe_names.contains(&name.as_str()))
        .collect()
}

fn enabled(paths: &AppPaths) -> bool {
    Flags::load(paths).is_enabled(ExperimentalFlag::LauncherShims)
}

type Store = BTreeMap<String, Vec<String>>;

fn file(paths: &AppPaths, bottle_id: &str) -> Result<PathBuf> {
    Ok(bottle::dir(paths, bottle_id)?.join(FILE))
}

fn read(paths: &AppPaths, bottle_id: &str) -> Result<Store> {
    let path = file(paths, bottle_id)?;
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| Error::Config {
            path,
            message: e.to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::new()),
        Err(e) => Err(e).at(&path),
    }
}

/// Ids of the shims turned on for a bottle's program.
pub fn turned_on(paths: &AppPaths, bottle_id: &str, index: usize) -> Result<Vec<String>> {
    let b = bottle::get(paths, bottle_id)?;
    let program = b.programs.get(index).ok_or(Error::ProgramNotFound(index))?;
    Ok(read(paths, bottle_id)?
        .remove(&program.exe)
        .unwrap_or_default())
}

/// Turns a shim on or off for a program. Only shims that fit it.
pub fn set(
    paths: &AppPaths,
    bottle_id: &str,
    index: usize,
    id: &str,
    on: bool,
) -> Result<Vec<String>> {
    if !enabled(paths) {
        return Err(Error::Unsupported(format!(
            "launcher fixes are experimental; turn on {} first",
            ExperimentalFlag::LauncherShims
        )));
    }
    let b = bottle::get(paths, bottle_id)?;
    let program = b.programs.get(index).ok_or(Error::ProgramNotFound(index))?;
    if !for_program(program).iter().any(|s| s.id == id) {
        return Err(Error::InvalidSetting(format!(
            "{id} is not a fix for {}",
            program.name
        )));
    }
    let mut store = read(paths, bottle_id)?;
    let ids = store.entry(program.exe.clone()).or_default();
    let before = ids.iter().any(|i| i == id);
    ids.retain(|i| i != id);
    if on {
        ids.push(id.to_string());
        ids.sort();
    }
    let result = ids.clone();
    store.retain(|_, v| !v.is_empty());
    let path = file(paths, bottle_id)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&store).expect("serializes"),
    )
    .at(&tmp)?;
    std::fs::rename(&tmp, &path).at(&path)?;
    bottle::history::note(
        paths,
        bottle_id,
        bottle::history::Change::Shim {
            program: program.exe.clone(),
            shim: id.to_string(),
            before,
            after: on,
        },
    );
    Ok(result)
}

/// The shims a launch applies: only with the flag on, and only ones that
/// still fit the program. Each comes with the arguments it adds.
pub fn for_launch(paths: &AppPaths, bottle_id: &str, program: &ProgramDef) -> Vec<Shim> {
    if !enabled(paths) {
        return Vec::new();
    }
    let ids = read(paths, bottle_id)
        .ok()
        .and_then(|mut s| s.remove(&program.exe))
        .unwrap_or_default();
    for_program(program)
        .into_iter()
        .filter(|s| ids.iter().any(|i| i == s.id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bottle::Bottle;

    fn steam(exe: &str) -> ProgramDef {
        ProgramDef {
            name: "Steam".into(),
            exe: exe.into(),
            args: vec![],
            icon: None,
            pinned: false,
            arm64_build: None,
            ..Default::default()
        }
    }

    #[test]
    fn every_shim_cites_its_source() {
        for s in all() {
            assert!(s.source.starts_with("https://"), "{}", s.id);
            assert!(!s.args.is_empty() && !s.exe_names.is_empty());
        }
    }

    #[test]
    fn matches_by_file_name() {
        assert_eq!(
            for_program(&steam(r"C:\Program Files (x86)\Steam\Steam.exe")).len(),
            1
        );
        assert!(for_program(&steam(r"C:\Games\notsteam.exe")).is_empty());
    }

    #[test]
    fn applied_only_with_the_flag_and_when_turned_on() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let dir = bottle::dir(&paths, "b").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let mut b: Bottle = toml::from_str(
            "id = \"b\"\nname = \"B\"\nruntime = \"rt\"\ncreated_at = \"2026-10-23T00:00:00Z\"\n",
        )
        .unwrap();
        b.programs
            .push(steam(r"C:\Program Files (x86)\Steam\steam.exe"));
        b.save(&dir).unwrap();
        let program = b.programs[0].clone();

        assert!(
            set(&paths, "b", 0, "steam-cef-no-gpu", true).is_err(),
            "flag off"
        );
        crate::experimental::set(&paths, ExperimentalFlag::LauncherShims, true).unwrap();
        assert!(
            for_launch(&paths, "b", &program).is_empty(),
            "not turned on yet"
        );
        assert!(set(&paths, "b", 0, "made-up", true).is_err());
        set(&paths, "b", 0, "steam-cef-no-gpu", true).unwrap();
        let applied = for_launch(&paths, "b", &program);
        assert_eq!(
            applied[0].args,
            ["-cef-disable-gpu", "-cef-disable-gpu-compositing"]
        );
        // The program's own arguments are untouched.
        assert!(
            bottle::get(&paths, "b").unwrap().programs[0]
                .args
                .is_empty()
        );

        crate::experimental::set(&paths, ExperimentalFlag::LauncherShims, false).unwrap();
        assert!(for_launch(&paths, "b", &program).is_empty());
    }
}
