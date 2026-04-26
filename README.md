# nodemoss

NodeMoss is a personal 3D and physics research framework in Rust built around a hierarchical scene tree.

## Status

- Cross-platform target: Linux and macOS
- Graphics stack: `wgpu` + `winit` + `glam`
- Current milestones implemented:
  - `hello_triangle` — minimal direct `wgpu` triangle
  - `triangle_scenegraph` — triangle rendered through `scene + assets + render + app`
  - `mesh_showcase` — procedural mesh primitives (box, sphere, plane, platonic solids)
  - `multi_object` — multiple objects with shared assets, depth, and visibility toggling
  - `offscreen_demo` — offscreen render target with fullscreen blit pass
 - `platonic_solids` — five platonic solids orbiting with fly-camera navigation
 - `textured_mesh` — sphere with procedurally generated checkerboard texture, demonstrates GPU texture/sampler caching
 - `trackball_demo` — arc-ball orbit camera with mouse drag (LMB = orbit, RMB = dolly)
 - All framework examples include an FPS counter overlay (press **F3** to toggle)

## Workspace

- `crates/math` — math primitives and camera/projection helpers
- `crates/scene` — arena-based scene graph with generational node handles
- `crates/assets` — immutable shared assets and procedural mesh generation (box, sphere, plane, platonic solids)
- `crates/gpu` — wgpu device/queue/surface context (`rig-gpu`)
- `crates/render` — concrete `wgpu` renderer
- `crates/overlay` — 2D text overlay via glyphon, retained element registry (`rig-overlay`)
- `crates/app` — runtime runner and app shell

## Reference Code

`GeometricTools/` is included as read-only reference material. It is not part of the Cargo workspace and is not compiled by this project.

## Development

```bash
nix develop --impure
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

### NVIDIA GPU (non-NixOS Linux)

On non-NixOS Linux with a proprietary NVIDIA driver, the Nix dev shell
cannot see the host GPU libraries. This causes Vulkan loader errors like:

```
libGLX_nvidia.so.0: cannot open shared object file: No such file or directory
loader_icd_scan: Failed loading library associated with ICD JSON libGLX_nvidia.so.0
```

The scene may still render via a software fallback, but without hardware acceleration.

The dev shell includes [nixGL](https://github.com/nix-community/nixGL) to solve this.
nixGL auto-detects the host NVIDIA driver version at evaluation time, which requires
impure Nix evaluation — hence `nix develop --impure` above.

Prefix GPU-using commands with `nixGL` so the host NVIDIA driver is injected into the
library path and wgpu can use the hardware Vulkan implementation:

```bash
nixGL cargo run -p hello_triangle
nixGL cargo run -p triangle_scenegraph
```

> **macOS and NixOS users:** `nixGL` is not needed — GPU drivers are already visible
> to Nix on these platforms. `nix develop --impure` still works, but the `--impure`
> flag is only strictly required for the nixGL NVIDIA integration.
