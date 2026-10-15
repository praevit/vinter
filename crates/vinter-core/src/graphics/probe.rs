//! "Test graphics": runs a tiny Direct3D 11 program in the bottle and reports
//! what the active backend delivered.
//!
//! The program is `tools/d3dprobe` (see its README), embedded as
//! `assets/d3dprobe.exe`. Wine's `+loaddll` trace shows whether `d3d11.dll`
//! came from Wine (builtin) or from a backend like DXVK (native).

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use super::Backend;
use crate::bottle::{self, PREFIX_DIR};
use crate::error::IoResultExt;
use crate::process::{self, logs};
use crate::{AppPaths, Error, Result, runtime};

const PROBE: &[u8] = include_bytes!("../../assets/d3dprobe.exe");
const TIMEOUT: Duration = Duration::from_secs(90);

/// Result of a graphics test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct GraphicsReport {
    /// The bottle's backend at the time of the test.
    pub backend: Backend,
    /// A Direct3D 11 device was created.
    pub ok: bool,
    /// Which implementation answered, e.g. "DXVK" or "Wine (wined3d)".
    pub renderer: Option<String>,
    /// e.g. "11_0".
    pub feature_level: Option<String>,
    /// GPU name as the renderer reports it.
    pub adapter: Option<String>,
    /// The loaded d3d11.dll came from the backend the bottle is set to.
    pub matches_backend: bool,
    /// Log of the test, for `log_read`.
    pub log_id: String,
}

/// Runs the probe in a bottle. Blocks for a few seconds.
pub fn test(paths: &AppPaths, bottle_id: &str) -> Result<GraphicsReport> {
    let bottle = bottle::get(paths, bottle_id)?;
    let dir = bottle::dir(paths, bottle_id)?;
    if dir.join(bottle::SETUP_MARKER).exists() {
        return Err(Error::BottleNotReady(bottle.name));
    }
    let rt = runtime::get(paths, &bottle.runtime)?;
    rt.check_can_run()?;
    let probe = write_probe(paths)?;

    let (log, _) = logs::create(&dir, "graphics-test")?;
    let mut cmd = process::command(&rt, &dir.join(PREFIX_DIR), &bottle, &[], "wine");
    cmd.arg(&probe)
        .env("WINEDEBUG", "+loaddll")
        // DXVK caches shaders in the working directory; a one-off probe has
        // nothing worth caching, so turn it off and run from the tools folder.
        .env("DXVK_STATE_CACHE", "0");
    if let Some(folder) = probe.parent() {
        cmd.current_dir(folder);
    }
    let code = process::run_status(cmd, &log, "Testing graphics", TIMEOUT)?;
    let text = std::fs::read_to_string(&log).at(&log)?;
    let log_id = log
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(parse(&text, bottle.graphics, code, log_id))
}

