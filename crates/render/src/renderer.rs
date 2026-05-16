//! Scene renderer: pipeline management, draw submission, offscreen targets.

use std::collections::HashMap;
use std::num::NonZeroU64;

use rig_assets::{AssetStore, DynamicMeshId, MaterialAsset, MeshSource, ShaderHandle};
use rig_gpu::{Frame, GpuContext};
use rig_math::Mat4;
use rig_scene::{
    ExtractedCamera, ExtractedLight, ExtractedRenderable, LightKind, NodeId, SceneGraph,
};

use crate::cache::ImmutableResourceCache;
use crate::frame::{FrameResources, ObjectUniforms};
use crate::helpers::{
    DEPTH_FORMAT, FrameUniforms, LightsBuffer, MAX_LIGHTS, MaterialUniforms,
    camera_projection_view, create_depth_texture, create_pipeline, decompose_pose,
};
use crate::pipeline::PipelineKey;
use crate::{RenderError, RenderTarget, RenderTargetDescriptor, Result};

/// GPU vertex + index buffers for a single dynamic mesh.
///
/// Buffers are grown on demand (next_power_of_two) when new data exceeds capacity.
pub struct DynamicMesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_format: wgpu::IndexFormat,
    pub index_count: u32,
    vertex_capacity_bytes: u64,
    index_capacity_bytes: u64,
}

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
    // Fallback (untextured) material resources — kept alive to back per-draw bind groups
    #[allow(dead_code)]
    pub(crate) fallback_texture: wgpu::Texture,
    pub(crate) fallback_texture_view: wgpu::TextureView,
    #[allow(dead_code)]
    pub(crate) fallback_normal_texture: wgpu::Texture,
    pub(crate) fallback_normal_texture_view: wgpu::TextureView,
    #[allow(dead_code)]
    pub(crate) fallback_black_texture: wgpu::Texture,
    pub(crate) fallback_black_texture_view: wgpu::TextureView,
    pub(crate) fallback_sampler: wgpu::Sampler,
    // Per-frame object uniforms
    pub(crate) frame_resources: FrameResources,
    pub(crate) cache: ImmutableResourceCache,
    pub(crate) depth_texture: wgpu::Texture,
    pub(crate) depth_view: wgpu::TextureView,
    /// Dynamic mesh GPU buffers, keyed by `DynamicMeshId`.
    pub(crate) dynamic_meshes: HashMap<DynamicMeshId, DynamicMesh>,
    /// Whether wireframe rendering is currently active.
    pub wireframe: bool,
}

