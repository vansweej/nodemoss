# Refactor & Feature Roadmap

This document describes the planned improvement rounds for the nodemoss workspace,
ordered by risk and dependency. Each round is self-contained and ends with a clean
commit on its own branch.

---

## Round 1 — Correctness fixes (complete ✓)

Branch: `fix/review-round-1`

| Slice | Commit | Summary |
|---|---|---|
| 1 | `be084ed` | `rig-scene`: reject cyclic reparenting (`CycleDetected`) |
| 2 | `865b917` | `rig-scene`: validate active cameras; inherited visibility via `effective_visibility()` |
| 3 | `b8e9a8a` | `rig-render`: key immutable caches by typed asset handle, not content hash |
| 4 | `34a227d` | `rig-render`: bind pipelines by full `PipelineKey`; propagate camera errors |
| 5 | `4bb669e` | `rig-app`: `UpdateContext::request_exit()`; replace `expect()` with graceful shutdown |
| 6 | `8b95fa6` | `rig-overlay`: all 9 `Anchor` variants; `measure_buffer_width` from glyph layout |
| 7 | `3b3d08e` | Examples: use `ctx.request_exit()`; `platonic_solids` elapsed → `f64` |
| 8 | `ac24349` | Docs: reconcile `APPLICATION.md`, `SCENEGRAPH.md`, `RESOURCES.md` |
| gate | `a79715d` | `cargo fmt`; `#[cfg(not(tarpaulin_include))]` on GPU-only code; coverage 97.4% |

---

## Round 2 — Coverage completeness (complete ✓)

**Goal**: reach 100% on the lines that are reachable without a GPU.  
**Risk**: low — pure unit tests, no API changes.  
**Branch**: `fix/coverage-round-2`

| Slice | Crate | Tests added | Missing scenario covered |
|---|---|---|---|
| 1 | `rig-scene` | `create_node_reuses_free_list_slot` | free-list reuse path |
| 2 | `rig-scene` | `detach_middle_child_updates_sibling_chain` | middle-child prev-sibling pointer |
| 3 | `rig-scene` | `renderable_nodes_returns_all_renderable_node_ids` | `renderable_nodes()` iterator |
| 4 | `rig-scene` | `world_transforms_propagate_to_grandchild` | recursive grandchild transform |
| 5 | `rig-render` | `aligned_uniform_size_alignment_zero_or_one` | `alignment <= 1` edge case |
| 6 | `rig-render` | `vertex_format_size_all_variants` | `Float32`, `Float32x2` match arms |
| 7 | `rig-render` | `wgpu_vertex_format_all_variants` | all four format variants |
| 8 | `rig-render` | `decompose_pose_identity` | identity matrix decomposition |
| 9 | `rig-render` | `decompose_pose_translation_only` | translation-only decomposition |
| 10 | `rig-render` | `camera_projection_view_produces_finite_matrix` | `camera_projection_view` function |
| 11 | `rig-math` | `bounding_sphere_union_coincident_centers` | distance==0 fallback direction branch |
| 12 | `rig-app` | `camera_rig_pitch_changes_rotation` | ArrowUp pitch branch in `CameraRig::update` |

**Deliverable**: 12 targeted unit tests across 4 crates.

| gate | `7badcb7` | `cargo fmt`; all tests pass; clippy clean |

---

## Round 3 — Module splitting (refactor, behaviour-neutral)

**Goal**: break up the three large `lib.rs` files into focused submodules.  
**Risk**: low — pure file moves, zero logic changes.  
**Branch**: `refactor/module-split`

| Crate | Current size | Proposed submodules |
|---|---|---|
| `rig-scene` | ~1530 lines | `node.rs`, `graph.rs`, `components.rs`, `extraction.rs`, `traversal.rs` |
| `rig-render` | ~1460 lines | `cache.rs`, `pipeline.rs`, `frame.rs`, `renderer.rs`, `helpers.rs` |
| `rig-app` | ~715 lines | `runner.rs`, `context.rs`, `input.rs`, `timer.rs`, `camera_rig.rs` |

