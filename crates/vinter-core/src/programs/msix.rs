//! Installing MSIX and AppX packages (and their bundles) into a bottle.
//!
//! Wine has no package deployment service, but a package is a zip: Vinter
//! unpacks it to `C:\Program Files\<name>`, puts the files under `VFS\` where
//! they belong (System32, Program Files, AppData, …), and adds the package's
//! applications as programs. Encrypted Microsoft Store downloads cannot be
//! unpacked; apps that are not desktop programs (UWP) usually do not run.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use super::{ProgramDef, fnv1a, icons, winpath};
use crate::bottle::{self, ICONS_DIR, PREFIX_DIR};
use crate::error::IoResultExt;
use crate::process::logs;
use crate::{AppPaths, Error, Result};

const EXTENSIONS: &[&str] = &["msix", "appx", "msixbundle", "appxbundle"];
const ENCRYPTED: &[&str] = &["emsix", "eappx", "emsixbundle", "eappxbundle"];
const MANIFEST: &str = "AppxManifest.xml";
const BUNDLE_MANIFEST: &str = "AppxMetadata/AppxBundleManifest.xml";
/// Package metadata that is not part of the app.
const SKIP: &[&str] = &[
    "AppxBlockMap.xml",
    "AppxSignature.p7x",
    "[Content_Types].xml",
];
/// Bundle architectures in order of preference (Vinter runs x86_64 Windows).
const ARCH_PREFERENCE: &[&str] = &["x64", "neutral", "x86"];

/// What installing a package did.
#[derive(Debug, Clone)]
pub struct Installed {
    pub name: String,
    pub programs: Vec<ProgramDef>,
    /// Things that may keep the app from running, for the log and the UI.
    pub warnings: Vec<String>,
}

/// True for `.msix`, `.appx` and their bundles (and the encrypted Store
/// variants, which [`install`] then explains it cannot open).
pub fn is_package(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        let e = e.to_string_lossy().to_lowercase();
        EXTENSIONS.contains(&e.as_str()) || ENCRYPTED.contains(&e.as_str())
    })
}

/// Unpacks a package into a bottle and adds its applications as programs.
/// Writes what it did to an "installer" log in the bottle.
pub fn install(paths: &AppPaths, bottle_id: &str, package: &Path) -> Result<Installed> {
    let dir = bottle::dir(paths, bottle_id)?;
    let prefix = dir.join(PREFIX_DIR);
    let ext = package
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ENCRYPTED.contains(&ext.as_str()) {
        return Err(encrypted(package));
    }
    let (_, mut log) = logs::create(&dir, "installer")?;
    let _ = writeln!(log, "Installing {}", package.display());

    let mut zip = open(package)?;
    // A bundle holds one package per architecture: take the best one out.
    let mut staged = None;
    if zip.by_name(BUNDLE_MANIFEST).is_ok() {
        let manifest = read_text(&mut zip, BUNDLE_MANIFEST)?;
        let inner = pick_from_bundle(&manifest)?;
        let _ = writeln!(log, "Bundle: using {inner}");
        let tmp = dir.join(format!(".package-{}.msix", uuid::Uuid::new_v4()));
        {
            let index = zip
                .index_for_name(&encode_name(&inner))
                .or_else(|| zip.index_for_name(&inner))
                .ok_or_else(|| Error::BadArchive(format!("{inner} is missing from the bundle")))?;
            let mut entry = zip
                .by_index(index)
                .map_err(|e| Error::BadArchive(format!("cannot read {inner}: {e}")))?;
            let mut out = File::create(&tmp).at(&tmp)?;
            std::io::copy(&mut entry, &mut out).at(&tmp)?;
        }
        zip = open(&tmp)?;
        staged = Some(tmp);
    }
    let result = install_package(&mut zip, &prefix, &dir, &mut log);
    if let Some(tmp) = staged {
        let _ = std::fs::remove_file(tmp);
    }
    let installed = result?;
    for w in &installed.warnings {
        let _ = writeln!(log, "Warning: {w}");
    }
    let _ = writeln!(
        log,
        "Installed {} with {} program(s).",
        installed.name,
        installed.programs.len()
    );
    super::add(paths, bottle_id, installed.programs.clone())?;
    Ok(installed)
}