#[cfg(not(tarpaulin_include))]
impl Renderer {
    pub fn new(gpu: &GpuContext) -> Self {
        let device = &gpu.device;

        // ── Group 0: frame uniforms (view/proj/camera_pos) + lights ──────────
        let frame_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("frame bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<FrameUniforms>() as u64
                            ),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<LightsBuffer>() as u64
                            ),
                        },
                        count: None,
                    },
                ],
            });

        // ── Group 1: material uniforms + 5 PBR texture/sampler slots ──────────
        let mut material_entries = Vec::with_capacity(1 + MaterialAsset::SLOT_COUNT * 2);
        material_entries.push(wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: NonZeroU64::new(std::mem::size_of::<MaterialUniforms>() as u64),
            },
            count: None,
        });
        for slot in 0..MaterialAsset::SLOT_COUNT {
            let texture_binding = 1 + (slot as u32 * 2);
            material_entries.push(wgpu::BindGroupLayoutEntry {
                binding: texture_binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
            material_entries.push(wgpu::BindGroupLayoutEntry {
                binding: texture_binding + 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            });
        }
        let material_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("material bind group layout"),
                entries: &material_entries,
            });

        // ── Group 2: per-object uniforms (world matrix, dynamic offset) ───────
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

        // ── Fallback 1×1 material textures ────────────────────────────────────
        let (fallback_texture, fallback_texture_view) = create_fallback_texture(
            device,
            &gpu.queue,
            "fallback white texture",
            [255, 255, 255, 255],
        );
        let (fallback_normal_texture, fallback_normal_texture_view) = create_fallback_texture(
            device,
            &gpu.queue,
            "fallback normal texture",
            [128, 128, 255, 255],
        );
        let (fallback_black_texture, fallback_black_texture_view) =
            create_fallback_texture(device, &gpu.queue, "fallback black texture", [0, 0, 0, 255]);
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
            fallback_normal_texture,
            fallback_normal_texture_view,
            fallback_black_texture,
            fallback_black_texture_view,
            fallback_sampler,
            frame_resources,
            cache: ImmutableResourceCache::default(),
            depth_texture,
            depth_view,
            dynamic_meshes: HashMap::new(),
            wireframe: false,
        }
    }

    #[cfg(not(tarpaulin_include))]
    pub fn resize(&mut self, gpu: &GpuContext) {
        (self.depth_texture, self.depth_view) =
            create_depth_texture(&gpu.device, gpu.width(), gpu.height());
    }

    /// Register a new dynamic mesh slot.  Call once per `DynamicMeshId` at startup.
    ///
    /// `initial_vertex_bytes` and `initial_index_bytes` set the initial GPU buffer sizes.
    /// Both buffers grow automatically in `update_dynamic_mesh()` when needed.
    #[cfg(not(tarpaulin_include))]
    pub fn register_dynamic_mesh(
        &mut self,
        device: &wgpu::Device,
        id: DynamicMeshId,
        initial_vertex_bytes: u64,
        initial_index_bytes: u64,
    ) {
        let vertex_capacity_bytes = initial_vertex_bytes.max(32).next_power_of_two();
        let index_capacity_bytes = initial_index_bytes.max(4).next_power_of_two();
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dynamic mesh vertex buffer"),
            size: vertex_capacity_bytes,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dynamic mesh index buffer"),
            size: index_capacity_bytes,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.dynamic_meshes.insert(
            id,
            DynamicMesh {
                vertex_buffer,
                index_buffer,
                index_format: wgpu::IndexFormat::Uint32,
                index_count: 0,
                vertex_capacity_bytes,
                index_capacity_bytes,
            },
        );
    }

    /// Upload new vertex and index data for a registered dynamic mesh.
    ///
    /// Grows the GPU buffers if the new data exceeds current capacity.
    /// Call this in `render()` before `render_scene()`.
    #[cfg(not(tarpaulin_include))]
    pub fn update_dynamic_mesh(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: DynamicMeshId,
        data: &rig_assets::DynamicMeshData,
    ) {
        let vertex_bytes = data.vertex_data.len() as u64;
        let index_bytes = data.index_data.len() as u64;

        let entry = self.dynamic_meshes.entry(id).or_insert_with(|| {
            let vc = vertex_bytes.max(32).next_power_of_two();
            let ic = index_bytes.max(4).next_power_of_two();
            DynamicMesh {
                vertex_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("dynamic mesh vertex buffer"),
                    size: vc,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                index_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("dynamic mesh index buffer"),
                    size: ic,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                index_format: wgpu::IndexFormat::Uint32,
                index_count: 0,
                vertex_capacity_bytes: vc,
                index_capacity_bytes: ic,
            }
        });

        // Grow vertex buffer if needed
        if vertex_bytes > entry.vertex_capacity_bytes {
            let new_cap = vertex_bytes.next_power_of_two();
            entry.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("dynamic mesh vertex buffer"),
                size: new_cap,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            entry.vertex_capacity_bytes = new_cap;
        }
        // Grow index buffer if needed
        if index_bytes > entry.index_capacity_bytes {
            let new_cap = index_bytes.next_power_of_two();
            entry.index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("dynamic mesh index buffer"),
                size: new_cap,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            entry.index_capacity_bytes = new_cap;
        }

        if !data.vertex_data.is_empty() {
            write_aligned_buffer(queue, &entry.vertex_buffer, &data.vertex_data);
        }
        if !data.index_data.is_empty() {
            write_aligned_buffer(queue, &entry.index_buffer, &data.index_data);
        }
        entry.index_format = match data.index_format {
            rig_assets::IndexFormat::Uint16 => wgpu::IndexFormat::Uint16,
            rig_assets::IndexFormat::Uint32 => wgpu::IndexFormat::Uint32,
        };
        entry.index_count = data.index_count;
    }

    /// Toggle wireframe rendering on/off.
    ///
    /// If `supports_wireframe` is `false` (adapter lacks `POLYGON_MODE_LINE`),
    /// this is a no-op and the function returns `false` to indicate the toggle
    /// was not applied.
    pub fn toggle_wireframe(&mut self, supports_wireframe: bool) -> bool {
        if !supports_wireframe {
            return false;
        }
        self.wireframe = !self.wireframe;
        // Invalidate pipeline cache so pipelines are rebuilt with new polygon_mode
        self.pipelines.clear();
        true
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
        let proj = rig_math::Camera {
            pose,
            projection: camera.projection,
        }
        .projection_matrix(aspect)
        .to_cols_array_2d();
        let view = rig_math::Camera {
            pose,
            projection: camera.projection,
        }
        .view_matrix()
        .to_cols_array_2d();
        let frame_uniforms = FrameUniforms {
            view,
            proj,
            camera_pos: [
                pose.translation.x,
                pose.translation.y,
                pose.translation.z,
                1.0,
            ],
        };
        gpu.queue.write_buffer(
            &self.frame_uniform_buffer,
            0,
            bytemuck::bytes_of(&frame_uniforms),
        );

        // Pack and upload lights
        let lights_buf = pack_lights_buffer(lights);
        gpu.queue
            .write_buffer(&self.lights_buffer, 0, bytemuck::bytes_of(&lights_buf));

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
            wgpu::Color::BLACK,
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
            let shader_key = assets
                .material(object.material)
                .map(|m| m.shader)
                .unwrap_or_else(|_| ShaderHandle::from_raw(u32::MAX));
            (shader_key, object.mesh)
        });
        // Upload only world matrices (PV is now in FrameUniforms)
        let object_uniforms: Vec<_> = sorted_indices
            .iter()
            .map(|&i| ObjectUniforms {
                world: draw_list[i].world_transform.to_cols_array_2d(),
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
        let mut current_mesh: Option<MeshSource> = None;
        let polygon_mode = if self.wireframe {
            wgpu::PolygonMode::Line
        } else {
            wgpu::PolygonMode::Fill
        };

        for (uniform_index, &draw_index) in sorted_indices.iter().enumerate() {
            let object = &draw_list[draw_index];
            let material = assets
                .material(object.material)
                .map_err(|err| RenderError::Asset(err.to_string()))?;
            let shader = assets
                .shader(material.shader)
                .map_err(|err| RenderError::Asset(err.to_string()))?;

            // Resolve mesh buffers based on MeshSource.
            // Clone the wgpu::Buffer handles (Arc-backed) so they outlive the match.
            let (vertex_buf, index_buf, index_format, index_count, vertex_layout) =
                match object.mesh {
                    MeshSource::Static(handle) => {
                        let mesh = assets
                            .mesh(handle)
                            .map_err(|err| RenderError::Asset(err.to_string()))?;
                        let buffers = self.cache.mesh_buffers(&gpu.device, handle, mesh);
                        (
                            buffers.vertex.clone(),
                            buffers.index.clone(),
                            buffers.index_format,
                            buffers.index_count,
                            mesh.vertex_layout.clone(),
                        )
                    }
                    MeshSource::Dynamic(id) => {
                        let Some(dyn_mesh) = self.dynamic_meshes.get(&id) else {
                            continue; // not yet registered — skip
                        };
                        (
                            dyn_mesh.vertex_buffer.clone(),
                            dyn_mesh.index_buffer.clone(),
                            dyn_mesh.index_format,
                            dyn_mesh.index_count,
                            rig_assets::standard_vertex_layout(),
                        )
                    }
                };

            let pipeline_key = PipelineKey {
                shader: material.shader,
                vertex_layout,
                color_format,
                depth_format,
                polygon_mode,
            };
            let pipeline = self.pipeline_for_key(gpu, &pipeline_key, shader)?;
            if current_pipeline.as_ref() != Some(&pipeline_key) {
                pass.set_pipeline(&pipeline);
                current_pipeline = Some(pipeline_key);
            }

            // Group 0: frame uniforms
            pass.set_bind_group(0, frame_bind_group, &[]);

            // Group 1: material bind group.
            //
            // Always build a per-draw uniform buffer from `material.parameters` so that
            // metallic, roughness, and base_color are correctly forwarded to the shader.
            // For textured materials the real texture/sampler are bound; otherwise the
            // renderer's fallback 1×1 white texture is used (so the shader can sample it
            // safely without branching).
            let material_bind_group = {
                let params = &material.parameters;
                let flags = material
                    .textures
                    .iter()
                    .take(MaterialAsset::SLOT_COUNT)
                    .enumerate()
                    .fold(0_u32, |flags, (slot, texture)| {
                        if texture.is_some() {
                            flags | (1_u32 << slot)
                        } else {
                            flags
                        }
                    });
                let mat_uniforms = MaterialUniforms {
                    base_color: params.diffuse,
                    metallic: params.metallic,
                    roughness: params.roughness,
                    flags: flags | params.custom_flags,
                    triplanar_scale: params.triplanar_scale,
                };
                let mat_buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("material uniform buffer"),
                    size: std::mem::size_of::<MaterialUniforms>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                gpu.queue
                    .write_buffer(&mat_buf, 0, bytemuck::bytes_of(&mat_uniforms));

                let mut resolved_slots: Vec<(wgpu::TextureView, wgpu::Sampler)> =
                    Vec::with_capacity(MaterialAsset::SLOT_COUNT);
                for slot in 0..MaterialAsset::SLOT_COUNT {
                    if let Some((tex_handle, samp_handle)) =
                        material.textures.get(slot).and_then(|texture| *texture)
                    {
                        let tex_asset = assets
                            .texture(tex_handle)
                            .map_err(|e| RenderError::Asset(e.to_string()))?;
                        let samp_desc = assets
                            .sampler(samp_handle)
                            .map_err(|e| RenderError::Asset(e.to_string()))?;
                        let tex_view = self
                            .cache
                            .texture_view(&gpu.device, &gpu.queue, tex_handle, tex_asset)
                            .clone();
                        let sampler = self
                            .cache
                            .sampler(&gpu.device, samp_handle, samp_desc)
                            .clone();
                        resolved_slots.push((tex_view, sampler));
                    } else {
                        let fallback_view = match slot {
                            0 => self.fallback_texture_view.clone(),
                            1 => self.fallback_normal_texture_view.clone(),
                            _ => self.fallback_black_texture_view.clone(),
                        };
                        resolved_slots.push((fallback_view, self.fallback_sampler.clone()));
                    }
                }

                let mut entries = Vec::with_capacity(1 + MaterialAsset::SLOT_COUNT * 2);
                entries.push(wgpu::BindGroupEntry {
                    binding: 0,
                    resource: mat_buf.as_entire_binding(),
                });
                for (slot, (texture_view, sampler)) in resolved_slots.iter().enumerate() {
                    let texture_binding = 1 + (slot as u32 * 2);
                    entries.push(wgpu::BindGroupEntry {
                        binding: texture_binding,
                        resource: wgpu::BindingResource::TextureView(texture_view),
                    });
                    entries.push(wgpu::BindGroupEntry {
                        binding: texture_binding + 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    });
                }

                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("material bind group"),
                    layout: &self.material_bind_group_layout,
                    entries: &entries,
                })
            };

            pass.set_bind_group(1, &material_bind_group, &[]);

            // Group 2: object uniforms (dynamic offset)
            pass.set_bind_group(
                2,
                &self.frame_resources.object_uniforms.bind_group,
                &[self
                    .frame_resources
                    .object_uniforms
                    .dynamic_offset(uniform_index)?],
            );

            if current_mesh != Some(object.mesh) {
                pass.set_vertex_buffer(0, vertex_buf.slice(..));
                pass.set_index_buffer(index_buf.slice(..), index_format);
                current_mesh = Some(object.mesh);
            }
            pass.draw_indexed(0..index_count, 0, 0..1);
        }
        Ok(())
    }

    fn pipeline_for_key(
        &mut self,
        gpu: &GpuContext,
        key: &PipelineKey,
        shader: &rig_assets::ShaderAsset,
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
            key.polygon_mode,
        )?;
        self.pipelines.insert(key.clone(), pipeline.clone());
        Ok(pipeline)
    }

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

        // Upload FrameUniforms
        let pose = decompose_pose(camera.world_transform);
        let proj = rig_math::Camera {
            pose,
            projection: camera.projection,
        }
        .projection_matrix(aspect)
        .to_cols_array_2d();
        let view = rig_math::Camera {
            pose,
            projection: camera.projection,
        }
        .view_matrix()
        .to_cols_array_2d();
        let frame_uniforms = FrameUniforms {
            view,
            proj,
            camera_pos: [
                pose.translation.x,
                pose.translation.y,
                pose.translation.z,
                1.0,
            ],
        };
        gpu.queue.write_buffer(
            &self.frame_uniform_buffer,
            0,
            bytemuck::bytes_of(&frame_uniforms),
        );

        let planes = rig_scene::frustum_planes_from_projection_view(pv);
        let draw_list = scene.extract_renderables_culled(&planes);
        let lights = scene.extract_lights();
        let sorted_indices = self.prepare_draw_order(gpu, assets, &draw_list, pv);

        // Pack and upload lights
        let lights_buf = pack_lights_buffer(&lights);
        gpu.queue
            .write_buffer(&self.lights_buffer, 0, bytemuck::bytes_of(&lights_buf));

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
            &frame_bind_group,
            target.color_format,
            target.depth_format,
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