Each crate keeps its `lib.rs` as a thin re-export facade (`pub use submodule::*`).
All public APIs and test modules stay in place. One commit per crate.

---

## Round 4 — Frustum culling in the renderer

**Goal**: wire the already-implemented `extract_renderables_culled` into the render path.  
**Risk**: medium — touches the hot render path.  
**Branch**: `feat/frustum-culling`

Steps:

1. Extract the active camera frustum planes from `ExtractedCamera` inside `render_scene`.
2. Pass them into `extract_renderables_culled` (already implemented in `rig-scene`).
3. Switch `render_scene` and `render_to_target` to use the culled draw list.
4. Add a unit test: place objects outside the frustum and assert they are absent from the
   extracted list.
5. Optional: add a `culled_count` field to a frame stats struct for overlay display.

**Affected files**: `rig-render/src/lib.rs`, `rig-app/src/lib.rs`, one example.

---

## Round 5 — Mouse input and TrackBall controller

**Goal**: implement the mouse input path and the TrackBall utility noted as "planned" in
`APPLICATION.md`.  
**Risk**: medium — new event wiring in the runner.  
**Branch**: `feat/mouse-trackball`

Steps:

1. Extend `InputState` with `mouse_buttons`, `mouse_position`, `mouse_delta`.
2. Wire `CursorMoved` and `MouseInput` winit events in the runner.
3. Implement `TrackBall`: arc-ball rotation around a target `NodeId` via scene mutation APIs.
4. Expose `TrackBall` from `rig-app` alongside `CameraRig`.
5. Wire it into `platonic_solids` or a new dedicated example.
6. Update `APPLICATION.md` section 9.2 to remove the "not yet implemented" note.

**Affected files**: `rig-app/src/lib.rs`, one example, `docs/APPLICATION.md`.

---

## Round 6 — Texture support end-to-end

**Goal**: upload and bind `TextureAsset` in the renderer; apply a texture to a mesh in an
example.  
**Risk**: high — touches WGSL shaders, bind group layout, and the pipeline cache.  
**Branch**: `feat/textures`

Steps:

1. Add texture upload to `ImmutableResourceCache` (keyed by `TextureHandle`).
2. Add a sampler cache entry (keyed by a `SamplerDescriptor` hash or handle).
3. Add a texture + sampler bind group slot to the pipeline layout.
4. Write a new WGSL shader variant that samples a texture.
5. Load a PNG in `mesh_showcase` (or a new example) and apply it to a quad or sphere.
6. Update `RESOURCES.md` to document the texture cache entry.

**Affected files**: `rig-assets`, `rig-render`, WGSL shaders, one example,
`docs/RESOURCES.md`.

---

## Round 7 — Lights and basic Phong shading

**Goal**: extract light data from the scene and implement a Phong shading model in WGSL.  
**Risk**: high — new uniform buffer, new extraction path, new shader.  
**Branch**: `feat/lighting`

Steps:

1. Implement `LightComponent` extraction alongside renderables (already stubbed in
   `rig-scene`).
2. Pack extracted lights into a uniform buffer passed to the shader each frame.
3. Write a Phong WGSL shader (ambient + diffuse + specular).
4. Add a `platonic_solids` variant (or new example) that uses the lit shader.
5. Update `RESOURCES.md` and `SCENEGRAPH.md` with the light extraction boundary.

**Affected files**: `rig-scene`, `rig-render`, WGSL shaders, one example, docs.

---

## Dependency order

```
Round 1 (done)
  └─► Round 2   (coverage — no deps)
  └─► Round 3   (module split — no deps)
        └─► Round 4  (culling — needs render internals to be navigable)
              └─► Round 5  (mouse/trackball — independent, but benefits from clean app module)
              └─► Round 6  (textures — needs clean render module)
                    └─► Round 7  (lighting — builds on texture pipeline layout)
```

Rounds 2 and 3 can be done in any order or in parallel. Rounds 4–7 should be done in
sequence because each one extends the render pipeline layout that the next round depends on.
