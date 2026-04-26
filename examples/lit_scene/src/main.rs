//! Lit scene demo — demonstrates Blinn-Phong shading with multiple lights.
//!
//! Five platonic solids are arranged in a circle, lit by one directional light
//! and two orbiting point lights (red and blue).
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

use std::sync::Arc;

use anyhow::Result;
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, StartupContext,
    UpdateContext,
    rig_assets::{MaterialAsset, ShaderAsset, mesh_factory},
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::PHONG_SHADER,
    rig_scene::{CameraComponent, LightComponent, LightKind, NodeId, Renderable},
    winit::{event::WindowEvent, keyboard::KeyCode},
};

// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

struct LitSceneApp {
    camera_node: NodeId,
    camera_rig: CameraRig,
    solid_nodes: Vec<NodeId>,
    point_light_nodes: [NodeId; 2],
    elapsed: f64,
    debug_hud: DebugHud,
    camera_pos: Vec3,
}

impl Application for LitSceneApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        // --- Phong shader & material -----------------------------------------
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PHONG_SHADER),
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: Default::default(),
            textures: vec![],
        });

        // --- Five solids arranged in a circle --------------------------------
        let meshes = [
            ctx.assets.add_mesh(mesh_factory::create_tetrahedron()),
            ctx.assets.add_mesh(mesh_factory::create_hexahedron()),
            ctx.assets.add_mesh(mesh_factory::create_octahedron()),
            ctx.assets.add_mesh(mesh_factory::create_dodecahedron()),
            ctx.assets.add_mesh(mesh_factory::create_icosahedron()),
        ];
        let names = [
            "tetrahedron",
            "hexahedron",
            "octahedron",
            "dodecahedron",
            "icosahedron",
        ];
        let radius = 4.0_f32;
        let two_pi_over_5 = 2.0 * std::f32::consts::PI / 5.0;

        let mut solid_nodes = Vec::with_capacity(5);
        for (i, (mesh, name)) in meshes.iter().zip(names.iter()).enumerate() {
            let node = ctx.scene.create_node(*name);
            ctx.scene.set_renderable(
                node,
                Renderable {
                    mesh: *mesh,
                    material,
                },
            )?;
            let angle = i as f32 * two_pi_over_5;
            ctx.scene.set_local_transform(
                node,
                Transform {
                    translation: Vec3::new(radius * angle.cos(), 0.0, radius * angle.sin()),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )?;
            solid_nodes.push(node);
        }

        // --- Directional light (white, pointing down-forward) ----------------
        let dir_light_node = ctx.scene.create_node("dir_light");
        ctx.scene.set_local_transform(
            dir_light_node,
            Transform {
                translation: Vec3::ZERO,
                // Rotate to point in the -Y direction with a slight forward tilt.
                rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_4),
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_light(
            dir_light_node,
            LightComponent {
                kind: LightKind::Directional {
                    color: Vec3::new(1.0, 1.0, 1.0),
                    intensity: 1.0,
                },
            },
        )?;

        // --- Two orbiting point lights (red and blue) ------------------------
        let point_light_node_red = ctx.scene.create_node("point_light_red");
        ctx.scene.set_local_transform(
            point_light_node_red,
            Transform {
                translation: Vec3::new(5.0, 2.0, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_light(
            point_light_node_red,
            LightComponent {
                kind: LightKind::Point {
                    color: Vec3::new(1.0, 0.2, 0.2),
                    intensity: 3.0,
                    range: 12.0,
                },
            },
        )?;

        let point_light_node_blue = ctx.scene.create_node("point_light_blue");
        ctx.scene.set_local_transform(
            point_light_node_blue,
            Transform {
                translation: Vec3::new(-5.0, 2.0, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_light(
            point_light_node_blue,
            LightComponent {
                kind: LightKind::Point {
                    color: Vec3::new(0.2, 0.4, 1.0),
                    intensity: 3.0,
                    range: 12.0,
                },
            },
        )?;

        // --- Camera ----------------------------------------------------------
        let camera_node = ctx.scene.create_node("camera");
        ctx.scene.set_local_transform(
            camera_node,
            Transform {
                translation: Vec3::new(0.0, 5.0, 14.0),
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

        let debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);

        log::info!(
            "Lit scene demo initialised. Controls: WASD/QE move, arrows rotate, Escape quits."
        );

        Ok(Self {
            camera_node,
            camera_rig: CameraRig {
                translation_speed: 5.0,
                rotation_speed: 1.5,
            },
            solid_nodes,
            point_light_nodes: [point_light_node_red, point_light_node_blue],
            elapsed: 0.0,
            debug_hud,
            camera_pos: Vec3::new(0.0, 5.0, 14.0),
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        self.elapsed += dt as f64;
        let t = self.elapsed as f32;

        // Spin solids slowly
        for (i, &node) in self.solid_nodes.iter().enumerate() {
            let angle = i as f32 * 2.0 * std::f32::consts::PI / 5.0;
            let spin = Quat::from_rotation_y(t * 0.5 + angle);
            let x = 4.0 * angle.cos();
            let z = 4.0 * angle.sin();
            ctx.scene.set_local_transform(
                node,
                Transform {
                    translation: Vec3::new(x, 0.0, z),
                    rotation: spin,
                    scale: Vec3::ONE,
                },
            )?;
        }

        // Orbit point lights around the scene
        let orbit_speed = 0.8_f32;
        let orbit_r = 6.0_f32;
        for (i, &light_node) in self.point_light_nodes.iter().enumerate() {
            let phase = i as f32 * std::f32::consts::PI;
            let lx = orbit_r * (t * orbit_speed + phase).cos();
            let lz = orbit_r * (t * orbit_speed + phase).sin();
            ctx.scene.set_local_transform(
                light_node,
                Transform {
                    translation: Vec3::new(lx, 2.0, lz),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            )?;
        }

        *ctx.active_camera = Some(self.camera_node);
        self.camera_rig.update(ctx, self.camera_node, dt)?;

        self.camera_pos = ctx
            .scene
            .local_transform(self.camera_node)
            .map(|t| t.translation)
            .unwrap_or(Vec3::ZERO);

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
    rig_app::run::<LitSceneApp>("Lit Scene")
}
