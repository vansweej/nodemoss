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
