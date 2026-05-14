//! High-level loader entry points.

use std::path::Path;

use crate::decode::{decode_obj, decode_ply, decode_shader, decode_texture};
use crate::{AssetPath, AssetSource, DecodedImage, DecodedModel, DecodedShader, LoadError};

/// Reads bytes from an [`AssetSource`] and dispatches them to format decoders.
pub struct Loader {
    source: Box<dyn AssetSource>,
}

impl Loader {
    /// Create a loader that reads from `source`.
    pub fn new(source: impl AssetSource + 'static) -> Self {
        Self {
            source: Box::new(source),
        }
    }

    /// Read and decode a PNG, JPEG, or TGA texture.
    pub fn read_texture(&self, path: &AssetPath) -> Result<DecodedImage, LoadError> {
        ensure_extension(path, &["png", "jpg", "jpeg", "tga"])?;
        let bytes = self.source.read(path)?;
        decode_texture(&bytes, path.extension().unwrap_or_default())
    }

    /// Read and decode an OBJ or PLY mesh.
    pub fn read_mesh(&self, path: &AssetPath) -> Result<DecodedModel, LoadError> {
        ensure_extension(path, &["obj", "ply"])?;
        let bytes = self.source.read(path)?;
        match path
            .extension()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "ply" => decode_ply(&bytes),
            _ => decode_obj(&bytes, |mtl_path| self.read_mtl(path, mtl_path)),
        }
    }

    /// Read and decode a WGSL shader as UTF-8 text.
    pub fn read_shader(&self, path: &AssetPath) -> Result<DecodedShader, LoadError> {
        ensure_extension(path, &["wgsl"])?;
        let bytes = self.source.read(path)?;
        decode_shader(&bytes)
    }

    fn read_mtl(&self, model_path: &AssetPath, mtl_path: &Path) -> Result<Vec<u8>, LoadError> {
        let mtl_name = mtl_path.to_string_lossy();
        self.source.read(&model_path.sibling(mtl_name.as_ref()))
    }
}

fn ensure_extension(path: &AssetPath, supported: &[&str]) -> Result<(), LoadError> {
    let Some(extension) = path.extension() else {
        return Err(LoadError::UnsupportedFormat(String::new()));
    };
    if supported
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    {
        Ok(())
    } else {
        Err(LoadError::UnsupportedFormat(extension.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, RgbaImage};

    use super::*;
    use crate::MemorySource;

    fn png_bytes() -> Vec<u8> {
        let image = RgbaImage::from_raw(1, 1, vec![1, 2, 3, 4]).unwrap();
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut cursor, ImageFormat::Png)
            .unwrap();
        cursor.into_inner()
    }

    #[test]
    fn read_texture_dispatches_by_extension() {
        let loader = Loader::new(MemorySource::new().with("checker.png", png_bytes()));

        let image = loader.read_texture(&AssetPath::new("checker.png")).unwrap();

        assert_eq!(image.data, vec![1, 2, 3, 4]);
    }

    #[test]
    fn read_shader_dispatches_by_extension() {
        let loader = Loader::new(MemorySource::new().with("shader.wgsl", b"source".to_vec()));

        assert_eq!(
            loader
                .read_shader(&AssetPath::new("shader.wgsl"))
                .unwrap()
                .source,
            "source"
        );
    }

    #[test]
    fn read_mesh_resolves_mtl_as_sibling() {
        let obj =
            b"mtllib cube.mtl\no tri\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl mat\nf 1 2 3\n".to_vec();
        let mtl = b"newmtl mat\nKd 1 0 0\n".to_vec();
        let source = MemorySource::new()
            .with("models/cube.obj", obj)
            .with("models/cube.mtl", mtl);
        let loader = Loader::new(source);

        let model = loader
            .read_mesh(&AssetPath::new("models/cube.obj"))
            .unwrap();

        assert_eq!(model.materials.len(), 1);
    }

    #[test]
    fn read_mesh_dispatches_ply() {
        let ply = b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n".to_vec();
        let source = MemorySource::new().with("models/triangle.ply", ply);
        let loader = Loader::new(source);

        let model = loader
            .read_mesh(&AssetPath::new("models/triangle.ply"))
            .unwrap();

        assert_eq!(model.meshes.len(), 1);
        assert_eq!(model.meshes[0].indices, vec![0, 1, 2]);
        assert!(model.materials.is_empty());
    }

    #[test]
    fn unsupported_extension_returns_error() {
        let loader = Loader::new(MemorySource::new());

        assert!(matches!(
            loader.read_texture(&AssetPath::new("texture.bmp")),
            Err(LoadError::UnsupportedFormat(ext)) if ext == "bmp"
        ));
    }
}
