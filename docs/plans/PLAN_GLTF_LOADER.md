# Feature: glTF 2.0 Loader (Phase D)

**Source doc:** `docs/MATERIAL.md` §7  
**Status:** Ready for implementation  
**Prerequisite:** Phases A–C complete ✓ (5-slot PBR, 48-byte vertex layout, tangent utils, terrain)

---

## Context

The `rig-gltf` crate does not yet exist. The workspace has no `gltf` dependency.
All other infrastructure is in place:

- `rig-assets::tangent_utils` — mikktspace + normal-derived fallback ✓
- `MaterialAsset.textures: Vec<Option<(TextureHandle, SamplerHandle)>>` — 5-slot ✓
- Renderer 11-binding material bind group ✓
- `SkinAsset`, `SkinWeights`, `AnimationClip` asset types ✓
- `AnimationPlayer`, `SkinEvaluator` runtime types ✓
- `SceneGraph` with `create_node`, `attach_child`, `set_renderable`, `set_camera`, `set_light` ✓

The new crate sits as a **peer of `rig-import`** — it does not depend on it.
Dependency chain: `rig-math` → `rig-assets` → `rig-gltf` → `rig-app`.

---

## Crate layout (target state)

```
crates/gltf/
  Cargo.toml
  src/
    lib.rs          — public API, module declarations, re-exports
    error.rs        — GltfError, Result alias
    buffers.rs      — accessor → typed Vec helpers
    textures.rs     — image + sampler adaptation
    meshes.rs       — primitive → MeshAsset (48-byte, tangents)
    materials.rs    — PBR metallic-roughness → 5-slot MaterialAsset
    nodes.rs        — glTF node tree → SceneGraph hierarchy
    cameras.rs      — glTF camera → CameraComponent
    lights.rs       — KHR_lights_punctual → LightComponent
    animations.rs   — glTF animation → AnimationClip
    skins.rs        — glTF skin + weights → SkinAsset + SkinWeights
    loader.rs       — load_gltf() entry point, LoadedGltf output type
```

---

## Phase 0: Acquire glTF sample asset with Git LFS

Commit message: `chore(assets): add DamagedHelmet glTF sample asset`

### Step 1: Confirm GLB files are tracked by Git LFS

Confirm `.gitattributes` contains:

```gitattributes
assets/**/*.glb filter=lfs diff=lfs merge=lfs -text
```

This repository already has that rule, so no `.gitattributes` change is needed.

### Step 2: Download DamagedHelmet.glb

Download the Khronos glTF sample asset from:

```text
https://github.com/KhronosGroup/glTF-Sample-Assets/raw/main/Models/DamagedHelmet/glTF-Binary/DamagedHelmet.glb
```

Store it at:

```text
assets/models/gltf/DamagedHelmet.glb
```

This path is used by the Phase 12 `gltf_demo` default CLI fallback.

### Step 3: Stage through Git LFS from the Nix dev shell

Run Git LFS operations from inside the dev shell:

```bash
nix develop --impure --command git add assets/models/gltf/DamagedHelmet.glb
nix develop --impure --command git lfs status
```

Expected status includes the asset under **Objects to be committed** with an
`LFS:` object id, for example:

```text
assets/models/gltf/DamagedHelmet.glb (LFS: ...)
```

### Step 4: Current completion status

Completed in this session:

- Created `assets/models/gltf/`
- Downloaded `DamagedHelmet.glb` from Khronos sample assets
- Staged `assets/models/gltf/DamagedHelmet.glb` through Git LFS in the Nix dev shell
- Verified `git lfs status` reports the file as an LFS object to be committed

---

## Phase 1: Crate scaffold and workspace integration

Commit message: `feat(gltf): scaffold rig-gltf crate with workspace integration`

### Step 1: Add gltf and rig-gltf to workspace dependencies

Edit root `Cargo.toml`.

In `[workspace.dependencies]`, after `noise = "0.9"` add:
```toml
gltf = { version = "1.4", features = ["KHR_lights_punctual"] }
rig-gltf = { path = "crates/gltf" }
```

In `[workspace]` `members`, after `"crates/skin"` add:
```
"crates/gltf",
```

### Step 2: Create crates/gltf/Cargo.toml

```toml
[package]
name = "rig-gltf"
version = "0.1.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "glTF 2.0 adaptation layer for the rig framework"

[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(tarpaulin_include)'] }

[dependencies]
rig-assets.workspace = true
rig-scene.workspace = true
rig-math.workspace = true
gltf.workspace = true
thiserror.workspace = true
log.workspace = true
```

### Step 3: Create crates/gltf/src/lib.rs (minimal stub)

```rust
//! glTF 2.0 asset adaptation for the rig framework.
//!
//! Parses `.gltf` and `.glb` files using the `gltf` crate and adapts their
//! contents into engine asset types and scene graph hierarchy.
//!
//! # Example
//!
//! ```no_run
//! use rig_gltf::load_gltf;
//! # use rig_assets::{AssetStore, ShaderHandle};
//! # use rig_scene::SceneGraph;
//! # let mut scene = SceneGraph::new();
//! # let mut store = AssetStore::new();
//! # let shader = ShaderHandle::from_raw(0);
//!
//! let loaded = load_gltf("model.glb", shader, &mut scene, &mut store)?;
//! println!("{} root nodes", loaded.root_nodes.len());
//! # Ok::<(), rig_gltf::GltfError>(())
//! ```

mod animations;
mod buffers;
mod cameras;
mod error;
mod lights;
mod loader;
mod materials;
mod meshes;
mod nodes;
mod skins;
mod textures;

