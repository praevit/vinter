//! "Copy debug report": everything useful for a bug report about a bottle,
//! as plain text. The user's home folder is replaced with `~`.

use std::fmt::Write;

use crate::bottle::{self, PREFIX_DIR};
use crate::process::logs;
use crate::{AppPaths, Result, graphics, recipe, runtime, system};

/// Lines of the newest log to include.
const LOG_TAIL: usize = 150;

pub fn debug_report(paths: &AppPaths, bottle_id: &str) -> Result<String> {
    let bottle = bottle::get(paths, bottle_id)?;
    let dir = bottle::dir(paths, bottle_id)?;
    let mut out = String::new();

    let _ = writeln!(out, "# Vinter debug report\n");
    let _ = writeln!(out, "Vinter {}", crate::VERSION);
    let _ = writeln!(out, "OS: {} ({})", os_description(), std::env::consts::ARCH);
    let status = system::system_checks(paths);
    for c in &status.checks {
        let _ = writeln!(out, "Check {}: {:?}: {}", c.label, c.status, c.detail);
    }

    let _ = writeln!(out, "\n## Runtime");
    match runtime::get(paths, &bottle.runtime) {
        Ok(rt) => {
            let _ = writeln!(
                out,
                "{} ({}), {} {}, needs Rosetta: {}, graphics: {}",
                rt.name,
                rt.id,
                rt.arch,
                rt.installed_at,
                rt.needs_rosetta,
                rt.graphics
                    .iter()
                    .map(|g| g.id())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            for b in graphics::backends(paths, &rt) {
                let state = match (&b.reason, &b.component, b.component_installed) {
                    (Some(_), _, _) => "unavailable".to_string(),
                    (None, Some(c), installed) => format!(
                        "{} ({})",
                        c.name,
                        if installed {
                            "installed"
                        } else {
                            "not downloaded"
                        }
                    ),
                    (None, None, _) => "built in".to_string(),
                };
                let _ = writeln!(out, "  backend {}: {state}", b.backend.id());
            }
        }
        Err(e) => {
            let _ = writeln!(out, "{}: {e}", bottle.runtime);
        }
    }

    let _ = writeln!(out, "\n## bottle.toml\n```toml");
    let _ = write!(
        out,
        "{}",
        toml::to_string_pretty(&bottle).unwrap_or_default()
    );
    let _ = writeln!(out, "```");
    let _ = writeln!(
        out,
        "Prefix exists: {}",
        dir.join(PREFIX_DIR).join("drive_c").is_dir()
    );
    if let Some(r) = recipe::state(paths, bottle_id) {
        let _ = writeln!(
            out,
            "Unfinished recipe {}: {}/{} steps, error: {}",
            r.recipe_id,
            r.completed_steps,
            r.total_steps,
            r.error.as_deref().unwrap_or("none")
        );
    }

    let all_logs = logs::list(&dir)?;
    let _ = writeln!(out, "\n## Recent logs");
    for l in all_logs.iter().take(10) {
        let _ = writeln!(out, "{}  {}  {} bytes", l.started_at, l.label, l.size);
    }
    if let Some(newest) = all_logs.first() {
        let text = logs::read(&dir, &newest.id).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let tail = &lines[lines.len().saturating_sub(LOG_TAIL)..];
        let _ = writeln!(
            out,
            "\n## End of {} ({} of {} lines)\n```",
            newest.id,
            tail.len(),
            lines.len()
        );
        for line in tail {
            let _ = writeln!(out, "{line}");
        }
        let _ = writeln!(out, "```");
    }

    Ok(redact_home(&out))
}

fn os_description() -> String {
    let run = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    if cfg!(target_os = "macos") {
        let version = run("/usr/bin/sw_vers", &["-productVersion"]).unwrap_or_default();
        let build = run("/usr/bin/sw_vers", &["-buildVersion"]).unwrap_or_default();
        format!("macOS {version} ({build})")
    } else {
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|t| {
                t.lines()
                    .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                    .map(|v| v.trim_matches('"').to_string())
            })
            .unwrap_or_else(|| std::env::consts::OS.to_string())
    }
}

fn redact_home(text: &str) -> String {
    match directories::BaseDirs::new() {
        Some(base) => {
            let home = base.home_dir().display().to_string();
            if home.len() > 1 {
                text.replace(&home, "~")
            } else {
                text.to_string()
            }
        }
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bottle::{Bottle, Graphics, WindowsVersion};

    #[test]
    fn report_has_settings_logs_and_no_home_path() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path().join("data"));
        let dir = bottle::dir(&paths, "b").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        Bottle {
            id: "b".into(),
            name: "Games".into(),
            runtime: "missing-rt".into(),
            engine: crate::engine::EngineKind::legacy(),
            created_at: "2026-10-15T00:00:00Z".into(),
            windows_version: WindowsVersion::Win7,
            graphics: Graphics::Dxvk,
            recipe: None,
            env: Default::default(),
            dll_overrides: Default::default(),
            options: Default::default(),
            programs: vec![],
            domain: None,
        }
        .save(&dir)
        .unwrap();
        let (log, _) = logs::create(&dir, "game").unwrap();
        let home = directories::BaseDirs::new()
            .unwrap()
            .home_dir()
            .display()
            .to_string();
        std::fs::write(
            &log,
            format!("line 1\nloaded {home}/secret/file\nerr: crash\n"),
        )
        .unwrap();

        let r = debug_report(&paths, "b").unwrap();
        assert!(r.contains("Vinter "));
        assert!(r.contains("windows_version = \"win7\""));
        assert!(r.contains("graphics = \"dxvk\""));
        assert!(r.contains("missing-rt: runtime \"missing-rt\" is not installed"));
        assert!(r.contains("err: crash"));
        assert!(r.contains("loaded ~/secret/file"));
        assert!(!r.contains(&home), "home path must be redacted");
    }
}
