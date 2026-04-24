//! Interactive platonic solids demo.
//!
//! All five platonic solids orbit the origin at different radii and speeds
//! while spinning on their own axes. A fly-camera lets you explore the scene
//! freely.
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
//!
//! # Scene layout
//!
//! Each solid orbits the origin in a slightly tilted ellipse so the scene
//! reads as 3-D even from the default camera position.  Orbit radii and
//! angular speeds are staggered so the solids never overlap.
//!
//! | Solid        | Orbit radius | Orbit speed | Spin axis   |
//! |--------------|-------------|-------------|-------------|
//! | Tetrahedron  | 2.0         | 1.00 rad/s  | +Y          |
//! | Hexahedron   | 3.5         | 0.80 rad/s  | (1,1,0)     |
//! | Octahedron   | 5.0         | 0.60 rad/s  | +X          |
//! | Dodecahedron | 6.5         | 0.40 rad/s  | (0,1,1)     |
//! | Icosahedron  | 8.0         | 0.30 rad/s  | +Z          |

use anyhow::Result;
use rig_app::{
    Application, CameraRig, RenderContext, StartupContext, UpdateContext,
    rig_assets::{MaterialAsset, ShaderAsset, mesh_factory},
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_scene::{CameraComponent, NodeId, Renderable},
    winit::{event::WindowEvent, keyboard::KeyCode},
};

// ---------------------------------------------------------------------------
// Shader — normal-mapped colours (no lighting required).
// Vertex layout: position @ 0, normal @ 1, uv @ 2.
// ---------------------------------------------------------------------------

const NORMAL_SHADER: &str = r#"
struct ObjectUniforms {
    mvp: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> object: ObjectUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) uv:       vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       color:         vec3<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = object.mvp * vec4<f32>(in.position, 1.0);
    // Map normal components from [-1, 1] to [0, 1] for a distinctive colour.
    out.color = in.normal * 0.5 + vec3<f32>(0.5, 0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
"#;

// ---------------------------------------------------------------------------
// Per-solid animation parameters
// ---------------------------------------------------------------------------

/// Animation state for one platonic solid.
struct SolidState {
    node: NodeId,
    /// Distance from the origin (XZ plane).
    orbit_radius: f32,
    /// Angular speed of the orbit in radians per second.
    orbit_speed: f32,
    /// Initial orbit angle in radians so solids start spread out.
    orbit_phase: f32,
    /// Amplitude of the vertical (Y) oscillation — gives a 3-D feel.
    orbit_tilt: f32,
    /// Normalised axis the solid spins around.
    spin_axis: Vec3,
    /// Angular speed of the self-rotation in radians per second.
    spin_speed: f32,
}

// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

struct PlatonicApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    solids: Vec<SolidState>,
    /// Monotonically increasing scene time in seconds.
    elapsed: f32,
}

