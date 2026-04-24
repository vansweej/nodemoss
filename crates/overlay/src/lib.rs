//! 2D text overlay for the rig framework.
//!
//! [`Overlay`] manages a set of retained [`TextElement`]s that are rendered
//! on top of the 3D scene each frame. Elements are registered once (typically
//! in `Application::init`) and updated per frame via [`Overlay::set_text`].
//!
//! The overlay render pass uses `LoadOp::Load` so it composites on top of
//! whatever was already rendered into the frame's colour attachment.

use thiserror::Error;

pub use glyphon;

// ── errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum OverlayError {
    #[error("invalid element id")]
    InvalidElementId,
    #[error("glyphon prepare error: {0}")]
    Prepare(String),
    #[error("glyphon render error: {0}")]
    Render(String),
}

pub type Result<T> = std::result::Result<T, OverlayError>;

// ── positioning ───────────────────────────────────────────────────────────────

/// Screen anchor point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    LeftCenter,
    Center,
    RightCenter,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// Position of a text element on screen.
#[derive(Clone, Debug)]
pub enum Position {
    /// Offset in pixels from an anchor corner.
    ///
    /// `offset[0]` is measured rightward from the left anchors and leftward
    /// from the right anchors. `offset[1]` is measured downward from the top
    /// anchors and upward from the bottom anchors.
    Anchor {
        anchor: Anchor,
        /// Pixel offset from the anchor corner (x right/left, y down/up).
        offset: [f32; 2],
    },
    /// Absolute pixel position from the top-left corner of the viewport.
    Absolute { x: f32, y: f32 },
}

// ── element ───────────────────────────────────────────────────────────────────

/// A retained text element registered with the [`Overlay`].
#[derive(Clone, Debug)]
pub struct TextElement {
    /// The text to display.
    pub text: String,
    /// Where to place the element on screen.
    pub position: Position,
    /// RGBA colour in linear [0, 1] range.
    pub color: [f32; 4],
    /// Font size in points.
    pub font_size: f32,
}

/// Opaque handle to a registered [`TextElement`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ElementId(usize);

// ── element registry (testable without GPU) ───────────────────────────────────

/// Element registry and positioning logic — no GPU state.
///
/// Extracted so that unit tests can exercise element management and position
/// resolution without needing a wgpu device.
pub(crate) struct ElementRegistry {
    pub(crate) elements: Vec<TextElement>,
    pub(crate) surface_width: u32,
    pub(crate) surface_height: u32,
}

impl ElementRegistry {
    pub(crate) fn new(surface_width: u32, surface_height: u32) -> Self {
        Self {
            elements: Vec::new(),
            surface_width,
            surface_height,
        }
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.surface_width = width;
        self.surface_height = height;
    }

    pub(crate) fn add_text(&mut self, element: TextElement) -> ElementId {
        let id = ElementId(self.elements.len());
        self.elements.push(element);
        id
    }

    pub(crate) fn set_text(&mut self, id: ElementId, text: impl Into<String>) -> Result<()> {
        self.elements
            .get_mut(id.0)
            .ok_or(OverlayError::InvalidElementId)?
            .text = text.into();
        Ok(())
    }

    pub(crate) fn set_position(&mut self, id: ElementId, position: Position) -> Result<()> {
        self.elements
            .get_mut(id.0)
            .ok_or(OverlayError::InvalidElementId)?
            .position = position;
        Ok(())
    }

    /// Resolve a [`Position`] to absolute pixel coordinates `(left, top)`.
    pub(crate) fn resolve_position(
        &self,
        position: &Position,
        text_width: f32,
        text_height: f32,
    ) -> (f32, f32) {
        match position {
            Position::Absolute { x, y } => (*x, *y),
            Position::Anchor { anchor, offset } => {
                let w = self.surface_width as f32;
                let h = self.surface_height as f32;
                let cx = (w - text_width) / 2.0;
                let cy = (h - text_height) / 2.0;
                match anchor {
                    Anchor::TopLeft => (offset[0], offset[1]),
                    Anchor::TopCenter => (cx + offset[0], offset[1]),
                    Anchor::TopRight => (w - text_width - offset[0], offset[1]),
                    Anchor::LeftCenter => (offset[0], cy + offset[1]),
                    Anchor::Center => (cx + offset[0], cy + offset[1]),
                    Anchor::RightCenter => (w - text_width - offset[0], cy + offset[1]),
                    Anchor::BottomLeft => (offset[0], h - text_height - offset[1]),
                    Anchor::BottomCenter => (cx + offset[0], h - text_height - offset[1]),
                    Anchor::BottomRight => {
                        (w - text_width - offset[0], h - text_height - offset[1])
                    }
                }
            }
        }
    }
}

