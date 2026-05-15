//! PBR normal-map demo.
//!
//! Renders side-by-side PBR planes so the procedurally generated tangent-space
//! normal map can be compared against a flat material under the same light.
//!
//! # Controls
//!
//! | Input          | Action                |
//! |----------------|-----------------------|
//! | WASD / arrows  | Fly camera            |
//! | LMB drag       | Orbit comparison      |
//! | RMB drag       | Dolly (zoom)          |
//! | Escape         | Quit                  |
//! | F3             | Toggle overlay        |

use std::sync::Arc;

use anyhow::Result;
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, Side, StartupContext,
    TrackBall, UpdateContext,
    rig_assets::{
        AddressMode, FilterMode, MaterialAsset, MaterialParams, SamplerDescriptor, ShaderAsset,
        TextureAsset, TextureFormat, mesh_factory,
    },
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::PBR_SHADER,
    rig_scene::{CameraComponent, LightComponent, LightKind, MeshSource, NodeId, Renderable},
    winit::{
        event::WindowEvent,
        keyboard::{KeyCode, PhysicalKey},
    },
};

const NORMAL_MAP_SIZE: u32 = 128;
const NORMAL_MAP_FREQUENCY: f32 = 10.0;
const NORMAL_MAP_STRENGTH: f32 = 1.65;
const PLANE_SIZE: f32 = 4.0;
const PLANE_X_OFFSET: f32 = 2.6;

struct NormalMapDemo {
    camera_node: NodeId,
    light_node: NodeId,
    camera_rig: CameraRig,
    trackball: TrackBall,
    debug_hud: DebugHud,
    elapsed: f32,
}

