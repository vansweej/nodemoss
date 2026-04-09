//! Application runner and runtime shell for the rig framework.

use std::{collections::HashSet, sync::Arc, time::Instant};

use anyhow::Result;
pub use rig_assets;
pub use rig_math;
pub use rig_render;
pub use rig_scene;
pub use winit;
use rig_assets::AssetStore;
use rig_render::{Renderer, TRIANGLE_SHADER};
use rig_scene::{NodeId, SceneGraph};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::KeyCode,
    window::{Window, WindowId},
};

pub trait Application: Sized + 'static {
    fn init(ctx: &mut StartupContext<'_>) -> Result<Self>;

    fn update(&mut self, ctx: &mut UpdateContext<'_>, dt: f32) -> Result<()>;

    fn render(&mut self, ctx: &mut RenderContext<'_>) -> Result<()>;

    fn on_window_event(&mut self, _ctx: &mut UpdateContext<'_>, _event: &WindowEvent) -> Result<()> {
        Ok(())
    }
}

pub struct StartupContext<'a> {
    pub scene: &'a mut SceneGraph,
    pub assets: &'a mut AssetStore,
    pub renderer: &'a mut Renderer,
    pub window: &'a Window,
}

pub struct UpdateContext<'a> {
    pub scene: &'a mut SceneGraph,
    pub assets: &'a AssetStore,
    pub input: &'a InputState,
    pub timer: &'a FrameTimer,
    pub active_camera: &'a mut Option<NodeId>,
}

pub struct RenderContext<'a> {
    pub scene: &'a SceneGraph,
    pub assets: &'a AssetStore,
    pub renderer: &'a mut Renderer,
    pub active_camera: Option<NodeId>,
}

#[derive(Default)]
pub struct InputState {
    keys: HashSet<KeyCode>,
}

impl InputState {
    pub fn is_key_pressed(&self, key: KeyCode) -> bool {
        self.keys.contains(&key)
    }

    fn update(&mut self, event: &winit::event::KeyEvent) {
        if let winit::keyboard::PhysicalKey::Code(code) = event.physical_key {
            self.update_key(code, event.state);
        }
    }

    fn update_key(&mut self, code: KeyCode, state: winit::event::ElementState) {
        match state {
            winit::event::ElementState::Pressed => {
                self.keys.insert(code);
            }
            winit::event::ElementState::Released => {
                self.keys.remove(&code);
            }
        }
    }
}

pub struct FrameTimer {
    last_instant: Instant,
    frame_count: u64,
    current_fps: f32,
    fps_accumulator: f32,
    fps_frames: u32,
}

impl FrameTimer {
    pub fn new() -> Self {
        Self {
            last_instant: Instant::now(),
            frame_count: 0,
            current_fps: 0.0,
            fps_accumulator: 0.0,
            fps_frames: 0,
        }
    }

    pub fn tick(&mut self) -> f32 {
        let now = Instant::now();
        let dt = (now - self.last_instant).as_secs_f32();
        self.last_instant = now;
        self.apply_delta(dt);
        dt
    }

    fn apply_delta(&mut self, dt: f32) {
        self.frame_count += 1;
        self.fps_accumulator += dt;
        self.fps_frames += 1;

        if self.fps_accumulator >= 1.0 {
            self.current_fps = self.fps_frames as f32 / self.fps_accumulator;
            self.fps_accumulator = 0.0;
            self.fps_frames = 0;
        }
    }

    pub fn fps(&self) -> f32 {
        self.current_fps
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }
}

impl Default for FrameTimer {
    fn default() -> Self {
        Self::new()
    }
}

struct RunnerState<A: Application> {
    app: A,
    scene: SceneGraph,
    assets: AssetStore,
    renderer: Renderer,
    input: InputState,
    timer: FrameTimer,
    active_camera: Option<NodeId>,
}

struct Runner<A: Application> {
    title: String,
    window: Option<Arc<Window>>,
    state: Option<RunnerState<A>>,
}

impl<A: Application> Runner<A> {
    fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            window: None,
            state: None,
        }
    }
}

impl<A: Application> ApplicationHandler for Runner<A> {
    #[cfg(not(tarpaulin_include))]
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(self.title.clone())
                        .with_inner_size(winit::dpi::PhysicalSize::new(800, 600)),
                )
                .expect("failed to create window"),
        );

        let mut renderer = pollster::block_on(Renderer::new(window.clone(), TRIANGLE_SHADER))
            .expect("failed to initialize renderer");
        let mut scene = SceneGraph::new();
        let mut assets = AssetStore::new();
        let input = InputState::default();
        let timer = FrameTimer::new();
        let mut startup = StartupContext {
            scene: &mut scene,
            assets: &mut assets,
            renderer: &mut renderer,
            window: window.as_ref(),
        };
        let app = A::init(&mut startup).expect("failed to initialize application");

        self.window = Some(window);
        self.state = Some(RunnerState {
            app,
            scene,
            assets,
            renderer,
            input,
            timer,
            active_camera: None,
        });
    }

    #[cfg(not(tarpaulin_include))]
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let Some(state) = self.state.as_mut() else {
            return;
        };

        match &event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                state.renderer.resize(*size);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                state.input.update(event);
            }
            _ => {}
        }

        match event {
            WindowEvent::RedrawRequested => {
                let dt = state.timer.tick();
                {
                    let input_snapshot = &state.input;
                    let timer_snapshot = &state.timer;
                    let mut update_ctx = UpdateContext {
                        scene: &mut state.scene,
                        assets: &state.assets,
                        input: input_snapshot,
                        timer: timer_snapshot,
                        active_camera: &mut state.active_camera,
                    };
                    state
                        .app
                        .update(&mut update_ctx, dt)
                        .expect("application update failed");
                }

                state
                    .scene
                    .update_all_world_transforms()
                    .expect("failed to update world transforms");
                state
                    .scene
                    .update_all_world_bounds(&state.assets)
                    .expect("failed to update world bounds");

                {
                    let mut render_ctx = RenderContext {
                        scene: &state.scene,
                        assets: &state.assets,
                        renderer: &mut state.renderer,
                        active_camera: state.active_camera,
                    };
                    state
                        .app
                        .render(&mut render_ctx)
                        .expect("application render failed");
                }
            }
            other => {
                let input_snapshot = &state.input;
                let timer_snapshot = &state.timer;
                let mut update_ctx = UpdateContext {
                    scene: &mut state.scene,
                    assets: &state.assets,
                    input: input_snapshot,
                    timer: timer_snapshot,
                    active_camera: &mut state.active_camera,
                };
                state
                    .app
                    .on_window_event(&mut update_ctx, &other)
                    .expect("application window event failed");
            }
        }

        window.request_redraw();
    }

    #[cfg(not(tarpaulin_include))]
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

#[cfg(not(tarpaulin_include))]
pub fn run<A: Application>(title: impl Into<String>) -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    let mut runner = Runner::<A>::new(title);
    event_loop.run_app(&mut runner)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::{event::ElementState, keyboard::KeyCode};

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

    struct TestApp;

    impl Application for TestApp {
        fn init(_ctx: &mut StartupContext<'_>) -> Result<Self> {
            Ok(Self)
        }

        fn update(&mut self, _ctx: &mut UpdateContext<'_>, _dt: f32) -> Result<()> {
            Ok(())
        }

        fn render(&mut self, _ctx: &mut RenderContext<'_>) -> Result<()> {
            Ok(())
        }
    }
}
