# Idea Brief: Examples Directory Reorganization

## Idea
Reorganize the flat `examples/` directory into domain-based subdirectories following the GeometricTools `Samples/` pattern, with decoupled package names, workspace globs, and maintained per-group READMEs.

## Context
The project has grown to 31 examples across 14 milestones. The flat structure makes it hard to navigate, discover related examples, or understand progression within a topic. GeometricTools (`GTE/Samples/Graphics/`, `SceneGraphs/`, `Physics/`) provides a proven model for this kind of organization in a graphics research framework.

## Explored directions
1. **By domain/topic** — Group by what the example demonstrates. ✓ Chosen.
2. **By milestone/progression** — Numbered directories for learning order. Rejected (brittle numbering).
3. **By crate dependency** — Group by which framework crate is exercised. Rejected (unbalanced buckets).
4. **Hybrid with prefix-stripping** — Variant of #1 with cleaner inner names. Folded into chosen direction.

## Chosen direction
Domain/topic grouping with:
- Prefix-stripped directory names where the parent directory provides context
- Package names unchanged (decoupled from filesystem path)
- Workspace globs for auto-discovery of new examples
- `default-members` restricting bare `cargo build` to library crates and tools only
- `examples/shared/` as a thin lib crate replacing the loose `shared_loading_example.rs`
- `README.md` at top level and per group, maintained as examples grow
- Group name `materials` (modern mainstream over legacy "shading")
- Group name `techniques` for rendering technique examples (offscreen, future: shadows, post-processing)

## Final structure

```text
examples/
  README.md                      # index of all groups + how to run
  shared/                        # thin lib crate for shared loading utilities
    Cargo.toml                   #   pkg name: example-shared (no rig- prefix — not a framework crate)
    src/lib.rs                   #   exports run_loading_example() + ExampleKind as pub
  basics/
    README.md
    hello_triangle/              # pkg: hello_triangle
    triangle_scenegraph/         # pkg: triangle_scenegraph
    trackball_demo/              # pkg: trackball_demo
  geometry/
    README.md
    mesh_showcase/               # pkg: mesh_showcase
    multi_object/                # pkg: multi_object
    platonic_solids/             # pkg: platonic_solids
  techniques/
    README.md
    offscreen_demo/              # pkg: offscreen_demo
  materials/
    README.md
    textured_mesh/               # pkg: textured_mesh
    lit_scene/                   # pkg: lit_scene
    normal_map_demo/             # pkg: normal_map_demo
  loading/
    README.md
    obj_load/                    # pkg: obj_load
    obj_textured/                # pkg: obj_textured
    multi_obj/                   # pkg: multi_obj
    texture_load/                # pkg: texture_load
    texture_formats/             # pkg: texture_formats
    shader_load/                 # pkg: shader_load
    asset_showcase/              # pkg: asset_showcase
    model_gallery/               # pkg: model_gallery
  animation/
    README.md
    skeleton_demo/               # pkg: skeleton_demo
    tentacle_demo/               # pkg: tentacle_demo
  terrain/
    README.md
    marching_cubes/              # pkg: terrain_mc
    heightmap/                   # pkg: terrain_heightmap
    warp/                        # pkg: terrain_warp
    erosion/                     # pkg: terrain_erosion
    triplanar/                   # pkg: terrain_triplanar
    chunks/                      # pkg: terrain_chunks
    lod/                         # pkg: terrain_lod
  gltf/
    README.md
    demo/                        # pkg: gltf_demo
    skinned/                     # pkg: gltf_skinned_demo
  procedural/
    README.md
    metaballs/                   # pkg: metaballs
    voice_metaballs/             # pkg: voice_metaballs
```

## Two breaking changes — both must be fixed before any `git mv`

The directory moves themselves are pure renames with zero risk. All the risk is in two
pre-existing code patterns that break the moment examples move one level deeper. Both
must be resolved and verified at the **current flat structure** before touching any paths.

### Breaking change 1: `include!()` with relative paths (7 examples)

Seven loading examples use textual inclusion to share ~400 lines of application code:

```rust
include!("../../shared_loading_example.rs");  // in e.g. examples/obj_load/src/main.rs
```

After the move to `examples/loading/obj_load/src/main.rs` the path becomes wrong.
Even if adjusted, `include!()` is a hack — it prevents IDE navigation, causes duplicate
compilation, and makes refactoring fragile.

**Fix:** Create `examples/shared/` as a proper lib crate. The public API surface is
intentionally tiny — only two symbols need to be `pub`:

