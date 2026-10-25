//! Compatibility notes: the user's own record of how well each app works, with the engine and graphics
//! backend it was tried on. Kept in `compat-notes.json` in the data folder.
//! Nothing is sent anywhere.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::bottle::{self, Graphics};
use crate::engine::EngineKind;
use crate::error::IoResultExt;
use crate::{AppPaths, Error, Result};

const FILE: &str = "compat-notes.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Rating {
    Works,
    Partly,
    Broken,
}

impl Rating {
    pub const ALL: [Self; 3] = [Self::Works, Self::Partly, Self::Broken];

    pub fn id(self) -> &'static str {
        match self {
            Self::Works => "works",
            Self::Partly => "partly",
            Self::Broken => "broken",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.id() == s)
    }
}

/// One app's note. An app is a program in a bottle, found by its Windows path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CompatNote {
    pub bottle_id: String,
    pub app: String,
    /// Windows path of the program.
    pub exe: String,
    pub rating: Rating,
    /// Engine and backend it was rated on (the bottle's, when saved).
    pub engine: EngineKind,
    pub backend: Graphics,
    pub note: String,
    /// RFC 3339.
    pub updated_at: String,
}

fn read(paths: &AppPaths) -> Result<Vec<CompatNote>> {
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

fn write(paths: &AppPaths, notes: &[CompatNote]) -> Result<()> {
    let dir = paths.data_dir();
    std::fs::create_dir_all(dir).at(dir)?;
    let file = dir.join(FILE);
    let tmp = dir.join(format!(".{FILE}.tmp"));
    let text = serde_json::to_string_pretty(notes).expect("notes serialize");
    std::fs::write(&tmp, text).at(&tmp)?;
    std::fs::rename(&tmp, &file).at(&file)
}

/// Every note, newest first.
pub fn list(paths: &AppPaths) -> Result<Vec<CompatNote>> {
    let mut notes = read(paths)?;
    notes.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(notes)
}

/// Rates program number `index` of a bottle, with the bottle's current
/// engine and backend. Replaces that program's earlier note.
pub fn set(
    paths: &AppPaths,
    bottle_id: &str,
    index: usize,
    rating: Rating,
    note: &str,
) -> Result<CompatNote> {
    let b = bottle::get(paths, bottle_id)?;
    let program = b.programs.get(index).ok_or(Error::ProgramNotFound(index))?;
    let entry = CompatNote {
        bottle_id: b.id.clone(),
        app: program.name.clone(),
        exe: program.exe.clone(),
        rating,
        engine: b.engine,
        backend: b.graphics,
        note: note.trim().chars().take(2000).collect(),
        updated_at: humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
    };
    let mut notes = read(paths)?;
    notes.retain(|n| !(n.bottle_id == entry.bottle_id && n.exe == entry.exe));
    notes.push(entry.clone());
    write(paths, &notes)?;
    Ok(entry)
}

/// Removes the note for program number `index` of a bottle, if any.
pub fn remove(paths: &AppPaths, bottle_id: &str, index: usize) -> Result<()> {
    let b = bottle::get(paths, bottle_id)?;
    let program = b.programs.get(index).ok_or(Error::ProgramNotFound(index))?;
    let mut notes = read(paths)?;
    notes.retain(|n| !(n.bottle_id == b.id && n.exe == program.exe));
    write(paths, &notes)
}

/// A bottle's notes, newest first.
pub fn for_bottle(paths: &AppPaths, bottle_id: &str) -> Result<Vec<CompatNote>> {
    let mut notes = list(paths)?;
    notes.retain(|n| n.bottle_id == bottle_id);
    Ok(notes)
}

/// Writes every note as JSON to `dest`.
pub fn export(paths: &AppPaths, dest: &Path) -> Result<()> {
    let notes = list(paths)?;
    let text = serde_json::to_string_pretty(&notes).expect("notes serialize");
    std::fs::write(dest, text).at(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bottle::{Bottle, ProgramDef};

    fn setup() -> (tempfile::TempDir, AppPaths) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let dir = bottle::dir(&paths, "b").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let mut b: Bottle = toml::from_str(
            "id = \"b\"\nname = \"Games\"\nruntime = \"rt\"\nengine = \"arm64-fex\"\ngraphics = \"wined3d\"\ncreated_at = \"2026-10-23T00:00:00Z\"\n",
        )
        .unwrap();
        b.programs.push(ProgramDef {
            name: "Notepad++".into(),
            exe: r"C:\Program Files\Notepad++\notepad++.exe".into(),
            args: vec![],
            icon: None,
            pinned: false,
            arm64_build: None,
            last_launched: None,
            launch_count: 0,
        });
        b.save(&dir).unwrap();
        (tmp, paths)
    }

    #[test]
    fn none_until_one_is_saved() {
        let (_tmp, paths) = setup();
        assert!(list(&paths).unwrap().is_empty());
        assert!(for_bottle(&paths, "b").unwrap().is_empty());
        assert!(!paths.data_dir().join(FILE).exists());
    }

    #[test]
    fn rate_replace_remove_and_export() {
        let (tmp, paths) = setup();

        let n = set(&paths, "b", 0, Rating::Partly, "  menus flicker  ").unwrap();
        assert_eq!(n.app, "Notepad++");
        assert_eq!(n.engine, EngineKind::Arm64Fex);
        assert_eq!(n.backend, Graphics::Wined3d);
        assert_eq!(n.note, "menus flicker");

        set(&paths, "b", 0, Rating::Works, "fine now").unwrap();
        let all = list(&paths).unwrap();
        assert_eq!(all.len(), 1, "one note per program");
        assert_eq!(all[0].rating, Rating::Works);

        let out = tmp.path().join("notes.json");
        export(&paths, &out).unwrap();
        let back: Vec<CompatNote> =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(back, all);

        remove(&paths, "b", 0).unwrap();
        assert!(list(&paths).unwrap().is_empty());
        assert!(set(&paths, "b", 5, Rating::Works, "").is_err());
    }
}
