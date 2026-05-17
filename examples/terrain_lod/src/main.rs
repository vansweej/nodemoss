//! Distance-based level-of-detail terrain chunks.
//!
//! Level of detail (LOD) reduces geometry density for distant terrain chunks.
//! This demo pre-generates three mesh resolutions per chunk and swaps scene-node
//! visibility based on camera distance. LOD colors are intentionally different:
//! green = near 64×64, brown = mid 32×32, grey = far 16×16. Geomorphing —
//! vertex-shader blending between LOD levels for pop-free transitions — is a
//! natural next step. Debug builds use fewer chunks and lower near/mid LOD
//! resolutions so `cargo run` reaches the first frame quickly; release builds
//! use the full demo settings.
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
        AlphaMode, ChunkCoord, ChunkManager, LodLevel, MaterialAsset, MaterialHandle,
        MaterialParams, ShaderAsset, mesh_factory, select_lod,
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
const RELEASE_LOAD_RADIUS: u32 = 5;
const RELEASE_UNLOAD_RADIUS: u32 = 7;
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
const HEIGHT_SCALE: f32 = 12.0;
const WARP_AMPLITUDE: f32 = 80.0;
const RELEASE_LOD_LEVELS: [LodLevel; 3] = [
    LodLevel {
        max_distance: 128.0,
        resolution: 64,
    },
    LodLevel {
        max_distance: 256.0,
        resolution: 32,
    },
    LodLevel {
        max_distance: 512.0,
        resolution: 16,
    },
];
const DEBUG_LOD_LEVELS: [LodLevel; 3] = [
    LodLevel {
        max_distance: 128.0,
        resolution: 32,
    },
    LodLevel {
        max_distance: 256.0,
        resolution: 24,
    },
    LodLevel {
        max_distance: 512.0,
        resolution: 16,
    },
];
const LOD_LEVELS: [LodLevel; 3] = if cfg!(debug_assertions) {
    DEBUG_LOD_LEVELS
} else {
    RELEASE_LOD_LEVELS
};

struct TerrainLodApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    trackball: TrackBall,
    chunk_manager: ChunkManager,
    chunk_nodes: HashMap<ChunkCoord, [NodeId; 3]>,
    current_lod: HashMap<ChunkCoord, usize>,
    debug_hud: DebugHud,
    active_chunks: rig_app::rig_overlay::ElementId,
    lod_counts: rig_app::rig_overlay::ElementId,
}

