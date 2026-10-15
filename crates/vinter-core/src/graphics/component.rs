//! Downloadable graphics components (e.g. DXVK), installed once under
//! `<data>/components/<id>/` and copied into bottles that use them.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::Backend;
use crate::download::{self, ArchiveFormat, InstallProgress};
use crate::error::{IoResultExt, validate_id};
use crate::{AppPaths, Error, Result};

const MANIFEST_JSON: &str = include_str!("../../components.json");
const RECORD_FILE: &str = "component.toml";

/// One downloadable component, from the embedded `components.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ComponentInfo {
    pub id: String,
    pub backend: Backend,
    pub name: String,
    pub version: String,
    pub os: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub format: ArchiveFormat,
    /// Top folder inside the archive, holding `x64/` and `x32/`.
    pub root: String,
    /// DLL names (without `.dll`) it provides in both `x64/` and `x32/`.
    pub dlls: Vec<String>,
    pub license: String,
    pub source_url: String,
    pub release_url: String,
}

/// An installed component.
#[derive(Debug, Clone)]
pub struct Component {
    pub info: ComponentInfo,
    pub path: PathBuf,
}

impl Component {
    /// 64-bit DLLs (copied to `system32`).
    pub fn x64_dir(&self) -> PathBuf {
        self.path.join(&self.info.root).join("x64")
    }

    /// 32-bit DLLs (copied to `syswow64`).
    pub fn x32_dir(&self) -> PathBuf {
        self.path.join(&self.info.root).join("x32")
    }
}

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    components: Vec<ComponentInfo>,
}

pub fn all() -> &'static [ComponentInfo] {
    static PARSED: OnceLock<Vec<ComponentInfo>> = OnceLock::new();
    PARSED.get_or_init(|| {
        let manifest: Manifest =
            serde_json::from_str(MANIFEST_JSON).expect("embedded components.json is invalid");
        assert_eq!(manifest.version, 1, "unsupported components.json version");
        manifest.components
    })
}

/// The component a backend needs on `os`, if it needs one.
pub fn for_backend(backend: Backend, os: &str) -> Option<ComponentInfo> {
    all()
        .iter()
        .find(|c| c.backend == backend && c.os == os)
        .cloned()
}

fn dir(paths: &AppPaths, id: &str) -> Result<PathBuf> {
    validate_id(id)?;
    Ok(paths.data_dir().join("components").join(id))
}

/// The installed copy of a component, if any.
pub fn installed(paths: &AppPaths, id: &str) -> Option<Component> {
    let path = dir(paths, id).ok()?;
    let text = std::fs::read_to_string(path.join(RECORD_FILE)).ok()?;
    let info: ComponentInfo = toml::from_str(&text).ok()?;
    Some(Component { info, path })
}

/// Downloads, verifies and installs a component, unless it is already there.
pub async fn ensure<P>(
    paths: &AppPaths,
    info: &ComponentInfo,
    mut on_progress: P,
) -> Result<Component>
where
    P: FnMut(InstallProgress) + Send,
{
    if let Some(c) = installed(paths, &info.id) {
        return Ok(c);
    }
    let downloads = paths.downloads_dir();
    tokio::fs::create_dir_all(&downloads).await.at(&downloads)?;
    let archive = downloads.join(format!("{}.{}", info.id, info.format.extension()));
    download::download(&info.url, &archive, info.size, &mut on_progress).await?;
    let result = install_from_archive(paths, info, &archive, &mut on_progress).await;
    let _ = tokio::fs::remove_file(&archive).await;
    result
}

