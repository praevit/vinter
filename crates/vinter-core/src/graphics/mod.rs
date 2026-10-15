//! Graphics backends: how Direct3D calls get translated (plan section 4.3).
//!
//! Each backend knows which DLLs it puts in a prefix (via a downloadable
//! [`component`]), which overrides and environment variables a launch
//! needs, and where it can run.

pub mod apply;
pub mod component;
pub mod probe;

use serde::Serialize;

pub use crate::bottle::Graphics as Backend;
use crate::runtime::Runtime;

pub use apply::apply;
pub use component::{Component, ComponentInfo};
pub use probe::{GraphicsReport, test};

/// What the UI shows for one backend in a bottle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BackendInfo {
    pub backend: Backend,
    pub name: String,
    pub description: String,
    /// Usable with this bottle's runtime on this machine.
    pub available: bool,
    /// Why not, when `available` is false.
    pub reason: Option<String>,
    /// The download it needs, if any.
    pub component: Option<ComponentInfo>,
    /// That download is already installed.
    pub component_installed: bool,
}

impl Backend {
    pub const ALL: [Self; 4] = [Self::Wined3d, Self::Dxvk, Self::Dxmt, Self::D3dmetal];

    pub fn id(self) -> &'static str {
        match self {
            Self::Wined3d => "wined3d",
            Self::Dxvk => "dxvk",
            Self::Dxmt => "dxmt",
            Self::D3dmetal => "d3dmetal",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.id() == s)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Wined3d => "Wine (wined3d)",
            Self::Dxvk => "DXVK",
            Self::Dxmt => "DXMT",
            Self::D3dmetal => "D3DMetal",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Wined3d => {
                "Wine's built-in Direct3D over OpenGL. Works everywhere; slowest, and limited to older Direct3D on macOS."
            }
            Self::Dxvk => {
                "Direct3D 10/11 over Vulkan (MoltenVK on macOS). Usually much faster than wined3d."
            }
            Self::Dxmt => "Direct3D 10/11 over Metal, made for macOS.",
            Self::D3dmetal => "Apple's Direct3D 11/12 over Metal, from the Game Porting Toolkit.",
        }
    }

    /// Operating systems the backend exists for.
    fn supports_os(self, os: &str) -> bool {
        match self {
            Self::Wined3d | Self::Dxvk => matches!(os, "macos" | "linux"),
            Self::Dxmt | Self::D3dmetal => os == "macos",
        }
    }
}

/// Every backend with its availability for `runtime` on this machine.
pub fn backends(paths: &crate::AppPaths, runtime: &Runtime) -> Vec<BackendInfo> {
    backends_for(paths, runtime, std::env::consts::OS)
}

pub(crate) fn backends_for(
    paths: &crate::AppPaths,
    runtime: &Runtime,
    os: &str,
) -> Vec<BackendInfo> {
    Backend::ALL
        .into_iter()
        .map(|backend| {
            let component = component::for_backend(backend, os);
            let reason = unavailable_reason(backend, runtime, os, component.is_some());
            BackendInfo {
                backend,
                name: backend.name().into(),
                description: backend.description().into(),
                available: reason.is_none(),
                reason,
                component_installed: component
                    .as_ref()
                    .is_some_and(|c| component::installed(paths, &c.id).is_some()),
                component,
            }
        })
        .collect()
}

fn unavailable_reason(
    backend: Backend,
    runtime: &Runtime,
    os: &str,
    has_component: bool,
) -> Option<String> {
    if !backend.supports_os(os) {
        return Some(format!("{} is only available on macOS.", backend.name()));
    }
    match backend {
        Backend::Wined3d => None,
        Backend::D3dmetal => Some(
            "Needs Apple's Game Porting Toolkit, which Vinter is not allowed to include, \
             and a Wine build made for it. Support for a toolkit you install yourself \
             is planned."
                .into(),
        ),
        // Tested with DXMT 0.80 on Gcenx Wine 11.0: the device works, but drawing
        // to a window needs Mac driver functions these Wine builds keep hidden.
        Backend::Dxmt if !runtime.graphics.contains(&backend) => Some(format!(
            "DXMT needs a Wine build that shares its macOS window functions, and {} \
             does not. A Vinter Wine build with DXMT is planned.",
            runtime.name
        )),
        _ if !runtime.graphics.contains(&backend) => Some(format!(
            "{} does not support {} yet.",
            runtime.name,
            backend.name()
        )),
        _ if !has_component => Some(format!(
            "No {} download is available for this system yet.",
            backend.name()
        )),
        _ => None,
    }
}

