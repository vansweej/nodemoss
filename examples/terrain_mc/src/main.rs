//! Marching-cubes terrain demo.
//!
//! A 3-D noise field drives the Marching Cubes isosurface extractor to produce
//! a terrain with caves, overhangs, and floating islands — geometry that a
//! heightmap cannot represent.  The mesh is generated once at startup and
//! rendered with Cook-Torrance PBR shading.
//!
//! The scalar field is:
//!
//! ```text
//! f(p) = -p.y + fBm(p × 0.1) × 4.0
//! ```
//!
//! Points where `f > 0` are solid.  The `-p.y` term creates a ground plane at
//! Y = 0; the noise term carves caves below and raises hills above.
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

use std::sync::Arc;

use anyhow::Result;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, Side, StartupContext,
    TrackBall, UpdateContext,
    rig_assets::{
        AlphaMode, DynamicMeshData, DynamicMeshId, MaterialAsset, MaterialParams, ShaderAsset,
        marching_cubes::{GridParams, extract},
    },
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::PBR_SHADER,
    rig_scene::{CameraComponent, LightComponent, LightKind, MeshSource, NodeId, Renderable},
    winit::{
        event::WindowEvent,
        keyboard::{KeyCode, PhysicalKey},
    },
};

// ---------------------------------------------------------------------------
// Grid configuration
// ---------------------------------------------------------------------------

/// Number of cells along X and Z.
const GRID_XZ: u32 = 48;
/// Number of cells along Y (terrain is flatter than it is wide).
const GRID_Y: u32 = 24;
/// Half-extent of the grid in world units.
const GRID_HALF_XZ: f32 = 20.0;
const GRID_HALF_Y: f32 = 10.0;

fn grid_params() -> GridParams {
    GridParams {
        min: Vec3::new(-GRID_HALF_XZ, -GRID_HALF_Y, -GRID_HALF_XZ),
        max: Vec3::new(GRID_HALF_XZ, GRID_HALF_Y, GRID_HALF_XZ),
        resolution: [GRID_XZ, GRID_Y, GRID_XZ],
    }
}

// ---------------------------------------------------------------------------
// Scalar field
// ---------------------------------------------------------------------------

/// Build the terrain scalar field from a seeded fBm noise source.
///
/// Returns a closure suitable for `marching_cubes::extract`.
fn make_field(seed: u32) -> impl Fn(Vec3) -> f32 {
    let fbm = Fbm::<Perlin>::new(seed)
        .set_octaves(6)
        .set_frequency(0.5)
        .set_persistence(0.5);

    move |p: Vec3| -> f32 {
        let n = fbm.get([p.x as f64 * 0.1, p.y as f64 * 0.1, p.z as f64 * 0.1]) as f32;
        // Ground plane biased by 3-D noise — creates caves and overhangs.
        -p.y + n * 4.0
    }
}

// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

struct TerrainMcApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    trackball: TrackBall,
    dyn_id: DynamicMeshId,
    /// Mesh data waiting to be uploaded on the first render call.
    pending_mesh: Option<DynamicMeshData>,
    debug_hud: DebugHud,
}

impl Application for TerrainMcApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        // ── Terrain mesh ──────────────────────────────────────────────────────
        let field = make_field(42);
        let params = grid_params();
        let mesh_data = extract(&field, &params, 0.0, None);

        let triangle_count = mesh_data.index_count / 3;

        // Allocate GPU buffers — round up to next power of two for headroom.
        let dyn_id = DynamicMeshId::from_raw(0);
        let vb_size = (mesh_data.vertex_data.len() as u64)
            .next_power_of_two()
            .max(64);
        let ib_size = (mesh_data.index_data.len() as u64)
            .next_power_of_two()
            .max(64);
        ctx.renderer
            .register_dynamic_mesh(&ctx.gpu.device, dyn_id, vb_size, ib_size);

        // ── Material — earthy rock ────────────────────────────────────────────
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PBR_SHADER),
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams {
                diffuse: [0.45, 0.42, 0.38, 1.0],
                metallic: 0.0,
                roughness: 0.85,
                ..Default::default()
            },
            textures: vec![],
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
        });

        // ── Scene node ────────────────────────────────────────────────────────
        let terrain_node = ctx.scene.create_node("terrain_mc");
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

        // ── Lights ────────────────────────────────────────────────────────────
        // Key light — warm sun from upper-right.
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

        // Fill light — cool blue from the left.
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

        // Back light — subtle warm rim.
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

        // ── Camera ────────────────────────────────────────────────────────────
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

        // ── HUD ───────────────────────────────────────────────────────────────
        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        debug_hud.add_element(ctx.overlay, Side::Left, "Terrain — Marching Cubes");
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            format!("Triangles: {triangle_count}"),
        );
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            "Field: f(p) = -p.y + fBm(p×0.1) × 4",
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
        // Upload the mesh exactly once on the first render call.
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

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<TerrainMcApp>(rig_app::RunConfig {
        title: "Terrain (Marching Cubes)".into(),
        ..Default::default()
    })
}
