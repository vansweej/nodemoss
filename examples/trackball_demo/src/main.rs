//! Arc-ball trackball camera demo.
//!
//! An icosahedron sits at the origin. Use the mouse to orbit and dolly the
//! camera around it.
//!
//! # Controls
//!
//! | Input          | Action                |
//! |----------------|-----------------------|
//! | LMB drag       | Orbit camera          |
//! | RMB drag       | Dolly (zoom)          |
//! | Escape         | Quit                  |
//! | F3             | Toggle overlay        |

use anyhow::Result;
use rig_app::{
    Application, DebugHud, OverlayUpdateContext, RenderContext, Side, StartupContext, TrackBall,
    UpdateContext,
    rig_assets::{MaterialAsset, ShaderAsset, mesh_factory},
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::NORMAL_COLOR_SHADER,
    rig_scene::{CameraComponent, NodeId, Renderable},
    winit::{event::WindowEvent, keyboard::KeyCode},
};

struct TrackballApp {
    camera_node: NodeId,
    #[allow(dead_code)]
    target_node: NodeId,
    trackball: TrackBall,
    debug_hud: DebugHud,
}

impl Application for TrackballApp {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        // Shared shader and material
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: NORMAL_COLOR_SHADER.into(),
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: Default::default(),
            textures: vec![],
        });

        // Icosahedron mesh at origin
        let mesh = ctx.assets.add_mesh(mesh_factory::create_icosahedron());
        let target_node = ctx.scene.create_node("icosahedron");
        ctx.scene
            .set_renderable(target_node, Renderable { mesh, material })?;
        ctx.scene.set_local_transform(
            target_node,
            Transform {
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;

        // Camera node — TrackBall will position it each frame
        let camera_node = ctx.scene.create_node("camera");
        ctx.scene.set_local_transform(
            camera_node,
            Transform {
                translation: Vec3::new(0.0, 0.0, 5.0),
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
                    far: 100.0,
                },
            },
        )?;

        // Overlay elements
        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            "LMB drag: orbit  RMB drag: dolly  Esc: quit",
        );

        log::info!("Trackball demo initialised. LMB=orbit RMB=dolly Esc=quit.");

        Ok(Self {
            camera_node,
            target_node,
            trackball: TrackBall::new(target_node, 5.0),
            debug_hud,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        *ctx.active_camera = Some(self.camera_node);
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
    rig_app::run::<TrackballApp>(rig_app::RunConfig {
        title: "Trackball Demo".into(),
        ..Default::default()
    })
}