/// DLL overrides a launch needs for `backend` (before the bottle's own).
pub fn launch_overrides(backend: Backend) -> Vec<(String, String)> {
    launch_overrides_for(backend, std::env::consts::OS)
}

pub(crate) fn launch_overrides_for(backend: Backend, os: &str) -> Vec<(String, String)> {
    match component::for_backend(backend, os) {
        Some(c) => c
            .dlls
            .iter()
            .map(|dll| (dll.clone(), "n,b".to_string()))
            .collect(),
        None => Vec::new(),
    }
}

/// Environment variables a launch needs for `backend` (before the bottle's own).
pub fn launch_env(backend: Backend) -> Vec<(String, String)> {
    match backend {
        // DXVK writes `<exe>_d3d11.log` next to the program by default; keep its
        // log on stderr only, where Vinter's run log captures it.
        Backend::Dxvk => vec![("DXVK_LOG_PATH".into(), "none".into())],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::runtime::Flavor;

    fn runtime(graphics: Vec<Backend>) -> Runtime {
        Runtime {
            id: "rt".into(),
            name: "Wine Test".into(),
            version: "11.0".into(),
            flavor: Flavor::Stable,
            path: PathBuf::from("/rt"),
            wine_bin: PathBuf::from("/rt/bin/wine"),
            installed_at: "2026-10-15T00:00:00Z".into(),
            size: 0,
            source_url: String::new(),
            arch: "x86_64".into(),
            needs_rosetta: false,
            graphics,
        }
    }

    fn availability(os: &str, graphics: Vec<Backend>) -> Vec<(Backend, bool)> {
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::AppPaths::with_data_dir(tmp.path());
        backends_for(&paths, &runtime(graphics), os)
            .into_iter()
            .map(|b| (b.backend, b.available))
            .collect()
    }

    #[test]
    fn availability_per_os_and_runtime() {
        let both = vec![Backend::Wined3d, Backend::Dxvk];
        assert_eq!(
            availability("macos", both.clone()),
            [
                (Backend::Wined3d, true),
                (Backend::Dxvk, true),
                (Backend::Dxmt, false),
                (Backend::D3dmetal, false),
            ]
        );
        assert_eq!(
            availability("linux", both),
            [
                (Backend::Wined3d, true),
                (Backend::Dxvk, true),
                (Backend::Dxmt, false),
                (Backend::D3dmetal, false),
            ]
        );
        // A runtime that does not list DXVK cannot use it.
        assert_eq!(
            availability("macos", vec![Backend::Wined3d])[1],
            (Backend::Dxvk, false)
        );
    }

    #[test]
    fn reasons_are_given() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::AppPaths::with_data_dir(tmp.path());
        let infos = backends_for(&paths, &runtime(vec![Backend::Wined3d]), "linux");
        assert!(infos[0].reason.is_none());
        assert!(
            infos[1]
                .reason
                .as_deref()
                .unwrap()
                .contains("does not support DXVK")
        );
        assert!(
            infos[2]
                .reason
                .as_deref()
                .unwrap()
                .contains("only available on macOS")
        );
    }

    #[test]
    fn launch_settings_per_backend() {
        assert!(launch_overrides_for(Backend::Wined3d, "macos").is_empty());
        assert!(launch_env(Backend::Wined3d).is_empty());

        // On macOS DXVK replaces only d3d11/d3d10core; Wine's dxgi stays.
        let mac: Vec<_> = launch_overrides_for(Backend::Dxvk, "macos");
        assert_eq!(
            mac,
            [
                ("d3d10core".into(), "n,b".into()),
                ("d3d11".into(), "n,b".into())
            ]
        );
        let linux: Vec<String> = launch_overrides_for(Backend::Dxvk, "linux")
            .into_iter()
            .map(|(dll, _)| dll)
            .collect();
        assert_eq!(linux, ["d3d8", "d3d9", "d3d10core", "d3d11", "dxgi"]);
        assert_eq!(
            launch_env(Backend::Dxvk),
            [("DXVK_LOG_PATH".into(), "none".into())]
        );
    }

    #[test]
    fn ids_round_trip() {
        for b in Backend::ALL {
            assert_eq!(Backend::parse(b.id()), Some(b));
            assert_eq!(serde_json::to_value(b).unwrap(), b.id());
        }
    }
}
