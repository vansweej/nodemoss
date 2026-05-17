//! Heightmap terrain demo with a procedural normal map.
//!
//! A 2-D fBm noise function displaces a regular XZ grid into rolling terrain.
//! A second, higher-frequency noise function is baked into a tangent-space
//! normal map at startup and bound to the Phase-A PBR normal-map slot for fine
//! rock-grain detail without adding triangles.
//!
//! # Controls
//!
//! | Input          | Action                |
//! |----------------|-----------------------|
//! | WASD / arrows  | Fly camera            |
//! | LMB drag       | Orbit terrain         |
//! | RMB drag       | Dolly (zoom)          |
//! | Escape         | Quit                  |
//! | F3             | Toggle overlay        |

use std::sync::Arc;

use anyhow::Result;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, Side, StartupContext,
    TrackBall, UpdateContext,
    rig_assets::{
        AddressMode, AlphaMode, FilterMode, MaterialAsset, MaterialParams, SamplerDescriptor,
        ShaderAsset, TextureAsset, TextureFormat, mesh_factory,
    },
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::PBR_SHADER,
    rig_scene::{CameraComponent, LightComponent, LightKind, MeshSource, NodeId, Renderable},
    winit::{
        event::WindowEvent,
        keyboard::{KeyCode, PhysicalKey},
    },
};

const TERRAIN_WIDTH: f32 = 128.0;
const TERRAIN_DEPTH: f32 = 128.0;
const TERRAIN_COLS: u32 = 128;
const TERRAIN_ROWS: u32 = 128;
const HEIGHT_SCALE: f32 = 8.0;
const NORMAL_MAP_SIZE: u32 = 512;
const NORMAL_MAP_STRENGTH: f32 = 0.45;

struct TerrainHeightmapApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    trackball: TrackBall,
    debug_hud: DebugHud,
}

impl Application for TerrainHeightmapApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let terrain_noise = Fbm::<Perlin>::new(42)
            .set_octaves(6)
            .set_frequency(0.8)
            .set_persistence(0.45);

        let height_fn = |x: f32, z: f32| -> f32 {
            terrain_noise.get([x as f64 * 0.02, z as f64 * 0.02]) as f32 * HEIGHT_SCALE
        };

        let terrain_mesh = mesh_factory::create_terrain_mesh(
            TERRAIN_WIDTH,
            TERRAIN_DEPTH,
            TERRAIN_COLS,
            TERRAIN_ROWS,
            &height_fn,
        );
        let mesh = ctx.assets.add_mesh(terrain_mesh);

        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PBR_SHADER),
        });
        let normal_map = ctx.assets.add_texture(TextureAsset {
            width: NORMAL_MAP_SIZE,
            height: NORMAL_MAP_SIZE,
            format: TextureFormat::Rgba8Unorm,
            data: Arc::from(generate_noise_normal_map().as_slice()),
        });
        let sampler = ctx.assets.add_sampler(SamplerDescriptor {
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams {
                diffuse: [0.55, 0.48, 0.38, 1.0],
                metallic: 0.0,
                roughness: 0.9,
                ..Default::default()
            },
            // Implementation slot order: 0=base color, 1=normal,
            // 2=metallic-roughness, 3=occlusion, 4=emissive.
            textures: vec![None, Some((normal_map, sampler)), None, None, None],
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
        });

        let terrain_node = ctx.scene.create_node("heightmap_terrain");
        ctx.scene.set_renderable(
            terrain_node,
            Renderable {
                mesh: MeshSource::Static(mesh),
                material,
            },
        )?;
        ctx.scene
            .set_local_transform(terrain_node, Transform::IDENTITY)?;

        let target_node = ctx.scene.create_node("terrain_focus");
        ctx.scene
            .set_local_transform(target_node, Transform::IDENTITY)?;

        let sun_node = ctx.scene.create_node("sun_directional_light");
        ctx.scene.set_local_transform(
            sun_node,
            Transform {
                translation: Vec3::ZERO,
                rotation: Quat::from_rotation_x(-0.8) * Quat::from_rotation_y(-0.35),
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_light(
            sun_node,
            LightComponent {
                kind: LightKind::Directional {
                    color: Vec3::new(1.0, 0.94, 0.82),
                    intensity: 2.2,
                },
            },
        )?;

        let fill_node = ctx.scene.create_node("cool_fill_light");
        ctx.scene.set_local_transform(
            fill_node,
            Transform {
                translation: Vec3::new(-30.0, 18.0, -22.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_light(
            fill_node,
            LightComponent {
                kind: LightKind::Point {
                    color: Vec3::new(0.55, 0.68, 1.0),
                    intensity: 850.0,
                    range: 140.0,
                },
            },
        )?;

        let camera_node = ctx.scene.create_node("camera");
        let eye = Vec3::new(0.0, 24.0, 54.0);
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
                    fov_y_radians: 60.0_f32.to_radians(),
                    near: 0.1,
                    far: 300.0,
                },
            },
        )?;

        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        debug_hud.add_element(ctx.overlay, Side::Left, "Terrain — Heightmap");
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            "Geometry: 2-D fBm heightmap (128×128 cells)",
        );
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            "Material: procedural noise normal map",
        );
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            "WASD/arrows: fly  LMB: orbit  RMB: dolly",
        );

        Ok(Self {
            camera_node,
            camera_rig: CameraRig {
                translation_speed: 12.0,
                rotation_speed: 1.5,
            },
            trackball: TrackBall::new(target_node, eye.length()),
            debug_hud,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        *ctx.active_camera = Some(self.camera_node);
        self.camera_rig.update(ctx, self.camera_node, dt)?;
        self.trackball.sync_to_camera(ctx.scene, self.camera_node)?;
        self.trackball
            .update(ctx.input, ctx.scene, self.camera_node, dt)?;
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
        if let WindowEvent::KeyboardInput { event, .. } = event
            && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
            && event.state == rig_app::winit::event::ElementState::Pressed
        {
            ctx.request_exit();
        }
        Ok(())
    }
}

fn generate_noise_normal_map() -> Vec<u8> {
    let detail_noise = Fbm::<Perlin>::new(99)
        .set_octaves(4)
        .set_frequency(8.0)
        .set_persistence(0.5);
    let eps = 1.0 / NORMAL_MAP_SIZE as f64;
    let mut pixels = vec![0_u8; (NORMAL_MAP_SIZE * NORMAL_MAP_SIZE * 4) as usize];

    for row in 0..NORMAL_MAP_SIZE {
        for col in 0..NORMAL_MAP_SIZE {
            let u = col as f64 / NORMAL_MAP_SIZE as f64;
            let v = row as f64 / NORMAL_MAP_SIZE as f64;
            let h_right = detail_noise.get([u + eps, v]) as f32;
            let h_left = detail_noise.get([u - eps, v]) as f32;
            let h_up = detail_noise.get([u, v + eps]) as f32;
            let h_down = detail_noise.get([u, v - eps]) as f32;
            let dx = (h_right - h_left) / (2.0 * eps as f32 * NORMAL_MAP_STRENGTH);
            let dy = (h_up - h_down) / (2.0 * eps as f32 * NORMAL_MAP_STRENGTH);
            let normal = Vec3::new(-dx, -dy, 1.0).normalize_or_zero();
            let normal = if normal.length_squared() > 0.0 {
                normal
            } else {
                Vec3::Z
            };

            let idx = ((row * NORMAL_MAP_SIZE + col) * 4) as usize;
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
    rig_app::run::<TerrainHeightmapApp>(rig_app::RunConfig {
        title: "Terrain (Heightmap + Normal Map)".into(),
        ..Default::default()
    })
}
