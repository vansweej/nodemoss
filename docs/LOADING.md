# Asset Loading Pipeline

**Crates**: `rig-loader`, `rig-import`, `rig-assets`, `rig-app`  
**Purpose**: Decode files from disk or memory, adapt them into framework assets, and register them in `AssetStore`.

---

## 1. Three-layer architecture

```mermaid
flowchart LR
    Disk["assets/ on disk"] --> Source["AssetSource"]
    Source --> Loader["rig-loader<br/>format-faithful decode"]
    Loader --> Importer["rig-import<br/>GPU/framework adaptation"]
    Importer --> Store["AssetStore<br/>typed handles"]
    Store --> Render["rig-render<br/>GPU cache"]
```

`rig-loader` answers “what does this file contain?” `rig-import` answers “how does this map to the renderer?”

## 2. Crate dependency graph

```mermaid
graph TD
    examples --> app["rig-app"]
    app --> import["rig-import"]
    import --> loader["rig-loader"]
    import --> assets["rig-assets"]
    import --> math["rig-math"]
    loader --> image["image"]
    loader --> tobj["tobj"]
    app --> render["rig-render"]
```

`rig-loader` has no `rig-*` dependencies. Examples access the importer via `rig_app::rig_import::*`.

## 3. AssetSource implementations

```mermaid
classDiagram
    class AssetSource {
      <<trait>>
      read(path) Result~Vec~u8~~
    }
    class FilesystemSource
    class MemorySource
    AssetSource <|.. FilesystemSource
    AssetSource <|.. MemorySource
```

`FilesystemSource` reads relative to the current working directory by default. `MemorySource` supports integration tests without temporary files.

## 4. Loader call flow

```mermaid
sequenceDiagram
    participant Caller
    participant Loader
    participant Source as AssetSource
    participant Decoder
    Caller->>Loader: read_texture/read_mesh/read_shader(path)
    Loader->>Source: read(path)
    Source-->>Loader: bytes
    Loader->>Decoder: decode bytes
    Decoder-->>Loader: DecodedImage/DecodedModel/DecodedShader
    Loader-->>Caller: decoded data
```

OBJ loading reads referenced MTL files as siblings of the OBJ path.

## 5. Importer call flow

```mermaid
sequenceDiagram
    participant Caller
    participant Importer
    participant Cache
    participant Loader
    participant Store as AssetStore
    Caller->>Importer: import_texture/import_shader/import_mesh
    Importer->>Cache: lookup path
    alt cache hit
        Cache-->>Importer: handle
    else cache miss
        Importer->>Loader: read decoded asset
        Loader-->>Importer: decoded data
        Importer->>Importer: adapt data
        Importer->>Store: add texture/shader/sampler
        Importer->>Cache: store handle
    end
    Importer-->>Caller: handle or LoadedModel
```

`import_mesh()` returns `LoadedModel`; callers register meshes and materials individually.

## 6. Dedup cache lifecycle

```mermaid
flowchart TD
    Start["import path"] --> Hit{"cache contains path?"}
    Hit -- yes --> Return["return cached handle"]
    Hit -- no --> Decode["load + decode"]
    Decode --> Register["register asset"]
    Register --> Store["cache path -> handle"]
    Store --> Return
```

The importer is a local startup object; its cache dies after initialization.

## 7. Shader pipeline

```mermaid
flowchart LR
    Embedded["embedded shader const"] --> Add["AssetStore::add_shader"]
    Runtime["Importer::import_shader(path)"] --> Add
    Manual["ShaderAsset { source }"] --> Add
    Add --> Handle["ShaderHandle"]
    Handle --> Material["MaterialAsset.shader"]
```

Runtime-loaded WGSL uses the same renderer path as embedded shader strings after registration.

## 8. import_mesh sequence

```mermaid
sequenceDiagram
    participant Importer
    participant Loader
    participant OBJ
    participant MTL
    participant Store
    Importer->>Loader: read_mesh(obj path)
    Loader->>OBJ: decode OBJ
    OBJ->>MTL: request material files
    MTL-->>OBJ: material bytes
    Loader-->>Importer: DecodedModel
    Importer->>Importer: resolve diffuse texture siblings
    Importer->>Store: register referenced textures
    Importer->>Importer: interleave vertices + pack indices + bounds
    Importer-->>Caller: LoadedModel
```

## 9. GPU adaptation decisions

```mermaid
flowchart TD
    Image["DecodedImage<br/>RGBA8 + channels + color_space"] --> Space{"color_space"}
    Space -- Srgb --> SrgbFmt["TextureFormat::Rgba8UnormSrgb"]
    Space -- Linear --> LinearFmt["TextureFormat::Rgba8Unorm"]
    Image --> Alpha{"channels < 4?"}
    Alpha -- yes --> Opaque["no real source alpha"]
    Alpha -- no --> AlphaTex["alpha came from source"]
```

The current image decoder defaults to sRGB, which matches diffuse texture loading.

## 10. Vertex interleaving pipeline

```mermaid
flowchart LR
    P["positions f32x3"] --> I["interleave"]
    N["normals f32x3"] --> I
    UV["uvs f32x2"] --> I
    I --> Bytes["32-byte stride vertex bytes"]
```

```text
byte 0..12   position: f32 x 3
byte 12..24  normal:   f32 x 3
byte 24..32  uv:       f32 x 2
```

## 11. Example progression

```mermaid
flowchart LR
    A[obj_load] --> B[obj_textured]
    B --> C[multi_obj]
    C --> D[texture_load]
    D --> E[texture_formats]
    E --> F[shader_load]
    F --> G[asset_showcase]
```

Run `cargo run -p gen_test_textures` once before texture examples.

## 12. Error handling

```mermaid
flowchart TD
    LE["LoadError"] --> NF["NotFound"]
    LE --> IO["Io"]
    LE --> UF["UnsupportedFormat"]
    LE --> DE["Decode"]
    IE["ImportError"] --> Load["Load(LoadError)"]
    IE --> Missing["MissingPositions"]
    IE --> Overflow["IndexOverflow"]
    IE --> Dep["UnresolvedDependency"]
```

Loader errors describe source/format failures. Import errors describe validation and adaptation failures.

## 13. Future extension points

```mermaid
flowchart LR
    Source["AssetSource"] --> Async["async/streaming sources"]
    Loader["rig-loader"] --> Gltf["future rig-gltf decoder"]
    Import["rig-import"] --> SceneBuild["future scene construction crate"]
```

glTF scene construction remains out of scope for this milestone.
