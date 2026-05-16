//! glTF mesh primitive adaptation.

use std::sync::Arc;

use rig_assets::{
    AssetStore, IndexFormat, MeshAsset, MeshHandle, MorphTargetHandle, MorphTargets,
    standard_vertex_layout, tangent_utils,
};
use rig_math::{BoundingSphere, Vec3};

use crate::buffers;
use crate::error::{GltfError, Result};

pub(crate) struct AdaptedPrimitive {
    pub mesh: MeshHandle,
    pub morph_targets: Option<MorphTargetHandle>,
}

/// Adapt a single glTF primitive into a `MeshAsset` with the standard 48-byte layout.
pub(crate) fn adapt_primitive(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
) -> Result<MeshAsset> {
    if primitive.mode() != gltf::mesh::Mode::Triangles {
        return Err(GltfError::UnsupportedTopology(primitive.mode()));
    }

    let positions = buffers::read_positions(primitive, buffers)?;
    let vertex_count = positions.len() / 3;
    let indices = buffers::read_indices(primitive, buffers)
        .unwrap_or_else(|| (0..vertex_count as u32).collect());
    let normals = buffers::read_normals(primitive, buffers)
        .filter(|normals| normals.len() == vertex_count * 3)
        .unwrap_or_else(|| generate_smooth_normals(&positions, &indices));
    let uvs = buffers::read_tex_coords(primitive, buffers, 0)
        .filter(|uvs| uvs.len() == vertex_count * 2)
        .unwrap_or_else(|| vec![0.0; vertex_count * 2]);
    let tangents = buffers::read_tangents(primitive, buffers)
        .filter(|tangents| tangents.len() == vertex_count)
        .unwrap_or_else(|| tangent_utils::generate_tangents(&positions, &normals, &uvs, &indices));

    let vertex_data = interleave_vertices(&positions, &normals, &uvs, &tangents, vertex_count);
    let (index_data, index_format) = pack_indices(&indices, vertex_count);

    Ok(MeshAsset {
        vertex_layout: standard_vertex_layout(),
        vertex_data: Arc::from(vertex_data),
        index_data: Arc::from(index_data),
        index_format,
        local_bounds: compute_bounding_sphere(&positions),
    })
}

/// Adapt all primitives of a glTF mesh, registering each in `store`.
pub(crate) fn adapt_mesh(
    mesh: &gltf::Mesh<'_>,
    buffers: &[gltf::buffer::Data],
    store: &mut AssetStore,
) -> Result<Vec<AdaptedPrimitive>> {
    mesh.primitives()
        .map(|primitive| {
            let positions = buffers::read_positions(&primitive, buffers)?;
            let vertex_count = positions.len() / 3;
            let morph_targets =
                buffers::read_morph_targets(&primitive, buffers, vertex_count).map(|data| {
                    store.add_morph_targets(MorphTargets {
                        vertex_count,
                        target_count: data.target_count,
                        position_deltas: data.position_deltas,
                        normal_deltas: data.normal_deltas,
                    })
                });
            let mesh = adapt_primitive(&primitive, buffers)?;
            Ok(AdaptedPrimitive {
                mesh: store.add_mesh(mesh),
                morph_targets,
            })
        })
        .collect()
}

pub(crate) fn primitive_count(mesh: &gltf::Mesh<'_>) -> usize {
    mesh.primitives().count()
}

fn interleave_vertices(
    positions: &[f32],
    normals: &[f32],
    uvs: &[f32],
    tangents: &[[f32; 4]],
    vertex_count: usize,
) -> Vec<u8> {
    let mut floats = Vec::with_capacity(vertex_count * 12);
    for index in 0..vertex_count {
        floats.extend_from_slice(&positions[index * 3..index * 3 + 3]);
        floats.extend_from_slice(&normals[index * 3..index * 3 + 3]);
        floats.extend_from_slice(&uvs[index * 2..index * 2 + 2]);
        floats.extend_from_slice(&tangents[index]);
    }
    bytemuck::cast_slice(&floats).to_vec()
}

fn generate_smooth_normals(positions: &[f32], indices: &[u32]) -> Vec<f32> {
    let vertex_count = positions.len() / 3;
    let mut normals = vec![Vec3::ZERO; vertex_count];
    for triangle in indices.chunks_exact(3) {
        let i0 = triangle[0] as usize;
        let i1 = triangle[1] as usize;
        let i2 = triangle[2] as usize;
        if i0 >= vertex_count || i1 >= vertex_count || i2 >= vertex_count {
            continue;
        }
        let p0 = read_position(positions, i0);
        let p1 = read_position(positions, i1);
        let p2 = read_position(positions, i2);
        let normal = (p1 - p0).cross(p2 - p0).normalize_or_zero();
        normals[i0] += normal;
        normals[i1] += normal;
        normals[i2] += normal;
    }
    normals
        .into_iter()
        .flat_map(|normal| normal.normalize_or_zero().to_array())
        .collect()
}

fn read_position(positions: &[f32], index: usize) -> Vec3 {
    Vec3::new(
        positions[index * 3],
        positions[index * 3 + 1],
        positions[index * 3 + 2],
    )
}

fn pack_indices(indices: &[u32], vertex_count: usize) -> (Vec<u8>, IndexFormat) {
    if vertex_count <= u16::MAX as usize {
        let packed: Vec<u16> = indices.iter().map(|&index| index as u16).collect();
        (bytemuck::cast_slice(&packed).to_vec(), IndexFormat::Uint16)
    } else {
        (bytemuck::cast_slice(indices).to_vec(), IndexFormat::Uint32)
    }
}

fn compute_bounding_sphere(positions: &[f32]) -> BoundingSphere {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for position in positions
        .chunks_exact(3)
        .map(|p| Vec3::new(p[0], p[1], p[2]))
    {
        min = min.min(position);
        max = max.max(position);
    }
    let center = (min + max) * 0.5;
    let radius = positions
        .chunks_exact(3)
        .map(|p| Vec3::new(p[0], p[1], p[2]).distance(center))
        .fold(0.0, f32::max);
    BoundingSphere { center, radius }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_normals_for_xy_triangle_face_positive_z() {
        let positions = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let normals = generate_smooth_normals(&positions, &[0, 1, 2]);

        for normal in normals.chunks_exact(3) {
            assert!((normal[0]).abs() < 1.0e-5);
            assert!((normal[1]).abs() < 1.0e-5);
            assert!((normal[2] - 1.0).abs() < 1.0e-5);
        }
    }

    #[test]
    fn pack_indices_uses_u16_for_small_meshes() {
        let (bytes, format) = pack_indices(&[0, 1, 255], 3);

        assert_eq!(format, IndexFormat::Uint16);
        assert_eq!(bytes, vec![0, 0, 1, 0, 255, 0]);
    }

    #[test]
    fn bounding_sphere_covers_positions() {
        let positions = [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
        let bounds = compute_bounding_sphere(&positions);

        assert_eq!(bounds.center, Vec3::ZERO);
        assert!((bounds.radius - 3.0_f32.sqrt()).abs() < 1.0e-5);
    }
}
