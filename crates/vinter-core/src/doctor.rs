//! Log doctor: finds known problems in a run's log and says how to fix them.
//!
//! Logs are written by Windows programs, so their text is untrusted: it is
//! only matched against fixed patterns here, and quoted lines are kept short.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::bottle::{self, Graphics};
use crate::process::logs;
use crate::{AppPaths, Result};

/// Matching lines quoted per finding, and the most characters per line.
const MAX_EVIDENCE: usize = 3;
const MAX_LINE: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Severity {
    /// Likely why the program failed.
    Error,
    /// May cause problems.
    Warning,
}

/// What would fix a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Fix {
    /// Install these components (winetricks verbs) into the bottle.
    Components { verbs: Vec<String> },
    /// Switch the bottle to this graphics backend.
    Graphics { backend: Graphics },
    /// Roll the bottle back to a snapshot from before it broke.
    RestoreSnapshot,
    /// Nothing Vinter can do for you; the finding's detail says what to try.
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Finding {
    /// Stable id of the pattern, e.g. "missing_dll".
    pub id: String,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub fix: Fix,
    /// Log lines that matched (a few, shortened).
    pub evidence: Vec<String>,
}

/// Diagnoses one of a bottle's logs.
pub fn diagnose_log(paths: &AppPaths, bottle_id: &str, log_id: &str) -> Result<Vec<Finding>> {
    let dir = bottle::dir(paths, bottle_id)?;
    Ok(diagnose(&logs::read(&dir, log_id)?))
}

/// Known problems in a log, most serious first.
pub fn diagnose(text: &str) -> Vec<Finding> {
    let mut found = Findings::default();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if let Some(dll) = missing_dll(line) {
            found.missing_dll(&dll, line);
        } else if line.contains("Wine Mono is not installed")
            || line.contains("Could not load Wine Mono")
        {
            found.add(
                "no_dotnet",
                Severity::Error,
                "The program needs .NET",
                "Wine's own .NET (Wine Mono) is not in this bottle. Installing Microsoft's \
                 .NET Framework makes most .NET programs work.",
                Fix::Components {
                    verbs: vec!["dotnet48".into()],
                },
                line,
            );
        } else if line.contains("Unhandled Exception:")
            || line.contains("FATAL UNHANDLED EXCEPTION")
            || line.contains("NtRaiseException Unhandled exception code e0434352")
        {
            // .NET prints the exception type on the same or the next line.
            let next = lines.peek().copied().unwrap_or_default();
            let kind = dotnet_exception(line).or_else(|| dotnet_exception(next));
            let detail = match &kind {
                Some(k) => format!(
                    "The program stopped with {k}. Programs built for Microsoft's .NET \
                     often work better with it than with Wine Mono."
                ),
                None => "The program stopped with a .NET error. Programs built for \
                         Microsoft's .NET often work better with it than with Wine Mono."
                    .into(),
            };
            found.add(
                "dotnet_exception",
                Severity::Error,
                ".NET program crashed",
                &detail,
                Fix::Components {
                    verbs: vec!["dotnet48".into()],
                },
                line,
            );
        } else if line.starts_with("err:d3dcompiler") || line.contains("HLSL shader parsing failed")
        {
            found.add(
                "d3dcompiler",
                Severity::Warning,
                "Shaders failed to compile",
                "Wine's shader compiler could not handle the program's shaders. Microsoft's \
                 d3dcompiler_47 usually can.",
                Fix::Components {
                    verbs: vec!["d3dcompiler_47".into()],
                },
                line,
            );
        } else if is_vulkan_failure(line) {
            found.add(
                "vulkan",
                Severity::Error,
                "Vulkan is not working",
                "DXVK needs Vulkan, which failed to start. WineD3D works without it.",
                Fix::Graphics {
                    backend: Graphics::Wined3d,
                },
                line,
            );
        } else if line.contains("could not load kernel32.dll")
            || line.contains("could not load ntdll.dll")
        {
            found.add(
                "broken_prefix",
                Severity::Error,
                "The bottle is damaged",
                "Wine could not load its core files from the bottle. Roll it back to a \
                 snapshot from before, or switch its runtime.",
                Fix::RestoreSnapshot,
                line,
            );
        } else if line.contains("Unhandled page fault")
            || line.contains("Unhandled exception: page fault")
        {
            found.add(
                "crash",
                Severity::Error,
                "The program crashed",
                "It read or wrote memory it should not. If it is a game, try another \
                 graphics backend; otherwise look for a recipe or a newer runtime.",
                Fix::Manual,
                line,
            );
        } else if line.contains("ntlm_auth was not found") {
            found.add(
                "ntlm_auth",
                Severity::Warning,
                "Windows sign-in is not available",
                "Programs that sign in to Windows servers (NTLM) need Samba's ntlm_auth, \
                 which is not installed on this computer.",
                Fix::Manual,
                line,
            );
        }
    }
    found.finish()
}

