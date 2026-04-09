# Milestone 3+ Implementation Plan: Multiple Objects & Offscreen Passes

Two features that share a critical prerequisite (the depth buffer) and
together bring the framework from "single spinning triangle" to a scene
renderer capable of displaying many objects with correct depth and rendering
to offscreen targets.

Each commit is self-contained: tests pass, clippy is clean, the example
still runs.

---

## Commit 1 — Depth Buffer and Pipeline Depth State

### Goal

Add a depth texture to the main render pass so overlapping geometry is
drawn correctly. This is the single biggest blocker for rendering multiple
objects and the foundation for offscreen passes.

### What exists today

| Item | Location | Status |
|------|----------|--------|
| Single render pass per frame | `render/src/lib.rs:413` | Color-only |
| `depth_stencil_attachment: None` | `render/src/lib.rs:429` | No depth |
| `depth_stencil: None` in pipeline | `render/src/lib.rs:568` | No depth test |
| `PipelineKey { shader, vertex_layout }` | `render/src/lib.rs:51-55` | Missing depth format |
| `Renderer::resize()` | `render/src/lib.rs:321` | No depth texture resize |

### What to add

#### rig-render

1. **`depth_texture` and `depth_view` fields on `Renderer`** — created at
   init with `TextureFormat::Depth32Float`, sized to match the surface. Usage
   flag: `RENDER_ATTACHMENT`.

2. **Recreate depth texture on resize** — `Renderer::resize()` must create a
   new depth texture at the new dimensions. Extract a
   `create_depth_texture(device, width, height)` helper.

3. **Attach depth in the render pass** — set `depth_stencil_attachment` to
   the depth view with `depth_ops: LoadOp::Clear(1.0)`, `stencil_ops: None`,
   `store: StoreOp::Store`.

4. **Add `DepthStencilState` to pipeline creation** — `depth_write_enabled:
   true`, `depth_compare: Less`, `format: Depth32Float`.

5. **Extend `PipelineKey` with `depth_format: Option<wgpu::TextureFormat>`**
   — pipelines created without depth are incompatible with passes that have
   depth. Including the depth format in the key invalidates stale pipelines.
   Also add `color_format: wgpu::TextureFormat` to the key for forward
   compatibility with offscreen passes that use different formats.

6. **Invalidate the pipeline cache** — since the existing pipelines were
   created without depth state, the cache will naturally create new pipelines
   with the updated key after the depth format is added.

### Tests

- `create_depth_texture_returns_correct_dimensions` — unit test for the
  helper that creates the depth texture.
- `pipeline_key_differs_with_depth_format` — two PipelineKeys that are
  identical except for depth format should not be equal.
- Existing `triangle_shader_mentions_expected_entry_points` test still passes
  (shader itself is unchanged).

### Example

- **Update `examples/triangle_scenegraph/`** so the existing triangle
  renders with correct depth testing enabled. No visual change (single
  object), but confirms the depth pipeline path works and does not regress
  the existing example.

### Inspiration from GeometricTools

GTE's `DrawTarget` always optionally bundles a `TextureDS` (depth-stencil).
The engine's `Enable(DrawTarget)` attaches both color and depth at the same
time. Our `wgpu` approach is simpler: the depth texture is just another
field on `Renderer`, attached to every main pass. Offscreen passes will get
their own depth textures later (Commit 6).

---

## Commit 2 — Generalize Vertex Validation and Extend VertexFormat

### Goal

Remove the hardcoded triangle-shader validation that blocks new vertex
layouts, and add missing vertex format variants so MeshFactory can output
meshes with normals, UVs, and other attributes.

### What exists today

