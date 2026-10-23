//! Graphics backends: how Direct3D calls get translated (plan section 4.3).
//!
//! Each backend knows which DLLs it puts in a prefix (via a downloadable
//! [`component`]), which overrides and environment variables a launch
//! needs, and where it can run.

pub mod apply;
pub mod component;
pub mod d3dmetal;
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
    let fex = crate::engine::is_fex(runtime.engine);
    Backend::ALL
        .into_iter()
        .map(|backend| {
            // The downloads are x86 builds; Apple Silicon bottles only use
            // what their runtime ships.
            let component = if fex {
                None
            } else {
                component::for_backend(backend, os)
            };
            let imported = backend == Backend::D3dmetal && d3dmetal::installed(paths).is_some();
            let reason = unavailable_reason(backend, runtime, os, component.is_some() || imported);
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
    if crate::engine::is_fex(runtime.engine) {
        return fex_unavailable_reason(backend, runtime);
    }
    match backend {
        Backend::Wined3d => None,
        Backend::D3dmetal if !runtime.graphics.contains(&backend) => Some(format!(
            "D3DMetal needs Vinter Wine 11.19 (macOS 15 or later); {} cannot run it. \
             Install it on the Runtimes page and choose it as this bottle's runtime.",
            runtime.name
        )),
        // D3DMetal's "component" is the user's own import of Apple's toolkit.
        Backend::D3dmetal if !has_component => Some(
            "Import Apple's Game Porting Toolkit in Settings first. Vinter cannot \
             include it, so you download it from Apple yourself."
                .into(),
        ),
        Backend::D3dmetal => None,
        // Tested with DXMT 0.80 on Gcenx Wine 11.0: the device works, but drawing
        // to a window needs Mac driver functions these Wine builds keep hidden.
        Backend::Dxmt if !runtime.graphics.contains(&backend) => Some(format!(
            "DXMT needs Vinter Wine (macOS 15 or later); {} cannot run it. Install \
             it on the Runtimes page, then choose it as this bottle's runtime.",
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

/// Apple Silicon bottles: WineD3D always works; DXMT only when the runtime
/// itself ships an ARM64EC build; the rest are experimental.
fn fex_unavailable_reason(backend: Backend, runtime: &Runtime) -> Option<String> {
    match backend {
        Backend::Wined3d => None,
        Backend::Dxmt if runtime_ships_dxmt(runtime) => None,
        Backend::Dxmt => Some(format!(
            "{} does not include DXMT for Apple Silicon yet, so bottles on it use Wine's own Direct3D.",
            runtime.name
        )),
        Backend::Dxvk => Some(
            "DXVK in Apple Silicon bottles is experimental (graphics-dxvk-arm64ec in Settings → Experimental), and only works with a runtime that includes it."
                .into(),
        ),
        Backend::D3dmetal => Some(
            "D3DMetal in Apple Silicon bottles is experimental (graphics-user-gptk in Settings → Experimental)."
                .into(),
        ),
    }
}

/// The runtime lists DXMT and ships its ARM64 Metal bridge (`winemetal`).
fn runtime_ships_dxmt(runtime: &Runtime) -> bool {
    runtime.graphics.contains(&Backend::Dxmt)
        && crate::engine::arm64_pe_dir(runtime)
            .join("winemetal.dll")
            .is_file()
}

/// The backend a new bottle on `runtime` starts with: DXMT on the Apple
/// Silicon engine when the runtime ships it, otherwise WineD3D, which
/// always renders. (Other backends need downloads, chosen by the user or a
/// recipe.)
pub fn default_for(runtime: &Runtime) -> Backend {
    if crate::engine::is_fex(runtime.engine) && runtime_ships_dxmt(runtime) {
        Backend::Dxmt
    } else {
        Backend::Wined3d
    }
}

/// DLL overrides a launch needs for `backend` (before the bottle's own).
pub fn launch_overrides(backend: Backend) -> Vec<(String, String)> {
    launch_overrides_for(backend, std::env::consts::OS)
}

/// The DirectX DLLs the D3DMetal runtime takes over (as builtins, from
/// `D3DMETAL_RUNTIME_DIR`).
const D3DMETAL_DLLS: &[&str] = &["d3d10", "d3d10core", "d3d11", "d3d12", "d3d12core", "dxgi"];

pub(crate) fn launch_overrides_for(backend: Backend, os: &str) -> Vec<(String, String)> {
    if backend == Backend::D3dmetal {
        return D3DMETAL_DLLS
            .iter()
            .map(|dll| (dll.to_string(), "n,b".to_string()))
            .collect();
    }
    match component::for_backend(backend, os) {
        Some(c) => c
            .dlls
            .iter()
            .map(|dll| (dll.clone(), "n,b".to_string()))
            .collect(),
        None => Vec::new(),
    }
}

/// Environment variables a launch needs for `backend` (before the bottle's
/// own). `data_dir` is Vinter's data folder (for D3DMetal's location).
pub fn launch_env(backend: Backend, data_dir: Option<&std::path::Path>) -> Vec<(String, String)> {
    match backend {
        // DXVK writes `<exe>_d3d11.log` next to the program by default; keep its
        // log on stderr only, where Vinter's run log captures it.
        Backend::Dxvk => vec![("DXVK_LOG_PATH".into(), "none".into())],
        // Read by a Vinter Wine built for D3DMetal.
        Backend::D3dmetal => {
            let mut env = vec![("GRAPHICS_BACKEND".to_string(), "d3dmetal".to_string())];
            if let Some(data) = data_dir {
                let lib = d3dmetal::lib_dir(&crate::AppPaths::with_data_dir(data));
                env.push(("D3DMETAL_RUNTIME_DIR".into(), lib.display().to_string()));
            }
            env
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::runtime::Flavor;

    #[test]
    fn apple_silicon_bottles_only_offer_what_the_runtime_ships() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::AppPaths::with_data_dir(tmp.path().join("data"));
        let root = tmp.path().join("rt");
        let mut rt = runtime(vec![Backend::Wined3d, Backend::Dxmt]);
        rt.engine = crate::engine::EngineKind::Arm64Fex;
        rt.path = root.clone();
        rt.wine_bin = root.join("bin/wine");

        let offered = |rt: &Runtime| {
            backends_for(&paths, rt, "macos")
                .into_iter()
                .map(|b| (b.backend, b.available, b.component.is_some()))
                .collect::<Vec<_>>()
        };
        // No ARM64 winemetal in the runtime: only WineD3D, and no x86 downloads.
        assert_eq!(
            offered(&rt),
            [
                (Backend::Wined3d, true, false),
                (Backend::Dxvk, false, false),
                (Backend::Dxmt, false, false),
                (Backend::D3dmetal, false, false),
            ]
        );
        assert_eq!(default_for(&rt), Backend::Wined3d);
        let dxvk = backends_for(&paths, &rt, "macos")
            .into_iter()
            .find(|b| b.backend == Backend::Dxvk)
            .unwrap();
        assert!(dxvk.reason.unwrap().contains("graphics-dxvk-arm64ec"));

        // The runtime ships DXMT for ARM64: offered, and the default.
        let pe = root.join("lib/wine/aarch64-windows");
        std::fs::create_dir_all(&pe).unwrap();
        std::fs::write(pe.join("winemetal.dll"), "").unwrap();
        assert!(offered(&rt).contains(&(Backend::Dxmt, true, false)));
        assert_eq!(default_for(&rt), Backend::Dxmt);

        // Not listed in the manifest: still not offered.
        rt.graphics = vec![Backend::Wined3d];
        assert_eq!(default_for(&rt), Backend::Wined3d);

        // Intel runtimes keep their defaults.
        assert_eq!(
            default_for(&runtime(vec![Backend::Wined3d, Backend::Dxmt])),
            Backend::Wined3d
        );
    }

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
            engine: crate::engine::EngineKind::legacy(),
            fex_version: None,
            unavailable: None,
            kerberos: false,
            domain_identity: false,
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
        assert!(launch_env(Backend::Wined3d, None).is_empty());

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
            launch_env(Backend::Dxvk, None),
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
