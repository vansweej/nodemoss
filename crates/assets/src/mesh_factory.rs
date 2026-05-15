//! Procedural mesh generation.
//!
//! # Parametric shapes
//!
//! - [`create_box`] — axis-aligned box with flat per-face normals
//! - [`create_sphere`] — UV sphere with configurable subdivision
//! - [`create_plane`] — flat quad in the XZ plane
//!
//! # Platonic solids
//!
//! All five platonic solids are inscribed in the unit sphere centred at the
//! origin. Normals equal vertex positions (smooth shading) and texture
//! coordinates use spherical (longitude/latitude) mapping.
//!
//! - [`create_tetrahedron`] — 4 vertices, 4 triangles
//! - [`create_hexahedron`] — 8 vertices, 12 triangles (inscribed cube)
//! - [`create_octahedron`] — 6 vertices, 8 triangles
//! - [`create_dodecahedron`] — 20 vertices, 36 triangles
//! - [`create_icosahedron`] — 12 vertices, 20 triangles
//!
//! # Vertex layout
//!
//! Every function returns a [`MeshAsset`] with the standard layout:
//!
//! ```text
//! Position: Float32x3  @ location 0, offset  0
//! Normal:   Float32x3  @ location 1, offset 12
//! UV:       Float32x2  @ location 2, offset 24
//! stride = 32 bytes
//! ```
//!
//! Index format is `Uint16` for meshes with ≤ 65 535 vertices, `Uint32` otherwise.

use std::sync::Arc;

use rig_math::{BoundingSphere, Vec3};

use crate::{IndexFormat, MeshAsset, VertexAttribute, VertexFormat, VertexLayout};

// ---------------------------------------------------------------------------
// Standard layout constants
// ---------------------------------------------------------------------------

const STRIDE: u64 = 32; // 3×f32 pos + 3×f32 normal + 2×f32 uv = 32 bytes

