//! Per-app FEX settings (the `fex-config-editor` experimental flag).
//!
//! The options come from FEX's own schema for the FEX version a runtime
//! ships (`FEXCore/Source/Interface/Config/Config.json.in`, vendored under
//! `assets/fex/`, MIT, see `licenses/FEX-LICENSE.txt`); nothing here names
//! an option itself. FEX reads every option from an environment variable
//! `FEX_<NAME IN CAPITALS>` (`Source/Common/Config.cpp`), so a program's
//! settings are passed that way when it starts. Values are kept per bottle
//! in `fex-config.json`, by the program's Windows path.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::bottle::{self, Bottle};
use crate::error::IoResultExt;
use crate::experimental::{ExperimentalFlag, Flags};
use crate::runtime::Runtime;
use crate::{AppPaths, Error, Result};

const FILE: &str = "fex-config.json";

/// Schemas Vinter knows, by the `fex_version` runtimes declare.
const SCHEMAS: &[(&str, &str)] = &[(
    "FEX-2609",
    include_str!("../assets/fex/FEX-2609-Config.json.in"),
)];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum FexOptionKind {
    Bool,
    Integer,
    Text,
    /// A named-value list or string list; FEX converts these itself, so
    /// Vinter only shows them.
    ReadOnly,
}

/// One option from FEX's schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FexOption {
    pub group: String,
    pub name: String,
    /// The schema's own type name, e.g. "uint32".
    pub schema_type: String,
    pub kind: FexOptionKind,
    pub default: String,
    pub description: String,
    /// Environment variable FEX reads it from.
    pub env: String,
}

/// The options of FEX `version`, or `None` if Vinter has no schema for it.
pub fn schema(version: &str) -> Option<Vec<FexOption>> {
    let text = SCHEMAS.iter().find(|(v, _)| *v == version)?.1;
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut out = Vec::new();
    for (group, options) in json.get("Options")?.as_object()? {
        for (name, o) in options.as_object()? {
            let schema_type = o.get("Type")?.as_str()?.to_string();
            let kind = match schema_type.as_str() {
                "bool" => FexOptionKind::Bool,
                "uint8" | "int32" | "uint32" | "uint64" => FexOptionKind::Integer,
                "str" => FexOptionKind::Text,
                _ => FexOptionKind::ReadOnly,
            };
            let description = o
                .get("Desc")
                .and_then(|d| d.as_array())
                .map(|lines| {
                    lines
                        .iter()
                        .filter_map(|l| l.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            out.push(FexOption {
                group: group.clone(),
                env: format!("FEX_{}", name.to_ascii_uppercase()),
                name: name.clone(),
                schema_type,
                kind,
                default: o
                    .get("Default")
                    .and_then(|d| d.as_str())
                    .unwrap_or_default()
                    .to_string(),
                description,
            });
        }
    }
    Some(out)
}

/// The options for `rt`'s FEX, or why there are none.
pub fn options_for(rt: &Runtime) -> Result<Vec<FexOption>> {
    let version = rt
        .fex_version
        .as_deref()
        .ok_or_else(|| Error::Unsupported(format!("{} does not use FEX", rt.name)))?;
    schema(version).ok_or_else(|| {
        Error::Unsupported(format!(
            "Vinter does not know the settings of {version} yet"
        ))
    })
}

/// Checks a value against the option's schema type and returns it as FEX
/// parses it: booleans as `1`/`0` (FEX reads them as numbers), integers in
/// range, text without NUL.
pub fn normalize(option: &FexOption, value: &str) -> Result<String> {
    let value = value.trim();
    let bad = || {
        Error::InvalidSetting(format!(
            "{value:?} is not a valid {} for {}",
            option.schema_type, option.name
        ))
    };
    match option.kind {
        FexOptionKind::Bool => match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "on" | "yes" => Ok("1".into()),
            "0" | "false" | "off" | "no" => Ok("0".into()),
            _ => Err(bad()),
        },
        FexOptionKind::Integer => {
            let ok = match option.schema_type.as_str() {
                "uint8" => value.parse::<u8>().is_ok(),
                "int32" => value.parse::<i32>().is_ok(),
                "uint32" => value.parse::<u32>().is_ok(),
                _ => value.parse::<u64>().is_ok(),
            };
            ok.then(|| value.to_string()).ok_or_else(bad)
        }
        FexOptionKind::Text if !value.contains('\0') => Ok(value.to_string()),
        FexOptionKind::Text => Err(bad()),
        FexOptionKind::ReadOnly => Err(Error::InvalidSetting(format!(
            "{} cannot be changed from Vinter",
            option.name
        ))),
    }
}

fn ensure_enabled(paths: &AppPaths) -> Result<()> {
    if Flags::load(paths).is_enabled(ExperimentalFlag::FexConfigEditor) {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "FEX settings are experimental; turn on {} first",
            ExperimentalFlag::FexConfigEditor
        )))
    }
}

