# Feature: glTF Enhancements, Cleanup, Documentation, and Phase E Planning

## Phase 1: Code review and loader cleanup

Commit message: `refactor(gltf): clean up loader internals and expand test coverage`

### Step 1: Simplify mesh adaptation return values

In `crates/gltf/src/meshes.rs`, simplify `adapt_mesh` so it returns `Vec<MeshHandle>` instead of `Vec<(MeshHandle, Option<usize>)>`. The material index tuple entry is currently unused by the caller because `loader.rs` re-reads primitive material indices while wiring renderables. Update `crates/gltf/src/loader.rs` to consume the simplified return type.

### Step 2: Add focused buffer reader tests

Add tests for buffer-reader edge cases in `crates/gltf/src/buffers.rs`. Cover missing required `POSITION` data, absent inverse bind matrices falling back to identity matrices, and helper behavior for unavailable buffer data. Prefer minimal inline fixtures or helper-level tests without introducing a new dependency.

### Step 3: Add texture conversion and sampler tests

Add tests in `crates/gltf/src/textures.rs` for image format conversion and sampler mapping. Cover `R8`, `R8G8B8`, `R8G8B8A8`, float channel clamping, wrap-mode mapping, and min/mag filter mapping.

### Step 4: Add node, camera, light, and material edge-case tests

Expand `rig-gltf` test coverage for empty scene handling, default material behavior, unsupported or missing data paths, and light/camera adaptation helpers. Keep tests close to the module owning the behavior.

### Step 5: Add file-path context to load errors

In `crates/gltf/src/error.rs`, add an error variant that includes the source path when `gltf::import` fails. In `crates/gltf/src/loader.rs`, wrap import failures with that path-aware error so CLI examples report which `.gltf` or `.glb` failed to load.

---

## Phase 2: Orthographic cameras and spot lights

Commit message: `feat(gltf): support orthographic cameras and spot lights`

### Step 1: Add or wire orthographic projection support

Check `rig_math::Projection`. If it does not already support orthographic cameras, add an orthographic variant that stores the view volume and produces the correct projection matrix for the renderer. Update any camera extraction code that pattern-matches `Projection` so existing perspective cameras keep working.

### Step 2: Adapt glTF orthographic cameras

In `crates/gltf/src/cameras.rs`, replace the current warning-and-skip behavior for `gltf::camera::Projection::Orthographic` with real adaptation. Map glTF `xmag`, `ymag`, `znear`, and `zfar` into the engine orthographic projection type.

### Step 3: Add spot light component support

Extend `rig_scene::LightKind` with a `Spot` variant that stores color, intensity, range, inner cone angle, and outer cone angle. Update extraction code and any tests that match all light variants.

### Step 4: Adapt KHR_lights_punctual spot lights

In `crates/gltf/src/lights.rs`, replace the current warning-and-skip behavior for glTF spot lights with adaptation into `LightKind::Spot`. Preserve default range behavior when glTF omits range.

### Step 5: Pack and shade spot lights

Update `rig-render` light uniform packing and WGSL lighting code so spot lights attenuate by range and cone angle. Derive spot direction from the light node world transform. Ensure directional and point lights continue to render exactly as before.

### Step 6: Add orthographic and spot light tests

Add tests for orthographic projection matrix creation, orthographic glTF camera adaptation, spot light adaptation, and renderer light packing of the new spot light variant.

---

## Phase 3: Multi-scene glTF loading API

Commit message: `feat(gltf): add scene selection API for multi-scene files`

### Step 1: Add SceneSelection API

In `crates/gltf/src/loader.rs`, add a public `SceneSelection` enum with `Default`, `Index(usize)`, and `Name(String)` variants. Keep `load_gltf` as the default-scene convenience function.

### Step 2: Add load_gltf_scene entry point

Add `pub fn load_gltf_scene(...) -> Result<LoadedGltf>` that accepts a `SceneSelection` argument and otherwise follows the current `load_gltf` flow. Update `load_gltf` to call `load_gltf_scene` with `SceneSelection::Default`.

### Step 3: Move scene resolution out of nodes.rs

Change `crates/gltf/src/nodes.rs` so `adapt_nodes` accepts a resolved `gltf::Scene<'_>` instead of internally selecting the default scene. Add a helper that resolves `SceneSelection` against the document.

