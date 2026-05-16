//! Camera-driven chunked terrain demo.
//!
//! `UpdateContext.assets` is immutable, so this example pre-generates every
//! chunk inside the unload radius during startup and then toggles scene-node
//! visibility as the camera crosses chunk boundaries. A production terrain
//! system would typically use async loading or DynamicMesh pooling to avoid the
//! startup cost and support truly infinite terrain. Debug builds use a smaller
//! radius and lower per-chunk resolution so `cargo run` reaches the first frame
//! quickly; release builds use the full demo settings.
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

use std::{collections::HashMap, sync::Arc, time::Instant};

use anyhow::Result;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, Side, StartupContext,
    TrackBall, UpdateContext,
    rig_assets::{
        ChunkCoord, ChunkManager, MaterialAsset, MaterialParams, ShaderAsset, mesh_factory,
    },
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::PBR_SHADER,
    rig_scene::{
        CameraComponent, LightComponent, LightKind, MeshSource, NodeId, Renderable, VisibilityMode,
    },
    winit::{
        event::WindowEvent,
        keyboard::{KeyCode, PhysicalKey},
    },
};

const CHUNK_SIZE: f32 = 64.0;
const RELEASE_LOAD_RADIUS: u32 = 4;
const RELEASE_UNLOAD_RADIUS: u32 = 6;
const DEBUG_LOAD_RADIUS: u32 = 2;
const DEBUG_UNLOAD_RADIUS: u32 = 3;
const LOAD_RADIUS: u32 = if cfg!(debug_assertions) {
    DEBUG_LOAD_RADIUS
} else {
    RELEASE_LOAD_RADIUS
};
const UNLOAD_RADIUS: u32 = if cfg!(debug_assertions) {
    DEBUG_UNLOAD_RADIUS
} else {
    RELEASE_UNLOAD_RADIUS
};
const RELEASE_CHUNK_COLS: u32 = 32;
const RELEASE_CHUNK_ROWS: u32 = 32;
const DEBUG_CHUNK_COLS: u32 = 24;
const DEBUG_CHUNK_ROWS: u32 = 24;
const CHUNK_COLS: u32 = if cfg!(debug_assertions) {
    DEBUG_CHUNK_COLS
} else {
    RELEASE_CHUNK_COLS
};
const CHUNK_ROWS: u32 = if cfg!(debug_assertions) {
    DEBUG_CHUNK_ROWS
} else {
    RELEASE_CHUNK_ROWS
};
const HEIGHT_SCALE: f32 = 12.0;
const WARP_AMPLITUDE: f32 = 80.0;

struct TerrainChunksApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    trackball: TrackBall,
    chunk_manager: ChunkManager,
    chunks: HashMap<ChunkCoord, NodeId>,
    debug_hud: DebugHud,
    active_chunks: rig_app::rig_overlay::ElementId,
}