pub use error::{GltfError, Result};
pub use loader::{LoadedGltf, load_gltf};
```

All submodules start as empty files (`// placeholder`) so the crate compiles.

---

## Phase 2: Error types and buffer helpers

Commit message: `feat(gltf): add GltfError type and typed accessor reader helpers`

### Step 1: Implement crates/gltf/src/error.rs

```rust
//! Error types for the rig-gltf crate.

use thiserror::Error;

/// Errors produced while loading and adapting glTF assets.
#[derive(Debug, Error)]
pub enum GltfError {
    #[error("glTF import error: {0}")]
    Import(#[from] gltf::Error),

    #[error("missing buffer data for buffer index {index}")]
    MissingBuffer { index: usize },

    #[error("missing image data for image index {index}")]
    MissingImage { index: usize },

    #[error("unsupported primitive topology: {0:?}")]
    UnsupportedTopology(gltf::mesh::Mode),

    #[error("primitive missing required POSITION attribute")]
    MissingPositions,

    #[error("accessor type/component mismatch: expected {expected}, got {got}")]
    AccessorMismatch { expected: String, got: String },

    #[error("sparse accessors are not supported")]
    SparseAccessor,
}

/// Convenience alias for `Result<T, GltfError>`.
pub type Result<T> = std::result::Result<T, GltfError>;
```

### Step 2: Implement crates/gltf/src/buffers.rs

This module provides typed accessor readers. The `gltf` crate's `Primitive::reader()`
returns a `Reader<'_, '_, F>` where `F: Clone + Fn(gltf::Buffer<'_>) -> Option<&'_ [u8]>`.
Build the buffer getter closure from `&[gltf::buffer::Data]`.

```rust
//! Typed accessor reader helpers for glTF buffer data.

use rig_math::{Mat4, Quat, Vec3};

use crate::error::{GltfError, Result};

/// Build the buffer getter closure required by `gltf::Primitive::reader`.
pub(crate) fn buffer_getter<'a>(
    buffers: &'a [gltf::buffer::Data],
) -> impl Clone + Fn(gltf::Buffer<'_>) -> Option<&'a [u8]> {
    move |buffer: gltf::Buffer<'_>| buffers.get(buffer.index()).map(|b| b.0.as_slice())
}
```

Implement the following functions using the reader API. Each function takes a
`gltf::Primitive` and `buffers: &[gltf::buffer::Data]` (or a pre-built reader).

**Positions:**
```rust
pub(crate) fn read_positions(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
) -> Result<Vec<f32>>
```
Use `reader.read_positions()` → `Option<ReadPositions>`. Flatten `[f32; 3]` into `Vec<f32>`.
Return `GltfError::MissingPositions` if absent.

**Normals:**
```rust
pub(crate) fn read_normals(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
) -> Option<Vec<f32>>
```
Use `reader.read_normals()`. Returns `None` if absent (caller generates normals).

**Tex coords:**
```rust
pub(crate) fn read_tex_coords(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
    set: u32,
) -> Option<Vec<f32>>
```
Use `reader.read_tex_coords(set)` → `ReadTexCoords::F32`. Flatten `[f32; 2]`.

**Tangents:**
```rust
pub(crate) fn read_tangents(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
) -> Option<Vec<[f32; 4]>>
```
Use `reader.read_tangents()`. Returns `None` if absent.

**Indices:**
```rust
pub(crate) fn read_indices(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
) -> Option<Vec<u32>>
```
Use `reader.read_indices()` → `ReadIndices` (handles u8/u16/u32 variants). Returns `None`
if the primitive has no index accessor (non-indexed geometry).

**Joints (for skinning):**
```rust
pub(crate) fn read_joints(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
    set: u32,
) -> Option<Vec<[u16; 4]>>
```
Use `reader.read_joints(set)` → `ReadJoints::U8` or `U16`. Promote U8 to U16.

**Weights (for skinning):**
```rust
pub(crate) fn read_weights(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
    set: u32,
) -> Option<Vec<[f32; 4]>>
```
Use `reader.read_weights(set)` → `ReadWeights::F32` or `U8`/`U16` (normalize to f32).

**Inverse bind matrices (for skins):**
```rust
pub(crate) fn read_inverse_bind_matrices(
    skin: &gltf::Skin,
    buffers: &[gltf::buffer::Data],
) -> Vec<rig_math::Mat4>
```
Use `skin.reader(getter).read_inverse_bind_matrices()`. Each `[[f32; 4]; 4]` → `Mat4::from_cols_array_2d`.
If absent, return `vec![Mat4::IDENTITY; skin.joints().count()]`.

**Animation timestamps:**
```rust
pub(crate) fn read_timestamps(
    sampler: &gltf::animation::Sampler,
    buffers: &[gltf::buffer::Data],
) -> Vec<f32>
```
Use `sampler.reader(getter).read_inputs()`. Collect into `Vec<f32>`.

**Animation output values (translations, rotations, scales):**
```rust
pub(crate) fn read_anim_translations(
    sampler: &gltf::animation::Sampler,
    buffers: &[gltf::buffer::Data],
) -> Vec<Vec3>

pub(crate) fn read_anim_rotations(
    sampler: &gltf::animation::Sampler,
    buffers: &[gltf::buffer::Data],
) -> Vec<Quat>

pub(crate) fn read_anim_scales(
    sampler: &gltf::animation::Sampler,
    buffers: &[gltf::buffer::Data],
) -> Vec<Vec3>
```
Use `sampler.reader(getter).read_outputs()` → `ReadOutputs::Translations | Rotations | Scales`.
For `Rotations`, the `gltf` crate returns `[f32; 4]` as `[x, y, z, w]` — construct
`Quat::from_xyzw(x, y, z, w)`.

