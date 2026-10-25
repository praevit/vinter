//! Building the Wine environment and running Wine processes.

pub mod logs;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::bottle::Bottle;
use crate::runtime::Runtime;
use crate::{Error, Result};

/// The full environment for a Wine process in a bottle, in the order of
/// section 4.5 of the plan:
/// `WINEPREFIX`, `WINEDEBUG`, merged `WINEDLLOVERRIDES`, then the bottle's
/// `[env]` table last so the user wins (except for `WINEPREFIX`).
///
/// A domain account's variables (user name, domain, Kerberos) come just
/// before the bottle's `[env]`.
///
/// `base_overrides` come first and are overridden by the bottle's own
/// overrides (graphics backends will pass theirs here in M4).
/// `winemenubuilder` is always disabled: Vinter makes its own launchers (M6),
/// and Wine's would add stray entries to the host's app menus.
pub fn wine_env(
    prefix: &Path,
    bottle: &Bottle,
    base_overrides: &[(&str, &str)],
) -> Vec<(String, String)> {
    let mut overrides: BTreeMap<String, String> = ALWAYS_OVERRIDES
        .iter()
        .chain(base_overrides)
        .map(|(dll, mode)| (dll.to_string(), mode.to_string()))
        .collect();
    overrides.extend(crate::graphics::launch_overrides(bottle.graphics));
    overrides.extend(bottle.dll_overrides.clone());

    let mut env = vec![
        ("WINEPREFIX".to_string(), prefix.display().to_string()),
        (
            "WINEDEBUG".to_string(),
            bottle.options.wine_debug.env_value().to_string(),
        ),
        // MoltenVK (macOS Vulkan) prints ~150 info lines per start; keep errors only.
        ("MVK_CONFIG_LOG_LEVEL".to_string(), "1".to_string()),
    ];
    if !overrides.is_empty() {
        env.push(("WINEDLLOVERRIDES".into(), dll_overrides_string(&overrides)));
    }
    // Bottles live in <data>/bottles/<id>/prefix.
    let data_dir = prefix
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent);
    let backend_env = crate::graphics::launch_env(bottle.graphics, data_dir);
    let option_env = options_env(&bottle.options, std::env::consts::OS);
    let domain_env = match (&bottle.domain, prefix.parent()) {
        (Some(account), Some(dir)) => crate::bottle::domain::env(dir, account),
        _ => Vec::new(),
    };
    let user_env = bottle.env.iter().map(|(k, v)| (k.clone(), v.clone()));
    for (key, value) in backend_env
        .into_iter()
        .chain(option_env)
        .chain(domain_env)
        .chain(user_env)
    {
        if key == "WINEPREFIX" {
            continue;
        }
        env.retain(|(k, _)| *k != key);
        env.push((key, value));
    }
    env
}

/// Environment variables for a bottle's options.
fn options_env(options: &crate::bottle::BottleOptions, os: &str) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if options.show_fps {
        // DXVK draws its own overlay; Metal's covers DXMT, D3DMetal and MoltenVK.
        env.push(("DXVK_HUD".into(), "fps".into()));
        if os == "macos" {
            env.push(("MTL_HUD_ENABLED".into(), "1".into()));
        }
    }
    env
}

const ALWAYS_OVERRIDES: &[(&str, &str)] = &[("winemenubuilder.exe", "d")];