### Step 4: Add scene-not-found errors

Add a `GltfError::SceneNotFound` variant that reports whether selection failed by index or by name. Use this error from the scene-resolution helper.

### Step 5: Export and document scene selection

Re-export `SceneSelection` and `load_gltf_scene` from `crates/gltf/src/lib.rs`. Update the crate-level example to show both default-scene loading and explicit scene selection.

### Step 6: Add multi-scene tests

Add tests for default scene selection, valid index selection, invalid index selection, valid name selection, and invalid name selection. Use a small inline or fixture glTF file with at least two scenes.

---

## Phase 4: Morph target loading and CPU evaluation

Commit message: `feat(gltf): add morph target loading and CPU evaluation`

### Step 1: Extend animation asset types for morph weights

In `crates/assets/src/animation.rs`, add `ChannelProperty::MorphTargetWeights` and matching `KeyframeValues` variants for linear/step and cubic morph weight data. Ensure existing transform animation code remains source-compatible.

### Step 2: Add morph target asset storage

In `crates/assets`, add a `MorphTargets` asset type plus a `MorphTargetHandle`. Store per-target position and normal deltas in a form that can be blended with a base `MeshAsset`. Add `AssetStore` registration and lookup methods.

### Step 3: Read morph target accessors from glTF

In `crates/gltf/src/buffers.rs`, add helpers for reading glTF primitive morph target `POSITION` and `NORMAL` displacement data. Treat absent normal deltas as zero deltas.

### Step 4: Register morph targets while adapting meshes

In `crates/gltf/src/meshes.rs`, detect primitive morph targets, register a `MorphTargets` asset, and return the optional handle alongside the mesh handle. Update `LoadedGltf` so morph target handles are available parallel to `meshes` and `skin_weights`.

### Step 5: Adapt morph target animation channels

In `crates/gltf/src/animations.rs`, stop skipping `gltf::animation::Property::MorphTargetWeights`. Read output values as morph weight vectors and produce `AnimationChannel` values using `ChannelProperty::MorphTargetWeights`.

### Step 6: Add a CPU MorphEvaluator

Add a CPU evaluator for morph targets, either in `rig-skin` beside `SkinEvaluator` or in a new small module if separation is clearer. The evaluator should accept a base mesh, morph target handle, and runtime weights, then output `DynamicMeshData` for the existing dynamic mesh renderer path.

### Step 7: Expose evaluated morph weights from AnimationPlayer

Update `rig-anim` so evaluating morph-weight channels does not try to set a scene transform. Store evaluated weights per target node and expose them through an accessor that examples and evaluators can consume.

### Step 8: Add morph target tests

Add tests for morph target asset registration, glTF morph target reading, morph weight animation sampling, and CPU morph blending of a minimal triangle mesh.

---

## Phase 5: Skinned glTF runtime demo

Commit message: `feat(examples): add gltf_skinned_demo with CPU skinning`

### Step 1: Expose skinned primitive descriptors from LoadedGltf

In `crates/gltf/src/loader.rs`, add a public descriptor type that identifies each loaded skinned primitive: node, mesh handle, skin handle, skin weights handle, material handle, and primitive index. Populate this while wiring renderables so examples do not have to reverse-engineer glTF node/mesh/skin relationships.

### Step 2: Acquire BrainStem sample asset

Add Khronos `BrainStem.glb` under `assets/models/gltf/BrainStem.glb` and track it through Git LFS from inside the Nix development shell. Use it as the default model for the skinned glTF demo.

### Step 3: Add gltf_skinned_demo example crate

Create `examples/gltf_skinned_demo` and add it to the workspace. Depend on `rig-app`, `anyhow`, and `env_logger`, matching the style of `examples/gltf_demo`.

### Step 4: Wire AnimationPlayer and SkinEvaluator

In the new demo, load the model, create an `AnimationPlayer` for the first animation, bind it, create one `SkinEvaluator` per skinned primitive descriptor, and register dynamic mesh IDs for those primitives.

### Step 5: Update skinned dynamic meshes each frame

Each frame, advance and evaluate animation, update world transforms, run every `SkinEvaluator`, update corresponding dynamic mesh data in the renderer, then render the scene. Keep non-skinned primitives static.

