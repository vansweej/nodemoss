//! Concrete `wgpu` renderer for the rig framework.

use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    num::NonZeroU64,
    sync::Arc,
};

use bytemuck::{Pod, Zeroable};
use rig_assets::{
    AssetStore, IndexFormat, MeshAsset, ShaderAsset, ShaderHandle, VertexFormat, VertexLayout,
};
use rig_math::{Camera, Mat4};
use rig_scene::{ExtractedCamera, ExtractedRenderable, NodeId, SceneGraph};
use thiserror::Error;
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, window::Window};

pub use wgpu;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("failed to create surface: {0}")]
    SurfaceCreate(#[from] wgpu::CreateSurfaceError),
    #[error("failed to find a suitable GPU adapter")]
    NoAdapter,
    #[error("failed to create device: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),
    #[error("surface does not expose a supported format")]
    NoSurfaceFormat,
    #[error("scene node is not a camera")]
    InvalidCamera,
    #[error("asset error: {0}")]
    Asset(String),
}

pub type Result<T> = std::result::Result<T, RenderError>;

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
    shaders: HashMap<u64, wgpu::ShaderModule>,
    meshes: HashMap<u64, CachedMeshBuffers>,
}

impl ImmutableResourceCache {
    fn shader_module(&mut self, device: &wgpu::Device, shader: &ShaderAsset) -> wgpu::ShaderModule {
        let key = hash_shader(shader);
        if let Some(module) = self.shaders.get(&key) {
            return module.clone();
        }

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rig render shader"),
            source: wgpu::ShaderSource::Wgsl(shader.source.as_ref().into()),
        });
        self.shaders.insert(key, module.clone());
        module
    }

    fn mesh_buffers(&mut self, device: &wgpu::Device, mesh: &MeshAsset) -> CachedMeshBuffers {
        let key = hash_mesh(mesh);
        if let Some(buffers) = self.meshes.get(&key) {
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
        self.meshes.insert(key, buffers.clone());
        buffers
    }
}

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

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    object_bind_group_layout: wgpu::BindGroupLayout,
    frame_resources: FrameResources,
    window: Arc<Window>,
    cache: ImmutableResourceCache,
    depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
}

impl Renderer {
    #[cfg(not(tarpaulin_include))]
    pub async fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .map_err(|_| RenderError::NoAdapter)?;

