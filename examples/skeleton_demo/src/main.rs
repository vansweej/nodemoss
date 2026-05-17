//! Rigid skeleton animation demo.
//!
//! Builds a small articulated robot arm from `MeshFactory` boxes and drives the
//! joint hierarchy with a hand-authored [`AnimationClip`]. Bones are ordinary
//! scene graph nodes; `AnimationPlayer` samples rotation keyframes into local
//! node transforms and the existing scene traversal propagates world transforms.
//!
//! # Controls
//!
//! | Key(s)      | Action                        |
//! |-------------|-------------------------------|
//! | W / S       | Move camera forward / backward|
//! | A / D       | Strafe camera left / right    |
//! | Q / E       | Move camera down / up         |
//! | Arrow keys  | Rotate camera (yaw / pitch)   |
//! | Space       | Pause / resume animation      |
//! | + / -       | Speed up / slow down          |
//! | Escape      | Close window                  |
//! | F3          | Toggle overlay                |

use anyhow::Result;
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, Side, StartupContext,
    UpdateContext,
    rig_anim::AnimationPlayer,
    rig_assets::{
        AlphaMode, AnimationChannel, AnimationClip, ChannelProperty, KeyframeSampler,
        KeyframeValues, MaterialAsset, ShaderAsset, mesh_factory,
    },
    rig_math::{Interpolation, Projection, Quat, Transform, Vec3},
    rig_overlay::ElementId,
    rig_render::NORMAL_COLOR_SHADER,
    rig_scene::{CameraComponent, LightComponent, LightKind, MeshSource, NodeId, Renderable},
    winit::{
        event::WindowEvent,
        keyboard::{KeyCode, PhysicalKey},
    },
};

struct SkeletonDemo {
    camera_node: NodeId,
    camera_rig: CameraRig,
    player: AnimationPlayer,
    debug_hud: DebugHud,
    time_label: ElementId,
    speed_label: ElementId,
    state_label: ElementId,
    /// Debounce flag — true while Space is held.
    space_held: bool,
}

