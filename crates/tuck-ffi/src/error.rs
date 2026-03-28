use tuck_core::error::TuckError;

#[derive(Debug, uniffi::Error)]
pub enum FfiTuckError {
    Io {
        message: String,
        path: String,
    },
    DriveNotConnected {
        name: String,
    },
    NoDriveFound,
    MultipleDrivesFound {
        names: Vec<String>,
    },
    PathNotFound {
        path: String,
    },
    NotArchived {
        path: String,
    },
    AlreadyExists {
        path: String,
    },
    InsufficientSpace {
        path: String,
        needed: String,
        available: String,
    },
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    Manifest {
        message: String,
    },
    Cancelled,
    Other {
        message: String,
    },
}

impl From<TuckError> for FfiTuckError {
    fn from(e: TuckError) -> Self {
        match e {
            TuckError::Io { source, path } => FfiTuckError::Io {
                message: source.to_string(),
                path: path.to_string_lossy().into_owned(),
            },
            TuckError::DriveNotConnected(name) => FfiTuckError::DriveNotConnected { name },
            TuckError::NoDriveFound => FfiTuckError::NoDriveFound,
            TuckError::MultipleDrivesFound(names) => {
                FfiTuckError::MultipleDrivesFound { names }
            }
            TuckError::PathNotFound(path) => FfiTuckError::PathNotFound {
                path: path.to_string_lossy().into_owned(),
            },
            TuckError::NotArchived(path) => FfiTuckError::NotArchived {
                path: path.to_string_lossy().into_owned(),
            },
            TuckError::AlreadyExists(path) => FfiTuckError::AlreadyExists {
                path: path.to_string_lossy().into_owned(),
            },
            TuckError::InsufficientSpace {
                path,
                needed,
                available,
            } => FfiTuckError::InsufficientSpace {
                path: path.to_string_lossy().into_owned(),
                needed,
                available,
            },
            TuckError::ChecksumMismatch {
                path,
                expected,
                actual,
            } => FfiTuckError::ChecksumMismatch {
                path: path.to_string_lossy().into_owned(),
                expected,
                actual,
            },
            TuckError::Manifest(message) => FfiTuckError::Manifest { message },
            TuckError::Cancelled => FfiTuckError::Cancelled,
            TuckError::Other(message) => FfiTuckError::Other { message },
        }
    }
}

impl std::fmt::Display for FfiTuckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FfiTuckError::Io { message, path } => write!(f, "IO error at {path}: {message}"),
            FfiTuckError::DriveNotConnected { name } => {
                write!(f, "Drive not connected: {name}")
            }
            FfiTuckError::NoDriveFound => write!(f, "No external drive found"),
            FfiTuckError::MultipleDrivesFound { names } => {
                write!(f, "Multiple drives found: {names:?}")
            }
            FfiTuckError::PathNotFound { path } => write!(f, "Path not found: {path}"),
            FfiTuckError::NotArchived { path } => write!(f, "Path is not archived: {path}"),
            FfiTuckError::AlreadyExists { path } => write!(f, "Path already exists: {path}"),
            FfiTuckError::InsufficientSpace {
                path,
                needed,
                available,
            } => write!(
                f,
                "Not enough space on {path}: need {needed}, available {available}"
            ),
            FfiTuckError::ChecksumMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "Checksum mismatch for {path}: expected {expected}, got {actual}"
            ),
            FfiTuckError::Manifest { message } => write!(f, "Manifest error: {message}"),
            FfiTuckError::Cancelled => write!(f, "Operation cancelled"),
            FfiTuckError::Other { message } => write!(f, "{message}"),
        }
    }
}
