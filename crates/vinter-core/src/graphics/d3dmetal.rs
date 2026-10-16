//! Apple's D3DMetal, imported from the user's own copy of the Game Porting
//! Toolkit (Vinter cannot include it).
//!
//! The toolkit's evaluation environment has a `redist/lib` folder:
//! `external/` (D3DMetal.framework, libd3dshared.dylib) and
//! `wine/x86_64-windows` + `wine/x86_64-unix` (Wine-side DLLs). Vinter copies
//! that folder to `<data>/components/d3dmetal/lib`, and a runtime built for
//! it loads D3DMetal from there when a process sets `GRAPHICS_BACKEND=d3dmetal`
//! and `D3DMETAL_RUNTIME_DIR` (see `launch_env`).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::IoResultExt;
use crate::{AppPaths, Error, Result};

const RECORD_FILE: &str = "d3dmetal.toml";
const LICENSE_FILE: &str = "License.rtf";

/// Files that must be in `redist/lib` for D3DMetal to work.
const REQUIRED: &[&str] = &[
    "external/D3DMetal.framework",
    "external/libd3dshared.dylib",
    "wine/x86_64-windows/d3d11.dll",
    "wine/x86_64-windows/dxgi.dll",
    "wine/x86_64-unix/d3d11.so",
];

/// An imported copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct D3dMetal {
    /// e.g. "4.0 beta 3", from the evaluation environment's name.
    pub version: String,
    /// RFC 3339 timestamp.
    pub imported_at: String,
}

/// What a toolkit download holds, shown before importing it.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Inspection {
    pub version: String,
    /// Apple's license for it, as plain text.
    pub license: String,
}

fn dir(paths: &AppPaths) -> PathBuf {
    paths.data_dir().join("components").join("d3dmetal")
}

/// The folder to point `D3DMETAL_RUNTIME_DIR` at.
pub fn lib_dir(paths: &AppPaths) -> PathBuf {
    dir(paths).join("lib")
}

/// The imported copy, if any.
pub fn installed(paths: &AppPaths) -> Option<D3dMetal> {
    let text = std::fs::read_to_string(dir(paths).join(RECORD_FILE)).ok()?;
    let record: D3dMetal = toml::from_str(&text).ok()?;
    REQUIRED
        .iter()
        .all(|f| lib_dir(paths).join(f).exists())
        .then_some(record)
}

/// Reads the version and license of a toolkit download (a `.dmg` or an
/// opened folder) without importing anything.
pub fn inspect(source: &Path) -> Result<Inspection> {
    with_redist(source, |root, version| {
        let license = license_text(&root.join(LICENSE_FILE))?;
        Ok(Inspection { version, license })
    })
}

/// Copies D3DMetal from a toolkit download into Vinter's data folder,
/// replacing an earlier import. The caller has shown Apple's license.
pub fn import(paths: &AppPaths, source: &Path) -> Result<D3dMetal> {
    let dest = dir(paths);
    let parent = dest.parent().expect("component dir has a parent");
    std::fs::create_dir_all(parent).at(parent)?;
    let staging = parent.join(format!(".importing-d3dmetal-{}", uuid::Uuid::new_v4()));
    let result = with_redist(source, |root, version| {
        std::fs::create_dir_all(&staging).at(&staging)?;
        // ditto keeps the framework's symlinks and extended attributes.
        let status = Command::new("/usr/bin/ditto")
            .arg(root.join("redist/lib"))
            .arg(staging.join("lib"))
            .status()
            .at(&staging)?;
        if !status.success() {
            return Err(Error::BadArchive("could not copy D3DMetal".into()));
        }
        let license = root.join(LICENSE_FILE);
        if license.is_file() {
            std::fs::copy(&license, staging.join(LICENSE_FILE)).at(&license)?;
        }
        let record = D3dMetal {
            version,
            imported_at: humantime::format_rfc3339_seconds(std::time::SystemTime::now())
                .to_string(),
        };
        let file = staging.join(RECORD_FILE);
        std::fs::write(&file, toml::to_string(&record).expect("record serializes")).at(&file)?;
        Ok(record)
    });
    match result {
        Ok(record) => {
            if dest.exists() {
                std::fs::remove_dir_all(&dest).at(&dest)?;
            }
            std::fs::rename(&staging, &dest).at(&dest)?;
            Ok(record)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}

/// Removes the imported copy.
pub fn remove(paths: &AppPaths) -> Result<()> {
    let dest = dir(paths);
    match std::fs::remove_dir_all(&dest) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).at(&dest),
        _ => Ok(()),
    }
}

