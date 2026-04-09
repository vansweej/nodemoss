//! Immutable shared asset store for the rig framework.

use std::sync::Arc;

use rig_math::BoundingSphere;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshHandle(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialHandle(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShaderHandle(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureHandle(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SamplerHandle(u32);

impl MeshHandle {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl MaterialHandle {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl ShaderHandle {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl TextureHandle {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl SamplerHandle {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VertexAttribute {
    pub shader_location: u32,
    pub format: VertexFormat,
    pub offset: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VertexLayout {
    pub array_stride: u64,
    pub attributes: Vec<VertexAttribute>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VertexFormat {
    Float32x2,
    Float32x3,
}

#[derive(Clone, Debug)]
pub struct MeshAsset {
    pub vertex_layout: VertexLayout,
    pub vertex_data: Arc<[u8]>,
    pub index_data: Arc<[u8]>,
    pub local_bounds: BoundingSphere,
}

/// Blinn-Phong material color properties for GPU upload.
///
/// All fields are `[f32; 4]` so the struct can derive `Pod`/`Zeroable` and be
/// uploaded directly to a uniform buffer without an intermediate conversion step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialParams {
    /// Ambient reflectance (RGBA).
    pub ambient: [f32; 4],
    /// Diffuse reflectance (RGBA).
    pub diffuse: [f32; 4],
    /// Specular reflectance; `w` component is the shininess (Phong exponent).
    pub specular: [f32; 4],
    /// Emissive color (RGBA). Zero by default.
    pub emissive: [f32; 4],
}

impl Default for MaterialParams {
    fn default() -> Self {
        Self {
            ambient: [0.2, 0.2, 0.2, 1.0],
            diffuse: [0.8, 0.8, 0.8, 1.0],
            specular: [1.0, 1.0, 1.0, 32.0],
            emissive: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

#[derive(Clone, Debug)]
pub struct MaterialAsset {
    pub shader: ShaderHandle,
    pub parameters: MaterialParams,
    /// Texture slots: each entry is a `(texture, sampler)` pair.
    pub textures: Vec<(TextureHandle, SamplerHandle)>,
}

#[derive(Clone, Debug)]
pub struct ShaderAsset {
    pub source: Arc<str>,
}

/// Pixel format for texture data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    Rgba8Unorm,
    Rgba8UnormSrgb,
}

/// Texture wrapping mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AddressMode {
    ClampToEdge,
    Repeat,
    MirrorRepeat,
}

/// Texture filtering mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FilterMode {
    Nearest,
    Linear,
}

/// Renderer-agnostic sampler description.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SamplerDescriptor {
    pub address_mode_u: AddressMode,
    pub address_mode_v: AddressMode,
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
}

impl Default for SamplerDescriptor {
    fn default() -> Self {
        Self {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TextureAsset {
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
    pub data: Arc<[u8]>,
}

#[derive(Debug, Error)]
pub enum AssetError {
    #[error("invalid mesh handle")]
    InvalidMesh,
    #[error("invalid material handle")]
    InvalidMaterial,
    #[error("invalid shader handle")]
    InvalidShader,
    #[error("invalid texture handle")]
    InvalidTexture,
    #[error("invalid sampler handle")]
    InvalidSampler,
}

#[derive(Default)]
pub struct AssetStore {
    meshes: Vec<MeshAsset>,
    materials: Vec<MaterialAsset>,
    shaders: Vec<ShaderAsset>,
    textures: Vec<TextureAsset>,
    samplers: Vec<SamplerDescriptor>,
}

impl AssetStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_mesh(&mut self, mesh: MeshAsset) -> MeshHandle {
        let handle = MeshHandle(self.meshes.len() as u32);
        self.meshes.push(mesh);
        handle
    }

    pub fn add_material(&mut self, material: MaterialAsset) -> MaterialHandle {
        let handle = MaterialHandle(self.materials.len() as u32);
        self.materials.push(material);
        handle
    }

    pub fn add_shader(&mut self, shader: ShaderAsset) -> ShaderHandle {
        let handle = ShaderHandle(self.shaders.len() as u32);
        self.shaders.push(shader);
        handle
    }

    pub fn add_texture(&mut self, texture: TextureAsset) -> TextureHandle {
        let handle = TextureHandle(self.textures.len() as u32);
        self.textures.push(texture);
        handle
    }

    pub fn add_sampler(&mut self, sampler: SamplerDescriptor) -> SamplerHandle {
        let handle = SamplerHandle(self.samplers.len() as u32);
        self.samplers.push(sampler);
        handle
    }

    pub fn mesh(&self, handle: MeshHandle) -> Result<&MeshAsset, AssetError> {
        self.meshes
            .get(handle.index())
            .ok_or(AssetError::InvalidMesh)
    }

    pub fn material(&self, handle: MaterialHandle) -> Result<&MaterialAsset, AssetError> {
        self.materials
            .get(handle.index())
            .ok_or(AssetError::InvalidMaterial)
    }

    pub fn shader(&self, handle: ShaderHandle) -> Result<&ShaderAsset, AssetError> {
        self.shaders
            .get(handle.index())
            .ok_or(AssetError::InvalidShader)
    }

    pub fn texture(&self, handle: TextureHandle) -> Result<&TextureAsset, AssetError> {
        self.textures
            .get(handle.index())
            .ok_or(AssetError::InvalidTexture)
    }

    pub fn sampler(&self, handle: SamplerHandle) -> Result<&SamplerDescriptor, AssetError> {
        self.samplers
            .get(handle.index())
            .ok_or(AssetError::InvalidSampler)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_math::Vec3;

    fn sample_layout() -> VertexLayout {
        VertexLayout {
            array_stride: 24,
            attributes: vec![
                VertexAttribute {
                    shader_location: 0,
                    format: VertexFormat::Float32x3,
                    offset: 0,
                },
                VertexAttribute {
                    shader_location: 1,
                    format: VertexFormat::Float32x3,
                    offset: 12,
                },
            ],
        }
    }

    fn sample_mesh() -> MeshAsset {
        MeshAsset {
            vertex_layout: sample_layout(),
            vertex_data: Arc::from([0_u8; 24]),
            index_data: Arc::from([0_u8; 6]),
            local_bounds: BoundingSphere {
                center: Vec3::ZERO,
                radius: 1.0,
            },
        }
    }

    #[test]
    fn handles_expose_underlying_index() {
        assert_eq!(MeshHandle(2).index(), 2);
        assert_eq!(MaterialHandle(3).index(), 3);
        assert_eq!(ShaderHandle(4).index(), 4);
        assert_eq!(TextureHandle(5).index(), 5);
    }

    #[test]
    fn new_asset_store_is_default() {
        let store = AssetStore::new();

        assert!(store.mesh(MeshHandle(0)).is_err());
    }

    #[test]
    fn add_mesh_returns_stable_handle_and_retrieves_asset() {
        let mut store = AssetStore::new();
        let mesh = sample_mesh();

        let handle = store.add_mesh(mesh.clone());

        assert_eq!(handle.index(), 0);
        assert_eq!(
            store.mesh(handle).unwrap().vertex_layout,
            mesh.vertex_layout
        );
    }

    #[test]
    fn add_material_returns_incrementing_handles() {
        let mut store = AssetStore::new();
        let shader = store.add_shader(ShaderAsset {
            source: Arc::from("shader"),
        });

        let first = store.add_material(MaterialAsset { shader, parameters: MaterialParams::default(), textures: vec![] });
        let second = store.add_material(MaterialAsset { shader, parameters: MaterialParams::default(), textures: vec![] });

        assert_eq!(first.index(), 0);
        assert_eq!(second.index(), 1);
    }

    #[test]
    fn add_shader_returns_retrievable_asset() {
        let mut store = AssetStore::new();
        let handle = store.add_shader(ShaderAsset {
            source: Arc::from("shader source"),
        });

        assert_eq!(&*store.shader(handle).unwrap().source, "shader source");
    }

    #[test]
    fn add_texture_returns_retrievable_asset() {
        let mut store = AssetStore::new();
        let handle = store.add_texture(TextureAsset {
            width: 2,
            height: 3,
            format: TextureFormat::Rgba8Unorm,
            data: Arc::from([255_u8, 0, 0, 255]),
        });

        let texture = store.texture(handle).unwrap();
        assert_eq!(texture.width, 2);
        assert_eq!(texture.height, 3);
        assert_eq!(texture.format, TextureFormat::Rgba8Unorm);
    }

    #[test]
    fn invalid_mesh_handle_returns_error() {
        let store = AssetStore::new();

        assert!(matches!(
            store.mesh(MeshHandle(99)),
            Err(AssetError::InvalidMesh)
        ));
    }

    #[test]
    fn invalid_material_handle_returns_error() {
        let store = AssetStore::new();

        assert!(matches!(
            store.material(MaterialHandle(99)),
            Err(AssetError::InvalidMaterial)
        ));
    }

    #[test]
    fn invalid_shader_handle_returns_error() {
        let store = AssetStore::new();

        assert!(matches!(
            store.shader(ShaderHandle(99)),
            Err(AssetError::InvalidShader)
        ));
    }

    #[test]
    fn invalid_texture_handle_returns_error() {
        let store = AssetStore::new();

        assert!(matches!(
            store.texture(TextureHandle(99)),
            Err(AssetError::InvalidTexture)
        ));
    }

    #[test]
    fn handles_expose_sampler_index() {
        assert_eq!(SamplerHandle(7).index(), 7);
    }

    #[test]
    fn sampler_descriptor_default_is_linear_clamp() {
        let desc = SamplerDescriptor::default();
        assert_eq!(desc.mag_filter, FilterMode::Linear);
        assert_eq!(desc.min_filter, FilterMode::Linear);
        assert_eq!(desc.address_mode_u, AddressMode::ClampToEdge);
        assert_eq!(desc.address_mode_v, AddressMode::ClampToEdge);
    }

    #[test]
    fn add_sampler_returns_retrievable_descriptor() {
        let mut store = AssetStore::new();
        let desc = SamplerDescriptor {
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
        };

        let handle = store.add_sampler(desc);

        assert_eq!(handle.index(), 0);
        assert_eq!(*store.sampler(handle).unwrap(), desc);
    }

    #[test]
    fn invalid_sampler_handle_returns_error() {
        let store = AssetStore::new();

        assert!(matches!(
            store.sampler(SamplerHandle(99)),
            Err(AssetError::InvalidSampler)
        ));
    }

    #[test]
    fn material_with_textures_stores_pairs() {
        let mut store = AssetStore::new();
        let shader = store.add_shader(ShaderAsset { source: Arc::from("s") });
        let tex = store.add_texture(TextureAsset {
            width: 1,
            height: 1,
            format: TextureFormat::Rgba8UnormSrgb,
            data: Arc::from([255_u8, 255, 255, 255]),
        });
        let samp = store.add_sampler(SamplerDescriptor::default());
        let mat = store.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams::default(),
            textures: vec![(tex, samp)],
        });

        let retrieved = store.material(mat).unwrap();
        assert_eq!(retrieved.textures.len(), 1);
        assert_eq!(retrieved.textures[0], (tex, samp));
    }
}
