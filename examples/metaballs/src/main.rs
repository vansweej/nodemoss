//! Metaballs demo — CPU Marching Cubes isosurface extraction.
//!
//! Four bouncing metaballs are animated in real time.  The scalar field is
//! evaluated on a 48³ grid every frame; the Marching Cubes algorithm extracts
//! a triangle mesh that is uploaded to the GPU as a `DynamicMesh`.
//!
//! The surface is rendered with Cook-Torrance PBR shading (metallic chrome
//! material, four point lights) for a liquid-metal look.
//!
//! # Controls
//!
//! | Key(s)      | Action                        |
//! |-------------|-------------------------------|
//! | W / S       | Move forward / backward       |
//! | A / D       | Strafe left / right           |
//! | Q / E       | Move up / down                |
//! | Arrow keys  | Rotate camera (yaw / pitch)   |
//! | F3          | Toggle overlay                |
//! | F4          | Toggle wireframe              |
//! | Escape      | Close window                  |

use std::sync::Arc;

use anyhow::Result;
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, StartupContext,
    UpdateContext,
    rig_assets::{
        DynamicMeshData, DynamicMeshId, MaterialAsset, MaterialParams, MeshSource, ShaderAsset,
        marching_cubes::{GridParams, extract},
    },
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::PBR_SHADER,
    rig_scene::{CameraComponent, LightComponent, LightKind, NodeId, Renderable},
    winit::{event::WindowEvent, keyboard::KeyCode},
};

// ---------------------------------------------------------------------------
// Grid configuration
// ---------------------------------------------------------------------------

const GRID_RES: u32 = 32;
const ISO_VALUE: f32 = 1.0;
const GRID_HALF: f32 = 6.0;

fn grid_params() -> GridParams {
    GridParams {
        min: Vec3::splat(-GRID_HALF),
        max: Vec3::splat(GRID_HALF),
        resolution: [GRID_RES, GRID_RES, GRID_RES],
    }
}

// ---------------------------------------------------------------------------
// Metaball field
// ---------------------------------------------------------------------------

/// A single metaball: position + radius.
struct Ball {
    pos: Vec3,
    radius: f32,
}

/// Evaluate the combined metaball scalar field at `p`.
/// Each ball contributes `r² / |p - center|²`.
fn metaball_field(balls: &[Ball], p: Vec3) -> f32 {
    balls
        .iter()
        .map(|b| {
            let d2 = (p - b.pos).length_squared().max(1e-6);
            b.radius * b.radius / d2
        })
        .sum()
}

/// Analytical gradient normal for the combined metaball field.
///
/// ∇(Σ rᵢ²/|p−cᵢ|²) = Σ −2rᵢ²(p−cᵢ)/|p−cᵢ|⁴
///
/// The gradient points inward (toward higher field values), so we negate it
/// to obtain an outward surface normal.
fn metaball_normal(balls: &[Ball], p: Vec3) -> [f32; 3] {
    let mut grad = Vec3::ZERO;
    for b in balls {
        let d = p - b.pos;
        let d2 = d.length_squared().max(1e-6);
        // Contribution: -2r²·d / d⁴
        grad += -2.0 * b.radius * b.radius * d / (d2 * d2);
    }
    // Negate gradient (inward → outward)
    let g = -grad;
    let len = g.length();
    if len > 1e-10 {
        let n = g / len;
        [n.x, n.y, n.z]
    } else {
        [0.0, 1.0, 0.0]
    }
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct MetaballsApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    metaball_node: NodeId,
    dyn_id: DynamicMeshId,
    /// Latest MC output — computed in `update()`, uploaded in `render()`.
    pending_mesh: Option<DynamicMeshData>,
    elapsed: f64,
    triangle_count: u32,
    debug_hud: DebugHud,
}