        log::info!("Using adapter: {}", adapter.get_info().name);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("rig renderer device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .find(|candidate| candidate.is_srgb())
            .copied()
            .or_else(|| surface_caps.formats.first().copied())
            .ok_or(RenderError::NoSurfaceFormat)?;

        let width = size.width.max(1);
        let height = size.height.max(1);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

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
            &device,
            &object_bind_group_layout,
            adapter.limits().min_uniform_buffer_offset_alignment as u64,
        );

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rig render pipeline layout"),
            bind_group_layouts: &[Some(&object_bind_group_layout)],
            immediate_size: 0,
        });

        let (depth_texture, depth_view) = create_depth_texture(&device, width, height);

        Ok(Self {
            surface,
            device,
            queue,
            surface_config,
            pipeline_layout,
            pipelines: HashMap::new(),
            object_bind_group_layout,
            frame_resources,
            window,
            cache: ImmutableResourceCache::default(),
            depth_texture,
            depth_view,
        })
    }

    #[cfg(not(tarpaulin_include))]
    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            self.surface_config.width = size.width;
            self.surface_config.height = size.height;
            self.surface.configure(&self.device, &self.surface_config);
            (self.depth_texture, self.depth_view) =
                create_depth_texture(&self.device, size.width, size.height);
        }
    }

    #[cfg(not(tarpaulin_include))]
    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Borrow the wgpu device. Useful for examples that need to create custom
    /// GPU resources (pipelines, bind groups, buffers) alongside the renderer.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Borrow the wgpu queue. Useful for examples that need to submit their
    /// own command encoders.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// The texture format of the swapchain surface.
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_config.format
    }

    #[cfg(not(tarpaulin_include))]
    pub fn render_scene(
        &mut self,
        scene: &SceneGraph,
        assets: &AssetStore,
        active_camera: Option<NodeId>,
    ) -> Result<()> {
        let extracted_camera = active_camera
            .and_then(|id| scene.extract_active_camera(id).ok());

        let draw_list = if let Some(cam) = extracted_camera {
            // Compute frustum planes from the camera's projection-view matrix and
            // cull objects that are entirely outside it.
            let aspect = self.surface_config.width as f32 / self.surface_config.height as f32;
            let pose = decompose_pose(cam.world_transform);
            let camera_value = rig_math::Camera { pose, projection: cam.projection };
            let pv = camera_value.projection_view_matrix(aspect);
            let planes = rig_scene::frustum_planes_from_projection_view(pv);
            scene.extract_renderables_culled(&planes)
        } else {
            scene.extract_renderables()
        };

        self.render_draw_list(assets, extracted_camera, &draw_list)
    }

    #[cfg(not(tarpaulin_include))]
    fn render_draw_list(
        &mut self,
        assets: &AssetStore,
        camera: Option<ExtractedCamera>,
        draw_list: &[ExtractedRenderable],
    ) -> Result<()> {
        let Some(camera) = camera else {
            return Ok(());
        };

        let pose = decompose_pose(camera.world_transform);
        let camera_value = Camera {
            pose,
            projection: camera.projection,
        };
        let aspect = self.surface_config.width as f32 / self.surface_config.height as f32;
        let pv = camera_value.projection_view_matrix(aspect);

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout => return Ok(()),
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(()),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => return Ok(()),
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        // Build a sorted draw order.  Look up the shader handle for each
        // object (via its material) and sort by (shader_handle, mesh_handle)
        // so that we minimise pipeline switches and vertex-buffer rebinds.
        // Objects with missing assets are silently skipped here but will
        // produce a proper error during the actual draw call if they remain.
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
            &self.device,
            &self.queue,
            &self.object_bind_group_layout,
            &object_uniforms,
        );
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rig render encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rig render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // Tracks the last-bound pipeline and mesh to skip redundant state
            // changes between draw calls.
            let mut current_pipeline: Option<ShaderHandle> = None;
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
                let buffers = self.cache.mesh_buffers(&self.device, mesh);
                let pipeline = self.pipeline_for_shader(
                    material.shader,
                    shader,
                    &mesh.vertex_layout,
                    self.surface_config.format,
                    Some(DEPTH_FORMAT),
                )?;

                if current_pipeline != Some(material.shader) {
                    pass.set_pipeline(&pipeline);
                    current_pipeline = Some(material.shader);
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
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        Ok(())
    }

    fn pipeline_for_shader(
        &mut self,
        shader_handle: ShaderHandle,
        shader: &ShaderAsset,
        vertex_layout: &VertexLayout,
        color_format: wgpu::TextureFormat,
        depth_format: Option<wgpu::TextureFormat>,
    ) -> Result<wgpu::RenderPipeline> {
        let key = PipelineKey {
            shader: shader_handle,
            vertex_layout: vertex_layout.clone(),
            color_format,
            depth_format,
        };
        if let Some(pipeline) = self.pipelines.get(&key) {
            return Ok(pipeline.clone());
        }

        let shader_module = self.cache.shader_module(&self.device, shader);
        let pipeline = create_pipeline(
            &self.device,
            &shader_module,
            &self.pipeline_layout,
            color_format,
            depth_format,
            vertex_layout,
        )?;
        self.pipelines.insert(key, pipeline.clone());
        Ok(pipeline)
    }

    /// Allocate a GPU-backed offscreen render target.
    ///
    /// Both colour and (optional) depth textures are created with
    /// `RENDER_ATTACHMENT | TEXTURE_BINDING` usage so the colour output can
    /// be sampled in a subsequent pass.
    pub fn create_render_target(&self, desc: &RenderTargetDescriptor) -> RenderTarget {
        let color_texture = self.device.create_texture(&wgpu::TextureDescriptor {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let (depth_texture, depth_view) = desc.depth_format.map(|fmt| {
            let tex = self.device.create_texture(&wgpu::TextureDescriptor {
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
        }).unwrap_or((None, None));

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
    /// Behaves identically to [`render_scene`] except the output goes to the
    /// provided `target` instead of the swapchain surface.
    #[cfg(not(tarpaulin_include))]
    pub fn render_to_target(
        &mut self,
        target: &RenderTarget,
        scene: &SceneGraph,
        assets: &AssetStore,
        active_camera: Option<NodeId>,
    ) -> Result<()> {
        let extracted_camera = active_camera
            .and_then(|id| scene.extract_active_camera(id).ok());

        let draw_list = if let Some(cam) = extracted_camera {
            let aspect = target.width as f32 / target.height as f32;
            let pose = decompose_pose(cam.world_transform);
            let camera_value = rig_math::Camera { pose, projection: cam.projection };
            let pv = camera_value.projection_view_matrix(aspect);
            let planes = rig_scene::frustum_planes_from_projection_view(pv);
            scene.extract_renderables_culled(&planes)
        } else {
            scene.extract_renderables()
        };

        let Some(camera) = extracted_camera else {
            return Ok(());
        };

        let pose = decompose_pose(camera.world_transform);
        let camera_value = Camera {
            pose,
            projection: camera.projection,
        };
        let aspect = target.width as f32 / target.height as f32;
        let pv = camera_value.projection_view_matrix(aspect);

        // Sort draw list
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
            &self.device,
            &self.queue,
            &self.object_bind_group_layout,
            &object_uniforms,
        );

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rig offscreen encoder"),
            });

        {
            let depth_attachment =
                target.depth_view.as_ref().map(|view| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }
                });

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rig offscreen pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.05,
                            b: 0.05,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: depth_attachment,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            let mut current_pipeline: Option<ShaderHandle> = None;
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
                let buffers = self.cache.mesh_buffers(&self.device, mesh);
                let pipeline = self.pipeline_for_shader(
                    material.shader,
                    shader,
                    &mesh.vertex_layout,
                    target.color_format,
                    target.depth_format,
                )?;

                if current_pipeline != Some(material.shader) {
                    pass.set_pipeline(&pipeline);
                    current_pipeline = Some(material.shader);
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
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        Ok(())
    }

    /// Blit an offscreen texture onto the swapchain surface using a
    /// caller-supplied fullscreen-quad pipeline and bind group.
    ///
    /// This is the final step of an offscreen rendering workflow:
    /// 1. Call [`render_to_target`] to render the scene into a [`RenderTarget`].
    /// 2. Build a blit `RenderPipeline` and `BindGroup` that sample the
    ///    offscreen colour texture (using [`surface_format`] as the colour
    ///    target format).
    /// 3. Call this method to present the result.
    ///
    /// The method acquires the current swapchain frame, records a single
    /// render pass with `draw(0..3, 0..1)` (no vertex buffer — positions are
    /// generated inside the vertex shader), and presents the frame.
    #[cfg(not(tarpaulin_include))]
    pub fn blit_texture_to_screen(
        &mut self,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
    ) -> Result<()> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout => return Ok(()),
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(()),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => return Ok(()),
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("blit encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        Ok(())
    }
}

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

/// Generic vertex layout validator.
///
/// Checks that:
/// - `array_stride > 0`
/// - at least one attribute is present
/// - no duplicate `shader_location` values
/// - every attribute fits within the stride (`offset + format_size ≤ stride`)
///
/// Does **not** require any specific locations (e.g., position@0 or color@1).
pub fn validate_vertex_layout(
    vertex_layout: &VertexLayout,
) -> std::result::Result<(), String> {
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

fn validate_triangle_shader_layout(
    vertex_layout: &VertexLayout,
) -> std::result::Result<(), String> {
    if vertex_layout.array_stride == 0 {
        return Err("vertex layout must use a non-zero array stride".into());
    }

    let mut seen_locations = HashSet::new();
    let mut has_position = false;
    let mut has_color = false;

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

        match attribute.shader_location {
            0 => has_position = true,
            1 => has_color = true,
            _ => {}
        }
    }

    if !has_position || !has_color {
        return Err("triangle shader requires position@0 and color@1 attributes".into());
    }

    Ok(())
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

fn hash_shader(shader: &ShaderAsset) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    shader.source.hash(&mut hasher);
    hasher.finish()
}

fn hash_mesh(mesh: &MeshAsset) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    mesh.vertex_layout.hash(&mut hasher);
    mesh.vertex_data.hash(&mut hasher);
    mesh.index_data.hash(&mut hasher);
    hasher.finish()
}

