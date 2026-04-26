//! Scene renderer: pipeline management, draw submission, offscreen targets.

use std::collections::HashMap;
use std::num::NonZeroU64;

use rig_assets::{AssetStore, ShaderHandle};
use rig_gpu::{Frame, GpuContext};
use rig_math::Mat4;
use rig_scene::{ExtractedCamera, ExtractedLight, ExtractedRenderable, LightKind, NodeId, SceneGraph};

use crate::cache::ImmutableResourceCache;
use crate::frame::{FrameResources, ObjectUniforms};
use crate::helpers::{
    camera_projection_view, create_depth_texture, create_pipeline, decompose_pose, FrameUniforms,
    LightsBuffer, MaterialUniforms, MAX_LIGHTS, DEPTH_FORMAT,
};
use crate::pipeline::PipelineKey;
use crate::{RenderError, RenderTarget, RenderTargetDescriptor, Result};

/// Scene renderer.
pub struct Renderer {
    pub(crate) pipeline_layout: wgpu::PipelineLayout,
    pub(crate) pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    // Bind group layouts
    pub(crate) frame_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) material_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) object_bind_group_layout: wgpu::BindGroupLayout,
    // Frame-level resources
    pub(crate) frame_uniform_buffer: wgpu::Buffer,
    pub(crate) lights_buffer: wgpu::Buffer,
    // Fallback (untextured) material resources — kept alive to back the bind group
    #[allow(dead_code)]
    pub(crate) fallback_texture: wgpu::Texture,
    #[allow(dead_code)]
    pub(crate) fallback_texture_view: wgpu::TextureView,
    #[allow(dead_code)]
    pub(crate) fallback_sampler: wgpu::Sampler,
    #[allow(dead_code)]
    pub(crate) fallback_material_uniform_buffer: wgpu::Buffer,
    pub(crate) fallback_material_bind_group: wgpu::BindGroup,
    // Per-frame object uniforms
    pub(crate) frame_resources: FrameResources,
    pub(crate) cache: ImmutableResourceCache,
    pub(crate) depth_texture: wgpu::Texture,
    pub(crate) depth_view: wgpu::TextureView,
}