impl Application for TerrainLodApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let startup_timer = Instant::now();
        let mut chunk_manager = ChunkManager::new(CHUNK_SIZE, LOAD_RADIUS, UNLOAD_RADIUS);
        let materials = create_lod_materials(ctx);
        let warped_height = make_warped_height_fn();
        let mut chunk_nodes = HashMap::new();
        let mut current_lod = HashMap::new();
        let radius = UNLOAD_RADIUS as i32;
        let chunk_timer = Instant::now();
        let chunk_count = (UNLOAD_RADIUS * 2 + 1).pow(2);
        eprintln!(
            "terrain_lod: generating {chunk_count} chunks × {} LOD meshes",
            LOD_LEVELS.len()
        );

        for z in -radius..=radius {
            for x in -radius..=radius {
                let coord = ChunkCoord { x, z };
                let (center_x, center_z) = chunk_manager.chunk_center(coord);
                let node0 = create_lod_node(
                    ctx,
                    coord,
                    0,
                    center_x,
                    center_z,
                    materials[0],
                    &warped_height,
                )?;
                let node1 = create_lod_node(
                    ctx,
                    coord,
                    1,
                    center_x,
                    center_z,
                    materials[1],
                    &warped_height,
                )?;
                let node2 = create_lod_node(
                    ctx,
                    coord,
                    2,
                    center_x,
                    center_z,
                    materials[2],
                    &warped_height,
                )?;
                chunk_nodes.insert(coord, [node0, node1, node2]);
                current_lod.insert(coord, 2);
            }
        }
        eprintln!(
            "terrain_lod: generated {} chunks ({} meshes) in {:?}",
            chunk_nodes.len(),
            chunk_nodes.len() * LOD_LEVELS.len(),
            chunk_timer.elapsed()
        );

        let camera_node = add_camera(ctx)?;
        let camera_pos = ctx.scene.local_transform(camera_node)?.translation;
        let target_node = ctx.scene.create_node("terrain_focus");
        ctx.scene
            .set_local_transform(target_node, Transform::IDENTITY)?;
        let update = chunk_manager.initialize(camera_pos.x, camera_pos.z);
        for coord in update.to_create {
            let lod_index = lod_index_for_coord(&chunk_manager, coord, camera_pos);
            set_chunk_lod_startup(ctx, &chunk_nodes, &mut current_lod, coord, lod_index)?;
        }

        add_sun(ctx)?;

        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        debug_hud.add_element(ctx.overlay, Side::Left, "Terrain — LOD (Level of Detail)");
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            format!(
                "Generated chunks: {}, LOD resolutions: {}/{}/{}",
                chunk_nodes.len(),
                LOD_LEVELS[0].resolution,
                LOD_LEVELS[1].resolution,
                LOD_LEVELS[2].resolution
            ),
        );
        let active_chunks = debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            format!("Active chunks: {}", chunk_manager.active_count()),
        );
        let lod_counts = debug_hud.add_element(ctx.overlay, Side::Left, "LOD 0/1/2 visible: 0/0/0");
        debug_hud.add_element(ctx.overlay, Side::Left, "WASD: fly  LMB: orbit  RMB: dolly");

        Ok(Self {
            camera_node,
            camera_rig: CameraRig {
                translation_speed: 25.0,
                rotation_speed: 1.5,
            },
            trackball: TrackBall::new(target_node, camera_pos.length()),
            chunk_manager,
            chunk_nodes,
            current_lod,
            debug_hud,
            active_chunks,
            lod_counts,
        })
        .inspect(|_| {
            eprintln!(
                "terrain_lod: startup completed in {:?}",
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
            let lod_index = lod_index_for_coord(&self.chunk_manager, coord, cam_pos);
            set_chunk_lod(
                ctx,
                &self.chunk_nodes,
                &mut self.current_lod,
                coord,
                lod_index,
            )?;
        }
        for coord in update.to_destroy {
            hide_chunk(ctx, &self.chunk_nodes, coord)?;
        }

        let active: Vec<_> = self.chunk_manager.active_chunks().copied().collect();
        for coord in active {
            let lod_index = lod_index_for_coord(&self.chunk_manager, coord, cam_pos);
            if self.current_lod.get(&coord).copied() != Some(lod_index) {
                set_chunk_lod(
                    ctx,
                    &self.chunk_nodes,
                    &mut self.current_lod,
                    coord,
                    lod_index,
                )?;
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
        )?;
        let counts = self.visible_lod_counts();
        ctx.set_text(
            self.lod_counts,
            format!(
                "LOD 0/1/2 visible: {}/{}/{}",
                counts[0], counts[1], counts[2]
            ),
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

impl TerrainLodApp {
    fn visible_lod_counts(&self) -> [usize; 3] {
        let mut counts = [0_usize; 3];
        for coord in self.chunk_manager.active_chunks() {
            if let Some(index) = self.current_lod.get(coord).copied() {
                counts[index] += 1;
            }
        }
        counts
    }
}

fn create_lod_materials(ctx: &mut StartupContext<'_>) -> [MaterialHandle; 3] {
    let shader = ctx.assets.add_shader(ShaderAsset {
        source: Arc::from(PBR_SHADER),
    });
    [
        ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams {
                diffuse: [0.45, 0.55, 0.40, 1.0],
                metallic: 0.0,
                roughness: 0.85,
                ..Default::default()
            },
            textures: vec![],
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
        }),
        ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams {
                diffuse: [0.55, 0.48, 0.38, 1.0],
                metallic: 0.0,
                roughness: 0.85,
                ..Default::default()
            },
            textures: vec![],
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
        }),
        ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams {
                diffuse: [0.60, 0.55, 0.50, 1.0],
                metallic: 0.0,
                roughness: 0.85,
                ..Default::default()
            },
            textures: vec![],
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
        }),
    ]
}

