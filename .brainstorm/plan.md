# Feature: Examples Directory Reorganization

## Phase 1: Normalize example Cargo.toml files

Commit message: `chore: normalize all example Cargo.toml files to workspace dependency pattern`

### Step 1: Normalize the 7 loading example Cargo.toml files

The following 7 files all have the same structure: `rig-app = { path = "../../crates/app" }`,
a `[[bin]]` section, `edition = "2024"`, and version-string external deps. Replace each
file's entire contents with the normalized form below. Substitute the correct `name` value
for each file.

Files and their `name` values:
- `examples/obj_load/Cargo.toml` → `obj_load`
- `examples/obj_textured/Cargo.toml` → `obj_textured`
- `examples/multi_obj/Cargo.toml` → `multi_obj`
- `examples/texture_load/Cargo.toml` → `texture_load`
- `examples/texture_formats/Cargo.toml` → `texture_formats`
- `examples/shader_load/Cargo.toml` → `shader_load`
- `examples/asset_showcase/Cargo.toml` → `asset_showcase`

Normalized content (substitute `<NAME>`):

```toml
[package]
name = "<NAME>"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
anyhow.workspace = true
env_logger.workspace = true
log.workspace = true
```

### Step 2: Normalize terrain_mc and terrain_heightmap Cargo.toml

These 2 files have `rig-app`, `rig-assets`, and `rig-math` as path deps, `noise` already
as a workspace dep, a `[[bin]]` section, and `edition = "2024"`. Replace each file entirely.

**`examples/terrain_mc/Cargo.toml`:**
```toml
[package]
name = "terrain_mc"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
rig-assets.workspace = true
rig-math.workspace = true
noise.workspace = true
anyhow.workspace = true
log.workspace = true
env_logger.workspace = true
```

**`examples/terrain_heightmap/Cargo.toml`:**
```toml
[package]
name = "terrain_heightmap"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
rig-assets.workspace = true
rig-math.workspace = true
noise.workspace = true
anyhow.workspace = true
log.workspace = true
env_logger.workspace = true
```

### Step 3: Normalize metaballs Cargo.toml

`examples/metaballs/Cargo.toml` has `rig-app`, `rig-assets`, `rig-math` as path deps,
a `[[bin]]` section, and `edition = "2024"`. Replace entirely:

```toml
[package]
name = "metaballs"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
rig-assets.workspace = true
rig-math.workspace = true
anyhow.workspace = true
log.workspace = true
env_logger.workspace = true
```

### Step 4: Normalize voice_metaballs Cargo.toml

`examples/voice_metaballs/Cargo.toml` has `rig-app`, `rig-assets`, `rig-math`, and
`bytemuck` as path/version deps, a `[[bin]]` section, and `edition = "2024"`. It also
has 4 external `rustycuda` path deps — convert the rig crates and bytemuck to workspace
deps, keep the rustycuda deps as path deps unchanged (they will be adjusted in Phase 4
when the example moves). Replace entirely:

```toml
[package]
name = "voice_metaballs"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
rig-assets.workspace = true
rig-math.workspace = true
anyhow.workspace = true
log.workspace = true
env_logger.workspace = true
bytemuck.workspace = true

# graphynx — signal processing pipeline (external; path adjusted when example moves)
graph-core   = { path = "../../../rustycuda/core" }
backends     = { path = "../../../rustycuda/backends" }
backends-cpu = { path = "../../../rustycuda/backends-cpu" }
runtime      = { path = "../../../rustycuda/runtime", features = ["live-audio"] }
```

### Step 5: Normalize lit_scene and trackball_demo Cargo.toml

Both files have `rig-app` as a path dep, a `[[bin]]` section, `edition = "2024"`, and
version-string external deps. Replace each entirely.

**`examples/lit_scene/Cargo.toml`:**
```toml
[package]
name = "lit_scene"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
anyhow.workspace = true
log.workspace = true
env_logger.workspace = true
```

**`examples/trackball_demo/Cargo.toml`:**
```toml
[package]
name = "trackball_demo"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
anyhow.workspace = true
log.workspace = true
env_logger.workspace = true
```

### Step 6: Normalize textured_mesh Cargo.toml

