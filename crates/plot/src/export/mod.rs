//! Image and data export (Q137) and the clipboard image (Q07).
//!
//! * [`svg`] writes a laid-out scene as SVG;
//! * [`raster`] rasterises that SVG to PNG, JPEG or clipboard pixels;
//! * [`table`] collects the columns written by `xsim_waveform::export::csv`.

pub mod raster;
pub mod svg;
pub mod table;

/// Image formats for [`crate::WaveformViewer::export_image`], re-exported at
/// the crate root.
pub use crate::ImageFormat;

/// The file extension of `format`.
pub fn extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Svg => "svg",
    }
}

/// Encodes the SVG document `svg` in `format` at `width`×`height` pixels
/// (SVG is returned as is; it already carries its size).
pub fn encode(svg: &str, format: ImageFormat, width: u32, height: u32) -> Result<Vec<u8>, String> {
    match format {
        ImageFormat::Svg => Ok(svg.as_bytes().to_vec()),
        ImageFormat::Png => raster::png(svg, width, height),
        ImageFormat::Jpeg => raster::jpeg(svg, width, height),
    }
}

/// Writes `bytes` to `path`, with a message naming the file on failure.
pub fn write_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("Could not write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_match_formats() {
        assert_eq!(extension(ImageFormat::Png), "png");
        assert_eq!(extension(ImageFormat::Jpeg), "jpg");
        assert_eq!(extension(ImageFormat::Svg), "svg");
    }

    #[test]
    fn svg_passes_through() {
        let svg = "<svg/>";
        assert_eq!(encode(svg, ImageFormat::Svg, 1, 1).unwrap(), svg.as_bytes());
    }
}
