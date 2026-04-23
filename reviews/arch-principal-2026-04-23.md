# Architecture Review: rig-scene (Scene Graph)

**Date**: 2026-04-23
**Reviewer**: Principal Software Architect
**Scope**: `crates/scene/src/lib.rs` — the `rig-scene` crate (~565 lines of production code, ~700 lines of tests)
**Supporting context**: `docs/SCENEGRAPH.md`, `crates/render/src/lib.rs` (consumer), `crates/app/src/lib.rs` (consumer), examples

---

## 1. Summary Verdict

The scene graph is well-designed for its current scope. The arena storage with generational handles, the clean separation from GPU concerns, and the extraction-based renderer boundary are all sound architectural choices. The implementation is readable, well-tested, and correctly models the core problem — hierarchical transforms with culling support.

The issues identified below are not structural flaws. They are friction points that will emerge as the scene grows in object count, hierarchy depth, or feature breadth. None requires a redesign.

---

## 2. What Works Well

### 2.1 Clean ownership boundary

The scene graph contains zero GPU types. No `wgpu::Buffer`, no `ShaderModule`, no pipeline handles. This is exactly right. The `Renderable` component stores only `MeshHandle` and `MaterialHandle` — opaque identifiers resolved by the renderer. This means the scene graph is fully testable without a GPU, and can survive a renderer rewrite.

### 2.2 Generational handles

`NodeId { index: u32, generation: u32 }` with a free-list is the correct pattern for an arena that supports deletion. The destroy path increments the generation and pushes to the free list. Stale handles fail validation deterministically. No silent aliasing.

### 2.3 Extraction boundary

`extract_renderables()`, `extract_renderables_culled()`, `extract_active_camera()`, and `extract_lights()` produce owned, self-contained value structs (`ExtractedRenderable`, `ExtractedCamera`, `ExtractedLight`). This avoids borrowing the `SceneGraph` across the renderer's mutable operations. Clean hand-off.

### 2.4 Hierarchy is simple and correct

First-child/next-sibling linked list is compact and appropriate for moderate fan-out trees. `attach_child` detaches first (preventing dangling sibling pointers), rejects self-parenting, and inserts at the head of the child list. `detach_child` walks the sibling list correctly.

### 2.5 Test quality

The tests cover the important correctness properties: generational reuse, reparenting, visibility filtering, frustum culling, world-transform propagation, and bounds computation. Good test/production ratio for a core data structure.

---

## 3. Findings — Issues and Risks

### F1. `VisibilityMode::Inherit` does not actually inherit

**Severity**: Medium (semantic incorrectness)

`VisibilityMode::Inherit` is documented as the default, but nothing in the code actually resolves the inheritance chain. If a parent is `Hidden` but its children are `Inherit`, `extract_renderables()` and `extract_renderables_culled()` will still emit the children.

The extraction methods only check the node's own visibility:

```rust
if matches!(visibility, VisibilityMode::Hidden) {
    return None;
}
```

True inheritance would require walking ancestors or propagating an effective-visibility flag during traversal.

**Impact**: A user who hides a subtree root expects the entire subtree to vanish. It won't. This is either a naming bug (it should be `Default` or `Normal` instead of `Inherit`) or a missing feature.

**Recommendation**: Either rename `Inherit` to `Normal` / `Default` to avoid the false promise, or implement actual downward inheritance during the transform/bounds update pass.

---

### F2. Recursive traversal will stack-overflow on deep hierarchies

**Severity**: Low (unlikely with current use, but a latent failure mode)

`update_world_transforms_with_parent`, `compute_world_bounds`, and `destroy_node` are all recursive. Each call to `children()` allocates a `Vec<NodeId>`. For a tree of depth `D`, the stack depth is `O(D)` and the total allocation count is `O(N)` (one vec per node).

At moderate depth (hundreds), this is fine. At depth ~10,000+, this will overflow the default Rust stack (8 MB on Linux).