### Step 6: Add demo controls and overlay

Include `CameraRig`, `TrackBall`, `DebugHud`, Escape to quit, and F3 overlay toggling. Document controls in the module-level docs and HUD.

### Step 7: Verify demo asset and runtime path

Run `nixGL cargo run -p gltf_skinned_demo` manually after the code compiles. Confirm the asset loads, the animation advances, and skinned meshes deform through the dynamic mesh renderer path.

---

## Phase 6: glTF documentation

Commit message: `docs(gltf): add loader architecture doc and rustdoc coverage`

### Step 1: Create docs/GLTF.md

Create `docs/GLTF.md` describing the `rig-gltf` crate, its dependency boundaries, loading flow, adaptation map, material mapping, animation/skinning pipeline, morph target handling, multi-scene API, examples, and known limitations.

### Step 2: Add loading flow diagrams

Include Mermaid diagrams for parse/adapt/register flow, material slot mapping, and animation/skin runtime flow. Reuse terminology from `docs/MATERIAL.md` and `docs/ANIMATION.md`.

### Step 3: Improve crate-level rustdoc

Expand `crates/gltf/src/lib.rs` docs with supported feature coverage, example usage for `load_gltf`, example usage for `load_gltf_scene`, and a note pointing readers to `docs/GLTF.md`.

### Step 4: Add public API rustdoc

Add or improve doc comments for all public `rig-gltf` types and functions, especially `LoadedGltf`, skinned primitive descriptors, `SceneSelection`, `load_gltf`, and `load_gltf_scene`.

### Step 5: Update architecture documentation references

Update the root `AGENTS.md` documentation list to include `docs/GLTF.md`. Add a glTF milestone summary to `docs/ARCHITECTURE.md` that reflects the implemented loader, enhanced feature coverage, and examples.

---

## Phase 7: Phase E roadmap document

Commit message: `docs: add Phase E plan for production glTF rendering`

### Step 1: Create docs/plans/PLAN_PHASE_E.md

Create a Phase E plan focused on production glTF rendering features: alpha modes, double-sided materials, and skinned glTF runtime validation. Mark it as planned work, not implemented behavior.

### Step 2: Plan alpha modes

Document `AlphaMode::Opaque`, `AlphaMode::Mask { cutoff }`, and `AlphaMode::Blend`. Include expected changes to `MaterialAsset`, `MaterialParams` or material uniforms, pipeline keys, shader discard logic, blend state, transparent pass ordering, and depth-write behavior.

### Step 3: Plan double-sided rendering

Document how glTF `material.doubleSided` maps to an engine material field and renderer cull mode. Include expected pipeline-key changes and shader implications.

### Step 4: Plan skinned glTF runtime validation

Document how `BrainStem.glb` and future skinned models validate `rig-gltf` + `AnimationPlayer` + `SkinEvaluator` end to end. Call out expected demo behavior and verification commands.

### Step 5: Record open questions

Capture open design questions for transparent sorting, order-independent transparency, alpha-test pass placement, and whether future material extension slots should be implemented before or after alpha handling.

### Step 6: Update MATERIAL.md status

Update `docs/MATERIAL.md` status text to reflect that Phase A-D are complete and Phase E is planned. Link to `docs/plans/PLAN_PHASE_E.md` from the roadmap or open-questions area.

---

## Phase 8: Final verification

Commit message: `test(gltf): verify enhanced loader and examples`

### Step 1: Format the workspace

Run `nix develop --impure --command cargo fmt --check`. If formatting fails, run `nix develop --impure --command cargo fmt` and re-check.

### Step 2: Typecheck the workspace

Run `nix develop --impure --command cargo check --workspace` and fix all errors.

### Step 3: Run tests

Run `nix develop --impure --command cargo test --workspace` and fix all failures.

### Step 4: Run clippy

Run `nix develop --impure --command cargo clippy --workspace -- -D warnings` and fix all warnings.

### Step 5: Run demos manually

Run `nixGL cargo run -p gltf_demo` and `nixGL cargo run -p gltf_skinned_demo` from inside the development shell or with the same GPU setup used for the existing examples. Confirm both demos load assets, render, and respond to camera controls.