`examples/textured_mesh/Cargo.toml` already uses `edition.workspace = true` and some
workspace deps, but still has `rig-app = { path = "../../crates/app" }` and a `[[bin]]`
section. Replace entirely:

```toml
[package]
name = "textured_mesh"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
anyhow.workspace = true
image.workspace = true
env_logger.workspace = true
log.workspace = true
```

### Step 7: Remove [[bin]] from tentacle_demo Cargo.toml

`examples/tentacle_demo/Cargo.toml` already uses workspace deps correctly but has an
unnecessary `[[bin]]` section and extra workspace keys. Replace entirely:

```toml
[package]
name = "tentacle_demo"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
anyhow.workspace = true
env_logger.workspace = true
log.workspace = true
```

### Step 8: Verify workspace builds

Run from the workspace root:
```bash
nix develop --impure --command cargo build --workspace
```

All 31 examples must compile. If any fail, a dependency was missed — check the error
and add the missing `.workspace = true` entry to that example's `Cargo.toml`.

---

## Phase 2: Create the example-shared lib crate

Commit message: `refactor: extract shared loading code into example-shared lib crate`

### Step 1: Add example-shared to root Cargo.toml

In the file `Cargo.toml` (workspace root), make two changes:

**Change 1** — In the `members` array, add `"examples/shared"` as a new entry after
`"tools/gen_test_textures"`:
```toml
    "tools/gen_test_textures",
    "examples/shared",
    "examples/hello_triangle",
```

**Change 2** — In the `[workspace.dependencies]` section, add after the `rig-gltf` line:
```toml
example-shared = { path = "examples/shared" }
```

### Step 2: Create examples/shared/Cargo.toml

Create the new file `examples/shared/Cargo.toml`:

```toml
[package]
name = "example-shared"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
anyhow.workspace = true
```

### Step 3: Create examples/shared/src/lib.rs

Create the new file `examples/shared/src/lib.rs` by copying the full contents of
`examples/shared_loading_example.rs` with exactly two modifications:

1. Change `enum ExampleKind {` to `pub enum ExampleKind {`
2. Change `fn run_loading_example(` to `pub fn run_loading_example(`

All other items — `LoadingExampleApp`, `EXAMPLE_KIND`, `impl ExampleKind`,
`impl Application for LoadingExampleApp`, and all helper functions (`add_imported_model`,
`add_loaded_texture_material`, `add_renderable`, `finish_scene`, `add_camera`,
`add_default_light`) — remain private (no `pub` prefix). They are only called internally
by `run_loading_example`.

The public API surface of this crate is intentionally minimal:
```rust
pub enum ExampleKind { ObjLoad, ObjTextured, MultiObj, TextureLoad, TextureFormats, ShaderLoad, AssetShowcase }
pub fn run_loading_example(kind: ExampleKind) -> Result<()>
```

### Step 4: Verify the shared crate compiles

Run from the workspace root:
```bash
nix develop --impure --command cargo build -p example-shared
```

Must compile without errors before proceeding.

---

## Phase 3: Convert include!() examples to use example-shared

Commit message: `refactor: replace include!() with example-shared dependency in 7 loading examples`

### Step 1: Add example-shared dependency to 7 loading example Cargo.toml files

Add `example-shared.workspace = true` to the `[dependencies]` section of each of these
7 files (which were normalized in Phase 1 Step 1):

- `examples/obj_load/Cargo.toml`
- `examples/obj_textured/Cargo.toml`
- `examples/multi_obj/Cargo.toml`
- `examples/texture_load/Cargo.toml`
- `examples/texture_formats/Cargo.toml`
- `examples/shader_load/Cargo.toml`
- `examples/asset_showcase/Cargo.toml`

For example, `examples/obj_load/Cargo.toml` becomes:
```toml
[package]
name = "obj_load"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
example-shared.workspace = true
anyhow.workspace = true
env_logger.workspace = true
log.workspace = true
```

Apply the same addition to all 7 files.

### Step 2: Replace include!() with use imports in 7 src/main.rs files

In each of the 7 loading examples, open `src/main.rs` and replace the line:
```rust
include!("../../shared_loading_example.rs");
```
with:
```rust
use example_shared::{ExampleKind, run_loading_example};
```

