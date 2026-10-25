//! Drive letters: folders on this computer shown to a bottle's programs as
//! drives of their own. Wine reads them from symlinks in
//! `<prefix>/dosdevices/` (`d:` -> `/Users/me/Projects`); it maps `c:` to the
//! bottle's `drive_c` and `z:` to `/`. Wine guesses a folder drive's type
//! itself, so nothing goes in the registry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{Bottle, DriveMapping, PREFIX_DIR};
use crate::error::IoResultExt;
use crate::{AppPaths, Error, Result};

/// The drive Wine maps to the whole computer.
pub const ROOT_LETTER: char = 'z';
const ROOT_TARGET: &str = "/";

/// One change to `dosdevices/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Remove the `<letter>:` link.
    Remove(char),
    /// Link `<letter>:` to a folder.
    Link(char, PathBuf),
}

/// The letter of a drive name such as `D`, `d` or `d:`, lowercase.
fn letter_of(s: &str) -> Option<char> {
    let s = s.trim().trim_end_matches(':');
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => Some(c.to_ascii_lowercase()),
        _ => None,
    }
}

/// Checks drives chosen by the user: letters `D` to `Y`, each once, and
/// absolute paths of existing folders. Letters come back in upper case.
pub fn validate(drives: &[DriveMapping]) -> Result<Vec<DriveMapping>> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for drive in drives {
        let letter = letter_of(&drive.letter)
            .filter(|l| ('d'..='y').contains(l))
            .ok_or_else(|| {
                Error::InvalidSetting(format!(
                    "{:?} is not a drive letter Vinter can use; pick one from D to Y",
                    drive.letter
                ))
            })?;
        if seen.contains(&letter) {
            return Err(Error::InvalidSetting(format!(
                "{}: is used twice",
                letter.to_ascii_uppercase()
            )));
        }
        seen.push(letter);
        let path = Path::new(drive.path.trim());
        if !path.is_absolute() || drive.path.contains('\0') || !path.is_dir() {
            return Err(Error::InvalidSetting(format!(
                "{} is not a folder",
                drive.path
            )));
        }
        out.push(DriveMapping {
            letter: letter.to_ascii_uppercase().to_string(),
            path: path.display().to_string(),
        });
    }
    Ok(out)
}

/// The drive links in `dosdevices/` now, by lowercase letter.
pub fn current_links(prefix: &Path) -> BTreeMap<char, PathBuf> {
    let mut links = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(prefix.join("dosdevices")) else {
        return links;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(letter) = name.strip_suffix(':').and_then(letter_of) else {
            continue;
        };
        if let Ok(target) = std::fs::read_link(entry.path()) {
            links.insert(letter, target);
        }
    }
    links
}

/// What to change in `dosdevices/` to go from `current` to the drives in
/// `wanted`. Letters in `previous` (what Vinter mapped before) that are no
/// longer wanted are removed; other links (`c:`, ones made in Wine's own
/// settings) are left alone. `Z:` is removed when `hide_root` is on and it
/// still points at `/`, and put back when it is off.
pub fn plan(
    current: &BTreeMap<char, PathBuf>,
    previous: &[DriveMapping],
    wanted: &[DriveMapping],
    hide_root: bool,
) -> Vec<Action> {
    let wanted: BTreeMap<char, PathBuf> = wanted
        .iter()
        .filter_map(|d| Some((letter_of(&d.letter)?, PathBuf::from(&d.path))))
        .filter(|(l, _)| *l != 'c')
        .collect();
    let mut state = current.clone();
    let mut actions = Vec::new();
    for letter in previous.iter().filter_map(|d| letter_of(&d.letter)) {
        if letter != 'c' && !wanted.contains_key(&letter) && state.remove(&letter).is_some() {
            actions.push(Action::Remove(letter));
        }
    }
    for (&letter, path) in &wanted {
        match state.get(&letter) {
            Some(target) if target == path => continue,
            Some(_) => actions.push(Action::Remove(letter)),
            None => {}
        }
        actions.push(Action::Link(letter, path.clone()));
        state.insert(letter, path.clone());
    }
    if !wanted.contains_key(&ROOT_LETTER) {
        match state.get(&ROOT_LETTER) {
            Some(target) if hide_root && target == Path::new(ROOT_TARGET) => {
                actions.push(Action::Remove(ROOT_LETTER));
            }
            None if !hide_root => {
                actions.push(Action::Link(ROOT_LETTER, PathBuf::from(ROOT_TARGET)));
            }
            _ => {}
        }
    }
    actions
}

/// Carries out a [`plan`].
pub fn apply(prefix: &Path, actions: &[Action]) -> Result<()> {
    let dir = prefix.join("dosdevices");
    if !actions.is_empty() {
        std::fs::create_dir_all(&dir).at(&dir)?;
    }
    for action in actions {
        match action {
            Action::Remove(letter) => {
                let link = dir.join(format!("{letter}:"));
                match std::fs::remove_file(&link) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                        return Err(e).at(&link);
                    }
                    _ => {}
                }
            }
            Action::Link(letter, target) => {
                let link = dir.join(format!("{letter}:"));
                #[cfg(unix)]
                std::os::unix::fs::symlink(target, &link).at(&link)?;
                #[cfg(not(unix))]
                let _ = (target, &link);
            }
        }
    }
    Ok(())
}

