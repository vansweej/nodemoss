# Milestone 3+ Implementation Plan

Four sequential commits that bring the framework from "spinning triangle" to a lit,
textured, culled scene with proper camera management.

Each commit is self-contained: tests pass, clippy is clean, the example still runs.

---

## Commit 1 — Active Camera Selection from Scene

### Goal

Promote the active-camera concept from an opaque `Option<NodeId>` managed entirely
by application code into a first-class scene-graph query, so the renderer and app
layer can discover cameras by convention rather than manual bookkeeping.

### What exists today

| Item | Location | Status |
|------|----------|--------|
| `CameraComponent` (projection only) | `rig-scene/src/lib.rs:29` | Done |
| `cameras` component map | `SceneGraph` field | Done |
| `set_camera()` / `camera()` | `SceneGraph` methods | Done |
| `active_camera: Option<NodeId>` | `UpdateContext` / `RenderContext` in `rig-app` | Done |
| `CameraRig` utility | `rig-app/src/lib.rs:62` | Done |

### What to add

#### rig-scene

1. **`SceneGraph::camera_nodes() -> Vec<NodeId>`** — return all nodes that have a
   `CameraComponent` attached. O(cameras) scan of the `cameras` HashMap keys.

2. **`SceneGraph::first_camera() -> Option<NodeId>`** — convenience: returns the
   first camera node found (useful for single-camera apps that want to skip
   manual `active_camera` wiring).

3. **`SceneGraph::camera_with_name(name: &str) -> Option<NodeId>`** — look up a
   camera by its node name. Enables named-camera selection ("main", "debug",
   "minimap") without application code tracking handles.

4. **`ExtractedCamera` struct:**
   ```rust
   pub struct ExtractedCamera {
       pub node: NodeId,
       pub projection: Projection,
       pub world_transform: Mat4,
   }
   ```

5. **`SceneGraph::extract_active_camera(id: NodeId) -> Result<ExtractedCamera>`** —
   given a camera NodeId, extract its projection + world transform in one call.
   The renderer currently does this inline; move the logic to rig-scene so it is
   testable.

#### rig-render

6. **Refactor `render_draw_list`** to accept `ExtractedCamera` instead of raw
   `Option<NodeId>` + internal scene queries. This removes the renderer's direct
   coupling to `SceneGraph` for camera lookup.

#### rig-app

7. **Auto-select camera on startup**: if the application's `init()` leaves
   `active_camera` as `None`, the runner calls `scene.first_camera()` as a
   fallback before the first render frame. Explicit selection always wins.

### Tests

- `camera_nodes_returns_all_cameras` — create 3 nodes, attach cameras to 2,
  verify `camera_nodes()` returns exactly 2.
- `first_camera_returns_none_for_empty_scene` — no cameras → `None`.
- `first_camera_returns_some_for_scene_with_camera` — one camera → `Some`.
- `camera_with_name_finds_matching_camera` — create "main" and "debug" cameras,
  look up each by name.
- `camera_with_name_returns_none_for_non_camera` — node exists but has no camera
  component → `None`.
- `extract_active_camera_computes_world_transform` — attach camera to child node,
  update transforms, extract, verify world transform is parent * local.
- `extract_active_camera_errors_for_non_camera` — non-camera node → error.

### Inspiration from GeometricTools

GTE does not have engine-level camera selection; `Window3` just owns a single
`shared_ptr<Camera>` and passes it everywhere. Our approach is better: cameras
live in the scene graph and can be queried/swapped at runtime.

---

## Commit 2 — Lights and Material Models

### Goal

Extend `MaterialAsset` with Blinn-Phong color properties, extract lights from the
scene, upload light + material uniforms, and write a directional lighting shader.

### What exists today

| Item | Location | Status |
|------|----------|--------|
| `LightComponent` / `LightKind` | `rig-scene/src/lib.rs:33-49` | Done |
| `lights` component map | `SceneGraph` field | Done |
| `set_light()` / `light()` | `SceneGraph` methods | Done |
| `MaterialAsset { shader }` | `rig-assets/src/lib.rs:71` | Shader only |
| `VertexFormat::Float32x3` | `rig-assets/src/lib.rs:58` | No normals yet |

### What to add

#### rig-assets

