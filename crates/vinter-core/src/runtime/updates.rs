//! Newer builds of installed runtimes, and moving bottles onto them.

use std::cmp::Ordering;

use serde::Serialize;

use super::Runtime;
use super::manifest::{self, RuntimeInfo};
use crate::{AppPaths, Result, bottle};

/// A newer build of the same flavor as an installed runtime: a newer Wine
/// version, or a newer build of the same runtime (e.g. Vinter Wine 11.19
/// build 3 for build 2).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RuntimeUpdate {
    /// The installed runtime it replaces.
    pub installed_id: String,
    pub installed_version: String,
    /// Build number of the installed runtime, if its id has one.
    pub installed_build: Option<u32>,
    pub newer: RuntimeInfo,
    /// Build number of the newer runtime, if its id has one.
    pub newer_build: Option<u32>,
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

/// The build number in a runtime id, which follows the version:
/// `vinter-wine-11.19-3-macos` is build 3 of 11.19. `None` for ids without one.
pub fn build_number(id: &str, version: &str) -> Option<u32> {
    let (_, rest) = id.split_once(&format!("-{version}-"))?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let after = rest[digits.len()..].chars().next();
    if digits.is_empty() || !matches!(after, None | Some('-')) {
        return None;
    }
    digits.parse().ok()
}

/// Orders runtimes by Wine version, then by build.
fn compare_releases(a: (&str, Option<u32>), b: (&str, Option<u32>)) -> Ordering {
    compare_versions(a.0, b.0).then(a.1.unwrap_or(0).cmp(&b.1.unwrap_or(0)))
}

/// For each installed runtime, the newest available build of its flavor
/// that is newer and not installed yet: a newer Wine version, or a newer
/// build of the same runtime.
pub fn find(paths: &AppPaths) -> Result<Vec<RuntimeUpdate>> {
    find_in(paths, &super::installed(paths)?, &manifest::available())
}

pub(crate) fn find_in<'a>(
    paths: &AppPaths,
    installed: &'a [Runtime],
    available: &'a [RuntimeInfo],
) -> Result<Vec<RuntimeUpdate>> {
    let release = |id: &str, version: &'a str| (version, build_number(id, version));
    let mut updates = Vec::new();
    for rt in installed {
        let have = release(&rt.id, &rt.version);
        let newest = available
            .iter()
            .filter(|a| a.flavor == rt.flavor && a.engine == rt.engine)
            .filter(|a| !installed.iter().any(|i| i.id == a.id))
            .filter(|a| match compare_versions(&a.version, &rt.version) {
                Ordering::Greater => true,
                // Same Wine version: only a newer build of this same runtime.
                Ordering::Equal => {
                    a.name == rt.name
                        && compare_releases(release(&a.id, &a.version), have) == Ordering::Greater
                }
                Ordering::Less => false,
            })
            .max_by(|a, b| {
                compare_releases(release(&a.id, &a.version), release(&b.id, &b.version))
            });
        if let Some(newer) = newest {
            // Two installed builds of one flavor: only offer it for the newest.
            let newest_installed = installed
                .iter()
                .filter(|i| i.flavor == rt.flavor && i.engine == rt.engine)
                .all(|i| compare_releases(release(&i.id, &i.version), have) != Ordering::Greater);
            if newest_installed {
                updates.push(RuntimeUpdate {
                    installed_id: rt.id.clone(),
                    installed_version: rt.version.clone(),
                    installed_build: have.1,
                    newer: newer.clone(),
                    newer_build: build_number(&newer.id, &newer.version),
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
    fn build_numbers_come_from_ids() {
        assert_eq!(build_number("vinter-wine-11.19-3-macos", "11.19"), Some(3));
        assert_eq!(build_number("vinter-wine-11.0-12-macos", "11.0"), Some(12));
        assert_eq!(build_number("wine-stable-11.0-macos", "11.0"), None);
        assert_eq!(build_number("wine-devel-11.19-linux", "11.19"), None);
        assert_eq!(build_number("x-11.0-4b-macos", "11.0"), None);
    }

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
        named(id, id, version, flavor)
    }

    fn named(id: &str, name: &str, version: &str, flavor: Flavor) -> Runtime {
        Runtime {
            id: id.into(),
            name: name.into(),
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
            engine: crate::engine::EngineKind::legacy(),
            fex_version: None,
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
        i.engine = crate::engine::EngineKind::legacy();
        i
    }

    #[test]
    fn never_offers_a_runtime_of_another_engine() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let have = [installed("s11", "11.0", Flavor::Stable)];
        let mut arm = info("arm-12", "12.0", Flavor::Stable);
        arm.engine = crate::engine::EngineKind::Arm64Fex;
        assert!(find_in(&paths, &have, &[arm]).unwrap().is_empty());
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

    #[test]
    fn offers_a_newer_build_of_the_same_runtime() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let with_name = |id: &str, name: &str, version: &str| {
            let mut i = info(id, version, Flavor::Stable);
            i.name = name.into();
            i
        };
        let offer = [
            with_name("wine-stable-11.0-macos", "Wine Stable 11.0", "11.0"),
            with_name("vinter-wine-11.0-4-macos", "Vinter Wine 11.0", "11.0"),
            with_name("vinter-wine-11.19-3-macos", "Vinter Wine 11.19", "11.19"),
        ];

        let have = [named(
            "vinter-wine-11.19-2-macos",
            "Vinter Wine 11.19",
            "11.19",
            Flavor::Stable,
        )];
        let u = find_in(&paths, &have, &offer).unwrap();
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].newer.id, "vinter-wine-11.19-3-macos");
        assert_eq!((u[0].installed_build, u[0].newer_build), (Some(2), Some(3)));

        // The same build, or another runtime at the same version, is not an update.
        let have = [named(
            "vinter-wine-11.19-3-macos",
            "Vinter Wine 11.19",
            "11.19",
            Flavor::Stable,
        )];
        assert!(find_in(&paths, &have, &offer).unwrap().is_empty());
        let have = [named(
            "wine-stable-11.0-macos",
            "Wine Stable 11.0",
            "11.0",
            Flavor::Stable,
        )];
        let u = find_in(&paths, &have, &offer).unwrap();
        assert_eq!(
            u[0].newer.id, "vinter-wine-11.19-3-macos",
            "a newer version still wins"
        );
    }
}
