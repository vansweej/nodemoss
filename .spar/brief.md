# Brief: PBR Material Pipeline — Phase A

## Goal
Implement the Phase A material system: 48-byte vertex layout with tangents,
5-slot PBR material binding, normal map sampling, and a `normal_map_demo` example.
Phases B–D (terrain, glTF) follow after Phase A is complete.

## Constraints & Preferences
- One vertex layout everywhere (48 bytes, stride 48) — no conditional paths
- One 11-binding material bind group layout for ALL shaders — fallback textures for unused slots
- `mikktspace` lives in `rig-assets/tangent_utils` — importers call in, no duplication
- `noise` stays in example crates only — no `rand`/`num-traits` in leaf crates
- Each phase produces a compilable, testable commit
- Memory cost accepted freely — consistency over savings

## Progress

### Done
- Brainstormed six directions → chose four-phase arc: A (material) → B (terrain) → C (terrain sub-problems) → D (glTF)
- Wrote `docs/MATERIAL.md` (~1325 lines, 15+ Mermaid diagrams) — all known issues fixed
- Updated `AGENTS.md`: `rig-assets` notes `+ mikktspace`, added `rig-gltf` to crate order
- Committed: `4203a04 docs: add MATERIAL.md roadmap and update AGENTS.md for Phase A–D arc`
- Resolved all five Phase A/B blocking open questions
- Produced detailed Phase A implementation plan (7 commits, see below)

### In Progress
- Phase A, Phase 1: tangent_utils module (not yet started)

### Blocked
- (none)

## Phase A Implementation Plan

### Phase 1 — Tangent utility module
**Commit:** `feat(assets): add tangent_utils module with mikktspace and normal-derived tangent generation`

1. Add `mikktspace = "0.3"` to `[workspace.dependencies]` in root `Cargo.toml` (after `tobj = "4"`).
   Add `mikktspace.workspace = true` to `crates/assets/Cargo.toml`.
2. Create `crates/assets/src/tangent_utils.rs` with:
   - `pub fn generate_tangents(positions, normals, uvs, indices) -> Vec<[f32; 4]>` — mikktspace impl of `Geometry` trait; falls back to `normal_derived_tangent` per vertex if mikktspace fails.
   - `pub fn normal_derived_tangent(normal: [f32; 3]) -> [f32; 4]` — `T = normalize(cross(N, UP))`, fallback axis `RIGHT` when N≈UP; returns `[Tx, Ty, Tz, 1.0]`.
   - `pub fn has_valid_uvs(uvs: &[f32], vertex_count: usize) -> bool` — true if `uvs.len() == vertex_count * 2` and at least one non-zero UV.
3. Add `pub mod tangent_utils;` to `crates/assets/src/lib.rs` after `pub mod marching_cubes;`.
4. Unit tests in `tangent_utils.rs`: normal-derived orthogonality, UP-facing fallback, has_valid_uvs, simple quad mikktspace round-trip.

### Phase 2 — Expand vertex layout to 48 bytes
**Commit:** `feat(assets): expand standard vertex layout to 48 bytes with tangent attribute`

1. `mesh_factory.rs`: `STRIDE: u64 = 32` → `48`; add `VertexAttribute { shader_location: 3, format: Float32x4, offset: 32 }` to `standard_layout()`; update doc comment.
2. `push_vertex(buf, pos, normal, uv)` → `push_vertex(buf, pos, normal, uv, tangent: [f32; 4])`.
3. `create_box`: call `normal_derived_tangent(face_normal)` per face; pass to `push_vertex`.
4. `create_sphere`: tangent = `[-sin(theta), 0, cos(theta), 1.0]` per vertex; pole fallback via `normal_derived_tangent`.
5. `create_plane`: tangent = `[1, 0, 0, 1.0]` for all vertices.
6. Platonic solids (5 functions): `normal_derived_tangent(normal)` per vertex.
7. `lib.rs`: update `DynamicMeshData` and `standard_vertex_layout()` doc comments (stride 32 → 48).
8. `marching_cubes.rs`: append `normal_derived_tangent(normal)` per vertex; capacity `* 8` → `* 12` floats.
9. `importer.rs`: `interleave_vertices` gains `tangents: &[[f32; 4]]` param; capacity `* 8` → `* 12`; writes tangent floats after UV. `import_decoded_mesh` calls `has_valid_uvs` → `generate_tangents` or map `normal_derived_tangent`; passes result to `interleave_vertices`.
10. Run `cargo test -p rig-assets` and `cargo test -p rig-import`; fix errors.

