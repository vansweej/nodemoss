# Feature: Terrain Sub-Problems (Phase C)

**Source**: `docs/MATERIAL.md` §6 — Phase C  
**Status**: Ready to implement  
**Depends on**: Phase A (material system) and Phase B (noise + terrain) — both complete

---

## Context

Phase B delivered two terrain examples (`terrain_mc`, `terrain_heightmap`) built on the
existing marching cubes pipeline and the new `mesh_factory::create_terrain_mesh` function.
Phase C is a set of five independent research sub-problems that deepen the terrain system.
Order: domain warping → erosion → triplanar → chunking → LOD (chunking must precede LOD;
the rest are independent).

Three reusable library modules go into `rig-assets` (pure data, no GPU/scene deps).
The triplanar shader is a new constant in `rig-render`. Each sub-problem gets its own
example binary. Chunking and LOD use a pre-generate-and-toggle-visibility pattern via
the existing `SceneGraph::set_visibility(NodeId, VisibilityMode)` API.

---

## Phase 1: Domain Warping Example

Commit message: `feat(examples): add terrain_warp demo showcasing domain-warped heightmap terrain`

### Step 1: Create example Cargo.toml

Create `examples/terrain_warp/Cargo.toml`:

```toml
[package]
name = "terrain_warp"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
noise.workspace = true
anyhow.workspace = true
env_logger.workspace = true
```

### Step 2: Add to workspace

Modify root `Cargo.toml` — add `"examples/terrain_warp"` to `members` after
`"examples/terrain_heightmap"`.

### Step 3: Implement terrain_warp/src/main.rs

Create `examples/terrain_warp/src/main.rs`. Follow the structure of
`examples/terrain_heightmap/src/main.rs` exactly (Application trait, CameraRig + TrackBall,
DebugHud, PBR_SHADER material with normal map in slot 1) but replace the height function
with a domain-warped version.

**Height function construction:**
1. Two `Fbm::<Perlin>` instances:
   - terrain: `seed=42`, 6 octaves, freq 0.8, persistence 0.45
   - warp: `seed=7`, 4 octaves, freq 1.2, persistence 0.5
2. Warp offsets (decorrelated by constant offsets):
   ```rust
   let warp_x = warp_fbm.get([x as f64 * 0.1, z as f64 * 0.1]);
   let warp_z = warp_fbm.get([x as f64 * 0.1 + 5.2, z as f64 * 0.1 + 1.3]);
   ```
3. Final height:
   ```rust
   terrain_fbm.get([
       (x as f64 + warp_x * WARP_AMPLITUDE as f64) * 0.01,
       (z as f64 + warp_z * WARP_AMPLITUDE as f64) * 0.01,
   ]) as f32 * HEIGHT_SCALE
   ```
4. All f64/f32 casts must be explicit.

**Constants:**
```rust
const TERRAIN_WIDTH: f32    = 192.0;
const TERRAIN_DEPTH: f32    = 192.0;
const TERRAIN_COLS: u32     = 160;
const TERRAIN_ROWS: u32     = 160;
const HEIGHT_SCALE: f32     = 12.0;
const WARP_AMPLITUDE: f32   = 80.0;
```

**Normal map:** `Fbm::<Perlin>::new(99)`, 4 octaves, freq 8.0, persistence 0.5,
512×512 Rgba8Unorm, finite-difference encoded (same technique as `terrain_heightmap`).
Bind to material slot 1.

**Material:** diffuse `[0.52, 0.45, 0.35, 1.0]`, metallic 0.0, roughness 0.85.

**Camera:** eye `Vec3::new(0.0, 28.0, 64.0)`, pitch `-eye.y.atan2(eye.z)`, FOV 60°,
near 0.1, far 400.0.

**Lights:**
- Directional sun: color `[1.0, 0.94, 0.82]`, intensity 2.2,
  rotation `from_rotation_x(-0.8) * from_rotation_y(-0.35)`
- Point fill: position `[-40.0, 20.0, -30.0]`, color `[0.55, 0.68, 1.0]`,
  intensity 900.0, range 160.0

**HUD:** "Terrain — Domain Warping", "Warp amplitude: 80.0",
"WASD/arrows: fly  LMB: orbit  RMB: dolly"

