//! Approvals: before an AI agent (through `vinter mcp`) changes anything,
//! the person at the computer allows or denies it in the Vinter app.
//!
//! The asking process writes `<data>/approvals/<id>.request.json` and opens
//! `vinter://approve/<id>`; the app shows the request and writes
//! `<id>.decision.json`, which the asking process waits for.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{IoResultExt, validate_id};
use crate::{AppPaths, Result};

const DIR: &str = "approvals";
/// Requests older than this are left out of [`pending`]: their asker gave up.
const STALE: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ApprovalRequest {
    pub id: String,
    /// Who asks, e.g. the MCP client's name ("Claude Code").
    pub client: String,
    /// What it wants to do, e.g. "install_components".
    pub action: String,
    /// One line for people, e.g. "Install vcrun2022 into “Steam”".
    pub summary: String,
    /// RFC 3339.
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Decision {
    /// Allow this one request.
    Once,
    /// Allow this kind of action until the agent's session ends.
    Session,
    Deny,
}

fn dir(paths: &AppPaths) -> PathBuf {
    paths.data_dir().join(DIR)
}

fn file(paths: &AppPaths, id: &str, kind: &str) -> Result<PathBuf> {
    validate_id(id)?;
    Ok(dir(paths).join(format!("{id}.{kind}.json")))
}

/// Records a request and returns it. Show it with `vinter://approve/<id>`.
pub fn request(
    paths: &AppPaths,
    client: &str,
    action: &str,
    summary: &str,
) -> Result<ApprovalRequest> {
    let req = ApprovalRequest {
        id: uuid::Uuid::new_v4().to_string(),
        client: client.chars().take(80).collect(),
        action: action.to_string(),
        summary: summary.chars().take(300).collect(),
        created_at: humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
    };
    let dir = dir(paths);
    std::fs::create_dir_all(&dir).at(&dir)?;
    write_json(&file(paths, &req.id, "request")?, &req)?;
    Ok(req)
}

/// Requests nobody has answered yet, oldest first.
pub fn pending(paths: &AppPaths) -> Vec<ApprovalRequest> {
    let Ok(entries) = std::fs::read_dir(dir(paths)) else {
        return Vec::new();
    };
    let mut reqs: Vec<ApprovalRequest> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name().to_string_lossy().ends_with(".request.json")
                && e.metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t.elapsed().unwrap_or_default() < STALE)
        })
        .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
        .filter(|r: &ApprovalRequest| file(paths, &r.id, "decision").is_ok_and(|d| !d.exists()))
        .collect();
    reqs.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
    reqs
}

/// Answers a request.
pub fn decide(paths: &AppPaths, id: &str, decision: Decision) -> Result<()> {
    let req = file(paths, id, "request")?;
    if !req.is_file() {
        return Err(crate::Error::ApprovalNotFound(id.to_string()));
    }
    write_json(&file(paths, id, "decision")?, &decision)
}

/// Waits for the answer to a request; `None` if nobody answered in time.
/// The request's files are removed either way.
pub fn wait(paths: &AppPaths, id: &str, timeout: Duration) -> Result<Option<Decision>> {
    let req = file(paths, id, "request")?;
    let dec = file(paths, id, "decision")?;
    let start = Instant::now();
    let decision = loop {
        if let Some(d) = std::fs::read(&dec)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
        {
            break Some(d);
        }
        if start.elapsed() >= timeout {
            break None;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let _ = std::fs::remove_file(&req);
    let _ = std::fs::remove_file(&dec);
    Ok(decision)
}

/// Writes through a temporary file, so readers never see half a file.
fn write_json(path: &std::path::Path, value: &impl Serialize) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value).expect("serializes");
    std::fs::write(&tmp, bytes).at(&tmp)?;
    std::fs::rename(&tmp, path).at(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_decide_and_wait() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        assert!(pending(&paths).is_empty());

        let req = request(&paths, "Test agent", "launch_app", "Open Notepad").unwrap();
        assert_eq!(pending(&paths), std::slice::from_ref(&req));

        decide(&paths, &req.id, Decision::Session).unwrap();
        assert!(
            pending(&paths).is_empty(),
            "answered requests are not pending"
        );
        let got = wait(&paths, &req.id, Duration::from_secs(1)).unwrap();
        assert_eq!(got, Some(Decision::Session));
        assert!(std::fs::read_dir(dir(&paths)).unwrap().next().is_none());

        let req = request(&paths, "Test agent", "launch_app", "Open Notepad").unwrap();
        assert_eq!(wait(&paths, &req.id, Duration::ZERO).unwrap(), None);
        assert!(pending(&paths).is_empty(), "a timed-out request is gone");
        assert!(decide(&paths, &req.id, Decision::Once).is_err());
        assert!(decide(&paths, "../x", Decision::Once).is_err());
    }
}
