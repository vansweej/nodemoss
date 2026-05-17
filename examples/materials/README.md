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
