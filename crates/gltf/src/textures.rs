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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba8_data_expands_r8_to_grayscale_rgba() {
        let image = image_data(gltf::image::Format::R8, &[0, 128, 255]);

        let data = rgba8_data(&image);

        assert_eq!(
            data,
            vec![0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255]
        );
    }

    #[test]
    fn rgba8_data_expands_rgb_to_rgba() {
        let image = image_data(gltf::image::Format::R8G8B8, &[10, 20, 30, 40, 50, 60]);

        let data = rgba8_data(&image);

        assert_eq!(data, vec![10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn rgba8_data_keeps_rgba_pixels_unchanged() {
        let image = image_data(gltf::image::Format::R8G8B8A8, &[1, 2, 3, 4, 5, 6, 7, 8]);

        let data = rgba8_data(&image);

        assert_eq!(data, image.pixels);
    }

    #[test]
    fn float_channel_clamps_and_scales_to_unorm8() {
        let bytes = [-1.0_f32, 0.0, 0.5, 1.0, 2.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();

        assert_eq!(float_channel(&bytes, 0), 0);
        assert_eq!(float_channel(&bytes, 4), 0);
        assert_eq!(float_channel(&bytes, 8), 128);
        assert_eq!(float_channel(&bytes, 12), 255);
        assert_eq!(float_channel(&bytes, 16), 255);
    }

    #[test]
    fn map_wrap_preserves_gltf_address_modes() {
        assert_eq!(
            map_wrap(gltf::texture::WrappingMode::ClampToEdge),
            AddressMode::ClampToEdge
        );
        assert_eq!(
            map_wrap(gltf::texture::WrappingMode::MirroredRepeat),
            AddressMode::MirrorRepeat
        );
        assert_eq!(
            map_wrap(gltf::texture::WrappingMode::Repeat),
            AddressMode::Repeat
        );
    }

    #[test]
    fn map_filters_collapse_mip_modes_to_nearest_or_linear() {
        assert_eq!(
            map_mag_filter(gltf::texture::MagFilter::Nearest),
            FilterMode::Nearest
        );
        assert_eq!(
            map_mag_filter(gltf::texture::MagFilter::Linear),
            FilterMode::Linear
        );
        assert_eq!(
            map_min_filter(gltf::texture::MinFilter::NearestMipmapNearest),
            FilterMode::Nearest
        );
        assert_eq!(
            map_min_filter(gltf::texture::MinFilter::LinearMipmapLinear),
            FilterMode::Linear
        );
    }

    #[test]
    fn adapt_images_registers_rgba8_texture_assets() {
        let mut store = AssetStore::new();
        let handles = adapt_images(
            &[image_data(gltf::image::Format::R8G8B8, &[1, 2, 3])],
            &mut store,
        );

        let texture = store.texture(handles[0]).expect("texture registered");
        assert_eq!(texture.width, 1);
        assert_eq!(texture.height, 1);
        assert_eq!(texture.format, TextureFormat::Rgba8Unorm);
        assert_eq!(texture.data.as_ref(), &[1, 2, 3, 255]);
    }

    fn image_data(format: gltf::image::Format, pixels: &[u8]) -> gltf::image::Data {
        gltf::image::Data {
            pixels: pixels.to_vec(),
            format,
            width: 1,
            height: (pixels.len() / format_bytes_per_pixel(format)) as u32,
        }
    }

    fn format_bytes_per_pixel(format: gltf::image::Format) -> usize {
        match format {
            gltf::image::Format::R8 => 1,
            gltf::image::Format::R8G8 => 2,
            gltf::image::Format::R8G8B8 => 3,
            gltf::image::Format::R8G8B8A8 => 4,
            gltf::image::Format::R16 => 2,
            gltf::image::Format::R16G16 => 4,
            gltf::image::Format::R16G16B16 => 6,
            gltf::image::Format::R16G16B16A16 => 8,
            gltf::image::Format::R32G32B32FLOAT => 12,
            gltf::image::Format::R32G32B32A32FLOAT => 16,
        }
    }
}