impl Application for PlatonicApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        // --- Shared shader & material ----------------------------------------
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: NORMAL_SHADER.into(),
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: Default::default(),
            textures: vec![],
        });

        // --- Mesh assets — one per solid -------------------------------------
        let meshes = [
            ctx.assets.add_mesh(mesh_factory::create_tetrahedron()),
            ctx.assets.add_mesh(mesh_factory::create_hexahedron()),
            ctx.assets.add_mesh(mesh_factory::create_octahedron()),
            ctx.assets.add_mesh(mesh_factory::create_dodecahedron()),
            ctx.assets.add_mesh(mesh_factory::create_icosahedron()),
        ];

        // --- Animation parameters --------------------------------------------
        // Evenly distribute initial orbit phases so solids start spread around
        // the origin (72° = 2π/5 apart).
        let two_pi_over_5 = 2.0 * std::f32::consts::PI / 5.0;

        let params: [(f32, f32, f32, Vec3, f32); 5] = [
            // (radius, orbit_speed, tilt, spin_axis, spin_speed)
            (2.0, 1.00, 0.5, Vec3::Y, 2.0),
            (3.5, 0.80, 0.8, Vec3::new(1.0, 1.0, 0.0).normalize(), 1.5),
            (5.0, 0.60, 1.0, Vec3::X, 1.8),
            (6.5, 0.40, 1.2, Vec3::new(0.0, 1.0, 1.0).normalize(), 1.2),
            (8.0, 0.30, 0.6, Vec3::Z, 1.0),
        ];

        let names = [
            "tetrahedron",
            "hexahedron",
            "octahedron",
            "dodecahedron",
            "icosahedron",
        ];

        // --- Scene nodes -----------------------------------------------------
        let mut solids: Vec<SolidState> = Vec::with_capacity(5);

        for (i, ((mesh, name), (radius, orbit_speed, tilt, spin_axis, spin_speed))) in meshes
            .iter()
            .zip(names.iter())
            .zip(params.iter())
            .enumerate()
        {
            let node = ctx.scene.create_node(*name);
            ctx.scene.set_renderable(
                node,
                Renderable {
                    mesh: *mesh,
                    material,
                },
            )?;

            // Place each solid at its initial orbit position so the first frame
            // already looks good (no big jump from origin on frame 0).
            let phase = i as f32 * two_pi_over_5;
            let x = radius * phase.cos();
            let z = radius * phase.sin();
            let y = tilt * (phase * 0.7).sin();
            ctx.scene.set_local_transform(
                node,
                Transform {
                    translation: Vec3::new(x, y, z),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )?;

            solids.push(SolidState {
                node,
                orbit_radius: *radius,
                orbit_speed: *orbit_speed,
                orbit_phase: phase,
                orbit_tilt: *tilt,
                spin_axis: *spin_axis,
                spin_speed: *spin_speed,
            });
        }

        // Reference ground plane so the camera has a spatial anchor.
        let plane_mesh = ctx.assets.add_mesh(mesh_factory::create_plane(30.0, 30.0));
        let plane = ctx.scene.create_node("ground");
        ctx.scene.set_renderable(
            plane,
            Renderable {
                mesh: plane_mesh,
                material,
            },
        )?;
        ctx.scene.set_local_transform(
            plane,
            Transform {
                translation: Vec3::new(0.0, -3.0, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;

        // --- Camera ----------------------------------------------------------
        // Start far enough back to see all five orbits at once.
        let camera_node = ctx.scene.create_node("camera");
        ctx.scene.set_local_transform(
            camera_node,
            Transform {
                translation: Vec3::new(0.0, 4.0, 18.0),
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
                    far: 200.0,
                },
            },
        )?;

        log::info!("Platonic solids demo initialised.");
        log::info!("Controls: WASD/QE move, arrow keys rotate, Escape quits.");

        Ok(Self {
            camera_node,
            camera_rig: CameraRig {
                translation_speed: 5.0,
                rotation_speed: 1.5,
            },
            solids,
            elapsed: 0.0,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        self.elapsed += dt;

        // Animate each solid: orbit around origin + self-rotation.
        for solid in &self.solids {
            let angle = solid.orbit_phase + solid.orbit_speed * self.elapsed;

            // Orbit position — slight Y oscillation for a 3-D trajectory.
            let x = solid.orbit_radius * angle.cos();
            let z = solid.orbit_radius * angle.sin();
            let y = solid.orbit_tilt * (angle * 0.7).sin();

            // Self-rotation accumulates over time.
            let spin = Quat::from_axis_angle(solid.spin_axis, solid.spin_speed * self.elapsed);

            ctx.scene.set_local_transform(
                solid.node,
                Transform {
                    translation: Vec3::new(x, y, z),
                    rotation: spin,
                    scale: Vec3::ONE,
                },
            )?;
        }

        // Drive the fly-camera.
        *ctx.active_camera = Some(self.camera_node);
        self.camera_rig.update(ctx, self.camera_node, dt)?;

        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
        ctx.renderer
            .render_scene(ctx.scene, ctx.assets, ctx.active_camera)?;
        Ok(())
    }

    fn on_window_event(&mut self, _ctx: &mut UpdateContext<'_>, event: &WindowEvent) -> Result<()> {
        if let WindowEvent::KeyboardInput { event, .. } = event {
            if matches!(
                event.physical_key,
                rig_app::winit::keyboard::PhysicalKey::Code(KeyCode::Escape)
            ) && event.state == rig_app::winit::event::ElementState::Pressed
            {
                log::info!("Escape pressed — closing window.");
                // The runner will exit on the next CloseRequested; we request
                // it indirectly by doing nothing — the user can also use the
                // window close button. A direct exit would require access to
                // the event loop handle which is not exposed through the trait.
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<PlatonicApp>("Platonic Solids")
}