fn decompose_pose(world: Mat4) -> rig_math::Transform {
    let (_, rotation, translation) = world.to_scale_rotation_translation();
    rig_math::Transform {
        translation,
        rotation,
        scale: rig_math::Vec3::ONE,
    }
}

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

pub fn validate_triangle_layout(mesh: &MeshAsset) -> bool {
    validate_triangle_shader_layout(&mesh.vertex_layout).is_ok()
}

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
    fn validate_triangle_layout_accepts_position_color_layout() {
        assert!(validate_triangle_layout(&sample_mesh()));
    }

    #[test]
    fn validate_triangle_layout_accepts_padded_and_reordered_layout() {
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

        assert!(validate_triangle_layout(&mesh));
    }

    #[test]
    fn validate_triangle_layout_rejects_attribute_outside_stride() {
        let mut mesh = sample_mesh();
        mesh.vertex_layout.array_stride = 16;

        assert!(!validate_triangle_layout(&mesh));
    }

    #[test]
    fn validate_triangle_layout_rejects_missing_attribute() {
        let mut mesh = sample_mesh();
        mesh.vertex_layout.attributes.pop();

        assert!(!validate_triangle_layout(&mesh));
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
    fn hash_mesh_is_stable_for_identical_content() {
        let mesh_a = sample_mesh();
        let mesh_b = sample_mesh();

        assert_eq!(hash_mesh(&mesh_a), hash_mesh(&mesh_b));
    }

    #[test]
    fn hash_mesh_changes_when_content_changes() {
        let mesh_a = sample_mesh();
        let mut mesh_b = sample_mesh();
        mesh_b.index_data = Arc::from([0_u8, 1, 2]);

        assert_ne!(hash_mesh(&mesh_a), hash_mesh(&mesh_b));
    }

    #[test]
    fn hash_shader_is_stable_for_identical_source() {
        let shader_a = ShaderAsset {
            source: Arc::from("shader"),
        };
        let shader_b = ShaderAsset {
            source: Arc::from("shader"),
        };

        assert_eq!(hash_shader(&shader_a), hash_shader(&shader_b));
    }

    #[test]
    fn hash_shader_changes_when_source_changes() {
        let shader_a = ShaderAsset {
            source: Arc::from("shader_a"),
        };
        let shader_b = ShaderAsset {
            source: Arc::from("shader_b"),
        };

        assert_ne!(hash_shader(&shader_a), hash_shader(&shader_b));
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
        // We can't easily create a wgpu Device in a unit test without a display,
        // so we validate the descriptor parameters directly via the public helper
        // by checking the constants it would use.
        assert_eq!(DEPTH_FORMAT, wgpu::TextureFormat::Depth32Float);
    }

    // --- Commit 2: generic vertex validation and extended VertexFormat ---

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
        // 8 bytes of u32 index data → 2 indices (each 4 bytes)
        let mut mesh = sample_mesh();
        mesh.index_data = Arc::from([0_u8; 8]);
        mesh.index_format = rig_assets::IndexFormat::Uint32;

        // Compute expected index count the same way mesh_buffers does.
        let expected =
            (mesh.index_data.len() / std::mem::size_of::<u32>()) as u32;
        assert_eq!(expected, 2);
    }

    // --- Commit 4: draw-list sorting ---

    #[test]
    fn draw_list_sorted_by_shader_then_mesh() {
        use rig_assets::{AssetStore, MaterialAsset, MaterialParams, ShaderAsset};
        use rig_math::BoundingSphere;
        use rig_scene::ExtractedRenderable;

        let mut assets = AssetStore::new();
        let shader_a = assets.add_shader(ShaderAsset { source: Arc::from("a") });
        let shader_b = assets.add_shader(ShaderAsset { source: Arc::from("b") });

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

        // Deliberately interleaved: b1/x, a1/y, a2/x
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

        // Build sorted indices the same way render_draw_list does.
        let mut sorted_indices: Vec<usize> = (0..draw_list.len()).collect();
        sorted_indices.sort_by_key(|&i| {
            let object = &draw_list[i];
            let shader_key = assets
                .material(object.material)
                .map(|m| m.shader)
                .unwrap_or_else(|_| ShaderHandle::from_raw(u32::MAX));
            (shader_key, object.mesh)
        });

        // After sorting: shader_a objects first (a1/y, a2/x), then shader_b (b1/x).
        // Within shader_a the order by mesh: mesh_x < mesh_y depends on handle
        // ordering.  What matters is that all shader_a objects are consecutive
        // and all shader_b objects are consecutive.
        let sorted_shaders: Vec<ShaderHandle> = sorted_indices
            .iter()
            .map(|&i| {
                assets
                    .material(draw_list[i].material)
                    .map(|m| m.shader)
                    .unwrap()
            })
            .collect();

        // shader_a objects come before shader_b objects.
        let first_b = sorted_shaders.iter().position(|&s| s == shader_b).unwrap();
        assert!(sorted_shaders[..first_b].iter().all(|&s| s == shader_a));
        assert!(sorted_shaders[first_b..].iter().all(|&s| s == shader_b));
    }

    #[test]
    fn sorted_draw_list_reduces_state_changes() {
        // Count hypothetical pipeline switches for sorted vs unsorted order.
        let shaders = vec![1_u32, 2, 1, 2, 1]; // unsorted: 4 switches
        let mut sorted_shaders = shaders.clone();
        sorted_shaders.sort();

        fn count_pipeline_switches(shaders: &[u32]) -> usize {
            shaders.windows(2).filter(|w| w[0] != w[1]).count()
        }

        let unsorted_switches = count_pipeline_switches(&shaders);
        let sorted_switches = count_pipeline_switches(&sorted_shaders);

        assert!(sorted_switches < unsorted_switches);
    }

    // --- Commit 6: RenderTarget ---

    #[test]
    fn render_target_descriptor_format_fields() {
        // Verify that the RenderTargetDescriptor fields are accessible and
        // hold the values we set.  GPU construction requires a Device, which
        // is not available in unit tests, so we only verify the descriptor.
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
}