type Store = BTreeMap<String, BTreeMap<String, String>>;

fn store_file(paths: &AppPaths, bottle_id: &str) -> Result<PathBuf> {
    Ok(bottle::dir(paths, bottle_id)?.join(FILE))
}

fn read(paths: &AppPaths, bottle_id: &str) -> Result<Store> {
    let file = store_file(paths, bottle_id)?;
    match std::fs::read_to_string(&file) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| Error::Config {
            path: file,
            message: e.to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::new()),
        Err(e) => Err(e).at(&file),
    }
}

/// A program's own FEX settings (option name → value as FEX reads it).
pub fn program_settings(
    paths: &AppPaths,
    bottle_id: &str,
    index: usize,
) -> Result<BTreeMap<String, String>> {
    let b = bottle::get(paths, bottle_id)?;
    let program = b.programs.get(index).ok_or(Error::ProgramNotFound(index))?;
    Ok(read(paths, bottle_id)?
        .remove(&program.exe)
        .unwrap_or_default())
}

/// Sets (or with `None`, removes) one FEX option for a program. The bottle
/// must be on a FEX runtime whose schema has the option.
pub fn set(
    paths: &AppPaths,
    bottle_id: &str,
    index: usize,
    name: &str,
    value: Option<&str>,
) -> Result<BTreeMap<String, String>> {
    ensure_enabled(paths)?;
    let b = bottle::get(paths, bottle_id)?;
    let program = b.programs.get(index).ok_or(Error::ProgramNotFound(index))?;
    let rt = crate::runtime::get(paths, &b.runtime)?;
    let options = options_for(&rt)?;
    let option = options
        .iter()
        .find(|o| o.name == name)
        .ok_or_else(|| Error::InvalidSetting(format!("FEX has no setting called {name}")))?;
    let mut all = read(paths, bottle_id)?;
    let settings = all.entry(program.exe.clone()).or_default();
    let before = settings.get(&option.name).cloned();
    match value {
        Some(v) => {
            settings.insert(option.name.clone(), normalize(option, v)?);
        }
        None => {
            settings.remove(&option.name);
        }
    }
    let result = settings.clone();
    all.retain(|_, s| !s.is_empty());
    let file = store_file(paths, bottle_id)?;
    let tmp = file.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&all).expect("serializes"),
    )
    .at(&tmp)?;
    std::fs::rename(&tmp, &file).at(&file)?;
    bottle::history::note(
        paths,
        bottle_id,
        bottle::history::Change::FexSetting {
            program: program.exe.clone(),
            name: option.name.clone(),
            before,
            after: result.get(&option.name).cloned(),
        },
    );
    Ok(result)
}