For **CubicSpline** interpolation, glTF stores 3 values per keyframe (in-tangent, value,
out-tangent). Extract only the middle value (index 1 of each triple) for the initial
implementation. Document this as a known limitation.

---

## Phase 3: Texture and sampler adaptation

Commit message: `feat(gltf): adapt glTF images and samplers into TextureAsset and SamplerDescriptor`

### Step 1: Implement crates/gltf/src/textures.rs

```rust
//! glTF image and sampler adaptation.

use rig_assets::{
    AddressMode, AssetStore, FilterMode, SamplerDescriptor, SamplerHandle, TextureAsset,
    TextureFormat, TextureHandle,
};

/// Adapt all glTF images into `TextureAsset` values registered in `store`.
///
/// Returns handles indexed by glTF image index. All images are stored as
/// `Rgba8Unorm` (linear). Color-space selection (sRGB vs linear) is deferred
/// to the material layer which knows which slot each image occupies.
pub(crate) fn adapt_images(
    images: &[gltf::image::Data],
    store: &mut AssetStore,
) -> Vec<TextureHandle>
```

For each `gltf::image::Data`:
- If `format == gltf::image::Format::R8G8B8A8` → use data directly.
- If `format == gltf::image::Format::R8G8B8` → expand to RGBA by inserting `255` alpha per pixel.
- If `format == gltf::image::Format::R8` → expand to RGBA (R, R, R, 255).
- Other formats (16-bit) → convert to 8-bit by right-shifting 8 bits, then expand to RGBA.
- Create `TextureAsset { width, height, format: TextureFormat::Rgba8Unorm, data: Arc::from(rgba_bytes) }`.
- Register with `store.add_texture(asset)`.

```rust
/// Adapt all glTF samplers into `SamplerDescriptor` values registered in `store`.
///
/// Returns handles indexed by glTF sampler index.
pub(crate) fn adapt_samplers(
    document: &gltf::Document,
    store: &mut AssetStore,
) -> Vec<SamplerHandle>
```

For each `gltf::texture::Sampler`:
- Map `wrap_s()` / `wrap_t()` → `AddressMode`:
  - `WrappingMode::ClampToEdge` → `AddressMode::ClampToEdge`
  - `WrappingMode::MirroredRepeat` → `AddressMode::MirrorRepeat`
  - `WrappingMode::Repeat` → `AddressMode::Repeat`
- Map `mag_filter()` → `FilterMode`:
  - `None` or `MagFilter::Linear` → `FilterMode::Linear`
  - `MagFilter::Nearest` → `FilterMode::Nearest`
- Map `min_filter()` similarly (ignore mipmap variants for now, use base filter).
- Register with `store.add_sampler(desc)`.

```rust
/// Create a default linear clamp-to-edge sampler for materials without explicit samplers.
pub(crate) fn default_sampler(store: &mut AssetStore) -> SamplerHandle {
    store.add_sampler(SamplerDescriptor::default())
}
```

---

## Phase 4: Mesh adaptation

Commit message: `feat(gltf): adapt glTF mesh primitives into 48-byte MeshAsset with tangent generation`

### Step 1: Implement crates/gltf/src/meshes.rs

```rust
//! glTF mesh primitive adaptation.

use std::sync::Arc;

use rig_assets::{
    AssetStore, IndexFormat, MeshAsset, MeshHandle, standard_vertex_layout, tangent_utils,
};
use rig_math::{BoundingSphere, Vec3};

use crate::buffers;
use crate::error::{GltfError, Result};

/// Adapt a single glTF primitive into a `MeshAsset` with the 48-byte vertex layout.
pub(crate) fn adapt_primitive(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
) -> Result<MeshAsset>
```

Implementation steps:
1. Reject non-`Triangles` mode: `if primitive.mode() != gltf::mesh::Mode::Triangles { return Err(GltfError::UnsupportedTopology(primitive.mode())); }`
2. Read positions (required).
3. Read normals; if absent, generate smooth normals from positions + indices using the private `generate_smooth_normals` helper below.
4. Read TEXCOORD_0; if absent, fill with `vec![0.0_f32; vertex_count * 2]`.
5. Read indices; if absent, generate sequential `(0..vertex_count as u32).collect()`.
6. Read tangents from accessor; if absent, call `tangent_utils::generate_tangents(&positions, &normals, &uvs, &indices)`.
7. Interleave into 48-byte vertex buffer:
   ```
   for each vertex i:
     write positions[i*3..i*3+3]   (12 bytes)
     write normals[i*3..i*3+3]     (12 bytes)
     write uvs[i*2..i*2+2]         (8 bytes)
     write tangents[i]             (16 bytes)
   ```
8. Pack indices: if `vertex_count <= 65535`, use `Uint16` (pack each u32 as u16 LE); else use `Uint32`.
9. Compute bounding sphere from positions using `compute_bounding_sphere`.
10. Return `MeshAsset { vertex_layout: standard_vertex_layout(), vertex_data: Arc::from(bytes), index_data: Arc::from(bytes), index_format, local_bounds }`.

```rust
/// Adapt all primitives of a glTF mesh, registering each in `store`.
///
/// Returns `(MeshHandle, Option<material_index>)` per primitive.
pub(crate) fn adapt_mesh(
    mesh: &gltf::Mesh,
    buffers: &[gltf::buffer::Data],
    store: &mut AssetStore,
) -> Result<Vec<(MeshHandle, Option<usize>)>>
```