### Phase 3 — MaterialAsset texture slots to Vec of Options
**Commit:** `refactor(assets): change MaterialAsset.textures to Vec<Option<(TextureHandle, SamplerHandle)>>`

1. `lib.rs`: `textures: Vec<(TextureHandle, SamplerHandle)>` → `Vec<Option<(TextureHandle, SamplerHandle)>>` with slot-index doc comment (0=base color, 1=normal, 2=metallic-roughness, 3=occlusion, 4=emissive).
2. Add `impl MaterialAsset { pub fn untextured(...) -> Self; pub const SLOT_COUNT: usize = 5; }`.
3. `importer.rs`: material construction → `textures: vec![Some((tex, samp))]` for diffuse slot 0; other slots `None`.
4. `renderer.rs` (lines 647–735): `!material.textures.is_empty()` → `material.textures.iter().any(|s| s.is_some())`; `material.textures[0]` → `material.textures.get(0).and_then(|s| *s)`; `flags` encodes populated slots bitwise.
5. All examples: `textures: vec![(t, s)]` → `textures: vec![Some((t, s))]`; `textures: vec![]` unchanged.
   Affected: `textured_mesh`, `lit_scene`, `obj_textured`, `texture_load`, `texture_formats`, `asset_showcase`, `model_gallery`, `crates/app/src/lib.rs` if applicable.
6. `cargo build --workspace && cargo test --workspace`.

### Phase 4 — Expand renderer to 11-binding material bind group
**Commit:** `feat(render): expand material bind group to 11 bindings with PBR fallback textures`

1. `Renderer` struct: add `fallback_normal_texture_view: wgpu::TextureView` (1×1 `[128,128,255,255]`) and `fallback_black_texture_view: wgpu::TextureView` (1×1 `[0,0,0,255]`); both share `fallback_sampler`.
2. `material_bind_group_layout`: expand from 3 to 11 entries — binding 0 = uniform, bindings 1–10 = 5×(Texture2D + Sampler), all Fragment visibility, filterable float textures.
3. Rewrite bind group creation block: compute `flags` from slot presence; for each of 5 slots resolve real or fallback texture view + sampler; create single `BindGroup` with all 11 entries; no textured/untextured branching.
4. Verify `PipelineLayout` still references `&self.material_bind_group_layout` for group 1.

### Phase 5 — PBR shader with TBN and 5-slot sampling
**Commit:** `feat(render): add PBR shader with tangent-space normal mapping and 5-slot material sampling`

1. Create `crates/render/src/shaders/pbr.wgsl` (or embed inline in `helpers.rs`).
   - Vertex inputs: `@location(0) position`, `@location(1) normal`, `@location(2) uv`, `@location(3) tangent: vec4f`.
   - Group 0: `FrameUniforms` (binding 0) + `LightsBuffer` (binding 1).
   - Group 1: `MaterialUniforms` (binding 0) + 5×(texture+sampler) (bindings 1–10).
   - Group 2: `ObjectUniforms` (binding 0).
   - Fragment: TBN construction → conditional normal map override → 5-slot material sampling → Blinn-Phong lighting loop → emissive add → output.
2. Declare `pub const PBR_SHADER: &str` in `helpers.rs`.

### Phase 6 — Refactor existing shaders to 11-binding layout
**Commit:** `refactor(render): update all shaders to declare 11-binding material group and tangent attribute`

1. `PHONG_SHADER`: add `@location(3) tangent: vec4f` to vertex input; declare all 11 group-1 bindings; keep Blinn-Phong logic; only sample binding 1 (base color) with `flags & 1` check.
2. `TEXTURED_SHADER`: same vertex input addition; declare 11 bindings; sample only binding 1.
3. `NORMAL_COLOR_SHADER`: add tangent input; declare 11 bindings; no texture sampling (visualizes normals as colors).
4. `TRIANGLE_SHADER`: add tangent input; declare 11 bindings; minimal pass-through.
5. `cargo build --workspace && cargo clippy --workspace -- -D warnings && cargo test --workspace`.

### Phase 7 — normal_map_demo example
**Commit:** `feat(examples): add normal_map_demo showcasing PBR normal mapping`

