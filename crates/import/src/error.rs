//! Import-layer errors.

use rig_loader::{AssetPath, LoadError};
use thiserror::Error;

/// Errors produced while adapting decoded assets into framework assets.
#[derive(Debug, Error)]
pub enum ImportError {
    /// Loading or decoding failed in `rig-loader`.
    #[error(transparent)]
    Load(#[from] LoadError),
    /// A mesh did not contain positions, which are required by the renderer.
    #[error("mesh '{mesh}' has no position data")]
    MissingPositions { mesh: String },
    /// An index referenced a vertex outside the vertex array.
    #[error("mesh '{mesh}': index {index} exceeds vertex count {vertex_count}")]
    IndexOverflow {
        /// Mesh name.
        mesh: String,
        /// Offending index value.
        index: u32,
        /// Number of vertices in the mesh.
        vertex_count: usize,
    },
    /// A referenced dependency such as a material texture could not be loaded.
    #[error("unresolved dependency '{path}': {source}")]
    UnresolvedDependency {
        /// Dependency path after importer-side resolution.
        path: AssetPath,
        /// Loader error for the dependency.
        source: LoadError,
    },
}
