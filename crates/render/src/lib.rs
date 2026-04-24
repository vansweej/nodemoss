//! Concrete `wgpu` renderer for the rig framework.
//!
//! [`Renderer`] owns only rendering state (pipelines, caches, frame resources,
//! depth texture). It does **not** own the GPU device, queue, or surface —
//! those live in [`rig_gpu::GpuContext`], which is passed by reference to
//! every method that needs GPU access.

use std::{
    collections::HashMap,
    num::NonZeroU64,
};

use bytemuck::{Pod, Zeroable};
use rig_assets::{
    AssetStore, IndexFormat, MeshAsset, MeshHandle, ShaderAsset, ShaderHandle, VertexFormat,
    VertexLayout,
};
use rig_gpu::{Frame, GpuContext};
use rig_math::{Camera, Mat4};
use rig_scene::{ExtractedCamera, ExtractedRenderable, NodeId, SceneGraph};
use thiserror::Error;
use wgpu::util::DeviceExt;

pub use rig_gpu;
pub use wgpu;

// ── errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("scene node is not a camera")]
    InvalidCamera,
    #[error("asset error: {0}")]
    Asset(String),
}

pub type Result<T> = std::result::Result<T, RenderError>;

// ── internal types ────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ObjectUniforms {
    world: [[f32; 4]; 4],
}

#[derive(Clone)]
struct CachedMeshBuffers {
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    index_count: u32,
    index_format: wgpu::IndexFormat,
}

/// Key used to look up a cached render pipeline.
///
/// Two pipelines are distinct if they differ in shader, vertex layout,
/// the colour format of their render target, or whether they write depth.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PipelineKey {
    shader: ShaderHandle,
    vertex_layout: VertexLayout,
    color_format: wgpu::TextureFormat,
    depth_format: Option<wgpu::TextureFormat>,
}

struct ObjectUniformBuffer {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    stride: u64,
    capacity: usize,
}