1. **`MaterialParams` struct:**
   ```rust
   pub struct MaterialParams {
       pub ambient: [f32; 4],    // default [0.2, 0.2, 0.2, 1.0]
       pub diffuse: [f32; 4],    // default [0.8, 0.8, 0.8, 1.0]
       pub specular: [f32; 4],   // xyz = color, w = shininess power
       pub emissive: [f32; 4],   // default [0.0, 0.0, 0.0, 1.0]
   }
   ```
   Stored as `[f32; 4]` arrays (not glam types) so it can derive `Pod`/`Zeroable`
   for direct GPU upload.

2. **Extend `MaterialAsset`:**
   ```rust
   pub struct MaterialAsset {
       pub shader: ShaderHandle,
       pub parameters: MaterialParams,
   }
   ```
   `textures` field deferred to Commit 4.

3. **`VertexFormat::Float32x2`** — needed later for UVs but cheap to add now for
   completeness.

#### rig-scene

4. **`ExtractedLight` struct:**
   ```rust
   pub struct ExtractedLight {
       pub kind: LightKind,
       pub world_position: Vec3,
       pub world_direction: Vec3,
   }
   ```

5. **`SceneGraph::extract_lights() -> Vec<ExtractedLight>`** — iterate the `lights`
   component map, read each node's world transform, compute position/direction
   from the transform's translation and forward vector.

#### rig-render

6. **`LightUniforms` / `MaterialUniforms` GPU structs:**
   ```rust
   #[repr(C)]
   struct LightUniforms {
       direction: [f32; 4],    // world-space, w=0 for directional
       color: [f32; 4],        // rgb + intensity
       ambient: [f32; 4],      // scene ambient
   }

   #[repr(C)]
   struct MaterialUniforms {
       ambient: [f32; 4],
       diffuse: [f32; 4],
       specular: [f32; 4],
       emissive: [f32; 4],
   }
   ```

7. **New bind group (group 1)** for scene-wide uniforms (camera + light data).
   The current group 0 remains for per-object data (world matrix). Layout:
   - binding 0: view uniform buffer (camera matrices — view, proj, camera position)
   - binding 1: light uniform buffer (first directional light)

8. **New bind group (group 2)** for per-material uniforms:
   - binding 0: material properties buffer

9. **Update `PipelineKey`** — no changes needed yet; shader handle + vertex layout
   is still sufficient since we are adding bind groups to the same pipeline layout.

10. **Update `pipeline_layout`** to include groups 0, 1, 2.

11. **Write `BLINN_PHONG_SHADER` (WGSL)** — embedded via `include_str!` or as a
    constant. Vertex stage: transform position by world matrix, pass world-space
    normal. Fragment stage: Blinn-Phong with one directional light.

12. **`FrameResources` additions**: `view_uniforms` buffer, `light_uniforms` buffer
    (both per-frame, uploaded each render call).

#### example

13. **Update `triangle_scenegraph`** to use `MaterialParams` with visible diffuse
    color, add a directional light node to the scene. Vertex data gains normals
    (already has position + color; replace color attribute with normal, or add a
    third attribute).

### Tests

- `material_params_default_is_sensible` — default should have non-zero diffuse.
- `extract_lights_returns_empty_for_no_lights` — clean scene → empty vec.
- `extract_lights_computes_world_direction` — rotated light node → rotated forward
  vector in extracted direction.
- `extract_lights_includes_point_position` — point light → world position from
  transform translation.
- Existing asset store tests extended for the new `parameters` field.

### Inspiration from GeometricTools

GTE uses three separate constant buffers (Material, Lighting, LightCameraGeometry)
per draw call. We simplify: one scene-wide light buffer (bind group 1) and one
per-material buffer (bind group 2). GTE computes lighting in model space; we
compute in world space (simpler, GPU handles the extra work).

GTE's `Material` packs shininess into `specular.w` — we adopt the same convention.

---

## Commit 3 — Frustum Culling

### Goal

Skip rendering objects whose bounding spheres are entirely outside the camera
frustum. Integrate the existing `frustum_planes_from_projection_view()` with a
sphere-vs-plane test and hook it into `extract_renderables()`.

### What exists today

