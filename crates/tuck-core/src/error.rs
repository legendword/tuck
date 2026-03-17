use std::path::PathBuf;

pub type TuckResult<T> = Result<T, TuckError>;

#[derive(Debug, thiserror::Error)]
pub enum TuckError {
    #[error("IO error at {path}: {source}")]
    Io {
        source: std::io::Error,
        path: PathBuf,
    },

    #[error("Drive not connected: {0}")]
    DriveNotConnected(String),

    #[error("No external drive found")]
    NoDriveFound,

    #[error("Multiple external drives found: {0:?}. Specify one with --drive")]
    MultipleDrivesFound(Vec<String>),

    #[error("Path not found: {0}")]
    PathNotFound(PathBuf),

    #[error("Path is not archived: {0}")]
    NotArchived(PathBuf),

    #[error("Path already exists: {0}")]
    AlreadyExists(PathBuf),

    #[error("Checksum mismatch for {path}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[error("Manifest error: {0}")]
    Manifest(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

impl TuckError {
    pub fn exit_code(&self) -> i32 {
        match self {
            TuckError::Io { .. } => 1,
            TuckError::DriveNotConnected(_) => 2,
            TuckError::NoDriveFound => 2,
            TuckError::MultipleDrivesFound(_) => 2,
            TuckError::PathNotFound(_) => 1,
            TuckError::NotArchived(_) => 1,
            TuckError::AlreadyExists(_) => 1,
            TuckError::ChecksumMismatch { .. } => 3,
            TuckError::Manifest(_) => 1,
            TuckError::Cancelled => 4,
            TuckError::Other(_) => 1,
        }
    }
}

/// Extension trait for wrapping IO errors with path context.
pub trait IoContext<T> {
    fn io_context(self, path: impl Into<PathBuf>) -> TuckResult<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn io_context(self, path: impl Into<PathBuf>) -> TuckResult<T> {
        self.map_err(|source| TuckError::Io {
            source,
            path: path.into(),
        })
    }
}
