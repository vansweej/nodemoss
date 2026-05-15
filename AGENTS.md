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
    MODELS.md                   # curated model/texture library, provenance, viewer usage
  crates/
    math/                       # rig-math    — glam re-exports + Transform, BoundingSphere, Projection, Camera
    scene/                      # rig-scene   — arena SceneGraph, generational NodeId, cameras/lights/renderables
    assets/                     # rig-assets  — immutable meshes, materials, shader source, textures
    loader/                     # rig-loader  — file/source abstraction + image/OBJ/PLY/WGSL decoders
    import/                     # rig-import  — decoded asset adaptation into AssetStore assets + model bounds
    anim/                       # rig-anim    — AnimationPlayer, binding table, keyframe sampling
    gpu/                        # rig-gpu     — GpuContext (device/queue/surface), Frame, GpuError
    render/                     # rig-render  — concrete wgpu renderer, immutable cache, frame resources
    overlay/                    # rig-overlay — 2D text overlay (glyphon), retained ElementRegistry
    app/                        # rig-app     — Application trait, runner, startup/update/render/overlay contexts
  examples/
    hello_triangle/             # milestone 1 — colored triangle (raw wgpu + winit, no framework)
    triangle_scenegraph/        # milestone 2 — triangle via scene graph + Application trait
    mesh_showcase/              # milestone 3 — MeshFactory primitives
    multi_object/               # milestone 3 — multiple objects, camera rig
    offscreen_demo/             # milestone 3 — offscreen render target + blit
    platonic_solids/            # milestone 3 — five animated solids, fly-camera, overlay
    skeleton_demo/              # milestone 9 — rigid skeleton animation via AnimationPlayer
    obj_load/                   # milestone 7 — geometry-only OBJ loading
    obj_textured/               # milestone 7 — OBJ + MTL diffuse texture loading
    multi_obj/                  # milestone 7 — importer cache demonstration
    texture_load/               # milestone 7 — texture file on procedural mesh
    texture_formats/            # milestone 7 — PNG/JPEG/TGA loading
    shader_load/                # milestone 7 — runtime WGSL loading
    asset_showcase/             # milestone 7 — combined loading showcase
    model_gallery/              # milestone 8 — CLI model viewer over curated assets
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

## Crate dependency order

```
rig-math          (leaf — depends only on glam)
  ^
rig-scene         (depends on rig-math)
  ^
rig-assets        (depends on rig-math)
  ^
rig-loader        (leaf — depends on image, tobj, thiserror; PLY decoded without external crate)
  ^
rig-import        (depends on rig-loader, rig-assets, rig-math; LoadedModel carries combined BoundingSphere)
  ^
rig-anim          (depends on rig-math, rig-assets, rig-scene; AnimationPlayer + binding table)
  ^
rig-gpu           (depends on wgpu, winit)
  ^
rig-render        (depends on rig-gpu, rig-math, rig-scene, rig-assets)
  ^
rig-overlay       (depends on rig-gpu, glyphon)
  ^
rig-app           (depends on rig-gpu, rig-render, rig-overlay, rig-scene, rig-assets, rig-import, rig-anim, winit)
  ^
examples/         (depend on rig-app)
```

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
   - `platonic_solids` example: five solids orbiting with spin animation and fly-camera ✓
   - Frustum culling: `extract_renderables_culled` wired as default render path ✓
4. **Overlay system** — `rig-gpu` crate, `rig-overlay` crate (glyphon), FPS counters in all examples, F3 toggle ✓
5. **Texture support** — 3-group bind layout (frame/material/object), GPU texture/sampler cache, `TEXTURED_SHADER`, `textured_mesh` example ✓
6. **Lights + Phong shading** — `LightUniform`/`LightsBuffer` types, group 0 binding 1 (lights buffer), `pack_lights_buffer()`, `PHONG_SHADER` (Blinn-Phong), `lit_scene` example ✓
7. **Asset loading** — `rig-loader`, `rig-import`, OBJ/MTL, PNG/JPEG/TGA, runtime WGSL, seven progressive examples ✓
8. **Asset library + PLY loader** — Git LFS, curated OBJ model library (Stanford,
   Keenan Crane CC0), ambientCG PBR texture scaffolding, hand-rolled ASCII PLY
   decoder in rig-loader, combined BoundingSphere on LoadedModel,
   model_gallery CLI viewer ✓
9. **Rigid skeleton animation** — `rig-anim` crate, `AnimationClip` assets,
   cached keyframe sampling, bind-time channel resolution to scene nodes,
   `AnimationPlayer` evaluation into local transforms, `skeleton_demo` robot arm ✓

## Documentation

All architecture docs live in `docs/` and use Mermaid diagrams extensively. Read them before making structural changes:

- `ARCHITECTURE.md` — start here for the big picture
- `SCENEGRAPH.md` — arena tree internals, components, traversal, culling
- `RESOURCES.md` — immutable assets, GPU cache, frame resources, pipeline specialization
- `APPLICATION.md` — Application trait, redraw-driven runner, contexts, camera utilities
- `LOADING.md` — AssetSource, Loader, Importer, cache behavior, loading examples
- `MODELS.md` — model library provenance, texture conventions, auto-scaling patterns
