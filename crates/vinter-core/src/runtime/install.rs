//! Download, verify and extract a runtime.

use std::path::{Path, PathBuf};

use super::{LICENSE_FILE, RECORD_FILE, Runtime, RuntimeInfo, RuntimeRecord};
use crate::download::{self, InstallProgress, InstallStage};
use crate::error::{IoResultExt, validate_id};
use crate::{AppPaths, Error, Result};

/// Downloads `info`'s archive, verifies it and installs it.
/// The archive is deleted afterwards, whether or not the install worked.
pub async fn install<P>(paths: &AppPaths, info: &RuntimeInfo, mut on_progress: P) -> Result<Runtime>
where
    P: FnMut(InstallProgress) + Send,
{
    validate_id(&info.id)?;
    ensure_not_installed(paths, &info.id)?;

    let downloads = paths.downloads_dir();
    tokio::fs::create_dir_all(&downloads).await.at(&downloads)?;
    let archive = downloads.join(format!("{}.{}", info.id, info.format.extension()));

    download::download(&info.url, &archive, info.size, &mut on_progress).await?;
    let result = install_from_archive(paths, info, &archive, &mut on_progress).await;
    let _ = tokio::fs::remove_file(&archive).await;
    result
}

/// Installs from an archive already on disk. Refuses to extract unless its
/// sha256 matches `info.sha256`; on mismatch the archive is deleted.
pub async fn install_from_archive<P>(
    paths: &AppPaths,
    info: &RuntimeInfo,
    archive: &Path,
    on_progress: &mut P,
) -> Result<Runtime>
where
    P: FnMut(InstallProgress) + Send,
{
    validate_id(&info.id)?;
    ensure_not_installed(paths, &info.id)?;

    download::verify(archive, &info.sha256, on_progress).await?;

    let runtimes_dir = paths.runtimes_dir();
    tokio::fs::create_dir_all(&runtimes_dir)
        .await
        .at(&runtimes_dir)?;
    // Extract next to the final location so the last step is an atomic rename.
    let staging = runtimes_dir.join(format!(".installing-{}-{}", info.id, uuid::Uuid::new_v4()));

    let result = extract_and_finish(paths, info, archive, &staging, on_progress).await;
    if result.is_err() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
    }
    result
}

async fn extract_and_finish<P>(
    paths: &AppPaths,
    info: &RuntimeInfo,
    archive: &Path,
    staging: &Path,
    on_progress: &mut P,
) -> Result<Runtime>
where
    P: FnMut(InstallProgress) + Send,
{
    download::extract_with_progress(archive, staging, info.format, on_progress).await?;

    on_progress(InstallProgress {
        stage: InstallStage::Finishing,
        done: 0,
        total: None,
    });
    if !staging.join(&info.wine_bin).is_file() {
        return Err(Error::BadArchive(format!(
            "`{}` is missing from the archive",
            info.wine_bin
        )));
    }

    let record = RuntimeRecord {
        id: info.id.clone(),
        name: info.name.clone(),
        version: info.version.clone(),
        flavor: info.flavor,
        url: info.url.clone(),
        sha256: info.sha256.to_ascii_lowercase(),
        installed_at: humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
        wine_bin: info.wine_bin.clone(),
        license: info.license.clone(),
        source_url: info.source_url.clone(),
        arch: info.arch.clone(),
        graphics: info.graphics.clone(),
        engine: Some(info.engine),
    };
    let toml = toml::to_string_pretty(&record).expect("runtime record serializes");
    write(staging.join(RECORD_FILE), toml).await?;
    write(staging.join(LICENSE_FILE), super::WINE_LICENSE).await?;

    let dest = paths.runtimes_dir().join(&info.id);
    ensure_not_installed(paths, &info.id)?;
    tokio::fs::rename(staging, &dest).await.at(&dest)?;
    super::get(paths, &info.id)
}

fn ensure_not_installed(paths: &AppPaths, id: &str) -> Result<()> {
    if paths.runtimes_dir().join(id).exists() {
        Err(Error::RuntimeAlreadyInstalled(id.to_string()))
    } else {
        Ok(())
    }
}

