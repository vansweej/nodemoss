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
