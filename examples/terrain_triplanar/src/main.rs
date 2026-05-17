//! Marching-cubes terrain demo with world-space triplanar texturing.
//!
//! Marching-cubes meshes do not have meaningful authored UVs: triangles are
//! created procedurally wherever the isosurface crosses a voxel grid. Triplanar
//! projection avoids UV seams by sampling a texture three times using
//! world-space coordinates (`YZ`, `XZ`, `XY`) and blending the results by the
//! surface normal's alignment to each axis. See `docs/MATERIAL.md` §6.5.
//! Debug builds use smaller extraction and texture workloads so `cargo run`
//! reaches the first frame quickly; release builds use the full demo settings.
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
        AddressMode, AlphaMode, DynamicMeshData, DynamicMeshId, FilterMode, MaterialAsset,
        MaterialParams, SamplerDescriptor, ShaderAsset, TextureAsset, TextureFormat,
        marching_cubes::{GridParams, extract},
    },
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::TRIPLANAR_PBR_SHADER,
    rig_scene::{CameraComponent, LightComponent, LightKind, MeshSource, NodeId, Renderable},
    winit::{
        event::WindowEvent,
        keyboard::{KeyCode, PhysicalKey},
    },
};

const RELEASE_GRID_XZ: u32 = 48;
const RELEASE_GRID_Y: u32 = 24;
const DEBUG_GRID_XZ: u32 = 36;
const DEBUG_GRID_Y: u32 = 18;
const GRID_XZ: u32 = if cfg!(debug_assertions) {
    DEBUG_GRID_XZ
} else {
    RELEASE_GRID_XZ
};
const GRID_Y: u32 = if cfg!(debug_assertions) {
    DEBUG_GRID_Y
} else {
    RELEASE_GRID_Y
};
const GRID_HALF_XZ: f32 = 20.0;
const GRID_HALF_Y: f32 = 10.0;
const RELEASE_ROCK_TEXTURE_SIZE: u32 = 256;
const DEBUG_ROCK_TEXTURE_SIZE: u32 = 128;
const ROCK_TEXTURE_SIZE: u32 = if cfg!(debug_assertions) {
    DEBUG_ROCK_TEXTURE_SIZE
} else {
    RELEASE_ROCK_TEXTURE_SIZE
};

struct TerrainTriplanarApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    trackball: TrackBall,
    dyn_id: DynamicMeshId,
    pending_mesh: Option<DynamicMeshData>,
    debug_hud: DebugHud,
}