Note: the Rust identifier uses underscores (`example_shared`) even though the Cargo
package name uses a hyphen (`example-shared`). This is standard Cargo behaviour.

The 7 files and the `ExampleKind` variant each calls:

| File | Variant |
|------|---------|
| `examples/obj_load/src/main.rs` | `ExampleKind::ObjLoad` |
| `examples/obj_textured/src/main.rs` | `ExampleKind::ObjTextured` |
| `examples/multi_obj/src/main.rs` | `ExampleKind::MultiObj` |
| `examples/texture_load/src/main.rs` | `ExampleKind::TextureLoad` |
| `examples/texture_formats/src/main.rs` | `ExampleKind::TextureFormats` |
| `examples/shader_load/src/main.rs` | `ExampleKind::ShaderLoad` |
| `examples/asset_showcase/src/main.rs` | `ExampleKind::AssetShowcase` |

Keep the doc comment at the top of each file unchanged. Keep the `fn main()` body
unchanged. Only the `include!()` line changes.

### Step 3: Delete the now-dead shared_loading_example.rs

Delete the file `examples/shared_loading_example.rs`. Its contents now live in
`examples/shared/src/lib.rs`.

### Step 4: Verify build and clippy at current flat structure

Run from the workspace root:
```bash
nix develop --impure --command cargo build --workspace
nix develop --impure --command cargo clippy --workspace -- -D warnings
```

Both must pass. This confirms phases 1–3 are correct before any directory changes.
If anything fails here, fix it before proceeding to Phase 4.

---

## Phase 4: Move examples into domain group directories

Commit message: `refactor: reorganize examples into domain-based subdirectories`

### Step 1: Create group directories

Create the following empty directories (they will be populated by git mv):
```
examples/basics/
examples/geometry/
examples/techniques/
examples/materials/
examples/loading/
examples/animation/
examples/terrain/
examples/gltf/
examples/procedural/
```

### Step 2: Move basics group

Run from the workspace root:
```bash
git mv examples/hello_triangle      examples/basics/hello_triangle
git mv examples/triangle_scenegraph examples/basics/triangle_scenegraph
git mv examples/trackball_demo      examples/basics/trackball_demo
```

### Step 3: Move geometry group

```bash
git mv examples/mesh_showcase  examples/geometry/mesh_showcase
git mv examples/multi_object   examples/geometry/multi_object
git mv examples/platonic_solids examples/geometry/platonic_solids
```

### Step 4: Move techniques group

```bash
git mv examples/offscreen_demo examples/techniques/offscreen_demo
```

### Step 5: Move materials group

```bash
git mv examples/textured_mesh   examples/materials/textured_mesh
git mv examples/lit_scene       examples/materials/lit_scene
git mv examples/normal_map_demo examples/materials/normal_map_demo
```

### Step 6: Move loading group

```bash
git mv examples/obj_load      examples/loading/obj_load
git mv examples/obj_textured  examples/loading/obj_textured
git mv examples/multi_obj     examples/loading/multi_obj
git mv examples/texture_load  examples/loading/texture_load
git mv examples/texture_formats examples/loading/texture_formats
git mv examples/shader_load   examples/loading/shader_load
git mv examples/asset_showcase examples/loading/asset_showcase
git mv examples/model_gallery  examples/loading/model_gallery
```

### Step 7: Move animation group

```bash
git mv examples/skeleton_demo examples/animation/skeleton_demo
git mv examples/tentacle_demo examples/animation/tentacle_demo
```

### Step 8: Move terrain group (with prefix stripping)

Directory names drop the `terrain_` prefix — the parent directory provides the context.
Package names inside each `Cargo.toml` are unchanged (e.g. `terrain_warp` stays `terrain_warp`).

```bash
git mv examples/terrain_mc        examples/terrain/marching_cubes
git mv examples/terrain_heightmap examples/terrain/heightmap
git mv examples/terrain_warp      examples/terrain/warp
git mv examples/terrain_erosion   examples/terrain/erosion
git mv examples/terrain_triplanar examples/terrain/triplanar
git mv examples/terrain_chunks    examples/terrain/chunks
git mv examples/terrain_lod       examples/terrain/lod
```

### Step 9: Move gltf group (with prefix stripping)

