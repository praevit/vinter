//! macOS launchers: minimal `.app` bundles that open a `vinter://` link.
//!
//! A launcher is normally a shell script (`exec open vinter://run/…`). Finder
//! hands documents to an app as an Apple Event, which a script never sees,
//! so launchers for programs with file types are AppleScript droplets made
//! with the system's `osacompile` instead: their `on open` handler adds each
//! file to the link (`?file=<percent-encoded path>`). If `osacompile` fails,
//! the launcher falls back to the script, without document types.

use std::path::{Path, PathBuf};

use super::{deep_link, short_hash};
use crate::Result;
use crate::bottle::Bottle;
use crate::error::IoResultExt;

/// Key in the launcher's Info.plist that names its bottle.
const BOTTLE_KEY: &str = "VinterBottleID";

/// Writes `<dir>/<name>.app` for program `index` of `bottle`.
pub fn create(
    dir: &Path,
    name: &str,
    bottle: &Bottle,
    index: usize,
    icon_png: &[u8],
) -> Result<PathBuf> {
    let program = &bottle.programs[index];
    let app = dir.join(format!("{name}.app"));
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    std::fs::create_dir_all(dir).at(dir)?;

    let url = deep_link(&bottle.id, &program.id);
    let id = short_hash(&format!("{}/{}", bottle.id, program.id));
    let keys = PlistKeys {
        name,
        bundle_id: &format!("com.praevit.vinter.launcher.{id}"),
        bottle_id: &bottle.id,
        program_id: &program.id,
        url: &url,
        file_types: &program.file_types,
    };

    if !program.file_types.is_empty() && droplet(&app, &keys, icon_png).is_ok() {
        return Ok(app);
    }
    // A failed droplet may leave part of a bundle behind.
    let _ = std::fs::remove_dir_all(&app);

    std::fs::create_dir_all(&macos).at(&macos)?;
    std::fs::create_dir_all(&resources).at(&resources)?;
    let script = macos.join("launch");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n# Made by Vinter: starts \"{}\" in the bottle \"{}\".\nexec /usr/bin/open \"{url}\"\n",
            program.name.replace('"', "'"),
            bottle.name.replace('"', "'"),
        ),
    )
    .at(&script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).at(&script)?;
    }

    let info = contents.join("Info.plist");
    std::fs::write(&info, script_plist(&keys)).at(&info)?;
    let icon = resources.join("icon.icns");
    std::fs::write(&icon, icns(icon_png)).at(&icon)?;
    Ok(app)
}

/// What Vinter puts in a launcher's Info.plist.
struct PlistKeys<'a> {
    name: &'a str,
    bundle_id: &'a str,
    bottle_id: &'a str,
    program_id: &'a str,
    url: &'a str,
    file_types: &'a [String],
}

/// The Info.plist of a script launcher.
fn script_plist(k: &PlistKeys) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key>
	<string>{name}</string>
	<key>CFBundleDisplayName</key>
	<string>{name}</string>
	<key>CFBundleIdentifier</key>
	<string>{bundle_id}</string>
	<key>CFBundleExecutable</key>
	<string>launch</string>
	<key>CFBundleIconFile</key>
	<string>icon</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>1.0</string>
	<key>CFBundleVersion</key>
	<string>1</string>
	<key>LSUIElement</key>
	<true/>
	<key>{BOTTLE_KEY}</key>
	<string>{bottle_id}</string>
	<key>VinterProgramID</key>
	<string>{program_id}</string>
	<key>VinterURL</key>
	<string>{url}</string>
</dict>
</plist>
"#,
        name = xml_escape(k.name),
        bundle_id = xml_escape(k.bundle_id),
        bottle_id = xml_escape(k.bottle_id),
        program_id = xml_escape(k.program_id),
        url = xml_escape(k.url),
    )
}

/// `CFBundleDocumentTypes` for file extensions. The rank is "Alternate", so
/// the launcher shows in "Open With" without taking over the file type.
pub fn document_types(file_types: &[String]) -> String {
    let exts: String = file_types
        .iter()
        .map(|e| format!("<string>{}</string>", xml_escape(e)))
        .collect();
    format!(
        "<array><dict><key>CFBundleTypeName</key><string>Document</string>\
         <key>CFBundleTypeRole</key><string>Editor</string>\
         <key>LSHandlerRank</key><string>Alternate</string>\
         <key>CFBundleTypeExtensions</key><array>{exts}</array></dict></array>"
    )
}

