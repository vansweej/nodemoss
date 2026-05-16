//! Domain-warped heightmap terrain demo with a procedural normal map.
//!
//! Domain warping uses one noise field to bend the lookup coordinates of
//! another. Instead of sampling fBm at `(x, z)` directly, this demo first
//! samples a low-frequency warp field, offsets the domain by those values, and
//! then samples the terrain field at the warped coordinate. This turns regular
//! fBm hills into folded ridges and broad carved-looking terrain features.
//! See Inigo Quilez, <https://iquilezles.org/articles/warp/>.
//! Debug builds use smaller startup workloads so `cargo run` reaches the first
//! frame quickly; release builds use the full demo settings.
//!
//! # Controls
//!
//! | Input         | Action                      |
//! |---------------|-----------------------------|
//! | W / S         | Move forward / backward     |
//! | A / D         | Strafe left / right         |
//! | Q / E         | Move up / down              |
//! | Arrow keys    | Rotate camera (yaw / pitch) |
//! | LMB drag      | Orbit terrain               |
//! | RMB drag      | Dolly (zoom)                |
//! | F3            | Toggle overlay              |
//! | Escape        | Quit                        |

use std::{sync::Arc, time::Instant};

use anyhow::Result;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
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

const TERRAIN_WIDTH: f32 = 192.0;
const TERRAIN_DEPTH: f32 = 192.0;
const RELEASE_TERRAIN_COLS: u32 = 160;
const RELEASE_TERRAIN_ROWS: u32 = 160;
const DEBUG_TERRAIN_COLS: u32 = 96;
const DEBUG_TERRAIN_ROWS: u32 = 96;
const TERRAIN_COLS: u32 = if cfg!(debug_assertions) {
    DEBUG_TERRAIN_COLS
} else {
    RELEASE_TERRAIN_COLS
};
const TERRAIN_ROWS: u32 = if cfg!(debug_assertions) {
    DEBUG_TERRAIN_ROWS
} else {
    RELEASE_TERRAIN_ROWS
};
const HEIGHT_SCALE: f32 = 12.0;
const WARP_AMPLITUDE: f32 = 80.0;
const RELEASE_NORMAL_MAP_SIZE: u32 = 512;
const DEBUG_NORMAL_MAP_SIZE: u32 = 256;
const NORMAL_MAP_SIZE: u32 = if cfg!(debug_assertions) {
    DEBUG_NORMAL_MAP_SIZE
} else {
    RELEASE_NORMAL_MAP_SIZE
};
const NORMAL_MAP_STRENGTH: f32 = 0.45;

struct TerrainWarpApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    trackball: TrackBall,
    debug_hud: DebugHud,
}

impl Application for TerrainWarpApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let startup_timer = Instant::now();
        let height_fn = make_warped_height_fn();
        let mesh_timer = Instant::now();
        eprintln!("terrain_warp: building {TERRAIN_COLS}x{TERRAIN_ROWS} terrain mesh");
        let terrain_mesh = mesh_factory::create_terrain_mesh(
            TERRAIN_WIDTH,
            TERRAIN_DEPTH,
            TERRAIN_COLS,
            TERRAIN_ROWS,
            &height_fn,
        );
        eprintln!("terrain_warp: mesh built in {:?}", mesh_timer.elapsed());
        let mesh = ctx.assets.add_mesh(terrain_mesh);

        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PBR_SHADER),
        });
        let normal_timer = Instant::now();
        eprintln!("terrain_warp: generating {NORMAL_MAP_SIZE}x{NORMAL_MAP_SIZE} normal map");
        let normal_map = ctx.assets.add_texture(TextureAsset {
            width: NORMAL_MAP_SIZE,
            height: NORMAL_MAP_SIZE,
            format: TextureFormat::Rgba8Unorm,
            data: Arc::from(generate_noise_normal_map().as_slice()),
        });
        eprintln!(
            "terrain_warp: normal map generated in {:?}",
            normal_timer.elapsed()
        );
        let sampler = ctx.assets.add_sampler(SamplerDescriptor {
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams {
                diffuse: [0.52, 0.45, 0.35, 1.0],
                metallic: 0.0,
                roughness: 0.85,
                ..Default::default()
            },
            textures: vec![None, Some((normal_map, sampler)), None, None, None],
        });

        let terrain_node = ctx.scene.create_node("domain_warped_terrain");
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
                translation: Vec3::new(-40.0, 20.0, -30.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_light(
            fill_node,
            LightComponent {
                kind: LightKind::Point {
                    color: Vec3::new(0.55, 0.68, 1.0),
                    intensity: 900.0,
                    range: 160.0,
                },
            },
        )?;

        let camera_node = ctx.scene.create_node("camera");
        let eye = Vec3::new(0.0, 28.0, 64.0);
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
                    far: 400.0,
                },
            },
        )?;

        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        debug_hud.add_element(ctx.overlay, Side::Left, "Terrain — Domain Warping");
        debug_hud.add_element(ctx.overlay, Side::Left, "Warp amplitude: 80.0");
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            format!("Grid: {TERRAIN_COLS}×{TERRAIN_ROWS}, normal map: {NORMAL_MAP_SIZE}²"),
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
        .inspect(|_| {
            eprintln!(
                "terrain_warp: startup completed in {:?}",
                startup_timer.elapsed()
            );
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

fn make_warped_height_fn() -> impl Fn(f32, f32) -> f32 {
    let terrain_fbm = Fbm::<Perlin>::new(42)
        .set_octaves(6)
        .set_frequency(0.8)
        .set_persistence(0.45);
    let warp_fbm = Fbm::<Perlin>::new(7)
        .set_octaves(4)
        .set_frequency(1.2)
        .set_persistence(0.5);

    move |x: f32, z: f32| -> f32 {
        let warp_x = warp_fbm.get([x as f64 * 0.1, z as f64 * 0.1]);
        let warp_z = warp_fbm.get([x as f64 * 0.1 + 5.2, z as f64 * 0.1 + 1.3]);
        terrain_fbm.get([
            (x as f64 + warp_x * WARP_AMPLITUDE as f64) * 0.01,
            (z as f64 + warp_z * WARP_AMPLITUDE as f64) * 0.01,
        ]) as f32
            * HEIGHT_SCALE
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
    rig_app::run::<TerrainWarpApp>(rig_app::RunConfig {
        title: "Terrain (Domain Warping)".into(),
        ..Default::default()
    })
}
