//! Rasterises exported SVG documents with resvg and encodes them as PNG or
//! JPEG (Q137), or wraps them for the clipboard (Q07).

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{self, fontdb};
use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

/// JPEG quality: high enough that thin trace lines show no ringing.
const JPEG_QUALITY: u8 = 92;
/// The largest raster export, in pixels per side.
pub const MAX_SIDE: u32 = 16_384;

/// Straight-alpha RGBA pixels, row by row.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba8 {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// System fonts, loaded once per process (loading takes a while).
fn fonts() -> Arc<fontdb::Database> {
    static FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            db.set_sans_serif_family("Arial");
            Arc::new(db)
        })
        .clone()
}

/// Renders `svg` scaled to exactly `width`×`height` pixels.
pub fn rasterize(svg: &str, width: u32, height: u32) -> Result<Rgba8, String> {
    check_size(width, height)?;
    let options = usvg::Options {
        font_family: "Arial".into(),
        fontdb: fonts(),
        ..usvg::Options::default()
    };
    let tree =
        usvg::Tree::from_str(svg, &options).map_err(|e| format!("Invalid plot image: {e}"))?;
    let mut pixmap = Pixmap::new(width, height).ok_or("The image is too large")?;
    let size = tree.size();
    let transform =
        Transform::from_scale(width as f32 / size.width(), height as f32 / size.height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(Rgba8 {
        width,
        height,
        data: unpremultiply(pixmap.take()),
    })
}

fn check_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!(
            "Image size {width}×{height} is not supported (1 to {MAX_SIDE} pixels per side)"
        ));
    }
    Ok(())
}

/// tiny-skia stores premultiplied alpha; image files and the clipboard want
/// straight alpha.
fn unpremultiply(mut data: Vec<u8>) -> Vec<u8> {
    for pixel in data.as_chunks_mut::<4>().0 {
        let alpha = pixel[3];
        if alpha != 0 && alpha != 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * 255 + u16::from(alpha) / 2) / u16::from(alpha))
                    .min(255) as u8;
            }
        }
    }
    data
}

/// `svg` as a PNG file.
pub fn png(svg: &str, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let image = rasterize(svg, width, height)?;
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(&image.data, width, height, ExtendedColorType::Rgba8)
        .map_err(|e| format!("Could not encode PNG: {e}"))?;
    Ok(bytes)
}

/// `svg` as a JPEG file (no alpha: the plot background is opaque).
pub fn jpeg(svg: &str, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let image = rasterize(svg, width, height)?;
    let rgb: Vec<u8> = image
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY)
        .write_image(&rgb, width, height, ExtendedColorType::Rgb8)
        .map_err(|e| format!("Could not encode JPEG: {e}"))?;
    Ok(bytes)
}

/// `svg` as an image for [`arboard::Clipboard::set_image`].
pub fn clipboard_image(
    svg: &str,
    width: u32,
    height: u32,
) -> Result<arboard::ImageData<'static>, String> {
    let image = rasterize(svg, width, height)?;
    Ok(arboard::ImageData {
        width: image.width as usize,
        height: image.height as usize,
        bytes: Cow::Owned(image.data),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##;

    #[test]
    fn rasterises_at_the_requested_size() {
        let image = rasterize(SQUARE, 30, 20).unwrap();
        assert_eq!((image.width, image.height), (30, 20));
        assert_eq!(image.data.len(), 30 * 20 * 4);
        assert_eq!(&image.data[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn rejects_empty_or_huge_images() {
        assert!(rasterize(SQUARE, 0, 10).is_err());
        assert!(rasterize(SQUARE, MAX_SIDE + 1, 10).is_err());
        assert!(rasterize("not svg", 10, 10).is_err());
    }

    #[test]
    fn unpremultiplies_translucent_pixels() {
        assert_eq!(unpremultiply(vec![64, 0, 0, 128]), vec![128, 0, 0, 128]);
        assert_eq!(unpremultiply(vec![0, 0, 0, 0]), vec![0, 0, 0, 0]);
    }

    #[test]
    fn png_and_jpeg_decode_to_the_same_size() {
        let png = image::load_from_memory(&png(SQUARE, 12, 7).unwrap()).unwrap();
        assert_eq!((png.width(), png.height()), (12, 7));
        let jpeg = image::load_from_memory(&jpeg(SQUARE, 12, 7).unwrap()).unwrap();
        assert_eq!((jpeg.width(), jpeg.height()), (12, 7));
        let pixel = jpeg.to_rgb8().get_pixel(6, 3).0;
        assert!(pixel[0] > 240 && pixel[1] < 20 && pixel[2] < 20);
    }

    #[test]
    fn clipboard_buffer_is_rgba() {
        let image = clipboard_image(SQUARE, 8, 5).unwrap();
        assert_eq!(image.bytes.len(), image.width * image.height * 4);
        assert_eq!((image.width, image.height), (8, 5));
    }
}
