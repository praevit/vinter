//! Games installed with Steam: found from Steam's own library files, so each
//! one can get a tile that starts it through Steam (`steam.exe -applaunch
//! <appid>`). Nothing is said or done about anti-cheat or DRM: a game runs
//! as well as it runs under Wine.

use std::path::{Path, PathBuf};

use super::winpath;

/// Steam's own redistributables "app", never a game.
const NOT_GAMES: &[&str] = &["228980"];

/// A value in Valve's KeyValues text format (`.vdf`, `.acf`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vdf {
    Str(String),
    Map(Vec<(String, Vdf)>),
}

impl Vdf {
    /// The value of `key` (matched ignoring case, as Steam does).
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Self::Map(entries) => entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Self::Str(_) => None,
        }
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Self::Str(s) => Some(s),
            Self::Map(_) => None,
        }
    }

    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Self::Map(entries) => entries,
            Self::Str(_) => &[],
        }
    }
}

/// Parses KeyValues text into a map of its top-level keys. Tolerant: stops
/// at the first thing it cannot read and keeps what came before; comments
/// (`//`) and conditions (`[$WIN32]`) are skipped.
pub fn parse(text: &str) -> Vdf {
    let mut tokens = Tokens {
        chars: text.chars().peekable(),
    };
    Vdf::Map(parse_map(&mut tokens))
}

fn parse_map(tokens: &mut Tokens) -> Vec<(String, Vdf)> {
    let mut entries = Vec::new();
    loop {
        let key = match tokens.next() {
            Some(Token::Str(s)) => s,
            Some(Token::Open) => continue,
            Some(Token::Close) | None => return entries,
        };
        match tokens.next() {
            Some(Token::Str(value)) => entries.push((key, Vdf::Str(value))),
            Some(Token::Open) => entries.push((key, Vdf::Map(parse_map(tokens)))),
            Some(Token::Close) | None => return entries,
        }
    }
}

enum Token {
    Str(String),
    Open,
    Close,
}

struct Tokens<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Iterator for Tokens<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        loop {
            let c = *self.chars.peek()?;
            match c {
                c if c.is_whitespace() => {
                    self.chars.next();
                }
                '/' => {
                    // A `//` comment runs to the end of the line.
                    for c in self.chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                '[' => {
                    for c in self.chars.by_ref() {
                        if c == ']' {
                            break;
                        }
                    }
                }
                '{' => {
                    self.chars.next();
                    return Some(Token::Open);
                }
                '}' => {
                    self.chars.next();
                    return Some(Token::Close);
                }
                '"' => {
                    self.chars.next();
                    let mut s = String::new();
                    while let Some(c) = self.chars.next() {
                        match c {
                            '"' => break,
                            '\\' => match self.chars.next() {
                                Some('n') => s.push('\n'),
                                Some('t') => s.push('\t'),
                                Some(other) => s.push(other),
                                None => break,
                            },
                            c => s.push(c),
                        }
                    }
                    return Some(Token::Str(s));
                }
                _ => {
                    let mut s = String::new();
                    while let Some(&c) = self.chars.peek() {
                        if c.is_whitespace() || matches!(c, '"' | '{' | '}') {
                            break;
                        }
                        s.push(c);
                        self.chars.next();
                    }
                    return Some(Token::Str(s));
                }
            }
        }
    }
}

/// A game installed with Steam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamGame {
    pub app_id: String,
    pub name: String,
    /// `steam.exe` in the bottle.
    pub steam_exe: PathBuf,
    /// A picture of the game from Steam's library cache, if it has one.
    pub image: Option<PathBuf>,
}

/// Steam's folder in a prefix, if Steam is installed there.
pub fn steam_dir(prefix: &Path) -> Option<PathBuf> {
    [
        r"C:\Program Files (x86)\Steam\steam.exe",
        r"C:\Program Files\Steam\steam.exe",
    ]
    .iter()
    .find_map(|exe| winpath::to_unix(prefix, exe))
    .and_then(|exe| exe.parent().map(Path::to_path_buf))
}

