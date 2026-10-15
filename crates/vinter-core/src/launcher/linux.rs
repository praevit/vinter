//! Linux launchers: `.desktop` files (freedesktop.org Desktop Entry spec).

use std::path::PathBuf;

use super::Target;
use crate::Result;
use crate::bottle::Bottle;
use crate::error::IoResultExt;

fn file_name(bottle_id: &str, index: usize) -> String {
    format!("vinter-{bottle_id}-{index}")
}

/// Writes `vinter-<bottle>-<index>.desktop` and its icon.
pub fn create(target: &Target, bottle: &Bottle, index: usize, icon_png: &[u8]) -> Result<PathBuf> {
    let program = &bottle.programs[index];
    std::fs::create_dir_all(&target.dir).at(&target.dir)?;
    std::fs::create_dir_all(&target.icons_dir).at(&target.icons_dir)?;
    let base = file_name(&bottle.id, index);
    let icon = target.icons_dir.join(format!("{base}.png"));
    std::fs::write(&icon, icon_png).at(&icon)?;

    let mut exec: Vec<String> = target.command.clone();
    exec.extend(["--run".to_string(), bottle.id.clone(), index.to_string()]);
    let exec = exec.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nComment=Windows program in the Vinter bottle {bottle}\nExec={exec}\nIcon={icon}\nTerminal=false\nCategories=Wine;\nStartupNotify=true\nX-Vinter-Bottle={id}\n",
        name = single_line(&program.name),
        bottle = single_line(&bottle.name),
        icon = icon.display(),
        id = bottle.id,
    );
    let path = target.dir.join(format!("{base}.desktop"));
    std::fs::write(&path, entry).at(&path)?;
    Ok(path)
}

pub fn find_for_bottle(target: &Target, bottle_id: &str) -> Vec<PathBuf> {
    let prefix = format!("vinter-{bottle_id}-");
    let Ok(entries) = std::fs::read_dir(&target.dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".desktop"))
        })
        .collect()
}

pub fn remove_icons(target: &Target, bottle_id: &str) -> Result<()> {
    let prefix = format!("vinter-{bottle_id}-");
    if let Ok(entries) = std::fs::read_dir(&target.icons_dir) {
        for path in entries.flatten().map(|e| e.path()) {
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix))
            {
                std::fs::remove_file(&path).at(&path)?;
            }
        }
    }
    Ok(())
}

/// Quotes an `Exec` argument as the Desktop Entry spec requires.
fn quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_' | '+' | ',' | '=')
        });
    if plain {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        if c == '%' {
            out.push('%');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn single_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn exec_quoting() {
        assert_eq!(quote("/usr/bin/vinter"), "/usr/bin/vinter");
        assert_eq!(quote("/opt/My Apps/vinter"), "\"/opt/My Apps/vinter\"");
        assert_eq!(quote("a\"b$c"), "\"a\\\"b\\$c\"");
        assert_eq!(quote("100%"), "\"100%%\"");
    }
}
