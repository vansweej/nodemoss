//! The core CPU skinning evaluator.

use rig_assets::{AssetStore, DynamicMeshData, MeshHandle, SkinAssetHandle, SkinWeightsHandle};
use rig_math::{BoundingSphere, Mat3, Mat4, Vec3};
use rig_scene::{NodeId, SceneGraph};

use crate::SkinError;

/// Runtime CPU skinning evaluator — one instance per skinned mesh instance.
///
/// Shared asset handles (`SkinAssetHandle`, `SkinWeightsHandle`, `MeshHandle`)
/// are `Copy` — multiple evaluator instances can reference the same assets.
///
/// # Lifecycle
///
/// 1. Construct with [`SkinEvaluator::new`].
/// 2. Call [`SkinEvaluator::bind`] once after scene setup.
/// 3. Each frame in `update()`, after `AnimationPlayer::evaluate()` and
///    `scene.update_all_world_transforms()`, call [`SkinEvaluator::evaluate`].
/// 4. In `render()`, upload the returned [`DynamicMeshData`] via
///    `Renderer::update_dynamic_mesh`.
pub struct SkinEvaluator {
    skin: SkinAssetHandle,
    weights: SkinWeightsHandle,
    rest_mesh: MeshHandle,
    mesh_node: NodeId,
    /// Resolved joint `NodeId`s. `None` means the joint name was not found.
    joint_nodes: Vec<Option<NodeId>>,
    /// Scratch: joint palette reused each frame (avoids per-frame allocation).
    joint_palette: Vec<Mat4>,
    /// Whether `bind()` has been called successfully.
    bound: bool,
}

impl SkinEvaluator {
    /// Create a new evaluator. Call [`bind`](Self::bind) before
    /// [`evaluate`](Self::evaluate).
    pub fn new(
        skin: SkinAssetHandle,
        weights: SkinWeightsHandle,
        rest_mesh: MeshHandle,
        mesh_node: NodeId,
    ) -> Self {
        Self {
            skin,
            weights,
            rest_mesh,
            mesh_node,
            joint_nodes: Vec::new(),
            joint_palette: Vec::new(),
            bound: false,
        }
    }

    /// Resolve joint names to scene graph `NodeId`s and allocate scratch buffers.
    ///
    /// Call once after scene setup. Same lifecycle pattern as
    /// `AnimationPlayer::bind`. Unresolved joint names store `None` and are
    /// treated as identity transforms during evaluation.
    pub fn bind(&mut self, assets: &AssetStore, scene: &SceneGraph) -> Result<(), SkinError> {
        let skin = assets.skin(self.skin).map_err(|_| SkinError::InvalidSkin)?;
        let weights = assets
            .skin_weights(self.weights)
            .map_err(|_| SkinError::InvalidWeights)?;
        let mesh = assets
            .mesh(self.rest_mesh)
            .map_err(|_| SkinError::InvalidMesh)?;

        let mesh_vertex_count = mesh.vertex_data.len() / mesh.vertex_layout.array_stride as usize;
        let weights_vertex_count = weights.vertex_count();
        if mesh_vertex_count != weights_vertex_count {
            return Err(SkinError::VertexCountMismatch {
                mesh: mesh_vertex_count,
                weights: weights_vertex_count,
            });
        }

        let num_joints = skin.joint_names.len();
        self.joint_nodes = skin
            .joint_names
            .iter()
            .map(|name| scene.find_node_by_name(name))
            .collect();
        self.joint_palette = vec![Mat4::IDENTITY; num_joints];
        self.bound = true;
        Ok(())
    }

    /// Low-level: evaluate LBS with a caller-supplied joint palette.
    ///
    /// Each `joint_palette[j]` must already incorporate
    /// `inverse(mesh_world) * joint_world[j] * IBM[j]`.
    ///
    /// Useful for unit testing (pass synthetic matrices without a scene graph)
    /// and for future graphynx integration (compute palette externally).
    pub fn evaluate_with_palette(
        &self,
        joint_palette: &[Mat4],
        assets: &AssetStore,
    ) -> Result<DynamicMeshData, SkinError> {
        if !self.bound {
            return Err(SkinError::NotBound);
        }
        let mesh = assets
            .mesh(self.rest_mesh)
            .map_err(|_| SkinError::InvalidMesh)?;
        let weights = assets
            .skin_weights(self.weights)
            .map_err(|_| SkinError::InvalidWeights)?;
        lbs_inner(joint_palette, mesh, weights)
    }