// ── Overlay ───────────────────────────────────────────────────────────────────

/// 2D text overlay renderer.
///
/// Owns a glyphon [`TextRenderer`] and a registry of [`TextElement`]s.
/// Call [`Overlay::render_pass`] once per frame after the scene pass to
/// composite text on top of the rendered image.
pub struct Overlay {
    font_system: glyphon::FontSystem,
    swash_cache: glyphon::SwashCache,
    atlas: glyphon::TextAtlas,
    text_renderer: glyphon::TextRenderer,
    viewport: glyphon::Viewport,
    registry: ElementRegistry,
}

#[cfg(not(tarpaulin_include))]
impl Overlay {
    /// Create a new overlay for the given GPU context and surface format.
    #[cfg(not(tarpaulin_include))]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        surface_width: u32,
        surface_height: u32,
    ) -> Self {
        let font_system = glyphon::FontSystem::new();
        let swash_cache = glyphon::SwashCache::new();
        let cache = glyphon::Cache::new(device);
        let mut atlas = glyphon::TextAtlas::new(device, queue, &cache, surface_format);
        let text_renderer =
            glyphon::TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let viewport = glyphon::Viewport::new(device, &cache);

        Self {
            font_system,
            swash_cache,
            atlas,
            text_renderer,
            viewport,
            registry: ElementRegistry::new(surface_width, surface_height),
        }
    }

    /// Update the surface dimensions after a window resize.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.registry.resize(width, height);
    }

    /// Register a new text element and return its [`ElementId`].
    pub fn add_text(&mut self, element: TextElement) -> ElementId {
        self.registry.add_text(element)
    }

    /// Update the text of an existing element.
    pub fn set_text(&mut self, id: ElementId, text: impl Into<String>) -> Result<()> {
        self.registry.set_text(id, text)
    }

    /// Update the position of an existing element.
    pub fn set_position(&mut self, id: ElementId, position: Position) -> Result<()> {
        self.registry.set_position(id, position)
    }

    /// Record the overlay render pass into `encoder`, rendering to `color_view`.
    ///
    /// Uses `LoadOp::Load` so the scene content is preserved beneath the text.
    #[cfg(not(tarpaulin_include))]
    pub fn render_pass(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        color_view: &wgpu::TextureView,
    ) -> Result<()> {
        if self.registry.elements.is_empty() {
            return Ok(());
        }

        self.viewport.update(
            queue,
            glyphon::Resolution {
                width: self.registry.surface_width,
                height: self.registry.surface_height,
            },
        );

        // Build text areas from elements.
        let mut buffers: Vec<glyphon::Buffer> = Vec::with_capacity(self.registry.elements.len());
        for element in &self.registry.elements {
            let metrics = glyphon::Metrics::new(element.font_size, element.font_size * 1.2);
            let mut buffer = glyphon::Buffer::new(&mut self.font_system, metrics);
            buffer.set_size(
                &mut self.font_system,
                Some(self.registry.surface_width as f32),
                Some(self.registry.surface_height as f32),
            );
            buffer.set_text(
                &mut self.font_system,
                &element.text,
                &glyphon::Attrs::new().family(glyphon::Family::Monospace),
                glyphon::Shaping::Basic,
                None,
            );
            buffer.shape_until_scroll(&mut self.font_system, false);
            buffers.push(buffer);
        }

        let text_areas: Vec<glyphon::TextArea<'_>> = buffers
            .iter()
            .zip(self.registry.elements.iter())
            .map(|(buffer, element)| {
                // Use shaped glyph metrics for accurate text width measurement.
                let text_width = measure_buffer_width(buffer);
                let text_height = element.font_size * 1.2;
                let (left, top) =
                    self.registry
                        .resolve_position(&element.position, text_width, text_height);
                let [r, g, b, a] = element.color;
                glyphon::TextArea {
                    buffer,
                    left,
                    top,
                    scale: 1.0,
                    bounds: glyphon::TextBounds {
                        left: 0,
                        top: 0,
                        right: self.registry.surface_width as i32,
                        bottom: self.registry.surface_height as i32,
                    },
                    default_color: glyphon::Color::rgba(
                        (r * 255.0) as u8,
                        (g * 255.0) as u8,
                        (b * 255.0) as u8,
                        (a * 255.0) as u8,
                    ),
                    custom_glyphs: &[],
                }
            })
            .collect();

        self.text_renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                text_areas,
                &mut self.swash_cache,
            )
            .map_err(|e| OverlayError::Prepare(e.to_string()))?;

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rig overlay pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .map_err(|e| OverlayError::Render(e.to_string()))?;
        }

        self.atlas.trim();
        Ok(())
    }
}

