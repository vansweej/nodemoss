## Feature
`DebugHud` helper in `rig-app` with dual-stack auto-layout and GPU adapter name display.

## Key decisions made
- Store full `wgpu::AdapterInfo` on `GpuContext` (not just the name string), making adapter metadata available to all downstream code.
- `DebugHud` lives in `rig-app`, alongside other opt-in utilities like `CameraRig` and `TrackBall`.
- `DebugHud` manages two independent auto-layout stacks: left side (anchored `TopLeft`) and right side (anchored `TopRight`).
- Built-in elements: GPU adapter name → left stack, FPS → right stack.
- Examples opt in explicitly by constructing a `DebugHud` in `init()` and calling `update()` in `update_overlay()`.
- Examples can extend either stack via `add_element(Side, text)` which returns an `ElementId` for per-frame updates.
- Camera position remains example-owned, added to the right stack via `add_element`.
- Per-element toggling (show/hide individual fields) is deferred to a future iteration.

## Open questions
- What font size and color should the GPU name use? Same as FPS (16pt white) or different to visually distinguish static info from dynamic counters?
- Should `DebugHud::new` register elements with sensible defaults (e.g., `"GPU: <name>"` prefix) or let the format be caller-controlled?
- What vertical spacing between stacked elements — fixed pixel gap, or derived from font size?
- Should `DebugHud` respect the F3 overlay toggle automatically (it already does, since it renders through `Overlay` which the runner toggles at `runner.rs:127`), or does it need its own visibility control?

## Rejected alternatives
- **Storing adapter info on `DebugHud` instead of `GpuContext`**: shifted the problem without solving it; someone still needs to extract adapter info from `GpuContext::new()`.
- **Storing only `adapter_name: String`**: full `AdapterInfo` has vendor, device type, backend, and driver info that's useful for future debug display.
- **Placing `DebugHud` in `rig-overlay`**: would create a dependency on `FrameTimer` which lives in `rig-app`, causing a circular dependency.
- **Runner-automatic debug HUD for all examples**: conflicts with the goal of opt-in for real applications.
- **`DebugHud` exposing `next_y()` instead of `add_element()`**: leaks positioning details to the example; `add_element` with auto-layout is cleaner.
- **Single-stack layout**: FPS belongs top-right, GPU name top-left; two stacks keeps both conventions and enables natural extension on either side.

## Risks identified
1. **Breaking change to `GpuContext`** — adding `adapter_info: wgpu::AdapterInfo` changes the struct layout. Since `GpuContext` is public and constructed only inside its own `new()`, this is low risk, but any code pattern-matching or destructuring on it will break.
2. **Layout fragility** — auto-stacking with pixel offsets may look wrong at different DPI scales or window sizes. May need DPI-aware spacing eventually.
3. **Migration churn** — existing examples that manually manage FPS overlay elements should migrate to `DebugHud` to avoid duplication and inconsistency, but this is optional work.

## Recommended next steps
1. Add `adapter_info: wgpu::AdapterInfo` field to `GpuContext` and retain it in `GpuContext::new()`.
2. Implement `DebugHud` struct in `rig-app` with `new()`, `add_element(Side, text)`, and `update()`.
3. Migrate `platonic_solids` example to use `DebugHud` as the proof-of-concept.
4. Update remaining examples that have FPS overlays.