impl Application for TerrainTriplanarApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let startup_timer = Instant::now();
        let field = make_field(42);
        let params = grid_params();
        let extract_timer = Instant::now();
        eprintln!("terrain_triplanar: extracting marching-cubes mesh {GRID_XZ}x{GRID_Y}x{GRID_XZ}");
        let mesh_data = extract(&field, &params, 0.0, None);
        eprintln!(
            "terrain_triplanar: extracted {} triangles in {:?}",
            mesh_data.index_count / 3,
            extract_timer.elapsed()
        );

        let dyn_id = DynamicMeshId::from_raw(0);
        let vb_size = (mesh_data.vertex_data.len() as u64)
            .next_power_of_two()
            .max(64);
        let ib_size = (mesh_data.index_data.len() as u64)
            .next_power_of_two()
            .max(64);
        ctx.renderer
            .register_dynamic_mesh(&ctx.gpu.device, dyn_id, vb_size, ib_size);

        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(TRIPLANAR_PBR_SHADER),
        });
        let texture_timer = Instant::now();
        eprintln!(
            "terrain_triplanar: generating {ROCK_TEXTURE_SIZE}x{ROCK_TEXTURE_SIZE} rock texture"
        );
        let rock_tex = ctx.assets.add_texture(TextureAsset {
            width: ROCK_TEXTURE_SIZE,
            height: ROCK_TEXTURE_SIZE,
            format: TextureFormat::Rgba8Unorm,
            data: Arc::from(generate_rock_texture().as_slice()),
        });
        eprintln!(
            "terrain_triplanar: rock texture generated in {:?}",
            texture_timer.elapsed()
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
                diffuse: [1.0, 1.0, 1.0, 1.0],
                metallic: 0.0,
                roughness: 0.85,
                custom_flags: 32,
                triplanar_scale: 4.0,
                ..Default::default()
            },
            textures: vec![Some((rock_tex, sampler)), None, None, None, None],
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
        });

        let terrain_node = ctx.scene.create_node("terrain_triplanar");
        ctx.scene.set_renderable(
            terrain_node,
            Renderable {
                mesh: MeshSource::Dynamic(dyn_id),
                material,
            },
        )?;
        ctx.scene.set_local_transform(
            terrain_node,
            Transform {
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene
            .set_dynamic_bounds(terrain_node, mesh_data.local_bounds)?;

        add_lights(ctx)?;

        let target_node = ctx.scene.create_node("terrain_focus");
        ctx.scene
            .set_local_transform(target_node, Transform::IDENTITY)?;

        let camera_node = ctx.scene.create_node("camera");
        let eye = Vec3::new(0.0, 12.0, 30.0);
        let pitch = -(eye.y / eye.z).atan();
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
                    far: 200.0,
                },
            },
        )?;

        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        debug_hud.add_element(ctx.overlay, Side::Left, "Terrain — Triplanar Texturing");
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            "Marching cubes + world-space UV projection",
        );
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            format!("Grid: {GRID_XZ}×{GRID_Y}×{GRID_XZ}, texture: {ROCK_TEXTURE_SIZE}²"),
        );
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            "WASD/arrows: fly  LMB: orbit  RMB: dolly",
        );

        Ok(Self {
            camera_node,
            camera_rig: CameraRig {
                translation_speed: 8.0,
                rotation_speed: 1.5,
            },
            trackball: TrackBall::new(target_node, eye.length()),
            dyn_id,
            pending_mesh: Some(mesh_data),
            debug_hud,
        })
        .inspect(|_| {
            eprintln!(
                "terrain_triplanar: startup completed in {:?}",
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
        if let Some(mesh_data) = self.pending_mesh.take() {
            ctx.renderer.update_dynamic_mesh(
                &ctx.gpu.device,
                &ctx.gpu.queue,
                self.dyn_id,
                &mesh_data,
            );
        }
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

fn grid_params() -> GridParams {
    GridParams {
        min: Vec3::new(-GRID_HALF_XZ, -GRID_HALF_Y, -GRID_HALF_XZ),
        max: Vec3::new(GRID_HALF_XZ, GRID_HALF_Y, GRID_HALF_XZ),
        resolution: [GRID_XZ, GRID_Y, GRID_XZ],
    }
}

fn make_field(seed: u32) -> impl Fn(Vec3) -> f32 {
    let fbm = Fbm::<Perlin>::new(seed)
        .set_octaves(6)
        .set_frequency(0.5)
        .set_persistence(0.5);

    move |p: Vec3| -> f32 {
        let n = fbm.get([p.x as f64 * 0.1, p.y as f64 * 0.1, p.z as f64 * 0.1]) as f32;
        -p.y + n * 4.0
    }
}

fn add_lights(ctx: &mut StartupContext<'_>) -> Result<()> {
    let key = ctx.scene.create_node("light_key");
    ctx.scene.set_local_transform(
        key,
        Transform {
            translation: Vec3::new(18.0, 22.0, 14.0),
            ..Default::default()
        },
    )?;
    ctx.scene.set_light(
        key,
        LightComponent {
            kind: LightKind::Point {
                color: Vec3::new(1.0, 0.95, 0.85),
                intensity: 1_200.0,
                range: 80.0,
            },
        },
    )?;

    let fill = ctx.scene.create_node("light_fill");
    ctx.scene.set_local_transform(
        fill,
        Transform {
            translation: Vec3::new(-20.0, 10.0, -10.0),
            ..Default::default()
        },
    )?;
    ctx.scene.set_light(
        fill,
        LightComponent {
            kind: LightKind::Point {
                color: Vec3::new(0.6, 0.7, 1.0),
                intensity: 300.0,
                range: 60.0,
            },
        },
    )?;

    let back = ctx.scene.create_node("light_back");
    ctx.scene.set_local_transform(
        back,
        Transform {
            translation: Vec3::new(0.0, 8.0, -22.0),
            ..Default::default()
        },
    )?;
    ctx.scene.set_light(
        back,
        LightComponent {
            kind: LightKind::Point {
                color: Vec3::new(1.0, 0.9, 0.7),
                intensity: 200.0,
                range: 60.0,
            },
        },
    )?;

    Ok(())
}

fn generate_rock_texture() -> Vec<u8> {
    let noise = Fbm::<Perlin>::new(123)
        .set_octaves(4)
        .set_frequency(4.0)
        .set_persistence(0.6);
    let mut pixels = vec![0_u8; (ROCK_TEXTURE_SIZE * ROCK_TEXTURE_SIZE * 4) as usize];
    let brown = [89_u8, 71, 51];
    let grey = [140_u8, 135, 128];

    for row in 0..ROCK_TEXTURE_SIZE {
        for col in 0..ROCK_TEXTURE_SIZE {
            let u = col as f64 / ROCK_TEXTURE_SIZE as f64;
            let v = row as f64 / ROCK_TEXTURE_SIZE as f64;
            let val = noise.get([u * 4.0, v * 4.0]) as f32;
            let t = (val * 0.5 + 0.5).clamp(0.0, 1.0);
            let idx = ((row * ROCK_TEXTURE_SIZE + col) * 4) as usize;
            for channel in 0..3 {
                let a = brown[channel] as f32;
                let b = grey[channel] as f32;
                pixels[idx + channel] = (a + (b - a) * t) as u8;
            }
            pixels[idx + 3] = 255;
        }
    }

    pixels
}

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<TerrainTriplanarApp>(rig_app::RunConfig {
        title: "Terrain (Triplanar Marching Cubes)".into(),
        ..Default::default()
    })
}
