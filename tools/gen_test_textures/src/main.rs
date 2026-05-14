//! Generate binary texture assets for loading examples.

use std::fs;
use std::path::Path;

use anyhow::Result;
use image::{ImageBuffer, Rgb, RgbImage, Rgba, RgbaImage};

fn main() -> Result<()> {
    let output = Path::new("assets/textures");
    fs::create_dir_all(output)?;

    generate_checker(&output.join("checker.png"))?;
    generate_stripes(&output.join("stripes.jpg"))?;
    generate_gradient(&output.join("gradient.tga"))?;

    println!("generated textures in {}", output.display());
    Ok(())
}

fn generate_checker(path: &Path) -> Result<()> {
    let mut image: RgbaImage = ImageBuffer::new(64, 64);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let checker = ((x / 8) + (y / 8)) % 2 == 0;
        *pixel = if checker {
            Rgba([255, 160, 32, 255])
        } else {
            Rgba([32, 32, 40, 255])
        };
    }
    image.save(path)?;
    Ok(())
}

fn generate_stripes(path: &Path) -> Result<()> {
    let mut image: RgbImage = ImageBuffer::new(64, 64);
    for (x, _y, pixel) in image.enumerate_pixels_mut() {
        let stripe = (x / 8) % 2 == 0;
        *pixel = if stripe {
            Rgb([64, 180, 255])
        } else {
            Rgb([10, 40, 90])
        };
    }
    image.save(path)?;
    Ok(())
}

fn generate_gradient(path: &Path) -> Result<()> {
    let mut image: RgbaImage = ImageBuffer::new(64, 64);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = Rgba([x as u8 * 4, y as u8 * 4, 220, 255]);
    }
    image.save(path)?;
    Ok(())
}