impl Application for NormalMapDemo {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PBR_SHADER),
        });

        let base_color = ctx.assets.add_texture(TextureAsset {
            width: 1,
            height: 1,
            format: TextureFormat::Rgba8Unorm,
            data: Arc::from([255_u8, 255, 255, 255]),
        });
        let normal_map = ctx.assets.add_texture(TextureAsset {
            width: NORMAL_MAP_SIZE,
            height: NORMAL_MAP_SIZE,
            format: TextureFormat::Rgba8Unorm,
            data: Arc::from(generate_normal_map().as_slice()),
        });
        let sampler = ctx.assets.add_sampler(SamplerDescriptor {
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
        });

        let material_params = MaterialParams {
            diffuse: [0.7, 0.62, 0.48, 1.0],
            metallic: 0.0,
            roughness: 0.26,
            ..Default::default()
        };

        let flat_material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: material_params,
            textures: vec![Some((base_color, sampler))],
        });
        let normal_mapped_material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: material_params,
            textures: vec![Some((base_color, sampler)), Some((normal_map, sampler))],
        });

        let plane_mesh = ctx
            .assets
            .add_mesh(mesh_factory::create_plane(PLANE_SIZE, PLANE_SIZE));

        let target_node = ctx.scene.create_node("comparison_focus");
        ctx.scene.set_local_transform(
            target_node,
            Transform {
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;

        let flat_plane = ctx.scene.create_node("flat_reference_plane");
        ctx.scene.set_renderable(
            flat_plane,
            Renderable {
                mesh: MeshSource::Static(plane_mesh),
                material: flat_material,
            },
        )?;
        ctx.scene.set_local_transform(
            flat_plane,
            Transform {
                translation: Vec3::new(-PLANE_X_OFFSET, 0.0, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;

        let normal_mapped_plane = ctx.scene.create_node("normal_mapped_plane");
        ctx.scene.set_renderable(
            normal_mapped_plane,
            Renderable {
                mesh: MeshSource::Static(plane_mesh),
                material: normal_mapped_material,
            },
        )?;
        ctx.scene.set_local_transform(
            normal_mapped_plane,
            Transform {
                translation: Vec3::new(PLANE_X_OFFSET, 0.0, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;

        let light_node = ctx.scene.create_node("orbiting_key_light");
        ctx.scene.set_local_transform(
            light_node,
            Transform {
                translation: Vec3::new(0.0, 1.15, 3.8),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_light(
            light_node,
            LightComponent {
                kind: LightKind::Point {
                    color: Vec3::new(1.0, 0.96, 0.9),
                    intensity: 36.0,
                    range: 9.0,
                },
            },
        )?;

        let camera_node = ctx.scene.create_node("camera");
        let eye = Vec3::new(0.0, 4.0, 8.0);
        let pitch = -eye.y.atan2(eye.z);
        ctx.scene.set_local_transform(
            camera_node,
            Transform {
                translation: eye,
                rotation: Quat::from_rotation_x(pitch),
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_camera(
            camera_node,
            CameraComponent {
                projection: Projection::Perspective {
                    fov_y_radians: 55.0_f32.to_radians(),
                    near: 0.1,
                    far: 100.0,
                },
            },
        )?;

        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        debug_hud.add_element(ctx.overlay, Side::Left, "Left: flat PBR reference");
        debug_hud.add_element(ctx.overlay, Side::Left, "Right: same material + normal map");
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            "Low orbiting light exaggerates bumps",
        );
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            "WASD/arrows: fly  LMB: orbit  RMB: dolly",
        );

        Ok(Self {
            camera_node,
            light_node,
            camera_rig: CameraRig {
                translation_speed: 4.0,
                rotation_speed: 1.5,
            },
            trackball: TrackBall::new(target_node, eye.length()),
            debug_hud,
            elapsed: 0.0,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        *ctx.active_camera = Some(self.camera_node);
        self.camera_rig.update(ctx, self.camera_node, dt)?;
        self.trackball.sync_to_camera(ctx.scene, self.camera_node)?;
        self.trackball
            .update(ctx.input, ctx.scene, self.camera_node, dt)?;
        self.elapsed += dt;

        let radius = 4.4;
        ctx.scene.set_local_transform(
            self.light_node,
            Transform {
                translation: Vec3::new(
                    radius * (self.elapsed * 0.85).cos(),
                    1.15,
                    0.35 + radius * (self.elapsed * 0.85).sin(),
                ),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;
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
            if matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
                && event.state == rig_app::winit::event::ElementState::Pressed
            {
                ctx.request_exit();
            }
        }
        Ok(())
    }
}

fn generate_normal_map() -> Vec<u8> {
    let mut pixels = vec![0_u8; (NORMAL_MAP_SIZE * NORMAL_MAP_SIZE * 4) as usize];
    let scale = std::f32::consts::TAU * NORMAL_MAP_FREQUENCY;
    for y in 0..NORMAL_MAP_SIZE {
        for x in 0..NORMAL_MAP_SIZE {
            let u = x as f32 / NORMAL_MAP_SIZE as f32;
            let v = y as f32 / NORMAL_MAP_SIZE as f32;
            let wave_u = u * scale;
            let wave_v = v * scale;
            let diagonal_wave = (u + v) * scale * 0.5;
            let dhdu = wave_u.cos() * wave_v.sin() + 0.35 * diagonal_wave.cos();
            let dhdv = wave_u.sin() * wave_v.cos() + 0.35 * diagonal_wave.cos();
            let normal = Vec3::new(
                -dhdu * NORMAL_MAP_STRENGTH,
                -dhdv * NORMAL_MAP_STRENGTH,
                1.0,
            )
            .normalize();
            let idx = ((y * NORMAL_MAP_SIZE + x) * 4) as usize;
            pixels[idx] = ((normal.x * 0.5 + 0.5) * 255.0) as u8;
            pixels[idx + 1] = ((normal.y * 0.5 + 0.5) * 255.0) as u8;
            pixels[idx + 2] = ((normal.z * 0.5 + 0.5) * 255.0) as u8;
            pixels[idx + 3] = 255;
        }
    }
    pixels
}

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<NormalMapDemo>(rig_app::RunConfig {
        title: "Normal Map Demo".into(),
        ..Default::default()
    })
}
