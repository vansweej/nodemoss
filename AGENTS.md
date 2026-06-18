# AGENTS.md

## Project

Personal 3D & physics research framework in Rust.
Cross-platform: Linux (X11/Wayland) and macOS (Cocoa/Metal).

## Repository layout

```
graphics/                       # workspace root (this directory)
  Cargo.toml                    # [workspace] — members listed below
  AGENTS.md                     # this file
  docs/
    ARCHITECTURE.md             # master architecture (crate map, ownership boundaries, milestones)
    SCENEGRAPH.md               # scene graph deep-dive (arena tree + scene-facing components)
    RESOURCES.md                # assets, GPU resources, frame resources
    APPLICATION.md              # runtime, event loop, contexts, interaction
    LOADING.md                  # file loading pipeline, importer adaptation, examples
    GLTF.md                     # glTF loader architecture, adaptation map, runtime handoff
    MODELS.md                   # curated model/texture library, provenance, viewer usage
  crates/
    math/                       # rig-math    — glam re-exports + Transform, BoundingSphere, Projection, Camera
    scene/                      # rig-scene   — arena SceneGraph, generational NodeId, cameras/lights/renderables
    assets/                     # rig-assets  — immutable meshes, materials, shader source, textures
    loader/                     # rig-loader  — file/source abstraction + image/OBJ/PLY/WGSL decoders
    import/                     # rig-import  — decoded asset adaptation into AssetStore assets + model bounds
    anim/                       # rig-anim    — AnimationPlayer, binding table, keyframe sampling
    skin/                       # rig-skin    — CPU linear blend skinning evaluator
    gpu/                        # rig-gpu     — GpuContext (device/queue/surface), Frame, GpuError
    render/                     # rig-render  — concrete wgpu renderer, immutable cache, frame resources
    overlay/                    # rig-overlay — 2D text overlay (glyphon), retained ElementRegistry
    app/                        # rig-app     — Application trait, runner, startup/update/render/overlay contexts
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
  GeometricTools/               # reference C++ codebase (NOT compiled by Rust)
```

## Language

This is a Rust project. When using OpenCode skills, load the  skill
for language-specific coding standards, error handling, and tooling rules.

## Technology choices

| Area             | Choice      | Notes                                                    |
|------------------|-------------|----------------------------------------------------------|
| Graphics API     | **wgpu**    | Vulkan on Linux, Metal on macOS. Rust-native.            |
| Windowing        | **winit**   | Cross-platform. Integrates with wgpu.                    |
| Math             | **glam**    | Fast, bytemuck-compatible. Extended by rig-math.         |
| Scene graph      | **Hybrid**  | Arena tree + scene-facing component maps.                 |
| Project layout   | **Cargo workspace** | Core crates plus loader/import pipeline.          |
| GPU resources    | **Immutable cache + frame resources** | Share immutable GPU state, allocate mutable frame data explicitly. |
| 2D overlay       | **glyphon** | GPU text rendering; retained element registry in rig-overlay. |
| Parallelism      | **rayon** (planned) | Thread-pool for terrain chunk generation; not yet a dep — add when chunk count becomes a bottleneck. |

## Crate dependency order

```
rig-math          (leaf — depends only on glam)
  ^
rig-scene         (depends on rig-math)
  ^
rig-assets        (depends on rig-math; + mikktspace for tangent generation)
  ^
rig-loader        (leaf — depends on image, tobj, thiserror; PLY decoded without external crate)
  ^
rig-import        (depends on rig-loader, rig-assets, rig-math; LoadedModel carries combined BoundingSphere)
  ^
rig-anim          (depends on rig-math, rig-assets, rig-scene; AnimationPlayer + binding table)
  ^
rig-skin          (depends on rig-math, rig-assets, rig-scene; CPU linear blend skinning)
  ^
rig-gpu           (depends on wgpu, winit)
  ^
rig-render        (depends on rig-gpu, rig-math, rig-scene, rig-assets)
  ^
rig-overlay       (depends on rig-gpu, glyphon)
  ^
rig-gltf          (depends on gltf, rig-assets, rig-scene, rig-math; peer of rig-import, not a consumer)
  ^
rig-app           (depends on rig-gpu, rig-render, rig-overlay, rig-scene, rig-assets, rig-import, rig-gltf, rig-anim, winit)
  ^
examples/         (depend on rig-app)
```

## Git workflow

- Always create a feature branch before making code or documentation changes.
  Do not commit implementation work directly on `main`/`master` unless the user
  explicitly requests it.
- Name branches descriptively, for example `feat/gltf-loader-demo` or
  `fix/loader-error-handling`.

## Build & run

This project uses a Nix flake for the development environment. nixGL is included to
bridge host GPU drivers (NVIDIA/Vulkan) into the Nix sandbox on non-NixOS Linux. nixGL
uses `builtins.currentTime` internally, which requires impure evaluation — so the dev
shell **must always be entered with `--impure`**:

```bash
nix develop --impure
```

This is normal and expected for GPU / CUDA / Vulkan development under Nix. Do not remove
`--impure` or attempt to remove nixGL to work around the flag requirement.

