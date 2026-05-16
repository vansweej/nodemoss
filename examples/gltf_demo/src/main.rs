//! glTF loading demo.
//!
//! Loads a glTF/GLB asset through `rig-gltf` and renders it with the standard
//! PBR material stack.
//!
//! # Controls
//!
//! | Input          | Action                |
//! |----------------|-----------------------|
//! | WASD / arrows  | Fly camera            |
//! | LMB drag       | Orbit model           |
//! | RMB drag       | Dolly (zoom)          |
//! | Escape         | Quit                  |
//! | F3             | Toggle overlay        |

use std::sync::Arc;

use anyhow::Result;
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, Side, StartupContext,
    TrackBall, UpdateContext,
    rig_anim::AnimationPlayer,
    rig_assets::ShaderAsset,
    rig_gltf::{LoadedGltf, load_gltf},
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::PBR_SHADER,
    rig_scene::{CameraComponent, LightComponent, LightKind, NodeId},
    winit::{
        event::WindowEvent,
        keyboard::{KeyCode, PhysicalKey},
    },
};

const DEFAULT_MODEL_PATH: &str = "assets/models/gltf/DamagedHelmet.glb";

struct GltfDemo {
    camera_node: NodeId,
    camera_rig: CameraRig,
    trackball: TrackBall,
    debug_hud: DebugHud,
    animation_player: Option<AnimationPlayer>,
    _loaded: LoadedGltf,
}

impl Application for GltfDemo {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| DEFAULT_MODEL_PATH.to_string());
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PBR_SHADER),
        });
        let loaded = load_gltf(&path, shader, ctx.scene, ctx.assets)?;

        let target_node = ctx.scene.create_node("gltf_focus");
        ctx.scene
            .set_local_transform(target_node, Transform::IDENTITY)?;

        if ctx.scene.light_nodes().is_empty() {
            let sun = ctx.scene.create_node("sun_directional_light");
            ctx.scene.set_local_transform(
                sun,
                Transform {
                    translation: Vec3::ZERO,
                    rotation: Quat::from_rotation_x(-0.65) * Quat::from_rotation_y(-0.45),
                    scale: Vec3::ONE,
                },
            )?;
            ctx.scene.set_light(
                sun,
                LightComponent {
                    kind: LightKind::Directional {
                        color: Vec3::new(1.0, 0.95, 0.88),
                        intensity: 3.0,
                    },
                },
            )?;
        }

        let camera_node = ctx.scene.create_node("camera");
        let eye = Vec3::new(0.0, 1.2, 4.0);
        let pitch = -eye.y.atan2(eye.z);
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
                    fov_y_radians: 55.0_f32.to_radians(),
                    near: 0.1,
                    far: 500.0,
                },
            },
        )?;

        let mut animation_player = loaded
            .animations
            .first()
            .map(|&clip| AnimationPlayer::new(clip));
        if let Some(player) = &mut animation_player {
            player.bind(ctx.assets, ctx.scene)?;
        }

        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        debug_hud.add_element(ctx.overlay, Side::Left, "glTF Demo");
        debug_hud.add_element(ctx.overlay, Side::Left, format!("Model: {path}"));
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            "WASD/arrows: fly  LMB: orbit  RMB: dolly",
        );

        Ok(Self {
            camera_node,
            camera_rig: CameraRig {
                translation_speed: 3.0,
                rotation_speed: 1.4,
            },
            trackball: TrackBall::new(target_node, eye.length()),
            debug_hud,
            animation_player,
            _loaded: loaded,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        *ctx.active_camera = Some(self.camera_node);
        self.camera_rig.update(ctx, self.camera_node, dt)?;
        self.trackball.sync_to_camera(ctx.scene, self.camera_node)?;
        self.trackball
            .update(ctx.input, ctx.scene, self.camera_node, dt)?;
        if let Some(player) = &mut self.animation_player {
            player.advance(dt);
            player.evaluate(ctx.assets, ctx.scene)?;
        }
        ctx.scene.update_all_world_transforms()?;
        ctx.scene.update_all_world_bounds(ctx.assets)?;
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
        if let WindowEvent::KeyboardInput { event, .. } = event {
            if matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
                && event.state == rig_app::winit::event::ElementState::Pressed
            {
                ctx.request_exit();
            }
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<GltfDemo>(rig_app::RunConfig {
        title: "glTF Demo".into(),
        ..Default::default()
    })
}