/// The `FEX_…` variables a program's launch gets: only with the flag on,
/// on a FEX runtime Vinter has the schema for, and only options still in it.
pub fn launch_env(
    paths: &AppPaths,
    bottle: &Bottle,
    rt: &Runtime,
    exe: &str,
) -> Vec<(String, String)> {
    if !crate::engine::is_fex(rt.engine) || ensure_enabled(paths).is_err() {
        return Vec::new();
    }
    let (Ok(options), Ok(mut all)) = (options_for(rt), read(paths, &bottle.id)) else {
        return Vec::new();
    };
    let Some(settings) = all.remove(exe) else {
        return Vec::new();
    };
    settings
        .into_iter()
        .filter_map(|(name, value)| {
            let option = options.iter().find(|o| o.name == name)?;
            Some((option.env.clone(), normalize(option, &value).ok()?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_fex_2609s_own_schema() {
        let options = schema("FEX-2609").unwrap();
        assert_eq!(options.len(), 64);
        let multiblock = options.iter().find(|o| o.name == "Multiblock").unwrap();
        assert_eq!(multiblock.group, "CPU");
        assert_eq!(multiblock.kind, FexOptionKind::Bool);
        assert_eq!(multiblock.env, "FEX_MULTIBLOCK");
        assert_eq!(multiblock.default, "true");
        assert!(multiblock.description.contains("multiblock"));
        assert!(options.iter().any(|o| o.kind == FexOptionKind::ReadOnly));
        assert!(schema("FEX-0000").is_none());
    }

    #[test]
    fn values_are_checked_and_written_as_fex_reads_them() {
        let options = schema("FEX-2609").unwrap();
        let by = |name: &str| options.iter().find(|o| o.name == name).unwrap().clone();
        let multiblock = by("Multiblock");
        assert_eq!(normalize(&multiblock, "false").unwrap(), "0");
        assert_eq!(normalize(&multiblock, "On").unwrap(), "1");
        assert!(normalize(&multiblock, "maybe").is_err());
        let max_inst = by("MaxInst");
        assert_eq!(normalize(&max_inst, " 200 ").unwrap(), "200");
        assert!(normalize(&max_inst, "lots").is_err());
        let readonly = options
            .iter()
            .find(|o| o.kind == FexOptionKind::ReadOnly)
            .unwrap();
        assert!(normalize(readonly, "x").is_err());
    }

    #[test]
    fn settings_reach_the_launch_only_with_the_flag_on_fex() {
        use crate::experimental;
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::with_data_dir(tmp.path());
        let rt_dir = paths.runtimes_dir().join("arm");
        std::fs::create_dir_all(rt_dir.join("bin")).unwrap();
        std::fs::write(
            rt_dir.join("runtime.toml"),
            "id = \"arm\"\nname = \"Arm\"\nversion = \"11.10\"\nflavor = \"devel\"\nurl = \"\"\n\
             sha256 = \"\"\ninstalled_at = \"2026-10-23T00:00:00Z\"\nwine_bin = \"bin/wine\"\n\
             license = \"\"\nsource_url = \"\"\narch = \"aarch64\"\ngraphics = [\"wined3d\"]\n\
             engine = \"arm64-fex\"\nfex_version = \"FEX-2609\"\n",
        )
        .unwrap();
        let dir = bottle::dir(&paths, "b").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let mut b: Bottle = toml::from_str(
            "id = \"b\"\nname = \"B\"\nruntime = \"arm\"\nengine = \"arm64-fex\"\ncreated_at = \"2026-10-23T00:00:00Z\"\n",
        )
        .unwrap();
        b.programs.push(crate::bottle::ProgramDef {
            name: "Game".into(),
            exe: r"C:\Game\game.exe".into(),
            args: vec![],
            icon: None,
            pinned: false,
            arm64_build: None,
            ..Default::default()
        });
        b.save(&dir).unwrap();
        let rt = crate::runtime::get(&paths, "arm").unwrap();

        assert!(
            set(&paths, "b", 0, "Multiblock", Some("false")).is_err(),
            "flag off"
        );
        experimental::set(&paths, ExperimentalFlag::FexConfigEditor, true).unwrap();
        set(&paths, "b", 0, "Multiblock", Some("false")).unwrap();
        set(&paths, "b", 0, "MaxInst", Some("500")).unwrap();
        assert!(set(&paths, "b", 0, "NoSuchOption", Some("1")).is_err());
        assert_eq!(
            launch_env(&paths, &b, &rt, r"C:\Game\game.exe"),
            [
                ("FEX_MAXINST".to_string(), "500".to_string()),
                ("FEX_MULTIBLOCK".to_string(), "0".to_string())
            ]
        );
        set(&paths, "b", 0, "MaxInst", None).unwrap();
        assert_eq!(program_settings(&paths, "b", 0).unwrap().len(), 1);

        experimental::set(&paths, ExperimentalFlag::FexConfigEditor, false).unwrap();
        assert!(launch_env(&paths, &b, &rt, r"C:\Game\game.exe").is_empty());
    }
}
