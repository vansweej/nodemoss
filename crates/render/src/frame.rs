//! Per-frame GPU resource allocation: dynamic uniform buffers.

use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};

use crate::Result;
use crate::helpers::{aligned_uniform_size, encode_object_uniforms, object_uniform_offset};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct ObjectUniforms {
    pub(crate) world: [[f32; 4]; 4],
}

pub(crate) struct ObjectUniformBuffer {
    pub(crate) buffer: wgpu::Buffer,
    pub(crate) bind_group: wgpu::BindGroup,
    pub(crate) stride: u64,
    pub(crate) capacity: usize,
}

#[cfg(not(tarpaulin_include))]
impl ObjectUniformBuffer {
    pub(crate) fn new(
        device: &wgpu::Device,
        object_bind_group_layout: &wgpu::BindGroupLayout,
        stride: u64,
        capacity: usize,
    ) -> Self {
        let capacity = capacity.max(1);
        let size = stride * capacity as u64;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("object uniform buffer"),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("object uniform bind group"),
            layout: object_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: NonZeroU64::new(std::mem::size_of::<ObjectUniforms>() as u64),
                }),
            }],
        });
        Self {
            buffer,
            bind_group,
            stride,
            capacity,
        }
    }

    pub(crate) fn ensure_capacity(
        &mut self,
        device: &wgpu::Device,
        object_bind_group_layout: &wgpu::BindGroupLayout,
        required_capacity: usize,
    ) {
        if required_capacity <= self.capacity {
            return;
        }
        *self = Self::new(
            device,
            object_bind_group_layout,
            self.stride,
            required_capacity.next_power_of_two(),
        );
    }

    pub(crate) fn write(&mut self, queue: &wgpu::Queue, uniforms: &[ObjectUniforms]) {
        if uniforms.is_empty() {
            return;
        }
        let bytes = encode_object_uniforms(uniforms, self.stride);
        queue.write_buffer(&self.buffer, 0, &bytes);
    }

    pub(crate) fn dynamic_offset(&self, index: usize) -> Result<u32> {
        object_uniform_offset(index, self.stride)
    }
}

pub(crate) struct FrameResources {
    pub(crate) object_uniforms: ObjectUniformBuffer,
}

#[cfg(not(tarpaulin_include))]
impl FrameResources {
    pub(crate) fn new(
        device: &wgpu::Device,
        object_bind_group_layout: &wgpu::BindGroupLayout,
        object_uniform_alignment: u64,
    ) -> Self {
        let stride = aligned_uniform_size(
            std::mem::size_of::<ObjectUniforms>() as u64,
            object_uniform_alignment,
        );
        Self {
            object_uniforms: ObjectUniformBuffer::new(device, object_bind_group_layout, stride, 1),
        }
    }

    pub(crate) fn prepare_object_uniforms(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        object_bind_group_layout: &wgpu::BindGroupLayout,
        uniforms: &[ObjectUniforms],
    ) {
        self.object_uniforms
            .ensure_capacity(device, object_bind_group_layout, uniforms.len());
        self.object_uniforms.write(queue, uniforms);
    }
}
