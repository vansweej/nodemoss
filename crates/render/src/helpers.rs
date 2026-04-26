//! Pure utility functions, uniform types, and embedded WGSL shader constants.

use bytemuck::{Pod, Zeroable};
use rig_assets::{VertexFormat, VertexLayout};
use rig_math::{Camera, Mat4};
use rig_scene::ExtractedCamera;

use crate::{RenderError, Result};

/// The depth format used for all main render passes.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

// ── Uniform structs ──────────────────────────────────────────────────────────

/// Per-frame uniform data: camera matrices uploaded once per frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct FrameUniforms {
    pub view: [[f32; 4]; 4],
    pub proj: [[f32; 4]; 4],
    /// xyz = camera world position, w = padding.
    pub camera_pos: [f32; 4],
}

/// Per-material uniform data.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct MaterialUniforms {
    pub base_color: [f32; 4],
    pub flags: u32,
    pub _pad: [u32; 3],
}

// ── Embedded shaders ─────────────────────────────────────────────────────────

/// Vertex-color triangle shader — 3-group layout (group 0 = frame, 1 = material, 2 = object).
pub const TRIANGLE_SHADER: &str = r#"
struct FrameUniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
}

struct MaterialUniforms {
    base_color: vec4<f32>,
    flags: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct ObjectUniforms {
    world: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(1) @binding(0) var<uniform> material: MaterialUniforms;
@group(1) @binding(1) var t_diffuse: texture_2d<f32>;
@group(1) @binding(2) var s_diffuse: sampler;
@group(2) @binding(0) var<uniform> object: ObjectUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let pv = frame.proj * frame.view;
    out.clip_position = pv * object.world * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
"#;

/// WGSL shader that maps vertex normals to RGB colour — 3-group layout.
///
/// Vertex layout: position @ location 0 (`Float32x3`), normal @ location 1
/// (`Float32x3`), UV @ location 2 (`Float32x2`). Stride = 32 bytes.
pub const NORMAL_COLOR_SHADER: &str = r#"
struct FrameUniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
}

struct MaterialUniforms {
    base_color: vec4<f32>,
    flags: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct ObjectUniforms {
    world: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(1) @binding(0) var<uniform> material: MaterialUniforms;
@group(1) @binding(1) var t_diffuse: texture_2d<f32>;
@group(1) @binding(2) var s_diffuse: sampler;
@group(2) @binding(0) var<uniform> object: ObjectUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       color:         vec3<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let pv = frame.proj * frame.view;
    out.clip_position = pv * object.world * vec4<f32>(in.position, 1.0);
    out.color = in.normal * 0.5 + vec3<f32>(0.5, 0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
"#;

/// WGSL shader that samples a diffuse texture — 3-group layout.
///
/// Vertex layout: position @ location 0 (`Float32x3`), normal @ location 1
/// (`Float32x3`), UV @ location 2 (`Float32x2`). Stride = 32 bytes.
pub const TEXTURED_SHADER: &str = r#"
struct FrameUniforms {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
}

struct MaterialUniforms {
    base_color: vec4<f32>,
    flags: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct ObjectUniforms {
    world: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(1) @binding(0) var<uniform> material: MaterialUniforms;
@group(1) @binding(1) var t_diffuse: texture_2d<f32>;
@group(1) @binding(2) var s_diffuse: sampler;
@group(2) @binding(0) var<uniform> object: ObjectUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       uv:            vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let pv = frame.proj * frame.view;
    out.clip_position = pv * object.world * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_diffuse, s_diffuse, in.uv);
    return tex_color * material.base_color;
}
"#;

// ── Helper functions ─────────────────────────────────────────────────────────

pub(crate) fn aligned_uniform_size(size: u64, alignment: u64) -> u64 {
    if alignment <= 1 {
        return size;
    }
    let remainder = size % alignment;
    if remainder == 0 {
        size
    } else {
        size + (alignment - remainder)
    }
}

pub(crate) fn object_uniform_offset(index: usize, stride: u64) -> Result<u32> {
    let offset = index as u64 * stride;
    u32::try_from(offset)
        .map_err(|_| RenderError::Asset("object uniform offset exceeds u32 range".into()))
}

pub(crate) fn encode_object_uniforms(uniforms: &[crate::frame::ObjectUniforms], stride: u64) -> Vec<u8> {
    let object_size = std::mem::size_of::<crate::frame::ObjectUniforms>();
    let stride = stride as usize;
    let mut bytes = vec![0_u8; stride * uniforms.len()];
    for (index, uniform) in uniforms.iter().enumerate() {
        let offset = index * stride;
        bytes[offset..offset + object_size].copy_from_slice(bytemuck::bytes_of(uniform));
    }
    bytes
}

pub(crate) fn decompose_pose(world: Mat4) -> rig_math::Transform {
    let (_, rotation, translation) = world.to_scale_rotation_translation();
    rig_math::Transform {
        translation,
        rotation,
        scale: rig_math::Vec3::ONE,
    }
}

pub(crate) fn camera_projection_view(camera: &ExtractedCamera, aspect: f32) -> Mat4 {
    let pose = decompose_pose(camera.world_transform);
    let camera_value = Camera {
        pose,
        projection: camera.projection,
    };
    camera_value.projection_view_matrix(aspect)
}

pub fn vertex_format_size(format: VertexFormat) -> u64 {
    match format {
        VertexFormat::Float32 => std::mem::size_of::<f32>() as u64,
        VertexFormat::Float32x2 => std::mem::size_of::<[f32; 2]>() as u64,
        VertexFormat::Float32x3 => std::mem::size_of::<[f32; 3]>() as u64,
        VertexFormat::Float32x4 => std::mem::size_of::<[f32; 4]>() as u64,
    }
}

pub fn wgpu_vertex_format(format: VertexFormat) -> wgpu::VertexFormat {
    match format {
        VertexFormat::Float32 => wgpu::VertexFormat::Float32,
        VertexFormat::Float32x2 => wgpu::VertexFormat::Float32x2,
        VertexFormat::Float32x3 => wgpu::VertexFormat::Float32x3,
        VertexFormat::Float32x4 => wgpu::VertexFormat::Float32x4,
    }
}

/// Generic vertex layout validator.
pub fn validate_vertex_layout(vertex_layout: &VertexLayout) -> std::result::Result<(), String> {
    if vertex_layout.array_stride == 0 {
        return Err("vertex layout must use a non-zero array stride".into());
    }
    if vertex_layout.attributes.is_empty() {
        return Err("vertex layout must contain at least one attribute".into());
    }
    let mut seen_locations = std::collections::HashSet::new();
    for attribute in &vertex_layout.attributes {
        if !seen_locations.insert(attribute.shader_location) {
            return Err(format!(
                "vertex layout contains duplicate shader location {}",
                attribute.shader_location
            ));
        }
        let format_size = vertex_format_size(attribute.format);
        if attribute.offset + format_size > vertex_layout.array_stride {
            return Err(format!(
                "vertex attribute at location {} exceeds the declared array stride",
                attribute.shader_location
            ));
        }
    }
    Ok(())
}

pub(crate) fn mesh_vertex_attributes(
    vertex_layout: &VertexLayout,
) -> std::result::Result<Vec<wgpu::VertexAttribute>, String> {
    validate_vertex_layout(vertex_layout)?;
    Ok(vertex_layout
        .attributes
        .iter()
        .map(|attribute| wgpu::VertexAttribute {
            format: wgpu_vertex_format(attribute.format),
            offset: attribute.offset,
            shader_location: attribute.shader_location,
        })
        .collect())
}

/// Create a depth texture and its default view sized to `width x height`.
#[cfg(not(tarpaulin_include))]
pub fn create_depth_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

#[cfg(not(tarpaulin_include))]
pub(crate) fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    pipeline_layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: Option<wgpu::TextureFormat>,
    vertex_layout: &VertexLayout,
) -> Result<wgpu::RenderPipeline> {
    let attributes = mesh_vertex_attributes(vertex_layout).map_err(RenderError::Asset)?;
    let buffer_layout = wgpu::VertexBufferLayout {
        array_stride: vertex_layout.array_stride,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &attributes,
    };
    let depth_stencil = depth_format.map(|format| wgpu::DepthStencilState {
        format,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    });
    Ok(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("rig render pipeline"),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[buffer_layout],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    }))
}
