//! Crate-wide error type. Every failure path in the library returns one of these; nothing panics.

use std::path::PathBuf;

/// Everything that can go wrong while turning a fabrication package into a cassette.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Reading or writing a file or directory failed.
    #[error("I/O error on {}: {source}", path.display())]
    Io {
        /// File or directory involved.
        path: PathBuf,
        /// Underlying OS error.
        source: std::io::Error,
    },

    /// The input `.zip` could not be read.
    #[error("cannot read archive {}: {message}", path.display())]
    Archive {
        /// Archive path.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },

    /// A required layer is absent from the package.
    #[error("{input}: no {what} found (expected e.g. {hint})")]
    MissingLayer {
        /// Human description of the layer ("board outline", "plated drill file").
        what: &'static str,
        /// The package that was searched.
        input: String,
        /// Example file names that would have matched.
        hint: &'static str,
    },

    /// A layer file is malformed.
    #[error("{file}{}: {message}", line.map_or_else(String::new, |l| format!(":{l}")))]
    Parse {
        /// File name inside the package.
        file: String,
        /// 1-based line number, when known.
        line: Option<usize>,
        /// What is wrong.
        message: String,
    },

    /// Invalid configuration, CLI value or settings combination.
    #[error("invalid settings: {0}")]
    Config(String),

    /// The probe / pin selection is impossible or ambiguous.
    #[error("pin selection: {0}")]
    Selection(String),

    /// The requested cassette cannot be built or printed as specified.
    #[error("cassette geometry: {0}")]
    Geometry(String),

    /// A reference image could not be inspected.
    #[error("cannot read image {}: {message}", path.display())]
    Image {
        /// Image path.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },

    /// JSON serialisation failed.
    #[error("cannot serialise JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Build an [`Error::Io`].
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Build an [`Error::Parse`] without a line number.
    pub fn parse(file: &str, message: impl Into<String>) -> Self {
        Self::Parse {
            file: file.to_owned(),
            line: None,
            message: message.into(),
        }
    }

    /// Build an [`Error::Parse`] pointing at a 1-based line.
    pub fn parse_at(file: &str, line: usize, message: impl Into<String>) -> Self {
        Self::Parse {
            file: file.to_owned(),
            line: Some(line),
            message: message.into(),
        }
    }
}
