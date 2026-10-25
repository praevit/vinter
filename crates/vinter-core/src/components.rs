//! Windows components (Visual C++ runtimes, .NET, DirectX parts, fonts)
//! installed into a bottle with winetricks: a short, curated list of its
//! most useful verbs. Only verbs from this list can be installed this way.

use std::path::Path;

use serde::Serialize;

use crate::bottle::{self, PREFIX_DIR, snapshot};
use crate::download::InstallProgress;
use crate::recipe::winetricks;
use crate::{AppPaths, Error, Result};

/// The file in a prefix where winetricks lists the verbs it installed.
const WINETRICKS_LOG: &str = "winetricks.log";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Category {
    Runtimes,
    Dotnet,
    Directx,
    Fonts,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WindowsComponent {
    /// Winetricks verb, e.g. "vcrun2022".
    pub verb: String,
    pub name: String,
    pub description: String,
    pub category: Category,
    /// Installed in this bottle (for [`list`]).
    pub installed: bool,
}

/// verb, name, description, category.
const CATALOG: &[(&str, &str, &str, Category)] = &[
    (
        "vcrun2022",
        "Visual C++ 2015–2022",
        "Needed by most current programs and games (msvcp140.dll, vcruntime140.dll).",
        Category::Runtimes,
    ),
    (
        "vcrun2013",
        "Visual C++ 2013",
        "msvcp120.dll and msvcr120.dll.",
        Category::Runtimes,
    ),
    (
        "vcrun2012",
        "Visual C++ 2012",
        "msvcp110.dll and msvcr110.dll.",
        Category::Runtimes,
    ),
    (
        "vcrun2010",
        "Visual C++ 2010",
        "msvcp100.dll and msvcr100.dll.",
        Category::Runtimes,
    ),
    (
        "vcrun2008",
        "Visual C++ 2008",
        "msvcp90.dll and msvcr90.dll.",
        Category::Runtimes,
    ),
    (
        "vcrun2005",
        "Visual C++ 2005",
        "msvcp80.dll and msvcr80.dll.",
        Category::Runtimes,
    ),
    (
        "vcrun6",
        "Visual C++ 6",
        "msvcp60.dll, for very old programs.",
        Category::Runtimes,
    ),
    (
        "mfc42",
        "MFC 4.2",
        "mfc42.dll, for old programs built with MFC.",
        Category::Runtimes,
    ),
    (
        "vb6run",
        "Visual Basic 6",
        "For programs written in Visual Basic 6.",
        Category::Runtimes,
    ),
    (
        "dotnet48",
        ".NET Framework 4.8",
        "For .NET programs that do not work with Wine Mono. Takes a while to install.",
        Category::Dotnet,
    ),
    (
        "dotnet35",
        ".NET Framework 3.5",
        "For older .NET programs (2.0 to 3.5).",
        Category::Dotnet,
    ),
    (
        "dotnetdesktop8",
        ".NET Desktop Runtime 8",
        "For current .NET programs with a window (WPF and Windows Forms).",
        Category::Dotnet,
    ),
    (
        "d3dx9",
        "DirectX 9 extras (d3dx9)",
        "d3dx9_*.dll, needed by many older games.",
        Category::Directx,
    ),
    (
        "d3dx10",
        "DirectX 10 extras (d3dx10)",
        "d3dx10_*.dll.",
        Category::Directx,
    ),
    (
        "d3dx11_43",
        "DirectX 11 extras (d3dx11)",
        "d3dx11_42.dll and d3dx11_43.dll.",
        Category::Directx,
    ),
    (
        "d3dcompiler_47",
        "Shader compiler 47",
        "Microsoft's d3dcompiler_47.dll, for games whose shaders Wine cannot compile.",
        Category::Directx,
    ),
    (
        "d3dcompiler_43",
        "Shader compiler 43",
        "d3dcompiler_43.dll.",
        Category::Directx,
    ),
    (
        "xact",
        "XACT audio",
        "XAudio2 and X3DAudio, for game sound.",
        Category::Directx,
    ),
    (
        "physx",
        "PhysX",
        "NVIDIA's physics engine, for some games.",
        Category::Directx,
    ),
    (
        "corefonts",
        "Core fonts",
        "Arial, Times New Roman, Verdana and the other classic Windows fonts.",
        Category::Fonts,
    ),
    (
        "tahoma",
        "Tahoma",
        "Windows' old interface font.",
        Category::Fonts,
    ),
    (
        "cjkfonts",
        "Chinese, Japanese and Korean fonts",
        "So CJK text shows instead of boxes.",
        Category::Fonts,
    ),
    (
        "gdiplus",
        "GDI+",
        "Microsoft's gdiplus.dll, for drawing problems.",
        Category::Other,
    ),
    ("msxml3", "MSXML 3", "Microsoft XML 3.", Category::Other),
    ("msxml4", "MSXML 4", "Microsoft XML 4.", Category::Other),
    ("msxml6", "MSXML 6", "Microsoft XML 6.", Category::Other),
    (
        "riched20",
        "Rich Edit",
        "Microsoft's riched20.dll, for text box problems.",
        Category::Other,
    ),
];

/// The components, each marked installed or not in the bottle.
pub fn list(paths: &AppPaths, bottle_id: &str) -> Result<Vec<WindowsComponent>> {
    let installed = installed_verbs(&bottle::dir(paths, bottle_id)?.join(PREFIX_DIR));
    Ok(CATALOG
        .iter()
        .map(|&(verb, name, description, category)| WindowsComponent {
            verb: verb.into(),
            name: name.into(),
            description: description.into(),
            category,
            installed: installed.iter().any(|v| v == verb),
        })
        .collect())
}

/// Whether a verb is in the catalog.
pub fn is_known(verb: &str) -> bool {
    CATALOG.iter().any(|&(v, ..)| v == verb)
}

/// The catalog's name for a verb, e.g. ".NET Framework 4.8".
pub fn name(verb: &str) -> Option<&'static str> {
    CATALOG
        .iter()
        .find(|&&(v, ..)| v == verb)
        .map(|&(_, name, ..)| name)
}