/// The AppleScript of a droplet launcher: started on its own it opens the
/// link; given files, it adds each one (percent-encoded) to the link.
pub fn droplet_script(url: &str) -> String {
    let url = url.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        r#"use framework "Foundation"
use scripting additions

-- Made by Vinter.
property baseURL : "{url}"

on run
	open location baseURL
end run

on open theFiles
	set keep to current application's NSCharacterSet's characterSetWithCharactersInString:"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~/"
	set link to baseURL
	set sep to "?"
	repeat with f in theFiles
		set p to current application's NSString's stringWithString:(POSIX path of f)
		set link to link & sep & "file=" & ((p's stringByAddingPercentEncodingWithAllowedCharacters:keep) as text)
		set sep to "&"
	end repeat
	open location link
end open
"#
    )
}

/// Builds a droplet launcher with `osacompile`, then sets Vinter's keys,
/// document types and icon in it and signs it again (ad hoc), since
/// changing Info.plist breaks the signature `osacompile` made.
#[cfg(target_os = "macos")]
fn droplet(app: &Path, k: &PlistKeys, icon_png: &[u8]) -> Result<()> {
    use std::process::{Command, Stdio};
    let run = |cmd: &mut Command| -> Result<()> {
        let status = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .at(app)?;
        if status.success() {
            Ok(())
        } else {
            Err(crate::Error::Unsupported(format!(
                "could not make the launcher {}",
                app.display()
            )))
        }
    };
    let source = app.with_extension("applescript");
    std::fs::write(&source, droplet_script(k.url)).at(&source)?;
    let compiled = run(Command::new("/usr/bin/osacompile")
        .arg("-o")
        .arg(app)
        .arg(&source));
    let _ = std::fs::remove_file(&source);
    compiled?;

    let info = app.join("Contents/Info.plist");
    let set = |key: &str, kind: &str, value: &str| {
        run(Command::new("/usr/bin/plutil")
            .args(["-replace", key, kind, value])
            .arg(&info))
    };
    set("CFBundleName", "-string", k.name)?;
    set("CFBundleDisplayName", "-string", k.name)?;
    set("CFBundleIdentifier", "-string", k.bundle_id)?;
    set("CFBundleIconFile", "-string", "icon")?;
    set("LSUIElement", "-bool", "true")?;
    set(BOTTLE_KEY, "-string", k.bottle_id)?;
    set("VinterProgramID", "-string", k.program_id)?;
    set("VinterURL", "-string", k.url)?;
    set(
        "CFBundleDocumentTypes",
        "-xml",
        &document_types(k.file_types),
    )?;
    // The compiled asset catalog's icon would win over ours.
    let _ = run(Command::new("/usr/bin/plutil")
        .args(["-remove", "CFBundleIconName"])
        .arg(&info));
    let _ = std::fs::remove_file(app.join("Contents/Resources/Assets.car"));
    let icon = app.join("Contents/Resources/icon.icns");
    std::fs::write(&icon, icns(icon_png)).at(&icon)?;
    run(Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-"])
        .arg(app))?;
    // Best effort: tell Launch Services now, so "Open With" lists it right
    // away. Not from tests, whose launchers are deleted again.
    if cfg!(test) {
        return Ok(());
    }
    let _ = run(Command::new(
        "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",
    )
    .arg("-f")
    .arg(app));
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn droplet(_app: &Path, _k: &PlistKeys, _icon_png: &[u8]) -> Result<()> {
    Err(crate::Error::Unsupported(
        "droplet launchers need macOS".into(),
    ))
}

/// Launchers in `dir` that belong to `bottle_id`.
pub fn find_for_bottle(dir: &Path, bottle_id: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .filter(|p| {
            std::fs::read_to_string(p.join("Contents/Info.plist"))
                .is_ok_and(|text| plist_string(&text, BOTTLE_KEY).as_deref() == Some(bottle_id))
        })
        .collect()
}

/// Made before program ids: it has no `VinterProgramID`.
pub fn is_legacy(app: &Path) -> bool {
    std::fs::read_to_string(app.join("Contents/Info.plist"))
        .is_ok_and(|text| plist_string(&text, "VinterProgramID").is_none())
}

/// The string value of `key` in an XML plist (whitespace between them can
/// differ: Vinter writes some plists itself, `plutil` the others).
fn plist_string(text: &str, key: &str) -> Option<String> {
    let after = text.split_once(&format!("<key>{key}</key>"))?.1;
    let value = after.trim_start().strip_prefix("<string>")?;
    let value = value.split_once("</string>")?.0;
    Some(xml_unescape(value))
}

/// A minimal `.icns` holding one 128x128 PNG (`ic07`), which macOS scales.
pub fn icns(png128: &[u8]) -> Vec<u8> {
    let entry_len = 8 + png128.len();
    let total = 8 + entry_len;
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&(total as u32).to_be_bytes());
    out.extend_from_slice(b"ic07");
    out.extend_from_slice(&(entry_len as u32).to_be_bytes());
    out.extend_from_slice(png128);
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_values_with_any_whitespace() {
        let ours = "<key>VinterBottleID</key>\n\t<string>b&amp;1</string>";
        assert_eq!(plist_string(ours, BOTTLE_KEY).as_deref(), Some("b&1"));
        let plutil = "<key>VinterBottleID</key>\n    <string>b2</string>";
        assert_eq!(plist_string(plutil, BOTTLE_KEY).as_deref(), Some("b2"));
        assert_eq!(
            plist_string("<key>Other</key><string>x</string>", BOTTLE_KEY),
            None
        );
    }

    #[test]
    fn document_types_and_droplet_script() {
        let types = document_types(&["docx".into(), "txt".into()]);
        assert!(types.contains("<string>docx</string><string>txt</string>"));
        assert!(types.contains("<string>Alternate</string>"));
        let script = droplet_script("vinter://run/b1/abc");
        assert!(script.contains("property baseURL : \"vinter://run/b1/abc\""));
        assert!(script.contains("on open theFiles"));
    }
}
