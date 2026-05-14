//! Format-faithful asset decoding for the rig framework.
//!
//! `rig-loader` answers one question: "what does this file contain?" It owns
//! path/source abstraction and decoders for images, OBJ meshes, and WGSL shader
//! source, but it deliberately has no dependency on any `rig-*` framework crate.
//!
//! # Example
//!
//! ```no_run
//! use rig_loader::{AssetPath, FilesystemSource, Loader};
//!
//! let loader = Loader::new(FilesystemSource::default());
//! let image = loader.read_texture(&AssetPath::new("assets/textures/checker.png"))?;
//! println!("{}x{}", image.width, image.height);
//! # Ok::<(), rig_loader::LoadError>(())
//! ```

pub mod decode;
mod error;
mod loader;
mod path;
mod types;

pub use error::LoadError;
pub use loader::Loader;
pub use path::{AssetPath, AssetSource, FilesystemSource, MemorySource};
pub use types::{
    ColorSpace, DecodedImage, DecodedMaterial, DecodedMesh, DecodedModel, DecodedShader,
};