/// The installed games of the Steam in `prefix`, sorted by name.
pub fn games(prefix: &Path) -> Vec<SteamGame> {
    let Some(steam) = steam_dir(prefix) else {
        return Vec::new();
    };
    let steam_exe = steam.join("steam.exe");
    let mut games: Vec<SteamGame> = Vec::new();
    for library in libraries(prefix, &steam) {
        let Ok(entries) = std::fs::read_dir(&library) else {
            continue;
        };
        let mut manifests: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("appmanifest_") && n.ends_with(".acf"))
            })
            .collect();
        manifests.sort();
        for manifest in manifests {
            let Ok(text) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            let Some((app_id, name)) = installed_game(&parse(&text)) else {
                continue;
            };
            if games.iter().any(|g| g.app_id == app_id) {
                continue;
            }
            games.push(SteamGame {
                image: library_image(&steam, &app_id),
                app_id,
                name,
                steam_exe: steam_exe.clone(),
            });
        }
    }
    games.sort_by_key(|g| g.name.to_lowercase());
    games
}

/// App id and name from an `appmanifest_<id>.acf`, if it is a game that is
/// installed (not being downloaded for the first time).
fn installed_game(manifest: &Vdf) -> Option<(String, String)> {
    let state = manifest.get("AppState")?;
    let app_id = state.str("appid")?.trim().to_string();
    let name = state.str("name")?.trim().to_string();
    if app_id.is_empty()
        || !app_id.chars().all(|c| c.is_ascii_digit())
        || name.is_empty()
        || NOT_GAMES.contains(&app_id.as_str())
    {
        return None;
    }
    // Bit 4 is "fully installed".
    let flags = state.str("StateFlags").and_then(|f| f.parse::<u32>().ok());
    if flags.is_some_and(|f| f & 4 == 0) {
        return None;
    }
    Some((app_id, name))
}

/// Every `steamapps` folder: Steam's own, plus the libraries listed in
/// `libraryfolders.vdf` (on any of the bottle's drives).
fn libraries(prefix: &Path, steam: &Path) -> Vec<PathBuf> {
    let own = steam.join("steamapps");
    let mut found = vec![own.clone()];
    let Ok(text) = std::fs::read_to_string(own.join("libraryfolders.vdf")) else {
        return found;
    };
    let vdf = parse(&text);
    let Some(folders) = vdf
        .get("libraryfolders")
        .or_else(|| vdf.get("LibraryFolders"))
    else {
        return found;
    };
    for (key, value) in folders.entries() {
        if !key.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        // Newer files: `"1" { "path" "D:\\Games" … }`; older: `"1" "D:\\Games"`.
        let path = match value {
            Vdf::Str(s) => Some(s.as_str()),
            map => map.str("path"),
        };
        let Some(dir) = path.and_then(|p| winpath::windows_to_host(prefix, p)) else {
            continue;
        };
        let apps = dir.join("steamapps");
        if !found.contains(&apps) {
            found.push(apps);
        }
    }
    found
}

