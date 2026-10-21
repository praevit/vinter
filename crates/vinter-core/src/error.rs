use std::path::{Path, PathBuf};

/// Errors returned by `vinter-core`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not determine the user's home directory")]
    NoHomeDir,

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid id {0:?}: use only letters, digits, '.', '_' and '-'")]
    InvalidId(String),

    #[error("no runtime called {0:?} is available for this system")]
    RuntimeNotFound(String),

    #[error("runtime {0:?} is not installed")]
    RuntimeNotInstalled(String),

    #[error("runtime {0:?} is already installed")]
    RuntimeAlreadyInstalled(String),

    #[error("download failed: {0}")]
    Download(String),

    #[error(
        "checksum mismatch: expected sha256 {expected}, got {actual}. The download was deleted."
    )]
    ChecksumMismatch { expected: String, actual: String },

    #[error("the archive does not look like the expected runtime: {0}")]
    BadArchive(String),

    #[error("could not read {path}: {message}")]
    Config { path: PathBuf, message: String },

    #[error("no bottle {0:?}")]
    BottleNotFound(String),

    #[error("no approval request {0:?}")]
    ApprovalNotFound(String),

    #[error("no snapshot {0:?}")]
    SnapshotNotFound(String),

    #[error("{0:?} matches more than one bottle; use its id")]
    AmbiguousBottle(String),

    #[error("invalid bottle name: {0}")]
    InvalidName(String),

    #[error("bottle {0:?} is not set up yet")]
    BottleNotReady(String),

    #[error("bottle {bottle:?} uses runtime {runtime:?}, which is not installed")]
    BottleRuntimeMissing { bottle: String, runtime: String },

    #[error("{runtime} cannot be removed while these bottles use it: {}", bottles.join(", "))]
    RuntimeInUse {
        runtime: String,
        bottles: Vec<String>,
    },

    #[error("{0} needs Rosetta 2, which is not installed")]
    RosettaRequired(String),

    #[error("{what} failed: {message}")]
    ProcessFailed { what: String, message: String },

    #[error("the bottle has no program number {0}")]
    ProgramNotFound(usize),

    #[error("{name} is no longer installed at {exe}")]
    ProgramMissing { name: String, exe: String },

    #[error("{0:?} is not a valid program for this bottle")]
    InvalidProgram(String),

    #[error("installer not found: {}", .0.display())]
    InstallerNotFound(PathBuf),

    #[error("{0}")]
    BackendUnavailable(String),

    #[error("{0} is not downloaded yet")]
    ComponentMissing(String),

    #[error("invalid setting: {0}")]
    InvalidSetting(String),

    #[error("invalid recipe: {0}")]
    InvalidRecipe(String),

    #[error("no recipe {0:?}")]
    RecipeNotFound(String),

    #[error(
        "installing {0} uses winetricks, which may download software from Microsoft and others under their own licenses; agree to continue"
    )]
    ConsentRequired(String),

    #[error("{0:?} is not a component Vinter can install")]
    UnknownComponent(String),

    #[error("this recipe needs an installer file. {0}")]
    InstallerRequired(String),

    #[error("{step} failed: {message}")]
    RecipeStepFailed { step: String, message: String },

    #[error("bottle {0:?} has no unfinished recipe")]
    NothingToResume(String),

    #[error("could not move the bottle to the Trash: {0}")]
    Trash(String),

    #[error("could not use the keychain: {0}")]
    Keychain(String),

    #[error("signing in to the domain failed: {0}")]
    DomainSignIn(String),

    #[error("Rosetta 2 installation failed: {0}")]
    RosettaInstall(String),

    #[error("{0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Attaches a path to I/O errors: `fs::read(p).at(p)?`.
pub(crate) trait IoResultExt<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T>;
}

impl<T> IoResultExt<T> for std::io::Result<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T> {
        self.map_err(|source| Error::Io {
            path: path.as_ref().to_path_buf(),
            source,
        })
    }
}

/// Ids end up in file paths, so they must never contain separators or `..`.
pub fn validate_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidId(id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert!(validate_id("wine-stable-11.0-macos").is_ok());
        for bad in ["", ".", "..", "../x", "a/b", "a\\b", ".hidden", "a b", "é"] {
            assert!(validate_id(bad).is_err(), "{bad:?} should be rejected");
        }
    }
}