    /// Evaluate LBS for the current frame using world transforms from the scene graph.
    ///
    /// Call after `AnimationPlayer::evaluate()` and
    /// `scene.update_all_world_transforms()`. Returns [`DynamicMeshData`] ready
    /// for `Renderer::update_dynamic_mesh`.
    pub fn evaluate(
        &mut self,
        assets: &AssetStore,
        scene: &SceneGraph,
    ) -> Result<DynamicMeshData, SkinError> {
        if !self.bound {
            return Err(SkinError::NotBound);
        }

        let skin = assets.skin(self.skin).map_err(|_| SkinError::InvalidSkin)?;
        let num_joints = skin.inverse_bind_matrices.len();

        let mesh_world = scene.world_transform(self.mesh_node)?;
        let mesh_inv = mesh_world.inverse();

        for j in 0..num_joints {
            let joint_world = match self.joint_nodes.get(j).copied().flatten() {
                Some(node) => scene.world_transform(node)?,
                None => Mat4::IDENTITY,
            };
            self.joint_palette[j] = mesh_inv * joint_world * skin.inverse_bind_matrices[j];
        }

        let palette = self.joint_palette.clone();
        self.evaluate_with_palette(&palette, assets)
    }
}

/// Core LBS vertex loop. Called by both `evaluate` and `evaluate_with_palette`.
///
/// Reads rest-pose vertices from `mesh.vertex_data` (standard layout, stride 32),
/// applies weighted joint transforms, writes skinned vertices to a new `Vec<u8>`,
/// and computes a bounding sphere from the skinned positions.
fn lbs_inner(
    joint_palette: &[Mat4],
    mesh: &rig_assets::MeshAsset,
    weights: &rig_assets::SkinWeights,
) -> Result<DynamicMeshData, SkinError> {
    let normal_palette: Vec<Mat3> = joint_palette
        .iter()
        .map(|m| Mat3::from_mat4(*m).inverse().transpose())
        .collect();

    let stride = mesh.vertex_layout.array_stride as usize;
    let vertex_count = mesh.vertex_data.len() / stride;
    let src = &mesh.vertex_data;

    let mut out_vertices = vec![0_u8; vertex_count * stride];
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);

    for v in 0..vertex_count {
        let base = v * stride;

        let rest_pos = Vec3::new(
            read_f32_le(src, base),
            read_f32_le(src, base + 4),
            read_f32_le(src, base + 8),
        );
        let rest_normal = Vec3::new(
            read_f32_le(src, base + 12),
            read_f32_le(src, base + 16),
            read_f32_le(src, base + 20),
        );

        let joint_indices = &weights.joints[v];
        let joint_weights = &weights.weights[v];

        let mut skinned_pos = Vec3::ZERO;
        let mut skinned_normal = Vec3::ZERO;

        for i in 0..8 {
            let w = joint_weights[i];
            if w == 0.0 {
                continue;
            }
            let j = joint_indices[i] as usize;
            skinned_pos += w * joint_palette[j].transform_point3(rest_pos);
            skinned_normal += w * (normal_palette[j] * rest_normal);
        }

        let skinned_normal = skinned_normal.normalize_or_zero();

        min = min.min(skinned_pos);
        max = max.max(skinned_pos);

        let ob = v * stride;
        write_f32_le(&mut out_vertices, ob, skinned_pos.x);
        write_f32_le(&mut out_vertices, ob + 4, skinned_pos.y);
        write_f32_le(&mut out_vertices, ob + 8, skinned_pos.z);
        write_f32_le(&mut out_vertices, ob + 12, skinned_normal.x);
        write_f32_le(&mut out_vertices, ob + 16, skinned_normal.y);
        write_f32_le(&mut out_vertices, ob + 20, skinned_normal.z);
        out_vertices[ob + 24..ob + 32].copy_from_slice(&src[base + 24..base + 32]);
    }

    let (center, radius) = if vertex_count == 0 {
        (Vec3::ZERO, 0.0)
    } else {
        let c = (min + max) * 0.5;
        let r = (max - min).length() * 0.5;
        (c, r)
    };

    let bytes_per_index = match mesh.index_format {
        rig_assets::IndexFormat::Uint16 => 2,
        rig_assets::IndexFormat::Uint32 => 4,
    };
    let index_count = (mesh.index_data.len() / bytes_per_index) as u32;

    Ok(DynamicMeshData {
        vertex_data: out_vertices,
        index_data: mesh.index_data.to_vec(),
        index_format: mesh.index_format,
        index_count,
        local_bounds: BoundingSphere { center, radius },
    })
}