/// Finds the evaluation environment in `source` (an opened folder, its
/// `.dmg`, or the toolkit's `.dmg` with the evaluation environment inside)
/// and calls `f` with its root folder and version. Disk images are mounted
/// read-only and unmounted afterwards.
fn with_redist<T>(source: &Path, f: impl FnOnce(&Path, String) -> Result<T>) -> Result<T> {
    let not_toolkit = || {
        Error::BadArchive(format!(
            "{} is not Apple's Game Porting Toolkit (no D3DMetal inside)",
            source.display()
        ))
    };
    if source.is_dir() {
        let root = find_root(source).ok_or_else(not_toolkit)?;
        let version = version_from_name(&root).ok_or_else(not_toolkit)?;
        return f(&root, version);
    }
    let outer = Mount::attach(source)?;
    if let Some(root) = find_root(&outer.0) {
        let version = version_from_name(&root).ok_or_else(not_toolkit)?;
        return f(&root, version);
    }
    let nested = std::fs::read_dir(&outer.0)
        .at(&outer.0)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            name.starts_with("Evaluation environment") && name.ends_with(".dmg")
        })
        .ok_or_else(not_toolkit)?;
    let inner = Mount::attach(&nested)?;
    let root = find_root(&inner.0).ok_or_else(not_toolkit)?;
    let version = version_from_name(&root).ok_or_else(not_toolkit)?;
    f(&root, version)
}

/// `folder` itself if it holds `redist/lib` with everything D3DMetal needs.
fn find_root(folder: &Path) -> Option<PathBuf> {
    let lib = folder.join("redist/lib");
    REQUIRED
        .iter()
        .all(|f| lib.join(f).exists())
        .then(|| folder.to_path_buf())
}

/// "Evaluation environment for Windows games 4.0 beta 3" → "4.0 beta 3".
fn version_from_name(root: &Path) -> Option<String> {
    let name = root.file_name()?.to_string_lossy().into_owned();
    let version = name
        .rsplit_once("games ")
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()));
    Some(version.unwrap_or_else(|| "unknown".into()))
}

/// The license as text (macOS's textutil reads the RTF).
fn license_text(rtf: &Path) -> Result<String> {
    if !rtf.is_file() {
        return Ok(String::new());
    }
    let out = Command::new("/usr/bin/textutil")
        .args(["-convert", "txt", "-stdout"])
        .arg(rtf)
        .output()
        .at(rtf)?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A disk image mounted read-only for as long as this lives.
struct Mount(PathBuf);

impl Mount {
    fn attach(image: &Path) -> Result<Self> {
        let out = Command::new("/usr/bin/hdiutil")
            .args(["attach", "-readonly", "-nobrowse", "-noautoopen", "-plist"])
            .arg(image)
            .output()
            .at(image)?;
        let plist = String::from_utf8_lossy(&out.stdout);
        // The mount point is the <string> after <key>mount-point</key>.
        let point = plist
            .split("<key>mount-point</key>")
            .nth(1)
            .and_then(|rest| rest.split("<string>").nth(1))
            .and_then(|rest| rest.split("</string>").next())
            .map(PathBuf::from)
            .ok_or_else(|| {
                Error::BadArchive(format!("could not open the disk image {}", image.display()))
            })?;
        Ok(Self(point))
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        let _ = Command::new("/usr/bin/hdiutil")
            .args(["detach", "-quiet"])
            .arg(&self.0)
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_toolkit(root: &Path) {
        for f in REQUIRED {
            let p = root.join("redist/lib").join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            if f.ends_with(".framework") {
                std::fs::create_dir_all(&p).unwrap();
            } else {
                std::fs::write(&p, b"x").unwrap();
            }
        }
    }

    #[test]
    fn versions_from_volume_names() {
        let v = |n: &str| version_from_name(Path::new(n)).unwrap();
        assert_eq!(
            v("/Volumes/Evaluation environment for Windows games 4.0 beta 3"),
            "4.0 beta 3"
        );
        assert_eq!(
            v("/Volumes/Evaluation environment for Windows games 3.0"),
            "3.0"
        );
        assert_eq!(v("/tmp/somewhere"), "unknown");
    }

    #[test]
    fn imports_from_a_folder_and_rejects_others() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path().join("data"));
        let kit = tmp
            .path()
            .join("Evaluation environment for Windows games 9.1");
        fake_toolkit(&kit);
        std::fs::write(kit.join(LICENSE_FILE), "{\\rtf1 hi}").unwrap();

        assert!(installed(&paths).is_none());
        let d = import(&paths, &kit).unwrap();
        assert_eq!(d.version, "9.1");
        assert_eq!(installed(&paths).unwrap(), d);
        assert!(
            lib_dir(&paths)
                .join("wine/x86_64-windows/d3d11.dll")
                .is_file()
        );

        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(import(&paths, &empty).is_err());
        assert!(
            installed(&paths).is_some(),
            "a failed import keeps the old one"
        );

        remove(&paths).unwrap();
        assert!(installed(&paths).is_none());
    }
}