/// Formats overrides as Wine expects: `dll=mode;dll2=mode`.
pub fn dll_overrides_string(overrides: &BTreeMap<String, String>) -> String {
    overrides
        .iter()
        .map(|(dll, mode)| format!("{dll}={mode}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// A command for `program` (e.g. `wine`, `wineserver`) from `runtime`,
/// with the bottle's full environment, both from the runtime's engine.
/// Fails if the bottle was made for a different engine.
pub fn command(
    runtime: &Runtime,
    prefix: &Path,
    bottle: &Bottle,
    base_overrides: &[(&str, &str)],
    program: &str,
) -> Result<Command> {
    crate::engine::check_bottle(runtime, bottle)?;
    let engine = runtime.engine();
    let program = match program {
        "wine" => engine.wine(runtime),
        "wineserver" => engine.wineserver(runtime),
        other => other.into(),
    };
    let mut cmd = runtime.command(&program.to_string_lossy());
    cmd.envs(engine.env(runtime, prefix, bottle, base_overrides));
    cmd.stdin(Stdio::null());
    Ok(cmd)
}

/// Runs `cmd` to completion with output appended to `log`.
/// Fails on a non-zero exit or after `timeout`, quoting the end of the log.
pub fn run_and_wait(cmd: Command, log: &Path, what: &str, timeout: Duration) -> Result<()> {
    match run_status(cmd, log, what, timeout)? {
        Some(0) => Ok(()),
        Some(code) => Err(failed(what, format!("exited with code {code}"), log)),
        None => Err(failed(what, "was stopped by a signal".into(), log)),
    }
}

/// Like [`run_and_wait`], but returns the exit code (`None` for a signal)
/// instead of failing on a non-zero exit.
pub fn run_status(
    mut cmd: Command,
    log: &Path,
    what: &str,
    timeout: Duration,
) -> Result<Option<i32>> {
    let file = std::fs::File::options()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| failed(what, e.to_string(), log))?;
    let err_file = file
        .try_clone()
        .map_err(|e| failed(what, e.to_string(), log))?;
    let child = cmd
        .stdout(file)
        .stderr(err_file)
        .spawn()
        .map_err(|e| failed(what, e.to_string(), log))?;
    wait_with_timeout(child, log, what, timeout)
}

/// Like [`run_and_wait`], with `input` written to the process's standard
/// input (e.g. a password, so it never shows up in the process list).
pub fn run_with_input(
    mut cmd: Command,
    input: &[u8],
    log: &Path,
    what: &str,
    timeout: Duration,
) -> Result<()> {
    let file = std::fs::File::options()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| failed(what, e.to_string(), log))?;
    let err_file = file
        .try_clone()
        .map_err(|e| failed(what, e.to_string(), log))?;
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(file)
        .stderr(err_file)
        .spawn()
        .map_err(|e| failed(what, e.to_string(), log))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A process that exits early closes its end; the exit code tells why.
        let _ = stdin.write_all(input);
    }
    match wait_with_timeout(child, log, what, timeout)? {
        Some(0) => Ok(()),
        Some(code) => Err(failed(what, format!("exited with code {code}"), log)),
        None => Err(failed(what, "was stopped by a signal".into(), log)),
    }
}

fn wait_with_timeout(
    mut child: std::process::Child,
    log: &Path,
    what: &str,
    timeout: Duration,
) -> Result<Option<i32>> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.code()),
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

/// Which output stream a line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Stream {
    Stdout,
    Stderr,
}

/// Starts `cmd` in the background. Every output line is appended to `log`
/// and passed to `on_line`; `on_exit` gets the exit code (`None` if killed
/// by a signal) once the process and its output are done. Returns the pid.
///
/// Wine keeps this process alive exactly as long as the Windows program,
/// so its exit is the program's exit.
pub fn launch(
    mut cmd: Command,
    log: &Path,
    what: &str,
    on_line: impl Fn(Stream, &str) + Send + Sync + 'static,
    on_exit: impl FnOnce(Option<i32>) + Send + 'static,
) -> Result<u32> {
    let file = std::fs::File::options()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| failed(what, e.to_string(), log))?;
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| failed(what, e.to_string(), log))?;
    let pid = child.id();

    let file = Arc::new(Mutex::new(file));
    let on_line = Arc::new(on_line);
    let readers: Vec<_> = [
        (
            Stream::Stdout,
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ),
        (
            Stream::Stderr,
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ),
    ]
    .into_iter()
    .filter_map(|(stream, pipe)| {
        let pipe = pipe?;
        let (file, on_line) = (file.clone(), on_line.clone());
        Some(std::thread::spawn(move || {
            let mut reader = BufReader::new(pipe);
            let mut buf = Vec::new();
            while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                let line = String::from_utf8_lossy(&buf);
                let line = line.trim_end_matches(['\r', '\n']);
                if let Ok(mut f) = file.lock() {
                    let _ = writeln!(f, "{line}");
                }
                on_line(stream, line);
                buf.clear();
            }
        }))
    })
    .collect();

    std::thread::spawn(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        for reader in readers {
            let _ = reader.join();
        }
        on_exit(code);
    });
    Ok(pid)
}

