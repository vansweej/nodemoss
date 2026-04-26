//! Fly camera rig driven by keyboard input.

use rig_math::{Quat, Vec3};
use rig_scene::NodeId;

use crate::context::UpdateContext;
use crate::input::key_axis;

#[derive(Clone, Copy, Debug)]
pub struct CameraRig {
    pub translation_speed: f32,
    pub rotation_speed: f32,
}

impl CameraRig {
    pub fn update(
        &self,
        ctx: &mut UpdateContext<'_>,
        node: NodeId,
        dt: f32,
    ) -> rig_scene::Result<()> {
        use winit::keyboard::KeyCode;
        let mut transform = ctx.scene.local_transform(node)?;

        let yaw = key_axis(ctx.input, KeyCode::ArrowLeft, KeyCode::ArrowRight) * self.rotation_speed * dt;
        if yaw != 0.0 {
            transform.rotation = Quat::from_rotation_y(-yaw) * transform.rotation;
        }

        let right = transform.rotation * Vec3::X;
        let pitch = key_axis(ctx.input, KeyCode::ArrowDown, KeyCode::ArrowUp) * self.rotation_speed * dt;
        if pitch != 0.0 {
            transform.rotation = Quat::from_axis_angle(right, pitch) * transform.rotation;
        }
        transform.rotation = transform.rotation.normalize();

        let forward = -(transform.rotation * Vec3::Z);
        let up = transform.rotation * Vec3::Y;
        let translation = (forward * key_axis(ctx.input, KeyCode::KeyS, KeyCode::KeyW)
            + right * key_axis(ctx.input, KeyCode::KeyA, KeyCode::KeyD)
            + up * key_axis(ctx.input, KeyCode::KeyQ, KeyCode::KeyE))
            * self.translation_speed
            * dt;

        if translation != Vec3::ZERO {
            transform.translation += translation;
        }

        ctx.scene.set_local_transform(node, transform)
    }
}

#[cfg(not(tarpaulin_include))]
impl Default for CameraRig {
    fn default() -> Self {
        Self {
            translation_speed: 2.5,
            rotation_speed: 1.5,
        }
    }
}
