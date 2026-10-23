//! Diagnostics: the header every launch writes to its log, and the
//! "Export diagnostics" zip (logs, system checks, bottle settings; never the
//! user's documents).

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::bottle::{self, Bottle};
use crate::error::IoResultExt;
use crate::process::logs;
use crate::programs::arch::{PeArch, RunPath};
use crate::runtime::Runtime;
use crate::{AppPaths, Error, Result, doctor, report, runtime, system};

/// Logs put in an export, newest first.
const EXPORT_LOGS: usize = 20;

/// Lines describing a launch, written at the top of its log: what runs, on
/// which engine, runtime, Wine and FEX version and graphics backend, and the
/// environment Vinter set (`env`, e.g. from `Command::get_envs`). Secrets and
/// the user's home folder are redacted.
pub fn launch_header(
    what: &str,
    found: Option<(PeArch, RunPath)>,
    rt: &Runtime,
    bottle: &Bottle,
    env: &[(String, String)],
) -> String {
    let mut lines = vec![
        format!("launching {what}"),
        format!("engine {}: {}", rt.engine.id(), rt.engine.label()),
        format!("runtime {} ({})", rt.id, rt.name),
        format!(
            "wine {}{}",
            rt.version,
            match &rt.fex_version {
                Some(fex) => format!(", fex {fex}"),
                None => String::new(),
            }
        ),
        format!("graphics {}", bottle.graphics.id()),
    ];
    match found {
        Some((pe, path)) => lines.push(format!(
            "architecture {} ({}), runs as {}",
            pe.id(),
            pe.label(),
            path.describe()
        )),
        None => lines.push("architecture unknown".into()),
    }
    let mut env: Vec<_> = env.iter().filter(|(k, _)| k != "PATH").collect();
    env.sort();
    for (key, value) in env {
        lines.push(format!("env {key}={}", redact_value(key, value)));
    }
    let mut out = String::new();
    for line in lines {
        out.push_str("vinter: ");
        out.push_str(&report::redact_home(&line));
        out.push('\n');
    }
    out
}

/// Appends [`launch_header`] to `log`.
pub fn write_launch_header(
    log: &Path,
    what: &str,
    found: Option<(PeArch, RunPath)>,
    rt: &Runtime,
    bottle: &Bottle,
    cmd: &std::process::Command,
) -> Result<()> {
    let env: Vec<(String, String)> = cmd
        .get_envs()
        .filter_map(|(k, v)| {
            Some((
                k.to_string_lossy().into_owned(),
                v?.to_string_lossy().into_owned(),
            ))
        })
        .collect();
    let header = launch_header(what, found, rt, bottle, &env);
    std::fs::File::options()
        .create(true)
        .append(true)
        .open(log)
        .and_then(|mut f| f.write_all(header.as_bytes()))
        .at(log)
}

/// Values of variables whose names suggest a secret are hidden.
fn redact_value(key: &str, value: &str) -> String {
    let upper = key.to_ascii_uppercase();
    let secret = [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "API_KEY",
        "APIKEY",
        "CREDENTIAL",
    ]
    .iter()
    .any(|s| upper.contains(s))
        || upper.ends_with("_KEY");
    if secret {
        "<redacted>".into()
    } else {
        value.to_string()
    }
}

