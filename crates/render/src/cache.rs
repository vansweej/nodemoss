//! Immutable GPU resource cache for shaders, meshes, textures, and samplers.

use std::collections::HashMap;

use rig_assets::{
    AddressMode, FilterMode, IndexFormat, MeshAsset, MeshHandle, SamplerDescriptor, SamplerHandle,
    ShaderAsset, ShaderHandle, TextureAsset, TextureFormat, TextureHandle,
};
use wgpu::util::DeviceExt;

#[derive(Clone)]
pub(crate) struct CachedMeshBuffers {
    pub(crate) vertex: wgpu::Buffer,
    pub(crate) index: wgpu::Buffer,
    pub(crate) index_count: u32,
    pub(crate) index_format: wgpu::IndexFormat,
}

#[derive(Default)]
pub(crate) struct ImmutableResourceCache {
    pub(crate) shaders: HashMap<ShaderHandle, wgpu::ShaderModule>,
    pub(crate) meshes: HashMap<MeshHandle, CachedMeshBuffers>,
    pub(crate) textures: HashMap<TextureHandle, wgpu::Texture>,
    pub(crate) texture_views: HashMap<TextureHandle, wgpu::TextureView>,
    pub(crate) samplers: HashMap<SamplerHandle, wgpu::Sampler>,
}

#[cfg(not(tarpaulin_include))]
impl ImmutableResourceCache {
    pub(crate) fn shader_module(
        &mut self,
        device: &wgpu::Device,
        handle: ShaderHandle,
        shader: &ShaderAsset,
    ) -> wgpu::ShaderModule {
        if let Some(module) = self.shaders.get(&handle) {
            return module.clone();
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rig render shader"),
            source: wgpu::ShaderSource::Wgsl(shader.source.as_ref().into()),
        });
        self.shaders.insert(handle, module.clone());
        module
    }

    pub(crate) fn mesh_buffers(
        &mut self,
        device: &wgpu::Device,
        handle: MeshHandle,
        mesh: &MeshAsset,
    ) -> CachedMeshBuffers {
        if let Some(buffers) = self.meshes.get(&handle) {
            return buffers.clone();
        }
        let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertex buffer"),
            contents: &mesh.vertex_data,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh index buffer"),
            contents: &mesh.index_data,
            usage: wgpu::BufferUsages::INDEX,
        });
        let index_count = match mesh.index_format {
            IndexFormat::Uint16 => (mesh.index_data.len() / std::mem::size_of::<u16>()) as u32,
            IndexFormat::Uint32 => (mesh.index_data.len() / std::mem::size_of::<u32>()) as u32,
        };
        let wgpu_index_format = match mesh.index_format {
            IndexFormat::Uint16 => wgpu::IndexFormat::Uint16,
            IndexFormat::Uint32 => wgpu::IndexFormat::Uint32,
        };
        let buffers = CachedMeshBuffers {
            vertex,
            index,
            index_count,
            index_format: wgpu_index_format,
        };
        self.meshes.insert(handle, buffers.clone());
        buffers
    }

    /// Upload a texture asset to the GPU (idempotent — returns cached view if already uploaded).
    pub(crate) fn texture_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        handle: TextureHandle,
        asset: &TextureAsset,
    ) -> &wgpu::TextureView {
        if !self.texture_views.contains_key(&handle) {
            let wgpu_format = map_texture_format(asset.format);
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("cached texture"),
                size: wgpu::Extent3d { width: asset.width, height: asset.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &asset.data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(asset.width * 4),
                    rows_per_image: Some(asset.height),
                },
                wgpu::Extent3d { width: asset.width, height: asset.height, depth_or_array_layers: 1 },
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.textures.insert(handle, texture);
            self.texture_views.insert(handle, view);
        }
        // SAFETY: we just inserted above if not present
        &self.texture_views[&handle]
    }

    /// Get or create a sampler from a descriptor (idempotent).
    pub(crate) fn sampler(
        &mut self,
        device: &wgpu::Device,
        handle: SamplerHandle,
        desc: &SamplerDescriptor,
    ) -> &wgpu::Sampler {
        self.samplers.entry(handle).or_insert_with(|| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("cached sampler"),
                address_mode_u: map_address_mode(desc.address_mode_u),
                address_mode_v: map_address_mode(desc.address_mode_v),
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: map_filter_mode(desc.mag_filter),
                min_filter: map_filter_mode(desc.min_filter),
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            })
        })
    }
}

fn map_texture_format(fmt: TextureFormat) -> wgpu::TextureFormat {
    match fmt {
        TextureFormat::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
        TextureFormat::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
    }
}

fn map_address_mode(mode: AddressMode) -> wgpu::AddressMode {
    match mode {
        AddressMode::ClampToEdge => wgpu::AddressMode::ClampToEdge,
        AddressMode::Repeat => wgpu::AddressMode::Repeat,
        AddressMode::MirrorRepeat => wgpu::AddressMode::MirrorRepeat,
    }
}

fn map_filter_mode(mode: FilterMode) -> wgpu::FilterMode {
    match mode {
        FilterMode::Nearest => wgpu::FilterMode::Nearest,
        FilterMode::Linear => wgpu::FilterMode::Linear,
    }
}
