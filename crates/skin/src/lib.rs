//! CPU-based linear blend skinning for the rig framework.
//!
//! Provides [`SkinEvaluator`] which builds a joint palette from the scene
//! graph each frame and applies 8-influence Linear Blend Skinning (LBS) to
//! positions and normals. Output is [`DynamicMeshData`] consumed by the
//! existing `DynamicMesh` renderer path.

mod evaluator;

pub use evaluator::SkinEvaluator;

use thiserror::Error;

/// Errors returned by skinning operations.
#[derive(Debug, Error)]
pub enum SkinError {
    #[error("invalid skin asset handle")]
    InvalidSkin,
    #[error("invalid skin weights handle")]
    InvalidWeights,
    #[error("invalid rest mesh handle")]
    InvalidMesh,
    #[error("vertex count mismatch: mesh has {mesh} vertices, weights has {weights}")]
    VertexCountMismatch { mesh: usize, weights: usize },
    #[error("evaluator not bound — call bind() before evaluate()")]
    NotBound,
    #[error("scene error: {0}")]
    Scene(#[from] rig_scene::SceneError),
}
