Idea
Add a two-crate asset loading pipeline — rig-loader (format-agnostic decode + I/O abstraction) and rig-import (conversion to rig-assets types with dedup caching) — plus seven progressive examples, comprehensive documentation, and housekeeping updates.
Context
The rig framework (milestone 7, 7 crates, 11 examples) currently creates all assets procedurally via MeshFactory or inline constants. There is no way to load meshes, textures, or shaders from disk. This blocks working with artist-authored content (OBJ models, PNG/JPEG/TGA textures, external WGSL shaders) and is the natural next capability gap after dynamic meshes/metaballs.
Explored directions
1. Two-crate layered pipeline (rig-loader + rig-import) — Pure decode layer with zero rig-* deps (Layer 1+2) feeding an import layer that targets rig-assets (Layer 3). Clean separation means rig-loader is reusable/testable without the framework.
   Trade-off: More crates and types to maintain, but maximum testability and reuse.
2. Single monolithic rig-loader crate — Combine decode, I/O, and rig-assets conversion in one crate. Fewer moving parts, simpler dependency graph.
   Trade-off: Faster to build initially, but couples format decoding to rig-assets types; harder to test decoders in isolation; the crate would need rig-assets + rig-math deps even for pure decode work.
3. Bevy-style async asset server — Full async pipeline with background loading, hot-reloading, and handle-based futures (inspired by bevy_asset).
   Trade-off: Maximum capability, but massive complexity jump — premature for a research framework with synchronous startup loading.
4. glTF-first instead of OBJ-first — Skip OBJ/MTL entirely; start with glTF 2.0 via the gltf crate, which packages geometry + materials + textures in one file.
   Trade-off: More modern format with richer material model, but heavier dependency, more complex scene graph mapping, and OBJ is better for incremental learning and testing.
Chosen direction
Direction 1: Two-crate layered pipeline — chosen because it aligns with the project's established pattern of small, focused crates with clean dependency boundaries (see ARCHITECTURE.md §3). The zero-rig-dep constraint on rig-loader means decoders are unit-testable with MemorySource and synthetic data, matching the existing test culture. The Importer cache in rig-import provides path-based dedup without polluting the decode layer with framework concepts.
Key characteristics
Architecture (3 layers)
- Layer 1 — Pure decoders (rig-loader): Stateless functions bytes → decoded struct. DecodedImage, DecodedMesh, DecodedModel, DecodedMaterial, DecodedShader. Config types (TextureConfig, MeshConfig) control flip, normals, winding. No I/O, no rig-* deps.
- Layer 2 — I/O abstraction (rig-loader): AssetPath(Arc<str>) for logical paths (never absolute). AssetSource trait (read, exists, list) with FilesystemSource (root from RIG_ASSETS_DIR or ./assets/) and MemorySource (for tests). Loader struct coordinates source reads → decoder dispatch by extension.
- Layer 3 — Import + registration (rig-import): Importer converts decoded types → rig-assets types, registers into AssetStore, owns HashMap<AssetPath, CachedHandle> for path-based dedup. ShaderPolicy decides which shader to assign. ModelOutput aggregates multi-mesh OBJ results with flatten() convenience.
Decoded types (rig-loader, zero rig-* deps)
Type	Fields	Source
DecodedImage	width, height, channels, color_space (Srgb/Linear), data: Vec<u8>	image crate
DecodedMesh	positions, normals (opt), uvs (opt), indices, name	tobj
DecodedModel	meshes: Vec<DecodedMesh>, materials: Vec<DecodedMaterial>	tobj
DecodedMaterial	name, diffuse, specular, shininess, diffuse_texture (opt)	tobj MTL
DecodedShader	source: String	UTF-8 read
Import conversion rules (rig-import)
- ColorSpace::Srgb → TextureFormat::Rgba8UnormSrgb; Linear → Rgba8Unorm
- Vertex interleaving to standard_layout() (stride 32: pos f32×3 + normal f32×3 + uv f32×2)
- Default SamplerDescriptor (linear filter, clamp-to-edge) — matches existing default in crates/assets/src/lib.rs:242
- ShaderPolicy { default: ShaderHandle, textured: Option<ShaderHandle> } decides material shader based on whether diffuse texture is present
Dependency graph additions
rig-loader        (leaf — depends only on image, tobj, thiserror. Zero rig-* deps)
  ↑
rig-import        (depends on rig-loader, rig-assets, rig-math, thiserror)
  ↑
rig-app           (adds rig-import to existing deps)
  ↑
