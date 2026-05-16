//! CPU morph target evaluator.

use rig_assets::{AssetStore, DynamicMeshData, MeshHandle, MorphTargetHandle};
use rig_math::{BoundingSphere, Vec3};

use crate::SkinError;

/// Runtime CPU morph target evaluator — one instance per morphed mesh instance.
pub struct MorphEvaluator {
    morph_targets: MorphTargetHandle,
    rest_mesh: MeshHandle,
    weights: Vec<f32>,
}

impl MorphEvaluator {
    pub fn new(morph_targets: MorphTargetHandle, rest_mesh: MeshHandle) -> Self {
        Self {
            morph_targets,
            rest_mesh,
            weights: Vec::new(),
        }
    }

    pub fn set_weights(&mut self, weights: &[f32]) {
        self.weights.clear();
        self.weights.extend_from_slice(weights);
    }

    pub fn evaluate(&self, assets: &AssetStore) -> Result<DynamicMeshData, SkinError> {
        let mesh = assets
            .mesh(self.rest_mesh)
            .map_err(|_| SkinError::InvalidMesh)?;
        let targets = assets
            .morph_targets(self.morph_targets)
            .map_err(|_| SkinError::InvalidMorphTargets)?;

        let stride = mesh.vertex_layout.array_stride as usize;
        let vertex_count = mesh.vertex_data.len() / stride;
        if vertex_count != targets.vertex_count {
            return Err(SkinError::MorphVertexCountMismatch {
                mesh: vertex_count,
                morph_targets: targets.vertex_count,
            });
        }

        let src = &mesh.vertex_data;
        let mut out_vertices = vec![0_u8; vertex_count * stride];
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);

        for v in 0..vertex_count {
            let base = v * stride;
            let mut position = Vec3::new(
                read_f32_le(src, base),
                read_f32_le(src, base + 4),
                read_f32_le(src, base + 8),
            );
            let rest_normal = Vec3::new(
                read_f32_le(src, base + 12),
                read_f32_le(src, base + 16),
                read_f32_le(src, base + 20),
            );
            let mut normal = rest_normal;

            for target in 0..targets.target_count {
                let weight = self.weights.get(target).copied().unwrap_or(0.0);
                if weight == 0.0 {
                    continue;
                }
                let offset = (target * targets.vertex_count + v) * 3;
                position += weight
                    * Vec3::new(
                        targets.position_deltas[offset],
                        targets.position_deltas[offset + 1],
                        targets.position_deltas[offset + 2],
                    );
                normal += weight
                    * Vec3::new(
                        targets.normal_deltas[offset],
                        targets.normal_deltas[offset + 1],
                        targets.normal_deltas[offset + 2],
                    );
            }

            let normal = normal.normalize_or_zero();
            min = min.min(position);
            max = max.max(position);

            write_f32_le(&mut out_vertices, base, position.x);
            write_f32_le(&mut out_vertices, base + 4, position.y);
            write_f32_le(&mut out_vertices, base + 8, position.z);
            write_f32_le(&mut out_vertices, base + 12, normal.x);
            write_f32_le(&mut out_vertices, base + 16, normal.y);
            write_f32_le(&mut out_vertices, base + 20, normal.z);
            out_vertices[base + 24..base + stride].copy_from_slice(&src[base + 24..base + stride]);
        }

        let (center, radius) = if vertex_count == 0 {
            (Vec3::ZERO, 0.0)
        } else {
            let center = (min + max) * 0.5;
            let radius = (max - min).length() * 0.5;
            (center, radius)
        };
        let bytes_per_index = match mesh.index_format {
            rig_assets::IndexFormat::Uint16 => 2,
            rig_assets::IndexFormat::Uint32 => 4,
        };

        Ok(DynamicMeshData {
            vertex_data: out_vertices,
            index_data: mesh.index_data.to_vec(),
            index_format: mesh.index_format,
            index_count: (mesh.index_data.len() / bytes_per_index) as u32,
            local_bounds: BoundingSphere { center, radius },
        })
    }
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
        IndexFormat, MeshAsset, MorphTargets, VertexAttribute, VertexFormat, VertexLayout,
    };

    use super::*;

    #[test]
    fn evaluate_blends_positions_and_normals() {
        let mut assets = AssetStore::new();
        let mesh = assets.add_mesh(mesh_with_one_vertex());
        let targets = assets.add_morph_targets(MorphTargets {
            vertex_count: 1,
            target_count: 2,
            position_deltas: vec![1.0, 0.0, 0.0, 0.0, 2.0, 0.0],
            normal_deltas: vec![0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        });
        let mut evaluator = MorphEvaluator::new(targets, mesh);
        evaluator.set_weights(&[0.5, 0.25]);

        let data = evaluator.evaluate(&assets).unwrap();

        assert!((read_f32_le(&data.vertex_data, 0) - 0.5).abs() < 1.0e-6);
        assert!((read_f32_le(&data.vertex_data, 4) - 0.5).abs() < 1.0e-6);
        let normal = Vec3::new(
            read_f32_le(&data.vertex_data, 12),
            read_f32_le(&data.vertex_data, 16),
            read_f32_le(&data.vertex_data, 20),
        );
        assert!(normal.is_normalized());
        assert_eq!(data.index_count, 1);
    }

    fn mesh_with_one_vertex() -> MeshAsset {
        let mut vertex = [0_u8; 32];
        write_f32_le(&mut vertex, 20, 1.0);
        let index_data = 0_u16.to_le_bytes();
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
            vertex_data: Arc::from(vertex.as_slice()),
            index_data: Arc::from(index_data.as_slice()),
            index_format: IndexFormat::Uint16,
            local_bounds: BoundingSphere::ZERO,
        }
    }
}