**Module doc:** explain domain warping technique, cite Inigo Quilez
(`iquilezles.org/articles/warp`), document controls in the same table format as
`terrain_mc`.

---

## Phase 2: Hydraulic Erosion Library Module

Commit message: `feat(assets): add erosion module for CPU droplet-based hydraulic erosion`

### Step 1: Create crates/assets/src/erosion.rs

Pure CPU simulation — no `noise`, no GPU, no scene graph deps.

**Public API:**

```rust
#[derive(Clone, Debug)]
pub struct ErosionParams {
    /// Number of water droplets to simulate. Typical: 50_000–200_000.
    pub iterations: u32,
    /// Maximum steps per droplet before evaporation.
    pub max_lifetime: u32,
    /// Droplet inertia [0, 1]. 0 = instant turn, 1 = never turns. Typical: 0.05.
    pub inertia: f32,
    /// Sediment carrying capacity multiplier. Typical: 6.0.
    pub capacity: f32,
    /// Fraction of capacity deficit eroded per step. Typical: 0.4.
    pub erosion_rate: f32,
    /// Fraction of excess sediment deposited per step. Typical: 0.4.
    pub deposition_rate: f32,
    /// Minimum slope to allow erosion (prevents flat-area instability). Typical: 0.01.
    pub min_slope: f32,
    /// Gravity constant for velocity update. Typical: 6.0.
    pub gravity: f32,
    /// Water evaporation per step: volume *= (1 - evaporation). Typical: 0.02.
    pub evaporation: f32,
    /// Erosion brush radius in grid cells. Typical: 4.
    pub erosion_radius: u32,
    /// Initial water volume per droplet. Typical: 1.0.
    pub initial_volume: f32,
    /// Initial droplet speed. Typical: 1.0.
    pub initial_speed: f32,
}

impl Default for ErosionParams {
    fn default() -> Self {
        Self {
            iterations: 100_000,
            max_lifetime: 64,
            inertia: 0.05,
            capacity: 6.0,
            erosion_rate: 0.4,
            deposition_rate: 0.4,
            min_slope: 0.01,
            gravity: 6.0,
            evaporation: 0.02,
            erosion_radius: 4,
            initial_volume: 1.0,
            initial_speed: 1.0,
        }
    }
}

/// Run hydraulic erosion on a heightmap grid in-place.
///
/// `heights` is a row-major flat array of `cols * rows` f32 height values.
/// Uses a deterministic LCG for reproducibility without depending on `rand`.
pub fn erode(heights: &mut [f32], cols: u32, rows: u32, params: &ErosionParams);
```

**Implementation details:**

- **LCG** (no `rand` dep):
  ```rust
  state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
  let val = (state >> 16) as f32 / 65_536.0;  // [0, 1)
  ```
  Seed from `params.iterations`.

- **Gradient:** bilinear interpolation of the 4 surrounding grid cells.

- **Erosion brush:** precompute a circular weight kernel of radius `erosion_radius`.
  Weight for each cell = `max(0.0, radius as f32 - distance)`. Normalize to sum 1.0.
  Precompute once before the droplet loop.

- **Droplet state:** `pos: (f32, f32)`, `dir: (f32, f32)`, `speed: f32`,
  `volume: f32`, `sediment: f32`.

- **Termination:** lifetime > `max_lifetime`, volume < 0.001, or pos exits grid bounds.

- **Edge cases:** grid smaller than 3×3 → return immediately.
  `erosion_radius == 0` → single-cell erosion (no brush).