Directory names drop the `gltf_` prefix. Package names are unchanged.

```bash
git mv examples/gltf_demo        examples/gltf/demo
git mv examples/gltf_skinned_demo examples/gltf/skinned
```

### Step 10: Move procedural group

```bash
git mv examples/metaballs      examples/procedural/metaballs
git mv examples/voice_metaballs examples/procedural/voice_metaballs
```

### Step 11: Fix voice_metaballs rustycuda paths after move

`examples/procedural/voice_metaballs/Cargo.toml` is now one directory level deeper than
before. The 4 rustycuda path deps must be updated from `../../../rustycuda/` to
`../../../../rustycuda/`.

In `examples/procedural/voice_metaballs/Cargo.toml`, replace:
```toml
graph-core   = { path = "../../../rustycuda/core" }
backends     = { path = "../../../rustycuda/backends" }
backends-cpu = { path = "../../../rustycuda/backends-cpu" }
runtime      = { path = "../../../rustycuda/runtime", features = ["live-audio"] }
```
with:
```toml
graph-core   = { path = "../../../../rustycuda/core" }
backends     = { path = "../../../../rustycuda/backends" }
backends-cpu = { path = "../../../../rustycuda/backends-cpu" }
runtime      = { path = "../../../../rustycuda/runtime", features = ["live-audio"] }
```

### Step 12: Update workspace Cargo.toml members and add default-members

In the root `Cargo.toml`, replace the entire `[workspace]` section (the `members` array
and surrounding keys) with the following. Keep everything after `[workspace]` (i.e.
`[workspace.package]` and `[workspace.dependencies]`) unchanged.

```toml
[workspace]
resolver = "2"
members = [
    "crates/*",
    "tools/*",
    "examples/shared",
    "examples/basics/*",
    "examples/geometry/*",
    "examples/techniques/*",
    "examples/materials/*",
    "examples/loading/*",
    "examples/animation/*",
    "examples/terrain/*",
    "examples/gltf/*",
    "examples/procedural/*",
]
default-members = ["crates/*", "tools/*"]
exclude = ["GeometricTools"]
```

### Step 13: Verify workspace builds after moves

Run from the workspace root:
```bash
nix develop --impure --command cargo build --workspace
nix develop --impure --command cargo clippy --workspace -- -D warnings
```

Both must pass. Any failure at this point is a path issue (wrong glob or missed path
adjustment) — not a code issue, since the code was verified clean in Phase 3 Step 4.

---

## Phase 5: Write README documentation

Commit message: `docs: add top-level and per-group README files for examples`

### Step 1: Create examples/README.md

```markdown
# Examples

Demonstrations of the rig framework organized by domain.

## Groups

| Group | Description | Examples |
|-------|-------------|----------|
| [`basics/`](basics/) | Getting started — raw wgpu and first framework usage | hello_triangle, triangle_scenegraph, trackball_demo |
| [`geometry/`](geometry/) | Mesh creation, multiple objects, and scene assembly | mesh_showcase, multi_object, platonic_solids |
| [`techniques/`](techniques/) | Rendering techniques — offscreen targets, post-processing | offscreen_demo |
| [`materials/`](materials/) | Surface appearance — texturing, lighting, normal maps | textured_mesh, lit_scene, normal_map_demo |
| [`loading/`](loading/) | Asset loading — OBJ, textures, shaders, combined pipelines | obj_load, obj_textured, multi_obj, texture_load, texture_formats, shader_load, asset_showcase, model_gallery |
| [`animation/`](animation/) | Skeleton animation and CPU skinning | skeleton_demo, tentacle_demo |
| [`terrain/`](terrain/) | Procedural terrain — marching cubes, heightmaps, erosion, LOD | marching_cubes, heightmap, warp, erosion, triplanar, chunks, lod |
| [`gltf/`](gltf/) | glTF model loading — static PBR and skinned models | demo, skinned |
| [`procedural/`](procedural/) | Procedural geometry — metaballs, audio-reactive surfaces | metaballs, voice_metaballs |

## Running examples

All examples must be run from the **workspace root** so that `assets/` resolves correctly:

```bash
# from the workspace root
cargo run -p <package_name>