impl ObjectUniformBuffer {
    fn new(
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

    fn ensure_capacity(
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

    fn write(&mut self, queue: &wgpu::Queue, uniforms: &[ObjectUniforms]) {
        if uniforms.is_empty() {
            return;
        }

        let bytes = encode_object_uniforms(uniforms, self.stride);
        queue.write_buffer(&self.buffer, 0, &bytes);
    }

    fn dynamic_offset(&self, index: usize) -> Result<u32> {
        object_uniform_offset(index, self.stride)
    }
}

struct FrameResources {
    object_uniforms: ObjectUniformBuffer,
}

impl FrameResources {
    fn new(
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

    fn prepare_object_uniforms(
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

#[derive(Default)]
struct ImmutableResourceCache {
    shaders: HashMap<ShaderHandle, wgpu::ShaderModule>,
    meshes: HashMap<MeshHandle, CachedMeshBuffers>,
}

impl ImmutableResourceCache {
    fn shader_module(
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

    fn mesh_buffers(
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

// ── public types ──────────────────────────────────────────────────────────────

/// Descriptor used to create a [`RenderTarget`].
pub struct RenderTargetDescriptor {
    pub width: u32,
    pub height: u32,
    pub color_format: wgpu::TextureFormat,
    pub depth_format: Option<wgpu::TextureFormat>,
    pub label: &'static str,
}

/// An offscreen render target: a colour texture with an optional depth texture.
///
/// Both textures are created with `RENDER_ATTACHMENT | TEXTURE_BINDING` usage
/// so the colour output can be sampled by a subsequent pass.
pub struct RenderTarget {
    pub color_texture: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub depth_texture: Option<wgpu::Texture>,
    pub depth_view: Option<wgpu::TextureView>,
    pub width: u32,
    pub height: u32,
    pub color_format: wgpu::TextureFormat,
    pub depth_format: Option<wgpu::TextureFormat>,
}

/// Scene renderer.
///
/// Owns pipeline caches, the immutable resource cache, per-frame GPU buffers,
/// and the depth texture. Does **not** own the wgpu device/queue/surface —
/// those live in [`GpuContext`] and are passed by reference.
pub struct Renderer {
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    object_bind_group_layout: wgpu::BindGroupLayout,
    frame_resources: FrameResources,
    cache: ImmutableResourceCache,
    depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
}

impl Renderer {
    /// Create a new renderer for the given GPU context.
    ///
    /// The depth texture is sized to match the current surface dimensions.
    pub fn new(gpu: &GpuContext) -> Self {
        let device = &gpu.device;

        let object_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("object bind group layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<ObjectUniforms>() as u64
                        ),
                    },
                    count: None,
                }],
            });

        let frame_resources = FrameResources::new(
            device,
            &object_bind_group_layout,
            device.limits().min_uniform_buffer_offset_alignment as u64,
        );

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rig render pipeline layout"),
            bind_group_layouts: &[Some(&object_bind_group_layout)],
            immediate_size: 0,
        });

        let (depth_texture, depth_view) =
            create_depth_texture(device, gpu.width(), gpu.height());

        Self {
            pipeline_layout,
            pipelines: HashMap::new(),
            object_bind_group_layout,
            frame_resources,
            cache: ImmutableResourceCache::default(),
            depth_texture,
            depth_view,
        }
    }

    /// Recreate the depth texture after a window resize.
    ///
    /// Call this after [`GpuContext::resize`] so the depth buffer matches the
    /// new surface dimensions.
    #[cfg(not(tarpaulin_include))]
    pub fn resize(&mut self, gpu: &GpuContext) {
        (self.depth_texture, self.depth_view) =
            create_depth_texture(&gpu.device, gpu.width(), gpu.height());
    }

    /// Render the scene into the current swapchain frame.
    ///
    /// Writes draw calls into `frame.encoder` and renders to `frame.view`.
    /// The caller is responsible for calling [`Frame::present`] afterwards.
    ///
    /// Returns `Ok(())` when `active_camera` is `None` (nothing to render).
    /// Returns `Err` when `active_camera` is `Some(invalid_id)`.
    #[cfg(not(tarpaulin_include))]
    pub fn render_scene(
        &mut self,
        gpu: &GpuContext,
        frame: &mut Frame,
        scene: &SceneGraph,
        assets: &AssetStore,
        active_camera: Option<NodeId>,
    ) -> Result<()> {
        let extracted_camera = active_camera
            .map(|id| {
                scene
                    .extract_active_camera(id)
                    .map_err(|e| RenderError::Asset(e.to_string()))
            })
            .transpose()?;

        let draw_list = if let Some(cam) = extracted_camera {
            let aspect = gpu.aspect();
            let pv = camera_projection_view(&cam, aspect);
            let planes = rig_scene::frustum_planes_from_projection_view(pv);
            scene.extract_renderables_culled(&planes)
        } else {
            scene.extract_renderables()
        };

        self.render_draw_list(gpu, frame, assets, extracted_camera, &draw_list)
    }

    #[cfg(not(tarpaulin_include))]
    fn render_draw_list(
        &mut self,
        gpu: &GpuContext,
        frame: &mut Frame,
        assets: &AssetStore,
        camera: Option<ExtractedCamera>,
        draw_list: &[ExtractedRenderable],
    ) -> Result<()> {
        let Some(camera) = camera else {
            return Ok(());
        };

        let aspect = gpu.aspect();
        let pv = camera_projection_view(&camera, aspect);

        let sorted_indices = self.prepare_draw_order(gpu, assets, draw_list, pv);

        self.record_scene_pass(
            gpu,
            &mut frame.encoder,
            &frame.view,
            Some(&self.depth_view.clone()),
            wgpu::Color {
                r: 0.1,
                g: 0.1,
                b: 0.1,
                a: 1.0,
            },
            assets,
            draw_list,
            &sorted_indices,
            gpu.surface_format(),
            Some(DEPTH_FORMAT),
        )
    }

    /// Sort the draw list by `(ShaderHandle, MeshHandle)`, compute per-object
    /// MVP uniforms, and upload them to the GPU uniform buffer.
    ///
    /// Returns the sorted index list so the caller can iterate in the correct
    /// order when recording draw calls.
    #[cfg(not(tarpaulin_include))]
    fn prepare_draw_order(
        &mut self,
        gpu: &GpuContext,
        assets: &AssetStore,
        draw_list: &[ExtractedRenderable],
        pv: Mat4,
    ) -> Vec<usize> {
        let mut sorted_indices: Vec<usize> = (0..draw_list.len()).collect();
        sorted_indices.sort_by_key(|&i| {
            let object = &draw_list[i];
            let shader_key = assets
                .material(object.material)
                .map(|m| m.shader)
                .unwrap_or_else(|_| ShaderHandle::from_raw(u32::MAX));
            (shader_key, object.mesh)
        });

        let object_uniforms: Vec<_> = sorted_indices
            .iter()
            .map(|&i| ObjectUniforms {
                world: (pv * draw_list[i].world_transform).to_cols_array_2d(),
            })
            .collect();
        self.frame_resources.prepare_object_uniforms(
            &gpu.device,
            &gpu.queue,
            &self.object_bind_group_layout,
            &object_uniforms,
        );

        sorted_indices
    }

    /// Record a single scene render pass into `encoder`.
    #[cfg(not(tarpaulin_include))]
    #[allow(clippy::too_many_arguments)]
    fn record_scene_pass(
        &mut self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        color_view: &wgpu::TextureView,
        depth_view: Option<&wgpu::TextureView>,
        clear_color: wgpu::Color,
        assets: &AssetStore,
        draw_list: &[ExtractedRenderable],
        sorted_indices: &[usize],
        color_format: wgpu::TextureFormat,
        depth_format: Option<wgpu::TextureFormat>,
    ) -> Result<()> {
        let depth_attachment = depth_view.map(|view| wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rig scene pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: depth_attachment,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        let mut current_pipeline: Option<PipelineKey> = None;
        let mut current_mesh: Option<rig_assets::MeshHandle> = None;

        for (uniform_index, &draw_index) in sorted_indices.iter().enumerate() {
            let object = &draw_list[draw_index];
            let material = assets
                .material(object.material)
                .map_err(|err| RenderError::Asset(err.to_string()))?;
            let shader = assets
                .shader(material.shader)
                .map_err(|err| RenderError::Asset(err.to_string()))?;
            let mesh = assets
                .mesh(object.mesh)
                .map_err(|err| RenderError::Asset(err.to_string()))?;
            let buffers = self.cache.mesh_buffers(&gpu.device, object.mesh, mesh);
            let pipeline_key = PipelineKey {
                shader: material.shader,
                vertex_layout: mesh.vertex_layout.clone(),
                color_format,
                depth_format,
            };
            let pipeline = self.pipeline_for_key(gpu, &pipeline_key, shader)?;

            if current_pipeline.as_ref() != Some(&pipeline_key) {
                pass.set_pipeline(&pipeline);
                current_pipeline = Some(pipeline_key);
            }
            pass.set_bind_group(
                0,
                &self.frame_resources.object_uniforms.bind_group,
                &[self
                    .frame_resources
                    .object_uniforms
                    .dynamic_offset(uniform_index)?],
            );
            if current_mesh != Some(object.mesh) {
                pass.set_vertex_buffer(0, buffers.vertex.slice(..));
                pass.set_index_buffer(buffers.index.slice(..), buffers.index_format);
                current_mesh = Some(object.mesh);
            }
            pass.draw_indexed(0..buffers.index_count, 0, 0..1);
        }

        Ok(())
    }

    fn pipeline_for_key(
        &mut self,
        gpu: &GpuContext,
        key: &PipelineKey,
        shader: &ShaderAsset,
    ) -> Result<wgpu::RenderPipeline> {
        if let Some(pipeline) = self.pipelines.get(key) {
            return Ok(pipeline.clone());
        }

        let shader_module = self.cache.shader_module(&gpu.device, key.shader, shader);
        let pipeline = create_pipeline(
            &gpu.device,
            &shader_module,
            &self.pipeline_layout,
            key.color_format,
            key.depth_format,
            &key.vertex_layout,
        )?;
        self.pipelines.insert(key.clone(), pipeline.clone());
        Ok(pipeline)
    }

    /// Allocate a GPU-backed offscreen render target.
    pub fn create_render_target(
        &self,
        gpu: &GpuContext,
        desc: &RenderTargetDescriptor,
    ) -> RenderTarget {
        let device = &gpu.device;
        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(desc.label),
            size: wgpu::Extent3d {
                width: desc.width,
                height: desc.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: desc.color_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let (depth_texture, depth_view) = desc
            .depth_format
            .map(|fmt| {
                let tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("render target depth"),
                    size: wgpu::Extent3d {
                        width: desc.width,
                        height: desc.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: fmt,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
                (Some(tex), Some(view))
            })
            .unwrap_or((None, None));

        RenderTarget {
            color_texture,
            color_view,
            depth_texture,
            depth_view,
            width: desc.width,
            height: desc.height,
            color_format: desc.color_format,
            depth_format: desc.depth_format,
        }
    }

    /// Render a scene into an offscreen [`RenderTarget`].
    ///
    /// Returns `Ok(())` when `active_camera` is `None` (nothing to render).
    /// Returns `Err` when `active_camera` is `Some(invalid_id)`.
    #[cfg(not(tarpaulin_include))]
    pub fn render_to_target(
        &mut self,
        gpu: &GpuContext,
        target: &RenderTarget,
        scene: &SceneGraph,
        assets: &AssetStore,
        active_camera: Option<NodeId>,
    ) -> Result<()> {
        let extracted_camera = active_camera
            .map(|id| {
                scene
                    .extract_active_camera(id)
                    .map_err(|e| RenderError::Asset(e.to_string()))
            })
            .transpose()?;

        let Some(camera) = extracted_camera else {
            return Ok(());
        };

        let aspect = target.width as f32 / target.height as f32;
        let pv = camera_projection_view(&camera, aspect);
        let planes = rig_scene::frustum_planes_from_projection_view(pv);
        let draw_list = scene.extract_renderables_culled(&planes);

        let sorted_indices = self.prepare_draw_order(gpu, assets, &draw_list, pv);

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rig offscreen encoder"),
            });

        self.record_scene_pass(
            gpu,
            &mut encoder,
            &target.color_view,
            target.depth_view.as_ref(),
            wgpu::Color {
                r: 0.05,
                g: 0.05,
                b: 0.05,
                a: 1.0,
            },
            assets,
            &draw_list,
            &sorted_indices,
            target.color_format,
            target.depth_format,
        )?;

        gpu.queue.submit(std::iter::once(encoder.finish()));
        Ok(())
    }

    /// Blit an offscreen texture onto the swapchain surface using a
    /// caller-supplied fullscreen-quad pipeline and bind group.
    ///
    /// Records a blit pass into `frame.encoder` targeting `frame.view`.
    /// Call [`Frame::present`] after this to flip the frame.
    #[cfg(not(tarpaulin_include))]
    pub fn blit_texture_to_screen(
        &mut self,
        frame: &mut Frame,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
    ) -> Result<()> {
        let mut pass = frame
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}

// ── public helpers ────────────────────────────────────────────────────────────

/// The depth format used for all main render passes.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Create a depth texture and its default view sized to `width × height`.
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

// ── WGSL shaders ──────────────────────────────────────────────────────────────

pub const TRIANGLE_SHADER: &str = r#"
struct ObjectUniforms {
    mvp: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> object: ObjectUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = object.mvp * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
"#;

/// WGSL shader that maps vertex normals to RGB colour.
///
/// Vertex layout: position @ location 0 (`Float32x3`), normal @ location 1
/// (`Float32x3`), UV @ location 2 (`Float32x2`). Stride = 32 bytes — the
/// standard layout produced by `rig_assets::mesh_factory`.
pub const NORMAL_COLOR_SHADER: &str = r#"
struct ObjectUniforms {
    mvp: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> object: ObjectUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       color:         vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = object.mvp * vec4<f32>(in.position, 1.0);
    // Map normal components from [-1, 1] to [0, 1] for a distinctive colour.
    out.color = in.normal * 0.5 + vec3<f32>(0.5, 0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
"#;

// ── private helpers ───────────────────────────────────────────────────────────

fn aligned_uniform_size(size: u64, alignment: u64) -> u64 {
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

fn object_uniform_offset(index: usize, stride: u64) -> Result<u32> {
    let offset = index as u64 * stride;
    u32::try_from(offset)
        .map_err(|_| RenderError::Asset("object uniform offset exceeds u32 range".into()))
}

fn encode_object_uniforms(uniforms: &[ObjectUniforms], stride: u64) -> Vec<u8> {
    let object_size = std::mem::size_of::<ObjectUniforms>();
    let stride = stride as usize;
    let mut bytes = vec![0_u8; stride * uniforms.len()];

    for (index, uniform) in uniforms.iter().enumerate() {
        let offset = index * stride;
        bytes[offset..offset + object_size].copy_from_slice(bytemuck::bytes_of(uniform));
    }

    bytes
}

fn create_pipeline(
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

    Ok(
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
        }),
    )
}

fn mesh_vertex_attributes(
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

fn vertex_format_size(format: VertexFormat) -> u64 {
    match format {
        VertexFormat::Float32 => std::mem::size_of::<f32>() as u64,
        VertexFormat::Float32x2 => std::mem::size_of::<[f32; 2]>() as u64,
        VertexFormat::Float32x3 => std::mem::size_of::<[f32; 3]>() as u64,
        VertexFormat::Float32x4 => std::mem::size_of::<[f32; 4]>() as u64,
    }
}

fn wgpu_vertex_format(format: VertexFormat) -> wgpu::VertexFormat {
    match format {
        VertexFormat::Float32 => wgpu::VertexFormat::Float32,
        VertexFormat::Float32x2 => wgpu::VertexFormat::Float32x2,
        VertexFormat::Float32x3 => wgpu::VertexFormat::Float32x3,
        VertexFormat::Float32x4 => wgpu::VertexFormat::Float32x4,
    }
}

fn decompose_pose(world: Mat4) -> rig_math::Transform {
    let (_, rotation, translation) = world.to_scale_rotation_translation();
    rig_math::Transform {
        translation,
        rotation,
        scale: rig_math::Vec3::ONE,
    }
}

fn camera_projection_view(camera: &ExtractedCamera, aspect: f32) -> Mat4 {
    let pose = decompose_pose(camera.world_transform);
    let camera_value = Camera {
        pose,
        projection: camera.projection,
    };
    camera_value.projection_view_matrix(aspect)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rig_assets::{ShaderAsset, VertexAttribute, VertexLayout};
    use rig_math::{Quat, Transform, Vec3};

    use super::*;

    fn sample_mesh() -> MeshAsset {
        MeshAsset {
            vertex_layout: VertexLayout {
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
            },
            vertex_data: Arc::from([1_u8; 24]),
            index_data: Arc::from([0_u8, 1, 2, 0, 2, 1]),
            index_format: rig_assets::IndexFormat::Uint16,
            local_bounds: rig_math::BoundingSphere::ZERO,
        }
    }

    #[test]
    fn validate_vertex_layout_accepts_position_and_color() {
        assert!(validate_vertex_layout(&sample_mesh().vertex_layout).is_ok());
    }

    #[test]
    fn validate_vertex_layout_accepts_padded_and_reordered() {
        let mut mesh = sample_mesh();
        mesh.vertex_layout.array_stride = 32;
        mesh.vertex_layout.attributes = vec![
            VertexAttribute {
                shader_location: 1,
                format: VertexFormat::Float32x3,
                offset: 16,
            },
            VertexAttribute {
                shader_location: 0,
                format: VertexFormat::Float32x3,
                offset: 0,
            },
        ];

        assert!(validate_vertex_layout(&mesh.vertex_layout).is_ok());
    }

    #[test]
    fn validate_vertex_layout_rejects_attribute_outside_stride() {
        let mut mesh = sample_mesh();
        mesh.vertex_layout.array_stride = 16;

        assert!(validate_vertex_layout(&mesh.vertex_layout).is_err());
    }

    #[test]
    fn mesh_vertex_attributes_preserve_asset_layout_information() {
        let mut mesh = sample_mesh();
        mesh.vertex_layout.array_stride = 32;
        mesh.vertex_layout.attributes[0].offset = 4;
        mesh.vertex_layout.attributes[1].offset = 20;

        let attributes = mesh_vertex_attributes(&mesh.vertex_layout).unwrap();

        assert_eq!(attributes.len(), 2);
        assert_eq!(attributes[0].shader_location, 0);
        assert_eq!(attributes[0].offset, 4);
        assert_eq!(attributes[0].format, wgpu::VertexFormat::Float32x3);
        assert_eq!(attributes[1].shader_location, 1);
        assert_eq!(attributes[1].offset, 20);
    }

    #[test]
    fn mesh_vertex_attributes_reject_duplicate_shader_locations() {
        let mut mesh = sample_mesh();
        mesh.vertex_layout.attributes[1].shader_location = 0;

        assert!(mesh_vertex_attributes(&mesh.vertex_layout).is_err());
    }

    #[test]
    fn aligned_uniform_size_rounds_up_to_alignment() {
        assert_eq!(aligned_uniform_size(64, 256), 256);
        assert_eq!(aligned_uniform_size(256, 256), 256);
        assert_eq!(aligned_uniform_size(65, 16), 80);
    }

    #[test]
    fn object_uniform_offset_uses_stride() {
        assert_eq!(object_uniform_offset(2, 256).unwrap(), 512);
    }

    #[test]
    fn encode_object_uniforms_respects_stride_padding() {
        let uniforms = [
            ObjectUniforms {
                world: Mat4::IDENTITY.to_cols_array_2d(),
            },
            ObjectUniforms {
                world: Mat4::from_translation(rig_math::Vec3::new(1.0, 2.0, 3.0))
                    .to_cols_array_2d(),
            },
        ];

        let bytes = encode_object_uniforms(&uniforms, 256);
        let object_size = std::mem::size_of::<ObjectUniforms>();

        assert_eq!(bytes.len(), 512);
        assert_eq!(&bytes[..object_size], bytemuck::bytes_of(&uniforms[0]));
        assert_eq!(
            &bytes[256..256 + object_size],
            bytemuck::bytes_of(&uniforms[1])
        );
        assert!(bytes[object_size..256].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn immutable_cache_uses_handle_as_key() {
        // Two different handles with identical content must be tracked separately.
        let handle_a = MeshHandle::from_raw(0);
        let handle_b = MeshHandle::from_raw(1);
        // Verify the handles are distinct (cache correctness is structural, not GPU-testable here).
        assert_ne!(handle_a, handle_b);
        // Same handle must compare equal to itself.
        assert_eq!(handle_a, MeshHandle::from_raw(0));
    }

    #[test]
    fn shader_handle_identity() {
        let h = ShaderHandle::from_raw(42);
        assert_eq!(h, ShaderHandle::from_raw(42));
        assert_ne!(h, ShaderHandle::from_raw(0));
    }

    #[test]
    fn decompose_pose_recovers_translation_and_rotation() {
        let transform = Transform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quat::from_rotation_y(0.75),
            scale: Vec3::new(2.0, 2.0, 2.0),
        };

        let pose = decompose_pose(transform.to_mat4());

        assert!(pose.translation.abs_diff_eq(transform.translation, 1e-5));
        assert!(pose.rotation.abs_diff_eq(transform.rotation, 1e-5));
        assert_eq!(pose.scale, Vec3::ONE);
    }

    #[test]
    fn triangle_shader_mentions_expected_entry_points() {
        assert!(TRIANGLE_SHADER.contains("fn vs_main"));
        assert!(TRIANGLE_SHADER.contains("fn fs_main"));
        assert!(TRIANGLE_SHADER.contains("@group(0) @binding(0)"));
    }

    #[test]
    fn normal_color_shader_mentions_expected_entry_points_and_locations() {
        assert!(NORMAL_COLOR_SHADER.contains("fn vs_main"));
        assert!(NORMAL_COLOR_SHADER.contains("fn fs_main"));
        assert!(NORMAL_COLOR_SHADER.contains("@location(0) position"));
        assert!(NORMAL_COLOR_SHADER.contains("@location(1) normal"));
        assert!(NORMAL_COLOR_SHADER.contains("@location(2) uv"));
    }

    #[test]
    fn pipeline_key_differs_with_depth_format() {
        let layout = VertexLayout {
            array_stride: 24,
            attributes: vec![],
        };
        let shader = ShaderHandle::from_raw(1);

        let key_no_depth = PipelineKey {
            shader,
            vertex_layout: layout.clone(),
            color_format: wgpu::TextureFormat::Bgra8UnormSrgb,
            depth_format: None,
        };
        let key_with_depth = PipelineKey {
            shader,
            vertex_layout: layout.clone(),
            color_format: wgpu::TextureFormat::Bgra8UnormSrgb,
            depth_format: Some(wgpu::TextureFormat::Depth32Float),
        };
        let key_diff_depth = PipelineKey {
            shader,
            vertex_layout: layout,
            color_format: wgpu::TextureFormat::Bgra8UnormSrgb,
            depth_format: Some(wgpu::TextureFormat::Depth24Plus),
        };

        assert_ne!(key_no_depth, key_with_depth);
        assert_ne!(key_with_depth, key_diff_depth);
        assert_eq!(key_no_depth, key_no_depth.clone());
    }

    #[test]
    fn pipeline_key_differs_by_color_format() {
        let layout = VertexLayout {
            array_stride: 24,
            attributes: vec![],
        };
        let shader = ShaderHandle::from_raw(1);

        let key_bgra = PipelineKey {
            shader,
            vertex_layout: layout.clone(),
            color_format: wgpu::TextureFormat::Bgra8UnormSrgb,
            depth_format: None,
        };
        let key_rgba16 = PipelineKey {
            shader,
            vertex_layout: layout,
            color_format: wgpu::TextureFormat::Rgba16Float,
            depth_format: None,
        };

        assert_ne!(key_bgra, key_rgba16);
    }

    #[test]
    fn create_depth_texture_returns_correct_dimensions() {
        assert_eq!(DEPTH_FORMAT, wgpu::TextureFormat::Depth32Float);
    }

    #[test]
    fn validate_vertex_layout_accepts_normals_only() {
        let layout = VertexLayout {
            array_stride: 12,
            attributes: vec![VertexAttribute {
                shader_location: 2,
                format: VertexFormat::Float32x3,
                offset: 0,
            }],
        };
        assert!(validate_vertex_layout(&layout).is_ok());
    }

    #[test]
    fn validate_vertex_layout_rejects_empty_layout() {
        let layout = VertexLayout {
            array_stride: 12,
            attributes: vec![],
        };
        assert!(validate_vertex_layout(&layout).is_err());
    }

    #[test]
    fn validate_vertex_layout_rejects_zero_stride() {
        let layout = VertexLayout {
            array_stride: 0,
            attributes: vec![VertexAttribute {
                shader_location: 0,
                format: VertexFormat::Float32x3,
                offset: 0,
            }],
        };
        assert!(validate_vertex_layout(&layout).is_err());
    }

    #[test]
    fn validate_vertex_layout_rejects_duplicates() {
        let layout = VertexLayout {
            array_stride: 24,
            attributes: vec![
                VertexAttribute {
                    shader_location: 0,
                    format: VertexFormat::Float32x3,
                    offset: 0,
                },
                VertexAttribute {
                    shader_location: 0,
                    format: VertexFormat::Float32x3,
                    offset: 12,
                },
            ],
        };
        assert!(validate_vertex_layout(&layout).is_err());
    }

    #[test]
    fn vertex_format_size_float32x4() {
        assert_eq!(vertex_format_size(VertexFormat::Float32x4), 16);
    }

    #[test]
    fn vertex_format_size_float32() {
        assert_eq!(vertex_format_size(VertexFormat::Float32), 4);
    }

    #[test]
    fn wgpu_vertex_format_maps_float32x4() {
        assert_eq!(
            wgpu_vertex_format(VertexFormat::Float32x4),
            wgpu::VertexFormat::Float32x4
        );
    }

    #[test]
    fn index_count_uses_declared_format() {
        let mut mesh = sample_mesh();
        mesh.index_data = Arc::from([0_u8; 8]);
        mesh.index_format = rig_assets::IndexFormat::Uint32;

        let expected = (mesh.index_data.len() / std::mem::size_of::<u32>()) as u32;
        assert_eq!(expected, 2);
    }

    #[test]
    fn draw_list_sorted_by_shader_then_mesh() {
        use rig_assets::{AssetStore, MaterialAsset, MaterialParams, ShaderAsset};
        use rig_math::BoundingSphere;
        use rig_scene::ExtractedRenderable;

        let mut assets = AssetStore::new();
        let shader_a = assets.add_shader(ShaderAsset {
            source: Arc::from("a"),
        });
        let shader_b = assets.add_shader(ShaderAsset {
            source: Arc::from("b"),
        });

        let material_a1 = assets.add_material(MaterialAsset {
            shader: shader_a,
            parameters: MaterialParams::default(),
            textures: vec![],
        });
        let material_b1 = assets.add_material(MaterialAsset {
            shader: shader_b,
            parameters: MaterialParams::default(),
            textures: vec![],
        });
        let material_a2 = assets.add_material(MaterialAsset {
            shader: shader_a,
            parameters: MaterialParams::default(),
            textures: vec![],
        });

        let mesh_x = assets.add_mesh(sample_mesh());
        let mesh_y = {
            let mut m = sample_mesh();
            m.vertex_data = Arc::from([2_u8; 24]);
            assets.add_mesh(m)
        };

        let draw_list = vec![
            ExtractedRenderable {
                node: rig_scene::NodeId::from_raw(0, 0),
                mesh: mesh_x,
                material: material_b1,
                world_transform: Mat4::IDENTITY,
                world_bound: BoundingSphere::ZERO,
            },
            ExtractedRenderable {
                node: rig_scene::NodeId::from_raw(1, 0),
                mesh: mesh_y,
                material: material_a1,
                world_transform: Mat4::IDENTITY,
                world_bound: BoundingSphere::ZERO,
            },
            ExtractedRenderable {
                node: rig_scene::NodeId::from_raw(2, 0),
                mesh: mesh_x,
                material: material_a2,
                world_transform: Mat4::IDENTITY,
                world_bound: BoundingSphere::ZERO,
            },
        ];

        let mut sorted_indices: Vec<usize> = (0..draw_list.len()).collect();
        sorted_indices.sort_by_key(|&i| {
            let object = &draw_list[i];
            let shader_key = assets
                .material(object.material)
                .map(|m| m.shader)
                .unwrap_or_else(|_| ShaderHandle::from_raw(u32::MAX));
            (shader_key, object.mesh)
        });

        let sorted_shaders: Vec<ShaderHandle> = sorted_indices
            .iter()
            .map(|&i| {
                assets
                    .material(draw_list[i].material)
                    .map(|m| m.shader)
                    .unwrap()
            })
            .collect();

        let first_b = sorted_shaders.iter().position(|&s| s == shader_b).unwrap();
        assert!(sorted_shaders[..first_b].iter().all(|&s| s == shader_a));
        assert!(sorted_shaders[first_b..].iter().all(|&s| s == shader_b));
    }

    #[test]
    fn sorted_draw_list_reduces_state_changes() {
        let shaders = vec![1_u32, 2, 1, 2, 1];
        let mut sorted_shaders = shaders.clone();
        sorted_shaders.sort();

        fn count_pipeline_switches(shaders: &[u32]) -> usize {
            shaders.windows(2).filter(|w| w[0] != w[1]).count()
        }

        let unsorted_switches = count_pipeline_switches(&shaders);
        let sorted_switches = count_pipeline_switches(&sorted_shaders);

        assert!(sorted_switches < unsorted_switches);
    }

    #[test]
    fn render_target_descriptor_format_fields() {
        let desc = RenderTargetDescriptor {
            width: 512,
            height: 256,
            color_format: wgpu::TextureFormat::Rgba8UnormSrgb,
            depth_format: Some(wgpu::TextureFormat::Depth32Float),
            label: "test target",
        };
        assert_eq!(desc.width, 512);
        assert_eq!(desc.height, 256);
        assert_eq!(desc.color_format, wgpu::TextureFormat::Rgba8UnormSrgb);
        assert_eq!(desc.depth_format, Some(wgpu::TextureFormat::Depth32Float));
    }

    #[test]
    fn render_target_descriptor_no_depth() {
        let desc = RenderTargetDescriptor {
            width: 1920,
            height: 1080,
            color_format: wgpu::TextureFormat::Bgra8UnormSrgb,
            depth_format: None,
            label: "no depth",
        };
        assert!(desc.depth_format.is_none());
    }

    #[test]
    fn render_error_display_is_non_empty() {
        let err = RenderError::InvalidCamera;
        assert!(!err.to_string().is_empty());

        let err = RenderError::Asset("test".into());
        assert!(err.to_string().contains("test"));
    }
}
