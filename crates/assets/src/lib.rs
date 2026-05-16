//! Immutable shared asset store for the rig framework.

pub mod animation;
pub mod chunk_manager;
pub mod erosion;
pub mod lod;
pub mod marching_cubes;
pub mod mesh_factory;
pub mod tangent_utils;

pub use animation::{
    AnimationChannel, AnimationClip, ChannelProperty, KeyframeSampler, KeyframeValues,
};
pub use chunk_manager::{ChunkCoord, ChunkManager, ChunkUpdate};
pub use erosion::{ErosionParams, erode};
pub use lod::{LodLevel, needs_lod_update, select_lod};

use std::sync::Arc;

use rig_math::{BoundingSphere, Mat4};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MeshHandle(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaterialHandle(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShaderHandle(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureHandle(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SamplerHandle(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimationClipHandle(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SkinAssetHandle(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SkinWeightsHandle(u32);

impl MeshHandle {
    pub fn from_raw(v: u32) -> Self {
        Self(v)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl MaterialHandle {
    pub fn from_raw(v: u32) -> Self {
        Self(v)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl ShaderHandle {
    pub fn from_raw(v: u32) -> Self {
        Self(v)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl TextureHandle {
    pub fn from_raw(v: u32) -> Self {
        Self(v)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl SamplerHandle {
    pub fn from_raw(v: u32) -> Self {
        Self(v)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl AnimationClipHandle {
    pub fn from_raw(v: u32) -> Self {
        Self(v)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl SkinAssetHandle {
    pub fn from_raw(v: u32) -> Self {
        Self(v)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl SkinWeightsHandle {
    pub fn from_raw(v: u32) -> Self {
        Self(v)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Opaque handle to a dynamic (per-frame mutable) mesh registered with the renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DynamicMeshId(u32);

impl DynamicMeshId {
    pub fn from_raw(v: u32) -> Self {
        Self(v)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Identifies the source of mesh geometry for a renderable node.
///
/// - `Static` — an immutable [`MeshAsset`] cached by the renderer; never changes after upload.
/// - `Dynamic` — a per-frame mutable mesh managed by the renderer; updated via
///   [`Renderer::update_dynamic_mesh`] each frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MeshSource {
    Static(MeshHandle),
    Dynamic(DynamicMeshId),
}

/// Output of a dynamic mesh generator (e.g. Marching Cubes).
///
/// Vertex data uses the framework's `standard_layout()` (pos + normal + uv + tangent, stride 48).
/// Index data is `u32` (Uint32 format).
pub struct DynamicMeshData {
    /// Raw vertex bytes in `standard_layout()` format (stride 48).
    pub vertex_data: Vec<u8>,
    /// Raw index bytes as packed little-endian values matching `index_format`.
    pub index_data: Vec<u8>,
    /// Index element format for `index_data`.
    pub index_format: IndexFormat,
    /// Number of indices (triangles = index_count / 3).
    pub index_count: u32,
    /// Axis-aligned bounding sphere of the output vertices, in local space.
    pub local_bounds: BoundingSphere,
}

/// Returns the standard vertex layout used by `MeshFactory` and dynamic meshes:
/// `Position: Float32x3` @ 0, `Normal: Float32x3` @ 12,
/// `UV: Float32x2` @ 24, `Tangent: Float32x4` @ 32, stride 48.
pub fn standard_vertex_layout() -> VertexLayout {
    mesh_factory::standard_layout()
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
    Float32,
    Float32x2,
    Float32x3,
    Float32x4,
}

/// Index element width used in a mesh's index buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IndexFormat {
    /// 16-bit unsigned indices (max 65 535 vertices). Default.
    #[default]
    Uint16,
    /// 32-bit unsigned indices for meshes with more than 65 535 vertices.
    Uint32,
}

#[derive(Clone, Debug)]
pub struct MeshAsset {
    pub vertex_layout: VertexLayout,
    pub vertex_data: Arc<[u8]>,
    pub index_data: Arc<[u8]>,
    /// Index element format. Defaults to `Uint16` for backward compatibility.
    pub index_format: IndexFormat,
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
    /// Diffuse reflectance (RGBA). For PBR materials this doubles as the albedo / base colour.
    pub diffuse: [f32; 4],
    /// Specular reflectance; `w` component is the shininess (Phong exponent).
    pub specular: [f32; 4],
    /// Emissive color (RGBA). Zero by default.
    pub emissive: [f32; 4],
    /// PBR metallic factor. `0.0` = dielectric, `1.0` = full metal.
    /// Ignored by non-PBR shaders.
    pub metallic: f32,
    /// PBR roughness factor. `0.0` = mirror-smooth, `1.0` = fully diffuse.
    /// Ignored by non-PBR shaders.
    pub roughness: f32,
    /// Additional shader-defined bit flags OR'd into the GPU MaterialUniforms.flags
    /// field at draw time. Default: 0.
    ///
    /// Example: set bit 5 (`32u32`) to enable triplanar sampling in TRIPLANAR_PBR_SHADER.
    pub custom_flags: u32,
    /// World-space texture repeat scale for triplanar sampling.
    /// Passed to the GPU as MaterialUniforms.triplanar_scale.
    /// Only meaningful when custom_flags includes bit 5 (USE_TRIPLANAR). Default: 4.0.
    pub triplanar_scale: f32,
}

impl Default for MaterialParams {
    fn default() -> Self {
        Self {
            ambient: [0.2, 0.2, 0.2, 1.0],
            diffuse: [0.8, 0.8, 0.8, 1.0],
            specular: [1.0, 1.0, 1.0, 32.0],
            emissive: [0.0, 0.0, 0.0, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            custom_flags: 0,
            triplanar_scale: 4.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MaterialAsset {
    pub shader: ShaderHandle,
    pub parameters: MaterialParams,
    /// PBR texture slots: 0=base color, 1=normal, 2=metallic-roughness,
    /// 3=occlusion, 4=emissive. Missing slots use renderer fallback textures.
    pub textures: Vec<Option<(TextureHandle, SamplerHandle)>>,
}

impl MaterialAsset {
    /// Number of material texture slots in the standard PBR bind group.
    pub const SLOT_COUNT: usize = 5;

    /// Create a material with no populated texture slots.
    pub fn untextured(shader: ShaderHandle, parameters: MaterialParams) -> Self {
        Self {
            shader,
            parameters,
            textures: Vec::new(),
        }
    }
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

/// Immutable skin definition — one per skeleton.
///
/// Stored in `AssetStore`. Multiple `SkinEvaluator` instances (one per
/// character instance) can share one `SkinAsset` via handle.
#[derive(Clone, Debug)]
pub struct SkinAsset {
    /// Joint names in joint-index order.
    /// Resolved to `NodeId`s at bind-time by `SkinEvaluator::bind()`.
    pub joint_names: Vec<String>,
    /// Inverse bind matrix per joint.
    /// `IBM[j]` transforms a vertex from model space into joint-j's local
    /// space at rest pose.
    pub inverse_bind_matrices: Vec<Mat4>,
}

/// Per-vertex skinning weights — paired with a `MeshAsset` by the caller.
///
/// Stored separately from `SkinAsset` so the same skeleton can drive
/// different meshes (e.g. body + clothing layers).
#[derive(Clone, Debug)]
pub struct SkinWeights {
    /// Up to 8 joint indices per vertex (`[joints_0[4], joints_1[4]]`).
    /// Indices reference `SkinAsset::joint_names`.
    pub joints: Vec<[u16; 8]>,
    /// Up to 8 normalized weights per vertex (sum ≈ 1.0).
    /// Unused influences must have weight 0.0.
    pub weights: Vec<[f32; 8]>,
}

impl SkinWeights {
    /// Construct from glTF's two-set layout (`JOINTS_0/1` + `WEIGHTS_0/1`).
    ///
    /// Both primary sets must have the same length (= vertex count).
    /// The second set (`joints_1`/`weights_1`) may be empty for ≤4-influence
    /// meshes — influences 4–7 are zeroed in that case.
    pub fn from_gltf_sets(
        joints_0: &[[u16; 4]],
        weights_0: &[[f32; 4]],
        joints_1: &[[u16; 4]],
        weights_1: &[[f32; 4]],
    ) -> Self {
        let count = joints_0.len();
        let mut joints = Vec::with_capacity(count);
        let mut weights = Vec::with_capacity(count);
        for i in 0..count {
            let j0 = joints_0[i];
            let j1 = if i < joints_1.len() {
                joints_1[i]
            } else {
                [0; 4]
            };
            let w0 = weights_0[i];
            let w1 = if i < weights_1.len() {
                weights_1[i]
            } else {
                [0.0; 4]
            };
            joints.push([j0[0], j0[1], j0[2], j0[3], j1[0], j1[1], j1[2], j1[3]]);
            weights.push([w0[0], w0[1], w0[2], w0[3], w1[0], w1[1], w1[2], w1[3]]);
        }
        Self { joints, weights }
    }

    /// Number of vertices that have weight data.
    pub fn vertex_count(&self) -> usize {
        self.joints.len()
    }
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
    #[error("invalid animation clip handle")]
    InvalidAnimationClip,
    #[error("invalid skin asset handle")]
    InvalidSkin,
    #[error("invalid skin weights handle")]
    InvalidSkinWeights,
}

#[derive(Default)]
pub struct AssetStore {
    meshes: Vec<MeshAsset>,
    materials: Vec<MaterialAsset>,
    shaders: Vec<ShaderAsset>,
    textures: Vec<TextureAsset>,
    samplers: Vec<SamplerDescriptor>,
    animation_clips: Vec<AnimationClip>,
    skins: Vec<SkinAsset>,
    skin_weights: Vec<SkinWeights>,
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

    pub fn add_animation_clip(&mut self, clip: AnimationClip) -> AnimationClipHandle {
        let handle = AnimationClipHandle(self.animation_clips.len() as u32);
        self.animation_clips.push(clip);
        handle
    }

    pub fn add_skin(&mut self, skin: SkinAsset) -> SkinAssetHandle {
        let handle = SkinAssetHandle(self.skins.len() as u32);
        self.skins.push(skin);
        handle
    }

    pub fn add_skin_weights(&mut self, weights: SkinWeights) -> SkinWeightsHandle {
        let handle = SkinWeightsHandle(self.skin_weights.len() as u32);
        self.skin_weights.push(weights);
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

    pub fn animation_clip(
        &self,
        handle: AnimationClipHandle,
    ) -> Result<&AnimationClip, AssetError> {
        self.animation_clips
            .get(handle.index())
            .ok_or(AssetError::InvalidAnimationClip)
    }

    pub fn skin(&self, handle: SkinAssetHandle) -> Result<&SkinAsset, AssetError> {
        self.skins
            .get(handle.index())
            .ok_or(AssetError::InvalidSkin)
    }

    pub fn skin_weights(&self, handle: SkinWeightsHandle) -> Result<&SkinWeights, AssetError> {
        self.skin_weights
            .get(handle.index())
            .ok_or(AssetError::InvalidSkinWeights)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_math::{Interpolation, Vec3};

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
            index_format: IndexFormat::Uint16,
            local_bounds: BoundingSphere {
                center: Vec3::ZERO,
                radius: 1.0,
            },
        }
    }

    fn sample_animation_clip(name: &str, duration: f32) -> AnimationClip {
        AnimationClip {
            name: name.to_string(),
            duration,
            looping: true,
            channels: vec![AnimationChannel {
                target_node: "node".to_string(),
                property: ChannelProperty::Translation,
                sampler: KeyframeSampler {
                    times: vec![0.0, duration],
                    interpolation: Interpolation::Linear,
                    values: KeyframeValues::Translations(vec![Vec3::ZERO, Vec3::X]),
                },
            }],
        }
    }

    fn sample_skin() -> SkinAsset {
        SkinAsset {
            joint_names: vec!["root".to_string(), "tip".to_string()],
            inverse_bind_matrices: vec![Mat4::IDENTITY, Mat4::IDENTITY],
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

        let first = store.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams::default(),
            textures: vec![],
        });
        let second = store.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams::default(),
            textures: vec![],
        });

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
    fn animation_clip_handle_from_raw_round_trips() {
        let handle = AnimationClipHandle::from_raw(3);
        assert_eq!(handle.index(), 3);
        assert_eq!(handle, AnimationClipHandle::from_raw(3));
    }

    #[test]
    fn add_animation_clip_returns_retrievable_asset() {
        let mut store = AssetStore::new();
        let clip = sample_animation_clip("wave", 4.0);

        let handle = store.add_animation_clip(clip);
        let retrieved = store.animation_clip(handle).unwrap();

        assert_eq!(handle.index(), 0);
        assert_eq!(retrieved.name, "wave");
        assert_eq!(retrieved.duration, 4.0);
        assert_eq!(retrieved.channels.len(), 1);
    }

    #[test]
    fn add_two_animation_clips_returns_incrementing_handles() {
        let mut store = AssetStore::new();

        let first = store.add_animation_clip(sample_animation_clip("first", 1.0));
        let second = store.add_animation_clip(sample_animation_clip("second", 2.0));

        assert_eq!(first.index(), 0);
        assert_eq!(second.index(), 1);
    }

    #[test]
    fn invalid_animation_clip_handle_returns_error() {
        let store = AssetStore::new();

        assert!(matches!(
            store.animation_clip(AnimationClipHandle::from_raw(99)),
            Err(AssetError::InvalidAnimationClip)
        ));
    }

    #[test]
    fn dynamic_mesh_id_from_raw_round_trips() {
        let id = DynamicMeshId::from_raw(7);
        assert_eq!(id.index(), 7);
        assert_eq!(id, DynamicMeshId::from_raw(7));
        assert_ne!(id, DynamicMeshId::from_raw(0));
    }

    #[test]
    fn mesh_source_static_and_dynamic_are_not_equal() {
        let static_src = MeshSource::Static(MeshHandle::from_raw(0));
        let dynamic_src = MeshSource::Dynamic(DynamicMeshId::from_raw(0));
        assert_ne!(static_src, dynamic_src);
    }

    #[test]
    fn mesh_source_same_variant_same_handle_are_equal() {
        let a = MeshSource::Static(MeshHandle::from_raw(3));
        let b = MeshSource::Static(MeshHandle::from_raw(3));
        assert_eq!(a, b);
    }

    #[test]
    fn mesh_source_ord_static_before_dynamic() {
        let s = MeshSource::Static(MeshHandle::from_raw(99));
        let d = MeshSource::Dynamic(DynamicMeshId::from_raw(0));
        assert!(s < d);
    }

    #[test]
    fn material_with_textures_stores_pairs() {
        let mut store = AssetStore::new();
        let shader = store.add_shader(ShaderAsset {
            source: Arc::from("s"),
        });
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
            textures: vec![Some((tex, samp))],
        });

        let retrieved = store.material(mat).unwrap();
        assert_eq!(retrieved.textures.len(), 1);
        assert_eq!(retrieved.textures[0], Some((tex, samp)));
    }

    #[test]
    fn skin_asset_handle_from_raw_round_trips() {
        let handle = SkinAssetHandle::from_raw(5);
        assert_eq!(handle.index(), 5);
        assert_eq!(handle, SkinAssetHandle::from_raw(5));
    }

    #[test]
    fn skin_weights_handle_from_raw_round_trips() {
        let handle = SkinWeightsHandle::from_raw(5);
        assert_eq!(handle.index(), 5);
        assert_eq!(handle, SkinWeightsHandle::from_raw(5));
    }

    #[test]
    fn add_skin_returns_retrievable_asset() {
        let mut store = AssetStore::new();
        let handle = store.add_skin(sample_skin());

        let skin = store.skin(handle).unwrap();

        assert_eq!(skin.joint_names.len(), 2);
        assert_eq!(skin.inverse_bind_matrices.len(), 2);
    }

    #[test]
    fn add_skin_weights_returns_retrievable_asset() {
        let mut store = AssetStore::new();
        let joints_0 = [[0, 1, 0, 0], [1, 2, 0, 0], [2, 3, 0, 0]];
        let weights_0 = [[0.5, 0.5, 0.0, 0.0]; 3];
        let weights = SkinWeights::from_gltf_sets(&joints_0, &weights_0, &[], &[]);

        let handle = store.add_skin_weights(weights);

        assert_eq!(store.skin_weights(handle).unwrap().vertex_count(), 3);
    }

    #[test]
    fn invalid_skin_handle_returns_error() {
        let store = AssetStore::new();

        assert!(matches!(
            store.skin(SkinAssetHandle::from_raw(99)),
            Err(AssetError::InvalidSkin)
        ));
    }

    #[test]
    fn invalid_skin_weights_handle_returns_error() {
        let store = AssetStore::new();

        assert!(matches!(
            store.skin_weights(SkinWeightsHandle::from_raw(99)),
            Err(AssetError::InvalidSkinWeights)
        ));
    }

    #[test]
    fn skin_weights_from_gltf_sets_merges_correctly() {
        let joints_0 = [[1, 2, 3, 4]];
        let weights_0 = [[0.1, 0.2, 0.3, 0.4]];
        let joints_1 = [[5, 6, 7, 8]];
        let weights_1 = [[0.5, 0.6, 0.7, 0.8]];

        let weights = SkinWeights::from_gltf_sets(&joints_0, &weights_0, &joints_1, &weights_1);

        assert_eq!(weights.joints[0], [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(weights.weights[0], [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]);
    }

    #[test]
    fn skin_weights_from_gltf_sets_handles_empty_second_set() {
        let joints_0 = [[1, 2, 3, 4]];
        let weights_0 = [[0.1, 0.2, 0.3, 0.4]];

        let weights = SkinWeights::from_gltf_sets(&joints_0, &weights_0, &[], &[]);

        assert_eq!(weights.joints[0], [1, 2, 3, 4, 0, 0, 0, 0]);
        assert_eq!(weights.weights[0], [0.1, 0.2, 0.3, 0.4, 0.0, 0.0, 0.0, 0.0]);
    }
}