| Item | Location | Status |
|------|----------|--------|
| `validate_triangle_shader_layout()` | `render/src/lib.rs:591-629` | Requires position@0 + color@1 |
| `mesh_vertex_attributes()` calls the validator | `render/src/lib.rs:579` | Blocks non-triangle shaders |
| `VertexFormat { Float32x2, Float32x3 }` | `assets/src/lib.rs:67-70` | Missing Float32x4, Float32 |
| `vertex_format_size()` / `wgpu_vertex_format()` | `render/src/lib.rs:632-644` | 2 variants only |
| `validate_triangle_layout()` (public) | `render/src/lib.rs:700-702` | Used by nobody externally |

### What to add

#### rig-assets

1. **Extend `VertexFormat`** with `Float32`, `Float32x4` at minimum. These
   cover scalar attributes (ao factor), normals (vec3), colours with alpha
   (vec4), and tangent frames (vec4). The `Float32x2` added earlier covers
   UVs.

#### rig-render

2. **Replace `validate_triangle_shader_layout()` with a generic
   `validate_vertex_layout()`** — checks:
   - `array_stride > 0`
   - No duplicate `shader_location` values
   - Each attribute fits within the stride (offset + format_size ≤ stride)
   - At least one attribute exists
   - Does NOT require specific locations like 0 and 1.

3. **Update `vertex_format_size()` and `wgpu_vertex_format()`** with the new
   variants.

4. **Deprecate / remove `validate_triangle_layout()`** public function — it
   was a convenience for the single-shader era. If external callers need it,
   provide a more generic version.

#### rig-assets

5. **Add `IndexFormat` enum** to `rig-assets`:
   ```rust
   pub enum IndexFormat {
       Uint16,
       Uint32,
   }
   ```

6. **Add `index_format` field to `MeshAsset`** — defaults to `Uint16` for
   backward compatibility but allows large meshes.

7. **Update `rig-render`** to use the declared index format when computing
   `index_count` and calling `set_index_buffer()`.

### Tests

- `validate_vertex_layout_accepts_normals_only` — layout with location 2
  (normals) but no color@1 should pass.
- `validate_vertex_layout_rejects_empty_layout` — no attributes → error.
- `validate_vertex_layout_rejects_zero_stride` — stride 0 → error.
- `validate_vertex_layout_rejects_duplicates` — same location twice → error.
- `vertex_format_size_float32x4` — verify size is 16 bytes.
- `wgpu_vertex_format_maps_float32x4` — verify correct wgpu mapping.
- `index_count_uses_declared_format` — u32 index format divides by 4, not 2.
- Existing triangle-layout tests updated to use generic validator.

### Example

- **Update `examples/triangle_scenegraph/`** to exercise the new vertex
  format variants. Confirm the existing triangle (position + color) still
  renders correctly through the generic validator path. No visual change,
  but the code path now goes through `validate_vertex_layout()` instead of
  the old triangle-specific validation.

---

## Commit 3 — MeshFactory: Procedural Mesh Generation

### Goal

Create a procedural mesh generation module so multi-object scenes can be
populated without hand-coded vertex arrays.

### What exists today

Nothing — meshes are created manually with raw byte arrays (see
`examples/triangle_scenegraph/src/main.rs:51-73`). The AGENTS.md milestone
list explicitly mentions "MeshFactory".

### What to add

#### rig-assets (new module: `mesh_factory`)

1. **`MeshFactory` module** in `rig-assets/src/mesh_factory.rs` — public
   functions that return `MeshAsset` values. Each function takes parameters
   (dimensions, subdivisions) and returns a mesh with a standard vertex
   layout:

   ```
   Position: Float32x3  @ location 0, offset 0
   Normal:   Float32x3  @ location 1, offset 12
   UV:       Float32x2  @ location 2, offset 24
   stride = 32
   ```

   Index format: `Uint16` for small meshes, `Uint32` when vertex count
   exceeds 65535.

2. **`create_box(width, height, depth) -> MeshAsset`** — axis-aligned box
   centred at origin. 24 vertices (4 per face, for unique normals), 36
   indices. BoundingSphere from half-diagonal.

