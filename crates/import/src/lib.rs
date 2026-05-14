//! Import decoded assets into the rig framework's asset model.
//!
//! `rig-import` adapts format-faithful data from `rig-loader` into GPU-ready
//! `rig-assets` values. It registers textures, samplers, and shaders directly
//! in an [`AssetStore`](rig_assets::AssetStore), while mesh imports return a
//! [`LoadedModel`] so callers can decide which meshes/materials to keep.
//!
//! # Example
//!
//! ```no_run
//! use rig_import::{AssetPath, FilesystemSource, Importer, MeshConfig};
//! # use rig_assets::{AssetStore, ShaderHandle};
//! # let mut assets = AssetStore::new();
//! # let shader = ShaderHandle::from_raw(0);
//!
//! let mut importer = Importer::new(FilesystemSource::default());
//! let model = importer.import_mesh(
//!     &AssetPath::new("assets/models/cube.obj"),
//!     &MeshConfig::default(),
//!     shader,
//!     &mut assets,
//! )?;
//! println!("{} meshes", model.meshes.len());
//! # Ok::<(), rig_import::ImportError>(())
//! ```

mod config;
mod error;
mod importer;
mod output;

pub use config::{MeshConfig, TextureConfig};
pub use error::ImportError;
pub use importer::Importer;
pub use output::{ImportedMesh, LoadedModel};

pub use rig_loader::{AssetPath, AssetSource, FilesystemSource, LoadError, MemorySource};
