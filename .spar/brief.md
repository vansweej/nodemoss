## Feature
Add `RunConfig` struct with GPU adapter preference, defaulting to high-performance (NVIDIA) GPU selection.

## Key decisions made
- `GpuContext::new` gains a `wgpu::PowerPreference` parameter (not a full config struct — GPU context stays lean)
- A `RunConfig` struct is introduced in `rig-app` with `title` and `power_preference` fields
- `RunConfig::default()` sets `power_preference` to `HighPerformance` — an opinionated default for this research framework
- `run()` takes `RunConfig` instead of `impl Into<String>`
- `hello_triangle` is left unchanged — it's an intentionally raw wgpu example
- No config file yet; that's a future concern and `RunConfig` is the natural target for deserialization when it arrives

## Open questions
- Should `RunConfig` also absorb window size (currently hardcoded to 800×600 at `runner.rs:64`) while we're touching the struct, or keep it minimal for now?
- Should the adapter name be logged at `INFO` or `WARN` level to make GPU selection more visible during debugging?

## Rejected alternatives
- **Environment variable only** (`WGPU_ADAPTER_NAME`, `DRI_PRIME=1`) — doesn't provide a code-level default; easy to forget at launch time
- **Bare parameter on `run()`** — doesn't scale; next config knob means another signature change and 8 example updates
- **`Application` trait method** returning preference — over-abstracted for a single field; config structs are more idiomatic

## Risks identified
1. **Opinionated default may confuse contributors** — `HighPerformance` as default diverges from wgpu's `None`; must be clearly documented
2. **8 examples + 1 internal test must update** — mechanical but error-prone; ensure `cargo test --workspace` and `cargo clippy` pass after
3. **Intel-only systems** — `HighPerformance` is a *hint*, not a guarantee; wgpu will still pick the best available adapter, so no functional risk

## Recommended next steps
1. Add `PowerPreference` parameter to `GpuContext::new` in `crates/gpu/src/lib.rs`
2. Create `RunConfig` struct in `crates/app/src/` (new module or in `runner.rs`), with `Default` impl and clear doc comments
3. Update `run()` and `Runner` to accept `RunConfig`
4. Update all 8 examples and the `runner_new_starts_empty` test
5. `cargo clippy --workspace -- -D warnings` and `cargo test --workspace`