/// Changes a bottle's drives. Running programs may not see the change until
/// they restart.
pub fn set(
    paths: &AppPaths,
    id: &str,
    drives: Vec<DriveMapping>,
    hide_root_drive: bool,
) -> Result<Bottle> {
    let mut bottle = super::get(paths, id)?;
    let drives = validate(&drives)?;
    let dir = super::dir(paths, id)?;
    let prefix = dir.join(PREFIX_DIR);
    let actions = plan(
        &current_links(&prefix),
        &bottle.drives,
        &drives,
        hide_root_drive,
    );
    apply(&prefix, &actions)?;
    bottle.drives = drives;
    bottle.hide_root_drive = hide_root_drive;
    bottle.save(&dir)?;
    Ok(bottle)
}

/// Puts the bottle's drives back if something changed them (best effort,
/// before a program starts). Only for prefixes Wine has set up.
pub fn ensure(prefix: &Path, bottle: &Bottle) {
    if bottle.drives.is_empty() && !bottle.hide_root_drive {
        return;
    }
    if !prefix.join("dosdevices").is_dir() {
        return;
    }
    let actions = plan(
        &current_links(prefix),
        &[],
        &bottle.drives,
        bottle.hide_root_drive,
    );
    let _ = apply(prefix, &actions);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(letter: &str, path: &str) -> DriveMapping {
        DriveMapping {
            letter: letter.into(),
            path: path.into(),
        }
    }

    fn links(pairs: &[(char, &str)]) -> BTreeMap<char, PathBuf> {
        pairs.iter().map(|(l, p)| (*l, PathBuf::from(p))).collect()
    }

    #[test]
    fn adds_and_replaces_folder_drives() {
        let current = links(&[('c', "../drive_c"), ('z', "/"), ('e', "/old")]);
        let wanted = [drive("D", "/Users/me/Projects"), drive("e", "/new")];
        assert_eq!(
            plan(&current, &[], &wanted, false),
            vec![
                Action::Link('d', "/Users/me/Projects".into()),
                Action::Remove('e'),
                Action::Link('e', "/new".into()),
            ]
        );
        // Already in place: nothing to do.
        let done = links(&[('c', "../drive_c"), ('z', "/"), ('d', "/Users/me/Projects")]);
        assert!(plan(&done, &[], &wanted[..1], false).is_empty());
    }

    #[test]
    fn removes_only_drives_vinter_made() {
        let current = links(&[('c', "../drive_c"), ('z', "/"), ('d', "/a"), ('f', "/mine")]);
        // D: was Vinter's; F: was made in Wine's settings and stays.
        assert_eq!(
            plan(&current, &[drive("D", "/a")], &[], false),
            vec![Action::Remove('d')]
        );
        // C: is never touched, even if asked.
        assert!(plan(&current, &[drive("C", "/x")], &[drive("C", "/y")], false).is_empty());
    }

    #[test]
    fn hides_and_restores_the_root_drive() {
        let with_z = links(&[('c', "../drive_c"), ('z', "/")]);
        assert_eq!(plan(&with_z, &[], &[], true), vec![Action::Remove('z')]);
        let without_z = links(&[('c', "../drive_c")]);
        assert!(plan(&without_z, &[], &[], true).is_empty());
        assert_eq!(
            plan(&without_z, &[], &[], false),
            vec![Action::Link('z', "/".into())]
        );
        // A Z: pointing somewhere else was set up on purpose; leave it.
        let custom = links(&[('z', "/Volumes/Games")]);
        assert!(plan(&custom, &[], &[], true).is_empty());
    }

    #[test]
    fn validation() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().display().to_string();
        let ok = validate(&[drive("d:", &folder)]).unwrap();
        assert_eq!(ok, vec![drive("D", &folder)]);
        for bad in [
            vec![drive("C", &folder)],
            vec![drive("Z", &folder)],
            vec![drive("DD", &folder)],
            vec![drive("D", "relative/path")],
            vec![drive(
                "D",
                &tmp.path().join("missing").display().to_string(),
            )],
            vec![drive("D", &folder), drive("d", &folder)],
        ] {
            assert!(validate(&bad).is_err(), "{bad:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn applies_links() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path();
        std::fs::create_dir_all(prefix.join("dosdevices")).unwrap();
        std::os::unix::fs::symlink("/", prefix.join("dosdevices/z:")).unwrap();
        let actions = plan(&current_links(prefix), &[], &[drive("D", "/tmp")], true);
        apply(prefix, &actions).unwrap();
        assert_eq!(current_links(prefix), links(&[('d', "/tmp")]));
    }
}