3. **`create_sphere(radius, slices, stacks) -> MeshAsset`** — UV sphere.
   `(slices + 1) * (stacks + 1)` vertices, `6 * slices * stacks` indices.
   BoundingSphere = `{ center: ZERO, radius }`.

4. **`create_plane(width, depth) -> MeshAsset`** — a single quad (2
   triangles) centred at origin in the XZ plane, normal = +Y. 4 vertices, 6
   indices. UVs span [0, 1].

5. **Re-export from `rig-assets/src/lib.rs`** as `pub mod mesh_factory`.

### Tests

- `create_box_produces_24_vertices_36_indices` — check data sizes.
- `create_box_bounds_are_half_diagonal` — verify bounding sphere.
- `create_sphere_vertex_normals_are_unit_length` — decode first few normals,
  check length ≈ 1.0.
- `create_sphere_indices_stay_in_range` — all indices < vertex count.
- `create_plane_is_a_quad` — 4 vertices, 6 indices, correct stride.
- All meshes pass `validate_vertex_layout()`.

### Example

- **Create `examples/mesh_showcase/`** — a minimal example that uses
  `MeshFactory::create_box()`, `create_sphere()`, and `create_plane()` to
  populate a scene with three objects at different positions. Uses a
  position + normal + UV shader (solid colour from normals for now,
  lighting comes later). Demonstrates that procedural meshes render
  correctly with depth testing. This is the first example using
  MeshFactory-generated geometry instead of hand-coded vertex arrays.

### Inspiration from GeometricTools

GTE's `MeshFactory` (in `GTE/Mathematics/MeshFactory.h`) creates
rectangles, disks, spheres, boxes, cylinders, tori, Platonic solids, etc.
We start with the three most useful shapes (box, sphere, plane) and add
more later. GTE uses a "standard vertex" with position, normal, tangent,
binormal, and texcoord — we use a simpler layout (position + normal + UV)
that still supports Blinn-Phong lighting and texture mapping.

---

## Commit 4 — Public Visibility API and Draw-Call Sorting

### Goal

Expose visibility control to application code and sort the draw list to
reduce GPU state changes when rendering many objects.

### What exists today

| Item | Location | Status |
|------|----------|--------|
| `VisibilityMode` enum | `scene/src/lib.rs:16-20` | Public |
| `SceneNode.visibility` field | `scene/src/lib.rs:60` | Private |
| `node_mut()` | `scene/src/lib.rs:476` | `fn` (crate-private) |
| Draw list iteration | `render/src/lib.rs:435-458` | Unordered |

### What to add

#### rig-scene

1. **`SceneGraph::set_visibility(node, mode) -> Result<()>`** — public
   setter for the visibility mode. Also add **`SceneGraph::visibility(node)
   -> Result<VisibilityMode>`** as a getter.

#### rig-render

2. **Sort `draw_list` by `(ShaderHandle, MeshHandle)`** before issuing draw
   calls. This groups objects by pipeline and mesh, minimising:
   - Pipeline switches (most expensive GPU state change)
   - Vertex/index buffer rebinds

3. **Track "current pipeline" and "current mesh" in the draw loop** — only
   call `set_pipeline()` when the pipeline changes, only call
   `set_vertex_buffer()`/`set_index_buffer()` when the mesh changes.

### Tests

- `set_visibility_changes_node_visibility` — set to Hidden, read back.
- `set_visibility_errors_for_invalid_node` — invalid NodeId → error.
- `draw_list_sorted_by_shader_then_mesh` — construct a draw list with mixed
  shaders/meshes, verify the sorted order groups by shader first.
- `sorted_draw_list_reduces_state_changes` — count hypothetical pipeline
  switches for sorted vs unsorted lists.

### Example

- **Update `examples/mesh_showcase/`** (from Commit 3) to toggle visibility
  of one object with a key press. Add a console log or on-screen counter
  showing how many draw calls are issued per frame vs how many objects
  exist in the scene, demonstrating that frustum culling and visibility
  filtering reduce the draw list.