```rust
pub fn run_loading_example(kind: ExampleKind) -> Result<()>
pub enum ExampleKind { ObjLoad, ObjTextured, MultiObj, TextureLoad, TextureFormats, ShaderLoad, AssetShowcase }
```

Everything else (`LoadingExampleApp`, helper functions, `EXAMPLE_KIND` static) stays
`pub(crate)`. Each of the 7 examples adds `example-shared.workspace = true` to its
`Cargo.toml` and replaces the `include!()` with normal `use` imports.

**Affected examples:** `obj_load`, `obj_textured`, `multi_obj`, `texture_load`,
`texture_formats`, `shader_load`, `asset_showcase`.

### Breaking change 2: relative `path` dependencies (14 examples)

Fourteen examples use `path = "../../crates/app"` style dependencies. After moving one
level deeper these paths resolve to `../../../crates/app` and the build fails.

**Fix:** Convert all relative path deps to workspace deps (`rig-app.workspace = true`).
This is a mechanical find-and-replace — the pattern is already established by the 17
examples that already use workspace deps correctly.

**Affected examples:** `terrain_mc`, `terrain_heightmap`, `obj_load`, `obj_textured`,
`multi_obj`, `texture_load`, `texture_formats`, `shader_load`, `asset_showcase`,
`metaballs`, `voice_metaballs`, `lit_scene`, `textured_mesh`, `trackball_demo`.

Note: `obj_load`, `obj_textured`, `multi_obj`, `texture_load`, `texture_formats`,
`shader_load`, `asset_showcase` appear in both lists — they need both fixes.

## Cargo.toml normalization (while we're touching every file)

The 14 path-dep examples also have inconsistent `Cargo.toml` style. Since every file
must be touched to fix path deps, normalize all 31 examples to the workspace pattern.
The template is minimal — examples that need additional crates (e.g. `terrain_mc` needs
`rig-assets.workspace = true` and `rig-math.workspace = true` beyond `rig-app`) keep
those entries, just converted to workspace syntax:

```toml
[package]
name = "example_name"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
anyhow.workspace = true
env_logger.workspace = true
```

Specific cleanups:
- Remove explicit `[[bin]]` sections — Cargo auto-discovers `src/main.rs`. Note: **15**
  examples have `[[bin]]` sections, not just the 14 with path deps (`tentacle_demo` has
  `[[bin]]` but already uses workspace deps — it still needs the `[[bin]]` removed).
- Replace `edition = "2024"` with `edition.workspace = true`
- Add `publish = false` where missing
- Add `license.workspace = true` where missing

## Workspace Cargo.toml changes

```toml
[workspace]
resolver = "2"
members = [
    "crates/*",
    "tools/*",
    "examples/shared",
    "examples/basics/*",
    "examples/geometry/*",
    "examples/techniques/*",
    "examples/materials/*",
    "examples/loading/*",
    "examples/animation/*",
    "examples/terrain/*",
    "examples/gltf/*",
    "examples/procedural/*",
]
default-members = ["crates/*", "tools/*"]
exclude = ["GeometricTools"]

[workspace.dependencies]
# add alongside existing entries:
example-shared = { path = "examples/shared" }
```

**Notes:**
- `"examples/shared"` is listed explicitly — it is not inside a group directory
- `"tools/*"` requires that `tools/` only contains Cargo packages (no stray subdirs)
- `Cargo.lock` remains valid — package names and versions do not change
- `default-members` includes `tools/*` so `cargo build` still compiles `gen_test_textures`

## Asset resolution — confirmed non-issue

`FilesystemSource::default()` roots at `.` (the process CWD). `cargo run -p <name>`
always sets CWD to the workspace root, so `assets/models/cube.obj` resolves correctly
regardless of where the source lives on disk. Moving examples into subdirectories has
zero effect on runtime asset loading.

Note: running the compiled binary directly (`./target/debug/terrain_warp`) from a
directory other than the workspace root will produce `NotFound` errors. Document this
in the top-level `examples/README.md`.

## Phased migration sequence

Phases are ordered to **decouple code changes from file moves**. Phases 1–4 are
validated at the current flat structure. If anything breaks, the cause is a code change,
not a path issue. Phase 5 is a pure rename with zero code changes.

Each phase should be a **separate commit** so that `git log --follow` correctly tracks
renames. In particular, "modify content" and "move files" must never be squashed into
the same commit — git's rename detection operates per-commit diff.

1. **Normalize all `Cargo.toml` files** — convert path deps → workspace deps, remove
   `[[bin]]` from all 15 affected examples, add `edition.workspace = true`, add
   `publish = false`. Verify: `cargo build --workspace` still passes.

