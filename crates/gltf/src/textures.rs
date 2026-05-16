//! glTF image and sampler adaptation.

use std::sync::Arc;

use rig_assets::{
    AddressMode, AssetStore, FilterMode, SamplerDescriptor, SamplerHandle, TextureAsset,
    TextureFormat, TextureHandle,
};

/// Adapt all glTF images into `TextureAsset` values registered in `store`.
pub(crate) fn adapt_images(
    images: &[gltf::image::Data],
    store: &mut AssetStore,
) -> Vec<TextureHandle> {
    images
        .iter()
        .map(|image| {
            let data = rgba8_data(image);
            store.add_texture(TextureAsset {
                width: image.width,
                height: image.height,
                format: TextureFormat::Rgba8Unorm,
                data: Arc::from(data),
            })
        })
        .collect()
}

/// Adapt all glTF samplers into `SamplerDescriptor` values registered in `store`.
pub(crate) fn adapt_samplers(
    document: &gltf::Document,
    store: &mut AssetStore,
) -> Vec<SamplerHandle> {
    document
        .samplers()
        .map(|sampler| store.add_sampler(adapt_sampler(sampler)))
        .collect()
}

/// Create a default linear clamp-to-edge sampler for implicit glTF samplers.
pub(crate) fn default_sampler(store: &mut AssetStore) -> SamplerHandle {
    store.add_sampler(SamplerDescriptor::default())
}

fn adapt_sampler(sampler: gltf::texture::Sampler<'_>) -> SamplerDescriptor {
    SamplerDescriptor {
        address_mode_u: map_wrap(sampler.wrap_s()),
        address_mode_v: map_wrap(sampler.wrap_t()),
        mag_filter: sampler
            .mag_filter()
            .map(map_mag_filter)
            .unwrap_or(FilterMode::Linear),
        min_filter: sampler
            .min_filter()
            .map(map_min_filter)
            .unwrap_or(FilterMode::Linear),
    }
}

fn map_wrap(mode: gltf::texture::WrappingMode) -> AddressMode {
    match mode {
        gltf::texture::WrappingMode::ClampToEdge => AddressMode::ClampToEdge,
        gltf::texture::WrappingMode::MirroredRepeat => AddressMode::MirrorRepeat,
        gltf::texture::WrappingMode::Repeat => AddressMode::Repeat,
    }
}

fn map_mag_filter(filter: gltf::texture::MagFilter) -> FilterMode {
    match filter {
        gltf::texture::MagFilter::Nearest => FilterMode::Nearest,
        gltf::texture::MagFilter::Linear => FilterMode::Linear,
    }
}

fn map_min_filter(filter: gltf::texture::MinFilter) -> FilterMode {
    match filter {
        gltf::texture::MinFilter::Nearest | gltf::texture::MinFilter::NearestMipmapNearest => {
            FilterMode::Nearest
        }
        gltf::texture::MinFilter::Linear
        | gltf::texture::MinFilter::NearestMipmapLinear
        | gltf::texture::MinFilter::LinearMipmapNearest
        | gltf::texture::MinFilter::LinearMipmapLinear => FilterMode::Linear,
    }
}

fn rgba8_data(image: &gltf::image::Data) -> Vec<u8> {
    match image.format {
        gltf::image::Format::R8 => image.pixels.iter().flat_map(|&r| [r, r, r, 255]).collect(),
        gltf::image::Format::R8G8 => image
            .pixels
            .chunks_exact(2)
            .flat_map(|px| [px[0], px[1], 0, 255])
            .collect(),
        gltf::image::Format::R8G8B8 => image
            .pixels
            .chunks_exact(3)
            .flat_map(|px| [px[0], px[1], px[2], 255])
            .collect(),
        gltf::image::Format::R8G8B8A8 => image.pixels.clone(),
        gltf::image::Format::R16 => image
            .pixels
            .chunks_exact(2)
            .flat_map(|px| [px[1], px[1], px[1], 255])
            .collect(),
        gltf::image::Format::R16G16 => image
            .pixels
            .chunks_exact(4)
            .flat_map(|px| [px[1], px[3], 0, 255])
            .collect(),
        gltf::image::Format::R16G16B16 => image
            .pixels
            .chunks_exact(6)
            .flat_map(|px| [px[1], px[3], px[5], 255])
            .collect(),
        gltf::image::Format::R16G16B16A16 => image
            .pixels
            .chunks_exact(8)
            .flat_map(|px| [px[1], px[3], px[5], px[7]])
            .collect(),
        gltf::image::Format::R32G32B32FLOAT => image
            .pixels
            .chunks_exact(12)
            .flat_map(|px| {
                [
                    float_channel(px, 0),
                    float_channel(px, 4),
                    float_channel(px, 8),
                    255,
                ]
            })
            .collect(),
        gltf::image::Format::R32G32B32A32FLOAT => image
            .pixels
            .chunks_exact(16)
            .flat_map(|px| {
                [
                    float_channel(px, 0),
                    float_channel(px, 4),
                    float_channel(px, 8),
                    float_channel(px, 12),
                ]
            })
            .collect(),
    }
}

fn float_channel(bytes: &[u8], start: usize) -> u8 {
    let value = f32::from_le_bytes([
        bytes[start],
        bytes[start + 1],
        bytes[start + 2],
        bytes[start + 3],
    ]);
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