---

## Commit 5 — Multi-Object Example

### Goal

Create a new example (or significantly extend `triangle_scenegraph`) that
renders multiple objects using shared assets, demonstrating that the
framework handles many objects with correct depth, shared meshes/materials,
and frustum culling.

### What to add

1. **New example: `examples/multi_object/`** — renders a scene with:
   - A ground plane (MeshFactory::create_plane)
   - Several boxes at different positions (shared box MeshAsset, different
     transforms)
   - A sphere (MeshFactory::create_sphere)
   - A camera with CameraRig controls
   - At least one object behind the camera (verify frustum culling)
   - Objects at different depths (verify depth buffer works)

2. **Wire up Cargo.toml** — add `multi_object` to workspace members.

3. **Use shared assets** — one box MeshAsset shared by multiple Renderable
   nodes (different world transforms, same mesh handle).

4. **Use visibility toggling** — pressing a key toggles one object's
   visibility to demonstrate `set_visibility()`.

### Tests

No new library tests — this is an integration example. Manual verification:
- Objects render with correct depth ordering
- Shared meshes display at different positions
- Frustum culling hides off-screen objects (check with logging or debug
  counter)
- Visibility toggle works at runtime

---

## Commit 6 — Offscreen Pass Infrastructure

### Goal

Add a `RenderTarget` abstraction so the renderer can draw to offscreen
textures, not just the swapchain. This is the foundation for shadow maps,
post-processing, reflection probes, etc.

### What exists today

| Item | Location | Status |
|------|----------|--------|
| Single render pass to surface | `render/src/lib.rs:413-458` | No offscreen |
| `PipelineKey { shader, vertex_layout, color_format, depth_format }` | Commit 1 | Has format keys |
| `create_pipeline()` takes format params | Commit 1 | Parameterised |
| `RenderTargets` in RESOURCES.md | `docs/RESOURCES.md:357-363` | Design only |

### What to add

#### rig-render

1. **`RenderTarget` struct**:
   ```rust
   pub struct RenderTarget {
       pub color_texture: wgpu::Texture,
       pub color_view: wgpu::TextureView,
       pub depth_texture: Option<wgpu::Texture>,
       pub depth_view: Option<wgpu::TextureView>,
       pub width: u32,
       pub height: u32,
       pub color_format: wgpu::TextureFormat,
       pub depth_format: Option<wgpu::TextureFormat>,
   }
   ```

2. **`RenderTargetDescriptor` struct**:
   ```rust
   pub struct RenderTargetDescriptor {
       pub width: u32,
       pub height: u32,
       pub color_format: wgpu::TextureFormat,
       pub depth_format: Option<wgpu::TextureFormat>,
       pub label: &'static str,
   }
   ```

3. **`Renderer::create_render_target(desc) -> RenderTarget`** — allocates
   GPU textures with `RENDER_ATTACHMENT | TEXTURE_BINDING` usage so the
   output can be sampled in subsequent passes.

4. **Extract pass recording helper** — factor out the inner render pass
   recording (begin pass, iterate draw list, end pass) into a function that
   accepts:
   - `color_view: &TextureView`
   - `depth_view: Option<&TextureView>`
   - `clear_color: Option<Color>`
   - `clear_depth: Option<f32>`
   - `draw_list` + `assets` + extracted camera

   The current `render_draw_list` becomes a thin wrapper that passes the
   surface texture view + main depth view.

5. **`Renderer::render_to_target(target, scene, assets, camera) ->
   Result<()>`** — renders the scene into the given `RenderTarget` instead
   of the swapchain. Uses the same extracted pass recording helper.

6. **No changes to rig-app yet** — `RenderContext` already exposes
   `&mut Renderer`, so applications can call `create_render_target` and
   `render_to_target` directly.