Iterate `mesh.primitives()`, call `adapt_primitive` for each, register with `store.add_mesh`,
collect `(handle, primitive.material().map(|m| m.index()))`.

**Private helpers:**

```rust
fn generate_smooth_normals(positions: &[f32], indices: &[u32]) -> Vec<f32>
```
Area-weighted smooth normals. Same algorithm as `crates/import/src/importer.rs`.
Initialize `normals = vec![0.0; positions.len()]`. For each triangle (i0, i1, i2):
compute edge vectors, cross product (face normal), add to each vertex's accumulator.
Normalize each vertex normal at the end.

```rust
fn compute_bounding_sphere(positions: &[f32]) -> BoundingSphere
```
Compute centroid, then max distance from centroid. Same pattern as `rig-import`.

```rust
fn pack_indices_u16(indices: &[u32]) -> Vec<u8>
fn pack_indices_u32(indices: &[u32]) -> Vec<u8>
```
Pack index arrays as little-endian bytes.

---

## Phase 5: Material adaptation

Commit message: `feat(gltf): adapt glTF PBR metallic-roughness materials into 5-slot MaterialAsset`

### Step 1: Implement crates/gltf/src/materials.rs

```rust
//! glTF material adaptation — PBR metallic-roughness → 5-slot MaterialAsset.

use rig_assets::{
    AssetStore, MaterialAsset, MaterialHandle, MaterialParams, SamplerHandle, ShaderHandle,
    TextureHandle,
};

/// Adapt a single glTF material into a `MaterialAsset` registered in `store`.
///
/// `image_handles` is indexed by glTF image index.
/// `sampler_handles` is indexed by glTF sampler index.
/// `default_sampler` is used when a texture has no explicit sampler.
pub(crate) fn adapt_material(
    material: &gltf::Material,
    image_handles: &[TextureHandle],
    sampler_handles: &[SamplerHandle],
    default_sampler: SamplerHandle,
    shader: ShaderHandle,
    store: &mut AssetStore,
) -> MaterialHandle
```

Implementation:
1. `let pbr = material.pbr_metallic_roughness();`
2. Build `MaterialParams`:
   - `diffuse`: `pbr.base_color_factor()` → `[r, g, b, a]`
   - `metallic`: `pbr.metallic_factor()`
   - `roughness`: `pbr.roughness_factor()`
   - `emissive`: `[material.emissive_factor()[0], [1], [2], 1.0]`
   - `ambient`: `[0.04, 0.04, 0.04, 1.0]`
   - `specular`: `[1.0, 1.0, 1.0, 32.0]` (Phong compat, unused by PBR shader)
   - `custom_flags`: `0`
   - `triplanar_scale`: `4.0`
3. Build `textures: Vec<Option<(TextureHandle, SamplerHandle)>>` with exactly 5 entries:
   - Slot 0: `resolve_texture(pbr.base_color_texture(), image_handles, sampler_handles, default_sampler)`
   - Slot 1: `resolve_normal_texture(material.normal_texture(), image_handles, sampler_handles, default_sampler)`
   - Slot 2: `resolve_texture(pbr.metallic_roughness_texture(), image_handles, sampler_handles, default_sampler)`
   - Slot 3: `resolve_occlusion_texture(material.occlusion_texture(), image_handles, sampler_handles, default_sampler)`
   - Slot 4: `resolve_texture(material.emissive_texture(), image_handles, sampler_handles, default_sampler)`
4. Register `MaterialAsset { shader, parameters, textures }` and return handle.

**Private helpers:**

```rust
fn resolve_texture(
    info: Option<gltf::texture::Info>,
    image_handles: &[TextureHandle],
    sampler_handles: &[SamplerHandle],
    default_sampler: SamplerHandle,
) -> Option<(TextureHandle, SamplerHandle)>
```
- If `info` is `None`, return `None`.
- `let tex = info.texture();`
- `let image_handle = image_handles[tex.source().index()];`
- `let sampler_handle = tex.sampler().index().map(|i| sampler_handles[i]).unwrap_or(default_sampler);`
- Return `Some((image_handle, sampler_handle))`.

```rust
fn resolve_normal_texture(
    info: Option<gltf::material::NormalTexture>,
    ...
) -> Option<(TextureHandle, SamplerHandle)>

fn resolve_occlusion_texture(
    info: Option<gltf::material::OcclusionTexture>,
    ...
) -> Option<(TextureHandle, SamplerHandle)>
```
Same pattern — `NormalTexture` and `OcclusionTexture` have a different type from `texture::Info`
but expose `.texture()` the same way.

```rust
/// Adapt all glTF materials, returning handles indexed by glTF material index.
pub(crate) fn adapt_materials(
    document: &gltf::Document,
    image_handles: &[TextureHandle],
    sampler_handles: &[SamplerHandle],
    default_sampler: SamplerHandle,
    shader: ShaderHandle,
    store: &mut AssetStore,
) -> Vec<MaterialHandle>
```

---

## Phase 6: Scene graph node hierarchy

Commit message: `feat(gltf): adapt glTF node tree into SceneGraph with transforms`

### Step 1: Implement crates/gltf/src/nodes.rs

```rust
//! glTF node tree → SceneGraph hierarchy.

use rig_math::{Quat, Transform, Vec3};
use rig_scene::{NodeId, SceneGraph};

/// Adapt the default glTF scene's node tree into `scene`.
///
/// Returns a mapping from glTF node index → `NodeId`. Indices with no
/// corresponding node in the active scene are `None`.
pub(crate) fn adapt_nodes(
    document: &gltf::Document,
    scene: &mut SceneGraph,
) -> (Vec<Option<NodeId>>, Vec<NodeId>)
```

