# Decision Brief — CPU Marching Cubes Metaballs

## Feature
CPU Marching Cubes metaballs integrated into the scene graph, with `DynamicMesh` framework support and scene-wide wireframe toggle.

## Key decisions made
- **Scene graph integration** — dynamic meshes participate in the scene graph like static meshes; no parallel draw path, no `DynamicDrawCommand` parameter
- **`MeshSource` enum on `Renderable`** — `Static(MeshHandle)` | `Dynamic(DynamicMeshId)`; extraction, culling, draw sorting, visibility all work automatically
- **`DynamicMeshId` lives in `rig-assets`** — alongside `MeshHandle`, `MaterialHandle`, and all other handle types
- **`DynamicMesh` GPU buffers live on `Renderer`** — registered via `StartupContext` in `init()`, stored in `HashMap<DynamicMeshId, DynamicMesh>`, looked up during `record_scene_pass()` draw dispatch
- **Single buffer pair** for `DynamicMesh` — no double-buffering; `queue.write_buffer()` handles staging internally
- **Grow buffers on demand** — follow `ObjectUniformBuffer::ensure_capacity()` pattern, reallocate when marching cubes output exceeds current capacity
- **CPU-side staging in `update()`, GPU upload in `render()`** — marching cubes produces `Vec<u8>` vertex/index data stored on `MetaballApp`; `render()` calls `renderer.update_dynamic_mesh(gpu, id, &vertices, &indices)` before `render_scene()`
- **Use `Renderer`'s `fallback_material_bind_group`** for dynamic draws — white Phong with strong specular = polished chrome "liquid metal"
- **Wireframe is a scene-wide `Renderer` mode** — one `bool` toggle, affects all draws (static + dynamic), toggled by F4; framework feature in `rig-render`
- **Add `polygon_mode` to `PipelineKey`** — Fill and Line variants cached on demand
- **Check `adapter.features()` for `POLYGON_MODE_LINE`** — request only if supported; store `supports_wireframe: bool` on `GpuContext`; F4 degrades gracefully
- **Marching Cubes module in `rig-assets`** — pure CPU, gradient normals via central differences, Paul Bourke edge/tri tables, `standard_layout()` vertex format (pos + normal + UV, stride 32)
- **Documentation** — `docs/METABALLS.md` covers the algorithm, demo architecture, and all four implicit surface directions as a reference for future implementations

## Open questions
- **MC module output type**: raw `(Vec<u8>, Vec<u32>)` vs a lightweight `DynamicMeshData` struct with vertex bytes, index bytes, vertex count, index count, and bounding sphere. The latter is more self-describing and carries the bounds needed for frustum culling
- **UV coordinates**: `[0.0, 0.0]` placeholder — `PHONG_SHADER` doesn't sample UVs for untextured materials; confirm this stays true
- **Cull mode for dynamic draws**: `create_pipeline()` uses `cull_mode: Some(Face::Back)` (`helpers.rs:556`). Marching cubes with consistent winding should be fine, but thin features may need `cull_mode: None`. May need a per-`MeshSource` override or just disable backface culling globally for dynamic meshes
- **Bounding sphere for dynamic meshes**: marching cubes grid has fixed extents, so a conservative static bound works. But if ball positions push the surface near grid edges, the bound may need recomputing each frame. Who computes it — the MC module, the example, or `compute_world_bounds()`?
- **`render_scene()` signature**: with scene graph integration, the signature stays unchanged — dynamic meshes come through extraction. But `render_scene()` now needs `&self.dynamic_meshes` internally. Confirm no public API change is needed beyond adding `update_dynamic_mesh()` and `register_dynamic_mesh()` on `Renderer`

## Rejected alternatives
- **`DynamicDrawCommand` parameter on `render_scene()`** — replaced by scene graph integration; no parallel draw path
- **Separate render pass for dynamic draws** — adds redundant depth resolve; dynamic meshes draw in the main scene pass via unified extraction
- **Double-buffered GPU buffers** — unnecessary; `queue.write_buffer()` stages internally
- **Wireframe as per-draw flag** — scene-wide is more useful for a research framework
- **Custom material bind group for metaballs** — fallback white Phong gives chrome appearance
- **Dynamic meshes outside the scene graph** — loses culling, transforms, visibility, draw sorting; bad precedent for Directions 2–4
- **Add `gpu`/`renderer` to `UpdateContext`** — blurs update/render boundary; CPU staging in `update()` + GPU upload in `render()` is cleaner
- **`DynamicMeshId` in `rig-scene`** — all handle types live in `rig-assets` for consistency

