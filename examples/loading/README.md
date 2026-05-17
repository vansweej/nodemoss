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