# examples:
cargo run -p hello_triangle
cargo run -p terrain_warp
cargo run -p gltf_demo
```

Running compiled binaries directly from another directory will produce asset-not-found
errors because `FilesystemSource` roots at the process working directory.

## Building

```bash
# build only library crates (fast iteration — the default)
cargo build

# build everything including all examples
cargo build --workspace
```
```

### Step 2: Create examples/basics/README.md

```markdown
# Basics

Getting started — from raw wgpu usage to the first full framework application.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `hello_triangle/` | `cargo run -p hello_triangle` | Minimal colored triangle using raw wgpu + winit | wgpu device/queue/surface, render pipeline, vertex buffer |
| `triangle_scenegraph/` | `cargo run -p triangle_scenegraph` | Same triangle rendered through the full framework | Application trait, SceneGraph, AssetStore, Renderer |
| `trackball_demo/` | `cargo run -p trackball_demo` | Interactive camera with trackball orbit and dolly | TrackBall, CameraRig, mouse input |

## Suggested order

1. Start with `hello_triangle` to understand the raw wgpu/winit foundation with no framework
2. Then `triangle_scenegraph` shows how the framework abstracts the same result
3. Finally `trackball_demo` introduces interactive camera controls used in all later examples

## Notes

Run all examples from the workspace root so that `assets/` resolves correctly:

```bash
cargo run -p hello_triangle
cargo run -p triangle_scenegraph
cargo run -p trackball_demo
```
```

### Step 3: Create examples/geometry/README.md

```markdown
# Geometry

Mesh creation with MeshFactory primitives and multi-object scene assembly.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `mesh_showcase/` | `cargo run -p mesh_showcase` | MeshFactory primitives — box, sphere, plane | mesh_factory, vertex layout, AssetStore |
| `multi_object/` | `cargo run -p multi_object` | Multiple objects with a camera rig | scene graph nodes, transforms, CameraRig |
| `platonic_solids/` | `cargo run -p platonic_solids` | Five animated Platonic solids with fly-camera and overlay | orbit animation, frustum culling, DebugHud |

## Suggested order

1. Start with `mesh_showcase` to see the built-in MeshFactory primitives
2. Then `multi_object` shows how to compose a scene with multiple meshes and a camera
3. Finally `platonic_solids` combines animation, camera control, and the overlay system

## Notes

Run all examples from the workspace root:

```bash
cargo run -p mesh_showcase
cargo run -p multi_object
cargo run -p platonic_solids
```
```

### Step 4: Create examples/techniques/README.md

```markdown
# Techniques

Rendering techniques beyond basic forward rendering.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `offscreen_demo/` | `cargo run -p offscreen_demo` | Render to offscreen texture then blit to screen | render targets, texture blit, multi-pass rendering |

## Notes

Run all examples from the workspace root:

```bash
cargo run -p offscreen_demo
```
```

### Step 5: Create examples/materials/README.md

```markdown
# Materials

Surface appearance — texturing, lighting, and normal mapping.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `textured_mesh/` | `cargo run -p textured_mesh` | Texture mapped onto a procedural mesh | bind groups, GPU texture/sampler cache, TEXTURED_SHADER |
| `lit_scene/` | `cargo run -p lit_scene` | Blinn-Phong lit scene with a directional light | LightUniform, LightsBuffer, PHONG_SHADER |
| `normal_map_demo/` | `cargo run -p normal_map_demo` | Normal mapping with tangent-space normals | mikktspace tangents, 5-slot PBR bind group, 48-byte vertex |

## Suggested order

1. Start with `textured_mesh` to understand how textures are bound and sampled
2. Then `lit_scene` adds dynamic lighting with Blinn-Phong shading
3. Finally `normal_map_demo` shows the full PBR material pipeline with normal maps

## Notes

Run all examples from the workspace root:

```bash
cargo run -p textured_mesh
cargo run -p lit_scene
cargo run -p normal_map_demo
```
```

### Step 6: Create examples/loading/README.md

