//! Startup, update, render, and overlay contexts passed to Application methods.

use anyhow::Result;
use rig_assets::AssetStore;
use rig_gpu::{Frame, GpuContext};
use rig_overlay::{ElementId, Overlay, Position};
use rig_render::Renderer;
use rig_scene::{NodeId, SceneGraph};
use winit::window::Window;

use crate::input::InputState;
use crate::timer::FrameTimer;

pub struct StartupContext<'a> {
    pub scene: &'a mut SceneGraph,
    pub assets: &'a mut AssetStore,
    pub gpu: &'a GpuContext,
    pub renderer: &'a mut Renderer,
    pub overlay: &'a mut Overlay,
    pub window: &'a Window,
}

pub struct UpdateContext<'a> {
    pub scene: &'a mut SceneGraph,
    pub assets: &'a AssetStore,
    pub input: &'a InputState,
    pub timer: &'a FrameTimer,
    pub active_camera: &'a mut Option<NodeId>,
    pub(crate) exit_requested: &'a mut bool,
}

impl UpdateContext<'_> {
    /// Request the runner to exit cleanly after the current frame.
    pub fn request_exit(&mut self) {
        *self.exit_requested = true;
    }
}

pub struct RenderContext<'a> {
    pub scene: &'a SceneGraph,
    pub assets: &'a AssetStore,
    pub gpu: &'a GpuContext,
    pub frame: &'a mut Frame,
    pub renderer: &'a mut Renderer,
    pub active_camera: Option<NodeId>,
}

/// Context passed to [`Application::update_overlay`].
pub struct OverlayUpdateContext<'a> {
    pub overlay: &'a mut Overlay,
    pub timer: &'a FrameTimer,
}

#[cfg(not(tarpaulin_include))]
impl OverlayUpdateContext<'_> {
    pub fn set_text(&mut self, id: ElementId, text: impl Into<String>) -> Result<()> {
        self.overlay.set_text(id, text).map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub fn set_position(&mut self, id: ElementId, position: Position) -> Result<()> {
        self.overlay.set_position(id, position).map_err(|e| anyhow::anyhow!("{e}"))
    }
}
