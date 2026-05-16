//! Error types for the rig-gltf crate.

use thiserror::Error;

/// Errors produced while loading and adapting glTF assets.
#[derive(Debug, Error)]
pub enum GltfError {
    #[error("glTF import error: {0}")]
    Import(#[from] gltf::Error),

    #[error("unsupported primitive topology: {0:?}")]
    UnsupportedTopology(gltf::mesh::Mode),

    #[error("primitive missing required POSITION attribute")]
    MissingPositions,

    #[error("skin weight attribute set {set} is incomplete")]
    IncompleteSkinWeights { set: u32 },
}

/// Convenience alias for `Result<T, GltfError>`.
pub type Result<T> = std::result::Result<T, GltfError>;