/// Writes a diagnostics zip for a bottle into `dest` (a folder, or the zip's
/// path) and returns its path. It holds the debug report, system checks,
/// `bottle.toml`, the runtime's record, the newest logs and the log doctor's
/// findings, with the home folder redacted. Nothing from the bottle's C: drive.
pub fn export(paths: &AppPaths, bottle_id: &str, dest: &Path) -> Result<PathBuf> {
    let bottle = bottle::get(paths, bottle_id)?;
    let dir = bottle::dir(paths, bottle_id)?;
    let target = if dest.is_dir() {
        dest.join(file_name(&bottle))
    } else {
        dest.to_path_buf()
    };

    let mut files: Vec<(String, String)> = vec![
        ("report.md".into(), report::debug_report(paths, bottle_id)?),
        ("system.json".into(), json(&system::system_checks(paths))?),
        (
            "bottle.toml".into(),
            toml::to_string_pretty(&bottle).unwrap_or_default(),
        ),
    ];
    if let Ok(rt) = runtime::get(paths, &bottle.runtime) {
        files.push(("runtime.json".into(), json(&rt)?));
    }
    let all_logs = logs::list(&dir)?;
    for log in all_logs.iter().take(EXPORT_LOGS) {
        if let Ok(text) = logs::read(&dir, &log.id) {
            files.push((format!("logs/{}", log.id), text));
        }
    }
    if let Some(newest) = all_logs.first()
        && let Ok(findings) = doctor::diagnose_log(paths, bottle_id, &newest.id)
    {
        files.push(("doctor.json".into(), json(&findings)?));
    }

    let tmp = target.with_extension("zip.part");
    let result = (|| -> Result<()> {
        let file = std::fs::File::create(&tmp).at(&tmp)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in &files {
            let zip_err = |e: zip::result::ZipError| Error::Unsupported(format!("zip: {e}"));
            zip.start_file(format!("vinter-diagnostics/{name}"), opts)
                .map_err(zip_err)?;
            zip.write_all(report::redact_home(text).as_bytes())
                .at(&tmp)?;
        }
        zip.finish()
            .map_err(|e| Error::Unsupported(format!("zip: {e}")))?;
        std::fs::rename(&tmp, &target).at(&target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|()| target)
}

/// The default name of a bottle's diagnostics zip.
pub fn file_name(bottle: &Bottle) -> String {
    let name: String = bottle
        .name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let stamp = humantime::format_rfc3339_seconds(std::time::SystemTime::now())
        .to_string()
        .replace(':', "-");
    format!("vinter-diagnostics-{}-{stamp}.zip", name.trim_matches('-'))
}

fn json(value: &impl serde::Serialize) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(|e| Error::Unsupported(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineKind;

    #[test]
    fn redacts_secret_looking_variables() {
        assert_eq!(redact_value("GITHUB_TOKEN", "abc"), "<redacted>");
        assert_eq!(redact_value("db_password", "abc"), "<redacted>");
        assert_eq!(redact_value("AWS_ACCESS_KEY", "abc"), "<redacted>");
        assert_eq!(redact_value("WINEDEBUG", "-all"), "-all");
        assert_eq!(redact_value("KEYBOARD", "us"), "us");
    }

    #[test]
    fn header_names_engine_runtime_and_env() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let rt_dir = paths.runtimes_dir().join("arm");
        std::fs::create_dir_all(rt_dir.join("bin")).unwrap();
        std::fs::write(
            rt_dir.join("runtime.toml"),
            "id = \"arm\"\nname = \"Apple Silicon Wine 11.10\"\nversion = \"11.10\"\nflavor = \"devel\"\n\
             url = \"\"\nsha256 = \"\"\ninstalled_at = \"2026-10-23T00:00:00Z\"\nwine_bin = \"bin/wine\"\n\
             license = \"\"\nsource_url = \"\"\narch = \"aarch64\"\ngraphics = [\"wined3d\"]\n\
             engine = \"arm64-fex\"\nfex_version = \"FEX-2609\"\n",
        )
        .unwrap();
        let rt = runtime::get(&paths, "arm").unwrap();
        let b: Bottle = toml::from_str(
            "id = \"b\"\nname = \"B\"\nruntime = \"arm\"\nengine = \"arm64-fex\"\ncreated_at = \"2026-10-23T00:00:00Z\"\n",
        )
        .unwrap();
        let home = directories::BaseDirs::new()
            .unwrap()
            .home_dir()
            .display()
            .to_string();
        let env = vec![
            ("WINEPREFIX".to_string(), format!("{home}/x/prefix")),
            ("API_TOKEN".to_string(), "s3cret".to_string()),
            ("PATH".to_string(), "/bin".to_string()),
        ];
        let h = launch_header(
            "Notepad",
            Some((PeArch::Amd64, RunPath::Fex64)),
            &rt,
            &b,
            &env,
        );
        assert_eq!(rt.engine, EngineKind::Arm64Fex);
        for want in [
            "vinter: launching Notepad\n",
            "vinter: engine arm64-fex: Apple Silicon\n",
            "vinter: runtime arm (Apple Silicon Wine 11.10)\n",
            "vinter: wine 11.10, fex FEX-2609\n",
            "vinter: graphics wined3d\n",
            "vinter: architecture amd64 (64-bit), runs as x86-64 translated by FEX (ARM64EC)\n",
            "vinter: env API_TOKEN=<redacted>\n",
            "vinter: env WINEPREFIX=~/x/prefix\n",
        ] {
            assert!(h.contains(want), "missing {want:?} in\n{h}");
        }
        assert!(!h.contains("s3cret") && !h.contains("PATH=") && !h.contains(&home));
    }
}
