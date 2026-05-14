//! Import configuration types.

/// Texture adaptation settings applied after decoding and before registration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TextureConfig {
    /// Flip rows vertically. Defaults to `false` for wgpu's top-left texture origin.
    pub flip_y: bool,
    /// Premultiply RGB by alpha in-place. Defaults to `false`.
    pub premultiply_alpha: bool,
    /// Resize the largest dimension to this value using nearest-neighbour sampling.
    pub max_dimension: Option<u32>,
}

/// Mesh adaptation settings applied after OBJ decoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshConfig {
    /// Generate smooth normals when a mesh has no normals.
    pub generate_normals: bool,
    /// Reverse each triangle's winding order.
    pub reverse_winding: bool,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            generate_normals: true,
            reverse_winding: false,
        }
    }
}
