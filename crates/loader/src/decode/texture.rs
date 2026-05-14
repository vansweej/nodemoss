//! Image decoder.

use crate::{ColorSpace, DecodedImage, LoadError};

/// Decode image bytes into RGBA8 pixels.
pub fn decode_texture(bytes: &[u8], extension: &str) -> Result<DecodedImage, LoadError> {
    let image = if let Some(format) = image_format(extension) {
        image::load_from_memory_with_format(bytes, format)
    } else {
        image::load_from_memory(bytes)
    }
    .map_err(|err| LoadError::Decode(err.to_string()))?;
    let channels = image.color().channel_count();
    let rgba = image.to_rgba8();
    Ok(DecodedImage {
        width: rgba.width(),
        height: rgba.height(),
        channels,
        color_space: ColorSpace::Srgb,
        data: rgba.into_raw(),
    })
}

fn image_format(extension: &str) -> Option<image::ImageFormat> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some(image::ImageFormat::Png),
        "jpg" | "jpeg" => Some(image::ImageFormat::Jpeg),
        "tga" => Some(image::ImageFormat::Tga),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, RgbaImage};

    use super::*;

    fn image_bytes(format: ImageFormat) -> Vec<u8> {
        let image = RgbaImage::from_raw(1, 1, vec![255, 0, 0, 255]).unwrap();
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut cursor, format)
            .unwrap();
        cursor.into_inner()
    }

    #[test]
    fn decodes_png_to_rgba8() {
        let decoded = decode_texture(&image_bytes(ImageFormat::Png), "png").unwrap();

        assert_eq!(decoded.width, 1);
        assert_eq!(decoded.height, 1);
        assert_eq!(decoded.channels, 4);
        assert_eq!(decoded.data, vec![255, 0, 0, 255]);
    }

    #[test]
    fn decodes_tga_with_extension_hint() {
        let decoded = decode_texture(&image_bytes(ImageFormat::Tga), "tga").unwrap();

        assert_eq!(decoded.width, 1);
        assert_eq!(decoded.height, 1);
        assert_eq!(decoded.data, vec![255, 0, 0, 255]);
    }

    #[test]
    fn returns_decode_error_for_invalid_image() {
        assert!(matches!(
            decode_texture(b"not an image", "png"),
            Err(LoadError::Decode(_))
        ));
    }
}
