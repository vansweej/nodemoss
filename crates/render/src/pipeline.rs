//! Pipeline key types for render pipeline caching.

use rig_assets::{ShaderHandle, VertexLayout};

/// Key used to look up a cached render pipeline.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PipelineKey {
    pub(crate) shader: ShaderHandle,
    pub(crate) vertex_layout: VertexLayout,
    pub(crate) color_format: wgpu::TextureFormat,
    pub(crate) depth_format: Option<wgpu::TextureFormat>,
    /// Polygon fill mode — `Fill` for solid, `Line` for wireframe.
    pub(crate) polygon_mode: wgpu::PolygonMode,
}
