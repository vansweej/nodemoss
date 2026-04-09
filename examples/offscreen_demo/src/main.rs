//! Offscreen rendering demo.
//!
//! What this demonstrates:
//!   - `Renderer::create_render_target` to allocate an offscreen colour+depth texture
//!   - `Renderer::render_to_target` to render a scene into the offscreen target
//!   - A custom fullscreen-quad pipeline that samples the offscreen colour texture
//!     and blits it to the swapchain surface
//!   - Pipeline specialisation: the offscreen pass uses `Rgba8UnormSrgb` while
//!     the swapchain may use `Bgra8UnormSrgb` (or another sRGB format)
//!
//! Controls: W/S/A/D/Q/E to move camera, arrow keys to rotate.

use anyhow::Result;
use rig_app::{
    Application, CameraRig, RenderContext, StartupContext, UpdateContext,
    rig_assets::{MaterialAsset, ShaderAsset, mesh_factory},
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::{RenderTarget, RenderTargetDescriptor, wgpu},
    rig_scene::{CameraComponent, NodeId, Renderable},
};

// ---------------------------------------------------------------------------
// Offscreen scene shader (normal-shaded, same as multi_object)
// ---------------------------------------------------------------------------

const SCENE_SHADER: &str = r#"
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
    out.color = in.normal * 0.5 + vec3<f32>(0.5, 0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
"#;

// ---------------------------------------------------------------------------
// Fullscreen-blit shader
//
// The vertex shader generates a clip-space triangle that covers the screen
// from just three vertex indices (no vertex buffer required).
// ---------------------------------------------------------------------------

const BLIT_SHADER: &str = r#"
@group(0) @binding(0) var t_color: texture_2d<f32>;
@group(0) @binding(1) var s_color: sampler;

struct BlitOutput {
    @builtin(position) position: vec4<f32>,
    @location(0)       uv:       vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> BlitOutput {
    // Generate a triangle that covers the entire [-1,1]^2 clip space.
    // vi=0 → (-1,-1), vi=1 → (3,-1), vi=2 → (-1, 3)
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * (-4) + 1);
    let u = (x + 1.0) * 0.5;
    let v = (1.0 - y) * 0.5;

    var out: BlitOutput;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(u, v);
    return out;
}

@fragment
fn fs_main(in: BlitOutput) -> @location(0) vec4<f32> {
    return textureSample(t_color, s_color, in.uv);
}
"#;

// ---------------------------------------------------------------------------
// GPU resources owned by the example
// ---------------------------------------------------------------------------

struct BlitResources {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    _sampler: wgpu::Sampler,
}

// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

const OFFSCREEN_WIDTH: u32 = 512;
const OFFSCREEN_HEIGHT: u32 = 512;
const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

struct OffscreenApp {
    camera: NodeId,
    camera_rig: CameraRig,
    box_node: NodeId,
    offscreen_target: RenderTarget,
    blit: BlitResources,
}

impl Application for OffscreenApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        // --- Scene shader & mesh ---------------------------------------------
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: SCENE_SHADER.into(),
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: Default::default(),
            textures: vec![],
        });
        let box_mesh = ctx.assets.add_mesh(mesh_factory::create_box(1.0, 1.0, 1.0));

        // --- Scene nodes -----------------------------------------------------
        let box_node = ctx.scene.create_node("box");
        ctx.scene.set_renderable(box_node, Renderable { mesh: box_mesh, material })?;

        let camera = ctx.scene.create_node("camera");
        ctx.scene.set_local_transform(camera, Transform {
            translation: Vec3::new(0.0, 1.0, 3.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        })?;
        ctx.scene.set_camera(camera, CameraComponent {
            projection: Projection::Perspective {
                fov_y_radians: 60.0_f32.to_radians(),
                near: 0.1,
                far: 100.0,
            },
        })?;

        // --- Offscreen render target -----------------------------------------
        let offscreen_target = ctx.renderer.create_render_target(&RenderTargetDescriptor {
            width: OFFSCREEN_WIDTH,
            height: OFFSCREEN_HEIGHT,
            color_format: OFFSCREEN_FORMAT,
            depth_format: Some(rig_app::rig_render::DEPTH_FORMAT),
            label: "offscreen scene",
        });

        // --- Blit pipeline ---------------------------------------------------
        let blit = build_blit_resources(
            ctx.renderer.device(),
            ctx.renderer.surface_format(),
            &offscreen_target,
        );

        Ok(Self {
            camera,
            camera_rig: CameraRig::default(),
            box_node,
            offscreen_target,
            blit,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        *ctx.active_camera = Some(self.camera);
        self.camera_rig.update(ctx, self.camera, dt)?;

        // Spin the box.
        let mut t = ctx.scene.local_transform(self.box_node)?;
        t.rotation = (Quat::from_rotation_y(dt) * t.rotation).normalize();
        ctx.scene.set_local_transform(self.box_node, t)?;

        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
        // 1. Render scene into offscreen target.
        ctx.renderer.render_to_target(
            &self.offscreen_target,
            ctx.scene,
            ctx.assets,
            ctx.active_camera,
        )?;

        // 2. Blit the offscreen colour texture onto the swapchain.
        blit_to_screen(ctx.renderer, &self.blit)?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Blit helpers
// ---------------------------------------------------------------------------

fn build_blit_resources(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    target: &RenderTarget,
) -> BlitResources {
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("blit sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("blit bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("blit bind group"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&target.color_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("blit pipeline layout"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("blit shader"),
        source: wgpu::ShaderSource::Wgsl(BLIT_SHADER.into()),
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("blit pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[], // positions generated in vertex shader
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None, // fullscreen tri has no backface
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    BlitResources {
        pipeline,
        bind_group,
        _sampler: sampler,
    }
}

fn blit_to_screen(
    renderer: &mut rig_app::rig_render::Renderer,
    blit: &BlitResources,
) -> Result<()> {
    renderer.blit_texture_to_screen(&blit.pipeline, &blit.bind_group)?;
    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<OffscreenApp>("Offscreen Demo")
}