impl Application for TerrainChunksApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let startup_timer = Instant::now();
        let mut chunk_manager = ChunkManager::new(CHUNK_SIZE, LOAD_RADIUS, UNLOAD_RADIUS);
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PBR_SHADER),
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams {
                diffuse: [0.50, 0.44, 0.36, 1.0],
                metallic: 0.0,
                roughness: 0.85,
                ..Default::default()
            },
            textures: vec![],
        });

        let warped_height = make_warped_height_fn();
        let mut chunks = HashMap::new();
        let radius = UNLOAD_RADIUS as i32;
        let chunk_timer = Instant::now();
        let chunk_count = (UNLOAD_RADIUS * 2 + 1).pow(2);
        eprintln!(
            "terrain_chunks: generating {chunk_count} chunks at {CHUNK_COLS}x{CHUNK_ROWS} cells"
        );
        for z in -radius..=radius {
            for x in -radius..=radius {
                let coord = ChunkCoord { x, z };
                let (center_x, center_z) = chunk_manager.chunk_center(coord);
                let height_fn = |local_x: f32, local_z: f32| -> f32 {
                    warped_height(center_x + local_x, center_z + local_z)
                };
                let mesh = mesh_factory::create_terrain_mesh(
                    CHUNK_SIZE, CHUNK_SIZE, CHUNK_COLS, CHUNK_ROWS, &height_fn,
                );
                let mesh_handle = ctx.assets.add_mesh(mesh);
                let node = ctx
                    .scene
                    .create_node(format!("chunk_{},{}", coord.x, coord.z));
                ctx.scene.set_renderable(
                    node,
                    Renderable {
                        mesh: MeshSource::Static(mesh_handle),
                        material,
                    },
                )?;
                ctx.scene.set_local_transform(
                    node,
                    Transform {
                        translation: Vec3::new(center_x, 0.0, center_z),
                        ..Default::default()
                    },
                )?;
                ctx.scene.set_visibility(node, VisibilityMode::Hidden)?;
                chunks.insert(coord, node);
            }
        }
        eprintln!(
            "terrain_chunks: generated {} chunks in {:?}",
            chunks.len(),
            chunk_timer.elapsed()
        );

        let update = chunk_manager.initialize(0.0, 0.0);
        for coord in update.to_create {
            if let Some(node) = chunks.get(&coord) {
                ctx.scene.set_visibility(*node, VisibilityMode::Inherit)?;
            }
        }

        add_sun(ctx)?;
        let camera_node = add_camera(ctx)?;
        let camera_pos = ctx.scene.local_transform(camera_node)?.translation;

        let target_node = ctx.scene.create_node("terrain_focus");
        ctx.scene
            .set_local_transform(target_node, Transform::IDENTITY)?;

        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        debug_hud.add_element(ctx.overlay, Side::Left, "Terrain — Chunked (Infinite)");
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            format!(
                "Generated chunks: {}, cells/chunk: {CHUNK_COLS}×{CHUNK_ROWS}",
                chunks.len()
            ),
        );
        let active_chunks = debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            format!("Active chunks: {}", chunk_manager.active_count()),
        );
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            "WASD: fly fast  LMB: orbit  RMB: dolly",
        );

        Ok(Self {
            camera_node,
            camera_rig: CameraRig {
                translation_speed: 20.0,
                rotation_speed: 1.5,
            },
            trackball: TrackBall::new(target_node, camera_pos.length()),
            chunk_manager,
            chunks,
            debug_hud,
            active_chunks,
        })
        .inspect(|_| {
            eprintln!(
                "terrain_chunks: startup completed in {:?}",
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

        let cam_pos = ctx.scene.local_transform(self.camera_node)?.translation;
        let update = self.chunk_manager.update(cam_pos.x, cam_pos.z);
        for coord in update.to_create {
            if let Some(node) = self.chunks.get(&coord) {
                ctx.scene.set_visibility(*node, VisibilityMode::Inherit)?;
            }
        }
        for coord in update.to_destroy {
            if let Some(node) = self.chunks.get(&coord) {
                ctx.scene.set_visibility(*node, VisibilityMode::Hidden)?;
            }
        }
        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
        ctx.renderer
            .render_scene(ctx.gpu, ctx.frame, ctx.scene, ctx.assets, ctx.active_camera)?;
        Ok(())
    }

    fn update_overlay(&mut self, ctx: &mut OverlayUpdateContext<'_>) -> Result<()> {
        self.debug_hud.update(ctx)?;
        ctx.set_text(
            self.active_chunks,
            format!("Active chunks: {}", self.chunk_manager.active_count()),
        )
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

fn add_camera(ctx: &mut StartupContext<'_>) -> Result<NodeId> {
    let camera_node = ctx.scene.create_node("camera");
    ctx.scene.set_local_transform(
        camera_node,
        Transform {
            translation: Vec3::new(0.0, 40.0, 0.0),
            rotation: Quat::from_rotation_x(-0.4),
            scale: Vec3::ONE,
        },
    )?;
    ctx.scene.set_camera(
        camera_node,
        CameraComponent {
            projection: Projection::Perspective {
                fov_y_radians: 60.0_f32.to_radians(),
                near: 0.1,
                far: 600.0,
            },
        },
    )?;
    Ok(camera_node)
}

fn add_sun(ctx: &mut StartupContext<'_>) -> Result<()> {
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
    Ok(())
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

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<TerrainChunksApp>(rig_app::RunConfig {
        title: "Terrain (Chunked Infinite)".into(),
        ..Default::default()
    })
}
