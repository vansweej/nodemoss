//! Mesh showcase: box, sphere, and plane rendered with depth testing.
//!
//! Demonstrates procedural mesh generation via `MeshFactory` and correct
//! depth ordering. Three objects are placed at different positions and
//! depths; the box and sphere spin so depth ordering is visually testable.
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
//! | Object | Position         | Notes                          |
//! |--------|-----------------|--------------------------------|
//! | Box    | (-2.5, 0.5, 0)  | Spins on Y axis                |
//! | Sphere | ( 2.5, 0.75, 0) | Spins on Y axis                |
//! | Plane  | (  0,  0,   0)  | Ground quad, static            |

use anyhow::Result;
use rig_app::{
    Application, CameraRig, RenderContext, StartupContext, UpdateContext,
    rig_assets::{MaterialAsset, ShaderAsset, mesh_factory},
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::NORMAL_COLOR_SHADER,
    rig_scene::{CameraComponent, NodeId, Renderable},
    winit::{event::WindowEvent, keyboard::KeyCode, keyboard::PhysicalKey},
};

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct MeshShowcaseApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    box_node: NodeId,
    sphere_node: NodeId,
    /// Monotonically increasing scene time in seconds.
    elapsed: f32,
}

impl Application for MeshShowcaseApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        // --- Shared shader & material ----------------------------------------
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: NORMAL_COLOR_SHADER.into(),
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: Default::default(),
            textures: vec![],
        });

        // --- Mesh assets -----------------------------------------------------
        let box_mesh = ctx.assets.add_mesh(mesh_factory::create_box(1.0, 1.0, 1.0));
        let sphere_mesh = ctx
            .assets
            .add_mesh(mesh_factory::create_sphere(0.75, 32, 16));
        let plane_mesh = ctx.assets.add_mesh(mesh_factory::create_plane(6.0, 6.0));

        // --- Scene nodes -----------------------------------------------------

        // Box — left of centre, elevated half its height above the ground.
        let box_node = ctx.scene.create_node("box");
        ctx.scene.set_local_transform(
            box_node,
            Transform {
                translation: Vec3::new(-2.5, 0.5, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_renderable(
            box_node,
            Renderable {
                mesh: box_mesh,
                material,
            },
        )?;

        // Sphere — right of centre, elevated by its radius.
        let sphere_node = ctx.scene.create_node("sphere");
        ctx.scene.set_local_transform(
            sphere_node,
            Transform {
                translation: Vec3::new(2.5, 0.75, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_renderable(
            sphere_node,
            Renderable {
                mesh: sphere_mesh,
                material,
            },
        )?;

        // Plane — ground quad, centred at origin.
        let plane_node = ctx.scene.create_node("plane");
        ctx.scene.set_local_transform(
            plane_node,
            Transform {
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_renderable(
            plane_node,
            Renderable {
                mesh: plane_mesh,
                material,
            },
        )?;

        // --- Camera ----------------------------------------------------------
        let camera_node = ctx.scene.create_node("camera");
        ctx.scene.set_local_transform(
            camera_node,
            Transform {
                translation: Vec3::new(0.0, 3.0, 8.0),
                rotation: Quat::from_rotation_x(-0.35),
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_camera(
            camera_node,
            CameraComponent {
                projection: Projection::Perspective {
                    fov_y_radians: std::f32::consts::FRAC_PI_4,
                    near: 0.1,
                    far: 200.0,
                },
            },
        )?;

        Ok(Self {
            camera_node,
            camera_rig: CameraRig::default(),
            box_node,
            sphere_node,
            elapsed: 0.0,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        self.elapsed += dt;

        // Spin the box and sphere on the Y axis.
        let box_angle = self.elapsed * 0.8;
        ctx.scene.set_local_transform(
            self.box_node,
            Transform {
                translation: Vec3::new(-2.5, 0.5, 0.0),
                rotation: Quat::from_rotation_y(box_angle),
                scale: Vec3::ONE,
            },
        )?;

        let sphere_angle = self.elapsed * 0.5;
        ctx.scene.set_local_transform(
            self.sphere_node,
            Transform {
                translation: Vec3::new(2.5, 0.75, 0.0),
                rotation: Quat::from_rotation_y(sphere_angle),
                scale: Vec3::ONE,
            },
        )?;

        // Camera controls via CameraRig.
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
            if event.physical_key == PhysicalKey::Code(KeyCode::Escape) {
                std::process::exit(0);
            }
        }
        Ok(())
    }
}

fn main() {
    env_logger::init();
    rig_app::run::<MeshShowcaseApp>("Mesh Showcase").expect("failed to run mesh showcase");
}
