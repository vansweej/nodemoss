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

## Round 2 — Coverage completeness

**Goal**: reach 100% on the lines that are reachable without a GPU.  
**Risk**: low — pure unit tests, no API changes.  
**Branch**: `fix/coverage-round-2`

The 21 remaining uncovered lines all exercise pure-Rust logic:

| Crate | Lines | Missing scenario |
|---|---|---|
| `rig-scene` | 138 | `create_node` free-list reuse path (destroy a node, then create another) |
| `rig-scene` | 151, 218, 224–225 | `detach_child` with a middle-of-list child (prev-sibling pointer update) |
| `rig-scene` | 289–290 | `renderable_nodes()` iterator — never called in tests |
| `rig-scene` | 361 | `update_world_transforms_with_parent` recursive grandchild path (only one level tested) |
| `rig-render` | 858 | `aligned_uniform_size` with `alignment <= 1` edge case |
| `rig-render` | 967, 975–976, 991–992, 995, 997 | `vertex_format_size` / `wgpu_vertex_format` for `Float32`, `Float32x2`, `Float32x4`; `decompose_pose`; `camera_projection_view` — partial match-arm coverage |
| `rig-assets` | 36, 56, 66 | `AssetStore::mesh/material/shader` getters for missing handles (error path) |
| `rig-math` | 76 | One uncovered branch in `BoundingSphere` or `Projection` |
| `rig-app` | 138 | `CameraRig::update` pitch branch (requires scene node + input state) |

**Deliverable**: ~15 targeted unit tests, one commit.

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