/// `err:module:import_dll Library MSVCP140.dll (which is needed by …) not found`
fn missing_dll(line: &str) -> Option<String> {
    if !line.starts_with("err:module:import_dll") || !line.contains("not found") {
        return None;
    }
    let rest = line.split_once("Library ")?.1;
    let name = rest.split_whitespace().next()?;
    let lower = name.to_ascii_lowercase();
    let ok = lower.ends_with(".dll")
        && lower.len() <= 64
        && lower
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    ok.then_some(lower)
}

/// The winetricks verb that provides a DLL, if there is a well-known one.
pub fn verb_for_dll(dll: &str) -> Option<&'static str> {
    let stem = dll.strip_suffix(".dll").unwrap_or(dll);
    let starts = |p: &str| stem.starts_with(p);
    Some(match stem {
        _ if starts("msvcp140")
            || starts("vcruntime140")
            || starts("concrt140")
            || starts("mfc140")
            || starts("vcomp140") =>
        {
            "vcrun2022"
        }
        "msvcp120" | "msvcr120" | "mfc120" | "vcomp120" => "vcrun2013",
        "msvcp110" | "msvcr110" | "mfc110" | "vcomp110" => "vcrun2012",
        "msvcp100" | "msvcr100" | "mfc100" | "vcomp100" => "vcrun2010",
        "msvcp90" | "msvcr90" | "mfc90" => "vcrun2008",
        "msvcp80" | "msvcr80" | "mfc80" => "vcrun2005",
        "msvcp60" => "vcrun6",
        "mfc42" | "mfc42u" => "mfc42",
        "msvbvm60" => "vb6run",
        _ if starts("d3dx9_") => "d3dx9",
        _ if starts("d3dx10_") => "d3dx10",
        _ if starts("d3dx11_") => "d3dx11_43",
        "d3dcompiler_47" => "d3dcompiler_47",
        "d3dcompiler_43" => "d3dcompiler_43",
        "xaudio2_7" | "x3daudio1_7" | "xactengine3_7" => "xact",
        "physxloader" | "physxcore" => "physx",
        "gdiplus" => "gdiplus",
        "msxml3" => "msxml3",
        "msxml4" => "msxml4",
        "msxml6" => "msxml6",
        "riched20" => "riched20",
        _ => return None,
    })
}

/// `System.IO.FileNotFoundException` from a line that mentions one.
fn dotnet_exception(line: &str) -> Option<String> {
    line.split(|c: char| c.is_whitespace() || c == ':')
        .find(|w| {
            w.starts_with("System.")
                && w.ends_with("Exception")
                && w.len() <= 100
                && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
        })
        .map(str::to_string)
}

fn is_vulkan_failure(line: &str) -> bool {
    line.contains("Failed to create Vulkan instance")
        || line.contains("vkCreateInstance failed")
        || line.contains("No Vulkan devices found")
        || (line.starts_with("err:") && line.contains("DxvkInstance"))
}

fn shorten(line: &str) -> String {
    let line = line.trim();
    match line.char_indices().nth(MAX_LINE) {
        Some((i, _)) => format!("{}…", &line[..i]),
        None => line.to_string(),
    }
}

/// Findings collected so far, one per id (missing DLLs: one per verb).
#[derive(Default)]
struct Findings {
    by_key: BTreeMap<String, Finding>,
    order: Vec<String>,
    /// Missing DLLs no known component provides.
    unknown_dlls: Vec<String>,
}

impl Findings {
    fn add(
        &mut self,
        id: &str,
        severity: Severity,
        title: &str,
        detail: &str,
        fix: Fix,
        line: &str,
    ) {
        self.add_keyed(id.to_string(), id, severity, title, detail, fix, line);
    }

    #[allow(clippy::too_many_arguments)]
    fn add_keyed(
        &mut self,
        key: String,
        id: &str,
        severity: Severity,
        title: &str,
        detail: &str,
        fix: Fix,
        line: &str,
    ) {
        let finding = self.by_key.entry(key.clone()).or_insert_with(|| {
            self.order.push(key);
            Finding {
                id: id.to_string(),
                severity,
                title: title.to_string(),
                detail: detail.to_string(),
                fix,
                evidence: Vec::new(),
            }
        });
        let line = shorten(line);
        if finding.evidence.len() < MAX_EVIDENCE && !finding.evidence.contains(&line) {
            finding.evidence.push(line);
        }
    }

