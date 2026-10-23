//! Which CPU a Windows program or installer is built for, read from its PE
//! header (`.exe`, `.dll`) or its MSI summary information (`.msi`).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::engine::EngineKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum PeArch {
    /// 32-bit x86.
    I386,
    /// 64-bit x86.
    Amd64,
    /// Native 64-bit ARM.
    Arm64,
    /// ARM64EC: ARM code that interoperates with x86-64 code in one process.
    Arm64ec,
    /// ARM64X: one file holding both ARM64 and ARM64EC code.
    Arm64x,
}

impl PeArch {
    /// The serde name, e.g. "arm64ec".
    pub fn id(self) -> &'static str {
        match self {
            Self::I386 => "i386",
            Self::Amd64 => "amd64",
            Self::Arm64 => "arm64",
            Self::Arm64ec => "arm64ec",
            Self::Arm64x => "arm64x",
        }
    }

    /// The badge on a program's tile.
    pub fn label(self) -> &'static str {
        match self {
            Self::I386 => "32-bit",
            Self::Amd64 => "64-bit",
            Self::Arm64 | Self::Arm64ec | Self::Arm64x => "ARM64",
        }
    }

    /// Runs as ARM code without translating x86 code first.
    pub fn is_arm(self) -> bool {
        matches!(self, Self::Arm64 | Self::Arm64ec | Self::Arm64x)
    }
}

const IMAGE_FILE_MACHINE_I386: u16 = 0x014c;
const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
const IMAGE_FILE_MACHINE_ARM64: u16 = 0xaa64;
/// What ARM64EC objects use; linked ARM64EC images say AMD64 instead.
const IMAGE_FILE_MACHINE_ARM64EC: u16 = 0xa641;
const IMAGE_FILE_MACHINE_ARM64X: u16 = 0xa64e;
const PE32_PLUS_MAGIC: u16 = 0x20b;
const LOAD_CONFIG_DIRECTORY: usize = 10;
/// `CHPEMetadataPointer` in `IMAGE_LOAD_CONFIG_DIRECTORY64`.
const CHPE_METADATA_OFFSET: u64 = 0xc8;

/// The architecture of `path`, or `None` if it is not a PE file or MSI
/// package Vinter can read.
pub fn detect(path: &Path) -> Option<PeArch> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    if ext.as_deref() == Some("msi") {
        return detect_msi(path);
    }
    detect_pe(&mut File::open(path).ok()?)
}

/// Reads the PE header, and for 64-bit images the load configuration: a
/// hybrid (CHPE) image is ARM64EC if its machine says AMD64, ARM64X if it
/// says ARM64.
pub fn detect_pe(file: &mut (impl Read + Seek)) -> Option<PeArch> {
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos).ok()?;
    if &dos[..2] != b"MZ" {
        return None;
    }
    let pe_offset = u64::from(u32::from_le_bytes(dos[0x3c..0x40].try_into().ok()?));
    file.seek(SeekFrom::Start(pe_offset)).ok()?;
    // Signature (4) + COFF header (20).
    let mut coff = [0u8; 24];
    file.read_exact(&mut coff).ok()?;
    if &coff[..4] != b"PE\0\0" {
        return None;
    }
    let machine = u16::from_le_bytes([coff[4], coff[5]]);
    let sections = usize::from(u16::from_le_bytes([coff[6], coff[7]]));
    let optional_size = usize::from(u16::from_le_bytes([coff[20], coff[21]]));

    let base = match machine {
        IMAGE_FILE_MACHINE_I386 => return Some(PeArch::I386),
        IMAGE_FILE_MACHINE_ARM64EC => return Some(PeArch::Arm64ec),
        IMAGE_FILE_MACHINE_ARM64X => return Some(PeArch::Arm64x),
        IMAGE_FILE_MACHINE_AMD64 => PeArch::Amd64,
        IMAGE_FILE_MACHINE_ARM64 => PeArch::Arm64,
        _ => return None,
    };
    let hybrid = has_chpe_metadata(file, optional_size, sections).unwrap_or(false);
    Some(match (base, hybrid) {
        (PeArch::Amd64, true) => PeArch::Arm64ec,
        (PeArch::Arm64, true) => PeArch::Arm64x,
        (arch, _) => arch,
    })
}

