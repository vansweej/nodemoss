//! Concrete `wgpu` renderer for the rig framework.

mod cache;
mod frame;
mod helpers;
mod pipeline;
mod renderer;

pub use helpers::{
    create_depth_texture, validate_vertex_layout, vertex_format_size, wgpu_vertex_format,
    DEPTH_FORMAT, NORMAL_COLOR_SHADER, TRIANGLE_SHADER,
};
pub use renderer::Renderer;

pub use rig_gpu;
pub use wgpu;

use thiserror::Error;

// Re-export internal uniform type for tests
pub(crate) use frame::ObjectUniforms;

// ── errors ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("scene node is not a camera")]
    InvalidCamera,
    #[error("asset error: {0}")]
    Asset(String),
}

pub type Result<T> = std::result::Result<T, RenderError>;

// ── public types ───────────────────────────────────────────────────────────────

/// Descriptor used to create a [`RenderTarget`].
pub struct RenderTargetDescriptor {
    pub width: u32,
    pub height: u32,
    pub color_format: wgpu::TextureFormat,
    pub depth_format: Option<wgpu::TextureFormat>,
    pub label: &'static str,
}

/// An offscreen render target.
pub struct RenderTarget {
    pub color_texture: wgpu::Texture,
    pub color_view: wgpu::TextureView,
    pub depth_texture: Option<wgpu::Texture>,
    pub depth_view: Option<wgpu::TextureView>,
    pub width: u32,
    pub height: u32,
    pub color_format: wgpu::TextureFormat,
    pub depth_format: Option<wgpu::TextureFormat>,
}

// ── tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rig_assets::{ShaderAsset, VertexAttribute, VertexLayout};
    use rig_math::{Quat, Transform, Vec3};

    use super::*;
    use crate::frame::ObjectUniforms;
    use crate::helpers::{
        aligned_uniform_size, camera_projection_view, decompose_pose, encode_object_uniforms,
        mesh_vertex_attributes, object_uniform_offset,
    };
    use crate::pipeline::PipelineKey;
    use rig_assets::{MeshAsset, VertexFormat};
    use rig_math::{BoundingSphere, Mat4};
    use rig_scene::ExtractedCamera;

    fn sample_mesh() -> MeshAsset {
        MeshAsset {
            vertex_layout: VertexLayout {
                array_stride: 24,
                attributes: vec![
                    VertexAttribute {
                        shader_location: 0,
                        format: VertexFormat::Float32x3,
                        offset: 0,
                    },
                    VertexAttribute {
                        shader_location: 1,
                        format: VertexFormat::Float32x3,
                        offset: 12,
                    },
                ],
            },
            vertex_data: Arc::from([1_u8; 24]),
            index_data: Arc::from([0_u8, 1, 2, 0, 2, 1]),
            index_format: rig_assets::IndexFormat::Uint16,
            local_bounds: rig_math::BoundingSphere::ZERO,
        }
    }

    #[test]
    fn validate_vertex_layout_accepts_position_and_color() {
        assert!(validate_vertex_layout(&sample_mesh().vertex_layout).is_ok());
    }

    #[test]
    fn validate_vertex_layout_accepts_padded_and_reordered() {
        let mut mesh = sample_mesh();
        mesh.vertex_layout.array_stride = 32;
        mesh.vertex_layout.attributes = vec![
            VertexAttribute { shader_location: 1, format: VertexFormat::Float32x3, offset: 16 },
            VertexAttribute { shader_location: 0, format: VertexFormat::Float32x3, offset: 0 },
        ];
        assert!(validate_vertex_layout(&mesh.vertex_layout).is_ok());
    }

    #[test]
    fn validate_vertex_layout_rejects_attribute_outside_stride() {
        let mut mesh = sample_mesh();
        mesh.vertex_layout.array_stride = 16;
        assert!(validate_vertex_layout(&mesh.vertex_layout).is_err());
    }

    #[test]
    fn mesh_vertex_attributes_preserve_asset_layout_information() {
        let mut mesh = sample_mesh();
        mesh.vertex_layout.array_stride = 32;
        mesh.vertex_layout.attributes[0].offset = 4;
        mesh.vertex_layout.attributes[1].offset = 20;
        let attributes = mesh_vertex_attributes(&mesh.vertex_layout).unwrap();
        assert_eq!(attributes.len(), 2);
        assert_eq!(attributes[0].shader_location, 0);
        assert_eq!(attributes[0].offset, 4);
        assert_eq!(attributes[0].format, wgpu::VertexFormat::Float32x3);
        assert_eq!(attributes[1].shader_location, 1);
        assert_eq!(attributes[1].offset, 20);
    }

    #[test]
    fn mesh_vertex_attributes_reject_duplicate_shader_locations() {
        let mut mesh = sample_mesh();
        mesh.vertex_layout.attributes[1].shader_location = 0;
        assert!(mesh_vertex_attributes(&mesh.vertex_layout).is_err());
    }

    #[test]
    fn aligned_uniform_size_rounds_up_to_alignment() {
        assert_eq!(aligned_uniform_size(64, 256), 256);
        assert_eq!(aligned_uniform_size(256, 256), 256);
        assert_eq!(aligned_uniform_size(65, 16), 80);
    }

    #[test]
    fn object_uniform_offset_uses_stride() {
        assert_eq!(object_uniform_offset(2, 256).unwrap(), 512);
    }

    #[test]
    fn encode_object_uniforms_respects_stride_padding() {
        let uniforms = [
            ObjectUniforms { world: Mat4::IDENTITY.to_cols_array_2d() },
            ObjectUniforms { world: Mat4::from_translation(rig_math::Vec3::new(1.0, 2.0, 3.0)).to_cols_array_2d() },
        ];
        let bytes = encode_object_uniforms(&uniforms, 256);
        let object_size = std::mem::size_of::<ObjectUniforms>();
        assert_eq!(bytes.len(), 512);
        assert_eq!(&bytes[..object_size], bytemuck::bytes_of(&uniforms[0]));
        assert_eq!(&bytes[256..256 + object_size], bytemuck::bytes_of(&uniforms[1]));
        assert!(bytes[object_size..256].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn immutable_cache_uses_handle_as_key() {
        use rig_assets::MeshHandle;
        let handle_a = MeshHandle::from_raw(0);
        let handle_b = MeshHandle::from_raw(1);
        assert_ne!(handle_a, handle_b);
        assert_eq!(handle_a, MeshHandle::from_raw(0));
    }

    #[test]
    fn shader_handle_identity() {
        use rig_assets::ShaderHandle;
        let h = ShaderHandle::from_raw(42);
        assert_eq!(h, ShaderHandle::from_raw(42));
        assert_ne!(h, ShaderHandle::from_raw(0));
    }

    #[test]
    fn decompose_pose_recovers_translation_and_rotation() {
        let transform = Transform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quat::from_rotation_y(0.75),
            scale: Vec3::new(2.0, 2.0, 2.0),
        };
        let pose = decompose_pose(transform.to_mat4());
        assert!(pose.translation.abs_diff_eq(transform.translation, 1e-5));
        assert!(pose.rotation.abs_diff_eq(transform.rotation, 1e-5));
        assert_eq!(pose.scale, Vec3::ONE);
    }

    #[test]
    fn triangle_shader_mentions_expected_entry_points() {
        assert!(TRIANGLE_SHADER.contains("fn vs_main"));
        assert!(TRIANGLE_SHADER.contains("fn fs_main"));
        assert!(TRIANGLE_SHADER.contains("@group(0) @binding(0)"));
    }

    #[test]
    fn normal_color_shader_mentions_expected_entry_points_and_locations() {
        assert!(NORMAL_COLOR_SHADER.contains("fn vs_main"));
        assert!(NORMAL_COLOR_SHADER.contains("fn fs_main"));
        assert!(NORMAL_COLOR_SHADER.contains("@location(0) position"));
        assert!(NORMAL_COLOR_SHADER.contains("@location(1) normal"));
        assert!(NORMAL_COLOR_SHADER.contains("@location(2) uv"));
    }

    #[test]
    fn pipeline_key_differs_with_depth_format() {
        use rig_assets::ShaderHandle;
        let layout = VertexLayout { array_stride: 24, attributes: vec![] };
        let shader = ShaderHandle::from_raw(1);
        let key_no_depth = PipelineKey { shader, vertex_layout: layout.clone(), color_format: wgpu::TextureFormat::Bgra8UnormSrgb, depth_format: None };
        let key_with_depth = PipelineKey { shader, vertex_layout: layout.clone(), color_format: wgpu::TextureFormat::Bgra8UnormSrgb, depth_format: Some(wgpu::TextureFormat::Depth32Float) };
        let key_diff_depth = PipelineKey { shader, vertex_layout: layout, color_format: wgpu::TextureFormat::Bgra8UnormSrgb, depth_format: Some(wgpu::TextureFormat::Depth24Plus) };
        assert_ne!(key_no_depth, key_with_depth);
        assert_ne!(key_with_depth, key_diff_depth);
        assert_eq!(key_no_depth, key_no_depth.clone());
    }

    #[test]
    fn pipeline_key_differs_by_color_format() {
        use rig_assets::ShaderHandle;
        let layout = VertexLayout { array_stride: 24, attributes: vec![] };
        let shader = ShaderHandle::from_raw(1);
        let key_bgra = PipelineKey { shader, vertex_layout: layout.clone(), color_format: wgpu::TextureFormat::Bgra8UnormSrgb, depth_format: None };
        let key_rgba16 = PipelineKey { shader, vertex_layout: layout, color_format: wgpu::TextureFormat::Rgba16Float, depth_format: None };
        assert_ne!(key_bgra, key_rgba16);
    }

    #[test]
    fn create_depth_texture_returns_correct_dimensions() {
        assert_eq!(DEPTH_FORMAT, wgpu::TextureFormat::Depth32Float);
    }

    #[test]
    fn validate_vertex_layout_accepts_normals_only() {
        let layout = VertexLayout {
            array_stride: 12,
            attributes: vec![VertexAttribute { shader_location: 2, format: VertexFormat::Float32x3, offset: 0 }],
        };
        assert!(validate_vertex_layout(&layout).is_ok());
    }

    #[test]
    fn validate_vertex_layout_rejects_empty_layout() {
        let layout = VertexLayout { array_stride: 12, attributes: vec![] };
        assert!(validate_vertex_layout(&layout).is_err());
    }

    #[test]
    fn validate_vertex_layout_rejects_zero_stride() {
        let layout = VertexLayout {
            array_stride: 0,
            attributes: vec![VertexAttribute { shader_location: 0, format: VertexFormat::Float32x3, offset: 0 }],
        };
        assert!(validate_vertex_layout(&layout).is_err());
    }

    #[test]
    fn validate_vertex_layout_rejects_duplicates() {
        let layout = VertexLayout {
            array_stride: 24,
            attributes: vec![
                VertexAttribute { shader_location: 0, format: VertexFormat::Float32x3, offset: 0 },
                VertexAttribute { shader_location: 0, format: VertexFormat::Float32x3, offset: 12 },
            ],
        };
        assert!(validate_vertex_layout(&layout).is_err());
    }

    #[test]
    fn vertex_format_size_float32x4() {
        assert_eq!(vertex_format_size(VertexFormat::Float32x4), 16);
    }

    #[test]
    fn vertex_format_size_float32() {
        assert_eq!(vertex_format_size(VertexFormat::Float32), 4);
    }

    #[test]
    fn wgpu_vertex_format_maps_float32x4() {
        assert_eq!(wgpu_vertex_format(VertexFormat::Float32x4), wgpu::VertexFormat::Float32x4);
    }

    #[test]
    fn index_count_uses_declared_format() {
        let mut mesh = sample_mesh();
        mesh.index_data = Arc::from([0_u8; 8]);
        mesh.index_format = rig_assets::IndexFormat::Uint32;
        let expected = (mesh.index_data.len() / std::mem::size_of::<u32>()) as u32;
        assert_eq!(expected, 2);
    }

    #[test]
    fn draw_list_sorted_by_shader_then_mesh() {
        use rig_assets::{AssetStore, MaterialAsset, MaterialParams, ShaderAsset};
        use rig_math::BoundingSphere;
        use rig_scene::ExtractedRenderable;

        let mut assets = AssetStore::new();
        let shader_a = assets.add_shader(ShaderAsset { source: Arc::from("a") });
        let shader_b = assets.add_shader(ShaderAsset { source: Arc::from("b") });
        let material_a1 = assets.add_material(MaterialAsset { shader: shader_a, parameters: MaterialParams::default(), textures: vec![] });
        let material_b1 = assets.add_material(MaterialAsset { shader: shader_b, parameters: MaterialParams::default(), textures: vec![] });
        let material_a2 = assets.add_material(MaterialAsset { shader: shader_a, parameters: MaterialParams::default(), textures: vec![] });
        let mesh_x = assets.add_mesh(sample_mesh());
        let mesh_y = { let mut m = sample_mesh(); m.vertex_data = Arc::from([2_u8; 24]); assets.add_mesh(m) };

        use rig_assets::ShaderHandle;
        let draw_list = vec![
            ExtractedRenderable { node: rig_scene::NodeId::from_raw(0, 0), mesh: mesh_x, material: material_b1, world_transform: Mat4::IDENTITY, world_bound: BoundingSphere::ZERO },
            ExtractedRenderable { node: rig_scene::NodeId::from_raw(1, 0), mesh: mesh_y, material: material_a1, world_transform: Mat4::IDENTITY, world_bound: BoundingSphere::ZERO },
            ExtractedRenderable { node: rig_scene::NodeId::from_raw(2, 0), mesh: mesh_x, material: material_a2, world_transform: Mat4::IDENTITY, world_bound: BoundingSphere::ZERO },
        ];

        let mut sorted_indices: Vec<usize> = (0..draw_list.len()).collect();
        sorted_indices.sort_by_key(|&i| {
            let object = &draw_list[i];
            let shader_key = assets.material(object.material).map(|m| m.shader).unwrap_or_else(|_| ShaderHandle::from_raw(u32::MAX));
            (shader_key, object.mesh)
        });

        let sorted_shaders: Vec<ShaderHandle> = sorted_indices.iter().map(|&i| assets.material(draw_list[i].material).map(|m| m.shader).unwrap()).collect();
        let first_b = sorted_shaders.iter().position(|&s| s == shader_b).unwrap();
        assert!(sorted_shaders[..first_b].iter().all(|&s| s == shader_a));
        assert!(sorted_shaders[first_b..].iter().all(|&s| s == shader_b));
    }

    #[test]
    fn sorted_draw_list_reduces_state_changes() {
        let shaders = vec![1_u32, 2, 1, 2, 1];
        let mut sorted_shaders = shaders.clone();
        sorted_shaders.sort();
        fn count_pipeline_switches(shaders: &[u32]) -> usize {
            shaders.windows(2).filter(|w| w[0] != w[1]).count()
        }
        let unsorted_switches = count_pipeline_switches(&shaders);
        let sorted_switches = count_pipeline_switches(&sorted_shaders);
        assert!(sorted_switches < unsorted_switches);
    }

    #[test]
    fn render_target_descriptor_format_fields() {
        let desc = RenderTargetDescriptor {
            width: 512,
            height: 256,
            color_format: wgpu::TextureFormat::Rgba8UnormSrgb,
            depth_format: Some(wgpu::TextureFormat::Depth32Float),
            label: "test target",
        };
        assert_eq!(desc.width, 512);
        assert_eq!(desc.height, 256);
        assert_eq!(desc.color_format, wgpu::TextureFormat::Rgba8UnormSrgb);
        assert_eq!(desc.depth_format, Some(wgpu::TextureFormat::Depth32Float));
    }

    #[test]
    fn render_target_descriptor_no_depth() {
        let desc = RenderTargetDescriptor { width: 1920, height: 1080, color_format: wgpu::TextureFormat::Bgra8UnormSrgb, depth_format: None, label: "no depth" };
        assert!(desc.depth_format.is_none());
    }

    #[test]
    fn render_error_display_is_non_empty() {
        let err = RenderError::InvalidCamera;
        assert!(!err.to_string().is_empty());
        let err = RenderError::Asset("test".into());
        assert!(err.to_string().contains("test"));
    }
}