### Tests

- `render_target_descriptor_creates_correct_dimensions` — create a target,
  verify width/height/format.
- `render_target_color_view_is_valid` — the returned color view should be
  usable (basic sanity check on the texture descriptor).
- `pipeline_key_differs_by_color_format` — pipelines specialised for
  `Rgba16Float` should differ from `Bgra8UnormSrgb`.

### Example

- **No new example in this commit** — the offscreen infrastructure is
  exercised by unit tests and by Commit 7's dedicated example. Keeping
  this commit focused on the library API.

### Inspiration from GeometricTools

GTE's `DrawTarget` bundles N color textures + optional depth texture, with
an imperative `Enable`/`Disable` API that saves and restores engine state.
wgpu's per-pass descriptor model is cleaner: no global state mutation, each
`begin_render_pass` is self-contained. Our `render_to_target` maps directly
to recording a render pass against a `RenderTarget`'s views.

GTE supports MRT (multiple render targets). Our `RenderTarget` starts with
a single color attachment — MRT can be added later by extending to
`color_textures: Vec<(Texture, TextureView)>`.

---

## Commit 7 — Offscreen Pass Example

### Goal

Create a new example that demonstrates offscreen rendering by rendering a
scene to an offscreen `RenderTarget` and then displaying the result as a
textured quad on the main pass. This proves the offscreen infrastructure
works end-to-end.

### What to add

1. **New example: `examples/offscreen_demo/`** — renders a small scene
   (e.g., a spinning box from MeshFactory) into an offscreen
   `RenderTarget`, then renders a fullscreen quad on the main pass that
   samples the offscreen colour texture.

2. **Fullscreen-quad shader** — a WGSL shader that takes a texture and
   sampler as bind group inputs and draws a screen-filling triangle or
   quad. Vertex positions are generated in the vertex shader (no vertex
   buffer needed).

3. **Wire up Cargo.toml** — add `offscreen_demo` to workspace members.

4. **Demonstrates**:
   - `Renderer::create_render_target()` to allocate an offscreen target
   - `Renderer::render_to_target()` to render the scene offscreen
   - Sampling the offscreen colour texture in a subsequent main pass
   - Pipeline specialisation for different colour formats (offscreen may
     use `Rgba8UnormSrgb` while swapchain uses `Bgra8UnormSrgb`)

### Tests

No new library tests — this is an integration example. Manual verification:
- The offscreen scene is visible on-screen via the textured quad
- Resizing the window does not crash (offscreen target stays fixed or
  resizes appropriately)
- Depth works correctly in the offscreen pass

---

## Dependency Order

```
Commit 1 (Depth buffer)     ← independent, prerequisite for everything
Commit 2 (Vertex/format)    ← independent of Commit 1 (but applied after)
Commit 3 (MeshFactory)      ← depends on Commit 2 for new vertex formats
Commit 4 (Visibility/sort)  ← independent
Commit 5 (Multi-object ex)  ← depends on Commits 1-4
Commit 6 (Offscreen passes) ← depends on Commit 1 (depth + PipelineKey)
Commit 7 (Offscreen example)← depends on Commits 3 + 6
```

```
    ┌── Commit 1 (depth) ──────┬── Commit 6 (offscreen) ─┐
    │                           │                          │
    ├── Commit 2 (vertex) ──┐  │                          │
    │                        ├──┤                          │
    ├── Commit 3 (mesh)  ───┘  │                    Commit 7 (offscreen ex)
    │                           │
    ├── Commit 4 (vis/sort) ────┤
    │                           │
    └── Commit 5 (multi-obj) ───┘
```

## Verification

After each commit:

```bash
nix develop -c cargo fmt --all
nix develop -c cargo test --workspace
nix develop -c cargo clippy --workspace -- -D warnings
nix develop -c cargo tarpaulin --engine llvm   # coverage should not regress
```