**Impact**: Not a problem for the current use case (hand-authored scenes). Becomes a problem if procedurally generated deep chains (e.g., physics ragdoll chains, particle trail histories) are parented into the scene.

**Recommendation**: No action needed now. If deep hierarchies become realistic, convert to iterative traversal using an explicit stack (`Vec<(NodeId, Mat4)>`). This would also eliminate the per-node `Vec<NodeId>` allocation in `children()`.

---

### F3. `children()` allocates a `Vec` on every call

**Severity**: Low (performance, not correctness)

`children()` collects all children into a `Vec<NodeId>` and returns it. This is called during `update_world_transforms`, `compute_world_bounds`, and `destroy_node` — once per node, every frame.

For a scene with 1,000 nodes, this is 1,000 small allocations per frame during transform propagation alone.

**Impact**: Negligible at current scale. Will show up in profiling at ~10k nodes.

**Recommendation**: For traversal-internal use, consider an inline `ChildIterator` that walks `first_child → next_sibling` without allocating. The public `children()` can remain as-is for convenience.

---

### F4. `root_nodes()` is O(N) over the entire arena

**Severity**: Low

`root_nodes()` scans all slots to find nodes where `parent.is_none()`. Called by `update_all_world_transforms()` and `update_all_world_bounds()` — i.e., every frame.

**Impact**: Negligible at current scale. At 100k slots (with many destroyed/reused), this becomes measurable.

**Recommendation**: Maintain a `HashSet<NodeId>` of roots, updated during `attach_child`, `detach_child`, `create_node`, and `destroy_node`. Low effort, eliminates the scan.

---

### F5. HashMap iteration order makes extraction non-deterministic

**Severity**: Low (correctness is fine, but reproducibility suffers)

`extract_renderables()`, `extract_renderables_culled()`, and `extract_lights()` iterate over `HashMap` keys. `HashMap` iteration order is non-deterministic across runs (and even across re-hashes within a run).

The renderer sorts by `(shader, mesh)` before drawing, so visual output is stable. But the extraction order itself varies, which can make debugging and frame-by-frame comparison difficult.

**Impact**: No visual bugs. Minor friction for debugging and deterministic replay/testing.

**Recommendation**: Accept as-is. If deterministic ordering becomes important (e.g., for frame capture tools), switch to `IndexMap` or sort the extracted list by `NodeId`.

---

### F6. No cycle detection in `attach_child`

**Severity**: Low (exploitable only by the application author)

`attach_child` prevents self-parenting (`parent == child`) but does not detect deeper cycles (A→B→C→A). If a caller creates a cycle, `update_world_transforms` will loop infinitely.

**Impact**: In a personal research framework where the API caller is also the framework author, this is acceptable. In a library consumed by third parties, it would be a bug.

**Recommendation**: Accept as-is. Document that the caller must not create cycles. If a defensive check is ever needed, add an ancestor-walk check in `attach_child` (O(depth), bounded by tree height).

---

### F7. `rig-scene` depends on `rig-assets` — coupling worth monitoring

**Severity**: Observation (not a defect)

`rig-scene` imports `rig_assets::{AssetStore, MaterialHandle, MeshHandle}`. The scene graph needs these handles for `Renderable` and needs `AssetStore` for `compute_world_bounds` (to read `local_bounds` from the mesh asset).

This is a reasonable coupling today. The risk is that `rig-assets` grows features (texture streaming, async loading, LOD selection) and that complexity leaks back into the scene graph via the `AssetStore` parameter.

**Recommendation**: Keep the coupling narrow. The only thing the scene graph should ever ask of assets is "give me the bounding sphere for this mesh handle." If the `AssetStore` API grows, consider passing a trait/closure (e.g., `Fn(MeshHandle) -> BoundingSphere`) instead of the full store.

---

### F8. `extract_active_camera` error ordering is ambiguous

**Severity**: Cosmetic

`extract_active_camera` checks the `cameras` HashMap first, then accesses the node:

```rust
let camera = self.cameras.get(&id).ok_or(SceneError::NotACamera)?;
let world_transform = self.node(id)?.world_transform;
```

If `id` is both invalid AND not a camera, the caller gets `NotACamera` (from the HashMap miss), not `InvalidNode`. The test at line 1043 acknowledges this ambiguity. It's not a bug, but it muddies diagnostics.

**Recommendation**: Swap the order — validate the node first, then check the component. Errors should report the most fundamental problem first (invalid handle > missing component).

---

### F9. No way to remove a component without destroying the node

**Severity**: Low (missing API)

There is `set_renderable`, `set_camera`, `set_light`, but no `remove_renderable`, `remove_camera`, `remove_light`. The only way to strip a component is to destroy and recreate the node, losing its hierarchy position and transform.

**Impact**: Minor inconvenience. Real use case: a node that transitions from renderable to non-renderable at runtime (e.g., picked up item disappears from world).

**Recommendation**: Add `remove_renderable(NodeId) -> Result<()>` etc. when the need arises. Trivial to implement (`self.renderables.remove(&id)`).

---

## 4. Scalability Assessment

| Dimension | Current state | Pressure point |
|-----------|--------------|----------------|
| **Node count** | Fine up to ~10k | `root_nodes()` scan, `children()` alloc per node |
| **Tree depth** | Fine up to ~1,000 | Stack overflow from recursion |
| **Component density** | Fine | HashMap overhead per component type |
| **Frame rate** | Fine | Full transform propagation every frame, even if nothing moved |
| **Multi-scene** | Supported (multiple root nodes) | No explicit scene isolation or scene-switching |

### Dirty-flag propagation (future consideration)

Currently `update_all_world_transforms()` recomputes every node every frame, regardless of whether anything changed. At scale, this is wasteful. A dirty-flag scheme (mark a node dirty when its local transform changes, propagate downward) would make the common case (few things move) much cheaper.

This is not needed now. It's the obvious next optimisation when profiling shows transform propagation dominating.

---

## 5. Reliability Assessment

| Property | Status |
|----------|--------|
| Stale handle detection | Correct — generational check in `slot()` / `slot_mut()` |
| Deletion safety | Correct — recursive child destruction, component cleanup, generation bump |
| Thread safety | Single-threaded only (`&mut self` everywhere). Fine for the current model. |
| Error handling | Consistent `Result<T, SceneError>` throughout. No panics in production paths. |
| Memory leaks | None identified — destroyed nodes go to free list, components removed from maps. |

---

## 6. Maintainability Assessment

| Property | Status |
|----------|--------|
| Single-file crate | Acceptable at ~565 LOC. Split into modules at ~1,000 LOC. |
| Public API surface | Narrow and well-documented. Private internals hidden behind methods. |
| Component extensibility | Easy — add a new `HashMap<NodeId, T>` and a setter/getter pair. |
| Test coverage | High. All public APIs tested. Edge cases (stale handles, reparenting) covered. |

---

## 7. Summary of Recommendations

| # | Finding | Priority | Action |
|---|---------|----------|--------|
| F1 | `Inherit` doesn't inherit | **Medium** | Rename to `Normal`/`Default`, or implement inheritance |
| F2 | Recursive traversal | Low | Convert to iterative when deep trees are realistic |
| F3 | `children()` allocates | Low | Add `ChildIterator` for internal traversal |
| F4 | `root_nodes()` is O(N) | Low | Track roots in a set |
| F5 | HashMap ordering | Low | Accept; switch to IndexMap if needed |
| F6 | No cycle detection | Low | Document; add ancestor-walk guard if needed |
| F7 | scene→assets coupling | Observation | Keep narrow; consider trait boundary later |
| F8 | Camera error ordering | Cosmetic | Validate node before component lookup |
| F9 | No component removal | Low | Add `remove_*` methods when needed |

**The only finding I'd recommend addressing soon is F1** — the `Inherit` naming creates a false expectation. Everything else is fine for the project's current stage and can be addressed incrementally as the framework grows.