pub(crate) fn standard_layout() -> VertexLayout {
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
// Internal vertex helpers
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

#[cfg(not(tarpaulin_include))]
fn push_u32(buf: &mut Vec<u8>, idx: u32) {
    buf.extend_from_slice(&idx.to_le_bytes());
}

/// Spherical UV mapping for a point on the unit sphere.
///
/// - `u` = longitude: `0.5 × (1 + atan2(y, x) / π)`
/// - `v` = latitude:  `acos(z) / π`
///
/// At the poles (`|z| ≈ 1`) `u` is clamped to 0.5 to avoid `atan2`
/// instability.
fn platonic_uv(pos: [f32; 3]) -> [f32; 2] {
    let u = if pos[2].abs() < 1.0 {
        0.5 * (1.0 + pos[1].atan2(pos[0]) * std::f32::consts::FRAC_1_PI)
    } else {
        0.5
    };
    let v = pos[2].clamp(-1.0, 1.0).acos() * std::f32::consts::FRAC_1_PI;
    [u, v]
}

// ---------------------------------------------------------------------------
// Public API — parametric shapes
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
            let sign = if nx > 0.0 { -1.0_f32 } else { 1.0_f32 };
            ([0.0_f32, 0.0, sign * hz], [0.0_f32, hy, 0.0])
        } else if ny.abs() > 0.5 {
            // normal is ±Y; tangents along X and Z
            let sign = if ny > 0.0 { -1.0_f32 } else { 1.0_f32 };
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
            if axis == 0 {
                sign * half_extents[0]
            } else {
                0.0
            },
            if axis == 1 {
                sign * half_extents[1]
            } else {
                0.0
            },
            if axis == 2 {
                sign * half_extents[2]
            } else {
                0.0
            },
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
                push_u32(&mut index_data, a + 1);
                push_u32(&mut index_data, b);
                push_u32(&mut index_data, b);
                push_u32(&mut index_data, a + 1);
                push_u32(&mut index_data, b + 1);
            } else {
                push_u16(&mut index_data, a as u16);
                push_u16(&mut index_data, (a + 1) as u16);
                push_u16(&mut index_data, b as u16);
                push_u16(&mut index_data, b as u16);
                push_u16(&mut index_data, (a + 1) as u16);
                push_u16(&mut index_data, (b + 1) as u16);
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
    push_vertex(&mut vertex_data, [hx, 0.0, -hz], normal, [1.0, 0.0]);
    push_vertex(&mut vertex_data, [-hx, 0.0, hz], normal, [0.0, 1.0]);
    push_vertex(&mut vertex_data, [hx, 0.0, hz], normal, [1.0, 1.0]);

    // Two CCW triangles (viewed from above, +Y direction)
    let mut index_data: Vec<u8> = Vec::with_capacity(6 * 2);
    push_u16(&mut index_data, 0);
    push_u16(&mut index_data, 2);
    push_u16(&mut index_data, 1);
    push_u16(&mut index_data, 1);
    push_u16(&mut index_data, 2);
    push_u16(&mut index_data, 3);

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
// Public API — platonic solids
// ---------------------------------------------------------------------------

/// Create a regular tetrahedron inscribed in the unit sphere.
///
/// All 4 vertices lie on the unit sphere centred at the origin.
/// Normals equal positions (smooth shading). UVs use spherical
/// longitude/latitude mapping.
///
/// 4 vertices, 12 indices (4 triangles).
pub fn create_tetrahedron() -> MeshAsset {
    let sqrt2_div3 = 2.0_f32.sqrt() / 3.0;
    let sqrt6_div3 = 6.0_f32.sqrt() / 3.0;
    let one_third = 1.0_f32 / 3.0;

    #[rustfmt::skip]
    let positions: [[f32; 3]; 4] = [
        [ 0.0,          0.0,       1.0      ],
        [ 2.0 * sqrt2_div3, 0.0,  -one_third],
        [-sqrt2_div3,   sqrt6_div3, -one_third],
        [-sqrt2_div3,  -sqrt6_div3, -one_third],
    ];

    let mut vertex_data: Vec<u8> = Vec::with_capacity(4 * STRIDE as usize);
    for pos in &positions {
        push_vertex(&mut vertex_data, *pos, *pos, platonic_uv(*pos));
    }

    // CCW winding for outside view (matches GTE)
    #[rustfmt::skip]
    let triangles: [[u16; 3]; 4] = [
        [0, 1, 2],
        [0, 2, 3],
        [0, 3, 1],
        [1, 3, 2],
    ];

    let mut index_data: Vec<u8> = Vec::with_capacity(12 * 2);
    for tri in &triangles {
        push_u16(&mut index_data, tri[0]);
        push_u16(&mut index_data, tri[1]);
        push_u16(&mut index_data, tri[2]);
    }

    MeshAsset {
        vertex_layout: standard_layout(),
        vertex_data: Arc::from(vertex_data.as_slice()),
        index_data: Arc::from(index_data.as_slice()),
        index_format: IndexFormat::Uint16,
        local_bounds: BoundingSphere {
            center: Vec3::ZERO,
            radius: 1.0,
        },
    }
}

/// Create a regular hexahedron (cube) inscribed in the unit sphere.
///
/// All 8 vertices lie on the unit sphere (half-edge = 1/√3). Normals
/// equal positions (smooth shading). UVs use spherical mapping.
///
/// This differs from [`create_box`]: the box uses flat per-face normals,
/// configurable extents, and 24 vertices. The hexahedron uses smooth
/// normals, a fixed unit-sphere inscription, and only 8 vertices.
///
/// 8 vertices, 36 indices (12 triangles).
pub fn create_hexahedron() -> MeshAsset {
    let s = 1.0_f32 / 3.0_f32.sqrt(); // ≈ 0.5774

    #[rustfmt::skip]
    let positions: [[f32; 3]; 8] = [
        [-s, -s, -s],
        [ s, -s, -s],
        [ s,  s, -s],
        [-s,  s, -s],
        [-s, -s,  s],
        [ s, -s,  s],
        [ s,  s,  s],
        [-s,  s,  s],
    ];

    let mut vertex_data: Vec<u8> = Vec::with_capacity(8 * STRIDE as usize);
    for pos in &positions {
        push_vertex(&mut vertex_data, *pos, *pos, platonic_uv(*pos));
    }

    // 12 triangles — CCW outside view (matches GTE)
    #[rustfmt::skip]
    let triangles: [[u16; 3]; 12] = [
        [0, 3, 2], [0, 2, 1],
        [0, 1, 5], [0, 5, 4],
        [0, 4, 7], [0, 7, 3],
        [6, 5, 1], [6, 1, 2],
        [6, 2, 3], [6, 3, 7],
        [6, 7, 4], [6, 4, 5],
    ];

    let mut index_data: Vec<u8> = Vec::with_capacity(36 * 2);
    for tri in &triangles {
        push_u16(&mut index_data, tri[0]);
        push_u16(&mut index_data, tri[1]);
        push_u16(&mut index_data, tri[2]);
    }

    MeshAsset {
        vertex_layout: standard_layout(),
        vertex_data: Arc::from(vertex_data.as_slice()),
        index_data: Arc::from(index_data.as_slice()),
        index_format: IndexFormat::Uint16,
        local_bounds: BoundingSphere {
            center: Vec3::ZERO,
            radius: 1.0,
        },
    }
}

/// Create a regular octahedron inscribed in the unit sphere.
///
/// The 6 vertices are the positive and negative axis-aligned unit vectors.
/// Normals equal positions (smooth shading). UVs use spherical mapping.
///
/// 6 vertices, 24 indices (8 triangles).
pub fn create_octahedron() -> MeshAsset {
    #[rustfmt::skip]
    let positions: [[f32; 3]; 6] = [
        [ 1.0,  0.0,  0.0],
        [-1.0,  0.0,  0.0],
        [ 0.0,  1.0,  0.0],
        [ 0.0, -1.0,  0.0],
        [ 0.0,  0.0,  1.0],
        [ 0.0,  0.0, -1.0],
    ];

    let mut vertex_data: Vec<u8> = Vec::with_capacity(6 * STRIDE as usize);
    for pos in &positions {
        push_vertex(&mut vertex_data, *pos, *pos, platonic_uv(*pos));
    }

    // 8 triangles — CCW outside view (matches GTE)
    #[rustfmt::skip]
    let triangles: [[u16; 3]; 8] = [
        [4, 0, 2],
        [4, 2, 1],
        [4, 1, 3],
        [4, 3, 0],
        [5, 2, 0],
        [5, 1, 2],
        [5, 3, 1],
        [5, 0, 3],
    ];

    let mut index_data: Vec<u8> = Vec::with_capacity(24 * 2);
    for tri in &triangles {
        push_u16(&mut index_data, tri[0]);
        push_u16(&mut index_data, tri[1]);
        push_u16(&mut index_data, tri[2]);
    }

    MeshAsset {
        vertex_layout: standard_layout(),
        vertex_data: Arc::from(vertex_data.as_slice()),
        index_data: Arc::from(index_data.as_slice()),
        index_format: IndexFormat::Uint16,
        local_bounds: BoundingSphere {
            center: Vec3::ZERO,
            radius: 1.0,
        },
    }
}

/// Create a regular dodecahedron inscribed in the unit sphere.
///
/// All 20 vertices lie on the unit sphere. Each pentagonal face is
/// triangulated into 3 triangles (36 total). Normals equal positions
/// (smooth shading). UVs use spherical mapping.
///
/// 20 vertices, 108 indices (36 triangles).
pub fn create_dodecahedron() -> MeshAsset {
    let a = 1.0_f32 / 3.0_f32.sqrt();
    let b = ((3.0 - 5.0_f32.sqrt()) / 6.0).sqrt();
    let c = ((3.0 + 5.0_f32.sqrt()) / 6.0).sqrt();

    #[rustfmt::skip]
    let positions: [[f32; 3]; 20] = [
        [ a,  a,  a],  //  0
        [ a,  a, -a],  //  1
        [ a, -a,  a],  //  2
        [ a, -a, -a],  //  3
        [-a,  a,  a],  //  4
        [-a,  a, -a],  //  5
        [-a, -a,  a],  //  6
        [-a, -a, -a],  //  7
        [ b,  c,  0.0], //  8
        [-b,  c,  0.0], //  9
        [ b, -c,  0.0], // 10
        [-b, -c,  0.0], // 11
        [ c,  0.0,  b], // 12
        [ c,  0.0, -b], // 13
        [-c,  0.0,  b], // 14
        [-c,  0.0, -b], // 15
        [ 0.0,  b,  c], // 16
        [ 0.0, -b,  c], // 17
        [ 0.0,  b, -c], // 18
        [ 0.0, -b, -c], // 19
    ];

    let mut vertex_data: Vec<u8> = Vec::with_capacity(20 * STRIDE as usize);
    for pos in &positions {
        push_vertex(&mut vertex_data, *pos, *pos, platonic_uv(*pos));
    }

    // 36 triangles — CCW outside view (matches GTE)
    #[rustfmt::skip]
    let triangles: [[u16; 3]; 36] = [
        [ 0,  8,  9], [ 0,  9,  4], [ 0,  4, 16],
        [ 0, 12, 13], [ 0, 13,  1], [ 0,  1,  8],
        [ 0, 16, 17], [ 0, 17,  2], [ 0,  2, 12],
        [ 8,  1, 18], [ 8, 18,  5], [ 8,  5,  9],
        [12,  2, 10], [12, 10,  3], [12,  3, 13],
        [16,  4, 14], [16, 14,  6], [16,  6, 17],
        [ 9,  5, 15], [ 9, 15, 14], [ 9, 14,  4],
        [ 6, 11, 10], [ 6, 10,  2], [ 6,  2, 17],
        [ 3, 19, 18], [ 3, 18,  1], [ 3,  1, 13],
        [ 7, 15,  5], [ 7,  5, 18], [ 7, 18, 19],
        [ 7, 11,  6], [ 7,  6, 14], [ 7, 14, 15],
        [ 7, 19,  3], [ 7,  3, 10], [ 7, 10, 11],
    ];

    let mut index_data: Vec<u8> = Vec::with_capacity(108 * 2);
    for tri in &triangles {
        push_u16(&mut index_data, tri[0]);
        push_u16(&mut index_data, tri[1]);
        push_u16(&mut index_data, tri[2]);
    }

    MeshAsset {
        vertex_layout: standard_layout(),
        vertex_data: Arc::from(vertex_data.as_slice()),
        index_data: Arc::from(index_data.as_slice()),
        index_format: IndexFormat::Uint16,
        local_bounds: BoundingSphere {
            center: Vec3::ZERO,
            radius: 1.0,
        },
    }
}

/// Create a regular icosahedron inscribed in the unit sphere.
///
/// All 12 vertices lie on the unit sphere, derived from the golden ratio.
/// Normals equal positions (smooth shading). UVs use spherical mapping.
///
/// 12 vertices, 60 indices (20 triangles).
pub fn create_icosahedron() -> MeshAsset {
    let golden_ratio = 0.5 * (1.0 + 5.0_f32.sqrt());
    let inv_root = 1.0 / (1.0 + golden_ratio * golden_ratio).sqrt();
    let u = golden_ratio * inv_root;
    let v = inv_root;

    #[rustfmt::skip]
    let positions: [[f32; 3]; 12] = [
        [ u,  v,  0.0],  //  0
        [-u,  v,  0.0],  //  1
        [ u, -v,  0.0],  //  2
        [-u, -v,  0.0],  //  3
        [ v,  0.0,  u],  //  4
        [ v,  0.0, -u],  //  5
        [-v,  0.0,  u],  //  6
        [-v,  0.0, -u],  //  7
        [ 0.0,  u,  v],  //  8
        [ 0.0, -u,  v],  //  9
        [ 0.0,  u, -v],  // 10
        [ 0.0, -u, -v],  // 11
    ];

    let mut vertex_data: Vec<u8> = Vec::with_capacity(12 * STRIDE as usize);
    for pos in &positions {
        push_vertex(&mut vertex_data, *pos, *pos, platonic_uv(*pos));
    }

    // 20 triangles — CCW outside view (matches GTE)
    #[rustfmt::skip]
    let triangles: [[u16; 3]; 20] = [
        [ 0,  8,  4], [ 0,  5, 10],
        [ 2,  4,  9], [ 2, 11,  5],
        [ 1,  6,  8], [ 1, 10,  7],
        [ 3,  9,  6], [ 3,  7, 11],
        [ 0, 10,  8], [ 1,  8, 10],
        [ 2,  9, 11], [ 3, 11,  9],
        [ 4,  2,  0], [ 5,  0,  2],
        [ 6,  1,  3], [ 7,  3,  1],
        [ 8,  6,  4], [ 9,  4,  6],
        [10,  5,  7], [11,  7,  5],
    ];

    let mut index_data: Vec<u8> = Vec::with_capacity(60 * 2);
    for tri in &triangles {
        push_u16(&mut index_data, tri[0]);
        push_u16(&mut index_data, tri[1]);
        push_u16(&mut index_data, tri[2]);
    }

    MeshAsset {
        vertex_layout: standard_layout(),
        vertex_data: Arc::from(vertex_data.as_slice()),
        index_data: Arc::from(index_data.as_slice()),
        index_format: IndexFormat::Uint16,
        local_bounds: BoundingSphere {
            center: Vec3::ZERO,
            radius: 1.0,
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

    // -----------------------------------------------------------------------
    // Decode helpers
    // -----------------------------------------------------------------------

    fn decode_positions(vertex_data: &[u8], vertex_count: usize) -> Vec<[f32; 3]> {
        (0..vertex_count)
            .map(|i| {
                let base = i * STRIDE as usize;
                let x = f32::from_le_bytes(vertex_data[base..base + 4].try_into().unwrap());
                let y = f32::from_le_bytes(vertex_data[base + 4..base + 8].try_into().unwrap());
                let z = f32::from_le_bytes(vertex_data[base + 8..base + 12].try_into().unwrap());
                [x, y, z]
            })
            .collect()
    }

    fn decode_normals(vertex_data: &[u8], vertex_count: usize) -> Vec<[f32; 3]> {
        (0..vertex_count)
            .map(|i| {
                let base = i * STRIDE as usize + 12;
                let x = f32::from_le_bytes(vertex_data[base..base + 4].try_into().unwrap());
                let y = f32::from_le_bytes(vertex_data[base + 4..base + 8].try_into().unwrap());
                let z = f32::from_le_bytes(vertex_data[base + 8..base + 12].try_into().unwrap());
                [x, y, z]
            })
            .collect()
    }

    fn decode_uvs(vertex_data: &[u8], vertex_count: usize) -> Vec<[f32; 2]> {
        (0..vertex_count)
            .map(|i| {
                let base = i * STRIDE as usize + 24;
                let u = f32::from_le_bytes(vertex_data[base..base + 4].try_into().unwrap());
                let v = f32::from_le_bytes(vertex_data[base + 4..base + 8].try_into().unwrap());
                [u, v]
            })
            .collect()
    }

    fn decode_u16_indices(index_data: &[u8]) -> Vec<u16> {
        index_data
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    fn decode_indices(mesh: &MeshAsset) -> Vec<u32> {
        match mesh.index_format {
            IndexFormat::Uint16 => mesh
                .index_data
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes(c.try_into().unwrap()) as u32)
                .collect(),
            IndexFormat::Uint32 => mesh
                .index_data
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                .collect(),
        }
    }

    fn vec3_len(v: [f32; 3]) -> f32 {
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    }

    // -----------------------------------------------------------------------
    // platonic_uv helper tests
    // -----------------------------------------------------------------------

    #[test]
    fn platonic_uv_at_north_pole() {
        let uv = platonic_uv([0.0, 0.0, 1.0]);
        assert!((uv[0] - 0.5).abs() < 1e-5, "u should be 0.5 at north pole");
        assert!(uv[1].abs() < 1e-5, "v should be 0.0 at north pole");
    }

    #[test]
    fn platonic_uv_at_south_pole() {
        let uv = platonic_uv([0.0, 0.0, -1.0]);
        assert!((uv[0] - 0.5).abs() < 1e-5, "u should be 0.5 at south pole");
        assert!((uv[1] - 1.0).abs() < 1e-5, "v should be 1.0 at south pole");
    }

    #[test]
    fn platonic_uv_at_equator_positive_x() {
        // (1, 0, 0): atan2(0, 1) = 0, so u = 0.5*(1+0) = 0.5; acos(0)/π = 0.5
        let uv = platonic_uv([1.0, 0.0, 0.0]);
        assert!((uv[0] - 0.5).abs() < 1e-5, "u={}", uv[0]);
        assert!((uv[1] - 0.5).abs() < 1e-5, "v={}", uv[1]);
    }

    #[test]
    fn platonic_uv_values_are_in_unit_range() {
        let test_points: [[f32; 3]; 6] = [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        for p in &test_points {
            let uv = platonic_uv(*p);
            assert!(
                uv[0] >= 0.0 && uv[0] <= 1.0,
                "u={} out of [0,1] for {:?}",
                uv[0],
                p
            );
            assert!(
                uv[1] >= 0.0 && uv[1] <= 1.0,
                "v={} out of [0,1] for {:?}",
                uv[1],
                p
            );
        }
    }

    // -----------------------------------------------------------------------
    // Shared assertion helpers for platonic solids
    // -----------------------------------------------------------------------

    fn assert_vertices_on_unit_sphere(vertex_data: &[u8], vertex_count: usize) {
        let positions = decode_positions(vertex_data, vertex_count);
        for (i, pos) in positions.iter().enumerate() {
            let len = vec3_len(*pos);
            assert!(
                (len - 1.0).abs() < 1e-5,
                "vertex {i} length {len} not on unit sphere"
            );
        }
    }

    fn assert_normals_unit_length(vertex_data: &[u8], vertex_count: usize) {
        let normals = decode_normals(vertex_data, vertex_count);
        for (i, n) in normals.iter().enumerate() {
            let len = vec3_len(*n);
            assert!((len - 1.0).abs() < 1e-5, "normal {i} length {len} not unit");
        }
    }

    fn assert_normals_equal_positions(vertex_data: &[u8], vertex_count: usize) {
        let positions = decode_positions(vertex_data, vertex_count);
        let normals = decode_normals(vertex_data, vertex_count);
        for (i, (pos, nor)) in positions.iter().zip(normals.iter()).enumerate() {
            for axis in 0..3 {
                assert!(
                    (pos[axis] - nor[axis]).abs() < 1e-5,
                    "vertex {i} axis {axis}: pos={} nor={}",
                    pos[axis],
                    nor[axis]
                );
            }
        }
    }

    fn assert_indices_in_range(index_data: &[u8], vertex_count: usize) {
        let indices = decode_u16_indices(index_data);
        for &idx in &indices {
            assert!(
                (idx as usize) < vertex_count,
                "index {idx} >= vertex count {vertex_count}"
            );
        }
    }

    fn assert_uvs_in_unit_range(vertex_data: &[u8], vertex_count: usize) {
        let uvs = decode_uvs(vertex_data, vertex_count);
        for (i, uv) in uvs.iter().enumerate() {
            assert!(
                uv[0] >= 0.0 && uv[0] <= 1.0,
                "vertex {i} u={} out of [0,1]",
                uv[0]
            );
            assert!(
                uv[1] >= 0.0 && uv[1] <= 1.0,
                "vertex {i} v={} out of [0,1]",
                uv[1]
            );
        }
    }

    fn assert_winding_matches_normals(mesh: &MeshAsset) {
        let vertex_count = mesh.vertex_data.len() / STRIDE as usize;
        let positions = decode_positions(&mesh.vertex_data, vertex_count);
        let normals = decode_normals(&mesh.vertex_data, vertex_count);
        let indices = decode_indices(mesh);

        for triangle in indices.chunks_exact(3) {
            let i0 = triangle[0] as usize;
            let i1 = triangle[1] as usize;
            let i2 = triangle[2] as usize;
            let p0 = Vec3::from_array(positions[i0]);
            let p1 = Vec3::from_array(positions[i1]);
            let p2 = Vec3::from_array(positions[i2]);
            let declared_normal = Vec3::from_array(normals[i0]);
            let geometric_normal = (p1 - p0).cross(p2 - p0);

            if geometric_normal.length_squared() <= 1e-12 {
                continue;
            }

            assert!(
                geometric_normal.dot(declared_normal) > 0.0,
                "triangle {:?} winding is opposite declared normal {:?}",
                triangle,
                declared_normal
            );
        }
    }

    fn assert_unit_sphere_bounding(mesh: &MeshAsset) {
        assert_eq!(mesh.local_bounds.center, Vec3::ZERO);
        assert!(
            (mesh.local_bounds.radius - 1.0).abs() < 1e-5,
            "radius={}",
            mesh.local_bounds.radius
        );
    }

    // -----------------------------------------------------------------------
    // Tetrahedron tests
    // -----------------------------------------------------------------------

    #[test]
    fn create_tetrahedron_vertex_and_index_counts() {
        let mesh = create_tetrahedron();
        let vertex_count = mesh.vertex_data.len() / STRIDE as usize;
        let index_count = mesh.index_data.len() / 2;
        assert_eq!(vertex_count, 4);
        assert_eq!(index_count, 12);
    }

    #[test]
    fn create_tetrahedron_vertices_on_unit_sphere() {
        let mesh = create_tetrahedron();
        assert_vertices_on_unit_sphere(&mesh.vertex_data, 4);
    }

    #[test]
    fn create_tetrahedron_normals_are_unit_length() {
        let mesh = create_tetrahedron();
        assert_normals_unit_length(&mesh.vertex_data, 4);
    }

    #[test]
    fn create_tetrahedron_normals_equal_positions() {
        let mesh = create_tetrahedron();
        assert_normals_equal_positions(&mesh.vertex_data, 4);
    }

    #[test]
    fn create_tetrahedron_indices_in_range() {
        let mesh = create_tetrahedron();
        assert_indices_in_range(&mesh.index_data, 4);
    }

    #[test]
    fn create_tetrahedron_winding_matches_normals() {
        let mesh = create_tetrahedron();
        assert_winding_matches_normals(&mesh);
    }

    #[test]
    fn create_tetrahedron_bounding_sphere() {
        let mesh = create_tetrahedron();
        assert_unit_sphere_bounding(&mesh);
    }

    #[test]
    fn create_tetrahedron_uses_uint16() {
        let mesh = create_tetrahedron();
        assert_eq!(mesh.index_format, IndexFormat::Uint16);
    }

    #[test]
    fn create_tetrahedron_uvs_in_unit_range() {
        let mesh = create_tetrahedron();
        assert_uvs_in_unit_range(&mesh.vertex_data, 4);
    }

    // -----------------------------------------------------------------------
    // Hexahedron tests
    // -----------------------------------------------------------------------

    #[test]
    fn create_hexahedron_vertex_and_index_counts() {
        let mesh = create_hexahedron();
        let vertex_count = mesh.vertex_data.len() / STRIDE as usize;
        let index_count = mesh.index_data.len() / 2;
        assert_eq!(vertex_count, 8);
        assert_eq!(index_count, 36);
    }

    #[test]
    fn create_hexahedron_vertices_on_unit_sphere() {
        let mesh = create_hexahedron();
        assert_vertices_on_unit_sphere(&mesh.vertex_data, 8);
    }

    #[test]
    fn create_hexahedron_normals_are_unit_length() {
        let mesh = create_hexahedron();
        assert_normals_unit_length(&mesh.vertex_data, 8);
    }

    #[test]
    fn create_hexahedron_normals_equal_positions() {
        let mesh = create_hexahedron();
        assert_normals_equal_positions(&mesh.vertex_data, 8);
    }

    #[test]
    fn create_hexahedron_indices_in_range() {
        let mesh = create_hexahedron();
        assert_indices_in_range(&mesh.index_data, 8);
    }

    #[test]
    fn create_hexahedron_winding_matches_normals() {
        let mesh = create_hexahedron();
        assert_winding_matches_normals(&mesh);
    }

    #[test]
    fn create_hexahedron_bounding_sphere() {
        let mesh = create_hexahedron();
        assert_unit_sphere_bounding(&mesh);
    }

    #[test]
    fn create_hexahedron_uses_uint16() {
        let mesh = create_hexahedron();
        assert_eq!(mesh.index_format, IndexFormat::Uint16);
    }

    #[test]
    fn create_hexahedron_uvs_in_unit_range() {
        let mesh = create_hexahedron();
        assert_uvs_in_unit_range(&mesh.vertex_data, 8);
    }

    // -----------------------------------------------------------------------
    // Octahedron tests
    // -----------------------------------------------------------------------

    #[test]
    fn create_octahedron_vertex_and_index_counts() {
        let mesh = create_octahedron();
        let vertex_count = mesh.vertex_data.len() / STRIDE as usize;
        let index_count = mesh.index_data.len() / 2;
        assert_eq!(vertex_count, 6);
        assert_eq!(index_count, 24);
    }

    #[test]
    fn create_octahedron_vertices_on_unit_sphere() {
        let mesh = create_octahedron();
        assert_vertices_on_unit_sphere(&mesh.vertex_data, 6);
    }

    #[test]
    fn create_octahedron_normals_are_unit_length() {
        let mesh = create_octahedron();
        assert_normals_unit_length(&mesh.vertex_data, 6);
    }

    #[test]
    fn create_octahedron_normals_equal_positions() {
        let mesh = create_octahedron();
        assert_normals_equal_positions(&mesh.vertex_data, 6);
    }

    #[test]
    fn create_octahedron_indices_in_range() {
        let mesh = create_octahedron();
        assert_indices_in_range(&mesh.index_data, 6);
    }

    #[test]
    fn create_octahedron_winding_matches_normals() {
        let mesh = create_octahedron();
        assert_winding_matches_normals(&mesh);
    }

    #[test]
    fn create_octahedron_bounding_sphere() {
        let mesh = create_octahedron();
        assert_unit_sphere_bounding(&mesh);
    }

    #[test]
    fn create_octahedron_uses_uint16() {
        let mesh = create_octahedron();
        assert_eq!(mesh.index_format, IndexFormat::Uint16);
    }

    #[test]
    fn create_octahedron_uvs_in_unit_range() {
        let mesh = create_octahedron();
        assert_uvs_in_unit_range(&mesh.vertex_data, 6);
    }

    // -----------------------------------------------------------------------
    // Dodecahedron tests
    // -----------------------------------------------------------------------

    #[test]
    fn create_dodecahedron_vertex_and_index_counts() {
        let mesh = create_dodecahedron();
        let vertex_count = mesh.vertex_data.len() / STRIDE as usize;
        let index_count = mesh.index_data.len() / 2;
        assert_eq!(vertex_count, 20);
        assert_eq!(index_count, 108);
    }

    #[test]
    fn create_dodecahedron_vertices_on_unit_sphere() {
        let mesh = create_dodecahedron();
        assert_vertices_on_unit_sphere(&mesh.vertex_data, 20);
    }

    #[test]
    fn create_dodecahedron_normals_are_unit_length() {
        let mesh = create_dodecahedron();
        assert_normals_unit_length(&mesh.vertex_data, 20);
    }

    #[test]
    fn create_dodecahedron_normals_equal_positions() {
        let mesh = create_dodecahedron();
        assert_normals_equal_positions(&mesh.vertex_data, 20);
    }

    #[test]
    fn create_dodecahedron_indices_in_range() {
        let mesh = create_dodecahedron();
        assert_indices_in_range(&mesh.index_data, 20);
    }

    #[test]
    fn create_dodecahedron_winding_matches_normals() {
        let mesh = create_dodecahedron();
        assert_winding_matches_normals(&mesh);
    }

    #[test]
    fn create_dodecahedron_bounding_sphere() {
        let mesh = create_dodecahedron();
        assert_unit_sphere_bounding(&mesh);
    }

    #[test]
    fn create_dodecahedron_uses_uint16() {
        let mesh = create_dodecahedron();
        assert_eq!(mesh.index_format, IndexFormat::Uint16);
    }

    #[test]
    fn create_dodecahedron_uvs_in_unit_range() {
        let mesh = create_dodecahedron();
        assert_uvs_in_unit_range(&mesh.vertex_data, 20);
    }

    // -----------------------------------------------------------------------
    // Icosahedron tests
    // -----------------------------------------------------------------------

    #[test]
    fn create_icosahedron_vertex_and_index_counts() {
        let mesh = create_icosahedron();
        let vertex_count = mesh.vertex_data.len() / STRIDE as usize;
        let index_count = mesh.index_data.len() / 2;
        assert_eq!(vertex_count, 12);
        assert_eq!(index_count, 60);
    }

    #[test]
    fn create_icosahedron_vertices_on_unit_sphere() {
        let mesh = create_icosahedron();
        assert_vertices_on_unit_sphere(&mesh.vertex_data, 12);
    }

    #[test]
    fn create_icosahedron_normals_are_unit_length() {
        let mesh = create_icosahedron();
        assert_normals_unit_length(&mesh.vertex_data, 12);
    }

    #[test]
    fn create_icosahedron_normals_equal_positions() {
        let mesh = create_icosahedron();
        assert_normals_equal_positions(&mesh.vertex_data, 12);
    }

    #[test]
    fn create_icosahedron_indices_in_range() {
        let mesh = create_icosahedron();
        assert_indices_in_range(&mesh.index_data, 12);
    }

    #[test]
    fn create_icosahedron_winding_matches_normals() {
        let mesh = create_icosahedron();
        assert_winding_matches_normals(&mesh);
    }

    #[test]
    fn create_icosahedron_bounding_sphere() {
        let mesh = create_icosahedron();
        assert_unit_sphere_bounding(&mesh);
    }

    #[test]
    fn create_icosahedron_uses_uint16() {
        let mesh = create_icosahedron();
        assert_eq!(mesh.index_format, IndexFormat::Uint16);
    }

    #[test]
    fn create_icosahedron_uvs_in_unit_range() {
        let mesh = create_icosahedron();
        assert_uvs_in_unit_range(&mesh.vertex_data, 12);
    }

    // -----------------------------------------------------------------------
    // Existing parametric shape tests (unchanged)
    // -----------------------------------------------------------------------

    #[test]
    fn create_sphere_uses_uint32_when_vertex_count_exceeds_u16_max() {
        // Need (slices+1)*(stacks+1) > 65535. 256×256 = 66049 vertices.
        let mesh = create_sphere(1.0, 256, 256);
        assert_eq!(mesh.index_format, IndexFormat::Uint32);
        // Sanity: index buffer entries are 4 bytes each, 6 per quad cell.
        let expected_index_bytes = 6 * 256 * 256 * 4;
        assert_eq!(mesh.index_data.len(), expected_index_bytes);
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
            let len = vec3_len(*normal);
            assert!((len - 1.0).abs() < 1e-5, "normal length was {len}");
        }
    }

    #[test]
    fn create_sphere_indices_stay_in_range() {
        let mesh = create_sphere(1.0, 8, 6);
        let vertex_count = (mesh.vertex_data.len() / STRIDE as usize) as u16;
        let indices = decode_u16_indices(&mesh.index_data);
        for &idx in &indices {
            assert!(
                idx < vertex_count,
                "index {idx} out of range (max {vertex_count})"
            );
        }
    }

    #[test]
    fn create_sphere_winding_matches_normals() {
        let mesh = create_sphere(1.0, 16, 8);
        assert_winding_matches_normals(&mesh);
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
    fn create_plane_winding_matches_up_normal() {
        let mesh = create_plane(2.0, 3.0);
        assert_winding_matches_normals(&mesh);
    }

    #[test]
    fn all_mesh_factory_layouts_pass_standard_layout() {
        let meshes = [
            create_box(1.0, 1.0, 1.0),
            create_sphere(1.0, 6, 4),
            create_plane(1.0, 1.0),
            create_tetrahedron(),
            create_hexahedron(),
            create_octahedron(),
            create_dodecahedron(),
            create_icosahedron(),
        ];

        for mesh in &meshes {
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
    fn create_box_winding_matches_face_normals() {
        let mesh = create_box(1.0, 2.0, 3.0);
        assert_winding_matches_normals(&mesh);
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