2. **Create `examples/shared/` lib crate** — add `example-shared` to
   `[workspace.dependencies]` in root `Cargo.toml`, add `"examples/shared"` to members.

3. **Convert 7 `include!()` examples** — replace `include!()` with `use` imports from
   `example-shared`. Add `example-shared.workspace = true` to each example's `Cargo.toml`.
   Delete the now-dead `examples/shared_loading_example.rs`.

4. **Verify at current flat structure** — `cargo build --workspace` and
   `cargo clippy --workspace` must pass. This proves phases 1–3 are correct before
   any directory changes.

5. **`git mv` all examples into groups** — pure renames, zero code changes. Update
   workspace `Cargo.toml` members → globs, add `default-members`.

6. **Verify after moves** — `cargo build --workspace` and `cargo clippy --workspace`
   must still pass. Any failure here is a path issue, not a code issue.

7. **Write READMEs** — top-level `examples/README.md` and per-group READMEs from template.

8. **Update living documentation** — `AGENTS.md`, `docs/ARCHITECTURE.md`,
   `docs/MATERIAL.md`, `docs/ANIMATION.md`, `docs/METABALLS.md`, `assets.md`, `quest2.md`.
   Historical plan docs under `docs/plans/` are NOT updated (completed work).

9. **Final verify** — `cargo build --workspace`, `cargo clippy --workspace`,
   `cargo test --workspace`.

## Per-group README template

    # [Group Name]

    [One-sentence description of what this group demonstrates.]

    ## Examples

    | Example | Run command | Description | Key concepts |
    |---------|-------------|-------------|--------------|
    | `name/` | `cargo run -p pkg_name` | What it does | concept1, concept2 |

    ## Suggested order

    1. Start with `X` to learn the basics of …
    2. Then `Y` builds on that by adding …
    3. Finally `Z` shows the full picture with …

    ## Notes

    Run all examples from the workspace root so that `assets/` resolves correctly:

    ```bash
    # from the workspace root
    cargo run -p <package_name>
    ```

    Running the compiled binary directly from another directory will produce
    asset-not-found errors.

## Key characteristics
- **Zero breaking change to CLI workflow** — `cargo run -p terrain_warp` unchanged
- **Faster dev iteration** — bare `cargo build` compiles only library crates + tools via `default-members`
- **Self-documenting** — READMEs provide discovery, progression order, and running instructions
- **Auto-discovery** — new examples in a group are picked up by workspace globs
- **History preservation** — use `git mv` for all moves to preserve rename detection in `git log --follow`
- **Follows proven prior art** — GeometricTools, Bevy, Three.js naming conventions

## Documentation update scope

**53 references** to `examples/<name>` paths exist across 9 files:

| File | References | Action |
|------|-----------|--------|
| `AGENTS.md` | 2 (repository layout) | **Update** — rewrite layout section |
| `docs/ARCHITECTURE.md` | 7 (Mermaid diagrams + prose) | **Update** — living doc |
| `docs/MATERIAL.md` | 6 | **Update** — living doc |
| `docs/ANIMATION.md` | 4 | **Update** — living doc |
| `docs/METABALLS.md` | 8 | **Update** — living doc |
| `docs/plans/PLAN_GLTF_LOADER.md` | 4 | **Skip** — historical plan (completed) |
| `docs/plans/PLAN_GLTF_ENHANCEMENTS.md` | 1 | **Skip** — historical plan (completed) |
| `assets.md` (root) | 1 | **Update** — or remove if superseded |
| `quest2.md` (root) | 1 | **Update** — or remove if superseded |

## Open questions
1. Should `model_gallery` eventually migrate to `tools/` (it's a CLI viewer utility, not a learning demo)?
2. Should group READMEs include screenshots/GIFs of the examples?
3. Should the top-level `examples/README.md` be auto-generated from group READMEs, or hand-maintained?

## Prior art
- **GeometricTools** `GTE/Samples/` — domain directories with clean inner names (in-repo at `GeometricTools/GTE/Samples/`)
- **Bevy** `examples/` — domain directories (`3d/`, `shader/`, `animation/`, `camera/`, etc.) with per-group tables in README — https://github.com/bevyengine/bevy/tree/main/examples
- **wgpu** `examples/` — `standalone/` + `features/` split, category tables in README — https://github.com/gfx-rs/wgpu/tree/trunk/examples
- **Three.js** — uses "materials" for surface appearance (MeshStandardMaterial, MeshPhongMaterial)
- **Google Filament** — "materials" as the central concept for surface definition
- **Cargo workspace globs** — https://doc.rust-lang.org/cargo/reference/workspaces.html