fn open(path: &Path) -> Result<zip::ZipArchive<File>> {
    let file = File::open(path).at(path)?;
    zip::ZipArchive::new(file).map_err(|_| {
        // Store packages are encrypted: not a zip at all.
        let mut head = [0u8; 4];
        let encrypted_store = File::open(path)
            .and_then(|mut f| f.read_exact(&mut head))
            .is_ok_and(|_| &head == b"EXTX" || &head[..2] != b"PK");
        if encrypted_store {
            encrypted(path)
        } else {
            Error::BadArchive(format!("{} is not a readable MSIX package", path.display()))
        }
    })
}

fn encrypted(path: &Path) -> Error {
    Error::BadArchive(format!(
        "{} is an encrypted Microsoft Store package, which only Windows can open. \
         Ask for the app's MSIX or installer instead.",
        path.file_name().unwrap_or_default().to_string_lossy()
    ))
}

fn read_text(zip: &mut zip::ZipArchive<File>, name: &str) -> Result<String> {
    let mut entry = zip
        .by_name(name)
        .map_err(|_| Error::BadArchive(format!("{name} is missing from the package")))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| Error::BadArchive(format!("cannot read {name}: {e}")))?;
    Ok(text)
}

/// The file name of the bundle's application package for x64 (or neutral, x86).
fn pick_from_bundle(manifest: &str) -> Result<String> {
    let doc = roxmltree::Document::parse(manifest)
        .map_err(|e| Error::BadArchive(format!("the bundle manifest is not valid XML: {e}")))?;
    let packages: Vec<(String, String)> = doc
        .descendants()
        .filter(|n| n.tag_name().name() == "Package")
        .filter(|n| {
            n.attribute("Type")
                .is_none_or(|t| t.eq_ignore_ascii_case("application"))
        })
        .filter_map(|n| {
            let file = n.attribute("FileName")?.to_string();
            let arch = n
                .attribute("Architecture")
                .unwrap_or("neutral")
                .to_lowercase();
            Some((arch, file))
        })
        .collect();
    ARCH_PREFERENCE
        .iter()
        .find_map(|want| packages.iter().find(|(arch, _)| arch == want))
        .map(|(_, file)| file.clone())
        .ok_or_else(|| {
            let found: Vec<_> = packages.iter().map(|(a, _)| a.as_str()).collect();
            Error::Unsupported(format!(
                "this bundle has no package for Intel PCs (only {})",
                if found.is_empty() {
                    "none".into()
                } else {
                    found.join(", ")
                }
            ))
        })
}