Git LFS is provided by the Nix development shell in this repository. Run Git LFS
operations and Git commands that trigger LFS hooks from inside the dev shell, for example
`nix develop --impure --command git push`.

Inside the dev shell:

```bash
# build entire workspace
cargo build --workspace

# run the hello-triangle example (prefix with nixGL on non-NixOS NVIDIA systems)
cargo run -p hello_triangle

# run tests
cargo test --workspace

# check without building
cargo clippy --workspace -- -D warnings
```

## Conventions

- **Rust edition**: 2024
- **Error handling**: `thiserror` for library crates, `anyhow` in examples.
- **Formatting**: `cargo fmt` (default rustfmt settings).
- **Linting**: `cargo clippy -- -D warnings` must pass.
- **Naming**: snake_case for files and modules, PascalCase for types, SCREAMING_SNAKE for constants.
- **Modules**: one public type per file where practical; re-export from `lib.rs`.
- **GPU code**: WGSL shaders, embedded via `include_str!` or loaded at runtime from `assets/`.
- **Platform code**: use `#[cfg(target_os = "...")]` only when absolutely necessary; prefer wgpu/winit abstractions.
- **Demo controls**: every framework demo should include `TrackBall` orbit/dolly controls plus `CameraRig` WASD/arrow-key camera control for now; document the controls in the example HUD or module docs.

## Architecture decisions (summary)

1. **Application pattern**: one `Application` trait + startup/update/render/overlay contexts, driven by a redraw-based runner.
2. **Scene graph**: arena-allocated storage with generational `NodeId` handles, first-child/next-sibling links, and scene-facing component maps.
3. **Asset model**: immutable shared assets (`MeshAsset`, `MaterialAsset`, `ShaderAsset`) referenced by typed handles.
4. **Renderer model**: concrete `wgpu` renderer with immutable resource caching and explicit frame-local allocations.
5. **Camera system**: active camera selected from scene camera nodes; `CameraRig` and `TrackBall` are opt-in utilities.
6. **GPU context**: `rig-gpu` owns device/queue/surface; `begin_frame` returns `Option<Frame>`; `Frame::present` submits and flips.
7. **Overlay system**: `rig-overlay` wraps glyphon; retained `ElementRegistry`; F3 toggles visibility; rendered after 3D scene with `LoadOp::Load`.

## Reference codebase

`GeometricTools/` is a C++14/OpenGL reference. It is read-only context for understanding scene-graph and rendering patterns. Key areas:

- `GTE/Applications/` — application hierarchy and camera controls
- `GTE/Graphics/` — engine, scene graph (`Spatial`, `Node`, `Visual`), effects, camera
- `GTE/Samples/Graphics/VertexColoring/` — minimal triangle sample

Do **not** compile, modify, or add GeometricTools to the Cargo workspace.

## Milestones

1. **Minimal triangle** — wgpu + winit, hardcoded vertices, no framework abstractions ✓
2. **Triangle via scene graph** — all core crates wired up, same triangle rendered through SceneGraph + AssetStore + Renderer + Application ✓
3. **Incremental features** — camera controls, frustum culling, lights, materials, MeshFactory, textures, multiple objects ✓
   - MeshFactory: box, sphere, plane, platonic solids (tetrahedron, hexahedron, octahedron, dodecahedron, icosahedron) ✓
    - `examples/geometry/platonic_solids` example: five solids orbiting with spin animation and fly-camera ✓
   - Frustum culling: `extract_renderables_culled` wired as default render path ✓
4. **Overlay system** — `rig-gpu` crate, `rig-overlay` crate (glyphon), FPS counters in all examples, F3 toggle ✓
5. **Texture support** — 3-group bind layout (frame/material/object), GPU texture/sampler cache, `TEXTURED_SHADER`, `examples/materials/textured_mesh` example ✓
6. **Lights + Phong shading** — `LightUniform`/`LightsBuffer` types, group 0 binding 1 (lights buffer), `pack_lights_buffer()`, `PHONG_SHADER` (Blinn-Phong), `examples/materials/lit_scene` example ✓
7. **Asset loading** — `rig-loader`, `rig-import`, OBJ/MTL, PNG/JPEG/TGA, runtime WGSL, seven progressive examples ✓
8. **Asset library + PLY loader** — Git LFS, curated OBJ model library (Stanford,
   Keenan Crane CC0), ambientCG PBR texture scaffolding, hand-rolled ASCII PLY
    decoder in rig-loader, combined BoundingSphere on LoadedModel,
    examples/loading/model_gallery CLI viewer ✓
9. **Rigid skeleton animation** — `rig-anim` crate, `AnimationClip` assets,
   cached keyframe sampling, bind-time channel resolution to scene nodes,
    `AnimationPlayer` evaluation into local transforms, `examples/animation/skeleton_demo` robot arm ✓
10. **CPU skinning** — `rig-skin` crate, `SkinEvaluator`, `SkinAsset`/`SkinWeights`
    asset types, 8-influence LBS with inverse-transpose normal skinning,
     `DynamicMesh` output path, `examples/animation/tentacle_demo` example ✓
