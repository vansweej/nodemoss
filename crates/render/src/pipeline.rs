//! Pipeline key types for render pipeline caching.

use rig_assets::{AlphaMode, ShaderHandle, VertexLayout};

/// Encodes the alpha blending mode for pipeline caching.
///
/// Mirrors [`rig_assets::AlphaMode`] but is `Eq + Hash` so it can be used as a
/// `HashMap` key. The `Mask` cutoff value is **not** part of the key — it is
/// uploaded per-draw via `MaterialUniforms.alpha_cutoff` and does not affect
/// pipeline state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PipelineAlphaMode {
    Opaque,
    Mask,
    Blend,
}

impl From<AlphaMode> for PipelineAlphaMode {
    fn from(mode: AlphaMode) -> Self {
        match mode {
            AlphaMode::Opaque => Self::Opaque,
            AlphaMode::Mask { .. } => Self::Mask,
            AlphaMode::Blend => Self::Blend,
        }
    }
}

/// Key used to look up a cached render pipeline.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PipelineKey {
    pub(crate) shader: ShaderHandle,
    pub(crate) vertex_layout: VertexLayout,
    pub(crate) color_format: wgpu::TextureFormat,
    pub(crate) depth_format: Option<wgpu::TextureFormat>,
    /// Polygon fill mode — `Fill` for solid, `Line` for wireframe.
    pub(crate) polygon_mode: wgpu::PolygonMode,
    /// Alpha blending mode — drives blend state and depth-write behaviour.
    pub(crate) alpha_mode: PipelineAlphaMode,
    /// When `true`, back-face culling is disabled (`cull_mode: None`).
    pub(crate) double_sided: bool,
}
