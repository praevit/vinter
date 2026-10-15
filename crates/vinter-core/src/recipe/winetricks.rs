//! Winetricks: the LGPL shell script that installs common Windows
//! components. Downloaded once (pinned version, sha256 checked) into
//! `<data>/cache/tools/`. Verbs that download Microsoft software only run
//! after the user agreed (see `ApplyOptions::accept_downloads`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::bottle::{self, PREFIX_DIR};
use crate::download::{self, InstallProgress};
use crate::error::IoResultExt;
use crate::process::{self, logs};
use crate::{AppPaths, Error, Result, runtime};

pub const VERSION: &str = "20260125";
const URL: &str = "https://raw.githubusercontent.com/Winetricks/winetricks/20260125/src/winetricks";
const SHA256: &str = "431f82fc74000e6c864409f1d8fb495d696c03928808e3e8acffc45179312a7b";
const SIZE: u64 = 830_687;
const TIMEOUT: Duration = Duration::from_secs(30 * 60);

fn script_path(paths: &AppPaths) -> PathBuf {
    paths
        .data_dir()
        .join("cache")
        .join("tools")
        .join(format!("winetricks-{VERSION}"))
}

/// Downloads the pinned winetricks script if it is not there yet.
pub async fn ensure<P>(paths: &AppPaths, on_progress: &mut P) -> Result<PathBuf>
where
    P: FnMut(InstallProgress) + Send,
{
    let path = script_path(paths);
    if path.is_file() {
        return Ok(path);
    }
    let dir = path.parent().expect("tools dir");
    tokio::fs::create_dir_all(dir).await.at(dir)?;
    let tmp = dir.join(format!(".winetricks-{}.download", uuid::Uuid::new_v4()));
    download::download(URL, &tmp, SIZE, on_progress).await?;
    download::verify(&tmp, SHA256, on_progress).await?;
    tokio::fs::rename(&tmp, &path).await.at(&path)?;
    Ok(path)
}

/// Runs `winetricks -q <verbs>` against a bottle with its runtime.
pub fn run(paths: &AppPaths, bottle_id: &str, script: &Path, verbs: &[String]) -> Result<()> {
    let bottle = bottle::get(paths, bottle_id)?;
    let rt = runtime::get(paths, &bottle.runtime)?;
    rt.check_can_run()?;
    let dir = bottle::dir(paths, bottle_id)?;
    let prefix = dir.join(PREFIX_DIR);

    let mut cmd = rt.command("sh");
    cmd.arg(script).arg("-q").args(verbs);
    cmd.envs(process::wine_env(&prefix, &bottle, &[]))
        .env("WINE", &rt.wine_bin)
        .env("WINESERVER", rt.bin_dir().join("wineserver"))
        // No "a newer winetricks exists" prompt; we pin a version on purpose.
        .env("WINETRICKS_LATEST_VERSION_CHECK", "disabled")
        // Keep winetricks' download cache inside Vinter's data folder.
        .env("XDG_CACHE_HOME", paths.data_dir().join("cache"))
        .stdin(std::process::Stdio::null());

    let (log, _) = logs::create(&dir, "winetricks")?;
    process::run_and_wait(cmd, &log, &format!("winetricks {}", verbs.join(" ")), TIMEOUT).map_err(|e| {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        match missing_tool(&text) {
            Some(tool) => Error::ProcessFailed {
                what: "winetricks".into(),
                message: format!(
                    "it needs `{tool}`, which is not installed. On macOS: `brew install {tool}`. ({e})"
                ),
            },
            None => e,
        }
    })
}

/// Tools winetricks commonly needs that may be missing on a Mac.
fn missing_tool(log: &str) -> Option<&'static str> {
    ["cabextract", "7z", "unzip", "wget"]
        .into_iter()
        .find(|tool| {
            log.contains(&format!("{tool} not found"))
                || log.contains(&format!("Please install {tool}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_missing_tools() {
        assert_eq!(
            missing_tool("------\nwarning: cabextract not found. Please install it."),
            Some("cabextract")
        );
        assert_eq!(missing_tool("Executing w_do_call corefonts"), None);
    }
}
