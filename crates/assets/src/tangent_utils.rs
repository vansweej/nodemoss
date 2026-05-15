//! Tangent generation helpers for the standard 48-byte vertex layout.

use mikktspace::Geometry;
use rig_math::Vec3;

const EPSILON: f32 = 1.0e-8;

/// Generate per-vertex MikkTSpace tangents for indexed triangle geometry.
///
/// Inputs are tightly-packed float slices: positions and normals are 3 floats per
/// vertex, UVs are 2 floats per vertex, and indices are triangle-list `u32`s. If
/// the inputs are unsuitable for MikkTSpace generation, tangents fall back to a
/// deterministic normal-derived tangent for every vertex.
pub fn generate_tangents(
    positions: &[f32],
    normals: &[f32],
    uvs: &[f32],
    indices: &[u32],
) -> Vec<[f32; 4]> {
    let vertex_count = positions.len() / 3;
    let fallback = fallback_tangents(normals, vertex_count);

    if vertex_count == 0
        || positions.len() != vertex_count * 3
        || normals.len() != vertex_count * 3
        || !has_valid_uvs(uvs, vertex_count)
        || indices.len() % 3 != 0
        || indices.iter().any(|&index| index as usize >= vertex_count)
    {
        return fallback;
    }

    let mut geometry = MikktGeometry {
        positions,
        normals,
        uvs,
        indices,
        tangents: fallback,
    };

    if mikktspace::generate_tangents(&mut geometry) {
        geometry.tangents
    } else {
        fallback_tangents(normals, vertex_count)
    }
}

/// Derive a stable tangent from a normal when UV-based generation is unavailable.
///
/// Uses `T = normalize(cross(N, UP))`, falling back to `RIGHT` if the normal is
/// parallel to `UP`. The handedness component is always `1.0`.
pub fn normal_derived_tangent(normal: [f32; 3]) -> [f32; 4] {
    let n = Vec3::from_array(normal).normalize_or_zero();
    if n.length_squared() <= EPSILON {
        return [1.0, 0.0, 0.0, 1.0];
    }

    let mut tangent = n.cross(Vec3::Y);
    if tangent.length_squared() <= EPSILON {
        tangent = n.cross(Vec3::X);
    }
    let tangent = tangent.normalize_or_zero();
    if tangent.length_squared() <= EPSILON {
        [1.0, 0.0, 0.0, 1.0]
    } else {
        [tangent.x, tangent.y, tangent.z, 1.0]
    }
}

/// Return true when a UV slice matches `vertex_count` and contains non-zero data.
pub fn has_valid_uvs(uvs: &[f32], vertex_count: usize) -> bool {
    uvs.len() == vertex_count * 2 && uvs.iter().any(|uv| uv.abs() > EPSILON)
}

struct MikktGeometry<'a> {
    positions: &'a [f32],
    normals: &'a [f32],
    uvs: &'a [f32],
    indices: &'a [u32],
    tangents: Vec<[f32; 4]>,
}

impl MikktGeometry<'_> {
    fn vertex_index(&self, face: usize, vert: usize) -> usize {
        self.indices[face * 3 + vert] as usize
    }
}

impl Geometry for MikktGeometry<'_> {
    fn num_faces(&self) -> usize {
        self.indices.len() / 3
    }

    fn num_vertices_of_face(&self, _face: usize) -> usize {
        3
    }

    fn position(&self, face: usize, vert: usize) -> [f32; 3] {
        let index = self.vertex_index(face, vert) * 3;
        [
            self.positions[index],
            self.positions[index + 1],
            self.positions[index + 2],
        ]
    }

    fn normal(&self, face: usize, vert: usize) -> [f32; 3] {
        let index = self.vertex_index(face, vert) * 3;
        [
            self.normals[index],
            self.normals[index + 1],
            self.normals[index + 2],
        ]
    }

    fn tex_coord(&self, face: usize, vert: usize) -> [f32; 2] {
        let index = self.vertex_index(face, vert) * 2;
        [self.uvs[index], self.uvs[index + 1]]
    }

    fn set_tangent_encoded(&mut self, tangent: [f32; 4], face: usize, vert: usize) {
        let index = self.vertex_index(face, vert);
        let direction = Vec3::new(tangent[0], tangent[1], tangent[2]).normalize_or_zero();
        if direction.length_squared() <= EPSILON || !tangent.iter().all(|value| value.is_finite()) {
            return;
        }
        self.tangents[index] = [
            direction.x,
            direction.y,
            direction.z,
            if tangent[3] < 0.0 { -1.0 } else { 1.0 },
        ];
    }
}

fn fallback_tangents(normals: &[f32], vertex_count: usize) -> Vec<[f32; 4]> {
    (0..vertex_count)
        .map(|index| {
            if normals.len() >= index * 3 + 3 {
                normal_derived_tangent([
                    normals[index * 3],
                    normals[index * 3 + 1],
                    normals[index * 3 + 2],
                ])
            } else {
                normal_derived_tangent([0.0, 1.0, 0.0])
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot3(a: [f32; 4], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    #[test]
    fn normal_derived_tangent_is_orthogonal() {
        let normal = [0.0, 0.0, 1.0];
        let tangent = normal_derived_tangent(normal);

        assert!(dot3(tangent, normal).abs() < 1.0e-5);
        assert!((tangent[3] - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn normal_derived_tangent_handles_up_facing_normal() {
        let tangent = normal_derived_tangent([0.0, 1.0, 0.0]);

        assert!(tangent[0].abs() < 1.0e-5);
        assert!(tangent[1].abs() < 1.0e-5);
        assert!((tangent[2] + 1.0).abs() < 1.0e-5);
        assert!((tangent[3] - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn has_valid_uvs_requires_expected_length_and_non_zero_data() {
        assert!(has_valid_uvs(&[0.0, 0.0, 1.0, 0.0], 2));
        assert!(!has_valid_uvs(&[0.0, 0.0, 0.0, 0.0], 2));
        assert!(!has_valid_uvs(&[0.0, 1.0], 2));
    }

    #[test]
    fn simple_quad_generates_mikktspace_tangents() {
        let positions = [
            -1.0, -1.0, 0.0, // 0
            1.0, -1.0, 0.0, // 1
            -1.0, 1.0, 0.0, // 2
            1.0, 1.0, 0.0, // 3
        ];
        let normals = [
            0.0, 0.0, 1.0, // 0
            0.0, 0.0, 1.0, // 1
            0.0, 0.0, 1.0, // 2
            0.0, 0.0, 1.0, // 3
        ];
        let uvs = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let indices = [0, 1, 2, 1, 3, 2];

        let tangents = generate_tangents(&positions, &normals, &uvs, &indices);

        assert_eq!(tangents.len(), 4);
        for tangent in tangents {
            assert!((tangent[0] - 1.0).abs() < 1.0e-5, "{tangent:?}");
            assert!(tangent[1].abs() < 1.0e-5, "{tangent:?}");
            assert!(tangent[2].abs() < 1.0e-5, "{tangent:?}");
            assert!((tangent[3] - 1.0).abs() < 1.0e-5, "{tangent:?}");
        }
    }
}