11. **Material system + normal maps** — `rig-assets::tangent_utils` (mikktspace),
     48-byte vertex layout, 5-slot PBR bind group, `examples/materials/normal_map_demo` example ✓
12. **Procedural terrain** — `noise` crate in examples, marching cubes terrain,
    heightmap terrain, procedural normal maps, two terrain examples ✓
13. **Terrain sub-problems** — domain warping, hydraulic erosion,
    triplanar texturing, chunked infinite terrain, distance-based LOD,
    five progressive terrain examples ✓
14. **glTF loading and runtime validation** — `rig-gltf`, PBR material mapping,
    cameras/lights, multi-scene selection, morph target loading, CPU skinning
     descriptors, `examples/gltf/demo`, and `examples/gltf/skinned` ✓
15. **graphynx injection (recipe iii)** — flake input + Cargo workspace
    `exclude` + Nix-store symlink at `vendor/graphynx`; `nix develop` +
    `cargo run -p voice_metaballs` works without fragile relative paths ✓

## graphynx injection recipe

### Problem

An external Rust workspace (graphynx) with intra-repo path dependencies
(`core`, `backends`, `backends-cpu`, `runtime`) lives outside the Cargo
workspace but must be consumed by a workspace member
(`examples/procedural/voice_metaballs`). Relative paths (`../../rustycuda/*`)
are fragile — they break on CI, in Nix builds, and when the directory layout
changes.

### Three options

| Option | Mechanism | Pros | Cons |
|--------|-----------|------|------|
| **(i) Path deps + sibling checkout** | `Cargo.toml` paths point to `../rustycuda/core`, etc. | Zero flake setup; live edits in the sibling are immediately visible. | Fragile; sibling may not exist; CI/Nix build fails. |
| **(ii) `[patch]` in `.cargo/config.toml`** | Declare path-patch overrides for the four crate names in a local cargo config. | Resolves workspace collision; no `exclude` needed. | Hidden config; confusing when crate name collisions occur. |
| **(iii) Flake input + symlink + `exclude`** | Pin source in `flake.lock`, symlink into `vendor/`, exclude `vendor/` from workspace. | Reproducible, declarative, no fragile paths; CI-friendly. | Requires Nix; symlink + `exclude` needed to avoid Cargo workspace collision. |

### Implementation (recipe iii)

In the consuming project (`nodemoss`):

1. **`flake.nix`** — add graphynx as a standard flake input:
   ```nix
   graphynx = {
     url = "github:vansweej/graphynx";
     inputs.nixpkgs.follows = "nixpkgs";
   };
   ```

2. **`flake.nix` `shellHook`** — provision the symlink:
   ```bash
   mkdir -p vendor
   ln -sfn "${graphynx}" vendor/graphynx
   ```

3. **`.gitignore`** — add `/vendor/` so the symlink is never committed
   (it points into the Nix store, which is machine-specific).

4. **`Cargo.toml`** — add `"vendor"` to the workspace `exclude` list so
   Cargo does not attempt to merge graphynx's `[workspace]` into the
   consuming project's workspace.

5. **per-crate `Cargo.toml`** — point path dependencies at the symlink:
   ```toml
   graph-core   = { path = "vendor/graphynx/core" }
   backends     = { path = "vendor/graphynx/backends" }
   backends-cpu = { path = "vendor/graphynx/backends-cpu" }
   runtime      = { path = "vendor/graphynx/runtime", features = ["live-audio"] }
   ```

### Extensions

- **To (i)** — replace the path deps with `../rustycuda/*` and remove the
  symlink. Useful when doing cross-project refactors (graphynx edits must
  be picked up without a `nix develop` re-entry).
- **To (ii)** — if the `exclude` approach fails (e.g. Cargo canonicalises
  the symlink and still detects the inner workspace), add a
  `.cargo/config.toml` with `[patch]` entries mapping the four crate
  names to the `vendor/graphynx/*` paths, and remove the `exclude` entry.

## Deferred (not yet started, triggers documented in plan)

- **Roundtrip 2:** Pure `nix build .#voice_metaballs` via `rustPlatform.buildRustPackage` + `postPatch` copy. Trigger: when reproducible/CI builds of the example binary are wanted.
- **Roundtrip 3:** CUDA/GPU backend build support for graphynx. Trigger: when graphynx gains a GPU backend and `voice_metaballs` switches from `CpuBackend` to a CUDA backend.

## Documentation

All architecture docs live in `docs/` and use Mermaid diagrams extensively. Read them before making structural changes:

- `ARCHITECTURE.md` — start here for the big picture
- `SCENEGRAPH.md` — arena tree internals, components, traversal, culling
- `RESOURCES.md` — immutable assets, GPU cache, frame resources, pipeline specialization
- `APPLICATION.md` — Application trait, redraw-driven runner, contexts, camera utilities
- `LOADING.md` — AssetSource, Loader, Importer, cache behavior, loading examples
- `GLTF.md` — glTF loader architecture, adaptation map, runtime handoff, examples
- `MODELS.md` — model library provenance, texture conventions, auto-scaling patterns
