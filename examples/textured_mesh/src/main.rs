//! Textured mesh demo.
//!
//! Renders a sphere with a procedurally generated checkerboard texture.
//! A fly-camera lets you orbit and inspect the textured surface.
//!
//! # Controls
//!
//! | Key(s)      | Action                        |
//! |-------------|-------------------------------|
//! | W / S       | Move forward / backward       |
//! | A / D       | Strafe left / right           |
//! | Q / E       | Move up / down                |
//! | Arrow keys  | Rotate camera (yaw / pitch)   |
//! | Escape      | Close window                  |

use std::sync::Arc;

use anyhow::Result;
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, StartupContext,
    UpdateContext,
    rig_assets::{
        AddressMode, AlphaMode, FilterMode, MaterialAsset, MaterialParams, SamplerDescriptor,
        ShaderAsset, TextureAsset, TextureFormat, mesh_factory,
    },
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::TEXTURED_SHADER,
    rig_scene::{CameraComponent, MeshSource, NodeId, Renderable},
    winit::{event::WindowEvent, keyboard::KeyCode},
};

struct TexturedMeshApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    debug_hud: DebugHud,
}

impl Application for TexturedMeshApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        // ── Checkerboard texture (64×64, 8-pixel squares) ─────────────────────
        let mut pixels = vec![0_u8; 64 * 64 * 4];
        for y in 0..64_u32 {
            for x in 0..64_u32 {
                let checker = ((x / 8) + (y / 8)) % 2 == 0;
                let color: [u8; 4] = if checker {
                    [255, 128, 0, 255] // orange
                } else {
                    [64, 64, 64, 255] // dark grey
                };
                let idx = ((y * 64 + x) * 4) as usize;
                pixels[idx..idx + 4].copy_from_slice(&color);
            }
        }
        let tex_asset = TextureAsset {
            width: 64,
            height: 64,
            format: TextureFormat::Rgba8Unorm,
            data: Arc::from(pixels.as_slice()),
        };
        let tex_handle = ctx.assets.add_texture(tex_asset);

        // ── Sampler: repeat + linear ──────────────────────────────────────────
        let samp_desc = SamplerDescriptor {
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
        };
        let samp_handle = ctx.assets.add_sampler(samp_desc);

        // ── Shader & material ─────────────────────────────────────────────────
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: TEXTURED_SHADER.into(),
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams::default(),
            textures: vec![Some((tex_handle, samp_handle))],
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
        });

        // ── Sphere mesh ───────────────────────────────────────────────────────
        let mesh = ctx
            .assets
            .add_mesh(mesh_factory::create_sphere(1.0, 32, 32));
        let sphere = ctx.scene.create_node("sphere");
        ctx.scene.set_renderable(
            sphere,
            Renderable {
                mesh: MeshSource::Static(mesh),
                material,
            },
        )?;
        ctx.scene.set_local_transform(
            sphere,
            Transform {
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;

        // ── Camera ────────────────────────────────────────────────────────────
        let camera_node = ctx.scene.create_node("camera");
        ctx.scene.set_local_transform(
            camera_node,
            Transform {
                translation: Vec3::new(0.0, 0.0, 4.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_camera(
            camera_node,
            CameraComponent {
                projection: Projection::Perspective {
                    fov_y_radians: 60.0_f32.to_radians(),
                    near: 0.1,
                    far: 100.0,
                },
            },
        )?;

        // ── Overlay ───────────────────────────────────────────────────────────
        let debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);

        log::info!("Textured mesh demo initialised.");
        log::info!("Controls: WASD/QE move, arrow keys rotate, Escape quits.");

        Ok(Self {
            camera_node,
            camera_rig: CameraRig {
                translation_speed: 3.0,
                rotation_speed: 1.5,
            },
            debug_hud,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        *ctx.active_camera = Some(self.camera_node);
        self.camera_rig.update(ctx, self.camera_node, dt)?;
        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
        ctx.renderer
            .render_scene(ctx.gpu, ctx.frame, ctx.scene, ctx.assets, ctx.active_camera)?;
        Ok(())
    }

    fn update_overlay(&mut self, ctx: &mut OverlayUpdateContext<'_>) -> Result<()> {
        self.debug_hud.update(ctx)
    }

    fn on_window_event(&mut self, ctx: &mut UpdateContext<'_>, event: &WindowEvent) -> Result<()> {
        if let WindowEvent::KeyboardInput { event, .. } = event {
            if matches!(
                event.physical_key,
                rig_app::winit::keyboard::PhysicalKey::Code(KeyCode::Escape)
            ) && event.state == rig_app::winit::event::ElementState::Pressed
            {
                ctx.request_exit();
            }
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<TexturedMeshApp>(rig_app::RunConfig {
        title: "Textured Mesh".into(),
        ..Default::default()
    })
}
