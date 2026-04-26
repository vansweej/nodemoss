//! Immutable GPU resource cache for shaders and mesh buffers.

use std::collections::HashMap;

use rig_assets::{IndexFormat, MeshAsset, MeshHandle, ShaderAsset, ShaderHandle};
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
}
