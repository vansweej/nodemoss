//! Arc-ball camera controller for orbiting around a target scene node.

use rig_math::{Quat, Transform, Vec3};
use rig_scene::{NodeId, SceneGraph};

use crate::input::InputState;

/// Arc-ball orbit controller.
///
/// Maps left-mouse drag to rotation around a target node's world position,
/// and right-mouse drag to dolly (distance change).
///
/// # Example
/// ```no_run
/// # use rig_app::{TrackBall, InputState};
/// # use rig_scene::SceneGraph;
/// // In Application::init:
/// # let mut scene = SceneGraph::new();
/// # let target_node = scene.create_node("target");
/// # let camera_node = scene.create_node("camera");
/// let mut trackball = TrackBall::new(target_node, 5.0);
///
/// // In Application::update:
/// # let mut input = InputState::default();
/// trackball.update(&input, &mut scene, camera_node, 0.016).unwrap();
/// ```
pub struct TrackBall {
    /// The scene node to orbit around.
    pub target: NodeId,
    /// Distance from target to camera.
    pub distance: f32,
    /// Mouse sensitivity for rotation (radians per pixel).
    pub sensitivity: f32,
    /// Current yaw angle in radians.
    yaw: f32,
    /// Current pitch angle in radians (clamped to avoid gimbal flip).
    pitch: f32,
}

impl TrackBall {
    /// Create a new TrackBall orbiting `target` at the given `distance`.
    pub fn new(target: NodeId, distance: f32) -> Self {
        Self {
            target,
            distance,
            sensitivity: 0.005,
            yaw: 0.0,
            pitch: 0.3,
        }
    }

    /// Update the camera node transform based on current input.
    ///
    /// - Left mouse button held: orbit (yaw/pitch) around the target.
    /// - Right mouse button held: dolly (adjust distance).
    pub fn update(
        &mut self,
        input: &InputState,
        scene: &mut SceneGraph,
        camera_node: NodeId,
        _dt: f32,
    ) -> Result<(), rig_scene::SceneError> {
        use winit::event::MouseButton;

        // Orbit on left mouse drag
        if input.is_mouse_button_pressed(MouseButton::Left) {
            self.yaw -= input.mouse_delta.x * self.sensitivity;
            self.pitch -= input.mouse_delta.y * self.sensitivity;
            // Clamp pitch to avoid flipping
            self.pitch = self.pitch.clamp(
                -std::f32::consts::FRAC_PI_2 + 0.05,
                std::f32::consts::FRAC_PI_2 - 0.05,
            );
        }

        // Dolly on right mouse drag
        if input.is_mouse_button_pressed(MouseButton::Right) {
            self.distance = (self.distance + input.mouse_delta.y * 0.01).max(0.1);
        }

        // Compute camera position from yaw/pitch/distance
        let target_world = scene
            .world_transform(self.target)?
            .transform_point3(Vec3::ZERO);

        let rotation = Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch);
        let offset = rotation * Vec3::new(0.0, 0.0, self.distance);
        let camera_pos = target_world + offset;

        // Look at target
        let forward = (target_world - camera_pos).normalize_or_zero();
        let look_rotation = if forward.length_squared() > 1e-6 {
            let right = Vec3::Y.cross(forward).normalize_or_zero();
            let up = forward.cross(right).normalize_or_zero();
            Quat::from_mat3(&rig_math::glam::Mat3::from_cols(right, up, -forward))
        } else {
            Quat::IDENTITY
        };

        scene.set_local_transform(
            camera_node,
            Transform {
                translation: camera_pos,
                rotation: look_rotation,
                scale: Vec3::ONE,
            },
        )?;
        Ok(())
    }
}
