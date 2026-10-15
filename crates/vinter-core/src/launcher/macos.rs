//! macOS launchers: minimal `.app` bundles that open a `vinter://` link.

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
    std::fs::create_dir_all(&macos).at(&macos)?;
    std::fs::create_dir_all(&resources).at(&resources)?;

    let url = deep_link(&bottle.id, index);
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

    let id = short_hash(&format!("{}/{index}", bottle.id));
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleName</key>
	<string>{name}</string>
	<key>CFBundleDisplayName</key>
	<string>{name}</string>
	<key>CFBundleIdentifier</key>
	<string>com.praevit.vinter.launcher.{id}</string>
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
	<key>VinterProgramIndex</key>
	<integer>{index}</integer>
	<key>VinterURL</key>
	<string>{url}</string>
</dict>
</plist>
"#,
        name = xml_escape(name),
        bottle_id = xml_escape(&bottle.id),
        url = xml_escape(&url),
    );
    let info = contents.join("Info.plist");
    std::fs::write(&info, plist).at(&info)?;
    let icon = resources.join("icon.icns");
    std::fs::write(&icon, icns(icon_png)).at(&icon)?;
    Ok(app)
}

/// Launchers in `dir` that belong to `bottle_id`.
pub fn find_for_bottle(dir: &Path, bottle_id: &str) -> Vec<PathBuf> {
    let needle = format!(
        "<key>{BOTTLE_KEY}</key>\n\t<string>{}</string>",
        xml_escape(bottle_id)
    );
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .filter(|p| {
            std::fs::read_to_string(p.join("Contents/Info.plist"))
                .is_ok_and(|text| text.contains(&needle))
        })
        .collect()
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