/// A picture of the game from Steam's library cache: its small icon if
/// there is one, otherwise its header. Older Steam keeps them as
/// `librarycache/<appid>_icon.jpg`; newer Steam in `librarycache/<appid>/`,
/// with the icon named after its hash.
fn library_image(steam: &Path, app_id: &str) -> Option<PathBuf> {
    let cache = steam.join("appcache").join("librarycache");
    let old_icon = cache.join(format!("{app_id}_icon.jpg"));
    if old_icon.is_file() {
        return Some(old_icon);
    }
    let folder = cache.join(app_id);
    if let Ok(entries) = std::fs::read_dir(&folder) {
        let mut hashed: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
                p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg"))
                    && stem.len() == 40
                    && stem.chars().all(|c| c.is_ascii_hexdigit())
            })
            .collect();
        hashed.sort();
        if let Some(icon) = hashed.into_iter().next() {
            return Some(icon);
        }
    }
    [
        folder.join("header.jpg"),
        cache.join(format!("{app_id}_header.jpg")),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keyvalues_tolerantly() {
        let text = r#"
// a comment
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps" { "228980" "1" "620" "2" }
	}
	"1"		"D:\\Old Library"
	"quoted \"name\""	value [$WIN32]
	"cut off"	{ "path" "E:\\x"
"#;
        let vdf = parse(text);
        let folders = vdf.get("LIBRARYFOLDERS").unwrap();
        assert_eq!(
            folders.get("0").unwrap().str("path"),
            Some(r"C:\Program Files (x86)\Steam")
        );
        assert_eq!(
            folders
                .get("0")
                .unwrap()
                .get("apps")
                .unwrap()
                .entries()
                .len(),
            2
        );
        assert_eq!(folders.str("1"), Some(r"D:\Old Library"));
        assert_eq!(folders.str("quoted \"name\""), Some("value"));
        // The unfinished entry keeps what was read.
        assert_eq!(folders.get("cut off").unwrap().str("path"), Some(r"E:\x"));
        assert_eq!(parse("").entries().len(), 0);
        assert_eq!(parse("}}}{").entries().len(), 0);
    }

    #[test]
    fn manifests() {
        let game =
            parse("\"AppState\" { \"appid\" \"620\" \"name\" \"Portal 2\" \"StateFlags\" \"4\" }");
        assert_eq!(
            installed_game(&game),
            Some(("620".into(), "Portal 2".into()))
        );
        let downloading = parse(
            "\"AppState\" { \"appid\" \"620\" \"name\" \"Portal 2\" \"StateFlags\" \"1026\" }",
        );
        assert_eq!(installed_game(&downloading), None);
        let redist = parse(
            "\"AppState\" { \"appid\" \"228980\" \"name\" \"Steamworks Common Redistributables\" }",
        );
        assert_eq!(installed_game(&redist), None);
    }

    #[cfg(unix)]
    #[test]
    fn finds_games_in_every_library() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        let steam = prefix.join("drive_c/Program Files (x86)/Steam");
        let other = tmp.path().join("Games");
        std::fs::create_dir_all(steam.join("steamapps")).unwrap();
        std::fs::create_dir_all(other.join("steamapps")).unwrap();
        std::fs::create_dir_all(prefix.join("dosdevices")).unwrap();
        std::os::unix::fs::symlink(&other, prefix.join("dosdevices/d:")).unwrap();
        std::fs::write(steam.join("steam.exe"), b"").unwrap();
        std::fs::write(
            steam.join("steamapps/libraryfolders.vdf"),
            "\"libraryfolders\" { \"0\" { \"path\" \"C:\\\\Program Files (x86)\\\\Steam\" } \"1\" { \"path\" \"D:\\\\\" } }",
        )
        .unwrap();
        let manifest = |dir: &Path, id: &str, name: &str| {
            std::fs::write(
                dir.join(format!("steamapps/appmanifest_{id}.acf")),
                format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"{id}\"\n\t\"name\"\t\t\"{name}\"\n\t\"StateFlags\"\t\t\"4\"\n}}\n"),
            )
            .unwrap();
        };
        manifest(&steam, "620", "Portal 2");
        manifest(&steam, "228980", "Steamworks Common Redistributables");
        manifest(&other, "400", "Portal");
        let cache = steam.join("appcache/librarycache/400");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("header.jpg"), b"").unwrap();

        let found = games(&prefix);
        let names: Vec<&str> = found.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Portal", "Portal 2"]);
        assert_eq!(found[0].app_id, "400");
        assert_eq!(found[0].image, Some(cache.join("header.jpg")));
        assert_eq!(found[1].image, None);
        assert_eq!(found[1].steam_exe, steam.join("steam.exe"));

        // No Steam, no games.
        assert!(games(&tmp.path().join("empty")).is_empty());
    }
}