| Item | Location | Status |
|------|----------|--------|
| `frustum_planes_from_projection_view()` | `rig-scene/src/lib.rs:415` | Done |
| `BoundingSphere` with transform/union | `rig-math/src/lib.rs` | Done |
| `ExtractedRenderable.world_bound` | `rig-scene/src/lib.rs:412` | Populated |
| `VisibilityMode` enum | `rig-scene/src/lib.rs:16` | Inherit/AlwaysVisible/Hidden |
| `update_world_bounds()` | `SceneGraph` method | Done |
| `world_bound` per node | `SceneNode` field | Done |

### What to add

#### rig-math

1. **`BoundingSphere::is_outside_plane(plane: Vec4) -> bool`** — signed distance
   test: `dot(center, plane.xyz) + plane.w < -radius`. Returns `true` if the
   sphere is entirely on the negative side of the plane.

2. **`BoundingSphere::is_outside_frustum(planes: &[Vec4; 6]) -> bool`** —
   convenience: returns `true` if the sphere is outside any of the 6 planes.

#### rig-scene

3. **`SceneGraph::extract_renderables_culled(frustum_planes: &[Vec4; 6]) -> Vec<ExtractedRenderable>`**
   — like `extract_renderables()` but skips nodes whose `world_bound` fails the
   frustum test. Respects `VisibilityMode`:
   - `Hidden` → always culled (existing behavior)
   - `AlwaysVisible` → skip frustum test, always included
   - `Inherit` → normal frustum test

4. **Hierarchical early-out (stretch):** If a parent's world bound is entirely
   inside the frustum, skip testing children. If entirely outside, cull the entire
   subtree. This matches GTE's `mPlaneState` bitmask approach but can be deferred
   to a follow-up if the flat renderable iteration is fast enough.

#### rig-render

5. **Call `extract_renderables_culled`** in `render_scene()` instead of
   `extract_renderables()`. Compute frustum planes from the camera's
   projection-view matrix and pass them through.

### Tests

- `sphere_outside_plane_detects_negative_halfspace` — sphere at (0,0,-10) with
  radius 1, near plane at z=0 → outside.
- `sphere_inside_plane_returns_false` — sphere at (0,0,5) → not outside.
- `sphere_straddling_plane_returns_false` — sphere at (0,0,0) with radius 2,
  plane at z=1 → straddles, not outside.
- `sphere_outside_frustum_any_plane` — outside one plane → outside frustum.
- `sphere_inside_all_planes` — inside all 6 → not outside frustum.
- `extract_renderables_culled_excludes_outside_objects` — place one object inside
  and one outside, verify only the inside one is extracted.
- `extract_renderables_culled_always_includes_always_visible` — object outside
  frustum but `AlwaysVisible` → still extracted.
- `extract_renderables_culled_always_excludes_hidden` — object inside frustum but
  `Hidden` → not extracted.

### Inspiration from GeometricTools

GTE's `Culler` extracts frustum planes geometrically from the camera frame
(position, axes, near/far/fov). We use the equivalent matrix-extraction method
(`frustum_planes_from_projection_view`), which is already implemented. Both
produce the same 6 oriented planes.

GTE propagates a `mPlaneState` bitmask through the tree: when a parent is fully
inside a plane, children skip that plane's test. We note this as a stretch goal
but start with the simpler flat-iteration approach since the renderable count
is small in early milestones.

---

## Commit 4 — Texture Support

### Goal

Load RGBA8 textures into the asset store, cache them as GPU textures, create
samplers, add a texture bind group, and write a textured (or lit+textured) shader.

### What exists today

| Item | Location | Status |
|------|----------|--------|
| `TextureAsset { width, height, data }` | `rig-assets/src/lib.rs:81` | No format field |
| `TextureHandle` | `rig-assets/src/lib.rs:17` | Done |
| `AssetStore::add_texture()` / `texture()` | `rig-assets/src/lib.rs:130` | Done |
| `ImmutableResourceCache` | `rig-render/src/lib.rs:166` | Shaders + meshes only |

### What to add

#### rig-assets

1. **`TextureFormat` enum:**
   ```rust
   pub enum TextureFormat {
       Rgba8Unorm,
       Rgba8UnormSrgb,
   }
   ```
   Start with just two formats. Map to `wgpu::TextureFormat` in the renderer.

2. **Extend `TextureAsset`:**
   ```rust
   pub struct TextureAsset {
       pub width: u32,
       pub height: u32,
       pub format: TextureFormat,
       pub data: Arc<[u8]>,
   }
   ```