## Risks identified
1. **`Renderable` enum change touches extraction + draw paths** — `extract_renderables()`, `extract_renderables_culled()`, `record_scene_pass()`, `prepare_draw_order()`, `compute_world_bounds()` all need branching on `MeshSource`. Contained within `rig-scene` + `rig-render` but significant surface area
2. **`POLYGON_MODE_LINE` not universally supported** — mitigated by adapter feature check + graceful degradation
3. **Marching cubes winding consistency** — inconsistent winding + backface culling = visual holes. May need `cull_mode: None` for dynamic meshes
4. **Buffer growth during gameplay** — reallocation creates new `wgpu::Buffer`, drops old one; possible frame hitch on first few frames before size stabilises at 48³
5. **Bounding sphere staleness** — if bounds are computed once and the metaball surface changes shape, frustum culling may clip visible geometry or fail to cull invisible geometry. Conservative bounds waste GPU; tight bounds risk popping

## Recommended next steps
1. **`docs/METABALLS.md`** — algorithm reference document covering: the metaball field function and iso-threshold, Marching Cubes algorithm (grid evaluation, edge tables, triangle tables, vertex interpolation, gradient normals via central differences), the `DynamicMesh` architecture and scene graph integration, demo controls and overlay, and a "Roadmap" section outlining all four implicit surface directions with their tradeoffs:
   - Direction 1: CPU Marching Cubes (this implementation)
   - Direction 2: GPU Compute Marching Cubes — same algorithm, field evaluation + MC in a compute shader, output to storage buffer, indirect draw
   - Direction 3: Ray Marching — no mesh extraction; fragment shader sphere-traces the field directly, screen-space normals
   - Direction 4: Dual Contouring — sharp feature preservation, hermite data on grid edges, QEF vertex placement
   Each direction section should describe the approach, where it differs from Direction 1, which crates it touches, expected performance characteristics, and what can be reused from prior directions
2. **`DynamicMeshId` handle in `rig-assets`** — define the type alongside existing handles
3. **`MeshSource` enum + `Renderable` refactor in `rig-scene`** — update `Renderable`, `ExtractedRenderable`, extraction methods, `compute_world_bounds()`
4. **`DynamicMesh` type + registry in `rig-render`** — `HashMap<DynamicMeshId, DynamicMesh>` on `Renderer`, `register_dynamic_mesh()`, `update_dynamic_mesh()`, draw dispatch in `record_scene_pass()`
5. **Wireframe support in `rig-render`** — `polygon_mode` in `PipelineKey`, `Renderer::set_wireframe()` / `Renderer::toggle_wireframe()`, conditional `POLYGON_MODE_LINE` in `rig-gpu`
6. **Marching Cubes module in `rig-assets`** — pure CPU, gradient normals, Paul Bourke tables, unit tests for vertex counts, index validity, normal unit length, winding consistency
7. **`examples/metaballs`** — field function, ball animation, MC extraction in `update()`, GPU upload in `render()`, overlay (FPS, tri count, grid res, wireframe indicator)
8. **Update `docs/ARCHITECTURE.md`** — add milestone 7 for implicit surfaces / dynamic meshes; reference `METABALLS.md`


## Implementation Plan

### Strategy
Work bottom-up through the crate dependency chain: `rig-assets` → `rig-scene` → `rig-gpu` → `rig-render` → `rig-app` → example. Each phase is independently compilable and testable before moving to the next.

### Phase 1: Documentation — `docs/METABALLS.md`
- Algorithm reference: metaball field function, MC algorithm, gradient normals, DynamicMesh architecture
- Demo description: controls, overlay, visual targets
- Roadmap: four implicit surface directions (CPU MC, GPU Compute MC, Ray Marching, Dual Contouring) with approach/crates/perf/reuse for each