async fn write(path: PathBuf, contents: impl AsRef<[u8]>) -> Result<()> {
    tokio::fs::write(&path, contents).await.at(&path)
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::download::ArchiveFormat;
    use crate::runtime::{self, Flavor};

    const WINE_SCRIPT: &[u8] = b"#!/bin/sh\necho wine-11.0-test\n";

    /// Builds a `.tar.xz` holding `top/bin/wine` plus any extra entries.
    fn make_archive(dir: &Path, top: &str, with_wine: bool) -> PathBuf {
        let path = dir.join(format!("{top}.tar.xz"));
        let file = std::fs::File::create(&path).unwrap();
        let mut tar = tar::Builder::new(liblzma::write::XzEncoder::new(file, 1));
        let mut add = |name: &str, data: &[u8], mode: u32| {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(mode);
            header.set_cksum();
            tar.append_data(&mut header, name, data).unwrap();
        };
        add(&format!("{top}/share/readme.txt"), b"hello", 0o644);
        if with_wine {
            add(&format!("{top}/bin/wine"), WINE_SCRIPT, 0o755);
        }
        tar.into_inner().unwrap().finish().unwrap();
        path
    }

    fn sha256_of(path: &Path) -> String {
        hex::encode(Sha256::digest(std::fs::read(path).unwrap()))
    }

    fn info(id: &str, sha256: String) -> RuntimeInfo {
        RuntimeInfo {
            id: id.into(),
            name: "Wine Test".into(),
            version: "11.0".into(),
            flavor: Flavor::Stable,
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            url: "https://example.invalid/wine.tar.xz".into(),
            sha256,
            size: 1,
            format: ArchiveFormat::TarXz,
            wine_bin: "wine-test/bin/wine".into(),
            license: "LGPL-2.1-or-later".into(),
            source_url: "https://gitlab.winehq.org/wine/wine/-/tree/wine-11.0".into(),
            release_url: "https://example.invalid".into(),
            needs_rosetta: false,
            graphics: vec![crate::bottle::Graphics::Wined3d],
            engine: crate::engine::EngineKind::legacy(),
            min_os_version: None,
        }
    }

    fn visible_entries(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|rd| {
                rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn installs_lists_runs_and_removes() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path().join("data"));
        let archive = make_archive(tmp.path(), "wine-test", true);
        let info = info("wine-test-1", sha256_of(&archive));

        let mut stages = Vec::new();
        let rt = install_from_archive(&paths, &info, &archive, &mut |p: InstallProgress| {
            stages.push(p.stage)
        })
        .await
        .unwrap();
        assert!(stages.contains(&InstallStage::Verifying));
        assert!(stages.contains(&InstallStage::Extracting));
        assert_eq!(stages.last(), Some(&InstallStage::Finishing));

        assert_eq!(rt.id, "wine-test-1");
        assert!(rt.wine_bin.is_file());
        assert!(rt.size > 0);
        assert!(rt.path.join(RECORD_FILE).is_file());
        assert!(rt.path.join(LICENSE_FILE).is_file());
        assert_eq!(runtime::installed(&paths).unwrap(), vec![rt.clone()]);
        assert_eq!(visible_entries(&paths.runtimes_dir()), ["wine-test-1"]);

        #[cfg(unix)]
        {
            let out = rt.command("wine").arg("--version").output().unwrap();
            assert_eq!(
                String::from_utf8_lossy(&out.stdout).trim(),
                "wine-11.0-test"
            );
        }

        let again = install_from_archive(&paths, &info, &archive, &mut |_| {}).await;
        assert!(matches!(again, Err(Error::RuntimeAlreadyInstalled(_))));

        runtime::remove(&paths, "wine-test-1").unwrap();
        assert!(runtime::installed(&paths).unwrap().is_empty());
        assert!(visible_entries(&paths.runtimes_dir()).is_empty());
        assert!(matches!(
            runtime::remove(&paths, "wine-test-1"),
            Err(Error::RuntimeNotInstalled(_))
        ));
    }

    #[tokio::test]
    async fn rejects_sha256_mismatch_without_extracting() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path().join("data"));
        let archive = make_archive(tmp.path(), "wine-test", true);
        let real = sha256_of(&archive);
        let info = info("wine-test-1", "0".repeat(64));

        let err = install_from_archive(&paths, &info, &archive, &mut |_| {})
            .await
            .unwrap_err();
        match err {
            Error::ChecksumMismatch { actual, .. } => assert_eq!(actual, real),
            other => panic!("unexpected error: {other}"),
        }
        assert!(!archive.exists(), "a bad download must be deleted");
        assert!(visible_entries(&paths.runtimes_dir()).is_empty());
    }

    #[tokio::test]
    async fn rejects_archive_without_wine_and_cleans_up() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path().join("data"));
        let archive = make_archive(tmp.path(), "wine-test", false);
        let info = info("wine-test-1", sha256_of(&archive));

        let err = install_from_archive(&paths, &info, &archive, &mut |_| {})
            .await
            .unwrap_err();
        assert!(matches!(err, Error::BadArchive(_)), "{err}");
        assert!(visible_entries(&paths.runtimes_dir()).is_empty());
    }

    #[tokio::test]
    async fn never_writes_outside_the_runtime_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path().join("data"));
        let archive = tmp.path().join("evil.tar.xz");
        {
            let file = std::fs::File::create(&archive).unwrap();
            let mut tar = tar::Builder::new(liblzma::write::XzEncoder::new(file, 1));
            let mut header = tar::Header::new_gnu();
            let name = b"../../../escaped.txt";
            header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name);
            header.set_size(4);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append(&header, &b"evil"[..]).unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        let info = info("wine-test-1", sha256_of(&archive));

        let result = install_from_archive(&paths, &info, &archive, &mut |_| {}).await;
        assert!(result.is_err());
        assert!(!tmp.path().join("escaped.txt").exists());
        assert!(!paths.data_dir().join("escaped.txt").exists());
        assert!(visible_entries(&paths.runtimes_dir()).is_empty());
    }

    #[tokio::test]
    async fn rejects_unsafe_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let info = info("../escape", "0".repeat(64));
        let err = install_from_archive(&paths, &info, Path::new("/nonexistent"), &mut |_| {})
            .await
            .unwrap_err();
        assert!(matches!(err, Error::InvalidId(_)));
        assert!(matches!(
            runtime::remove(&paths, "../x"),
            Err(Error::InvalidId(_))
        ));
        assert!(matches!(
            runtime::get(&paths, "a/b"),
            Err(Error::InvalidId(_))
        ));
    }

    #[test]
    fn progress_fraction() {
        let p = |done, total| InstallProgress {
            stage: InstallStage::Downloading,
            done,
            total,
        };
        assert_eq!(p(50, Some(100)).fraction(), Some(0.5));
        assert_eq!(p(150, Some(100)).fraction(), Some(1.0));
        assert_eq!(p(5, None).fraction(), None);
        assert_eq!(p(5, Some(0)).fraction(), None);
    }
}