/// Asks a process started by [`launch`] to stop (SIGTERM). Wine then ends
/// the Windows program; other programs in the bottle keep running.
pub fn terminate(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        let pid = libc::pid_t::try_from(pid)
            .map_err(|_| Error::Unsupported(format!("invalid pid {pid}")))?;
        // SAFETY: kill(2) has no memory-safety preconditions.
        if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::ESRCH) {
                return Err(Error::Unsupported(format!("could not stop process: {err}")));
            }
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(Error::Unsupported(
            "stopping programs is not supported here".into(),
        ))
    }
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
            engine: crate::engine::EngineKind::legacy(),
            created_at: "2026-10-14T20:00:00Z".into(),
            windows_version: WindowsVersion::Win10,
            graphics: Graphics::Wined3d,
            recipe: None,
            env: BTreeMap::new(),
            dll_overrides: BTreeMap::new(),
            options: Default::default(),
            programs: vec![],
            domain: None,
        }
    }

    fn get<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn fps_overlay_env() {
        let on = crate::bottle::BottleOptions {
            show_fps: true,
            ..Default::default()
        };
        assert_eq!(
            options_env(&on, "macos"),
            vec![
                ("DXVK_HUD".to_string(), "fps".to_string()),
                ("MTL_HUD_ENABLED".to_string(), "1".to_string())
            ]
        );
        assert_eq!(options_env(&on, "linux").len(), 1);
        assert!(options_env(&Default::default(), "macos").is_empty());
    }

    #[test]
    fn defaults() {
        let env = wine_env(Path::new("/b/prefix"), &bottle(), &[]);
        assert_eq!(get(&env, "WINEPREFIX"), Some("/b/prefix"));
        assert_eq!(get(&env, "WINEDEBUG"), Some("-all,err+all"));
        assert_eq!(get(&env, "WINEDLLOVERRIDES"), Some("winemenubuilder.exe=d"));
        assert_eq!(get(&env, "MVK_CONFIG_LOG_LEVEL"), Some("1"));
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
            Some("d3d11=native,builtin;mscoree=builtin;mshtml=;winemenubuilder.exe=d")
        );
    }

    #[test]
    fn backend_settings_sit_between_base_and_bottle() {
        let mut b = bottle();
        b.graphics = Graphics::Dxvk;
        let env = wine_env(Path::new("/p"), &b, &[]);
        assert_eq!(get(&env, "DXVK_LOG_PATH"), Some("none"));
        let overrides = get(&env, "WINEDLLOVERRIDES").unwrap();
        assert!(overrides.contains("d3d11=n,b"), "{overrides}");

        // The bottle's own settings win over the backend's.
        b.dll_overrides.insert("d3d11".into(), "b".into());
        b.env.insert("DXVK_LOG_PATH".into(), "/tmp".into());
        let env = wine_env(Path::new("/p"), &b, &[]);
        assert!(get(&env, "WINEDLLOVERRIDES").unwrap().contains("d3d11=b;"));
        assert_eq!(get(&env, "DXVK_LOG_PATH"), Some("/tmp"));

        // wined3d adds nothing.
        b.graphics = Graphics::Wined3d;
        b.env.clear();
        b.dll_overrides.clear();
        let env = wine_env(Path::new("/p"), &b, &[]);
        assert_eq!(get(&env, "WINEDLLOVERRIDES"), Some("winemenubuilder.exe=d"));
        assert_eq!(get(&env, "DXVK_LOG_PATH"), None);
    }

    #[test]
    fn domain_account_env() {
        let mut b = bottle();
        assert_eq!(
            get(&wine_env(Path::new("/b/prefix"), &b, &[]), "WINEDOMAIN"),
            None
        );
        b.domain = Some(crate::bottle::DomainAccount {
            user: "jdoe".into(),
            domain: "CORP".into(),
            dns_domain: "corp.example.com".into(),
            computer_name: String::new(),
            kdc: String::new(),
        });
        b.env.insert("USER".into(), "override".into());
        let env = wine_env(Path::new("/b/prefix"), &b, &[]);
        assert_eq!(get(&env, "WINEDOMAIN"), Some("CORP"));
        assert_eq!(get(&env, "KRB5CCNAME"), Some("FILE:/b/kerberos/ccache"));
        // The bottle's own variables still win.
        assert_eq!(get(&env, "USER"), Some("override"));
    }

    #[test]
    fn wine_debug_presets() {
        use crate::bottle::WineDebug;
        let mut b = bottle();
        for (preset, value) in [
            (WineDebug::Off, "-all"),
            (WineDebug::Errors, "-all,err+all"),
            (
                WineDebug::Verbose,
                "err+all,warn+all,fixme+all,+loaddll,+seh",
            ),
        ] {
            b.options.wine_debug = preset;
            assert_eq!(
                get(&wine_env(Path::new("/p"), &b, &[]), "WINEDEBUG"),
                Some(value)
            );
        }
        // The bottle's own WINEDEBUG still wins.
        b.env.insert("WINEDEBUG".into(), "+relay".into());
        assert_eq!(
            get(&wine_env(Path::new("/p"), &b, &[]), "WINEDEBUG"),
            Some("+relay")
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

    /// Files as Vinter 0.1.11 wrote them, before engines existed.
    const OLD_RUNTIME_TOML: &str = r#"
id = "vinter-wine-11.0-4-macos"
name = "Vinter Wine 11.0"
version = "11.0"
flavor = "stable"
url = "https://example.invalid/wine.tar.xz"
sha256 = "1e07264ea20cdcef6d28155f71fe5c1720ffdd8d6035a49a99cfe37e61023eaa"
installed_at = "2026-10-20T10:00:00Z"
wine_bin = "vinter-wine-11.0-4/bin/wine"
license = "LGPL-2.1-or-later"
source_url = "https://example.invalid/src"
arch = "x86_64"
graphics = ["wined3d", "dxvk", "dxmt"]
"#;
    const OLD_BOTTLE_TOML: &str = r#"
id = "8f2c"
name = "Old"
runtime = "vinter-wine-11.0-4-macos"
created_at = "2026-10-20T10:05:00Z"
windows_version = "win10"
graphics = "dxvk"

[env]
WINEESYNC = "1"

[dll_overrides]
mscoree = "builtin"

[options]
retina_mode = true
command_as_ctrl = true
option_as_alt = true
show_fps = true
"#;

    /// Regression test for M8.1: an existing x86_64 bottle launches through
    /// its engine with exactly the program and environment it had before.
    #[test]
    fn existing_bottles_launch_exactly_as_before() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::AppPaths::with_data_dir(tmp.path());
        let rt_dir = paths.runtimes_dir().join("vinter-wine-11.0-4-macos");
        let bin = rt_dir.join("vinter-wine-11.0-4/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("wine"), "").unwrap();
        std::fs::write(bin.join("wineserver"), "").unwrap();
        std::fs::write(rt_dir.join(crate::runtime::RECORD_FILE), OLD_RUNTIME_TOML).unwrap();
        let bottle_dir = paths.bottles_dir().join("8f2c");
        std::fs::create_dir_all(&bottle_dir).unwrap();
        std::fs::write(bottle_dir.join(crate::bottle::CONFIG_FILE), OLD_BOTTLE_TOML).unwrap();

        let rt = crate::runtime::get(&paths, "vinter-wine-11.0-4-macos").unwrap();
        let b = crate::bottle::get(&paths, "8f2c").unwrap();
        assert_eq!(rt.engine, crate::engine::EngineKind::legacy());
        assert_eq!(b.engine, rt.engine);

        let prefix = bottle_dir.join("prefix");
        let p = prefix.display().to_string();
        let mut expected: Vec<(&str, String)> = vec![
            ("WINEPREFIX", p.clone()),
            // Wine errors are logged since the default changed (for the log doctor).
            ("WINEDEBUG", "-all,err+all".into()),
            ("MVK_CONFIG_LOG_LEVEL", "1".into()),
            (
                "WINEDLLOVERRIDES",
                "d3d10core=n,b;d3d11=n,b;mscoree=builtin;mshtml=;winemenubuilder.exe=d".into(),
            ),
            ("DXVK_LOG_PATH", "none".into()),
            ("DXVK_HUD", "fps".into()),
        ];
        if cfg!(target_os = "macos") {
            expected.push(("MTL_HUD_ENABLED", "1".into()));
        }
        expected.push(("WINEESYNC", "1".into()));

        for (program, file) in [("wine", "wine"), ("wineserver", "wineserver")] {
            let cmd = command(&rt, &prefix, &b, &[("mshtml", "")], program).unwrap();
            assert_eq!(cmd.get_program(), bin.join(file).as_os_str());
            let mut envs: Vec<(String, String)> = cmd
                .get_envs()
                .map(|(k, v)| {
                    let v = v
                        .map(|v| v.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    (k.to_string_lossy().into_owned(), v)
                })
                .collect();
            let path = envs.iter().position(|(k, _)| k == "PATH").unwrap();
            let path = envs.remove(path).1;
            assert!(path.starts_with(&bin.display().to_string()), "{path}");
            let mut want: Vec<(String, String)> = expected
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect();
            // Command keeps its environment sorted by name.
            want.sort();
            envs.sort();
            assert_eq!(envs, want);
        }
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

    #[cfg(unix)]
    #[test]
    fn run_with_input_feeds_stdin() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("x.log");
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "read line; test \"$line\" = secret"]);
        run_with_input(cmd, b"secret\n", &log, "read", Duration::from_secs(10)).unwrap();

        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "read line; echo wrong >&2; exit 1"]);
        let err = run_with_input(cmd, b"x\n", &log, "read", Duration::from_secs(10)).unwrap_err();
        assert!(err.to_string().contains("wrong"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn launch_streams_lines_and_reports_exit() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("run.log");
        let lines = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = std::sync::mpsc::channel();
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "echo one; echo two >&2; printf 'three'; exit 4"]);
        let sink = lines.clone();
        launch(
            cmd,
            &log,
            "test",
            move |stream, line| sink.lock().unwrap().push((stream, line.to_string())),
            move |code| tx.send(code).unwrap(),
        )
        .unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), Some(4));
        let mut got = lines.lock().unwrap().clone();
        got.sort_by_key(|(_, l)| l.clone());
        assert_eq!(
            got,
            [
                (Stream::Stdout, "one".to_string()),
                (Stream::Stdout, "three".to_string()),
                (Stream::Stderr, "two".to_string()),
            ]
        );
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("one\n") && text.contains("two\n") && text.contains("three\n"));
    }

    #[cfg(unix)]
    #[test]
    fn terminate_stops_a_launched_process() {
        let tmp = tempfile::tempdir().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "exec sleep 30"]);
        let pid = launch(
            cmd,
            &tmp.path().join("x.log"),
            "sleep",
            |_, _| {},
            move |c| tx.send(c).unwrap(),
        )
        .unwrap();
        terminate(pid).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), None);
        // Stopping an already finished process is not an error.
        terminate(pid).unwrap();
    }
}
