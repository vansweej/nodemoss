//! Tentacle demo — CPU linear blend skinning.
//!
//! A 4-bone cylinder deforms smoothly via weighted vertex skinning,
//! demonstrating `SkinEvaluator` from `rig-skin`.
//!
//! # Controls
//!
//! | Key(s)      | Action                        |
//! |-------------|-------------------------------|
//! | W / S       | Move camera forward / backward|
//! | A / D       | Strafe left / right           |
//! | Q / E       | Move camera down / up         |
//! | Arrow keys  | Rotate camera (yaw / pitch)   |
//! | LMB drag    | Orbit camera around tentacle  |
//! | RMB drag    | Dolly camera in / out         |
//! | Space       | Pause / resume animation      |
//! | Escape      | Close window                  |
//! | F3          | Toggle overlay                |

use std::f32::consts::PI;
use std::sync::Arc;

use anyhow::Result;
use rig_app::{
    Application, CameraRig, DebugHud, OverlayUpdateContext, RenderContext, StartupContext,
    TrackBall, UpdateContext,
    rig_anim::AnimationPlayer,
    rig_assets::{
        AlphaMode, AnimationChannel, AnimationClip, ChannelProperty, DynamicMeshData,
        DynamicMeshId, IndexFormat, KeyframeSampler, KeyframeValues, MaterialAsset, MeshAsset,
        MeshSource, ShaderAsset, SkinAsset, SkinWeights, VertexAttribute, VertexFormat,
        VertexLayout,
    },
    rig_math::{BoundingSphere, Interpolation, Mat4, Projection, Quat, Transform, Vec3},
    rig_render::NORMAL_COLOR_SHADER,
    rig_scene::{CameraComponent, LightComponent, LightKind, NodeId, Renderable},
    rig_skin::SkinEvaluator,
    winit::{
        event::WindowEvent,
        keyboard::{KeyCode, PhysicalKey},
    },
};

const RADIAL_SEGMENTS: usize = 12;
const AXIS_SLICES: usize = 9;
const RING_COUNT: usize = AXIS_SLICES + 1;
const VERTICES_PER_RING: usize = RADIAL_SEGMENTS + 1;
const VERTEX_COUNT: usize = RING_COUNT * VERTICES_PER_RING;
const INDEX_COUNT: usize = RADIAL_SEGMENTS * AXIS_SLICES * 6;
const STRIDE: u64 = 48;

struct TentacleDemo {
    camera_node: NodeId,
    camera_rig: CameraRig,
    trackball: TrackBall,
    player: AnimationPlayer,
    skin_evaluator: SkinEvaluator,
    tentacle_node: NodeId,
    dyn_id: DynamicMeshId,
    pending_mesh: Option<DynamicMeshData>,
    debug_hud: DebugHud,
    space_held: bool,
}

impl Application for TentacleDemo {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self> {
        let shader = ctx.assets.add_shader(ShaderAsset {
            source: Arc::from(NORMAL_COLOR_SHADER),
        });
        let material = ctx.assets.add_material(MaterialAsset {
            shader,
            parameters: Default::default(),
            textures: vec![],
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
        });

        let mesh_handle = ctx.assets.add_mesh(create_cylinder_mesh());
        let skin_handle = ctx.assets.add_skin(create_tentacle_skin());
        let weights_handle = ctx.assets.add_skin_weights(create_tentacle_weights());

        let bone_0 = ctx.scene.create_node("bone_0");
        ctx.scene.set_local_transform(bone_0, Transform::IDENTITY)?;
        let bone_1 = create_bone(ctx, "bone_1", bone_0)?;
        let bone_2 = create_bone(ctx, "bone_2", bone_1)?;
        let _bone_3 = create_bone(ctx, "bone_3", bone_2)?;

        let dyn_id = DynamicMeshId::from_raw(0);
        ctx.renderer.register_dynamic_mesh(
            &ctx.gpu.device,
            dyn_id,
            (VERTEX_COUNT * STRIDE as usize * 2) as u64,
            (INDEX_COUNT * 2 * 2) as u64,
        );

        let tentacle_node = ctx.scene.create_node("tentacle");
        ctx.scene.set_renderable(
            tentacle_node,
            Renderable {
                mesh: MeshSource::Dynamic(dyn_id),
                material,
            },
        )?;
        ctx.scene
            .set_local_transform(tentacle_node, Transform::IDENTITY)?;