```markdown
# Loading

Asset loading pipeline — OBJ models, textures, runtime shaders, and combined workflows.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `obj_load/` | `cargo run -p obj_load` | Geometry-only OBJ loading with Phong shading | rig-import, Importer, MeshConfig, smooth normals |
| `obj_textured/` | `cargo run -p obj_textured` | OBJ + MTL with diffuse texture | material import, TextureConfig, ShaderPolicy |
| `multi_obj/` | `cargo run -p multi_obj` | Multiple OBJ loads demonstrating importer cache | path dedup, cache hits, shared texture atlas |
| `texture_load/` | `cargo run -p texture_load` | Texture file loaded onto a procedural sphere | import_texture, SamplerDescriptor |
| `texture_formats/` | `cargo run -p texture_formats` | PNG/JPEG/TGA side-by-side comparison | format detection, color space, channel count |
| `shader_load/` | `cargo run -p shader_load` | Runtime WGSL shader loading | import_shader, ShaderAsset, hot path |
| `asset_showcase/` | `cargo run -p asset_showcase` | Combined loading showcase with registry stats | full pipeline, cache summary overlay |
| `model_gallery/` | `cargo run -p model_gallery` | CLI model viewer over curated asset library | BoundingSphere, auto-scaling, PLY decoder |

## Suggested order

1. Start with `obj_load` for the simplest mesh import
2. Then `obj_textured` adds material and texture handling
3. `multi_obj` demonstrates the importer cache deduplication
4. `texture_load` and `texture_formats` focus on texture importing in isolation
5. `shader_load` demonstrates runtime shader compilation
6. `asset_showcase` combines OBJ, texture, and shader loading in one scene
7. `model_gallery` is a standalone CLI viewer — run with `cargo run -p model_gallery -- <model>`

## Notes

Run all examples from the workspace root:

```bash
cargo run -p obj_load
cargo run -p model_gallery -- assets/models/bunny.ply
```
```

### Step 7: Create examples/animation/README.md

```markdown
# Animation

Skeleton animation and CPU linear blend skinning.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `skeleton_demo/` | `cargo run -p skeleton_demo` | Rigid skeleton animation — procedural robot arm | AnimationPlayer, AnimationClip, keyframe sampling, binding table |
| `tentacle_demo/` | `cargo run -p tentacle_demo` | CPU skinning — 4-bone animated cylinder | SkinEvaluator, SkinAsset, SkinWeights, linear blend skinning |

## Suggested order

1. Start with `skeleton_demo` for rigid (non-deforming) skeleton animation
2. Then `tentacle_demo` adds per-vertex skinning with bone weights and inverse-transpose normals

## Notes

Run all examples from the workspace root:

```bash
cargo run -p skeleton_demo
cargo run -p tentacle_demo
```
```

### Step 8: Create examples/terrain/README.md

```markdown
# Terrain

Procedural terrain generation — from basic heightmaps to infinite chunked landscapes.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `marching_cubes/` | `cargo run -p terrain_mc` | Marching cubes isosurface from a noise field | DynamicMesh, MC lookup tables, noise field |
| `heightmap/` | `cargo run -p terrain_heightmap` | Heightmap terrain with procedural normal map | noise sampling, TextureAsset, normal generation |
| `warp/` | `cargo run -p terrain_warp` | Domain-warped heightmap for natural-looking terrain | fractal noise, domain warping, fbm |
| `erosion/` | `cargo run -p terrain_erosion` | Hydraulic erosion simulation | particle-based erosion, sediment transport |
| `triplanar/` | `cargo run -p terrain_triplanar` | UV-free triplanar texturing on steep surfaces | triplanar projection, blend weights, no UV stretching |
| `chunks/` | `cargo run -p terrain_chunks` | Camera-driven infinite chunked terrain | chunk loading/unloading, spatial index, streaming |
| `lod/` | `cargo run -p terrain_lod` | Distance-based level of detail | LOD selection, mesh simplification, transition |

## Suggested order

1. Start with `marching_cubes` for 3D isosurface terrain
2. Then `heightmap` for the simpler and faster 2D heightfield approach
3. `warp` adds visual complexity via domain warping
4. `erosion` simulates natural weathering on a heightmap
5. `triplanar` solves UV stretching on steep cliff faces
6. `chunks` introduces spatial streaming for infinite worlds
7. `lod` adds performance scaling with camera distance

## Notes

Run all examples from the workspace root:

```bash
cargo run -p terrain_warp
cargo run -p terrain_chunks
```
```