/// Measure the pixel width of a shaped glyphon [`Buffer`] by taking the
/// maximum `line_w` across all layout runs.
///
/// Returns `0.0` for an empty buffer.
#[cfg(not(tarpaulin_include))]
fn measure_buffer_width(buffer: &glyphon::Buffer) -> f32 {
    buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0_f32, f32::max)
}

// ── tests ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_error_display_is_non_empty() {
        let err = OverlayError::InvalidElementId;
        assert!(!err.to_string().is_empty());

        let err = OverlayError::Prepare("test".into());
        assert!(err.to_string().contains("test"));

        let err = OverlayError::Render("oops".into());
        assert!(err.to_string().contains("oops"));
    }

    #[test]
    fn add_text_returns_sequential_ids() {
        let mut reg = ElementRegistry::new(800, 600);

        let id0 = reg.add_text(sample_element("hello"));
        let id1 = reg.add_text(sample_element("world"));

        assert_eq!(id0, ElementId(0));
        assert_eq!(id1, ElementId(1));
    }

    #[test]
    fn set_text_updates_element() {
        let mut reg = ElementRegistry::new(800, 600);
        let id = reg.add_text(sample_element("initial"));

        reg.set_text(id, "updated").unwrap();

        assert_eq!(reg.elements[0].text, "updated");
    }

    #[test]
    fn set_text_returns_error_for_invalid_id() {
        let mut reg = ElementRegistry::new(800, 600);

        let result = reg.set_text(ElementId(99), "x");

        assert!(matches!(result, Err(OverlayError::InvalidElementId)));
    }

    #[test]
    fn set_position_updates_element() {
        let mut reg = ElementRegistry::new(800, 600);
        let id = reg.add_text(sample_element("hi"));

        reg.set_position(id, Position::Absolute { x: 10.0, y: 20.0 })
            .unwrap();

        assert!(matches!(
            reg.elements[0].position,
            Position::Absolute { x, y } if x == 10.0 && y == 20.0
        ));
    }

    #[test]
    fn set_position_returns_error_for_invalid_id() {
        let mut reg = ElementRegistry::new(800, 600);

        let result = reg.set_position(ElementId(99), Position::Absolute { x: 0.0, y: 0.0 });

        assert!(matches!(result, Err(OverlayError::InvalidElementId)));
    }

    #[test]
    fn resize_updates_dimensions() {
        let mut reg = ElementRegistry::new(800, 600);

        reg.resize(1920, 1080);

        assert_eq!(reg.surface_width, 1920);
        assert_eq!(reg.surface_height, 1080);
    }

    #[test]
    fn resolve_position_absolute() {
        let reg = ElementRegistry::new(800, 600);

        let (x, y) = reg.resolve_position(&Position::Absolute { x: 42.0, y: 17.0 }, 0.0, 0.0);

        assert_eq!(x, 42.0);
        assert_eq!(y, 17.0);
    }

    #[test]
    fn resolve_position_top_left() {
        let reg = ElementRegistry::new(800, 600);

        let (x, y) = reg.resolve_position(
            &Position::Anchor {
                anchor: Anchor::TopLeft,
                offset: [10.0, 5.0],
            },
            0.0,
            0.0,
        );

        assert_eq!(x, 10.0);
        assert_eq!(y, 5.0);
    }

    #[test]
    fn resolve_position_top_right() {
        let reg = ElementRegistry::new(800, 600);
        let text_width = 100.0;

        let (x, _y) = reg.resolve_position(
            &Position::Anchor {
                anchor: Anchor::TopRight,
                offset: [8.0, 8.0],
            },
            text_width,
            0.0,
        );

        // 800 - 100 - 8 = 692
        assert_eq!(x, 692.0);
    }

    #[test]
    fn resolve_position_bottom_left() {
        let reg = ElementRegistry::new(800, 600);
        let text_height = 20.0;

        let (_x, y) = reg.resolve_position(
            &Position::Anchor {
                anchor: Anchor::BottomLeft,
                offset: [0.0, 10.0],
            },
            0.0,
            text_height,
        );

        // 600 - 20 - 10 = 570
        assert_eq!(y, 570.0);
    }

    #[test]
    fn resolve_position_bottom_right() {
        let reg = ElementRegistry::new(800, 600);
        let text_width = 80.0;
        let text_height = 20.0;

        let (x, y) = reg.resolve_position(
            &Position::Anchor {
                anchor: Anchor::BottomRight,
                offset: [8.0, 8.0],
            },
            text_width,
            text_height,
        );

        // x: 800 - 80 - 8 = 712, y: 600 - 20 - 8 = 572
        assert_eq!(x, 712.0);
        assert_eq!(y, 572.0);
    }

    #[test]
    fn resolve_position_top_center() {
        let reg = ElementRegistry::new(800, 600);
        let text_width = 100.0;

        let (x, y) = reg.resolve_position(
            &Position::Anchor {
                anchor: Anchor::TopCenter,
                offset: [0.0, 8.0],
            },
            text_width,
            0.0,
        );

        // x: (800 - 100) / 2 = 350, y: 8
        assert_eq!(x, 350.0);
        assert_eq!(y, 8.0);
    }

    #[test]
    fn resolve_position_bottom_center() {
        let reg = ElementRegistry::new(800, 600);
        let text_width = 100.0;
        let text_height = 20.0;

        let (x, y) = reg.resolve_position(
            &Position::Anchor {
                anchor: Anchor::BottomCenter,
                offset: [0.0, 8.0],
            },
            text_width,
            text_height,
        );

        // x: (800 - 100) / 2 = 350, y: 600 - 20 - 8 = 572
        assert_eq!(x, 350.0);
        assert_eq!(y, 572.0);
    }

    #[test]
    fn resolve_position_left_center() {
        let reg = ElementRegistry::new(800, 600);
        let text_height = 20.0;

        let (x, y) = reg.resolve_position(
            &Position::Anchor {
                anchor: Anchor::LeftCenter,
                offset: [8.0, 0.0],
            },
            0.0,
            text_height,
        );

        // x: 8, y: (600 - 20) / 2 = 290
        assert_eq!(x, 8.0);
        assert_eq!(y, 290.0);
    }

    #[test]
    fn resolve_position_right_center() {
        let reg = ElementRegistry::new(800, 600);
        let text_width = 100.0;
        let text_height = 20.0;

        let (x, y) = reg.resolve_position(
            &Position::Anchor {
                anchor: Anchor::RightCenter,
                offset: [8.0, 0.0],
            },
            text_width,
            text_height,
        );

        // x: 800 - 100 - 8 = 692, y: (600 - 20) / 2 = 290
        assert_eq!(x, 692.0);
        assert_eq!(y, 290.0);
    }

    #[test]
    fn resolve_position_center() {
        let reg = ElementRegistry::new(800, 600);
        let text_width = 100.0;
        let text_height = 20.0;

        let (x, y) = reg.resolve_position(
            &Position::Anchor {
                anchor: Anchor::Center,
                offset: [0.0, 0.0],
            },
            text_width,
            text_height,
        );

        // x: (800 - 100) / 2 = 350, y: (600 - 20) / 2 = 290
        assert_eq!(x, 350.0);
        assert_eq!(y, 290.0);
    }

    #[test]
    fn element_id_equality() {
        assert_eq!(ElementId(0), ElementId(0));
        assert_ne!(ElementId(0), ElementId(1));
    }

    // ── helpers ────────────────────────────────────────────────────────────────

    fn sample_element(text: &str) -> TextElement {
        TextElement {
            text: text.into(),
            position: Position::Absolute { x: 0.0, y: 0.0 },
            color: [1.0, 1.0, 1.0, 1.0],
            font_size: 16.0,
        }
    }
}
