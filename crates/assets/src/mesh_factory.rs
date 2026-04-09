//! Procedural mesh generation.
//!
//! Each function returns a [`MeshAsset`] with the standard position + normal + UV
//! vertex layout:
//!
//! ```text
//! Position: Float32x3  @ location 0, offset  0
//! Normal:   Float32x3  @ location 1, offset 12
//! UV:       Float32x2  @ location 2, offset 24
//! stride = 32 bytes
//! ```
//!
//! Index format is `Uint16` for meshes with ≤ 65535 vertices, `Uint32` otherwise.

use std::sync::Arc;

use rig_math::{BoundingSphere, Vec3};

use crate::{IndexFormat, MeshAsset, VertexAttribute, VertexFormat, VertexLayout};

// ---------------------------------------------------------------------------
// Standard layout constants
// ---------------------------------------------------------------------------

const STRIDE: u64 = 32; // 3×f32 pos + 3×f32 normal + 2×f32 uv = 32 bytes

fn standard_layout() -> VertexLayout {
    VertexLayout {
        array_stride: STRIDE,
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
    }
}

// ---------------------------------------------------------------------------
// Internal vertex helper
// ---------------------------------------------------------------------------

fn push_vertex(buf: &mut Vec<u8>, pos: [f32; 3], normal: [f32; 3], uv: [f32; 2]) {
    for f in pos.iter().chain(normal.iter()) {
        buf.extend_from_slice(&f.to_le_bytes());
    }
    for f in &uv {
        buf.extend_from_slice(&f.to_le_bytes());
    }
}

fn push_u16(buf: &mut Vec<u8>, idx: u16) {
    buf.extend_from_slice(&idx.to_le_bytes());
}