/// True if the image's load configuration points at CHPE (hybrid) metadata.
/// `file` is positioned right after the COFF header.
fn has_chpe_metadata(
    file: &mut (impl Read + Seek),
    optional_size: usize,
    sections: usize,
) -> Option<bool> {
    let mut optional = vec![0u8; optional_size];
    file.read_exact(&mut optional).ok()?;
    if optional.len() < 112 || u16::from_le_bytes([optional[0], optional[1]]) != PE32_PLUS_MAGIC {
        return Some(false);
    }
    let directory_count = u32::from_le_bytes(optional[108..112].try_into().ok()?) as usize;
    if directory_count <= LOAD_CONFIG_DIRECTORY {
        return Some(false);
    }
    let entry = 112 + LOAD_CONFIG_DIRECTORY * 8;
    let rva = u32::from_le_bytes(optional.get(entry..entry + 4)?.try_into().ok()?);
    let size = u32::from_le_bytes(optional.get(entry + 4..entry + 8)?.try_into().ok()?);
    if rva == 0 || u64::from(size) < CHPE_METADATA_OFFSET + 8 {
        return Some(false);
    }

    // Find the section holding the load configuration.
    let mut table = vec![0u8; sections * 40];
    file.read_exact(&mut table).ok()?;
    let offset = table.chunks_exact(40).find_map(|s| {
        let virtual_size = u32::from_le_bytes(s[8..12].try_into().ok()?);
        let virtual_address = u32::from_le_bytes(s[12..16].try_into().ok()?);
        let raw_size = u32::from_le_bytes(s[16..20].try_into().ok()?);
        let raw_pointer = u32::from_le_bytes(s[20..24].try_into().ok()?);
        let span = virtual_size.max(raw_size);
        (rva >= virtual_address && rva - virtual_address < span)
            .then(|| u64::from(raw_pointer) + u64::from(rva - virtual_address))
    })?;

    // The load configuration says how big it is; older ones end before the field.
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut declared = [0u8; 4];
    file.read_exact(&mut declared).ok()?;
    if u64::from(u32::from_le_bytes(declared)) < CHPE_METADATA_OFFSET + 8 {
        return Some(false);
    }
    file.seek(SeekFrom::Start(offset + CHPE_METADATA_OFFSET))
        .ok()?;
    let mut pointer = [0u8; 8];
    file.read_exact(&mut pointer).ok()?;
    Some(u64::from_le_bytes(pointer) != 0)
}

/// MSI packages name their platform in the summary information's Template
/// property, e.g. "x64;1033". An empty platform (";1033") means Intel, and
/// the `msi` crate reports that as no platform.
fn detect_msi(path: &Path) -> Option<PeArch> {
    let package = msi::open(path).ok()?;
    match package.summary_info().arch() {
        Some(platform) => arch_from_msi_platform(platform),
        None => Some(PeArch::I386),
    }
}

fn arch_from_msi_platform(platform: &str) -> Option<PeArch> {
    match platform.trim().to_ascii_lowercase().as_str() {
        "intel" | "" => Some(PeArch::I386),
        "x64" | "amd64" => Some(PeArch::Amd64),
        "arm64" => Some(PeArch::Arm64),
        _ => None,
    }
}

/// How a program of `arch` runs on `engine`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum RunPath {
    /// ARM code on the Apple Silicon engine: no emulation.
    NativeArm64,
    /// x86-64 code translated by FEX through ARM64EC.
    Fex64,
    /// 32-bit x86 code through WoW64, translated by FEX.
    FexWow64,
    /// x86 code run by x86_64 Wine directly (natively, or under Rosetta on Apple Silicon).
    X86,
    /// 32-bit x86 code through WoW64 in x86_64 Wine.
    X86Wow64,
}

