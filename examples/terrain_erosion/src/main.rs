//! Hydraulic erosion heightmap terrain demo.
//!
//! This demo starts from domain-warped fBm terrain and then runs a CPU droplet
//! hydraulic erosion pass over the height grid during startup. Erosion
//! transforms noisy bumps into more geologically plausible terrain: valleys tend
//! to follow drainage paths, ridgelines emerge between flows, and flatter
//! deposited areas appear around slope bases. Debug builds use fewer droplets so
//! `cargo run` reaches the first frame quickly; release builds use the full
//! 100,000-droplet pass. The technique is inspired by Sebastian Lague's
//! hydraulic erosion work and Benes & Forsbach (2002), *Visual Simulation of
//! Hydraulic Erosion*.
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
        AddressMode, ErosionParams, FilterMode, MaterialAsset, MaterialParams, SamplerDescriptor,
        ShaderAsset, TextureAsset, TextureFormat, erode, mesh_factory,
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
const TERRAIN_COLS: u32 = 128;
const TERRAIN_ROWS: u32 = 128;
const HEIGHT_SCALE: f32 = 14.0;
const WARP_AMPLITUDE: f32 = 80.0;
const NORMAL_MAP_SIZE: u32 = 256;
const NORMAL_SAMPLE_DISTANCE: f32 = 0.25;
const NORMAL_STRENGTH: f32 = 3.0;
const RELEASE_EROSION_ITERATIONS: u32 = 20_000;
const DEBUG_EROSION_ITERATIONS: u32 = 2_000;
const EROSION_ITERATIONS: u32 = if cfg!(debug_assertions) {
    DEBUG_EROSION_ITERATIONS
} else {
    RELEASE_EROSION_ITERATIONS
};
const EROSION_MAX_LIFETIME: u32 = 24;
const EROSION_RADIUS: u32 = 2;

struct TerrainErosionApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    trackball: TrackBall,
    debug_hud: DebugHud,
}

impl Application for TerrainErosionApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let startup_timer = Instant::now();
        eprintln!("terrain_erosion: generating initial height grid");
        let mut heights = generate_initial_heights();
        eprintln!(
            "terrain_erosion: height grid generated in {:?}",
            startup_timer.elapsed()
        );

        let erosion_timer = Instant::now();
        eprintln!(
            "terrain_erosion: running {EROSION_ITERATIONS} droplets, lifetime {EROSION_MAX_LIFETIME}, radius {EROSION_RADIUS}"
        );
        erode(
            &mut heights,
            TERRAIN_COLS + 1,
            TERRAIN_ROWS + 1,
            &ErosionParams {
                iterations: EROSION_ITERATIONS,
                max_lifetime: EROSION_MAX_LIFETIME,
                erosion_radius: EROSION_RADIUS,
                ..Default::default()
            },
        );
        clamp_heights(&mut heights);
        eprintln!(
            "terrain_erosion: erosion completed in {:?}",
            erosion_timer.elapsed()
        );

        let height_fn = |x: f32, z: f32| -> f32 { sample_eroded_height(&heights, x, z) };
        let mesh_timer = Instant::now();
        eprintln!("terrain_erosion: building {TERRAIN_COLS}x{TERRAIN_ROWS} terrain mesh");
        let terrain_mesh = mesh_factory::create_terrain_mesh(
            TERRAIN_WIDTH,
            TERRAIN_DEPTH,
            TERRAIN_COLS,
            TERRAIN_ROWS,
            &height_fn,
        );
        eprintln!("terrain_erosion: mesh built in {:?}", mesh_timer.elapsed());
        let mesh = ctx.assets.add_mesh(terrain_mesh);

        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PBR_SHADER),
        });
        let normal_timer = Instant::now();
        eprintln!("terrain_erosion: generating {NORMAL_MAP_SIZE}x{NORMAL_MAP_SIZE} normal map");
        let normal_map = ctx.assets.add_texture(TextureAsset {
            width: NORMAL_MAP_SIZE,
            height: NORMAL_MAP_SIZE,
            format: TextureFormat::Rgba8Unorm,
            data: Arc::from(generate_eroded_normal_map(&height_fn).as_slice()),
        });
        eprintln!(
            "terrain_erosion: normal map generated in {:?}",
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
                diffuse: [0.50, 0.44, 0.36, 1.0],
                metallic: 0.0,
                roughness: 0.9,
                ..Default::default()
            },
            textures: vec![None, Some((normal_map, sampler)), None, None, None],
        });

        let terrain_node = ctx.scene.create_node("eroded_terrain");
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

        add_lights(ctx)?;

        let camera_node = ctx.scene.create_node("camera");
        let eye = Vec3::new(0.0, 32.0, 72.0);
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
                    far: 500.0,
                },
            },
        )?;

        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        debug_hud.add_element(ctx.overlay, Side::Left, "Terrain — Hydraulic Erosion");
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            format!("{EROSION_ITERATIONS} droplets, radius {EROSION_RADIUS}"),
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