/// Installs components into a bottle. Winetricks may download software
/// from Microsoft and others, so `accepted` must say the user agreed.
/// Snapshots the bottle first where that is free. Blocks for minutes:
/// .NET can take a long time.
pub async fn install<P>(
    paths: &AppPaths,
    bottle_id: &str,
    verbs: &[String],
    accepted: bool,
    mut on_download: P,
) -> Result<()>
where
    P: FnMut(InstallProgress) + Send,
{
    if verbs.is_empty() {
        return Ok(());
    }
    if let Some(unknown) = verbs.iter().find(|v| !is_known(v)) {
        return Err(Error::UnknownComponent(unknown.clone()));
    }
    if !accepted {
        return Err(Error::ConsentRequired(verbs.join(", ")));
    }
    let bottle = bottle::get(paths, bottle_id)?;
    if bottle::dir(paths, bottle_id)?
        .join(bottle::SETUP_MARKER)
        .exists()
    {
        return Err(Error::BottleNotReady(bottle.name));
    }
    let script = winetricks::ensure(paths, &mut on_download).await?;
    let (paths, id, verbs) = (paths.clone(), bottle_id.to_string(), verbs.to_vec());
    tokio::task::spawn_blocking(move || {
        snapshot::auto(
            &paths,
            &id,
            &format!("Before installing {}", verbs.join(", ")),
        )?;
        winetricks::run(&paths, &id, &script, &verbs)
    })
    .await
    .map_err(|e| Error::ProcessFailed {
        what: "winetricks".into(),
        message: e.to_string(),
    })?
}

/// Verbs winetricks recorded as installed in a prefix.
fn installed_verbs(prefix: &Path) -> Vec<String> {
    std::fs::read_to_string(prefix.join(WINETRICKS_LOG))
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_doctor_suggestion_can_be_installed() {
        let dlls = [
            "msvcp140.dll",
            "vcruntime140_1.dll",
            "msvcr120.dll",
            "msvcp110.dll",
            "msvcr100.dll",
            "msvcr90.dll",
            "msvcr80.dll",
            "msvcp60.dll",
            "mfc42.dll",
            "msvbvm60.dll",
            "d3dx9_43.dll",
            "d3dx10_43.dll",
            "d3dx11_43.dll",
            "d3dcompiler_47.dll",
            "d3dcompiler_43.dll",
            "xaudio2_7.dll",
            "physxloader.dll",
            "gdiplus.dll",
            "msxml3.dll",
            "msxml4.dll",
            "msxml6.dll",
            "riched20.dll",
        ];
        for dll in dlls {
            let verb = crate::doctor::verb_for_dll(dll).unwrap_or_else(|| panic!("{dll}"));
            assert!(is_known(verb), "{verb} (for {dll}) is not in the catalog");
        }
        assert!(is_known("dotnet48"));
    }

    #[test]
    fn catalog_verbs_are_unique_and_plain() {
        let mut verbs: Vec<_> = CATALOG.iter().map(|&(v, ..)| v).collect();
        assert!(
            verbs
                .iter()
                .all(|v| v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        );
        verbs.sort();
        verbs.dedup();
        assert_eq!(verbs.len(), CATALOG.len());
    }

    #[test]
    fn reads_what_winetricks_installed() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(installed_verbs(tmp.path()).is_empty());
        std::fs::write(tmp.path().join(WINETRICKS_LOG), "vcrun2022\n\ncorefonts\n").unwrap();
        assert_eq!(installed_verbs(tmp.path()), ["vcrun2022", "corefonts"]);
    }

    #[tokio::test]
    async fn refuses_unknown_verbs_and_missing_consent() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let install = |verbs: Vec<&str>, ok: bool| {
            let verbs: Vec<String> = verbs.into_iter().map(String::from).collect();
            let paths = paths.clone();
            async move { install(&paths, "some-bottle", &verbs, ok, |_| {}).await }
        };
        assert!(matches!(
            install(vec!["vcrun2022", "--force; rm"], true).await,
            Err(Error::UnknownComponent(_))
        ));
        assert!(matches!(
            install(vec!["corefonts"], false).await,
            Err(Error::ConsentRequired(_))
        ));
        assert!(install(vec![], false).await.is_ok());
    }
}