impl Application for MetaballsApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        // --- PBR shader — chrome/liquid-metal material -----------------------
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PBR_SHADER),
        });
        // Silver-chrome: near-white albedo, fully metallic, very smooth.
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: MaterialParams {
                // Blue-grey silver: darker and more chromatic than pure Ag white.
                // The blue shift and reduced brightness give a steely silver
                // rather than a near-white chrome finish.
                diffuse: [0.72, 0.76, 0.86, 1.0],
                metallic: 1.0,
                roughness: 0.10,
                ..Default::default()
            },
            textures: vec![],
        });

        // --- Dynamic mesh slot -----------------------------------------------
        let dyn_id = DynamicMeshId::from_raw(0);

        // Register the GPU buffers (grow-on-demand; initial size is a guess).
        // Vertex stride = 32 bytes; 48³ cells can produce at most ~5 * 48³ vertices.
        let initial_vertex_bytes = (32 * 5 * (GRID_RES as u64).pow(3)).next_power_of_two();
        let initial_index_bytes = (4 * 15 * (GRID_RES as u64).pow(3)).next_power_of_two();
        ctx.renderer.register_dynamic_mesh(
            &ctx.gpu.device,
            dyn_id,
            initial_vertex_bytes,
            initial_index_bytes,
        );

        // --- Scene node for the metaball surface -----------------------------
        let metaball_node = ctx.scene.create_node("metaballs");
        ctx.scene.set_renderable(
            metaball_node,
            Renderable {
                mesh: MeshSource::Dynamic(dyn_id),
                material,
            },
        )?;
        // Identity transform — the field is already in world space.
        ctx.scene.set_local_transform(
            metaball_node,
            Transform {
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;

        // --- Lights: four point lights surrounding the scene -----------------
        // With UE4 inverse-square attenuation and ACES tone mapping the shader
        // works in HDR internally, so light intensities can be set much higher
        // than 1.0 — highlights will roll off naturally instead of clipping.
        let light_setup: &[(&str, Vec3, Vec3, f32, f32)] = &[
            // (name,          position,                   colour (linear),          intensity, range)
            (
                "light_key",
                Vec3::new(8.0, 8.0, 8.0),
                Vec3::new(1.00, 0.97, 0.90),
                18.0,
                32.0,
            ),
            (
                "light_fill",
                Vec3::new(-9.0, 5.0, 6.0),
                Vec3::new(0.55, 0.65, 1.00),
                8.0,
                32.0,
            ),
            (
                "light_rim",
                Vec3::new(1.0, 7.0, -10.0),
                Vec3::new(0.85, 0.90, 1.00),
                12.0,
                32.0,
            ),
            (
                "light_low",
                Vec3::new(-4.0, -8.0, 3.0),
                Vec3::new(1.00, 0.75, 0.55),
                6.0,
                28.0,
            ),
        ];
        for (name, pos, color, intensity, range) in light_setup {
            let node = ctx.scene.create_node(*name);
            ctx.scene.set_local_transform(
                node,
                Transform {
                    translation: *pos,
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )?;
            ctx.scene.set_light(
                node,
                LightComponent {
                    kind: LightKind::Point {
                        color: *color,
                        intensity: *intensity,
                        range: *range,
                    },
                },
            )?;
        }

        // --- Camera ----------------------------------------------------------
        let camera_node = ctx.scene.create_node("camera");
        // Pitch down to look from (0, 4, 12) toward the origin.
        let pitch = -(4.0_f32).atan2(12.0);
        ctx.scene.set_local_transform(
            camera_node,
            Transform {
                translation: Vec3::new(0.0, 4.0, 12.0),
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

        let debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);

        log::info!(
            "Metaballs demo initialised (PBR chrome). F4 = wireframe, F3 = overlay, Escape = quit."
        );

        Ok(Self {
            camera_node,
            camera_rig: CameraRig {
                translation_speed: 5.0,
                rotation_speed: 1.5,
            },
            metaball_node,
            dyn_id,
            pending_mesh: None,
            elapsed: 0.0,
            triangle_count: 0,
            debug_hud,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        self.elapsed += dt as f64;
        let t = self.elapsed as f32;

        // Animate 4 balls along Lissajous-like paths inside the grid.
        let balls = [
            Ball {
                pos: Vec3::new(
                    3.2 * (t * 0.7).sin(),
                    2.5 * (t * 0.5).cos(),
                    3.0 * (t * 0.9).sin(),
                ),
                radius: 1.4,
            },
            Ball {
                pos: Vec3::new(
                    -3.0 * (t * 0.6).cos(),
                    2.8 * (t * 0.8).sin(),
                    -2.5 * (t * 0.4).cos(),
                ),
                radius: 1.3,
            },
            Ball {
                pos: Vec3::new(
                    2.8 * (t * 1.1).sin(),
                    -2.5 * (t * 0.7).cos(),
                    3.0 * (t * 0.6).cos(),
                ),
                radius: 1.2,
            },
            Ball {
                pos: Vec3::new(
                    -2.5 * (t * 0.9).cos(),
                    -3.0 * (t * 0.5).sin(),
                    -3.2 * (t * 1.0).sin(),
                ),
                radius: 1.1,
            },
        ];

        // Run Marching Cubes on the CPU with analytical gradient normals.
        let params = grid_params();
        let field = |p: Vec3| metaball_field(&balls, p);
        let normal = |p: Vec3| metaball_normal(&balls, p);
        let mesh_data = extract(&field, &params, ISO_VALUE, Some(&normal));

        // Update dynamic bounds for frustum culling.
        ctx.scene
            .set_dynamic_bounds(self.metaball_node, mesh_data.local_bounds)?;

        self.triangle_count = mesh_data.index_count / 3;
        self.pending_mesh = Some(mesh_data);

        *ctx.active_camera = Some(self.camera_node);
        self.camera_rig.update(ctx, self.camera_node, dt)?;

        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
        // Upload the latest MC output to the GPU before rendering.
        if let Some(data) = self.pending_mesh.take() {
            ctx.renderer
                .update_dynamic_mesh(&ctx.gpu.device, &ctx.gpu.queue, self.dyn_id, &data);
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
            && matches!(
                event.physical_key,
                rig_app::winit::keyboard::PhysicalKey::Code(KeyCode::Escape)
            )
            && event.state == rig_app::winit::event::ElementState::Pressed
        {
            ctx.request_exit();
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<MetaballsApp>(rig_app::RunConfig {
        title: "Metaballs".into(),
        ..Default::default()
    })
}
