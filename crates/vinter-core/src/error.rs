use std::path::PathBuf;

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
}

pub type Result<T> = std::result::Result<T, Error>;