examples/         (depend on rig-app)
Error model
- LoadError (rig-loader): NotFound, Io, UnsupportedFormat, Decode, MissingPositions, IndexOverflow, UnresolvedDependency
- ImportError (rig-import): wraps LoadError + import-specific variants (conversion failures, missing handles)
7 progressive examples
#	Name	Focus	Key overlay stats
9	texture_load	PNG from disk, procedural sphere	path + dimensions
10	texture_formats	PNG + JPEG + TGA side-by-side	format + channels per quad
11	obj_load	Geometry-only OBJ (Utah Teapot), Phong	vertex/triangle count
12	obj_textured	OBJ + MTL + textures (Kenney CC0)	mesh/material/texture counts
13	multi_obj	3–5 OBJs sharing texture atlas	"textures registered: 1 despite 5 models"
14	shader_load	WGSL from disk	identical to lit_scene but runtime-loaded
15	asset_showcase	All loaders in one scene	full registry summary, cache hit/miss, startup time
17-phase implementation plan
Workspace wiring → loader types → I/O layer → decoders → loader tests → import foundation → import tests → test assets → 7 examples (phases 9–15) → documentation (12 Mermaid diagrams) → housekeeping.
Open questions
1. MTL texture path resolution — tobj returns texture paths relative to the MTL file. Should AssetPath::sibling() handle this, or should Loader normalize paths before returning DecodedMaterial?
2. Texture channel forcing — TextureConfig { force_channels } implies converting grayscale/RGB to RGBA. Should this happen in the decoder (Layer 1) or during import (Layer 3)? Doing it in Layer 1 keeps DecodedImage always RGBA, simplifying import; doing it in Layer 3 preserves original channel info for inspection.
3. max_dimension downscaling — CPU-side resize in the decoder adds the image crate's resize dependency. Is this needed for the initial implementation, or can it be deferred?
4. OBJ single_index mode — tobj's GPU_LOAD_OPTIONS enables single_index: true (deduplicates vertex/normal/uv index triples). This is critical for GPU upload. Should this be hardcoded or exposed via MeshConfig?
5. Sampler per texture vs shared default — The spec uses a single default SamplerDescriptor. Some MTL files imply different wrapping modes. Is one sampler enough for v1?
6. Test asset licensing — Kenney CC0 assets are ideal for examples. The Kenney "Mini Dungeon" pack (https://kenney.nl/assets/mini-dungeon) includes OBJ models with textures under CC0. Utah Teapot is public domain. Are these the intended sources, or should we generate all test assets programmatically?
7. rig-app integration surface — Should Importer be exposed via StartupContext, or should examples construct it manually? Adding it to the context couples rig-app to rig-import.
8. Future glTF path — The doc spec mentions "future extension points (rig-gltf, async sources)" in the Mermaid diagrams. Should rig-loader's trait design anticipate scene-graph-level imports (glTF nodes → SceneGraph), or is that a separate crate entirely?
Prior art
- Bevy bevy_asset — Full async asset server with AssetLoader trait, AssetServer, handle-based futures, and hot reloading. Much heavier than what's needed here, but the AssetPath concept is similar. https://github.com/bevyengine/bevy/tree/main/crates/bevy_asset
- tobj v4 — Lightweight OBJ/MTL loader returning flat Vec<f32> data. Supports GPU_LOAD_OPTIONS with single_index + triangulation. Key types: Model, Mesh, Material, LoadOptions. The load_obj_buf variant accepts a reader, enabling integration with AssetSource. https://docs.rs/tobj/4.0.2/tobj/
- image crate v0.25 — Already a workspace dependency (Cargo.toml:50). Handles PNG, JPEG, TGA decode. DynamicImage::to_rgba8() gives the RGBA conversion needed for GPU upload.
- Kenney game assets — CC0 3D assets in OBJ format with textures, ideal for examples. "Mini Dungeon" pack has low-poly models with diffuse textures. https://kenney.nl/assets/mini-dungeon
- Utah Teapot — Classic public-domain test model, widely available as OBJ. Good geometry-only test case (no textures, no MTL).
Recommended next steps
- spar should challenge: the two-crate split (is rig-loader truly reusable outside this framework?), the synchronous-only design (will this paint you into a corner when glTF/async arrives?), and whether ShaderPolicy is the right abstraction vs. a more general material mapping callback.
- plan should focus on: the 17-phase execution order, dependency wiring in Cargo.toml, the exact tobj::LoadOptions configuration, test asset procurement/generation, and the StartupContext integration decision.