fn create_lod_node(
    ctx: &mut StartupContext<'_>,
    coord: ChunkCoord,
    lod_index: usize,
    center_x: f32,
    center_z: f32,
    material: MaterialHandle,
    warped_height: &dyn Fn(f32, f32) -> f32,
) -> Result<NodeId> {
    let resolution = LOD_LEVELS[lod_index].resolution;
    let height_fn = |local_x: f32, local_z: f32| -> f32 {
        warped_height(center_x + local_x, center_z + local_z)
    };
    let mesh = mesh_factory::create_terrain_mesh(
        CHUNK_SIZE, CHUNK_SIZE, resolution, resolution, &height_fn,
    );
    let mesh_handle = ctx.assets.add_mesh(mesh);
    let node = ctx
        .scene
        .create_node(format!("chunk_{},{}_lod{}", coord.x, coord.z, lod_index));
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
    Ok(node)
}

fn lod_index_for_coord(manager: &ChunkManager, coord: ChunkCoord, camera_pos: Vec3) -> usize {
    let (center_x, center_z) = manager.chunk_center(coord);
    let distance = Vec3::new(center_x - camera_pos.x, 0.0, center_z - camera_pos.z).length();
    let resolution = select_lod(distance, &LOD_LEVELS);
    LOD_LEVELS
        .iter()
        .position(|level| level.resolution == resolution)
        .unwrap_or(LOD_LEVELS.len() - 1)
}

fn set_chunk_lod(
    ctx: &mut UpdateContext<'_>,
    chunk_nodes: &HashMap<ChunkCoord, [NodeId; 3]>,
    current_lod: &mut HashMap<ChunkCoord, usize>,
    coord: ChunkCoord,
    lod_index: usize,
) -> Result<()> {
    let Some(nodes) = chunk_nodes.get(&coord) else {
        return Ok(());
    };
    for (index, node) in nodes.iter().enumerate() {
        let visibility = if index == lod_index {
            VisibilityMode::Inherit
        } else {
            VisibilityMode::Hidden
        };
        ctx.scene.set_visibility(*node, visibility)?;
    }
    current_lod.insert(coord, lod_index);
    Ok(())
}

fn set_chunk_lod_startup(
    ctx: &mut StartupContext<'_>,
    chunk_nodes: &HashMap<ChunkCoord, [NodeId; 3]>,
    current_lod: &mut HashMap<ChunkCoord, usize>,
    coord: ChunkCoord,
    lod_index: usize,
) -> Result<()> {
    let Some(nodes) = chunk_nodes.get(&coord) else {
        return Ok(());
    };
    for (index, node) in nodes.iter().enumerate() {
        let visibility = if index == lod_index {
            VisibilityMode::Inherit
        } else {
            VisibilityMode::Hidden
        };
        ctx.scene.set_visibility(*node, visibility)?;
    }
    current_lod.insert(coord, lod_index);
    Ok(())
}

fn hide_chunk(
    ctx: &mut UpdateContext<'_>,
    chunk_nodes: &HashMap<ChunkCoord, [NodeId; 3]>,
    coord: ChunkCoord,
) -> Result<()> {
    if let Some(nodes) = chunk_nodes.get(&coord) {
        for node in nodes {
            ctx.scene.set_visibility(*node, VisibilityMode::Hidden)?;
        }
    }
    Ok(())
}

fn add_camera(ctx: &mut StartupContext<'_>) -> Result<NodeId> {
    let camera_node = ctx.scene.create_node("camera");
    ctx.scene.set_local_transform(
        camera_node,
        Transform {
            translation: Vec3::new(0.0, 50.0, 0.0),
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
                far: 800.0,
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
    rig_app::run::<TerrainLodApp>(rig_app::RunConfig {
        title: "Terrain (LOD Chunks)".into(),
        ..Default::default()
    })
}