fn clamp_heights(heights: &mut [f32]) {
    let min_height = -HEIGHT_SCALE * 4.0;
    let max_height = HEIGHT_SCALE * 4.0;
    for height in heights {
        *height = height.clamp(min_height, max_height);
    }
}

fn add_lights(ctx: &mut StartupContext<'_>) -> Result<()> {
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
    Ok(())
}

fn generate_initial_heights() -> Vec<f32> {
    let warped_height = make_warped_height_fn();
    let cols = TERRAIN_COLS + 1;
    let rows = TERRAIN_ROWS + 1;
    let cell_width = TERRAIN_WIDTH / TERRAIN_COLS as f32;
    let cell_depth = TERRAIN_DEPTH / TERRAIN_ROWS as f32;
    let mut heights = vec![0.0_f32; (cols * rows) as usize];

    for row in 0..rows {
        for col in 0..cols {
            let x = -TERRAIN_WIDTH / 2.0 + col as f32 * cell_width;
            let z = -TERRAIN_DEPTH / 2.0 + row as f32 * cell_depth;
            heights[(row * cols + col) as usize] = warped_height(x, z);
        }
    }

    heights
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

fn sample_eroded_height(heights: &[f32], x: f32, z: f32) -> f32 {
    let cols = (TERRAIN_COLS + 1) as usize;
    let rows = (TERRAIN_ROWS + 1) as usize;
    let grid_x = ((x + TERRAIN_WIDTH * 0.5) / TERRAIN_WIDTH * TERRAIN_COLS as f32)
        .clamp(0.0, TERRAIN_COLS as f32);
    let grid_z = ((z + TERRAIN_DEPTH * 0.5) / TERRAIN_DEPTH * TERRAIN_ROWS as f32)
        .clamp(0.0, TERRAIN_ROWS as f32);

    let cell_x = grid_x.floor().min((cols - 2) as f32) as usize;
    let cell_z = grid_z.floor().min((rows - 2) as f32) as usize;
    let tx = grid_x - cell_x as f32;
    let tz = grid_z - cell_z as f32;

    let h00 = heights[cell_z * cols + cell_x];
    let h10 = heights[cell_z * cols + cell_x + 1];
    let h01 = heights[(cell_z + 1) * cols + cell_x];
    let h11 = heights[(cell_z + 1) * cols + cell_x + 1];

    h00 * (1.0 - tx) * (1.0 - tz) + h10 * tx * (1.0 - tz) + h01 * (1.0 - tx) * tz + h11 * tx * tz
}

fn generate_eroded_normal_map(height_fn: &dyn Fn(f32, f32) -> f32) -> Vec<u8> {
    let mut pixels = vec![0_u8; (NORMAL_MAP_SIZE * NORMAL_MAP_SIZE * 4) as usize];

    for row in 0..NORMAL_MAP_SIZE {
        for col in 0..NORMAL_MAP_SIZE {
            let u = col as f32 / NORMAL_MAP_SIZE as f32;
            let v = row as f32 / NORMAL_MAP_SIZE as f32;
            let x = -TERRAIN_WIDTH * 0.5 + u * TERRAIN_WIDTH;
            let z = -TERRAIN_DEPTH * 0.5 + v * TERRAIN_DEPTH;

            let h_right = height_fn(x + NORMAL_SAMPLE_DISTANCE, z);
            let h_left = height_fn(x - NORMAL_SAMPLE_DISTANCE, z);
            let h_up = height_fn(x, z + NORMAL_SAMPLE_DISTANCE);
            let h_down = height_fn(x, z - NORMAL_SAMPLE_DISTANCE);
            let dx = (h_right - h_left) / (2.0 * NORMAL_SAMPLE_DISTANCE * NORMAL_STRENGTH);
            let dz = (h_up - h_down) / (2.0 * NORMAL_SAMPLE_DISTANCE * NORMAL_STRENGTH);
            let normal = Vec3::new(-dx, -dz, 1.0).normalize_or_zero();
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
    rig_app::run::<TerrainErosionApp>(rig_app::RunConfig {
        title: "Terrain (Hydraulic Erosion)".into(),
        ..Default::default()
    })
}
