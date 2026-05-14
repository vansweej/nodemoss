//! Arc-ball camera controller for orbiting around a target scene node.

use rig_math::{Quat, Transform, Vec3};
use rig_scene::{NodeId, SceneGraph};

use crate::input::InputState;

const MIN_DISTANCE: f32 = 0.1;
const MIN_PITCH: f32 = -std::f32::consts::FRAC_PI_2 + 0.05;
const MAX_PITCH: f32 = std::f32::consts::FRAC_PI_2 - 0.05;

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
    /// Offset from the target node's world position, used for camera panning.
    focus_offset: Vec3,
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
            focus_offset: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    /// Orbit by explicit yaw/pitch deltas in radians.
    pub fn orbit_by(&mut self, yaw_delta: f32, pitch_delta: f32) {
        self.yaw += yaw_delta;
        self.pitch = (self.pitch + pitch_delta).clamp(MIN_PITCH, MAX_PITCH);
    }

    /// Adjust camera distance by an explicit delta.
    pub fn dolly_by(&mut self, distance_delta: f32) {
        self.distance = (self.distance + distance_delta).max(MIN_DISTANCE);
    }

    /// Pan the orbit focus in the camera's right/up plane.
    pub fn pan_by(&mut self, right_delta: f32, up_delta: f32) {
        let rotation = self.rotation();
        let right = rotation * Vec3::X;
        let up = rotation * Vec3::Y;
        self.focus_offset += right * right_delta + up * up_delta;
    }

    fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch)
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
            self.orbit_by(
                -input.mouse_delta.x * self.sensitivity,
                -input.mouse_delta.y * self.sensitivity,
            );
        }

        // Dolly on right mouse drag
        if input.is_mouse_button_pressed(MouseButton::Right) {
            self.dolly_by(input.mouse_delta.y * 0.01);
        }

        // Compute camera position from yaw/pitch/distance
        let target_world = scene
            .world_transform(self.target)?
            .transform_point3(Vec3::ZERO)
            + self.focus_offset;

        let rotation = self.rotation();
        let offset = rotation * Vec3::new(0.0, 0.0, self.distance);
        let camera_pos = target_world + offset;

        // The orbit rotation already orients the camera's -Z axis toward the target.
        // Using it directly avoids look-at edge cases and is mathematically equivalent.
        let look_rotation = rotation;

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
