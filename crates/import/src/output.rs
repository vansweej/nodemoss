//! Import result types.

use rig_assets::{MaterialAsset, MeshAsset};

/// A single adapted mesh ready for registration in `AssetStore`.
#[derive(Clone, Debug)]
pub struct ImportedMesh {
    /// GPU-ready mesh asset.
    pub mesh: MeshAsset,
    /// Index into [`LoadedModel::materials`] if the source mesh had a material.
    pub material_index: Option<usize>,
    /// Source mesh name.
    pub name: String,
}

/// Meshes and materials adapted from one model import.
#[derive(Clone, Debug)]
pub struct LoadedModel {
    /// Adapted meshes. Callers register these individually with `AssetStore::add_mesh`.
    pub meshes: Vec<ImportedMesh>,
    /// Adapted materials paired with source material names.
    pub materials: Vec<(MaterialAsset, String)>,
}
