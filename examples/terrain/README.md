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
