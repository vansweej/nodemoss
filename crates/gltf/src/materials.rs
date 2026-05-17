//! glTF material adaptation — PBR metallic-roughness → 5-slot MaterialAsset.

use rig_assets::{
    AlphaMode, AssetStore, MaterialAsset, MaterialHandle, MaterialParams, SamplerHandle,
    ShaderHandle, TextureHandle,
};

/// Adapt all glTF materials, returning handles indexed by glTF material index.
pub(crate) fn adapt_materials(
    document: &gltf::Document,
    image_handles: &[TextureHandle],
    sampler_handles: &[SamplerHandle],
    default_sampler: SamplerHandle,
    shader: ShaderHandle,
    store: &mut AssetStore,
) -> Vec<MaterialHandle> {
    document
        .materials()
        .map(|material| {
            adapt_material(
                &material,
                image_handles,
                sampler_handles,
                default_sampler,
                shader,
                store,
            )
        })
        .collect()
}

/// Adapt a single glTF material into a `MaterialAsset` registered in `store`.
pub(crate) fn adapt_material(
    material: &gltf::Material<'_>,
    image_handles: &[TextureHandle],
    sampler_handles: &[SamplerHandle],
    default_sampler: SamplerHandle,
    shader: ShaderHandle,
    store: &mut AssetStore,
) -> MaterialHandle {
    let pbr = material.pbr_metallic_roughness();
    let base_color = pbr.base_color_factor();
    let emissive = material.emissive_factor();
    let params = MaterialParams {
        ambient: [0.04, 0.04, 0.04, 1.0],
        diffuse: base_color,
        specular: [1.0, 1.0, 1.0, 32.0],
        emissive: [emissive[0], emissive[1], emissive[2], 1.0],
        metallic: pbr.metallic_factor(),
        roughness: pbr.roughness_factor(),
        custom_flags: 0,
        triplanar_scale: MaterialParams::default().triplanar_scale,
    };

    let textures = vec![
        resolve_texture(
            pbr.base_color_texture().map(|info| info.texture()),
            image_handles,
            sampler_handles,
            default_sampler,
        ),
        resolve_texture(
            material.normal_texture().map(|info| info.texture()),
            image_handles,
            sampler_handles,
            default_sampler,
        ),
        resolve_texture(
            pbr.metallic_roughness_texture().map(|info| info.texture()),
            image_handles,
            sampler_handles,
            default_sampler,
        ),
        resolve_texture(
            material.occlusion_texture().map(|info| info.texture()),
            image_handles,
            sampler_handles,
            default_sampler,
        ),
        resolve_texture(
            material.emissive_texture().map(|info| info.texture()),
            image_handles,
            sampler_handles,
            default_sampler,
        ),
    ];

    let alpha_mode = match material.alpha_mode() {
        gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
        gltf::material::AlphaMode::Mask => AlphaMode::Mask {
            cutoff: material.alpha_cutoff().unwrap_or(0.5),
        },
        gltf::material::AlphaMode::Blend => AlphaMode::Blend,
    };
    let double_sided = material.double_sided();

    store.add_material(MaterialAsset {
        shader,
        parameters: params,
        textures,
        alpha_mode,
        double_sided,
    })
}

pub(crate) fn default_material(shader: ShaderHandle, store: &mut AssetStore) -> MaterialHandle {
    store.add_material(MaterialAsset::untextured(shader, MaterialParams::default()))
}

fn resolve_texture(
    texture: Option<gltf::Texture<'_>>,
    image_handles: &[TextureHandle],
    sampler_handles: &[SamplerHandle],
    default_sampler: SamplerHandle,
) -> Option<(TextureHandle, SamplerHandle)> {
    let texture = texture?;
    let image = image_handles.get(texture.source().index()).copied()?;
    let sampler = texture
        .sampler()
        .index()
        .and_then(|index| sampler_handles.get(index).copied())
        .unwrap_or(default_sampler);
    Some((image, sampler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_assets::{ShaderAsset, TextureAsset, TextureFormat};
    use std::sync::Arc;

    #[test]
    fn default_material_is_untextured() {
        let mut store = AssetStore::new();
        let shader = store.add_shader(ShaderAsset {
            source: Arc::from("shader"),
        });

        let material = default_material(shader, &mut store);

        assert!(store.material(material).unwrap().textures.is_empty());
    }

    #[test]
    fn missing_image_handle_returns_none() {
        let store = &mut AssetStore::new();
        let _texture = store.add_texture(TextureAsset {
            width: 1,
            height: 1,
            format: TextureFormat::Rgba8Unorm,
            data: Arc::from([255_u8, 255, 255, 255]),
        });
        let sampler = store.add_sampler(Default::default());

        assert_eq!(resolve_texture(None, &[], &[], sampler), None);
    }
}