fn read_f32_le(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn write_f32_le(bytes: &mut [u8], offset: usize, value: f32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rig_assets::{
        IndexFormat, MeshAsset, SkinAsset, SkinWeights, VertexAttribute, VertexFormat, VertexLayout,
    };

    use super::*;

    fn make_vertex(pos: Vec3, normal: Vec3, uv: [f32; 2]) -> [u8; 32] {
        let mut vertex = [0_u8; 32];
        write_f32_le(&mut vertex, 0, pos.x);
        write_f32_le(&mut vertex, 4, pos.y);
        write_f32_le(&mut vertex, 8, pos.z);
        write_f32_le(&mut vertex, 12, normal.x);
        write_f32_le(&mut vertex, 16, normal.y);
        write_f32_le(&mut vertex, 20, normal.z);
        write_f32_le(&mut vertex, 24, uv[0]);
        write_f32_le(&mut vertex, 28, uv[1]);
        vertex
    }

    fn make_mesh(vertices: &[[u8; 32]], indices: &[u16]) -> MeshAsset {
        let vertex_data: Vec<u8> = vertices
            .iter()
            .flat_map(|vertex| vertex.iter().copied())
            .collect();
        let index_data: Vec<u8> = indices
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect();
        MeshAsset {
            vertex_layout: VertexLayout {
                array_stride: 32,
                attributes: vec![
                    VertexAttribute {
                        shader_location: 0,
                        format: VertexFormat::Float32x3,
                        offset: 0,
                    },
                    VertexAttribute {
                        shader_location: 1,
                        format: VertexFormat::Float32x3,
                        offset: 12,
                    },
                    VertexAttribute {
                        shader_location: 2,
                        format: VertexFormat::Float32x2,
                        offset: 24,
                    },
                ],
            },
            vertex_data: Arc::from(vertex_data.as_slice()),
            index_data: Arc::from(index_data.as_slice()),
            index_format: IndexFormat::Uint16,
            local_bounds: BoundingSphere::ZERO,
        }
    }

    fn make_store(
        mesh: MeshAsset,
        skin: SkinAsset,
        weights: SkinWeights,
    ) -> (AssetStore, MeshHandle, SkinAssetHandle, SkinWeightsHandle) {
        let mut assets = AssetStore::new();
        let mesh_handle = assets.add_mesh(mesh);
        let skin_handle = assets.add_skin(skin);
        let weights_handle = assets.add_skin_weights(weights);
        (assets, mesh_handle, skin_handle, weights_handle)
    }

    fn make_evaluator(
        assets: &AssetStore,
        mesh: MeshHandle,
        skin: SkinAssetHandle,
        weights: SkinWeightsHandle,
    ) -> Result<SkinEvaluator, SkinError> {
        let mut scene = SceneGraph::new();
        let mesh_node = scene.create_node("mesh");
        let mut evaluator = SkinEvaluator::new(skin, weights, mesh, mesh_node);
        evaluator.bind(assets, &scene)?;
        Ok(evaluator)
    }

    fn skin_with_joints(count: usize) -> SkinAsset {
        SkinAsset {
            joint_names: (0..count).map(|i| format!("joint_{i}")).collect(),
            inverse_bind_matrices: vec![Mat4::IDENTITY; count],
        }
    }

    fn weights_for_vertices(count: usize, joints: [u16; 8], weights: [f32; 8]) -> SkinWeights {
        SkinWeights {
            joints: vec![joints; count],
            weights: vec![weights; count],
        }
    }

    fn read_pos(data: &DynamicMeshData, v: usize) -> Vec3 {
        let base = v * 32;
        Vec3::new(
            read_f32_le(&data.vertex_data, base),
            read_f32_le(&data.vertex_data, base + 4),
            read_f32_le(&data.vertex_data, base + 8),
        )
    }

    fn read_normal(data: &DynamicMeshData, v: usize) -> Vec3 {
        let base = v * 32;
        Vec3::new(
            read_f32_le(&data.vertex_data, base + 12),
            read_f32_le(&data.vertex_data, base + 16),
            read_f32_le(&data.vertex_data, base + 20),
        )
    }

    fn assert_vec3_approx_eq(actual: Vec3, expected: Vec3) {
        assert!(
            actual.abs_diff_eq(expected, 1e-5),
            "actual={actual:?} expected={expected:?}"
        );
    }

    #[test]
    fn identity_palette_preserves_vertices() {
        let vertices = [
            make_vertex(Vec3::ZERO, Vec3::Y, [0.0, 0.0]),
            make_vertex(Vec3::X, Vec3::Y, [1.0, 0.0]),
            make_vertex(Vec3::Z, Vec3::Y, [0.0, 1.0]),
        ];
        let mesh = make_mesh(&vertices, &[0, 1, 2]);
        let weights = weights_for_vertices(
            3,
            [0, 0, 0, 0, 0, 0, 0, 0],
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let (assets, mesh, skin, weights) = make_store(mesh, skin_with_joints(1), weights);
        let evaluator = make_evaluator(&assets, mesh, skin, weights).unwrap();

        let output = evaluator
            .evaluate_with_palette(&[Mat4::IDENTITY], &assets)
            .unwrap();

        assert_vec3_approx_eq(read_pos(&output, 0), Vec3::ZERO);
        assert_vec3_approx_eq(read_pos(&output, 1), Vec3::X);
        assert_vec3_approx_eq(read_pos(&output, 2), Vec3::Z);
        assert_vec3_approx_eq(read_normal(&output, 0), Vec3::Y);
        assert_vec3_approx_eq(read_normal(&output, 1), Vec3::Y);
        assert_vec3_approx_eq(read_normal(&output, 2), Vec3::Y);
    }

    #[test]
    fn single_bone_translation() {
        let vertices = [make_vertex(Vec3::X, Vec3::Y, [0.0, 0.0])];
        let mesh = make_mesh(&vertices, &[0]);
        let weights = weights_for_vertices(
            1,
            [0, 0, 0, 0, 0, 0, 0, 0],
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let (assets, mesh, skin, weights) = make_store(mesh, skin_with_joints(1), weights);
        let evaluator = make_evaluator(&assets, mesh, skin, weights).unwrap();

        let output = evaluator
            .evaluate_with_palette(&[Mat4::from_translation(Vec3::new(0.0, 2.0, 0.0))], &assets)
            .unwrap();

        assert_vec3_approx_eq(read_pos(&output, 0), Vec3::new(1.0, 2.0, 0.0));
        assert_vec3_approx_eq(read_normal(&output, 0), Vec3::Y);
    }

    #[test]
    fn two_bone_blend_50_50_cancels() {
        let vertices = [make_vertex(Vec3::ZERO, Vec3::Y, [0.0, 0.0])];
        let mesh = make_mesh(&vertices, &[0]);
        let weights = weights_for_vertices(
            1,
            [0, 1, 0, 0, 0, 0, 0, 0],
            [0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let (assets, mesh, skin, weights) = make_store(mesh, skin_with_joints(2), weights);
        let evaluator = make_evaluator(&assets, mesh, skin, weights).unwrap();

        let output = evaluator
            .evaluate_with_palette(
                &[
                    Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0)),
                    Mat4::from_translation(Vec3::new(-2.0, 0.0, 0.0)),
                ],
                &assets,
            )
            .unwrap();

        assert_vec3_approx_eq(read_pos(&output, 0), Vec3::ZERO);
    }

    #[test]
    fn normal_is_renormalized_after_blend() {
        let vertices = [make_vertex(Vec3::ZERO, Vec3::Z, [0.0, 0.0])];
        let mesh = make_mesh(&vertices, &[0]);
        let weights = weights_for_vertices(
            1,
            [0, 1, 0, 0, 0, 0, 0, 0],
            [0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let (assets, mesh, skin, weights) = make_store(mesh, skin_with_joints(2), weights);
        let evaluator = make_evaluator(&assets, mesh, skin, weights).unwrap();

        let output = evaluator
            .evaluate_with_palette(
                &[
                    Mat4::from_rotation_x(45.0_f32.to_radians()),
                    Mat4::from_rotation_x(-45.0_f32.to_radians()),
                ],
                &assets,
            )
            .unwrap();

        assert!((read_normal(&output, 0).length() - 1.0).abs() <= 1e-5);
    }

    #[test]
    fn uv_is_passed_through_unchanged() {
        let vertices = [make_vertex(Vec3::ZERO, Vec3::Y, [0.25, 0.75])];
        let mesh = make_mesh(&vertices, &[0]);
        let weights = weights_for_vertices(
            1,
            [0, 0, 0, 0, 0, 0, 0, 0],
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let (assets, mesh, skin, weights) = make_store(mesh, skin_with_joints(1), weights);
        let evaluator = make_evaluator(&assets, mesh, skin, weights).unwrap();

        let output = evaluator
            .evaluate_with_palette(&[Mat4::IDENTITY], &assets)
            .unwrap();

        assert_eq!(&output.vertex_data[24..32], &vertices[0][24..32]);
    }

    #[test]
    fn not_bound_returns_error() {
        let vertices = [make_vertex(Vec3::ZERO, Vec3::Y, [0.0, 0.0])];
        let mesh = make_mesh(&vertices, &[0]);
        let weights = weights_for_vertices(
            1,
            [0, 0, 0, 0, 0, 0, 0, 0],
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let (assets, mesh, skin, weights) = make_store(mesh, skin_with_joints(1), weights);
        let mut scene = SceneGraph::new();
        let evaluator = SkinEvaluator::new(skin, weights, mesh, scene.create_node("mesh"));

        assert!(matches!(
            evaluator.evaluate_with_palette(&[Mat4::IDENTITY], &assets),
            Err(SkinError::NotBound)
        ));
    }

    #[test]
    fn vertex_count_mismatch_detected_at_bind() {
        let vertices = [
            make_vertex(Vec3::ZERO, Vec3::Y, [0.0, 0.0]),
            make_vertex(Vec3::X, Vec3::Y, [0.0, 0.0]),
            make_vertex(Vec3::Z, Vec3::Y, [0.0, 0.0]),
        ];
        let mesh = make_mesh(&vertices, &[0, 1, 2]);
        let weights = weights_for_vertices(
            5,
            [0, 0, 0, 0, 0, 0, 0, 0],
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let (assets, mesh, skin, weights) = make_store(mesh, skin_with_joints(1), weights);
        let mut scene = SceneGraph::new();
        let mut evaluator = SkinEvaluator::new(skin, weights, mesh, scene.create_node("mesh"));

        assert!(matches!(
            evaluator.bind(&assets, &scene),
            Err(SkinError::VertexCountMismatch {
                mesh: 3,
                weights: 5
            })
        ));
    }

    #[test]
    fn unused_influences_ignored() {
        let vertices = [make_vertex(Vec3::X, Vec3::Y, [0.0, 0.0])];
        let mesh = make_mesh(&vertices, &[0]);
        let weights = weights_for_vertices(
            1,
            [0, 1, 2, 3, 0, 0, 0, 0],
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let (assets, mesh, skin, weights) = make_store(mesh, skin_with_joints(4), weights);
        let evaluator = make_evaluator(&assets, mesh, skin, weights).unwrap();

        let output = evaluator
            .evaluate_with_palette(
                &[
                    Mat4::from_translation(Vec3::Y),
                    Mat4::from_scale(Vec3::splat(100.0)),
                    Mat4::from_scale(Vec3::splat(100.0)),
                    Mat4::from_scale(Vec3::splat(100.0)),
                ],
                &assets,
            )
            .unwrap();

        assert_vec3_approx_eq(read_pos(&output, 0), Vec3::new(1.0, 1.0, 0.0));
    }

    #[test]
    fn index_data_is_passed_through_unchanged() {
        let vertices = [
            make_vertex(Vec3::ZERO, Vec3::Y, [0.0, 0.0]),
            make_vertex(Vec3::X, Vec3::Y, [0.0, 0.0]),
            make_vertex(Vec3::Z, Vec3::Y, [0.0, 0.0]),
        ];
        let mesh = make_mesh(&vertices, &[2, 1, 0]);
        let expected_index_data = mesh.index_data.to_vec();
        let weights = weights_for_vertices(
            3,
            [0, 0, 0, 0, 0, 0, 0, 0],
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let (assets, mesh, skin, weights) = make_store(mesh, skin_with_joints(1), weights);
        let evaluator = make_evaluator(&assets, mesh, skin, weights).unwrap();

        let output = evaluator
            .evaluate_with_palette(&[Mat4::IDENTITY], &assets)
            .unwrap();

        assert_eq!(output.index_data, expected_index_data);
    }

    #[test]
    fn bounds_computed_from_skinned_positions() {
        let vertices = [make_vertex(Vec3::ZERO, Vec3::Y, [0.0, 0.0])];
        let mesh = make_mesh(&vertices, &[0]);
        let weights = weights_for_vertices(
            1,
            [0, 0, 0, 0, 0, 0, 0, 0],
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let (assets, mesh, skin, weights) = make_store(mesh, skin_with_joints(1), weights);
        let evaluator = make_evaluator(&assets, mesh, skin, weights).unwrap();

        let output = evaluator
            .evaluate_with_palette(&[Mat4::from_translation(Vec3::new(3.0, 0.0, 0.0))], &assets)
            .unwrap();

        assert_vec3_approx_eq(output.local_bounds.center, Vec3::new(3.0, 0.0, 0.0));
    }
}