### Step 9: Create examples/gltf/README.md

```markdown
# glTF

glTF 2.0 model loading — static PBR materials and runtime CPU skinning.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `demo/` | `cargo run -p gltf_demo` | Static/PBR model viewer (DamagedHelmet.glb) | rig-gltf, PBR material mapping, cameras, lights, multi-scene |
| `skinned/` | `cargo run -p gltf_skinned_demo` | CPU skinning runtime validation (BrainStem.glb) | skin descriptors, joint transforms, morph target loading |

## Suggested order

1. Start with `demo` for static model loading with full PBR materials
2. Then `skinned` adds runtime skeletal animation via CPU skinning descriptors

## Notes

Run all examples from the workspace root:

```bash
cargo run -p gltf_demo
cargo run -p gltf_skinned_demo
```

glTF assets are stored under `assets/gltf/` and tracked via Git LFS.
Run `git lfs pull` if models appear missing.
```

### Step 10: Create examples/procedural/README.md

```markdown
# Procedural

Procedural geometry — runtime mesh generation from implicit surface functions.

## Examples

| Example | Run command | Description | Key concepts |
|---------|-------------|-------------|--------------|
| `metaballs/` | `cargo run -p metaballs` | 4 bouncing metaballs rendered via marching cubes | DynamicMesh, field functions, Blinn-Phong, fly-camera |
| `voice_metaballs/` | `cargo run -p voice_metaballs` | Audio-reactive metaballs driven by live voice input | signal processing pipeline, real-time mesh update |

## Suggested order

1. Start with `metaballs` for the base implicit-surface technique
2. Then `voice_metaballs` extends it with live audio reactivity via the graphynx pipeline

## Notes

Run all examples from the workspace root:

```bash
cargo run -p metaballs
cargo run -p voice_metaballs
```

`voice_metaballs` requires the external `rustycuda`/graphynx project to be present at
`../../../../rustycuda/` relative to this directory (i.e. a sibling of the workspace
root). It will be refactored to use the Nix flake system in a future milestone.
```

---

## Phase 6: Update living documentation

Commit message: `docs: update example paths in AGENTS.md and architecture docs`

### Step 1: Update AGENTS.md repository layout

In `AGENTS.md`, find the repository layout code block (the `examples/` section). Replace
the flat list of example directories with the new grouped structure. The new layout
section for examples should read:

```
  examples/
    shared/                         # example-shared — shared loading utilities lib crate
    basics/                         # hello_triangle, triangle_scenegraph, trackball_demo
    geometry/                       # mesh_showcase, multi_object, platonic_solids
    techniques/                     # offscreen_demo
    materials/                      # textured_mesh, lit_scene, normal_map_demo
    loading/                        # obj_load, obj_textured, multi_obj, texture_load, texture_formats, shader_load, asset_showcase, model_gallery
    animation/                      # skeleton_demo, tentacle_demo
    terrain/                        # marching_cubes (terrain_mc), heightmap, warp, erosion, triplanar, chunks, lod
    gltf/                           # demo (gltf_demo), skinned (gltf_skinned_demo)
    procedural/                     # metaballs, voice_metaballs
```

Also update the milestone notes at the bottom of AGENTS.md where individual examples are
referenced by path. Apply these substitutions throughout the file:

