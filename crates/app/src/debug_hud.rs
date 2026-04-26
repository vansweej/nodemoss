//! Opt-in debug HUD for rig-app examples.
//!
//! [`DebugHud`] manages two auto-layout stacks of [`TextElement`]s:
//! - **Left** (anchored `TopLeft`): GPU adapter name + any example-added elements.
//! - **Right** (anchored `TopRight`): FPS counter + any example-added elements.
//!
//! # Usage
//!
//! ```ignore
//! // in Application::init
//! let mut debug_hud = DebugHud::new(ctx.overlay, ctx.gpu);
//! let cam_pos_id = debug_hud.add_element(ctx.overlay, Side::Right, "Cam: (0, 0, 0)");
//!
//! // in Application::update_overlay
//! self.debug_hud.update(ctx)?;
//! ctx.set_text(self.cam_pos_id, format!("Cam: ({:.1}, …)", …))?;
//! ```

use anyhow::Result;
use rig_gpu::GpuContext;
use rig_overlay::{Anchor, ElementId, Overlay, Position, TextElement};

use crate::context::OverlayUpdateContext;

// ── layout constants ──────────────────────────────────────────────────────────

/// Pixel margin from the screen edge for both stacks.
const MARGIN: f32 = 8.0;
/// Extra vertical gap between stacked elements (on top of the line height).
const LINE_SPACING: f32 = 4.0;
/// Font size for built-in elements (GPU name, FPS).
const BUILTIN_FONT_SIZE: f32 = 16.0;
/// Font size for example-added elements.
const CUSTOM_FONT_SIZE: f32 = 14.0;
/// Colour for built-in elements — full white.
const BUILTIN_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Colour for custom elements — slightly dimmed.
const CUSTOM_COLOR: [f32; 4] = [0.8, 0.8, 0.8, 1.0];

// ── Side ─────────────────────────────────────────────────────────────────────

/// Which side of the screen to append an element to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Left column, anchored to the top-left corner.
    Left,
    /// Right column, anchored to the top-right corner.
    Right,
}

// ── internal helpers ──────────────────────────────────────────────────────────

/// Compute the Y offset for the next element in a stack.
///
/// `current_y` is the top-left Y of the *current* element.
/// Returns the Y for the element that should appear directly below it.
pub(crate) fn next_y(current_y: f32, font_size: f32) -> f32 {
    current_y + font_size * 1.2 + LINE_SPACING
}

// ── DebugHud ──────────────────────────────────────────────────────────────────

/// Opt-in debug HUD with dual auto-layout stacks.
///
/// Construct once in [`Application::init`] and call [`DebugHud::update`] every
/// frame from [`Application::update_overlay`].
pub struct DebugHud {
    /// Built-in right-stack element: FPS counter.
    fps_id: ElementId,
    /// Next available Y offset for the left stack.
    left_next_y: f32,
    /// Next available Y offset for the right stack.
    right_next_y: f32,
}

#[cfg(not(tarpaulin_include))]
impl DebugHud {
    /// Create a new `DebugHud`, registering the GPU name (left) and FPS (right)
    /// built-in elements on `overlay`.
    pub fn new(overlay: &mut Overlay, gpu: &GpuContext) -> Self {
        // Left stack — GPU adapter name (static, never updated).
        let gpu_label = format!(
            "GPU: {} ({:?})",
            gpu.adapter_info.name, gpu.adapter_info.backend
        );
        overlay.add_text(TextElement {
            text: gpu_label,
            position: Position::Anchor {
                anchor: Anchor::TopLeft,
                offset: [MARGIN, MARGIN],
            },
            color: BUILTIN_COLOR,
            font_size: BUILTIN_FONT_SIZE,
        });

        // Right stack — FPS counter (updated every frame).
        let fps_id = overlay.add_text(TextElement {
            text: "FPS: 0".into(),
            position: Position::Anchor {
                anchor: Anchor::TopRight,
                offset: [MARGIN, MARGIN],
            },
            color: BUILTIN_COLOR,
            font_size: BUILTIN_FONT_SIZE,
        });

        Self {
            fps_id,
            left_next_y: next_y(MARGIN, BUILTIN_FONT_SIZE),
            right_next_y: next_y(MARGIN, BUILTIN_FONT_SIZE),
        }
    }

