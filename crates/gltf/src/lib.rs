//! glTF 2.0 asset adaptation for the rig framework.
//!
//! Parses `.gltf` and `.glb` files using the `gltf` crate and adapts their
//! contents into engine asset types and scene graph hierarchy.
//!
//! # Example
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
pub use loader::{LoadedGltf, load_gltf};
