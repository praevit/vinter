//! Building the Wine environment and running Wine processes.

pub mod logs;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::bottle::Bottle;
use crate::runtime::Runtime;
use crate::{Error, Result};

/// The full environment for a Wine process in a bottle, in the order of
/// section 4.5 of the plan:
/// `WINEPREFIX`, `WINEDEBUG`, merged `WINEDLLOVERRIDES`, then the bottle's
/// `[env]` table last so the user wins (except for `WINEPREFIX`).
///
/// `base_overrides` come first and are overridden by the bottle's own
/// overrides (graphics backends will pass theirs here in M4).
pub fn wine_env(
    prefix: &Path,
    bottle: &Bottle,
    base_overrides: &[(&str, &str)],
) -> Vec<(String, String)> {
    let mut overrides: BTreeMap<String, String> = base_overrides
        .iter()
        .map(|(dll, mode)| (dll.to_string(), mode.to_string()))
        .collect();
    overrides.extend(bottle.dll_overrides.clone());

    let mut env = vec![
        ("WINEPREFIX".to_string(), prefix.display().to_string()),
        ("WINEDEBUG".to_string(), "-all".to_string()),
    ];
    if !overrides.is_empty() {
        env.push(("WINEDLLOVERRIDES".into(), dll_overrides_string(&overrides)));
    }
    for (key, value) in &bottle.env {
        if key == "WINEPREFIX" {
            continue;
        }
        env.retain(|(k, _)| k != key);
        env.push((key.clone(), value.clone()));
    }
    env
}

/// Formats overrides as Wine expects: `dll=mode;dll2=mode`.
pub fn dll_overrides_string(overrides: &BTreeMap<String, String>) -> String {
    overrides
        .iter()
        .map(|(dll, mode)| format!("{dll}={mode}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// A command for `program` (e.g. `wine`, `wineserver`) from `runtime`,
/// with the bottle's full environment.
pub fn command(
    runtime: &Runtime,
    prefix: &Path,
    bottle: &Bottle,
    base_overrides: &[(&str, &str)],
    program: &str,
) -> Command {
    let mut cmd = runtime.command(program);
    cmd.envs(wine_env(prefix, bottle, base_overrides));
    cmd.stdin(Stdio::null());
    cmd
}

/// Runs `cmd` to completion with output appended to `log`.
/// Fails on a non-zero exit or after `timeout`, quoting the end of the log.
pub fn run_and_wait(mut cmd: Command, log: &Path, what: &str, timeout: Duration) -> Result<()> {
    let file = std::fs::File::options()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| failed(what, e.to_string(), log))?;
    let err_file = file
        .try_clone()
        .map_err(|e| failed(what, e.to_string(), log))?;
    let mut child = cmd
        .stdout(file)
        .stderr(err_file)
        .spawn()
        .map_err(|e| failed(what, e.to_string(), log))?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(failed(what, format!("exited with {status}"), log)),
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failed(
                    what,
                    format!("timed out after {}s", timeout.as_secs()),
                    log,
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(failed(what, e.to_string(), log)),
        }
    }
}

/// Starts `cmd` in the background with output appended to `log` and returns
/// its pid. A thread reaps it when it exits (M3 adds proper tracking).
pub fn spawn_detached(mut cmd: Command, log: &Path, what: &str) -> Result<u32> {
    let file = std::fs::File::options()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| failed(what, e.to_string(), log))?;
    let err_file = file
        .try_clone()
        .map_err(|e| failed(what, e.to_string(), log))?;
    let mut child = cmd
        .stdout(file)
        .stderr(err_file)
        .spawn()
        .map_err(|e| failed(what, e.to_string(), log))?;
    let pid = child.id();
    std::thread::spawn(move || child.wait());
    Ok(pid)
}

fn failed(what: &str, reason: String, log: &Path) -> Error {
    let tail = logs::tail(log, 15);
    Error::ProcessFailed {
        what: what.to_string(),
        message: if tail.is_empty() {
            reason
        } else {
            format!("{reason}\n{tail}")
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bottle::{Graphics, WindowsVersion};

    fn bottle() -> Bottle {
        Bottle {
            id: "b".into(),
            name: "B".into(),
            runtime: "r".into(),
            created_at: "2026-10-14T20:00:00Z".into(),
            windows_version: WindowsVersion::Win10,
            graphics: Graphics::Wined3d,
            recipe: None,
            env: BTreeMap::new(),
            dll_overrides: BTreeMap::new(),
            programs: vec![],
        }
    }

    fn get<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn defaults() {
        let env = wine_env(Path::new("/b/prefix"), &bottle(), &[]);
        assert_eq!(get(&env, "WINEPREFIX"), Some("/b/prefix"));
        assert_eq!(get(&env, "WINEDEBUG"), Some("-all"));
        assert_eq!(get(&env, "WINEDLLOVERRIDES"), None);
    }

    #[test]
    fn bottle_overrides_win_over_base() {
        let mut b = bottle();
        b.dll_overrides
            .insert("d3d11".into(), "native,builtin".into());
        b.dll_overrides.insert("mscoree".into(), "builtin".into());
        let env = wine_env(Path::new("/p"), &b, &[("mscoree", ""), ("mshtml", "")]);
        assert_eq!(
            get(&env, "WINEDLLOVERRIDES"),
            Some("d3d11=native,builtin;mscoree=builtin;mshtml=")
        );
    }

    #[test]
    fn bottle_env_is_last_but_cannot_move_the_prefix() {
        let mut b = bottle();
        b.env.insert("WINEDEBUG".into(), "+loaddll".into());
        b.env.insert("WINEPREFIX".into(), "/elsewhere".into());
        b.env.insert("WINEESYNC".into(), "1".into());
        let env = wine_env(Path::new("/p"), &b, &[]);
        assert_eq!(get(&env, "WINEDEBUG"), Some("+loaddll"));
        assert_eq!(get(&env, "WINEPREFIX"), Some("/p"));
        assert_eq!(get(&env, "WINEESYNC"), Some("1"));
        assert_eq!(env.iter().filter(|(k, _)| k == "WINEDEBUG").count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn run_and_wait_reports_failures_with_log_tail() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("x.log");
        let mut ok = Command::new("/bin/sh");
        ok.args(["-c", "echo hello"]);
        run_and_wait(ok, &log, "echo", Duration::from_secs(10)).unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "hello\n");

        let mut bad = Command::new("/bin/sh");
        bad.args(["-c", "echo boom >&2; exit 3"]);
        let err = run_and_wait(bad, &log, "failing step", Duration::from_secs(10)).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("failing step") && msg.contains("boom"),
            "{msg}"
        );

        let mut slow = Command::new("/bin/sh");
        slow.args(["-c", "sleep 5"]);
        let err = run_and_wait(slow, &log, "slow", Duration::from_millis(200)).unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }
}