1. Create `examples/normal_map_demo/Cargo.toml` + `src/main.rs`; add to workspace `members`.
2. `startup()`: create plane/sphere mesh; generate 64×64 procedural normal map (sine-wave bumps encoded as tangent-space normals); build `MaterialAsset` with `PBR_SHADER`, slot 0 = white base color, slot 1 = normal map; add directional light; position camera.
3. `update()`: optionally rotate light to show normal map effect.
4. Run `cargo run -p normal_map_demo`; verify no GPU validation errors, visible bump shading, F3 overlay toggle.

## After Phase A
- Phase B: noise terrain (Perlin/fBm in example crates, noise-driven marching cubes, heightmap mesh, noise normal map)
- Phase C: terrain sub-problems (domain warping, erosion, chunking, LOD, triplanar texturing)
- Phase D: `rig-gltf` crate (glTF 2.0 parsing, adaptation into engine asset types, full PBR from file)

## Key Decisions
- **48-byte `standard_layout()` globally** — every mesh source emits tangents; no conditional paths; one layout everywhere
- **Single 11-binding material layout for ALL shaders** — fallback textures for unused slots; one `PipelineLayout`
- **MC emits normal-derived tangents** — `T = normalize(cross(N, up))` with fallback axis when N≈up
- **`mikktspace` moves to `rig-assets`** — shared `tangent_utils` module; valid UVs → mikktspace, degenerate UVs → normal-derived
- **`rig-gltf` is peer of `rig-import`**, not consumer — depends on `gltf`, `rig-assets`, `rig-scene`, `rig-math`
- **5-slot PBR binding layout** matching glTF spec with 1×1 fallback textures (3 textures serving 5 slots)
- **`noise` in example crates only** — avoids pulling `rand`/`num-traits` into `rig-assets`

## Critical Context
- 5 shaders to update: `PBR_SHADER`, `PHONG_SHADER`, `TEXTURED_SHADER`, `NORMAL_COLOR_SHADER`, `TRIANGLE_SHADER` (all in `helpers.rs`)
- ~14 material construction sites across examples + importer
- Renderer bind group creation at `renderer.rs:645–735` — checks `material.textures.is_empty()` and indexes `textures[0]`; uses raw pointer casts for texture_view/sampler from cache
- `MaterialUniforms` already has `flags: u32` field — ready for 5-bit feature flags
- `mesh_factory.rs:44` has `const STRIDE: u64 = 32` → changes to 48
- `mesh_factory.rs:73` `push_vertex` takes `(pos, normal, uv)` → gains tangent parameter
- `interleave_vertices` at `importer.rs:308` — capacity `vertex_count * 8` → `vertex_count * 12`, adds tangent slice param
- `import_decoded_mesh` at `importer.rs:249` — builds normals, calls `interleave_vertices`; tangent generation inserts between these
- Test `all_mesh_factory_layouts_pass_standard_layout` at `mesh_factory.rs:1303` — passes if all factories updated consistently
- MC has `[0,0]` placeholder UVs → normal-derived tangents, not mikktspace
- Workspace: 14 internal crates + 23 examples; edition 2024, rust-version 1.85, wgpu 29.0.1
- `tentacle_demo` / `skeleton_demo` use custom vertex layouts (skinning) — unaffected by `standard_layout` change
- Risks: big-bang MaterialAsset type change touches many files; WGSL validation strict on all 5 shader rewrites

## Relevant Files
- `docs/MATERIAL.md`: comprehensive roadmap — decisions in §8.1; updated to reflect Phase A implementation plan
- `AGENTS.md`: updated with `rig-gltf` crate and `mikktspace` in `rig-assets`
- `Cargo.toml` (root): workspace members (37 entries), workspace deps — `mikktspace` to be added
- `crates/assets/Cargo.toml`: currently has `rig-math` + `thiserror` — gains `mikktspace`
- `crates/assets/src/lib.rs`: `MaterialAsset` (textures type change), `standard_vertex_layout()`, handle types
- `crates/assets/src/mesh_factory.rs`: `STRIDE=32`, `push_vertex`, `standard_layout()`, all shape functions, test at line 1303
- `crates/assets/src/marching_cubes.rs`: `extract()` — needs 48-byte stride + normal-derived tangents
- `crates/render/src/helpers.rs`: all 5 shader constants + `MaterialUniforms` with `flags: u32`
- `crates/render/src/renderer.rs`: `material_bind_group_layout` (3 bindings), bind group creation (lines 645–735), fallback texture/sampler
- `crates/render/src/pipeline.rs`: `PipelineKey` with `vertex_layout` field
- `crates/import/src/importer.rs`: `interleave_vertices` (line 308), `import_decoded_mesh` (line 249) — will call `rig_assets::tangent_utils`