#[cfg(not(tarpaulin_include))]
impl Renderer {
    pub fn new(gpu: &GpuContext) -> Self {
        let device = &gpu.device;

        // ── Group 0: frame uniforms (view/proj/camera_pos) + lights ──────────
        let frame_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(std::mem::size_of::<FrameUniforms>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(std::mem::size_of::<LightsBuffer>() as u64),
                    },
                    count: None,
                },
            ],
        });

        // ── Group 1: material uniforms + texture + sampler ───────────────────
        let material_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(std::mem::size_of::<MaterialUniforms>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // ── Group 2: per-object uniforms (world matrix, dynamic offset) ───────
        let object_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("object bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(std::mem::size_of::<ObjectUniforms>() as u64),
                },
                count: None,
            }],
        });

        // ── Frame uniform buffer ──────────────────────────────────────────────
        let frame_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame uniform buffer"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ── Lights buffer ─────────────────────────────────────────────────────
        let lights_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lights uniform buffer"),
            size: std::mem::size_of::<LightsBuffer>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ── Fallback 1×1 white RGBA texture ──────────────────────────────────
        let fallback_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fallback white texture"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // Write white pixels immediately (we have queue access)
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &fallback_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255_u8, 255, 255, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let fallback_texture_view = fallback_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let fallback_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fallback sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // ── Fallback material uniform buffer (white, flags=0) ─────────────────
        let fallback_material_uniforms = MaterialUniforms {
            base_color: [1.0, 1.0, 1.0, 1.0],
            flags: 0,
            _pad: [0; 3],
        };
        let fallback_material_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fallback material uniform buffer"),
            size: std::mem::size_of::<MaterialUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(
            &fallback_material_uniform_buffer,
            0,
            bytemuck::bytes_of(&fallback_material_uniforms),
        );

        // ── Fallback material bind group (group 1) ────────────────────────────
        let fallback_material_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fallback material bind group"),
            layout: &material_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: fallback_material_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&fallback_texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&fallback_sampler),
                },
            ],
        });

        // ── Pipeline layout: all three groups ─────────────────────────────────
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rig render pipeline layout"),
            bind_group_layouts: &[
                Some(&frame_bind_group_layout),
                Some(&material_bind_group_layout),
                Some(&object_bind_group_layout),
            ],
            immediate_size: 0,
        });

        let frame_resources = FrameResources::new(
            device,
            &object_bind_group_layout,
            device.limits().min_uniform_buffer_offset_alignment as u64,
        );
        let (depth_texture, depth_view) = create_depth_texture(device, gpu.width(), gpu.height());

        Self {
            pipeline_layout,
            pipelines: HashMap::new(),
            frame_bind_group_layout,
            material_bind_group_layout,
            object_bind_group_layout,
            frame_uniform_buffer,
            lights_buffer,
            fallback_texture,
            fallback_texture_view,
            fallback_sampler,
            fallback_material_uniform_buffer,
            fallback_material_bind_group,
            frame_resources,
            cache: ImmutableResourceCache::default(),
            depth_texture,
            depth_view,
        }
    }

    #[cfg(not(tarpaulin_include))]
    pub fn resize(&mut self, gpu: &GpuContext) {
        (self.depth_texture, self.depth_view) =
            create_depth_texture(&gpu.device, gpu.width(), gpu.height());
    }

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
            .map(|id| scene.extract_active_camera(id).map_err(|e| RenderError::Asset(e.to_string())))
            .transpose()?;
        let draw_list = if let Some(cam) = extracted_camera {
            let aspect = gpu.aspect();
            let pv = camera_projection_view(&cam, aspect);
            let planes = rig_scene::frustum_planes_from_projection_view(pv);
            scene.extract_renderables_culled(&planes)
        } else {
            scene.extract_renderables()
        };
        let lights = scene.extract_lights();
        self.render_draw_list(gpu, frame, assets, extracted_camera, &draw_list, &lights)
    }

    #[cfg(not(tarpaulin_include))]
    fn render_draw_list(
        &mut self,
        gpu: &GpuContext,
        frame: &mut Frame,
        assets: &AssetStore,
        camera: Option<ExtractedCamera>,
        draw_list: &[ExtractedRenderable],
        lights: &[ExtractedLight],
    ) -> Result<()> {
        let Some(camera) = camera else {
            return Ok(());
        };
        let aspect = gpu.aspect();
        let pv = camera_projection_view(&camera, aspect);

        // Upload FrameUniforms once per frame
        let pose = decompose_pose(camera.world_transform);
        let proj = rig_math::Camera { pose, projection: camera.projection }
            .projection_matrix(aspect)
            .to_cols_array_2d();
        let view = rig_math::Camera { pose, projection: camera.projection }
            .view_matrix()
            .to_cols_array_2d();
        let frame_uniforms = FrameUniforms {
            view,
            proj,
            camera_pos: [pose.translation.x, pose.translation.y, pose.translation.z, 1.0],
        };
        gpu.queue.write_buffer(&self.frame_uniform_buffer, 0, bytemuck::bytes_of(&frame_uniforms));

        // Pack and upload lights
        let lights_buf = pack_lights_buffer(lights);
        gpu.queue.write_buffer(&self.lights_buffer, 0, bytemuck::bytes_of(&lights_buf));

        let sorted_indices = self.prepare_draw_order(gpu, assets, draw_list, pv);

        // Create frame bind group (group 0) — references the frame uniform buffer and lights
        let frame_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame bind group"),
            layout: &self.frame_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.frame_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.lights_buffer.as_entire_binding(),
                },
            ],
        });

        self.record_scene_pass(
            gpu,
            &mut frame.encoder,
            &frame.view,
            Some(&self.depth_view.clone()),
            wgpu::Color { r: 0.1, g: 0.1, b: 0.1, a: 1.0 },
            assets,
            draw_list,
            &sorted_indices,
            &frame_bind_group,
            gpu.surface_format(),
            Some(DEPTH_FORMAT),
        )
    }

    #[cfg(not(tarpaulin_include))]
    fn prepare_draw_order(
        &mut self,
        gpu: &GpuContext,
        assets: &AssetStore,
        draw_list: &[ExtractedRenderable],
        _pv: Mat4,
    ) -> Vec<usize> {
        let mut sorted_indices: Vec<usize> = (0..draw_list.len()).collect();
        sorted_indices.sort_by_key(|&i| {
            let object = &draw_list[i];
            let shader_key = assets.material(object.material).map(|m| m.shader).unwrap_or_else(|_| ShaderHandle::from_raw(u32::MAX));
            (shader_key, object.mesh)
        });
        // Upload only world matrices (PV is now in FrameUniforms)
        let object_uniforms: Vec<_> = sorted_indices.iter().map(|&i| ObjectUniforms {
            world: draw_list[i].world_transform.to_cols_array_2d(),
        }).collect();
        self.frame_resources.prepare_object_uniforms(
            &gpu.device,
            &gpu.queue,
            &self.object_bind_group_layout,
            &object_uniforms,
        );
        sorted_indices
    }

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
        frame_bind_group: &wgpu::BindGroup,
        color_format: wgpu::TextureFormat,
        depth_format: Option<wgpu::TextureFormat>,
    ) -> Result<()> {
        let depth_attachment = depth_view.map(|view| wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
            stencil_ops: None,
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rig scene pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear_color), store: wgpu::StoreOp::Store },
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
            let material = assets.material(object.material).map_err(|err| RenderError::Asset(err.to_string()))?;
            let shader = assets.shader(material.shader).map_err(|err| RenderError::Asset(err.to_string()))?;
            let mesh = assets.mesh(object.mesh).map_err(|err| RenderError::Asset(err.to_string()))?;
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

            // Group 0: frame uniforms
            pass.set_bind_group(0, frame_bind_group, &[]);

            // Group 1: material bind group — build per-material if it has a texture
            let material_bind_group = if !material.textures.is_empty() {
                let (tex_handle, samp_handle) = material.textures[0];
                let tex_asset = assets.texture(tex_handle)
                    .map_err(|e| RenderError::Asset(e.to_string()))?;
                let samp_desc = assets.sampler(samp_handle)
                    .map_err(|e| RenderError::Asset(e.to_string()))?;
                // Obtain raw pointers to break the two-borrow problem on self.cache.
                // SAFETY: Both borrows are non-overlapping (different HashMaps) and the
                // returned references remain valid for the duration of this block.
                let tex_view = self.cache.texture_view(&gpu.device, &gpu.queue, tex_handle, tex_asset)
                    as *const wgpu::TextureView;
                let sampler = self.cache.sampler(&gpu.device, samp_handle, samp_desc)
                    as *const wgpu::Sampler;
                // SAFETY: pointers dereference into stable HashMap values; no removal occurs.
                let tex_view = unsafe { &*tex_view };
                let sampler = unsafe { &*sampler };
                // Build a per-material uniform buffer with base_color = [1,1,1,1]
                let mat_uniforms = MaterialUniforms {
                    base_color: [1.0, 1.0, 1.0, 1.0],
                    flags: 1,
                    _pad: [0; 3],
                };
                let mat_buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("material uniform buffer"),
                    size: std::mem::size_of::<MaterialUniforms>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                gpu.queue.write_buffer(&mat_buf, 0, bytemuck::bytes_of(&mat_uniforms));
                let bg = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("textured material bind group"),
                    layout: &self.material_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: mat_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(tex_view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                    ],
                });
                Some(bg)
            } else {
                None
            };

            let mat_bg_ref = material_bind_group.as_ref()
                .unwrap_or(&self.fallback_material_bind_group);
            pass.set_bind_group(1, mat_bg_ref, &[]);

            // Group 2: object uniforms (dynamic offset)
            pass.set_bind_group(2, &self.frame_resources.object_uniforms.bind_group, &[self.frame_resources.object_uniforms.dynamic_offset(uniform_index)?]);

            if current_mesh != Some(object.mesh) {
                pass.set_vertex_buffer(0, buffers.vertex.slice(..));
                pass.set_index_buffer(buffers.index.slice(..), buffers.index_format);
                current_mesh = Some(object.mesh);
            }
            pass.draw_indexed(0..buffers.index_count, 0, 0..1);
        }
        Ok(())
    }

    fn pipeline_for_key(&mut self, gpu: &GpuContext, key: &PipelineKey, shader: &rig_assets::ShaderAsset) -> Result<wgpu::RenderPipeline> {
        if let Some(pipeline) = self.pipelines.get(key) {
            return Ok(pipeline.clone());
        }
        let shader_module = self.cache.shader_module(&gpu.device, key.shader, shader);
        let pipeline = create_pipeline(&gpu.device, &shader_module, &self.pipeline_layout, key.color_format, key.depth_format, &key.vertex_layout)?;
        self.pipelines.insert(key.clone(), pipeline.clone());
        Ok(pipeline)
    }

    pub fn create_render_target(&self, gpu: &GpuContext, desc: &RenderTargetDescriptor) -> RenderTarget {
        let device = &gpu.device;
        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(desc.label),
            size: wgpu::Extent3d { width: desc.width, height: desc.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: desc.color_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let (depth_texture, depth_view) = desc.depth_format.map(|fmt| {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("render target depth"),
                size: wgpu::Extent3d { width: desc.width, height: desc.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: fmt,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            (Some(tex), Some(view))
        }).unwrap_or((None, None));
        RenderTarget {
            color_texture, color_view, depth_texture, depth_view,
            width: desc.width, height: desc.height,
            color_format: desc.color_format, depth_format: desc.depth_format,
        }
    }

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
            .map(|id| scene.extract_active_camera(id).map_err(|e| RenderError::Asset(e.to_string())))
            .transpose()?;
        let Some(camera) = extracted_camera else { return Ok(()); };
        let aspect = target.width as f32 / target.height as f32;
        let pv = camera_projection_view(&camera, aspect);

        // Upload FrameUniforms
        let pose = decompose_pose(camera.world_transform);
        let proj = rig_math::Camera { pose, projection: camera.projection }
            .projection_matrix(aspect)
            .to_cols_array_2d();
        let view = rig_math::Camera { pose, projection: camera.projection }
            .view_matrix()
            .to_cols_array_2d();
        let frame_uniforms = FrameUniforms {
            view,
            proj,
            camera_pos: [pose.translation.x, pose.translation.y, pose.translation.z, 1.0],
        };
        gpu.queue.write_buffer(&self.frame_uniform_buffer, 0, bytemuck::bytes_of(&frame_uniforms));

        let planes = rig_scene::frustum_planes_from_projection_view(pv);
        let draw_list = scene.extract_renderables_culled(&planes);
        let lights = scene.extract_lights();
        let sorted_indices = self.prepare_draw_order(gpu, assets, &draw_list, pv);

        // Pack and upload lights
        let lights_buf = pack_lights_buffer(&lights);
        gpu.queue.write_buffer(&self.lights_buffer, 0, bytemuck::bytes_of(&lights_buf));

        let frame_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame bind group offscreen"),
            layout: &self.frame_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.frame_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.lights_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("rig offscreen encoder") });
        self.record_scene_pass(
            gpu, &mut encoder, &target.color_view, target.depth_view.as_ref(),
            wgpu::Color { r: 0.05, g: 0.05, b: 0.05, a: 1.0 },
            assets, &draw_list, &sorted_indices,
            &frame_bind_group,
            target.color_format, target.depth_format,
        )?;
        gpu.queue.submit(std::iter::once(encoder.finish()));
        Ok(())
    }

    #[cfg(not(tarpaulin_include))]
    pub fn blit_texture_to_screen(
        &mut self,
        frame: &mut Frame,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
    ) -> Result<()> {
        let mut pass = frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blit pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
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

/// Pack a slice of extracted lights into a [`LightsBuffer`] for GPU upload.
///
/// At most [`MAX_LIGHTS`] lights are packed; extra lights are silently ignored.
pub fn pack_lights_buffer(lights: &[ExtractedLight]) -> LightsBuffer {
    let mut buf = LightsBuffer::default();
    let count = lights.len().min(MAX_LIGHTS);
    buf.count[0] = count as u32;
    for (i, light) in lights.iter().take(MAX_LIGHTS).enumerate() {
        match light.kind {
            LightKind::Directional { color, intensity } => {
                buf.lights[i].color_intensity = [color.x, color.y, color.z, intensity];
                buf.lights[i].range_pad = [0.0; 4];
                buf.lights[i].position = [0.0, 0.0, 0.0, 0.0];
            }
            LightKind::Point { color, intensity, range } => {
                buf.lights[i].color_intensity = [color.x, color.y, color.z, intensity];
                buf.lights[i].range_pad = [range, 0.0, 0.0, 0.0];
                buf.lights[i].position = [
                    light.world_position.x,
                    light.world_position.y,
                    light.world_position.z,
                    1.0,
                ];
            }
        }
        buf.lights[i].direction = [
            light.world_direction.x,
            light.world_direction.y,
            light.world_direction.z,
            0.0,
        ];
    }
    buf
}