/// One `<Application>` in a package manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
struct App {
    id: String,
    executable: String,
    display_name: Option<String>,
    logo: Option<String>,
    desktop: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Manifest {
    identity: String,
    display_name: Option<String>,
    logo: Option<String>,
    apps: Vec<App>,
    dependencies: Vec<String>,
}

/// Text that is not a reference into the package's resources.
fn plain(text: Option<&str>) -> Option<String> {
    let t = text?.trim();
    (!t.is_empty() && !t.starts_with("ms-resource:")).then(|| t.to_string())
}

fn parse_manifest(xml: &str) -> Result<Manifest> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|e| Error::BadArchive(format!("AppxManifest.xml is not valid XML: {e}")))?;
    let find = |name: &str| doc.descendants().find(|n| n.tag_name().name() == name);
    let identity = find("Identity")
        .and_then(|n| n.attribute("Name"))
        .unwrap_or("App")
        .to_string();
    let properties = find("Properties");
    let child_text = |parent: Option<roxmltree::Node>, name: &str| {
        parent?
            .children()
            .find(|c| c.tag_name().name() == name)
            .and_then(|c| c.text())
            .map(str::to_string)
    };
    let apps = doc
        .descendants()
        .filter(|n| n.tag_name().name() == "Application")
        .map(|n| {
            let visual = n
                .descendants()
                .find(|c| c.tag_name().name() == "VisualElements");
            let entry = n.attribute("EntryPoint").unwrap_or("");
            App {
                id: n.attribute("Id").unwrap_or("App").to_string(),
                executable: n.attribute("Executable").unwrap_or("").to_string(),
                display_name: plain(visual.and_then(|v| v.attribute("DisplayName"))),
                logo: visual
                    .and_then(|v| {
                        v.attribute("Square44x44Logo")
                            .or(v.attribute("Square150x150Logo"))
                    })
                    .map(str::to_string),
                desktop: entry.is_empty() || entry == "Windows.FullTrustApplication",
            }
        })
        .collect();
    let dependencies = doc
        .descendants()
        .filter(|n| n.tag_name().name() == "PackageDependency")
        .filter_map(|n| n.attribute("Name").map(str::to_string))
        .collect();
    Ok(Manifest {
        display_name: plain(child_text(properties, "DisplayName").as_deref()),
        logo: child_text(properties, "Logo"),
        identity,
        apps,
        dependencies,
    })
}

/// Where a top-level `VFS\<folder>` goes inside the prefix's C: drive.
fn vfs_target(folder: &str, user: &str) -> Option<String> {
    let user_dir = format!(r"C:\users\{user}");
    Some(match folder {
        "ProgramFilesX64" => r"C:\Program Files".into(),
        "ProgramFilesX86" => r"C:\Program Files (x86)".into(),
        "ProgramFilesCommonX64" => r"C:\Program Files\Common Files".into(),
        "ProgramFilesCommonX86" => r"C:\Program Files (x86)\Common Files".into(),
        "SystemX64" | "System" => r"C:\windows\system32".into(),
        "SystemX86" => r"C:\windows\syswow64".into(),
        "Windows" => r"C:\windows".into(),
        "Fonts" => r"C:\windows\Fonts".into(),
        "Common AppData" => r"C:\ProgramData".into(),
        "AppData" => format!(r"{user_dir}\AppData\Roaming"),
        "LocalAppData" => format!(r"{user_dir}\AppData\Local"),
        "Personal" => format!(r"{user_dir}\Documents"),
        _ => return None,
    })
}

/// The Windows user inside the prefix (the one that is not Public).
fn prefix_user(prefix: &Path) -> String {
    std::fs::read_dir(prefix.join("drive_c/users"))
        .ok()
        .and_then(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .find(|n| n != "Public")
        })
        .unwrap_or_else(|| "user".into())
}

/// Zip entry names in packages are URL-encoded ("My%20App/app.exe").
fn decode_name(name: &str) -> String {
    percent_encoding::percent_decode_str(name)
        .decode_utf8_lossy()
        .into_owned()
}

fn encode_name(name: &str) -> String {
    name.replace(' ', "%20")
}

/// A folder name without characters Windows or macOS dislike.
fn folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if r#"<>:"/\|?*"#.contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() {
        "App".into()
    } else {
        trimmed
    }
}

