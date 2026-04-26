//! Application runner and runtime shell for the rig framework.

mod camera_rig;
mod context;
mod input;
mod runner;
mod timer;

pub use camera_rig::CameraRig;
pub use context::{OverlayUpdateContext, RenderContext, StartupContext, UpdateContext};
pub use input::InputState;
pub use runner::run;
pub use timer::FrameTimer;

pub use rig_assets;
pub use rig_gpu;
pub use rig_math;
pub use rig_overlay;
pub use rig_render;
pub use rig_scene;
pub use winit;

use anyhow::Result;

// Re-export overlay types for convenience.
pub use rig_overlay::{Anchor, OverlayError};

pub trait Application: Sized + 'static {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self>;

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()>;

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()>;

    #[cfg(not(tarpaulin_include))]
    fn update_overlay(&mut self, _ctx: &mut OverlayUpdateContext<'_>) -> Result<()> {
        Ok(())
    }

    #[cfg(not(tarpaulin_include))]
    fn on_window_event(
        &mut self,
        _ctx: &mut UpdateContext<'_>,
        _event: &winit::event::WindowEvent,
    ) -> Result<()> {
        Ok(())
    }
}


#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use runner::Runner;

#[cfg(test)]
mod tests {
    use super::*;
    use winit::{event::ElementState, keyboard::KeyCode};
    use rig_assets::AssetStore;
    use rig_scene::SceneGraph;

    #[test]
    fn input_state_tracks_pressed_key() {
        let mut input = InputState::default();
        input.update_key(KeyCode::KeyW, ElementState::Pressed);
        assert!(input.is_key_pressed(KeyCode::KeyW));
    }

    #[test]
    fn input_state_releases_key() {
        let mut input = InputState::default();
        input.update_key(KeyCode::KeyW, ElementState::Pressed);
        input.update_key(KeyCode::KeyW, ElementState::Released);
        assert!(!input.is_key_pressed(KeyCode::KeyW));
    }

    #[test]
    fn key_axis_tracks_positive_and_negative_input() {
        let mut input = InputState::default();
        input.update_key(KeyCode::KeyA, ElementState::Pressed);
        assert_eq!(input::key_axis(&input, KeyCode::KeyA, KeyCode::KeyD), -1.0);
        input.update_key(KeyCode::KeyA, ElementState::Released);
        input.update_key(KeyCode::KeyD, ElementState::Pressed);
        assert_eq!(input::key_axis(&input, KeyCode::KeyA, KeyCode::KeyD), 1.0);
    }

    #[test]
    fn camera_rig_moves_camera_forward() {
        use rig_math::Vec3;
        let mut scene = SceneGraph::new();
        let camera = scene.create_node("camera");
        let assets = AssetStore::new();
        let input = {
            let mut input = InputState::default();
            input.update_key(KeyCode::KeyW, ElementState::Pressed);
            input
        };
        let timer = FrameTimer::new();
        let mut active_camera = None;
        let mut exit_requested = false;
        let mut ctx = UpdateContext {
            scene: &mut scene,
            assets: &assets,
            input: &input,
            timer: &timer,
            active_camera: &mut active_camera,
            exit_requested: &mut exit_requested,
        };
        CameraRig { translation_speed: 4.0, rotation_speed: 1.0 }.update(&mut ctx, camera, 0.5).unwrap();
        let transform = scene.local_transform(camera).unwrap();
        assert!(transform.translation.abs_diff_eq(Vec3::new(0.0, 0.0, -2.0), 1e-5));
    }

    #[test]
    fn camera_rig_rotates_camera_with_arrow_keys() {
        use rig_math::{Quat, Vec3};
        let mut scene = SceneGraph::new();
        let camera = scene.create_node("camera");
        let assets = AssetStore::new();
        let input = {
            let mut input = InputState::default();
            input.update_key(KeyCode::ArrowRight, ElementState::Pressed);
            input
        };
        let timer = FrameTimer::new();
        let mut active_camera = None;
        let mut exit_requested = false;
        let mut ctx = UpdateContext {
            scene: &mut scene,
            assets: &assets,
            input: &input,
            timer: &timer,
            active_camera: &mut active_camera,
            exit_requested: &mut exit_requested,
        };
        CameraRig { translation_speed: 1.0, rotation_speed: 2.0 }.update(&mut ctx, camera, 0.25).unwrap();
        let transform = scene.local_transform(camera).unwrap();
        assert!(transform.rotation.abs_diff_eq(Quat::from_rotation_y(-0.5), 1e-5));
    }

    #[test]
    fn frame_timer_defaults_match_new() {
        let timer = FrameTimer::default();
        assert_eq!(timer.frame_count(), 0);
        assert_eq!(timer.fps(), 0.0);
    }

    #[test]
    fn frame_timer_tick_advances_frame_count() {
        let mut timer = FrameTimer::new();
        let dt = timer.tick();
        assert!(dt >= 0.0);
        assert_eq!(timer.frame_count(), 1);
    }

    #[test]
    fn frame_timer_updates_fps_after_one_second_of_accumulated_time() {
        let mut timer = FrameTimer::new();
        timer.apply_delta(0.25);
        timer.apply_delta(0.25);
        timer.apply_delta(0.25);
        timer.apply_delta(0.25);
        assert_eq!(timer.frame_count(), 4);
        assert!((timer.fps() - 4.0).abs() <= 1e-5);
    }

    #[test]
    fn frame_timer_accumulator_resets_after_fps_update() {
        let mut timer = FrameTimer::new();
        timer.apply_delta(1.5);
        assert_eq!(timer.frame_count(), 1);
        assert!((timer.fps() - (1.0 / 1.5)).abs() <= 1e-5);
        assert_eq!(timer.fps_accumulator, 0.0);
        assert_eq!(timer.fps_frames, 0);
    }

    #[test]
    fn runner_new_starts_empty() {
        let runner = Runner::<TestApp>::new("test");
        assert_eq!(runner.title, "test");
        assert!(runner.window.is_none());
        assert!(runner.state.is_none());
    }

    #[test]
    fn update_context_request_exit_sets_flag() {
        let mut scene = SceneGraph::new();
        let assets = AssetStore::new();
        let input = InputState::default();
        let timer = FrameTimer::new();
        let mut active_camera = None;
        let mut exit_requested = false;
        let mut ctx = UpdateContext {
            scene: &mut scene,
            assets: &assets,
            input: &input,
            timer: &timer,
            active_camera: &mut active_camera,
            exit_requested: &mut exit_requested,
        };
        ctx.request_exit();
        assert!(exit_requested);
    }

    struct TestApp;

    impl Application for TestApp {
        fn init(_ctx: &mut StartupContext<'_>) -> anyhow::Result<Self> {
            Ok(Self)
        }
        fn update(&mut self, _ctx: &mut UpdateContext<'_>, _dt: f32) -> anyhow::Result<()> {
            Ok(())
        }
        fn render(&mut self, _ctx: &mut RenderContext<'_>) -> anyhow::Result<()> {
            Ok(())
        }
    }
}