/// Installs a component from an archive on disk (verifying its sha256 first).
pub async fn install_from_archive<P>(
    paths: &AppPaths,
    info: &ComponentInfo,
    archive: &Path,
    on_progress: &mut P,
) -> Result<Component>
where
    P: FnMut(InstallProgress) + Send,
{
    let dest = dir(paths, &info.id)?;
    download::verify(archive, &info.sha256, on_progress).await?;
    let parent = dest.parent().expect("component dir has a parent");
    tokio::fs::create_dir_all(parent).await.at(parent)?;
    let staging = parent.join(format!(".installing-{}-{}", info.id, uuid::Uuid::new_v4()));

    let result = async {
        download::extract_with_progress(archive, &staging, info.format, on_progress).await?;
        let component = Component {
            info: info.clone(),
            path: staging.clone(),
        };
        for dll in &info.dlls {
            let x64 = component.x64_dir().join(format!("{dll}.dll"));
            if !x64.is_file() {
                return Err(Error::BadArchive(format!(
                    "{dll}.dll is missing from {}",
                    info.name
                )));
            }
        }
        let record = toml::to_string_pretty(info).expect("component info serializes");
        let file = staging.join(RECORD_FILE);
        tokio::fs::write(&file, record).await.at(&file)?;
        if tokio::fs::metadata(&dest).await.is_ok() {
            tokio::fs::remove_dir_all(&dest).await.at(&dest)?;
        }
        tokio::fs::rename(&staging, &dest).await.at(&dest)
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
    }
    result?;
    installed(paths, &info.id)
        .ok_or_else(|| Error::BadArchive(format!("{} did not install", info.name)))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashSet;

    use super::*;

    /// Builds a component archive with fake `x64/` and `x32/` DLLs.
    pub(crate) fn make_archive(dir: &Path, root: &str, dlls: &[&str]) -> PathBuf {
        let path = dir.join(format!("{root}.tar.gz"));
        let file = std::fs::File::create(&path).unwrap();
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            file,
            flate2::Compression::fast(),
        ));
        for arch in ["x64", "x32"] {
            for dll in dlls {
                let data = format!("{arch} {dll}");
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append_data(
                    &mut header,
                    format!("{root}/{arch}/{dll}.dll"),
                    data.as_bytes(),
                )
                .unwrap();
            }
        }
        tar.into_inner().unwrap().finish().unwrap();
        path
    }

    pub(crate) fn info_for(archive: &Path, root: &str, dlls: &[&str]) -> ComponentInfo {
        use sha2::{Digest, Sha256};
        ComponentInfo {
            id: "fake-dxvk".into(),
            backend: Backend::Dxvk,
            name: "Fake DXVK".into(),
            version: "1".into(),
            os: std::env::consts::OS.into(),
            url: "https://example.invalid".into(),
            sha256: hex::encode(Sha256::digest(std::fs::read(archive).unwrap())),
            size: 1,
            format: ArchiveFormat::TarGz,
            root: root.into(),
            dlls: dlls.iter().map(|d| d.to_string()).collect(),
            license: "Zlib".into(),
            source_url: String::new(),
            release_url: String::new(),
        }
    }

    #[test]
    fn embedded_manifest_is_valid() {
        let mut ids = HashSet::new();
        for c in all() {
            assert!(ids.insert(&c.id), "duplicate {}", c.id);
            validate_id(&c.id).unwrap();
            assert!(c.url.starts_with("https://"));
            assert_eq!(c.sha256.len(), 64);
            assert!(!c.dlls.is_empty());
            assert!(matches!(c.os.as_str(), "macos" | "linux"));
            for dll in &c.dlls {
                validate_id(dll).unwrap();
            }
        }
        assert!(for_backend(Backend::Dxvk, "macos").is_some());
        assert!(for_backend(Backend::Dxvk, "linux").is_some());
        assert!(for_backend(Backend::Wined3d, "macos").is_none());
    }

    #[tokio::test]
    async fn installs_and_rejects_bad_archives() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path().join("data"));
        let archive = make_archive(tmp.path(), "dxvk-x", &["d3d11", "dxgi"]);
        let info = info_for(&archive, "dxvk-x", &["d3d11", "dxgi"]);

        let c = install_from_archive(&paths, &info, &archive, &mut |_| {})
            .await
            .unwrap();
        assert!(c.x64_dir().join("d3d11.dll").is_file());
        assert!(c.x32_dir().join("dxgi.dll").is_file());
        assert_eq!(installed(&paths, "fake-dxvk").unwrap().info, info);

        // Missing DLL -> rejected, nothing left behind.
        let other = make_archive(tmp.path(), "dxvk-y", &["d3d11"]);
        let mut bad = info_for(&other, "dxvk-y", &["d3d11", "d3d9"]);
        bad.id = "bad".into();
        let err = install_from_archive(&paths, &bad, &other, &mut |_| {})
            .await
            .unwrap_err();
        assert!(matches!(err, Error::BadArchive(_)), "{err}");
        assert!(installed(&paths, "bad").is_none());
        let leftovers: Vec<_> = std::fs::read_dir(paths.data_dir().join("components"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, ["fake-dxvk"]);

        // Wrong checksum -> rejected.
        let mut wrong = info.clone();
        wrong.id = "wrong".into();
        wrong.sha256 = "0".repeat(64);
        let again = make_archive(tmp.path(), "dxvk-x", &["d3d11", "dxgi"]);
        assert!(matches!(
            install_from_archive(&paths, &wrong, &again, &mut |_| {}).await,
            Err(Error::ChecksumMismatch { .. })
        ));
    }
}
