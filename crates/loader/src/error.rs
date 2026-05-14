//! Error type for format loading and source I/O.

use std::io;

use thiserror::Error;

/// Errors produced by `rig-loader` while reading bytes or decoding formats.
#[derive(Debug, Error)]
pub enum LoadError {
    /// The requested asset path was not present in the source.
    #[error("asset not found")]
    NotFound,
    /// The source returned an operating-system I/O error.
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
    /// The path extension is not handled by the requested loader entry point.
    #[error("unsupported format '{0}'")]
    UnsupportedFormat(String),
    /// The decoder rejected the bytes as malformed or unsupported data.
    #[error("decode error: {0}")]
    Decode(String),
}