#[cfg(not(tarpaulin_include))]
fn write_aligned_buffer(queue: &wgpu::Queue, buffer: &wgpu::Buffer, data: &[u8]) {
    const ALIGNMENT: usize = wgpu::COPY_BUFFER_ALIGNMENT as usize;
    if data.len() % ALIGNMENT == 0 {
        queue.write_buffer(buffer, 0, data);
        return;
    }

    let padded_len = data.len().div_ceil(ALIGNMENT) * ALIGNMENT;
    let mut padded = Vec::with_capacity(padded_len);
    padded.extend_from_slice(data);
    padded.resize(padded_len, 0);
    queue.write_buffer(buffer, 0, &padded);
}

#[cfg(not(tarpaulin_include))]
fn create_fallback_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &'static str,
    rgba: [u8; 4],
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
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
        &rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
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
            LightKind::Point {
                color,
                intensity,
                range,
            } => {
                buf.lights[i].color_intensity = [color.x, color.y, color.z, intensity];
                buf.lights[i].range_pad = [range, 0.0, 0.0, 0.0];
                buf.lights[i].position = [
                    light.world_position.x,
                    light.world_position.y,
                    light.world_position.z,
                    1.0,
                ];
            }
            LightKind::Spot {
                color,
                intensity,
                range,
                inner_cone_angle,
                outer_cone_angle,
            } => {
                buf.lights[i].color_intensity = [color.x, color.y, color.z, intensity];
                buf.lights[i].range_pad =
                    [range, inner_cone_angle.cos(), outer_cone_angle.cos(), 0.0];
                buf.lights[i].position = [
                    light.world_position.x,
                    light.world_position.y,
                    light.world_position.z,
                    2.0,
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
