//! Newer builds of installed runtimes, and moving bottles onto them.

use std::cmp::Ordering;

use serde::Serialize;

use super::Runtime;
use super::manifest::{self, RuntimeInfo};
use crate::{AppPaths, Result, bottle};

/// A newer build of the same flavor as an installed runtime.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RuntimeUpdate {
    /// The installed runtime it replaces.
    pub installed_id: String,
    pub installed_version: String,
    pub newer: RuntimeInfo,
    /// Names of the bottles that use the installed runtime now.
    pub bottles: Vec<String>,
}

/// Compares versions like `11.0` and `11.18` part by part, as numbers.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['.', '-'])
            .map(|p| {
                p.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
            })
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        match a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0)) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// For each installed runtime, the newest available build of its flavor
/// that is newer and not installed yet.
pub fn find(paths: &AppPaths) -> Result<Vec<RuntimeUpdate>> {
    find_in(paths, &super::installed(paths)?, &manifest::available())
}

pub(crate) fn find_in(
    paths: &AppPaths,
    installed: &[Runtime],
    available: &[RuntimeInfo],
) -> Result<Vec<RuntimeUpdate>> {
    let mut updates = Vec::new();
    for rt in installed {
        let newest = available
            .iter()
            .filter(|a| a.flavor == rt.flavor && a.arch == rt.arch)
            .filter(|a| !installed.iter().any(|i| i.id == a.id))
            .filter(|a| compare_versions(&a.version, &rt.version) == Ordering::Greater)
            .max_by(|a, b| compare_versions(&a.version, &b.version));
        if let Some(newer) = newest {
            // Two installed builds of one flavor: only offer it for the newest.
            let newest_installed = installed
                .iter()
                .filter(|i| i.flavor == rt.flavor && i.arch == rt.arch)
                .all(|i| compare_versions(&i.version, &rt.version) != Ordering::Greater);
            if newest_installed {
                updates.push(RuntimeUpdate {
                    installed_id: rt.id.clone(),
                    installed_version: rt.version.clone(),
                    newer: newer.clone(),
                    bottles: bottle::using_runtime(paths, &rt.id)?,
                });
            }
        }
    }
    Ok(updates)
}

/// Switches every bottle on runtime `from` to `to`. Returns how many moved.
/// Wine updates each prefix on its next start.
pub fn move_bottles(paths: &AppPaths, from: &str, to: &str) -> Result<usize> {
    super::get(paths, to)?;
    let ids: Vec<String> = bottle::list(paths)?
        .into_iter()
        .filter(|b| b.runtime == from)
        .map(|b| b.id)
        .collect();
    for id in &ids {
        bottle::set_runtime(paths, id, to)?;
    }
    Ok(ids.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert_eq!(compare_versions("11.18", "11.9"), Ordering::Greater);
        assert_eq!(compare_versions("11.0", "11"), Ordering::Equal);
        assert_eq!(compare_versions("10.20", "11.0"), Ordering::Less);
    }
}

#[cfg(test)]
mod find_tests {
    use std::path::PathBuf;

    use super::*;
    use crate::runtime::Flavor;

    fn installed(id: &str, version: &str, flavor: Flavor) -> Runtime {
        Runtime {
            id: id.into(),
            name: id.into(),
            version: version.into(),
            flavor,
            path: PathBuf::new(),
            wine_bin: PathBuf::new(),
            installed_at: String::new(),
            size: 0,
            source_url: String::new(),
            arch: "x86_64".into(),
            needs_rosetta: false,
            graphics: vec![],
            kerberos: false,
            domain_identity: false,
        }
    }

    fn info(id: &str, version: &str, flavor: Flavor) -> RuntimeInfo {
        let mut i = manifest::all()[0].clone();
        i.id = id.into();
        i.version = version.into();
        i.flavor = flavor;
        i.arch = "x86_64".into();
        i
    }

    #[test]
    fn offers_the_newest_same_flavor_build_once() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let have = [
            installed("s11", "11.0", Flavor::Stable),
            installed("d18", "11.18", Flavor::Devel),
        ];
        let offer = [
            info("s11", "11.0", Flavor::Stable),
            info("s11.1", "11.1", Flavor::Stable),
            info("s11.2", "11.2", Flavor::Stable),
            info("d17", "11.17", Flavor::Devel),
        ];
        let u = find_in(&paths, &have, &offer).unwrap();
        assert_eq!(u.len(), 1);
        assert_eq!(
            (u[0].installed_id.as_str(), u[0].newer.id.as_str()),
            ("s11", "s11.2")
        );

        // Already installed the newest: nothing to offer.
        let have2 = [
            installed("s11", "11.0", Flavor::Stable),
            installed("s11.2", "11.2", Flavor::Stable),
        ];
        assert!(find_in(&paths, &have2, &offer).unwrap().is_empty());
    }
}