**Unit tests:**
- `erode_with_zero_iterations_does_not_modify_heights`
- `erode_on_flat_grid_produces_no_change` (no gradient → droplets don't move)
- `erode_on_slope_reduces_peak_height` (cone heightmap → peak decreases)
- `erode_preserves_total_mass_approximately` (sum before ≈ sum after, within 1%)

### Step 2: Register in lib.rs

Add `pub mod erosion;` after `pub mod tangent_utils;` in `crates/assets/src/lib.rs`.
Add re-exports: `pub use erosion::{ErosionParams, erode};`.

---

## Phase 3: Hydraulic Erosion Example

Commit message: `feat(examples): add terrain_erosion demo showcasing hydraulic erosion on heightmap terrain`

### Step 1: Create example Cargo.toml

Create `examples/terrain_erosion/Cargo.toml`:

```toml
[package]
name = "terrain_erosion"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
noise.workspace = true
anyhow.workspace = true
env_logger.workspace = true
```

### Step 2: Add to workspace

Add `"examples/terrain_erosion"` to root `Cargo.toml` members after `"examples/terrain_warp"`.

### Step 3: Implement terrain_erosion/src/main.rs

Create `examples/terrain_erosion/src/main.rs`.

**Pipeline:**
1. Generate a flat `Vec<f32>` height array of `(COLS+1) * (ROWS+1)` entries by evaluating
   domain-warped fBm at each grid vertex. For vertex at `(col, row)`:
   ```rust
   let x = -TERRAIN_WIDTH / 2.0 + col as f32 * cell_width;
   let z = -TERRAIN_DEPTH / 2.0 + row as f32 * cell_depth;
   heights[row * (COLS+1) + col] = warped_height(x, z);
   ```
2. Call `rig_assets::erode(&mut heights, COLS+1, ROWS+1, &ErosionParams { iterations: 100_000, ..Default::default() })`.
3. Build terrain mesh via `mesh_factory::create_terrain_mesh(WIDTH, DEPTH, COLS, ROWS, &height_fn)`
   where `height_fn` bilinearly interpolates the eroded `heights` array from world `(x, z)`.
4. Generate 512×512 procedural normal map from the eroded height closure (same
   finite-difference technique as `terrain_heightmap`). Bind to material slot 1.

**Constants:**
```rust
const TERRAIN_WIDTH: f32  = 192.0;
const TERRAIN_DEPTH: f32  = 192.0;
const TERRAIN_COLS: u32   = 192;
const TERRAIN_ROWS: u32   = 192;
const HEIGHT_SCALE: f32   = 14.0;
```

**Material:** PBR_SHADER, diffuse `[0.50, 0.44, 0.36, 1.0]`, metallic 0.0, roughness 0.9.

**Camera:** eye `Vec3::new(0.0, 32.0, 72.0)`, pitch `-eye.y.atan2(eye.z)`, FOV 60°,
near 0.1, far 500.0.

**Lights:** same two-light setup as terrain_warp (directional sun + point fill).

**HUD:** "Terrain — Hydraulic Erosion", "100,000 droplets, radius 4",
"WASD/arrows: fly  LMB: orbit  RMB: dolly"

**Module doc:** explain that erosion transforms noisy bumps into geologically plausible
terrain — valleys follow drainage paths, ridgelines emerge, flat deposited areas appear
at slope bases. Cite Sebastian Lague and Benes & Forsbach (2002).

---

## Phase 4: Triplanar PBR Shader and MaterialParams Extension

Commit message: `feat(render): add TRIPLANAR_PBR_SHADER with world-space triplanar sampling and custom_flags support`

### Step 1: Extend MaterialParams in crates/assets/src/lib.rs

Add two new fields to `MaterialParams`:

```rust
/// Additional shader-defined bit flags OR'd into the GPU MaterialUniforms.flags
/// field at draw time. Default: 0.
///
/// Example: set bit 5 (`32u32`) to enable triplanar sampling in TRIPLANAR_PBR_SHADER.
pub custom_flags: u32,

/// World-space texture repeat scale for triplanar sampling.
/// Passed to the GPU as MaterialUniforms.triplanar_scale.
/// Only meaningful when custom_flags includes bit 5 (USE_TRIPLANAR). Default: 4.0.
pub triplanar_scale: f32,
```

Ensure `Default` impl sets `custom_flags: 0`, `triplanar_scale: 4.0`.

### Step 2: Update MaterialUniforms in crates/render/src/helpers.rs

Replace `pub _pad: u32` with `pub triplanar_scale: f32` in the `MaterialUniforms` struct.
Update the doc comment: "World-space texture repeat scale for triplanar shaders; 0.0 when
unused. Also replaces the former `_pad` field — binary layout is unchanged (4 bytes)."

Also update the `_pad: u32` field in the WGSL `MaterialUniforms` struct declaration
**inside `PBR_SHADER`** to `triplanar_scale: f32` for consistency (binary layout is
identical; the PBR shader never reads this field).

### Step 3: Update renderer flag computation in crates/render/src/renderer.rs

Find where `MaterialUniforms` is constructed from a `MaterialAsset`. After computing
`flags` from slot presence (existing logic), OR in `material.parameters.custom_flags`.
Set `triplanar_scale` from `material.parameters.triplanar_scale`.

### Step 4: Add TRIPLANAR_PBR_SHADER constant to crates/render/src/helpers.rs

Add `pub const TRIPLANAR_PBR_SHADER: &str = r#"..."#;` immediately after the closing
`"#;` of `PBR_SHADER`.

The shader is structurally identical to `PBR_SHADER` with these additions:

**New constant:**
```wgsl
const USE_TRIPLANAR: u32 = 32u;  // bit 5
```

**`MaterialUniforms` struct:** `_pad: u32` → `triplanar_scale: f32` (same as Step 2).

**New helper function** (insert before `surface_normal`):
```wgsl
fn triplanar_sample(
    tex: texture_2d<f32>,
    samp: sampler,
    world_pos: vec3<f32>,
    N: vec3<f32>,
    scale: f32,
) -> vec4<f32> {
    let p = world_pos / scale;
    let blend = pow(abs(N), vec3<f32>(2.0));
    let b = blend / (blend.x + blend.y + blend.z);
    return textureSample(tex, samp, p.yz) * b.x
         + textureSample(tex, samp, p.xz) * b.y
         + textureSample(tex, samp, p.xy) * b.z;
}
```

**Modified `surface_normal`:** when `material_flag(1u)` (has normal map) AND
`material_flag(5u)` (triplanar), use `triplanar_sample(t_normal, s_normal, in.world_position, N, material.triplanar_scale)` instead of `textureSample(..., in.uv)`. Decode, apply TBN as normal.

**Modified `fs_main`:** for each of the five texture slots, when both `material_flag(slot)`
and `material_flag(5u)` are set, use `triplanar_sample(...)` with `in.world_position` and
the resolved `N` instead of `textureSample(..., in.uv)`. When only `material_flag(slot)` is
set (no triplanar), use UV sampling as before. All BRDF, lighting, environment, and
tone-mapping code is identical to `PBR_SHADER`.

### Step 5: Export TRIPLANAR_PBR_SHADER from crates/render/src/lib.rs

Add `TRIPLANAR_PBR_SHADER` alongside `PBR_SHADER` in the existing pub use statement.

### Step 6: Fix existing construction sites

Search the workspace for `MaterialParams {`. Any site that does NOT use
`..Default::default()` must have `custom_flags: 0, triplanar_scale: 4.0` added.
(All current examples already use `..Default::default()` — verify and fix any that don't.)

---

## Phase 5: Triplanar Texturing Example

Commit message: `feat(examples): add terrain_triplanar demo with world-space textured marching cubes terrain`

### Step 1: Create example Cargo.toml

Create `examples/terrain_triplanar/Cargo.toml`:

```toml
[package]
name = "terrain_triplanar"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
noise.workspace = true
anyhow.workspace = true
env_logger.workspace = true
```

### Step 2: Add to workspace

Add `"examples/terrain_triplanar"` to root `Cargo.toml` members after
`"examples/terrain_erosion"`.

### Step 3: Implement terrain_triplanar/src/main.rs

Follow `examples/terrain_mc/src/main.rs` exactly (DynamicMeshData, DynamicMeshId,
register on init, upload on first render, CameraRig + TrackBall, DebugHud, same grid
48×24×48, same field function seed 42).

**Differences from terrain_mc:**

1. **Shader:** `rig_app::rig_render::TRIPLANAR_PBR_SHADER`.

2. **Procedural rock texture** (256×256 Rgba8Unorm, generated at startup):
   - Noise: `Fbm::<Perlin>::new(123)`, 4 octaves, freq 4.0, persistence 0.6
   - For each texel `(u, v)`: sample at `[u as f64 * 4.0, v as f64 * 4.0]`
   - Map `[-1, 1]` → `[0, 1]`: `t = val * 0.5 + 0.5`
   - Lerp brown `[89u8, 71, 51]` → grey `[140u8, 135, 128]` by `t`
   - Alpha = 255

3. **Material:**
   ```rust
   MaterialAsset {
       shader,
       parameters: MaterialParams {
           diffuse: [1.0, 1.0, 1.0, 1.0],
           metallic: 0.0,
           roughness: 0.85,
           custom_flags: 32,      // bit 5 = USE_TRIPLANAR
           triplanar_scale: 4.0,
           ..Default::default()
       },
       textures: vec![Some((rock_tex, sampler)), None, None, None, None],
   }
   ```

4. **Sampler:** `AddressMode::Repeat` U+V, `FilterMode::Linear` mag+min.

**HUD:** "Terrain — Triplanar Texturing", "Marching cubes + world-space UV projection",
"WASD/arrows: fly  LMB: orbit  RMB: dolly"

**Module doc:** explain that marching cubes meshes have no meaningful UVs, so triplanar
projection samples the texture three times using world-space coordinates and blends by
surface normal alignment. Cite MATERIAL.md §6.5.

---

## Phase 6: Chunk Manager Library Module

Commit message: `feat(assets): add chunk_manager module for camera-driven infinite terrain chunking`

### Step 1: Create crates/assets/src/chunk_manager.rs

Pure data structure — no GPU, no scene graph, no noise deps. Uses `std::collections::HashSet`.

**Public API:**

```rust
/// Integer chunk coordinate in the XZ plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkCoord {
    pub x: i32,
    pub z: i32,
}

/// Result of a chunk manager update.
#[derive(Clone, Debug, Default)]
pub struct ChunkUpdate {
    /// Chunks that entered the active radius — make visible.
    pub to_create: Vec<ChunkCoord>,
    /// Chunks that exited the unload radius — hide.
    pub to_destroy: Vec<ChunkCoord>,
}

pub struct ChunkManager {
    pub chunk_size: f32,
    pub load_radius: u32,
    /// Must be >= load_radius. Creates a hysteresis band.
    pub unload_radius: u32,
    active_chunks: HashSet<ChunkCoord>,
    last_camera_chunk: Option<ChunkCoord>,
}

impl ChunkManager {
    /// Panics if unload_radius < load_radius or chunk_size <= 0.0.
    pub fn new(chunk_size: f32, load_radius: u32, unload_radius: u32) -> Self;

    /// floor(world / chunk_size) per axis.
    pub fn world_to_chunk(&self, world_x: f32, world_z: f32) -> ChunkCoord;

    /// Center of chunk in world space (Y = 0).
    pub fn chunk_center(&self, coord: ChunkCoord) -> (f32, f32);

    /// Update active set. Returns empty ChunkUpdate if camera hasn't moved chunks.
    pub fn update(&mut self, camera_x: f32, camera_z: f32) -> ChunkUpdate;

    pub fn active_chunks(&self) -> impl Iterator<Item = &ChunkCoord>;
    pub fn active_count(&self) -> usize;

    /// Force initial population regardless of last position.
    pub fn initialize(&mut self, camera_x: f32, camera_z: f32) -> ChunkUpdate;
}
```

**`update()` logic:**
1. `current = world_to_chunk(camera_x, camera_z)`
2. If `last_camera_chunk == Some(current)` → return `ChunkUpdate::default()`
3. `last_camera_chunk = Some(current)`
4. `desired`: all coords with Chebyshev distance ≤ `load_radius` from `current`
5. `to_create`: desired NOT in `active_chunks`
6. `to_destroy`: active coords with Chebyshev distance > `unload_radius` from `current`
7. Apply both sets to `active_chunks`
8. Return `ChunkUpdate { to_create, to_destroy }`

**`chunk_center`:**
```rust
(coord.x as f32 * self.chunk_size + self.chunk_size * 0.5,
 coord.z as f32 * self.chunk_size + self.chunk_size * 0.5)
```

**Unit tests:**
- `world_to_chunk_maps_correctly` — chunk_size=64: (31.0,31.0)→(0,0), (64.0,0.0)→(1,0), (-1.0,0.0)→(-1,0)
- `initialize_populates_correct_radius` — radius 2 → 5×5 = 25 chunks in to_create
- `update_no_movement_returns_empty`
- `update_creates_and_destroys_on_movement` — move 2 chunks right, verify leading edge created, trailing edge beyond unload_radius destroyed
- `hysteresis_prevents_thrashing` — unload_radius > load_radius, boundary movement doesn't toggle
- `chunk_center_round_trip` — `world_to_chunk(chunk_center(coord))` == `coord`

### Step 2: Register in lib.rs

Add `pub mod chunk_manager;` after `pub mod erosion;`.
Add re-exports: `pub use chunk_manager::{ChunkCoord, ChunkManager, ChunkUpdate};`.

---

## Phase 7: Chunked Terrain Example

Commit message: `feat(examples): add terrain_chunks demo with camera-driven infinite terrain loading`

### Step 1: Create example Cargo.toml

Create `examples/terrain_chunks/Cargo.toml`:

```toml
[package]
name = "terrain_chunks"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
noise.workspace = true
anyhow.workspace = true
env_logger.workspace = true
```

### Step 2: Add to workspace

Add `"examples/terrain_chunks"` to root `Cargo.toml` members after
`"examples/terrain_triplanar"`.

### Step 3: Implement terrain_chunks/src/main.rs

**Architecture note:** `UpdateContext.assets` is `&AssetStore` (immutable), so runtime
mesh creation is not possible in `update()`. All chunks are pre-generated at startup in
`init()` where `StartupContext` provides `&mut AssetStore`. Visibility is toggled in
`update()` via `SceneGraph::set_visibility(NodeId, VisibilityMode)`.

**Init:**
- `ChunkManager::new(64.0, 4, 6)` — load radius 4, unload radius 6
- Pre-generate all chunks within `unload_radius` (13×13 = 169 chunks):
  - For each coord: compute `(center_x, center_z)` via `chunk_manager.chunk_center(coord)`
  - `height_fn = |x, z| warped_height(center_x + x, center_z + z)` — absolute world coords
  - `mesh = mesh_factory::create_terrain_mesh(64.0, 64.0, 32, 32, &height_fn)`
  - `mesh_handle = ctx.assets.add_mesh(mesh)`
  - `node = ctx.scene.create_node(format!("chunk_{},{}", coord.x, coord.z))`
  - `ctx.scene.set_renderable(node, Renderable { mesh: MeshSource::Static(mesh_handle), material })?`
  - `ctx.scene.set_local_transform(node, Transform { translation: Vec3::new(center_x, 0.0, center_z), ..Default::default() })?`
  - `ctx.scene.set_visibility(node, VisibilityMode::Hidden)?`
  - Store in `HashMap<ChunkCoord, NodeId>`
- Call `chunk_manager.initialize(0.0, 0.0)` and set `VisibilityMode::Inherit` for
  returned `to_create` coords.

**Update:**
- Get camera position: `ctx.scene.local_transform(self.camera_node)?.translation`
- `let update = self.chunk_manager.update(cam_pos.x, cam_pos.z)`
- For each `to_create`: `ctx.scene.set_visibility(node, VisibilityMode::Inherit)?`
- For each `to_destroy`: `ctx.scene.set_visibility(node, VisibilityMode::Hidden)?`

**Height function:** domain-warped fBm — `Fbm::<Perlin>::new(42)` terrain +
`Fbm::<Perlin>::new(7)` warp. Takes absolute world `(x, z)` for seamless boundaries.

**Material:** one shared `MaterialHandle` — PBR_SHADER, diffuse `[0.50, 0.44, 0.36, 1.0]`,
metallic 0.0, roughness 0.85, no textures.

**Camera:** eye `Vec3::new(0.0, 40.0, 0.0)`, pitch -0.4 rad, CameraRig
`translation_speed = 20.0`, FOV 60°, near 0.1, far 600.0.

**Lights:** directional sun (color `[1.0, 0.94, 0.82]`, intensity 2.2).

**HUD:** "Terrain — Chunked (Infinite)", dynamic "Active chunks: {N}" updated each frame,
"WASD: fly fast to see chunk loading"

**Module doc:** explain pre-generate-and-toggle approach; note that a production system
would use async loading or DynamicMesh pooling to avoid the startup cost and to support
truly infinite terrain. Document controls.

---

## Phase 8: LOD Selection Module

Commit message: `feat(assets): add lod module with distance-based level-of-detail selection`

### Step 1: Create crates/assets/src/lod.rs

Pure data utility — no deps beyond `f32` arithmetic.

**Public API:**

```rust
/// A single LOD level definition.
#[derive(Clone, Copy, Debug)]
pub struct LodLevel {
    /// Maximum distance (world units) at which this LOD is used.
    pub max_distance: f32,
    /// Grid resolution (cols and rows) for terrain mesh generation.
    pub resolution: u32,
}

/// Select terrain resolution based on camera distance.
///
/// Levels must be ordered nearest→farthest. Returns the first level's resolution
/// where `distance <= level.max_distance`. Falls back to the last level's resolution
/// if distance exceeds all levels. Returns 16 if `levels` is empty.
pub fn select_lod(distance: f32, levels: &[LodLevel]) -> u32;

/// Returns `Some(new_resolution)` if the chunk should be regenerated at a different
/// LOD, `None` if `current_resolution` already matches the desired level.
pub fn needs_lod_update(
    current_resolution: u32,
    camera_distance: f32,
    levels: &[LodLevel],
) -> Option<u32>;
```

**Unit tests:**
- `select_lod_returns_highest_for_near_distance` — dist 50.0, levels [128→64, 256→32, 512→16] → 64
- `select_lod_returns_lowest_for_far_distance` — dist 999.0 → 16
- `select_lod_at_exact_boundary` — dist 128.0 → 64 (≤ check)
- `select_lod_empty_levels_returns_16`
- `needs_lod_update_returns_none_when_same` — current=64, dist=50.0 → None
- `needs_lod_update_returns_some_when_different` — current=64, dist=200.0 → Some(32)

### Step 2: Register in lib.rs

Add `pub mod lod;` after `pub mod chunk_manager;`.
Add re-exports: `pub use lod::{LodLevel, select_lod, needs_lod_update};`.

---

## Phase 9: LOD Terrain Example

Commit message: `feat(examples): add terrain_lod demo with distance-based level-of-detail terrain chunks`

### Step 1: Create example Cargo.toml

Create `examples/terrain_lod/Cargo.toml`:

```toml
[package]
name = "terrain_lod"
version = "0.1.0"
edition.workspace = true
publish = false

[dependencies]
rig-app.workspace = true
noise.workspace = true
anyhow.workspace = true
env_logger.workspace = true
```

### Step 2: Add to workspace

Add `"examples/terrain_lod"` to root `Cargo.toml` members after `"examples/terrain_chunks"`.

### Step 3: Implement terrain_lod/src/main.rs

Extends chunked terrain with 3 LOD variants per chunk, swapped by camera distance.

**LOD levels:**
```rust
const LOD_LEVELS: [LodLevel; 3] = [
    LodLevel { max_distance: 128.0, resolution: 64 },
    LodLevel { max_distance: 256.0, resolution: 32 },
    LodLevel { max_distance: 512.0, resolution: 16 },
];
```

**Architecture:**
- `ChunkManager::new(64.0, 5, 7)` — 15×15 = 225 chunks within unload_radius
- At startup: for each chunk, generate 3 meshes (one per LOD level), create 3 scene nodes,
  all initially `VisibilityMode::Hidden`.
- Store: `HashMap<ChunkCoord, [NodeId; 3]>` and `HashMap<ChunkCoord, usize>` (current LOD index).
- On init: for chunks within load_radius, compute distance from camera to chunk center,
  call `select_lod`, show matching node, hide others.
- In `update()`:
  1. Get camera XZ.
  2. Call `chunk_manager.update(cam_x, cam_z)`.
  3. `to_create`: compute distance, select LOD, show matching node.
  4. `to_destroy`: hide all 3 nodes.
  5. For all active chunks: recompute LOD. If changed, swap visibility (hide old, show new),
     update `current_lod` map.

**Materials:** 3 separate handles with LOD-color coding for visual debugging:
- LOD 0 (64×64, near): diffuse `[0.45, 0.55, 0.40, 1.0]` (greenish)
- LOD 1 (32×32, mid): diffuse `[0.55, 0.48, 0.38, 1.0]` (brownish)
- LOD 2 (16×16, far): diffuse `[0.60, 0.55, 0.50, 1.0]` (greyish)

All use PBR_SHADER, metallic 0.0, roughness 0.85, no textures.

**Camera:** eye `Vec3::new(0.0, 50.0, 0.0)`, pitch -0.4 rad, `translation_speed = 25.0`,
FOV 60°, near 0.1, far 800.0.

**Lights:** directional sun.

**HUD:** "Terrain — LOD (Level of Detail)", dynamic "Active chunks: {N}",
dynamic "LOD 0/1/2 visible: {n0}/{n1}/{n2}", "WASD: fly — watch LOD transitions"

**Module doc:** explain LOD concept. Note that geomorphing (vertex-shader blending between
LOD levels for pop-free transitions) is a natural next step. Explain the color coding.
Note startup cost: 225 × 3 = 675 meshes; reduce `unload_radius` to 5 if too slow.

---

## Phase 10: Documentation Update

Commit message: `docs: update MATERIAL.md and AGENTS.md for Phase C completion`

### Step 1: Update MATERIAL.md status line (line 4)

Change:
```
**Status**: Phase A complete, Phase B implemented — 2026-05-16
```
To:
```
**Status**: Phase A–C complete, Phase D pending
```

### Step 2: Update AGENTS.md milestones

After milestone 10 (CPU skinning ✓), add:

```markdown
11. **Material system + normal maps** — `rig-assets::tangent_utils` (mikktspace),
    48-byte vertex layout, 5-slot PBR bind group, `normal_map_demo` example ✓
12. **Procedural terrain** — `noise` crate in examples, marching cubes terrain,
    heightmap terrain, procedural normal maps, two terrain examples ✓
13. **Terrain sub-problems** — domain warping, hydraulic erosion,
    triplanar texturing, chunked infinite terrain, distance-based LOD,
    five progressive terrain examples ✓
```

### Step 3: Update AGENTS.md repository layout

Add after `terrain_heightmap/` in the examples list:

```
    terrain_warp/               # milestone 13 — domain-warped heightmap
    terrain_erosion/            # milestone 13 — hydraulic erosion
    terrain_triplanar/          # milestone 13 — triplanar UV-free MC texturing
    terrain_chunks/             # milestone 13 — camera-driven chunked terrain
    terrain_lod/                # milestone 13 — distance-based level of detail
```

### Step 4: Add rayon future-work note

In `docs/MATERIAL.md` section `### 8.2 Open`, add a row to the table:

```markdown
| Parallelizing chunk/LOD mesh generation with `rayon` | Add `rayon` as a workspace dep when chunk count or LOD regen becomes a startup bottleneck; not needed at current scale |
```

In `AGENTS.md` Technology choices table, add:

```markdown
| Parallelism      | **rayon** (planned) | Thread-pool for terrain chunk generation; not yet a dep — add when chunk count becomes a bottleneck. |
```

---

## Resolved decisions

| Decision | Resolution |
|----------|------------|
| Scope | All five sub-problems |
| Demo structure | One example binary per sub-problem |
| Library placement | `erosion`, `chunk_manager`, `lod` → `rig-assets` |
| Triplanar shader | New `TRIPLANAR_PBR_SHADER` constant in `rig-render` |
| Visibility API | `SceneGraph::set_visibility(NodeId, VisibilityMode)` — confirmed present |
| Chunking runtime constraint | `UpdateContext.assets` is `&AssetStore`; pre-generate all chunks at startup, toggle `VisibilityMode` in update |
| `MaterialUniforms._pad` | Renamed to `triplanar_scale: f32` — same 4-byte layout, PBR_SHADER WGSL updated for consistency |
| Rayon | Noted as planned future dep in documentation; not added now |

## Known risks

| Risk | Mitigation |
|------|------------|
| LOD startup time (675 meshes) | Reduce `unload_radius` to 5 if too slow; `rayon` planned for future |
| `MaterialParams` struct expansion | All existing sites use `..Default::default()` — verify in Phase 4 Step 6 |
| `MaterialUniforms._pad → triplanar_scale` | PBR_SHADER never reads this field; Zeroable gives 0.0f32 which is a safe default |