impl RunPath {
    pub fn describe(self) -> &'static str {
        match self {
            Self::NativeArm64 => "native ARM64, no emulation",
            Self::Fex64 => "x86-64 translated by FEX (ARM64EC)",
            Self::FexWow64 => "32-bit x86 through WoW64, translated by FEX",
            Self::X86 => "x86-64 code run directly by x86-64 Wine",
            Self::X86Wow64 => "32-bit x86 through WoW64 in x86-64 Wine",
        }
    }
}

/// How `arch` runs on `engine`, or why it cannot.
pub fn run_path(arch: PeArch, engine: EngineKind) -> Result<RunPath, String> {
    let fex = matches!(engine, EngineKind::Arm64Fex | EngineKind::Arm64FexLinux);
    match (fex, arch) {
        (true, a) if a.is_arm() => Ok(RunPath::NativeArm64),
        (true, PeArch::Amd64) => Ok(RunPath::Fex64),
        (true, PeArch::I386) => Ok(RunPath::FexWow64),
        (false, PeArch::Amd64) => Ok(RunPath::X86),
        (false, PeArch::I386) => Ok(RunPath::X86Wow64),
        (false, _) => {
            Err("it is built for ARM64 Windows, which only runs in an Apple Silicon bottle".into())
        }
        (true, _) => unreachable!("every architecture is covered above"),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Cursor;

    use super::*;

    /// A minimal PE image: DOS header, COFF header, optional header with
    /// 16 data directories, one section. With `chpe`, a load configuration
    /// with a CHPE metadata pointer sits in that section.
    pub(crate) fn pe(machine: u16, pe32_plus: bool, chpe: bool) -> Vec<u8> {
        let pe_offset = 0x80usize;
        let optional_size: usize = if pe32_plus { 112 + 16 * 8 } else { 96 + 16 * 8 };
        let section_table = pe_offset + 24 + optional_size;
        let raw = 0x400usize;
        let mut f = vec![0u8; raw + 0x200];
        f[..2].copy_from_slice(b"MZ");
        f[0x3c..0x40].copy_from_slice(&(pe_offset as u32).to_le_bytes());
        f[pe_offset..pe_offset + 4].copy_from_slice(b"PE\0\0");
        f[pe_offset + 4..pe_offset + 6].copy_from_slice(&machine.to_le_bytes());
        f[pe_offset + 6..pe_offset + 8].copy_from_slice(&1u16.to_le_bytes());
        f[pe_offset + 20..pe_offset + 22].copy_from_slice(&(optional_size as u16).to_le_bytes());
        let opt = pe_offset + 24;
        let magic: u16 = if pe32_plus { 0x20b } else { 0x10b };
        f[opt..opt + 2].copy_from_slice(&magic.to_le_bytes());
        let dirs = if pe32_plus { 108 } else { 92 };
        f[opt + dirs..opt + dirs + 4].copy_from_slice(&16u32.to_le_bytes());
        // Section: VA 0x1000, raw data at 0x400, 0x200 bytes.
        let s = section_table;
        f[s..s + 5].copy_from_slice(b".text");
        f[s + 8..s + 12].copy_from_slice(&0x200u32.to_le_bytes());
        f[s + 12..s + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        f[s + 16..s + 20].copy_from_slice(&0x200u32.to_le_bytes());
        f[s + 20..s + 24].copy_from_slice(&(raw as u32).to_le_bytes());
        if pe32_plus && chpe {
            let entry = opt + 112 + LOAD_CONFIG_DIRECTORY * 8;
            f[entry..entry + 4].copy_from_slice(&0x1000u32.to_le_bytes());
            f[entry + 4..entry + 8].copy_from_slice(&0x140u32.to_le_bytes());
            f[raw..raw + 4].copy_from_slice(&0x140u32.to_le_bytes());
            let p = raw + CHPE_METADATA_OFFSET as usize;
            f[p..p + 8].copy_from_slice(&0x1_4000_2000u64.to_le_bytes());
        }
        f
    }

    fn arch(bytes: Vec<u8>) -> Option<PeArch> {
        detect_pe(&mut Cursor::new(bytes))
    }

    #[test]
    fn detects_each_architecture() {
        assert_eq!(
            arch(pe(IMAGE_FILE_MACHINE_I386, false, false)),
            Some(PeArch::I386)
        );
        assert_eq!(
            arch(pe(IMAGE_FILE_MACHINE_AMD64, true, false)),
            Some(PeArch::Amd64)
        );
        assert_eq!(
            arch(pe(IMAGE_FILE_MACHINE_ARM64, true, false)),
            Some(PeArch::Arm64)
        );
        assert_eq!(
            arch(pe(IMAGE_FILE_MACHINE_AMD64, true, true)),
            Some(PeArch::Arm64ec)
        );
        assert_eq!(
            arch(pe(IMAGE_FILE_MACHINE_ARM64, true, true)),
            Some(PeArch::Arm64x)
        );
        assert_eq!(
            arch(pe(IMAGE_FILE_MACHINE_ARM64X, true, false)),
            Some(PeArch::Arm64x)
        );
    }

    #[test]
    fn rejects_what_is_not_a_pe() {
        assert_eq!(
            arch(b"not an exe at all, just text, long enough to read 64 bytes of it....".to_vec()),
            None
        );
        assert_eq!(arch(vec![]), None);
        let mut bad_sig = pe(IMAGE_FILE_MACHINE_AMD64, true, false);
        bad_sig[0x80] = b'X';
        assert_eq!(arch(bad_sig), None);
        // An unknown machine (e.g. Itanium).
        assert_eq!(arch(pe(0x0200, true, false)), None);
        // A truncated file still gives the machine.
        let mut short = pe(IMAGE_FILE_MACHINE_AMD64, true, true);
        short.truncate(0x200);
        assert_eq!(arch(short), Some(PeArch::Amd64));
    }

    #[test]
    fn reads_files_and_msi_platforms() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("app.EXE");
        std::fs::write(&exe, pe(IMAGE_FILE_MACHINE_I386, false, false)).unwrap();
        assert_eq!(detect(&exe), Some(PeArch::I386));
        assert_eq!(detect(&tmp.path().join("missing.exe")), None);
        // Not a real MSI: unreadable, so unknown.
        let msi = tmp.path().join("setup.msi");
        std::fs::write(&msi, b"nope").unwrap();
        assert_eq!(detect(&msi), None);

        assert_eq!(arch_from_msi_platform("Intel"), Some(PeArch::I386));
        assert_eq!(arch_from_msi_platform("x64"), Some(PeArch::Amd64));
        assert_eq!(arch_from_msi_platform("Arm64"), Some(PeArch::Arm64));
        assert_eq!(arch_from_msi_platform("Intel64"), None);
    }

    #[test]
    fn run_paths() {
        use EngineKind::*;
        assert_eq!(run_path(PeArch::Arm64, Arm64Fex), Ok(RunPath::NativeArm64));
        assert_eq!(run_path(PeArch::Arm64x, Arm64Fex), Ok(RunPath::NativeArm64));
        assert_eq!(run_path(PeArch::Amd64, Arm64Fex), Ok(RunPath::Fex64));
        assert_eq!(run_path(PeArch::I386, Arm64Fex), Ok(RunPath::FexWow64));
        assert_eq!(run_path(PeArch::Amd64, X86_64Rosetta), Ok(RunPath::X86));
        assert_eq!(run_path(PeArch::I386, X86_64Native), Ok(RunPath::X86Wow64));
        assert!(run_path(PeArch::Arm64, X86_64Rosetta).is_err());
        assert!(run_path(PeArch::Arm64ec, X86_64Native).is_err());
    }
}
