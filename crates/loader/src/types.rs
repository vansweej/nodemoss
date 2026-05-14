//! Decoded, format-faithful asset data.

/// Color-space interpretation of decoded image bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    /// Linear data such as normal maps or masks.
    Linear,
    /// sRGB color data such as diffuse/albedo textures.
    Srgb,
}

/// Image decoded to RGBA8 pixels.
///
/// `data` is always normalized to RGBA8. `channels` records the original source
/// channel count so importers can distinguish real alpha from padded alpha.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Original source channel count before RGBA normalization.
    pub channels: u8,
    /// Color-space interpretation for GPU format selection.
    pub color_space: ColorSpace,
    /// RGBA8 pixel bytes in row-major order.
    pub data: Vec<u8>,
}

/// Mesh decoded from an OBJ model.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedMesh {
    /// Mesh/object name from the source file.
    pub name: String,
    /// Flat `x, y, z` position array.
    pub positions: Vec<f32>,
    /// Flat `x, y, z` normal array, possibly empty.
    pub normals: Vec<f32>,
    /// Flat `u, v` texture-coordinate array, possibly empty.
    pub uvs: Vec<f32>,
    /// Triangle indices into the vertex arrays.
    pub indices: Vec<u32>,
    /// Material index into [`DecodedModel::materials`].
    pub material_index: Option<usize>,
}

/// Full model decoded from one mesh file.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedModel {
    /// Meshes contained in the file.
    pub meshes: Vec<DecodedMesh>,
    /// Materials referenced by meshes.
    pub materials: Vec<DecodedMaterial>,
}

/// Material values decoded from an MTL file.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedMaterial {
    /// Material name.
    pub name: String,
    /// Diffuse RGB color.
    pub diffuse: [f32; 3],
    /// Specular RGB color.
    pub specular: [f32; 3],
    /// Phong shininess exponent.
    pub shininess: f32,
    /// Optional diffuse texture path as written in the MTL file.
    pub diffuse_texture: Option<String>,
}

/// WGSL shader source decoded as UTF-8 text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedShader {
    /// Shader source text.
    pub source: String,
}
