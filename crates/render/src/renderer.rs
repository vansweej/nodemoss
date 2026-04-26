//! Scene renderer: pipeline management, draw submission, offscreen targets.

use std::collections::HashMap;
use std::num::NonZeroU64;

use rig_assets::{AssetStore, ShaderHandle};
use rig_gpu::{Frame, GpuContext};
use rig_math::Mat4;
use rig_scene::{ExtractedCamera, ExtractedRenderable, NodeId, SceneGraph};

use crate::cache::ImmutableResourceCache;
use crate::frame::{FrameResources, ObjectUniforms};
use crate::helpers::{camera_projection_view, create_depth_texture, create_pipeline, DEPTH_FORMAT};
use crate::pipeline::PipelineKey;
use crate::{RenderError, RenderTarget, RenderTargetDescriptor, Result};

/// Scene renderer.
pub struct Renderer {
    pub(crate) pipeline_layout: wgpu::PipelineLayout,
    pub(crate) pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    pub(crate) object_bind_group_layout: wgpu::BindGroupLayout,
    pub(crate) frame_resources: FrameResources,
    pub(crate) cache: ImmutableResourceCache,
    pub(crate) depth_texture: wgpu::Texture,
    pub(crate) depth_view: wgpu::TextureView,
}

#[cfg(not(tarpaulin_include))]
impl Renderer {
    pub fn new(gpu: &GpuContext) -> Self {
        let device = &gpu.device;
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
        let (depth_texture, depth_view) = create_depth_texture(device, gpu.width(), gpu.height());
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
            wgpu::Color { r: 0.1, g: 0.1, b: 0.1, a: 1.0 },
            assets,
            draw_list,
            &sorted_indices,
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
        pv: Mat4,
    ) -> Vec<usize> {
        let mut sorted_indices: Vec<usize> = (0..draw_list.len()).collect();
        sorted_indices.sort_by_key(|&i| {
            let object = &draw_list[i];
            let shader_key = assets.material(object.material).map(|m| m.shader).unwrap_or_else(|_| ShaderHandle::from_raw(u32::MAX));
            (shader_key, object.mesh)
        });
        let object_uniforms: Vec<_> = sorted_indices.iter().map(|&i| ObjectUniforms {
            world: (pv * draw_list[i].world_transform).to_cols_array_2d(),
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
            pass.set_bind_group(0, &self.frame_resources.object_uniforms.bind_group, &[self.frame_resources.object_uniforms.dynamic_offset(uniform_index)?]);
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
        let planes = rig_scene::frustum_planes_from_projection_view(pv);
        let draw_list = scene.extract_renderables_culled(&planes);
        let sorted_indices = self.prepare_draw_order(gpu, assets, &draw_list, pv);
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("rig offscreen encoder") });
        self.record_scene_pass(
            gpu, &mut encoder, &target.color_view, target.depth_view.as_ref(),
            wgpu::Color { r: 0.05, g: 0.05, b: 0.05, a: 1.0 },
            assets, &draw_list, &sorted_indices,
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
