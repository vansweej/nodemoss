//! glTF CPU skinning demo.
//!
//! Loads a skinned glTF/GLB asset, evaluates its first animation with
//! `AnimationPlayer`, skins every skinned primitive on the CPU with
//! `SkinEvaluator`, and renders those primitives through the dynamic mesh path.
//!
//! # Controls
//!
//! | Input          | Action                    |
//! |----------------|---------------------------|
//! | WASD / arrows  | Fly camera                |
//! | LMB drag       | Orbit model               |
//! | RMB drag       | Dolly (zoom)              |
//! | Space          | Pause / resume animation  |
//! | Escape         | Quit                      |
//! | F3             | Toggle overlay            |

use std::sync::Arc;

use anyhow::{Context, Result};
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, Side, StartupContext,
    TrackBall, UpdateContext,
    rig_anim::AnimationPlayer,
    rig_assets::{DynamicMeshData, DynamicMeshId, MeshSource, ShaderAsset},
    rig_gltf::{LoadedGltf, SkinnedPrimitive, load_gltf},
    rig_math::{Projection, Quat, Transform, Vec3},
    rig_render::PBR_SHADER,
    rig_scene::{CameraComponent, LightComponent, LightKind, NodeId, Renderable},
    rig_skin::SkinEvaluator,
    winit::{
        event::WindowEvent,
        keyboard::{KeyCode, PhysicalKey},
    },
};

const DEFAULT_MODEL_PATH: &str = "assets/models/gltf/BrainStem.glb";

struct SkinnedRuntimePrimitive {
    descriptor: SkinnedPrimitive,
    evaluator: SkinEvaluator,
    dynamic_mesh: DynamicMeshId,
    pending_mesh: Option<DynamicMeshData>,
}

struct GltfSkinnedDemo {
    camera_node: NodeId,
    camera_rig: CameraRig,
    trackball: TrackBall,
    debug_hud: DebugHud,
    animation_player: Option<AnimationPlayer>,
    skinned_primitives: Vec<SkinnedRuntimePrimitive>,
    space_held: bool,
    _loaded: LoadedGltf,
}

impl Application for GltfSkinnedDemo {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let path = std::env::args()
            .nth(1)
            .unwrap_or_else(|| DEFAULT_MODEL_PATH.to_string());
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(PBR_SHADER),
        });
        let loaded = load_gltf(&path, shader, ctx.scene, ctx.assets)?;

        let target_node = ctx.scene.create_node("gltf_skinned_focus");
        ctx.scene
            .set_local_transform(target_node, Transform::IDENTITY)?;

        ensure_light(ctx)?;
        let camera_node = create_camera(ctx)?;
        let eye = Vec3::new(0.0, 1.5, 5.0);

        let mut animation_player = loaded
            .animations
            .first()
            .map(|&clip| AnimationPlayer::new(clip));
        if let Some(player) = &mut animation_player {
            player.bind(ctx.assets, ctx.scene)?;
        }

        let skinned_primitives = create_skinned_primitives(ctx, &loaded.skinned_primitives)
            .context("failed to create CPU skinning runtime primitives")?;

        let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
        debug_hud.add_element(ctx.overlay, Side::Left, "glTF Skinned Demo");
        debug_hud.add_element(ctx.overlay, Side::Left, format!("Model: {path}"));
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            format!("Skinned primitives: {}", skinned_primitives.len()),
        );
        debug_hud.add_element(
            ctx.overlay,
            Side::Left,
            "WASD/arrows: fly  LMB: orbit  RMB: dolly  Space: pause",
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
            skinned_primitives,
            space_held: false,
            _loaded: loaded,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        let space_down = ctx.input.is_key_pressed(KeyCode::Space);
        if space_down
            && !self.space_held
            && let Some(player) = &mut self.animation_player
        {
            player.toggle();
        }
        self.space_held = space_down;

        if let Some(player) = &mut self.animation_player {
            player.advance(dt);
            player.evaluate(ctx.assets, ctx.scene)?;
        }

        ctx.scene.update_all_world_transforms()?;
        for primitive in &mut self.skinned_primitives {
            let mesh_data = primitive.evaluator.evaluate(ctx.assets, ctx.scene)?;
            ctx.scene
                .set_dynamic_bounds(primitive.descriptor.node, mesh_data.local_bounds)?;
            primitive.pending_mesh = Some(mesh_data);
        }

        *ctx.active_camera = Some(self.camera_node);
        self.camera_rig.update(ctx, self.camera_node, dt)?;
        self.trackball.sync_to_camera(ctx.scene, self.camera_node)?;
        self.trackball
            .update(ctx.input, ctx.scene, self.camera_node, dt)?;
        ctx.scene.update_all_world_transforms()?;
        ctx.scene.update_all_world_bounds(ctx.assets)?;
        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
        for primitive in &mut self.skinned_primitives {
            if let Some(data) = primitive.pending_mesh.take() {
                ctx.renderer.update_dynamic_mesh(
                    &ctx.gpu.device,
                    &ctx.gpu.queue,
                    primitive.dynamic_mesh,
                    &data,
                );
            }
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

fn create_skinned_primitives(
    ctx: &mut StartupContext<'_>,
    descriptors: &[SkinnedPrimitive],
) -> Result<Vec<SkinnedRuntimePrimitive>> {
    let mut runtime_primitives = Vec::with_capacity(descriptors.len());
    for (index, descriptor) in descriptors.iter().copied().enumerate() {
        let dynamic_mesh = DynamicMeshId::from_raw(index as u32);
        let mesh = ctx.assets.mesh(descriptor.mesh)?;
        ctx.renderer.register_dynamic_mesh(
            &ctx.gpu.device,
            dynamic_mesh,
            (mesh.vertex_data.len() * 2) as u64,
            (mesh.index_data.len() * 2) as u64,
        );
        ctx.scene.set_renderable(
            descriptor.node,
            Renderable {
                mesh: MeshSource::Dynamic(dynamic_mesh),
                material: descriptor.material,
            },
        )?;
        let mut evaluator = SkinEvaluator::new(
            descriptor.skin,
            descriptor.skin_weights,
            descriptor.mesh,
            descriptor.node,
        );
        evaluator.bind(ctx.assets, ctx.scene)?;
        runtime_primitives.push(SkinnedRuntimePrimitive {
            descriptor,
            evaluator,
            dynamic_mesh,
            pending_mesh: None,
        });
    }
    Ok(runtime_primitives)
}

fn create_camera(ctx: &mut StartupContext<'_>) -> Result<NodeId> {
    let camera_node = ctx.scene.create_node("camera");
    let eye = Vec3::new(0.0, 1.5, 5.0);
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
    Ok(camera_node)
}

fn ensure_light(ctx: &mut StartupContext<'_>) -> Result<()> {
    if !ctx.scene.light_nodes().is_empty() {
        return Ok(());
    }
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
    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<GltfSkinnedDemo>(rig_app::RunConfig {
        title: "glTF Skinned Demo".into(),
        ..Default::default()
    })
}