impl Application for SkeletonDemo {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: NORMAL_COLOR_SHADER.into(),
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: Default::default(),
            textures: vec![],
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
        });

        let base = add_arm_segment(ctx, "base", None, Vec3::ZERO, (0.7, 0.25, 0.7), material)?;
        let shoulder = add_arm_segment(
            ctx,
            "shoulder",
            Some(base),
            Vec3::new(0.0, 0.35, 0.0),
            (0.35, 0.7, 0.35),
            material,
        )?;
        let upper_arm = add_arm_segment(
            ctx,
            "upper_arm",
            Some(shoulder),
            Vec3::new(0.0, 0.75, 0.0),
            (0.28, 0.9, 0.28),
            material,
        )?;
        let lower_arm = add_arm_segment(
            ctx,
            "lower_arm",
            Some(upper_arm),
            Vec3::new(0.0, 0.85, 0.0),
            (0.24, 0.8, 0.24),
            material,
        )?;
        let _hand = add_arm_segment(
            ctx,
            "hand",
            Some(lower_arm),
            Vec3::new(0.0, 0.75, 0.0),
            (0.45, 0.25, 0.35),
            material,
        )?;

        let ground_mesh = ctx.assets.add_mesh(mesh_factory::create_plane(8.0, 8.0));
        let ground = ctx.scene.create_node("ground");
        ctx.scene.set_renderable(
            ground,
            Renderable {
                mesh: MeshSource::Static(ground_mesh),
                material,
            },
        )?;
        ctx.scene.set_local_transform(
            ground,
            Transform {
                translation: Vec3::new(0.0, -0.15, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;

        let light = ctx.scene.create_node("key_light");
        ctx.scene.set_local_transform(
            light,
            Transform {
                translation: Vec3::ZERO,
                rotation: Quat::from_rotation_x(-45.0_f32.to_radians()),
                scale: Vec3::ONE,
            },
        )?;
        ctx.scene.set_light(
            light,
            LightComponent {
                kind: LightKind::Directional {
                    color: Vec3::ONE,
                    intensity: 1.0,
                },
            },
        )?;

        let camera_node = ctx.scene.create_node("camera");
        let eye = Vec3::new(0.0, 2.4, 7.0);
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
                    fov_y_radians: 60.0_f32.to_radians(),
                    near: 0.1,
                    far: 100.0,
                },
            },
        )?;

        let clip = robot_wave_clip();
        let clip_handle = ctx.assets.add_animation_clip(clip);
        let mut player = AnimationPlayer::new(clip_handle);
        player.bind(ctx.assets, ctx.scene)?;

        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        let time_label = debug_hud.add_element(ctx.overlay, Side::Right, "Anim: 0.00 / 0.00");
        let speed_label = debug_hud.add_element(ctx.overlay, Side::Right, "Speed: 1.0x");
        let state_label = debug_hud.add_element(ctx.overlay, Side::Right, "State: playing");

        log::info!("Skeleton demo initialised.");
        log::info!("Controls: WASD/QE move, arrows rotate, Space pauses, +/- changes speed.");

        Ok(Self {
            camera_node,
            camera_rig: CameraRig {
                translation_speed: 3.5,
                rotation_speed: 1.5,
            },
            player,
            debug_hud,
            time_label,
            speed_label,
            state_label,
            space_held: false,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        let space_down = ctx.input.is_key_pressed(KeyCode::Space);
        if space_down && !self.space_held {
            self.player.toggle();
        }
        self.space_held = space_down;

        if ctx.input.is_key_pressed(KeyCode::Equal) || ctx.input.is_key_pressed(KeyCode::NumpadAdd)
        {
            self.player
                .set_speed((self.player.speed() + 0.75 * dt).min(4.0));
        }
        if ctx.input.is_key_pressed(KeyCode::Minus)
            || ctx.input.is_key_pressed(KeyCode::NumpadSubtract)
        {
            self.player
                .set_speed((self.player.speed() - 0.75 * dt).max(0.1));
        }

        self.player.advance(dt);
        self.player.evaluate(ctx.assets, ctx.scene)?;

        *ctx.active_camera = Some(self.camera_node);
        self.camera_rig.update(ctx, self.camera_node, dt)?;
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
            self.time_label,
            format!(
                "Anim: {:.2} / {:.2}",
                self.player.time(),
                self.player.duration()
            ),
        )?;
        ctx.set_text(
            self.speed_label,
            format!("Speed: {:.1}x", self.player.speed()),
        )?;
        let state = if self.player.is_playing() {
            "playing"
        } else {
            "paused"
        };
        ctx.set_text(self.state_label, format!("State: {state}"))
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

fn add_arm_segment(
    ctx: &mut StartupContext<'_>,
    name: &str,
    parent: Option<NodeId>,
    translation: Vec3,
    size: (f32, f32, f32),
    material: rig_app::rig_assets::MaterialHandle,
) -> Result<NodeId> {
    let mesh = ctx
        .assets
        .add_mesh(mesh_factory::create_box(size.0, size.1, size.2));
    let node = ctx.scene.create_node(name);
    ctx.scene.set_renderable(
        node,
        Renderable {
            mesh: MeshSource::Static(mesh),
            material,
        },
    )?;
    ctx.scene.set_local_transform(
        node,
        Transform {
            translation,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        },
    )?;
    if let Some(parent) = parent {
        ctx.scene.attach_child(parent, node)?;
    }
    Ok(node)
}

fn robot_wave_clip() -> AnimationClip {
    AnimationClip {
        name: "robot_wave".to_string(),
        duration: 4.0,
        looping: true,
        channels: vec![
            rotation_channel(
                "shoulder",
                vec![
                    Quat::IDENTITY,
                    Quat::from_rotation_z(30.0_f32.to_radians()),
                    Quat::IDENTITY,
                    Quat::from_rotation_z(-30.0_f32.to_radians()),
                    Quat::IDENTITY,
                ],
            ),
            rotation_channel(
                "upper_arm",
                vec![
                    Quat::IDENTITY,
                    // Bend in the default camera's view plane so the end
                    // effector stays visible instead of hiding behind the arm.
                    Quat::from_rotation_z(-25.0_f32.to_radians()),
                    Quat::IDENTITY,
                ],
            ),
            rotation_channel(
                "lower_arm",
                vec![
                    Quat::IDENTITY,
                    Quat::from_rotation_z(55.0_f32.to_radians()),
                    Quat::from_rotation_z(25.0_f32.to_radians()),
                    Quat::from_rotation_z(55.0_f32.to_radians()),
                    Quat::IDENTITY,
                ],
            ),
        ],
    }
}

fn rotation_channel(target_node: &str, rotations: Vec<Quat>) -> AnimationChannel {
    let duration = 4.0;
    let step = duration / (rotations.len().saturating_sub(1).max(1) as f32);
    let times = (0..rotations.len()).map(|i| i as f32 * step).collect();
    AnimationChannel {
        target_node: target_node.to_string(),
        property: ChannelProperty::Rotation,
        sampler: KeyframeSampler {
            times,
            interpolation: Interpolation::Linear,
            values: KeyframeValues::Rotations(rotations),
        },
    }
}

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<SkeletonDemo>(rig_app::RunConfig {
        title: "Skeleton Demo".into(),
        ..Default::default()
    })
}