### Phase 2: `rig-assets` — handles, MeshSource, Marching Cubes
- `DynamicMeshId` handle type (Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)
- `MeshSource` enum: `Static(MeshHandle)` | `Dynamic(DynamicMeshId)`
- `DynamicMeshData` struct: vertex_data, index_data, index_count, local_bounds
- `marching_cubes` module: `extract(field, params, iso_value) -> DynamicMeshData`
  - Paul Bourke edge/tri tables (256 entries each)
  - Grid evaluation, edge interpolation, vertex deduplication via HashMap
  - Gradient normals via central differences (6 field evals per vertex)
  - UVs: [0.0, 0.0] placeholder
  - Bounding sphere computed from output vertices
- Tests: empty field, uniform above, single sphere, index validity, normal unit length, winding consistency, vertex stride, bounding sphere, resolution scaling

### Phase 3: `rig-scene` — Renderable refactor + dynamic bounds
- `Renderable.mesh` changes from `MeshHandle` to `MeshSource`
- Add `Renderable::new(mesh, material)` and `Renderable::dynamic(id, material)` convenience constructors
- `ExtractedRenderable.mesh` changes to `MeshSource`
- `SceneGraph` gains `dynamic_bounds: HashMap<NodeId, BoundingSphere>`
- `set_dynamic_bounds()` / `dynamic_bounds()` public methods
- `compute_world_bounds` branches on `MeshSource::Static` vs `MeshSource::Dynamic`
- `destroy_node` cleans up `dynamic_bounds`
- Re-export `DynamicMeshId`, `MeshSource` from `lib.rs`
- Update all 35 `Renderable { mesh, material }` call sites to use `Renderable::new()`
- Tests: dynamic bounds set/get, compute_world_bounds with dynamic, destroy cleanup, extraction with dynamic

### Phase 4: `rig-gpu` — wireframe feature detection
- Check `adapter.features().contains(POLYGON_MODE_LINE)` in `GpuContext::new()`
- Conditionally add to `required_features`
- Store `pub supports_wireframe: bool` on `GpuContext`

### Phase 5: `rig-render` — DynamicMesh, wireframe, draw dispatch
- `PipelineKey` gains `polygon_mode: wgpu::PolygonMode`
- `create_pipeline` gains `polygon_mode` parameter (replaces hardcoded Fill)
- `DynamicMesh` struct: vertex_buffer, index_buffer, capacities, index_count, vertex_layout, index_format
- `Renderer` gains: `dynamic_meshes: HashMap<DynamicMeshId, DynamicMesh>`, `next_dynamic_id: u32`, `wireframe: bool`
- `register_dynamic_mesh()`: allocate buffers, return DynamicMeshId
- `update_dynamic_mesh()`: grow-on-demand (next_power_of_two), queue.write_buffer
- `toggle_wireframe(supports_wireframe)`, `wireframe() -> bool`
- `record_scene_pass` draw loop: branch on `MeshSource::Static` vs `Dynamic` for buffer/layout lookup
- `prepare_draw_order` sort key: `MeshSource` needs `Ord` (add derive)
- `current_mesh` optimization: compare `MeshSource` instead of `MeshHandle`
- Update all existing tests (PipelineKey polygon_mode, ExtractedRenderable MeshSource)

### Phase 6: `rig-app` — F4 wireframe toggle
- Runner `KeyboardInput` handler: F4 calls `renderer.toggle_wireframe(gpu.supports_wireframe)`
- Alongside existing F3 overlay toggle

### Phase 7: `examples/metaballs`
- `Cargo.toml` + add to workspace members
- `MetaballApp`: 4 bouncing balls (radii 0.8–1.2), bounce off ±4.0 bounding box
- Field: `f(p) = Σ rᵢ² / |p - cᵢ|²`, threshold 1.0
- `init()`: PHONG_SHADER + material, register dynamic mesh, camera, directional light, overlay
- `update()`: animate balls, run `marching_cubes::extract()`, store staged data + update dynamic bounds
- `render()`: `update_dynamic_mesh()` then `render_scene()`
- `update_overlay()`: FPS, tri count, grid res "48³", wireframe status
- Controls: fly-camera (WASD/QE/arrows), Escape exit, F3 overlay, F4 wireframe

### Phase 8: Update `docs/ARCHITECTURE.md`
- Add Milestone 7: dynamic meshes + implicit surfaces
- Reference `METABALLS.md`
- Update GTE mapping table

### Phase 9: Final verification
- `cargo fmt --all`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- `cargo tarpaulin --workspace` — target ≥90% on non-GPU code
- Manual run: `cargo run -p metaballs` + verify existing examples