    fn missing_dll(&mut self, dll: &str, line: &str) {
        match verb_for_dll(dll) {
            Some(verb) => self.add_keyed(
                format!("missing_dll:{verb}"),
                "missing_dll",
                Severity::Error,
                &format!("Missing {dll}"),
                &format!("The program needs {dll}, which comes with the {verb} component."),
                Fix::Components {
                    verbs: vec![verb.to_string()],
                },
                line,
            ),
            None => {
                if !self.unknown_dlls.iter().any(|d| d == dll) {
                    self.unknown_dlls.push(dll.to_string());
                }
                self.add(
                    "missing_dll_unknown",
                    Severity::Error,
                    "Missing DLLs",
                    "",
                    Fix::Manual,
                    line,
                );
            }
        }
    }

    fn finish(mut self) -> Vec<Finding> {
        if let Some(f) = self.by_key.get_mut("missing_dll_unknown") {
            f.detail = format!(
                "The program needs {}, which no known component provides. It usually \
                 ships with the program: try reinstalling it.",
                self.unknown_dlls.join(", ")
            );
        }
        let mut out: Vec<_> = self
            .order
            .iter()
            .filter_map(|k| self.by_key.remove(k))
            .collect();
        // Stable: within a severity, the order they first appeared in.
        out.sort_by_key(|f| f.severity);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|f| f.id.as_str()).collect()
    }

    #[test]
    fn missing_dlls_map_to_components_once_per_component() {
        let log = "\
err:module:import_dll Library MSVCP140.dll (which is needed by L\"C:\\\\app\\\\app.exe\") not found
err:module:import_dll Library VCRUNTIME140_1.dll (which is needed by L\"C:\\\\app\\\\app.exe\") not found
err:module:import_dll Library d3dx9_43.dll (which is needed by L\"C:\\\\app\\\\game.exe\") not found
err:module:import_dll Library steam_api.dll (which is needed by L\"C:\\\\app\\\\game.exe\") not found
err:module:LdrInitializeThunk Importing dlls for L\"C:\\\\app\\\\app.exe\" failed, status c0000135
";
        let f = diagnose(log);
        assert_eq!(
            ids(&f),
            ["missing_dll", "missing_dll", "missing_dll_unknown"]
        );
        assert_eq!(
            f[0].fix,
            Fix::Components {
                verbs: vec!["vcrun2022".into()]
            }
        );
        assert_eq!(f[0].title, "Missing msvcp140.dll");
        assert_eq!(f[0].evidence.len(), 2);
        assert_eq!(
            f[1].fix,
            Fix::Components {
                verbs: vec!["d3dx9".into()]
            }
        );
        assert!(f[2].detail.contains("steam_api.dll"));
        assert_eq!(f[2].fix, Fix::Manual);
    }

    #[test]
    fn dotnet_problems_suggest_dotnet() {
        let log = "\
err:mscoree:CLRRuntimeInfo_GetRuntimeHost Wine Mono is not installed
Unhandled Exception:
System.IO.FileNotFoundException: Could not load file or assembly
";
        let f = diagnose(log);
        assert_eq!(ids(&f), ["no_dotnet", "dotnet_exception"]);
        assert!(f[1].detail.contains("System.IO.FileNotFoundException"));
        for finding in &f {
            assert_eq!(
                finding.fix,
                Fix::Components {
                    verbs: vec!["dotnet48".into()]
                }
            );
        }
    }

    #[test]
    fn graphics_and_crash_patterns() {
        let log = "\
warn:   DXVK: Failed to create Vulkan instance
fixme:d3d:wined3d_check_device_format noise
err:d3dcompiler:compile_shader HLSL shader parsing failed.
wine: Unhandled page fault on read access to 0000000000000000 at address 0000000140001000
wine: could not load kernel32.dll, status c0000135
";
        let f = diagnose(log);
        assert_eq!(ids(&f), ["vulkan", "crash", "broken_prefix", "d3dcompiler"]);
        assert_eq!(
            f[0].fix,
            Fix::Graphics {
                backend: Graphics::Wined3d
            }
        );
        assert_eq!(f[2].fix, Fix::RestoreSnapshot);
        assert_eq!(f[3].severity, Severity::Warning);
    }

    #[test]
    fn untrusted_text_is_contained() {
        let long = "x".repeat(10_000);
        let log = format!(
            "err:module:import_dll Library ../../evil{long}.dll (which is needed by L\"a\") not found\n\
             Unhandled Exception: System.{long}Exception\n\
             err:d3dcompiler:{long}\n"
        );
        let f = diagnose(&log);
        // The odd DLL name is not reported; the long lines are cut.
        assert_eq!(ids(&f), ["dotnet_exception", "d3dcompiler"]);
        assert!(!f[0].detail.contains("xxxx"));
        assert!(f[1].evidence[0].chars().count() <= MAX_LINE + 1);
        assert!(diagnose("").is_empty());
    }
}