/// Plain relative parts of a package path, or `None` if it escapes.
fn safe_parts(path: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    for c in Path::new(&path.replace('\\', "/")).components() {
        match c {
            Component::Normal(p) => parts.push(p.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then_some(parts)
}

fn install_package(
    zip: &mut zip::ZipArchive<File>,
    prefix: &Path,
    bottle_dir: &Path,
    log: &mut File,
) -> Result<Installed> {
    let manifest = parse_manifest(&read_text(zip, MANIFEST)?)?;
    let name = manifest.display_name.clone().unwrap_or_else(|| {
        // "Contoso.MyApp" → "MyApp"
        manifest
            .identity
            .rsplit('.')
            .next()
            .unwrap_or("App")
            .to_string()
    });
    let user = prefix_user(prefix);
    let install_dir = format!(r"C:\Program Files\{}", folder_name(&name));
    let _ = writeln!(log, "Unpacking {name} to {install_dir}");

    let mut skipped_vfs = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| Error::BadArchive(format!("cannot read the package: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let raw = decode_name(&entry.name().map(|n| n.into_owned()).unwrap_or_default());
        if SKIP.contains(&raw.as_str()) || raw.starts_with("AppxMetadata/") {
            continue;
        }
        let Some(parts) = safe_parts(&raw) else {
            return Err(Error::BadArchive(format!(
                "unsafe path in the package: {raw}"
            )));
        };
        let windows = if parts[0] == "VFS" && parts.len() > 2 {
            match vfs_target(&parts[1], &user) {
                Some(root) => format!(r"{root}\{}", parts[2..].join(r"\")),
                None => {
                    if !skipped_vfs.contains(&parts[1]) {
                        skipped_vfs.push(parts[1].clone());
                    }
                    continue;
                }
            }
        } else {
            format!(r"{install_dir}\{}", parts.join(r"\"))
        };
        let target = winpath::to_unix_for_write(prefix, &windows)
            .ok_or_else(|| Error::BadArchive(format!("cannot place {raw}")))?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).at(parent)?;
        }
        let mut out = File::create(&target).at(&target)?;
        std::io::copy(&mut entry, &mut out).at(&target)?;
    }

    let mut warnings = Vec::new();
    if !skipped_vfs.is_empty() {
        warnings.push(format!(
            "files for {} were left out (Vinter does not know where they go)",
            skipped_vfs.join(", ")
        ));
    }
    let frameworks: Vec<_> = manifest
        .dependencies
        .iter()
        .filter(|d| !d.is_empty())
        .cloned()
        .collect();
    if !frameworks.is_empty() {
        warnings.push(format!(
            "it needs {}, which Vinter cannot install from Microsoft; if it does not \
             start, install those packages' MSIX files into this bottle too",
            frameworks.join(", ")
        ));
    }

    let mut programs = Vec::new();
    for app in &manifest.apps {
        if app.executable.is_empty() {
            continue;
        }
        if !app.desktop {
            warnings.push(format!(
                "{} is a Windows Store-style (UWP) app, which usually does not run in Wine",
                app.display_name.as_deref().unwrap_or(&app.id)
            ));
        }
        let exe_parts = safe_parts(&app.executable)
            .ok_or_else(|| Error::BadArchive(format!("unsafe program path {}", app.executable)))?;
        let exe = if exe_parts[0] == "VFS" && exe_parts.len() > 2 {
            match vfs_target(&exe_parts[1], &user) {
                Some(root) => format!(r"{root}\{}", exe_parts[2..].join(r"\")),
                None => continue,
            }
        } else {
            format!(r"{install_dir}\{}", exe_parts.join(r"\"))
        };
        let logo = app.logo.as_deref().or(manifest.logo.as_deref());
        let icon = logo
            .and_then(|l| find_logo(prefix, &install_dir, l))
            .and_then(|png| store_icon(bottle_dir, &png).ok());
        // The first app without its own name takes the package's name.
        let program_name = match &app.display_name {
            Some(n) => n.clone(),
            None if !programs.iter().any(|p: &ProgramDef| p.name == name) => name.clone(),
            None => format!("{name} ({})", app.id),
        };
        programs.push(ProgramDef {
            name: program_name,
            exe,
            args: vec![],
            icon,
            pinned: true,
        });
    }
    if programs.is_empty() {
        return Err(Error::Unsupported(format!(
            "{name} has no program Vinter can start (it may be a framework or a resource package)"
        )));
    }
    Ok(Installed {
        name,
        programs,
        warnings,
    })
}

/// The best PNG for a manifest logo path. Packages usually only ship scaled
/// variants ("Logo.scale-200.png", "Logo.targetsize-48.png"); take the largest.
fn find_logo(prefix: &Path, install_dir: &str, logo: &str) -> Option<Vec<u8>> {
    let parts = safe_parts(logo)?;
    let windows = format!(r"{install_dir}\{}", parts.join(r"\"));
    let exact = winpath::to_unix(prefix, &windows);
    let base = winpath::to_unix_for_write(prefix, &windows)?;
    let folder = base.parent()?.to_path_buf();
    let stem = base.file_stem()?.to_string_lossy().to_lowercase();
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&folder)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            n.ends_with(".png")
                && (n == format!("{stem}.png") || n.starts_with(&format!("{stem}.")))
        })
        .collect();
    if let Some(e) = exact {
        candidates.push(e);
    }
    candidates
        .sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)));
    candidates
        .iter()
        .find_map(|p| icons::png_from_file(p).ok().flatten())
}

/// Saves a PNG as a bottle icon and returns its `icons/...` path.
fn store_icon(bottle_dir: &Path, png: &[u8]) -> Result<String> {
    let name = format!("{:016x}.png", fnv1a(png));
    let dir = bottle_dir.join(ICONS_DIR);
    std::fs::create_dir_all(&dir).at(&dir)?;
    let path = dir.join(&name);
    std::fs::write(&path, png).at(&path)?;
    Ok(format!("{ICONS_DIR}/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10">
  <Identity Name="Contoso.WorkApp" Publisher="CN=Contoso" Version="1.2.3.0" ProcessorArchitecture="x64"/>
  <Properties>
    <DisplayName>Work App</DisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Dependencies>
    <PackageDependency Name="Microsoft.VCLibs.140.00.UWPDesktop" MinVersion="14.0.0.0"/>
  </Dependencies>
  <Applications>
    <Application Id="App" Executable="Work App\work.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="ms-resource:AppName" Square44x44Logo="Assets\Square44x44Logo.png"/>
    </Application>
    <Application Id="Tool" Executable="VFS\ProgramFilesX64\Contoso\tool.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="Work Tool"/>
    </Application>
  </Applications>
</Package>"#;

    #[test]
    fn reads_manifests_and_bundles() {
        let m = parse_manifest(MANIFEST_XML).unwrap();
        assert_eq!(m.display_name.as_deref(), Some("Work App"));
        assert_eq!(m.apps.len(), 2);
        assert_eq!(
            m.apps[0].display_name, None,
            "ms-resource names are not shown"
        );
        assert_eq!(m.apps[1].display_name.as_deref(), Some("Work Tool"));
        assert!(m.apps.iter().all(|a| a.desktop));
        assert_eq!(m.dependencies, ["Microsoft.VCLibs.140.00.UWPDesktop"]);

        let bundle = r#"<Bundle><Packages>
            <Package Type="resource" FileName="lang.msix"/>
            <Package Type="application" Architecture="arm64" FileName="a_arm64.msix"/>
            <Package Type="application" Architecture="x64" FileName="a_x64.msix"/>
            <Package Type="application" Architecture="x86" FileName="a_x86.msix"/>
        </Packages></Bundle>"#;
        assert_eq!(pick_from_bundle(bundle).unwrap(), "a_x64.msix");
        let arm_only = r#"<Bundle><Packages><Package Type="application" Architecture="arm64" FileName="a.msix"/></Packages></Bundle>"#;
        assert!(pick_from_bundle(arm_only).is_err());
    }

    fn make_package(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in files {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    fn fake_bottle(tmp: &Path) -> (AppPaths, String) {
        let paths = AppPaths::with_data_dir(tmp.join("data"));
        let dir = bottle::dir(&paths, "b").unwrap();
        let c = dir.join("prefix/drive_c");
        std::fs::create_dir_all(c.join("users/me")).unwrap();
        std::fs::create_dir_all(c.join("users/Public")).unwrap();
        crate::bottle::Bottle {
            id: "b".into(),
            name: "B".into(),
            runtime: "rt".into(),
            created_at: "2026-10-19T00:00:00Z".into(),
            windows_version: crate::bottle::WindowsVersion::Win10,
            graphics: crate::bottle::Graphics::Wined3d,
            recipe: None,
            env: Default::default(),
            dll_overrides: Default::default(),
            options: Default::default(),
            programs: vec![],
        }
        .save(&dir)
        .unwrap();
        // c: is a link to drive_c in a real prefix.
        std::fs::create_dir_all(dir.join("prefix/dosdevices")).unwrap();
        std::os::unix::fs::symlink("../drive_c", dir.join("prefix/dosdevices/c:")).unwrap();
        (paths, "b".into())
    }

    #[test]
    fn installs_a_package_with_vfs_files_and_icons() {
        let tmp = tempfile::tempdir().unwrap();
        let (paths, id) = fake_bottle(tmp.path());
        let mut png = Vec::new();
        image::RgbaImage::new(64, 64)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let pkg = tmp.path().join("work.msix");
        make_package(
            &pkg,
            &[
                ("AppxManifest.xml", MANIFEST_XML.as_bytes()),
                ("AppxBlockMap.xml", b"x"),
                ("[Content_Types].xml", b"x"),
                ("Work%20App/work.exe", b"MZ"),
                ("Assets/Square44x44Logo.scale-200.png", &png),
                ("VFS/ProgramFilesX64/Contoso/tool.exe", b"MZ"),
                ("VFS/SystemX64/helper.dll", b"MZ"),
                ("VFS/Mystery/x.dat", b"?"),
            ],
        );
        let done = install(&paths, &id, &pkg).unwrap();
        assert_eq!(done.name, "Work App");
        assert_eq!(done.programs.len(), 2);
        assert_eq!(done.programs[0].name, "Work App");
        assert_eq!(
            done.programs[0].exe,
            r"C:\Program Files\Work App\Work App\work.exe"
        );
        assert!(
            done.programs[0].icon.is_some(),
            "scaled logo variant is used"
        );
        assert_eq!(done.programs[1].exe, r"C:\Program Files\Contoso\tool.exe");
        assert!(done.warnings.iter().any(|w| w.contains("VCLibs")));
        assert!(done.warnings.iter().any(|w| w.contains("Mystery")));

        let c = bottle::dir(&paths, &id).unwrap().join("prefix/drive_c");
        assert!(c.join("Program Files/Work App/Work App/work.exe").is_file());
        assert!(c.join("Program Files/Contoso/tool.exe").is_file());
        assert!(c.join("windows/system32/helper.dll").is_file());
        assert!(!c.join("Program Files/Work App/AppxBlockMap.xml").exists());
        let saved = bottle::get(&paths, &id).unwrap();
        assert_eq!(saved.programs.len(), 2);
        assert!(saved.programs.iter().all(|p| p.pinned));
    }

    #[test]
    fn refuses_unsafe_and_encrypted_packages() {
        let tmp = tempfile::tempdir().unwrap();
        let (paths, id) = fake_bottle(tmp.path());
        let evil = tmp.path().join("evil.msix");
        make_package(
            &evil,
            &[
                ("AppxManifest.xml", MANIFEST_XML.as_bytes()),
                ("../../escape.txt", b"x"),
            ],
        );
        assert!(install(&paths, &id, &evil).is_err());
        assert!(!tmp.path().join("data/escape.txt").exists());

        let store = tmp.path().join("app.eappx");
        std::fs::write(&store, b"EXTXjunk").unwrap();
        let err = install(&paths, &id, &store).unwrap_err().to_string();
        assert!(err.contains("encrypted Microsoft Store package"), "{err}");
        assert!(is_package(Path::new("x.MsixBundle")) && !is_package(Path::new("x.exe")));
    }
}
