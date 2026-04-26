//! CPU Marching Cubes isosurface extraction.
//!
//! Extracts a triangle mesh from a scalar field using the Marching Cubes algorithm
//! (Lorensen & Cline, 1987).  Output uses the framework's `standard_layout()`:
//!
//! ```text
//! Position: Float32x3  @ location 0, offset  0
//! Normal:   Float32x3  @ location 1, offset 12
//! UV:       Float32x2  @ location 2, offset 24
//! stride = 32 bytes
//! ```
//!
//! Normals are computed via central differences of the scalar field — smooth Phong
//! shading without a separate normal-averaging pass.
//!
//! Reference: [paulbourke.net/geometry/polygonise/](http://paulbourke.net/geometry/polygonise/)

use std::collections::HashMap;

use rig_math::{BoundingSphere, Vec3};

use crate::DynamicMeshData;

// ---------------------------------------------------------------------------
// Paul Bourke lookup tables
// ---------------------------------------------------------------------------

/// For each of the 256 cube configurations, a bitmask of which of the 12 edges
/// are intersected by the isosurface.
#[rustfmt::skip]
const EDGE_TABLE: [u16; 256] = [
    0x000, 0x109, 0x203, 0x30a, 0x406, 0x50f, 0x605, 0x70c,
    0x80c, 0x905, 0xa0f, 0xb06, 0xc0a, 0xd03, 0xe09, 0xf00,
    0x190, 0x099, 0x393, 0x29a, 0x596, 0x49f, 0x795, 0x69c,
    0x99c, 0x895, 0xb9f, 0xa96, 0xd9a, 0xc93, 0xf99, 0xe90,
    0x230, 0x339, 0x033, 0x13a, 0x636, 0x73f, 0x435, 0x53c,
    0xa3c, 0xb35, 0x83f, 0x936, 0xe3a, 0xf33, 0xc39, 0xd30,
    0x3a0, 0x2a9, 0x1a3, 0x0aa, 0x7a6, 0x6af, 0x5a5, 0x4ac,
    0xbac, 0xaa5, 0x9af, 0x8a6, 0xfaa, 0xea3, 0xda9, 0xca0,
    0x460, 0x569, 0x663, 0x76a, 0x066, 0x16f, 0x265, 0x36c,
    0xc6c, 0xd65, 0xe6f, 0xf66, 0x86a, 0x963, 0xa69, 0xb60,
    0x5f0, 0x4f9, 0x7f3, 0x6fa, 0x1f6, 0x0ff, 0x3f5, 0x2fc,
    0xdfc, 0xcf5, 0xfff, 0xef6, 0x9fa, 0x8f3, 0xbf9, 0xaf0,
    0x650, 0x759, 0x453, 0x55a, 0x256, 0x35f, 0x055, 0x15c,
    0xe5c, 0xf55, 0xc5f, 0xd56, 0xa5a, 0xb53, 0x859, 0x950,
    0x7c0, 0x6c9, 0x5c3, 0x4ca, 0x3c6, 0x2cf, 0x1c5, 0x0cc,
    0xfcc, 0xec5, 0xdcf, 0xcc6, 0xbca, 0xac3, 0x9c9, 0x8c0,
    0x8c0, 0x9c9, 0xac3, 0xbca, 0xcc6, 0xdcf, 0xec5, 0xfcc,
    0x0cc, 0x1c5, 0x2cf, 0x3c6, 0x4ca, 0x5c3, 0x6c9, 0x7c0,
    0x950, 0x859, 0xb53, 0xa5a, 0xd56, 0xc5f, 0xf55, 0xe5c,
    0x15c, 0x055, 0x35f, 0x256, 0x55a, 0x453, 0x759, 0x650,
    0xaf0, 0xbf9, 0x8f3, 0x9fa, 0xef6, 0xfff, 0xcf5, 0xdfc,
    0x2fc, 0x3f5, 0x0ff, 0x1f6, 0x6fa, 0x7f3, 0x4f9, 0x5f0,
    0xb60, 0xa69, 0x963, 0x86a, 0xf66, 0xe6f, 0xd65, 0xc6c,
    0x36c, 0x265, 0x16f, 0x066, 0x76a, 0x663, 0x569, 0x460,
    0xca0, 0xda9, 0xea3, 0xfaa, 0x8a6, 0x9af, 0xaa5, 0xbac,
    0x4ac, 0x5a5, 0x6af, 0x7a6, 0x0aa, 0x1a3, 0x2a9, 0x3a0,
    0xd30, 0xc39, 0xf33, 0xe3a, 0x936, 0x83f, 0xb35, 0xa3c,
    0x53c, 0x435, 0x73f, 0x636, 0x13a, 0x033, 0x339, 0x230,
    0xe90, 0xf99, 0xc93, 0xd9a, 0xa96, 0xb9f, 0x895, 0x99c,
    0x69c, 0x795, 0x49f, 0x596, 0x29a, 0x393, 0x099, 0x190,
    0xf00, 0xe09, 0xd03, 0xc0a, 0xb06, 0xa0f, 0x905, 0x80c,
    0x70c, 0x605, 0x50f, 0x406, 0x30a, 0x203, 0x109, 0x000,
];