    /// Append a custom element to the given `side` stack.
    ///
    /// Returns an [`ElementId`] that the caller can use with
    /// [`OverlayUpdateContext::set_text`] to update the text each frame.
    pub fn add_element(
        &mut self,
        overlay: &mut Overlay,
        side: Side,
        text: impl Into<String>,
    ) -> ElementId {
        let (anchor, y) = match side {
            Side::Left => (Anchor::TopLeft, self.left_next_y),
            Side::Right => (Anchor::TopRight, self.right_next_y),
        };

        let id = overlay.add_text(TextElement {
            text: text.into(),
            position: Position::Anchor {
                anchor,
                offset: [MARGIN, y],
            },
            color: CUSTOM_COLOR,
            font_size: CUSTOM_FONT_SIZE,
        });

        match side {
            Side::Left => self.left_next_y = next_y(y, CUSTOM_FONT_SIZE),
            Side::Right => self.right_next_y = next_y(y, CUSTOM_FONT_SIZE),
        }

        id
    }

    /// Update the dynamic built-in elements (FPS).
    ///
    /// Call this once per frame from [`Application::update_overlay`].
    pub fn update(&self, ctx: &mut OverlayUpdateContext<'_>) -> Result<()> {
        ctx.set_text(self.fps_id, format!("FPS: {:.0}", ctx.timer.fps()))
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Side ──────────────────────────────────────────────────────────────────

    #[test]
    fn side_clone_and_copy() {
        let s = Side::Left;
        let s2 = s; // Copy
        let s3 = s.clone(); // Clone
        assert_eq!(s2, s3);
    }

    #[test]
    fn side_debug_is_non_empty() {
        assert!(!format!("{:?}", Side::Left).is_empty());
        assert!(!format!("{:?}", Side::Right).is_empty());
    }

    #[test]
    fn side_equality() {
        assert_eq!(Side::Left, Side::Left);
        assert_eq!(Side::Right, Side::Right);
        assert_ne!(Side::Left, Side::Right);
    }

    // ── next_y ────────────────────────────────────────────────────────────────

    #[test]
    fn next_y_advances_by_line_height_plus_spacing() {
        // font_size=16, line_height=16*1.2=19.2, spacing=4 → delta=23.2
        let result = next_y(8.0, 16.0);
        let expected = 8.0 + 16.0 * 1.2 + LINE_SPACING;
        assert!(
            (result - expected).abs() < 1e-5,
            "got {result}, expected {expected}"
        );
    }

    #[test]
    fn next_y_from_zero() {
        let result = next_y(0.0, 14.0);
        let expected = 14.0 * 1.2 + LINE_SPACING;
        assert!((result - expected).abs() < 1e-5);
    }

    #[test]
    fn next_y_is_monotonically_increasing() {
        let y0 = MARGIN;
        let y1 = next_y(y0, BUILTIN_FONT_SIZE);
        let y2 = next_y(y1, CUSTOM_FONT_SIZE);
        assert!(y1 > y0);
        assert!(y2 > y1);
    }

    // ── constants ─────────────────────────────────────────────────────────────

    #[test]
    fn constants_are_positive() {
        assert!(MARGIN > 0.0);
        assert!(LINE_SPACING >= 0.0);
        assert!(BUILTIN_FONT_SIZE > 0.0);
        assert!(CUSTOM_FONT_SIZE > 0.0);
    }

    #[test]
    fn builtin_color_is_opaque_white() {
        assert_eq!(BUILTIN_COLOR, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn custom_color_is_dimmer_than_builtin() {
        // All channels except alpha should be ≤ builtin
        for i in 0..3 {
            assert!(CUSTOM_COLOR[i] <= BUILTIN_COLOR[i]);
        }
    }
}