Returns `(node_map, root_nodes)`.

Implementation:
1. Select the active glTF scene: `document.default_scene().or_else(|| document.scenes().next())`.
2. Allocate `node_map: Vec<Option<NodeId>> = vec![None; document.nodes().count()]`.
3. For each root node in the scene, call `create_node_recursive(root, None, &mut node_map, scene)`.
4. Collect root `NodeId`s.

```rust
fn create_node_recursive(
    node: gltf::Node,
    parent: Option<NodeId>,
    node_map: &mut Vec<Option<NodeId>>,
    scene: &mut SceneGraph,
) -> NodeId
```
1. `let name = node.name().unwrap_or(&format!("node_{}", node.index())).to_string();`
2. `let id = scene.create_node(name);`
3. `node_map[node.index()] = Some(id);`
4. Extract transform from `node.transform()`:
   - `Transform::Decomposed { translation, rotation, scale }` → build `rig_math::Transform { translation: Vec3::from(translation), rotation: Quat::from_array(rotation), scale: Vec3::from(scale) }`.
   - `Transform::Matrix { matrix }` → decompose via `Mat4::from_cols_array_2d(matrix).to_scale_rotation_translation()` → build `Transform`.
5. `scene.set_local_transform(id, transform).ok();`
6. If `parent` is `Some(p)`, call `scene.attach_child(p, id).ok();`
7. For each child in `node.children()`, call `create_node_recursive(child, Some(id), node_map, scene)`.
8. Return `id`.

---

## Phase 7: Camera and light adaptation

Commit message: `feat(gltf): adapt glTF cameras and KHR_lights_punctual into scene components`

### Step 1: Implement crates/gltf/src/cameras.rs

```rust
//! glTF camera → CameraComponent adaptation.

use rig_math::Projection;
use rig_scene::{CameraComponent, NodeId, SceneGraph};

/// Attach camera components to scene nodes that have glTF cameras.
pub(crate) fn adapt_cameras(
    document: &gltf::Document,
    node_map: &[Option<NodeId>],
    scene: &mut SceneGraph,
)
```

For each node in `document.nodes()` that has `node.camera()`:
- Look up `node_map[node.index()]` → skip if `None`.
- Match `camera.projection()`:
  - `Projection::Perspective(p)` → `rig_math::Projection::Perspective { fov_y: p.yfov(), near: p.znear(), far: p.zfar().unwrap_or(1000.0) }`.
  - `Projection::Orthographic(_)` → log a warning (`log::warn!`) and skip (engine does not yet support orthographic cameras).
- Call `scene.set_camera(node_id, CameraComponent { projection })`.

### Step 2: Implement crates/gltf/src/lights.rs

```rust
//! KHR_lights_punctual → LightComponent adaptation.

use rig_math::Vec3;
use rig_scene::{LightComponent, LightKind, NodeId, SceneGraph};

/// Attach light components to scene nodes that have KHR_lights_punctual lights.
pub(crate) fn adapt_lights(
    document: &gltf::Document,
    node_map: &[Option<NodeId>],
    scene: &mut SceneGraph,
)
```

For each node in `document.nodes()` that has `node.light()` (requires `KHR_lights_punctual` feature):
- Look up `node_map[node.index()]` → skip if `None`.
- Match `light.kind()`:
  - `Kind::Directional` → `LightKind::Directional { color: Vec3::from(light.color()), intensity: light.intensity() }`.
  - `Kind::Point` → `LightKind::Point { color: Vec3::from(light.color()), intensity: light.intensity(), range: light.range().unwrap_or(20.0) }`.
  - `Kind::Spot` → `log::warn!("glTF spot lights are not yet supported, skipping"); continue;`
- Call `scene.set_light(node_id, LightComponent { kind })`.

---

## Phase 8: Animation adaptation

Commit message: `feat(gltf): adapt glTF animations into AnimationClip assets`

### Step 1: Implement crates/gltf/src/animations.rs

```rust
//! glTF animation → AnimationClip adaptation.

use rig_assets::{
    AnimationChannel, AnimationClip, AnimationClipHandle, AssetStore, ChannelProperty,
    KeyframeSampler, KeyframeValues,
};
use rig_math::{Interpolation, Quat, Vec3};
use rig_scene::{NodeId, SceneGraph};

use crate::buffers;
use crate::error::Result;

/// Adapt all glTF animations into `AnimationClip` assets registered in `store`.
///
/// Returns clip handles indexed by glTF animation index.
pub(crate) fn adapt_animations(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    node_map: &[Option<NodeId>],
    scene: &SceneGraph,
    store: &mut AssetStore,
) -> Result<Vec<AnimationClipHandle>>
```