/// For each of the 256 cube configurations, a list of triangle edge triplets.
/// Each group of 3 values is one triangle (indices into the 12 cell edges).
/// Terminated by -1.
#[rustfmt::skip]
const TRI_TABLE: [[i8; 16]; 256] = [
    [-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 8, 3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 1, 9,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 8, 3, 9, 8, 1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 2,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 8, 3, 1, 2,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 2,10, 0, 2, 9,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 2, 8, 3, 2,10, 8,10, 9, 8,-1,-1,-1,-1,-1,-1,-1],
    [ 3,11, 2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0,11, 2, 8,11, 0,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 9, 0, 2, 3,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 1,11, 2, 1, 9,11, 9, 8,11,-1,-1,-1,-1,-1,-1,-1],
    [ 3,10, 1,11,10, 3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0,10, 1, 0, 8,10, 8,11,10,-1,-1,-1,-1,-1,-1,-1],
    [ 3, 9, 0, 3,11, 9,11,10, 9,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 8,10,10, 8,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 4, 7, 8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 4, 3, 0, 7, 3, 4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 1, 9, 8, 4, 7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 4, 1, 9, 4, 7, 1, 7, 3, 1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 2,10, 8, 4, 7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 3, 4, 7, 3, 0, 4, 1, 2,10,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 2,10, 9, 0, 2, 8, 4, 7,-1,-1,-1,-1,-1,-1,-1],
    [ 2,10, 9, 2, 9, 7, 2, 7, 3, 7, 9, 4,-1,-1,-1,-1],
    [ 8, 4, 7, 3,11, 2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [11, 4, 7,11, 2, 4, 2, 0, 4,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 0, 1, 8, 4, 7, 2, 3,11,-1,-1,-1,-1,-1,-1,-1],
    [ 4, 7,11, 9, 4,11, 9,11, 2, 9, 2, 1,-1,-1,-1,-1],
    [ 3,10, 1, 3,11,10, 7, 8, 4,-1,-1,-1,-1,-1,-1,-1],
    [ 1,11,10, 1, 4,11, 1, 0, 4, 7,11, 4,-1,-1,-1,-1],
    [ 4, 7, 8, 9, 0,11, 9,11,10,11, 0, 3,-1,-1,-1,-1],
    [ 4, 7,11, 4,11, 9, 9,11,10,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 5, 4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 5, 4, 0, 8, 3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 5, 4, 1, 5, 0,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 8, 5, 4, 8, 3, 5, 3, 1, 5,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 2,10, 9, 5, 4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 3, 0, 8, 1, 2,10, 4, 9, 5,-1,-1,-1,-1,-1,-1,-1],
    [ 5, 2,10, 5, 4, 2, 4, 0, 2,-1,-1,-1,-1,-1,-1,-1],
    [ 2,10, 5, 3, 2, 5, 3, 5, 4, 3, 4, 8,-1,-1,-1,-1],
    [ 9, 5, 4, 2, 3,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0,11, 2, 0, 8,11, 4, 9, 5,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 5, 4, 0, 1, 5, 2, 3,11,-1,-1,-1,-1,-1,-1,-1],
    [ 2, 1, 5, 2, 5, 8, 2, 8,11, 4, 8, 5,-1,-1,-1,-1],
    [10, 3,11,10, 1, 3, 9, 5, 4,-1,-1,-1,-1,-1,-1,-1],
    [ 4, 9, 5, 0, 8, 1, 8,10, 1, 8,11,10,-1,-1,-1,-1],
    [ 5, 4, 0, 5, 0,11, 5,11,10,11, 0, 3,-1,-1,-1,-1],
    [ 5, 4, 8, 5, 8,10,10, 8,11,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 7, 8, 5, 7, 9,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 3, 0, 9, 5, 3, 5, 7, 3,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 7, 8, 0, 1, 7, 1, 5, 7,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 5, 3, 3, 5, 7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 7, 8, 9, 5, 7,10, 1, 2,-1,-1,-1,-1,-1,-1,-1],
    [10, 1, 2, 9, 5, 0, 5, 3, 0, 5, 7, 3,-1,-1,-1,-1],
    [ 8, 0, 2, 8, 2, 5, 8, 5, 7,10, 5, 2,-1,-1,-1,-1],
    [ 2,10, 5, 2, 5, 3, 3, 5, 7,-1,-1,-1,-1,-1,-1,-1],
    [ 7, 9, 5, 7, 8, 9, 3,11, 2,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 5, 7, 9, 7, 2, 9, 2, 0, 2, 7,11,-1,-1,-1,-1],
    [ 2, 3,11, 0, 1, 8, 1, 7, 8, 1, 5, 7,-1,-1,-1,-1],
    [11, 2, 1,11, 1, 7, 7, 1, 5,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 5, 8, 8, 5, 7,10, 1, 3,10, 3,11,-1,-1,-1,-1],
    [ 5, 7, 0, 5, 0, 9, 7,11, 0, 1, 0,10,11,10, 0,-1],
    [11,10, 0,11, 0, 3,10, 5, 0, 8, 0, 7, 5, 7, 0,-1],
    [11,10, 5, 7,11, 5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [10, 6, 5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 8, 3, 5,10, 6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 0, 1, 5,10, 6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 8, 3, 1, 9, 8, 5,10, 6,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 6, 5, 2, 6, 1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 6, 5, 1, 2, 6, 3, 0, 8,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 6, 5, 9, 0, 6, 0, 2, 6,-1,-1,-1,-1,-1,-1,-1],
    [ 5, 9, 8, 5, 8, 2, 5, 2, 6, 3, 2, 8,-1,-1,-1,-1],
    [ 2, 3,11,10, 6, 5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [11, 0, 8,11, 2, 0,10, 6, 5,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 1, 9, 2, 3,11, 5,10, 6,-1,-1,-1,-1,-1,-1,-1],
    [ 5,10, 6, 1, 9, 2, 9,11, 2, 9, 8,11,-1,-1,-1,-1],
    [ 6, 3,11, 6, 5, 3, 5, 1, 3,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 8,11, 0,11, 5, 0, 5, 1, 5,11, 6,-1,-1,-1,-1],
    [ 3,11, 6, 0, 3, 6, 0, 6, 5, 0, 5, 9,-1,-1,-1,-1],
    [ 6, 5, 9, 6, 9,11,11, 9, 8,-1,-1,-1,-1,-1,-1,-1],
    [ 5,10, 6, 4, 7, 8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 4, 3, 0, 4, 7, 3, 6, 5,10,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 9, 0, 5,10, 6, 8, 4, 7,-1,-1,-1,-1,-1,-1,-1],
    [10, 6, 5, 1, 9, 7, 1, 7, 3, 7, 9, 4,-1,-1,-1,-1],
    [ 6, 1, 2, 6, 5, 1, 4, 7, 8,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 2, 5, 5, 2, 6, 3, 0, 4, 3, 4, 7,-1,-1,-1,-1],
    [ 8, 4, 7, 9, 0, 5, 0, 6, 5, 0, 2, 6,-1,-1,-1,-1],
    [ 7, 3, 9, 7, 9, 4, 3, 2, 9, 5, 9, 6, 2, 6, 9,-1],
    [ 3,11, 2, 7, 8, 4,10, 6, 5,-1,-1,-1,-1,-1,-1,-1],
    [ 5,10, 6, 4, 7, 2, 4, 2, 0, 2, 7,11,-1,-1,-1,-1],
    [ 0, 1, 9, 4, 7, 8, 2, 3,11, 5,10, 6,-1,-1,-1,-1],
    [ 9, 2, 1, 9,11, 2, 9, 4,11, 7,11, 4, 5,10, 6,-1],
    [ 8, 4, 7, 3,11, 5, 3, 5, 1, 5,11, 6,-1,-1,-1,-1],
    [ 5, 1,11, 5,11, 6, 1, 0,11, 7,11, 4, 0, 4,11,-1],
    [ 0, 5, 9, 0, 6, 5, 0, 3, 6,11, 6, 3, 8, 4, 7,-1],
    [ 6, 5, 9, 6, 9,11, 4, 7, 9, 7,11, 9,-1,-1,-1,-1],
    [10, 4, 9, 6, 4,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 4,10, 6, 4, 9,10, 0, 8, 3,-1,-1,-1,-1,-1,-1,-1],
    [10, 0, 1,10, 6, 0, 6, 4, 0,-1,-1,-1,-1,-1,-1,-1],
    [ 8, 3, 1, 8, 1, 6, 8, 6, 4, 6, 1,10,-1,-1,-1,-1],
    [ 1, 4, 9, 1, 2, 4, 2, 6, 4,-1,-1,-1,-1,-1,-1,-1],
    [ 3, 0, 8, 1, 2, 9, 2, 4, 9, 2, 6, 4,-1,-1,-1,-1],
    [ 0, 2, 4, 4, 2, 6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 8, 3, 2, 8, 2, 4, 4, 2, 6,-1,-1,-1,-1,-1,-1,-1],
    [10, 4, 9,10, 6, 4,11, 2, 3,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 8, 2, 2, 8,11, 4, 9,10, 4,10, 6,-1,-1,-1,-1],
    [ 3,11, 2, 0, 1, 6, 0, 6, 4, 6, 1,10,-1,-1,-1,-1],
    [ 6, 4, 1, 6, 1,10, 4, 8, 1, 2, 1,11, 8,11, 1,-1],
    [ 9, 6, 4, 9, 3, 6, 9, 1, 3,11, 6, 3,-1,-1,-1,-1],
    [ 8,11, 1, 8, 1, 0,11, 6, 1, 9, 1, 4, 6, 4, 1,-1],
    [ 3,11, 6, 3, 6, 0, 0, 6, 4,-1,-1,-1,-1,-1,-1,-1],
    [ 6, 4, 8,11, 6, 8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 7,10, 6, 7, 8,10, 8, 9,10,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 7, 3, 0,10, 7, 0, 9,10, 6, 7,10,-1,-1,-1,-1],
    [10, 6, 7, 1,10, 7, 1, 7, 8, 1, 8, 0,-1,-1,-1,-1],
    [10, 6, 7,10, 7, 1, 1, 7, 3,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 2, 6, 1, 6, 8, 1, 8, 9, 8, 6, 7,-1,-1,-1,-1],
    [ 2, 6, 9, 2, 9, 1, 6, 7, 9, 0, 9, 3, 7, 3, 9,-1],
    [ 7, 8, 0, 7, 0, 6, 6, 0, 2,-1,-1,-1,-1,-1,-1,-1],
    [ 7, 3, 2, 6, 7, 2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 2, 3,11,10, 6, 8,10, 8, 9, 8, 6, 7,-1,-1,-1,-1],
    [ 2, 0, 7, 2, 7,11, 0, 9, 7, 6, 7,10, 9,10, 7,-1],
    [ 1, 8, 0, 1, 7, 8, 1,10, 7, 6, 7,10, 2, 3,11,-1],
    [11, 2, 1,11, 1, 7,10, 6, 1, 6, 7, 1,-1,-1,-1,-1],
    [ 8, 9, 6, 8, 6, 7, 9, 1, 6,11, 6, 3, 1, 3, 6,-1],
    [ 0, 9, 1,11, 6, 7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 7, 8, 0, 7, 0, 6, 3,11, 0,11, 6, 0,-1,-1,-1,-1],
    [ 7,11, 6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 7, 6,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 3, 0, 8,11, 7, 6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 1, 9,11, 7, 6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 8, 1, 9, 8, 3, 1,11, 7, 6,-1,-1,-1,-1,-1,-1,-1],
    [10, 1, 2, 6,11, 7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 2,10, 3, 0, 8, 6,11, 7,-1,-1,-1,-1,-1,-1,-1],
    [ 2, 9, 0, 2,10, 9, 6,11, 7,-1,-1,-1,-1,-1,-1,-1],
    [ 6,11, 7, 2,10, 3,10, 8, 3,10, 9, 8,-1,-1,-1,-1],
    [ 7, 2, 3, 6, 2, 7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 7, 0, 8, 7, 6, 0, 6, 2, 0,-1,-1,-1,-1,-1,-1,-1],
    [ 2, 7, 6, 2, 3, 7, 0, 1, 9,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 6, 2, 1, 8, 6, 1, 9, 8, 8, 7, 6,-1,-1,-1,-1],
    [10, 7, 6,10, 1, 7, 1, 3, 7,-1,-1,-1,-1,-1,-1,-1],
    [10, 7, 6, 1, 7,10, 1, 8, 7, 1, 0, 8,-1,-1,-1,-1],
    [ 0, 3, 7, 0, 7,10, 0,10, 9, 6,10, 7,-1,-1,-1,-1],
    [ 7, 6,10, 7,10, 8, 8,10, 9,-1,-1,-1,-1,-1,-1,-1],
    [ 6, 8, 4,11, 8, 6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 3, 6,11, 3, 0, 6, 0, 4, 6,-1,-1,-1,-1,-1,-1,-1],
    [ 8, 6,11, 8, 4, 6, 9, 0, 1,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 4, 6, 9, 6, 3, 9, 3, 1,11, 3, 6,-1,-1,-1,-1],
    [ 6, 8, 4, 6,11, 8, 2,10, 1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 2,10, 3, 0,11, 0, 6,11, 0, 4, 6,-1,-1,-1,-1],
    [ 4,11, 8, 4, 6,11, 0, 2, 9, 2,10, 9,-1,-1,-1,-1],
    [10, 9, 3,10, 3, 2, 9, 4, 3,11, 3, 6, 4, 6, 3,-1],
    [ 8, 2, 3, 8, 4, 2, 4, 6, 2,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 4, 2, 4, 6, 2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 9, 0, 2, 3, 4, 2, 4, 6, 4, 3, 8,-1,-1,-1,-1],
    [ 1, 9, 4, 1, 4, 2, 2, 4, 6,-1,-1,-1,-1,-1,-1,-1],
    [ 8, 1, 3, 8, 6, 1, 8, 4, 6, 6,10, 1,-1,-1,-1,-1],
    [10, 1, 0,10, 0, 6, 6, 0, 4,-1,-1,-1,-1,-1,-1,-1],
    [ 4, 6, 3, 4, 3, 8, 6,10, 3, 0, 3, 9,10, 9, 3,-1],
    [10, 9, 4, 6,10, 4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 4, 9, 5, 7, 6,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 8, 3, 4, 9, 5,11, 7, 6,-1,-1,-1,-1,-1,-1,-1],
    [ 5, 0, 1, 5, 4, 0, 7, 6,11,-1,-1,-1,-1,-1,-1,-1],
    [11, 7, 6, 8, 3, 4, 3, 5, 4, 3, 1, 5,-1,-1,-1,-1],
    [ 9, 5, 4,10, 1, 2, 7, 6,11,-1,-1,-1,-1,-1,-1,-1],
    [ 6,11, 7, 1, 2,10, 0, 8, 3, 4, 9, 5,-1,-1,-1,-1],
    [ 7, 6,11, 5, 4,10, 4, 2,10, 4, 0, 2,-1,-1,-1,-1],
    [ 3, 4, 8, 3, 5, 4, 3, 2, 5,10, 5, 2,11, 7, 6,-1],
    [ 7, 2, 3, 7, 6, 2, 5, 4, 9,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 5, 4, 0, 8, 6, 0, 6, 2, 6, 8, 7,-1,-1,-1,-1],
    [ 3, 6, 2, 3, 7, 6, 1, 5, 0, 5, 4, 0,-1,-1,-1,-1],
    [ 6, 2, 8, 6, 8, 7, 2, 1, 8, 4, 8, 5, 1, 5, 8,-1],
    [ 9, 5, 4,10, 1, 6, 1, 7, 6, 1, 3, 7,-1,-1,-1,-1],
    [ 1, 6,10, 1, 7, 6, 1, 0, 7, 8, 7, 0, 9, 5, 4,-1],
    [ 4, 0,10, 4,10, 5, 0, 3,10, 6,10, 7, 3, 7,10,-1],
    [ 7, 6,10, 7,10, 8, 5, 4,10, 4, 8,10,-1,-1,-1,-1],
    [ 6, 9, 5, 6,11, 9,11, 8, 9,-1,-1,-1,-1,-1,-1,-1],
    [ 3, 6,11, 0, 6, 3, 0, 5, 6, 0, 9, 5,-1,-1,-1,-1],
    [ 0,11, 8, 0, 5,11, 0, 1, 5, 5, 6,11,-1,-1,-1,-1],
    [ 6,11, 3, 6, 3, 5, 5, 3, 1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 2,10, 9, 5,11, 9,11, 8,11, 5, 6,-1,-1,-1,-1],
    [ 0,11, 3, 0, 6,11, 0, 9, 6, 5, 6, 9, 1, 2,10,-1],
    [11, 8, 5,11, 5, 6, 8, 0, 5,10, 5, 2, 0, 2, 5,-1],
    [ 6,11, 3, 6, 3, 5, 2,10, 3,10, 5, 3,-1,-1,-1,-1],
    [ 5, 8, 9, 5, 2, 8, 5, 6, 2, 3, 8, 2,-1,-1,-1,-1],
    [ 9, 5, 6, 9, 6, 0, 0, 6, 2,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 5, 8, 1, 8, 0, 5, 6, 8, 3, 8, 2, 6, 2, 8,-1],
    [ 1, 5, 6, 2, 1, 6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 3, 6, 1, 6,10, 3, 8, 6, 5, 6, 9, 8, 9, 6,-1],
    [10, 1, 0,10, 0, 6, 9, 5, 0, 5, 6, 0,-1,-1,-1,-1],
    [ 0, 3, 8, 5, 6,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [10, 5, 6,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [11, 5,10, 7, 5,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [11, 5,10,11, 7, 5, 8, 3, 0,-1,-1,-1,-1,-1,-1,-1],
    [ 5,11, 7, 5,10,11, 1, 9, 0,-1,-1,-1,-1,-1,-1,-1],
    [10, 7, 5,10,11, 7, 9, 8, 1, 8, 3, 1,-1,-1,-1,-1],
    [11, 1, 2,11, 7, 1, 7, 5, 1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 8, 3, 1, 2, 7, 1, 7, 5, 7, 2,11,-1,-1,-1,-1],
    [ 9, 7, 5, 9, 2, 7, 9, 0, 2, 2,11, 7,-1,-1,-1,-1],
    [ 7, 5, 2, 7, 2,11, 5, 9, 2, 3, 2, 8, 9, 8, 2,-1],
    [ 2, 5,10, 2, 3, 5, 3, 7, 5,-1,-1,-1,-1,-1,-1,-1],
    [ 8, 2, 0, 8, 5, 2, 8, 7, 5,10, 2, 5,-1,-1,-1,-1],
    [ 9, 0, 1, 5,10, 3, 5, 3, 7, 3,10, 2,-1,-1,-1,-1],
    [ 9, 8, 2, 9, 2, 1, 8, 7, 2,10, 2, 5, 7, 5, 2,-1],
    [ 1, 3, 5, 3, 7, 5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 8, 7, 0, 7, 1, 1, 7, 5,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 0, 3, 9, 3, 5, 5, 3, 7,-1,-1,-1,-1,-1,-1,-1],
    [ 9, 8, 7, 5, 9, 7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 5, 8, 4, 5,10, 8,10,11, 8,-1,-1,-1,-1,-1,-1,-1],
    [ 5, 0, 4, 5,11, 0, 5,10,11,11, 3, 0,-1,-1,-1,-1],
    [ 0, 1, 9, 8, 4,10, 8,10,11,10, 4, 5,-1,-1,-1,-1],
    [10,11, 4,10, 4, 5,11, 3, 4, 9, 4, 1, 3, 1, 4,-1],
    [ 2, 5, 1, 2, 8, 5, 2,11, 8, 4, 5, 8,-1,-1,-1,-1],
    [ 0, 4,11, 0,11, 3, 4, 5,11, 2,11, 1, 5, 1,11,-1],
    [ 0, 2, 5, 0, 5, 9, 2,11, 5, 4, 5, 8,11, 8, 5,-1],
    [ 9, 4, 5, 2,11, 3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 2, 5,10, 3, 5, 2, 3, 4, 5, 3, 8, 4,-1,-1,-1,-1],
    [ 5,10, 2, 5, 2, 4, 4, 2, 0,-1,-1,-1,-1,-1,-1,-1],
    [ 3,10, 2, 3, 5,10, 3, 8, 5, 4, 5, 8, 0, 1, 9,-1],
    [ 5,10, 2, 5, 2, 4, 1, 9, 2, 9, 4, 2,-1,-1,-1,-1],
    [ 8, 4, 5, 8, 5, 3, 3, 5, 1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 4, 5, 1, 0, 5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 8, 4, 5, 8, 5, 3, 9, 0, 5, 0, 3, 5,-1,-1,-1,-1],
    [ 9, 4, 5,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 4,11, 7, 4, 9,11, 9,10,11,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 8, 3, 4, 9, 7, 9,11, 7, 9,10,11,-1,-1,-1,-1],
    [ 1,10,11, 1,11, 4, 1, 4, 0, 7, 4,11,-1,-1,-1,-1],
    [ 3, 1, 4, 3, 4, 8, 1,10, 4, 7, 4,11,10,11, 4,-1],
    [ 4,11, 7, 9,11, 4, 9, 2,11, 9, 1, 2,-1,-1,-1,-1],
    [ 9, 7, 4, 9,11, 7, 9, 1,11, 2,11, 1, 0, 8, 3,-1],
    [11, 7, 4,11, 4, 2, 2, 4, 0,-1,-1,-1,-1,-1,-1,-1],
    [11, 7, 4,11, 4, 2, 8, 3, 4, 3, 2, 4,-1,-1,-1,-1],
    [ 2, 9,10, 2, 7, 9, 2, 3, 7, 7, 4, 9,-1,-1,-1,-1],
    [ 9,10, 7, 9, 7, 4,10, 2, 7, 8, 7, 0, 2, 0, 7,-1],
    [ 3, 7,10, 3,10, 2, 7, 4,10, 1,10, 0, 4, 0,10,-1],
    [ 1,10, 2, 8, 7, 4,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 4, 9, 1, 4, 1, 7, 7, 1, 3,-1,-1,-1,-1,-1,-1,-1],
    [ 4, 9, 1, 4, 1, 7, 0, 8, 1, 8, 7, 1,-1,-1,-1,-1],
    [ 4, 0, 3, 7, 4, 3,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 4, 8, 7,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 9,10, 8,10,11, 8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 3, 0, 9, 3, 9,11,11, 9,10,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 1,10, 0,10, 8, 8,10,11,-1,-1,-1,-1,-1,-1,-1],
    [ 3, 1,10,11, 3,10,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 2,11, 1,11, 9, 9,11, 8,-1,-1,-1,-1,-1,-1,-1],
    [ 3, 0, 9, 3, 9,11, 1, 2, 9, 2,11, 9,-1,-1,-1,-1],
    [ 0, 2,11, 8, 0,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 3, 2,11,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 2, 3, 8, 2, 8,10,10, 8, 9,-1,-1,-1,-1,-1,-1,-1],
    [ 9,10, 2, 0, 9, 2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 2, 3, 8, 2, 8,10, 0, 1, 8, 1,10, 8,-1,-1,-1,-1],
    [ 1,10, 2,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 1, 3, 8, 9, 1, 8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 9, 1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [ 0, 3, 8,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
    [-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1],
];

// ---------------------------------------------------------------------------
// Edge vertex pairs (which two corners does each of the 12 edges connect?)
// ---------------------------------------------------------------------------

/// Corner indices for each of the 12 cell edges: (corner_a, corner_b).
const EDGE_CORNERS: [(usize, usize); 12] = [
    (0, 1), // edge 0
    (1, 2), // edge 1
    (2, 3), // edge 2
    (3, 0), // edge 3
    (4, 5), // edge 4
    (5, 6), // edge 5
    (6, 7), // edge 6
    (7, 4), // edge 7
    (0, 4), // edge 8
    (1, 5), // edge 9
    (2, 6), // edge 10
    (3, 7), // edge 11
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Grid parameters for a Marching Cubes extraction.
pub struct GridParams {
    /// Minimum corner of the grid bounding box.
    pub min: Vec3,
    /// Maximum corner of the grid bounding box.
    pub max: Vec3,
    /// Number of *cells* along each axis (vertices = resolution + 1).
    pub resolution: [u32; 3],
}

/// Extract an isosurface from a scalar field using Marching Cubes.
///
/// `field` is called at each grid vertex. Vertices where `field(p) >= iso_value` are
/// considered *inside* the surface.
///
/// Output uses `standard_layout()` (pos + normal + uv, stride 32 bytes).
/// If `normal_fn` is `Some`, it is called to compute the outward unit normal at each
/// new vertex position (analytical gradient). If `None`, normals are computed via
/// central differences of the field (6 extra `field` calls per vertex).
/// UVs are `[0.0, 0.0]` placeholder.
/// Indices are `u32`.
///
/// Returns [`DynamicMeshData`] with vertex bytes, index bytes, index count, and a
/// bounding sphere computed from all output vertex positions.
pub fn extract(
    field: &dyn Fn(Vec3) -> f32,
    params: &GridParams,
    iso_value: f32,
    normal_fn: Option<&dyn Fn(Vec3) -> [f32; 3]>,
) -> DynamicMeshData {
    let [nx, ny, nz] = params.resolution;
    // Number of vertices along each axis = cells + 1
    let vx = nx + 1;
    let vy = ny + 1;
    let vz = nz + 1;

    let cell_size = Vec3::new(
        (params.max.x - params.min.x) / nx as f32,
        (params.max.y - params.min.y) / ny as f32,
        (params.max.z - params.min.z) / nz as f32,
    );
    let epsilon = cell_size.min_element() * 0.01;

    // Pre-evaluate field at all grid vertices
    let total_verts = (vx * vy * vz) as usize;
    let mut grid_values = vec![0.0_f32; total_verts];
    let mut grid_positions = vec![Vec3::ZERO; total_verts];

    for iz in 0..vz {
        for iy in 0..vy {
            for ix in 0..vx {
                let idx = (iz * vy * vx + iy * vx + ix) as usize;
                let p = params.min
                    + Vec3::new(
                        ix as f32 * cell_size.x,
                        iy as f32 * cell_size.y,
                        iz as f32 * cell_size.z,
                    );
                grid_positions[idx] = p;
                grid_values[idx] = field(p);
            }
        }
    }

    // Output buffers
    let mut vertex_data: Vec<u8> = Vec::new();
    let mut index_data: Vec<u8> = Vec::new();
    let mut vertex_count: u32 = 0;

    // Deduplication: (cell_flat_index * 12 + edge_index) -> vertex_index
    let mut edge_to_vertex: HashMap<u64, u32> = HashMap::new();

    // Bounding box accumulation (single-pass, no extra Vec allocation)
    let mut bb_min = Vec3::splat(f32::MAX);
    let mut bb_max = Vec3::splat(f32::MIN);

    for iz in 0..nz {
        for iy in 0..ny {
            for ix in 0..nx {
                // 8 corner indices into the grid_values/grid_positions arrays
                let corners: [usize; 8] = [
                    (iz * vy * vx + iy * vx + ix) as usize,
                    (iz * vy * vx + iy * vx + ix + 1) as usize,
                    (iz * vy * vx + (iy + 1) * vx + ix + 1) as usize,
                    (iz * vy * vx + (iy + 1) * vx + ix) as usize,
                    ((iz + 1) * vy * vx + iy * vx + ix) as usize,
                    ((iz + 1) * vy * vx + iy * vx + ix + 1) as usize,
                    ((iz + 1) * vy * vx + (iy + 1) * vx + ix + 1) as usize,
                    ((iz + 1) * vy * vx + (iy + 1) * vx + ix) as usize,
                ];

                // Build cube index.
                // Bourke's TRI_TABLE uses the convention that bit=1 means the vertex is
                // *below* the isovalue (outside the surface).  Our fields are "inside=high"
                // (metaball-style), so we set the bit when value < iso_value to match the
                // table's winding expectations.
                let mut cube_index: usize = 0;
                for (bit, &c) in corners.iter().enumerate() {
                    if grid_values[c] < iso_value {
                        cube_index |= 1 << bit;
                    }
                }

                let edge_mask = EDGE_TABLE[cube_index];
                if edge_mask == 0 {
                    continue; // no surface in this cell
                }

                let cell_flat = (iz * ny * nx + iy * nx + ix) as u64;

                // For each active edge, compute or retrieve the interpolated vertex
                let mut edge_verts: [u32; 12] = [u32::MAX; 12];

                for edge_idx in 0..12u64 {
                    if edge_mask & (1 << edge_idx) == 0 {
                        continue;
                    }

                    let dedup_key = cell_flat * 12 + edge_idx;

                    let vert_idx = if let Some(&existing) = edge_to_vertex.get(&dedup_key) {
                        existing
                    } else {
                        let (ca, cb) = EDGE_CORNERS[edge_idx as usize];
                        let pa = grid_positions[corners[ca]];
                        let pb = grid_positions[corners[cb]];
                        let va = grid_values[corners[ca]];
                        let vb = grid_values[corners[cb]];

                        let pos = interpolate_edge(pa, pb, va, vb, iso_value);
                        let normal = if let Some(nfn) = normal_fn {
                            nfn(pos)
                        } else {
                            gradient_normal(field, pos, epsilon)
                        };

                        push_vertex(&mut vertex_data, pos, normal, [0.0, 0.0]);
                        let idx = vertex_count;
                        vertex_count += 1;

                        bb_min = bb_min.min(pos);
                        bb_max = bb_max.max(pos);

                        edge_to_vertex.insert(dedup_key, idx);
                        idx
                    };

                    edge_verts[edge_idx as usize] = vert_idx;
                }

                // Emit triangles from TRI_TABLE
                let tri_row = &TRI_TABLE[cube_index];
                let mut i = 0;
                while i < 15 {
                    let e0 = tri_row[i];
                    if e0 < 0 {
                        break;
                    }
                    let e1 = tri_row[i + 1];
                    let e2 = tri_row[i + 2];
                    let v0 = edge_verts[e0 as usize];
                    let v1 = edge_verts[e1 as usize];
                    let v2 = edge_verts[e2 as usize];
                    push_u32(&mut index_data, v0);
                    push_u32(&mut index_data, v1);
                    push_u32(&mut index_data, v2);
                    i += 3;
                }
            }
        }
    }

    let index_count = (index_data.len() / 4) as u32;

    // Compute bounding sphere from AABB (single-pass, no extra allocation)
    let local_bounds = if vertex_count == 0 {
        BoundingSphere::ZERO
    } else {
        let center = (bb_min + bb_max) * 0.5;
        let radius = (bb_max - bb_min).length() * 0.5;
        BoundingSphere { center, radius }
    };

    DynamicMeshData {
        vertex_data,
        index_data,
        index_count,
        local_bounds,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn interpolate_edge(p0: Vec3, p1: Vec3, v0: f32, v1: f32, iso: f32) -> Vec3 {
    let dv = v1 - v0;
    if dv.abs() < 1e-10 {
        return (p0 + p1) * 0.5;
    }
    let t = (iso - v0) / dv;
    p0 + t * (p1 - p0)
}

fn gradient_normal(field: &dyn Fn(Vec3) -> f32, pos: Vec3, epsilon: f32) -> [f32; 3] {
    let dx = field(pos + Vec3::new(epsilon, 0.0, 0.0)) - field(pos - Vec3::new(epsilon, 0.0, 0.0));
    let dy = field(pos + Vec3::new(0.0, epsilon, 0.0)) - field(pos - Vec3::new(0.0, epsilon, 0.0));
    let dz = field(pos + Vec3::new(0.0, 0.0, epsilon)) - field(pos - Vec3::new(0.0, 0.0, epsilon));
    // Gradient points inward (toward higher field values); negate for outward normal.
    let g = Vec3::new(-dx, -dy, -dz);
    let len = g.length();
    if len > 1e-10 {
        let n = g / len;
        [n.x, n.y, n.z]
    } else {
        [0.0, 1.0, 0.0] // fallback
    }
}

fn push_vertex(buf: &mut Vec<u8>, pos: Vec3, normal: [f32; 3], uv: [f32; 2]) {
    for f in [pos.x, pos.y, pos.z].iter().chain(normal.iter()) {
        buf.extend_from_slice(&f.to_le_bytes());
    }
    for f in &uv {
        buf.extend_from_slice(&f.to_le_bytes());
    }
}

fn push_u32(buf: &mut Vec<u8>, idx: u32) {
    buf.extend_from_slice(&idx.to_le_bytes());
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sphere_field(center: Vec3, radius: f32) -> impl Fn(Vec3) -> f32 {
        move |p: Vec3| {
            let d2 = (p - center).length_squared().max(1e-10);
            radius * radius / d2
        }
    }

    fn default_params(res: u32) -> GridParams {
        GridParams {
            min: Vec3::splat(-3.0),
            max: Vec3::splat(3.0),
            resolution: [res, res, res],
        }
    }

    #[test]
    fn empty_field_produces_no_geometry() {
        let field = |_: Vec3| 0.0_f32;
        let result = extract(&field, &default_params(8), 1.0, None);
        assert_eq!(result.vertex_data.len(), 0);
        assert_eq!(result.index_count, 0);
    }

    #[test]
    fn uniform_above_produces_no_geometry() {
        // All corners inside → cube index 255 → no surface
        let field = |_: Vec3| 2.0_f32;
        let result = extract(&field, &default_params(8), 1.0, None);
        assert_eq!(result.index_count, 0);
    }

    #[test]
    fn single_sphere_produces_geometry() {
        let field = sphere_field(Vec3::ZERO, 1.0);
        let result = extract(&field, &default_params(16), 1.0, None);
        assert!(
            result.index_count > 0,
            "expected triangles for a sphere field"
        );
        assert!(result.vertex_data.len() > 0);
    }

    #[test]
    fn index_count_is_multiple_of_three() {
        let field = sphere_field(Vec3::ZERO, 1.0);
        let result = extract(&field, &default_params(16), 1.0, None);
        assert_eq!(result.index_count % 3, 0);
    }

    #[test]
    fn all_indices_are_within_vertex_count() {
        let field = sphere_field(Vec3::ZERO, 1.0);
        let result = extract(&field, &default_params(16), 1.0, None);
        let vertex_count = (result.vertex_data.len() / 32) as u32;
        let indices: Vec<u32> = result
            .index_data
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        for &idx in &indices {
            assert!(
                idx < vertex_count,
                "index {idx} >= vertex_count {vertex_count}"
            );
        }
    }

    #[test]
    fn vertex_data_length_is_multiple_of_stride() {
        let field = sphere_field(Vec3::ZERO, 1.0);
        let result = extract(&field, &default_params(16), 1.0, None);
        assert_eq!(result.vertex_data.len() % 32, 0);
    }

    #[test]
    fn all_normals_are_unit_length() {
        let field = sphere_field(Vec3::ZERO, 1.0);
        let result = extract(&field, &default_params(16), 1.0, None);
        let vertex_count = result.vertex_data.len() / 32;
        for i in 0..vertex_count {
            let base = i * 32 + 12; // normal at offset 12
            let nx = f32::from_le_bytes(result.vertex_data[base..base + 4].try_into().unwrap());
            let ny = f32::from_le_bytes(result.vertex_data[base + 4..base + 8].try_into().unwrap());
            let nz =
                f32::from_le_bytes(result.vertex_data[base + 8..base + 12].try_into().unwrap());
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            assert!(
                (len - 1.0).abs() < 1e-4,
                "normal at vertex {i} has length {len}"
            );
        }
    }

    #[test]
    fn bounding_sphere_contains_all_vertices() {
        let field = sphere_field(Vec3::ZERO, 1.0);
        let result = extract(&field, &default_params(16), 1.0, None);
        let vertex_count = result.vertex_data.len() / 32;
        let bounds = result.local_bounds;
        for i in 0..vertex_count {
            let base = i * 32;
            let px = f32::from_le_bytes(result.vertex_data[base..base + 4].try_into().unwrap());
            let py = f32::from_le_bytes(result.vertex_data[base + 4..base + 8].try_into().unwrap());
            let pz =
                f32::from_le_bytes(result.vertex_data[base + 8..base + 12].try_into().unwrap());
            let pos = Vec3::new(px, py, pz);
            let dist = (pos - bounds.center).length();
            assert!(
                dist <= bounds.radius + 1e-4,
                "vertex {i} at distance {dist} exceeds bounds radius {}",
                bounds.radius
            );
        }
    }

    #[test]
    fn higher_resolution_produces_more_triangles() {
        let field = sphere_field(Vec3::ZERO, 1.0);
        let lo = extract(&field, &default_params(8), 1.0, None);
        let hi = extract(&field, &default_params(24), 1.0, None);
        assert!(
            hi.index_count > lo.index_count,
            "higher resolution should produce more triangles"
        );
    }

    #[test]
    fn winding_normals_point_outward_for_sphere() {
        // For a sphere field centred at origin, each triangle's face normal should
        // have a positive dot product with the vector from origin to triangle centroid.
        let field = sphere_field(Vec3::ZERO, 1.0);
        let result = extract(&field, &default_params(16), 1.0, None);
        let vertex_count = result.vertex_data.len() / 32;
        let positions: Vec<Vec3> = (0..vertex_count)
            .map(|i| {
                let base = i * 32;
                let px = f32::from_le_bytes(result.vertex_data[base..base + 4].try_into().unwrap());
                let py =
                    f32::from_le_bytes(result.vertex_data[base + 4..base + 8].try_into().unwrap());
                let pz =
                    f32::from_le_bytes(result.vertex_data[base + 8..base + 12].try_into().unwrap());
                Vec3::new(px, py, pz)
            })
            .collect();

        let indices: Vec<u32> = result
            .index_data
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();

        let mut outward = 0usize;
        let mut total = 0usize;
        for tri in indices.chunks_exact(3) {
            let p0 = positions[tri[0] as usize];
            let p1 = positions[tri[1] as usize];
            let p2 = positions[tri[2] as usize];
            let centroid = (p0 + p1 + p2) / 3.0;
            let edge1 = p1 - p0;
            let edge2 = p2 - p0;
            let face_normal = edge1.cross(edge2);
            if face_normal.dot(centroid) > 0.0 {
                outward += 1;
            }
            total += 1;
        }
        // At least 90% of triangles should have outward-facing normals
        assert!(
            outward * 10 >= total * 9,
            "only {outward}/{total} triangles have outward normals"
        );
    }

    #[test]
    fn empty_field_bounding_sphere_is_zero() {
        let field = |_: Vec3| 0.0_f32;
        let result = extract(&field, &default_params(8), 1.0, None);
        assert_eq!(result.local_bounds.radius, 0.0);
    }
}
