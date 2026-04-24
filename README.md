# nodemoss

NodeMoss is a personal 3D and physics research framework in Rust built around a hierarchical scene tree.

## Status

- Cross-platform target: Linux and macOS
- Graphics stack: `wgpu` + `winit` + `glam`
- Current milestones implemented:
  - `hello_triangle` — minimal direct `wgpu` triangle
  - `triangle_scenegraph` — triangle rendered through `scene + assets + render + app`
  - `multi_object` — multiple objects with shared assets, depth, and visibility toggling
  - `offscreen_demo` — offscreen render target with fullscreen blit pass
  - `platonic_solids` — five platonic solids orbiting with fly-camera navigation

## Workspace

- `crates/math` — math primitives and camera/projection helpers
- `crates/scene` — arena-based scene graph with generational node handles
- `crates/assets` — immutable shared assets and procedural mesh generation (box, sphere, plane, platonic solids)
- `crates/render` — concrete `wgpu` renderer
- `crates/app` — runtime runner and app shell

## Reference Code

`GeometricTools/` is included as read-only reference material. It is not part of the Cargo workspace and is not compiled by this project.

## Development

```bash
nix develop
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```
