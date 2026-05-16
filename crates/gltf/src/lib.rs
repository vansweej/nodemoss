//! glTF 2.0 asset adaptation for the rig framework.
//!
//! Parses `.gltf` and `.glb` files using the `gltf` crate and adapts their
//! contents into engine asset types and scene graph hierarchy.
//! GPU upload and rendering are intentionally out of scope; callers pass the
//! produced assets and scene graph to `rig-render` or higher-level examples.
//!
//! # Supported data
//!
//! - scene hierarchy and local transforms
//! - triangle mesh primitives in the engine standard vertex layout
//! - PBR metallic-roughness materials and five texture slots
//! - PNG/JPEG/TGA-compatible image payloads decoded by the `image` crate
//! - perspective and orthographic cameras
//! - `KHR_lights_punctual` directional, point, and spot lights
//! - transform animation channels and morph target weight channels
//! - skins, skin weights, skinned primitive descriptors, and morph targets
//!
//! See `docs/GLTF.md` for the architecture, loading flow, runtime skinning
//! handoff, examples, and known limitations.
//!
//! # Loading the default scene
//!
//! ```no_run
//! use rig_gltf::load_gltf;
//! # use rig_assets::{AssetStore, ShaderHandle};
//! # use rig_scene::SceneGraph;
//! # let mut scene = SceneGraph::new();
//! # let mut store = AssetStore::new();
//! # let shader = ShaderHandle::from_raw(0);
//!
//! let loaded = load_gltf("model.glb", shader, &mut scene, &mut store)?;
//! println!("{} root nodes", loaded.root_nodes.len());
//! # Ok::<(), rig_gltf::GltfError>(())
//! ```
//!
//! # Loading a specific scene
//!
//! ```no_run
//! use rig_gltf::{SceneSelection, load_gltf_scene};
//! # use rig_assets::{AssetStore, ShaderHandle};
//! # use rig_scene::SceneGraph;
//! # let mut scene = SceneGraph::new();
//! # let mut store = AssetStore::new();
//! # let shader = ShaderHandle::from_raw(0);
//!
//! let loaded = load_gltf_scene(
//!     "multi_scene.glb",
//!     SceneSelection::Index(0),
//!     shader,
//!     &mut scene,
//!     &mut store,
//! )?;
//! # let _ = loaded;
//! # Ok::<(), rig_gltf::GltfError>(())
//! ```

mod animations;
mod buffers;
mod cameras;
mod error;
mod lights;
mod loader;
mod materials;
mod meshes;
mod nodes;
mod skins;
mod textures;

pub use error::{GltfError, Result};
pub use loader::{LoadedGltf, SceneSelection, SkinnedPrimitive, load_gltf, load_gltf_scene};