        let trackball_target = ctx.scene.create_node("tentacle_focus");
        ctx.scene.set_local_transform(
            trackball_target,
            Transform {
                translation: Vec3::new(0.0, 2.0, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        )?;

        let clip_handle = ctx.assets.add_animation_clip(tentacle_wave_clip());
        let mut player = AnimationPlayer::new(clip_handle);
        player.bind(ctx.assets, ctx.scene)?;

        let mut skin_evaluator =
            SkinEvaluator::new(skin_handle, weights_handle, mesh_handle, tentacle_node);
        skin_evaluator.bind(ctx.assets, ctx.scene)?;

        let camera_node = ctx.scene.create_node("camera");
        let eye = Vec3::new(0.0, 2.0, 8.0);
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

        let debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);

        log::info!("Tentacle demo initialised. Space pauses, F3 toggles overlay.");

        Ok(Self {
            camera_node,
            camera_rig: CameraRig {
                translation_speed: 3.5,
                rotation_speed: 1.5,
            },
            trackball: TrackBall::new(trackball_target, 8.0),
            player,
            skin_evaluator,
            tentacle_node,
            dyn_id,
            pending_mesh: None,
            debug_hud,
            space_held: false,
        })
    }

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()> {
        let space_down = ctx.input.is_key_pressed(KeyCode::Space);
        if space_down && !self.space_held {
            self.player.toggle();
        }
        self.space_held = space_down;

        self.player.advance(dt);
        self.player.evaluate(ctx.assets, ctx.scene)?;

        ctx.scene.update_all_world_transforms()?;

        let mesh_data = self.skin_evaluator.evaluate(ctx.assets, ctx.scene)?;
        ctx.scene
            .set_dynamic_bounds(self.tentacle_node, mesh_data.local_bounds)?;
        self.pending_mesh = Some(mesh_data);

        *ctx.active_camera = Some(self.camera_node);
        self.camera_rig.update(ctx, self.camera_node, dt)?;
        self.trackball.sync_to_camera(ctx.scene, self.camera_node)?;
        self.trackball
            .update(ctx.input, ctx.scene, self.camera_node, dt)?;
        Ok(())
    }

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()> {
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
            && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
            && event.state == rig_app::winit::event::ElementState::Pressed
        {
            ctx.request_exit();
        }
        Ok(())
    }
}

fn create_bone(ctx: &mut StartupContext<'_>, name: &str, parent: NodeId) -> Result<NodeId> {
    let node = ctx.scene.create_node(name);
    ctx.scene.set_local_transform(
        node,
        Transform {
            translation: Vec3::new(0.0, 1.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        },
    )?;
    ctx.scene.attach_child(parent, node)?;
    Ok(node)
}

fn create_cylinder_mesh() -> MeshAsset {
    let mut vertex_data = Vec::with_capacity(VERTEX_COUNT * STRIDE as usize);
    let mut index_data = Vec::with_capacity(INDEX_COUNT * 2);
    let radius = 0.3_f32;
    let height = 4.0_f32;

    for ring in 0..RING_COUNT {
        let v = ring as f32 / AXIS_SLICES as f32;
        let y = v * height;
        for column in 0..=RADIAL_SEGMENTS {
            let u = column as f32 / RADIAL_SEGMENTS as f32;
            let theta = u * 2.0 * PI;
            let (sin, cos) = theta.sin_cos();
            push_vertex(
                &mut vertex_data,
                [radius * cos, y, radius * sin],
                [cos, 0.0, sin],
                [u, v],
                [-sin, 0.0, cos, 1.0],
            );
        }
    }

    for ring in 0..AXIS_SLICES {
        for column in 0..RADIAL_SEGMENTS {
            let a = (ring * VERTICES_PER_RING + column) as u16;
            let b = a + 1;
            let c = ((ring + 1) * VERTICES_PER_RING + column) as u16;
            let d = c + 1;
            push_u16(&mut index_data, a);
            push_u16(&mut index_data, b);
            push_u16(&mut index_data, c);
            push_u16(&mut index_data, b);
            push_u16(&mut index_data, d);
            push_u16(&mut index_data, c);
        }
    }

    MeshAsset {
        vertex_layout: standard_layout(),
        vertex_data: Arc::from(vertex_data.as_slice()),
        index_data: Arc::from(index_data.as_slice()),
        index_format: IndexFormat::Uint16,
        local_bounds: BoundingSphere {
            center: Vec3::new(0.0, height * 0.5, 0.0),
            radius: Vec3::new(radius, height * 0.5, radius).length(),
        },
    }
}

fn create_tentacle_weights() -> SkinWeights {
    let mapping = [
        ([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
        ([0, 1, 0, 0], [0.5, 0.5, 0.0, 0.0]),
        ([1, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
        ([1, 2, 0, 0], [0.5, 0.5, 0.0, 0.0]),
        ([2, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
        ([2, 3, 0, 0], [0.5, 0.5, 0.0, 0.0]),
        ([3, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
        ([3, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
        ([3, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
        ([3, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]),
    ];
    let mut joints = Vec::with_capacity(VERTEX_COUNT);
    let mut weights = Vec::with_capacity(VERTEX_COUNT);
    for (ring_joints, ring_weights) in mapping.iter().take(RING_COUNT) {
        for _ in 0..VERTICES_PER_RING {
            joints.push([
                ring_joints[0],
                ring_joints[1],
                ring_joints[2],
                ring_joints[3],
                0,
                0,
                0,
                0,
            ]);
            weights.push([
                ring_weights[0],
                ring_weights[1],
                ring_weights[2],
                ring_weights[3],
                0.0,
                0.0,
                0.0,
                0.0,
            ]);
        }
    }
    SkinWeights { joints, weights }
}

fn create_tentacle_skin() -> SkinAsset {
    SkinAsset {
        joint_names: vec![
            "bone_0".to_string(),
            "bone_1".to_string(),
            "bone_2".to_string(),
            "bone_3".to_string(),
        ],
        inverse_bind_matrices: vec![
            Mat4::IDENTITY,
            Mat4::from_translation(Vec3::new(0.0, -1.0, 0.0)),
            Mat4::from_translation(Vec3::new(0.0, -2.0, 0.0)),
            Mat4::from_translation(Vec3::new(0.0, -3.0, 0.0)),
        ],
    }
}

fn tentacle_wave_clip() -> AnimationClip {
    AnimationClip {
        name: "tentacle_wave".to_string(),
        duration: 4.0,
        looping: true,
        channels: vec![
            rotation_channel("bone_0", [0.0, 20.0, 0.0, -20.0, 0.0]),
            rotation_channel("bone_1", [15.0, 30.0, -15.0, -30.0, 15.0]),
            rotation_channel("bone_2", [30.0, 0.0, -30.0, 0.0, 30.0]),
            rotation_channel("bone_3", [15.0, -30.0, 15.0, 30.0, 15.0]),
        ],
    }
}

fn rotation_channel(target_node: &str, degrees: [f32; 5]) -> AnimationChannel {
    AnimationChannel {
        target_node: target_node.to_string(),
        property: ChannelProperty::Rotation,
        sampler: KeyframeSampler {
            times: vec![0.0, 1.0, 2.0, 3.0, 4.0],
            interpolation: Interpolation::Linear,
            values: KeyframeValues::Rotations(
                degrees
                    .into_iter()
                    .map(|angle| Quat::from_rotation_z(angle.to_radians()))
                    .collect(),
            ),
        },
    }
}

fn standard_layout() -> VertexLayout {
    VertexLayout {
        array_stride: STRIDE,
        attributes: vec![
            VertexAttribute {
                shader_location: 0,
                format: VertexFormat::Float32x3,
                offset: 0,
            },
            VertexAttribute {
                shader_location: 1,
                format: VertexFormat::Float32x3,
                offset: 12,
            },
            VertexAttribute {
                shader_location: 2,
                format: VertexFormat::Float32x2,
                offset: 24,
            },
            VertexAttribute {
                shader_location: 3,
                format: VertexFormat::Float32x4,
                offset: 32,
            },
        ],
    }
}

fn push_vertex(
    buf: &mut Vec<u8>,
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
    tangent: [f32; 4],
) {
    for f in pos
        .iter()
        .chain(normal.iter())
        .chain(uv.iter())
        .chain(tangent.iter())
    {
        buf.extend_from_slice(&f.to_le_bytes());
    }
}

fn push_u16(buf: &mut Vec<u8>, idx: u16) {
    buf.extend_from_slice(&idx.to_le_bytes());
}

fn main() -> Result<()> {
    env_logger::init();
    rig_app::run::<TentacleDemo>(rig_app::RunConfig {
        title: "Tentacle Demo".into(),
        ..Default::default()
    })
}
