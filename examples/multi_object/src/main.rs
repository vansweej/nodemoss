//! Multi-object example: several MeshFactory meshes rendered in a single scene.
//!
//! Controls:
//!   W/S/A/D/Q/E       — move camera (forward/back/strafe/up-down)
//!   Arrow keys         — rotate camera
//!   V                  — toggle visibility of the sphere
//!
//! What this demonstrates:
//!   - Procedural geometry via `MeshFactory` (box × 4, sphere × 1, plane × 1)
//!   - Shared mesh assets: all four boxes reference the same `MeshHandle`
//!   - Correct depth ordering via the depth buffer
//!   - Frustum culling: a box placed far behind the camera is never drawn
//!   - Runtime visibility toggling via `SceneGraph::set_visibility`

use anyhow::Result;
use rig_app::{
    Application, CameraRig, RenderContext, StartupContext, UpdateContext,
    rig_assets::{MaterialAsset, ShaderAsset, mesh_factory},
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_scene::{CameraComponent, NodeId, Renderable, VisibilityMode},
    winit::{event::WindowEvent, keyboard::KeyCode},
};

// ---------------------------------------------------------------------------
// Normal-shaded WGSL shader (no lighting — colours derived from normals).
// Layout: position @ 0, normal @ 1, uv @ 2.
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
    // Map normal from [-1,1] to [0,1] for a simple colour.
    out.color = in.normal * 0.5 + vec3<f32>(0.5, 0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
"#;

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct MultiObjectApp {
    camera: NodeId,
    camera_rig: CameraRig,
    sphere: NodeId,
    /// Whether the sphere is currently visible.
    sphere_visible: bool,
    /// Debounce: was V held last frame?
    v_was_pressed: bool,
}

impl Application for MultiObjectApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        // --- Shader & material -----------------------------------------------
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: NORMAL_SHADER.into(),
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: Default::default(),
            textures: vec![],
        });

        // --- Meshes ----------------------------------------------------------
        // Shared box mesh — all four box nodes reference the same handle.
        let box_mesh = ctx
            .assets
            .add_mesh(mesh_factory::create_box(1.0, 1.0, 1.0));
        let sphere_mesh = ctx
            .assets
            .add_mesh(mesh_factory::create_sphere(0.6, 16, 12));
        let plane_mesh = ctx
            .assets
            .add_mesh(mesh_factory::create_plane(10.0, 10.0));

        // --- Scene nodes -----------------------------------------------------

        // Ground plane at y = -1
        let plane = ctx.scene.create_node("plane");
        ctx.scene.set_renderable(plane, Renderable { mesh: plane_mesh, material })?;
        ctx.scene.set_local_transform(plane, Transform {
            translation: Vec3::new(0.0, -1.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        })?;

        // Four boxes at different positions (all share one MeshAsset)
        let box_positions = [
            Vec3::new(-2.0,  0.0,  0.0),
            Vec3::new( 2.0,  0.0,  0.0),
            Vec3::new( 0.0,  0.0, -3.0),
            Vec3::new( 0.0,  1.5,  0.0),
        ];
        for (i, &pos) in box_positions.iter().enumerate() {
            let node = ctx.scene.create_node(format!("box_{i}"));
            ctx.scene.set_renderable(node, Renderable { mesh: box_mesh, material })?;
            ctx.scene.set_local_transform(node, Transform {
                translation: pos,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            })?;
        }

        // One box placed far behind the camera — should be frustum-culled.
        let behind = ctx.scene.create_node("box_behind_camera");
        ctx.scene.set_renderable(behind, Renderable { mesh: box_mesh, material })?;
        ctx.scene.set_local_transform(behind, Transform {
            translation: Vec3::new(0.0, 0.0, 500.0), // well behind near camera
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        })?;

        // Sphere (toggled with V)
        let sphere = ctx.scene.create_node("sphere");
        ctx.scene.set_renderable(sphere, Renderable { mesh: sphere_mesh, material })?;
        ctx.scene.set_local_transform(sphere, Transform {
            translation: Vec3::new(0.0, 0.5, -1.5),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        })?;

        // --- Camera ----------------------------------------------------------
        let camera = ctx.scene.create_node("camera");
        ctx.scene.set_local_transform(camera, Transform {
            translation: Vec3::new(0.0, 1.0, 6.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        })?;
        ctx.scene.set_camera(camera, CameraComponent {
            projection: Projection::Perspective {
                fov_y_radians: 60.0_f32.to_radians(),
                near: 0.1,
                far: 100.0,
            },
        })?;

        Ok(Self {
            camera,
            camera_rig: CameraRig::default(),
            sphere,
            sphere_visible: true,
            v_was_pressed: false,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        *ctx.active_camera = Some(self.camera);
        self.camera_rig.update(ctx, self.camera, dt)?;
        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
        ctx.renderer
            .render_scene(ctx.scene, ctx.assets, ctx.active_camera)?;
        Ok(())
    }

    fn on_window_event(
        &mut self,
        ctx: &mut UpdateContext<'_>,
        event: &WindowEvent,
    ) -> Result<()> {
        // Toggle sphere visibility when V is pressed (on key-down, not held).
        if let WindowEvent::KeyboardInput { event, .. } = event {
            let is_v = matches!(
                event.physical_key,
                rig_app::winit::keyboard::PhysicalKey::Code(KeyCode::KeyV)
            );
            let just_pressed =
                is_v && event.state == rig_app::winit::event::ElementState::Pressed
                    && !self.v_was_pressed;
            if just_pressed {
                self.sphere_visible = !self.sphere_visible;
                let mode = if self.sphere_visible {
                    VisibilityMode::Inherit
                } else {
                    VisibilityMode::Hidden
                };
                ctx.scene.set_visibility(self.sphere, mode)?;
                log::info!(
                    "sphere visibility: {}",
                    if self.sphere_visible { "visible" } else { "hidden" }
                );
            }
            self.v_was_pressed =
                is_v && event.state == rig_app::winit::event::ElementState::Pressed;
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<MultiObjectApp>("Multi-Object Scene")
}
