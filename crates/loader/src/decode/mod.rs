//! Format decoders used by [`crate::Loader`].

mod mesh;
mod shader;
mod texture;

pub use mesh::decode_obj;
pub use shader::decode_shader;
pub use texture::decode_texture;