For each `gltf::Animation`:
1. `let name = animation.name().unwrap_or("animation").to_string();`
2. Build `channels: Vec<AnimationChannel>`:
   - For each `channel` in `animation.channels()`:
     - `let target_node_idx = channel.target().node().index();`
     - Look up `node_map[target_node_idx]` → if `None`, skip.
     - Get node name: `scene.node_name(node_id).unwrap_or("unknown").to_string()`.
     - Map `channel.target().property()`:
       - `Property::Translation` → `ChannelProperty::Translation`
       - `Property::Rotation` → `ChannelProperty::Rotation`
       - `Property::Scale` → `ChannelProperty::Scale`
       - `Property::MorphTargetWeights` → `log::warn!` and skip.
     - Read sampler: `let sampler = channel.sampler();`
     - `let times = buffers::read_timestamps(&sampler, buffers);`
     - Map `sampler.interpolation()`:
       - `Interpolation::Linear` → `Interpolation::Linear`
       - `Interpolation::Step` → `Interpolation::Step`
       - `Interpolation::CubicSpline` → `Interpolation::CubicSpline` (extract middle values from 3N output)
     - Build `KeyframeValues` based on property:
       - Translation → `buffers::read_anim_translations(&sampler, buffers)` → `KeyframeValues::Translations(vec)`
       - Rotation → `buffers::read_anim_rotations(&sampler, buffers)` → `KeyframeValues::Rotations(vec)`
       - Scale → `buffers::read_anim_scales(&sampler, buffers)` → `KeyframeValues::Scales(vec)`
     - Push `AnimationChannel { target_node: name, property, sampler: KeyframeSampler { times, interpolation, values } }`.
3. Compute `duration = channels.iter().flat_map(|c| c.sampler.times.iter().copied()).fold(0.0_f32, f32::max);`
4. Register `AnimationClip { name, duration, looping: true, channels }` → `store.add_animation_clip(clip)`.

**CubicSpline note:** glTF stores `[in_tangent, value, out_tangent]` per keyframe.
For the initial implementation, extract only the `value` (middle element of each triple).
Add a `// TODO: full cubic spline support` comment.

---

## Phase 9: Skin adaptation

Commit message: `feat(gltf): adapt glTF skins and per-primitive skin weights into SkinAsset and SkinWeights`

### Step 1: Implement crates/gltf/src/skins.rs

```rust
//! glTF skin and skinning weight adaptation.

use rig_assets::{AssetStore, SkinAsset, SkinAssetHandle, SkinWeights, SkinWeightsHandle};
use rig_scene::{NodeId, SceneGraph};

use crate::buffers;
use crate::error::Result;

/// Adapt all glTF skins into `SkinAsset` values registered in `store`.
///
/// Returns handles indexed by glTF skin index.
pub(crate) fn adapt_skins(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    node_map: &[Option<NodeId>],
    scene: &SceneGraph,
    store: &mut AssetStore,
) -> Result<Vec<SkinAssetHandle>>
```

For each `gltf::Skin`:
1. Collect `joint_names`: for each joint node, look up `node_map[joint.index()]` → `scene.node_name(id).to_string()`. If node not found, use `format!("joint_{}", joint.index())`.
2. Read `inverse_bind_matrices` via `buffers::read_inverse_bind_matrices(&skin, buffers)`.
3. Register `SkinAsset { joint_names, inverse_bind_matrices }` → `store.add_skin(asset)`.

```rust
/// Adapt skinning weights for a single glTF primitive.
///
/// Returns `None` if the primitive has no JOINTS_0 attribute.
pub(crate) fn adapt_skin_weights(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
    store: &mut AssetStore,
) -> Result<Option<SkinWeightsHandle>>
```

1. Read `joints_0 = buffers::read_joints(primitive, buffers, 0)` → if `None`, return `Ok(None)`.
2. Read `weights_0 = buffers::read_weights(primitive, buffers, 0)` → if `None`, return `Ok(None)`.
3. Read `joints_1 = buffers::read_joints(primitive, buffers, 1).unwrap_or_default()`.
4. Read `weights_1 = buffers::read_weights(primitive, buffers, 1).unwrap_or_default()`.
5. Build `SkinWeights::from_gltf_sets(&joints_0, &weights_0, &joints_1, &weights_1)`.
6. Register → `Ok(Some(store.add_skin_weights(weights)))`.

---

## Phase 10: Top-level loader API

Commit message: `feat(gltf): implement load_gltf entry point and LoadedGltf output type`

### Step 1: Implement crates/gltf/src/loader.rs

```rust
//! Top-level glTF loading entry point.

use std::path::Path;

use rig_assets::{
    AnimationClipHandle, AssetStore, MaterialHandle, MeshHandle, MeshSource, ShaderHandle,
    SkinAssetHandle, SkinWeightsHandle,
};
use rig_scene::{NodeId, Renderable, SceneGraph};

use crate::{animations, cameras, lights, materials, meshes, nodes, skins, textures};
use crate::error::Result;

/// Result of loading a glTF file into the engine.
pub struct LoadedGltf {
    /// Top-level scene nodes (roots of the glTF scene hierarchy).
    pub root_nodes: Vec<NodeId>,
    /// All created nodes, indexed by glTF node index. `None` for unused slots.
    pub node_map: Vec<Option<NodeId>>,
    /// Animation clip handles, one per glTF animation (in document order).
    pub animations: Vec<AnimationClipHandle>,
    /// Skin asset handles, one per glTF skin (in document order).
    pub skins: Vec<SkinAssetHandle>,
    /// Mesh handles grouped by glTF mesh index, then primitive index.
    pub meshes: Vec<Vec<MeshHandle>>,
    /// Material handles, one per glTF material (in document order).
    pub materials: Vec<MaterialHandle>,
    /// Skin weights handles per primitive, parallel to `meshes`.
    pub skin_weights: Vec<Vec<Option<SkinWeightsHandle>>>,
}

/// Load a glTF or GLB file and adapt its contents into the engine.
///
/// All materials are assigned `shader`. The loaded scene hierarchy is
/// populated into `scene`; all assets are registered in `store`.
pub fn load_gltf(
    path: impl AsRef<Path>,
    shader: ShaderHandle,
    scene: &mut SceneGraph,
    store: &mut AssetStore,
) -> Result<LoadedGltf>
```

