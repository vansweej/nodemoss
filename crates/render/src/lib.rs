//! Concrete `wgpu` renderer for the rig framework.

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

use bytemuck::{Pod, Zeroable};
use rig_assets::{AssetStore, MeshAsset, VertexFormat};
use rig_math::{Camera, Mat4};
use rig_scene::{ExtractedRenderable, NodeId, SceneGraph};
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
}

#[derive(Default)]
struct ImmutableResourceCache {
    meshes: HashMap<u64, CachedMeshBuffers>,
}

impl ImmutableResourceCache {
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
        let index_count = (mesh.index_data.len() / std::mem::size_of::<u16>()) as u32;
        let buffers = CachedMeshBuffers {
            vertex,
            index,
            index_count,
        };
        self.meshes.insert(key, buffers.clone());
        buffers
    }
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    object_bind_group_layout: wgpu::BindGroupLayout,
    window: Arc<Window>,
    cache: ImmutableResourceCache,
}

impl Renderer {
    #[cfg(not(tarpaulin_include))]
    pub async fn new(window: Arc<Window>, shader_source: &str) -> Result<Self> {
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

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rig render shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let object_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("object bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rig render pipeline layout"),
            bind_group_layouts: &[Some(&object_bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rig render pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[triangle_vertex_layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
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
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Ok(Self {
            surface,
            device,
            queue,
            surface_config,
            pipeline,
            object_bind_group_layout,
            window,
            cache: ImmutableResourceCache::default(),
        })
    }

    #[cfg(not(tarpaulin_include))]
    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            self.surface_config.width = size.width;
            self.surface_config.height = size.height;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }

    #[cfg(not(tarpaulin_include))]
    pub fn window(&self) -> &Window {
        &self.window
    }

    #[cfg(not(tarpaulin_include))]
    pub fn render_scene(
        &mut self,
        scene: &SceneGraph,
        assets: &AssetStore,
        active_camera: Option<NodeId>,
    ) -> Result<()> {
        let draw_list = scene.extract_renderables();
        self.render_draw_list(scene, assets, active_camera, &draw_list)
    }

    #[cfg(not(tarpaulin_include))]
    fn render_draw_list(
        &mut self,
        scene: &SceneGraph,
        assets: &AssetStore,
        active_camera: Option<NodeId>,
        draw_list: &[ExtractedRenderable],
    ) -> Result<()> {
        let Some(active_camera) = active_camera else {
            return Ok(());
        };

        let camera_component = scene
            .camera(active_camera)
            .map_err(|_| RenderError::InvalidCamera)?
            .copied()
            .ok_or(RenderError::InvalidCamera)?;
        let pose = decompose_pose(scene.world_transform(active_camera).map_err(|_| RenderError::InvalidCamera)?);
        let camera = Camera {
            pose,
            projection: camera_component.projection,
        };
        let aspect = self.surface_config.width as f32 / self.surface_config.height as f32;
        let pv = camera.projection_view_matrix(aspect);

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
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.pipeline);
            for object in draw_list {
                let material = assets
                    .material(object.material)
                    .map_err(|err| RenderError::Asset(err.to_string()))?;
                let _shader = assets
                    .shader(material.shader)
                    .map_err(|err| RenderError::Asset(err.to_string()))?;
                let mesh = assets
                    .mesh(object.mesh)
                    .map_err(|err| RenderError::Asset(err.to_string()))?;
                if !validate_triangle_layout(mesh) {
                    return Err(RenderError::Asset(
                        "renderer currently expects position+color triangle vertex layout".into(),
                    ));
                }
                let buffers = self.cache.mesh_buffers(&self.device, mesh);

                let object_uniforms = ObjectUniforms {
                    world: (pv * object.world_transform).to_cols_array_2d(),
                };
                let uniform_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("object uniforms"),
                    contents: bytemuck::bytes_of(&object_uniforms),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("object bind group"),
                    layout: &self.object_bind_group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffer.as_entire_binding(),
                    }],
                });

                pass.set_bind_group(0, &bind_group, &[]);
                pass.set_vertex_buffer(0, buffers.vertex.slice(..));
                pass.set_index_buffer(buffers.index.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..buffers.index_count, 0, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        Ok(())
    }
}

fn triangle_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

    wgpu::VertexBufferLayout {
        array_stride: (std::mem::size_of::<[f32; 3]>() * 2) as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
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
    mesh.vertex_layout.attributes.len() == 2
        && mesh.vertex_layout.array_stride == 24
        && mesh.vertex_layout.attributes[0].shader_location == 0
        && mesh.vertex_layout.attributes[0].format == VertexFormat::Float32x3
        && mesh.vertex_layout.attributes[1].shader_location == 1
        && mesh.vertex_layout.attributes[1].format == VertexFormat::Float32x3
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rig_assets::{VertexAttribute, VertexLayout};
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
            local_bounds: rig_math::BoundingSphere::ZERO,
        }
    }

    #[test]
    fn validate_triangle_layout_accepts_position_color_layout() {
        assert!(validate_triangle_layout(&sample_mesh()));
    }

    #[test]
    fn validate_triangle_layout_rejects_wrong_stride() {
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
    fn triangle_vertex_layout_matches_expected_shader_contract() {
        let layout = triangle_vertex_layout();

        assert_eq!(layout.array_stride, 24);
        assert_eq!(layout.step_mode, wgpu::VertexStepMode::Vertex);
        assert_eq!(layout.attributes.len(), 2);
        assert_eq!(layout.attributes[0].shader_location, 0);
        assert_eq!(layout.attributes[1].shader_location, 1);
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
}