fn push_u32(buf: &mut Vec<u8>, idx: u32) {
    buf.extend_from_slice(&idx.to_le_bytes());
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create an axis-aligned box centred at the origin.
///
/// Each face has its own 4 vertices so normals are face-flat (no shared
/// vertices).  24 vertices, 36 indices.
pub fn create_box(width: f32, height: f32, depth: f32) -> MeshAsset {
    let hx = width * 0.5;
    let hy = height * 0.5;
    let hz = depth * 0.5;

    // Per-face data: normals and corner UVs stored in parallel arrays to keep
    // the types simple. Corner order: BL, BR, TL, TR.
    #[rustfmt::skip]
    let face_normals: [[f32; 3]; 6] = [
        [ 1.0,  0.0,  0.0], // +X
        [-1.0,  0.0,  0.0], // -X
        [ 0.0,  1.0,  0.0], // +Y
        [ 0.0, -1.0,  0.0], // -Y
        [ 0.0,  0.0,  1.0], // +Z
        [ 0.0,  0.0, -1.0], // -Z
    ];
    #[rustfmt::skip]
    let face_uvs: [[[f32; 2]; 4]; 6] = [
        [[0.0,1.0],[1.0,1.0],[0.0,0.0],[1.0,0.0]], // +X
        [[1.0,1.0],[0.0,1.0],[1.0,0.0],[0.0,0.0]], // -X
        [[0.0,0.0],[1.0,0.0],[0.0,1.0],[1.0,1.0]], // +Y
        [[0.0,1.0],[1.0,1.0],[0.0,0.0],[1.0,0.0]], // -Y
        [[1.0,1.0],[0.0,1.0],[1.0,0.0],[0.0,0.0]], // +Z
        [[0.0,1.0],[1.0,1.0],[0.0,0.0],[1.0,0.0]], // -Z
    ];

    // Per-face vertex positions relative to the face normal.
    // For each face we define positions by offsetting from centre using the
    // two tangent axes derived from the normal.
    let half_extents = [hx, hy, hz];

    let mut vertex_data: Vec<u8> = Vec::with_capacity(24 * STRIDE as usize);
    let mut index_data: Vec<u8> = Vec::with_capacity(36 * 2);

    for (face_idx, (normal, uvs)) in face_normals.iter().zip(face_uvs.iter()).enumerate() {
        let nx = normal[0];
        let ny = normal[1];
        let nz = normal[2];

        // Build two tangent axes perpendicular to the normal and to each other.
        // tangent_u × tangent_v = normal (CCW winding).
        let (tangent_u, tangent_v) = if nx.abs() > 0.5 {
            // normal is ±X; tangents along Z and Y
            let sign = if nx > 0.0 { 1.0_f32 } else { -1.0_f32 };
            ([0.0_f32, 0.0, sign * hz], [0.0_f32, hy, 0.0])
        } else if ny.abs() > 0.5 {
            // normal is ±Y; tangents along X and Z
            let sign = if ny > 0.0 { 1.0_f32 } else { -1.0_f32 };
            ([hx, 0.0_f32, 0.0], [0.0_f32, 0.0, sign * hz])
        } else {
            // normal is ±Z; tangents along X and Y
            let sign = if nz > 0.0 { 1.0_f32 } else { -1.0_f32 };
            ([sign * hx, 0.0_f32, 0.0], [0.0_f32, hy, 0.0])
        };

        // The face centre is the normal scaled to the half-extent for that axis.
        let axis = face_idx / 2; // 0=X, 1=Y, 2=Z
        let sign = if face_idx % 2 == 0 { 1.0_f32 } else { -1.0_f32 };
        let centre = [
            if axis == 0 { sign * half_extents[0] } else { 0.0 },
            if axis == 1 { sign * half_extents[1] } else { 0.0 },
            if axis == 2 { sign * half_extents[2] } else { 0.0 },
        ];

        // Four corner positions: BL, BR, TL, TR (in face-local coords)
        let corners: [[f32; 3]; 4] = [
            [
                centre[0] - tangent_u[0] - tangent_v[0],
                centre[1] - tangent_u[1] - tangent_v[1],
                centre[2] - tangent_u[2] - tangent_v[2],
            ],
            [
                centre[0] + tangent_u[0] - tangent_v[0],
                centre[1] + tangent_u[1] - tangent_v[1],
                centre[2] + tangent_u[2] - tangent_v[2],
            ],
            [
                centre[0] - tangent_u[0] + tangent_v[0],
                centre[1] - tangent_u[1] + tangent_v[1],
                centre[2] - tangent_u[2] + tangent_v[2],
            ],
            [
                centre[0] + tangent_u[0] + tangent_v[0],
                centre[1] + tangent_u[1] + tangent_v[1],
                centre[2] + tangent_u[2] + tangent_v[2],
            ],
        ];

        let base = (face_idx * 4) as u16;
        for (corner, uv) in corners.iter().zip(uvs.iter()) {
            push_vertex(&mut vertex_data, *corner, *normal, *uv);
        }

        // Two CCW triangles: (0,1,2) and (1,3,2)
        push_u16(&mut index_data, base);
        push_u16(&mut index_data, base + 1);
        push_u16(&mut index_data, base + 2);
        push_u16(&mut index_data, base + 1);
        push_u16(&mut index_data, base + 3);
        push_u16(&mut index_data, base + 2);
    }

    let half_diagonal = Vec3::new(hx, hy, hz).length();

    MeshAsset {
        vertex_layout: standard_layout(),
        vertex_data: Arc::from(vertex_data.as_slice()),
        index_data: Arc::from(index_data.as_slice()),
        index_format: IndexFormat::Uint16,
        local_bounds: BoundingSphere {
            center: Vec3::ZERO,
            radius: half_diagonal,
        },
    }
}

/// Create a UV sphere centred at the origin.
///
/// - `slices`: longitudinal divisions (≥ 3)
/// - `stacks`: latitudinal divisions (≥ 2)
///
/// Vertex count: `(slices + 1) * (stacks + 1)`.
/// Index count:  `6 * slices * stacks`.
pub fn create_sphere(radius: f32, slices: u32, stacks: u32) -> MeshAsset {
    let slices = slices.max(3);
    let stacks = stacks.max(2);

    let vertex_count = (slices + 1) * (stacks + 1);
    let index_count = 6 * slices * stacks;

    let mut vertex_data: Vec<u8> = Vec::with_capacity(vertex_count as usize * STRIDE as usize);
    let mut index_data: Vec<u8> = Vec::with_capacity(index_count as usize * 2);

    for stack in 0..=stacks {
        let phi = std::f32::consts::PI * stack as f32 / stacks as f32; // [0, π]
        let sin_phi = phi.sin();
        let cos_phi = phi.cos();
        let v = stack as f32 / stacks as f32;

        for slice in 0..=slices {
            let theta = 2.0 * std::f32::consts::PI * slice as f32 / slices as f32; // [0, 2π]
            let sin_theta = theta.sin();
            let cos_theta = theta.cos();

            let nx = sin_phi * cos_theta;
            let ny = cos_phi;
            let nz = sin_phi * sin_theta;

            let pos = [radius * nx, radius * ny, radius * nz];
            let normal = [nx, ny, nz];
            let u = slice as f32 / slices as f32;
            push_vertex(&mut vertex_data, pos, normal, [u, v]);
        }
    }

    let use_u32 = vertex_count > u16::MAX as u32;

    for stack in 0..stacks {
        for slice in 0..slices {
            let a = stack * (slices + 1) + slice;
            let b = a + (slices + 1);

            if use_u32 {
                push_u32(&mut index_data, a);
                push_u32(&mut index_data, b);
                push_u32(&mut index_data, a + 1);
                push_u32(&mut index_data, b);
                push_u32(&mut index_data, b + 1);
                push_u32(&mut index_data, a + 1);
            } else {
                push_u16(&mut index_data, a as u16);
                push_u16(&mut index_data, b as u16);
                push_u16(&mut index_data, (a + 1) as u16);
                push_u16(&mut index_data, b as u16);
                push_u16(&mut index_data, (b + 1) as u16);
                push_u16(&mut index_data, (a + 1) as u16);
            }
        }
    }

    MeshAsset {
        vertex_layout: standard_layout(),
        vertex_data: Arc::from(vertex_data.as_slice()),
        index_data: Arc::from(index_data.as_slice()),
        index_format: if use_u32 {
            IndexFormat::Uint32
        } else {
            IndexFormat::Uint16
        },
        local_bounds: BoundingSphere {
            center: Vec3::ZERO,
            radius,
        },
    }
}

/// Create a flat quad in the XZ plane centred at the origin, facing +Y.
///
/// 4 vertices, 6 indices.
pub fn create_plane(width: f32, depth: f32) -> MeshAsset {
    let hx = width * 0.5;
    let hz = depth * 0.5;

    // Four corners: positions, shared normal (+Y), UV [0,1]
    let normal = [0.0_f32, 1.0, 0.0];
    let mut vertex_data: Vec<u8> = Vec::with_capacity(4 * STRIDE as usize);

    push_vertex(&mut vertex_data, [-hx, 0.0, -hz], normal, [0.0, 0.0]);
    push_vertex(&mut vertex_data, [ hx, 0.0, -hz], normal, [1.0, 0.0]);
    push_vertex(&mut vertex_data, [-hx, 0.0,  hz], normal, [0.0, 1.0]);
    push_vertex(&mut vertex_data, [ hx, 0.0,  hz], normal, [1.0, 1.0]);

    // Two CCW triangles (viewed from above, +Y direction)
    let mut index_data: Vec<u8> = Vec::with_capacity(6 * 2);
    push_u16(&mut index_data, 0);
    push_u16(&mut index_data, 1);
    push_u16(&mut index_data, 2);
    push_u16(&mut index_data, 1);
    push_u16(&mut index_data, 3);
    push_u16(&mut index_data, 2);

    let half_diagonal = Vec3::new(hx, 0.0, hz).length();

    MeshAsset {
        vertex_layout: standard_layout(),
        vertex_data: Arc::from(vertex_data.as_slice()),
        index_data: Arc::from(index_data.as_slice()),
        index_format: IndexFormat::Uint16,
        local_bounds: BoundingSphere {
            center: Vec3::ZERO,
            radius: half_diagonal,
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IndexFormat;

    fn decode_positions(vertex_data: &[u8], vertex_count: usize) -> Vec<[f32; 3]> {
        (0..vertex_count)
            .map(|i| {
                let base = i * STRIDE as usize;
                let x = f32::from_le_bytes(vertex_data[base..base + 4].try_into().unwrap());
                let y =
                    f32::from_le_bytes(vertex_data[base + 4..base + 8].try_into().unwrap());
                let z =
                    f32::from_le_bytes(vertex_data[base + 8..base + 12].try_into().unwrap());
                [x, y, z]
            })
            .collect()
    }

    fn decode_normals(vertex_data: &[u8], vertex_count: usize) -> Vec<[f32; 3]> {
        (0..vertex_count)
            .map(|i| {
                let base = i * STRIDE as usize + 12;
                let x = f32::from_le_bytes(vertex_data[base..base + 4].try_into().unwrap());
                let y =
                    f32::from_le_bytes(vertex_data[base + 4..base + 8].try_into().unwrap());
                let z =
                    f32::from_le_bytes(vertex_data[base + 8..base + 12].try_into().unwrap());
                [x, y, z]
            })
            .collect()
    }

    fn decode_u16_indices(index_data: &[u8]) -> Vec<u16> {
        index_data
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn create_box_produces_24_vertices_36_indices() {
        let mesh = create_box(1.0, 1.0, 1.0);
        let vertex_count = mesh.vertex_data.len() / STRIDE as usize;
        let index_count = mesh.index_data.len() / 2; // Uint16
        assert_eq!(vertex_count, 24);
        assert_eq!(index_count, 36);
    }

    #[test]
    fn create_box_bounds_are_half_diagonal() {
        let mesh = create_box(2.0, 4.0, 6.0);
        let expected = Vec3::new(1.0, 2.0, 3.0).length();
        assert!((mesh.local_bounds.radius - expected).abs() < 1e-5);
        assert_eq!(mesh.local_bounds.center, Vec3::ZERO);
    }

    #[test]
    fn create_sphere_vertex_normals_are_unit_length() {
        let mesh = create_sphere(1.0, 8, 6);
        let vertex_count = mesh.vertex_data.len() / STRIDE as usize;
        let normals = decode_normals(&mesh.vertex_data, vertex_count);

        for normal in &normals {
            let len = (normal[0] * normal[0]
                + normal[1] * normal[1]
                + normal[2] * normal[2])
                .sqrt();
            assert!((len - 1.0).abs() < 1e-5, "normal length was {len}");
        }
    }

    #[test]
    fn create_sphere_indices_stay_in_range() {
        let mesh = create_sphere(1.0, 8, 6);
        let vertex_count = (mesh.vertex_data.len() / STRIDE as usize) as u16;
        let indices = decode_u16_indices(&mesh.index_data);
        for &idx in &indices {
            assert!(idx < vertex_count, "index {idx} out of range (max {vertex_count})");
        }
    }

    #[test]
    fn create_plane_is_a_quad() {
        let mesh = create_plane(4.0, 6.0);
        let vertex_count = mesh.vertex_data.len() / STRIDE as usize;
        let index_count = mesh.index_data.len() / 2;
        assert_eq!(vertex_count, 4);
        assert_eq!(index_count, 6);
        assert_eq!(mesh.index_format, IndexFormat::Uint16);
        assert_eq!(mesh.vertex_layout.array_stride, STRIDE);
    }

    #[test]
    fn create_plane_normals_point_up() {
        let mesh = create_plane(2.0, 2.0);
        let normals = decode_normals(&mesh.vertex_data, 4);
        for n in &normals {
            assert!((n[0]).abs() < 1e-5);
            assert!((n[1] - 1.0).abs() < 1e-5);
            assert!((n[2]).abs() < 1e-5);
        }
    }

    #[test]
    fn all_mesh_factory_layouts_pass_standard_layout() {
        let box_mesh = create_box(1.0, 1.0, 1.0);
        let sphere_mesh = create_sphere(1.0, 6, 4);
        let plane_mesh = create_plane(1.0, 1.0);

        for mesh in &[box_mesh, sphere_mesh, plane_mesh] {
            assert_eq!(mesh.vertex_layout, standard_layout());
        }
    }

    #[test]
    fn create_sphere_bounding_sphere_equals_radius() {
        let mesh = create_sphere(3.5, 6, 4);
        assert!((mesh.local_bounds.radius - 3.5).abs() < 1e-5);
        assert_eq!(mesh.local_bounds.center, Vec3::ZERO);
    }

    #[test]
    fn create_box_uses_uint16_index_format() {
        let mesh = create_box(1.0, 2.0, 3.0);
        assert_eq!(mesh.index_format, IndexFormat::Uint16);
    }

    #[test]
    fn create_plane_positions_span_full_width_and_depth() {
        let mesh = create_plane(4.0, 6.0);
        let positions = decode_positions(&mesh.vertex_data, 4);
        let xs: Vec<f32> = positions.iter().map(|p| p[0]).collect();
        let zs: Vec<f32> = positions.iter().map(|p| p[2]).collect();
        let max_x = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let min_x = xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let max_z = zs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let min_z = zs.iter().cloned().fold(f32::INFINITY, f32::min);
        assert!((max_x - 2.0).abs() < 1e-5);
        assert!((min_x + 2.0).abs() < 1e-5);
        assert!((max_z - 3.0).abs() < 1e-5);
        assert!((min_z + 3.0).abs() < 1e-5);
    }
}
