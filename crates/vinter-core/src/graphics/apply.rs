//! Switching a bottle's graphics backend.

use std::path::Path;

use super::{Backend, component};
use crate::bottle::{self, Bottle, PREFIX_DIR};
use crate::error::IoResultExt;
use crate::{AppPaths, Error, Result, runtime};

/// Every DLL any backend may replace. Switching restores all of them from
/// the runtime first, so leftovers from the previous backend never linger.
const MANAGED_DLLS: &[&str] = &["d3d8", "d3d9", "d3d10core", "d3d11", "dxgi"];

/// Sets a bottle's backend: restores Wine's own Direct3D DLLs, copies in the
/// backend's DLLs (its component must be installed, see
/// [`component::ensure`]) and saves the choice. Close the bottle's programs first.
pub fn apply(paths: &AppPaths, bottle_id: &str, backend: Backend) -> Result<Bottle> {
    let mut bottle = bottle::get(paths, bottle_id)?;
    let rt = runtime::get(paths, &bottle.runtime)?;
    let info = super::backends(paths, &rt)
        .into_iter()
        .find(|b| b.backend == backend)
        .expect("every backend is listed");
    if let Some(reason) = info.reason {
        return Err(Error::BackendUnavailable(reason));
    }
    let component = match &info.component {
        Some(c) => Some(
            component::installed(paths, &c.id)
                .ok_or_else(|| Error::ComponentMissing(c.name.clone()))?,
        ),
        None => None,
    };

    let windows = bottle::dir(paths, bottle_id)?
        .join(PREFIX_DIR)
        .join("drive_c/windows");
    let system32 = windows.join("system32");
    let syswow64 = windows.join("syswow64");
    let wine_lib = rt.bin_dir().parent().unwrap_or(&rt.path).join("lib/wine");

    for dll in MANAGED_DLLS {
        copy_if_present(&wine_lib.join("x86_64-windows"), &system32, dll)?;
        if syswow64.is_dir() {
            copy_if_present(&wine_lib.join("i386-windows"), &syswow64, dll)?;
        }
    }
    if let Some(c) = &component {
        for dll in &c.info.dlls {
            copy_if_present(&c.x64_dir(), &system32, dll)?;
            if syswow64.is_dir() {
                copy_if_present(&c.x32_dir(), &syswow64, dll)?;
            }
        }
    }

    bottle.graphics = backend;
    bottle.save(&bottle::dir(paths, bottle_id)?)?;
    Ok(bottle)
}

fn copy_if_present(from_dir: &Path, to_dir: &Path, dll: &str) -> Result<()> {
    let from = from_dir.join(format!("{dll}.dll"));
    if from.is_file() && to_dir.is_dir() {
        let to = to_dir.join(format!("{dll}.dll"));
        std::fs::copy(&from, &to).at(&to)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bottle::{Graphics, WindowsVersion};
    use crate::graphics::component::tests::{info_for, make_archive};
    use crate::runtime::{Flavor, RuntimeRecord};

    fn read(p: impl AsRef<Path>) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    /// A fake runtime with builtin DLLs, and a bottle with system32/syswow64.
    fn setup(tmp: &Path) -> (AppPaths, PathBuf) {
        let paths = AppPaths::with_data_dir(tmp.join("data"));
        let rt = paths.runtimes_dir().join("rt");
        for (arch, dir) in [("x64", "x86_64-windows"), ("x32", "i386-windows")] {
            let lib = rt.join("lib/wine").join(dir);
            std::fs::create_dir_all(&lib).unwrap();
            for dll in MANAGED_DLLS {
                std::fs::write(
                    lib.join(format!("{dll}.dll")),
                    format!("builtin {arch} {dll}"),
                )
                .unwrap();
            }
        }
        std::fs::create_dir_all(rt.join("bin")).unwrap();
        std::fs::write(rt.join("bin/wine"), "").unwrap();
        let record = RuntimeRecord {
            id: "rt".into(),
            name: "Wine".into(),
            version: "11".into(),
            flavor: Flavor::Stable,
            url: String::new(),
            sha256: "0".repeat(64),
            installed_at: "2026-10-15T00:00:00Z".into(),
            wine_bin: "bin/wine".into(),
            license: String::new(),
            source_url: String::new(),
            arch: std::env::consts::ARCH.into(),
            graphics: vec![Graphics::Wined3d, Graphics::Dxvk],
        };
        std::fs::write(rt.join("runtime.toml"), toml::to_string(&record).unwrap()).unwrap();

        let dir = bottle::dir(&paths, "b").unwrap();
        let windows = dir.join("prefix/drive_c/windows");
        std::fs::create_dir_all(windows.join("system32")).unwrap();
        std::fs::create_dir_all(windows.join("syswow64")).unwrap();
        Bottle {
            id: "b".into(),
            name: "B".into(),
            runtime: "rt".into(),
            created_at: "2026-10-15T00:00:00Z".into(),
            windows_version: WindowsVersion::Win10,
            graphics: Graphics::Wined3d,
            recipe: None,
            env: Default::default(),
            dll_overrides: Default::default(),
            options: Default::default(),
            programs: vec![],
        }
        .save(&dir)
        .unwrap();
        (paths, windows)
    }

    use std::path::PathBuf;

    #[tokio::test]
    async fn switching_copies_and_restores_dlls() {
        let tmp = tempfile::tempdir().unwrap();
        let (paths, windows) = setup(tmp.path());

        // DXVK without its component installed: refused.
        assert!(matches!(
            apply(&paths, "b", Backend::Dxvk),
            Err(Error::ComponentMissing(_)) | Err(Error::BackendUnavailable(_))
        ));

        // Install a fake component under the id the manifest expects for this OS.
        if let Some(real) = component::for_backend(Backend::Dxvk, std::env::consts::OS) {
            let dlls: Vec<&str> = real.dlls.iter().map(String::as_str).collect();
            let archive = make_archive(tmp.path(), &real.root, &dlls);
            let mut info = info_for(&archive, &real.root, &dlls);
            info.id = real.id.clone();
            component::install_from_archive(&paths, &info, &archive, &mut |_| {})
                .await
                .unwrap();

            let b = apply(&paths, "b", Backend::Dxvk).unwrap();
            assert_eq!(b.graphics, Backend::Dxvk);
            assert_eq!(read(windows.join("system32/d3d11.dll")), "x64 d3d11");
            assert_eq!(read(windows.join("syswow64/d3d11.dll")), "x32 d3d11");

            let b = apply(&paths, "b", Backend::Wined3d).unwrap();
            assert_eq!(b.graphics, Backend::Wined3d);
            assert_eq!(
                read(windows.join("system32/d3d11.dll")),
                "builtin x64 d3d11"
            );
            assert_eq!(
                read(windows.join("syswow64/d3d11.dll")),
                "builtin x32 d3d11"
            );
            assert_eq!(bottle::get(&paths, "b").unwrap().graphics, Backend::Wined3d);
        }

        // Unavailable backends are refused with a reason.
        let err = apply(&paths, "b", Backend::D3dmetal).unwrap_err();
        assert!(matches!(err, Error::BackendUnavailable(_)), "{err}");
    }
}
