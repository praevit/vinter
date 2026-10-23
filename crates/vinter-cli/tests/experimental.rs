//! `vinter experimental` and `--experimental`, run against a temporary data folder.

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn vinter(data: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vinter"))
        .env("VINTER_DATA_DIR", data)
        .args(args)
        .output()
        .unwrap()
}

/// `vinter --json experimental list`, as (id, enabled, saved).
fn list(data: &Path, extra: &[&str]) -> Vec<(String, bool, bool)> {
    let mut args = vec!["--json"];
    args.extend_from_slice(extra);
    args.extend_from_slice(&["experimental", "list"]);
    let out = vinter(data, &args);
    assert!(out.status.success(), "{out:?}");
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["flag"].as_str().unwrap().to_string(),
                f["enabled"].as_bool().unwrap(),
                f["saved"].as_bool().unwrap(),
            )
        })
        .collect()
}

fn state(flags: &[(String, bool, bool)], id: &str) -> (bool, bool) {
    let f = flags.iter().find(|f| f.0 == id).unwrap();
    (f.1, f.2)
}

#[test]
fn enable_and_disable() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path();
    let flags = list(data, &[]);
    assert!(flags.len() > 1);
    assert!(flags.iter().all(|f| !f.1 && !f.2), "all off by default");

    assert!(
        vinter(data, &["experimental", "enable", "compat-notes"])
            .status
            .success()
    );
    assert_eq!(state(&list(data, &[]), "compat-notes"), (true, true));
    let text = std::fs::read_to_string(data.join("settings.toml")).unwrap();
    assert!(text.contains("compat-notes = true"), "{text}");

    assert!(
        vinter(data, &["experimental", "disable", "compat-notes"])
            .status
            .success()
    );
    assert_eq!(state(&list(data, &[]), "compat-notes"), (false, false));
}

#[test]
fn one_command_only() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path();
    let flags = list(data, &["--experimental", "simulate-macos28"]);
    assert_eq!(state(&flags, "simulate-macos28"), (true, false));
    assert_eq!(state(&list(data, &[]), "simulate-macos28"), (false, false));
}

#[test]
fn unknown_ids_are_rejected_on_the_command_line() {
    let tmp = tempfile::tempdir().unwrap();
    let out = vinter(tmp.path(), &["experimental", "enable", "nope"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("compat-notes"));
}

#[test]
fn unknown_ids_in_settings_are_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path();
    std::fs::write(
        data.join("settings.toml"),
        "[experimental]\nfrom-the-future = true\ncompat-notes = true\n",
    )
    .unwrap();
    assert_eq!(state(&list(data, &[]), "compat-notes"), (true, true));
    assert!(
        vinter(data, &["experimental", "enable", "launcher-shims"])
            .status
            .success()
    );
    let text = std::fs::read_to_string(data.join("settings.toml")).unwrap();
    assert!(text.contains("from-the-future = true"), "{text}");
}
