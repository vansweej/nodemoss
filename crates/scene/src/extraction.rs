//! Extraction types and frustum culling helpers.

use rig_math::{BoundingSphere, Mat4, Projection, Vec3, Vec4};

use crate::components::LightKind;
use crate::node::NodeId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedRenderable {
    pub node: NodeId,
    pub mesh: rig_assets::MeshHandle,
    pub material: rig_assets::MaterialHandle,
    pub world_transform: Mat4,
    pub world_bound: BoundingSphere,
}

/// Camera data extracted from the scene, ready for the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedCamera {
    pub node: NodeId,
    pub projection: Projection,
    pub world_transform: Mat4,
}

/// Light data extracted from the scene, ready for the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedLight {
    pub kind: LightKind,
    /// World-space position (relevant for `Point` lights).
    pub world_position: Vec3,
    /// World-space forward direction of the light node (-Z axis in local space).
    pub world_direction: Vec3,
}

pub fn frustum_planes_from_projection_view(matrix: Mat4) -> [Vec4; 6] {
    let left = matrix.col(3) + matrix.col(0);
    let right = matrix.col(3) - matrix.col(0);
    let bottom = matrix.col(3) + matrix.col(1);
    let top = matrix.col(3) - matrix.col(1);
    let near = matrix.col(3) + matrix.col(2);
    let far = matrix.col(3) - matrix.col(2);

    [left, right, bottom, top, near, far].map(normalize_plane)
}

pub(crate) fn normalize_plane(plane: Vec4) -> Vec4 {
    let normal = plane.truncate();
    let length = normal.length();
    if length > 0.0 { plane / length } else { plane }
}