3. **`SamplerDescriptor` struct:**
   ```rust
   pub struct SamplerDescriptor {
       pub address_mode_u: AddressMode,
       pub address_mode_v: AddressMode,
       pub mag_filter: FilterMode,
       pub min_filter: FilterMode,
   }

   pub enum AddressMode { ClampToEdge, Repeat, MirrorRepeat }
   pub enum FilterMode { Nearest, Linear }
   ```
   A renderer-agnostic sampler description. Mapped to `wgpu::SamplerDescriptor`
   in rig-render.

4. **`SamplerHandle` + `AssetStore::add_sampler()` / `sampler()`** — sampler
   assets are immutable descriptions, not GPU objects.

5. **Extend `MaterialAsset`:**
   ```rust
   pub struct MaterialAsset {
       pub shader: ShaderHandle,
       pub parameters: MaterialParams,
       pub textures: Vec<(TextureHandle, SamplerHandle)>,
   }
   ```
   Each texture slot is a (texture, sampler) pair. Most materials will have 0 or 1
   entries.

6. **`VertexFormat::Float32x2`** — for UV coordinates (may already exist from
   Commit 2).

#### rig-render

7. **Extend `ImmutableResourceCache`:**
   ```rust
   textures: HashMap<u64, wgpu::Texture>,
   texture_views: HashMap<u64, wgpu::TextureView>,
   samplers: HashMap<u64, wgpu::Sampler>,
   ```
   Add `gpu_texture()`, `texture_view()`, `sampler()` cache methods that create
   on first access and return cloned handles.

8. **New bind group (group 3)** for textures:
   - binding 0: texture view
   - binding 1: sampler

   For materials with no textures, bind a 1x1 white fallback texture (avoids
   pipeline permutations for textured vs untextured).

9. **Update `pipeline_layout`** to include group 3.

10. **Write `TEXTURED_SHADER` (WGSL)** — samples texture at UV, multiplies with
    material diffuse color. Optionally a `LIT_TEXTURED_SHADER` that combines
    Blinn-Phong lighting with texture sampling.

11. **`texture_format_to_wgpu()`** helper — maps `rig_assets::TextureFormat` to
    `wgpu::TextureFormat`.

#### example

12. **New example or update existing** — render a textured quad (two triangles)
    with a checkerboard pattern generated procedurally (no image loading
    dependency yet). Demonstrates UV mapping, texture upload, and sampling.

### Tests

- `texture_format_maps_correctly` — verify Rgba8Unorm → wgpu::TextureFormat::Rgba8Unorm.
- `sampler_descriptor_default` — default sampler uses linear filtering and clamp.
- `material_with_textures_stores_pairs` — add a material with one texture+sampler
  pair, retrieve it, verify the pair.
- `sampler_handle_round_trips` — add sampler to store, retrieve by handle.
- `texture_asset_with_format_round_trips` — add texture with format field, verify.
- Existing tests updated wherever `MaterialAsset` or `TextureAsset` constructors
  changed.

### Inspiration from GeometricTools

GTE's textures are effect-owned, not material-owned. We diverge: textures are
referenced from `MaterialAsset` via handles, which is more data-driven and
avoids the deep `Effect` class hierarchy.

GTE's `SamplerState` is a first-class object with filter/wrap/LOD/anisotropy —
we adopt the same idea with our `SamplerDescriptor`, but keep it minimal (no LOD
or anisotropy fields yet).

GTE's `Texture2Effect` pairs one texture with one sampler and binds them together.
Our bind group 3 does the same thing, but extends naturally to multi-texture
materials by adding more bindings.

---

## Dependency Order

```
Commit 1 (Camera)  ←  independent
Commit 2 (Lights)  ←  independent of Commit 1 (but applied after)
Commit 3 (Culling)  ←  uses camera from Commit 1 for frustum planes
Commit 4 (Textures) ←  extends MaterialAsset from Commit 2
```

Commits 1 and 2 are logically independent but ordered so the renderer
refactoring in Commit 1 (ExtractedCamera) simplifies Commit 2's bind group
changes. Commit 3 depends on camera extraction. Commit 4 depends on the
material model from Commit 2.

## Verification

After each commit:

```bash
nix develop -c cargo fmt --all
nix develop -c cargo test --workspace
nix develop -c cargo clippy --workspace -- -D warnings
nix develop -c cargo tarpaulin --engine llvm   # coverage should not regress
```
