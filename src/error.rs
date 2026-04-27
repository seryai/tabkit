//! Error and Result types for tabkit.

use std::io;
use thiserror::Error;

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can arise during tabular extraction.
///
/// `#[non_exhaustive]` so future minor versions can add new
/// variants (e.g. a dedicated `EncryptedSpreadsheet` once
/// password-protected XLSX support lands) without breaking
/// downstream `match` blocks. Pattern-matchers must include a
/// wildcard arm.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// Filesystem failure: file missing, permission denied, EOF.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// The file extension has no registered reader on this engine.
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    /// Backend-specific parse failure. The string is the backend's
    /// error message verbatim, prefixed with the backend name so a
    /// caller can route on it.
    #[error("parse error: {0}")]
    ParseError(String),

    /// The requested sheet doesn't exist in a multi-sheet file, or
    /// the file contains no sheets at all.
    #[error("sheet `{requested}` not found in {path}; available: {available}")]
    SheetNotFound {
        /// The sheet name the caller asked for.
        requested: String,
        /// The file the caller pointed at.
        path: String,
        /// Comma-separated list of sheet names that ARE present,
        /// for diagnostic usefulness.
        available: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_errors_convert_via_from() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "missing");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }

    #[test]
    fn unsupported_format_renders_message() {
        let err = Error::UnsupportedFormat(".xyz".into());
        assert!(err.to_string().contains(".xyz"));
    }

    #[test]
    fn sheet_not_found_renders_diagnostically() {
        let err = Error::SheetNotFound {
            requested: "Q5".into(),
            path: "/tmp/x.xlsx".into(),
            available: "Q1, Q2, Q3, Q4".into(),
        };
        let s = err.to_string();
        assert!(s.contains("Q5"));
        assert!(s.contains("Q1, Q2, Q3, Q4"));
    }
}
