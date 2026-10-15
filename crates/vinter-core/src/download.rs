//! Downloading, verifying and extracting archives (runtimes, graphics components).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::IoResultExt;
use crate::{Error, Result};

/// How often progress is reported while a stage is running.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ArchiveFormat {
    TarXz,
    TarGz,
}

impl ArchiveFormat {
    /// File extension of archives in this format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::TarXz => "tar.xz",
            Self::TarGz => "tar.gz",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum InstallStage {
    Downloading,
    Verifying,
    Extracting,
    Finishing,
}

impl InstallStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Downloading => "Downloading",
            Self::Verifying => "Verifying checksum",
            Self::Extracting => "Extracting",
            Self::Finishing => "Finishing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallProgress {
    pub stage: InstallStage,
    /// Bytes processed in this stage.
    pub done: u64,
    pub total: Option<u64>,
}

impl InstallProgress {
    /// Progress of this stage from 0.0 to 1.0, if the total is known.
    pub fn fraction(&self) -> Option<f64> {
        self.total
            .filter(|t| *t > 0)
            .map(|t| (self.done as f64 / t as f64).min(1.0))
    }
}

/// Streams `url` to `dest`, writing to `dest.part` first.
pub(crate) async fn download<P>(
    url: &str,
    dest: &Path,
    expected_size: u64,
    on_progress: &mut P,
) -> Result<()>
where
    P: FnMut(InstallProgress) + Send,
{
    let mut part = dest.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);

    let result = download_to(url, &part, expected_size, on_progress).await;
    match result {
        Ok(()) => tokio::fs::rename(&part, dest).await.at(dest),
        Err(e) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(e)
        }
    }
}

async fn download_to<P>(
    url: &str,
    part: &Path,
    expected_size: u64,
    on_progress: &mut P,
) -> Result<()>
where
    P: FnMut(InstallProgress) + Send,
{
    let failed = |e: reqwest::Error| Error::Download(e.to_string());
    let client = reqwest::Client::builder()
        .user_agent(concat!("Vinter/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(failed)?;
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(failed)?;
    let total = response.content_length().or(Some(expected_size));

    let mut file = tokio::fs::File::create(part).await.at(part)?;
    let mut done = 0u64;
    let mut last_report = Instant::now();
    on_progress(InstallProgress {
        stage: InstallStage::Downloading,
        done,
        total,
    });
    while let Some(chunk) = response.chunk().await.map_err(failed)? {
        file.write_all(&chunk).await.at(part)?;
        done += chunk.len() as u64;
        if last_report.elapsed() >= PROGRESS_INTERVAL {
            last_report = Instant::now();
            on_progress(InstallProgress {
                stage: InstallStage::Downloading,
                done,
                total,
            });
        }
    }
    file.flush().await.at(part)?;
    on_progress(InstallProgress {
        stage: InstallStage::Downloading,
        done,
        total: Some(done),
    });
    Ok(())
}

/// Lowercase hex sha256 of a file.
pub(crate) async fn sha256_file<P>(path: &Path, on_progress: &mut P) -> Result<String>
where
    P: FnMut(InstallProgress) + Send,
{
    let mut file = tokio::fs::File::open(path).await.at(path)?;
    let total = file.metadata().await.at(path)?.len();
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    let mut last_report = Instant::now();
    loop {
        let n = file.read(&mut buf).await.at(path)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        done += n as u64;
        if last_report.elapsed() >= PROGRESS_INTERVAL {
            last_report = Instant::now();
            on_progress(InstallProgress {
                stage: InstallStage::Verifying,
                done,
                total: Some(total),
            });
        }
    }
    on_progress(InstallProgress {
        stage: InstallStage::Verifying,
        done,
        total: Some(total),
    });
    Ok(hex::encode(hasher.finalize()))
}

/// Unpacks a tar archive into `dest`. `tar::Archive::unpack` refuses entries
/// that would land outside `dest` (`..`, absolute paths, symlinked parents).
fn extract(archive: &Path, dest: &Path, format: ArchiveFormat, read: &AtomicU64) -> Result<()> {
    let file = std::fs::File::open(archive).at(archive)?;
    let counting = CountingReader {
        inner: std::io::BufReader::new(file),
        read,
    };
    let decoder: Box<dyn Read> = match format {
        ArchiveFormat::TarXz => Box::new(liblzma::read::XzDecoder::new_multi_decoder(counting)),
        ArchiveFormat::TarGz => Box::new(flate2::read::MultiGzDecoder::new(counting)),
    };
    std::fs::create_dir_all(dest).at(dest)?;
    let mut tar = tar::Archive::new(decoder);
    tar.set_preserve_permissions(true);
    tar.set_unpack_xattrs(false);
    tar.unpack(dest)
        .map_err(|e| Error::BadArchive(format!("could not extract: {e}")))
}

/// Counts compressed bytes read, for extraction progress.
struct CountingReader<'a, R> {
    inner: R,
    read: &'a AtomicU64,
}

impl<R: Read> Read for CountingReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// Fails with `ChecksumMismatch` (and deletes the archive) unless its
/// sha256 is `expected`. Reports `Verifying` progress.
pub(crate) async fn verify<P>(archive: &Path, expected: &str, on_progress: &mut P) -> Result<()>
where
    P: FnMut(InstallProgress) + Send,
{
    let actual = sha256_file(archive, on_progress).await?;
    if !actual.eq_ignore_ascii_case(expected) {
        let _ = tokio::fs::remove_file(archive).await;
        return Err(Error::ChecksumMismatch {
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

/// Extracts `archive` into `dest` on a worker thread, reporting `Extracting`
/// progress (compressed bytes read) while it runs.
pub(crate) async fn extract_with_progress<P>(
    archive: &Path,
    dest: &Path,
    format: ArchiveFormat,
    on_progress: &mut P,
) -> Result<()>
where
    P: FnMut(InstallProgress) + Send,
{
    let total = tokio::fs::metadata(archive).await.at(archive)?.len();
    let read = Arc::new(AtomicU64::new(0));
    let mut task = tokio::task::spawn_blocking({
        let (archive, dest, read) = (archive.to_path_buf(), dest.to_path_buf(), read.clone());
        move || extract(&archive, &dest, format, &read)
    });
    let report = |on_progress: &mut P| {
        on_progress(InstallProgress {
            stage: InstallStage::Extracting,
            done: read.load(Ordering::Relaxed),
            total: Some(total),
        })
    };
    report(on_progress);
    let joined = loop {
        tokio::select! {
            joined = &mut task => break joined,
            _ = tokio::time::sleep(PROGRESS_INTERVAL) => report(on_progress),
        }
    };
    joined.map_err(|e| Error::BadArchive(format!("extraction stopped: {e}")))??;
    report(on_progress);
    Ok(())
}