Implementation (in order):
1. `let (document, buffers, images) = gltf::import(path.as_ref())?;`
2. `let image_handles = textures::adapt_images(&images, store);`
3. `let sampler_handles = textures::adapt_samplers(&document, store);`
4. `let default_sampler = textures::default_sampler(store);`
5. `let material_handles = materials::adapt_materials(&document, &image_handles, &sampler_handles, default_sampler, shader, store);`
6. `let (node_map, root_nodes) = nodes::adapt_nodes(&document, scene);`
7. `scene.update_all_world_transforms().ok();`
8. Adapt meshes and wire renderables:
   ```
   let mut mesh_handles: Vec<Vec<MeshHandle>> = Vec::new();
   let mut skin_weight_handles: Vec<Vec<Option<SkinWeightsHandle>>> = Vec::new();
   for gltf_mesh in document.meshes() {
       let primitives = meshes::adapt_mesh(&gltf_mesh, &buffers, store)?;
       let mut prim_mesh_handles = Vec::new();
       let mut prim_weight_handles = Vec::new();
       for (i, (mesh_handle, mat_idx)) in primitives.iter().enumerate() {
           prim_mesh_handles.push(*mesh_handle);
           let weights = skins::adapt_skin_weights(
               &gltf_mesh.primitives().nth(i).unwrap(),
               &buffers,
               store,
           )?;
           prim_weight_handles.push(weights);
       }
       mesh_handles.push(prim_mesh_handles);
       skin_weight_handles.push(prim_weight_handles);
   }
   ```
9. Wire renderables: for each glTF node with a mesh, look up `node_map[node.index()]`,
   then for each primitive of that mesh, create a child node (or use the node itself for
   single-primitive meshes) and call `scene.set_renderable(node_id, Renderable { mesh: MeshSource::Static(mesh_handle), material: material_handles[mat_idx] })`.
   - **Multi-primitive strategy:** if a mesh has exactly 1 primitive, attach the renderable
     directly to the node. If it has >1 primitives, create child nodes named
     `"{node_name}_prim_{i}"` and attach each renderable to a child.
10. `cameras::adapt_cameras(&document, &node_map, scene);`
11. `lights::adapt_lights(&document, &node_map, scene);`
12. `let animation_handles = animations::adapt_animations(&document, &buffers, &node_map, scene, store)?;`
13. `let skin_handles = skins::adapt_skins(&document, &buffers, &node_map, scene, store)?;`
14. Return `LoadedGltf { root_nodes, node_map, animations: animation_handles, skins: skin_handles, meshes: mesh_handles, materials: material_handles, skin_weights: skin_weight_handles }`.

---

## Phase 11: Integrate into rig-app

Commit message: `feat(app): re-export rig-gltf through rig-app`

### Step 1: Add rig-gltf to crates/app/Cargo.toml

In `[dependencies]`, add `rig-gltf.workspace = true` after `rig-skin.workspace = true`.

### Step 2: Re-export in crates/app/src/lib.rs

After `pub use rig_skin;`, add:
```rust
pub use rig_gltf;
```

---

## Phase 12: glTF demo example

Commit message: `feat(examples): add gltf_demo example showcasing glTF model loading with full PBR`

### Step 1: Create examples/gltf_demo/Cargo.toml

```toml
[package]
name = "gltf_demo"
version = "0.1.0"
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Phase D: glTF model loading with full PBR materials"

[dependencies]
rig-app.workspace = true
env_logger.workspace = true
anyhow.workspace = true
```

Add `"examples/gltf_demo"` to root `Cargo.toml` workspace members.

### Step 2: Create examples/gltf_demo/src/main.rs

The demo accepts an optional CLI argument for the glTF file path (defaults to
`"assets/models/gltf/DamagedHelmet.glb"`). It demonstrates:

- `load_gltf()` loading a GLB file.
- Full PBR rendering with normal map, emissive, and occlusion (DamagedHelmet has all five slots).
- `TrackBall` orbit/dolly controls (left-drag to orbit, scroll to dolly).
- `CameraRig` WASD/arrow-key fly controls.
- `DebugHud` with FPS counter and model name.
- If the model has animations, creates an `AnimationPlayer` and plays the first clip.