/// Writes the probe to `<data>/cache/tools/` once per version.
fn write_probe(paths: &AppPaths) -> Result<PathBuf> {
    let dir = paths.data_dir().join("cache").join("tools");
    std::fs::create_dir_all(&dir).at(&dir)?;
    // Named by content hash, so a new probe version never reuses an old file.
    use sha2::{Digest, Sha256};
    let hash = hex::encode(Sha256::digest(PROBE));
    let path = dir.join(format!("d3dprobe-{}.exe", &hash[..12]));
    if !path.is_file() {
        let tmp = dir.join(format!(".d3dprobe-{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, PROBE).at(&tmp)?;
        std::fs::rename(&tmp, &path).at(&path)?;
    }
    Ok(path)
}

/// Turns the probe's output plus Wine's load trace into a report.
fn parse(text: &str, backend: Backend, code: Option<i32>, log_id: String) -> GraphicsReport {
    let value = |key: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
            .map(String::from)
    };
    // e.g. `trace:loaddll:build_module Loaded L"C:\\windows\\system32\\d3d11.dll" at ...: native`
    let d3d11 = text
        .lines()
        .filter(|l| l.contains("loaddll") && l.to_ascii_lowercase().contains("\\d3d11.dll\""))
        .find_map(|l| {
            let kind = l.rsplit(':').next()?.trim();
            matches!(kind, "native" | "builtin").then(|| kind.to_string())
        });
    // DXVK logs the real GPU (Wine's dxgi on macOS reports a generic one).
    let dxvk_device = text
        .lines()
        .find_map(|l| l.split("Device name:").nth(1))
        .map(|s| s.trim_start_matches([' ', ':']).trim().to_string());

    let ok = code == Some(0) && value("d3d11_create").as_deref() == Some("0x00000000");
    let renderer = d3d11.as_deref().map(|kind| match (kind, backend) {
        ("builtin", _) => "Wine (wined3d)".to_string(),
        ("native", Backend::Dxvk) => "DXVK".to_string(),
        ("native", other) => format!("{} (native DLL)", other.name()),
        _ => kind.to_string(),
    });
    let matches_backend = match (d3d11.as_deref(), backend) {
        (Some("builtin"), Backend::Wined3d) => true,
        (Some("native"), Backend::Dxvk) => dxvk_device.is_some() || ok,
        _ => false,
    };
    GraphicsReport {
        backend,
        ok,
        renderer,
        feature_level: value("feature_level").map(|l| feature_level_name(&l)),
        adapter: dxvk_device.or_else(|| value("adapter")),
        matches_backend,
        log_id,
    }
}

fn feature_level_name(hex: &str) -> String {
    match hex {
        "0x9100" => "9_1",
        "0x9200" => "9_2",
        "0x9300" => "9_3",
        "0xa000" => "10_0",
        "0xa100" => "10_1",
        "0xb000" => "11_0",
        "0xb100" => "11_1",
        "0xc000" => "12_0",
        "0xc100" => "12_1",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed output of real runs on an M4 Pro Mac with Wine 11.0.
    const WINED3D: &str = r#"0024:trace:loaddll:build_module Loaded L"C:\\windows\\system32\\dxgi.dll" at 00006FFFFB140000: builtin
0024:trace:loaddll:build_module Loaded L"C:\\windows\\system32\\wined3d.dll" at 00006FFFF9F70000: builtin
0024:trace:loaddll:build_module Loaded L"C:\\windows\\system32\\d3d11.dll" at 00006FFFFB360000: builtin
d3d11_create=0x00000000
feature_level=0x9300
adapter=NVIDIA GeForce 6800
vendor=0x10de
"#;
    const DXVK: &str = r#"0024:trace:loaddll:build_module Loaded L"C:\\windows\\system32\\d3d11.dll" at 000000006A6C0000: native
info:  DXVK: v1.10.3-20230507-async (macOS)
info:  D3D11CoreCreateDevice: Using feature level D3D_FEATURE_LEVEL_11_0
info:    Device name:     : Apple M4 Pro
d3d11_create=0x00000000
feature_level=0xb000
adapter=NVIDIA GeForce 6800
vendor=0x10de
"#;

    #[test]
    fn wined3d_report() {
        let r = parse(WINED3D, Backend::Wined3d, Some(0), "x.log".into());
        assert!(r.ok && r.matches_backend);
        assert_eq!(r.renderer.as_deref(), Some("Wine (wined3d)"));
        assert_eq!(r.feature_level.as_deref(), Some("9_3"));
        assert_eq!(r.adapter.as_deref(), Some("NVIDIA GeForce 6800"));
    }

    #[test]
    fn dxvk_report_uses_the_real_gpu_name() {
        let r = parse(DXVK, Backend::Dxvk, Some(0), "x.log".into());
        assert!(r.ok && r.matches_backend);
        assert_eq!(r.renderer.as_deref(), Some("DXVK"));
        assert_eq!(r.feature_level.as_deref(), Some("11_0"));
        assert_eq!(r.adapter.as_deref(), Some("Apple M4 Pro"));
    }

    #[test]
    fn mismatch_and_failure_are_reported() {
        // Bottle says DXVK but Wine's own d3d11 answered.
        let r = parse(WINED3D, Backend::Dxvk, Some(0), "x.log".into());
        assert!(r.ok && !r.matches_backend);
        let r = parse(
            "d3d11_create=0x887a0004\n",
            Backend::Wined3d,
            Some(1),
            "x.log".into(),
        );
        assert!(!r.ok && r.feature_level.is_none() && r.renderer.is_none());
    }

    #[test]
    fn embedded_probe_is_a_windows_exe() {
        assert_eq!(&PROBE[..2], b"MZ");
        assert!(PROBE.len() < 64 * 1024);
    }
}
