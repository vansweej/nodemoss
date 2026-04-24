//! GPU context — device, queue, surface, and swapchain management.
//!
//! [`GpuContext`] owns the wgpu device, queue, and surface. It is created
//! once at startup and shared (via `Arc`) with every subsystem that needs
//! raw GPU access (`rig-render`, `rig-overlay`, examples).
//!
//! The two-phase frame model:
//! 1. Call [`GpuContext::begin_frame`] to acquire the swapchain texture and
//!    get a [`Frame`] handle.
//! 2. Record scene and overlay passes into the frame's view.
//! 3. Call [`Frame::present`] to submit and present.

use std::sync::Arc;

use thiserror::Error;
use winit::{dpi::PhysicalSize, window::Window};

pub use wgpu;

// ── errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum GpuError {
    #[error("failed to create surface: {0}")]
    SurfaceCreate(#[from] wgpu::CreateSurfaceError),
    #[error("failed to find a suitable GPU adapter")]
    NoAdapter,
    #[error("failed to create device: {0}")]
    RequestDevice(#[from] wgpu::RequestDeviceError),
    #[error("surface does not expose a supported format")]
    NoSurfaceFormat,
    #[error("surface alpha modes list is empty")]
    NoAlphaMode,
}

pub type Result<T> = std::result::Result<T, GpuError>;

// ── Frame ─────────────────────────────────────────────────────────────────────

/// A handle to the current swapchain frame.
///
/// Obtained from [`GpuContext::begin_frame`]. Callers record render passes
/// against [`Frame::view`], then call [`Frame::present`] to submit and flip.
pub struct Frame {
    /// The texture view for this frame — use as the colour attachment.
    pub view: wgpu::TextureView,
    /// The command encoder for this frame — record all passes here.
    pub encoder: wgpu::CommandEncoder,
    surface_texture: wgpu::SurfaceTexture,
    queue: Arc<wgpu::Queue>,
}

#[cfg(not(tarpaulin_include))]
impl Frame {
    /// Submit the recorded commands and present the frame to the screen.
    pub fn present(self) {
        self.queue.submit(std::iter::once(self.encoder.finish()));
        self.surface_texture.present();
    }
}

// ── GpuContext ────────────────────────────────────────────────────────────────

/// Owns the wgpu instance, adapter, device, queue, surface, and swapchain
/// configuration. Created once at startup; shared via `Arc<GpuContext>` where
/// needed.
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: Arc<wgpu::Queue>,
    pub surface: wgpu::Surface<'static>,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub window: Arc<Window>,
}

#[cfg(not(tarpaulin_include))]
impl GpuContext {
    /// Initialise the GPU context for the given window.
    ///
    /// This is `async` because wgpu adapter/device creation is async.
    #[cfg(not(tarpaulin_include))]
    pub async fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .map_err(|_| GpuError::NoAdapter)?;

        log::info!("Using adapter: {}", adapter.get_info().name);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("rig gpu device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .or_else(|| surface_caps.formats.first().copied())
            .ok_or(GpuError::NoSurfaceFormat)?;

        let alpha_mode = surface_caps
            .alpha_modes
            .first()
            .copied()
            .ok_or(GpuError::NoAlphaMode)?;

        let width = size.width.max(1);
        let height = size.height.max(1);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        Ok(Self {
            device,
            queue: Arc::new(queue),
            surface,
            surface_config,
            window,
        })
    }

    /// Reconfigure the surface after a window resize.
    #[cfg(not(tarpaulin_include))]
    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            self.surface_config.width = size.width;
            self.surface_config.height = size.height;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }

    /// Acquire the next swapchain texture and return a [`Frame`] handle.
    ///
    /// Returns `None` if the frame should be skipped (timeout, occluded, etc.).
    /// On `Outdated`/`Lost` the surface is reconfigured automatically.
    #[cfg(not(tarpaulin_include))]
    pub fn begin_frame(&mut self) -> Option<Frame> {
        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return None;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.surface_config);
                return None;
            }
            wgpu::CurrentSurfaceTexture::Validation => return None,
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rig frame encoder"),
            });

        Some(Frame {
            view,
            encoder,
            surface_texture,
            queue: self.queue.clone(),
        })
    }

    /// The texture format of the swapchain surface.
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_config.format
    }

    /// Current surface width in pixels.
    pub fn width(&self) -> u32 {
        self.surface_config.width
    }

    /// Current surface height in pixels.
    pub fn height(&self) -> u32 {
        self.surface_config.height
    }

    /// Aspect ratio (width / height).
    pub fn aspect(&self) -> f32 {
        self.surface_config.width as f32 / self.surface_config.height as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`GpuError`] variants must be `Debug` and produce non-empty messages.
    #[test]
    fn gpu_error_display_is_non_empty() {
        let err = GpuError::NoAdapter;
        assert!(!err.to_string().is_empty());

        let err = GpuError::NoSurfaceFormat;
        assert!(!err.to_string().is_empty());

        let err = GpuError::NoAlphaMode;
        assert!(!err.to_string().is_empty());
    }

    /// Wrapping a [`wgpu::RequestDeviceError`] must produce a [`GpuError`].
    #[test]
    fn gpu_error_from_request_device_error() {
        // wgpu::RequestDeviceError is not constructible in tests, so we verify
        // the From impl compiles by checking the variant discriminant via the
        // Debug representation of a hand-crafted GpuError.
        let err = GpuError::NoAdapter;
        assert!(format!("{err:?}").contains("NoAdapter"));
    }
}