| Old path | New path |
|----------|----------|
| `examples/hello_triangle` | `examples/basics/hello_triangle` |
| `examples/triangle_scenegraph` | `examples/basics/triangle_scenegraph` |
| `examples/mesh_showcase` | `examples/geometry/mesh_showcase` |
| `examples/multi_object` | `examples/geometry/multi_object` |
| `examples/offscreen_demo` | `examples/techniques/offscreen_demo` |
| `examples/platonic_solids` | `examples/geometry/platonic_solids` |
| `examples/skeleton_demo` | `examples/animation/skeleton_demo` |
| `examples/tentacle_demo` | `examples/animation/tentacle_demo` |
| `examples/obj_load` | `examples/loading/obj_load` |
| `examples/obj_textured` | `examples/loading/obj_textured` |
| `examples/multi_obj` | `examples/loading/multi_obj` |
| `examples/texture_load` | `examples/loading/texture_load` |
| `examples/texture_formats` | `examples/loading/texture_formats` |
| `examples/shader_load` | `examples/loading/shader_load` |
| `examples/asset_showcase` | `examples/loading/asset_showcase` |
| `examples/model_gallery` | `examples/loading/model_gallery` |
| `examples/normal_map_demo` | `examples/materials/normal_map_demo` |
| `examples/terrain_mc` | `examples/terrain/marching_cubes` |
| `examples/terrain_heightmap` | `examples/terrain/heightmap` |
| `examples/terrain_warp` | `examples/terrain/warp` |
| `examples/terrain_erosion` | `examples/terrain/erosion` |
| `examples/terrain_triplanar` | `examples/terrain/triplanar` |
| `examples/terrain_chunks` | `examples/terrain/chunks` |
| `examples/terrain_lod` | `examples/terrain/lod` |
| `examples/gltf_demo` | `examples/gltf/demo` |
| `examples/gltf_skinned_demo` | `examples/gltf/skinned` |
| `examples/metaballs` | `examples/procedural/metaballs` |
| `examples/voice_metaballs` | `examples/procedural/voice_metaballs` |

### Step 2: Update docs/ARCHITECTURE.md

Search `docs/ARCHITECTURE.md` for all occurrences of `examples/` and apply the same
substitution table from Step 1. Pay particular attention to Mermaid diagram nodes that
reference example paths — update the label text but keep the Mermaid node syntax intact.

### Step 3: Update docs/MATERIAL.md

Search `docs/MATERIAL.md` for all occurrences of `examples/` and apply the substitution
table. Key replacements in this file:
- `examples/normal_map_demo/` → `examples/materials/normal_map_demo/`
- `examples/terrain_mc/` → `examples/terrain/marching_cubes/`
- `examples/terrain_heightmap/` → `examples/terrain/heightmap/`

### Step 4: Update docs/ANIMATION.md

Search `docs/ANIMATION.md` for all occurrences of `examples/` and apply the substitution
table. Note: some references are to future/planned examples (`examples/animation_showcase`,
`examples/skinned_mesh`, `examples/gltf_anim`) — update these to their expected future
paths under the new structure (`examples/animation/animation_showcase`, etc.).

### Step 5: Update docs/METABALLS.md

Search `docs/METABALLS.md` for all occurrences of `examples/metaballs` and
`examples/voice_metaballs` and apply the substitution table:
- `examples/metaballs` → `examples/procedural/metaballs`
- `examples/voice_metaballs` → `examples/procedural/voice_metaballs`

### Step 6: Update assets.md and quest2.md

In `assets.md` (workspace root), find the line:
```
examples/         (depend on rig-app)
```
Replace with:
```
examples/         (grouped by domain — see examples/README.md)
```

In `quest2.md` (workspace root), find any reference to `examples/` paths and apply the
substitution table from Step 1.

### Step 7: Final verification

Run from the workspace root:
```bash
nix develop --impure --command cargo build --workspace
nix develop --impure --command cargo clippy --workspace -- -D warnings
nix develop --impure --command cargo test --workspace
```

All three must pass. The reorganization is complete.

---

## Summary

| Phase | Commit | Files changed | Risk |
|-------|--------|---------------|------|
| 1 — Normalize Cargo.toml | `chore: normalize...` | 15 Cargo.toml files | Low — mechanical |
| 2 — Create shared crate | `refactor: extract...` | 2 new files + root Cargo.toml | Medium — new crate |
| 3 — Convert include!() | `refactor: replace...` | 7 Cargo.toml + 7 main.rs + delete 1 | Medium — API change |
| 4 — Move directories | `refactor: reorganize...` | 31 git mv + root Cargo.toml | Low — pure rename |
| 5 — Write READMEs | `docs: add...` | 10 new README.md files | None |
| 6 — Update docs | `docs: update...` | 6 existing doc files | None |

**Key invariants throughout:**
- `cargo run -p terrain_warp` (and all other `-p` targets) works unchanged — package names never change
- Phases 1–3 are verified at the flat structure before any `git mv` — code changes and file moves are in separate commits
- `git mv` is used for all moves to preserve `git log --follow` rename tracking
