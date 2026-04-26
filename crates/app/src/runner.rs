//! Winit event loop runner.

use std::sync::Arc;

use anyhow::Result;
use rig_assets::AssetStore;
use rig_gpu::GpuContext;
use rig_overlay::Overlay;
use rig_render::Renderer;
use rig_scene::{NodeId, SceneGraph};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::KeyCode,
    window::{Window, WindowId},
};

use crate::context::{OverlayUpdateContext, RenderContext, StartupContext, UpdateContext};
use crate::input::InputState;
use crate::timer::FrameTimer;
use crate::Application;

pub(crate) struct RunnerState<A: Application> {
    pub(crate) app: A,
    pub(crate) scene: SceneGraph,
    pub(crate) assets: AssetStore,
    pub(crate) gpu: GpuContext,
    pub(crate) renderer: Renderer,
    pub(crate) overlay: Overlay,
    pub(crate) overlay_visible: bool,
    pub(crate) input: InputState,
    pub(crate) timer: FrameTimer,
    pub(crate) active_camera: Option<NodeId>,
    pub(crate) exit_requested: bool,
}

pub(crate) struct Runner<A: Application> {
    pub(crate) title: String,
    pub(crate) window: Option<Arc<Window>>,
    pub(crate) state: Option<RunnerState<A>>,
}

impl<A: Application> Runner<A> {
    pub(crate) fn new(title: impl Into<String>) -> Self {
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
        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title(self.title.clone())
                .with_inner_size(winit::dpi::PhysicalSize::new(800, 600)),
        ) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };
        let gpu = match pollster::block_on(GpuContext::new(window.clone())) {
            Ok(g) => g,
            Err(e) => {
                log::error!("failed to initialize GPU context: {e}");
                event_loop.exit();
                return;
            }
        };
        let mut renderer = Renderer::new(&gpu);
        let mut overlay = Overlay::new(&gpu.device, &gpu.queue, gpu.surface_format(), gpu.width(), gpu.height());
        let mut scene = SceneGraph::new();
        let mut assets = AssetStore::new();
        let input = InputState::default();
        let timer = FrameTimer::new();
        let exit_requested = false;
        let mut startup = StartupContext {
            scene: &mut scene,
            assets: &mut assets,
            gpu: &gpu,
            renderer: &mut renderer,
            overlay: &mut overlay,
            window: window.as_ref(),
        };
        let app = match A::init(&mut startup) {
            Ok(a) => a,
            Err(e) => {
                log::error!("failed to initialize application: {e}");
                event_loop.exit();
                return;
            }
        };
        let active_camera = scene.first_camera();
        self.window = Some(window);
        self.state = Some(RunnerState {
            app, scene, assets, gpu, renderer, overlay,
            overlay_visible: true,
            input, timer, active_camera, exit_requested,
        });
    }

    #[cfg(not(tarpaulin_include))]
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.as_ref() else { return; };
        let Some(state) = self.state.as_mut() else { return; };
        match &event {
            WindowEvent::CloseRequested => { event_loop.exit(); return; }
            WindowEvent::Resized(size) => {
                state.gpu.resize(*size);
                state.renderer.resize(&state.gpu);
                state.overlay.resize(size.width, size.height);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let winit::keyboard::PhysicalKey::Code(KeyCode::F3) = event.physical_key {
                    if event.state == winit::event::ElementState::Pressed {
                        state.overlay_visible = !state.overlay_visible;
                    }
                }
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
                        exit_requested: &mut state.exit_requested,
                    };
                    if let Err(e) = state.app.update(&mut update_ctx, dt) {
                        log::error!("application update failed: {e}");
                        event_loop.exit();
                        return;
                    }
                }
                if state.exit_requested {
                    event_loop.exit();
                    return;
                }
                {
                    let mut overlay_ctx = OverlayUpdateContext {
                        overlay: &mut state.overlay,
                        timer: &state.timer,
                    };
                    if let Err(e) = state.app.update_overlay(&mut overlay_ctx) {
                        log::error!("application update_overlay failed: {e}");
                        event_loop.exit();
                        return;
                    }
                }
                if let Err(e) = state.scene.update_all_world_transforms() {
                    log::error!("failed to update world transforms: {e}");
                    event_loop.exit();
                    return;
                }
                if let Err(e) = state.scene.update_all_world_bounds(&state.assets) {
                    log::error!("failed to update world bounds: {e}");
                    event_loop.exit();
                    return;
                }
                if let Some(mut frame) = state.gpu.begin_frame() {
                    let mut render_ctx = RenderContext {
                        scene: &state.scene,
                        assets: &state.assets,
                        gpu: &state.gpu,
                        frame: &mut frame,
                        renderer: &mut state.renderer,
                        active_camera: state.active_camera,
                    };
                    if let Err(e) = state.app.render(&mut render_ctx) {
                        log::error!("application render failed: {e}");
                        event_loop.exit();
                        return;
                    }
                    if state.overlay_visible {
                        if let Err(e) = state.overlay.render_pass(&state.gpu.device, &state.gpu.queue, &mut frame.encoder, &frame.view) {
                            log::error!("overlay render failed: {e}");
                            event_loop.exit();
                            return;
                        }
                    }
                    frame.present();
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
                    exit_requested: &mut state.exit_requested,
                };
                if let Err(e) = state.app.on_window_event(&mut update_ctx, &other) {
                    log::error!("application window event failed: {e}");
                    event_loop.exit();
                    return;
                }
                if state.exit_requested {
                    event_loop.exit();
                    return;
                }
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