```rust
use anyhow::Result;
use rig_app::{
    Application, CameraRig, DebugHud, InputState, OverlayUpdateContext, RenderContext,
    RunConfig, Side, StartupContext, TrackBall, UpdateContext, rig_anim::AnimationPlayer,
    rig_assets::{AssetStore, ShaderHandle, ShaderAsset}, rig_gltf::{load_gltf, LoadedGltf},
    rig_math::{Projection, Transform, Vec3}, rig_render::helpers::PBR_SHADER,
    rig_scene::{CameraComponent, LightComponent, LightKind, SceneGraph},
    run,
};
use std::sync::Arc;

struct GltfDemo {
    camera: rig_scene::NodeId,
    trackball: TrackBall,
    camera_rig: CameraRig,
    hud: DebugHud,
    animation_player: Option<AnimationPlayer>,
    loaded: LoadedGltf,
}

impl Application for GltfDemo {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let path = std::env::args().nth(1)
            .unwrap_or_else(|| "assets/models/gltf/DamagedHelmet.glb".to_string());

        // PBR shader
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PBR_SHADER),
        });

        // Load glTF
        let loaded = load_gltf(&path, shader, ctx.scene, ctx.assets)?;

        // Camera
        let camera = ctx.scene.create_node("camera");
        ctx.scene.set_camera(camera, CameraComponent {
            projection: Projection::Perspective { fov_y: 0.8, near: 0.1, far: 500.0 },
        })?;
        ctx.scene.set_local_transform(camera, Transform {
            translation: Vec3::new(0.0, 0.0, 3.0),
            ..Transform::IDENTITY
        })?;
        *ctx.active_camera = Some(camera);

        // Directional light (if model has no lights)
        if ctx.scene.light_nodes().is_empty() {
            let light_node = ctx.scene.create_node("sun");
            ctx.scene.set_light(light_node, LightComponent {
                kind: LightKind::Directional {
                    color: Vec3::new(1.0, 0.95, 0.9),
                    intensity: 3.0,
                },
            })?;
            ctx.scene.set_local_transform(light_node, Transform {
                rotation: rig_math::Quat::from_rotation_x(-0.6),
                ..Transform::IDENTITY
            })?;
        }

        // TrackBall targeting scene origin
        let target = ctx.scene.create_node("target");
        let trackball = TrackBall::new(target, 3.0);

        // Animation player for first clip (if any)
        let animation_player = loaded.animations.first().map(|&clip_handle| {
            let mut player = AnimationPlayer::new(clip_handle);
            player.bind(ctx.assets, ctx.scene).ok();
            player
        });

        // HUD
        let mut hud = DebugHud::new();
        hud.add_label("fps", Side::TopLeft, "FPS: --");
        hud.add_label("model", Side::TopLeft, &format!("Model: {path}"));

        Ok(Self {
            camera,
            trackball,
            camera_rig: CameraRig { translation_speed: 4.0, rotation_speed: 1.0 },
            hud,
            animation_player,
            loaded,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        self.trackball.update(ctx.input, ctx.scene, self.camera, dt)?;
        self.camera_rig.update(ctx, self.camera, dt)?;

        if let Some(player) = &mut self.animation_player {
            player.advance(dt);
            player.evaluate(ctx.assets, ctx.scene)?;
        }

        ctx.scene.update_all_world_transforms()?;
        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
        ctx.renderer.render_scene(
            ctx.gpu,
            ctx.frame,
            ctx.scene,
            ctx.assets,
            *ctx.active_camera,
        )?;
        Ok(())
    }

    fn update_overlay(&mut self, ctx: &mut OverlayUpdateContext<'_>) -> Result<()> {
        self.hud.update_fps(ctx, ctx.timer.fps());
        self.hud.render(ctx)?;
        Ok(())
    }
}

fn main() -> Result<()> {
    env_logger::init();
    run::<GltfDemo>(RunConfig {
        title: "rig — glTF Demo".into(),
        ..Default::default()
    })
}
```

---

## Phase 13: Tests

Commit message: `test(gltf): add unit tests for buffer helpers, material adaptation, and mesh interleaving`

### Step 1: Unit tests in meshes.rs

Test `generate_smooth_normals` with a known triangle:
- Single triangle with vertices at `(0,0,0)`, `(1,0,0)`, `(0,1,0)` → normals should all be `(0,0,1)`.

Test `compute_bounding_sphere`:
- Unit cube vertices → sphere radius ≈ `sqrt(3)/2`.

Test `pack_indices_u16` / `pack_indices_u32`:
- Known index values → expected byte sequences.

### Step 2: Unit tests in materials.rs

Test `adapt_materials` with a document that has no materials → returns empty vec.

Test material parameter mapping:
- Default glTF material (all defaults) → `diffuse = [1,1,1,1]`, `metallic = 1.0`, `roughness = 1.0`.
- All 5 texture slots are `None` when no textures are present.

### Step 3: Integration test (optional, if a minimal GLB fixture is available)

Create `crates/gltf/tests/load_minimal.rs`:
```rust
// Requires a minimal 1-triangle GLB fixture at tests/fixtures/triangle.glb
// Generate with: gltf-transform merge (or embed raw bytes)
#[test]
fn load_minimal_glb_produces_one_mesh() {
    // ... load_gltf("tests/fixtures/triangle.glb", ...) ...
}
```

If no fixture is available, skip this test and note it as a follow-up.

---

## Risks and known limitations

| Item | Impact | Mitigation |
|---|---|---|
| **Sparse accessors** | Low — rare in practice | Return `GltfError::SparseAccessor` if encountered. Document as unsupported. |
| **Morph targets** | Low | Log warning and skip. |
| **Spot lights** | Low | Log warning and skip. |
| **Orthographic cameras** | Low | Log warning and skip. |
| **CubicSpline animation** | Medium — affects skinned models | Extract middle values only. Full cubic support is a follow-up. |
| **Texture color space** | Medium — visual correctness | All images stored as `Rgba8Unorm`. The PBR shader handles linear data correctly for normal/MR/occlusion. Base color sRGB correction is a follow-up (add `Rgba8UnormSrgb` variant in material adaptation). |
| **Multi-primitive meshes** | High — common in real models | Handled: single-primitive → attach to node directly; multi-primitive → create child nodes. |
| **Missing test GLB fixture** | Low | Unit tests cover helpers; integration test is optional. |
| **`gltf` crate `KHR_lights_punctual` feature** | Low | Feature is declared in workspace Cargo.toml. If `node.light()` is not available, the lights module compiles to a no-op. |

---

## Dependency graph (after implementation)

```
rig-math ──► rig-assets ──► rig-gltf ──► rig-app ──► examples/gltf_demo
                 │                            ▲
                 └────────────────────────────┘
             (tangent_utils, SkinWeights, AnimationClip, etc.)

rig-scene ──────────────────► rig-gltf
rig-math  ──────────────────► rig-gltf
gltf (external) ────────────► rig-gltf
```
